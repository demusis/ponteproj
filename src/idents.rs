//! Identities of the items (library schema 22, F2a of Frente F).
//!
//! Every row of the twelve kinds that are addressed by row id gets a random
//! `ident` (`item_ident`), given by a trigger in the same statement that
//! inserts the row and removed with it, so no path that writes rows can
//! forget it. The current note and the current analysis of each document
//! are kept in `current_pointer` by triggers too (the rule of today: the
//! newest note by number; the analysis made last by its own time, as
//! `storage::current_analysis` chooses it). `ident_state` will say where
//! the identities came from; nothing writes it yet.
//!
//! Each connection also logs, in TEMP tables of its own, the rows it
//! inserts while a log is open ([`start_log`]): a change applied here
//! records the identities of the rows IT inserted, by slot
//! ([`created_idents`]), and a replay of that change elsewhere gives them to
//! the rows the replay inserted for the same slots ([`rekey`]). A row a
//! change only finds (an import of a known document) is never given an
//! identity from elsewhere.
//!
//! Nothing reads the identities yet: every query still addresses rows by
//! number, and the shared file and the team protocol do not carry them.
use crate::idmap::{ConnFrame, IdKind};
use crate::intranet::{IntranetMutation, MutationReply};
use crate::storage::Store;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// The ident kinds and their tables.
pub(crate) const KINDS: [(&str, &str); 12] = [
    ("document", "documents"),
    ("capture", "captures"),
    ("note", "notes"),
    ("draft", "drafts"),
    ("analysis", "analyses"),
    ("file", "files"),
    ("link", "document_links"),
    ("task", "tasks"),
    ("attachment", "attachments"),
    ("saiop", "saiop_attendances"),
    ("saiop_batch", "saiop_batches"),
    ("example", "template_examples"),
];

/// The identity of a row a change inserted, named by its slot.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SlotIdent {
    pub kind: String,
    pub slot: String,
    pub ident: String,
}

/// One item's identity as the shared file of format 9 carries it:
/// `["document",12,"<32 hex>"]` (kind, row id, identity).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IdentRow(pub String, pub i64, pub String);

/// What a reader says when the identities of a copy do not check.
fn broken(detail: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(
        "A identidade dos itens desta cópia não confere ({detail}). Nada dela foi usado; sincronize de novo, ou a partir de outro computador."
    )
}

/// The rows a snapshot projects, as (ident kind, row id): the rows whose
/// identity it must carry (one triple each), sorted as the triples are.
pub fn projected_rows(
    snapshot: &crate::intranet::CabinetSnapshot,
) -> std::collections::BTreeSet<(String, i64)> {
    let mut rows = std::collections::BTreeSet::new();
    let mut add = |kind: &str, ids: &mut dyn Iterator<Item = i64>| {
        for id in ids {
            rows.insert((kind.to_owned(), id));
        }
    };
    add("document", &mut snapshot.documents.iter().map(|r| r.id));
    add("capture", &mut snapshot.captures.iter().map(|r| r.id));
    add("note", &mut snapshot.notes.iter().map(|r| r.id));
    add("draft", &mut snapshot.drafts.iter().map(|r| r.id));
    add("analysis", &mut snapshot.analyses.iter().map(|r| r.id));
    add("file", &mut snapshot.files_meta.iter().map(|r| r.id));
    add("link", &mut snapshot.document_links.iter().map(|r| r.id));
    add("task", &mut snapshot.tasks.iter().map(|r| r.id));
    add("attachment", &mut snapshot.attachments.iter().map(|r| r.id));
    add(
        "saiop",
        &mut snapshot.saiop_attendances.iter().map(|r| r.attendance.id),
    );
    add(
        "saiop_batch",
        &mut snapshot.saiop_batches.iter().map(|r| r.id),
    );
    add(
        "example",
        &mut snapshot.templates.examples.iter().map(|r| r.id),
    );
    rows
}

fn is_ident(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Checks the identities a snapshot carries (design §3.1), on the very
/// object a writer is about to publish and on every snapshot of format 9 or
/// of the team protocol V2 before it is used:
///
/// - format 9 ⇔ schema ≥ 22 ⇔ a non-empty epoch (`header` of a shared file;
///   `None` for a snapshot of the team protocol V2, which always carries an
///   epoch);
/// - triples in strict order (kind, then row), so no duplicates;
/// - known kinds, 32 lowercase hex digits, no identity twice;
/// - exactly one triple per row projected (zero rows, zero triples).
pub fn check(
    snapshot: &crate::intranet::CabinetSnapshot,
    header: Option<&crate::sharedfolder::Header>,
) -> Result<()> {
    let nine = match header {
        Some(h) => {
            let nine = h.format >= crate::sharedfolder::FORMAT_9;
            if nine != (h.schema_version >= crate::storage::LIBRARY_SCHEMA) {
                return Err(broken("formato e esquema não combinam"));
            }
            nine
        }
        // A snapshot of the team protocol V2.
        None => true,
    };
    if nine == snapshot.ident_epoch.is_empty() {
        return Err(broken("época ausente ou fora do lugar"));
    }
    if !nine {
        if !snapshot.idents.is_empty() {
            return Err(broken("identidades numa cópia do formato anterior"));
        }
        return Ok(());
    }
    check_triples(snapshot)
}

/// The triples of [`check`] alone (a portable copy may carry them without
/// an epoch).
pub fn check_triples(snapshot: &crate::intranet::CabinetSnapshot) -> Result<()> {
    let mut seen = std::collections::HashSet::with_capacity(snapshot.idents.len());
    for (index, IdentRow(kind, row, ident)) in snapshot.idents.iter().enumerate() {
        if !KINDS.iter().any(|(k, _)| k == kind) {
            return Err(broken(format!("tipo desconhecido «{kind}»")));
        }
        if !is_ident(ident) {
            return Err(broken(format!("identidade malformada em {kind} {row}")));
        }
        if !seen.insert(ident.as_str()) {
            return Err(broken(format!("identidade repetida em {kind} {row}")));
        }
        if index > 0 {
            let IdentRow(pk, pr, _) = &snapshot.idents[index - 1];
            if (pk.as_bytes(), *pr) >= (kind.as_bytes(), *row) {
                return Err(broken("triplas fora de ordem ou repetidas"));
            }
        }
    }
    let rows = projected_rows(snapshot);
    if rows.len() != snapshot.idents.len()
        || !snapshot
            .idents
            .iter()
            .all(|IdentRow(kind, row, _)| rows.contains(&(kind.clone(), *row)))
    {
        return Err(broken(format!(
            "{} itens e {} identidades",
            rows.len(),
            snapshot.idents.len()
        )));
    }
    Ok(())
}

/// Whether this library has the tables of schema 22 (a library left at 21
/// for an opening has none).
pub(crate) fn has_idents(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='item_ident')",
        [],
        |r| r.get(0),
    )?)
}

