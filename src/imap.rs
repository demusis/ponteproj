//! Reading a mailbox over IMAP (TLS) to import messages as records: list
//! the folders (Gmail labels included), list the newest messages of one
//! folder, fetch the ones the person ticked. Read-only on purpose: messages
//! are fetched with `BODY.PEEK`, so nothing is marked as read, moved or
//! flagged; the only command that writes anything is `LOGOUT`.
//!
//! Accounts are kept sealed in this computer's library only (they are
//! credentials, not team data). A Gmail personal account needs 2-step
//! verification and an *app password*; the person's normal password is
//! never accepted by Gmail for IMAP and is never asked for here.
//!
//! The client speaks the IMAP4rev1 subset it needs: LOGIN, LIST, SELECT,
//! UID SEARCH, UID FETCH with literals, LOGOUT. TLS uses `rustls` with the
//! Mozilla root store, the same components the program already uses for
//! HTTPS; there is no plain-text fallback.
use crate::{
    protection::{seal, unseal},
    storage::Store,
};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 993;
pub const MAX_LIST: usize = 200;
/// A fetched message; anything bigger is left for the `.eml` path.
pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
/// Bytes downloaded in one go; the rest waits for another round, so a
/// hundred large messages do not pile up in memory.
pub const MAX_BATCH_BYTES: usize = 96 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Silence tolerated from the server before giving up.
const IO_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a blocked read wakes up to look at cancel and the deadline.
const POLL: Duration = Duration::from_secs(2);
/// Longest one command may take, however steadily the server trickles
/// (a 32 MB message on a slow line fits).
const COMMAND_DEADLINE: Duration = Duration::from_secs(20 * 60);
/// Most one command's response may bring, every line and literal counted.
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// One mailbox the person reads from.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailAccount {
    #[serde(default)]
    pub uid: String,
    /// How the person calls it ("Gmail pessoal").
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    /// The app password; empty when not remembered (asked each session).
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub remember: bool,
    /// Folder opened by default ("INBOX", "[Gmail]/Com estrela", a label).
    #[serde(default = "default_folder")]
    pub folder: String,
}

/// The password never shows in debug output.
impl std::fmt::Debug for MailAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailAccount")
            .field("uid", &self.uid)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field(
                "password",
                &if self.password.is_empty() { "" } else { "***" },
            )
            .field("remember", &self.remember)
            .field("folder", &self.folder)
            .finish()
    }
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_folder() -> String {
    "INBOX".into()
}

impl MailAccount {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 200,
            "Dê um nome à conta."
        );
        ensure!(
            !self.host.trim().is_empty()
                && self.host.len() <= 253
                && self
                    .host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'),
            "Servidor IMAP inválido (ex.: imap.gmail.com)."
        );
        ensure!(self.port > 0, "Porta inválida.");
        ensure!(
            !self.user.trim().is_empty()
                && self.user.len() <= 320
                && !self.user.chars().any(char::is_control),
            "Informe o usuário (o endereço de e-mail)."
        );
        ensure!(
            self.password.len() <= 1024 && !self.password.chars().any(char::is_control),
            "Senha inválida."
        );
        // IMAP quoted strings are 7-bit; anything else would need a literal.
        ensure!(
            self.user.is_ascii() && self.password.is_ascii(),
            "Usuário e senha de app devem usar só letras, números e sinais comuns."
        );
        ensure!(
            !self.folder.trim().is_empty()
                && self.folder.len() <= 500
                && !self.folder.chars().any(char::is_control),
            "Escolha uma pasta."
        );
        Ok(())
    }

    /// Gmail shows app passwords in groups of four with spaces; the server
    /// wants them without.
    pub fn login_password(&self) -> String {
        if self.host.to_ascii_lowercase().contains("gmail") {
            self.password.chars().filter(|c| *c != ' ').collect()
        } else {
            self.password.clone()
        }
    }
}

/// A folder as the server lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// Name to send back to the server (modified UTF-7, as received).
    pub raw: String,
    /// Name to show (decoded).
    pub name: String,
    /// Cannot hold messages (a parent only, such as "[Gmail]").
    pub no_select: bool,
}

