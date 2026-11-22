//! Manual records change number when a creation made offline (or in a
//! shared folder) is replayed onto the shared copy: the number was "the
//! highest of this library plus one", and the shared copy may already have a
//! record with it. This module tells the person (a local notice, never
//! synchronized), keeps the old number findable on this computer, says
//! which numbers are still provisional, and keeps a queued edit of such a
//! record from landing on the record that took the number. It changes nothing
//! about how numbers are given.
//!
//! The identity of a record across the change is its request ids (one per
//! version): every snapshot row of `requests` carries them, so the same record
//! is found on both sides of an adoption without any new data in the shared
//! copy. A record with no request on either side has no identity and gives no
//! notice (the title is never used to guess).
use crate::intranet::{CabinetSnapshot, IntranetMutation};
use crate::model::{Capture, MANUAL_ORIGIN, MANUAL_PENDING};
use crate::protection::{seal, unseal};
use crate::storage::Store;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// How many dismissed notices this library remembers (they keep the old
/// number findable). Pending ones are never dropped.
pub(crate) const KEEP_DISMISSED: i64 = 300;

/// "Record X went from number A to number B when this computer adopted the
/// shared copy." Local to this computer: it is not part of the snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenumberNotice {
    /// Row of the local table (not stored in the payload).
    #[serde(skip)]
    pub id: i64,
    /// Identity of the record: request ids of its versions (the first is the
    /// creation's). The record is the one all of them lead to.
    pub uids: Vec<String>,
    pub title: String,
    pub from: String,
    pub to: String,
    /// When this computer saw the change (RFC 3339).
    pub at: String,
    #[serde(default)]
    pub dismissed: bool,
}

impl RenumberNotice {
    /// The sentence shown to the person.
    pub fn sentence(&self) -> String {
        format!(
            "O registro “{}” passou de {} para {} ao sincronizar.",
            short_title(&self.title),
            self.from,
            self.to
        )
    }
}

fn short_title(title: &str) -> String {
    let title = title.trim();
    if title.chars().count() > 70 {
        format!("{}…", title.chars().take(70).collect::<String>().trim_end())
    } else {
        title.to_owned()
    }
}

/// The one sentence for several notices (the sync summary, the card in Hoje).
pub fn summarize(notices: &[RenumberNotice]) -> Option<String> {
    match notices {
        [] => None,
        [one] => Some(one.sentence()),
        many => Some(format!(
            "{} registros mudaram de número ao sincronizar.",
            many.len()
        )),
    }
}

fn same_number(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().collect::<String>().to_uppercase();
    norm(a) == norm(b)
}

fn read_capture(blob: &[u8]) -> Option<Capture> {
    serde_json::from_slice(&unseal(blob).ok()?).ok()
}

/// A request id as the libraries store it.
fn normalized_request(request: &str) -> Option<String> {
    Some(
        uuid::Uuid::parse_str(request)
            .ok()?
            .hyphenated()
            .to_string(),
    )
}

// ---------------------------------------------------------------------------
// Where an edit of a manual record goes

/// What the replay of a queued manual edit needs to know of a library: the
/// one the edit was written in, and the one it is being applied to.
pub(crate) trait Records {
    /// The number a document has (its latest version tells it).
    fn number_of_doc(&self, doc: i64) -> Result<Option<String>, String>;
    /// The document that holds a manual number (trashed ones included).
    fn doc_by_number(&self, number: &str) -> Result<Option<i64>, String>;
    /// The document a request id led to.
    fn doc_of_request(&self, request: &str) -> Result<Option<i64>, String>;
    /// Every request id of a document's versions.
    fn requests_of_doc(&self, doc: i64) -> Result<Vec<String>, String>;
}

impl Records for Store {
    fn number_of_doc(&self, doc: i64) -> Result<Option<String>, String> {
        self.document_number_by_id(doc).map_err(|e| e.to_string())
    }
    fn doc_by_number(&self, number: &str) -> Result<Option<i64>, String> {
        self.document_id_by_number(number)
            .map_err(|e| e.to_string())
    }
    fn doc_of_request(&self, request: &str) -> Result<Option<i64>, String> {
        let Some(request) = normalized_request(request) else {
            return Ok(None);
        };
        self.conn
            .query_row(
                "SELECT document_id FROM requests WHERE request_id=?1",
                [request],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    fn requests_of_doc(&self, doc: i64) -> Result<Vec<String>, String> {
        let mut statement = self
            .conn
            .prepare("SELECT request_id FROM requests WHERE document_id=?1")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([doc], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())
    }
}

impl Records for CabinetSnapshot {
    fn number_of_doc(&self, doc: i64) -> Result<Option<String>, String> {
        let Some(row) = self.documents.iter().find(|d| d.id == doc) else {
            return Ok(None);
        };
        Ok(row.latest_capture_id.and_then(|latest| {
            self.captures
                .iter()
                .find(|c| c.id == latest)
                .map(|c| c.capture.document_number.clone())
        }))
    }
    fn doc_by_number(&self, number: &str) -> Result<Option<i64>, String> {
        let hash = crate::storage::identity_hash_for_number(number);
        Ok(self
            .documents
            .iter()
            .find(|d| d.identity_hash == hash)
            .map(|d| d.id))
    }
    fn doc_of_request(&self, request: &str) -> Result<Option<i64>, String> {
        let Some(request) = normalized_request(request) else {
            return Ok(None);
        };
        Ok(self
            .requests
            .iter()
            .find(|r| r.request_id == request)
            .map(|r| r.document_id))
    }
    fn requests_of_doc(&self, doc: i64) -> Result<Vec<String>, String> {
        Ok(self
            .requests
            .iter()
            .filter(|r| r.document_id == doc)
            .map(|r| r.request_id.clone())
            .collect())
    }
}

/// Which library an edit was written in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Written {
    /// This computer's own library (its queue): the edit was applied here as
    /// it was made, so its record is known here.
    Here,
    /// The copy another computer wrote (its journal): the edit is in that
    /// copy's own record of requests.
    InCopy,
}

/// What became of one creation replayed in this exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreationOutcome {
    /// It got this document in the library the edit is applied to.
    Applied(i64),
    /// It was refused there.
    Refused,
}

/// What is known about a queued manual edit before it is replayed.
pub(crate) struct EditSource<'a> {
    /// The library the edit was written in.
    pub records: &'a dyn Records,
    pub written: Written,
    /// What became of the creations replayed so far in this exchange, by the
    /// document (in the library the edit was written in) each one made. A
    /// creation is named here only by its own entry's result: the receipt of
    /// an edit never says anything about it.
    pub creations: &'a BTreeMap<i64, CreationOutcome>,
    /// The document the receipt of the edit itself named, in that library.
    pub receipt_doc: Option<i64>,
}

/// A manual record's numbered edit: a capture that carries the record's number
/// (an `ImportCapture`, or an `ImportBundle` of no PDFs, which is what the
/// native import queues for the same thing).
fn manual_edit(mutation: &IntranetMutation) -> Option<&Capture> {
    match mutation {
        IntranetMutation::ImportCapture { capture }
        | IntranetMutation::ImportBundle { capture, .. }
            if capture.is_manual() && capture.document_number != MANUAL_PENDING =>
        {
            Some(capture)
        }
        _ => None,
    }
}

/// Whether a mutation is a manual record's numbered edit.
pub(crate) fn is_manual_edit(mutation: &IntranetMutation) -> bool {
    manual_edit(mutation).is_some()
}

fn readdress(mutation: IntranetMutation, number: &str) -> IntranetMutation {
    let set = |capture: &mut Capture| {
        if !same_number(&capture.document_number, number) {
            capture.source_url = format!("{MANUAL_ORIGIN}{number}");
            capture.document_number = number.to_owned();
        }
    };
    match mutation {
        IntranetMutation::ImportCapture { mut capture } => {
            set(&mut capture);
            IntranetMutation::ImportCapture { capture }
        }
        IntranetMutation::ImportBundle { mut capture, files } => {
            set(&mut capture);
            IntranetMutation::ImportBundle { capture, files }
        }
        other => other,
    }
}

const CANNOT_TELL: &str = "não foi possível ter certeza de qual registro da cópia compartilhada ela corrige, porque o número pode ter mudado ao sincronizar.";
const DELETED_HERE: &str = "o registro foi excluído neste computador.";
const DELETED_THERE: &str = "o registro foi excluído no computador de onde esta alteração veio.";
const REFUSED: &str = "a criação desse registro foi recusada pela cópia compartilhada.";

/// The words of a refused edit. The text is the person's own work: it stays
/// on the card.
fn not_applied(capture: &Capture, why: &str) -> String {
    // A record the person deleted is information, not a task to redo.
    let tail = if why == DELETED_HERE || why == DELETED_THERE {
        "O texto da edição está guardado neste aviso, caso ainda seja necessário; você pode copiá-lo ou dispensar o aviso."
    } else {
        "O texto da edição está guardado neste aviso; copie-o e refaça a edição no registro certo, se ainda for necessária."
    };
    format!(
        "A edição do registro “{}” (número {} neste computador) não foi aplicada: {why} {tail}",
        short_title(&capture.title),
        capture.document_number
    )
}

/// The record an edit was written against, in the library it comes from.
fn written_against(source: &EditSource<'_>, capture: &Capture) -> Result<Option<i64>, String> {
    let own_request = normalized_request(&capture.request_id);
    let number = &capture.document_number;
    let mut found = None;
    match source.written {
        Written::Here => {
            if let Some(doc) = source.receipt_doc {
                match source.records.number_of_doc(doc)? {
                    // A record this library has since deleted: the receipt is
                    // still the proof of where the edit was written.
                    None => found = Some(doc),
                    Some(n) if same_number(&n, number) => found = Some(doc),
                    Some(_) => {}
                }
            }
            if found.is_none() {
                if let Some(request) = &own_request {
                    found = source.records.doc_of_request(request)?;
                }
            }
            if found.is_none() {
                found = source.records.doc_by_number(number)?;
            }
        }
        Written::InCopy => {
            if let Some(request) = &own_request {
                found = source.records.doc_of_request(request)?;
            }
            if found.is_none() {
                found = source.receipt_doc;
            }
        }
    }
    Ok(found)
}

/// How many request ids of a record are kept to find it again.
const MAX_IDENTITIES: usize = 64;

/// The request ids that identify the record an edit was written against
/// (the request of the edit itself is not one: it exists only where the edit
/// was made). Empty when the record is unknown.
pub(crate) fn edit_identity(mutation: &IntranetMutation, source: &EditSource<'_>) -> Vec<String> {
    let Some(capture) = manual_edit(mutation) else {
        return Vec::new();
    };
    let own = normalized_request(&capture.request_id);
    let Ok(Some(local)) = written_against(source, capture) else {
        return Vec::new();
    };
    let mut ids = source.records.requests_of_doc(local).unwrap_or_default();
    ids.retain(|r| Some(r) != own.as_ref());
    ids.truncate(MAX_IDENTITIES);
    ids
}

