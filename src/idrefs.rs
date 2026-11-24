//! The id sites of a change, and the proof of each (F3 of Frente F).
//!
//! A change names items by row number ("sites": the note's document, the
//! dependencies of a task, the capture behind a source…). A number means
//! something only in the library that wrote it. [`visit_mutation`] walks
//! every site of a change in one fixed order, the one [`crate::idmap::IdMap`]
//! translates through; [`Refs`] keeps, for each site of the change and of
//! its base, the proof of the item the number named where the change was
//! made: its identity (`item_ident`) when the row has one.
//!
//! Level 2 of the shared file and of the team protocol (F3): every change
//! made by this build carries `refs: Some(..)`, written by the producer
//! before the change runs, in the frame it was made in. `None` is a change
//! of an older Ponte (legacy). A receiver checks the refs against the change
//! ([`Refs::check`]) before resolving any number: a mismatch is a damaged
//! change, never a number to guess.
use crate::idmap::IdKind;
use crate::intranet::IntranetMutation;
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What proves the item a site named where the change was made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Proof {
    /// The row's identity (`item_ident`).
    Ident(String),
    /// A site of a kind addressed by identity whose row had none: it can
    /// never be resolved elsewhere.
    Unprovable,
    /// A row without identity, of a kind not yet addressed by identity: the
    /// number holds only in the frame the change was made in ([`FrameTag`]).
    Frame,
}

/// One site: its kind, its number where the change was made, its proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ref {
    pub kind: IdKind,
    pub row: i64,
    pub proof: Proof,
    /// For a link: its two ends and kind where the change was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkProof>,
}

/// A link by what it joins (v2 §2.1, links): the identities of its source
/// and target records and the hash of its kind. Found by them only when
/// exactly one link matches; none or two make a card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkProof {
    pub source: String,
    pub target: String,
    pub kind_hash: Vec<u8>,
}

/// The frame a change was made in: the epoch of the identities and the
/// revision of the copy the library last adopted or published.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameTag {
    #[serde(default)]
    pub epoch: String,
    #[serde(default)]
    pub revision: i64,
}

/// The proofs of a change: one [`Ref`] per site of `mutation` and of its
/// `base`, in the order [`visit_mutation`] visits them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refs {
    pub mutation: Vec<Ref>,
    #[serde(default)]
    pub base: Vec<Ref>,
    /// The record a numbered manual edit is meant for (an `ImportCapture`
    /// of a manual record that already has its number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_target: Option<Ref>,
    #[serde(default)]
    pub frame: FrameTag,
}

/// What a receiver says of a change whose proofs do not match it.
pub const DAMAGED: &str = "A alteração chegou danificada (as provas dos itens que ela cita não conferem com ela). Ela não foi aplicada; refaça-a se ainda for preciso.";

/// What a receiver says of a change one of whose items it cannot prove
/// there (frozen rules 6 and 7): a card, never the number.
pub const UNPROVEN: &str = "A alteração se refere a um item que não pôde ser reconhecido aqui com certeza (ele foi apagado, ou a alteração foi feita sobre um estado que mudou depois). Ela não foi aplicada; confira o item e refaça a alteração, se ainda for o caso.";

/// Where the proofs of a change are resolved: a library, or a copy.
pub(crate) trait IdentFrame {
    /// The row of `kind` that holds `ident` there.
    fn row_of(&self, kind: IdKind, ident: &str) -> Result<Option<i64>>;

    /// The links there from record `source` to record `target` (their
    /// numbers there) whose kind hashes to `kind_hash`, two at most.
    fn links_between(&self, source: i64, target: i64, kind_hash: &[u8]) -> Result<Vec<i64>>;

    /// The one link there that joins the records `proof` names by identity
    /// with its kind; `None` when no link or more than one does.
    fn link_by_ends(&self, proof: &LinkProof) -> Result<Option<i64>> {
        let (Some(source), Some(target)) = (
            self.row_of(IdKind::Document, &proof.source)?,
            self.row_of(IdKind::Document, &proof.target)?,
        ) else {
            return Ok(None);
        };
        let found = self.links_between(source, target, &proof.kind_hash)?;
        Ok(match found[..] {
            [one] => Some(one),
            _ => None,
        })
    }
}

