//! Working on more than one computer. Four modes: only this computer, main
//! computer of the team, connected to the main computer (HTTP JSON on the
//! local network) and shared folder (an encrypted transport file, see
//! `sharedfolder`). No SQLite file is ever opened from a network or synced
//! folder: each computer keeps its own DPAPI-sealed library. Changes travel
//! as `IntranetMutation`s; when the other side is out of reach they wait in
//! the offline queue, and edits made on both sides become conflicts the
//! person resolves field by field.

use crate::{
    dossier::LinkPayload,
    evidence::Source,
    files::{FileInput, FileSummary},
    model::{Analysis, Capture, DraftProvenance, ImportReceipt},
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
#[cfg(feature = "desktop")]
use eframe::egui;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(feature = "desktop")]
use std::sync::mpsc;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use url::Url;

pub const DEFAULT_PORT: u16 = 7878;
const MAX_HTTP_BODY: usize = 28 * 1024 * 1024;
/// Request line plus headers of one team request.
const MAX_HTTP_HEADER: u64 = 16 * 1024;
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
            Self::Host => "Este é o computador principal",
            Self::Client => "Conectar ao computador principal",
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

/// Network configuration of this computer. `Debug` is written by hand so
/// the cabinet key and the folder passphrase never reach logs or test output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl std::fmt::Debug for IntranetConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let secret = |value: &str| if value.is_empty() { "" } else { "<oculto>" };
        f.debug_struct("IntranetConfig")
            .field("mode", &self.mode)
            .field("member_name", &self.member_name)
            .field("bind_addr", &self.bind_addr)
            .field("host_url", &self.host_url)
            .field("cabinet_token", &secret(&self.cabinet_token))
            .field("auto_start_host", &self.auto_start_host)
            .field("synced_revision", &self.synced_revision)
            .field("last_sync_at", &self.last_sync_at)
            .field("folder_files", &self.folder_files)
            .field("folder_key", &secret(&self.folder_key))
            .field("remember_folder_key", &self.remember_folder_key)
            .field("folder_writer", &self.folder_writer)
            .field("folder_lineage", &self.folder_lineage)
            .finish()
    }
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
            !member.is_empty() && member.len() <= 120 && !member.chars().any(char::is_control),
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
    for (name, value) in url::form_urlencoded::parse(&query.as_bytes()[..end]) {
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
        "A chave de acesso do gabinete deve ter entre 16 e 256 caracteres, sem espaços."
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
        "O computador principal só pode receber conexões de endereços da rede interna do órgão."
    );
    Ok(addr)
}

pub fn validate_host_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    ensure!(
        !trimmed.is_empty() && trimmed.len() <= 512,
        "Informe o endereço do computador principal."
    );
    let url = Url::parse(trimmed)
        .context("Endereço do computador principal inválido. Exemplo: http://192.168.1.10:7878")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "O endereço do computador principal deve começar com http:// ou https://."
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && (url.path() == "/" || url.path().is_empty()),
        "Informe só o endereço do computador principal (ex.: http://192.168.1.10:7878), sem nada depois do número da porta."
    );
    let host = url
        .host_str()
        .context("O endereço do computador principal não indica o computador.")?;
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        ensure!(
            is_allowed_peer_ip(ip),
            "O endereço do computador principal deve ser da rede interna do órgão."
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
            "Use o número IP do computador na rede interna (ex.: 192.168.1.10) ou o nome dele na rede."
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
    #[serde(default)]
    pub provenance: Option<DraftProvenance>,
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
    /// None for an attachment of an atendimento without a linked document.
    #[serde(default)]
    pub document_id: Option<i64>,
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
    /// `None` for a task without document (and its attachments).
    pub document_id: Option<i64>,
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
    /// Work templates, blocks, examples and generation records.
    #[serde(default)]
    pub templates: crate::work_templates::TemplatesSnapshot,
}

// Captures and file bundles make some variants large. A change is built,
// sent and stored once, so boxing them would only add noise at every call.
#[allow(clippy::large_enum_variant)]
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
        #[serde(default)]
        provenance: Option<DraftProvenance>,
    },
    SaveAnalysis {
        document_id: i64,
        analysis: Analysis,
    },
    SaveTask {
        task: Task,
        expected_version: Option<i64>,
    },
    /// Moves a task to another document, or to none, keeping everything
    /// else about it (id, attachments, checklist, history). Never a side
    /// effect of `SaveTask`, which refuses a changed link.
    RelinkTask {
        task_id: i64,
        document_id: Option<i64>,
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
        items: Vec<(
            crate::saiop::SaiopAttendance,
            crate::saiop::RawSpreadsheetRow,
        )>,
    },
    UndoSaiopBatch {
        batch_id: i64,
    },
    /// Permanent removal of one atendimento (and its attachments); there is
    /// no trash for atendimentos.
    DeleteSaiopAttendance {
        id: i64,
    },
    /// A task created from an atendimento and its record in the
    /// atendimento's timeline, written together (`task` is `None` when the
    /// atendimento has no document: only the timeline changes).
    CreateTaskForSaiop {
        attendance_id: i64,
        task: Option<Task>,
        title: String,
        expected_version: Option<i64>,
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
    /// Permanent removal of an attachment (atendimentos have no trash).
    DeleteAttachment {
        id: i64,
    },
    /// Permanent removal of an item that sat in the trash past its time.
    PurgeTrash {
        kind: String,
        id: i64,
    },
    /// Work templates, instruction blocks and examples are addressed by
    /// uid, so these need no number translation.
    SaveTemplate {
        template: crate::work_templates::WorkTemplate,
        expected_version: Option<i64>,
    },
    SetTemplateStatus {
        uid: String,
        status: crate::work_templates::Status,
        /// What the publisher saw (draft number, block versions); absent
        /// from older computers and for archiving.
        #[serde(default)]
        plan: Option<crate::work_templates::PublishPlan>,
        expected_version: Option<i64>,
    },
    DeleteTemplate {
        uid: String,
    },
    SaveInstructionBlock {
        block: crate::work_templates::InstructionBlock,
        expected_version: Option<i64>,
    },
    SetBlockStatus {
        uid: String,
        status: crate::work_templates::Status,
        expected_version: Option<i64>,
    },
    DeleteBlock {
        uid: String,
    },
    SaveExample {
        example: crate::work_templates::Example,
        expected_version: Option<i64>,
    },
    DeleteExample {
        uid: String,
    },
    /// What one AI generation used (its document and capture numbers are
    /// translated like a draft's).
    RecordGeneration {
        record: crate::work_templates::GenerationRecord,
    },
    /// Text of a file attached to an example: extracted by the computer
    /// holding the library, or confirmed by a person.
    ExtractAttachmentText {
        attachment_id: i64,
    },
    ReviewAttachmentText {
        attachment_id: i64,
        text: String,
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
    /// What the change created here, with this computer's numbers, so that
    /// later changes can be translated to the numbers it gets elsewhere.
    #[serde(default)]
    pub created: Vec<crate::idmap::CreatedId>,
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
    /// The change refers to an item created without connection that never
    /// reached the team's library: it cannot be applied again, only redone.
    #[serde(default)]
    pub unresolved: bool,
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

// A request carries a whole change; it is built and sent once.
#[allow(clippy::large_enum_variant)]
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
    #[serde(default)]
    provenance: Option<DraftProvenance>,
}

/// Why a replacement of the library was rolled back: a change was queued
/// meanwhile (by the window or by the process serving the browser) on the
/// library about to be replaced. The next synchronization redoes it.
/// What a main computer before 0.23.6 answers to a message it cannot read:
/// from an older version, it does not know a newer kind of change. From
/// 0.23.6 on, the answer names the version (`host_cannot_read`), so a
/// message that a current main computer cannot read is a plain refusal.
const HOST_TOO_OLD: &str = "Formato de mensagem incompatível com o anfitrião.";

fn host_cannot_read() -> String {
    format!(
        "O computador principal não entendeu esta mensagem (Ponte {}). Atualize o Ponte nos dois computadores.",
        env!("CARGO_PKG_VERSION")
    )
}

const QUEUE_GREW: &str =
    "Uma alteração foi feita durante a sincronização; ela será concluída na próxima.";

/// Inside the transaction that replaces the library: nothing may be queued
/// besides the changes that were sent (`sent`), or its numbers would refer
/// to a library that no longer exists.
fn ensure_queue_settled(conn: &Connection, sent: &[(i64, String)]) -> Result<()> {
    for queued in queued_identities(conn)? {
        ensure!(sent.contains(&queued), "{QUEUE_GREW}");
    }
    Ok(())
}

/// Number and uid of every queued change. A change merged into a queued row
/// (the same item edited again) keeps the number but gets a new uid, so the
/// pair tells a row that was sent from one rewritten while it was sent.
fn queued_identities(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let rows = conn
        .prepare("SELECT id,created_at,payload FROM offline_outbox")?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, created_at, blob)| {
            let entry: OutboxEntry = serde_json::from_slice(&unseal(&blob)?)?;
            Ok((id, outbox_uid(&entry.uid, &created_at, id)))
        })
        .collect()
}

/// The uid of a queued change; rows queued by older versions get the same
/// stand-in as `list_offline_outbox`.
fn outbox_uid(uid: &str, created_at: &str, id: i64) -> String {
    if uid.is_empty() {
        format!("legacy-{created_at}-{id}")
    } else {
        uid.to_owned()
    }
}

/// Removes a queued change that was sent, unless it was rewritten meanwhile
/// (then it holds a newer edit and goes with the next exchange).
fn delete_sent_outbox(conn: &Connection, id: i64, uid: &str) -> Result<()> {
    delete_sent_outboxes(conn, &[(id, uid.to_owned())])
}

/// The same for several changes, reading the queue once.
fn delete_sent_outboxes(conn: &Connection, sent: &[(i64, String)]) -> Result<()> {
    let queued = queued_identities(conn)?;
    for (id, uid) in sent {
        if queued.iter().any(|(q, q_uid)| q == id && q_uid == uid) {
            conn.execute("DELETE FROM offline_outbox WHERE id=?1", params![id])?;
        }
    }
    Ok(())
}

