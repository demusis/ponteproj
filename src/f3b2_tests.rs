//! F3.0b-ii of Frente F: acceptance tests of the adoption mapping (id floor
//! per table, reservation, allocation, epoch change), the version keys and
//! the retired sources, coverage and the exchange report by identity, and
//! the card that keeps the original envelope (`desenho-f3-v3-adendo.md` §4,
//! §7 and §12; `desenho-f3-v2.md` §2.3, §2.6–§2.8). Written before the code:
//! each test says why it fails on `93885fc` (F3.0b-i).
//!
//! No kind is mapped yet ([`crate::mapper::ADDRESSED`] is empty until
//! F3.1a): the mapper is tested through a kind set of the tests
//! ([`kinds`]), on this thread only.
#![allow(clippy::all)]

use crate::coverage::{dropped, Check, LibraryHolder, Removed, ResultIndex, SnapshotHolder};
use crate::f3a2_tests::{capture, count, fsync, ident, two, Team};
use crate::idents::IdentRow;
use crate::idmap::IdKind;
use crate::idrefs::{Proof, Ref, Refs};
use crate::intranet::{CabinetSnapshot, IntranetMutation};
use crate::storage::Store;

/// The kinds the mapper maps on this thread while the value lives.
pub(crate) struct Kinds;

