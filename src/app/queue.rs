//! What happens around a job of the AI: asking before a document leaves
//! this computer (once per document and destination), waiting in the queue
//! when another job is running, and what to tell the person when the
//! document they had open is gone.
//!
//! The decisions are pure functions so the rules are tested without a window.
use super::{PonteApp, WorkKind, WorkResult};
use crate::ai::Reporter;
use crate::theme;
use crate::trash::ItemState;
use crate::widgets;
use eframe::egui;

/// Whether the person must confirm the sending first: what is sent leaves
/// this computer (`ai::leaves_this_computer`: another provider, another
/// server, or a model served from the cloud) and something that leaves has
/// not been confirmed yet for this destination.
pub(super) fn needs_consent(leaves_this_computer: bool, something_unconfirmed: bool) -> bool {
    leaves_this_computer && something_unconfirmed
}

/// Which AI job is waiting (or being started): the kind and the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct QueueKey {
    pub kind: WorkKind,
    pub document_id: i64,
}

/// What to do with a new AI job when another one is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QueueDecision {
    /// Nothing is waiting: it takes the place.
    Take,
    /// An older one of the same kind and document waits: it is replaced.
    Replace,
    /// A different job already waits: there is room for one only.
    Refuse,
}

pub(super) fn queue_decision(pending: Option<QueueKey>, new: QueueKey) -> QueueDecision {
    match pending {
        None => QueueDecision::Take,
        Some(old) if old == new => QueueDecision::Replace,
        Some(_) => QueueDecision::Refuse,
    }
}

/// An AI job that waits for the running one to end. Its inputs were captured
/// when the person clicked, as for a job started at once.
pub(super) struct QueuedJob {
    pub key: QueueKey,
    /// Shown in the status bar while it runs ("Analisando ORG-…").
    pub label: String,
    /// "analisar ORG-…" / "gerar minuta de ORG-…", for the line of the queue.
    pub waiting: String,
    /// The version of the document the inputs were read from.
    pub capture_id: i64,
    /// The context and notes the inputs were read with.
    pub context: String,
    /// The documents that leave with it and the destination, when it
    /// leaves this computer: checked again when it starts.
    pub consent: Option<(Vec<i64>, String)>,
    pub task: Box<dyn FnOnce(Reporter) -> WorkResult + Send>,
    pub ctx: eframe::egui::Context,
}

/// Whether a waiting job still describes the document: the same version, the
/// same context and notes. Otherwise it is not run, and the person is told.
pub(super) fn queued_still_valid(
    queued_capture: i64,
    queued_context: &str,
    now_capture: Option<i64>,
    now_context: Option<&str>,
) -> bool {
    now_capture == Some(queued_capture) && now_context == Some(queued_context)
}

/// What explains that an open document could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LoadFailure {
    /// Not in the library any more (deleted for good or removed by a
    /// synchronization).
    Gone,
    /// In the Lixeira.
    Trashed,
    /// Something else went wrong: the person may try again.
    Other,
}

pub(super) fn load_failure(state: Option<&ItemState>) -> LoadFailure {
    match state {
        Some(ItemState::Missing) => LoadFailure::Gone,
        Some(ItemState::Trashed { .. }) => LoadFailure::Trashed,
        _ => LoadFailure::Other,
    }
}

/// The notice of each case: the sentence and, when there is one, the button.
pub(super) fn load_failure_text(failure: LoadFailure) -> (&'static str, Option<&'static str>) {
    match failure {
        LoadFailure::Gone => (
            "Este documento não está mais no acervo: foi apagado de vez ou saiu numa sincronização. A lista foi atualizada.",
            None,
        ),
        LoadFailure::Trashed => (
            "Este documento está na Lixeira. Para abri-lo de novo, restaure-o em Ajustes ▸ Lixeira.",
            Some("Abrir a Lixeira"),
        ),
        LoadFailure::Other => (
            "Não foi possível abrir o documento agora. Tente de novo; se continuar, feche e abra o Ponte.",
            None,
        ),
    }
}

/// What the question "Enviar … a …?" is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingSend {
    Analysis,
    Draft(crate::work_templates::Intent),
    /// "Testar a IA com casos fictícios".
    Test,
}

