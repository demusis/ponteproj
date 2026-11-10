//! The "Hoje" area: what needs attention now, from every area, in one place.
//! Tasks that are late or due today, atendimentos whose deadline has come,
//! what comes in the next days, and the documents seen most recently.
use super::{Area, Leave, NoticeAction, PonteApp, ReturnTo};
use crate::model::DocumentSummary;
use crate::saiop::SaiopAttendance;
use crate::storage::Store;
use crate::tasks::Task;
use crate::theme::{self, icon, S1, S2, S3, S4};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};

/// How many of the coming tasks and of the recent documents are listed.
const UPCOMING_LIMIT: usize = 6;
const RECENT_LIMIT: usize = 6;

/// What the area shows, read from the library on each refresh (not on each frame).
#[derive(Default)]
pub(super) struct TodayData {
    pub late: Vec<Task>,
    pub today: Vec<Task>,
    pub upcoming: Vec<Task>,
    /// Open atendimentos whose deadline is today or already past.
    pub attendances: Vec<SaiopAttendance>,
    pub recent: Vec<DocumentSummary>,
    /// Number and title of every document, to name the document of a task.
    pub index: std::collections::BTreeMap<i64, (String, String)>,
    /// Why the library could not be read, if it could not.
    pub error: Option<String>,
}

impl TodayData {
    pub fn load(store: &Store, today: &str) -> Self {
        let mut data = Self::default();
        match store.list_tasks() {
            Ok(tasks) => {
                let (late, due_today, upcoming) = split_tasks(tasks, today);
                data.late = late;
                data.today = due_today;
                data.upcoming = upcoming;
            }
            Err(error) => data.error = Some(error.to_string()),
        }
        match store.list_saiop_attendances() {
            Ok(list) => data.attendances = due_attendances(list, today),
            Err(error) => data.error = Some(error.to_string()),
        }
        match store.list_documents("") {
            Ok(mut documents) => {
                data.index = documents
                    .iter()
                    .map(|d| (d.id, (d.document_number.clone(), d.title.clone())))
                    .collect();
                documents.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));
                documents.truncate(RECENT_LIMIT);
                data.recent = documents;
            }
            Err(error) => data.error = Some(error.to_string()),
        }
        data
    }

    /// Tasks late or due today: the number on the rail.
    pub fn pending_tasks(&self) -> usize {
        self.late.len() + self.today.len()
    }
}

/// Open tasks split into late, due today and the next ones (soonest first).
fn split_tasks(tasks: Vec<Task>, today: &str) -> (Vec<Task>, Vec<Task>, Vec<Task>) {
    let mut late = Vec::new();
    let mut due_today = Vec::new();
    let mut upcoming = Vec::new();
    for task in tasks {
        if task.completed_at.is_some() || task.status == "concluida" || task.due_date.is_empty() {
            continue;
        }
        match task.due_date.as_str().cmp(today) {
            std::cmp::Ordering::Less => late.push(task),
            std::cmp::Ordering::Equal => due_today.push(task),
            std::cmp::Ordering::Greater => upcoming.push(task),
        }
    }
    late.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    upcoming.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    upcoming.truncate(UPCOMING_LIMIT);
    (late, due_today, upcoming)
}

/// Open atendimentos with a deadline that is today or past, oldest first.
fn due_attendances(list: Vec<SaiopAttendance>, today: &str) -> Vec<SaiopAttendance> {
    let mut due: Vec<SaiopAttendance> = list
        .into_iter()
        .filter(|a| a.is_open() && !a.due_date.is_empty() && a.due_date.as_str() <= today)
        .collect();
    due.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    due
}

/// "terça-feira, 10 de novembro".
fn long_date(date: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    const DAYS: [&str; 7] = [
        "segunda-feira",
        "terça-feira",
        "quarta-feira",
        "quinta-feira",
        "sexta-feira",
        "sábado",
        "domingo",
    ];
    const MONTHS: [&str; 12] = [
        "janeiro",
        "fevereiro",
        "março",
        "abril",
        "maio",
        "junho",
        "julho",
        "agosto",
        "setembro",
        "outubro",
        "novembro",
        "dezembro",
    ];
    format!(
        "{}, {} de {}",
        DAYS[date.weekday().num_days_from_monday() as usize],
        date.day(),
        MONTHS[date.month0() as usize]
    )
}

