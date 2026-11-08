//! Text of files attached to examples of a work template, so that an
//! approved example can show the AI the shape of a real reply.
//!
//! An attachment is a sealed file; its text is a separate, derived record
//! (`attachment_texts`), also sealed, tied to the file's SHA-256 and to the
//! version of the extractor that produced it. Change either and the text is
//! stale: it is extracted again, never trusted blindly. A person may read
//! and correct the extracted text ("conferido"), which then stands.
//!
//! Formats, in the order supported: PDF with a text layer (`pdf_extract`,
//! page by page), plain text (UTF-8 or Windows-1252) and Word `.docx`
//! (`word/document.xml` read with the ZIP reader of the spreadsheet import).
//! A scanned PDF yields no text: the status says OCR would be needed, and
//! nothing is sent to the AI for it.
use crate::{
    attachments::Owner,
    intranet::{record_mutation, IntranetMutation},
    protection::{seal, unseal},
    storage::Store,
};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// Bumped whenever the extraction changes in a way that should redo the
/// stored texts.
pub const EXTRACTOR_VERSION: i64 = 1;
/// Text kept per file; what the AI sees is capped again at assembly.
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_PAGES: usize = 300;
const MAX_DOCX_XML_BYTES: usize = 32 * 1024 * 1024;

/// How far the text of a file got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextStatus {
    /// Text extracted by the program.
    Extracted,
    /// The file has no text layer (a scanned PDF, an image): OCR would be
    /// needed, which this version does not do for example files.
    Empty,
    /// The format is not supported, or the file could not be read.
    #[default]
    Failed,
    /// A person read and confirmed (or corrected) the text.
    Reviewed,
}

impl TextStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TextStatus::Extracted => "extracted",
            TextStatus::Empty => "empty",
            TextStatus::Failed => "failed",
            TextStatus::Reviewed => "reviewed",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "extracted" => Ok(TextStatus::Extracted),
            "empty" => Ok(TextStatus::Empty),
            "failed" => Ok(TextStatus::Failed),
            "reviewed" => Ok(TextStatus::Reviewed),
            _ => bail!("Situação de texto inválida."),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            TextStatus::Extracted => "Texto extraído",
            TextStatus::Empty => "Sem texto (precisa de OCR)",
            TextStatus::Failed => "Extração falhou",
            TextStatus::Reviewed => "Texto conferido",
        }
    }
    /// Whether the text may be shown to the AI.
    pub fn usable(self) -> bool {
        matches!(self, TextStatus::Extracted | TextStatus::Reviewed)
    }
}

/// The text record of one attachment.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AttachmentText {
    #[serde(default)]
    pub attachment_id: i64,
    /// SHA-256 of the file the text came from.
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub extractor: String,
    #[serde(default)]
    pub extractor_version: i64,
    #[serde(default)]
    pub status: TextStatus,
    pub text: String,
    /// Number of pages (PDF) or 0.
    #[serde(default)]
    pub pages: usize,
    /// What happened, in words ("3 páginas sem texto").
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub reviewed_by: String,
    #[serde(default)]
    pub created_at: String,
}

impl AttachmentText {
    /// Whether this record still describes the file: same bytes, same
    /// extractor. A reviewed text stays valid for the same bytes.
    pub fn current_for(&self, sha256: &str) -> bool {
        self.sha256 == sha256
            && (self.status == TextStatus::Reviewed || self.extractor_version == EXTRACTOR_VERSION)
    }
}

/// Result of extracting one file, before it is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extracted {
    pub status: TextStatus,
    pub extractor: &'static str,
    pub text: String,
    pub pages: usize,
    pub note: String,
}

/// Extracts the text of a file by its name's extension and its bytes.
pub fn extract(name: &str, bytes: &[u8]) -> Extracted {
    let extension = name
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => extract_pdf(bytes),
        // E-mails (.eml) are raw MIME: headers with addresses and encoded
        // bodies would go to the AI as if they were text, so they wait for
        // a real decoder.
        "txt" | "csv" => extract_plain(bytes),
        "docx" => extract_docx(bytes),
        _ => Extracted {
            status: TextStatus::Failed,
            extractor: "none",
            text: String::new(),
            pages: 0,
            note: format!(
                "Arquivos .{extension} ainda não têm extração de texto; use PDF com texto, .txt ou .docx."
            ),
        },
    }
}

