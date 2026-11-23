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
//! copy aside first. Items are found by what identifies them on every
//! computer (request ids and number identity, uids), never by row number,
//! which changes between copies.

use crate::intranet::{CabinetSnapshot, IntranetMutation};
use std::collections::BTreeSet;

/// What an item hangs from, when removing that also removes it.
enum Parent {
    Document(i64),
    Uid(String),
    Batch(i64),
}

/// One first-level item: its kind and number in the holder, what finds it
/// in another copy, and how it is shown to the person.
struct Item {
    kind: &'static str,
    id: i64,
    uid: String,
    keys: Vec<String>,
    parent: Option<Parent>,
    label: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn items(s: &CabinetSnapshot) -> Vec<Item> {
    let mut out = Vec::new();
    // What a task or an atendimento made automatically with its document
    // (the triage, the atendimento of an import) is made again, with a new
    // uid and time, when the import is replayed: it is also found by its
    // document and what it is.
    let identity: std::collections::BTreeMap<i64, String> = s
        .documents
        .iter()
        .map(|d| (d.id, hex(&d.identity_hash)))
        .collect();
    for doc in &s.documents {
        let current = s
            .captures
            .iter()
            .find(|c| Some(c.id) == doc.latest_capture_id)
            .map(|c| &c.capture);
        let title = current.map(|c| c.title.trim()).unwrap_or_default();
        // By its number identity, or by a request id that created it AND its
        // title: a renumbered record keeps both of these, while another
        // record that came with the same request id (and took its place in
        // the replay) does not have its title.
        let mut keys = vec![format!("documento:{}", hex(&doc.identity_hash))];
        keys.extend(
            s.requests
                .iter()
                .filter(|r| r.document_id == doc.id)
                .map(|r| format!("pedido:{}|{title}", r.request_id.to_lowercase())),
        );
        let label = current
            .map(|c| format!("documento {} — {title}", c.document_number.trim()))
            .unwrap_or_else(|| format!("documento #{}", doc.id));
        out.push(Item {
            kind: "document",
            id: doc.id,
            uid: String::new(),
            keys,
            parent: None,
            label,
        });
    }
    for task in &s.tasks {
        let key = if task.uid.is_empty() {
            format!("tarefa#{}:{}", task.id, task.title.trim())
        } else {
            format!("tarefa:{}", task.uid)
        };
        // A task of a copy written before task uids gets one when adopted:
        // it is also found by its number and title.
        let mut keys = vec![
            key,
            format!("tarefa-numero:{}|{}", task.id, task.title.trim()),
        ];
        if let Some(doc) = task.document_id.and_then(|d| identity.get(&d)) {
            keys.push(format!(
                "tarefa-do-documento:{doc}|{}|{}",
                task.title.trim(),
                task.action
            ));
        }
        out.push(Item {
            kind: "task",
            id: task.id,
            uid: task.uid.clone(),
            keys,
            parent: task.document_id.map(Parent::Document),
            label: format!("tarefa “{}”", task.title.trim()),
        });
    }
    for row in &s.saiop_attendances {
        let a = &row.attendance;
        let when = if a.created_at.is_empty() {
            format!("{}|{}", a.entry_date, a.subject.trim())
        } else {
            a.created_at.clone()
        };
        let name = if a.reference_raw.trim().is_empty() {
            a.subject.trim()
        } else {
            a.reference_raw.trim()
        };
        // Its creation time is given again when a new atendimento is
        // replayed: it is also found by what the person typed.
        let mut keys = vec![
            format!("atendimento:{}|{when}", a.reference_norm),
            format!(
                "atendimento-dados:{}|{}|{}",
                a.reference_norm,
                a.entry_date,
                a.subject.trim()
            ),
        ];
        if let Some(doc) = a.document_id.and_then(|d| identity.get(&d)) {
            keys.push(format!(
                "atendimento-do-documento:{doc}|{}",
                a.reference_norm
            ));
        }
        out.push(Item {
            kind: "saiop",
            id: a.id,
            uid: String::new(),
            keys,
            parent: a.batch_id.map(Parent::Batch),
            label: format!("atendimento {name}"),
        });
    }
    let mut by_uid = |kind: &'static str, uid: &str, parent: Option<Parent>, label: String| {
        out.push(Item {
            kind,
            id: 0,
            uid: uid.to_owned(),
            keys: vec![format!("{kind}:{uid}")],
            parent,
            label,
        });
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
    out
}

/// Every key that finds an item of `s`.
pub(crate) fn keys(s: &CabinetSnapshot) -> BTreeSet<String> {
    items(s).into_iter().flat_map(|i| i.keys).collect()
}

/// The removals a journal records: by number (in the numbering of the copy
/// the entry was written for) and by uid.
#[derive(Default)]
pub(crate) struct Removed {
    ids: BTreeSet<(&'static str, i64)>,
    uids: BTreeSet<String>,
    batches: BTreeSet<i64>,
}

impl Removed {
    pub(crate) fn note(&mut self, mutation: &IntranetMutation) {
        use IntranetMutation as M;
        match mutation {
            M::PurgeTrash { kind, id } if kind == "document" => {
                self.ids.insert(("document", *id));
            }
            M::PurgeTrash { kind, id } if kind == "task" => {
                self.ids.insert(("task", *id));
            }
            M::DeleteDocument { id } => {
                self.ids.insert(("document", *id));
            }
            M::DeleteSaiopAttendance { id } => {
                self.ids.insert(("saiop", *id));
            }
            M::UndoSaiopBatch { batch_id } => {
                self.batches.insert(*batch_id);
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

    /// A change that became a conflict card: what it created stays in the
    /// card, so its absence from the result is not a loss.
    pub(crate) fn kept_in_card(
        &mut self,
        mutation: &IntranetMutation,
        created: &[crate::idmap::CreatedId],
    ) {
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
        use crate::idmap::IdKind;
        for c in created {
            let kind = match c.kind {
                IdKind::Document => "document",
                IdKind::Task => "task",
                IdKind::Saiop => "saiop",
                _ => continue,
            };
            self.ids.insert((kind, c.id));
        }
    }

    /// The same removals without those recorded by number: for a copy
    /// whose numbers may name other items.
    pub(crate) fn by_uid_only(&self) -> Self {
        Self {
            ids: BTreeSet::new(),
            uids: self.uids.clone(),
            batches: BTreeSet::new(),
        }
    }

    fn explains(&self, item: &Item) -> bool {
        (item.id > 0 && self.ids.contains(&(item.kind, item.id)))
            || (!item.uid.is_empty() && self.uids.contains(&item.uid))
            || match &item.parent {
                Some(Parent::Document(doc)) => self.ids.contains(&("document", *doc)),
                Some(Parent::Uid(uid)) => self.uids.contains(uid),
                Some(Parent::Batch(batch)) => self.batches.contains(batch),
                None => false,
            }
    }
}

/// The items of `holder` that are neither in the result (`result_keys`) nor
/// removed by a change it records, as shown to the person.
pub(crate) fn dropped(
    holder: &CabinetSnapshot,
    result_keys: &BTreeSet<String>,
    removed: &[&Removed],
) -> Vec<String> {
    items(holder)
        .into_iter()
        .filter(|item| !item.keys.iter().any(|k| result_keys.contains(k)))
        .filter(|item| !removed.iter().any(|r| r.explains(item)))
        .map(|item| item.label)
        .collect()
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