/// Fills `snapshot.idents` with the identity of every row it projects (and
/// only those: an unreadable trashed task that the exporter left out has
/// none), sorted, and `snapshot.ident_epoch` with `epoch`.
pub(crate) fn attach(
    conn: &Connection,
    snapshot: &mut crate::intranet::CabinetSnapshot,
    epoch: &str,
) -> Result<()> {
    let mut all: std::collections::HashMap<(String, i64), String> = Default::default();
    {
        let mut statement = conn.prepare("SELECT kind,row_id,ident FROM item_ident")?;
        let rows = statement.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?;
        for row in rows {
            let (key, ident) = row?;
            all.insert(key, ident);
        }
    }
    let mut triples = Vec::new();
    for (kind, row) in projected_rows(snapshot) {
        let ident = all
            .remove(&(kind.clone(), row))
            .ok_or_else(|| broken(format!("{kind} {row} sem identidade")))?;
        triples.push(IdentRow(kind, row, ident));
    }
    #[cfg(any(test, feature = "test-support"))]
    if hooks::DROP_TRIPLE.with(|c| c.replace(false)) && !triples.is_empty() {
        triples.remove(0);
    }
    snapshot.idents = triples;
    snapshot.ident_epoch = epoch.to_owned();
    Ok(())
}

impl Store {
    /// [`Store::export_snapshot`] with the identity of every row it projects
    /// and `epoch`, read in one state of the library.
    pub fn export_with_idents(&self, epoch: &str) -> Result<crate::intranet::CabinetSnapshot> {
        let read = if self.conn.is_autocommit() {
            Some(self.conn.unchecked_transaction()?)
        } else {
            None
        };
        let mut snapshot = self.export_snapshot()?;
        attach(&self.conn, &mut snapshot, epoch)?;
        drop(read);
        Ok(snapshot)
    }
}

