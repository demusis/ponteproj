//! Tasks: model, validation, dependencies between tasks and the cascade that
//! concludes the tasks of joined documents.
use crate::{protection, storage::Store};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
pub const ACTIONS: &[&str] = &[
    "triar",
    "examinar",
    "tomar ciencia",
    "baixar documento",
    "responder",
    "encaminhar",
    "pedir complementacao",
    "acompanhar",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub document_id: i64,
    pub title: String,
    pub action: String,
    pub status: String,
    pub priority: String,
    pub due_date: String,
    pub notes: String,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub due_source: String,
    #[serde(default)]
    pub dependencies: Vec<i64>,
    #[serde(default)]
    pub reminder_date: String,
    #[serde(default)]
    pub checklist: Vec<(bool, String)>,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub assignee: String,
}
impl Task {
    pub fn new(document_id: i64) -> Self {
        Self {
            id: 0,
            document_id,
            title: "Triar documento".into(),
            action: "triar".into(),
            status: "pendente".into(),
            priority: "normal".into(),
            due_date: String::new(),
            notes: String::new(),
            completed_at: None,
            due_source: String::new(),
            dependencies: Vec::new(),
            reminder_date: String::new(),
            checklist: Vec::new(),
            outcome: String::new(),
            assignee: String::new(),
        }
    }
    pub fn overdue(&self) -> bool {
        self.status != "concluida"
            && !self.due_date.is_empty()
            && self.due_date < chrono::Local::now().format("%Y-%m-%d").to_string()
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            !self.title.trim().is_empty() && self.title.len() <= 1000 && self.notes.len() <= 32000,
            "Informe o título da tarefa. O título e as observações passaram do tamanho permitido."
        );
        ensure!(
            self.assignee.len() <= 256 && !self.assignee.chars().any(char::is_control),
            "O nome do responsável é inválido ou longo demais."
        );
        ensure!(ACTIONS.contains(&self.action.as_str()), "A ação da tarefa é inválida.");
        ensure!(
            ["pendente", "em analise", "aguardando", "concluida"].contains(&self.status.as_str()),
            "A situação da tarefa é inválida."
        );
        ensure!(
            ["baixa", "normal", "alta", "urgente"].contains(&self.priority.as_str()),
            "A prioridade da tarefa é inválida."
        );
        ensure!(
            self.due_source.len() <= 4000
                && self.outcome.len() <= 16000
                && self.checklist.len() <= 100
                && self.checklist.iter().all(|(_, text)| text.len() <= 1000),
            "A tarefa tem texto ou itens demais."
        );
        if !self.reminder_date.is_empty() {
            let date = chrono::NaiveDate::parse_from_str(&self.reminder_date, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("Data do lembrete inválida. Use o formato dd/mm/aaaa."))?;
            ensure!(
                date.format("%Y-%m-%d").to_string() == self.reminder_date,
                "Data do lembrete inválida. Use o formato dd/mm/aaaa."
            );
        }
        if !self.due_date.is_empty() {
            let date = chrono::NaiveDate::parse_from_str(&self.due_date, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("Prazo inválido. Use o formato dd/mm/aaaa."))?;
            ensure!(
                date.format("%Y-%m-%d").to_string() == self.due_date,
                "Prazo inválido. Use o formato dd/mm/aaaa."
            );
        }
        Ok(())
    }
}
fn encode(task: &Task) -> Result<Vec<u8>> {
    protection::seal(&serde_json::to_vec(task)?)
}
pub(crate) fn ensure_document_task(conn: &Connection, document_id: i64) -> Result<()> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM tasks WHERE document_id=?1",
        [document_id],
        |r| r.get(0),
    )?;
    if count == 0 {
        conn.execute(
            "INSERT INTO tasks(document_id,payload) VALUES(?1,?2)",
            params![document_id, encode(&Task::new(document_id))?],
        )?;
    }
    Ok(())
}
impl Store {
    pub(crate) fn ensure_tasks(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS tasks(id INTEGER PRIMARY KEY AUTOINCREMENT,document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,payload BLOB NOT NULL); CREATE INDEX IF NOT EXISTS tasks_document ON tasks(document_id);")?;
        let ids = tx
            .prepare("SELECT id FROM documents")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            ensure_document_task(&tx, id)?;
        }
        if tx.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? < 2 {
            tx.pragma_update(None, "user_version", 2)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Active tasks (neither they nor their document in the trash), overdue
    /// ones first. Memoized until the library changes or the day turns, since
    /// the order depends on today.
    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let today = chrono::Local::now().date_naive();
        self.cached(&format!("tasks\u{1}{today}"), || self.list_tasks_uncached())
    }
    fn list_tasks_uncached(&self) -> Result<Vec<Task>> {
        let rows = self
            .conn
            .prepare("SELECT t.id,t.document_id,t.payload FROM tasks t WHERE NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='task' AND x.item_id=t.id) AND NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='document' AND x.item_id=t.document_id)")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (id, document_id, blob) in rows {
            let mut task: Task = serde_json::from_slice(&protection::unseal(&blob)?)?;
            task.id = id;
            task.document_id = document_id;
            out.push(task);
        }
        out.sort_by_key(|t| {
            (
                t.status == "concluida",
                !t.overdue(),
                t.due_date.is_empty(),
                t.due_date.clone(),
                match t.priority.as_str() {
                    "urgente" => 0,
                    "alta" => 1,
                    "normal" => 2,
                    _ => 3,
                },
                t.id,
            )
        });
        Ok(out)
    }
    pub fn save_task(&mut self, task: &Task) -> Result<i64> {
        self.save_task_checked(task, None)
    }

