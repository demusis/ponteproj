//! Deliveries at risk: open tasks with a deadline that the plan or the
//! projection does not meet, or that cannot be projected for lack of a
//! plan or an estimate. Pure and never persisted.
use crate::{projection, resources::parse_moment, tasks::Task};
use chrono::{NaiveDate, NaiveDateTime};
use std::collections::{HashMap, HashSet};

type Projections = HashMap<i64, (NaiveDateTime, NaiveDateTime)>;

/// Why a delivery is at risk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskKind {
    /// The planned period of the task itself ends after the deadline.
    LatePlanned,
    /// The projection (from the prerequisites) ends after the deadline.
    LateProjected,
    /// No projection: a prerequisite has no planned period.
    MissingPlan,
    /// No projection: a step has no estimate.
    MissingEstimate,
}

/// A delivery at risk and the task that needs attention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Risk {
    pub task_id: i64,
    pub title: String,
    /// The deadline, `AAAA-MM-DD`.
    pub due_date: String,
    pub kind: RiskKind,
    /// The planned or projected end (late kinds only).
    pub end: Option<NaiveDateTime>,
    /// The task to open: the delivery itself or the step that blocks it.
    pub cause_id: i64,
    pub cause_title: String,
}

/// The second line of a delivery at risk: why it is at risk.
pub fn reason(risk: &Risk) -> String {
    let due = match (risk.due_date.get(8..10), risk.due_date.get(5..7)) {
        (Some(day), Some(month)) => format!("{day}/{month}"),
        _ => risk.due_date.clone(),
    };
    let end = risk
        .end
        .map(|end| end.format("%d/%m").to_string())
        .unwrap_or_default();
    match risk.kind {
        RiskKind::LatePlanned => format!("fim planejado {end} passa do prazo {due}"),
        RiskKind::LateProjected => format!(
            "previsto até {end}, depois do prazo {due} · por causa de {}",
            risk.cause_title
        ),
        RiskKind::MissingPlan => format!("sem previsão: falta planejar {}", risk.cause_title),
        RiskKind::MissingEstimate => format!("sem previsão: falta estimar {}", risk.cause_title),
    }
}

/// Whether an end falls after the day of the deadline. An end at exactly
/// 00:00 counts as the previous day.
pub fn end_after_due(end: NaiveDateTime, due: NaiveDate) -> bool {
    due.succ_opt()
        .and_then(|next| next.and_hms_opt(0, 0, 0))
        .is_some_and(|limit| end > limit)
}

/// [`end_after_due`] on texts (`AAAA-MM-DDTHH:MM` and `AAAA-MM-DD`); invalid
/// texts are never late.
pub fn planned_end_after_due(end: &str, due: &str) -> bool {
    match (
        parse_moment(end),
        NaiveDate::parse_from_str(due, "%Y-%m-%d"),
    ) {
        (Ok(end), Ok(due)) => end_after_due(end, due),
        _ => false,
    }
}

/// The planned period of a task, when it has a valid one.
pub(crate) fn planned(task: &Task) -> Option<(NaiveDateTime, NaiveDateTime)> {
    match (
        parse_moment(&task.scheduled_start),
        parse_moment(&task.scheduled_end),
    ) {
        (Ok(start), Ok(end)) if end > start => Some((start, end)),
        _ => None,
    }
}

/// The deliveries at risk as of `today`; see [`deliveries_at_risk_with`].
pub fn deliveries_at_risk(tasks: &[Task], today: NaiveDate) -> Vec<Risk> {
    deliveries_at_risk_with(tasks, today, &projection::project(tasks))
}

