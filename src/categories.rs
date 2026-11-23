//! Categories: names the person defines and attaches to tasks and documents.
//!
//! One list serves tasks and documents; in the team modes it is shared by
//! everyone and travels with the library. A category is addressed by a random
//! `uid` (32 hexadecimal digits, the same on every computer), never by row
//! number, so an offline change needs no number translation for it. An item
//! may have several categories (at most [`MAX_PER_ITEM`]):
//!
//! - a task keeps the uids in `Task::categories`;
//! - a document keeps them in `WorkContext::categories`, the per-person
//!   record the team sync already carries (versions, conflicts and the
//!   renumbering of documents come with it).
//!
//! Only existing categories are kept when an item is saved. A category that
//! is archived stays on the items that have it but is not offered for new
//! use. Deleting a category takes it off every task and document that has it,
//! in the same transaction; a task or document in the trash is not touched
//! (the library refuses to edit it) and keeps the uid, which means nothing
//! once the category is gone.
//!
//! The name lives only in the sealed payload; in clear there is the uid.
use crate::{
    intranet::{record_mutation, IntranetMutation},
    protection::{seal, unseal},
    storage::Store,
    work_context::WorkContext,
    work_templates::version_key,
};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Longest name, in characters.
pub const MAX_NAME: usize = 40;
/// Most categories a library may have.
pub const MAX_CATEGORIES: usize = 100;
/// Most categories one task or document may have.
pub const MAX_PER_ITEM: usize = 10;
/// How many colors the palette of the screens has: `color` is an index below
/// this number. The palette itself lives in the interface.
pub const PALETTE_SIZE: u8 = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    /// 32 hexadecimal digits; empty for a category not saved yet.
    pub uid: String,
    pub name: String,
    /// Index into the palette, `0..PALETTE_SIZE`.
    pub color: u8,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Category {
    /// A new category as the form proposes it (no uid yet).
    pub fn new(name: &str, color: u8) -> Self {
        Self {
            name: name.to_owned(),
            color,
            ..Self::default()
        }
    }

    /// The name as it is kept: trimmed, with inner spaces collapsed.
    pub fn normalized(&self) -> Self {
        let mut copy = self.clone();
        copy.name = self.name.split_whitespace().collect::<Vec<_>>().join(" ");
        copy
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "Informe o nome da categoria.");
        ensure!(
            self.name.chars().count() <= MAX_NAME,
            "O nome da categoria pode ter no máximo {MAX_NAME} caracteres."
        );
        ensure!(
            !self.name.chars().any(char::is_control),
            "O nome da categoria tem um caractere inválido."
        );
        ensure!(self.color < PALETTE_SIZE, "A cor da categoria é inválida.");
        ensure!(
            self.uid.is_empty() || valid_uid(&self.uid),
            "A identificação da categoria é inválida."
        );
        Ok(())
    }

    /// Whether everything the person can edit is the same (the stamps are
    /// the library's).
    pub fn same_content(&self, other: &Self) -> bool {
        self.name == other.name && self.color == other.color && self.archived == other.archived
    }

    /// The key two names are compared by: ignoring case and accents.
    pub fn name_key(&self) -> String {
        name_key(&self.name)
    }
}

/// Case and accent folding of a name, the one the filters of the screens use.
pub fn name_key(name: &str) -> String {
    crate::labels::fold(name)
}

