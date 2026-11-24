//! A purge of the Lixeira met on another device (0.52.0, tested on real
//! devices): the same expired item purged by every computer's retention,
//! a purge replayed after the 0.51 → 0.52 update, a purge of an item
//! restored elsewhere, and the cards 0.52.0-teste already stored. A purge
//! whose effect already holds is no conflict; a purge never deletes a
//! record it cannot prove, nor a live one. Tests only.
#![allow(clippy::all)]

use crate::intranet::{
    ConflictResolution, IntranetConfig, IntranetMode, IntranetMutation, SyncConflict,
};
use crate::storage::Store;
use crate::trash::{Kind, Target};

const KEY: &str = "senha-de-teste-123";
const X: &str = "SEFAZ-PRO-2026/09001";
const Y: &str = "SEFAZ-PRO-2026/09002";

fn on(promotion: bool) {
    crate::idents::hooks::PROMOTION.with(|c| c.set(promotion));
}

fn config(member: &str, files: Vec<String>) -> IntranetConfig {
    IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: files,
        folder_key: KEY.into(),
        ..IntranetConfig::default()
    }
}

fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
}

fn doc(store: &Store, number: &str) -> Option<i64> {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .map(|d| d.id)
}

/// The document with `number`, live or in the Lixeira.
fn any_doc(store: &Store, number: &str) -> Option<i64> {
    doc(store, number).or_else(|| {
        store
            .trash_entries()
            .unwrap()
            .into_iter()
            .find(|e| e.target.kind == Kind::Document && e.name.contains(number))
            .map(|e| e.target.id)
    })
}

fn in_trash(store: &Store, id: i64) -> bool {
    crate::trash::contains(&store.conn, Kind::Document, id).unwrap()
}

fn target(id: i64) -> Target {
    Target {
        kind: Kind::Document,
        id,
    }
}

fn cards(store: &Store) -> Vec<SyncConflict> {
    store.list_sync_conflicts().unwrap()
}

fn purge_cards(store: &Store) -> Vec<SyncConflict> {
    cards(store)
        .into_iter()
        .filter(|c| matches!(c.mutation, IntranetMutation::PurgeTrash { .. }))
        .collect()
}

fn capture(number: &str, text: &str) -> crate::model::Capture {
    crate::f3a2_tests::capture(number, text)
}

/// Every item of the Lixeira of `store` deleted `days` ago.
fn age_trash(store: &Store, days: i64) {
    let at = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    store
        .conn
        .execute("UPDATE trash SET deleted_at=?1", [at])
        .unwrap();
}

struct Folder {
    _dir: tempfile::TempDir,
    drive: std::path::PathBuf,
    a: Store,
    b: Store,
}

/// A ("Principal") and B ("Notebook") on one shared file. With
/// `promotion`, the folder is at format 9 and both are mapped; without,
/// it stays at format 8 and both libraries are provisional (what 0.52.0
/// finds on its first opening of a 0.51.0 library). X and Y are in both
/// libraries, X in the Lixeira on both.
fn folder(promotion: bool) -> Folder {
    on(promotion);
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&config("Principal", files.clone()))
        .unwrap();
    a.import_capture(&capture(Y, "Documento Y.")).unwrap();
    let x = a
        .import_capture(&capture(X, "Documento X."))
        .unwrap()
        .document_id;
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&config("Notebook", files)).unwrap();
    sync(&mut b);
    if promotion {
        assert!(sync(&mut a).promoted, "A promotes");
        sync(&mut b);
        assert!(crate::idents::is_mapped(&a.conn).unwrap());
        assert!(crate::idents::is_mapped(&b.conn).unwrap());
    }
    a.trash_item(target(x)).unwrap();
    sync(&mut a);
    sync(&mut b);
    let xb = any_doc(&b, X).expect("X on B");
    assert!(in_trash(&b, xb), "X is in B's Lixeira");
    Folder {
        _dir: dir,
        drive,
        a,
        b,
    }
}

