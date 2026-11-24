//! Review of F3.0b-i (`revisao-f3-0b-i.md`): each test reproduces a finding
//! or a note of that review, or a defect of the same class found while
//! fixing them, and says why it fails on `989309f` (F3.0b-ii), or that it
//! is a guard (green there, red when the code it guards is taken out).
#![allow(clippy::all)]

use crate::f3a2_tests::{count, edit_last_queued, fsync, note_of, two, Team};
use crate::idents::{hooks, test_support};
use crate::intranet::{IntranetMode, IntranetMutation, RpcRequest, RpcResponse};
use crate::storage::Store;
use std::path::Path;

/// The value of `key` in the library's mapping state (0 when absent).
fn meta(store: &Store, key: &str) -> i64 {
    if !table(store, "ident_meta") {
        return 0;
    }
    store
        .conn
        .query_row(
            "SELECT coalesce((SELECT value FROM ident_meta WHERE key=?1),0)",
            [key],
            |r| r.get(0),
        )
        .unwrap()
}

fn table(store: &Store, name: &str) -> bool {
    store
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [name],
            |r| r.get(0),
        )
        .unwrap()
}

/// As a library updated from F2b: identities adopted, never settled.
fn as_updated(store: &Store) {
    store
        .conn
        .execute_batch("DROP TABLE IF EXISTS ident_meta;")
        .unwrap();
    assert_eq!(meta(store, "mapped"), 0);
}

/// The library at `path`, put back at schema 21 and opened as a Ponte of
/// this build opens it when its update cannot be done (the free space
/// cannot be measured): it stays at 21 for this opening.
fn open_at_21(path: &Path) -> Store {
    test_support::downgrade_to_21(path).unwrap();
    hooks::FREE_SPACE.with(|h| *h.borrow_mut() = Some(Ok(0)));
    let store = Store::open(path).unwrap();
    hooks::FREE_SPACE.with(|h| h.borrow_mut().take());
    assert_eq!(
        test_support::user_version(path).unwrap(),
        21,
        "stayed at 21"
    );
    store
}

/// The client of `t`, left at schema 21 for this opening.
fn client_at_21(t: &mut Team) {
    let path = t._dir.path().join("client.sqlite3");
    let spare = Store::open(&t._dir.path().join("reserva.sqlite3")).unwrap();
    drop(std::mem::replace(&mut t.client, spare));
    t.client = open_at_21(&path);
}

/// The content of the library (its notes), changed here without any queue:
/// as work made in Local mode ("Só neste computador").
fn local_note(store: &mut Store, doc: i64, note: &str) {
    let cfg = store.load_intranet_config().unwrap();
    let mut local = cfg.clone();
    local.mode = IntranetMode::Local;
    store.save_intranet_config(&local).unwrap();
    store.save_note(doc, note).unwrap();
    store.save_intranet_config(&cfg).unwrap();
    assert!(store.list_offline_outbox().unwrap().is_empty());
}

// ----------------------------------------------------------- finding 1 --

/// Finding 1 (first way). An updated quiet client syncs once with the main
/// computer out of reach, then with it back, unchanged: the one full
/// snapshot of that revision is asked when it can be answered, and the
/// library settles.
///
/// Red on `989309f`: the try of the revision is spent before the request
/// leaves; the next exchange asks "since", gets `UpToDate` and never
/// settles.
#[test]
fn f3_0b_r1_settle_try_survives_an_unreachable_host() {
    let mut t = Team::new();
    as_updated(&t.client);
    t.offline();
    assert!(t.client.sync_from_host(false).is_err(), "out of reach");
    assert_eq!(meta(&t.client, "mapped"), 0);
    t.online();
    let full = hooks::rpc_count(&t.db, "SyncFull");
    t.client.sync_from_host(false).unwrap();
    assert_eq!(hooks::rpc_count(&t.db, "SyncFull"), full + 1);
    assert_eq!(meta(&t.client, "mapped"), 1, "settled when reached");
}

/// Finding 1 (second way). An updated quiet client whose copy predates the
/// projects asks the whole snapshot for them; at the same revision it
/// takes the projects and, the content then being the main computer's,
/// its identities: it settles in that same exchange.
///
/// Red on `989309f`: the same-revision snapshot returns through
/// `take_projects_from`, which never settles, and the try is spent.
#[test]
fn f3_0b_r1_settle_try_survives_a_stale_projects_cache() {
    let mut t = Team::new();
    as_updated(&t.client);
    t.client
        .conn
        .execute("UPDATE client_cache SET schema=20", [])
        .unwrap();
    assert!(t.client.projects_cache_stale());
    t.client.sync_from_host(false).unwrap();
    assert!(!t.client.projects_cache_stale(), "projects taken");
    assert_eq!(meta(&t.client, "mapped"), 1, "settled by the same exchange");
    let full = hooks::rpc_count(&t.db, "SyncFull");
    t.client.sync_from_host(false).unwrap();
    assert_eq!(
        hooks::rpc_count(&t.db, "SyncFull"),
        full,
        "nothing asked again"
    );
}

