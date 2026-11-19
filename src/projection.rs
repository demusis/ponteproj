//! Working-day projections derived from task dependencies; never persisted.
use crate::{resources::parse_moment, tasks::Task};
use chrono::{Datelike, NaiveDateTime, Timelike, Weekday};
use std::collections::{HashMap, HashSet};

fn weekday(t: NaiveDateTime) -> bool {
    !matches!(t.weekday(), Weekday::Sat | Weekday::Sun)
}

pub fn next_work_start(after: NaiveDateTime) -> NaiveDateTime {
    let mut date = after.date();
    if weekday(after) && after.time().num_seconds_from_midnight() < 18 * 3600 {
        return after.max(date.and_hms_opt(9, 0, 0).unwrap());
    }
    loop {
        date = date.succ_opt().unwrap();
        let next = date.and_hms_opt(9, 0, 0).unwrap();
        if weekday(next) {
            return next;
        }
    }
}

pub fn work_end(start: NaiveDateTime, days: u32) -> NaiveDateTime {
    assert!(days >= 1);
    let mut date = start.date();
    while !weekday(date.and_hms_opt(9, 0, 0).unwrap()) {
        date = date.succ_opt().unwrap();
    }
    for _ in 1..days {
        loop {
            date = date.succ_opt().unwrap();
            if weekday(date.and_hms_opt(9, 0, 0).unwrap()) {
                break;
            }
        }
    }
    date.and_hms_opt(18, 0, 0).unwrap()
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
    task.dependencies
        .iter()
        .filter_map(|id| tasks.iter().find(|t| t.id == *id && !t.is_done()))
        .filter_map(|t| {
            let end = match (
                parse_moment(&t.scheduled_start),
                parse_moment(&t.scheduled_end),
            ) {
                (Ok(start), Ok(end)) if end > start => Some(end),
                _ => projections.get(&t.id).map(|p| p.1),
            }?;
            Some((end, t.title.as_str()))
        })
        .max_by_key(|(end, _)| *end)
        .map(|(_, title)| title)
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
        assert_eq!(work_end(dt("2026-11-20T09:00"), 2), dt("2026-11-23T18:00"));
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