/// Stores the network settings (sealed), also inside a caller's transaction.
fn write_intranet_config(conn: &Connection, config: &IntranetConfig) -> Result<()> {
    config.validate()?;
    let payload = seal(&serde_json::to_vec(&config.for_storage())?)?;
    conn.execute(
        "INSERT INTO intranet_config(id,payload) VALUES(1,?1)
         ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
        [payload],
    )?;
    Ok(())
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Records one change to the library. Must run inside the write
/// transaction of every change: checks `expected_version` (fails with the
/// "conflito de edição" message when another computer saved first), bumps
/// the item and library versions and logs a team event (the newest 500 are
/// kept). Each argument is a separate field of the team log, so they stay
/// separate parameters.
#[allow(clippy::too_many_arguments)]
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
        crate::idmap::ensure_schema(&tx)?;
        if tx.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? < 9 {
            tx.pragma_update(None, "user_version", 9)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Network configuration of this computer, memoized until the library
    /// changes (the status bar reads it on every frame).
    pub fn load_intranet_config(&self) -> Result<IntranetConfig> {
        self.cached("intranet_config", || self.load_intranet_config_uncached())
    }
    fn load_intranet_config_uncached(&self) -> Result<IntranetConfig> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row("SELECT payload FROM intranet_config WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        match blob {
            Some(bytes) => Ok(serde_json::from_slice(&unseal(&bytes)?)?),
            None => Ok(IntranetConfig::default()),
        }
    }

    pub fn save_intranet_config(&mut self, config: &IntranetConfig) -> Result<()> {
        write_intranet_config(&self.conn, config)?;
        // Out of the shared-folder mode, this computer's copy of the library
        // (passphrase-protected) has no use and is not left behind.
        if config.mode != IntranetMode::SharedFolder {
            if let Some(path) = self.guard_copy_path() {
                let _ = std::fs::remove_file(path);
            }
        }
        // With nothing waiting to be sent, this library matches the team's
        // (or starts a shared copy): its numbers become the reference.
        crate::idmap::reset_frame_if_settled(&self.conn)?;
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
        let mut stmt = self
            .conn
            .prepare("SELECT id,created_at,payload FROM team_events ORDER BY id DESC LIMIT ?1")?;
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
            .prepare(
                "SELECT id,identity_hash,latest_capture_id,last_seen_at FROM documents ORDER BY id",
            )?
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
            .prepare(
                "SELECT id,document_id,content_hash,payload,created_at FROM captures ORDER BY id",
            )?
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
                provenance: payload.provenance,
            });
        }

        let raw_analyses = self
            .conn
            .prepare(
                "SELECT id,document_id,capture_id,payload,created_at FROM analyses ORDER BY id",
            )?
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
                    r.get::<_, Option<i64>>(1)?,
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
            .prepare(
                "SELECT kind,item_id,version,payload FROM item_versions ORDER BY kind,item_id",
            )?
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
            .prepare(
                "SELECT id,owner_kind,owner_id,document_id,metadata FROM attachments ORDER BY id",
            )?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
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
            templates: self.export_templates()?,
        })
    }

    /// Replaces this library with a snapshot of the main computer (or of a
    /// shared folder). PDFs and attachments missing here are fetched through
    /// the two closures and checked by hash; those already present are kept.
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
        self.apply_snapshot_and(snapshot, fetch_pdf, fetch_attachment, |_| Ok(()))
    }

    /// [`Store::apply_snapshot`] plus `finish`, run in the same transaction
    /// after the copy is in place: whatever must change together with the
    /// library (the queue that was sent, its conflicts) does.
    pub(crate) fn apply_snapshot_and<F, G, H>(
        &mut self,
        snapshot: &CabinetSnapshot,
        fetch_pdf: F,
        fetch_attachment: G,
        finish: H,
    ) -> Result<()>
    where
        F: Fn(i64, i64) -> Result<Vec<u8>>,
        G: Fn(i64) -> Result<Vec<u8>>,
        H: FnOnce(&rusqlite::Transaction) -> Result<()>,
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
                if let Ok(meta) =
                    serde_json::from_slice::<crate::attachments::AttachmentMeta>(&plain)
                {
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
                "O anexo recebido do computador principal está danificado ou é grande demais."
            );
            ensure!(
                format!("{:x}", Sha256::digest(&bytes)) == row.meta.sha256,
                "O anexo recebido do computador principal está danificado. Sincronize de novo."
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
                "O PDF recebido do computador principal está danificado ou é grande demais."
            );
            let actual_sha = format!("{:x}", Sha256::digest(&bytes));
            ensure!(
                actual_sha == meta.summary.sha256,
                "O PDF recebido do computador principal está danificado. Sincronize de novo."
            );
            prepared_file_blobs.insert(meta.id, seal(&bytes)?);
        }

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Clear `trash` first so active-record triggers never block re-inserting archived items.
        tx.execute_batch(
            "DELETE FROM attachment_texts;
             DELETE FROM draft_generations;
             DELETE FROM template_examples;
             DELETE FROM work_template_versions;
             DELETE FROM work_templates;
             DELETE FROM instruction_block_versions;
             DELETE FROM instruction_blocks;
             DELETE FROM trash;
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
                provenance: draft.provenance.clone(),
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
                params![task.id, task.document_id, seal(&serde_json::to_vec(task)?)?],
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
                .context("O PDF não veio completo na sincronização. Sincronize de novo.")?;
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
                    crate::saiop::reference_key(&row.attendance.reference_norm),
                    seal(&serde_json::to_vec(&row.attendance)?)?,
                    raw_blob,
                    row.updated_at
                ],
            )?;
        }
        for row in &snapshot.attachments {
            let payload_blob = prepared_attachment_blobs
                .get(&row.id)
                .context("O anexo não veio completo na sincronização. Sincronize de novo.")?;
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
        crate::work_templates::apply_templates(&tx, &snapshot.templates)?;
        for t in &snapshot.trash {
            tx.execute(
                "INSERT INTO trash(kind,item_id,document_id,deleted_at) VALUES(?1,?2,?3,?4)",
                params![t.kind, t.item_id, t.document_id, t.deleted_at],
            )?;
        }

        if let Some(blob) = tx
            .query_row("SELECT payload FROM intranet_config WHERE id=1", [], |r| {
                r.get::<_, Vec<u8>>(0)
            })
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
        // This library now numbers its items as the copy it received.
        crate::idmap::reset_frame(&tx, true)?;
        finish(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Applies one change here, as `actor`, without forwarding it anywhere.
    /// Used by the main computer for its clients and when replaying the
    /// offline queue or a shared folder.
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
                provenance,
            } => self
                .save_draft_full(
                    document_id,
                    capture_id,
                    &kind,
                    &body,
                    &materials,
                    provenance.as_ref(),
                )
                .map(MutationReply::Id),
            IntranetMutation::SaveTemplate {
                template,
                expected_version,
            } => self
                .save_template(&template, expected_version)
                .map(|_| MutationReply::Unit),
            IntranetMutation::SetTemplateStatus {
                uid,
                status,
                plan,
                expected_version,
            } => self
                .set_template_status_with(&uid, status, plan, expected_version)
                .map(|_| MutationReply::Unit),
            IntranetMutation::DeleteTemplate { uid } => {
                self.delete_template(&uid).map(|()| MutationReply::Unit)
            }
            IntranetMutation::SaveInstructionBlock {
                block,
                expected_version,
            } => self
                .save_block(&block, expected_version)
                .map(|_| MutationReply::Unit),
            IntranetMutation::SetBlockStatus {
                uid,
                status,
                expected_version,
            } => self
                .set_block_status(&uid, status, expected_version)
                .map(|_| MutationReply::Unit),
            IntranetMutation::DeleteBlock { uid } => {
                self.delete_block(&uid).map(|()| MutationReply::Unit)
            }
            IntranetMutation::SaveExample {
                example,
                expected_version,
            } => self
                .save_example(&example, expected_version)
                .map(|e| MutationReply::Id(e.id)),
            IntranetMutation::DeleteExample { uid } => {
                self.delete_example(&uid).map(|()| MutationReply::Unit)
            }
            IntranetMutation::RecordGeneration { record } => self
                .record_generation(&record)
                .map(|()| MutationReply::Unit),
            IntranetMutation::ExtractAttachmentText { attachment_id } => self
                .extract_attachment_text(attachment_id)
                .map(|_| MutationReply::Unit),
            IntranetMutation::ReviewAttachmentText {
                attachment_id,
                text,
            } => self
                .review_attachment_text(attachment_id, &text)
                .map(|_| MutationReply::Unit),
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
            IntranetMutation::RelinkTask {
                task_id,
                document_id,
                expected_version,
            } => self
                .relink_task(task_id, document_id, expected_version)
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
                    .map(|document| MutationReply::Id(document.unwrap_or(0)))
            }
            IntranetMutation::RestoreItem { kind, id } => {
                let parsed = TrashKind::parse(&kind)?;
                self.restore_item(TrashTarget { kind: parsed, id })
                    .map(|document| MutationReply::Id(document.unwrap_or(0)))
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
                    .context("O anexo recebido está danificado.")?;
                self.add_attachment(owner, &name, &description, &data)
                    .map(MutationReply::Id)
            }
            IntranetMutation::UpdateAttachment { id, description } => self
                .update_attachment_description(id, &description)
                .map(|()| MutationReply::Unit),
            IntranetMutation::DeleteAttachment { id } => {
                self.delete_attachment(id).map(|()| MutationReply::Unit)
            }
            IntranetMutation::PurgeTrash { kind, id } => {
                let parsed = TrashKind::parse(&kind)?;
                self.purge_trash_item(TrashTarget { kind: parsed, id })
                    .map(|document| MutationReply::Id(document.unwrap_or(0)))
            }
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
            IntranetMutation::DeleteSaiopAttendance { id } => self
                .delete_saiop_attendance(id)
                .map(|()| MutationReply::Unit),
            IntranetMutation::CreateTaskForSaiop {
                attendance_id,
                task,
                title,
                expected_version,
            } => self
                .write_task_for_saiop(attendance_id, task.as_ref(), &title, expected_version)
                .map(|id| MutationReply::Id(id.unwrap_or(0))),
        };
        self.suppress_remote = prev_suppress;
        self.actor_override = prev_actor;
        result
    }

    pub fn sync_from_host(&mut self, force_full: bool) -> Result<bool> {
        let cfg = self.load_intranet_config()?;
        ensure!(
            cfg.mode == IntranetMode::Client,
            "Este computador não está conectado ao computador principal."
        );
        cfg.validate()?;

        // 1. Se houver alterações feitas offline nesta estação cliente, sincroniza mutuamente com o anfitrião:
        let outbox = self.list_offline_outbox()?;
        let mut pushed_offline = false;
        if !outbox.is_empty() {
            let host_snapshot = match send_rpc_classified(
                &cfg,
                &RpcRequest::Sync {
                    since_revision: None,
                },
            ) {
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

            // Numbers of items created here are translated into the ones the
            // main computer gives them, change by change. The translation is
            // stored, so a connection that drops halfway loses nothing.
            let mut ids = crate::idmap::load(&self.conn)?;
            let mut host_frame = host_snapshot;
            for entry in outbox {
                let sent = match ids.remap(
                    &entry.mutation,
                    &*self,
                    host_frame.as_deref().map(|s| s as &dyn crate::idmap::Frame),
                ) {
                    Ok(mutation) => OutboxEntry {
                        mutation,
                        base: entry.base.as_ref().map(|base| {
                            ids.remap(base, &*self, None)
                                .unwrap_or_else(|_| base.clone())
                        }),
                        ..entry.clone()
                    },
                    Err(reason) => {
                        let learned = ids.learn(&entry.created, None);
                        let mut conflict =
                            build_sync_conflict(&entry, &reason, host_frame.as_deref());
                        conflict.unresolved = true;
                        self.finish_outbox_entry(entry.id, &entry.uid, &learned, Some(&conflict))?;
                        pushed_offline = true;
                        continue;
                    }
                };
                match send_rpc_classified(
                    &cfg,
                    &RpcRequest::Mutate {
                        actor: sent.actor.clone(),
                        mutation: sent.mutation.clone(),
                    },
                ) {
                    Ok(RpcResponse::Mutated {
                        reply, snapshot, ..
                    }) => {
                        let there = crate::idmap::created_ids(&*snapshot, &sent.mutation, &reply)?;
                        let learned = ids.learn(&entry.created, Some(&there));
                        self.finish_outbox_entry(entry.id, &entry.uid, &learned, None)?;
                        host_frame = Some(snapshot);
                        pushed_offline = true;
                    }
                    Ok(RpcResponse::Error { message }) | Err(RpcError::Rejected(message)) => {
                        let learned = ids.learn(&entry.created, None);
                        let conflict = build_sync_conflict(&sent, &message, host_frame.as_deref());
                        self.finish_outbox_entry(entry.id, &entry.uid, &learned, Some(&conflict))?;
                        pushed_offline = true;
                    }
                    Ok(_) => {
                        bail!(
                            "Resposta inesperada do computador principal ao enviar as alterações."
                        );
                    }
                    Err(RpcError::Unreachable(msg)) => {
                        bail!("{msg}");
                    }
                }
            }
        }

        // A pending translation means this library still uses its own
        // numbers: only a full copy of the main one settles it.
        let since = if force_full || pushed_offline || crate::idmap::has_pending(&self.conn)? {
            None
        } else {
            Some(cfg.synced_revision)
        };
        let response = match send_rpc_classified(
            &cfg,
            &RpcRequest::Sync {
                since_revision: since,
            },
        ) {
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
                self.apply_snapshot_and(
                    &snapshot,
                    |file_id, capture_id| fetch_remote_pdf(&cfg_clone, file_id, capture_id),
                    |id| fetch_remote_attachment(&cfg_clone, id),
                    |tx| ensure_queue_settled(tx, &[]),
                )?;
                Ok(true)
            }
            RpcResponse::Error { message } => bail!("{message}"),
            _ => bail!("Resposta inesperada do computador principal durante a sincronização."),
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
        ensure!(
            !key.is_empty(),
            "Informe a senha de sincronização em Ajustes ▸ Equipe e sincronização."
        );
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
        // This computer's own last written copy, kept beside the library:
        // if another member overwrites the folder copy from an old base
        // (a phone whose Drive app served a cached file, two computers
        // writing at once), what this computer wrote comes back from here.
        let guard_path = self.guard_copy_path();
        let guard_header = guard_path
            .as_ref()
            .and_then(|path| sharedfolder::read_header(path).ok().flatten());
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
                "A pasta {} contém outro acervo, não o deste computador. Retire-a da lista ou escolha outro arquivo.",
                copy.path.display()
            );
            ensure!(
                copy.header.schema_version <= crate::storage::SCHEMA_VERSION,
                "A cópia em {} foi gravada por um Ponte mais novo; atualize o Ponte neste computador.",
                copy.path.display()
            );
        }
        // The guard copy counts only as this computer's last write: same
        // library, written by this computer, not older than the last
        // exchange (a guard whose write failed is stale and would roll
        // changes back). Anything else is removed and written again.
        let guard_trusted = guard_header.as_ref().is_some_and(|h| {
            h.lineage == lineage
                && h.writer == cfg.folder_writer
                && h.revision >= cfg.synced_revision
                && h.schema_version <= crate::storage::SCHEMA_VERSION
        });
        if guard_header.is_some() && !guard_trusted {
            if let Some(path) = &guard_path {
                let _ = std::fs::remove_file(path);
            }
        }
        copies.sort_by(|a, b| {
            (b.header.revision, &b.header.written_at)
                .cmp(&(a.header.revision, &a.header.written_at))
        });
        let outbox = self.list_offline_outbox()?;
        // Its body is read only when someone wrote after it (the newest
        // folder copy is not this very copy): the usual exchange with
        // nothing new pays for the header alone.
        let guard: Option<sharedfolder::Copy> = match (&guard_path, &guard_header) {
            (Some(path), Some(header)) if guard_trusted => {
                let same = copies.first().is_some_and(|best| {
                    best.header.revision == header.revision
                        && best.header.writer == header.writer
                        && best.header.written_at == header.written_at
                });
                if same || copies.is_empty() {
                    None
                } else {
                    sharedfolder::read(path, key).ok().flatten()
                }
            }
            _ => None,
        };
        let first_write = copies.is_empty();
        let mut joining = false;
        if let Some(best) = copies.first() {
            // Older than what this computer last wrote: either the cloud has
            // not delivered that write yet (the folder copy is part of what
            // this computer wrote: wait), or someone wrote over it from an
            // old base (it carries changes this computer never saw: merge,
            // the guard copy brings back what was lost).
            let overwritten = guard.as_ref().is_some_and(|g| {
                best.payload
                    .applied
                    .iter()
                    .any(|uid| !g.payload.applied.contains(uid))
            });
            ensure!(
                best.header.revision >= cfg.synced_revision || overwritten,
                "A pasta ainda não recebeu a versão mais recente (revisão {} na pasta, {} neste computador). Espere a nuvem terminar de sincronizar e tente de novo.",
                best.header.revision,
                cfg.synced_revision
            );
            // Joining: this computer never exchanged with this lineage. A copy
            // this very computer wrote proves otherwise, even when the local
            // bookkeeping was lost (a window of 0.18.0 overwrote it).
            let wrote_a_copy = copies.iter().any(|c| c.header.writer == cfg.folder_writer);
            joining =
                cfg.synced_revision == 0 && cfg.folder_lineage.trim().is_empty() && !wrote_a_copy;
            if joining && !allow_replace {
                let local_documents = self.list_documents("")?.len();
                ensure!(
                    local_documents == 0,
                    "[confirmar-substituicao] Este computador ainda não sincronizou com esta pasta e tem {} no acervo. Ao sincronizar pela primeira vez, o acervo daqui será substituído pelo da pasta ({}). Faça uma cópia de segurança antes, se quiser guardá-los.",
                    plural(local_documents, "documento", "documentos"),
                    sharedfolder::describe(&best.header)
                );
            }
        }
        // The guard copy joins the copies read (never as a folder, never in
        // place of an empty folder): its changes missing from the newest
        // copy are merged like any other.
        let guard_missing = guard_path.is_some() && !guard_trusted;
        if let Some(copy) = guard {
            copies.push(copy);
            copies.sort_by(|a, b| {
                (b.header.revision, &b.header.written_at)
                    .cmp(&(a.header.revision, &a.header.written_at))
            });
        }
        // Changes other copies carry that the newest one never received.
        let best_applied: std::collections::BTreeSet<String> = copies
            .first()
            .map(|c| c.payload.applied.iter().cloned().collect())
            .unwrap_or_default();
        let mut foreign: Vec<(JournalEntry, usize)> = Vec::new();
        for (index, copy) in copies.iter().enumerate().skip(1) {
            let missing_from_journal = copy
                .payload
                .applied
                .iter()
                .filter(|uid| {
                    !best_applied.contains(*uid)
                        && !copy.payload.journal.iter().any(|e| &e.uid == *uid)
                })
                .count();
            if missing_from_journal > 0 && guard_path.as_deref() != Some(copy.path.as_path()) {
                report.notes.push(format!(
                    "{}: {} antigas não puderam ser recuperadas (o histórico dessa cópia já foi resumido).",
                    copy.path.display(),
                    plural(missing_from_journal, "alteração", "alterações")
                ));
            }
            for entry in &copy.payload.journal {
                if !best_applied.contains(&entry.uid)
                    && !foreign.iter().any(|(e, _)| e.uid == entry.uid)
                {
                    foreign.push((entry.clone(), index));
                }
            }
        }
        foreign
            .sort_by(|a, b| (a.0.revision, &a.0.created_at).cmp(&(b.0.revision, &b.0.created_at)));
        let lagging = copies
            .iter()
            .skip(1)
            .any(|c| copies[0].header.revision > c.header.revision)
            || !absent.is_empty()
            || guard_missing;
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
            joining,
            guard_path.as_deref(),
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
        foreign: Vec<(crate::sharedfolder::JournalEntry, usize)>,
        outbox: Vec<OutboxEntry>,
        first_write: bool,
        joining: bool,
        guard_path: Option<&std::path::Path>,
        temp_path: &std::path::Path,
        reporter: &crate::ai::Reporter,
        mut report: FolderSyncReport,
    ) -> Result<FolderSyncReport> {
        use crate::sharedfolder::{self, JournalEntry, Payload, WriteRequest};
        let mut applied: std::collections::BTreeSet<String> = Default::default();
        let mut journal: Vec<JournalEntry> = Vec::new();
        let mut processed_outbox: Vec<(i64, String)> = Vec::new();
        // Recorded only once the exchange succeeds: until then they refer to
        // a copy that may never be written.
        let mut conflicts: Vec<SyncConflict> = Vec::new();
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
                    created: entry.created.clone(),
                });
                processed_outbox.push((entry.id, entry.uid.clone()));
            }
            report.created = true;
        } else {
            let best = &copies[0];
            reporter.step(format!(
                "Carregando a cópia mais recente ({})…",
                sharedfolder::describe(&best.header)
            ));
            let mut store = Store::open(temp_path)
                .context("Não foi possível preparar o acervo temporário da sincronização.")?;
            store.apply_snapshot(
                &best.payload.snapshot,
                |file_id, _| {
                    best.blobs
                        .pdfs
                        .get(&file_id)
                        .cloned()
                        .context("PDF ausente no arquivo de sincronização.")
                },
                |id| {
                    best.blobs
                        .attachments
                        .get(&id)
                        .cloned()
                        .context("Anexo ausente no arquivo de sincronização.")
                },
            )?;
            applied = best.payload.applied.iter().cloned().collect();
            journal = best.payload.journal.clone();
            // What each change created, with the numbers of the copy being
            // built (`None` when it was refused here), by change uid.
            let mut resolved: BTreeMap<String, Option<Vec<crate::idmap::CreatedId>>> = journal
                .iter()
                .map(|e| (e.uid.clone(), Some(e.created.clone())))
                .collect();
            if !foreign.is_empty() {
                reporter.step(format!(
                    "Reunindo {} vindas de outras pastas…",
                    plural(foreign.len(), "alteração", "alterações")
                ));
            }
            for (entry, index) in foreign {
                if applied.contains(&entry.uid) {
                    continue;
                }
                let source = &copies[index];
                let from = if guard_path == Some(source.path.as_path()) {
                    "deste computador".to_owned()
                } else {
                    format!("da pasta {}", source.path.display())
                };
                // That copy may have numbered new items differently.
                let ids = crate::idmap::IdMap::from_journal(
                    source
                        .payload
                        .journal
                        .iter()
                        .map(|e| (e.uid.as_str(), e.created.as_slice())),
                    &resolved,
                    &applied,
                );
                let remapped = ids.remap(&entry.mutation, &source.payload.snapshot, Some(&store));
                let outcome = match &remapped {
                    Ok(mutation) => store
                        .execute_mutation(&entry.actor, mutation.clone())
                        .map_err(|error| error.to_string()),
                    Err(reason) => Err(reason.clone()),
                };
                match (remapped, outcome) {
                    (Ok(mutation), Ok(reply)) => {
                        let created = crate::idmap::created_ids(&store, &mutation, &reply)?;
                        report.merged_foreign += 1;
                        let revision = store.cabinet_revision()?;
                        applied.insert(entry.uid.clone());
                        resolved.insert(entry.uid.clone(), Some(created.clone()));
                        let base = entry.base.as_ref().map(|base| {
                            ids.remap(base, &source.payload.snapshot, None)
                                .unwrap_or_else(|_| base.clone())
                        });
                        journal.push(JournalEntry {
                            revision,
                            mutation,
                            base,
                            created,
                            ..entry
                        });
                    }
                    (remapped, outcome) => {
                        let unresolved = remapped.is_err();
                        let error = outcome.err().unwrap_or_default();
                        resolved.insert(entry.uid.clone(), None);
                        let as_entry = OutboxEntry {
                            id: 0,
                            created_at: entry.created_at.clone(),
                            actor: entry.actor.clone(),
                            label: format!("[{}] {}", from, entry.label),
                            mutation: remapped.unwrap_or_else(|_| entry.mutation.clone()),
                            base: entry.base.as_ref().map(|base| {
                                ids.remap(base, &source.payload.snapshot, None)
                                    .unwrap_or_else(|_| base.clone())
                            }),
                            uid: entry.uid.clone(),
                            created: Vec::new(),
                        };
                        conflicts.push(conflict_for(
                            &as_entry,
                            &format!("{error} (alteração vinda de {from})"),
                            base_snapshot,
                            unresolved,
                        ));
                        applied.insert(entry.uid.clone());
                        report.conflicts += 1;
                    }
                }
            }
            if !outbox.is_empty() {
                reporter.step(format!(
                    "Aplicando {} deste computador…",
                    plural(outbox.len(), "alteração", "alterações")
                ));
            }
            // This computer's numbers, translated into the copy's. This
            // library matches the copy it last exchanged with; when the copy
            // being built diverged from that one, the items created there are
            // found through the journal kept from it.
            let mut ids = crate::idmap::load(&self.conn)?;
            let frame = crate::idmap::load_frame_journal(&self.conn)?;
            ids.absorb(crate::idmap::IdMap::from_journal(
                frame
                    .iter()
                    .map(|(uid, created)| (uid.as_str(), created.as_slice())),
                &resolved,
                &applied,
            ));
            for entry in &outbox {
                processed_outbox.push((entry.id, entry.uid.clone()));
                if applied.contains(&entry.uid) {
                    // Already in the copy (an exchange that could not finish
                    // here): what it created there is in the journal.
                    // Without it in the journal (compacted), it is shared
                    // history: same numbers.
                    if let Some(Some(there)) = resolved.get(&entry.uid) {
                        ids.learn(&entry.created, Some(there));
                    }
                    continue;
                }
                if joining {
                    // Made on a library that the folder's one has replaced:
                    // its numbers mean nothing there.
                    ids.learn(&entry.created, None);
                    conflicts.push(conflict_for(
                        entry,
                        "Alteração feita antes de este computador entrar na pasta; o acervo daqui foi substituído pelo da pasta.",
                        base_snapshot,
                        true,
                    ));
                    applied.insert(entry.uid.clone());
                    report.conflicts += 1;
                    continue;
                }
                let remapped = ids.remap(&entry.mutation, &*self, Some(&store));
                let outcome = match &remapped {
                    Ok(mutation) => store
                        .execute_mutation(&entry.actor, mutation.clone())
                        .map_err(|error| error.to_string()),
                    Err(reason) => Err(reason.clone()),
                };
                match (remapped, outcome) {
                    (Ok(mutation), Ok(reply)) => {
                        let created = crate::idmap::created_ids(&store, &mutation, &reply)?;
                        ids.learn(&entry.created, Some(&created));
                        report.pushed += 1;
                        let revision = store.cabinet_revision()?;
                        applied.insert(entry.uid.clone());
                        let base = entry.base.as_ref().map(|base| {
                            ids.remap(base, &*self, None)
                                .unwrap_or_else(|_| base.clone())
                        });
                        journal.push(JournalEntry {
                            uid: entry.uid.clone(),
                            created_at: entry.created_at.clone(),
                            actor: entry.actor.clone(),
                            writer: cfg.folder_writer.clone(),
                            label: entry.label.clone(),
                            mutation,
                            base,
                            revision,
                            created,
                        });
                    }
                    (remapped, outcome) => {
                        ids.learn(&entry.created, None);
                        let unresolved = remapped.is_err();
                        let error = outcome.err().unwrap_or_default();
                        let sent = OutboxEntry {
                            mutation: remapped.unwrap_or_else(|_| entry.mutation.clone()),
                            base: entry.base.as_ref().map(|base| {
                                ids.remap(base, &*self, None)
                                    .unwrap_or_else(|_| base.clone())
                            }),
                            ..entry.clone()
                        };
                        conflicts.push(conflict_for(&sent, &error, base_snapshot, unresolved));
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
        let changed_content =
            first_write || report.pushed > 0 || report.merged_foreign > 0 || report.conflicts > 0;
        let mut targets: Vec<PathBuf> = absent.to_vec();
        for copy in copies {
            if guard_path == Some(copy.path.as_path()) {
                continue;
            }
            // A copy already at this exact state is left alone.
            if changed_content || copy.header.revision < revision {
                targets.push(copy.path.clone());
            }
        }
        if !targets.is_empty() {
            reporter.step(format!(
                "Gravando em {}…",
                plural(targets.len(), "pasta", "pastas")
            ));
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
        // Only now, with the folders written, does this become this
        // computer's last write.
        if let Some(path) = guard_path {
            if let Err(error) = sharedfolder::write(path, key, &request, &payload, &blobs) {
                report.notes.push(format!(
                    "A cópia de segurança deste computador não foi gravada: {error}"
                ));
            }
        }

        // The library, the queue that was sent, its conflicts, the settings
        // and the journal of the copy now matched change together: an
        // interruption leaves either the old state (and the next exchange
        // finds its changes already in the copy) or the new one.
        let mut updated = cfg.clone();
        updated.synced_revision = revision;
        updated.last_sync_at = now_iso();
        updated.folder_lineage = lineage.to_owned();
        let replaced = temp.is_some();
        let finish = |tx: &rusqlite::Transaction| -> Result<()> {
            for conflict in &conflicts {
                tx.execute(
                    "INSERT INTO offline_conflicts(created_at,payload) VALUES(?1,?2)",
                    params![conflict.created_at, seal(&serde_json::to_vec(conflict)?)?],
                )?;
            }
            ensure_queue_settled(tx, &processed_outbox)?;
            delete_sent_outboxes(tx, &processed_outbox)?;
            write_intranet_config(tx, &updated)?;
            if !replaced {
                // First copy, made from this library: its numbers are the
                // reference from now on.
                crate::idmap::reset_frame(tx, true)?;
            }
            crate::idmap::save_frame_journal(tx, &payload.journal)
        };
        if let Some(store) = &temp {
            reporter.step("Atualizando o acervo deste computador…");
            let before = self.version_map()?;
            let applied_here = self.apply_snapshot_and(
                &final_snapshot,
                |file_id, capture_id| store.pdf_bytes(file_id, capture_id),
                |id| store.attachment_bytes(id).map(|(_, bytes)| bytes),
                finish,
            );
            if let Err(error) = applied_here {
                self.keep_conflicts(&conflicts)?;
                return Err(error);
            }
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
        } else {
            let tx = self.conn.transaction()?;
            if let Err(error) = finish(&tx) {
                drop(tx);
                self.keep_conflicts(&conflicts)?;
                return Err(error);
            }
            tx.commit()?;
        }
        report.revision = revision;
        Ok(report)
    }

    /// Where this library keeps its own last written transport copy: beside
    /// the library file, so every library (and every test) has its own.
    /// `None` for a library without a file (in memory).
    fn guard_copy_path(&self) -> Option<PathBuf> {
        let db = self.conn.path().filter(|p| !p.is_empty())?;
        Some(PathBuf::from(format!("{db}-guarda.ponte")))
    }

    /// Conflicts of an exchange whose last step was undone (a change made
    /// meanwhile): the copy in the folders already counts the conflicting
    /// changes as handled, so the conflicts are kept now or never shown.
    fn keep_conflicts(&mut self, conflicts: &[SyncConflict]) -> Result<()> {
        if conflicts.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        for conflict in conflicts {
            tx.execute(
                "INSERT INTO offline_conflicts(created_at,payload) VALUES(?1,?2)",
                params![conflict.created_at, seal(&serde_json::to_vec(conflict)?)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes a queued change that was sent (or refused), storing the
    /// numbers it taught and its conflict in the same transaction. A row
    /// rewritten while it was sent (a newer edit) stays queued.
    fn finish_outbox_entry(
        &mut self,
        entry_id: i64,
        entry_uid: &str,
        learned: &[(crate::idmap::IdKind, i64, Option<i64>)],
        conflict: Option<&SyncConflict>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        crate::idmap::save(&tx, learned)?;
        if let Some(conflict) = conflict {
            tx.execute(
                "INSERT INTO offline_conflicts(created_at,payload) VALUES(?1,?2)",
                params![conflict.created_at, seal(&serde_json::to_vec(conflict)?)?],
            )?;
        }
        delete_sent_outbox(&tx, entry_id, entry_uid)?;
        tx.commit()?;
        Ok(())
    }

    fn version_map(&self) -> Result<BTreeMap<(String, i64), i64>> {
        let mut statement = self
            .conn
            .prepare("SELECT kind,item_id,version FROM item_versions")?;
        let rows = statement.query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, i64>(1)?),
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (key, version) = row?;
            out.insert(key, version);
        }
        Ok(out)
    }

    #[cfg(any(feature = "desktop", test))]
    /// How many changes wait to be sent and when the newest was made
    /// (RFC 3339), without opening them: cheap enough for every frame.
    pub(crate) fn offline_outbox_status(&self) -> Result<(usize, String)> {
        let (count, newest): (i64, Option<String>) = self.conn.query_row(
            "SELECT count(*),max(created_at) FROM offline_outbox",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((count as usize, newest.unwrap_or_default()))
    }

    /// Sends a change to the main computer. When it cannot be reached, or in
    /// shared-folder mode, the change is applied here and queued to be sent
    /// later. A refusal (conflict, invalid data) comes back as an error.
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
        if !self.list_offline_outbox()?.is_empty() || crate::idmap::has_pending(&self.conn)? {
            // Older changes still wait (or this library still uses its own
            // numbers): this one goes after them, in order, with the next
            // synchronization.
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
                match self.apply_snapshot_and(
                    &snapshot,
                    |file_id, capture_id| fetch_remote_pdf(&cfg_clone, file_id, capture_id),
                    |id| fetch_remote_attachment(&cfg_clone, id),
                    |tx| ensure_queue_settled(tx, &[]),
                ) {
                    // The change is on the main computer; this copy catches up
                    // with the next synchronization, after the queued change.
                    // A number the main computer gave does not exist in this
                    // copy yet: it is never handed back as if it did.
                    Err(error) if error.to_string() == QUEUE_GREW => match reply {
                        MutationReply::Unit => Ok(reply),
                        _ => bail!(
                            "A alteração foi gravada no computador principal e aparece aqui na próxima sincronização; não a repita."
                        ),
                    },
                    Err(error) => Err(error),
                    Ok(()) => Ok(reply),
                }
            }
            Ok(RpcResponse::Error { message }) | Err(RpcError::Rejected(message)) => {
                bail!("{message}")
            }
            Ok(_) => bail!("Resposta inesperada do computador principal ao gravar a alteração."),
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
        let created = crate::idmap::created_ids(&*self, &queued_mutation, &reply)?;
        self.enqueue_offline_mutation(actor, queued_mutation, base, created)?;
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
                    let ver = expected_version
                        .or_else(|| self.item_version("task", task.id).ok().map(|m| m.version));
                    IntranetMutation::SaveTask {
                        task,
                        expected_version: ver,
                    }
                }
            }
            IntranetMutation::RelinkTask {
                task_id,
                document_id,
                expected_version,
            } => {
                let ver = expected_version
                    .or_else(|| self.item_version("task", task_id).ok().map(|m| m.version));
                IntranetMutation::RelinkTask {
                    task_id,
                    document_id,
                    expected_version: ver,
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
            IntranetMutation::SaveTemplate {
                template,
                expected_version,
            } => {
                let ver = expected_version.or_else(|| self.uid_version("template", &template.uid));
                IntranetMutation::SaveTemplate {
                    template,
                    expected_version: ver,
                }
            }
            IntranetMutation::SetTemplateStatus {
                uid,
                status,
                plan,
                expected_version,
            } => {
                let ver = expected_version.or_else(|| self.uid_version("template", &uid));
                IntranetMutation::SetTemplateStatus {
                    uid,
                    status,
                    plan,
                    expected_version: ver,
                }
            }
            IntranetMutation::SaveInstructionBlock {
                block,
                expected_version,
            } => {
                let ver = expected_version.or_else(|| self.uid_version("block", &block.uid));
                IntranetMutation::SaveInstructionBlock {
                    block,
                    expected_version: ver,
                }
            }
            IntranetMutation::SetBlockStatus {
                uid,
                status,
                expected_version,
            } => {
                let ver = expected_version.or_else(|| self.uid_version("block", &uid));
                IntranetMutation::SetBlockStatus {
                    uid,
                    status,
                    expected_version: ver,
                }
            }
            IntranetMutation::SaveExample {
                example,
                expected_version,
            } => {
                let ver = expected_version.or_else(|| self.uid_version("example", &example.uid));
                IntranetMutation::SaveExample {
                    example,
                    expected_version: ver,
                }
            }
            other => other,
        }
    }

    /// The version of a uid-addressed item (template, block, example), or
    /// `None` when it does not exist here yet (a creation).
    fn uid_version(&self, kind: &str, uid: &str) -> Option<i64> {
        if uid.is_empty() {
            return None;
        }
        let exists: bool = match kind {
            "template" => self.template_by_uid(uid).ok().flatten().is_some(),
            "block" => self.block_by_uid(uid).ok().flatten().is_some(),
            _ => self
                .list_examples()
                .ok()
                .is_some_and(|list| list.iter().any(|e| e.uid == uid)),
        };
        if !exists {
            return None;
        }
        self.item_version(kind, crate::work_templates::version_key(uid))
            .ok()
            .map(|m| m.version)
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
                let current = self
                    .list_tasks()
                    .ok()?
                    .into_iter()
                    .find(|t| t.id == task.id)?;
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
        created: Vec<crate::idmap::CreatedId>,
    ) -> Result<()> {
        let existing = self.list_offline_outbox()?;
        // A relink is never merged into another change, and nothing is merged
        // across it: later edits of the task follow it in the queue.
        let target_key = if matches!(mutation, IntranetMutation::RelinkTask { .. }) {
            None
        } else {
            mutation_target_key(&mutation)
        };
        if let Some(key) = target_key {
            if let Some(prev) = existing
                .iter()
                .rev()
                .find(|e| mutation_target_key(&e.mutation).as_deref() == Some(key.as_str()))
                .filter(|e| !matches!(e.mutation, IntranetMutation::RelinkTask { .. }))
                .cloned()
            {
                // The merged edit keeps the place of the first one (later
                // changes may depend on it), unless it now refers to items
                // created by changes queued after that place.
                let later = crate::idmap::IdMap::refusing(
                    existing
                        .iter()
                        .filter(|e| e.id > prev.id)
                        .flat_map(|e| e.created.iter()),
                );
                let must_move = later.remap(&mutation, &*self, None).is_err();
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
                    // Edits create nothing: what the first one recorded stays.
                    created: prev.created,
                };
                let payload = seal(&serde_json::to_vec(&updated)?)?;
                if must_move {
                    let tx = self.conn.transaction()?;
                    tx.execute("DELETE FROM offline_outbox WHERE id=?1", params![prev.id])?;
                    tx.execute(
                        "INSERT INTO offline_outbox(created_at,payload) VALUES(?1,?2)",
                        params![updated.created_at, payload],
                    )?;
                    tx.commit()?;
                } else {
                    self.conn.execute(
                        "UPDATE offline_outbox SET created_at=?1, payload=?2 WHERE id=?3",
                        params![updated.created_at, payload, prev.id],
                    )?;
                }
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
            created,
        };
        self.conn.execute(
            "INSERT INTO offline_outbox(created_at,payload) VALUES(?1,?2)",
            params![entry.created_at, seal(&serde_json::to_vec(&entry)?)?],
        )?;
        Ok(())
    }

    /// Changes saved here and not yet sent, oldest first; memoized until the
    /// library changes.
    pub fn list_offline_outbox(&self) -> Result<Vec<OutboxEntry>> {
        self.cached("offline_outbox", || self.list_offline_outbox_uncached())
    }
    fn list_offline_outbox_uncached(&self) -> Result<Vec<OutboxEntry>> {
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
                item.uid = outbox_uid("", &item.created_at, id);
            }
            out.push(item);
        }
        Ok(out)
    }

    /// Editing conflicts waiting for a decision; memoized until the library
    /// changes.
    pub fn list_sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        self.cached("sync_conflicts", || self.list_sync_conflicts_uncached())
    }
    fn list_sync_conflicts_uncached(&self) -> Result<Vec<SyncConflict>> {
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
            .context("Conflito de edição não encontrado.")?;

        // Its numbers mean nothing in the team's library: only redone.
        if conflict.unresolved && resolution != ConflictResolution::KeepHost {
            bail!("{}", crate::idmap::UNRESOLVED);
        }
        // The conflict uses the team's numbers; while a translation is
        // pending (a synchronization that stopped halfway) this library
        // still uses its own.
        if resolution != ConflictResolution::KeepHost && crate::idmap::has_pending(&self.conn)? {
            bail!("A sincronização com o computador principal ainda não terminou. Sincronize e tente de novo.");
        }
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
            IntranetMutation::SaveTask { mut task, .. } => {
                let ver = self.item_version("task", task.id)?.version;
                // The link is not edited by a plain save: it follows the one
                // the task has now (another computer may have relinked it).
                if let Some(current) = self.list_tasks()?.into_iter().find(|t| t.id == task.id) {
                    task.document_id = current.document_id;
                }
                IntranetMutation::SaveTask {
                    task,
                    expected_version: Some(ver),
                }
            }
            IntranetMutation::RelinkTask {
                task_id,
                document_id,
                ..
            } => {
                let ver = self.item_version("task", task_id)?.version;
                IntranetMutation::RelinkTask {
                    task_id,
                    document_id,
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
            IntranetMutation::CreateTaskForSaiop {
                attendance_id,
                task,
                title,
                ..
            } => {
                let ver = self.item_version("saiop", attendance_id)?.version;
                IntranetMutation::CreateTaskForSaiop {
                    attendance_id,
                    task,
                    title,
                    expected_version: Some(ver),
                }
            }
            IntranetMutation::SaveTemplate { template, .. } => {
                let ver = self.uid_version("template", &template.uid);
                IntranetMutation::SaveTemplate {
                    template,
                    expected_version: ver,
                }
            }
            IntranetMutation::SetTemplateStatus {
                uid, status, plan, ..
            } => {
                let ver = self.uid_version("template", &uid);
                IntranetMutation::SetTemplateStatus {
                    uid,
                    status,
                    plan,
                    expected_version: ver,
                }
            }
            IntranetMutation::SaveInstructionBlock { block, .. } => {
                let ver = self.uid_version("block", &block.uid);
                IntranetMutation::SaveInstructionBlock {
                    block,
                    expected_version: ver,
                }
            }
            IntranetMutation::SetBlockStatus { uid, status, .. } => {
                let ver = self.uid_version("block", &uid);
                IntranetMutation::SetBlockStatus {
                    uid,
                    status,
                    expected_version: ver,
                }
            }
            IntranetMutation::SaveExample { example, .. } => {
                let ver = self.uid_version("example", &example.uid);
                IntranetMutation::SaveExample {
                    example,
                    expected_version: ver,
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
    ///
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
                            let (value, both) =
                                pick(base_task.map(|b| &b.$field), &ht.$field, &local_task.$field);
                            if both {
                                kept.push(format!(
                                    "{}: no computador principal estava \"{}\"",
                                    $label,
                                    $show(&ht.$field)
                                ));
                            }
                            m.$field = value;
                        }};
                    }
                    let plain = |v: &String| v.clone();
                    scalar!(title, "Título", plain);
                    scalar!(action, "Ação", |v: &String| crate::tasks::action_label(v)
                        .to_owned());
                    scalar!(status, "Situação", |v: &String| {
                        crate::tasks::status_label(v).to_owned()
                    });
                    scalar!(priority, "Prioridade", |v: &String| {
                        crate::labels::priority(v).to_owned()
                    });
                    scalar!(due_date, "Prazo", |v: &String| crate::labels::date(v));
                    scalar!(due_source, "Origem do prazo", plain);
                    scalar!(reminder_date, "Lembrete", |v: &String| crate::labels::date(
                        v
                    ));
                    scalar!(assignee, "Responsável", plain);
                    m.notes = pick_text(
                        base_task.map(|b| b.notes.as_str()),
                        &ht.notes,
                        &local_task.notes,
                        actor,
                    );
                    m.outcome = pick_text(
                        base_task.map(|b| b.outcome.as_str()),
                        &ht.outcome,
                        &local_task.outcome,
                        actor,
                    );
                    for (done_l, text_l) in &local_task.checklist {
                        if let Some((done_m, _)) = m.checklist.iter_mut().find(|(_, t)| t == text_l)
                        {
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
                            kept.push(format!(
                                "{}: no computador principal estava \"{}\"",
                                $label,
                                $show(&host_ctx.$field)
                            ));
                        }
                        m.$field = value;
                    }};
                }
                let plain = |v: &String| v.clone();
                scalar!(purpose, "Finalidade", |v: &String| {
                    crate::work_context::purpose_label(v).to_owned()
                });
                scalar!(document_type, "Tipo local", plain);
                scalar!(priority, "Prioridade", |v: &String| {
                    crate::labels::priority(v).to_owned()
                });
                scalar!(sender, "Remetente informado", plain);
                scalar!(received_date, "Recebido em", |v: &String| {
                    crate::labels::date(v)
                });
                scalar!(process_number, "Processo vinculado", plain);
                m.subjects = pick_text(
                    base_ctx.map(|b| b.subjects.as_str()),
                    &host_ctx.subjects,
                    &local_ctx.subjects,
                    actor,
                );
                m.priority_reason = pick_text(
                    base_ctx.map(|b| b.priority_reason.as_str()),
                    &host_ctx.priority_reason,
                    &local_ctx.priority_reason,
                    actor,
                );
                m.notes = pick_text(
                    base_ctx.map(|b| b.notes.as_str()),
                    &host_ctx.notes,
                    &local_ctx.notes,
                    actor,
                );
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
                    Some(IntranetMutation::SaveSaiopAttendance { attendance, .. }) => {
                        Some(attendance)
                    }
                    _ => None,
                };
                let merged_att = if let Some(ha) = host_att {
                    let mut m = ha.clone();
                    let mut kept = Vec::new();
                    macro_rules! scalar {
                        ($field:ident, $label:expr, $show:expr) => {{
                            let (value, both) =
                                pick(base_att.map(|b| &b.$field), &ha.$field, &local_att.$field);
                            if both {
                                kept.push(format!(
                                    "{}: no computador principal estava \"{}\"",
                                    $label,
                                    $show(&ha.$field)
                                ));
                            }
                            m.$field = value;
                        }};
                    }
                    let plain = |v: &String| v.clone();
                    let flag = |v: &bool| {
                        if *v {
                            "sim".to_owned()
                        } else {
                            "não".to_owned()
                        }
                    };
                    scalar!(reference_raw, "Referência", plain);
                    scalar!(subject, "Assunto", plain);
                    scalar!(entry_date, "Entrada", |v: &String| crate::labels::date(v));
                    scalar!(assignee, "Responsável", plain);
                    scalar!(received_from, "Recebido de", plain);
                    scalar!(uas_return, "Retorno para UAS", |v: &String| {
                        crate::saiop::tri_state_label(v).to_owned()
                    });
                    scalar!(due_date, "Prazo", |v: &String| crate::labels::date(v));
                    scalar!(due_kind, "Tipo de prazo", |v: &String| {
                        crate::saiop::due_kind_label(v).to_owned()
                    });
                    scalar!(status, "Situação", |v: &String| {
                        crate::saiop::status_label(v).to_owned()
                    });
                    scalar!(priority, "Prioridade", |v: &String| {
                        crate::saiop::priority_label(v).to_owned()
                    });
                    scalar!(priority_reason, "Motivo da prioridade", plain);
                    scalar!(signed_status, "Assinado", |v: &String| {
                        crate::saiop::tri_state_label(v).to_owned()
                    });
                    scalar!(exit_date, "Data de saída", |v: &String| {
                        crate::labels::date(v)
                    });
                    scalar!(planned_doc_type, "Entrega prevista", plain);
                    scalar!(sent_to, "Enviado para", plain);
                    scalar!(outcome, "Resultado da etapa", |v: &String| {
                        crate::saiop::outcome_label(v).to_owned()
                    });
                    scalar!(scheduled, "Agendado", flag);
                    scalar!(sensitive, "Documento sensível", flag);
                    scalar!(attachment_relation, "Apensado / juntado", plain);
                    m.reference_norm = crate::saiop::normalize_reference(&m.reference_raw);
                    m.observation = pick_text(
                        base_att.map(|b| b.observation.as_str()),
                        &ha.observation,
                        &local_att.observation,
                        actor,
                    );
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
                        if !m.events.iter().any(|e| {
                            e.when == ev.when && e.kind == ev.kind && e.detail == ev.detail
                        }) {
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
                    base: base
                        .as_ref()
                        .and_then(|b| b.get(index).map(|(_, v)| v.clone())),
                })
                .collect()
        }
        match &conflict.mutation {
            IntranetMutation::SaveNote {
                document_id, note, ..
            } => {
                let host = self
                    .load_document(*document_id)
                    .map(|d| d.note)
                    .unwrap_or_default();
                let base = match &conflict.base {
                    Some(IntranetMutation::SaveNote { note, .. }) => Some(note.clone()),
                    _ => None,
                };
                if host == *note {
                    vec![]
                } else {
                    vec![FieldDiff {
                        label: "Notas do documento".into(),
                        host,
                        local: note.clone(),
                        base,
                    }]
                }
            }
            IntranetMutation::SaveTask { task, .. } => {
                let Some(host) = self
                    .list_tasks()
                    .ok()
                    .and_then(|t| t.into_iter().find(|h| h.id == task.id))
                else {
                    return vec![];
                };
                let base = match &conflict.base {
                    Some(IntranetMutation::SaveTask { task, .. }) => Some(task_fields(task)),
                    _ => None,
                };
                diff(task_fields(&host), task_fields(task), base)
            }
            IntranetMutation::SaveWorkContext {
                document_id, value, ..
            } => {
                let host = self.load_work_context(*document_id).unwrap_or_default();
                let base = match &conflict.base {
                    Some(IntranetMutation::SaveWorkContext { value, .. }) => {
                        Some(context_fields(value))
                    }
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
                    Some(IntranetMutation::SaveSaiopAttendance { attendance, .. }) => {
                        Some(saiop_fields(attendance))
                    }
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
        (
            "Prioridade",
            crate::labels::priority(&t.priority).to_owned(),
        ),
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
        (
            "Finalidade",
            crate::work_context::purpose_label(&c.purpose).to_owned(),
        ),
        ("Assuntos", c.subjects.clone()),
        ("Tipo local", c.document_type.clone()),
        (
            "Prioridade",
            crate::labels::priority(&c.priority).to_owned(),
        ),
        ("Motivo da prioridade", c.priority_reason.clone()),
        ("Remetente informado", c.sender.clone()),
        ("Recebido em", crate::labels::date(&c.received_date)),
        ("Processo vinculado", c.process_number.clone()),
        ("Orientação e contexto", c.notes.clone()),
    ]
}

fn saiop_fields(a: &crate::saiop::SaiopAttendance) -> Vec<(&'static str, String)> {
    let flag = |v: bool| {
        if v {
            "sim".to_owned()
        } else {
            "não".to_owned()
        }
    };
    vec![
        ("Referência", a.reference_raw.clone()),
        ("Assunto", a.subject.clone()),
        ("Entrada", crate::labels::date(&a.entry_date)),
        ("Responsável", a.assignee.clone()),
        ("Recebido de", a.received_from.clone()),
        (
            "Retorno para UAS",
            crate::saiop::tri_state_label(&a.uas_return).to_owned(),
        ),
        ("Prazo", crate::labels::date(&a.due_date)),
        (
            "Tipo de prazo",
            crate::saiop::due_kind_label(&a.due_kind).to_owned(),
        ),
        ("Situação", crate::saiop::status_label(&a.status).to_owned()),
        (
            "Prioridade",
            crate::saiop::priority_label(&a.priority).to_owned(),
        ),
        ("Motivo da prioridade", a.priority_reason.clone()),
        (
            "Assinado",
            crate::saiop::tri_state_label(&a.signed_status).to_owned(),
        ),
        ("Data de saída", crate::labels::date(&a.exit_date)),
        ("Entrega prevista", a.planned_doc_type.clone()),
        ("Enviado para", a.sent_to.clone()),
        (
            "Resultado da etapa",
            crate::saiop::outcome_label(&a.outcome).to_owned(),
        ),
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
        IntranetMutation::SaveTask { task, .. } if task.id > 0 => Some(format!("task:{}", task.id)),
        IntranetMutation::RelinkTask { task_id, .. } => Some(format!("task:{task_id}")),
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

/// "1 alteração" / "3 alterações".
fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

fn mutation_label(mutation: &IntranetMutation) -> String {
    match mutation {
        IntranetMutation::ImportCapture { capture } if capture.is_manual() => {
            let email = capture.metadata.get("manual_kind").map(String::as_str) == Some("email");
            match (
                email,
                capture.document_number == crate::model::MANUAL_PENDING,
            ) {
                (true, true) => "E-mail importado".to_owned(),
                (true, false) => format!("E-mail {}", capture.document_number),
                (false, true) => "Registro manual novo".to_owned(),
                (false, false) => format!("Registro manual {}", capture.document_number),
            }
        }
        IntranetMutation::ImportCapture { capture } => {
            format!("Importação de {}", capture.document_number)
        }
        IntranetMutation::ImportBundle { capture, files } => format!(
            "Importação de {} com {}",
            capture.document_number,
            plural(files.len(), "PDF", "PDFs")
        ),
        IntranetMutation::SaveNote { document_id, .. } => {
            format!("Notas do documento #{document_id}")
        }
        IntranetMutation::SaveDraft {
            document_id, kind, ..
        } => format!("Minuta ({kind}) do documento #{document_id}"),
        IntranetMutation::SaveTemplate { template, .. } => {
            format!("Modelo de trabalho \"{}\"", template.name)
        }
        IntranetMutation::SetTemplateStatus { status, .. } => {
            format!("Modelo de trabalho: {}", status.label().to_lowercase())
        }
        IntranetMutation::DeleteTemplate { .. } => "Exclusão de modelo de trabalho".to_owned(),
        IntranetMutation::SaveInstructionBlock { block, .. } => {
            format!("Instrução \"{}\"", block.name)
        }
        IntranetMutation::SetBlockStatus { status, .. } => {
            format!("Instrução: {}", status.label().to_lowercase())
        }
        IntranetMutation::DeleteBlock { .. } => "Exclusão de instrução".to_owned(),
        IntranetMutation::SaveExample { example, .. } => {
            format!("Exemplo \"{}\"", example.title)
        }
        IntranetMutation::DeleteExample { .. } => "Exclusão de exemplo".to_owned(),
        IntranetMutation::RecordGeneration { record } => {
            format!("Registro de geração do documento #{}", record.document_id)
        }
        IntranetMutation::ExtractAttachmentText { attachment_id } => {
            format!("Texto do anexo #{attachment_id}")
        }
        IntranetMutation::ReviewAttachmentText { attachment_id, .. } => {
            format!("Texto conferido do anexo #{attachment_id}")
        }
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
        IntranetMutation::RelinkTask { task_id, .. } => {
            format!("Tarefa #{task_id}: troca de documento")
        }
        IntranetMutation::SaveWorkContext { document_id, .. } => {
            format!("Contexto do documento #{document_id}")
        }
        IntranetMutation::SavePageText { file_id, page, .. } => {
            format!("Texto lido da página {page} (arquivo #{file_id})")
        }
        IntranetMutation::AddDocumentLink { source, target, .. } => {
            format!("Vínculo entre documentos #{source} e #{target}")
        }
        IntranetMutation::SetLinkActive { id, .. } => format!("Situação do vínculo #{id}"),
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
                    "Atendimento #{}: {}",
                    attendance.id, attendance.reference_raw
                )
            } else {
                format!("Novo atendimento: {}", attendance.reference_raw)
            }
        }
        IntranetMutation::ImportSaiopBatch { file_name, .. } => {
            format!("Importação de planilha {file_name}")
        }
        IntranetMutation::UndoSaiopBatch { batch_id } => {
            format!("Desfazer importação de planilha #{batch_id}")
        }
        IntranetMutation::DeleteSaiopAttendance { id } => {
            format!("Exclusão do atendimento #{id}")
        }
        IntranetMutation::CreateTaskForSaiop {
            attendance_id,
            title,
            ..
        } => format!("Tarefa \"{title}\" do atendimento #{attendance_id}"),
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
        IntranetMutation::DeleteAttachment { id } => format!("Exclusão definitiva do anexo #{id}"),
        IntranetMutation::PurgeTrash { kind, id } => format!(
            "Apagado de vez da lixeira: {} #{id}",
            crate::labels::item_kind(kind)
        ),
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
                "{} e {} recebidos",
                plural(self.imported_new, "item novo", "itens novos"),
                plural(self.imported_changed, "atualizado", "atualizados")
            ));
        }
        if self.pushed > 0 {
            parts.push(format!(
                "{} deste computador {}",
                plural(self.pushed, "alteração", "alterações"),
                if self.pushed == 1 {
                    "enviada"
                } else {
                    "enviadas"
                }
            ));
        }
        if self.merged_foreign > 0 {
            parts.push(format!(
                "{} de outras pastas {}",
                plural(self.merged_foreign, "alteração", "alterações"),
                if self.merged_foreign == 1 {
                    "reunida"
                } else {
                    "reunidas"
                }
            ));
        }
        if self.conflicts > 0 {
            parts.push(format!(
                "⚠ {} para decidir",
                plural(self.conflicts, "conflito de edição", "conflitos de edição")
            ));
        }
        if !self.written_to.is_empty() {
            parts.push(format!(
                "gravado em {}",
                plural(self.written_to.len(), "pasta", "pastas")
            ));
        }
        if !self.unreachable.is_empty() {
            parts.push(format!(
                "{} fora de alcance",
                plural(self.unreachable.len(), "pasta", "pastas")
            ));
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
        blobs
            .pdfs
            .insert(meta.id, store.pdf_bytes(meta.id, meta.capture_id)?);
    }
    for row in &snapshot.attachments {
        blobs
            .attachments
            .insert(row.id, store.attachment_bytes(row.id)?.1);
    }
    Ok(blobs)
}

