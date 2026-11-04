//! Compartilhamento simples na intranet entre computadores Windows (Modo Anfitrião e Modo Cliente).
//! Nenhum banco SQLite é aberto em pasta de rede: cada estação mantém seu arquivo local protegido por DPAPI
//! e sincroniza/opera via HTTP JSON com o processo `ponteproj.exe` do computador anfitrião na intranet.

use crate::{
    dossier::LinkPayload,
    evidence::Source,
    files::{FileInput, FileSummary},
    model::{Analysis, Capture, ImportReceipt},
    pdf_reader::PageText,
    protection::{seal, unseal},
    storage::Store,
    tasks::Task,
    trash::{Kind as TrashKind, Target as TrashTarget},
    work_context::WorkContext,
};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{SecondsFormat, Utc};
use eframe::egui;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use url::Url;

pub const DEFAULT_PORT: u16 = 7878;
const MAX_HTTP_BODY: usize = 28 * 1024 * 1024;
const MAX_TEAM_EVENTS: i64 = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntranetMode {
    #[default]
    Local,
    Host,
    Client,
    /// The same person on more than one computer, exchanging a passphrase-
    /// encrypted transport file through synced folders (see `sharedfolder`).
    SharedFolder,
}

impl IntranetMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Só neste computador",
            Self::Host => "Este é o computador principal do gabinete",
            Self::Client => "Conectar ao computador principal do gabinete",
            Self::SharedFolder => "Pasta compartilhada: eu, em mais de um computador",
        }
    }

    /// Modes where every change is queued for a later exchange (with the
    /// host over HTTP, or with the transport file in the folders).
    pub fn queues_changes(self) -> bool {
        matches!(self, Self::Client | Self::SharedFolder)
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntranetConfig {
    pub mode: IntranetMode,
    pub member_name: String,
    pub bind_addr: String,
    pub host_url: String,
    pub cabinet_token: String,
    pub auto_start_host: bool,
    #[serde(default)]
    pub synced_revision: i64,
    #[serde(default)]
    pub last_sync_at: String,
    /// Transport files of the shared-folder mode, one per folder
    /// (`…\Google Drive\Ponte\ponte-sync.ponte`, a pen drive, a share…).
    #[serde(default)]
    pub folder_files: Vec<String>,
    /// Passphrase of the transport files; empty when not kept.
    #[serde(default)]
    pub folder_key: String,
    #[serde(default = "default_true")]
    pub remember_folder_key: bool,
    /// Stable identity of this computer in the folders (uuid).
    #[serde(default)]
    pub folder_writer: String,
    /// Identity of the library lineage adopted from (or given to) the folders.
    #[serde(default)]
    pub folder_lineage: String,
}

impl IntranetConfig {
    /// Copy to persist: the passphrase is dropped when the person chose not
    /// to keep it (it then lives only in the window, for the session).
    fn for_storage(&self) -> Self {
        let mut copy = self.clone();
        if !copy.remember_folder_key {
            copy.folder_key.clear();
        }
        copy
    }

    pub fn folder_paths(&self) -> Vec<PathBuf> {
        self.folder_files
            .iter()
            .map(|p| PathBuf::from(p.trim()))
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    }
}

impl Default for IntranetConfig {
    fn default() -> Self {
        let raw_user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "Integrante".into());
        let member: String = raw_user
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect();
        Self {
            mode: IntranetMode::Local,
            member_name: if member.trim().is_empty() {
                "Integrante".into()
            } else {
                member.trim().into()
            },
            bind_addr: format!("0.0.0.0:{DEFAULT_PORT}"),
            host_url: format!("http://127.0.0.1:{DEFAULT_PORT}"),
            cabinet_token: generate_token(),
            auto_start_host: false,
            synced_revision: 0,
            last_sync_at: String::new(),
            folder_files: Vec::new(),
            folder_key: String::new(),
            remember_folder_key: true,
            folder_writer: uuid::Uuid::new_v4().simple().to_string(),
            folder_lineage: String::new(),
        }
    }
}

pub fn generate_token() -> String {
    format!(
        "ponte-{}-{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

impl IntranetConfig {
    pub fn validate(&self) -> Result<()> {
        let member = self.member_name.trim();
        ensure!(
            !member.is_empty()
                && member.len() <= 120
                && !member.chars().any(char::is_control),
            "Informe seu nome ou identificação na equipe (até 120 bytes)."
        );
        match self.mode {
            IntranetMode::Local => Ok(()),
            IntranetMode::Host => {
                validate_bind_addr(&self.bind_addr)?;
                validate_token(&self.cabinet_token)?;
                Ok(())
            }
            IntranetMode::Client => {
                validate_host_url(&self.host_url)?;
                validate_token(&self.cabinet_token)?;
                Ok(())
            }
            IntranetMode::SharedFolder => {
                let paths = self.folder_paths();
                ensure!(
                    !paths.is_empty(),
                    "Escolha pelo menos uma pasta (o arquivo ponte-sync.ponte dentro dela)."
                );
                ensure!(paths.len() <= 8, "Use no máximo 8 pastas.");
                for path in &paths {
                    let text = path.to_string_lossy();
                    ensure!(
                        text.len() <= 1024 && !text.chars().any(char::is_control),
                        "Caminho de pasta inválido."
                    );
                    ensure!(
                        path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ponte")),
                        "O arquivo de sincronização precisa terminar em .ponte (ex.: ponte-sync.ponte)."
                    );
                }
                ensure!(
                    self.folder_key.len() <= 256 && !self.folder_key.chars().any(char::is_control),
                    "Senha de sincronização inválida."
                );
                ensure!(
                    !self.folder_writer.trim().is_empty() && self.folder_writer.len() <= 64,
                    "Identificação deste computador inválida."
                );
                Ok(())
            }
        }
    }
}

/// One line the person on the host copies and the person on the client
/// pastes, instead of transcribing an address and a key separately.
pub fn connection_code(host_url: &str, token: &str) -> String {
    let host: String = url::form_urlencoded::byte_serialize(host_url.trim().as_bytes()).collect();
    let key: String = url::form_urlencoded::byte_serialize(token.trim().as_bytes()).collect();
    format!("ponte://conectar?host={host}&chave={key}")
}

/// Accepts the code alone or pasted inside a longer message.
pub fn parse_connection_code(text: &str) -> Option<(String, String)> {
    let start = text.find("ponte://conectar?")?;
    let query = &text[start + "ponte://conectar?".len()..];
    let end = query
        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>')
        .unwrap_or(query.len());
    let mut host = None;
    let mut key = None;
    for (name, value) in url::form_urlencoded::parse(query[..end].as_bytes()) {
        match name.as_ref() {
            "host" => host = Some(value.into_owned()),
            "chave" => key = Some(value.into_owned()),
            _ => {}
        }
    }
    let host = host?;
    let key = key?;
    validate_host_url(&host).ok()?;
    validate_token(&key).ok()?;
    Some((host, key))
}

pub fn validate_token(token: &str) -> Result<()> {
    let trimmed = token.trim();
    ensure!(
        trimmed.len() >= 16
            && trimmed.len() <= 256
            && !trimmed.chars().any(|c| c.is_control() || c.is_whitespace()),
        "A chave do gabinete deve ter entre 16 e 256 caracteres, sem espaços."
    );
    Ok(())
}

pub fn is_intranet_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_loopback() || mapped.is_private() || mapped.is_link_local();
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

pub fn is_allowed_peer_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_loopback() || mapped.is_private() || mapped.is_link_local();
            }
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

pub fn validate_bind_addr(raw: &str) -> Result<SocketAddr> {
    let addr: SocketAddr = raw
        .trim()
        .parse()
        .context("Endereço de escuta inválido. Use o formato IP:PORTA (ex.: 0.0.0.0:7878).")?;
    ensure!(
        is_intranet_ip(addr.ip()),
        "O anfitrião só pode escutar em endereço local ou de intranet privada."
    );
    Ok(addr)
}

pub fn validate_host_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    ensure!(
        !trimmed.is_empty() && trimmed.len() <= 512,
        "Informe o endereço do computador anfitrião."
    );
    let url = Url::parse(trimmed)
        .context("Endereço do anfitrião inválido. Exemplo: http://192.168.1.10:7878")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "Use http:// ou https:// no endereço do anfitrião."
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && (url.path() == "/" || url.path().is_empty()),
        "Informe apenas o endereço base do anfitrião (ex.: http://192.168.1.10:7878), sem caminho ou credenciais na URL."
    );
    let host = url.host_str().context("Endereço do anfitrião sem host.")?;
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        ensure!(
            is_allowed_peer_ip(ip),
            "O endereço IP do anfitrião deve pertencer à rede interna (intranet) ou loopback."
        );
    } else {
        let lower = host.to_ascii_lowercase();
        let is_local_name = !lower.contains('.')
            || lower.ends_with(".local")
            || lower.ends_with(".lan")
            || lower.ends_with(".intranet")
            || lower.ends_with(".internal")
            || lower.ends_with(".corp")
            || lower.ends_with(".gov.br");
        ensure!(
            is_local_name,
            "Use o IP da máquina na intranet (ex.: 192.168.x.x) ou o nome do computador na rede interna."
        );
    }
    Ok(trimmed.to_owned())
}