/// The deliveries at risk as of `today`, with the projections already
/// computed by [`projection::project`] over the same `tasks`.
///
/// Considers every open task whose deadline is valid and not before
/// `today`:
/// - with a planned period: [`RiskKind::LatePlanned`] when it ends after the
///   deadline (the cause is the task itself);
/// - with a projection: [`RiskKind::LateProjected`] when it ends after the
///   deadline. The cause is the *direct* prerequisite whose end anchors the
///   projection, even when that prerequisite is itself projected;
/// - otherwise, when it has open prerequisites, the first open step (depth
///   first, cycle-safe) that is neither planned nor projected blocks it:
///   [`RiskKind::MissingPlan`] when it has no open prerequisites,
///   [`RiskKind::MissingEstimate`] when it has some but no estimate. A step
///   with an estimate and unanchored prerequisites is walked through. When
///   every prerequisite is anchored and the task itself has no estimate, the
///   task is the cause of a `MissingEstimate`.
///
/// Tasks with no plan and no open prerequisites are just unplanned, not at
/// risk. Late ones come first, then the missing ones, each by deadline.
pub fn deliveries_at_risk_with(
    tasks: &[Task],
    today: NaiveDate,
    projections: &Projections,
) -> Vec<Risk> {
    let by_id: HashMap<i64, &Task> = tasks.iter().map(|t| (t.id, t)).collect();
    let mut risks = Vec::new();
    for task in tasks {
        if task.is_done() {
            continue;
        }
        let Ok(due) = NaiveDate::parse_from_str(&task.due_date, "%Y-%m-%d") else {
            continue;
        };
        if due < today {
            continue;
        }
        let risk = |kind, end, cause: &Task| Risk {
            task_id: task.id,
            title: task.title.clone(),
            due_date: task.due_date.clone(),
            kind,
            end,
            cause_id: cause.id,
            cause_title: cause.title.clone(),
        };
        if let Some((_, end)) = planned(task) {
            if end_after_due(end, due) {
                risks.push(risk(RiskKind::LatePlanned, Some(end), task));
            }
        } else if !task.scheduled_start.is_empty() || !task.scheduled_end.is_empty() {
            // A broken plan is for the form to flag, not a risk.
        } else if let Some(&(_, end)) = projections.get(&task.id) {
            if end_after_due(end, due) {
                let cause = projection::anchor_indexed(task, &by_id, projections).unwrap_or(task);
                risks.push(risk(RiskKind::LateProjected, Some(end), cause));
            }
        } else if has_open_dependency(task, &by_id) {
            let mut seen = HashSet::from([task.id]);
            if let Some((cause, kind)) = blocking_step(task, &by_id, projections, &mut seen) {
                risks.push(risk(kind, None, cause));
            } else if task.estimate_minutes == 0 {
                risks.push(risk(RiskKind::MissingEstimate, None, task));
            }
        }
    }
    risks.sort_by(|a, b| {
        let missing =
            |r: &Risk| matches!(r.kind, RiskKind::MissingPlan | RiskKind::MissingEstimate);
        (missing(a), &a.due_date, a.task_id).cmp(&(missing(b), &b.due_date, b.task_id))
    });
    risks
}

fn open_dependencies<'a>(
    task: &Task,
    by_id: &HashMap<i64, &'a Task>,
) -> impl Iterator<Item = &'a Task> {
    task.dependencies
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .filter(|t| !t.is_done())
        .collect::<Vec<_>>()
        .into_iter()
}

fn has_open_dependency(task: &Task, by_id: &HashMap<i64, &Task>) -> bool {
    open_dependencies(task, by_id).next().is_some()
}

