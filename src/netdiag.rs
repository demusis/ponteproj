//! How the program reaches the internet, and what to say when it cannot.
//!
//! - Every HTTPS client of the program is built here ([`client_builder`]):
//!   it trusts the certificates bundled with the program AND the ones the
//!   Windows certificate store trusts (reqwest's native roots), so a network
//!   that inspects secure connections with a root installed in Windows works
//!   as it does in the browser. Verification is never turned off.
//! - A proxy given by automatic configuration (WPAD or a PAC script) is
//!   resolved the way the system does, with WinHTTP, and handed to reqwest
//!   (a static proxy and the proxy variables keep being read by reqwest).
//! - A failed connection is classified ([`NetFailure`]) from the error chain
//!   into a plain sentence that says what to do ([`describe`]).
//! - [`diagnose`] runs the checks of "Testar a conexão com a OpenAI" and
//!   writes a report that is safe to send to support: host names, counts,
//!   times and proxy host names only; never a token, an account or a body.
use reqwest::blocking::{Client, ClientBuilder};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;

/// The hosts of the ChatGPT sign-in and of its inference API.
pub const OPENAI_HOSTS: [&str; 2] = ["auth.openai.com", "api.openai.com"];

/// How long the system may take to tell which proxy a URL needs before the
/// program goes direct (a missing WPAD must not stall every request).
const PROXY_LOOKUP_LIMIT: Duration = Duration::from_millis(2500);
const TCP_LIMIT: Duration = Duration::from_secs(5);
const DNS_LIMIT: Duration = Duration::from_secs(8);
const HTTPS_CONNECT_LIMIT: Duration = Duration::from_secs(10);
const HTTPS_LIMIT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// Clients

/// Builder for every client that talks to the internet: bundled roots plus
/// the roots Windows trusts (the crate's `rustls-tls` and
/// `rustls-tls-native-roots` features), the proxy of the system's automatic
/// configuration, and the proxy settings reqwest reads by itself (variables
/// and the static proxy of the registry). Callers add their timeouts.
pub fn client_builder() -> ClientBuilder {
    Client::builder().proxy(reqwest::Proxy::custom(|url: &Url| proxy_for(url)))
}

/// Certificates the operating system trusts, for code that builds its own
/// TLS configuration (the e-mail connection). Unreadable ones are skipped.
pub fn native_roots() -> Vec<rustls::pki_types::CertificateDer<'static>> {
    rustls_native_certs::load_native_certs().certs
}

// ---------------------------------------------------------------------------
// Proxy

/// What the system says about proxies for the current user.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxySettings {
    /// "Detectar automaticamente as configurações" (WPAD).
    pub auto_detect: bool,
    /// Address of the configuration script (PAC), when there is one.
    pub pac_url: String,
    /// The static proxy ("host:port" or "http=a:1;https=b:2").
    pub static_proxy: String,
}

impl ProxySettings {
    /// A proxy that only the system can resolve for a URL.
    pub fn needs_resolution(&self) -> bool {
        self.auto_detect || !self.pac_url.trim().is_empty()
    }
}

/// Host and port of a proxy entry, without credentials, for showing.
fn shown_host(entry: &str) -> String {
    let entry = entry.trim();
    let entry = entry.split_once('=').map_or(entry, |(_, rest)| rest);
    let entry = entry.rsplit('@').next().unwrap_or(entry);
    let entry = entry.split_once("://").map_or(entry, |(_, rest)| rest);
    entry
        .trim_end_matches('/')
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect()
}

/// The settings in one plain sentence (no credentials, no script contents).
pub fn describe_settings(settings: &ProxySettings) -> String {
    let mut parts = Vec::new();
    if !settings.static_proxy.trim().is_empty() {
        let hosts: Vec<String> = settings
            .static_proxy
            .split(';')
            .map(shown_host)
            .filter(|h| !h.is_empty())
            .collect();
        parts.push(format!("proxy fixo ({})", hosts.join("; ")));
    }
    if !settings.pac_url.trim().is_empty() {
        let host = Url::parse(settings.pac_url.trim())
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| "endereço não reconhecido".into());
        parts.push(format!("configuração automática por script, em {host}"));
    }
    if settings.auto_detect {
        parts.push("detecção automática de proxy da rede".to_owned());
    }
    if parts.is_empty() {
        "nenhum proxy configurado".to_owned()
    } else {
        parts.join(" e ")
    }
}

/// Reads one proxy out of a list in either of the two forms the system
/// uses: a PAC answer ("PROXY a:8080; PROXY b:80; DIRECT", "DIRECT") or the
/// WinHTTP list ("a:8080", "http=a:80;https=b:8080"). The first entry that
/// applies wins; `DIRECT` first (or nothing usable) means no proxy. SOCKS
/// entries are not supported and skipped. `https` is the scheme of the
/// address being reached.
pub fn parse_proxy_list(list: &str, https: bool) -> Option<Url> {
    let wanted = if https { "https" } else { "http" };
    let entries: Vec<&str> = list
        .split([';', ',', '\n', '\r'])
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect();
    // WinHTTP's "scheme=host:port" entries: the one for this scheme.
    let keyed = |entry: &str| {
        let (key, value) = entry.split_once('=')?;
        (!key.is_empty()
            && key.chars().all(|c| c.is_ascii_alphabetic())
            && !value.trim().is_empty())
        .then(|| (key.to_lowercase(), value.trim().to_owned()))
    };
    if entries.iter().any(|e| keyed(e).is_some()) {
        return entries
            .iter()
            .filter_map(|e| keyed(e))
            .find(|(key, _)| key == wanted)
            .and_then(|(_, value)| proxy_url("http", &value));
    }
    for entry in entries {
        let (word, rest) = entry
            .split_once(char::is_whitespace)
            .map_or((entry, ""), |(w, r)| (w, r.trim()));
        match word.to_uppercase().as_str() {
            "DIRECT" => return None,
            "PROXY" => {
                if let Some(url) = proxy_url("http", rest) {
                    return Some(url);
                }
            }
            "HTTPS" => {
                if let Some(url) = proxy_url("https", rest) {
                    return Some(url);
                }
            }
            "SOCKS" | "SOCKS4" | "SOCKS5" => {}
            _ => {
                if let Some(url) = proxy_url("http", entry) {
                    return Some(url);
                }
            }
        }
    }
    None
}

