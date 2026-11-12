//! The "Recursos" area: rooms, books and equipment, and their uses (a
//! booking or a loan with a start and an end date-time).
//!
//! Three views in tabs: what is happening now and next ("Agora e próximos"),
//! the month ("Calendário") and the register of resources ("Recursos"). The
//! ficha of a use or of a resource opens in a panel on the right, with its
//! buttons fixed in the footer, as in Atendimentos.
//!
//! The rules (clashes, quantities, statuses) live in `crate::resources`; this
//! file only draws and asks the library. "Now" is read once per frame and
//! handed down: no row calls the clock.
use crate::{
    doc_picker::DocumentPicker,
    resources::{
        self, day_counts, free_quantity, kind_label, moment_date, moment_time, now_moment,
        parse_moment, strip_marker, Resource, ResourceStatus, ResourceUse, UseStatus,
        RESOURCE_KINDS,
    },
    storage::Store,
    tasks::Task,
    theme::{self, c, icon, S1, S2, S3, S4},
    widgets::{self, form_section, labeled, pick, pick_row, Tone},
};
use chrono::{Duration, NaiveDate};
use eframe::egui::{self, RichText};
use std::collections::BTreeMap;

/// Width of the side panel (the ficha).
const FICHA_WIDTH: f32 = 440.0;
/// How many days ahead "Próximos dias" looks.
const HORIZON_DAYS: i64 = 14;

/// What the person asked the app to open from a link of a use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Document(i64),
    Attendance(i64),
    Task(i64),
}

/// What a new use is born linked to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseLink {
    Document(i64),
    Attendance(i64),
    Task(i64),
}

/// What a panel of another area (atendimento, task) asks the app to do with
/// the uses of resources linked to it: the panel cannot navigate by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UseRequest {
    /// Open this use (by uid) in Recursos.
    Open(String),
    /// Start a new use linked to the atendimento or task.
    Reserve(UseLink),
}

// ---------------------------------------------------------------------------
// Pure helpers (no window, no library)

/// The uses linked to `link`, in the order given.
pub fn uses_linked_to<'a>(uses: &'a [ResourceUse], link: &UseLink) -> Vec<&'a ResourceUse> {
    uses.iter()
        .filter(|item| match link {
            UseLink::Document(id) => item.document_id == Some(*id),
            UseLink::Attendance(id) => item.saiop_id == Some(*id),
            UseLink::Task(id) => item.task_id == Some(*id),
        })
        .collect()
}

/// A use as the other screens list it (ficha of a document, atendimento or
/// task, exported ficha). Never carries the notes of the use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedUse {
    pub uid: String,
    pub resource: String,
    pub who: String,
    pub sector: String,
    /// "10/11 14:00–16:00".
    pub period: String,
    /// The line from [`use_line`].
    pub text: String,
    pub tone: Tone,
    /// The situation as the person reads it ("Reservado", "Atrasado"...).
    pub label: String,
}

/// The uses linked to `link`, ready to list, from lists already read.
pub fn linked_uses_from(
    resources: &[Resource],
    uses: &[ResourceUse],
    link: &UseLink,
    now: &str,
) -> Vec<LinkedUse> {
    uses_linked_to(uses, link)
        .into_iter()
        .map(|item| {
            let name = resources
                .iter()
                .find(|r| r.uid == item.resource_uid)
                .map_or("Recurso removido", |r| r.name.as_str());
            let (text, tone, label) = use_line(name, item, now);
            LinkedUse {
                uid: item.uid.clone(),
                resource: name.to_owned(),
                who: item.who.clone(),
                sector: item.sector.clone(),
                period: period_text(&item.start, &item.end),
                text,
                tone,
                label,
            }
        })
        .collect()
}

/// The uses linked to `link`, read from the memoized lists of the library
/// (empty when they cannot be read). Call once per frame, not per row.
pub fn linked_uses(store: &Store, link: &UseLink) -> Vec<LinkedUse> {
    let Ok(uses) = store.list_resource_uses() else {
        return Vec::new();
    };
    if !uses.iter().any(|item| match link {
        UseLink::Document(id) => item.document_id == Some(*id),
        UseLink::Attendance(id) => item.saiop_id == Some(*id),
        UseLink::Task(id) => item.task_id == Some(*id),
    }) {
        return Vec::new();
    }
    let resources = store.list_resources().unwrap_or_default();
    linked_uses_from(&resources, &uses, link, &now_moment())
}

/// "10/11 14:00–16:00", or both dates when the use crosses days.
pub fn period_text(start: &str, end: &str) -> String {
    let short = |moment: &str| match parse_moment(moment) {
        Ok(m) => m.format("%d/%m").to_string(),
        Err(_) => moment.to_owned(),
    };
    if moment_date(start) == moment_date(end) {
        format!(
            "{} {}–{}",
            short(start),
            moment_time(start),
            moment_time(end)
        )
    } else {
        format!(
            "{} {} – {} {}",
            short(start),
            moment_time(start),
            short(end),
            moment_time(end)
        )
    }
}

/// The pill of a use: the situation, "Atrasado" when the return is late.
fn status_pill(item: &ResourceUse, now: &str) -> (&'static str, Tone) {
    if item.overdue_at(now) {
        return ("Atrasado", Tone::Danger);
    }
    let tone = match item.status {
        UseStatus::Reserved => Tone::Info,
        UseStatus::InUse => Tone::Violet,
        UseStatus::Returned => Tone::Ok,
        UseStatus::Cancelled => Tone::Neutral,
    };
    (item.status.label(), tone)
}

/// The one-line summary of a use shown wherever it is listed: the text, the
/// tone of its situation and the situation as the person reads it
/// ("Reservado", "Em uso", "Devolvido", "Cancelado", or "Atrasado").
pub fn use_line(resource_name: &str, item: &ResourceUse, now: &str) -> (String, Tone, String) {
    let (label, tone) = status_pill(item, now);
    let who = item.who.trim();
    let text = if who.is_empty() {
        format!("{resource_name} · {}", period_text(&item.start, &item.end))
    } else {
        format!(
            "{resource_name} · {who} · {}",
            period_text(&item.start, &item.end)
        )
    };
    (text, tone, label.to_owned())
}

/// The next full half hour after `now`, and one hour later.
pub fn default_times(now: &str) -> (String, String) {
    let moment = parse_moment(now)
        .or_else(|_| parse_moment(&now_moment()))
        .unwrap_or_default();
    let ahead = 30 - i64::from(chrono::Timelike::minute(&moment)) % 30;
    let start = moment + Duration::minutes(ahead);
    (
        resources::format_moment(start),
        resources::format_moment(start + Duration::hours(1)),
    )
}

/// Default period for a new use on `day` (ISO): the next half hour when the
/// day is today, nine o'clock otherwise.
pub fn times_for_day(day: &str, now: &str) -> (String, String) {
    if moment_date(now) == day {
        let (start, end) = default_times(now);
        if moment_date(&start) == day {
            return (start, end);
        }
    }
    (format!("{day}T09:00"), format!("{day}T10:00"))
}

/// What of a resource is out at `now`: every use in use (even a late one),
/// and the reserved ones whose time has come.
pub fn taken_now(uses: &[ResourceUse], resource_uid: &str, now: &str) -> u32 {
    uses.iter()
        .filter(|item| item.resource_uid == resource_uid)
        .filter(|item| match item.status {
            UseStatus::InUse => true,
            UseStatus::Reserved => item.start.as_str() <= now && now < item.end.as_str(),
            UseStatus::Returned | UseStatus::Cancelled => false,
        })
        .map(|item| item.quantity)
        .sum()
}

/// What view 1 shows, by group.
#[derive(Debug, Default)]
pub struct Groups<'a> {
    pub overdue: Vec<&'a ResourceUse>,
    pub in_use: Vec<&'a ResourceUse>,
    pub today: Vec<&'a ResourceUse>,
    pub next_days: Vec<&'a ResourceUse>,
    pub later: Vec<&'a ResourceUse>,
    pub older: Vec<&'a ResourceUse>,
}

impl Groups<'_> {
    pub fn current(&self) -> usize {
        self.overdue.len()
            + self.in_use.len()
            + self.today.len()
            + self.next_days.len()
            + self.later.len()
    }
}

/// Splits uses into Atrasados / Em uso agora / Hoje / Próximos dias, then
/// what lies further ahead, then what is over (returned, cancelled, or a
/// reservation whose day passed without being used).
pub fn group_uses<'a>(items: &[&'a ResourceUse], today: NaiveDate, now: &str) -> Groups<'a> {
    let today_iso = today.format("%Y-%m-%d").to_string();
    let horizon = (today + Duration::days(HORIZON_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let mut groups = Groups::default();
    for &item in items {
        match item.status {
            UseStatus::Returned | UseStatus::Cancelled => groups.older.push(item),
            UseStatus::InUse if item.overdue_at(now) => groups.overdue.push(item),
            UseStatus::InUse => groups.in_use.push(item),
            UseStatus::Reserved => {
                let (first, last) = (moment_date(&item.start), moment_date(&item.end));
                if last < today_iso {
                    groups.older.push(item);
                } else if first <= today_iso {
                    groups.today.push(item);
                } else if first <= horizon {
                    groups.next_days.push(item);
                } else {
                    groups.later.push(item);
                }
            }
        }
    }
    groups.overdue.sort_by(|a, b| a.end.cmp(&b.end));
    groups.in_use.sort_by(|a, b| a.end.cmp(&b.end));
    groups.older.sort_by(|a, b| b.start.cmp(&a.start));
    groups
}

/// The filters of view 1.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub query: String,
    /// Kind of the resource ("" = any).
    pub kind: String,
    pub status: Option<UseStatus>,
    /// ISO dates; the use must touch `[from, to]`.
    pub from: String,
    pub to: String,
    /// Uid of the resource ("" = any).
    pub resource: String,
}

impl Filters {
    pub fn active(&self) -> bool {
        self != &Self::default()
    }

    /// The filters that are not the search, named for the person.
    pub fn named(&self, resources: &[Resource]) -> Vec<String> {
        let mut out = Vec::new();
        if !self.kind.is_empty() {
            out.push(format!("tipo {}", kind_label(&self.kind).to_lowercase()));
        }
        if let Some(status) = self.status {
            out.push(format!("situação {}", status.label().to_lowercase()));
        }
        if !self.from.is_empty() || !self.to.is_empty() {
            out.push("período escolhido".to_owned());
        }
        if let Some(r) = resources.iter().find(|r| r.uid == self.resource) {
            out.push(format!("recurso {}", r.name));
        }
        out
    }
}

/// Lowercase without the accents of Portuguese, for searching.
fn fold_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Whether a use passes the filters (the resource is its resource, if known).
pub fn use_matches(item: &ResourceUse, resource: Option<&Resource>, filters: &Filters) -> bool {
    if !filters.resource.is_empty() && item.resource_uid != filters.resource {
        return false;
    }
    if !filters.kind.is_empty() && resource.is_none_or(|r| r.kind != filters.kind) {
        return false;
    }
    if filters.status.is_some_and(|status| item.status != status) {
        return false;
    }
    let (first, last) = (moment_date(&item.start), moment_date(&item.end));
    if !filters.from.is_empty() && last.as_str() < filters.from.as_str() {
        return false;
    }
    if !filters.to.is_empty() && first.as_str() > filters.to.as_str() {
        return false;
    }
    let query = fold_text(filters.query.trim());
    if query.is_empty() {
        return true;
    }
    let haystack = fold_text(&format!(
        "{} {} {} {}",
        resource.map(|r| r.name.as_str()).unwrap_or(""),
        item.who,
        item.sector,
        item.purpose
    ));
    query.split_whitespace().all(|word| haystack.contains(word))
}

/// The uses on each day of `[first, last]`, cancelled ones left out; a use
/// that crosses days is on each of them. `resource` ("" = all) narrows them.
pub fn uses_by_day<'a>(
    uses: &'a [ResourceUse],
    resource: &str,
    first: NaiveDate,
    last: NaiveDate,
) -> BTreeMap<String, Vec<&'a ResourceUse>> {
    let mut days: BTreeMap<String, Vec<&ResourceUse>> = BTreeMap::new();
    for item in uses {
        if item.status == UseStatus::Cancelled
            || (!resource.is_empty() && item.resource_uid != resource)
        {
            continue;
        }
        let (Ok(start), Ok(end)) = (parse_moment(&item.start), parse_moment(&item.end)) else {
            continue;
        };
        let mut from = start.date();
        let mut to = end.date();
        // Ending at midnight sharp does not touch the day that begins there.
        if end.time() == chrono::NaiveTime::MIN && to > from {
            to -= Duration::days(1);
        }
        from = from.max(first);
        to = to.min(last);
        let mut day = from;
        while day <= to {
            days.entry(day.format("%Y-%m-%d").to_string())
                .or_default()
                .push(item);
            day += Duration::days(1);
        }
    }
    days
}

/// The other uses holding `resource_uid` on `day` (ISO), by start.
pub fn day_occupation<'a>(
    uses: &'a [ResourceUse],
    resource_uid: &str,
    except_uid: &str,
    day: &str,
    now: &str,
) -> Vec<&'a ResourceUse> {
    let from = format!("{day}T00:00");
    let to = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| format!("{}T00:00", (d + Duration::days(1)).format("%Y-%m-%d")))
        .unwrap_or_default();
    let mut out: Vec<&ResourceUse> = uses
        .iter()
        .filter(|item| {
            item.resource_uid == resource_uid && (except_uid.is_empty() || item.uid != except_uid)
        })
        .filter(|item| {
            item.occupies_at(now)
                .is_some_and(|(start, end)| start < to && from < end)
        })
        .collect();
    out.sort_by(|a, b| (&a.start, &a.uid).cmp(&(&b.start, &b.uid)));
    out
}

/// Joins a typed date (ISO or raw) and time into a moment; what cannot be
/// read stays visible so that saving refuses it.
fn join_moment(date: &str, time: &str) -> String {
    format!("{}T{}", date.trim(), time.trim())
}

/// The date and time fields of a use being edited, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct UseForm {
    start_date: String,
    start_time: String,
    end_date: String,
    end_time: String,
    returned_date: String,
    returned_time: String,
}

impl UseForm {
    fn from_use(item: &ResourceUse) -> Self {
        let returned = item.returned_at.as_deref().unwrap_or("");
        Self {
            start_date: moment_date(&item.start),
            start_time: moment_time(&item.start),
            end_date: moment_date(&item.end),
            end_time: moment_time(&item.end),
            returned_date: moment_date(returned),
            returned_time: moment_time(returned),
        }
    }

    /// Writes the typed times into the use.
    fn apply(&self, item: &mut ResourceUse) {
        item.start = join_moment(&self.start_date, &self.start_time);
        item.end = join_moment(&self.end_date, &self.end_time);
        if item.status == UseStatus::Returned {
            item.returned_at = Some(join_moment(&self.returned_date, &self.returned_time));
        }
    }
}