fn x_of(store: &Store) -> i64 {
    any_doc(store, X).expect("X here")
}

fn gone(store: &Store, number: &str) -> bool {
    any_doc(store, number).is_none()
}

// ----------------------------------------------------- the five cards --

/// The owner's cards, in steady state: the retention of each computer
/// purges the same expired X; A exchanges first, then B. B's purge finds
/// X already purged: no card anywhere, X gone on both.
#[test]
fn lixeira_same_expired_item_purged_by_two_computers_steady_state() {
    let Folder { mut a, mut b, .. } = folder(true);
    for store in [&a, &b] {
        age_trash(store, 40);
    }
    assert_eq!(a.purge_expired_trash(30).unwrap(), 1);
    assert_eq!(b.purge_expired_trash(30).unwrap(), 1);
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        assert!(
            purge_cards(store).is_empty(),
            "a purge already done elsewhere is no card: {:?}",
            purge_cards(store)
                .iter()
                .map(|c| &c.reason)
                .collect::<Vec<_>>()
        );
        assert!(gone(store, X));
        assert!(doc(store, Y).is_some(), "Y untouched");
    }
}

/// The same with "Apagar de vez" of the one item, by hand, on both.
#[test]
fn lixeira_same_item_purged_by_hand_on_two_computers_steady_state() {
    let Folder { mut a, mut b, .. } = folder(true);
    let (xa, xb) = (x_of(&a), x_of(&b));
    a.purge_trash_item(target(xa)).unwrap();
    b.purge_trash_item(target(xb)).unwrap();
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        assert!(purge_cards(store).is_empty(), "no card");
        assert!(gone(store, X));
    }
}

/// The owner's first exchange after the update: both computers opened
/// 0.52.0 on a 0.51.0 library (provisional: their changes carry no
/// proofs), the retention of each purged the same expired X before the
/// first exchange, and the folder is promoted by the first one that
/// exchanges. The other gets no card.
#[test]
fn lixeira_same_expired_item_purged_by_two_computers_across_the_update() {
    let Folder { mut a, mut b, .. } = folder(false);
    for store in [&a, &b] {
        age_trash(store, 40);
    }
    assert_eq!(a.purge_expired_trash(30).unwrap(), 1);
    assert_eq!(b.purge_expired_trash(30).unwrap(), 1);
    let queued = b.list_offline_outbox().unwrap();
    assert!(
        queued
            .iter()
            .any(|e| matches!(e.mutation, IntranetMutation::PurgeTrash { .. }) && e.refs.is_none()),
        "a provisional library queues the purge without proofs"
    );
    on(true);
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert!(
            purge_cards(store).is_empty(),
            "no card: {:?}",
            purge_cards(store)
                .iter()
                .map(|c| &c.reason)
                .collect::<Vec<_>>()
        );
        assert!(gone(store, X));
        assert!(doc(store, Y).is_some(), "Y untouched");
    }
}

/// A purge done on one computer still at format 8 (before the others
/// updated) and published; B purged the same X before its first exchange
/// with the update; B then promotes the folder. No card.
#[test]
fn lixeira_purge_published_before_the_others_updated() {
    let Folder { mut a, mut b, .. } = folder(false);
    let xa = x_of(&a);
    a.purge_trash_item(target(xa)).unwrap();
    sync(&mut a);
    let xb = x_of(&b);
    b.purge_trash_item(target(xb)).unwrap();
    on(true);
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert!(
            purge_cards(store).is_empty(),
            "no card: {:?}",
            purge_cards(store)
                .iter()
                .map(|c| &c.reason)
                .collect::<Vec<_>>()
        );
        assert!(gone(store, X));
    }
}

// ------------------------------------------- the card already stored --

