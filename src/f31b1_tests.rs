//! F3.1b1 of Frente F: acceptance tests of the manual target
//! (`desenho-f3-v3-adendo.md` §9 R2-09 and §12 rules 18 and 23,
//! `desenho-f3-v2.md` §3 F3.1b): a numbered edit of a manual record names
//! its record by identity (`Refs::manual_target`), and every executor (the
//! library that made it, the copy being built in a folder exchange, the
//! main computer, a card re-applied) binds the write to that record inside
//! the import's own transaction, with the number read from that record. A
//! request id already on another record is refused, never taken for a
//! repeat. The native host writes by identity on a crossed pair. Written
//! before the code: each test says why it fails on `b1de851` (F3.1a-ii
//! with its review), or that it is a guard there.
#![allow(clippy::all)]

use crate::f31a2_tests::{
    by_title, cards, crossed_manual, manual, sync, version, versions, MX, MY,
};
use crate::f3a2_tests::{capture, edit_last_queued, ident, two, Team};
use crate::idrefs::Proof;
use crate::storage::test_numbers::{row_of_ident, swap_numbers};
use crate::storage::Store;
use std::cell::RefCell;
use std::rc::Rc;

/// Arms the window of R2-09 in this thread: `then` runs on the library the
/// next import writes, after its record was resolved and right before the
/// import's transaction begins.
fn before_import_tx(then: impl FnOnce(&rusqlite::Connection) + 'static) {
    crate::idents::hooks::before_import_tx(then);
}

/// Whether the window armed in this thread was used (and disarms it).
fn window_used() -> bool {
    crate::idents::hooks::BEFORE_IMPORT_TX.with(|h| h.borrow_mut().take().is_none())
}

/// Everything stored of record `id`, byte for byte: its row, its versions
/// and its requests.
fn rows(conn: &rusqlite::Connection, id: i64) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for sql in [
        "SELECT hex(identity_hash)||'|'||coalesce(latest_capture_id,'') FROM documents WHERE id=?1",
        "SELECT id||'|'||hex(content_hash)||'|'||hex(payload)||'|'||created_at FROM captures WHERE document_id=?1 ORDER BY id",
        "SELECT request_id||'|'||capture_id FROM requests WHERE document_id=?1 ORDER BY request_id",
    ] {
        let mut statement = conn.prepare(sql).unwrap();
        let found = statement
            .query_map([id], |r| {
                let value: rusqlite::types::Value = r.get(0)?;
                Ok(format!("{value:?}").into_bytes())
            })
            .unwrap();
        out.extend(found.map(Result::unwrap));
    }
    out
}

/// The record the last queued change of `store` names as its manual
/// target, by identity.
fn queued_target(store: &Store) -> Option<String> {
    let entry = store.list_offline_outbox().unwrap().pop()?;
    match entry.refs?.manual_target?.proof {
        Proof::Ident(ident) => Some(ident),
        _ => None,
    }
}

// ---------------------------------------------- the window (R2-09) --

/// Adendo §9, `f3_1b1_manual_target_bound_in_import_tx`. The person saves
/// an edit of X; between the moment X was read and the import's
/// transaction, X and Y swap numbers (Y now holds the number the form
/// read). The version goes to X, under the number X has now; Y is
/// byte-identical to what the window left. With X in the Lixeira, the
/// edit is refused and Y is untouched.
///
/// Red on `b1de851`: the import picks the record by the number
/// (`storage.rs:964-975`), Y, and `update_manual_document` notices only
/// after writing it (`manual.rs:232`).
#[test]
fn f3_1b1_manual_target_bound_in_import_tx() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
    let x = store
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    let y = store
        .create_manual_document(&manual(MY, "Texto de Y"))
        .unwrap()
        .document_id;
    let y_number = crate::storage::test_numbers::number_of(&store.conn, y);
    let left: Rc<RefCell<Vec<Vec<u8>>>> = Rc::default();
    let keep = left.clone();
    before_import_tx(move |conn| {
        swap_numbers(conn, x, y);
        *keep.borrow_mut() = rows(conn, y);
    });
    let saved = store.update_manual_document(x, &manual(MX, "Texto de X, editado"));
    assert!(window_used());
    let saved = saved.expect("the edit is saved on X");
    assert_eq!(saved.document_id, x);
    assert_eq!(rows(&store.conn, y), *left.borrow(), "Y byte-identical");
    assert_eq!(
        versions(&store, x),
        vec![
            version(MX, "Texto de X, editado"),
            version(MX, "Texto de X")
        ]
    );
    assert_eq!(
        crate::storage::test_numbers::number_of(&store.conn, x),
        y_number,
        "the number X has now, read from X"
    );

    // X in the Lixeira: refused, and Y (which holds X's old number) stays.
    store
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: x,
        })
        .unwrap();
    let y_before = rows(&store.conn, y);
    let refused = store.update_manual_document(x, &manual(MX, "Texto de X, de novo"));
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(rows(&store.conn, y), y_before, "Y untouched");
}