/// An edit of a manual record is addressed by the record's number, and a
/// creation made before the exchange is numbered again when it is replayed
/// on the shared copy (another record may already hold its number). Replaying
/// the edit as written would put it on that other record. This finds the
/// record the edit was written against and re-addresses the edit to that same
/// record's number in `target`, using only what already exists and travels:
///
/// 0. the edit is already in `target` (its answer was lost and it is sent
///    again) and the record it was written against is confirmed to be the
///    one it landed on (by the outcome of its creation, or by all its other
///    request ids that resolve there agreeing on one record): it goes to
///    that record. A request id alone proves nothing, and otherwise the
///    capture goes as written, for the importer to check;
/// 1. the record is gone from the library the edit comes from (the person
///    deleted it): the edit is not applied, whatever else is true;
/// 2. its creation was replayed in this exchange (or an earlier, interrupted
///    one kept where it went): it follows the number the creation got, and a
///    creation that was refused takes the edit with it;
/// 3. otherwise the request ids of the record are looked up in `target`: the
///    one record they lead to there is the same record, wherever its number
///    is now (the edit moves when another record holds the old number);
///    ids that lead to different records cannot be trusted;
/// 4. no id in common, nothing known of the record, or a holder of the number
///    with no ids: nothing contradicts the number, which is what it always
///    was addressed by.
///
/// An `Err` is a refusal to apply it: the edit becomes a conflict to decide,
/// never a change of another record.
pub(crate) fn retarget_manual_edit(
    mutation: IntranetMutation,
    source: &EditSource<'_>,
    ids: &crate::idmap::IdMap,
    target: Option<&dyn Records>,
) -> Result<IntranetMutation, String> {
    let Some(capture) = manual_edit(&mutation).cloned() else {
        return Ok(mutation);
    };
    let uncertain = |why: &str| not_applied(&capture, why);
    let unreadable = |_: String| uncertain(CANNOT_TELL);
    let own_request = normalized_request(&capture.request_id);
    let number = capture.document_number.clone();
    let number_there = |t: &dyn Records, doc: i64| -> Result<String, String> {
        t.number_of_doc(doc)
            .map_err(unreadable)?
            .ok_or_else(|| uncertain(CANNOT_TELL))
    };

    // 1. The record the edit was written against.
    let local = written_against(source, &capture).map_err(unreadable)?;
    let Some(local) = local else {
        return match source.written {
            Written::Here => Err(uncertain(DELETED_HERE)),
            Written::InCopy => Ok(mutation),
        };
    };
    let creation =
        source
            .creations
            .get(&local)
            .copied()
            .or_else(|| match ids.learned_document(local) {
                Some(Some(there)) => Some(CreationOutcome::Applied(there)),
                _ => None,
            });

    // 0. Already there: the edit was applied and its answer was lost. The
    // request id alone proves nothing (two records may have been given the
    // same one). A known creation outcome decides (step 2); without one, it
    // is the record that edit landed on only when every other request id of
    // the record that resolves there agrees on that one record. Otherwise
    // the capture goes as it was written and the importer checks it as it
    // always did.
    if let (Some(t), Some(request), None) = (target, &own_request, creation) {
        if let Some(doc) = t.doc_of_request(request).map_err(unreadable)? {
            // The other request ids of the record that resolve there must all
            // agree on that one record (and there must be one).
            let mut others = BTreeSet::new();
            for other in source
                .records
                .requests_of_doc(local)
                .map_err(unreadable)?
                .iter()
                .filter(|r| Some(*r) != own_request.as_ref())
            {
                if let Some(found) = t.doc_of_request(other).map_err(unreadable)? {
                    others.insert(found);
                }
            }
            if others == BTreeSet::from([doc]) {
                return Ok(readdress(mutation, &number_there(t, doc)?));
            }
        }
    }

    // The record is gone from the library the edit comes from.
    if source
        .records
        .number_of_doc(local)
        .map_err(unreadable)?
        .is_none()
    {
        return Err(uncertain(match source.written {
            Written::Here => DELETED_HERE,
            Written::InCopy => DELETED_THERE,
        }));
    }

    // 2. Its creation.
    match creation {
        Some(CreationOutcome::Refused) => return Err(uncertain(REFUSED)),
        Some(CreationOutcome::Applied(there)) => {
            let t = target.ok_or_else(|| uncertain(CANNOT_TELL))?;
            return Ok(readdress(mutation, &number_there(t, there)?));
        }
        None => {}
    }

    // 3. By identity.
    let Some(t) = target else {
        return Ok(mutation);
    };
    let mut same = BTreeSet::new();
    for request in source
        .records
        .requests_of_doc(local)
        .map_err(unreadable)?
        .iter()
        .filter(|r| Some(*r) != own_request.as_ref())
    {
        if let Some(doc) = t.doc_of_request(request).map_err(unreadable)? {
            same.insert(doc);
        }
    }
    match same.len() {
        // 4. Nothing to compare, nothing that contradicts.
        0 => Ok(mutation),
        1 => {
            let doc = *same.iter().next().expect("one element");
            let holder = t.doc_by_number(&number).map_err(unreadable)?;
            if holder == Some(doc) {
                Ok(mutation)
            } else {
                Ok(readdress(mutation, &number_there(t, doc)?))
            }
        }
        _ => Err(uncertain(CANNOT_TELL)),
    }
}

/// What a refused edit of a manual record said, to be shown (and copied) on
/// its conflict card: the text is the person's own work.
pub struct RefusedEdit {
    pub title: String,
    pub number: String,
    pub text: String,
    /// The record was deleted (by the person, or where the edit came from):
    /// the text is kept in case it is still needed, nothing is to be redone.
    pub deleted: bool,
}

/// The edit that any conflict of a manual record carries.
pub fn manual_edit_in(conflict: &crate::intranet::SyncConflict) -> Option<RefusedEdit> {
    manual_edit(&conflict.mutation).map(|capture| RefusedEdit {
        title: capture.title.clone(),
        number: capture.document_number.clone(),
        text: capture.text.clone(),
        deleted: conflict.reason.contains(DELETED_HERE) || conflict.reason.contains(DELETED_THERE),
    })
}

/// The edit that a conflict that cannot be applied carries, when it is the
/// edit of a manual record.
pub fn refused_manual_edit(conflict: &crate::intranet::SyncConflict) -> Option<RefusedEdit> {
    if conflict.unresolved {
        manual_edit_in(conflict)
    } else {
        None
    }
}

/// The documents (in the library the changes were written in) that
/// creations waiting to be replayed made.
pub(crate) fn pending_creations<'a>(
    entries: impl IntoIterator<Item = (&'a IntranetMutation, &'a [crate::idmap::CreatedId])>,
) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();
    for (mutation, created) in entries {
        if let IntranetMutation::ImportCapture { capture } = mutation {
            if capture.is_manual() && capture.document_number == MANUAL_PENDING {
                out.extend(
                    created
                        .iter()
                        .filter(|c| c.kind == crate::idmap::IdKind::Document)
                        .map(|c| c.id),
                );
            }
        }
    }
    out
}

/// The document a creation entry made, in the library it was written in
/// (`None` for anything that is not the creation of a manual record).
pub(crate) fn creation_document(
    mutation: &IntranetMutation,
    created: &[crate::idmap::CreatedId],
) -> Option<i64> {
    match mutation {
        IntranetMutation::ImportCapture { capture }
            if capture.is_manual() && capture.document_number == MANUAL_PENDING =>
        {
            receipt_document(created)
        }
        _ => None,
    }
}

/// The document an edit's receipt named (the record the edit landed on).
pub(crate) fn receipt_document(created: &[crate::idmap::CreatedId]) -> Option<i64> {
    created
        .iter()
        .find(|c| c.kind == crate::idmap::IdKind::Document)
        .map(|c| c.id)
}

/// The outcome of a creation from what its replay answered: the document it
/// got there, or its refusal.
pub(crate) fn creation_outcome(
    there: Option<&[crate::idmap::CreatedId]>,
) -> Option<CreationOutcome> {
    match there {
        None => Some(CreationOutcome::Refused),
        Some(list) => receipt_document(list).map(CreationOutcome::Applied),
    }
}

// ---------------------------------------------------------------------------
// The record a form was opened on, and the conflict that is retried

/// The record a form was opened on: its number and the request ids of its
/// versions. A sync that runs while the form is open may give that row and
/// number to another record; saving checks that it is still this one, and
/// only asks for proof when something shows it may not be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualRecordRef {
    pub number: String,
    /// Every request id the record had when the form opened.
    pub identity: Vec<String>,
    /// The record was a creation still waiting to be sent: its number may
    /// have been given again, so the same row and number prove nothing.
    pub provisional: bool,
    /// When the record's first version was stored (what the ficha shows as
    /// "added"): it travels unchanged through the adoption of a snapshot, so
    /// when no id of the record resolves any more it tells whether the row
    /// still holds the same record.
    pub added_at: String,
}

impl Store {
    /// The reference to remember when a form opens on a record.
    pub fn manual_record_ref(&self, id: i64) -> Result<ManualRecordRef> {
        let number = self
            .document_number_by_id(id)?
            .ok_or_else(|| anyhow::anyhow!("Documento não encontrado."))?;
        let identity = self.requests_of_doc(id).map_err(|e| anyhow::anyhow!(e))?;
        let provisional = self.provisional_documents()?.contains(&id);
        let added_at = self.document_added_at(id)?.unwrap_or_default();
        Ok(ManualRecordRef {
            number,
            identity,
            provisional,
            added_at,
        })
    }

    /// When the first version of a document was stored here.
    fn document_added_at(&self, id: i64) -> Result<Option<String>> {
        Ok(self.conn.query_row(
            "SELECT min(created_at) FROM captures WHERE document_id=?1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        )?)
    }

    /// [`Store::update_manual_document`] for a form that was opened on a
    /// record (`opened`): the new version goes to that record, even when a
    /// synchronization meanwhile gave its row id and number to another one
    /// (then to the number it has now). A record that cannot be found when
    /// something shows it moved is refused, and the text stays with the
    /// person.
    pub fn update_manual_document_expecting(
        &mut self,
        id: i64,
        opened: Option<&ManualRecordRef>,
        fields: &crate::manual::ManualFields,
    ) -> Result<crate::model::ImportReceipt> {
        let target = match opened {
            None => id,
            Some(opened) => self.form_target(id, opened)?,
        };
        self.update_manual_document(target, fields)
    }

    /// Where a form opened on `opened` saves. Where nothing shows that the
    /// row changed hands, as always: by its id.
    fn form_target(&self, id: i64, opened: &ManualRecordRef) -> Result<i64> {
        // Nothing to compare (a record that predates the requests): the
        // save that always was.
        if opened.identity.is_empty() {
            return Ok(id);
        }
        // Where the ids the form knew lead now.
        let mut leads = BTreeSet::new();
        for request in &opened.identity {
            if let Some(doc) = self
                .doc_of_request(request)
                .map_err(|e| anyhow::anyhow!(e))?
            {
                leads.insert(doc);
            }
        }
        // Some id of the record resolves to this row: the same record.
        if leads.contains(&id) {
            return Ok(id);
        }
        // Some id resolves to another row: the record moved, and it is saved
        // there (ids that lead to several rows prove nothing).
        if leads.len() == 1 {
            return Ok(leads.into_iter().next().expect("one element"));
        }
        // No id resolves anywhere. The same row and number can then be
        // another record that took them, so it is saved here only when the
        // record was stored at the very same moment as when the form opened
        // (a creation that was waiting to be sent has no such proof).
        if leads.is_empty() && !opened.provisional && !opened.added_at.is_empty() {
            let number = self.document_number_by_id(id)?;
            let same_number_now = number
                .as_deref()
                .is_some_and(|n| same_number(n, &opened.number));
            if same_number_now
                && self.document_added_at(id)?.as_deref() == Some(opened.added_at.as_str())
            {
                return Ok(id);
            }
        }
        anyhow::bail!(
            "O registro mudou ao sincronizar e não foi encontrado neste computador. O que você escreveu continua nesta janela: copie o texto, abra o registro de novo e confira o número antes de salvar."
        )
    }

    /// The edit of a conflict, validated against this library as it is now:
    /// the record it was written against, found by identity, is the one that
    /// holds the number (or the edit goes to the number it has). `Err` when
    /// that cannot be established: it is then never applied by number.
    pub(crate) fn revalidated_manual_edit(
        &self,
        conflict: &crate::intranet::SyncConflict,
    ) -> Result<IntranetMutation, String> {
        let Some(capture) = manual_edit(&conflict.mutation) else {
            return Ok(conflict.mutation.clone());
        };
        let cannot = not_applied(
            capture,
            "não foi possível confirmar que o registro que ela corrige ainda é o que tem esse número.",
        );
        // A conflict stored by an earlier version (or of a record with no
        // identity) has nothing to check: it behaves as it always did.
        if conflict.identity.is_empty() {
            return Ok(conflict.mutation.clone());
        }
        let Ok(Some(doc)) = document_of_identity(&self.conn, &conflict.identity) else {
            return Err(cannot);
        };
        let holder = self.doc_by_number(&capture.document_number)?;
        if holder == Some(doc) {
            return Ok(conflict.mutation.clone());
        }
        match self.number_of_doc(doc)? {
            Some(number) => Ok(readdress(conflict.mutation.clone(), &number)),
            None => Err(cannot),
        }
    }
}

// ---------------------------------------------------------------------------
// The notice

/// What this library shows of a manual record: its number and title, and the
/// request ids of all its versions.
type Shown = (String, String, Vec<String>);

/// The manual numbers this library presents, read before a snapshot replaces
/// the library.
#[derive(Default)]
pub(crate) struct Presented(Vec<Shown>);

impl Presented {
    /// Reads them (best effort: a row that cannot be read gives no notice).
    pub(crate) fn read(conn: &Connection) -> Self {
        let mut out = Vec::new();
        let _ = (|| -> Result<()> {
            let mut requests: BTreeMap<i64, Vec<(i64, String)>> = BTreeMap::new();
            let mut statement = conn.prepare(
                "SELECT r.document_id,r.capture_id,r.request_id FROM requests r ORDER BY r.capture_id,r.request_id",
            )?;
            for row in statement.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (document, capture, request) = row?;
                requests
                    .entry(document)
                    .or_default()
                    .push((capture, request));
            }
            let mut statement = conn.prepare(
                "SELECT d.id,c.payload FROM documents d JOIN captures c ON c.id=d.latest_capture_id",
            )?;
            for row in
                statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            {
                let (document, blob) = row?;
                let Some(own) = requests.get(&document) else {
                    continue;
                };
                let Some(capture) = read_capture(&blob) else {
                    continue;
                };
                if capture.is_manual() && capture.document_number != MANUAL_PENDING {
                    out.push((
                        capture.document_number,
                        capture.title,
                        own.iter().map(|(_, request)| request.clone()).collect(),
                    ));
                }
            }
            Ok(())
        })();
        Self(out)
    }

    /// Compares with the library as it is now (the snapshot adopted, inside
    /// the same transaction) and records one notice for each record whose
    /// number differs. The record is the one that any of the request ids
    /// leads to, when they all lead to one. Best effort: the exchange never
    /// fails because of it. `event` names the adopted state (revision and
    /// generation), so the same transition of the same state is never
    /// recorded twice.
    pub(crate) fn record_changes(&self, conn: &Connection, event: &str) {
        let _ = (|| -> Result<()> {
            for (number, title, requests) in &self.0 {
                let Some(document) = document_of_identity(conn, requests)? else {
                    continue;
                };
                let after: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT c.payload FROM documents d JOIN captures c ON c.id=d.latest_capture_id WHERE d.id=?1",
                        [document],
                        |r| r.get(0),
                    )
                    .optional()?;
                let Some(after) = after.as_deref().and_then(read_capture) else {
                    continue;
                };
                if !after.is_manual()
                    || after.document_number == MANUAL_PENDING
                    || same_number(&after.document_number, number)
                {
                    continue;
                }
                // Every request id that confirmed the match comes first, then
                // the rest.
                let mut uids: Vec<String> = Vec::new();
                for request in requests {
                    if conn
                        .query_row(
                            "SELECT 1 FROM requests WHERE request_id=?1 AND document_id=?2",
                            params![request, document],
                            |_| Ok(()),
                        )
                        .optional()?
                        .is_some()
                    {
                        uids.push(request.clone());
                    }
                }
                for request in requests {
                    if !uids.contains(request) {
                        uids.push(request.clone());
                    }
                }
                uids.truncate(MAX_IDENTITIES);
                let notice = RenumberNotice {
                    id: 0,
                    uids,
                    title: title.clone(),
                    from: number.clone(),
                    to: after.document_number,
                    at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    dismissed: false,
                };
                let dedup = Sha256::digest(
                    format!(
                        "{}\n{}\n{}\n{event}",
                        notice.uids.first().map(String::as_str).unwrap_or(""),
                        notice.from,
                        notice.to
                    )
                    .as_bytes(),
                )
                .to_vec();
                conn.execute(
                    "INSERT OR IGNORE INTO renumber_notices(dedup,dismissed,payload) VALUES(?1,0,?2)",
                    params![dedup, seal(&serde_json::to_vec(&notice)?)?],
                )?;
            }
            conn.execute(
                "DELETE FROM renumber_notices WHERE dismissed=1 AND id NOT IN
                   (SELECT id FROM renumber_notices WHERE dismissed=1 ORDER BY id DESC LIMIT ?1)",
                [KEEP_DISMISSED],
            )?;
            Ok(())
        })();
    }
}

