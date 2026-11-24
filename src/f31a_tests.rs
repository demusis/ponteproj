//! F3.1a-i of Frente F: acceptance tests of the one comparator of versions
//! and the pointers on final identities (`desenho-f3-v3-adendo.md` §8,
//! frozen rule 17), and of the obligations the record kinds need before
//! they are mapped (`plano-frente-f.md` §9): a card of this library's own
//! creation explains its absence in this library, a row of a mapped kind
//! without identity is unprovable, a number without identity of a mapped
//! kind never holds, a mapped library never takes identities by number,
//! and a recovery library starts unmapped. Written before the code: each
//! test says why it fails on `258b37a`.
#![allow(clippy::all)]

use crate::coverage::{dropped, Check, LibraryHolder, Removed, ResultIndex};
use crate::f3a2_tests::{capture, two};
use crate::f3b2_tests::{adopt, copy_of, kinds};
use crate::idents::{IdentRow, SlotIdent};
use crate::idmap::{CreatedId, IdKind};
use crate::idrefs::{Proof, Ref, Refs};
use crate::intranet::{AnalysisRow, CabinetSnapshot, CaptureRow, IntranetMutation, NoteRow};
use crate::storage::Store;

fn same(c: char) -> String {
    std::iter::repeat(c).take(32).collect()
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

fn latest(store: &Store, doc: i64) -> i64 {
    store
        .conn
        .query_row(
            "SELECT latest_capture_id FROM documents WHERE id=?1",
            [doc],
            |r| r.get(0),
        )
        .unwrap()
}

/// `store`'s copy with two notes and two analyses of its first document
/// made at the same moment: "alta" (the higher identity) at `high` and
/// "baixa" at `low`.
fn tied_copy(store: &Store, high: i64, low: i64) -> CabinetSnapshot {
    let mut copy = copy_of(store);
    let doc = copy.documents[0].id;
    let version = copy.documents[0].latest_capture_id.unwrap();
    let at = "2026-11-23T10:00:00.000Z";
    for (id, text, note, analysis) in [
        (high, "alta", same('f'), same('e')),
        (low, "baixa", same('1'), same('2')),
    ] {
        copy.notes.push(NoteRow {
            id,
            document_id: doc,
            note: format!("nota {text}"),
            created_at: at.into(),
        });
        copy.analyses.push(AnalysisRow {
            id,
            document_id: doc,
            capture_id: version,
            analysis: crate::model::Analysis {
                capture_id: version,
                summary: format!("análise {text}"),
                created_at: at.into(),
                ..Default::default()
            },
            created_at: at.into(),
        });
        copy.idents.push(IdentRow("note".into(), id, note));
        copy.idents.push(IdentRow("analysis".into(), id, analysis));
    }
    copy.idents.sort();
    copy
}

// ------------------------------------------- adendo §8 (R2-08) --

/// R2-08 / frozen rule 17. Two notes and two analyses of one document were
/// made at the same moment; A and B hold them under numbers in inverse
/// order. On both, the persisted pointers and the reads choose by
/// (time, identity, number): the higher identity, the same item on both.
///
/// Red on `258b37a`: the note pointer and the read take the highest
/// number, and the analysis breaks the tie by number, so A and B differ.
#[test]
fn f3_1a_pointers_use_final_idents() {
    let (_dir, _path, mut a, mut b) = two();
    let doc = a.list_documents("").unwrap()[0].id;
    let on_a = tied_copy(&a, 100, 101);
    let on_b = tied_copy(&a, 101, 100);
    adopt(&mut a, &on_a);
    adopt(&mut b, &on_b);
    for store in [&a, &b] {
        let note = row_of(store, "note", &same('f'));
        let analysis = row_of(store, "analysis", &same('e'));
        assert_eq!(pointer(store, doc), (Some(note), Some(analysis)));
        let detail = store.load_document(doc).unwrap();
        assert_eq!(detail.note, "nota alta");
        assert_eq!(detail.analysis.unwrap().summary, "análise alta");
    }
}

/// R2-08. The pointers kept by an older build's triggers are read again
/// by this build's rule when the library opens: the repair of the
/// triggers recomputes them in the same transaction.
///
/// Red on `258b37a`: the triggers already have that build's text, and a
/// stale pointer stays.
#[test]
fn f3_1a_reopen_recomputes_stale_pointers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sqlite3");
    let doc = {
        let mut a = Store::open(&path).unwrap();
        let doc = a
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        a.save_note(doc, "Primeira.").unwrap();
        a.save_note(doc, "Segunda.").unwrap();
        a.conn
            .execute_batch(
                "DROP TRIGGER ponte_pointer_notes_insert;
                 CREATE TRIGGER ponte_pointer_notes_insert AFTER INSERT ON notes BEGIN SELECT 1; END;
                 UPDATE current_pointer SET note_id=NULL;",
            )
            .unwrap();
        doc
    };
    let a = Store::open(&path).unwrap();
    let newest: i64 = a
        .conn
        .query_row("SELECT max(id) FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pointer(&a, doc).0, Some(newest));
    assert_eq!(a.load_document(doc).unwrap().note, "Segunda.");
}

/// `store`'s copy with three versions of its first document: V1 (higher
/// identity) and V2 captured at the same moment, at `v1` and `v2`, and V3,
/// captured later and current, at 202.
fn versions_copy(store: &Store, v1: i64, v2: i64) -> CabinetSnapshot {
    let mut copy = copy_of(store);
    let doc = copy.documents[0].id;
    let model = copy.captures[0].clone();
    for (id, text, at, ident) in [
        (v1, "V1", "2030-01-01T10:00:00Z", same('d')),
        (v2, "V2", "2030-01-01T10:00:00Z", same('3')),
        (202, "V3", "2030-01-02T10:00:00Z", same('4')),
    ] {
        let mut capture = model.capture.clone();
        capture.text = format!("Texto {text}");
        capture.captured_at = at.into();
        copy.captures.push(CaptureRow {
            id,
            document_id: doc,
            content_hash: text.as_bytes().to_vec(),
            capture,
            created_at: at.into(),
        });
        copy.idents.push(IdentRow("capture".into(), id, ident));
    }
    copy.documents[0].latest_capture_id = Some(202);
    copy.idents.sort();
    copy
}

/// R2-08 / v2 F3.1a `f3_1_tied_captures_choose_the_same_current_version`.
/// V1 and V2 tie on time, numbered in inverse order on A and B; each
/// sends V3 (current) to the Lixeira. The version that takes over is the
/// same item on both: the higher identity.
///
/// Red on `258b37a`: `newest_capture` breaks the tie by number.
#[test]
fn f3_1a_tied_captures_choose_the_same_current_version() {
    let (_dir, _path, mut a, mut b) = two();
    let doc = a.list_documents("").unwrap()[0].id;
    let on_a = versions_copy(&a, 200, 201);
    let on_b = versions_copy(&a, 201, 200);
    adopt(&mut a, &on_a);
    adopt(&mut b, &on_b);
    for store in [&mut a, &mut b] {
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Capture,
                id: 202,
            })
            .unwrap();
        let v1 = row_of(store, "capture", &same('d'));
        assert_eq!(latest(store, doc), v1, "V1 takes over");
    }
}

