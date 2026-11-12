//! Translation of item numbers between libraries ("frames").
//!
//! Every library numbers its rows itself. A change made while a computer
//! works on its own (offline client, shared-folder mode) refers to the
//! numbers of that computer; when it reaches the library of the team, an
//! item created here may have received another number there, and the same
//! number may belong to another member's item. Replaying such a change as
//! it is would edit the wrong item.
//!
//! Each queued change records what it created here ([`CreatedId`]). When it
//! is replayed, the same lookup on the receiving library tells the new
//! numbers, and [`IdMap`] rewrites every later change before it is sent.
//! Numbers above the *watermark* (the highest number of each kind when this
//! library last matched the team's) were created here; one that no recorded
//! creation explains is refused instead of guessed, and the change becomes
//! a conflict for the person to redo.
use crate::{
    intranet::{CabinetSnapshot, IntranetMutation, MutationReply},
    storage::Store,
};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The kinds of item a change can refer to by number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IdKind {
    Document,
    Capture,
    File,
    Task,
    Saiop,
    Attachment,
    Draft,
    SaiopBatch,
    Link,
    /// An example of a work template (owner of attachments).
    Example,
}

impl IdKind {
    pub const ALL: [IdKind; 10] = [
        IdKind::Document,
        IdKind::Capture,
        IdKind::File,
        IdKind::Task,
        IdKind::Saiop,
        IdKind::Attachment,
        IdKind::Draft,
        IdKind::SaiopBatch,
        IdKind::Link,
        IdKind::Example,
    ];

    fn table(self) -> &'static str {
        match self {
            IdKind::Document => "documents",
            IdKind::Capture => "captures",
            IdKind::File => "files",
            IdKind::Task => "tasks",
            IdKind::Saiop => "saiop_attendances",
            IdKind::Attachment => "attachments",
            IdKind::Draft => "drafts",
            IdKind::SaiopBatch => "saiop_batches",
            IdKind::Link => "document_links",
            IdKind::Example => "template_examples",
        }
    }

    fn code(self) -> &'static str {
        match self {
            IdKind::Document => "document",
            IdKind::Capture => "capture",
            IdKind::File => "file",
            IdKind::Task => "task",
            IdKind::Saiop => "saiop",
            IdKind::Attachment => "attachment",
            IdKind::Draft => "draft",
            IdKind::SaiopBatch => "saiop_batch",
            IdKind::Link => "link",
            IdKind::Example => "example",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.code() == code)
    }

    /// Kind of a trash entry (`TrashItem`, `RestoreItem`, `PurgeTrash`).
    fn from_trash(kind: &str) -> Option<Self> {
        match kind {
            "document" => Some(IdKind::Document),
            "capture" => Some(IdKind::Capture),
            "task" => Some(IdKind::Task),
            "attachment" => Some(IdKind::Attachment),
            _ => None,
        }
    }

    /// Kind of the owner of an attachment.
    fn from_owner(kind: &str) -> Option<Self> {
        match kind {
            "document" => Some(IdKind::Document),
            "task" => Some(IdKind::Task),
            "saiop" => Some(IdKind::Saiop),
            "example" => Some(IdKind::Example),
            _ => None,
        }
    }
}

/// One item a change created (or found, as an import of a known document
/// does), named by a `slot` that means the same thing in every library:
/// "" for the main item, "file#0" for the first PDF of a capture, and so on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedId {
    pub kind: IdKind,
    pub slot: String,
    pub id: i64,
}

impl CreatedId {
    fn new(kind: IdKind, slot: impl Into<String>, id: i64) -> Self {
        Self {
            kind,
            slot: slot.into(),
            id,
        }
    }
}

/// The lookups needed to name what a change created, answered by a library
/// ([`Store`]) or by its exported copy ([`CabinetSnapshot`]).
pub trait Frame {
    /// PDFs of a capture, in the order they were stored.
    fn files_of_capture(&self, capture: i64) -> Result<Vec<i64>>;
    /// The triage task created with the document: its lowest-numbered task
    /// whose action is "triar". A task moved here from elsewhere (relinked)
    /// with another action is never taken for it.
    fn first_task_of(&self, document: i64) -> Result<Option<i64>>;
    /// The atendimento an import records for a SIGADOC document.
    fn import_attendance_of(&self, document: i64, number: &str) -> Result<Option<i64>>;
    /// Atendimentos of a spreadsheet import, as (row, id).
    fn rows_of_batch(&self, batch: i64) -> Result<Vec<(i64, i64)>>;
    /// Ends and hashed kind of a link between documents.
    fn link_identity(&self, id: i64) -> Result<Option<(i64, i64, Vec<u8>)>>;
    /// The link with these ends and kind, the active one first.
    fn link_by_identity(&self, source: i64, target: i64, kind_hash: &[u8]) -> Result<Option<i64>>;
}

