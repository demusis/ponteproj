//! "Entrar com o ChatGPT": the Ponte uses the person's own ChatGPT plan
//! (Plus or Pro) for its AI actions, through the open-source/local flow that
//! OpenAI documents for third-party tools ("Sign in with ChatGPT" with plan
//! usage). Nothing here runs on import: the person signs in once and then
//! clicks to analyse or draft.
//!
//! Wire details follow developers.openai.com/siwc/token-sharing-open-source:
//! dynamic client registration (no secret, no API key), PKCE, loopback
//! callback on 127.0.0.1, ID token checked against OpenAI's JWKS, requests
//! to the public Responses API with `store: false` and `stream: true`.
use anyhow::{bail, ensure, Context, Result};
use base64::Engine;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
const DISCOVERY_URL: &str = "https://auth.openai.com/.well-known/openid-configuration";
/// Public API used for inference and for listing models.
pub const API_BASE: &str = "https://api.openai.com/v1";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets the app spend the person's plan allowance.
pub const SHARING_SCOPE: &str = "chatgpt.tokens.use.direct";
const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const AGENT_NAME: &str = "Ponte";
const CALLBACK_PATH: &str = "/auth/callback";
/// How long the person has to finish in the browser.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_GENERATED_BYTES: usize = 128 * 1024;
const MAX_ERROR_BYTES: u64 = 64 * 1024;

/// What the sign-in left on this computer. Stored sealed (DPAPI) inside the
/// AI configuration. No `Debug`: tokens must not reach logs.
#[derive(Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ChatGptSession {
    /// Client id issued by OpenAI on the first sign-in from this computer.
    pub client_id: String,
    /// E-mail from the ID token, shown in the configuration window.
    pub account_label: String,
    /// `sub` claim: stable identity of the account.
    pub subject: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Retained for `id_token_hint` on later sign-ins.
    pub id_token: String,
    /// RFC 3339 UTC.
    pub expires_at: String,
    pub scopes: Vec<String>,
}

impl ChatGptSession {
    pub fn signed_in(&self) -> bool {
        !self.client_id.is_empty() && !self.refresh_token.is_empty()
    }

    /// True when the person allowed the Ponte to use the plan allowance.
    pub fn sharing(&self) -> bool {
        self.scopes.iter().any(|scope| scope == SHARING_SCOPE)
    }

    pub fn expires_within(&self, margin: Duration) -> bool {
        match chrono::DateTime::parse_from_rfc3339(&self.expires_at) {
            Ok(at) => at.with_timezone(&chrono::Utc) <= chrono::Utc::now() + margin,
            Err(_) => true,
        }
    }

    /// The same account without anything usable as a credential; what is
    /// stored when the person chose not to keep the session.
    pub fn without_tokens(&self) -> Self {
        Self {
            client_id: self.client_id.clone(),
            account_label: self.account_label.clone(),
            subject: self.subject.clone(),
            ..Default::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        for (label, value, max) in [
            ("identificação do cliente", &self.client_id, 512),
            ("conta", &self.account_label, 512),
            ("identidade", &self.subject, 512),
            ("chave de acesso", &self.access_token, 64 * 1024),
            ("chave de renovação", &self.refresh_token, 64 * 1024),
            ("chave de identidade", &self.id_token, 64 * 1024),
            ("validade", &self.expires_at, 64),
        ] {
            ensure!(
                value.len() <= max && !value.chars().any(char::is_control),
                "Sessão do ChatGPT: dado inválido ({label})."
            );
        }
        ensure!(
            self.scopes.len() <= 32 && self.scopes.iter().all(|s| s.len() <= 128),
            "Sessão do ChatGPT: permissões inválidas."
        );
        Ok(())
    }
}

/// Shared by the window and the worker threads, so a token refreshed during
/// an analysis is the one saved afterwards and refreshes never race.
pub type SharedSession = Arc<Mutex<ChatGptSession>>;

fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn base64url_decode(text: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .context("Codificação inválida na resposta de identidade.")
}

fn random_token(bytes: usize) -> Result<String> {
    use ring::rand::SecureRandom;
    let mut buffer = vec![0_u8; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut buffer)
        .map_err(|_| anyhow::anyhow!("Não foi possível gerar valores aleatórios para o login."))?;
    Ok(base64url(&buffer))
}

/// S256 challenge of a PKCE verifier (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// New host identity for this installation, in the documented URN form.
pub fn new_host_id() -> String {
    format!("urn:uuid:{}", uuid::Uuid::new_v4())
}

fn http(timeout: Duration) -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Não foi possível preparar a conexão com a OpenAI.")
}

/// A sign-in attempt: the loopback listener is bound before the browser
/// opens, so the callback always has somewhere to land.
pub struct Login {
    listener: TcpListener,
    pub authorize_url: String,
    redirect_uri: String,
    host_id: String,
    state: String,
    nonce: String,
    verifier: String,
    /// Issued client id when re-authorizing; `None` on first registration.
    expected_client: Option<String>,
    /// Hints for a returning account (ID token, e-mail).
    hints: Option<(String, String)>,
}

/// Query of the authorization request, as documented for the open-source
/// flow: dynamic registration on the first sign-in, the issued client id
/// afterwards.
fn authorize_url(
    client_id: Option<&str>,
    host_id: &str,
    redirect_uri: &str,
    state: &str,
    nonce: &str,
    verifier: &str,
    hints: Option<&(String, String)>,
) -> Result<String> {
    let mut url = url::Url::parse(AUTHORIZE_URL)?;
    {
        let mut query = url.query_pairs_mut();
        match client_id {
            Some(client_id) => {
                query.append_pair("client_id", client_id);
            }
            None => {
                query.append_pair("client_id", DYNAMIC_CLIENT);
                query.append_pair("agent_name_hint", AGENT_NAME);
            }
        }
        query.append_pair("ext_agent_host_id", host_id);
        query.append_pair("response_type", "code");
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("scope", SCOPES);
        query.append_pair("resource", RESOURCE);
        query.append_pair("state", state);
        query.append_pair("nonce", nonce);
        query.append_pair("code_challenge_method", "S256");
        query.append_pair("code_challenge", &pkce_challenge(verifier));
        if let Some((id_token, email)) = hints.filter(|_| client_id.is_some()) {
            if !id_token.is_empty() {
                query.append_pair("id_token_hint", id_token);
            }
            if email.contains('@') {
                query.append_pair("login_hint", email);
            }
        }
    }
    Ok(url.to_string())
}