/// The card 0.52.0-teste stored on B for its own purge of X, exactly as
/// the exchange built it (`card_of` / `build_sync_conflict`): the queued
/// entry, the reason, `unresolved` as the exchange set it. The entry
/// leaves the queue, as the exchange takes it.
fn store_card_of_queued_purge(store: &Store, reason: &str, unresolved: bool) -> SyncConflict {
    let entry = store
        .list_offline_outbox()
        .unwrap()
        .into_iter()
        .find(|e| matches!(e.mutation, IntranetMutation::PurgeTrash { .. }))
        .expect("a queued purge");
    store
        .conn
        .execute("DELETE FROM offline_outbox WHERE id=?1", [entry.id])
        .unwrap();
    let mut card =
        crate::intranet::build_sync_conflict(&entry, reason, None, crate::intranet::Named::InCopy);
    card.unresolved = unresolved;
    crate::intranet::insert_conflict(&store.conn, &card).unwrap();
    card
}

fn explained(error: &anyhow::Error) -> String {
    crate::intranet::explain_resolve_error(IntranetMode::SharedFolder, None, &error.to_string())
}

/// The owner's card: B's purge of X queued before its first exchange with
/// the update (no proofs), refused by 0.52.0-teste ("O item não está na
/// lixeira."), then B took the copy where A had purged X. "Usar só a minha
/// versão…" resolves it as done: the card goes, nothing else is deleted.
#[test]
fn lixeira_stuck_card_without_proofs_resolves_with_keep_mine() {
    let Folder { mut a, mut b, .. } = folder(false);
    let xa = x_of(&a);
    a.purge_trash_item(target(xa)).unwrap();
    let xb = x_of(&b);
    b.purge_trash_item(target(xb)).unwrap();
    let card = store_card_of_queued_purge(&b, "O item não está na lixeira.", false);
    assert!(card.refs.is_none(), "a card without proofs");
    on(true);
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    let stuck = purge_cards(&b);
    assert_eq!(stuck.len(), 1, "the stored card is listed");
    let docs_before = b.list_documents("").unwrap().len();
    let result = b.resolve_sync_conflict(stuck[0].id, ConflictResolution::KeepLocal);
    assert!(
        result.is_ok(),
        "keep mine resolves a purge already done: {:?}",
        result.as_ref().err().map(explained)
    );
    assert!(purge_cards(&b).is_empty(), "the card is gone");
    assert_eq!(b.list_documents("").unwrap().len(), docs_before);
    assert!(doc(&b, Y).is_some());
}

/// The same card from a mapped library (with proofs: 0.52.0-teste refused
/// it as unproven, and it lists as unresolved once X is purged here).
#[test]
fn lixeira_stuck_card_with_proofs_resolves_with_keep_mine() {
    let Folder { mut a, mut b, .. } = folder(true);
    let xa = x_of(&a);
    a.purge_trash_item(target(xa)).unwrap();
    let xb = x_of(&b);
    b.purge_trash_item(target(xb)).unwrap();
    let card = store_card_of_queued_purge(&b, crate::idrefs::UNPROVEN, true);
    assert!(card.refs.is_some(), "a card with proofs");
    sync(&mut a);
    sync(&mut b);
    let stuck = purge_cards(&b);
    assert_eq!(stuck.len(), 1);
    let result = b.resolve_sync_conflict(stuck[0].id, ConflictResolution::KeepLocal);
    assert!(
        result.is_ok(),
        "keep mine resolves it: {:?}",
        result.as_ref().err().map(explained)
    );
    assert!(purge_cards(&b).is_empty());
    assert!(gone(&b, X) && doc(&b, Y).is_some());
}