impl Frame for Store {
    fn files_of_capture(&self, capture: i64) -> Result<Vec<i64>> {
        let mut statement = self
            .conn
            .prepare("SELECT id FROM files WHERE capture_id=?1 ORDER BY id")?;
        let ids = statement
            .query_map([capture], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    fn first_task_of(&self, document: i64) -> Result<Option<i64>> {
        self.triage_task_of(document)
    }

    fn import_attendance_of(&self, document: i64, number: &str) -> Result<Option<i64>> {
        let key = crate::saiop::reference_key(&crate::saiop::normalize_reference(number));
        Ok(self
            .conn
            .query_row(
                "SELECT min(id) FROM saiop_attendances WHERE document_id=?1 AND reference_norm=?2",
                params![document, key],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten())
    }

    fn rows_of_batch(&self, batch: i64) -> Result<Vec<(i64, i64)>> {
        let mut statement = self.conn.prepare(
            "SELECT source_row,id FROM saiop_attendances
             WHERE batch_id=?1 AND source_row IS NOT NULL ORDER BY id",
        )?;
        let rows = statement
            .query_map([batch], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn link_identity(&self, id: i64) -> Result<Option<(i64, i64, Vec<u8>)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT source_id,target_id,kind_hash FROM document_links WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }

    fn link_by_identity(&self, source: i64, target: i64, kind_hash: &[u8]) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM document_links WHERE source_id=?1 AND target_id=?2 AND kind_hash=?3
                 ORDER BY active DESC, id DESC LIMIT 1",
                params![source, target, kind_hash],
                |r| r.get(0),
            )
            .optional()?)
    }
}

impl Frame for CabinetSnapshot {
    fn files_of_capture(&self, capture: i64) -> Result<Vec<i64>> {
        let mut ids: Vec<i64> = self
            .files_meta
            .iter()
            .filter(|f| f.capture_id == capture)
            .map(|f| f.id)
            .collect();
        ids.sort_unstable();
        Ok(ids)
    }

    fn first_task_of(&self, document: i64) -> Result<Option<i64>> {
        Ok(self
            .tasks
            .iter()
            .filter(|t| t.document_id == Some(document) && t.action == "triar")
            .map(|t| t.id)
            .min())
    }

    fn import_attendance_of(&self, document: i64, number: &str) -> Result<Option<i64>> {
        let norm = crate::saiop::normalize_reference(number);
        Ok(self
            .saiop_attendances
            .iter()
            .map(|row| &row.attendance)
            .filter(|a| a.document_id == Some(document) && a.reference_norm == norm)
            .map(|a| a.id)
            .min())
    }

    fn rows_of_batch(&self, batch: i64) -> Result<Vec<(i64, i64)>> {
        let mut rows: Vec<(i64, i64)> = self
            .saiop_attendances
            .iter()
            .map(|row| &row.attendance)
            .filter(|a| a.batch_id == Some(batch))
            .filter_map(|a| a.source_row.map(|r| (r as i64, a.id)))
            .collect();
        rows.sort_unstable_by_key(|(_, id)| *id);
        Ok(rows)
    }

    fn link_identity(&self, id: i64) -> Result<Option<(i64, i64, Vec<u8>)>> {
        Ok(self
            .document_links
            .iter()
            .find(|l| l.id == id)
            .map(|l| (l.source_id, l.target_id, l.kind_hash.clone())))
    }

