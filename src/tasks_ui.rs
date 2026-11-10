//! The Tarefas panel: list views, the task form, dependency marks and
//! attachments of a task.
use crate::theme::{self, c, icon, S1, S2, S3, S4};
use crate::widgets::{self, Tone};
use crate::{storage::Store, tasks::Task};
use eframe::egui::{self, RichText};

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
const PRIMARY_VIEWS: [usize; 4] = [0, 2, 3, 4];
const SECONDARY_VIEWS: [usize; 5] = [8, 1, 5, 6, 7];
/// Width of the side panel with the task form.
const FORM_WIDTH: f32 = 420.0;
/// Room kept on the right of a row for the priority and deadline pills.
const PILLS_WIDTH: f32 = 170.0;
/// Size of the round conclude control of a row.
const CONTROL: f32 = 22.0;

/// "2026-11-07" → "07/11"; anything else as it is written.
fn short_date(iso: &str) -> String {
    chrono::NaiveDate::parse_from_str(iso.trim(), "%Y-%m-%d")
        .map(|d| d.format("%d/%m").to_string())
        .unwrap_or_else(|_| crate::labels::date(iso))
}

/// Icon and colour that say where a related task stands.
fn state_mark(task: &Task) -> (&'static str, egui::Color32) {
    if task.status == "concluida" {
        (icon::CHECK, c().muted)
    } else if task.overdue() {
        (icon::WARNING, c().danger)
    } else {
        (icon::CLOCK, c().warn)
    }
}

/// A clickable link naming a related task with its state; returns the task
/// when clicked, so the list can open it.
fn dependency_chip(ui: &mut egui::Ui, task: &Task) -> Option<Task> {
    let (mark, color) = state_mark(task);
    let mut text = RichText::new(format!("{mark} {}", task.title))
        .size(theme::SIZE_SMALL)
        .color(color);
    if task.status == "concluida" {
        text = text.strikethrough();
    }
    let response = ui
        .add(egui::Button::new(text).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
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
        ui.add_space(S3);
        let (mark, color) = state_mark(task);
        ui.label(RichText::new(mark).color(color));
        ui.label(&task.title);
        widgets::small_muted(ui, format!("• {}", crate::tasks::status_label(&task.status)));
        if !task.due_date.is_empty() {
            widgets::small_muted(ui, format!("• prazo {}", crate::labels::date(&task.due_date)));
        }
        if ui.add(widgets::quiet("Abrir")).clicked() {
            *open = Some(task.clone());
        }
    });
}

/// Two fields side by side; `frac` is the share of the left one.
fn two_columns(
    ui: &mut egui::Ui,
    frac: f32,
    left: impl FnOnce(&mut egui::Ui),
    right: impl FnOnce(&mut egui::Ui),
) {
    let gap = S3;
    let total = ui.available_width();
    let left_width = ((total - gap) * frac).floor();
    let right_width = total - gap - left_width;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(left_width);
                left(ui);
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(right_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(right_width);
                right(ui);
            },
        );
    });
}

/// What the list needs to draw its rows.
struct ListEnv<'a> {
    tasks: &'a [Task],
    documents: &'a [crate::model::DocumentSummary],
    deps: &'a crate::tasks::Dependencies<'a>,
    attachment_counts: &'a std::collections::BTreeMap<i64, usize>,
    focus_before: &'a [i64],
    focus_after: &'a [i64],
    today: &'a str,
    /// Task open in the form, and whether its edit is unsaved.
    editing: Option<i64>,
    editing_dirty: bool,
}

/// What the person asked for in the list during this frame.
#[derive(Default)]
struct RowOut {
    select: Option<Task>,
    complete: Option<i64>,
    reopen: Option<i64>,
}

/// The round control on the left of a row: concludes a pending task, reopens
/// a concluded one. Returns true when clicked.
fn conclude_control(ui: &mut egui::Ui, done: bool, enabled: bool, hint: &str, disabled_hint: &str) -> bool {
    let p = c();
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(CONTROL, CONTROL), sense);
    let hovered = enabled && response.hovered();
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center = rect.center();
        if done {
            painter.circle_filled(center, CONTROL / 2.0 - 1.0, p.ok);
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                icon::CHECK,
                theme::font(14.0),
                p.on_primary,
            );
        } else {
            let (stroke, fill) = if hovered {
                (p.primary, p.primary_soft)
            } else if enabled {
                (p.muted, egui::Color32::TRANSPARENT)
            } else {
                (p.outline, egui::Color32::TRANSPARENT)
            };
            painter.circle(center, CONTROL / 2.0 - 1.5, fill, egui::Stroke::new(1.5, stroke));
            if hovered {
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    icon::CHECK,
                    theme::font(14.0),
                    p.primary,
                );
            }
        }
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if enabled {
        response.on_hover_text(hint).clicked()
    } else {
        response.on_hover_text(disabled_hint);
        false
    }
}