/// `host[:port]` (or with a scheme) as a proxy address, credentials dropped.
fn proxy_url(default_scheme: &str, value: &str) -> Option<Url> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let text = if value.contains("://") {
        value.to_owned()
    } else {
        format!("{default_scheme}://{value}")
    };
    let mut url = Url::parse(&text).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return None;
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Some(url)
}

fn is_local_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A proxy given by the environment: reqwest reads it, so nothing is
/// resolved here.
fn env_proxy_set(https: bool) -> bool {
    let names: [&str; 4] = if https {
        ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    names
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|v| !v.trim().is_empty()))
}

fn proxy_cache() -> &'static Mutex<HashMap<String, Option<Url>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Url>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The proxy the system's automatic configuration gives for `url`, cached by
/// host for the session; `None` goes direct (also on any failure, and for a
/// static proxy or the variables, which reqwest handles).
fn proxy_for(url: &Url) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    if is_local_host(host) || env_proxy_set(url.scheme() == "https") {
        return None;
    }
    let key = format!(
        "{}://{}:{}",
        url.scheme(),
        host.to_lowercase(),
        url.port_or_known_default().unwrap_or(0)
    );
    if let Ok(cache) = proxy_cache().lock() {
        if let Some(found) = cache.get(&key) {
            return found.clone();
        }
    }
    let found = system_proxy_for(url);
    if let Ok(mut cache) = proxy_cache().lock() {
        cache.insert(key, found.clone());
    }
    found
}

/// What the system would use for `url`, as shown in the report: the proxy
/// host, "direct", or why it could not be told.
pub fn system_proxy_text(url: &Url) -> String {
    match system_settings() {
        None => "não foi possível consultar o Windows".to_owned(),
        Some(settings) if !settings.needs_resolution() => {
            if settings.static_proxy.trim().is_empty() {
                "nenhum (conexão direta)".to_owned()
            } else {
                "o proxy fixo informado acima".to_owned()
            }
        }
        Some(_) => match proxy_for(url) {
            Some(proxy) => format!("proxy {}", shown_host(proxy.as_str())),
            None => "nenhum (conexão direta) ou o Windows não respondeu a tempo".to_owned(),
        },
    }
}

#[cfg(windows)]
fn system_settings() -> Option<ProxySettings> {
    static SETTINGS: OnceLock<Option<ProxySettings>> = OnceLock::new();
    SETTINGS.get_or_init(winhttp::ie_settings).clone()
}

#[cfg(not(windows))]
fn system_settings() -> Option<ProxySettings> {
    Some(ProxySettings::default())
}

/// Asks the system, on a helper thread so that a lookup that hangs costs at
/// most [`PROXY_LOOKUP_LIMIT`]; the helper is left to finish by itself.
#[cfg(windows)]
fn system_proxy_for(url: &Url) -> Option<Url> {
    let settings = system_settings()?;
    if !settings.needs_resolution() {
        return None;
    }
    let target = url.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("ponte-proxy".into())
        .spawn(move || {
            let _ = sender.send(winhttp::proxy_for_url(&target, &settings));
        })
        .ok()?;
    let list = receiver.recv_timeout(PROXY_LOOKUP_LIMIT).ok()??;
    parse_proxy_list(&list, url.scheme() == "https")
}

#[cfg(not(windows))]
fn system_proxy_for(_url: &Url) -> Option<Url> {
    let _ = PROXY_LOOKUP_LIMIT;
    None
}

/// The one place that talks to WinHTTP (the proxy of the system's automatic
/// configuration). Everything unsafe is here, around the documented calls.
#[cfg(windows)]
mod winhttp {
    use super::ProxySettings;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
    use windows::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpGetIEProxyConfigForCurrentUser, WinHttpGetProxyForUrl,
        WinHttpOpen, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_NO_PROXY,
        WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS,
        WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A,
        WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WINHTTP_PROXY_INFO,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Text of a system-allocated wide string, which is freed here.
    fn take_string(pointer: PWSTR) -> String {
        if pointer.is_null() {
            return String::new();
        }
        // SAFETY: WinHTTP hands out a NUL-terminated UTF-16 string that it
        // allocated with GlobalAlloc; it is read once up to the NUL and
        // freed once, and not used afterwards.
        unsafe {
            let mut length = 0;
            while *pointer.0.add(length) != 0 && length < 8192 {
                length += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(pointer.0, length));
            let _ = GlobalFree(Some(HGLOBAL(pointer.0.cast())));
            text
        }
    }