/// A conflict to record, `unresolved` when the change cannot be translated.
fn conflict_for(
    entry: &OutboxEntry,
    reason: &str,
    snapshot: Option<&CabinetSnapshot>,
    unresolved: bool,
) -> SyncConflict {
    let mut conflict = build_sync_conflict(entry, reason, snapshot);
    conflict.unresolved = unresolved;
    conflict
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
                .unwrap_or_else(|| "(vazia no computador principal)".into());
            (
                format!("Nota deste computador: {}", note.trim()),
                format!("Nota no computador principal: {}", host_note.trim()),
            )
        }
        IntranetMutation::SaveTask { task, .. } => {
            let host_desc = host_snapshot
                .and_then(|s| s.tasks.iter().find(|t| t.id == task.id))
                .map(|t| {
                    format!(
                        "Título: '{}' | Responsável: '{}' | Situação: {} | Notas: '{}'",
                        t.title,
                        t.assignee,
                        crate::tasks::status_label(&t.status),
                        t.notes
                    )
                })
                .unwrap_or_else(|| "(não encontrada no computador principal)".into());
            let local_desc = format!(
                "Título: '{}' | Responsável: '{}' | Situação: {} | Notas: '{}'",
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
                .unwrap_or_else(|| "(sem contexto no computador principal)".into());
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
                        "Situação: {} | Responsável: '{}' | Observação: '{}'",
                        crate::saiop::status_label(&r.attendance.status),
                        r.attendance.assignee,
                        r.attendance.observation
                    )
                })
                .unwrap_or_else(|| "(não localizado no computador principal)".into());
            let local_desc = format!(
                "Situação: {} | Responsável: '{}' | Observação: '{}'",
                crate::saiop::status_label(&attendance.status),
                attendance.assignee,
                attendance.observation
            );
            (local_desc, host_desc)
        }
        _ => (
            entry.label.clone(),
            "Também alterado no computador principal".into(),
        ),
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
        unresolved: false,
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
        _ => bail!("Resposta inesperada ao verificar o computador principal."),
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
            .context("O PDF recebido do computador principal está danificado."),
        RpcResponse::Error { message } => bail!("{message}"),
        _ => bail!("Resposta inesperada ao receber o PDF do computador principal."),
    }
}

