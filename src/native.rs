//! Native Messaging host: length-prefixed JSON frames on standard input and
//! output, the extension ids accepted, and replies that always explain a
//! refusal in words instead of closing silently.
use crate::{
    model::{Capture, ImportOutcome},
    storage::Store,
};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

pub const MAX_FRAME: usize = 24 * 1024 * 1024;
pub const PROTOCOL_VERSION: u32 = 1;
pub const HOST_NAME: &str = "com.ponteproj.assistente";
pub const EXTENSION_ID: &str = include_str!("../extension-id.txt");
/// The same extension as published in the Chrome Web Store: the store signs
/// it with its own key, so it has another id. Both are the Ponte's.
pub const STORE_EXTENSION_ID: &str = include_str!("../extension-store-id.txt");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol_version: u32,
    action: String,
    capture: Option<Capture>,
    files: Option<Vec<crate::files::FileInput>>,
    /// `lookup`: the number shown on the page.
    document_number: Option<String>,
    /// `open_document`: the local id a lookup or an import returned.
    document_id: Option<i64>,
    /// `import_capture` / `import_bundle`: a note and a priority the person
    /// typed in the extension before importing.
    extras: Option<Extras>,
    /// `import_record`: the conversation to keep.
    record: Option<RecordInput>,
    /// `import_record`: files that go with it.
    attachments: Option<Vec<AttachmentInput>>,
}

/// `import_record`: what the extension read from the page. Untrusted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordInput {
    kind: String,
    title: String,
    origin: String,
    document_date: String,
    text: String,
    meta: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentInput {
    name: String,
    data_base64: String,
}

const MAX_RECORD_TITLE_CHARS: usize = 500;
const MAX_RECORD_ORIGIN_CHARS: usize = 200;
const MAX_RECORD_MESSAGES: usize = 500;
const MAX_RECORD_ATTACHMENTS: usize = 10;
const RECORD_META_KEYS: [&str; 4] = ["wa_chat", "wa_first", "wa_last", "wa_messages"];

/// What the person added to an import. Both fields are optional.
///
/// Rule: the extras are validated BEFORE anything is imported and a
/// malformed object refuses the whole request (nothing stored), so a bad
/// request never half-applies. A valid one is applied AFTER the import
/// succeeded, whatever its outcome (imported, confirmed, unchanged: the
/// person typed it now). A failure to apply never undoes or fails the import;
/// the reply says so (`extras_applied`, `extras_message`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extras {
    note: Option<String>,
    priority: Option<String>,
}

const MAX_EXTRAS_NOTE_CHARS: usize = 2000;
/// The priorities the ficha offers (`work_context`).
const EXTRAS_PRIORITIES: [&str; 4] = ["baixa", "normal", "alta", "urgente"];

/// Extras checked and normalised: the note trimmed (`None` when empty).
struct CheckedExtras {
    note: Option<String>,
    priority: Option<&'static str>,
}

fn check_extras(extras: &Extras) -> std::result::Result<CheckedExtras, String> {
    let note = extras
        .note
        .as_deref()
        .map(|note| note.replace("\r\n", "\n").trim().to_owned())
        .filter(|note| !note.is_empty());
    if let Some(note) = &note {
        if note.chars().count() > MAX_EXTRAS_NOTE_CHARS {
            return Err(format!(
                "A nota passa de {MAX_EXTRAS_NOTE_CHARS} caracteres. Encurte o texto e importe de novo."
            ));
        }
        if note
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(
                "A nota tem caracteres que o Ponte não aceita. Digite o texto de novo.".to_owned(),
            );
        }
    }
    let priority = match extras.priority.as_deref() {
        None => None,
        Some(value) => Some(
            EXTRAS_PRIORITIES
                .iter()
                .copied()
                .find(|known| *known == value)
                .ok_or_else(|| {
                    "A prioridade escolhida não é reconhecida pelo Ponte. Escolha baixa, normal, alta ou urgente."
                        .to_owned()
                })?,
        ),
    };
    Ok(CheckedExtras { note, priority })
}

/// Whether `current` already ends with this note as a dated entry
/// ("dd/mm/aaaa — note"), whatever the date: a retried request that crosses
/// midnight must not append it twice.
fn ends_with_entry(current: &str, note: &str) -> bool {
    let Some(before) = current.trim_end().strip_suffix(note) else {
        return false;
    };
    let Some(before) = before.strip_suffix(" — ") else {
        return false;
    };
    let date: Vec<char> = before.chars().rev().take(10).collect();
    date.len() == 10
        && date.iter().rev().enumerate().all(|(i, c)| match i {
            2 | 5 => *c == '/',
            _ => c.is_ascii_digit(),
        })
}

/// The note text to save: `current` plus a new paragraph "date — note", or
/// `None` when `skip_repeat` and that entry is already the last one.
fn appended_note(current: &str, note: &str, date: &str, skip_repeat: bool) -> Option<String> {
    if skip_repeat && ends_with_entry(current, note) {
        return None;
    }
    let entry = format!("{date} — {note}");
    Some(if current.trim().is_empty() {
        entry
    } else {
        format!("{}\n\n{entry}", current.trim_end())
    })
}

/// Fixed sentence for a failure to apply; the store's own message is never
/// passed on (a conflict names the team member who edited).
fn apply_failure(subject: &str, err: &anyhow::Error) -> String {
    let conflict = err.to_string().contains("Conflito de edição");
    if conflict {
        format!("{subject} porque o documento foi alterado ao mesmo tempo")
    } else {
        subject.to_owned()
    }
}