impl IdentFrame for Connection {
    /// The row holding `ident`; else the row holding the identity a late
    /// receipt bound it to (`idents::bind_inserted`). None in a library
    /// left at schema 21: no row there holds an identity.
    fn row_of(&self, kind: IdKind, ident: &str) -> Result<Option<i64>> {
        use rusqlite::OptionalExtension;
        if !crate::idents::has_idents(self)? {
            return Ok(None);
        }
        let holder = |ident: &str| -> Result<Option<i64>> {
            Ok(self
                .query_row(
                    "SELECT row_id FROM item_ident WHERE kind=?1 AND ident=?2",
                    [crate::idents::kind_of(kind), ident],
                    |r| r.get(0),
                )
                .optional()?)
        };
        match holder(ident)? {
            Some(row) => Ok(Some(row)),
            None => match crate::idents::alias_of(self, crate::idents::kind_of(kind), ident)? {
                Some(bound) => holder(&bound),
                None => Ok(None),
            },
        }
    }

    fn links_between(&self, source: i64, target: i64, kind_hash: &[u8]) -> Result<Vec<i64>> {
        Ok(self
            .prepare(
                "SELECT id FROM document_links WHERE source_id=?1 AND target_id=?2 AND kind_hash=?3 LIMIT 2",
            )?
            .query_map(rusqlite::params![source, target, kind_hash], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

/// The main computer's frame seen by a sender that keeps the bindings of
/// late receipts (`idents::learn_bindings`) in `aliases`.
pub(crate) struct Aliased<'a> {
    pub frame: &'a dyn IdentFrame,
    pub aliases: &'a Connection,
}

impl IdentFrame for Aliased<'_> {
    fn row_of(&self, kind: IdKind, ident: &str) -> Result<Option<i64>> {
        if let Some(row) = self.frame.row_of(kind, ident)? {
            return Ok(Some(row));
        }
        match crate::idents::alias_of(self.aliases, crate::idents::kind_of(kind), ident)? {
            Some(bound) => self.frame.row_of(kind, &bound),
            None => Ok(None),
        }
    }

    fn links_between(&self, source: i64, target: i64, kind_hash: &[u8]) -> Result<Vec<i64>> {
        self.frame.links_between(source, target, kind_hash)
    }
}

impl IdentFrame for crate::intranet::CabinetSnapshot {
    fn row_of(&self, kind: IdKind, ident: &str) -> Result<Option<i64>> {
        let kind = crate::idents::kind_of(kind);
        Ok(self
            .idents
            .iter()
            .find(|crate::idents::IdentRow(k, _, i)| k == kind && i == ident)
            .map(|row| row.1))
    }

    fn links_between(&self, source: i64, target: i64, kind_hash: &[u8]) -> Result<Vec<i64>> {
        Ok(self
            .document_links
            .iter()
            .filter(|l| l.source_id == source && l.target_id == target && l.kind_hash == kind_hash)
            .map(|l| l.id)
            .take(2)
            .collect())
    }
}

/// `mutation` in the numbers of `frame`, each site by its proof (frozen
/// rule 6): an identity to the row holding it there, a `Frame` number kept
/// only when `frame_holds` (the very frame it was captured in, or this
/// library's own queue replayed in it), anything else [`UNPROVEN`]. The
/// proofs come back with their rows rewritten to those numbers.
fn resolve_sites(
    mutation: &IntranetMutation,
    refs: &[Ref],
    frame: &dyn IdentFrame,
    frame_holds: bool,
) -> Result<(IntranetMutation, Vec<Ref>), String> {
    let mut out = mutation.clone();
    let mut proofs = refs.iter();
    let mut proved = Vec::with_capacity(refs.len());
    visit_mutation(&mut out, &mut |kind, id| {
        if *id <= 0 {
            return Ok(());
        }
        let site = proofs
            .next()
            .filter(|site| site.kind == kind && site.row == *id)
            .ok_or_else(|| DAMAGED.to_owned())?;
        let row = resolve_ref(site, frame, frame_holds)?;
        *id = row;
        proved.push(Ref {
            row,
            ..site.clone()
        });
        Ok(())
    })?;
    Ok((out, proved))
}

/// The number in `frame` of the item `site` proves (its kind and number
/// already checked against the change). A card, never the number, when it
/// cannot be told.
fn resolve_ref(site: &Ref, frame: &dyn IdentFrame, frame_holds: bool) -> Result<i64, String> {
    let kind = site.kind;
    // A link whose identity is not there (each device derived it with one
    // of its own, or it had none): the one link of the same ends and kind,
    // never one of two (v2 §2.1).
    let by_ends = || match site.link.as_ref().filter(|_| kind == IdKind::Link) {
        Some(proof) => frame
            .link_by_ends(proof)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| UNPROVEN.to_owned()),
        None => Err(UNPROVEN.to_owned()),
    };
    match &site.proof {
        Proof::Ident(ident) => match frame.row_of(kind, ident).map_err(|e| e.to_string())? {
            Some(row) => Ok(row),
            None => by_ends(),
        },
        // A mapped kind's number is this library's own, never the copy's:
        // it holds nowhere else (obligation (c), F3.1a).
        Proof::Frame if frame_holds && !crate::mapper::addressed(crate::idents::kind_of(kind)) => {
            Ok(site.row)
        }
        Proof::Unprovable => by_ends(),
        Proof::Frame => Err(UNPROVEN.into()),
    }
}

/// What a walk does with one site: its kind and its number, which it may
/// change (a translation) or only read.
pub type Visit<'a> = dyn FnMut(IdKind, &mut i64) -> Result<(), String> + 'a;

/// One source of a draft, an analysis or a generation: its numbers, then
/// its name, which follows them (`C{c}`, `C{c}F{f}P{p}[OCR]` or
/// `A{a}H{hash}`, whose attachment is a site of its own). The name it had
/// and the one it got, when it changed.
pub(crate) fn visit_source(
    name: &mut String,
    document_id: &mut i64,
    capture_id: &mut i64,
    file_id: &mut Option<i64>,
    visit: &mut Visit<'_>,
) -> Result<Option<(String, String)>, String> {
    let (capture, file) = (*capture_id, *file_id);
    visit(IdKind::Document, document_id)?;
    visit(IdKind::Capture, capture_id)?;
    if let Some(file) = file_id {
        visit(IdKind::File, file)?;
    }
    let renamed = match crate::idmap::rename_source(name, capture, file, *capture_id, *file_id) {
        Some(renamed) => Some(renamed),
        None => crate::idmap::rename_attachment_source(name, |id| {
            let mut id = id;
            visit(IdKind::Attachment, &mut id)?;
            Ok(id)
        })?,
    };
    Ok(renamed.map(|renamed| (std::mem::replace(name, renamed.clone()), renamed)))
}

fn visit_sources(
    sources: &mut [crate::evidence::Source],
    visit: &mut Visit<'_>,
) -> Result<BTreeMap<String, String>, String> {
    let mut renamed = BTreeMap::new();
    for s in sources {
        if let Some((old, new)) = visit_source(
            &mut s.id,
            &mut s.document_id,
            &mut s.capture_id,
            &mut s.file_id,
            visit,
        )? {
            renamed.insert(old, new);
        }
    }
    Ok(renamed)
}

/// A task as a change carries it: its number, document, parent and
/// dependencies (`with_id`: whether its own number is a site).
fn visit_task(
    task: &mut crate::tasks::Task,
    with_id: bool,
    visit: &mut Visit<'_>,
) -> Result<(), String> {
    if with_id {
        visit(IdKind::Task, &mut task.id)?;
    }
    if let Some(document) = &mut task.document_id {
        visit(IdKind::Document, document)?;
    }
    if let Some(parent) = &mut task.parent_task_id {
        visit(IdKind::Task, parent)?;
    }
    for dependency in &mut task.dependencies {
        visit(IdKind::Task, dependency)?;
    }
    Ok(())
}

/// Visits every id site of `mutation`, in one fixed order, letting `visit`
/// change each number; the names that follow numbers (sources and the
/// citations and facts that use them) follow. Every variant is listed: the
/// compiler keeps the walk exhaustive. Sentinels (0, negative) are visited
/// too; [`sites`] leaves them out.
pub fn visit_mutation(
    mutation: &mut IntranetMutation,
    visit: &mut Visit<'_>,
) -> Result<(), String> {
    use IdKind as K;
    use IntranetMutation as M;
    match mutation {
        M::ImportCapture { .. } | M::ImportBundle { .. } => {}
        M::SaveNote { document_id, .. } | M::SaveWorkContext { document_id, .. } => {
            visit(K::Document, document_id)?;
        }
        M::SaveDraft {
            document_id,
            capture_id,
            materials,
            ..
        } => {
            visit(K::Document, document_id)?;
            visit(K::Capture, capture_id)?;
            visit_sources(materials, visit)?;
        }
        M::SaveAnalysis {
            document_id,
            analysis,
        } => {
            visit(K::Document, document_id)?;
            visit(K::Capture, &mut analysis.capture_id)?;
            let renamed = visit_sources(&mut analysis.materials, visit)?;
            for citation in &mut analysis.citations {
                if let Some(name) = renamed.get(&citation.source_id) {
                    citation.source_id = name.clone();
                }
            }
            for fact in &mut analysis.facts {
                if let Some(name) = renamed.get(&fact.source_id) {
                    fact.source_id = name.clone();
                }
            }
        }
        M::SaveTask { task, .. } => visit_task(task, true, visit)?,
        M::RelinkTask {
            task_id,
            document_id,
            ..
        } => {
            visit(K::Task, task_id)?;
            if let Some(document) = document_id {
                visit(K::Document, document)?;
            }
        }
        M::SavePageText { file_id, .. } => visit(K::File, file_id)?,
        M::AddDocumentLink { source, target, .. } => {
            visit(K::Document, source)?;
            visit(K::Document, target)?;
        }
        M::SetLinkActive { id, .. } => visit(K::Link, id)?,
        M::TrashItem { kind, id } | M::RestoreItem { kind, id } | M::PurgeTrash { kind, id } => {
            if let Some(kind) = IdKind::from_trash(kind) {
                visit(kind, id)?;
            }
        }
        M::DeleteDocument { id } => visit(K::Document, id)?,
        M::SaveSaiopAttendance { attendance, .. } => {
            visit(K::Saiop, &mut attendance.id)?;
            if let Some(batch) = &mut attendance.batch_id {
                visit(K::SaiopBatch, batch)?;
            }
            if let Some(document) = &mut attendance.document_id {
                visit(K::Document, document)?;
            }
            for task in &mut attendance.task_ids {
                visit(K::Task, task)?;
            }
        }
        M::ImportSaiopBatch { items, .. } => {
            for (attendance, _) in items {
                if let Some(document) = &mut attendance.document_id {
                    visit(K::Document, document)?;
                }
                for task in &mut attendance.task_ids {
                    visit(K::Task, task)?;
                }
            }
        }
        M::UndoSaiopBatch { batch_id } => visit(K::SaiopBatch, batch_id)?,
        M::DeleteSaiopAttendance { id } => visit(K::Saiop, id)?,
        M::CreateTaskForSaiop {
            attendance_id,
            task,
            ..
        } => {
            visit(K::Saiop, attendance_id)?;
            if let Some(task) = task {
                visit_task(task, false, visit)?;
            }
        }
        M::AddAttachment {
            owner_kind,
            owner_id,
            ..
        } => {
            if let Some(kind) = IdKind::from_owner(owner_kind) {
                visit(kind, owner_id)?;
            }
        }
        M::UpdateAttachment { id, .. } | M::DeleteAttachment { id } => {
            visit(K::Attachment, id)?;
        }
        M::ExtractAttachmentText { attachment_id }
        | M::ReviewAttachmentText { attachment_id, .. } => {
            visit(K::Attachment, attachment_id)?;
        }
        // Addressed by uid: no site.
        M::SaveTemplate { .. }
        | M::SetTemplateStatus { .. }
        | M::DeleteTemplate { .. }
        | M::SaveInstructionBlock { .. }
        | M::SetBlockStatus { .. }
        | M::DeleteBlock { .. }
        | M::SaveExample { .. }
        | M::DeleteExample { .. }
        | M::DeleteResource { .. }
        | M::SaveResource { .. }
        | M::SaveCategory { .. }
        | M::DeleteCategory { .. }
        | M::SaveProject { .. }
        | M::DeleteProject { .. }
        | M::SaveSetting { .. }
        | M::DeleteResourceUse { .. } => {}
        // Addressed by uid; only the links to numbered items are sites.
        M::SaveResourceUse { item, .. } => {
            if let Some(document) = &mut item.document_id {
                visit(K::Document, document)?;
            }
            if let Some(attendance) = &mut item.saiop_id {
                visit(K::Saiop, attendance)?;
            }
            if let Some(task) = &mut item.task_id {
                visit(K::Task, task)?;
            }
        }
        M::RecordGeneration { record } => {
            visit(K::Document, &mut record.document_id)?;
            visit(K::Capture, &mut record.capture_id)?;
            if let Some(draft) = &mut record.draft_id {
                visit(K::Draft, draft)?;
            }
            for s in &mut record.sources {
                visit_source(
                    &mut s.id,
                    &mut s.document_id,
                    &mut s.capture_id,
                    &mut s.file_id,
                    visit,
                )?;
            }
        }
    }
    Ok(())
}

/// The sites of `mutation` (kind and number), in walk order; sentinels
/// (0, negative: "new", "none") are no site.
pub fn sites(mutation: &IntranetMutation) -> Vec<(IdKind, i64)> {
    let mut walked = mutation.clone();
    let mut out = Vec::new();
    // The visitor never fails: neither does the walk.
    let _ = visit_mutation(&mut walked, &mut |kind, id| {
        if *id > 0 {
            out.push((kind, *id));
        }
        Ok(())
    });
    out
}

/// What the person is told of a change applied by the main computer whose
/// answer names an item with no proof (an older receipt, or one whose
/// recording failed): no number is given back (frozen rule 12).
pub const APPLIED: &str = "A alteração foi aplicada.";

/// The same, when the item the answer names is no longer in the copy.
pub const APPLIED_GONE: &str = "A alteração foi aplicada, mas o item já não existe.";

/// Walks the ids of `reply`, an answer to `mutation`, by the kind the
/// request names (adendo §6 rule 4): an `Id` after `AddAttachment` is an
/// attachment, after a Lixeira change the document it belongs to. `Id` of 0
/// or less, `Unit` and `Counts` are no site.
pub(crate) fn visit_reply(
    mutation: &IntranetMutation,
    reply: &mut crate::intranet::MutationReply,
    visit: &mut Visit<'_>,
) -> Result<(), String> {
    use crate::intranet::MutationReply as R;
    use IntranetMutation as M;
    let mut site = |kind, id: &mut i64| if *id > 0 { visit(kind, id) } else { Ok(()) };
    match reply {
        R::Receipt(receipt) => {
            site(IdKind::Document, &mut receipt.document_id)?;
            site(IdKind::Capture, &mut receipt.capture_id)
        }
        R::SaiopBatch(batch) => site(IdKind::SaiopBatch, &mut batch.id),
        R::Id(id) => match mutation {
            M::SaveDraft { .. } => site(IdKind::Draft, id),
            M::SaveExample { .. } => site(IdKind::Example, id),
            M::SaveTask { .. } | M::RelinkTask { .. } | M::CreateTaskForSaiop { .. } => {
                site(IdKind::Task, id)
            }
            M::SaveSaiopAttendance { .. } => site(IdKind::Saiop, id),
            M::AddAttachment { .. } => site(IdKind::Attachment, id),
            M::TrashItem { .. } | M::RestoreItem { .. } | M::PurgeTrash { .. } => {
                site(IdKind::Document, id)
            }
            _ => Ok(()),
        },
        R::Counts(..) | R::Unit => Ok(()),
    }
}

/// The sites of `reply`, an answer to `mutation` (see [`visit_reply`]).
pub(crate) fn reply_sites(
    mutation: &IntranetMutation,
    reply: &crate::intranet::MutationReply,
) -> Vec<(IdKind, i64)> {
    let mut walked = reply.clone();
    let mut out = Vec::new();
    let _ = visit_reply(mutation, &mut walked, &mut |kind, id| {
        out.push((kind, *id));
        Ok(())
    });
    out
}

/// `reply`, an answer of the main computer to `mutation`, in the numbers of
/// the library on `conn`, which has just adopted the copy of that answer:
/// each id goes to the row holding its proof's identity here (v2 §2.5).
/// An id without a proof gives [`APPLIED`]; one whose item is not here,
/// [`APPLIED_GONE`]. Never the number of the copy.
///
/// A library left at schema 21 for this opening is never mapped: it adopted
/// the copy verbatim, so the numbers of the answer are its own.
pub(crate) fn translate_reply(
    conn: &Connection,
    mutation: &IntranetMutation,
    mut reply: crate::intranet::MutationReply,
    proof: &[Ref],
) -> Result<crate::intranet::MutationReply> {
    if !crate::idents::has_idents(conn)? {
        return Ok(reply);
    }
    visit_reply(mutation, &mut reply, &mut |kind, id| {
        let ident = proof.iter().find_map(|r| match &r.proof {
            Proof::Ident(ident) if r.kind == kind && r.row == *id => Some(ident),
            _ => None,
        });
        let ident = ident.ok_or_else(|| APPLIED.to_owned())?;
        *id = conn
            .row_of(kind, ident)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| APPLIED_GONE.to_owned())?;
        Ok(())
    })
    .map_err(|message| anyhow::anyhow!(message))?;
    Ok(reply)
}

