//! Once-a-day reminder of what is late or due: a Windows notification with
//! counts only (never titles: it shows over other windows and on the lock
//! screen), and the same summary as an in-app notice when the window is seen.
//!
//! The decision (`Reminders::tick`) is pure and works on dates, never on
//! elapsed time, so clock changes and midnight are handled. The Win32 part
//! lives in `windows_ui`; this file only calls its three small functions.
use crate::storage::Store;
use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime};

/// Name of the small local file, next to `aparencia.txt`. It holds on/off,
/// the time and the last day notified: nothing from the library.
const FILE_NAME: &str = "lembretes.txt";
/// After start-up the library has a moment to load before the first reading.
const START_DELAY_SECONDS: i64 = 60;
/// The library is read at most this often while a reminder is still possible.
const READ_INTERVAL_SECONDS: i64 = 60;
/// A last day further than this in the future is taken as a wrong clock that
/// was corrected, not as a reason to stay silent.
const FUTURE_TOLERANCE_DAYS: i64 = 2;

/// What is due, with the same rules as the "Hoje" area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Open tasks whose deadline has passed.
    pub late_tasks: usize,
    /// Open tasks due today.
    pub today_tasks: usize,
    /// Open atendimentos whose deadline is today or past.
    pub due_attendances: usize,
    /// Open tasks whose reminder date is today or past, not already counted
    /// as late or due today.
    pub reminders: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.late_tasks + self.today_tasks + self.due_attendances + self.reminders
    }

    /// Reads the library once. Rules of `app/today.rs` (`split_tasks`,
    /// `due_attendances`) and of the "Com lembrete" view of the tasks.
    pub fn from_store(store: &Store, today: NaiveDate) -> Result<Counts> {
        let today = today.format("%Y-%m-%d").to_string();
        let mut counts = Counts::default();
        for task in store.list_tasks()? {
            let open = task.completed_at.is_none() && task.status != "concluida";
            let mut counted = false;
            if open && !task.due_date.is_empty() {
                match task.due_date.as_str().cmp(today.as_str()) {
                    std::cmp::Ordering::Less => {
                        counts.late_tasks += 1;
                        counted = true;
                    }
                    std::cmp::Ordering::Equal => {
                        counts.today_tasks += 1;
                        counted = true;
                    }
                    std::cmp::Ordering::Greater => {}
                }
            }
            if !counted
                && task.status != "concluida"
                && !task.reminder_date.is_empty()
                && task.reminder_date.as_str() <= today.as_str()
            {
                counts.reminders += 1;
            }
        }
        counts.due_attendances = store
            .list_saiop_attendances()?
            .iter()
            .filter(|a| {
                a.is_open() && !a.due_date.is_empty() && a.due_date.as_str() <= today.as_str()
            })
            .count();
        Ok(counts)
    }

    /// "3 tarefas atrasadas, 2 para hoje e 1 atendimento com prazo."
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.late_tasks > 0 {
            parts.push(plural(
                self.late_tasks,
                "tarefa atrasada",
                "tarefas atrasadas",
            ));
        }
        if self.today_tasks > 0 {
            if self.late_tasks > 0 {
                parts.push(format!("{} para hoje", self.today_tasks));
            } else {
                parts.push(plural(
                    self.today_tasks,
                    "tarefa para hoje",
                    "tarefas para hoje",
                ));
            }
        }
        if self.due_attendances > 0 {
            parts.push(plural(
                self.due_attendances,
                "atendimento com prazo",
                "atendimentos com prazo",
            ));
        }
        if self.reminders > 0 {
            parts.push(plural(self.reminders, "lembrete", "lembretes"));
        }
        let text = match parts.len() {
            0 => return "Nada vence hoje.".to_owned(),
            1 => parts.remove(0),
            _ => {
                let last = parts.pop().unwrap_or_default();
                format!("{} e {last}", parts.join(", "))
            }
        };
        format!("{text}.")
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The person's choices and the last day notified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReminderSettings {
    pub enabled: bool,
    pub hour: u32,
    pub minute: u32,
    pub last_day: Option<NaiveDate>,
}

impl Default for ReminderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            hour: 9,
            minute: 0,
            last_day: None,
        }
    }
}