    /// One-click conclusion. Dependency rules are still enforced by
    /// `save_task_checked`, and the edit is version-checked like any other.
    pub fn complete_task(&mut self, id: i64) -> Result<Task> {
        let mut task = self
            .list_tasks()?
            .into_iter()
            .find(|t| t.id == id)
            .context("Tarefa não encontrada.")?;
        ensure!(task.status != "concluida", "A tarefa já está concluída.");
        let version = self.item_version("task", id).ok().map(|v| v.version);
        task.status = "concluida".into();
        task.completed_at = None;
        self.save_task_checked(&task, version)?;
        self.list_tasks()?
            .into_iter()
            .find(|t| t.id == id)
            .context("Não foi possível encontrar a tarefa depois de concluí-la.")
    }

    /// Undo of `complete_task`: the task returns to "pendente".
    pub fn reopen_task(&mut self, id: i64) -> Result<Task> {
        let mut task = self
            .list_tasks()?
            .into_iter()
            .find(|t| t.id == id)
            .context("Tarefa não encontrada.")?;
        ensure!(task.status == "concluida", "A tarefa não está concluída.");
        let version = self.item_version("task", id).ok().map(|v| v.version);
        task.status = "pendente".into();
        task.completed_at = None;
        self.save_task_checked(&task, version)?;
        self.list_tasks()?
            .into_iter()
            .find(|t| t.id == id)
            .context("Não foi possível encontrar a tarefa depois de reabri-la.")
    }

    pub fn save_task_checked(&mut self, task: &Task, expected_version: Option<i64>) -> Result<i64> {
        task.validate()?;
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveTask {
                    task: task.clone(),
                    expected_version,
                },
            )? {
                crate::intranet::MutationReply::Id(id) => return Ok(id),
                _ => anyhow::bail!("O computador principal respondeu de forma inesperada ao salvar a tarefa."),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let saved = write_task(&tx, &actor, task, expected_version)?;
        tx.commit()?;
        if saved.status == "concluida" && !self.cascade_suppressed {
            // A concluded main document implicitly concludes the pieces
            // joined to it. The parent's own conclusion is already committed;
            // a problem in the cascade is reported, never raised.
            if let Err(error) = self.cascade_conclusion(task.document_id) {
                self.cascade_notes.push(format!(
                    "Não foi possível concluir as peças juntadas automaticamente: {error}"
                ));
            }
        }
        Ok(saved.id)
    }
}

