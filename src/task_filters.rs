//! Filters of the Tarefas list by responsável, prioridade and situação, and
//! what is kept on this computer between sessions: the chosen view and the
//! filters (also category and project). The preferences live in a small file
//! in the data folder, next to the appearance one: they say nothing about
//! the library and are never synchronized.
use crate::category_ui::CategoryFilter;
use crate::project_ui::ProjectFilter;
use crate::tasks::Task;
use serde::{Deserialize, Serialize};

/// The situations a task still open can be in, in the order they are offered.
pub const STATUSES: [&str; 3] = ["pendente", "em analise", "aguardando"];
/// The priorities of a task, from the most to the least urgent.
pub const PRIORITIES: [&str; 4] = ["urgente", "alta", "normal", "baixa"];

/// The library's own fold, to compare names: lowercase, no accents, one
/// space between words, none at the ends.
fn fold(text: &str) -> String {
    crate::labels::fold(text)
}

/// The person's own name for "Minhas tarefas". The name of the team member of
/// this computer is, without a saved one, the Windows user name, which is
/// often not the name written in the tasks; so the filter is offered only
/// when some task has exactly this person (`names`, from
/// [`assignee_names`]) in charge, and it can then give a list.
pub fn own_name<'a>(actor: &'a str, names: &[String]) -> Option<&'a str> {
    let name = actor.trim();
    let key = fold(name);
    (!key.is_empty() && names.iter().any(|n| fold(n) == key)).then_some(name)
}

/// Who the list is narrowed to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AssigneeFilter {
    #[default]
    Any,
    /// Tasks with nobody in charge.
    Without,
    /// Tasks of this person (compared without case or accents).
    Named(String),
}

/// The filters of responsável, prioridade and situação; they combine (AND).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttrFilter {
    pub assignee: AssigneeFilter,
    /// A value of [`PRIORITIES`].
    pub priority: Option<String>,
    /// A value of [`STATUSES`].
    pub status: Option<String>,
}

/// No narrowing, for places that need a filter by reference.
pub static NO_ATTR_FILTER: AttrFilter = AttrFilter {
    assignee: AssigneeFilter::Any,
    priority: None,
    status: None,
};

impl AttrFilter {
    pub fn is_active(&self) -> bool {
        self.assignee != AssigneeFilter::Any || self.priority.is_some() || self.status.is_some()
    }

    pub fn clear(&mut self) {
        *self = AttrFilter::default();
    }

    pub fn matches(&self, task: &Task) -> bool {
        let assignee = match &self.assignee {
            AssigneeFilter::Any => true,
            AssigneeFilter::Without => task.assignee.trim().is_empty(),
            AssigneeFilter::Named(name) => fold(&task.assignee) == fold(name),
        };
        assignee
            && self.priority.as_ref().is_none_or(|p| task.priority == *p)
            && self.status.as_ref().is_none_or(|s| task.status == *s)
    }

    /// True when "Minhas tarefas" is on for this person.
    pub fn is_mine(&self, me: &str) -> bool {
        matches!(&self.assignee, AssigneeFilter::Named(n) if fold(n) == fold(me))
    }

    /// Turns "Minhas tarefas" on (the person's tasks) or off (everyone).
    pub fn set_mine(&mut self, me: &str, on: bool) {
        self.assignee = if on {
            AssigneeFilter::Named(me.trim().to_owned())
        } else {
            AssigneeFilter::Any
        };
    }

    /// The words that say which of these filters are on, for the sentence of
    /// an empty list.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        match &self.assignee {
            AssigneeFilter::Any => {}
            AssigneeFilter::Without => parts.push("sem responsável".to_owned()),
            AssigneeFilter::Named(name) => parts.push(format!("de {name}")),
        }
        if let Some(priority) = &self.priority {
            parts.push(format!(
                "com prioridade {}",
                crate::tasks::priority_label(priority).to_lowercase()
            ));
        }
        if let Some(status) = &self.status {
            parts.push(crate::tasks::status_label(status).to_lowercase());
        }
        parts.join(", ")
    }
}