/// The message of the first thing wrong with the times, in words.
fn time_problem(item: &ResourceUse) -> Option<&'static str> {
    if parse_moment(item.start.trim()).is_err() {
        return Some("Informe a data e a hora de início, como 10/11/2026 14:30.");
    }
    if parse_moment(item.end.trim()).is_err() {
        return Some("Informe a data e a hora do fim previsto, como 10/11/2026 16:00.");
    }
    if item.status == UseStatus::Returned
        && item
            .returned_at
            .as_deref()
            .is_none_or(|at| parse_moment(at.trim()).is_err())
    {
        return Some("Informe a data e a hora da devolução, como 10/11/2026 16:00.");
    }
    None
}

// ---------------------------------------------------------------------------
// The panel

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Now,
    Calendar,
    Register,
}

/// What the library is read for in one frame, with the "now" of that frame.
struct Data {
    now: String,
    today: NaiveDate,
    resources: Vec<Resource>,
    uses: Vec<ResourceUse>,
}

impl Data {
    fn resource(&self, uid: &str) -> Option<&Resource> {
        self.resources.iter().find(|r| r.uid == uid)
    }

    fn name(&self, uid: &str) -> &str {
        self.resource(uid)
            .map(|r| r.name.as_str())
            .unwrap_or("Recurso não encontrado")
    }
}

/// The names of what the use in the ficha is linked to.
#[derive(Clone, Debug, Default)]
struct LinkNames {
    document: Option<String>,
    attendance: Option<String>,
    task: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickKind {
    Attendance,
    Task,
}

/// A search field with the matching atendimentos or tasks under it.
#[derive(Default)]
struct LinkPicker {
    query: String,
    searched: String,
    results: Vec<(i64, String)>,
}

impl LinkPicker {
    fn clear(&mut self) {
        *self = Self::default();
    }

    fn run(&mut self, store: &Store, kind: PickKind) {
        self.searched = self.query.clone();
        self.results.clear();
        let query = fold_text(self.query.trim());
        if query.chars().count() < 2 {
            return;
        }
        let words: Vec<&str> = query.split_whitespace().collect();
        let all = |haystack: &str| {
            let haystack = fold_text(haystack);
            words.iter().all(|word| haystack.contains(word))
        };
        match kind {
            PickKind::Attendance => {
                for att in store.list_saiop_attendances().unwrap_or_default() {
                    if all(&format!(
                        "{} {} {}",
                        att.reference_raw, att.subject, att.assignee
                    )) {
                        self.results.push((att.id, attendance_name(&att)));
                    }
                    if self.results.len() >= 6 {
                        break;
                    }
                }
            }
            PickKind::Task => {
                for task in store.list_tasks().unwrap_or_default() {
                    if all(&task.title) {
                        self.results.push((task.id, task.title.clone()));
                    }
                    if self.results.len() >= 6 {
                        break;
                    }
                }
            }
        }
    }

    /// Draws the field and the matches; returns the one chosen.
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        kind: PickKind,
        hint: &str,
    ) -> Option<(i64, String)> {
        let p = c();
        let width = ui.available_width();
        widgets::search(ui, &mut self.query, hint, width);
        if self.query != self.searched {
            self.run(store, kind);
        }
        let typed = self.query.trim().chars().count();
        if typed >= 2 && self.results.is_empty() {
            widgets::small_muted(ui, format!("Nada encontrado para “{}”.", self.query.trim()));
            return None;
        }
        let mut chosen = None;
        if !self.results.is_empty() {
            egui::Frame::NONE
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.outline))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width((width - 10.0).max(60.0));
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (id, label) in &self.results {
                        let row = ui
                            .with_layout(
                                egui::Layout::top_down_justified(egui::Align::LEFT),
                                |ui| {
                                    ui.add(
                                        egui::Button::selectable(false, label.as_str())
                                            .truncate()
                                            .min_size(egui::vec2(0.0, 26.0)),
                                    )
                                },
                            )
                            .inner
                            .on_hover_text(label.as_str());
                        if row.clicked() {
                            chosen = Some((*id, label.clone()));
                        }
                    }
                });
        }
        if chosen.is_some() {
            self.clear();
        }
        chosen
    }
}

/// "Referência — assunto" of an atendimento, for naming it in a link.
fn attendance_name(att: &crate::saiop::SaiopAttendance) -> String {
    let reference = att.reference_raw.trim();
    let subject = att.subject.trim();
    match (reference.is_empty(), subject.is_empty()) {
        (false, false) => format!("{reference} — {subject}"),
        (false, true) => reference.to_owned(),
        (true, false) => subject.to_owned(),
        (true, true) => "Atendimento sem referência".to_owned(),
    }
}

/// What the footer of the ficha of a use asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UseAction {
    Save,
    Reserve,
    StartNow,
    CancelReservation,
    Reopen,
    BeginReturn,
    ConfirmReturn,
    EndReturn,
    Extend,
    Revert,
    Close,
    Delete,
    CreateTask,
}

#[derive(Default)]
pub struct ResourcesPanel {
    view: View,
    filters: Filters,
    filters_open: bool,
    show_old: bool,
    cal_resource: String,
    cal_month: Option<(i32, u32)>,
    cal_day: String,
    /// The next frame scrolls the list of the chosen day into view.
    cal_scroll: bool,
    reg_query: String,
    hide_inactive: bool,
    // The ficha of a use.
    use_editor: Option<ResourceUse>,
    use_saved: Option<ResourceUse>,
    use_version: Option<i64>,
    form: UseForm,
    /// "Registrar devolução" was clicked: the time of the return is being
    /// confirmed.
    returning: bool,
    use_error: String,
    focus_end: bool,
    focus_first: bool,
    confirm_delete_use: bool,
    doc_picker: DocumentPicker,
    attendance_picker: LinkPicker,
    task_picker: LinkPicker,
    link_names: LinkNames,
    // The ficha of a resource.
    res_editor: Option<Resource>,
    res_saved: Option<Resource>,
    res_version: Option<i64>,
    res_error: String,
    confirm_delete_res: bool,
    // Messages and requests to the app.
    message: String,
    error: bool,
    toast: Option<String>,
    changed: bool,
    ask: Option<Ask>,
    /// A use chosen in the list of a resource, to open once that ficha allows.
    pending_open_use: Option<String>,
    focus_search: bool,
    export_wanted: bool,
    #[cfg(feature = "shots")]
    shots_empty: bool,
    #[cfg(feature = "shots")]
    shots_scroll_links: bool,
}

impl ResourcesPanel {
    // -- what the app asks ---------------------------------------------------

    /// A use or a resource has changes that were not saved.
    pub fn dirty(&self) -> bool {
        self.use_editor != self.use_saved || self.res_editor != self.res_saved
    }

    /// Ctrl+F: the next frame puts the keyboard in the search box.
    pub fn key_find(&mut self) {
        if self.view == View::Calendar {
            self.view = View::Now;
        }
        self.focus_search = true;
    }

    /// Ctrl+N: what the primary button of the view does. True when handled.
    pub fn key_new(&mut self, store: &Store) -> bool {
        if self.view == View::Register {
            self.new_resource();
        } else if let Err(why) = self.new_use(store, None, None, None) {
            self.set_notice(why, true);
        }
        true
    }

    /// Ctrl+S: what "Salvar" does, with a ficha open and unsaved. True when
    /// a save was attempted.
    pub fn key_save(&mut self, store: &mut Store) -> bool {
        if self.use_editor.is_some() {
            let unsaved =
                self.use_editor.as_ref().is_some_and(|e| e.uid.is_empty()) || self.dirty();
            if !unsaved {
                return false;
            }
            let now = now_moment();
            self.save_use_as(store, &now, None);
            return true;
        }
        if self.res_editor.is_some() {
            let unsaved =
                self.res_editor.as_ref().is_some_and(|e| e.uid.is_empty()) || self.dirty();
            if !unsaved {
                return false;
            }
            self.save_resource(store, None);
            return true;
        }
        false
    }

    /// Esc: closes the ficha when it has no unsaved changes. True when
    /// something is open (even when it could not be closed).
    pub fn key_escape(&mut self) -> bool {
        if self.use_editor.is_some() || self.res_editor.is_some() {
            if !self.dirty() {
                self.close_ficha();
            }
            return true;
        }
        false
    }

    /// What the person asked to open from a link, once.
    pub fn take_ask(&mut self) -> Option<Ask> {
        self.ask.take()
    }

    /// A message for the app's floating notice, once.
    pub fn take_message(&mut self) -> Option<String> {
        self.toast.take()
    }

    /// The uid of the saved use open in the ficha, if any (for the way back
    /// from a document opened out of it).
    pub fn open_use_uid(&self) -> Option<String> {
        self.use_editor
            .as_ref()
            .map(|e| e.uid.clone())
            .filter(|uid| !uid.is_empty())
    }

    /// Something was saved or deleted: the app refreshes its counters.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Opens a use in the ficha, unless another edit is pending.
    pub fn focus_use(&mut self, store: &Store, uid: &str) -> Result<(), String> {
        let open = self.use_editor.as_ref().is_some_and(|e| e.uid == uid);
        if self.dirty() {
            return if open {
                Ok(())
            } else {
                Err(BLOCKED.to_owned())
            };
        }
        let list = store.list_resource_uses().map_err(|e| e.to_string())?;
        let item = list
            .into_iter()
            .find(|u| u.uid == uid)
            .ok_or_else(|| "Esse uso não foi encontrado.".to_owned())?;
        self.view = View::Now;
        self.load_use(store, item);
        Ok(())
    }

    /// Opens a resource in the ficha, unless another edit is pending.
    pub fn focus_resource(&mut self, store: &Store, uid: &str) -> Result<(), String> {
        let open = self.res_editor.as_ref().is_some_and(|e| e.uid == uid);
        if self.dirty() {
            return if open {
                Ok(())
            } else {
                Err(BLOCKED.to_owned())
            };
        }
        let list = store.list_resources().map_err(|e| e.to_string())?;
        let item = list
            .into_iter()
            .find(|r| r.uid == uid)
            .ok_or_else(|| "Esse recurso não foi encontrado.".to_owned())?;
        self.view = View::Register;
        self.load_resource(store, item);
        Ok(())
    }

    /// Opens a new use already linked to a document, an atendimento or a
    /// task (the "Reservar recurso…" buttons of the other screens).
    pub fn new_use_linked(&mut self, store: &Store, link: UseLink) -> Result<(), String> {
        self.new_use(store, None, None, Some(link))
    }

    // -- opening and closing -------------------------------------------------

    fn set_notice(&mut self, text: impl Into<String>, error: bool) {
        self.message = text.into();
        self.error = error;
    }

    fn close_ficha(&mut self) {
        self.use_editor = None;
        self.use_saved = None;
        self.res_editor = None;
        self.res_saved = None;
        self.returning = false;
        self.use_error.clear();
        self.res_error.clear();
        self.confirm_delete_use = false;
        self.confirm_delete_res = false;
        self.doc_picker.clear();
        self.attendance_picker.clear();
        self.task_picker.clear();
    }

    fn load_use(&mut self, store: &Store, item: ResourceUse) {
        self.close_ficha();
        self.use_version = if item.uid.is_empty() {
            None
        } else {
            store.resource_use_version(&item.uid).ok()
        };
        self.form = UseForm::from_use(&item);
        self.refresh_link_names(store, &item);
        self.use_saved = Some(item.clone());
        self.use_editor = Some(item);
        self.focus_first = true;
    }

    fn load_resource(&mut self, store: &Store, item: Resource) {
        self.close_ficha();
        self.res_version = if item.uid.is_empty() {
            None
        } else {
            store.resource_version(&item.uid).ok()
        };
        self.res_saved = Some(item.clone());
        self.res_editor = Some(item);
        self.focus_first = true;
    }

    fn refresh_link_names(&mut self, store: &Store, item: &ResourceUse) {
        self.link_names = LinkNames {
            document: item.document_id.map(|id| {
                store
                    .list_documents("")
                    .ok()
                    .and_then(|docs| docs.into_iter().find(|d| d.id == id))
                    .map(|d| d.document_number)
                    .unwrap_or_else(|| "Documento não encontrado".to_owned())
            }),
            attendance: item.saiop_id.map(|id| {
                store
                    .list_saiop_attendances()
                    .ok()
                    .and_then(|list| list.into_iter().find(|a| a.id == id))
                    .map(|a| attendance_name(&a))
                    .unwrap_or_else(|| "Atendimento não encontrado".to_owned())
            }),
            task: item.task_id.map(|id| {
                store
                    .list_tasks()
                    .ok()
                    .and_then(|list| list.into_iter().find(|t| t.id == id))
                    .map(|t| t.title)
                    .unwrap_or_else(|| "Tarefa não encontrada".to_owned())
            }),
        };
    }

