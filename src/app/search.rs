//! Search across the whole library — documents, tasks and atendimentos — in
//! one box (Ctrl+K), over whatever area is on screen.
use super::{Leave, PonteApp};
use crate::saiop::SaiopAttendance;
use crate::tasks::Task;
use crate::theme::{self, S1, S2, S3};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};
use std::time::{Duration, Instant};

/// Rows shown per group; the rest is summed up in an "e mais N" line.
const GROUP_LIMIT: usize = 8;
/// Fewer characters than this do not search.
const MIN_CHARS: usize = 2;
/// Pause after the last keystroke before searching.
const DEBOUNCE: Duration = Duration::from_millis(200);
const PANEL_WIDTH: f32 = 640.0;

/// A document found.
struct DocumentRow {
    id: i64,
    number: String,
    title: String,
    /// "Manual" or "E-mail" for what was made in the Ponte.
    kind: Option<&'static str>,
}

/// A task found.
struct TaskRow {
    id: i64,
    title: String,
    /// "number · title" of the task's document, when known.
    document: String,
    /// The task belongs to no document: "Sem documento" is shown instead.
    standalone: bool,
    due_date: String,
    done: bool,
}

/// An atendimento found.
struct AttendanceRow {
    id: i64,
    reference: String,
    subject: String,
    status: String,
}

/// At most `GROUP_LIMIT` rows of a group and how many were left out.
struct Group<T> {
    rows: Vec<T>,
    more: usize,
}

impl<T> Default for Group<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            more: 0,
        }
    }
}

impl<T> Group<T> {
    fn limited(mut all: Vec<T>) -> Self {
        let more = all.len().saturating_sub(GROUP_LIMIT);
        all.truncate(GROUP_LIMIT);
        Self { rows: all, more }
    }
}

#[derive(Default)]
struct Results {
    documents: Group<DocumentRow>,
    tasks: Group<TaskRow>,
    attendances: Group<AttendanceRow>,
    /// Why the library could not be read, if it could not.
    error: Option<String>,
}

impl Results {
    fn total(&self) -> usize {
        self.documents.rows.len() + self.tasks.rows.len() + self.attendances.rows.len()
    }
}

/// What the person chose.
enum Pick {
    Document(i64),
    Task(i64),
    Attendance(i64),
}

/// What the search box holds between frames.
#[derive(Default)]
pub(super) struct GlobalSearch {
    /// The box is on screen.
    pub open: bool,
    query: String,
    /// The text the shown results were searched for.
    searched: String,
    /// When the text last changed and has not been searched yet.
    edited_at: Option<Instant>,
    results: Results,
    highlight: Option<usize>,
    /// Put the keyboard in the field (set when the box opens).
    focus_field: bool,
    was_open: bool,
    /// Search at once, without waiting (the pictures of the tour).
    immediate: bool,
}

#[cfg(feature = "shots")]
impl GlobalSearch {
    /// Opens the box with this text already searched, for the tour.
    // Used by the tour steps, which are not in this file.
    pub(super) fn shots_open(&mut self, query: &str) {
        self.query = query.to_owned();
        self.open = true;
        self.immediate = true;
        self.focus_field = false;
    }
}

