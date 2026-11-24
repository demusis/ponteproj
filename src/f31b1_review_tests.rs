//! Review of F3.1b1 (`revisao-f3-1b1.md`): each test reproduces a finding
//! or covers a path of risk the review listed (finding 3). Written before
//! the fix: each says whether it is red on `77e24b7` (the head of `f3-1b1`)
//! and why, or that it is a guard there.
//!
//! The contract: `desenho-f3-v3-adendo.md` §9 and §12 rules 6, 9, 18 and
//! 23, `desenho-f3-v2.md` §2.2 and §3 F3.1b, and the owner's decision Q2
//! (`plano-frente-f.md` §8): once the identities are renewed, a queued edit
//! of an existing item becomes a card with its text, never matched by its
//! number or by its request ids.
#![allow(clippy::all)]

use crate::f31a2_tests::{by_title, cards, manual, sync, version, versions, MX, MY};
use crate::f3a2_tests::{edit_last_queued, fsync, ident, two, Team};
use crate::idrefs::{Proof, TARGET_GONE};
use crate::intranet::{ConflictResolution, IntranetMutation, SyncConflict};
use crate::model::{MANUAL_ORIGIN, MANUAL_PENDING};
use crate::storage::test_numbers::{row_of_ident, swap_numbers};
use crate::storage::Store;
use std::path::{Path, PathBuf};

/// What the card of an edit from another epoch of identities says (the
/// record cannot be recognized: the codes were renewed since).
const RENEWED: &str = "códigos dos registros foram renovados";

const KEY: &str = "senha-de-teste-123";

fn doc(id: i64) -> crate::trash::Target {
    crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id,
    }
}

/// An identity no library holds.
fn unknown_ident() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// The proofs of the last queued change of `store` as a library in another
/// epoch of identities made them: another epoch, and its record under an
/// identity this epoch does not know (Q2: the identities were drawn again).
fn from_another_epoch(store: &Store) {
    edit_last_queued(store, |entry| {
        let refs = entry.refs.as_mut().expect("a change with proofs");
        refs.frame.epoch = unknown_ident();
        refs.manual_target.as_mut().expect("its record").proof = Proof::Ident(unknown_ident());
    });
}

