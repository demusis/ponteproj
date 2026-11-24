//! Whether the result of a shared-folder exchange covers what a copy (or
//! this library) holds, before that copy is rewritten or replaced (F0,
//! R2-B4).
//!
//! An exchange replaces whole copies with its result. A first-level item
//! (document, task, atendimento, recurso, uso, projeto, categoria, modelo,
//! bloco, exemplo) missing from the result may have been removed by a change
//! the result carries, or may exist only in the copy being replaced (it was
//! never in any journal, so nothing replayed it). The two cannot be told
//! apart by its absence alone, so an absence is accepted only when a removal
//! recorded in the result's journal explains it; any other absence keeps a
//! copy aside first.
//!
//! An item is found in the result by what proves it is the same item:
//! - a uid (tasks, recursos, usos, projetos, categorias, modelos, blocos,
//!   exemplos), never a title;
//! - for a document, its number identity, or a request id that created it
//!   together with the title of the version that request made;
//! - for an atendimento of a spreadsheet, the file of its batch and its row;
//! - what a replay in this very exchange made of it (the slots `created` of
//!   the replayed change: an import's triage task and atendimento, a new
//!   record, task or atendimento);
//! - in the same numbering only (a copy whose changes the result all has,
//!   or this library): an atendimento by number and creation time, a task
//!   without uid (from a copy written before task uids) by number and title;
//! - a triage task without uid, by its document's request id and its action
//!   (a task with a uid is found by its uid or by the replay's `triage`
//!   slot, never by this key: one document may have two triage tasks).
//!
//! A false alarm only keeps one more copy aside; a false match would lose an
//! item, so a match needs one of the proofs above.

use crate::idmap::{CreatedId, IdKind};
use crate::idrefs::Proof;
use crate::intranet::{CabinetSnapshot, IntranetMutation};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// What an item hangs from, when removing that also removes it.
#[derive(Clone)]
enum Parent {
    Document(i64),
    Uid(String),
    Batch(i64),
}

/// One first-level item: its kind and number in the holder, the keys that
/// find it in the result, and how it is shown to the person.
#[derive(Clone)]
pub(crate) struct Item {
    kind: &'static str,
    id: i64,
    uid: String,
    /// Proofs valid whatever the numbering.
    keys: Vec<String>,
    /// Proofs valid only in the same numbering.
    frame_keys: Vec<String>,
    parent: Option<Parent>,
    /// The identities of the item and of its parent in the holder (v2
    /// §2.6), and whether the holder numbers the mapped kinds on its own:
    /// this library, once mapped (a copy, or a library that adopts
    /// verbatim, numbers as the copies do).
    ident: Option<String>,
    parent_ident: Option<String>,
    here: bool,
    label: String,
    /// The keys and label still to be read (a library reads them only for
    /// the items the cheap keys did not find).
    complete: bool,
}

impl Item {
    fn new(kind: &'static str, id: i64, uid: &str, parent: Option<Parent>) -> Self {
        Self {
            kind,
            id,
            uid: uid.to_owned(),
            keys: Vec::new(),
            frame_keys: Vec::new(),
            parent,
            ident: None,
            parent_ident: None,
            here: false,
            label: String::new(),
            complete: true,
        }
    }
}

/// Gives `items` their identities and their parents', as `ident_of` reads
/// them in their holder (`here`: this library).
fn with_idents(items: &mut [Item], here: bool, ident_of: impl Fn(&str, i64) -> Option<String>) {
    for item in items {
        item.here = here;
        if item.id > 0 {
            item.ident = ident_of(item.kind, item.id);
        }
        item.parent_ident = match &item.parent {
            Some(Parent::Document(doc)) => ident_of("document", *doc),
            Some(Parent::Batch(batch)) => ident_of("saiop_batch", *batch),
            _ => None,
        };
    }
}