/// "07/11" from "2026-11-07".
fn day_month(iso: &str) -> String {
    match (iso.get(8..10), iso.get(5..7)) {
        (Some(day), Some(month)) => format!("{day}/{month}"),
        _ => iso.to_owned(),
    }
}

/// What the person asked for in a row.
enum Asked {
    Conclude(Task),
    OpenTask(Task),
    OpenDocument(i64),
    OpenAttendance(i64),
}

impl PonteApp {
    pub(super) fn today_area(&mut self, ui: &mut egui::Ui) {
        let p = theme::c();
        let now = chrono::Local::now().date_naive();
        let mut asked: Option<Asked> = None;
        ui.horizontal(|ui| {
            widgets::heading(ui, "Hoje");
            widgets::muted(ui, long_date(now));
        });
        let late = self.today.late.len();
        let due = self.today.today.len();
        let attendances = self.today.attendances.len();
        ui.horizontal_wrapped(|ui| {
            if late > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(late, "tarefa atrasada", "tarefas atrasadas"),
                    Tone::Danger,
                );
            }
            if due > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(due, "tarefa para hoje", "tarefas para hoje"),
                    Tone::Warn,
                );
            }
            if attendances > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(
                        attendances,
                        "atendimento com prazo vencido ou para hoje",
                        "atendimentos com prazo vencido ou para hoje",
                    ),
                    Tone::Info,
                );
            }
            if late + due + attendances == 0 && self.today.error.is_none() {
                widgets::pill(ui, "Nada vence hoje", Tone::Ok);
            }
        });
        if let Some(error) = &self.today.error {
            let (fg, bg) = Tone::Danger.colors();
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Não foi possível ler o acervo: {error}")).color(fg),
                    );
                });
        }
        ui.add_space(S2);
        egui::ScrollArea::vertical()
            .id_salt("today-area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Room for the scroll bar, so the cards keep their right edge.
                ui.set_max_width((ui.available_width() - 14.0).min(1180.0));
                let wide = ui.available_width() >= 860.0;
                let documents = &self.today.index;
                let title_of = |id: i64| {
                    documents
                        .get(&id)
                        .map(|(number, title)| format!("{number} · {title}"))
                        .unwrap_or_default()
                };
                let left = |ui: &mut egui::Ui, asked: &mut Option<Asked>| {
                    task_group(
                        ui,
                        "Atrasadas",
                        Some(p.danger),
                        &self.today.late,
                        &now.to_string(),
                        &title_of,
                        asked,
                    );
                    task_group(
                        ui,
                        "Para hoje",
                        None,
                        &self.today.today,
                        &now.to_string(),
                        &title_of,
                        asked,
                    );
                    if self.today.late.is_empty()
                        && self.today.today.is_empty()
                        && self.today.error.is_none()
                    {
                        widgets::overline(ui, "Tarefas", None);
                        ui.add_space(S1);
                        widgets::card(ui, |ui| {
                            widgets::muted(ui, "Nenhuma tarefa atrasada ou para hoje.");
                        });
                        ui.add_space(S3);
                    }
                    task_group(
                        ui,
                        "Próximos dias",
                        None,
                        &self.today.upcoming,
                        &now.to_string(),
                        &title_of,
                        asked,
                    );
                };
                let right = |ui: &mut egui::Ui, asked: &mut Option<Asked>| {
                    attendance_group(ui, &self.today.attendances, &now.to_string(), asked);
                    recent_group(ui, &self.today.recent, asked);
                };
                if wide {
                    ui.columns(2, |columns| {
                        left(&mut columns[0], &mut asked);
                        right(&mut columns[1], &mut asked);
                    });
                } else {
                    left(ui, &mut asked);
                    right(ui, &mut asked);
                }
                ui.add_space(S4);
            });
        match asked {
            None => {}
            Some(Asked::Conclude(task)) => self.conclude_from_today(task),
            Some(Asked::OpenTask(task)) => {
                // The list may be a few seconds old: open what the library holds now.
                let current = self
                    .store
                    .list_tasks()
                    .ok()
                    .and_then(|tasks| tasks.into_iter().find(|t| t.id == task.id));
                match current {
                    Some(current) => self.navigate(Leave::OpenTask(current)),
                    None => {
                        self.refresh();
                        self.message("Esta tarefa não está mais disponível.");
                    }
                }
            }
            Some(Asked::OpenDocument(id)) => {
                self.return_after_select = Some(ReturnTo::Today);
                self.navigate(Leave::Select(id));
            }
            Some(Asked::OpenAttendance(id)) => {
                if self.saiop.focus_attendance(&self.store, id) {
                    self.area = Area::Saiop;
                } else {
                    self.error("Há um atendimento com alterações não salvas em Atendimentos. Salve ou descarte antes de abrir outro.");
                }
            }
        }
    }

    /// Concludes a task from the "Hoje" list, offering to undo it.
    fn conclude_from_today(&mut self, task: Task) {
        if self.tasks.open_task_id() == Some(task.id) && self.tasks.dirty() {
            self.error("Esta tarefa está aberta em Tarefas com alterações não salvas. Salve ou cancele a edição antes de concluí-la.");
            return;
        }
        match self.store.complete_task(task.id) {
            Ok(_) => {
                // A clean copy of this task open in Tarefas would be stale.
                self.tasks.forget_task(task.id);
                let cascade = self.store.take_cascade_notes();
                self.refresh();
                if cascade.is_empty() {
                    self.message_with_action(
                        format!("Tarefa concluída: {}", task.title),
                        "Desfazer",
                        NoticeAction::ReopenTask(task.id),
                    );
                } else {
                    // Other tasks were concluded with it: no one-click undo.
                    self.message(format!(
                        "Tarefa concluída: {}. {}",
                        task.title,
                        cascade.join(" ")
                    ));
                }
            }
            Err(error) => self.error(format!("Não foi possível concluir a tarefa: {error}")),
        }
    }
}