    fn link_by_identity(&self, source: i64, target: i64, kind_hash: &[u8]) -> Result<Option<i64>> {
        Ok(self
            .document_links
            .iter()
            .filter(|l| l.source_id == source && l.target_id == target && l.kind_hash == kind_hash)
            .max_by_key(|l| (l.active, l.id))
            .map(|l| l.id))
    }
}

/// What `mutation`, answered with `reply`, created in `frame` (the library
/// where it has just been applied).
pub fn created_ids(
    frame: &dyn Frame,
    mutation: &IntranetMutation,
    reply: &MutationReply,
) -> Result<Vec<CreatedId>> {
    let mut out = Vec::new();
    match (mutation, reply) {
        (
            IntranetMutation::ImportCapture { capture }
            | IntranetMutation::ImportBundle { capture, .. },
            MutationReply::Receipt(receipt),
        ) => {
            out.push(CreatedId::new(IdKind::Document, "", receipt.document_id));
            out.push(CreatedId::new(IdKind::Capture, "", receipt.capture_id));
            for (index, id) in frame
                .files_of_capture(receipt.capture_id)?
                .into_iter()
                .enumerate()
            {
                out.push(CreatedId::new(IdKind::File, format!("file#{index}"), id));
            }
            if let Some(task) = frame.first_task_of(receipt.document_id)? {
                out.push(CreatedId::new(IdKind::Task, "triage", task));
            }
            if !capture.is_manual() {
                if let Some(att) =
                    frame.import_attendance_of(receipt.document_id, &capture.document_number)?
                {
                    out.push(CreatedId::new(IdKind::Saiop, "import", att));
                }
            }
        }
        (IntranetMutation::SaveDraft { .. }, MutationReply::Id(id)) => {
            out.push(CreatedId::new(IdKind::Draft, "", *id));
        }
        (IntranetMutation::SaveTask { task, .. }, MutationReply::Id(id)) if task.id <= 0 => {
            out.push(CreatedId::new(IdKind::Task, "", *id));
        }
        (IntranetMutation::SaveSaiopAttendance { attendance, .. }, MutationReply::Id(id))
            if attendance.id <= 0 =>
        {
            out.push(CreatedId::new(IdKind::Saiop, "", *id));
        }
        // An example is addressed by uid, but its row number owns
        // attachments, which refer to it by number.
        (IntranetMutation::SaveExample { example, .. }, MutationReply::Id(id))
            if example.id <= 0 =>
        {
            out.push(CreatedId::new(IdKind::Example, "", *id));
        }
        // Recursos and their uses are addressed by uid: nothing is created
        // with a number of its own.
        (
            IntranetMutation::SaveResource { .. } | IntranetMutation::SaveResourceUse { .. },
            MutationReply::Unit,
        ) => {}
        (IntranetMutation::CreateTaskForSaiop { task: Some(_), .. }, MutationReply::Id(id))
            if *id > 0 =>
        {
            out.push(CreatedId::new(IdKind::Task, "", *id));
        }
        (IntranetMutation::AddAttachment { .. }, MutationReply::Id(id)) => {
            out.push(CreatedId::new(IdKind::Attachment, "", *id));
        }
        (IntranetMutation::ImportSaiopBatch { .. }, MutationReply::SaiopBatch(batch)) => {
            out.push(CreatedId::new(IdKind::SaiopBatch, "", batch.id));
            for (row, id) in frame.rows_of_batch(batch.id)? {
                out.push(CreatedId::new(IdKind::Saiop, format!("row#{row}"), id));
            }
        }
        (
            IntranetMutation::AddDocumentLink {
                source,
                target,
                kind,
                ..
            },
            _,
        ) => {
            let hash = Sha256::digest(kind.as_bytes()).to_vec();
            if let Some(id) = frame.link_by_identity(*source, *target, &hash)? {
                out.push(CreatedId::new(IdKind::Link, "", id));
            }
        }
        _ => {}
    }
    Ok(out)
}

/// The name of a source after its capture (and file) got new numbers, when
/// it follows the pattern built in `evidence`: `C{capture}` and
/// `C{capture}F{file}P{page}` (optionally followed by `OCR`).
fn rename_source(
    name: &str,
    capture: i64,
    file: Option<i64>,
    new_capture: i64,
    new_file: Option<i64>,
) -> Option<String> {
    if (capture, file) == (new_capture, new_file) {
        return None;
    }
    let rest = name.strip_prefix(&format!("C{capture}"))?;
    match (file, new_file) {
        (None, None) if rest.is_empty() => Some(format!("C{new_capture}")),
        (Some(file), Some(new_file)) => {
            let page = rest.strip_prefix(&format!("F{file}P"))?;
            let digits = page.trim_end_matches("OCR");
            (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                .then(|| format!("C{new_capture}F{new_file}P{page}"))
        }
        _ => None,
    }
}

/// Why a change cannot be translated.
pub const UNRESOLVED: &str = "A alteração se refere a um item criado sem conexão que não chegou ao acervo da equipe (ele foi recusado ou não pôde ser identificado). Refaça a alteração manualmente.";

/// Numbers of this library translated into another one.
#[derive(Clone, Debug, Default)]
pub struct IdMap {
    /// Local number → number there; `None` when the creation was refused.
    entries: BTreeMap<(IdKind, i64), Option<i64>>,
    /// Highest number of each kind when this library last matched the
    /// team's. Empty for changes from another computer (its numbers are
    /// translated only through `entries`).
    watermark: BTreeMap<IdKind, i64>,
}

impl IdMap {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records what a replayed change created: `here` as recorded where it
    /// was made, `there` as found where it was applied (`None` when it was
    /// refused, so that changes depending on it are refused too).
    ///
    /// A number that already has a translation keeps it. A number missing on the other side means
    /// "refused" when it was created here (above the watermark) or comes
    /// from another computer's copy (no watermark); otherwise it is an item
    /// that already existed (an import of a known document) and keeps its
    /// number.
    pub fn learn(
        &mut self,
        here: &[CreatedId],
        there: Option<&[CreatedId]>,
    ) -> Vec<(IdKind, i64, Option<i64>)> {
        let mut learned = Vec::new();
        for created in here {
            let key = (created.kind, created.id);
            let target = there.and_then(|list| {
                list.iter()
                    .find(|c| c.kind == created.kind && c.slot == created.slot)
                    .map(|c| c.id)
            });
            // The first translation of a number stays: later changes only
            // find the item again (an import of the same document), and a
            // lookup there may name another member's item. A number that
            // existed before this computer went offline (at or below the
            // watermark) is shared history with the same number everywhere:
            // it is never translated, whatever a lookup on the main computer
            // finds in that slot now (the item may have been removed there).
            let shared_history =
                !self.watermark.is_empty() && !self.is_new(created.kind, created.id);
            if self.entries.contains_key(&key) || shared_history {
                continue;
            }
            self.entries.insert(key, target);
            learned.push((created.kind, created.id, target));
        }
        learned
    }

    /// Adds translations from `other` for numbers this map does not know.
    pub fn absorb(&mut self, other: IdMap) {
        for (key, value) in other.entries {
            self.entries.entry(key).or_insert(value);
        }
    }

    /// A map that refuses the given numbers (used to tell whether a change
    /// refers to items created by later queued changes).
    pub fn refusing<'a>(created: impl IntoIterator<Item = &'a CreatedId>) -> Self {
        let mut map = Self::default();
        for c in created {
            map.entries.insert((c.kind, c.id), None);
        }
        map
    }

    fn is_new(&self, kind: IdKind, id: i64) -> bool {
        self.watermark.get(&kind).is_some_and(|mark| id > *mark)
    }

    fn translate(&self, kind: IdKind, id: i64) -> Result<i64, String> {
        if id <= 0 {
            return Ok(id);
        }
        match self.entries.get(&(kind, id)) {
            Some(Some(target)) => Ok(*target),
            Some(None) => Err(UNRESOLVED.into()),
            None if self.is_new(kind, id) => Err(UNRESOLVED.into()),
            None => Ok(id),
        }
    }

    fn translate_link(
        &self,
        id: i64,
        source: &dyn Frame,
        target: Option<&dyn Frame>,
    ) -> Result<i64, String> {
        if let Some(found) = self.entries.get(&(IdKind::Link, id)) {
            return found.ok_or_else(|| UNRESOLVED.to_owned());
        }
        let new = self.is_new(IdKind::Link, id);
        if !new && !self.watermark.is_empty() {
            return Ok(id);
        }
        // Links made as a side effect (juntadas) are found by their ends.
        let identity = source.link_identity(id).map_err(|e| e.to_string())?;
        let found = match (identity, target) {
            (Some((from, to, hash)), Some(target)) => {
                let from = self.translate(IdKind::Document, from)?;
                let to = self.translate(IdKind::Document, to)?;
                target
                    .link_by_identity(from, to, &hash)
                    .map_err(|e| e.to_string())?
            }
            _ => None,
        };
        match found {
            Some(target_id) => Ok(target_id),
            None if new => Err(UNRESOLVED.into()),
            None => Ok(id),
        }
    }

    /// `mutation` with every number translated. `source` is the library the
    /// change was made in and `target` the one it goes to (only links need
    /// them). Fails with [`UNRESOLVED`] when a number cannot be translated.
    pub fn remap(
        &self,
        mutation: &IntranetMutation,
        source: &dyn Frame,
        target: Option<&dyn Frame>,
    ) -> Result<IntranetMutation, String> {
        use IdKind as K;
        use IntranetMutation as M;
        let t = |kind: IdKind, id: i64| self.translate(kind, id);
        // Sources are named after their numbers ("C12", "C12F3P1",
        // "C12F3P1OCR"): the name follows, and the citations with it.
        let remap_sources = |sources: &mut Vec<crate::evidence::Source>| -> Result<BTreeMap<String, String>, String> {
            let mut renamed = BTreeMap::new();
            for source in sources {
                let (capture, file) = (source.capture_id, source.file_id);
                source.document_id = t(K::Document, source.document_id)?;
                source.capture_id = t(K::Capture, source.capture_id)?;
                if let Some(file) = source.file_id {
                    source.file_id = Some(t(K::File, file)?);
                }
                if let Some(name) =
                    rename_source(&source.id, capture, file, source.capture_id, source.file_id)
                {
                    renamed.insert(std::mem::replace(&mut source.id, name.clone()), name);
                }
            }
            Ok(renamed)
        };
        let mut out = mutation.clone();
        match &mut out {
            M::ImportCapture { .. } | M::ImportBundle { .. } => {}
            M::SaveNote { document_id, .. } | M::SaveWorkContext { document_id, .. } => {
                *document_id = t(K::Document, *document_id)?;
            }
            M::SaveDraft {
                document_id,
                capture_id,
                materials,
                ..
            } => {
                *document_id = t(K::Document, *document_id)?;
                *capture_id = t(K::Capture, *capture_id)?;
                remap_sources(materials)?;
            }
            M::SaveAnalysis {
                document_id,
                analysis,
            } => {
                *document_id = t(K::Document, *document_id)?;
                analysis.capture_id = t(K::Capture, analysis.capture_id)?;
                let renamed = remap_sources(&mut analysis.materials)?;
                for citation in &mut analysis.citations {
                    if let Some(name) = renamed.get(&citation.source_id) {
                        citation.source_id = name.clone();
                    }
                }
            }
            M::SaveTask { task, .. } => {
                task.id = t(K::Task, task.id)?;
                if let Some(document) = task.document_id {
                    task.document_id = Some(t(K::Document, document)?);
                }
                for dependency in &mut task.dependencies {
                    *dependency = t(K::Task, *dependency)?;
                }
            }
            M::RelinkTask {
                task_id,
                document_id,
                ..
            } => {
                *task_id = t(K::Task, *task_id)?;
                if let Some(document) = document_id {
                    *document_id = Some(t(K::Document, *document)?);
                }
            }
            M::SavePageText { file_id, .. } => *file_id = t(K::File, *file_id)?,
            M::AddDocumentLink { source, target, .. } => {
                *source = t(K::Document, *source)?;
                *target = t(K::Document, *target)?;
            }
            M::SetLinkActive { id, .. } => *id = self.translate_link(*id, source, target)?,
            M::TrashItem { kind, id }
            | M::RestoreItem { kind, id }
            | M::PurgeTrash { kind, id } => {
                if let Some(kind) = IdKind::from_trash(kind) {
                    *id = t(kind, *id)?;
                }
            }
            M::DeleteDocument { id } => *id = t(K::Document, *id)?,
            M::SaveSaiopAttendance { attendance, .. } => {
                attendance.id = t(K::Saiop, attendance.id)?;
                if let Some(batch) = attendance.batch_id {
                    attendance.batch_id = Some(t(K::SaiopBatch, batch)?);
                }
                if let Some(document) = attendance.document_id {
                    attendance.document_id = Some(t(K::Document, document)?);
                }
                for task in &mut attendance.task_ids {
                    *task = t(K::Task, *task)?;
                }
            }
            M::ImportSaiopBatch { items, .. } => {
                for (attendance, _) in items {
                    if let Some(document) = attendance.document_id {
                        attendance.document_id = Some(t(K::Document, document)?);
                    }
                    for task in &mut attendance.task_ids {
                        *task = t(K::Task, *task)?;
                    }
                }
            }
            M::UndoSaiopBatch { batch_id } => *batch_id = t(K::SaiopBatch, *batch_id)?,
            M::DeleteSaiopAttendance { id } => *id = t(K::Saiop, *id)?,
            M::CreateTaskForSaiop {
                attendance_id,
                task,
                ..
            } => {
                *attendance_id = t(K::Saiop, *attendance_id)?;
                if let Some(task) = task {
                    if let Some(document) = task.document_id {
                        task.document_id = Some(t(K::Document, document)?);
                    }
                    for dependency in &mut task.dependencies {
                        *dependency = t(K::Task, *dependency)?;
                    }
                }
            }
            M::AddAttachment {
                owner_kind,
                owner_id,
                ..
            } => {
                if let Some(kind) = IdKind::from_owner(owner_kind) {
                    *owner_id = t(kind, *owner_id)?;
                }
            }
            M::UpdateAttachment { id, .. } | M::DeleteAttachment { id } => {
                *id = t(K::Attachment, *id)?;
            }
            M::ExtractAttachmentText { attachment_id }
            | M::ReviewAttachmentText { attachment_id, .. } => {
                *attachment_id = t(K::Attachment, *attachment_id)?;
            }
            // Addressed by uid: nothing to translate.
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
            | M::DeleteResourceUse { .. } => {}
            // Addressed by uid; only the links to numbered items change.
            M::SaveResourceUse { item, .. } => {
                if let Some(document) = item.document_id {
                    item.document_id = Some(t(K::Document, document)?);
                }
                if let Some(attendance) = item.saiop_id {
                    item.saiop_id = Some(t(K::Saiop, attendance)?);
                }
                if let Some(task) = item.task_id {
                    item.task_id = Some(t(K::Task, task)?);
                }
            }
            M::RecordGeneration { record } => {
                record.document_id = t(K::Document, record.document_id)?;
                record.capture_id = t(K::Capture, record.capture_id)?;
                if let Some(draft) = record.draft_id {
                    record.draft_id = Some(t(K::Draft, draft)?);
                }
                for source in &mut record.sources {
                    let (capture, file) = (source.capture_id, source.file_id);
                    source.document_id = t(K::Document, source.document_id)?;
                    source.capture_id = t(K::Capture, source.capture_id)?;
                    if let Some(file) = source.file_id {
                        source.file_id = Some(t(K::File, file)?);
                    }
                    if let Some(name) =
                        rename_source(&source.id, capture, file, source.capture_id, source.file_id)
                    {
                        source.id = name;
                    }
                }
            }
        }
        Ok(out)
    }

    /// The translation that a copy's journal implies: for each change in it
    /// that created something, its numbers in that copy against the numbers
    /// the same change got in the copy being built (`resolved`, by change
    /// `uid`; `None` when it was refused there). A change the copy being
    /// built never received (`applied`) left nothing there: its items are
    /// refused. One it received whose numbers are unknown (journal already
    /// compacted, or written by an older Ponte) is shared history: same
    /// numbers.
    pub fn from_journal<'a>(
        journal: impl IntoIterator<Item = (&'a str, &'a [CreatedId])>,
        resolved: &BTreeMap<String, Option<Vec<CreatedId>>>,
        applied: &std::collections::BTreeSet<String>,
    ) -> Self {
        let mut map = Self::default();
        for (uid, created) in journal {
            if created.is_empty() {
                continue;
            }
            match resolved.get(uid) {
                Some(Some(there)) if !there.is_empty() => {
                    map.learn(created, Some(there));
                }
                Some(Some(_)) => {}
                Some(None) => {
                    map.learn(created, None);
                }
                None if applied.contains(uid) => {}
                None => {
                    map.learn(created, None);
                }
            }
        }
        map
    }
}

