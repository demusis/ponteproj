//! Identities of the items (library schema 22, F2a): a random, fixed
//! `ident` for every row of the twelve kinds that are addressed by row id,
//! given in the same statement that inserts the row.
//!
//! STUB (tests first): only the types and the test helpers exist; the
//! behaviour comes with the implementation.
use crate::intranet::{IntranetMutation, MutationReply};
use crate::storage::Store;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
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

pub(crate) fn start_log(_conn: &Connection) -> Result<()> {
    Ok(())
}

pub(crate) fn logged(_conn: &Connection) -> Result<Inserted> {
    Ok(Inserted::new())
}

pub(crate) fn stop_log(_conn: &Connection) {}

impl Store {
    /// Applies a replayed change and gives the rows it inserted the
    /// identities the change carried.
    pub(crate) fn execute_carrying(
        &mut self,
        actor: &str,
        mutation: IntranetMutation,
        _carried: &[SlotIdent],
    ) -> Result<(MutationReply, Rekeyed)> {
        Ok((self.execute_mutation(actor, mutation)?, Rekeyed::default()))
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