impl Login {
    /// Prepares a sign-in for this host. `previous` reuses the client id
    /// issued earlier (and hints the account) instead of registering again.
    pub fn start(host_id: &str, previous: Option<&ChatGptSession>) -> Result<Self> {
        ensure!(
            host_id.starts_with("urn:uuid:") && host_id.len() <= 128,
            "Identificação deste computador inválida."
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .context("Não foi possível abrir a porta local para receber a autorização.")?;
        let port = listener.local_addr()?.port();
        // Documented: only the port may vary; never "localhost".
        let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
        let state = random_token(32)?;
        let nonce = random_token(32)?;
        let verifier = random_token(32)?;
        let issued = previous
            .map(|s| s.client_id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        let hints = previous.map(|p| (p.id_token.clone(), p.account_label.clone()));
        let url = authorize_url(
            issued.as_deref(),
            host_id,
            &redirect_uri,
            &state,
            &nonce,
            &verifier,
            hints.as_ref(),
        )?;
        Ok(Self {
            listener,
            authorize_url: url,
            redirect_uri,
            host_id: host_id.to_owned(),
            state,
            nonce,
            verifier,
            expected_client: issued,
            hints,
        })
    }

    /// Waits for the browser to come back, then exchanges the code and checks
    /// the identity. Blocks: run it on its own thread. `cancel` stops the wait.
    /// `open_browser` is used when a second authorization is needed: a code
    /// issued together with the dynamic registration may be refused
    /// (`invalid_grant`), and the documented recovery is to authorize again
    /// with the client id just issued.
    pub fn wait(
        self,
        cancel: &AtomicBool,
        timeout: Duration,
        open_browser: fn(&str) -> Result<()>,
    ) -> Result<ChatGptSession> {
        self.wait_keeping(cancel, timeout, open_browser, &mut None)
    }

    /// `wait`, telling in `issued` the client id OpenAI issued on the way,
    /// even when the sign-in then fails. The caller must keep it: OpenAI
    /// binds the registration of this computer to that id, and a new
    /// registration for the same computer is refused in the browser
    /// ("Agent registration session changed; restart sign-in").
    pub fn wait_keeping(
        mut self,
        cancel: &AtomicBool,
        timeout: Duration,
        open_browser: fn(&str) -> Result<()>,
        issued: &mut Option<String>,
    ) -> Result<ChatGptSession> {
        self.listener.set_nonblocking(true)?;
        let mut callback = self.receive_callback(cancel, timeout)?;
        if self.expected_client.is_none() && !callback.client_id.trim().is_empty() {
            *issued = Some(callback.client_id.clone());
        }
        let client = http(Duration::from_secs(30))?;
        let mut tokens = exchange_code(
            &client,
            &callback.client_id,
            &callback.code,
            &self.verifier,
            &self.redirect_uri,
        );
        if let Err(error) = &tokens {
            let refused_registration_code = self.expected_client.is_none()
                && error
                    .downcast_ref::<TokenError>()
                    .is_some_and(|t| t.code == "invalid_grant");
            if refused_registration_code {
                // Registration done; authorize once more with the issued id
                // (fresh state, nonce and verifier, same local port).
                self.expected_client = Some(callback.client_id.clone());
                self.state = random_token(32)?;
                self.nonce = random_token(32)?;
                self.verifier = random_token(32)?;
                self.authorize_url = authorize_url(
                    self.expected_client.as_deref(),
                    &self.host_id,
                    &self.redirect_uri,
                    &self.state,
                    &self.nonce,
                    &self.verifier,
                    self.hints.as_ref(),
                )?;
                open_browser(&self.authorize_url)?;
                callback = self.receive_callback(cancel, timeout)?;
                tokens = exchange_code(
                    &client,
                    &callback.client_id,
                    &callback.code,
                    &self.verifier,
                    &self.redirect_uri,
                );
            }
        }
        let tokens = tokens.map_err(|error| {
            anyhow::anyhow!("{error} Clique em “Entrar com o ChatGPT…” para tentar de novo.")
        })?;
        self.finish(callback, tokens, &client)
    }

    fn receive_callback(&self, cancel: &AtomicBool, timeout: Duration) -> Result<Callback> {
        let started = Instant::now();
        loop {
            ensure!(!cancel.load(Ordering::Relaxed), "Login cancelado.");
            ensure!(
                started.elapsed() < timeout,
                "O navegador não voltou com a autorização em {} minutos. Clique em “Entrar com o ChatGPT…” para tentar de novo.",
                timeout.as_secs() / 60
            );
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                    let head = read_request_head(&mut stream)?;
                    let Some(target) = request_target(&head) else {
                        reply_http(&mut stream, 400, "Pedido não reconhecido.");
                        continue;
                    };
                    let (path, query) = target.split_once('?').unwrap_or((target, ""));
                    if path != CALLBACK_PATH {
                        // Browsers also ask for /favicon.ico and the like.
                        reply_http(&mut stream, 404, "Nada aqui.");
                        continue;
                    }
                    let parsed =
                        parse_callback(query, &self.state, self.expected_client.as_deref());
                    match &parsed {
                        Ok(_) => reply_http(&mut stream, 200, "Pronto. A autorização chegou ao Ponte; você já pode fechar esta aba e voltar ao programa."),
                        Err(error) => reply_http(&mut stream, 400, &format!("A autorização não foi concluída: {error} Volte ao Ponte e tente de novo.")),
                    }
                    return parsed;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(150));
                }
                Err(error) => {
                    return Err(error).context("Falha ao receber a autorização do navegador.")
                }
            }
        }
    }

    fn finish(
        self,
        callback: Callback,
        tokens: TokenResponse,
        client: &Client,
    ) -> Result<ChatGptSession> {
        let id_token = tokens
            .id_token
            .clone()
            .filter(|t| !t.is_empty())
            .context("A OpenAI não devolveu a identidade da conta.")?;
        let claims = verify_id_token(client, &id_token, &callback.client_id, &self.nonce)?;
        let scopes = if tokens
            .scope
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            split_scopes(tokens.scope.as_deref().unwrap_or(""))
        } else {
            callback.scopes
        };
        let session = ChatGptSession {
            client_id: callback.client_id,
            account_label: if claims.email.is_empty() {
                "conta do ChatGPT".into()
            } else {
                claims.email
            },
            subject: claims.sub,
            access_token: tokens.access_token,
            refresh_token: tokens
                .refresh_token
                .filter(|t| !t.is_empty())
                .context("A OpenAI não devolveu a chave de renovação; entre de novo.")?,
            id_token,
            expires_at: expires_at(tokens.expires_in),
            scopes,
        };
        session.validate()?;
        Ok(session)
    }
}

fn read_request_head(stream: &mut std::net::TcpStream) -> Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let count = stream.read(&mut chunk)?;
        ensure!(count > 0, "Conexão do navegador encerrada antes do pedido.");
        buffer.extend_from_slice(&chunk[..count]);
        if buffer.windows(4).any(|w| w == b"\r\n\r\n") || buffer.len() > 16 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// "GET /auth/callback?code=… HTTP/1.1" → "/auth/callback?code=…".
fn request_target(head: &str) -> Option<&str> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    (method == "GET").then_some(target)
}