/// Tables that keep the translation of a computer whose queue is being sent
/// (it survives a connection that drops halfway) and the watermark.
pub(crate) fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS offline_id_map(
            kind TEXT NOT NULL,
            local_id INTEGER NOT NULL,
            target_id INTEGER,
            PRIMARY KEY(kind,local_id)
        );
        CREATE TABLE IF NOT EXISTS offline_watermark(
            kind TEXT PRIMARY KEY,
            max_id INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS offline_frame_journal(
            uid TEXT PRIMARY KEY,
            created BLOB NOT NULL
        );",
    )?;
    Ok(())
}

/// Marks the current library as matching the team's: forgets the
/// translation and records the highest number of each kind. Called when a
/// copy of the team's library replaces this one, and when a computer with
/// nothing waiting to be sent starts or changes its network setting.
///
/// `replaced`: the library was just replaced by a copy, so the journal kept
/// from the previous copy no longer describes it. Otherwise (a settings
/// change, say) the library still matches that copy and the journal stays.
pub(crate) fn reset_frame(conn: &Connection, replaced: bool) -> Result<()> {
    conn.execute("DELETE FROM offline_id_map", [])?;
    if replaced {
        conn.execute("DELETE FROM offline_frame_journal", [])?;
    }
    for kind in IdKind::ALL {
        let max: i64 = conn.query_row(
            &format!("SELECT coalesce(max(id),0) FROM {}", kind.table()),
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO offline_watermark(kind,max_id) VALUES(?1,?2)
             ON CONFLICT(kind) DO UPDATE SET max_id=excluded.max_id",
            params![kind.code(), max],
        )?;
    }
    Ok(())
}

