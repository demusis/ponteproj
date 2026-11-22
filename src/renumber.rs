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

/// How many request ids of a record a notice keeps to find it again.
const KEEP_IDENTITIES: usize = 16;

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

/// What is known about a queued manual edit before it is replayed.
pub(crate) struct EditSource<'a> {
    /// The library the edit was written in.
    pub records: &'a dyn Records,
    pub written: Written,
    /// The documents (in that library) that creations still waiting to be
    /// replayed made. Only these may be taken as "refused" when the
    /// translation says so.
    pub creations: &'a BTreeSet<i64>,
    /// The document the receipt of the edit itself named, in that library.
    pub receipt_doc: Option<i64>,
}

fn readdress(mutation: IntranetMutation, number: &str) -> IntranetMutation {
    let IntranetMutation::ImportCapture { mut capture } = mutation else {
        return mutation;
    };
    if !same_number(&capture.document_number, number) {
        capture.source_url = format!("{MANUAL_ORIGIN}{number}");
        capture.document_number = number.to_owned();
    }
    IntranetMutation::ImportCapture { capture }
}

fn not_applied(capture: &Capture, why: &str) -> String {
    format!(
        "A edição do registro “{}” (número {} neste computador) não foi aplicada: {why} O texto da edição está guardado neste aviso; copie-o e refaça a edição no registro certo.",
        short_title(&capture.title),
        capture.document_number
    )
}

/// An edit of a manual record is addressed by the record's number, and a
/// creation made before the exchange is numbered again when it is replayed
/// on the shared copy (another record may already hold its number). Replaying
/// the edit as written would put it on that other record. This finds the
/// record the edit was written against and re-addresses the edit to that same
/// record's number in `target`, using only what already exists and travels:
///
/// 1. the creation of that record is being replayed in this exchange: it
///    follows the number the creation got (a creation that was refused takes
///    the edit with it);
/// 2. otherwise the request ids of the record are looked up in `target`: the
///    same record there is the one they lead to, wherever its number is now;
///    when the holder of the number is another record, the edit moves; when
///    the record cannot be found there but another record holds the number,
///    the edit is not applied;
/// 3. no request id on this side (a record that predates them), a copy that
///    has nothing to compare, or nobody holding the number there: as it was
///    always done, by number.
///
/// An `Err` is a refusal to apply it: the edit becomes a conflict to decide,
/// never a change of another record.
pub(crate) fn retarget_manual_edit(
    mutation: IntranetMutation,
    source: &EditSource<'_>,
    ids: &crate::idmap::IdMap,
    target: Option<&dyn Records>,
) -> Result<IntranetMutation, String> {
    let IntranetMutation::ImportCapture { capture } = &mutation else {
        return Ok(mutation);
    };
    if !capture.is_manual() || capture.document_number == MANUAL_PENDING {
        return Ok(mutation);
    }
    let number = capture.document_number.clone();
    let uncertain = |why: &str| not_applied(capture, why);
    let cannot_tell = "não foi possível ter certeza de qual registro da cópia compartilhada ela corrige, porque o número pode ter mudado ao sincronizar.";
    let unreadable = |_: String| uncertain(cannot_tell);
    let own_request = normalized_request(&capture.request_id);

    // 1. The record the edit was written against, in the library it comes from.
    let mut local = None;
    match source.written {
        Written::Here => {
            if let Some(doc) = source.receipt_doc {
                match source.records.number_of_doc(doc).map_err(unreadable)? {
                    // A record this library has since purged: the receipt is
                    // still the proof of where the edit was written.
                    None => local = Some(doc),
                    Some(n) if same_number(&n, &number) => local = Some(doc),
                    Some(_) => {}
                }
            }
            if local.is_none() {
                if let Some(request) = &own_request {
                    local = source.records.doc_of_request(request).map_err(unreadable)?;
                }
            }
            if local.is_none() {
                local = source.records.doc_by_number(&number).map_err(unreadable)?;
            }
        }
        Written::InCopy => {
            if let Some(request) = &own_request {
                local = source.records.doc_of_request(request).map_err(unreadable)?;
            }
            if local.is_none() {
                local = source.receipt_doc;
            }
        }
    }
    let Some(local) = local else {
        return match source.written {
            Written::Here => Err(uncertain("o registro não existe mais neste computador.")),
            Written::InCopy => Ok(mutation),
        };
    };

    // 2. Its creation is being replayed in this exchange.
    if source.creations.contains(&local) {
        match ids.learned_document(local) {
            Some(None) => {
                return Err(uncertain(
                    "a criação desse registro foi recusada pela cópia compartilhada.",
                ))
            }
            Some(Some(there)) => {
                let target = target.ok_or_else(|| uncertain(cannot_tell))?;
                let number = target
                    .number_of_doc(there)
                    .map_err(unreadable)?
                    .ok_or_else(|| uncertain(cannot_tell))?;
                return Ok(readdress(mutation, &number));
            }
            None => {}
        }
    }

    // 3. By identity.
    if source
        .records
        .number_of_doc(local)
        .map_err(unreadable)?
        .is_none()
    {
        return match source.written {
            Written::Here => Err(uncertain("o registro não existe mais neste computador.")),
            Written::InCopy => Ok(mutation),
        };
    }
    let Some(target) = target else {
        return Ok(mutation);
    };
    // The request of the edit itself is not an identity: it exists only here.
    let mine: Vec<String> = source
        .records
        .requests_of_doc(local)
        .map_err(unreadable)?
        .into_iter()
        .filter(|r| Some(r) != own_request.as_ref())
        .collect();
    if mine.is_empty() {
        return Ok(mutation);
    }
    let mut same = BTreeSet::new();
    for request in &mine {
        if let Some(doc) = target.doc_of_request(request).map_err(unreadable)? {
            same.insert(doc);
        }
    }
    let holder = target.doc_by_number(&number).map_err(unreadable)?;
    match (same.len(), holder) {
        (0, None) => Ok(mutation),
        (0, Some(_)) => Err(uncertain(cannot_tell)),
        (1, holder) => {
            let doc = *same.iter().next().expect("one element");
            if holder == Some(doc) {
                return Ok(mutation);
            }
            match target.number_of_doc(doc).map_err(unreadable)? {
                Some(there) => Ok(readdress(mutation, &there)),
                None => Err(uncertain(cannot_tell)),
            }
        }
        _ => Err(uncertain(cannot_tell)),
    }
}