/// The one document that the request ids lead to (`None` when none of them
/// is here, or when they lead to different documents: no guess).
fn document_of_identity(conn: &Connection, requests: &[String]) -> Result<Option<i64>> {
    let mut found = BTreeSet::new();
    let mut statement = conn.prepare("SELECT document_id FROM requests WHERE request_id=?1")?;
    for request in requests {
        if let Some(document) = statement
            .query_row([request], |r| r.get::<_, i64>(0))
            .optional()?
        {
            found.insert(document);
        }
    }
    Ok(if found.len() == 1 {
        found.into_iter().next()
    } else {
        None
    })
}

impl Store {
    /// The local table of notices. Idempotent, like the other local tables;
    /// nothing in it is part of a snapshot, and adopting one leaves it alone.
    pub(crate) fn ensure_renumber_notices(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS renumber_notices(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                dedup BLOB NOT NULL UNIQUE,
                dismissed INTEGER NOT NULL DEFAULT 0,
                payload BLOB NOT NULL
            );",
        )?;
        Ok(())
    }

    fn all_renumber_notices(&self) -> Result<Vec<RenumberNotice>> {
        self.cached("renumber_notices", || {
            let mut statement = self
                .conn
                .prepare("SELECT id,payload FROM renumber_notices ORDER BY id DESC")?;
            let rows =
                statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (id, blob) = row?;
                // A notice that cannot be read (another account's key) is
                // only skipped.
                let Ok(plain) = unseal(&blob) else { continue };
                if let Ok(mut notice) = serde_json::from_slice::<RenumberNotice>(&plain) {
                    notice.id = id;
                    out.push(notice);
                }
            }
            Ok(out)
        })
    }

    /// The notices the person has not dismissed, newest first.
    pub fn pending_renumber_notices(&self) -> Result<Vec<RenumberNotice>> {
        Ok(self
            .all_renumber_notices()?
            .into_iter()
            .filter(|n| !n.dismissed)
            .collect())
    }

    /// Notices recorded after row `last` (what one exchange just found),
    /// oldest first.
    pub(crate) fn renumber_notices_after(&self, last: i64) -> Result<Vec<RenumberNotice>> {
        Ok(self
            .all_renumber_notices()?
            .into_iter()
            .filter(|n| n.id > last)
            .rev()
            .collect())
    }

    /// The last row of the table (0: empty).
    pub(crate) fn last_renumber_notice_id(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT coalesce(max(id),0) FROM renumber_notices",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// The record a notice is about, when this library has it and it is not
    /// in the trash.
    pub fn renumber_notice_document(&self, notice: &RenumberNotice) -> Option<i64> {
        let id = document_of_identity(&self.conn, &notice.uids).ok()??;
        crate::trash::require_active_document(&self.conn, id)
            .ok()
            .map(|_| id)
    }

    /// Marks a notice as read. It stays as history, so the old number keeps
    /// being found on this computer.
    pub fn dismiss_renumber_notice(&self, id: i64) -> Result<()> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT payload FROM renumber_notices WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(blob) = blob else { return Ok(()) };
        let mut notice: RenumberNotice = serde_json::from_slice(&unseal(&blob)?)?;
        if notice.dismissed {
            return Ok(());
        }
        notice.dismissed = true;
        self.conn.execute(
            "UPDATE renumber_notices SET dismissed=1,payload=?1 WHERE id=?2",
            params![seal(&serde_json::to_vec(&notice)?)?, id],
        )?;
        Ok(())
    }

    /// Dismisses every notice.
    pub fn dismiss_all_renumber_notices(&self) -> Result<()> {
        for notice in self.pending_renumber_notices()? {
            self.dismiss_renumber_notice(notice.id)?;
        }
        Ok(())
    }

    /// The numbers records had before this computer adopted a copy that
    /// renumbered them, by the document they are now (newest change first).
    pub(crate) fn old_numbers_by_document(&self) -> Result<BTreeMap<i64, Vec<String>>> {
        self.cached("renumber_old_numbers", || {
            let mut out: BTreeMap<i64, Vec<String>> = BTreeMap::new();
            for notice in self.all_renumber_notices()? {
                if let Some(document) = document_of_identity(&self.conn, &notice.uids)? {
                    let numbers = out.entry(document).or_default();
                    if !numbers.contains(&notice.from) {
                        numbers.push(notice.from);
                    }
                }
            }
            Ok(out)
        })
    }

    /// "Número anterior neste computador": the numbers this record had here
    /// before, except the one it has now.
    pub fn previous_numbers(&self, document_id: i64) -> Vec<String> {
        let now = self.document_number_by_id(document_id).ok().flatten();
        self.old_numbers_by_document()
            .ok()
            .and_then(|map| map.get(&document_id).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|number| now.as_deref().is_none_or(|now| !same_number(now, number)))
            .collect()
    }

    /// Documents whose creation is still waiting in the outgoing queue: a
    /// manual record there shows a number this library gave it, and the shared
    /// copy may give it another one when the creation is replayed there.
    pub(crate) fn provisional_documents(&self) -> Result<BTreeSet<i64>> {
        self.cached("provisional_documents", || {
            let waiting: i64 =
                self.conn
                    .query_row("SELECT count(*) FROM offline_outbox", [], |r| r.get(0))?;
            if waiting == 0 {
                return Ok(BTreeSet::new());
            }
            let queue = self.list_offline_outbox()?;
            Ok(pending_creations(
                queue.iter().map(|e| (&e.mutation, e.created.as_slice())),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::intranet::{IntranetConfig, IntranetMode};
    use crate::manual::ManualFields;
    use crate::renumber::{refused_manual_edit, Presented, RenumberNotice, KEEP_DISMISSED};
    use crate::storage::Store;

    const KEY: &str = "senha-de-teste-123";

    fn folder_config(member: &str, files: Vec<String>) -> IntranetConfig {
        IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: files,
            folder_key: KEY.into(),
            ..IntranetConfig::default()
        }
    }

    fn fields(title: &str, text: &str) -> ManualFields {
        ManualFields {
            kind: "ligacao".into(),
            title: title.into(),
            origin: "Gabinete".into(),
            document_date: String::new(),
            text: text.into(),
        }
    }

    /// Two computers sharing one folder, both already in sync on a library
    /// with one SIGADOC document (the copy exists and each one has adopted it).
    struct Pair {
        _dir: tempfile::TempDir,
        a: Store,
        b: Store,
    }

    fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
        let cfg = store.load_intranet_config().unwrap();
        store
            .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
            .unwrap()
    }

    fn pair() -> Pair {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let files = vec![drive.display().to_string()];
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", files.clone()))
            .unwrap();
        a.import_capture(&crate::model::sample_capture()).unwrap();
        sync(&mut a);
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", files))
            .unwrap();
        sync(&mut b);
        Pair { _dir: dir, a, b }
    }

    fn create(store: &mut Store, title: &str, text: &str) -> (i64, String) {
        let receipt = store.create_manual_document(&fields(title, text)).unwrap();
        let number = store
            .load_document(receipt.document_id)
            .unwrap()
            .summary
            .document_number;
        (receipt.document_id, number)
    }

    fn number_of(store: &Store, title: &str) -> Option<String> {
        store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.title == title)
            .map(|d| d.document_number)
    }

    fn id_of(store: &Store, title: &str) -> i64 {
        store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.title == title)
            .unwrap_or_else(|| panic!("sem “{title}”"))
            .id
    }

    fn text_of(store: &Store, number: &str) -> String {
        let id = store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.document_number == number)
            .unwrap_or_else(|| panic!("sem {number}"))
            .id;
        store.load_document(id).unwrap().capture.text
    }

    /// The collision of the observed incident: A publishes its record, B
    /// (still without that copy) created one with the same number.
    fn collide(p: &mut Pair) -> (String, String) {
        let (_, x) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (_, y) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_eq!(x, y, "the two libraries numbered alike");
        (x, y)
    }

    /// The defect: B creates a manual record offline and edits it before the
    /// first exchange; the shared copy already has another record with the
    /// same number. B's creation is numbered again on replay, and its edit,
    /// addressed by the old number, must follow it, never land on the other
    /// record.
    #[test]
    fn an_edit_of_a_renumbered_creation_never_lands_on_the_record_that_took_its_number() {
        let mut p = pair();
        let (x_number, _) = collide(&mut p);
        let y = id_of(&p.b, "Registro de B");
        p.b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        sync(&mut p.b);
        sync(&mut p.a);

        for store in [&p.a, &p.b] {
            assert_eq!(
                number_of(store, "Registro de A").as_deref(),
                Some(x_number.as_str()),
                "A's record keeps its number and its title (B's edit replaced it?)"
            );
            assert_eq!(
                text_of(store, &x_number),
                "Texto de A",
                "B's edit was applied to A's record"
            );
            let b_number = number_of(store, "Registro de B").expect("B's record exists");
            assert_ne!(b_number, x_number);
            assert_eq!(text_of(store, &b_number), "Texto de B, editado");
        }
    }

    /// The same defect on the other road: the edit reaches a third computer
    /// through the journal of another folder copy, not through its own queue.
    #[test]
    fn an_edit_that_arrives_in_the_journal_of_another_copy_follows_its_creation_too() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let both = vec![drive.display().to_string(), pen.display().to_string()];
        let only = |path: &std::path::Path| vec![path.display().to_string()];
        let sync_with = |store: &mut Store, files: Vec<String>| {
            let mut cfg = store.load_intranet_config().unwrap();
            cfg.folder_files = files;
            store.save_intranet_config(&cfg).unwrap();
            store
                .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
                .unwrap()
        };
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", both.clone()))
            .unwrap();
        a.import_capture(&crate::model::sample_capture()).unwrap();
        sync_with(&mut a, both.clone());
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", both.clone()))
            .unwrap();
        sync_with(&mut b, both.clone());

        // A writes only to the drive, with more changes than B will make
        // (so its copy is the base); B only to the pen drive.
        let (x, _) = create(&mut a, "Registro de A", "Texto de A");
        for n in 0..4 {
            let mut task = crate::tasks::Task::new(x);
            task.title = format!("Tarefa {n}");
            a.save_task(&task).unwrap();
        }
        sync_with(&mut a, only(&drive));
        let (y, y_number) = create(&mut b, "Registro de B", "Texto de B");
        b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        sync_with(&mut b, only(&pen));
        let revision = |path: &std::path::Path| {
            crate::sharedfolder::read_header(path)
                .unwrap()
                .unwrap()
                .revision
        };
        assert!(revision(&drive) > revision(&pen), "the drive is the base");

        // A reads both: B's creation and edit arrive from the other copy.
        let report = sync_with(&mut a, both.clone());
        assert!(report.merged_foreign >= 2, "{}", report.summary());
        assert_eq!(report.conflicts, 0, "{}", report.summary());
        let x_number = number_of(&a, "Registro de A").unwrap();
        assert_eq!(x_number, y_number, "A kept its number");
        assert_eq!(text_of(&a, &x_number), "Texto de A");
        let b_number = number_of(&a, "Registro de B").unwrap();
        assert_ne!(b_number, x_number);
        assert_eq!(text_of(&a, &b_number), "Texto de B, editado");
        // And B, adopting that copy, is told.
        let report = sync_with(&mut b, both);
        assert_eq!(report.renumbered.len(), 1, "{}", report.summary());
        assert_eq!(number_of(&b, "Registro de B").unwrap(), b_number);
        assert_eq!(text_of(&b, &x_number), "Texto de A");
    }

    #[test]
    fn the_renumber_notice_appears_once_after_a_collision_and_not_without_one() {
        let mut p = pair();
        // No collision: B's record is the only one, it keeps its number.
        let (_, z_number) = create(&mut p.b, "Registro sem colisão", "Texto");
        let report = sync(&mut p.b);
        assert!(report.renumbered.is_empty(), "{}", report.summary());
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        assert_eq!(number_of(&p.b, "Registro sem colisão").unwrap(), z_number);
        sync(&mut p.a);

        // Collision: A publishes a record; B's, made without that copy,
        // takes the next number when it is sent.
        let (_, x_number) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (_, y_number) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_ne!(x_number, z_number);
        let report = sync(&mut p.b);
        let new_number = number_of(&p.b, "Registro de B").unwrap();
        if y_number == new_number {
            panic!("the record kept its number: {y_number}");
        }
        assert_eq!(report.renumbered.len(), 1);
        assert!(
            report.summary().contains(&format!(
                "O registro “Registro de B” passou de {y_number} para {new_number} ao sincronizar"
            )),
            "{}",
            report.summary()
        );
        let notices = p.b.pending_renumber_notices().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "Registro de B");
        assert_eq!((&notices[0].from, &notices[0].to), (&y_number, &new_number));
        assert_eq!(
            notices[0].sentence(),
            format!(
                "O registro “Registro de B” passou de {y_number} para {new_number} ao sincronizar."
            )
        );
        // Exactly once: the next exchanges, and the one on A, add nothing.
        sync(&mut p.b);
        sync(&mut p.a);
        sync(&mut p.b);
        assert_eq!(p.b.pending_renumber_notices().unwrap().len(), 1);
        assert!(p.a.pending_renumber_notices().unwrap().is_empty());
        // The notice is local: the shared snapshot carries nothing about it.
        let snapshot = serde_json::to_string(&p.b.export_snapshot().unwrap()).unwrap();
        assert!(!snapshot.contains("renumber"));
    }

    #[test]
    fn a_record_without_identity_gives_no_notice() {
        let mut p = pair();
        let (_, x) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (y_id, y) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_eq!(x, y);
        // A record that, on this computer, knows no request of its creation
        // (the library predates them): it is renumbered like the others, but
        // nothing says it is the same record, and the title is not a proof.
        p.b.conn
            .execute("DELETE FROM requests WHERE document_id=?1", [y_id])
            .unwrap();
        let report = sync(&mut p.b);
        assert_ne!(number_of(&p.b, "Registro de B").unwrap(), y);
        assert!(report.renumbered.is_empty(), "{}", report.summary());
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
    }

    #[test]
    fn dismissing_a_notice_hides_it_and_applying_the_same_snapshot_again_does_not_bring_it_back() {
        let mut p = pair();
        collide(&mut p);
        sync(&mut p.b);
        let notice = p.b.pending_renumber_notices().unwrap().remove(0);
        p.b.dismiss_renumber_notice(notice.id).unwrap();
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        // The same snapshot, adopted again.
        let snapshot = p.b.export_snapshot().unwrap();
        for _ in 0..2 {
            p.b.apply_snapshot_and(
                &snapshot,
                |_, _| unreachable!(),
                |_| unreachable!(),
                |_| Ok(()),
            )
            .unwrap();
        }
        sync(&mut p.b);
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        // Dismissing twice, or an unknown one, is harmless.
        p.b.dismiss_renumber_notice(notice.id).unwrap();
        p.b.dismiss_renumber_notice(9999).unwrap();
        // And a notice that is not dismissed survives the same adoption,
        // still one.
        let mut q = pair();
        collide(&mut q);
        sync(&mut q.b);
        let snapshot = q.b.export_snapshot().unwrap();
        q.b.apply_snapshot_and(
            &snapshot,
            |_, _| unreachable!(),
            |_| unreachable!(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(q.b.pending_renumber_notices().unwrap().len(), 1);
    }

    #[test]
    fn a_scratch_library_never_writes_a_notice() {
        let moved_copy = |store: &Store, title: &str, to: &str| {
            let mut copy = store.export_snapshot().unwrap();
            for row in &mut copy.captures {
                if row.capture.title == title {
                    row.capture.document_number = to.to_owned();
                    row.capture.source_url = format!("ponte://manual/{to}");
                }
            }
            copy
        };
        let mut p = pair();
        let (_, y) = create(&mut p.b, "Registro de B", "Texto de B");
        let moved = format!("MANUAL-{}/0042", &y[7..11]);
        let other = moved_copy(&p.b, "Registro de B", &moved);
        // The scratch road (the temporary library of an exchange): nothing.
        p.b.apply_snapshot(&other, |_, _| unreachable!(), |_| unreachable!())
            .unwrap();
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        assert_eq!(number_of(&p.b, "Registro de B").unwrap(), moved);
        // The library the person works in: the same change is told.
        let mut q = pair();
        let (_, y) = create(&mut q.b, "Registro de B", "Texto de B");
        let other = moved_copy(&q.b, "Registro de B", &moved);
        q.b.apply_snapshot_and(
            &other,
            |_, _| unreachable!(),
            |_| unreachable!(),
            |_| Ok(()),
        )
        .unwrap();
        let notices = q.b.pending_renumber_notices().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(
            (notices[0].from.as_str(), notices[0].to.as_str()),
            (y.as_str(), moved.as_str())
        );
    }

    #[test]
    fn the_old_number_is_found_by_search_and_shown_in_the_sheet_while_the_history_exists() {
        let mut p = pair();
        let (x, _) = collide(&mut p);
        sync(&mut p.b);
        let y = id_of(&p.b, "Registro de B");
        let hits: Vec<i64> =
            p.b.list_documents(&x)
                .unwrap()
                .iter()
                .map(|d| d.id)
                .collect();
        assert!(
            hits.contains(&y),
            "the record renumbered is found by its old number"
        );
        assert!(
            hits.contains(&id_of(&p.b, "Registro de A")),
            "and the one that holds it now"
        );
        assert_eq!(p.b.previous_numbers(y), vec![x.clone()]);
        assert!(p
            .b
            .previous_numbers(id_of(&p.b, "Registro de A"))
            .is_empty());
        // Dismissed, it stays findable (the history is what finds it).
        p.b.dismiss_all_renumber_notices().unwrap();
        assert!(p.b.list_documents(&x).unwrap().iter().any(|d| d.id == y));
        assert_eq!(p.b.previous_numbers(y), vec![x]);
        // On the computer that did not renumber anything: only its own numbers.
        sync(&mut p.a);
        assert!(p
            .a
            .previous_numbers(id_of(&p.a, "Registro de B"))
            .is_empty());
    }

    #[test]
    fn a_creation_waiting_to_be_sent_has_a_provisional_number() {
        let mut p = pair();
        // Nothing waits for a library alone, nor for what came from the copy.
        let dir = tempfile::tempdir().unwrap();
        let mut alone = Store::open(&dir.path().join("alone.sqlite3")).unwrap();
        let (own, _) = create(&mut alone, "Registro solto", "Texto");
        assert!(!alone.load_document(own).unwrap().summary.provisional);
        assert!(!p
            .a
            .list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.provisional));

        // In the shared-folder mode the number is provisional until it is sent.
        let (a_doc, _) = create(&mut p.a, "Registro de A", "Texto de A");
        assert!(p.a.load_document(a_doc).unwrap().summary.provisional);
        // An edit while it waits does not change that, and the SIGADOC
        // document never is.
        p.a.update_manual_document(a_doc, &fields("Registro de A", "Texto novo"))
            .unwrap();
        let docs = p.a.list_documents("").unwrap();
        assert_eq!(docs.iter().filter(|d| d.provisional).count(), 1);
        assert!(docs.iter().filter(|d| d.provisional).all(|d| d.id == a_doc));
        assert!(docs.iter().filter(|d| !d.manual).all(|d| !d.provisional));
        sync(&mut p.a);
        assert!(!p
            .a
            .list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.provisional));
        // B receives it as final.
        sync(&mut p.b);
        assert!(!p
            .b
            .list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.provisional));
        // B's own record is provisional until B sends it.
        let (b_doc, _) = create(&mut p.b, "Registro de B", "Texto de B");
        assert!(p.b.load_document(b_doc).unwrap().summary.provisional);
        sync(&mut p.b);
        assert!(!p
            .b
            .list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.provisional));
    }
    /// CONFIRMED, NOT FIXED in this release (review of item 2, blocker 5): two
    /// different messages that carry the same request id (an e-mail's
    /// Message-ID is only a label the sender chose) are taken for one record
    /// when the second is replayed: the "prior request" branch of the import
    /// answers with the first one, and the content of the second is lost from
    /// the library (it only stays in this computer's guard copy and in the
    /// queue's journal). Fixing it needs a content check in the replay path.
    #[test]
    #[ignore = "defeito confirmado, fora do escopo desta versão (revisão do item 2, bloqueio 5)"]
    fn two_different_messages_with_the_same_request_id_both_survive_the_exchange() {
        let mut p = pair();
        let request_id = uuid::Uuid::new_v4().to_string();
        let make = |title: &str, text: &str| {
            let mut capture = fields(title, text).capture(crate::model::MANUAL_PENDING, "Teste");
            capture.request_id = request_id.clone();
            capture
        };
        p.a.import_capture(&make("Mensagem de A", "Texto de A"))
            .unwrap();
        sync(&mut p.a);
        p.b.import_capture(&make("Mensagem de B", "Texto de B"))
            .unwrap();
        sync(&mut p.b);
        sync(&mut p.a);
        for store in [&p.a, &p.b] {
            let titles: Vec<String> = store
                .list_documents("")
                .unwrap()
                .into_iter()
                .map(|d| d.title)
                .collect();
            assert!(
                titles.iter().any(|t| t == "Mensagem de B"),
                "a mensagem de B sumiu do acervo: {titles:?}"
            );
        }
    }

    /// A principal computer (host) and a client with its own library, on a
    /// private port; `offline` stops the host, `online` brings it back.
    struct Team {
        _dir: tempfile::TempDir,
        host: Store,
        client: Store,
        db: std::path::PathBuf,
        token: String,
        server: Option<crate::intranet::HostServer>,
    }

    impl Team {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("host.sqlite3");
            let token = crate::intranet::generate_token();
            let mut host = Store::open(&db).unwrap();
            host.save_intranet_config(&IntranetConfig {
                mode: IntranetMode::Host,
                member_name: "ANFITRIAO".into(),
                bind_addr: "127.0.0.1:0".into(),
                host_url: "http://127.0.0.1:7878".into(),
                cabinet_token: token.clone(),
                ..IntranetConfig::default()
            })
            .unwrap();
            let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
            client
                .save_intranet_config(&IntranetConfig {
                    mode: IntranetMode::Client,
                    member_name: "SERVIDOR-A".into(),
                    host_url: "http://127.0.0.1:1".into(),
                    cabinet_token: token.clone(),
                    ..IntranetConfig::default()
                })
                .unwrap();
            let mut team = Self {
                _dir: dir,
                host,
                client,
                db,
                token,
                server: None,
            };
            team.online();
            team.client.sync_from_host(false).unwrap();
            team
        }

        fn online(&mut self) {
            self.offline();
            let server =
                crate::intranet::HostServer::start(self.db.clone(), "127.0.0.1:0").unwrap();
            let mut cfg = self.client.load_intranet_config().unwrap();
            cfg.host_url = format!("http://{}", server.local_addr());
            cfg.cabinet_token = self.token.clone();
            self.client.save_intranet_config(&cfg).unwrap();
            self.server = Some(server);
        }

        fn offline(&mut self) {
            if let Some(mut server) = self.server.take() {
                server.stop();
            }
        }
    }

    /// BLOCKER of the review: a client that created a record offline and
    /// edited it sends its queue to the principal; the principal numbers the
    /// creation again, and the edit, still addressed by the old number, must
    /// follow it instead of landing on the principal's own record.
    #[test]
    fn a_team_client_s_offline_edit_follows_its_creation_to_the_number_the_main_computer_gave() {
        let mut t = Team::new();
        t.offline();
        let (y, y_number) = create(&mut t.client, "Registro do cliente", "Texto do cliente");
        t.client
            .update_manual_document(
                y,
                &fields("Registro do cliente", "Texto do cliente, editado"),
            )
            .unwrap();
        let (_, x_number) = create(&mut t.host, "Registro do principal", "Texto do principal");
        assert_eq!(x_number, y_number, "the scenario needs colliding numbers");
        t.online();
        t.client.sync_from_host(false).unwrap();

        assert!(t.client.list_offline_outbox().unwrap().is_empty());
        assert!(
            t.client.list_sync_conflicts().unwrap().is_empty(),
            "{:?}",
            t.client.list_sync_conflicts().unwrap()
        );
        for store in [&t.host, &t.client] {
            assert_eq!(
                text_of(store, &x_number),
                "Texto do principal",
                "the client's edit was applied to the principal's record"
            );
            let number = number_of(store, "Registro do cliente").expect("the client's record");
            assert_ne!(number, x_number);
            assert_eq!(text_of(store, &number), "Texto do cliente, editado");
        }
        // The client is told about the change, once.
        assert_eq!(t.client.pending_renumber_notices().unwrap().len(), 1);
    }

    /// Two folders (a Drive and a pen drive) and two computers, both joined to
    /// both, on a library with one SIGADOC document.
    struct Folders {
        dir: tempfile::TempDir,
        drive: std::path::PathBuf,
        pen: std::path::PathBuf,
        a: Store,
        b: Store,
    }

    impl Folders {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let drive = dir.path().join("Drive").join("ponte-sync.ponte");
            let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
            std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
            std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
            let both = vec![drive.display().to_string(), pen.display().to_string()];
            let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
            a.save_intranet_config(&folder_config("Casa", both.clone()))
                .unwrap();
            a.import_capture(&crate::model::sample_capture()).unwrap();
            let mut f = Self {
                dir,
                drive,
                pen,
                a,
                b: Store::open(std::path::Path::new(":memory:")).unwrap(),
            };
            f.sync_a(Where::Both);
            let mut b = Store::open(&f.dir.path().join("b.sqlite3")).unwrap();
            b.save_intranet_config(&folder_config("Trabalho", both))
                .unwrap();
            f.b = b;
            f.sync_b(Where::Both);
            f
        }

        fn files(&self, at: Where) -> Vec<String> {
            let drive = self.drive.display().to_string();
            let pen = self.pen.display().to_string();
            match at {
                Where::Both => vec![drive, pen],
                Where::Drive => vec![drive],
                Where::Pen => vec![pen],
            }
        }

        fn sync_a(&mut self, at: Where) -> crate::intranet::FolderSyncReport {
            let files = self.files(at);
            sync_in(&mut self.a, files)
        }

        fn sync_b(&mut self, at: Where) -> crate::intranet::FolderSyncReport {
            let files = self.files(at);
            sync_in(&mut self.b, files)
        }
    }

    #[derive(Clone, Copy)]
    enum Where {
        Both,
        Drive,
        Pen,
    }

    fn sync_in(store: &mut Store, files: Vec<String>) -> crate::intranet::FolderSyncReport {
        let mut cfg = store.load_intranet_config().unwrap();
        cfg.folder_files = files;
        store.save_intranet_config(&cfg).unwrap();
        store
            .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
            .unwrap()
    }

    /// `count` distinct changes (tasks of the SIGADOC document), enough to
    /// push older entries out of the copy's journal.
    fn busy_work(store: &mut Store, count: usize) {
        let document = store
            .list_documents("")
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .id;
        for n in 0..count {
            let mut task = crate::tasks::Task::new(document);
            task.title = format!("Tarefa de enchimento {n}");
            store.save_task(&task).unwrap();
        }
    }

    /// BLOCKER 2 of the review, first road: B creates and edits Y, then
    /// trashes and purges it and creates Z, which takes the freed number,
    /// all before the first exchange. Nothing of Y's history may be applied to
    /// the record (A's) that holds that number in the shared copy.
    #[test]
    fn a_purged_creation_s_edits_never_land_on_the_record_that_holds_its_number() {
        let mut p = pair();
        let (_, x) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (y, y_number) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_eq!(x, y_number);
        p.b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        p.b.trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: y,
        })
        .unwrap();
        p.b.purge_trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: y,
        })
        .unwrap();
        let (_, z_number) = create(&mut p.b, "Registro Z", "Texto de Z");
        assert_eq!(z_number, y_number, "Z takes the freed number");
        sync(&mut p.b);
        // The person deleted the record: the edit is a card that says so.
        let cards = p.b.list_sync_conflicts().unwrap();
        assert!(
            cards.iter().any(|c| refused_manual_edit(c).is_some()
                && c.reason.contains("excluído neste computador")),
            "{cards:?}"
        );
        sync(&mut p.a);
        for store in [&p.a, &p.b] {
            assert_eq!(
                number_of(store, "Registro de A").as_deref(),
                Some(x.as_str()),
                "A's record keeps its number and its title"
            );
            assert_eq!(
                text_of(store, &x),
                "Texto de A",
                "A's record was overwritten"
            );
            assert!(number_of(store, "Registro Z").is_some(), "Z survives");
        }
    }

    /// BLOCKER 2 of the review, second road: the link between B's queued edit
    /// and its creation is lost because the copy's journal no longer has the
    /// creation (it was renumbered by A, who then published enough changes to
    /// compact its journal). The edit is addressed by the old number, which A's
    /// own record holds now.
    #[test]
    fn an_edit_whose_creation_left_the_journal_still_reaches_its_own_record() {
        let mut f = Folders::new();
        // A keeps X on the Drive, with more changes than B will make (so its
        // copy is the base); B publishes Y on the pen drive only.
        let (x_id, x_number) = create(&mut f.a, "Registro de A", "Texto de A");
        let _ = x_id;
        busy_work(&mut f.a, 4);
        f.sync_a(Where::Drive);
        let (_, y_number) = create(&mut f.b, "Registro de B", "Texto de B");
        assert_eq!(x_number, y_number);
        f.sync_b(Where::Pen);
        // A merges both: Y is renumbered; A then publishes enough work to
        // push Y's creation out of the journal.
        f.sync_a(Where::Both);
        let y_new = number_of(&f.a, "Registro de B").unwrap();
        assert_ne!(y_new, x_number);
        crate::sharedfolder::set_journal_cap_for_test(6);
        busy_work(&mut f.a, 8);
        f.sync_a(Where::Both);
        // B still shows Y under the old number and edits it.
        let y = id_of(&f.b, "Registro de B");
        assert_eq!(
            f.b.load_document(y).unwrap().summary.document_number,
            y_number
        );
        f.b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        f.sync_b(Where::Both);
        f.sync_a(Where::Both);
        for store in [&f.a, &f.b] {
            assert_eq!(
                text_of(store, &x_number),
                "Texto de A",
                "A's record was overwritten"
            );
            assert_eq!(
                text_of(store, &number_of(store, "Registro de B").unwrap()),
                "Texto de B, editado"
            );
        }
    }

    /// MAJOR 3 of the review: a record that came in the very first copy (no
    /// creation in any journal) edited on another computer and read through
    /// that computer's own folder copy must be edited, not refused as if its
    /// creation had failed.
    #[test]
    fn an_edit_of_a_record_from_the_first_copy_that_arrives_in_another_journal_is_applied() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let both = vec![drive.display().to_string(), pen.display().to_string()];
        // A creates the record alone, before any folder exists.
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        let (m, m_number) = create(&mut a, "Registro antigo", "Texto antigo");
        let _ = m;
        a.save_intranet_config(&folder_config("Casa", both.clone()))
            .unwrap();
        sync_in(&mut a, both.clone());
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", both.clone()))
            .unwrap();
        sync_in(&mut b, both.clone());
        assert_eq!(number_of(&b, "Registro antigo").unwrap(), m_number);

        // A publishes more on the Drive only; B edits the record and
        // publishes on the pen drive only.
        busy_work(&mut a, 4);
        sync_in(&mut a, vec![drive.display().to_string()]);
        let b_id = id_of(&b, "Registro antigo");
        b.update_manual_document(b_id, &fields("Registro antigo", "Texto antigo, editado"))
            .unwrap();
        sync_in(&mut b, vec![pen.display().to_string()]);
        let revision = |path: &std::path::Path| {
            crate::sharedfolder::read_header(path)
                .unwrap()
                .unwrap()
                .revision
        };
        assert!(revision(&drive) > revision(&pen), "the Drive is the base");

        let report = sync_in(&mut a, both);
        assert_eq!(
            report.conflicts,
            0,
            "{:?}",
            a.list_sync_conflicts().unwrap()
        );
        assert_eq!(text_of(&a, &m_number), "Texto antigo, editado");
    }

    /// MAJOR 6a of the review: the synchronization with the main computer
    /// returned an empty summary even when it renumbered a record of this
    /// computer; the notice existed but the summary said nothing.
    #[test]
    fn the_summary_of_a_team_synchronization_tells_which_record_was_renumbered() {
        let mut t = Team::new();
        t.offline();
        let (_, y_number) = create(&mut t.client, "Registro do cliente", "Texto do cliente");
        let (_, x_number) = create(&mut t.host, "Registro do principal", "Texto do principal");
        assert_eq!(x_number, y_number);
        t.online();
        let (changed, summary) = t.client.sync_from_host_with_summary(false, false).unwrap();
        assert!(changed);
        let now = number_of(&t.client, "Registro do cliente").unwrap();
        assert_eq!(
            summary,
            format!(
                "O registro “Registro do cliente” passou de {y_number} para {now} ao sincronizar."
            )
        );
        // Told once: the next synchronization has nothing to say.
        let (_, again) = t.client.sync_from_host_with_summary(false, false).unwrap();
        assert!(again.is_empty(), "{again}");
    }

    /// The scenario of MAJOR 5: B creates Y and edits it (two versions), A
    /// (whose copy is the base) merges B's copy and renumbers Y.
    fn y_with_two_versions_renumbered_by_a(f: &mut Folders) -> (String, String) {
        let (_, x_number) = create(&mut f.a, "Registro de A", "Texto de A");
        busy_work(&mut f.a, 4);
        f.sync_a(Where::Drive);
        let (y, y_number) = create(&mut f.b, "Registro de B", "Texto de B");
        assert_eq!(x_number, y_number);
        f.b.update_manual_document(y, &fields("Registro de B", "Texto de B, segunda versão"))
            .unwrap();
        f.sync_b(Where::Pen);
        f.sync_a(Where::Both);
        assert_ne!(number_of(&f.a, "Registro de B").unwrap(), x_number);
        (x_number, y_number)
    }

    /// A drops the first version of Y (trash, then purge): the request of that
    /// version goes with it.
    fn purge_first_version(store: &mut Store, title: &str) {
        let document = id_of(store, title);
        let versions = store.capture_versions(document).unwrap();
        assert_eq!(versions.len(), 2);
        let first = versions.last().unwrap().id;
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Capture,
            id: first,
        };
        store.trash_item(target.clone()).unwrap();
        store.purge_trash_item(target).unwrap();
    }

    /// MAJOR 5 of the review: the notice is told even when the first version
    /// of the record (the one whose request was the only identity) is gone
    /// from the copy, because a later version identifies the record on both
    /// sides.
    #[test]
    fn the_notice_is_told_when_the_first_version_of_the_record_was_purged_before_the_adoption() {
        let mut f = Folders::new();
        let (_, y_number) = y_with_two_versions_renumbered_by_a(&mut f);
        purge_first_version(&mut f.a, "Registro de B");
        f.sync_a(Where::Both);
        let report = f.sync_b(Where::Both);
        assert_eq!(report.renumbered.len(), 1, "{}", report.summary());
        let notice = f.b.pending_renumber_notices().unwrap().remove(0);
        assert_eq!(notice.from, y_number);
        assert!(f.b.renumber_notice_document(&notice).is_some());
    }

    /// The other half of MAJOR 5: the notice already exists when the first
    /// version is purged; the old number stays findable and the notice keeps
    /// opening the record.
    #[test]
    fn the_old_number_stays_findable_after_the_first_version_of_the_record_is_purged() {
        let mut f = Folders::new();
        let (x_number, y_number) = y_with_two_versions_renumbered_by_a(&mut f);
        f.sync_a(Where::Both);
        f.sync_b(Where::Both);
        let notice = f.b.pending_renumber_notices().unwrap().remove(0);
        purge_first_version(&mut f.a, "Registro de B");
        f.sync_a(Where::Both);
        f.sync_b(Where::Both);
        let y = id_of(&f.b, "Registro de B");
        assert_eq!(f.b.previous_numbers(y), vec![y_number.clone()]);
        assert!(f
            .b
            .list_documents(&y_number)
            .unwrap()
            .iter()
            .any(|d| d.id == y));
        assert_eq!(f.b.renumber_notice_document(&notice), Some(y));
        assert_eq!(text_of(&f.b, &x_number), "Texto de A");
    }

    /// MAJOR 4 of the review: what the person wrote in an edit that could not
    /// be applied is on the card, to be read and copied.
    #[test]
    fn a_refused_edit_keeps_its_text_on_the_conflict_card() {
        let mut p = pair();
        let (_, x) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        sync(&mut p.b);
        // B edits the record and, before the exchange, throws it away.
        let doc = id_of(&p.b, "Registro de A");
        p.b.update_manual_document(doc, &fields("Registro de A", "Texto escrito por B"))
            .unwrap();
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: doc,
        };
        p.b.trash_item(target.clone()).unwrap();
        p.b.purge_trash_item(target).unwrap();
        let report = sync(&mut p.b);
        assert_eq!(report.conflicts, 1, "{}", report.summary());
        let conflicts = p.b.list_sync_conflicts().unwrap();
        let card = conflicts.iter().find(|c| c.unresolved).expect("the card");
        let edit = refused_manual_edit(card).expect("the edit is on the card");
        assert_eq!(edit.text, "Texto escrito por B");
        assert_eq!(edit.title, "Registro de A");
        assert_eq!(edit.number, x);
        assert!(card.reason.contains("Registro de A"), "{}", card.reason);
        assert!(card.reason.contains(&x), "{}", card.reason);
        assert!(card.reason.contains("não foi aplicada"), "{}", card.reason);
        assert!(
            card.reason.contains("excluído neste computador"),
            "{}",
            card.reason
        );
        // Information, not a task: nothing asks the person to redo it.
        assert!(!card.reason.contains("refaça"), "{}", card.reason);
        assert!(
            card.reason.contains("caso ainda seja necessário"),
            "{}",
            card.reason
        );
        assert!(crate::renumber::manual_edit_in(card).unwrap().deleted);
        // The card is offered (copy and dismiss) even while a drain waits.
        p.b.conn
            .execute(
                "INSERT INTO offline_id_map(kind,local_id,target_id) VALUES('document',9999,NULL)",
                [],
            )
            .unwrap();
        assert!(!p.b.conflict_info(card).waiting_sync);
        p.b.conn.execute("DELETE FROM offline_id_map", []).unwrap();
        // Another kind of change, or a conflict that can be decided, has none.
        let mut ordinary = card.clone();
        ordinary.unresolved = false;
        assert!(refused_manual_edit(&ordinary).is_none());
    }

    /// MAJOR 2, the other side of the coin: edits of records that are in the
    /// shared copy as they always were (MANUAL- and EMAIL-, a SIGADOC
    /// document, a record another computer trashed) never become the new kind
    /// of conflict.
    #[test]
    fn ordinary_edits_of_records_already_in_the_shared_copy_are_not_turned_into_conflicts() {
        let mut p = pair();
        let (_, m_number) = create(&mut p.a, "Registro antigo", "Texto antigo");
        let mail = ManualFields {
            kind: "email".into(),
            ..fields("E-mail antigo", "Corpo antigo")
        };
        let mail_id = p.a.create_manual_document(&mail).unwrap().document_id;
        let mail_number = p.a.load_document(mail_id).unwrap().summary.document_number;
        assert!(mail_number.starts_with("EMAIL-"), "{mail_number}");
        let (_, t_number) = create(&mut p.a, "Registro que será apagado", "Texto");
        sync(&mut p.a);
        sync(&mut p.b);

        // B edits all of them, and the SIGADOC document too.
        for (title, text) in [
            ("Registro antigo", "Texto antigo, editado"),
            ("Registro que será apagado", "Texto editado por B"),
        ] {
            let id = id_of(&p.b, title);
            p.b.update_manual_document(id, &fields(title, text))
                .unwrap();
        }
        let id = id_of(&p.b, "E-mail antigo");
        p.b.update_manual_document(
            id,
            &ManualFields {
                kind: "email".into(),
                ..fields("E-mail antigo", "Corpo antigo, editado")
            },
        )
        .unwrap();
        let mut official = crate::model::sample_capture();
        official.request_id = uuid::Uuid::new_v4().to_string();
        official.text.push_str("\nEditado por B.");
        p.b.import_capture(&official).unwrap();
        // Meanwhile A throws the last one away.
        let t = id_of(&p.a, "Registro que será apagado");
        p.a.trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: t,
        })
        .unwrap();
        sync(&mut p.a);

        let report = sync(&mut p.b);
        let conflicts = p.b.list_sync_conflicts().unwrap();
        // Only the edit of the trashed record is refused, for the reason that
        // always applied to it, not for an uncertain target.
        assert_eq!(report.conflicts, 1, "{conflicts:?}");
        assert!(
            !conflicts[0].reason.contains("não foi aplicada"),
            "{}",
            conflicts[0].reason
        );
        sync(&mut p.a);
        for store in [&p.a, &p.b] {
            assert_eq!(text_of(store, &m_number), "Texto antigo, editado");
            assert_eq!(text_of(store, &mail_number), "Corpo antigo, editado");
            let official_text = store
                .list_documents("")
                .unwrap()
                .into_iter()
                .find(|d| !d.manual)
                .map(|d| store.load_document(d.id).unwrap().capture.text)
                .unwrap();
            assert!(official_text.ends_with("Editado por B."), "{official_text}");
        }
        assert!(number_of(&p.a, "Registro que será apagado").is_none());
        let _ = t_number;
    }

    /// The history is capped, but only what was already read: a notice that
    /// is still pending is never dropped to make room.
    #[test]
    fn the_history_cap_never_drops_a_pending_notice() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        let notice = |n: usize, dismissed: bool| RenumberNotice {
            id: 0,
            uids: vec![format!("uid-{n}")],
            title: format!("Registro {n}"),
            from: "MANUAL-2026/0001".into(),
            to: "MANUAL-2026/0002".into(),
            at: "2026-11-22T10:00:00Z".into(),
            dismissed,
        };
        // The three oldest are pending; 305 newer ones were read.
        for n in 0..308 {
            let read = n >= 3;
            store
                .conn
                .execute(
                    "INSERT INTO renumber_notices(dedup,dismissed,payload) VALUES(?1,?2,?3)",
                    rusqlite::params![
                        vec![n as u8, (n >> 8) as u8],
                        i64::from(read),
                        crate::protection::seal(&serde_json::to_vec(&notice(n, read)).unwrap())
                            .unwrap()
                    ],
                )
                .unwrap();
        }
        Presented::default().record_changes(&store.conn, "state");
        let count = |dismissed: i64| -> i64 {
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM renumber_notices WHERE dismissed=?1",
                    [dismissed],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(count(0), 3, "pending notices stay");
        assert_eq!(count(1), KEEP_DISMISSED, "the read ones are capped");
        assert_eq!(store.pending_renumber_notices().unwrap().len(), 3);
    }

    // ---- Round 2 -----------------------------------------------------------

    /// A: a form opened on Y while a background sync adopts a copy in which
    /// Y's old row id and number belong to another record.
    #[test]
    fn a_form_opened_before_a_sync_never_saves_into_the_record_that_took_its_row() {
        let mut p = pair();
        let (_, x_number) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (y, y_number) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_eq!(x_number, y_number);
        // The form opens on Y: it remembers the record, not only the row.
        let opened = p.b.manual_record_ref(y).unwrap();
        // The background sync adopts the copy.
        sync(&mut p.b);
        let holder = p.b.load_document(y).unwrap();
        assert_eq!(
            holder.summary.title, "Registro de A",
            "the scenario needs A's record on Y's old row"
        );
        assert_eq!(holder.summary.document_number, y_number);

        // Saving goes to the record the form was opened on, at its new number.
        let receipt =
            p.b.update_manual_document_expecting(
                y,
                Some(&opened),
                &fields("Registro de B", "Texto de B, editado"),
            )
            .unwrap();
        assert_ne!(receipt.document_id, y);
        assert_eq!(
            text_of(&p.b, &x_number),
            "Texto de A",
            "A's record untouched"
        );
        let moved = number_of(&p.b, "Registro de B").unwrap();
        assert_ne!(moved, x_number);
        assert_eq!(text_of(&p.b, &moved), "Texto de B, editado");

        // A record that cannot be found is refused, with a message that keeps
        // the text in the form (the caller leaves the form open on an error).
        let lost = crate::renumber::ManualRecordRef {
            number: "MANUAL-1999/0001".into(),
            identity: vec![uuid::Uuid::new_v4().to_string()],
            provisional: false,
            added_at: String::new(),
        };
        let error =
            p.b.update_manual_document_expecting(y, Some(&lost), &fields("Registro de B", "Outro"))
                .unwrap_err()
                .to_string();
        assert!(error.contains("O registro mudou ao sincronizar"), "{error}");
        assert_eq!(text_of(&p.b, &x_number), "Texto de A");
        // Another record on the SAME row and the SAME number, and no id of the
        // form resolves anywhere: never saved into it (a form on a creation
        // that was waiting to be sent, or one opened at another moment).
        for (provisional, added_at) in [
            (true, String::new()),
            (false, "2000-01-01T00:00:00.000Z".to_owned()),
        ] {
            let same_place = crate::renumber::ManualRecordRef {
                number: x_number.clone(),
                identity: vec![uuid::Uuid::new_v4().to_string()],
                provisional,
                added_at,
            };
            assert!(p
                .b
                .update_manual_document_expecting(
                    y,
                    Some(&same_place),
                    &fields("Registro de B", "Outro")
                )
                .is_err());
            assert_eq!(text_of(&p.b, &x_number), "Texto de A");
        }
        // A form on a record that did not move saves as always.
        let still = p.b.manual_record_ref(id_of(&p.b, "Registro de A")).unwrap();
        p.b.update_manual_document_expecting(
            id_of(&p.b, "Registro de A"),
            Some(&still),
            &fields("Registro de A", "Texto de A, revisto"),
        )
        .unwrap();
        assert_eq!(text_of(&p.b, &x_number), "Texto de A, revisto");
    }

    /// Y shared by both computers; B edits it offline; A trashes it, so B's
    /// edit is refused by the trash (an ordinary conflict).
    fn conflict_on_a_trashed_record(p: &mut Pair) -> (String, i64) {
        let (_, number) = create(&mut p.a, "Registro compartilhado", "Texto compartilhado");
        sync(&mut p.a);
        sync(&mut p.b);
        let on_b = id_of(&p.b, "Registro compartilhado");
        p.b.update_manual_document(
            on_b,
            &fields("Registro compartilhado", "Texto editado por B"),
        )
        .unwrap();
        let on_a = id_of(&p.a, "Registro compartilhado");
        p.a.trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: on_a,
        })
        .unwrap();
        sync(&mut p.a);
        let report = sync(&mut p.b);
        assert_eq!(report.conflicts, 1, "{}", report.summary());
        let conflicts = p.b.list_sync_conflicts().unwrap();
        assert!(!conflicts[0].unresolved, "an ordinary conflict");
        (number, on_a)
    }

    /// B, conflict retry: the card of a refused edit must not apply it by
    /// number to another record that took the number later.
    #[test]
    fn a_stored_manual_edit_is_not_reapplied_by_number_to_another_record() {
        let mut p = pair();
        let (number, on_a) = conflict_on_a_trashed_record(&mut p);
        // A throws the record away for good and a new one takes its number.
        p.a.purge_trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: on_a,
        })
        .unwrap();
        let (_, again) = create(&mut p.a, "Registro novo", "Texto do novo");
        assert_eq!(again, number, "the scenario needs the number reused");
        sync(&mut p.a);
        sync(&mut p.b);
        let conflict = p.b.list_sync_conflicts().unwrap().remove(0);
        let info = p.b.conflict_info(&conflict);
        assert!(info.reapply_unsafe, "only copy and dismiss are offered");
        let error =
            p.b.resolve_sync_conflict(conflict.id, crate::intranet::ConflictResolution::KeepLocal)
                .unwrap_err()
                .to_string();
        assert!(error.contains("não"), "{error}");
        assert_eq!(
            text_of(&p.b, &number),
            "Texto do novo",
            "the new record is untouched"
        );
        // Dismissing is still possible, and the text is on the card.
        assert!(crate::renumber::manual_edit_in(&conflict).is_some());
        p.b.resolve_sync_conflict(conflict.id, crate::intranet::ConflictResolution::KeepHost)
            .unwrap();
    }

    /// The other side of B: when the record is still the same, the card
    /// reapplies the edit as before.
    #[test]
    fn a_stored_manual_edit_is_reapplied_when_it_is_still_the_same_record() {
        let mut p = pair();
        let (number, on_a) = conflict_on_a_trashed_record(&mut p);
        p.a.restore_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: on_a,
        })
        .unwrap();
        sync(&mut p.a);
        sync(&mut p.b);
        let conflict = p.b.list_sync_conflicts().unwrap().remove(0);
        assert!(!p.b.conflict_info(&conflict).reapply_unsafe);
        p.b.resolve_sync_conflict(conflict.id, crate::intranet::ConflictResolution::KeepLocal)
            .unwrap();
        assert_eq!(text_of(&p.b, &number), "Texto editado por B");
    }

    /// C: a manual numbered capture queued as an ImportBundle (no PDFs) is
    /// decided like any other edit.
    #[test]
    fn a_manual_edit_queued_as_a_bundle_follows_its_creation_too() {
        let mut p = pair();
        let (x_number, _) = collide(&mut p);
        let y = id_of(&p.b, "Registro de B");
        let number = p.b.load_document(y).unwrap().summary.document_number;
        let mut capture =
            fields("Registro de B", "Texto de B, em pacote").capture(&number, "Teste");
        capture.request_id = uuid::Uuid::new_v4().to_string();
        p.b.import_bundle(&capture, &[]).unwrap();
        sync(&mut p.b);
        sync(&mut p.a);
        for store in [&p.a, &p.b] {
            assert_eq!(
                text_of(store, &x_number),
                "Texto de A",
                "A's record was overwritten"
            );
            let moved = number_of(store, "Registro de B").unwrap();
            assert_eq!(text_of(store, &moved), "Texto de B, em pacote");
        }
    }

    /// D, first half: a creation that was applied long ago (its result is
    /// gone from the journal of the copy being built) does not make a later
    /// edit of the same record look like an edit of a refused creation.
    #[test]
    fn an_edit_of_a_record_whose_creation_was_applied_and_compacted_is_applied() {
        let mut f = Folders::new();
        crate::sharedfolder::set_journal_cap_for_test(6);
        // B creates Y and both computers learn it.
        let (y, y_number) = create(&mut f.b, "Registro de B", "Texto de B");
        f.sync_b(Where::Both);
        f.sync_a(Where::Both);
        assert_eq!(number_of(&f.a, "Registro de B").unwrap(), y_number);
        // A publishes enough, on the Drive only, for Y's creation to leave
        // that journal; B edits Y and publishes on the pen drive only.
        busy_work(&mut f.a, 8);
        f.sync_a(Where::Drive);
        f.b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        f.sync_b(Where::Pen);
        let report = f.sync_a(Where::Both);
        assert_eq!(
            report.conflicts,
            0,
            "{:?}",
            f.a.list_sync_conflicts().unwrap()
        );
        assert_eq!(text_of(&f.a, &y_number), "Texto de B, editado");
    }

    /// D, second half: when the creation really was refused (the same e-mail
    /// was imported on two computers and the other one trashed it), the edit
    /// of the computer that made it ends as a card that keeps its text.
    #[test]
    fn an_edit_of_a_really_refused_creation_ends_as_a_card_with_its_text() {
        let mut p = pair();
        let request_id = uuid::Uuid::new_v4().to_string();
        let make = |text: &str| {
            let mut capture =
                fields("Mensagem recebida", text).capture(crate::model::MANUAL_PENDING, "Teste");
            capture.request_id = request_id.clone();
            capture
        };
        let a_id =
            p.a.import_capture(&make("Texto original"))
                .unwrap()
                .document_id;
        sync(&mut p.a);
        let b_id =
            p.b.import_capture(&make("Texto original"))
                .unwrap()
                .document_id;
        p.b.update_manual_document(b_id, &fields("Mensagem recebida", "Texto que B escreveu"))
            .unwrap();
        p.a.trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: a_id,
        })
        .unwrap();
        sync(&mut p.a);
        sync(&mut p.b);
        let conflicts = p.b.list_sync_conflicts().unwrap();
        let card = conflicts
            .iter()
            .find_map(crate::renumber::refused_manual_edit)
            .expect("a card for the edit");
        assert_eq!(card.text, "Texto que B escreveu");
    }

    /// E (i): a holder of the number that has no request rows says nothing
    /// against the edit: it goes by number, as it always did. A refusal needs
    /// positive evidence.
    #[test]
    fn an_edit_is_not_refused_because_the_holder_has_no_identity() {
        use crate::idmap::IdMap;
        use crate::intranet::IntranetMutation;
        use crate::renumber::{retarget_manual_edit, EditSource, Written};
        let dir = tempfile::tempdir().unwrap();
        let mut mine = Store::open(&dir.path().join("mine.sqlite3")).unwrap();
        let (y, number) = create(&mut mine, "Registro antigo", "Texto antigo");
        // The copy the edit is applied to carries the record, with no request
        // rows at all.
        let copy = mine.export_snapshot().unwrap();
        let mut theirs = Store::open(&dir.path().join("theirs.sqlite3")).unwrap();
        theirs
            .apply_snapshot(&copy, |_, _| unreachable!(), |_| unreachable!())
            .unwrap();
        theirs.conn.execute("DELETE FROM requests", []).unwrap();
        let mut capture =
            fields("Registro antigo", "Texto antigo, editado").capture(&number, "Teste");
        capture.request_id = uuid::Uuid::new_v4().to_string();
        let creations = std::collections::BTreeMap::new();
        let decided = retarget_manual_edit(
            IntranetMutation::ImportCapture { capture },
            &EditSource {
                records: &mine,
                written: Written::Here,
                creations: &creations,
                receipt_doc: Some(y),
            },
            &IdMap::default(),
            Some(&theirs),
        )
        .unwrap();
        let IntranetMutation::ImportCapture { capture } = decided else {
            panic!()
        };
        assert_eq!(capture.document_number, number, "by number, as before");
    }

    /// E (ii): a drain that stopped after the creation went: the mapping it
    /// kept is used by the next drain, even when no request is left in common
    /// (the main computer dropped the original version).
    #[test]
    fn the_next_drain_uses_the_creation_mapping_the_interrupted_one_kept() {
        use crate::idmap::{CreatedId, IdKind, IdMap};
        use crate::intranet::IntranetMutation;
        use crate::renumber::{retarget_manual_edit, EditSource, Written};
        let dir = tempfile::tempdir().unwrap();
        let mut mine = Store::open(&dir.path().join("mine.sqlite3")).unwrap();
        let mut theirs = Store::open(&dir.path().join("theirs.sqlite3")).unwrap();
        let (y, y_number) = create(&mut mine, "Registro de B", "Texto de B");
        // The main computer has its own record on that number, and B's record
        // under another one, with requests of its own.
        let (_, x_number) = create(&mut theirs, "Registro do principal", "Texto do principal");
        let (y_there, y_number_there) = create(&mut theirs, "Registro de B", "Texto de B");
        assert_eq!(x_number, y_number);
        assert_ne!(y_number_there, y_number);
        theirs
            .conn
            .execute(
                "UPDATE requests SET request_id=?1 WHERE document_id=?2",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), y_there],
            )
            .unwrap();
        let mut capture =
            fields("Registro de B", "Texto de B, editado").capture(&y_number, "Teste");
        capture.request_id = uuid::Uuid::new_v4().to_string();
        let mut ids = IdMap::default();
        ids.learn(
            &[CreatedId::new(IdKind::Document, "", y)],
            Some(&[CreatedId::new(IdKind::Document, "", y_there)]),
        );
        let creations = std::collections::BTreeMap::new();
        let moved = retarget_manual_edit(
            IntranetMutation::ImportCapture { capture },
            &EditSource {
                records: &mine,
                written: Written::Here,
                creations: &creations,
                receipt_doc: Some(y),
            },
            &ids,
            Some(&theirs),
        )
        .unwrap();
        let IntranetMutation::ImportCapture { capture } = moved else {
            panic!()
        };
        assert_eq!(capture.document_number, y_number_there);
    }

    /// E (iii): the edit was applied but its answer was lost, and the other
    /// versions went away meanwhile: the retry is recognised as applied, not
    /// refused.
    #[test]
    fn the_retry_of_an_edit_that_was_already_applied_is_not_a_conflict() {
        use crate::idmap::IdMap;
        use crate::intranet::IntranetMutation;
        use crate::renumber::{retarget_manual_edit, EditSource, Written};
        let dir = tempfile::tempdir().unwrap();
        let mut mine = Store::open(&dir.path().join("mine.sqlite3")).unwrap();
        let (y, number) = create(&mut mine, "Registro de B", "Texto de B");
        mine.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        // The main computer has the edit (and only it): the original version
        // was purged there.
        let copy = mine.export_snapshot().unwrap();
        let mut theirs = Store::open(&dir.path().join("theirs.sqlite3")).unwrap();
        theirs
            .apply_snapshot(&copy, |_, _| unreachable!(), |_| unreachable!())
            .unwrap();
        let edit_request: String = mine
            .conn
            .query_row(
                "SELECT r.request_id FROM requests r JOIN captures c ON c.id=r.capture_id WHERE r.document_id=?1 ORDER BY c.id DESC LIMIT 1",
                [y],
                |r| r.get(0),
            )
            .unwrap();
        theirs
            .conn
            .execute("DELETE FROM requests WHERE request_id<>?1", [&edit_request])
            .unwrap();
        let mut capture = fields("Registro de B", "Texto de B, editado").capture(&number, "Teste");
        capture.request_id = edit_request;
        let creations = std::collections::BTreeMap::new();
        let retried = retarget_manual_edit(
            IntranetMutation::ImportCapture { capture },
            &EditSource {
                records: &mine,
                written: Written::Here,
                creations: &creations,
                receipt_doc: Some(y),
            },
            &IdMap::default(),
            Some(&theirs),
        );
        assert!(retried.is_ok(), "{retried:?}");
    }

    /// F: every request id of the record is kept, so the alias and the button
    /// survive a later purge of any version.
    #[test]
    fn the_notice_keeps_the_identity_of_a_record_with_many_versions() {
        let mut f = Folders::new();
        let (_, _) = create(&mut f.a, "Registro de A", "Texto de A");
        busy_work(&mut f.a, 25);
        f.sync_a(Where::Drive);
        let (y, y_number) = create(&mut f.b, "Registro de B", "Texto de B");
        for n in 0..16 {
            f.b.update_manual_document(
                y,
                &fields("Registro de B", &format!("Texto de B, versão {n}")),
            )
            .unwrap();
        }
        f.sync_b(Where::Pen);
        f.sync_a(Where::Both);
        // A keeps only the ninth version of Y (the middle one) and adds one.
        let on_a = id_of(&f.a, "Registro de B");
        let versions = f.a.capture_versions(on_a).unwrap();
        assert_eq!(versions.len(), 17);
        for (position, version) in versions.iter().rev().enumerate() {
            if position == 8 {
                continue;
            }
            let target = crate::trash::Target {
                kind: crate::trash::Kind::Capture,
                id: version.id,
            };
            f.a.trash_item(target.clone()).unwrap();
            f.a.purge_trash_item(target).unwrap();
        }
        f.a.update_manual_document(on_a, &fields("Registro de B", "Texto de A sobre Y"))
            .unwrap();
        f.sync_a(Where::Both);
        let report = f.sync_b(Where::Both);
        assert_eq!(report.renumbered.len(), 1, "{}", report.summary());
        let notice = f.b.pending_renumber_notices().unwrap().remove(0);
        let doc = f.b.renumber_notice_document(&notice);
        assert_eq!(doc, Some(id_of(&f.b, "Registro de B")));
        assert_eq!(f.b.previous_numbers(doc.unwrap()), vec![y_number]);
    }

    /// KNOWN LIMIT, NOT FIXED in this release (review of item 2, round 2,
    /// blocker 1, "online"): a record was PERMANENTLY purged on the main
    /// computer and its number reused by a new record while an edit of the
    /// purged one is still in flight. The edit carries only the number (a
    /// format change would be needed to carry the record's identity), so the
    /// main computer cannot tell the two records apart and appends the text
    /// to the new one. Same family: the purge-and-reuse variants of blockers
    /// 1 ("drenagem offline") and 2 ("fila própria", "diário estrangeiro").
    #[test]
    #[ignore = "limite conhecido: número de registro apagado de vez e reutilizado com uma edição ainda em trânsito (revisão do item 2, rodada 2, bloqueios 1 e 2)"]
    fn an_edit_in_flight_never_lands_on_a_record_that_reused_a_purged_number() {
        let mut t = Team::new();
        // Y is created through the client and the main computer numbers it.
        let (y, y_number) = create(&mut t.client, "Registro do cliente", "Texto do cliente");
        // The main computer drops Y for good and a new record takes its number.
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: y,
        };
        t.host.trash_item(target.clone()).unwrap();
        t.host.purge_trash_item(target).unwrap();
        let (_, reused) = create(&mut t.host, "Registro novo do principal", "Texto do novo");
        assert_eq!(reused, y_number, "the scenario needs the number reused");
        // Before the client pulls, its form saves an edit of Y.
        let _ = t.client.update_manual_document(
            y,
            &fields("Registro do cliente", "Texto do cliente, editado"),
        );
        assert_eq!(text_of(&t.host, &y_number), "Texto do novo");
    }

    // ---- Round 3 -----------------------------------------------------------

    /// 1: two different records were given captures with the same request id
    /// (native bundle imports without PDFs). The edit of the second must not
    /// be re-addressed to the first on the mere fact that its request id
    /// exists there: the importer refuses it, as it always did, and it ends
    /// as a card that keeps its text.
    #[test]
    fn a_repeated_request_id_never_makes_an_edit_of_another_record_vanish() {
        let mut p = pair();
        let (_, x_number) = create(&mut p.a, "Registro X", "Texto de X");
        let (_, y_number) = create(&mut p.a, "Registro Y", "Texto de Y");
        sync(&mut p.a);
        sync(&mut p.b);
        let request_id = uuid::Uuid::new_v4().to_string();
        let edit = |number: &str| {
            let mut capture = fields("Título repetido", "Texto repetido").capture(number, "Teste");
            capture.request_id = request_id.clone();
            capture
        };
        p.a.import_bundle(&edit(&x_number), &[]).unwrap();
        sync(&mut p.a);
        p.b.import_bundle(&edit(&y_number), &[]).unwrap();
        let report = sync(&mut p.b);
        let cards = p.b.list_sync_conflicts().unwrap();
        assert_eq!(report.conflicts, 1, "the edit disappeared: {cards:?}");
        assert!(p.b.list_offline_outbox().unwrap().is_empty());
        assert_eq!(
            crate::renumber::manual_edit_in(&cards[0]).unwrap().text,
            "Texto repetido"
        );
        assert_eq!(text_of(&p.b, &y_number), "Texto de Y", "Y is untouched");
        assert_eq!(text_of(&p.b, &x_number), "Texto repetido", "A's edit of X");
    }

    /// 1 (decision table, non-provisional form, same row and number):
    /// some id resolves to this row → saved; no id resolves anywhere → saved
    /// only when the record was stored at the very same moment as when the
    /// form opened; otherwise refused, with the text kept in the window.
    #[test]
    fn a_form_whose_ids_resolve_nowhere_saves_only_with_the_same_timestamp() {
        let mut p = pair();
        let (_, number) = create(&mut p.a, "Registro compartilhado", "Texto compartilhado");
        sync(&mut p.a);
        sync(&mut p.b);
        let on_b = id_of(&p.b, "Registro compartilhado");
        let opened = p.b.manual_record_ref(on_b).unwrap();
        assert!(!opened.identity.is_empty() && !opened.provisional);
        assert!(!opened.added_at.is_empty());
        // Someone else saves a second version and purges the first: the ids
        // the form knew resolve nowhere, and the first version's moment is gone.
        let on_a = id_of(&p.a, "Registro compartilhado");
        p.a.update_manual_document(
            on_a,
            &fields("Registro compartilhado", "Texto compartilhado, v2"),
        )
        .unwrap();
        let first = p.a.capture_versions(on_a).unwrap().last().unwrap().id;
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Capture,
            id: first,
        };
        p.a.trash_item(target.clone()).unwrap();
        p.a.purge_trash_item(target).unwrap();
        sync(&mut p.a);
        sync(&mut p.b);
        assert_eq!(
            p.b.load_document(on_b).unwrap().summary.document_number,
            number,
            "same row, same number"
        );
        let error =
            p.b.update_manual_document_expecting(
                on_b,
                Some(&opened),
                &fields("Registro compartilhado", "Texto de B"),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("O registro mudou ao sincronizar"), "{error}");
        assert_eq!(text_of(&p.b, &number), "Texto compartilhado, v2");
        // Opened again, the form knows the record as it is now.
        let reopened = p.b.manual_record_ref(on_b).unwrap();
        p.b.update_manual_document_expecting(
            on_b,
            Some(&reopened),
            &fields("Registro compartilhado", "Texto de B"),
        )
        .unwrap();
        assert_eq!(text_of(&p.b, &number), "Texto de B");

        // No id resolves, but the record was stored at the same moment: the
        // same record.
        let kept = p.b.manual_record_ref(on_b).unwrap();
        p.b.conn.execute("DELETE FROM requests", []).unwrap();
        let receipt =
            p.b.update_manual_document_expecting(
                on_b,
                Some(&kept),
                &fields("Registro compartilhado", "Texto de B, outra vez"),
            )
            .unwrap();
        assert_eq!(receipt.document_id, on_b);
        // A form with no identity at all is the save that always was.
        let blank = crate::renumber::ManualRecordRef {
            number: "MANUAL-1999/0001".into(),
            identity: Vec::new(),
            provisional: false,
            added_at: String::new(),
        };
        p.b.update_manual_document_expecting(
            on_b,
            Some(&blank),
            &fields("Registro compartilhado", "Texto de B, de novo"),
        )
        .unwrap();
    }

    /// 1 (the review's sequence): a non-provisional form on Y; X is adopted on
    /// Y's old row and number and Y moves; then Y's only version the form
    /// knew is replaced and purged, so no id resolves. Saving must never go to
    /// X.
    #[test]
    fn a_non_provisional_form_never_saves_into_another_record_on_its_row() {
        let mut f = Folders::new();
        let (_, x_number) = create(&mut f.a, "Registro de A", "Texto de A");
        busy_work(&mut f.a, 4);
        f.sync_a(Where::Drive);
        let (y, y_number) = create(&mut f.b, "Registro de B", "Texto de B");
        assert_eq!(x_number, y_number);
        f.sync_b(Where::Pen);
        let opened = f.b.manual_record_ref(y).unwrap();
        assert!(!opened.provisional, "the queue is empty: sent already");
        // The copies are reconciled and B adopts X on Y's row and number.
        f.sync_a(Where::Both);
        f.sync_b(Where::Both);
        assert_eq!(f.b.load_document(y).unwrap().summary.title, "Registro de A");
        // While the form is open, A saves v2 of Y and purges v1.
        let on_a = id_of(&f.a, "Registro de B");
        f.a.update_manual_document(on_a, &fields("Registro de B", "Texto de B, v2"))
            .unwrap();
        let first = f.a.capture_versions(on_a).unwrap().last().unwrap().id;
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Capture,
            id: first,
        };
        f.a.trash_item(target.clone()).unwrap();
        f.a.purge_trash_item(target).unwrap();
        f.sync_a(Where::Both);
        f.sync_b(Where::Both);
        assert_eq!(f.b.load_document(y).unwrap().summary.title, "Registro de A");
        assert!(f
            .b
            .update_manual_document_expecting(
                y,
                Some(&opened),
                &fields("Registro de B", "Texto de B, do formulário"),
            )
            .is_err());
        assert_eq!(text_of(&f.b, &x_number), "Texto de A", "X was written");
    }

    /// 2 (two reused request ids): two captures reuse two request ids for two
    /// different records. The first link that points to X is not enough: the
    /// ids of Y that resolve there disagree, so nothing is re-addressed and
    /// both edits end as cards that keep their text.
    #[test]
    fn two_repeated_request_ids_never_make_the_first_edit_vanish() {
        let mut p = pair();
        let (_, x_number) = create(&mut p.a, "Registro X", "Texto de X");
        let (_, y_number) = create(&mut p.a, "Registro Y", "Texto de Y");
        sync(&mut p.a);
        sync(&mut p.b);
        let (u, v) = (
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        );
        let edit = |number: &str, request: &str, text: &str| {
            let mut capture = fields("Título repetido", text).capture(number, "Teste");
            capture.request_id = request.to_owned();
            capture
        };
        p.a.import_bundle(&edit(&x_number, &u, "Texto U"), &[])
            .unwrap();
        p.a.import_bundle(&edit(&x_number, &v, "Texto V"), &[])
            .unwrap();
        sync(&mut p.a);
        p.b.import_bundle(&edit(&y_number, &u, "Texto U"), &[])
            .unwrap();
        p.b.import_bundle(&edit(&y_number, &v, "Texto V"), &[])
            .unwrap();
        let report = sync(&mut p.b);
        let cards = p.b.list_sync_conflicts().unwrap();
        assert_eq!(report.conflicts, 2, "an edit disappeared: {cards:?}");
        assert!(p.b.list_offline_outbox().unwrap().is_empty());
        assert_eq!(text_of(&p.b, &y_number), "Texto de Y", "Y is untouched");
    }

    /// 3: a conflict stored by the current release has no `identity`: it keeps
    /// the options it had ("Manter a minha" reapplies it).
    #[test]
    fn a_conflict_stored_without_identity_keeps_its_options() {
        let mut p = pair();
        let (number, on_a) = conflict_on_a_trashed_record(&mut p);
        // The shape the previous release stored: no `identity`.
        let card = p.b.list_sync_conflicts().unwrap().remove(0);
        let mut old_shape = serde_json::to_value(&card).unwrap();
        old_shape.as_object_mut().unwrap().remove("identity");
        p.b.conn
            .execute(
                "UPDATE offline_conflicts SET payload=?1 WHERE id=?2",
                rusqlite::params![
                    crate::protection::seal(&serde_json::to_vec(&old_shape).unwrap()).unwrap(),
                    card.id
                ],
            )
            .unwrap();
        p.a.restore_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: on_a,
        })
        .unwrap();
        sync(&mut p.a);
        sync(&mut p.b);
        let stored = p.b.list_sync_conflicts().unwrap().remove(0);
        assert!(stored.identity.is_empty(), "read from the old shape");
        assert!(!p.b.conflict_info(&stored).reapply_unsafe);
        p.b.resolve_sync_conflict(stored.id, crate::intranet::ConflictResolution::KeepLocal)
            .unwrap();
        assert_eq!(text_of(&p.b, &number), "Texto editado por B");
    }

    /// KNOWN LIMIT, NOT FIXED in this release (review of item 2, round 3,
    /// blocker 2, which already existed before this branch): the record's
    /// creation was renumbered by another computer and has left the journal
    /// of the copy, and the only version both computers still shared was
    /// purged there. The queued edit then has nothing in common with the
    /// copy to prove which record it is, and goes by number, to the record
    /// that holds the old number. Telling this case from an ordinary edit of
    /// a legacy record would need the identity carried in the edit itself (a
    /// format change planned for a later release).
    #[test]
    #[ignore = "limite conhecido: criação renumerada fora do diário e única versão em comum apagada (revisão do item 2, rodada 3, bloqueio 2)"]
    fn an_edit_with_no_version_in_common_never_lands_on_the_holder_of_the_old_number() {
        let mut f = Folders::new();
        crate::sharedfolder::set_journal_cap_for_test(6);
        let (_, x_number) = create(&mut f.a, "Registro de A", "Texto de A");
        busy_work(&mut f.a, 4);
        f.sync_a(Where::Drive);
        let (y, y_number) = create(&mut f.b, "Registro de B", "Texto de B");
        assert_eq!(x_number, y_number);
        f.sync_b(Where::Pen);
        f.sync_a(Where::Both);
        // A saves a second version of Y, purges the first and publishes
        // enough to push Y's creation out of the journal.
        let on_a = id_of(&f.a, "Registro de B");
        f.a.update_manual_document(on_a, &fields("Registro de B", "Texto de B, v2"))
            .unwrap();
        let first = f.a.capture_versions(on_a).unwrap().last().unwrap().id;
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Capture,
            id: first,
        };
        f.a.trash_item(target.clone()).unwrap();
        f.a.purge_trash_item(target).unwrap();
        busy_work(&mut f.a, 8);
        f.sync_a(Where::Both);
        // B edits Y, which it still shows under the old number.
        f.b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        f.sync_b(Where::Both);
        assert_eq!(text_of(&f.b, &x_number), "Texto de A");
    }
}