/// Lowercase without accents, for comparing text.
fn fold(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Every word of the (already folded) `needle` appears in some of the fields.
fn fields_match(needle: &str, fields: &[&str]) -> bool {
    let haystack = fold(&fields.join("\n"));
    needle
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

fn task_matches(task: &Task, needle: &str) -> bool {
    fields_match(needle, &[&task.title, &task.notes, &task.assignee])
}

fn attendance_matches(attendance: &SaiopAttendance, needle: &str) -> bool {
    fields_match(
        needle,
        &[
            &attendance.reference_raw,
            &attendance.subject,
            &attendance.observation,
            &attendance.assignee,
        ],
    )
}

fn task_done(task: &Task) -> bool {
    task.completed_at.is_some() || task.status == "concluida"
}

/// The tasks that match, open ones first (each part keeps its order).
fn matching_tasks(tasks: Vec<Task>, needle: &str) -> Vec<Task> {
    let (done, open): (Vec<Task>, Vec<Task>) = tasks
        .into_iter()
        .filter(|t| task_matches(t, needle))
        .partition(task_done);
    open.into_iter().chain(done).collect()
}

/// The next highlighted row when moving by `delta` over `total` rows,
/// wrapping at both ends.
fn move_highlight(current: Option<usize>, total: usize, delta: i32) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let next = match current {
        None if delta >= 0 => 0,
        None => total - 1,
        Some(index) => (index as i64 + delta as i64).rem_euclid(total as i64) as usize,
    };
    Some(next)
}

/// "07/11" from "2026-11-07".
fn day_month(iso: &str) -> String {
    match (iso.get(8..10), iso.get(5..7)) {
        (Some(day), Some(month)) => format!("{day}/{month}"),
        _ => iso.to_owned(),
    }
}

impl GlobalSearch {
    /// Runs the search for the text in the field.
    fn run(
        &mut self,
        store: &crate::storage::Store,
        index: &std::collections::BTreeMap<i64, (String, String)>,
    ) {
        self.edited_at = None;
        self.immediate = false;
        self.searched = self.query.clone();
        self.highlight = None;
        let text = self.query.trim().to_owned();
        if text.chars().count() < MIN_CHARS {
            self.results = Results::default();
            return;
        }
        let needle = fold(&text);
        let mut results = Results::default();
        let mut errors = Vec::new();
        match store.list_documents(&text) {
            Ok(documents) => {
                results.documents = Group::limited(
                    documents
                        .into_iter()
                        .map(|d| DocumentRow {
                            id: d.id,
                            number: d.document_number,
                            title: d.title,
                            kind: if d.email {
                                Some("E-mail")
                            } else if d.manual {
                                Some("Manual")
                            } else {
                                None
                            },
                        })
                        .collect(),
                );
            }
            Err(error) => errors.push(error.to_string()),
        }
        match store.list_tasks() {
            Ok(tasks) => {
                results.tasks = Group::limited(
                    matching_tasks(tasks, &needle)
                        .into_iter()
                        .map(|t| TaskRow {
                            id: t.id,
                            document: t
                                .document_id
                                .and_then(|id| index.get(&id))
                                .map(|(number, _)| number.clone())
                                .unwrap_or_default(),
                            standalone: t.document_id.is_none(),
                            done: task_done(&t),
                            due_date: t.due_date,
                            title: t.title,
                        })
                        .collect(),
                );
            }
            Err(error) => errors.push(error.to_string()),
        }
        match store.list_saiop_attendances() {
            Ok(list) => {
                results.attendances = Group::limited(
                    list.into_iter()
                        .filter(|a| attendance_matches(a, &needle))
                        .map(|a| AttendanceRow {
                            id: a.id,
                            status: crate::saiop::status_label(&a.status).to_owned(),
                            reference: a.reference_raw,
                            subject: a.subject,
                        })
                        .collect(),
                );
            }
            Err(error) => errors.push(error.to_string()),
        }
        if !errors.is_empty() {
            results.error = Some(errors.join("; "));
        }
        self.highlight = if results.total() > 0 { Some(0) } else { None };
        self.results = results;
    }

    /// The row at this position counting across the groups.
    fn pick_at(&self, index: usize) -> Option<Pick> {
        let r = &self.results;
        let documents = r.documents.rows.len();
        let tasks = r.tasks.rows.len();
        if index < documents {
            Some(Pick::Document(r.documents.rows[index].id))
        } else if index < documents + tasks {
            Some(Pick::Task(r.tasks.rows[index - documents].id))
        } else {
            r.attendances
                .rows
                .get(index - documents - tasks)
                .map(|a| Pick::Attendance(a.id))
        }
    }
}

/// One row; returns whether it was clicked.
fn result_row(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    highlighted: bool,
    scroll: bool,
    muted: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
    let p = theme::c();
    let response = ui
        .scope_builder(
            egui::UiBuilder::new()
                .id_salt(id)
                .sense(egui::Sense::click()),
            |ui| {
                let hovered = ui.response().hovered();
                egui::Frame::new()
                    .fill(if highlighted || hovered {
                        p.primary_soft
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::symmetric(S2 as i8, 7))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if muted {
                            ui.multiply_opacity(0.6);
                        }
                        add(ui);
                    });
            },
        )
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if highlighted && scroll {
        response.scroll_to_me(None);
    }
    response.clicked()
}

