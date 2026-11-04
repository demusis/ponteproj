use crate::{protection, storage::Store};
use anyhow::{ensure, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const PURPOSES: &[&str] = &[
    "nao definido",
    "ciencia e download",
    "responder",
    "examinar",
    "encaminhar",
    "acompanhar",
];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkContext {
    pub purpose: String,
    #[serde(default)]
    pub subjects: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub priority_reason: String,
    pub sender: String,
    pub received_date: String,
    pub process_number: String,
    pub notes: String,
}
impl Default for WorkContext {
    fn default() -> Self {
        Self {
            purpose: "nao definido".into(),
            subjects: String::new(),
            document_type: String::new(),
            priority: String::new(),
            priority_reason: String::new(),
            sender: String::new(),
            received_date: String::new(),
            process_number: String::new(),
            notes: String::new(),
        }
    }
}
impl WorkContext {
    fn validate(&self) -> Result<()> {
        ensure!(
            PURPOSES.contains(&self.purpose.as_str()),
            "Finalidade inválida."
        );
        ensure!(
            self.sender.len() <= 1000
                && self.process_number.len() <= 256
                && self.notes.len() <= 16000,
            "Contexto excede os limites dos campos."
        );
        ensure!(
            self.subjects.len() <= 2000
                && self.document_type.len() <= 256
                && self.priority_reason.len() <= 4000,
            "Classificação excede os limites."
        );
        ensure!(
            ["", "baixa", "normal", "alta", "urgente"].contains(&self.priority.as_str()),
            "Prioridade inválida."
        );
        if !self.received_date.is_empty() {
            let date = chrono::NaiveDate::parse_from_str(&self.received_date, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("Data de recebimento inválida; use dd/mm/aaaa."))?;
            ensure!(
                date.format("%Y-%m-%d").to_string() == self.received_date,
                "Data de recebimento inválida; use dd/mm/aaaa."
            );
        }
        Ok(())
    }
    pub fn analysis_text(&self, document_notes: &str) -> String {
        if self == &Self::default() && document_notes.is_empty() {
            return String::new();
        }
        serde_json::json!({"informado_pelo_usuario":self,"notas_salvas":document_notes}).to_string()
    }
}
impl Store {
    pub(crate) fn ensure_work_context(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS work_context(document_id INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,payload BLOB NOT NULL);")?;
        tx.commit()?;
        Ok(())
    }
    pub fn load_work_context(&self, document_id: i64) -> Result<WorkContext> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT payload FROM work_context WHERE document_id=?1",
                [document_id],
                |r| r.get(0),
            )
            .optional()?;
        match blob {
            Some(b) => Ok(serde_json::from_slice(&protection::unseal(&b)?)?),
            None => Ok(WorkContext::default()),
        }
    }
    pub fn save_work_context(&mut self, document_id: i64, value: &WorkContext) -> Result<()> {
        self.save_work_context_checked(document_id, value, None)
    }
    pub fn save_work_context_checked(
        &mut self,
        document_id: i64,
        value: &WorkContext,
        expected_version: Option<i64>,
    ) -> Result<()> {
        value.validate()?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveWorkContext {
                    document_id,
                    value: value.clone(),
                    expected_version,
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO work_context(document_id,payload) VALUES(?1,?2) ON CONFLICT(document_id) DO UPDATE SET payload=excluded.payload",params![document_id,protection::seal(&serde_json::to_vec(value)?)?])?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "work_context",
            document_id,
            expected_version,
            "save_work_context",
            Some(document_id),
            &format!("Contexto atualizado no documento #{document_id}"),
        )?;
        tx.commit()?;
        Ok(())
    }
}
#[derive(Default)]
pub struct ContextPanel {
    drafts: std::collections::BTreeMap<i64, (WorkContext, WorkContext)>,
    versions: std::collections::BTreeMap<i64, i64>,
    message: String,
    reveal: Option<i64>,
}
impl ContextPanel {
    pub fn reveal(&mut self, id: i64) {
        self.reveal = Some(id);
    }
    pub fn forget_document(&mut self, id: i64) {
        self.drafts.remove(&id);
        self.versions.remove(&id);
    }
    pub fn dirty(&self) -> bool {
        self.drafts.values().any(|(a, b)| a != b)
    }
    pub fn document_dirty(&self, id: i64) -> bool {
        self.drafts.get(&id).is_some_and(|(a, b)| a != b)
    }
    pub fn dirty_documents(&self) -> Vec<i64> {
        self.drafts
            .iter()
            .filter(|(_, (a, b))| a != b)
            .map(|(id, _)| *id)
            .collect()
    }
    /// Saves the pending edit of this document, if any, so an action that
    /// depends on the saved context (analysis, draft) can proceed without
    /// asking the user to go back and click "Salvar contexto" first.
    pub fn save_pending(&mut self, store: &mut Store, id: i64) -> Result<bool> {
        let Some((draft, saved)) = self.drafts.get_mut(&id) else {
            return Ok(false);
        };
        if draft == saved {
            return Ok(false);
        }
        let expected = self.versions.get(&id).copied();
        store.save_work_context_checked(id, draft, expected)?;
        *saved = draft.clone();
        if let Ok(v) = store.item_version("work_context", id) {
            self.versions.insert(id, v.version);
        }
        self.message = "Contexto salvo.".into();
        Ok(true)
    }
    pub fn show(&mut self, ui: &mut eframe::egui::Ui, store: &mut Store, id: i64) -> bool {
        use eframe::egui;
        if !self.drafts.contains_key(&id) {
            match store.load_work_context(id) {
                Ok(value) => {
                    let ver = store
                        .item_version("work_context", id)
                        .map(|v| v.version)
                        .unwrap_or(0);
                    self.versions.insert(id, ver);
                    self.drafts.insert(id, (value.clone(), value));
                }
                Err(e) => {
                    ui.label(format!("Falha ao ler contexto: {e}"));
                    return false;
                }
            }
        } else if !self.document_dirty(id) {
            if let Ok(value) = store.load_work_context(id) {
                if let Ok(v) = store.item_version("work_context", id) {
                    self.versions.insert(id, v.version);
                }
                self.drafts.insert(id, (value.clone(), value));
            }
        }
        let force_open = self.reveal == Some(id);
        if force_open {
            self.reveal = None;
        }
        let expected_ver = self.versions.get(&id).copied();
        let (draft, saved) = self.drafts.get_mut(&id).unwrap();
        let mut changed = false;
        let mut new_ver = None;
        egui::CollapsingHeader::new(if draft!=saved {"Contexto de trabalho ●"} else {"Contexto de trabalho"}).id_salt(("work-context",id)).default_open(false).open(if force_open{Some(true)}else{None}).show(ui,|ui| {
            ui.small("Finalidade, assuntos e orientações que só você sabe. Nada disto altera o SIGADOC.");
            egui::ComboBox::from_id_salt(("purpose",id)).selected_text(format!("Finalidade: {}",purpose_label(&draft.purpose))).show_ui(ui,|ui| {
                for purpose in PURPOSES { ui.selectable_value(&mut draft.purpose, purpose.to_string(), purpose_label(purpose)); }
            });
            ui.label("Classificação confirmada por você (assuntos separados por ponto e vírgula)");
            ui.text_edit_singleline(&mut draft.subjects);
            ui.horizontal(|ui| {
                ui.label("Tipo local confirmado (opcional)"); ui.text_edit_singleline(&mut draft.document_type);
                egui::ComboBox::from_id_salt(("document-priority", id)).selected_text(format!("Prioridade: {}", crate::labels::priority(&draft.priority))).show_ui(ui, |ui| {
                    for value in ["", "baixa", "normal", "alta", "urgente"] { ui.selectable_value(&mut draft.priority, value.to_string(), crate::labels::priority(value)); }
                });
            });
            ui.label("Motivo da prioridade"); ui.text_edit_singleline(&mut draft.priority_reason);
            egui::Grid::new(("local-context",id)).num_columns(2).show(ui,|ui| {
                ui.label("Remetente informado"); ui.text_edit_singleline(&mut draft.sender); ui.end_row();
                ui.label("Recebido em");crate::labels::date_field(ui,("context-received",id),&mut draft.received_date);ui.end_row();
                ui.label("Processo vinculado informado");ui.text_edit_singleline(&mut draft.process_number);ui.end_row();
            });
            ui.label("Orientação e contexto (por exemplo: apenas baixar e tomar ciência; não foi solicitada resposta)");
            ui.add(egui::TextEdit::multiline(&mut draft.notes).desired_rows(2).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if ui.button("Salvar contexto").clicked() {
                    match store.save_work_context_checked(id,draft,expected_ver) { Ok(())=>{*saved=draft.clone();new_ver=store.item_version("work_context",id).ok().map(|v|v.version);self.message="Contexto salvo. A próxima análise considerará estas informações.".into();changed=true;},Err(e)=>self.message=format!("Contexto não salvo: {e}") }
                }
                if ui.button("Cancelar edição do contexto").clicked() { *draft=saved.clone();self.message.clear(); }
                if draft!=saved {ui.label("Alterações não salvas");}
            });
            if !self.message.is_empty() {ui.label(&self.message);}
        });
        if let Some(ver) = new_ver {
            self.versions.insert(id, ver);
        }
        changed
    }
}

