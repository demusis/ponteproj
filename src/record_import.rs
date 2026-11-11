//! A record created together with its attachments, all or nothing: the
//! files are validated before the record exists, and a record whose
//! attachment still fails goes to the Lixeira.
//!
//! The input of this module may come from an untrusted web page (through the
//! browser extension), so everything that can be checked without the store is
//! checked first, and the messages of the errors are fixed Portuguese
//! sentences (never a raw store error, which can carry names or paths).
use crate::{
    attachments::{sanitize_file_name, validate_description, validate_file, Owner, MAX_PER_OWNER},
    manual::ManualFields,
    model::{ImportOutcome, MANUAL_PENDING},
    storage::Store,
    trash::{Kind, Target},
};
use anyhow::{Error, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// All the attachments of one request together (decoded bytes).
pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 16 * 1024 * 1024;
/// Longest metadata value, in characters.
pub const MAX_META_VALUE_CHARS: usize = 200;
const MAX_META_KEY_CHARS: usize = 64;
/// The core allows 256 keys; the record adds five of its own.
const MAX_META_KEYS: usize = 200;
/// Keys the record itself writes (or that a manual record may not have).
const RESERVED_META: [&str; 7] = [
    "manual_kind",
    "document_type",
    "origin",
    "document_date",
    "registered_by",
    "movement_history",
    "movement_source",
];

/// Why a record was not created. `Display` is always a sentence the person
/// can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// The record itself (fields, metadata, request id) is not acceptable.
    Invalid(String),
    /// A file is not acceptable (or all together are too many or too big).
    Attachment(String),
    /// Everything was acceptable but the library could not store it.
    Failed(String),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::Invalid(m) | Self::Attachment(m) | Self::Failed(m)) = self;
        f.write_str(m)
    }
}
impl std::error::Error for RecordError {}

/// What to store: the fields the person would type, extra metadata, and the
/// request id that makes a repeated save answer "already there".
#[derive(Clone, Debug, Default)]
pub struct NewRecord {
    pub fields: ManualFields,
    pub metadata: BTreeMap<String, String>,
    /// A UUID (see `whatsapp_request_id`).
    pub request_id: String,
}