/// R2-08 / frozen rule 17, with the time of a version taken as the moment
/// it was captured, then the one it was stored at (both travel with it).
/// A re-import captured in the same second is stored after the current
/// version, so it is current, here and replayed with the identity it
/// carries (a guard: as before). Among versions of one moment, the one
/// stored last takes over from the Lixeira, on A and on B, whatever their
/// numbers and identities.
///
/// Red on `258b37a`: the promotion breaks the tie of the moment of capture
/// by number, so A and B promote different versions.
#[test]
fn f3_1a_versions_of_one_moment_follow_their_stored_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("c.sqlite3")).unwrap();
    let version = |text: &str| {
        let mut c = capture("SEFAZ-PRO-2026/04001", text);
        c.captured_at = "2030-01-01T10:00:00Z".into();
        c.request_id = uuid::Uuid::new_v4().to_string();
        c
    };
    let first = store.import_capture(&version("V1")).unwrap();
    let second = store.import_capture(&version("V2")).unwrap();
    assert_eq!(latest(&store, first.document_id), second.capture_id);
    let carried = vec![SlotIdent {
        kind: "capture".into(),
        slot: String::new(),
        ident: same('0'),
    }];
    match store
        .execute_carrying(
            "Casa",
            IntranetMutation::ImportCapture {
                capture: version("V3"),
            },
            &carried,
        )
        .unwrap()
    {
        crate::idents::Carried::Applied(..) => {}
        crate::idents::Carried::Refused(e) => panic!("{e}"),
    }
    let third = row_of(&store, "capture", &same('0'));
    assert_eq!(latest(&store, first.document_id), third, "replayed too");

    let (_dir, _path, mut a, mut b) = two();
    let doc = a.list_documents("").unwrap()[0].id;
    let stored = |mut copy: CabinetSnapshot, later: i64| {
        for row in &mut copy.captures {
            if row.id == later {
                row.created_at = "2030-01-01T10:00:05Z".into();
            }
        }
        copy
    };
    // V2 (the lower identity, stored later) is the higher number on A,
    // the lower on B.
    let on_a = stored(versions_copy(&a, 200, 201), 201);
    let on_b = stored(versions_copy(&a, 201, 200), 200);
    adopt(&mut a, &on_a);
    adopt(&mut b, &on_b);
    for store in [&mut a, &mut b] {
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Capture,
                id: 202,
            })
            .unwrap();
        let v2 = row_of(store, "capture", &same('3'));
        assert_eq!(latest(store, doc), v2, "V2, stored last, takes over");
    }
}

// -------------------------------- plano-frente-f §9 obligations --