/// What does not leave this computer, as the dialog and the hover say it.
pub(super) const NOT_SENT_LINE: &str = "Endereços da internet e identificadores internos, nomes de arquivo, outros destinatários e cópias de e-mail, quem assinou ou movimentou o documento, o cadastro de recursos, as tarefas, os atendimentos e os nomes da equipe.";

/// What a job sends, for the lines of the dialog.
pub(super) struct SentSummary<'a> {
    pub number: &'a str,
    /// Numbers of the other documents the selected sources come from.
    pub others: Vec<&'a str>,
    pub sources: usize,
    pub examples: usize,
    pub example_files: usize,
    pub draft: bool,
}

fn count_of(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The lines under "O que sai deste computador".
pub(super) fn sent_lines(sent: &SentSummary) -> Vec<String> {
    let mut lines = vec![
        format!(
            "• O texto do documento {}, com número, título, tipo, datas, remetente, destinatário e interessado, quando constam.",
            sent.number
        ),
        "• As notas e o contexto de trabalho que você salvou.".to_owned(),
        match sent.sources {
            0 => "• Nenhuma fonte selecionada.".to_owned(),
            n => format!(
                "• {} (texto das páginas).",
                count_of(n, "fonte selecionada", "fontes selecionadas")
            ),
        },
    ];
    if !sent.others.is_empty() {
        lines.push("• Também saem trechos de:".to_owned());
        for number in &sent.others {
            lines.push(format!("      {number}"));
        }
    }
    if sent.draft {
        lines.push("• A orientação desta minuta e as instruções do modelo de trabalho.".to_owned());
        if sent.examples > 0 {
            let mut line = format!(
                "• {} do modelo de trabalho",
                count_of(sent.examples, "exemplo aprovado", "exemplos aprovados")
            );
            if sent.example_files > 0 {
                line.push_str(&format!(
                    " (e {})",
                    count_of(
                        sent.example_files,
                        "arquivo de exemplo",
                        "arquivos de exemplo"
                    )
                ));
            }
            line.push('.');
            lines.push(line);
        }
    }
    lines
}

/// The question shown before a document leaves this computer.
pub(super) struct SendConfirm {
    pub what: PendingSend,
    /// The document being analysed or drafted (0 for the fictitious cases).
    pub document_id: i64,
    pub number: String,
    /// Every document whose content leaves, this one first (the ones the
    /// person has not yet agreed to send are recorded on "Enviar").
    pub ids: Vec<i64>,
    /// The other documents the selected sources come from (id, number).
    pub others: Vec<(i64, String)>,
    pub provider: String,
    pub destination: String,
    pub model: String,
    pub sources: usize,
    /// Approved examples of the work template and the files of those
    /// examples (drafts only).
    pub examples: usize,
    pub example_files: usize,
    /// The server is this computer's Ollama, but the model is served from the
    /// cloud: the data goes to ollama.com all the same.
    pub cloud_on_local: bool,
}

/// The documents whose content leaves with a job: this one first, then every
/// other document the selected sources come from, each once.
pub(super) fn consent_scope(main: i64, source_documents: &[i64]) -> Vec<i64> {
    let mut ids = vec![main];
    for id in source_documents {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    ids
}

/// What the program decided about the question before a job.
pub(super) enum SendPlan {
    /// Nothing leaves, or everything that leaves was confirmed: go on.
    Go,
    /// Ask first.
    Ask(Box<SendConfirm>),
    /// Something is wrong and was already told to the person.
    Stop,
}

/// The notes and the saved context of a document as an AI job reads them
/// (`None` when the context cannot be read).
pub(super) fn note_and_context(
    store: &crate::storage::Store,
    editors: &std::collections::BTreeMap<i64, super::Editor>,
    id: i64,
) -> Option<String> {
    let note = editors.get(&id).map(|e| e.note.clone()).unwrap_or_default();
    store
        .load_work_context(id)
        .ok()
        .map(|context| context.analysis_text(&note))
}

/// Whether the agreement still holds for everything a waiting job sends.
pub(super) fn consent_holds(store: &crate::storage::Store, ids: &[i64], destination: &str) -> bool {
    store.ai_consents_missing(ids, destination).is_empty()
}

impl PonteApp {
    /// Whether what the AI is configured to run leaves this computer.
    pub(super) fn ai_leaves(&self) -> bool {
        // Without a model nothing can be sent at all.
        if self.ai.provider == crate::ai::AiProvider::Local && self.ai.model().trim().is_empty() {
            return false;
        }
        crate::ai::config_leaves_this_computer(&self.ai)
    }

    /// Decides whether the person is asked before this document (and the
    /// documents its selected sources come from) is sent. `sends` is what a
    /// draft adds (examples); an analysis passes the default.
    pub(super) fn send_plan(
        &mut self,
        what: PendingSend,
        detail: &crate::model::DocumentDetail,
        sends: crate::ai::DraftSends,
    ) -> SendPlan {
        if !self.ai_leaves() {
            return SendPlan::Go;
        }
        let id = detail.summary.id;
        let sources = match self.sources.selected(id, &self.store) {
            Ok(found) => found,
            Err(error) => {
                self.error(error.to_string());
                return SendPlan::Stop;
            }
        };
        let source_documents: Vec<i64> = sources.iter().map(|s| s.document_id).collect();
        let ids = consent_scope(id, &source_documents);
        let (_, destination) = self.ai_destination_text();
        let missing = self.store.ai_consents_missing(&ids, &destination);
        if !needs_consent(true, !missing.is_empty()) {
            return SendPlan::Go;
        }
        let numbers = self.store.list_documents("").unwrap_or_default();
        let others = ids
            .iter()
            .skip(1)
            .map(|other| {
                let number = numbers
                    .iter()
                    .find(|d| d.id == *other)
                    .map(|d| d.document_number.clone())
                    .unwrap_or_else(|| format!("documento #{other}"));
                (*other, number)
            })
            .collect();
        let mut question = self.send_question(
            what,
            id,
            detail.summary.document_number.clone(),
            sources.len(),
        );
        question.ids = ids;
        question.others = others;
        question.examples = sends.examples;
        question.example_files = sends.example_files;
        SendPlan::Ask(Box::new(question))
    }

    /// What a job started now needs agreed: the documents and the
    /// destination, `None` when nothing leaves this computer.
    pub(super) fn consent_of(
        &self,
        settings: &crate::ai::AiSettings,
        document_id: i64,
        sources: &[crate::evidence::Source],
    ) -> Option<(Vec<i64>, String)> {
        if !crate::ai::leaves_this_computer(settings) {
            return None;
        }
        let documents: Vec<i64> = sources.iter().map(|s| s.document_id).collect();
        let (_, destination) = self.ai_destination_text();
        Some((consent_scope(document_id, &documents), destination))
    }

    pub(super) fn send_question(
        &self,
        what: PendingSend,
        document_id: i64,
        number: String,
        sources: usize,
    ) -> SendConfirm {
        let (provider, destination) = self.ai_destination_text();
        SendConfirm {
            what,
            document_id,
            number,
            ids: vec![document_id],
            others: Vec::new(),
            provider,
            destination,
            model: self.ai.model().trim().to_owned(),
            sources,
            examples: 0,
            example_files: 0,
            cloud_on_local: self.ai.provider == crate::ai::AiProvider::Local && self.ai_leaves(),
        }
    }

    /// The question itself. "Enviar" records the agreement for every
    /// document that leaves and starts the job; "Cancelar" starts nothing.
    pub(super) fn send_confirm_window(&mut self, ctx: &egui::Context) {
        let Some(question) = &self.send_confirm else {
            return;
        };
        let title = match question.what {
            PendingSend::Test => format!("Enviar os casos fictícios a {}?", question.provider),
            _ => format!("Enviar este documento a {}?", question.provider),
        };
        let mut send = false;
        let mut cancel = false;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .default_width(540.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = theme::S1;
                match question.what {
                    PendingSend::Test => {
                        widgets::field_label(ui, "O que sai deste computador");
                        ui.label("• Só documentos fictícios de exemplo, que já acompanham o Ponte.");
                        ui.label("• Nada do seu acervo, das suas notas ou do cadastro de recursos.");
                    }
                    what => {
                        widgets::field_label(ui, "O que sai deste computador");
                        for line in sent_lines(&SentSummary {
                            number: &question.number,
                            others: question.others.iter().map(|(_, n)| n.as_str()).collect(),
                            sources: question.sources,
                            examples: question.examples,
                            example_files: question.example_files,
                            draft: matches!(what, PendingSend::Draft(_)),
                        }) {
                            ui.add(egui::Label::new(line).wrap());
                        }
                        ui.add_space(theme::S1);
                        widgets::field_label(ui, "O que não sai");
                        ui.add(egui::Label::new(NOT_SENT_LINE).wrap());
                    }
                }
                ui.add_space(theme::S1);
                widgets::field_label(ui, "Para onde");
                ui.label(format!("{} ({})", question.provider, question.destination));
                if question.cloud_on_local {
                    widgets::small_muted(
                        ui,
                        "O Ollama está neste computador, mas o modelo escolhido roda na nuvem: o que você enviar sai daqui.",
                    );
                }
                ui.label(format!(
                    "Modelo de IA: {}",
                    if question.model.is_empty() {
                        "não escolhido"
                    } else {
                        question.model.as_str()
                    }
                ));
                widgets::small_muted(
                    ui,
                    "Esta pergunta aparece uma vez por documento e destino. O resultado é uma sugestão para a sua revisão.",
                );
                ui.add_space(theme::S1);
                ui.horizontal(|ui| {
                    send = ui.add(widgets::primary("Enviar")).clicked();
                    cancel = ui.add(widgets::secondary("Cancelar")).clicked();
                });
            });
        if cancel {
            self.send_confirm = None;
            return;
        }
        if !send {
            return;
        }
        let Some(question) = self.send_confirm.take() else {
            return;
        };
        if question.what == PendingSend::Test {
            self.ai_test_consented.insert(question.destination.clone());
            self.start_ai_test(ctx);
            return;
        }
        if let Err(error) = self
            .store
            .record_ai_consents(&question.ids, &question.destination)
        {
            self.error(format!("Não foi possível registrar a confirmação: {error}"));
            return;
        }
        let Some(detail) = self
            .detail
            .clone()
            .filter(|d| d.summary.id == question.document_id)
        else {
            self.error("Abra o documento de novo e repita o pedido.");
            return;
        };
        match question.what {
            PendingSend::Analysis => self.analyze_now(&detail, ctx),
            PendingSend::Draft(intent) => self.generate_now(&detail, intent, ctx),
            PendingSend::Test => {}
        }
    }

    /// Starts an AI job now, or leaves it waiting behind the running one
    /// (one place only; the same job again takes the place of the older).
    pub(super) fn run_ai_job(&mut self, job: QueuedJob) {
        if self.worker.is_none() {
            self.start_job(job);
            return;
        }
        match queue_decision(self.queued.as_ref().map(|q| q.key), job.key) {
            QueueDecision::Refuse => {
                let waiting = self
                    .queued
                    .as_ref()
                    .map(|q| q.waiting.clone())
                    .unwrap_or_default();
                self.error(format!(
                    "Já há um trabalho esperando na fila ({waiting}). Tire-o da fila, na barra de baixo, ou espere o trabalho atual terminar."
                ));
            }
            QueueDecision::Take | QueueDecision::Replace => {
                self.message(format!(
                    "Na fila: {}. Começa quando terminar o trabalho atual.",
                    job.waiting
                ));
                self.queued = Some(job);
            }
        }
    }

    fn start_job(&mut self, job: QueuedJob) {
        let QueuedJob {
            key,
            label,
            task,
            ctx,
            ..
        } = job;
        self.spawn(label, key.kind, task, &ctx);
    }

    /// Called every frame: when the worker is free, the waiting job starts,
    /// unless the document changed meanwhile (then the person is told).
    pub(super) fn start_queued(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let Some(job) = self.queued.take() else {
            return;
        };
        let id = job.key.document_id;
        let capture = self
            .store
            .document_state(id)
            .ok()
            .filter(|state| *state == ItemState::Active)
            .and_then(|_| self.store.load_document(id).ok())
            .map(|detail| detail.summary.latest_capture_id);
        if let Some((ids, destination)) = &job.consent {
            // A library replacement may have cleared the agreement meanwhile.
            if !consent_holds(&self.store, ids, destination) {
                self.message(format!(
                    "Não foi feito: {}. O envio deste documento não está mais confirmado. Peça de novo.",
                    job.waiting
                ));
                return;
            }
        }
        let context = note_and_context(&self.store, &self.editors, id);
        if queued_still_valid(job.capture_id, &job.context, capture, context.as_deref()) {
            self.start_job(job);
        } else {
            self.message(format!(
                "Não foi feito: {}. O documento, o contexto ou as notas mudaram enquanto o trabalho esperava. Peça de novo.",
                job.waiting
            ));
        }
    }

    /// "Tirar da fila".
    pub(super) fn drop_queued(&mut self) {
        if self.queued.take().is_some() {
            self.message("Tirado da fila.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: crate::ai::AiProvider, model: &str) -> crate::ai::AiConfig {
        let mut config = crate::ai::AiConfig {
            provider,
            ..Default::default()
        };
        match provider {
            crate::ai::AiProvider::Local => config.local_model = model.into(),
            crate::ai::AiProvider::Cloud => {
                config.cloud_model = model.into();
                config.cloud_key = "chave".into();
            }
            _ => {}
        }
        config
    }

    #[test]
    fn what_stays_on_this_computer_never_asks_and_what_leaves_asks_once() {
        assert!(!needs_consent(false, true));
        assert!(!needs_consent(false, false));
        assert!(needs_consent(true, true), "first time asks");
        assert!(!needs_consent(true, false), "then it does not");
    }

    #[test]
    fn a_cloud_model_on_the_local_server_leaves_the_computer_and_asks() {
        use crate::ai::{config_leaves_this_computer, AiProvider};
        let local = config(AiProvider::Local, "qwen3:4b-instruct");
        assert!(!config_leaves_this_computer(&local));
        assert!(!needs_consent(config_leaves_this_computer(&local), true));
        let cloud_model = config(AiProvider::Local, "gpt-oss:120b-cloud");
        assert!(config_leaves_this_computer(&cloud_model));
        assert!(needs_consent(
            config_leaves_this_computer(&cloud_model),
            true
        ));
        assert!(config_leaves_this_computer(&config(
            AiProvider::Cloud,
            "gpt-oss:120b"
        )));
    }

    #[test]
    fn the_scope_is_the_document_and_every_other_one_the_sources_come_from() {
        assert_eq!(consent_scope(1, &[]), [1]);
        assert_eq!(consent_scope(1, &[1, 1]), [1], "the document itself, once");
        assert_eq!(consent_scope(1, &[7, 1, 9, 7]), [1, 7, 9]);
    }

    fn seeded() -> (tempfile::TempDir, crate::storage::Store, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::storage::Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        crate::shots_data::seed(&mut store).unwrap();
        let ids = store
            .list_documents("")
            .unwrap()
            .into_iter()
            .map(|d| d.id)
            .take(3)
            .collect();
        (dir, store, ids)
    }

    #[test]
    fn a_document_only_in_the_sources_still_has_to_be_confirmed() {
        let (_dir, mut store, ids) = seeded();
        let destination = "https://ollama.com";
        let scope = consent_scope(ids[0], &[ids[1]]);
        assert_eq!(
            store.ai_consents_missing(&scope, destination),
            [ids[0], ids[1]]
        );
        // Confirmed for the main document only: the other still asks.
        store.record_ai_consents(&[ids[0]], destination).unwrap();
        assert_eq!(store.ai_consents_missing(&scope, destination), [ids[1]]);
        assert!(!consent_holds(&store, &scope, destination));
        // "Enviar" records all that leave.
        store.record_ai_consents(&scope, destination).unwrap();
        assert!(consent_holds(&store, &scope, destination));
        // Another destination is another question.
        assert!(!consent_holds(&store, &scope, "https://api.openai.com"));
    }

    #[test]
    fn a_waiting_job_without_its_agreement_does_not_run() {
        let (_dir, store, ids) = seeded();
        // Nothing was ever confirmed (or a library replacement cleared it).
        assert!(!consent_holds(&store, &[ids[0]], "https://ollama.com"));
    }

    #[test]
    fn the_dialog_lists_what_leaves_in_plain_words() {
        let lines = sent_lines(&SentSummary {
            number: "ORG-OFI-2026/00431",
            others: vec!["ORG-MEM-2026/01207"],
            sources: 2,
            examples: 3,
            example_files: 1,
            draft: true,
        });
        let text = lines.join("\n");
        assert!(text.contains("O texto do documento ORG-OFI-2026/00431, com número, título, tipo, datas, remetente, destinatário e interessado, quando constam."));
        assert!(text.contains("As notas e o contexto de trabalho que você salvou."));
        assert!(text.contains("2 fontes selecionadas (texto das páginas)."));
        assert!(text.contains("Também saem trechos de:"));
        assert!(text.contains("ORG-MEM-2026/01207"));
        assert!(
            text.contains("3 exemplos aprovados do modelo de trabalho (e 1 arquivo de exemplo).")
        );
        let plain = sent_lines(&SentSummary {
            number: "ORG-1",
            others: Vec::new(),
            sources: 0,
            examples: 0,
            example_files: 0,
            draft: false,
        })
        .join("\n");
        assert!(plain.contains("Nenhuma fonte selecionada."));
        assert!(!plain.contains("Também saem") && !plain.contains("exemplo"));
        assert!(NOT_SENT_LINE.contains("o cadastro de recursos"));
        assert!(NOT_SENT_LINE.contains("quem assinou ou movimentou o documento"));
    }

    fn key(kind: WorkKind, document_id: i64) -> QueueKey {
        QueueKey { kind, document_id }
    }

    #[test]
    fn one_waiting_job_is_replaced_by_the_same_kind_and_document_and_blocks_others() {
        let analysis = key(WorkKind::Analysis, 1);
        assert_eq!(queue_decision(None, analysis), QueueDecision::Take);
        assert_eq!(
            queue_decision(Some(analysis), key(WorkKind::Analysis, 1)),
            QueueDecision::Replace
        );
        assert_eq!(
            queue_decision(Some(analysis), key(WorkKind::Analysis, 2)),
            QueueDecision::Refuse,
            "another document"
        );
        assert_eq!(
            queue_decision(Some(analysis), key(WorkKind::Draft, 1)),
            QueueDecision::Refuse,
            "another kind"
        );
    }

    #[test]
    fn a_waiting_job_runs_only_while_the_document_is_as_it_was() {
        assert!(queued_still_valid(4, "ctx", Some(4), Some("ctx")));
        assert!(
            !queued_still_valid(4, "ctx", Some(5), Some("ctx")),
            "new version"
        );
        assert!(
            !queued_still_valid(4, "ctx", Some(4), Some("outro")),
            "context or notes changed"
        );
        assert!(
            !queued_still_valid(4, "ctx", None, Some("ctx")),
            "document gone"
        );
        assert!(
            !queued_still_valid(4, "ctx", Some(4), None),
            "context unreadable"
        );
    }

    #[test]
    fn a_vanished_document_is_told_apart_from_a_trashed_one_and_from_a_failure() {
        assert_eq!(load_failure(Some(&ItemState::Missing)), LoadFailure::Gone);
        assert_eq!(
            load_failure(Some(&ItemState::Trashed {
                deleted_at: "2031-01-01T00:00:00Z".into()
            })),
            LoadFailure::Trashed
        );
        assert_eq!(load_failure(Some(&ItemState::Active)), LoadFailure::Other);
        assert_eq!(load_failure(None), LoadFailure::Other);
        let (gone, action) = load_failure_text(LoadFailure::Gone);
        assert!(gone.starts_with("Este documento não está mais no acervo"));
        assert!(action.is_none());
        let (trashed, action) = load_failure_text(LoadFailure::Trashed);
        assert!(trashed.contains("Ajustes ▸ Lixeira"));
        assert_eq!(action, Some("Abrir a Lixeira"));
    }
}