    /// "Configurações de rede" of the current user (Internet Options).
    pub fn ie_settings() -> Option<ProxySettings> {
        let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
        // SAFETY: `config` is a valid out structure of the documented type.
        unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) }.ok()?;
        Some(ProxySettings {
            auto_detect: config.fAutoDetect.as_bool(),
            pac_url: take_string(config.lpszAutoConfigUrl),
            static_proxy: take_string(config.lpszProxy),
        })
        // The bypass list is freed too (it is not used here: reqwest reads it).
        .inspect(|_| {
            let _ = take_string(config.lpszProxyBypass);
        })
    }

    /// The proxy list WinHTTP resolves for `url` with the script address
    /// and/or automatic detection of `settings`; `None` when it says direct
    /// or cannot tell.
    pub fn proxy_for_url(url: &str, settings: &ProxySettings) -> Option<String> {
        let agent = wide("Ponte");
        // SAFETY: plain documented calls with valid, NUL-terminated buffers
        // that outlive them; the session handle is closed before returning.
        unsafe {
            let session = WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            );
            if session.is_null() {
                return None;
            }
            let _ = WinHttpSetTimeouts(session, 2000, 2000, 2000, 2000);
            let target = wide(url);
            let pac = wide(settings.pac_url.trim());
            let mut attempts: Vec<WINHTTP_AUTOPROXY_OPTIONS> = Vec::new();
            if !settings.pac_url.trim().is_empty() {
                attempts.push(WINHTTP_AUTOPROXY_OPTIONS {
                    dwFlags: WINHTTP_AUTOPROXY_CONFIG_URL,
                    lpszAutoConfigUrl: PCWSTR(pac.as_ptr()),
                    fAutoLogonIfChallenged: true.into(),
                    ..Default::default()
                });
            }
            if settings.auto_detect {
                attempts.push(WINHTTP_AUTOPROXY_OPTIONS {
                    dwFlags: WINHTTP_AUTOPROXY_AUTO_DETECT,
                    dwAutoDetectFlags: WINHTTP_AUTO_DETECT_TYPE_DHCP
                        | WINHTTP_AUTO_DETECT_TYPE_DNS_A,
                    fAutoLogonIfChallenged: true.into(),
                    ..Default::default()
                });
            }
            let mut found = None;
            for mut options in attempts {
                let mut info = WINHTTP_PROXY_INFO::default();
                if WinHttpGetProxyForUrl(session, PCWSTR(target.as_ptr()), &mut options, &mut info)
                    .is_ok()
                {
                    let list = take_string(info.lpszProxy);
                    let _ = take_string(info.lpszProxyBypass);
                    if info.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY {
                        found = Some(String::from("DIRECT"));
                    } else if !list.trim().is_empty() {
                        found = Some(list);
                    }
                    break;
                }
            }
            let _ = WinHttpCloseHandle(session);
            found
        }
    }
}

// ---------------------------------------------------------------------------
// What failed

/// Why a connection could not be made, as far as the error chain says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetFailure {
    /// The name was not resolved.
    Dns,
    /// Refused, blocked or reset: a firewall, an antivirus, a closed port.
    Blocked,
    /// The certificate of the server is not trusted: something is inspecting
    /// the secure connections.
    Certificate,
    /// The proxy of the network asks for a sign-in.
    ProxyAuth,
    /// No answer in time.
    Timeout,
    Other,
}

/// The texts of an error and of everything under it.
pub fn chain_texts(error: &(dyn std::error::Error + 'static)) -> Vec<String> {
    let mut texts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        texts.push(cause.to_string());
        source = cause.source();
    }
    texts
}

/// Classifies an error chain (the texts of [`chain_texts`], plus the
/// Debug text when it says more) and whether the library called it a
/// timeout. Order matters: a refused certificate is told before anything
/// else, because its chain also says "error sending request".
pub fn classify_texts(texts: &[String], timed_out: bool) -> NetFailure {
    let text = texts.join(" | ").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&[
        "unknownissuer",
        "unknown issuer",
        "invalid peer certificate",
        "invalidcertificate",
        "certificate verify failed",
        "badcertificate",
        "bad certificate",
        "certificate is not trusted",
        "not valid for name",
        "notvalidforname",
        "self-signed",
        "self signed",
        "untrusted",
        "certificate expired",
        "certexpired",
    ]) {
        return NetFailure::Certificate;
    }
    if has(&[
        "proxy authentication required",
        "proxy authentication",
        "status 407",
        " 407 ",
    ]) {
        return NetFailure::ProxyAuth;
    }
    if has(&[
        "dns error",
        "failed to lookup address",
        "no such host",
        "nodename nor servname",
        "name or service not known",
        "temporary failure in name resolution",
        "os error 11001",
        "os error 11004",
        "os error 11002",
        "não é conhecido",
        "host desconhecido",
    ]) {
        return NetFailure::Dns;
    }
    if timed_out || has(&["timed out", "os error 10060", "deadline has elapsed"]) {
        // 10060 on a connect is the silence of a firewall; the person
        // reads it as "no answer", with the same advice.
        return NetFailure::Timeout;
    }
    if has(&[
        "os error 10061",
        "os error 10013",
        "os error 10051",
        "os error 10065",
        "os error 10054",
        "os error 10053",
        "connection refused",
        "actively refused",
        "forbidden by its access permissions",
        "connection reset",
        "network is unreachable",
        "no route to host",
        "recusou",
    ]) {
        return NetFailure::Blocked;
    }
    NetFailure::Other
}

/// Classification of a reqwest error.
pub fn classify(error: &reqwest::Error) -> NetFailure {
    let mut texts = chain_texts(error);
    // The Debug text names the inner TLS error even where Display hides it.
    texts.push(format!("{error:?}"));
    classify_texts(&texts, error.is_timeout())
}

/// A plain sentence for a failure to reach `host`, with what to do.
pub fn describe(failure: NetFailure, host: &str) -> String {
    match failure {
        NetFailure::Dns => format!(
            "O Ponte não achou o endereço de {host}: o nome não foi resolvido. Verifique a conexão com a internet e o DNS da rede; se o computador usa proxy, peça à TI para liberar {host}."
        ),
        NetFailure::Blocked => format!(
            "A rede recusou ou bloqueou a conexão do Ponte com {host}. É comum um firewall ou antivírus liberar o navegador e não o Ponte: peça à TI para liberar o programa do Ponte para {host}, porta 443."
        ),
        NetFailure::Certificate => format!(
            "Um antivírus ou a rede está inspecionando as conexões seguras e o certificado de {host} não é aceito pelo Ponte. Peça à TI para instalar no Windows o certificado dessa inspeção (o Ponte confia no que o Windows confia) ou para liberar {host} da inspeção."
        ),
        NetFailure::ProxyAuth => format!(
            "O proxy da rede exige identificação que o Ponte não sabe fornecer. Peça à TI para liberar {host} no proxy sem identificação."
        ),
        NetFailure::Timeout => format!(
            "{host} não respondeu a tempo. Verifique a conexão com a internet; se a rede usa proxy ou firewall, peça à TI para liberar {host}."
        ),
        NetFailure::Other => format!(
            "O Ponte não conseguiu abrir a conexão com {host}. Verifique a internet e se um antivírus, firewall ou proxy está bloqueando o Ponte."
        ),
    }
}