/// Which messages of a folder to list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Filter {
    /// The newest messages.
    All,
    /// Starred (IMAP `\Flagged`).
    Flagged,
    /// Not yet read.
    Unseen,
}

impl Filter {
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "Todas",
            Filter::Flagged => "Com estrela",
            Filter::Unseen => "Não lidas",
        }
    }
    fn search_key(self) -> &'static str {
        match self {
            Filter::All => "ALL",
            Filter::Flagged => "FLAGGED",
            Filter::Unseen => "UNSEEN",
        }
    }
}

/// Fetched messages by uid: the raw bytes, or why one could not be read.
pub type Fetched = Vec<(u32, std::result::Result<Vec<u8>, String>)>;

/// What the list shows about one message before it is fetched.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSummary {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    /// `AAAA-MM-DD` when the Date header parses, else as written.
    pub date: String,
    pub message_id: String,
    pub flagged: bool,
    pub seen: bool,
    pub size: usize,
}

// ---------------------------------------------------------------------------
// Modified UTF-7 (RFC 3501 §5.1.3) for folder names.

fn utf7_decode(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let encoded = &after[..end];
        if encoded.is_empty() {
            out.push('&');
        } else {
            use base64::Engine;
            let standard: String = encoded.replace(',', "/");
            let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(standard.as_bytes())
                .unwrap_or_default();
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_be_bytes(*p))
                .collect();
            out.push_str(&String::from_utf16_lossy(&units));
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
fn utf7_encode(name: &str) -> String {
    use base64::Engine;
    let mut out = String::new();
    let mut run: Vec<u16> = Vec::new();
    let flush = |run: &mut Vec<u16>, out: &mut String| {
        if run.is_empty() {
            return;
        }
        let bytes: Vec<u8> = run.iter().flat_map(|u| u.to_be_bytes()).collect();
        let encoded = base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(bytes)
            .replace('/', ",");
        out.push('&');
        out.push_str(&encoded);
        out.push('-');
        run.clear();
    };
    for c in name.chars() {
        if c == '&' {
            flush(&mut run, &mut out);
            out.push_str("&-");
        } else if (' '..='~').contains(&c) {
            flush(&mut run, &mut out);
            out.push(c);
        } else {
            let mut buffer = [0u16; 2];
            run.extend_from_slice(c.encode_utf16(&mut buffer));
        }
    }
    flush(&mut run, &mut out);
    out
}

// ---------------------------------------------------------------------------
// The connection.

/// The socket as rustls sees it: every read first checks "Cancelar" and the
/// deadline, so a server trickling a TLS record byte by byte (rustls keeps
/// reading until the record is whole) still cannot hold the session.
struct Guarded {
    tcp: TcpStream,
    cancel: Arc<AtomicBool>,
    deadline: Arc<std::sync::Mutex<Instant>>,
}

impl Read for Guarded {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let late = self
            .deadline
            .lock()
            .map(|deadline| Instant::now() >= *deadline)
            .unwrap_or(true);
        if self.cancel.load(Ordering::Relaxed) || late {
            return Err(std::io::Error::other("interrompido"));
        }
        self.tcp.read(buf)
    }
}

impl Write for Guarded {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tcp.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.tcp.flush()
    }
}

struct Session {
    stream: rustls::StreamOwned<rustls::ClientConnection, Guarded>,
    tag: u32,
    buffer: Vec<u8>,
    /// Set by "Cancelar" in the window; looked at while waiting.
    cancel: Arc<AtomicBool>,
    /// When the current command (or the greeting) must be done; shared
    /// with the socket guard.
    deadline: Arc<std::sync::Mutex<Instant>>,
    /// Bytes the current command has received so far.
    received: usize,
}

/// One untagged line of a response, with any literals inlined.
type Line = Vec<u8>;