/// The first open prerequisite (depth first) with neither a planned period
/// nor a projection, and why it blocks. `seen` makes cycles end.
fn blocking_step<'a>(
    task: &Task,
    by_id: &HashMap<i64, &'a Task>,
    projections: &Projections,
    seen: &mut HashSet<i64>,
) -> Option<(&'a Task, RiskKind)> {
    for step in open_dependencies(task, by_id) {
        if planned(step).is_some() || projections.contains_key(&step.id) {
            continue;
        }
        if !seen.insert(step.id) {
            continue;
        }
        if !has_open_dependency(step, by_id) {
            return Some((step, RiskKind::MissingPlan));
        }
        if step.estimate_minutes == 0 {
            return Some((step, RiskKind::MissingEstimate));
        }
        if let Some(found) = blocking_step(step, by_id, projections, seen) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        parse_moment(s).unwrap()
    }
    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    const TODAY: &str = "2026-11-19";

    fn task(id: i64, deps: &[i64], estimate: u32, due: &str) -> Task {
        let mut t = Task::standalone();
        t.id = id;
        t.title = format!("T{id}");
        t.dependencies = deps.to_vec();
        t.estimate_minutes = estimate;
        t.due_date = due.into();
        t
    }
    fn plan(mut t: Task, start: &str, end: &str) -> Task {
        t.scheduled_start = start.into();
        t.scheduled_end = end.into();
        t
    }
    fn risks(tasks: &[Task]) -> Vec<Risk> {
        deliveries_at_risk(tasks, day(TODAY))
    }

    #[test]
    fn on_time_projection_is_not_a_risk() {
        let a = plan(task(1, &[], 0, ""), "2026-11-20T09:00", "2026-11-20T18:00");
        let b = task(2, &[1], 1440, "2026-11-30");
        assert!(risks(&[a, b]).is_empty());
    }

    #[test]
    fn late_projection_names_the_prerequisite_that_ends_last() {
        let a = plan(task(1, &[], 0, ""), "2026-11-20T09:00", "2026-11-20T18:00");
        let c = plan(task(3, &[], 0, ""), "2026-11-25T09:00", "2026-11-25T18:00");
        let b = task(2, &[1, 3], 1440, "2026-11-25");
        let found = risks(&[a, b, c]);
        assert_eq!(found.len(), 1);
        let r = &found[0];
        assert_eq!(r.task_id, 2);
        assert_eq!(r.title, "T2");
        assert_eq!(r.due_date, "2026-11-25");
        assert_eq!(r.kind, RiskKind::LateProjected);
        assert_eq!(r.end, Some(dt("2026-11-26T18:00")));
        assert_eq!((r.cause_id, r.cause_title.as_str()), (3, "T3"));
    }

    #[test]
    fn late_projection_cause_is_the_direct_prerequisite() {
        let a = plan(task(1, &[], 0, ""), "2026-11-25T09:00", "2026-11-25T18:00");
        let b = task(2, &[1], 1440, "");
        let c = task(3, &[2], 1440, "2026-11-26");
        let found = risks(&[a, b, c]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, RiskKind::LateProjected);
        assert_eq!(found[0].cause_id, 2);
    }

    #[test]
    fn late_planned_period_is_the_tasks_own_cause() {
        let t = plan(
            task(1, &[], 0, "2026-11-25"),
            "2026-11-25T09:00",
            "2026-11-26T18:00",
        );
        let found = risks(&[t]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, RiskKind::LatePlanned);
        assert_eq!(found[0].end, Some(dt("2026-11-26T18:00")));
        assert_eq!(found[0].cause_id, 1);
    }

    #[test]
    fn an_end_at_midnight_counts_as_the_previous_day() {
        let t = plan(
            task(1, &[], 0, "2026-11-25"),
            "2026-11-25T09:00",
            "2026-11-26T00:00",
        );
        assert!(risks(&[t]).is_empty());
        let t = plan(
            task(1, &[], 0, "2026-11-25"),
            "2026-11-25T09:00",
            "2026-11-26T00:01",
        );
        assert_eq!(risks(&[t]).len(), 1);
        assert!(planned_end_after_due("2026-11-04T18:00", "2026-11-03"));
        assert!(!planned_end_after_due("2026-11-04T00:00", "2026-11-03"));
        assert!(!planned_end_after_due("invalid", "2026-11-03"));
    }

    #[test]
    fn a_prerequisite_without_plan_blocks_the_projection() {
        let a = task(1, &[], 0, "");
        let b = task(2, &[1], 1440, "");
        let c = task(3, &[2], 1440, "2026-11-30");
        let found = risks(&[a, b, c]);
        // b has no due date: only c is listed, caused by the leaf a.
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].task_id, 3);
        assert_eq!(found[0].kind, RiskKind::MissingPlan);
        assert_eq!(found[0].end, None);
        assert_eq!(
            (found[0].cause_id, found[0].cause_title.as_str()),
            (1, "T1")
        );
    }

    #[test]
    fn a_prerequisite_without_estimate_blocks_the_projection() {
        let z = plan(task(9, &[], 0, ""), "2026-11-20T09:00", "2026-11-20T18:00");
        let a = task(1, &[9], 0, "");
        let b = task(2, &[1], 1440, "2026-11-30");
        let found = risks(&[z, a, b]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, RiskKind::MissingEstimate);
        assert_eq!(found[0].cause_id, 1);
    }

    #[test]
    fn a_task_without_estimate_after_planned_prerequisites_is_its_own_cause() {
        let z = plan(task(9, &[], 0, ""), "2026-11-20T09:00", "2026-11-20T18:00");
        let b = task(2, &[9], 0, "2026-11-30");
        let found = risks(&[z, b]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, RiskKind::MissingEstimate);
        assert_eq!(found[0].cause_id, 2);
    }

    #[test]
    fn unplanned_tasks_alone_are_not_risks() {
        let a = task(1, &[], 0, "2026-11-30");
        let b = task(2, &[], 1440, "");
        let c = task(3, &[1], 1440, "");
        assert!(risks(&[a, b, c]).is_empty());
    }

    #[test]
    fn cycles_do_not_panic_or_blame_anyone() {
        let a = task(1, &[2], 1440, "2026-11-30");
        let b = task(2, &[1], 1440, "2026-11-30");
        assert!(risks(&[a, b]).is_empty());
    }

    #[test]
    fn done_and_past_due_tasks_are_ignored() {
        let mut done = plan(
            task(1, &[], 0, "2026-11-25"),
            "2026-11-25T09:00",
            "2026-11-27T18:00",
        );
        done.status = "concluida".into();
        let past = plan(
            task(2, &[], 0, "2026-11-18"),
            "2026-11-18T09:00",
            "2026-11-20T18:00",
        );
        let after_done = task(3, &[1], 1440, "2026-11-30");
        assert!(risks(&[done, past, after_done]).is_empty());
    }

    #[test]
    fn late_ones_come_first_then_the_missing_ones_by_deadline() {
        let missing_early = task(1, &[], 0, "2026-11-20");
        let blocked_early = task(2, &[1], 1440, "2026-11-21");
        let late_later = plan(
            task(3, &[], 0, "2026-11-24"),
            "2026-11-24T09:00",
            "2026-11-25T18:00",
        );
        let late_sooner = plan(
            task(4, &[], 0, "2026-11-23"),
            "2026-11-23T09:00",
            "2026-11-24T18:00",
        );
        let found = risks(&[blocked_early, missing_early, late_later, late_sooner]);
        let ids: Vec<_> = found.iter().map(|r| r.task_id).collect();
        assert_eq!(ids, vec![4, 3, 2]);
    }

    #[test]
    fn precomputed_projections_give_the_same_answer() {
        let a = plan(task(1, &[], 0, ""), "2026-11-25T09:00", "2026-11-25T18:00");
        let b = task(2, &[1], 1440, "2026-11-25");
        let tasks = [a, b];
        let projections = projection::project(&tasks);
        assert_eq!(
            deliveries_at_risk_with(&tasks, day(TODAY), &projections),
            risks(&tasks)
        );
    }
}