/// Like [`reset_frame`], only when nothing waits to be sent nor to be
/// translated (otherwise the local numbers still matter).
pub(crate) fn reset_frame_if_settled(conn: &Connection) -> Result<()> {
    let pending: i64 = conn.query_row(
        "SELECT (SELECT count(*) FROM offline_outbox) + (SELECT count(*) FROM offline_id_map)",
        [],
        |r| r.get(0),
    )?;
    if pending == 0 {
        reset_frame(conn, false)?;
    }
    Ok(())
}

/// First opening after this feature arrived: a library with nothing waiting
/// takes its current numbers as the reference.
pub(crate) fn init_watermark(conn: &Connection) -> Result<()> {
    let marked: i64 = conn.query_row("SELECT count(*) FROM offline_watermark", [], |r| r.get(0))?;
    if marked == 0 {
        reset_frame_if_settled(conn)?;
        return Ok(());
    }
    // A kind added by an update has no mark yet. Nothing of that kind can be
    // waiting in the queue (the old version did not know it), so its current
    // highest number is the right mark.
    let present: Vec<String> = conn
        .prepare("SELECT kind FROM offline_watermark")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for kind in IdKind::ALL {
        if present.iter().any(|k| k == kind.code()) {
            continue;
        }
        let max: i64 = conn.query_row(
            &format!("SELECT coalesce(max(id),0) FROM {}", kind.table()),
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO offline_watermark(kind,max_id) VALUES(?1,?2)",
            params![kind.code(), max],
        )?;
    }
    Ok(())
}