/// The names in charge of the tasks, once each (the first spelling met), in
/// alphabetical order: of the tasks still open (what the Responsável list
/// offers) or of all of them (what decides whether a saved name or "Minhas
/// tarefas" still makes sense).
pub fn assignee_names(tasks: &[Task], open_only: bool) -> Vec<String> {
    let mut names: Vec<(String, String)> = Vec::new();
    for task in tasks.iter().filter(|t| !open_only || !t.is_done()) {
        let name = task.assignee.trim();
        if name.is_empty() {
            continue;
        }
        let key = fold(name);
        if !names.iter().any(|(k, _)| *k == key) {
            names.push((key, name.to_owned()));
        }
    }
    names.sort();
    names.into_iter().map(|(_, name)| name).collect()
}

/// What is kept between sessions. Every field has a default, so a file from
/// another version still reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub view: usize,
    pub categories: Vec<String>,
    pub no_category: bool,
    /// `None`: any project; empty: tasks without project; else a project uid.
    pub project: Option<String>,
    /// `None`: anyone; empty: nobody in charge; else the name.
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
}

/// The state of the controls, with everything that no longer exists dropped.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Restored {
    pub view: usize,
    pub categories: CategoryFilter,
    pub project: ProjectFilter,
    pub attrs: AttrFilter,
}

impl Saved {
    pub fn of(
        view: usize,
        categories: &CategoryFilter,
        project: &ProjectFilter,
        attrs: &AttrFilter,
    ) -> Self {
        Self {
            view,
            categories: categories.chosen.iter().cloned().collect(),
            no_category: categories.none,
            project: match project {
                ProjectFilter::Any => None,
                ProjectFilter::Without => Some(String::new()),
                ProjectFilter::Only(uid) => Some(uid.clone()),
            },
            assignee: match &attrs.assignee {
                AssigneeFilter::Any => None,
                AssigneeFilter::Without => Some(String::new()),
                AssigneeFilter::Named(name) => Some(name.clone()),
            },
            priority: attrs.priority.clone(),
            status: attrs.status.clone(),
        }
    }

    /// Brings the saved choices back, silently dropping each one whose target
    /// is gone: a category or project that no longer exists, a name no open
    /// task has any more, a view or value this version does not know.
    pub fn restore(
        &self,
        views: usize,
        category_uids: &[&str],
        project_uids: &[&str],
        assignees: &[String],
    ) -> Restored {
        let mut categories = CategoryFilter {
            none: self.no_category,
            ..CategoryFilter::default()
        };
        categories.chosen.extend(
            self.categories
                .iter()
                .filter(|uid| category_uids.contains(&uid.as_str()))
                .cloned(),
        );
        let project = match self.project.as_deref() {
            None => ProjectFilter::Any,
            Some("") => ProjectFilter::Without,
            Some(uid) if project_uids.contains(&uid) => ProjectFilter::Only(uid.to_owned()),
            Some(_) => ProjectFilter::Any,
        };
        let assignee = match self.assignee.as_deref() {
            None => AssigneeFilter::Any,
            Some("") => AssigneeFilter::Without,
            Some(name) => match assignees.iter().find(|n| fold(n) == fold(name)) {
                Some(found) => AssigneeFilter::Named(found.clone()),
                None => AssigneeFilter::Any,
            },
        };
        Restored {
            view: if self.view < views { self.view } else { 0 },
            categories,
            project,
            attrs: AttrFilter {
                assignee,
                priority: self
                    .priority
                    .clone()
                    .filter(|p| PRIORITIES.contains(&p.as_str())),
                status: self
                    .status
                    .clone()
                    .filter(|s| STATUSES.contains(&s.as_str())),
            },
        }
    }
}

/// Where the preferences are kept: the data folder of this computer.
fn path() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        // A test never touches the data folder of the person running it.
        return None;
    }
    crate::paths::data_dir()
        .ok()
        .map(|dir| dir.join("tarefas-filtros.json"))
}

/// The bytes of the file.
pub fn encode(saved: &Saved) -> Option<Vec<u8>> {
    serde_json::to_vec(saved).ok()
}

/// The choices in these bytes; nothing when they do not read.
pub fn decode(bytes: &[u8]) -> Option<Saved> {
    serde_json::from_slice(bytes).ok()
}