/// Something whose items are checked: a copy's snapshot or this library.
pub(crate) trait Holder {
    /// Every item with its cheap keys.
    fn items(&self) -> Result<Vec<Item>>;
    /// Adds what an item's cheap keys left out (the rest of its keys, its
    /// label); nothing for a snapshot, where everything is at hand.
    fn complete(&self, item: &mut Item) -> Result<()>;
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn document_label(number: &str, title: &str) -> String {
    format!("documento {} — {}", number.trim(), title.trim())
}

fn task_keys(item: &mut Item, title: &str, action: &str, requests: &[String]) {
    if item.uid.is_empty() {
        item.frame_keys
            .push(format!("tarefa#{}|{}", item.id, title.trim()));
    }
    // A task with a uid is found by its uid only (a replay that made it
    // again is found through the replay's `triage` slot): two triage tasks
    // of one document are two items. The document + action key is for a
    // triage task without uid (a copy written before task uids).
    if action == "triar" && item.uid.is_empty() {
        item.keys
            .extend(requests.iter().map(|r| format!("triagem:{r}")));
    }
    item.label = format!("tarefa “{}”", title.trim());
}

/// An atendimento by its number and the moment it was created (to the
/// millisecond): together they name one atendimento whatever else changed
/// in it. Without that moment (older rows), the number alone counts only in
/// the same numbering.
fn attendance_keys(item: &mut Item, created_at: &str) {
    let key = format!("atendimento#{}|{}", item.id, created_at.trim());
    if created_at.trim().is_empty() {
        item.frame_keys.push(key);
    } else {
        item.keys.push(key);
    }
}

fn attendance_label(a: &crate::saiop::SaiopAttendance) -> String {
    let name = if a.reference_raw.trim().is_empty() {
        a.subject.trim()
    } else {
        a.reference_raw.trim()
    };
    format!("atendimento {name}")
}

// ------------------------------------------------------------ snapshots --

/// The items of a copy (or of the result), all keys at once.
pub(crate) struct SnapshotHolder<'a>(pub(crate) &'a CabinetSnapshot);

impl Holder for SnapshotHolder<'_> {
    fn items(&self) -> Result<Vec<Item>> {
        let s = self.0;
        let captures: HashMap<i64, &crate::model::Capture> =
            s.captures.iter().map(|c| (c.id, &c.capture)).collect();
        let mut requests: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
        for r in &s.requests {
            requests
                .entry(r.document_id)
                .or_default()
                .push((r.request_id.to_lowercase(), r.capture_id));
        }
        let none = Vec::new();
        let mut out = Vec::new();
        for doc in &s.documents {
            let mut item = Item::new("document", doc.id, "", None);
            item.keys
                .push(format!("documento:{}", hex(&doc.identity_hash)));
            for (request, capture) in requests.get(&doc.id).unwrap_or(&none) {
                if let Some(made) = captures.get(capture) {
                    item.keys
                        .push(format!("pedido:{request}|{}", made.title.trim()));
                }
            }
            item.label = match doc.latest_capture_id.and_then(|c| captures.get(&c)) {
                Some(c) => document_label(&c.document_number, &c.title),
                None => format!("documento #{}", doc.id),
            };
            out.push(item);
        }
        for task in &s.tasks {
            let mut item = Item::new(
                "task",
                task.id,
                &task.uid,
                task.document_id.map(Parent::Document),
            );
            if !task.uid.is_empty() {
                item.keys.push(format!("tarefa:{}", task.uid));
            }
            let of_document: Vec<String> = task
                .document_id
                .and_then(|d| requests.get(&d))
                .map(|rs| rs.iter().map(|(r, _)| r.clone()).collect())
                .unwrap_or_default();
            task_keys(&mut item, &task.title, &task.action, &of_document);
            out.push(item);
        }
        let batches: HashMap<i64, &str> = s
            .saiop_batches
            .iter()
            .map(|b| (b.id, b.file_sha256.as_str()))
            .collect();
        for row in &s.saiop_attendances {
            let a = &row.attendance;
            let mut item = Item::new("saiop", a.id, "", a.batch_id.map(Parent::Batch));
            if let (Some(sha), Some(source)) =
                (a.batch_id.and_then(|b| batches.get(&b)), a.source_row)
            {
                item.keys.push(format!("atendimento-lote:{sha}|{source}"));
            }
            attendance_keys(&mut item, &a.created_at);
            item.label = attendance_label(a);
            out.push(item);
        }
        let mut by_uid = |kind: &'static str, uid: &str, parent: Option<Parent>, label: String| {
            let mut item = Item::new(kind, 0, uid, parent);
            item.keys.push(format!("{kind}:{uid}"));
            item.label = label;
            out.push(item);
        };
        for r in &s.resources.resources {
            by_uid(
                "resource",
                &r.uid,
                None,
                format!("recurso “{}”", r.value.name.trim()),
            );
        }
        for u in &s.resources.uses {
            by_uid(
                "resource_use",
                &u.uid,
                Some(Parent::Uid(u.resource_uid.clone())),
                format!(
                    "uso de recurso por {} ({})",
                    u.value.who.trim(),
                    u.value.start.trim()
                ),
            );
        }
        for p in &s.projects {
            by_uid(
                "project",
                &p.uid,
                None,
                format!("projeto “{}”", p.name.trim()),
            );
        }
        for c in &s.categories {
            by_uid(
                "category",
                &c.uid,
                None,
                format!("categoria “{}”", c.name.trim()),
            );
        }
        for t in &s.templates.templates {
            by_uid(
                "template",
                &t.uid,
                None,
                format!("modelo “{}”", t.head.name.trim()),
            );
        }
        for b in &s.templates.blocks {
            by_uid(
                "block",
                &b.uid,
                None,
                format!("bloco “{}”", b.head.name.trim()),
            );
        }
        for e in &s.templates.examples {
            by_uid(
                "example",
                &e.uid,
                Some(Parent::Uid(e.value.template_uid.clone())),
                format!("exemplo “{}”", e.value.title.trim()),
            );
        }
        let idents: HashMap<(&str, i64), &str> = s
            .idents
            .iter()
            .map(|crate::idents::IdentRow(k, row, i)| ((k.as_str(), *row), i.as_str()))
            .collect();
        with_idents(&mut out, false, |k, row| {
            idents.get(&(k, row)).map(|i| i.to_string())
        });
        Ok(out)
    }

    fn complete(&self, _item: &mut Item) -> Result<()> {
        Ok(())
    }
}

