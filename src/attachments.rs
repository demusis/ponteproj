//! Files people attach to a document or a task ("Anexos"). They are stored
//! sealed in the library like the SIGADOC PDFs, shown to the whole team, and
//! removed through the trash (restorable). Nothing here touches the SIGADOC.
use crate::{labels, protection, storage::Store};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PER_OWNER: usize = 20;
pub const MAX_NAME_CHARS: usize = 180;
pub const MAX_DESCRIPTION_CHARS: usize = 500;

/// Who an attachment belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    Document(i64),
    Task(i64),
    /// An atendimento of the "Atendimentos" panel (kind `saiop`).
    Attendance(i64),
    /// An example of a work template (kind `example`): a past reply whose
    /// text may be shown to the AI as a model of form.
    Example(i64),
}

impl Owner {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Document(_) => "document",
            Self::Task(_) => "task",
            Self::Attendance(_) => "saiop",
            Self::Example(_) => "example",
        }
    }
    pub fn id(self) -> i64 {
        match self {
            Self::Document(id) | Self::Task(id) | Self::Attendance(id) | Self::Example(id) => id,
        }
    }
    pub fn parse(kind: &str, id: i64) -> Result<Self> {
        match kind {
            "document" => Ok(Self::Document(id)),
            "task" => Ok(Self::Task(id)),
            "saiop" => Ok(Self::Attendance(id)),
            "example" => Ok(Self::Example(id)),
            _ => bail!("Tipo de item inválido para anexo."),
        }
    }
    fn noun(self) -> &'static str {
        match self {
            Self::Document(_) => "documento",
            Self::Task(_) => "tarefa",
            Self::Attendance(_) => "atendimento",
            Self::Example(_) => "exemplo",
        }
    }
    /// Owners whose attachments have no trash (they hang from no document):
    /// removal is final.
    pub fn deletes_for_good(self) -> bool {
        matches!(self, Self::Attendance(_) | Self::Example(_))
    }
}

/// Everything about an attachment except its bytes. Stored sealed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMeta {
    #[serde(default)]
    pub id: i64,
    /// Original file name, without folders.
    pub name: String,
    /// Human type ("PDF", "Word", "Imagem"…).
    pub kind: String,
    pub size: usize,
    pub sha256: String,
    #[serde(default)]
    pub description: String,
    pub added_by: String,
    pub added_at: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Signature {
    Pdf,
    Png,
    Jpeg,
    Gif,
    Bmp,
    Tiff,
    Zip,
    Ole,
    Rtf,
    Text,
}

/// Accepted extensions: work documents and images only.
const TYPES: &[(&str, &str, Signature)] = &[
    ("pdf", "PDF", Signature::Pdf),
    ("doc", "Word", Signature::Ole),
    ("docx", "Word", Signature::Zip),
    ("odt", "Texto OpenDocument", Signature::Zip),
    ("rtf", "RTF", Signature::Rtf),
    ("txt", "Texto", Signature::Text),
    ("xls", "Excel", Signature::Ole),
    ("xlsx", "Excel", Signature::Zip),
    ("ods", "Planilha OpenDocument", Signature::Zip),
    ("csv", "CSV", Signature::Text),
    ("ppt", "PowerPoint", Signature::Ole),
    ("pptx", "PowerPoint", Signature::Zip),
    ("odp", "Apresentação OpenDocument", Signature::Zip),
    ("png", "Imagem", Signature::Png),
    ("jpg", "Imagem", Signature::Jpeg),
    ("jpeg", "Imagem", Signature::Jpeg),
    ("gif", "Imagem", Signature::Gif),
    ("bmp", "Imagem", Signature::Bmp),
    ("tif", "Imagem", Signature::Tiff),
    ("tiff", "Imagem", Signature::Tiff),
    ("eml", "E-mail", Signature::Text),
    ("msg", "E-mail (Outlook)", Signature::Ole),
];

pub const ACCEPTED_TYPES_TEXT: &str = "PDF, Word (.doc, .docx), Excel (.xls, .xlsx), PowerPoint (.ppt, .pptx), OpenDocument (.odt, .ods, .odp), texto (.txt, .rtf, .csv), imagens (.png, .jpg, .gif, .bmp, .tif) e e-mails (.eml, .msg)";

/// Pattern for the Windows file dialog.
pub const DIALOG_PATTERN: &str = "*.pdf;*.doc;*.docx;*.odt;*.rtf;*.txt;*.xls;*.xlsx;*.ods;*.csv;*.ppt;*.pptx;*.odp;*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.tif;*.tiff;*.eml;*.msg";

/// Why a known-but-refused extension is refused, and what to do instead.
fn refusal_hint(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "zip" | "rar" | "7z" | "gz" | "tar" | "cab" => {
            "arquivos compactados podem esconder programas. Extraia o conteúdo e anexe os documentos um a um."
        }
        "exe" | "msi" | "com" | "scr" | "dll" | "bat" | "cmd" | "ps1" | "vbs" | "vbe" | "js"
        | "jse" | "wsf" | "wsh" | "hta" | "jar" | "lnk" | "reg" | "inf" | "pif" | "cpl" => {
            "programas, scripts e atalhos não são anexados por segurança. Se precisar compartilhá-lo, use a pasta da rede e anote o caminho nas observações."
        }
        "docm" | "xlsm" | "pptm" | "dotm" | "xltm" => {
            "arquivos do Office com macros não são aceitos. Abra-o no Office e salve como .docx, .xlsx ou .pptx (sem macros)."
        }
        "html" | "htm" | "mht" | "svg" | "xml" => {
            "páginas da web e arquivos semelhantes podem conter programas embutidos. Exporte o conteúdo como PDF e anexe o PDF."
        }
        _ => return None,
    })
}

fn extension_of(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| ext.to_ascii_lowercase())
}

fn signature_matches(signature: Signature, data: &[u8]) -> bool {
    match signature {
        Signature::Pdf => data.starts_with(b"%PDF-"),
        Signature::Png => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        Signature::Jpeg => data.starts_with(&[0xFF, 0xD8, 0xFF]),
        Signature::Gif => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        Signature::Bmp => data.starts_with(b"BM"),
        Signature::Tiff => data.starts_with(b"II*\0") || data.starts_with(b"MM\0*"),
        Signature::Zip => data.starts_with(b"PK\x03\x04"),
        Signature::Ole => data.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]),
        Signature::Rtf => data.starts_with(b"{\\rtf"),
        // Plain text: no NUL bytes in the first chunk (binary files have them).
        Signature::Text => !data.iter().take(8192).any(|b| *b == 0),
    }
}

/// Checks name, extension, size and content signature. Returns the human
/// type. Every error says what to do, because the person cannot change the
/// rules, only the file.
pub fn validate_file(name: &str, data: &[u8]) -> Result<&'static str> {
    let name = name.trim();
    ensure!(!name.is_empty(), "O arquivo não tem nome.");
    ensure!(
        name.chars().count() <= MAX_NAME_CHARS && !name.chars().any(char::is_control),
        "O nome do arquivo é longo demais (máximo de {MAX_NAME_CHARS} caracteres). Renomeie-o antes de anexar."
    );
    ensure!(
        !name.contains(['/', '\\']),
        "O nome do arquivo não pode conter barras."
    );
    let Some(extension) = extension_of(name) else {
        bail!("O arquivo “{name}” não tem extensão (.pdf, .docx…), então o Ponte não consegue conferir o tipo. Renomeie-o com a extensão correta e tente de novo. Tipos aceitos: {ACCEPTED_TYPES_TEXT}.");
    };
    if let Some(hint) = refusal_hint(&extension) {
        bail!("Arquivos .{extension} não são aceitos: {hint}");
    }
    let Some((_, kind, signature)) = TYPES.iter().find(|(ext, _, _)| *ext == extension) else {
        bail!("Arquivos .{extension} não são aceitos. Tipos aceitos: {ACCEPTED_TYPES_TEXT}. Converta o arquivo para um desses formatos (por exemplo, exporte como PDF) ou guarde-o na pasta da rede e anote o caminho nas observações.");
    };
    ensure!(
        !data.is_empty(),
        "O arquivo “{name}” está vazio. Confira se ele foi salvo corretamente antes de anexar."
    );
    ensure!(
        data.len() <= MAX_ATTACHMENT_BYTES,
        "{}",
        size_message(name, data.len())
    );
    ensure!(
        !data.starts_with(b"MZ"),
        "O conteúdo de “{name}” é um programa do Windows, não um {kind}. Programas não são anexados por segurança."
    );
    ensure!(
        signature_matches(*signature, data),
        "O conteúdo de “{name}” não corresponde a um arquivo .{extension} ({kind}). Abra o arquivo no programa de origem e salve-o de novo no formato correto, ou confira se a extensão está certa."
    );
    Ok(kind)
}