fn verify_token_constant_time(expected: &str, provided: &str) -> bool {
    if validate_token(expected).is_err() || validate_token(provided).is_err() {
        return false;
    }
    let a = Sha256::digest(expected.trim().as_bytes());
    let b = Sha256::digest(provided.trim().as_bytes());
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn local_intranet_hints(port: u16) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("10.254.254.254:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                if !addr.ip().is_loopback() && !addr.ip().is_unspecified() {
                    out.push(format!("http://{}:{port}", addr.ip()));
                }
            }
        }
    }
    if let Ok(name) = std::env::var("COMPUTERNAME") {
        let clean = name.trim();
        if !clean.is_empty() {
            out.push(format!("http://{clean}:{port}"));
        }
    }
    if out.is_empty() {
        out.push(format!("http://127.0.0.1:{port}"));
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionMeta {
    pub version: i64,
    pub updated_by: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamEvent {
    pub id: i64,
    pub created_at: String,
    pub actor: String,
    pub action: String,
    pub document_id: Option<i64>,
    pub details: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocRow {
    pub id: i64,
    pub identity_hash: Vec<u8>,
    pub latest_capture_id: Option<i64>,
    pub last_seen_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureRow {
    pub id: i64,
    pub document_id: i64,
    pub content_hash: Vec<u8>,
    pub capture: Capture,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestRow {
    pub request_id: String,
    pub document_id: i64,
    pub capture_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoteRow {
    pub id: i64,
    pub document_id: i64,
    pub note: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftRow {
    pub id: i64,
    pub document_id: i64,
    pub capture_id: i64,
    pub materials: Vec<Source>,
    pub kind: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisRow {
    pub id: i64,
    pub document_id: i64,
    pub capture_id: i64,
    pub analysis: Analysis,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileMetaRow {
    pub id: i64,
    pub capture_id: i64,
    pub summary: FileSummary,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachmentRow {
    pub id: i64,
    pub owner_kind: String,
    pub owner_id: i64,
    pub document_id: i64,
    pub meta: crate::attachments::AttachmentMeta,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageTextRow {
    pub file_id: i64,
    pub page: u32,
    pub value: PageText,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkRow {
    pub id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub active: bool,
    pub payload: LinkPayload,
    pub kind_hash: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashRow {
    pub kind: String,
    pub item_id: i64,
    pub document_id: i64,
    pub deleted_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureRefRow {
    pub capture_id: i64,
    pub owner_kind: String,
    pub owner_id: i64,
    pub owner_document: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaiopAttendanceRow {
    pub attendance: crate::saiop::SaiopAttendance,
    pub raw_import: Option<crate::saiop::RawSpreadsheetRow>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CabinetSnapshot {
    pub revision: i64,
    pub documents: Vec<DocRow>,
    pub captures: Vec<CaptureRow>,
    pub requests: Vec<RequestRow>,
    pub notes: Vec<NoteRow>,
    pub drafts: Vec<DraftRow>,
    pub analyses: Vec<AnalysisRow>,
    pub tasks: Vec<Task>,
    pub work_contexts: Vec<(i64, WorkContext)>,
    pub files_meta: Vec<FileMetaRow>,
    pub page_texts: Vec<PageTextRow>,
    pub document_links: Vec<LinkRow>,
    pub trash: Vec<TrashRow>,
    pub capture_refs: Vec<CaptureRefRow>,
    pub item_versions: Vec<(String, i64, VersionMeta)>,
    pub team_events: Vec<TeamEvent>,
    #[serde(default)]
    pub saiop_batches: Vec<crate::saiop::SaiopImportBatch>,
    #[serde(default)]
    pub saiop_attendances: Vec<SaiopAttendanceRow>,
    #[serde(default)]
    pub attachments: Vec<AttachmentRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IntranetMutation {
    ImportCapture {
        capture: Capture,
    },
    ImportBundle {
        capture: Capture,
        files: Vec<FileInput>,
    },
    SaveNote {
        document_id: i64,
        note: String,
        expected_version: Option<i64>,
    },
    SaveDraft {
        document_id: i64,
        capture_id: i64,
        kind: String,
        body: String,
        materials: Vec<Source>,
    },
    SaveAnalysis {
        document_id: i64,
        analysis: Analysis,
    },
    SaveTask {
        task: Task,
        expected_version: Option<i64>,
    },
    SaveWorkContext {
        document_id: i64,
        value: WorkContext,
        expected_version: Option<i64>,
    },
    SavePageText {
        file_id: i64,
        page: u32,
        value: PageText,
    },
    AddDocumentLink {
        source: i64,
        target: i64,
        kind: String,
        reason: String,
    },
    SetLinkActive {
        id: i64,
        active: bool,
    },
    TrashItem {
        kind: String,
        id: i64,
    },
    RestoreItem {
        kind: String,
        id: i64,
    },
    DeleteDocument {
        id: i64,
    },
    SaveSaiopAttendance {
        attendance: crate::saiop::SaiopAttendance,
        expected_version: Option<i64>,
    },
    ImportSaiopBatch {
        file_name: String,
        file_sha256: String,
        sheet_name: String,
        items: Vec<(crate::saiop::SaiopAttendance, crate::saiop::RawSpreadsheetRow)>,
    },
    UndoSaiopBatch {
        batch_id: i64,
    },
    AddAttachment {
        owner_kind: String,
        owner_id: i64,
        name: String,
        description: String,
        data_base64: String,
    },
    UpdateAttachment {
        id: i64,
        description: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MutationReply {
    Receipt(ImportReceipt),
    Id(i64),
    SaiopBatch(crate::saiop::SaiopImportBatch),
    Counts(usize, usize),
    Unit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub id: i64,
    pub created_at: String,
    pub actor: String,
    pub label: String,
    pub mutation: IntranetMutation,
    /// The item as it was before the offline edit (same variant as
    /// `mutation`). It is the common ancestor for a field-by-field merge.
    #[serde(default)]
    pub base: Option<IntranetMutation>,
    /// Stable identity of the change, so a transport copy can say whether
    /// it already incorporates it (shared-folder mode).
    #[serde(default)]
    pub uid: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncConflict {
    pub id: i64,
    pub created_at: String,
    pub actor: String,
    pub item_label: String,
    pub local_summary: String,
    pub host_summary: String,
    pub reason: String,
    pub mutation: IntranetMutation,
    #[serde(default)]
    pub base: Option<IntranetMutation>,
}

/// One field that differs between the host and the offline edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDiff {
    pub label: String,
    pub host: String,
    pub local: String,
    /// What the field was before the offline edit, when known.
    pub base: Option<String>,
}

impl FieldDiff {
    /// Who changed this field relative to the common ancestor.
    pub fn changed_by(&self) -> &'static str {
        match &self.base {
            Some(base) if *base == self.local => "só no computador principal",
            Some(base) if *base == self.host => "só por você",
            Some(_) => "pelos dois lados",
            None => "desconhecido (sem versão anterior registrada)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    Merge,
    KeepLocal,
    KeepHost,
}

enum RpcError {
    Unreachable(String),
    Rejected(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RpcRequest {
    Ping,
    Sync {
        since_revision: Option<i64>,
    },
    FetchFileBytes {
        file_id: i64,
        capture_id: i64,
    },
    FetchAttachmentBytes {
        id: i64,
    },
    Mutate {
        actor: String,
        mutation: IntranetMutation,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RpcResponse {
    Pong {
        revision: i64,
        host_member: String,
        documents: usize,
        tasks: usize,
    },
    UpToDate {
        revision: i64,
    },
    Snapshot(Box<CabinetSnapshot>),
    FileBytes {
        data_base64: String,
    },
    Mutated {
        revision: i64,
        reply: MutationReply,
        snapshot: Box<CabinetSnapshot>,
    },
    Error {
        message: String,
    },
}

#[derive(Serialize, Deserialize)]
struct StoredDraftPayload {
    #[serde(default)]
    materials: Vec<Source>,
    kind: String,
    body: String,
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn record_mutation(
    tx: &Connection,
    actor: &str,
    item_kind: &str,
    item_id: i64,
    expected_version: Option<i64>,
    action: &str,
    document_id: Option<i64>,
    details: &str,
) -> Result<i64> {
    let current = read_version_tx(tx, item_kind, item_id)?;
    if let Some(expected) = expected_version {
        if current.version != expected {
            let who = if current.updated_by.is_empty() {
                "outro integrante"
            } else {
                &current.updated_by
            };
            bail!(
                "Conflito de edição simultânea: este registro já foi atualizado por {} (versão #{}). Atualize ou cancele sua edição para revisar os dados atuais antes de salvar.",
                who,
                current.version
            );
        }
    }
    let next_version = current.version + 1;
    let timestamp = now_iso();
    let actor_clean = if actor.trim().is_empty() {
        "Integrante"
    } else {
        actor.trim()
    };
    let meta = VersionMeta {
        version: next_version,
        updated_by: actor_clean.to_owned(),
        updated_at: timestamp.clone(),
    };
    tx.execute(
        "INSERT INTO item_versions(kind,item_id,version,payload) VALUES(?1,?2,?3,?4)
         ON CONFLICT(kind,item_id) DO UPDATE SET version=excluded.version,payload=excluded.payload",
        params![
            item_kind,
            item_id,
            next_version,
            seal(&serde_json::to_vec(&meta)?)?
        ],
    )?;
    let cab = read_version_tx(tx, "cabinet", 1)?;
    let next_cab = cab.version + 1;
    let cab_meta = VersionMeta {
        version: next_cab,
        updated_by: actor_clean.to_owned(),
        updated_at: timestamp.clone(),
    };
    tx.execute(
        "INSERT INTO item_versions(kind,item_id,version,payload) VALUES('cabinet',1,?1,?2)
         ON CONFLICT(kind,item_id) DO UPDATE SET version=excluded.version,payload=excluded.payload",
        params![next_cab, seal(&serde_json::to_vec(&cab_meta)?)?],
    )?;
    let event = TeamEvent {
        id: 0,
        created_at: timestamp.clone(),
        actor: actor_clean.to_owned(),
        action: action.to_owned(),
        document_id,
        details: details.to_owned(),
    };
    tx.execute(
        "INSERT INTO team_events(created_at,payload) VALUES(?1,?2)",
        params![timestamp, seal(&serde_json::to_vec(&event)?)?],
    )?;
    tx.execute(
        "DELETE FROM team_events WHERE id NOT IN (SELECT id FROM team_events ORDER BY id DESC LIMIT ?1)",
        [MAX_TEAM_EVENTS],
    )?;
    Ok(next_version)
}

fn read_version_tx(tx: &Connection, kind: &str, item_id: i64) -> Result<VersionMeta> {
    let row: Option<(i64, Vec<u8>)> = tx
        .query_row(
            "SELECT version,payload FROM item_versions WHERE kind=?1 AND item_id=?2",
            params![kind, item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        Some((version, blob)) => {
            let mut meta: VersionMeta = serde_json::from_slice(&unseal(&blob)?)?;
            meta.version = version;
            Ok(meta)
        }
        None => Ok(VersionMeta::default()),
    }
}

impl Store {
    pub(crate) fn ensure_intranet(&mut self) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS intranet_config(
                id INTEGER PRIMARY KEY CHECK(id=1),
                payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS item_versions(
                kind TEXT NOT NULL,
                item_id INTEGER NOT NULL,
                version INTEGER NOT NULL,
                payload BLOB NOT NULL,
                PRIMARY KEY(kind,item_id)
            );
            CREATE TABLE IF NOT EXISTS team_events(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS offline_outbox(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS offline_conflicts(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                payload BLOB NOT NULL
            );",
        )?;
        if tx.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? < 9 {
            tx.pragma_update(None, "user_version", 9)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_intranet_config(&self) -> Result<IntranetConfig> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT payload FROM intranet_config WHERE id=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match blob {
            Some(bytes) => Ok(serde_json::from_slice(&unseal(&bytes)?)?),
            None => Ok(IntranetConfig::default()),
        }
    }

    pub fn save_intranet_config(&mut self, config: &IntranetConfig) -> Result<()> {
        config.validate()?;
        let payload = seal(&serde_json::to_vec(&config.for_storage())?)?;
        self.conn.execute(
            "INSERT INTO intranet_config(id,payload) VALUES(1,?1)
             ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            [payload],
        )?;
        Ok(())
    }

    pub(crate) fn active_actor(&self) -> String {
        if !self.actor_override.is_empty() {
            return self.actor_override.clone();
        }
        self.load_intranet_config()
            .map(|c| c.member_name)
            .unwrap_or_else(|_| "Integrante".into())
    }

    pub(crate) fn client_config(&self) -> Result<Option<IntranetConfig>> {
        if self.suppress_remote {
            return Ok(None);
        }
        let cfg = self.load_intranet_config()?;
        if cfg.mode.queues_changes() {
            Ok(Some(cfg))
        } else {
            Ok(None)
        }
    }

    pub fn cabinet_revision(&self) -> Result<i64> {
        Ok(read_version_tx(&self.conn, "cabinet", 1)?.version)
    }

    pub fn item_version(&self, kind: &str, item_id: i64) -> Result<VersionMeta> {
        read_version_tx(&self.conn, kind, item_id)
    }

    pub fn list_team_events(&self, limit: usize) -> Result<Vec<TeamEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,created_at,payload FROM team_events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit.min(500) as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, created_at, blob) = row?;
            let mut event: TeamEvent = serde_json::from_slice(&unseal(&blob)?)?;
            event.id = id;
            event.created_at = created_at;
            out.push(event);
        }
        Ok(out)
    }

    pub fn export_snapshot(&self) -> Result<CabinetSnapshot> {
        let revision = self.cabinet_revision()?;
        let documents = self
            .conn
            .prepare("SELECT id,identity_hash,latest_capture_id,last_seen_at FROM documents ORDER BY id")?
            .query_map([], |r| {
                Ok(DocRow {
                    id: r.get(0)?,
                    identity_hash: r.get(1)?,
                    latest_capture_id: r.get(2)?,
                    last_seen_at: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let raw_captures = self
            .conn
            .prepare("SELECT id,document_id,content_hash,payload,created_at FROM captures ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut captures = Vec::with_capacity(raw_captures.len());
        for (id, document_id, content_hash, blob, created_at) in raw_captures {
            captures.push(CaptureRow {
                id,
                document_id,
                content_hash,
                capture: serde_json::from_slice(&unseal(&blob)?)?,
                created_at,
            });
        }

        let requests = self
            .conn
            .prepare("SELECT request_id,document_id,capture_id FROM requests ORDER BY request_id")?
            .query_map([], |r| {
                Ok(RequestRow {
                    request_id: r.get(0)?,
                    document_id: r.get(1)?,
                    capture_id: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let raw_notes = self
            .conn
            .prepare("SELECT id,document_id,payload,created_at FROM notes ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut notes = Vec::with_capacity(raw_notes.len());
        for (id, document_id, blob, created_at) in raw_notes {
            notes.push(NoteRow {
                id,
                document_id,
                note: serde_json::from_slice(&unseal(&blob)?)?,
                created_at,
            });
        }

        let raw_drafts = self
            .conn
            .prepare("SELECT id,document_id,capture_id,payload,created_at FROM drafts ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut drafts = Vec::with_capacity(raw_drafts.len());
        for (id, document_id, capture_id, blob, created_at) in raw_drafts {
            let payload: StoredDraftPayload = serde_json::from_slice(&unseal(&blob)?)?;
            drafts.push(DraftRow {
                id,
                document_id,
                capture_id,
                materials: payload.materials,
                kind: payload.kind,
                body: payload.body,
                created_at,
            });
        }

        let raw_analyses = self
            .conn
            .prepare("SELECT id,document_id,capture_id,payload,created_at FROM analyses ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut analyses = Vec::with_capacity(raw_analyses.len());
        for (id, document_id, capture_id, blob, created_at) in raw_analyses {
            analyses.push(AnalysisRow {
                id,
                document_id,
                capture_id,
                analysis: serde_json::from_slice(&unseal(&blob)?)?,
                created_at,
            });
        }

        let raw_tasks = self
            .conn
            .prepare("SELECT id,document_id,payload FROM tasks ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tasks = Vec::with_capacity(raw_tasks.len());
        for (id, document_id, blob) in raw_tasks {
            let mut task: Task = serde_json::from_slice(&unseal(&blob)?)?;
            task.id = id;
            task.document_id = document_id;
            tasks.push(task);
        }

        let raw_contexts = self
            .conn
            .prepare("SELECT document_id,payload FROM work_context ORDER BY document_id")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut work_contexts = Vec::with_capacity(raw_contexts.len());
        for (doc_id, blob) in raw_contexts {
            work_contexts.push((doc_id, serde_json::from_slice(&unseal(&blob)?)?));
        }

        let raw_files = self
            .conn
            .prepare("SELECT id,capture_id,metadata FROM files ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut files_meta = Vec::with_capacity(raw_files.len());
        for (id, capture_id, blob) in raw_files {
            let mut summary: FileSummary = serde_json::from_slice(&unseal(&blob)?)?;
            summary.id = id;
            files_meta.push(FileMetaRow {
                id,
                capture_id,
                summary,
            });
        }

        let raw_pages = self
            .conn
            .prepare("SELECT file_id,page,payload FROM page_text ORDER BY file_id,page")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut page_texts = Vec::with_capacity(raw_pages.len());
        for (file_id, page, blob) in raw_pages {
            page_texts.push(PageTextRow {
                file_id,
                page,
                value: serde_json::from_slice(&unseal(&blob)?)?,
            });
        }

        let raw_links = self
            .conn
            .prepare("SELECT id,source_id,target_id,active,payload,kind_hash FROM document_links ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                    r.get::<_, Vec<u8>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut document_links = Vec::with_capacity(raw_links.len());
        for (id, source_id, target_id, active, blob, kind_hash) in raw_links {
            document_links.push(LinkRow {
                id,
                source_id,
                target_id,
                active,
                payload: serde_json::from_slice(&unseal(&blob)?)?,
                kind_hash,
            });
        }

        let trash = self
            .conn
            .prepare("SELECT kind,item_id,document_id,deleted_at FROM trash ORDER BY kind,item_id")?
            .query_map([], |r| {
                Ok(TrashRow {
                    kind: r.get(0)?,
                    item_id: r.get(1)?,
                    document_id: r.get(2)?,
                    deleted_at: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let capture_refs = self
            .conn
            .prepare("SELECT capture_id,owner_kind,owner_id,owner_document FROM capture_refs ORDER BY capture_id,owner_kind,owner_id")?
            .query_map([], |r| {
                Ok(CaptureRefRow {
                    capture_id: r.get(0)?,
                    owner_kind: r.get(1)?,
                    owner_id: r.get(2)?,
                    owner_document: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let raw_versions = self
            .conn
            .prepare("SELECT kind,item_id,version,payload FROM item_versions ORDER BY kind,item_id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut item_versions = Vec::with_capacity(raw_versions.len());
        for (kind, item_id, version, blob) in raw_versions {
            let mut meta: VersionMeta = serde_json::from_slice(&unseal(&blob)?)?;
            meta.version = version;
            item_versions.push((kind, item_id, meta));
        }

        let raw_attachments = self
            .conn
            .prepare("SELECT id,owner_kind,owner_id,document_id,metadata FROM attachments ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut attachments = Vec::with_capacity(raw_attachments.len());
        for (id, owner_kind, owner_id, document_id, blob) in raw_attachments {
            let mut meta: crate::attachments::AttachmentMeta =
                serde_json::from_slice(&unseal(&blob)?)?;
            meta.id = id;
            attachments.push(AttachmentRow {
                id,
                owner_kind,
                owner_id,
                document_id,
                meta,
            });
        }

        let team_events = self.list_team_events(MAX_TEAM_EVENTS as usize)?;
        let saiop_batches = self.list_saiop_batches()?;
        let mut saiop_attendances = Vec::new();
        for att in self.list_saiop_attendances()? {
            let raw_import = self.load_saiop_raw_row(att.id)?;
            let updated_at = att.updated_at.clone();
            saiop_attendances.push(SaiopAttendanceRow {
                attendance: att,
                raw_import,
                updated_at,
            });
        }

        Ok(CabinetSnapshot {
            revision,
            documents,
            captures,
            requests,
            notes,
            drafts,
            analyses,
            tasks,
            work_contexts,
            files_meta,
            page_texts,
            document_links,
            trash,
            capture_refs,
            item_versions,
            team_events,
            saiop_batches,
            saiop_attendances,
            attachments,
        })
    }

    pub fn apply_snapshot<F, G>(
        &mut self,
        snapshot: &CabinetSnapshot,
        fetch_pdf: F,
        fetch_attachment: G,
    ) -> Result<()>
    where
        F: Fn(i64, i64) -> Result<Vec<u8>>,
        G: Fn(i64) -> Result<Vec<u8>>,
    {
        // Attachments already here with the same content are kept; the rest
        // are transferred from the main computer and checked by hash.
        let existing_attachments = self
            .conn
            .prepare("SELECT id,metadata,payload FROM attachments")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut attachment_blobs = BTreeMap::<i64, (String, Vec<u8>)>::new();
        for (id, meta_blob, payload_blob) in existing_attachments {
            if let Ok(plain) = unseal(&meta_blob) {
                if let Ok(meta) = serde_json::from_slice::<crate::attachments::AttachmentMeta>(&plain) {
                    attachment_blobs.insert(id, (meta.sha256, payload_blob));
                }
            }
        }
        let mut prepared_attachment_blobs = BTreeMap::<i64, Vec<u8>>::new();
        for row in &snapshot.attachments {
            if let Some((sha, blob)) = attachment_blobs.get(&row.id) {
                if *sha == row.meta.sha256 {
                    prepared_attachment_blobs.insert(row.id, blob.clone());
                    continue;
                }
            }
            let bytes = fetch_attachment(row.id)?;
            ensure!(
                !bytes.is_empty() && bytes.len() <= crate::attachments::MAX_ATTACHMENT_BYTES,
                "Anexo recebido do computador principal é inválido ou excede o limite."
            );
            ensure!(
                format!("{:x}", Sha256::digest(&bytes)) == row.meta.sha256,
                "Integridade SHA-256 do anexo recebido não confere com o registro do computador principal."
            );
            prepared_attachment_blobs.insert(row.id, seal(&bytes)?);
        }
        let existing_files = self
            .conn
            .prepare("SELECT id,metadata,payload FROM files")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut file_blobs = BTreeMap::<i64, (String, Vec<u8>)>::new();
        for (id, meta_blob, payload_blob) in existing_files {
            if let Ok(plain) = unseal(&meta_blob) {
                if let Ok(summary) = serde_json::from_slice::<FileSummary>(&plain) {
                    file_blobs.insert(id, (summary.sha256, payload_blob));
                }
            }
        }
        let mut prepared_file_blobs = BTreeMap::<i64, Vec<u8>>::new();
        for meta in &snapshot.files_meta {
            if let Some((sha, blob)) = file_blobs.get(&meta.id) {
                if *sha == meta.summary.sha256 {
                    prepared_file_blobs.insert(meta.id, blob.clone());
                    continue;
                }
            }
            let bytes = fetch_pdf(meta.id, meta.capture_id)?;
            ensure!(
                bytes.starts_with(b"%PDF-") && bytes.len() <= crate::files::MAX_FILE_BYTES,
                "PDF recebido do anfitrião é inválido ou excede o limite."
            );
            let actual_sha = format!("{:x}", Sha256::digest(&bytes));
            ensure!(
                actual_sha == meta.summary.sha256,
                "Integridade SHA-256 do PDF recebido não confere com o registro do anfitrião."
            );
            prepared_file_blobs.insert(meta.id, seal(&bytes)?);
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Clear `trash` first so active-record triggers never block re-inserting archived items.
        tx.execute_batch(
            "DELETE FROM trash;
             DELETE FROM attachments;
             DELETE FROM saiop_attendances;
             DELETE FROM saiop_batches;
             DELETE FROM capture_refs;
             DELETE FROM document_links;
             DELETE FROM page_text;
             DELETE FROM files;
             DELETE FROM work_context;
             DELETE FROM tasks;
             DELETE FROM analyses;
             DELETE FROM drafts;
             DELETE FROM notes;
             DELETE FROM requests;
             DELETE FROM captures;
             DELETE FROM documents;
             DELETE FROM item_versions;
             DELETE FROM team_events;",
        )?;

        for doc in &snapshot.documents {
            tx.execute(
                "INSERT INTO documents(id,identity_hash,latest_capture_id,last_seen_at) VALUES(?1,?2,NULL,?3)",
                params![doc.id, doc.identity_hash, doc.last_seen_at],
            )?;
        }
        for cap in &snapshot.captures {
            tx.execute(
                "INSERT INTO captures(id,document_id,content_hash,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
                params![
                    cap.id,
                    cap.document_id,
                    cap.content_hash,
                    seal(&serde_json::to_vec(&cap.capture)?)?,
                    cap.created_at
                ],
            )?;
        }
        for doc in &snapshot.documents {
            tx.execute(
                "UPDATE documents SET latest_capture_id=?1 WHERE id=?2",
                params![doc.latest_capture_id, doc.id],
            )?;
        }
        for req in &snapshot.requests {
            tx.execute(
                "INSERT INTO requests(request_id,document_id,capture_id) VALUES(?1,?2,?3)",
                params![req.request_id, req.document_id, req.capture_id],
            )?;
        }
        for note in &snapshot.notes {
            tx.execute(
                "INSERT INTO notes(id,document_id,payload,created_at) VALUES(?1,?2,?3,?4)",
                params![
                    note.id,
                    note.document_id,
                    seal(&serde_json::to_vec(&note.note)?)?,
                    note.created_at
                ],
            )?;
        }
        for draft in &snapshot.drafts {
            let payload = StoredDraftPayload {
                materials: draft.materials.clone(),
                kind: draft.kind.clone(),
                body: draft.body.clone(),
            };
            tx.execute(
                "INSERT INTO drafts(id,document_id,capture_id,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
                params![
                    draft.id,
                    draft.document_id,
                    draft.capture_id,
                    seal(&serde_json::to_vec(&payload)?)?,
                    draft.created_at
                ],
            )?;
        }
        for analysis in &snapshot.analyses {
            tx.execute(
                "INSERT INTO analyses(id,document_id,capture_id,payload,created_at) VALUES(?1,?2,?3,?4,?5)",
                params![
                    analysis.id,
                    analysis.document_id,
                    analysis.capture_id,
                    seal(&serde_json::to_vec(&analysis.analysis)?)?,
                    analysis.created_at
                ],
            )?;
        }
        for task in &snapshot.tasks {
            tx.execute(
                "INSERT INTO tasks(id,document_id,payload) VALUES(?1,?2,?3)",
                params![
                    task.id,
                    task.document_id,
                    seal(&serde_json::to_vec(task)?)?
                ],
            )?;
        }
        for (doc_id, ctx) in &snapshot.work_contexts {
            tx.execute(
                "INSERT INTO work_context(document_id,payload) VALUES(?1,?2)",
                params![doc_id, seal(&serde_json::to_vec(ctx)?)?],
            )?;
        }
        for meta in &snapshot.files_meta {
            let payload_blob = prepared_file_blobs
                .get(&meta.id)
                .context(" Conteúdo do PDF ausente ao aplicar sincronização.")?;
            tx.execute(
                "INSERT INTO files(id,capture_id,metadata,payload) VALUES(?1,?2,?3,?4)",
                params![
                    meta.id,
                    meta.capture_id,
                    seal(&serde_json::to_vec(&meta.summary)?)?,
                    payload_blob
                ],
            )?;
        }
        for page in &snapshot.page_texts {
            tx.execute(
                "INSERT INTO page_text(file_id,page,payload) VALUES(?1,?2,?3)",
                params![
                    page.file_id,
                    page.page,
                    seal(&serde_json::to_vec(&page.value)?)?
                ],
            )?;
        }
        for link in &snapshot.document_links {
            tx.execute(
                "INSERT INTO document_links(id,source_id,target_id,active,payload,kind_hash) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    link.id,
                    link.source_id,
                    link.target_id,
                    link.active,
                    seal(&serde_json::to_vec(&link.payload)?)?,
                    link.kind_hash
                ],
            )?;
        }
        for r in &snapshot.capture_refs {
            tx.execute(
                "INSERT INTO capture_refs(capture_id,owner_kind,owner_id,owner_document) VALUES(?1,?2,?3,?4)",
                params![r.capture_id, r.owner_kind, r.owner_id, r.owner_document],
            )?;
        }
        for (kind, item_id, meta) in &snapshot.item_versions {
            tx.execute(
                "INSERT INTO item_versions(kind,item_id,version,payload) VALUES(?1,?2,?3,?4)",
                params![
                    kind,
                    item_id,
                    meta.version,
                    seal(&serde_json::to_vec(meta)?)?
                ],
            )?;
        }
        for event in snapshot.team_events.iter().rev() {
            tx.execute(
                "INSERT INTO team_events(id,created_at,payload) VALUES(?1,?2,?3)",
                params![
                    event.id,
                    event.created_at,
                    seal(&serde_json::to_vec(event)?)?
                ],
            )?;
        }
        for batch in &snapshot.saiop_batches {
            tx.execute(
                "INSERT INTO saiop_batches(id,file_sha256,payload,imported_at) VALUES(?1,?2,?3,?4)",
                params![
                    batch.id,
                    batch.file_sha256,
                    seal(&serde_json::to_vec(batch)?)?,
                    batch.imported_at
                ],
            )?;
        }
        for row in &snapshot.saiop_attendances {
            let raw_blob = match &row.raw_import {
                Some(raw) => Some(seal(&serde_json::to_vec(raw)?)?),
                None => None,
            };
            tx.execute(
                "INSERT INTO saiop_attendances(id,batch_id,source_row,document_id,reference_norm,payload,raw_import,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    row.attendance.id,
                    row.attendance.batch_id,
                    row.attendance.source_row.map(|r| r as i64),
                    row.attendance.document_id,
                    row.attendance.reference_norm,
                    seal(&serde_json::to_vec(&row.attendance)?)?,
                    raw_blob,
                    row.updated_at
                ],
            )?;
        }
        for row in &snapshot.attachments {
            let payload_blob = prepared_attachment_blobs
                .get(&row.id)
                .context("Conteúdo de anexo ausente ao aplicar sincronização.")?;
            tx.execute(
                "INSERT INTO attachments(id,owner_kind,owner_id,document_id,metadata,payload) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    row.id,
                    row.owner_kind,
                    row.owner_id,
                    row.document_id,
                    seal(&serde_json::to_vec(&row.meta)?)?,
                    payload_blob
                ],
            )?;
        }
        // Populate `trash` last so active-item triggers remain satisfied throughout the batch.
        for t in &snapshot.trash {
            tx.execute(
                "INSERT INTO trash(kind,item_id,document_id,deleted_at) VALUES(?1,?2,?3,?4)",
                params![t.kind, t.item_id, t.document_id, t.deleted_at],
            )?;
        }

        if let Some(blob) = tx
            .query_row(
                "SELECT payload FROM intranet_config WHERE id=1",
                [],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()?
        {
            if let Ok(plain) = unseal(&blob) {
                if let Ok(mut cfg) = serde_json::from_slice::<IntranetConfig>(&plain) {
                    cfg.synced_revision = snapshot.revision;
                    cfg.last_sync_at = now_iso();
                    tx.execute(
                        "UPDATE intranet_config SET payload=?1 WHERE id=1",
                        [seal(&serde_json::to_vec(&cfg)?)?],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn execute_mutation(
        &mut self,
        actor: &str,
        mutation: IntranetMutation,
    ) -> Result<MutationReply> {
        let prev_suppress = self.suppress_remote;
        let prev_actor = std::mem::replace(&mut self.actor_override, actor.trim().to_owned());
        self.suppress_remote = true;
        let result = match mutation {
            IntranetMutation::ImportCapture { capture } => {
                self.import_capture(&capture).map(MutationReply::Receipt)
            }
            IntranetMutation::ImportBundle { capture, files } => self
                .import_bundle(&capture, &files)
                .map(MutationReply::Receipt),
            IntranetMutation::SaveNote {
                document_id,
                note,
                expected_version,
            } => self
                .save_note_checked(document_id, &note, expected_version)
                .map(|()| MutationReply::Unit),
            IntranetMutation::SaveDraft {
                document_id,
                capture_id,
                kind,
                body,
                materials,
            } => self
                .save_draft_with_sources(document_id, capture_id, &kind, &body, &materials)
                .map(MutationReply::Id),
            IntranetMutation::SaveAnalysis {
                document_id,
                analysis,
            } => self
                .save_analysis(document_id, &analysis)
                .map(|()| MutationReply::Unit),
            IntranetMutation::SaveTask {
                task,
                expected_version,
            } => self
                .save_task_checked(&task, expected_version)
                .map(MutationReply::Id),
            IntranetMutation::SaveWorkContext {
                document_id,
                value,
                expected_version,
            } => self
                .save_work_context_checked(document_id, &value, expected_version)
                .map(|()| MutationReply::Unit),
            IntranetMutation::SavePageText {
                file_id,
                page,
                value,
            } => self
                .save_page_text(file_id, page, &value)
                .map(|()| MutationReply::Unit),
            IntranetMutation::AddDocumentLink {
                source,
                target,
                kind,
                reason,
            } => self
                .add_document_link(source, target, &kind, &reason)
                .map(|()| MutationReply::Unit),
            IntranetMutation::SetLinkActive { id, active } => self
                .set_link_active(id, active)
                .map(|()| MutationReply::Unit),
            IntranetMutation::TrashItem { kind, id } => {
                let parsed = TrashKind::parse(&kind)?;
                self.trash_item(TrashTarget { kind: parsed, id })
                    .map(MutationReply::Id)
            }
            IntranetMutation::RestoreItem { kind, id } => {
                let parsed = TrashKind::parse(&kind)?;
                self.restore_item(TrashTarget { kind: parsed, id })
                    .map(MutationReply::Id)
            }
            IntranetMutation::AddAttachment {
                owner_kind,
                owner_id,
                name,
                description,
                data_base64,
            } => {
                let owner = crate::attachments::Owner::parse(&owner_kind, owner_id)?;
                ensure!(
                    data_base64.len() <= crate::attachments::MAX_ATTACHMENT_BYTES.div_ceil(3) * 4,
                    "{}",
                    crate::attachments::size_message(&name, data_base64.len() / 4 * 3)
                );
                let data = STANDARD
                    .decode(data_base64.trim())
                    .context("Anexo recebido com codificação inválida.")?;
                self.add_attachment(owner, &name, &description, &data)
                    .map(MutationReply::Id)
            }
            IntranetMutation::UpdateAttachment { id, description } => self
                .update_attachment_description(id, &description)
                .map(|()| MutationReply::Unit),
            IntranetMutation::DeleteDocument { id } => {
                self.delete_document(id).map(|()| MutationReply::Unit)
            }
            IntranetMutation::SaveSaiopAttendance {
                attendance,
                expected_version,
            } => self
                .save_saiop_attendance(&attendance, expected_version)
                .map(MutationReply::Id),
            IntranetMutation::ImportSaiopBatch {
                file_name,
                file_sha256,
                sheet_name,
                items,
            } => self
                .commit_saiop_import_raw(&file_name, &file_sha256, &sheet_name, &items)
                .map(MutationReply::SaiopBatch),
            IntranetMutation::UndoSaiopBatch { batch_id } => self
                .undo_saiop_batch(batch_id)
                .map(|(r, k)| MutationReply::Counts(r, k)),
        };
        self.suppress_remote = prev_suppress;
        self.actor_override = prev_actor;
        result
    }

    pub fn sync_from_host(&mut self, force_full: bool) -> Result<bool> {
        let cfg = self.load_intranet_config()?;
        ensure!(
            cfg.mode == IntranetMode::Client,
            "A sincronização remota requer o modo Cliente na Intranet."
        );
        cfg.validate()?;

        // 1. Se houver alterações feitas offline nesta estação cliente, sincroniza mutuamente com o anfitrião:
        let outbox = self.list_offline_outbox()?;
        let mut pushed_offline = false;
        if !outbox.is_empty() {
            let host_snapshot = match send_rpc_classified(&cfg, &RpcRequest::Sync { since_revision: None }) {
                Ok(RpcResponse::Snapshot(snap)) => Some(snap),
                Ok(RpcResponse::Error { message }) => bail!("{message}"),
                Ok(_) => None,
                Err(RpcError::Unreachable(msg)) => {
                    self.note_host_reachability(false);
                    bail!("{msg}");
                }
                Err(RpcError::Rejected(msg)) => bail!("{msg}"),
            };
            self.note_host_reachability(true);

            for entry in outbox {
                match send_rpc_classified(
                    &cfg,
                    &RpcRequest::Mutate {
                        actor: entry.actor.clone(),
                        mutation: entry.mutation.clone(),
                    },
                ) {
                    Ok(RpcResponse::Mutated { .. }) => {
                        self.conn.execute(
                            "DELETE FROM offline_outbox WHERE id=?1",
                            params![entry.id],
                        )?;
                        pushed_offline = true;
                    }
                    Ok(RpcResponse::Error { message }) | Err(RpcError::Rejected(message)) => {
                        let conflict = build_sync_conflict(
                            &entry,
                            &message,
                            host_snapshot.as_deref(),
                        );
                        let blob = seal(&serde_json::to_vec(&conflict)?)?;
                        self.conn.execute(
                            "INSERT INTO offline_conflicts(created_at,payload) VALUES(?1,?2)",
                            params![conflict.created_at, blob],
                        )?;
                        self.conn.execute(
                            "DELETE FROM offline_outbox WHERE id=?1",
                            params![entry.id],
                        )?;
                        pushed_offline = true;
                    }
                    Ok(_) => {
                        bail!("Resposta inesperada do anfitrião ao sincronizar fila offline.");
                    }
                    Err(RpcError::Unreachable(msg)) => {
                        bail!("{msg}");
                    }
                }
            }
        }

        let since = if force_full || pushed_offline {
            None
        } else {
            Some(cfg.synced_revision)
        };
        let response = match send_rpc_classified(&cfg, &RpcRequest::Sync { since_revision: since }) {
            Ok(response) => {
                self.note_host_reachability(true);
                response
            }
            Err(RpcError::Unreachable(msg)) => {
                self.note_host_reachability(false);
                bail!("{msg}");
            }
            Err(RpcError::Rejected(msg)) => bail!("{msg}"),
        };
        match response {
            RpcResponse::UpToDate { .. } => Ok(pushed_offline),
            RpcResponse::Snapshot(snapshot) => {
                let cfg_clone = cfg.clone();
                self.apply_snapshot(
                    &snapshot,
                    |file_id, capture_id| fetch_remote_pdf(&cfg_clone, file_id, capture_id),
                    |id| fetch_remote_attachment(&cfg_clone, id),
                )?;
                Ok(true)
            }
            RpcResponse::Error { message } => bail!("{message}"),
            _ => bail!("Resposta inesperada do computador anfitrião durante a sincronização."),
        }
    }

    /// Exchange with the transport files of the shared-folder mode. Reads
    /// every reachable folder, reconciles on a temporary library (newest
    /// copy as base, then changes from other copies that it lacks, then this
    /// computer's queue), writes the result to every reachable folder and
    /// only then updates the local library. Conflicts go to the same screen
    /// as in the connected mode. `allow_replace` confirms, on a computer
    /// joining with its own documents, that they will be replaced.
    pub fn sync_with_folders(
        &mut self,
        cfg: &IntranetConfig,
        key: &str,
        allow_replace: bool,
        reporter: &crate::ai::Reporter,
    ) -> Result<FolderSyncReport> {
        use crate::sharedfolder::{self, JournalEntry};
        ensure!(
            cfg.mode == IntranetMode::SharedFolder,
            "A pasta compartilhada não está ativa neste computador."
        );
        cfg.validate()?;
        ensure!(!key.is_empty(), "Informe a senha de sincronização em Rede e equipe.");
        let paths = cfg.folder_paths();
        let mut report = FolderSyncReport::default();
        reporter.step("Lendo as pastas…");
        let mut copies = Vec::new();
        let mut absent = Vec::new();
        for path in &paths {
            let folder = path.parent().context("Caminho de pasta inválido.")?;
            if !folder.is_dir() {
                report.unreachable.push(path.display().to_string());
                continue;
            }
            match sharedfolder::read(path, key) {
                Ok(Some(copy)) => copies.push(copy),
                Ok(None) => absent.push(path.clone()),
                Err(error) => bail!("{}: {error}", path.display()),
            }
        }
        ensure!(
            !(copies.is_empty() && absent.is_empty()),
            "Nenhuma pasta está acessível agora ({}). Conecte o pendrive ou espere a nuvem ficar disponível.",
            report.unreachable.join("; ")
        );
        let lineage = if !cfg.folder_lineage.trim().is_empty() {
            cfg.folder_lineage.clone()
        } else {
            copies
                .first()
                .map(|c| c.header.lineage.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string())
        };
        for copy in &copies {
            ensure!(
                copy.header.lineage == lineage,
                "A pasta {} contém outro acervo, não o deste computador. Remova-a da lista ou aponte para outro arquivo.",
                copy.path.display()
            );
            ensure!(
                copy.header.schema_version <= crate::storage::SCHEMA_VERSION,
                "A cópia em {} foi gravada por um Ponte mais novo; atualize o Ponte neste computador.",
                copy.path.display()
            );
        }
        copies.sort_by(|a, b| {
            (b.header.revision, &b.header.written_at).cmp(&(a.header.revision, &a.header.written_at))
        });
        let outbox = self.list_offline_outbox()?;
        let first_write = copies.is_empty();
        if let Some(best) = copies.first() {
            ensure!(
                best.header.revision >= cfg.synced_revision,
                "A pasta ainda não recebeu a versão mais recente (revisão {} na pasta, {} neste computador). Espere a nuvem terminar de sincronizar e tente de novo.",
                best.header.revision,
                cfg.synced_revision
            );
            if cfg.synced_revision == 0 && !allow_replace {
                let local_documents = self.list_documents("")?.len();
                ensure!(
                    local_documents == 0,
                    "[confirmar-substituicao] Este computador tem {local_documents} documento(s) que não estão na pasta. Ao sincronizar pela primeira vez, o acervo daqui será substituído pelo da pasta ({}). Faça um backup antes, se quiser guardá-los.",
                    sharedfolder::describe(&best.header)
                );
            }
        }
        // Changes other copies carry that the newest one never received.
        let best_applied: std::collections::BTreeSet<String> = copies
            .first()
            .map(|c| c.payload.applied.iter().cloned().collect())
            .unwrap_or_default();
        let mut foreign: Vec<(JournalEntry, String)> = Vec::new();
        for copy in copies.iter().skip(1) {
            let missing_from_journal = copy
                .payload
                .applied
                .iter()
                .filter(|uid| !best_applied.contains(*uid) && !copy.payload.journal.iter().any(|e| &e.uid == *uid))
                .count();
            if missing_from_journal > 0 {
                report.notes.push(format!(
                    "{}: {missing_from_journal} alteração(ões) antigas não puderam ser recuperadas (diário já compactado).",
                    copy.path.display()
                ));
            }
            for entry in &copy.payload.journal {
                if !best_applied.contains(&entry.uid) && !foreign.iter().any(|(e, _)| e.uid == entry.uid) {
                    foreign.push((entry.clone(), copy.path.display().to_string()));
                }
            }
        }
        foreign.sort_by(|a, b| (a.0.revision, &a.0.created_at).cmp(&(b.0.revision, &b.0.created_at)));
        let lagging = copies
            .iter()
            .skip(1)
            .any(|c| copies[0].header.revision > c.header.revision)
            || !absent.is_empty();
        if !first_write
            && outbox.is_empty()
            && foreign.is_empty()
            && copies[0].header.revision == cfg.synced_revision
            && !lagging
        {
            report.revision = cfg.synced_revision;
            return Ok(report);
        }

        let temp_dir = crate::paths::data_dir()?.join("temp");
        std::fs::create_dir_all(&temp_dir)?;
        let temp_path = temp_dir.join(format!("sync-{}.sqlite3", uuid::Uuid::new_v4().simple()));
        let result = self.reconcile_with_folders(
            cfg,
            key,
            &lineage,
            &copies,
            &absent,
            foreign,
            outbox,
            first_write,
            &temp_path,
            reporter,
            report,
        );
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", temp_path.display()));
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn reconcile_with_folders(
        &mut self,
        cfg: &IntranetConfig,
        key: &str,
        lineage: &str,
        copies: &[crate::sharedfolder::Copy],
        absent: &[PathBuf],
        foreign: Vec<(crate::sharedfolder::JournalEntry, String)>,
        outbox: Vec<OutboxEntry>,
        first_write: bool,
        temp_path: &std::path::Path,
        reporter: &crate::ai::Reporter,
        mut report: FolderSyncReport,
    ) -> Result<FolderSyncReport> {
        use crate::sharedfolder::{self, JournalEntry, Payload, WriteRequest};
        let mut applied: std::collections::BTreeSet<String> = Default::default();
        let mut journal: Vec<JournalEntry> = Vec::new();
        let mut processed_outbox: Vec<i64> = Vec::new();
        let mut temp: Option<Store> = None;
        let base_snapshot = copies.first().map(|c| &c.payload.snapshot);
        if first_write {
            reporter.step("Preparando a primeira cópia a partir do acervo deste computador…");
            for entry in &outbox {
                applied.insert(entry.uid.clone());
                journal.push(JournalEntry {
                    uid: entry.uid.clone(),
                    created_at: entry.created_at.clone(),
                    actor: entry.actor.clone(),
                    writer: cfg.folder_writer.clone(),
                    label: entry.label.clone(),
                    mutation: entry.mutation.clone(),
                    base: entry.base.clone(),
                    revision: 0,
                });
                processed_outbox.push(entry.id);
            }
            report.created = true;
        } else {
            let best = &copies[0];
            reporter.step(format!("Carregando a cópia mais recente ({})…", sharedfolder::describe(&best.header)));
            let mut store = Store::open(temp_path).context("Não foi possível preparar o acervo temporário da sincronização.")?;
            store.apply_snapshot(
                &best.payload.snapshot,
                |file_id, _| best.blobs.pdfs.get(&file_id).cloned().context("PDF ausente no arquivo de sincronização."),
                |id| best.blobs.attachments.get(&id).cloned().context("Anexo ausente no arquivo de sincronização."),
            )?;
            applied = best.payload.applied.iter().cloned().collect();
            journal = best.payload.journal.clone();
            if !foreign.is_empty() {
                reporter.step(format!("Conciliando {} alteração(ões) vindas de outras pastas…", foreign.len()));
            }
            for (entry, from) in foreign {
                if applied.contains(&entry.uid) {
                    continue;
                }
                match store.execute_mutation(&entry.actor, entry.mutation.clone()) {
                    Ok(_) => {
                        report.merged_foreign += 1;
                        let revision = store.cabinet_revision()?;
                        applied.insert(entry.uid.clone());
                        journal.push(JournalEntry { revision, ..entry });
                    }
                    Err(error) => {
                        let as_entry = OutboxEntry {
                            id: 0,
                            created_at: entry.created_at.clone(),
                            actor: entry.actor.clone(),
                            label: format!("[da pasta {}] {}", from, entry.label),
                            mutation: entry.mutation.clone(),
                            base: entry.base.clone(),
                            uid: entry.uid.clone(),
                        };
                        self.record_conflict(&as_entry, &format!("{error} (alteração vinda de {from})"), base_snapshot)?;
                        applied.insert(entry.uid.clone());
                        report.conflicts += 1;
                    }
                }
            }
            if !outbox.is_empty() {
                reporter.step(format!("Aplicando {} alteração(ões) deste computador…", outbox.len()));
            }
            for entry in &outbox {
                processed_outbox.push(entry.id);
                if applied.contains(&entry.uid) {
                    continue;
                }
                match store.execute_mutation(&entry.actor, entry.mutation.clone()) {
                    Ok(_) => {
                        report.pushed += 1;
                        let revision = store.cabinet_revision()?;
                        applied.insert(entry.uid.clone());
                        journal.push(JournalEntry {
                            uid: entry.uid.clone(),
                            created_at: entry.created_at.clone(),
                            actor: entry.actor.clone(),
                            writer: cfg.folder_writer.clone(),
                            label: entry.label.clone(),
                            mutation: entry.mutation.clone(),
                            base: entry.base.clone(),
                            revision,
                        });
                    }
                    Err(error) => {
                        self.record_conflict(entry, &error.to_string(), base_snapshot)?;
                        applied.insert(entry.uid.clone());
                        report.conflicts += 1;
                    }
                }
            }
            temp = Some(store);
        }

        reporter.step("Reunindo PDFs e anexos…");
        let (final_snapshot, blobs) = match &temp {
            Some(store) => {
                let snapshot = store.export_snapshot()?;
                let blobs = collect_blobs(store, &snapshot)?;
                (snapshot, blobs)
            }
            None => {
                let snapshot = self.export_snapshot()?;
                let blobs = collect_blobs(self, &snapshot)?;
                (snapshot, blobs)
            }
        };
        let revision = final_snapshot.revision;
        sharedfolder::trim_journal(&mut journal);
        let payload = Payload {
            snapshot: final_snapshot.clone(),
            applied: applied.into_iter().collect(),
            journal,
        };
        let request = WriteRequest {
            lineage,
            revision,
            writer: &cfg.folder_writer,
            writer_name: cfg.member_name.trim(),
            schema_version: crate::storage::SCHEMA_VERSION,
        };
        let changed_content = first_write || report.pushed > 0 || report.merged_foreign > 0;
        let mut targets: Vec<PathBuf> = absent.to_vec();
        for copy in copies {
            // A copy already at this exact state is left alone.
            if changed_content || copy.header.revision < revision {
                targets.push(copy.path.clone());
            }
        }
        if !targets.is_empty() {
            reporter.step(format!("Gravando em {} pasta(s)…", targets.len()));
        }
        let mut failures = Vec::new();
        for path in &targets {
            match sharedfolder::write(path, key, &request, &payload, &blobs) {
                Ok(_) => report.written_to.push(path.display().to_string()),
                Err(error) => failures.push(error.to_string()),
            }
        }
        if changed_content {
            ensure!(
                !report.written_to.is_empty(),
                "Nenhuma pasta pôde ser gravada: {}. As alterações continuam guardadas neste computador.",
                failures.join("; ")
            );
        }
        report.notes.extend(failures);

        if let Some(store) = &temp {
            reporter.step("Atualizando o acervo deste computador…");
            let before = self.version_map()?;
            self.apply_snapshot(
                &final_snapshot,
                |file_id, capture_id| store.pdf_bytes(file_id, capture_id),
                |id| store.attachment_bytes(id).map(|(_, bytes)| bytes),
            )?;
            for (kind, id, meta) in &final_snapshot.item_versions {
                if kind == "cabinet" {
                    continue;
                }
                match before.get(&(kind.clone(), *id)) {
                    None => report.imported_new += 1,
                    Some(version) if *version != meta.version => report.imported_changed += 1,
                    _ => {}
                }
            }
            report.changed = true;
        }
        for id in processed_outbox {
            self.conn
                .execute("DELETE FROM offline_outbox WHERE id=?1", params![id])?;
        }
        let mut updated = cfg.clone();
        updated.synced_revision = revision;
        updated.last_sync_at = now_iso();
        updated.folder_lineage = lineage.to_owned();
        self.save_intranet_config(&updated)?;
        report.revision = revision;
        Ok(report)
    }

    fn record_conflict(
        &mut self,
        entry: &OutboxEntry,
        reason: &str,
        host_snapshot: Option<&CabinetSnapshot>,
    ) -> Result<()> {
        let conflict = build_sync_conflict(entry, reason, host_snapshot);
        let blob = seal(&serde_json::to_vec(&conflict)?)?;
        self.conn.execute(
            "INSERT INTO offline_conflicts(created_at,payload) VALUES(?1,?2)",
            params![conflict.created_at, blob],
        )?;
        Ok(())
    }

    fn version_map(&self) -> Result<BTreeMap<(String, i64), i64>> {
        let mut statement = self
            .conn
            .prepare("SELECT kind,item_id,version FROM item_versions")?;
        let rows = statement.query_map([], |r| {
            Ok(((r.get::<_, String>(0)?, r.get::<_, i64>(1)?), r.get::<_, i64>(2)?))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (key, version) = row?;
            out.insert(key, version);
        }
        Ok(out)
    }

    pub(crate) fn remote_mutate(
        &mut self,
        cfg: &IntranetConfig,
        mutation: IntranetMutation,
    ) -> Result<MutationReply> {
        cfg.validate()?;
        let actor = cfg.member_name.trim().to_owned();
        if cfg.mode == IntranetMode::SharedFolder {
            // There is no host to ask: every change is applied here and
            // queued for the next exchange through the folders.
            return self.queue_locally(&actor, mutation);
        }
        let attempt = if self.host_known_unreachable() {
            Err(RpcError::Unreachable(
                "Computador principal fora de alcance (verificado há instantes).".into(),
            ))
        } else {
            send_rpc_classified(
                cfg,
                &RpcRequest::Mutate {
                    actor: actor.clone(),
                    mutation: mutation.clone(),
                },
            )
        };
        match attempt {
            Ok(RpcResponse::Mutated {
                reply, snapshot, ..
            }) => {
                let cfg_clone = cfg.clone();
                self.apply_snapshot(
                    &snapshot,
                    |file_id, capture_id| fetch_remote_pdf(&cfg_clone, file_id, capture_id),
                    |id| fetch_remote_attachment(&cfg_clone, id),
                )?;
                Ok(reply)
            }
            Ok(RpcResponse::Error { message }) | Err(RpcError::Rejected(message)) => {
                bail!("{message}")
            }
            Ok(_) => bail!("Resposta inesperada do anfitrião ao gravar alteração."),
            Err(RpcError::Unreachable(_)) => {
                // O computador anfitrião está desligado ou inacessível:
                // aplica a alteração localmente e enfileira na caixa de saída offline para sincronização mútua posterior!
                self.note_host_reachability(false);
                self.queue_locally(&actor, mutation)
            }
        }
    }

    /// Applies the change here and queues it, keeping the state before the
    /// change as the common ancestor for a field-by-field merge later.
    fn queue_locally(&mut self, actor: &str, mutation: IntranetMutation) -> Result<MutationReply> {
        let queued_mutation = self.prepare_mutation_for_outbox(mutation.clone());
        let base = self.snapshot_for_mutation(&queued_mutation);
        let reply = self.execute_mutation(actor, mutation)?;
        self.enqueue_offline_mutation(actor, queued_mutation, base)?;
        self.offline_queued += 1;
        Ok(reply)
    }

    fn prepare_mutation_for_outbox(&self, mutation: IntranetMutation) -> IntranetMutation {
        match mutation {
            IntranetMutation::SaveNote {
                document_id,
                note,
                expected_version,
            } => {
                let ver = expected_version.or_else(|| {
                    self.item_version("note", document_id)
                        .ok()
                        .map(|m| m.version)
                });
                IntranetMutation::SaveNote {
                    document_id,
                    note,
                    expected_version: ver,
                }
            }
            IntranetMutation::SaveTask {
                mut task,
                expected_version,
            } => {
                if task.id <= 0 {
                    task.id = 0;
                    IntranetMutation::SaveTask {
                        task,
                        expected_version: None,
                    }
                } else {
                    let ver = expected_version.or_else(|| {
                        self.item_version("task", task.id).ok().map(|m| m.version)
                    });
                    IntranetMutation::SaveTask {
                        task,
                        expected_version: ver,
                    }
                }
            }
            IntranetMutation::SaveWorkContext {
                document_id,
                value,
                expected_version,
            } => {
                let ver = expected_version.or_else(|| {
                    self.item_version("work_context", document_id)
                        .ok()
                        .map(|m| m.version)
                });
                IntranetMutation::SaveWorkContext {
                    document_id,
                    value,
                    expected_version: ver,
                }
            }
            IntranetMutation::SaveSaiopAttendance {
                mut attendance,
                expected_version,
            } => {
                if attendance.id <= 0 {
                    attendance.id = 0;
                    IntranetMutation::SaveSaiopAttendance {
                        attendance,
                        expected_version: None,
                    }
                } else {
                    let ver = expected_version.or_else(|| {
                        self.item_version("saiop", attendance.id)
                            .ok()
                            .map(|m| m.version)
                    });
                    IntranetMutation::SaveSaiopAttendance {
                        attendance,
                        expected_version: ver,
                    }
                }
            }
            other => other,
        }
    }

    /// The current stored state of the item a mutation is about to change.
    /// Only the four editable kinds have a meaningful ancestor.
    fn snapshot_for_mutation(&self, mutation: &IntranetMutation) -> Option<IntranetMutation> {
        match mutation {
            IntranetMutation::SaveNote { document_id, .. } => {
                let note = self.load_document(*document_id).ok()?.note;
                Some(IntranetMutation::SaveNote {
                    document_id: *document_id,
                    note,
                    expected_version: None,
                })
            }
            IntranetMutation::SaveTask { task, .. } if task.id > 0 => {
                let current = self.list_tasks().ok()?.into_iter().find(|t| t.id == task.id)?;
                Some(IntranetMutation::SaveTask {
                    task: current,
                    expected_version: None,
                })
            }
            IntranetMutation::SaveWorkContext { document_id, .. } => {
                let value = self.load_work_context(*document_id).ok()?;
                Some(IntranetMutation::SaveWorkContext {
                    document_id: *document_id,
                    value,
                    expected_version: None,
                })
            }
            IntranetMutation::SaveSaiopAttendance { attendance, .. } if attendance.id > 0 => {
                let current = self
                    .list_saiop_attendances()
                    .ok()?
                    .into_iter()
                    .find(|a| a.id == attendance.id)?;
                Some(IntranetMutation::SaveSaiopAttendance {
                    attendance: current,
                    expected_version: None,
                })
            }
            _ => None,
        }
    }

    fn enqueue_offline_mutation(
        &mut self,
        actor: &str,
        mutation: IntranetMutation,
        base: Option<IntranetMutation>,
    ) -> Result<()> {
        let existing = self.list_offline_outbox()?;
        let target_key = mutation_target_key(&mutation);
        if let Some(key) = target_key {
            if let Some(prev) = existing
                .into_iter()
                .find(|e| mutation_target_key(&e.mutation).as_deref() == Some(key.as_str()))
            {
                let coalesced = preserve_original_base_version(&prev.mutation, mutation);
                let updated = OutboxEntry {
                    id: prev.id,
                    created_at: now_iso(),
                    actor: actor.to_owned(),
                    label: mutation_label(&coalesced),
                    mutation: coalesced,
                    // The ancestor is the state before the FIRST offline edit.
                    base: prev.base.or(base),
                    // A coalesced edit is a new change: a copy that had the
                    // first version must take this one too.
                    uid: uuid::Uuid::new_v4().simple().to_string(),
                };
                self.conn.execute(
                    "UPDATE offline_outbox SET created_at=?1, payload=?2 WHERE id=?3",
                    params![
                        updated.created_at,
                        seal(&serde_json::to_vec(&updated)?)?,
                        prev.id
                    ],
                )?;
                return Ok(());
            }
        }
        let entry = OutboxEntry {
            id: 0,
            created_at: now_iso(),
            actor: actor.to_owned(),
            label: mutation_label(&mutation),
            mutation,
            base,
            uid: uuid::Uuid::new_v4().simple().to_string(),
        };
        self.conn.execute(
            "INSERT INTO offline_outbox(created_at,payload) VALUES(?1,?2)",
            params![entry.created_at, seal(&serde_json::to_vec(&entry)?)?],
        )?;
        Ok(())
    }

    pub fn list_offline_outbox(&self) -> Result<Vec<OutboxEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,created_at,payload FROM offline_outbox ORDER BY id ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, created_at, blob) = row?;
            let mut item: OutboxEntry = serde_json::from_slice(&unseal(&blob)?)?;
            item.id = id;
            item.created_at = created_at;
            if item.uid.is_empty() {
                // Queued by an older version: a stable stand-in identity.
                item.uid = format!("legacy-{}-{}", item.created_at, id);
            }
            out.push(item);
        }
        Ok(out)
    }

    pub fn list_sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,created_at,payload FROM offline_conflicts ORDER BY id ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, created_at, blob) = row?;
            let mut item: SyncConflict = serde_json::from_slice(&unseal(&blob)?)?;
            item.id = id;
            item.created_at = created_at;
            out.push(item);
        }
        Ok(out)
    }

    pub fn resolve_sync_conflict(
        &mut self,
        conflict_id: i64,
        resolution: ConflictResolution,
    ) -> Result<()> {
        let conflicts = self.list_sync_conflicts()?;
        let conflict = conflicts
            .into_iter()
            .find(|c| c.id == conflict_id)
            .context("Choque de sincronização não encontrado.")?;

        match resolution {
            ConflictResolution::KeepHost => {
                // Mantém a versão que já está no anfitrião (e sincronizada localmente) e remove o alerta de choque.
            }
            ConflictResolution::KeepLocal => {
                let rebased = self.rebase_mutation_to_current(conflict.mutation)?;
                let cfg = self.load_intranet_config()?;
                if cfg.mode.queues_changes() {
                    self.remote_mutate(&cfg, rebased)?;
                } else {
                    self.execute_mutation(&conflict.actor, rebased)?;
                }
            }
            ConflictResolution::Merge => {
                let merged = self.build_merged_mutation(
                    &conflict.actor,
                    conflict.mutation,
                    conflict.base.as_ref(),
                )?;
                let cfg = self.load_intranet_config()?;
                if cfg.mode.queues_changes() {
                    self.remote_mutate(&cfg, merged)?;
                } else {
                    self.execute_mutation(&conflict.actor, merged)?;
                }
            }
        }
        self.conn.execute(
            "DELETE FROM offline_conflicts WHERE id=?1",
            params![conflict_id],
        )?;
        Ok(())
    }

    fn rebase_mutation_to_current(&self, mutation: IntranetMutation) -> Result<IntranetMutation> {
        Ok(match mutation {
            IntranetMutation::SaveNote {
                document_id, note, ..
            } => {
                let ver = self.item_version("note", document_id)?.version;
                IntranetMutation::SaveNote {
                    document_id,
                    note,
                    expected_version: Some(ver),
                }
            }
            IntranetMutation::SaveTask { task, .. } => {
                let ver = self.item_version("task", task.id)?.version;
                IntranetMutation::SaveTask {
                    task,
                    expected_version: Some(ver),
                }
            }
            IntranetMutation::SaveWorkContext {
                document_id, value, ..
            } => {
                let ver = self.item_version("work_context", document_id)?.version;
                IntranetMutation::SaveWorkContext {
                    document_id,
                    value,
                    expected_version: Some(ver),
                }
            }
            IntranetMutation::SaveSaiopAttendance { attendance, .. } => {
                let ver = self.item_version("saiop", attendance.id)?.version;
                IntranetMutation::SaveSaiopAttendance {
                    attendance,
                    expected_version: Some(ver),
                }
            }
            other => other,
        })
    }

    /// Three-way merge of the offline edit against the host version, using the
    /// state before the offline edit as common ancestor:
    /// - a field changed on one side only takes that side's value;
    /// - a text changed on both sides keeps both texts;
    /// - a scalar changed on both sides keeps the local value and records the
    ///   host value in the item itself, so nothing is lost silently.
    /// Without an ancestor (entries queued by older versions) every differing
    /// field counts as changed on both sides.
    fn build_merged_mutation(
        &self,
        actor: &str,
        mutation: IntranetMutation,
        base: Option<&IntranetMutation>,
    ) -> Result<IntranetMutation> {
        let stamp = format!(
            "[Mesclagem em {} por {actor}]",
            crate::labels::datetime(&now_iso())
        );
        match mutation {
            IntranetMutation::SaveNote {
                document_id,
                note: local_note,
                ..
            } => {
                let host_note = self
                    .load_document(document_id)
                    .map(|d| d.note)
                    .unwrap_or_default();
                let base_note = match base {
                    Some(IntranetMutation::SaveNote { note, .. }) => Some(note.as_str()),
                    _ => None,
                };
                let ver = self.item_version("note", document_id)?.version;
                Ok(IntranetMutation::SaveNote {
                    document_id,
                    note: pick_text(base_note, &host_note, &local_note, actor),
                    expected_version: Some(ver),
                })
            }
            IntranetMutation::SaveTask {
                task: local_task, ..
            } => {
                let ver = self.item_version("task", local_task.id)?.version;
                let host_task = self
                    .list_tasks()?
                    .into_iter()
                    .find(|t| t.id == local_task.id);
                let base_task = match base {
                    Some(IntranetMutation::SaveTask { task, .. }) => Some(task),
                    _ => None,
                };
                let merged_task = if let Some(ht) = host_task {
                    let mut m = ht.clone();
                    let mut kept = Vec::new();
                    macro_rules! scalar {
                        ($field:ident, $label:expr, $show:expr) => {{
                            let (value, both) = pick(
                                base_task.map(|b| &b.$field),
                                &ht.$field,
                                &local_task.$field,
                            );
                            if both {
                                kept.push(format!("{}: no computador principal estava \"{}\"", $label, $show(&ht.$field)));
                            }
                            m.$field = value;
                        }};
                    }
                    let plain = |v: &String| v.clone();
                    scalar!(title, "Título", plain);
                    scalar!(action, "Ação", |v: &String| crate::tasks::action_label(v).to_owned());
                    scalar!(status, "Situação", |v: &String| crate::tasks::status_label(v).to_owned());
                    scalar!(priority, "Prioridade", |v: &String| crate::labels::priority(v).to_owned());
                    scalar!(due_date, "Prazo", |v: &String| crate::labels::date(v));
                    scalar!(due_source, "Origem do prazo", plain);
                    scalar!(reminder_date, "Lembrete", |v: &String| crate::labels::date(v));
                    scalar!(assignee, "Responsável", plain);
                    m.notes = pick_text(base_task.map(|b| b.notes.as_str()), &ht.notes, &local_task.notes, actor);
                    m.outcome = pick_text(base_task.map(|b| b.outcome.as_str()), &ht.outcome, &local_task.outcome, actor);
                    for (done_l, text_l) in &local_task.checklist {
                        if let Some((done_m, _)) = m.checklist.iter_mut().find(|(_, t)| t == text_l) {
                            *done_m = *done_m || *done_l;
                        } else {
                            m.checklist.push((*done_l, text_l.clone()));
                        }
                    }
                    for dep in &local_task.dependencies {
                        if !m.dependencies.contains(dep) {
                            m.dependencies.push(*dep);
                        }
                    }
                    m.completed_at = if m.status == "concluida" {
                        local_task.completed_at.clone().or(ht.completed_at.clone())
                    } else {
                        None
                    };
                    if !kept.is_empty() {
                        m.notes.push_str(&format!(
                            "\n\n{stamp} Campos alterados nos dois lados; sua versão prevaleceu:\n• {}",
                            kept.join("\n• ")
                        ));
                    }
                    m
                } else {
                    local_task
                };
                Ok(IntranetMutation::SaveTask {
                    task: merged_task,
                    expected_version: Some(ver),
                })
            }
            IntranetMutation::SaveWorkContext {
                document_id,
                value: local_ctx,
                ..
            } => {
                let ver = self.item_version("work_context", document_id)?.version;
                let host_ctx = self.load_work_context(document_id).unwrap_or_default();
                let base_ctx = match base {
                    Some(IntranetMutation::SaveWorkContext { value, .. }) => Some(value),
                    _ => None,
                };
                let mut m = host_ctx.clone();
                let mut kept = Vec::new();
                macro_rules! scalar {
                    ($field:ident, $label:expr, $show:expr) => {{
                        let (value, both) = pick(
                            base_ctx.map(|b| &b.$field),
                            &host_ctx.$field,
                            &local_ctx.$field,
                        );
                        if both {
                            kept.push(format!("{}: no computador principal estava \"{}\"", $label, $show(&host_ctx.$field)));
                        }
                        m.$field = value;
                    }};
                }
                let plain = |v: &String| v.clone();
                scalar!(purpose, "Finalidade", |v: &String| crate::work_context::purpose_label(v).to_owned());
                scalar!(document_type, "Tipo local", plain);
                scalar!(priority, "Prioridade", |v: &String| crate::labels::priority(v).to_owned());
                scalar!(sender, "Remetente informado", plain);
                scalar!(received_date, "Recebido em", |v: &String| crate::labels::date(v));
                scalar!(process_number, "Processo vinculado", plain);
                m.subjects = pick_text(base_ctx.map(|b| b.subjects.as_str()), &host_ctx.subjects, &local_ctx.subjects, actor);
                m.priority_reason = pick_text(base_ctx.map(|b| b.priority_reason.as_str()), &host_ctx.priority_reason, &local_ctx.priority_reason, actor);
                m.notes = pick_text(base_ctx.map(|b| b.notes.as_str()), &host_ctx.notes, &local_ctx.notes, actor);
                if !kept.is_empty() {
                    m.notes.push_str(&format!(
                        "\n\n{stamp} Campos alterados nos dois lados; sua versão prevaleceu:\n• {}",
                        kept.join("\n• ")
                    ));
                }
                Ok(IntranetMutation::SaveWorkContext {
                    document_id,
                    value: m,
                    expected_version: Some(ver),
                })
            }
            IntranetMutation::SaveSaiopAttendance {
                attendance: local_att,
                ..
            } => {
                let ver = self.item_version("saiop", local_att.id)?.version;
                let host_att = self
                    .list_saiop_attendances()?
                    .into_iter()
                    .find(|a| a.id == local_att.id);
                let base_att = match base {
                    Some(IntranetMutation::SaveSaiopAttendance { attendance, .. }) => Some(attendance),
                    _ => None,
                };
                let merged_att = if let Some(ha) = host_att {
                    let mut m = ha.clone();
                    let mut kept = Vec::new();
                    macro_rules! scalar {
                        ($field:ident, $label:expr, $show:expr) => {{
                            let (value, both) = pick(
                                base_att.map(|b| &b.$field),
                                &ha.$field,
                                &local_att.$field,
                            );
                            if both {
                                kept.push(format!("{}: no computador principal estava \"{}\"", $label, $show(&ha.$field)));
                            }
                            m.$field = value;
                        }};
                    }
                    let plain = |v: &String| v.clone();
                    let flag = |v: &bool| if *v { "sim".to_owned() } else { "não".to_owned() };
                    scalar!(reference_raw, "Referência", plain);
                    scalar!(subject, "Assunto", plain);
                    scalar!(entry_date, "Entrada", |v: &String| crate::labels::date(v));
                    scalar!(assignee, "Responsável", plain);
                    scalar!(received_from, "Recebido de", plain);
                    scalar!(uas_return, "Retorno para UAS", |v: &String| crate::saiop::tri_state_label(v).to_owned());
                    scalar!(due_date, "Prazo", |v: &String| crate::labels::date(v));
                    scalar!(due_kind, "Tipo de prazo", |v: &String| crate::saiop::due_kind_label(v).to_owned());
                    scalar!(status, "Situação", |v: &String| crate::saiop::status_label(v).to_owned());
                    scalar!(priority, "Prioridade", |v: &String| crate::saiop::priority_label(v).to_owned());
                    scalar!(priority_reason, "Motivo da prioridade", plain);
                    scalar!(signed_status, "Assinado", |v: &String| crate::saiop::tri_state_label(v).to_owned());
                    scalar!(exit_date, "Data de saída", |v: &String| crate::labels::date(v));
                    scalar!(planned_doc_type, "Entrega prevista", plain);
                    scalar!(sent_to, "Enviado para", plain);
                    scalar!(outcome, "Resultado da etapa", |v: &String| crate::saiop::outcome_label(v).to_owned());
                    scalar!(scheduled, "Agendado", flag);
                    scalar!(sensitive, "Documento sensível", flag);
                    scalar!(attachment_relation, "Apensado / juntado", plain);
                    m.reference_norm = crate::saiop::normalize_reference(&m.reference_raw);
                    m.observation = pick_text(base_att.map(|b| b.observation.as_str()), &ha.observation, &local_att.observation, actor);
                    for tag in &local_att.tags {
                        if !m.tags.contains(tag) {
                            m.tags.push(tag.clone());
                        }
                    }
                    for tid in &local_att.task_ids {
                        if !m.task_ids.contains(tid) {
                            m.task_ids.push(*tid);
                        }
                    }
                    for flag in &local_att.review_flags {
                        if !m.review_flags.contains(flag) {
                            m.review_flags.push(flag.clone());
                        }
                    }
                    for ev in &local_att.events {
                        if !m.events.iter().any(|e| e.when == ev.when && e.kind == ev.kind && e.detail == ev.detail) {
                            m.events.push(ev.clone());
                        }
                    }
                    m.events.sort_by(|a, b| a.when.cmp(&b.when));
                    if m.status != ha.status {
                        m.stage_changed_at = local_att.stage_changed_at.clone();
                        m.closed_at = local_att.closed_at.clone();
                    }
                    if !kept.is_empty() {
                        m.events.push(crate::saiop::SaiopEvent {
                            when: now_iso(),
                            actor: actor.to_owned(),
                            kind: "mesclagem".into(),
                            detail: format!(
                                "Campos alterados nos dois lados; a versão de {actor} prevaleceu. {}",
                                kept.join("; ")
                            ),
                        });
                    }
                    m
                } else {
                    local_att
                };
                Ok(IntranetMutation::SaveSaiopAttendance {
                    attendance: merged_att,
                    expected_version: Some(ver),
                })
            }
            other => self.rebase_mutation_to_current(other),
        }
    }

    /// The fields in which the host (as mirrored locally after the last sync)
    /// and the offline edit disagree, for the person to compare before deciding.
    pub fn conflict_differences(&self, conflict: &SyncConflict) -> Vec<FieldDiff> {
        fn diff(
            host: Vec<(&'static str, String)>,
            local: Vec<(&'static str, String)>,
            base: Option<Vec<(&'static str, String)>>,
        ) -> Vec<FieldDiff> {
            host.into_iter()
                .zip(local)
                .enumerate()
                .filter(|(_, ((_, h), (_, l)))| h != l)
                .map(|(index, ((label, h), (_, l)))| FieldDiff {
                    label: label.to_owned(),
                    host: h,
                    local: l,
                    base: base.as_ref().and_then(|b| b.get(index).map(|(_, v)| v.clone())),
                })
                .collect()
        }
        match &conflict.mutation {
            IntranetMutation::SaveNote {
                document_id, note, ..
            } => {
                let host = self.load_document(*document_id).map(|d| d.note).unwrap_or_default();
                let base = match &conflict.base {
                    Some(IntranetMutation::SaveNote { note, .. }) => Some(note.clone()),
                    _ => None,
                };
                if host == *note {
                    vec![]
                } else {
                    vec![FieldDiff { label: "Notas do documento".into(), host, local: note.clone(), base }]
                }
            }
            IntranetMutation::SaveTask { task, .. } => {
                let Some(host) = self.list_tasks().ok().and_then(|t| t.into_iter().find(|h| h.id == task.id)) else {
                    return vec![];
                };
                let base = match &conflict.base {
                    Some(IntranetMutation::SaveTask { task, .. }) => Some(task_fields(task)),
                    _ => None,
                };
                diff(task_fields(&host), task_fields(task), base)
            }
            IntranetMutation::SaveWorkContext { document_id, value, .. } => {
                let host = self.load_work_context(*document_id).unwrap_or_default();
                let base = match &conflict.base {
                    Some(IntranetMutation::SaveWorkContext { value, .. }) => Some(context_fields(value)),
                    _ => None,
                };
                diff(context_fields(&host), context_fields(value), base)
            }
            IntranetMutation::SaveSaiopAttendance { attendance, .. } => {
                let Some(host) = self
                    .list_saiop_attendances()
                    .ok()
                    .and_then(|a| a.into_iter().find(|h| h.id == attendance.id))
                else {
                    return vec![];
                };
                let base = match &conflict.base {
                    Some(IntranetMutation::SaveSaiopAttendance { attendance, .. }) => Some(saiop_fields(attendance)),
                    _ => None,
                };
                diff(saiop_fields(&host), saiop_fields(attendance), base)
            }
            _ => vec![],
        }
    }
}

/// `(value, changed_on_both_sides)` for a scalar field in a three-way merge.
fn pick<T: PartialEq + Clone>(base: Option<&T>, host: &T, local: &T) -> (T, bool) {
    if local == host {
        return (host.clone(), false);
    }
    match base {
        Some(b) if local == b => (host.clone(), false),
        Some(b) if host == b => (local.clone(), false),
        _ => (local.clone(), true),
    }
}

fn pick_text(base: Option<&str>, host: &str, local: &str, actor: &str) -> String {
    if local == host {
        return host.to_owned();
    }
    match base {
        Some(b) if local == b => host.to_owned(),
        Some(b) if host == b => local.to_owned(),
        _ => merge_text_blocks(host, local, actor),
    }
}

fn task_fields(t: &Task) -> Vec<(&'static str, String)> {
    vec![
        ("Título", t.title.clone()),
        ("Ação", crate::tasks::action_label(&t.action).to_owned()),
        ("Situação", crate::tasks::status_label(&t.status).to_owned()),
        ("Prioridade", crate::labels::priority(&t.priority).to_owned()),
        ("Prazo", crate::labels::date(&t.due_date)),
        ("Origem do prazo", t.due_source.clone()),
        ("Lembrete", crate::labels::date(&t.reminder_date)),
        ("Responsável", t.assignee.clone()),
        ("Observações", t.notes.clone()),
        ("Resultado", t.outcome.clone()),
        (
            "Checklist",
            t.checklist
                .iter()
                .map(|(done, text)| format!("{} {text}", if *done { "☑" } else { "☐" }))
                .collect::<Vec<_>>()
                .join("; "),
        ),
    ]
}

fn context_fields(c: &WorkContext) -> Vec<(&'static str, String)> {
    vec![
        ("Finalidade", crate::work_context::purpose_label(&c.purpose).to_owned()),
        ("Assuntos", c.subjects.clone()),
        ("Tipo local", c.document_type.clone()),
        ("Prioridade", crate::labels::priority(&c.priority).to_owned()),
        ("Motivo da prioridade", c.priority_reason.clone()),
        ("Remetente informado", c.sender.clone()),
        ("Recebido em", crate::labels::date(&c.received_date)),
        ("Processo vinculado", c.process_number.clone()),
        ("Orientação e contexto", c.notes.clone()),
    ]
}

fn saiop_fields(a: &crate::saiop::SaiopAttendance) -> Vec<(&'static str, String)> {
    let flag = |v: bool| if v { "sim".to_owned() } else { "não".to_owned() };
    vec![
        ("Referência", a.reference_raw.clone()),
        ("Assunto", a.subject.clone()),
        ("Entrada", crate::labels::date(&a.entry_date)),
        ("Responsável", a.assignee.clone()),
        ("Recebido de", a.received_from.clone()),
        ("Retorno para UAS", crate::saiop::tri_state_label(&a.uas_return).to_owned()),
        ("Prazo", crate::labels::date(&a.due_date)),
        ("Tipo de prazo", crate::saiop::due_kind_label(&a.due_kind).to_owned()),
        ("Situação", crate::saiop::status_label(&a.status).to_owned()),
        ("Prioridade", crate::saiop::priority_label(&a.priority).to_owned()),
        ("Motivo da prioridade", a.priority_reason.clone()),
        ("Assinado", crate::saiop::tri_state_label(&a.signed_status).to_owned()),
        ("Data de saída", crate::labels::date(&a.exit_date)),
        ("Entrega prevista", a.planned_doc_type.clone()),
        ("Enviado para", a.sent_to.clone()),
        ("Resultado da etapa", crate::saiop::outcome_label(&a.outcome).to_owned()),
        ("Agendado", flag(a.scheduled)),
        ("Documento sensível", flag(a.sensitive)),
        ("Apensado / juntado", a.attachment_relation.clone()),
        ("Observações", a.observation.clone()),
    ]
}

fn merge_text_blocks(host_text: &str, local_text: &str, actor: &str) -> String {
    let h = host_text.trim();
    let l = local_text.trim();
    if h.is_empty() {
        return l.to_owned();
    }
    if l.is_empty() || h == l || h.contains(l) {
        return h.to_owned();
    }
    if l.contains(h) {
        return l.to_owned();
    }
    format!("{h}\n\n[Mesclado da edição offline de {actor}]:\n{l}")
}

fn mutation_target_key(mutation: &IntranetMutation) -> Option<String> {
    match mutation {
        IntranetMutation::SaveNote { document_id, .. } => Some(format!("note:{document_id}")),
        IntranetMutation::SaveTask { task, .. } if task.id > 0 => {
            Some(format!("task:{}", task.id))
        }
        IntranetMutation::SaveWorkContext { document_id, .. } => {
            Some(format!("work_context:{document_id}"))
        }
        IntranetMutation::SaveSaiopAttendance { attendance, .. } if attendance.id > 0 => {
            Some(format!("saiop:{}", attendance.id))
        }
        _ => None,
    }
}

fn preserve_original_base_version(
    previous: &IntranetMutation,
    next: IntranetMutation,
) -> IntranetMutation {
    match (previous, next) {
        (
            IntranetMutation::SaveNote {
                expected_version: orig,
                ..
            },
            IntranetMutation::SaveNote {
                document_id, note, ..
            },
        ) => IntranetMutation::SaveNote {
            document_id,
            note,
            expected_version: *orig,
        },
        (
            IntranetMutation::SaveTask {
                expected_version: orig,
                ..
            },
            IntranetMutation::SaveTask { task, .. },
        ) => IntranetMutation::SaveTask {
            task,
            expected_version: *orig,
        },
        (
            IntranetMutation::SaveWorkContext {
                expected_version: orig,
                ..
            },
            IntranetMutation::SaveWorkContext {
                document_id, value, ..
            },
        ) => IntranetMutation::SaveWorkContext {
            document_id,
            value,
            expected_version: *orig,
        },
        (
            IntranetMutation::SaveSaiopAttendance {
                expected_version: orig,
                ..
            },
            IntranetMutation::SaveSaiopAttendance { attendance, .. },
        ) => IntranetMutation::SaveSaiopAttendance {
            attendance,
            expected_version: *orig,
        },
        (_, other) => other,
    }
}

fn mutation_label(mutation: &IntranetMutation) -> String {
    match mutation {
        IntranetMutation::ImportCapture { capture } if capture.is_manual() => {
            if capture.document_number == crate::model::MANUAL_PENDING {
                "Registro manual novo".to_owned()
            } else {
                format!("Registro manual {}", capture.document_number)
            }
        }
        IntranetMutation::ImportCapture { capture } => {
            format!("Captura de {}", capture.document_number)
        }
        IntranetMutation::ImportBundle { capture, files } => format!(
            "Captura de {} com {} PDF(s)",
            capture.document_number,
            files.len()
        ),
        IntranetMutation::SaveNote { document_id, .. } => {
            format!("Notas do documento #{document_id}")
        }
        IntranetMutation::SaveDraft {
            document_id, kind, ..
        } => format!("Minuta ({kind}) do documento #{document_id}"),
        IntranetMutation::SaveAnalysis { document_id, .. } => {
            format!("Análise do documento #{document_id}")
        }
        IntranetMutation::SaveTask { task, .. } => {
            if task.id > 0 {
                format!("Tarefa #{}: {}", task.id, task.title)
            } else {
                format!("Nova tarefa: {}", task.title)
            }
        }
        IntranetMutation::SaveWorkContext { document_id, .. } => {
            format!("Contexto do documento #{document_id}")
        }
        IntranetMutation::SavePageText { file_id, page, .. } => {
            format!("OCR da página {page} (arquivo #{file_id})")
        }
        IntranetMutation::AddDocumentLink { source, target, .. } => {
            format!("Vínculo entre documentos #{source} e #{target}")
        }
        IntranetMutation::SetLinkActive { id, .. } => format!("Estado do vínculo #{id}"),
        IntranetMutation::TrashItem { kind, id } => format!(
            "Mover {} #{id} para a lixeira",
            crate::labels::item_kind(kind)
        ),
        IntranetMutation::RestoreItem { kind, id } => format!(
            "Restaurar {} #{id} da lixeira",
            crate::labels::item_kind(kind)
        ),
        IntranetMutation::DeleteDocument { id } => format!("Excluir documento #{id}"),
        IntranetMutation::SaveSaiopAttendance { attendance, .. } => {
            if attendance.id > 0 {
                format!(
                    "Atendimento SAIOP #{}: {}",
                    attendance.id, attendance.reference_raw
                )
            } else {
                format!("Novo atendimento SAIOP: {}", attendance.reference_raw)
            }
        }
        IntranetMutation::ImportSaiopBatch { file_name, .. } => {
            format!("Importação de planilha {file_name}")
        }
        IntranetMutation::UndoSaiopBatch { batch_id } => {
            format!("Reversão do lote SAIOP #{batch_id}")
        }
        IntranetMutation::AddAttachment {
            owner_kind,
            owner_id,
            name,
            ..
        } => format!(
            "Anexo “{name}” em {} #{owner_id}",
            crate::labels::item_kind(owner_kind)
        ),
        IntranetMutation::UpdateAttachment { id, .. } => format!("Descrição do anexo #{id}"),
    }
}

/// What one exchange with the folders did, for the window and the bar.
#[derive(Clone, Debug, Default)]
pub struct FolderSyncReport {
    pub changed: bool,
    pub created: bool,
    pub revision: i64,
    pub pushed: usize,
    pub merged_foreign: usize,
    pub conflicts: usize,
    pub imported_new: usize,
    pub imported_changed: usize,
    pub written_to: Vec<String>,
    pub unreachable: Vec<String>,
    pub notes: Vec<String>,
}

impl FolderSyncReport {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.created {
            parts.push("primeira cópia criada a partir deste computador".to_owned());
        }
        if self.imported_new > 0 || self.imported_changed > 0 {
            parts.push(format!(
                "{} item(ns) novo(s) e {} atualizado(s) recebidos",
                self.imported_new, self.imported_changed
            ));
        }
        if self.pushed > 0 {
            parts.push(format!("{} alteração(ões) deste computador enviada(s)", self.pushed));
        }
        if self.merged_foreign > 0 {
            parts.push(format!("{} alteração(ões) de outras pastas conciliada(s)", self.merged_foreign));
        }
        if self.conflicts > 0 {
            parts.push(format!("⚠ {} conflito(s) para você decidir", self.conflicts));
        }
        if !self.written_to.is_empty() {
            parts.push(format!("gravado em {} pasta(s)", self.written_to.len()));
        }
        if !self.unreachable.is_empty() {
            parts.push(format!("{} pasta(s) fora de alcance", self.unreachable.len()));
        }
        if parts.is_empty() {
            format!("em dia (revisão {})", self.revision)
        } else {
            format!("{} (revisão {})", parts.join(" • "), self.revision)
        }
    }
}

/// PDF and attachment bytes referenced by a snapshot, from the store that
/// holds them.
fn collect_blobs(store: &Store, snapshot: &CabinetSnapshot) -> Result<crate::sharedfolder::Blobs> {
    let mut blobs = crate::sharedfolder::Blobs::default();
    for meta in &snapshot.files_meta {
        blobs.pdfs.insert(meta.id, store.pdf_bytes(meta.id, meta.capture_id)?);
    }
    for row in &snapshot.attachments {
        blobs.attachments.insert(row.id, store.attachment_bytes(row.id)?.1);
    }
    Ok(blobs)
}

fn build_sync_conflict(
    entry: &OutboxEntry,
    reason: &str,
    host_snapshot: Option<&CabinetSnapshot>,
) -> SyncConflict {
    let (local_summary, host_summary) = match &entry.mutation {
        IntranetMutation::SaveNote {
            document_id, note, ..
        } => {
            let host_note = host_snapshot
                .and_then(|s| {
                    s.notes
                        .iter()
                        .filter(|n| n.document_id == *document_id)
                        .max_by_key(|n| n.id)
                })
                .map(|n| n.note.clone())
                .unwrap_or_else(|| "(vazio no anfitrião)".into());
            (
                format!("Nota offline: {}", note.trim()),
                format!("Nota no anfitrião: {}", host_note.trim()),
            )
        }
        IntranetMutation::SaveTask { task, .. } => {
            let host_desc = host_snapshot
                .and_then(|s| s.tasks.iter().find(|t| t.id == task.id))
                .map(|t| {
                    format!(
                        "Título: '{}' | Resp: '{}' | Situação: {} | Notas: '{}'",
                        t.title,
                        t.assignee,
                        crate::tasks::status_label(&t.status),
                        t.notes
                    )
                })
                .unwrap_or_else(|| "(não encontrada no computador principal)".into());
            let local_desc = format!(
                "Título: '{}' | Resp: '{}' | Situação: {} | Notas: '{}'",
                task.title,
                task.assignee,
                crate::tasks::status_label(&task.status),
                task.notes
            );
            (local_desc, host_desc)
        }
        IntranetMutation::SaveWorkContext {
            document_id, value, ..
        } => {
            let host_desc = host_snapshot
                .and_then(|s| s.work_contexts.iter().find(|(id, _)| id == document_id))
                .map(|(_, c)| {
                    format!(
                        "Finalidade: '{}' | Assuntos: '{}' | Notas: '{}'",
                        c.purpose, c.subjects, c.notes
                    )
                })
                .unwrap_or_else(|| "(sem contexto no anfitrião)".into());
            let local_desc = format!(
                "Finalidade: '{}' | Assuntos: '{}' | Notas: '{}'",
                value.purpose, value.subjects, value.notes
            );
            (local_desc, host_desc)
        }
        IntranetMutation::SaveSaiopAttendance { attendance, .. } => {
            let host_desc = host_snapshot
                .and_then(|s| {
                    s.saiop_attendances
                        .iter()
                        .find(|r| r.attendance.id == attendance.id)
                })
                .map(|r| {
                    format!(
                        "Situação: {} | Resp: '{}' | Obs: '{}'",
                        crate::saiop::status_label(&r.attendance.status),
                        r.attendance.assignee,
                        r.attendance.observation
                    )
                })
                .unwrap_or_else(|| "(não localizado no computador principal)".into());
            let local_desc = format!(
                "Situação: {} | Resp: '{}' | Obs: '{}'",
                crate::saiop::status_label(&attendance.status),
                attendance.assignee,
                attendance.observation
            );
            (local_desc, host_desc)
        }
        _ => (entry.label.clone(), "Modificado simultaneamente no anfitrião".into()),
    };

    SyncConflict {
        id: 0,
        created_at: now_iso(),
        actor: entry.actor.clone(),
        item_label: entry.label.clone(),
        base: entry.base.clone(),
        local_summary,
        host_summary,
        reason: reason.to_owned(),
        mutation: entry.mutation.clone(),
    }
}

pub fn ping_host(cfg: &IntranetConfig) -> Result<(i64, String, usize, usize)> {
    validate_host_url(&cfg.host_url)?;
    validate_token(&cfg.cabinet_token)?;
    match send_rpc(cfg, &RpcRequest::Ping)? {
        RpcResponse::Pong {
            revision,
            host_member,
            documents,
            tasks,
        } => Ok((revision, host_member, documents, tasks)),
        RpcResponse::Error { message } => bail!("{message}"),
        _ => bail!("Resposta inesperada ao verificar o anfitrião."),
    }
}

fn fetch_remote_pdf(cfg: &IntranetConfig, file_id: i64, capture_id: i64) -> Result<Vec<u8>> {
    match send_rpc(
        cfg,
        &RpcRequest::FetchFileBytes {
            file_id,
            capture_id,
        },
    )? {
        RpcResponse::FileBytes { data_base64 } => STANDARD
            .decode(data_base64.trim())
            .context("PDF transferido pelo anfitrião está com codificação inválida."),
        RpcResponse::Error { message } => bail!("{message}"),
        _ => bail!("Resposta inesperada ao transferir PDF do anfitrião."),
    }
}

fn fetch_remote_attachment(cfg: &IntranetConfig, id: i64) -> Result<Vec<u8>> {
    match send_rpc(cfg, &RpcRequest::FetchAttachmentBytes { id })? {
        RpcResponse::FileBytes { data_base64 } => STANDARD
            .decode(data_base64.trim())
            .context("Anexo transferido pelo computador principal está com codificação inválida."),
        RpcResponse::Error { message } => bail!("{message}"),
        _ => bail!("Resposta inesperada ao transferir anexo do computador principal."),
    }
}

fn send_rpc(cfg: &IntranetConfig, request: &RpcRequest) -> Result<RpcResponse> {
    match send_rpc_classified(cfg, request) {
        Ok(resp) => Ok(resp),
        Err(RpcError::Unreachable(msg)) | Err(RpcError::Rejected(msg)) => bail!("{msg}"),
    }
}

fn send_rpc_classified(
    cfg: &IntranetConfig,
    request: &RpcRequest,
) -> std::result::Result<RpcResponse, RpcError> {
    let base = validate_host_url(&cfg.host_url).map_err(|e| RpcError::Rejected(e.to_string()))?;
    validate_token(&cfg.cabinet_token).map_err(|e| RpcError::Rejected(e.to_string()))?;
    let endpoint = format!("{base}/api/v1/rpc");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(4))
        .no_proxy()
        .build()
        .map_err(|e| RpcError::Unreachable(format!("Não foi possível preparar o cliente de intranet: {e}")))?;
    let response = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", cfg.cabinet_token.trim()))
        .json(request)
        .send()
        .map_err(|e| {
            RpcError::Unreachable(format!("Sem conexão com o computador anfitrião em {base}: {e}"))
        })?;
    let status = response.status();
    let rpc: RpcResponse = response.json().map_err(|e| {
        RpcError::Rejected(format!(
            "Resposta inválida recebida do computador anfitrião: {e}"
        ))
    })?;
    if !status.is_success() {
        if let RpcResponse::Error { message } = rpc {
            return Err(RpcError::Rejected(message));
        }
        return Err(RpcError::Rejected(format!(
            "O computador anfitrião recusou a operação (HTTP {}).",
            status.as_u16()
        )));
    }
    Ok(rpc)
}

pub struct HostServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl HostServer {
    pub fn start(db_path: PathBuf, bind_addr: &str) -> Result<Self> {
        let requested = validate_bind_addr(bind_addr)?;
        let listener = TcpListener::bind(requested).with_context(|| {
            format!(
                "Não foi possível abrir a porta de intranet em {requested}. Verifique se outra janela já está hospedando nesta porta."
            )
        })?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let db_lock = Arc::new(Mutex::new(db_path));

        let handle = std::thread::Builder::new()
            .name("ponte-intranet-host".into())
            .spawn(move || {
                while !stop_flag.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, peer)) => {
                            if !is_allowed_peer_ip(peer.ip()) {
                                continue;
                            }
                            let db = Arc::clone(&db_lock);
                            let _ = handle_connection(stream, &db);
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            })
            .context("Não foi possível iniciar a thread do anfitrião de intranet.")?;

        Ok(Self {
            addr,
            stop,
            handle: Some(handle),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let connect_ip = if self.addr.ip().is_unspecified() {
            IpAddr::from([127, 0, 0, 1])
        } else {
            self.addr.ip()
        };
        let _ = TcpStream::connect_timeout(
            &SocketAddr::new(connect_ip, self.addr.port()),
            Duration::from_millis(80),
        );
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for HostServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_connection(mut stream: TcpStream, db_lock: &Mutex<PathBuf>) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(20)))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_http_json(
            &mut stream,
            400,
            &RpcResponse::Error {
                message: "Requisição HTTP inválida.".into(),
            },
        );
    }
    let method = parts[0];
    let path = parts[1];

    let mut content_length = 0usize;
    let mut auth_token = String::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let key = name.trim().to_ascii_lowercase();
            let val = value.trim();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            } else if key == "authorization" {
                if let Some(rest) = val.strip_prefix("Bearer ").or_else(|| val.strip_prefix("bearer ")) {
                    auth_token = rest.trim().to_owned();
                }
            }
        }
    }

    if method != "POST" || path != "/api/v1/rpc" {
        return write_http_json(
            &mut stream,
            404,
            &RpcResponse::Error {
                message: "Rota não encontrada no anfitrião do Ponte.".into(),
            },
        );
    }
    if content_length == 0 || content_length > MAX_HTTP_BODY {
        return write_http_json(
            &mut stream,
            413,
            &RpcResponse::Error {
                message: "Tamanho da requisição não permitido.".into(),
            },
        );
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;

    let path_guard = db_lock
        .lock()
        .map_err(|_| anyhow::anyhow!("Bloqueio interno do anfitrião"))?;
    let mut store = match Store::open(&path_guard) {
        Ok(s) => s,
        Err(e) => {
            return write_http_json(
                &mut stream,
                500,
                &RpcResponse::Error {
                    message: format!("Erro ao abrir acervo do anfitrião: {e}"),
                },
            );
        }
    };
    let host_cfg = match store.load_intranet_config() {
        Ok(c) => c,
        Err(e) => {
            return write_http_json(
                &mut stream,
                500,
                &RpcResponse::Error {
                    message: format!("Erro ao ler configuração do anfitrião: {e}"),
                },
            );
        }
    };
    if !verify_token_constant_time(&host_cfg.cabinet_token, &auth_token) {
        return write_http_json(
            &mut stream,
            401,
            &RpcResponse::Error {
                message: "Token do gabinete inválido ou não autorizado.".into(),
            },
        );
    }

    let rpc_req: RpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => {
            return write_http_json(
                &mut stream,
                400,
                &RpcResponse::Error {
                    message: "Formato de mensagem incompatível com o anfitrião.".into(),
                },
            );
        }
    };

    let (status, response) = match process_rpc(&mut store, &host_cfg, rpc_req) {
        Ok(resp) => (200, resp),
        Err(err) => (
            400,
            RpcResponse::Error {
                message: err.to_string(),
            },
        ),
    };
    write_http_json(&mut stream, status, &response)
}

fn process_rpc(
    store: &mut Store,
    host_cfg: &IntranetConfig,
    request: RpcRequest,
) -> Result<RpcResponse> {
    match request {
        RpcRequest::Ping => {
            let revision = store.cabinet_revision()?;
            let documents = store.list_documents("")?.len();
            let tasks = store.list_tasks()?.len();
            Ok(RpcResponse::Pong {
                revision,
                host_member: host_cfg.member_name.clone(),
                documents,
                tasks,
            })
        }
        RpcRequest::Sync { since_revision } => {
            let revision = store.cabinet_revision()?;
            if since_revision == Some(revision) && revision > 0 {
                return Ok(RpcResponse::UpToDate { revision });
            }
            let snapshot = store.export_snapshot()?;
            Ok(RpcResponse::Snapshot(Box::new(snapshot)))
        }
        RpcRequest::FetchFileBytes {
            file_id,
            capture_id,
        } => {
            let bytes = store.pdf_bytes(file_id, capture_id)?;
            Ok(RpcResponse::FileBytes {
                data_base64: STANDARD.encode(&bytes),
            })
        }
        RpcRequest::FetchAttachmentBytes { id } => {
            let (_, bytes) = store.attachment_bytes(id)?;
            Ok(RpcResponse::FileBytes {
                data_base64: STANDARD.encode(&bytes),
            })
        }
        RpcRequest::Mutate { actor, mutation } => {
            ensure!(
                !actor.trim().is_empty()
                    && actor.len() <= 120
                    && !actor.chars().any(char::is_control),
                "Identificação do integrante inválida."
            );
            let reply = store.execute_mutation(&actor, mutation)?;
            let snapshot = store.export_snapshot()?;
            Ok(RpcResponse::Mutated {
                revision: snapshot.revision,
                reply,
                snapshot: Box::new(snapshot),
            })
        }
    }
}

fn write_http_json(stream: &mut TcpStream, status: u16, body: &RpcResponse) -> Result<()> {
    let payload = serde_json::to_vec(body)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()?;
    Ok(())
}

/// Result of one synchronization run on the background thread.
struct SyncOutcome {
    result: std::result::Result<SyncDone, String>,
    manual: bool,
}

/// Long-lived thread with its own database connection. The interface never
/// waits for the network: it posts a request and reads the outcome later.
/// One request to the background synchronization.
struct SyncRequest {
    force_full: bool,
    manual: bool,
    /// Passphrase kept only in the window (shared-folder mode, "not remembered").
    folder_key: Option<String>,
    /// The person confirmed that a library joining the folders is replaced.
    allow_replace: bool,
}

struct SyncWorker {
    requests: mpsc::Sender<SyncRequest>,
    outcomes: mpsc::Receiver<SyncOutcome>,
    /// Steps reported while a synchronization runs.
    progress: mpsc::Receiver<String>,
}

impl SyncWorker {
    fn start(db_path: PathBuf, repaint: Option<egui::Context>) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel::<SyncRequest>();
        let (outcome_tx, outcome_rx) = mpsc::channel::<SyncOutcome>();
        let (progress_tx, progress_rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("ponte-sync".into())
            .spawn(move || {
                let mut store: Option<Store> = None;
                for request in request_rx {
                    if store.is_none() {
                        store = Store::open(&db_path).ok();
                    }
                    let reporter = crate::ai::Reporter::new(
                        progress_tx.clone(),
                        Arc::new(AtomicBool::new(false)),
                    );
                    let result = match store.as_mut() {
                        Some(store) => run_sync(store, &request, &reporter).map_err(|e| e.to_string()),
                        None => Err("Não foi possível abrir o acervo para sincronizar.".into()),
                    };
                    if outcome_tx
                        .send(SyncOutcome {
                            result,
                            manual: request.manual,
                        })
                        .is_err()
                    {
                        break;
                    }
                    if let Some(ctx) = &repaint {
                        ctx.request_repaint();
                    }
                }
            })
            .context("Não foi possível iniciar a sincronização em segundo plano.")?;
        Ok(Self {
            requests: request_tx,
            outcomes: outcome_rx,
            progress: progress_rx,
        })
    }
}

/// What a finished synchronization reports back to the window.
pub struct SyncDone {
    pub changed: bool,
    pub summary: String,
}

fn run_sync(store: &mut Store, request: &SyncRequest, reporter: &crate::ai::Reporter) -> Result<SyncDone> {
    let mut cfg = store.load_intranet_config()?;
    match cfg.mode {
        IntranetMode::SharedFolder => {
            if let Some(key) = &request.folder_key {
                if !key.is_empty() {
                    cfg.folder_key = key.clone();
                }
            }
            let key = cfg.folder_key.clone();
            let report = store.sync_with_folders(&cfg, &key, request.allow_replace, reporter)?;
            Ok(SyncDone {
                changed: report.changed,
                summary: report.summary(),
            })
        }
        _ => {
            reporter.step("Sincronizando com o computador principal…");
            let changed = store.sync_from_host(request.force_full)?;
            Ok(SyncDone {
                changed,
                summary: String::new(),
            })
        }
    }
}

#[derive(Default)]
pub struct IntranetPanel {
    pub open: bool,
    loaded: bool,
    draft: IntranetConfig,
    show_token: bool,
    message: String,
    error: bool,
    pub offline: bool,
    pub status_summary: String,
    server: Option<HostServer>,
    sync: Option<SyncWorker>,
    sync_in_flight: bool,
    last_sync_request: Option<Instant>,
    /// When the last synchronization finished and whether it succeeded.
    pub last_sync_finished: Option<(Instant, bool)>,
    /// Current step of the running synchronization, for the status bar.
    sync_step: String,
    /// Summary of the last folder exchange.
    last_summary: String,
    /// The folders already hold a library and this computer has its own:
    /// the person must confirm the replacement.
    replace_confirmation: Option<String>,
    allow_replace_next: bool,
    show_folder_key: bool,
    folder_key_confirm: String,
    force_next_sync: bool,
    manual_sync: bool,
    pending_resolution: Option<(i64, ConflictResolution)>,
    repaint: Option<egui::Context>,
    connection_code_input: String,
}

const SYNC_INTERVAL: Duration = Duration::from_secs(4);
/// Shared folders are re-read every few minutes; the cloud client needs
/// that long to carry a file anyway. Opening the Ponte and "Sincronizar
/// agora" do not wait.
const FOLDER_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);

impl IntranetPanel {
    /// Lets the background thread wake the window when a sync finishes.
    pub fn attach_repaint(&mut self, ctx: &egui::Context) {
        if self.repaint.is_none() {
            self.repaint = Some(ctx.clone());
        }
    }

    pub fn syncing(&self) -> bool {
        self.sync_in_flight
    }

    /// Asks for a full synchronization on the next refresh, reporting the
    /// result in this window.
    pub fn request_full_sync(&mut self) {
        self.force_next_sync = true;
        self.manual_sync = true;
        self.last_sync_request = None;
    }

    pub fn initialize(&mut self, store: &mut Store, db_path: &PathBuf) {
        if let Ok(cfg) = store.load_intranet_config() {
            self.draft = cfg.clone();
            self.loaded = true;
            if cfg.mode == IntranetMode::Host && cfg.auto_start_host && self.server.is_none() {
                match HostServer::start(db_path.clone(), &cfg.bind_addr) {
                    Ok(srv) => {
                        self.status_summary = format!(
                            "Anfitrião ativo em {} • Integrante: {}",
                            srv.local_addr(),
                            cfg.member_name
                        );
                        self.server = Some(srv);
                    }
                    Err(e) => {
                        self.error = true;
                        self.message = format!("Não foi possível iniciar o anfitrião: {e}");
                    }
                }
            } else if cfg.mode.queues_changes() {
                self.refresh_client(store, db_path);
            }
        }
    }

    /// The "Pasta compartilhada" part of the window.
    fn shared_folder_section(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        db_path: &PathBuf,
        refresh_needed: &mut bool,
    ) {
        ui.strong("Eu, em mais de um computador");
        ui.label("Os computadores trocam o acervo por um arquivo cifrado com a sua senha, guardado nas pastas que você escolher: Google Drive, OneDrive, pendrive, pasta da rede. Pode ser mais de uma pasta; o Ponte usa a cópia mais recente e grava em todas as que alcançar.");
        ui.small("Espere a nuvem terminar de sincronizar antes de abrir o Ponte no outro computador. Sem a senha o arquivo não abre em lugar nenhum, nem para os autores do Ponte.");
        ui.add_space(4.0);
        ui.strong("Pastas (o arquivo ponte-sync.ponte dentro de cada uma)");
        let mut remove: Option<usize> = None;
        let saved_revision = store.load_intranet_config().map(|c| c.synced_revision).unwrap_or(0);
        for (index, path) in self.draft.folder_files.clone().iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{}.", index + 1));
                ui.add(egui::Label::new(path).wrap());
                let state = match crate::sharedfolder::read_header(std::path::Path::new(path)) {
                    Ok(Some(header)) => {
                        if header.revision < saved_revision {
                            format!("({}; atrás deste computador)", crate::sharedfolder::describe(&header))
                        } else {
                            format!("({})", crate::sharedfolder::describe(&header))
                        }
                    }
                    Ok(None) => {
                        if std::path::Path::new(path).parent().is_some_and(|p| p.is_dir()) {
                            "(pasta acessível; arquivo ainda não criado)".to_owned()
                        } else {
                            "(pasta fora de alcance agora)".to_owned()
                        }
                    }
                    Err(error) => format!("({error})"),
                };
                ui.small(state);
                if ui.small_button("Remover").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            self.draft.folder_files.remove(index);
        }
        if self.draft.folder_files.len() < 8
            && ui
                .button("Adicionar pasta…")
                .on_hover_text("Escolha a pasta (do Drive, do OneDrive, do pendrive…) e mantenha o nome ponte-sync.ponte.")
                .clicked()
        {
            if let Some(path) = crate::windows_ui::save_file(
                "Onde guardar o arquivo de sincronização",
                "ponte-sync.ponte",
                &[("Arquivo de sincronização do Ponte", "*.ponte")],
            ) {
                let text = path.display().to_string();
                if !self.draft.folder_files.contains(&text) {
                    self.draft.folder_files.push(text);
                }
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Senha de sincronização:");
            ui.add(
                egui::TextEdit::singleline(&mut self.draft.folder_key)
                    .password(!self.show_folder_key)
                    .desired_width(240.0),
            );
            ui.checkbox(&mut self.show_folder_key, "Mostrar");
        });
        let first_time = saved_revision == 0;
        if first_time {
            ui.horizontal(|ui| {
                ui.label("Repita a senha:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.folder_key_confirm)
                        .password(!self.show_folder_key)
                        .desired_width(240.0),
                );
            });
            ui.small("Mínimo de 8 caracteres. A mesma senha vai ser pedida no outro computador. Anote-a em lugar seguro: sem ela, o arquivo é irrecuperável.");
        }
        ui.checkbox(&mut self.draft.remember_folder_key, "Guardar a senha neste computador")
            .on_hover_text("Fica no acervo, cifrada pela sua conta do Windows. Desmarcada, vale só até fechar o Ponte.");
        ui.add_space(4.0);
        if let Some(text) = self.replace_confirmation.clone() {
            ui.colored_label(egui::Color32::from_rgb(153, 88, 16), format!("⚠ {text}"));
            ui.horizontal(|ui| {
                if ui.button("Substituir o acervo deste computador e sincronizar").clicked() {
                    self.allow_replace_next = true;
                    self.replace_confirmation = None;
                    self.request_full_sync();
                    *refresh_needed = self.refresh_client(store, db_path);
                }
                if ui.button("Cancelar").clicked() {
                    self.replace_confirmation = None;
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            let can_sync = !self.sync_in_flight && !self.draft.folder_files.is_empty();
            if ui
                .add_enabled(can_sync, egui::Button::new("Salvar e sincronizar agora"))
                .clicked()
            {
                let mut problem = None;
                if first_time && self.draft.folder_key != self.folder_key_confirm {
                    problem = Some("As duas senhas não coincidem.".to_owned());
                }
                if let Err(error) = crate::sharedfolder::validate_password(&self.draft.folder_key) {
                    problem = Some(error.to_string());
                }
                match problem {
                    Some(text) => {
                        self.error = true;
                        self.message = text;
                    }
                    None => match store.save_intranet_config(&self.draft) {
                        Ok(()) => {
                            self.error = false;
                            self.message = "Sincronizando com as pastas…".into();
                            self.request_full_sync();
                            *refresh_needed = self.refresh_client(store, db_path);
                        }
                        Err(error) => {
                            self.error = true;
                            self.message = error.to_string();
                        }
                    },
                }
            }
            if self.sync_in_flight {
                ui.spinner();
                ui.small(if self.sync_step.is_empty() { "sincronizando…" } else { self.sync_step.as_str() });
            }
        });
        if !self.last_summary.is_empty() {
            ui.small(format!("Última sincronização: {}.", self.last_summary));
        }
        ui.small("O que você importa ou edita fica salvo neste computador e vai para as pastas ao sincronizar: ao abrir o Ponte, a cada 5 minutos, e em “Sincronizar agora”. Ao fechar, o Ponte avisa se algo ainda não foi enviado.");
    }

    /// Text for the status bar while a synchronization runs.
    pub fn sync_step(&self) -> &str {
        &self.sync_step
    }

    pub fn folder_mode(&self, store: &Store) -> bool {
        store
            .load_intranet_config()
            .is_ok_and(|cfg| cfg.mode == IntranetMode::SharedFolder)
    }

    /// Changes made here and not yet carried to the folders.
    pub fn unsent_folder_changes(&self, store: &Store) -> usize {
        if !self.folder_mode(store) {
            return 0;
        }
        store.list_offline_outbox().map(|v| v.len()).unwrap_or(0)
    }

    pub fn host_running(&self) -> bool {
        self.server.is_some()
    }

    /// Non-blocking: collects the outcome of the previous background sync and,
    /// when due, posts the next request. Returns true when the library changed.
    pub fn refresh_client(&mut self, store: &mut Store, db_path: &PathBuf) -> bool {
        let Ok(cfg) = store.load_intranet_config() else {
            return false;
        };
        if !cfg.mode.queues_changes() {
            self.offline = false;
            self.sync_in_flight = false;
            self.sync_step.clear();
            return false;
        }
        let mut changed = false;
        if let Some(worker) = &self.sync {
            if let Some(step) = worker.progress.try_iter().last() {
                self.sync_step = step;
            }
            match worker.outcomes.try_recv() {
                Ok(outcome) => {
                    self.sync_in_flight = false;
                    self.apply_sync_outcome(store, &cfg, outcome);
                    changed = matches!(self.last_sync_finished, Some((_, true)));
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.sync = None;
                    self.sync_in_flight = false;
                }
            }
        }
        // The folders are checked seldom (a cloud client has to carry the
        // file anyway); the host over HTTP every few seconds, as before.
        let interval = if cfg.mode == IntranetMode::SharedFolder {
            FOLDER_SYNC_INTERVAL
        } else {
            SYNC_INTERVAL
        };
        let due = self
            .last_sync_request
            .is_none_or(|at| at.elapsed() >= interval);
        if cfg.mode == IntranetMode::SharedFolder && self.replace_confirmation.is_some() && !self.manual_sync {
            // Waiting for the person's decision: no automatic retries.
            return changed;
        }
        if !self.sync_in_flight && due {
            if self.sync.is_none() {
                match SyncWorker::start(db_path.clone(), self.repaint.clone()) {
                    Ok(worker) => self.sync = Some(worker),
                    Err(e) => {
                        self.offline = true;
                        self.status_summary = e.to_string();
                        return false;
                    }
                }
            }
            if let Some(worker) = &self.sync {
                let force = std::mem::take(&mut self.force_next_sync);
                let manual = std::mem::take(&mut self.manual_sync);
                let request = SyncRequest {
                    force_full: force,
                    manual,
                    folder_key: Some(self.draft.folder_key.clone()).filter(|k| !k.is_empty()),
                    allow_replace: std::mem::take(&mut self.allow_replace_next),
                };
                if worker.requests.send(request).is_ok() {
                    self.sync_in_flight = true;
                    self.sync_step = "Iniciando…".into();
                    self.last_sync_request = Some(Instant::now());
                } else {
                    self.sync = None;
                }
            }
        }
        changed
    }

    fn apply_sync_outcome(&mut self, store: &mut Store, cfg: &IntranetConfig, outcome: SyncOutcome) {
        let conflicts = store.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        let outbox = store.list_offline_outbox().map(|v| v.len()).unwrap_or(0);
        self.sync_step.clear();
        if cfg.mode == IntranetMode::SharedFolder {
            match outcome.result {
                Ok(done) => {
                    self.offline = false;
                    self.replace_confirmation = None;
                    self.last_sync_finished = Some((Instant::now(), done.changed));
                    self.last_summary = done.summary.clone();
                    let when = store
                        .load_intranet_config()
                        .map(|c| crate::labels::datetime(&c.last_sync_at))
                        .unwrap_or_default();
                    self.status_summary = format!("Pasta compartilhada • {} • {}", done.summary, when);
                    if outcome.manual {
                        self.error = false;
                        self.message = format!("Sincronização concluída: {}.", done.summary);
                    }
                }
                Err(error) => {
                    self.offline = true;
                    self.last_sync_finished = Some((Instant::now(), false));
                    if let Some(rest) = error.strip_prefix("[confirmar-substituicao] ") {
                        self.replace_confirmation = Some(rest.to_owned());
                        self.status_summary = "Pasta compartilhada • aguardando sua confirmação (ver Rede e equipe)".into();
                        self.error = false;
                        self.message.clear();
                    } else {
                        let mut extras = format!(" • {outbox} alteração(ões) aguardando envio");
                        if conflicts > 0 {
                            extras.push_str(&format!(" • ⚠ {conflicts} conflito(s) de edição"));
                        }
                        self.status_summary = format!("Pasta compartilhada fora de alcance{extras}");
                        if outcome.manual {
                            self.error = true;
                            self.message = format!("Não foi possível sincronizar agora: {error}");
                        }
                    }
                }
            }
            return;
        }
        match outcome.result {
            Ok(SyncDone { changed, .. }) => {
                self.offline = false;
                store.note_host_reachability(true);
                self.last_sync_finished = Some((Instant::now(), changed));
                let mut suffix = String::new();
                if conflicts > 0 {
                    suffix.push_str(&format!(" • ⚠ {conflicts} conflito(s) de edição para decidir"));
                }
                if outbox > 0 {
                    suffix.push_str(&format!(" • {outbox} alteração(ões) a enviar"));
                }
                self.status_summary = format!(
                    "Conectado ao computador principal ({}) • você: {} • sincronizado agora{suffix}",
                    cfg.host_url, cfg.member_name
                );
                if outcome.manual {
                    self.error = false;
                    self.message = "Acervo sincronizado com o computador principal.".into();
                }
            }
            Err(error) => {
                self.offline = true;
                store.note_host_reachability(false);
                self.last_sync_finished = Some((Instant::now(), false));
                let mut extras = format!(" • trabalhando neste computador ({outbox} alteração(ões) aguardando envio)");
                if conflicts > 0 {
                    extras.push_str(&format!(" • ⚠ {conflicts} conflito(s) de edição"));
                }
                self.status_summary = format!(
                    "Computador principal fora de alcance ({}){extras}",
                    cfg.host_url
                );
                if outcome.manual {
                    self.error = true;
                    self.message = format!("Não foi possível sincronizar agora: {error}");
                }
            }
        }
    }

    pub fn banner_text(&self, store: &Store) -> String {
        let Ok(cfg) = store.load_intranet_config() else {
            return "Biblioteca local • conteúdo protegido para este usuário do Windows".into();
        };
        let conflicts = store.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        let outbox = store.list_offline_outbox().map(|v| v.len()).unwrap_or(0);
        let mut badge = String::new();
        if conflicts > 0 {
            badge.push_str(&format!(
                " • ⚠ {conflicts} CHOQUE(S) DE EDIÇÃO AGUARDANDO DECISÃO (abra 'Rede / Intranet')"
            ));
        } else if outbox > 0 && cfg.mode == IntranetMode::Client {
            badge.push_str(&format!(
                " • {outbox} alteração(ões) offline aguardando reconexão com o Anfitrião"
            ));
        }
        match cfg.mode {
            IntranetMode::SharedFolder => {
                if !self.status_summary.is_empty() {
                    format!("{}{badge}", self.status_summary)
                } else {
                    let pending = if outbox > 0 {
                        format!(" • {outbox} alteração(ões) a enviar")
                    } else {
                        String::new()
                    };
                    format!(
                        "Pasta compartilhada ({} pasta(s)) • revisão {}{pending}{badge}",
                        cfg.folder_paths().len(),
                        cfg.synced_revision
                    )
                }
            }
            IntranetMode::Local => format!(
                "Biblioteca local ({}) • conteúdo protegido para este usuário do Windows{badge}",
                cfg.member_name
            ),
            IntranetMode::Host => {
                let rev = store.cabinet_revision().unwrap_or(0);
                if let Some(srv) = &self.server {
                    format!(
                        "Modo Anfitrião na Intranet ATIVO ({}) • Integrante: {} • Revisão #{rev}{badge}",
                        srv.local_addr(),
                        cfg.member_name
                    )
                } else {
                    format!(
                        "Modo Anfitrião configurado (parado) • Integrante: {} • Revisão #{rev}{badge}",
                        cfg.member_name
                    )
                }
            }
            IntranetMode::Client => {
                if !self.status_summary.is_empty() {
                    self.status_summary.clone()
                } else {
                    format!(
                        "Modo Cliente na Intranet ({}) • Integrante: {}{badge}",
                        cfg.host_url, cfg.member_name
                    )
                }
            }
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        store: &mut Store,
        db_path: &PathBuf,
    ) -> bool {
        if !self.open {
            return false;
        }
        if !self.loaded {
            if let Ok(cfg) = store.load_intranet_config() {
                self.draft = cfg;
                self.loaded = true;
            }
        }
        let mut open = self.open;
        let mut refresh_needed = false;

        egui::Window::new("Rede e equipe")
            .open(&mut open)
            .default_size([700.0, 560.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.small("A equipe trabalha no mesmo acervo usando um dos computadores do gabinete como principal, na rede interna.");
                    ui.label(egui::RichText::new("ⓘ").color(egui::Color32::from_rgb(120, 130, 140)))
                        .on_hover_text("Nada vai para a nuvem. Cada computador guarda a sua cópia protegida do acervo e conversa com o principal pela rede interna, com um código de acesso do gabinete.");
                });
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Seu nome na equipe:");
                    ui.text_edit_singleline(&mut self.draft.member_name)
                        .on_hover_text("Aparece no histórico de alterações e como responsável ao assumir tarefas.");
                });

                ui.add_space(6.0);
                ui.strong("Modo de operação deste computador");
                for mode in [
                    IntranetMode::Local,
                    IntranetMode::Host,
                    IntranetMode::Client,
                    IntranetMode::SharedFolder,
                ] {
                    ui.radio_value(&mut self.draft.mode, mode, mode.label());
                }

                ui.separator();
                match self.draft.mode {
                    IntranetMode::SharedFolder => {
                        self.shared_folder_section(ui, store, db_path, &mut refresh_needed);
                    }
                    IntranetMode::Local => {
                        ui.label("Neste modo, o Ponte opera apenas no acervo deste computador.");
                        if self.server.is_some() && ui.button("Parar serviço de Anfitrião").clicked() {
                            self.server = None;
                            self.message = "Serviço de anfitrião encerrado.".into();
                            self.error = false;
                        }
                    }
                    IntranetMode::Host => {
                        ui.strong("Este computador como principal");
                        ui.checkbox(
                            &mut self.draft.auto_start_host,
                            "Ficar disponível para a equipe sempre que o Ponte abrir",
                        );
                        let port = self
                            .draft
                            .bind_addr
                            .rsplit_once(':')
                            .and_then(|(_, p)| p.parse::<u16>().ok())
                            .unwrap_or(DEFAULT_PORT);
                        let hints = local_intranet_hints(port);
                        let saved_token = store
                            .load_intranet_config()
                            .map(|c| c.cabinet_token)
                            .unwrap_or_default();
                        ui.horizontal_wrapped(|ui| {
                            if let Some(srv) = &self.server {
                                ui.colored_label(
                                    egui::Color32::from_rgb(20, 118, 72),
                                    format!("● Disponível para a equipe em {}", srv.local_addr()),
                                );
                                if ui.button("Parar").clicked() {
                                    self.server = None;
                                    self.message = "Este computador deixou de estar disponível para a equipe.".into();
                                    self.error = false;
                                }
                            } else if ui.button("Salvar e ficar disponível agora").clicked() {
                                match store.save_intranet_config(&self.draft) {
                                    Ok(()) => match HostServer::start(db_path.clone(), &self.draft.bind_addr) {
                                        Ok(srv) => {
                                            self.message = format!(
                                                "Disponível em {}. Copie o código de conexão e envie aos colegas.",
                                                srv.local_addr()
                                            );
                                            self.error = false;
                                            self.server = Some(srv);
                                        }
                                        Err(e) => {
                                            self.message = e.to_string();
                                            self.error = true;
                                        }
                                    },
                                    Err(e) => {
                                        self.message = e.to_string();
                                        self.error = true;
                                    }
                                }
                            }
                            if ui
                                .button("Copiar código de conexão")
                                .on_hover_text("Um texto único com endereço e chave. Os colegas colam no Ponte deles em Menu ▸ Rede e equipe.")
                                .clicked()
                            {
                                let addr = hints.first().cloned().unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
                                let code = connection_code(&addr, &self.draft.cabinet_token);
                                ui.ctx().copy_text(format!(
                                    "Código de conexão do Ponte (gabinete):\n{code}\n\nNo seu Ponte: Menu ▸ Rede e equipe ▸ “Conectar ao computador principal” ▸ cole o código."
                                ));
                                self.message = if self.draft.cabinet_token.trim() != saved_token.trim() && self.server.is_some() {
                                    "Código copiado com a nova chave. Atenção: até você salvar, o computador principal ainda aceita só a chave anterior.".into()
                                } else {
                                    "Código de conexão copiado. Envie aos colegas por um canal seguro.".into()
                                };
                                self.error = false;
                            }
                        });
                        if hints.len() > 1 {
                            ui.small(format!(
                                "Endereços deste computador na rede: {}",
                                hints.join("  ou  ")
                            ));
                        }
                        ui.small(format!(
                            "Se os colegas não conseguirem conectar, peça à TI para liberar a porta {port} (TCP) no Firewall do Windows deste computador."
                        ))
                        .on_hover_text("Comando para quem administra o computador:\nnetsh advfirewall firewall add rule name=\"Ponte\" dir=in action=allow protocol=TCP localport=PORTA profile=private,domain".replace("PORTA", &port.to_string()));
                        egui::CollapsingHeader::new("Mais opções")
                            .id_salt("host-advanced")
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Endereço de escuta:");
                                    ui.text_edit_singleline(&mut self.draft.bind_addr)
                                        .on_hover_text("IP:porta onde este computador atende a equipe. 0.0.0.0 significa todas as redes deste computador.");
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Chave do gabinete:");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.draft.cabinet_token)
                                            .password(!self.show_token)
                                            .desired_width(300.0),
                                    );
                                    ui.checkbox(&mut self.show_token, "Mostrar");
                                    if ui.button("Gerar nova chave").clicked() {
                                        self.draft.cabinet_token = generate_token();
                                    }
                                });
                                if self.draft.cabinet_token.trim() != saved_token.trim() {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(153, 88, 16),
                                        "Ao salvar a nova chave, todos os colegas precisarão colar um novo código de conexão.",
                                    );
                                }
                            });
                        ui.small("Se este computador for desligado, qualquer outro já conectado pode assumir como principal.").on_hover_text("Cada computador conectado mantém uma cópia completa e protegida do acervo.");
                    }
                    IntranetMode::Client => {
                        ui.strong("Conectar ao computador principal");
                        ui.horizontal(|ui| {
                            ui.label("Código de conexão:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.connection_code_input)
                                    .hint_text("cole aqui o código recebido (ponte://conectar?…)")
                                    .desired_width(360.0),
                            );
                            if ui.button("Usar código").clicked() {
                                match parse_connection_code(&self.connection_code_input) {
                                    Some((host, key)) => {
                                        self.draft.host_url = host;
                                        self.draft.cabinet_token = key;
                                        self.connection_code_input.clear();
                                        match ping_host(&self.draft) {
                                            Ok((_, host_member, docs, _)) => {
                                                self.message = format!(
                                                    "Código válido: computador principal de {host_member}, com {docs} documento(s). Clique em “Salvar e sincronizar” para começar."
                                                );
                                                self.error = false;
                                                self.offline = false;
                                            }
                                            Err(e) => {
                                                self.message = format!("O código foi lido, mas o computador principal não respondeu: {e}");
                                                self.error = true;
                                            }
                                        }
                                    }
                                    None => {
                                        self.message = "Código não reconhecido. Peça ao colega do computador principal para copiar de novo em Rede e equipe ▸ “Copiar código de conexão”.".into();
                                        self.error = true;
                                    }
                                }
                            }
                        });
                        egui::CollapsingHeader::new("Informar endereço e chave separadamente")
                            .id_salt("client-advanced")
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Endereço do computador principal:");
                                    ui.text_edit_singleline(&mut self.draft.host_url)
                                        .on_hover_text("Ex.: http://192.168.1.10:7878");
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Chave do gabinete:");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.draft.cabinet_token)
                                            .password(!self.show_token)
                                            .desired_width(300.0),
                                    );
                                    ui.checkbox(&mut self.show_token, "Mostrar");
                                });
                            });
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Testar conexão").clicked() {
                                match ping_host(&self.draft) {
                                    Ok((_, host_member, docs, tasks)) => {
                                        self.message = format!(
                                            "Conexão confirmada com o computador principal de {host_member}: {docs} documento(s) e {tasks} tarefa(s)."
                                        );
                                        self.error = false;
                                        self.offline = false;
                                    }
                                    Err(e) => {
                                        self.message = format!("Não foi possível conectar: {e}");
                                        self.error = true;
                                    }
                                }
                            }
                            if ui
                                .add_enabled(!self.sync_in_flight, egui::Button::new("Salvar e sincronizar acervo agora"))
                                .clicked()
                            {
                                match store.save_intranet_config(&self.draft) {
                                    Ok(()) => {
                                        self.error = false;
                                        self.message = "Sincronizando com o computador principal…".into();
                                        self.request_full_sync();
                                        refresh_needed = self.refresh_client(store, db_path);
                                    }
                                    Err(e) => {
                                        self.error = true;
                                        self.message = e.to_string();
                                    }
                                }
                            }
                            if self.sync_in_flight {
                                ui.spinner();
                                ui.small("sincronizando…");
                            }
                        });
                        ui.small("O que você importa ou edita aqui vai para o computador principal. Se ele estiver desligado, fica salvo neste computador e é enviado quando a conexão voltar.");
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Salvar configuração").clicked() {
                        match store.save_intranet_config(&self.draft) {
                            Ok(()) => {
                                self.error = false;
                                self.message = "Configuração salva.".into();
                                if self.draft.mode != IntranetMode::Host && self.server.is_some() {
                                    self.server = None;
                                    self.message.push_str(" Este computador deixou de estar disponível para a equipe.");
                                }
                                if self.draft.mode.queues_changes() {
                                    self.last_sync_request = None;
                                    refresh_needed = self.refresh_client(store, db_path);
                                }
                            }
                            Err(e) => {
                                self.error = true;
                                self.message = e.to_string();
                            }
                        }
                    }
                });

                if !self.message.is_empty() {
                    ui.colored_label(
                        if self.error {
                            egui::Color32::from_rgb(168, 40, 45)
                        } else {
                            egui::Color32::from_rgb(24, 84, 97)
                        },
                        &self.message,
                    );
                }

                let outbox = store.list_offline_outbox().unwrap_or_default();
                let conflicts = store.list_sync_conflicts().unwrap_or_default();
                if !outbox.is_empty() || !conflicts.is_empty() {
                    ui.separator();
                    ui.strong("Fila Offline e Resolução de Choques (Sincronização Mútua)");
                    if !outbox.is_empty() {
                        ui.colored_label(
                            egui::Color32::from_rgb(24, 84, 97),
                            format!(
                                "{} alteração(ões) gravada(s) localmente enquanto o Anfitrião estava desligado. Elas serão enviadas automaticamente assim que a conexão voltar.",
                                outbox.len()
                            ),
                        );
                        egui::ScrollArea::vertical()
                            .id_salt("offline_outbox_scroll")
                            .max_height(110.0)
                            .show(ui, |ui| {
                                for entry in &outbox {
                                    ui.small(format!(
                                        "• {} ({} por {})",
                                        entry.label,
                                        crate::labels::datetime(&entry.created_at),
                                        entry.actor
                                    ));
                                }
                            });
                    }

                    if !conflicts.is_empty() {
                        ui.add_space(4.0);
                        ui.colored_label(
                            egui::Color32::from_rgb(168, 40, 45),
                            format!(
                                "⚠ {} conflito(s) de edição: você alterou o item sem conexão e ele também foi alterado no computador principal.",
                                conflicts.len()
                            ),
                        );
                        let mut resolution_action: Option<(i64, ConflictResolution)> = None;
                        egui::ScrollArea::vertical()
                            .id_salt("sync_conflicts_scroll")
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for conflict in &conflicts {
                                    let differences = store.conflict_differences(conflict);
                                    let both_sides = differences.iter().filter(|d| d.changed_by() == "pelos dois lados").count();
                                    ui.group(|ui| {
                                        ui.set_width(ui.available_width());
                                        ui.strong(format!("Conflito: {}", conflict.item_label));
                                        ui.small(format!(
                                            "Sua alteração de {} ({})",
                                            crate::labels::datetime(&conflict.created_at),
                                            conflict.actor
                                        ));
                                        if differences.is_empty() {
                                            ui.label("As duas versões já coincidem campo a campo.");
                                            ui.small(format!("Resumo da sua versão: {}", conflict.local_summary));
                                        } else {
                                            egui::Grid::new(("conflict-diff", conflict.id))
                                                .striped(true)
                                                .num_columns(4)
                                                .max_col_width(220.0)
                                                .show(ui, |ui| {
                                                    ui.strong("Campo");
                                                    ui.strong("No computador principal");
                                                    ui.strong("Sua versão");
                                                    ui.strong("Alterado");
                                                    ui.end_row();
                                                    for d in &differences {
                                                        ui.label(&d.label);
                                                        ui.add(egui::Label::new(if d.host.is_empty() { "(vazio)" } else { &d.host }).wrap());
                                                        ui.add(egui::Label::new(if d.local.is_empty() { "(vazio)" } else { &d.local }).wrap());
                                                        ui.small(d.changed_by());
                                                        ui.end_row();
                                                    }
                                                });
                                        }
                                        ui.add_space(4.0);
                                        let pending_here = self
                                            .pending_resolution
                                            .filter(|(id, _)| *id == conflict.id);
                                        match pending_here {
                                            None => {
                                                ui.horizontal_wrapped(|ui| {
                                                    if ui
                                                        .button("Combinar as duas versões…")
                                                        .on_hover_text("Cada campo fica com o lado que o alterou. Textos alterados dos dois lados são juntados; nos demais campos alterados dos dois lados, sua versão prevalece e o valor do computador principal fica registrado no próprio item.")
                                                        .clicked()
                                                    {
                                                        self.pending_resolution = Some((conflict.id, ConflictResolution::Merge));
                                                    }
                                                    if ui
                                                        .button("Usar só a minha versão…")
                                                        .on_hover_text("Substitui a versão do computador principal pela sua em todos os campos.")
                                                        .clicked()
                                                    {
                                                        self.pending_resolution = Some((conflict.id, ConflictResolution::KeepLocal));
                                                    }
                                                    if ui
                                                        .button("Usar só a do computador principal…")
                                                        .on_hover_text("Descarta a sua alteração feita sem conexão.")
                                                        .clicked()
                                                    {
                                                        self.pending_resolution = Some((conflict.id, ConflictResolution::KeepHost));
                                                    }
                                                });
                                            }
                                            Some((_, resolution)) => {
                                                let description = match resolution {
                                                    ConflictResolution::Merge => {
                                                        if both_sides > 0 {
                                                            format!("Combinar: {} campo(s) alterado(s) só de um lado seguem esse lado; {both_sides} alterado(s) dos dois lados ficam com a sua versão (textos são juntados) e o valor do computador principal é anotado no item.", differences.len() - both_sides)
                                                        } else {
                                                            "Combinar: cada campo fica com o lado que o alterou. Nenhum campo foi alterado dos dois lados.".to_owned()
                                                        }
                                                    }
                                                    ConflictResolution::KeepLocal => "Usar só a minha versão: o que está no computador principal será substituído.".to_owned(),
                                                    ConflictResolution::KeepHost => "Usar só a do computador principal: a sua alteração será descartada.".to_owned(),
                                                };
                                                ui.colored_label(egui::Color32::from_rgb(153, 88, 16), description);
                                                ui.horizontal(|ui| {
                                                    if ui.button("Confirmar").clicked() {
                                                        resolution_action = Some((conflict.id, resolution));
                                                        self.pending_resolution = None;
                                                    }
                                                    if ui.button("Cancelar").clicked() {
                                                        self.pending_resolution = None;
                                                    }
                                                });
                                            }
                                        }
                                    });
                                }
                            });

                        if let Some((conflict_id, res)) = resolution_action {
                            match store.resolve_sync_conflict(conflict_id, res) {
                                Ok(()) => {
                                    self.error = false;
                                    self.message = match res {
                                        ConflictResolution::Merge => {
                                            "Conflito resolvido: as duas versões foram combinadas.".into()
                                        }
                                        ConflictResolution::KeepLocal => {
                                            "Conflito resolvido: a sua versão foi aplicada.".into()
                                        }
                                        ConflictResolution::KeepHost => {
                                            "Conflito resolvido: a versão do computador principal foi mantida.".into()
                                        }
                                    };
                                    self.last_sync_request = None;
                                    let _ = self.refresh_client(store, db_path);
                                    refresh_needed = true;
                                }
                                Err(e) => {
                                    self.error = true;
                                    self.message =
                                        format!("Não foi possível resolver o choque agora: {e}");
                                }
                            }
                        }
                    }
                }

                ui.separator();
                ui.collapsing("Histórico recente de atividade da equipe", |ui| {
                    match store.list_team_events(30) {
                        Ok(events) => {
                            if events.is_empty() {
                                ui.label("Nenhuma atividade registrada ainda.");
                            }
                            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                for ev in events {
                                    ui.small(format!(
                                        "{} • {} • {}",
                                        crate::labels::datetime(&ev.created_at),
                                        ev.actor,
                                        ev.details
                                    ));
                                }
                            });
                        }
                        Err(e) => {
                            ui.label(format!("Falha ao ler eventos: {e}"));
                        }
                    }
                });
            });

        self.open = open;
        refresh_needed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::sample_capture;

    #[test]
    fn host_and_client_sync_mutations_detect_conflicts_and_support_failover() {
        let dir = tempfile::tempdir().unwrap();
        let host_db_path = dir.path().join("host.sqlite3");
        let client_db_path = dir.path().join("client.sqlite3");

        let token = generate_token();
        let mut host_store = Store::open(&host_db_path).unwrap();
        let mut host_cfg = IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "CARLO_ANFITRIAO".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: "http://127.0.0.1:7878".into(),
            cabinet_token: token.clone(),
            auto_start_host: true,
            synced_revision: 0,
            last_sync_at: String::new(),
            ..IntranetConfig::default()
        };
        host_store.save_intranet_config(&host_cfg).unwrap();

        let mut server = HostServer::start(host_db_path.clone(), "127.0.0.1:0").unwrap();
        let bound_url = format!("http://{}", server.local_addr());
        host_cfg.bind_addr = server.local_addr().to_string();
        host_store.save_intranet_config(&host_cfg).unwrap();

        let mut client_store = Store::open(&client_db_path).unwrap();
        let client_cfg = IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "MARIA_CLIENTE".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: bound_url.clone(),
            cabinet_token: token.clone(),
            auto_start_host: false,
            synced_revision: 0,
            last_sync_at: String::new(),
            ..IntranetConfig::default()
        };
        client_store.save_intranet_config(&client_cfg).unwrap();

        // 1. Client imports a document with a PDF bundle through the Host!
        let mut capture = sample_capture();
        let pdf_url = "https://www.sigadoc.mt.gov.br/arquivos/teste.pdf".to_owned();
        capture.references.push(crate::model::DocumentReference {
            label: "PDF Intranet".into(),
            url: pdf_url.clone(),
            kind: "attachment".into(),
        });
        let pdf_input = FileInput {
            url: pdf_url,
            data_base64: STANDARD.encode(include_bytes!("../tests/fixtures/document.pdf")),
        };
        let receipt = client_store
            .import_bundle(&capture, &[pdf_input])
            .unwrap();
        assert!(!receipt.duplicate);

        // Both Host and Client have the document, task, and PDF encrypted in their respective stores!
        assert_eq!(host_store.list_documents("").unwrap().len(), 1);
        assert_eq!(client_store.list_documents("").unwrap().len(), 1);
        let files = client_store.list_files(receipt.capture_id).unwrap();
        assert_eq!(files.len(), 1);
        assert!(client_store
            .pdf_bytes(files[0].id, receipt.capture_id)
            .unwrap()
            .starts_with(b"%PDF-"));

        // 1b. Client attaches a file through the Host; both keep it sealed and
        // the client receives the bytes with the snapshot (hash-checked).
        let owner = crate::attachments::Owner::Document(receipt.document_id);
        let attachment_id = client_store
            .add_attachment(owner, "despacho.txt", "foto do despacho", b"texto do despacho\n")
            .unwrap();
        let host_list = host_store.list_attachments(owner).unwrap();
        assert_eq!(host_list.len(), 1);
        assert_eq!(host_list[0].id, attachment_id);
        assert_eq!(host_list[0].added_by, "MARIA_CLIENTE");
        let client_list = client_store.list_attachments(owner).unwrap();
        assert_eq!(client_list.len(), 1);
        assert_eq!(client_list[0].description, "foto do despacho");
        assert_eq!(
            client_store.attachment_bytes(attachment_id).unwrap().1,
            b"texto do despacho\n"
        );
        client_store
            .update_attachment_description(attachment_id, "despacho assinado")
            .unwrap();
        assert_eq!(
            host_store.list_attachments(owner).unwrap()[0].description,
            "despacho assinado"
        );
        // Trash through the host hides it on both sides; restore brings it back.
        let target = TrashTarget { kind: TrashKind::Attachment, id: attachment_id };
        client_store.trash_item(target.clone()).unwrap();
        assert!(host_store.list_attachments(owner).unwrap().is_empty());
        assert!(client_store.list_attachments(owner).unwrap().is_empty());
        client_store.restore_item(target).unwrap();
        assert_eq!(client_store.list_attachments(owner).unwrap().len(), 1);
        // Limits are enforced by the host with the same guidance.
        let big = vec![b'a'; crate::attachments::MAX_ATTACHMENT_BYTES + 1];
        let refused = client_store
            .add_attachment(owner, "grande.txt", "", &big)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("8 MB"), "{refused}");

        // 2. Optimistic Concurrency Control on Task (while online):
        // Both Host and Client load Task #1 at version 0.
        let mut client_task = client_store.list_tasks().unwrap().remove(0);
        let base_ver = client_store
            .item_version("task", client_task.id)
            .unwrap()
            .version;

        // Host updates the task first (version becomes 1):
        let mut host_task = host_store.list_tasks().unwrap().remove(0);
        host_task.title = "Atualizada pelo Anfitrião".into();
        host_task.assignee = "CARLO_ANFITRIAO".into();
        host_store
            .save_task_checked(&host_task, Some(base_ver))
            .unwrap();

        // Client tries to save using stale `base_ver` while online -> must be rejected with conflict error!
        client_task.title = "Edição concorrente desatualizada".into();
        let conflict = client_store.save_task_checked(&client_task, Some(base_ver));
        assert!(conflict.is_err());
        assert!(conflict
            .unwrap_err()
            .to_string()
            .contains("Conflito de edição simultânea"));

        // Client syncs, sees Host's update, and saves on top of the new version (version 1 -> 2):
        assert!(client_store.sync_from_host(false).unwrap());
        let mut synced_task = client_store.list_tasks().unwrap().remove(0);
        assert_eq!(synced_task.title, "Atualizada pelo Anfitrião");
        assert_eq!(synced_task.assignee, "CARLO_ANFITRIAO");
        let new_ver = client_store
            .item_version("task", synced_task.id)
            .unwrap()
            .version;
        assert_eq!(new_ver, 1);

        synced_task.notes = "Complementado por Maria após revisão".into();
        synced_task.assignee = "MARIA_CLIENTE".into();
        client_store
            .save_task_checked(&synced_task, Some(new_ver))
            .unwrap();
        assert_eq!(
            host_store.list_tasks().unwrap()[0].notes,
            "Complementado por Maria após revisão"
        );

        // 3. Offline work while Host is stopped + Mutual Sync + Conflict Detection & Merge!
        server.stop();
        assert!(client_store.sync_from_host(false).is_err());

        // Client continues working normally while Host is offline:
        // (a) Client edits the document note twice while offline (coalesced in outbox keeping initial base version 0):
        client_store
            .save_note(receipt.document_id, "Primeira anotação offline de Maria")
            .unwrap();
        client_store
            .save_note(receipt.document_id, "Parecer técnico elaborado por Maria offline")
            .unwrap();
        assert_eq!(
            client_store
                .load_document(receipt.document_id)
                .unwrap()
                .note,
            "Parecer técnico elaborado por Maria offline"
        );
        // (b) Client creates a new task while offline (no conflict):
        let mut offline_new_task = Task::new(receipt.document_id);
        offline_new_task.title = "Diligência criada por Maria sem servidor".into();
        offline_new_task.assignee = "MARIA_CLIENTE".into();
        client_store.save_task(&offline_new_task).unwrap();
        assert_eq!(client_store.list_offline_outbox().unwrap().len(), 2);

        // Meanwhile, on the Host PC (before reconnecting), Carlo also edits the same document's note!
        host_store
            .save_note(receipt.document_id, "Despacho urgente registrado por Carlo no Anfitrião")
            .unwrap();

        // Now Host comes back online!
        let mut server2 = HostServer::start(host_db_path.clone(), "127.0.0.1:0").unwrap();
        let mut reconnected_cfg = client_store.load_intranet_config().unwrap();
        reconnected_cfg.host_url = format!("http://{}", server2.local_addr());
        client_store.save_intranet_config(&reconnected_cfg).unwrap();

        // Client triggers mutual sync (`sync_from_host`):
        // - `offline_new_task` has no conflict and is pushed to Host automatically!
        // - `save_note` collides with Carlo's concurrent edit on Host and is moved to `offline_conflicts`!
        assert!(client_store.sync_from_host(false).unwrap());
        assert_eq!(client_store.list_offline_outbox().unwrap().len(), 0);
        assert_eq!(host_store.list_tasks().unwrap().len(), 2);
        assert!(host_store
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Diligência criada por Maria sem servidor"));

        let conflicts = client_store.list_sync_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].item_label.contains("Notas do documento"));
        assert!(conflicts[0]
            .local_summary
            .contains("Parecer técnico elaborado por Maria offline"));
        assert!(conflicts[0]
            .host_summary
            .contains("Despacho urgente registrado por Carlo no Anfitrião"));

        // Maria chooses to MERGE both versions (`ConflictResolution::Merge`):
        client_store
            .resolve_sync_conflict(conflicts[0].id, ConflictResolution::Merge)
            .unwrap();
        assert_eq!(client_store.list_sync_conflicts().unwrap().len(), 0);

        let merged_host_note = host_store
            .load_document(receipt.document_id)
            .unwrap()
            .note;
        let merged_client_note = client_store
            .load_document(receipt.document_id)
            .unwrap()
            .note;
        assert_eq!(merged_host_note, merged_client_note);
        assert!(merged_host_note.contains("Despacho urgente registrado por Carlo no Anfitrião"));
        assert!(merged_host_note.contains("Parecer técnico elaborado por Maria offline"));
        server2.stop();

        // 4. Contingency failover: Client switches to Host mode and continues seamlessly!
        let mut failover_cfg = client_store.load_intranet_config().unwrap();
        failover_cfg.mode = IntranetMode::Host;
        failover_cfg.bind_addr = "127.0.0.1:0".into();
        client_store.save_intranet_config(&failover_cfg).unwrap();
        client_store
            .save_note(receipt.document_id, "Nota gravada após assumir como Anfitrião")
            .unwrap();
        assert_eq!(
            client_store
                .load_document(receipt.document_id)
                .unwrap()
                .note,
            "Nota gravada após assumir como Anfitrião"
        );

        // 5. Verify no plaintext markers leak in either SQLite database file on disk:
        host_store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL);")
            .unwrap();
        client_store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL);")
            .unwrap();
        for db_file in [&host_db_path, &client_db_path] {
            let raw = std::fs::read(db_file).unwrap();
            for secret in [
                token.as_str(),
                "CARLO_ANFITRIAO",
                "MARIA_CLIENTE",
                "Complementado por Maria",
                "Parecer técnico elaborado por Maria offline",
                "Diligência criada por Maria sem servidor",
                capture.title.as_str(),
            ] {
                assert!(
                    !raw.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "Plaintext leaked on disk: {secret}"
                );
            }
        }
    }

    fn folder_config(member: &str, files: Vec<String>) -> IntranetConfig {
        IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: files,
            folder_key: "senha-de-teste-123".into(),
            ..IntranetConfig::default()
        }
    }

    #[test]
    fn two_computers_exchange_the_library_through_folders_and_reconcile_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let files = vec![drive.display().to_string(), pen.display().to_string()];
        let reporter = crate::ai::Reporter::silent();
        let key = "senha-de-teste-123";

        // Computer A: a document with an attachment, then the first copy.
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", files.clone())).unwrap();
        let cfg_a = a.load_intranet_config().unwrap();
        let receipt = a.import_capture(&crate::model::sample_capture()).unwrap();
        a.add_attachment(
            crate::attachments::Owner::Document(receipt.document_id),
            "oficio.pdf",
            "cópia",
            b"%PDF-1.4 anexo sintetico",
        )
        .unwrap();
        a.save_note(receipt.document_id, "Nota inicial").unwrap();
        assert_eq!(a.list_offline_outbox().unwrap().len(), 3, "every change queued for the folders");
        let report = a.sync_with_folders(&cfg_a, key, false, &reporter).unwrap();
        assert!(report.created);
        assert_eq!(report.written_to.len(), 1, "the pen drive folder does not exist yet");
        assert_eq!(report.unreachable.len(), 1);
        assert!(a.list_offline_outbox().unwrap().is_empty());
        let header = crate::sharedfolder::read_header(&drive).unwrap().unwrap();
        assert!(header.revision > 0);
        let cfg_a = a.load_intranet_config().unwrap();
        assert_eq!(cfg_a.synced_revision, header.revision);
        assert!(!cfg_a.folder_lineage.is_empty());

        // Computer B joins with an empty library: receives everything.
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", files.clone())).unwrap();
        let cfg_b = b.load_intranet_config().unwrap();
        let report = b.sync_with_folders(&cfg_b, key, false, &reporter).unwrap();
        assert!(report.changed);
        assert_eq!(b.list_documents("").unwrap().len(), 1);
        let detail = b.load_document(receipt.document_id).unwrap();
        assert_eq!(detail.note, "Nota inicial");
        let attachments = b
            .list_attachments(crate::attachments::Owner::Document(receipt.document_id))
            .unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(b.attachment_bytes(attachments[0].id).unwrap().1, b"%PDF-1.4 anexo sintetico");
        assert!(b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter).unwrap().written_to.is_empty(), "up to date: nothing rewritten");

        // Wrong password and a copy of another library are refused.
        assert!(b.sync_with_folders(&cfg_b, "outra-senha-999", false, &reporter).is_err());

        // Both edit the same note; B also adds a task. B syncs first.
        b.save_note(receipt.document_id, "Nota inicial\nAcréscimo do trabalho").unwrap();
        let mut task = crate::tasks::Task::new(receipt.document_id);
        task.title = "Tarefa criada no trabalho".into();
        b.save_task(&task).unwrap();
        a.save_note(receipt.document_id, "Nota inicial\nAcréscimo de casa").unwrap();
        let cfg_b = b.load_intranet_config().unwrap();
        let report = b.sync_with_folders(&cfg_b, key, false, &reporter).unwrap();
        assert_eq!(report.pushed, 2);
        let revision_after_b = crate::sharedfolder::read_header(&drive).unwrap().unwrap().revision;
        assert!(revision_after_b > header.revision);

        // A syncs: its note conflicts, the task arrives, the file is rewritten.
        let cfg_a = a.load_intranet_config().unwrap();
        let report = a.sync_with_folders(&cfg_a, key, false, &reporter).unwrap();
        assert_eq!(report.conflicts, 1);
        assert!(a.list_tasks().unwrap().iter().any(|t| t.title == "Tarefa criada no trabalho"));
        assert_eq!(a.load_document(receipt.document_id).unwrap().note, "Nota inicial\nAcréscimo do trabalho");
        let conflicts = a.list_sync_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        a.resolve_sync_conflict(conflicts[0].id, ConflictResolution::Merge).unwrap();
        let merged = a.load_document(receipt.document_id).unwrap().note;
        assert!(merged.contains("Acréscimo do trabalho") && merged.contains("Acréscimo de casa"), "{merged}");
        let cfg_a = a.load_intranet_config().unwrap();
        let report = a.sync_with_folders(&cfg_a, key, false, &reporter).unwrap();
        assert_eq!(report.pushed, 1);
        assert!(report.conflicts == 0);

        // B receives the merged note; a second folder appears and catches up.
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let cfg_b = b.load_intranet_config().unwrap();
        let report = b.sync_with_folders(&cfg_b, key, false, &reporter).unwrap();
        assert_eq!(b.load_document(receipt.document_id).unwrap().note, merged);
        assert!(report.written_to.iter().any(|p| p.contains("Pendrive")), "{:?}", report.written_to);
        let drive_header = crate::sharedfolder::read_header(&drive).unwrap().unwrap();
        let pen_header = crate::sharedfolder::read_header(&pen).unwrap().unwrap();
        assert_eq!(drive_header.revision, pen_header.revision);

        // Diverged copies: A writes only to the pen drive, B only to the drive;
        // the next full sync reconciles both changes without losing either.
        let mut a_pen_only = a.load_intranet_config().unwrap();
        a_pen_only.folder_files = vec![pen.display().to_string()];
        a.save_intranet_config(&a_pen_only).unwrap();
        a.save_note(receipt.document_id, &format!("{merged}\nSó pelo pendrive")).unwrap();
        a.sync_with_folders(&a_pen_only, key, false, &reporter).unwrap();
        let mut b_drive_only = b.load_intranet_config().unwrap();
        b_drive_only.folder_files = vec![drive.display().to_string()];
        b.save_intranet_config(&b_drive_only).unwrap();
        let mut other = crate::tasks::Task::new(receipt.document_id);
        other.title = "Tarefa só pelo Drive".into();
        b.save_task(&other).unwrap();
        b.sync_with_folders(&b_drive_only, key, false, &reporter).unwrap();
        let mut b_both = b.load_intranet_config().unwrap();
        b_both.folder_files = files.clone();
        b.save_intranet_config(&b_both).unwrap();
        let report = b.sync_with_folders(&b_both, key, false, &reporter).unwrap();
        assert_eq!(report.merged_foreign, 1, "the pen drive's note reaches B");
        assert!(b.load_document(receipt.document_id).unwrap().note.ends_with("Só pelo pendrive"));
        assert!(b.list_tasks().unwrap().iter().any(|t| t.title == "Tarefa só pelo Drive"));
        assert_eq!(report.written_to.len(), 2);

        // A joining computer with its own documents must confirm the replacement.
        let mut c = Store::open(&dir.path().join("c.sqlite3")).unwrap();
        let mut own = crate::model::sample_capture();
        own.document_number = "SEFAZ-PRO-2026/00099".into();
        own.source_url = "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00099".into();
        c.import_capture(&own).unwrap();
        c.save_intranet_config(&folder_config("Notebook", files.clone())).unwrap();
        let cfg_c = c.load_intranet_config().unwrap();
        let error = c.sync_with_folders(&cfg_c, key, false, &reporter).unwrap_err().to_string();
        assert!(error.starts_with("[confirmar-substituicao]"), "{error}");
        c.sync_with_folders(&cfg_c, key, true, &reporter).unwrap();
        assert_eq!(c.list_documents("").unwrap().len(), 1);

        // Nothing readable in the transport files.
        for path in [&drive, &pen] {
            let raw = std::fs::read(path).unwrap();
            assert!(!raw.windows(5).any(|w| w == b"Nota "));
            assert!(!raw.windows(8).any(|w| w == b"sintetico"));
        }
    }

    #[test]
    fn three_way_merge_keeps_each_side_s_change_and_records_double_changes() {
        // Scalar: only one side changed → that side wins, nothing recorded.
        assert_eq!(pick(Some(&"a"), &"a", &"b"), ("b", false));
        assert_eq!(pick(Some(&"a"), &"b", &"a"), ("b", false));
        assert_eq!(pick(Some(&"a"), &"b", &"c"), ("c", true));
        assert_eq!(pick(None, &"b", &"c"), ("c", true));
        assert_eq!(pick(None, &"b", &"b"), ("b", false));
        // Text: both changed → both kept.
        assert_eq!(pick_text(Some("x"), "x", "y", "Ana"), "y");
        assert_eq!(pick_text(Some("x"), "y", "x", "Ana"), "y");
        let merged = pick_text(Some("x"), "host", "local", "Ana");
        assert!(merged.contains("host") && merged.contains("local"));

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("merge.sqlite3")).unwrap();
        let receipt = store.import_capture(&sample_capture()).unwrap();
        let base = store.list_tasks().unwrap().remove(0);
        // Host changed the assignee; the offline edit changed the title and
        // the priority; both changed the status.
        let mut host = base.clone();
        host.assignee = "CARLO".into();
        host.status = "em analise".into();
        store.save_task(&host).unwrap();
        let mut local = base.clone();
        local.title = "Título revisto offline".into();
        local.priority = "alta".into();
        local.status = "aguardando".into();
        local.notes = "Observação feita sem conexão".into();
        let merged = store
            .build_merged_mutation(
                "MARIA",
                IntranetMutation::SaveTask {
                    task: local.clone(),
                    expected_version: None,
                },
                Some(&IntranetMutation::SaveTask {
                    task: base.clone(),
                    expected_version: None,
                }),
            )
            .unwrap();
        let IntranetMutation::SaveTask { task, .. } = merged else {
            panic!("variant preserved");
        };
        assert_eq!(task.title, "Título revisto offline");
        assert_eq!(task.priority, "alta");
        assert_eq!(task.assignee, "CARLO");
        assert_eq!(task.status, "aguardando", "local wins when both changed");
        assert!(task.notes.contains("Observação feita sem conexão"));
        assert!(
            task.notes.contains("Em análise"),
            "the host value of a double-changed field is recorded: {}",
            task.notes
        );
        assert_eq!(task.document_id, receipt.document_id);

        // The differences shown to the person name who changed each field.
        let conflict = SyncConflict {
            id: 0,
            created_at: now_iso(),
            actor: "MARIA".into(),
            item_label: "Tarefa".into(),
            local_summary: String::new(),
            host_summary: String::new(),
            reason: String::new(),
            mutation: IntranetMutation::SaveTask {
                task: local,
                expected_version: None,
            },
            base: Some(IntranetMutation::SaveTask {
                task: base,
                expected_version: None,
            }),
        };
        let diffs = store.conflict_differences(&conflict);
        let by_label = |label: &str| {
            diffs
                .iter()
                .find(|d| d.label == label)
                .unwrap_or_else(|| panic!("{label} missing from {diffs:?}"))
        };
        assert_eq!(by_label("Título").changed_by(), "só por você");
        assert_eq!(by_label("Responsável").changed_by(), "só no computador principal");
        assert_eq!(by_label("Situação").changed_by(), "pelos dois lados");
        assert_eq!(by_label("Situação").host, "Em análise");
        assert_eq!(by_label("Situação").local, "Aguardando");
    }

    #[test]
    fn connection_code_round_trips_and_rejects_bad_input() {
        let token = generate_token();
        let code = connection_code("http://192.168.1.10:7878", &token);
        assert!(code.starts_with("ponte://conectar?host="));
        let (host, key) = parse_connection_code(&code).unwrap();
        assert_eq!(host, "http://192.168.1.10:7878");
        assert_eq!(key, token);
        // Pasted inside a message, with surrounding text.
        let pasted = format!("Oi! Use este código:\n{code}\nObrigado.");
        assert_eq!(parse_connection_code(&pasted).unwrap().0, host);
        // Public addresses and short keys are refused like in the form.
        assert!(parse_connection_code(&connection_code("http://8.8.8.8:7878", &token)).is_none());
        assert!(parse_connection_code(&connection_code("http://192.168.1.10:7878", "curta")).is_none());
        assert!(parse_connection_code("ponte://conectar?host=x").is_none());
        assert!(parse_connection_code("qualquer coisa").is_none());
    }

    #[test]
    fn rejects_public_urls_and_unauthorized_tokens() {
        assert!(validate_host_url("http://8.8.8.8:7878").is_err());
        assert!(validate_host_url("https://example.com:7878").is_err());
        assert!(validate_host_url("http://192.168.1.25:7878").is_ok());
        assert!(validate_host_url("http://10.0.0.5:7878").is_ok());
        assert!(validate_host_url("http://172.20.1.1:7878").is_ok());
        assert!(validate_host_url("http://pc-gabinete:7878").is_ok());
        assert!(validate_bind_addr("8.8.8.8:7878").is_err());
        assert!(validate_bind_addr("0.0.0.0:7878").is_ok());
    }
}