/// Rewrites the card `id` of `store` as stored (not the local view).
fn rewrite_card(store: &Store, id: i64, edit: impl FnOnce(&mut SyncConflict)) {
    let blob: Vec<u8> = store
        .conn
        .query_row(
            "SELECT payload FROM offline_conflicts WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let mut card: SyncConflict =
        serde_json::from_slice(&crate::protection::unseal(&blob).unwrap()).unwrap();
    edit(&mut card);
    store
        .conn
        .execute(
            "UPDATE offline_conflicts SET payload=?1 WHERE id=?2",
            rusqlite::params![
                crate::protection::seal(&serde_json::to_vec(&card).unwrap()).unwrap(),
                id
            ],
        )
        .unwrap();
}

/// Whether any record of `store` has a version with `text`.
fn written_anywhere(store: &Store, text: &str) -> bool {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .any(|d| versions(store, d.id).iter().any(|(_, t)| t == text))
}

fn titles(store: &Store) -> Vec<String> {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .map(|d| d.title)
        .collect()
}

/// The newest version of record `id`, as an edit to send again: a numbered
/// manual capture under a new request id.
fn edit_of(store: &Store, id: i64, text: &str) -> crate::model::Capture {
    let mut capture = store.capture_versions(id).unwrap()[0].capture.clone();
    capture.request_id = uuid::Uuid::new_v4().to_string();
    capture.text = text.into();
    capture
}

/// X and Y made on A and exchanged: both devices hold them. Answers
/// (dir, A, B, X on A, X on B, Y on B).
fn x_and_y() -> (tempfile::TempDir, Store, Store, i64, i64, i64) {
    let (dir, _path, mut a, mut b) = two();
    let xa = a
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    a.create_manual_document(&manual(MY, "Texto de Y")).unwrap();
    sync(&mut a);
    sync(&mut b);
    let (xb, yb) = (by_title(&b, MX), by_title(&b, MY));
    (dir, a, b, xa, xb, yb)
}

// ------------------------------------------------ finding 1 (HIGH) --

/// Finding 1, scenario 1b (probe P2). A and B each create a manual record
/// under the SAME request id (the same WhatsApp conversation imported on
/// both), and B edits its own. B's creation is a duplicate in the copy, so
/// its record never gets there: B's edit becomes a card with the
/// `TARGET_GONE` words, and A's record keeps its one version on both.
///
/// Red on `77e24b7`: the folder compares the edit's epoch with the
/// temporary library's (always empty), takes the route of "another epoch",
/// follows the duplicate creation's receipt to A's record and writes B's
/// edit there, with no card.
#[test]
fn rev_f31b1_1_same_request_edit_never_lands_on_the_other_record() {
    let (_dir, _path, mut a, mut b) = two();
    let request = uuid::Uuid::new_v4().to_string();
    let mut ca = manual("Conversa em A", "Texto igual").capture(MANUAL_PENDING, "Casa");
    ca.request_id = request.clone();
    let mut cb = manual("Conversa em B", "Texto igual").capture(MANUAL_PENDING, "Trabalho");
    cb.request_id = request;
    let da = a.import_capture(&ca).unwrap().document_id;
    let xb = b.import_capture(&cb).unwrap().document_id;
    let ia = ident(&a, "document", da);
    b.update_manual_document(xb, &manual("Conversa em B", "Texto de B, editado"))
        .unwrap();
    sync(&mut a);
    let report = sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    assert_eq!(report.conflicts, 1, "B's edit is a card");
    for store in [&a, &b] {
        assert_eq!(
            versions(store, row_of_ident(&store.conn, &ia)),
            vec![version("Conversa em A", "Texto igual")],
            "A's record keeps its one version"
        );
        assert!(!written_anywhere(store, "Texto de B, editado"));
    }
    let card = cards(&b);
    assert_eq!(card.len(), 1, "{card:?}");
    assert!(card[0].reason.contains(TARGET_GONE), "{}", card[0].reason);
    assert!(!card[0].unresolved);
    assert!(cards(&a).is_empty());
}

/// Finding 1, scenario 1d (probe P3), and finding 3 item 1 in a folder.
/// B holds a queued edit of X; A deletes X for good. B's edit becomes a
/// card that says the record is gone (`TARGET_GONE`), not that the number
/// may have changed; Y is untouched on both.
///
/// Red on `77e24b7`: the card says "não foi possível ter certeza…" (the
/// route of another epoch, taken because the temporary library has none).
#[test]
fn rev_f31b1_1_record_deleted_for_good_in_the_copy_is_target_gone() {
    let (_dir, mut a, mut b, xa, xb, _yb) = x_and_y();
    b.update_manual_document(xb, &manual(MX, "Texto de X, editado em B"))
        .unwrap();
    a.trash_item(doc(xa)).unwrap();
    a.purge_trash_item(doc(xa)).unwrap();
    sync(&mut a);
    let report = sync(&mut b);
    assert_eq!(report.conflicts, 1);
    let card = cards(&b);
    assert_eq!(card.len(), 1, "{card:?}");
    assert!(card[0].reason.contains(TARGET_GONE), "{}", card[0].reason);
    assert!(!card[0].unresolved);
    sync(&mut a);
    for store in [&a, &b] {
        assert_eq!(
            versions(store, by_title(store, MY)),
            vec![version(MY, "Texto de Y")]
        );
        assert!(!titles(store).contains(&MX.to_owned()));
        assert!(!written_anywhere(store, "Texto de X, editado em B"));
    }
}

// ---------------------------------------- another copy's journal --

/// A and B of two shared files (a Drive and a pen drive), both mapped, with
/// X and Y made on A. Answers (dir, drive, pen, A, B).
fn two_files() -> (tempfile::TempDir, PathBuf, PathBuf, Store, Store) {
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
    a.create_manual_document(&manual(MX, "Texto de X")).unwrap();
    a.create_manual_document(&manual(MY, "Texto de Y")).unwrap();
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert!(crate::idents::is_mapped(&store.conn).unwrap());
    }
    (dir, drive, pen, a, b)
}

