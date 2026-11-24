//! The adoption mapping of a mapped library (F3.0b of Frente F;
//! `desenho-f3-v3-adendo.md` §4 and §7, `desenho-f3-v2.md` §2.3 and §2.8).
//!
//! A kind is mapped on this device when it is in [`ADDRESSED`]: the wire is
//! the same for every F3 build (copy numbers plus proofs), and only the
//! mapper reads the list (v2 §2.4). It stays empty until the families map
//! their kinds, each with the references to it (F3.1a onwards), so every
//! adoption is still verbatim.

use crate::idents::{IdentRow, KINDS};
use crate::intranet::CabinetSnapshot;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap};

/// The ident kinds the mapper maps on this device (none yet: F3.1a adds
/// the first ones).
pub(crate) const ADDRESSED: &[&str] = &[];

/// Whether `kind` (an ident kind) is mapped on this device.
pub(crate) fn addressed(kind: &str) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(kinds) = hooks::ADDRESSED.with(|k| k.borrow().clone()) {
        return kinds.contains(&kind);
    }
    ADDRESSED.contains(&kind)
}

/// The numbers a mapped adoption gives here: copy number → number here,
/// by ident kind. A kind absent from it is adopted verbatim.
#[derive(Default)]
pub(crate) struct Mapping(BTreeMap<&'static str, BTreeMap<i64, i64>>);

impl Mapping {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number here of row `id` of `kind` in the copy: `None` when the
    /// kind is not mapped (the number stays), `Some(None)` when the copy has
    /// no such row.
    fn get(&self, kind: &str, id: i64) -> Option<Option<i64>> {
        self.0.get(kind).map(|m| m.get(&id).copied())
    }

    /// `blobs` keyed by the copy's numbers of `kind`, keyed by these.
    pub(crate) fn rekey<T>(&self, kind: &str, blobs: BTreeMap<i64, T>) -> BTreeMap<i64, T> {
        blobs
            .into_iter()
            .filter_map(|(id, blob)| match self.get(kind, id) {
                None => Some((id, blob)),
                Some(here) => here.map(|n| (n, blob)),
            })
            .collect()
    }
}

/// The numbers `snapshot`'s rows of the mapped kinds get in the library on
/// `tx`, whose id floor (read before anything was removed) is `floor`
/// (adendo §4, frozen rules 13 and 14). Only a mapped library maps:
/// - in the same epoch, a row whose identity is here keeps its number here;
///   then a row whose copy number is above the floor keeps it (reserved
///   first); the rest get numbers above the floor and the reserved, in the
///   copy's order;
/// - in another epoch nothing is kept nor reserved: every row gets a number
///   above the floor, in the copy's order.
///
/// No number at or below the floor is ever given to a row it did not name
/// here, so a retired number (a purged top, one held by a screen) names
/// nothing; the floor kept after the adoption covers what was given.
pub(crate) fn plan(
    tx: &Connection,
    snapshot: &CabinetSnapshot,
    floor: &BTreeMap<String, i64>,
) -> Result<Mapping> {
    let mut out = Mapping::default();
    if !KINDS.iter().any(|(k, _)| addressed(k)) || !crate::idents::is_mapped(tx)? {
        return Ok(out);
    }
    let epoch = crate::idents::origin(tx)?.2;
    let same_epoch =
        !snapshot.ident_epoch.is_empty() && epoch.as_deref() == Some(&*snapshot.ident_epoch);
    let carried: HashMap<(&str, i64), &str> = snapshot
        .idents
        .iter()
        .map(|IdentRow(k, row, ident)| ((k.as_str(), *row), ident.as_str()))
        .collect();
    let rows = crate::idents::projected_rows(snapshot);
    for (kind, table) in KINDS.iter().filter(|(k, _)| addressed(k)) {
        let high = floor.get(*table).copied().unwrap_or(0);
        let here: HashMap<String, i64> = if same_epoch {
            tx.prepare("SELECT ident,row_id FROM item_ident WHERE kind=?1")?
                .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        } else {
            HashMap::new()
        };
        let mut map = BTreeMap::new();
        let mut rest = Vec::new();
        for (_, id) in rows.iter().filter(|(k, _)| k == kind) {
            match carried.get(&(*kind, *id)).and_then(|i| here.get(*i)) {
                Some(local) => {
                    map.insert(*id, *local);
                }
                None => rest.push(*id),
            }
        }
        // Every number here is at or below the floor: a reserved one meets
        // none of them.
        let mut top = high;
        rest.retain(|id| {
            let reserved = same_epoch && *id > high;
            if reserved {
                map.insert(*id, *id);
                top = top.max(*id);
            }
            !reserved
        });
        for id in rest {
            top += 1;
            map.insert(id, top);
        }
        out.0.insert(kind, map);
    }
    Ok(out)
}

/// The rows' own numbers of `kind` in `s`.
fn own_ids<'a>(s: &'a mut CabinetSnapshot, kind: &str) -> Vec<&'a mut i64> {
    fn ids<T>(rows: &mut [T], id: fn(&mut T) -> &mut i64) -> Vec<&mut i64> {
        rows.iter_mut().map(id).collect()
    }
    match kind {
        "document" => ids(&mut s.documents, |r| &mut r.id),
        "capture" => ids(&mut s.captures, |r| &mut r.id),
        "note" => ids(&mut s.notes, |r| &mut r.id),
        "draft" => ids(&mut s.drafts, |r| &mut r.id),
        "analysis" => ids(&mut s.analyses, |r| &mut r.id),
        // The sealed metadata of a file and of an attachment, and the value
        // of an example, repeat the row's number.
        "file" => s
            .files_meta
            .iter_mut()
            .flat_map(|r| [&mut r.id, &mut r.summary.id])
            .collect(),
        "link" => ids(&mut s.document_links, |r| &mut r.id),
        "task" => ids(&mut s.tasks, |r| &mut r.id),
        "attachment" => s
            .attachments
            .iter_mut()
            .flat_map(|r| [&mut r.id, &mut r.meta.id])
            .collect(),
        "saiop" => ids(&mut s.saiop_attendances, |r| &mut r.attendance.id),
        "saiop_batch" => ids(&mut s.saiop_batches, |r| &mut r.id),
        "example" => s
            .templates
            .examples
            .iter_mut()
            .flat_map(|r| [&mut r.id, &mut r.value.id])
            .collect(),
        _ => Vec::new(),
    }
}

