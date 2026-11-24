//! F3.1a-ii of Frente F: acceptance tests of the record kinds (document,
//! capture, note, draft, analysis, link) addressed by identity
//! (`desenho-f3-v2.md` §3 F3.1a, `desenho-f3-v3-adendo.md` §12,
//! `plano-frente-f.md` §9): a mapped adoption keeps this library's numbers
//! and rewrites every reference of the copy to them, the copy's numbers of
//! a mapped kind never explain an item here, the history of a record is in
//! the order of its versions, and a link is found by its two ends only
//! when exactly one matches. Written before the code: each test says why it
//! fails on `f7eff3b` (F3.1a-i), or that it is a guard there.
#![allow(clippy::all)]

use crate::f3a2_tests::{capture, two, Team};
use crate::f3b2_tests::{fresh_ident, kinds};
use crate::intranet::{CabinetSnapshot, FolderSyncReport};
use crate::storage::Store;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The record family (F3.1a).
const RECORDS: &[&str] = &["document", "capture", "note", "draft", "analysis", "link"];

const KEY: &str = "senha-de-teste-123";

// ---------------------------------------------------------------- helpers --

/// One exchange, which must keep nothing aside: a false alarm of the guard
/// (an item of this library taken as lost) fails the test.
fn sync(store: &mut Store) -> FolderSyncReport {
    let report = crate::f3a2_tests::fsync(store);
    assert!(
        !report
            .notes
            .iter()
            .any(|n| n.contains("no resultado desta sincronização")),
        "aviso de perda inesperado: {:?}",
        report.notes
    );
    report
}

/// The same, with only `files` in the list of shared files.
fn sync_in(store: &mut Store, files: &[&Path]) -> FolderSyncReport {
    let mut cfg = store.load_intranet_config().unwrap();
    cfg.folder_files = files.iter().map(|p| p.display().to_string()).collect();
    store.save_intranet_config(&cfg).unwrap();
    sync(store)
}

fn mapped(store: &Store) -> bool {
    crate::idents::is_mapped(&store.conn).unwrap()
}

fn doc(store: &Store, number: &str) -> Option<i64> {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .map(|d| d.id)
}

fn note(store: &Store, number: &str) -> String {
    store
        .load_document(doc(store, number).unwrap_or_else(|| panic!("sem {number}")))
        .unwrap()
        .note
}

fn cards(store: &Store) -> Vec<crate::intranet::SyncConflict> {
    store.list_sync_conflicts().unwrap()
}

const X: &str = "SEFAZ-PRO-2026/07001";
const Y: &str = "SEFAZ-PRO-2026/07002";

/// A and B of one shared file (format 9, this build's level), both mapped;
/// X made on A and Y on B without exchanging, at the same number. After
/// A, B and A exchange, each holds the other's item at the number its own
/// has (42 is X on A and Y on B). Answers (dir, A, B, the number both
/// made their item at).
fn crossed() -> (tempfile::TempDir, Store, Store, i64) {
    let (dir, _path, mut a, mut b) = two();
    assert!(mapped(&a) && mapped(&b), "both libraries are mapped");
    let x = a
        .import_capture(&capture(X, "Documento X, de A."))
        .unwrap()
        .document_id;
    let y = b
        .import_capture(&capture(Y, "Documento Y, de B."))
        .unwrap()
        .document_id;
    assert_eq!(x, y, "the same number on both");
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    (dir, a, b, x)
}

// --------------------------------------------- crossed pair (42 ↔ 93) --

/// v2 F3.1a `f3_1_held_document_survives_crossing`. B's screen holds Y at
/// 42 before the adoption that crosses; A holds X at 42. After the
/// exchanges 42 is X on A and Y on B, and the other item is at the same
/// other number on both. Each saves a note through the number it holds:
/// on both devices Y has B's note and X has A's, with no card.
///
/// Red on `f7eff3b`: B adopts the copy verbatim, so its 42 becomes X (F1
/// C2a), and B's note lands on X.
#[test]
fn f3_1a_crossed_pair_held_across_a_sync() {
    let (_dir, mut a, mut b, held) = crossed();
    assert_eq!(doc(&a, X), Some(held), "A keeps X at its number");
    assert_eq!(doc(&b, Y), Some(held), "B keeps Y at its number");
    assert_eq!(doc(&a, Y), doc(&b, X), "the other at the same number");
    assert_ne!(doc(&a, Y), Some(held));
    b.save_note(held, "Nota de B em Y").unwrap();
    a.save_note(held, "Nota de A em X").unwrap();
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert_eq!(note(store, Y), "Nota de B em Y");
        assert_eq!(note(store, X), "Nota de A em X");
        assert!(cards(store).is_empty());
    }
    assert_eq!(doc(&b, Y), Some(held), "still Y on B");
}

/// A record made offline with dependents queued on it (a note, a draft of
/// its version, a link to a record both know), while another device
/// published a record first: the exchange gives the copy's number of the
/// new record to that other one, and this library keeps its own. The
/// dependents land on the new record, on both devices, by identity.
///
/// Red on `f7eff3b`: B's new record takes the copy's number (verbatim), so
/// the number B held names A's record.
#[test]
fn f3_1a_queued_creation_with_dependents_keeps_its_number() {
    let (_dir, _path, mut a, mut b) = two();
    let first = a.list_documents("").unwrap()[0].id;
    a.import_capture(&capture(X, "Documento X, de A.")).unwrap();
    sync(&mut a);
    let made = b.import_capture(&capture(Y, "Documento Y, de B.")).unwrap();
    let w = made.document_id;
    b.save_note(w, "Nota de Y").unwrap();
    b.save_draft(w, made.capture_id, "resposta", "Minuta de Y")
        .unwrap();
    b.add_document_link(w, first, "relacionado", "Y responde ao primeiro")
        .unwrap();
    sync(&mut b);
    sync(&mut a);
    assert_eq!(doc(&b, Y), Some(w), "B keeps its number of Y");
    assert_ne!(doc(&b, X), Some(w), "X is elsewhere on B");
    for store in [&a, &b] {
        let y = doc(store, Y).unwrap();
        assert_eq!(note(store, Y), "Nota de Y");
        let drafts = store.load_document(y).unwrap().drafts;
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].body, "Minuta de Y");
        let links = store.document_links(y).unwrap();
        assert_eq!(links.len(), 1);
        let first_here = store.list_documents("").unwrap();
        let first_here = first_here
            .iter()
            .find(|d| d.document_number != X && d.document_number != Y)
            .unwrap()
            .id;
        assert_eq!((links[0].source, links[0].target), (y, first_here));
        assert!(cards(store).is_empty());
    }
}