fn clip(mut text: String) -> (String, bool) {
    if text.len() <= MAX_TEXT_BYTES {
        return (text, false);
    }
    let mut end = MAX_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

fn extract_pdf(bytes: &[u8]) -> Extracted {
    let pages = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(bytes))
        .ok()
        .and_then(Result::ok);
    let Some(pages) = pages else {
        return Extracted {
            status: TextStatus::Failed,
            extractor: "pdf_extract",
            text: String::new(),
            pages: 0,
            note: "O PDF não pôde ser lido.".into(),
        };
    };
    let total = pages.len();
    let mut text = String::new();
    let mut blank = 0usize;
    for (index, page) in pages.iter().enumerate().take(MAX_PAGES) {
        if page.trim().is_empty() {
            blank += 1;
            continue;
        }
        text.push_str(&format!("[Página {}]\n{}\n\n", index + 1, page.trim()));
    }
    let (text, clipped) = clip(text);
    let mut notes = Vec::new();
    if total > MAX_PAGES {
        notes.push(format!("só as primeiras {MAX_PAGES} de {total} páginas"));
    }
    if blank > 0 {
        notes.push(format!("{blank} página(s) sem camada de texto"));
    }
    if clipped {
        notes.push(format!("texto cortado em {} KiB", MAX_TEXT_BYTES / 1024));
    }
    let status = if text.trim().is_empty() {
        TextStatus::Empty
    } else {
        TextStatus::Extracted
    };
    Extracted {
        status,
        extractor: "pdf_extract",
        text,
        pages: total,
        note: notes.join("; "),
    }
}

fn extract_plain(bytes: &[u8]) -> Extracted {
    // Notepad's "Unicode" and PowerShell 5 redirection write UTF-16 with a
    // byte-order mark.
    let utf16 = |little: bool, body: &[u8]| -> String {
        let units: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    let (decoded, note) = if let Some(body) = bytes.strip_prefix(b"\xFF\xFE") {
        (utf16(true, body), "lido como UTF-16".to_owned())
    } else if let Some(body) = bytes.strip_prefix(b"\xFE\xFF") {
        (utf16(false, body), "lido como UTF-16".to_owned())
    } else {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        match std::str::from_utf8(bytes) {
            Ok(text) => (text.to_owned(), String::new()),
            // Windows-1252 is what Notepad and old systems write; every
            // byte is a character, so the mapping is total.
            Err(_) => (
                bytes.iter().map(|&b| windows_1252(b)).collect::<String>(),
                "lido como Windows-1252".to_owned(),
            ),
        }
    };
    let decoded = decoded.replace('\0', "");
    let text = decoded.replace("\r\n", "\n").replace('\r', "\n");
    let (text, clipped) = clip(text);
    let mut notes: Vec<String> = Vec::new();
    if !note.is_empty() {
        notes.push(note);
    }
    if clipped {
        notes.push(format!("texto cortado em {} KiB", MAX_TEXT_BYTES / 1024));
    }
    Extracted {
        status: if text.trim().is_empty() {
            TextStatus::Empty
        } else {
            TextStatus::Extracted
        },
        extractor: "plain",
        text,
        pages: 0,
        note: notes.join("; "),
    }
}

fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9f => HIGH[(byte - 0x80) as usize],
        _ => byte as char,
    }
}

fn extract_docx(bytes: &[u8]) -> Extracted {
    let failed = |note: String| Extracted {
        status: TextStatus::Failed,
        extractor: "docx",
        text: String::new(),
        pages: 0,
        note,
    };
    let entries = match crate::saiop::read_zip_entries(bytes) {
        Ok(entries) => entries,
        Err(error) => return failed(format!("O .docx não pôde ser aberto: {error}")),
    };
    let Some(xml) = entries.get("word/document.xml") else {
        return failed("O .docx não contém word/document.xml.".into());
    };
    if xml.len() > MAX_DOCX_XML_BYTES {
        return failed("O conteúdo do .docx é grande demais.".into());
    }
    let xml = String::from_utf8_lossy(xml);
    let text = docx_xml_text(&xml);
    let (text, clipped) = clip(text);
    Extracted {
        status: if text.trim().is_empty() {
            TextStatus::Empty
        } else {
            TextStatus::Extracted
        },
        extractor: "docx",
        text,
        pages: 0,
        note: if clipped {
            format!("texto cortado em {} KiB", MAX_TEXT_BYTES / 1024)
        } else {
            String::new()
        },
    }
}