/// Whether `mutation` is the edit of a manual record that already has its
/// number (its target is named by `Refs::manual_target`): an import, or a
/// bundle without files, which the native host queues for the same thing.
fn numbered_manual(mutation: &IntranetMutation) -> bool {
    manual_number(mutation).is_some()
}

/// The number a numbered manual edit carries.
fn manual_number(mutation: &IntranetMutation) -> Option<&str> {
    match mutation {
        IntranetMutation::ImportCapture { capture }
        | IntranetMutation::ImportBundle { capture, .. }
            if capture.is_numbered_manual() =>
        {
            Some(&capture.document_number)
        }
        _ => None,
    }
}

/// What the import of a manual edit says when its record cannot be bound
/// (adendo §9 rule 1): the edit is not written anywhere.
pub const TARGET_GONE: &str = "O registro desta edição não está mais disponível (foi excluído, está na Lixeira ou não pôde ser reconhecido com certeza). A edição não foi gravada em nenhum registro.";

/// The record a numbered manual edit is written to (adendo §9): its proof
/// and, on the library that made the edit, the frame it was proved in.
#[derive(Clone, Debug)]
pub(crate) struct ManualTarget {
    pub target: Ref,
    /// The library that made the edit: a number without identity holds
    /// there while its frame is still this one. `None` for a receiver,
    /// which binds by identity only (frozen rule 6, a mapped kind).
    pub own: Option<FrameTag>,
}

