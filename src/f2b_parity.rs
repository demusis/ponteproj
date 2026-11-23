//! F2b (Frente F): the parity machinery of F2a (`f2a_parity`), extended to
//! the shared file of format 9.
//!
//! - the file published by the build without F2 (0.51 final, fc6342e,
//!   `tests/fixtures/paridade-0.51.ponte`) is promoted here: the file of
//!   format 9 carries the same content plus the identities, and a reader of
//!   0.51 (formats up to 8, `read_header_up_to(…, 8)`, the very check of
//!   that build) refuses both the stub and the file of format 9 before it
//!   writes anything;
//! - a library migrated through F2b still exchanges every variant of
//!   change with another F2b device, in format 9.
#![allow(clippy::all)]

use crate::f2a_parity::{comparable, every_variant, KEY};
use crate::intranet::{IntranetConfig, IntranetMode};
use crate::sharedfolder::{self, Found};
use crate::storage::Store;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn on() {
    crate::idents::hooks::PROMOTION.with(|c| c.set(true));
}

fn folder_config(member: &str, files: Vec<String>) -> IntranetConfig {
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

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The content of a file of format 9 as `f2a_parity::content_of` gives the
/// one of format 8 (the identities and the epoch taken out), with them.
fn nine_content(path: &Path) -> (Value, usize, String) {
    let Some(Found::Copy(mut copy)) = sharedfolder::read_any(path, KEY).unwrap() else {
        panic!("{} is not a copy", path.display())
    };
    assert_eq!(
        (
            copy.header.format,
            copy.header.schema_version,
            copy.header.level
        ),
        (9, 22, 1)
    );
    let triples = copy.payload.snapshot.idents.len();
    let epoch = std::mem::take(&mut copy.payload.snapshot.ident_epoch);
    copy.payload.snapshot.idents.clear();
    for e in copy.payload.journal.iter_mut() {
        e.idents.clear();
    }
    // Written again as an 8 of schema 21 (what 0.51 wrote) to compare.
    let dir = tempfile::tempdir().unwrap();
    let eight = dir.path().join("como-8.ponte");
    sharedfolder::write(
        &eight,
        KEY,
        &sharedfolder::WriteRequest {
            lineage: &copy.header.lineage,
            revision: copy.header.revision,
            writer: &copy.header.writer,
            writer_name: &copy.header.writer_name,
            schema_version: 21,
        },
        &copy.payload,
        &copy.blobs,
    )
    .unwrap();
    (crate::f2a_parity::content_of(&eight), triples, epoch)
}

/// The 0.51 file, promoted by this build: the 9 carries the very content of
/// the 0.51 file and one identity per item; a reader of 0.51 refuses the
/// stub and the 9 before writing anything (both files byte for byte as
/// they were).
#[test]
fn f2b_a_0_51_file_promoted_here_is_refused_by_a_0_51_reader() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::copy(fixture("paridade-0.51.ponte"), &file).unwrap();
    let golden = crate::f2a_parity::content_of(&file);
    let mut store = Store::open(&dir.path().join("novo.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config(
            "Acervo 22",
            vec![file.display().to_string()],
        ))
        .unwrap();
    assert!(!sync(&mut store).promoted, "joining first");
    let report = sync(&mut store);
    assert!(report.promoted, "{:?}", report.promotion_wait);
    let nine = sharedfolder::nine_path(&file);
    let (content, triples, epoch) = nine_content(&nine);
    assert_eq!(
        comparable(&content, &content),
        comparable(&golden, &golden),
        "the 9 carries the content of the 0.51 file"
    );
    assert!(!epoch.is_empty());
    assert_eq!(
        triples,
        crate::idents::test_support::all_idents(&store).len(),
        "one identity per item"
    );
    for path in [&file, &nine] {
        let before = std::fs::read(path).unwrap();
        let error = sharedfolder::read_header_up_to(path, 8)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("versão mais nova") && error.contains("Atualize"),
            "{}: {error}",
            path.display()
        );
        assert_eq!(std::fs::read(path).unwrap(), before, "{}", path.display());
    }
    assert!(matches!(
        sharedfolder::read_any(&file, KEY).unwrap(),
        Some(Found::Moved { .. })
    ));
    if let Ok(out) = std::env::var("F2B_PARIDADE") {
        let out = PathBuf::from(out);
        std::fs::copy(&file, out.join("paridade-trava.ponte")).unwrap();
        std::fs::copy(&nine, out.join("paridade-9.ponte")).unwrap();
    }
}

