//! The Tarefas panel: list views, the task form, dependency marks and
//! attachments of a task.
use crate::{storage::Store, tasks::Task};
use eframe::egui::{self, Color32, RichText};

const WARNING: Color32 = Color32::from_rgb(153, 88, 16);
const ERROR: Color32 = Color32::from_rgb(168, 40, 45);
const MUTED: Color32 = Color32::from_rgb(120, 130, 140);

/// Views over the task list. The first four are the everyday ones; the rest
/// live under "Mais".
const VIEWS: [&str; 9] = [
    "Todas",
    "A triar",
    "Hoje",
    "Esta semana",
    "Atrasadas",
    "Aguardando terceiros",
    "Lembretes",
    "Sem prazo",
    "Prontas para fazer",
];
const PRIMARY_VIEWS: [usize; 4] = [2, 3, 4, 0];
const SECONDARY_VIEWS: [usize; 5] = [8, 1, 5, 6, 7];

/// Bar colour of a task that depends on the one open in the form: the same
/// plain blue the document list uses for "no prazo".
const RELATED: Color32 = Color32::from_rgb(26, 95, 180);
const SUCCESS: Color32 = Color32::from_rgb(20, 118, 72);

/// A clickable chip naming a related task with its state; returns the task
/// when clicked, so the list can open it.
fn dependency_chip(ui: &mut egui::Ui, task: &Task) -> Option<Task> {
    let (mark, color) = if task.status == "concluida" {
        ("✓", MUTED)
    } else if task.overdue() {
        ("⚠", ERROR)
    } else {
        ("○", WARNING)
    };
    let mut text = RichText::new(format!("{mark} {}", task.title)).small().color(color);
    if task.status == "concluida" {
        text = text.strikethrough();
    }
    let response = ui
        .add(egui::Button::new(text).small().frame(false))
        .on_hover_text(format!(
            "{}{} • clique para abrir",
            crate::tasks::status_label(&task.status),
            if task.due_date.is_empty() {
                String::new()
            } else {
                format!(" • prazo {}", crate::labels::date(&task.due_date))
            }
        ));
    response.clicked().then(|| task.clone())
}

/// Names of tasks for a tooltip: "A, B e C".
fn titles(tasks: &[&Task]) -> String {
    let names: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].to_owned(),
        n => format!("{} e {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// One line of the "Cadeia de dependências" block: state, title, "Abrir".
fn chain_row(ui: &mut egui::Ui, task: &Task, open: &mut Option<Task>) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(12.0);
        if task.status == "concluida" {
            ui.small(RichText::new("✓").color(MUTED));
        } else if task.overdue() {
            ui.small(RichText::new("⚠").color(ERROR));
        } else {
            ui.small(RichText::new("○").color(WARNING));
        }
        ui.small(&task.title);
        ui.small(RichText::new(format!("• {}", crate::tasks::status_label(&task.status))).color(MUTED));
        if !task.due_date.is_empty() {
            ui.small(RichText::new(format!("• prazo {}", crate::labels::date(&task.due_date))).color(MUTED));
        }
        if ui.small_button("Abrir").clicked() {
            *open = Some(task.clone());
        }
    });
}

/// Something the person asked for while another task edit was still unsaved.
/// The panel never honours it by itself: the app asks what to do with the
/// pending edit first and then calls `focus_task` / `start_new`.
// One intent at a time is held while the app asks what to do with an
// unsaved edit; its size does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// Open this task in the form.
    Open(Task),
    /// Start a blank task for this document.
    New(i64),
}