/// The identity a folder promotion gives row `row` of kind `kind`: the
/// same on every computer that cuts the same result (`lineage`, `epoch`).
pub fn stamped_ident(lineage: &str, epoch: &str, kind: &str, row: i64) -> String {
    let mut input = Vec::with_capacity(128);
    for part in [
        "ponte-ident-v1",
        "leg",
        lineage,
        epoch,
        kind,
        &row.to_string(),
    ] {
        input.extend_from_slice(part.as_bytes());
        input.push(0);
    }
    let digest = ring::digest::digest(&ring::digest::SHA256, &input);
    digest.as_ref()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Gives every row of the library (the temporary library of a promotion)
/// its [`stamped_ident`].
pub(crate) fn stamp(conn: &Connection, lineage: &str, epoch: &str) -> Result<()> {
    let rows: Vec<(String, i64)> = conn
        .prepare("SELECT kind,row_id FROM item_ident")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut update =
            tx.prepare("UPDATE item_ident SET ident=?1 WHERE kind=?2 AND row_id=?3")?;
        for (kind, row) in rows {
            update.execute(params![
                stamped_ident(lineage, epoch, &kind, row),
                kind,
                row
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Inside the adoption of a snapshot: every row it put in place takes the
/// identity the snapshot carries for it.
pub(crate) fn adopt_triples(
    tx: &Connection,
    snapshot: &crate::intranet::CabinetSnapshot,
) -> Result<()> {
    if snapshot.idents.is_empty() || !has_idents(tx)? {
        return Ok(());
    }
    // The rows were all inserted by this adoption (with random identities):
    // they are set aside first, so no carried identity meets a random one.
    tx.execute_batch("UPDATE item_ident SET ident='-'||kind||'-'||row_id;")?;
    let mut update = tx.prepare("UPDATE item_ident SET ident=?1 WHERE kind=?2 AND row_id=?3")?;
    for IdentRow(kind, row, ident) in &snapshot.idents {
        update.execute(params![ident, kind, row])?;
    }
    drop(update);
    // A row the snapshot carries no identity for (none, when it checks)
    // gets a new random one again.
    tx.execute_batch(
        "UPDATE item_ident SET ident=lower(hex(randomblob(16))) WHERE ident LIKE '-%';",
    )?;
    Ok(())
}

/// Records where the identities of this library come from.
pub(crate) fn set_origin(
    conn: &Connection,
    origin: &str,
    lineage: Option<&str>,
    epoch: Option<&str>,
) -> Result<()> {
    if !has_idents(conn)? {
        return Ok(());
    }
    conn.execute(
        "INSERT OR REPLACE INTO ident_state(id,origin,lineage,epoch) VALUES(1,?1,?2,?3)",
        params![origin, lineage, epoch],
    )?;
    Ok(())
}

/// The epoch of the identities a main computer serves (team protocol V2):
/// the one its library already has, or, the first time it serves V2 with
/// provisional identities, a new random nonce kept in the library
/// (`host(nonce)`), the identities themselves unchanged. A library put back
/// from a backup of schema 21 has none and gets a new one.
pub(crate) fn host_epoch(conn: &Connection) -> Result<String> {
    if !has_idents(conn)? {
        return Ok(String::new());
    }
    let (origin, _, epoch) = origin(conn)?;
    if origin != "provisional" {
        if let Some(epoch) = epoch.filter(|e| !e.is_empty()) {
            return Ok(epoch);
        }
    }
    let nonce: String = crate::sharedfolder::random(16)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    set_origin(conn, "host", None, Some(&nonce))?;
    Ok(nonce)
}

/// Where the identities of this library come from (`ident_state`): the
/// origin (`provisional` when nothing says otherwise), the lineage and the
/// epoch.
pub fn origin(conn: &Connection) -> Result<(String, Option<String>, Option<String>)> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ident_state')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(("provisional".into(), None, None));
    }
    Ok(conn
        .query_row(
            "SELECT origin,lineage,epoch FROM ident_state WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .unwrap_or_else(|| ("provisional".into(), None, None)))
}

/// The rows a logged execution inserted, (ident kind, row id), with the
/// identity each was given in the write that created it.
pub(crate) type Inserted = std::collections::BTreeMap<(String, i64), String>;

/// What a replay did with the identities its change carried.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rekeyed {
    /// Carried identities now held by the row this replay inserted for
    /// their slot, with that row.
    pub applied: Vec<(SlotIdent, i64)>,
    /// Carried identities already held by another row (left as they were).
    pub skipped: usize,
}

/// The identity of row `row` of kind `kind`.
pub(crate) fn ident_of(conn: &Connection, kind: &str, row: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT ident FROM item_ident WHERE kind=?1 AND row_id=?2",
            rusqlite::params![kind, row],
            |r| r.get(0),
        )
        .optional()?)
}

/// The tables of schema 22. Created inside the 21→22 transaction (a
/// leftover of an interrupted, foreign attempt is replaced: a library of
/// schema 21 has none of them).
pub(crate) const TABLES_SQL: &str = "
DROP TABLE IF EXISTS item_ident;
DROP TABLE IF EXISTS current_pointer;
DROP TABLE IF EXISTS ident_state;
CREATE TABLE item_ident(kind TEXT NOT NULL, row_id INTEGER NOT NULL, ident TEXT NOT NULL,
  PRIMARY KEY(kind,row_id)) WITHOUT ROWID;
CREATE UNIQUE INDEX item_ident_ident ON item_ident(ident);
CREATE TABLE current_pointer(document_id INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
  note_id INTEGER, analysis_id INTEGER);
CREATE TABLE ident_state(id INTEGER PRIMARY KEY CHECK(id=1), origin TEXT NOT NULL, lineage TEXT, epoch TEXT);
";

/// A new random identity: 32 lowercase hex digits.
const RANDOM_IDENT: &str = "lower(hex(randomblob(16)))";

/// The moment an analysis was made, in microseconds since 1970: what
/// `storage::current_analysis` orders by, and (as the SQL function
/// `ponte_analysis_micros`) what the triggers of `current_pointer` order by.
/// ONE converter for both: an RFC 3339 time as `chrono` reads it (any offset
/// it accepts, a leap second included), or `None` when it does not read as
/// one.
pub(crate) fn analysis_micros(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|t| t.timestamp_micros())
}

/// The SQL functions the triggers of schema 22 call, registered on every
/// connection of this build that writes (`Store::open`). A connection of
/// an older Ponte does not have them: its writes that would fire those
/// triggers are refused when they are prepared ("no such function"), as its
/// guard would refuse them.
pub(crate) fn register_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "ponte_analysis_micros",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
            | rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
        |ctx| {
            Ok(match ctx.get_raw(0) {
                rusqlite::types::ValueRef::Text(text) => {
                    std::str::from_utf8(text).ok().and_then(analysis_micros)
                }
                _ => None,
            })
        },
    )?;
    Ok(())
}

/// [`analysis_micros`] of `column` (an SQL expression).
pub(crate) fn analysis_time(column: &str) -> String {
    format!("ponte_analysis_micros({column})")
}

/// The current analysis of the document `document` (an SQL expression), as
/// `storage::current_analysis_row`: of the analyses of its current version
/// (`documents.latest_capture_id`), the one made last by its own time,
/// unreadable times first, ties by number; only when the current version
/// has none, the same choice among all of them.
fn current_analysis_sql(document: &str) -> String {
    let at = analysis_time("a.created_at");
    format!(
        "(SELECT a.id FROM analyses a WHERE a.document_id={document} \
         ORDER BY coalesce(a.capture_id=(SELECT d.latest_capture_id FROM documents d WHERE d.id={document}),0) DESC, \
         {at} IS NULL, {at} DESC, a.id DESC LIMIT 1)"
    )
}

/// Every row of the twelve kinds gets an identity, and every document its
/// pointers (the documents in the Lixeira included: `documents` itself is
/// never written, so its trash guard is never in the way).
pub(crate) fn backfill_sql() -> String {
    let mut sql = String::new();
    for (kind, table) in KINDS {
        sql.push_str(&format!(
            "INSERT INTO item_ident(kind,row_id,ident) SELECT '{kind}',id,{RANDOM_IDENT} FROM {table};\n"
        ));
    }
    sql.push_str(&format!(
        "INSERT INTO current_pointer(document_id,note_id,analysis_id) SELECT d.id,(SELECT max(n.id) FROM notes n WHERE n.document_id=d.id),{} FROM documents d;\n",
        current_analysis_sql("d.id")
    ));
    sql
}

