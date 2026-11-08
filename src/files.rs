//! PDFs imported together with a capture (`import_bundle`): limits, sealed
//! storage and export.
use crate::{
    model::{Capture, ImportReceipt, MAX_CAPTURE_TEXT_BYTES},
    protection,
    storage::Store,
};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileInput {
    pub url: String,
    pub data_base64: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileSummary {
    pub id: i64,
    pub label: String,
    pub url: String,
    pub sha256: String,
    pub size: usize,
    pub text_status: String,
}
pub(crate) struct PreparedFile {
    pub summary: FileSummary,
    pub data: Vec<u8>,
}

pub fn downloadable_url(raw: &str) -> bool {
    let Ok(url) = crate::model::validate_sigadoc_url(raw) else {
        return false;
    };
    if url.path() == "/sigaex/app/arquivo/exibir" {
        let pairs: Vec<_> = url.query_pairs().collect();
        let sigla = pairs
            .iter()
            .find(|(k, _)| k == "sigla")
            .map(|(_, v)| v.as_ref());
        let file = pairs
            .iter()
            .find(|(k, _)| k == "arquivo")
            .map(|(_, v)| v.as_ref());
        return pairs.len() == 2
            && sigla.is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                    && file == Some(format!("{s}.pdf").as_str())
            });
    }
    // Only static PDF resources; never follow document-action endpoints.
    url.query().is_none()
        && url.path().to_ascii_lowercase().ends_with(".pdf")
        && !url.path().contains("/app/")
        && !url.path().contains(';')
}