// ----------------------------------------------------------- finding 2 --

/// Finding 2. A client left at schema 21 for an opening, its main computer
/// at 22: an online change whose answer names ids (a task, an attachment)
/// is applied there and here, and gives back its ids (the library is
/// never mapped: its adoption is verbatim, the numbers are its own).
///
/// Red on `989309f`: `translate_reply` reads `item_ident`, which a library
/// at 21 does not have: the change applied on both sides answers an error.
#[test]
fn f3_0b_r2_reply_on_a_client_left_at_21() {
    let mut t = Team::new();
    client_at_21(&mut t);
    let doc = t.client.list_documents("").unwrap()[0].id;
    let task = t
        .client
        .save_task(&crate::tasks::Task {
            title: "Tarefa do acervo em 21".into(),
            ..crate::tasks::Task::new(doc)
        })
        .expect("the id given back");
    assert!(t
        .client
        .list_tasks()
        .unwrap()
        .iter()
        .any(|x| x.id == task && x.title == "Tarefa do acervo em 21"));
    let attachment = t
        .client
        .add_attachment(
            crate::attachments::Owner::Document(doc),
            "em-21.pdf",
            "anexo do acervo em 21",
            b"%PDF-1.4 anexo",
        )
        .expect("the id given back");
    assert_eq!(
        count(
            &t.client,
            &format!("SELECT count(*) FROM attachments WHERE id={attachment}")
        ),
        1
    );
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
}

/// Finding 2, the same class: a client at 21 can never settle (it has no
/// identity tables), so it asks no whole snapshot for it and writes no
/// mapping state into a library of schema 21.
///
/// Red on `989309f`: `settling` holds at 21, the whole snapshot is asked
/// and `set_meta` creates `ident_meta` there.
#[test]
fn f3_0b_r2_client_left_at_21_does_not_try_to_settle() {
    let mut t = Team::new();
    client_at_21(&mut t);
    let full = hooks::rpc_count(&t.db, "SyncFull");
    t.client.sync_from_host(false).unwrap();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(hooks::rpc_count(&t.db, "SyncFull"), full);
    assert!(!table(&t.client, "ident_meta"));
    assert_eq!(
        test_support::user_version(&t._dir.path().join("client.sqlite3")).unwrap(),
        21
    );
}

/// Finding 2, the same class, on the main computer: left at 21 for an
/// opening, it still serves an older Ponte (protocol V1). A change whose
/// answer names an id is applied once, its receipt kept: the same change
/// sent again (its answer lost) gives the same answer and is not applied
/// again. No table of schema 22 is made in that library.
///
/// Red on `989309f`: the proof now names the reply's ids and reads
/// `item_ident`; the receipt's transaction fails and is dropped, so the
/// repeat applies the change a second time (and `carry_log` is created).
#[test]
fn f3_0b_r2_host_left_at_21_keeps_the_receipt_of_an_older_ponte() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("principal.sqlite3");
    let doc = {
        let mut store = Store::open(&db).unwrap();
        store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id
    };
    let mut host = open_at_21(&db);
    let cfg = host.load_intranet_config().unwrap();
    let uid = uuid::Uuid::new_v4().to_string();
    let send = |host: &mut Store| {
        crate::intranet::process_rpc(
            host,
            &cfg,
            RpcRequest::Mutate {
                actor: "Ponte antigo".into(),
                mutation: IntranetMutation::SaveTask {
                    task: crate::tasks::Task {
                        title: "Tarefa de um Ponte antigo".into(),
                        ..crate::tasks::Task::new(doc)
                    },
                    expected_version: None,
                    no_project: false,
                },
                client_schema: 21,
                expected_generation: String::new(),
                mutation_uid: uid.clone(),
                idents: Vec::new(),
                refs: None,
                leaves: None,
            },
        )
    };
    let first = match send(&mut host).unwrap() {
        RpcResponse::Mutated { reply, .. } => reply,
        other => panic!("{other:?}"),
    };
    let again = match send(&mut host).unwrap() {
        RpcResponse::Mutated { reply, .. } => reply,
        other => panic!("{other:?}"),
    };
    assert_eq!(format!("{first:?}"), format!("{again:?}"));
    let made = host
        .list_tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.title == "Tarefa de um Ponte antigo")
        .count();
    assert_eq!(made, 1, "applied once");
    for name in ["carry_log", "ident_alias", "ident_meta", "item_ident"] {
        assert!(!table(&host, name), "{name} made at 21");
    }
    assert_eq!(test_support::user_version(&db).unwrap(), 21);
}