/// One task of the list. `first`/`last` round the corners of the card.
fn task_row(
    ui: &mut egui::Ui,
    env: &ListEnv,
    task: &Task,
    first: bool,
    last: bool,
    // True when a task this one waits for is right above it, in the same
    // group: only then the row is drawn hanging from it.
    nested: bool,
    out: &mut RowOut,
) {
    let p = c();
    let editing_this = env.editing == Some(task.id);
    let pending = task.status != "concluida";
    let document = env.documents.iter().find(|d| d.id == task.document_id);
    let level = usize::from(nested && env.deps.level(task.id) > 0);
    let indent = 18.0 * level as f32;
    let direct = env.deps.direct(task.id);
    let blockers = env.deps.open_blockers(task.id);
    let blocked = pending && !blockers.is_empty();
    let releases: Vec<&Task> = env
        .tasks
        .iter()
        .filter(|t| t.dependencies.contains(&task.id))
        .collect();
    let relation = if env.focus_before.contains(&task.id) {
        Some((p.warn, "Esta tarefa vem ANTES da tarefa aberta na ficha."))
    } else if env.focus_after.contains(&task.id) {
        Some((p.ok, "Esta tarefa vem DEPOIS da tarefa aberta na ficha (depende dela)."))
    } else {
        None
    };

    // The row answers clicks on its empty parts. Its area is registered
    // before the content (from the last frame), so the controls inside stay
    // on top of it.
    let id = ui.make_persistent_id(("task-row", task.id));
    let cached: Option<egui::Rect> = ui.data(|d| d.get_temp(id));
    let row = cached.map(|rect| ui.interact(rect, id, egui::Sense::click()));
    let background = ui.painter().add(egui::Shape::Noop);

    let inner = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(S3 as i8, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = S3;
                if indent > 0.0 {
                    ui.add_space(indent);
                }
                if pending {
                    let enabled = !(editing_this && env.editing_dirty) && !blocked;
                    let disabled_hint = if blocked {
                        format!("Conclua antes: {}", titles(&blockers))
                    } else {
                        "Salve ou cancele a edição desta tarefa antes de concluí-la.".to_owned()
                    };
                    if conclude_control(
                        ui,
                        false,
                        enabled,
                        "Concluir: marca como concluída. Nada é enviado ao SIGADOC.",
                        &disabled_hint,
                    ) {
                        out.complete = Some(task.id);
                    }
                } else if conclude_control(
                    ui,
                    true,
                    !(editing_this && env.editing_dirty),
                    "Reabrir esta tarefa",
                    "Salve ou cancele a edição desta tarefa antes de reabri-la.",
                ) {
                    out.reopen = Some(task.id);
                }
                let middle = (ui.available_width() - PILLS_WIDTH - S3).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(middle, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(middle);
                        ui.spacing_mut().item_spacing.y = S1;
                        ui.spacing_mut().interact_size.y = 18.0;
                        ui.spacing_mut().button_padding.y = 0.0;
                        let mut title = RichText::new(&task.title).font(theme::font_strong(theme::SIZE_BODY));
                        if !pending {
                            title = title.color(p.muted).strikethrough();
                        }
                        ui.add(egui::Label::new(title).selectable(false).wrap());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = S1;
                            if let Some(d) = document.filter(|d| d.manual) {
                                let email = d.email;
                                widgets::pill(ui, if email { "E-mail" } else { "Manual" }, Tone::Warn)
                                    .on_hover_text(if email {
                                        "E-mail importado no Ponte, fora do SIGADOC."
                                    } else {
                                        "Registro criado no Ponte, fora do SIGADOC."
                                    });
                            }
                            let mut line = document
                                .map(|d| format!("{} · {}", d.document_number, d.title))
                                .unwrap_or_else(|| "documento".to_owned());
                            if pending && task.status != "pendente" {
                                line.push_str(&format!(" · {}", crate::tasks::status_label(&task.status)));
                            }
                            if !task.assignee.trim().is_empty() {
                                line.push_str(&format!(" · {}", task.assignee.trim()));
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(line).size(theme::SIZE_SMALL).color(p.muted),
                                )
                                .selectable(false),
                            );
                            if level > 4 {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("· nível {level}"))
                                            .size(theme::SIZE_SMALL)
                                            .color(p.muted),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text("Profundidade na cadeia de dependências; o recuo para em 4 níveis.");
                            }
                            if let Some(count) = env.attachment_counts.get(&task.id) {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{} {count}", icon::CLIP))
                                            .size(theme::SIZE_SMALL)
                                            .color(p.muted),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text(format!("{count} anexo(s) nesta tarefa"));
                            }
                        });
                        if !direct.is_empty() {
                            let (color, text) = if blocked {
                                (
                                    p.warn,
                                    format!("{} de {} anterior(es) em aberto", blockers.len(), direct.len()),
                                )
                            } else if pending {
                                (p.ok, format!("{} anterior(es) concluída(s)", direct.len()))
                            } else {
                                (p.muted, format!("{} anterior(es)", direct.len()))
                            };
                            let detail = direct
                                .iter()
                                .map(|d| format!("{} {}", state_mark(d).0, d.title))
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = S1;
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{} {text}", icon::LINK))
                                            .size(theme::SIZE_SMALL)
                                            .color(color),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text(if blocked {
                                    format!("Só pode ser concluída depois de:\n{detail}")
                                } else {
                                    format!("Depende de:\n{detail}")
                                });
                                widgets::small_muted(ui, "depende de");
                                for dependency in &direct {
                                    if let Some(clicked) = dependency_chip(ui, dependency) {
                                        out.select = Some(clicked);
                                    }
                                }
                            });
                        }
                        if !releases.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = S1;
                                widgets::small_muted(ui, "libera")
                                    .on_hover_text("Tarefas que só podem ser concluídas depois desta.");
                                for next in &releases {
                                    if let Some(clicked) = dependency_chip(ui, next) {
                                        out.select = Some(clicked);
                                    }
                                }
                            });
                        }
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = S2;
                    if !pending {
                        widgets::pill(ui, "Concluída", Tone::Ok);
                    } else if task.due_date.is_empty() {
                        widgets::pill(ui, "Sem prazo", Tone::Neutral);
                    } else if task.overdue() {
                        widgets::pill(ui, format!("Atrasada · {}", short_date(&task.due_date)), Tone::Danger);
                    } else if task.due_date == env.today {
                        widgets::pill(ui, "Hoje", Tone::Warn);
                    } else {
                        widgets::pill(ui, short_date(&task.due_date), Tone::Info);
                    }
                    if task.priority != "normal" && !task.priority.is_empty() {
                        let tone = match task.priority.as_str() {
                            "urgente" => Tone::Danger,
                            "alta" => Tone::Warn,
                            _ => Tone::Neutral,
                        };
                        widgets::pill(ui, crate::labels::priority(&task.priority), tone);
                    }
                });
            });
        });

    let rect = inner.response.rect;
    ui.data_mut(|d| d.insert_temp(id, rect));
    let r = theme::RADIUS_CARD.saturating_sub(1);
    let rounding = egui::CornerRadius {
        nw: if first { r } else { 0 },
        ne: if first { r } else { 0 },
        sw: if last { r } else { 0 },
        se: if last { r } else { 0 },
    };
    let hovered = row.as_ref().is_some_and(|r| r.hovered());
    let fill = if editing_this {
        Some(p.primary_soft)
    } else if hovered {
        Some(p.sunken)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter()
            .set(background, egui::Shape::rect_filled(rect, rounding, fill));
    }
    // Quiet dependency line and the mark of the tasks around the open one.
    if level > 0 {
        let x = rect.left() + S3 + indent - 9.0;
        ui.painter().vline(
            x,
            rect.y_range().shrink(6.0),
            egui::Stroke::new(2.0, p.primary.gamma_multiply(0.5)),
        );
    }
    if let Some((color, hint)) = relation {
        let bar = egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.left() + 3.0, rect.bottom()));
        ui.painter().rect_filled(bar, 0.0, color);
        if let Some(row) = &row {
            row.clone().on_hover_text(hint);
        }
    }
    if !last {
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, p.outline));
    }
    // The click only counts when the area it landed on is where this row
    // still is: after the list changes, last frame's area may belong to
    // another row's place.
    let in_place = cached.is_some_and(|old| (old.min - rect.min).length() < 1.0);
    if let Some(row) = row {
        if in_place && row.clicked() {
            out.select = Some(task.clone());
        }
    }
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
    /// Development only: keeps "Mais detalhes" open for the picture.
    #[cfg(feature = "shots")]
    shots_details: bool,
    /// Development only: scroll of the list, to picture its lower groups.
    #[cfg(feature = "shots")]
    shots_scroll: f32,
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
                self.base_version = store.item_version("task", id).ok().map(|v| v.version);
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
        self.base_version = store.item_version("task", task.id).ok().map(|v| v.version);
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
        self.notice(
            "Revise e salve para criar a tarefa. Nada foi enviado ao SIGADOC.",
            false,
        );
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

        let contexts: std::collections::BTreeMap<_, _> = documents
            .iter()
            .filter_map(|d| store.load_work_context(d.id).ok().map(|c| (d.id, c)))
            .collect();
        let today = chrono::Local::now().date_naive();
        let today_text = today.format("%Y-%m-%d").to_string();
        let week_text = (today + chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();
        let belongs = |task: &&Task| self.document_filter.is_none_or(|id| task.document_id == id);
        let pending_count = tasks
            .iter()
            .filter(belongs)
            .filter(|t| t.status != "concluida")
            .count();
        let overdue_count = tasks.iter().filter(belongs).filter(|t| t.overdue()).count();
        let today_count = tasks
            .iter()
            .filter(belongs)
            .filter(|t| t.status != "concluida" && !t.due_date.is_empty() && t.due_date == today_text)
            .count();
        let week_count = tasks
            .iter()
            .filter(belongs)
            .filter(|t| {
                t.status != "concluida"
                    && !t.due_date.is_empty()
                    && t.due_date >= today_text
                    && t.due_date <= week_text
            })
            .count();
        // "Todas" counts what that view lists: pending tasks, plus the
        // concluded ones when "Mostrar concluídas" is on.
        let all_count = if self.show_done {
            tasks.iter().filter(belongs).count()
        } else {
            pending_count
        };
        let deps = crate::tasks::Dependencies::new(&tasks);
        let visible: Vec<&Task> = tasks
            .iter()
            .filter(|task| {
                if self
                    .document_filter
                    .is_some_and(|id| task.document_id != id)
                {
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

        let mut out = RowOut::default();
        let attachment_counts = store.attachment_counts("task").unwrap_or_default();

        // The task form is the side panel on the right; the rest is the area.
        if self.draft.is_some() {
            egui::Panel::right("task-form")
                .exact_size(FORM_WIDTH)
                .resizable(false)
                .frame(egui::Frame::new().fill(c().surface))
                .show(ui, |ui| {
                    if let Some(id) = self.show_form(ui, store, &tasks, &documents) {
                        open = Some(id);
                    }
                });
        }

        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(S4 as i8, S3 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = S2;
                // Area header: name, search and the one primary action.
                ui.horizontal(|ui| {
                    widgets::heading(ui, "Tarefas");
                    crate::help_ui::help(ui,
                        "Cada documento importado recebe uma tarefa de triagem. As tarefas são locais: concluir aqui não tramita nem assina nada no SIGADOC.",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let new_task = ui
                            .add_enabled(
                                selected_document.is_some(),
                                widgets::primary(format!("{} Nova tarefa", icon::PLUS)),
                            )
                            .on_disabled_hover_text("Abra um documento para criar uma tarefa ligada a ele.");
                        let new_task = match selected_document {
                            Some(id) => new_task.on_hover_text(format!("Cria uma tarefa para {}", number_of(id))),
                            None => new_task,
                        };
                        if new_task.clicked() {
                            if let Some(id) = selected_document {
                                if !self.start_new(id) {
                                    self.blocked = Some(Intent::New(id));
                                }
                            }
                        }
                        widgets::search(ui, &mut self.query, "Buscar tarefa ou documento", 280.0);
                    });
                });
                // Second row: the views as chips, and the checkbox on the right.
                let checkbox_width = 170.0;
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2((ui.available_width() - checkbox_width).max(200.0), 0.0),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(S2, S2);
                            for index in PRIMARY_VIEWS {
                                let label = match index {
                                    0 => format!("{} · {all_count}", VIEWS[index]),
                                    2 => format!("{} · {today_count}", VIEWS[index]),
                                    3 => format!("{} · {week_count}", VIEWS[index]),
                                    _ => format!("{} · {overdue_count}", VIEWS[index]),
                                };
                                if widgets::chip(ui, self.view == index, label).clicked() {
                                    self.view = index;
                                }
                            }
                            let in_more = SECONDARY_VIEWS.contains(&self.view);
                            let more_label = if in_more {
                                format!("{} {}", VIEWS[self.view], icon::DOWN)
                            } else {
                                format!("Mais {}", icon::DOWN)
                            };
                            let more = widgets::chip(ui, in_more, more_label);
                            egui::Popup::menu(&more).show(|ui| {
                                for index in SECONDARY_VIEWS {
                                    if ui.selectable_label(self.view == index, VIEWS[index]).clicked() {
                                        self.view = index;
                                    }
                                }
                            });
                            // Which documents the list covers.
                            if let Some(id) = self.document_filter {
                                widgets::chip(ui, true, format!("Só {}", number_of(id)))
                                    .on_hover_text("A lista segue o documento aberto na biblioteca.");
                                if widgets::chip(ui, false, "Todos os documentos").clicked() {
                                    self.document_filter = None;
                                    self.show_all_documents = true;
                                }
                            } else {
                                if let Some(id) = selected_document {
                                    if widgets::chip(ui, false, format!("Só {}", number_of(id)))
                                        .on_hover_text("Volta a mostrar apenas as tarefas do documento aberto.")
                                        .clicked()
                                    {
                                        self.document_filter = Some(id);
                                        self.show_all_documents = false;
                                    }
                                }
                                widgets::chip(ui, true, "Todos os documentos");
                            }
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.add_space(S1);
                        ui.checkbox(&mut self.show_done, "Mostrar concluídas");
                    });
                });
                if !self.message.is_empty() {
                    let (fg, bg) = if self.message_is_error { Tone::Danger } else { Tone::Ok }.colors();
                    let text = self.message.clone();
                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                        .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let width = (ui.available_width() - 40.0).max(100.0);
                                ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                                    ui.set_width(width);
                                    ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                                });
                                if widgets::icon_button(ui, icon::CLOSE, "Fechar").clicked() {
                                    self.message.clear();
                                }
                            });
                        });
                }

                // The list: tasks grouped by deadline, one card per group.
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
                let env = ListEnv {
                    tasks: &tasks,
                    documents: &documents,
                    deps: &deps,
                    attachment_counts: &attachment_counts,
                    focus_before: &focus_before,
                    focus_after: &focus_after,
                    today: &today_text,
                    editing: self.draft.as_ref().map(|t| t.id),
                    editing_dirty: self.dirty(),
                };
                let mut groups: [Vec<&Task>; 5] = Default::default();
                for task in &visible {
                    let slot = if task.status == "concluida" {
                        4
                    } else if task.overdue() {
                        0
                    } else if task.due_date.is_empty() {
                        3
                    } else if task.due_date == today_text {
                        1
                    } else {
                        2
                    };
                    groups[slot].push(task);
                }
                let titles_of_groups = [
                    ("Atrasadas", Some(c().danger)),
                    ("Para hoje", None),
                    ("Próximas", None),
                    ("Sem prazo", None),
                    ("Concluídas", None),
                ];
                // A view that already is one deadline group needs no overline.
                let single_group = matches!(self.view, 2 | 4 | 7);
                #[allow(unused_mut)]
                let mut list_scroll = egui::ScrollArea::vertical()
                    .id_salt("task-manager")
                    .auto_shrink([false, false]);
                #[cfg(feature = "shots")]
                if crate::shots::active() {
                    list_scroll = list_scroll.vertical_scroll_offset(self.shots_scroll);
                }
                list_scroll.show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if visible.is_empty() {
                            ui.add_space(S2);
                            widgets::muted(ui, "Nenhuma tarefa nesta visão.");
                        }
                        for (index, group) in groups.iter().enumerate() {
                            if group.is_empty() {
                                continue;
                            }
                            if !single_group {
                                ui.add_space(S3);
                                let (name, color) = titles_of_groups[index];
                                widgets::overline(ui, name, color);
                                ui.add_space(S1);
                            } else {
                                ui.add_space(S2);
                            }
                            widgets::card_frame()
                                .inner_margin(egui::Margin::ZERO)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for (position, task) in group.iter().enumerate() {
                                        // A task hangs from the one it waits
                                        // for only when that one is above it
                                        // in this same group.
                                        let nested = group[..position]
                                            .iter()
                                            .any(|above| task.dependencies.contains(&above.id));
                                        // Controls keep the task's own id, so a
                                        // click never lands on a neighbour when
                                        // the list changes under the pointer.
                                        ui.push_id(("task", task.id), |ui| {
                                            task_row(
                                                ui,
                                                &env,
                                                task,
                                                position == 0,
                                                position + 1 == group.len(),
                                                nested,
                                                &mut out,
                                            );
                                        });
                                    }
                                });
                        }
                        ui.add_space(S4);
                    });
            });

        if let Some(task) = out.select.or_else(|| self.open_task_requested.take()) {
            if !self.open_task(&task, store) {
                self.blocked = Some(Intent::Open(task));
            }
        }
        if let Some(id) = out.complete {
            self.apply_status_change(store, id, true);
        }
        if let Some(id) = out.reopen {
            self.apply_status_change(store, id, false);
        }
        open
    }

    /// Closes the form of a task nobody has changed.
    fn close_form(&mut self) {
        self.draft = None;
        self.original = None;
        self.base_version = None;
        self.message.clear();
        self.attachments.forget();
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
                self.base_version = store.item_version("task", task.id).ok().map(|v| v.version);
            }
        }
        let current_actor = store.active_actor();
        let deps = crate::tasks::Dependencies::new(tasks);
        let mut save = false;
        let mut cancel = false;
        let mut conclude = false;
        let mut delete = false;
        let mut close = false;
        let mut open_task: Option<Task> = None;
        let dirty = self.dirty();
        let (task_id, task_status, document_id) = {
            let task = self.draft.as_ref()?;
            (task.id, task.status.clone(), task.document_id)
        };
        let panel_margin = egui::Margin::symmetric(S4 as i8, S3 as i8);

        // Header: name of the panel, the unsaved mark and the close button.
        egui::Frame::new().inner_margin(panel_margin).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if task_id > 0 { "Tarefa" } else { "Nova tarefa" })
                        .font(theme::font_strong(17.0)),
                );
                if dirty {
                    widgets::pill(ui, "Não salva", Tone::Warn);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = ui.add_enabled_ui(!dirty && !self.attachments.dirty(), |ui| {
                        widgets::icon_button(ui, icon::CLOSE, "Fechar o painel")
                    });
                    if response.inner.clicked() {
                        close = true;
                    }
                    response
                        .response
                        .on_disabled_hover_text("Salve ou cancele as alterações antes de fechar.");
                });
            });
            widgets::small_muted(
                ui,
                format!(
                    "Documento: {}",
                    documents
                        .iter()
                        .find(|d| d.id == document_id)
                        .map(|d| d.document_number.as_str())
                        .unwrap_or("não disponível")
                ),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(widgets::quiet(format!("{} Consultar documento", icon::EYE)))
                    .on_hover_text("Mostra o texto e os PDFs do documento numa janela por cima desta ficha, para copiar um prazo ou conferir um trecho sem sair da tarefa.")
                    .clicked()
                {
                    self.consult_requested = Some(document_id);
                }
                if ui
                    .add(widgets::quiet(format!("{} Ir para o documento", icon::DOCUMENT)))
                    .on_hover_text("Abre a ficha completa do documento. Se a tarefa tiver alterações não salvas, o Ponte pergunta antes o que fazer com elas.")
                    .clicked()
                {
                    open = Some(document_id);
                }
            });
        });
        ui.separator();

        // Footer, fixed at the bottom: Salvar, Concluir, Cancelar; Excluir apart.
        egui::Panel::bottom("task-form-foot")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(c().surface)
                    .inner_margin(panel_margin),
            )
            .show(ui, |ui| {
                let top = ui.max_rect();
                ui.painter().hline(
                    (top.left() - S4)..=(top.right() + S4),
                    top.top() - S3,
                    egui::Stroke::new(1.0, c().outline),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = S2;
                    save = ui
                        .add_enabled(dirty || task_id == 0, widgets::primary("Salvar"))
                        .clicked();
                    if task_id > 0 && task_status != "concluida" {
                        conclude = ui
                            .add_enabled(!dirty, widgets::secondary("Concluir"))
                            .on_disabled_hover_text("Salve ou cancele as alterações antes de concluir.")
                            .clicked();
                    }
                    cancel = ui
                        .add_enabled(dirty || task_id == 0, widgets::quiet("Cancelar"))
                        .clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        delete = ui
                            .add_enabled(task_id > 0 && !dirty, widgets::danger("Excluir…"))
                            .on_disabled_hover_text(
                                "Salve ou cancele as alterações antes de excluir esta tarefa.",
                            )
                            .clicked();
                    });
                });
            });

        let Some(task) = &mut self.draft else {
            return None;
        };
        #[cfg(feature = "shots")]
        let force_details = self.shots_details;
        #[allow(unused_mut)]
        let mut form_scroll = egui::ScrollArea::vertical()
            .id_salt("task-form")
            .auto_shrink([false, false]);
        #[cfg(feature = "shots")]
        if crate::shots::active() {
            form_scroll = form_scroll.vertical_scroll_offset(if force_details { 330.0 } else { 0.0 });
        }
        form_scroll.show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(S4 as i8, S3 as i8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = S1;
                        widgets::field_label(ui, "Título");
                        ui.add(egui::TextEdit::singleline(&mut task.title).desired_width(f32::INFINITY));
                        ui.add_space(S2);
                        two_columns(
                            ui,
                            0.62,
                            |ui| {
                                ui.horizontal(|ui| {
                                    widgets::field_label(ui, "Prazo");
                                    crate::help_ui::help(
                                        ui,
                                        "Deixe vazio se não houver prazo confirmado. Anote a origem em “Mais detalhes”.",
                                    );
                                });
                                let response =
                                    crate::labels::date_field(ui, ("task-due", task_id), &mut task.due_date);
                                if !task.due_date.trim().is_empty()
                                    && crate::labels::parse_typed_date(&task.due_date).is_none()
                                {
                                    widgets::outline(ui, response.rect.expand(2.0), c().danger);
                                }
                            },
                            |ui| {
                                widgets::field_label(ui, "Prioridade");
                                egui::ComboBox::from_id_salt("Prioridade")
                                    .selected_text(crate::labels::priority(&task.priority).to_owned())
                                    .width(ui.available_width() - 24.0)
                                    .show_ui(ui, |ui| {
                                        for choice in ["baixa", "normal", "alta", "urgente"] {
                                            ui.selectable_value(
                                                &mut task.priority,
                                                choice.to_string(),
                                                crate::labels::priority(choice),
                                            );
                                        }
                                    });
                            },
                        );
                        ui.add_space(S2);
                        two_columns(
                            ui,
                            0.55,
                            |ui| {
                                widgets::field_label(ui, "Responsável");
                                ui.horizontal(|ui| {
                                    let width = (ui.available_width() - 52.0).max(60.0);
                                    ui.add(egui::TextEdit::singleline(&mut task.assignee).desired_width(width));
                                    if ui
                                        .add(widgets::secondary("Eu"))
                                        .on_hover_text(format!("Atribuir a {current_actor}"))
                                        .clicked()
                                    {
                                        task.assignee = current_actor.clone();
                                    }
                                });
                            },
                            |ui| {
                                widgets::field_label(ui, "Situação");
                                egui::ComboBox::from_id_salt("Situação")
                                    .selected_text(crate::tasks::status_label(&task.status).to_owned())
                                    .width(ui.available_width() - 24.0)
                                    .show_ui(ui, |ui| {
                                        for choice in ["pendente", "em analise", "aguardando", "concluida"] {
                                            ui.selectable_value(
                                                &mut task.status,
                                                choice.to_string(),
                                                crate::tasks::status_label(choice),
                                            );
                                        }
                                    });
                            },
                        );
                        ui.add_space(S2);
                        widgets::field_label(ui, "Ação");
                        egui::ComboBox::from_id_salt("Ação")
                            .selected_text(crate::tasks::action_label(&task.action).to_owned())
                            .width(ui.available_width() - 24.0)
                            .show_ui(ui, |ui| {
                                for choice in crate::tasks::ACTIONS {
                                    ui.selectable_value(
                                        &mut task.action,
                                        choice.to_string(),
                                        crate::tasks::action_label(choice),
                                    );
                                }
                            });
                        ui.add_space(S2);
                        widgets::field_label(ui, "Observações");
                        ui.add(
                            egui::TextEdit::multiline(&mut task.notes)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        if let Some(at) = &task.completed_at {
                            widgets::small_muted(ui, format!("Concluída em {}", crate::labels::datetime(at)));
                        }
                        ui.add_space(S2);
                        ui.separator();
                        ui.add_space(S1);
                        #[allow(unused_mut)]
                        let mut more = egui::CollapsingHeader::new(
                            RichText::new("Mais detalhes").font(theme::font_strong(theme::SIZE_BODY)),
                        )
                        .id_salt(("task-more", task_id));
                        #[cfg(feature = "shots")]
                        if force_details {
                            more = more.open(Some(true));
                        }
                        more.show(ui, |ui| {
                            widgets::small_muted(
                                ui,
                                "Lembrete, dependências, lista de providências e anexos.",
                            );
                            ui.add_space(S1);
                            widgets::field_label(ui, "Origem do prazo (trecho, arquivo ou orientação)");
                            ui.add(
                                egui::TextEdit::singleline(&mut task.due_source)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(S2);
                            ui.horizontal(|ui| {
                                widgets::field_label(ui, "Lembrar a partir de");
                                crate::help_ui::help(ui, "Lembrete local, mostrado na visão “Lembretes” enquanto o Ponte estiver aberto.");
                            });
                            crate::labels::date_field(ui, ("task-reminder", task_id), &mut task.reminder_date);
                            ui.add_space(S2);
                            widgets::field_label(ui, "Resultado (número da resposta, data de envio, observações)");
                            ui.add(
                                egui::TextEdit::multiline(&mut task.outcome)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(S2);
                            ui.collapsing("Checklist de providências", |ui| {
                                let mut remove = None;
                                for (index, (done, text)) in task.checklist.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(done, "");
                                        ui.add(
                                            egui::TextEdit::singleline(text)
                                                .desired_width((ui.available_width() - 40.0).max(60.0)),
                                        );
                                        if widgets::icon_button(ui, icon::CLOSE, "Remover etapa").clicked() {
                                            remove = Some(index);
                                        }
                                    });
                                }
                                if let Some(index) = remove {
                                    task.checklist.remove(index);
                                }
                                if task.checklist.len() < 100
                                    && ui.add(widgets::secondary(format!("{} Adicionar etapa", icon::PLUS))).clicked()
                                {
                                    task.checklist.push((false, String::new()));
                                }
                            });
                            if let Ok(doc) = store.load_document(document_id) {
                                if let Some(analysis) = doc.analysis {
                                    ui.horizontal_wrapped(|ui| {
                                        widgets::small_muted(
                                            ui,
                                            format!(
                                                "{} Sugestão da IA: prioridade {} • prazo mencionado: {}{}",
                                                icon::AI,
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
                                            ),
                                        );
                                        if !doc.summary.analysis_stale
                                            && ui.add(widgets::quiet("Usar prioridade sugerida")).clicked()
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
                                            .add(widgets::quiet(format!(
                                                "Usar a finalidade da ficha: {}",
                                                crate::tasks::action_label(action)
                                            )))
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
                                            .add(widgets::quiet(format!(
                                                "Usar a prioridade da ficha: {}",
                                                crate::labels::priority(&context.priority)
                                            )))
                                            .clicked()
                                    {
                                        task.priority = context.priority;
                                    }
                                });
                            }
                            ui.collapsing("Depende de outras tarefas", |ui| {
                                widgets::small_muted(ui, "Uma tarefa só pode ser concluída depois das que ela depende. Tarefas que já dependem desta ficam desabilitadas: marcá-las formaria um ciclo.");
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
                                            RichText::new(label).color(c().muted)
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
                                            .add(widgets::secondary(format!("Remover dependência #{missing} (tarefa não disponível)")))
                                            .clicked()
                                    {
                                        task.dependencies.retain(|id| *id != missing);
                                    }
                                }
                            });
                            if task_id > 0 {
                                let before = deps.ancestors(task_id);
                                let after = deps.descendants(task_id);
                                if !before.is_empty() || !after.is_empty() {
                                    ui.add_space(S2);
                                    ui.horizontal_wrapped(|ui| {
                                        widgets::field_label(ui, "Cadeia de dependências");
                                        crate::help_ui::help(ui,
                                            "Tudo o que vem antes desta tarefa (em qualquer nível) e tudo o que espera por ela. Para mudar as dependências diretas, use “Mais detalhes ▸ Depende de outras tarefas”.",
                                        );
                                    });
                                    if !before.is_empty() {
                                        widgets::small_muted(ui, "Antes desta (precisam ser concluídas primeiro):");
                                        for previous in &before {
                                            chain_row(ui, previous, &mut open_task);
                                        }
                                    }
                                    if !after.is_empty() {
                                        widgets::small_muted(ui, "Depois desta (esperam por ela):");
                                        for next in &after {
                                            chain_row(ui, next, &mut open_task);
                                        }
                                    }
                                }
                            }
                            ui.add_space(S2);
                            widgets::field_label(ui, "Anexos");
                            if task_id > 0 {
                                self.attachments
                                    .show(ui, store, crate::attachments::Owner::Task(task_id), false);
                            } else {
                                widgets::small_muted(ui, "Salve a tarefa para poder anexar arquivos a ela.");
                            }
                        });
                    });
            });
        if open_task.is_some() {
            self.open_task_requested = open_task;
        }
        if delete {
            self.trash_requested = Some(crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id: task_id,
            });
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
        if close && !self.dirty() && !self.attachments.dirty() {
            self.close_form();
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
                    self.base_version = store.item_version("task", id).ok().map(|v| v.version);
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
        assert_eq!(
            panel.editing_title().as_deref(),
            None,
            "a blank new task has nothing to lose"
        );
        panel.draft.as_mut().unwrap().title = "Responder ao ofício".into();
        assert_eq!(
            panel.editing_title().as_deref(),
            Some("Responder ao ofício")
        );

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
        assert!(
            panel.take_blocked().is_none(),
            "start_new itself does not queue; the panel's button does"
        );
        panel.blocked = Some(Intent::New(document));
        assert_eq!(panel.take_blocked(), Some(Intent::New(document)));
        assert!(panel.take_blocked().is_none());
    }
}