#[derive(Clone, Debug, Default)]
pub struct NewAttachment {
    pub name: String,
    pub bytes: Vec<u8>,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Created {
    New {
        document_id: i64,
        attachments_added: usize,
    },
    /// The same content (same request id) was saved before: nothing was
    /// stored now and no attachment was added again.
    Already { document_id: i64 },
}

/// The request id of a WhatsApp conversation: a UUID (version 8, like the
/// e-mail ones) taken from the first 16 bytes of the SHA-256 of
/// `"whatsapp\n" + origin + "\n" + text`, where the text has its line ends
/// as `\n` (`\r\n` and lone `\r` become `\n`) and the spaces and tabs at the
/// end of each line removed. Nothing else is changed (no trim of the whole).
pub fn whatsapp_request_id(origin: &str, text: &str) -> String {
    // One formula for the window and for the extension: the reader's.
    let id = crate::whatsapp::request_id(origin, text);
    debug_assert_eq!(id, local_request_id(origin, text));
    id
}

/// The formula of the contract written out here too; the tests pin both.
fn local_request_id(origin: &str, text: &str) -> String {
    let digest = whatsapp_digest(origin, text);
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&digest[..16]);
    raw[6] = (raw[6] & 0x0f) | 0x80;
    raw[8] = (raw[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(raw).to_string()
}

fn whatsapp_digest(origin: &str, text: &str) -> [u8; 32] {
    // The same rules as `crate::whatsapp::request_id`: CRLF as LF, every
    // line without its trailing whitespace.
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let body: Vec<&str> = normalised.split('\n').map(str::trim_end).collect();
    let mut hasher = Sha256::new();
    hasher.update(b"whatsapp\n");
    hasher.update(origin.as_bytes());
    hasher.update(b"\n");
    hasher.update(body.join("\n").as_bytes());
    hasher.finalize().into()
}

fn invalid(message: &str) -> Error {
    RecordError::Invalid(message.to_owned()).into()
}

/// Fields and metadata, without the store.
pub fn validate_record(record: &NewRecord) -> Result<()> {
    record
        .fields
        .validate()
        .map_err(|e| Error::from(RecordError::Invalid(e.to_string())))?;
    uuid::Uuid::parse_str(&record.request_id)
        .map_err(|_| invalid("O identificador da importação é inválido."))?;
    if record.metadata.len() > MAX_META_KEYS {
        return Err(invalid("O registro traz informações demais."));
    }
    for (key, value) in &record.metadata {
        let key_ok = !key.is_empty()
            && key.chars().count() <= MAX_META_KEY_CHARS
            && !key.chars().any(char::is_control)
            && !RESERVED_META.contains(&key.as_str());
        let value_ok =
            value.chars().count() <= MAX_META_VALUE_CHARS && !value.chars().any(char::is_control);
        if !key_ok || !value_ok {
            return Err(invalid(
                "O registro traz uma informação inválida ou grande demais.",
            ));
        }
    }
    Ok(())
}

/// Everything that can be known about the files without the store: how many,
/// how big (each and together), name, type by content, description.
pub fn validate_attachments(files: &[NewAttachment]) -> Result<()> {
    prepared(files).map(|_| ())
}

fn refuse_file(message: String) -> Error {
    RecordError::Attachment(message).into()
}

/// The files to store: names sanitised, checked, identical contents once.
fn prepared(files: &[NewAttachment]) -> Result<Vec<(String, &NewAttachment)>> {
    if files.len() > MAX_PER_OWNER {
        return Err(refuse_file(format!(
            "São arquivos demais ({}); o máximo é {MAX_PER_OWNER} por registro.",
            files.len()
        )));
    }
    let total: usize = files.iter().map(|f| f.bytes.len()).sum();
    if total > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(refuse_file(format!(
            "Os arquivos juntos passam de {} MB, o máximo por registro.",
            MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024)
        )));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for file in files {
        let name = sanitize_file_name(&file.name);
        validate_file(&name, &file.bytes).map_err(|e| refuse_file(e.to_string()))?;
        validate_description(&file.description).map_err(|e| refuse_file(e.to_string()))?;
        // The same content twice in one request is stored once.
        if seen.insert(format!("{:x}", Sha256::digest(&file.bytes))) {
            out.push((name, file));
        }
    }
    Ok(out)
}

fn failed(message: &str) -> Error {
    RecordError::Failed(message.to_owned()).into()
}

/// A fixed sentence for a store refusal; the text of the error is passed on
/// only when it is one of the application's own sentences about the main
/// computer (they carry no names).
fn store_refusal(err: &Error) -> Error {
    let text = err.to_string();
    if text.contains("lixeira") {
        return failed("Esta conversa já foi guardada antes e está na Lixeira. Restaure-a de lá para vê-la no Ponte.");
    }
    if text.contains("computador principal") && text.chars().count() < 300 {
        return failed(&text);
    }
    failed("O Ponte não conseguiu guardar o registro no acervo. Tente de novo.")
}

impl Store {
    /// Creates a manual record with its attachments, all or nothing.
    ///
    /// 1. Everything that does not need the store is checked first.
    /// 2. The record is created like a manual one, with the given request id:
    ///    the same id again answers `Already` (nothing is added).
    /// 3. The attachments are added; if one fails, the record goes to the
    ///    Lixeira and the error is returned.
    ///
    /// In team client mode the record travels like any manual one
    /// (`import_capture`): the main computer numbers it, or the change is
    /// queued while it is unreachable; attachments and the Lixeira step use
    /// the same relay (`add_attachment`, `trash_item`).
    pub fn create_record_with_attachments(
        &mut self,
        record: &NewRecord,
        files: &[NewAttachment],
    ) -> Result<Created> {
        validate_record(record)?;
        let files = prepared(files)?;
        let actor = self.active_actor();
        let capture = record.fields.capture_with(
            MANUAL_PENDING,
            &actor,
            record.request_id.clone(),
            record.metadata.clone(),
        );
        let receipt = self
            .import_capture(&capture)
            .map_err(|e| store_refusal(&e))?;
        if receipt.effective_outcome() != ImportOutcome::Imported {
            return Ok(Created::Already {
                document_id: receipt.document_id,
            });
        }
        let document_id = receipt.document_id;
        let mut added = 0;
        for (name, file) in files {
            let result = self.add_attachment(
                Owner::Document(document_id),
                &name,
                file.description.trim(),
                &file.bytes,
            );
            if result.is_err() {
                let target = Target {
                    kind: Kind::Document,
                    id: document_id,
                };
                let trashed = self.trash_item(target.clone());
                // The record was made a moment ago by this very request and
                // nobody has seen it: it is removed for good, so that trying
                // again starts clean. If that cannot be done it stays in the
                // Lixeira and the person is told.
                let removed = trashed.is_ok() && self.purge_trash_item(target).is_ok();
                if removed {
                    return Err(failed("Um dos arquivos não pôde ser guardado, então a conversa não foi guardada. Tente de novo."));
                }
                return Err(match trashed {
                    Ok(_) => failed("Um dos arquivos não pôde ser guardado, então a conversa não foi guardada. Ela foi para a Lixeira; tente de novo."),
                    Err(_) => failed("Um dos arquivos não pôde ser guardado e o registro ficou incompleto. Exclua-o na lista de documentos e tente de novo."),
                });
            }
            added += 1;
        }
        Ok(Created::New {
            document_id,
            attachments_added: added,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        (dir, store)
    }

    fn record(text: &str) -> NewRecord {
        let fields = ManualFields {
            kind: "whatsapp".into(),
            title: "Conversa com Servidor A — 04/11/2026".into(),
            origin: "Servidor A".into(),
            document_date: "2026-11-04".into(),
            text: text.into(),
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("imported_from".to_owned(), "whatsapp-web".to_owned());
        metadata.insert("wa_messages".to_owned(), "1".to_owned());
        NewRecord {
            request_id: whatsapp_request_id(&fields.origin, &fields.text),
            fields,
            metadata,
        }
    }

    fn png(seed: u8) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', seed, 1, 2, 3]);
        bytes
    }
    fn pdf(seed: u8) -> Vec<u8> {
        let mut bytes = b"%PDF-1.4\n".to_vec();
        bytes.extend_from_slice(format!("% {seed}\n%%EOF\n").as_bytes());
        bytes
    }
    fn file(name: &str, bytes: Vec<u8>) -> NewAttachment {
        NewAttachment {
            name: name.into(),
            bytes,
            description: String::new(),
        }
    }
    const TEXT: &str = "[04/11/2026 10:00] Servidor A: bom dia";

    fn documents(store: &Store) -> usize {
        store.list_documents("").unwrap().len()
    }

    #[test]
    fn request_id_pins_the_documented_formula() {
        // Definition: SHA-256 of "whatsapp\n" + origin + "\n" + text (line
        // ends as \n, spaces/tabs at the end of lines removed), first 16
        // bytes, version 8 / RFC 4122 variant bits.
        let first = whatsapp_request_id("Servidor A", "[04/11/2026 10:00] Servidor A: bom dia");
        let second = whatsapp_request_id(
            "Grupo da Equipe B",
            "[04/11/2026 10:00] Servidor A: oi\nsegunda linha\n[04/11/2026 10:01] \u{2014} mensagem do sistema",
        );
        assert_eq!(first, "c1889eab-0032-8d12-a2a0-6d9737bdc3a1");
        assert_eq!(second, "534f2b77-2a53-892c-87c0-85ac665a5e0d");
        // The digest itself, from an independent computation.
        let mut hasher = Sha256::new();
        hasher.update(b"whatsapp\nServidor A\n[04/11/2026 10:00] Servidor A: bom dia");
        let digest: [u8; 32] = hasher.finalize().into();
        assert_eq!(
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "c1889eab0032bd12a2a06d9737bdc3a1417d0efcd265e77ebb3a1bcccbc9d824"
        );
        assert_eq!(
            digest,
            whatsapp_digest("Servidor A", "[04/11/2026 10:00] Servidor A: bom dia")
        );
        // Line ends and trailing spaces do not change it; content does.
        assert_eq!(
            whatsapp_request_id("Grupo da Equipe B", "a  \r\nb\t\r\nc"),
            whatsapp_request_id("Grupo da Equipe B", "a\nb\nc")
        );
        assert_eq!(
            whatsapp_request_id("x", "a\rb"),
            whatsapp_request_id("x", "a\nb")
        );
        assert_ne!(
            whatsapp_request_id("x", "a\nb"),
            whatsapp_request_id("y", "a\nb")
        );
        assert_ne!(
            whatsapp_request_id("x", " a"),
            whatsapp_request_id("x", "a")
        );
        assert!(uuid::Uuid::parse_str(&first).is_ok());
    }

    #[test]
    fn a_new_record_without_files_and_with_two_files() {
        let (_dir, mut store) = store();
        let created = store
            .create_record_with_attachments(&record(TEXT), &[])
            .unwrap();
        let Created::New {
            document_id,
            attachments_added,
        } = created
        else {
            panic!("expected a new record")
        };
        assert_eq!(attachments_added, 0);
        let detail = store.load_document(document_id).unwrap();
        assert!(detail.summary.manual);
        assert_eq!(detail.capture.metadata["manual_kind"], "whatsapp");
        assert_eq!(detail.capture.metadata["imported_from"], "whatsapp-web");
        assert_eq!(detail.capture.metadata["origin"], "Servidor A");
        assert_eq!(detail.capture.text, TEXT);

        let two = record("[04/11/2026 11:00] Servidor B: veja");
        let created = store
            .create_record_with_attachments(
                &two,
                &[file("foto:1.png", png(1)), file("nota.pdf", pdf(1))],
            )
            .unwrap();
        let Created::New {
            document_id,
            attachments_added,
        } = created
        else {
            panic!("expected a new record")
        };
        assert_eq!(attachments_added, 2);
        let list = store
            .list_attachments(Owner::Document(document_id))
            .unwrap();
        let names: Vec<&str> = list.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["foto_1.png", "nota.pdf"], "names are sanitised");
        assert_eq!(documents(&store), 2);
    }

    #[test]
    fn the_same_request_id_answers_already_and_adds_nothing() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);
        let first = store
            .create_record_with_attachments(&rec, &[file("a.png", png(1))])
            .unwrap();
        let Created::New { document_id, .. } = first else {
            panic!()
        };
        let again = store
            .create_record_with_attachments(&rec, &[file("a.png", png(1)), file("b.pdf", pdf(2))])
            .unwrap();
        assert_eq!(again, Created::Already { document_id });
        assert_eq!(
            store
                .list_attachments(Owner::Document(document_id))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(documents(&store), 1);
        // Same text with other trailing spaces: the same identity.
        let spaced = NewRecord {
            request_id: whatsapp_request_id(
                "Servidor A",
                "[04/11/2026 10:00] Servidor A: bom dia  ",
            ),
            ..rec.clone()
        };
        assert_eq!(
            store.create_record_with_attachments(&spaced, &[]).unwrap(),
            Created::Already { document_id }
        );
        // The same file twice in one request is stored once.
        let other = record("[04/11/2026 12:00] Servidor A: duas vezes");
        let Created::New {
            attachments_added, ..
        } = store
            .create_record_with_attachments(
                &other,
                &[file("a.png", png(3)), file("copia.png", png(3))],
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(attachments_added, 1);
    }

    fn assert_refused(store: &mut Store, rec: &NewRecord, files: &[NewAttachment]) -> RecordError {
        let err = store
            .create_record_with_attachments(rec, files)
            .unwrap_err();
        assert_eq!(documents(store), 0, "a refusal leaves no record: {err}");
        assert!(store.trash_entries().unwrap().is_empty());
        err.downcast_ref::<RecordError>().cloned().unwrap()
    }

    fn assert_file_refused(store: &mut Store, rec: &NewRecord, files: &[NewAttachment]) {
        assert!(matches!(
            assert_refused(store, rec, files),
            RecordError::Attachment(_)
        ));
    }

    #[test]
    fn refusals_before_creation_leave_no_record() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);