fn fetch_remote_attachment(cfg: &IntranetConfig, id: i64) -> Result<Vec<u8>> {
    match send_rpc(cfg, &RpcRequest::FetchAttachmentBytes { id })? {
        RpcResponse::FileBytes { data_base64 } => STANDARD
            .decode(data_base64.trim())
            .context("O anexo recebido do computador principal está danificado."),
        RpcResponse::Error { message } => bail!("{message}"),
        _ => bail!("Resposta inesperada ao receber o anexo do computador principal."),
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
        .map_err(|e| {
            RpcError::Unreachable(format!(
                "Não foi possível preparar a conexão com o computador principal: {e}"
            ))
        })?;
    let response = client
        .post(&endpoint)
        .header(
            "Authorization",
            format!("Bearer {}", cfg.cabinet_token.trim()),
        )
        .json(request)
        .send()
        .map_err(|e| {
            RpcError::Unreachable(format!(
                "Sem conexão com o computador principal em {base}: {e}"
            ))
        })?;
    let status = response.status();
    if status.is_server_error() {
        // Busy or failing for a moment (library locked, too many
        // connections), whatever the body says or even if it was cut off:
        // the change waits and is sent again later; it is not a refusal of
        // the change itself.
        return Err(RpcError::Unreachable(
            match response.json::<RpcResponse>() {
                Ok(RpcResponse::Error { message }) => message,
                _ => format!(
                    "O computador principal não conseguiu responder (código {}).",
                    status.as_u16()
                ),
            },
        ));
    }
    let rpc: RpcResponse = response.json().map_err(|e| {
        RpcError::Rejected(format!(
            "Resposta inválida recebida do computador principal: {e}"
        ))
    })?;
    if let RpcResponse::Error { message } = &rpc {
        if message == HOST_TOO_OLD {
            // A main computer on an older version does not know this kind
            // of change: it waits here, queued, until that computer is
            // updated, instead of becoming a conflict and being lost.
            return Err(RpcError::Unreachable(
                "O computador principal está numa versão anterior do Ponte e ainda não reconhece esta alteração; atualize-o. A alteração continua guardada aqui.".into(),
            ));
        }
    }
    if !status.is_success() {
        if let RpcResponse::Error { message } = rpc {
            return Err(RpcError::Rejected(message));
        }
        return Err(RpcError::Rejected(format!(
            "O computador principal recusou a operação (código {}).",
            status.as_u16()
        )));
    }
    Ok(rpc)
}

pub struct HostServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    slots: Arc<Slots>,
}

/// Connections served at the same time, in all and from one computer. Each
/// has its own thread, so a slow or hostile peer only holds its own slots;
/// the library itself is used by one request at a time, and only while the
/// request is processed (never while bytes travel).
const MAX_CONNECTIONS: usize = 16;
const MAX_CONNECTIONS_PER_PEER: usize = 4;

/// Connections being served, by peer address.
#[derive(Default)]
struct Slots {
    busy: Mutex<BTreeMap<IpAddr, usize>>,
}

/// One connection being served; frees its slot when dropped.
struct Slot {
    slots: Arc<Slots>,
    peer: IpAddr,
}

impl Slots {
    fn take(slots: &Arc<Slots>, peer: IpAddr) -> Option<Slot> {
        let mut busy = slots.busy.lock().ok()?;
        let total: usize = busy.values().sum();
        let mine = busy.entry(peer).or_insert(0);
        if total >= MAX_CONNECTIONS || *mine >= MAX_CONNECTIONS_PER_PEER {
            if *mine == 0 {
                busy.remove(&peer);
            }
            return None;
        }
        *mine += 1;
        Some(Slot {
            slots: Arc::clone(slots),
            peer,
        })
    }

    fn in_use(&self) -> usize {
        self.busy.lock().map(|b| b.values().sum()).unwrap_or(0)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut busy) = self.slots.busy.lock() {
            if let Some(count) = busy.get_mut(&self.peer) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    busy.remove(&self.peer);
                }
            }
        }
    }
}

/// Refusals being answered at the same time; beyond that a connection is
/// simply closed.
const MAX_REFUSALS: usize = 32;

/// Answers, on a short-lived thread, a connection that found no free slot:
/// "busy, try again" (HTTP 503), which the sender treats as a temporary
/// absence. The request is read first (briefly, at most 64 KiB): closing a
/// socket with unread data would reset it before the answer arrives.
fn refuse_busy(stream: TcpStream, refusals: &Arc<std::sync::atomic::AtomicUsize>) {
    if refusals.fetch_add(1, Ordering::SeqCst) >= MAX_REFUSALS {
        refusals.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let counter = Arc::clone(refusals);
    let answer = move || {
        let mut stream = stream;
        let drain = |stream: &mut TcpStream| {
            let deadline = Instant::now() + Duration::from_millis(500);
            let mut buffer = [0u8; 8192];
            let mut total = 0usize;
            while total < 64 * 1024 {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
                    break;
                }
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => total += read,
                }
            }
        };
        if stream.set_nonblocking(false).is_ok() {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            drain(&mut stream);
            let _ = write_http_json(
                &mut stream,
                503,
                &RpcResponse::Error {
                    message: "O computador principal está atendendo muitas conexões agora; tente de novo em instantes.".into(),
                },
            );
            let _ = stream.shutdown(std::net::Shutdown::Write);
            drain(&mut stream);
        }
    };
    let spawned = std::thread::Builder::new()
        .name("ponte-intranet-busy".into())
        .spawn(move || {
            answer();
            counter.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        refusals.fetch_sub(1, Ordering::SeqCst);
    }
}

impl HostServer {
    pub fn start(db_path: PathBuf, bind_addr: &str) -> Result<Self> {
        let requested = validate_bind_addr(bind_addr)?;
        let listener = TcpListener::bind(requested).with_context(|| {
            format!(
                "Não foi possível abrir a porta {requested} da rede interna. Verifique se outra janela do Ponte já está usando essa porta."
            )
        })?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let db_lock = Arc::new(Mutex::new(db_path));
        let slots = Arc::new(Slots::default());
        let accept_slots = Arc::clone(&slots);
        let refusals = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let handle = std::thread::Builder::new()
            .name("ponte-intranet-host".into())
            .spawn(move || {
                while !stop_flag.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, peer)) => {
                            if !is_allowed_peer_ip(peer.ip()) {
                                continue;
                            }
                            let Some(slot) = Slots::take(&accept_slots, peer.ip()) else {
                                refuse_busy(stream, &refusals);
                                continue;
                            };
                            let db = Arc::clone(&db_lock);
                            // If the thread cannot start, the closure (with
                            // the connection and the slot) is dropped.
                            let _ = std::thread::Builder::new()
                                .name("ponte-intranet-connection".into())
                                .spawn(move || {
                                    let _slot = slot;
                                    let _ = handle_connection(stream, &db);
                                });
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
            .context("Não foi possível iniciar o computador principal.")?;

        Ok(Self {
            addr,
            stop,
            handle: Some(handle),
            slots,
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
        // Requests already accepted finish (each is bounded by its deadlines);
        // a stop waits a little for them, so the library is left quiet.
        let waited = Instant::now();
        while self.slots.in_use() > 0 && waited.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for HostServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Time a team request has to send its headers, and then its body (up to
/// `MAX_HTTP_BODY`, so generous for a slow network).
const HTTP_HEADER_TIME: Duration = Duration::from_secs(30);
const HTTP_BODY_TIME: Duration = Duration::from_secs(180);

/// A socket whose every read must finish before `deadline`: the remaining
/// time becomes the socket timeout before each read, so a slow sender is cut
/// off when the whole part is due, not after each byte.
struct DeadlineReader {
    stream: TcpStream,
    deadline: Instant,
}

impl Read for DeadlineReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "requisição enviada devagar demais; conexão encerrada",
            ));
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buf)
    }
}

fn handle_connection(mut stream: TcpStream, db_lock: &Mutex<PathBuf>) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(20)))?;

    // Every read, however small, must finish before a deadline for the whole
    // part being read (headers, then body): a peer that sends an endless line
    // or one byte at a time cannot grow memory nor keep its slot for long.
    let mut reader = BufReader::new(DeadlineReader {
        stream: stream.try_clone()?,
        deadline: Instant::now() + HTTP_HEADER_TIME,
    });
    let mut head = (&mut reader).take(MAX_HTTP_HEADER);
    let mut request_line = String::new();
    if head.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_http_json(
            &mut stream,
            400,
            &RpcResponse::Error {
                message: "Pedido inválido recebido pelo computador principal.".into(),
            },
        );
    }
    let method = parts[0];
    let path = parts[1];

    let mut content_length = 0usize;
    let mut auth_token = String::new();
    loop {
        let mut line = String::new();
        let read = head.read_line(&mut line)?;
        if read == 0 {
            // Either the peer closed early or the window ran out mid-header.
            ensure!(head.limit() > 0, "A mensagem recebida é grande demais.");
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let key = name.trim().to_ascii_lowercase();
            let val = value.trim();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            } else if key == "authorization" {
                if let Some(rest) = val
                    .strip_prefix("Bearer ")
                    .or_else(|| val.strip_prefix("bearer "))
                {
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
                message: "Pedido não reconhecido pelo computador principal do Ponte.".into(),
            },
        );
    }
    if content_length == 0 || content_length > MAX_HTTP_BODY {
        return write_http_json(
            &mut stream,
            413,
            &RpcResponse::Error {
                message: "O pedido é grande demais para o computador principal.".into(),
            },
        );
    }

    reader.get_mut().deadline = Instant::now() + HTTP_BODY_TIME;
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;

    // The library is used by one request at a time, and released before the
    // answer travels: a peer that reads slowly holds nothing but its slot.
    let (status, response) = {
        let path_guard = db_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("O computador principal está ocupado. Tente de novo."))?;
        answer_rpc(&path_guard, &auth_token, &body)
    };
    write_http_json(&mut stream, status, &response)
}

/// Opens the library, checks the token and processes one request.
fn answer_rpc(db_path: &std::path::Path, auth_token: &str, body: &[u8]) -> (u16, RpcResponse) {
    let error = |message: String| RpcResponse::Error { message };
    let mut store = match Store::open(db_path) {
        Ok(s) => s,
        Err(e) => {
            return (
                500,
                error(format!(
                    "Não foi possível abrir o acervo do computador principal: {e}"
                )),
            )
        }
    };
    let host_cfg = match store.load_intranet_config() {
        Ok(c) => c,
        Err(e) => {
            return (
                500,
                error(format!(
                    "Não foi possível ler os ajustes do computador principal: {e}"
                )),
            )
        }
    };
    if !verify_token_constant_time(&host_cfg.cabinet_token, auth_token) {
        return (
            401,
            error("Chave de acesso do gabinete inválida ou sem autorização.".into()),
        );
    }
    let rpc_req: RpcRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return (400, error(host_cannot_read())),
    };
    match process_rpc(&mut store, &host_cfg, rpc_req) {
        Ok(resp) => (200, resp),
        Err(err) => (400, error(err.to_string())),
    }
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
        503 => "Service Unavailable",
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

#[cfg(feature = "desktop")]
/// Result of one synchronization run on the background thread.
struct SyncOutcome {
    result: std::result::Result<SyncDone, String>,
    manual: bool,
}

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
struct SyncWorker {
    requests: mpsc::Sender<SyncRequest>,
    outcomes: mpsc::Receiver<SyncOutcome>,
    /// Steps reported while a synchronization runs.
    progress: mpsc::Receiver<String>,
}

#[cfg(feature = "desktop")]
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
                        Some(store) => {
                            run_sync(store, &request, &reporter).map_err(|e| e.to_string())
                        }
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

#[cfg(feature = "desktop")]
/// What a finished synchronization reports back to the window.
pub struct SyncDone {
    pub changed: bool,
    pub summary: String,
}

#[cfg(feature = "desktop")]
fn run_sync(
    store: &mut Store,
    request: &SyncRequest,
    reporter: &crate::ai::Reporter,
) -> Result<SyncDone> {
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

#[cfg(feature = "desktop")]
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
    /// Result of "Testar conexão": the address tested, success, text.
    connection_test: Option<(String, bool, String)>,
}

#[cfg(feature = "desktop")]
const SYNC_INTERVAL: Duration = Duration::from_secs(4);
#[cfg(feature = "desktop")]
/// Shared folders are re-read every few minutes; the cloud client needs
/// that long to carry a file anyway. Opening the Ponte and "Sincronizar
/// agora" do not wait.
const FOLDER_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
#[cfg(feature = "desktop")]
/// A change made here goes to the folders without waiting for the five
/// minutes: once nothing has changed for this long (changes made in a row
/// travel together)...
const FOLDER_CHANGE_QUIET: Duration = Duration::from_secs(30);
#[cfg(feature = "desktop")]
/// ...and no sooner than this after the previous exchange, so steady
/// editing does not rewrite (and upload) the whole file every few seconds.
const FOLDER_CHANGE_MIN_GAP: Duration = Duration::from_secs(2 * 60);