// ------------------------------------------------- team: reply translated --

/// v2 §2.5 on a mapped client. The client gave document numbers above the
/// main computer's (a top it purged); an online import and a draft are
/// answered in the main computer's numbers, and the client gets its own:
/// the receipt names the record here, the draft is on it.
///
/// Red on `f7eff3b`: the client adopts verbatim, so the receipt's number
/// is the main computer's, below the floor this client already gave.
#[test]
fn f3_1a_team_reply_is_translated_to_this_library() {
    let mut t = Team::new();
    t.client.sync_from_host(false).unwrap();
    t.client.sync_from_host(false).unwrap();
    assert!(mapped(&t.client), "the client is mapped");
    t.client
        .conn
        .execute(
            "UPDATE sqlite_sequence SET seq=seq+50 WHERE name IN ('documents','captures','drafts')",
            [],
        )
        .unwrap();
    let floor: i64 = t
        .client
        .conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='documents'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let receipt = t
        .client
        .import_capture(&capture(X, "Documento X, pela equipe."))
        .unwrap();
    let on_host = doc(&t.host, X).unwrap();
    assert!(
        receipt.document_id > floor,
        "the client's own number ({} > {floor}; the host's is {on_host})",
        receipt.document_id
    );
    assert_eq!(doc(&t.client, X), Some(receipt.document_id));
    assert_eq!(
        t.client
            .load_capture(receipt.document_id, receipt.capture_id)
            .unwrap()
            .text,
        "Documento X, pela equipe."
    );
    let draft = t
        .client
        .save_draft(
            receipt.document_id,
            receipt.capture_id,
            "resposta",
            "Minuta",
        )
        .unwrap();
    let drafts = t.client.load_document(receipt.document_id).unwrap().drafts;
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].id, draft, "the draft's number here");
}

// ------------------------------------------------------ divergent merge --

/// v2 `f3_1_divergent_merge_keeps_local_ids` (F1 C2b). A publishes X to the
/// pen drive and queues a note on it; B publishes Y to the Drive, which is
/// newer. When A exchanges with both, the Drive is the base and A's pen
/// journal is replayed on it: X gets another number in the copy. A keeps
/// its own number of X, A's queued note lands on X, and B sees it; no card.
///
/// Red on `f7eff3b`: A adopts the merged copy verbatim, so X changes number
/// on A (F1 C2b) and the number A held names Y.
#[test]
fn f3_1a_divergent_merge_keeps_local_ids() {
    crate::idents::hooks::PROMOTION.with(|c| c.set(true));
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
    let config = |member: &str| crate::intranet::IntranetConfig {
        mode: crate::intranet::IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: vec![drive.display().to_string(), pen.display().to_string()],
        folder_key: KEY.into(),
        ..crate::intranet::IntranetConfig::default()
    };
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&config("Casa")).unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&config("Trabalho")).unwrap();
    sync(&mut b);
    assert!(sync(&mut a).promoted);
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    assert!(mapped(&a) && mapped(&b), "both libraries are mapped");

    let x = a
        .import_capture(&capture(X, "Documento X, no pen drive."))
        .unwrap()
        .document_id;
    sync_in(&mut a, &[&pen]);
    a.save_note(x, "Nota de A em X, na fila").unwrap();
    b.import_capture(&capture(Y, "Documento Y, no Drive."))
        .unwrap();
    sync_in(&mut b, &[&drive]);
    let first = b.list_documents("").unwrap();
    let first = first.iter().find(|d| d.document_number != Y).unwrap().id;
    for n in 0..3 {
        b.save_note(first, &format!("enchimento {n}")).unwrap();
        sync_in(&mut b, &[&drive]);
    }
    sync_in(&mut a, &[&drive, &pen]);
    sync_in(&mut b, &[&drive, &pen]);
    sync_in(&mut a, &[&drive, &pen]);
    assert_eq!(doc(&a, X), Some(x), "A keeps its number of X");
    for store in [&a, &b] {
        assert_eq!(note(store, X), "Nota de A em X, na fila");
        assert_eq!(note(store, Y), "");
        assert!(cards(store).is_empty());
    }
}

// --------------------------------------------------------- purge + reuse --

/// v2 `f3_1_purge_reuse_never_redirects`, on the crossed pair. B's screen
/// holds X at 93 (its number there) and B queues a note through it. A
/// purges X and makes Z; B exchanges: the note becomes a card with its
/// text, Y is unchanged, Z is not given 93, and 93 names nothing on B.
///
/// Red on `f7eff3b`: nothing crosses (B adopts verbatim), so B's X is at
/// the copy's number and the pair is not crossed.
#[test]
fn f3_1a_purge_and_reuse_never_redirect_a_held_number() {
    let (_dir, mut a, mut b, held_y) = crossed();
    let held_x = doc(&b, X).unwrap();
    assert_ne!(held_x, held_y);
    assert_eq!(doc(&a, Y), Some(held_x), "93 is Y on A");
    let target = |id| crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id,
    };
    let x_on_a = doc(&a, X).unwrap();
    a.trash_item(target(x_on_a)).unwrap();
    a.purge_trash_item(target(x_on_a)).unwrap();
    sync(&mut a);
    a.import_capture(&capture("SEFAZ-PRO-2026/07003", "Documento Z."))
        .unwrap();
    sync(&mut a);
    b.save_note(held_x, "Nota de B em X").unwrap();
    sync(&mut b);
    sync(&mut a);
    let c = cards(&b);
    assert_eq!(c.len(), 1, "B's note on X is a card");
    assert!(
        serde_json::to_string(&c[0].mutation)
            .unwrap()
            .contains("Nota de B em X"),
        "the card keeps the text"
    );
    for store in [&a, &b] {
        assert_eq!(note(store, Y), "", "Y is unchanged");
        assert!(doc(store, X).is_none(), "X is gone");
    }
    assert!(b.load_document(held_x).is_err(), "93 names nothing on B");
    assert_ne!(doc(&b, "SEFAZ-PRO-2026/07003"), Some(held_x));
    assert_eq!(doc(&b, Y), Some(held_y));
}

