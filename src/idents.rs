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
use std::collections::BTreeSet;

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

/// The rows a logged execution inserted: (ident kind, row id).
pub(crate) type Inserted = BTreeSet<(String, i64)>;

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

/// The moment an analysis was made, as `storage::current_analysis` reads
/// it (an RFC 3339 time with its offset), or NULL when it does not read as
/// one. SQLite keeps milliseconds, which is what the Ponte writes.
fn analysis_time(column: &str) -> String {
    format!(
        "(CASE WHEN trim({column}) GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9][T ][0-9][0-9]:[0-9][0-9]:[0-9][0-9]*' \
         AND (trim({column}) GLOB '*Z' OR trim({column}) GLOB '*[+-][0-9][0-9]:[0-9][0-9]') \
         THEN julianday(trim({column})) END)"
    )
}

/// The current analysis of the document `document` (an SQL expression):
/// the one made last by its own time, unreadable times first, ties by
/// number, as `storage::current_analysis`.
fn current_analysis_sql(document: &str) -> String {
    let at = analysis_time("a.created_at");
    format!(
        "(SELECT a.id FROM analyses a WHERE a.document_id={document} ORDER BY {at} IS NULL, {at} DESC, a.id DESC LIMIT 1)"
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
            "AFTER UPDATE OF id,document_id,created_at ON analyses BEGIN UPDATE current_pointer SET analysis_id={} WHERE document_id IN (OLD.document_id,NEW.document_id); END;",
            current_analysis_sql("current_pointer.document_id")
        ),
    );
    out
}

/// The log of the rows this connection inserts: a TEMP table, seen by this
/// connection only (another process, or another connection of this one,
/// never writes to it). Created at every opening.
pub(crate) fn install_insert_log(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS ponte_inserted(kind TEXT NOT NULL, row_id INTEGER NOT NULL);",
    )?;
    Ok(())
}

/// The TEMP triggers that fill the log: they exist only while a log is
/// open. (A TEMP trigger on a main table that stayed for the life of the
/// connection would keep that connection from renaming tables whenever
/// another connection had renamed the table away: SQLite checks it and
/// cannot even drop it then.)
fn log_triggers(create: bool) -> String {
    let mut sql = String::new();
    for (kind, table) in KINDS {
        if create {
            sql.push_str(&format!(
                "CREATE TEMP TRIGGER IF NOT EXISTS ponte_log_{table} AFTER INSERT ON main.{table} \
                 BEGIN INSERT INTO temp.ponte_inserted(kind,row_id) VALUES('{kind}',NEW.id); END;\n"
            ));
        } else {
            sql.push_str(&format!("DROP TRIGGER IF EXISTS temp.ponte_log_{table};\n"));
        }
    }
    sql
}

/// Opens the log of this connection, empty. Inside a transaction, a
/// rollback closes it again.
pub(crate) fn start_log(conn: &Connection) -> Result<()> {
    conn.execute_batch(&format!(
        "DELETE FROM temp.ponte_inserted;\n{}",
        log_triggers(true)
    ))?;
    Ok(())
}

/// The rows inserted since [`start_log`].
pub(crate) fn logged(conn: &Connection) -> Result<Inserted> {
    let mut statement = conn.prepare("SELECT kind,row_id FROM temp.ponte_inserted")?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Inserted>>()?;
    Ok(rows)
}