/// One exchange with only `files` in the list (no check of the guard's
/// notices: a record deleted on purpose may be one).
fn sync_in(store: &mut Store, files: &[&Path]) -> crate::intranet::FolderSyncReport {
    let mut cfg = store.load_intranet_config().unwrap();
    cfg.folder_files = files.iter().map(|p| p.display().to_string()).collect();
    store.save_intranet_config(&cfg).unwrap();
    fsync(store)
}

/// B's edit of X goes only to the pen drive; A writes more to the Drive,
/// which becomes the copy the next exchange of both files builds on, so
/// B's edit is replayed from another copy's journal.
fn edit_of_x_on_the_pen(a: &mut Store, b: &mut Store, drive: &Path, pen: &Path) {
    let xb = by_title(b, MX);
    b.update_manual_document(xb, &manual(MX, "Texto de X, editado em B"))
        .unwrap();
    sync_in(b, &[pen]);
    let sample = a
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.title != MX && d.title != MY)
        .unwrap()
        .id;
    for n in 0..3 {
        a.save_note(sample, &format!("enchimento {n}")).unwrap();
        sync_in(a, &[drive]);
    }
}

/// Finding 3 item 6 and finding 1 (line 6371): an edit with a target,
/// replayed from another copy's journal, whose record was deleted for good
/// in the copy being built. A card that says the record is gone.
///
/// Red on `77e24b7`: the epoch compared is the temporary library's, so
/// the card says "não foi possível ter certeza…".
#[test]
fn rev_f31b1_3_other_copy_journal_record_gone_is_target_gone() {
    let (_dir, drive, pen, mut a, mut b) = two_files();
    let xa = by_title(&a, MX);
    a.trash_item(doc(xa)).unwrap();
    a.purge_trash_item(doc(xa)).unwrap();
    edit_of_x_on_the_pen(&mut a, &mut b, &drive, &pen);
    let report = sync_in(&mut a, &[&drive, &pen]);
    assert_eq!(report.conflicts, 1, "{:?}", cards(&a));
    let card = cards(&a);
    assert_eq!(card.len(), 1, "{card:?}");
    assert!(card[0].reason.contains(TARGET_GONE), "{}", card[0].reason);
    assert!(!written_anywhere(&a, "Texto de X, editado em B"));
    assert_eq!(
        versions(&a, by_title(&a, MY)),
        vec![version(MY, "Texto de Y")]
    );
}

/// Finding 3 item 6: an edit with a target, replayed from another copy's
/// journal (its proofs republished there), is bound to its record by
/// identity in the copy being built: X and Y swap numbers right before
/// the import, and the version still lands on X.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_other_copy_journal_edit_is_bound_by_identity() {
    let (_dir, drive, pen, mut a, mut b) = two_files();
    let (ix, iy) = (
        ident(&a, "document", by_title(&a, MX)),
        ident(&a, "document", by_title(&a, MY)),
    );
    edit_of_x_on_the_pen(&mut a, &mut b, &drive, &pen);
    crate::idents::hooks::before_import_tx(move |conn| {
        swap_numbers(conn, row_of_ident(conn, &ix), row_of_ident(conn, &iy));
    });
    let report = sync_in(&mut a, &[&drive, &pen]);
    assert!(
        crate::idents::hooks::BEFORE_IMPORT_TX.with(|h| h.borrow_mut().take().is_none()),
        "the replay imported the edit"
    );
    assert_eq!(report.conflicts, 0, "{:?}", cards(&a));
    sync_in(&mut b, &[&drive, &pen]);
    sync_in(&mut a, &[&drive, &pen]);
    for store in [&a, &b] {
        assert_eq!(
            versions(store, by_title(store, MX)),
            vec![
                version(MX, "Texto de X, editado em B"),
                version(MX, "Texto de X")
            ]
        );
        assert_eq!(
            versions(store, by_title(store, MY)),
            vec![version(MY, "Texto de Y")]
        );
        assert!(cards(store).is_empty(), "{:?}", cards(store));
    }
}