// ---------------------------------------------- a link by its two ends --

/// The links of the record `number` here, as (active, reason).
fn links_of(store: &Store, number: &str) -> Vec<(bool, String)> {
    let id = doc(store, number).unwrap();
    store
        .document_links(id)
        .unwrap()
        .into_iter()
        .map(|l| (l.active, l.payload.reason))
        .collect()
}

/// A and B with X and Y and one link X→Y; on B the link has an identity of
/// its own (as a link each device derives by itself, v2 N2), and B turns it
/// off without exchanging.
fn link_derived_apart() -> (tempfile::TempDir, Store, Store) {
    let (dir, _path, mut a, mut b) = two();
    let x = a
        .import_capture(&capture(X, "Documento X."))
        .unwrap()
        .document_id;
    let y = a
        .import_capture(&capture(Y, "Documento Y."))
        .unwrap()
        .document_id;
    a.add_document_link(x, y, "relacionado", "Vínculo de A")
        .unwrap();
    sync(&mut a);
    sync(&mut b);
    let on_b = b.document_links(doc(&b, X).unwrap()).unwrap()[0].id;
    b.conn
        .execute(
            "UPDATE item_ident SET ident=?1 WHERE kind='link' AND row_id=?2",
            rusqlite::params![fresh_ident(), on_b],
        )
        .unwrap();
    b.set_link_active(on_b, false).unwrap();
    (dir, a, b)
}

/// v2 `f3_1_auto_link_zero_one_two_candidates` (frozen rule 23). B turns
/// off a link whose identity the copy does not have: with exactly one link
/// of the same ends and kind there, that one is turned off, on both
/// devices; with two (A turned its link off and made it again), or none (A
/// purged an end), B's change becomes a card that keeps the toggle, and
/// nothing else changes.
///
/// Red on `f7eff3b`: the one-candidate case is a card (the identity alone
/// is tried).
#[test]
fn f3_1a_link_by_its_two_ends_only_when_one_matches() {
    // One candidate.
    let (_dir, mut a, mut b) = link_derived_apart();
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        assert!(cards(store).is_empty(), "one candidate: no card");
        assert_eq!(links_of(store, X), vec![(false, "Vínculo de A".into())]);
    }

    // Two candidates.
    let (_dir, mut a, mut b) = link_derived_apart();
    let (x, y) = (doc(&a, X).unwrap(), doc(&a, Y).unwrap());
    let link = a.document_links(x).unwrap()[0].id;
    a.set_link_active(link, false).unwrap();
    a.add_document_link(x, y, "relacionado", "De novo").unwrap();
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    let c = cards(&b);
    assert_eq!(c.len(), 1, "two candidates: a card");
    assert!(matches!(
        c[0].mutation,
        crate::intranet::IntranetMutation::SetLinkActive { active: false, .. }
    ));
    let mut both = links_of(&a, X);
    both.sort();
    assert_eq!(
        both,
        vec![(false, "Vínculo de A".into()), (true, "De novo".into())]
    );

    // No candidate.
    let (_dir, mut a, mut b) = link_derived_apart();
    let target = |id| crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id,
    };
    let y = doc(&a, Y).unwrap();
    a.trash_item(target(y)).unwrap();
    a.purge_trash_item(target(y)).unwrap();
    sync(&mut a);
    sync(&mut b);
    assert_eq!(cards(&b).len(), 1, "no candidate: a card");
}

// -------------------------------------------- the history of a record --

/// v2 F3.1a "dossier history `created_at, id`": the versions of a record
/// are listed newest first by the one order of versions (adendo §8: the
/// moment captured, then stored, then identity), not by number. A version
/// captured earlier and imported later is listed after the newer one.
///
/// Red on `f7eff3b`: the history is ordered by number.
#[test]
fn f3_1a_history_follows_the_order_of_versions() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    let mut newer = capture(X, "Versão nova.");
    newer.captured_at = "2026-11-20T10:00:00Z".into();
    let mut older = capture(X, "Versão antiga.");
    older.captured_at = "2026-11-10T10:00:00Z".into();
    let doc = store.import_capture(&newer).unwrap().document_id;
    store.import_capture(&older).unwrap();
    let history: Vec<String> = store
        .capture_versions(doc)
        .unwrap()
        .into_iter()
        .map(|v| v.capture.text)
        .collect();
    assert_eq!(history, vec!["Versão nova.", "Versão antiga."]);
}

// ------------------------------------- a numbered edit of a manual record --

fn manual(title: &str, text: &str) -> crate::manual::ManualFields {
    crate::manual::ManualFields {
        kind: "ligacao".into(),
        title: title.into(),
        origin: "Gabinete".into(),
        document_date: String::new(),
        text: text.into(),
    }
}

fn by_title(store: &Store, title: &str) -> i64 {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.title == title)
        .unwrap_or_else(|| panic!("sem “{title}”"))
        .id
}

/// The (title, text) of every version of record `id`, newest first.
fn versions(store: &Store, id: i64) -> Vec<(String, String)> {
    store
        .capture_versions(id)
        .unwrap()
        .into_iter()
        .map(|v| (v.capture.title, v.capture.text))
        .collect()
}

fn version(title: &str, text: &str) -> (String, String) {
    (title.to_owned(), text.to_owned())
}

const MX: &str = "Registro de A";
const MY: &str = "Registro de B";

