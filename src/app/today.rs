//! The "Hoje" area: what needs attention now, from every area, in one place.
//! Tasks that are late or due today, atendimentos whose deadline has come,
//! what comes in the next days, and the documents seen most recently.
use super::calendar::{
    self, grid_row_h, line_rect, lines_that_fit, long_date, month_grid, month_weeks, paint_line,
};
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
    /// Every open task with a deadline (the calendar shows them all).
    pub dated_tasks: Vec<Task>,
    /// Every open atendimento with a deadline (the calendar shows them all).
    pub dated_attendances: Vec<SaiopAttendance>,
    /// `dated_tasks` and `dated_attendances` grouped by day, computed once
    /// per refresh (the calendar reads it on every frame).
    by_day: std::collections::BTreeMap<String, Vec<CalItem>>,
    /// Uses of resources in use and past their expected end, oldest first.
    pub overdue_uses: Vec<UseRow>,
    /// Reserved or in-use uses touching today (the late ones are listed
    /// above, not repeated here).
    pub today_uses: Vec<UseRow>,
    /// Why the library could not be read, if it could not.
    pub error: Option<String>,
}

/// A use of a resource as a row of the list (read once per refresh).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UseRow {
    pub uid: String,
    /// The line from `use_line`.
    pub text: String,
    pub tone: Tone,
    /// The situation as the person reads it.
    pub label: String,
}