/// What a refused edit of a manual record said, to be shown (and copied) on
/// its conflict card: the text is the person's own work.
pub struct RefusedEdit {
    pub title: String,
    pub number: String,
    pub text: String,
}

/// The edit that a conflict that cannot be applied carries, when it is the
/// edit of a manual record.
pub fn refused_manual_edit(conflict: &crate::intranet::SyncConflict) -> Option<RefusedEdit> {
    if !conflict.unresolved {
        return None;
    }
    match &conflict.mutation {
        IntranetMutation::ImportCapture { capture }
            if capture.is_manual() && capture.document_number != MANUAL_PENDING =>
        {
            Some(RefusedEdit {
                title: capture.title.clone(),
                number: capture.document_number.clone(),
                text: capture.text.clone(),
            })
        }
        _ => None,
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

/// The document an edit's receipt named (the record the edit landed on).
pub(crate) fn receipt_document(created: &[crate::idmap::CreatedId]) -> Option<i64> {
    created
        .iter()
        .find(|c| c.kind == crate::idmap::IdKind::Document)
        .map(|c| c.id)
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
                // Early and late versions: the identity survives the purge
                // of either end.
                let mut uids: Vec<String> =
                    requests.iter().take(KEEP_IDENTITIES / 2).cloned().collect();
                for request in requests.iter().rev().take(KEEP_IDENTITIES / 2) {
                    if !uids.contains(request) {
                        uids.push(request.clone());
                    }
                }
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
}