#[cfg(feature = "desktop")]
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

    pub fn initialize(&mut self, store: &mut Store, db_path: &std::path::Path) {
        if let Ok(cfg) = store.load_intranet_config() {
            self.draft = cfg.clone();
            self.loaded = true;
            if cfg.mode == IntranetMode::Host && cfg.auto_start_host && self.server.is_none() {
                match HostServer::start(db_path.to_path_buf(), &cfg.bind_addr) {
                    Ok(srv) => {
                        self.status_summary = format!(
                            "Computador principal ativo em {} • Integrante: {}",
                            srv.local_addr(),
                            cfg.member_name
                        );
                        self.server = Some(srv);
                    }
                    Err(e) => {
                        self.error = true;
                        self.message =
                            format!("Não foi possível iniciar o computador principal: {e}");
                    }
                }
            } else if cfg.mode.queues_changes() {
                self.refresh_client(store, db_path);
            }
        }
    }

    /// Saves what the person edited in the window without clobbering what
    /// the background synchronization recorded meanwhile (revision, last
    /// sync, lineage): the window's copy of the configuration may be older
    /// than the stored one.
    fn save_draft(&mut self, store: &mut Store) -> Result<()> {
        if let Ok(stored) = store.load_intranet_config() {
            self.draft.synced_revision = stored.synced_revision;
            self.draft.last_sync_at = stored.last_sync_at;
            self.draft.folder_lineage = stored.folder_lineage;
            if self.draft.folder_writer.trim().is_empty() {
                self.draft.folder_writer = stored.folder_writer;
            }
        }
        store.save_intranet_config(&self.draft)
    }

    /// Brings the window's copy up to date after a synchronization.
    fn refresh_draft_bookkeeping(&mut self, store: &Store) {
        if let Ok(stored) = store.load_intranet_config() {
            self.draft.synced_revision = stored.synced_revision;
            self.draft.last_sync_at = stored.last_sync_at;
            self.draft.folder_lineage = stored.folder_lineage;
        }
    }

    /// The "Pasta compartilhada" part of the window: folders, password and
    /// the main row.
    fn shared_folder_section(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        db_path: &std::path::Path,
        refresh_needed: &mut bool,
    ) {
        use crate::theme::{self, icon};
        use crate::widgets::{self, Tone};
        let saved_revision = store
            .load_intranet_config()
            .map(|c| c.synced_revision)
            .unwrap_or(0);
        let first_time = saved_revision == 0;
        let dirty = self.has_unsaved_draft(store);

        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::section(ui, "Pastas");
                crate::help_ui::help(ui, "Os computadores trocam o acervo por um arquivo protegido pela sua senha, guardado nas pastas que você escolher: Google Drive, OneDrive, pendrive, pasta da rede. Pode ser mais de uma pasta; o Ponte usa a cópia mais recente e grava em todas as que alcançar.");
            });
            widgets::small_muted(ui, "O arquivo ponte-sync.ponte fica dentro de cada pasta.");
            ui.add_space(theme::S2);
            let mut remove: Option<usize> = None;
            for (index, path) in self.draft.folder_files.clone().iter().enumerate() {
                let generic_hint = "Confira se a pasta está disponível neste computador.";
                let (line, pill_text, tone, what_to_do) =
                    match crate::sharedfolder::read_header(std::path::Path::new(path)) {
                        Ok(Some(header)) => {
                            let line = crate::sharedfolder::describe(&header);
                            if header.revision < saved_revision {
                                (line, "Atrás deste computador", Tone::Warn, None)
                            } else {
                                (line, "Em dia", Tone::Ok, None)
                            }
                        }
                        Ok(None) => {
                            if std::path::Path::new(path)
                                .parent()
                                .is_some_and(|p| p.is_dir())
                            {
                                (
                                    "Pasta acessível; arquivo ainda não criado.".to_owned(),
                                    "Arquivo a criar",
                                    Tone::Info,
                                    None,
                                )
                            } else {
                                let hint = if path.to_lowercase().contains("google drive") {
                                    "Abra o Google Drive e espere a pasta aparecer."
                                } else {
                                    generic_hint
                                };
                                (
                                    "Pasta fora de alcance agora.".to_owned(),
                                    "Fora de alcance",
                                    Tone::Danger,
                                    Some(hint),
                                )
                            }
                        }
                        Err(error) => (
                            error.to_string(),
                            "Com problema",
                            Tone::Danger,
                            Some(generic_hint),
                        ),
                    };
                if index > 0 {
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width((ui.available_width() - 230.0).max(160.0));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(path)
                                    .font(theme::font_strong(theme::SIZE_BODY)),
                            )
                            .wrap(),
                        );
                        widgets::small_muted(ui, line);
                        if let Some(hint) = what_to_do {
                            widgets::small_muted(ui, hint);
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::quiet("Remover")).clicked() {
                            remove = Some(index);
                        }
                        ui.horizontal(|ui| widgets::pill(ui, pill_text, tone));
                    });
                });
            }
            if let Some(index) = remove {
                self.draft.folder_files.remove(index);
            }
            if self.draft.folder_files.is_empty() {
                widgets::muted(ui, "Nenhuma pasta escolhida ainda.");
            }
            if self.draft.folder_files.len() < 8 {
                ui.add_space(theme::S2);
                if ui
                    .add(widgets::secondary(format!("{} Adicionar pasta…", icon::FOLDER)))
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
            }
        });
        ui.add_space(theme::S3);

        widgets::card(ui, |ui| {
            widgets::section(ui, "Senha de sincronização");
            ui.add_space(theme::S2);
            widgets::field_label(ui, "Senha");
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 100.0).max(160.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.folder_key)
                        .password(!self.show_folder_key)
                        .desired_width(width),
                );
                ui.checkbox(&mut self.show_folder_key, "Mostrar");
            });
            if first_time {
                ui.add_space(theme::S2);
                widgets::field_label(ui, "Repita a senha");
                ui.add(
                    egui::TextEdit::singleline(&mut self.folder_key_confirm)
                        .password(!self.show_folder_key)
                        .desired_width(ui.available_width().min(360.0)),
                );
                widgets::small_muted(ui, "Mínimo de 8 caracteres. A mesma senha vai ser pedida no outro computador. Anote-a em lugar seguro: sem ela, o arquivo é irrecuperável.");
            }
            ui.add_space(theme::S2);
            ui.checkbox(&mut self.draft.remember_folder_key, "Guardar a senha neste computador")
                .on_hover_text("Fica guardada neste computador, protegida pela sua conta do Windows. Desmarcada, vale só até fechar o Ponte.");
        });
        ui.add_space(theme::S3);

        if let Some(text) = self.replace_confirmation.clone() {
            soft_note(ui, Tone::Warn, icon::WARNING, &text);
            ui.horizontal(|ui| {
                if ui
                    .add(widgets::danger(
                        "Substituir o acervo deste computador e sincronizar",
                    ))
                    .clicked()
                {
                    self.allow_replace_next = true;
                    self.replace_confirmation = None;
                    self.request_full_sync();
                    *refresh_needed = self.refresh_client(store, db_path);
                }
                if ui.add(widgets::secondary("Cancelar")).clicked() {
                    self.replace_confirmation = None;
                }
            });
            ui.add_space(theme::S3);
        }

        ui.horizontal_wrapped(|ui| {
            let can_sync = !self.sync_in_flight && !self.draft.folder_files.is_empty();
            if ui
                .add_enabled(
                    can_sync,
                    widgets::primary(format!("{} Salvar e sincronizar agora", icon::SYNC)),
                )
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
                    None => match self.save_draft(store) {
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
            if ui.add(widgets::secondary("Salvar configuração")).clicked() {
                self.save_configuration(store, db_path, refresh_needed);
            }
            if dirty {
                widgets::pill(ui, "Não salvo", Tone::Warn);
            }
            if self.sync_in_flight {
                ui.spinner();
                widgets::small_muted(
                    ui,
                    if self.sync_step.is_empty() {
                        "sincronizando…"
                    } else {
                        self.sync_step.as_str()
                    },
                );
            }
        });
        if !self.last_summary.is_empty() {
            widgets::small_muted(ui, format!("Última sincronização: {}.", self.last_summary));
        }
        ui.add_space(theme::S2);
        egui::CollapsingHeader::new("Como funciona")
            .id_salt("folder-how")
            .show(ui, |ui| {
                widgets::small_muted(ui, "Espere a nuvem terminar de sincronizar antes de abrir o Ponte no outro computador. Sem a senha o arquivo não abre em lugar nenhum, nem para os autores do Ponte.");
                widgets::small_muted(ui, "O que você importa ou edita fica salvo neste computador e vai para as pastas ao sincronizar: ao abrir o Ponte, a cada 5 minutos, e em “Sincronizar agora”. Ao fechar, o Ponte avisa se algo ainda não foi enviado.");
            });
    }

    /// True while the form differs from the saved configuration (the
    /// bookkeeping the synchronization writes is left out).
    fn has_unsaved_draft(&self, store: &Store) -> bool {
        let Ok(saved) = store.load_intranet_config() else {
            return false;
        };
        let d = &self.draft;
        if d.mode != saved.mode || d.member_name.trim() != saved.member_name.trim() {
            return true;
        }
        // Only what the chosen mode uses counts (a never-saved configuration
        // draws a new random key at each load).
        match d.mode {
            IntranetMode::Local => false,
            IntranetMode::Host => {
                d.bind_addr != saved.bind_addr
                    || d.cabinet_token != saved.cabinet_token
                    || d.auto_start_host != saved.auto_start_host
            }
            IntranetMode::Client => {
                d.host_url != saved.host_url || d.cabinet_token != saved.cabinet_token
            }
            IntranetMode::SharedFolder => {
                d.folder_files != saved.folder_files
                    || d.remember_folder_key != saved.remember_folder_key
                    || (d.remember_folder_key && d.folder_key != saved.folder_key)
            }
        }
    }

    /// The card at the top: how this computer works now, from the SAVED
    /// configuration, its health, the last exchange and the one action.
    fn state_card(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        db_path: &std::path::Path,
        refresh_needed: &mut bool,
    ) {
        use crate::theme;
        use crate::widgets::{self, Tone};
        let Ok(saved) = store.load_intranet_config() else {
            return;
        };
        let conflicts = store.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        let outbox = store.list_offline_outbox().map(|v| v.len()).unwrap_or(0);
        let sentence = match saved.mode {
            IntranetMode::Local => "Só neste computador.".to_owned(),
            IntranetMode::Host => match &self.server {
                Some(server) => format!(
                    "Este é o computador principal — disponível em {}.",
                    server.local_addr()
                ),
                None => "Este é o computador principal — parado.".to_owned(),
            },
            IntranetMode::Client => {
                if self.offline {
                    "Sem conexão com o computador principal.".to_owned()
                } else if self.last_sync_finished.is_some() {
                    format!("Conectado ao computador principal em {}.", saved.host_url)
                } else {
                    format!("Usando o computador principal em {}.", saved.host_url)
                }
            }
            IntranetMode::SharedFolder => format!(
                "Pasta compartilhada: {}.",
                plural(saved.folder_paths().len(), "pasta", "pastas")
            ),
        };
        let (health, tone) = if conflicts > 0 {
            (
                plural(conflicts, "edição em conflito", "edições em conflito"),
                Tone::Danger,
            )
        } else if saved.mode == IntranetMode::Local {
            ("Só neste computador".to_owned(), Tone::Neutral)
        } else if outbox > 0 {
            (
                plural(outbox, "alteração a enviar", "alterações a enviar"),
                Tone::Warn,
            )
        } else if self.sync_in_flight {
            ("Sincronizando…".to_owned(), Tone::Info)
        } else if saved.mode == IntranetMode::Host {
            if self.server.is_some() {
                ("Disponível".to_owned(), Tone::Ok)
            } else {
                ("Parado".to_owned(), Tone::Warn)
            }
        } else if self.offline {
            let text = if saved.mode == IntranetMode::SharedFolder {
                "Pasta fora de alcance"
            } else {
                "Sem conexão"
            };
            (text.to_owned(), Tone::Warn)
        } else {
            ("Em dia".to_owned(), Tone::Ok)
        };
        let last = if saved.mode.queues_changes() && !saved.last_sync_at.trim().is_empty() {
            let when = crate::labels::datetime(&saved.last_sync_at);
            Some(match crate::labels::elapsed(&saved.last_sync_at) {
                Some(ago) => format!("Última sincronização: {when} ({ago})."),
                None => format!("Última sincronização: {when}."),
            })
        } else {
            None
        };
        widgets::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&sentence).font(theme::font_strong(theme::SIZE_BODY)),
                    )
                    .wrap(),
                );
                widgets::pill(ui, health, tone);
            });
            if let Some(last) = &last {
                widgets::small_muted(ui, last);
            }
            let can_sync = match saved.mode {
                IntranetMode::SharedFolder => {
                    !self.sync_in_flight && !saved.folder_files.is_empty()
                }
                IntranetMode::Client => !self.sync_in_flight,
                _ => false,
            };
            if saved.mode.queues_changes() {
                ui.add_space(theme::S2);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            can_sync,
                            widgets::secondary(format!("{} Sincronizar agora", theme::icon::SYNC)),
                        )
                        .on_hover_text("Sincroniza agora com o que está salvo na configuração.")
                        .clicked()
                    {
                        self.error = false;
                        self.message = "Sincronizando…".into();
                        self.request_full_sync();
                        *refresh_needed = self.refresh_client(store, db_path);
                    }
                    if self.sync_in_flight {
                        ui.spinner();
                        widgets::small_muted(
                            ui,
                            if self.sync_step.is_empty() {
                                "sincronizando…"
                            } else {
                                self.sync_step.as_str()
                            },
                        );
                    }
                });
            }
        });
        ui.add_space(theme::S3);
    }

    /// The "Salvar configuração" action, shared by every mode.
    fn save_configuration(
        &mut self,
        store: &mut Store,
        db_path: &std::path::Path,
        refresh_needed: &mut bool,
    ) {
        match self.save_draft(store) {
            Ok(()) => {
                self.error = false;
                self.message = "Configuração salva.".into();
                if self.draft.mode != IntranetMode::Host && self.server.is_some() {
                    self.server = None;
                    self.message
                        .push_str(" Este computador deixou de estar disponível para a equipe.");
                }
                if self.draft.mode.queues_changes() {
                    self.last_sync_request = None;
                    *refresh_needed = self.refresh_client(store, db_path);
                }
            }
            Err(e) => {
                self.error = true;
                self.message = e.to_string();
            }
        }
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
        store
            .offline_outbox_status()
            .map(|(count, _)| count)
            .unwrap_or(0)
    }

    pub fn host_running(&self) -> bool {
        self.server.is_some()
    }

    /// Non-blocking: collects the outcome of the previous background sync and,
    /// when due, posts the next request. Returns true when the library changed.
    pub fn refresh_client(&mut self, store: &mut Store, db_path: &std::path::Path) -> bool {
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
        let elapsed = self.last_sync_request.map(|at| at.elapsed());
        // After a failed exchange (folder out of reach, passphrase,
        // copy not arrived yet) the five-minute rhythm returns: retrying
        // would read the whole file again for nothing.
        let changes_waiting = cfg.mode == IntranetMode::SharedFolder
            && !self.offline
            && elapsed.is_some_and(|e| e >= FOLDER_CHANGE_MIN_GAP)
            && store.offline_outbox_status().is_ok_and(|(count, newest)| {
                count > 0
                    && chrono::DateTime::parse_from_rfc3339(&newest).is_ok_and(|t| {
                        chrono::Utc::now().signed_duration_since(t)
                            >= chrono::Duration::from_std(FOLDER_CHANGE_QUIET).unwrap_or_default()
                    })
            });
        let due = elapsed.is_none_or(|e| e >= interval) || changes_waiting;
        if cfg.mode == IntranetMode::SharedFolder
            && self.replace_confirmation.is_some()
            && !self.manual_sync
        {
            // Waiting for the person's decision: no automatic retries.
            return changed;
        }
        if !self.sync_in_flight && due {
            if self.sync.is_none() {
                match SyncWorker::start(db_path.to_path_buf(), self.repaint.clone()) {
                    Ok(worker) => self.sync = Some(worker),
                    Err(e) => {
                        // Counted as an attempt, so the next one waits too.
                        self.last_sync_request = Some(Instant::now());
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

    fn apply_sync_outcome(
        &mut self,
        store: &mut Store,
        cfg: &IntranetConfig,
        outcome: SyncOutcome,
    ) {
        let conflicts = store.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        let outbox = store.list_offline_outbox().map(|v| v.len()).unwrap_or(0);
        self.sync_step.clear();
        self.refresh_draft_bookkeeping(store);
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
                    self.status_summary =
                        format!("Pasta compartilhada • {} • {}", done.summary, when);
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
                        self.status_summary =
                            "Pasta compartilhada • aguardando sua confirmação (ver Ajustes ▸ Equipe e sincronização)"
                                .into();
                        self.error = false;
                        self.message.clear();
                    } else {
                        let mut extras = format!(
                            " • {} aguardando envio",
                            plural(outbox, "alteração", "alterações")
                        );
                        if conflicts > 0 {
                            extras.push_str(&format!(
                                " • ⚠ {}",
                                plural(conflicts, "conflito de edição", "conflitos de edição")
                            ));
                        }
                        self.status_summary =
                            format!("Pasta compartilhada fora de alcance{extras}");
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
                    suffix.push_str(&format!(
                        " • ⚠ {} para decidir",
                        plural(conflicts, "conflito de edição", "conflitos de edição")
                    ));
                }
                if outbox > 0 {
                    suffix.push_str(&format!(
                        " • {} a enviar",
                        plural(outbox, "alteração", "alterações")
                    ));
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
                let mut extras = format!(
                    " • trabalhando neste computador ({} aguardando envio)",
                    plural(outbox, "alteração", "alterações")
                );
                if conflicts > 0 {
                    extras.push_str(&format!(
                        " • ⚠ {}",
                        plural(conflicts, "conflito de edição", "conflitos de edição")
                    ));
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
            return "Acervo local • conteúdo protegido para este usuário do Windows".into();
        };
        let conflicts = store.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        let outbox = store.list_offline_outbox().map(|v| v.len()).unwrap_or(0);
        let mut badge = String::new();
        if conflicts > 0 {
            badge.push_str(&format!(
                " • ⚠ {} aguardando decisão (abra Ajustes ▸ Equipe e sincronização)",
                plural(conflicts, "edição em conflito", "edições em conflito")
            ));
        } else if outbox > 0 && cfg.mode == IntranetMode::Client {
            badge.push_str(&format!(
                " • {} a enviar quando o computador principal voltar a responder",
                plural(outbox, "alteração", "alterações")
            ));
        }
        match cfg.mode {
            IntranetMode::SharedFolder => {
                if !self.status_summary.is_empty() {
                    format!("{}{badge}", self.status_summary)
                } else {
                    let pending = if outbox > 0 {
                        format!(" • {} a enviar", plural(outbox, "alteração", "alterações"))
                    } else {
                        String::new()
                    };
                    format!(
                        "Pasta compartilhada ({}) • revisão {}{pending}{badge}",
                        plural(cfg.folder_paths().len(), "pasta", "pastas"),
                        cfg.synced_revision
                    )
                }
            }
            IntranetMode::Local => format!(
                "Acervo local ({}) • conteúdo protegido para este usuário do Windows{badge}",
                cfg.member_name
            ),
            IntranetMode::Host => {
                let rev = store.cabinet_revision().unwrap_or(0);
                if let Some(srv) = &self.server {
                    format!(
                        "Computador principal ativo ({}) • Integrante: {} • Revisão #{rev}{badge}",
                        srv.local_addr(),
                        cfg.member_name
                    )
                } else {
                    format!(
                        "Computador principal configurado (parado) • Integrante: {} • Revisão #{rev}{badge}",
                        cfg.member_name
                    )
                }
            }
            IntranetMode::Client => {
                if !self.status_summary.is_empty() {
                    self.status_summary.clone()
                } else {
                    format!(
                        "Computador principal configurado: {} • Integrante: {}{badge}",
                        cfg.host_url, cfg.member_name
                    )
                }
            }
        }
    }

    /// The team and synchronization settings in their own window.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        store: &mut Store,
        db_path: &std::path::Path,
    ) -> bool {
        if !self.open {
            return false;
        }
        let mut open = self.open;
        let mut refresh_needed = false;

        egui::Window::new("Equipe e sincronização")
            .open(&mut open)
            .default_size([720.0, 740.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("team-window-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        refresh_needed = self.ui(ui, store, db_path);
                    });
            });

        self.open = open;
        refresh_needed
    }

    /// The same content, drawn in place (the Ajustes area embeds it).
    /// Returns true when the library changed.
    pub fn ui(&mut self, ui: &mut egui::Ui, store: &mut Store, db_path: &std::path::Path) -> bool {
        use crate::theme::{self, icon};
        use crate::widgets::{self, Tone};
        if !self.loaded {
            if let Ok(cfg) = store.load_intranet_config() {
                self.draft = cfg;
                self.loaded = true;
            }
        }
        let mut refresh_needed = false;

        self.state_card(ui, store, db_path, &mut refresh_needed);
        let dirty = self.has_unsaved_draft(store);

        ui.horizontal_wrapped(|ui| {
            widgets::small_muted(ui, "A equipe trabalha no mesmo acervo usando um dos computadores do gabinete como principal, na rede interna.");
            crate::help_ui::help(ui, "Nada vai para a nuvem. Cada computador guarda a sua cópia protegida do acervo e conversa com o principal pela rede interna, com um código de acesso do gabinete.");
        });
        ui.add_space(theme::S2);

        widgets::card(ui, |ui| {
            widgets::field_label(ui, "Seu nome na equipe");
            ui.add(
                egui::TextEdit::singleline(&mut self.draft.member_name)
                    .desired_width(ui.available_width().min(420.0)),
            )
            .on_hover_text(
                "Aparece no histórico de alterações e como responsável ao assumir tarefas.",
            );
        });
        ui.add_space(theme::S3);

        widgets::card(ui, |ui| {
            widgets::section(ui, "Como este computador trabalha");
            ui.add_space(theme::S2);
            let modes = [
                (
                    IntranetMode::Local,
                    "Só neste computador",
                    "O Ponte usa só o acervo deste computador.",
                ),
                (
                    IntranetMode::Host,
                    "Computador principal",
                    "Os outros computadores se conectam a este.",
                ),
                (
                    IntranetMode::Client,
                    "Conectar ao computador principal",
                    "Usa o acervo do computador principal.",
                ),
                (
                    IntranetMode::SharedFolder,
                    "Pasta compartilhada",
                    "Meus computadores trocam o acervo por pastas.",
                ),
            ];
            let current = self.draft.mode;
            let mut chosen = None;
            if ui.available_width() >= 680.0 {
                for pair in modes.chunks(2) {
                    ui.columns(2, |columns| {
                        for (column, (mode, title, line)) in columns.iter_mut().zip(pair) {
                            if option_card(column, current == *mode, title, line) {
                                chosen = Some(*mode);
                            }
                        }
                    });
                    ui.add_space(theme::S1);
                }
            } else {
                for (mode, title, line) in modes {
                    if option_card(ui, current == mode, title, line) {
                        chosen = Some(mode);
                    }
                    ui.add_space(theme::S1);
                }
            }
            if let Some(mode) = chosen {
                self.draft.mode = mode;
            }
        });
        ui.add_space(theme::S3);

        match self.draft.mode {
            IntranetMode::SharedFolder => {
                self.shared_folder_section(ui, store, db_path, &mut refresh_needed);
            }
            IntranetMode::Local => {
                widgets::card(ui, |ui| {
                    widgets::section(ui, "Só neste computador");
                    ui.add_space(theme::S1);
                    ui.label("Neste modo, o Ponte opera apenas no acervo deste computador.");
                    if self.server.is_some() {
                        ui.add_space(theme::S2);
                        if ui
                            .add(widgets::secondary("Parar serviço do computador principal"))
                            .clicked()
                        {
                            self.server = None;
                            self.message = "Serviço do computador principal encerrado.".into();
                            self.error = false;
                        }
                    }
                });
                ui.add_space(theme::S3);
                ui.horizontal(|ui| {
                    if ui.add(widgets::primary("Salvar configuração")).clicked() {
                        self.save_configuration(store, db_path, &mut refresh_needed);
                    }
                    if dirty {
                        widgets::pill(ui, "Não salvo", Tone::Warn);
                    }
                });
            }
            IntranetMode::Host => {
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
                widgets::card(ui, |ui| {
                    widgets::section(ui, "Este computador como principal");
                    ui.add_space(theme::S2);
                    ui.checkbox(
                        &mut self.draft.auto_start_host,
                        "Ficar disponível para a equipe sempre que o Ponte abrir",
                    );
                    ui.add_space(theme::S2);
                    ui.horizontal_wrapped(|ui| {
                        if let Some(srv) = &self.server {
                            widgets::pill(
                                ui,
                                format!("Disponível para a equipe em {}", srv.local_addr()),
                                Tone::Ok,
                            );
                            if ui.add(widgets::secondary("Parar")).clicked() {
                                self.server = None;
                                self.message = "Este computador deixou de estar disponível para a equipe.".into();
                                self.error = false;
                            }
                        } else if ui.add(widgets::primary("Salvar e ficar disponível agora")).clicked() {
                            match self.save_draft(store) {
                                Ok(()) => match HostServer::start(db_path.to_path_buf(), &self.draft.bind_addr) {
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
                            .add(widgets::secondary(format!("{} Copiar código de conexão", icon::COPY)))
                            .on_hover_text("Um texto único com endereço e chave. Os colegas colam no Ponte deles em Ajustes ▸ Equipe e sincronização.")
                            .clicked()
                        {
                            let addr = hints.first().cloned().unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
                            let code = connection_code(&addr, &self.draft.cabinet_token);
                            ui.ctx().copy_text(format!(
                                "Código de conexão do Ponte (gabinete):\n{code}\n\nNo seu Ponte: Ajustes ▸ Equipe e sincronização ▸ “Conectar ao computador principal” ▸ cole o código."
                            ));
                            self.message = if self.draft.cabinet_token.trim() != saved_token.trim() && self.server.is_some() {
                                "Código copiado com a nova chave. Atenção: até você salvar, o computador principal ainda aceita só a chave anterior.".into()
                            } else {
                                "Código de conexão copiado. Envie aos colegas por um canal seguro.".into()
                            };
                            self.error = false;
                        }
                        if ui.add(widgets::secondary("Salvar configuração")).clicked() {
                            self.save_configuration(store, db_path, &mut refresh_needed);
                        }
                        if dirty {
                            widgets::pill(ui, "Não salvo", Tone::Warn);
                        }
                    });
                    if hints.len() > 1 {
                        ui.add_space(theme::S1);
                        widgets::small_muted(
                            ui,
                            format!(
                                "Endereços deste computador na rede: {}",
                                hints.join("  ou  ")
                            ),
                        );
                    }
                    ui.add_space(theme::S2);
                    soft_note(
                        ui,
                        Tone::Info,
                        icon::INFO,
                        &format!("Se os outros computadores não conseguirem conectar, peça ao suporte para liberar a porta {port} do Firewall do Windows neste computador, só para a rede interna."),
                    )
                    .on_hover_text("Comando para quem administra o computador:\nnetsh advfirewall firewall add rule name=\"Ponte\" dir=in action=allow protocol=TCP localport=PORTA profile=private,domain".replace("PORTA", &port.to_string()));
                    ui.add_space(theme::S2);
                    egui::CollapsingHeader::new("Mais opções")
                        .id_salt("host-advanced")
                        .show(ui, |ui| {
                            widgets::field_label(ui, "Endereço de escuta");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.draft.bind_addr)
                                    .desired_width(ui.available_width().min(360.0)),
                            )
                            .on_hover_text("IP:porta onde este computador atende a equipe. 0.0.0.0 significa todas as redes deste computador.");
                            ui.add_space(theme::S2);
                            widgets::field_label(ui, "Chave do gabinete");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.draft.cabinet_token)
                                        .password(!self.show_token)
                                        .desired_width(300.0),
                                );
                                ui.checkbox(&mut self.show_token, "Mostrar");
                                if ui.add(widgets::secondary("Gerar nova chave")).clicked() {
                                    self.draft.cabinet_token = generate_token();
                                }
                            });
                            if self.draft.cabinet_token.trim() != saved_token.trim() {
                                soft_note(
                                    ui,
                                    Tone::Warn,
                                    icon::WARNING,
                                    "Ao salvar a nova chave, todos os colegas precisarão colar um novo código de conexão.",
                                );
                            }
                        });
                    egui::CollapsingHeader::new("Como funciona")
                        .id_salt("host-how")
                        .show(ui, |ui| {
                            widgets::small_muted(ui, "Os outros computadores do gabinete se conectam a este pela rede interna, com o código de conexão.");
                            widgets::small_muted(ui, "Se este computador for desligado, qualquer outro já conectado pode assumir como principal. Cada computador conectado mantém uma cópia completa e protegida do acervo.");
                        });
                });
            }
            IntranetMode::Client => {
                widgets::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        widgets::section(ui, "Conectar ao computador principal");
                        crate::help_ui::help(ui, "O que você importa ou edita aqui vai para o computador principal. Se ele estiver desligado, fica salvo neste computador e é enviado quando a conexão voltar.");
                    });
                    ui.add_space(theme::S2);
                    widgets::field_label(ui, "Código de conexão");
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 130.0).max(160.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.connection_code_input)
                                .hint_text("cole aqui o código recebido (ponte://conectar?…)")
                                .desired_width(width),
                        );
                        if ui.add(widgets::secondary("Usar código")).clicked() {
                            match parse_connection_code(&self.connection_code_input) {
                                Some((host, key)) => {
                                    self.draft.host_url = host;
                                    self.draft.cabinet_token = key;
                                    self.connection_code_input.clear();
                                    match ping_host(&self.draft) {
                                        Ok((_, host_member, docs, _)) => {
                                            self.message = format!(
                                                "Código válido: computador principal de {host_member}, com {}. Clique em “Salvar e sincronizar acervo agora” para começar.",
                                                plural(docs, "documento", "documentos")
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
                                    self.message = "Código não reconhecido. Peça ao colega do computador principal para copiar de novo em Ajustes ▸ Equipe e sincronização ▸ “Copiar código de conexão”.".into();
                                    self.error = true;
                                }
                            }
                        }
                    });
                    ui.add_space(theme::S2);
                    egui::CollapsingHeader::new("Informar endereço e chave separadamente")
                        .id_salt("client-advanced")
                        .show(ui, |ui| {
                            widgets::field_label(ui, "Endereço do computador principal");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.draft.host_url)
                                    .desired_width(ui.available_width().min(360.0)),
                            )
                            .on_hover_text("Ex.: http://192.168.1.10:7878");
                            ui.add_space(theme::S2);
                            widgets::field_label(ui, "Chave do gabinete");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.draft.cabinet_token)
                                        .password(!self.show_token)
                                        .desired_width(300.0),
                                );
                                ui.checkbox(&mut self.show_token, "Mostrar");
                            });
                        });
                });
                ui.add_space(theme::S3);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !self.sync_in_flight,
                            widgets::primary(format!("{} Salvar e sincronizar acervo agora", icon::SYNC)),
                        )
                        .clicked()
                    {
                        match self.save_draft(store) {
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
                    if ui.add(widgets::secondary("Testar conexão")).clicked() {
                        let url = self.draft.host_url.clone();
                        match ping_host(&self.draft) {
                            Ok((_, host_member, docs, tasks)) => {
                                self.connection_test = Some((
                                    url,
                                    true,
                                    format!(
                                        "Conexão feita com o computador principal (de {host_member}: {} e {}).",
                                        plural(docs, "documento", "documentos"),
                                        plural(tasks, "tarefa", "tarefas")
                                    ),
                                ));
                                self.offline = false;
                            }
                            Err(e) => {
                                self.connection_test = Some((
                                    url,
                                    false,
                                    format!(
                                        "Não foi possível conectar: {}. Confira se o computador principal está ligado com o Ponte aberto, se o endereço e a chave estão certos e se os dois computadores estão na mesma rede.",
                                        e.to_string().trim_end_matches('.')
                                    ),
                                ));
                            }
                        }
                    }
                    if ui.add(widgets::secondary("Salvar configuração")).clicked() {
                        self.save_configuration(store, db_path, &mut refresh_needed);
                    }
                    if dirty {
                        widgets::pill(ui, "Não salvo", Tone::Warn);
                    }
                    if self.sync_in_flight {
                        ui.spinner();
                        widgets::small_muted(ui, "sincronizando…");
                    }
                });
                if let Some((url, ok, text)) = &self.connection_test {
                    if *url == self.draft.host_url {
                        ui.add_space(theme::S2);
                        let (tone, glyph) = if *ok {
                            (Tone::Ok, icon::CHECK)
                        } else {
                            (Tone::Danger, icon::ERROR)
                        };
                        soft_note(ui, tone, glyph, text);
                    }
                }
            }
        }

        if !self.message.is_empty() {
            ui.add_space(theme::S3);
            let tone = if self.error { Tone::Danger } else { Tone::Info };
            let (fg, bg) = tone.colors();
            let mut close = false;
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, icon::CLOSE, "Fechar o aviso").clicked() {
                            close = true;
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(if self.error {
                                    icon::ERROR
                                } else {
                                    icon::INFO
                                })
                                .size(18.0)
                                .color(fg),
                            );
                            ui.add(
                                egui::Label::new(egui::RichText::new(&self.message).color(fg))
                                    .wrap(),
                            );
                        });
                    });
                });
            if close {
                self.message.clear();
            }
        }

        let outbox = store.list_offline_outbox().unwrap_or_default();
        let conflicts = store.list_sync_conflicts().unwrap_or_default();

        if !conflicts.is_empty() {
            ui.add_space(theme::S3);
            let mut resolution_action: Option<(i64, ConflictResolution)> = None;
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::section(ui, "Edições em conflito");
                    widgets::pill(ui, conflicts.len().to_string(), Tone::Danger);
                });
                ui.add_space(theme::S1);
                ui.label(format!(
                    "{}: você alterou o item sem conexão e ele também foi alterado no computador principal.",
                    plural(conflicts.len(), "conflito de edição", "conflitos de edição")
                ));
                ui.add_space(theme::S2);
                egui::ScrollArea::vertical()
                    .id_salt("sync_conflicts_scroll")
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for conflict in &conflicts {
                            let differences = store.conflict_differences(conflict);
                            let both_sides = differences.iter().filter(|d| d.changed_by() == "pelos dois lados").count();
                            egui::Frame::new()
                                .fill(theme::c().sunken)
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new(format!("Conflito: {}", conflict.item_label))
                                            .font(theme::font_strong(theme::SIZE_BODY)),
                                    );
                                    widgets::small_muted(ui, format!(
                                        "Sua alteração de {} ({})",
                                        crate::labels::datetime(&conflict.created_at),
                                        conflict.actor
                                    ));
                                    ui.add_space(theme::S2);
                                    if differences.is_empty() {
                                        ui.label("As duas versões já coincidem campo a campo.");
                                        widgets::small_muted(ui, format!("Resumo da sua versão: {}", conflict.local_summary));
                                    } else {
                                        egui::Grid::new(("conflict-diff", conflict.id))
                                            .striped(true)
                                            .num_columns(4)
                                            .spacing([theme::S3, theme::S1])
                                            .max_col_width(220.0)
                                            .show(ui, |ui| {
                                                for head in ["Campo", "No computador principal", "Sua versão", "Alterado"] {
                                                    widgets::field_label(ui, head);
                                                }
                                                ui.end_row();
                                                for d in &differences {
                                                    ui.label(egui::RichText::new(&d.label).font(theme::font_strong(theme::SIZE_BODY)));
                                                    ui.add(egui::Label::new(if d.host.is_empty() { "(vazio)" } else { &d.host }).wrap());
                                                    ui.add(egui::Label::new(if d.local.is_empty() { "(vazio)" } else { &d.local }).wrap());
                                                    widgets::small_muted(ui, d.changed_by());
                                                    ui.end_row();
                                                }
                                            });
                                    }
                                    ui.add_space(theme::S2);
                                    let pending_here = self
                                        .pending_resolution
                                        .filter(|(id, _)| *id == conflict.id);
                                    match pending_here {
                                        None if conflict.unresolved => {
                                            widgets::small_muted(ui, "Esta alteração dependia de um item criado sem conexão que não chegou ao acervo da equipe. Ela não pode ser reaplicada: refaça-a, se ainda for necessária, e dispense o aviso.");
                                            ui.add_space(theme::S1);
                                            if ui.add(widgets::secondary("Dispensar o aviso")).clicked() {
                                                self.pending_resolution = Some((conflict.id, ConflictResolution::KeepHost));
                                            }
                                        }
                                        None => {
                                            ui.horizontal_wrapped(|ui| {
                                                if ui
                                                    .add(widgets::secondary("Combinar as duas versões…"))
                                                    .on_hover_text("Cada campo fica com o lado que o alterou. Textos alterados dos dois lados são juntados; nos demais campos alterados dos dois lados, sua versão prevalece e o valor do computador principal fica registrado no próprio item.")
                                                    .clicked()
                                                {
                                                    self.pending_resolution = Some((conflict.id, ConflictResolution::Merge));
                                                }
                                                if ui
                                                    .add(widgets::secondary("Usar só a minha versão…"))
                                                    .on_hover_text("Substitui a versão do computador principal pela sua em todos os campos.")
                                                    .clicked()
                                                {
                                                    self.pending_resolution = Some((conflict.id, ConflictResolution::KeepLocal));
                                                }
                                                if ui
                                                    .add(widgets::secondary("Usar só a do computador principal…"))
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
                                                        format!("Combinar: os campos alterados só de um lado ficam com esse lado ({}); os alterados dos dois lados ({both_sides}) ficam com a sua versão (textos são juntados) e o valor do computador principal é anotado no item.", differences.len() - both_sides)
                                                    } else {
                                                        "Combinar: cada campo fica com o lado que o alterou. Nenhum campo foi alterado dos dois lados.".to_owned()
                                                    }
                                                }
                                                ConflictResolution::KeepLocal => "Usar só a minha versão: o que está no computador principal será substituído.".to_owned(),
                                                ConflictResolution::KeepHost => "Usar só a do computador principal: a sua alteração será descartada.".to_owned(),
                                            };
                                            soft_note(ui, Tone::Warn, icon::WARNING, &description);
                                            ui.add_space(theme::S1);
                                            ui.horizontal(|ui| {
                                                if ui.add(widgets::primary("Confirmar")).clicked() {
                                                    resolution_action = Some((conflict.id, resolution));
                                                    self.pending_resolution = None;
                                                }
                                                if ui.add(widgets::secondary("Cancelar")).clicked() {
                                                    self.pending_resolution = None;
                                                }
                                            });
                                        }
                                    }
                                });
                            ui.add_space(theme::S2);
                        }
                    });
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
                                "Conflito resolvido: a versão do computador principal foi mantida."
                                    .into()
                            }
                        };
                        self.last_sync_request = None;
                        let _ = self.refresh_client(store, db_path);
                        refresh_needed = true;
                    }
                    Err(e) => {
                        self.error = true;
                        self.message = format!("Não foi possível resolver o conflito agora: {e}");
                    }
                }
            }
        }

        if !outbox.is_empty() {
            ui.add_space(theme::S3);
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::section(ui, "Alterações a enviar");
                    widgets::pill(ui, outbox.len().to_string(), Tone::Info);
                });
                ui.add_space(theme::S1);
                ui.label(format!(
                    "Alterações feitas neste computador enquanto o computador principal estava desligado ({}). Serão enviadas automaticamente quando a conexão voltar.",
                    outbox.len()
                ));
                ui.add_space(theme::S2);
                egui::ScrollArea::vertical()
                    .id_salt("offline_outbox_scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for (index, entry) in outbox.iter().enumerate() {
                            if index > 0 {
                                ui.separator();
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&entry.label);
                                widgets::small_muted(
                                    ui,
                                    format!(
                                        "{} por {}",
                                        crate::labels::datetime(&entry.created_at),
                                        entry.actor
                                    ),
                                );
                            });
                        }
                    });
            });
        }

        ui.add_space(theme::S3);
        widgets::card(ui, |ui| {
            ui.collapsing("Histórico recente de atividade da equipe", |ui| match store
                .list_team_events(30)
            {
                Ok(events) => {
                    if events.is_empty() {
                        widgets::muted(ui, "Nenhuma atividade registrada ainda.");
                    }
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for (index, ev) in events.into_iter().enumerate() {
                                if index > 0 {
                                    ui.separator();
                                }
                                ui.horizontal_wrapped(|ui| {
                                    widgets::small_muted(
                                        ui,
                                        crate::labels::datetime(&ev.created_at),
                                    );
                                    ui.label(
                                        egui::RichText::new(&ev.actor)
                                            .font(theme::font_strong(theme::SIZE_BODY)),
                                    );
                                    ui.label(&ev.details);
                                });
                            }
                        });
                }
                Err(e) => {
                    widgets::muted(ui, format!("Não foi possível ler o histórico: {e}"));
                }
            });
        });
        refresh_needed
    }
}

