//! Projects ("Projetos"): an optional grouping of tasks.
//!
//! A project has a name, an objective, an assignee, a status and, optionally,
//! a start and a target date. A task belongs to at most one project, through
//! `Task::project_uid` (empty: no project). Everything a screen shows about a
//! project (progress, risk, timeline) is computed here from the tasks, never
//! stored.
//!
//! Like the categories, the projects of a library are shared by every
//! computer and phone that syncs it, and a project is addressed by a random
//! `uid` (32 hexadecimal digits, the same on every device), never by row
//! number, so an offline change needs no number translation for it:
//!
//! - saving is [`Store::save_project`] (a new project has an empty uid);
//!   archiving is a save with the status `arquivado`;
//! - [`Store::delete_project`] never deletes tasks: the tasks of the project
//!   go back to "no project" in the same transaction (and in the same
//!   replicated change, `DeleteProject`);
//! - when a task is saved with the uid of a project that does not exist, the
//!   field is cleared, as an unknown category is dropped; a task read from
//!   the library never shows the uid of a project that is gone either;
//! - a NEW subtask under a parent takes the project of the parent unless it
//!   comes with one; changing the project of a parent later does not rewrite
//!   its subtasks.
//!
//! Conflicts follow the categories: the change applied last wins. A save made
//! on top of an older version of the project is a conflict to review (the
//! edit is not lost and not forced over the newer one); a project deleted on
//! another computer is never brought back by an edit made from an older view
//! of it; a task that arrives pointing at a project deleted meanwhile ends
//! with no project.
//!
//! The name, objective and assignee live only in the sealed payload; in clear
//! there is the uid.
use crate::{
    intranet::{record_mutation, IntranetMutation},
    protection::{seal, unseal},
    risk::{self, Risk},
    storage::Store,
    tasks::Task,
    work_templates::version_key,
};
use anyhow::{bail, ensure, Context, Result};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Longest name, in characters.
pub const MAX_NAME: usize = 120;
/// Longest objective, in bytes.
pub const MAX_OBJECTIVE: usize = 8000;
/// Most projects a library may have (the person in the interface is refused
/// past it; changes replayed from other computers are not).
pub const MAX_PROJECTS: usize = 500;

/// Where a project stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    #[default]
    Ativo,
    Pausado,
    Concluido,
    Arquivado,
}

impl ProjectStatus {
    /// Every status, in the order the screens offer them.
    pub const ALL: [ProjectStatus; 4] = [
        ProjectStatus::Ativo,
        ProjectStatus::Pausado,
        ProjectStatus::Concluido,
        ProjectStatus::Arquivado,
    ];