// ---------------------------------------------- finding 2 (MEDIUM) --

/// Finding 2 and finding 3 item 3, in a folder (the device's own queue).
/// An edit whose proofs were made in another epoch of identities, whose
/// record that epoch does not know, becomes a card with its text (Q2),
/// even though the record's request ids still lead to X: X is untouched.
///
/// Red on `77e24b7`: the route of "another epoch" follows X's request ids
/// and writes the edit on X.
#[test]
fn rev_f31b1_2_folder_edit_from_another_epoch_is_a_card() {
    let (_dir, mut a, mut b, _xa, xb, _yb) = x_and_y();
    b.update_manual_document(xb, &manual(MX, "Texto de X, editado em B"))
        .unwrap();
    from_another_epoch(&b);
    let report = sync(&mut b);
    assert_eq!(report.conflicts, 1);
    let card = cards(&b);
    assert_eq!(card.len(), 1, "{card:?}");
    assert!(card[0].reason.contains(RENEWED), "{}", card[0].reason);
    sync(&mut a);
    for store in [&a, &b] {
        assert_eq!(
            versions(store, by_title(store, MX)),
            vec![version(MX, "Texto de X")]
        );
        assert!(!written_anywhere(store, "Texto de X, editado em B"));
    }
}

/// Finding 2 and finding 3 item 3, on a team (the drain to the main
/// computer): the same edit from another epoch is a card before anything
/// is sent; X is untouched on the main computer.
///
/// Red on `77e24b7`: the drain follows X's request ids there and the main
/// computer writes the edit on X.
#[test]
fn rev_f31b1_2_team_edit_from_another_epoch_is_a_card() {
    let mut t = Team::new();
    let x = t
        .client
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    t.client.sync_from_host(false).unwrap();
    t.offline();
    t.client
        .update_manual_document(x, &manual(MX, "Texto de X, editado"))
        .unwrap();
    from_another_epoch(&t.client);
    t.online();
    let _ = t.client.sync_from_host(false);
    let card = cards(&t.client);
    assert_eq!(card.len(), 1, "{card:?}");
    assert!(card[0].reason.contains(RENEWED), "{}", card[0].reason);
    assert_eq!(
        versions(&t.host, by_title(&t.host, MX)),
        vec![version(MX, "Texto de X")]
    );
    assert!(!written_anywhere(&t.host, "Texto de X, editado"));
}

/// Finding 2 on a card (`revalidated_manual_edit`): a card whose proofs
/// are from another epoch, whose record is not known here by identity, is
/// never re-applied to the record its request ids lead to. "Manter a
/// minha" is refused and X is untouched.
///
/// Red on `77e24b7`: in another epoch the card follows `conflict.identity`
/// (the request ids) to X and writes the edit there.
#[test]
fn rev_f31b1_2_card_from_another_epoch_is_not_reapplied_by_request() {
    let (_dir, mut a, mut b, xa, xb, _yb) = x_and_y();
    b.update_manual_document(xb, &manual(MX, "Texto de X, editado em B"))
        .unwrap();
    a.trash_item(doc(xa)).unwrap();
    sync(&mut a);
    sync(&mut b);
    let card = cards(&b);
    assert_eq!(card.len(), 1, "X in the Lixeira: a card");
    a.restore_item(doc(xa)).unwrap();
    sync(&mut a);
    sync(&mut b);
    rewrite_card(&b, card[0].id, |card| {
        let refs = card.refs.as_mut().expect("a card with proofs");
        refs.frame.epoch = unknown_ident();
        refs.manual_target.as_mut().expect("its record").proof = Proof::Ident(unknown_ident());
    });
    let done = b.resolve_sync_conflict(card[0].id, ConflictResolution::KeepLocal);
    assert!(done.is_err(), "{done:?}");
    assert!(b.list_offline_outbox().unwrap().is_empty());
    assert_eq!(
        versions(&b, by_title(&b, MX)),
        vec![version(MX, "Texto de X")]
    );
}