/// A reference to an item: one that is always there, or one that may be
/// absent.
pub(crate) enum Site<'a> {
    One(&'a mut i64),
    Maybe(&'a mut Option<i64>),
}

/// Visits every reference of `s` to an item of an ident kind
/// (`f1-inventario.md` §A), with the kind it names: the columns, the
/// copies of a number inside a payload (an analysis's version, a
/// generation's record, a use's value) and the polymorphic owners. The
/// rows' own numbers, the Lixeira, the version rows and the sources follow
/// rules of their own ([`apply`]).
pub(crate) fn visit_references(s: &mut CabinetSnapshot, visit: &mut dyn FnMut(&'static str, Site)) {
    use Site::{Maybe, One};
    for d in &mut s.documents {
        visit("capture", Maybe(&mut d.latest_capture_id));
    }
    for c in &mut s.captures {
        visit("document", One(&mut c.document_id));
    }
    for r in &mut s.requests {
        visit("document", One(&mut r.document_id));
        visit("capture", One(&mut r.capture_id));
    }
    for n in &mut s.notes {
        visit("document", One(&mut n.document_id));
    }
    for d in &mut s.drafts {
        visit("document", One(&mut d.document_id));
        visit("capture", One(&mut d.capture_id));
    }
    for a in &mut s.analyses {
        visit("document", One(&mut a.document_id));
        visit("capture", One(&mut a.capture_id));
        visit("capture", One(&mut a.analysis.capture_id));
    }
    for t in &mut s.tasks {
        visit("document", Maybe(&mut t.document_id));
        visit("task", Maybe(&mut t.parent_task_id));
        for dependency in &mut t.dependencies {
            visit("task", One(dependency));
        }
    }
    for (document, _) in &mut s.work_contexts {
        visit("document", One(document));
    }
    for f in &mut s.files_meta {
        visit("capture", One(&mut f.capture_id));
    }
    for p in &mut s.page_texts {
        visit("file", One(&mut p.file_id));
    }
    for l in &mut s.document_links {
        visit("document", One(&mut l.source_id));
        visit("document", One(&mut l.target_id));
    }
    for r in &mut s.capture_refs {
        visit("capture", One(&mut r.capture_id));
        visit("document", One(&mut r.owner_document));
        match r.owner_kind.as_str() {
            "draft" => visit("draft", One(&mut r.owner_id)),
            "analysis" => visit("analysis", One(&mut r.owner_id)),
            _ => {}
        }
    }
    for e in &mut s.team_events {
        visit("document", Maybe(&mut e.document_id));
    }
    for a in &mut s.saiop_attendances {
        let a = &mut a.attendance;
        visit("saiop_batch", Maybe(&mut a.batch_id));
        visit("document", Maybe(&mut a.document_id));
        for task in &mut a.task_ids {
            visit("task", One(task));
        }
    }
    for a in &mut s.attachments {
        if let Some(kind) = crate::idmap::IdKind::from_owner(&a.owner_kind) {
            visit(crate::idents::kind_of(kind), One(&mut a.owner_id));
        }
        visit("document", Maybe(&mut a.document_id));
    }
    for g in &mut s.templates.generations {
        visit("document", One(&mut g.document_id));
        visit("capture", One(&mut g.capture_id));
        visit("draft", Maybe(&mut g.draft_id));
        let r = &mut g.record;
        visit("document", One(&mut r.document_id));
        visit("capture", One(&mut r.capture_id));
        visit("draft", Maybe(&mut r.draft_id));
    }
    for u in &mut s.resources.uses {
        visit("document", Maybe(&mut u.document_id));
        let v = &mut u.value;
        visit("document", Maybe(&mut v.document_id));
        visit("saiop", Maybe(&mut v.saiop_id));
        visit("task", Maybe(&mut v.task_id));
    }
}

/// The ident kind whose number keys a version row of `kind` (v2 §2.8: the
/// row keys), `None` for the cabinet counter and the uid or setting keys.
pub(crate) fn row_kind(kind: &str) -> Option<&'static str> {
    let (_, table) = crate::intranet::ROW_VERSION_KINDS
        .iter()
        .find(|(k, _)| *k == kind)?;
    KINDS.iter().find(|(_, t)| t == table).map(|(k, _)| *k)
}

/// The key of a source whose item the copy no longer has (adendo §7,
/// frozen rule 16): `R` and 12 hex digits of the sha256 of its name in
/// the copy. The same on every device, distinct for distinct names, and
/// never read as numbers (no `C`, `F` or `A` grammar), so never resolved.
pub(crate) fn retired_key(name: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "R{}",
        &format!("{:x}", Sha256::digest(name.as_bytes()))[..12]
    )
}