    /// The stored and exchanged name: `ativo`, `pausado`, `concluido`,
    /// `arquivado`.
    pub fn key(self) -> &'static str {
        match self {
            ProjectStatus::Ativo => "ativo",
            ProjectStatus::Pausado => "pausado",
            ProjectStatus::Concluido => "concluido",
            ProjectStatus::Arquivado => "arquivado",
        }
    }

    /// The word the screens show.
    pub fn label(self) -> &'static str {
        match self {
            ProjectStatus::Ativo => "Ativo",
            ProjectStatus::Pausado => "Pausado",
            ProjectStatus::Concluido => "Concluído",
            ProjectStatus::Arquivado => "Arquivado",
        }
    }

    /// The status with this key (ignoring case and the accent of
    /// "concluído"); `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let key = text.trim().to_lowercase();
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key || s.label().to_lowercase() == key)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// 32 hexadecimal digits; empty for a project not saved yet.
    #[serde(default)]
    pub uid: String,
    pub name: String,
    #[serde(default)]
    pub objective: String,
    /// Who answers for the project; same conventions as `Task::assignee`.
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub status: ProjectStatus,
    /// `AAAA-MM-DD` or empty.
    #[serde(default)]
    pub start_date: String,
    /// `AAAA-MM-DD` or empty; never before `start_date` when both are set.
    #[serde(default)]
    pub target_date: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn valid_uid(uid: &str) -> bool {
    uid.len() == 32
        && uid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether `uid` is a well-formed project uid (32 hexadecimal digits).
pub fn is_valid_uid(uid: &str) -> bool {
    valid_uid(uid)
}

fn parse_day(text: &str) -> Option<NaiveDate> {
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    (date.format("%Y-%m-%d").to_string() == text).then_some(date)
}

impl Project {
    /// A new project as the form proposes it (no uid yet, status ativo).
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    /// The text fields as they are kept: name with inner spaces collapsed,
    /// the other texts trimmed.
    pub fn normalized(&self) -> Self {
        let mut copy = self.clone();
        copy.name = self.name.split_whitespace().collect::<Vec<_>>().join(" ");
        copy.objective = self.objective.trim().to_owned();
        copy.assignee = self.assignee.trim().to_owned();
        copy.start_date = self.start_date.trim().to_owned();
        copy.target_date = self.target_date.trim().to_owned();
        copy
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "Informe o nome do projeto.");
        ensure!(
            self.name.chars().count() <= MAX_NAME,
            "O nome do projeto pode ter no máximo {MAX_NAME} caracteres."
        );
        ensure!(
            !self.name.chars().any(char::is_control),
            "O nome do projeto tem um caractere inválido."
        );
        ensure!(
            self.objective.len() <= MAX_OBJECTIVE
                && !self.objective.contains('\0')
                && !self
                    .objective
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
            "O objetivo do projeto é longo demais ou tem um caractere inválido."
        );
        ensure!(
            self.assignee.len() <= 256 && !self.assignee.chars().any(char::is_control),
            "O nome do responsável é inválido ou longo demais."
        );
        ensure!(
            self.uid.is_empty() || valid_uid(&self.uid),
            "A identificação do projeto é inválida."
        );
        let start = if self.start_date.is_empty() {
            None
        } else {
            Some(
                parse_day(&self.start_date)
                    .context("Data de início inválida. Use o formato dd/mm/aaaa.")?,
            )
        };
        let target = if self.target_date.is_empty() {
            None
        } else {
            Some(
                parse_day(&self.target_date)
                    .context("Data-alvo inválida. Use o formato dd/mm/aaaa.")?,
            )
        };
        if let (Some(start), Some(target)) = (start, target) {
            ensure!(
                target >= start,
                "A data-alvo do projeto não pode ser anterior ao início."
            );
        }
        Ok(())
    }

    /// Whether everything the person can edit is the same (the stamps are
    /// the library's).
    pub fn same_content(&self, other: &Self) -> bool {
        self.name == other.name
            && self.objective == other.objective
            && self.assignee == other.assignee
            && self.status == other.status
            && self.start_date == other.start_date
            && self.target_date == other.target_date
    }

    /// The key two names are compared by: ignoring case and accents.
    pub fn name_key(&self) -> String {
        crate::labels::fold(&self.name)
    }

    /// The target date, when it is set.
    pub fn target(&self) -> Option<NaiveDate> {
        parse_day(&self.target_date)
    }
}

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

/// How far a project is, counted over its tasks (subtasks included).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Progress {
    /// Tasks of the project.
    pub total: usize,
    /// Concluded tasks.
    pub done: usize,
    /// Open tasks whose due date is before today.
    pub late: usize,
    /// The earliest due date (`AAAA-MM-DD`) among the open tasks that have
    /// one, late ones included.
    pub next_due: Option<String>,
}

fn belongs(task: &Task, project_uid: &str) -> bool {
    !project_uid.is_empty() && task.project_uid == project_uid
}

/// Progress of the project `project_uid` over `tasks` as of `today`.
pub fn progress(project_uid: &str, tasks: &[Task], today: NaiveDate) -> Progress {
    let mut out = Progress::default();
    for task in tasks.iter().filter(|t| belongs(t, project_uid)) {
        out.total += 1;
        if task.is_done() {
            out.done += 1;
            continue;
        }
        let Some(due) = parse_day(&task.due_date) else {
            continue;
        };
        if due < today {
            out.late += 1;
        }
        if out
            .next_due
            .as_deref()
            .is_none_or(|current| task.due_date.as_str() < current)
        {
            out.next_due = Some(task.due_date.clone());
        }
    }
    out
}

/// What threatens the delivery of a project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProjectRisk {
    /// Deliveries of the project at risk (see `risk::deliveries_at_risk`).
    pub deliveries_at_risk: usize,
    /// The latest planned-or-projected end among the open tasks of the
    /// project, when it falls after the target date of the project.
    pub target_at_risk: Option<NaiveDateTime>,
}

/// The risk of `project` over ALL the `tasks` of the library: prerequisites
/// may live outside the project, so the deliveries at risk and the
/// projections are computed over everything and then filtered to the tasks
/// of the project.
pub fn project_risk(project: &Project, tasks: &[Task], today: NaiveDate) -> ProjectRisk {
    let projections = crate::projection::project(tasks);
    let risks = risk::deliveries_at_risk_with(tasks, today, &projections);
    risk_with(project, tasks, &projections, &risks)
}