#[derive(Default)]
pub struct TaskPanel {
    draft: Option<Task>,
    original: Option<Task>,
    base_version: Option<i64>,
    show_done: bool,
    query: String,
    message: String,
    message_is_error: bool,
    view: usize,
    document_filter: Option<i64>,
    /// The list follows the document open in the library (default) until
    /// the person asks for every document; selecting another document in
    /// the library brings the filter back.
    show_all_documents: bool,
    /// Document the library had selected at the last frame, to notice a change.
    last_selected_document: Option<i64>,
    trash_requested: Option<crate::trash::Target>,
    /// "Anexos" block of the task being edited.
    attachments: crate::attachments::AttachmentsPanel,
    /// Action refused because of the unsaved edit; the app resolves it.
    blocked: Option<Intent>,
    /// Document the person wants to read without leaving the form.
    consult_requested: Option<i64>,
    /// "Abrir" clicked in the dependency chain of the form.
    open_task_requested: Option<Task>,
}
impl TaskPanel {
    pub fn take_trash_request(&mut self) -> Option<crate::trash::Target> {
        self.trash_requested
            .take()
            .or_else(|| self.attachments.take_trash_request())
    }
    /// What the person tried to do while the current edit was unsaved.
    pub fn take_blocked(&mut self) -> Option<Intent> {
        self.blocked.take()
    }
    /// Document to show in the read-only consultation window.
    pub fn take_consult_request(&mut self) -> Option<i64> {
        self.consult_requested.take()
    }
    /// Title of the unsaved task, for the "leave the edit?" question.
    pub fn editing_title(&self) -> Option<String> {
        if !self.dirty() {
            return None;
        }
        let draft = self.draft.as_ref()?;
        let title = draft.title.trim();
        Some(if title.is_empty() {
            if draft.id > 0 {
                format!("tarefa #{}", draft.id)
            } else {
                "nova tarefa (sem título)".to_owned()
            }
        } else {
            title.to_owned()
        })
    }
    /// Saves the task being edited. On failure the reason is returned and
    /// also shown in the panel; the edit stays as it was.
    pub fn save_draft(&mut self, store: &mut Store) -> Result<(), String> {
        let Some(task) = &self.draft else {
            return Ok(());
        };
        match store.save_task_checked(task, self.base_version) {
            Ok(id) => {
                self.draft = store
                    .list_tasks()
                    .ok()
                    .and_then(|tasks| tasks.into_iter().find(|t| t.id == id));
                self.original = self.draft.clone();
                self.base_version = store
                    .item_version("task", id)
                    .ok()
                    .map(|v| v.version);
                let mut text = "Tarefa salva.".to_owned();
                for note in store.take_cascade_notes() {
                    text.push_str(" • ");
                    text.push_str(&note);
                }
                self.notice(text, false);
                Ok(())
            }
            Err(e) => {
                let text = format!("Tarefa não salva: {e}");
                self.notice(text.clone(), true);
                Err(text)
            }
        }
    }
    /// Drops the unsaved changes: an existing task goes back to what is
    /// saved, a new one disappears.
    pub fn discard_draft(&mut self) {
        self.draft = self.original.clone();
        self.message.clear();
        if self.draft.is_none() {
            self.base_version = None;
            self.attachments.forget();
        }
    }
    /// PDF attachment to open in the Ponte reader: (attachment id, label).
    pub fn take_attachment_pdf_request(&mut self) -> Option<(i64, String)> {
        self.attachments.take_pdf_request()
    }
    /// True while an attachment description of the open task is not saved.
    pub fn attachments_dirty(&self) -> bool {
        self.attachments.dirty()
    }
    pub fn document_dirty(&self, id: i64) -> bool {
        self.dirty()
            && self
                .draft
                .as_ref()
                .is_some_and(|task| task.document_id == id)
    }
    pub fn task_dirty(&self, id: i64) -> bool {
        self.dirty() && self.draft.as_ref().is_some_and(|task| task.id == id)
    }
    pub fn forget_task(&mut self, id: i64) {
        if self.draft.as_ref().is_some_and(|task| task.id == id) {
            self.draft = None;
            self.original = None;
            self.base_version = None;
            self.message.clear();
            self.attachments.forget();
        }
    }

    pub fn focus_document(&mut self, id: i64) {
        self.document_filter = Some(id);
        self.show_all_documents = false;
        self.view = 0;
        self.query.clear();
    }
    /// Opens a blank task for this document, unless another edit is pending.
    pub fn start_new(&mut self, document_id: i64) -> bool {
        if self.dirty() {
            return false;
        }
        self.document_filter = Some(document_id);
        self.show_all_documents = false;
        self.view = 0;
        self.query.clear();
        self.draft = Some(Task::new(document_id));
        self.original = None;
        self.base_version = None;
        self.message.clear();
        true
    }
    pub fn focus_task(&mut self, task: &Task) -> bool {
        if self.dirty() {
            return self
                .draft
                .as_ref()
                .is_some_and(|draft| draft.id == task.id && draft.document_id == task.document_id);
        }
        self.document_filter = Some(task.document_id);
        self.show_all_documents = false;
        self.view = 0;
        self.query.clear();
        self.draft = Some(task.clone());
        self.original = Some(task.clone());
        self.base_version = None;
        true
    }

    /// Opens a task from the list, keeping the current view and filter.
    /// Refused (false) while a different task has unsaved changes.
    pub fn open_task(&mut self, task: &Task, store: &Store) -> bool {
        if self.draft.as_ref().is_some_and(|t| t.id == task.id) {
            return true;
        }
        if self.dirty() {
            return false;
        }
        self.original = Some(task.clone());
        self.draft = Some(task.clone());
        self.base_version = store
            .item_version("task", task.id)
            .ok()
            .map(|v| v.version);
        self.message.clear();
        true
    }