/// The triggers of schema 22 that keep `item_ident` and `current_pointer`,
/// as (name, `CREATE TRIGGER` text). They are part of the guard set of the
/// library (`Store::expected_guards`): written in the 21→22 transaction and
/// put back by the repair at every opening.
pub(crate) fn triggers() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (kind, table) in KINDS {
        // The delete before the insert makes a leftover impossible; an
        // identity that already exists elsewhere (never, in practice: 128
        // random bits) aborts the statement instead of being taken over.
        let name = format!("ponte_ident_{table}_insert");
        out.push((name.clone(), format!(
            "CREATE TRIGGER {name} AFTER INSERT ON {table} BEGIN DELETE FROM item_ident WHERE kind='{kind}' AND row_id=NEW.id; INSERT INTO item_ident(kind,row_id,ident) VALUES('{kind}',NEW.id,{RANDOM_IDENT}); END;"
        )));
        let name = format!("ponte_ident_{table}_delete");
        out.push((name.clone(), format!(
            "CREATE TRIGGER {name} AFTER DELETE ON {table} BEGIN DELETE FROM item_ident WHERE kind='{kind}' AND row_id=OLD.id; END;"
        )));
        let name = format!("ponte_ident_{table}_renumber");
        out.push((name.clone(), format!(
            "CREATE TRIGGER {name} AFTER UPDATE OF id ON {table} WHEN NEW.id<>OLD.id BEGIN UPDATE item_ident SET row_id=NEW.id WHERE kind='{kind}' AND row_id=OLD.id; END;"
        )));
    }
    let pointer_of =
        |doc: &str| format!("INSERT OR IGNORE INTO current_pointer(document_id) VALUES({doc});");
    let push = |out: &mut Vec<(String, String)>, name: &str, body: String| {
        out.push((name.to_owned(), format!("CREATE TRIGGER {name} {body}")));
    };
    push(
        &mut out,
        "ponte_pointer_documents_insert",
        format!(
            "AFTER INSERT ON documents BEGIN {} END;",
            pointer_of("NEW.id")
        ),
    );
    let newest_note =
        |doc: &str| format!("(SELECT max(n.id) FROM notes n WHERE n.document_id={doc})");
    // A document that changes number takes its pointer row along, read
    // again for the new number (its notes and analyses may have moved
    // first, with the foreign keys off).
    push(
        &mut out,
        "ponte_pointer_documents_renumber",
        format!(
            "AFTER UPDATE OF id ON documents WHEN NEW.id<>OLD.id BEGIN UPDATE OR REPLACE current_pointer SET document_id=NEW.id WHERE document_id=OLD.id; {} UPDATE current_pointer SET note_id={}, analysis_id={} WHERE document_id=NEW.id; END;",
            pointer_of("NEW.id"),
            newest_note("NEW.id"),
            current_analysis_sql("NEW.id")
        ),
    );
    push(
        &mut out,
        "ponte_pointer_notes_insert",
        format!(
            "AFTER INSERT ON notes BEGIN {} UPDATE current_pointer SET note_id=NEW.id WHERE document_id=NEW.document_id AND (note_id IS NULL OR note_id<NEW.id); END;",
            pointer_of("NEW.document_id")
        ),
    );
    push(
        &mut out,
        "ponte_pointer_notes_delete",
        format!(
            "AFTER DELETE ON notes BEGIN UPDATE current_pointer SET note_id={} WHERE document_id=OLD.document_id AND note_id=OLD.id; END;",
            newest_note("OLD.document_id")
        ),
    );
    push(
        &mut out,
        "ponte_pointer_notes_update",
        format!(
            "AFTER UPDATE OF id,document_id ON notes BEGIN UPDATE current_pointer SET note_id={} WHERE document_id IN (OLD.document_id,NEW.document_id); END;",
            newest_note("current_pointer.document_id")
        ),
    );
    push(
        &mut out,
        "ponte_pointer_analyses_insert",
        format!(
            "AFTER INSERT ON analyses BEGIN {} UPDATE current_pointer SET analysis_id={} WHERE document_id=NEW.document_id; END;",
            pointer_of("NEW.document_id"),
            current_analysis_sql("NEW.document_id")
        ),
    );
    push(
        &mut out,
        "ponte_pointer_analyses_delete",
        format!(
            "AFTER DELETE ON analyses BEGIN UPDATE current_pointer SET analysis_id={} WHERE document_id=OLD.document_id AND analysis_id=OLD.id; END;",
            current_analysis_sql("OLD.document_id")
        ),
    );
    push(
        &mut out,
        "ponte_pointer_analyses_update",
        format!(
            "AFTER UPDATE OF id,document_id,capture_id,created_at ON analyses BEGIN UPDATE current_pointer SET analysis_id={} WHERE document_id IN (OLD.document_id,NEW.document_id); END;",
            current_analysis_sql("current_pointer.document_id")
        ),
    );
    // A new current version (an import, or the current one sent to the
    // Lixeira) can change the current analysis. Only `current_pointer` is
    // written: `documents` keeps its own guards untouched.
    push(
        &mut out,
        "ponte_pointer_documents_version",
        format!(
            "AFTER UPDATE OF latest_capture_id ON documents WHEN NEW.latest_capture_id IS NOT OLD.latest_capture_id BEGIN UPDATE current_pointer SET analysis_id={} WHERE document_id=NEW.id; END;",
            current_analysis_sql("NEW.id")
        ),
    );
    out
}