/// [`crossed`] with manual records: X made on A and Y on B, offline, at the
/// same number. After A, B and A exchange, 42 is X on A and Y on B.
fn crossed_manual() -> (tempfile::TempDir, Store, Store, i64) {
    let (dir, _path, mut a, mut b) = two();
    assert!(mapped(&a) && mapped(&b), "both libraries are mapped");
    let x = a
        .create_manual_document(&manual(MX, "Texto de A"))
        .unwrap()
        .document_id;
    let y = b
        .create_manual_document(&manual(MY, "Texto de B"))
        .unwrap()
        .document_id;
    assert_eq!(x, y, "the same number on both");
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    assert_eq!(by_title(&a, MX), x, "A keeps X at its number");
    assert_eq!(by_title(&b, MY), y, "B keeps Y at its number");
    assert_eq!(by_title(&a, MY), by_title(&b, MX), "crossed");
    (dir, a, b, x)
}

/// revisao-f3-1a-ii #1. B edits Y (its record at 42, a numbered manual
/// edit, which names no item by a proof) on the crossed pair. The journal
/// of the copy B last adopted says "the change that made X created 42"
/// in the copy's numbers; 42 is Y here. The edit lands on Y, on both
/// devices; X keeps its one version, byte for byte; no card.
///
/// Red on `bcafb9b`: the own queue's replay absorbs that journal into the
/// translation keyed by this library's numbers, so the edit of Y is taken
/// for a creation that went to the copy's 42 (X) and is written on X.
#[test]
fn f3_1a_a_manual_edit_never_follows_a_copy_number_of_another_record() {
    let (_dir, mut a, mut b, held) = crossed_manual();
    let pair = |s: &Store| (by_title(s, MX), by_title(s, MY));
    let (on_a, on_b) = (pair(&a), pair(&b));
    let x_before = versions(&a, on_a.0);
    b.update_manual_document(held, &manual(MY, "Texto de B, editado"))
        .unwrap();
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for (store, (x, y)) in [(&a, on_a), (&b, on_b)] {
        assert_eq!(versions(store, x), x_before, "X untouched");
        assert_eq!(
            versions(store, y),
            vec![
                version(MY, "Texto de B, editado"),
                version(MY, "Texto de B")
            ],
            "Y has B's edit"
        );
        assert!(cards(store).is_empty(), "{:?}", cards(store));
    }
    assert_eq!(on_b.1, held, "still Y on B");
}

/// The mirror: B edits X (A's record, at the other number here, which is
/// Y's number in the copy). The edit lands on X, Y is untouched.
///
/// Red on `bcafb9b`, as above: the copy's "the change that made Y created
/// 43" is read with this library's 43, which is X.
#[test]
fn f3_1a_a_manual_edit_of_the_other_record_of_the_pair_stays_on_it() {
    let (_dir, mut a, mut b, _held) = crossed_manual();
    let pair = |s: &Store| (by_title(s, MX), by_title(s, MY));
    let (on_a, on_b) = (pair(&a), pair(&b));
    let y_before = versions(&a, on_a.1);
    b.update_manual_document(on_b.0, &manual(MX, "Texto de A, editado em B"))
        .unwrap();
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for (store, (x, y)) in [(&a, on_a), (&b, on_b)] {
        assert_eq!(versions(store, y), y_before, "Y untouched");
        assert_eq!(
            versions(store, x),
            vec![
                version(MX, "Texto de A, editado em B"),
                version(MX, "Texto de A")
            ],
            "X has B's edit"
        );
        assert!(cards(store).is_empty(), "{:?}", cards(store));
    }
}

/// The same edit queued behind its own creation: on the crossed pair, B
/// makes a new record W offline and edits it before exchanging. The edit
/// follows W's creation in this very exchange; neither record of the pair
/// gets it.
///
/// Guard on `bcafb9b`: the outcome of a creation replayed in the same
/// exchange is read before the translation (`creations` first), and W's
/// number here is above every number of the copy's journal.
#[test]
fn f3_1a_a_manual_edit_behind_its_own_creation_follows_it() {
    let (_dir, mut a, mut b, _held) = crossed_manual();
    let pair = |s: &Store| (by_title(s, MX), by_title(s, MY));
    let (on_a, on_b) = (pair(&a), pair(&b));
    let (x_before, y_before) = (versions(&a, on_a.0), versions(&a, on_a.1));
    let w = b
        .create_manual_document(&manual("Registro W", "Texto de W"))
        .unwrap()
        .document_id;
    b.update_manual_document(w, &manual("Registro W", "Texto de W, editado"))
        .unwrap();
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for (store, (x, y)) in [(&a, on_a), (&b, on_b)] {
        assert_eq!(versions(store, x), x_before, "X untouched");
        assert_eq!(versions(store, y), y_before, "Y untouched");
        assert_eq!(
            versions(store, by_title(store, "Registro W")),
            vec![
                version("Registro W", "Texto de W, editado"),
                version("Registro W", "Texto de W")
            ]
        );
        assert!(cards(store).is_empty(), "{:?}", cards(store));
    }
    assert_eq!(by_title(&b, "Registro W"), w, "B keeps its number of W");
}

// ----------------------------------------------------- coverage guard --

