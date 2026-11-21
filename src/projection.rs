//! Working-day projections derived from task dependencies; never persisted.
use crate::{holidays::is_working_day, resources::parse_moment, tasks::Task};
use chrono::{NaiveDateTime, Timelike};
use std::collections::{HashMap, HashSet};

/// The planning working window, 09:00–18:00, on working days.
pub const WORK_START_HOUR: u32 = 9;
pub const WORK_END_HOUR: u32 = 18;

pub fn next_work_start(after: NaiveDateTime) -> NaiveDateTime {
    let mut date = after.date();
    if is_working_day(date) && after.time().num_seconds_from_midnight() < WORK_END_HOUR * 3600 {
        return after.max(date.and_hms_opt(WORK_START_HOUR, 0, 0).unwrap());
    }
    loop {
        date = date.succ_opt().unwrap();
        if is_working_day(date) {
            return date.and_hms_opt(WORK_START_HOUR, 0, 0).unwrap();
        }
    }
}

pub fn work_end(start: NaiveDateTime, days: u32) -> NaiveDateTime {
    assert!(days >= 1);
    let mut date = start.date();
    while !is_working_day(date) {
        date = date.succ_opt().unwrap();
    }
    for _ in 1..days {
        loop {
            date = date.succ_opt().unwrap();
            if is_working_day(date) {
                break;
            }
        }
    }
    date.and_hms_opt(WORK_END_HOUR, 0, 0).unwrap()
}

pub fn project(tasks: &[Task]) -> HashMap<i64, (NaiveDateTime, NaiveDateTime)> {
    fn visit(
        id: i64,
        by_id: &HashMap<i64, &Task>,
        visiting: &mut HashSet<i64>,
        memo: &mut HashMap<i64, Option<(NaiveDateTime, NaiveDateTime)>>,
    ) -> Option<(NaiveDateTime, NaiveDateTime)> {
        if let Some(result) = memo.get(&id) {
            return *result;
        }
        let task = *by_id.get(&id)?;
        if task.is_done()
            || !task.scheduled_start.is_empty()
            || !task.scheduled_end.is_empty()
            || task.estimate_minutes == 0
            || task.dependencies.is_empty()
        {
            return None;
        }
        if !visiting.insert(id) {
            return None;
        }
        let result = (|| {
            let mut latest = None;
            for dep in &task.dependencies {
                let previous = *by_id.get(dep)?;
                if previous.is_done() {
                    continue;
                }
                let end = match (
                    parse_moment(&previous.scheduled_start),
                    parse_moment(&previous.scheduled_end),
                ) {
                    (Ok(start), Ok(end)) if end > start => end,
                    _ => visit(*dep, by_id, visiting, memo)?.1,
                };
                latest = Some(latest.map_or(end, |value: NaiveDateTime| value.max(end)));
            }
            let start = next_work_start(latest?);
            Some((start, work_end(start, task.estimate_minutes.div_ceil(1440))))
        })();
        visiting.remove(&id);
        memo.insert(id, result);
        result
    }
    let by_id = tasks.iter().map(|t| (t.id, t)).collect();
    let mut memo = HashMap::new();
    for task in tasks {
        visit(task.id, &by_id, &mut HashSet::new(), &mut memo);
    }
    memo.into_iter()
        .filter_map(|(id, value)| value.map(|period| (id, period)))
        .collect()
}

/// The open prerequisite whose end anchors this projection.
pub fn anchor_title<'a>(
    task: &Task,
    tasks: &'a [Task],
    projections: &HashMap<i64, (NaiveDateTime, NaiveDateTime)>,
) -> Option<&'a str> {
    let by_id = tasks.iter().map(|task| (task.id, task)).collect();
    anchor_title_indexed(task, &by_id, projections)
}

pub(crate) fn anchor_title_indexed<'a>(
    task: &Task,
    by_id: &HashMap<i64, &'a Task>,
    projections: &HashMap<i64, (NaiveDateTime, NaiveDateTime)>,
) -> Option<&'a str> {
    anchor_indexed(task, by_id, projections).map(|t| t.title.as_str())
}