        let mut big = pdf(1);
        big.resize(8 * 1024 * 1024 + 1, 0);
        assert_file_refused(&mut store, &rec, &[file("a.pdf", big)]);
        assert_file_refused(&mut store, &rec, &[file("a.pdf", Vec::new())]);
        let mut program = b"MZ\x90\x00".to_vec();
        program.extend_from_slice(&[0; 64]);
        assert_file_refused(&mut store, &rec, &[file("fatura.pdf", program)]);
        assert_file_refused(
            &mut store,
            &rec,
            &[file("a.zip", b"PK\x03\x04zzzz".to_vec())],
        );
        assert_file_refused(
            &mut store,
            &rec,
            &[file(&format!("{}.pdf", "n".repeat(200)), pdf(1))],
        );
        // A good file first does not save the bad one after it.
        assert_file_refused(
            &mut store,
            &rec,
            &[file("a.pdf", pdf(1)), file("b.exe", b"MZ..".to_vec())],
        );
        // Limits: 20 are allowed by the count, 21 are not; the total is capped.
        let many = |n: usize| -> Vec<NewAttachment> {
            (0..n)
                .map(|i| file(&format!("f{i}.pdf"), pdf(i as u8)))
                .collect()
        };
        assert!(validate_attachments(&many(20)).is_ok());
        assert_file_refused(&mut store, &rec, &many(21));
        let mut a = pdf(1);
        a.resize(7 * 1024 * 1024, 0);
        let mut b = pdf(2);
        b.resize(7 * 1024 * 1024, 1);
        let mut c = pdf(3);
        c.resize(3 * 1024 * 1024, 2);
        assert_file_refused(
            &mut store,
            &rec,
            &[file("a.pdf", a), file("b.pdf", b), file("c.pdf", c)],
        );