impl ReminderSettings {
    /// Lines `lembrar sim|nao`, `hora HH:MM`, `ultimo AAAA-MM-DD`. Anything
    /// unreadable keeps the default; unknown lines are ignored.
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(key), Some(value)) = (words.next(), words.next()) else {
                continue;
            };
            match key {
                "lembrar" => settings.enabled = value != "nao",
                "hora" => {
                    if let Some((h, m)) = value.split_once(':') {
                        if let (Ok(h), Ok(m)) = (h.parse::<u32>(), m.parse::<u32>()) {
                            if h < 24 && m < 60 {
                                settings.hour = h;
                                settings.minute = m;
                            }
                        }
                    }
                }
                "ultimo" => {
                    settings.last_day = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
                }
                _ => {}
            }
        }
        settings
    }

    pub fn to_text(&self) -> String {
        let mut text = format!(
            "lembrar {}\nhora {:02}:{:02}\n",
            if self.enabled { "sim" } else { "nao" },
            self.hour,
            self.minute
        );
        if let Some(day) = self.last_day {
            text.push_str(&format!("ultimo {}\n", day.format("%Y-%m-%d")));
        }
        text
    }

    fn path() -> Option<std::path::PathBuf> {
        crate::paths::data_dir().ok().map(|dir| dir.join(FILE_NAME))
    }

    /// Unreadable or missing file: the defaults, never an error.
    pub fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// Best effort: a failure to write is not shown to the person.
    pub fn save(&self) {
        if let Some(path) = Self::path() {
            let _ = std::fs::write(path, self.to_text());
        }
    }

    fn time(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.hour, self.minute, 0).unwrap_or_default()
    }
}

/// What to show: the title and the sentence of the notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fire {
    pub title: String,
    pub text: String,
    pub counts: Counts,
}

impl Fire {
    fn new(counts: Counts) -> Self {
        Self {
            title: "Prazos de hoje no Ponte".to_owned(),
            text: counts.summary(),
            counts,
        }
    }
}

/// The state machine and the hand-off to the window.
#[derive(Default)]
pub struct Reminders {
    settings: ReminderSettings,
    /// When the first check of this run happened (the start-up delay).
    first_seen: Option<DateTime<Local>>,
    /// The last time the library was read for a decision.
    last_read: Option<DateTime<Local>>,
    /// The day already decided in this run (also when nothing was due).
    checked_day: Option<NaiveDate>,
    notice: Option<String>,
    open_today: bool,
    focused: bool,
    wired: bool,
}

impl Reminders {
    /// With the saved choices; the person's file is read once, here.
    pub fn load() -> Self {
        let mut reminders = Self::default();
        reminders.settings = ReminderSettings::load();
        reminders
    }

    pub fn enabled(&self) -> bool {
        self.settings.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.settings.enabled != enabled {
            self.settings.enabled = enabled;
            self.settings.save();
        }
    }

    pub fn time(&self) -> (u32, u32) {
        (self.settings.hour, self.settings.minute)
    }

    pub fn set_time(&mut self, hour: u32, minute: u32) {
        if hour < 24 && minute < 60 && (hour, minute) != self.time() {
            self.settings.hour = hour;
            self.settings.minute = minute;
            self.settings.save();
        }
    }

    /// Whether a reminder is still possible now: on, the hour reached, not
    /// notified (nor decided) today, start-up delay over.
    fn is_candidate(&self, now: DateTime<Local>) -> bool {
        if !self.settings.enabled {
            return false;
        }
        let today = now.date_naive();
        if self.checked_day == Some(today) {
            return false;
        }
        if let Some(last) = self.settings.last_day {
            // Today or later (a clock set back): already done. A day far in
            // the future is a wrong clock that was fixed: ignore it.
            if last >= today && last <= today + Duration::days(FUTURE_TOLERANCE_DAYS) {
                return false;
            }
        }
        if now.time() < self.settings.time() {
            return false;
        }
        match self.first_seen {
            Some(first) => now >= first + Duration::seconds(START_DELAY_SECONDS),
            None => true,
        }
    }

    /// Cheap: `true` when the caller should read the library and call `tick`.
    /// At most once a minute, and only when a reminder is still possible.
    pub fn wants_counts(&mut self, now: DateTime<Local>) -> bool {
        if self.first_seen.is_none_or(|first| now < first) {
            // First call, or the clock went back: start the delay again.
            self.first_seen = Some(now);
        }
        if !self.is_candidate(now) {
            return false;
        }
        if let Some(last) = self.last_read {
            if now >= last && now - last < Duration::seconds(READ_INTERVAL_SECONDS) {
                return false;
            }
        }
        self.last_read = Some(now);
        true
    }

    /// Decides whether to fire now. Marks the day as decided (and as notified
    /// when it fires); nothing due means no reminder for the rest of the day.
    pub fn tick(&mut self, now: DateTime<Local>, counts: Counts) -> Option<Fire> {
        if !self.is_candidate(now) {
            return None;
        }
        let today = now.date_naive();
        self.checked_day = Some(today);
        if counts.total() == 0 {
            return None;
        }
        self.settings.last_day = Some(today);
        Some(Fire::new(counts))
    }