/// The log of the rows this connection inserts: a TEMP table, seen by this
/// connection only (another process, or another connection of this one,
/// never writes to it). Created at every opening.
pub(crate) fn install_insert_log(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS ponte_inserted(kind TEXT NOT NULL, row_id INTEGER NOT NULL, ident TEXT NOT NULL);",
    )?;
    Ok(())
}

/// The TEMP trigger that fills the log. It sits on `item_ident`: every row
/// inserted into one of the twelve tables gets its identity there in the
/// same statement, so the log records the row AND the identity it was given
/// in the very write that created it (read back later, after the commit,
/// another connection may have replaced the row under the same number). It
/// exists only while a log is open: a TEMP trigger on a main table that
/// stayed for the life of the connection would keep it from renaming tables
/// whenever another connection had renamed that table away (SQLite checks
/// it and cannot even drop it then).
const LOG_TRIGGER: &str = "CREATE TEMP TRIGGER IF NOT EXISTS ponte_log_item_ident AFTER INSERT ON main.item_ident \
     BEGIN INSERT INTO temp.ponte_inserted(kind,row_id,ident) VALUES(NEW.kind,NEW.row_id,NEW.ident); END;";

/// Whether this connection keeps the log: only on a library of schema 22
/// (`install_insert_log` ran). A library left at 21 for this opening (its
/// update could not be done, see `Store::upgrade_notice`) works as before
/// F2a: nothing is logged and nothing is carried.
fn log_kept(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE type='table' AND name='ponte_inserted')",
        [],
        |r| r.get(0),
    )?)
}

/// Opens the log of this connection, empty. Whoever opens it closes it
/// ([`stop_log`]) on every way out that does not roll back; inside a
/// transaction, a rollback closes it too.
pub(crate) fn start_log(conn: &Connection) -> Result<()> {
    if !log_kept(conn)? {
        return Ok(());
    }
    conn.execute_batch(&format!("DELETE FROM temp.ponte_inserted;\n{LOG_TRIGGER}"))?;
    Ok(())
}

/// The rows inserted since [`start_log`], with the identity each was given.
pub(crate) fn logged(conn: &Connection) -> Result<Inserted> {
    if !log_kept(conn)? {
        return Ok(Inserted::new());
    }
    let mut statement = conn.prepare("SELECT kind,row_id,ident FROM temp.ponte_inserted")?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, i64>(1)?),
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Inserted>>()?;
    Ok(rows)
}

/// Closes the log and empties it.
pub(crate) fn stop_log(conn: &Connection) -> Result<()> {
    if !log_kept(conn)? {
        return Ok(());
    }
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS temp.ponte_log_item_ident; DELETE FROM temp.ponte_inserted;",
    )
    .map_err(|error| {
        anyhow::anyhow!("Não foi possível fechar o registro das linhas inseridas ({error}).")
    })
}

/// The ident kind of an id-map kind.
fn kind_of(kind: IdKind) -> &'static str {
    match kind {
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

/// The row ids the reply itself names: the rows of these slots are the ones
/// this execution wrote, whatever happened to the library since.
fn reply_rows(reply: &MutationReply) -> Vec<(&'static str, i64)> {
    match reply {
        MutationReply::Receipt(receipt) => vec![
            ("document", receipt.document_id),
            ("capture", receipt.capture_id),
        ],
        MutationReply::SaiopBatch(batch) => vec![("saiop_batch", batch.id)],
        _ => Vec::new(),
    }
}

/// The rows `mutation`, answered with `reply`, inserted for each of its
/// slots, as (kind, slot, row, identity given at the insert): the slots of
/// `idmap::created_ids` whose row is in the log, plus `note` for a saved
/// note and `analysis` for a saved analysis (the row of that document this
/// very execution inserted, never the newest of the table).
///
/// The identity is the one the log recorded in the write that created the
/// row, never the one the row holds now. A slot found by a lookup in the
/// library as it is now (the files of a version, the triage task, the
/// atendimento of an import, a link) counts only when its row still holds
/// that identity: the row is then the very row this execution created, not
/// another one that took its number since. A slot named by the reply itself
/// is the row this execution wrote.
fn slot_rows(
    conn: &Connection,
    mutation: &IntranetMutation,
    reply: &MutationReply,
    inserted: &Inserted,
) -> Result<Vec<(&'static str, String, i64, String)>> {
    let mut out = Vec::new();
    if inserted.is_empty() {
        return Ok(out);
    }
    let named = reply_rows(reply);
    let unchanged = |kind: &str, row: i64, ident: &str| -> Result<bool> {
        Ok(ident_of(conn, kind, row)?.as_deref() == Some(ident))
    };
    for created in crate::idmap::created_ids(&ConnFrame(conn), mutation, reply)? {
        let kind = kind_of(created.kind);
        let Some(ident) = inserted.get(&(kind.to_owned(), created.id)) else {
            continue;
        };
        let from_reply = named.contains(&(kind, created.id))
            || (created.slot.is_empty()
                && matches!(reply, MutationReply::Id(id) if *id == created.id));
        if from_reply || unchanged(kind, created.id, ident)? {
            out.push((kind, created.slot, created.id, ident.clone()));
        }
    }
    let own = |kind: &'static str, table: &str, document: i64| -> Result<Option<(i64, String)>> {
        let mut newest: Option<(i64, String)> = None;
        for ((_, row), ident) in inserted.iter().filter(|((k, _), _)| k == kind) {
            let of: Option<i64> = conn
                .query_row(
                    &format!("SELECT document_id FROM {table} WHERE id=?1"),
                    [row],
                    |r| r.get(0),
                )
                .optional()?;
            if of == Some(document)
                && unchanged(kind, *row, ident)?
                && newest.as_ref().is_none_or(|(n, _)| row > n)
            {
                newest = Some((*row, ident.clone()));
            }
        }
        Ok(newest)
    };
    match mutation {
        IntranetMutation::SaveNote { document_id, .. } => {
            if let Some((row, ident)) = own("note", "notes", *document_id)? {
                out.push(("note", "note".into(), row, ident));
            }
        }
        IntranetMutation::SaveAnalysis { document_id, .. } => {
            if let Some((row, ident)) = own("analysis", "analyses", *document_id)? {
                out.push(("analysis", "analysis".into(), row, ident));
            }
        }
        _ => {}
    }
    Ok(out)
}