fn reply_http(stream: &mut std::net::TcpStream, status: u16, text: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let body = format!(
        "<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><title>Ponte</title></head><body style=\"font-family:system-ui;margin:3em;max-width:40em\"><h1 style=\"color:#185461\">Ponte</h1><p>{}</p></body></html>",
        html_escape(text)
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct Callback {
    code: String,
    client_id: String,
    scopes: Vec<String>,
}

/// Checks the query string of the callback: state, denial, issued client id.
fn parse_callback(query: &str, state: &str, expected_client: Option<&str>) -> Result<Callback> {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    if let Some(error) = get("error") {
        if error == "access_denied" {
            bail!("você não autorizou o Ponte na página do ChatGPT.");
        }
        let description = get("error_description").unwrap_or("");
        bail!(
            "a OpenAI recusou o pedido ({}).",
            clean(
                if description.is_empty() {
                    error
                } else {
                    description
                },
                200
            )
        );
    }
    ensure!(
        get("state") == Some(state),
        "a resposta não corresponde a este pedido de login."
    );
    let code = get("code")
        .filter(|c| !c.is_empty())
        .context("a resposta veio sem o código de autorização.")?;
    let client_id = match (expected_client, get("client_id")) {
        (Some(expected), Some(given)) => {
            ensure!(
                given == expected,
                "a resposta veio para outro cliente registrado."
            );
            expected.to_owned()
        }
        (Some(expected), None) => expected.to_owned(),
        (None, Some(given)) if !given.is_empty() && given != DYNAMIC_CLIENT => given.to_owned(),
        (None, _) => bail!("o registro do Ponte na OpenAI não foi concluído."),
    };
    ensure!(
        client_id.len() <= 512
            && !client_id
                .chars()
                .any(|c| c.is_control() || c.is_whitespace()),
        "identificação do cliente inválida."
    );
    Ok(Callback {
        code: code.to_owned(),
        client_id,
        scopes: split_scopes(get("scope").unwrap_or("")),
    })
}

/// Scopes come space-separated in token responses and "+"-separated in the
/// callback query (already decoded to spaces by form parsing).
fn split_scopes(text: &str) -> Vec<String> {
    text.split([' ', '+'])
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

fn expires_at(expires_in: Option<u64>) -> String {
    let seconds = expires_in.unwrap_or(3600).min(30 * 24 * 3600) as i64;
    (chrono::Utc::now() + chrono::Duration::seconds(seconds))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

fn token_request(client: &Client, form: &[(&str, &str)]) -> Result<TokenResponse> {
    // A connection that could not even be opened never delivered the code,
    // so trying again is safe; anything later is not repeated (the code is
    // good for one use).
    let mut tries = 0;
    let response = loop {
        tries += 1;
        match client
            .post(TOKEN_URL)
            .header("Accept", "application/json")
            .form(form)
            .send()
        {
            Ok(response) => break response,
            Err(error) if error.is_connect() && tries < 3 => {
                std::thread::sleep(Duration::from_secs(2 * tries));
            }
            Err(error) => {
                bail!(if error.is_timeout() {
                    "A OpenAI demorou demais para responder ao login."
                } else if error.is_connect() {
                    "O Ponte não conseguiu abrir a conexão com a OpenAI (auth.openai.com) para concluir o login, em três tentativas. Verifique a internet e se um antivírus, firewall ou proxy está bloqueando o Ponte."
                } else {
                    "A conexão com a OpenAI caiu durante o login. Verifique a internet."
                });
            }
        }
    };
    let status = response.status();
    let body = read_limited(response, MAX_ERROR_BYTES)?;
    if !status.is_success() {
        let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let code = value["error"].as_str().unwrap_or("").to_owned();
        let description = value["error_description"].as_str().unwrap_or("").to_owned();
        bail!(TokenError {
            code: code.clone(),
            message: format!(
                "A OpenAI recusou o pedido de acesso (código {}{}).",
                status.as_u16(),
                if description.is_empty() && code.is_empty() {
                    String::new()
                } else {
                    format!(
                        ": {}",
                        clean(
                            if description.is_empty() {
                                &code
                            } else {
                                &description
                            },
                            200
                        )
                    )
                }
            ),
        });
    }
    let tokens: TokenResponse = serde_json::from_slice(&body)
        .context("Resposta de acesso da OpenAI em formato inesperado.")?;
    ensure!(
        !tokens.access_token.is_empty(),
        "A OpenAI devolveu uma chave de acesso vazia."
    );
    Ok(tokens)
}

/// Error of the token endpoint, with the OAuth code when there is one.
#[derive(Debug)]
pub struct TokenError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TokenError {}

impl TokenError {
    /// Codes that mean the stored refresh token is gone for good.
    pub fn signed_out(&self) -> bool {
        matches!(
            self.code.as_str(),
            "invalid_grant"
                | "invalid_refresh_token"
                | "token_expired"
                | "refresh_token_expired"
                | "refresh_token_invalidated"
                | "refresh_token_reused"
        )
    }
}

fn exchange_code(
    client: &Client,
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    token_request(
        client,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
            ("resource", RESOURCE),
        ],
    )
}

/// Standard refresh; the refresh token rotates, so the caller must store the
/// returned session at once.
pub fn refresh(session: &ChatGptSession) -> Result<ChatGptSession> {
    ensure!(
        session.signed_in(),
        "Entre com a sua conta do ChatGPT em Ajustes ▸ Inteligência artificial."
    );
    let client = http(Duration::from_secs(30))?;
    let tokens = token_request(
        &client,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &session.client_id),
            ("refresh_token", &session.refresh_token),
            ("resource", RESOURCE),
        ],
    )?;
    let mut renewed = session.clone();
    renewed.access_token = tokens.access_token;
    if let Some(token) = tokens.refresh_token.filter(|t| !t.is_empty()) {
        renewed.refresh_token = token;
    }
    if let Some(token) = tokens.id_token.filter(|t| !t.is_empty()) {
        renewed.id_token = token;
    }
    if let Some(scope) = tokens.scope.filter(|s| !s.trim().is_empty()) {
        renewed.scopes = split_scopes(&scope);
    }
    renewed.expires_at = expires_at(tokens.expires_in);
    renewed.validate()?;
    Ok(renewed)
}

/// Access token good for a call, refreshing (under the lock) when it is
/// about to expire. A dead refresh token clears the session and says so.
pub fn access_token(shared: &SharedSession) -> Result<String> {
    let mut session = shared
        .lock()
        .map_err(|_| anyhow::anyhow!("Sessão do ChatGPT indisponível."))?;
    ensure!(
        session.signed_in(),
        "Entre com a sua conta do ChatGPT em Ajustes ▸ Inteligência artificial."
    );
    ensure!(
        session.sharing(),
        "Sua conta do ChatGPT entrou, mas não liberou o uso do plano para o Ponte. Em Ajustes ▸ Inteligência artificial, clique em “Entrar de novo…” e marque a permissão de uso do plano."
    );
    if session.access_token.is_empty() || session.expires_within(REFRESH_MARGIN) {
        match refresh(&session) {
            Ok(renewed) => *session = renewed,
            Err(error) => {
                if error
                    .downcast_ref::<TokenError>()
                    .is_some_and(TokenError::signed_out)
                {
                    *session = session.without_tokens();
                    bail!("A sessão do ChatGPT foi encerrada (desconectada nas configurações do ChatGPT ou expirada). Entre de novo em Ajustes ▸ Inteligência artificial.");
                }
                return Err(error.context("Não foi possível renovar a sessão do ChatGPT"));
            }
        }
    }
    Ok(session.access_token.clone())
}

/// Revokes the refresh token at OpenAI. Returns Ok even when the token was
/// already invalid; the caller clears the local tokens either way.
pub fn sign_out(session: &ChatGptSession) -> Result<()> {
    if !session.signed_in() {
        return Ok(());
    }
    let client = http(Duration::from_secs(20))?;
    let discovery = discovery(&client)?;
    let endpoint = discovery["revocation_endpoint"]
        .as_str()
        .filter(|u| u.starts_with("https://auth.openai.com/"))
        .context("A OpenAI não publicou o endereço para encerrar a sessão.")?;
    let response = client
        .post(endpoint)
        .form(&[
            ("token", session.refresh_token.as_str()),
            ("token_type_hint", "refresh_token"),
            ("client_id", session.client_id.as_str()),
        ])
        .send()
        .context("Não foi possível falar com a OpenAI para encerrar a sessão.")?;
    ensure!(
        response.status().is_success(),
        "A OpenAI respondeu com erro (código {}) ao encerrar a sessão. Desconecte o Ponte nas configurações do ChatGPT.",
        response.status().as_u16()
    );
    Ok(())
}

fn discovery(client: &Client) -> Result<Value> {
    let response = client
        .get(DISCOVERY_URL)
        .send()
        .context("Não foi possível obter a configuração de identidade da OpenAI.")?;
    ensure!(
        response.status().is_success(),
        "A OpenAI respondeu com erro (código {}) ao consultar a configuração de identidade.",
        response.status().as_u16()
    );
    let body = read_limited(response, MAX_ERROR_BYTES)?;
    serde_json::from_slice(&body)
        .context("Configuração de identidade da OpenAI em formato inesperado.")
}

fn read_limited(response: reqwest::blocking::Response, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .take(max + 1)
        .read_to_end(&mut bytes)
        .context("Não foi possível ler a resposta da OpenAI.")?;
    ensure!(
        bytes.len() as u64 <= max,
        "Resposta da OpenAI grande demais."
    );
    Ok(bytes)
}

/// Claims kept from a verified ID token.
pub struct IdClaims {
    pub sub: String,
    pub email: String,
}

/// RS256 signature against the published JWKS, then issuer, audience,
/// expiry and nonce.
fn verify_id_token(client: &Client, token: &str, client_id: &str, nonce: &str) -> Result<IdClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    ensure!(
        parts.len() == 3,
        "Identidade da conta em formato inesperado."
    );
    let header: Value = serde_json::from_slice(&base64url_decode(parts[0])?)
        .context("Cabeçalho da identidade inválido.")?;
    ensure!(
        header["alg"].as_str() == Some("RS256"),
        "A identidade usa um algoritmo de assinatura não suportado."
    );
    let kid = header["kid"].as_str().unwrap_or("");
    let jwks_uri = discovery(client)?["jwks_uri"]
        .as_str()
        .filter(|u| u.starts_with("https://auth.openai.com/"))
        .context("A OpenAI não publicou as chaves de assinatura.")?
        .to_owned();
    let response = client
        .get(&jwks_uri)
        .send()
        .context("Não foi possível obter as chaves de assinatura da OpenAI.")?;
    ensure!(
        response.status().is_success(),
        "A OpenAI respondeu com erro (código {}) ao consultar as chaves de assinatura.",
        response.status().as_u16()
    );
    let jwks: Value = serde_json::from_slice(&read_limited(response, MAX_ERROR_BYTES)?)
        .context("Chaves de assinatura em formato inesperado.")?;
    let key = jwks["keys"]
        .as_array()
        .and_then(|keys| {
            keys.iter().find(|k| {
                k["kty"].as_str() == Some("RSA")
                    && (kid.is_empty() || k["kid"].as_str() == Some(kid))
            })
        })
        .context("A chave que assinou a identidade não está publicada pela OpenAI.")?;
    let n = base64url_decode(key["n"].as_str().unwrap_or(""))?;
    let e = base64url_decode(key["e"].as_str().unwrap_or(""))?;
    let signature = base64url_decode(parts[2])?;
    let message = format!("{}.{}", parts[0], parts[1]);
    ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            message.as_bytes(),
            &signature,
        )
        .map_err(|_| anyhow::anyhow!("A assinatura da identidade da conta não confere."))?;
    let claims: Value = serde_json::from_slice(&base64url_decode(parts[1])?)
        .context("Identidade da conta ilegível.")?;
    check_claims(&claims, client_id, nonce, chrono::Utc::now().timestamp())
}

fn check_claims(claims: &Value, client_id: &str, nonce: &str, now: i64) -> Result<IdClaims> {
    ensure!(
        claims["iss"].as_str() == Some(ISSUER),
        "A identidade não foi emitida pela OpenAI."
    );
    let audience_ok = match &claims["aud"] {
        Value::String(aud) => aud == client_id,
        Value::Array(list) => list.iter().any(|a| a.as_str() == Some(client_id)),
        _ => false,
    };
    ensure!(
        audience_ok,
        "A identidade foi emitida para outro aplicativo."
    );
    let exp = claims["exp"].as_i64().context("Identidade sem validade.")?;
    ensure!(
        exp + 60 > now,
        "A identidade recebida já expirou; entre de novo."
    );
    ensure!(
        claims["nonce"].as_str() == Some(nonce),
        "A identidade não corresponde a este pedido de login."
    );
    let sub = claims["sub"].as_str().unwrap_or("").to_owned();
    ensure!(!sub.is_empty(), "Identidade sem identificação da conta.");
    Ok(IdClaims {
        sub: clean(&sub, 512),
        email: clean(claims["email"].as_str().unwrap_or(""), 512),
    })
}

/// Models the signed-in account may use (`GET /v1/models`), in the server's
/// order, keeping only those meant to be listed.
pub fn list_models(shared: &SharedSession) -> Result<Vec<crate::ai::ModelInfo>> {
    let token = access_token(shared)?;
    list_models_at(API_BASE, &token)
}

fn list_models_at(base: &str, token: &str) -> Result<Vec<crate::ai::ModelInfo>> {
    let client = http(Duration::from_secs(30))?;
    let response = client
        .get(format!("{}/models", base.trim_end_matches('/')))
        .bearer_auth(token)
        .send()
        .map_err(|error| network_error(&error, "listar os modelos"))?;
    let status = response.status();
    let body = read_limited(response, MAX_RESPONSE_BYTES)?;
    if !status.is_success() {
        return Err(api_error(status, &body));
    }
    parse_models(&body)
}

/// The catalogue comes in two shapes: the plan-sharing flow answers with a
/// `models` array of `{slug, display_name, visibility}` (OpenAI's Sign in
/// with ChatGPT documentation), the plain API with `data` of `{id}`.
fn parse_models(bytes: &[u8]) -> Result<Vec<crate::ai::ModelInfo>> {
    let value: Value =
        serde_json::from_slice(bytes).context("Lista de modelos em formato inesperado.")?;
    let entries = value["models"]
        .as_array()
        .or_else(|| value["data"].as_array())
        .with_context(|| {
            let keys = value
                .as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            format!("A lista de modelos veio sem os dados esperados (campos recebidos: {keys}).")
        })?;
    let models = entries
        .iter()
        .filter(|m| m["visibility"].as_str().is_none_or(|v| v == "list"))
        .filter_map(|m| m["slug"].as_str().or_else(|| m["id"].as_str()))
        .map(|id| clean(id, 128))
        .filter(|id| !id.is_empty())
        .map(|name| crate::ai::ModelInfo {
            name,
            parameter_size: String::new(),
        })
        .collect();
    Ok(models)
}