impl Session {
    fn connect(account: &MailAccount, cancel: Arc<AtomicBool>) -> Result<Self> {
        account.validate()?;
        let addresses: Vec<_> = (account.host.as_str(), account.port)
            .to_socket_addrs()
            .with_context(|| format!("Servidor {} não encontrado.", account.host))?
            .collect();
        ensure!(!addresses.is_empty(), "Servidor sem endereço.");
        // Every address the name resolves to is tried: an IPv6 one without
        // a route must not stop an IPv4 one from working.
        let mut tcp = None;
        let mut last_error = String::from("sem resposta");
        for address in &addresses {
            match TcpStream::connect_timeout(address, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    tcp = Some(stream);
                    break;
                }
                Err(error) => last_error = error.to_string(),
            }
        }
        let tcp = tcp.with_context(|| {
            format!(
                "Não foi possível conectar a {}:{}: {last_error}",
                account.host, account.port
            )
        })?;
        tcp.set_read_timeout(Some(POLL))?;
        tcp.set_write_timeout(Some(IO_TIMEOUT))?;
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        // The provider is named, so a future dependency enabling another
        // one cannot make the default ambiguous.
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .context("Não foi possível preparar a conexão segura.")?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let server_name = rustls::pki_types::ServerName::try_from(account.host.clone())
            .context("Nome do servidor inválido para TLS.")?;
        let connection = rustls::ClientConnection::new(Arc::new(config), server_name)
            .context("Não foi possível preparar a conexão segura.")?;
        let deadline = Arc::new(std::sync::Mutex::new(Instant::now() + IO_TIMEOUT));
        let guarded = Guarded {
            tcp,
            cancel: Arc::clone(&cancel),
            deadline: Arc::clone(&deadline),
        };
        let mut session = Session {
            stream: rustls::StreamOwned::new(connection, guarded),
            tag: 0,
            buffer: Vec::new(),
            cancel,
            deadline,
            received: 0,
        };
        let greeting = session
            .read_line()
            .context("A conexão segura não pôde ser estabelecida")?;
        ensure!(
            greeting.starts_with(b"* OK") || greeting.starts_with(b"* PREAUTH"),
            "O servidor não respondeu como um servidor IMAP."
        );
        Ok(session)
    }

    fn read_line(&mut self) -> Result<Line> {
        loop {
            if let Some(position) = self.buffer.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buffer.drain(..=position).collect();
                while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                    line.pop();
                }
                return Ok(line);
            }
            let mut chunk = [0u8; 16 * 1024];
            let read = self.read_chunk(&mut chunk)?;
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    /// One read from the server. A quiet server is waited for in short
    /// steps, so "Cancelar", the command's deadline and the silence limit
    /// are all honoured; every byte counts against the command's budget.
    fn read_chunk(&mut self, chunk: &mut [u8]) -> Result<usize> {
        let mut silent = Duration::ZERO;
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                bail!("Cancelado por você.");
            }
            ensure!(
                !self.late(),
                "O servidor de e-mail demorou demais para responder."
            );
            match self.stream.read(chunk) {
                Ok(0) => bail!("O servidor de e-mail encerrou a conexão."),
                Ok(read) => {
                    self.received += read;
                    ensure!(
                        self.received <= MAX_RESPONSE_BYTES,
                        "Resposta do servidor grande demais."
                    );
                    return Ok(read);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    silent += POLL;
                    ensure!(
                        silent < IO_TIMEOUT,
                        "O servidor de e-mail parou de responder."
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                // Stopped by the guard: the loop says why.
                Err(_) if self.cancel.load(Ordering::Relaxed) || self.late() => {}
                Err(error) => {
                    return Err(error).context("A conexão com o servidor de e-mail caiu.")
                }
            }
        }
    }

    fn late(&self) -> bool {
        self.deadline
            .lock()
            .map(|deadline| Instant::now() >= *deadline)
            .unwrap_or(true)
    }

    fn read_exact(&mut self, count: usize) -> Result<Vec<u8>> {
        ensure!(
            count <= MAX_MESSAGE_BYTES + 1024 * 1024,
            "Mensagem maior que {} MB.",
            MAX_MESSAGE_BYTES / (1024 * 1024)
        );
        while self.buffer.len() < count {
            let mut chunk = vec![0u8; (count - self.buffer.len()).min(64 * 1024)];
            let read = self.read_chunk(&mut chunk)?;
            self.buffer.extend_from_slice(&chunk[..read]);
        }
        Ok(self.buffer.drain(..count).collect())
    }

    /// Sends a command and returns its untagged lines (literals inlined)
    /// once the tagged status arrives; a NO or BAD is an error with the
    /// server's words.
    fn command(&mut self, command: &str) -> Result<Vec<Line>> {
        self.tag += 1;
        if let Ok(mut deadline) = self.deadline.lock() {
            *deadline = Instant::now() + COMMAND_DEADLINE;
        }
        self.received = 0;
        let tag = format!("P{:04}", self.tag);
        self.stream
            .write_all(format!("{tag} {command}\r\n").as_bytes())
            .context("Não foi possível enviar ao servidor de e-mail.")?;
        let mut lines = Vec::new();
        loop {
            let mut line = self.read_line()?;
            // A literal: "{n}" at the end announces n raw bytes, after which
            // the same response line continues. Only the text after the
            // last literal is looked at: the literal itself is raw bytes.
            let mut text_from = 0;
            while let Some(size) = literal_size(&line[text_from..]) {
                let bytes = self.read_exact(size)?;
                line.extend_from_slice(&bytes);
                text_from = line.len();
                let rest = self.read_line()?;
                line.extend_from_slice(&rest);
            }
            if line.starts_with(tag.as_bytes()) {
                let status = String::from_utf8_lossy(&line[tag.len()..])
                    .trim()
                    .to_owned();
                if status.starts_with("OK") {
                    return Ok(lines);
                }
                let message = status
                    .trim_start_matches("NO")
                    .trim_start_matches("BAD")
                    .trim()
                    .to_owned();
                bail!("{}", friendly(&message));
            }
            if line.starts_with(b"* ") || line.starts_with(b"+") {
                lines.push(line);
            }
        }
    }
}