impl ManualTarget {
    /// The edit of record `row`, made now in the library on `conn`.
    pub(crate) fn here(conn: &Connection, row: i64) -> Result<Self> {
        let idents = crate::idents::has_idents(conn)?;
        let target = prove(conn, vec![(IdKind::Document, row)], idents)?
            .pop()
            .expect("one site");
        Ok(Self {
            target,
            own: Some(frame_tag(conn)?),
        })
    }

    /// The record a numbered manual edit names here by its number, now.
    pub(crate) fn named(conn: &Connection, mutation: &IntranetMutation) -> Result<Option<Self>> {
        match manual_number(mutation) {
            Some(number) => crate::storage::doc_by_number(conn, number)?
                .map(|row| Self::here(conn, row))
                .transpose(),
            None => Ok(None),
        }
    }

    /// The target a receiver binds, from the proofs it resolved.
    pub(crate) fn of(refs: &Refs) -> Option<Self> {
        refs.manual_target
            .clone()
            .map(|target| Self { target, own: None })
    }

    /// The record named, in the library on `conn`, read inside the
    /// import's transaction (adendo §9 rule 1); `None`: not proven there.
    pub(crate) fn bind(&self, conn: &Connection) -> Result<Option<i64>> {
        Ok(match (&self.target.proof, &self.own) {
            (Proof::Ident(ident), _) => conn.row_of(IdKind::Document, ident)?,
            (_, Some(tag)) if frame_tag(conn)? == *tag => Some(self.target.row),
            _ => None,
        })
    }
}