fn more_line(ui: &mut egui::Ui, more: usize) {
    if more > 0 {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(S2);
            widgets::small_muted(ui, format!("e mais {more}"));
        });
    }
}

impl PonteApp {
    /// Draws the search box when it is open. Called on every frame.
    pub(super) fn global_search(&mut self, ctx: &egui::Context) {
        if !self.global_search.open {
            if self.global_search.was_open {
                // Closed: keep the text, forget the highlighted row.
                self.global_search.was_open = false;
                self.global_search.highlight = None;
            }
            return;
        }
        if !self.global_search.was_open {
            self.global_search.was_open = true;
            self.global_search.focus_field = !self.global_search.immediate;
            if self.global_search.highlight.is_none() && self.global_search.results.total() > 0 {
                self.global_search.highlight = Some(0);
            }
        }
        // The text may have changed: search after a short pause.
        let search = &mut self.global_search;
        if search.query != search.searched && search.edited_at.is_none() {
            search.edited_at = Some(Instant::now());
        }
        if search.query == search.searched {
            search.edited_at = None;
        }
        let due = match search.edited_at {
            Some(at) => at.elapsed() >= DEBOUNCE,
            None => false,
        };
        if search.immediate || due {
            search.run(&self.store, &self.today.index);
        } else if let Some(at) = search.edited_at {
            ctx.request_repaint_after(DEBOUNCE.saturating_sub(at.elapsed()));
        }

        // Keys first, so the text field does not take them.
        let (up, down, enter, escape) = ctx.input_mut(|input| {
            use egui::{Key, Modifiers};
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        // Enter never opens a row of an older text: it searches first.
        let enter = if enter && self.global_search.query != self.global_search.searched {
            self.global_search.run(&self.store, &self.today.index);
            false
        } else {
            enter
        };
        let total = self.global_search.results.total();
        let mut moved = false;
        if up || down {
            let delta = if down { 1 } else { -1 };
            self.global_search.highlight =
                move_highlight(self.global_search.highlight, total, delta);
            moved = true;
        }
        let mut picked: Option<Pick> = None;
        if enter {
            if let Some(index) = self.global_search.highlight {
                picked = self.global_search.pick_at(index);
            }
        }
        let mut close = escape;

        // Dim backdrop: a click on it closes the box.
        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::unique("global-search-backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(70));
                response
            });
        if backdrop.inner.clicked() {
            close = true;
        }

        let p = theme::c();
        let x = (screen.center().x - PANEL_WIDTH / 2.0).max(screen.left() + 8.0);
        let mut clicked: Option<Pick> = None;
        egui::Area::new(egui::Id::unique("global-search-panel"))
            .order(egui::Order::Tooltip)
            .fixed_pos(egui::pos2(x, screen.top() + 72.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(egui::Stroke::new(1.0, p.outline))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CARD))
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 28,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(60),
                    })
                    .inner_margin(egui::Margin::same(S3 as i8))
                    .show(ui, |ui| {
                        let inner = (PANEL_WIDTH.min(screen.width() - 16.0)) - 2.0 * S3;
                        ui.set_width(inner);
                        let search = &mut self.global_search;
                        let response = ui
                            .scope(|ui| {
                                ui.spacing_mut().text_edit_width = inner;
                                widgets::search(
                                    ui,
                                    &mut search.query,
                                    "Buscar no acervo, nas tarefas e nos atendimentos",
                                    inner,
                                )
                            })
                            .inner;
                        if search.focus_field {
                            search.focus_field = false;
                            response.request_focus();
                            let len = search.query.chars().count();
                            let mut state =
                                egui::text_edit::TextEditState::load(ui.ctx(), response.id)
                                    .unwrap_or_default();
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(len),
                                )));
                            state.store(ui.ctx(), response.id);
                        }
                        ui.add_space(S2);
                        let trimmed = search.query.trim();
                        if trimmed.chars().count() < MIN_CHARS {
                            widgets::muted(
                                ui,
                                "Busque por número, assunto, texto, tarefa ou atendimento.",
                            );
                        } else if search.searched != search.query && search.results.total() == 0 {
                            // Still waiting for the pause: say nothing yet.
                        } else {
                            let query = search.searched.trim().to_owned();
                            clicked = draw_results(ui, search, &query, moved);
                        }
                    });
            });
        let picked = picked.or(clicked);
        if let Some(pick) = picked {
            self.global_search.open = false;
            self.global_search.was_open = false;
            self.global_search.highlight = None;
            self.open_pick(pick);
        } else if close {
            self.global_search.open = false;
        }
    }

    /// Goes to the chosen row through the app's own navigation.
    fn open_pick(&mut self, pick: Pick) {
        match pick {
            Pick::Document(id) => self.navigate(Leave::Select(id)),
            Pick::Task(id) => {
                let current = self
                    .store
                    .list_tasks()
                    .ok()
                    .and_then(|tasks| tasks.into_iter().find(|t| t.id == id));
                match current {
                    Some(task) => self.navigate(Leave::OpenTask(task)),
                    None => {
                        self.refresh();
                        self.message("Esta tarefa não está mais disponível.");
                    }
                }
            }
            Pick::Attendance(id) => {
                if self.saiop.focus_attendance(&self.store, id) {
                    self.navigate(Leave::Saiop);
                } else if self.saiop.dirty() {
                    self.error("Há um atendimento com alterações não salvas em Atendimentos. Salve ou descarte antes de abrir outro.");
                } else {
                    self.refresh();
                    self.message("Este atendimento não está mais disponível.");
                }
            }
        }
    }
}