/// Renders the context that was sent to the AI (stored as JSON by
/// `analysis_text`) field by field, never as raw JSON.
pub fn show_analysis_context(ui: &mut eframe::egui::Ui, human_context: &str) {
    #[derive(Deserialize)]
    struct Stored {
        #[serde(default)]
        informado_pelo_usuario: Option<WorkContext>,
        #[serde(default)]
        notas_salvas: String,
    }
    let Ok(stored) = serde_json::from_str::<Stored>(human_context) else {
        ui.label(human_context);
        return;
    };
    let context = stored.informado_pelo_usuario.unwrap_or_default();
    let rows: [(&str, String); 8] = [
        ("Finalidade", purpose_label(&context.purpose).to_owned()),
        ("Assuntos", context.subjects.clone()),
        ("Tipo local", context.document_type.clone()),
        ("Prioridade", crate::labels::priority(&context.priority).to_owned()),
        ("Motivo da prioridade", context.priority_reason.clone()),
        ("Remetente informado", context.sender.clone()),
        ("Recebido em", crate::labels::date(&context.received_date)),
        ("Processo vinculado", context.process_number.clone()),
    ];
    eframe::egui::Grid::new(("analysis-context", human_context.len()))
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (label, value) in rows {
                if value.trim().is_empty() {
                    continue;
                }
                ui.label(label);
                ui.add(eframe::egui::Label::new(value).wrap());
                ui.end_row();
            }
        });
    if !context.notes.trim().is_empty() {
        ui.label("Orientação e contexto:");
        ui.add(eframe::egui::Label::new(&context.notes).wrap());
    }
    if !stored.notas_salvas.trim().is_empty() {
        ui.label("Notas do documento usadas:");
        ui.add(eframe::egui::Label::new(&stored.notas_salvas).wrap());
    }
}

