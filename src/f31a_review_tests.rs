//! Review of F3.1a-i (`revisao-f3-1a-i.md`): each test reproduces a finding
//! of that review, or the decision the owner took on its finding 4, and
//! says why it fails on `8d1f21a` (F3.1a-ii), or that it is a guard (green
//! there, red when the code it guards is taken out).
#![allow(clippy::all)]

use crate::f3a2_tests::capture;
use crate::idents::{hooks, test_support, SlotIdent};
use crate::intranet::{CabinetSnapshot, IntranetMutation, NoteRow};
use crate::storage::Store;
use rusqlite::types::Value;
use std::path::Path;

fn same(c: char) -> String {
    std::iter::repeat(c).take(32).collect()
}

fn library(dir: &tempfile::TempDir, name: &str) -> Store {
    Store::open(&dir.path().join(name)).unwrap()
}

fn pointer(store: &Store, doc: i64) -> (Option<i64>, Option<i64>) {
    store
        .conn
        .query_row(
            "SELECT note_id,analysis_id FROM current_pointer WHERE document_id=?1",
            [doc],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

fn row_of(store: &Store, kind: &str, ident: &str) -> i64 {
    crate::f3b2_tests::row_of(store, kind, ident).unwrap_or_else(|| panic!("{kind} {ident}"))
}

fn stamp(store: &Store, table: &str, id: i64, at: &str) {
    store
        .conn
        .execute(
            &format!("UPDATE {table} SET created_at=?1 WHERE id=?2"),
            rusqlite::params![at, id],
        )
        .unwrap();
}

fn moment(store: &Store, table: &str, id: i64) -> String {
    store
        .conn
        .query_row(
            &format!("SELECT created_at FROM {table} WHERE id=?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

fn newest(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT max(id) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// Saves a note and answers its number.
fn note(store: &mut Store, doc: i64, text: &str) -> i64 {
    store.save_note(doc, text).unwrap();
    newest(store, "notes")
}

fn analysis_of(capture_id: i64, summary: &str, at: &str) -> crate::model::Analysis {
    crate::model::Analysis {
        capture_id,
        summary: summary.into(),
        priority: "normal".into(),
        reason: "Motivo.".into(),
        model: "modelo".into(),
        created_at: at.into(),
        ..Default::default()
    }
}

/// Saves an analysis of version `capture_id` made at `at` and answers its
/// number.
fn analysis(store: &mut Store, doc: i64, capture_id: i64, summary: &str, at: &str) -> i64 {
    store
        .save_analysis(doc, &analysis_of(capture_id, summary, at))
        .unwrap();
    newest(store, "analyses")
}

fn shown(store: &Store, doc: i64) -> (String, Option<String>) {
    let detail = store.load_document(doc).unwrap();
    (detail.note, detail.analysis.map(|a| a.summary))
}

// ------------------------------------------------------ finding 1 --

/// Finding 1. The moment of a new version is compared with the current
/// one at the precision it is stored at (milliseconds): a current version
/// stored at `.000Z` and a clock at `.000400` give `.001Z`, strictly after
/// it; a moment kept with more precision than that (a copy's) is passed
/// too; a current version in the future gives one millisecond after it;
/// one in the past, or unreadable, gives now.
///
/// Red on `8d1f21a`: `.000400` is after `.000Z`, so now is taken, and
/// stored as `.000Z`: the same moment as the current version.
#[test]
fn f3_1a_r1_moment_after_is_strict_at_the_stored_precision() {
    let at = |text: &str| {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&chrono::Utc)
    };
    let after = crate::storage::moment_after_at;
    assert_eq!(
        after(
            Some("2026-11-24T10:00:00.000Z"),
            at("2026-11-24T10:00:00.000400Z")
        ),
        "2026-11-24T10:00:00.001Z"
    );
    assert_eq!(
        after(
            Some("2026-11-24T10:00:00.000900Z"),
            at("2026-11-24T10:00:00.000950Z")
        ),
        "2026-11-24T10:00:00.001Z"
    );
    assert_eq!(
        after(
            Some("2026-11-24T10:00:00.000900Z"),
            at("2026-11-24T10:00:00.001100Z")
        ),
        "2026-11-24T10:00:00.001Z"
    );
    assert_eq!(
        after(
            Some("2099-01-01T00:00:00.000Z"),
            at("2026-11-24T10:00:00.000400Z")
        ),
        "2099-01-01T00:00:00.001Z"
    );
    assert_eq!(
        after(
            Some("2026-11-24T09:00:00.000Z"),
            at("2026-11-24T10:00:00.000400Z")
        ),
        "2026-11-24T10:00:00.000Z"
    );
    for previous in [None, Some("ontem")] {
        assert_eq!(
            after(previous, at("2026-11-24T10:00:00.000400Z")),
            "2026-11-24T10:00:00.000Z"
        );
    }
}

/// Findings 1 and 5. A note is saved while the current one is stamped in
/// the future (a clock behind another computer's): the new note is the
/// one shown and pointed at, stamped one millisecond after the current
/// one (never at the same moment, never far beyond it).
///
/// A guard on `8d1f21a` (green: the future case was already handled by
/// `moment_after`); red if the new note is stamped with plain now.
#[test]
fn f3_1a_r1_note_saved_while_the_current_is_stamped_in_the_future() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = library(&dir, "a.sqlite3");
    let doc = store
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let first = note(&mut store, doc, "Primeira.");
    stamp(&store, "notes", first, "2099-01-01T00:00:00.000Z");
    let second = note(&mut store, doc, "Segunda.");
    assert_eq!(store.load_document(doc).unwrap().note, "Segunda.");
    assert_eq!(pointer(&store, doc).0, Some(second));
    assert_eq!(moment(&store, "notes", second), "2099-01-01T00:00:00.001Z");
}

/// Findings 1 and 5. A version captured at the same moment as the current
/// one is imported while the current one is stored in the future: it is
/// the current version, stored one millisecond after the current one, so
/// the one order of versions (what a promotion from the Lixeira uses) puts
/// it first too.
///
/// A guard on `8d1f21a` (green); red if the new version is stored with
/// plain now (the order of versions would put the old one first while the
/// document shows the new one).
#[test]
fn f3_1a_r1_version_imported_while_the_current_is_stored_in_the_future() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = library(&dir, "a.sqlite3");
    let version = |text: &str| {
        let mut c = capture("SEFAZ-PRO-2026/05001", text);
        c.captured_at = "2026-11-01T10:00:00Z".into();
        c
    };
    let first = store.import_capture(&version("V1")).unwrap();
    stamp(
        &store,
        "captures",
        first.capture_id,
        "2099-01-01T00:00:00.000Z",
    );
    let second = store.import_capture(&version("V2")).unwrap();
    let latest: i64 = store
        .conn
        .query_row(
            "SELECT latest_capture_id FROM documents WHERE id=?1",
            [first.document_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(latest, second.capture_id);
    assert_eq!(
        moment(&store, "captures", second.capture_id),
        "2099-01-01T00:00:00.001Z"
    );
    assert_eq!(
        crate::storage::newest_capture(&store.conn, &[first.capture_id, second.capture_id])
            .unwrap(),
        Some(second.capture_id)
    );
}

// ------------------------------------------------------ finding 2 --

/// Finding 2 (a) / frozen rule 17. Two analyses of one document made at
/// the same moment (the moment travels in the change) are replayed with
/// the identities they carry, in opposite orders on A and B. On both, the
/// pointer and the read choose the higher identity.
///
/// A guard on `8d1f21a` (green: the trigger on `UPDATE OF ident` reads
/// the pointer again). Red when that trigger is taken out: on A the
/// second replay is inserted with a random identity above the first's and
/// becomes current; its carried identity, set afterwards, is the lower.
#[test]
fn f3_1a_r2_tied_analyses_replayed_in_opposite_orders_agree() {
    let dir = tempfile::tempdir().unwrap();
    let high = format!("{}1", "0".repeat(31));
    let low = same('0');
    let at = "2026-11-23T10:00:00.000Z";
    let on = |name: &str, order: [(&str, &String); 2]| {
        let mut store = library(&dir, name);
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        for (text, ident) in order {
            let carried = vec![SlotIdent {
                kind: "analysis".into(),
                slot: "analysis".into(),
                ident: ident.clone(),
            }];
            let mutation = IntranetMutation::SaveAnalysis {
                document_id: receipt.document_id,
                analysis: analysis_of(receipt.capture_id, &format!("análise {text}"), at),
            };
            match store
                .execute_carrying("Casa", mutation, &carried, None)
                .unwrap()
            {
                crate::idents::Carried::Applied(..) => {}
                crate::idents::Carried::Refused(e) => panic!("{e}"),
            }
        }
        (store, receipt.document_id)
    };
    let a = on("a.sqlite3", [("alta", &high), ("baixa", &low)]);
    let b = on("b.sqlite3", [("baixa", &low), ("alta", &high)]);
    for (store, doc) in [&a, &b] {
        let alta = row_of(store, "analysis", &high);
        assert_eq!(pointer(store, *doc).1, Some(alta));
        assert_eq!(shown(store, *doc).1.as_deref(), Some("análise alta"));
    }
}

/// Finding 2 (b) / frozen rule 17. A library holds two notes of one
/// document made at the same moment; it takes the identities of an equal
/// snapshot (`take_identities_from`), which gives the note it shows the
/// lower identity. The pointer and the read follow the identities taken.
///
/// A guard on `8d1f21a` (green: `adopt_triples` updates `item_ident`, and
/// its trigger reads the pointer again). Red when that trigger is taken
/// out: the pointer stays on the note it had.
#[test]
fn f3_1a_r2_tied_notes_follow_identities_taken_from_an_equal_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = library(&dir, "a.sqlite3");
    let doc = store
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let one = note(&mut store, doc, "Uma.");
    let other = note(&mut store, doc, "Outra.");
    for id in [one, other] {
        stamp(&store, "notes", id, "2026-11-23T10:00:00.000Z");
    }
    let before = pointer(&store, doc).0.unwrap();
    let (shown_text, hidden, hidden_text) = if before == one {
        ("Uma.", other, "Outra.")
    } else {
        ("Outra.", one, "Uma.")
    };
    assert_eq!(store.load_document(doc).unwrap().note, shown_text);
    let mut copy = store.export_with_idents(&same('a')).unwrap();
    for row in &mut copy.idents {
        if row.0 == "note" {
            row.2 = if row.1 == before {
                same('0')
            } else {
                same('f')
            };
        }
    }
    copy.idents.sort();
    store.take_identities_from(&copy, false).unwrap();
    assert_eq!(row_of(&store, "note", &same('f')), hidden, "taken");
    assert_eq!(pointer(&store, doc).0, Some(hidden));
    assert_eq!(store.load_document(doc).unwrap().note, hidden_text);
}

// ------------------------------------------------------ finding 3 --

/// Finding 3. The oracle of the invariants (run after every step of the
/// F1 tests) states the note rule as the product reads it: a library
/// whose newest note by time is not the highest number has no problem; a
/// pointer that is not the product's read is one.
///
/// Red on `8d1f21a`: the oracle compares the pointer with `max(id)` and
/// reports a problem where there is none.
#[test]
fn f3_1a_r3_the_oracle_reads_the_current_note_as_the_product() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = library(&dir, "a.sqlite3");
    let doc = store
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let older = note(&mut store, doc, "Um.");
    let higher = note(&mut store, doc, "Dois.");
    stamp(&store, "notes", higher, "2000-01-01T00:00:00.000Z");
    assert_eq!(store.load_document(doc).unwrap().note, "Um.");
    assert_eq!(pointer(&store, doc).0, Some(older));
    let problems = test_support::invariant_problems(&store.conn).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    store
        .conn
        .execute(
            "UPDATE current_pointer SET note_id=?1 WHERE document_id=?2",
            [higher, doc],
        )
        .unwrap();
    let problems = test_support::invariant_problems(&store.conn).unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
}

// ------------------------------------------------------ finding 4 --

/// The rows of `table`, every column, by number.
fn dump(store: &Store, table: &str) -> Vec<Vec<Value>> {
    let mut statement = store
        .conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |r| {
            (0..columns)
                .map(|i| r.get::<_, Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// The tables whose rows an update to schema 22 must keep as they are
/// (`notes` and `analyses` but for the moments it moves).
const KEPT: &[&str] = &[
    "documents",
    "captures",
    "requests",
    "notes",
    "drafts",
    "analyses",
    "tasks",
    "trash",
    "document_links",
    "capture_refs",
];

fn dump_all(store: &Store) -> Vec<(String, Vec<Vec<Value>>)> {
    KEPT.iter()
        .map(|t| (t.to_string(), dump(store, t)))
        .collect()
}

/// `before` with the moment (`created_at`) of the row `id` of `table` set
/// to `at`.
fn moved(before: &mut [(String, Vec<Vec<Value>>)], store: &Store, table: &str, id: i64, at: &str) {
    let column = store
        .conn
        .prepare(&format!("SELECT * FROM {table}"))
        .unwrap()
        .column_names()
        .into_iter()
        .position(|c| c == "created_at")
        .unwrap();
    let rows = &mut before.iter_mut().find(|(t, _)| t == table).unwrap().1;
    let row = rows
        .iter_mut()
        .find(|r| r[0] == Value::Integer(id))
        .unwrap();
    row[column] = Value::Text(at.into());
}

/// Puts the library at `path` back at schema 21 (as Ponte 0.51 left it)
/// and copies it to `twin` too, when given (the same synchronized state on
/// two computers).
fn back_to_21(path: &Path, twin: Option<&Path>) {
    test_support::downgrade_to_21(path).unwrap();
    if let Some(twin) = twin {
        for suffix in ["", "-wal", "-shm"] {
            let from = format!("{}{suffix}", path.display());
            if Path::new(&from).exists() {
                std::fs::copy(&from, format!("{}{suffix}", twin.display())).unwrap();
            }
        }
    }
}

/// A library with what Ponte 0.51 showed, where the one order of versions
/// would choose otherwise. Answers (document, the note and the analysis
/// 0.51 showed) per case, and the moments the update must give them.
struct Shown051 {
    /// Notes saved on computers whose clocks disagree: the last saved
    /// (the highest number, shown by 0.51) is stamped before the other.
    skewed: i64,
    skewed_note: i64,
    skewed_analysis: i64,
    /// Nothing to move: time order and number order agree.
    plain: i64,
    /// In the Lixeira, with notes as `skewed`.
    trashed: i64,
    trashed_note: i64,
    /// A version brought back as current by an identical re-import, older
    /// than another version of the document.
    reactivated: i64,
    reactivated_version: i64,
}

fn shown_051(store: &mut Store) -> Shown051 {
    let receipt = store
        .import_capture(&capture("SEFAZ-PRO-2026/06001", "Um."))
        .unwrap();
    let skewed = receipt.document_id;
    let v1 = note(store, skewed, "A v1");
    let skewed_note = note(store, skewed, "A v2");
    stamp(store, "notes", v1, "2026-11-20T10:05:00.000Z");
    stamp(store, "notes", skewed_note, "2026-11-20T10:02:00.000Z");
    let tie = "2026-11-20T09:00:00.000Z";
    analysis(store, skewed, receipt.capture_id, "A x1", tie);
    let skewed_analysis = analysis(store, skewed, receipt.capture_id, "A x2", tie);

    let plain = store
        .import_capture(&capture("SEFAZ-PRO-2026/06002", "Dois."))
        .unwrap();
    let b1 = note(store, plain.document_id, "B v1");
    let b2 = note(store, plain.document_id, "B v2");
    stamp(store, "notes", b1, "2026-11-20T10:00:00.000Z");
    stamp(store, "notes", b2, "2026-11-20T10:01:00.000Z");
    analysis(
        store,
        plain.document_id,
        plain.capture_id,
        "B x1",
        "2026-11-20T09:00:00.000Z",
    );

    let trashed = store
        .import_capture(&capture("SEFAZ-PRO-2026/06003", "Três."))
        .unwrap()
        .document_id;
    let c1 = note(store, trashed, "C v1");
    let trashed_note = note(store, trashed, "C v2");
    stamp(store, "notes", c1, "2026-11-20T12:00:00.000Z");
    stamp(store, "notes", trashed_note, "2026-11-20T11:00:00.000Z");
    store
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: trashed,
        })
        .unwrap();

    let version = |text: &str, at: &str| {
        let mut c = capture("SEFAZ-PRO-2026/06004", text);
        c.captured_at = at.into();
        c
    };
    let old = store
        .import_capture(&version("D antigo", "2026-11-01T10:00:00Z"))
        .unwrap();
    store
        .import_capture(&version("D novo", "2026-11-02T10:00:00Z"))
        .unwrap();
    // The same content seen again later: the version already there.
    let again = store
        .import_capture(&version("D antigo", "2026-11-03T10:00:00Z"))
        .unwrap();
    assert_eq!(again.capture_id, old.capture_id, "the same version");
    Shown051 {
        skewed,
        skewed_note,
        skewed_analysis,
        plain: plain.document_id,
        trashed,
        trashed_note,
        reactivated: old.document_id,
        reactivated_version: old.capture_id,
    }
}

/// The owner's decision on finding 4. The update to schema 22 keeps
/// current what Ponte 0.51 showed: the last note saved (the highest
/// number) though another is stamped later, and of two analyses made at
/// the same moment the one 0.51 chose (the higher number), whatever the
/// identities the update draws; the version a re-import made current
/// stays current. Only the moment of those rows moves, to one millisecond
/// after the latest of their document, documents in the Lixeira included;
/// nothing else of the library changes.
///
/// Red on `8d1f21a`: the update leaves the moments as they are, and the
/// order of versions shows the note stamped later ("A v1"), and the
/// analysis of the higher identity (a coin toss).
#[test]
fn f3_1a_r4_the_update_keeps_what_0_51_showed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sqlite3");
    let (case, expected) = {
        let mut store = Store::open(&path).unwrap();
        let case = shown_051(&mut store);
        let mut expected = dump_all(&store);
        for (table, id, at) in [
            ("notes", case.skewed_note, "2026-11-20T10:05:00.001Z"),
            ("analyses", case.skewed_analysis, "2026-11-20T09:00:00.001Z"),
            ("notes", case.trashed_note, "2026-11-20T12:00:00.001Z"),
        ] {
            moved(&mut expected, &store, table, id, at);
        }
        (case, expected)
    };
    back_to_21(&path, None);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
    assert_eq!(dump_all(&store), expected);
    assert_eq!(
        shown(&store, case.skewed),
        ("A v2".into(), Some("A x2".into()))
    );
    assert_eq!(
        pointer(&store, case.skewed),
        (Some(case.skewed_note), Some(case.skewed_analysis))
    );
    assert_eq!(
        shown(&store, case.plain),
        ("B v2".into(), Some("B x1".into()))
    );
    let latest: i64 = store
        .conn
        .query_row(
            "SELECT latest_capture_id FROM documents WHERE id=?1",
            [case.reactivated],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(latest, case.reactivated_version);
    assert_eq!(
        store.load_document(case.reactivated).unwrap().capture.text,
        "D antigo"
    );
    store
        .restore_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: case.trashed,
        })
        .unwrap();
    assert_eq!(store.load_document(case.trashed).unwrap().note, "C v2");
    assert_eq!(pointer(&store, case.trashed).0, Some(case.trashed_note));
    let problems = test_support::invariant_problems(&store.conn).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
}

/// The owner's decision on finding 4, across computers. Two libraries
/// hold the same synchronized state at schema 21, with notes of one
/// moment in several documents (0.51 showed the highest number) and the
/// notes of clocks that disagree. Each is updated on its own: both show
/// what 0.51 showed, with the same rows and moments, whatever identities
/// each update drew.
///
/// Red on `8d1f21a`: the skewed note goes to the one stamped later, and
/// each tie is decided by the identities each library drew at random.
#[test]
fn f3_1a_r4_two_libraries_updated_from_the_same_state_agree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sqlite3");
    let twin = dir.path().join("b.sqlite3");
    let (case, tied) = {
        let mut store = Store::open(&path).unwrap();
        let case = shown_051(&mut store);
        let mut tied = Vec::new();
        for n in 0..6 {
            let doc = store
                .import_capture(&capture(&format!("SEFAZ-PRO-2026/0610{n}"), "Empate."))
                .unwrap()
                .document_id;
            let first = note(&mut store, doc, "primeira");
            let last = note(&mut store, doc, "última");
            for id in [first, last] {
                stamp(&store, "notes", id, "2026-11-20T10:00:00.000Z");
            }
            tied.push((doc, last));
        }
        (case, tied)
    };
    back_to_21(&path, Some(&twin));
    let a = Store::open(&path).unwrap();
    let b = Store::open(&twin).unwrap();
    assert_eq!(dump_all(&a), dump_all(&b));
    for store in [&a, &b] {
        assert_eq!(shown(store, case.skewed).0, "A v2");
        for (doc, last) in &tied {
            assert_eq!(shown(store, *doc).0, "última");
            assert_eq!(pointer(store, *doc).0, Some(*last));
            assert_eq!(moment(store, "notes", *last), "2026-11-20T10:00:00.001Z");
        }
    }
}

/// The owner's decision on finding 4, for a copy of the older format: a
/// copy without identities (a folder file of format 8, a portable copy of
/// 0.51) is what Ponte 0.51 shows, and the library that adopts it keeps
/// that: the note saved last (the highest number) though another is
/// stamped later, the same on every library that adopts it. A copy with
/// identities (format 9, team V2) is taken as it is: there the order of
/// versions decides.
///
/// Red on `8d1f21a`: the copy without identities shows the note stamped
/// later.
#[test]
fn f3_1a_r4_a_copy_without_identities_keeps_what_0_51_shows() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = library(&dir, "fonte.sqlite3");
    let doc = source
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let mut copy: CabinetSnapshot = source.export_snapshot().unwrap();
    assert!(copy.idents.is_empty() && copy.ident_epoch.is_empty());
    for (id, text, at) in [
        (900, "v1", "2026-11-20T10:05:00.000Z"),
        (901, "v2", "2026-11-20T10:02:00.000Z"),
    ] {
        copy.notes.push(NoteRow {
            id,
            document_id: doc,
            note: text.into(),
            created_at: at.into(),
        });
    }
    let mut adopted = Vec::new();
    for name in ["a.sqlite3", "b.sqlite3"] {
        let mut store = library(&dir, name);
        crate::f3b2_tests::adopt(&mut store, &copy);
        assert_eq!(store.load_document(doc).unwrap().note, "v2");
        assert_eq!(pointer(&store, doc).0, Some(901));
        adopted.push(serde_json::to_value(store.export_snapshot().unwrap().notes).unwrap());
    }
    assert_eq!(adopted[0], adopted[1]);

    // With identities: taken as it is.
    let mut store = library(&dir, "c.sqlite3");
    crate::f3b2_tests::adopt(&mut store, &copy);
    let mut with = store.export_with_idents(&same('a')).unwrap();
    for row in &mut with.notes {
        row.created_at = if row.id == 900 {
            "2026-11-20T10:05:00.000Z".into()
        } else {
            "2026-11-20T10:02:00.000Z".into()
        };
    }
    let mut other = library(&dir, "d.sqlite3");
    crate::f3b2_tests::adopt(&mut other, &with);
    assert_eq!(other.load_document(doc).unwrap().note, "v1");
    assert_eq!(moment(&other, "notes", 901), "2026-11-20T10:02:00.000Z");
}