/// Guidance when a file is too big (also used before reading it).
pub fn size_message(name: &str, size: impl u64_or_usize) -> String {
    let size = size.into_u64();
    format!(
        "O arquivo “{name}” tem {}; o limite é {}. Para reduzir: salve o PDF com compressão ou em resolução menor, reduza a resolução da imagem, ou divida o documento em partes menores e anexe cada uma. Se precisar manter o original intacto, guarde-o na pasta da rede e anote o caminho nas observações.",
        labels::file_size(size),
        labels::file_size(MAX_ATTACHMENT_BYTES as u64)
    )
}

/// `usize` or `u64` for `size_message`, so callers can pass a file length
/// from the file system before reading it.
#[allow(non_camel_case_types)]
pub trait u64_or_usize {
    fn into_u64(self) -> u64;
}
impl u64_or_usize for usize {
    fn into_u64(self) -> u64 {
        self as u64
    }
}
impl u64_or_usize for u64 {
    fn into_u64(self) -> u64 {
        self
    }
}

pub fn validate_description(description: &str) -> Result<()> {
    ensure!(
        description.chars().count() <= MAX_DESCRIPTION_CHARS,
        "A descrição tem {} caracteres; o máximo é {MAX_DESCRIPTION_CHARS}. Resuma-a ou passe os detalhes para as observações.",
        description.chars().count()
    );
    ensure!(
        !description
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "A descrição contém caracteres inválidos."
    );
    Ok(())
}

/// File name safe to write in the temp folder: no separators or reserved
/// characters, never empty.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() {
        "anexo".into()
    } else {
        cleaned
    }
}

/// Where decrypted working copies live: `%LOCALAPPDATA%/PonteProj/temp` of the
/// local user profile, never inside the library folder (which may be synced to
/// the cloud when `PONTEPROJ_DATA_DIR` points there). Each running Ponte keeps
/// its copies in a private subfolder of it (see [`temp_dir`]).
fn production_temp_dir() -> Result<PathBuf> {
    Ok(directories::BaseDirs::new()
        .context("Perfil local do usuário indisponível")?
        .data_local_dir()
        .join("PonteProj")
        .join("temp"))
}

/// Shared parent of the per-session folders.
fn temp_root() -> Result<PathBuf> {
    // Tests never touch the real profile: each test thread gets its own folder.
    #[cfg(test)]
    let dir = std::env::temp_dir().join(format!(
        "ponteproj-tests-temp-{:?}",
        std::thread::current().id()
    ));
    #[cfg(not(test))]
    let dir = production_temp_dir()?;
    Ok(dir)
}

/// File inside a session folder that its owner keeps open without sharing for
/// as long as the process lives: another Ponte can tell a live folder from an
/// abandoned one by trying to open it.
const SESSION_LOCK: &str = ".ponte-sessao";

/// A session folder untouched for this long and without a lock is abandoned.
const STALE_SESSION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Name of this process's session folder: unique per run, so two windows (for
/// example on different libraries) never share working copies.
fn session_name() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| format!("sessao-{}", uuid::Uuid::new_v4().simple()))
}

/// Locks held by this process, by session folder.
fn held_locks() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, std::fs::File>> {
    static LOCKS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, std::fs::File>>,
    > = std::sync::OnceLock::new();
    LOCKS.get_or_init(Default::default)
}

/// Opens the lock file of a session folder so that nobody else can, creating
/// it when `create` is set. `None` while another process holds it.
fn open_session_lock(dir: &Path, create: bool) -> Option<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(create);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(dir.join(SESSION_LOCK)).ok()
}

/// Creates (once) and returns the private folder of this session inside `root`.
fn session_dir_in(root: &Path) -> Result<PathBuf> {
    let dir = root.join(session_name());
    std::fs::create_dir_all(&dir).context("Não foi possível criar a pasta temporária.")?;
    let mut locks = held_locks().lock().unwrap_or_else(|e| e.into_inner());
    if !locks.contains_key(&dir) {
        if let Some(lock) = open_session_lock(&dir, true) {
            locks.insert(dir.clone(), lock);
        }
    }
    Ok(dir)
}

/// Folder for temporary copies opened in other programs: private to this
/// running Ponte. Removed when the Ponte closes; abandoned ones of crashed
/// sessions are removed at the next start by [`sweep_stale_temp`].
pub fn temp_dir() -> Result<PathBuf> {
    session_dir_in(&temp_root()?)
}

/// Removes this session's working copies and its folder (called at close).
/// A file still open in another program stays; the sweep at a later start
/// retries once nothing holds the folder.
pub fn clear_temp_dir() {
    if let Ok(root) = temp_root() {
        remove_session_dir(&root.join(session_name()));
    }
}

