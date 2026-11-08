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
            "páginas e arquivos de marcação podem conter scripts. Exporte o conteúdo como PDF e anexe o PDF."
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
        "O arquivo “{name}” está vazio (0 bytes). Confira se ele foi salvo corretamente antes de anexar."
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

/// Folder for temporary copies opened in other programs. Emptied when the
/// Ponte starts and when it closes.
pub fn temp_dir() -> Result<PathBuf> {
    let dir = crate::paths::data_dir()?.join("temp");
    std::fs::create_dir_all(&dir).context("Não foi possível criar a pasta temporária.")?;
    Ok(dir)
}

pub fn clear_temp_dir() {
    let Ok(dir) = temp_dir() else {
        return;
    };
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            // A file still open in another program stays; next start retries.
            let _ = std::fs::remove_file(entry.path());
        }
    }
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
            Owner::Task(id) => crate::trash::require_active_task(conn, id).map(Some),
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
                _ => bail!("Resposta inesperada do computador principal ao excluir o anexo."),
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
                _ => bail!("Resposta inesperada do computador principal ao anexar arquivo."),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let document = Self::attachment_owner_document(&tx, owner)?;
        let existing = list_active(&tx, owner)?;
        ensure!(
            existing.len() < MAX_PER_OWNER,
            "Este {} já tem {MAX_PER_OWNER} anexos, o máximo. Exclua um anexo que não seja mais necessário (ele vai para a Lixeira e pode ser restaurado) ou junte vários arquivos num único PDF.",
            owner.noun()
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
            added_by: actor.clone(),
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
            &tx,
            &actor,
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
        tx.commit()?;
        Ok(id)
    }

    /// Active attachments of one document, task or atendimento, oldest first.
    pub fn list_attachments(&self, owner: Owner) -> Result<Vec<AttachmentMeta>> {
        list_active(&self.conn, owner)
    }

    /// Number of active attachments per owner id of one kind ("document",
    /// "task" or "saiop"), for badges in lists.
    pub fn attachment_counts(&self, owner_kind: &str) -> Result<BTreeMap<i64, usize>> {
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
        let path = temp_dir()?.join(format!("{id}-{}", sanitize_file_name(&meta.name)));
        std::fs::write(&path, data).context("Não foi possível gravar a cópia temporária.")?;
        Ok(path)
    }

    /// Saves a copy where the person chose.
    pub fn export_attachment(&self, id: i64, destination: &Path) -> Result<()> {
        let (_, data) = self.attachment_bytes(id)?;
        std::fs::write(destination, data)
            .with_context(|| format!("Não foi possível gravar em {}.", destination.display()))?;
        Ok(())
    }
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
        use eframe::egui::{self, Color32, RichText};
        const WARNING: Color32 = Color32::from_rgb(153, 88, 16);
        const ERROR: Color32 = Color32::from_rgb(168, 40, 45);
        const MUTED: Color32 = Color32::from_rgb(120, 130, 140);

        let list = match store.list_attachments(owner) {
            Ok(list) => list,
            Err(error) => {
                ui.colored_label(WARNING, format!("Não foi possível ler os anexos: {error}"));
                return;
            }
        };
        let full = list.len() >= MAX_PER_OWNER;
        let mut pick = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!busy && !full, egui::Button::new("Anexar arquivo…"))
                .on_hover_text(format!("Escolhe um arquivo deste computador (até {}). Ele fica guardado protegido no acervo e visível para a equipe.", labels::file_size(MAX_ATTACHMENT_BYTES as u64)))
                .on_disabled_hover_text(if full { "Limite de anexos atingido. Exclua um anexo para abrir espaço." } else { "Aguarde a operação em andamento." })
                .clicked()
            {
                pick = true;
            }
            ui.small(format!("{} de {MAX_PER_OWNER}", list.len()));
            ui.label(RichText::new("ⓘ").color(MUTED)).on_hover_text(format!(
                "Até {MAX_PER_OWNER} anexos de até {} cada. Tipos aceitos: {ACCEPTED_TYPES_TEXT}. Programas, scripts, atalhos e arquivos compactados não são aceitos. Excluir um anexo o leva para a Lixeira, de onde pode ser restaurado.",
                labels::file_size(MAX_ATTACHMENT_BYTES as u64)
            ));
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
            let color = if self.message_is_error {
                ERROR
            } else {
                Color32::from_rgb(30, 110, 60)
            };
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(color, &self.message);
                if ui.small_button("✕").clicked() {
                    self.message.clear();
                }
            });
        }
        if list.is_empty() {
            ui.small(match owner {
                Owner::Document(_) => "Nenhum arquivo anexado a este documento.",
                Owner::Task(_) => "Nenhum arquivo anexado a esta tarefa.",
                Owner::Attendance(_) => "Nenhum arquivo anexado a este atendimento.",
                Owner::Example(_) => "Nenhum arquivo anexado a este exemplo.",
            });
        }
        let mut save_description: Option<(i64, String)> = None;
        for meta in &list {
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&meta.name);
                    ui.small(format!(
                        "{} • {} • por {} em {}",
                        meta.kind,
                        labels::file_size(meta.size as u64),
                        if meta.added_by.trim().is_empty() { "—" } else { meta.added_by.trim() },
                        labels::datetime(&meta.added_at)
                    ));
                });
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
                        if ui.add_enabled(!busy, egui::Button::new("Salvar descrição").small()).clicked()
                            || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            save_description = Some((meta.id, text.clone()));
                        }
                        if ui.small_button("Desfazer").clicked() {
                            text = meta.description.clone();
                        }
                    }
                });
                if text != meta.description {
                    self.descriptions.insert(meta.id, text);
                } else {
                    self.descriptions.remove(&meta.id);
                }
                ui.horizontal_wrapped(|ui| {
                    let is_pdf = meta.kind == "PDF";
                    let open_label = if is_pdf { "Ler no Ponte" } else { "Abrir" };
                    if ui
                        .add_enabled(!busy, egui::Button::new(open_label))
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
                    if ui
                        .add_enabled(!busy, egui::Button::new("Salvar como…"))
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
                    if owner.deletes_for_good() {
                        // Atendimentos and examples have no trash: the
                        // removal is final.
                        if ui
                            .add_enabled(!busy, egui::Button::new("Excluir de vez…"))
                            .on_hover_text("Apaga o anexo definitivamente (anexos de atendimentos e de exemplos não vão para a Lixeira).")
                            .clicked()
                        {
                            self.confirm_delete = Some(meta.id);
                        }
                    } else if ui
                        .add_enabled(!busy, egui::Button::new("Excluir…"))
                        .on_hover_text("Move o anexo para a Lixeira; pode ser restaurado depois.")
                        .clicked()
                    {
                        self.trash_request = Some(crate::trash::Target {
                            kind: crate::trash::Kind::Attachment,
                            id: meta.id,
                        });
                    }
                    ui.small(format!("identificação {}…", &meta.sha256[..meta.sha256.len().min(12)]))
                        .on_hover_text(format!("SHA-256 completo: {}", meta.sha256));
                });
                if self.confirm_delete == Some(meta.id) {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(WARNING, format!("Apagar “{}” de vez? Não há como restaurar.", meta.name));
                        if ui.add_enabled(!busy, egui::Button::new("Apagar de vez").small()).clicked() {
                            self.confirm_delete = None;
                            match store.delete_attachment(meta.id) {
                                Ok(()) => self.notice(format!("Anexo “{}” apagado.", meta.name), false),
                                Err(error) => self.notice(error.to_string(), true),
                            }
                        }
                        if ui.small_button("Cancelar").clicked() {
                            self.confirm_delete = None;
                        }
                    });
                }
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
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Check the size before reading, so a huge file is refused at once.
        match std::fs::metadata(path) {
            Ok(info) if info.len() > MAX_ATTACHMENT_BYTES as u64 => {
                self.notice(size_message(&name, info.len()), true);
                return;
            }
            Ok(_) => {}
            Err(error) => {
                self.notice(format!("Não foi possível ler “{name}”: {error}"), true);
                return;
            }
        }
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) => {
                self.notice(format!("Não foi possível ler “{name}”: {error}"), true);
                return;
            }
        };
        match store.add_attachment(owner, &name, "", &data) {
            Ok(_) => self.notice(
                format!(
                    "Anexado: “{name}” ({}). Se quiser, escreva uma descrição.",
                    labels::file_size(data.len() as u64)
                ),
                false,
            ),
            Err(error) => self.notice(error.to_string(), true),
        }
    }
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
        assert_eq!(store.trash_item(target.clone()).unwrap(), document);
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
        let export = _dir.path().join("copia.pdf");
        store.export_attachment(id, &export).unwrap();
        assert_eq!(std::fs::read(&export).unwrap(), pdf);
    }
}