        // Bad fields and metadata.
        let mut bad = rec.clone();
        bad.fields.title = "ab".into();
        assert!(matches!(
            assert_refused(&mut store, &bad, &[]),
            RecordError::Invalid(_)
        ));
        let mut bad = rec.clone();
        bad.fields.kind = "inexistente".into();
        assert_refused(&mut store, &bad, &[]);
        let mut bad = rec.clone();
        bad.fields.document_date = "04/11/2026".into();
        assert_refused(&mut store, &bad, &[]);
        let mut bad = rec.clone();
        bad.fields.text = "x".repeat(crate::model::MAX_CAPTURE_TEXT_BYTES + 1);
        assert_refused(&mut store, &bad, &[]);
        let mut bad = rec.clone();
        bad.request_id = "nao-e-uuid".into();
        assert_refused(&mut store, &bad, &[]);
    }

    #[test]
    fn metadata_limits() {
        let ok = record(TEXT);
        assert!(validate_record(&ok).is_ok());
        let with = |key: &str, value: String| {
            let mut r = ok.clone();
            r.metadata.insert(key.into(), value);
            r
        };
        assert!(validate_record(&with("wa_chat", "x".repeat(200))).is_ok());
        assert!(validate_record(&with("wa_chat", "x".repeat(201))).is_err());
        assert!(validate_record(&with("wa_chat", "a\nb".into())).is_err());
        assert!(validate_record(&with("", "v".into())).is_err());
        assert!(validate_record(&with(&"k".repeat(65), "v".into())).is_err());
        assert!(validate_record(&with("manual_kind", "email".into())).is_err());
        assert!(validate_record(&with("origin", "".into())).is_err());
        let mut many = ok.clone();
        for i in 0..201 {
            many.metadata.insert(format!("k{i}"), "v".into());
        }
        assert!(validate_record(&many).is_err());
        // Even a large accepted metadata passes the core's own limits.
        let (_dir, mut store) = store();
        let mut full = ok;
        for i in 0..190 {
            full.metadata.insert(format!("k{i}"), "v".repeat(200));
        }
        assert!(store.create_record_with_attachments(&full, &[]).is_ok());
    }