/// One source, by the mapping: its numbers here and its name after them;
/// when a mapped item it names is not in the copy, the retired key and
/// every number 0. The old name and the new one, when it changed.
fn source(
    name: &mut String,
    document: &mut i64,
    capture: &mut i64,
    file: &mut Option<i64>,
    m: &Mapping,
) -> Option<(String, String)> {
    let old = name.clone();
    let mut retired = false;
    let renamed = crate::idrefs::visit_source(name, document, capture, file, &mut |kind, id| {
        match m.get(crate::idents::kind_of(kind), *id) {
            Some(Some(here)) if *id > 0 => *id = here,
            Some(None) if *id > 0 => retired = true,
            _ => {}
        }
        Ok(())
    })
    .ok()
    .flatten();
    if !retired {
        return renamed;
    }
    *name = retired_key(&old);
    (*document, *capture) = (0, 0);
    if let Some(file) = file {
        *file = 0;
    }
    Some((old, name.clone()))
}

fn sources(list: &mut [crate::evidence::Source], m: &Mapping) -> BTreeMap<String, String> {
    list.iter_mut()
        .filter_map(|s| {
            source(
                &mut s.id,
                &mut s.document_id,
                &mut s.capture_id,
                &mut s.file_id,
                m,
            )
        })
        .collect()
}

