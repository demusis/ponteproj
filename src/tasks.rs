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
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.title.trim().is_empty() && self.title.len() <= 1000 && self.notes.len() <= 32000,
            "Informe um título de tarefa válido (até 1000 bytes) e observações de até 32 KiB."
        );
        ensure!(
            self.assignee.len() <= 256 && !self.assignee.chars().any(char::is_control),
            "Responsável inválido (até 256 bytes)."
        );
        ensure!(ACTIONS.contains(&self.action.as_str()), "Ação inválida.");
        ensure!(
            ["pendente", "em analise", "aguardando", "concluida"].contains(&self.status.as_str()),
            "Situação inválida."
        );
        ensure!(
            ["baixa", "normal", "alta", "urgente"].contains(&self.priority.as_str()),
            "Prioridade inválida."
        );
        ensure!(
            self.due_source.len() <= 4000
                && self.outcome.len() <= 16000
                && self.checklist.len() <= 100
                && self.checklist.iter().all(|(_, text)| text.len() <= 1000),
            "Campos da tarefa excedem os limites."
        );
        if !self.reminder_date.is_empty() {
            let date = chrono::NaiveDate::parse_from_str(&self.reminder_date, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("Data do lembrete inválida; use dd/mm/aaaa."))?;
            ensure!(
                date.format("%Y-%m-%d").to_string() == self.reminder_date,
                "Data do lembrete inválida; use dd/mm/aaaa."
            );
        }
        if !self.due_date.is_empty() {
            let date = chrono::NaiveDate::parse_from_str(&self.due_date, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("Prazo inválido; use dd/mm/aaaa."))?;
            ensure!(
                date.format("%Y-%m-%d").to_string() == self.due_date,
                "Prazo inválido; use dd/mm/aaaa."
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
    pub fn list_tasks(&self) -> Result<Vec<Task>> {
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
            .context("Tarefa não encontrada após concluir.")
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
            .context("Tarefa não encontrada após reabrir.")
    }

    pub fn save_task_checked(
        &mut self,
        task: &Task,
        expected_version: Option<i64>,
    ) -> Result<i64> {
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
                _ => anyhow::bail!("Resposta inesperada do anfitrião ao salvar tarefa."),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::trash::require_active_document(&tx, task.document_id)?;
        if task.id > 0 {
            ensure!(
                crate::trash::require_active_task(&tx, task.id)? == task.document_id,
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
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        let mut graph = std::collections::BTreeMap::<i64, Task>::new();
        for row in rows {
            let (id, bytes) = row?;
            let mut value: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            value.id = id;
            graph.insert(id, value);
            ensure!(
                graph.len() <= 10000,
                "Limite de tarefas para validação de dependências excedido."
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
            "Dependências duplicadas ou excessivas."
        );
        for &dependency in &saved.dependencies {
            ensure!(
                dependency != saved.id && graph.contains_key(&dependency),
                "Dependência inexistente ou da própria tarefa."
            );
            if saved.status == "concluida" {
                ensure!(
                    graph[&dependency].status == "concluida",
                    "Conclua as tarefas anteriores antes de concluir esta tarefa."
                );
            }
            let mut pending = vec![dependency];
            let mut visited = std::collections::BTreeSet::new();
            while let Some(id) = pending.pop() {
                ensure!(id != saved.id, "As dependências formariam um ciclo.");
                if visited.insert(id) {
                    if let Some(value) = graph.get(&id) {
                        pending.extend(value.dependencies.iter().copied());
                    }
                }
            }
        }
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "task",
            saved.id,
            if task.id > 0 { expected_version } else { None },
            "save_task",
            Some(task.document_id),
            &format!("Tarefa #{}: {} ({})", saved.id, saved.title, saved.status),
        )?;
        tx.commit()?;
        Ok(saved.id)
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