fn network_error(error: &reqwest::Error, action: &str) -> anyhow::Error {
    if error.is_timeout() {
        anyhow::anyhow!("O ChatGPT demorou demais para {action}. Tente de novo.")
    } else if error.is_connect() {
        anyhow::anyhow!("Não foi possível conectar ao ChatGPT para {action}. Verifique a conexão com a internet.")
    } else {
        anyhow::anyhow!("A comunicação com o ChatGPT falhou ao {action}.")
    }
}

/// User-facing message for a non-success HTTP answer of the API.
fn api_error(status: reqwest::StatusCode, body: &[u8]) -> anyhow::Error {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let code = value["error"]["code"]
        .as_str()
        .or_else(|| value["error"]["type"].as_str())
        .unwrap_or("");
    let message = value["error"]["message"]
        .as_str()
        .or_else(|| value["detail"].as_str())
        .unwrap_or("");
    let param = value["error"]["param"].as_str().unwrap_or("");
    anyhow::anyhow!("{}", explain(status.as_u16(), code, message, param))
}

/// A provider code or field name that is safe to show: short and made only
/// of the characters such identifiers use. Anything else is left out.
fn ident(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()
        && text.len() <= 60
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
    .then_some(text)
}

/// Maps the documented plan-usage errors to what the person can do. What the
/// provider wrote in its `message` is never shown (it may repeat what was
/// sent): only the status, the stable code and the name of the field it
/// objected to, each when it looks like an identifier.
fn explain(status: u16, code: &str, _message: &str, param: &str) -> String {
    match (code, status) {
        ("subscription_sharing_usage_limit_exceeded", _) | (_, 429) => {
            "A cota do seu plano ChatGPT para o Ponte acabou por enquanto (limite semanal, ou das últimas horas no plano Plus). Veja e ajuste em ChatGPT ▸ Configurações ▸ Uso, ou escolha outro provedor em Ajustes ▸ Inteligência artificial. Nada foi enviado a outro provedor.".into()
        }
        ("subscription_sharing_user_not_eligible", _) => {
            "Seu plano do ChatGPT não permite uso por aplicativos: é preciso Plus ou Pro (ou, em conta de trabalho, liberação do administrador).".into()
        }
        ("subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable", _)
        | (_, 503) => {
            "O ChatGPT não conseguiu conferir a sua cota agora. Tente de novo em alguns minutos; a sessão continua válida.".into()
        }
        ("subscription_sharing_unsupported_capability", _) => format!(
            "O ChatGPT recusou um recurso deste pedido{}. Informe o problema aos autores do Ponte.",
            ident(param).map_or(String::new(), |p| format!(" ({p})"))
        ),
        ("subscription_sharing_route_not_supported", _) => {
            "O ChatGPT não aceitou este tipo de acesso para uso do plano. Informe o problema aos autores do Ponte.".into()
        }
        ("subscription_sharing_invalid_user", _) | (_, 401) => {
            "A sessão do ChatGPT não foi aceita. Entre de novo em Ajustes ▸ Inteligência artificial.".into()
        }
        (_, 403) => "O ChatGPT bloqueou o pedido (política ou região da conta).".into(),
        (_, 0) => format!(
            "O ChatGPT interrompeu a resposta{}.",
            ident(code).map_or(String::new(), |c| format!(" ({c})"))
        ),
        _ => format!("O ChatGPT respondeu com erro (código {status})."),
    }
}

/// One answer from the Responses API, streamed as the documented flow
/// requires. `instructions` is the Ponte's system text; `input` is the
/// untrusted document data. A failure that may pass is tried again, the
/// call can be cancelled at any moment (also before the first byte), and a
/// strict schema the server refuses is sent once more as a plain one.
pub fn respond(
    shared: &SharedSession,
    model: &str,
    instructions: &str,
    input: &str,
    schema: Option<Value>,
    reporter: &crate::ai::Reporter,
    ledger: &crate::ai::Ledger,
) -> Result<String> {
    reporter.check()?;
    let token = access_token(shared)?;
    respond_at(
        API_BASE,
        &token,
        model,
        instructions,
        input,
        schema,
        reporter,
        ledger,
    )
}

#[allow(clippy::too_many_arguments)]
fn respond_at(
    base: &str,
    token: &str,
    model: &str,
    instructions: &str,
    input: &str,
    schema: Option<Value>,
    reporter: &crate::ai::Reporter,
    ledger: &crate::ai::Ledger,
) -> Result<String> {
    respond_at_with(
        base,
        token,
        model,
        instructions,
        input,
        schema,
        reporter,
        ledger,
        &crate::ai::Timing::standard(),
    )
}

/// The server refused the strict form of the schema.
#[derive(Debug)]
struct SchemaRefused;

impl std::fmt::Display for SchemaRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("O ChatGPT não aceitou o formato rígido da resposta.")
    }
}

impl std::error::Error for SchemaRefused {}

/// Servers (by base address) that refused a strict schema: they are not
/// asked again in this session.
fn strict_refused_bases() -> &'static Mutex<std::collections::BTreeSet<String>> {
    static BASES: std::sync::OnceLock<Mutex<std::collections::BTreeSet<String>>> =
        std::sync::OnceLock::new();
    BASES.get_or_init(Default::default)
}

fn strict_refused(base: &str) -> bool {
    strict_refused_bases()
        .lock()
        .is_ok_and(|bases| bases.contains(base))
}

/// Whether `schema` fits what the Responses API accepts with
/// `strict: true`: an object at the root; every object lists all its
/// properties in `required` and says `additionalProperties: false` (an
/// optional value is a type array with `"null"`); only the keywords the
/// strict mode knows (`type`, `enum`, `const`, `properties`, `required`,
/// `additionalProperties`, `items`, `minItems`, `maxItems`, `anyOf`,
/// `description`, `title`); at most five levels of objects and arrays and a
/// hundred properties. `minLength`, `maxLength`, `pattern`, `format` and the
/// numeric bounds are not in that list on purpose: a schema that uses one is
/// sent as a plain (non-strict) schema, and the program's own check enforces
/// the limit. The role schemas are written to pass this.
pub fn strict_ok(schema: &Value) -> bool {
    let mut properties = 0;
    schema["type"] == "object" && strict_node(schema, 1, &mut properties) && properties <= 100
}