/// Applies checked extras to a document. Returns what could not be applied,
/// as one fixed Portuguese sentence. The note is appended as a new paragraph
/// prefixed by the date, never replacing the existing text; `skip_repeat`
/// avoids appending the same entry twice (a retried, already confirmed
/// request). In both cases the version is read BEFORE the content, so a
/// write by someone else in between is a conflict, not an overwrite.
fn apply_extras(
    store: &mut Store,
    document_id: i64,
    extras: &CheckedExtras,
    skip_repeat: bool,
) -> std::result::Result<(), String> {
    let mut problems = Vec::new();
    let mut guidance = Vec::new();
    if let Some(note) = &extras.note {
        let date = chrono::Local::now().format("%d/%m/%Y").to_string();
        let result = (|| -> Result<()> {
            let version = store
                .item_version("note", document_id)
                .ok()
                .map(|v| v.version);
            let current = store.load_document(document_id)?.note;
            match appended_note(&current, note, &date, skip_repeat) {
                Some(joined) => store.save_note_checked(document_id, &joined, version),
                None => Ok(()),
            }
        })();
        if let Err(err) = result {
            problems.push(apply_failure("a nota não foi salva", &err));
            guidance.push("Escreva a nota na ficha do documento.");
        }
    }
    if let Some(priority) = extras.priority {
        let result = (|| -> Result<()> {
            let version = store
                .item_version("work_context", document_id)
                .ok()
                .map(|v| v.version);
            let mut context = store.load_work_context(document_id)?;
            if context.priority == priority {
                return Ok(());
            }
            context.priority = priority.to_owned();
            store.save_work_context_checked(document_id, &context, version)
        })();
        if let Err(err) = result {
            problems.push(apply_failure("a prioridade não foi alterada", &err));
            guidance.push("Escolha a prioridade na ficha do documento.");
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "A importação foi feita, mas {}. {}",
            problems.join(" e "),
            guidance.join(" ")
        ))
    }
}

/// What this Ponte answers besides importing; the extension reads it from
/// the `ping` to know whether the Ponte is recent enough.
pub const ACTIONS: [&str; 4] = ["lookup", "open_document", "import_extras", "import_record"];

/// A refusal of `import_record`: the error code and a fixed sentence.
type Refusal = (&'static str, String);

fn invalid_record(message: &str) -> Refusal {
    ("invalid_record", message.to_owned())
}

/// Whether the line starts a message: at column 0, `[dd/mm/aaaa hh:mm]` or
/// `[sem data]`, the shape of every message of the record. Continuation lines
/// are indented in the record, so an indented line never counts.
fn has_message_start(line: &str) -> bool {
    const SHAPE: &[u8; 18] = b"[dd/dd/dddd dd:dd]";
    let bytes = line.as_bytes();
    line.starts_with("[sem data]")
        || bytes.len() >= SHAPE.len()
            && SHAPE.iter().zip(bytes).all(|(shape, byte)| match shape {
                b'd' => byte.is_ascii_digit(),
                other => other == byte,
            })
}

fn no_control(text: &str, allow_text_breaks: bool) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() && !(allow_text_breaks && (c == '\n' || c == '\t')))
}

/// An `AAAA-MM-DD` date, written exactly so.
fn plain_date(text: &str) -> bool {
    text.len() == 10 && chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

/// The record of the request, fully checked and normalised; nothing is
/// stored here. The volume is bounded by the byte limit of the text (500 KiB,
/// the core's). The message count is counted from the text itself (lines in
/// the shape `[dd/mm/aaaa hh:mm]`) and never taken from `wa_messages`, which
/// only has to be a plausible number.
fn check_record(
    input: RecordInput,
) -> std::result::Result<crate::record_import::NewRecord, Refusal> {
    if input.kind != "whatsapp" {
        return Err(invalid_record(
            "O Ponte só recebe conversas do WhatsApp por este caminho.",
        ));
    }
    let title = input.title.trim().to_owned();
    if !(3..=MAX_RECORD_TITLE_CHARS).contains(&title.chars().count()) || !no_control(&title, false)
    {
        return Err(invalid_record(
            "O título da conversa deve ter de 3 a 500 caracteres, sem caracteres especiais.",
        ));
    }
    let origin = input.origin.trim().to_owned();
    if origin.chars().count() > MAX_RECORD_ORIGIN_CHARS || !no_control(&origin, false) {
        return Err(invalid_record(
            "O nome da conversa passa de 200 caracteres ou tem caracteres inválidos.",
        ));
    }
    let document_date = input.document_date.trim().to_owned();
    if !document_date.is_empty() && !plain_date(&document_date) {
        return Err(invalid_record("A data da conversa é inválida."));
    }
    let text = input.text.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return Err(invalid_record("A conversa está vazia."));
    }
    if text.len() > crate::model::MAX_CAPTURE_TEXT_BYTES {
        return Err(invalid_record(
            "A conversa passa de 500 KB. Selecione menos mensagens e guarde de novo.",
        ));
    }
    if !no_control(&text, true) {
        return Err(invalid_record(
            "A conversa tem caracteres que o Ponte não aceita.",
        ));
    }
    if text.lines().filter(|line| has_message_start(line)).count() > MAX_RECORD_MESSAGES {
        return Err(invalid_record(
            "A conversa tem mais de 500 mensagens. Guarde em partes menores.",
        ));
    }
    let mut metadata = BTreeMap::new();
    for (key, value) in input.meta.unwrap_or_default() {
        if key == "imported_from" {
            return Err(invalid_record(
                "As informações da conversa não podem indicar a origem da importação.",
            ));
        }
        let value = value.trim();
        let ok = RECORD_META_KEYS.contains(&key.as_str())
            && value.chars().count() <= crate::record_import::MAX_META_VALUE_CHARS
            && no_control(value, false)
            && match key.as_str() {
                "wa_first" | "wa_last" => {
                    value.is_empty()
                        || (value.len() == 16
                            && chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
                                .is_ok())
                }
                "wa_messages" => {
                    !value.is_empty()
                        && value.len() <= 3
                        && value.bytes().all(|b| b.is_ascii_digit())
                        && value
                            .parse::<usize>()
                            .is_ok_and(|n| n <= MAX_RECORD_MESSAGES)
                }
                _ => true,
            };
        if !ok {
            return Err(invalid_record("As informações da conversa são inválidas."));
        }
        if !value.is_empty() {
            metadata.insert(key, value.to_owned());
        }
    }
    metadata
        .entry("wa_chat".to_owned())
        .or_insert_with(|| origin.clone());
    metadata.retain(|_, value| !value.is_empty());
    metadata.insert("imported_from".to_owned(), "whatsapp-web".to_owned());
    let request_id = crate::record_import::whatsapp_request_id(&origin, &text);
    Ok(crate::record_import::NewRecord {
        fields: crate::manual::ManualFields {
            kind: "whatsapp".to_owned(),
            title,
            origin,
            document_date,
            text,
        },
        metadata,
        request_id,
    })
}