fn literal_size(line: &[u8]) -> Option<usize> {
    let line = std::str::from_utf8(line).ok()?;
    let open = line.rfind('{')?;
    let inner = line[open + 1..].strip_suffix('}')?;
    inner.parse().ok()
}

/// Server refusals in the person's words.
fn friendly(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("authenticationfailed")
        || lower.contains("invalid credentials")
        || lower.contains("login failed")
    {
        "Usuário ou senha de app recusados. No Gmail, a senha normal não funciona: crie uma senha de app em Conta Google ▸ Segurança ▸ Verificação em duas etapas ▸ Senhas de app.".to_owned()
    } else if lower.contains("application-specific password")
        || lower.contains("web login required")
    {
        "O Gmail exige uma senha de app (Conta Google ▸ Segurança ▸ Verificação em duas etapas ▸ Senhas de app).".to_owned()
    } else {
        format!("O servidor de e-mail recusou: {message}")
    }
}

/// Opens a folder read-only. A name with control characters (a line break
/// would start another command) is refused, whatever the server listed.
fn examine(session: &mut Session, folder: &str) -> Result<()> {
    ensure!(
        !folder.is_empty() && !folder.chars().any(char::is_control),
        "Nome de pasta inválido."
    );
    session.command(&format!("EXAMINE {}", quote(folder)))?;
    Ok(())
}

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Splits an IMAP response line into top-level tokens: atoms, quoted
/// strings and parenthesized lists (kept whole).
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for c in line.chars() {
        if quoted {
            current.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                current.push(c);
            }
            '(' | '[' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(c);
            }
            ' ' if depth <= 0 => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn unquote(token: &str) -> String {
    let Some(inner) = token.strip_prefix('"').and_then(|t| t.strip_suffix('"')) else {
        return token.to_owned();
    };
    let mut out = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn login(session: &mut Session, account: &MailAccount) -> Result<()> {
    ensure!(
        !account.password.trim().is_empty(),
        "Informe a senha de app da conta."
    );
    session
        .command(&format!(
            "LOGIN {} {}",
            quote(account.user.trim()),
            quote(&account.login_password())
        ))
        .map(|_| ())
}

/// The folders of the account (Gmail labels included), selectable first.
pub fn list_folders(account: &MailAccount, reporter: &crate::ai::Reporter) -> Result<Vec<Folder>> {
    let mut session = Session::connect(account, reporter.cancel_flag())?;
    login(&mut session, account)?;
    let lines = session.command("LIST \"\" \"*\"")?;
    let _ = session.command("LOGOUT");
    let mut folders = Vec::new();
    for line in lines {
        let text = String::from_utf8_lossy(&line);
        let parts = tokens(&text);
        // * LIST (\HasNoChildren) "/" "INBOX"
        if parts.len() < 5 || parts[1] != "LIST" {
            continue;
        }
        let flags = parts[2].to_ascii_lowercase();
        let mut raw = unquote(parts.last().unwrap());
        // A name sent as a literal arrives as "{n}name".
        if let Some(rest) = raw.strip_prefix('{') {
            if let Some((digits, name)) = rest.split_once('}') {
                if digits.bytes().all(|b| b.is_ascii_digit()) {
                    raw = name.to_owned();
                }
            }
        }
        if raw.is_empty() || raw.chars().any(char::is_control) {
            continue;
        }
        folders.push(Folder {
            name: utf7_decode(&raw),
            no_select: flags.contains("\\noselect"),
            raw,
        });
    }
    folders.sort_by(|a, b| {
        let rank = |f: &Folder| {
            if f.raw.eq_ignore_ascii_case("INBOX") {
                0
            } else if f.raw.starts_with("[Gmail]") {
                2
            } else {
                1
            }
        };
        rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
    });
    Ok(folders)
}

/// The newest `limit` messages of `folder` that match `filter`, newest
/// first. `folder` is the name as the server lists it (`Folder::raw`).
pub fn list_messages(
    account: &MailAccount,
    folder: &str,
    filter: Filter,
    limit: usize,
    reporter: &crate::ai::Reporter,
) -> Result<Vec<MessageSummary>> {
    let limit = limit.clamp(1, MAX_LIST);
    let mut session = Session::connect(account, reporter.cancel_flag())?;
    login(&mut session, account)?;
    examine(&mut session, folder)?;
    let lines = session.command(&format!("UID SEARCH {}", filter.search_key()))?;
    let mut uids: Vec<u32> = Vec::new();
    for line in &lines {
        let text = String::from_utf8_lossy(line);
        if let Some(rest) = text.strip_prefix("* SEARCH") {
            uids.extend(
                rest.split_whitespace()
                    .filter_map(|n| n.parse::<u32>().ok()),
            );
        }
    }
    uids.sort_unstable();
    let chosen: Vec<u32> = uids.iter().rev().take(limit).copied().collect();
    if chosen.is_empty() {
        let _ = session.command("LOGOUT");
        return Ok(Vec::new());
    }
    let set = chosen
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let lines = session.command(&format!(
        "UID FETCH {set} (UID FLAGS RFC822.SIZE BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID)])"
    ))?;
    let _ = session.command("LOGOUT");
    let mut summaries = Vec::new();
    for line in lines {
        if let Some(summary) = parse_fetch_summary(&line) {
            summaries.push(summary);
        }
    }
    summaries.sort_by_key(|a| std::cmp::Reverse(a.uid));
    Ok(summaries)
}

fn parse_fetch_summary(line: &[u8]) -> Option<MessageSummary> {
    // The headers literal is binary-safe; everything before it is ASCII.
    let open = line.windows(1).position(|w| w == b"{")?;
    let head = std::str::from_utf8(&line[..open]).ok()?;
    if !head.contains(" FETCH ") {
        return None;
    }
    let literal_len: usize = {
        let close = line[open..].iter().position(|&b| b == b'}')? + open;
        std::str::from_utf8(&line[open + 1..close])
            .ok()?
            .parse()
            .ok()?
    };
    let close = line[open..].iter().position(|&b| b == b'}')? + open;
    let end = (close + 1).saturating_add(literal_len).min(line.len());
    let headers = &line[close + 1..end];
    let mut summary = MessageSummary::default();
    // The attributes sit inside the FETCH parenthesized list, before and
    // after the literal (a server may order them as it likes).
    let inner = head
        .split_once("FETCH (")
        .map(|(_, rest)| rest)
        .unwrap_or(head);
    let tail = String::from_utf8_lossy(&line[end..]);
    let attributes = tokens(&format!("{inner} {tail}"));
    let mut iterator = attributes.iter().peekable();
    while let Some(token) = iterator.next() {
        match token.as_str() {
            "UID" => summary.uid = iterator.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "RFC822.SIZE" => {
                summary.size = iterator.next().and_then(|v| v.parse().ok()).unwrap_or(0)
            }
            "FLAGS" => {
                let flags = iterator
                    .next()
                    .cloned()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                summary.flagged = flags.contains("\\flagged");
                summary.seen = flags.contains("\\seen");
            }
            _ => {}
        }
    }
    // A header-only "message" goes through the mail decoder for the
    // encoded words and the date.
    let mut pseudo = headers.to_vec();
    pseudo.extend_from_slice(b"\r\n\r\n");
    if let Ok(parsed) = crate::mail::parse(&pseudo) {
        summary.from = parsed.from;
        summary.subject = parsed.subject;
        summary.date = if parsed.date.is_empty() {
            parsed.date_raw
        } else {
            parsed.date
        };
        summary.message_id = parsed.message_id;
    }
    (summary.uid > 0).then_some(summary)
}

/// Fetches whole messages by UID, reporting progress. Each result is kept
/// apart, so one bad message does not lose the others.
pub fn fetch_messages(
    account: &MailAccount,
    folder: &str,
    uids: &[u32],
    reporter: &crate::ai::Reporter,
) -> Result<Fetched> {
    let mut session = Session::connect(account, reporter.cancel_flag())?;
    login(&mut session, account)?;
    examine(&mut session, folder)?;
    let mut out: Fetched = Vec::new();
    let mut total = 0usize;
    for (index, uid) in uids.iter().copied().enumerate() {
        if reporter.check().is_err() {
            // Cancelled: what was downloaded is kept, the rest is told.
            out.extend(
                uids[index..]
                    .iter()
                    .map(|u| (*u, Err("Cancelado por você.".to_owned()))),
            );
            break;
        }
        if total >= MAX_BATCH_BYTES {
            out.extend(uids[index..].iter().map(|u| {
                (
                    *u,
                    Err("Lote cheio; marque esta mensagem numa próxima leva.".to_owned()),
                )
            }));
            break;
        }
        reporter.step(format!(
            "Baixando mensagem {} de {}…",
            index + 1,
            uids.len()
        ));
        // A transport or protocol error leaves the session out of step:
        // nothing more is read from it, so a later message can never be a
        // fragment of an earlier one.
        let lines = match session.command(&format!("UID FETCH {uid} (UID BODY.PEEK[])")) {
            Ok(lines) => lines,
            Err(error) => {
                out.push((uid, Err(error.to_string())));
                out.extend(uids[index + 1..].iter().map(|u| {
                    (
                        *u,
                        Err("Conexão encerrada depois de um erro; tente de novo.".to_owned()),
                    )
                }));
                return Ok(out);
            }
        };
        match lines.iter().find_map(|line| parse_fetch_body(line, uid)) {
            Some(bytes) if bytes.len() > MAX_MESSAGE_BYTES => out.push((
                uid,
                Err(format!(
                    "Mensagem maior que {} MB.",
                    MAX_MESSAGE_BYTES / (1024 * 1024)
                )),
            )),
            Some(bytes) => {
                total += bytes.len();
                out.push((uid, Ok(bytes)));
            }
            None => out.push((uid, Err("O servidor não devolveu a mensagem.".to_owned()))),
        }
    }
    let _ = session.command("LOGOUT");
    Ok(out)
}

fn parse_fetch_body(line: &[u8], wanted: u32) -> Option<Vec<u8>> {
    let open = line.iter().position(|&b| b == b'{')?;
    let head = std::str::from_utf8(&line[..open]).ok()?;
    if !head.contains(" FETCH ") {
        return None;
    }
    let close = line[open..].iter().position(|&b| b == b'}')? + open;
    let size: usize = std::str::from_utf8(&line[open + 1..close])
        .ok()?
        .parse()
        .ok()?;
    let start = close + 1;
    let end = start.saturating_add(size).min(line.len());
    // The UID is matched exactly, wherever the server put it.
    let inner = head
        .split_once("FETCH (")
        .map(|(_, rest)| rest)
        .unwrap_or(head);
    let tail = String::from_utf8_lossy(&line[end..]);
    let attributes = tokens(&format!("{inner} {tail}"));
    let uid = attributes
        .windows(2)
        .find(|pair| pair[0] == "UID")
        .and_then(|pair| pair[1].trim_end_matches(')').parse::<u32>().ok())?;
    (uid == wanted).then(|| line[start..end].to_vec())
}

// ---------------------------------------------------------------------------
// Accounts, sealed in this library only.

impl Store {
    pub(crate) fn ensure_mail_accounts(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS mail_accounts(uid TEXT PRIMARY KEY, payload BLOB NOT NULL);",
        )?;
        Ok(())
    }

    pub fn list_mail_accounts(&self) -> Result<Vec<MailAccount>> {
        let mut statement = self
            .conn
            .prepare("SELECT uid,payload FROM mail_accounts ORDER BY rowid")?;
        let rows = statement
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (uid, blob) in rows {
            let mut account: MailAccount = serde_json::from_slice(&unseal(&blob)?)?;
            account.uid = uid;
            out.push(account);
        }
        Ok(out)
    }

    /// Saves an account. The password is stored only when `remember` is on.
    pub fn save_mail_account(&mut self, account: &MailAccount) -> Result<MailAccount> {
        account.validate()?;
        let mut stored = account.clone();
        if stored.uid.is_empty() {
            stored.uid = uuid::Uuid::new_v4().simple().to_string();
        }
        let mut on_disk = stored.clone();
        if !on_disk.remember {
            on_disk.password.clear();
        }
        self.conn.execute(
            "INSERT INTO mail_accounts(uid,payload) VALUES(?1,?2)
             ON CONFLICT(uid) DO UPDATE SET payload=excluded.payload",
            params![stored.uid, seal(&serde_json::to_vec(&on_disk)?)?],
        )?;
        Ok(stored)
    }

    pub fn delete_mail_account(&mut self, uid: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM mail_accounts WHERE uid=?1", [uid])?;
        Ok(())
    }

    /// The document that already holds this message (by Message-ID), if any.
    pub fn mail_already_imported(&self, message_id: &str) -> Result<Option<i64>> {
        if message_id.trim().is_empty() {
            return Ok(None);
        }
        let request_id = crate::mail::request_id_for_message_id(message_id);
        Ok(self
            .conn
            .query_row(
                "SELECT document_id FROM requests WHERE request_id=?1",
                [request_id],
                |r| r.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_round_trip_through_modified_utf7() {
        assert_eq!(utf7_decode("[Gmail]/Com estrela"), "[Gmail]/Com estrela");
        assert_eq!(utf7_decode("Rascunhos &- Notas"), "Rascunhos & Notas");
        assert_eq!(utf7_decode("Of&AO0-cios"), "Ofícios");
        assert_eq!(utf7_encode("Ofícios"), "Of&AO0-cios");
        assert_eq!(utf7_encode("A & B"), "A &- B");
        for name in ["Ofícios", "Licitação 2026", "Reunião & Despacho", "INBOX"] {
            assert_eq!(utf7_decode(&utf7_encode(name)), name);
        }
    }

    #[test]
    fn fetch_lines_are_parsed_into_summaries_and_bodies() {
        let headers = b"From: =?UTF-8?Q?Jo=C3=A3o?= <j@x.br>\r\nSubject: Teste\r\nDate: Tue, 3 Nov 2026 10:00:00 -0300\r\nMessage-ID: <m1@x.br>\r\n";
        let mut line = format!(
            "* 7 FETCH (UID 42 FLAGS (\\Seen \\Flagged) RFC822.SIZE 1234 BODY[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID)] {{{}}}",
            headers.len()
        )
        .into_bytes();
        line.extend_from_slice(headers);
        line.extend_from_slice(b")");
        let summary = parse_fetch_summary(&line).unwrap();
        assert_eq!(summary.uid, 42);
        assert!(summary.flagged && summary.seen);
        assert_eq!(summary.size, 1234);
        assert_eq!(summary.from, "João <j@x.br>");
        assert_eq!(summary.subject, "Teste");
        assert_eq!(summary.date, "2026-11-03");
        assert_eq!(summary.message_id, "m1@x.br");

        let body = b"From: a@b\r\nSubject: Oi\r\n\r\nCorpo";
        let mut line = format!("* 7 FETCH (UID 42 BODY[] {{{}}}", body.len()).into_bytes();
        line.extend_from_slice(body);
        line.extend_from_slice(b")");
        assert_eq!(parse_fetch_body(&line, 42).unwrap(), body);
        assert!(parse_fetch_body(&line, 43).is_none());
        assert_eq!(literal_size(b"* 1 FETCH (BODY[] {12}"), Some(12));
        assert_eq!(literal_size(b"P0001 OK done"), None);
    }

    #[test]
    fn hostile_fetch_lines_never_panic() {
        let lines: [&[u8]; 5] = [
            b"* 1 FETCH (UID 5 X {18446744073709551615} BODY[] {10}abc",
            b"* 1 FETCH (UID 5 BODY[] {99999999}short)",
            b"* 1 FETCH (UID 5 BODY[] {}",
            b"* 1 FETCH (UID 5 BODY[] {abc}",
            b"* 1 FETCH {",
        ];
        for line in lines {
            let _ = parse_fetch_summary(line);
            let _ = parse_fetch_body(line, 5);
        }
        assert_eq!(
            parse_fetch_body(b"* 1 FETCH (UID 5 BODY[] {99999999}short)", 5).unwrap(),
            b"short)"
        );
        // The password stays out of debug output.
        let account = MailAccount {
            password: "abcd efgh ijkl mnop".into(),
            ..Default::default()
        };
        assert!(!format!("{account:?}").contains("abcd"));
        // A folder name with a line break is never sent.
        let mut account = MailAccount {
            name: "x".into(),
            host: "imap.exemplo.com".into(),
            port: 993,
            user: "a@b".into(),
            folder: "INBOX\r\nP9 DELETE INBOX".into(),
            ..Default::default()
        };
        assert!(account.validate().is_err());
        account.folder = "INBOX".into();
        assert!(account.validate().is_ok());
    }

    #[test]
    fn tokens_keep_lists_and_quotes_whole() {
        let parts = tokens(r#"* LIST (\HasNoChildren \Flagged) "/" "[Gmail]/Com estrela""#);
        assert_eq!(parts[2], r"(\HasNoChildren \Flagged)");
        assert_eq!(unquote(&parts[4]), "[Gmail]/Com estrela");
        assert_eq!(quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn accounts_are_sealed_and_passwords_only_kept_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let account = MailAccount {
            uid: String::new(),
            name: "Gmail pessoal".into(),
            host: "imap.gmail.com".into(),
            port: 993,
            user: "pessoa@gmail.com".into(),
            password: "abcd efgh ijkl mnop".into(),
            remember: false,
            folder: "INBOX".into(),
        };
        let saved = store.save_mail_account(&account).unwrap();
        assert!(!saved.uid.is_empty());
        assert_eq!(
            saved.password, "abcd efgh ijkl mnop",
            "kept for this session"
        );
        assert_eq!(saved.login_password(), "abcdefghijklmnop");
        let listed = store.list_mail_accounts().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].password.is_empty(), "not remembered on disk");
        let mut remembered = saved.clone();
        remembered.remember = true;
        store.save_mail_account(&remembered).unwrap();
        assert_eq!(
            store.list_mail_accounts().unwrap()[0].password,
            "abcd efgh ijkl mnop"
        );
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        let raw = std::fs::read(dir.path().join("acervo.sqlite3")).unwrap();
        for secret in ["abcd efgh", "pessoa@gmail.com", "Gmail pessoal"] {
            assert!(!raw.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
        store.delete_mail_account(&saved.uid).unwrap();
        assert!(store.list_mail_accounts().unwrap().is_empty());
        assert!(store.mail_already_imported("x@y").unwrap().is_none());
        assert!(MailAccount {
            host: "imap.gmail.com; rm".into(),
            ..account
        }
        .validate()
        .is_err());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Needs the internet: proves the TLS handshake and the greeting parse
    /// against Gmail, without any credentials.
    #[test]
    #[ignore]
    fn gmail_greets_over_tls() {
        let account = MailAccount {
            uid: String::new(),
            name: "teste".into(),
            host: "imap.gmail.com".into(),
            port: 993,
            user: "x@gmail.com".into(),
            password: "irrelevante".into(),
            remember: false,
            folder: "INBOX".into(),
        };
        let mut session = Session::connect(&account, Arc::default()).unwrap();
        // A wrong app password is refused with the friendly explanation.
        let error = login(&mut session, &account).unwrap_err().to_string();
        assert!(error.contains("senha de app"), "{error}");
    }
}