/// A stored purge card never deletes a record it does not prove: a card
/// whose proof names X (purged) and whose number is Y's here, and an
/// older card with that number and no proof, while Y is in the Lixeira.
#[test]
fn lixeira_card_never_purges_another_record() {
    let Folder { mut a, mut b, .. } = folder(true);
    let yb = doc(&b, Y).unwrap();
    b.trash_item(target(yb)).unwrap();
    sync(&mut b);
    sync(&mut a);
    let xa = x_of(&a);
    a.purge_trash_item(target(xa)).unwrap();
    sync(&mut a);
    let xb = x_of(&b);
    b.purge_trash_item(target(xb)).unwrap();
    let card = store_card_of_queued_purge(&b, crate::idrefs::UNPROVEN, true);
    sync(&mut b);
    let mut moved = card.clone();
    moved.mutation = IntranetMutation::PurgeTrash {
        kind: "document".into(),
        id: yb,
    };
    moved.refs = card.refs.clone().map(|mut refs| {
        refs.mutation[0].row = yb;
        refs
    });
    moved.mutation_uid = "outro-cartao".into();
    crate::intranet::insert_conflict(&b.conn, &moved).unwrap();
    let mut legacy = moved.clone();
    legacy.refs = None;
    legacy.unresolved = false;
    legacy.mutation_uid = "cartao-antigo".into();
    crate::intranet::insert_conflict(&b.conn, &legacy).unwrap();
    for card in purge_cards(&b) {
        let outcome = b.resolve_sync_conflict(card.id, ConflictResolution::KeepLocal);
        assert!(
            any_doc(&b, Y).is_some_and(|y| in_trash(&b, y)),
            "Y stays in the Lixeira ({:?})",
            outcome.as_ref().err().map(explained)
        );
    }
}

// ------------------------------------------------ restored elsewhere --

/// A restored X (live again on A) while B purged it: X is never deleted,
/// the card on B says it was restored on another device, and "Usar só a
/// minha versão…" does not delete it either, with words that say so.
#[test]
fn lixeira_purge_of_an_item_restored_elsewhere_never_deletes_it() {
    let Folder { mut a, mut b, .. } = folder(true);
    let xa = x_of(&a);
    a.restore_item(target(xa)).unwrap();
    sync(&mut a);
    let xb = x_of(&b);
    b.purge_trash_item(target(xb)).unwrap();
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        assert!(doc(store, X).is_some(), "X is live");
    }
    let c = purge_cards(&b);
    assert_eq!(c.len(), 1, "B is told");
    assert!(
        c[0].reason.contains("restaurado") && c[0].reason.contains("outro aparelho"),
        "{}",
        c[0].reason
    );
    let error = b
        .resolve_sync_conflict(c[0].id, ConflictResolution::KeepLocal)
        .expect_err("a live record is not deleted by a card");
    let said = explained(&error);
    assert!(said.contains("restaurado"), "{said}");
    assert!(!said.contains("está na Lixeira. Restaure-o"), "{said}");
    assert!(doc(&b, X).is_some(), "X is still live");
}

// ------------------------------------------------------ the words --

/// "O item não está na lixeira." is never told as "está na Lixeira".
#[test]
fn lixeira_not_in_the_trash_is_never_told_as_in_the_trash() {
    let say = |error: &str| {
        crate::intranet::explain_resolve_error(IntranetMode::SharedFolder, None, error)
    };
    let said = say("O item não está na lixeira.");
    assert!(!said.contains("está na Lixeira. Restaure-o"), "{said}");
    assert!(said.contains("não está na Lixeira"), "{said}");
    // The positive form is still told as such.
    let said = say("Documento na lixeira. Restaure-o antes de usar ou importar novamente.");
    assert!(said.contains("está na Lixeira. Restaure-o"), "{said}");
    // "Does not exist or is in the Lixeira" is not told as only one.
    let said = say("A tarefa pai não existe ou está na lixeira.");
    assert!(!said.contains("está na Lixeira. Restaure-o"), "{said}");
}

// -------------------------------------------------- an old copy --