fn remove_session_dir(dir: &Path) {
    let lock = held_locks()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(dir);
    // The lock file must be closed before the folder can go.
    drop(lock);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_name() != SESSION_LOCK {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let _ = std::fs::remove_file(dir.join(SESSION_LOCK));
    let _ = std::fs::remove_dir(dir);
}

/// Name of a working copy as `temp_file_name` writes it: `<record id>-<name>`.
fn is_working_copy_name(name: &str) -> bool {
    match name.split_once('-') {
        Some((id, rest)) => {
            !rest.is_empty()
                && (1..=19).contains(&id.len())
                && id.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// Removes, at start, what dead sessions left in `root`: session folders no
/// live process holds, and working copies of earlier versions that sat loose
/// in the folder. Never touches this session's folder or another live one.
fn sweep_stale_in(root: &Path) {
    let own = root.join(session_name());
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_file() {
            if is_working_copy_name(&entry.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        let name = entry.file_name();
        if !kind.is_dir() || path == own || !name.to_string_lossy().starts_with("sessao-") {
            continue;
        }
        let abandoned = if path.join(SESSION_LOCK).exists() {
            // The lock opens only when its owner is gone.
            open_session_lock(&path, false).is_some()
        } else {
            std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STALE_SESSION)
        };
        if abandoned {
            // Closes the probe handle first inside, so the folder can go.
            remove_session_dir(&path);
        }
    }
}

/// Earlier versions kept the copies inside the library folder, where a cloud
/// drive could have picked them up. That folder can hold other things (it may
/// be a shared Drive folder): only files named like a working copy go.
fn sweep_legacy_in(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let is_file = entry.file_type().is_ok_and(|t| t.is_file());
            if is_file && is_working_copy_name(&entry.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// At start: removes what earlier or crashed sessions left behind.
pub fn sweep_stale_temp() {
    if let Ok(root) = temp_root() {
        sweep_stale_in(&root);
    }
    #[cfg(not(test))]
    if let Ok(data) = crate::paths::data_dir() {
        sweep_legacy_in(&data.join("temp"));
    }
}

/// Longest file name a Windows folder entry accepts, in UTF-16 units.
const WINDOWS_NAME_UNITS: usize = 255;

/// Name of a temporary copy: `<id>-<name>`, cut to fit one Windows path
/// component (255 UTF-16 units) by shortening the stem and keeping the
/// validated extension, so the program that opens it still recognises it.
pub fn temp_file_name(id: i64, name: &str) -> String {
    let clean = sanitize_file_name(name);
    let prefix = format!("{id}-");
    let (stem, ext) = match (extension_of(&clean), clean.rsplit_once('.')) {
        (Some(_), Some((stem, ext))) => (stem.to_owned(), format!(".{ext}")),
        _ => (clean.clone(), String::new()),
    };
    let units = |s: &str| s.encode_utf16().count();
    let budget = WINDOWS_NAME_UNITS.saturating_sub(units(&prefix) + units(&ext));
    let mut kept = String::new();
    for ch in stem.chars() {
        if units(&kept) + ch.len_utf16() > budget {
            break;
        }
        kept.push(ch);
    }
    let kept = kept.trim_end_matches(['.', ' ']);
    let kept = if kept.is_empty() { "anexo" } else { kept };
    format!("{prefix}{kept}{ext}")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Store {
    pub(crate) fn ensure_attachments(&mut self) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        const DEFINITION: &str = "(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                owner_kind TEXT NOT NULL CHECK(owner_kind IN ('document','task','saiop','example')),
                owner_id INTEGER NOT NULL,
                document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,
                metadata BLOB NOT NULL,
                payload BLOB NOT NULL
            )";
        tx.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS attachments{DEFINITION};
            CREATE INDEX IF NOT EXISTS attachments_owner ON attachments(owner_kind,owner_id);"
        ))?;
        // Libraries from before 0.19.0 only allow documents and tasks as
        // owners and demand a document; those before 0.20.1 lack the
        // example owner. SQLite cannot alter a CHECK, so the table is
        // rebuilt once, keeping every row and id. Triggers that mention the
        // table go first (`ensure_write_guards` recreates them).
        let definition: String = tx.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='attachments'",
            [],
            |r| r.get(0),
        )?;
        if !definition.contains("'example'") {
            let triggers = tx
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'ponte_%'",
                )?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for name in triggers {
                tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {name};"))?;
            }
            tx.execute_batch(&format!(
                "CREATE TABLE attachments_v20{DEFINITION};
                 INSERT INTO attachments_v20(id,owner_kind,owner_id,document_id,metadata,payload)
                   SELECT id,owner_kind,owner_id,document_id,metadata,payload FROM attachments;
                 DROP TABLE attachments;
                 ALTER TABLE attachments_v20 RENAME TO attachments;
                 CREATE INDEX IF NOT EXISTS attachments_owner ON attachments(owner_kind,owner_id);"
            ))?;
        }
        if tx.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? < 10 {
            tx.pragma_update(None, "user_version", 10)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Document the owner belongs to, after checking that both are active.
    /// The document an attachment hangs from: the owner itself, the task's
    /// document, or the atendimento's linked document (none when it stands
    /// alone). Fails when that document or owner is in the trash.
    fn attachment_owner_document(conn: &rusqlite::Connection, owner: Owner) -> Result<Option<i64>> {
        match owner {
            Owner::Document(id) => {
                crate::trash::require_active_document(conn, id)?;
                Ok(Some(id))
            }
            Owner::Task(id) => crate::trash::require_active_task(conn, id),
            Owner::Attendance(id) => {
                // The atendimento may stand alone; when it is linked to a
                // document, that document must be active.
                let document: Option<i64> = conn
                    .query_row(
                        "SELECT document_id FROM saiop_attendances WHERE id=?1",
                        [id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .context("Atendimento não encontrado.")?;
                if let Some(document) = document {
                    crate::trash::require_active_document(conn, document)?;
                }
                Ok(document)
            }
            Owner::Example(id) => {
                let exists: Option<i64> = conn
                    .query_row("SELECT id FROM template_examples WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .optional()?;
                ensure!(
                    exists.is_some(),
                    "Exemplo não encontrado; salve-o antes de anexar."
                );
                Ok(None)
            }
        }
    }

    /// Removes an attachment for good. Used for attachments of atendimentos,
    /// which have no trash (the trash hangs from documents). On a client
    /// computer the request goes to the main computer.
    pub fn delete_attachment(&mut self, id: i64) -> Result<()> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::DeleteAttachment { id },
            )? {
                crate::intranet::MutationReply::Unit => return Ok(()),
                _ => bail!("O computador principal respondeu de forma inesperada ao excluir o anexo. Tente de novo."),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (owner_kind, owner_id, document, blob): (String, i64, Option<i64>, Vec<u8>) = tx
            .query_row(
                "SELECT owner_kind,owner_id,document_id,metadata FROM attachments WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .context("Anexo não encontrado (já excluído?).")?;
        let meta: AttachmentMeta = serde_json::from_slice(&protection::unseal(&blob)?)?;
        tx.execute(
            "DELETE FROM trash WHERE kind='attachment' AND item_id=?1",
            [id],
        )?;
        tx.execute("DELETE FROM attachments WHERE id=?1", [id])?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "attachment",
            id,
            None,
            "delete_attachment",
            document,
            &format!(
                "Apagou de vez o anexo “{}” de {} #{owner_id}",
                meta.name,
                labels::item_kind(&owner_kind)
            ),
        )?;
        crate::intranet::forget_orphan_versions(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Attaches a file. On a client computer the file goes to the main
    /// computer (or to the offline queue). Returns the attachment id.
    pub fn add_attachment(
        &mut self,
        owner: Owner,
        name: &str,
        description: &str,
        data: &[u8],
    ) -> Result<i64> {
        let kind = validate_file(name, data)?;
        validate_description(description)?;
        if let Some(cfg) = self.client_config()? {
            use base64::Engine;
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::AddAttachment {
                    owner_kind: owner.kind().into(),
                    owner_id: owner.id(),
                    name: name.trim().into(),
                    description: description.trim().into(),
                    data_base64: base64::engine::general_purpose::STANDARD.encode(data),
                },
            )? {
                crate::intranet::MutationReply::Id(id) => return Ok(id),
                _ => bail!("O computador principal respondeu de forma inesperada ao anexar o arquivo. Tente de novo."),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = insert_attachment(&tx, &actor, owner, name, description, data, kind)?;
        tx.commit()?;
        Ok(id)
    }

    /// [`Store::add_attachment`] to a document, for an action of the phone app:
    /// with an `operation_id` the row that says "this was committed" is written
    /// in the same transaction as the attachment and its queue entry (an id that
    /// is already there adds nothing: the caller looks it up first), and with
    /// `expected_number` the target must still be a document that is not in the
    /// trash and has exactly that number; otherwise nothing at all is written and
    /// the error is [`TargetChanged`]. Returns the attachment id.
    pub fn attach_to_document(&mut self, request: &AttachRequest<'_>) -> Result<i64> {
        let kind = validate_file(request.name, request.data)?;
        validate_description(request.description)?;
        let operation = request
            .operation_id
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let queue_actor = match self.client_config()? {
            None => None,
            Some(cfg) if cfg.mode == crate::intranet::IntranetMode::SharedFolder => {
                cfg.validate()?;
                Some(cfg.member_name.trim().to_owned())
            }
            Some(_) => {
                // Every change is a request to a main computer here: no single
                // transaction. The check comes first; the row after the change.
                if let Some(number) = request.expected_number {
                    if Store::live_document_number(&self.conn, request.document_id)?
                        .as_deref()
                        .map(str::trim)
                        != Some(number.trim())
                    {
                        return Err(TargetChanged.into());
                    }
                }
                let id = self.add_attachment(
                    Owner::Document(request.document_id),
                    request.name,
                    request.description,
                    request.data,
                )?;
                if let Some(operation) = operation {
                    let _ = crate::storage::record_operation(
                        &self.conn,
                        operation,
                        "attach",
                        request.document_id,
                        Some(id),
                        Some(request.data.len() as i64),
                    );
                }
                return Ok(id);
            }
        };
        let actor = queue_actor.clone().unwrap_or_else(|| self.active_actor());
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Queued inside this transaction: the entry carries the identity of
        // the attachment it inserts. A rollback closes the log again.
        if queue_actor.is_some() {
            crate::idents::start_log(&tx)?;
        }
        if let Some(number) = request.expected_number {
            if Store::live_document_number(&tx, request.document_id)?
                .as_deref()
                .map(str::trim)
                != Some(number.trim())
            {
                return Err(TargetChanged.into());
            }
        }
        let id = insert_attachment(
            &tx,
            &actor,
            Owner::Document(request.document_id),
            request.name,
            request.description,
            request.data,
            kind,
        )?;
        if let Some(queue_actor) = &queue_actor {
            crate::intranet::queue_attachment_locally(
                &tx,
                queue_actor,
                request.document_id,
                request.name,
                request.description,
                request.data,
                id,
            )?;
        }
        if let Some(operation) = operation {
            crate::storage::record_operation(
                &tx,
                operation,
                "attach",
                request.document_id,
                Some(id),
                Some(request.data.len() as i64),
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Active attachments of one document, task or atendimento, oldest first.
    pub fn list_attachments(&self, owner: Owner) -> Result<Vec<AttachmentMeta>> {
        list_active(&self.conn, owner)
    }

    /// Number of active attachments per owner id of one kind ("document",
    /// "task" or "saiop"), for badges in lists. The lists ask on every frame,
    /// so the answer is memoized until the library changes.
    pub fn attachment_counts(&self, owner_kind: &str) -> Result<BTreeMap<i64, usize>> {
        self.cached(&format!("attachment_counts\u{1}{owner_kind}"), || {
            self.attachment_counts_uncached(owner_kind)
        })
    }

    fn attachment_counts_uncached(&self, owner_kind: &str) -> Result<BTreeMap<i64, usize>> {
        let mut statement = self.conn.prepare(
            "SELECT a.owner_id, count(*) FROM attachments a
             WHERE a.owner_kind=?1
               AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='attachment' AND t.item_id=a.id)
             GROUP BY a.owner_id",
        )?;
        let rows = statement.query_map([owner_kind], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    /// Metadata and bytes of one attachment, including those in the trash
    /// (the team synchronization needs them too).
    pub fn attachment_bytes(&self, id: i64) -> Result<(AttachmentMeta, Vec<u8>)> {
        let (meta_blob, payload): (Vec<u8>, Vec<u8>) = self
            .conn
            .query_row(
                "SELECT metadata,payload FROM attachments WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .context("Anexo não encontrado.")?;
        let mut meta: AttachmentMeta = serde_json::from_slice(&protection::unseal(&meta_blob)?)?;
        meta.id = id;
        let data = protection::unseal(&payload)?;
        ensure!(
            format!("{:x}", Sha256::digest(&data)) == meta.sha256,
            "O conteúdo do anexo não confere com o registro; o acervo pode estar danificado."
        );
        Ok((meta, data))
    }

    /// Changes the description of an attachment. Goes through
    /// `record_mutation`; on a client computer it is sent to the main one.
    pub fn update_attachment_description(&mut self, id: i64, description: &str) -> Result<()> {
        validate_description(description)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::UpdateAttachment {
                    id,
                    description: description.trim().into(),
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // The document is NULL for an atendimento that stands alone.
        let (owner, document): (Owner, Option<i64>) = {
            let (kind, owner_id, document): (String, i64, Option<i64>) = tx
                .query_row(
                    "SELECT owner_kind,owner_id,document_id FROM attachments WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .context("Anexo não encontrado.")?;
            (Owner::parse(&kind, owner_id)?, document)
        };
        Self::attachment_owner_document(&tx, owner)?;
        ensure!(
            !crate::trash::contains(&tx, crate::trash::Kind::Attachment, id)?,
            "Anexo na lixeira. Restaure-o antes de editar."
        );
        let blob: Vec<u8> =
            tx.query_row("SELECT metadata FROM attachments WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
        let mut meta: AttachmentMeta = serde_json::from_slice(&protection::unseal(&blob)?)?;
        meta.description = description.trim().into();
        tx.execute(
            "UPDATE attachments SET metadata=?1 WHERE id=?2",
            params![protection::seal(&serde_json::to_vec(&meta)?)?, id],
        )?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "attachment",
            id,
            None,
            "update_attachment",
            document,
            &format!("Descrição do anexo “{}” alterada", meta.name),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Writes a copy in the temp folder so another program can open it.
    pub fn attachment_temp_copy(&self, id: i64) -> Result<PathBuf> {
        let (meta, data) = self.attachment_bytes(id)?;
        let path = temp_dir()?.join(temp_file_name(id, &meta.name));
        std::fs::write(&path, data).context("Não foi possível gravar a cópia temporária.")?;
        Ok(path)
    }

    /// Saves a copy where the person chose.
    pub fn export_attachment(&self, id: i64, destination: &Path) -> Result<()> {
        let (_, data) = self.attachment_bytes(id)?;
        crate::atomic_file::write_atomic(destination, &data)
            .with_context(|| format!("Não foi possível gravar em {}.", destination.display()))?;
        Ok(())
    }
}

/// Content digests of every attachment of `owner`, those in the trash too.
pub(crate) fn hashes_including_trash(
    conn: &rusqlite::Connection,
    owner: Owner,
) -> Result<Vec<String>> {
    let mut statement =
        conn.prepare("SELECT metadata FROM attachments WHERE owner_kind=?1 AND owner_id=?2")?;
    let rows = statement
        .query_map(params![owner.kind(), owner.id()], |r| {
            r.get::<_, Vec<u8>>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|blob| {
            let meta: AttachmentMeta = serde_json::from_slice(&protection::unseal(&blob)?)?;
            Ok(meta.sha256)
        })
        .collect()
}

/// Stores one attachment inside the caller's write transaction: owner and
/// limit checks, duplicate check, the sealed rows, and the library's version
/// and team-event entries. `kind` is what `validate_file` answered for these
/// bytes. Nothing is committed here.
fn insert_attachment(
    tx: &rusqlite::Connection,
    actor: &str,
    owner: Owner,
    name: &str,
    description: &str,
    data: &[u8],
    kind: &str,
) -> Result<i64> {
    let document = Store::attachment_owner_document(tx, owner)?;
    let existing = list_active(tx, owner)?;
    ensure!(
        existing.len() < MAX_PER_OWNER,
        "{} {} já tem {MAX_PER_OWNER} anexos, o máximo. {} ou junte vários arquivos num único PDF.",
        if matches!(owner, Owner::Task(_)) {
            "Esta"
        } else {
            "Este"
        },
        owner.noun(),
        if owner.deletes_for_good() {
            "Apague de vez um anexo que não seja mais necessário"
        } else {
            "Exclua um anexo que não seja mais necessário (ele vai para a Lixeira e pode ser restaurado)"
        }
    );
    let sha256 = format!("{:x}", Sha256::digest(data));
    if let Some(same) = existing.iter().find(|a| a.sha256 == sha256) {
        bail!(
            "Este arquivo já está anexado como “{}” (conteúdo idêntico). Se quiser outra descrição, edite a do anexo existente.",
            same.name
        );
    }
    let meta = AttachmentMeta {
        id: 0,
        name: name.trim().into(),
        kind: kind.into(),
        size: data.len(),
        sha256,
        description: description.trim().into(),
        added_by: actor.to_owned(),
        added_at: now(),
    };
    tx.execute(
        "INSERT INTO attachments(owner_kind,owner_id,document_id,metadata,payload) VALUES(?1,?2,?3,?4,?5)",
        params![
            owner.kind(),
            owner.id(),
            document,
            protection::seal(&serde_json::to_vec(&meta)?)?,
            protection::seal(data)?
        ],
    )?;
    let id = tx.last_insert_rowid();
    crate::intranet::record_mutation(
        tx,
        actor,
        "attachment",
        id,
        None,
        "add_attachment",
        document,
        &format!(
            "Anexou “{}” ({}, {}) a {} #{}",
            meta.name,
            meta.kind,
            labels::file_size(meta.size as u64),
            owner.noun(),
            owner.id()
        ),
    )?;
    Ok(id)
}

/// What [`Store::attach_to_document`] is asked to do.
pub struct AttachRequest<'a> {
    pub document_id: i64,
    pub name: &'a str,
    pub description: &'a str,
    pub data: &'a [u8],
    /// The phone app's idempotency key for this action.
    pub operation_id: Option<&'a str>,
    /// The number the person saw when choosing the document.
    pub expected_number: Option<&'a str>,
}

/// The document chosen for an attachment is no longer the one the person saw
/// (it was trashed, deleted or has another number): nothing was written.
#[derive(Debug)]
pub struct TargetChanged;

impl std::fmt::Display for TargetChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("O documento escolhido mudou (foi para a Lixeira, excluído ou renumerado) desde que você o escolheu. Escolha o documento de novo.")
    }
}
impl std::error::Error for TargetChanged {}

/// A file to attach to a record, as the importer hands it over: the name is
/// already sanitised.
pub(crate) struct AttachmentInput<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub data: &'a [u8],
}

/// The attachment step of an import failed. `completing` tells whether the
/// record was there before this import (a retry) and so is kept.
#[derive(Debug)]
pub(crate) struct AttachmentStepFailed {
    pub completing: bool,
    pub source: anyhow::Error,
}

impl std::fmt::Display for AttachmentStepFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}
impl std::error::Error for AttachmentStepFailed {}

/// Attaches to the document, inside the caller's transaction, every file
/// whose content is not on it yet (the trash counts as "there": a file the
/// person deleted does not come back). Returns the files stored, as (index
/// in `files`, attachment id), in order.
pub(crate) fn attach_missing(
    tx: &rusqlite::Connection,
    actor: &str,
    document_id: i64,
    files: &[AttachmentInput<'_>],
) -> Result<Vec<(usize, i64)>> {
    let owner = Owner::Document(document_id);
    let mut present: std::collections::BTreeSet<String> =
        hashes_including_trash(tx, owner)?.into_iter().collect();
    let mut added = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if !present.insert(format!("{:x}", Sha256::digest(file.data))) {
            continue;
        }
        let kind = validate_file(file.name, file.data)?;
        validate_description(file.description)?;
        let id = insert_attachment(
            tx,
            actor,
            owner,
            file.name,
            file.description,
            file.data,
            kind,
        )?;
        added.push((index, id));
    }
    Ok(added)
}

fn list_active(conn: &rusqlite::Connection, owner: Owner) -> Result<Vec<AttachmentMeta>> {
    let mut statement = conn.prepare(
        "SELECT a.id, a.metadata FROM attachments a
         WHERE a.owner_kind=?1 AND a.owner_id=?2
           AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='attachment' AND t.item_id=a.id)
         ORDER BY a.id",
    )?;
    let rows = statement
        .query_map(params![owner.kind(), owner.id()], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, blob)| {
            let mut meta: AttachmentMeta = serde_json::from_slice(&protection::unseal(&blob)?)?;
            meta.id = id;
            Ok(meta)
        })
        .collect()
}

#[cfg(feature = "desktop")]
/// The "Anexos" block shown in the document tab and in the task form.
#[derive(Default)]
pub struct AttachmentsPanel {
    /// Description edits not yet saved, by attachment id.
    descriptions: BTreeMap<i64, String>,
    message: String,
    message_is_error: bool,
    pdf_request: Option<(i64, String)>,
    trash_request: Option<crate::trash::Target>,
    /// "Excluir de vez" clicked on this attachment of an atendimento; waiting
    /// for confirmation.
    confirm_delete: Option<i64>,
}

/// Per-pass bookkeeping of the attachments blocks, kept in egui temp memory
/// (one value shared by every block of the window).
#[cfg(feature = "desktop")]
#[derive(Clone, Default)]
struct DropFrame {
    /// Pass this record describes.
    pass: u64,
    /// Blocks drawn so far in `pass`.
    count: usize,
    /// Blocks drawn in the previous pass (0 when that pass was not the one
    /// right before this one).
    prev: usize,
    /// Pass whose dropped files were already taken by a block.
    handled: Option<u64>,
}

/// What a drop of files from Windows Explorer does, decided by how many
/// attachments blocks are on screen. Windows gives the application no
/// pointer position during a drag from another program (winit's
/// `DragOver` ignores the point and sends no `CursorMoved`; egui-winit only
/// forwards `HoveredFile`/`DroppedFile`), so the target cannot be chosen by
/// the pointer: only a single block can safely receive.
#[cfg(feature = "desktop")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropTarget {
    /// No attachments block is on screen.
    Nobody,
    /// The only block receives the files.
    Block,
    /// Two or more blocks: the person must use the button of one of them.
    Ambiguous,
}

#[cfg(feature = "desktop")]
fn drop_target(blocks: usize) -> DropTarget {
    match blocks {
        0 => DropTarget::Nobody,
        1 => DropTarget::Block,
        _ => DropTarget::Ambiguous,
    }
}

#[cfg(feature = "desktop")]
const AMBIGUOUS_DROP: &str = "Para anexar, use o botão Anexar arquivo… do bloco desejado.";
#[cfg(feature = "desktop")]
const FOLDER_REFUSAL: &str = "Pastas não podem ser anexadas; solte os arquivos.";

/// Message for a drop of `outcomes` (file name and result, in order) plus
/// `skipped` files left out because the count limit was reached; the flag is
/// true for an error. One file keeps the message of the dialog path.
#[cfg(feature = "desktop")]
fn drop_summary(outcomes: &[(String, Result<String, String>)], skipped: usize) -> (String, bool) {
    if let ([(_, result)], 0) = (outcomes, skipped) {
        return match result {
            Ok(text) => (text.clone(), false),
            Err(text) => (text.clone(), true),
        };
    }
    let attached = outcomes.iter().filter(|(_, r)| r.is_ok()).count();
    let mut parts = Vec::new();
    if skipped > 0 {
        let done = match attached {
            0 => "Nenhum anexado".to_owned(),
            1 => "1 anexado".to_owned(),
            n => format!("{n} anexados"),
        };
        parts.push(format!(
            "{done}; os demais passam do limite de {MAX_PER_OWNER} anexos."
        ));
    } else if attached > 0 {
        parts.push(if attached == 1 {
            "1 anexado.".to_owned()
        } else {
            format!("{attached} anexados.")
        });
    }
    for (name, result) in outcomes {
        if let Err(text) = result {
            parts.push(format!("“{name}”: {text}"));
        }
    }
    (parts.join(" "), skipped > 0 || attached < outcomes.len())
}

#[cfg(feature = "desktop")]
impl AttachmentsPanel {
    /// PDF attachment the person asked to read in the Ponte: (id, label).
    pub fn take_pdf_request(&mut self) -> Option<(i64, String)> {
        self.pdf_request.take()
    }
    pub fn take_trash_request(&mut self) -> Option<crate::trash::Target> {
        self.trash_request.take()
    }
    /// True while a description edit is not saved.
    pub fn dirty(&self) -> bool {
        !self.descriptions.is_empty()
    }
    pub fn forget(&mut self) {
        self.descriptions.clear();
        self.message.clear();
    }

    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.message = text.into();
        self.message_is_error = error;
    }

    pub fn show(&mut self, ui: &mut eframe::egui::Ui, store: &mut Store, owner: Owner, busy: bool) {
        use crate::theme::{self, icon};
        use crate::widgets::{self, Tone};
        use eframe::egui;

        let list = match store.list_attachments(owner) {
            Ok(list) => list,
            Err(error) => {
                soft_note(
                    ui,
                    Tone::Warn,
                    icon::WARNING,
                    &format!("Não foi possível ler os anexos: {error}"),
                    None,
                );
                return;
            }
        };
        let full = list.len() >= MAX_PER_OWNER;
        // Files dragged over the window / dropped on it (native: paths only).
        let ctx = ui.ctx().clone();
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let pass = ctx.cumulative_pass_nr();
        let state_id = egui::Id::unique("ponte-attachments-drop");
        let mut state: DropFrame = ctx.data_mut(|d| d.get_temp(state_id)).unwrap_or_default();
        if state.pass != pass {
            state.prev = if state.pass + 1 == pass {
                state.count
            } else {
                0
            };
            state.pass = pass;
            state.count = 0;
        }
        state.count += 1;
        // Blocks seen so far, or in the previous pass (blocks drawn later in
        // this pass are not known yet; frames are continuous during a drag).
        let target = drop_target(state.count.max(state.prev));
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        let cannot_receive = if !ui.is_enabled() {
            Some("Responda à pergunta aberta antes de anexar.")
        } else if busy {
            Some("Aguarde a operação em andamento.")
        } else if full {
            Some("Limite de anexos atingido. Exclua um anexo para abrir espaço.")
        } else {
            None
        };
        if hovering {
            ctx.request_repaint();
        }
        if !dropped.is_empty() && state.handled != Some(pass) {
            match (target, cannot_receive) {
                (DropTarget::Ambiguous, _) => self.notice(AMBIGUOUS_DROP, true),
                (_, Some(reason)) => {
                    state.handled = Some(pass);
                    self.notice(reason, true);
                }
                _ => {
                    state.handled = Some(pass);
                    self.attach_dropped(store, owner, &dropped);
                }
            }
        }
        ctx.data_mut(|d| d.insert_temp(state_id, state));
        let mut pick = false;
        ui.horizontal_wrapped(|ui| {
            widgets::small_muted(ui, format!("{} de {MAX_PER_OWNER} anexos", list.len()));
            let on_delete = if owner.deletes_for_good() {
                "Apagar de vez remove o anexo em definitivo, sem passar pela Lixeira."
            } else {
                "Excluir um anexo o leva para a Lixeira, de onde pode ser restaurado."
            };
            crate::help_ui::help(ui, format!(
                "Até {MAX_PER_OWNER} anexos de até {} cada. Tipos aceitos: {ACCEPTED_TYPES_TEXT}. Programas, atalhos e arquivos compactados não são aceitos. {on_delete}",
                labels::file_size(MAX_ATTACHMENT_BYTES as u64)
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!busy && !full, widgets::secondary(format!("{} Anexar arquivo…", icon::CLIP)))
                    .on_hover_text(format!("Escolhe um arquivo deste computador (até {}). Ele fica guardado, protegido, no acervo e a equipe também pode vê-lo.", labels::file_size(MAX_ATTACHMENT_BYTES as u64)))
                    .on_disabled_hover_text(if full { "Limite de anexos atingido. Exclua um anexo para abrir espaço." } else { "Aguarde a operação em andamento." })
                    .clicked()
                {
                    pick = true;
                }
            });
        });
        if pick {
            if let Some(path) = crate::windows_ui::pick_file(
                "Anexar arquivo",
                &[
                    ("Documentos e imagens", DIALOG_PATTERN),
                    ("Todos os arquivos", "*.*"),
                ],
            ) {
                self.attach(store, owner, &path);
            }
        }
        if !self.message.is_empty() {
            ui.add_space(theme::S1);
            let tone = if self.message_is_error {
                Tone::Danger
            } else {
                Tone::Ok
            };
            let glyph = if self.message_is_error {
                icon::ERROR
            } else {
                icon::CHECK
            };
            let mut close = false;
            soft_note(ui, tone, glyph, &self.message, Some(&mut close));
            if close {
                self.message.clear();
            }
        }
        if hovering {
            ui.add_space(theme::S1);
            let (text, active) = match (target, cannot_receive) {
                (DropTarget::Ambiguous, _) => (AMBIGUOUS_DROP, false),
                (_, Some(reason)) => (reason, false),
                _ => ("Solte aqui para anexar", true),
            };
            let p = theme::c();
            let color = if active { p.primary } else { p.muted };
            egui::Frame::new()
                .fill(if active { p.primary_soft } else { p.sunken })
                .stroke(egui::Stroke::new(if active { 1.5 } else { 1.0 }, color))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(text)
                                .font(theme::font_strong(theme::SIZE_BODY))
                                .color(color),
                        );
                    });
                });
        } else if list.is_empty() {
            ui.add_space(theme::S1);
            widgets::small_muted(
                ui,
                match owner {
                    Owner::Document(_) => "Nenhum arquivo anexado a este documento.",
                    Owner::Task(_) => "Nenhum arquivo anexado a esta tarefa.",
                    Owner::Attendance(_) => "Nenhum arquivo anexado a este atendimento.",
                    Owner::Example(_) => "Nenhum arquivo anexado a este exemplo.",
                },
            );
        }
        let mut save_description: Option<(i64, String)> = None;
        for (index, meta) in list.iter().enumerate() {
            if index == 0 {
                ui.add_space(theme::S1);
            }
            ui.separator();
            let is_pdf = meta.kind == "PDF";
            ui.horizontal_top(|ui| {
                ui.label(
                    egui::RichText::new(if is_pdf { icon::PDF } else { icon::CLIP })
                        .size(20.0)
                        .color(theme::c().muted),
                );
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 190.0).max(200.0));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&meta.name)
                                .font(theme::font_strong(theme::SIZE_BODY)),
                        )
                        .wrap(),
                    );
                    widgets::small_muted(ui, format!(
                        "{} • {} • por {} em {}",
                        meta.kind,
                        labels::file_size(meta.size as u64),
                        if meta.added_by.trim().is_empty() { "—" } else { meta.added_by.trim() },
                        labels::datetime(&meta.added_at)
                    ));
                    let mut text = self
                        .descriptions
                        .get(&meta.id)
                        .cloned()
                        .unwrap_or_else(|| meta.description.clone());
                    ui.horizontal_wrapped(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .hint_text("Descrição (opcional): o que é este arquivo")
                                .desired_width((ui.available_width() - 200.0).max(160.0)),
                        );
                        let changed = text != meta.description;
                        if changed {
                            if ui.add_enabled(!busy, widgets::secondary("Salvar descrição")).clicked()
                                || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                            {
                                save_description = Some((meta.id, text.clone()));
                            }
                            if ui.add(widgets::quiet("Desfazer")).clicked() {
                                text = meta.description.clone();
                            }
                        }
                    });
                    if text != meta.description {
                        self.descriptions.insert(meta.id, text);
                    } else {
                        self.descriptions.remove(&meta.id);
                    }
                    widgets::small_muted(ui, format!("código de verificação {}…", &meta.sha256[..meta.sha256.len().min(12)]))
                        .on_hover_text(format!("Código de verificação completo: {}", meta.sha256));
                    if self.confirm_delete == Some(meta.id) {
                        ui.add_space(theme::S1);
                        soft_note(ui, Tone::Warn, icon::WARNING, &format!("Apagar “{}” de vez? Não há como restaurar.", meta.name), None);
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!busy, widgets::danger("Apagar de vez")).clicked() {
                                self.confirm_delete = None;
                                match store.delete_attachment(meta.id) {
                                    Ok(()) => self.notice(format!("Anexo “{}” apagado.", meta.name), false),
                                    Err(error) => self.notice(error.to_string(), true),
                                }
                            }
                            if ui.add(widgets::secondary("Cancelar")).clicked() {
                                self.confirm_delete = None;
                            }
                        });
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    // Removal sits at the far right, apart from the others.
                    let remove_hint = if owner.deletes_for_good() {
                        "Apagar de vez: o anexo é apagado em definitivo, sem passar pela Lixeira (anexos de atendimentos e de exemplos não têm lixeira)."
                    } else {
                        "Excluir: envia o anexo para a Lixeira, de onde pode ser restaurado."
                    };
                    let remove = ui
                        .add_enabled_ui(!busy, |ui| widgets::icon_button(ui, icon::TRASH, remove_hint))
                        .inner;
                    if remove.clicked() {
                        if owner.deletes_for_good() {
                            // Atendimentos and examples have no trash: the
                            // removal is final.
                            self.confirm_delete = Some(meta.id);
                        } else {
                            self.trash_request = Some(crate::trash::Target {
                                kind: crate::trash::Kind::Attachment,
                                id: meta.id,
                            });
                        }
                    }
                    if ui
                        .add_enabled(!busy, widgets::quiet("Exportar"))
                        .on_hover_text("Grava uma cópia onde você escolher.")
                        .clicked()
                    {
                        if let Some(destination) = crate::windows_ui::save_file(
                            "Salvar cópia do anexo",
                            &sanitize_file_name(&meta.name),
                            &[("Todos os arquivos", "*.*")],
                        ) {
                            match store.export_attachment(meta.id, &destination) {
                                Ok(()) => self.notice(format!("Cópia salva em {}.", destination.display()), false),
                                Err(error) => self.notice(error.to_string(), true),
                            }
                        }
                    }
                    let open_label = if is_pdf { "Ler no Ponte" } else { "Abrir" };
                    if ui
                        .add_enabled(!busy, widgets::quiet(open_label))
                        .on_hover_text(if is_pdf {
                            "Abre no leitor de PDF do Ponte, sem sair do aplicativo.".to_owned()
                        } else {
                            format!("Abre uma cópia temporária no programa padrão do Windows para {}. Alterações feitas lá não voltam para o Ponte.", meta.kind)
                        })
                        .clicked()
                    {
                        if is_pdf {
                            self.pdf_request = Some((meta.id, meta.name.clone()));
                        } else {
                            match store
                                .attachment_temp_copy(meta.id)
                                .and_then(|path| crate::windows_ui::open_with_default_app(&path))
                            {
                                Ok(()) => self.notice(format!("“{}” aberto no programa padrão do Windows.", meta.name), false),
                                Err(error) => self.notice(format!("Não foi possível abrir “{}”: {error}", meta.name), true),
                            }
                        }
                    }
                });
            });
        }
        if let Some((id, description)) = save_description {
            match store.update_attachment_description(id, &description) {
                Ok(()) => {
                    self.descriptions.remove(&id);
                    self.notice("Descrição salva.", false);
                }
                Err(error) => self.notice(error.to_string(), true),
            }
        }
    }

    /// Reads the chosen file and attaches it, with guidance on every refusal.
    fn attach(&mut self, store: &mut Store, owner: Owner, path: &Path) {
        match Self::attach_file(store, owner, path) {
            Ok(text) => self.notice(text, false),
            Err(text) => self.notice(text, true),
        }
    }

    /// The attach path shared by the dialog and by drops: limits, checks and
    /// store call. Gives the message for the person either way.
    fn attach_file(store: &mut Store, owner: Owner, path: &Path) -> Result<String, String> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Check the size before reading, so a huge file is refused at once.
        match std::fs::metadata(path) {
            Ok(info) if info.len() > MAX_ATTACHMENT_BYTES as u64 => {
                return Err(size_message(&name, info.len()));
            }
            Ok(_) => {}
            Err(error) => return Err(format!("Não foi possível ler “{name}”: {error}")),
        }
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) => return Err(format!("Não foi possível ler “{name}”: {error}")),
        };
        match store.add_attachment(owner, &name, "", &data) {
            Ok(_) => Ok(format!(
                "Anexado: “{name}” ({}). Se quiser, escreva uma descrição.",
                labels::file_size(data.len() as u64)
            )),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Attaches files dropped from Windows Explorer, one by one in order.
    fn attach_dropped(&mut self, store: &mut Store, owner: Owner, paths: &[PathBuf]) {
        let mut outcomes: Vec<(String, Result<String, String>)> = Vec::new();
        let mut skipped = 0;
        for (index, path) in paths.iter().enumerate() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                outcomes.push((name, Err(FOLDER_REFUSAL.to_owned())));
                continue;
            }
            let count = store.list_attachments(owner).map_or(0, |l| l.len());
            if count >= MAX_PER_OWNER {
                skipped = paths[index..].iter().filter(|p| !p.is_dir()).count();
                break;
            }
            outcomes.push((name, Self::attach_file(store, owner, path)));
        }
        let (text, error) = drop_summary(&outcomes, skipped);
        self.notice(text, error);
    }
}