/// What the replays of a copy's changes made of its records (the copy's
/// numbers) says nothing of this library's records of a mapped kind, whose
/// numbers are its own: a record only here, at the copy's number of
/// another one, is still missing from the result.
///
/// Red on `f7eff3b`: the copy's replays are absorbed for every kind, so the
/// record here is taken as found and would be lost without a copy aside.
#[test]
fn f3_1a_a_copy_s_replays_never_find_a_mapped_item_here() {
    use crate::coverage::{dropped, Check, LibraryHolder, Replayed, ResultIndex};
    use crate::idmap::{CreatedId, IdKind};
    let _kinds = kinds(RECORDS);
    let (dir, _path, mut here, _b) = two();
    assert!(mapped(&here), "this library numbers the records on its own");
    let only_here = here
        .import_capture(&capture(X, "Só aqui."))
        .unwrap()
        .document_id;
    let mut there = Store::open(&dir.path().join("la.sqlite3")).unwrap();
    let other = there
        .import_capture(&capture(Y, "Na cópia."))
        .unwrap()
        .document_id;
    let result = there.export_snapshot().unwrap();
    let mut of_copy = Replayed::default();
    of_copy.learn(
        &[CreatedId {
            kind: IdKind::Document,
            slot: String::new(),
            id: only_here,
        }],
        &[CreatedId {
            kind: IdKind::Document,
            slot: String::new(),
            id: other,
        }],
    );
    let mut replayed_here = Replayed::default();
    replayed_here.absorb(&of_copy, &here.conn).unwrap();
    let index = ResultIndex::of(&result);
    let lost = |replayed: Option<&Replayed>| {
        dropped(
            &LibraryHolder::new(&here.conn).unwrap(),
            &Check {
                result: &index,
                explained: &[],
                same_numbering: true,
                replayed,
            },
        )
        .unwrap()
    };
    // The record (and what hangs from it) is missing, with or without the
    // copy's replays.
    let without = lost(None);
    assert!(without.iter().any(|l| l.contains(X)), "{without:?}");
    assert_eq!(lost(Some(&replayed_here)), without);
}

// ------------------------------------- every reference of the family --

/// What a place of a copy names (`f1-inventario.md` §A).
#[derive(Clone, Copy, Debug)]
enum Names {
    /// An item of this ident kind (a row's own number included).
    Kind(&'static str),
    /// An item of the kind the sibling field says (by that field's value).
    By(&'static str, fn(&str) -> Option<&'static str>),
    /// The row of the version row, by its kind (the first of the triple).
    Version,
    /// A source name: `C{c}`, `C{c}F{f}P{p}[OCR]`, `A{a}H{hash}`.
    Source,
    /// No item of an ident kind (a row of a uid kind, a counter): kept.
    Nothing,
}

fn trash_kind(kind: &str) -> Option<&'static str> {
    crate::idmap::IdKind::from_trash(kind).map(crate::idents::kind_of)
}

fn owner_kind(kind: &str) -> Option<&'static str> {
    crate::idmap::IdKind::from_owner(kind).map(crate::idents::kind_of)
}

fn ref_owner_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "draft" => Some("draft"),
        "analysis" => Some("analysis"),
        _ => None,
    }
}

/// Every place of a copy that holds the number of an item (or a name made
/// of numbers), by its path in the copy (`[]`: any element), from the
/// inventory: §A.1 (document), §A.2 (capture), §A.4–A.6 (note, draft,
/// analysis), §A.7 (file), §A.10 (link), §A.11 (task), §A.12 (attachment),
/// §A.13 (atendimento, batch), §A.14 (example; the uid kinds' rows), §A.15
/// (trash, versions, capture refs, team events).
const PLACES: &[(&str, Names)] = &[
    (".documents[].id", Names::Kind("document")),
    (".documents[].latest_capture_id", Names::Kind("capture")),
    (".captures[].id", Names::Kind("capture")),
    (".captures[].document_id", Names::Kind("document")),
    (".requests[].document_id", Names::Kind("document")),
    (".requests[].capture_id", Names::Kind("capture")),
    (".notes[].id", Names::Kind("note")),
    (".notes[].document_id", Names::Kind("document")),
    (".drafts[].id", Names::Kind("draft")),
    (".drafts[].document_id", Names::Kind("document")),
    (".drafts[].capture_id", Names::Kind("capture")),
    (".drafts[].materials[].id", Names::Source),
    (".drafts[].materials[].document_id", Names::Kind("document")),
    (".drafts[].materials[].capture_id", Names::Kind("capture")),
    (".drafts[].materials[].file_id", Names::Kind("file")),
    (".analyses[].id", Names::Kind("analysis")),
    (".analyses[].document_id", Names::Kind("document")),
    (".analyses[].capture_id", Names::Kind("capture")),
    (".analyses[].analysis.capture_id", Names::Kind("capture")),
    (".analyses[].analysis.materials[].id", Names::Source),
    (
        ".analyses[].analysis.materials[].document_id",
        Names::Kind("document"),
    ),
    (
        ".analyses[].analysis.materials[].capture_id",
        Names::Kind("capture"),
    ),
    (
        ".analyses[].analysis.materials[].file_id",
        Names::Kind("file"),
    ),
    (".analyses[].analysis.citations[].source_id", Names::Source),
    (".analyses[].analysis.facts[].source_id", Names::Source),
    (".tasks[].id", Names::Kind("task")),
    (".tasks[].document_id", Names::Kind("document")),
    (".tasks[].parent_task_id", Names::Kind("task")),
    (".tasks[].dependencies[]", Names::Kind("task")),
    (".work_contexts[][]", Names::Kind("document")),
    (".files_meta[].id", Names::Kind("file")),
    (".files_meta[].capture_id", Names::Kind("capture")),
    (".files_meta[].summary.id", Names::Kind("file")),
    (".page_texts[].file_id", Names::Kind("file")),
    (".document_links[].id", Names::Kind("link")),
    (".document_links[].source_id", Names::Kind("document")),
    (".document_links[].target_id", Names::Kind("document")),
    (".trash[].item_id", Names::By("kind", trash_kind)),
    (".trash[].document_id", Names::Kind("document")),
    (".capture_refs[].capture_id", Names::Kind("capture")),
    (
        ".capture_refs[].owner_id",
        Names::By("owner_kind", ref_owner_kind),
    ),
    (".capture_refs[].owner_document", Names::Kind("document")),
    (".item_versions[][]", Names::Version),
    (".team_events[].id", Names::Nothing),
    (".team_events[].document_id", Names::Kind("document")),
    (".saiop_batches[].id", Names::Kind("saiop_batch")),
    (".saiop_attendances[].attendance.id", Names::Kind("saiop")),
    (
        ".saiop_attendances[].attendance.batch_id",
        Names::Kind("saiop_batch"),
    ),
    (
        ".saiop_attendances[].attendance.document_id",
        Names::Kind("document"),
    ),
    (
        ".saiop_attendances[].attendance.task_ids[]",
        Names::Kind("task"),
    ),
    (".attachments[].id", Names::Kind("attachment")),
    (".attachments[].meta.id", Names::Kind("attachment")),
    (
        ".attachments[].owner_id",
        Names::By("owner_kind", owner_kind),
    ),
    (".attachments[].document_id", Names::Kind("document")),
    (".templates.templates[].id", Names::Nothing),
    (".templates.template_versions[].id", Names::Nothing),
    (".templates.template_versions[].template_id", Names::Nothing),
    (".templates.blocks[].id", Names::Nothing),
    (".templates.block_versions[].id", Names::Nothing),
    (".templates.block_versions[].block_id", Names::Nothing),
    (".templates.examples[].id", Names::Kind("example")),
    (".templates.examples[].value.id", Names::Kind("example")),
    (".templates.examples[].template_id", Names::Nothing),
    (".templates.generations[].id", Names::Nothing),
    (
        ".templates.generations[].document_id",
        Names::Kind("document"),
    ),
    (
        ".templates.generations[].capture_id",
        Names::Kind("capture"),
    ),
    (".templates.generations[].draft_id", Names::Kind("draft")),
    (
        ".templates.generations[].record.document_id",
        Names::Kind("document"),
    ),
    (
        ".templates.generations[].record.capture_id",
        Names::Kind("capture"),
    ),
    (
        ".templates.generations[].record.draft_id",
        Names::Kind("draft"),
    ),
    (
        ".templates.generations[].record.sources[].id",
        Names::Source,
    ),
    (
        ".templates.generations[].record.sources[].document_id",
        Names::Kind("document"),
    ),
    (
        ".templates.generations[].record.sources[].capture_id",
        Names::Kind("capture"),
    ),
    (
        ".templates.generations[].record.sources[].file_id",
        Names::Kind("file"),
    ),
    (".resources.resources[].id", Names::Nothing),
    (".resources.uses[].id", Names::Nothing),
    (".resources.uses[].document_id", Names::Kind("document")),
    (
        ".resources.uses[].value.document_id",
        Names::Kind("document"),
    ),
    (".resources.uses[].value.saiop_id", Names::Kind("saiop")),
    (".resources.uses[].value.task_id", Names::Kind("task")),
    (".revision", Names::Nothing),
];