/// The stored translation and watermark of this library.
pub(crate) fn load(conn: &Connection) -> Result<IdMap> {
    let mut map = IdMap::default();
    let mut statement = conn.prepare("SELECT kind,local_id,target_id FROM offline_id_map")?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    for row in rows {
        let (kind, local, target) = row?;
        if let Some(kind) = IdKind::from_code(&kind) {
            map.entries.insert((kind, local), target);
        }
    }
    let mut statement = conn.prepare("SELECT kind,max_id FROM offline_watermark")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (kind, max) = row?;
        if let Some(kind) = IdKind::from_code(&kind) {
            map.watermark.insert(kind, max);
        }
    }
    Ok(map)
}

/// Shared-folder mode: keeps what the changes of the copy this library now
/// matches created, with its numbers. If the next exchange builds on a copy
/// that diverged from it, the queue is translated through it.
pub(crate) fn save_frame_journal(
    conn: &Connection,
    journal: &[crate::sharedfolder::JournalEntry],
) -> Result<()> {
    conn.execute("DELETE FROM offline_frame_journal", [])?;
    for entry in journal.iter().filter(|e| !e.created.is_empty()) {
        conn.execute(
            "INSERT OR REPLACE INTO offline_frame_journal(uid,created) VALUES(?1,?2)",
            params![
                entry.uid,
                crate::protection::seal(&serde_json::to_vec(&entry.created)?)?
            ],
        )?;
    }
    Ok(())
}

