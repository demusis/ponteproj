//! Source snapshots and literal citation verification. No network access.
use crate::storage::Store;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub document_id: i64,
    pub capture_id: i64,
    pub label: String,
    pub file_id: Option<i64>,
    pub page: Option<u32>,
    pub method: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub source_id: String,
    pub quote: String,
    pub claim: String,
}
#[derive(Clone, Default)]
pub struct Prepared {
    pub sources: Vec<Source>,
    pub pending: Vec<String>,
}
fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn validate(sources: &[Source], citations: &[Citation]) -> Result<()> {
    ensure!(
        sources.len() <= 128 && citations.len() <= 32,
        "Há fontes ou citações demais."
    );
    let mut ids = BTreeSet::new();
    let mut size = 0;
    for source in sources {
        ensure!(
            ids.insert(&source.id)
                && source.document_id > 0
                && source.capture_id > 0
                && source.id.len() <= 128
                && source.label.len() <= 1000
                && !source.text.trim().is_empty()
                && source.text.len() <= 500 * 1024,
            "Fonte inválida ou duplicada."
        );
        ensure!(
            ["captura", "pdf", "ocr"].contains(&source.method.as_str()),
            "Forma de obtenção da fonte inválida."
        );
        ensure!(
            source.method == "captura"
                || (source.file_id.is_some_and(|id| id > 0) && source.page.is_some_and(|p| p > 0)),
            "Página ou arquivo ausente na fonte."
        );
        size += source.text.len();
    }
    ensure!(
        size <= 500 * 1024,
        "As fontes selecionadas passam de 500 KB; escolha menos páginas."
    );
    for citation in citations {
        let source = sources
            .iter()
            .find(|s| s.id == citation.source_id)
            .ok_or_else(|| anyhow::anyhow!("A IA citou uma fonte que não foi selecionada."))?;
        let quote = normalized(&citation.quote);
        ensure!(
            citation.quote.len() <= 8000
                && (12..=2000).contains(&quote.len())
                && !citation.claim.trim().is_empty()
                && citation.claim.len() <= 4000,
            "Citação vazia ou fora dos limites."
        );
        ensure!(
            normalized(&source.text).contains(&quote),
            "O trecho citado pela IA não foi encontrado literalmente na fonte indicada; a resposta não foi aceita."
        );
    }
    Ok(())
}
impl Store {
    pub fn prepare_sources(&self, documents: &[i64]) -> Result<Prepared> {
        ensure!(
            !documents.is_empty() && documents.len() <= 8,
            "Selecione entre um e oito documentos."
        );
        let mut prepared = Prepared::default();
        let mut total = 0usize;
        let mut ids = BTreeSet::new();
        for &id in documents {
            ensure!(ids.insert(id), "Documento repetido na seleção.");
            let detail = self.load_document(id)?;
            let capture = detail.summary.latest_capture_id;
            let text = &detail.capture.text;
            if !text.trim().is_empty() {
                total += text.len();
                prepared.sources.push(Source {
                    id: format!("C{capture}"),
                    document_id: id,
                    capture_id: capture,
                    label: format!(
                        "{} — texto da captura (página não determinada)",
                        detail.capture.document_number
                    ),
                    file_id: None,
                    page: None,
                    method: "captura".into(),
                    text: text.clone(),
                });
            }
            for file in self.list_files(capture)? {
                let bytes = self.pdf_bytes(file.id, capture)?;
                let pages = std::panic::catch_unwind(|| {
                    pdf_extract::extract_text_from_mem_by_pages(&bytes)
                })
                .ok()
                .and_then(Result::ok);
                match pages {
                    Some(pages) => {
                        for (index, text) in pages.into_iter().enumerate() {
                            if index >= 200
                                || prepared.sources.len() >= 512
                                || total + text.len() > 2 * 1024 * 1024
                            {
                                prepared.pending.push(format!("{}: algumas páginas não foram preparadas (limite de preparação).",file.label));
                                break;
                            }
                            if !text.trim().is_empty() {
                                total += text.len();
                                prepared.sources.push(Source {
                                    id: format!("C{capture}F{}P{}", file.id, index + 1),
                                    document_id: id,
                                    capture_id: capture,
                                    label: format!(
                                        "{} — {} — página {}",
                                        detail.capture.document_number,
                                        file.label,
                                        index + 1
                                    ),
                                    file_id: Some(file.id),
                                    page: Some(index as u32 + 1),
                                    method: "pdf".into(),
                                    text,
                                });
                            }
                        }
                    }
                    None => prepared.pending.push(format!(
                        "{}: não foi possível extrair páginas de texto; confira o leitor e o OCR.",
                        file.label
                    )),
                }
                // Saved OCR is opt-in in the source selector, separate from native PDF text.
                let page_texts = self.page_texts_for_file(file.id)?;
                for (page, value) in page_texts {
                    {
                        if !value.text.trim().is_empty() {
                            if total + value.text.len() > 2 * 1024 * 1024
                                || prepared.sources.len() >= 512
                            {
                                prepared.pending.push(format!(
                                    "{}: o OCR de mais páginas não foi preparado (limite de preparação).",
                                    file.label
                                ));
                                break;
                            }
                            total += value.text.len();
                            prepared.sources.push(Source {
                                id: format!("C{capture}F{}P{}OCR", file.id, page + 1),
                                document_id: id,
                                capture_id: capture,
                                label: format!(
                                    "{} — {} — página {} — OCR {} (conferir)",
                                    detail.capture.document_number,
                                    file.label,
                                    page + 1,
                                    value.language
                                ),
                                file_id: Some(file.id),
                                page: Some(page + 1),
                                method: "ocr".into(),
                                text: value.text,
                            });
                        }
                    }
                }
            }
            for pending in detail.capture.coverage.pending {
                prepared
                    .pending
                    .push(format!("{}: {pending}", detail.capture.document_number));
            }
        }
        ensure!(
            total <= 6 * 1024 * 1024,
            "A preparação passou do limite; escolha menos documentos."
        );
        Ok(prepared)
    }
    pub fn sources_stale(&self, sources: &[Source]) -> bool {
        sources.iter().any(|source| {
            let current: rusqlite::Result<i64> = self.conn.query_row(
                "SELECT d.latest_capture_id FROM documents d WHERE d.id=?1 AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='document' AND t.item_id=d.id)",
                [source.document_id],
                |r| r.get(0),
            );
            if current.ok() != Some(source.capture_id) {
                return true;
            }
            if source.method == "ocr" {
                return match (source.file_id, source.page) {
                    (Some(file), Some(page)) => self
                        .load_page_text(file, page - 1)
                        .ok()
                        .flatten()
                        .is_none_or(|text| text.text != source.text),
                    _ => true,
                };
            }
            false
        })
    }
}
#[cfg(feature = "desktop")]
#[derive(Default)]
struct Draft {
    documents: BTreeSet<i64>,
    prepared: Option<Prepared>,
    selected: BTreeSet<String>,
    message: String,
}
#[cfg(feature = "desktop")]
#[derive(Default)]
pub struct SourcesPanel {
    drafts: BTreeMap<i64, Draft>,
}
#[cfg(feature = "desktop")]
impl SourcesPanel {
    pub fn forget_document(&mut self, id: i64) {
        self.drafts.remove(&id);
        for d in self.drafts.values_mut() {
            d.documents.remove(&id);
        }
    }
    pub fn accept(&mut self, id: i64, result: std::result::Result<Prepared, String>) {
        let draft = self.drafts.entry(id).or_default();
        match result {
            Ok(value) => {
                draft.prepared = Some(value);
                draft.selected.clear();
                draft.message =
                    "Marque os textos ou páginas que deseja enviar à IA.".into();
            }
            Err(e) => draft.message = e,
        }
    }
    pub fn selected(&self, id: i64, store: &Store) -> Result<Vec<Source>> {
        let Some(draft) = self.drafts.get(&id) else {
            return Ok(vec![]);
        };
        let sources = draft
            .prepared
            .as_ref()
            .map(|p| {
                p.sources
                    .iter()
                    .filter(|s| draft.selected.contains(&s.id))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        validate(&sources, &[])?;
        ensure!(
            !store.sources_stale(&sources),
            "As fontes selecionadas estão desatualizadas; prepare as páginas de novo."
        );
        Ok(sources)
    }
    pub fn show(
        &mut self,
        ui: &mut eframe::egui::Ui,
        store: &Store,
        id: i64,
        busy: bool,
    ) -> Option<Vec<i64>> {
        let draft = self.drafts.entry(id).or_default();
        draft.documents.insert(id);
        let mut request = None;
        eframe::egui::CollapsingHeader::new(eframe::egui::RichText::new("Outros documentos e páginas para a IA").font(crate::theme::font_strong(crate::theme::SIZE_SECTION))).show(ui,|ui|{
            crate::widgets::small_muted(ui,"Opcional: marque documentos já importados e, depois de preparar, as páginas que a IA deve considerar.").on_hover_text("A seleção vale para a próxima análise ou minuta; análises anteriores guardam as fontes que usaram. O documento atual e o contexto são sempre incluídos. Texto obtido por OCR exige conferência.");
            match store.list_documents(""){Ok(documents)=>{for document in documents{if document.id!=id{let mut checked=draft.documents.contains(&document.id);if ui.add_enabled(!busy,eframe::egui::Checkbox::new(&mut checked,format!("{} — {}",document.document_number,document.title))).changed(){if checked{draft.documents.insert(document.id);}else{draft.documents.remove(&document.id);}draft.prepared=None;draft.selected.clear();}}}},Err(e)=>{ui.label(format!("Não foi possível ler os documentos: {e}"));}}
            if ui.add_enabled(!busy,eframe::egui::Button::new(if busy {"Preparando…"} else {"Preparar páginas"})).on_hover_text("Lê os PDFs dos documentos marcados neste computador para você escolher as páginas.").clicked(){request=Some(draft.documents.iter().copied().collect());}
            if !draft.message.is_empty(){ui.label(&draft.message);}
            if let Some(prepared)=&draft.prepared {
                for pending in &prepared.pending{ui.small(format!("Cobertura: {pending}"));}
                eframe::egui::ScrollArea::vertical().id_salt(("sources",id)).max_height(260.0).show(ui,|ui|{for source in &prepared.sources{
                    let mut selected=draft.selected.contains(&source.id);if ui.add_enabled(!busy,eframe::egui::Checkbox::new(&mut selected,format!("{} • {} bytes",source.label,source.text.len()))).changed(){if selected{draft.selected.insert(source.id.clone());}else{draft.selected.remove(&source.id);}}
                    ui.push_id(&source.id,|ui|{ui.collapsing("Conferir texto antes de selecionar",|ui|{let mut text=source.text.as_str();ui.add(eframe::egui::TextEdit::multiline(&mut text).desired_rows(5).desired_width(f32::INFINITY));});});
                }});
                let size:usize=prepared.sources.iter().filter(|s|draft.selected.contains(&s.id)).map(|s|s.text.len()).sum();ui.label(format!("{} • {} KB adicionais",if draft.selected.len()==1{"1 fonte selecionada".to_owned()}else{format!("{} fontes selecionadas",draft.selected.len())},size.div_ceil(1024)));
            }
        });
        request
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn citations_must_match_selected_source_and_literal_text() {
        let source = Source {
            id: "C1F2P3".into(),
            document_id: 1,
            capture_id: 1,
            label: "Arquivo — página 3".into(),
            file_id: Some(2),
            page: Some(3),
            method: "pdf".into(),
            text: "Solicita-se resposta até 30/11/2026.\nApenas informação fictícia.".into(),
        };
        let mut citation = Citation {
            source_id: source.id.clone(),
            quote: "Solicita-se resposta até 30/11/2026.".into(),
            claim: "Prazo transcrito".into(),
        };
        validate(std::slice::from_ref(&source), &[citation.clone()]).unwrap();
        citation.quote = "O prazo é 15/11/2026 e é obrigatório".into();
        assert!(validate(std::slice::from_ref(&source), &[citation.clone()]).is_err());
        citation.source_id = "C99".into();
        assert!(validate(&[source], &[citation]).is_err());
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    #[test]
    fn source_snapshots_make_analysis_stale_when_antecedent_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("evidence.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let first = crate::model::sample_capture();
        let a = store.import_capture(&first).unwrap();
        let mut other = first.clone();
        other.request_id = uuid::Uuid::new_v4().to_string();
        other.document_number = "TESTE-OFI-2026/00099".into();
        other.source_url="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00099".into();
        other.text = "Recebido apenas para ciência e download, sem solicitação de resposta.".into();
        let b = store.import_capture(&other).unwrap();
        let sources = store.prepare_sources(&[b.document_id]).unwrap().sources;
        let citation = Citation {
            source_id: sources[0].id.clone(),
            quote: other.text.clone(),
            claim: "Finalidade explícita no antecedente".into(),
        };
        let analysis = crate::model::Analysis {
            capture_id: a.capture_id,
            subjects: vec!["Ciência".into()],
            priority: "normal".into(),
            reason: "Apenas ciência".into(),
            summary: "Documento para ciência".into(),
            model: "test".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            materials: sources,
            citations: vec![citation],
            ..Default::default()
        };
        store.save_analysis(a.document_id, &analysis).unwrap();
        store
            .save_draft_with_sources(
                a.document_id,
                a.capture_id,
                "resposta",
                "Minuta fictícia",
                &analysis.materials,
            )
            .unwrap();
        assert!(!store.load_document(a.document_id).unwrap().drafts[0].stale);
        assert!(
            !store
                .load_document(a.document_id)
                .unwrap()
                .summary
                .analysis_stale
        );
        other.request_id = uuid::Uuid::new_v4().to_string();
        other.text.push_str(" Informação nova.");
        store.import_capture(&other).unwrap();
        assert!(
            store
                .load_document(a.document_id)
                .unwrap()
                .summary
                .analysis_stale
        );
        assert!(store.load_document(a.document_id).unwrap().drafts[0].stale);
        let mut forged = analysis;
        forged.citations[0].quote = "Trecho inventado que não está na fonte".into();
        assert!(store.save_analysis(a.document_id, &forged).is_err());
    }
}
