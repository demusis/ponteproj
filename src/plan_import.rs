//! Importação retomável de um documento e suas tarefas.
use crate::{
    manual::ManualFields,
    record_import::{validate_record, Created, NewRecord},
    storage::Store,
    tasks::Task,
};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub request_id: String,
    pub document: Option<PlanDocument>,
    pub document_number: Option<String>,
    pub tasks: Vec<PlanTask>,
    #[serde(default)]
    pub complete_triage: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDocument {
    pub title: String,
    pub text: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub categories: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanTask {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub due_date: String,
    #[serde(default = "normal")]
    pub priority: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub checklist: Vec<(bool, String)>,
    #[serde(default)]
    pub scheduled_start: String,
    #[serde(default)]
    pub scheduled_end: String,
    #[serde(default)]
    pub estimate_minutes: u32,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub project: String,
}
fn normal() -> String {
    "normal".into()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanResult {
    pub document_id: Option<i64>,
    pub document_number: String,
    pub title: String,
    pub tasks: Vec<TaskResult>,
    pub created: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskResult {
    pub id: Option<i64>,
    pub title: String,
}

fn uid(request: &str, index: usize) -> String {
    let hash = Sha256::digest(format!("ponte-plan\n{request}\n{index}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 15) | 0x80;
    bytes[8] = (bytes[8] & 63) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}
fn resolve(value: &str, entries: &[(String, String)], field: &str) -> Result<String> {
    let matches: Vec<_> = entries
        .iter()
        .filter(|(id, name)| id == value || name.eq_ignore_ascii_case(value))
        .collect();
    ensure!(
        matches.len() == 1,
        "{field}: valor desconhecido ou ambíguo: {value}"
    );
    Ok(matches[0].0.clone())
}
fn categories(values: &[String], entries: &[(String, String)], field: &str) -> Result<Vec<String>> {
    let result: Vec<_> = values
        .iter()
        .map(|v| resolve(v, entries, field))
        .collect::<Result<_>>()?;
    crate::categories::check_item_categories(&result).with_context(|| field.to_owned())?;
    Ok(result)
}
impl Store {
    /// Valida antes de gravar; uma repetição retoma as etapas faltantes.
    pub fn import_plan(&mut self, plan: &Plan, dry_run: bool) -> Result<PlanResult> {
        let request = uuid::Uuid::parse_str(&plan.request_id)
            .context("request_id: informe um UUID válido")?
            .to_string();
        ensure!(
            plan.document.is_some() != plan.document_number.is_some(),
            "Informe exatamente um dos campos document e document_number."
        );
        ensure!(
            (1..=100).contains(&plan.tasks.len()),
            "tasks: informe de 1 a 100 tarefas."
        );
        let cats: Vec<_> = self
            .list_categories()?
            .into_iter()
            .map(|c| (c.uid, c.name))
            .collect();
        let projects: Vec<_> = self
            .list_projects()?
            .into_iter()
            .map(|p| (p.uid, p.name))
            .collect();
        let mut record = None;
        let mut doc_categories = Vec::new();
        if let Some(doc) = &plan.document {
            let rec = NewRecord {
                fields: ManualFields {
                    kind: doc.kind.clone(),
                    title: doc.title.clone(),
                    origin: doc.origin.clone(),
                    document_date: doc.date.clone(),
                    text: doc.text.clone(),
                },
                request_id: request.clone(),
                ..Default::default()
            };
            validate_record(&rec).context("document")?;
            if !doc.date.is_empty() {
                let d = chrono::NaiveDate::parse_from_str(&doc.date, "%Y-%m-%d")
                    .context("document.date: use AAAA-MM-DD")?;
                ensure!(
                    d.format("%Y-%m-%d").to_string() == doc.date,
                    "document.date: use AAAA-MM-DD"
                );
            }
            ensure!(
                doc.note.len() <= 500 * 1024,
                "document.note: nota grande demais"
            );
            doc_categories = categories(&doc.categories, &cats, "document.categories")?;
            record = Some(rec);
        }
        let existing = if let Some(number) = &plan.document_number {
            let docs: Vec<_> = self
                .list_documents("")?
                .into_iter()
                .filter(|d| d.document_number == *number)
                .collect();
            ensure!(
                docs.len() == 1,
                "document_number: documento não encontrado ou ambíguo"
            );
            Some(docs[0].clone())
        } else {
            None
        };
        let mut tasks = Vec::new();
        for (i, input) in plan.tasks.iter().enumerate() {
            let label = format!("tasks[{}]", i + 1);
            let mut task = Task::new(existing.as_ref().map_or(0, |d| d.id));
            task.uid = uid(&request, i);
            task.title = input.title.clone();
            task.action = input.action.clone();
            task.notes = input.notes.clone();
            task.due_date = input.due_date.clone();
            task.priority = input.priority.clone();
            task.assignee = input.assignee.clone();
            task.checklist = input.checklist.clone();
            task.scheduled_start = input.scheduled_start.clone();
            task.scheduled_end = input.scheduled_end.clone();
            task.estimate_minutes = input.estimate_minutes;
            task.categories = categories(&input.categories, &cats, &format!("{label}.categories"))?;
            if !input.project.is_empty() {
                task.project_uid = resolve(&input.project, &projects, &format!("{label}.project"))?;
            }
            task.validate().with_context(|| label.clone())?;
            tasks.push(task);
        }
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(plan)?));
        let has_table: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='plan_import_receipts')", [], |r| r.get(0))?;
        let receipt: Option<(String, Option<Vec<u8>>)> = if has_table {
            self.conn
                .query_row(
                    "SELECT digest, result FROM plan_import_receipts WHERE request_id=?1",
                    [&request],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
        } else {
            None
        };
        if let Some((old, result)) = &receipt {
            ensure!(*old == digest, "request_id: já utilizado para outro plano");
            if let Some(result) = result {
                let mut result: PlanResult =
                    serde_json::from_slice(&crate::protection::unseal(result)?)?;
                result.created = false;
                return Ok(result);
            }
        }
        if dry_run {
            return Ok(PlanResult {
                document_id: existing.as_ref().map(|d| d.id),
                document_number: existing
                    .as_ref()
                    .map_or_else(|| "(novo)".into(), |d| d.document_number.clone()),
                title: existing.as_ref().map_or_else(
                    || plan.document.as_ref().unwrap().title.clone(),
                    |d| d.title.clone(),
                ),
                tasks: tasks
                    .iter()
                    .map(|t| TaskResult {
                        id: None,
                        title: t.title.clone(),
                    })
                    .collect(),
                created: false,
            });
        }
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS plan_import_receipts(request_id TEXT PRIMARY KEY, digest TEXT NOT NULL, result TEXT)")?;
        self.conn.execute(
            "INSERT OR IGNORE INTO plan_import_receipts(request_id,digest) VALUES(?1,?2)",
            params![request, digest],
        )?;
        let stored: String = self.conn.query_row(
            "SELECT digest FROM plan_import_receipts WHERE request_id=?1",
            [&request],
            |r| r.get(0),
        )?;
        ensure!(
            stored == digest,
            "request_id: já utilizado para outro plano"
        );
        let id = if let Some(d) = existing {
            d.id
        } else {
            match self.create_record_with_attachments(record.as_ref().unwrap(), &[])? {
                Created::New { document_id, .. } | Created::Already { document_id } => document_id,
            }
        };
        if let Some(doc) = &plan.document {
            let current = self.load_document(id)?;
            if current.note != doc.note {
                self.save_note(id, &doc.note)?;
            }
            if self.document_categories(id)? != doc_categories {
                self.set_document_categories(id, &doc_categories)?;
            }
            if plan.complete_triage {
                if let Some(triage) = self.list_tasks()?.into_iter().find(|t| {
                    t.document_id == Some(id)
                        && t.title == "Triar documento"
                        && t.action == "triar"
                        && !t.is_done()
                }) {
                    self.complete_task(triage.id)?;
                }
            }
        }
        let existing_tasks = self.list_tasks()?;
        let mut result_tasks = Vec::new();
        for mut task in tasks {
            task.document_id = Some(id);
            let task_id = if let Some(found) = existing_tasks.iter().find(|t| t.uid == task.uid) {
                found.id
            } else {
                self.save_task(&task)?
            };
            result_tasks.push(TaskResult {
                id: Some(task_id),
                title: task.title,
            });
        }
        let doc = self.load_document(id)?.summary;
        let result = PlanResult {
            document_id: Some(id),
            document_number: doc.document_number,
            title: doc.title,
            tasks: result_tasks,
            created: true,
        };
        self.conn.execute(
            "UPDATE plan_import_receipts SET result=?2 WHERE request_id=?1",
            params![
                request,
                crate::protection::seal(&serde_json::to_vec(&result)?)?
            ],
        )?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Plan {
        serde_json::from_str(include_str!("../tests/fixtures/import-plan-exemplo.json")).unwrap()
    }
    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        (dir, store)
    }
    #[test]
    fn creates_and_repeats() {
        let (_dir, mut store) = store();
        let plan = plan();
        let first = store.import_plan(&plan, false).unwrap();
        assert!(first.created);
        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 3);
        assert!(tasks.iter().all(|t| t.document_id == first.document_id));
        assert!(tasks
            .iter()
            .find(|t| t.title == "Triar documento")
            .unwrap()
            .is_done());
        store
            .conn
            .execute("UPDATE plan_import_receipts SET result=NULL", [])
            .unwrap();
        let resumed = store.import_plan(&plan, false).unwrap();
        assert!(resumed.created);
        assert_eq!(
            first.tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            resumed.tasks.iter().map(|t| t.id).collect::<Vec<_>>()
        );
        let second = store.import_plan(&plan, false).unwrap();
        assert!(!second.created);
        assert_eq!(first.document_id, second.document_id);
        assert_eq!(
            first.tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            second.tasks.iter().map(|t| t.id).collect::<Vec<_>>()
        );
        assert_eq!(store.list_tasks().unwrap().len(), 3);
    }
    #[test]
    fn rejects_invalid_before_writing() {
        let (_dir, mut store) = store();
        let mut cases = Vec::new();
        let mut bad = plan();
        bad.tasks[0].action = "inválida".into();
        cases.push(bad);
        let mut bad = plan();
        bad.tasks[0].title.clear();
        cases.push(bad);
        let mut bad = plan();
        bad.document_number = Some("123".into());
        cases.push(bad);
        let mut bad = plan();
        bad.document = None;
        cases.push(bad);
        let mut bad = plan();
        bad.tasks[0].due_date = "2026-02-30".into();
        cases.push(bad);
        for bad in cases {
            assert!(store.import_plan(&bad, false).is_err());
        }
        assert!(store.list_documents("").unwrap().is_empty());
        assert!(store.list_tasks().unwrap().is_empty());
    }
    #[test]
    fn dry_run_and_existing_document() {
        let (_dir, mut store) = store();
        let mut input = plan();
        let dry = store.import_plan(&input, true).unwrap();
        assert_eq!(dry.document_id, None);
        assert!(store.list_documents("").unwrap().is_empty());
        let first = store.import_plan(&input, false).unwrap();
        input.request_id = uuid::Uuid::new_v4().to_string();
        input.document = None;
        input.document_number = Some(first.document_number);
        input.complete_triage = false;
        let second = store.import_plan(&input, false).unwrap();
        assert_eq!(first.document_id, second.document_id);
        assert_eq!(store.list_tasks().unwrap().len(), 5);
    }
}