/// The cards of a library, as (item, reason), sorted.
fn cards(store: &Store) -> Vec<(String, String)> {
    let mut cards: Vec<(String, String)> = store
        .list_sync_conflicts()
        .unwrap()
        .into_iter()
        .map(|c| (c.item_label, c.reason))
        .collect();
    cards.sort();
    cards
}

/// The cards B gets in the scenario below when the folder stays at format
/// 8 (the promotion off): what the exchange in format 9 must give too.
fn cards_in_format_8() -> Vec<(String, String)> {
    let promotion = crate::idents::hooks::PROMOTION.with(|c| c.replace(false));
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    let mut a = crate::f2a_parity::scripted_file(dir.path(), &file);
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Outro", vec![file.display().to_string()]))
        .unwrap();
    sync(&mut b);
    every_variant(&mut b, "b");
    sync(&mut b);
    crate::idents::hooks::PROMOTION.with(|c| c.set(promotion));
    assert_eq!(sharedfolder::read_header(&file).unwrap().unwrap().format, 8);
    cards(&b)
}

/// Two F2b devices exchange every variant of change in format 9: the
/// first publishes and promotes, the second takes the 9 and makes every
/// variant again, the first takes them back. Both end with the same
/// content and the same identities.
#[test]
fn f2b_a_library_migrated_through_f2b_exchanges_every_variant_with_another() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    let a = crate::f2a_parity::scripted_file(dir.path(), &file);
    let mut a = a;
    assert!(sync(&mut a).promoted);
    let nine = sharedfolder::nine_path(&file);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Outro", vec![file.display().to_string()]))
        .unwrap();
    sync(&mut b);
    every_variant(&mut b, "b");
    // Every variant again on B (the holidays may already be the ones the
    // script saves, and an unchanged setting is not queued).
    let queued = b.list_offline_outbox().unwrap();
    let variants: std::collections::BTreeSet<&str> = queued
        .iter()
        .map(|e| crate::f2a_parity::variant_name(&e.mutation))
        .collect();
    assert!(
        variants.len() + 1 >= crate::f2a_parity::enum_variant_count(),
        "{variants:?}"
    );
    sync(&mut b);
    assert_eq!(
        cards(&b),
        cards_in_format_8(),
        "the same cards as the exchange in format 8"
    );
    assert!(b.list_offline_outbox().unwrap().is_empty());
    sync(&mut a);
    sync(&mut b);
    let (content, _, _) = nine_content(&nine);
    assert!(
        content.to_string().contains("T-b-doc"),
        "B's work is in the 9"
    );
    // Both scripts, every variant, are in the journal of the 9.
    let Some(Found::Copy(copy)) = sharedfolder::read_any(&nine, KEY).unwrap() else {
        panic!()
    };
    let journaled: std::collections::BTreeSet<&str> = copy
        .payload
        .journal
        .iter()
        .map(|e| crate::f2a_parity::variant_name(&e.mutation))
        .collect();
    assert_eq!(
        journaled.len(),
        crate::f2a_parity::enum_variant_count(),
        "{journaled:?}"
    );
    let ids = |s: &Store| crate::idents::test_support::all_idents(s);
    assert_eq!(ids(&a), ids(&b), "the same identities on both");
    let snap = |s: &Store| serde_json::to_value(s.export_snapshot().unwrap()).unwrap();
    assert_eq!(snap(&a), snap(&b), "the same content on both");
}