    #[test]
    fn a_failure_after_creation_sends_the_record_to_the_lixeira() {
        let (_dir, mut store) = store();
        // The library refuses to store attachments (as a full disk would):
        // the files pass the checks made without the store.
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER fail_attachments BEFORE INSERT ON attachments BEGIN SELECT RAISE(ABORT,'falha simulada em C:\\acervo'); END;",
            )
            .unwrap();
        let rec = record(TEXT);
        let err = store
            .create_record_with_attachments(&rec, &[file("a.png", png(1))])
            .unwrap_err();
        let text = err.to_string();
        assert!(matches!(
            err.downcast_ref::<RecordError>(),
            Some(RecordError::Failed(_))
        ));
        assert!(text.contains("não foi guardada"), "{text}");
        assert!(!text.contains("acervo\\"), "no raw store text: {text}");
        assert_eq!(documents(&store), 0, "the record is out of the list");
        assert!(
            store.trash_entries().unwrap().is_empty(),
            "and nothing is left behind"
        );
        // Trying again the same content starts clean.
        assert!(matches!(
            store.create_record_with_attachments(&rec, &[]).unwrap(),
            Created::New { .. }
        ));
        // Without files the library still works.
        let other = record("[04/11/2026 10:05] Servidor A: outra");
        assert!(store.create_record_with_attachments(&other, &[]).is_ok());
    }
}