/// A key whose number names a row: every such leaf of a copy must be a
/// place above (a field added later fails here until it is listed).
fn id_like(key: &str) -> bool {
    key == "id"
        || key.ends_with("_id")
        || key.ends_with("_ids")
        || key == "dependencies"
        || key == "owner_document"
}

/// The columns of a library that hold the number of a row of another
/// table (by name, `f1-inventario.md` "How completeness was ensured" 1):
/// each is carried by a place of the copy, or is local (never in a copy:
/// it keeps this library's numbers, or is rebuilt from what the copy
/// carries).
const COLUMNS: &[(&str, &str)] = &[
    ("analyses.capture_id", ".analyses[].capture_id"),
    ("analyses.document_id", ".analyses[].document_id"),
    ("attachments.document_id", ".attachments[].document_id"),
    ("attachments.owner_id", ".attachments[].owner_id"),
    ("capture_refs.capture_id", ".capture_refs[].capture_id"),
    ("capture_refs.owner_id", ".capture_refs[].owner_id"),
    (
        "capture_refs.owner_document",
        ".capture_refs[].owner_document",
    ),
    ("captures.document_id", ".captures[].document_id"),
    ("document_links.source_id", ".document_links[].source_id"),
    ("document_links.target_id", ".document_links[].target_id"),
    (
        "documents.latest_capture_id",
        ".documents[].latest_capture_id",
    ),
    (
        "draft_generations.capture_id",
        ".templates.generations[].capture_id",
    ),
    (
        "draft_generations.document_id",
        ".templates.generations[].document_id",
    ),
    (
        "draft_generations.draft_id",
        ".templates.generations[].draft_id",
    ),
    ("drafts.capture_id", ".drafts[].capture_id"),
    ("drafts.document_id", ".drafts[].document_id"),
    ("files.capture_id", ".files_meta[].capture_id"),
    ("item_versions.item_id", ".item_versions[][]"),
    ("notes.document_id", ".notes[].document_id"),
    ("page_text.file_id", ".page_texts[].file_id"),
    ("requests.capture_id", ".requests[].capture_id"),
    ("requests.document_id", ".requests[].document_id"),
    ("resource_uses.document_id", ".resources.uses[].document_id"),
    (
        "saiop_attendances.batch_id",
        ".saiop_attendances[].attendance.batch_id",
    ),
    (
        "saiop_attendances.document_id",
        ".saiop_attendances[].attendance.document_id",
    ),
    ("tasks.document_id", ".tasks[].document_id"),
    (
        "template_examples.template_id",
        ".templates.examples[].template_id",
    ),
    ("trash.document_id", ".trash[].document_id"),
    ("trash.item_id", ".trash[].item_id"),
    ("work_context.document_id", ".work_contexts[][]"),
    (
        "work_template_versions.template_id",
        ".templates.template_versions[].template_id",
    ),
    (
        "instruction_block_versions.block_id",
        ".templates.block_versions[].block_id",
    ),
];

/// Tables never carried by a copy, whose numbers are this library's own
/// (kept across a mapped adoption, which keeps the numbers of the items it
/// matches) or rebuilt after it (`current_pointer` by its triggers,
/// `item_ident` from the copy's triples).
const LOCAL: &[&str] = &[
    "attachment_texts",
    "carry_log",
    "current_pointer",
    "gone_documents",
    "id_high",
    "item_ident",
    "offline_id_map",
    "offline_watermark",
    "phone_operations",
    "saiop_auto_failures",
];