/// Frozen rule 18 on the copy a folder exchange builds. On the crossed pair
/// (42 is X on A and Y on B) B edits X; while B's queue is replayed onto
/// the copy, X and Y swap numbers there right before the import's
/// transaction. The version lands on X on both devices, Y keeps its one
/// version, no card; the queued envelope names X by identity.
///
/// Red on `b1de851`: the queued edit names no record (`manual_target` is
/// never captured), and the replay imports by the number, which is Y's.
#[test]
fn f3_1b1_folder_crossed_pair_edit_is_bound_by_identity() {
    let (_dir, mut a, mut b, _held) = crossed_manual();
    let pair = |s: &Store| (by_title(s, MX), by_title(s, MY));
    let (on_a, on_b) = (pair(&a), pair(&b));
    b.update_manual_document(on_b.0, &manual(MX, "Texto de A, editado em B"))
        .unwrap();
    let (ix, iy) = (ident(&b, "document", on_b.0), ident(&b, "document", on_b.1));
    let named = queued_target(&b);
    let named_as = Some(ix.clone());
    before_import_tx(move |conn| {
        swap_numbers(conn, row_of_ident(conn, &ix), row_of_ident(conn, &iy));
    });
    sync(&mut b);
    assert!(window_used());
    sync(&mut a);
    sync(&mut b);
    for (store, (x, y)) in [(&a, on_a), (&b, on_b)] {
        assert_eq!(
            versions(store, x),
            vec![
                version(MX, "Texto de A, editado em B"),
                version(MX, "Texto de A")
            ],
            "X has B's edit"
        );
        assert_eq!(versions(store, y), vec![version(MY, "Texto de B")]);
        assert!(cards(store).is_empty(), "{:?}", cards(store));
    }
    assert_eq!(named, named_as, "the queued edit named X by identity");
}

// ------------------------------------------------- the main computer --

/// A team client and its main computer, crossed: the client made manual
/// record X offline while the main computer made Y, both as the first
/// record of the year. After the exchange the client's number (row) of X
/// is the main computer's of Y. Answers (team, X here, Y here).
fn crossed_team() -> (Team, i64, i64) {
    let mut t = Team::new();
    t.client.sync_from_host(false).unwrap();
    assert!(crate::idents::is_mapped(&t.client.conn).unwrap());
    t.offline();
    let x = t
        .client
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    let y_there = t
        .host
        .create_manual_document(&manual(MY, "Texto de Y"))
        .unwrap()
        .document_id;
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(by_title(&t.client, MX), x, "the client keeps its X");
    assert_eq!(
        by_title(&t.host, MY),
        x,
        "crossed: the host's Y is that row"
    );
    let _ = y_there;
    let y = by_title(&t.client, MY);
    (t, x, y)
}

/// Frozen rule 18 on the main computer (`Mutate`). On the crossed team the
/// client edits X online; when the change arrives, X and Y swap numbers on
/// the main computer. X gets the version on both, Y is untouched.
///
/// Red on `b1de851`: the main computer imports by the number, which names
/// Y there, and the client's check fails after Y was written.
#[test]
fn f3_1b1_team_crossed_pair_edit_is_bound_by_identity() {
    let (mut t, x, y) = crossed_team();
    let (ix, iy) = (
        ident(&t.client, "document", x),
        ident(&t.client, "document", y),
    );
    crate::idents::hooks::arm_before_mutate(
        &t.db,
        Box::new(move |host: &mut Store| {
            let (x, y) = (row_of_ident(&host.conn, &ix), row_of_ident(&host.conn, &iy));
            swap_numbers(&host.conn, x, y);
        }),
    );
    let saved = t
        .client
        .update_manual_document(x, &manual(MX, "Texto de X, editado"));
    assert!(!crate::idents::hooks::before_mutate_armed(&t.db));
    assert_eq!(saved.expect("saved on X").document_id, x);
    t.client.sync_from_host(false).unwrap();
    for store in [&t.host, &t.client] {
        assert_eq!(
            versions(store, by_title(store, MX)),
            vec![
                version(MX, "Texto de X, editado"),
                version(MX, "Texto de X")
            ]
        );
        assert_eq!(
            versions(store, by_title(store, MY)),
            vec![version(MY, "Texto de Y")]
        );
    }
    assert_eq!(t.cards(), 0);
}