/// Whether the library on `conn` makes its changes without proofs: its
/// identities were never shared (provisional), so [`Refs::capture`] gives
/// none and its changes take the legacy route, by number.
pub(crate) fn proofless(conn: &Connection) -> Result<bool> {
    Ok(crate::idents::origin(conn)?.0 == "provisional")
}

/// The frame of the library on `conn`: the epoch of its identities and the
/// revision of the copy it last adopted or published.
pub(crate) fn frame_tag(conn: &Connection) -> Result<FrameTag> {
    let (_, _, epoch) = crate::idents::origin(conn)?;
    Ok(FrameTag {
        epoch: epoch.unwrap_or_default(),
        revision: crate::intranet::synced_revision(conn)?,
    })
}

/// The proofs of `sites` in the library on `conn` (identities read there).
pub(crate) fn prove(
    conn: &Connection,
    sites: Vec<(IdKind, i64)>,
    idents: bool,
) -> Result<Vec<Ref>> {
    sites
        .into_iter()
        .map(|(kind, row)| {
            let ident = if idents {
                crate::idents::ident_of(conn, crate::idents::kind_of(kind), row)?
            } else {
                None
            };
            // A row without identity: of a mapped kind, its number here is
            // not the copy's, so nothing proves it elsewhere (v2 §2.1); of
            // any other kind, it is proved only in its own frame.
            let proof = match ident {
                Some(ident) => Proof::Ident(ident),
                None if crate::mapper::addressed(crate::idents::kind_of(kind)) => Proof::Unprovable,
                None => Proof::Frame,
            };
            let link = match kind {
                IdKind::Link if idents => link_proof(conn, row)?,
                _ => None,
            };
            Ok(Ref {
                kind,
                row,
                proof,
                link,
            })
        })
        .collect()
}

