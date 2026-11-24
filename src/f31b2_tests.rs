//! F3.1b2 of Frente F: acceptance tests of the library's own side of a photo
//! the phone took for a record (`desenho-f3-v3-adendo.md` §10 R2-10 and
//! §12 rules 19-23, `desenho-f3-v2.md` §2.3 "library_space"): a deletion is
//! proved only by a same-epoch adoption that retires a row with an identity
//! (`gone_documents`), and the library space changes whenever the frame of
//! the numbers does. The phone's half (refusal cards, outcomes and the
//! recovery actions) is in `mobile/rust`. Written before the code: each
//! test says why it fails on `77e24b7` (F3.1b1).
#![allow(clippy::all)]

use crate::f31a2_tests::doc;
use crate::f3a2_tests::{capture, fsync, ident, two};
use crate::f3b2_tests::{adopt, fresh_ident};
use crate::idents::IdentRow;
use crate::storage::Store;
use crate::trash::{Kind, Target};

/// The identities this library holds as proved deleted elsewhere.
fn gone(store: &Store) -> Vec<String> {
    let mut statement = store
        .conn
        .prepare("SELECT ident FROM gone_documents ORDER BY ident")
        .unwrap();
    statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// The library space as kept (`ident_meta`).
fn space(store: &Store) -> Option<i64> {
    crate::idents::meta(&store.conn, "library_space").unwrap()
}

fn purge(store: &mut Store, id: i64) {
    let target = || Target {
        kind: Kind::Document,
        id,
    };
    store.trash_item(target()).unwrap();
    store.purge_trash_item(target()).unwrap();
}

/// Adendo §10 rule 5 and frozen rule 21. B joins (its own library is
/// replaced: a new space, nothing proved deleted). A deletes X for good:
/// B's next adoption keeps the epoch and retires X, which had an identity,
/// so X is proved deleted elsewhere; the space stays, since every number
/// B held still names the same record or nothing. A row without an
/// identity proves nothing. An epoch change retires every row and proves
/// nothing, and gives a new space.
///
/// Red on `77e24b7`: there is no `gone_documents` and no space.
#[test]
fn f3_1b2_gone_only_after_a_same_epoch_adoption() {
    let (_dir, _path, mut a, mut b) = two();
    const X: &str = "SEFAZ-PRO-2026/09001";
    const Y: &str = "SEFAZ-PRO-2026/09002";
    let x = a
        .import_capture(&capture(X, "Documento X."))
        .unwrap()
        .document_id;
    let y = a
        .import_capture(&capture(Y, "Documento Y."))
        .unwrap()
        .document_id;
    fsync(&mut a);
    fsync(&mut b);
    assert!(gone(&b).is_empty(), "joining proves no deletion");
    let joined = space(&b).expect("B's library has a space");
    let ident_x = ident(&b, "document", doc(&b, X).unwrap());

    purge(&mut a, x);
    fsync(&mut a);
    fsync(&mut b);
    assert_eq!(doc(&b, X), None);
    assert_eq!(gone(&b), vec![ident_x.clone()]);
    assert_eq!(space(&b), Some(joined), "the same frame keeps the space");

    // Y has no identity here (a failure path of v2 limit 2): its
    // retirement proves nothing.
    b.conn
        .execute(
            "DELETE FROM item_ident WHERE kind='document' AND row_id=?1",
            [doc(&b, Y).unwrap()],
        )
        .unwrap();
    purge(&mut a, y);
    fsync(&mut a);
    fsync(&mut b);
    assert_eq!(doc(&b, Y), None);
    assert_eq!(gone(&b), vec![ident_x.clone()]);

    // An epoch change (Q2): every row is retired, nothing is proved.
    let kept = b.list_documents("").unwrap().len();
    assert!(kept > 0);
    let mut other = b.export_with_idents(&fresh_ident()).unwrap();
    for IdentRow(_, _, ident) in &mut other.idents {
        *ident = fresh_ident();
    }
    other.idents.sort();
    adopt(&mut b, &other);
    assert_eq!(b.list_documents("").unwrap().len(), kept);
    assert_eq!(gone(&b), vec![ident_x], "an epoch change says nothing");
    let renewed = space(&b).expect("still a space");
    assert_ne!(renewed, joined, "a new frame of numbers, a new space");
}