// --------------------------------------------- finding 3 (MEDIUM) --

/// Finding 3 item 1, on a team: the client holds a queued edit of X, and
/// the main computer deletes X for good. A card that says the record is
/// gone; Y is untouched.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_team_record_deleted_for_good_is_target_gone() {
    let mut t = Team::new();
    let x = t
        .client
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    t.client
        .create_manual_document(&manual(MY, "Texto de Y"))
        .unwrap();
    t.client.sync_from_host(false).unwrap();
    t.offline();
    t.client
        .update_manual_document(x, &manual(MX, "Texto de X, editado"))
        .unwrap();
    let x_host = by_title(&t.host, MX);
    t.host.trash_item(doc(x_host)).unwrap();
    t.host.purge_trash_item(doc(x_host)).unwrap();
    t.online();
    let _ = t.client.sync_from_host(false);
    let card = cards(&t.client);
    assert_eq!(card.len(), 1, "{card:?}");
    assert!(card[0].reason.contains(TARGET_GONE), "{}", card[0].reason);
    assert!(!card[0].unresolved);
    assert_eq!(
        versions(&t.host, by_title(&t.host, MY)),
        vec![version(MY, "Texto de Y")]
    );
    assert!(!written_anywhere(&t.host, "Texto de X, editado"));
}

/// Finding 3 item 2, on a team (probe P4): the main computer and the
/// client create a manual record under the same request id; the client
/// edits its own. Its creation is a duplicate there (it lends no
/// identity: rule 9), so its edit is a card, and the main computer's
/// record keeps its one version.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_team_edit_after_a_duplicate_creation_is_a_card() {
    let mut t = Team::new();
    t.offline();
    let request = uuid::Uuid::new_v4().to_string();
    let mut ch = manual("Conversa no principal", "Texto igual").capture(MANUAL_PENDING, "ANFITRIAO");
    ch.request_id = request.clone();
    let mut cc = manual("Conversa no cliente", "Texto igual").capture(MANUAL_PENDING, "SERVIDOR-A");
    cc.request_id = request;
    let on_host = t.host.import_capture(&ch).unwrap().document_id;
    let xc = t.client.import_capture(&cc).unwrap().document_id;
    t.client
        .update_manual_document(xc, &manual("Conversa no cliente", "Texto do cliente, editado"))
        .unwrap();
    t.online();
    let _ = t.client.sync_from_host(false);
    let _ = t.client.sync_from_host(false);
    assert_eq!(
        versions(&t.host, on_host),
        vec![version("Conversa no principal", "Texto igual")]
    );
    assert!(!written_anywhere(&t.host, "Texto do cliente, editado"));
    let card: Vec<_> = cards(&t.client)
        .into_iter()
        .filter(|c| c.reason.contains(TARGET_GONE))
        .collect();
    assert_eq!(card.len(), 1, "{:?}", cards(&t.client));
}