/// `snapshot` in this library's numbers ([`plan`]): the rows' own numbers,
/// their identities, every reference to an item ([`visit_references`]; one
/// the copy does not have is named by no number here: 0, or none), the
/// version rows (v2 §2.8: a row key follows its row, and is dropped when
/// the copy has no such row; the cabinet counter and the uid keys stay),
/// the Lixeira by the same rule, and the sources with the citations and
/// facts that name them (adendo §7).
pub(crate) fn apply(snapshot: &CabinetSnapshot, m: &Mapping) -> CabinetSnapshot {
    let mut s = snapshot.clone();
    for kind in m.0.keys() {
        for id in own_ids(&mut s, kind) {
            if let Some(Some(here)) = m.get(kind, *id) {
                *id = here;
            }
        }
    }
    visit_references(&mut s, &mut |kind, site| match site {
        Site::One(id) if *id > 0 => {
            if let Some(here) = m.get(kind, *id) {
                *id = here.unwrap_or(0);
            }
        }
        Site::Maybe(id) => {
            if let Some(here) = id.filter(|n| *n > 0).and_then(|n| m.get(kind, n)) {
                *id = here;
            }
        }
        Site::One(_) => {}
    });
    for IdentRow(kind, row, _) in &mut s.idents {
        if let Some(Some(here)) = m.get(kind, *row) {
            *row = here;
        }
    }
    s.idents.sort();
    // `None`: dropped; otherwise the number here.
    let follow = |kind: Option<&str>, id: i64| match kind.and_then(|k| m.get(k, id)) {
        None => Some(id),
        Some(here) => here,
    };
    s.item_versions
        .retain_mut(|(kind, id, _)| follow(row_kind(kind), *id).map(|n| *id = n).is_some());
    s.trash.retain_mut(|t| {
        let kind = crate::idmap::IdKind::from_trash(&t.kind).map(crate::idents::kind_of);
        let document = t.document_id.map(|d| follow(Some("document"), d));
        match (follow(kind, t.item_id), document) {
            (None, _) | (_, Some(None)) => false,
            (Some(item), document) => {
                t.item_id = item;
                t.document_id = document.flatten();
                true
            }
        }
    });
    for row in &mut s.analyses {
        let a = &mut row.analysis;
        let renamed = sources(&mut a.materials, m);
        let names = a
            .citations
            .iter_mut()
            .map(|c| &mut c.source_id)
            .chain(a.facts.iter_mut().map(|f| &mut f.source_id));
        for name in names {
            if let Some(new) = renamed.get(name.as_str()) {
                name.clone_from(new);
            }
        }
    }
    for row in &mut s.drafts {
        sources(&mut row.materials, m);
    }
    for row in &mut s.templates.generations {
        for u in &mut row.record.sources {
            source(
                &mut u.id,
                &mut u.document_id,
                &mut u.capture_id,
                &mut u.file_id,
                m,
            );
        }
    }
    s
}

/// The kind set of the tests of the mapper, before any family maps one.
#[cfg(any(test, feature = "test-support"))]
pub mod hooks {
    thread_local! {
        /// Replaces [`super::ADDRESSED`] on this thread.
        pub static ADDRESSED: std::cell::RefCell<Option<Vec<&'static str>>> =
            const { std::cell::RefCell::new(None) };
    }
}