    /// A blank use. `resource_uid` and `day` (ISO) are proposals, `link`
    /// ties it to something.
    fn new_use(
        &mut self,
        store: &Store,
        resource_uid: Option<&str>,
        day: Option<&str>,
        link: Option<UseLink>,
    ) -> Result<(), String> {
        if self.dirty() {
            return Err(BLOCKED.to_owned());
        }
        let now = now_moment();
        let (start, end) = match day {
            Some(day) => times_for_day(day, &now),
            None => default_times(&now),
        };
        let mut item = ResourceUse {
            start,
            end,
            ..Default::default()
        };
        let active: Vec<Resource> = store
            .list_resources()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|r| r.status == ResourceStatus::Active)
            .collect();
        item.resource_uid = match resource_uid {
            Some(uid) => uid.to_owned(),
            None if active.len() == 1 => active[0].uid.clone(),
            None => String::new(),
        };
        match link {
            Some(UseLink::Document(id)) => item.document_id = Some(id),
            Some(UseLink::Attendance(id)) => item.saiop_id = Some(id),
            Some(UseLink::Task(id)) => item.task_id = Some(id),
            None => {}
        }
        if self.view == View::Register {
            self.view = View::Now;
        }
        self.load_use(store, item);
        Ok(())
    }

    fn new_resource(&mut self) {
        if self.dirty() {
            self.set_notice(BLOCKED, true);
            return;
        }
        self.view = View::Register;
        self.close_ficha();
        let fresh = Resource {
            kind: RESOURCE_KINDS[0].to_owned(),
            ..Default::default()
        };
        self.res_saved = Some(fresh.clone());
        self.res_editor = Some(fresh);
        self.res_version = None;
        self.focus_first = true;
    }

    fn open_use(&mut self, store: &Store, data: &Data, uid: &str) {
        if self.dirty() {
            if !self.use_editor.as_ref().is_some_and(|e| e.uid == uid) {
                self.set_notice(BLOCKED, true);
            }
            return;
        }
        if let Some(item) = data.uses.iter().find(|u| u.uid == uid) {
            self.load_use(store, item.clone());
        }
    }

    fn open_resource(&mut self, store: &Store, data: &Data, uid: &str) {
        if self.dirty() {
            if !self.res_editor.as_ref().is_some_and(|e| e.uid == uid) {
                self.set_notice(BLOCKED, true);
            }
            return;
        }
        if let Some(item) = data.resources.iter().find(|r| r.uid == uid) {
            self.load_resource(store, item.clone());
        }
    }

    // -- the area -------------------------------------------------------------

    /// Draws the area. Returns what the person asked to open from a link.
    pub fn show(&mut self, ui: &mut egui::Ui, store: &mut Store) -> Option<Ask> {
        // The one clock reading of the frame.
        let now = now_moment();
        let today = parse_moment(&now)
            .map(|m| m.date())
            .unwrap_or_else(|_| chrono::Local::now().date_naive());
        let (resources, uses) = match (store.list_resources(), store.list_resource_uses()) {
            (Ok(resources), Ok(uses)) => (resources, uses),
            (Err(e), _) | (_, Err(e)) => {
                notice_line(
                    ui,
                    &format!("Não foi possível carregar os recursos: {e}"),
                    Tone::Danger,
                );
                return None;
            }
        };
        #[cfg(feature = "shots")]
        let (resources, uses) = if self.shots_empty {
            (Vec::new(), Vec::new())
        } else {
            (resources, uses)
        };
        let data = Data {
            now,
            today,
            resources,
            uses,
        };
        let counts = day_counts(&data.uses, data.today, &data.now);
        let in_use = data
            .uses
            .iter()
            .filter(|u| u.status == UseStatus::InUse)
            .count();

        // The ficha is a panel on the right; the rest is drawn after it.
        if self.use_editor.is_some() {
            self.use_ficha(ui, store, &data);
        } else if self.res_editor.is_some() {
            self.resource_ficha(ui, store, &data);
        }
        let room = ui.available_rect_before_wrap();
        let room = egui::Rect::from_min_max(
            room.min + egui::vec2(20.0, 14.0),
            room.max - egui::vec2(20.0, 0.0),
        );
        let mut content = ui.new_child(egui::UiBuilder::new().max_rect(room));
        // Whatever is too wide for the room is cut, never drawn over the ficha.
        content.set_clip_rect(
            room.expand2(egui::vec2(20.0, 0.0))
                .intersect(ui.clip_rect()),
        );
        let ui = &mut content;
        ui.spacing_mut().item_spacing.y = S2;

        // ---- Area header --------------------------------------------------
        let mut shown_for_export: Vec<ResourceUse> = Vec::new();
        // The counts sit beside the name when there is room, under it when not.
        let inline_pills = ui.available_width() >= 700.0;
        let pills = |ui: &mut egui::Ui| {
            if in_use > 0 {
                widgets::pill(ui, format!("{in_use} em uso"), Tone::Violet);
            }
            if counts.reservations_today > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(
                        counts.reservations_today,
                        "reserva hoje",
                        "reservas hoje",
                    ),
                    Tone::Warn,
                );
            }
            if counts.overdue > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(
                        counts.overdue,
                        "devolução atrasada",
                        "devoluções atrasadas",
                    ),
                    Tone::Danger,
                );
            }
        };
        ui.horizontal(|ui| {
            widgets::heading(ui, "Recursos");
            crate::help_ui::help(
                ui,
                format!(
                    "Salas, livros e equipamentos do gabinete: {} e {}. Cada reserva ou empréstimo é um uso, com início e fim previsto.",
                    crate::labels::count(data.resources.len(), "recurso", "recursos"),
                    crate::labels::count(data.uses.len(), "uso", "usos"),
                ),
            );
            if inline_pills {
                ui.add_space(S2);
                pills(ui);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.view == View::Register {
                    if ui
                        .add(widgets::primary(format!("{} Novo recurso", icon::PLUS)))
                        .on_hover_text("Cadastra uma sala, um equipamento, um livro ou outro recurso. (Ctrl+N)")
                        .clicked()
                    {
                        self.new_resource();
                    }
                } else if ui
                    .add(widgets::primary(format!("{} Reservar", icon::PLUS)))
                    .on_hover_text("Marca o uso de um recurso, agora ou para depois. (Ctrl+N)")
                    .clicked()
                {
                    if let Err(why) = self.new_use(store, None, None, None) {
                        self.set_notice(why, true);
                    }
                }
                if self.view == View::Now {
                    let (button, _) = egui::containers::menu::MenuButton::from_button(
                        widgets::secondary(icon::MORE),
                    )
                    .ui(ui, |ui| {
                        ui.set_min_width(190.0);
                        if ui
                            .add(egui::Button::new(format!(
                                "{} Exportar esta lista…",
                                icon::EXPORT
                            )))
                            .on_hover_text("Grava num arquivo CSV os usos que a lista mostra agora, com os filtros escolhidos.")
                            .clicked()
                        {
                            self.export_wanted = true;
                            ui.close();
                        }
                    });
                    button.on_hover_text("Mais ações");
                }
            });
        });

        if !inline_pills {
            ui.horizontal_wrapped(|ui| pills(ui));
        }

        if !self.message.is_empty() {
            let tone = if self.error { Tone::Danger } else { Tone::Info };
            if notice_line(ui, &self.message.clone(), tone) {
                self.message.clear();
            }
        }

        // ---- Tabs ------------------------------------------------------------
        let mut picked = self.view;
        widgets::tab_row(ui, |ui| {
            for (view, label) in [
                (View::Now, "Agora e próximos"),
                (View::Calendar, "Calendário"),
                (View::Register, "Recursos"),
            ] {
                if widgets::tab(ui, self.view == view, label, false).clicked() {
                    picked = view;
                }
            }
        });
        self.view = picked;

        let mut open_use: Option<String> = None;
        let mut open_resource: Option<String> = None;
        let avail = ui.available_height();
        match self.view {
            View::Now => self.view_now(ui, store, &data, &mut open_use, &mut shown_for_export),
            View::Calendar => self.view_calendar(ui, store, &data, avail, &mut open_use),
            View::Register => self.view_register(ui, store, &data, &mut open_resource),
        }
        if let Some(uid) = open_use {
            self.open_use(store, &data, &uid);
        }
        if let Some(uid) = open_resource {
            self.open_resource(store, &data, &uid);
        }
        if let Some(uid) = self.pending_open_use.take() {
            self.open_use(store, &data, &uid);
        }
        if std::mem::take(&mut self.export_wanted) {
            self.export_list(store, &data, &shown_for_export);
        }
        self.ask.take()
    }

    /// "Exportar esta lista…": the uses on screen, as the CSV of the
    /// exported data, to a file the person chooses.
    fn export_list(&mut self, store: &Store, data: &Data, shown: &[ResourceUse]) {
        if shown.is_empty() {
            self.set_notice("A lista está vazia: não há usos para exportar.", true);
            return;
        }
        let Some(path) = crate::windows_ui::save_file(
            "Exportar usos de recursos",
            "usos-de-recursos.csv",
            &[("Planilha CSV", "*.csv")],
        ) else {
            return;
        };
        let text = export_text(store, data, shown);
        match std::fs::write(&path, text) {
            Ok(()) => self.set_notice(
                format!(
                    "{} para {}.",
                    crate::labels::count(shown.len(), "uso exportado", "usos exportados"),
                    path.display()
                ),
                false,
            ),
            Err(e) => self.set_notice(format!("Não foi possível gravar o arquivo: {e}"), true),
        }
    }
}

/// The CSV of the uses on screen: the file of the exported data, with the
/// number of each linked document and the reference of each atendimento.
fn export_text(store: &Store, data: &Data, shown: &[ResourceUse]) -> String {
    let documents: BTreeMap<i64, String> = store
        .list_documents("")
        .unwrap_or_default()
        .into_iter()
        .map(|d| (d.id, d.document_number))
        .collect();
    let attendances: BTreeMap<i64, String> = store
        .list_saiop_attendances()
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.id, a.reference_raw.trim().to_owned()))
        .collect();
    crate::export::resource_uses_csv(&data.resources, shown, &documents, &attendances, &data.now)
}

/// Said when another ficha has unsaved changes.
const BLOCKED: &str =
    "Salve ou descarte as alterações do recurso ou do uso que está aberto antes de abrir outro.";

/// A message of the screen: one soft-coloured line with a close button.
/// Returns true when it was closed.
fn notice_line(ui: &mut egui::Ui, text: &str, tone: Tone) -> bool {
    let (fg, bg) = tone.colors();
    let mut closed = false;
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let glyph = if tone == Tone::Danger {
                    icon::ERROR
                } else {
                    icon::INFO
                };
                ui.label(RichText::new(glyph).size(16.0).color(fg));
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - 36.0).max(80.0));
                    ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(ui, icon::CLOSE, "Fechar").clicked() {
                        closed = true;
                    }
                });
            });
        });
    closed
}

// ---------------------------------------------------------------------------
// Views

/// The little marks of what a use is linked to.
fn link_marks(ui: &mut egui::Ui, item: &ResourceUse) {
    let p = c();
    for (on, glyph, hint) in [
        (item.task_id.is_some(), icon::TASKS, "Ligado a uma tarefa"),
        (
            item.saiop_id.is_some(),
            icon::ATTENDANCES,
            "Ligado a um atendimento",
        ),
        (
            item.document_id.is_some(),
            icon::DOCUMENT,
            "Ligado a um documento",
        ),
    ] {
        if on {
            ui.label(RichText::new(glyph).size(15.0).color(p.muted))
                .on_hover_text(hint);
        }
    }
}

/// One use as a row of a list: the resource, who has it and when, the
/// situation and the links. True when it was clicked.
fn use_row(ui: &mut egui::Ui, item: &ResourceUse, data: &Data, selected: bool) -> bool {
    let (label, tone) = status_pill(item, &data.now);
    let name = data.name(&item.resource_uid);
    let kind = data
        .resource(&item.resource_uid)
        .map(|r| kind_label(&r.kind))
        .unwrap_or("");
    let links = usize::from(item.document_id.is_some())
        + usize::from(item.saiop_id.is_some())
        + usize::from(item.task_id.is_some());
    pick_row(ui, ("use-row", item.uid.as_str()), selected, 14, |ui| {
        ui.horizontal(|ui| {
            let right = 84.0 + 21.0 * links as f32 + if item.quantity > 1 { 34.0 } else { 0.0 };
            let kind_w = if kind.is_empty() { 0.0 } else { 90.0 };
            ui.scope(|ui| {
                ui.set_max_width((ui.available_width() - right - kind_w).max(60.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(name).font(theme::font_strong(theme::SIZE_BODY)),
                    )
                    .truncate(),
                );
            });
            if !kind.is_empty() {
                widgets::pill(ui, kind, Tone::Neutral);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::pill(ui, label, tone);
                link_marks(ui, item);
                if item.quantity > 1 {
                    widgets::small_muted(ui, format!("× {}", item.quantity))
                        .on_hover_text(format!("Quantidade: {}", item.quantity));
                }
            });
        });
        let who = item.who.trim();
        let sector = item.sector.trim();
        let person = match (who.is_empty(), sector.is_empty()) {
            (false, false) => format!("{who} — {sector}"),
            (false, true) => who.to_owned(),
            (true, false) => sector.to_owned(),
            (true, true) => String::new(),
        };
        let mut line = period_text(&item.start, &item.end);
        if !person.is_empty() {
            line = format!("{person}  ·  {line}");
        }
        ui.add(
            egui::Label::new(RichText::new(line).size(theme::SIZE_SMALL).color(c().muted))
                .truncate(),
        );
    })
    .clicked()
}

/// A list of rows inside a white box with an outline.
fn list_box<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(c().surface)
        .stroke(egui::Stroke::new(1.0, c().outline))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CARD))
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

impl ResourcesPanel {
    fn clear_filters(&mut self) {
        self.filters = Filters::default();
    }