/// A card of B's edit of X (X was in the Lixeira in the copy), with X
/// back on both and, on B, X and Y swapped (the card's number is Y's).
/// Answers (dir, A, B, X on B, Y on B, the card).
fn card_of_x() -> (tempfile::TempDir, Store, Store, i64, i64, SyncConflict) {
    let (dir, mut a, mut b, xa, xb, yb) = x_and_y();
    b.update_manual_document(xb, &manual(MX, "Texto de X, editado em B"))
        .unwrap();
    a.trash_item(doc(xa)).unwrap();
    sync(&mut a);
    sync(&mut b);
    let mut card = cards(&b);
    assert_eq!(card.len(), 1, "X in the Lixeira: a card");
    assert!(!card[0].unresolved);
    a.restore_item(doc(xa)).unwrap();
    sync(&mut a);
    sync(&mut b);
    swap_numbers(&b.conn, xb, yb);
    (dir, a, b, xb, yb, card.remove(0))
}

fn has_the_edit(store: &Store) {
    assert_eq!(
        versions(store, by_title(store, MX)),
        vec![
            version(MX, "Texto de X, editado em B"),
            version(MX, "Texto de X")
        ],
        "X has the edit"
    );
    assert_eq!(
        versions(store, by_title(store, MY)),
        vec![version(MY, "Texto de Y")],
        "Y untouched"
    );
    assert!(cards(store).is_empty(), "{:?}", cards(store));
}

/// Finding 3 item 4: "Combinar" re-applies the edit with the card's
/// target, in a folder: the queued change names X by identity and X gets
/// the version on both devices.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_merge_keeps_the_target() {
    let (_dir, mut a, mut b, xb, _yb, card) = card_of_x();
    b.resolve_sync_conflict(card.id, ConflictResolution::Merge)
        .unwrap();
    let named = b
        .list_offline_outbox()
        .unwrap()
        .pop()
        .and_then(|e| e.refs?.manual_target)
        .map(|t| t.proof);
    assert_eq!(named, Some(Proof::Ident(ident(&b, "document", xb))));
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    has_the_edit(&a);
    has_the_edit(&b);
}

/// Finding 3 item 4: "Manter a minha" outside the queue mode (a library no
/// longer shared: `execute_bound`) writes to the card's record, X, by its
/// identity, not to Y, which holds the card's number.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_keep_mine_outside_the_queue_mode_keeps_the_target() {
    let (_dir, _a, mut b, _xb, _yb, card) = card_of_x();
    let mut cfg = b.load_intranet_config().unwrap();
    cfg.mode = crate::intranet::IntranetMode::Local;
    b.save_intranet_config(&cfg).unwrap();
    b.resolve_sync_conflict(card.id, ConflictResolution::KeepLocal)
        .unwrap();
    has_the_edit(&b);
}

/// Finding 3 item 4: "Combinar" outside the queue mode, the same.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_merge_outside_the_queue_mode_keeps_the_target() {
    let (_dir, _a, mut b, _xb, _yb, card) = card_of_x();
    let mut cfg = b.load_intranet_config().unwrap();
    cfg.mode = crate::intranet::IntranetMode::Local;
    b.save_intranet_config(&cfg).unwrap();
    b.resolve_sync_conflict(card.id, ConflictResolution::Merge)
        .unwrap();
    has_the_edit(&b);
}

/// Finding 3 item 4, on a team: "Manter a minha" sends the edit to the
/// main computer with the card's target. X and Y are swapped on the
/// client (the card's number is Y's here); X gets the version on both.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_team_keep_mine_keeps_the_target() {
    let mut t = Team::new();
    let x = t
        .client
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    t.client
        .create_manual_document(&manual(MY, "Texto de Y"))
        .unwrap();
    t.client.sync_from_host(false).unwrap();
    t.offline();
    t.client
        .update_manual_document(x, &manual(MX, "Texto de X, editado em B"))
        .unwrap();
    let x_host = by_title(&t.host, MX);
    t.host.trash_item(doc(x_host)).unwrap();
    t.online();
    let _ = t.client.sync_from_host(false);
    let card = cards(&t.client);
    assert_eq!(card.len(), 1, "X in the Lixeira: a card");
    assert!(!card[0].unresolved, "{card:?}");
    t.host.restore_item(doc(x_host)).unwrap();
    t.client.sync_from_host(false).unwrap();
    let (x, y) = (by_title(&t.client, MX), by_title(&t.client, MY));
    swap_numbers(&t.client.conn, x, y);
    t.client
        .resolve_sync_conflict(card[0].id, ConflictResolution::KeepLocal)
        .unwrap();
    t.client.sync_from_host(false).unwrap();
    has_the_edit(&t.host);
    has_the_edit(&t.client);
}