impl TodayData {
    pub fn load(store: &Store, today: &str) -> Self {
        let mut data = Self::default();
        match store.list_tasks() {
            Ok(tasks) => {
                data.dated_tasks = tasks
                    .iter()
                    .filter(|t| is_open_dated_task(t))
                    .cloned()
                    .collect();
                let (late, due_today, upcoming) = split_tasks(tasks, today);
                data.late = late;
                data.today = due_today;
                data.upcoming = upcoming;
            }
            Err(error) => data.error = Some(error.to_string()),
        }
        match store.list_saiop_attendances() {
            Ok(list) => {
                data.dated_attendances = list
                    .iter()
                    .filter(|a| is_open_dated_attendance(a))
                    .cloned()
                    .collect();
                data.attendances = due_attendances(list, today);
            }
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
        data.by_day = items_by_day(&data.dated_tasks, &data.dated_attendances, today);
        // Resources: the lists are memoized by the library, read once here.
        match (store.list_resources(), store.list_resource_uses()) {
            (Ok(resources), Ok(uses)) => {
                let now = crate::resources::now_moment();
                let groups = resource_groups(&resources, &uses, today, &now);
                data.overdue_uses = groups.overdue;
                data.today_uses = groups.today;
                for (day, items) in groups.by_day {
                    data.by_day.entry(day).or_default().extend(items);
                }
            }
            (Err(error), _) | (_, Err(error)) => data.error = Some(error.to_string()),
        }
        data
    }

    /// Tasks late or due today: the number on the rail.
    pub fn pending_tasks(&self) -> usize {
        self.late.len() + self.today.len()
    }
}

fn is_open_dated_task(task: &Task) -> bool {
    task.completed_at.is_none() && task.status != "concluida" && !task.due_date.is_empty()
}

fn is_open_dated_attendance(attendance: &SaiopAttendance) -> bool {
    attendance.is_open() && !attendance.due_date.is_empty()
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

/// Where a deadline stands against today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DueState {
    Late,
    Today,
    Future,
}

fn due_state(due: &str, today: &str) -> DueState {
    match due.cmp(today) {
        std::cmp::Ordering::Less => DueState::Late,
        std::cmp::Ordering::Equal => DueState::Today,
        std::cmp::Ordering::Greater => DueState::Future,
    }
}

/// Late: danger; today: warn; future: info.
fn due_tone(state: DueState) -> Tone {
    match state {
        DueState::Late => Tone::Danger,
        DueState::Today => Tone::Warn,
        DueState::Future => Tone::Info,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemKind {
    Task,
    Attendance,
    /// A reservation or loan of a resource.
    Resource,
}

/// One deadline in a day of the calendar; `index` points into the task or
/// atendimento list it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CalItem {
    kind: ItemKind,
    index: usize,
    state: DueState,
    /// The line in the grid.
    label: String,
    /// The full text, for the tooltip and the day list.
    detail: String,
    /// Resources only: the use, to open it.
    uid: String,
    /// Resources only: the situation as the person reads it.
    situation: String,
}

/// Open items with a deadline grouped by day (ISO date), tasks first and
/// atendimentos after them in each day.
fn items_by_day(
    tasks: &[Task],
    attendances: &[SaiopAttendance],
    today: &str,
) -> std::collections::BTreeMap<String, Vec<CalItem>> {
    let mut days: std::collections::BTreeMap<String, Vec<CalItem>> = Default::default();
    for (index, task) in tasks.iter().enumerate() {
        if !is_open_dated_task(task) {
            continue;
        }
        days.entry(task.due_date.clone())
            .or_default()
            .push(CalItem {
                kind: ItemKind::Task,
                index,
                state: due_state(&task.due_date, today),
                label: task.title.clone(),
                detail: task.title.clone(),
                uid: String::new(),
                situation: String::new(),
            });
    }
    for (index, attendance) in attendances.iter().enumerate() {
        if !is_open_dated_attendance(attendance) {
            continue;
        }
        let reference = if !attendance.reference_raw.is_empty() {
            attendance.reference_raw.as_str()
        } else if !attendance.subject.is_empty() {
            attendance.subject.as_str()
        } else {
            "Sem referência"
        };
        let detail = if attendance.subject.is_empty() || attendance.subject == reference {
            reference.to_owned()
        } else {
            format!("{reference} · {}", attendance.subject)
        };
        days.entry(attendance.due_date.clone())
            .or_default()
            .push(CalItem {
                kind: ItemKind::Attendance,
                index,
                state: due_state(&attendance.due_date, today),
                label: reference.to_owned(),
                detail,
                uid: String::new(),
                situation: String::new(),
            });
    }
    days
}

/// How many days of a long use the calendar marks, from its start.
const USE_DAYS_LIMIT: i64 = 31;

/// What the area shows about resources.
#[derive(Default)]
struct ResourceGroups {
    overdue: Vec<UseRow>,
    today: Vec<UseRow>,
    by_day: std::collections::BTreeMap<String, Vec<CalItem>>,
}

/// Splits the open uses (reserved or in use) into the late returns, the ones
/// touching `today`, and calendar items: one on each day from the start to
/// the end of the use (at most [`USE_DAYS_LIMIT`] days). Closed uses
/// (returned, cancelled) appear nowhere. Pure: `now` comes from the caller.
fn resource_groups(
    resources: &[crate::resources::Resource],
    uses: &[crate::resources::ResourceUse],
    today: &str,
    now: &str,
) -> ResourceGroups {
    use crate::resources::{parse_moment, UseStatus};
    let names: std::collections::BTreeMap<&str, &str> = resources
        .iter()
        .map(|r| (r.uid.as_str(), r.name.as_str()))
        .collect();
    let name_of = |uid: &str| names.get(uid).copied().unwrap_or("Recurso removido");
    let day_start = format!("{today}T00:00");
    let day_end = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map(|d| {
            format!(
                "{}T00:00",
                (d + chrono::Duration::days(1)).format("%Y-%m-%d")
            )
        })
        .unwrap_or_default();
    let mut groups = ResourceGroups::default();
    let mut overdue: Vec<&crate::resources::ResourceUse> = Vec::new();
    let mut touching: Vec<&crate::resources::ResourceUse> = Vec::new();
    for item in uses {
        if !matches!(item.status, UseStatus::Reserved | UseStatus::InUse) {
            continue;
        }
        let name = name_of(&item.resource_uid);
        let (text, _, label) = crate::resources_ui::use_line(name, item, now);
        if item.overdue_at(now) {
            overdue.push(item);
        } else if item
            .occupies_at(now)
            .is_some_and(|(from, to)| from < day_end && day_start < to)
        {
            touching.push(item);
        }
        // Calendar: the start day and each following day of the use.
        let (Ok(start), Ok(end)) = (parse_moment(&item.start), parse_moment(&item.end)) else {
            continue;
        };
        let mut last = end.date();
        // Ending at midnight sharp does not touch the day that begins there.
        if end.time() == chrono::NaiveTime::MIN && last > start.date() {
            last -= chrono::Duration::days(1);
        }
        let mut day = start.date();
        let mut marked = 0;
        while day <= last && marked < USE_DAYS_LIMIT {
            let iso = day.format("%Y-%m-%d").to_string();
            groups.by_day.entry(iso.clone()).or_default().push(CalItem {
                kind: ItemKind::Resource,
                index: 0,
                state: if item.overdue_at(now) {
                    DueState::Late
                } else {
                    due_state(&iso, today)
                },
                label: name.to_owned(),
                detail: text.clone(),
                uid: item.uid.clone(),
                situation: label.clone(),
            });
            day += chrono::Duration::days(1);
            marked += 1;
        }
    }
    overdue.sort_by(|a, b| (&a.end, &a.uid).cmp(&(&b.end, &b.uid)));
    touching.sort_by(|a, b| (&a.start, &a.uid).cmp(&(&b.start, &b.uid)));
    let row = |item: &crate::resources::ResourceUse| {
        let (text, tone, label) =
            crate::resources_ui::use_line(name_of(&item.resource_uid), item, now);
        UseRow {
            uid: item.uid.clone(),
            text,
            tone,
            label,
        }
    };
    groups.overdue = overdue.into_iter().map(row).collect();
    groups.today = touching.into_iter().map(row).collect();
    groups
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
    /// A use of a resource, by uid.
    OpenUse(String),
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
        let view_id = view_id();
        let mut calendar = ui
            .data(|d| d.get_temp::<bool>(view_id))
            .unwrap_or(DEFAULT_CALENDAR);
        ui.horizontal(|ui| {
            if widgets::chip(ui, !calendar, "Lista").clicked() {
                calendar = false;
            }
            if widgets::chip(ui, calendar, "Calendário").clicked() {
                calendar = true;
            }
        });
        ui.data_mut(|d| d.insert_temp(view_id, calendar));
        ui.add_space(S1);
        let late = self.today.late.len();
        let due = self.today.today.len();
        let attendances = self.today.attendances.len();
        let returns = self.today.overdue_uses.len();
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
            if returns > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(returns, "devolução atrasada", "devoluções atrasadas"),
                    Tone::Danger,
                );
            }
            if late + due + attendances + returns == 0 && self.today.error.is_none() {
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
        let grid_height = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt(if calendar {
                "today-calendar"
            } else {
                "today-area"
            })
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if calendar {
                    ui.set_max_width(ui.available_width() - 14.0);
                    calendar_view(ui, &self.today, now, grid_height, &mut asked);
                    ui.add_space(S4);
                    return;
                }
                // Room for the scroll bar, so the cards keep their right edge.
                ui.set_max_width(ui.available_width() - 14.0);
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
                    use_group(
                        ui,
                        "Devoluções atrasadas",
                        Some(p.danger),
                        &self.today.overdue_uses,
                        asked,
                    );
                    use_group(ui, "Reservas de hoje", None, &self.today.today_uses, asked);
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
            Some(Asked::OpenUse(uid)) => self.open_use(&uid),
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

/// Whether the area opens in the calendar view.
const DEFAULT_CALENDAR: bool = false;

/// Where the chosen view (list or calendar) is remembered for the session.
fn view_id() -> egui::Id {
    egui::Id::NULL.with("today-calendar-view")
}

/// The month on show, as `(year, month)`; absent means the current month.
fn month_id() -> egui::Id {
    egui::Id::NULL.with("today-calendar-month")
}

/// The ISO date of the day opened under the grid; empty means none.
fn day_id() -> egui::Id {
    egui::Id::NULL.with("today-calendar-day")
}

/// Puts the area in the calendar or in the list view (for the picture tour).
#[cfg(feature = "shots")]
pub(super) fn shots_calendar(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| {
        d.insert_temp(view_id(), on);
        d.remove::<(i32, u32)>(month_id());
        d.remove::<String>(day_id());
    });
}

/// The words that say where an item stands.
fn state_text(kind: ItemKind, state: DueState) -> &'static str {
    match (kind, state) {
        (ItemKind::Task, DueState::Late) => "Atrasada",
        (ItemKind::Attendance, DueState::Late) => "Prazo vencido",
        // A use says its own situation (see `item_state`).
        (ItemKind::Resource, _) => "",
        (_, DueState::Today) => "Para hoje",
        (_, DueState::Future) => "No prazo",
    }
}