/// Writes a task inside the caller's transaction (dependency rules,
/// version check, team log) and returns it as stored. `save_task_checked`
/// and the task created from an atendimento share it.
pub(crate) fn write_task(
    tx: &rusqlite::Transaction,
    actor: &str,
    task: &Task,
    expected_version: Option<i64>,
) -> Result<Task> {
    task.validate()?;
    crate::trash::require_active_document(tx, task.document_id)?;
    if task.id > 0 {
        ensure!(
            crate::trash::require_active_task(tx, task.id)? == task.document_id,
            "Vínculo da tarefa alterado."
        );
    }
    let mut saved = task.clone();
    saved.completed_at = if task.status == "concluida" {
        task.completed_at
            .clone()
            .or_else(|| Some(chrono::Utc::now().to_rfc3339()))
    } else {
        None
    };
    if task.id == 0 {
        tx.execute(
            "INSERT INTO tasks(document_id,payload) VALUES(?1,?2)",
            params![task.document_id, encode(&saved)?],
        )?;
        saved.id = tx.last_insert_rowid();
    } else {
        ensure!(
            tx.execute(
                "UPDATE tasks SET payload=?1 WHERE id=?2 AND document_id=?3",
                params![encode(&saved)?, task.id, task.document_id]
            )? == 1,
            "Tarefa não encontrada ou vínculo alterado."
        );
    }
    let mut statement = tx.prepare("SELECT t.id,t.payload FROM tasks t WHERE NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='task' AND x.item_id=t.id) AND NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='document' AND x.item_id=t.document_id)")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    let mut graph = std::collections::BTreeMap::<i64, Task>::new();
    for row in rows {
        let (id, bytes) = row?;
        let mut value: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
        value.id = id;
        graph.insert(id, value);
        ensure!(
            graph.len() <= 10000,
            "Há tarefas demais para verificar as dependências."
        );
    }
    drop(statement);
    ensure!(
        saved.dependencies.len() <= 100
            && saved
                .dependencies
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == saved.dependencies.len(),
        "As dependências estão repetidas ou são demais."
    );
    for &dependency in &saved.dependencies {
        ensure!(
            dependency != saved.id && graph.contains_key(&dependency),
            "Uma dependência não existe ou é a própria tarefa."
        );
        if saved.status == "concluida" {
            ensure!(
                graph[&dependency].status == "concluida",
                "Conclua as tarefas anteriores antes de concluir esta."
            );
        }
        let mut pending = vec![dependency];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            ensure!(id != saved.id, "As dependências formariam um ciclo: uma tarefa dependeria dela mesma.");
            if visited.insert(id) {
                if let Some(value) = graph.get(&id) {
                    pending.extend(value.dependencies.iter().copied());
                }
            }
        }
    }
    crate::intranet::record_mutation(
        tx,
        actor,
        "task",
        saved.id,
        if task.id > 0 { expected_version } else { None },
        "save_task",
        Some(task.document_id),
        &format!("Tarefa #{}: {} ({})", saved.id, saved.title, saved.status),
    )?;
    Ok(saved)
}