/// Development only: the views of this screen pictured by `--shots`, by
/// name. The tour calls `shots_open` with each one on a fresh panel.
#[cfg(feature = "shots")]
pub const SHOTS: &[&str] = &["lista", "aberta", "nova", "concluidas", "detalhes"];

#[cfg(feature = "shots")]
impl TaskPanel {
    /// Puts the panel in the state called `view` (see `SHOTS`).
    pub fn shots_open(&mut self, store: &mut crate::storage::Store, view: &str) {
        let tasks = store.list_tasks().unwrap_or_default();
        self.show_all_documents = true;
        self.document_filter = None;
        match view {
            "aberta" => {
                if let Some(task) = tasks.iter().find(|t| t.overdue()) {
                    self.open_task(task, store);
                }
            }
            "nova" => {
                let target = store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| d.document_number == "SESP-OFI-2026/00431");
                if let Some(document) = target {
                    self.start_new(document.id);
                    self.show_all_documents = true;
                    self.document_filter = None;
                }
            }
            "concluidas" => {
                self.show_done = true;
                self.shots_scroll = 520.0;
            }
            "detalhes" => {
                let task = tasks
                    .iter()
                    .find(|t| !t.dependencies.is_empty())
                    .or_else(|| tasks.iter().find(|t| t.overdue()));
                if let Some(task) = task {
                    self.open_task(task, store);
                }
                self.shots_details = true;
            }
            _ => {}
        }
    }
}