/// The owner's decision on finding 4, for a library whose update could not
/// be done at this opening (it stays at schema 21): it shows the note
/// Ponte 0.51 showed (the highest number), a note saved there is the one
/// shown, and the update done at a later opening keeps it.
///
/// Red on `8d1f21a`: a library left at 21 orders its notes by time, and
/// shows the one stamped later.
#[test]
fn f3_1a_r4_a_library_left_at_21_shows_what_0_51_showed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sqlite3");
    let case = {
        let mut store = Store::open(&path).unwrap();
        shown_051(&mut store)
    };
    back_to_21(&path, None);
    hooks::FREE_SPACE.with(|h| *h.borrow_mut() = Some(Ok(0)));
    let mut store = Store::open(&path).unwrap();
    hooks::FREE_SPACE.with(|h| h.borrow_mut().take());
    assert_eq!(test_support::user_version(&path).unwrap(), 21);
    assert_eq!(store.load_document(case.skewed).unwrap().note, "A v2");
    store.save_note(case.skewed, "A v3").unwrap();
    assert_eq!(store.load_document(case.skewed).unwrap().note, "A v3");
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
    assert_eq!(store.load_document(case.skewed).unwrap().note, "A v3");
}

/// The rule of finding 4, alone. The version 0.51 chose moves only when
/// another is not strictly before it, to one millisecond after the latest
/// of the group (past a moment kept with more precision too); one already
/// first by time stays; an unreadable moment counts as the oldest, and a
/// group with none readable gets the start of 1970. Of the analyses, only
/// those of the current version compete when it has any.
#[test]
fn f3_1a_r4_the_rule_kept_from_0_51() {
    use crate::idents::{keep_shown, shown_moves, Shown051};
    let note = Shown051::HighestNumber;
    let analysis = Shown051::LatestThenNumber;
    assert_eq!(
        keep_shown(
            &[
                (1, "2026-11-20T10:05:00.000Z"),
                (2, "2026-11-20T10:02:00.000Z")
            ],
            note
        ),
        Some((2, "2026-11-20T10:05:00.001Z".into()))
    );
    assert_eq!(
        keep_shown(
            &[
                (1, "2026-11-20T10:00:00.000900Z"),
                (2, "2026-11-20T10:00:00.000Z")
            ],
            note
        ),
        Some((2, "2026-11-20T10:00:00.001Z".into()))
    );
    assert_eq!(
        keep_shown(
            &[
                (1, "2026-11-20T10:00:00.000Z"),
                (2, "2026-11-20T10:00:00.001Z")
            ],
            note
        ),
        None
    );
    assert_eq!(keep_shown(&[(7, "ontem")], note), None);
    assert_eq!(
        keep_shown(&[(1, "2026-11-20T10:00:00.000Z"), (2, "ontem")], note),
        Some((2, "2026-11-20T10:00:00.001Z".into()))
    );
    assert_eq!(
        keep_shown(&[(1, "ontem"), (2, "anteontem")], note),
        Some((2, "1970-01-01T00:00:00.000Z".into()))
    );
    assert_eq!(
        keep_shown(
            &[
                (1, "2026-11-20T09:00:00.000Z"),
                (2, "2026-11-20T09:00:00.000Z")
            ],
            analysis
        ),
        Some((2, "2026-11-20T09:00:00.001Z".into()))
    );
    assert_eq!(
        keep_shown(
            &[
                (1, "2026-11-20T10:00:00.000Z"),
                (2, "2026-11-20T09:00:00.000Z")
            ],
            analysis
        ),
        None
    );
    let current = std::collections::HashMap::from([(5, 50)]);
    let analyses = [
        (1, 5, 49, "2026-11-20T11:00:00.000Z".to_string()),
        (2, 5, 50, "2026-11-20T09:00:00.000Z".to_string()),
        (3, 5, 50, "2026-11-20T09:00:00.000Z".to_string()),
    ];
    assert_eq!(
        shown_moves(&[], &analyses, &current),
        (vec![], vec![(3, "2026-11-20T09:00:00.001Z".into())])
    );
}