// -------------------------------------------------------------- library --

/// The items of this library, read with narrow queries: numbers, uids and
/// keys from their columns; a payload is decrypted only for an item those
/// did not find in the result (its title, action or creation time).
pub(crate) struct LibraryHolder<'a> {
    conn: &'a Connection,
    requests: HashMap<i64, Vec<(String, i64)>>,
    /// The library is mapped: its numbers of the mapped kinds are its own.
    mapped: bool,
}

impl<'a> LibraryHolder<'a> {
    pub(crate) fn new(conn: &'a Connection) -> Result<Self> {
        let mut requests: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
        let mut statement =
            conn.prepare("SELECT request_id,document_id,capture_id FROM requests")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (request, document, capture) = row?;
            requests
                .entry(document)
                .or_default()
                .push((request.to_lowercase(), capture));
        }
        Ok(Self {
            conn,
            requests,
            mapped: crate::idents::is_mapped(conn)?,
        })
    }

    fn sealed<T: serde::de::DeserializeOwned>(
        &self,
        sql: &str,
        key: rusqlite::types::Value,
    ) -> Result<Option<T>> {
        let blob: Option<Vec<u8>> = self.conn.query_row(sql, [key], |r| r.get(0)).optional()?;
        match blob {
            Some(blob) => Ok(Some(serde_json::from_slice(&crate::protection::unseal(
                &blob,
            )?)?)),
            None => Ok(None),
        }
    }

    fn capture(&self, id: i64) -> Result<Option<crate::model::Capture>> {
        self.sealed(
            "SELECT payload FROM captures WHERE id=?1",
            rusqlite::types::Value::Integer(id),
        )
    }

    fn uid_label(&self, table: &str, uid: &str) -> Option<String> {
        let value: serde_json::Value = self
            .sealed(
                &format!("SELECT payload FROM {table} WHERE uid=?1"),
                rusqlite::types::Value::Text(uid.to_owned()),
            )
            .ok()
            .flatten()?;
        ["name", "title", "who"]
            .iter()
            .find_map(|k| value.get(*k).and_then(|v| v.as_str()))
            .map(|s| s.trim().to_owned())
    }
}