/// Finding 2, the same class, at the primitives: in a library left at 21
/// no row holds an identity, and reading one is no error.
///
/// Red on `989309f`: both read `item_ident` unconditionally.
#[test]
fn f3_0b_r2_identity_reads_on_a_library_left_at_21() {
    use crate::idrefs::IdentFrame;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    let doc = {
        let mut store = Store::open(&path).unwrap();
        store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id
    };
    let store = open_at_21(&path);
    assert_eq!(
        crate::idents::ident_of(&store.conn, "document", doc).unwrap(),
        None
    );
    assert_eq!(
        store
            .conn
            .row_of(crate::idmap::IdKind::Document, &"0".repeat(32))
            .unwrap(),
        None
    );
}

/// Finding 2, the same class, on a shared-folder device (the phone's
/// route): left at 21, it exchanges as before even once a kind is mapped
/// (the version keys of a mapped kind read identities).
///
/// Red on `989309f`: `version_map` reads `item_ident` for a mapped kind.
#[test]
fn f3_0b_r2_folder_device_left_at_21_with_a_mapped_kind() {
    let (dir, _path, mut a, b) = two();
    let path = dir.path().join("b.sqlite3");
    drop(b);
    let mut b = open_at_21(&path);
    let doc = a.list_documents("").unwrap()[0].id;
    a.save_note(doc, "Nota vista pelo acervo em 21").unwrap();
    fsync(&mut a);
    crate::mapper::hooks::ADDRESSED.with(|k| *k.borrow_mut() = Some(vec!["document"]));
    let report = {
        let cfg = b.load_intranet_config().unwrap();
        b.sync_with_folders(
            &cfg,
            "senha-de-teste-123",
            false,
            &crate::ai::Reporter::silent(),
        )
    };
    crate::mapper::hooks::ADDRESSED.with(|k| *k.borrow_mut() = None);
    report.expect("exchanged");
    assert_eq!(note_of(&b, doc), "Nota vista pelo acervo em 21");
}

// ----------------------------------------------------------- finding 3 --

/// Finding 3 / frozen rule 15. Content that differs at the same revision
/// (work made here in Local mode): the one full snapshot of that revision
/// is asked, nothing is taken, and the next exchange asks no full snapshot
/// again; a change of the main computer then settles in its adoption.
///
/// Guard: green on `989309f`; red when `settle_tried_revision` is taken
/// out (every exchange asks the whole snapshot again).
#[test]
fn f3_0b_r3_differing_content_asks_once_per_revision() {
    let mut t = Team::new();
    as_updated(&t.client);
    let doc = t.client.list_documents("").unwrap()[0].id;
    local_note(&mut t.client, doc, "Feita só neste computador");
    let full = hooks::rpc_count(&t.db, "SyncFull");
    t.client.sync_from_host(false).unwrap();
    assert_eq!(hooks::rpc_count(&t.db, "SyncFull"), full + 1);
    assert_eq!(meta(&t.client, "mapped"), 0, "nothing taken");
    assert_eq!(note_of(&t.client, doc), "Feita só neste computador");
    t.client.sync_from_host(false).unwrap();
    assert_eq!(
        hooks::rpc_count(&t.db, "SyncFull"),
        full + 1,
        "not asked again at this revision"
    );
    assert_eq!(meta(&t.client, "mapped"), 0);
    t.host.save_note(doc, "Mudou no principal").unwrap();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(note_of(&t.client, doc), "Mudou no principal");
    assert_eq!(meta(&t.client, "mapped"), 1, "settled in the adoption");
}

// ----------------------------------------------------------- finding 4 --

/// Finding 4 / frozen rule 4, on the folder route. A mapped device holds a
/// change without proofs in its own queue: a card that keeps the change as
/// it was made, never applied by number in the copy.
///
/// Guard: green on `989309f`; red when the mapped side of
/// `!use_refs || !is_mapped` on the folder route is taken out (the note is
/// then applied by number and no card is made).
#[test]
fn f3_0b_r4_refless_entry_of_a_mapped_folder_device_is_a_card() {
    let (_dir, _path, mut a, mut b) = two();
    assert_eq!(meta(&b, "mapped"), 1);
    let doc = b.list_documents("").unwrap()[0].id;
    b.save_note(doc, "Nota sem provas na pasta").unwrap();
    edit_last_queued(&b, |entry| entry.refs = None);
    let report = fsync(&mut b);
    assert_eq!(report.conflicts, 1, "{:?}", b.list_sync_conflicts());
    let cards = b.list_sync_conflicts().unwrap();
    assert_eq!(cards.len(), 1);
    assert!(
        matches!(&cards[0].mutation, IntranetMutation::SaveNote { note, document_id, .. }
            if note == "Nota sem provas na pasta" && *document_id == doc),
        "{:?}",
        cards[0].mutation
    );
    assert!(b.list_offline_outbox().unwrap().is_empty());
    fsync(&mut a);
    assert_ne!(note_of(&a, doc), "Nota sem provas na pasta", "not applied");
    assert_ne!(note_of(&b, doc), "Nota sem provas na pasta", "nor here");
}