/// The files of the request, decoded strictly and checked (count, size each
/// and together, name, type by content); nothing is stored here.
fn check_attachments(
    inputs: Vec<AttachmentInput>,
) -> std::result::Result<Vec<crate::record_import::NewAttachment>, Refusal> {
    use base64::Engine;
    let refuse = |message: &str| ("invalid_attachment", message.to_owned());
    if inputs.len() > MAX_RECORD_ATTACHMENTS {
        return Err(refuse(
            "A conversa tem anexos demais; o máximo é 10 por vez.",
        ));
    }
    let max_encoded = crate::attachments::MAX_ATTACHMENT_BYTES.div_ceil(3) * 4;
    let mut files = Vec::with_capacity(inputs.len());
    let mut total = 0usize;
    for input in inputs {
        if input.data_base64.len() > max_encoded {
            return Err(refuse("Um dos anexos passa de 8 MB."));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(input.data_base64.as_bytes())
            .map_err(|_| {
                refuse("Um dos anexos chegou com dados inválidos. Tente guardar de novo.")
            })?;
        if bytes.len() > crate::attachments::MAX_ATTACHMENT_BYTES {
            return Err(refuse("Um dos anexos passa de 8 MB."));
        }
        total += bytes.len();
        if total > crate::record_import::MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(refuse("Os anexos juntos passam de 16 MB."));
        }
        files.push(crate::record_import::NewAttachment {
            name: input.name,
            bytes,
            description: String::new(),
        });
    }
    crate::record_import::validate_attachments(&files).map_err(|err| refusal_of(&err))?;
    Ok(files)
}

/// The code and sentence for an error of `record_import`; anything that is
/// not one of its own sentences becomes a fixed one.
fn refusal_of(err: &anyhow::Error) -> Refusal {
    use crate::record_import::RecordError;
    match err.downcast_ref::<RecordError>() {
        Some(RecordError::Invalid(message)) => ("invalid_record", message.clone()),
        Some(RecordError::Attachment(message)) => ("invalid_attachment", message.clone()),
        Some(RecordError::Failed(message)) => ("import_failed", message.clone()),
        None => (
            "import_failed",
            "A importação não foi concluída. Tente de novo.".to_owned(),
        ),
    }
}

/// `import_record`: validates everything, stores the record with its
/// attachments (all or nothing), then applies the extras. `already` opens
/// the existing record too: the person asked to save it and wants to see it.
fn import_record(request: Request, store: &mut Store) -> (Value, Option<i64>) {
    use crate::record_import::Created;
    let refuse = |(code, message): Refusal| (error(code, &message), None);
    if request.capture.is_some() || request.files.is_some() || request.document_id.is_some() {
        return refuse(invalid_record("Pedido de importação inválido."));
    }
    let Some(record) = request.record else {
        return refuse(invalid_record(
            "A mensagem não contém a conversa a ser guardada.",
        ));
    };
    let extras = match request.extras.as_ref().map(check_extras).transpose() {
        Ok(extras) => extras,
        Err(message) => return (error("invalid_extras", &message), None),
    };
    let record = match check_record(record) {
        Ok(record) => record,
        Err(refusal) => return refuse(refusal),
    };
    let files = match check_attachments(request.attachments.unwrap_or_default()) {
        Ok(files) => files,
        Err(refusal) => return refuse(refusal),
    };
    let created = match store.create_record_with_attachments(&record, &files) {
        Ok(created) => created,
        Err(err) => return refuse(refusal_of(&err)),
    };
    let (document_id, outcome, attachments_added) = match created {
        Created::New {
            document_id,
            attachments_added,
        } => (document_id, "imported", attachments_added),
        Created::Already { document_id } => (document_id, "already", 0),
    };
    let number = store
        .load_document(document_id)
        .map(|detail| detail.summary.document_number)
        .unwrap_or_default();
    let mut response = json!({"protocol_version":1,"ok":true,"outcome":outcome,"document_id":document_id,"document_number":number,"attachments_added":attachments_added,"extras_applied":false});
    response["message"] = json!(if outcome == "already" {
        "Esta conversa já estava no Ponte. Nada foi gravado de novo."
    } else {
        "Conversa guardada no acervo local."
    });
    if let Some(extras) = extras.filter(|e| e.note.is_some() || e.priority.is_some()) {
        match apply_extras(store, document_id, &extras, outcome == "already") {
            Ok(()) => response["extras_applied"] = json!(true),
            Err(message) => response["extras_message"] = json!(message),
        }
    }
    (response, Some(document_id))
}

/// Whether the browser launched us for one of the Ponte's own extensions:
/// the one loaded from the installed folder or the one from the store. The
/// installer authorizes the same two in the browser; an executable that
/// knew only the first closed without a word for whoever installed from
/// the store.
pub fn valid_extension_origin(origin: &str) -> bool {
    [EXTENSION_ID, STORE_EXTENSION_ID]
        .iter()
        .map(|id| id.trim())
        .filter(|id| id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .any(|id| origin == format!("chrome-extension://{id}/"))
}

/// Answers every request with the same refusal instead of leaving without a
/// reply: the extension then shows the reason, not "no confirmation".
pub fn refuse<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    code: &str,
    message: &str,
) -> Result<()> {
    let message: String = message.chars().take(900).collect();
    while read_frame(reader)?.is_some() {
        write_frame(writer, &error(code, &message))?;
    }
    Ok(())
}

pub fn refuse_on_stdio(code: &str, message: &str) -> Result<()> {
    refuse(
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        code,
        message,
    )
}

pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => (),
        Err(e) => return Err(e).context("Não foi possível ler a mensagem do navegador"),
    }
    reader
        .read_exact(&mut header[1..])
        .context("Mensagem do navegador incompleta")?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        bail!("A mensagem do navegador tem um tamanho não permitido");
    }
    let mut bytes = vec![0; len];
    reader
        .read_exact(&mut bytes)
        .context("Mensagem do navegador incompleta")?;
    Ok(Some(bytes))
}