/// Finding 3 item 5: the refusal at the origin (`77e24b7`): a numbered
/// manual edit whose number names no record here, with no record named,
/// is refused with `TARGET_GONE` and nothing is written or queued. A
/// library whose identities were never shared (provisional) still queues
/// it the legacy way, without proofs.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_origin_refuses_an_edit_that_names_no_record() {
    let (_dir, _a, mut b, _xa, xb, _yb) = x_and_y();
    let cfg = b.load_intranet_config().unwrap();
    let number = "MANUAL-1999/99999998";
    let numbered = |text: &str| {
        let mut capture = edit_of(&b, xb, text);
        capture.document_number = number.into();
        capture.source_url = format!("{MANUAL_ORIGIN}{number}");
        capture
    };
    let (first, second) = (numbered("Sem registro"), numbered("Sem registro, legado"));
    let queued = b.list_offline_outbox().unwrap().len();
    let refused = b.remote_mutate(&cfg, IntranetMutation::ImportCapture { capture: first });
    let said = refused.expect_err("refused at the origin").to_string();
    assert!(said.contains(TARGET_GONE), "{said}");
    assert_eq!(b.list_offline_outbox().unwrap().len(), queued);
    assert!(crate::storage::doc_by_number(&b.conn, number)
        .unwrap()
        .is_none());

    b.conn
        .execute("UPDATE ident_state SET origin='provisional' WHERE id=1", [])
        .unwrap();
    b.remote_mutate(&cfg, IntranetMutation::ImportCapture { capture: second })
        .expect("a provisional library queues it the legacy way");
    let entry = b.list_offline_outbox().unwrap().pop().unwrap();
    assert!(entry.refs.is_none(), "{:?}", entry.refs);
}

/// Finding 3 item 7: an `ImportBundle` with no files of a numbered manual
/// record (what the native host queues) names its record and is bound to
/// it by identity in the copy: X and Y swap numbers right before the
/// import, and the version lands on X.
///
/// Guard on `77e24b7`.
#[test]
fn rev_f31b1_3_import_bundle_edit_is_bound_by_identity() {
    let (_dir, mut a, mut b, _xa, xb, yb) = x_and_y();
    let cfg = b.load_intranet_config().unwrap();
    let capture = edit_of(&b, xb, "Texto de X, editado em B");
    b.remote_mutate(
        &cfg,
        IntranetMutation::ImportBundle {
            capture,
            files: Vec::new(),
        },
    )
    .unwrap();
    let (ix, iy) = (ident(&b, "document", xb), ident(&b, "document", yb));
    let named = b
        .list_offline_outbox()
        .unwrap()
        .pop()
        .and_then(|e| e.refs?.manual_target)
        .map(|t| t.proof);
    assert_eq!(named, Some(Proof::Ident(ix.clone())));
    crate::idents::hooks::before_import_tx(move |conn| {
        swap_numbers(conn, row_of_ident(conn, &ix), row_of_ident(conn, &iy));
    });
    sync(&mut b);
    assert!(
        crate::idents::hooks::BEFORE_IMPORT_TX.with(|h| h.borrow_mut().take().is_none()),
        "the replay imported the edit"
    );
    sync(&mut a);
    sync(&mut b);
    has_the_edit(&a);
    has_the_edit(&b);
}

