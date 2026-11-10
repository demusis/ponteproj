//! Draft export to RTF and the comparison window. The old personal-template
//! table is only read by the migration to work templates; `save_personal_template`
//! and `apply` are kept for the tests of that migration and are not called by the UI.
use crate::{model::Capture, protection, storage::Store};
use anyhow::{ensure, Result};
#[cfg(feature = "desktop")]
use eframe::egui;
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct PersonalTemplate {
    pub name: String,
    pub kind: String,
    pub body: String,
    pub created_at: String,
}
impl Store {
    pub(crate) fn ensure_templates(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS personal_templates(id INTEGER PRIMARY KEY AUTOINCREMENT,payload BLOB NOT NULL);")?;
        tx.commit()?;
        Ok(())
    }
    pub fn personal_templates(&self) -> Result<Vec<(i64, PersonalTemplate)>> {
        let mut statement = self
            .conn
            .prepare("SELECT id,payload FROM personal_templates ORDER BY id DESC")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        rows.map(|row| {
            let (id, bytes) = row?;
            Ok((id, serde_json::from_slice(&protection::unseal(&bytes)?)?))
        })
        .collect()
    }
    pub fn save_personal_template(&mut self, name: &str, kind: &str, body: &str) -> Result<()> {
        ensure!(
            !name.trim().is_empty()
                && name.len() <= 256
                && !body.trim().is_empty()
                && body.len() <= 128 * 1024
                && crate::model::valid_kind(kind),
            "Nome, tipo ou corpo do modelo inválido."
        );
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: i64 =
            tx.query_row("SELECT count(*) FROM personal_templates", [], |r| r.get(0))?;
        ensure!(count < 200, "Limite de 200 versões de modelos pessoais.");
        let value = PersonalTemplate {
            name: name.into(),
            kind: kind.into(),
            body: body.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        tx.execute(
            "INSERT INTO personal_templates(payload) VALUES(?1)",
            [protection::seal(&serde_json::to_vec(&value)?)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}
pub fn apply(template: &str, capture: &Capture) -> String {
    regex::Regex::new(r"\{\{(numero_documento|assunto|destinatario|providencia|prazo)\}\}")
        .unwrap()
        .replace_all(template, |caps: &regex::Captures| match &caps[1] {
            "numero_documento" => capture.document_number.clone(),
            "assunto" => capture.title.clone(),
            "destinatario" => "[preencher destinatário]".into(),
            "providencia" => "[preencher providência]".into(),
            _ => "[conferir prazo na fonte]".into(),
        })
        .into_owned()
}
fn rtf_text(value: &str) -> String {
    let mut output = String::new();
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    for unit in value.encode_utf16() {
        match unit {
            10 => output.push_str("\\par\n"),
            13 => {}
            9 => output.push_str("\\tab "),
            92 => output.push_str("\\\\"),
            123 => output.push_str("\\{"),
            125 => output.push_str("\\}"),
            32..=126 => output.push(char::from_u32(unit as u32).unwrap()),
            _ => output.push_str(&format!("\\u{}?", unit as i16)),
        }
    }
    output
}
pub fn rich_text(capture: &Capture, body: &str) -> Result<String> {
    ensure!(
        !body.trim().is_empty() && body.len() <= 500 * 1024,
        "Minuta vazia ou grande demais."
    );
    Ok(format!("{{\\rtf1\\ansi\\ansicpg1252\\deff0{{\\fonttbl{{\\f0 Arial;}}}}\\uc1\\paperw11907\\paperh16840\\margl1417\\margr1417\\margt1417\\margb1417\\f0\\fs24\\b MINUTA PARA REVISAO\\b0\\par\nReferencia: {}\\par\nAssunto: {}\\par\\par\n{}\\par\n}}",rtf_text(&capture.document_number),rtf_text(&capture.title),rtf_text(body)))
}
pub fn export(capture: &Capture, body: &str) -> Result<std::path::PathBuf> {
    let text = rich_text(capture, body)?;
    let directory = crate::paths::data_dir()?.join("exportados");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("minuta-{}.rtf", uuid::Uuid::new_v4()));
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?
        .write_all(text.as_bytes())?;
    Ok(path)
}
#[cfg(feature = "desktop")]
/// The side-by-side comparison window of two draft texts.
#[derive(Default)]
pub struct DraftTools {
    comparison: Option<(String, String, String)>,
}
#[cfg(feature = "desktop")]
impl DraftTools {
    /// Opens the side-by-side comparison window.
    pub fn open_comparison(&mut self, label: String, before: String, after: String) {
        self.comparison = Some((label, before, after));
    }
    pub fn comparison(&mut self, ctx: &egui::Context) {
        let mut open = true;
        if let Some((label, left, right)) = &self.comparison {
            egui::Window::new("Comparação de minutas")
                .open(&mut open)
                .default_size([1000.0, 650.0])
                .show(ctx, |ui| {
                    use crate::{theme, widgets};
                    widgets::small_muted(ui, "Comparação dos textos no momento em que você abriu esta janela; edições posteriores não alteram esta visualização.");
                    ui.add_space(theme::S2);
                    ui.columns(2, |columns| {
                        widgets::section(&mut columns[0], label.as_str());
                        widgets::section(&mut columns[1], "Texto do editor");
                        let mut before = left.as_str();
                        let mut after = right.as_str();
                        egui::ScrollArea::vertical()
                            .id_salt("compare-before")
                            .max_height(550.0)
                            .show(&mut columns[0], |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut before)
                                        .desired_rows(30)
                                        .desired_width(f32::INFINITY),
                                );
                            });
                        egui::ScrollArea::vertical()
                            .id_salt("compare-after")
                            .max_height(550.0)
                            .show(&mut columns[1], |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut after)
                                        .desired_rows(30)
                                        .desired_width(f32::INFINITY),
                                );
                            });
                    });
                });
        }
        if !open {
            self.comparison = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates_are_protected_versioned_and_export_escapes_rtf_commands() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("model.sqlite3");
        let mut store = Store::open(&path).unwrap();
        for _ in 0..2 {
            store
                .save_personal_template(
                    "MODELO_PRIVADO_TESTE",
                    "resposta",
                    "Ref. {{numero_documento}} — {{destinatario}}",
                )
                .unwrap();
        }
        assert_eq!(store.personal_templates().unwrap().len(), 2);
        let capture = crate::model::sample_capture();
        assert!(
            apply(&store.personal_templates().unwrap()[0].1.body, &capture)
                .contains(&capture.document_number)
        );
        let encoded = rich_text(&capture, "Olá {campo} \\object\nNova linha 😀").unwrap();
        assert!(encoded.contains("\\{campo\\}"));
        assert!(encoded.contains("\\\\object"));
        assert!(encoded.contains("\\u225?"));
        assert!(encoded.contains("\\u-10179?\\u-8704?"));
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        assert!(!std::fs::read(&path)
            .unwrap()
            .windows(b"MODELO_PRIVADO_TESTE".len())
            .any(|b| b == b"MODELO_PRIVADO_TESTE"));
    }
}