pub(crate) fn kinds(set: &[&'static str]) -> Kinds {
    crate::mapper::hooks::ADDRESSED.with(|k| *k.borrow_mut() = Some(set.to_vec()));
    Kinds
}

impl Drop for Kinds {
    fn drop(&mut self) {
        crate::mapper::hooks::ADDRESSED.with(|k| *k.borrow_mut() = None);
    }
}

fn epoch(store: &Store) -> String {
    crate::idents::origin(&store.conn).unwrap().2.unwrap()
}

/// The copy of `store` as its folder would carry it (same epoch).
pub(crate) fn copy_of(store: &Store) -> CabinetSnapshot {
    store.export_with_idents(&epoch(store)).unwrap()
}

/// `store` adopts `copy`, as the library the person works in.
pub(crate) fn adopt(store: &mut Store, copy: &CabinetSnapshot) {
    store
        .apply_snapshot_and(
            copy,
            |_, _| unreachable!("no PDF to fetch"),
            |_| unreachable!("no attachment to fetch"),
            |_| Ok(()),
        )
        .unwrap();
}

pub(crate) fn fresh_ident() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn doc(store: &mut Store, number: &str) -> i64 {
    store
        .import_capture(&capture(number, "Texto do documento."))
        .unwrap()
        .document_id
}

pub(crate) fn link_ids(store: &Store) -> Vec<i64> {
    let mut statement = store
        .conn
        .prepare("SELECT id FROM document_links ORDER BY id")
        .unwrap();
    statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

pub(crate) fn row_of(store: &Store, kind: &str, ident: &str) -> Option<i64> {
    use rusqlite::OptionalExtension;
    store
        .conn
        .query_row(
            "SELECT row_id FROM item_ident WHERE kind=?1 AND ident=?2",
            [kind, ident],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
}

fn version_of(store: &Store, kind: &str, id: i64) -> Option<i64> {
    use rusqlite::OptionalExtension;
    store
        .conn
        .query_row(
            "SELECT version FROM item_versions WHERE kind=?1 AND item_id=?2",
            rusqlite::params![kind, id],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
}

// ------------------------------------------- adendo §4 (R2-04) --

/// R2-04 / frozen rules 13 and 14, with links mapped (kind set of the
/// test). X is the top link here, held by a screen at x, and purged with no
/// adoption in between. The next copy carries Y at x: Y is given a number
/// above the floor, never x, and the held x names nothing. The link both
/// sides know keeps its number (matched by identity). An epoch change
/// keeps nothing and reserves nothing: every link is given a number above
/// the floor, in the copy's order.
///
/// Red on `93885fc`: the adoption is verbatim, and Y takes x.
#[test]
fn f3_0b_purged_top_between_adoptions_never_reused() {
    let _kinds = kinds(&["link"]);
    let (_dir, _path, mut a, _b) = two();
    let d1 = a.list_documents("").unwrap()[0].id;
    let d2 = doc(&mut a, "SEFAZ-PRO-2026/01001");
    let d3 = doc(&mut a, "SEFAZ-PRO-2026/01002");
    a.add_document_link(d1, d2, "relacionado", "Vínculo que fica")
        .unwrap();
    a.add_document_link(d1, d3, "relacionado", "Vínculo X")
        .unwrap();
    let ids = link_ids(&a);
    let (kept, held) = (ids[0], *ids.last().unwrap());
    let kept_ident = ident(&a, "link", kept);
    a.conn
        .execute("DELETE FROM document_links WHERE id=?1", [held])
        .unwrap();
    let mut copy = copy_of(&a);
    let mut y = copy.document_links[0].clone();
    y.id = held;
    (y.source_id, y.target_id) = (d2, d1);
    copy.document_links.push(y);
    let y_ident = fresh_ident();
    copy.idents
        .push(IdentRow("link".into(), held, y_ident.clone()));
    copy.idents.sort();
    let meta = copy
        .item_versions
        .iter()
        .find(|(k, _, _)| k == "document_link")
        .map(|(_, _, m)| m.clone())
        .unwrap();
    copy.item_versions
        .push(("document_link".into(), held, meta.clone()));
    adopt(&mut a, &copy);
    let y_here = row_of(&a, "link", &y_ident).expect("Y is here");
    assert!(y_here > held, "Y got {y_here}, the held {held} is retired");
    assert!(
        !link_ids(&a).contains(&held),
        "the held number names nothing"
    );
    assert_eq!(row_of(&a, "link", &kept_ident), Some(kept), "matched: kept");

    // An epoch change: nothing is kept, nothing reserved.
    let floor = *link_ids(&a).last().unwrap();
    let mut other = a.export_with_idents(&fresh_ident()).unwrap();
    for IdentRow(_, _, ident) in &mut other.idents {
        *ident = fresh_ident();
    }
    other.idents.sort();
    adopt(&mut a, &other);
    let after = link_ids(&a);
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(
        after.iter().all(|id| *id > floor),
        "{after:?} above {floor}"
    );
    assert!(!after.contains(&held));
}

// ------------------------------------------------ v2 §2.8 (H5) --

/// v2 §2.8, with links mapped (kind set of the test). The version of a
/// link follows its row to the number it is given here; one whose row is
/// not in the copy is dropped. The cabinet counter and the row keys of a
/// kind that is not mapped (the document's note) are kept as they come.
///
/// Red on `93885fc`: the version rows are copied verbatim (the copy's
/// number, and the one of no row).
#[test]
fn f3_0b_version_keys_follow_their_class() {
    let _kinds = kinds(&["link"]);
    let (_dir, _path, mut a, _b) = two();
    let d1 = a.list_documents("").unwrap()[0].id;
    let d2 = doc(&mut a, "SEFAZ-PRO-2026/01011");
    a.save_note(d1, "Nota que fica.").unwrap();
    a.add_document_link(d1, d2, "relacionado", "Vínculo")
        .unwrap();
    let link = link_ids(&a)[0];
    let link_ident = ident(&a, "link", link);
    // Numbers above the copy's in this library (a top it gave and purged).
    a.conn
        .execute(
            "UPDATE sqlite_sequence SET seq=seq+50 WHERE name='document_links'",
            [],
        )
        .unwrap();
    let mut copy = copy_of(&a);
    let (cabinet, note) = (
        version_of(&a, "cabinet", 1),
        version_of(&a, "note", d1).expect("note version"),
    );
    // The copy's link under another number, and a version of no row.
    let copy_link = link + 7;
    copy.document_links[0].id = copy_link;
    for IdentRow(kind, row, _) in &mut copy.idents {
        if kind == "link" {
            *row = copy_link;
        }
    }
    let mut link_version = 0;
    for (kind, id, meta) in &mut copy.item_versions {
        if kind == "document_link" {
            *id = copy_link;
            meta.version += 3;
            link_version = meta.version;
        }
    }
    let meta = copy.item_versions[0].2.clone();
    copy.item_versions.push(("document_link".into(), 999, meta));
    adopt(&mut a, &copy);
    let here = row_of(&a, "link", &link_ident).expect("the link");
    assert_eq!(here, link, "matched by identity: its number here");
    assert_eq!(version_of(&a, "document_link", here), Some(link_version));
    assert_eq!(version_of(&a, "document_link", copy_link), None);
    assert_eq!(
        version_of(&a, "document_link", 999),
        None,
        "no row: dropped"
    );
    assert_eq!(version_of(&a, "cabinet", 1), cabinet, "counter: verbatim");
    assert_eq!(version_of(&a, "note", d1), Some(note), "unmapped kind");
}

// ------------------------------------------- adendo §7 (R2-07) --

/// R2-07 / frozen rule 16, with files and attachments mapped (kind set of
/// the test). An analysis cites two PDFs on the same page and two
/// attachments with the same bytes, none of which is in the copy. After
/// two adoptions: four distinct `R` keys (12 hex of the sha256 of the
/// copy's name), their numbers 0, each citation (and fact) still on its own
/// source and quote, and a new attachment with the same bytes is not taken
/// for any of them.
///
/// Red on `93885fc` (and [v2]): the names stay `C…F…P1` and `A…H…`, which
/// still parse and would resolve (and v2's `C0F0P1`/`A0H…` collide).
#[test]
fn f3_0b_orphan_sources_stay_distinct() {
    use sha2::{Digest, Sha256};
    let _kinds = kinds(&["file", "attachment"]);
    let (_dir, _path, mut a, _b) = two();
    let mut copy = copy_of(&a);
    assert!(copy.files_meta.is_empty() && copy.attachments.is_empty());
    let (document, capture) = (copy.captures[0].document_id, copy.captures[0].id);
    let bytes = b"%PDF-1.4 anexo repetido";
    let hash = format!("{:x}", Sha256::digest(bytes));
    let source = |id: String, file: Option<i64>, text: &str| crate::evidence::Source {
        id,
        document_id: document,
        capture_id: capture,
        label: "fonte".into(),
        file_id: file,
        page: file.map(|_| 1),
        method: "captura".into(),
        text: text.into(),
    };
    let names = [
        format!("C{capture}F900P1"),
        format!("C{capture}F901P1"),
        crate::evidence::attachment_source_id(800, &hash),
        crate::evidence::attachment_source_id(801, &hash),
    ];
    let texts = [
        "primeira página do primeiro PDF",
        "primeira página do segundo PDF",
        "texto do primeiro anexo",
        "texto do segundo anexo",
    ];
    let files = [Some(900), Some(901), None, None];
    let analysis = crate::model::Analysis {
        capture_id: capture,
        materials: (0..4)
            .map(|i| source(names[i].clone(), files[i], texts[i]))
            .collect(),
        citations: (0..4)
            .map(|i| crate::evidence::Citation {
                source_id: names[i].clone(),
                quote: texts[i].into(),
                claim: format!("afirmação {i}"),
            })
            .collect(),
        facts: (0..4)
            .map(|i| crate::model::Fact {
                id: format!("f{i}"),
                source_id: names[i].clone(),
                quote: texts[i].into(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let id = copy.analyses.iter().map(|r| r.id).max().unwrap_or(0) + 1;
    copy.analyses.push(crate::intranet::AnalysisRow {
        id,
        document_id: document,
        capture_id: capture,
        analysis,
        created_at: "2026-11-23T10:00:00Z".into(),
    });
    copy.idents
        .push(IdentRow("analysis".into(), id, fresh_ident()));
    copy.idents.sort();
    adopt(&mut a, &copy);
    let again = copy_of(&a);
    adopt(&mut a, &again);

    let row = a
        .export_snapshot()
        .unwrap()
        .analyses
        .into_iter()
        .find(|r| r.id == id)
        .expect("the analysis");
    let materials = &row.analysis.materials;
    let keys: Vec<&str> = materials.iter().map(|s| s.id.as_str()).collect();
    for (i, key) in keys.iter().enumerate() {
        let expected = format!(
            "R{}",
            &format!("{:x}", Sha256::digest(names[i].as_bytes()))[..12]
        );
        assert_eq!(*key, expected, "source {i}");
    }
    let distinct: std::collections::BTreeSet<_> = keys.iter().collect();
    assert_eq!(distinct.len(), 4, "{keys:?}");
    for s in materials {
        assert_eq!((s.document_id, s.capture_id), (0, 0), "{s:?}");
        assert_eq!(s.file_id.unwrap_or(0), 0, "{s:?}");
        assert!(!crate::evidence::is_attachment_source(s), "{s:?}");
    }
    assert_eq!(row.analysis.citations.len(), 4);
    for c in row.analysis.citations.iter() {
        let s = materials.iter().find(|s| s.id == c.source_id).unwrap();
        assert_eq!(s.text, c.quote, "each citation on its own source");
    }
    for (i, f) in row.analysis.facts.iter().enumerate() {
        assert_eq!(f.source_id, keys[i], "fact {i}");
    }
    let new = a
        .add_attachment(
            crate::attachments::Owner::Document(document),
            "mesmo.pdf",
            "mesmos bytes",
            bytes,
        )
        .unwrap();
    let named = crate::evidence::attachment_source_id(new, &hash);
    assert!(materials.iter().all(|s| s.id != named));
}

// -------------------------------------------------- v2 §2.6 (H4) --

/// v2 §2.6, with documents mapped (kind set of the test). X is a document
/// the result does not have; the journal records the purge of Y under the
/// same number. A removal proved by Y's identity never explains X, in a
/// copy or here; the one proved by X's does. A removal without proofs (an
/// older Ponte's) explains by number only an item of a copy, never one of
/// this library.
///
/// Red on `93885fc`: `Removed::explains` matches by number, and X goes
/// without a copy kept aside.
#[test]
fn f3_0_guard_never_explains_x_by_y_purge() {
    let _kinds = kinds(&["document"]);
    let (_dir, _path, a, _b) = two();
    let holder = copy_of(&a);
    let x = holder.documents[0].id;
    let x_ident = ident(&a, "document", x);
    let mut result = holder.clone();
    result.documents.retain(|d| d.id != x);
    result.requests.retain(|r| r.document_id != x);
    let index = ResultIndex::of(&result);
    let purge = IntranetMutation::PurgeTrash {
        kind: "document".into(),
        id: x,
    };
    let proved = |ident: &str| Refs {
        mutation: vec![Ref {
            kind: IdKind::Document,
            row: x,
            proof: Proof::Ident(ident.into()),
            link: None,
        }],
        ..Refs::default()
    };
    let lost = |removed: &Removed, here: bool| -> usize {
        let check = Check {
            result: &index,
            explained: &[removed],
            same_numbering: true,
            replayed: None,
        };
        if here {
            dropped(&LibraryHolder::new(&a.conn).unwrap(), &check)
        } else {
            dropped(&SnapshotHolder(&holder), &check)
        }
        .unwrap()
        .len()
    };
    let mut of_y = Removed::default();
    of_y.note(&purge, Some(&proved(&fresh_ident())));
    assert_eq!(lost(&of_y, false), 1, "Y's purge never explains X (copy)");
    assert_eq!(lost(&of_y, true), 1, "nor here");
    let mut of_x = Removed::default();
    of_x.note(&purge, Some(&proved(&x_ident)));
    assert_eq!(lost(&of_x, false), 0, "X's own purge explains it");
    assert_eq!(lost(&of_x, true), 0);
    let mut legacy = Removed::default();
    legacy.note(&purge, None);
    assert_eq!(lost(&legacy, false), 0, "by number, in a copy's numbering");
    assert_eq!(lost(&legacy, true), 1, "never here");
}

/// v2 §2.6 (the report), with links mapped (kind set of the test). B gives
/// A's link a number of its own (above B's floor). In the next exchange,
/// only A's note changed: the report counts one change and nothing new;
/// the link, unchanged, is not counted as new because its numbers differ.
///
/// Red on `93885fc`: B's versions before the adoption are keyed by its own
/// numbers and the copy's by the copy's (and the adoption is verbatim, so
/// the numbers do not even differ: the first assertion fails).
#[test]
fn f3_0_report_counts_by_ident() {
    let _kinds = kinds(&["link"]);
    let (_dir, _path, mut a, mut b) = two();
    let d1 = a.list_documents("").unwrap()[0].id;
    let d2 = doc(&mut a, "SEFAZ-PRO-2026/01021");
    a.save_note(d1, "Primeira nota.").unwrap();
    a.add_document_link(d1, d2, "relacionado", "Vínculo de A")
        .unwrap();
    fsync(&mut a);
    b.conn
        .execute_batch(
            "UPDATE sqlite_sequence SET seq=seq+50 WHERE name='document_links';
             INSERT INTO sqlite_sequence(name,seq) SELECT 'document_links',50
              WHERE NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='document_links');",
        )
        .unwrap();
    fsync(&mut b);
    let (on_a, on_b) = (link_ids(&a)[0], link_ids(&b)[0]);
    assert_ne!(on_a, on_b, "B numbers the link above its own floor");
    assert_eq!(ident(&a, "link", on_a), ident(&b, "link", on_b));
    a.save_note(d1, "Segunda nota.").unwrap();
    fsync(&mut a);
    let report = fsync(&mut b);
    assert_eq!(report.imported_new, 0, "{report:?}");
    assert!(report.imported_changed >= 1, "{report:?}");
    assert_eq!(link_ids(&b), vec![on_b], "the link keeps its number here");
}

// -------------------------------------------------- v2 §2.7 (H3) --

/// v2 §2.7 / frozen rule 8. A card keeps the change as it was made, with
/// its proofs, and shows it through the local view: once the item is gone
/// here, the card offers only Copiar/Dispensar, and "Manter a minha" is
/// refused instead of being applied by number.
///
/// Red on `93885fc`: the card keeps no proofs, and its numbers are taken
/// as they are.
#[test]
fn f3_0b_card_keeps_the_original_envelope() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    t.offline();
    t.client.save_note(doc, "Nota do cliente").unwrap();
    let queued = t.client.list_offline_outbox().unwrap()[0].clone();
    assert!(queued.refs.is_some());
    t.host.save_note(doc, "Nota do principal").unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    let cards = t.client.list_sync_conflicts().unwrap();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert_eq!(cards[0].refs, queued.refs, "the proofs as captured");
    let json = |m: &IntranetMutation| serde_json::to_string(m).unwrap();
    assert_eq!(json(&cards[0].mutation), json(&queued.mutation), "as made");
    assert!(!cards[0].unresolved, "its item is here");

    t.host.delete_document(doc).unwrap();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(count(&t.client, "SELECT count(*) FROM documents"), 0);
    let card = t.client.list_sync_conflicts().unwrap().remove(0);
    assert!(card.unresolved, "the local view does not resolve");
    let refused = t
        .client
        .resolve_sync_conflict(card.id, crate::intranet::ConflictResolution::KeepLocal)
        .unwrap_err()
        .to_string();
    assert_eq!(refused, crate::idmap::UNRESOLVED);
    assert_eq!(t.cards(), 1, "the card stays");
}