    /// Once per frame. Cheap: reads the library only when `wants_counts` says
    /// so; otherwise it only polls two flags.
    pub fn update(&mut self, ctx: &eframe::egui::Context, store: &Store) {
        if !self.wired {
            self.wired = true;
            let wake = ctx.clone();
            crate::windows_ui::on_notification_click(move || wake.request_repaint());
        }
        self.focused = ctx.input(|i| i.focused);
        if crate::windows_ui::take_notification_click() {
            self.open_today = true;
        }
        if !self.settings.enabled {
            return;
        }
        let now = Local::now();
        if self.wants_counts(now) {
            // On a read error, try again in a minute.
            if let Ok(counts) = Counts::from_store(store, now.date_naive()) {
                if let Some(fire) = self.tick(now, counts) {
                    self.settings.save();
                    self.deliver(&fire, false);
                }
            }
        }
        // The window may be minimised and idle: look again in a while.
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }

    /// The Windows toast (not when the person is looking at Ponte already)
    /// and, always, the in-app notice for the next time the window is seen.
    fn deliver(&mut self, fire: &Fire, force_toast: bool) {
        if force_toast || !self.focused {
            let shown = crate::windows_ui::show_notification(&fire.title, &fire.text);
            if force_toast && !shown {
                self.notice = Some("Não foi possível mostrar o aviso do Windows.".to_owned());
            }
        }
        if !force_toast {
            self.notice = Some(fire.text.clone());
        }
    }

    /// "Mostrar um aviso de teste": sample counts, always as a Windows toast.
    pub fn test_notification(&mut self) {
        let sample = Counts {
            late_tasks: 3,
            today_tasks: 2,
            due_attendances: 1,
            reminders: 0,
        };
        self.deliver(&Fire::new(sample), true);
    }

    /// `true` once after the person clicked the notification or its icon: the
    /// app should bring its window forward on "Hoje".
    pub fn take_open_today(&mut self) -> bool {
        std::mem::take(&mut self.open_today)
    }

    /// The summary to show with the app's own notice, only while the window is
    /// in front (a notice would expire unseen otherwise).
    pub fn take_notice(&mut self) -> Option<String> {
        if self.focused {
            self.notice.take()
        } else {
            None
        }
    }

    /// Removes the notification-area icon. Also done when dropped.
    pub fn shutdown(&mut self) {
        crate::windows_ui::shutdown_notifications();
    }
}

impl Drop for Reminders {
    fn drop(&mut self) {
        crate::windows_ui::shutdown_notifications();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    fn at(day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 6, day, hour, minute, 0)
            .single()
            .expect("a valid local time")
    }

    fn due() -> Counts {
        Counts {
            late_tasks: 3,
            today_tasks: 2,
            due_attendances: 1,
            reminders: 0,
        }
    }

    #[test]
    fn the_summary_reads_naturally() {
        assert_eq!(
            due().summary(),
            "3 tarefas atrasadas, 2 para hoje e 1 atendimento com prazo."
        );
        let one = Counts {
            late_tasks: 1,
            ..Counts::default()
        };
        assert_eq!(one.summary(), "1 tarefa atrasada.");
        let today = Counts {
            today_tasks: 1,
            reminders: 2,
            ..Counts::default()
        };
        assert_eq!(today.summary(), "1 tarefa para hoje e 2 lembretes.");
        let many = Counts {
            today_tasks: 4,
            due_attendances: 2,
            reminders: 1,
            ..Counts::default()
        };
        assert_eq!(
            many.summary(),
            "4 tarefas para hoje, 2 atendimentos com prazo e 1 lembrete."
        );
        assert_eq!(Counts::default().summary(), "Nada vence hoje.");
    }

    #[test]
    fn nothing_before_the_hour_then_once_at_or_after_it() {
        let mut r = Reminders::default();
        assert!(r.tick(at(10, 8, 59), due()).is_none());
        let fire = r.tick(at(10, 9, 0), due()).expect("fires at the hour");
        assert_eq!(fire.text, due().summary());
        assert!(r.tick(at(10, 9, 1), due()).is_none());
        assert!(r.tick(at(10, 18, 0), due()).is_none());
        assert_eq!(r.settings.last_day, Some(at(10, 9, 0).date_naive()));
    }

