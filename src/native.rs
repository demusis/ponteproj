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
}

/// What this Ponte answers besides importing; the extension reads it from
/// the `ping` to know whether the Ponte is recent enough.
pub const ACTIONS: [&str; 2] = ["lookup", "open_document"];

/// Numbers are compared as the library does: without spaces, in capitals.
fn same_number(a: &str, b: &str) -> bool {
    let fold = |text: &str| text.split_whitespace().collect::<String>().to_uppercase();
    fold(a) == fold(b)
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
            match store.list_documents("") {
                Ok(documents) => {
                    match documents
                        .iter()
                        .find(|d| !d.manual && same_number(&d.document_number, number))
                    {
                        Some(found) => (
                            json!({"protocol_version":1,"ok":true,"found":true,"document_id":found.id,"imported_at":crate::labels::datetime(&found.last_seen_at)}),
                            None,
                        ),
                        None => (json!({"protocol_version":1,"ok":true,"found":false}), None),
                    }
                }
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
            let known = request.document_id.filter(|id| {
                store
                    .list_documents("")
                    .is_ok_and(|documents| documents.iter().any(|d| d.id == *id))
            });
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
        "import_capture" | "import_bundle" => {
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
        assert_eq!(ping["actions"], json!(["lookup", "open_document"]));

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
}