impl Store {
    pub(crate) fn ensure_files(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY AUTOINCREMENT,capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE,metadata BLOB NOT NULL,payload BLOB NOT NULL); CREATE INDEX IF NOT EXISTS files_capture ON files(capture_id);")?;
        if tx.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? < 3 {
            tx.pragma_update(None, "user_version", 3)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn import_bundle(
        &mut self,
        original: &Capture,
        inputs: &[FileInput],
    ) -> Result<ImportReceipt> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::ImportBundle {
                    capture: original.clone(),
                    files: inputs.to_vec(),
                },
            )? {
                crate::intranet::MutationReply::Receipt(r) => return Ok(r),
                _ => anyhow::bail!("Resposta inesperada do anfitrião ao importar pacote."),
            }
        }
        original.validate()?;
        ensure!(inputs.len() <= 10, "Máximo de 10 PDFs por importação.");
        let mut capture = original.clone();
        let mut files = Vec::new();
        let mut total = 0;
        let mut manifest = String::new();
        let mut sorted = inputs.to_vec();
        sorted.sort_by(|a, b| a.url.cmp(&b.url));
        for (index, input) in sorted.iter().enumerate() {
            ensure!(
                index == 0 || sorted[index - 1].url != input.url,
                "Arquivo duplicado no pacote."
            );
            ensure!(
                downloadable_url(&input.url),
                "Rota de arquivo não suportada."
            );
            let reference = original
                .references
                .iter()
                .find(|r| {
                    r.url == input.url && matches!(r.kind.as_str(), "original" | "attachment")
                })
                .context("Arquivo não vinculado às referências desta captura.")?;
            ensure!(
                input.data_base64.len() <= MAX_FILE_BYTES.div_ceil(3) * 4,
                "PDF excede 8 MiB."
            );
            let data = STANDARD
                .decode(&input.data_base64)
                .context("Arquivo codificado incorretamente.")?;
            total += data.len();
            ensure!(
                data.len() <= MAX_FILE_BYTES && total <= MAX_TOTAL_BYTES,
                "Arquivos excedem o limite de tamanho."
            );
            ensure!(
                data.starts_with(b"%PDF-"),
                "A resposta não é um PDF; a sessão pode ter expirado."
            );
            let hash = format!("{:x}", Sha256::digest(&data));
            let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&data))
                .ok()
                .and_then(Result::ok);
            let text_status = match text {
                Some(text) if !text.trim().is_empty() => {
                    let section = format!(
                        "\n\n════════ PDF: {} ════════\n{}",
                        reference.label,
                        text.trim()
                    );
                    if capture.text.len() + section.len() <= MAX_CAPTURE_TEXT_BYTES {
                        capture.text.push_str(&section);
                        "Texto extraído e incluído na busca e na análise".to_owned()
                    } else {
                        "PDF salvo; texto não incluído porque excede o limite de 500 KiB da captura"
                            .to_owned()
                    }
                }
                _ => "PDF salvo sem texto extraível; pode exigir OCR ou estar protegido".to_owned(),
            };
            if !text_status.starts_with("Texto extraído") {
                capture
                    .coverage
                    .pending
                    .push(format!("{}: {text_status}", reference.label));
            }
            manifest.push_str(&format!("{}\n{hash}\n", input.url));
            files.push(PreparedFile {
                summary: FileSummary {
                    id: 0,
                    label: reference.label.clone(),
                    url: input.url.clone(),
                    sha256: hash,
                    size: data.len(),
                    text_status,
                },
                data,
            });
        }
        if !files.is_empty() {
            capture.metadata.insert(
                "downloaded_files_sha256".into(),
                format!("{:x}", Sha256::digest(manifest.as_bytes())),
            );
            capture.coverage.recognized.push(format!(
                "{} PDF(s) baixado(s) e protegido(s) no acervo local",
                files.len()
            ));
            capture.coverage.pending.retain(|p| {
                !p.contains("nenhum arquivo foi baixado")
                    && !p.contains("arquivos anexos ainda não foram baixados")
            });
            if original.references.iter().any(|r| {
                matches!(r.kind.as_str(), "original" | "attachment")
                    && !files.iter().any(|f| f.summary.url == r.url)
            }) {
                capture.coverage.pending.push("Há referências a arquivos que não foram baixados; confira as pendências específicas".into());
            }
        }
        self.import_capture_with_files(&capture, &files)
    }
    pub fn list_files(&self, capture_id: i64) -> Result<Vec<FileSummary>> {
        let rows = self
            .conn
            .prepare("SELECT id,metadata FROM files WHERE capture_id=?1 ORDER BY id")?
            .query_map([capture_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, blob)| {
                let mut summary: FileSummary = serde_json::from_slice(&protection::unseal(&blob)?)?;
                summary.id = id;
                Ok(summary)
            })
            .collect()
    }
    pub fn export_pdf(&self, file_id: i64, capture_id: i64) -> Result<std::path::PathBuf> {
        let blob: Vec<u8> = self.conn.query_row(
            "SELECT payload FROM files WHERE id=?1 AND capture_id=?2",
            params![file_id, capture_id],
            |r| r.get(0),
        )?;
        let bytes = protection::unseal(&blob)?;
        let directory = crate::paths::data_dir()?.join("exportados");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("documento-{}.pdf", uuid::Uuid::new_v4()));
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(&bytes)?;
        Ok(path)
    }
}
pub(crate) fn save_files(conn: &Connection, capture_id: i64, files: &[PreparedFile]) -> Result<()> {
    for file in files {
        conn.execute(
            "INSERT INTO files(capture_id,metadata,payload) VALUES(?1,?2,?3)",
            params![
                capture_id,
                protection::seal(&serde_json::to_vec(&file.summary)?)?,
                protection::seal(&file.data)?
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bundle() -> (Capture, FileInput) {
        let mut capture = crate::model::sample_capture();
        let url = "https://www.sigadoc.mt.gov.br/arquivos/teste.pdf".to_owned();
        capture.references.push(crate::model::DocumentReference {
            label: "PDF fictício".into(),
            url: url.clone(),
            kind: "attachment".into(),
        });
        (
            capture,
            FileInput {
                url,
                data_base64: STANDARD.encode(include_bytes!("../tests/fixtures/document.pdf")),
            },
        )
    }
    #[test]
    fn pdf_bundle_is_atomic_encrypted_searchable_and_preserves_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("files.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let (capture, file) = bundle();
        let first = store.import_capture(&capture).unwrap();
        let mut next = capture.clone();
        next.request_id = uuid::Uuid::new_v4().to_string();
        let receipt = store.import_bundle(&next, std::slice::from_ref(&file)).unwrap();
        assert_ne!(receipt.capture_id, first.capture_id);
        assert_eq!(store.list_files(receipt.capture_id).unwrap().len(), 1);
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        assert!(store
            .load_document(receipt.document_id)
            .unwrap()
            .capture
            .text
            .contains("MARCADOR_PDF_TESTE"));
        assert_eq!(store.list_documents("MARCADOR_PDF_TESTE").unwrap().len(), 1);
        assert!(
            store
                .import_bundle(&next, std::slice::from_ref(&file))
                .unwrap()
                .duplicate
        );
        assert_eq!(store.list_files(receipt.capture_id).unwrap().len(), 1);
        let mut bad = file.clone();
        bad.url = "https://evil.test/teste.pdf".into();
        assert!(store.import_bundle(&next, &[bad]).is_err());
        let mut bad = file;
        bad.data_base64 = STANDARD.encode(b"<html>Login</html>");
        assert!(store.import_bundle(&next, &[bad]).is_err());
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        let bytes = std::fs::read(path).unwrap();
        assert!(!bytes
            .windows(b"MARCADOR_PDF_TESTE".len())
            .any(|w| w == b"MARCADOR_PDF_TESTE"));
        assert!(!bytes.windows(5).any(|w| w == b"%PDF-"));
    }
}