    #[test]
    fn fires_late_when_the_program_starts_after_the_hour_and_again_next_day() {
        let mut r = Reminders::default();
        assert!(r.tick(at(10, 14, 30), due()).is_some());
        assert!(r.tick(at(11, 8, 0), due()).is_none());
        assert!(r.tick(at(11, 9, 0), due()).is_some());
        // Midnight while running: still only once for the new date.
        assert!(r.tick(at(12, 0, 5), due()).is_none());
        assert!(r.tick(at(12, 9, 0), due()).is_some());
    }

    #[test]
    fn off_or_nothing_due_stays_silent() {
        let mut off = Reminders::default();
        off.settings.enabled = false;
        assert!(off.tick(at(10, 10, 0), due()).is_none());
        let mut empty = Reminders::default();
        assert!(empty.tick(at(10, 10, 0), Counts::default()).is_none());
        // Decided for the day: a later change does not nag.
        assert!(empty.tick(at(10, 15, 0), due()).is_none());
        assert!(empty.settings.last_day.is_none());
        assert!(empty.tick(at(11, 10, 0), due()).is_some());
    }

    #[test]
    fn a_clock_set_back_never_repeats_a_day() {
        let mut r = Reminders::default();
        assert!(r.tick(at(10, 9, 30), due()).is_some());
        assert!(r.tick(at(10, 9, 10), due()).is_none());
        assert!(r.tick(at(9, 12, 0), due()).is_none());
    }

    #[test]
    fn a_wrong_far_future_day_does_not_silence_forever() {
        let mut r = Reminders::default();
        r.settings.last_day = Some(at(10, 0, 0).date_naive() + Duration::days(300));
        assert!(r.tick(at(10, 10, 0), due()).is_some());
    }

    #[test]
    fn the_library_is_read_at_most_once_a_minute_and_after_start_up() {
        let mut r = Reminders::default();
        // First call after the hour: the start-up delay has not passed.
        assert!(!r.wants_counts(at(10, 9, 0)));
        let t = at(10, 9, 0) + Duration::seconds(61);
        assert!(r.wants_counts(t));
        assert!(!r.wants_counts(t + Duration::seconds(5)));
        assert!(r.wants_counts(t + Duration::seconds(61)));
        // Once decided for the day, no more readings.
        assert!(r.tick(t + Duration::seconds(61), due()).is_some());
        assert!(!r.wants_counts(t + Duration::seconds(300)));
        // Before the hour nothing is read either.
        let mut early = Reminders::default();
        assert!(!early.wants_counts(at(10, 7, 0)));
        assert!(!early.wants_counts(at(10, 7, 5)));
    }

    #[test]
    fn the_settings_file_round_trips_and_bad_files_give_defaults() {
        let settings = ReminderSettings {
            enabled: false,
            hour: 7,
            minute: 30,
            last_day: Some(NaiveDate::from_ymd_opt(2026, 6, 10).expect("a valid date")),
        };
        assert_eq!(ReminderSettings::parse(&settings.to_text()), settings);
        assert_eq!(ReminderSettings::parse(""), ReminderSettings::default());
        assert_eq!(
            ReminderSettings::parse("lixo\nhora 25:99\nultimo ontem\n\u{0}\u{0}"),
            ReminderSettings::default()
        );
        assert_eq!(ReminderSettings::default().time().hour(), 9);
    }

    #[test]
    fn counts_follow_the_rules_of_the_today_area() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let mut store = Store::open(&dir.path().join("ponte.sqlite3")).expect("a library");
        let today = NaiveDate::from_ymd_opt(2026, 6, 10).expect("a valid date");
        assert_eq!(
            Counts::from_store(&store, today).expect("counts"),
            Counts::default()
        );
        let mut add = |due: &str, reminder: &str, status: &str| {
            let mut task = crate::tasks::Task::blank(None);
            task.title = "Tarefa de teste".into();
            task.due_date = due.into();
            task.reminder_date = reminder.into();
            task.status = status.into();
            store.save_task(&task).expect("saved");
        };
        add("2026-06-01", "", "pendente"); // late
        add("2026-06-09", "2026-06-01", "pendente"); // late, counted once
        add("2026-06-10", "", "aguardando"); // today
        add("2026-06-30", "2026-06-10", "pendente"); // reminder only
        add("2026-06-30", "2026-06-11", "pendente"); // reminder in the future
        add("", "", "pendente"); // no date at all
        add("2026-06-01", "2026-06-01", "concluida"); // done
        assert_eq!(
            Counts::from_store(&store, today).expect("counts"),
            Counts {
                late_tasks: 2,
                today_tasks: 1,
                due_attendances: 0,
                reminders: 1,
            }
        );
    }
}