/// The open prerequisite (planned or projected) that ends last: the one
/// the projection of `task` starts after.
pub(crate) fn anchor_indexed<'a>(
    task: &Task,
    by_id: &HashMap<i64, &'a Task>,
    projections: &HashMap<i64, (NaiveDateTime, NaiveDateTime)>,
) -> Option<&'a Task> {
    task.dependencies
        .iter()
        .filter_map(|id| by_id.get(id).copied().filter(|t| !t.is_done()))
        .filter_map(|t| {
            let end = match (
                parse_moment(&t.scheduled_start),
                parse_moment(&t.scheduled_end),
            ) {
                (Ok(start), Ok(end)) if end > start => Some(end),
                _ => projections.get(&t.id).map(|p| p.1),
            }?;
            Some((end, t))
        })
        .max_by_key(|(end, _)| *end)
        .map(|(_, t)| t)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dt(s: &str) -> NaiveDateTime {
        parse_moment(s).unwrap()
    }
    fn task(id: i64, deps: &[i64]) -> Task {
        let mut t = Task::standalone();
        t.id = id;
        t.dependencies = deps.to_vec();
        t.estimate_minutes = 1440;
        t
    }
    #[test]
    fn working_days() {
        assert_eq!(
            next_work_start(dt("2026-11-20T18:00")),
            dt("2026-11-23T09:00")
        );
        assert_eq!(
            next_work_start(dt("2026-11-17T12:00")),
            dt("2026-11-17T12:00")
        );
        assert_eq!(
            next_work_start(dt("2026-11-21T10:00")),
            dt("2026-11-23T09:00")
        );
        assert_eq!(work_end(dt("2026-11-19T09:00"), 2), dt("2026-11-23T18:00"));
    }
    #[test]
    fn holidays_are_not_working_days() {
        // 21/4/2026 (Tiradentes) is a Tuesday.
        assert_eq!(
            next_work_start(dt("2026-04-20T18:00")),
            dt("2026-04-22T09:00")
        );
        assert_eq!(work_end(dt("2026-04-20T09:00"), 2), dt("2026-04-22T18:00"));
        // A start on a holiday moves to the next working day.
        assert_eq!(work_end(dt("2026-04-21T09:00"), 1), dt("2026-04-22T18:00"));
        // 20/11/2026 is Consciência Negra (Friday).
        assert_eq!(work_end(dt("2026-11-20T09:00"), 2), dt("2026-11-24T18:00"));
    }
    #[test]
    fn a_state_holiday_is_skipped_only_with_the_state_chosen() {
        let _guard = crate::holidays::ConfigGuard::new();
        // 9/7/2026 (SP) is a Thursday.
        assert_eq!(work_end(dt("2026-07-08T09:00"), 2), dt("2026-07-09T18:00"));
        crate::holidays::set_config(crate::holidays::HolidayConfig::parse(
            "uf SP
",
        ));
        assert_eq!(work_end(dt("2026-07-08T09:00"), 2), dt("2026-07-10T18:00"));
    }
    #[test]
    fn chain_across_carnaval() {
        // Carnaval 2026: Mon 16 and Tue 17 February.
        let mut a = task(1, &[]);
        a.scheduled_start = "2026-02-13T09:00".into();
        a.scheduled_end = "2026-02-13T18:00".into();
        let b = task(2, &[1]);
        let c = task(3, &[2]);
        let result = project(&[a, b, c]);
        assert_eq!(result[&2], (dt("2026-02-18T09:00"), dt("2026-02-18T18:00")));
        assert_eq!(result[&3], (dt("2026-02-19T09:00"), dt("2026-02-19T18:00")));
    }
    #[test]
    fn chain_and_exclusions() {
        let mut a = task(1, &[]);
        a.scheduled_start = "2026-11-20T09:00".into();
        a.scheduled_end = "2026-11-20T18:00".into();
        let b = task(2, &[1]);
        let c = task(3, &[2]);
        let mut no_estimate = task(4, &[1]);
        no_estimate.estimate_minutes = 0;
        let result = project(&[a, b, c, no_estimate]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[&2], (dt("2026-11-23T09:00"), dt("2026-11-23T18:00")));
        assert_eq!(result[&3], (dt("2026-11-24T09:00"), dt("2026-11-24T18:00")));
    }
    #[test]
    fn cycles_missing_and_done() {
        let mut done = task(3, &[]);
        done.status = "concluida".into();
        assert!(project(&[
            task(1, &[2]),
            task(2, &[1]),
            done,
            task(4, &[3]),
            task(5, &[99])
        ])
        .is_empty());
    }
}
