//! F3.0b-i of Frente F: acceptance tests of the activation of the mapping
//! (`settle_mapping`), the id floor kept by the copy and the translation of
//! the answers by the proof of each id (`desenho-f3-v3-adendo.md` §2, §4,
//! §6 and §12; `desenho-f3-v2.md` §2.3–§2.5). Written before the code: each
//! test says why it fails on `bb6ea4e`, or that it is a guard.
#![allow(clippy::all)]

use crate::f3a2_tests::{capture, count, edit_last_queued, fsync, ident, note_of, two, Team};
use crate::idents::hooks;
use crate::storage::Store;

/// The value of `key` in the library's mapping state (0 when absent).
fn meta(store: &Store, key: &str) -> i64 {
    let kept: bool = store
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ident_meta')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if !kept {
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

/// As a library updated from F2b: identities adopted, never settled.
fn as_updated(store: &Store) {
    store
        .conn
        .execute_batch("DROP TABLE IF EXISTS ident_meta;")
        .unwrap();
    assert_eq!(meta(store, "mapped"), 0);
}

fn document_numbered(store: &Store, number: &str) -> Option<i64> {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .map(|d| d.id)
}

// ------------------------------------------- adendo §2 (R2-02) --

/// R2-02 / frozen rule 15. A client updated with an empty queue, and a main
/// computer unchanged since: one exchange asks the whole snapshot once, at
/// the same revision, takes its identities and settles the mapping; the
/// next one asks no whole snapshot again.
///
/// Red on `bb6ea4e` ([v2]): `UpToDate` adopts nothing and nothing ever
/// sets `mapped`.
#[test]
fn f3_0b_quiet_team_client_settles() {
    let mut t = Team::new();
    as_updated(&t.client);
    let full = hooks::rpc_count(&t.db, "SyncFull");
    t.client.sync_from_host(false).unwrap();
    assert_eq!(meta(&t.client, "mapped"), 1, "settled by one exchange");
    assert_eq!(hooks::rpc_count(&t.db, "SyncFull"), full + 1);
    t.client.sync_from_host(false).unwrap();
    assert_eq!(
        hooks::rpc_count(&t.db, "SyncFull"),
        full + 1,
        "no whole snapshot asked again"
    );
    assert_eq!(meta(&t.client, "mapped"), 1);
}

/// R2-02 rule 2. An updated client sends its last change of the old queue
/// (no proofs, a creation whose number is translated) and its adoption
/// fails right after: not settled, the translation kept. The next exchange
/// clears the translation and settles in the adoption itself.
///
/// Red on `bb6ea4e` ([v2]): activation is tied to no transaction (nothing
/// sets `mapped`).
#[test]
fn f3_0b_settle_commits_with_translation_clear() {
    let mut t = Team::new();
    as_updated(&t.client);
    t.offline();
    t.client
        .import_capture(&capture("SEFAZ-PRO-2026/00911", "Fila antiga."))
        .unwrap();
    edit_last_queued(&t.client, |e| e.refs = None);
    t.online();
    hooks::FAIL_TEAM_ADOPTION.with(|c| c.set(true));
    assert!(t.client.sync_from_host(false).is_err());
    assert!(
        !hooks::FAIL_TEAM_ADOPTION.with(|c| c.get()),
        "it failed there"
    );
    assert!(t.client.list_offline_outbox().unwrap().is_empty(), "sent");
    assert!(count(&t.client, "SELECT count(*) FROM offline_id_map") > 0);
    assert_eq!(meta(&t.client, "mapped"), 0);
    t.client.sync_from_host(false).unwrap();
    assert_eq!(count(&t.client, "SELECT count(*) FROM offline_id_map"), 0);
    assert_eq!(meta(&t.client, "mapped"), 1);
    assert!(document_numbered(&t.client, "SEFAZ-PRO-2026/00911").is_some());
}

/// R2-02 rule 2(b). The device that stamps the first 9/2 from its own
/// library settles, and so does the one that adopts it.
///
/// Red on `bb6ea4e` ([v2]): a first copy (`!replaced`) is no adoption.
#[test]
fn f3_0b_first_publish_settles() {
    let (_dir, _path, a, b) = two();
    assert_eq!(meta(&a, "mapped"), 1, "the device that stamped");
    assert_eq!(meta(&b, "mapped"), 1, "the device that adopted");
}

/// Frozen rule 4. Once the library is mapped, a queued change without
/// proofs is damaged: a card, never sent by number.
///
/// Red on `bb6ea4e`: the queue of a team client always takes the legacy
/// route for `refs: None`, and the note is written by number.
#[test]
fn f3_0b_refless_queue_entry_after_mapping_is_a_card() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    t.offline();
    t.client.save_note(doc, "Nota sem provas").unwrap();
    edit_last_queued(&t.client, |e| e.refs = None);
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert_ne!(note_of(&t.host, doc), "Nota sem provas");
    assert_eq!(t.cards(), 1, "{:?}", t.client.list_sync_conflicts());
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
}

