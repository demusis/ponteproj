//! Planned workload of a week: how many hours of work blocks fall on each day
//! against the working window, so an overbooked day shows before a deadline
//! slips. Pure functions; the Semana view computes them once per week.
use crate::projection::{WORK_END_HOUR, WORK_START_HOUR};
use crate::resources::parse_moment;
use crate::tasks::Task;
use chrono::{Duration, NaiveDate, NaiveDateTime};
use std::collections::HashSet;

/// Minutes of one working window (09:00–18:00).
pub const DAY_CAPACITY_MINUTES: u32 = (WORK_END_HOUR - WORK_START_HOUR) * 60;

/// Planned minutes against the capacity of a day or of a whole week.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub planned_minutes: u32,
    pub capacity_minutes: u32,
}

impl Load {
    /// More planned than the capacity, on a day that has capacity. Planned
    /// work on a day off is shown but not flagged.
    pub fn is_over(&self) -> bool {
        self.capacity_minutes > 0 && self.planned_minutes > self.capacity_minutes
    }
}

/// The planned periods that count, one per open task. A task with a valid
/// period and at least one open subtask with a valid period is left out: its
/// subtasks carry the hours, so the same hours are not counted twice.
/// Concluded tasks and invalid, empty or reversed periods are ignored.
pub fn counted_periods(tasks: &[Task]) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let period = |task: &Task| -> Option<(NaiveDateTime, NaiveDateTime)> {
        if task.is_done() {
            return None;
        }
        let start = parse_moment(&task.scheduled_start).ok()?;
        let end = parse_moment(&task.scheduled_end).ok()?;
        (end > start).then_some((start, end))
    };
    let planned: Vec<_> = tasks.iter().map(|task| (task, period(task))).collect();
    let parents: HashSet<i64> = planned
        .iter()
        .filter(|(_, p)| p.is_some())
        .filter_map(|(task, _)| task.parent_task_id)
        .collect();
    planned
        .into_iter()
        .filter(|(task, _)| !parents.contains(&task.id))
        .filter_map(|(_, p)| p)
        .collect()
}

/// Minutes of `period` that count for `day`. On a working day, the part inside
/// 09:00–18:00; a period with nothing inside the window (19:00–21:00) counts
/// its real length that day. On a day off, the real length that day. Outside
/// the window one period never counts more than a working day (9 h), so a
/// block that spans a weekend or a holiday does not add 24 h a day.
pub fn minutes_on_day(
    period: (NaiveDateTime, NaiveDateTime),
    day: NaiveDate,
    working: bool,
) -> u32 {
    let overlap = |from: NaiveDateTime, to: NaiveDateTime| -> i64 {
        (period.1.min(to) - period.0.max(from)).num_minutes().max(0)
    };
    let midnight = day.and_hms_opt(0, 0, 0).unwrap();
    let whole = overlap(midnight, midnight + Duration::days(1));
    let minutes = if working {
        let inside = overlap(
            day.and_hms_opt(WORK_START_HOUR, 0, 0).unwrap(),
            day.and_hms_opt(WORK_END_HOUR, 0, 0).unwrap(),
        );
        if inside > 0 {
            inside
        } else {
            whole
        }
    } else {
        whole
    };
    u32::try_from(minutes.min(i64::from(DAY_CAPACITY_MINUTES))).unwrap_or(0)
}

/// The load of each day from `start` (a Monday) for seven days, and the total.
/// `working` tells which days have a working window.
pub fn week_load(
    tasks: &[Task],
    start: NaiveDate,
    working: impl Fn(NaiveDate) -> bool,
) -> ([Load; 7], Load) {
    let periods = counted_periods(tasks);
    let mut days = [Load::default(); 7];
    let mut total = Load::default();
    for (offset, load) in days.iter_mut().enumerate() {
        let day = start + Duration::days(offset as i64);
        let is_working = working(day);
        load.capacity_minutes = if is_working { DAY_CAPACITY_MINUTES } else { 0 };
        load.planned_minutes = periods
            .iter()
            .map(|&period| minutes_on_day(period, day, is_working))
            .sum();
        total.planned_minutes += load.planned_minutes;
        total.capacity_minutes += load.capacity_minutes;
    }
    (days, total)
}