    pub fn forget_document(&mut self, id: i64) {
        if self.document_filter == Some(id) {
            self.document_filter = None;
        }
        if self.draft.as_ref().is_some_and(|t| t.document_id == id) {
            self.draft = None;
            self.original = None;
            self.base_version = None;
            self.message.clear();
        }
    }
    pub fn prepare_suggestion(
        &mut self,
        document_id: i64,
        suggestion: &crate::model::ActionSuggestion,
        priority: &str,
    ) -> bool {
        if self.dirty() {
            return false;
        }
        let mut task = Task::new(document_id);
        task.title = format!("{} — revisar providência", suggestion.action);
        task.action = suggestion.action.clone();
        task.priority = priority.into();
        task.notes = format!(
            "Sugestão da IA para revisão.\nMotivo: {}\nBase informada: {}",
            suggestion.reason, suggestion.evidence
        );
        self.draft = Some(task);
        self.original = None;
        self.notice("Revise e salve para criar a tarefa. Nada foi enviado ao SIGADOC.", false);
        true
    }
    /// True while the form holds something that would be lost. A new task
    /// nobody has touched yet (still exactly what "Nova tarefa" proposes)
    /// has nothing to lose and does not count.
    pub fn dirty(&self) -> bool {
        match (&self.draft, &self.original) {
            (Some(draft), None) => *draft != Task::new(draft.document_id),
            (draft, original) => draft != original,
        }
    }

    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.message = text.into();
        self.message_is_error = error;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        selected_document: Option<i64>,
    ) -> Option<i64> {
        let mut open = None;
        // By default the list is the open document's; the person can widen it,
        // but picking another document in the library narrows it again.
        if selected_document != self.last_selected_document {
            self.last_selected_document = selected_document;
            if selected_document.is_some() {
                self.show_all_documents = false;
            }
        }
        if !self.show_all_documents {
            if let Some(id) = selected_document {
                if self.document_filter != Some(id) {
                    self.document_filter = Some(id);
                }
            }
        }
        let tasks = match store.list_tasks() {
            Ok(t) => t,
            Err(e) => {
                ui.label(format!("Não foi possível ler tarefas: {e}"));
                return None;
            }
        };
        let documents = store.list_documents("").unwrap_or_default();
        let number_of = |id: i64| {
            documents
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.document_number.as_str())
                .unwrap_or("documento não disponível")
        };

        ui.horizontal_wrapped(|ui| {
            ui.heading("Tarefas");
            ui.label(RichText::new("ⓘ").color(MUTED)).on_hover_text(
                "Cada documento importado recebe uma tarefa de triagem. As tarefas são locais: concluir aqui não tramita nem assina nada no SIGADOC.",
            );
            let new_label = match selected_document {
                Some(id) => format!("Nova tarefa para {}", number_of(id)),
                None => "Nova tarefa".to_owned(),
            };
            if ui
                .add_enabled(selected_document.is_some(), egui::Button::new(new_label))
                .on_disabled_hover_text("Abra um documento para criar uma tarefa ligada a ele.")
                .clicked()
            {
                if let Some(id) = selected_document {
                    if !self.start_new(id) {
                        self.blocked = Some(Intent::New(id));
                    }
                }
            }
            ui.checkbox(&mut self.show_done, "Mostrar concluídas");
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Buscar tarefa ou documento")
                    .desired_width(220.0),
            );
        });
        let belongs = |task: &&Task| self.document_filter.is_none_or(|id| task.document_id == id);
        let pending_count = tasks
            .iter()
            .filter(belongs)
            .filter(|t| t.status != "concluida")
            .count();
        let overdue_count = tasks.iter().filter(belongs).filter(|t| t.overdue()).count();
        // "Todas" counts what that view lists: pending tasks, plus the
        // concluded ones when "Mostrar concluídas" is on.
        let all_count = if self.show_done {
            tasks.iter().filter(belongs).count()
        } else {
            pending_count
        };
        ui.horizontal_wrapped(|ui| {
            for index in PRIMARY_VIEWS {
                let label = match index {
                    4 if overdue_count > 0 => {
                        RichText::new(format!("{} ({overdue_count})", VIEWS[index])).color(ERROR)
                    }
                    0 => RichText::new(format!("{} ({all_count})", VIEWS[index])),
                    _ => RichText::new(VIEWS[index]),
                };
                ui.selectable_value(&mut self.view, index, label);
            }
            let more_label = if SECONDARY_VIEWS.contains(&self.view) {
                format!("{} ▾", VIEWS[self.view])
            } else {
                "Mais ▾".to_owned()
            };
            ui.menu_button(more_label, |ui| {
                for index in SECONDARY_VIEWS {
                    if ui.selectable_value(&mut self.view, index, VIEWS[index]).clicked() {
                        ui.close();
                    }
                }
            });
            ui.separator();
            if let Some(id) = self.document_filter {
                ui.label(format!("Só {}", number_of(id)))
                    .on_hover_text("A lista segue o documento aberto na biblioteca.");
                if ui.small_button("Todos os documentos").clicked() {
                    self.document_filter = None;
                    self.show_all_documents = true;
                }
            } else {
                ui.label("Todos os documentos");
                if let Some(id) = selected_document {
                    if ui
                        .small_button(format!("Só {}", number_of(id)))
                        .on_hover_text("Volta a mostrar apenas as tarefas do documento aberto.")
                        .clicked()
                    {
                        self.document_filter = Some(id);
                        self.show_all_documents = false;
                    }
                }
            }
        });
        if !self.message.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    if self.message_is_error { ERROR } else { Color32::from_rgb(24, 84, 97) },
                    &self.message,
                );
                if ui.small_button("Fechar").clicked() {
                    self.message.clear();
                }
            });
        }
        ui.separator();

        let contexts: std::collections::BTreeMap<_, _> = documents
            .iter()
            .filter_map(|d| store.load_work_context(d.id).ok().map(|c| (d.id, c)))
            .collect();
        let today = chrono::Local::now().date_naive();
        let today_text = today.format("%Y-%m-%d").to_string();
        let week_text = (today + chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();
        let deps = crate::tasks::Dependencies::new(&tasks);
        let visible: Vec<&Task> = tasks
            .iter()
            .filter(|task| {
                if self.document_filter.is_some_and(|id| task.document_id != id) {
                    return false;
                }
                if !self.show_done && task.status == "concluida" {
                    return false;
                }
                let context = contexts.get(&task.document_id);
                let pending = task.status != "concluida";
                let matches = match self.view {
                    1 => pending && context.is_none_or(|c| c.purpose == "nao definido"),
                    2 => pending && !task.due_date.is_empty() && task.due_date == today_text,
                    3 => {
                        pending
                            && !task.due_date.is_empty()
                            && task.due_date >= today_text
                            && task.due_date <= week_text
                    }
                    4 => task.overdue(),
                    5 => task.status == "aguardando",
                    6 => {
                        pending
                            && !task.reminder_date.is_empty()
                            && task.reminder_date <= today_text
                    }
                    7 => pending && task.due_date.is_empty(),
                    8 => pending && !deps.is_blocked(task.id),
                    _ => true,
                };
                if !matches {
                    return false;
                }
                if self.query.trim().is_empty() {
                    return true;
                }
                let document = documents.iter().find(|d| d.id == task.document_id);
                let extra = context
                    .map(|c| {
                        format!(
                            "{} {} {} {} {}",
                            c.subjects, c.document_type, c.purpose, c.process_number, c.sender
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "{} {} {} {extra} {} {} {} {}",
                    task.title,
                    document.map(|d| d.document_number.as_str()).unwrap_or(""),
                    document.map(|d| d.title.as_str()).unwrap_or(""),
                    task.status,
                    task.priority,
                    task.action,
                    task.assignee
                )
                .to_lowercase()
                .contains(&self.query.to_lowercase())
            })
            .collect();
        // Each task after the ones it depends on; ties keep the usual order.
        let visible = deps.order(&visible);

        let mut complete_request: Option<i64> = None;
        let mut reopen_request: Option<i64> = None;
        let mut select_request: Option<Task> = None;
        let attachment_counts = store.attachment_counts("task").unwrap_or_default();
        let form_open = self.draft.is_some();
        let total_width = ui.available_width();
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(if form_open {
                    (total_width * 0.46).max(360.0)
                } else {
                    total_width
                });
                egui::ScrollArea::vertical()
                    .id_salt("task-manager")
                    .max_height(height)
                    .show(ui, |ui| {
                        if visible.is_empty() {
                            ui.label("Nenhuma tarefa nesta visão.");
                        }
                        // The task open in the form: its predecessors and
                        // successors get a coloured bar so the chain is
                        // visible even when the list is long.
                        let focus = self.draft.as_ref().map(|t| t.id).filter(|id| *id > 0);
                        let focus_before: Vec<i64> = focus
                            .map(|id| deps.direct(id).iter().map(|t| t.id).collect())
                            .unwrap_or_default();
                        let focus_after: Vec<i64> = focus
                            .map(|id| {
                                tasks
                                    .iter()
                                    .filter(|t| t.dependencies.contains(&id))
                                    .map(|t| t.id)
                                    .collect()
                            })
                            .unwrap_or_default();
                        for task in &visible {
                            let editing_this = self.draft.as_ref().is_some_and(|t| t.id == task.id);
                            let pending = task.status != "concluida";
                            let document = documents.iter().find(|d| d.id == task.document_id);
                            // Depth in the dependency chain, shown as indent like
                            // the joined documents in the library list.
                            let level = deps.level(task.id);
                            let indent = 18.0 * level.min(4) as f32;
                            let direct = deps.direct(task.id);
                            let blockers = deps.open_blockers(task.id);
                            let blocked = pending && !blockers.is_empty();
                            let releases: Vec<&Task> = tasks
                                .iter()
                                .filter(|t| t.dependencies.contains(&task.id))
                                .collect();
                            let relation = if focus_before.contains(&task.id) {
                                Some((WARNING, "Esta tarefa vem ANTES da tarefa aberta na ficha."))
                            } else if focus_after.contains(&task.id) {
                                Some((RELATED, "Esta tarefa vem DEPOIS da tarefa aberta na ficha (depende dela)."))
                            } else {
                                None
                            };
                            ui.horizontal(|ui| {
                            if indent > 0.0 {
                                ui.add_space(indent);
                            }
                            ui.vertical(|ui| {
                            let card = ui.group(|ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    if task.overdue() {
                                        ui.colored_label(ERROR, "ATRASADA");
                                    }
                                    if !direct.is_empty() {
                                        let (color, text) = if blocked {
                                            (WARNING, format!("⛓ {} de {} anterior(es) em aberto", blockers.len(), direct.len()))
                                        } else if pending {
                                            (SUCCESS, format!("⛓ {} anterior(es) concluída(s)", direct.len()))
                                        } else {
                                            (MUTED, format!("⛓ {} anterior(es)", direct.len()))
                                        };
                                        let detail = direct
                                            .iter()
                                            .map(|d| format!("{} {}", if d.status == "concluida" { "✓" } else if d.overdue() { "⚠" } else { "○" }, d.title))
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        ui.small(RichText::new(text).color(color)).on_hover_text(if blocked {
                                            format!("Só pode ser concluída depois de:\n{detail}")
                                        } else {
                                            format!("Depende de:\n{detail}")
                                        });
                                    }
                                    let title = RichText::new(&task.title).strong();
                                    if ui.selectable_label(editing_this, title).clicked() {
                                        select_request = Some((*task).clone());
                                    }
                                });
                                if !direct.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.small(RichText::new("↳ depende de:").color(MUTED));
                                        for dependency in &direct {
                                            if let Some(clicked) = dependency_chip(ui, dependency) {
                                                select_request = Some(clicked);
                                            }
                                        }
                                    });
                                }
                                if !releases.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.small(RichText::new("↱ libera:").color(MUTED))
                                            .on_hover_text("Tarefas que só podem ser concluídas depois desta.");
                                        for next in &releases {
                                            if let Some(clicked) = dependency_chip(ui, next) {
                                                select_request = Some(clicked);
                                            }
                                        }
                                    });
                                }
                                ui.horizontal_wrapped(|ui| {
                                    ui.small(document.map(|d| d.document_number.as_str()).unwrap_or("documento"));
                                    if document.is_some_and(|d| d.manual) {
                                        ui.small(RichText::new("Manual").color(WARNING))
                                            .on_hover_text("Registro criado no Ponte, fora do SIGADOC.");
                                    }
                                    ui.small("•");
                                    ui.small(crate::tasks::status_label(&task.status));
                                    if level > 4 {
                                        ui.small("•");
                                        ui.small(format!("·· nível {level}"))
                                            .on_hover_text("Profundidade na cadeia de dependências; o recuo para em 4 níveis.");
                                    }
                                    if matches!(task.priority.as_str(), "alta" | "urgente") {
                                        ui.small("•");
                                        ui.small(crate::labels::priority(&task.priority));
                                    }
                                    if !task.due_date.is_empty() {
                                        ui.small("•");
                                        ui.small(format!("prazo {}", crate::labels::date(&task.due_date)));
                                    }
                                    if !task.assignee.trim().is_empty() {
                                        ui.small("•");
                                        ui.small(task.assignee.trim());
                                    }
                                    if let Some(count) = attachment_counts.get(&task.id) {
                                        ui.small("•");
                                        ui.small(format!("📎 {count}"))
                                            .on_hover_text(format!("{count} anexo(s) nesta tarefa"));
                                    }
                                    if pending {
                                        let disabled_hint = if blocked {
                                            format!("Conclua antes: {}", titles(&blockers))
                                        } else {
                                            "Salve ou cancele a edição desta tarefa antes de concluí-la.".to_owned()
                                        };
                                        if ui
                                            .add_enabled(
                                                !(editing_this && self.dirty()) && !blocked,
                                                egui::Button::new("✓ Concluir").small(),
                                            )
                                            .on_hover_text("Marca como concluída. Nada é enviado ao SIGADOC.")
                                            .on_disabled_hover_text(disabled_hint)
                                            .clicked()
                                        {
                                            complete_request = Some(task.id);
                                        }
                                    } else if ui
                                        .add_enabled(
                                            !(editing_this && self.dirty()),
                                            egui::Button::new("Reabrir").small(),
                                        )
                                        .clicked()
                                    {
                                        reopen_request = Some(task.id);
                                    }
                                });
                            });
                            if let Some((color, hint)) = relation {
                                let rect = card.response.rect;
                                let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(4.0, rect.height()));
                                ui.painter().rect_filled(bar, 2.0, color);
                                card.response.on_hover_text(hint);
                            }
                            });
                            });
                        }
                    });
            });
            if form_open {
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt("task-form")
                        .max_height(height)
                        .show(ui, |ui| {
                            if let Some(id) = self.show_form(ui, store, &tasks, &documents) {
                                open = Some(id);
                            }
                        });
                });
            }
        });

        if let Some(task) = select_request.or_else(|| self.open_task_requested.take()) {
            if !self.open_task(&task, store) {
                self.blocked = Some(Intent::Open(task));
            }
        }
        if let Some(id) = complete_request {
            self.apply_status_change(store, id, true);
        }
        if let Some(id) = reopen_request {
            self.apply_status_change(store, id, false);
        }
        open
    }

    fn show_form(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        tasks: &[Task],
        documents: &[crate::model::DocumentSummary],
    ) -> Option<i64> {
        let mut open = None;
        if let Some(task) = &self.draft {
            if task.id > 0 && self.base_version.is_none() {
                self.base_version = store
                    .item_version("task", task.id)
                    .ok()
                    .map(|v| v.version);
            }
        }
        let current_actor = store.active_actor();
        let deps = crate::tasks::Dependencies::new(tasks);
        let mut save = false;
        let mut cancel = false;
        let mut conclude = false;
        let mut open_task: Option<Task> = None;
        let dirty = self.dirty();
        let Some(task) = &mut self.draft else {
            return None;
        };
        ui.horizontal_wrapped(|ui| {
            ui.heading(if task.id > 0 { "Tarefa" } else { "Nova tarefa" });
            if dirty {
                ui.colored_label(WARNING, "● não salva");
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.small(format!(
                "Documento: {}",
                documents
                    .iter()
                    .find(|d| d.id == task.document_id)
                    .map(|d| d.document_number.as_str())
                    .unwrap_or("não disponível")
            ));
            if ui
                .small_button("Consultar documento")
                .on_hover_text("Mostra o texto e os PDFs do documento numa janela por cima desta ficha, para copiar um prazo ou conferir um trecho sem sair da tarefa.")
                .clicked()
            {
                self.consult_requested = Some(task.document_id);
            }
            if ui
                .small_button("Ir para o documento")
                .on_hover_text("Abre a ficha completa do documento. Se a tarefa tiver alterações não salvas, o Ponte pergunta antes o que fazer com elas.")
                .clicked()
            {
                open = Some(task.document_id);
            }
        });
        ui.horizontal_wrapped(|ui| {
            save = ui
                .add_enabled(dirty || task.id == 0, egui::Button::new("Salvar"))
                .clicked();
            cancel = ui
                .add_enabled(dirty || task.id == 0, egui::Button::new("Cancelar"))
                .clicked();
            if task.id > 0 && task.status != "concluida" {
                conclude = ui
                    .add_enabled(!dirty, egui::Button::new("✓ Concluir"))
                    .on_disabled_hover_text("Salve ou cancele as alterações antes de concluir.")
                    .clicked();
            }
            if ui
                .add_enabled(task.id > 0 && !dirty, egui::Button::new("Excluir…"))
                .on_disabled_hover_text("Salve ou cancele as alterações antes de excluir esta tarefa.")
                .clicked()
            {
                self.trash_requested = Some(crate::trash::Target {
                    kind: crate::trash::Kind::Task,
                    id: task.id,
                });
            }
        });
        ui.add_space(4.0);
        ui.label("Título");
        ui.add(egui::TextEdit::singleline(&mut task.title).desired_width(f32::INFINITY));
        ui.horizontal_wrapped(|ui| {
            ui.label("Responsável:");
            ui.add(egui::TextEdit::singleline(&mut task.assignee).desired_width(180.0));
            if ui.small_button("Eu").on_hover_text(format!("Atribuir a {current_actor}")).clicked() {
                task.assignee = current_actor.clone();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (name, value, choices) in [
                ("Ação", &mut task.action, crate::tasks::ACTIONS),
                (
                    "Situação",
                    &mut task.status,
                    &["pendente", "em analise", "aguardando", "concluida"][..],
                ),
                (
                    "Prioridade",
                    &mut task.priority,
                    &["baixa", "normal", "alta", "urgente"][..],
                ),
            ] {
                let label_of = |v: &str| -> String {
                    match name {
                        "Ação" => crate::tasks::action_label(v).to_owned(),
                        "Situação" => crate::tasks::status_label(v).to_owned(),
                        _ => crate::labels::priority(v).to_owned(),
                    }
                };
                egui::ComboBox::from_id_salt(name)
                    .selected_text(format!("{name}: {}", label_of(value)))
                    .show_ui(ui, |ui| {
                        for choice in choices {
                            ui.selectable_value(value, choice.to_string(), label_of(choice));
                        }
                    });
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Prazo:");
            crate::labels::date_field(ui, ("task-due", task.id), &mut task.due_date);
            ui.label(RichText::new("ⓘ").color(MUTED))
                .on_hover_text("Deixe vazio se não houver prazo confirmado. Anote a origem em “Mais detalhes”.");
        });
        ui.label("Observações");
        ui.add(
            egui::TextEdit::multiline(&mut task.notes)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if let Some(at) = &task.completed_at {
            ui.small(format!("Concluída em {}", crate::labels::datetime(at)));
        }
        let task_id = task.id;
        let document_id = task.document_id;
        ui.add_space(4.0);
        ui.strong("Anexos");
        if task_id > 0 {
            self.attachments
                .show(ui, store, crate::attachments::Owner::Task(task_id), false);
        } else {
            ui.small("Salve a tarefa para poder anexar arquivos a ela.");
        }
        if task_id > 0 {
            let before = deps.ancestors(task_id);
            let after = deps.descendants(task_id);
            if !before.is_empty() || !after.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Cadeia de dependências");
                    ui.label(RichText::new("ⓘ").color(MUTED)).on_hover_text(
                        "Tudo o que vem antes desta tarefa (em qualquer nível) e tudo o que espera por ela. Para mudar as dependências diretas, use “Mais detalhes ▸ Depende de outras tarefas”.",
                    );
                });
                if !before.is_empty() {
                    ui.small("Antes desta (precisam ser concluídas primeiro):");
                    for previous in &before {
                        chain_row(ui, previous, &mut open_task);
                    }
                }
                if !after.is_empty() {
                    ui.small("Depois desta (esperam por ela):");
                    for next in &after {
                        chain_row(ui, next, &mut open_task);
                    }
                }
            }
        }
        egui::CollapsingHeader::new("Mais detalhes")
            .id_salt(("task-more", task_id))
            .show(ui, |ui| {
                ui.label("Origem do prazo (trecho, arquivo ou orientação)");
                ui.add(egui::TextEdit::singleline(&mut task.due_source).desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    ui.label("Lembrar a partir de:");
                    crate::labels::date_field(ui, ("task-reminder", task_id), &mut task.reminder_date);
                    ui.label(RichText::new("ⓘ").color(MUTED))
                        .on_hover_text("Lembrete local, mostrado na visão “Lembretes” enquanto o Ponte estiver aberto.");
                });
                ui.label("Resultado (número da resposta, data de envio, observações)");
                ui.add(
                    egui::TextEdit::multiline(&mut task.outcome)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                ui.collapsing("Checklist de providências", |ui| {
                    let mut remove = None;
                    for (index, (done, text)) in task.checklist.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.checkbox(done, "");
                            ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width() - 40.0));
                            if ui.small_button("✕").on_hover_text("Remover etapa").clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    if let Some(index) = remove {
                        task.checklist.remove(index);
                    }
                    if task.checklist.len() < 100 && ui.button("Adicionar etapa").clicked() {
                        task.checklist.push((false, String::new()));
                    }
                });
                if let Ok(doc) = store.load_document(document_id) {
                    if let Some(analysis) = doc.analysis {
                        ui.horizontal_wrapped(|ui| {
                            ui.small(format!(
                                "Sugestão da IA: prioridade {} • prazo mencionado: {}{}",
                                crate::labels::priority(&analysis.priority),
                                analysis
                                    .deadline_candidate
                                    .as_deref()
                                    .unwrap_or("não identificado"),
                                if doc.summary.analysis_stale {
                                    " • análise desatualizada"
                                } else {
                                    ""
                                }
                            ));
                            if !doc.summary.analysis_stale
                                && ui.small_button("Usar prioridade sugerida").clicked()
                            {
                                task.priority = analysis.priority;
                            }
                        });
                    }
                }
                if let Ok(context) = store.load_work_context(document_id) {
                    let action = match context.purpose.as_str() {
                        "ciencia e download" => "tomar ciencia",
                        "responder" => "responder",
                        "examinar" => "examinar",
                        "encaminhar" => "encaminhar",
                        "acompanhar" => "acompanhar",
                        _ => "",
                    };
                    ui.horizontal_wrapped(|ui| {
                        if !action.is_empty()
                            && ui
                                .small_button(format!(
                                    "Usar a finalidade da ficha: {}",
                                    crate::tasks::action_label(action)
                                ))
                                .on_hover_text("Define a ação da tarefa. O título só é preenchido se estiver vazio.")
                                .clicked()
                        {
                            task.action = action.into();
                            if task.title.trim().is_empty() {
                                task.title =
                                    format!("{} o documento", crate::tasks::action_label(action));
                            }
                        }
                        if !context.priority.is_empty()
                            && ui
                                .small_button(format!(
                                    "Usar a prioridade da ficha: {}",
                                    crate::labels::priority(&context.priority)
                                ))
                                .clicked()
                        {
                            task.priority = context.priority;
                        }
                    });
                }
                ui.collapsing("Depende de outras tarefas", |ui| {
                    ui.small("Uma tarefa só pode ser concluída depois das que ela depende. Tarefas que já dependem desta ficam desabilitadas: marcá-las formaria um ciclo.");
                    // Marking a task that already waits for this one would
                    // close a loop; saving would refuse it anyway.
                    let cycle_makers: std::collections::BTreeSet<i64> = if task_id > 0 {
                        deps.descendants(task_id).iter().map(|t| t.id).collect()
                    } else {
                        Default::default()
                    };
                    let mut options: Vec<&Task> = tasks.iter().filter(|t| t.id != task_id).collect();
                    // Concluded tasks last, greyed, so the live ones stand out.
                    options.sort_by_key(|t| t.status == "concluida");
                    egui::ScrollArea::vertical().id_salt("dependency-options").max_height(180.0).show(ui, |ui| {
                        for previous in options {
                            let mut checked = task.dependencies.contains(&previous.id);
                            let would_cycle = cycle_makers.contains(&previous.id);
                            let label = format!(
                                "{} — {}",
                                previous.title,
                                crate::tasks::status_label(&previous.status)
                            );
                            let text = if previous.status == "concluida" {
                                RichText::new(label).color(MUTED)
                            } else {
                                RichText::new(label)
                            };
                            let response = ui.add_enabled(
                                !would_cycle || checked,
                                egui::Checkbox::new(&mut checked, text),
                            );
                            let response = if would_cycle {
                                response.on_disabled_hover_text(format!(
                                    "“{}” já depende desta tarefa (direta ou indiretamente); marcar formaria um ciclo.",
                                    previous.title
                                ))
                            } else {
                                response
                            };
                            if response.changed() {
                                if checked {
                                    task.dependencies.push(previous.id);
                                } else {
                                    task.dependencies.retain(|id| *id != previous.id);
                                }
                            }
                        }
                    });
                    for missing in task.dependencies.clone() {
                        if !tasks.iter().any(|t| t.id == missing)
                            && ui
                                .button(format!("Remover dependência #{missing} (tarefa não disponível)"))
                                .clicked()
                        {
                            task.dependencies.retain(|id| *id != missing);
                        }
                    }
                });
            });
        if open_task.is_some() {
            self.open_task_requested = open_task;
        }
        if save {
            // The panel already shows the reason when saving fails.
            let _ = self.save_draft(store);
        }
        if cancel {
            self.discard_draft();
        }
        if conclude {
            self.apply_status_change(store, task_id, true);
        }
        open
    }

    fn apply_status_change(&mut self, store: &mut Store, id: i64, complete: bool) {
        let result = if complete {
            store.complete_task(id)
        } else {
            store.reopen_task(id)
        };
        match result {
            Ok(updated) => {
                if self.draft.as_ref().is_some_and(|t| t.id == id) {
                    self.draft = Some(updated.clone());
                    self.original = Some(updated.clone());
                    self.base_version = store
                        .item_version("task", id)
                        .ok()
                        .map(|v| v.version);
                }
                let mut text = if complete {
                    format!("Tarefa concluída: {}", updated.title)
                } else {
                    format!("Tarefa reaberta: {}", updated.title)
                };
                for note in store.take_cascade_notes() {
                    text.push_str(" • ");
                    text.push_str(&note);
                }
                self.notice(text, false);
            }
            Err(e) => {
                let text = format!(
                    "Não foi possível {} a tarefa: {e}",
                    if complete { "concluir" } else { "reabrir" }
                );
                self.notice(text, true);
            }
        }
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn navigation_filters_without_losing_unsaved_task() {
        let mut panel = TaskPanel::default();
        let mut first = Task::new(1);
        first.id = 10;
        assert!(panel.focus_task(&first));
        panel.draft.as_mut().unwrap().notes = "Edição ainda não salva".into();
        let mut other = Task::new(2);
        other.id = 20;
        assert!(!panel.focus_task(&other));
        panel.focus_document(2);
        assert_eq!(
            panel.draft.as_ref().unwrap().notes,
            "Edição ainda não salva"
        );
        assert!(panel.focus_task(&first));
        assert!(panel.dirty());
        assert!(!panel.start_new(3), "a pending edit is never discarded");
    }

    #[test]
    fn leaving_an_unsaved_task_is_a_question_not_a_silent_loss() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("tasks-ui.sqlite3")).unwrap();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let mut panel = TaskPanel::default();

        // A new task with text typed in: the question names it.
        assert!(panel.start_new(document));
        assert_eq!(panel.editing_title().as_deref(), None, "a blank new task has nothing to lose");
        panel.draft.as_mut().unwrap().title = "Responder ao ofício".into();
        assert_eq!(panel.editing_title().as_deref(), Some("Responder ao ofício"));

        // "Salvar e continuar" saves and leaves nothing pending.
        panel.save_draft(&mut store).unwrap();
        assert!(!panel.dirty());
        let saved = store.list_tasks().unwrap();
        assert!(saved.iter().any(|t| t.title == "Responder ao ofício"));

        // A refused save keeps the edit and explains.
        panel.draft.as_mut().unwrap().title = "   ".into();
        let error = panel.save_draft(&mut store).unwrap_err();
        assert!(error.starts_with("Tarefa não salva:"), "{error}");
        assert!(panel.dirty());

        // "Descartar alterações" on an existing task goes back to what is saved.
        panel.discard_draft();
        assert!(!panel.dirty());
        assert_eq!(panel.draft.as_ref().unwrap().title, "Responder ao ofício");

        // Discarding a brand-new task closes the form.
        let mut other = TaskPanel::default();
        assert!(other.start_new(document));
        other.draft.as_mut().unwrap().notes = "x".into();
        other.discard_draft();
        assert!(other.draft.is_none());

        // Blocked intents are handed to the app instead of being refused with a message.
        panel.draft.as_mut().unwrap().notes = "ainda escrevendo".into();
        assert!(panel.editing_title().is_some());
        assert!(!panel.start_new(document));
        assert!(panel.take_blocked().is_none(), "start_new itself does not queue; the panel's button does");
        panel.blocked = Some(Intent::New(document));
        assert_eq!(panel.take_blocked(), Some(Intent::New(document)));
        assert!(panel.take_blocked().is_none());
    }
}