/// A short note in a soft-coloured frame: icon and text.
#[cfg(feature = "desktop")]
fn soft_note(
    ui: &mut egui::Ui,
    tone: crate::widgets::Tone,
    icon: &str,
    text: &str,
) -> egui::Response {
    let (fg, bg) = tone.colors();
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icon).size(18.0).color(fg));
                ui.add(egui::Label::new(egui::RichText::new(text).color(fg)).wrap());
            });
        })
        .response
}

/// One way of working: a card with a radio, the chosen one tinted and
/// outlined. Returns true when it was clicked.
#[cfg(feature = "desktop")]
fn option_card(ui: &mut egui::Ui, selected: bool, title: &str, line: &str) -> bool {
    use crate::theme;
    let p = theme::c();
    let (fill, stroke) = if selected {
        (p.primary_soft, egui::Stroke::new(1.5, p.primary))
    } else {
        (p.surface, egui::Stroke::new(1.0, p.outline))
    };
    let response = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(38.0);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                let center = egui::pos2(dot.center().x, dot.top() + 10.0);
                let ring = if selected { p.primary } else { p.muted };
                ui.painter()
                    .circle_stroke(center, 8.0, egui::Stroke::new(1.5, ring));
                if selected {
                    ui.painter().circle_filled(center, 4.5, p.primary);
                }
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(title).font(theme::font_strong(theme::SIZE_BODY)),
                        )
                        .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(line)
                                .size(theme::SIZE_SMALL)
                                .color(p.muted),
                        )
                        .selectable(false),
                    );
                });
            });
        })
        .response;
    // Reachable with Tab; Space or Enter chooses it, and the focus shows.
    let choice = ui
        .interact(
            response.rect,
            ui.auto_id_with(("team-mode", title)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if choice.has_focus() {
        crate::widgets::outline(ui, choice.rect.expand(2.0), p.primary);
    }
    choice.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::sample_capture;

    #[test]
    fn a_request_part_must_arrive_before_its_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut sender = TcpStream::connect(address).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        sender.write_all(b"POST").unwrap();
        let mut reader = DeadlineReader {
            stream: accepted,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        let mut buf = [0u8; 16];
        assert_eq!(reader.read(&mut buf).unwrap(), 4, "bytes in time are read");
        // Nothing else arrives: the read gives up when the part is due, not
        // after a fixed timeout per byte.
        reader.deadline = Instant::now() + Duration::from_millis(200);
        let started = Instant::now();
        let error = reader.read(&mut buf).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ),
            "{error:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        reader.deadline = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            reader.read(&mut buf).unwrap_err().kind(),
            std::io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn debug_output_of_the_configuration_hides_the_secrets() {
        let config = IntranetConfig {
            cabinet_token: "ponte-chave-secreta".into(),
            folder_key: "senha-da-pasta".into(),
            member_name: "SERVIDOR-A".into(),
            ..IntranetConfig::default()
        };
        let shown = format!("{config:?}");
        assert!(
            shown.contains("SERVIDOR-A") && shown.contains("<oculto>"),
            "{shown}"
        );
        assert!(
            !shown.contains("ponte-chave-secreta") && !shown.contains("senha-da-pasta"),
            "{shown}"
        );
    }

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
        let receipt = client_store.import_bundle(&capture, &[pdf_input]).unwrap();
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
            .add_attachment(
                owner,
                "despacho.txt",
                "foto do despacho",
                b"texto do despacho\n",
            )
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
        let target = TrashTarget {
            kind: TrashKind::Attachment,
            id: attachment_id,
        };
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

        synced_task.notes = "Complementado por Servidor A após revisão".into();
        synced_task.assignee = "MARIA_CLIENTE".into();
        client_store
            .save_task_checked(&synced_task, Some(new_ver))
            .unwrap();
        assert_eq!(
            host_store.list_tasks().unwrap()[0].notes,
            "Complementado por Servidor A após revisão"
        );

        // 3. Offline work while Host is stopped + Mutual Sync + Conflict Detection & Merge!
        server.stop();
        assert!(client_store.sync_from_host(false).is_err());

        // Client continues working normally while Host is offline:
        // (a) Client edits the document note twice while offline (coalesced in outbox keeping initial base version 0):
        client_store
            .save_note(
                receipt.document_id,
                "Primeira anotação offline de Servidor A",
            )
            .unwrap();
        client_store
            .save_note(
                receipt.document_id,
                "Parecer técnico elaborado por Servidor A offline",
            )
            .unwrap();
        assert_eq!(
            client_store
                .load_document(receipt.document_id)
                .unwrap()
                .note,
            "Parecer técnico elaborado por Servidor A offline"
        );
        // (b) Client creates a new task while offline (no conflict):
        let mut offline_new_task = Task::new(receipt.document_id);
        offline_new_task.title = "Diligência criada por Servidor A sem servidor".into();
        offline_new_task.assignee = "MARIA_CLIENTE".into();
        client_store.save_task(&offline_new_task).unwrap();
        assert_eq!(client_store.list_offline_outbox().unwrap().len(), 2);

        // Meanwhile, on the Host PC (before reconnecting), Servidor B also edits the same document's note!
        host_store
            .save_note(
                receipt.document_id,
                "Despacho urgente registrado por Servidor B no Anfitrião",
            )
            .unwrap();

        // Now Host comes back online!
        let mut server2 = HostServer::start(host_db_path.clone(), "127.0.0.1:0").unwrap();
        let mut reconnected_cfg = client_store.load_intranet_config().unwrap();
        reconnected_cfg.host_url = format!("http://{}", server2.local_addr());
        client_store.save_intranet_config(&reconnected_cfg).unwrap();

        // Client triggers mutual sync (`sync_from_host`):
        // - `offline_new_task` has no conflict and is pushed to Host automatically!
        // - `save_note` collides with Servidor B's concurrent edit on Host and is moved to `offline_conflicts`!
        assert!(client_store.sync_from_host(false).unwrap());
        assert_eq!(client_store.list_offline_outbox().unwrap().len(), 0);
        assert_eq!(host_store.list_tasks().unwrap().len(), 2);
        assert!(host_store
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Diligência criada por Servidor A sem servidor"));

        let conflicts = client_store.list_sync_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].item_label.contains("Notas do documento"));
        assert!(conflicts[0]
            .local_summary
            .contains("Parecer técnico elaborado por Servidor A offline"));
        assert!(conflicts[0]
            .host_summary
            .contains("Despacho urgente registrado por Servidor B no Anfitrião"));

        // Servidor A chooses to MERGE both versions (`ConflictResolution::Merge`):
        client_store
            .resolve_sync_conflict(conflicts[0].id, ConflictResolution::Merge)
            .unwrap();
        assert_eq!(client_store.list_sync_conflicts().unwrap().len(), 0);

        let merged_host_note = host_store.load_document(receipt.document_id).unwrap().note;
        let merged_client_note = client_store
            .load_document(receipt.document_id)
            .unwrap()
            .note;
        assert_eq!(merged_host_note, merged_client_note);
        assert!(
            merged_host_note.contains("Despacho urgente registrado por Servidor B no Anfitrião")
        );
        assert!(merged_host_note.contains("Parecer técnico elaborado por Servidor A offline"));
        server2.stop();

        // 4. Contingency failover: Client switches to Host mode and continues seamlessly!
        let mut failover_cfg = client_store.load_intranet_config().unwrap();
        failover_cfg.mode = IntranetMode::Host;
        failover_cfg.bind_addr = "127.0.0.1:0".into();
        client_store.save_intranet_config(&failover_cfg).unwrap();
        client_store
            .save_note(
                receipt.document_id,
                "Nota gravada após assumir como Anfitrião",
            )
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
                "Complementado por Servidor A",
                "Parecer técnico elaborado por Servidor A offline",
                "Diligência criada por Servidor A sem servidor",
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
    fn a_folder_copy_overwritten_from_an_old_base_heals_from_the_guard_copy() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let files = vec![drive.display().to_string()];
        let reporter = crate::ai::Reporter::silent();
        let key = "senha-de-teste-123";
        let sync = |store: &mut Store| {
            let cfg = store.load_intranet_config().unwrap();
            store.sync_with_folders(&cfg, key, false, &reporter)
        };

        // A: two documents and the first copy; B joins.
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", files.clone()))
            .unwrap();
        let one = a.import_capture(&crate::model::sample_capture()).unwrap();
        let mut other = crate::model::sample_capture();
        other.request_id = uuid::Uuid::new_v4().to_string();
        other.document_number = "SEFAZ-PRO-2026/00999".into();
        other.source_url = other.source_url.replace("00001", "00999");
        let two = a.import_capture(&other).unwrap();
        sync(&mut a).unwrap();
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Celular", files.clone()))
            .unwrap();
        sync(&mut b).unwrap();
        let old_copy = std::fs::read(&drive).unwrap();

        // 1. Same revision: A writes a note; B, from a cached (old) copy,
        // writes another one over it.
        a.save_note(one.document_id, "Nota de A").unwrap();
        sync(&mut a).unwrap();
        std::fs::write(&drive, &old_copy).unwrap();
        b.save_note(two.document_id, "Nota de B").unwrap();
        sync(&mut b).unwrap();
        let report = sync(&mut a).unwrap();
        assert_eq!(
            report.conflicts,
            0,
            "{:?}",
            a.list_sync_conflicts().unwrap()
        );
        assert_eq!(a.load_document(one.document_id).unwrap().note, "Nota de A");
        assert_eq!(a.load_document(two.document_id).unwrap().note, "Nota de B");
        sync(&mut b).unwrap();
        assert_eq!(
            b.load_document(one.document_id).unwrap().note,
            "Nota de A",
            "A's note came back to the folder"
        );

        // 2. Two revisions behind: A writes twice, B overwrites from an even
        // older copy. Before, A waited forever; now it merges.
        let older = std::fs::read(&drive).unwrap();
        a.save_note(one.document_id, "A, segunda").unwrap();
        sync(&mut a).unwrap();
        a.save_note(one.document_id, "A, terceira").unwrap();
        sync(&mut a).unwrap();
        std::fs::write(&drive, &older).unwrap();
        // B's own guard copy is as old as that file here.
        b.save_note(two.document_id, "B, segunda").unwrap();
        sync(&mut b).unwrap();
        sync(&mut a).unwrap();
        assert_eq!(
            a.load_document(one.document_id).unwrap().note,
            "A, terceira"
        );
        assert_eq!(a.load_document(two.document_id).unwrap().note, "B, segunda");

        // 3. Only the cloud lagging: the folder shows an older copy with
        // nothing new in it. A waits, as before.
        let lagging = std::fs::read(&drive).unwrap();
        a.save_note(one.document_id, "A, quarta").unwrap();
        sync(&mut a).unwrap();
        std::fs::write(&drive, &lagging).unwrap();
        let error = sync(&mut a).unwrap_err().to_string();
        assert!(error.contains("ainda não recebeu"), "{error}");
        assert_eq!(a.load_document(one.document_id).unwrap().note, "A, quarta");
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
        a.save_intranet_config(&folder_config("Casa", files.clone()))
            .unwrap();
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
        assert_eq!(
            a.list_offline_outbox().unwrap().len(),
            3,
            "every change queued for the folders"
        );
        let report = a.sync_with_folders(&cfg_a, key, false, &reporter).unwrap();
        assert!(report.created);
        assert_eq!(
            report.written_to.len(),
            1,
            "the pen drive folder does not exist yet"
        );
        assert_eq!(report.unreachable.len(), 1);
        assert!(a.list_offline_outbox().unwrap().is_empty());
        let header = crate::sharedfolder::read_header(&drive).unwrap().unwrap();
        assert!(header.revision > 0);
        let cfg_a = a.load_intranet_config().unwrap();
        assert_eq!(cfg_a.synced_revision, header.revision);
        assert!(!cfg_a.folder_lineage.is_empty());

        // Computer B joins with an empty library: receives everything.
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", files.clone()))
            .unwrap();
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
        assert_eq!(
            b.attachment_bytes(attachments[0].id).unwrap().1,
            b"%PDF-1.4 anexo sintetico"
        );
        assert!(
            b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
                .unwrap()
                .written_to
                .is_empty(),
            "up to date: nothing rewritten"
        );

        // Wrong password and a copy of another library are refused.
        assert!(b
            .sync_with_folders(&cfg_b, "outra-senha-999", false, &reporter)
            .is_err());

        // Both edit the same note; B also adds a task. B syncs first.
        b.save_note(receipt.document_id, "Nota inicial\nAcréscimo do trabalho")
            .unwrap();
        let mut task = crate::tasks::Task::new(receipt.document_id);
        task.title = "Tarefa criada no trabalho".into();
        b.save_task(&task).unwrap();
        a.save_note(receipt.document_id, "Nota inicial\nAcréscimo de casa")
            .unwrap();
        let cfg_b = b.load_intranet_config().unwrap();
        let report = b.sync_with_folders(&cfg_b, key, false, &reporter).unwrap();
        assert_eq!(report.pushed, 2);
        let revision_after_b = crate::sharedfolder::read_header(&drive)
            .unwrap()
            .unwrap()
            .revision;
        assert!(revision_after_b > header.revision);

        // A syncs: its note conflicts, the task arrives, the file is rewritten.
        let cfg_a = a.load_intranet_config().unwrap();
        let report = a.sync_with_folders(&cfg_a, key, false, &reporter).unwrap();
        assert_eq!(report.conflicts, 1);
        assert!(a
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Tarefa criada no trabalho"));
        assert_eq!(
            a.load_document(receipt.document_id).unwrap().note,
            "Nota inicial\nAcréscimo do trabalho"
        );
        let conflicts = a.list_sync_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        a.resolve_sync_conflict(conflicts[0].id, ConflictResolution::Merge)
            .unwrap();
        let merged = a.load_document(receipt.document_id).unwrap().note;
        assert!(
            merged.contains("Acréscimo do trabalho") && merged.contains("Acréscimo de casa"),
            "{merged}"
        );
        let cfg_a = a.load_intranet_config().unwrap();
        let report = a.sync_with_folders(&cfg_a, key, false, &reporter).unwrap();
        assert_eq!(report.pushed, 1);
        assert!(report.conflicts == 0);

        // B receives the merged note; a second folder appears and catches up.
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let cfg_b = b.load_intranet_config().unwrap();
        let report = b.sync_with_folders(&cfg_b, key, false, &reporter).unwrap();
        assert_eq!(b.load_document(receipt.document_id).unwrap().note, merged);
        assert!(
            report.written_to.iter().any(|p| p.contains("Pendrive")),
            "{:?}",
            report.written_to
        );
        let drive_header = crate::sharedfolder::read_header(&drive).unwrap().unwrap();
        let pen_header = crate::sharedfolder::read_header(&pen).unwrap().unwrap();
        assert_eq!(drive_header.revision, pen_header.revision);

        // Diverged copies: A writes only to the pen drive, B only to the drive;
        // the next full sync reconciles both changes without losing either.
        let mut a_pen_only = a.load_intranet_config().unwrap();
        a_pen_only.folder_files = vec![pen.display().to_string()];
        a.save_intranet_config(&a_pen_only).unwrap();
        a.save_note(receipt.document_id, &format!("{merged}\nSó pelo pendrive"))
            .unwrap();
        a.sync_with_folders(&a_pen_only, key, false, &reporter)
            .unwrap();
        let mut b_drive_only = b.load_intranet_config().unwrap();
        b_drive_only.folder_files = vec![drive.display().to_string()];
        b.save_intranet_config(&b_drive_only).unwrap();
        let mut other = crate::tasks::Task::new(receipt.document_id);
        other.title = "Tarefa só pelo Drive".into();
        b.save_task(&other).unwrap();
        b.sync_with_folders(&b_drive_only, key, false, &reporter)
            .unwrap();
        let mut b_both = b.load_intranet_config().unwrap();
        b_both.folder_files = files.clone();
        b.save_intranet_config(&b_both).unwrap();
        let report = b.sync_with_folders(&b_both, key, false, &reporter).unwrap();
        assert_eq!(report.merged_foreign, 1, "the pen drive's note reaches B");
        assert!(b
            .load_document(receipt.document_id)
            .unwrap()
            .note
            .ends_with("Só pelo pendrive"));
        assert!(b
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Tarefa só pelo Drive"));
        assert_eq!(report.written_to.len(), 2);

        // A joining computer with its own documents must confirm the replacement.
        let mut c = Store::open(&dir.path().join("c.sqlite3")).unwrap();
        let mut own = crate::model::sample_capture();
        own.document_number = "SEFAZ-PRO-2026/00099".into();
        own.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00099"
                .into();
        c.import_capture(&own).unwrap();
        c.save_intranet_config(&folder_config("Notebook", files.clone()))
            .unwrap();
        let cfg_c = c.load_intranet_config().unwrap();
        let error = c
            .sync_with_folders(&cfg_c, key, false, &reporter)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("[confirmar-substituicao]"), "{error}");
        c.sync_with_folders(&cfg_c, key, true, &reporter).unwrap();
        assert_eq!(c.list_documents("").unwrap().len(), 1);

        // A computer whose bookkeeping was lost, but which wrote a copy
        // itself, is not joining: it reconciles without the question.
        c.save_note(receipt.document_id, "Nota gravada por C")
            .unwrap();
        let cfg_c = c.load_intranet_config().unwrap();
        c.sync_with_folders(&cfg_c, key, false, &reporter).unwrap();
        let mut forgotten = c.load_intranet_config().unwrap();
        forgotten.synced_revision = 0;
        forgotten.folder_lineage.clear();
        c.save_intranet_config(&forgotten).unwrap();
        c.save_note(receipt.document_id, "Nota depois do esquecimento")
            .unwrap();
        c.sync_with_folders(&forgotten, key, false, &reporter)
            .unwrap();
        assert!(c.load_intranet_config().unwrap().synced_revision > 0);
        assert!(c
            .load_document(receipt.document_id)
            .unwrap()
            .note
            .ends_with("esquecimento"));

        // Nothing readable in the transport files.
        for path in [&drive, &pen] {
            let raw = std::fs::read(path).unwrap();
            assert!(!raw.windows(5).any(|w| w == b"Nota "));
            assert!(!raw.windows(8).any(|w| w == b"sintetico"));
        }
    }

    fn manual_attendance(reference: &str, subject: &str) -> crate::saiop::SaiopAttendance {
        let mut att = crate::saiop::SaiopAttendance::new_manual();
        att.reference_raw = reference.into();
        att.reference_norm = crate::saiop::normalize_reference(reference);
        att.subject = subject.into();
        att
    }

    #[test]
    fn changes_to_items_created_offline_follow_the_numbers_the_team_gave_them() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let files = vec![drive.display().to_string(), pen.display().to_string()];
        let reporter = crate::ai::Reporter::silent();
        let key = "senha-de-teste-123";
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", files.clone()))
            .unwrap();
        let receipt = a.import_capture(&sample_capture()).unwrap();
        a.sync_with_folders(&a.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", files.clone()))
            .unwrap();
        b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();

        // Both create a task and an atendimento before exchanging: each
        // library gives them the same numbers.
        let mut from_b = Task::new(receipt.document_id);
        from_b.title = "Tarefa de B".into();
        let b_task = b.save_task(&from_b).unwrap();
        let b_att = b
            .save_saiop_attendance(&manual_attendance("PROC-B", "Atendimento de B"), None)
            .unwrap();
        let mut from_a = Task::new(receipt.document_id);
        from_a.title = "Tarefa de A".into();
        let a_task = a.save_task(&from_a).unwrap();
        let a_att = a
            .save_saiop_attendance(&manual_attendance("PROC-A", "Atendimento de A"), None)
            .unwrap();
        assert_eq!(
            (a_task, a_att),
            (b_task, b_att),
            "the scenario needs colliding numbers"
        );
        // A keeps working on what it created: edits, an attachment, a link
        // between the new task and the new atendimento.
        let mut edited = a
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == a_task)
            .unwrap();
        edited.title = "Tarefa de A, revista".into();
        a.save_task(&edited).unwrap();
        a.add_attachment(
            crate::attachments::Owner::Task(a_task),
            "nota.txt",
            "",
            b"anexo de A",
        )
        .unwrap();
        let mut att = a
            .list_saiop_attendances()
            .unwrap()
            .into_iter()
            .find(|x| x.id == a_att)
            .unwrap();
        att.observation = "Observação de A".into();
        att.task_ids = vec![a_task];
        a.save_saiop_attendance(&att, None).unwrap();

        // B exchanges first; then A.
        b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        let report = a
            .sync_with_folders(&a.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        assert_eq!(
            report.conflicts,
            0,
            "{:?}",
            a.list_sync_conflicts().unwrap()
        );
        b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();

        for store in [&a, &b] {
            let tasks = store.list_tasks().unwrap();
            let b_side = tasks.iter().find(|t| t.title == "Tarefa de B").unwrap();
            let a_side = tasks
                .iter()
                .find(|t| t.title == "Tarefa de A, revista")
                .expect("A's edit reached A's task");
            assert_ne!(a_side.id, b_side.id);
            assert!(!tasks.iter().any(|t| t.title == "Tarefa de A"));
            assert!(store
                .list_attachments(crate::attachments::Owner::Task(b_side.id))
                .unwrap()
                .is_empty());
            assert_eq!(
                store
                    .list_attachments(crate::attachments::Owner::Task(a_side.id))
                    .unwrap()
                    .len(),
                1
            );
            let atts = store.list_saiop_attendances().unwrap();
            let of_b = atts
                .iter()
                .find(|x| x.subject == "Atendimento de B")
                .unwrap();
            let of_a = atts
                .iter()
                .find(|x| x.subject == "Atendimento de A")
                .unwrap();
            assert!(of_b.observation.is_empty(), "B's atendimento untouched");
            assert_eq!(of_a.observation, "Observação de A");
            assert_eq!(of_a.task_ids, vec![a_side.id]);
        }

        // Diverged copies. A works with the pen drive only: creates a task
        // and edits it, and that copy is what A now matches. B works with the
        // Drive only and makes more changes, so the Drive copy wins (newest)
        // and A's pen drive changes come in as changes of another copy. A
        // then edits its task again and exchanges with both folders: its
        // numbers refer to the pen drive copy, not to the one being built.
        let mut a_pen = a.load_intranet_config().unwrap();
        a_pen.folder_files = vec![pen.display().to_string()];
        a.save_intranet_config(&a_pen).unwrap();
        let mut pen_task = Task::new(receipt.document_id);
        pen_task.title = "Pelo pendrive".into();
        let pen_id = a.save_task(&pen_task).unwrap();
        let find = |store: &Store, id: i64| {
            store
                .list_tasks()
                .unwrap()
                .into_iter()
                .find(|t| t.id == id)
                .unwrap()
        };
        let mut pen_task = find(&a, pen_id);
        pen_task.notes = "Editada sem conexão".into();
        a.save_task(&pen_task).unwrap();
        a.sync_with_folders(&a_pen, key, false, &reporter).unwrap();
        let mut b_drive = b.load_intranet_config().unwrap();
        b_drive.folder_files = vec![drive.display().to_string()];
        b.save_intranet_config(&b_drive).unwrap();
        let mut drive_task = Task::new(receipt.document_id);
        drive_task.title = "Pelo Drive".into();
        assert_eq!(b.save_task(&drive_task).unwrap(), pen_id);
        b.save_note(receipt.document_id, "Nota pelo Drive").unwrap();
        b.save_note(receipt.document_id, "Nota pelo Drive, revista")
            .unwrap();
        let mut second = Task::new(receipt.document_id);
        second.title = "Outra pelo Drive".into();
        b.save_task(&second).unwrap();
        b.sync_with_folders(&b_drive, key, false, &reporter)
            .unwrap();
        assert!(
            crate::sharedfolder::read_header(&drive)
                .unwrap()
                .unwrap()
                .revision
                > crate::sharedfolder::read_header(&pen)
                    .unwrap()
                    .unwrap()
                    .revision,
            "the scenario needs the Drive copy to win"
        );
        // Changing the folders before editing must not lose track of the copy
        // A matches.
        let mut a_both = a.load_intranet_config().unwrap();
        a_both.folder_files = files.clone();
        a.save_intranet_config(&a_both).unwrap();
        let mut pen_task = find(&a, pen_id);
        pen_task.outcome = "Concluída depois".into();
        a.save_task(&pen_task).unwrap();
        let report = a.sync_with_folders(&a_both, key, false, &reporter).unwrap();
        assert_eq!(
            report.merged_foreign, 2,
            "creation and edit from the pen drive"
        );
        assert_eq!(
            report.conflicts,
            0,
            "{:?}",
            a.list_sync_conflicts().unwrap()
        );
        b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        for store in [&a, &b] {
            let tasks = store.list_tasks().unwrap();
            let drive_side = tasks.iter().find(|t| t.title == "Pelo Drive").unwrap();
            let pen_side = tasks.iter().find(|t| t.title == "Pelo pendrive").unwrap();
            assert_ne!(drive_side.id, pen_side.id);
            assert!(drive_side.notes.is_empty(), "B's task untouched");
            assert!(drive_side.outcome.is_empty(), "B's task untouched");
            assert_eq!(pen_side.notes, "Editada sem conexão");
            assert_eq!(pen_side.outcome, "Concluída depois");
        }
    }

    #[test]
    fn an_offline_client_s_new_items_get_the_main_computer_s_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let host_db_path = dir.path().join("host.sqlite3");
        let token = generate_token();
        let mut host = Store::open(&host_db_path).unwrap();
        host.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "ANFITRIAO".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: "http://127.0.0.1:7878".into(),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        })
        .unwrap();
        let mut server = HostServer::start(host_db_path.clone(), "127.0.0.1:0").unwrap();
        let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
        let mut client_cfg = IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "SERVIDOR-A".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: format!("http://{}", server.local_addr()),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        };
        client.save_intranet_config(&client_cfg).unwrap();
        let receipt = client.import_capture(&sample_capture()).unwrap();
        let mut second = sample_capture();
        second.document_number = "SEFAZ-PRO-2026/00777".into();
        second.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00777"
                .into();
        second.request_id = uuid::Uuid::new_v4().to_string();
        let receipt2 = client.import_capture(&second).unwrap();
        server.stop();

        // Offline: a new task, its edit and an attachment to it.
        let mut task = Task::new(receipt.document_id);
        task.title = "Tarefa de Servidor A".into();
        let local_id = client.save_task(&task).unwrap();
        let mut task = client
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == local_id)
            .unwrap();
        task.notes = "Editada sem conexão".into();
        client.save_task(&task).unwrap();
        client
            .add_attachment(
                crate::attachments::Owner::Task(local_id),
                "nota.txt",
                "",
                b"anexo de Servidor A",
            )
            .unwrap();
        // Meanwhile the main computer creates a task with the same number.
        let mut hosts = Task::new(receipt.document_id);
        hosts.title = "Tarefa do anfitrião".into();
        assert_eq!(host.save_task(&hosts).unwrap(), local_id);

        // Back online: everything lands on the right items.
        let mut server = HostServer::start(host_db_path.clone(), "127.0.0.1:0").unwrap();
        client_cfg = client.load_intranet_config().unwrap();
        client_cfg.host_url = format!("http://{}", server.local_addr());
        client.save_intranet_config(&client_cfg).unwrap();
        client.sync_from_host(false).unwrap();
        assert!(client.list_offline_outbox().unwrap().is_empty());
        assert!(client.list_sync_conflicts().unwrap().is_empty());
        let tasks = host.list_tasks().unwrap();
        let of_host = tasks
            .iter()
            .find(|t| t.title == "Tarefa do anfitrião")
            .unwrap();
        let of_maria = tasks
            .iter()
            .find(|t| t.title == "Tarefa de Servidor A")
            .unwrap();
        assert!(of_host.notes.is_empty(), "the host's task untouched");
        assert_eq!(of_maria.notes, "Editada sem conexão");
        assert!(host
            .list_attachments(crate::attachments::Owner::Task(of_host.id))
            .unwrap()
            .is_empty());
        assert_eq!(
            host.list_attachments(crate::attachments::Owner::Task(of_maria.id))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(client.list_tasks().unwrap().len(), tasks.len());

        // A refused creation turns the changes that depend on it into a
        // notice that cannot be applied, never into an edit of another item.
        server.stop();
        let mut doomed = Task::new(receipt2.document_id);
        doomed.title = "Tarefa recusada".into();
        let doomed_id = client.save_task(&doomed).unwrap();
        let mut doomed = client
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == doomed_id)
            .unwrap();
        doomed.notes = "Edição órfã".into();
        client.save_task(&doomed).unwrap();
        host.trash_item(TrashTarget {
            kind: TrashKind::Document,
            id: receipt2.document_id,
        })
        .unwrap();
        let count = |store: &Store| -> i64 {
            store
                .conn
                .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
                .unwrap()
        };
        let before = count(&host);
        let mut server = HostServer::start(host_db_path.clone(), "127.0.0.1:0").unwrap();
        client_cfg = client.load_intranet_config().unwrap();
        client_cfg.host_url = format!("http://{}", server.local_addr());
        client.save_intranet_config(&client_cfg).unwrap();
        client.sync_from_host(false).unwrap();
        let conflicts = client.list_sync_conflicts().unwrap();
        assert_eq!(conflicts.len(), 2, "{conflicts:?}");
        let orphan = conflicts
            .iter()
            .find(|c| c.unresolved)
            .expect("orphan edit");
        assert!(client
            .resolve_sync_conflict(orphan.id, ConflictResolution::KeepLocal)
            .is_err());
        client
            .resolve_sync_conflict(orphan.id, ConflictResolution::KeepHost)
            .unwrap();
        assert_eq!(count(&host), before, "nothing created nor edited there");
        assert!(host
            .list_tasks()
            .unwrap()
            .iter()
            .all(|t| t.notes != "Edição órfã"));
        server.stop();
    }

    #[test]
    fn a_silent_peer_does_not_hold_the_main_computer() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("host.sqlite3");
        let token = generate_token();
        let mut host = Store::open(&db).unwrap();
        host.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "ANFITRIAO".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: "http://127.0.0.1:7878".into(),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        })
        .unwrap();
        let mut server = HostServer::start(db.clone(), "127.0.0.1:0").unwrap();
        let cfg = IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "SERVIDOR-A".into(),
            host_url: format!("http://{}", server.local_addr()),
            cabinet_token: token,
            ..IntranetConfig::default()
        };
        let wait_for = |count: usize| {
            let started = Instant::now();
            while server.slots.in_use() != count && started.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(server.slots.in_use(), count);
        };

        // Connections that never send anything do not delay another request.
        let mut idle: Vec<TcpStream> = (1..MAX_CONNECTIONS_PER_PEER)
            .map(|_| TcpStream::connect(server.local_addr()).unwrap())
            .collect();
        wait_for(MAX_CONNECTIONS_PER_PEER - 1);
        let started = Instant::now();
        ping_host(&cfg).unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));

        // With every slot of this computer taken, a request is told to come
        // back later: a change would wait in the queue, not become a conflict.
        idle.push(TcpStream::connect(server.local_addr()).unwrap());
        wait_for(MAX_CONNECTIONS_PER_PEER);
        match send_rpc_classified(&cfg, &RpcRequest::Ping) {
            Err(RpcError::Unreachable(message)) => {
                assert!(message.contains("muitas conexões"), "{message}")
            }
            Err(RpcError::Rejected(message)) => panic!("refused: {message}"),
            Ok(_) => panic!("served beyond the limit"),
        }

        // Closed connections give their slots back.
        drop(idle);
        wait_for(0);
        ping_host(&cfg).unwrap();
        server.stop();
    }

    #[test]
    fn a_change_queued_during_a_synchronization_keeps_the_library_it_was_made_on() {
        let dir = tempfile::tempdir().unwrap();
        let team = Store::open(&dir.path().join("team.sqlite3")).unwrap();
        let copy = team.export_snapshot().unwrap();
        let mut here = Store::open(&dir.path().join("here.sqlite3")).unwrap();
        let folder = dir.path().join("ponte-sync.ponte").display().to_string();
        here.save_intranet_config(&folder_config("Casa", vec![folder]))
            .unwrap();
        let receipt = here.import_capture(&sample_capture()).unwrap();
        let queued: Vec<(i64, String)> = here
            .list_offline_outbox()
            .unwrap()
            .iter()
            .map(|e| (e.id, e.uid.clone()))
            .collect();
        assert_eq!(queued.len(), 1);
        let replace = |store: &mut Store, sent: &[(i64, String)]| {
            store.apply_snapshot_and(
                &copy,
                |_, _| unreachable!(),
                |_| unreachable!(),
                |tx| ensure_queue_settled(tx, sent),
            )
        };
        // The import was queued after the exchange listed the queue (empty).
        let error = replace(&mut here, &[]).unwrap_err().to_string();
        assert_eq!(error, QUEUE_GREW);
        assert_eq!(here.list_documents("").unwrap().len(), 1, "rolled back");
        assert!(here.load_document(receipt.document_id).is_ok());
        // A row rewritten while it was sent (new uid) is neither settled
        // nor deleted: it holds a newer edit.
        let (id, uid) = queued[0].clone();
        delete_sent_outbox(&here.conn, id, "outra-versao").unwrap();
        assert_eq!(here.offline_outbox_status().unwrap().0, 1);
        assert_eq!(
            replace(&mut here, &[(id, "outra-versao".into())])
                .unwrap_err()
                .to_string(),
            QUEUE_GREW
        );
        // Once it was among the changes sent, the replacement goes through.
        replace(&mut here, &queued).unwrap();
        let _ = uid;
        assert!(here.list_documents("").unwrap().is_empty());
    }

    #[test]
    fn work_templates_and_generation_records_travel_with_the_library() {
        use crate::work_templates::{
            BlockMode, BlockRef, Example, ExampleStatus, GenerationRecord, InstructionBlock,
            Outcome, Scope, Status, UsedBlock, WorkTemplate,
        };
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let files = vec![drive.display().to_string()];
        let reporter = crate::ai::Reporter::silent();
        let key = "senha-de-teste-123";

        // A publishes a shared template with a mandatory block and an
        // approved example, generates a draft with it and saves the draft.
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", files.clone()))
            .unwrap();
        let mut block = InstructionBlock::new("Linguagem", "estilo", "Use frases curtas.");
        block.scope = Scope::Shared;
        let block = a.save_block(&block, None).unwrap();
        a.set_block_status(&block.uid, Status::Published, None)
            .unwrap();
        let mut template =
            WorkTemplate::new("Ofício padrão", "resposta", "Ref. {{numero_documento}}");
        template.scope = Scope::Shared;
        template.versions[0].block_refs.push(BlockRef {
            block_uid: block.uid.clone(),
            version: None,
            mode: BlockMode::Mandatory,
        });
        let template = a.save_template(&template, None).unwrap();
        let template = a
            .set_template_status(&template.uid, Status::Published, None)
            .unwrap();
        let example = a
            .save_example(
                &Example {
                    template_uid: template.uid.clone(),
                    title: "Caso aprovado".into(),
                    reply: "Resposta exemplar".into(),
                    status: ExampleStatus::Approved,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        // A file of the example, with its extracted text, travels too.
        let file_id = a
            .add_attachment(
                crate::attachments::Owner::Example(example.id),
                "modelo-2024.txt",
                "",
                b"Texto do arquivo de exemplo",
            )
            .unwrap();
        a.extract_attachment_text(file_id).unwrap();
        let receipt = a.import_capture(&sample_capture()).unwrap();
        let record = GenerationRecord {
            uid: uuid::Uuid::new_v4().simple().to_string(),
            document_id: receipt.document_id,
            capture_id: receipt.capture_id,
            template_uid: Some(template.uid.clone()),
            template_version: Some(1),
            template_name: template.name.clone(),
            blocks: vec![UsedBlock {
                uid: block.uid.clone(),
                name: block.name.clone(),
                version: 1,
                mode: "Obrigatória".into(),
                text: "Use frases curtas.".into(),
            }],
            model: "modelo-teste".into(),
            outcome: Outcome::Accepted,
            ..Default::default()
        };
        a.record_generation(&record).unwrap();
        let draft_id = a
            .save_draft_full(
                receipt.document_id,
                receipt.capture_id,
                "resposta",
                "MINUTA PARA REVISÃO gerada",
                &[],
                Some(&crate::model::DraftProvenance {
                    generation_uid: Some(record.uid.clone()),
                    template_uid: Some(template.uid.clone()),
                    template_version: Some(1),
                    template_name: template.name.clone(),
                    model: "modelo-teste".into(),
                    ..Default::default()
                }),
            )
            .unwrap();
        a.sync_with_folders(&a.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();

        // B receives everything, with the same identities and versions.
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", files.clone()))
            .unwrap();
        b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        let on_b = b.template_by_uid(&template.uid).unwrap().unwrap();
        assert_eq!(on_b.status, Status::Published);
        assert_eq!(on_b.current_version, 1);
        assert_eq!(
            on_b.published_version().unwrap().block_refs[0].version,
            Some(1)
        );
        assert!(
            on_b.offered_for("resposta", "Trabalho"),
            "shared: visible to B"
        );
        assert_eq!(
            b.block_by_uid(&block.uid).unwrap().unwrap().current_version,
            1
        );
        let examples = b.examples_of(&template.uid).unwrap();
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].status, ExampleStatus::Approved);
        let files = b.example_files(&examples[0]).unwrap();
        assert_eq!(files.len(), 1, "the file and its text arrived");
        assert_eq!(files[0].text, "Texto do arquivo de exemplo");
        assert_eq!(
            b.attachment_bytes(file_id).unwrap().1,
            b"Texto do arquivo de exemplo"
        );
        let generations = b.generations_for_document(receipt.document_id).unwrap();
        assert_eq!(generations.len(), 1);
        assert_eq!(generations[0].draft_id, Some(draft_id));
        assert_eq!(generations[0].blocks[0].text, "Use frases curtas.");
        let drafts = b.load_document(receipt.document_id).unwrap().drafts;
        assert_eq!(
            drafts[0].provenance.as_ref().unwrap().template_version,
            Some(1)
        );

        // B revises the template (a queued change replayed on A's side):
        // version 2 is published, and the draft made with version 1 still
        // says version 1, whose text is intact.
        let mut revised = on_b.clone();
        let mut next = on_b.published_version().unwrap().clone();
        next.sections[0].guidance = "Texto revisto por B".into();
        revised.versions.push(next);
        b.save_template(&revised, None).unwrap();
        b.set_template_status(&template.uid, Status::Published, None)
            .unwrap();
        b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        let report = a
            .sync_with_folders(&a.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        assert_eq!(
            report.conflicts,
            0,
            "{:?}",
            a.list_sync_conflicts().unwrap()
        );
        let on_a = a.template_by_uid(&template.uid).unwrap().unwrap();
        assert_eq!(on_a.current_version, 2);
        assert_eq!(
            on_a.version(1).unwrap().sections[0].guidance,
            "Ref. {{numero_documento}}"
        );
        assert_eq!(
            on_a.version(2).unwrap().sections[0].guidance,
            "Texto revisto por B"
        );
        let drafts = a.load_document(receipt.document_id).unwrap().drafts;
        assert_eq!(
            drafts[0].provenance.as_ref().unwrap().template_version,
            Some(1)
        );
        assert!(a.template_in_use(&template.uid).unwrap());
        assert!(a.delete_template(&template.uid).is_err());

        // A personal template of A is carried but not offered to B.
        let personal = a
            .save_template(&WorkTemplate::new("Só meu", "resposta", "Texto"), None)
            .unwrap();
        a.set_template_status(&personal.uid, Status::Published, None)
            .unwrap();
        a.sync_with_folders(&a.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        b.sync_with_folders(&b.load_intranet_config().unwrap(), key, false, &reporter)
            .unwrap();
        let personal_on_b = b.template_by_uid(&personal.uid).unwrap().unwrap();
        assert!(personal_on_b.offered_for("resposta", "Casa"));
        assert!(!personal_on_b.offered_for("resposta", "Trabalho"));
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
        host.assignee = "SERVIDOR-B".into();
        host.status = "em analise".into();
        store.save_task(&host).unwrap();
        let mut local = base.clone();
        local.title = "Título revisto offline".into();
        local.priority = "alta".into();
        local.status = "aguardando".into();
        local.notes = "Observação feita sem conexão".into();
        let merged = store
            .build_merged_mutation(
                "SERVIDOR-A",
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
        assert_eq!(task.assignee, "SERVIDOR-B");
        assert_eq!(task.status, "aguardando", "local wins when both changed");
        assert!(task.notes.contains("Observação feita sem conexão"));
        assert!(
            task.notes.contains("Em análise"),
            "the host value of a double-changed field is recorded: {}",
            task.notes
        );
        assert_eq!(task.document_id, Some(receipt.document_id));

        // The differences shown to the person name who changed each field.
        let conflict = SyncConflict {
            id: 0,
            created_at: now_iso(),
            actor: "SERVIDOR-A".into(),
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
            unresolved: false,
        };
        let diffs = store.conflict_differences(&conflict);
        let by_label = |label: &str| {
            diffs
                .iter()
                .find(|d| d.label == label)
                .unwrap_or_else(|| panic!("{label} missing from {diffs:?}"))
        };
        assert_eq!(by_label("Título").changed_by(), "só por você");
        assert_eq!(
            by_label("Responsável").changed_by(),
            "só no computador principal"
        );
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
        assert!(
            parse_connection_code(&connection_code("http://192.168.1.10:7878", "curta")).is_none()
        );
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

/// Development only: the views of this screen pictured by `--shots`, by
/// name. The tour calls `shots_open` with each one on a fresh panel.
#[cfg(feature = "shots")]
pub const SHOTS: &[&str] = &[
    "pasta",
    "principal",
    "conectar",
    "conectar-teste",
    "conectar-falha",
    "local",
];

#[cfg(feature = "shots")]
impl IntranetPanel {
    /// Puts the panel in the state called `view` (see `SHOTS`): the mode
    /// is chosen in the form only, nothing is saved or started.
    pub fn shots_open(&mut self, store: &mut crate::storage::Store, view: &str) {
        if let Ok(cfg) = store.load_intranet_config() {
            self.draft = cfg;
        }
        self.loaded = true;
        self.open = true;
        self.draft.mode = match view {
            "pasta" => {
                self.draft.folder_files =
                    vec![r"C:\Users\Exemplo\Google Drive\Ponte\ponte-sync.ponte".to_owned()];
                IntranetMode::SharedFolder
            }
            "principal" => IntranetMode::Host,
            "conectar" | "conectar-teste" | "conectar-falha" => {
                // A made-up result: nothing is contacted.
                let url = self.draft.host_url.clone();
                self.connection_test = match view {
                    "conectar-teste" => Some((
                        url,
                        true,
                        "Conexão feita com o computador principal (de Servidor A: 7 documentos e 10 tarefas).".to_owned(),
                    )),
                    "conectar-falha" => Some((
                        url,
                        false,
                        "Não foi possível conectar: sem resposta. Confira se o computador principal está ligado com o Ponte aberto, se o endereço e a chave estão certos e se os dois computadores estão na mesma rede.".to_owned(),
                    )),
                    _ => None,
                };
                IntranetMode::Client
            }
            _ => IntranetMode::Local,
        };
    }
}

#[cfg(test)]
mod standalone_task_sync_tests {
    use super::*;
    use crate::tasks::Task;

    const KEY: &str = "senha-de-teste-123";

    fn config(member: &str, files: &[String]) -> IntranetConfig {
        IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: files.to_vec(),
            folder_key: KEY.into(),
            ..IntranetConfig::default()
        }
    }

    fn sync(store: &mut Store) {
        let cfg = store.load_intranet_config().unwrap();
        store
            .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
            .unwrap();
    }

    /// Two computers that already share a library with one document.
    fn pair(dir: &std::path::Path) -> (Store, Store, i64) {
        let folder = dir.join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
        let files = vec![folder.display().to_string()];
        let mut a = Store::open(&dir.join("a.sqlite3")).unwrap();
        a.save_intranet_config(&config("Casa", &files)).unwrap();
        let document = a
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        sync(&mut a);
        let mut b = Store::open(&dir.join("b.sqlite3")).unwrap();
        b.save_intranet_config(&config("Trabalho", &files)).unwrap();
        sync(&mut b);
        assert_eq!(b.list_documents("").unwrap().len(), 1);
        (a, b, document)
    }

    fn standalone(title: &str) -> Task {
        let mut task = Task::standalone();
        task.title = title.into();
        task
    }

    fn find(store: &Store, title: &str) -> Task {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.title == title)
            .unwrap_or_else(|| panic!("task {title:?} missing"))
    }

    #[test]
    fn a_task_without_document_in_the_lixeira_does_not_stop_the_sync() {
        let dir = tempfile::tempdir().unwrap();
        let (mut a, mut b, _) = pair(dir.path());
        let id = a
            .save_task(&standalone("Devolver a chave da sala"))
            .unwrap();
        sync(&mut a);
        sync(&mut b);
        assert_eq!(find(&b, "Devolver a chave da sala").id, id);
        // In the Lixeira its row has no document: the snapshot still travels.
        a.trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Task,
            id,
        })
        .unwrap();
        sync(&mut a);
        sync(&mut b);
        assert!(b.list_tasks().unwrap().iter().all(|t| t.id != id));
        // And it comes back on both when restored.
        a.restore_item(crate::trash::Target {
            kind: crate::trash::Kind::Task,
            id,
        })
        .unwrap();
        sync(&mut a);
        sync(&mut b);
        assert_eq!(find(&b, "Devolver a chave da sala").document_id, None);
    }

    #[test]
    fn a_task_without_document_travels_and_is_attached_as_the_same_task() {
        let dir = tempfile::tempdir().unwrap();
        let (mut a, mut b, document) = pair(dir.path());

        // A creates a task without document: B receives it as such.
        a.save_task(&standalone("Renovar o certificado digital"))
            .unwrap();
        sync(&mut a);
        sync(&mut b);
        let at_b = find(&b, "Renovar o certificado digital");
        assert_eq!(at_b.document_id, None);

        // A attaches it to the document both have: B sees the same task
        // attached, not a copy.
        let id = find(&a, "Renovar o certificado digital").id;
        assert_eq!(id, at_b.id);
        a.relink_task(id, Some(document), None).unwrap();
        sync(&mut a);
        sync(&mut b);
        let tasks = b.list_tasks().unwrap();
        assert_eq!(
            tasks
                .iter()
                .filter(|t| t.title == "Renovar o certificado digital")
                .count(),
            1
        );
        assert_eq!(find(&b, "Renovar o certificado digital").id, id);
        assert_eq!(
            find(&b, "Renovar o certificado digital").document_id,
            Some(document)
        );

        // And back: B detaches it, A receives it without document.
        b.relink_task(id, None, None).unwrap();
        sync(&mut b);
        sync(&mut a);
        assert_eq!(find(&a, "Renovar o certificado digital").document_id, None);
    }

    #[test]
    fn tasks_without_document_created_on_both_sides_before_syncing_all_arrive() {
        let dir = tempfile::tempdir().unwrap();
        let (mut a, mut b, _document) = pair(dir.path());
        a.save_task(&standalone("Pedir a sala da reunião")).unwrap();
        b.save_task(&standalone("Confirmar o café da manhã"))
            .unwrap();
        sync(&mut b);
        sync(&mut a);
        sync(&mut b);
        for store in [&a, &b] {
            let tasks = store.list_tasks().unwrap();
            for title in ["Pedir a sala da reunião", "Confirmar o café da manhã"] {
                let found: Vec<_> = tasks.iter().filter(|t| t.title == title).collect();
                assert_eq!(found.len(), 1, "{title}");
                assert_eq!(found[0].document_id, None);
            }
        }
        // Same task ids on both computers.
        assert_eq!(
            find(&a, "Pedir a sala da reunião").id,
            find(&b, "Pedir a sala da reunião").id
        );
    }

    #[test]
    fn relinking_while_another_computer_edits_the_task_follows_the_conflict_rules() {
        let dir = tempfile::tempdir().unwrap();
        let (mut a, mut b, document) = pair(dir.path());
        a.save_task(&standalone("Preparar a pauta")).unwrap();
        sync(&mut a);
        sync(&mut b);
        let id = find(&a, "Preparar a pauta").id;

        // B edits the notes (the form's save, link unchanged); A relinks.
        let mut edited = find(&b, "Preparar a pauta");
        edited.notes = "Incluir o item da licitação.".into();
        b.save_task(&edited).unwrap();
        a.relink_task(id, Some(document), None).unwrap();
        sync(&mut b);
        sync(&mut a);

        // The relink was made on an older version of the task: it is a
        // conflict to resolve, like any other edit.
        let conflicts = a.list_sync_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        a.resolve_sync_conflict(conflicts[0].id, ConflictResolution::KeepLocal)
            .unwrap();
        sync(&mut a);
        sync(&mut b);
        for store in [&a, &b] {
            let task = find(store, "Preparar a pauta");
            assert_eq!(task.document_id, Some(document));
            assert_eq!(task.notes, "Incluir o item da licitação.");
        }
    }

    #[test]
    fn an_edit_queued_after_a_relink_does_not_overtake_it() {
        let dir = tempfile::tempdir().unwrap();
        let (mut a, mut b, document) = pair(dir.path());
        a.save_task(&standalone("Revisar a minuta")).unwrap();
        sync(&mut a);
        sync(&mut b);
        // Offline on B: edit, relink, edit again.
        let mut task = find(&b, "Revisar a minuta");
        task.notes = "Primeira nota.".into();
        b.save_task(&task).unwrap();
        b.relink_task(task.id, Some(document), None).unwrap();
        let mut task = find(&b, "Revisar a minuta");
        task.notes = "Segunda nota.".into();
        b.save_task(&task).unwrap();
        sync(&mut b);
        sync(&mut a);
        let at_a = find(&a, "Revisar a minuta");
        assert_eq!(at_a.document_id, Some(document));
        assert_eq!(at_a.notes, "Segunda nota.");
    }
}