fn risk_with(
    project: &Project,
    tasks: &[Task],
    projections: &HashMap<i64, (NaiveDateTime, NaiveDateTime)>,
    risks: &[Risk],
) -> ProjectRisk {
    let mine: HashSet<i64> = tasks
        .iter()
        .filter(|t| belongs(t, &project.uid))
        .map(|t| t.id)
        .collect();
    let deliveries = risks.iter().filter(|r| mine.contains(&r.task_id)).count();
    let end = tasks
        .iter()
        .filter(|t| belongs(t, &project.uid) && !t.is_done())
        .filter_map(|t| {
            risk::planned(t)
                .map(|(_, end)| end)
                .or_else(|| projections.get(&t.id).map(|p| p.1))
        })
        .max();
    let target_at_risk = match (end, project.target()) {
        (Some(end), Some(target)) if risk::end_after_due(end, target) => Some(end),
        _ => None,
    };
    ProjectRisk {
        deliveries_at_risk: deliveries,
        target_at_risk,
    }
}

/// One bar of the timeline of a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineRow {
    pub id: i64,
    pub uid: String,
    pub title: String,
    pub done: bool,
    /// The planned period, or the one projected from the prerequisites;
    /// `None` when the task has neither.
    pub period: Option<(NaiveDateTime, NaiveDateTime)>,
    /// The period came from the projection and not from a fixed plan.
    pub projected: bool,
    pub due: Option<NaiveDate>,
    /// The period ends after the due date.
    pub late: bool,
}

/// The tasks of the project as rows of a timeline, in a stable order: start
/// (the ones with a period first), then due date, then title (ignoring case),
/// then id. Projections are computed over all of `tasks`.
pub fn timeline(project_uid: &str, tasks: &[Task]) -> Vec<TimelineRow> {
    let projections = crate::projection::project(tasks);
    let mut rows: Vec<TimelineRow> = tasks
        .iter()
        .filter(|t| belongs(t, project_uid))
        .map(|task| {
            let planned = risk::planned(task);
            let projected = planned.is_none() && projections.contains_key(&task.id);
            let period = planned.or_else(|| projections.get(&task.id).copied());
            let due = parse_day(&task.due_date);
            let late = match (period, due) {
                (Some((_, end)), Some(due)) => risk::end_after_due(end, due),
                _ => false,
            };
            TimelineRow {
                id: task.id,
                uid: task.uid.clone(),
                title: task.title.clone(),
                done: task.is_done(),
                period,
                projected,
                due,
                late,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        let key = |r: &TimelineRow| {
            (
                r.period.is_none(),
                r.period.map(|p| p.0),
                r.due.is_none(),
                r.due,
                r.title.to_lowercase(),
                r.id,
            )
        };
        key(a).cmp(&key(b))
    });
    rows
}

/// The overall span of a timeline, so a screen can scale its bars: the
/// earliest start and the latest end, where a row with a due date also counts
/// from the start of that day to its end (midnight of the next day), and a row
/// with no period counts only through its due date. `None` for an empty
/// timeline or one with neither periods nor due dates.
pub fn timeline_span(rows: &[TimelineRow]) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let mut span: Option<(NaiveDateTime, NaiveDateTime)> = None;
    let mut widen = |from: NaiveDateTime, to: NaiveDateTime| {
        span = Some(match span {
            Some((a, b)) => (a.min(from), b.max(to)),
            None => (from, to),
        });
    };
    for row in rows {
        if let Some((start, end)) = row.period {
            widen(start, end);
        }
        if let Some(due) = row.due {
            let from = due.and_hms_opt(0, 0, 0).expect("midnight exists");
            widen(from, from + Duration::days(1));
        }
    }
    span
}

/// A project with the numbers of the screens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectOverview {
    pub project: Project,
    pub progress: Progress,
    pub risk: ProjectRisk,
}

// ---------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------

fn decode<T: for<'a> Deserialize<'a>>(blob: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(&unseal(blob)?)?)
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    seal(&serde_json::to_vec(value)?)
}

fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Every project with its uid, by name; read inside a transaction or not.
fn read_all(conn: &Connection) -> Result<Vec<Project>> {
    let mut statement = conn.prepare("SELECT uid,payload FROM projects")?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (uid, blob) in rows {
        let mut project: Project =
            decode(&blob).context("Não foi possível ler um projeto: o registro está inválido.")?;
        project.uid = uid;
        out.push(project);
    }
    out.sort_by(|a, b| {
        a.name_key()
            .cmp(&b.name_key())
            .then_with(|| a.uid.cmp(&b.uid))
    });
    Ok(out)
}

/// The rules between projects: at most [`MAX_PROJECTS`], and no two with the
/// same name ignoring case and accents. `enforce_max` is off when replaying
/// changes made elsewhere.
fn check_unique(all: &[Project], project: &Project, enforce_max: bool) -> Result<()> {
    let key = project.name_key();
    if all
        .iter()
        .any(|p| p.uid != project.uid && p.name_key() == key)
    {
        bail!("Já existe um projeto com este nome.");
    }
    if enforce_max && !all.iter().any(|p| p.uid == project.uid) && all.len() >= MAX_PROJECTS {
        bail!("Já existem {MAX_PROJECTS} projetos, o máximo permitido. Apague algum antes de criar outro.");
    }
    Ok(())
}

/// The uid when the project exists in the library, empty otherwise. Used
/// inside every write of a task.
pub(crate) fn retain_known(conn: &Connection, uid: &str) -> Result<String> {
    if uid.is_empty() {
        return Ok(String::new());
    }
    let known = conn
        .query_row("SELECT 1 FROM projects WHERE uid=?1", [uid], |_| Ok(()))
        .optional()?
        .is_some();
    Ok(if known { uid.to_owned() } else { String::new() })
}

/// The project a stored task has, read from its payload (for a new subtask
/// to inherit); empty when the parent is unknown or has none.
pub(crate) fn project_of_task(conn: &Connection, task_id: i64) -> Result<String> {
    let blob: Option<Vec<u8>> = conn
        .query_row("SELECT payload FROM tasks WHERE id=?1", [task_id], |r| {
            r.get(0)
        })
        .optional()?;
    match blob {
        Some(blob) => Ok(decode::<Task>(&blob)?.project_uid),
        None => Ok(String::new()),
    }
}