/// A short note in a soft-coloured frame: icon, text and, when asked for,
/// a close button (`close` is set when it is clicked).
#[cfg(feature = "desktop")]
fn soft_note(
    ui: &mut eframe::egui::Ui,
    tone: crate::widgets::Tone,
    glyph: &str,
    text: &str,
    close: Option<&mut bool>,
) {
    use eframe::egui;
    let (fg, bg) = tone.colors();
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // `horizontal` keeps the row as high as its content; a bare
            // `with_layout` here would take all the height left in the page.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(glyph).size(18.0).color(fg));
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - 36.0).max(80.0));
                    ui.add(egui::Label::new(egui::RichText::new(text).color(fg)).wrap());
                });
                if let Some(close) = close {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::icon_button(
                            ui,
                            crate::theme::icon::CLOSE,
                            "Fechar o aviso",
                        )
                        .clicked()
                        {
                            *close = true;
                        }
                    });
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::sample_capture;

    fn store_with_document() -> (tempfile::TempDir, Store, i64) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let receipt = store.import_capture(&sample_capture()).unwrap();
        (dir, store, receipt.document_id)
    }

    #[test]
    fn types_sizes_and_signatures_are_checked_with_guidance() {
        let pdf = include_bytes!("../tests/fixtures/document.pdf");
        assert_eq!(validate_file("oficio.pdf", pdf).unwrap(), "PDF");
        assert_eq!(
            validate_file("foto.PNG", b"\x89PNG\r\n\x1a\n....").unwrap(),
            "Imagem"
        );
        assert_eq!(
            validate_file("nota.txt", "texto simples\n".as_bytes()).unwrap(),
            "Texto"
        );
        assert_eq!(
            validate_file("planilha.xlsx", b"PK\x03\x04rest").unwrap(),
            "Excel"
        );
        assert_eq!(
            validate_file(
                "antigo.doc",
                &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0]
            )
            .unwrap(),
            "Word"
        );

        let zip = validate_file("lote.zip", b"PK\x03\x04")
            .unwrap_err()
            .to_string();
        assert!(
            zip.contains("compactados") && zip.contains("um a um"),
            "{zip}"
        );
        let exe = validate_file("instalador.exe", b"MZ...")
            .unwrap_err()
            .to_string();
        assert!(exe.contains("segurança"), "{exe}");
        let macro_doc = validate_file("modelo.docm", b"PK\x03\x04")
            .unwrap_err()
            .to_string();
        assert!(
            macro_doc.contains("macros") && macro_doc.contains(".docx"),
            "{macro_doc}"
        );
        let unknown = validate_file("desenho.dwg", b"xx").unwrap_err().to_string();
        assert!(
            unknown.contains("Tipos aceitos") && unknown.contains("PDF"),
            "{unknown}"
        );
        let no_ext = validate_file("semextensao", b"xx").unwrap_err().to_string();
        assert!(no_ext.contains("não tem extensão"), "{no_ext}");
        let empty = validate_file("vazio.pdf", b"").unwrap_err().to_string();
        assert!(empty.contains("vazio"), "{empty}");
        let renamed = validate_file("programa.pdf", b"MZ\x90\x00")
            .unwrap_err()
            .to_string();
        assert!(renamed.contains("programa"), "{renamed}");
        let mismatch = validate_file("foto.jpg", b"%PDF-1.4")
            .unwrap_err()
            .to_string();
        assert!(
            mismatch.contains("não corresponde") && mismatch.contains(".jpg"),
            "{mismatch}"
        );
        let binary_text = validate_file("nota.txt", b"abc\0def")
            .unwrap_err()
            .to_string();
        assert!(binary_text.contains("não corresponde"), "{binary_text}");

        let big = vec![b'a'; MAX_ATTACHMENT_BYTES + 1];
        let too_big = validate_file("grande.txt", &big).unwrap_err().to_string();
        assert!(
            too_big.contains("8 MB") && too_big.contains("divida"),
            "{too_big}"
        );
        assert!(size_message("x.pdf", 13_000_000_u64).contains("12,4 MB"));

        assert!(validate_description(&"x".repeat(MAX_DESCRIPTION_CHARS)).is_ok());
        assert!(validate_description(&"x".repeat(MAX_DESCRIPTION_CHARS + 1))
            .unwrap_err()
            .to_string()
            .contains("Resuma"));
        // Separators and reserved characters become "_"; leading dots and spaces go.
        assert_eq!(sanitize_file_name("  ../ofi:cio?.pdf "), "_ofi_cio_.pdf");
        assert_eq!(sanitize_file_name("..."), "anexo");
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn drop_goes_to_a_block_only_when_it_is_the_only_one() {
        assert_eq!(drop_target(0), DropTarget::Nobody);
        assert_eq!(drop_target(1), DropTarget::Block);
        assert_eq!(drop_target(2), DropTarget::Ambiguous);
        assert_eq!(drop_target(5), DropTarget::Ambiguous);
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn drop_summary_reports_counts_limit_and_failures() {
        let ok = |n: &str| (n.to_owned(), Ok(format!("Anexado: “{n}”.")));
        let bad = |n: &str, e: &str| (n.to_owned(), Err(e.to_owned()));
        assert_eq!(
            drop_summary(&[ok("a.pdf")], 0),
            ("Anexado: “a.pdf”.".to_owned(), false)
        );
        assert_eq!(
            drop_summary(&[bad("a.pdf", "Vazio.")], 0),
            ("Vazio.".to_owned(), true)
        );
        assert_eq!(
            drop_summary(&[ok("a.pdf"), ok("b.pdf")], 0),
            ("2 anexados.".to_owned(), false)
        );
        let (text, error) = drop_summary(&[ok("a.pdf"), bad("b.pdf", "Vazio.")], 0);
        assert_eq!(text, "1 anexado. “b.pdf”: Vazio.");
        assert!(error);
        let (text, error) = drop_summary(&[ok("a.pdf"), ok("b.pdf")], 3);
        assert_eq!(text, "2 anexados; os demais passam do limite de 20 anexos.");
        assert!(error);
        let (text, _) = drop_summary(&[bad("pasta", FOLDER_REFUSAL), ok("a.pdf")], 0);
        assert!(text.contains("“pasta”: Pastas não podem ser anexadas; solte os arquivos."));
        assert_eq!(
            drop_summary(&[bad("pasta", FOLDER_REFUSAL)], 0).0,
            FOLDER_REFUSAL
        );
    }

    #[test]
    fn trash_table_from_older_libraries_is_rebuilt_to_accept_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acervo.sqlite3");
        let document = {
            let mut store = Store::open(&path).unwrap();
            let receipt = store.import_capture(&sample_capture()).unwrap();
            // Put a task in the trash, then downgrade the table to the 0.10 shape.
            let task = store
                .save_task(&crate::tasks::Task::new(receipt.document_id))
                .unwrap();
            store
                .trash_item(crate::trash::Target {
                    kind: crate::trash::Kind::Task,
                    id: task,
                })
                .unwrap();
            let triggers = store
                .conn
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'ponte_%'",
                )
                .unwrap()
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            for name in triggers {
                store
                    .conn
                    .execute_batch(&format!("DROP TRIGGER {name};"))
                    .unwrap();
            }
            store
                .conn
                .execute_batch(
                    "CREATE TABLE trash_old(kind TEXT NOT NULL CHECK(kind IN ('document','capture','task')),item_id INTEGER NOT NULL,document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,deleted_at TEXT NOT NULL,PRIMARY KEY(kind,item_id));
                     INSERT INTO trash_old SELECT kind,item_id,document_id,deleted_at FROM trash;
                     DROP TABLE trash;
                     ALTER TABLE trash_old RENAME TO trash;",
                )
                .unwrap();
            receipt.document_id
        };
        let mut store = Store::open(&path).unwrap();
        let definition: String = store
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='trash'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(definition.contains("'attachment'"));
        // The row that was in the trash survived the rebuild.
        assert_eq!(store.trash_entries().unwrap().len(), 1);
        let owner = Owner::Document(document);
        let id = store
            .add_attachment(owner, "nota.txt", "", b"ok\n")
            .unwrap();
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Attachment,
                id,
            })
            .unwrap();
        assert_eq!(store.trash_entries().unwrap().len(), 2);
    }

    #[test]
    fn attachments_are_sealed_limited_deduplicated_trashed_and_restored() {
        let (_dir, mut store, document) = store_with_document();
        let owner = Owner::Document(document);
        let pdf = include_bytes!("../tests/fixtures/document.pdf").to_vec();
        let id = store
            .add_attachment(owner, "Ofício recebido.pdf", "cópia do e-mail", &pdf)
            .unwrap();
        assert!(id > 0);

        // Sealed: neither the bytes nor the name are visible in the database file.
        let blob: Vec<u8> = store
            .conn
            .query_row("SELECT payload FROM attachments WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!blob.windows(5).any(|w| w == b"%PDF-"));
        let meta_blob: Vec<u8> = store
            .conn
            .query_row("SELECT metadata FROM attachments WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!meta_blob.windows(6).any(|w| w == "recebido".as_bytes()));

        let listed = store.list_attachments(owner).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Ofício recebido.pdf");
        assert_eq!(listed[0].kind, "PDF");
        assert_eq!(listed[0].description, "cópia do e-mail");
        let (meta, bytes) = store.attachment_bytes(id).unwrap();
        assert_eq!(bytes, pdf);
        assert_eq!(meta.sha256, listed[0].sha256);

        // Same content twice is refused with the existing name.
        let duplicate = store
            .add_attachment(owner, "outro-nome.pdf", "", &pdf)
            .unwrap_err()
            .to_string();
        assert!(duplicate.contains("Ofício recebido.pdf"), "{duplicate}");

        // Description edits persist; invalid ones are refused.
        store
            .update_attachment_description(id, "versão assinada")
            .unwrap();
        assert_eq!(
            store.list_attachments(owner).unwrap()[0].description,
            "versão assinada"
        );
        assert!(store
            .update_attachment_description(id, &"x".repeat(MAX_DESCRIPTION_CHARS + 1))
            .is_err());

        // Trash hides it, keeps the bytes, and restore brings it back.
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Attachment,
            id,
        };
        let plan = store.trash_plan(target.clone()).unwrap();
        assert!(plan.blocker.is_none());
        assert!(plan.label.contains("Ofício recebido.pdf"));
        assert_eq!(store.trash_item(target.clone()).unwrap(), Some(document));
        assert!(store.list_attachments(owner).unwrap().is_empty());
        assert_eq!(
            store.attachment_counts("document").unwrap().get(&document),
            None
        );
        assert!(store.attachment_bytes(id).is_ok());
        assert!(store.update_attachment_description(id, "x").is_err());
        let entries = store.trash_entries().unwrap();
        assert!(entries
            .iter()
            .any(|e| e.target.kind == crate::trash::Kind::Attachment && e.target.id == id));
        store.restore_item(target).unwrap();
        assert_eq!(store.list_attachments(owner).unwrap().len(), 1);
        assert_eq!(store.attachment_counts("document").unwrap()[&document], 1);

        // Per-owner limit, with guidance.
        for i in 0..(MAX_PER_OWNER - 1) {
            store
                .add_attachment(
                    owner,
                    &format!("nota-{i}.txt"),
                    "",
                    format!("nota {i}\n").as_bytes(),
                )
                .unwrap();
        }
        let full = store
            .add_attachment(owner, "mais-uma.txt", "", b"mais\n")
            .unwrap_err()
            .to_string();
        assert!(
            full.contains("máximo") && full.contains("Lixeira"),
            "{full}"
        );

        // Tasks have their own list; a trashed owner refuses new attachments.
        let task = store.save_task(&crate::tasks::Task::new(document)).unwrap();
        store
            .add_attachment(Owner::Task(task), "checklist.txt", "", b"ok\n")
            .unwrap();
        assert_eq!(store.list_attachments(Owner::Task(task)).unwrap().len(), 1);
        assert_eq!(store.attachment_counts("task").unwrap()[&task], 1);
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id: task,
            })
            .unwrap();
        assert!(store
            .add_attachment(Owner::Task(task), "depois.txt", "", b"x\n")
            .is_err());

        // Temp copy and export carry the exact bytes.
        let copy = store.attachment_temp_copy(id).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), pdf);
        clear_temp_dir();
        assert!(!copy.exists());
        let export = _dir.path().join("copia.pdf");
        store.export_attachment(id, &export).unwrap();
        assert_eq!(std::fs::read(&export).unwrap(), pdf);
        // Exporting over an existing file replaces it whole and leaves no temporary file.
        std::fs::write(&export, b"anterior").unwrap();
        store.export_attachment(id, &export).unwrap();
        assert_eq!(std::fs::read(&export).unwrap(), pdf);
        assert_eq!(
            std::fs::read_dir(_dir.path())
                .unwrap()
                .filter(|e| e
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp"))
                .count(),
            0
        );
        // A destination that cannot be replaced fails without touching anything else.
        let blocked = _dir.path().join("pasta.pdf");
        std::fs::create_dir(&blocked).unwrap();
        assert!(store.export_attachment(id, &blocked).is_err());
        assert!(blocked.is_dir());
    }

    #[test]
    fn working_copies_live_in_the_local_profile_not_in_the_library_folder() {
        let dir = production_temp_dir().unwrap();
        assert!(dir.ends_with(std::path::Path::new("PonteProj").join("temp")));
        let local = directories::BaseDirs::new().unwrap();
        assert!(dir.starts_with(local.data_local_dir()));
        // And the test folder is never the real one.
        assert!(!temp_dir().unwrap().starts_with(&dir));
    }

    fn root_for_test(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ponteproj-sessions-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn each_session_keeps_its_working_copies_in_a_private_folder() {
        let root = root_for_test("own");
        let own = session_dir_in(&root).unwrap();
        assert_ne!(own, root);
        assert_eq!(own.parent().unwrap(), root);
        // Another window (a dead or live session folder) in the same root.
        let other = root.join("sessao-outra");
        std::fs::create_dir_all(&other).unwrap();
        let theirs = other.join(temp_file_name(7, "peticao.pdf"));
        std::fs::write(&theirs, b"outra biblioteca").unwrap();
        let mine = own.join(temp_file_name(7, "peticao.pdf"));
        std::fs::write(&mine, b"esta biblioteca").unwrap();
        assert_ne!(mine, theirs);
        // Closing this session removes only its own folder.
        remove_session_dir(&own);
        assert!(!own.exists());
        assert_eq!(std::fs::read(&theirs).unwrap(), b"outra biblioteca");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_start_sweep_spares_live_and_recent_sessions() {
        let root = root_for_test("sweep");
        let own = session_dir_in(&root).unwrap();
        let own_file = own.join("1-a.txt");
        std::fs::write(&own_file, b"x").unwrap();

        // A live window of another process holds its lock open.
        let live = root.join("sessao-viva");
        std::fs::create_dir_all(&live).unwrap();
        let live_lock = open_session_lock(&live, true);
        assert!(live_lock.is_some());
        std::fs::write(live.join("2-b.txt"), b"x").unwrap();

        // A crashed one left its folder and a released lock.
        let dead = root.join("sessao-morta");
        std::fs::create_dir_all(&dead).unwrap();
        drop(open_session_lock(&dead, true));
        std::fs::write(dead.join("3-c.txt"), b"x").unwrap();

        // A folder without a lock is only removed once old; this one is new.
        let fresh = root.join("sessao-recente");
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::write(fresh.join("4-d.txt"), b"x").unwrap();

        // Unrelated things in the same root stay; loose copies of old versions go.
        std::fs::write(root.join("notas.txt"), b"x").unwrap();
        std::fs::write(root.join("5-antigo.pdf"), b"x").unwrap();
        std::fs::create_dir_all(root.join("outra-pasta")).unwrap();

        sweep_stale_in(&root);
        assert!(own_file.exists(), "own session untouched");
        #[cfg(windows)]
        assert!(live.join("2-b.txt").exists(), "live window untouched");
        assert!(!dead.exists(), "abandoned session removed");
        assert!(
            fresh.join("4-d.txt").exists(),
            "recent unlocked folder kept"
        );
        assert!(root.join("notas.txt").exists());
        assert!(root.join("outra-pasta").is_dir());
        assert!(!root.join("5-antigo.pdf").exists());
        drop(live_lock);
        remove_session_dir(&own);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_legacy_library_folder_only_loses_files_named_like_working_copies() {
        let dir = root_for_test("legacy");
        for name in [
            "12-peticao.pdf",
            "3-sem extensão",
            "ata.docx",
            "readme.txt",
            "-x.pdf",
            "abc-1.pdf",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::create_dir_all(dir.join("9-pasta")).unwrap();
        sweep_legacy_in(&dir);
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            ["-x.pdf", "9-pasta", "abc-1.pdf", "ata.docx", "readme.txt"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn temporary_names_fit_one_windows_path_component() {
        let units = |s: &str| s.encode_utf16().count();
        assert_eq!(temp_file_name(7, "peticao.pdf"), "7-peticao.pdf");
        // 180 astral characters = 360 UTF-16 units: too long once the id is prepended.
        let long = format!("{}.docx", "😀".repeat(174));
        let name = temp_file_name(123456, &long);
        assert!(units(&name) <= 255, "{}", units(&name));
        assert!(name.starts_with("123456-") && name.ends_with(".docx"));
        // A name at the 180-character limit made only of BMP letters still fits.
        let long = format!("{}.pdf", "a".repeat(176));
        let name = temp_file_name(99, &long);
        assert!(units(&name) <= 255 && name.ends_with(".pdf"));
        // Nothing left of the stem falls back to a neutral one.
        assert_eq!(temp_file_name(5, " .. "), "5-anexo");
    }
}