/// The groups of results in a scroll area; returns the row clicked, if any.
fn draw_results(
    ui: &mut egui::Ui,
    search: &GlobalSearch,
    query: &str,
    scroll_to_highlight: bool,
) -> Option<Pick> {
    let p = theme::c();
    let results = &search.results;
    if let Some(error) = &results.error {
        let (fg, bg) = Tone::Danger.colors();
        egui::Frame::new()
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.label(RichText::new(format!("Não foi possível buscar: {error}")).color(fg));
            });
        ui.add_space(S2);
    }
    if results.total() == 0 {
        if results.error.is_none() {
            widgets::muted(ui, format!("Nada encontrado para \"{query}\"."));
        }
        return None;
    }
    let today = chrono::Local::now().date_naive().to_string();
    let mut clicked: Option<Pick> = None;
    let mut index = 0usize;
    egui::ScrollArea::vertical()
        .id_salt("global-search-results")
        .max_height(420.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut first = true;
            let mut heading = |ui: &mut egui::Ui, text: &str| {
                if !first {
                    ui.add_space(S2);
                }
                first = false;
                widgets::overline(ui, text, None);
                ui.add_space(S1);
            };
            if !results.documents.rows.is_empty() {
                heading(ui, "Documentos");
                for document in &results.documents.rows {
                    let on = search.highlight == Some(index);
                    if result_row(
                        ui,
                        ("gs-doc", document.id),
                        on,
                        scroll_to_highlight,
                        false,
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = S2;
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&document.number)
                                                .font(theme::font_strong(theme::SIZE_SMALL))
                                                .color(p.primary),
                                        )
                                        .selectable(false)
                                        .truncate(),
                                    );
                                    ui.add(
                                        egui::Label::new(RichText::new(&document.title))
                                            .selectable(false)
                                            .truncate(),
                                    );
                                });
                                if let Some(kind) = document.kind {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Min),
                                        |ui| {
                                            widgets::pill(ui, kind, Tone::Neutral);
                                        },
                                    );
                                }
                            });
                        },
                    ) {
                        clicked = Some(Pick::Document(document.id));
                    }
                    index += 1;
                }
                more_line(ui, results.documents.more);
            }
            if !results.tasks.rows.is_empty() {
                heading(ui, "Tarefas");
                for task in &results.tasks.rows {
                    let on = search.highlight == Some(index);
                    if result_row(
                        ui,
                        ("gs-task", task.id),
                        on,
                        scroll_to_highlight,
                        task.done,
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = S2;
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&task.title)
                                                .font(theme::font_strong(theme::SIZE_BODY)),
                                        )
                                        .selectable(false)
                                        .truncate(),
                                    );
                                    if !task.document.is_empty() || task.standalone {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(if task.standalone {
                                                    "Sem documento"
                                                } else {
                                                    task.document.as_str()
                                                })
                                                .size(theme::SIZE_SMALL)
                                                .color(p.muted),
                                            )
                                            .selectable(false)
                                            .truncate(),
                                        );
                                    }
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Min),
                                    |ui| {
                                        let (text, tone) = if task.done {
                                            ("Concluída".to_owned(), Tone::Ok)
                                        } else if task.due_date.is_empty() {
                                            ("Sem prazo".to_owned(), Tone::Neutral)
                                        } else if task.due_date.as_str() < today.as_str() {
                                            (
                                                format!("Atrasada · {}", day_month(&task.due_date)),
                                                Tone::Danger,
                                            )
                                        } else if task.due_date == today {
                                            ("Hoje".to_owned(), Tone::Warn)
                                        } else {
                                            (day_month(&task.due_date), Tone::Info)
                                        };
                                        widgets::pill(ui, text, tone);
                                    },
                                );
                            });
                        },
                    ) {
                        clicked = Some(Pick::Task(task.id));
                    }
                    index += 1;
                }
                more_line(ui, results.tasks.more);
            }
            if !results.attendances.rows.is_empty() {
                heading(ui, "Atendimentos");
                for attendance in &results.attendances.rows {
                    let on = search.highlight == Some(index);
                    if result_row(
                        ui,
                        ("gs-attendance", attendance.id),
                        on,
                        scroll_to_highlight,
                        false,
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = S2;
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    let reference = if attendance.reference.is_empty() {
                                        "Sem referência"
                                    } else {
                                        attendance.reference.as_str()
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(reference)
                                                .font(theme::font_strong(theme::SIZE_BODY)),
                                        )
                                        .selectable(false)
                                        .truncate(),
                                    );
                                    if !attendance.subject.is_empty() {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&attendance.subject)
                                                    .size(theme::SIZE_SMALL)
                                                    .color(p.muted),
                                            )
                                            .selectable(false)
                                            .truncate(),
                                        );
                                    }
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Min),
                                    |ui| {
                                        widgets::pill(ui, attendance.status.clone(), Tone::Info);
                                    },
                                );
                            });
                        },
                    ) {
                        clicked = Some(Pick::Attendance(attendance.id));
                    }
                    index += 1;
                }
                more_line(ui, results.attendances.more);
            }
        });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: i64, title: &str, notes: &str, assignee: &str, done: bool) -> Task {
        let mut task = Task::new(1);
        task.id = id;
        task.title = title.to_owned();
        task.notes = notes.to_owned();
        task.assignee = assignee.to_owned();
        if done {
            task.status = "concluida".to_owned();
        }
        task
    }

    #[test]
    fn folding_drops_case_and_accents() {
        assert_eq!(fold("AÇÃO Média"), "acao media");
        assert_eq!(fold("Parecer Jurídico"), "parecer juridico");
    }

    #[test]
    fn tasks_match_on_title_notes_or_assignee_ignoring_accents() {
        let t = task(
            1,
            "Elaborar parecer",
            "Verificar o prazo",
            "Servidor A",
            false,
        );
        assert!(task_matches(&t, "parecer"));
        assert!(task_matches(&t, "verificar prazo"));
        assert!(task_matches(&t, "servidor a"));
        assert!(!task_matches(&t, "oficio"));
        let u = task(2, "Responder ofício", "", "", false);
        assert!(task_matches(&u, &fold("OFICIO")));
    }

    #[test]
    fn open_tasks_come_before_concluded_ones() {
        let list = vec![
            task(1, "Parecer antigo", "", "", true),
            task(2, "Parecer novo", "", "", false),
            task(3, "Outra coisa", "", "", false),
        ];
        let ids: Vec<i64> = matching_tasks(list, "parecer")
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, [2, 1]);
    }

    #[test]
    fn attendances_match_on_reference_subject_observation_or_assignee() {
        let mut a = SaiopAttendance::new_manual();
        a.reference_raw = "ORG-ATD-2026/0007".to_owned();
        a.subject = "Solicitação de revisão".to_owned();
        a.observation = "Aguardando retorno".to_owned();
        a.assignee = "Servidor B".to_owned();
        assert!(attendance_matches(&a, "atd-2026/0007"));
        assert!(attendance_matches(&a, "solicitacao"));
        assert!(attendance_matches(&a, "retorno"));
        assert!(attendance_matches(&a, "servidor b"));
        assert!(!attendance_matches(&a, "inexistente"));
    }

    #[test]
    fn a_group_keeps_eight_rows_and_counts_the_rest() {
        let group = Group::limited((0..11).collect::<Vec<i32>>());
        assert_eq!(group.rows.len(), 8);
        assert_eq!(group.more, 3);
        let small = Group::limited(vec![1, 2]);
        assert_eq!((small.rows.len(), small.more), (2, 0));
        let exact = Group::limited((0..8).collect::<Vec<i32>>());
        assert_eq!(exact.more, 0);
    }

    #[test]
    fn the_highlight_moves_across_groups_and_wraps() {
        assert_eq!(move_highlight(None, 0, 1), None);
        assert_eq!(move_highlight(None, 5, 1), Some(0));
        assert_eq!(move_highlight(None, 5, -1), Some(4));
        assert_eq!(move_highlight(Some(4), 5, 1), Some(0));
        assert_eq!(move_highlight(Some(0), 5, -1), Some(4));
        assert_eq!(move_highlight(Some(2), 5, 1), Some(3));
    }

    #[test]
    fn rows_are_numbered_across_the_groups() {
        let mut search = GlobalSearch::default();
        search.results.documents = Group::limited(vec![DocumentRow {
            id: 10,
            number: "ORG-OFI-2026/00431".into(),
            title: "Ofício".into(),
            kind: None,
        }]);
        search.results.tasks = Group::limited(vec![TaskRow {
            id: 20,
            title: "Tarefa".into(),
            document: String::new(),
            standalone: false,
            due_date: String::new(),
            done: false,
        }]);
        search.results.attendances = Group::limited(vec![AttendanceRow {
            id: 30,
            reference: "ORG-ATD-1".into(),
            subject: String::new(),
            status: "Em análise".into(),
        }]);
        assert_eq!(search.results.total(), 3);
        assert!(matches!(search.pick_at(0), Some(Pick::Document(10))));
        assert!(matches!(search.pick_at(1), Some(Pick::Task(20))));
        assert!(matches!(search.pick_at(2), Some(Pick::Attendance(30))));
        assert!(search.pick_at(3).is_none());
    }
}