impl Store {
    pub(crate) fn ensure_projects(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects(
                id INTEGER PRIMARY KEY,
                uid TEXT NOT NULL UNIQUE,
                payload BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    // -- reads --------------------------------------------------------------

    /// Every project (archived ones too), by name ignoring case and accents,
    /// memoized until the library changes.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        self.cached("projects", || read_all(&self.conn))
    }

    /// The project with this uid, if it exists.
    pub fn get_project(&self, uid: &str) -> Result<Option<Project>> {
        Ok(self.list_projects()?.into_iter().find(|p| p.uid == uid))
    }

    /// The version a screen hands back as `expected_version`; 0 when new.
    pub fn project_version(&self, uid: &str) -> Result<i64> {
        Ok(self.item_version("project", version_key(uid))?.version)
    }

    pub(crate) fn project_exists(&self, uid: &str) -> bool {
        self.conn
            .query_row("SELECT 1 FROM projects WHERE uid=?1", [uid], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    /// `uid` when the project exists, empty otherwise.
    pub(crate) fn known_project_of(&self, uid: &str) -> Result<String> {
        if uid.is_empty() {
            return Ok(String::new());
        }
        Ok(if self.list_projects()?.iter().any(|p| p.uid == uid) {
            uid.to_owned()
        } else {
            String::new()
        })
    }

    /// The tasks of a project (active ones, as `list_tasks` gives them).
    pub fn project_tasks(&self, uid: &str) -> Result<Vec<Task>> {
        Ok(self
            .list_tasks()?
            .into_iter()
            .filter(|t| belongs(t, uid))
            .collect())
    }

    /// Every project with its progress and risk as of `today`, by name.
    /// Reads the tasks once and computes the projections once.
    pub fn project_overview(&self, today: NaiveDate) -> Result<Vec<ProjectOverview>> {
        let tasks = self.list_tasks()?;
        let projections = crate::projection::project(&tasks);
        let risks = risk::deliveries_at_risk_with(&tasks, today, &projections);
        Ok(self
            .list_projects()?
            .into_iter()
            .map(|project| ProjectOverview {
                progress: progress(&project.uid, &tasks, today),
                risk: risk_with(&project, &tasks, &projections, &risks),
                project,
            })
            .collect())
    }

    // -- writes ---------------------------------------------------------------

    /// Creates (empty uid) or changes a project; returns it as stored.
    pub fn save_project(&mut self, project: &Project) -> Result<Project> {
        self.save_project_checked(project, None)
    }

    /// The same, refusing a change made on top of an older version of the
    /// project (`expected_version`, from [`Store::project_version`]).
    pub fn save_project_checked(
        &mut self,
        project: &Project,
        expected_version: Option<i64>,
    ) -> Result<Project> {
        self.save_project_inner(project, expected_version, true)
    }

    /// Applies a project change that came from another computer (main
    /// computer, offline queue, shared folder). A project, new or edited,
    /// whose name another project has is stored as "Nome (2)", "Nome (3)"…,
    /// and the limit of projects is not enforced, so nothing sent later is
    /// lost.
    pub(crate) fn save_project_applied(
        &mut self,
        project: &Project,
        expected_version: Option<i64>,
    ) -> Result<Project> {
        let mut project = project.normalized();
        project.validate()?;
        if !project.uid.is_empty() {
            let all = read_all(&self.conn)?;
            let own_uid = project.uid.clone();
            let taken_by_other = |name_key: &str| {
                all.iter()
                    .any(|p| p.uid != own_uid && p.name_key() == name_key)
            };
            if taken_by_other(&project.name_key()) {
                let base = project.name.clone();
                for n in 2.. {
                    let suffix = format!(" ({n})");
                    let room = MAX_NAME - suffix.chars().count();
                    let head: String = base.chars().take(room).collect();
                    let mut candidate = project.clone();
                    candidate.name = format!("{}{suffix}", head.trim_end());
                    if !taken_by_other(&candidate.name_key()) {
                        project = candidate;
                        break;
                    }
                }
            }
        }
        self.save_project_inner(&project, expected_version, false)
    }

    fn save_project_inner(
        &mut self,
        project: &Project,
        expected_version: Option<i64>,
        enforce_max: bool,
    ) -> Result<Project> {
        let mut project = project.normalized();
        if project.uid.is_empty() {
            project.uid = new_uid();
        }
        project.validate()?;
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_projects(&cfg)?;
            // The rules are checked here first, so a refusal is told now and
            // not found in the queue of changes later.
            check_unique(&read_all(&self.conn)?, &project, enforce_max)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveProject {
                    project: project.clone(),
                    expected_version,
                },
            )?;
            return Ok(project);
        }
        let actor = self.active_actor();
        let timestamp = now_stamp();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let all = read_all(&tx)?;
        check_unique(&all, &project, enforce_max)?;
        let previous = all.iter().find(|p| p.uid == project.uid);
        if previous.is_none() {
            // A project that was deleted here (its version is kept) is never
            // brought back by a change made from an older view of it.
            let was = crate::intranet::version_of(&tx, "project", version_key(&project.uid))?;
            ensure!(
                was == 0,
                "Projeto não encontrado (já apagado?). Crie-o de novo, se ainda for necessário."
            );
        }
        let mut stored = project.clone();
        stored.updated_at = timestamp.clone();
        let details = match previous {
            Some(before) => {
                stored.created_at = before.created_at.clone();
                tx.execute(
                    "UPDATE projects SET payload=?1,updated_at=?2 WHERE uid=?3",
                    params![encode(&stored)?, timestamp, stored.uid],
                )?;
                if before.status != stored.status {
                    format!(
                        "Mudou o projeto \"{}\" para {}",
                        stored.name,
                        stored.status.label().to_lowercase()
                    )
                } else {
                    format!("Alterou o projeto \"{}\"", stored.name)
                }
            }
            None => {
                stored.created_at = timestamp.clone();
                tx.execute(
                    "INSERT INTO projects(uid,payload,updated_at) VALUES(?1,?2,?3)",
                    params![stored.uid, encode(&stored)?, timestamp],
                )?;
                format!("Cadastrou o projeto \"{}\"", stored.name)
            }
        };
        record_mutation(
            &tx,
            &actor,
            "project",
            version_key(&stored.uid),
            expected_version,
            "save_project",
            None,
            &details,
        )?;
        tx.commit()?;
        Ok(stored)
    }

    /// Deletes a project. Its tasks are NOT deleted: they go back to no
    /// project, in the same transaction.
    pub fn delete_project(&mut self, uid: &str) -> Result<()> {
        let version = self.project_version(uid).ok().filter(|v| *v > 0);
        self.delete_project_checked(uid, version)
    }

    pub fn delete_project_checked(
        &mut self,
        uid: &str,
        expected_version: Option<i64>,
    ) -> Result<()> {
        ensure!(valid_uid(uid), "A identificação do projeto é inválida.");
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_projects(&cfg)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::DeleteProject {
                    uid: uid.to_owned(),
                    expected_version,
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let project = read_all(&tx)?
            .into_iter()
            .find(|p| p.uid == uid)
            .context("Projeto não encontrado (já apagado?).")?;
        tx.execute("DELETE FROM projects WHERE uid=?1", [uid])?;
        // Tasks in the trash cannot be edited and are left as they are: the
        // uid they keep means nothing once the project is gone, and a task
        // read from the library never shows it.
        let tasks = tx
            .prepare("SELECT t.id,t.payload FROM tasks t WHERE NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='task' AND x.item_id=t.id) AND NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='document' AND x.item_id=t.document_id)")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, blob) in tasks {
            let mut task: Task = decode(&blob)?;
            if task.project_uid == uid {
                task.project_uid.clear();
                tx.execute(
                    "UPDATE tasks SET payload=?1 WHERE id=?2",
                    params![encode(&task)?, id],
                )?;
            }
        }
        record_mutation(
            &tx,
            &actor,
            "project",
            version_key(uid),
            expected_version,
            "delete_project",
            None,
            &format!("Apagou o projeto \"{}\"", project.name),
        )?;
        tx.commit()?;
        Ok(())
    }

    // -- team sync --------------------------------------------------------------

    pub(crate) fn export_projects(&self) -> Result<Vec<Project>> {
        let mut statement = self
            .conn
            .prepare("SELECT uid,payload FROM projects ORDER BY id")?;
        let rows = statement
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (uid, blob) in rows {
            let mut project: Project = decode(&blob)?;
            project.uid = uid;
            out.push(project);
        }
        Ok(out)
    }
}

/// Writes the table from a snapshot (the table was emptied by the caller),
/// re-sealing the payloads for this Windows account.
pub(crate) fn apply_projects(tx: &Connection, projects: &[Project]) -> Result<()> {
    for project in projects {
        tx.execute(
            "INSERT OR REPLACE INTO projects(uid,payload,updated_at) VALUES(?1,?2,?3)",
            params![project.uid, encode(project)?, project.updated_at],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod rules_tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }
    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M").unwrap()
    }
    const P: &str = "11111111111111111111111111111111";
    const OTHER: &str = "22222222222222222222222222222222";

    fn task(id: i64, title: &str, project: &str, due: &str) -> Task {
        let mut t = Task::standalone();
        t.id = id;
        t.uid = format!("00000000-0000-4000-8000-{id:012}");
        t.title = title.into();
        t.project_uid = project.into();
        t.due_date = due.into();
        t
    }
    fn planned(mut t: Task, start: &str, end: &str) -> Task {
        t.scheduled_start = start.into();
        t.scheduled_end = end.into();
        t
    }
    fn project(target: &str) -> Project {
        Project {
            uid: P.into(),
            name: "Reforma".into(),
            target_date: target.into(),
            ..Project::default()
        }
    }

    #[test]
    fn status_keys_and_labels_round_trip() {
        for status in ProjectStatus::ALL {
            assert_eq!(ProjectStatus::parse(status.key()), Some(status));
            assert_eq!(ProjectStatus::parse(status.label()), Some(status));
        }
        assert_eq!(ProjectStatus::Concluido.label(), "Concluído");
        assert_eq!(ProjectStatus::parse(" ATIVO "), Some(ProjectStatus::Ativo));
        assert_eq!(ProjectStatus::parse("feito"), None);
        let json = serde_json::to_string(&ProjectStatus::Arquivado).unwrap();
        assert_eq!(json, "\"arquivado\"");
    }

    #[test]
    fn validation_messages_and_rules() {
        let ok = |p: &Project| p.validate().map_err(|e| e.to_string());
        assert_eq!(
            ok(&Project::new("   ")),
            Err("Informe o nome do projeto.".into())
        );
        assert!(ok(&Project::new(&"x".repeat(MAX_NAME))).is_ok());
        assert!(ok(&Project::new(&"x".repeat(MAX_NAME + 1)))
            .unwrap_err()
            .contains("120"));
        assert!(ok(&Project::new("a\u{7}b")).is_err());
        let mut p = Project::new("Ok");
        p.assignee = "x".repeat(257);
        assert!(ok(&p).unwrap_err().contains("responsável"));
        p.assignee = "Ana".into();
        p.objective = "linha 1\nlinha 2".into();
        assert!(ok(&p).is_ok());
        p.objective = "x".repeat(MAX_OBJECTIVE + 1);
        assert!(ok(&p).unwrap_err().contains("objetivo"));
        p.objective.clear();
        p.start_date = "2026-13-01".into();
        assert!(ok(&p).unwrap_err().contains("início"));
        p.start_date = "2026-05-10".into();
        p.target_date = "2026-5-12".into();
        assert!(ok(&p).unwrap_err().contains("alvo"));
        p.target_date = "2026-05-09".into();
        assert!(ok(&p).unwrap_err().contains("anterior"));
        p.target_date = "2026-05-10".into();
        assert!(ok(&p).is_ok());
        p.target_date.clear();
        assert!(ok(&p).is_ok());
        p.uid = "abc".into();
        assert!(ok(&p).unwrap_err().contains("identificação"));
    }

    #[test]
    fn normalizing_collapses_spaces_and_trims() {
        let mut p = Project::new("  Reforma   do   prédio ");
        p.objective = "  texto \n".into();
        p.assignee = " Ana ".into();
        let n = p.normalized();
        assert_eq!(n.name, "Reforma do prédio");
        assert_eq!(n.objective, "texto");
        assert_eq!(n.assignee, "Ana");
        assert_eq!(
            Project::new("Orientação").name_key(),
            Project::new("ORIENTACAO ").name_key()
        );
    }

    #[test]
    fn progress_counts_done_late_and_the_next_due_date() {
        let mut done = task(1, "feita", P, "2026-01-01");
        done.status = "concluida".into();
        let tasks = vec![
            done,
            task(2, "atrasada", P, "2026-11-10"),
            task(3, "hoje", P, "2026-11-19"),
            task(4, "depois", P, "2026-12-01"),
            task(5, "sem prazo", P, ""),
            task(6, "de outro", OTHER, "2026-11-01"),
            task(7, "sem projeto", "", "2026-11-01"),
        ];
        let p = progress(P, &tasks, day("2026-11-19"));
        assert_eq!(
            p,
            Progress {
                total: 5,
                done: 1,
                late: 1,
                next_due: Some("2026-11-10".into()),
            }
        );
        // A concluded task never counts as late nor as the next due date.
        let p = progress(OTHER, &tasks, day("2026-11-19"));
        assert_eq!((p.total, p.done, p.late), (1, 0, 1));
        // No project uid matches nothing, not the tasks without a project.
        assert_eq!(progress("", &tasks, day("2026-11-19")), Progress::default());
        assert_eq!(progress("x", &[], day("2026-11-19")), Progress::default());
        // Nothing open with a due date: no next date.
        let only_done = vec![{
            let mut t = task(1, "feita", P, "2026-12-01");
            t.completed_at = Some("2026-11-01T10:00:00Z".into());
            t
        }];
        let p = progress(P, &only_done, day("2026-11-19"));
        assert_eq!((p.total, p.done, p.next_due), (1, 1, None));
    }

    #[test]
    fn timeline_orders_rows_and_flags_projected_and_late_bars() {
        let a = planned(
            task(1, "Planejar", P, "2026-11-30"),
            "2026-11-23T09:00",
            "2026-11-24T18:00",
        );
        let mut b = task(2, "Executar", P, "2026-11-25");
        b.dependencies = vec![1];
        b.estimate_minutes = 1440;
        let c = task(3, "Sem período", P, "2026-11-20");
        let d = task(4, "Sem nada", P, "");
        let e = task(5, "Fora", OTHER, "");
        let tasks = vec![d, c, b, a, e];
        let rows = timeline(P, &tasks);
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        // Periods first by start, then the ones without period by due date.
        assert_eq!(ids, [1, 2, 3, 4]);
        assert_eq!(
            rows[0].period,
            Some((at("2026-11-23T09:00"), at("2026-11-24T18:00")))
        );
        assert!(!rows[0].projected && !rows[0].late);
        // Projected after the planned end: next working day, one day long.
        assert!(rows[1].projected);
        let (start, end) = rows[1].period.unwrap();
        assert_eq!(start, at("2026-11-25T09:00"));
        assert_eq!(end, at("2026-11-25T18:00"));
        assert!(!rows[1].late);
        assert_eq!(rows[2].period, None);
        assert!(!rows[2].late && rows[2].due == Some(day("2026-11-20")));
        assert_eq!(rows[3].due, None);
        // A bar that ends after its due date is late.
        let mut late = tasks.clone();
        late[3].due_date = "2026-11-23".into();
        let rows = timeline(P, &late);
        assert!(rows.iter().find(|r| r.id == 1).unwrap().late);
        // Ties: same start, due, then title ignoring case, then id.
        let x = planned(
            task(10, "beta", P, ""),
            "2026-12-01T09:00",
            "2026-12-01T10:00",
        );
        let y = planned(
            task(11, "Alfa", P, ""),
            "2026-12-01T09:00",
            "2026-12-01T10:00",
        );
        let z = planned(
            task(12, "alfa", P, ""),
            "2026-12-01T09:00",
            "2026-12-01T10:00",
        );
        let rows = timeline(P, &[x, y, z]);
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [11, 12, 10]);
        assert!(timeline(OTHER, &[]).is_empty());
    }

