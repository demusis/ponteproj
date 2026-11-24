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

// ------------------------------------------------- review fixes (c) --
// `revisao-f3-1b2.md` findings 2, 3 and 6, the library's side.

use crate::attachments::{AttachRequest, TargetGone};
use crate::intranet::{IntranetMutation, SyncConflict};
use base64::Engine;

const AFTER_A_CARD: &str = "Uma alteração anterior deste mesmo item não pôde ser aplicada e ficou em Conflitos de edição; esta, feita depois dela, também não foi aplicada, para não gravar sobre o que outro aparelho mudou. Confira as duas e refaça o que ainda for preciso.";

fn card_payload(store: &Store, card: i64) -> SyncConflict {
    let blob: Vec<u8> = store
        .conn
        .query_row(
            "SELECT payload FROM offline_conflicts WHERE id=?1",
            [card],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_slice(&crate::protection::unseal(&blob).unwrap()).unwrap()
}

fn write_payload(store: &Store, card: i64, held: &SyncConflict) {
    store
        .conn
        .execute(
            "UPDATE offline_conflicts SET payload=?1 WHERE id=?2",
            rusqlite::params![
                crate::protection::seal(&serde_json::to_vec(held).unwrap()).unwrap(),
                card
            ],
        )
        .unwrap();
}

fn data_of(held: &SyncConflict) -> String {
    match &held.mutation {
        IntranetMutation::AddAttachment { data_base64, .. } => data_base64.clone(),
        other => panic!("not a file: {other:?}"),
    }
}

/// A file for `document` refused at once (another library space): its card.
fn kept(store: &mut Store, document: i64, name: &str, data: &[u8], operation: &str) -> i64 {
    let error = store
        .attach_to_document(&AttachRequest {
            document_id: document,
            name,
            description: "",
            data,
            operation_id: Some(operation),
            expected_number: None,
            document_ident: None,
            library_space: Some("7ffffffffffffffe"),
        })
        .unwrap_err();
    error
        .downcast_ref::<TargetGone>()
        .unwrap_or_else(|| panic!("kept in a card: {error:#}"))
        .card_id
}

/// A file for `document` queued with its proof: the uid of its queue entry.
fn queued(store: &mut Store, document: i64, name: &str, data: &[u8], operation: &str) -> String {
    let (ident, space) = store.attach_proof(document).unwrap();
    store
        .attach_to_document(&AttachRequest {
            document_id: document,
            name,
            description: "",
            data,
            operation_id: Some(operation),
            expected_number: None,
            document_ident: Some(&ident),
            library_space: Some(&space),
        })
        .unwrap();
    store
        .conn
        .query_row(
            "SELECT outbox_uid FROM phone_operations WHERE operation_id=?1",
            [operation],
            |r| r.get(0),
        )
        .unwrap()
}

/// The card an exchange makes of queued change `uid` (a file for `document`).
fn late_card(
    store: &Store,
    uid: &str,
    document: i64,
    name: &str,
    data: &[u8],
    reason: &str,
) -> i64 {
    let conflict = SyncConflict {
        id: 0,
        created_at: "2026-11-24T10:00:00.000Z".into(),
        actor: "Celular".into(),
        item_label: format!("Anexo {name}"),
        local_summary: String::new(),
        host_summary: String::new(),
        reason: reason.into(),
        mutation: IntranetMutation::AddAttachment {
            owner_kind: "document".into(),
            owner_id: document,
            name: name.into(),
            description: String::new(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(data),
        },
        base: None,
        unresolved: false,
        identity: Vec::new(),
        mutation_uid: uid.into(),
        resolved_at: String::new(),
        refs: None,
    };
    crate::intranet::insert_conflict(&store.conn, &conflict)
        .unwrap()
        .expect("a new card")
}

/// Finding 6 (LOW). The recovery list is read from what the operation
/// keeps of its file (name and size): a card whose bytes cannot even be
/// decoded is still listed. A card resolved by "Descartar" lets its bytes
/// go in that transaction; only what tells it apart stays.
///
/// Red on `645c23f`: the list decodes every file (the damaged one is
/// missing) and a discarded card keeps the whole file.
#[test]
fn f3_1b2c_the_list_decodes_no_file_and_a_resolved_card_keeps_none() {
    let (_dir, _path, mut a, _b) = two();
    let x = a
        .import_capture(&capture("SEFAZ-PRO-2026/09101", "Documento X."))
        .unwrap()
        .document_id;
    let first = kept(&mut a, x, "foto.pdf", b"%PDF-1.4 foto", "op-1");
    let second = kept(&mut a, x, "foto2.pdf", b"%PDF-1.4 outra foto", "op-2");
    let mut damaged = card_payload(&a, first);
    if let IntranetMutation::AddAttachment { data_base64, .. } = &mut damaged.mutation {
        *data_base64 = "isto não é base64".into();
    }
    write_payload(&a, first, &damaged);

    let listed = a.refused_attachments().unwrap();
    let seen: Vec<(i64, String, usize)> = listed
        .iter()
        .map(|r| (r.card_id, r.name.clone(), r.size))
        .collect();
    assert_eq!(
        seen,
        vec![
            (first, "foto.pdf".to_owned(), 13),
            (second, "foto2.pdf".to_owned(), 19)
        ]
    );

    assert!(!data_of(&card_payload(&a, second)).is_empty());
    a.discard_card(second).unwrap();
    let resolved = card_payload(&a, second);
    assert!(!resolved.resolved_at.is_empty());
    assert!(
        !resolved.mutation_uid.is_empty(),
        "still tells the change apart"
    );
    assert_eq!(data_of(&resolved), "", "the file went with the card");
    assert_eq!(a.refused_attachments().unwrap().len(), 1);
}

/// Findings 2 and 3. A late refusal with the record alive says the card's
/// own reason (here, the change came after another card). An operation
/// written before this version (no queue uid, as the upgrade leaves it) is
/// bound to its card all the same, and listed.
///
/// Red on `645c23f`: the first says "não está mais disponível", the second
/// is not listed.
#[test]
fn f3_1b2c_a_late_refusal_says_why_and_an_older_operation_is_listed() {
    let (_dir, _path, mut a, _b) = two();
    let x = a
        .import_capture(&capture("SEFAZ-PRO-2026/09102", "Documento X."))
        .unwrap()
        .document_id;
    let uid = queued(&mut a, x, "foto.pdf", b"%PDF-1.4 foto", "op-3");
    let card = late_card(&a, &uid, x, "foto.pdf", b"%PDF-1.4 foto", AFTER_A_CARD);
    let record = a.operation_record("op-3").unwrap().unwrap();
    assert_eq!(
        (record.outcome.as_str(), record.card_id),
        ("refused", Some(card))
    );
    let listed = a.refused_attachments().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].operation_id, "op-3");
    assert_eq!(listed[0].message, AFTER_A_CARD, "the record is alive");

    let older = queued(&mut a, x, "foto2.pdf", b"%PDF-1.4 outra foto", "op-4");
    a.conn
        .execute(
            "UPDATE phone_operations SET outbox_uid=NULL, outcome='accepted' WHERE operation_id='op-4'",
            [],
        )
        .unwrap();
    let card = late_card(
        &a,
        &older,
        x,
        "foto2.pdf",
        b"%PDF-1.4 outra foto",
        "O registro não existe mais.",
    );
    let listed = a.refused_attachments().unwrap();
    assert_eq!(listed.len(), 2);
    let older = listed
        .iter()
        .find(|r| r.card_id == card)
        .expect("the older operation's card is listed");
    assert_eq!((older.name.as_str(), older.size), ("foto2.pdf", 19));
}