/// One row with a thin line under it; returns its rectangle and whether the
/// empty part of the row was clicked.
fn row(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    last: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
    let p = theme::c();
    let inner = ui
        .scope_builder(
            egui::UiBuilder::new()
                .id_salt(id)
                .sense(egui::Sense::click()),
            |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(S3 as i8, 9))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        add(ui);
                    });
            },
        )
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if !last {
        ui.painter().hline(
            inner.rect.x_range(),
            inner.rect.bottom(),
            egui::Stroke::new(1.0, p.outline),
        );
    }
    inner.clicked()
}

fn list_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    widgets::card_frame()
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

fn task_group(
    ui: &mut egui::Ui,
    title: &str,
    color: Option<egui::Color32>,
    tasks: &[Task],
    today: &str,
    title_of: &dyn Fn(i64) -> String,
    asked: &mut Option<Asked>,
) {
    if tasks.is_empty() {
        return;
    }
    let p = theme::c();
    widgets::overline(ui, title, color);
    ui.add_space(S1);
    list_card(ui, |ui| {
        for (index, task) in tasks.iter().enumerate() {
            let mut conclude = false;
            let mut open_document = false;
            let clicked = row(
                ui,
                ("today-task", task.id),
                index + 1 == tasks.len(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S3;
                        conclude = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(icon::CIRCLE).size(22.0).color(p.muted),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Conclui a tarefa. Nada é enviado ao SIGADOC.")
                            .clicked();
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
                            let document = title_of(task.document_id);
                            if !document.is_empty() {
                                open_document = ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(document)
                                                .size(theme::SIZE_SMALL)
                                                .color(p.muted),
                                        )
                                        .selectable(false)
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text("Abrir o documento desta tarefa")
                                    .clicked();
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            let (text, tone) = if task.due_date.as_str() < today {
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
                            if matches!(task.priority.as_str(), "urgente" | "alta") {
                                widgets::pill(
                                    ui,
                                    crate::labels::priority(&task.priority),
                                    if task.priority == "urgente" {
                                        Tone::Danger
                                    } else {
                                        Tone::Warn
                                    },
                                );
                            }
                        });
                    });
                },
            );
            if conclude {
                *asked = Some(Asked::Conclude(task.clone()));
            } else if open_document {
                *asked = Some(Asked::OpenDocument(task.document_id));
            } else if clicked {
                *asked = Some(Asked::OpenTask(task.clone()));
            }
        }
    });
    ui.add_space(S3);
}