pub fn write_frame<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        bail!("A resposta é grande demais para ser enviada ao navegador");
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn error(code: &str, message: &str) -> Value {
    json!({"protocol_version": PROTOCOL_VERSION, "ok": false, "error": {"code": code, "message": message}})
}

/// Handles one request frame (`ping`, `lookup`, `open_document`,
/// `import_capture`, `import_bundle`).
/// Returns the reply and, when something was stored, the document to open.
/// Requests may reach `MAX_FRAME` (PDFs travel inside); replies stay small.
pub fn handle_request(bytes: &[u8], store: &mut Store) -> (Value, Option<i64>) {
    let request: Request = match serde_json::from_slice(bytes) {
        Ok(request) => request,
        Err(_) => {
            return (
                error(
                    "invalid_request",
                    "Pacote de importação inválido ou incompleto.",
                ),
                None,
            )
        }
    };
    if request.protocol_version != PROTOCOL_VERSION {
        return (
            error(
                "incompatible_version",
                "Extensão e aplicativo usam versões incompatíveis. Atualize os dois componentes.",
            ),
            None,
        );
    }
    match request.action.as_str() {
        "ping" => (
            json!({"protocol_version":1,"ok":true,"app_version":env!("CARGO_PKG_VERSION"),"actions":ACTIONS,"message":"O Ponte está disponível"}),
            None,
        ),
        // Is the document on the page already in the library? Read only.
        "lookup" => {
            let Some(number) = request
                .document_number
                .as_deref()
                .map(str::trim)
                .filter(|number| !number.is_empty() && number.len() <= 200)
            else {
                return (
                    error(
                        "missing_number",
                        "A consulta não traz o número do documento.",
                    ),
                    None,
                );
            };
            match store.imported_document_by_number(number) {
                Ok(Some((id, seen))) => (
                    json!({"protocol_version":1,"ok":true,"found":true,"document_id":id,"imported_at":crate::labels::datetime(&seen)}),
                    None,
                ),
                Ok(None) => (json!({"protocol_version":1,"ok":true,"found":false}), None),
                Err(err) => (
                    error(
                        "lookup_failed",
                        &format!("O Ponte não conseguiu consultar o acervo: {err}"),
                    ),
                    None,
                ),
            }
        }
        // Show a document the library already has.
        "open_document" => {
            let known = request
                .document_id
                .filter(|id| store.is_active_document(*id));
            match known {
                Some(id) => (
                    json!({"protocol_version":1,"ok":true,"document_id":id}),
                    Some(id),
                ),
                None => (
                    error(
                        "unknown_document",
                        "Este documento não está mais no acervo do Ponte.",
                    ),
                    None,
                ),
            }
        }
        "import_record" => import_record(request, store),
        "import_capture" | "import_bundle" => {
            // Malformed extras refuse the whole request before anything is stored.
            let extras = match request.extras.as_ref().map(check_extras).transpose() {
                Ok(extras) => extras,
                Err(message) => return (error("invalid_extras", &message), None),
            };
            let Some(capture) = request.capture else {
                return (
                    error(
                        "missing_capture",
                        "A mensagem não contém as informações da página.",
                    ),
                    None,
                );
            };
            let result = if request.action == "import_bundle" {
                store.import_bundle(&capture, &request.files.unwrap_or_default())
            } else if request.files.is_some() {
                Err(anyhow::anyhow!(
                    "Esta importação inclui arquivos; atualize a extensão e o Ponte."
                ))
            } else {
                store.import_capture(&capture)
            };
            match result {
                Ok(receipt) => {
                    let outcome = receipt.effective_outcome();
                    let mut response = json!({"protocol_version":1,"ok":true,"request_id":receipt.request_id,"document_id":receipt.document_id,"capture_id":receipt.capture_id,"duplicate":receipt.duplicate,"outcome":outcome.as_str()});
                    response["extras_applied"] = json!(false);
                    if let Some(extras) =
                        extras.filter(|e| e.note.is_some() || e.priority.is_some())
                    {
                        match apply_extras(
                            store,
                            receipt.document_id,
                            &extras,
                            outcome == ImportOutcome::Confirmed,
                        ) {
                            Ok(()) => response["extras_applied"] = json!(true),
                            Err(message) => response["extras_message"] = json!(message),
                        }
                    }
                    match outcome {
                        ImportOutcome::Imported => {
                            response["message"] =
                                json!("Informações da página gravadas no acervo local.");
                            (response, Some(receipt.document_id))
                        }
                        ImportOutcome::Confirmed => {
                            response["message"] = json!("Importação confirmada: a confirmação anterior tinha se perdido. Nada foi gravado de novo.");
                            (response, Some(receipt.document_id))
                        }
                        ImportOutcome::Unchanged => {
                            // Nothing was stored, so the Ponte is not opened:
                            // the page is where the person is, and the warning
                            // is shown there.
                            let when = receipt
                                .existing_captured_at
                                .as_deref()
                                .map(crate::labels::datetime)
                                .unwrap_or_else(|| "data desconhecida".to_owned());
                            response["message"] = json!(if receipt.previous_version {
                                format!("O SIGADOC mostra uma versão anterior deste documento, importada em {when}; ela voltou a ser a versão atual no Ponte. Nada novo foi gravado.")
                            } else {
                                format!("Importação ignorada: o SIGADOC mostra o mesmo conteúdo já importado em {when}. Nada foi gravado. Se o documento mudou, atualize a página (F5) e importe de novo.")
                            });
                            response["ui_opened"] = json!(false);
                            (response, None)
                        }
                    }
                }
                Err(err) => (
                    error(
                        "import_failed",
                        &format!("A importação não foi concluída: {err}"),
                    ),
                    None,
                ),
            }
        }
        _ => (
            error(
                "unsupported_action",
                "O Ponte não reconhece este pedido da extensão. Atualize a extensão e o Ponte.",
            ),
            None,
        ),
    }
}