fn strict_node(node: &Value, depth: usize, properties: &mut usize) -> bool {
    let Some(map) = node.as_object() else {
        return false;
    };
    const KEYWORDS: [&str; 10] = [
        "type",
        "enum",
        "const",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "minItems",
        "maxItems",
        "anyOf",
    ];
    if map
        .keys()
        .any(|key| !KEYWORDS.contains(&key.as_str()) && key != "description" && key != "title")
    {
        return false;
    }
    let kinds: Vec<&str> = match map.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => {
            let names: Vec<&str> = kinds.iter().filter_map(Value::as_str).collect();
            if names.len() != kinds.len() {
                return false;
            }
            names
        }
        None => Vec::new(),
        Some(_) => return false,
    };
    if kinds.iter().any(|kind| {
        ![
            "object", "array", "string", "number", "integer", "boolean", "null",
        ]
        .contains(kind)
    }) {
        return false;
    }
    if map.get("type").is_none()
        && !(map.contains_key("enum") || map.contains_key("const") || map.contains_key("anyOf"))
    {
        return false;
    }
    if let Some(variants) = map.get("anyOf") {
        let Some(variants) = variants.as_array() else {
            return false;
        };
        if variants.is_empty()
            || !variants
                .iter()
                .all(|variant| strict_node(variant, depth, properties))
        {
            return false;
        }
    }
    if kinds.contains(&"object") {
        if depth > 5 || map.get("additionalProperties") != Some(&Value::Bool(false)) {
            return false;
        }
        let Some(declared) = map.get("properties").and_then(Value::as_object) else {
            return false;
        };
        let Some(required) = map.get("required").and_then(Value::as_array) else {
            return false;
        };
        let mut required: Vec<&str> = required.iter().filter_map(Value::as_str).collect();
        required.sort_unstable();
        required.dedup();
        let mut names: Vec<&str> = declared.keys().map(String::as_str).collect();
        names.sort_unstable();
        if required != names {
            return false;
        }
        *properties += declared.len();
        if !declared
            .values()
            .all(|value| strict_node(value, depth + 1, properties))
        {
            return false;
        }
    }
    if kinds.contains(&"array") {
        if depth > 5 {
            return false;
        }
        match map.get("items") {
            Some(items) if strict_node(items, depth + 1, properties) => {}
            _ => return false,
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn respond_at_with(
    base: &str,
    token: &str,
    model: &str,
    instructions: &str,
    input: &str,
    schema: Option<Value>,
    reporter: &crate::ai::Reporter,
    ledger: &crate::ai::Ledger,
    timing: &crate::ai::Timing,
) -> Result<String> {
    ensure!(
        !model.trim().is_empty() && model.len() <= 128 && !model.chars().any(char::is_control),
        "Escolha o modelo do ChatGPT em Ajustes ▸ Inteligência artificial."
    );
    reporter.check()?;
    let mut strict =
        schema.as_ref().is_some_and(strict_ok) && !strict_refused(base) && !ledger.strict_dropped();
    let mut was_strict = false;
    loop {
        let call = Exchange {
            url: format!("{}/responses", base.trim_end_matches('/')),
            token: token.to_owned(),
            body: request_body(model, instructions, input, schema.as_ref(), strict),
            strict,
            limit: timing.chatgpt_timeout,
            ledger: ledger.clone(),
        };
        let result = crate::ai::with_retries(reporter, timing, || {
            let call = call.clone();
            crate::ai::run_cancellable(reporter, move |inner| exchange(call, inner))
        });
        match result {
            Err(error) if strict && error.downcast_ref::<SchemaRefused>().is_some() => {
                strict = false;
                was_strict = true;
                ledger.drop_strict();
                reporter.check()?;
            }
            other => {
                // Remembered for the session only when the plain request
                // worked: a 400 for another reason must not mark the server.
                if was_strict && other.is_ok() {
                    if let Ok(mut bases) = strict_refused_bases().lock() {
                        bases.insert(base.to_owned());
                    }
                }
                return other;
            }
        }
    }
}

fn request_body(
    model: &str,
    instructions: &str,
    input: &str,
    schema: Option<&Value>,
    strict: bool,
) -> Value {
    // With a plan token the gateway insists on `input` being a list of
    // messages ("Input must be a list"), as in OpenAI's examples.
    let mut body = json!({
        "model": model.trim(),
        "instructions": instructions,
        "input": [{"role": "user", "content": input}],
        "store": false,
        "stream": true,
    });
    if let Some(schema) = schema {
        body["text"] = json!({"format": {"type": "json_schema", "name": "ponte", "schema": schema, "strict": strict}});
    }
    body
}

/// What the helper thread needs for one exchange; it owns all of it.
#[derive(Clone)]
struct Exchange {
    url: String,
    token: String,
    body: Value,
    strict: bool,
    limit: Duration,
    ledger: crate::ai::Ledger,
}

fn network_failure(error: &reqwest::Error, limit: Duration) -> anyhow::Error {
    if error.is_timeout() {
        crate::ai::Transient::error(crate::ai::timeout_message(limit), None)
    } else if error.is_connect() {
        crate::ai::Transient::error(
            "Não foi possível conectar ao ChatGPT. Verifique a conexão com a internet.",
            None,
        )
    } else if error.is_builder() {
        anyhow::anyhow!("Não foi possível preparar a conexão com a OpenAI.")
    } else {
        crate::ai::Transient::error("A comunicação com o ChatGPT falhou.", None)
    }
}

/// A refusal of the API, as the error the loop above needs: a refused strict
/// schema, a failure that may pass, or the plain explanation.
fn refusal(
    status: reqwest::StatusCode,
    body: &[u8],
    strict: bool,
    retry_after: Option<Duration>,
) -> anyhow::Error {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let code = value["error"]["code"]
        .as_str()
        .or_else(|| value["error"]["type"].as_str())
        .unwrap_or("");
    let status = status.as_u16();
    // Any 400 to a request that carried the strict schema is taken as a
    // refusal of the strict form: the wording of the provider is not known,
    // so the request is repeated once as a plain schema (and only that
    // second answer decides whether the refusal was really about the schema).
    if strict && status == 400 {
        return anyhow::Error::new(SchemaRefused);
    }
    // The limits of the plan are not a hiccup: waiting seconds does not
    // bring the allowance back.
    let plan_limit = code.starts_with("subscription_sharing_")
        && code != "subscription_sharing_usage_unavailable";
    let message = api_error(
        reqwest::StatusCode::from_u16(status).unwrap_or(reqwest::StatusCode::BAD_GATEWAY),
        body,
    )
    .to_string();
    if crate::ai::transient_status(status) && !plan_limit {
        return crate::ai::Transient::error(message, retry_after);
    }
    anyhow::anyhow!(message)
}

/// One request and its streamed answer, on the helper thread.
fn exchange(call: Exchange, reporter: &crate::ai::Reporter) -> Result<String> {
    reporter.check()?;
    call.ledger.take()?;
    let client = Client::builder()
        .connect_timeout(crate::ai::CHATGPT_CONNECT_TIMEOUT)
        .timeout(call.limit)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Não foi possível preparar a conexão com a OpenAI.")?;
    reporter.step("Enviando ao ChatGPT e aguardando o início da resposta…");
    let response = client
        .post(&call.url)
        .bearer_auth(&call.token)
        .header("Accept", "text/event-stream")
        .json(&call.body)
        .send()
        .map_err(|error| network_failure(&error, call.limit))?;
    let status = response.status();
    if !status.is_success() {
        let retry_after = crate::ai::retry_after_of(response.headers());
        let body = read_limited(response, MAX_ERROR_BYTES)?;
        return Err(refusal(status, &body, call.strict, retry_after));
    }
    read_stream_within(
        BufReader::new(response.take(MAX_RESPONSE_BYTES + 1)),
        reporter,
        call.limit,
    )
}

/// Collects the text of a server-sent-events stream; success only on
/// `response.completed`, as documented.
#[cfg(test)]
fn read_stream(reader: impl BufRead, reporter: &crate::ai::Reporter) -> Result<String> {
    read_stream_within(reader, reporter, crate::ai::CHATGPT_REQUEST_TIMEOUT)
}

fn read_stream_within(
    reader: impl BufRead,
    reporter: &crate::ai::Reporter,
    limit: Duration,
) -> Result<String> {
    let mut text = String::new();
    let mut data = String::new();
    let mut completed: Option<Value> = None;
    let mut total = 0_u64;
    let mut last_report = (Instant::now(), 0_usize);
    let handle_event = |event: &Value, text: &mut String| -> Result<bool> {
        match event["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => {
                text.push_str(event["delta"].as_str().unwrap_or(""));
                ensure!(
                    text.len() <= MAX_GENERATED_BYTES,
                    "A resposta da IA passa de 128 KB."
                );
                Ok(false)
            }
            "response.completed" => Ok(true),
            "response.failed" | "error" => {
                // The documented shapes put the fields at the top level
                // (`error` event) or under `response.error` (`response.failed`);
                // some gateways nest them under `error`. Only the stable code
                // is read: what the event says in words is never shown.
                let places = [event, &event["error"], &event["response"]["error"]];
                let pick = |key: &str| {
                    places
                        .iter()
                        .find_map(|place| place[key].as_str().filter(|s| !s.trim().is_empty()))
                        .unwrap_or("")
                        .to_owned()
                };
                let code = {
                    let code = pick("code");
                    if code.is_empty() {
                        places[1..]
                            .iter()
                            .find_map(|place| place["type"].as_str().filter(|s| !s.is_empty()))
                            .unwrap_or("")
                            .to_owned()
                    } else {
                        code
                    }
                };
                let message = explain(0, &code, "", &pick("param"));
                if matches!(
                    code.as_str(),
                    "server_error" | "internal_error" | "service_unavailable" | "overloaded_error"
                ) {
                    return Err(crate::ai::Transient::error(message, None));
                }
                bail!("{message}")
            }
            "response.incomplete" => {
                let reason = event["response"]["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("");
                if reason == "max_output_tokens" {
                    return Err(crate::ai::truncated(
                        "A IA atingiu o limite de tamanho da resposta; nada foi salvo.",
                    ));
                }
                bail!(
                    "A IA interrompeu a resposta antes do fim{}; nada foi salvo.",
                    ident(reason).map_or(String::new(), |r| format!(" ({r})"))
                )
            }
            _ => Ok(false),
        }
    };
    for line in reader.lines() {
        let line = line.map_err(|error| {
            crate::ai::stream_failure(
                &error,
                limit,
                "A conexão com o ChatGPT caiu no meio da resposta.",
            )
        })?;
        total += line.len() as u64 + 1;
        ensure!(
            total <= MAX_RESPONSE_BYTES,
            "A resposta da IA passa de 2 MB."
        );
        if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.trim_start());
            continue;
        }
        if line.is_empty() && !data.is_empty() {
            reporter.check()?;
            let event: Value =
                serde_json::from_str(&data).context("Evento do ChatGPT em formato inesperado.")?;
            data.clear();
            if handle_event(&event, &mut text)? {
                completed = Some(event);
                break;
            }
            let now = Instant::now();
            if (!text.is_empty() && last_report.1 == 0)
                || text.len() >= last_report.1 + 2048
                || now.duration_since(last_report.0) >= Duration::from_millis(500)
            {
                last_report = (now, text.len());
                reporter.step(format!(
                    "Recebendo a resposta… ({} caracteres)",
                    text.chars().count()
                ));
            }
        }
    }
    let Some(completed) = completed else {
        return Err(crate::ai::Transient::error(
            "A resposta do ChatGPT terminou sem confirmação de conclusão; nada foi salvo.",
            None,
        ));
    };
    if text.trim().is_empty() {
        // Some servers only send the final object.
        if let Some(outputs) = completed["response"]["output"].as_array() {
            for item in outputs {
                if let Some(parts) = item["content"].as_array() {
                    for part in parts {
                        if part["type"] == "output_text" {
                            text.push_str(part["text"].as_str().unwrap_or(""));
                        }
                    }
                }
            }
        }
    }
    ensure!(
        !text.trim().is_empty() && text.len() <= MAX_GENERATED_BYTES,
        "A resposta da IA está vazia ou passa de 128 KB."
    );
    Ok(text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diagnostic, run by hand: does this build reach OpenAI's token endpoint?
    /// It sends a made-up code (no account, no secret) and expects OpenAI to
    /// REFUSE it; a connection failure here is the fault to look for.
    #[test]
    #[ignore = "Fala com auth.openai.com; só para diagnóstico manual"]
    fn live_token_endpoint_answers() {
        let client = http(Duration::from_secs(30)).unwrap();
        let raw = client
            .post(TOKEN_URL)
            .header("Accept", "application/json")
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", "diagnostico"),
            ])
            .send();
        match &raw {
            Ok(response) => println!("resposta HTTP {}", response.status()),
            Err(error) => {
                println!(
                    "falha: connect={} timeout={} request={} body={}",
                    error.is_connect(),
                    error.is_timeout(),
                    error.is_request(),
                    error.is_body()
                );
                let mut source = std::error::Error::source(error);
                while let Some(cause) = source {
                    println!("  causa: {cause}");
                    source = cause.source();
                }
            }
        }
        assert!(raw.is_ok(), "o pedido não chegou à OpenAI");
    }
    use std::io::Cursor;
    use std::net::TcpListener;

    #[test]
    fn pkce_authorize_url_and_callback_follow_the_documented_flow() {
        // RFC 7636 appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let host = new_host_id();
        assert!(host.starts_with("urn:uuid:"));
        let first = Login::start(&host, None).unwrap();
        let url = url::Url::parse(&first.authorize_url).unwrap();
        let pairs: std::collections::BTreeMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(pairs["client_id"], DYNAMIC_CLIENT);
        assert_eq!(pairs["agent_name_hint"], "Ponte");
        assert_eq!(pairs["ext_agent_host_id"], host);
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert_eq!(pairs["code_challenge"], pkce_challenge(&first.verifier));
        assert_eq!(pairs["resource"], RESOURCE);
        assert!(pairs["scope"].contains(SHARING_SCOPE));
        assert!(pairs["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(pairs["redirect_uri"].ends_with(CALLBACK_PATH));
        assert_eq!(pairs["redirect_uri"], first.redirect_uri);
        assert!(!pairs.contains_key("id_token_hint"));

        let previous = ChatGptSession {
            client_id: "cid_123".into(),
            account_label: "pessoa@example.org".into(),
            id_token: "a.b.c".into(),
            ..Default::default()
        };
        let again = Login::start(&host, Some(&previous)).unwrap();
        let url = url::Url::parse(&again.authorize_url).unwrap();
        let pairs: std::collections::BTreeMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(pairs["client_id"], "cid_123");
        assert!(!pairs.contains_key("agent_name_hint"));
        assert_eq!(pairs["id_token_hint"], "a.b.c");
        assert_eq!(pairs["login_hint"], "pessoa@example.org");
        assert_ne!(first.state, again.state, "fresh state per attempt");

        // Callback checks.
        let ok = parse_callback(
            "code=abc&state=S1&client_id=cid_new&scope=openid+chatgpt.tokens.use.direct",
            "S1",
            None,
        )
        .unwrap();
        assert_eq!(
            (ok.code.as_str(), ok.client_id.as_str()),
            ("abc", "cid_new")
        );
        assert_eq!(ok.scopes, ["openid", SHARING_SCOPE]);
        assert!(parse_callback("code=abc&state=OTHER&client_id=x", "S1", None).is_err());
        assert!(
            parse_callback("code=abc&state=S1", "S1", None).is_err(),
            "dynamic registration must return a client id"
        );
        assert!(parse_callback("error=access_denied&state=S1", "S1", None)
            .err()
            .unwrap()
            .to_string()
            .contains("não autorizou"));
        let reauth = parse_callback("code=abc&state=S1", "S1", Some("cid_123")).unwrap();
        assert_eq!(reauth.client_id, "cid_123");
        assert!(parse_callback(
            "code=abc&state=S1&client_id=cid_other",
            "S1",
            Some("cid_123")
        )
        .is_err());
        assert_eq!(split_scopes("a b+c"), ["a", "b", "c"]);
    }

    #[test]
    fn id_token_claims_are_checked_before_an_account_is_accepted() {
        let good = json!({"iss": ISSUER, "aud": ["cid"], "exp": 2_000_000_000_i64, "nonce": "n1", "sub": "user-1", "email": "p@example.org"});
        let claims = check_claims(&good, "cid", "n1", 1_900_000_000).unwrap();
        assert_eq!(
            (claims.sub.as_str(), claims.email.as_str()),
            ("user-1", "p@example.org")
        );
        let mut bad = good.clone();
        bad["iss"] = json!("https://evil.example");
        assert!(check_claims(&bad, "cid", "n1", 1_900_000_000).is_err());
        let mut bad = good.clone();
        bad["aud"] = json!("other");
        assert!(check_claims(&bad, "cid", "n1", 1_900_000_000).is_err());
        assert!(
            check_claims(&good, "cid", "n1", 2_100_000_000).is_err(),
            "expired"
        );
        assert!(
            check_claims(&good, "cid", "n2", 1_900_000_000).is_err(),
            "nonce"
        );
        let mut string_aud = good.clone();
        string_aud["aud"] = json!("cid");
        assert!(check_claims(&string_aud, "cid", "n1", 1_900_000_000).is_ok());
    }

    #[test]
    fn session_helpers_and_model_list_behave() {
        let mut session = ChatGptSession {
            client_id: "cid".into(),
            refresh_token: "r".into(),
            access_token: "a".into(),
            expires_at: "2000-01-01T00:00:00Z".into(),
            scopes: vec!["openid".into(), SHARING_SCOPE.into()],
            account_label: "p@example.org".into(),
            ..Default::default()
        };
        assert!(session.signed_in() && session.sharing());
        assert!(session.expires_within(Duration::from_secs(0)), "past date");
        session.expires_at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        assert!(!session.expires_within(Duration::from_secs(60)));
        assert!(session.expires_within(Duration::from_secs(2 * 3600)));
        let stripped = session.without_tokens();
        assert!(!stripped.signed_in());
        assert_eq!(stripped.client_id, "cid");
        assert_eq!(stripped.account_label, "p@example.org");
        assert!(stripped.scopes.is_empty());
        session.scopes = vec!["openid".into()];
        assert!(!session.sharing());

        let models = parse_models(
            json!({"data":[{"id":"gpt-a","visibility":"list"},{"id":"gpt-hidden","visibility":"hidden"},{"id":"gpt-b"},{"id":""}]})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["gpt-a", "gpt-b"]
        );
        // Shape documented for the plan-sharing flow.
        let models = parse_models(
            json!({"object":"list","models":[
                {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list"},
                {"slug":"gpt-internal","display_name":"Interno","visibility":"hidden"}
            ]})
            .to_string()
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["gpt-6.1-sol"]
        );
        let error = parse_models(br#"{"object":"list","items":[]}"#)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("items") && error.contains("object"),
            "{error}"
        );
    }

    #[test]
    fn stream_text_is_collected_only_until_completion_and_limits_are_explained() {
        let stream = "event: response.created\ndata: {\"type\":\"response.created\"}\n\nevent: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"{\\\"a\\\":\"}\n\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"1}\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\"}}\n\n";
        let silent = crate::ai::Reporter::silent();
        assert_eq!(
            read_stream(Cursor::new(stream), &silent).unwrap(),
            "{\"a\":1}"
        );
        let cut = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
        assert!(read_stream(Cursor::new(cut), &silent)
            .unwrap_err()
            .to_string()
            .contains("sem confirmação"));
        let failed = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\",\"message\":\"limit\"}}}\n\n";
        assert!(read_stream(Cursor::new(failed), &silent)
            .unwrap_err()
            .to_string()
            .contains("cota"));
        // Nested error object, and an error event with nothing recognizable.
        let nested = "data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"campo recusado\"}}\n\n";
        let error = read_stream(Cursor::new(nested), &silent)
            .unwrap_err()
            .to_string();
        // The code is shown, the words of the provider are not.
        assert!(
            error.contains("interrompeu")
                && error.contains("invalid_request_error")
                && !error.contains("campo recusado"),
            "{error}"
        );
        let bare = "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"id\":\"r9\"}}\n\n";
        let error = read_stream(Cursor::new(bare), &silent)
            .unwrap_err()
            .to_string();
        // Nothing of the raw event is shown.
        assert!(
            error.contains("interrompeu") && !error.contains("evento") && !error.contains("r9"),
            "{error}"
        );
        let only_final ="data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final\"}]}]}}\n\n";
        assert_eq!(
            read_stream(Cursor::new(only_final), &silent).unwrap(),
            "final"
        );
        // Progress is reported and a cancellation stops the reading.
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let reporter = crate::ai::Reporter::new(sender, cancel.clone());
        assert_eq!(
            read_stream(Cursor::new(stream), &reporter).unwrap(),
            "{\"a\":1}"
        );
        let steps: Vec<String> = receiver.try_iter().collect();
        assert!(
            steps.iter().any(|s| s.contains("Recebendo a resposta")),
            "{steps:?}"
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(read_stream(Cursor::new(stream), &reporter)
            .unwrap_err()
            .to_string()
            .contains("Cancelado"));
        assert!(explain(401, "", "", "").contains("Entre de novo"));
        assert!(
            explain(403, "subscription_sharing_user_not_eligible", "", "").contains("Plus ou Pro")
        );
        assert!(explain(503, "", "", "").contains("Tente de novo"));
        assert!(explain(
            400,
            "subscription_sharing_unsupported_capability",
            "",
            "tools"
        )
        .contains("tools"));
    }

    #[test]
    fn mock_responses_api_receives_documented_body_and_bearer_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut all = Vec::new();
                let mut buffer = [0_u8; 4096];
                let (header_end, content_length) = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    all.extend_from_slice(&buffer[..count]);
                    if let Some(end) = all.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&all[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while all.len() < header_end + content_length {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    all.extend_from_slice(&buffer[..count]);
                }
                let head = String::from_utf8_lossy(&all[..header_end]).into_owned();
                let body: Value =
                    serde_json::from_slice(&all[header_end..header_end + content_length]).unwrap();
                let limit = body["model"] == "limited";
                sent.send((head, body)).unwrap();
                if limit {
                    let error = json!({"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"weekly limit"}}).to_string();
                    write!(stream, "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", error.len(), error).unwrap();
                } else {
                    let events = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ol\u{e1}\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n";
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", events.len(), events).unwrap();
                }
            }
        });
        let base = format!("http://{address}");
        let silent = crate::ai::Reporter::silent();
        let text = respond_at(
            &base,
            "tok",
            "gpt-x",
            "instru\u{e7}\u{f5}es",
            "{\"dado\":1}",
            Some(json!({"type":"object"})),
            &silent,
            &crate::ai::Ledger::analysis(),
        )
        .unwrap();
        assert_eq!(text, "ol\u{e1}");
        let error = respond_at(
            &base,
            "tok",
            "limited",
            "i",
            "d",
            None,
            &silent,
            &crate::ai::Ledger::analysis(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cota"), "{error}");
        server.join().unwrap();
        let (head, body) = received.recv().unwrap();
        assert!(head.to_lowercase().contains("authorization: bearer tok"));
        assert!(head.starts_with("POST /responses "));
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["instructions"], "instru\u{e7}\u{f5}es");
        assert!(body["input"].is_array(), "input must be a list: {body}");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], "{\"dado\":1}");
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert!(body.get("temperature").is_none() && body.get("max_output_tokens").is_none());
        let (_, second) = received.recv().unwrap();
        assert!(second.get("text").is_none());
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use crate::ai::{Reporter, Timing, Transient, Truncated};
    use std::io::Cursor;
    use std::net::TcpListener;
    use std::sync::mpsc;

    fn quick() -> Timing {
        Timing {
            waits: [Duration::from_millis(20), Duration::from_millis(40)],
            ollama_timeout: Duration::from_secs(5),
            chatgpt_timeout: Duration::from_secs(5),
        }
    }

    /// Serves one scripted reply per connection and hands back the request
    /// bodies. Replies: `OK <text>` (a stream ending with the text),
    /// `HTTP <code>[ retry-after=<s>]|<body>`, `INCOMPLETE <reason>`,
    /// `SILENT <ms>` (the request is read, nothing comes back).
    fn mock_responses(
        replies: Vec<String>,
    ) -> (String, mpsc::Receiver<Value>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sent, received) = mpsc::channel();
        let server = std::thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut all = Vec::new();
                let mut buffer = [0_u8; 4096];
                let (header_end, content_length) = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    all.extend_from_slice(&buffer[..count]);
                    if let Some(end) = all.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&all[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while all.len() < header_end + content_length {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    all.extend_from_slice(&buffer[..count]);
                }
                let body: Value =
                    serde_json::from_slice(&all[header_end..header_end + content_length]).unwrap();
                sent.send(body).unwrap();
                if let Some(rest) = reply.strip_prefix("HTTP ") {
                    let (head, refusal) = rest.split_once('|').unwrap_or((rest, ""));
                    let mut parts = head.split_whitespace();
                    let code = parts.next().unwrap();
                    let extra: String = parts
                        .filter_map(|p| p.strip_prefix("retry-after="))
                        .map(|seconds| format!("Retry-After: {seconds}\r\n"))
                        .collect();
                    let _ = write!(stream, "HTTP/1.1 {code} Refused\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{refusal}", refusal.len());
                } else if let Some(wait) = reply.strip_prefix("SILENT ") {
                    std::thread::sleep(Duration::from_millis(wait.trim().parse().unwrap()));
                } else {
                    let events = if let Some(reason) = reply.strip_prefix("INCOMPLETE ") {
                        format!("data: {{\"type\":\"response.output_text.delta\",\"delta\":\"{{\\\"a\\\":\"}}\n\ndata: {{\"type\":\"response.incomplete\",\"response\":{{\"incomplete_details\":{{\"reason\":\"{reason}\"}}}}}}\n\n")
                    } else {
                        let text = reply.strip_prefix("OK ").unwrap();
                        format!("data: {{\"type\":\"response.output_text.delta\",\"delta\":{}}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{}}}}\n\n", json!(text))
                    };
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", events.len(), events);
                }
            }
        });
        (format!("http://{address}"), received, server)
    }

    fn call(
        base: &str,
        schema: Option<Value>,
        reporter: &Reporter,
        timing: &Timing,
    ) -> Result<String> {
        respond_at_with(
            base,
            "tok",
            "gpt-x",
            "instruções",
            "{\"dado\":1}",
            schema,
            reporter,
            &crate::ai::Ledger::analysis(),
            timing,
        )
    }

    fn strict_schema() -> Value {
        json!({"type":"object","additionalProperties":false,"required":["a","b"],"properties":{
            "a":{"type":"string","enum":["x","y"]},
            "b":{"type":["string","null"]},
        }})
    }

    #[test]
    fn strict_ok_accepts_only_what_the_strict_mode_takes() {
        assert!(strict_ok(&strict_schema()));
        // Nested arrays of objects, all required, nullable by type array.
        assert!(strict_ok(
            &json!({"type":"object","additionalProperties":false,"required":["items"],"properties":{
            "items":{"type":"array","maxItems":8,"items":{"type":"object","additionalProperties":false,"required":["id","date"],"properties":{
                "id":{"type":"string"},"date":{"type":["string","null"]}}}}}})
        ));
        for bad in [
            // not an object at the root
            json!({"type":"array","items":{"type":"string"}}),
            // a property that is not required
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"},"b":{"type":"string"}}}),
            // a required name that is not a property
            json!({"type":"object","additionalProperties":false,"required":["a","c"],"properties":{"a":{"type":"string"}}}),
            // additionalProperties missing or true
            json!({"type":"object","required":["a"],"properties":{"a":{"type":"string"}}}),
            json!({"type":"object","additionalProperties":true,"required":["a"],"properties":{"a":{"type":"string"}}}),
            // keywords the strict mode does not take
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string","maxLength":10}}}),
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string","minLength":1}}}),
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string","pattern":"x"}}}),
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"integer","minimum":1}}}),
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"oneOf":[{"type":"string"}]}}}),
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"$ref":"#/x"}}}),
            // an array without items; a node with no type at all
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"array"}}}),
            json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{}}}),
            // an object without properties
            json!({"type":"object","additionalProperties":false}),
            Value::Null,
        ] {
            assert!(!strict_ok(&bad), "{bad}");
        }
        // Too deep: six levels of objects.
        let mut deep = json!({"type":"string"});
        for _ in 0..6 {
            deep = json!({"type":"object","additionalProperties":false,"required":["n"],"properties":{"n":deep}});
        }
        assert!(!strict_ok(&deep));
        // A schema with length limits is sent as plain.
        assert!(!strict_ok(
            &json!({"type":"object","additionalProperties":false,"required":["s"],"properties":{"s":{"type":"string","maxLength":500}}})
        ));
    }

    #[test]
    fn strict_is_sent_when_the_schema_allows_and_refused_once_then_plain() {
        // The wording of the refusal is not known: any 400 counts.
        let refusal = r#"HTTP 400|{"error":{"message":"Bad request."}}"#;
        let (base, received, server) = mock_responses(vec![
            "OK {\"a\":\"x\",\"b\":null}".into(),
            refusal.into(),
            "OK {\"a\":\"y\",\"b\":null}".into(),
            "OK {\"a\":\"x\",\"b\":null}".into(),
        ]);
        let silent = Reporter::silent();
        assert_eq!(
            call(&base, Some(strict_schema()), &silent, &quick()).unwrap(),
            "{\"a\":\"x\",\"b\":null}"
        );
        // The server refuses the strict form: once more, plain, and the answer is used.
        assert_eq!(
            call(&base, Some(strict_schema()), &silent, &quick()).unwrap(),
            "{\"a\":\"y\",\"b\":null}"
        );
        // And it is not asked in the strict form again in this session.
        call(&base, Some(strict_schema()), &silent, &quick()).unwrap();
        server.join().unwrap();
        let bodies: Vec<Value> = received.try_iter().collect();
        assert_eq!(bodies.len(), 4);
        assert_eq!(bodies[0]["text"]["format"]["strict"], true);
        assert_eq!(bodies[0]["text"]["format"]["type"], "json_schema");
        assert_eq!(bodies[1]["text"]["format"]["strict"], true);
        assert_eq!(bodies[2]["text"]["format"]["strict"], false);
        assert_eq!(bodies[3]["text"]["format"]["strict"], false);
        assert_eq!(bodies[0]["text"]["format"]["schema"], strict_schema());
    }

    #[test]
    fn a_400_that_is_not_about_the_schema_is_asked_once_more_and_does_not_mark_the_server() {
        let refusal = r#"HTTP 400|{"error":{"message":"Bad request."}}"#;
        let (base, received, server) = mock_responses(vec![
            refusal.into(),
            refusal.into(),
            "OK {\"a\":\"x\",\"b\":null}".into(),
        ]);
        let silent = Reporter::silent();
        assert!(call(&base, Some(strict_schema()), &silent, &quick()).is_err());
        // Not remembered: the next request is strict again.
        call(&base, Some(strict_schema()), &silent, &quick()).unwrap();
        server.join().unwrap();
        let bodies: Vec<Value> = received.try_iter().collect();
        assert_eq!(bodies.len(), 3);
        assert_eq!(bodies[0]["text"]["format"]["strict"], true);
        assert_eq!(bodies[1]["text"]["format"]["strict"], false);
        assert_eq!(bodies[2]["text"]["format"]["strict"], true);
    }

    #[test]
    fn the_sends_of_a_job_are_capped_and_the_plain_fallback_is_kept_across_its_attempts() {
        let refusal = r#"HTTP 400|{"error":{"message":"Bad request."}}"#;
        // The cap of one send leaves no room for the plain repeat.
        let (base, received, _server) =
            mock_responses(vec![refusal.into(), "OK {\"a\":\"x\",\"b\":null}".into()]);
        let silent = Reporter::silent();
        let one = crate::ai::Ledger::new(1);
        let error = respond_at_with(
            &base,
            "tok",
            "gpt-x",
            "i",
            "{}",
            Some(strict_schema()),
            &silent,
            &one,
            &quick(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("muitas vezes"), "{error}");
        assert_eq!(received.try_iter().count(), 1);
        // With room: strict, then plain, and the job remembers it, so a
        // transient failure afterwards is retried plain.
        let (base, received, _server) = mock_responses(vec![
            refusal.into(),
            "HTTP 503|{}".into(),
            "OK {\"a\":\"x\",\"b\":null}".into(),
        ]);
        let ledger = crate::ai::Ledger::analysis();
        respond_at_with(
            &base,
            "tok",
            "gpt-x",
            "i",
            "{}",
            Some(strict_schema()),
            &silent,
            &ledger,
            &quick(),
        )
        .unwrap();
        let bodies: Vec<Value> = received.try_iter().collect();
        assert_eq!(bodies.len(), 3);
        assert_eq!(bodies[0]["text"]["format"]["strict"], true);
        assert_eq!(bodies[1]["text"]["format"]["strict"], false);
        assert_eq!(bodies[2]["text"]["format"]["strict"], false);
        assert!(ledger.strict_dropped());
    }

    #[test]
    fn a_schema_the_strict_mode_cannot_take_goes_plain_from_the_start() {
        let (base, received, server) = mock_responses(vec!["OK {}".into()]);
        let schema = json!({"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string","maxLength":5}}});
        call(&base, Some(schema), &Reporter::silent(), &quick()).unwrap();
        server.join().unwrap();
        assert_eq!(received.recv().unwrap()["text"]["format"]["strict"], false);
    }

    #[test]
    fn a_failure_that_may_pass_is_tried_again_and_the_limits_of_the_plan_are_not() {
        let silent = Reporter::silent();
        // 503 and a rate limit with a short Retry-After pass the second time.
        let (base, received, server) = mock_responses(vec![
            "HTTP 503|{}".into(),
            "HTTP 429 retry-after=0|{\"error\":{\"code\":\"rate_limit_exceeded\"}}".into(),
            "OK ok".into(),
        ]);
        assert_eq!(call(&base, None, &silent, &quick()).unwrap(), "ok");
        server.join().unwrap();
        assert_eq!(received.try_iter().count(), 3);
        // Three in a row end with the plain explanation, no more requests.
        let (base, received, server) = mock_responses(vec![
            "HTTP 500|{}".into(),
            "HTTP 500|{}".into(),
            "HTTP 500|{}".into(),
        ]);
        let error = call(&base, None, &silent, &quick())
            .unwrap_err()
            .to_string();
        server.join().unwrap();
        assert!(error.contains("código 500"), "{error}");
        assert_eq!(received.try_iter().count(), 3);
        // The allowance of the plan is not a hiccup.
        let (base, received, server) = mock_responses(vec![
            "HTTP 429|{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}".into(),
        ]);
        let error = call(&base, None, &silent, &quick())
            .unwrap_err()
            .to_string();
        server.join().unwrap();
        assert!(error.contains("cota"), "{error}");
        assert_eq!(received.try_iter().count(), 1);
        // Neither is a refusal of the key.
        let (base, received, server) = mock_responses(vec!["HTTP 401|{}".into()]);
        assert!(call(&base, None, &silent, &quick())
            .unwrap_err()
            .to_string()
            .contains("Entre de novo"));
        server.join().unwrap();
        assert_eq!(received.try_iter().count(), 1);
    }

    #[test]
    fn what_the_provider_wrote_is_never_shown() {
        let silent = Reporter::silent();
        let secret = "TRECHO-DO-DOCUMENTO-NO-ERRO";
        for (code, status) in [("400", ""), ("403", ""), ("418", "")] {
            let (base, _received, server) = mock_responses(vec![format!(
                "HTTP {code}|{{\"error\":{{\"message\":\"{secret} {status}\",\"code\":\"x y\"}},\"detail\":\"{secret}\"}}"
            )]);
            let error = call(&base, None, &silent, &quick())
                .unwrap_err()
                .to_string();
            server.join().unwrap();
            assert!(!error.contains(secret), "{error}");
            assert!(error.contains("ChatGPT"), "{error}");
        }
        // The same for the events of a stream.
        for event in [
            format!("data: {{\"type\":\"error\",\"message\":\"{secret}\"}}\n\n"),
            format!("data: {{\"type\":\"response.failed\",\"response\":{{\"error\":{{\"code\":\"bad code!\",\"message\":\"{secret}\",\"param\":\"{secret}\"}}}}}}\n\n"),
            format!("data: {{\"type\":\"response.incomplete\",\"response\":{{\"incomplete_details\":{{\"reason\":\"{secret} com espaços\"}}}}}}\n\n"),
        ] {
            let error = read_stream(Cursor::new(event), &silent)
                .unwrap_err()
                .to_string();
            assert!(!error.contains(secret), "{error}");
        }
        // A stable code and a field name that look like identifiers are kept.
        assert!(explain(
            400,
            "subscription_sharing_unsupported_capability",
            "x",
            "tools"
        )
        .contains("tools"));
        assert!(explain(0, "invalid_request_error", "frase do provedor", "")
            .contains("invalid_request_error"));
        assert!(!explain(0, "invalid_request_error", "frase do provedor", "").contains("frase"));
        assert!(!explain(403, "", "frase do provedor", "").contains("frase"));
        assert!(!explain(500, "", "frase do provedor", "").contains("frase"));
    }

    #[test]
    fn a_timeout_is_told_as_a_timeout_after_the_tries() {
        let (base, received, server) = mock_responses(vec![
            "SILENT 400".into(),
            "SILENT 400".into(),
            "SILENT 400".into(),
        ]);
        let timing = Timing {
            chatgpt_timeout: Duration::from_millis(120),
            ..quick()
        };
        let (sender, progress) = mpsc::channel();
        let reporter = Reporter::new(sender, Arc::new(AtomicBool::new(false)));
        let error = call(&base, None, &reporter, &timing)
            .unwrap_err()
            .to_string();
        assert!(error.contains("limite de espera"), "{error}");
        assert!(!error.contains("caiu"), "{error}");
        server.join().unwrap();
        assert_eq!(received.try_iter().count(), 3);
        let steps: Vec<String> = progress.try_iter().collect();
        assert!(
            steps.iter().any(|s| s.contains("Nova tentativa")),
            "{steps:?}"
        );
    }

    #[test]
    fn cancelling_works_before_the_first_byte() {
        let (base, _received, server) = mock_responses(vec!["SILENT 4000".into()]);
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, _progress) = mpsc::channel();
        let reporter = Reporter::new(sender, cancel.clone());
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let error = call(&base, None, &reporter, &quick())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Cancelado"), "{error}");
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
        drop(server);
    }

    #[test]
    fn an_answer_cut_by_length_is_a_typed_error_and_a_dropped_stream_may_pass() {
        let silent = Reporter::silent();
        let cut = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"{\\\"a\\\":\"}\n\ndata: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n";
        let error = read_stream(Cursor::new(cut), &silent).unwrap_err();
        assert!(error.downcast_ref::<Truncated>().is_some(), "{error}");
        // Another reason is a plain refusal, with the reason when it is an identifier.
        let filtered = "data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"content_filter\"}}}\n\n";
        let error = read_stream(Cursor::new(filtered), &silent).unwrap_err();
        assert!(error.downcast_ref::<Truncated>().is_none());
        assert!(error.to_string().contains("content_filter"), "{error}");
        // A stream that ends without `response.completed` may pass.
        let dropped = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n";
        let error = read_stream(Cursor::new(dropped), &silent).unwrap_err();
        assert!(error.downcast_ref::<Transient>().is_some(), "{error}");
        // A server-side failure event may pass; a plan limit may not.
        let busy = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_error\"}}}\n\n";
        let error = read_stream(Cursor::new(busy), &silent).unwrap_err();
        assert!(error.downcast_ref::<Transient>().is_some(), "{error}");
        let limit = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}}\n\n";
        let error = read_stream(Cursor::new(limit), &silent).unwrap_err();
        assert!(error.downcast_ref::<Transient>().is_none(), "{error}");
        // End to end: the cut answer is reported, with nothing saved.
        let (base, _received, server) = mock_responses(vec!["INCOMPLETE max_output_tokens".into()]);
        let error = call(&base, None, &silent, &quick()).unwrap_err();
        server.join().unwrap();
        assert!(error.downcast_ref::<Truncated>().is_some(), "{error}");
    }
}