/// The identities of the rows `mutation` inserted here, by slot: what a
/// queued change carries (`OutboxEntry::idents`). See [`slot_rows`].
pub(crate) fn created_idents(
    conn: &Connection,
    mutation: &IntranetMutation,
    reply: &MutationReply,
    inserted: &Inserted,
) -> Result<Vec<SlotIdent>> {
    #[cfg(any(test, feature = "test-support"))]
    if hooks::FAIL_CAPTURE.with(|h| h.replace(false)) {
        anyhow::bail!("falha injetada na captura das identidades (teste)");
    }
    Ok(slot_rows(conn, mutation, reply, inserted)?
        .into_iter()
        .map(|(kind, slot, _, ident)| SlotIdent {
            kind: kind.to_owned(),
            slot,
            ident,
        })
        .collect())
}

/// Gives the rows a replay of `mutation` inserted (`inserted`) the
/// identities the change carried, slot by slot. A slot whose row the replay
/// did not insert keeps its own identity; an identity another row already
/// holds is left there and counted.
pub(crate) fn rekey(
    conn: &Connection,
    carried: &[SlotIdent],
    mutation: &IntranetMutation,
    reply: &MutationReply,
    inserted: &Inserted,
) -> Result<Rekeyed> {
    let mut out = Rekeyed::default();
    if carried.is_empty() {
        return Ok(out);
    }
    #[cfg(any(test, feature = "test-support"))]
    if hooks::FAIL_REKEY.with(|h| h.replace(false)) {
        anyhow::bail!("falha injetada na troca de identidades (teste)");
    }
    let rows = slot_rows(conn, mutation, reply, inserted)?;
    for slot in carried {
        let Some((kind, _, row, _)) = rows
            .iter()
            .find(|(kind, name, _, _)| *kind == slot.kind && *name == slot.slot)
        else {
            continue;
        };
        let holder: Option<(String, i64)> = conn
            .query_row(
                "SELECT kind,row_id FROM item_ident WHERE ident=?1",
                [&slot.ident],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match holder {
            Some((k, r)) if k == *kind && r == *row => {}
            Some(_) => {
                out.skipped += 1;
                continue;
            }
            None => {
                conn.execute(
                    "UPDATE item_ident SET ident=?1 WHERE kind=?2 AND row_id=?3",
                    params![slot.ident, kind, row],
                )?;
            }
        }
        out.applied.push((slot.clone(), *row));
    }
    Ok(out)
}

/// What a logged execution gave: the result of the work, the rows it
/// inserted (or why they could not be read) and whether the log closed.
pub(crate) struct Logged<T> {
    pub result: Result<T>,
    pub inserted: Result<Inserted>,
    pub closed: Result<()>,
}

/// A replayed change: applied (with what became of the identities it
/// carried) or refused by the library (its own error, as before).
// What became of the identities is read by the tests only.
#[allow(dead_code)]
pub(crate) enum Carried {
    Applied(MutationReply, Rekeyed),
    Refused(anyhow::Error),
}

impl Store {
    /// Runs `work` with the log of inserted rows open, and closes it again.
    /// An error opening the log is returned before `work` runs (nothing
    /// done); after that, the outcome of the work and of the log are kept
    /// apart, so a change that was applied is never taken for one that was
    /// not.
    pub(crate) fn run_logged<T>(
        &mut self,
        work: impl FnOnce(&mut Store) -> Result<T>,
    ) -> Result<Logged<T>> {
        start_log(&self.conn)?;
        let result = work(self);
        let inserted = logged(&self.conn);
        let closed = stop_log(&self.conn);
        Ok(Logged {
            result,
            inserted,
            closed,
        })
    }

    /// Applies a replayed change (as `execute_mutation`) and gives the rows
    /// it inserted the identities the change carried (see [`rekey`]).
    ///
    /// `Ok(Refused)` is a refusal of the change itself, as `execute_mutation`
    /// gives it. `Err` is a failure of the log or of the identities AFTER the
    /// change may have been applied: the caller must abandon what it was
    /// building (an exchange stops before publishing anything), never record
    /// the change as refused. A change that carries no identity is applied
    /// without the log, as before F2a.
    pub(crate) fn execute_carrying(
        &mut self,
        actor: &str,
        mutation: IntranetMutation,
        carried: &[SlotIdent],
    ) -> Result<Carried> {
        if carried.is_empty() {
            return Ok(match self.execute_mutation(actor, mutation) {
                Ok(reply) => Carried::Applied(reply, Rekeyed::default()),
                Err(error) => Carried::Refused(error),
            });
        }
        let applied = mutation.clone();
        let logged = self.run_logged(|store| store.execute_mutation(actor, mutation))?;
        let reply = match logged.result {
            Ok(reply) => reply,
            Err(error) => {
                logged.closed?;
                return Ok(Carried::Refused(error));
            }
        };
        logged.closed?;
        let inserted = logged.inserted?;
        let rekeyed = rekey(&self.conn, carried, &applied, &reply, &inserted)?;
        Ok(Carried::Applied(reply, rekeyed))
    }
}

/// Failure and pause points for the tests of the migration.
#[cfg(any(test, feature = "test-support"))]
pub mod hooks {
    use std::cell::RefCell;
    type Pause = Box<dyn FnOnce()>;
    type Fault = Box<dyn FnOnce(&std::path::Path) -> anyhow::Result<()>>;
    type Peek = Box<dyn FnMut(&crate::storage::Store)>;
    type PathHook = Box<dyn FnOnce(&std::path::Path)>;
    thread_local! {
        /// Runs once, in `Store::open`, after the `ensure_*` steps and before
        /// the 21→22 step.
        pub static AFTER_ENSURE: RefCell<Option<Pause>> = RefCell::new(None);
        /// Runs once inside the 21→22 transaction, after the guards were
        /// rewritten and before the commit; an error aborts the migration.
        pub static BEFORE_MIGRATION_COMMIT: RefCell<Option<Fault>> = RefCell::new(None);
        /// Sees the temporary library of a folder exchange right before it
        /// is exported.
        pub static PEEK_TEMP: RefCell<Option<Peek>> = RefCell::new(None);
        /// Runs once in `queue_locally`, after the change was applied (and
        /// committed) and before what it created is captured.
        pub static AFTER_LOCAL_COMMIT: RefCell<Option<Pause>> = RefCell::new(None);
        /// Runs once right after the current analysis of a document was
        /// read (`storage::current_analysis_row`).
        pub static AFTER_ANALYSIS_SELECT: RefCell<Option<Pause>> = RefCell::new(None);
        /// The next capture of identities fails once.
        pub static FAIL_CAPTURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// The next rekey fails once.
        pub static FAIL_REKEY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// The date in the name of the copy made before the 21→22 step.
        pub static MIGRATION_STAMP: RefCell<Option<String>> = const { RefCell::new(None) };
        /// Runs once when the copy before the 21→22 step is written and
        /// checked, before it gets its final name.
        pub static MIGRATION_COPY_WRITTEN: RefCell<Option<Pause>> = RefCell::new(None);
        /// What the measure of free space answers (once).
        pub static FREE_SPACE: RefCell<Option<std::io::Result<u64>>> = const { RefCell::new(None) };
        /// Whether a folder exchange of this thread may promote the folder
        /// to format 9. The product always may; the tests of the old
        /// exchange (format 8) keep it off, the tests of F2b turn it on.
        pub static PROMOTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// Runs once in a promotion, after the files of format 9 were
        /// written (and the floor raised) and before the stubs and the
        /// local adoption.
        pub static AFTER_NINE_WRITTEN: RefCell<Option<Pause>> = RefCell::new(None);
        /// The next promotion stops (as a crash would) right after the
        /// files of format 9 were written.
        pub static CRASH_AFTER_NINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// Runs once right before a transport file is renamed into place,
        /// with its path.
        pub static BEFORE_RENAME: RefCell<Option<PathHook>> = RefCell::new(None);
        /// Runs once in a folder exchange, right before the folders are
        /// written.
        pub static BEFORE_FOLDER_WRITE: RefCell<Option<Pause>> = RefCell::new(None);
        /// Runs once in a folder exchange, right before the local adoption.
        pub static BEFORE_ADOPTION: RefCell<Option<Pause>> = RefCell::new(None);
        /// The next export of identities leaves one triple out.
        pub static DROP_TRIPLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// The library file of a main computer whose next answer to a change
    /// carries a snapshot with a broken identity (its server runs on
    /// another thread).
    pub static CORRUPT_MUTATED: std::sync::Mutex<Option<std::path::PathBuf>> =
        std::sync::Mutex::new(None);
}

/// Helpers for tests of this crate and of the phone core.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::KINDS;
    use anyhow::{Context, Result};
    use rusqlite::Connection;
    use std::path::Path;

    /// `sqlite_master` of a fresh library of schema 21 (Ponte 0.51), as
    /// (type, name, table, sql), dumped from that build.
    const SCHEMA_21: &str = include_str!("../tests/fixtures/esquema-acervo-21.json");

    /// Every identity of a library, by (kind, row).
    pub fn all_idents(
        store: &crate::storage::Store,
    ) -> std::collections::BTreeMap<(String, i64), String> {
        store
            .conn
            .prepare("SELECT kind,row_id,ident FROM item_ident")
            .and_then(|mut s| {
                s.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn schema_21() -> Vec<(String, String, String, Option<String>)> {
        serde_json::from_str(SCHEMA_21).expect("esquema-acervo-21.json")
    }

    fn raw(path: &Path) -> Result<Connection> {
        let conn = Connection::open(path).context("abrir o acervo")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.create_scalar_function(
            "ponte_client_schema",
            0,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
                | rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
            |_| Ok(99i64),
        )?;
        Ok(conn)
    }

    /// Turns a library of this build back into what Ponte 0.51 (schema 21)
    /// writes: the F2a tables go, every trigger is replaced by the triggers
    /// of [`schema_21`], and `user_version` is 21. The rows are kept.
    pub fn downgrade_to_21(path: &Path) -> Result<()> {
        let conn = raw(path)?;
        let tx = conn.unchecked_transaction()?;
        let triggers: Vec<String> = tx
            .prepare("SELECT name FROM sqlite_master WHERE type='trigger'")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for name in triggers {
            tx.execute_batch(&format!("DROP TRIGGER \"{name}\";"))?;
        }
        tx.execute_batch(
            "DROP TABLE IF EXISTS item_ident; DROP TABLE IF EXISTS current_pointer; DROP TABLE IF EXISTS ident_state;",
        )?;
        for (kind, _, _, sql) in schema_21() {
            if kind == "trigger" {
                tx.execute_batch(&sql.expect("trigger sql"))?;
            }
        }
        tx.execute_batch("PRAGMA user_version=21;")?;
        tx.commit()?;
        Ok(())
    }

    pub fn user_version(path: &Path) -> Result<i64> {
        Ok(raw(path)?.pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// The current analysis of a document as the product chooses it, by the
    /// product's own read (`storage::current_analysis_row`, with the
    /// document's current version).
    pub fn current_analysis(conn: &Connection, document: i64) -> Result<Option<i64>> {
        let current: Option<i64> = conn.query_row(
            "SELECT latest_capture_id FROM documents WHERE id=?1",
            [document],
            |r| r.get(0),
        )?;
        Ok(
            crate::storage::current_analysis_row(conn, document, current.unwrap_or(0))?
                .map(|(id, _)| id),
        )
    }

    /// Every way the library breaks the F2a invariants, in words: each row
    /// of the twelve kinds has exactly one well-formed identity and no
    /// identity names a missing row; each document has one pointer row,
    /// whose note is the newest note (`max(id)`) and whose analysis is the
    /// current analysis; no pointer row names a missing document.
    pub fn invariant_problems(conn: &Connection) -> Result<Vec<String>> {
        let mut problems = Vec::new();
        let count = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
        for (kind, table) in KINDS {
            let missing = count(&format!(
                "SELECT count(*) FROM {table} t WHERE NOT EXISTS(SELECT 1 FROM item_ident i WHERE i.kind='{kind}' AND i.row_id=t.id)"
            ))?;
            let extra = count(&format!(
                "SELECT count(*) FROM item_ident i WHERE i.kind='{kind}' AND NOT EXISTS(SELECT 1 FROM {table} t WHERE t.id=i.row_id)"
            ))?;
            if missing > 0 {
                problems.push(format!("{missing} linha(s) de {table} sem identidade"));
            }
            if extra > 0 {
                problems.push(format!("{extra} identidade(s) de {kind} sem linha"));
            }
        }
        let known: Vec<String> = KINDS.iter().map(|(k, _)| format!("'{k}'")).collect();
        let unknown = count(&format!(
            "SELECT count(*) FROM item_ident WHERE kind NOT IN ({})",
            known.join(",")
        ))?;
        if unknown > 0 {
            problems.push(format!("{unknown} identidade(s) de tipo desconhecido"));
        }
        let malformed = count(
            "SELECT count(*) FROM item_ident WHERE length(ident)<>32 OR ident GLOB '*[^0-9a-f]*'",
        )?;
        if malformed > 0 {
            problems.push(format!("{malformed} identidade(s) mal formada(s)"));
        }
        let orphans = count(
            "SELECT count(*) FROM current_pointer p WHERE NOT EXISTS(SELECT 1 FROM documents d WHERE d.id=p.document_id)",
        )?;
        if orphans > 0 {
            problems.push(format!("{orphans} ponteiro(s) de documento inexistente"));
        }
        let documents: Vec<i64> = conn
            .prepare("SELECT id FROM documents ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for doc in documents {
            let pointer: Option<(Option<i64>, Option<i64>)> =
                rusqlite::OptionalExtension::optional(conn.query_row(
                    "SELECT note_id,analysis_id FROM current_pointer WHERE document_id=?1",
                    [doc],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ))?;
            let Some((note, analysis)) = pointer else {
                problems.push(format!("documento {doc} sem ponteiro"));
                continue;
            };
            let newest_note: Option<i64> = conn.query_row(
                "SELECT max(id) FROM notes WHERE document_id=?1",
                [doc],
                |r| r.get(0),
            )?;
            if note != newest_note {
                problems.push(format!(
                    "documento {doc}: nota atual {note:?}, a regra dá {newest_note:?}"
                ));
            }
            let current = current_analysis(conn, doc)?;
            if analysis != current {
                problems.push(format!(
                    "documento {doc}: análise atual {analysis:?}, a regra dá {current:?}"
                ));
            }
        }
        Ok(problems)
    }

    /// The invariants after every step of the F1 characterization tests
    /// (`f1_inventory`), which run unchanged: called by the product, in
    /// this crate's tests only, after each change queued and each exchange;
    /// it acts on the threads of those tests alone (a test runs on a thread
    /// named after it) and on a library of schema 22.
    #[cfg(test)]
    pub(crate) fn check_f1_step(conn: &Connection, step: &str) {
        let thread = std::thread::current();
        let Some(name) = thread.name().filter(|n| n.starts_with("f1_inventory::")) else {
            return;
        };
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap_or(0);
        if version < crate::storage::LIBRARY_SCHEMA {
            return;
        }
        let problems = invariant_problems(conn).unwrap();
        assert!(
            problems.is_empty(),
            "{name}, depois de {step}: {problems:?}"
        );
        F1_STEPS_CHECKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// How many F1 steps `check_f1_step` checked in this run (a test makes
    /// sure the hook is not idle).
    #[cfg(test)]
    pub(crate) static F1_STEPS_CHECKED: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    /// For the phone core: `user_version`, the invariant problems and the
    /// number of identities of the library at `path`.
    pub fn check_library(path: &Path) -> Result<(i64, Vec<String>, i64)> {
        let conn = raw(path)?;
        let version = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let problems = invariant_problems(&conn)?;
        let idents = conn.query_row("SELECT count(*) FROM item_ident", [], |r| r.get(0))?;
        Ok((version, problems, idents))
    }
}