/// An 8 written by a device still on 0.51.0 after the folder moved to 9,
/// with its own purge of X, which the base already purged: no card.
#[test]
fn lixeira_old_copy_purge_already_done_is_no_card() {
    use crate::sharedfolder;
    let Folder {
        mut a,
        mut b,
        drive,
        ..
    } = folder(true);
    let xa = x_of(&a);
    a.purge_trash_item(target(xa)).unwrap();
    sync(&mut a);
    let nine = sharedfolder::nine_path(&drive);
    let copy = match sharedfolder::read_any(&nine, KEY).unwrap() {
        Some(sharedfolder::Found::Copy(copy)) => copy,
        _ => panic!("no 9"),
    };
    let mut payload = copy.payload;
    payload.snapshot.idents.clear();
    payload.snapshot.ident_epoch.clear();
    for e in payload.journal.iter_mut() {
        e.idents.clear();
    }
    let purge = sharedfolder::JournalEntry {
        uid: "aparelho-antigo-apaga-x".into(),
        created_at: "2026-11-20T12:00:00Z".into(),
        actor: "Pessoa do aparelho antigo".into(),
        writer: "aparelho-antigo".into(),
        label: "Apagar de vez da lixeira".into(),
        mutation: IntranetMutation::PurgeTrash {
            kind: "document".into(),
            id: xa,
        },
        base: None,
        revision: 1000,
        created: Vec::new(),
        idents: Vec::new(),
        refs: None,
        legacy: false,
    };
    payload.applied.push(purge.uid.clone());
    payload.journal.push(purge);
    sharedfolder::write(
        &drive,
        KEY,
        &sharedfolder::WriteRequest {
            lineage: &copy.header.lineage,
            revision: copy.header.revision + 1,
            writer: "aparelho-antigo",
            writer_name: "Aparelho antigo",
            schema_version: 21,
        },
        &payload,
        &copy.blobs,
    )
    .unwrap();
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        assert!(
            purge_cards(store).is_empty(),
            "no card: {:?}",
            purge_cards(store)
                .iter()
                .map(|c| &c.reason)
                .collect::<Vec<_>>()
        );
        assert!(gone(store, X));
        assert!(doc(store, Y).is_some());
    }
}

// ---------------------------------------------- what still purges --

/// Guard: a stored purge card whose record is proved and still in the
/// Lixeira here is applied by "Usar só a minha versão…": that record, and
/// only it, goes for good, on both devices. (The card of a purge made here
/// whose exchange left X in this Lixeira, built from its proofs here.)
#[test]
fn lixeira_keep_mine_still_purges_the_proven_record_in_the_lixeira() {
    let Folder { mut a, mut b, .. } = folder(true);
    let xb = x_of(&b);
    let mutation = IntranetMutation::PurgeTrash {
        kind: "document".into(),
        id: xb,
    };
    let refs = crate::idrefs::Refs::capture(&b.conn, &mutation, None).unwrap();
    assert!(refs.is_some(), "proofs captured on B");
    let entry = crate::intranet::OutboxEntry {
        id: 0,
        created_at: "2026-11-24T10:00:00Z".into(),
        actor: "Notebook".into(),
        label: "Apagado de vez da lixeira".into(),
        mutation,
        base: None,
        uid: "purga-de-b".into(),
        created: Vec::new(),
        attempted: false,
        idents: Vec::new(),
        refs,
    };
    let mut card = crate::intranet::build_sync_conflict(
        &entry,
        crate::idrefs::UNPROVEN,
        None,
        crate::intranet::Named::InCopy,
    );
    card.unresolved = true;
    crate::intranet::insert_conflict(&b.conn, &card).unwrap();
    assert!(in_trash(&b, xb), "X is in B's Lixeira");
    let c = purge_cards(&b);
    assert_eq!(c.len(), 1);
    b.resolve_sync_conflict(c[0].id, ConflictResolution::KeepLocal)
        .unwrap();
    assert!(gone(&b, X), "X purged on B");
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        assert!(gone(store, X), "X purged");
        assert!(doc(store, Y).is_some(), "Y untouched");
        assert!(purge_cards(store).is_empty());
    }
}