impl Holder for LibraryHolder<'_> {
    fn items(&self) -> Result<Vec<Item>> {
        let mut out = Vec::new();
        let mut statement = self
            .conn
            .prepare("SELECT id,identity_hash FROM documents")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        for row in rows {
            let (id, identity) = row?;
            let mut item = Item::new("document", id, "", None);
            item.keys.push(format!("documento:{}", hex(&identity)));
            item.complete = false;
            out.push(item);
        }
        let mut statement = self.conn.prepare("SELECT id,uid,document_id FROM tasks")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, uid, document) = row?;
            let mut item = Item::new("task", id, &uid, document.map(Parent::Document));
            if !uid.is_empty() {
                item.keys.push(format!("tarefa:{uid}"));
            }
            item.complete = false;
            out.push(item);
        }
        let batches: HashMap<i64, String> = self
            .conn
            .prepare("SELECT id,file_sha256 FROM saiop_batches")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut statement = self
            .conn
            .prepare("SELECT id,batch_id,source_row FROM saiop_attendances")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for row in rows {
            let (id, batch, source) = row?;
            let mut item = Item::new("saiop", id, "", batch.map(Parent::Batch));
            if let (Some(sha), Some(source)) = (batch.and_then(|b| batches.get(&b)), source) {
                item.keys.push(format!("atendimento-lote:{sha}|{source}"));
            }
            item.complete = false;
            out.push(item);
        }
        for (kind, sql) in [
            ("resource", "SELECT uid,NULL FROM resources"),
            ("resource_use", "SELECT uid,resource_uid FROM resource_uses"),
            ("project", "SELECT uid,NULL FROM projects"),
            ("category", "SELECT uid,NULL FROM categories"),
            ("template", "SELECT uid,NULL FROM work_templates"),
            ("block", "SELECT uid,NULL FROM instruction_blocks"),
            (
                "example",
                "SELECT e.uid,t.uid FROM template_examples e LEFT JOIN work_templates t ON t.id=e.template_id",
            ),
        ] {
            let mut statement = self.conn.prepare(sql)?;
            let rows = statement.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            for row in rows {
                let (uid, parent) = row?;
                let mut item = Item::new(kind, 0, &uid, parent.map(Parent::Uid));
                item.keys.push(format!("{kind}:{uid}"));
                item.complete = false;
                out.push(item);
            }
        }
        let mut idents: HashMap<(String, i64), String> = HashMap::new();
        if crate::idents::has_idents(self.conn)? {
            let mut statement = self.conn.prepare(
                "SELECT kind,row_id,ident FROM item_ident WHERE kind IN ('document','task','saiop','saiop_batch')",
            )?;
            let rows = statement.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?;
            for row in rows {
                let (key, ident) = row?;
                idents.insert(key, ident);
            }
        }
        with_idents(&mut out, self.mapped, |k, row| {
            idents.get(&(k.to_owned(), row)).cloned()
        });
        Ok(out)
    }

    fn complete(&self, item: &mut Item) -> Result<()> {
        if item.complete {
            return Ok(());
        }
        item.complete = true;
        let none = Vec::new();
        match item.kind {
            "document" => {
                for (request, capture) in self.requests.get(&item.id).unwrap_or(&none) {
                    if let Some(made) = self.capture(*capture)? {
                        item.keys
                            .push(format!("pedido:{request}|{}", made.title.trim()));
                    }
                }
                let latest: Option<i64> = self
                    .conn
                    .query_row(
                        "SELECT latest_capture_id FROM documents WHERE id=?1",
                        [item.id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .flatten();
                item.label = match latest.map(|c| self.capture(c)).transpose()?.flatten() {
                    Some(c) => document_label(&c.document_number, &c.title),
                    None => format!("documento #{}", item.id),
                };
            }
            "task" => {
                let task: Option<crate::tasks::Task> = self.sealed(
                    "SELECT payload FROM tasks WHERE id=?1",
                    rusqlite::types::Value::Integer(item.id),
                )?;
                let (title, action) = task.map(|t| (t.title, t.action)).unwrap_or_default();
                let of_document: Vec<String> = match &item.parent {
                    Some(Parent::Document(d)) => self
                        .requests
                        .get(d)
                        .map(|rs| rs.iter().map(|(r, _)| r.clone()).collect())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                task_keys(item, &title, &action, &of_document);
            }
            "saiop" => {
                let a: Option<crate::saiop::SaiopAttendance> = self.sealed(
                    "SELECT payload FROM saiop_attendances WHERE id=?1",
                    rusqlite::types::Value::Integer(item.id),
                )?;
                match a {
                    Some(a) => {
                        attendance_keys(item, &a.created_at);
                        item.label = attendance_label(&a);
                    }
                    None => item.label = format!("atendimento #{}", item.id),
                }
            }
            kind => {
                let (table, word) = match kind {
                    "resource" => ("resources", "recurso"),
                    "resource_use" => ("resource_uses", "uso de recurso"),
                    "project" => ("projects", "projeto"),
                    "category" => ("categories", "categoria"),
                    "template" => ("work_templates", "modelo"),
                    "block" => ("instruction_blocks", "bloco"),
                    _ => ("template_examples", "exemplo"),
                };
                item.label = match self.uid_label(table, &item.uid) {
                    Some(name) => format!("{word} “{name}”"),
                    None => word.to_owned(),
                };
            }
        }
        Ok(())
    }
}

// --------------------------------------------------------------- result --

/// What finds an item in the result.
pub(crate) struct ResultIndex {
    keys: BTreeSet<String>,
    frame_keys: BTreeSet<String>,
    present: BTreeSet<(&'static str, i64)>,
}

impl ResultIndex {
    pub(crate) fn of(result: &CabinetSnapshot) -> Self {
        let items = SnapshotHolder(result).items().unwrap_or_default();
        Self {
            keys: items.iter().flat_map(|i| i.keys.iter().cloned()).collect(),
            frame_keys: items
                .iter()
                .flat_map(|i| i.frame_keys.iter().cloned())
                .collect(),
            present: items
                .iter()
                .filter(|i| i.id > 0)
                .map(|i| (i.kind, i.id))
                .collect(),
        }
    }
}

/// What the replays of this exchange made of the holder's items: the
/// holder's (kind, number) to the result's number, from the `created`
/// slots of each change replayed successfully.
#[derive(Default)]
pub(crate) struct Replayed(BTreeMap<(&'static str, i64), i64>);

impl Replayed {
    /// The mappings of `other`, what the replays made of a copy's items (in
    /// the copy's numbers), for the kinds the library on `conn` numbers as
    /// the copy it adopted: in a mapped library a mapped kind has numbers
    /// of its own, which the copy's say nothing of (F3.1a).
    pub(crate) fn absorb(&mut self, other: &Replayed, conn: &Connection) -> Result<()> {
        let mapped = crate::idents::is_mapped(conn)?;
        self.0.extend(
            other
                .0
                .iter()
                .filter(|((kind, _), _)| !(mapped && crate::mapper::addressed(kind)))
                .map(|(k, v)| (*k, *v)),
        );
        Ok(())
    }

    pub(crate) fn learn(&mut self, there: &[CreatedId], here: &[CreatedId]) {
        for from in there {
            let kind = match from.kind {
                IdKind::Document => "document",
                IdKind::Task => "task",
                IdKind::Saiop => "saiop",
                _ => continue,
            };
            if let Some(to) = here
                .iter()
                .find(|c| c.kind == from.kind && c.slot == from.slot)
            {
                self.0.insert((kind, from.id), to.id);
            }
        }
    }
}

/// The removals a journal records: by uid; for a kind mapped here (v2
/// §2.6), by the identity the entry's proof names, or, from an entry
/// without proofs (an older Ponte's), by number for the items of a copy
/// only; for any other kind, by number (in the numbering of the copy the
/// entry was written for).
#[derive(Default)]
pub(crate) struct Removed {
    ids: BTreeSet<(&'static str, i64)>,
    legacy: BTreeSet<(&'static str, i64)>,
    idents: BTreeSet<(&'static str, String)>,
    uids: BTreeSet<String>,
    /// What changes that became cards created, by number in the frame of
    /// the holder they are checked against (this library's own queue, or
    /// one copy's journal): never a legacy number, whatever the kind.
    carded: BTreeSet<(&'static str, i64)>,
}

impl Removed {
    /// The removal of row `id` of `kind` (an ident kind), proved by `proof`
    /// (`None`: an entry without proofs).
    fn removal(&mut self, kind: &'static str, id: i64, proof: Option<Option<&Proof>>) {
        if !crate::mapper::addressed(kind) {
            self.ids.insert((kind, id));
            return;
        }
        match proof {
            None => {
                self.legacy.insert((kind, id));
            }
            Some(Some(Proof::Ident(ident))) => {
                self.idents.insert((kind, ident.clone()));
            }
            // Proved by no identity: it explains nothing.
            Some(_) => {}
        }
    }

    pub(crate) fn note(&mut self, mutation: &IntranetMutation, refs: Option<&crate::idrefs::Refs>) {
        use IntranetMutation as M;
        // A removal names one site.
        let proof = refs.map(|r| r.mutation.first().map(|site| &site.proof));
        match mutation {
            M::PurgeTrash { kind, id } if kind == "document" => {
                self.removal("document", *id, proof);
            }
            M::PurgeTrash { kind, id } if kind == "task" => {
                self.removal("task", *id, proof);
            }
            M::DeleteDocument { id } => {
                self.removal("document", *id, proof);
            }
            M::DeleteSaiopAttendance { id } => {
                self.removal("saiop", *id, proof);
            }
            M::UndoSaiopBatch { batch_id } => {
                self.removal("saiop_batch", *batch_id, proof);
            }
            M::DeleteTemplate { uid }
            | M::DeleteBlock { uid }
            | M::DeleteExample { uid }
            | M::DeleteResource { uid, .. }
            | M::DeleteResourceUse { uid, .. }
            | M::DeleteCategory { uid, .. }
            | M::DeleteProject { uid, .. } => {
                self.uids.insert(uid.clone());
            }
            _ => {}
        }
    }

    /// A change that became a conflict card: what it created (or the uid
    /// item it saves) stays in the card, so its absence is not a loss.
    pub(crate) fn kept_in_card(&mut self, mutation: &IntranetMutation, created: &[CreatedId]) {
        use IntranetMutation as M;
        let uid = match mutation {
            M::SaveTask { task, .. } => Some(&task.uid),
            M::SaveResource { resource, .. } => Some(&resource.uid),
            M::SaveResourceUse { item, .. } => Some(&item.uid),
            M::SaveProject { project, .. } => Some(&project.uid),
            M::SaveCategory { category, .. } => Some(&category.uid),
            M::SaveTemplate { template, .. } => Some(&template.uid),
            M::SaveInstructionBlock { block, .. } => Some(&block.uid),
            M::SaveExample { example, .. } => Some(&example.uid),
            _ => None,
        };
        if let Some(uid) = uid.filter(|u| !u.is_empty()) {
            self.uids.insert(uid.clone());
        }
        for c in created {
            let kind = match c.kind {
                IdKind::Document => "document",
                IdKind::Task => "task",
                IdKind::Saiop => "saiop",
                _ => continue,
            };
            self.carded.insert((kind, c.id));
        }
    }

    /// The same removals without those recorded by number: for a holder
    /// whose numbers may name other items.
    pub(crate) fn by_uid_only(&self) -> Self {
        Self {
            ids: BTreeSet::new(),
            legacy: BTreeSet::new(),
            idents: self.idents.clone(),
            uids: self.uids.clone(),
            carded: BTreeSet::new(),
        }
    }

    fn explains(&self, item: &Item) -> bool {
        let removed = |kind: &'static str, id: i64, ident: &Option<String>| {
            if self.carded.contains(&(kind, id)) {
                true
            } else if crate::mapper::addressed(kind) {
                ident
                    .as_ref()
                    .is_some_and(|i| self.idents.contains(&(kind, i.clone())))
                    || (!item.here && self.legacy.contains(&(kind, id)))
            } else {
                self.ids.contains(&(kind, id))
            }
        };
        (item.id > 0 && removed(item.kind, item.id, &item.ident))
            || (!item.uid.is_empty() && self.uids.contains(&item.uid))
            || match &item.parent {
                Some(Parent::Document(doc)) => removed("document", *doc, &item.parent_ident),
                Some(Parent::Uid(uid)) => self.uids.contains(uid),
                Some(Parent::Batch(batch)) => removed("saiop_batch", *batch, &item.parent_ident),
                None => false,
            }
    }
}

/// How a holder is checked against the result.
pub(crate) struct Check<'a> {
    pub(crate) result: &'a ResultIndex,
    /// Removals and cards that explain an absence.
    pub(crate) explained: &'a [&'a Removed],
    /// The holder's numbers are the result's (frame keys count).
    pub(crate) same_numbering: bool,
    pub(crate) replayed: Option<&'a Replayed>,
}

/// The items of `holder` the result neither has nor explains, as shown to
/// the person.
pub(crate) fn dropped(holder: &dyn Holder, check: &Check) -> Result<Vec<String>> {
    let found = |item: &Item| {
        item.keys.iter().any(|k| check.result.keys.contains(k))
            || (check.same_numbering
                && item
                    .frame_keys
                    .iter()
                    .any(|k| check.result.frame_keys.contains(k)))
            || check
                .replayed
                .and_then(|r| r.0.get(&(item.kind, item.id)))
                .is_some_and(|there| check.result.present.contains(&(item.kind, *there)))
            || check.explained.iter().any(|r| r.explains(item))
    };
    let mut out = Vec::new();
    for mut item in holder.items()? {
        if found(&item) {
            continue;
        }
        holder.complete(&mut item)?;
        if !found(&item) {
            out.push(item.label);
        }
    }
    Ok(out)
}

/// "a, b, c e mais 4": at most six names.
pub(crate) fn describe(labels: &[String]) -> String {
    const SHOWN: usize = 6;
    let mut text = labels
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if labels.len() > SHOWN {
        text.push_str(&format!(" e mais {}", labels.len() - SHOWN));
    }
    text
}