    #[test]
    fn timeline_span_covers_periods_and_due_dates() {
        assert_eq!(timeline_span(&[]), None);
        let tasks = vec![
            planned(
                task(1, "a", P, "2026-11-30"),
                "2026-11-23T09:00",
                "2026-11-24T18:00",
            ),
            task(2, "b", P, "2026-11-20"),
            task(3, "c", P, ""),
        ];
        let rows = timeline(P, &tasks);
        // Starts with the due day of the task that has no period, ends at
        // the end of the last due day.
        assert_eq!(
            timeline_span(&rows),
            Some((at("2026-11-20T00:00"), at("2026-12-01T00:00")))
        );
        let none = timeline(P, &[task(3, "c", P, "")]);
        assert_eq!(timeline_span(&none), None);
    }

    #[test]
    fn project_risk_counts_only_the_projects_deliveries_and_the_target() {
        // "Entrega" (in the project) depends on "Insumo" (outside): the
        // prerequisite has no plan, so the delivery is at risk even though
        // the cause is not in the project.
        let insumo = task(1, "Insumo", OTHER, "");
        let mut entrega = task(2, "Entrega", P, "2026-12-10");
        entrega.dependencies = vec![1];
        entrega.estimate_minutes = 1440;
        let outra = {
            let mut t = planned(
                task(3, "Outra", OTHER, "2026-11-25"),
                "2026-11-23T09:00",
                "2026-11-27T18:00",
            );
            t.estimate_minutes = 60;
            t
        };
        let tasks = vec![insumo, entrega, outra];
        let today = day("2026-11-19");
        let r = project_risk(&project("2026-12-31"), &tasks, today);
        assert_eq!(r.deliveries_at_risk, 1);
        assert_eq!(r.target_at_risk, None);
        let r_other = project_risk(
            &Project {
                uid: OTHER.into(),
                ..project("")
            },
            &tasks,
            today,
        );
        assert_eq!(
            r_other.deliveries_at_risk, 1,
            "Outra ends after its deadline"
        );

        // With a plan for the prerequisite the delivery is projected: the
        // latest end of the project's open tasks passes the target date.
        let mut planned_tasks = tasks.clone();
        planned_tasks[0] = planned(
            planned_tasks[0].clone(),
            "2026-11-23T09:00",
            "2026-11-24T18:00",
        );
        let r = project_risk(&project("2026-11-25"), &planned_tasks, today);
        assert_eq!(r.deliveries_at_risk, 0);
        assert_eq!(r.target_at_risk, Some(at("2026-11-25T18:00")));
        let r = project_risk(&project("2026-11-26"), &planned_tasks, today);
        assert_eq!(r.target_at_risk, None);
        // No target date: nothing to miss. Concluded tasks do not count.
        let r = project_risk(&project(""), &planned_tasks, today);
        assert_eq!(r.target_at_risk, None);
        let mut finished = planned_tasks.clone();
        finished[1].status = "concluida".into();
        let r = project_risk(&project("2026-11-25"), &finished, today);
        assert_eq!(r.target_at_risk, None);
        // A fixed plan counts as well as a projection.
        let fixed = vec![planned(
            task(9, "Fixa", P, ""),
            "2026-11-30T09:00",
            "2026-12-02T18:00",
        )];
        let r = project_risk(&project("2026-12-01"), &fixed, today);
        assert_eq!(r.target_at_risk, Some(at("2026-12-02T18:00")));
    }
}
