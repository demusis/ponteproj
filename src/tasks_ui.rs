use crate::{storage::Store, tasks::Task};
use eframe::egui::{self, Color32, RichText};

const WARNING: Color32 = Color32::from_rgb(153, 88, 16);
const ERROR: Color32 = Color32::from_rgb(168, 40, 45);
const MUTED: Color32 = Color32::from_rgb(120, 130, 140);

/// Views over the task list. The first four are the everyday ones; the rest
/// live under "Mais".
const VIEWS: [&str; 8] = [
    "Todas",
    "A triar",
    "Hoje",
    "Esta semana",
    "Atrasadas",
    "Aguardando terceiros",
    "Lembretes",
    "Sem prazo",
];
const PRIMARY_VIEWS: [usize; 4] = [2, 3, 4, 0];
const SECONDARY_VIEWS: [usize; 4] = [1, 5, 6, 7];

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
    trash_requested: Option<crate::trash::Target>,
}
impl TaskPanel {
    pub fn take_trash_request(&mut self) -> Option<crate::trash::Target> {
        self.trash_requested.take()
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
        }
    }

    pub fn focus_document(&mut self, id: i64) {
        self.document_filter = Some(id);
        self.view = 0;
        self.query.clear();
    }
    /// Opens a blank task for this document, unless another edit is pending.
    pub fn start_new(&mut self, document_id: i64) -> bool {
        if self.dirty() {
            return false;
        }
        self.document_filter = Some(document_id);
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
        self.view = 0;
        self.query.clear();
        self.draft = Some(task.clone());
        self.original = Some(task.clone());
        self.base_version = None;
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
    pub fn dirty(&self) -> bool {
        self.draft != self.original
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
                        self.notice("Salve ou cancele a edição atual antes de criar outra tarefa.", true);
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
            if let Some(id) = self.document_filter {
                ui.separator();
                ui.label(format!("Só {}", number_of(id)));
                if ui.small_button("Todos os documentos").clicked() {
                    self.document_filter = None;
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

        let mut complete_request: Option<i64> = None;
        let mut reopen_request: Option<i64> = None;
        let mut select_request: Option<Task> = None;
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
                        for task in &visible {
                            let editing_this = self.draft.as_ref().is_some_and(|t| t.id == task.id);
                            let pending = task.status != "concluida";
                            let document = documents.iter().find(|d| d.id == task.document_id);
                            ui.group(|ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    if task.overdue() {
                                        ui.colored_label(ERROR, "ATRASADA");
                                    }
                                    let title = RichText::new(&task.title).strong();
                                    if ui.selectable_label(editing_this, title).clicked() {
                                        select_request = Some((*task).clone());
                                    }
                                });
                                ui.horizontal_wrapped(|ui| {
                                    ui.small(document.map(|d| d.document_number.as_str()).unwrap_or("documento"));
                                    ui.small("•");
                                    ui.small(crate::tasks::status_label(&task.status));
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
                                    if pending {
                                        if ui
                                            .add_enabled(
                                                !(editing_this && self.dirty()),
                                                egui::Button::new("✓ Concluir").small(),
                                            )
                                            .on_hover_text("Marca como concluída. Nada é enviado ao SIGADOC.")
                                            .on_disabled_hover_text("Salve ou cancele a edição desta tarefa antes de concluí-la.")
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

        if let Some(task) = select_request {
            if self.dirty() && !self.draft.as_ref().is_some_and(|t| t.id == task.id) {
                self.notice("Salve ou cancele a edição atual antes de trocar de tarefa.", true);
            } else if !self.draft.as_ref().is_some_and(|t| t.id == task.id) {
                self.original = Some(task.clone());
                self.draft = Some(task.clone());
                self.base_version = store
                    .item_version("task", task.id)
                    .ok()
                    .map(|v| v.version);
                self.message.clear();
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
        let mut save = false;
        let mut cancel = false;
        let mut conclude = false;
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
            if ui.small_button("Abrir documento").clicked() {
                open = Some(task.document_id);
            }
        });
        ui.horizontal_wrapped(|ui| {
            save = ui
                .add_enabled(dirty, egui::Button::new("Salvar"))
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
                    ui.small("Uma tarefa só pode ser concluída depois das que ela depende.");
                    egui::ScrollArea::vertical().id_salt("dependency-options").max_height(180.0).show(ui, |ui| {
                        for previous in tasks {
                            if previous.id != task_id {
                                let mut checked = task.dependencies.contains(&previous.id);
                                if ui
                                    .checkbox(
                                        &mut checked,
                                        format!(
                                            "{} — {}",
                                            previous.title,
                                            crate::tasks::status_label(&previous.status)
                                        ),
                                    )
                                    .changed()
                                {
                                    if checked {
                                        task.dependencies.push(previous.id);
                                    } else {
                                        task.dependencies.retain(|id| *id != previous.id);
                                    }
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
        if save {
            if let Some(task) = &self.draft {
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
                    }
                    Err(e) => self.notice(format!("Tarefa não salva: {e}"), true),
                }
            }
        }
        if cancel {
            self.draft = self.original.clone();
            self.message.clear();
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
}