impl Store {
    /// When `document_id` has no open task left, concludes the open tasks of
    /// every document joined to it (and of their joined documents), recording
    /// the reason in each task. Tasks whose dependencies are still open are
    /// retried after the others; what remains open is reported.
    fn cascade_conclusion(&mut self, document_id: i64) -> Result<()> {
        let open_here = self
            .list_tasks()?
            .iter()
            .any(|t| t.document_id == document_id && t.status != "concluida");
        if open_here {
            return Ok(());
        }
        let documents = self.list_documents("")?;
        let number_of = |id: i64| {
            documents
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.document_number.clone())
                .unwrap_or_else(|| format!("#{id}"))
        };
        // Breadth-first over the juntada tree, never visiting a document twice.
        let mut visited = std::collections::BTreeSet::from([document_id]);
        let mut queue = std::collections::VecDeque::from([document_id]);
        let mut targets: Vec<(i64, i64)> = Vec::new(); // (child, parent)
        while let Some(parent) = queue.pop_front() {
            for child in documents.iter().filter(|d| d.joined_to_id == Some(parent)) {
                if visited.insert(child.id) {
                    targets.push((child.id, parent));
                    queue.push_back(child.id);
                }
            }
        }
        if targets.is_empty() {
            return Ok(());
        }
        let today = crate::labels::date(&chrono::Local::now().format("%Y-%m-%d").to_string());
        self.cascade_suppressed = true;
        let result = (|| -> Result<()> {
            let mut concluded: Vec<String> = Vec::new();
            let mut left_open: Vec<String> = Vec::new();
            // Dependencies between tasks may require several passes.
            for _pass in 0..8 {
                let mut progress = false;
                let tasks = self.list_tasks()?;
                for (child, parent) in &targets {
                    for task in tasks
                        .iter()
                        .filter(|t| t.document_id == *child && t.status != "concluida")
                    {
                        let mut done = task.clone();
                        done.status = "concluida".into();
                        done.completed_at = None;
                        done.notes.push_str(&format!(
                            "\n\n[{today}] Concluída automaticamente: o documento principal {} foi concluído.",
                            number_of(*parent)
                        ));
                        let version = self.item_version("task", task.id).ok().map(|v| v.version);
                        if self.save_task_checked(&done, version).is_ok() {
                            progress = true;
                            concluded.push(format!("{} — {}", number_of(*child), task.title));
                        }
                    }
                }
                if !progress {
                    break;
                }
            }
            for task in self.list_tasks()? {
                if targets.iter().any(|(child, _)| *child == task.document_id)
                    && task.status != "concluida"
                {
                    left_open.push(format!("{} — {}", number_of(task.document_id), task.title));
                }
            }
            if !concluded.is_empty() {
                self.cascade_notes.push(format!(
                    "{} automaticamente: {}",
                    if concluded.len() == 1 {
                        "1 tarefa de documento juntado foi concluída".to_owned()
                    } else {
                        format!("{} tarefas de documentos juntados foram concluídas", concluded.len())
                    },
                    concluded.join("; ")
                ));
            }
            if !left_open.is_empty() {
                self.cascade_notes.push(format!(
                    "Ficaram abertas por dependências pendentes: {}",
                    left_open.join("; ")
                ));
            }
            Ok(())
        })();
        self.cascade_suppressed = false;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_replay_and_task_workflow_preserve_human_choices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let capture = crate::model::sample_capture();
        let receipt = store.import_capture(&capture).unwrap();
        let mut task = store.list_tasks().unwrap().remove(0);
        assert_eq!(task.document_id, receipt.document_id);
        task.title = "Responder ofício CONFIDENCIAL_TESTE".into();
        task.priority = "urgente".into();
        task.due_date = "2020-01-01".into();
        task.due_source = "Prazo informado no ofício, página 2".into();
        task.reminder_date = "2019-12-30".into();
        task.checklist = vec![
            (true, "Conferir anexo".into()),
            (false, "Preparar resposta".into()),
        ];
        task.outcome = "Resultado privado de teste".into();
        store.save_task(&task).unwrap();
        store.import_capture(&capture).unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        assert_eq!(store.list_tasks().unwrap()[0], task);
        assert!(store.list_tasks().unwrap()[0].overdue());
        task.status = "concluida".into();
        store.save_task(&task).unwrap();
        assert!(store.list_tasks().unwrap()[0].completed_at.is_some());
        let mut extra = Task::new(receipt.document_id);
        extra.title = "Encaminhamento adicional".into();
        store.save_task(&extra).unwrap();
        extra.reminder_date = "2026-02-30".into();
        assert!(store.save_task(&extra).is_err());
        extra.reminder_date.clear();
        extra.due_date = "2026-02-30".into();
        assert!(store.save_task(&extra).is_err());
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL);")
            .unwrap();
        assert!(!std::fs::read(&path)
            .unwrap()
            .windows(b"CONFIDENCIAL_TESTE".len())
            .any(|w| w == b"CONFIDENCIAL_TESTE"));
        // Simulate an existing v1 database without task storage, preserving documents and notes.
        store
            .save_note(receipt.document_id, "Nota preservada")
            .unwrap();
        store
            .conn
            .execute_batch("DROP TABLE tasks; PRAGMA user_version=1;")
            .unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        assert_eq!(
            store.load_document(receipt.document_id).unwrap().note,
            "Nota preservada"
        );
        assert!(!std::fs::read(path)
            .unwrap()
            .windows(b"Nota preservada".len())
            .any(|w| w == b"Nota preservada"));
    }
}