    /// View 1: what is late, in use, today and next.
    fn view_now(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        data: &Data,
        open: &mut Option<String>,
        shown_for_export: &mut Vec<ResourceUse>,
    ) {
        let advanced = !self.filters.kind.is_empty()
            || self.filters.status.is_some()
            || !self.filters.from.is_empty()
            || !self.filters.to.is_empty()
            || !self.filters.resource.is_empty();
        ui.add_space(S1);
        ui.horizontal_wrapped(|ui| {
            let clearable = self.filters.active() || self.show_old;
            let reserve = 150.0 + 250.0 + if clearable { 70.0 } else { 0.0 };
            let width = (ui.available_width() - reserve).max(160.0);
            let search = widgets::search(
                ui,
                &mut self.filters.query,
                "Buscar recurso, quem, setor ou finalidade",
                width,
            );
            if std::mem::take(&mut self.focus_search) {
                search.request_focus();
            }
            if widgets::chip(
                ui,
                self.filters_open || advanced,
                format!(
                    "{} Filtros {}",
                    icon::FILTER,
                    if self.filters_open { icon::UP } else { icon::DOWN }
                ),
            )
            .clicked()
            {
                self.filters_open = !self.filters_open;
            }
            if widgets::chip(ui, self.show_old, "Ver mais antigos / encerrados")
                .on_hover_text("Inclui os usos já devolvidos, os cancelados e as reservas que passaram sem uso.")
                .clicked()
            {
                self.show_old = !self.show_old;
            }
            if clearable && ui.add(widgets::quiet("Limpar")).clicked() {
                self.clear_filters();
                self.show_old = false;
            }
        });
        if self.filters_open || advanced {
            widgets::card(ui, |ui| {
                ui.columns(3, |cols| {
                    labeled(&mut cols[0], "Tipo", |ui| {
                        let text = if self.filters.kind.is_empty() {
                            "Todos".to_owned()
                        } else {
                            kind_label(&self.filters.kind).to_owned()
                        };
                        pick(ui, "res-filter-kind", text, |ui| {
                            ui.selectable_value(&mut self.filters.kind, String::new(), "Todos");
                            for kind in RESOURCE_KINDS {
                                ui.selectable_value(
                                    &mut self.filters.kind,
                                    kind.to_owned(),
                                    kind_label(kind),
                                );
                            }
                        });
                    });
                    labeled(&mut cols[1], "Situação", |ui| {
                        let text = self
                            .filters
                            .status
                            .map(|s| s.label().to_owned())
                            .unwrap_or_else(|| "Todas".to_owned());
                        pick(ui, "res-filter-status", text, |ui| {
                            ui.selectable_value(&mut self.filters.status, None, "Todas");
                            for status in [
                                UseStatus::Reserved,
                                UseStatus::InUse,
                                UseStatus::Returned,
                                UseStatus::Cancelled,
                            ] {
                                ui.selectable_value(
                                    &mut self.filters.status,
                                    Some(status),
                                    status.label(),
                                );
                            }
                        });
                    });
                    labeled(&mut cols[2], "Recurso", |ui| {
                        let text = data
                            .resource(&self.filters.resource)
                            .map(|r| r.name.clone())
                            .unwrap_or_else(|| "Todos".to_owned());
                        pick(ui, "res-filter-resource", text, |ui| {
                            ui.selectable_value(&mut self.filters.resource, String::new(), "Todos");
                            for r in &data.resources {
                                ui.selectable_value(
                                    &mut self.filters.resource,
                                    r.uid.clone(),
                                    r.name.as_str(),
                                );
                            }
                        });
                    });
                });
                ui.add_space(S2);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = S4;
                    labeled(ui, "Período: de", |ui| {
                        crate::labels::date_field(ui, "res-from", &mut self.filters.from);
                    });
                    labeled(ui, "até", |ui| {
                        crate::labels::date_field(ui, "res-to", &mut self.filters.to);
                    });
                });
            });
        }

        let selected_items: Vec<&ResourceUse> = data
            .uses
            .iter()
            .filter(|u| use_matches(u, data.resource(&u.resource_uid), &self.filters))
            .collect();
        let groups = group_uses(&selected_items, data.today, &data.now);
        let closed_asked = matches!(
            self.filters.status,
            Some(UseStatus::Returned | UseStatus::Cancelled)
        );
        let with_old = self.show_old || closed_asked;
        // What the list shows is what "Exportar esta lista…" writes.
        for group in [
            &groups.overdue,
            &groups.in_use,
            &groups.today,
            &groups.next_days,
            &groups.later,
        ] {
            shown_for_export.extend(group.iter().map(|u| (*u).clone()));
        }
        if with_old {
            shown_for_export.extend(groups.older.iter().map(|u| (*u).clone()));
        }

        let open_uid = self.use_editor.as_ref().map(|e| e.uid.clone());
        let mut any = false;
        egui::ScrollArea::vertical()
            .id_salt("res-now-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let p = c();
                ui.spacing_mut().item_spacing.y = S2;
                let sections: [(&str, &Vec<&ResourceUse>, Option<egui::Color32>); 6] = [
                    ("Atrasados", &groups.overdue, Some(p.danger)),
                    ("Em uso agora", &groups.in_use, None),
                    ("Hoje", &groups.today, None),
                    ("Próximos dias", &groups.next_days, None),
                    ("Mais adiante", &groups.later, None),
                    (
                        "Mais antigos e encerrados",
                        if with_old { &groups.older } else { &EMPTY },
                        None,
                    ),
                ];
                for (title, items, color) in sections {
                    if items.is_empty() {
                        continue;
                    }
                    any = true;
                    ui.add_space(S1);
                    widgets::overline(ui, &format!("{title} · {}", items.len()), color);
                    list_box(ui, |ui| {
                        for item in items {
                            let is_open = open_uid.as_deref() == Some(item.uid.as_str());
                            if use_row(ui, item, data, is_open) {
                                *open = Some(item.uid.clone());
                            }
                        }
                    });
                }
                if !any {
                    self.empty_now(ui, store, data, &groups);
                }
                ui.add_space(S5_GAP);
            });
    }

    /// Nothing to list: say why, and the way out.
    fn empty_now(&mut self, ui: &mut egui::Ui, store: &Store, data: &Data, groups: &Groups<'_>) {
        ui.add_space(S3);
        let query = self.filters.query.trim().to_owned();
        let named = self.filters.named(&data.resources);
        if data.resources.is_empty() {
            widgets::muted(
                ui,
                "Ainda não há recursos. Cadastre uma sala, um equipamento ou um livro para poder reservar.",
            );
            if ui.add(widgets::secondary("Cadastrar um recurso")).clicked() {
                self.new_resource();
            }
        } else if data.uses.is_empty() {
            widgets::muted(
                ui,
                "Ainda não há usos. Use “Reservar” para marcar o primeiro.",
            );
            if ui.add(widgets::secondary("Reservar")).clicked() {
                if let Err(why) = self.new_use(store, None, None, None) {
                    self.set_notice(why, true);
                }
            }
        } else if self.filters.active() {
            let mut active = Vec::new();
            if !query.is_empty() {
                active.push(format!("busca “{query}”"));
            }
            active.extend(named);
            widgets::muted(ui, format!("Nenhum uso com {}.", active.join(", ")));
            if ui.add(widgets::quiet("Limpar filtros")).clicked() {
                self.clear_filters();
            }
        } else if !groups.older.is_empty() {
            widgets::muted(
                ui,
                "Nada em uso nem reservado. Os usos encerrados aparecem em “Ver mais antigos / encerrados”.",
            );
            if ui
                .add(widgets::quiet("Ver mais antigos / encerrados"))
                .clicked()
            {
                self.show_old = true;
            }
        } else {
            widgets::muted(ui, "Nada em uso nem reservado.");
        }
    }

    // -- calendar --------------------------------------------------------------

    /// View 2: the month, all the resources or one.
    fn view_calendar(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        data: &Data,
        avail: f32,
        open: &mut Option<String>,
    ) {
        use crate::app::calendar::{
            grid_row_h, line_rect, lines_that_fit, long_date, month_grid, month_nav, month_weeks,
            paint_line,
        };
        use chrono::Datelike;
        let p = c();
        ui.add_space(S1);
        let (mut year, mut month) = self
            .cal_month
            .unwrap_or((data.today.year(), data.today.month()));
        ui.horizontal(|ui| {
            let text = data
                .resource(&self.cal_resource)
                .map(|r| r.name.clone())
                .unwrap_or_else(|| "Todos os recursos".to_owned());
            ui.scope(|ui| {
                ui.set_width(250.0);
                pick(ui, "res-cal-resource", text, |ui| {
                    ui.selectable_value(&mut self.cal_resource, String::new(), "Todos os recursos");
                    for r in &data.resources {
                        ui.selectable_value(&mut self.cal_resource, r.uid.clone(), r.name.as_str());
                    }
                });
            });
            ui.add_space(S3);
            let shown = (year, month);
            (year, month) = month_nav(ui, shown, data.today);
            if (year, month) != shown {
                self.cal_day.clear();
            }
        });
        self.cal_month = Some((year, month));
        ui.add_space(S1);

        let weeks = month_weeks(year, month);
        let (Some(first), Some(last)) = (weeks.first().map(|w| w[0]), weeks.last().map(|w| w[6]))
        else {
            return;
        };
        let by_day = uses_by_day(&data.uses, &self.cal_resource, first, last);
        let one_resource = !self.cal_resource.is_empty();
        let row_h = grid_row_h(avail - 40.0, weeks.len());
        let base = ui.make_persistent_id("resources-calendar");
        let selected = self.cal_day.clone();
        let scroll_to_day = std::mem::take(&mut self.cal_scroll);
        let mut chosen_use: Option<String> = None;
        let open_uid = self.use_editor.as_ref().map(|e| e.uid.clone());
        let (new_day, reserve_day) = egui::ScrollArea::vertical()
            .id_salt("res-calendar-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let chosen = month_grid(
                    ui,
                    base,
                    (year, month),
                    data.today,
                    &selected,
                    row_h,
                    |_| true,
                    |ui, painter, cell| {
                        let items: &[&ResourceUse] = if cell.in_month {
                            by_day.get(cell.iso).map(Vec::as_slice).unwrap_or(&[])
                        } else {
                            &[]
                        };
                        let mut more_clicked = false;
                        let lines = lines_that_fit(cell.row_h);
                        let shown = if items.len() > lines {
                            lines.saturating_sub(1)
                        } else {
                            items.len()
                        };
                        for (i, item) in items.iter().take(shown).enumerate() {
                            let line = line_rect(cell.rect, cell.col_w, i);
                            let (_, tone) = status_pill(item, &data.now);
                            let (fg, bg) = tone.colors();
                            painter.rect_filled(line, egui::CornerRadius::same(3), bg);
                            let when = if moment_date(&item.start) == cell.iso {
                                moment_time(&item.start)
                            } else {
                                "…".to_owned()
                            };
                            let what = if one_resource {
                                item.who.trim().to_owned()
                            } else {
                                data.name(&item.resource_uid).to_owned()
                            };
                            paint_line(painter, line, &format!("{when} {what}"), fg);
                            let response = ui
                                .interact(
                                    line,
                                    base.with(("cal-item", cell.iso, item.uid.as_str())),
                                    egui::Sense::click(),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(use_line_hover(item, data));
                            if response.clicked() {
                                chosen_use = Some(item.uid.clone());
                            }
                        }
                        if items.len() > shown {
                            let line = line_rect(cell.rect, cell.col_w, shown);
                            let rest = &items[shown..];
                            paint_line(painter, line, &format!("+{}", rest.len()), p.muted);
                            let list = rest
                                .iter()
                                .map(|item| use_line_hover(item, data))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let response = ui
                                .interact(
                                    line,
                                    base.with(("cal-more", cell.iso)),
                                    egui::Sense::click(),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(list);
                            if response.clicked() {
                                more_clicked = true;
                            }
                        }
                        more_clicked
                    },
                );
                // The day clicked opens under the grid; clicking it again closes it.
                let chosen_now = chosen.is_some();
                let mut day = selected.clone();
                if let Some(iso) = chosen {
                    day = if selected == iso { String::new() } else { iso };
                }
                let mut reserve_day = None;
                if let Ok(date) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") {
                    ui.add_space(S3);
                    let mut close = false;
                    let header = ui.horizontal(|ui| {
                        widgets::overline(ui, &long_date(date), None);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            close = ui.add(widgets::quiet("Fechar")).clicked();
                            if ui
                                .add(widgets::secondary("Reservar neste dia"))
                                .on_hover_text("Abre um uso novo com o início neste dia.")
                                .clicked()
                            {
                                reserve_day = Some(day.clone());
                            }
                        });
                    });
                    if scroll_to_day || chosen_now {
                        header.response.scroll_to_me(Some(egui::Align::TOP));
                    }
                    ui.add_space(S1);
                    let items = by_day.get(&day).map(Vec::as_slice).unwrap_or(&[]);
                    if items.is_empty() {
                        widgets::muted(ui, "Nenhum uso neste dia.");
                    } else {
                        list_box(ui, |ui| {
                            for item in items {
                                let is_open = open_uid.as_deref() == Some(item.uid.as_str());
                                if use_row(ui, item, data, is_open) {
                                    chosen_use = Some(item.uid.clone());
                                }
                            }
                        });
                    }
                    ui.add_space(S4);
                    if close {
                        day.clear();
                    }
                }
                (day, reserve_day)
            })
            .inner;
        self.cal_day = new_day;
        if let Some(day) = reserve_day {
            let resource = (!self.cal_resource.is_empty()).then(|| self.cal_resource.clone());
            if let Err(why) = self.new_use(store, resource.as_deref(), Some(&day), None) {
                self.set_notice(why, true);
            }
        }
        if chosen_use.is_some() {
            *open = chosen_use;
        }
    }

    // -- register ---------------------------------------------------------------

    /// View 3: the register of resources.
    fn view_register(
        &mut self,
        ui: &mut egui::Ui,
        _store: &Store,
        data: &Data,
        open: &mut Option<String>,
    ) {
        ui.add_space(S1);
        ui.horizontal_wrapped(|ui| {
            let width = (ui.available_width() - 190.0).max(160.0);
            let search = widgets::search(
                ui,
                &mut self.reg_query,
                "Buscar por nome, tipo, local ou identificação",
                width,
            );
            if std::mem::take(&mut self.focus_search) {
                search.request_focus();
            }
            if widgets::chip(ui, !self.hide_inactive, "Mostrar fora de uso").clicked() {
                self.hide_inactive = !self.hide_inactive;
            }
        });
        let query = fold_text(self.reg_query.trim());
        let items: Vec<&Resource> = data
            .resources
            .iter()
            .filter(|r| !(self.hide_inactive && r.status == ResourceStatus::Inactive))
            .filter(|r| {
                query.is_empty()
                    || query.split_whitespace().all(|word| {
                        fold_text(&format!(
                            "{} {} {} {}",
                            r.name,
                            kind_label(&r.kind),
                            r.location,
                            r.identification
                        ))
                        .contains(word)
                    })
            })
            .collect();
        let open_uid = self.res_editor.as_ref().map(|e| e.uid.clone());
        egui::ScrollArea::vertical()
            .id_salt("res-register-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(S1);
                if items.is_empty() {
                    ui.add_space(S3);
                    if data.resources.is_empty() {
                        widgets::muted(
                            ui,
                            "Ainda não há recursos. Cadastre uma sala, um equipamento ou um livro com “Novo recurso”.",
                        );
                        if ui.add(widgets::secondary("Novo recurso")).clicked() {
                            self.new_resource();
                        }
                    } else {
                        widgets::muted(ui, "Nenhum recurso encontrado.");
                        if ui.add(widgets::quiet("Limpar a busca")).clicked() {
                            self.reg_query.clear();
                            self.hide_inactive = false;
                        }
                    }
                    return;
                }
                list_box(ui, |ui| {
                    for r in items {
                        let is_open = open_uid.as_deref() == Some(r.uid.as_str());
                        if resource_row(ui, r, data, is_open) {
                            *open = Some(r.uid.clone());
                        }
                    }
                });
                ui.add_space(S5_GAP);
            });
    }
}

/// An empty list, for sections that are switched off.
static EMPTY: Vec<&ResourceUse> = Vec::new();
const S5_GAP: f32 = 24.0;

/// "Sala · Servidor B · 10/11 14:00–16:00 · Em uso", for hovering.
fn use_line_hover(item: &ResourceUse, data: &Data) -> String {
    let (text, _, label) = use_line(data.name(&item.resource_uid), item, &data.now);
    format!("{text}\n{label}")
}

/// One resource as a row of the register; true when it was clicked.
fn resource_row(ui: &mut egui::Ui, r: &Resource, data: &Data, selected: bool) -> bool {
    let inactive = r.status == ResourceStatus::Inactive;
    let taken = taken_now(&data.uses, &r.uid, &data.now);
    pick_row(ui, ("resource-row", r.uid.as_str()), selected, 14, |ui| {
        ui.horizontal(|ui| {
            let right = 220.0;
            ui.scope(|ui| {
                ui.set_max_width((ui.available_width() - right - 90.0).max(60.0));
                let text = RichText::new(&r.name).font(theme::font_strong(theme::SIZE_BODY));
                ui.add(
                    egui::Label::new(if inactive {
                        text.color(c().muted)
                    } else {
                        text
                    })
                    .truncate(),
                );
            });
            widgets::pill(ui, kind_label(&r.kind), Tone::Neutral);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if inactive {
                    widgets::pill(ui, r.status.label(), Tone::Neutral);
                } else {
                    widgets::pill(ui, r.status.label(), Tone::Ok);
                }
                let (text, tone) = if taken > 0 {
                    (
                        format!("em uso agora: {taken} de {}", r.quantity),
                        Tone::Violet,
                    )
                } else {
                    (
                        format!("livre: {} de {}", r.quantity, r.quantity),
                        Tone::Neutral,
                    )
                };
                if !inactive {
                    widgets::pill(ui, text, tone);
                }
            });
        });
        let mut line = Vec::new();
        if r.quantity > 1 {
            line.push(format!("Quantidade: {}", r.quantity));
        }
        if !r.location.trim().is_empty() {
            line.push(format!("Fica em: {}", r.location.trim()));
        }
        if !r.identification.trim().is_empty() {
            line.push(r.identification.trim().to_owned());
        }
        if !line.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(line.join("  ·  "))
                        .size(theme::SIZE_SMALL)
                        .color(c().muted),
                )
                .truncate(),
            );
        }
    })
    .clicked()
}

// ---------------------------------------------------------------------------
// The ficha of a use

/// The sentence the person reads for an error of the library: the clash
/// marker is dropped and the sentence kept.
fn error_sentence(text: &str, what: &str) -> String {
    if text.contains(resources::HORARIO_OCUPADO) {
        strip_marker(text).to_owned()
    } else {
        format!("Não foi possível {what}: {text}")
    }
}

