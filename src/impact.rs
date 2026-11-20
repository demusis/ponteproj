//! Unsaved planning changes and their effects; never persisted.
use crate::{
    projection,
    risk::{self, Risk, RiskKind},
    tasks::Task,
};
use chrono::{NaiveDate, NaiveDateTime};
use std::collections::HashMap;

type Period = (NaiveDateTime, NaiveDateTime);

#[derive(Clone, Debug)]
pub struct MovedTask {
    pub id: i64,
    pub title: String,
    pub before: Option<Period>,
    pub after: Option<Period>,
}

#[derive(Clone, Debug, Default)]
pub struct Impact {
    pub moved: Vec<MovedTask>,
    pub new_risks: Vec<Risk>,
    pub cleared_risks: Vec<Risk>,
}

impl Impact {
    pub fn is_empty(&self) -> bool {
        self.moved.is_empty() && self.new_risks.is_empty() && self.cleared_risks.is_empty()
    }
}

pub fn impact(saved: &[Task], edited: &Task, today: NaiveDate) -> Impact {
    let original = saved.iter().find(|task| task.id == edited.id);
    if original.is_some_and(|task| {
        task.scheduled_start == edited.scheduled_start
            && task.scheduled_end == edited.scheduled_end
            && task.estimate_minutes == edited.estimate_minutes
            && task.due_date == edited.due_date
            && task.status == edited.status
            && task.is_done() == edited.is_done()
            && task.dependencies == edited.dependencies
    }) {
        return Impact::default();
    }
    let mut changed = saved.to_vec();
    if let Some(task) = changed.iter_mut().find(|task| task.id == edited.id) {
        *task = edited.clone();
    } else {
        changed.push(edited.clone());
    }
    let before = projection::project(saved);
    let after = projection::project(&changed);
    let mut moved: Vec<_> = saved
        .iter()
        .filter(|task| task.id != edited.id)
        .filter_map(|task| {
            let old = before.get(&task.id).copied();
            let new = after.get(&task.id).copied();
            (old != new).then(|| MovedTask {
                id: task.id,
                title: task.title.clone(),
                before: old,
                after: new,
            })
        })
        .collect();
    moved.sort_by_key(|task| (task.after.is_none(), task.after.map(|p| p.0), task.id));
    let old_risks = risk::deliveries_at_risk_with(saved, today, &before);
    let new_risks = risk::deliveries_at_risk_with(&changed, today, &after);
    let old_by_id: HashMap<_, _> = old_risks.iter().map(|r| (r.task_id, r)).collect();
    let new_by_id: HashMap<_, _> = new_risks.iter().map(|r| (r.task_id, r)).collect();
    let missing = |kind| matches!(kind, RiskKind::MissingPlan | RiskKind::MissingEstimate);
    let late = |kind| matches!(kind, RiskKind::LatePlanned | RiskKind::LateProjected);
    let visible = |r: &&Risk| r.task_id != edited.id || r.kind != RiskKind::LatePlanned;
    let new_risks = new_risks
        .iter()
        .filter(|r| {
            old_by_id
                .get(&r.task_id)
                .is_none_or(|old| missing(old.kind) && late(r.kind))
        })
        .filter(visible)
        .cloned()
        .collect();
    let cleared_risks = old_risks
        .iter()
        .filter(|r| !new_by_id.contains_key(&r.task_id))
        .filter(visible)
        .cloned()
        .collect();
    Impact {
        moved,
        new_risks,
        cleared_risks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: i64, deps: &[i64], due: &str) -> Task {
        let mut t = Task::standalone();
        t.id = id;
        t.title = format!("T{id}");
        t.dependencies = deps.to_vec();
        t.estimate_minutes = 1440;
        t.due_date = due.into();
        t
    }
    fn plan(end: &str) -> Task {
        let mut t = task(1, &[], "");
        t.scheduled_start = "2026-11-19T09:00".into();
        t.scheduled_end = end.into();
        t
    }
    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
    }
    #[test]
    fn chain_moves_and_risk_appears_and_clears() {
        let a = plan("2026-11-19T18:00");
        let b = task(2, &[1], "2026-11-24");
        let c = task(3, &[2], "");
        let mut later = a.clone();
        later.scheduled_end = "2026-11-25T18:00".into();
        let result = impact(&[a.clone(), b.clone(), c.clone()], &later, day());
        assert_eq!(
            result.moved.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(result.new_risks[0].task_id, 2);
        let result = impact(&[later, b, c], &a, day());
        assert_eq!(result.cleared_risks[0].task_id, 2);
    }
    #[test]
    fn unchanged_new_and_invalid() {
        let a = plan("2026-11-19T18:00");
        let b = task(2, &[1], "2026-11-30");
        assert!(impact(&[a.clone(), b.clone()], &a, day()).is_empty());
        let new = task(3, &[], "");
        assert!(impact(&[a.clone(), b.clone()], &new, day()).is_empty());
        let mut invalid = a.clone();
        invalid.scheduled_end = "invalid".into();
        let result = impact(&[a, b], &invalid, day());
        assert!(result.moved[0].after.is_none());
    }
    #[test]
    fn missing_becomes_late() {
        let a = task(1, &[], "");
        let b = task(2, &[1], "2026-11-24");
        let planned = plan("2026-11-25T18:00");
        let result = impact(&[a, b], &planned, day());
        assert_eq!(result.new_risks[0].kind, RiskKind::LateProjected);
    }
}