// ------------------------------------------- adendo §4 (R2-04) --

/// R2-04 rule 2 / frozen rule 13. X is published at copy row x, then
/// removed by B (the copy's top row). A creates Y: the scratch library
/// takes the copy's `id_high`, so Y never gets x in the copy (nor here).
///
/// Red on `bb6ea4e`: the scratch library numbers from its own rows, and Y
/// takes x.
#[test]
fn f3_0b_copy_id_high_never_reuses_a_removed_copy_row() {
    let (_dir, _path, mut a, mut b) = two();
    let (nx, ny) = ("SEFAZ-PRO-2026/00921", "SEFAZ-PRO-2026/00922");
    a.import_capture(&capture(nx, "X")).unwrap();
    fsync(&mut a);
    fsync(&mut b);
    let x = document_numbered(&a, nx).unwrap();
    assert_eq!(document_numbered(&b, nx), Some(x));
    b.delete_document(x).unwrap();
    fsync(&mut b);
    fsync(&mut a);
    assert_eq!(document_numbered(&a, nx), None, "removed in the copy");
    a.import_capture(&capture(ny, "Y")).unwrap();
    fsync(&mut a);
    fsync(&mut b);
    let y = document_numbered(&a, ny).unwrap();
    assert_ne!(y, x, "a removed copy row is never given again");
    assert_eq!(document_numbered(&b, ny), Some(y));
}

// ------------------------------------------- adendo §6 (R2-06) --

/// R2-06 rule 5 / frozen rule 12. The answer of the main computer carries
/// no proof for the id of its reply: the change is acknowledged ("A
/// alteração foi aplicada.") with no id given back, and nothing else is
/// written; the copy arrives with the answer.
///
/// Red on `bb6ea4e` ([v2]): nothing defines a fallback, and the number of
/// the copy is given back.
#[test]
fn f3_0b_proofless_receipt_returns_no_ids() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    hooks::DROP_PROOF.arm(&t.db);
    let answer = t.client.add_attachment(
        crate::attachments::Owner::Document(doc),
        "sem-prova.pdf",
        "resposta sem prova",
        b"%PDF-1.4 anexo",
    );
    assert!(!hooks::DROP_PROOF.disarm(&t.db), "the proof was dropped");
    let message = answer.expect_err("no id given back").to_string();
    assert!(message.contains("A alteração foi aplicada"), "{message}");
    assert_eq!(count(&t.client, "SELECT count(*) FROM attachments"), 1);
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
}

/// R2-06 rule 4. Each id of an answer is given back as the row holding
/// its proof's identity here, by the kind its request names: an import
/// (document and version), a new task, an attachment.
///
/// Guard: green on `bb6ea4e`, where the numbers coincide; it keeps the
/// translation exact once they do not (F3.0b-ii).
#[test]
fn f3_0b_reply_ids_follow_their_proof() {
    let mut t = Team::new();
    let receipt = t
        .client
        .import_capture(&capture("SEFAZ-PRO-2026/00931", "Em linha."))
        .unwrap();
    let doc = receipt.document_id;
    assert_eq!(
        ident(&t.client, "document", doc),
        ident(&t.host, "document", doc)
    );
    assert_eq!(
        ident(&t.client, "capture", receipt.capture_id),
        ident(&t.host, "capture", receipt.capture_id)
    );
    let task = t
        .client
        .save_task(&crate::tasks::Task {
            title: "Tarefa em linha".into(),
            ..crate::tasks::Task::new(doc)
        })
        .unwrap();
    assert_eq!(ident(&t.client, "task", task), ident(&t.host, "task", task));
    let attachment = t
        .client
        .add_attachment(
            crate::attachments::Owner::Document(doc),
            "em-linha.pdf",
            "anexo em linha",
            b"%PDF-1.4 anexo",
        )
        .unwrap();
    assert_eq!(
        ident(&t.client, "attachment", attachment),
        ident(&t.host, "attachment", attachment)
    );
}