/// The copy `store` would publish, with every place above filled: the
/// sources of a draft, an analysis (citations and facts too) and a
/// generation of every grammar.
fn full_copy(store: &Store) -> CabinetSnapshot {
    let mut copy = store.export_with_idents(&fresh_ident()).unwrap();
    let file = copy.files_meta[0].clone();
    let file_doc = copy
        .captures
        .iter()
        .find(|c| c.id == file.capture_id)
        .unwrap()
        .document_id;
    let attachment = copy
        .attachments
        .iter()
        .find(|a| a.owner_kind == "document")
        .unwrap()
        .clone();
    let (doc, version) = (copy.drafts[0].document_id, copy.drafts[0].capture_id);
    let names = [
        format!("C{version}"),
        format!("C{}F{}P1", file.capture_id, file.id),
        format!("C{}F{}P2OCR", file.capture_id, file.id),
        format!("A{}H{}", attachment.id, "deadbeef"),
    ];
    let sources: Vec<crate::evidence::Source> = names
        .iter()
        .map(|name| {
            let in_file = name.contains('F');
            crate::evidence::Source {
                id: name.clone(),
                document_id: if in_file { file_doc } else { doc },
                capture_id: if in_file { file.capture_id } else { version },
                label: name.clone(),
                file_id: in_file.then_some(file.id),
                page: in_file.then_some(1),
                method: "captura".into(),
                text: "trecho".into(),
            }
        })
        .collect();
    copy.drafts[0].materials = sources.clone();
    let analysis = &mut copy.analyses[0].analysis;
    analysis.materials = sources.clone();
    analysis.citations = names
        .iter()
        .map(|n| crate::evidence::Citation {
            source_id: n.clone(),
            quote: "trecho".into(),
            claim: "afirmação".into(),
        })
        .collect();
    analysis.facts = names
        .iter()
        .map(|n| crate::model::Fact {
            id: "f1".into(),
            source_id: n.clone(),
            ..Default::default()
        })
        .collect();
    copy.templates.generations[0].record.sources = sources
        .iter()
        .map(|s| crate::work_templates::UsedSource {
            id: s.id.clone(),
            label: s.label.clone(),
            document_id: s.document_id,
            capture_id: s.capture_id,
            file_id: s.file_id,
        })
        .collect();
    copy
}

/// Walks `copy` and `mapped` side by side and checks every leaf: a place
/// of a kind in `set` holds the number `here` gives its item (the copy's
/// number of a kind outside it); a source name follows its numbers; any
/// other leaf is unchanged. Records which places were seen with an item.
fn check_leaf(
    path: &str,
    key: &str,
    parent: Option<&serde_json::Map<String, Value>>,
    triple: Option<&[Value]>,
    copy: &Value,
    mapped: &Value,
    expected: &dyn Fn(&'static str, i64) -> i64,
    seen: &mut BTreeSet<&'static str>,
    failures: &mut Vec<String>,
) {
    let place = PLACES.iter().find(|(p, _)| *p == path);
    if place.is_none() && copy.is_number() && (id_like(key) || path.ends_with("[][]")) {
        failures.push(format!("{path}: number of a row not in the list"));
        return;
    }
    let Some((name, names)) = place else {
        if copy != mapped {
            failures.push(format!("{path}: changed ({copy} → {mapped})"));
        }
        return;
    };
    let number = |kind: Option<&'static str>, v: &Value| -> Value {
        match (kind, v.as_i64()) {
            (Some(kind), Some(n)) if n > 0 => Value::from(expected(kind, n)),
            _ => v.clone(),
        }
    };
    let want = match names {
        Names::Kind(kind) => number(Some(kind), copy),
        Names::By(sibling, decode) => {
            let kind = parent
                .and_then(|p| p.get(*sibling))
                .and_then(Value::as_str)
                .and_then(|k| decode(k));
            number(kind, copy)
        }
        Names::Version => {
            let kind = triple
                .and_then(|t| t.first())
                .and_then(Value::as_str)
                .and_then(crate::mapper::row_kind);
            number(kind, copy)
        }
        Names::Source => match copy.as_str() {
            Some(s) => Value::String(source_name(s, expected)),
            None => copy.clone(),
        },
        Names::Nothing => copy.clone(),
    };
    if copy.as_i64().is_some_and(|n| n > 0) || copy.as_str().is_some_and(|s| !s.is_empty()) {
        seen.insert(name);
    }
    if &want != mapped {
        failures.push(format!("{path}: {copy} should be {want}, is {mapped}"));
    }
}