fn valid_uid(uid: &str) -> bool {
    uid.len() == 32
        && uid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What a task or a document may carry: few, and with a well-formed uid.
pub(crate) fn check_item_categories(uids: &[String]) -> Result<()> {
    ensure!(
        uids.len() <= MAX_PER_ITEM,
        "Um item pode ter no máximo {MAX_PER_ITEM} categorias."
    );
    ensure!(
        uids.iter().all(|u| valid_uid(u)),
        "A identificação de uma categoria é inválida."
    );
    Ok(())
}

/// The uids that exist in the library, once each, in the given order and at
/// most [`MAX_PER_ITEM`]. Used inside every write of a task or a document.
pub(crate) fn retain_known(conn: &Connection, uids: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for uid in uids {
        if out.contains(uid) {
            continue;
        }
        let known = conn
            .query_row("SELECT 1 FROM categories WHERE uid=?1", [uid], |_| Ok(()))
            .optional()?
            .is_some();
        if known {
            out.push(uid.clone());
        }
    }
    out.truncate(MAX_PER_ITEM);
    Ok(out)
}

/// Three-way merge of the categories of an item changed on both sides: what
/// the host has, plus what the local side added, minus what it removed.
pub(crate) fn merge_uids(
    base: Option<&[String]>,
    host: &[String],
    local: &[String],
) -> Vec<String> {
    let Some(base) = base else {
        // Without an ancestor: the union, so nothing is lost.
        let mut all = host.to_vec();
        for uid in local {
            if !all.contains(uid) {
                all.push(uid.clone());
            }
        }
        all.truncate(MAX_PER_ITEM);
        return all;
    };
    let mut all: Vec<String> = host
        .iter()
        .filter(|u| local.contains(u) || !base.contains(u))
        .cloned()
        .collect();
    for uid in local {
        if !base.contains(uid) && !all.contains(uid) {
            all.push(uid.clone());
        }
    }
    all.truncate(MAX_PER_ITEM);
    all
}

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

/// Every category with its uid, by name; read inside a transaction or not.
fn read_all(conn: &Connection) -> Result<Vec<Category>> {
    let mut statement = conn.prepare("SELECT uid,payload FROM categories")?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (uid, blob) in rows {
        let mut category: Category = decode(&blob)
            .context("Não foi possível ler uma categoria: o registro está inválido.")?;
        category.uid = uid;
        out.push(category);
    }
    out.sort_by(|a, b| {
        a.name_key()
            .cmp(&b.name_key())
            .then_with(|| a.uid.cmp(&b.uid))
    });
    Ok(out)
}

/// The payload without `uid` among its categories; `None` when it did not
/// have it.
fn without_uid<T>(
    blob: &[u8],
    uid: &str,
    get: impl Fn(&mut T) -> &mut Vec<String>,
) -> Result<Option<Vec<u8>>>
where
    T: for<'a> Deserialize<'a> + Serialize,
{
    let mut value: T = decode(blob)?;
    let list = get(&mut value);
    if !list.iter().any(|u| u == uid) {
        return Ok(None);
    }
    list.retain(|u| u != uid);
    Ok(Some(encode(&value)?))
}

/// The rules between categories: at most [`MAX_CATEGORIES`], and no two with
/// the same name ignoring case and accents.
/// `enforce_max` is off when replaying changes made elsewhere.
fn check_unique(all: &[Category], category: &Category, enforce_max: bool) -> Result<()> {
    let key = category.name_key();
    if all
        .iter()
        .any(|c| c.uid != category.uid && c.name_key() == key)
    {
        bail!("Já existe uma categoria com este nome.");
    }
    if enforce_max && !all.iter().any(|c| c.uid == category.uid) && all.len() >= MAX_CATEGORIES {
        bail!("Já existem {MAX_CATEGORIES} categorias, o máximo permitido. Apague alguma antes de criar outra.");
    }
    Ok(())
}

impl Store {
    pub(crate) fn ensure_categories(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS categories(
                id INTEGER PRIMARY KEY,
                uid TEXT NOT NULL UNIQUE,
                payload BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    // -- reads --------------------------------------------------------------

    /// Every category (archived ones too), by name ignoring case and accents,
    /// memoized until the library changes.
    pub fn list_categories(&self) -> Result<Vec<Category>> {
        self.cached("categories", || read_all(&self.conn))
    }

    /// The version a screen hands back as `expected_version`; 0 when new.
    pub fn category_version(&self, uid: &str) -> Result<i64> {
        Ok(self.item_version("category", version_key(uid))?.version)
    }

    pub(crate) fn category_exists(&self, uid: &str) -> bool {
        self.conn
            .query_row("SELECT 1 FROM categories WHERE uid=?1", [uid], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    /// Of `uids`, the ones that exist, in order and without repeats.
    pub(crate) fn known_categories_of(&self, uids: &[String]) -> Result<Vec<String>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let known: HashSet<String> = self.list_categories()?.into_iter().map(|c| c.uid).collect();
        let mut out: Vec<String> = Vec::new();
        for uid in uids {
            if known.contains(uid) && !out.contains(uid) {
                out.push(uid.clone());
            }
        }
        Ok(out)
    }

    /// The categories of a document (existing ones only).
    pub fn document_categories(&self, document_id: i64) -> Result<Vec<String>> {
        let context = self.load_work_context(document_id)?;
        self.known_categories_of(&context.categories)
    }

    /// For every category: how many tasks and how many documents have it,
    /// counting what is not in the trash.
    pub fn category_usage(&self) -> Result<HashMap<String, (usize, usize)>> {
        let known: HashSet<String> = self.list_categories()?.into_iter().map(|c| c.uid).collect();
        let mut usage: HashMap<String, (usize, usize)> = HashMap::new();
        for task in self.list_tasks()? {
            for uid in task.categories.iter().filter(|u| known.contains(*u)) {
                usage.entry(uid.clone()).or_default().0 += 1;
            }
        }
        let mut statement = self.conn.prepare(
            "SELECT w.payload FROM work_context w WHERE EXISTS(SELECT 1 FROM documents d WHERE d.id=w.document_id) AND NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='document' AND x.item_id=w.document_id)",
        )?;
        let blobs = statement
            .query_map([], |r| r.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for blob in blobs {
            let context: WorkContext = decode(&blob)?;
            let mut seen: Vec<&String> = Vec::new();
            for uid in context.categories.iter().filter(|u| known.contains(*u)) {
                if !seen.contains(&uid) {
                    seen.push(uid);
                    usage.entry(uid.clone()).or_default().1 += 1;
                }
            }
        }
        Ok(usage)
    }

    // -- writes ---------------------------------------------------------------

    /// A client older than the library of this computer drops fields it does
    /// not know when it reads a task or a work context, and sends them back
    /// empty. For a saved item, the stored values of those fields are kept.
    /// The same for the project of a task of a client that never saw it: an
    /// empty `project_uid` takes the link off only when the change says so
    /// (`no_project`), so a queue written before the projects, or a save made
    /// from a copy that predates them, never clears a link.
    pub(crate) fn keep_unseen_fields(
        &self,
        mutation: IntranetMutation,
        client_schema: i64,
    ) -> Result<IntranetMutation> {
        Ok(match mutation {
            IntranetMutation::SaveTask {
                mut task,
                expected_version,
                no_project,
            } if task.id > 0
                && (client_schema < crate::intranet::PROJECTS_SCHEMA
                    || (task.project_uid.is_empty() && !no_project)) =>
            {
                if let Some(stored) = self.list_tasks()?.into_iter().find(|t| t.id == task.id) {
                    if client_schema < crate::intranet::SCHEDULE_SCHEMA {
                        task.uid = stored.uid;
                        task.scheduled_start = stored.scheduled_start;
                        task.scheduled_end = stored.scheduled_end;
                        task.estimate_minutes = stored.estimate_minutes;
                        task.parent_task_id = stored.parent_task_id;
                        task.reminder_offsets_minutes = stored.reminder_offsets_minutes;
                        if client_schema < crate::intranet::CATEGORIES_SCHEMA {
                            task.categories = stored.categories;
                        }
                        if client_schema < crate::intranet::PROPOSALS_SCHEMA {
                            task.proposal_id = stored.proposal_id;
                        }
                    }
                    // A Ponte before the projects never sees the field and
                    // sends the task back without it; a current one sends it
                    // empty either to clear it (no_project) or untouched.
                    if client_schema < crate::intranet::PROJECTS_SCHEMA || !no_project {
                        task.project_uid = stored.project_uid;
                    }
                }
                IntranetMutation::SaveTask {
                    task,
                    expected_version,
                    no_project,
                }
            }
            IntranetMutation::SaveResourceUse {
                mut item,
                expected_version,
            } if client_schema < crate::intranet::PROPOSALS_SCHEMA && !item.uid.is_empty() => {
                if let Some(stored) = self
                    .list_resource_uses()?
                    .into_iter()
                    .find(|u| u.uid == item.uid)
                {
                    item.proposal_id = stored.proposal_id;
                }
                IntranetMutation::SaveResourceUse {
                    item,
                    expected_version,
                }
            }
            IntranetMutation::SaveWorkContext {
                document_id,
                mut value,
                expected_version,
            } if client_schema < crate::intranet::CATEGORIES_SCHEMA => {
                value.categories = self.load_work_context(document_id)?.categories;
                IntranetMutation::SaveWorkContext {
                    document_id,
                    value,
                    expected_version,
                }
            }
            other => other,
        })
    }

    /// Creates (empty uid) or changes a category; returns it as stored.
    pub fn save_category(&mut self, category: &Category) -> Result<Category> {
        self.save_category_checked(category, None)
    }

    /// The same, refusing a change made on top of an older version of the
    /// category (`expected_version`, from [`Store::category_version`]).
    pub fn save_category_checked(
        &mut self,
        category: &Category,
        expected_version: Option<i64>,
    ) -> Result<Category> {
        self.save_category_inner(category, expected_version, true)
    }

    /// Applies a category change that came from another computer (main
    /// computer, offline queue, shared folder). A category, new or edited,
    /// whose name another category has is stored as "Nome (2)", "Nome (3)"…,
    /// and the limit of categories is not enforced, so nothing sent later is
    /// lost.
    pub(crate) fn save_category_applied(
        &mut self,
        category: &Category,
        expected_version: Option<i64>,
    ) -> Result<Category> {
        let mut category = category.normalized();
        category.validate()?;
        if !category.uid.is_empty() {
            let all = read_all(&self.conn)?;
            let key = category.name_key();
            // New or edited: either way a name another category has is
            // stored with a number, as a refusal here could not be undone.
            let own_uid = category.uid.clone();
            let taken_by_other = |name_key: &str| {
                all.iter()
                    .any(|c| c.uid != own_uid && c.name_key() == name_key)
            };
            if taken_by_other(&key) {
                let base: String = category.name.clone();
                for n in 2.. {
                    let suffix = format!(" ({n})");
                    let room = MAX_NAME - suffix.chars().count();
                    let head: String = base.chars().take(room).collect();
                    let mut candidate = category.clone();
                    candidate.name = format!("{}{suffix}", head.trim_end());
                    let taken = candidate.name_key();
                    if !taken_by_other(&taken) {
                        category = candidate;
                        break;
                    }
                }
            }
        }
        self.save_category_inner(&category, expected_version, false)
    }

    fn save_category_inner(
        &mut self,
        category: &Category,
        expected_version: Option<i64>,
        enforce_max: bool,
    ) -> Result<Category> {
        let mut category = category.normalized();
        if category.uid.is_empty() {
            category.uid = new_uid();
        }
        category.validate()?;
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_categories(&cfg)?;
            // The rules are checked here first, so a refusal is told now and
            // not found in the queue of changes later.
            check_unique(&read_all(&self.conn)?, &category, enforce_max)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveCategory {
                    category: category.clone(),
                    expected_version,
                },
            )?;
            return Ok(category);
        }
        let actor = self.active_actor();
        let timestamp = now_stamp();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let all = read_all(&tx)?;
        check_unique(&all, &category, enforce_max)?;
        let previous = all.iter().find(|c| c.uid == category.uid);
        let mut stored = category.clone();
        stored.updated_at = timestamp.clone();
        let details = match previous {
            Some(before) => {
                stored.created_at = before.created_at.clone();
                tx.execute(
                    "UPDATE categories SET payload=?1,updated_at=?2 WHERE uid=?3",
                    params![encode(&stored)?, timestamp, stored.uid],
                )?;
                if before.archived != stored.archived {
                    format!(
                        "{} a categoria \"{}\"",
                        if stored.archived {
                            "Arquivou"
                        } else {
                            "Reativou"
                        },
                        stored.name
                    )
                } else {
                    format!("Alterou a categoria \"{}\"", stored.name)
                }
            }
            None => {
                stored.created_at = timestamp.clone();
                tx.execute(
                    "INSERT INTO categories(uid,payload,updated_at) VALUES(?1,?2,?3)",
                    params![stored.uid, encode(&stored)?, timestamp],
                )?;
                format!("Cadastrou a categoria \"{}\"", stored.name)
            }
        };
        record_mutation(
            &tx,
            &actor,
            "category",
            version_key(&stored.uid),
            expected_version,
            "save_category",
            None,
            &details,
        )?;
        tx.commit()?;
        Ok(stored)
    }

    /// Archives a category (it stays on the items that have it, but is not
    /// offered for new use) or brings it back.
    pub fn set_category_archived(&mut self, uid: &str, archived: bool) -> Result<Category> {
        let mut category = self
            .list_categories()?
            .into_iter()
            .find(|c| c.uid == uid)
            .context("Categoria não encontrada (já apagada?).")?;
        if category.archived == archived {
            return Ok(category);
        }
        category.archived = archived;
        let version = self.category_version(uid).ok();
        self.save_category_checked(&category, version)
    }

    /// Deletes a category and takes it off every task and document that has it.
    pub fn delete_category(&mut self, uid: &str) -> Result<()> {
        let version = self.category_version(uid).ok().filter(|v| *v > 0);
        self.delete_category_checked(uid, version)
    }

    pub fn delete_category_checked(
        &mut self,
        uid: &str,
        expected_version: Option<i64>,
    ) -> Result<()> {
        ensure!(valid_uid(uid), "A identificação da categoria é inválida.");
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_categories(&cfg)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::DeleteCategory {
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
        let category = read_all(&tx)?
            .into_iter()
            .find(|c| c.uid == uid)
            .context("Categoria não encontrada (já apagada?).")?;
        tx.execute("DELETE FROM categories WHERE uid=?1", [uid])?;
        // Items in the trash cannot be edited and are left as they are.
        let tasks = tx
            .prepare("SELECT t.id,t.payload FROM tasks t WHERE NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='task' AND x.item_id=t.id) AND NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='document' AND x.item_id=t.document_id)")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, blob) in tasks {
            if let Some(blob) =
                without_uid::<crate::tasks::Task>(&blob, uid, |t| &mut t.categories)?
            {
                tx.execute("UPDATE tasks SET payload=?1 WHERE id=?2", params![blob, id])?;
            }
        }
        let contexts = tx
            .prepare("SELECT w.document_id,w.payload FROM work_context w WHERE NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind='document' AND x.item_id=w.document_id)")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (document_id, blob) in contexts {
            if let Some(blob) = without_uid::<WorkContext>(&blob, uid, |c| &mut c.categories)? {
                tx.execute(
                    "UPDATE work_context SET payload=?1 WHERE document_id=?2",
                    params![blob, document_id],
                )?;
            }
        }
        record_mutation(
            &tx,
            &actor,
            "category",
            version_key(uid),
            expected_version,
            "delete_category",
            None,
            &format!("Apagou a categoria \"{}\"", category.name),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Replaces the categories of a document. Categories that do not exist
    /// are dropped; at most [`MAX_PER_ITEM`].
    pub fn set_document_categories(&mut self, document_id: i64, uids: &[String]) -> Result<()> {
        check_item_categories(uids)?;
        let mut context = self.load_work_context(document_id)?;
        if context.categories == uids {
            return Ok(());
        }
        context.categories = uids.to_vec();
        let version = self
            .item_version("work_context", document_id)
            .ok()
            .map(|m| m.version);
        self.save_work_context_checked(document_id, &context, version)
    }

    // -- team sync --------------------------------------------------------------

    pub(crate) fn export_categories(&self) -> Result<Vec<Category>> {
        let mut statement = self
            .conn
            .prepare("SELECT uid,payload FROM categories ORDER BY id")?;
        let rows = statement
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (uid, blob) in rows {
            let mut category: Category = decode(&blob)?;
            category.uid = uid;
            out.push(category);
        }
        Ok(out)
    }
}

/// Writes the table from a snapshot (the table was emptied by the caller),
/// re-sealing the payloads for this Windows account.
pub(crate) fn apply_categories(tx: &Connection, categories: &[Category]) -> Result<()> {
    for category in categories {
        tx.execute(
            "INSERT INTO categories(uid,payload,updated_at) VALUES(?1,?2,?3)",
            params![category.uid, encode(category)?, category.updated_at],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intranet::{generate_token, HostServer, IntranetConfig, IntranetMode};
    use crate::tasks::Task;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        (dir, store)
    }

    fn make(store: &mut Store, name: &str) -> Category {
        store.save_category(&Category::new(name, 2)).unwrap()
    }

    fn task_with(store: &mut Store, title: &str, uids: &[&str]) -> i64 {
        let mut task = Task::standalone();
        task.title = title.into();
        task.categories = uids.iter().map(|u| u.to_string()).collect();
        store.save_task(&task).unwrap()
    }

    fn uids(list: &[&Category]) -> Vec<String> {
        list.iter().map(|c| c.uid.clone()).collect()
    }

    fn categories_of_task(store: &Store, id: i64) -> Vec<String> {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
            .categories
    }

    #[test]
    fn categories_are_created_renamed_archived_and_deleted() {
        let (_dir, mut store) = store();
        assert!(store.list_categories().unwrap().is_empty());
        let urgent = make(&mut store, "  Urgente   do mês ");
        assert_eq!(urgent.name, "Urgente do mês");
        assert_eq!(urgent.uid.len(), 32);
        assert!(!urgent.archived && !urgent.created_at.is_empty());
        let other = make(&mut store, "Análise");
        // Sorted by name ignoring accents and case.
        let names: Vec<_> = store
            .list_categories()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["Análise", "Urgente do mês"]);
        // Rename keeps the uid and the creation stamp.
        let mut renamed = urgent.clone();
        renamed.name = "Prioridade".into();
        renamed.color = 7;
        let saved = store.save_category(&renamed).unwrap();
        assert_eq!(saved.uid, urgent.uid);
        assert_eq!(saved.created_at, urgent.created_at);
        assert_eq!(store.list_categories().unwrap().len(), 2);
        assert_eq!(store.list_categories().unwrap()[1].color, 7);
        // Archive and bring back.
        assert!(
            store
                .set_category_archived(&other.uid, true)
                .unwrap()
                .archived
        );
        assert!(store
            .list_categories()
            .unwrap()
            .iter()
            .any(|c| c.uid == other.uid && c.archived));
        assert!(
            !store
                .set_category_archived(&other.uid, false)
                .unwrap()
                .archived
        );
        // Delete.
        store.delete_category(&other.uid).unwrap();
        assert_eq!(store.list_categories().unwrap().len(), 1);
        assert!(store
            .delete_category(&other.uid)
            .unwrap_err()
            .to_string()
            .contains("não encontrada"));
    }

    #[test]
    fn the_rules_of_names_colors_and_the_limit_are_enforced() {
        let (_dir, mut store) = store();
        make(&mut store, "Orientação");
        for name in ["orientacao", "  ORIENTAÇÃO ", "Orientacao"] {
            let error = store
                .save_category(&Category::new(name, 0))
                .unwrap_err()
                .to_string();
            assert_eq!(error, "Já existe uma categoria com este nome.");
        }
        assert!(store.save_category(&Category::new("   ", 0)).is_err());
        assert!(store
            .save_category(&Category::new(&"x".repeat(41), 0))
            .is_err());
        assert!(store
            .save_category(&Category::new(&"y".repeat(40), 0))
            .is_ok());
        assert!(store
            .save_category(&Category::new("Cor ruim", PALETTE_SIZE))
            .is_err());
        // Saving a category again with its own name is not a duplicate.
        let mut same = store.list_categories().unwrap()[0].clone();
        same.color = 3;
        assert!(store.save_category(&same).is_ok());
        // The library holds at most 100.
        let have = store.list_categories().unwrap().len();
        for n in have..MAX_CATEGORIES {
            make(&mut store, &format!("Categoria {n}"));
        }
        let error = store
            .save_category(&Category::new("Uma a mais", 0))
            .unwrap_err()
            .to_string();
        assert!(error.contains("100 categorias"), "{error}");
        // Changing an existing one still works at the limit.
        assert!(store.save_category(&same).is_ok());
    }

    #[test]
    fn assignments_round_trip_and_unknown_uids_are_dropped() {
        let (_dir, mut store) = store();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let a = make(&mut store, "Contrato");
        let b = make(&mut store, "Jurídico");
        let ghost = "0123456789abcdef0123456789abcdef".to_owned();
        store
            .set_document_categories(document, &[a.uid.clone(), ghost.clone(), b.uid.clone()])
            .unwrap();
        assert_eq!(
            store.document_categories(document).unwrap(),
            uids(&[&a, &b])
        );
        let summary = store.list_documents("").unwrap();
        assert_eq!(summary[0].categories, uids(&[&a, &b]));
        let id = task_with(&mut store, "Revisar", &[&b.uid, &ghost, &b.uid]);
        assert_eq!(categories_of_task(&store, id), uids(&[&b]));
        // More than 10, or a malformed uid, is refused.
        let many: Vec<String> = (0..11).map(|n| format!("{n:032x}")).collect();
        assert!(store.set_document_categories(document, &many).is_err());
        assert!(store
            .set_document_categories(document, &["abc".to_owned()])
            .is_err());
        let mut task = Task::standalone();
        task.title = "Demais".into();
        task.categories = many;
        assert!(store.save_task(&task).is_err());
        // Clearing.
        store.set_document_categories(document, &[]).unwrap();
        assert!(store.document_categories(document).unwrap().is_empty());
        // Archived categories stay on the items that have them.
        store.set_category_archived(&b.uid, true).unwrap();
        assert_eq!(categories_of_task(&store, id), uids(&[&b]));
        // The payload keeps the field only when there is something in it.
        let plain = serde_json::to_string(&Task::standalone()).unwrap();
        assert!(!plain.contains("categories"));
    }

    #[test]
    fn deleting_a_category_takes_it_off_tasks_and_documents() {
        let (_dir, mut store) = store();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let a = make(&mut store, "Contrato");
        let b = make(&mut store, "Jurídico");
        store
            .set_document_categories(document, &[a.uid.clone(), b.uid.clone()])
            .unwrap();
        let first = task_with(&mut store, "Primeira", &[&a.uid, &b.uid]);
        let second = task_with(&mut store, "Segunda", &[&a.uid]);
        let usage = store.category_usage().unwrap();
        assert_eq!(usage[&a.uid], (2, 1));
        assert_eq!(usage[&b.uid], (1, 1));
        store.delete_category(&a.uid).unwrap();
        assert_eq!(categories_of_task(&store, first), uids(&[&b]));
        assert!(categories_of_task(&store, second).is_empty());
        assert_eq!(store.document_categories(document).unwrap(), uids(&[&b]));
        assert!(!store.category_usage().unwrap().contains_key(&a.uid));
        // The stored context lost the uid too, not only the listings.
        let raw = store.load_work_context(document).unwrap();
        assert_eq!(raw.categories, uids(&[&b]));
    }

    #[test]
    fn the_csv_exports_have_a_categories_column() {
        let (dir, mut store) = store();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let a = make(&mut store, "Contrato");
        let b = make(&mut store, "Jurídico");
        store
            .set_document_categories(document, &[a.uid.clone(), b.uid.clone()])
            .unwrap();
        task_with(&mut store, "Revisar", &[&b.uid, &a.uid]);
        let folder = dir.path().join("csv");
        std::fs::create_dir_all(&folder).unwrap();
        crate::export::export_to(&store, &folder).unwrap();
        for name in ["documentos.csv", "tarefas.csv"] {
            let text = std::fs::read_to_string(folder.join(name)).unwrap();
            assert!(text.contains("Categorias"), "{name}");
            assert!(text.contains("Contrato; Jurídico") || text.contains("Jurídico; Contrato"));
        }
    }

    #[test]
    fn a_schema_16_library_opens_with_everything_and_no_categories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sqlite3");
        let (document, task) = {
            let mut store = Store::open(&path).unwrap();
            let document = store
                .import_capture(&crate::model::sample_capture())
                .unwrap()
                .document_id;
            let task = task_with(&mut store, "Antiga", &[]);
            (document, task)
        };
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("DROP TABLE categories; PRAGMA user_version=16;")
                .unwrap();
        }
        let mut store = Store::open(&path).unwrap();
        let version: i64 = store
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::storage::LIBRARY_SCHEMA);
        assert!(store.list_categories().unwrap().is_empty());
        assert!(store.list_tasks().unwrap().iter().any(|t| t.id == task));
        assert_eq!(store.list_documents("").unwrap()[0].id, document);
        let a = make(&mut store, "Nova");
        store.set_document_categories(document, &[a.uid]).unwrap();
        // A library of a newer schema is refused, as before.
        drop(store);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version={};",
            crate::storage::LIBRARY_SCHEMA + 1
        ))
        .unwrap();
        drop(conn);
        assert!(Store::open(&path).is_err());
    }

    const KEY: &str = "senha-de-teste-123";

    fn folder_config(member: &str, files: &[String]) -> IntranetConfig {
        IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: files.to_vec(),
            folder_key: KEY.into(),
            ..IntranetConfig::default()
        }
    }

    fn sync(store: &mut Store) {
        let cfg = store.load_intranet_config().unwrap();
        store
            .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
            .unwrap();
    }

    #[test]
    fn the_shared_folder_carries_the_list_and_the_assignments_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
        let files = vec![folder.display().to_string()];
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Servidor A", &files))
            .unwrap();
        let document = a
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        sync(&mut a);
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Servidor B", &files))
            .unwrap();
        sync(&mut b);

        // A defines the list and uses it; B receives all of it.
        let contract = make(&mut a, "Contrato");
        let legal = make(&mut a, "Jurídico");
        a.set_document_categories(document, std::slice::from_ref(&contract.uid))
            .unwrap();
        let task = task_with(&mut a, "Revisar minuta", &[&contract.uid, &legal.uid]);
        sync(&mut a);
        sync(&mut b);
        assert_eq!(b.list_categories().unwrap(), a.list_categories().unwrap());
        assert_eq!(b.document_categories(document).unwrap(), uids(&[&contract]));
        assert_eq!(
            b.list_documents("").unwrap()[0].categories,
            uids(&[&contract])
        );
        assert_eq!(categories_of_task(&b, task), uids(&[&contract, &legal]));

        // B renames, archives, creates another and re-assigns; A receives it.
        let mut renamed = b.list_categories().unwrap()[0].clone();
        renamed.name = "Contratos".into();
        b.save_category(&renamed).unwrap();
        b.set_category_archived(&legal.uid, true).unwrap();
        let extra = make(&mut b, "Prazo");
        b.set_document_categories(document, std::slice::from_ref(&extra.uid))
            .unwrap();
        sync(&mut b);
        sync(&mut a);
        let names: Vec<_> = a
            .list_categories()
            .unwrap()
            .into_iter()
            .map(|c| (c.name, c.archived))
            .collect();
        assert_eq!(
            names,
            [
                ("Contratos".to_owned(), false),
                ("Jurídico".to_owned(), true),
                ("Prazo".to_owned(), false)
            ]
        );
        assert_eq!(a.document_categories(document).unwrap(), uids(&[&extra]));

        // A deletes one: it leaves the task on both computers.
        a.delete_category(&contract.uid).unwrap();
        sync(&mut a);
        sync(&mut b);
        assert_eq!(b.list_categories().unwrap().len(), 2);
        assert_eq!(categories_of_task(&b, task), uids(&[&legal]));
    }

    #[test]
    fn a_client_changes_categories_through_the_main_computer() {
        let dir = tempfile::tempdir().unwrap();
        let host_path = dir.path().join("host.sqlite3");
        let token = generate_token();
        let mut host = Store::open(&host_path).unwrap();
        host.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "Servidor A".into(),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        })
        .unwrap();
        let document = host
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let mut server = HostServer::start(host_path, "127.0.0.1:0").unwrap();
        let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
        client
            .save_intranet_config(&IntranetConfig {
                mode: IntranetMode::Client,
                member_name: "Servidor B".into(),
                host_url: format!("http://{}", server.local_addr()),
                cabinet_token: token,
                ..IntranetConfig::default()
            })
            .unwrap();
        // Same rules on the client side, told before anything is sent.
        let created = make(&mut client, "Orientação");
        assert_eq!(host.list_categories().unwrap()[0].uid, created.uid);
        assert!(client
            .save_category(&Category::new("orientacao", 1))
            .is_err());
        client
            .set_document_categories(document, std::slice::from_ref(&created.uid))
            .unwrap();
        assert_eq!(
            host.document_categories(document).unwrap(),
            vec![created.uid.clone()]
        );
        let id = task_with(&mut client, "Responder", &[&created.uid]);
        assert_eq!(categories_of_task(&host, id), vec![created.uid.clone()]);
        client.set_category_archived(&created.uid, true).unwrap();
        assert!(host.list_categories().unwrap()[0].archived);
        assert_eq!(
            client.category_version(&created.uid).unwrap(),
            host.category_version(&created.uid).unwrap()
        );
        client.delete_category(&created.uid).unwrap();
        assert!(host.list_categories().unwrap().is_empty());
        assert!(categories_of_task(&host, id).is_empty());
        assert!(host.document_categories(document).unwrap().is_empty());
        server.stop();
    }

    #[test]
    fn merging_two_edits_of_the_categories_of_an_item_loses_nothing() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let base = s(&["a", "b"]);
        // The host added "c"; the local side removed "a" and added "d".
        let merged = merge_uids(Some(&base), &s(&["a", "b", "c"]), &s(&["b", "d"]));
        assert_eq!(merged, s(&["b", "c", "d"]));
        // Without an ancestor: the union.
        assert_eq!(
            merge_uids(None, &s(&["a"]), &s(&["b", "a"])),
            s(&["a", "b"])
        );
    }

    fn host_with_document() -> (tempfile::TempDir, Store, IntranetConfig, i64) {
        let dir = tempfile::tempdir().unwrap();
        let mut host = Store::open(&dir.path().join("host.sqlite3")).unwrap();
        let cfg = IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "Servidor A".into(),
            cabinet_token: generate_token(),
            ..IntranetConfig::default()
        };
        host.save_intranet_config(&cfg).unwrap();
        let document = host
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        (dir, host, cfg, document)
    }

    /// A request as a Ponte before the categories sends it: no `client_schema`.
    fn old_request(mutation: IntranetMutation) -> crate::intranet::RpcRequest {
        let mut json = serde_json::to_value(crate::intranet::RpcRequest::Mutate {
            actor: "Servidor B".into(),
            mutation,
            client_schema: 16,
            expected_generation: String::new(),
            mutation_uid: String::new(),
            idents: Vec::new(),
            refs: None,
        })
        .unwrap();
        json["Mutate"]
            .as_object_mut()
            .unwrap()
            .remove("client_schema")
            .unwrap();
        serde_json::from_value(json).unwrap()
    }

    fn stored_task(store: &Store, id: i64) -> Task {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    #[test]
    fn an_older_client_saving_a_task_or_a_context_keeps_what_it_cannot_see() {
        use crate::intranet::{process_rpc, RpcRequest, RpcResponse};
        let (_dir, mut host, cfg, document) = host_with_document();
        let urgent = make(&mut host, "Urgente");
        let mut task = Task::standalone();
        task.title = "Responder".into();
        task.categories = vec![urgent.uid.clone()];
        task.proposal_id = "p".repeat(32);
        let id = host.save_task(&task).unwrap();
        host.set_document_categories(document, std::slice::from_ref(&urgent.uid))
            .unwrap();

        // What an older Ponte sends back: the same task without the fields.
        let mut seen = stored_task(&host, id);
        seen.notes = "Nova observação".into();
        seen.categories.clear();
        seen.proposal_id.clear();
        let reply = process_rpc(
            &mut host,
            &cfg,
            old_request(IntranetMutation::SaveTask {
                task: seen,
                expected_version: None,
                no_project: false,
            }),
        )
        .unwrap();
        assert!(matches!(reply, RpcResponse::Mutated { .. }));
        let stored = stored_task(&host, id);
        assert_eq!(stored.notes, "Nova observação");
        assert_eq!(stored.categories, vec![urgent.uid.clone()]);
        assert_eq!(stored.proposal_id, "p".repeat(32));

        let mut context = host.load_work_context(document).unwrap();
        context.notes = "Orientação nova".into();
        context.categories.clear();
        process_rpc(
            &mut host,
            &cfg,
            old_request(IntranetMutation::SaveWorkContext {
                document_id: document,
                value: context,
                expected_version: None,
            }),
        )
        .unwrap();
        let stored = host.load_work_context(document).unwrap();
        assert_eq!(stored.notes, "Orientação nova");
        assert_eq!(stored.categories, vec![urgent.uid.clone()]);

        // A current client can still take the categories off.
        let mut task = stored_task(&host, id);
        task.categories.clear();
        process_rpc(
            &mut host,
            &cfg,
            RpcRequest::Mutate {
                actor: "Servidor B".into(),
                mutation: IntranetMutation::SaveTask {
                    task,
                    expected_version: None,
                    no_project: false,
                },
                client_schema: crate::storage::SCHEMA_VERSION,
                expected_generation: String::new(),
                mutation_uid: String::new(),
                idents: Vec::new(),
                refs: None,
            },
        )
        .unwrap();
        assert!(categories_of_task(&host, id).is_empty());
    }

    #[test]
    fn the_same_name_created_offline_on_two_computers_keeps_both_assignments() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
        let files = vec![folder.display().to_string()];
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Servidor A", &files))
            .unwrap();
        sync(&mut a);
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Servidor B", &files))
            .unwrap();
        sync(&mut b);

        let on_a = make(&mut a, "Urgente");
        task_with(&mut a, "Tarefa de A", &[&on_a.uid]);
        let on_b = make(&mut b, "Urgente");
        task_with(&mut b, "Tarefa de B", &[&on_b.uid]);
        sync(&mut a);
        sync(&mut b);
        sync(&mut a);
        for store in [&a, &b] {
            let mut names: Vec<_> = store
                .list_categories()
                .unwrap()
                .into_iter()
                .map(|c| c.name)
                .collect();
            names.sort();
            assert_eq!(names, ["Urgente", "Urgente (2)"]);
            assert!(store.list_sync_conflicts().unwrap().is_empty());
            let name_of_task = |title: &str| {
                let task = store
                    .list_tasks()
                    .unwrap()
                    .into_iter()
                    .find(|t| t.title == title)
                    .unwrap();
                assert_eq!(task.categories.len(), 1);
                store
                    .list_categories()
                    .unwrap()
                    .into_iter()
                    .find(|c| c.uid == task.categories[0])
                    .unwrap()
                    .name
            };
            assert_eq!(name_of_task("Tarefa de A"), "Urgente");
            assert_eq!(name_of_task("Tarefa de B"), "Urgente (2)");
        }
    }

    #[test]
    fn applying_a_category_with_a_taken_name_renames_it_and_ignores_the_limit() {
        let (_dir, mut host, _cfg, _document) = host_with_document();
        make(&mut host, &"x".repeat(40));
        let mut clash = Category::new(&"x".repeat(40), 1);
        clash.uid = "b".repeat(32);
        host.execute_mutation(
            "Servidor B",
            IntranetMutation::SaveCategory {
                category: clash,
                expected_version: None,
            },
        )
        .unwrap();
        let names: Vec<_> = host
            .list_categories()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert!(names.contains(&format!("{} (2)", "x".repeat(36))));
        assert!(names.iter().all(|n| n.chars().count() <= MAX_NAME));
        // Past the limit: replayed changes are not refused.
        for n in 0..MAX_CATEGORIES {
            let mut c = Category::new(&format!("Extra {n}"), 0);
            c.uid = format!("{n:032x}");
            host.execute_mutation(
                "Servidor B",
                IntranetMutation::SaveCategory {
                    category: c,
                    expected_version: None,
                },
            )
            .unwrap();
        }
        assert!(host.list_categories().unwrap().len() > MAX_CATEGORIES);
        // The person in the interface is still refused.
        assert!(host.save_category(&Category::new("Mais uma", 0)).is_err());
    }

    #[test]
    fn an_edit_that_arrives_with_a_taken_name_is_stored_with_a_number() {
        let (_dir, mut host, _cfg, _document) = host_with_document();
        make(&mut host, "Urgente");
        let other = make(&mut host, "Rotina");
        // Someone renamed "Rotina" to "urgente" while another computer made
        // "Urgente": the edit is kept, not refused.
        let mut edited = other.clone();
        edited.name = "urgente".into();
        host.execute_mutation(
            "Servidor B",
            IntranetMutation::SaveCategory {
                category: edited.clone(),
                expected_version: None,
            },
        )
        .unwrap();
        let mut names: Vec<_> = host
            .list_categories()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        names.sort();
        assert_eq!(names, ["Urgente", "urgente (2)"]);
        // The person in the interface is still refused.
        let mut again = other;
        again.name = "Urgente".into();
        assert!(host.save_category(&again).is_err());
        // Saving a category again under its own name is no collision.
        edited.name = "urgente (2)".into();
        host.execute_mutation(
            "Servidor B",
            IntranetMutation::SaveCategory {
                category: edited,
                expected_version: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn deleting_a_category_already_deleted_is_not_a_conflict_when_applied() {
        let (_dir, mut host, _cfg, _document) = host_with_document();
        let made = make(&mut host, "Prazo");
        host.delete_category(&made.uid).unwrap();
        host.execute_mutation(
            "Servidor B",
            IntranetMutation::DeleteCategory {
                uid: made.uid.clone(),
                expected_version: None,
            },
        )
        .unwrap();
        // The interface keeps telling it.
        assert!(host.delete_category(&made.uid).is_err());
    }
}