/// Whether trying the same connection again soon could work: not a refused
/// certificate nor a proxy that wants a sign-in.
pub fn worth_retrying(failure: NetFailure) -> bool {
    !matches!(failure, NetFailure::Certificate | NetFailure::ProxyAuth)
}

/// The technical cause, short, for a muted second line: host names and the
/// texts of the operating system and of the secure connection, at most 200
/// characters. Addresses lose their query (it could carry a code); bodies and
/// credentials never appear in these errors.
pub fn technical_detail(error: &(dyn std::error::Error + 'static)) -> String {
    let texts = chain_texts(error);
    let joined = texts.join(" › ");
    let cleaned: String = joined
        .split_whitespace()
        .map(|word| match word.find('?') {
            Some(at) if word.contains("http") => format!("{}…", &word[..at]),
            _ => word.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned: String = cleaned.chars().filter(|c| !c.is_control()).collect();
    if cleaned.chars().count() > 200 {
        cleaned.chars().take(199).collect::<String>() + "…"
    } else {
        cleaned
    }
}

/// The sentence of [`describe`] and, on a second line, the technical cause.
pub fn explain(error: &reqwest::Error, host: &str) -> String {
    format!(
        "{}\nDetalhe técnico: {}",
        describe(classify(error), host),
        technical_detail(error)
    )
}

/// Host of the address a reqwest error was about, else `fallback`.
pub fn host_of(error: &reqwest::Error, fallback: &str) -> String {
    error
        .url()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| fallback.to_owned())
}

/// Says (second line) that a proxy asked for a sign-in, from a status.
pub fn proxy_auth_status_message(host: &str) -> String {
    describe(NetFailure::ProxyAuth, host)
}

// ---------------------------------------------------------------------------
// The test of the connection

/// The report of [`diagnose`]: plain Portuguese, safe to copy and send.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetReport {
    pub lines: Vec<String>,
    pub verdict: String,
}

impl NetReport {
    /// Everything as one text, for the clipboard.
    pub fn text(&self) -> String {
        let mut text = self.lines.join("\n");
        text.push_str("\n\nConclusão: ");
        text.push_str(&self.verdict);
        text
    }
}

/// Direct TCP connection to port 443 of one address family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tcp {
    /// No address of this family.
    NoAddress,
    Ok(u128),
    Failed(NetFailure, Option<i32>),
}

/// What was found for one host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostResult {
    pub host: String,
    /// Addresses found (v4, v6), or the failure of the name resolution.
    pub names: Result<(usize, usize), NetFailure>,
    pub tcp4: Tcp,
    pub tcp6: Tcp,
    /// What the system would use, in words.
    pub proxy: String,
    /// The HTTP status the host answered (any status is "reached"), or why not.
    pub https: Result<u16, NetFailure>,
    /// Technical cause of a failed HTTPS request, short.
    pub detail: String,
}

fn tcp_text(tcp: &Tcp) -> String {
    match tcp {
        Tcp::NoAddress => "sem endereço desse tipo".to_owned(),
        Tcp::Ok(ms) => format!("conectou em {ms} ms"),
        Tcp::Failed(failure, code) => format!(
            "falhou ({}{})",
            match failure {
                NetFailure::Blocked => "recusada ou bloqueada",
                NetFailure::Timeout => "sem resposta em 5 s",
                NetFailure::Dns => "nome não resolvido",
                _ => "erro de rede",
            },
            code.map_or(String::new(), |c| format!(", erro do sistema {c}"))
        ),
    }
}

/// The lines of one host's result.
pub fn host_lines(result: &HostResult) -> Vec<String> {
    let mut lines = vec![result.host.clone()];
    lines.push(match &result.names {
        Ok((v4, v6)) => format!(
            "  Nome encontrado: {} endereço(s) (IPv4: {v4}, IPv6: {v6})",
            v4 + v6
        ),
        Err(failure) => format!("  Nome não encontrado ({})", short_cause(*failure)),
    });
    lines.push(format!(
        "  Conexão direta, IPv4, porta 443: {}",
        tcp_text(&result.tcp4)
    ));
    lines.push(format!(
        "  Conexão direta, IPv6, porta 443: {}",
        tcp_text(&result.tcp6)
    ));
    lines.push(format!("  Proxy que o Windows indicaria: {}", result.proxy));
    lines.push(match &result.https {
        Ok(status) => format!(
            "  Acesso seguro (HTTPS) pelo Ponte: chegou, o servidor respondeu (código {status})"
        ),
        Err(failure) => {
            let mut line = format!(
                "  Acesso seguro (HTTPS) pelo Ponte: falhou ({})",
                short_cause(*failure)
            );
            if !result.detail.is_empty() {
                line.push_str(&format!(" — {}", result.detail));
            }
            line
        }
    });
    lines
}

fn short_cause(failure: NetFailure) -> &'static str {
    match failure {
        NetFailure::Dns => "nome não resolvido",
        NetFailure::Blocked => "conexão recusada ou bloqueada",
        NetFailure::Certificate => {
            "certificado não aceito: a rede ou um antivírus inspeciona as conexões seguras"
        }
        NetFailure::ProxyAuth => "o proxy exige identificação",
        NetFailure::Timeout => "sem resposta a tempo",
        NetFailure::Other => "outro erro de conexão",
    }
}