/// Guard: a purge made on one computer only still reaches the others.
#[test]
fn lixeira_a_purge_on_one_computer_still_reaches_the_other() {
    let Folder { mut a, mut b, .. } = folder(true);
    let xa = x_of(&a);
    a.purge_trash_item(target(xa)).unwrap();
    sync(&mut a);
    sync(&mut b);
    assert!(gone(&b, X));
    assert!(cards(&b).is_empty() && cards(&a).is_empty());
}

// ------------------------------------------ main computer and client --

/// A team: the client purged X while out of reach of the main computer,
/// which purged it too. The client's queued purge is no card.
#[test]
fn lixeira_team_client_purge_already_done_on_the_main_computer() {
    let mut t = crate::f3a2_tests::Team::new();
    t.client.sync_from_host(false).unwrap();
    let x_host = t.host.list_documents("").unwrap()[0].id;
    t.host.trash_item(target(x_host)).unwrap();
    t.client.sync_from_host(false).unwrap();
    let x_client = t.client.trash_entries().unwrap()[0].target.id;
    t.offline();
    t.client.purge_trash_item(target(x_client)).unwrap();
    assert_eq!(t.client.list_offline_outbox().unwrap().len(), 1, "queued");
    t.host.purge_trash_item(target(x_host)).unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert!(
        purge_cards(&t.client).is_empty(),
        "no card: {:?}",
        purge_cards(&t.client)
            .iter()
            .map(|c| &c.reason)
            .collect::<Vec<_>>()
    );
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert!(t.client.trash_entries().unwrap().is_empty());
    assert!(t.host.trash_entries().unwrap().is_empty());
}

// ----------------------------------------------- two shared files --

/// Two files (Drive and a pen drive): A purges X through the Drive, B
/// through the pen drive; A then exchanges through both and replays B's
/// purge from the other copy. No card on anyone.
#[test]
fn lixeira_purge_from_another_copy_already_done_is_no_card() {
    let Folder {
        _dir,
        mut a,
        mut b,
        drive,
    } = folder(true);
    let pen = drive
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Pendrive")
        .join("ponte-sync.ponte");
    std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
    let only = |store: &mut Store, files: &[&std::path::Path]| {
        let mut cfg = store.load_intranet_config().unwrap();
        cfg.folder_files = files.iter().map(|p| p.display().to_string()).collect();
        store.save_intranet_config(&cfg).unwrap();
        sync(store)
    };
    only(&mut b, &[&drive, &pen]);
    only(&mut a, &[&drive, &pen]);
    let xa = x_of(&a);
    a.purge_trash_item(target(xa)).unwrap();
    only(&mut a, &[&drive]);
    let xb = x_of(&b);
    b.purge_trash_item(target(xb)).unwrap();
    only(&mut b, &[&pen]);
    only(&mut a, &[&drive, &pen]);
    only(&mut b, &[&drive, &pen]);
    for store in [&a, &b] {
        assert!(
            purge_cards(store).is_empty(),
            "no card: {:?}",
            purge_cards(store)
                .iter()
                .map(|c| &c.reason)
                .collect::<Vec<_>>()
        );
        assert!(gone(store, X));
        assert!(doc(store, Y).is_some());
    }
}

/// Guard: in a shared folder, "Apagar de vez" of an item no longer in the
/// Lixeira (a screen not refreshed yet) is refused here and never queued.
#[test]
fn lixeira_a_purge_of_an_item_out_of_the_lixeira_is_refused_and_not_queued() {
    let Folder { b, .. } = folder(true);
    let mut b = b;
    let yb = doc(&b, Y).unwrap();
    let queued = b.list_offline_outbox().unwrap().len();
    let error = b.purge_trash_item(target(yb)).unwrap_err().to_string();
    assert_eq!(error, "O item não está na lixeira.");
    assert_eq!(
        b.list_offline_outbox().unwrap().len(),
        queued,
        "nothing queued"
    );
    assert!(doc(&b, Y).is_some(), "Y untouched");
}