/// Paragraphs, tabs, breaks and table cells of a WordprocessingML body, as
/// plain text. Tags are dropped; entities decoded.
fn docx_xml_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    // Only the content of <w:t> is text; whitespace between tags is layout.
    let mut in_text = false;
    let mut in_cell = 0usize;
    // <w:tab/> is a tab only inside a run (<w:r>); inside <w:tabs> it is a
    // tab-stop definition. <mc:Fallback> repeats what <mc:Choice> holds.
    let mut in_run = false;
    let mut in_fallback = 0usize;
    while let Some(start) = rest.find('<') {
        if in_text && in_fallback == 0 {
            push_decoded(&mut out, &rest[..start]);
        }
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start + 1..start + end];
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        if name == "mc:Fallback" {
            if closing {
                in_fallback = in_fallback.saturating_sub(1);
            } else if !self_closing {
                in_fallback += 1;
            }
            rest = &rest[start + end + 1..];
            continue;
        }
        if in_fallback > 0 {
            rest = &rest[start + end + 1..];
            continue;
        }
        match name {
            "w:t" => in_text = !closing && !self_closing,
            "w:r" => in_run = !closing && !self_closing,
            // Paragraphs inside a table cell are joined; the cell ends with a tab.
            "w:p" if closing && in_cell == 0 => out.push('\n'),
            "w:p" if closing => out.push(' '),
            "w:tab" if in_run && (self_closing || !closing) => out.push('\t'),
            "w:br" | "w:cr" if self_closing || !closing => out.push('\n'),
            "w:tc" if closing => {
                in_cell = in_cell.saturating_sub(1);
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push('\t');
            }
            "w:tc" => in_cell += 1,
            "w:tr" if closing => {
                while out.ends_with('\t') {
                    out.pop();
                }
                out.push('\n');
            }
            _ => {}
        }
        rest = &rest[start + end + 1..];
    }
    if in_text && in_fallback == 0 {
        push_decoded(&mut out, rest);
    }
    // Tidy: trailing tabs before newlines, runs of blank lines.
    let mut tidy = String::with_capacity(out.len());
    let mut blank_run = 0;
    for line in out.lines() {
        let line = line.trim_end_matches('\t');
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        tidy.push_str(line);
        tidy.push('\n');
    }
    tidy.trim().to_owned()
}

fn push_decoded(out: &mut String, text: &str) {
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find(';') else {
            out.push_str(&rest[start..]);
            return;
        };
        let entity = &rest[start + 1..start + end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix('#')
                .and_then(|number| {
                    if let Some(hex) = number.strip_prefix('x').or(number.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        number.parse().ok()
                    }
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[start..start + end + 1]),
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
}

// ---------------------------------------------------------------------------
// Storage

/// Snapshot row for the team sync.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachmentTextRow {
    pub attachment_id: i64,
    pub sha256: String,
    pub extractor: String,
    pub extractor_version: i64,
    pub status: String,
    pub value: AttachmentText,
    pub created_at: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub(crate) fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS attachment_texts(
            attachment_id INTEGER PRIMARY KEY REFERENCES attachments(id) ON DELETE CASCADE,
            sha256 TEXT NOT NULL,
            extractor TEXT NOT NULL,
            extractor_version INTEGER NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('extracted','empty','failed','reviewed')),
            payload BLOB NOT NULL,
            created_at TEXT NOT NULL
        );",
    )?;
    Ok(())
}