fn source_name(name: &str, expected: &dyn Fn(&'static str, i64) -> i64) -> String {
    let c = regex::Regex::new(r"^C(\d+)(?:F(\d+)P(\d+(?:OCR)?))?$").unwrap();
    let a = regex::Regex::new(r"^A(\d+)H([0-9a-fA-F]+)$").unwrap();
    let n = |s: &str, kind| expected(kind, s.parse().unwrap());
    if let Some(m) = c.captures(name) {
        let capture = n(&m[1], "capture");
        match (m.get(2), m.get(3)) {
            (Some(f), Some(p)) => format!("C{capture}F{}P{}", n(f.as_str(), "file"), p.as_str()),
            _ => format!("C{capture}"),
        }
    } else if let Some(m) = a.captures(name) {
        format!("A{}H{}", n(&m[1], "attachment"), &m[2])
    } else {
        name.to_owned()
    }
}

fn walk(
    path: &str,
    key: &str,
    parent: Option<&serde_json::Map<String, Value>>,
    triple: Option<&[Value]>,
    copy: &Value,
    mapped: &Value,
    expected: &dyn Fn(&'static str, i64) -> i64,
    seen: &mut BTreeSet<&'static str>,
    failures: &mut Vec<String>,
) {
    match (copy, mapped) {
        (Value::Object(c), Value::Object(m)) => {
            for (k, v) in c {
                // The identities are the mapping itself.
                if path.is_empty() && k == "idents" {
                    continue;
                }
                let other = m.get(k).unwrap_or(&Value::Null);
                walk(
                    &format!("{path}.{k}"),
                    k,
                    Some(c),
                    None,
                    v,
                    other,
                    expected,
                    seen,
                    failures,
                );
            }
        }
        (Value::Array(c), Value::Array(m)) => {
            if c.len() != m.len() {
                failures.push(format!("{path}: {} rows, then {}", c.len(), m.len()));
                return;
            }
            for (v, w) in c.iter().zip(m) {
                walk(
                    &format!("{path}[]"),
                    key,
                    parent,
                    Some(c),
                    v,
                    w,
                    expected,
                    seen,
                    failures,
                );
            }
        }
        _ => check_leaf(
            path, key, parent, triple, copy, mapped, expected, seen, failures,
        ),
    }
}

/// The copy of a library holding every kind, mapped for the kinds `set`
/// into that library after an epoch change (nothing kept: every item of
/// those kinds gets a number of its own here, above the floor). Every place
/// of the inventory must then hold the number here of the item it names.
fn every_place_follows(set: &'static [&'static str]) {
    let _kinds = kinds(set);
    let (_dir, _path, mut a, _b) = two();
    assert!(mapped(&a));
    crate::f1_inventory::fill(&mut a, "a");
    let copy = full_copy(&a);
    let tx = a.conn.unchecked_transaction().unwrap();
    let floor = crate::idents::id_floor(&tx).unwrap();
    let mapping = crate::mapper::plan(&tx, &copy, &floor).unwrap();
    let mapped = crate::mapper::apply(&copy, &mapping);
    drop(tx);
    // copy number → number here, by kind, through the identities.
    let before: BTreeMap<(String, String), i64> = copy
        .idents
        .iter()
        .map(|r| ((r.0.clone(), r.2.clone()), r.1))
        .collect();
    let mut numbers: BTreeMap<(String, i64), i64> = BTreeMap::new();
    for r in &mapped.idents {
        if let Some(copy_row) = before.get(&(r.0.clone(), r.2.clone())) {
            numbers.insert((r.0.clone(), *copy_row), r.1);
        }
    }
    let expected = |kind: &'static str, n: i64| -> i64 {
        if !set.contains(&kind) {
            return n;
        }
        let here = *numbers
            .get(&(kind.to_owned(), n))
            .unwrap_or_else(|| panic!("{kind} {n} is not in the copy"));
        assert_ne!(
            here, n,
            "{kind} {n}: every number moves after an epoch change"
        );
        here
    };
    let (copy_json, mapped_json) = (
        serde_json::to_value(&copy).unwrap(),
        serde_json::to_value(&mapped).unwrap(),
    );
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    walk(
        "",
        "",
        None,
        None,
        &copy_json,
        &mapped_json,
        &expected,
        &mut seen,
        &mut failures,
    );
    // The export leaves the copy of an example's number inside its value
    // at 0 (the row's number is `.templates.examples[].id`); a copy that
    // carries it has it translated all the same.
    let may_be_empty = [".templates.examples[].value.id"];
    let unseen: Vec<&str> = PLACES
        .iter()
        .filter(|(p, n)| {
            !matches!(n, Names::Nothing) && !seen.contains(p) && !may_be_empty.contains(p)
        })
        .map(|(p, _)| *p)
        .collect();
    assert!(
        unseen.is_empty(),
        "places the copy did not fill: {unseen:?}"
    );
    assert!(
        failures.is_empty(),
        "{} places not translated:\n{}",
        failures.len(),
        failures.join("\n")
    );

    // Every column that names a row of another table is carried by one of
    // the places above, or is local.
    let mut columns = Vec::new();
    let tables: Vec<String> = a
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for table in tables {
        let names: Vec<String> = a
            .conn
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') WHERE type LIKE 'INT%'"
            ))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for column in names {
            if column != "id" && id_like(&column) {
                columns.push(format!("{table}.{column}"));
            }
        }
    }
    let unlisted: Vec<&String> = columns
        .iter()
        .filter(|c| {
            let table = c.split('.').next().unwrap();
            !LOCAL.contains(&table) && !COLUMNS.iter().any(|(col, _)| col == c)
        })
        .collect();
    assert!(unlisted.is_empty(), "columns of no place: {unlisted:?}");
    for (_, place) in COLUMNS {
        assert!(
            PLACES.iter().any(|(p, _)| p == place),
            "{place} is not a place"
        );
    }
}

/// Frozen rules 23 and v2 §2.8 for the record family: every place of a
/// copy that names a document, a version, a note, a draft, an analysis or
/// a link — the columns, the copies of a number inside a payload (an
/// analysis's version, a generation's record, a use's value), the
/// polymorphic owners, the Lixeira, the version rows and the names of the
/// sources with their citations and facts — holds the number here of the
/// item it named in the copy, and the family is mapped on this build.
/// Driven by the inventory's list ([`PLACES`]), checked against the copy's
/// own fields and the library's columns: a place left out fails.
///
/// Red on `f7eff3b`: the mapper rewrites only the rows' own numbers, the
/// trash, the versions and the sources (`.captures[].document_id` and the
/// other references keep the copy's numbers), and `ADDRESSED` is empty.
#[test]
fn f3_1a_every_reference_of_the_records_is_translated() {
    every_place_follows(RECORDS);
    for kind in RECORDS {
        assert!(
            crate::mapper::ADDRESSED.contains(kind),
            "{kind} is not mapped on this build"
        );
    }
}

/// The same walk with every ident kind mapped (the families after F3.1a):
/// the mapper already follows each of their places, so the next families
/// only add their kinds.
///
/// Red on `f7eff3b`: as above, and the references to tasks, files,
/// attachments, atendimentos and batches keep the copy's numbers.
#[test]
fn f3_1a_every_reference_of_every_kind_is_translated() {
    const ALL: &[&str] = &[
        "document",
        "capture",
        "note",
        "draft",
        "analysis",
        "file",
        "link",
        "task",
        "attachment",
        "saiop",
        "saiop_batch",
        "example",
    ];
    for (kind, _) in crate::idents::KINDS {
        assert!(ALL.contains(&kind), "{kind} is not in the walk");
    }
    every_place_follows(ALL);
}