/// What [`save_frame_journal`] kept, as (change uid, created items).
pub(crate) fn load_frame_journal(conn: &Connection) -> Result<Vec<(String, Vec<CreatedId>)>> {
    let mut statement = conn.prepare("SELECT uid,created FROM offline_frame_journal")?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (uid, blob) in rows {
        let created = serde_json::from_slice(&crate::protection::unseal(&blob)?)?;
        out.push((uid, created));
    }
    Ok(out)
}

/// Stores translations learned while sending the queue.
pub(crate) fn save(conn: &Connection, learned: &[(IdKind, i64, Option<i64>)]) -> Result<()> {
    for (kind, local, target) in learned {
        conn.execute(
            "INSERT INTO offline_id_map(kind,local_id,target_id) VALUES(?1,?2,?3)
             ON CONFLICT(kind,local_id) DO UPDATE SET target_id=excluded.target_id",
            params![kind.code(), local, target],
        )?;
    }
    Ok(())
}

/// Whether a translation is still pending (a queue sent halfway).
pub(crate) fn has_pending(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT count(*) FROM offline_id_map", [], |r| r.get(0))?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(watermark: &[(IdKind, i64)]) -> IdMap {
        IdMap {
            entries: BTreeMap::new(),
            watermark: watermark.iter().copied().collect(),
        }
    }

    fn open_store(dir: &tempfile::TempDir) -> Store {
        Store::open(&dir.path().join("acervo.sqlite3")).unwrap()
    }

    #[test]
    fn the_triage_task_is_the_one_the_import_created() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(&dir);
        // Plain import.
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let triage = store.first_task_of(document).unwrap().expect("triage");
        let task = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == triage)
            .unwrap();
        assert_eq!(task.action, "triar");
        // No tasks at all: none.
        assert_eq!(store.first_task_of(document + 99).unwrap(), None);
    }

    #[test]
    fn an_older_relinked_task_is_not_taken_for_the_triage_task() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(&dir);
        // A task without document, older than the one the import creates.
        let mut free = crate::tasks::Task::blank(None);
        free.title = "Tarefa avulsa".to_owned();
        let free_id = store.save_task(&free).unwrap();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        store.relink_task(free_id, Some(document), None).unwrap();
        let triage = store.first_task_of(document).unwrap().expect("triage");
        assert_ne!(triage, free_id);
        assert!(triage > free_id);
    }

    #[test]
    fn a_concluded_or_removed_triage_task_is_still_found_and_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(&dir);
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let triage = store.first_task_of(document).unwrap().unwrap();
        store.complete_task(triage).unwrap();
        assert_eq!(store.first_task_of(document).unwrap(), Some(triage));
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id: triage,
            })
            .unwrap();
        assert_eq!(store.first_task_of(document).unwrap(), Some(triage));
        // A task of another action alone is not a triage task.
        let mut other = crate::tasks::Task::new(document);
        other.action = "examinar".to_owned();
        let other_id = store.save_task(&other).unwrap();
        assert_ne!(store.first_task_of(document).unwrap(), Some(other_id));
        assert_eq!(store.first_task_of(document).unwrap(), Some(triage));
    }

    #[test]
    fn numbers_from_before_going_offline_are_never_translated() {
        let mut ids = map(&[(IdKind::Document, 100), (IdKind::Task, 100)]);
        // A re-import of document 10 reports its triage task 40 (shared
        // history); there the slot now holds another task, 77.
        let learned = ids.learn(
            &[CreatedId::new(IdKind::Task, "triage", 40)],
            Some(&[CreatedId::new(IdKind::Task, "triage", 77)]),
        );
        assert!(learned.is_empty());
        assert!(!ids.entries.contains_key(&(IdKind::Task, 40)));
        // A task created here (above the watermark) is translated.
        let learned = ids.learn(
            &[CreatedId::new(IdKind::Task, "triage", 105)],
            Some(&[CreatedId::new(IdKind::Task, "triage", 130)]),
        );
        assert_eq!(learned, vec![(IdKind::Task, 105, Some(130))]);
    }

    #[test]
    fn a_refusal_only_affects_items_created_here() {
        let mut ids = map(&[(IdKind::Document, 100), (IdKind::Task, 100)]);
        // Import #1 created document 105, which became 130 there.
        ids.learn(
            &[CreatedId::new(IdKind::Document, "", 105)],
            Some(&[CreatedId::new(IdKind::Document, "", 130)]),
        );
        // Import #2 of the same document was refused there; it also names
        // an older document (50) and its triage task (40).
        ids.learn(
            &[
                CreatedId::new(IdKind::Document, "", 105),
                CreatedId::new(IdKind::Document, "", 50),
                CreatedId::new(IdKind::Task, "triage", 40),
            ],
            None,
        );
        assert_eq!(ids.translate(IdKind::Document, 105), Ok(130));
        assert_eq!(ids.translate(IdKind::Document, 50), Ok(50));
        assert_eq!(ids.translate(IdKind::Task, 40), Ok(40));
        // A new item of a refused creation is refused, and so is a new
        // number nobody explains.
        ids.learn(&[CreatedId::new(IdKind::Task, "", 101)], None);
        assert!(ids.translate(IdKind::Task, 101).is_err());
        assert!(ids.translate(IdKind::Task, 102).is_err());
    }

    #[test]
    fn numbers_of_another_copy_missing_here_are_refused() {
        let journal = [(
            "u1".to_owned(),
            vec![
                CreatedId::new(IdKind::Document, "", 7),
                CreatedId::new(IdKind::Saiop, "import", 3),
            ],
        )];
        let mut resolved = BTreeMap::new();
        resolved.insert(
            "u1".to_owned(),
            Some(vec![CreatedId::new(IdKind::Document, "", 9)]),
        );
        let applied = Default::default();
        let ids = IdMap::from_journal(
            journal.iter().map(|(u, c)| (u.as_str(), c.as_slice())),
            &resolved,
            &applied,
        );
        assert_eq!(ids.translate(IdKind::Document, 7), Ok(9));
        assert!(ids.translate(IdKind::Saiop, 3).is_err());

        // A change the copy being built never received left nothing there.
        let ids = IdMap::from_journal(
            journal.iter().map(|(u, c)| (u.as_str(), c.as_slice())),
            &BTreeMap::new(),
            &applied,
        );
        assert!(ids.translate(IdKind::Document, 7).is_err());
        // One it received, with unknown numbers, is shared history.
        let applied = ["u1".to_owned()].into_iter().collect();
        let ids = IdMap::from_journal(
            journal.iter().map(|(u, c)| (u.as_str(), c.as_slice())),
            &BTreeMap::new(),
            &applied,
        );
        assert_eq!(ids.translate(IdKind::Document, 7), Ok(7));
    }

    #[test]
    fn source_names_follow_their_numbers() {
        assert_eq!(
            rename_source("C12", 12, None, 30, None).as_deref(),
            Some("C30")
        );
        assert_eq!(
            rename_source("C12F3P4", 12, Some(3), 30, Some(8)).as_deref(),
            Some("C30F8P4")
        );
        assert_eq!(
            rename_source("C12F3P4OCR", 12, Some(3), 30, Some(8)).as_deref(),
            Some("C30F8P4OCR")
        );
        assert_eq!(rename_source("C12", 12, None, 12, None), None);
        assert_eq!(rename_source("C123", 12, None, 30, None), None);
        assert_eq!(rename_source("outro", 12, Some(3), 30, Some(8)), None);
    }
}