/// The ends of link `row` here by identity, and its kind; `None` when the
/// link, or an end's identity, is not here.
fn link_proof(conn: &Connection, row: i64) -> Result<Option<LinkProof>> {
    use rusqlite::OptionalExtension;
    let Some((source, target, kind_hash)) = conn
        .query_row(
            "SELECT source_id,target_id,kind_hash FROM document_links WHERE id=?1",
            [row],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get(2)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let end = |id| crate::idents::ident_of(conn, "document", id);
    Ok(match (end(source)?, end(target)?) {
        (Some(source), Some(target)) => Some(LinkProof {
            source,
            target,
            kind_hash,
        }),
        _ => None,
    })
}

impl Refs {
    /// These proofs for the same change written without its base.
    pub fn without_base(self) -> Refs {
        Refs {
            base: Vec::new(),
            ..self
        }
    }

    /// The proofs of `mutation` and of its `base`, read in the library on
    /// `conn` BEFORE the change runs there (in the same transaction when
    /// the caller has one), in its frame. `None` for a library whose
    /// identities were never shared (provisional: just updated, or not yet
    /// adopted): no proof of it holds anywhere else, so its changes take
    /// the legacy route, as an unmapped library's queue does (F3.0a2).
    pub(crate) fn capture(
        conn: &Connection,
        mutation: &IntranetMutation,
        base: Option<&IntranetMutation>,
    ) -> Result<Option<Refs>> {
        if proofless(conn)? {
            return Ok(None);
        }
        let idents = crate::idents::has_idents(conn)?;
        Ok(Some(Refs {
            mutation: prove(conn, sites(mutation), idents)?,
            base: match base {
                Some(base) => prove(conn, sites(base), idents)?,
                None => Vec::new(),
            },
            // The record a numbered manual edit names here by the number
            // it carries; an edit the person makes names the record that
            // was opened instead (`Store::import_capture_to`).
            manual_target: match manual_number(mutation) {
                Some(number) => match crate::storage::doc_by_number(conn, number)? {
                    Some(row) => prove(conn, vec![(IdKind::Document, row)], idents)?.pop(),
                    None => None,
                },
                None => None,
            },
            frame: frame_tag(conn)?,
        }))
    }

    /// `mutation` and its `base` in the numbers of `frame`, after
    /// [`Refs::check`]: every site by its proof ([`resolve_sites`]), the
    /// base in the same frame and about the same item (frozen rule 7), and
    /// these proofs with their rows rewritten to those numbers (rule 8).
    /// Any failure is a card, never a number.
    ///
    /// `epoch` is the frame's epoch of identities (`None`: not known). A
    /// purge of the Lixeira whose record is proved by an identity of that
    /// same epoch that no row holds there is done already (purged there, or
    /// gone with its document): it resolves to the item 0, which names
    /// nothing, which every executor takes as done and which deletes
    /// nothing. Proved otherwise, or in another epoch, it resolves as any
    /// change: never to another record (frozen rule 23).
    pub(crate) fn resolve(
        &self,
        mutation: &IntranetMutation,
        base: Option<&IntranetMutation>,
        frame: &dyn IdentFrame,
        frame_holds: bool,
        epoch: Option<&str>,
    ) -> Result<(IntranetMutation, Option<IntranetMutation>, Refs), String> {
        Refs::check(mutation, base, self)?;
        if let Some(done) = self.purge_done(mutation, base, frame, epoch)? {
            return Ok(done);
        }
        let (mutation, proved) = resolve_sites(mutation, &self.mutation, frame, frame_holds)?;
        let (base, base_proved) = match base {
            Some(base) => {
                let (base, proved) = resolve_sites(base, &self.base, frame, frame_holds)?;
                (Some(base), proved)
            }
            None => (None, Vec::new()),
        };
        // The item a change and its base are about: a resource use by its
        // uid (its sites are only its links, which the change may move or
        // drop: review F3.0a2, finding 5), any other by its first site.
        let item = |refs: &[Ref]| refs.first().map(|r| (r.kind, r.row));
        let same = match (&mutation, &base) {
            (
                IntranetMutation::SaveResourceUse { item: now, .. },
                Some(IntranetMutation::SaveResourceUse { item: then, .. }),
            ) => !now.uid.is_empty() && now.uid == then.uid,
            _ => base_proved.is_empty() || item(&base_proved) == item(&proved),
        };
        if !same {
            return Err(UNPROVEN.into());
        }
        // The record of a manual edit in these numbers, for the republished
        // proofs (rule 8); row 0 when its proof is not there: it is bound,
        // or refused, inside the import's transaction (adendo §9).
        let manual_target = self.manual_target.as_ref().map(|target| Ref {
            row: resolve_ref(target, frame, frame_holds).unwrap_or(0),
            ..target.clone()
        });
        Ok((
            mutation,
            base,
            Refs {
                mutation: proved,
                base: base_proved,
                manual_target,
                ..self.clone()
            },
        ))
    }

    /// See [`Refs::resolve`]: `Some` (the purge of item 0, with no site)
    /// when `mutation` purges a record whose one proof is an identity of
    /// `epoch`, the frame's, that no row of `frame` holds.
    fn purge_done(
        &self,
        mutation: &IntranetMutation,
        base: Option<&IntranetMutation>,
        frame: &dyn IdentFrame,
        epoch: Option<&str>,
    ) -> Result<Option<(IntranetMutation, Option<IntranetMutation>, Refs)>, String> {
        let IntranetMutation::PurgeTrash { kind, .. } = mutation else {
            return Ok(None);
        };
        let same_epoch = epoch.is_some_and(|e| !e.is_empty() && e == self.frame.epoch);
        let (true, None, [site]) = (same_epoch, base, self.mutation.as_slice()) else {
            return Ok(None);
        };
        let Proof::Ident(ident) = &site.proof else {
            return Ok(None);
        };
        if frame
            .row_of(site.kind, ident)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Ok(None);
        }
        Ok(Some((
            IntranetMutation::PurgeTrash {
                kind: kind.clone(),
                id: 0,
            },
            None,
            Refs {
                mutation: Vec::new(),
                base: Vec::new(),
                manual_target: None,
                ..self.clone()
            },
        )))
    }

    /// Whether these proofs are the proofs of `mutation` and `base`: one per
    /// site, each of the site's kind and number, none for a missing base,
    /// and a manual target (a document) exactly for a numbered manual edit.
    /// Run by every receiver before any number is resolved; a failure is a
    /// damaged change ([`DAMAGED`]), never a reason to use the numbers.
    pub fn check(
        mutation: &IntranetMutation,
        base: Option<&IntranetMutation>,
        refs: &Refs,
    ) -> Result<(), String> {
        let agree = |sites: Vec<(IdKind, i64)>, refs: &[Ref]| {
            sites.len() == refs.len()
                && sites
                    .iter()
                    .zip(refs)
                    .all(|((kind, row), r)| r.kind == *kind && r.row == *row)
        };
        let base_ok = match base {
            Some(base) => agree(sites(base), &refs.base),
            None => refs.base.is_empty(),
        };
        // Exactly a numbered manual edit names its record (F3.1b1): without
        // it the edit could only go by its number.
        let target_ok = match &refs.manual_target {
            Some(target) => numbered_manual(mutation) && target.kind == IdKind::Document,
            None => !numbered_manual(mutation),
        };
        if agree(sites(mutation), &refs.mutation) && base_ok && target_ok {
            Ok(())
        } else {
            Err(DAMAGED.to_owned())
        }
    }
}
