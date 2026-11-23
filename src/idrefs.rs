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

/// What a walk does with one site: its kind and its number, which it may
/// change (a translation) or only read.
pub type Visit<'a> = dyn FnMut(IdKind, &mut i64) -> Result<(), String> + 'a;

/// One source of a draft, an analysis or a generation: its numbers, then
/// its name, which follows them (`C{c}`, `C{c}F{f}P{p}[OCR]` or
/// `A{a}H{hash}`, whose attachment is a site of its own). The name it had
/// and the one it got, when it changed.
fn visit_source(
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