/// Closes the log and empties it.
pub(crate) fn stop_log(conn: &Connection) {
    let _ = conn.execute_batch(&format!(
        "{}DELETE FROM temp.ponte_inserted;",
        log_triggers(false)
    ));
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

/// The rows `mutation`, answered with `reply`, inserted for each of its
/// slots, as (kind, slot, row): the slots of `idmap::created_ids` whose row
/// is in `inserted`, plus `note` for a saved note and `analysis` for a
/// saved analysis (the row of that document this very execution inserted,
/// never the newest of the table).
fn slot_rows(
    conn: &Connection,
    mutation: &IntranetMutation,
    reply: &MutationReply,
    inserted: &Inserted,
) -> Result<Vec<(&'static str, String, i64)>> {
    let mut out = Vec::new();
    if inserted.is_empty() {
        return Ok(out);
    }
    for created in crate::idmap::created_ids(&ConnFrame(conn), mutation, reply)? {
        let kind = kind_of(created.kind);
        if inserted.contains(&(kind.to_owned(), created.id)) {
            out.push((kind, created.slot, created.id));
        }
    }
    let own = |kind: &'static str, table: &str, document: i64| -> Result<Option<i64>> {
        let mut newest = None;
        for (_, row) in inserted.iter().filter(|(k, _)| k == kind) {
            let of: Option<i64> = conn
                .query_row(
                    &format!("SELECT document_id FROM {table} WHERE id=?1"),
                    [row],
                    |r| r.get(0),
                )
                .optional()?;
            if of == Some(document) {
                newest = newest.max(Some(*row));
            }
        }
        Ok(newest)
    };
    match mutation {
        IntranetMutation::SaveNote { document_id, .. } => {
            if let Some(row) = own("note", "notes", *document_id)? {
                out.push(("note", "note".into(), row));
            }
        }
        IntranetMutation::SaveAnalysis { document_id, .. } => {
            if let Some(row) = own("analysis", "analyses", *document_id)? {
                out.push(("analysis", "analysis".into(), row));
            }
        }
        _ => {}
    }
    Ok(out)
}

/// The identities of the rows `mutation` inserted here, by slot: what a
/// queued change carries (`OutboxEntry::idents`).
pub(crate) fn created_idents(
    conn: &Connection,
    mutation: &IntranetMutation,
    reply: &MutationReply,
    inserted: &Inserted,
) -> Result<Vec<SlotIdent>> {
    let mut out = Vec::new();
    for (kind, slot, row) in slot_rows(conn, mutation, reply, inserted)? {
        if let Some(ident) = ident_of(conn, kind, row)? {
            out.push(SlotIdent {
                kind: kind.to_owned(),
                slot,
                ident,
            });
        }
    }
    Ok(out)
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
    let rows = slot_rows(conn, mutation, reply, inserted)?;
    for slot in carried {
        let Some((kind, _, row)) = rows
            .iter()
            .find(|(kind, name, _)| *kind == slot.kind && *name == slot.slot)
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

impl Store {
    /// Runs `work` with the log of inserted rows open; answers its result
    /// and the rows it inserted.
    pub(crate) fn run_logged<T>(
        &mut self,
        work: impl FnOnce(&mut Store) -> Result<T>,
    ) -> Result<(T, Inserted)> {
        start_log(&self.conn)?;
        let result = work(self);
        let inserted = logged(&self.conn);
        stop_log(&self.conn);
        Ok((result?, inserted?))
    }

    /// Applies a replayed change (as `execute_mutation`) and gives the rows
    /// it inserted the identities the change carried (see [`rekey`]).
    pub(crate) fn execute_carrying(
        &mut self,
        actor: &str,
        mutation: IntranetMutation,
        carried: &[SlotIdent],
    ) -> Result<(MutationReply, Rekeyed)> {
        let applied = mutation.clone();
        let (reply, inserted) = self.run_logged(|store| store.execute_mutation(actor, mutation))?;
        let rekeyed = rekey(&self.conn, carried, &applied, &reply, &inserted)?;
        Ok((reply, rekeyed))
    }
}

/// Failure and pause points for the tests of the migration.
#[cfg(any(test, feature = "test-support"))]
pub mod hooks {
    use std::cell::RefCell;
    type Pause = Box<dyn FnOnce()>;
    type Fault = Box<dyn FnOnce(&std::path::Path) -> anyhow::Result<()>>;
    type Peek = Box<dyn FnMut(&crate::storage::Store)>;
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
    }
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

    /// The current analysis of a document as the product chooses it
    /// (`storage::current_analysis`): the one made last by its own time,
    /// unreadable times first, ties by number.
    fn current_analysis(conn: &Connection, document: i64) -> Result<Option<i64>> {
        let rows: Vec<(i64, String)> = conn
            .prepare("SELECT id,created_at FROM analyses WHERE document_id=?1")?
            .query_map([document], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows
            .into_iter()
            .max_by_key(|(id, at)| {
                (
                    chrono::DateTime::parse_from_rfc3339(at.trim())
                        .ok()
                        .map(|t| t.timestamp_micros()),
                    *id,
                )
            })
            .map(|(id, _)| id))
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