impl ResourcesPanel {
    fn use_ficha(&mut self, ui: &mut egui::Ui, store: &mut Store, data: &Data) {
        let Some(mut ed) = self.use_editor.clone() else {
            return;
        };
        let p = c();
        let is_dirty = self.dirty();
        let mut action: Option<UseAction> = None;
        let width = FICHA_WIDTH.min(ui.available_width() * 0.62);

        egui::Panel::right("resources-ficha")
            .exact_size(width)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.surface))
            .show(ui, |ui| {
                // ---- Footer, fixed at the bottom ------------------------------
                egui::Panel::bottom("resources-ficha-foot")
                    .frame(
                        egui::Frame::new()
                            .fill(p.surface)
                            .inner_margin(egui::Margin::symmetric(16, 10)),
                    )
                    .show(ui, |ui| {
                        self.use_footer(ui, &ed, is_dirty, &mut action);
                    });

                // ---- Header ----------------------------------------------------
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = S2;
                        ui.horizontal(|ui| {
                            let reserve = 40.0 + if is_dirty { 90.0 } else { 0.0 };
                            ui.scope(|ui| {
                                ui.set_max_width((ui.available_width() - reserve).max(80.0));
                                let title = if ed.uid.is_empty() {
                                    "Novo uso".to_owned()
                                } else {
                                    data.name(&ed.resource_uid).to_owned()
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(title).font(theme::font_strong(17.0)),
                                    )
                                    .truncate(),
                                );
                            });
                            if is_dirty {
                                widgets::pill(ui, "não salvo", Tone::Warn);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled_ui(!is_dirty, |ui| {
                                            widgets::icon_button(ui, icon::CLOSE, "Fechar ficha")
                                        })
                                        .inner
                                        .on_disabled_hover_text(
                                            "Salve ou descarte as alterações antes de fechar.",
                                        )
                                        .clicked()
                                    {
                                        action = Some(UseAction::Close);
                                    }
                                },
                            );
                        });
                        if !ed.uid.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                let (label, tone) = status_pill(&ed, &data.now);
                                widgets::pill(ui, label, tone);
                                if let Some(r) = data.resource(&ed.resource_uid) {
                                    widgets::pill(ui, kind_label(&r.kind), Tone::Neutral);
                                }
                            });
                        }
                    });

                // ---- Form ------------------------------------------------------
                egui::ScrollArea::vertical()
                    .id_salt("resources-use-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(16, 0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = S2;
                                self.use_form(ui, store, data, &mut ed, &mut action);
                                ui.add_space(S4);
                            });
                    });
            });

        self.use_editor = Some(ed.clone());
        if let Some(action) = action {
            self.run_use_action(store, data, ed, action);
        }
    }

    fn use_footer(
        &mut self,
        ui: &mut egui::Ui,
        ed: &ResourceUse,
        is_dirty: bool,
        action: &mut Option<UseAction>,
    ) {
        let p = c();
        ui.spacing_mut().item_spacing.y = S2;
        let closed = matches!(ed.status, UseStatus::Returned | UseStatus::Cancelled);
        let new = ed.uid.is_empty();
        if self.confirm_delete_use && !new {
            let (fg, bg) = Tone::Warn.colors();
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new("Apagar de vez este uso? Ele não vai para a Lixeira. O recurso, o documento e a tarefa ligados permanecem.")
                                .color(fg),
                        )
                        .wrap(),
                    );
                    ui.horizontal(|ui| {
                        if ui.add(widgets::danger("Apagar de vez")).clicked() {
                            *action = Some(UseAction::Delete);
                        }
                        if ui.add(widgets::secondary("Cancelar")).clicked() {
                            self.confirm_delete_use = false;
                        }
                    });
                });
        }
        ui.horizontal_wrapped(|ui| {
            if new {
                if ui
                    .add(widgets::primary("Reservar"))
                    .on_hover_text("Guarda a reserva para o horário escolhido.")
                    .clicked()
                {
                    *action = Some(UseAction::Reserve);
                }
                if ui
                    .add(widgets::secondary("Registrar uso agora"))
                    .on_hover_text("O uso começa agora: o início passa a ser o horário atual.")
                    .clicked()
                {
                    *action = Some(UseAction::StartNow);
                }
            } else if closed {
                if ui
                    .add(widgets::primary("Reabrir"))
                    .on_hover_text(if ed.status == UseStatus::Returned {
                        "Volta para “Em uso”: o recurso ainda não foi devolvido."
                    } else {
                        "Volta a ser uma reserva."
                    })
                    .clicked()
                {
                    *action = Some(UseAction::Reopen);
                }
            } else if self.returning {
                if ui
                    .add(widgets::primary("Confirmar devolução"))
                    .on_hover_text("Grava a devolução na data e hora informadas.")
                    .clicked()
                {
                    *action = Some(UseAction::ConfirmReturn);
                }
                if ui.add(widgets::secondary("Voltar")).clicked() {
                    *action = Some(UseAction::EndReturn);
                }
            } else {
                if ui
                    .add_enabled(is_dirty, widgets::primary("Salvar"))
                    .clicked()
                {
                    *action = Some(UseAction::Save);
                }
                if ed.status == UseStatus::Reserved {
                    if ui
                        .add(widgets::secondary("Registrar uso agora"))
                        .on_hover_text("O uso começa agora: situação “Em uso”.")
                        .clicked()
                    {
                        *action = Some(UseAction::StartNow);
                    }
                    if ui
                        .add(widgets::secondary("Cancelar a reserva"))
                        .on_hover_text("A reserva deixa de ocupar o recurso. O registro fica na lista de encerrados.")
                        .clicked()
                    {
                        *action = Some(UseAction::CancelReservation);
                    }
                } else {
                    if ui
                        .add(widgets::secondary("Registrar devolução"))
                        .on_hover_text("Informa quando o recurso voltou e encerra o uso.")
                        .clicked()
                    {
                        *action = Some(UseAction::BeginReturn);
                    }
                    if ui
                        .add(widgets::secondary("Prorrogar"))
                        .on_hover_text("Vai para o fim previsto, para informar a nova data e hora.")
                        .clicked()
                    {
                        *action = Some(UseAction::Extend);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(is_dirty, widgets::quiet("Descartar alterações"))
                .clicked()
            {
                *action = Some(UseAction::Revert);
            }
            if !new {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!is_dirty, widgets::danger("Apagar de vez…"))
                        .on_hover_text("Apaga o uso sem passar pela Lixeira.")
                        .on_disabled_hover_text("Salve ou descarte as alterações antes de apagar.")
                        .clicked()
                    {
                        self.confirm_delete_use = true;
                    }
                });
            }
        });
        let _ = p;
    }

    /// The fields of the ficha of a use, in sections.
    fn use_form(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        data: &Data,
        ed: &mut ResourceUse,
        action: &mut Option<UseAction>,
    ) {
        let p = c();
        let closed = matches!(ed.status, UseStatus::Returned | UseStatus::Cancelled);
        let uid = ed.uid.clone();
        if !self.use_error.is_empty() {
            let text = self.use_error.clone();
            if notice_line(ui, &text, Tone::Danger) {
                self.use_error.clear();
            }
        }
        if let Some(r) = data.resource(&ed.resource_uid) {
            if r.status == ResourceStatus::Inactive && !closed {
                notice_line(
                    ui,
                    "Este recurso está fora de uso: só é possível registrar devolução ou encerrar usos que já existem.",
                    Tone::Warn,
                );
            }
        }

        form_section(ui, "Recurso", |ui| {
            ui.add_enabled_ui(!closed, |ui| {
                labeled(ui, "Recurso", |ui| {
                    let text = data
                        .resource(&ed.resource_uid)
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| "Escolha o recurso".to_owned());
                    pick(ui, "res-use-resource", text, |ui| {
                        for r in &data.resources {
                            if r.status == ResourceStatus::Inactive && r.uid != ed.resource_uid {
                                continue;
                            }
                            let label = if r.quantity > 1 {
                                format!("{} ({})", r.name, r.quantity)
                            } else {
                                r.name.clone()
                            };
                            ui.selectable_value(&mut ed.resource_uid, r.uid.clone(), label);
                        }
                    });
                });
                if let Some(r) = data.resource(&ed.resource_uid) {
                    ed.quantity = ed.quantity.clamp(1, r.quantity.max(1));
                    if r.quantity > 1 {
                        labeled(ui, "Quantidade", |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(&mut ed.quantity)
                                        .range(1..=r.quantity)
                                        .speed(0.2),
                                );
                                widgets::small_muted(ui, format!("de {}", r.quantity));
                            });
                        });
                    }
                }
            });
            if !closed {
                if let Some(r) = data.resource(&ed.resource_uid) {
                    if let (Ok(start), Ok(end)) =
                        (parse_moment(ed.start.trim()), parse_moment(ed.end.trim()))
                    {
                        if end > start {
                            let free = free_quantity(
                                r,
                                &data.uses,
                                ed.start.trim(),
                                ed.end.trim(),
                                &ed.uid,
                                &data.now,
                            );
                            let enough = free >= ed.quantity;
                            let text = if r.quantity == 1 {
                                if free >= 1 {
                                    "Livre neste horário".to_owned()
                                } else {
                                    "Ocupado neste horário".to_owned()
                                }
                            } else {
                                format!("{free} de {} livres neste horário", r.quantity)
                            };
                            ui.horizontal(|ui| {
                                widgets::pill(
                                    ui,
                                    text,
                                    if enough { Tone::Ok } else { Tone::Danger },
                                );
                            });
                        }
                    }
                }
            }
        });

        form_section(ui, "Quem usa", |ui| {
            ui.add_enabled_ui(!closed, |ui| {
                labeled(ui, "Quem", |ui| {
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 56.0).max(80.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut ed.who)
                                .desired_width(width)
                                .hint_text("Nome de quem vai usar"),
                        );
                        if std::mem::take(&mut self.focus_first) {
                            field.request_focus();
                        }
                        if ui
                            .add(widgets::secondary("Eu"))
                            .on_hover_text("Preenche com o seu nome (o que está em Ajustes ▸ Equipe e sincronização).")
                            .clicked()
                        {
                            if let Ok(cfg) = store.load_intranet_config() {
                                ed.who = cfg.member_name;
                            }
                        }
                    });
                });
                labeled(ui, "Setor", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut ed.sector).desired_width(f32::INFINITY));
                });
                labeled(ui, "Finalidade", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut ed.purpose).desired_width(f32::INFINITY),
                    );
                });
            });
        });

        form_section(ui, "Período", |ui| {
            ui.add_enabled_ui(!closed, |ui| {
                labeled(ui, "Início", |ui| {
                    ui.horizontal(|ui| {
                        crate::labels::date_field(
                            ui,
                            ("res-start-date", uid.as_str()),
                            &mut self.form.start_date,
                        );
                        crate::labels::time_field(
                            ui,
                            ("res-start-time", uid.as_str()),
                            &mut self.form.start_time,
                        );
                    });
                });
                labeled(ui, "Fim previsto", |ui| {
                    ui.horizontal(|ui| {
                        let date = crate::labels::date_field(
                            ui,
                            ("res-end-date", uid.as_str()),
                            &mut self.form.end_date,
                        );
                        if std::mem::take(&mut self.focus_end) {
                            date.request_focus();
                        }
                        crate::labels::time_field(
                            ui,
                            ("res-end-time", uid.as_str()),
                            &mut self.form.end_time,
                        );
                    });
                });
            });
            if self.returning || ed.status == UseStatus::Returned {
                let editable = self.returning;
                ui.add_enabled_ui(editable, |ui| {
                    labeled(ui, "Devolvido em", |ui| {
                        ui.horizontal(|ui| {
                            crate::labels::date_field(
                                ui,
                                ("res-ret-date", uid.as_str()),
                                &mut self.form.returned_date,
                            );
                            crate::labels::time_field(
                                ui,
                                ("res-ret-time", uid.as_str()),
                                &mut self.form.returned_time,
                            );
                        });
                    });
                });
                if self.returning {
                    widgets::small_muted(
                        ui,
                        "Confira a data e a hora em que o recurso voltou e confirme.",
                    );
                }
            }
            self.form.apply(ed);
            if self.returning {
                // The return is typed in the form; the use stays "Em uso"
                // until it is confirmed.
                ed.returned_at = None;
            }
            // The day of the resource, so a clash is seen before saving.
            if !closed && !ed.resource_uid.is_empty() {
                let day = moment_date(&ed.start);
                if !day.is_empty() {
                    let others =
                        day_occupation(&data.uses, &ed.resource_uid, &ed.uid, &day, &data.now);
                    ui.add_space(S1);
                    egui::Frame::new()
                        .fill(p.sunken)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 2.0;
                            widgets::field_label(
                                ui,
                                &format!("Ocupação do dia {}", crate::labels::date(&day)),
                            );
                            if others.is_empty() {
                                widgets::small_muted(ui, "Nenhum outro uso neste dia.");
                            }
                            let mine = (ed.start.trim().to_owned(), ed.end.trim().to_owned());
                            for item in others {
                                let (from, to) = item
                                    .occupies_at(&data.now)
                                    .unwrap_or((item.start.clone(), item.end.clone()));
                                let clips = |m: &str, edge: &str| {
                                    if moment_date(m) == day {
                                        moment_time(m)
                                    } else {
                                        edge.to_owned()
                                    }
                                };
                                let overlaps = from < mine.1 && mine.0 < to;
                                let mut text = format!(
                                    "{}–{} · {}",
                                    clips(&from, "00:00"),
                                    clips(&to, "24:00"),
                                    item.who.trim()
                                );
                                if item.quantity > 1 {
                                    text.push_str(&format!(" · × {}", item.quantity));
                                }
                                let color = if overlaps { p.warn } else { p.muted };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(text).size(theme::SIZE_SMALL).color(color),
                                    )
                                    .truncate(),
                                );
                            }
                        });
                }
            }
        });

        form_section(ui, "Observações", |ui| {
            ui.add_enabled(
                !closed,
                egui::TextEdit::multiline(&mut ed.notes)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
        });

        self.links_section(ui, store, data, ed, action);
    }

    /// Document, atendimento and task of the use.
    fn links_section(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        data: &Data,
        ed: &mut ResourceUse,
        action: &mut Option<UseAction>,
    ) {
        let closed = matches!(ed.status, UseStatus::Returned | UseStatus::Cancelled);
        form_section(ui, "Vínculos", |ui| {
            #[cfg(feature = "shots")]
            if std::mem::take(&mut self.shots_scroll_links) {
                ui.scroll_to_cursor(Some(egui::Align::Min));
            }
            // Documento
            widgets::field_label(ui, "Documento");
            if let Some(id) = ed.document_id {
                let name = self
                    .link_names
                    .document
                    .clone()
                    .unwrap_or_else(|| "Documento".to_owned());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(name).font(theme::font_strong(theme::SIZE_BODY)),
                        )
                        .truncate(),
                    );
                    if ui.add(widgets::quiet("Abrir")).clicked() {
                        self.ask = Some(Ask::Document(id));
                    }
                    if ui
                        .add_enabled(!closed, widgets::quiet("Desligar"))
                        .clicked()
                    {
                        ed.document_id = None;
                        self.link_names.document = None;
                    }
                });
            } else if !closed {
                if let Some(doc) =
                    self.doc_picker
                        .show(ui, store, "Buscar por número, assunto ou texto")
                {
                    ed.document_id = Some(doc.id);
                    self.link_names.document = Some(doc.document_number);
                }
            } else {
                widgets::small_muted(ui, "Nenhum documento ligado.");
            }
            ui.add_space(S1);
            // Atendimento
            widgets::field_label(ui, "Atendimento");
            if let Some(id) = ed.saiop_id {
                let name = self
                    .link_names
                    .attendance
                    .clone()
                    .unwrap_or_else(|| "Atendimento".to_owned());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(name).font(theme::font_strong(theme::SIZE_BODY)),
                        )
                        .truncate(),
                    );
                    if ui.add(widgets::quiet("Abrir")).clicked() {
                        self.ask = Some(Ask::Attendance(id));
                    }
                    if ui
                        .add_enabled(!closed, widgets::quiet("Desligar"))
                        .clicked()
                    {
                        ed.saiop_id = None;
                        self.link_names.attendance = None;
                    }
                });
            } else if !closed {
                if let Some((id, name)) = self.attendance_picker.show(
                    ui,
                    store,
                    PickKind::Attendance,
                    "Buscar por referência, assunto ou responsável",
                ) {
                    ed.saiop_id = Some(id);
                    self.link_names.attendance = Some(name);
                }
            } else {
                widgets::small_muted(ui, "Nenhum atendimento ligado.");
            }
            ui.add_space(S1);
            // Tarefa
            widgets::field_label(ui, "Tarefa");
            if let Some(id) = ed.task_id {
                let name = self
                    .link_names
                    .task
                    .clone()
                    .unwrap_or_else(|| "Tarefa".to_owned());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(name).font(theme::font_strong(theme::SIZE_BODY)),
                        )
                        .truncate(),
                    );
                    if ui.add(widgets::quiet("Abrir")).clicked() {
                        self.ask = Some(Ask::Task(id));
                    }
                    if ui
                        .add_enabled(!closed, widgets::quiet("Desligar"))
                        .clicked()
                    {
                        ed.task_id = None;
                        self.link_names.task = None;
                    }
                });
            } else if !closed {
                if ui
                    .add(widgets::secondary("Criar tarefa de devolução"))
                    .on_hover_text("Cria uma tarefa com prazo no dia do fim previsto e a liga a este uso. O uso é salvo antes, se tiver alterações.")
                    .clicked()
                {
                    *action = Some(UseAction::CreateTask);
                }
                if let Some((id, name)) = self.task_picker.show(
                    ui,
                    store,
                    PickKind::Task,
                    "Ou buscar uma tarefa que já existe",
                ) {
                    ed.task_id = Some(id);
                    self.link_names.task = Some(name);
                }
            } else {
                widgets::small_muted(ui, "Nenhuma tarefa ligada.");
            }
        });
        let _ = data;
    }

    /// What the footer of the ficha asked for.
    fn run_use_action(
        &mut self,
        store: &mut Store,
        data: &Data,
        ed: ResourceUse,
        action: UseAction,
    ) {
        let now = data.now.clone();
        match action {
            UseAction::Close => self.close_ficha(),
            UseAction::Revert => {
                if let Some(saved) = self.use_saved.clone() {
                    self.form = UseForm::from_use(&saved);
                    self.refresh_link_names(store, &saved);
                    self.use_editor = Some(saved);
                }
                self.returning = false;
                self.use_error.clear();
            }
            UseAction::Delete => {
                self.confirm_delete_use = false;
                match store.delete_resource_use(&ed.uid) {
                    Ok(()) => {
                        self.close_ficha();
                        self.changed = true;
                        self.set_notice("Uso apagado.", false);
                    }
                    Err(e) => {
                        self.use_error = error_sentence(&e.to_string(), "apagar o uso");
                    }
                }
            }
            UseAction::Save | UseAction::Reserve => {
                self.save_use_as(store, &now, None);
            }
            UseAction::StartNow => {
                let mut item = ed;
                self.form.apply(&mut item);
                item.status = UseStatus::InUse;
                item.returned_at = None;
                if item.start.as_str() > now.as_str() || item.start.starts_with('T') {
                    item.start = now.clone();
                }
                if item.end.as_str() <= item.start.as_str() {
                    if let Ok(start) = parse_moment(&item.start) {
                        item.end = resources::format_moment(start + Duration::hours(1));
                    }
                }
                self.save_use_as(store, &now, Some(item));
            }
            UseAction::CancelReservation => {
                let mut item = ed;
                item.status = UseStatus::Cancelled;
                self.save_use_as(store, &now, Some(item));
            }
            UseAction::Reopen => {
                let mut item = ed;
                item.status = if item.status == UseStatus::Returned {
                    UseStatus::InUse
                } else {
                    UseStatus::Reserved
                };
                item.returned_at = None;
                self.save_use_as(store, &now, Some(item));
            }
            UseAction::BeginReturn => {
                self.returning = true;
                self.form.returned_date = moment_date(&now);
                self.form.returned_time = moment_time(&now);
                self.use_error.clear();
            }
            UseAction::EndReturn => {
                self.returning = false;
                self.use_error.clear();
            }
            UseAction::ConfirmReturn => {
                let mut item = ed;
                item.status = UseStatus::Returned;
                self.form.apply(&mut item);
                if self.save_use_as(store, &now, Some(item)) {
                    self.returning = false;
                }
            }
            UseAction::Extend => self.focus_end = true,
            UseAction::CreateTask => self.create_return_task(store, data),
        }
    }

    /// "Salvar" and its variants: writes `replace` (or the ficha's use) and
    /// reloads it as saved. On refusal the sentence is shown in the ficha and
    /// the edit stays as it was.
    fn save_use_as(&mut self, store: &mut Store, _now: &str, replace: Option<ResourceUse>) -> bool {
        let Some(item) = replace.or_else(|| self.use_editor.clone()) else {
            return false;
        };
        if let Some(problem) = time_problem(&item) {
            self.use_error = problem.to_owned();
            return false;
        }
        let was_new = item.uid.is_empty();
        let status = item.status;
        match store.save_resource_use(&item, self.use_version) {
            Ok(uid) => {
                self.changed = true;
                let text = match (was_new, status) {
                    (true, UseStatus::InUse) => "Uso registrado.",
                    (true, _) => "Reserva feita.",
                    (false, UseStatus::InUse) => "Uso salvo.",
                    (false, UseStatus::Returned) => "Devolução registrada.",
                    (false, UseStatus::Cancelled) => "Reserva cancelada.",
                    (false, UseStatus::Reserved) => "Reserva salva.",
                };
                let reloaded = store
                    .list_resource_uses()
                    .ok()
                    .and_then(|list| list.into_iter().find(|u| u.uid == uid));
                match reloaded {
                    Some(saved) => self.load_use(store, saved),
                    None => self.close_ficha(),
                }
                self.focus_first = false;
                self.set_notice(text, false);
                true
            }
            Err(e) => {
                self.use_error = error_sentence(&e.to_string(), "salvar o uso");
                false
            }
        }
    }

    /// "Criar tarefa de devolução": a task due on the day of the expected
    /// end, linked to the document of the use when there is one.
    fn create_return_task(&mut self, store: &mut Store, data: &Data) {
        let Some(mut item) = self.use_editor.clone() else {
            return;
        };
        if item.task_id.is_some() {
            return;
        }
        // Unsaved edits are saved first; they are never discarded.
        if item.uid.is_empty() || self.dirty() {
            if !self.save_use_as(store, &data.now, None) {
                return;
            }
            match self.use_editor.clone() {
                Some(saved) => item = saved,
                None => return,
            }
        }
        let name = data.name(&item.resource_uid).to_owned();
        let mut task = match item.document_id {
            Some(id) => Task::new(id),
            None => Task::standalone(),
        };
        task.title = format!("Devolução: {name} — {}", item.who.trim());
        task.action = "acompanhar".to_owned();
        task.due_date = moment_date(&item.end);
        let title = task.title.clone();
        match store.save_task(&task) {
            Ok(id) => {
                item.task_id = Some(id);
                let version = self.use_version;
                match store.save_resource_use(&item, version) {
                    Ok(uid) => {
                        let reloaded = store
                            .list_resource_uses()
                            .ok()
                            .and_then(|list| list.into_iter().find(|u| u.uid == uid));
                        if let Some(saved) = reloaded {
                            self.load_use(store, saved);
                            self.focus_first = false;
                        }
                        self.changed = true;
                        self.toast = Some(format!("Tarefa criada: {title}"));
                    }
                    Err(e) => {
                        self.use_error = error_sentence(&e.to_string(), "ligar a tarefa ao uso");
                    }
                }
            }
            Err(e) => {
                self.use_error = error_sentence(&e.to_string(), "criar a tarefa");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The ficha of a resource

/// What the footer of the ficha of a resource asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceAction {
    Save,
    ToggleStatus,
    Revert,
    Close,
    ConfirmDelete,
    Reserve,
}

impl ResourcesPanel {
    fn resource_ficha(&mut self, ui: &mut egui::Ui, store: &mut Store, data: &Data) {
        let Some(mut ed) = self.res_editor.clone() else {
            return;
        };
        let p = c();
        let is_dirty = self.dirty();
        let new = ed.uid.is_empty();
        let mut action: Option<ResourceAction> = None;
        let width = FICHA_WIDTH.min(ui.available_width() * 0.62);
        let has_uses = data.uses.iter().any(|u| u.resource_uid == ed.uid);
        let taken = taken_now(&data.uses, &ed.uid, &data.now);

        egui::Panel::right("resources-ficha")
            .exact_size(width)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.surface))
            .show(ui, |ui| {
                // ---- Footer ---------------------------------------------------
                egui::Panel::bottom("resources-ficha-foot")
                    .frame(
                        egui::Frame::new()
                            .fill(p.surface)
                            .inner_margin(egui::Margin::symmetric(16, 10)),
                    )
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S2;
                        if self.confirm_delete_res && !new {
                            let (fg, bg) = Tone::Warn.colors();
                            egui::Frame::new()
                                .fill(bg)
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                                .inner_margin(egui::Margin::symmetric(12, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "Apagar de vez “{}”? Ele não vai para a Lixeira.",
                                                ed.name.trim()
                                            ))
                                            .color(fg),
                                        )
                                        .wrap(),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui.add(widgets::danger("Apagar de vez")).clicked() {
                                            action = Some(ResourceAction::ConfirmDelete);
                                        }
                                        if ui.add(widgets::secondary("Cancelar")).clicked() {
                                            self.confirm_delete_res = false;
                                        }
                                    });
                                });
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(is_dirty || new, widgets::primary("Salvar"))
                                .clicked()
                            {
                                action = Some(ResourceAction::Save);
                            }
                            if !new {
                                let (label, hint) = if ed.status == ResourceStatus::Active {
                                    (
                                        "Marcar como fora de uso",
                                        "O recurso deixa de aceitar reservas novas. Os usos que já existem continuam.",
                                    )
                                } else {
                                    ("Voltar a usar", "O recurso volta a aceitar reservas.")
                                };
                                if ui
                                    .add_enabled(!is_dirty, widgets::secondary(label))
                                    .on_hover_text(hint)
                                    .on_disabled_hover_text(
                                        "Salve ou descarte as alterações antes.",
                                    )
                                    .clicked()
                                {
                                    action = Some(ResourceAction::ToggleStatus);
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(is_dirty, widgets::quiet("Descartar alterações"))
                                .clicked()
                            {
                                action = Some(ResourceAction::Revert);
                            }
                            if !new && !has_uses {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_enabled(!is_dirty, widgets::danger("Apagar de vez…"))
                                            .on_hover_text("Apaga o recurso sem passar pela Lixeira. Só é possível porque ele não tem nenhum uso.")
                                            .on_disabled_hover_text("Salve ou descarte as alterações antes de apagar.")
                                            .clicked()
                                        {
                                            self.confirm_delete_res = true;
                                        }
                                    },
                                );
                            }
                        });
                    });

                // ---- Header ------------------------------------------------------
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = S2;
                        ui.horizontal(|ui| {
                            let reserve = 40.0 + if is_dirty { 90.0 } else { 0.0 };
                            ui.scope(|ui| {
                                ui.set_max_width((ui.available_width() - reserve).max(80.0));
                                let title = if new || ed.name.trim().is_empty() {
                                    "Novo recurso".to_owned()
                                } else {
                                    ed.name.trim().to_owned()
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(title).font(theme::font_strong(17.0)),
                                    )
                                    .truncate(),
                                );
                            });
                            if is_dirty {
                                widgets::pill(ui, "não salvo", Tone::Warn);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add_enabled_ui(!is_dirty, |ui| {
                                        widgets::icon_button(ui, icon::CLOSE, "Fechar ficha")
                                    })
                                    .inner
                                    .on_disabled_hover_text(
                                        "Salve ou descarte as alterações antes de fechar.",
                                    )
                                    .clicked()
                                {
                                    action = Some(ResourceAction::Close);
                                }
                            });
                        });
                        if !new {
                            ui.horizontal_wrapped(|ui| {
                                widgets::pill(ui, kind_label(&ed.kind), Tone::Neutral);
                                widgets::pill(
                                    ui,
                                    ed.status.label(),
                                    if ed.status == ResourceStatus::Active {
                                        Tone::Ok
                                    } else {
                                        Tone::Neutral
                                    },
                                );
                                if ed.status == ResourceStatus::Active {
                                    widgets::pill(
                                        ui,
                                        format!("em uso agora: {taken} de {}", ed.quantity),
                                        if taken > 0 { Tone::Violet } else { Tone::Neutral },
                                    );
                                }
                            });
                        }
                    });

                // ---- Form ------------------------------------------------------
                egui::ScrollArea::vertical()
                    .id_salt("resources-resource-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(16, 0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = S2;
                                self.resource_form(ui, data, &mut ed, is_dirty, &mut action);
                                ui.add_space(S4);
                            });
                    });
            });

        self.res_editor = Some(ed.clone());
        let Some(action) = action else {
            return;
        };
        match action {
            ResourceAction::Close => self.close_ficha(),
            ResourceAction::Revert => {
                self.res_editor = self.res_saved.clone();
                self.res_error.clear();
            }
            ResourceAction::Save => self.save_resource(store, None),
            ResourceAction::ToggleStatus => {
                let mut item = ed;
                item.status = match item.status {
                    ResourceStatus::Active => ResourceStatus::Inactive,
                    ResourceStatus::Inactive => ResourceStatus::Active,
                };
                self.save_resource(store, Some(item));
            }
            ResourceAction::ConfirmDelete => {
                self.confirm_delete_res = false;
                match store.delete_resource(&ed.uid) {
                    Ok(()) => {
                        let name = ed.name.trim().to_owned();
                        self.close_ficha();
                        self.changed = true;
                        self.set_notice(format!("Recurso “{name}” apagado."), false);
                    }
                    Err(e) => {
                        self.res_error = error_sentence(&e.to_string(), "apagar o recurso");
                    }
                }
            }
            ResourceAction::Reserve => {
                if let Err(why) = self.new_use(store, Some(&ed.uid), None, None) {
                    self.set_notice(why, true);
                }
            }
        }
    }

    fn resource_form(
        &mut self,
        ui: &mut egui::Ui,
        data: &Data,
        ed: &mut Resource,
        is_dirty: bool,
        action: &mut Option<ResourceAction>,
    ) {
        if !self.res_error.is_empty() {
            let text = self.res_error.clone();
            if notice_line(ui, &text, Tone::Danger) {
                self.res_error.clear();
            }
        }
        form_section(ui, "Identificação do recurso", |ui| {
            labeled(ui, "Nome", |ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut ed.name)
                        .desired_width(f32::INFINITY)
                        .hint_text("Por exemplo: Sala de reuniões do 2º andar"),
                );
                if std::mem::take(&mut self.focus_first) {
                    field.request_focus();
                }
            });
            ui.columns(2, |cols| {
                labeled(&mut cols[0], "Tipo", |ui| {
                    pick(ui, "res-kind", kind_label(&ed.kind).to_owned(), |ui| {
                        for kind in RESOURCE_KINDS {
                            ui.selectable_value(&mut ed.kind, kind.to_owned(), kind_label(kind));
                        }
                    });
                });
                labeled(&mut cols[1], "Quantidade", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut ed.quantity)
                            .range(1..=resources::MAX_QUANTITY)
                            .speed(0.2),
                    )
                    .on_hover_text("Quantas unidades existem. Uma sala é 1; cadeiras empilháveis podem ser 40.");
                });
            });
            labeled(ui, "Onde fica", |ui| {
                ui.add(egui::TextEdit::singleline(&mut ed.location).desired_width(f32::INFINITY));
            });
            labeled(ui, "Identificação", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut ed.identification)
                        .desired_width(f32::INFINITY)
                        .hint_text("Tombo, patrimônio, ISBN…"),
                );
            });
        });
        form_section(ui, "Observações", |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut ed.notes)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
        });

        if ed.uid.is_empty() {
            return;
        }
        // The next uses of this resource, and the way to reserve it.
        form_section(ui, "Próximos usos", |ui| {
            if ed.status == ResourceStatus::Active
                && ui
                    .add_enabled(!is_dirty, widgets::secondary("Reservar…"))
                    .on_hover_text("Abre um uso novo deste recurso.")
                    .on_disabled_hover_text("Salve ou descarte as alterações antes de reservar.")
                    .clicked()
            {
                *action = Some(ResourceAction::Reserve);
            }
            let upcoming: Vec<&ResourceUse> = data
                .uses
                .iter()
                .filter(|u| u.resource_uid == ed.uid)
                .filter(|u| matches!(u.status, UseStatus::Reserved | UseStatus::InUse))
                .take(8)
                .collect();
            if upcoming.is_empty() {
                widgets::small_muted(ui, "Nenhum uso reservado ou em andamento.");
                return;
            }
            list_box(ui, |ui| {
                for item in upcoming {
                    let (label, tone) = status_pill(item, &data.now);
                    let clicked = pick_row(ui, ("res-next", item.uid.as_str()), false, 12, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(period_text(&item.start, &item.end))
                                        .font(theme::font_strong(theme::SIZE_BODY)),
                                )
                                .truncate(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    widgets::pill(ui, label, tone);
                                },
                            );
                        });
                        let who = item.who.trim();
                        let text = if item.quantity > 1 {
                            format!("{who} · × {}", item.quantity)
                        } else {
                            who.to_owned()
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(text).size(theme::SIZE_SMALL).color(c().muted),
                            )
                            .truncate(),
                        );
                    })
                    .clicked();
                    if clicked {
                        self.pending_open_use = Some(item.uid.clone());
                    }
                }
            });
        });
    }

    /// Writes `replace` (or the ficha's resource) and reloads it as saved.
    fn save_resource(&mut self, store: &mut Store, replace: Option<Resource>) {
        let Some(item) = replace.or_else(|| self.res_editor.clone()) else {
            return;
        };
        let was_new = item.uid.is_empty();
        match store.save_resource(&item, self.res_version) {
            Ok(uid) => {
                self.changed = true;
                let reloaded = store
                    .list_resources()
                    .ok()
                    .and_then(|list| list.into_iter().find(|r| r.uid == uid));
                match reloaded {
                    Some(saved) => self.load_resource(store, saved),
                    None => self.close_ficha(),
                }
                self.focus_first = false;
                self.set_notice(
                    if was_new {
                        "Recurso cadastrado."
                    } else {
                        "Recurso salvo."
                    },
                    false,
                );
            }
            Err(e) => {
                self.res_error = error_sentence(&e.to_string(), "salvar o recurso");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pictures of the screens (development only)

/// Views of the area the picture tour takes (prefix `recursos-`).
#[cfg(feature = "shots")]
pub const SHOTS: &[&str] = &[
    "agora",
    "agora-filtros",
    "agora-antigos",
    "calendario",
    "calendario-dia",
    "cadastro",
    "uso-novo",
    "uso-em-uso",
    "uso-devolucao",
    "uso-atrasado",
    "uso-encerrado",
    "uso-choque",
    "uso-vinculos",
    "uso-vinculos-busca",
    "recurso",
    "recurso-novo",
    "vazio",
    "vazio-busca",
];

#[cfg(feature = "shots")]
impl ResourcesPanel {
    /// Puts the panel in the state called `view` (see `SHOTS`).
    pub fn shots_open(&mut self, store: &mut Store, view: &str) {
        let now = now_moment();
        let uses = store.list_resource_uses().unwrap_or_default();
        let resources = store.list_resources().unwrap_or_default();
        let by_name = |name: &str| {
            resources
                .iter()
                .find(|r| r.name == name)
                .map(|r| r.uid.clone())
                .unwrap_or_default()
        };
        match view {
            "agora-filtros" => {
                self.filters_open = true;
                self.filters.kind = "equipamento".to_owned();
            }
            "agora-antigos" => self.show_old = true,
            "calendario" => self.view = View::Calendar,
            "calendario-dia" => {
                self.view = View::Calendar;
                let day = (chrono::Local::now().date_naive() + Duration::days(2))
                    .format("%Y-%m-%d")
                    .to_string();
                if let Ok(date) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") {
                    use chrono::Datelike;
                    self.cal_month = Some((date.year(), date.month()));
                }
                self.cal_day = day;
                self.cal_scroll = true;
            }
            "cadastro" => self.view = View::Register,
            "uso-novo" => {
                let _ = self.new_use(
                    store,
                    Some(&by_name("Sala de reuniões do 2º andar")),
                    None,
                    None,
                );
            }
            "uso-em-uso" => {
                if let Some(item) = uses
                    .iter()
                    .find(|u| u.status == UseStatus::InUse && !u.overdue_at(&now))
                {
                    self.load_use(store, item.clone());
                }
            }
            "uso-devolucao" | "uso-atrasado" => {
                if let Some(item) = uses.iter().find(|u| u.overdue_at(&now)) {
                    self.load_use(store, item.clone());
                    if view == "uso-devolucao" {
                        self.returning = true;
                        self.form.returned_date = moment_date(&now);
                        self.form.returned_time = moment_time(&now);
                    }
                }
            }
            "uso-encerrado" => {
                if let Some(item) = uses.iter().find(|u| u.status == UseStatus::Returned) {
                    self.load_use(store, item.clone());
                }
            }
            "uso-choque" => {
                // A real attempt: the library refuses it and says why.
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                let _ = self.new_use(
                    store,
                    Some(&by_name("Sala de reuniões do 2º andar")),
                    None,
                    None,
                );
                if let Some(item) = self.use_editor.as_mut() {
                    item.who = "Servidor A".to_owned();
                    item.sector = "Setor Jurídico".to_owned();
                    item.purpose = "Reunião de alinhamento".to_owned();
                    item.start = format!("{today}T15:00");
                    item.end = format!("{today}T16:30");
                    self.form = UseForm::from_use(item);
                }
                self.save_use_as(store, &now, None);
            }
            "uso-vinculos" => {
                if let Some(item) = uses
                    .iter()
                    .find(|u| u.document_id.is_some() && u.task_id.is_some())
                {
                    self.load_use(store, item.clone());
                    self.shots_scroll_links = true;
                }
            }
            "uso-vinculos-busca" => {
                let _ = self.new_use(store, Some(&by_name("Auditório")), None, None);
                self.doc_picker.shots_query(store, "veículos");
                self.shots_scroll_links = true;
            }
            "recurso" => {
                let uid = by_name("Notebook de apresentações");
                let _ = self.focus_resource(store, &uid);
            }
            "recurso-novo" => self.new_resource(),
            "vazio" => self.shots_empty = true,
            "vazio-busca" => self.filters.query = "projetor jurídico".to_owned(),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(uid: &str, name: &str, quantity: u32) -> Resource {
        Resource {
            uid: uid.to_owned(),
            name: name.to_owned(),
            quantity,
            ..Default::default()
        }
    }

    fn item(
        uid: &str,
        resource_uid: &str,
        status: UseStatus,
        start: &str,
        end: &str,
    ) -> ResourceUse {
        ResourceUse {
            uid: uid.to_owned(),
            resource_uid: resource_uid.to_owned(),
            who: "Servidor B".to_owned(),
            sector: "Setor Jurídico".to_owned(),
            purpose: "Reunião".to_owned(),
            status,
            start: start.to_owned(),
            end: end.to_owned(),
            ..Default::default()
        }
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    const NOW: &str = "2026-11-10T14:10";

    #[test]
    fn uses_are_found_by_what_they_are_linked_to() {
        let mut a = item(
            "a",
            "r",
            UseStatus::Reserved,
            "2026-11-11T09:00",
            "2026-11-11T10:00",
        );
        a.document_id = Some(7);
        let mut b = item(
            "b",
            "r",
            UseStatus::Reserved,
            "2026-11-12T09:00",
            "2026-11-12T10:00",
        );
        b.saiop_id = Some(3);
        b.task_id = Some(9);
        let c = item(
            "c",
            "r",
            UseStatus::Reserved,
            "2026-11-13T09:00",
            "2026-11-13T10:00",
        );
        let all = vec![a, b, c];
        let ids = |link: UseLink| -> Vec<&str> {
            uses_linked_to(&all, &link)
                .into_iter()
                .map(|u| u.uid.as_str())
                .collect()
        };
        assert_eq!(ids(UseLink::Document(7)), ["a"]);
        assert_eq!(ids(UseLink::Attendance(3)), ["b"]);
        assert_eq!(ids(UseLink::Task(9)), ["b"]);
        assert!(ids(UseLink::Document(8)).is_empty());
    }

    #[test]
    fn the_period_shows_one_date_or_two() {
        assert_eq!(
            period_text("2026-11-10T14:00", "2026-11-10T16:00"),
            "10/11 14:00–16:00"
        );
        assert_eq!(
            period_text("2026-11-10T14:00", "2026-11-12T09:30"),
            "10/11 14:00 – 12/11 09:30"
        );
    }

    #[test]
    fn the_line_of_a_use_has_text_tone_and_situation() {
        let reserved = item(
            "a",
            "r",
            UseStatus::Reserved,
            "2026-11-11T09:00",
            "2026-11-11T10:30",
        );
        let (text, tone, label) = use_line("Projetor", &reserved, NOW);
        assert_eq!(text, "Projetor · Servidor B · 11/11 09:00–10:30");
        assert_eq!((tone, label.as_str()), (Tone::Info, "Reservado"));
        let late = item(
            "b",
            "r",
            UseStatus::InUse,
            "2026-11-09T09:00",
            "2026-11-10T09:00",
        );
        let (_, tone, label) = use_line("Projetor", &late, NOW);
        assert_eq!((tone, label.as_str()), (Tone::Danger, "Atrasado"));
        let out = item(
            "c",
            "r",
            UseStatus::InUse,
            "2026-11-10T09:00",
            "2026-11-10T18:00",
        );
        assert_eq!(use_line("Projetor", &out, NOW).2, "Em uso");
        let back = item(
            "d",
            "r",
            UseStatus::Returned,
            "2026-11-09T09:00",
            "2026-11-09T10:00",
        );
        assert_eq!(use_line("Projetor", &back, NOW).2, "Devolvido");
        let off = item(
            "e",
            "r",
            UseStatus::Cancelled,
            "2026-11-12T09:00",
            "2026-11-12T10:00",
        );
        assert_eq!(use_line("Projetor", &off, NOW).2, "Cancelado");
    }

    #[test]
    fn a_new_use_starts_at_the_next_half_hour_and_lasts_an_hour() {
        let at = |now: &str| default_times(now);
        assert_eq!(
            at("2026-11-10T14:10"),
            ("2026-11-10T14:30".to_owned(), "2026-11-10T15:30".to_owned())
        );
        assert_eq!(
            at("2026-11-10T14:30"),
            ("2026-11-10T15:00".to_owned(), "2026-11-10T16:00".to_owned())
        );
        assert_eq!(
            at("2026-11-10T23:45"),
            ("2026-11-11T00:00".to_owned(), "2026-11-11T01:00".to_owned())
        );
        // On another day the use starts at nine.
        assert_eq!(
            times_for_day("2026-11-12", NOW),
            ("2026-11-12T09:00".to_owned(), "2026-11-12T10:00".to_owned())
        );
        // Today it starts at the next half hour.
        assert_eq!(times_for_day("2026-11-10", NOW).0, "2026-11-10T14:30");
        // Late at night there is no such half hour today: nine o'clock.
        assert_eq!(
            times_for_day("2026-11-10", "2026-11-10T23:50").0,
            "2026-11-10T09:00"
        );
    }

    #[test]
    fn view_one_is_grouped_into_late_in_use_today_and_next_days() {
        let overdue = item(
            "overdue",
            "r",
            UseStatus::InUse,
            "2026-11-08T09:00",
            "2026-11-09T17:00",
        );
        let in_use = item(
            "in-use",
            "r",
            UseStatus::InUse,
            "2026-11-10T12:00",
            "2026-11-10T18:00",
        );
        let today = item(
            "today",
            "r",
            UseStatus::Reserved,
            "2026-11-10T16:00",
            "2026-11-10T17:00",
        );
        let ongoing = item(
            "ongoing",
            "r",
            UseStatus::Reserved,
            "2026-11-09T09:00",
            "2026-11-12T17:00",
        );
        let next = item(
            "next",
            "r",
            UseStatus::Reserved,
            "2026-11-12T09:00",
            "2026-11-12T10:00",
        );
        let edge = item(
            "edge",
            "r",
            UseStatus::Reserved,
            "2026-11-24T09:00",
            "2026-11-24T10:00",
        );
        let later = item(
            "later",
            "r",
            UseStatus::Reserved,
            "2026-11-25T09:00",
            "2026-11-25T10:00",
        );
        let missed = item(
            "missed",
            "r",
            UseStatus::Reserved,
            "2026-11-05T09:00",
            "2026-11-05T10:00",
        );
        let returned = item(
            "returned",
            "r",
            UseStatus::Returned,
            "2026-11-04T09:00",
            "2026-11-04T10:00",
        );
        let cancelled = item(
            "cancelled",
            "r",
            UseStatus::Cancelled,
            "2026-11-11T09:00",
            "2026-11-11T10:00",
        );
        let all = [
            &overdue, &in_use, &today, &ongoing, &next, &edge, &later, &missed, &returned,
            &cancelled,
        ];
        let groups = group_uses(&all, day(10), NOW);
        let ids = |list: &Vec<&ResourceUse>| -> Vec<String> {
            list.iter().map(|u| u.uid.clone()).collect()
        };
        assert_eq!(ids(&groups.overdue), ["overdue"]);
        assert_eq!(ids(&groups.in_use), ["in-use"]);
        assert_eq!(ids(&groups.today), ["today", "ongoing"]);
        assert_eq!(ids(&groups.next_days), ["next", "edge"]);
        assert_eq!(ids(&groups.later), ["later"]);
        // Most recent first; a reservation that passed unused is over too.
        assert_eq!(ids(&groups.older), ["cancelled", "missed", "returned"]);
        assert_eq!(groups.current(), 7);
    }

    #[test]
    fn filters_match_by_text_kind_situation_period_and_resource() {
        let mut room = resource("sala", "Sala de reuniões", 1);
        room.kind = "sala".into();
        let mut projector = resource("proj", "Projetor", 1);
        projector.kind = "equipamento".into();
        let a = item(
            "a",
            "sala",
            UseStatus::Reserved,
            "2026-11-11T09:00",
            "2026-11-11T10:00",
        );
        let mut b = item(
            "b",
            "proj",
            UseStatus::InUse,
            "2026-11-09T09:00",
            "2026-11-12T10:00",
        );
        b.who = "Servidora Ávila".into();
        b.purpose = "Evento de capacitação".into();
        let f = |filters: &Filters, u: &ResourceUse, r: &Resource| use_matches(u, Some(r), filters);

        let none = Filters::default();
        assert!(f(&none, &a, &room) && f(&none, &b, &projector));

        let text = Filters {
            query: "reuniao".into(),
            ..Default::default()
        };
        assert!(f(&text, &a, &room), "accents are ignored: name");
        assert!(!f(&text, &b, &projector));
        let who = Filters {
            query: "avila".into(),
            ..Default::default()
        };
        assert!(f(&who, &b, &projector) && !f(&who, &a, &room));
        let purpose = Filters {
            query: "capacitacao evento".into(),
            ..Default::default()
        };
        assert!(f(&purpose, &b, &projector), "every word, any order");
        let sector = Filters {
            query: "juridico".into(),
            ..Default::default()
        };
        assert!(f(&sector, &a, &room));

        let kind = Filters {
            kind: "sala".into(),
            ..Default::default()
        };
        assert!(f(&kind, &a, &room) && !f(&kind, &b, &projector));
        let status = Filters {
            status: Some(UseStatus::InUse),
            ..Default::default()
        };
        assert!(f(&status, &b, &projector) && !f(&status, &a, &room));
        let resource_only = Filters {
            resource: "proj".into(),
            ..Default::default()
        };
        assert!(f(&resource_only, &b, &projector) && !f(&resource_only, &a, &room));

        // The period must touch the days chosen.
        let period = Filters {
            from: "2026-11-12".into(),
            to: "2026-11-12".into(),
            ..Default::default()
        };
        assert!(f(&period, &b, &projector), "crosses the 12th");
        assert!(!f(&period, &a, &room));
        let before = Filters {
            to: "2026-11-08".into(),
            ..Default::default()
        };
        assert!(!f(&before, &a, &room) && !f(&before, &b, &projector));
        assert!(Filters {
            query: "x".into(),
            ..Default::default()
        }
        .active());
        assert!(!none.active());
    }

    #[test]
    fn the_calendar_puts_a_use_on_every_day_it_touches() {
        let one = item(
            "one",
            "r",
            UseStatus::Reserved,
            "2026-11-10T09:00",
            "2026-11-10T10:00",
        );
        let loan = item(
            "loan",
            "r",
            UseStatus::Reserved,
            "2026-11-11T09:00",
            "2026-11-13T17:00",
        );
        let midnight = item(
            "midnight",
            "r",
            UseStatus::Reserved,
            "2026-11-14T20:00",
            "2026-11-15T00:00",
        );
        let other = item(
            "other",
            "s",
            UseStatus::Reserved,
            "2026-11-10T09:00",
            "2026-11-10T10:00",
        );
        let cancelled = item(
            "off",
            "r",
            UseStatus::Cancelled,
            "2026-11-10T09:00",
            "2026-11-10T10:00",
        );
        let all = vec![one, loan, midnight, other, cancelled];
        let days = uses_by_day(&all, "r", day(1), day(30));
        let on = |d: u32| -> Vec<&str> {
            days.get(&format!("2026-11-{d:02}"))
                .map(|v| v.iter().map(|u| u.uid.as_str()).collect())
                .unwrap_or_default()
        };
        assert_eq!(on(10), ["one"]);
        assert_eq!(on(11), ["loan"]);
        assert_eq!(on(13), ["loan"]);
        assert!(on(14).contains(&"midnight"));
        assert!(on(15).is_empty(), "ends at midnight sharp");
        // All the resources: the other one too.
        let everything = uses_by_day(&all, "", day(10), day(10));
        assert_eq!(everything["2026-11-10"].len(), 2);
        // A window that cuts the loan.
        let cut = uses_by_day(&all, "r", day(12), day(12));
        assert_eq!(cut["2026-11-12"][0].uid, "loan");
        assert_eq!(cut.len(), 1);
    }

    #[test]
    fn the_day_of_a_resource_lists_the_other_uses_in_order() {
        let a = item(
            "a",
            "r",
            UseStatus::Reserved,
            "2026-11-10T15:00",
            "2026-11-10T16:00",
        );
        let b = item(
            "b",
            "r",
            UseStatus::Reserved,
            "2026-11-10T09:00",
            "2026-11-10T10:00",
        );
        let mine = item(
            "mine",
            "r",
            UseStatus::Reserved,
            "2026-11-10T11:00",
            "2026-11-10T12:00",
        );
        let other_day = item(
            "d",
            "r",
            UseStatus::Reserved,
            "2026-11-11T09:00",
            "2026-11-11T10:00",
        );
        let other_resource = item(
            "o",
            "s",
            UseStatus::Reserved,
            "2026-11-10T09:00",
            "2026-11-10T10:00",
        );
        let back = item(
            "back",
            "r",
            UseStatus::Returned,
            "2026-11-10T08:00",
            "2026-11-10T08:30",
        );
        let night = item(
            "night",
            "r",
            UseStatus::InUse,
            "2026-11-09T22:00",
            "2026-11-10T01:00",
        );
        let all = vec![a, b, mine, other_day, other_resource, back, night];
        let list = day_occupation(&all, "r", "mine", "2026-11-10", NOW);
        let ids: Vec<&str> = list.iter().map(|u| u.uid.as_str()).collect();
        // The one in use since last night is still out (until now).
        assert_eq!(ids, ["night", "b", "a"]);
    }

    #[test]
    fn what_is_out_now_counts_uses_in_use_and_reserved_ones_whose_time_has_come() {
        let mut two = item(
            "a",
            "r",
            UseStatus::Reserved,
            "2026-11-10T14:00",
            "2026-11-10T16:00",
        );
        two.quantity = 2;
        let late = item(
            "b",
            "r",
            UseStatus::InUse,
            "2026-11-08T09:00",
            "2026-11-09T09:00",
        );
        let future = item(
            "c",
            "r",
            UseStatus::Reserved,
            "2026-11-10T16:00",
            "2026-11-10T17:00",
        );
        let back = item(
            "d",
            "r",
            UseStatus::Returned,
            "2026-11-10T09:00",
            "2026-11-10T10:00",
        );
        let other = item(
            "e",
            "s",
            UseStatus::InUse,
            "2026-11-10T09:00",
            "2026-11-10T18:00",
        );
        let all = vec![two, late, future, back, other];
        assert_eq!(taken_now(&all, "r", NOW), 3);
        assert_eq!(taken_now(&all, "s", NOW), 1);
        assert_eq!(taken_now(&all, "x", NOW), 0);
    }

    #[test]
    fn typed_times_become_a_moment_and_wrong_ones_are_named() {
        let mut u = item("a", "r", UseStatus::Reserved, "", "");
        let mut form = UseForm {
            start_date: "2026-11-10".into(),
            start_time: "09:00".into(),
            end_date: "2026-11-10".into(),
            end_time: "10:30".into(),
            ..Default::default()
        };
        form.apply(&mut u);
        assert_eq!(u.start, "2026-11-10T09:00");
        assert_eq!(u.end, "2026-11-10T10:30");
        assert_eq!(time_problem(&u), None);
        assert_eq!(UseForm::from_use(&u), form);
        form.end_time = "9x".into();
        form.apply(&mut u);
        assert!(time_problem(&u).unwrap().contains("fim previsto"));
        form.start_date = "10/1".into();
        form.apply(&mut u);
        assert!(time_problem(&u).unwrap().contains("início"));
        // A return needs its own time.
        let mut returned = item(
            "b",
            "r",
            UseStatus::Returned,
            "2026-11-10T09:00",
            "2026-11-10T10:00",
        );
        assert!(time_problem(&returned).unwrap().contains("devolução"));
        returned.returned_at = Some("2026-11-10T09:40".into());
        assert_eq!(time_problem(&returned), None);
    }

    #[test]
    fn a_clash_reaches_the_person_without_its_marker() {
        let marked = format!(
            "{}O recurso “Projetor” já está reservado por Servidor B até 10/11/2026 16:00.",
            resources::HORARIO_OCUPADO
        );
        assert_eq!(
            error_sentence(&marked, "salvar o uso"),
            "O recurso “Projetor” já está reservado por Servidor B até 10/11/2026 16:00."
        );
        assert_eq!(
            error_sentence("disco cheio", "salvar o uso"),
            "Não foi possível salvar o uso: disco cheio"
        );
    }

    fn seeded_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        crate::shots_data::seed(&mut store).unwrap();
        (dir, store)
    }

    fn frame(panel: &mut ResourcesPanel, store: &mut Store, size: egui::Vec2) -> Option<Ask> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut ask = None;
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ask = panel.show(ui, store);
            });
        });
        output.textures_delta.clear();
        ask
    }

    #[test]
    fn the_exported_list_is_the_csv_of_the_uses_shown_without_their_notes() {
        let (_dir, store) = seeded_store();
        let data = Data {
            now: now_moment(),
            today: chrono::Local::now().date_naive(),
            resources: store.list_resources().unwrap(),
            uses: store.list_resource_uses().unwrap(),
        };
        let mut shown: Vec<ResourceUse> = data
            .uses
            .iter()
            .filter(|u| u.document_id.is_some())
            .cloned()
            .collect();
        assert!(!shown.is_empty());
        shown[0].notes = "observação que não sai".into();
        let text = export_text(&store, &data, &shown);
        assert!(text.starts_with('\u{feff}'));
        assert_eq!(
            text.lines().count(),
            1 + shown.len(),
            "header and one row each"
        );
        assert!(
            text.contains("ORG-OFI-2026/00431"),
            "the linked document by number"
        );
        assert!(!text.contains("observação que não sai"));
    }

    #[test]
    fn every_view_and_ficha_draws_without_panicking_and_fits_the_window() {
        let (_dir, mut store) = seeded_store();
        let mut panel = ResourcesPanel::default();
        let small = egui::vec2(900.0, 560.0);
        for view in [View::Now, View::Calendar, View::Register] {
            panel.view = view;
            frame(&mut panel, &mut store, small);
        }
        // The ficha of a use, new, and of a resource.
        panel.view = View::Now;
        panel.key_new(&store);
        assert!(panel.use_editor.is_some());
        frame(&mut panel, &mut store, small);
        panel.close_ficha();
        panel.view = View::Register;
        panel.key_new(&store);
        assert!(panel.res_editor.is_some());
        frame(&mut panel, &mut store, small);
        assert!(!panel.dirty(), "a blank ficha is not unsaved work");
    }

    #[test]
    fn a_ficha_with_edits_cannot_be_replaced_and_a_clash_keeps_the_edit() {
        let (_dir, mut store) = seeded_store();
        let mut panel = ResourcesPanel::default();
        let room = store
            .list_resources()
            .unwrap()
            .into_iter()
            .find(|r| r.name.starts_with("Sala de reuniões"))
            .unwrap();
        let held = store
            .list_resource_uses()
            .unwrap()
            .into_iter()
            .find(|u| u.resource_uid == room.uid && u.status == UseStatus::Reserved)
            .unwrap();
        // The same slot as an existing reservation.
        panel.new_use(&store, Some(&room.uid), None, None).unwrap();
        {
            let editor = panel.use_editor.as_mut().unwrap();
            editor.who = "Servidor A".into();
            editor.start = held.start.clone();
            editor.end = held.end.clone();
        }
        assert!(panel.dirty());
        let now = now_moment();
        assert!(!panel.save_use_as(&mut store, &now, None));
        assert!(
            panel.use_error.contains("já está") && !panel.use_error.contains('['),
            "{}",
            panel.use_error
        );
        assert_eq!(panel.use_editor.as_ref().unwrap().who, "Servidor A");
        // Nothing replaces what is unsaved.
        assert!(panel.focus_use(&store, &held.uid).is_err());
        assert!(panel.new_use_linked(&store, UseLink::Document(1)).is_err());
        // Another time works.
        let editor = panel.use_editor.as_mut().unwrap();
        editor.start = "2030-01-10T09:00".into();
        editor.end = "2030-01-10T10:00".into();
        assert!(panel.save_use_as(&mut store, &now, None));
        assert!(panel.use_error.is_empty() && !panel.dirty());
    }

    #[test]
    fn a_new_use_can_be_born_linked_and_a_return_task_is_linked_back() {
        let (_dir, mut store) = seeded_store();
        let mut panel = ResourcesPanel::default();
        let document = store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.document_number == "ORG-OFI-2026/00431")
            .unwrap();
        panel
            .new_use_linked(&store, UseLink::Document(document.id))
            .unwrap();
        let room = store
            .list_resources()
            .unwrap()
            .into_iter()
            .find(|r| r.name == "Auditório")
            .unwrap();
        {
            let editor = panel.use_editor.as_mut().unwrap();
            assert_eq!(editor.document_id, Some(document.id));
            editor.resource_uid = room.uid.clone();
            editor.who = "Servidor C".into();
            editor.start = "2031-02-10T09:00".into();
            editor.end = "2031-02-10T11:00".into();
        }
        assert_eq!(
            panel.link_names.document.as_deref(),
            Some("ORG-OFI-2026/00431"),
            "the document is named when the ficha opens"
        );
        let data = Data {
            now: now_moment(),
            today: chrono::Local::now().date_naive(),
            resources: store.list_resources().unwrap(),
            uses: store.list_resource_uses().unwrap(),
        };
        panel.create_return_task(&mut store, &data);
        let saved = panel.use_editor.clone().unwrap();
        assert!(!saved.uid.is_empty() && !panel.dirty());
        let task = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| Some(t.id) == saved.task_id)
            .expect("the task exists and is linked");
        assert_eq!(task.title, "Devolução: Auditório — Servidor C");
        assert_eq!(task.due_date, "2031-02-10");
        assert_eq!(task.document_id, Some(document.id));
        assert!(panel.take_message().is_some() && panel.take_changed());
        assert_eq!(panel.link_names.task.as_deref(), Some(task.title.as_str()));
    }
}