/// Whether `now` has to be written, given what was last written or read
/// (`None`: nothing known yet).
pub fn needs_saving(baseline: Option<&Saved>, now: &Saved) -> bool {
    baseline != Some(now)
}

/// What the file at `path` holds; nothing when there is none or it does not
/// read.
pub fn load_from(path: &std::path::Path) -> Option<Saved> {
    decode(&std::fs::read(path).ok()?)
}

/// Writes the choices to `path`; a failure is of no consequence (they are
/// only a convenience).
pub fn save_to(path: &std::path::Path, saved: &Saved) {
    if let Some(bytes) = encode(saved) {
        let _ = crate::atomic_file::write_atomic(path, &bytes);
    }
}

/// What was kept on this computer.
pub fn load() -> Option<Saved> {
    load_from(&path()?)
}

/// Keeps the choices on this computer.
pub fn save(saved: &Saved) {
    if let Some(path) = path() {
        save_to(&path, saved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(assignee: &str, priority: &str, status: &str) -> Task {
        let mut t = Task::standalone();
        t.assignee = assignee.into();
        t.priority = priority.into();
        t.status = status.into();
        t
    }

    #[test]
    fn the_assignee_is_compared_without_case_or_accents() {
        let filter = AttrFilter {
            assignee: AssigneeFilter::Named("José Álvares".into()),
            ..AttrFilter::default()
        };
        assert!(filter.matches(&task("jose alvares", "normal", "pendente")));
        assert!(filter.matches(&task("  JOSÉ ÁLVARES ", "normal", "pendente")));
        assert!(!filter.matches(&task("Ana", "normal", "pendente")));
        assert!(!filter.matches(&task("", "normal", "pendente")));
    }

    #[test]
    fn nobody_in_charge_is_its_own_choice() {
        let filter = AttrFilter {
            assignee: AssigneeFilter::Without,
            ..AttrFilter::default()
        };
        assert!(filter.matches(&task("", "normal", "pendente")));
        assert!(filter.matches(&task("  ", "normal", "pendente")));
        assert!(!filter.matches(&task("Ana", "normal", "pendente")));
    }

    #[test]
    fn the_three_filters_combine() {
        let filter = AttrFilter {
            assignee: AssigneeFilter::Named("Ana".into()),
            priority: Some("alta".into()),
            status: Some("aguardando".into()),
        };
        assert!(filter.matches(&task("Ana", "alta", "aguardando")));
        assert!(!filter.matches(&task("Ana", "alta", "pendente")));
        assert!(!filter.matches(&task("Ana", "baixa", "aguardando")));
        assert!(!filter.matches(&task("Rui", "alta", "aguardando")));
        assert!(AttrFilter::default().matches(&task("", "baixa", "pendente")));
        assert!(!AttrFilter::default().is_active());
        assert!(filter.is_active());
    }

    #[test]
    fn my_tasks_is_a_toggle_on_my_name() {
        let mut filter = AttrFilter::default();
        assert!(!filter.is_mine("Ana"));
        filter.set_mine("Ana ", true);
        assert!(filter.is_mine("ana"));
        assert!(filter.matches(&task("ANA", "normal", "pendente")));
        filter.set_mine("Ana", false);
        assert!(!filter.is_active());
    }

    #[test]
    fn my_tasks_is_offered_only_when_some_task_has_my_name() {
        let names = vec!["Ana  Souza".to_owned(), "Rui".to_owned()];
        // Without a saved team name the actor is the Windows user name.
        assert_eq!(own_name("maria.souza", &names), None);
        assert_eq!(own_name("  ", &names), None);
        assert_eq!(own_name("Integrante", &names), None);
        assert_eq!(own_name(" ana souza ", &names), Some("ana souza"));
        assert_eq!(own_name("ANA SOUZA", &names), Some("ANA SOUZA"));
        assert_eq!(own_name("Ana Souza", &[]), None);
        let accented = vec!["José".to_owned()];
        assert_eq!(own_name("jose", &accented), Some("jose"));
    }

    #[test]
    fn the_names_of_all_tasks_include_those_of_done_ones() {
        let mut done = task("Zeca", "normal", "concluida");
        done.completed_at = Some("2026-01-01T00:00:00Z".into());
        let tasks = vec![task("Ana", "normal", "pendente"), done];
        assert_eq!(assignee_names(&tasks, true), vec!["Ana"]);
        assert_eq!(assignee_names(&tasks, false), vec!["Ana", "Zeca"]);
    }

    #[test]
    fn spaces_inside_a_name_do_not_make_two_names() {
        let tasks = vec![
            task("Ana  Souza", "normal", "pendente"),
            task("Ana Souza", "normal", "pendente"),
        ];
        assert_eq!(assignee_names(&tasks, true).len(), 1);
    }

    #[test]
    fn the_file_is_written_read_and_only_written_when_it_changed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tarefas-filtros.json");
        assert_eq!(load_from(&file), None, "no file: nothing");
        let saved = Saved {
            view: 2,
            priority: Some("alta".into()),
            ..Saved::default()
        };
        save_to(&file, &saved);
        assert_eq!(load_from(&file), Some(saved.clone()));
        std::fs::write(&file, b"not json").unwrap();
        assert_eq!(load_from(&file), None, "a broken file reads as nothing");
        assert_eq!(decode(&encode(&saved).unwrap()), Some(saved.clone()));

        assert!(needs_saving(None, &saved));
        assert!(!needs_saving(Some(&saved), &saved));
        assert!(needs_saving(Some(&Saved::default()), &saved));
    }

    #[test]
    fn the_names_offered_are_those_of_open_tasks_once_each() {
        let mut done = task("Zeca", "normal", "concluida");
        done.completed_at = Some("2026-01-01T00:00:00Z".into());
        let tasks = vec![
            task("Rui", "normal", "pendente"),
            task("ana", "normal", "pendente"),
            task("Ana", "normal", "aguardando"),
            task("", "normal", "pendente"),
            done,
        ];
        assert_eq!(assignee_names(&tasks, true), vec!["ana", "Rui"]);
    }

    #[test]
    fn what_no_longer_exists_is_dropped_on_restore() {
        let saved = Saved {
            view: 3,
            categories: vec!["c1".into(), "gone".into()],
            no_category: true,
            project: Some("p-gone".into()),
            assignee: Some("Nobody".into()),
            priority: Some("alta".into()),
            status: Some("inexistente".into()),
        };
        let restored = saved.restore(9, &["c1"], &["p1"], &["Ana".to_owned()]);
        assert_eq!(restored.view, 3);
        assert_eq!(
            restored.categories.chosen.iter().collect::<Vec<_>>(),
            vec!["c1"]
        );
        assert!(restored.categories.none);
        assert_eq!(restored.project, ProjectFilter::Any);
        assert_eq!(restored.attrs.assignee, AssigneeFilter::Any);
        assert_eq!(restored.attrs.priority.as_deref(), Some("alta"));
        assert_eq!(restored.attrs.status, None);

        let kept = Saved {
            view: 99,
            project: Some("p1".into()),
            assignee: Some("ana".into()),
            ..Saved::default()
        }
        .restore(9, &[], &["p1"], &["Ana".to_owned()]);
        assert_eq!(kept.view, 0, "an unknown view falls back to Todas");
        assert_eq!(kept.project, ProjectFilter::Only("p1".into()));
        assert_eq!(kept.attrs.assignee, AssigneeFilter::Named("Ana".into()));
    }

    #[test]
    fn the_choices_survive_a_round_trip() {
        let mut categories = CategoryFilter::default();
        categories.chosen.insert("c1".into());
        let attrs = AttrFilter {
            assignee: AssigneeFilter::Without,
            priority: Some("urgente".into()),
            status: Some("pendente".into()),
        };
        let saved = Saved::of(4, &categories, &ProjectFilter::Without, &attrs);
        let text = serde_json::to_vec(&saved).unwrap();
        let back: Saved = serde_json::from_slice(&text).unwrap();
        assert_eq!(back, saved);
        let restored = back.restore(9, &["c1"], &[], &[]);
        assert_eq!(restored.view, 4);
        assert_eq!(restored.categories, categories);
        assert_eq!(restored.project, ProjectFilter::Without);
        assert_eq!(restored.attrs, attrs);
        // A file from another version still reads.
        let old: Saved = serde_json::from_str("{}").unwrap();
        assert_eq!(old, Saved::default());
    }
}