/// The conclusion for the results: what works, or the most telling failure
/// with what to do.
pub fn verdict(results: &[HostResult]) -> String {
    let failed: Vec<(&HostResult, NetFailure)> = results
        .iter()
        .filter_map(|r| match (&r.https, &r.names) {
            (Err(failure), _) => Some((r, *failure)),
            (Ok(_), _) => None,
        })
        .collect();
    if results.is_empty() {
        return "Nenhum endereço foi testado.".to_owned();
    }
    if failed.is_empty() {
        return "A conexão com a OpenAI funciona neste computador: os dois endereços responderam pelo mesmo caminho que o Ponte usa. Se o login ou a IA ainda falharem, tente de novo em alguns minutos; se continuar, envie este relatório a quem cuida do Ponte.".to_owned();
    }
    let rank = |failure: NetFailure| match failure {
        NetFailure::Certificate => 0,
        NetFailure::ProxyAuth => 1,
        NetFailure::Dns => 2,
        NetFailure::Blocked => 3,
        NetFailure::Timeout => 4,
        NetFailure::Other => 5,
    };
    let (result, failure) = failed
        .iter()
        .min_by_key(|(_, failure)| rank(*failure))
        .copied()
        .expect("there is a failure");
    let others = failed.len() - 1;
    let mut text = describe(failure, &result.host);
    if others > 0 {
        text.push_str(" O outro endereço da OpenAI também não respondeu.");
    }
    text.push_str(" Envie este relatório a quem cuida do Ponte, se precisar de ajuda.");
    text
}

/// Runs `job` on a thread and waits at most `limit` for it.
fn within<T: Send + 'static>(
    limit: Duration,
    job: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("ponte-diag".into())
        .spawn(move || {
            let _ = sender.send(job());
        })
        .ok()?;
    receiver.recv_timeout(limit).ok()
}

fn tcp_try(address: SocketAddr) -> Tcp {
    let started = Instant::now();
    match TcpStream::connect_timeout(&address, TCP_LIMIT) {
        Ok(_) => Tcp::Ok(started.elapsed().as_millis()),
        Err(error) => {
            let code = error.raw_os_error();
            let text = format!("{error}");
            let timed = error.kind() == std::io::ErrorKind::TimedOut;
            Tcp::Failed(
                match classify_texts(&[text], timed) {
                    NetFailure::Other => NetFailure::Blocked,
                    other => other,
                },
                code,
            )
        }
    }
}

fn check_host(host: &str) -> HostResult {
    let name = host.to_owned();
    let resolved = within(DNS_LIMIT, move || {
        (name.as_str(), 443)
            .to_socket_addrs()
            .map(|found| found.collect::<Vec<_>>())
            .map_err(|error| classify_texts(&[error.to_string(), format!("{error:?}")], false))
    });
    let (names, tcp4, tcp6) = match resolved {
        Some(Ok(addresses)) => {
            let v4: Vec<SocketAddr> = addresses
                .iter()
                .copied()
                .filter(SocketAddr::is_ipv4)
                .collect();
            let v6: Vec<SocketAddr> = addresses
                .iter()
                .copied()
                .filter(SocketAddr::is_ipv6)
                .collect();
            let counts = (v4.len(), v6.len());
            // One address of each kind is enough to see whether the way is open.
            let first = |list: &[SocketAddr]| match list.first() {
                Some(address) => {
                    let address = *address;
                    within(TCP_LIMIT + Duration::from_secs(1), move || tcp_try(address))
                        .unwrap_or(Tcp::Failed(NetFailure::Timeout, None))
                }
                None => Tcp::NoAddress,
            };
            (Ok(counts), first(&v4), first(&v6))
        }
        Some(Err(failure)) => (Err(failure), Tcp::NoAddress, Tcp::NoAddress),
        None => (Err(NetFailure::Timeout), Tcp::NoAddress, Tcp::NoAddress),
    };
    let address = format!("https://{host}/");
    let proxy = Url::parse(&address)
        .map(|url| system_proxy_text(&url))
        .unwrap_or_default();
    let (https, detail) = match client_builder()
        .connect_timeout(HTTPS_CONNECT_LIMIT)
        .timeout(HTTPS_LIMIT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => match client.get(&address).send() {
            Ok(response) => (Ok(response.status().as_u16()), String::new()),
            Err(error) => {
                let failure = classify(&error);
                (Err(failure), technical_detail(&error))
            }
        },
        Err(error) => (Err(NetFailure::Other), technical_detail(&error)),
    };
    HostResult {
        host: host.to_owned(),
        names,
        tcp4,
        tcp6,
        proxy,
        https,
        detail,
    }
}