/// Read-only view over the dependency graph of a task list, for the
/// interface: depth of each task, what blocks it, what waits for it, and a
/// stable order in which every task comes after the ones it depends on.
///
/// Saving refuses cycles, so the graph is acyclic; every method still
/// tolerates one (a library edited by hand, or a sync in progress) so the
/// screen never hangs.
pub struct Dependencies<'a> {
    by_id: std::collections::BTreeMap<i64, &'a Task>,
    /// Reverse edges: who depends on each task.
    dependents: std::collections::BTreeMap<i64, Vec<i64>>,
    levels: std::collections::BTreeMap<i64, usize>,
}

impl<'a> Dependencies<'a> {
    pub fn new(tasks: &'a [Task]) -> Self {
        use std::collections::BTreeMap;
        let by_id: BTreeMap<i64, &Task> = tasks.iter().map(|t| (t.id, t)).collect();
        let mut dependents: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        for task in tasks {
            for &dependency in &task.dependencies {
                if by_id.contains_key(&dependency) {
                    dependents.entry(dependency).or_default().push(task.id);
                }
            }
        }
        let mut levels = BTreeMap::new();
        let mut visiting = std::collections::BTreeSet::new();
        for task in tasks {
            Self::level_of(task.id, &by_id, &mut levels, &mut visiting);
        }
        Self {
            by_id,
            dependents,
            levels,
        }
    }

    /// 0 for a task that depends on nothing (or only on tasks not in the
    /// list); 1 + the deepest dependency otherwise. A cycle counts as 0.
    fn level_of(
        id: i64,
        by_id: &std::collections::BTreeMap<i64, &Task>,
        levels: &mut std::collections::BTreeMap<i64, usize>,
        visiting: &mut std::collections::BTreeSet<i64>,
    ) -> usize {
        if let Some(level) = levels.get(&id) {
            return *level;
        }
        if !visiting.insert(id) {
            return 0;
        }
        let level = by_id
            .get(&id)
            .map(|task| {
                task.dependencies
                    .iter()
                    .filter(|d| by_id.contains_key(d))
                    .map(|&d| Self::level_of(d, by_id, levels, visiting) + 1)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        visiting.remove(&id);
        levels.insert(id, level);
        level
    }

    pub fn level(&self, id: i64) -> usize {
        self.levels.get(&id).copied().unwrap_or(0)
    }

    /// Direct dependencies that exist, in the order the task lists them.
    pub fn direct(&self, id: i64) -> Vec<&'a Task> {
        self.by_id
            .get(&id)
            .map(|task| {
                task.dependencies
                    .iter()
                    .filter_map(|d| self.by_id.get(d).copied())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Direct dependencies still open: what must be concluded first.
    pub fn open_blockers(&self, id: i64) -> Vec<&'a Task> {
        self.direct(id)
            .into_iter()
            .filter(|t| t.status != "concluida")
            .collect()
    }

    pub fn is_blocked(&self, id: i64) -> bool {
        !self.open_blockers(id).is_empty()
    }

    /// Everything that must be concluded before this task, at any depth,
    /// nearest first. Each task appears once.
    pub fn ancestors(&self, id: i64) -> Vec<&'a Task> {
        self.walk(id, |t| t.dependencies.to_vec())
    }

    /// Everything waiting for this task, at any depth, nearest first.
    pub fn descendants(&self, id: i64) -> Vec<&'a Task> {
        self.walk(id, |t| {
            self.dependents.get(&t.id).cloned().unwrap_or_default()
        })
    }

    fn walk(&self, start: i64, next: impl Fn(&Task) -> Vec<i64>) -> Vec<&'a Task> {
        let mut seen = std::collections::BTreeSet::new();
        seen.insert(start);
        let mut queue = std::collections::VecDeque::new();
        let mut out = Vec::new();
        if let Some(task) = self.by_id.get(&start) {
            queue.extend(next(task));
        }
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(task) = self.by_id.get(&id) {
                out.push(*task);
                queue.extend(next(task));
            }
        }
        out
    }

    /// The same tasks, each listed after the ones it depends on (within the
    /// slice), otherwise keeping the given order. A task in a cycle keeps
    /// its place at the end.
    pub fn order<'b>(&self, tasks: &[&'b Task]) -> Vec<&'b Task> {
        use std::cmp::Reverse;
        use std::collections::{BTreeMap, BinaryHeap};
        let index: BTreeMap<i64, usize> =
            tasks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        let mut remaining: Vec<usize> = tasks
            .iter()
            .map(|t| {
                t.dependencies
                    .iter()
                    .filter(|d| index.contains_key(d))
                    .count()
            })
            .collect();
        let mut ready: BinaryHeap<Reverse<usize>> = remaining
            .iter()
            .enumerate()
            .filter(|(_, n)| **n == 0)
            .map(|(i, _)| Reverse(i))
            .collect();
        let mut done = vec![false; tasks.len()];
        let mut out = Vec::with_capacity(tasks.len());
        while let Some(Reverse(i)) = ready.pop() {
            done[i] = true;
            out.push(tasks[i]);
            for &dependent in self
                .dependents
                .get(&tasks[i].id)
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if let Some(&j) = index.get(&dependent) {
                    remaining[j] = remaining[j].saturating_sub(1);
                    if remaining[j] == 0 && !done[j] {
                        ready.push(Reverse(j));
                    }
                }
            }
        }
        for (i, task) in tasks.iter().enumerate() {
            if !done[i] {
                out.push(*task);
            }
        }
        out
    }
}