fn attendance_group(
    ui: &mut egui::Ui,
    attendances: &[SaiopAttendance],
    today: &str,
    asked: &mut Option<Asked>,
) {
    widgets::overline(ui, "Atendimentos com prazo vencido ou para hoje", None);
    ui.add_space(S1);
    if attendances.is_empty() {
        widgets::card(ui, |ui| {
            widgets::muted(ui, "Nenhum atendimento com prazo vencido ou para hoje.");
        });
        ui.add_space(S3);
        return;
    }
    let p = theme::c();
    list_card(ui, |ui| {
        for (index, attendance) in attendances.iter().enumerate() {
            let clicked = row(
                ui,
                ("today-attendance", attendance.id),
                index + 1 == attendances.len(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S3;
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let reference = if attendance.reference_raw.is_empty() {
                                "Sem referência"
                            } else {
                                attendance.reference_raw.as_str()
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(reference)
                                        .font(theme::font_strong(theme::SIZE_BODY)),
                                )
                                .selectable(false)
                                .truncate(),
                            );
                            let mut detail = attendance.subject.clone();
                            if !attendance.assignee.is_empty() {
                                if !detail.is_empty() {
                                    detail.push_str(" · ");
                                }
                                detail.push_str(&attendance.assignee);
                            }
                            if !detail.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(detail)
                                            .size(theme::SIZE_SMALL)
                                            .color(p.muted),
                                    )
                                    .selectable(false)
                                    .truncate(),
                                );
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if attendance.due_date.as_str() < today {
                                widgets::pill(
                                    ui,
                                    format!("Vencido · {}", day_month(&attendance.due_date)),
                                    Tone::Danger,
                                );
                            } else {
                                widgets::pill(ui, "Hoje", Tone::Warn);
                            }
                        });
                    });
                },
            );
            if clicked {
                *asked = Some(Asked::OpenAttendance(attendance.id));
            }
        }
    });
    ui.add_space(S3);
}

fn recent_group(ui: &mut egui::Ui, documents: &[DocumentSummary], asked: &mut Option<Asked>) {
    if documents.is_empty() {
        return;
    }
    let p = theme::c();
    widgets::overline(ui, "Documentos recentes", None);
    ui.add_space(S1);
    list_card(ui, |ui| {
        for (index, document) in documents.iter().enumerate() {
            let clicked = row(
                ui,
                ("today-document", document.id),
                index + 1 == documents.len(),
                |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(
                            egui::Label::new(
                                RichText::new(&document.document_number)
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
                },
            );
            if clicked {
                *asked = Some(Asked::OpenDocument(document.id));
            }
        }
    });
    ui.add_space(S3);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: i64, due: &str, status: &str) -> Task {
        let mut task = Task::new(1);
        task.id = id;
        task.title = format!("Tarefa {id}");
        task.due_date = due.to_owned();
        task.status = status.to_owned();
        task
    }

    #[test]
    fn tasks_are_split_by_deadline_and_concluded_or_undated_ones_stay_out() {
        let tasks = vec![
            task(1, "2026-11-08", "pendente"),
            task(2, "2026-11-10", "pendente"),
            task(3, "2026-11-12", "pendente"),
            task(4, "", "pendente"),
            task(5, "2026-11-01", "concluida"),
            task(6, "2026-11-05", "aguardando"),
        ];
        let (late, today, upcoming) = split_tasks(tasks, "2026-11-10");
        assert_eq!(late.iter().map(|t| t.id).collect::<Vec<_>>(), [6, 1]);
        assert_eq!(today.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
        assert_eq!(upcoming.iter().map(|t| t.id).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn the_long_date_is_written_in_portuguese() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        assert_eq!(long_date(date), "terça-feira, 10 de novembro");
        assert_eq!(day_month("2026-11-07"), "07/11");
    }
}
