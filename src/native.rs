use crate::{model::Capture, storage::Store};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, Read, Write};

pub const MAX_FRAME: usize = 24 * 1024 * 1024;
pub const PROTOCOL_VERSION: u32 = 1;
pub const HOST_NAME: &str = "com.ponteproj.assistente";
pub const EXTENSION_ID: &str = include_str!("../extension-id.txt");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol_version: u32,
    action: String,
    capture: Option<Capture>,
    files: Option<Vec<crate::files::FileInput>>,
}

pub fn valid_extension_origin(origin: &str) -> bool {
    origin == format!("chrome-extension://{}/", EXTENSION_ID.trim())
}

pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => (),
        Err(e) => return Err(e).context("Falha ao ler mensagem do navegador"),
    }
    reader
        .read_exact(&mut header[1..])
        .context("Cabeçalho de mensagem incompleto")?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        bail!("Tamanho de mensagem não permitido");
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
        bail!("Resposta excede o limite de transporte");
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn error(code: &str, message: &str) -> Value {
    json!({"protocol_version": PROTOCOL_VERSION, "ok": false, "error": {"code": code, "message": message}})
}

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
            json!({"protocol_version":1,"ok":true,"app_version":env!("CARGO_PKG_VERSION"),"message":"Aplicativo disponível"}),
            None,
        ),
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
                Err(anyhow::anyhow!("Arquivos exigem a ação import_bundle."))
            } else {
                store.import_capture(&capture)
            };
            match result {
                Ok(receipt) => (
                    json!({"protocol_version":1,"ok":true,"request_id":receipt.request_id,"document_id":receipt.document_id,"capture_id":receipt.capture_id,"duplicate":receipt.duplicate,"message":"Informações da página gravadas no acervo local."}),
                    Some(receipt.document_id),
                ),
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
            error("unsupported_action", "Ação não suportada pelo aplicativo."),
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
    let mut store = Store::open(&crate::paths::db_path()?)?;
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
        assert!(!valid_extension_origin("https://www.sigadoc.mt.gov.br/"));
        assert!(!valid_extension_origin(
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"
        ));
    }
}
