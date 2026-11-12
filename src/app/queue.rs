//! What happens around a job of the AI: asking before a document leaves
//! this computer (once per document and destination), waiting in the queue
//! when another job is running, and what to tell the person when the
//! document they had open is gone.
//!
//! The decisions are pure functions so the rules are tested without a window.
use super::{PonteApp, WorkKind, WorkResult};
use crate::ai::{AiProvider, Reporter};
use crate::theme;
use crate::trash::ItemState;
use crate::widgets;
use eframe::egui;

/// Whether the person must confirm the sending first: any provider but the
/// Ollama of this computer, until they confirmed it for this document and
/// destination.
pub(super) fn needs_consent(provider: AiProvider, already_confirmed: bool) -> bool {
    provider != AiProvider::Local && !already_confirmed
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

/// The question shown before a document leaves this computer.
pub(super) struct SendConfirm {
    pub what: PendingSend,
    pub document_id: i64,
    pub number: String,
    pub provider: String,
    pub destination: String,
    pub model: String,
    pub sources: usize,
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

impl PonteApp {
    /// Whether the document must be confirmed before it is sent to the
    /// configured AI.
    pub(super) fn consent_needed(&self, document_id: i64) -> bool {
        let (_, destination) = self.ai_destination_text();
        needs_consent(
            self.ai.provider,
            self.store.ai_consent_given(document_id, &destination),
        )
    }

    /// Opens the question for this document.
    pub(super) fn ask_to_send(&mut self, what: PendingSend, detail: &crate::model::DocumentDetail) {
        let id = detail.summary.id;
        let sources = self
            .sources
            .selected(id, &self.store)
            .map(|found| found.len())
            .unwrap_or(0);
        self.send_confirm =
            Some(self.send_question(what, id, detail.summary.document_number.clone(), sources));
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
            provider,
            destination,
            model: self.ai.model().trim().to_owned(),
            sources,
        }
    }

    /// The question itself. "Enviar" records the agreement and starts the
    /// job; "Cancelar" starts nothing.
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
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = theme::S2;
                widgets::field_label(ui, "O que sai deste computador");
                match question.what {
                    PendingSend::Test => {
                        ui.label("• Só documentos fictícios de exemplo, que já acompanham o Ponte.");
                        ui.label("• Nada do seu acervo, das suas notas ou do cadastro de recursos.");
                    }
                    what => {
                        ui.label(format!(
                            "• O texto do documento {} (com número, título e datas).",
                            question.number
                        ));
                        ui.label("• As notas e o contexto de trabalho salvos.");
                        ui.label(match question.sources {
                            0 => "• Nenhuma fonte selecionada.".to_owned(),
                            1 => "• 1 fonte selecionada.".to_owned(),
                            n => format!("• {n} fontes selecionadas."),
                        });
                        if matches!(what, PendingSend::Draft(_)) {
                            ui.label("• A orientação desta minuta e as instruções do modelo de trabalho.");
                        }
                        widgets::small_muted(
                            ui,
                            "Endereços, identificadores internos e o cadastro de recursos não são enviados.",
                        );
                    }
                }
                ui.add_space(theme::S1);
                widgets::field_label(ui, "Para onde");
                ui.label(format!("{} ({})", question.provider, question.destination));
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
            .record_ai_consent(question.document_id, &question.destination)
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

    #[test]
    fn the_local_ai_never_asks_and_an_external_one_asks_once() {
        assert!(!needs_consent(AiProvider::Local, false));
        assert!(!needs_consent(AiProvider::Local, true));
        for external in [AiProvider::Cloud, AiProvider::Remote, AiProvider::ChatGpt] {
            assert!(needs_consent(external, false), "first time asks");
            assert!(!needs_consent(external, true), "then it does not");
        }
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
