//! Review of F3.0b-ii (`revisao-f3-0b-ii.md`): the card of a change of
//! another copy refused here keeps its change, base and proofs in one
//! frame (finding 1), and the reservation branch of the mapper (finding 5).
//! Written before the fix: each test says why it fails on `258b37a`.
#![allow(clippy::all)]

use crate::f3a2_tests::capture;
use crate::idents::IdentRow;
use crate::intranet::{ConflictResolution, IntranetConfig, IntranetMode};
use crate::storage::Store;
use std::path::PathBuf;

const KEY: &str = "senha-de-teste-123";

fn sync(store: &mut Store) {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap();
}

/// Exchanges `store` with only `path` of its configured mirrors.
fn sync_only(store: &mut Store, path: &PathBuf) {
    let full = store.load_intranet_config().unwrap();
    let mut narrow = full.clone();
    narrow.folder_files = vec![path.display().to_string()];
    store.save_intranet_config(&narrow).unwrap();
    sync(store);
    let mut after = store.load_intranet_config().unwrap();
    after.folder_files = full.folder_files;
    store.save_intranet_config(&after).unwrap();
}

fn task_version(store: &Store, id: i64) -> i64 {
    store
        .conn
        .query_row(
            "SELECT version FROM item_versions WHERE kind='task' AND item_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

fn task(store: &Store, uid: &str) -> crate::tasks::Task {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.uid == uid)
        .unwrap()
}

fn document_numbered(store: &Store, number: &str) -> i64 {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .unwrap_or_else(|| panic!("{number} is here"))
        .id
}

/// Finding 1. Two mirrors of one lineage part: on the pendrive, B creates
/// X and links task T to it; on the Drive, A creates Y (the same number X
/// has on the pendrive) and edits T. A then exchanges with both: the Drive
/// is the base, X is replayed under another number, and B's edit of T is
/// refused on T's version. The card shows the change as resolved here,
/// with proofs that agree with it: it resolves, and "Manter a minha" links
/// T to X.
///
/// Red on `258b37a`: the card keeps the change in the result's numbers but
/// the proofs in the pendrive's rows, so the listing finds them damaged
/// (unresolved) and "Manter a minha" is refused.
#[test]
fn f3_0b_foreign_refused_change_card_resolves_in_one_frame() {
    crate::idents::hooks::PROMOTION.with(|c| c.set(true));
    let dir = tempfile::tempdir().unwrap();
    let paths = [
        dir.path().join("Drive").join("ponte-sync.ponte"),
        dir.path().join("Pendrive").join("ponte-sync.ponte"),
    ];
    for path in &paths {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    let config = |member: &str| IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: paths.iter().map(|p| p.display().to_string()).collect(),
        folder_key: KEY.into(),
        ..IntranetConfig::default()
    };
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&config("Casa")).unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    let mut t = crate::tasks::Task::standalone();
    t.title = "Tarefa de todos".into();
    a.save_task(&t).unwrap();
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&config("Trabalho")).unwrap();
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    let uid = t.uid.clone();

    // The pendrive: B creates X and links T to it.
    let x = b
        .import_capture(&capture("SEFAZ-PRO-2026/03001", "Documento X."))
        .unwrap()
        .document_id;
    let on_b = task(&b, &uid);
    let version = task_version(&b, on_b.id);
    b.relink_task(on_b.id, Some(x), Some(version)).unwrap();
    sync_only(&mut b, &paths[1]);

    // The Drive: A creates Y (X's number there) and edits T, twice
    // exchanged so the Drive is the newer copy.
    let y = a
        .import_capture(&capture("SEFAZ-PRO-2026/03002", "Documento Y."))
        .unwrap()
        .document_id;
    assert_eq!(y, x, "Y has X's number");
    let mut on_a = task(&a, &uid);
    let version = task_version(&a, on_a.id);
    on_a.title = "Editada por A".into();
    a.save_task_checked(&on_a, Some(version)).unwrap();
    sync_only(&mut a, &paths[0]);
    a.save_note(y, "Nota de A em Y.").unwrap();
    sync_only(&mut a, &paths[0]);

    sync(&mut a);
    let x_here = document_numbered(&a, "SEFAZ-PRO-2026/03001");
    assert_ne!(x_here, x, "X is replayed under another number");
    let cards = a.list_sync_conflicts().unwrap();
    let card = cards
        .iter()
        .find(|c| {
            matches!(
                &c.mutation,
                crate::intranet::IntranetMutation::RelinkTask { .. }
            )
        })
        .unwrap_or_else(|| panic!("B's edit of T is a card: {cards:?}"));
    assert!(!card.unresolved, "its proofs agree with it: {card:?}");
    a.resolve_sync_conflict(card.id, ConflictResolution::KeepLocal)
        .unwrap();
    let now = task(&a, &uid);
    assert_eq!(now.document_id, Some(x_here), "T is linked to X");
    assert_eq!(now.title, "Editada por A", "A's edit stays");
}

/// Finding 5, with links mapped (kind set of the test): in the same epoch,
/// a link of the copy that is not here, numbered above the floor F, keeps
/// its number (reserved first); another, numbered below, is given the next
/// number above both: F+5 and F+6.
///
/// A guard on `258b37a` (the branch reads correct; no test reached it).
#[test]
fn f3_0b_reserved_copy_number_above_the_floor() {
    let _kinds = crate::f3b2_tests::kinds(&["link"]);
    let (_dir, _path, mut a, _b) = crate::f3a2_tests::two();
    let d1 = a.list_documents("").unwrap()[0].id;
    let d2 = a
        .import_capture(&capture("SEFAZ-PRO-2026/03011", "Outro."))
        .unwrap()
        .document_id;
    a.add_document_link(d1, d2, "relacionado", "Vínculo daqui")
        .unwrap();
    let here = crate::f3b2_tests::link_ids(&a)[0];
    let floor = crate::idents::id_floor(&a.conn).unwrap()["document_links"];
    let mut copy = crate::f3b2_tests::copy_of(&a);
    let model = copy.document_links[0].clone();
    copy.document_links.clear();
    copy.idents.retain(|IdentRow(kind, _, _)| kind != "link");
    copy.item_versions
        .retain(|(kind, _, _)| kind != "document_link");
    let (high, low) = (
        crate::f3b2_tests::fresh_ident(),
        crate::f3b2_tests::fresh_ident(),
    );
    for (id, ident, ends) in [(1, &low, (d1, d2)), (floor + 5, &high, (d2, d1))] {
        let mut link = model.clone();
        link.id = id;
        (link.source_id, link.target_id) = ends;
        copy.document_links.push(link);
        copy.idents.push(IdentRow("link".into(), id, ident.clone()));
    }
    copy.idents.sort();
    crate::f3b2_tests::adopt(&mut a, &copy);
    let row = |ident: &str| crate::f3b2_tests::row_of(&a, "link", ident);
    assert_eq!(row(&high), Some(floor + 5), "reserved: its own number");
    assert_eq!(
        row(&low),
        Some(floor + 6),
        "above the floor and the reserved"
    );
    assert!(
        !crate::f3b2_tests::link_ids(&a).contains(&here),
        "the link the copy does not have is retired"
    );
}
