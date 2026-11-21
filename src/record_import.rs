//! A record created together with its attachments, all or nothing: the
//! files are validated before the record exists, and a record whose
//! attachment cannot be stored is not stored at all (one transaction).
//!
//! The input of this module may come from an untrusted web page (through the
//! browser extension), so everything that can be checked without the store is
//! checked first, and the messages of the errors are fixed Portuguese
//! sentences (never a raw store error, which can carry names or paths).
use crate::{
    attachments::{
        hashes_including_trash, sanitize_file_name, validate_description, validate_file,
        AttachmentInput, AttachmentStepFailed, Owner, MAX_PER_OWNER,
    },
    manual::ManualFields,
    model::{ImportOutcome, ImportReceipt, MANUAL_PENDING},
    storage::{
        import_receipt_state, remove_in_progress_import_receipt, write_import_receipt,
        write_import_receipt_in_progress, ReceiptState, Store,
    },
    trash::{Kind, Target},
};
use anyhow::{Error, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::{AtomicBool, Ordering};

/// Test hook: when set, the next import that has attachments fails right
/// after its record was written and before its attachments are, as a full
/// disk or an interrupted process would. The flag is consumed by that
/// import. Compiled only for tests (`cfg(test)`, or the `test-support` feature
/// for the tests of other crates): a release build has neither the flag nor
/// the check.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub static INJECT_FAILURE_AFTER_RECORD: AtomicBool = AtomicBool::new(false);

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

/// Called by an import with attachments after its record is written and
/// before its files are (see `INJECT_FAILURE_AFTER_RECORD`).
pub(crate) fn injected_failure() -> Result<()> {
    #[cfg(any(test, feature = "test-support"))]
    if INJECT_FAILURE_AFTER_RECORD.swap(false, Ordering::SeqCst) {
        anyhow::bail!("falha simulada depois de gravar o registro");
    }
    Ok(())
}

/// The answer for a failed import: a fixed sentence, never the raw error.
fn refusal_of_import(err: &Error) -> Error {
    if let Some(step) = err.downcast_ref::<AttachmentStepFailed>() {
        return failed(if step.completing {
            "Um dos arquivos não pôde ser guardado. A conversa já estava no Ponte e continua como estava. Tente de novo."
        } else {
            "Um dos arquivos não pôde ser guardado, então a conversa não foi guardada. Tente de novo."
        });
    }
    store_refusal(err)
}

/// What the caller is told: `Already` only when nothing at all was stored
/// now and the record was there before.
fn created_from(receipt: &ImportReceipt, attachments_added: usize) -> Created {
    if receipt.effective_outcome() != ImportOutcome::Imported && attachments_added == 0 {
        return Created::Already {
            document_id: receipt.document_id,
        };
    }
    Created::New {
        document_id: receipt.document_id,
        attachments_added,
    }
}

impl Store {
    /// Creates a manual record with its attachments, all or nothing.
    ///
    /// 1. Everything that does not need the store is checked first.
    /// 2. The record and its attachments are written in ONE transaction (with
    ///    the queued copies of the changes, in shared-folder mode): either the
    ///    record, its files and every version and team-event entry they
    ///    carry are in the library, or nothing is. The attachments are rows
    ///    of the library itself (sealed), so no file outside it takes part.
    /// 3. The record is created like a manual one, with the given request id.
    ///    The same id again does not create a second record: it answers
    ///    `Already` and adds nothing, whatever happened to the files since (a
    ///    request id can be reused for other content, and a file the person
    ///    purged must not come back). That is what the receipt of the import
    ///    (`import_receipts`, local, written in the same transaction) says. Only
    ///    a record without a receipt (an older version made it) that has no
    ///    attachment at all, none live and none in the trash, gets the files,
    ///    once, and then the receipt.
    ///
    /// On a computer connected to a main computer every change is a request
    /// to it, so one transaction is not possible there: the record is created
    /// first and then each file is sent (`import_capture`, `add_attachment`);
    /// if one fails the new record is removed, and a retry starts clean. If the
    /// process dies in the middle, the receipt written before the first
    /// request (in progress, with the manifest) lets the retry send only the
    /// files still missing.
    pub fn create_record_with_attachments(
        &mut self,
        record: &NewRecord,
        files: &[NewAttachment],
    ) -> Result<Created> {
        self.create_record_for_operation(record, files, None)
    }

    /// [`Store::create_record_with_attachments`] for an action of the phone app
    /// that carries an `operation_id`: the id is its idempotency key. The row
    /// that says "this operation was committed" is written in the very
    /// transaction of the record, its files and its queue entries, so after a
    /// process death the id alone tells whether the record exists. An id that
    /// is already there creates nothing and answers `Already` with the record
    /// of the first call. (On a computer connected to a main computer, where
    /// the changes are sent one by one, the row is written after the last one.)
    pub fn create_record_for_operation(
        &mut self,
        record: &NewRecord,
        files: &[NewAttachment],
        operation_id: Option<&str>,
    ) -> Result<Created> {
        let operation_id = operation_id.map(str::trim).filter(|id| !id.is_empty());
        if let Some(id) = operation_id {
            if let Some(done) = self.operation_record(id).map_err(|e| store_refusal(&e))? {
                return Ok(Created::Already {
                    document_id: done.document_id,
                });
            }
        }
        validate_record(record)?;
        let files = prepared(files)?;
        let actor = self.active_actor();
        let capture = record.fields.capture_with(
            MANUAL_PENDING,
            &actor,
            record.request_id.clone(),
            record.metadata.clone(),
        );
        let inputs: Vec<AttachmentInput<'_>> = files
            .iter()
            .map(|(name, file)| AttachmentInput {
                name,
                description: file.description.trim(),
                data: &file.bytes,
            })
            .collect();
        match self.import_with_attachments_atomically(&capture, &inputs, operation_id) {
            Ok(Some((receipt, added))) => return Ok(created_from(&receipt, added)),
            Ok(None) => {}
            Err(err) => return Err(refusal_of_import(&err)),
        }
        let created = self.create_record_through_the_main_computer(&capture, &inputs)?;
        if let (Some(id), Created::New { document_id, .. }) = (operation_id, &created) {
            // Not in one transaction here (every change is a request to the
            // main computer): written once the record and its files are in.
            let _ = crate::storage::record_operation(
                &self.conn,
                id,
                "create_record",
                *document_id,
                None,
                None,
            );
        }
        Ok(created)
    }

    /// The record and then each file, as separate requests to the main
    /// computer. A process that dies between two of them leaves a record with
    /// some of its files, so the local receipt is written BEFORE the first
    /// request, "in progress", with the manifest (the sorted digests of the
    /// files of this import), and completed after the last file went through.
    /// A retry of the same request id then knows what it was doing:
    ///
    /// - receipt completed: `Already`, nothing is added;
    /// - in progress with the SAME manifest: the files of the manifest whose
    ///   digest is not on the record (live or in the trash) are added, the
    ///   receipt is completed;
    /// - in progress with ANOTHER manifest (the id was reused for other
    ///   content): `Already`, nothing is added;
    /// - no receipt and the record already there (an older version left it):
    ///   the rule of the single transaction, a record with no attachment at all
    ///   is completed once.
    ///
    /// If the record itself cannot be created, the receipt written here goes
    /// away.
    fn create_record_through_the_main_computer(
        &mut self,
        capture: &crate::model::Capture,
        files: &[AttachmentInput<'_>],
    ) -> Result<Created> {
        let request_key = uuid::Uuid::parse_str(&capture.request_id)
            .map_err(|_| invalid("O identificador da importação é inválido."))?
            .hyphenated()
            .to_string();
        let manifest: BTreeSet<String> = files
            .iter()
            .map(|file| format!("{:x}", Sha256::digest(file.data)))
            .collect();
        let before =
            import_receipt_state(&self.conn, &request_key).map_err(|e| store_refusal(&e))?;
        if before.is_none() {
            write_import_receipt_in_progress(&self.conn, &request_key, manifest.iter().cloned())
                .map_err(|e| store_refusal(&e))?;
        }
        let receipt = match self.import_capture(capture) {
            Ok(receipt) => receipt,
            Err(err) => {
                if before.is_none() {
                    let _ = remove_in_progress_import_receipt(&self.conn, &request_key);
                }
                return Err(store_refusal(&err));
            }
        };
        self.send_files_of_import(&receipt, &request_key, files, &manifest, before)
    }

    /// The files of an import on a computer connected to a main computer, one
    /// request each, after the record exists there (`receipt` is the record's;
    /// `before` is what the local receipt said before this call, `None` for no
    /// receipt). See `create_record_through_the_main_computer` for the rules.
    fn send_files_of_import(
        &mut self,
        receipt: &ImportReceipt,
        request_key: &str,
        files: &[AttachmentInput<'_>],
        manifest: &BTreeSet<String>,
        before: Option<ReceiptState>,
    ) -> Result<Created> {
        let document_id = receipt.document_id;
        let completing = receipt.effective_outcome() != ImportOutcome::Imported;
        let in_the_library: BTreeSet<String> = if completing {
            match &before {
                Some(ReceiptState::Completed) => return Ok(Created::Already { document_id }),
                // The request id was used for other content: not ours to touch.
                Some(ReceiptState::InProgress(other)) if other != manifest => {
                    return Ok(Created::Already { document_id });
                }
                _ => {}
            }
            hashes_including_trash(&self.conn, Owner::Document(document_id))
                .map_err(|e| store_refusal(&e))?
                .into_iter()
                .collect()
        } else {
            BTreeSet::new()
        };
        if before.is_none() && !in_the_library.is_empty() {
            // The record has files that are not this import's to complete.
            let _ = write_import_receipt(&self.conn, request_key, in_the_library);
            return Ok(Created::Already { document_id });
        }
        let mut added = 0;
        for file in files {
            let digest = format!("{:x}", Sha256::digest(file.data));
            // Already on the record (a previous run sent it) or in the trash
            // (the person removed it meanwhile): not sent again.
            if in_the_library.contains(&digest) {
                continue;
            }
            let result = self.add_attachment(
                Owner::Document(document_id),
                file.name,
                file.description,
                file.data,
            );
            let Err(err) = result else {
                added += 1;
                #[cfg(test)]
                if let Some(err) = simulated_death(added) {
                    return Err(err);
                }
                continue;
            };
            if completing {
                // The record was there before this import: it stays, and the
                // receipt stays "in progress" for the next try.
                return Err(refusal_of_import(&Error::new(AttachmentStepFailed {
                    completing: true,
                    source: err,
                })));
            }
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
                let _ = remove_in_progress_import_receipt(&self.conn, request_key);
                return Err(failed("Um dos arquivos não pôde ser guardado, então a conversa não foi guardada. Tente de novo."));
            }
            return Err(match trashed {
                Ok(_) => failed("Um dos arquivos não pôde ser guardado, então a conversa não foi guardada. Ela foi para a Lixeira; tente de novo."),
                Err(_) => failed("Um dos arquivos não pôde ser guardado e o registro ficou incompleto. Exclua-o na lista de documentos e tente de novo."),
            });
        }
        // Every file of the manifest is on the record: this import is
        // complete. (If this write fails the receipt stays "in progress" and a
        // retry finds every file already there and completes it.)
        let _ = write_import_receipt(&self.conn, request_key, manifest.iter().cloned());
        Ok(if completing && added == 0 {
            Created::Already { document_id }
        } else {
            Created::New {
                document_id,
                attachments_added: added,
            }
        })
    }
}

// Test hook: the process "dies" (the call returns an error and nothing is
// cleaned up) right after this many files of an import went through.
#[cfg(test)]
thread_local! {
    static DIE_AFTER_FILES: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn simulated_death(files_sent: usize) -> Option<Error> {
    DIE_AFTER_FILES.with(|limit| {
        (limit.get() == Some(files_sent)).then(|| {
            limit.set(None);
            anyhow::anyhow!("processo interrompido")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;

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
    fn the_same_request_id_answers_already_and_adds_nothing_when_nothing_is_missing() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);
        let first = store
            .create_record_with_attachments(&rec, &[file("a.png", png(1))])
            .unwrap();
        let Created::New { document_id, .. } = first else {
            panic!()
        };
        let again = store
            .create_record_with_attachments(&rec, &[file("a.png", png(1))])
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

    // ---- record and attachments are one operation ----

    use crate::intranet::{IntranetConfig, IntranetMode, OutboxEntry};

    fn shared_folder(store: &mut Store) {
        let cfg = IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: "Celular de teste".into(),
            folder_files: vec!["ponte-sync-teste.ponte".into()],
            folder_writer: uuid::Uuid::new_v4().simple().to_string(),
            ..store.load_intranet_config().unwrap()
        };
        store.save_intranet_config(&cfg).unwrap();
    }

    /// Everything a failed import must not leave in the library.
    fn traces(store: &Store) -> (i64, usize, usize, usize) {
        (
            store.cabinet_revision().unwrap(),
            store.list_team_events(500).unwrap().len(),
            store.list_offline_outbox().unwrap().len(),
            store.list_documents("").unwrap().len(),
        )
    }

    fn fail_on(store: &Store, trigger: &str) {
        store
            .conn
            .execute_batch(&format!(
                "CREATE TRIGGER {trigger} BEFORE INSERT ON {} BEGIN SELECT RAISE(ABORT,'falha simulada'); END;",
                if trigger.contains("trash") { "trash" } else { "attachments" }
            ))
            .unwrap();
    }

    #[test]
    fn a_failure_while_attaching_leaves_no_trace_in_any_mode() {
        for queued in [false, true] {
            let (_dir, mut store) = store();
            if queued {
                shared_folder(&mut store);
            }
            let before = traces(&store);
            fail_on(&store, "fail_attachments");
            let err = store
                .create_record_with_attachments(
                    &record(TEXT),
                    &[file("a.png", png(1)), file("b.pdf", pdf(2))],
                )
                .unwrap_err();
            assert!(matches!(
                err.downcast_ref::<RecordError>(),
                Some(RecordError::Failed(_))
            ));
            assert_eq!(
                traces(&store),
                before,
                "no record, no journal, no queue (queued: {queued})"
            );
            assert!(store.trash_entries().unwrap().is_empty());
        }
    }

    #[test]
    fn a_failure_cannot_leave_a_record_even_when_the_cleanup_would_fail() {
        for queued in [false, true] {
            let (_dir, mut store) = store();
            if queued {
                shared_folder(&mut store);
            }
            // Before: the attachment failed and the cleanup (trash, purge)
            // failed too, leaving the record without its file.
            fail_on(&store, "fail_attachments");
            fail_on(&store, "fail_trash");
            assert!(store
                .create_record_with_attachments(&record(TEXT), &[file("a.png", png(1))])
                .is_err());
            assert_eq!(documents(&store), 0, "queued: {queued}");
        }
    }

    fn attachment_names(store: &Store, document_id: i64) -> Vec<String> {
        store
            .list_attachments(Owner::Document(document_id))
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    /// The manifest of the receipt of a request, if it has one.
    fn manifest(store: &Store, rec: &NewRecord) -> Option<Vec<String>> {
        store
            .conn
            .query_row(
                "SELECT manifest FROM import_receipts WHERE request_id=?1",
                [&rec.request_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .unwrap()
            .map(|m| serde_json::from_str(&m).unwrap())
    }

    /// Makes a record look as an older version left it: no receipt.
    fn forget_receipts(store: &Store) {
        store
            .conn
            .execute("DELETE FROM import_receipts", [])
            .unwrap();
    }

    fn purge_attachment(store: &mut Store, id: i64) {
        let target = Target {
            kind: Kind::Attachment,
            id,
        };
        store.trash_item(target.clone()).unwrap();
        store.purge_trash_item(target).unwrap();
    }

    #[test]
    fn the_receipt_is_written_with_the_record_and_lists_the_files_sorted() {
        for queued in [false, true] {
            let (_dir, mut store) = store();
            if queued {
                shared_folder(&mut store);
            }
            let rec = record(TEXT);
            let (a, b) = (png(1), pdf(2));
            store
                .create_record_with_attachments(
                    &rec,
                    &[file("b.pdf", b.clone()), file("a.png", a.clone())],
                )
                .unwrap();
            let mut expected = vec![sha(&a), sha(&b)];
            expected.sort();
            assert_eq!(manifest(&store, &rec), Some(expected), "queued: {queued}");
            // A record without files has a receipt too (an empty list).
            let bare = record("[04/11/2026 10:05] Servidor A: sem arquivos");
            store.create_record_with_attachments(&bare, &[]).unwrap();
            assert_eq!(manifest(&store, &bare), Some(Vec::new()));
            // A failure takes the receipt away with the record.
            fail_on(&store, "fail_attachments");
            let failing = record("[04/11/2026 10:06] Servidor A: falha");
            assert!(store
                .create_record_with_attachments(&failing, &[file("c.png", png(3))])
                .is_err());
            assert_eq!(manifest(&store, &failing), None, "queued: {queued}");
        }
    }

    #[test]
    fn a_request_with_its_receipt_answers_already_whatever_happened_to_the_files() {
        for queued in [false, true] {
            let (_dir, mut store) = store();
            if queued {
                shared_folder(&mut store);
            }
            let rec = record(TEXT);
            let Created::New { document_id, .. } = store
                .create_record_with_attachments(&rec, &[file("a.png", png(1))])
                .unwrap()
            else {
                panic!()
            };
            let queue_before = store.list_offline_outbox().unwrap().len();
            let events_before = store.list_team_events(500).unwrap().len();
            let more = [file("outro-nome.png", png(1)), file("b.pdf", pdf(2))];
            // Same id, other files: nothing is added, nothing is queued.
            assert_eq!(
                store.create_record_with_attachments(&rec, &more).unwrap(),
                Created::Already { document_id },
                "queued: {queued}"
            );
            assert_eq!(attachment_names(&store, document_id), ["a.png"]);
            // The person purges the only file; the retry does not bring it back.
            let id = store
                .list_attachments(Owner::Document(document_id))
                .unwrap()[0]
                .id;
            purge_attachment(&mut store, id);
            let events_after_purge = store.list_team_events(500).unwrap().len();
            let queue_after_purge = store.list_offline_outbox().unwrap().len();
            assert_eq!(
                store
                    .create_record_with_attachments(&rec, &[file("a.png", png(1))])
                    .unwrap(),
                Created::Already { document_id }
            );
            assert!(attachment_names(&store, document_id).is_empty());
            assert_eq!(
                store.list_team_events(500).unwrap().len(),
                events_after_purge
            );
            assert_eq!(
                store.list_offline_outbox().unwrap().len(),
                queue_after_purge
            );
            assert!(queue_before <= queue_after_purge);
            assert!(events_before <= events_after_purge);
        }
    }

    #[test]
    fn a_reused_request_id_cannot_put_the_files_of_another_import_on_the_record() {
        let (_dir, mut store) = store();
        // A record that was saved without files (the receipt says: complete).
        let rec = record(TEXT);
        let Created::New { document_id, .. } =
            store.create_record_with_attachments(&rec, &[]).unwrap()
        else {
            panic!()
        };
        // The same id (same origin and text) arrives again with files.
        let again = store
            .create_record_with_attachments(&rec, &[file("de-outra.png", png(9))])
            .unwrap();
        assert_eq!(again, Created::Already { document_id });
        assert!(attachment_names(&store, document_id).is_empty());
    }

    #[test]
    fn a_record_an_older_version_left_without_any_file_is_completed_once() {
        for queued in [false, true] {
            let (_dir, mut store) = store();
            if queued {
                shared_folder(&mut store);
            }
            let rec = record(TEXT);
            // The state an interrupted import of an older version left: the
            // record is there, with no file and no receipt.
            let Created::New { document_id, .. } =
                store.create_record_with_attachments(&rec, &[]).unwrap()
            else {
                panic!()
            };
            forget_receipts(&store);
            let both = [file("a.png", png(1)), file("b.pdf", pdf(2))];
            assert_eq!(
                store.create_record_with_attachments(&rec, &both).unwrap(),
                Created::New {
                    document_id,
                    attachments_added: 2
                },
                "queued: {queued}"
            );
            assert_eq!(documents(&store), 1);
            assert_eq!(attachment_names(&store, document_id), ["a.png", "b.pdf"]);
            assert_eq!(manifest(&store, &rec).map(|m| m.len()), Some(2));
            // Once: the next retry is "already", even with another file.
            assert_eq!(
                store
                    .create_record_with_attachments(&rec, &[file("c.png", png(3))])
                    .unwrap(),
                Created::Already { document_id }
            );
            assert_eq!(attachment_names(&store, document_id).len(), 2);
        }
    }

    #[test]
    fn a_record_without_a_receipt_that_has_a_file_is_left_alone_and_gets_its_receipt() {
        for in_the_trash in [false, true] {
            let (_dir, mut store) = store();
            let rec = record(TEXT);
            let a = png(1);
            let Created::New { document_id, .. } = store
                .create_record_with_attachments(&rec, &[file("a.png", a.clone())])
                .unwrap()
            else {
                panic!()
            };
            if in_the_trash {
                // Deleted by the person: still an attachment of the record.
                let id = store
                    .list_attachments(Owner::Document(document_id))
                    .unwrap()[0]
                    .id;
                store
                    .trash_item(Target {
                        kind: Kind::Attachment,
                        id,
                    })
                    .unwrap();
            }
            forget_receipts(&store);
            assert_eq!(manifest(&store, &rec), None);
            let more = [file("a.png", a.clone()), file("b.pdf", pdf(2))];
            assert_eq!(
                store.create_record_with_attachments(&rec, &more).unwrap(),
                Created::Already { document_id },
                "in the trash: {in_the_trash}"
            );
            // Nothing was added, and the receipt says what is there.
            assert_eq!(
                store
                    .list_attachments(Owner::Document(document_id))
                    .unwrap()
                    .len(),
                usize::from(!in_the_trash)
            );
            assert_eq!(manifest(&store, &rec), Some(vec![sha(&a)]));
        }
    }

    /// The record on the main computer, as the connected computer gets it
    /// (here the library itself plays that part), and the files as the
    /// sequential path takes them.
    fn sequential(store: &mut Store, rec: &NewRecord, files: &[NewAttachment]) -> Result<Created> {
        let prepared = prepared(files).unwrap();
        let inputs: Vec<AttachmentInput<'_>> = prepared
            .iter()
            .map(|(name, file)| AttachmentInput {
                name,
                description: file.description.trim(),
                data: &file.bytes,
            })
            .collect();
        let actor = store.active_actor();
        let capture = rec.fields.capture_with(
            MANUAL_PENDING,
            &actor,
            rec.request_id.clone(),
            rec.metadata.clone(),
        );
        store.create_record_through_the_main_computer(&capture, &inputs)
    }

    #[test]
    fn the_sequential_path_of_a_connected_computer_follows_the_same_rule() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);
        let a = png(1);
        let Created::New {
            document_id,
            attachments_added,
        } = sequential(&mut store, &rec, &[file("a.png", a.clone())]).unwrap()
        else {
            panic!()
        };
        assert_eq!(attachments_added, 1);
        // The receipt is written locally once the last file is in.
        assert_eq!(manifest(&store, &rec), Some(vec![sha(&a)]));
        // Same id, other files: nothing added.
        let more = [file("a.png", a.clone()), file("b.pdf", pdf(2))];
        assert_eq!(
            sequential(&mut store, &rec, &more).unwrap(),
            Created::Already { document_id }
        );
        // The person purges the file: it does not come back.
        let id = store
            .list_attachments(Owner::Document(document_id))
            .unwrap()[0]
            .id;
        purge_attachment(&mut store, id);
        assert_eq!(
            sequential(&mut store, &rec, &more).unwrap(),
            Created::Already { document_id }
        );
        assert!(attachment_names(&store, document_id).is_empty());
        // Without a receipt: a record with files is left alone (and gets its
        // receipt); a record without any is completed once.
        let other = record("[04/11/2026 10:05] Servidor A: outro");
        let Created::New { document_id, .. } = sequential(&mut store, &other, &[]).unwrap() else {
            panic!()
        };
        forget_receipts(&store);
        assert_eq!(
            sequential(&mut store, &other, &more).unwrap(),
            Created::New {
                document_id,
                attachments_added: 2
            }
        );
        assert_eq!(manifest(&store, &other).map(|m| m.len()), Some(2));
        forget_receipts(&store);
        assert_eq!(
            sequential(&mut store, &other, &[file("c.png", png(3))]).unwrap(),
            Created::Already { document_id }
        );
        assert_eq!(attachment_names(&store, document_id).len(), 2);
        assert_eq!(manifest(&store, &other).map(|m| m.len()), Some(2));
    }

    /// The state of the local receipt of a request ("done" / "in_progress").
    fn receipt_state(store: &Store, rec: &NewRecord) -> Option<String> {
        store
            .conn
            .query_row(
                "SELECT state FROM import_receipts WHERE request_id=?1",
                [&rec.request_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .unwrap()
    }

    fn die_after(files: usize) {
        DIE_AFTER_FILES.with(|limit| limit.set(Some(files)));
    }

    #[test]
    fn a_connected_import_that_died_after_the_first_file_is_completed_by_the_retry() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);
        let (a, b) = (png(1), pdf(2));
        let both = [file("a.png", a.clone()), file("b.pdf", b.clone())];
        // The importer dies after the first of the two file requests.
        die_after(1);
        assert!(sequential(&mut store, &rec, &both).is_err());
        let document_id = store.list_documents("").unwrap()[0].id;
        assert_eq!(attachment_names(&store, document_id), ["a.png"]);
        // The receipt was written before the first request, in progress, with
        // the manifest of the whole import.
        assert_eq!(receipt_state(&store, &rec).as_deref(), Some("in_progress"));
        let mut expected = vec![sha(&a), sha(&b)];
        expected.sort();
        assert_eq!(manifest(&store, &rec), Some(expected));
        // The retry adds exactly the missing file and completes the import.
        assert_eq!(
            sequential(&mut store, &rec, &both).unwrap(),
            Created::New {
                document_id,
                attachments_added: 1
            }
        );
        assert_eq!(attachment_names(&store, document_id), ["a.png", "b.pdf"]);
        assert_eq!(receipt_state(&store, &rec).as_deref(), Some("done"));
        assert_eq!(documents(&store), 1);
        // The next retry has nothing to do.
        assert_eq!(
            sequential(&mut store, &rec, &both).unwrap(),
            Created::Already { document_id }
        );
        assert_eq!(attachment_names(&store, document_id).len(), 2);
    }

    #[test]
    fn a_connected_import_that_died_before_any_file_gets_all_of_them() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);
        let both = [file("a.png", png(1)), file("b.pdf", pdf(2))];
        let actor = store.active_actor();
        let capture = rec.fields.capture_with(
            MANUAL_PENDING,
            &actor,
            rec.request_id.clone(),
            rec.metadata.clone(),
        );
        let key = uuid::Uuid::parse_str(&rec.request_id)
            .unwrap()
            .hyphenated()
            .to_string();
        // What a run that died right after the record leaves: the record and
        // the receipt in progress, no file.
        crate::storage::write_import_receipt_in_progress(
            &store.conn,
            &key,
            [sha(&png(1)), sha(&pdf(2))],
        )
        .unwrap();
        let document_id = store.import_capture(&capture).unwrap().document_id;
        assert_eq!(
            sequential(&mut store, &rec, &both).unwrap(),
            Created::New {
                document_id,
                attachments_added: 2
            }
        );
        assert_eq!(attachment_names(&store, document_id), ["a.png", "b.pdf"]);
        assert_eq!(receipt_state(&store, &rec).as_deref(), Some("done"));
    }

    #[test]
    fn a_receipt_in_progress_with_another_manifest_adds_nothing() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);
        die_after(1);
        assert!(sequential(
            &mut store,
            &rec,
            &[file("a.png", png(1)), file("b.pdf", pdf(2))]
        )
        .is_err());
        let document_id = store.list_documents("").unwrap()[0].id;
        // The same request id arrives with other files: not this import.
        let others = [file("x.png", png(7)), file("y.pdf", pdf(8))];
        assert_eq!(
            sequential(&mut store, &rec, &others).unwrap(),
            Created::Already { document_id }
        );
        assert_eq!(attachment_names(&store, document_id), ["a.png"]);
        // And the receipt still waits for the import it belongs to.
        assert_eq!(receipt_state(&store, &rec).as_deref(), Some("in_progress"));
        assert_eq!(
            sequential(
                &mut store,
                &rec,
                &[file("a.png", png(1)), file("b.pdf", pdf(2))]
            )
            .unwrap(),
            Created::New {
                document_id,
                attachments_added: 1
            }
        );
        assert_eq!(attachment_names(&store, document_id), ["a.png", "b.pdf"]);
    }

    #[test]
    fn a_file_the_person_moved_to_the_lixeira_during_the_gap_is_not_added_again() {
        let (_dir, mut store) = store();
        let rec = record(TEXT);
        let both = [file("a.png", png(1)), file("b.pdf", pdf(2))];
        die_after(1);
        assert!(sequential(&mut store, &rec, &both).is_err());
        let document_id = store.list_documents("").unwrap()[0].id;
        let id = store
            .list_attachments(Owner::Document(document_id))
            .unwrap()[0]
            .id;
        // In the gap the person sends the first file to the Lixeira.
        store
            .trash_item(Target {
                kind: Kind::Attachment,
                id,
            })
            .unwrap();
        assert_eq!(
            sequential(&mut store, &rec, &both).unwrap(),
            Created::New {
                document_id,
                attachments_added: 1
            }
        );
        // Only the second file was added; the first stays in the Lixeira.
        assert_eq!(attachment_names(&store, document_id), ["b.pdf"]);
        assert_eq!(receipt_state(&store, &rec).as_deref(), Some("done"));
        // Completed now: nothing comes back, whatever happens to the files.
        assert_eq!(
            sequential(&mut store, &rec, &both).unwrap(),
            Created::Already { document_id }
        );
        assert_eq!(attachment_names(&store, document_id), ["b.pdf"]);
    }

    #[test]
    fn a_record_that_cannot_be_created_leaves_no_receipt_in_progress() {
        let (_dir, mut store) = store();
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER fail_documents BEFORE INSERT ON documents BEGIN SELECT RAISE(ABORT,'falha simulada'); END;",
            )
            .unwrap();
        let rec = record(TEXT);
        assert!(sequential(&mut store, &rec, &[file("a.png", png(1))]).is_err());
        assert_eq!(receipt_state(&store, &rec), None);
        // A failure of a file on a fresh record removes the record and the receipt too.
        store
            .conn
            .execute_batch("DROP TRIGGER fail_documents")
            .unwrap();
        fail_on(&store, "fail_attachments");
        assert!(sequential(&mut store, &rec, &[file("a.png", png(1))]).is_err());
        assert_eq!(receipt_state(&store, &rec), None);
        assert_eq!(documents(&store), 0);
    }

    #[test]
    fn a_library_whose_receipts_have_no_state_column_gets_it_once() {
        let (_dir, mut store) = store();
        store
            .conn
            .execute_batch(
                "DROP TABLE import_receipts;
                 CREATE TABLE import_receipts(request_id TEXT PRIMARY KEY, manifest TEXT NOT NULL, completed_at TEXT NOT NULL);
                 INSERT INTO import_receipts VALUES('antigo','[]','2026-01-01T00:00:00.000Z');",
            )
            .unwrap();
        store.ensure_intranet().unwrap();
        store.ensure_intranet().unwrap();
        let state: String = store
            .conn
            .query_row(
                "SELECT state FROM import_receipts WHERE request_id='antigo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "done");
        assert!(crate::storage::import_receipt_exists(&store.conn, "antigo").unwrap());
    }

    /// What the journal and the queue hold, without clocks and row numbers.
    type Journal = (Vec<String>, Vec<(i64, String, i64)>, Vec<serde_json::Value>);

    fn journal(store: &Store) -> Journal {
        let mut events: Vec<_> = store.list_team_events(500).unwrap();
        events.sort_by_key(|e| e.id);
        let events = events
            .into_iter()
            .map(|e| format!("{}|{}|{:?}|{}", e.actor, e.action, e.document_id, e.details))
            .collect();
        let versions = store
            .conn
            .prepare("SELECT version,kind,item_id FROM item_versions ORDER BY kind,item_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let outbox = store
            .list_offline_outbox()
            .unwrap()
            .into_iter()
            .map(|e: OutboxEntry| {
                let mut value = serde_json::to_value(&e).unwrap();
                value["mutation"]["ImportCapture"]["capture"]
                    .as_object_mut()
                    .map(|c| c.remove("captured_at"));
                for key in ["id", "created_at", "uid"] {
                    value.as_object_mut().unwrap().remove(key);
                }
                value
            })
            .collect();
        (events, versions, outbox)
    }

    #[test]
    fn a_successful_import_writes_what_the_two_steps_wrote() {
        for queued in [false, true] {
            let rec = record(TEXT);
            let files = [file("foto:1.png", png(1)), file("nota.pdf", pdf(2))];

            // The sequence the importer used to run: the record, then each file.
            let (_old_dir, mut old) = store();
            if queued {
                shared_folder(&mut old);
            }
            let actor = old.active_actor();
            let capture = rec.fields.capture_with(
                MANUAL_PENDING,
                &actor,
                rec.request_id.clone(),
                rec.metadata.clone(),
            );
            let receipt = old.import_capture(&capture).unwrap();
            for (name, f) in prepared(&files).unwrap() {
                old.add_attachment(
                    Owner::Document(receipt.document_id),
                    &name,
                    f.description.trim(),
                    &f.bytes,
                )
                .unwrap();
            }

            let (_new_dir, mut new) = store();
            if queued {
                shared_folder(&mut new);
            }
            new.create_record_with_attachments(&rec, &files).unwrap();

            let (old_events, old_versions, old_outbox) = journal(&old);
            let (new_events, new_versions, new_outbox) = journal(&new);
            assert_eq!(new_events, old_events, "team events (queued: {queued})");
            assert_eq!(
                new_versions, old_versions,
                "item versions (queued: {queued})"
            );
            assert_eq!(new_outbox, old_outbox, "outbox (queued: {queued})");
            assert_eq!(new_events.len(), 3, "import + two attachments");
            assert_eq!(new_outbox.len(), if queued { 3 } else { 0 });
            assert_eq!(
                new.cabinet_revision().unwrap(),
                old.cabinet_revision().unwrap()
            );
        }
    }

    #[test]
    fn an_operation_row_is_committed_with_the_record_or_not_at_all_and_old_ones_are_pruned() {
        const OP: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a01";
        for queued in [false, true] {
            let (_dir, mut store) = store();
            if queued {
                shared_folder(&mut store);
            }
            // A failure takes the row away with the record.
            fail_on(&store, "fail_attachments");
            let rec = record(TEXT);
            assert!(store
                .create_record_for_operation(&rec, &[file("a.png", png(1))], Some(OP))
                .is_err());
            assert_eq!(store.operation_record(OP).unwrap(), None);
            store
                .conn
                .execute_batch("DROP TRIGGER fail_attachments")
                .unwrap();
            // Success: the row says which record, a repeat creates nothing.
            let Created::New { document_id, .. } = store
                .create_record_for_operation(&rec, &[file("a.png", png(1))], Some(OP))
                .unwrap()
            else {
                panic!()
            };
            let done = store.operation_record(OP).unwrap().unwrap();
            assert_eq!(
                (done.kind.as_str(), done.document_id),
                ("create_record", document_id)
            );
            let other = record("[04/11/2026 10:05] Servidor A: outro texto");
            assert_eq!(
                store
                    .create_record_for_operation(&other, &[], Some(OP))
                    .unwrap(),
                Created::Already { document_id }
            );
            assert_eq!(documents(&store), 1, "queued: {queued}");
            // Pruning by age.
            store
                .conn
                .execute(
                    "INSERT INTO phone_operations(operation_id,kind,document_id,created_at) VALUES('velha','attach',1,'2020-01-01T00:00:00.000Z')",
                    [],
                )
                .unwrap();
            assert_eq!(store.prune_operations(30).unwrap(), 1);
            assert_eq!(store.operation_record("velha").unwrap(), None);
            assert!(store.operation_record(OP).unwrap().is_some());
        }
    }
}