pub fn serve<R: Read, W: Write, F: FnMut(i64) -> bool>(
    reader: &mut R,
    writer: &mut W,
    store: &mut Store,
    mut open_document: F,
) -> Result<()> {
    while let Some(bytes) = read_frame(reader)? {
        let (mut response, document) = handle_request(&bytes, store);
        if let Some(id) = document {
            response["ui_opened"] = json!(open_document(id));
        }
        write_frame(writer, &response)?;
    }
    Ok(())
}

pub fn run_host() -> Result<()> {
    // A library that cannot be opened is reported to the page, in words.
    let mut store = match crate::paths::db_path().and_then(|path| Store::open(&path)) {
        Ok(store) => store,
        Err(error) => {
            return refuse_on_stdio(
                "library_unavailable",
                &format!(
                    "O Ponte não conseguiu abrir o acervo deste computador: {error}. Abra o Ponte pelo menu Iniciar para ver o detalhe e depois clique em Repetir."
                ),
            )
        }
    };
    serve(
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut store,
        |id| {
            if std::env::var_os("PONTEPROJ_NO_UI").is_some() {
                return false;
            }
            let Ok(exe) = std::env::current_exe() else {
                return false;
            };
            let mut command = std::process::Command::new(exe);
            command
                .arg("--open-document")
                .arg(id.to_string())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            command.spawn().is_ok()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_roundtrip_uses_byte_length() {
        let value = json!({"texto":"Ofício — informação"});
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &value).unwrap();
        let payload = read_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&payload).unwrap(), value);
    }
    fn ask(store: &mut Store, request: Value) -> (Value, Option<i64>) {
        handle_request(&serde_json::to_vec(&request).unwrap(), store)
    }

    #[test]
    fn lookup_and_open_answer_about_the_library_without_changing_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let (ping, _) = ask(&mut store, json!({"protocol_version":1,"action":"ping"}));
        assert_eq!(
            ping["actions"],
            json!(["lookup", "open_document", "import_extras", "import_record"])
        );

        let capture = crate::model::sample_capture();
        let number = capture.document_number.clone();
        let (missing, open) = ask(
            &mut store,
            json!({"protocol_version":1,"action":"lookup","document_number":number}),
        );
        assert_eq!(missing["found"], false);
        assert_eq!(open, None);

        let id = store.import_capture(&capture).unwrap().document_id;
        // Spaces and capitals do not matter, as in the library.
        let typed = format!(" {} ", number.to_lowercase());
        let (found, open) = ask(
            &mut store,
            json!({"protocol_version":1,"action":"lookup","document_number":typed}),
        );
        assert_eq!(found["found"], true);
        assert_eq!(found["document_id"], id);
        assert!(found["imported_at"]
            .as_str()
            .is_some_and(|when| !when.is_empty()));
        // A lookup never opens the window.
        assert_eq!(open, None);

        let (reply, open) = ask(
            &mut store,
            json!({"protocol_version":1,"action":"open_document","document_id":id}),
        );
        assert_eq!(reply["ok"], true);
        assert_eq!(open, Some(id));
        let (reply, open) = ask(
            &mut store,
            json!({"protocol_version":1,"action":"open_document","document_id":id + 99}),
        );
        assert_eq!(reply["error"]["code"], "unknown_document");
        assert_eq!(open, None);
        let (reply, _) = ask(&mut store, json!({"protocol_version":1,"action":"lookup"}));
        assert_eq!(reply["error"]["code"], "missing_number");
        assert_eq!(store.list_documents("").unwrap().len(), 1);
        // A record typed in the Ponte is never answered for, whatever the page shows.
        let (reply, _) = ask(
            &mut store,
            json!({"protocol_version":1,"action":"lookup","document_number":"MANUAL-2026/0001"}),
        );
        assert_eq!(reply["found"], false);
        // In the Lixeira: not found, and not opened.
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Document,
                id,
            })
            .unwrap();
        let (reply, _) = ask(
            &mut store,
            json!({"protocol_version":1,"action":"lookup","document_number":number}),
        );
        assert_eq!(reply["found"], false);
        let (reply, open) = ask(
            &mut store,
            json!({"protocol_version":1,"action":"open_document","document_id":id}),
        );
        assert_eq!(reply["error"]["code"], "unknown_document");
        assert_eq!(open, None);
    }

    #[test]
    fn rejects_partial_and_oversized_frames() {
        assert!(read_frame(&mut [1u8, 0].as_slice()).is_err());
        assert!(read_frame(&mut (MAX_FRAME as u32 + 1).to_le_bytes().as_slice()).is_err());
        assert!(read_frame(&mut [5u8, 0, 0, 0, 1].as_slice()).is_err());
        assert!(read_frame(&mut [].as_slice()).unwrap().is_none());
    }
    #[test]
    fn only_expected_extension_origin_is_accepted() {
        assert!(valid_extension_origin(&format!(
            "chrome-extension://{}/",
            EXTENSION_ID.trim()
        )));
        // The copy installed from the Chrome Web Store has its own id.
        assert_ne!(EXTENSION_ID.trim(), STORE_EXTENSION_ID.trim());
        assert!(valid_extension_origin(&format!(
            "chrome-extension://{}/",
            STORE_EXTENSION_ID.trim()
        )));
        assert!(!valid_extension_origin("https://www.sigadoc.mt.gov.br/"));
        assert!(!valid_extension_origin(
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"
        ));
        assert!(!valid_extension_origin("chrome-extension:///"));
    }
    #[test]
    fn a_refusal_answers_each_request_with_the_reason() {
        let mut input = Vec::new();
        for _ in 0..2 {
            write_frame(&mut input, &json!({"protocol_version":1,"action":"ping"})).unwrap();
        }
        let mut output = Vec::new();
        refuse(
            &mut input.as_slice(),
            &mut output,
            "origin_denied",
            "Atualize o Ponte.",
        )
        .unwrap();
        let mut reader = output.as_slice();
        for _ in 0..2 {
            let reply: Value =
                serde_json::from_slice(&read_frame(&mut reader).unwrap().unwrap()).unwrap();
            assert_eq!(reply["ok"], false);
            assert_eq!(reply["error"]["code"], "origin_denied");
            assert_eq!(reply["error"]["message"], "Atualize o Ponte.");
        }
        assert!(read_frame(&mut reader).unwrap().is_none());
    }

    fn today() -> String {
        chrono::Local::now().format("%d/%m/%Y").to_string()
    }
    fn import_request(extras: Value) -> Value {
        let capture = serde_json::to_value(crate::model::sample_capture()).unwrap();
        json!({"protocol_version":1,"action":"import_capture","capture":capture,"extras":extras})
    }

    #[test]
    fn extras_apply_after_import_and_append_to_the_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let (reply, _) = ask(
            &mut store,
            import_request(json!({"note":"  Falar com o setor  ","priority":"alta"})),
        );
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["outcome"], "imported");
        assert_eq!(reply["extras_applied"], true);
        let id = reply["document_id"].as_i64().unwrap();
        let detail = store.load_document(id).unwrap();
        assert_eq!(detail.note, format!("{} — Falar com o setor", today()));
        assert_eq!(detail.summary.priority, "alta");

        // The same page again: unchanged, and the new note is appended below
        // the existing one; the priority already matches.
        let (reply, _) = ask(
            &mut store,
            import_request(json!({"note":"Segunda observação","priority":"alta"})),
        );
        assert_eq!(reply["outcome"], "unchanged");
        assert_eq!(reply["extras_applied"], true);
        let note = store.load_document(id).unwrap().note;
        assert_eq!(
            note,
            format!(
                "{0} — Falar com o setor\n\n{0} — Segunda observação",
                today()
            )
        );

        // No extras: nothing changes and the reply says so.
        let (reply, _) = ask(&mut store, import_request(json!({})));
        assert_eq!(reply["extras_applied"], false);
        assert!(reply.get("extras_message").is_none());
        assert_eq!(store.load_document(id).unwrap().note, note);
    }

    #[test]
    fn a_retry_across_midnight_does_not_append_the_note_twice() {
        let first = appended_note("", "Falar com o setor", "09/11/2026", false).unwrap();
        assert_eq!(first, "09/11/2026 — Falar com o setor");
        // Confirmed retry the next day: the same text is already the last entry.
        assert_eq!(
            appended_note(&first, "Falar com o setor", "10/11/2026", true),
            None
        );
        // Another text, or an unchanged (typed again) import, is appended.
        assert_eq!(
            appended_note(&first, "Outro texto", "10/11/2026", true).unwrap(),
            "09/11/2026 — Falar com o setor\n\n10/11/2026 — Outro texto"
        );
        assert!(appended_note(&first, "Falar com o setor", "10/11/2026", false).is_some());
        // The same words without a date prefix are not an entry.
        assert!(
            appended_note("Falar com o setor", "Falar com o setor", "10/11/2026", true).is_some()
        );
        // Multi-paragraph notes are recognised too.
        let long = appended_note("Antes", "um\n\ndois", "09/11/2026", false).unwrap();
        assert_eq!(appended_note(&long, "um\n\ndois", "10/11/2026", true), None);
    }

    #[test]
    fn failures_use_fixed_sentences_without_the_store_message() {
        let conflict = anyhow::anyhow!(
            "Conflito de edição simultânea: este registro já foi atualizado por Servidor B (versão #3)."
        );
        let text = apply_failure("a nota não foi salva", &conflict);
        assert_eq!(
            text,
            "a nota não foi salva porque o documento foi alterado ao mesmo tempo"
        );
        assert!(!text.contains("Servidor B"));
        let other = anyhow::anyhow!("disco cheio em C:\\acervo");
        assert_eq!(
            apply_failure("a prioridade não foi alterada", &other),
            "a prioridade não foi alterada"
        );
    }

    #[test]
    fn malformed_extras_refuse_the_whole_request_before_importing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        for extras in [
            json!({"priority":"critica"}),
            json!({"note":"x".repeat(2001)}),
            json!({"note":"texto\u{0}oculto"}),
            json!({"nota":"campo desconhecido"}),
        ] {
            let (reply, open) = ask(&mut store, import_request(extras));
            assert_eq!(reply["ok"], false);
            assert!(open.is_none());
            assert!(store.list_documents("").unwrap().is_empty());
        }
        let (reply, _) = ask(&mut store, import_request(json!({"priority":"critica"})));
        assert_eq!(reply["error"]["code"], "invalid_extras");
        // Newline and tab are allowed; exactly 2000 characters too.
        let (reply, _) = ask(
            &mut store,
            import_request(json!({"note":format!("a\n\tb{}", "x".repeat(1996))})),
        );
        assert_eq!(reply["extras_applied"], true, "{reply}");
    }

    fn b64(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }
    fn tiny_png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', 1, 2, 3, 4]);
        bytes
    }
    fn tiny_pdf() -> Vec<u8> {
        b"%PDF-1.4\n% teste\n%%EOF\n".to_vec()
    }
    const WA_TEXT: &str =
        "[04/11/2026 10:00] Servidor A: bom dia\ncontinua\n[04/11/2026 10:01] Servidor B: oi";
    fn record_value() -> Value {
        json!({"kind":"whatsapp","title":"Conversa com Servidor A — 04/11/2026","origin":"Servidor A",
               "document_date":"2026-11-04","text":WA_TEXT,
               "meta":{"wa_chat":"Servidor A","wa_first":"2026-11-04T10:00","wa_last":"2026-11-04T10:01","wa_messages":"2"}})
    }
    fn record_request(record: Value) -> Value {
        json!({"protocol_version":1,"action":"import_record","record":record})
    }

    #[test]
    fn import_record_stores_a_conversation_with_its_files_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let (reply, open) = ask(&mut store, record_request(record_value()));
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["outcome"], "imported");
        assert_eq!(reply["attachments_added"], 0);
        assert_eq!(reply["extras_applied"], false);
        let id = reply["document_id"].as_i64().unwrap();
        assert_eq!(open, Some(id));
        let year = chrono::Local::now().format("%Y").to_string();
        assert_eq!(reply["document_number"], format!("MANUAL-{year}/0001"));
        let detail = store.load_document(id).unwrap();
        assert_eq!(detail.capture.metadata["manual_kind"], "whatsapp");
        assert_eq!(detail.capture.metadata["imported_from"], "whatsapp-web");
        assert_eq!(detail.capture.metadata["wa_last"], "2026-11-04T10:01");
        assert_eq!(detail.capture.text, WA_TEXT);

        // Another conversation, with two files and extras.
        let mut request = record_request(json!({
            "kind":"whatsapp","title":"Grupo da Equipe B","origin":"","document_date":"",
            "text":"[05/11/2026 09:00] Servidor B: segue"}));
        request["attachments"] = json!([
            {"name":"foto 1.png","data_base64":b64(&tiny_png())},
            {"name":"nota.pdf","data_base64":b64(&tiny_pdf())}
        ]);
        request["extras"] = json!({"note":"Responder hoje","priority":"alta"});
        let (reply, open) = ask(&mut store, request.clone());
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["outcome"], "imported");
        assert_eq!(reply["attachments_added"], 2);
        assert_eq!(reply["extras_applied"], true);
        let second = reply["document_id"].as_i64().unwrap();
        assert_eq!(open, Some(second));
        assert_eq!(
            store
                .list_attachments(crate::attachments::Owner::Document(second))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            store.load_document(second).unwrap().summary.priority,
            "alta"
        );

        // The same again: already there, nothing added, the window opens on it.
        let (reply, open) = ask(&mut store, request);
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["outcome"], "already");
        assert_eq!(reply["document_id"], second);
        assert_eq!(reply["attachments_added"], 0);
        assert_eq!(open, Some(second));
        assert_eq!(
            store
                .list_attachments(crate::attachments::Owner::Document(second))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(store.list_documents("").unwrap().len(), 2);
        // serve() tells the extension the window was opened.
        let mut input = Vec::new();
        let frame = serde_json::to_vec(&record_request(record_value())).unwrap();
        input.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        input.extend_from_slice(&frame);
        let mut output = Vec::new();
        serve(&mut input.as_slice(), &mut output, &mut store, |_| true).unwrap();
        let reply: Value =
            serde_json::from_slice(&read_frame(&mut output.as_slice()).unwrap().unwrap()).unwrap();
        assert_eq!(reply["outcome"], "already");
        assert_eq!(reply["ui_opened"], true);
    }

    #[test]
    fn import_record_refusals_store_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let with = |key: &str, value: Value| {
            let mut record = record_value();
            record[key] = value;
            record_request(record)
        };
        let with_meta = |key: &str, value: Value| {
            let mut record = record_value();
            record["meta"][key] = value;
            record_request(record)
        };
        let with_files = |files: Value| {
            let mut request = record_request(record_value());
            request["attachments"] = files;
            request
        };
        let file = |name: &str, bytes: &[u8]| json!({"name":name,"data_base64":b64(bytes)});
        let mut missing = json!({"protocol_version":1,"action":"import_record"});
        missing["extras"] = json!({});
        let mut unknown_top = record_request(record_value());
        unknown_top["extra_field"] = json!(1);
        let mut unknown_record = record_request(record_value());
        unknown_record["record"]["surprise"] = json!(1);
        let mut unknown_file = record_request(record_value());
        unknown_file["attachments"] =
            json!([{"name":"a.png","data_base64":b64(&tiny_png()),"x":1}]);
        let mut with_capture = record_request(record_value());
        with_capture["capture"] = serde_json::to_value(crate::model::sample_capture()).unwrap();
        let mut bad_extras = record_request(record_value());
        bad_extras["extras"] = json!({"priority":"critica"});
        let mut program = b"MZ\x90\x00".to_vec();
        program.resize(100, 0);
        let eleven: Vec<Value> = (0..11)
            .map(|i| file(&format!("f{i}.pdf"), format!("%PDF-1.4 {i}").as_bytes()))
            .collect();
        let mut big = tiny_pdf();
        big.resize(8 * 1024 * 1024 + 1, 0);
        let many_messages = (0..501)
            .map(|i| format!("[04/11/2026 10:00] Servidor A: mensagem {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        let cases: Vec<(Value, &str)> = vec![
            (with("kind", json!("memorando")), "invalid_record"),
            (missing, "invalid_record"),
            (unknown_top, "invalid_request"),
            (unknown_record, "invalid_request"),
            (unknown_file, "invalid_request"),
            (with_capture, "invalid_record"),
            (with("title", json!("ab")), "invalid_record"),
            (with("title", json!("t".repeat(501))), "invalid_record"),
            (with("origin", json!("o".repeat(201))), "invalid_record"),
            (with("document_date", json!("04/11/2026")), "invalid_record"),
            (with("document_date", json!("2026-1-4")), "invalid_record"),
            (with("text", json!("")), "invalid_record"),
            (
                with("text", json!("[04/11/2026 10:00] A: x\u{0}y")),
                "invalid_record",
            ),
            (
                with("text", json!("[04/11/2026 10:00] A: x\ry")),
                "invalid_record",
            ),
            (
                with(
                    "text",
                    json!("x".repeat(crate::model::MAX_CAPTURE_TEXT_BYTES + 1)),
                ),
                "invalid_record",
            ),
            (with("text", json!(many_messages)), "invalid_record"),
            (
                with_meta("imported_from", json!("whatsapp-colado")),
                "invalid_record",
            ),
            (with_meta("outra", json!("x")), "invalid_record"),
            (
                with_meta("wa_first", json!("2026-11-04 10:00")),
                "invalid_record",
            ),
            (
                with_meta("wa_last", json!("2026-13-04T10:00")),
                "invalid_record",
            ),
            (with_meta("wa_messages", json!("501")), "invalid_record"),
            (with_meta("wa_messages", json!("dez")), "invalid_record"),
            (
                with_meta("wa_chat", json!("c".repeat(201))),
                "invalid_record",
            ),
            (with_meta("wa_chat", json!("a\nb")), "invalid_record"),
            (
                with_files(json!([{"name":"a.png","data_base64":"@@@"}])),
                "invalid_attachment",
            ),
            (
                with_files(json!([{"name":"a.png","data_base64":"iVBOR"}])),
                "invalid_attachment",
            ),
            (with_files(json!(eleven)), "invalid_attachment"),
            (
                with_files(json!([file("a.pdf", &big)])),
                "invalid_attachment",
            ),
            (
                with_files(json!([file("a.pdf", &[])])),
                "invalid_attachment",
            ),
            (
                with_files(json!([file("fatura.pdf", &program)])),
                "invalid_attachment",
            ),
            (
                with_files(json!([file("a.zip", b"PK\x03\x04zz")])),
                "invalid_attachment",
            ),
            (
                with_files(json!([file("a.png", &tiny_png()), file("b.exe", b"MZ..")])),
                "invalid_attachment",
            ),
            (bad_extras, "invalid_extras"),
        ];
        for (request, code) in cases {
            let (reply, open) = ask(&mut store, request.clone());
            assert_eq!(reply["ok"], false, "{request}");
            assert_eq!(reply["error"]["code"], code, "{reply}");
            assert!(open.is_none());
            assert!(!reply["error"]["message"].as_str().unwrap().is_empty());
            assert!(
                store.list_documents("").unwrap().is_empty(),
                "left a record: {reply}"
            );
            assert!(store.trash_entries().unwrap().is_empty(), "{reply}");
        }
        // The wrong kind is answered with the fixed sentence.
        let (reply, _) = ask(&mut store, with("kind", json!("email")));
        assert_eq!(
            reply["error"]["message"],
            "O Ponte só recebe conversas do WhatsApp por este caminho."
        );
        // Only lines at column 0 are messages: 600 indented header-looking
        // lines are one message; 501 real ones are refused.
        let mut one = record_request(record_value());
        one["record"]["text"] = json!(format!(
            "[04/11/2026 10:00] Servidor A: início\n{}",
            (0..600)
                .map(|_| "    [04/11/2026 10:01] Servidor B: falso")
                .collect::<Vec<_>>()
                .join("\n")
        ));
        let (reply, _) = ask(&mut store, one);
        assert_eq!(reply["ok"], true, "{reply}");
        let mut many = record_request(record_value());
        many["record"]["text"] = json!((0..501)
            .map(|i| format!("[04/11/2026 10:00] Servidor A: mensagem {i}"))
            .collect::<Vec<_>>()
            .join("\n"));
        let (reply, _) = ask(&mut store, many);
        assert_eq!(reply["error"]["code"], "invalid_record");
        // The limits are inclusive: 10 files, 500 messages, 500 characters.
        let ten: Vec<Value> = (0..10)
            .map(|i| file(&format!("f{i}.pdf"), format!("%PDF-1.4 {i}").as_bytes()))
            .collect();
        let mut request = with("title", json!("t".repeat(500)));
        request["attachments"] = json!(ten);
        request["record"]["text"] = json!((0..500)
            .map(|i| format!("[04/11/2026 10:00] Servidor A: mensagem {i}"))
            .collect::<Vec<_>>()
            .join("\n"));
        request["record"]["meta"]["wa_messages"] = json!("500");
        let (reply, _) = ask(&mut store, request);
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["attachments_added"], 10);
    }

    #[test]
    fn a_failure_while_storing_files_answers_import_failed_without_store_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER fail_attachments BEFORE INSERT ON attachments BEGIN SELECT RAISE(ABORT,'falha em C:\\acervo'); END;",
            )
            .unwrap();
        let mut request = record_request(record_value());
        request["attachments"] = json!([{"name":"a.png","data_base64":b64(&tiny_png())}]);
        let (reply, open) = ask(&mut store, request);
        assert_eq!(reply["error"]["code"], "import_failed");
        assert!(!reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("acervo"));
        assert!(open.is_none());
        assert!(store.list_documents("").unwrap().is_empty());
    }

    #[test]
    fn extras_are_not_applied_to_a_refused_record_and_import_record_is_listed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let (ping, _) = ask(&mut store, json!({"protocol_version":1,"action":"ping"}));
        assert!(ping["actions"]
            .as_array()
            .unwrap()
            .contains(&json!("import_record")));
        let mut request = record_request(record_value());
        request["record"]["kind"] = json!("outro");
        request["extras"] = json!({"note":"não deve ficar"});
        let (reply, _) = ask(&mut store, request);
        assert_eq!(reply["ok"], false);
        assert!(store.list_documents("").unwrap().is_empty());
    }

    #[test]
    fn a_frame_near_the_limit_is_handled_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        // Three files of 5.5 MiB: each one is fine, together they pass 16 MiB;
        // the frame is about 22 MiB, under MAX_FRAME.
        let mut pdf = tiny_pdf();
        pdf.resize(5632 * 1024, 7);
        let mut request = record_request(record_value());
        request["attachments"] = json!([
            {"name":"a.pdf","data_base64":b64(&pdf)},
            {"name":"b.pdf","data_base64":b64(&[&b"%PDF-1.4 b"[..], &pdf[10..]].concat())},
            {"name":"c.pdf","data_base64":b64(&[&b"%PDF-1.4 c"[..], &pdf[10..]].concat())}
        ]);
        let frame = serde_json::to_vec(&request).unwrap();
        assert!(frame.len() > 20 * 1024 * 1024 && frame.len() <= MAX_FRAME);
        let mut input = (frame.len() as u32).to_le_bytes().to_vec();
        input.extend_from_slice(&frame);
        let mut output = Vec::new();
        serve(&mut input.as_slice(), &mut output, &mut store, |_| true).unwrap();
        let reply: Value =
            serde_json::from_slice(&read_frame(&mut output.as_slice()).unwrap().unwrap()).unwrap();
        assert_eq!(reply["error"]["code"], "invalid_attachment", "{reply}");
        assert!(store.list_documents("").unwrap().is_empty());
        // Over MAX_FRAME the frame is refused before it is read.
        let too_big = (MAX_FRAME as u32 + 1).to_le_bytes();
        assert!(read_frame(&mut too_big.as_slice()).is_err());
    }
}