/// Obligation (a), with documents mapped (kind set of the test): a
/// creation of this library whose replay became a card explains its own
/// absence from the result in this library: the card holds this library's
/// number, in this library's frame, and is no legacy number.
///
/// Red on `258b37a`: the card's creation is recorded as a legacy number,
/// which never explains an item of this library.
#[test]
fn f3_1a_carded_creation_explains_itself_here() {
    let _kinds = kinds(&["document"]);
    let (_dir, _path, mut a, _b) = two();
    let x = a
        .import_capture(&capture("SEFAZ-PRO-2026/04011", "Criado aqui."))
        .unwrap()
        .document_id;
    let mut result = copy_of(&a);
    result.documents.retain(|d| d.id != x);
    result.requests.retain(|r| r.document_id != x);
    let index = ResultIndex::of(&result);
    let mut cards = Removed::default();
    cards.kept_in_card(
        &IntranetMutation::ImportCapture {
            capture: capture("SEFAZ-PRO-2026/04011", "Criado aqui."),
        },
        &[CreatedId::new(IdKind::Document, "", x)],
    );
    let check = Check {
        result: &index,
        explained: &[&cards],
        same_numbering: true,
        replayed: None,
    };
    let lost = dropped(&LibraryHolder::new(&a.conn).unwrap(), &check).unwrap();
    assert!(lost.is_empty(), "{lost:?}");
}

/// Obligations (b) and (c), with documents mapped (kind set of the test):
/// a row of a mapped kind without identity is proved `Unprovable` (v2
/// §2.1), and a number without identity (`Frame`) of a mapped kind never
/// holds, not even on this library's own queue replayed in its frame: its
/// number here is not the copy's.
///
/// Red on `258b37a`: the row is proved `Frame`, and that `Frame` number
/// is kept as it is.
#[test]
fn f3_1a_mapped_kind_without_identity_never_goes_by_number() {
    let _kinds = kinds(&["document"]);
    let (_dir, _path, a, _b) = two();
    let doc = a.list_documents("").unwrap()[0].id;
    let note = IntranetMutation::SaveNote {
        document_id: doc,
        note: "Nota.".into(),
        expected_version: None,
    };
    a.conn
        .execute(
            "DELETE FROM item_ident WHERE kind='document' AND row_id=?1",
            [doc],
        )
        .unwrap();
    let refs = Refs::capture(&a.conn, &note, None).unwrap().unwrap();
    assert_eq!(refs.mutation[0].proof, Proof::Unprovable);
    let framed = Refs {
        mutation: vec![Ref {
            kind: IdKind::Document,
            row: doc,
            proof: Proof::Frame,
            link: None,
        }],
        ..refs
    };
    assert!(framed.resolve(&note, None, &a.conn, true).is_err());
}

/// Obligation (c), with links mapped (kind set of the test): a mapped
/// library never takes identities by number from a copy of the same
/// revision (its numbers are its own); it keeps the ones it holds.
///
/// Red on `258b37a`: the content compares equal number by number and the
/// copy's triples are taken, crossed.
#[test]
fn f3_1a_mapped_library_never_takes_identities_by_number() {
    let _kinds = kinds(&["link"]);
    let (_dir, _path, mut a, _b) = two();
    let d1 = a.list_documents("").unwrap()[0].id;
    let d2 = a
        .import_capture(&capture("SEFAZ-PRO-2026/04021", "Outro."))
        .unwrap()
        .document_id;
    a.add_document_link(d1, d2, "relacionado", "Um").unwrap();
    a.add_document_link(d2, d1, "relacionado", "Dois").unwrap();
    assert!(crate::idents::is_mapped(&a.conn).unwrap());
    let ids = crate::f3b2_tests::link_ids(&a);
    let before: Vec<String> = ids
        .iter()
        .map(|id| crate::f3a2_tests::ident(&a, "link", *id))
        .collect();
    let mut copy = copy_of(&a);
    for IdentRow(kind, row, _) in &mut copy.idents {
        if kind == "link" {
            *row = if *row == ids[0] { ids[1] } else { ids[0] };
        }
    }
    copy.idents.sort();
    a.take_identities_from(&copy, false).unwrap();
    let after: Vec<String> = ids
        .iter()
        .map(|id| crate::f3a2_tests::ident(&a, "link", *id))
        .collect();
    assert_eq!(after, before);
}

/// Obligation (e): a recovery library is detached from every exchange;
/// its identities stay, but its origin is cleared, so the state of the
/// mapping goes with it (a provisional library is never mapped).
///
/// Red on `258b37a`: `ident_meta` is not cleared, so the recovery library
/// stays mapped while its changes go without proofs.
#[test]
fn f3_1a_recovery_library_starts_unmapped() {
    let (_dir, _path, a, _b) = two();
    assert!(crate::idents::is_mapped(&a.conn).unwrap());
    crate::recovery::detach(&a.conn).unwrap();
    assert_eq!(crate::idents::origin(&a.conn).unwrap().0, "provisional");
    assert!(!crate::idents::is_mapped(&a.conn).unwrap());
}