#[cfg(test)]
mod graph_tests {
    use super::*;

    fn task(id: i64, title: &str, deps: &[i64], status: &str) -> Task {
        let mut t = Task::new(1);
        t.id = id;
        t.title = title.into();
        t.dependencies = deps.to_vec();
        t.status = status.into();
        t
    }

    #[test]
    fn levels_order_blockers_and_chains_follow_a_diamond_and_survive_a_cycle() {
        // D depends on B and C, both depend on A; E is independent.
        let tasks = vec![
            task(4, "D", &[2, 3], "pendente"),
            task(5, "E", &[], "pendente"),
            task(2, "B", &[1], "concluida"),
            task(3, "C", &[1], "pendente"),
            task(1, "A", &[], "concluida"),
        ];
        let deps = Dependencies::new(&tasks);
        assert_eq!(deps.level(1), 0);
        assert_eq!(deps.level(2), 1);
        assert_eq!(deps.level(3), 1);
        assert_eq!(deps.level(4), 2);
        assert_eq!(deps.level(5), 0);

        fn titles(v: Vec<&Task>) -> Vec<String> {
            v.iter().map(|t| t.title.clone()).collect()
        }
        assert_eq!(titles(deps.direct(4)), ["B", "C"]);
        assert_eq!(
            titles(deps.open_blockers(4)),
            ["C"],
            "B is concluded, only C blocks D"
        );
        assert!(deps.is_blocked(4));
        assert!(!deps.is_blocked(3), "A is concluded, C may go");
        assert_eq!(
            titles(deps.ancestors(4)),
            ["B", "C", "A"],
            "nearest first, A once"
        );
        assert_eq!(titles(deps.descendants(1)), ["B", "C", "D"]);
        assert!(deps.descendants(5).is_empty());

        // Order: each after its dependencies, otherwise the given order;
        // D appears once, after the last of B and C.
        let refs: Vec<&Task> = tasks.iter().collect();
        let ordered = titles(deps.order(&refs));
        let pos = |name: &str| ordered.iter().position(|t| *t == name).unwrap();
        assert!(
            pos("A") < pos("B")
                && pos("A") < pos("C")
                && pos("B") < pos("D")
                && pos("C") < pos("D")
        );
        assert_eq!(ordered.iter().filter(|t| **t == "D").count(), 1);
        assert_eq!(ordered[0], "E", "independent tasks keep their place");

        // A subset without the dependency keeps the given order.
        let subset: Vec<&Task> = tasks.iter().filter(|t| t.id == 4 || t.id == 5).collect();
        assert_eq!(titles(deps.order(&subset)), ["D", "E"]);

        // A long chain climbs one level per step.
        let chain: Vec<Task> = (1..=12)
            .map(|i| {
                let deps: Vec<i64> = if i > 1 { vec![i - 1] } else { Vec::new() };
                task(i, &format!("T{i}"), &deps, "pendente")
            })
            .collect();
        let chain_deps = Dependencies::new(&chain);
        assert_eq!(chain_deps.level(12), 11);
        assert_eq!(chain_deps.ancestors(12).len(), 11);
        let reversed: Vec<&Task> = chain.iter().rev().collect();
        assert_eq!(
            titles(chain_deps.order(&reversed)),
            (1..=12).map(|i| format!("T{i}")).collect::<Vec<_>>()
        );

        // A cycle (never produced by saving) neither hangs nor hides tasks.
        let looped = vec![
            task(1, "X", &[2], "pendente"),
            task(2, "Y", &[1], "pendente"),
            task(3, "Z", &[], "pendente"),
        ];
        let loop_deps = Dependencies::new(&looped);
        let refs: Vec<&Task> = looped.iter().collect();
        assert_eq!(loop_deps.order(&refs).len(), 3);
        assert_eq!(loop_deps.ancestors(1).len(), 1);
        // The loop is cut on the second visit: levels stay small and finite.
        assert!(loop_deps.level(1) <= 2 && loop_deps.level(2) <= 2);
        assert_eq!(loop_deps.level(3), 0);
    }
}