/// Adendo §9 rule 2. The client's edit of Y reached the main computer under
/// request r. A later edit of X arrives under the same request r, with Y's
/// number and content (two records that came with one request id). It is
/// refused and kept as a card: never answered as a repeat of Y's edit, and
/// neither record changes.
///
/// Red on `b1de851`: the main computer finds r on Y with the same number
/// and content and answers "already imported", so the edit of X vanishes
/// without a card.
#[test]
fn f3_1b1_request_on_another_document_is_refused() {
    let (mut t, x, y) = crossed_team();
    t.client
        .update_manual_document(y, &manual(MY, "Texto de Y, editado"))
        .unwrap();
    // Y's edit as the main computer stored it: its request, number and text.
    let y_edit = t.client.capture_versions(y).unwrap()[0].capture.clone();
    t.offline();
    t.client
        .update_manual_document(x, &manual(MX, "Texto de X, editado"))
        .unwrap();
    edit_last_queued(&t.client, |entry| {
        entry.mutation = crate::intranet::IntranetMutation::ImportCapture {
            capture: y_edit.clone(),
        };
    });
    t.online();
    let _ = t.client.sync_from_host(false);
    assert_eq!(t.cards(), 1, "the edit of X is a card");
    t.client.sync_from_host(false).unwrap();
    for store in [&t.host, &t.client] {
        assert_eq!(
            versions(store, by_title(store, MX)),
            vec![version(MX, "Texto de X")]
        );
        assert_eq!(
            versions(store, by_title(store, MY)),
            vec![
                version(MY, "Texto de Y, editado"),
                version(MY, "Texto de Y")
            ]
        );
    }
}

// ------------------------------------------------------------ cards --

/// Adendo §9 rule 3. B's edit of X becomes a card because X is in the
/// Lixeira in the copy (Y untouched). X comes back; on B, X and Y swap
/// numbers, so the card's number is Y's. "Manter a minha" re-applies the
/// edit with the card's target: the queued change names X by identity,
/// and X gets the version on both devices.
///
/// Red on `b1de851`: the re-applied change names no record (it goes by
/// the number it was readdressed to).
#[test]
fn f3_1b1_card_reapply_keeps_the_target() {
    let (_dir, _path, mut a, mut b) = two();
    let x_a = a
        .create_manual_document(&manual(MX, "Texto de X"))
        .unwrap()
        .document_id;
    a.create_manual_document(&manual(MY, "Texto de Y")).unwrap();
    sync(&mut a);
    sync(&mut b);
    let (x, y) = (by_title(&b, MX), by_title(&b, MY));
    b.update_manual_document(x, &manual(MX, "Texto de X, editado em B"))
        .unwrap();
    let doc = |id| crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id,
    };
    a.trash_item(doc(x_a)).unwrap();
    sync(&mut a);
    sync(&mut b);
    let card = cards(&b);
    assert_eq!(card.len(), 1, "X in the Lixeira: a card");
    assert_eq!(versions(&b, y), vec![version(MY, "Texto de Y")]);
    a.restore_item(doc(x_a)).unwrap();
    sync(&mut a);
    sync(&mut b);
    swap_numbers(&b.conn, x, y);
    b.resolve_sync_conflict(card[0].id, crate::intranet::ConflictResolution::KeepLocal)
        .unwrap();
    let named = queued_target(&b);
    let named_as = Some(ident(&b, "document", x));
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
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
    assert_eq!(named, named_as, "re-applied with the card's target");
}

// ------------------------------------------------------- native host --

/// v2 `f3_1_native_extras_on_crossed_host_and_local`. A team client holds X
/// at the row that is Y on the main computer. A native import of X (a new
/// version) with a note and a priority: the answer names X here, and the
/// note and the priority land on X on both computers; Y is unchanged.
///
/// Guard on `b1de851`: the receipt comes back in this library's numbers
/// (F3.0b) and the extras are sent with the proofs of X here (F3.0a2).
#[test]
fn f3_1b1_native_extras_on_a_crossed_pair() {
    const X: &str = "SEFAZ-PRO-2026/07101";
    const Y: &str = "SEFAZ-PRO-2026/07102";
    let mut t = Team::new();
    t.client.sync_from_host(false).unwrap();
    t.offline();
    let x = t
        .client
        .import_capture(&capture(X, "Documento X."))
        .unwrap()
        .document_id;
    t.host.import_capture(&capture(Y, "Documento Y.")).unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    let number = |s: &Store, n: &str| crate::f31a2_tests::doc(s, n).unwrap();
    assert_eq!(number(&t.client, X), x);
    assert_eq!(number(&t.host, Y), x, "crossed");
    let mut newer = capture(X, "Documento X, versão nova.");
    newer.request_id = uuid::Uuid::new_v4().to_string();
    let request = serde_json::json!({
        "protocol_version": 1,
        "action": "import_capture",
        "capture": newer,
        "extras": {"note": "Nota da extensão", "priority": "alta"},
    });
    let (reply, _) =
        crate::native::handle_request(&serde_json::to_vec(&request).unwrap(), &mut t.client);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["document_id"], x, "{reply}");
    assert_eq!(reply["extras_applied"], true, "{reply}");
    t.client.sync_from_host(false).unwrap();
    for store in [&t.host, &t.client] {
        let (x, y) = (number(store, X), number(store, Y));
        assert!(store
            .load_document(x)
            .unwrap()
            .note
            .contains("Nota da extensão"));
        assert_eq!(store.load_work_context(x).unwrap().priority, "alta");
        assert_eq!(store.load_document(y).unwrap().note, "");
        assert_ne!(store.load_work_context(y).unwrap().priority, "alta");
    }
}