// ---------------------------------------------------- findings 4-6 --

/// Finding 4. A team client whose identities are still provisional (just
/// updated) edits X online; its change goes without proofs and the main
/// computer imports it by the number, which there is now Y's (X was
/// deleted for good and its number given again). The person is told: the
/// edit is not reported as saved on the record opened.
///
/// Red on `77e24b7`: the check of the receipt was removed, and the edit
/// is reported as saved.
#[test]
fn rev_f31b1_4_provisional_edit_written_elsewhere_is_reported() {
    let mut t = Team::new();
    let x = t
        .client
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    t.client.sync_from_host(false).unwrap();
    let x_host = by_title(&t.host, MX);
    t.host.trash_item(doc(x_host)).unwrap();
    t.host.purge_trash_item(doc(x_host)).unwrap();
    let y_host = t
        .host
        .create_manual_document(&manual(MY, "Texto de Y"))
        .unwrap()
        .document_id;
    assert_eq!(
        crate::storage::test_numbers::number_of(&t.host.conn, y_host),
        crate::storage::test_numbers::number_of(&t.client.conn, x),
        "X's number was given to Y there"
    );
    t.client
        .conn
        .execute("UPDATE ident_state SET origin='provisional' WHERE id=1", [])
        .unwrap();
    let saved = t
        .client
        .update_manual_document(x, &manual(MX, "Texto de X, editado"));
    let said = saved.expect_err("not reported as saved").to_string();
    assert!(said.contains("não corresponde ao documento aberto"), "{said}");
}

/// Finding 5. The record opened is named explicitly, but its number
/// selects no record here (`identity_hash` out of step with its version):
/// the change still carries its record, and the main computer writes X.
///
/// Red on `77e24b7`: `with_target` only replaces a target that exists, so
/// the change goes without one and the main computer refuses it as
/// damaged.
#[test]
fn rev_f31b1_5_explicit_target_is_carried_when_the_number_names_none() {
    let mut t = Team::new();
    let x = t
        .client
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    t.client.sync_from_host(false).unwrap();
    t.client
        .conn
        .execute(
            "UPDATE documents SET identity_hash=randomblob(32) WHERE id=?1",
            [x],
        )
        .unwrap();
    let saved = t
        .client
        .update_manual_document(x, &manual(MX, "Texto de X, editado"));
    assert!(saved.is_ok(), "{saved:?}");
    assert_eq!(
        versions(&t.host, by_title(&t.host, MX)),
        vec![
            version(MX, "Texto de X, editado"),
            version(MX, "Texto de X")
        ]
    );
}

/// Finding 6. A manual capture still to be numbered, whose number came
/// with a trailing space (the validation trims it), is a creation for
/// every module: it is not refused as an edit that names no record, and
/// it gets a number of its own.
///
/// Red on `77e24b7`: `remote_mutate_to` takes it for a numbered edit (no
/// trim) while the proofs take it for a creation (trim), and refuses it
/// with `TARGET_GONE`.
#[test]
fn rev_f31b1_6_pending_number_with_a_space_is_a_creation() {
    let (_dir, _path, mut a, mut b) = two();
    let cfg = b.load_intranet_config().unwrap();
    let mut capture = manual("Registro com espaço", "Texto").capture(MANUAL_PENDING, "Trabalho");
    capture.document_number = format!("{MANUAL_PENDING} ");
    let mutation = IntranetMutation::ImportCapture { capture };
    assert!(!crate::renumber::is_manual_edit(&mutation));
    b.remote_mutate(&cfg, mutation).expect("created");
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        let id = by_title(store, "Registro com espaço");
        let number = crate::storage::test_numbers::number_of(&store.conn, id);
        assert!(!number.contains(MANUAL_PENDING), "{number}");
        assert!(crate::model::is_manual_number(&number), "{number}");
    }
}