#[cfg(test)]
mod dependency_tests {
    use super::*;
    #[test]
    fn dependencies_reject_cycles_missing_steps_and_early_completion_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("dependencies.sqlite3")).unwrap();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let mut a = store.list_tasks().unwrap().remove(0);
        let mut b = Task::new(receipt.document_id);
        b.title = "Redigir após exame".into();
        b.dependencies = vec![a.id];
        b.id = store.save_task(&b).unwrap();
        a.dependencies = vec![b.id];
        assert!(store.save_task(&a).is_err());
        assert!(store
            .list_tasks()
            .unwrap()
            .iter()
            .find(|t| t.id == a.id)
            .unwrap()
            .dependencies
            .is_empty());
        b.status = "concluida".into();
        assert!(store.save_task(&b).is_err());
        a.dependencies.clear();
        a.status = "concluida".into();
        store.save_task(&a).unwrap();
        store.save_task(&b).unwrap();
        let mut c = Task::new(receipt.document_id);
        c.dependencies = vec![999999];
        let before = store.list_tasks().unwrap().len();
        assert!(store.save_task(&c).is_err());
        assert_eq!(store.list_tasks().unwrap().len(), before);
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    fn capture(number: &str, text: &str) -> crate::model::Capture {
        let mut c = crate::model::sample_capture();
        c.document_number = number.into();
        c.source_url = format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla={}-A",
            number.replace('/', "%2F")
        );
        c.title = format!("Documento {number}");
        c.text = text.into();
        c
    }

    #[test]
    fn concluding_the_main_document_concludes_the_joined_pieces_in_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("cascade.sqlite3")).unwrap();
        let parent = store
            .import_capture(&capture("ORG-DIC-2026/97546", "Processo principal."))
            .unwrap();
        let child = store
            .import_capture(&capture(
                "ORG-DES-2026/39019",
                "| Juntada | Juntado ao documento: ORG-DIC-2026/97546-A |",
            ))
            .unwrap();
        let grandchild = store
            .import_capture(&capture(
                "ORG-OFI-2026/00500",
                "| Juntada | Juntado ao documento: ORG-DES-2026/39019-A |",
            ))
            .unwrap();
        let unrelated = store
            .import_capture(&capture("ORG-OFI-2026/00999", "Outro documento."))
            .unwrap();
        // The child also has a task that depends on its triage task.
        let child_triage = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.document_id == child.document_id)
            .unwrap();
        let mut dependent = Task::new(child.document_id);
        dependent.title = "Arquivar após triagem".into();
        dependent.dependencies = vec![child_triage.id];
        store.save_task(&dependent).unwrap();

        // Concluding the parent's only open task concludes the whole tree.
        let parent_task = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.document_id == parent.document_id)
            .unwrap();
        store.complete_task(parent_task.id).unwrap();
        let notes = store.take_cascade_notes();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("3 tarefas de documentos juntados foram concluídas"), "{notes:?}");
        let tasks = store.list_tasks().unwrap();
        for id in [child.document_id, grandchild.document_id] {
            for task in tasks.iter().filter(|t| t.document_id == id) {
                assert_eq!(task.status, "concluida", "{}", task.title);
                assert!(task.notes.contains("Concluída automaticamente"));
                assert!(task.completed_at.is_some());
            }
        }
        assert!(tasks
            .iter()
            .find(|t| t.document_id == grandchild.document_id)
            .unwrap()
            .notes
            .contains("ORG-DES-2026/39019"));
        // Unrelated documents are untouched; nothing cascades again.
        assert_eq!(
            tasks
                .iter()
                .find(|t| t.document_id == unrelated.document_id)
                .unwrap()
                .status,
            "pendente"
        );
        assert!(store.take_cascade_notes().is_empty());
        // Reopening the parent does not reopen the pieces.
        store.reopen_task(parent_task.id).unwrap();
        assert!(store
            .list_tasks()
            .unwrap()
            .iter()
            .filter(|t| t.document_id == child.document_id)
            .all(|t| t.status == "concluida"));
    }
    #[test]
    fn one_click_completion_and_reopening_keep_versions_and_dependency_rules() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("tasks.sqlite3")).unwrap();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let first = store.list_tasks().unwrap().remove(0);
        let mut second = Task::new(receipt.document_id);
        second.title = "Depende da triagem".into();
        second.dependencies = vec![first.id];
        let second_id = store.save_task(&second).unwrap();
        // A task whose dependency is still open cannot be concluded.
        assert!(store.complete_task(second_id).is_err());
        let done = store.complete_task(first.id).unwrap();
        assert_eq!(done.status, "concluida");
        assert!(done.completed_at.is_some());
        assert!(store.complete_task(first.id).is_err(), "already concluded");
        let done_second = store.complete_task(second_id).unwrap();
        assert_eq!(done_second.status, "concluida");
        let reopened = store.reopen_task(first.id).unwrap();
        assert_eq!(reopened.status, "pendente");
        assert!(reopened.completed_at.is_none());
        assert!(store.reopen_task(first.id).is_err());
        // Concluding and reopening are two recorded, version-checked edits.
        assert!(store.item_version("task", first.id).unwrap().version >= 2);
    }
}

pub fn status_label(value: &str) -> &str {
    match value {
        "pendente" => "Pendente",
        "em analise" => "Em análise",
        "aguardando" => "Aguardando",
        "concluida" => "Concluída",
        other => other,
    }
}
pub fn priority_label(value: &str) -> &str {
    match value {
        "baixa" => "Baixa",
        "normal" => "Normal",
        "alta" => "Alta",
        "urgente" => "Urgente",
        other => other,
    }
}

pub fn action_label(value: &str) -> &str {
    match value {
        "triar" => "Triar",
        "examinar" => "Examinar",
        "tomar ciencia" => "Tomar ciência",
        "baixar documento" => "Baixar documento",
        "responder" => "Responder",
        "encaminhar" => "Encaminhar",
        "pedir complementacao" => "Pedir complementação",
        "acompanhar" => "Acompanhar",
        other => other,
    }
}