/// Tests the way to each host, and writes it up. Blocks for up to some
/// tens of seconds: run it on a worker thread. Nothing here sends a token,
/// an account or any content: only a plain `GET /` to the host.
pub fn diagnose(hosts: &[&str]) -> NetReport {
    let mut lines = vec![format!(
        "Teste da conexão — Ponte {}",
        env!("CARGO_PKG_VERSION")
    )];
    lines.push(format!(
        "Proxy do Windows: {}",
        match system_settings() {
            Some(settings) => describe_settings(&settings),
            None => "não foi possível consultar".to_owned(),
        }
    ));
    let variables: Vec<String> = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map(|v| format!("{name} ({})", shown_host(&v)))
        })
        .collect();
    lines.push(format!(
        "Variáveis de proxy: {}",
        if variables.is_empty() {
            "nenhuma".to_owned()
        } else {
            variables.join(", ")
        }
    ));
    lines.push(format!(
        "Certificados do Windows aceitos além dos do Ponte: {}",
        native_roots().len()
    ));
    let mut results = Vec::new();
    for host in hosts {
        let result = check_host(host);
        lines.extend(host_lines(&result));
        results.push(result);
    }
    NetReport {
        verdict: verdict(&results),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_failed_connection_is_told_for_what_it_was() {
        use NetFailure::*;
        for (chain, timed, expected) in [
            (&["error sending request for url (https://auth.openai.com/x)", "client error (Connect)", "invalid peer certificate: UnknownIssuer"][..], false, Certificate),
            (&["error sending request", "client error (Connect)", "invalid peer certificate: NotValidForName"][..], false, Certificate),
            (&["error trying to connect: certificate verify failed: self-signed certificate in certificate chain"][..], false, Certificate),
            (&["error sending request", "client error (Connect)", "proxy authentication required"][..], false, ProxyAuth),
            (&["error sending request", "client error (Connect)", "dns error", "failed to lookup address information: No such host is known. (os error 11001)"][..], false, Dns),
            (&["client error (Connect)", "dns error: Name or service not known"][..], false, Dns),
            (&["client error (Connect)", "tcp connect error", "Nenhuma conexão pôde ser feita porque o computador de destino as recusou ativamente. (os error 10061)"][..], false, Blocked),
            (&["client error (Connect)", "Uma tentativa de acesso a um soquete foi feita de uma maneira proibida por suas permissões de acesso. (os error 10013)"][..], false, Blocked),
            (&["client error (Connect)", "tcp connect error: Connection refused (os error 111)"][..], false, Blocked),
            (&["connection reset by peer (os error 10054)"][..], false, Blocked),
            (&["error sending request", "operation timed out"][..], true, Timeout),
            (&["client error (Connect)", "tcp connect error", "A tentativa de conexão falhou (os error 10060)"][..], false, Timeout),
            (&["something else entirely"][..], false, Other),
            (&[][..], false, Other),
        ] {
            assert_eq!(classify_texts(&texts(chain), timed), expected, "{chain:?}");
        }
        // The certificate wins over the generic "error sending request".
        assert_eq!(
            classify_texts(
                &texts(&["error sending request", "UnknownIssuer", "timed out"]),
                true
            ),
            Certificate
        );
    }

    #[test]
    fn every_failure_has_its_sentence_with_what_to_do() {
        use NetFailure::*;
        let host = "auth.openai.com";
        for failure in [Dns, Blocked, Certificate, ProxyAuth, Timeout, Other] {
            let text = describe(failure, host);
            assert!(text.contains(host), "{text}");
            assert!(text.contains("TI") || text.contains("Verifique"), "{text}");
        }
        assert!(describe(Certificate, host).contains("inspecionando as conexões seguras"));
        assert!(
            describe(ProxyAuth, host)
                .contains("o proxy da rede exige identificação que o Ponte não sabe fornecer")
                || describe(ProxyAuth, host)
                    .contains("proxy da rede exige identificação que o Ponte não sabe fornecer")
        );
        assert!(describe(Blocked, host).contains("liberar o programa do Ponte"));
        assert!(describe(Dns, host).contains("nome não foi resolvido"));
        assert!(
            worth_retrying(Dns)
                && worth_retrying(Blocked)
                && worth_retrying(Timeout)
                && worth_retrying(Other)
        );
        assert!(!worth_retrying(Certificate) && !worth_retrying(ProxyAuth));
    }

    #[test]
    fn the_technical_line_is_short_and_carries_no_query_or_control_characters() {
        #[derive(Debug)]
        struct Cause(String);
        impl std::fmt::Display for Cause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl std::error::Error for Cause {}
        let long = format!("{}\u{7}fim", "x".repeat(400));
        let detail = technical_detail(&Cause(long));
        assert!(detail.chars().count() <= 200, "{}", detail.chars().count());
        assert!(!detail.contains('\u{7}'));
        let detail = technical_detail(&Cause(
            "error sending request for url (https://auth.openai.com/api?code=SEGREDO123&state=S)"
                .into(),
        ));
        assert!(
            detail.contains("auth.openai.com") && !detail.contains("SEGREDO123"),
            "{detail}"
        );
    }

    #[test]
    fn proxy_lists_are_read_in_both_forms() {
        let host = |list: &str, https: bool| parse_proxy_list(list, https).map(|u| u.to_string());
        // PAC answers.
        assert_eq!(
            host("PROXY proxy.exemplo.org:8080", true).as_deref(),
            Some("http://proxy.exemplo.org:8080/")
        );
        assert_eq!(
            host("PROXY proxy.exemplo.org:8080; DIRECT", true).as_deref(),
            Some("http://proxy.exemplo.org:8080/")
        );
        assert_eq!(
            host(
                "PROXY a.exemplo.org:80; PROXY b.exemplo.org:81; DIRECT",
                false
            )
            .as_deref(),
            Some("http://a.exemplo.org/")
        );
        assert_eq!(host("DIRECT", true), None);
        assert_eq!(
            host("DIRECT; PROXY a.exemplo.org:80", true),
            None,
            "the first entry decides"
        );
        assert_eq!(
            host("SOCKS s.exemplo.org:1080; PROXY a.exemplo.org:3128", true).as_deref(),
            Some("http://a.exemplo.org:3128/")
        );
        assert_eq!(
            host("HTTPS seguro.exemplo.org:443", true).as_deref(),
            Some("https://seguro.exemplo.org/")
        );
        assert_eq!(
            host("proxy proxy.exemplo.org:8080", true).as_deref(),
            Some("http://proxy.exemplo.org:8080/")
        );
        // WinHTTP lists.
        assert_eq!(
            host("proxy.exemplo.org:8080", true).as_deref(),
            Some("http://proxy.exemplo.org:8080/")
        );
        assert_eq!(
            host("a.exemplo.org:80;b.exemplo.org:81", true).as_deref(),
            Some("http://a.exemplo.org/")
        );
        assert_eq!(
            host("http=a.exemplo.org:80;https=b.exemplo.org:8443", true).as_deref(),
            Some("http://b.exemplo.org:8443/")
        );
        assert_eq!(
            host("http=a.exemplo.org:80;https=b.exemplo.org:8443", false).as_deref(),
            Some("http://a.exemplo.org/")
        );
        assert_eq!(
            host("http=a.exemplo.org:80", true),
            None,
            "no entry for this scheme"
        );
        assert_eq!(
            host("http://proxy.exemplo.org:3128", true).as_deref(),
            Some("http://proxy.exemplo.org:3128/")
        );
        // Credentials are never kept.
        assert_eq!(
            host("usuario:senha@proxy.exemplo.org:8080", true).as_deref(),
            Some("http://proxy.exemplo.org:8080/")
        );
        // Garbage gives nothing.
        for garbage in [
            "",
            "   ",
            ";;;",
            "PROXY",
            "PROXY ",
            "???",
            "ftp://x.exemplo.org",
            "PROXY :80",
            "SOCKS5 s.exemplo.org:1080",
            "DIRECT;",
            "\u{0}\u{1}",
        ] {
            assert_eq!(host(garbage, true), None, "{garbage:?}");
        }
    }

    #[test]
    fn the_settings_are_told_without_credentials_or_script_contents() {
        assert_eq!(
            describe_settings(&ProxySettings::default()),
            "nenhum proxy configurado"
        );
        let text = describe_settings(&ProxySettings {
            static_proxy: "usuario:senha@proxy.exemplo.org:8080".into(),
            ..Default::default()
        });
        assert!(
            text.contains("proxy.exemplo.org:8080")
                && !text.contains("senha")
                && !text.contains("usuario"),
            "{text}"
        );
        let text = describe_settings(&ProxySettings {
            auto_detect: true,
            pac_url: "http://config.exemplo.org/wpad.dat?token=segredo".into(),
            static_proxy: String::new(),
        });
        assert!(
            text.contains("config.exemplo.org")
                && text.contains("detecção automática")
                && !text.contains("segredo"),
            "{text}"
        );
        let keyed = describe_settings(&ProxySettings {
            static_proxy: "http=a.exemplo.org:80;https=b.exemplo.org:8443".into(),
            ..Default::default()
        });
        assert!(keyed.contains("a.exemplo.org:80") && keyed.contains("b.exemplo.org:8443"));
        assert!(ProxySettings {
            auto_detect: true,
            ..Default::default()
        }
        .needs_resolution());
        assert!(!ProxySettings {
            static_proxy: "a:1".into(),
            ..Default::default()
        }
        .needs_resolution());
    }

    fn reached(host: &str) -> HostResult {
        HostResult {
            host: host.into(),
            names: Ok((2, 1)),
            tcp4: Tcp::Ok(80),
            tcp6: Tcp::NoAddress,
            proxy: "nenhum (conexão direta)".into(),
            https: Ok(404),
            detail: String::new(),
        }
    }

    #[test]
    fn the_report_is_plain_and_the_verdict_names_the_most_telling_failure() {
        let ok = [reached("auth.openai.com"), reached("api.openai.com")];
        let lines = host_lines(&ok[0]).join("\n");
        for needed in [
            "auth.openai.com",
            "3 endereço(s)",
            "IPv4: 2, IPv6: 1",
            "conectou em 80 ms",
            "sem endereço desse tipo",
            "Proxy que o Windows indicaria: nenhum",
            "chegou",
            "código 404",
        ] {
            assert!(lines.contains(needed), "{lines}");
        }
        assert!(verdict(&ok).contains("funciona neste computador"));
        // One host failing on the certificate: that is told, with what to do.
        let mut bad = ok.clone();
        bad[0].https = Err(NetFailure::Certificate);
        bad[0].detail = "UnknownIssuer".into();
        bad[1].https = Err(NetFailure::Timeout);
        let said = verdict(&bad);
        assert!(
            said.contains("inspecionando as conexões seguras")
                && said.contains("auth.openai.com")
                && said.contains("também não respondeu"),
            "{said}"
        );
        let line = host_lines(&bad[0]).join("\n");
        assert!(
            line.contains("falhou") && line.contains("UnknownIssuer"),
            "{line}"
        );
        // Blocked on the TCP level, and a name that does not resolve.
        let mut blocked = reached("api.openai.com");
        blocked.tcp4 = Tcp::Failed(NetFailure::Blocked, Some(10061));
        blocked.names = Err(NetFailure::Dns);
        blocked.https = Err(NetFailure::Dns);
        let text = host_lines(&blocked).join("\n");
        assert!(
            text.contains("erro do sistema 10061") && text.contains("Nome não encontrado"),
            "{text}"
        );
        assert!(verdict(&[blocked]).contains("nome não foi resolvido"));
        assert!(verdict(&[]).contains("Nenhum"));
        // The text to copy has everything and the conclusion.
        let report = NetReport {
            lines: vec!["a".into(), "b".into()],
            verdict: "ok".into(),
        };
        assert_eq!(report.text(), "a\nb\n\nConclusão: ok");
        // Nothing of an account or a token is in a report.
        let all = format!("{}{}", lines, said).to_lowercase();
        for secret in ["token", "bearer", "refresh", "@"] {
            assert!(!all.contains(secret), "{secret}");
        }
    }

    #[test]
    fn the_diagnosis_of_a_name_that_does_not_exist_says_so() {
        // `.invalid` never resolves (RFC 2606): no network needed to learn it.
        let report = diagnose(&["ponte-teste.invalid"]);
        let text = report.text();
        assert!(text.contains("ponte-teste.invalid"), "{text}");
        assert!(
            text.contains("Nome não encontrado") || text.contains("falhou"),
            "{text}"
        );
        assert!(!report.verdict.is_empty());
        assert!(text.contains("Proxy do Windows:") && text.contains("Variáveis de proxy:"));
    }

    #[test]
    fn local_addresses_never_go_through_a_proxy() {
        for url in [
            "http://127.0.0.1:11434/api",
            "http://localhost:11434",
            "http://[::1]:1/",
        ] {
            assert_eq!(proxy_for(&Url::parse(url).unwrap()), None);
        }
        assert_eq!(
            proxy_for(&Url::parse("ftp://x.exemplo.org/").unwrap()),
            None
        );
    }

    // A throw-away root and a server certificate for localhost signed by it
    // (made once with openssl, valid for a century; never trusted by anything).
    const TEST_ROOT: &str = "-----BEGIN CERTIFICATE-----
MIIBmTCCAT+gAwIBAgIUQaFTtiJbMDcr6je2UNy2MRsjyBYwCgYIKoZIzj0EAwIw
GTEXMBUGA1UEAwwOUG9udGUgVGVzdGUgQ0EwIBcNMjYxMTEzMTQxNDQ3WhgPMjEy
NjEwMjAxNDE0NDdaMBkxFzAVBgNVBAMMDlBvbnRlIFRlc3RlIENBMFkwEwYHKoZI
zj0CAQYIKoZIzj0DAQcDQgAEkZDThulJtPkUTQRDYT2BjE+xnF+rStIoNlC0oq99
Yn63xiJjdKOrjtDlLcbv8LCRjl1Wh0CigxD0bXullulmBaNjMGEwHQYDVR0OBBYE
FG9bb/GaU/J5rCEfR4ZVA84FfTvWMB8GA1UdIwQYMBaAFG9bb/GaU/J5rCEfR4ZV
A84FfTvWMA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgIEMAoGCCqGSM49
BAMCA0gAMEUCIQDWTKKKBzMTQS7k13txL98HyWcJxWP8qcORsfC3lB0aQgIgYMPI
mdB96aAJYgUkI3EOz/81/PtsVYGUsSv1SyFakgQ=
-----END CERTIFICATE-----
";
    const TEST_SERVER: &str = "-----BEGIN CERTIFICATE-----
MIIBvDCCAWSgAwIBAgIUbDRs2ZAtTvC/DA0DZGZRhRY6C04wCgYIKoZIzj0EAwIw
GTEXMBUGA1UEAwwOUG9udGUgVGVzdGUgQ0EwIBcNMjYxMTEzMTQxNDQ4WhgPMjEy
NjEwMjAxNDE0NDhaMBQxEjAQBgNVBAMMCWxvY2FsaG9zdDBZMBMGByqGSM49AgEG
CCqGSM49AwEHA0IABBDZL33KQ06MNd4AqH6tIgpRUCBTQlGJt2GZSJ/r+/XtH2XZ
144K66GGq6bPYI+JrkQlhCiItSGKcxIddg0vOo6jgYwwgYkwGgYDVR0RBBMwEYIJ
bG9jYWxob3N0hwR/AAABMBMGA1UdJQQMMAoGCCsGAQUFBwMBMAkGA1UdEwQCMAAw
CwYDVR0PBAQDAgeAMB0GA1UdDgQWBBQUOCZyeLIrWtR9FriNbRq66/YvlDAfBgNV
HSMEGDAWgBRvW2/xmlPyeawhH0eGVQPOBX071jAKBggqhkjOPQQDAgNGADBDAh9+
Nelwvc0cbDahEu8Arp2twN5zxA5ngsARx3i46rFzAiAG2XqQOwK7km64wpHZ59c/
0zp6TqXzh4Co0laD/vyWPA==
-----END CERTIFICATE-----
";
    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgoPzVXZuOqlfwkoAW
hZBnJ3XxDXgHPP6U+dxjev+99s+hRANCAAQQ2S99ykNOjDXeAKh+rSIKUVAgU0JR
ibdhmUif6/v17R9l2deOCuuhhqumz2CPia5EJYQoiLUhinMSHXYNLzqO
-----END PRIVATE KEY-----
";

    /// A TLS server on loopback that answers 200 to `connections` clients.
    fn tls_server(connections: usize) -> (u16, std::thread::JoinHandle<()>) {
        use rustls::pki_types::pem::PemObject;
        use std::io::{Read, Write};
        let certs: Vec<_> =
            rustls::pki_types::CertificateDer::pem_slice_iter(TEST_SERVER.as_bytes())
                .collect::<Result<_, _>>()
                .unwrap();
        let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(TEST_KEY.as_bytes()).unwrap();
        let config = std::sync::Arc::new(
            rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            for _ in 0..connections {
                let (tcp, _) = listener.accept().unwrap();
                tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let connection = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut stream = rustls::StreamOwned::new(connection, tcp);
                let mut buffer = [0_u8; 2048];
                // A client that refuses the certificate ends the handshake here.
                if stream.read(&mut buffer).is_ok() {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK
Content-Length: 2
Connection: close

ok",
                    );
                    let _ = stream.flush();
                }
            }
        });
        (port, handle)
    }

    #[test]
    fn a_certificate_nobody_trusts_is_refused_and_told_so_and_a_root_we_add_is_accepted() {
        let (port, server) = tls_server(2);
        let address = format!("https://localhost:{port}/");
        // The program's own client: bundled roots and the roots of Windows.
        // The root of the test is in neither, so verification must fail.
        let client = client_builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let error = client.get(&address).send().unwrap_err();
        assert!(error.is_connect(), "{error:?}");
        assert_eq!(
            classify(&error),
            NetFailure::Certificate,
            "{:?}",
            chain_texts(&error)
        );
        assert!(!worth_retrying(classify(&error)));
        let said = explain(&error, "localhost");
        assert!(
            said.contains("inspecionando as conexões seguras") && said.contains("Detalhe técnico:"),
            "{said}"
        );
        // The same client with that root added trusts the server: the roots
        // decide, verification is not off. (What the Windows store holds
        // cannot be injected from a test; that path is reasoned, not run.)
        let trusting = client_builder()
            .timeout(Duration::from_secs(10))
            .add_root_certificate(reqwest::Certificate::from_pem(TEST_ROOT.as_bytes()).unwrap())
            .build()
            .unwrap();
        let response = trusting.get(&address).send().unwrap();
        assert_eq!(response.status().as_u16(), 200);
        server.join().unwrap();
    }

    /// By hand: the real report for the OpenAI hosts, printed.
    #[test]
    #[ignore = "Fala com a OpenAI; só para diagnóstico manual"]
    fn live_diagnosis_prints_the_report() {
        let report = diagnose(&OPENAI_HOSTS);
        println!("{}", report.text());
        assert!(!report.verdict.is_empty());
    }
}