/// The words that say where a calendar item stands.
fn item_state(item: &CalItem) -> &str {
    match item.kind {
        ItemKind::Resource => item.situation.as_str(),
        kind => state_text(kind, item.state),
    }
}

fn item_text(data: &CalItem, text: &str) -> String {
    match data.kind {
        ItemKind::Task => text.to_owned(),
        ItemKind::Attendance => format!("{} {text}", icon::ATTENDANCES),
        ItemKind::Resource => format!("{} {text}", icon::RESOURCES),
    }
}

fn asked_for(item: &CalItem, data: &TodayData) -> Option<Asked> {
    match item.kind {
        ItemKind::Task => data
            .dated_tasks
            .get(item.index)
            .cloned()
            .map(Asked::OpenTask),
        ItemKind::Attendance => data
            .dated_attendances
            .get(item.index)
            .map(|a| Asked::OpenAttendance(a.id)),
        ItemKind::Resource => Some(Asked::OpenUse(item.uid.clone())),
    }
}

/// The month grid with the deadlines; clicking a day with items lists them
/// under the grid (a "day list" with the date as its title).
fn calendar_view(
    ui: &mut egui::Ui,
    data: &TodayData,
    today: chrono::NaiveDate,
    avail_h: f32,
    asked: &mut Option<Asked>,
) {
    use chrono::Datelike;
    let p = theme::c();
    let (mut year, mut month) = ui
        .data(|d| d.get_temp::<(i32, u32)>(month_id()))
        .unwrap_or((today.year(), today.month()));
    let mut selected = ui
        .data(|d| d.get_temp::<String>(day_id()))
        .unwrap_or_default();

    let shown = (year, month);
    (year, month) = calendar::month_nav(ui, shown, today);
    ui.add_space(S1);

    if (year, month) != shown {
        // The day list belongs to the month that was on show.
        selected.clear();
    }
    let by_day = &data.by_day;
    let row_h = grid_row_h(avail_h, month_weeks(year, month).len());
    let base = ui.make_persistent_id("today-calendar");
    let chosen = month_grid(
        ui,
        base,
        (year, month),
        today,
        &selected,
        row_h,
        |iso| by_day.get(iso).is_some_and(|items| !items.is_empty()),
        |ui, painter, cell| {
            let iso = cell.iso;
            let items: &[CalItem] = if cell.in_month {
                by_day.get(iso).map(Vec::as_slice).unwrap_or(&[])
            } else {
                &[]
            };
            let mut more_clicked = false;
            // The deadlines: as many lines as fit, then "+N".
            let lines = lines_that_fit(cell.row_h);
            let shown = if items.len() > lines {
                lines.saturating_sub(1)
            } else {
                items.len()
            };
            for (i, item) in items.iter().take(shown).enumerate() {
                let line = line_rect(cell.rect, cell.col_w, i);
                let (fg, bg) = due_tone(item.state).colors();
                painter.rect_filled(line, egui::CornerRadius::same(3), bg);
                paint_line(painter, line, &item_text(item, &item.label), fg);
                let response = ui
                    .interact(
                        line,
                        base.with(("cal-item", iso, item.kind as u8, item.index, &item.uid)),
                        egui::Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!(
                        "{}\n{}",
                        item_text(item, &item.detail),
                        item_state(item)
                    ));
                if response.clicked() {
                    if let Some(ask) = asked_for(item, data) {
                        *asked = Some(ask);
                    }
                }
            }
            if items.len() > shown {
                let line = line_rect(cell.rect, cell.col_w, shown);
                let rest = &items[shown..];
                paint_line(painter, line, &format!("+{}", rest.len()), p.muted);
                let list = rest
                    .iter()
                    .map(|item| item_text(item, &item.detail))
                    .collect::<Vec<_>>()
                    .join("\n");
                let response = ui
                    .interact(line, base.with(("cal-more", iso)), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(list);
                if response.clicked() {
                    more_clicked = true;
                }
            }
            more_clicked
        },
    );

    if let Some(iso) = chosen {
        selected = if selected == iso { String::new() } else { iso };
    }
    ui.data_mut(|d| {
        d.insert_temp(month_id(), (year, month));
        d.insert_temp(day_id(), selected.clone());
    });

    // The day list under the grid.
    let Some(items) = by_day.get(&selected) else {
        return;
    };
    let Ok(date) = chrono::NaiveDate::parse_from_str(&selected, "%Y-%m-%d") else {
        return;
    };
    ui.add_space(S3);
    let mut close = false;
    ui.horizontal(|ui| {
        widgets::overline(ui, &long_date(date), None);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.add(widgets::quiet("Fechar")).clicked();
        });
    });
    ui.add_space(S1);
    list_card(ui, |ui| {
        for (index, item) in items.iter().enumerate() {
            let clicked = row(
                ui,
                ("cal-day-item", item.kind as u8, item.index, &item.uid),
                index + 1 == items.len(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S3;
                        ui.add(
                            egui::Label::new(
                                RichText::new(item_text(item, &item.detail))
                                    .font(theme::font_strong(theme::SIZE_BODY)),
                            )
                            .selectable(false)
                            .truncate(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            widgets::pill(ui, item_state(item), due_tone(item.state));
                        });
                    });
                },
            );
            if clicked {
                if let Some(ask) = asked_for(item, data) {
                    *asked = Some(ask);
                }
            }
        }
    });
    if close {
        ui.data_mut(|d| d.insert_temp(day_id(), String::new()));
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
                            // A task without document says so, quietly, and has nothing to open.
                            let document = task.document_id.map(title_of).unwrap_or_default();
                            if task.document_id.is_none() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new("Sem documento")
                                            .size(theme::SIZE_SMALL)
                                            .color(p.muted),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text("Tarefa avulsa: não pertence a nenhum documento.");
                            } else if !document.is_empty() {
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
                if let Some(id) = task.document_id {
                    *asked = Some(Asked::OpenDocument(id));
                }
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

/// Uses of resources (late returns, today's reservations); nothing when none.
fn use_group(
    ui: &mut egui::Ui,
    title: &str,
    color: Option<egui::Color32>,
    uses: &[UseRow],
    asked: &mut Option<Asked>,
) {
    if uses.is_empty() {
        return;
    }
    widgets::overline(ui, title, color);
    ui.add_space(S1);
    list_card(ui, |ui| {
        for (index, item) in uses.iter().enumerate() {
            let clicked = row(
                ui,
                ("today-use", &item.uid),
                index + 1 == uses.len(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S3;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            widgets::pill(ui, item.label.clone(), item.tone);
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&item.text)
                                            .font(theme::font_strong(theme::SIZE_BODY)),
                                    )
                                    .selectable(false)
                                    .truncate(),
                                );
                            });
                        });
                    });
                },
            );
            if clicked {
                *asked = Some(Asked::OpenUse(item.uid.clone()));
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
    use super::calendar::{month_title, shift_month};
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

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn the_month_title_and_navigation_are_in_portuguese() {
        assert_eq!(month_title(2026, 11), "novembro de 2026");
        assert_eq!(month_title(2027, 3), "março de 2027");
        assert_eq!(shift_month((2026, 11), 1), (2026, 12));
        assert_eq!(shift_month((2026, 12), 1), (2027, 1));
        assert_eq!(shift_month((2026, 1), -1), (2025, 12));
    }

    #[test]
    fn the_month_grid_starts_on_sunday_and_covers_the_month() {
        // November 2026 starts on a Sunday and has 30 days: five weeks.
        let weeks = month_weeks(2026, 11);
        assert_eq!(weeks.len(), 5);
        assert_eq!(weeks[0][0], date(2026, 11, 1));
        assert_eq!(weeks[4][6], date(2026, 12, 5));
        // August 2026 starts on a Saturday and has 31 days: six weeks.
        let weeks = month_weeks(2026, 8);
        assert_eq!(weeks.len(), 6);
        assert_eq!(weeks[0][0], date(2026, 7, 26));
        assert_eq!(weeks[0][6], date(2026, 8, 1));
        assert_eq!(weeks[5][0], date(2026, 8, 30));
        // February 2026 fits exactly in four weeks.
        assert_eq!(month_weeks(2026, 2).len(), 4);
        // Every row is seven consecutive days.
        for week in month_weeks(2027, 5) {
            for pair in week.windows(2) {
                assert_eq!(pair[1], pair[0] + chrono::Duration::days(1));
            }
        }
    }

    #[test]
    fn the_deadline_state_gives_the_colour() {
        assert_eq!(due_state("2026-11-09", "2026-11-10"), DueState::Late);
        assert_eq!(due_state("2026-11-10", "2026-11-10"), DueState::Today);
        assert_eq!(due_state("2026-11-11", "2026-11-10"), DueState::Future);
        assert_eq!(due_tone(DueState::Late), Tone::Danger);
        assert_eq!(due_tone(DueState::Today), Tone::Warn);
        assert_eq!(due_tone(DueState::Future), Tone::Info);
    }

    #[test]
    fn items_are_grouped_by_day_with_tasks_first_and_closed_ones_left_out() {
        let tasks = vec![
            task(1, "2026-11-10", "pendente"),
            task(2, "2026-11-10", "concluida"),
            task(3, "", "pendente"),
            task(4, "2026-11-12", "pendente"),
        ];
        let mut open = SaiopAttendance::new_manual();
        open.id = 7;
        open.due_date = "2026-11-10".to_owned();
        open.subject = "Assunto".to_owned();
        let mut closed = open.clone();
        closed.id = 8;
        closed.status = "encerrado".to_owned();
        let mut undated = open.clone();
        undated.id = 9;
        undated.due_date.clear();
        let days = items_by_day(&tasks, &[open, closed, undated], "2026-11-10");
        assert_eq!(days.len(), 2);
        let day = &days["2026-11-10"];
        assert_eq!(day.len(), 2);
        assert_eq!((day[0].kind, day[0].index), (ItemKind::Task, 0));
        assert_eq!((day[1].kind, day[1].index), (ItemKind::Attendance, 0));
        assert_eq!(day[1].label, "Assunto");
        assert_eq!(day[1].state, DueState::Today);
        assert_eq!(days["2026-11-12"][0].state, DueState::Future);
    }

    fn resource(uid: &str, name: &str) -> crate::resources::Resource {
        crate::resources::Resource {
            uid: uid.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    fn booking(
        uid: &str,
        status: crate::resources::UseStatus,
        start: &str,
        end: &str,
    ) -> crate::resources::ResourceUse {
        crate::resources::ResourceUse {
            uid: uid.into(),
            resource_uid: "r1".into(),
            who: "Servidor A".into(),
            start: start.into(),
            end: end.into(),
            status,
            notes: "Levar o cabo".into(),
            ..Default::default()
        }
    }

    #[test]
    fn resources_split_into_late_returns_today_and_calendar_days() {
        use crate::resources::UseStatus::{Cancelled, InUse, Reserved, Returned};
        let resources = [resource("r1", "Projetor")];
        let uses = [
            booking("late", InUse, "2026-11-08T09:00", "2026-11-09T09:00"),
            booking("today", Reserved, "2026-11-10T14:00", "2026-11-10T16:00"),
            booking("span", Reserved, "2026-11-09T18:00", "2026-11-11T08:00"),
            booking("later", Reserved, "2026-11-15T09:00", "2026-11-15T10:00"),
            booking("done", Returned, "2026-11-10T09:00", "2026-11-10T10:00"),
            booking("off", Cancelled, "2026-11-10T09:00", "2026-11-10T10:00"),
        ];
        let groups = resource_groups(&resources, &uses, "2026-11-10", "2026-11-10T10:00");
        let uids = |rows: &[UseRow]| rows.iter().map(|r| r.uid.clone()).collect::<Vec<_>>();
        assert_eq!(uids(&groups.overdue), ["late"]);
        assert_eq!(groups.overdue[0].label, "Atrasado");
        assert_eq!(groups.overdue[0].tone, Tone::Danger);
        // The late one is listed above, not repeated; the multi-day one
        // touches today.
        assert_eq!(uids(&groups.today), ["span", "today"]);
        // Never the notes.
        assert!(!groups.today[0].text.contains("cabo"));
        // Calendar: closed uses are absent; the multi-day use is on each day.
        let days = &groups.by_day;
        assert!(days
            .values()
            .flatten()
            .all(|i| i.uid != "done" && i.uid != "off"));
        let on = |day: &str| {
            days.get(day)
                .map(|items| items.iter().map(|i| i.uid.as_str()).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        assert_eq!(on("2026-11-08"), ["late"]);
        assert_eq!(on("2026-11-09"), ["late", "span"]);
        assert_eq!(on("2026-11-10"), ["today", "span"]);
        assert_eq!(on("2026-11-11"), ["span"]);
        assert_eq!(on("2026-11-15"), ["later"]);
        // Tones: overdue danger, today warn, future info.
        let item = |day: &str, uid: &str| {
            days[day]
                .iter()
                .find(|i| i.uid == uid)
                .expect("item")
                .clone()
        };
        assert_eq!(due_tone(item("2026-11-08", "late").state), Tone::Danger);
        assert_eq!(due_tone(item("2026-11-10", "today").state), Tone::Warn);
        assert_eq!(due_tone(item("2026-11-15", "later").state), Tone::Info);
        assert_eq!(item("2026-11-10", "today").situation, "Reservado");
        assert_eq!(item("2026-11-10", "today").kind, ItemKind::Resource);
    }

    #[test]
    fn a_long_use_marks_at_most_31_days_and_midnight_ends_the_day_before() {
        use crate::resources::UseStatus::Reserved;
        let resources = [resource("r1", "Auditório")];
        let uses = [
            booking("long", Reserved, "2026-11-01T09:00", "2027-02-01T09:00"),
            booking("midnight", Reserved, "2026-11-20T20:00", "2026-11-21T00:00"),
        ];
        let groups = resource_groups(&resources, &uses, "2026-11-10", "2026-11-10T10:00");
        let long = groups
            .by_day
            .values()
            .filter(|items| items.iter().any(|i| i.uid == "long"))
            .count();
        assert_eq!(long, 31);
        assert!(groups.by_day.contains_key("2026-12-01"));
        assert!(!groups.by_day.contains_key("2026-12-02"));
        // Ending at midnight sharp does not touch the day that begins there.
        assert!(groups.by_day["2026-11-20"]
            .iter()
            .any(|i| i.uid == "midnight"));
        assert!(!groups
            .by_day
            .get("2026-11-21")
            .map(|items| items.iter().any(|i| i.uid == "midnight"))
            .unwrap_or(false));
    }

    #[test]
    fn the_library_load_reads_resources_once_into_groups_and_calendar() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let uid = store
            .save_resource(
                &crate::resources::Resource {
                    name: "Auditório".into(),
                    kind: "sala".into(),
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        let start = chrono::Local::now().naive_local() + chrono::Duration::days(3);
        let item = crate::resources::ResourceUse {
            resource_uid: uid,
            quantity: 1,
            who: "Servidor A".into(),
            start: crate::resources::format_moment(start),
            end: crate::resources::format_moment(start + chrono::Duration::hours(2)),
            ..Default::default()
        };
        store.save_resource_use(&item, None).unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let data = TodayData::load(&store, &today);
        assert!(data.overdue_uses.is_empty() && data.today_uses.is_empty());
        assert_eq!(
            data.by_day
                .values()
                .flatten()
                .filter(|i| i.kind == ItemKind::Resource)
                .count(),
            1
        );
        assert_eq!(data.pending_tasks(), 0);
    }

    #[test]
    fn the_cached_grouping_equals_the_function_output() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let capture = crate::model::sample_capture();
        let document = store.import_capture(&capture).unwrap().document_id;
        let data = TodayData::load(&store, "2026-11-10");
        assert_eq!(
            data.by_day,
            items_by_day(&data.dated_tasks, &data.dated_attendances, "2026-11-10")
        );
        let mut dated = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.document_id == Some(document))
            .expect("triage task");
        dated.due_date = "2026-11-12".to_owned();
        store.save_task(&dated).unwrap();
        let data = TodayData::load(&store, "2026-11-10");
        assert_eq!(data.by_day["2026-11-12"].len(), 1);
        assert_eq!(
            data.by_day,
            items_by_day(&data.dated_tasks, &data.dated_attendances, "2026-11-10")
        );
    }
}