/// "6 h", "1 h 30", "45 min", "0 h".
pub fn hours_label(minutes: u32) -> String {
    let (hours, rest) = (minutes / 60, minutes % 60);
    match (hours, rest) {
        (_, 0) => format!("{hours} h"),
        (0, _) => format!("{rest} min"),
        _ => format!("{hours} h {rest:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }
    fn task(id: i64, start: &str, end: &str) -> Task {
        let mut t = Task::standalone();
        t.id = id;
        t.scheduled_start = start.into();
        t.scheduled_end = end.into();
        t
    }
    fn weekdays(day: NaiveDate) -> bool {
        use chrono::Datelike;
        day.weekday().num_days_from_monday() < 5
    }
    // Monday 2026-11-16 .. Sunday 2026-11-22.
    fn week(tasks: &[Task]) -> ([Load; 7], Load) {
        week_load(tasks, d(16), weekdays)
    }

    #[test]
    fn block_inside_the_window_counts_its_length() {
        let (days, total) = week(&[task(1, "2026-11-17T10:00", "2026-11-17T12:30")]);
        assert_eq!(days[1].planned_minutes, 150);
        assert_eq!(total.planned_minutes, 150);
        assert_eq!(total.capacity_minutes, 5 * 540);
        assert_eq!(days[0].planned_minutes, 0);
    }

    #[test]
    fn block_partly_outside_counts_only_the_window() {
        let (days, _) = week(&[task(1, "2026-11-17T17:00", "2026-11-17T20:00")]);
        assert_eq!(days[1].planned_minutes, 60);
        let (days, _) = week(&[task(1, "2026-11-17T07:00", "2026-11-17T10:00")]);
        assert_eq!(days[1].planned_minutes, 60);
    }

    #[test]
    fn block_entirely_outside_counts_its_real_length() {
        let (days, _) = week(&[task(1, "2026-11-17T19:00", "2026-11-17T21:00")]);
        assert_eq!(days[1].planned_minutes, 120);
    }

    #[test]
    fn multi_day_block_counts_on_each_day_it_touches() {
        let (days, total) = week(&[task(1, "2026-11-17T14:00", "2026-11-19T10:00")]);
        assert_eq!(days[1].planned_minutes, 240);
        assert_eq!(days[2].planned_minutes, 540);
        assert_eq!(days[3].planned_minutes, 60);
        assert_eq!(total.planned_minutes, 840);
    }

    #[test]
    fn overlapping_blocks_both_count_and_flag_the_day() {
        let tasks = [
            task(1, "2026-11-17T09:00", "2026-11-17T18:00"),
            task(2, "2026-11-17T10:00", "2026-11-17T12:00"),
        ];
        let (days, _) = week(&tasks);
        assert_eq!(days[1].planned_minutes, 660);
        assert!(days[1].is_over());
        assert!(!days[0].is_over());
    }

    #[test]
    fn exactly_the_capacity_is_not_over() {
        let (days, _) = week(&[task(1, "2026-11-17T09:00", "2026-11-17T18:00")]);
        assert!(!days[1].is_over());
    }

    #[test]
    fn holiday_week_has_reduced_capacity() {
        let holiday_wednesday = |day: NaiveDate| weekdays(day) && day != d(18);
        let (days, total) = week_load(&[], d(16), holiday_wednesday);
        assert_eq!(days[2].capacity_minutes, 0);
        assert_eq!(total.capacity_minutes, 4 * 540);
    }

    #[test]
    fn weekend_block_counts_but_is_not_flagged() {
        let (days, total) = week(&[task(1, "2026-11-21T10:00", "2026-11-21T14:00")]);
        assert_eq!(days[5].planned_minutes, 240);
        assert_eq!(days[5].capacity_minutes, 0);
        assert!(!days[5].is_over());
        assert_eq!(total.planned_minutes, 240);
    }

    #[test]
    fn a_block_spanning_a_weekend_counts_at_most_a_working_day_per_day() {
        let (days, total) = week(&[task(1, "2026-11-20T09:00", "2026-11-23T18:00")]);
        assert_eq!(days[4].planned_minutes, 540);
        assert_eq!(days[5].planned_minutes, 540);
        assert_eq!(days[6].planned_minutes, 540);
        assert_eq!(total.planned_minutes, 1620);
        let (days, _) = week(&[task(1, "2026-11-21T06:00", "2026-11-21T23:00")]);
        assert_eq!(days[5].planned_minutes, 540);
    }

    #[test]
    fn concluded_task_is_ignored() {
        let mut done = task(1, "2026-11-17T10:00", "2026-11-17T12:00");
        done.status = "concluida".into();
        let mut stamped = task(2, "2026-11-17T10:00", "2026-11-17T12:00");
        stamped.completed_at = Some("2026-11-17T12:00".into());
        let (_, total) = week(&[done, stamped]);
        assert_eq!(total.planned_minutes, 0);
    }

    #[test]
    fn invalid_empty_or_reversed_periods_are_ignored() {
        let tasks = [
            task(1, "", ""),
            task(2, "2026-11-17T10:00", ""),
            task(3, "lixo", "2026-11-17T12:00"),
            task(4, "2026-11-17T12:00", "2026-11-17T10:00"),
            task(5, "2026-11-17T10:00", "2026-11-17T10:00"),
            task(6, "2026-02-30T10:00", "2026-02-30T12:00"),
        ];
        let (_, total) = week(&tasks);
        assert_eq!(total.planned_minutes, 0);
    }

    #[test]
    fn a_parent_with_planned_subtasks_is_not_counted_twice() {
        let parent = task(1, "2026-11-17T09:00", "2026-11-17T18:00");
        let mut a = task(2, "2026-11-17T09:00", "2026-11-17T11:00");
        a.parent_task_id = Some(1);
        let mut b = task(3, "2026-11-17T13:00", "2026-11-17T14:00");
        b.parent_task_id = Some(1);
        let (days, _) = week(&[parent.clone(), a.clone(), b.clone()]);
        assert_eq!(days[1].planned_minutes, 180);
        // Once no subtask is open and planned, the parent counts again.
        a.status = "concluida".into();
        b.scheduled_end = String::new();
        let (days, _) = week(&[parent, a, b]);
        assert_eq!(days[1].planned_minutes, 540);
    }

    #[test]
    fn a_subtask_without_period_leaves_the_parent_counted() {
        let parent = task(1, "2026-11-17T09:00", "2026-11-17T12:00");
        let mut child = Task::standalone();
        child.id = 2;
        child.parent_task_id = Some(1);
        let (days, _) = week(&[parent, child]);
        assert_eq!(days[1].planned_minutes, 180);
    }

    #[test]
    fn hours_are_written_compactly() {
        assert_eq!(hours_label(0), "0 h");
        assert_eq!(hours_label(360), "6 h");
        assert_eq!(hours_label(90), "1 h 30");
        assert_eq!(hours_label(65), "1 h 05");
        assert_eq!(hours_label(45), "45 min");
    }
}