fn read_text(conn: &Connection, attachment_id: i64) -> Result<Option<AttachmentText>> {
    let row: Option<(String, String, i64, String, Vec<u8>, String)> = conn
        .query_row(
            "SELECT sha256,extractor,extractor_version,status,payload,created_at FROM attachment_texts WHERE attachment_id=?1",
            [attachment_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    let Some((sha256, extractor, extractor_version, status, blob, created_at)) = row else {
        return Ok(None);
    };
    let mut value: AttachmentText = serde_json::from_slice(&unseal(&blob)?)?;
    value.attachment_id = attachment_id;
    value.sha256 = sha256;
    value.extractor = extractor;
    value.extractor_version = extractor_version;
    value.status = TextStatus::parse(&status)?;
    value.created_at = created_at;
    Ok(Some(value))
}

fn write_text(conn: &Connection, value: &AttachmentText) -> Result<()> {
    conn.execute(
        "INSERT INTO attachment_texts(attachment_id,sha256,extractor,extractor_version,status,payload,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(attachment_id) DO UPDATE SET sha256=excluded.sha256, extractor=excluded.extractor,
             extractor_version=excluded.extractor_version, status=excluded.status,
             payload=excluded.payload, created_at=excluded.created_at",
        params![
            value.attachment_id,
            value.sha256,
            value.extractor,
            value.extractor_version,
            value.status.as_str(),
            seal(&serde_json::to_vec(value)?)?,
            value.created_at
        ],
    )?;
    Ok(())
}

/// A file of an example, with its usable text, as the prompt sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExampleFile {
    pub example_uid: String,
    pub name: String,
    pub text: String,
}

/// Only files of examples have their text kept: an attachment of a
/// document, task or atendimento is refused here.
fn require_example_file(conn: &Connection, attachment_id: i64) -> Result<()> {
    let owner: Option<String> = conn
        .query_row(
            "SELECT owner_kind FROM attachments WHERE id=?1",
            [attachment_id],
            |r| r.get(0),
        )
        .optional()?;
    match owner.as_deref() {
        Some("example") => Ok(()),
        Some(_) => bail!("Só arquivos de exemplos de modelos têm o texto extraído."),
        None => bail!("Anexo não encontrado."),
    }
}

impl Store {
    /// The stored text record of an attachment, if any (it may be stale:
    /// see [`AttachmentText::current_for`]).
    pub fn attachment_text(&self, attachment_id: i64) -> Result<Option<AttachmentText>> {
        read_text(&self.conn, attachment_id)
    }

    /// Extracts (or re-extracts) the text of an attachment and stores it.
    /// On a client computer the main one does it, from the same bytes.
    pub fn extract_attachment_text(&mut self, attachment_id: i64) -> Result<AttachmentText> {
        require_example_file(&self.conn, attachment_id)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::ExtractAttachmentText { attachment_id },
            )?;
            return self
                .attachment_text(attachment_id)?
                .context("O texto não voltou do computador principal.");
        }
        let (meta, bytes) = self.attachment_bytes(attachment_id)?;
        let extracted = extract(&meta.name, &bytes);
        let value = AttachmentText {
            attachment_id,
            sha256: meta.sha256.clone(),
            extractor: extracted.extractor.to_owned(),
            extractor_version: EXTRACTOR_VERSION,
            status: extracted.status,
            text: extracted.text,
            pages: extracted.pages,
            note: extracted.note,
            reviewed_by: String::new(),
            created_at: now(),
        };
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_text(&tx, &value)?;
        record_mutation(
            &tx,
            &actor,
            "attachment_text",
            attachment_id,
            None,
            "extract_attachment_text",
            None,
            &format!(
                "Texto de “{}”: {}{}",
                meta.name,
                value.status.label(),
                if value.note.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", value.note)
                }
            ),
        )?;
        tx.commit()?;
        Ok(value)
    }

    /// Stores the text as a person confirmed or corrected it.
    pub fn review_attachment_text(
        &mut self,
        attachment_id: i64,
        text: &str,
    ) -> Result<AttachmentText> {
        ensure!(
            !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES,
            "O texto conferido está vazio ou passa de {} KiB.",
            MAX_TEXT_BYTES / 1024
        );
        require_example_file(&self.conn, attachment_id)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::ReviewAttachmentText {
                    attachment_id,
                    text: text.to_owned(),
                },
            )?;
            return self
                .attachment_text(attachment_id)?
                .context("O texto não voltou do computador principal.");
        }
        let (meta, _) = self.attachment_bytes(attachment_id)?;
        let actor = self.active_actor();
        let previous = self.attachment_text(attachment_id)?;
        let value = AttachmentText {
            attachment_id,
            sha256: meta.sha256.clone(),
            extractor: previous
                .as_ref()
                .map(|p| p.extractor.clone())
                .unwrap_or_else(|| "manual".into()),
            extractor_version: EXTRACTOR_VERSION,
            status: TextStatus::Reviewed,
            text: text.replace("\r\n", "\n"),
            pages: previous.as_ref().map(|p| p.pages).unwrap_or(0),
            note: "conferido por uma pessoa".into(),
            reviewed_by: actor.clone(),
            created_at: now(),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_text(&tx, &value)?;
        record_mutation(
            &tx,
            &actor,
            "attachment_text",
            attachment_id,
            None,
            "review_attachment_text",
            None,
            &format!("Conferiu o texto de “{}”", meta.name),
        )?;
        tx.commit()?;
        Ok(value)
    }

    /// The files of an example whose text may go to the AI: text extracted
    /// or reviewed, for the bytes now attached. Stale records are left out
    /// (the panel offers to extract again).
    pub fn example_files(
        &self,
        example: &crate::work_templates::Example,
    ) -> Result<Vec<ExampleFile>> {
        let mut out = Vec::new();
        for meta in self.list_attachments(Owner::Example(example.id))? {
            if let Some(text) = self.attachment_text(meta.id)? {
                if text.status.usable() && text.current_for(&meta.sha256) {
                    out.push(ExampleFile {
                        example_uid: example.uid.clone(),
                        name: meta.name.clone(),
                        text: text.text,
                    });
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn export_attachment_texts(&self) -> Result<Vec<AttachmentTextRow>> {
        let mut statement = self.conn.prepare(
            "SELECT attachment_id,sha256,extractor,extractor_version,status,payload,created_at FROM attachment_texts ORDER BY attachment_id",
        )?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Vec<u8>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (attachment_id, sha256, extractor, extractor_version, status, blob, created_at) in rows
        {
            out.push(AttachmentTextRow {
                attachment_id,
                sha256,
                extractor,
                extractor_version,
                status,
                value: serde_json::from_slice(&unseal(&blob)?)?,
                created_at,
            });
        }
        Ok(out)
    }
}

pub(crate) fn apply_attachment_texts(tx: &Connection, rows: &[AttachmentTextRow]) -> Result<()> {
    for row in rows {
        tx.execute(
            "INSERT INTO attachment_texts(attachment_id,sha256,extractor,extractor_version,status,payload,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                row.attachment_id,
                row.sha256,
                row.extractor,
                row.extractor_version,
                row.status,
                seal(&serde_json::to_vec(&row.value)?)?,
                row.created_at
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ZIP with stored (uncompressed) entries, enough for a `.docx`.
    fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for &(name, data) in entries {
            let offset = out.len() as u32;
            let crc = crc32(data);
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0u8; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let central_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&central_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn plain_text_is_decoded_from_utf8_or_windows_1252() {
        let utf8 = extract("nota.txt", "Ofício nº 12\r\nSegunda linha".as_bytes());
        assert_eq!(utf8.status, TextStatus::Extracted);
        assert_eq!(utf8.text, "Ofício nº 12\nSegunda linha");
        let legacy = extract("nota.txt", b"Of\xedcio n\xba 12 \x96 urgente");
        assert_eq!(legacy.text, "Ofício nº 12 – urgente");
        assert!(legacy.note.contains("Windows-1252"));
        assert_eq!(extract("vazio.txt", b"   ").status, TextStatus::Empty);
        // Notepad's "Unicode": UTF-16 LE with a byte-order mark.
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "Ofício".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let wide = extract("nota.txt", &utf16);
        assert_eq!(wide.text, "Ofício");
        assert!(wide.note.contains("UTF-16"));
        // E-mails are not decoded yet: refused, not sent as raw MIME.
        assert_eq!(
            extract("msg.eml", b"From: a@b\r\n\r\nQm9t").status,
            TextStatus::Failed
        );
    }

    #[test]
    fn docx_paragraphs_tabs_and_tables_become_lines() {
        let xml = br#"<?xml version="1.0"?><w:document xmlns:w="x"><w:body>
<w:p><w:r><w:t>Primeiro par&#225;grafo &amp; mais</w:t></w:r></w:p>
<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:r><w:t xml:space="preserve">Com </w:t></w:r><w:r><w:tab/><w:t>tabula&#xE7;&#xE3;o</w:t></w:r></w:p>
<w:p><mc:AlternateContent><mc:Choice><w:r><w:t>Caixa de texto</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>Caixa de texto</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#;
        let docx = stored_zip(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("word/document.xml", xml),
        ]);
        let out = extract("modelo.docx", &docx);
        assert_eq!(out.status, TextStatus::Extracted, "{}", out.note);
        assert_eq!(
            out.text,
            "Primeiro parágrafo & mais\nCom \ttabulação\nCaixa de texto\nA1\tB1"
        );
        let broken = extract("x.docx", b"not a zip at all, really not");
        assert_eq!(broken.status, TextStatus::Failed);
        assert_eq!(extract("foto.png", b"\x89PNG").status, TextStatus::Failed);
    }

    #[test]
    fn pdf_text_is_extracted_page_by_page() {
        let out = extract("doc.pdf", include_bytes!("../tests/fixtures/document.pdf"));
        assert_eq!(out.status, TextStatus::Extracted, "{}", out.note);
        assert!(out.text.starts_with("[Página 1]"));
        assert!(out.pages >= 1);
    }

    #[test]
    fn example_files_follow_the_bytes_and_the_extractor_version() {
        use crate::work_templates::{Example, ExampleStatus, Status, WorkTemplate};
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let template = store
            .save_template(&WorkTemplate::new("Ofício", "resposta", "Texto"), None)
            .unwrap();
        store
            .set_template_status(&template.uid, Status::Published, None)
            .unwrap();
        let example = store
            .save_example(
                &Example {
                    template_uid: template.uid.clone(),
                    title: "Caso".into(),
                    reply: "Resposta".into(),
                    status: ExampleStatus::Approved,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        let id = store
            .add_attachment(
                Owner::Example(example.id),
                "antigo.txt",
                "",
                b"SEGREDO_ARQUIVO texto do exemplo",
            )
            .unwrap();
        assert!(store.attachment_text(id).unwrap().is_none());
        assert!(
            store.example_files(&example).unwrap().is_empty(),
            "no text yet"
        );
        let text = store.extract_attachment_text(id).unwrap();
        assert_eq!(text.status, TextStatus::Extracted);
        let files = store.example_files(&example).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].text, "SEGREDO_ARQUIVO texto do exemplo");

        // An older extractor's record is stale until extracted again.
        store
            .conn
            .execute(
                "UPDATE attachment_texts SET extractor_version=0 WHERE attachment_id=?1",
                [id],
            )
            .unwrap();
        assert!(store.example_files(&example).unwrap().is_empty());
        store.extract_attachment_text(id).unwrap();
        assert_eq!(store.example_files(&example).unwrap().len(), 1);

        // A reviewed text stands, for the same bytes.
        let reviewed = store
            .review_attachment_text(id, "Texto corrigido pela pessoa")
            .unwrap();
        assert_eq!(reviewed.status, TextStatus::Reviewed);
        assert!(!reviewed.reviewed_by.is_empty());
        assert_eq!(
            store.example_files(&example).unwrap()[0].text,
            "Texto corrigido pela pessoa"
        );

        // Deleting the example removes its files and texts.
        store.delete_example(&example.uid).unwrap();
        assert!(store.attachment_text(id).unwrap().is_none());
        let left: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM attachments WHERE owner_kind='example'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        let raw = std::fs::read(dir.path().join("acervo.sqlite3")).unwrap();
        assert!(!raw
            .windows(b"SEGREDO_ARQUIVO".len())
            .any(|w| w == b"SEGREDO_ARQUIVO"));
    }
}
