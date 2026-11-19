//! Tasks: model, validation, dependencies between tasks and the cascade that
//! concludes the tasks of joined documents.
use crate::{protection, storage::Store};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
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

/// The deadline groups of the task lists, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Late,
    Today,
    Upcoming,
    NoDeadline,
    Done,
}

impl Bucket {
    /// The key the phone reads.
    pub fn key(self) -> &'static str {
        match self {
            Bucket::Late => "atrasadas",
            Bucket::Today => "hoje",
            Bucket::Upcoming => "proximas",
            Bucket::NoDeadline => "sem_prazo",
            Bucket::Done => "concluidas",
        }
    }
    /// Position in the display order.
    pub fn slot(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    #[serde(default)]
    pub uid: String,
    /// The document the task belongs to; `None` for a task without document
    /// ("sem documento"). A plain integer stored by older versions reads as
    /// `Some(n)`.
    #[serde(default)]
    pub document_id: Option<i64>,
    pub title: String,
    pub action: String,
    pub status: String,
    pub priority: String,
    pub due_date: String,
    #[serde(default)]
    pub scheduled_start: String,
    #[serde(default)]
    pub scheduled_end: String,
    #[serde(default)]
    pub estimate_minutes: u32,
    #[serde(default)]
    pub reminder_offsets_minutes: Vec<u32>,
    #[serde(default)]
    pub parent_task_id: Option<i64>,
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
    /// Id of the proposal of an analysis this task was made from (schema
    /// 16); empty for any other task. "Already used" is decided by it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proposal_id: String,
    /// Uids of the categories of the task (schema 17), at most
    /// [`crate::categories::MAX_PER_ITEM`]. Only existing categories are
    /// kept when the task is saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}
impl Task {
    pub fn new(document_id: i64) -> Self {
        Self::blank(Some(document_id))
    }
    /// A task without document, as the form proposes it.
    pub fn standalone() -> Self {
        Self::blank(None)
    }
    /// What "Nova tarefa" proposes: the triage of the document, or an empty
    /// task without document.
    pub fn blank(document_id: Option<i64>) -> Self {
        Self {
            id: 0,
            uid: uuid::Uuid::new_v4().to_string(),
            document_id,
            title: if document_id.is_some() {
                "Triar documento".into()
            } else {
                String::new()
            },
            action: if document_id.is_some() {
                "triar".into()
            } else {
                "examinar".into()
            },
            status: "pendente".into(),
            priority: "normal".into(),
            due_date: String::new(),
            scheduled_start: String::new(),
            scheduled_end: String::new(),
            estimate_minutes: 0,
            reminder_offsets_minutes: Vec::new(),
            parent_task_id: None,
            notes: String::new(),
            completed_at: None,
            due_source: String::new(),
            dependencies: Vec::new(),
            reminder_date: String::new(),
            checklist: Vec::new(),
            outcome: String::new(),
            assignee: String::new(),
            proposal_id: String::new(),
            categories: Vec::new(),
        }
    }
    /// Concluded: the status says so, or a conclusion date is set.
    pub fn is_done(&self) -> bool {
        self.status == "concluida" || self.completed_at.is_some()
    }
    /// The deadline group of the task as of `today` (`AAAA-MM-DD`).
    pub fn bucket(&self, today: &str) -> Bucket {
        if self.is_done() {
            Bucket::Done
        } else if self.due_date.is_empty() {
            Bucket::NoDeadline
        } else if self.due_date.as_str() < today {
            Bucket::Late
        } else if self.due_date == today {
            Bucket::Today
        } else {
            Bucket::Upcoming
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
        ensure!(
            ACTIONS.contains(&self.action.as_str()),
            "A ação da tarefa é inválida."
        );
        ensure!(
            self.proposal_id.is_empty() || crate::model::valid_proposal_id(&self.proposal_id),
            "A identificação da proposta da tarefa é inválida."
        );
        crate::categories::check_item_categories(&self.categories)?;
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
        ensure!(
            self.reminder_offsets_minutes.len() <= 10
                && self.reminder_offsets_minutes.iter().all(|v| *v <= 525_600)
                && self
                    .reminder_offsets_minutes
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == self.reminder_offsets_minutes.len(),
            "Os avisos devem ser únicos, no máximo dez, e de até 525600 minutos."
        );
        ensure!(
            self.estimate_minutes <= 525_600,
            "A estimativa não pode ultrapassar 525600 minutos."
        );
        ensure!(
            self.scheduled_start.is_empty() == self.scheduled_end.is_empty(),
            "Informe início e fim do agendamento, ou deixe ambos vazios."
        );
        if !self.scheduled_start.is_empty() {
            let parse = |text: &str| -> Result<chrono::NaiveDateTime> {
                let date = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
                    .map_err(|_| anyhow::anyhow!("Agendamento inválido. Use AAAA-MM-DDTHH:MM."))?;
                ensure!(
                    date.format("%Y-%m-%dT%H:%M").to_string() == text,
                    "Agendamento inválido. Use AAAA-MM-DDTHH:MM."
                );
                Ok(date)
            };
            ensure!(
                parse(&self.scheduled_end)? > parse(&self.scheduled_start)?,
                "O fim do agendamento deve ser posterior ao início."
            );
        }
        if !self.reminder_date.is_empty() {
            let date = chrono::NaiveDate::parse_from_str(&self.reminder_date, "%Y-%m-%d").map_err(
                |_| anyhow::anyhow!("Data do lembrete inválida. Use o formato dd/mm/aaaa."),
            )?;
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
pub(crate) fn backfill_task_uids(conn: &mut Connection) -> Result<()> {
    let missing: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE uid IS NULL OR uid='')",
        [],
        |r| r.get(0),
    )?;
    if !missing {
        return Ok(());
    }
    let tx = conn.transaction()?;
    // Tasks in the trash, or of a document in the trash, take their uid too.
    // The guards that refuse edits to them are not about this bookkeeping:
    // drop them here; Store::open puts them back right after
    // (ensure_write_guards). Without this, one such task kept the whole
    // library from opening.
    tx.execute_batch(
        "DROP TRIGGER IF EXISTS ponte_active_tasks_update; DROP TRIGGER IF EXISTS ponte_item_active_tasks;",
    )?;
    let rows = {
        let mut statement =
            tx.prepare("SELECT id,payload FROM tasks WHERE uid IS NULL OR uid=''")?;
        let values =
            statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        values.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, bytes) in rows {
        let mut task: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
        if task.uid.is_empty() {
            task.uid = uuid::Uuid::new_v4().to_string();
        }
        tx.execute(
            "UPDATE tasks SET uid=?1,payload=?2 WHERE id=?3",
            params![task.uid, encode(&task)?, id],
        )?;
    }
    tx.commit()?;
    Ok(())
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
        let task = Task::new(document_id);
        conn.execute(
            "INSERT INTO tasks(document_id,payload,uid) VALUES(?1,?2,?3)",
            params![document_id, encode(&task)?, task.uid],
        )?;
    }
    Ok(())
}
/// Libraries up to schema 13 declared `tasks.document_id NOT NULL`. SQLite
/// cannot drop a constraint in place, so the table is rebuilt (the documented
/// procedure: new table, copy, drop, rename) inside the caller's transaction.
/// Nothing references `tasks` by foreign key, so no other table is touched;
/// ids are copied as they are and the AUTOINCREMENT counter keeps the highest
/// value ever used, so an id is never given twice. Idempotent: a table that
/// already allows NULL is left alone.
fn migrate_task_links(tx: &Connection) -> Result<()> {
    let not_null: bool = tx.query_row(
        "SELECT \"notnull\" FROM pragma_table_info('tasks') WHERE name='document_id'",
        [],
        |r| r.get(0),
    )?;
    if !not_null {
        return Ok(());
    }
    let sequence: Option<i64> = tx
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='tasks'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    // Everything below runs in the caller's transaction: a failure leaves
    // the library exactly as it was.
    tx.execute_batch(
        "DROP TABLE IF EXISTS tasks_new;
         CREATE TABLE tasks_new(id INTEGER PRIMARY KEY AUTOINCREMENT,document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,payload BLOB NOT NULL);
         INSERT INTO tasks_new(id,document_id,payload) SELECT id,document_id,payload FROM tasks ORDER BY id;
         DROP TABLE tasks;
         ALTER TABLE tasks_new RENAME TO tasks;
         CREATE INDEX IF NOT EXISTS tasks_document ON tasks(document_id);",
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "Não foi possível atualizar as tarefas do acervo para esta versão ({error}). Nada foi alterado. Feche o Ponte e avise quem cuida dele; a cópia de segurança feita pelo instalador fica na pasta de cópias do acervo."
        )
    })?;
    let highest: i64 = tx.query_row("SELECT coalesce(max(id),0) FROM tasks", [], |r| r.get(0))?;
    let keep = sequence.unwrap_or(0).max(highest);
    if tx.execute(
        "UPDATE sqlite_sequence SET seq=?1 WHERE name='tasks'",
        [keep],
    )? == 0
    {
        tx.execute(
            "INSERT INTO sqlite_sequence(name,seq) VALUES('tasks',?1)",
            [keep],
        )?;
    }
    let broken: i64 = tx.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check('tasks')",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        broken == 0,
        "Não foi possível atualizar as tarefas: há tarefas ligadas a documentos que não existem."
    );
    Ok(())
}
impl Store {
    pub(crate) fn ensure_tasks(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS tasks(id INTEGER PRIMARY KEY AUTOINCREMENT,document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,payload BLOB NOT NULL); CREATE INDEX IF NOT EXISTS tasks_document ON tasks(document_id);")?;
        migrate_task_links(&tx)?;
        let has_uid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tasks') WHERE name='uid')",
            [],
            |r| r.get(0),
        )?;
        if !has_uid {
            tx.execute("ALTER TABLE tasks ADD COLUMN uid TEXT", [])?;
        }
        tx.execute("CREATE INDEX IF NOT EXISTS tasks_uid ON tasks(uid)", [])?;
        let version = tx.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?;
        if version < 2 {
            // A library from before tasks existed: every document gets its
            // triage task, once. Later a document may well have no task (its
            // only one was moved to another document, or left without one).
            let ids = tx
                .prepare("SELECT id FROM documents")?
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in ids {
                ensure_document_task(&tx, id)?;
            }
            tx.pragma_update(None, "user_version", 2)?;
        } else if (10..crate::storage::SCHEMA_VERSION).contains(&version) {
            // Every earlier step of this library is done (they end at 10):
            // the new shape of the table and its number are committed
            // together, so an older Ponte never opens a table it cannot read.
            tx.pragma_update(None, "user_version", crate::storage::SCHEMA_VERSION)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// The lowest-numbered task of a document whose action is "triar": the
    /// one an import creates with the document (`ensure_document_task`).
    /// Read straight from the rows (not memoized, trash included: it is the
    /// identity of a row that matters, not whether it is shown); payloads are
    /// decoded, never queried.
    pub(crate) fn triage_task_of(&self, document_id: i64) -> Result<Option<i64>> {
        let rows = self
            .conn
            .prepare("SELECT id,payload FROM tasks WHERE document_id=?1 ORDER BY id")?
            .query_map([document_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, blob) in rows {
            let task: Task = serde_json::from_slice(&protection::unseal(&blob)?)?;
            if task.action == "triar" {
                return Ok(Some(id));
            }
        }
        Ok(None)
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
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (id, document_id, blob) in rows {
            let mut task: Task = serde_json::from_slice(&protection::unseal(&blob)?)?;
            task.id = id;
            task.document_id = document_id;
            task.categories = self.known_categories_of(&task.categories)?;
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

    /// Moves a task to another document, or (`None`) takes it out of its
    /// document so that it belongs to none. It is the only way to change the
    /// link of a saved task: the form's save refuses a changed link. The id,
    /// attachments, checklist and history stay with the task. Dependencies
    /// between tasks are not tied to a document, so they are kept. The edit is
    /// version-checked like any other (`expected_version`) and recorded in
    /// the team log. On a client computer the request goes to the main one.
    pub fn relink_task(
        &mut self,
        task_id: i64,
        document_id: Option<i64>,
        expected_version: Option<i64>,
    ) -> Result<i64> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::RelinkTask {
                    task_id,
                    document_id,
                    expected_version,
                },
            )? {
                crate::intranet::MutationReply::Id(id) => return Ok(id),
                _ => anyhow::bail!(
                    "O computador principal respondeu de forma inesperada ao trocar o documento da tarefa."
                ),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let before = crate::trash::require_active_task(&tx, task_id)?;
        if let Some(target) = document_id {
            crate::trash::require_active_document(&tx, target)?;
        }
        ensure!(
            before != document_id,
            "A tarefa já está nesse documento (ou já está sem documento)."
        );
        let blob: Vec<u8> =
            tx.query_row("SELECT payload FROM tasks WHERE id=?1", [task_id], |r| {
                r.get(0)
            })?;
        let mut task: Task = serde_json::from_slice(&protection::unseal(&blob)?)?;
        task.id = task_id;
        task.document_id = document_id;
        tx.execute(
            "UPDATE tasks SET document_id=?1,payload=?2 WHERE id=?3",
            params![document_id, encode(&task)?, task_id],
        )?;
        // Attachments of a task hang from the task's document.
        tx.execute(
            "UPDATE attachments SET document_id=?1 WHERE owner_kind='task' AND owner_id=?2",
            params![document_id, task_id],
        )?;
        tx.execute(
            "UPDATE trash SET document_id=?1 WHERE kind='attachment' AND item_id IN (SELECT id FROM attachments WHERE owner_kind='task' AND owner_id=?2)",
            params![document_id, task_id],
        )?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "task",
            task_id,
            expected_version,
            "relink_task",
            document_id.or(before),
            &format!(
                "Tarefa #{task_id}: {} ({})",
                task.title,
                match (before, document_id) {
                    (None, Some(_)) => "vinculada a um documento",
                    (Some(_), None) => "desvinculada do documento",
                    _ => "trocou de documento",
                }
            ),
        )?;
        tx.commit()?;
        Ok(task_id)
    }

    pub fn save_task_checked(&mut self, task: &Task, expected_version: Option<i64>) -> Result<i64> {
        task.validate()?;
        if let Some(cfg) = self.client_config()? {
            if !task.proposal_id.is_empty() {
                self.ensure_host_stores_proposals(&cfg)?;
            }
            if !task.categories.is_empty() {
                self.ensure_host_stores_categories(&cfg)?;
            }
            self.ensure_host_stores_schedule(&cfg)?;
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveTask {
                    task: task.clone(),
                    expected_version,
                },
            )? {
                crate::intranet::MutationReply::Id(id) => return Ok(id),
                _ => anyhow::bail!(
                    "O computador principal respondeu de forma inesperada ao salvar a tarefa."
                ),
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
            // A task without document takes no part in the cascade.
            if let Some(document_id) = task.document_id {
                if let Err(error) = self.cascade_conclusion(document_id) {
                    self.cascade_notes.push(format!(
                        "Não foi possível concluir as peças juntadas automaticamente: {error}"
                    ));
                }
            }
        }
        Ok(saved.id)
    }

    /// Saves the copies of a repetition (`recurrence::task_occurrences`) as
    /// one batch: a single write transaction, so either every new occurrence
    /// is stored or none is. Each copy is an independent task (its own row,
    /// uid, status, history) saved by the same rules as `save_task`.
    ///
    /// The `uid` each copy was born with is the idempotence key: a copy whose
    /// uid is already stored is **not written again** (it keeps whatever the
    /// person did with it since: concluded, edited, in the trash), so sending
    /// the same preview twice creates nothing new. Returns the id of every
    /// copy, in order, whether it was just created or was already there.
    ///
    /// Overdue or past occurrences are saved like any other: nothing is
    /// filtered by date. On a client computer the batch is refused (the
    /// host has no batch mutation yet, and a loop of single requests could
    /// stop halfway); make the repetition on the main computer.
    pub fn save_repeated_tasks(&mut self, tasks: &[Task]) -> Result<Vec<i64>> {
        self.save_repeated_tasks_counted(tasks).map(|(ids, _)| ids)
    }

    pub fn save_repeated_tasks_counted(&mut self, tasks: &[Task]) -> Result<(Vec<i64>, usize)> {
        ensure!(!tasks.is_empty(), "Não há ocorrências para criar.");
        ensure!(
            tasks.len() <= usize::from(crate::recurrence::MAX_COUNT),
            "São ocorrências demais de uma vez (no máximo {}).",
            crate::recurrence::MAX_COUNT
        );
        let mut seen = std::collections::BTreeSet::new();
        for (index, task) in tasks.iter().enumerate() {
            let label = || format!("Ocorrência {} de {}", index + 1, tasks.len());
            ensure!(
                task.id == 0,
                "{}: a cópia já tem número; não é uma ocorrência nova.",
                label()
            );
            ensure!(
                uuid::Uuid::parse_str(&task.uid).is_ok() && seen.insert(task.uid.clone()),
                "{}: a identidade da cópia está vazia, inválida ou repetida.",
                label()
            );
            task.validate()
                .map_err(|e| anyhow::anyhow!("{}: {e}", label()))?;
        }
        if self.client_config()?.is_some() {
            anyhow::bail!(
                "Neste computador (conectado ao principal) não é possível criar várias ocorrências de uma vez. Faça a repetição no computador principal."
            );
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut graph = load_task_graph(&tx)?;
        let mut ids = Vec::with_capacity(tasks.len());
        let mut created = 0;
        for (index, task) in tasks.iter().enumerate() {
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT id FROM tasks WHERE uid=?1 ORDER BY id LIMIT 1",
                    [&task.uid],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                ids.push(id);
                continue;
            }
            created += 1;
            let saved =
                write_task_with_graph(&tx, &actor, task, None, Some(&mut graph)).map_err(|e| {
                    anyhow::anyhow!(
                        "Ocorrência {} de {} ({}): {e}",
                        index + 1,
                        tasks.len(),
                        crate::labels::date(if task.due_date.is_empty() {
                            task.scheduled_start.get(..10).unwrap_or_default()
                        } else {
                            &task.due_date
                        })
                    )
                })?;
            ids.push(saved.id);
        }
        tx.commit()?;
        Ok((ids, created))
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
    write_task_with_graph(tx, actor, task, expected_version, None)
}

type TaskGraph = std::collections::BTreeMap<i64, Task>;

fn load_task_graph(tx: &rusqlite::Transaction) -> Result<TaskGraph> {
    let mut statement = tx.prepare("SELECT t.id,t.payload FROM tasks t WHERE NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='task' AND x.item_id=t.id) AND NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='document' AND x.item_id=t.document_id)")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    let mut graph = TaskGraph::new();
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
    Ok(graph)
}

fn write_task_with_graph(
    tx: &rusqlite::Transaction,
    actor: &str,
    task: &Task,
    expected_version: Option<i64>,
    batch_graph: Option<&mut TaskGraph>,
) -> Result<Task> {
    task.validate()?;
    if let Some(document_id) = task.document_id {
        crate::trash::require_active_document(tx, document_id)?;
    }
    let mut saved = task.clone();
    saved.categories = crate::categories::retain_known(tx, &task.categories)?;
    if task.id == 0 && !task.uid.is_empty() {
        let found: Option<(i64, Vec<u8>)> = tx
            .query_row(
                "SELECT id,payload FROM tasks WHERE uid=?1 ORDER BY id LIMIT 1",
                [&task.uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, bytes)) = found {
            let mut existing: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            ensure!(
                crate::trash::require_active_task(tx, id).is_ok(),
                "A identidade da tarefa pertence a um item na lixeira."
            );
            existing.id = 0;
            // A generated completion timestamp belongs to the original create, not the retry.
            if saved.status == "concluida" && saved.completed_at.is_none() {
                saved.completed_at = existing.completed_at.clone();
            } else if saved.status != "concluida" {
                saved.completed_at = None;
            }
            ensure!(
                existing == saved,
                "Conflito: esta identidade já pertence a outra versão da tarefa."
            );
            existing.id = id;
            return Ok(existing);
        }
    }
    if task.id > 0 {
        ensure!(
            crate::trash::require_active_task(tx, task.id)? == task.document_id,
            "Vínculo da tarefa alterado."
        );
    }
    if task.id > 0 {
        let bytes: Vec<u8> =
            tx.query_row("SELECT payload FROM tasks WHERE id=?1", [task.id], |r| {
                r.get(0)
            })?;
        let stored: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
        // An existing task keeps the uid it has here. Each copy of the
        // library gave its old tasks a uid of its own when it first opened
        // in this version, so an edit made on another copy may carry a
        // different one: that is the same task (same id), not a conflict.
        if !stored.uid.is_empty() {
            saved.uid = stored.uid;
        }
    }
    if saved.uid.is_empty() {
        saved.uid = uuid::Uuid::new_v4().to_string();
    }
    ensure!(
        uuid::Uuid::parse_str(&saved.uid).is_ok(),
        "Identidade da tarefa inválida."
    );
    saved.completed_at = if task.status == "concluida" {
        task.completed_at
            .clone()
            .or_else(|| Some(chrono::Utc::now().to_rfc3339()))
    } else {
        None
    };
    if task.id == 0 {
        tx.execute(
            "INSERT INTO tasks(document_id,payload,uid) VALUES(?1,?2,?3)",
            params![task.document_id, encode(&saved)?, saved.uid],
        )?;
        saved.id = tx.last_insert_rowid();
    } else {
        ensure!(
            tx.execute(
                "UPDATE tasks SET payload=?1,uid=?4 WHERE id=?2 AND document_id IS ?3",
                params![encode(&saved)?, task.id, task.document_id, saved.uid]
            )? == 1,
            "Tarefa não encontrada ou vínculo alterado."
        );
    }
    let mut owned_graph;
    let graph = match batch_graph {
        Some(graph) => graph,
        None => {
            owned_graph = load_task_graph(tx)?;
            &mut owned_graph
        }
    };
    graph.insert(saved.id, saved.clone());
    ensure!(
        graph.len() <= 10000,
        "Há tarefas demais para verificar as dependências."
    );
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
            ensure!(
                id != saved.id,
                "As dependências formariam um ciclo: uma tarefa dependeria dela mesma."
            );
            if visited.insert(id) {
                if let Some(value) = graph.get(&id) {
                    pending.extend(value.dependencies.iter().copied());
                }
            }
        }
    }
    if let Some(parent) = saved.parent_task_id {
        let mut current = Some(parent);
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = current {
            ensure!(
                id != saved.id && visited.insert(id),
                "A hierarquia de tarefas formaria um ciclo."
            );
            current = graph
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("A tarefa pai não existe ou está na lixeira."))?
                .parent_task_id;
        }
    }
    crate::intranet::record_mutation(
        tx,
        actor,
        "task",
        saved.id,
        if task.id > 0 { expected_version } else { None },
        "save_task",
        task.document_id,
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
            .any(|t| t.document_id == Some(document_id) && t.status != "concluida");
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
                        .filter(|t| t.document_id == Some(*child) && t.status != "concluida")
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
                if targets
                    .iter()
                    .any(|(child, _)| Some(*child) == task.document_id)
                    && task.status != "concluida"
                {
                    left_open.push(format!(
                        "{} — {}",
                        number_of(task.document_id.unwrap_or_default()),
                        task.title
                    ));
                }
            }
            if !concluded.is_empty() {
                self.cascade_notes.push(format!(
                    "{} automaticamente: {}",
                    if concluded.len() == 1 {
                        "1 tarefa de documento juntado foi concluída".to_owned()
                    } else {
                        format!(
                            "{} tarefas de documentos juntados foram concluídas",
                            concluded.len()
                        )
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
        assert_eq!(task.document_id, Some(receipt.document_id));
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
            .find(|t| t.document_id == Some(child.document_id))
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
            .find(|t| t.document_id == Some(parent.document_id))
            .unwrap();
        store.complete_task(parent_task.id).unwrap();
        let notes = store.take_cascade_notes();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].starts_with("3 tarefas de documentos juntados foram concluídas"),
            "{notes:?}"
        );
        let tasks = store.list_tasks().unwrap();
        for id in [child.document_id, grandchild.document_id] {
            for task in tasks.iter().filter(|t| t.document_id == Some(id)) {
                assert_eq!(task.status, "concluida", "{}", task.title);
                assert!(task.notes.contains("Concluída automaticamente"));
                assert!(task.completed_at.is_some());
            }
        }
        assert!(tasks
            .iter()
            .find(|t| t.document_id == Some(grandchild.document_id))
            .unwrap()
            .notes
            .contains("ORG-DES-2026/39019"));
        // Unrelated documents are untouched; nothing cascades again.
        assert_eq!(
            tasks
                .iter()
                .find(|t| t.document_id == Some(unrelated.document_id))
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
            .filter(|t| t.document_id == Some(child.document_id))
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

#[cfg(test)]
mod link_tests {
    use super::*;
    use crate::trash::{Kind, Target};

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        (dir, store)
    }

    fn standalone(title: &str) -> Task {
        let mut task = Task::standalone();
        task.title = title.into();
        task
    }

    fn find(store: &Store, id: i64) -> Task {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    fn second_document(store: &mut Store) -> i64 {
        let mut capture = crate::model::sample_capture();
        capture.request_id = uuid::Uuid::new_v4().to_string();
        capture.document_number = "TESTE-OFI-2026/00099".into();
        capture.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=TESTE-OFI-2026%2F00099"
                .into();
        capture.title = "Segundo ofício de exemplo".into();
        capture.text.push_str(" Segundo.");
        store.import_capture(&capture).unwrap().document_id
    }

    #[test]
    fn a_payload_stored_with_an_integer_document_still_loads() {
        let mut task = Task::new(7);
        task.id = 3;
        let mut value = serde_json::to_value(&task).unwrap();
        assert_eq!(
            value["document_id"], 7,
            "an integer, as older versions wrote"
        );
        let loaded: Task = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(loaded.document_id, Some(7));
        value["document_id"] = serde_json::Value::Null;
        let loaded: Task = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(loaded.document_id, None);
        value.as_object_mut().unwrap().remove("document_id");
        let loaded: Task = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.document_id, None);
    }

    #[test]
    fn the_table_of_an_older_library_is_rebuilt_keeping_ids_and_the_counter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("antigo.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let other = second_document(&mut store);
        let mut extra = Task::new(document);
        extra.title = "Segunda tarefa".into();
        let kept = store.save_task(&extra).unwrap();
        extra.title = "Terceira tarefa".into();
        extra.uid = uuid::Uuid::new_v4().to_string();
        let gone = store.save_task(&extra).unwrap();
        store
            .conn
            .execute("DELETE FROM tasks WHERE id=?1", [gone])
            .unwrap();
        let before = store.list_tasks().unwrap();
        // The table as versions up to 13 declared it, with a counter that is
        // higher than any id left (tasks were purged).
        store
            .conn
            .execute_batch(
                "CREATE TABLE tasks_old(id INTEGER PRIMARY KEY AUTOINCREMENT,document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,payload BLOB NOT NULL);
                 INSERT INTO tasks_old(id,document_id,payload) SELECT id,document_id,payload FROM tasks;
                 DROP TABLE tasks;
                 ALTER TABLE tasks_old RENAME TO tasks;
                 CREATE INDEX tasks_document ON tasks(document_id);
                 UPDATE sqlite_sequence SET seq=40 WHERE name='tasks';",
            )
            .unwrap();
        drop(store);

        let mut store = Store::open(&path).unwrap();
        let allows_null: bool = store
            .conn
            .query_row(
                "SELECT NOT \"notnull\" FROM pragma_table_info('tasks') WHERE name='document_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(allows_null);
        let after = store.list_tasks().unwrap();
        assert_eq!(after, before, "rows and ids are exactly the same");
        assert!(after.iter().any(|t| t.id == kept));
        // The counter was kept: an id is never given twice.
        let fresh = store.save_task(&standalone("Sem documento")).unwrap();
        assert_eq!(fresh, 41);
        // A task without document survives the deletion of any document;
        // the tasks of a deleted document still go with it.
        store.delete_document(document).unwrap();
        let left = store.list_tasks().unwrap();
        assert!(left.iter().any(|t| t.id == fresh));
        assert!(left.iter().all(|t| t.document_id != Some(document)));
        store.delete_document(other).unwrap();
        assert!(store.list_tasks().unwrap().iter().any(|t| t.id == fresh));
        // Opening it again changes nothing.
        drop(store);
        let store = Store::open(&path).unwrap();
        assert!(store.list_tasks().unwrap().iter().any(|t| t.id == fresh));
    }

    #[test]
    fn a_task_without_document_is_saved_listed_edited_and_concluded() {
        let (_dir, mut store) = store();
        let id = store
            .save_task(&standalone("Renovar o certificado"))
            .unwrap();
        let mut task = find(&store, id);
        assert_eq!(task.document_id, None);
        task.notes = "Anotação".into();
        store.save_task(&task).unwrap();
        let done = store.complete_task(id).unwrap();
        assert_eq!(done.status, "concluida");
        assert_eq!(done.document_id, None);
        // The ensure step never gives it a triage task of its own.
        store.ensure_tasks().unwrap();
        assert_eq!(
            store
                .list_tasks()
                .unwrap()
                .iter()
                .filter(|t| t.document_id.is_none())
                .count(),
            1
        );
    }

    #[test]
    fn relinking_is_explicit_checked_and_keeps_the_task_whole() {
        let (_dir, mut store) = store();
        let first = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let second = second_document(&mut store);
        let mut task = standalone("Preparar a pauta");
        task.checklist = vec![(true, "Reunir os temas".into())];
        task.outcome = "Resultado".into();
        let id = store.save_task(&task).unwrap();
        let attachment = store
            .add_attachment(
                crate::attachments::Owner::Task(id),
                "pauta.txt",
                "",
                b"pauta\n",
            )
            .unwrap();
        let hung_from = |store: &Store| -> Option<i64> {
            store
                .conn
                .query_row(
                    "SELECT document_id FROM attachments WHERE id=?1",
                    [attachment],
                    |r| r.get(0),
                )
                .unwrap()
        };

        // The form's save never changes the link.
        let mut moved = find(&store, id);
        moved.document_id = Some(first);
        assert!(store
            .save_task(&moved)
            .unwrap_err()
            .to_string()
            .contains("Vínculo da tarefa alterado"));

        // Attach, with the version check.
        let version = store.item_version("task", id).unwrap().version;
        assert!(store
            .relink_task(id, Some(first), Some(version + 5))
            .is_err());
        store.relink_task(id, Some(first), Some(version)).unwrap();
        let linked = find(&store, id);
        assert_eq!(linked.document_id, Some(first));
        assert_eq!(linked.checklist, task.checklist);
        assert_eq!(linked.outcome, "Resultado");
        assert_eq!(
            store
                .list_attachments(crate::attachments::Owner::Task(id))
                .unwrap()
                .len(),
            1,
            "attachments stay with the task"
        );
        assert_eq!(hung_from(&store), Some(first), "and hang from its document");
        assert!(store
            .relink_task(id, Some(first), None)
            .unwrap_err()
            .to_string()
            .contains("já está"));

        // From one document to another, then to none.
        store.relink_task(id, Some(second), None).unwrap();
        store.relink_task(id, None, None).unwrap();
        assert_eq!(find(&store, id).document_id, None);
        assert_eq!(hung_from(&store), None);

        // The target must be an active document that exists.
        assert!(store.relink_task(id, Some(9999), None).is_err());
        store
            .trash_item(Target {
                kind: Kind::Document,
                id: second,
            })
            .unwrap();
        assert!(store
            .relink_task(id, Some(second), None)
            .unwrap_err()
            .to_string()
            .contains("lixeira"));
        assert!(store.relink_task(9999, Some(first), None).is_err());
    }

    #[test]
    fn dependencies_are_kept_across_the_link_and_a_task_without_document_has_no_cascade() {
        let (_dir, mut store) = store();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let loose = store.save_task(&standalone("Antes de tudo")).unwrap();
        let mut dependent = Task::new(document);
        dependent.title = "Depois".into();
        dependent.dependencies = vec![loose];
        let dependent = store.save_task(&dependent).unwrap();
        store.relink_task(loose, Some(document), None).unwrap();
        store.relink_task(loose, None, None).unwrap();
        assert_eq!(find(&store, dependent).dependencies, vec![loose]);
        // Concluding a task without document concludes nothing else.
        store.complete_task(loose).unwrap();
        assert!(store.take_cascade_notes().is_empty());
    }

    #[test]
    fn a_task_without_document_goes_to_the_trash_and_comes_back_with_its_attachment() {
        let (_dir, mut store) = store();
        let id = store.save_task(&standalone("Guardar o recibo")).unwrap();
        let attachment = store
            .add_attachment(
                crate::attachments::Owner::Task(id),
                "recibo.txt",
                "",
                b"recibo\n",
            )
            .unwrap();
        let task = Target {
            kind: Kind::Task,
            id,
        };
        assert_eq!(store.trash_item(task.clone()).unwrap(), None);
        assert!(store.list_tasks().unwrap().iter().all(|t| t.id != id));
        assert!(store
            .trash_entries()
            .unwrap()
            .iter()
            .any(|e| e.document.is_none()));
        assert_eq!(store.restore_item(task.clone()).unwrap(), None);
        assert!(store.list_tasks().unwrap().iter().any(|t| t.id == id));
        // Its attachment too.
        let file = Target {
            kind: Kind::Attachment,
            id: attachment,
        };
        assert_eq!(store.trash_item(file.clone()).unwrap(), None);
        assert!(store
            .list_attachments(crate::attachments::Owner::Task(id))
            .unwrap()
            .is_empty());
        store.restore_item(file).unwrap();
        assert_eq!(
            store
                .list_attachments(crate::attachments::Owner::Task(id))
                .unwrap()
                .len(),
            1
        );
        // And the task can be removed for good.
        store.trash_item(task.clone()).unwrap();
        assert_eq!(store.purge_trash_item(task).unwrap(), None);
        assert!(store.list_tasks().unwrap().iter().all(|t| t.id != id));
    }
}

#[cfg(test)]
mod planning_tests {
    use super::*;

    #[test]
    fn old_json_and_planning_validation() {
        let mut original = Task::standalone();
        original.title = "Exame".into();
        let mut json = serde_json::to_value(&original).unwrap();
        for key in [
            "uid",
            "scheduled_start",
            "scheduled_end",
            "estimate_minutes",
            "parent_task_id",
            "reminder_offsets_minutes",
        ] {
            json.as_object_mut().unwrap().remove(key);
        }
        let mut task: Task = serde_json::from_value(json).unwrap();
        assert!(task.uid.is_empty());
        original.uid.clear();
        assert_eq!(task, original);
        task.scheduled_start = "2026-11-17T10:00".into();
        assert!(task.validate().is_err());
        task.scheduled_end = "2026-11-17T10:00".into();
        assert!(task.validate().is_err());
        task.scheduled_end = "2026-11-17T11:00".into();
        assert!(task.validate().is_ok());
        task.reminder_offsets_minutes = vec![15, 15];
        assert!(task.validate().is_err());
        task.reminder_offsets_minutes = vec![525_601];
        assert!(task.validate().is_err());
        task.reminder_offsets_minutes.clear();
        task.estimate_minutes = 525_601;
        assert!(task.validate().is_err());
    }

    #[test]
    fn scheduling_preserves_deadline_and_hierarchy_rejects_cycles_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("planning.sqlite3")).unwrap();
        let mut a = Task::standalone();
        a.title = "Pai".into();
        a.due_date = "2026-12-20".into();
        a.id = store.save_task(&a).unwrap();
        let mut b = Task::standalone();
        b.title = "Filha".into();
        b.parent_task_id = Some(a.id);
        b.id = store.save_task(&b).unwrap();
        a.scheduled_start = "2026-11-17T10:00".into();
        a.scheduled_end = "2026-11-17T11:00".into();
        store.save_task(&a).unwrap();
        a.parent_task_id = Some(b.id);
        assert!(store.save_task(&a).is_err());
        let saved = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == a.id)
            .unwrap();
        assert_eq!(saved.due_date, "2026-12-20");
        assert_eq!(saved.parent_task_id, None);
        b.parent_task_id = Some(999999);
        assert!(store.save_task(&b).is_err());
    }
    #[test]
    fn uid_replay_is_idempotent_and_never_overwrites_a_newer_edit() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("replay.sqlite3")).unwrap();
        let mut task = Task::standalone();
        task.title = "Primeira".into();
        let id = store.save_task(&task).unwrap();
        assert_eq!(store.save_task(&task).unwrap(), id);
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        let mut edited = store.list_tasks().unwrap().remove(0);
        edited.title = "Editada".into();
        store.save_task(&edited).unwrap();
        assert!(store.save_task(&task).is_err());
        assert_eq!(store.list_tasks().unwrap()[0].title, "Editada");
    }

    #[test]
    fn uid_replay_normalizes_completion_and_categories_but_rejects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("normalized-replay.sqlite3")).unwrap();
        let mut done = Task::standalone();
        done.title = "Concluída".into();
        done.status = "concluida".into();
        let id = store.save_task(&done).unwrap();
        assert_eq!(store.save_task(&done).unwrap(), id);
        assert!(store.list_tasks().unwrap()[0].completed_at.is_some());
        let mut categorized = Task::standalone();
        categorized.title = "Categoria desconhecida".into();
        categorized.categories = vec![uuid::Uuid::new_v4().simple().to_string()];
        let category_id = store.save_task(&categorized).unwrap();
        assert_eq!(store.save_task(&categorized).unwrap(), category_id);
        assert!(store
            .list_tasks()
            .unwrap()
            .iter()
            .find(|t| t.id == category_id)
            .unwrap()
            .categories
            .is_empty());
        done.title = "Outra tarefa".into();
        assert!(store
            .save_task(&done)
            .unwrap_err()
            .to_string()
            .contains("Conflito"));
        assert_eq!(store.list_tasks().unwrap().len(), 2);
    }

    #[test]
    fn uid_column_backfill_migrates_and_skips_completed_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("uid-migration.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let mut task = Task::standalone();
        task.title = "Migration".into();
        let id = store.save_task(&task).unwrap();
        store
            .conn
            .execute_batch(
                "DROP INDEX tasks_uid; ALTER TABLE tasks DROP COLUMN uid; PRAGMA user_version=18;",
            )
            .unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        let uid: String = store
            .conn
            .query_row("SELECT uid FROM tasks WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(uid, task.uid);
        store
            .conn
            .execute("UPDATE tasks SET payload=zeroblob(1) WHERE id=?1", [id])
            .unwrap();
        backfill_task_uids(&mut store.conn).unwrap();
    }

    #[test]
    fn a_task_of_a_trashed_document_does_not_keep_the_library_from_opening() {
        // The 0.39.0 installer case: an older library (no uid column) with a
        // task whose document is in the trash. Opening must fill its uid and
        // keep the trash guard working afterwards.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("uid-trash.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let task = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.document_id == Some(document))
            .unwrap();
        let mut alone = Task::standalone();
        alone.title = "Tarefa que foi para a lixeira".into();
        let alone = store.save_task(&alone).unwrap();
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Document,
                id: document,
            })
            .unwrap();
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id: alone,
            })
            .unwrap();
        store
            .conn
            .execute_batch(
                "DROP INDEX tasks_uid; ALTER TABLE tasks DROP COLUMN uid; PRAGMA user_version=18;",
            )
            .unwrap();
        drop(store);
        let store = Store::open(&path).expect("the library opens");
        let uid: String = store
            .conn
            .query_row("SELECT uid FROM tasks WHERE id=?1", [task.id], |r| r.get(0))
            .unwrap();
        assert!(!uid.is_empty());
        // The guard is back: the trashed document's task still refuses edits.
        let refused = store
            .conn
            .execute("UPDATE tasks SET uid=uid WHERE id=?1", [task.id])
            .unwrap_err()
            .to_string();
        assert!(refused.contains("lixeira"), "{refused}");
        let refused = store
            .conn
            .execute("UPDATE tasks SET uid=uid WHERE id=?1", [alone])
            .unwrap_err()
            .to_string();
        assert!(refused.contains("lixeira"), "{refused}");
    }

    fn weekly(count: u16) -> crate::recurrence::Recurrence {
        crate::recurrence::Recurrence {
            frequency: crate::recurrence::Frequency::Weekly,
            interval: 1,
            until: String::new(),
            count,
        }
    }

    fn series_origin(store: &mut Store, due: &str) -> Task {
        let mut task = Task::standalone();
        task.title = "Relatório semanal".into();
        task.due_date = due.into();
        task.id = store.save_task(&task).unwrap();
        task
    }

    #[test]
    fn repeated_tasks_are_independent_idempotent_and_keep_past_ones() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("series.sqlite3")).unwrap();
        // The origin is already overdue: nothing may be hidden or dropped.
        let origin = series_origin(&mut store, "2020-01-06");
        let copies = crate::recurrence::task_occurrences(&origin, &weekly(4)).unwrap();
        assert_eq!(copies.len(), 3);
        let (ids, created) = store.save_repeated_tasks_counted(&copies[..1]).unwrap();
        assert_eq!(created, 1);
        let (all_ids, created) = store.save_repeated_tasks_counted(&copies).unwrap();
        assert_eq!(created, 2);
        assert_eq!(ids[0], all_ids[0]);
        let ids = all_ids;
        assert_eq!(ids.len(), 3);
        assert_eq!(store.list_tasks().unwrap().len(), 4);
        // Past occurrences are still listed as late.
        assert!(store.list_tasks().unwrap().iter().all(|t| t.overdue()));
        // The person works on one copy; a replay of the same batch changes nothing.
        let mut second = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == ids[1])
            .unwrap();
        second.status = "concluida".into();
        store.save_task(&second).unwrap();
        let regenerated = crate::recurrence::task_occurrences(&origin, &weekly(4)).unwrap();
        assert_eq!(
            store.save_repeated_tasks_counted(&regenerated).unwrap(),
            (ids.clone(), 0)
        );
        let all = store.list_tasks().unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(
            all.iter().find(|t| t.id == ids[1]).unwrap().status,
            "concluida"
        );
        assert_eq!(all.iter().filter(|t| t.status == "pendente").count(), 3);
        // The origin is untouched.
        assert_eq!(
            all.iter().find(|t| t.id == origin.id).unwrap().due_date,
            "2020-01-06"
        );
    }

    #[test]
    fn repeated_tasks_are_all_or_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("atomic.sqlite3")).unwrap();
        let origin = series_origin(&mut store, "2026-12-07");
        let mut copies = crate::recurrence::task_occurrences(&origin, &weekly(4)).unwrap();
        copies[2].parent_task_id = Some(999_999); // the last one cannot be stored
        let error = store.save_repeated_tasks(&copies).unwrap_err().to_string();
        assert!(error.contains("Ocorrência 3 de 3"), "{error}");
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        // An invalid or repeated identity is refused before anything is written.
        let mut copies = crate::recurrence::task_occurrences(&origin, &weekly(3)).unwrap();
        copies[1].uid = copies[0].uid.clone();
        assert!(store.save_repeated_tasks(&copies).is_err());
        copies[1].uid.clear();
        assert!(store.save_repeated_tasks(&copies).is_err());
        assert!(store.save_repeated_tasks(&[]).is_err());
        assert_eq!(store.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn old_task_json_without_new_fields_still_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("old.sqlite3")).unwrap();
        let mut json = serde_json::to_value(Task::standalone()).unwrap();
        for key in [
            "uid",
            "scheduled_start",
            "scheduled_end",
            "estimate_minutes",
            "parent_task_id",
            "reminder_offsets_minutes",
            "categories",
            "proposal_id",
        ] {
            json.as_object_mut().unwrap().remove(key);
        }
        json["title"] = "Antiga".into();
        json["due_date"] = "2026-12-01".into();
        let mut old: Task = serde_json::from_value(json).unwrap();
        assert!(old.uid.is_empty());
        old.id = store.save_task(&old).unwrap();
        let old = store.list_tasks().unwrap().remove(0);
        assert!(!old.uid.is_empty(), "a uid is given on save");
        let copies = crate::recurrence::task_occurrences(&old, &weekly(3)).unwrap();
        assert_eq!(store.save_repeated_tasks(&copies).unwrap().len(), 2);
        assert_eq!(store.list_tasks().unwrap().len(), 3);
    }

    #[test]
    fn old_client_and_snapshot_preserve_planning_and_uid() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("compat.sqlite3")).unwrap();
        let mut task = Task::standalone();
        task.title = "Planejada".into();
        task.scheduled_start = "2026-11-17T10:00".into();
        task.scheduled_end = "2026-11-17T11:00".into();
        task.reminder_offsets_minutes = vec![15];
        task.id = store.save_task(&task).unwrap();
        let mut old = task.clone();
        old.uid.clear();
        old.scheduled_start.clear();
        old.scheduled_end.clear();
        old.reminder_offsets_minutes.clear();
        let mutation = crate::intranet::IntranetMutation::SaveTask {
            task: old,
            expected_version: None,
        };
        let crate::intranet::IntranetMutation::SaveTask { task: kept, .. } =
            store.keep_unseen_fields(mutation, 17).unwrap()
        else {
            panic!()
        };
        assert_eq!(kept.uid, task.uid);
        assert_eq!(kept.scheduled_start, task.scheduled_start);
        assert_eq!(kept.reminder_offsets_minutes, vec![15]);
        let mut snapshot = store.export_snapshot().unwrap();
        snapshot.schema_version = 17;
        snapshot.tasks[0].uid.clear();
        snapshot.tasks[0].scheduled_start.clear();
        snapshot.tasks[0].scheduled_end.clear();
        snapshot.tasks[0].reminder_offsets_minutes.clear();
        store
            .apply_snapshot(&snapshot, |_, _| unreachable!(), |_| unreachable!())
            .unwrap();
        let restored = store.list_tasks().unwrap().remove(0);
        assert_eq!(restored.uid, task.uid);
        assert_eq!(restored.scheduled_start, task.scheduled_start);
        assert_eq!(restored.reminder_offsets_minutes, vec![15]);
    }
}