pub fn purpose_label(value: &str) -> &str {
    match value {
        "nao definido" => "Não definida",
        "ciencia e download" => "Ciência e download",
        "responder" => "Responder",
        "examinar" => "Examinar",
        "encaminhar" => "Encaminhar",
        "acompanhar" => "Acompanhar",
        other => other,
    }
}

pub fn metadata_label(key: &str) -> &str {
    match key {
        "document_type" => "Tipo / espécie",
        "classification" => "Classificação documental oficial",
        "document_date" => "Data do documento",
        "document_model" => "Modelo do documento",
        "medium" => "Suporte",
        "origin" => "De / origem indicada",
        "registered_by" => "Cadastrante",
        "recipient" => "Para / destinatário indicado",
        "sender" => "Remetente informado na ficha",
        "sent_at" => "Data de envio informada na ficha",
        "received_at" => "Data de recebimento informada na ficha",
        "process_number" => "Processo vinculado",
        "via_code" => "Código da via",
        "via_status" => "Situação da via",
        "current_holder" => "Detentor da via",
        "signature_status" => "Marca de assinatura",
        "signature_person" => "Pessoa na marca de assinatura",
        "access_level" => "Nível de acesso",
        "access_people" => "Pessoas listadas no nível de acesso",
        "interested_party" => "Interessado",
        other => other,
    }
}
pub fn provenance(ui: &mut eframe::egui::Ui, detail: &crate::model::DocumentDetail) {
    use eframe::egui;
    let metadata = &detail.capture.metadata;
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.strong("Dados do documento no SIGADOC");
        ui.small("Informações efetivamente importadas. Situação e datas correspondem à captura, não a uma consulta em tempo real.");
        egui::Grid::new(("document-facts",detail.summary.id)).num_columns(2).striped(true).max_col_width(600.0).show(ui,|ui| {
            ui.label("Documento");ui.label(&detail.capture.document_number);ui.end_row();
            for key in ["document_type","document_date","medium","document_model","classification","process_number","via_code"] {
                ui.label(metadata_label(key));ui.add(egui::Label::new(metadata.get(key).map(String::as_str).unwrap_or("Não disponível nos campos importados")).wrap());ui.end_row();
            }
        });
        ui.small("A classificação documental oficial é distinta dos assuntos e da prioridade definidos no aplicativo.");
    });
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.strong("Pessoas, situação e acesso no SIGADOC");
        egui::Grid::new(("source-facts",detail.summary.id)).num_columns(2).striped(true).max_col_width(600.0).show(ui,|ui| {
            for key in ["origin","registered_by","recipient","current_holder","via_status","signature_status","signature_person","access_level","access_people"] {
                ui.label(metadata_label(key));ui.add(egui::Label::new(metadata.get(key).map(String::as_str).unwrap_or("Não disponível nos campos importados")).wrap());ui.end_row();
            }
            for key in ["sender","sent_at","received_at"] {
                if let Some(value)=metadata.get(key){ui.label(metadata_label(key));ui.add(egui::Label::new(value).wrap());ui.end_row();}
            }
        });
        ui.small("De/origem não identifica necessariamente quem tramitou. Recebimento e transferência também podem estar registrados na aba Histórico, com os rótulos da fonte.");
        ui.small("Marca de assinatura e nível de acesso são transcritos da fonte; não representam validação criptográfica nem alteração de permissões. Ciência/conclusão local não tramita no SIGADOC.");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_context_marks_analysis_stale_and_survives_reimport() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("context.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let mut capture = crate::model::sample_capture();
        let receipt = store.import_capture(&capture).unwrap();
        let context = WorkContext {
            purpose: "ciencia e download".into(),
            notes: "CONTEXTO_PRIVADO_TESTE; apenas ciência".into(),
            subjects: "Assiduidade; Gestão de pessoas".into(),
            document_type: "Declaração".into(),
            priority: "baixa".into(),
            priority_reason: "Apenas ciência".into(),
            ..Default::default()
        };
        store
            .save_work_context(receipt.document_id, &context)
            .unwrap();
        let summary = store.load_document(receipt.document_id).unwrap().summary;
        assert_eq!(summary.subjects, vec!["Assiduidade", "Gestão de pessoas"]);
        assert_eq!(summary.priority, "baixa");
        assert_eq!(store.list_documents("Assiduidade").unwrap().len(), 1);
        let analysis = crate::model::Analysis {
            capture_id: receipt.capture_id,
            priority: "normal".into(),
            reason: "Sem prazo".into(),
            summary: "Para ciência".into(),
            model: "test".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            human_context: context.analysis_text(""),
            ..Default::default()
        };
        store.save_analysis(receipt.document_id, &analysis).unwrap();
        assert!(
            !store
                .load_document(receipt.document_id)
                .unwrap()
                .summary
                .analysis_stale
        );
        let mut changed = context.clone();
        changed.purpose = "responder".into();
        store
            .save_work_context(receipt.document_id, &changed)
            .unwrap();
        assert!(
            store
                .load_document(receipt.document_id)
                .unwrap()
                .summary
                .analysis_stale
        );
        capture.request_id = uuid::Uuid::new_v4().to_string();
        store.import_capture(&capture).unwrap();
        assert_eq!(
            store.load_work_context(receipt.document_id).unwrap(),
            changed
        );
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        changed.received_date = "2026-02-30".into();
        assert!(store
            .save_work_context(receipt.document_id, &changed)
            .is_err());
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        assert!(!std::fs::read(path)
            .unwrap()
            .windows(b"CONTEXTO_PRIVADO_TESTE".len())
            .any(|w| w == b"CONTEXTO_PRIVADO_TESTE"));
    }
}
