//! F2b of Frente F: acceptance tests of the shared file of format 9 and of
//! the team protocol V2 (`desenho-f2-v2.md` §4, B1–B30). Written before the
//! code; the harness copies the patterns of `f1_inventory` (two computers
//! sharing a folder, a principal and a client on a private port).
//!
//! The tests of the old exchange (format 8) run with the promotion off
//! (`idents::hooks::PROMOTION`); every test here turns it on first.
#![allow(clippy::all)]

use crate::idents::hooks;
use crate::intranet::{
    FolderSyncReport, IntranetConfig, IntranetMode, IntranetMutation, RpcRequest, RpcResponse,
};
use crate::sharedfolder::{self, Found, JournalEntry};
use crate::storage::Store;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const KEY: &str = "senha-de-teste-123";

// ---------------------------------------------------------------- helpers --

fn on() {
    hooks::PROMOTION.with(|c| c.set(true));
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

fn try_sync(store: &mut Store) -> anyhow::Result<FolderSyncReport> {
    let cfg = store.load_intranet_config().unwrap();
    store.sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
}

fn sync(store: &mut Store) -> FolderSyncReport {
    try_sync(store).unwrap()
}

fn capture(number: &str, text: &str) -> crate::model::Capture {
    let mut c = crate::model::sample_capture();
    c.document_number = number.into();
    c.title = format!("Documento {number}");
    c.text = text.into();
    c.source_url = format!(
        "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla={}",
        number.replace('/', "%2F")
    );
    c
}

fn number(tag: &str, n: u32) -> String {
    let base = match tag {
        "a" => 100,
        "b" => 200,
        "c" => 300,
        "d" => 400,
        _ => 900,
    };
    format!("SEFAZ-PRO-2026/{:05}", base + n)
}

fn doc_by_number(store: &Store, number: &str) -> Option<i64> {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .map(|d| d.id)
}

/// Every identity of the library, by (kind, row).
fn idents(store: &Store) -> BTreeMap<(String, i64), String> {
    store
        .conn
        .prepare("SELECT kind,row_id,ident FROM item_ident")
        .unwrap()
        .query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn ident_of(store: &Store, kind: &str, row: i64) -> Option<String> {
    idents(store).get(&(kind.to_owned(), row)).cloned()
}

fn origin(store: &Store) -> (String, Option<String>, Option<String>) {
    crate::idents::origin(&store.conn).unwrap()
}

/// The copy (not a stub) at `path`.
fn copy_at(path: &Path) -> sharedfolder::Copy {
    match sharedfolder::read_any(path, KEY).unwrap() {
        Some(Found::Copy(copy)) => copy,
        Some(Found::Moved { moved_to, .. }) => panic!("{} is a stub to {moved_to}", path.display()),
        None => panic!("{} does not exist", path.display()),
    }
}

fn is_stub(path: &Path) -> bool {
    matches!(
        sharedfolder::read_any(path, KEY),
        Ok(Some(Found::Moved { .. }))
    )
}

/// The triples a file of format 9 carries, by (kind, row).
fn triples(path: &Path) -> BTreeMap<(String, i64), String> {
    copy_at(path)
        .payload
        .snapshot
        .idents
        .iter()
        .map(|t| ((t.0.clone(), t.1), t.2.clone()))
        .collect()
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn aside_files(dir: &Path) -> Vec<PathBuf> {
    files_in(dir)
        .into_iter()
        .filter(|n| n.contains("-divergente-"))
        .map(|n| dir.join(n))
        .collect()
}

/// Two computers sharing one folder (`drive`, the configured name), in sync
/// on a library with one SIGADOC document, both still at format 8.
struct Two {
    dir: tempfile::TempDir,
    drive: PathBuf,
    a: Store,
    b: Store,
}

fn two_named(name: &str) -> Two {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join(name);
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", files.clone()))
        .unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    let first = sync(&mut a);
    assert!(!first.promoted, "the first write is an 8");
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    let join = sync(&mut b);
    assert!(!join.promoted, "a computer joining never promotes");
    Two { dir, drive, a, b }
}

fn two() -> Two {
    two_named("ponte-sync.ponte")
}

impl Two {
    fn nine(&self) -> PathBuf {
        sharedfolder::nine_path(&self.drive)
    }
    fn lineage(&self) -> String {
        self.a.load_intranet_config().unwrap().folder_lineage
    }
}

/// [`two`], then A promotes the folder and B takes the file of format 9.
fn promoted() -> Two {
    let mut t = two();
    let report = sync(&mut t.a);
    assert!(report.promoted, "A promotes: {:?}", report.promotion_wait);
    sync(&mut t.b);
    t
}

fn entry(
    uid: &str,
    label: &str,
    mutation: IntranetMutation,
    created: Vec<crate::idmap::CreatedId>,
) -> JournalEntry {
    JournalEntry {
        uid: uid.into(),
        created_at: "2026-11-20T12:00:00Z".into(),
        actor: "Pessoa do aparelho antigo".into(),
        writer: "aparelho-antigo".into(),
        label: label.into(),
        mutation,
        base: None,
        revision: 1000,
        created,
        idents: Vec::new(),
    }
}

/// An 8 of the folder's lineage, as an older Ponte writes it, at `path`:
/// the content of the file of format 9 (without identities) plus `entries`
/// in its journal and `applied`.
fn old_writer_over(path: &Path, nine: &Path, entries: Vec<JournalEntry>) -> Vec<u8> {
    let copy = copy_at(nine);
    let mut payload = copy.payload;
    payload.snapshot.idents.clear();
    payload.snapshot.ident_epoch.clear();
    for e in payload.journal.iter_mut() {
        e.idents.clear();
    }
    for e in &entries {
        payload.applied.push(e.uid.clone());
    }
    payload.journal.extend(entries);
    sharedfolder::write(
        path,
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
    assert_eq!(
        sharedfolder::read_header(path).unwrap().unwrap().format,
        8,
        "an old writer's 8"
    );
    std::fs::read(path).unwrap()
}

fn conflicts_with(store: &Store, text: &str) -> usize {
    store
        .list_sync_conflicts()
        .unwrap()
        .iter()
        .filter(|c| c.reason.contains(text) || c.item_label.contains(text))
        .count()
}

const OLD_VERSION_CARD: &str =
    "feita numa versão anterior do Ponte depois que a pasta foi atualizada";

// ------------------------------------------------------------------- B1 --

/// B1. C2c in folders, after the promotion: J creates X at row 2 on the pen
/// drive, X is purged, Y reuses row 2; the Drive moved on meanwhile. The
/// merge replays J and the rest: Y keeps the identity it was published
/// with, and X's is not Y's.
#[test]
fn f2b_b1_divergent_merge_with_a_reused_row() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
    let both = vec![drive.display().to_string(), pen.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", both.clone()))
        .unwrap();
    sync(&mut a);
    assert!(sync(&mut a).promoted, "both folders promoted");
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", both.clone()))
        .unwrap();
    sync(&mut b);
    let only = |store: &mut Store, path: &Path| {
        let mut cfg = store.load_intranet_config().unwrap();
        cfg.folder_files = vec![path.display().to_string()];
        store.save_intranet_config(&cfg).unwrap();
        sync(store)
    };
    // J: A creates X on the pen drive only, then purges it; Y takes its row.
    let x = a
        .import_capture(&capture(&number("a", 1), "Documento X"))
        .unwrap()
        .document_id;
    only(&mut a, &pen);
    let ident_x = ident_of(&a, "document", x).unwrap();
    let target = crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id: x,
    };
    a.trash_item(target).unwrap();
    a.purge_trash_item(crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id: x,
    })
    .unwrap();
    only(&mut a, &pen);
    a.import_capture(&capture(&number("a", 2), "Documento Y"))
        .unwrap();
    only(&mut a, &pen);
    let y = doc_by_number(&a, &number("a", 2)).unwrap();
    assert_eq!(y, x, "Y reused X's row");
    let ident_y = ident_of(&a, "document", y).unwrap();
    assert_ne!(ident_x, ident_y);
    // B moves the Drive on (newest copy), then A merges both.
    for n in 0..8 {
        let mut task = crate::tasks::Task::standalone();
        task.title = format!("enchimento {n}");
        b.save_task(&task).unwrap();
    }
    only(&mut b, &drive);
    let mut cfg = a.load_intranet_config().unwrap();
    cfg.folder_files = both.clone();
    a.save_intranet_config(&cfg).unwrap();
    sync(&mut a);
    let y_after = doc_by_number(&a, &number("a", 2)).unwrap();
    assert_eq!(
        ident_of(&a, "document", y_after).as_deref(),
        Some(ident_y.as_str()),
        "Y keeps the identity it was published with"
    );
    assert!(
        !idents(&a).values().any(|i| *i == ident_x),
        "X's identity is held by no row"
    );
    assert_eq!(
        triples(&sharedfolder::nine_path(&drive)).get(&("document".to_owned(), y_after)),
        Some(&ident_y)
    );
}

// --------------------------------------------------------------- B2–B5 --

/// B2. An old copy (an 8 written over the stub) adds an attachment to
/// document 2, whose creation is no longer in its journal; in the base 9,
/// row 2 is Y. The attachment never lands on Y: it is a card with the
/// content, the old copy is kept aside and its change is not "applied".
#[test]
fn f2b_b2_old_copy_attachment_on_a_reused_owner_becomes_a_card() {
    let mut t = promoted();
    let y =
        t.b.import_capture(&capture(&number("b", 1), "Documento Y"))
            .unwrap()
            .document_id;
    sync(&mut t.b);
    sync(&mut t.a);
    use base64::Engine;
    let attach = entry(
        "donor-anexo-1",
        "Anexar arquivo",
        IntranetMutation::AddAttachment {
            owner_kind: "document".into(),
            owner_id: y,
            name: "oficio.txt".into(),
            description: "Ofício anexado no aparelho antigo".into(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(b"texto do oficio"),
        },
        vec![crate::idmap::CreatedId::new(
            crate::idmap::IdKind::Attachment,
            "",
            77,
        )],
    );
    let nine = t.nine();
    let donor = old_writer_over(&t.drive, &nine, vec![attach]);
    let report = sync(&mut t.a);
    assert!(
        t.a.list_attachments(crate::attachments::Owner::Document(y))
            .unwrap()
            .is_empty(),
        "nothing attached to Y"
    );
    assert_eq!(conflicts_with(&t.a, OLD_VERSION_CARD), 1, "{report:?}");
    let kept = aside_files(t.drive.parent().unwrap());
    assert!(
        kept.iter().any(|p| std::fs::read(p).unwrap() == donor),
        "the old copy is kept aside: {kept:?}"
    );
    assert!(is_stub(&t.drive), "the stub is back");
    assert!(
        !copy_at(&nine)
            .payload
            .applied
            .contains(&"donor-anexo-1".to_owned()),
        "a carded change is not applied"
    );
}

/// B3. An old copy creates document N and then a task on N: both are
/// replayed, the task on N.
#[test]
fn f2b_b3_self_contained_old_creation_is_replayed() {
    let mut t = promoted();
    let mut task = crate::tasks::Task::new(41);
    task.title = "Tarefa do aparelho antigo".into();
    let entries = vec![
        entry(
            "donor-doc-n",
            "Importar documento",
            IntranetMutation::ImportCapture {
                capture: capture(&number("d", 1), "Documento N do aparelho antigo"),
            },
            vec![
                crate::idmap::CreatedId::new(crate::idmap::IdKind::Document, "", 41),
                crate::idmap::CreatedId::new(crate::idmap::IdKind::Capture, "", 43),
            ],
        ),
        entry(
            "donor-task-n",
            "Criar tarefa",
            IntranetMutation::SaveTask {
                task,
                expected_version: None,
                no_project: false,
            },
            vec![crate::idmap::CreatedId::new(
                crate::idmap::IdKind::Task,
                "",
                45,
            )],
        ),
    ];
    old_writer_over(&t.drive, &t.nine(), entries);
    sync(&mut t.a);
    let n = doc_by_number(&t.a, &number("d", 1)).expect("N replayed");
    let made =
        t.a.list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.title == "Tarefa do aparelho antigo")
            .expect("the task replayed");
    assert_eq!(made.document_id, Some(n), "the task is on N");
    assert_eq!(conflicts_with(&t.a, OLD_VERSION_CARD), 0);
}

/// B4. An old copy has an item that no journal entry explains: the file
/// is archived, byte for byte, before the stub is written back, and a
/// notice names the archive.
#[test]
fn f2b_b4_snapshot_only_item_in_an_old_copy_is_kept_aside() {
    let mut t = promoted();
    let mut base_task = crate::tasks::Task::standalone();
    base_task.title = "Tarefa da base".into();
    t.a.save_task(&base_task).unwrap();
    sync(&mut t.a);
    let nine = t.nine();
    let copy = copy_at(&nine);
    let mut payload = copy.payload;
    payload.snapshot.idents.clear();
    payload.snapshot.ident_epoch.clear();
    for e in payload.journal.iter_mut() {
        e.idents.clear();
    }
    let mut extra = payload.snapshot.tasks[0].clone();
    extra.id = 900;
    extra.uid = "tarefa-so-no-retrato".into();
    extra.title = "Só no retrato antigo".into();
    payload.snapshot.tasks.push(extra);
    sharedfolder::write(
        &t.drive,
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
    let donor = std::fs::read(&t.drive).unwrap();
    let report = sync(&mut t.a);
    let kept: Vec<PathBuf> = aside_files(t.drive.parent().unwrap())
        .into_iter()
        .filter(|p| std::fs::read(p).unwrap() == donor)
        .collect();
    assert_eq!(kept.len(), 1, "archived byte for byte");
    let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        report.notes.iter().any(|n| n.contains(&name)),
        "a notice names the archive: {:?}",
        report.notes
    );
    assert!(is_stub(&t.drive));
}

/// B5. An old copy imports a new version of a manual record whose number
/// the base already has: a card, and no new version on the record.
#[test]
fn f2b_b5_old_manual_number_edit_becomes_a_card() {
    let mut t = promoted();
    let receipt =
        t.a.create_manual_document(&crate::manual::ManualFields {
            kind: "ligacao".into(),
            title: "Registro manual do gabinete".into(),
            origin: "Gabinete".into(),
            document_date: String::new(),
            text: "Texto do registro manual".into(),
        })
        .unwrap();
    sync(&mut t.a);
    let current = t.a.load_document(receipt.document_id).unwrap().capture;
    let number = current.document_number.clone();
    assert!(crate::model::is_manual_number(&number), "{number}");
    let versions = |store: &Store| -> i64 {
        store
            .conn
            .query_row(
                "SELECT count(*) FROM captures WHERE document_id=?1",
                [receipt.document_id],
                |r| r.get(0),
            )
            .unwrap()
    };
    let before = versions(&t.a);
    let mut edit = current.clone();
    edit.text = "Versão editada no aparelho antigo".into();
    edit.captured_at = "2026-11-21T12:00:00Z".into();
    old_writer_over(
        &t.drive,
        &t.nine(),
        vec![entry(
            "donor-manual",
            "Editar registro manual",
            IntranetMutation::ImportCapture { capture: edit },
            vec![
                crate::idmap::CreatedId::new(
                    crate::idmap::IdKind::Document,
                    "",
                    receipt.document_id,
                ),
                crate::idmap::CreatedId::new(crate::idmap::IdKind::Capture, "", 99),
            ],
        )],
    );
    sync(&mut t.a);
    assert_eq!(conflicts_with(&t.a, OLD_VERSION_CARD), 1);
    assert_eq!(versions(&t.a), before, "no new version on the record");
}

// --------------------------------------------------------------- B6–B8 --

fn notes_of(store: &Store, text: &str) -> usize {
    let mut count = 0;
    for d in store.list_documents("").unwrap() {
        count += store
            .conn
            .prepare("SELECT payload FROM notes WHERE document_id=?1")
            .unwrap()
            .query_map([d.id], |r| r.get::<_, Vec<u8>>(0))
            .unwrap()
            .filter_map(|b| b.ok())
            .filter(|b| {
                crate::protection::unseal(b)
                    .map(|plain| String::from_utf8_lossy(&plain).contains(text))
                    .unwrap_or(false)
            })
            .count();
    }
    count
}

/// B6. A note saved (through a second connection) between the writing of
/// the files of format 9 and the local adoption stays out of the cut: the
/// adoption rolls back, the next exchange adopts the 9 and replays the
/// note once, and the epoch does not change.
#[test]
fn f2b_b6_a_note_saved_during_promotion_stays_out_of_the_cut() {
    let mut t = two();
    let db = t.dir.path().join("a.sqlite3");
    let doc = t.a.list_documents("").unwrap()[0].id;
    hooks::AFTER_NINE_WRITTEN.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            let mut other = Store::open(&db).unwrap();
            other
                .save_note(doc, "Nota feita durante a atualização")
                .unwrap();
        }))
    });
    let error = try_sync(&mut t.a).unwrap_err().to_string();
    assert!(error.contains("durante a sincronização"), "{error}");
    let nine = t.nine();
    let first = copy_at(&nine);
    let epoch = first.payload.snapshot.ident_epoch.clone();
    assert!(!epoch.is_empty());
    let report = sync(&mut t.a);
    assert!(!report.promoted, "already promoted");
    assert_eq!(notes_of(&t.a, "Nota feita durante a atualização"), 1);
    let after = copy_at(&nine);
    assert_eq!(after.payload.snapshot.ident_epoch, epoch, "same epoch");
    assert!(
        after.header.revision > first.header.revision,
        "the note came after the cut"
    );
    sync(&mut t.b);
    assert_eq!(notes_of(&t.b, "Nota feita durante a atualização"), 1);
}

/// B7. A promotion that stops right after the files of format 9 were
/// written resumes, at the next exchange, by adopting them.
#[test]
fn f2b_b7_crash_after_writing_9_resumes_by_adoption() {
    let mut t = two();
    hooks::CRASH_AFTER_NINE.with(|c| c.set(true));
    assert!(try_sync(&mut t.a).is_err(), "stopped after the 9");
    let nine = t.nine();
    let epoch = copy_at(&nine).payload.snapshot.ident_epoch;
    let lineage = t.lineage();
    drop(t.a);
    let mut a = Store::open(&t.dir.path().join("a.sqlite3")).unwrap();
    assert_eq!(a.format_floor(&lineage).unwrap(), 9, "floor 9 survived");
    sync(&mut a);
    let file = triples(&nine);
    assert_eq!(idents(&a), file, "adopted with the file's identities");
    assert_eq!(origin(&a).0, "adopted");
    assert!(is_stub(&t.drive), "the stub is written");
    assert_eq!(
        copy_at(&nine).payload.snapshot.ident_epoch,
        epoch,
        "no new epoch"
    );
}

/// B8. The first 9's header revision is the revision of its snapshot and
/// the library's after adoption.
#[test]
fn f2b_b8_first_9_revision_is_coherent() {
    let mut t = two();
    let doc = t.a.list_documents("").unwrap()[0].id;
    t.a.save_note(doc, "Uma nota antes da atualização").unwrap();
    assert!(sync(&mut t.a).promoted);
    let copy = copy_at(&t.nine());
    assert_eq!(copy.header.format, 9);
    assert_eq!(copy.header.revision, copy.payload.snapshot.revision);
    assert_eq!(copy.header.revision, t.a.cabinet_revision().unwrap());
    let stub = sharedfolder::read_any(&t.drive, KEY).unwrap();
    let Some(Found::Moved { header, moved_to }) = stub else {
        panic!("a stub at the old name")
    };
    assert_eq!(header.format, 9);
    assert_eq!(moved_to, "ponte-sync-formato9.ponte");
}

// -------------------------------------------------------------- B9–B15 --

/// B9. `registro.ponte` and `registro-formato9.ponte`, both 8: no
/// promotion, the reason names the file, and nothing is rewritten.
#[test]
fn f2b_b9_an_existing_formato9_name_that_is_8_blocks_promotion() {
    let mut t = two_named("registro.ponte");
    let nine = t.nine();
    assert!(nine.ends_with("registro-formato9.ponte"));
    std::fs::copy(&t.drive, &nine).unwrap();
    let before = (
        std::fs::read(&t.drive).unwrap(),
        std::fs::read(&nine).unwrap(),
    );
    let report = sync(&mut t.a);
    assert!(!report.promoted);
    let wait = report.promotion_wait.clone().unwrap_or_default();
    assert!(
        wait.starts_with("Sincronize antes de atualizar o arquivo da pasta:")
            && wait.contains("registro-formato9.ponte")
            && wait.contains("Já existe"),
        "{wait}"
    );
    assert_eq!(
        (
            std::fs::read(&t.drive).unwrap(),
            std::fs::read(&nine).unwrap()
        ),
        before,
        "both files untouched"
    );
}

/// B10. A configured `x-formato9.ponte` holding an 8 is an 8 (the name is
/// never stripped): its file of format 9 is `x-formato9-formato9.ponte`.
#[test]
fn f2b_b10_configured_formato9_8_file_is_an_8() {
    let mut t = two_named("x-formato9.ponte");
    assert_eq!(
        sharedfolder::read_header(&t.drive).unwrap().unwrap().format,
        8
    );
    assert!(sync(&mut t.a).promoted);
    let folder = t.drive.parent().unwrap().to_path_buf();
    assert!(folder.join("x-formato9-formato9.ponte").is_file());
    assert!(
        !folder.join("x.ponte").exists(),
        "never taken for x.ponte's"
    );
    assert!(is_stub(&t.drive));
}

/// B11. Two entries of the list that are one file (same name in another
/// case, or one is the other's file of format 9) are refused when saved
/// and when exchanging.
#[test]
fn f2b_b11_list_entries_aliasing_one_file_are_refused() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Drive");
    std::fs::create_dir_all(&folder).unwrap();
    let mut store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    for pair in [
        ["ponte-sync.ponte", "PONTE-SYNC.ponte"],
        ["ponte-sync.ponte", "ponte-sync-formato9.ponte"],
    ] {
        let files: Vec<String> = pair
            .iter()
            .map(|n| folder.join(n).display().to_string())
            .collect();
        let cfg = folder_config("Casa", files);
        let saved = store.save_intranet_config(&cfg).unwrap_err().to_string();
        assert!(saved.contains("mesmo arquivo"), "{pair:?}: {saved}");
        let exchanged = store
            .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
            .unwrap_err()
            .to_string();
        assert!(exchanged.contains("mesmo arquivo"), "{pair:?}: {exchanged}");
        assert!(files_in(&folder).is_empty(), "nothing written");
    }
}

/// B12. Between the read and the rename, the destination is replaced by a
/// copy of the same revision with another nonce: the exchange aborts and
/// the destination is left as the other writer left it.
#[test]
fn f2b_b12_writer_aborts_on_changed_header_bytes() {
    let mut t = promoted();
    let nine = t.nine();
    let doc = t.a.list_documents("").unwrap()[0].id;
    t.a.save_note(doc, "Nota de A").unwrap();
    let replaced: std::rc::Rc<std::cell::RefCell<Vec<u8>>> = Default::default();
    let keep = replaced.clone();
    let target = nine.clone();
    hooks::BEFORE_RENAME.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |path: &Path| {
            assert_eq!(path, target.as_path(), "the 9 is written first");
            let copy = copy_at(path);
            sharedfolder::write_nine(
                path,
                KEY,
                &sharedfolder::WriteRequest {
                    lineage: &copy.header.lineage,
                    revision: copy.header.revision,
                    writer: &copy.header.writer,
                    writer_name: &copy.header.writer_name,
                    schema_version: copy.header.schema_version,
                },
                &copy.payload,
                &copy.blobs,
            )
            .unwrap();
            *keep.borrow_mut() = std::fs::read(path).unwrap();
        }))
    });
    let error = try_sync(&mut t.a).unwrap_err().to_string();
    assert!(error.contains("mudou"), "{error}");
    assert!(!replaced.borrow().is_empty(), "the hook ran");
    assert_eq!(std::fs::read(&nine).unwrap(), *replaced.borrow());
    assert_eq!(
        t.a.list_offline_outbox().unwrap().len(),
        1,
        "the note waits"
    );
}

/// B13. A device that read a 9 keeps refusing the old 8 after another
/// library (portable or recovered) is put in use on it.
#[test]
fn f2b_b13_floor_survives_library_replacement() {
    let mut t = two();
    let device = t.dir.path().join("dispositivo-a");
    t.a.set_device_dir(device.clone());
    let old8 = std::fs::read(&t.drive).unwrap();
    assert!(sync(&mut t.a).promoted);
    let lineage = t.lineage();
    let other_dir = t.dir.path().join("recuperado");
    std::fs::create_dir_all(&other_dir).unwrap();
    let mut other = Store::open(&other_dir.join("acervo.sqlite3")).unwrap();
    other.set_device_dir(device.clone());
    let mut cfg = folder_config("Casa", vec![t.drive.display().to_string()]);
    cfg.folder_lineage = lineage.clone();
    other.save_intranet_config(&cfg).unwrap();
    // The folder shows only the old 8 (the 9 hidden by the cloud).
    let nine = t.nine();
    let hidden = t.dir.path().join("escondido.ponte");
    std::fs::rename(&nine, &hidden).unwrap();
    std::fs::write(&t.drive, &old8).unwrap();
    assert_eq!(other.format_floor(&lineage).unwrap(), 9);
    let error = try_sync(&mut other).unwrap_err().to_string();
    assert!(error.contains("ainda não chegou"), "{error}");
    assert_eq!(std::fs::read(&t.drive).unwrap(), old8, "no 8 written");
    assert!(!nine.exists());
    assert!(
        other.list_documents("").unwrap().is_empty(),
        "nothing adopted"
    );
}

/// B14. The floor is raised by another process after the folders were
/// read: no 8 is written, and an 8 result is not adopted.
#[test]
fn f2b_b14_floor_rechecked_before_8_write_and_adoption() {
    for point in ["write", "adoption"] {
        let mut t = two();
        hooks::PROMOTION.with(|c| c.set(false));
        let doc = t.a.list_documents("").unwrap()[0].id;
        t.a.save_note(doc, "Nota que espera").unwrap();
        let before = std::fs::read(&t.drive).unwrap();
        let lineage = t.lineage();
        let floor = PathBuf::from(format!(
            "{}-{}",
            t.dir.path().join("a.sqlite3").display(),
            crate::formatfloor::FILE
        ));
        let raise = move || {
            crate::formatfloor::raise(
                &floor,
                &lineage,
                crate::formatfloor::Floor {
                    format: 9,
                    level: 1,
                },
            )
            .unwrap()
        };
        match point {
            "write" => hooks::BEFORE_FOLDER_WRITE.with(|h| *h.borrow_mut() = Some(Box::new(raise))),
            _ => hooks::BEFORE_ADOPTION.with(|h| *h.borrow_mut() = Some(Box::new(raise))),
        }
        assert!(try_sync(&mut t.a).is_err(), "{point}: refused");
        if point == "write" {
            assert_eq!(std::fs::read(&t.drive).unwrap(), before, "no 8 written");
        }
        assert_eq!(
            t.a.list_offline_outbox().unwrap().len(),
            1,
            "{point}: not adopted, the note waits"
        );
        hooks::BEFORE_FOLDER_WRITE.with(|h| *h.borrow_mut() = None);
        hooks::BEFORE_ADOPTION.with(|h| *h.borrow_mut() = None);
    }
}

/// B15. A Ponte 0.50 writes its 8 over the stub. The next exchange of a
/// 22 device takes it in as an old copy, keeps it aside and puts the stub
/// back, which an older reader refuses again.
#[test]
fn f2b_b15_old_writer_over_the_stub() {
    let mut t = promoted();
    let mut task = crate::tasks::Task::standalone();
    task.title = "Tarefa avulsa do 0.50".into();
    let donor = old_writer_over(
        &t.drive,
        &t.nine(),
        vec![entry(
            "donor-avulsa",
            "Criar tarefa",
            IntranetMutation::SaveTask {
                task,
                expected_version: None,
                no_project: false,
            },
            vec![crate::idmap::CreatedId::new(
                crate::idmap::IdKind::Task,
                "",
                88,
            )],
        )],
    );
    sync(&mut t.a);
    assert!(
        t.a.list_tasks()
            .unwrap()
            .iter()
            .any(|x| x.title == "Tarefa avulsa do 0.50"),
        "a self-contained creation is taken in"
    );
    assert!(aside_files(t.drive.parent().unwrap())
        .iter()
        .any(|p| std::fs::read(p).unwrap() == donor));
    assert!(is_stub(&t.drive), "the stub is back");
    let refused = sharedfolder::read_header_up_to(&t.drive, 8)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("Atualize"), "{refused}");
}

// ------------------------------------------------------------- B16–B19 --

/// B16. An empty folder lineage is promoted with zero triples and a
/// non-empty epoch, and a second computer adopts it.
#[test]
fn f2b_b16_empty_library_round_trips_as_9() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", files.clone()))
        .unwrap();
    sync(&mut a);
    assert!(sync(&mut a).promoted);
    let nine = sharedfolder::nine_path(&drive);
    let copy = copy_at(&nine);
    assert!(copy.payload.snapshot.idents.is_empty(), "zero triples");
    assert!(!copy.payload.snapshot.ident_epoch.is_empty());
    crate::idents::check(&copy.payload.snapshot, Some(&copy.header)).unwrap();
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    sync(&mut b);
    assert_eq!(origin(&b).0, "adopted");
    assert_eq!(
        origin(&b).2.as_deref(),
        Some(copy.payload.snapshot.ident_epoch.as_str())
    );
}

/// B17. Purging the last item leaves zero rows: the exchange goes on.
#[test]
fn f2b_b17_purging_the_last_row_keeps_exchanging() {
    let mut t = promoted();
    for d in t.a.list_documents("").unwrap() {
        let target = || crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: d.id,
        };
        t.a.trash_item(target()).unwrap();
        t.a.purge_trash_item(target()).unwrap();
    }
    for task in t.a.list_tasks().unwrap() {
        let target = || crate::trash::Target {
            kind: crate::trash::Kind::Task,
            id: task.id,
        };
        let _ = t.a.trash_item(target());
        let _ = t.a.purge_trash_item(target());
    }
    sync(&mut t.a);
    sync(&mut t.b);
    let copy = copy_at(&t.nine());
    crate::idents::check(&copy.payload.snapshot, Some(&copy.header)).unwrap();
    assert!(t.b.list_documents("").unwrap().is_empty());
    assert_eq!(idents(&t.b), triples(&t.nine()));
}

/// B18. A trashed task whose payload cannot be read is left out of the
/// export and has no triple: export, check and the receiver succeed. (The
/// exporter that reads a live library is the main computer's; a folder
/// exchange exports its temporary library, built from the file.)
#[test]
fn f2b_b18_unreadable_trashed_task_has_no_triple() {
    let mut team = Team::new(true);
    let mut task = crate::tasks::Task::standalone();
    task.title = "Tarefa que vai estragar".into();
    let id = team.host.save_task(&task).unwrap();
    team.host
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Task,
            id,
        })
        .unwrap();
    // Damaged on disk: written past the guards of the Lixeira, which are
    // put back as they were.
    let guards: Vec<(String, String)> = team
        .host
        .conn
        .prepare("SELECT name, sql FROM sqlite_master WHERE type='trigger' AND tbl_name='tasks' AND sql IS NOT NULL")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for (name, _) in &guards {
        team.host
            .conn
            .execute_batch(&format!("DROP TRIGGER \"{name}\";"))
            .unwrap();
    }
    team.host
        .conn
        .execute("UPDATE tasks SET payload=x'00010203' WHERE id=?1", [id])
        .unwrap();
    for (_, sql) in &guards {
        team.host.conn.execute_batch(sql).unwrap();
    }
    let RpcResponse::Snapshot(snapshot) = team.ask(v2_sync(None, "")) else {
        panic!("a snapshot")
    };
    crate::idents::check(&snapshot, None).unwrap();
    assert!(!snapshot.tasks.iter().any(|x| x.id == id), "left out");
    assert!(
        !snapshot.idents.iter().any(|t| t.0 == "task" && t.1 == id),
        "no triple"
    );
    team.client.sync_from_host(false).unwrap();
    assert!(!idents(&team.client).contains_key(&("task".to_owned(), id)));
}

/// B19. A missing triple is caught before any destination is touched.
#[test]
fn f2b_b19_writer_validates_before_touching_any_destination() {
    let mut t = two();
    let folder = t.drive.parent().unwrap().to_path_buf();
    let names = files_in(&folder);
    let before = std::fs::read(&t.drive).unwrap();
    hooks::DROP_TRIPLE.with(|c| c.set(true));
    let error = try_sync(&mut t.a).unwrap_err().to_string();
    assert!(error.contains("identidade"), "{error}");
    assert_eq!(files_in(&folder), names, "no file created");
    assert_eq!(std::fs::read(&t.drive).unwrap(), before, "byte-identical");
    assert_eq!(t.a.format_floor(&t.lineage()).unwrap() < 9, true);
}

// ------------------------------------------------------------- B20–B24 --

/// B20. Nothing new anywhere (the revision shortcut would stop): the
/// folder is promoted in one exchange.
#[test]
fn f2b_b20_unchanged_folder_promotes_in_one_exchange() {
    let mut t = two();
    let report = sync(&mut t.a);
    assert!(report.promoted, "{:?}", report.promotion_wait);
    assert!(t.nine().is_file() && is_stub(&t.drive));
    assert_eq!(origin(&t.a).0, "stamped");
    assert_eq!(idents(&t.a), triples(&t.nine()));
    let report_b = sync(&mut t.b);
    assert!(!report_b.promoted);
    assert_eq!(
        idents(&t.b),
        triples(&t.nine()),
        "B takes the same identities"
    );
}

/// B21. A juntada derived when the library was opened (a link with no
/// queued change, so the library is not the folder's content) does not
/// keep the folder from being promoted.
#[test]
fn f2b_b21_juntada_at_open_does_not_block_promotion() {
    let mut t = two();
    // What opening the window derives (a juntada) changes the library
    // without a queued change: applied with the queue bypassed.
    t.a.suppress_remote = true;
    let other =
        t.a.import_capture(&capture(&number("a", 5), "Outro documento juntado"))
            .unwrap()
            .document_id;
    t.a.suppress_remote = false;
    assert!(other > 0);
    assert!(t.a.list_offline_outbox().unwrap().is_empty());
    let report = sync(&mut t.a);
    assert!(report.promoted, "{:?}", report.promotion_wait);
}

/// B22. Copies that diverged (a mirror lagging behind) or a guard newer
/// than the base: an ordinary 8 exchange with the reason, then the next
/// exchange promotes.
#[test]
fn f2b_b22_divergent_copies_or_newer_guard_refuse_with_reason() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
    let both = vec![drive.display().to_string(), pen.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
        .unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    hooks::PROMOTION.with(|c| c.set(false));
    sync(&mut a);
    let mut cfg = a.load_intranet_config().unwrap();
    cfg.folder_files = both.clone();
    a.save_intranet_config(&cfg).unwrap();
    sync(&mut a);
    // The pen drive lags behind the Drive.
    cfg.folder_files = vec![drive.display().to_string()];
    a.save_intranet_config(&cfg).unwrap();
    a.save_note(a.list_documents("").unwrap()[0].id, "Só no Drive")
        .unwrap();
    sync(&mut a);
    cfg.folder_files = both.clone();
    a.save_intranet_config(&cfg).unwrap();
    on();
    let report = sync(&mut a);
    assert!(!report.promoted);
    let wait = report.promotion_wait.unwrap_or_default();
    assert!(
        wait.starts_with("Sincronize antes de atualizar o arquivo da pasta:")
            && wait.ends_with("Até lá o Ponte continua sincronizando no formato anterior."),
        "{wait}"
    );
    assert_eq!(sharedfolder::read_header(&pen).unwrap().unwrap().format, 8);
    let next = sync(&mut a);
    assert!(next.promoted, "{:?}", next.promotion_wait);
    assert!(is_stub(&drive) && is_stub(&pen));
}

/// B24. A configured folder out of reach: the promotion waits, and the
/// reason names the folder.
#[test]
fn f2b_b24_unreachable_folder_waits() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
    let both = vec![drive.display().to_string(), pen.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", both))
        .unwrap();
    sync(&mut a);
    std::fs::remove_dir_all(pen.parent().unwrap()).unwrap();
    let report = sync(&mut a);
    assert!(!report.promoted);
    let wait = report.promotion_wait.unwrap_or_default();
    assert!(wait.contains("Pendrive"), "{wait}");
    assert!(!sharedfolder::nine_path(&drive).exists(), "the 9 waits");
}

// ------------------------------------------------------------- B25–B29 --

/// A principal computer (host) and a client on a private port.
struct Team {
    dir: tempfile::TempDir,
    host: Store,
    client: Store,
    db: PathBuf,
    token: String,
    server: Option<crate::intranet::HostServer>,
}

impl Team {
    fn new(with_document: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("host.sqlite3");
        let token = crate::intranet::generate_token();
        let mut host = Store::open(&db).unwrap();
        host.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "ANFITRIAO".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: "http://127.0.0.1:7878".into(),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        })
        .unwrap();
        if with_document {
            host.import_capture(&crate::model::sample_capture())
                .unwrap();
        }
        let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
        client
            .save_intranet_config(&IntranetConfig {
                mode: IntranetMode::Client,
                member_name: "SERVIDOR-A".into(),
                host_url: "http://127.0.0.1:1".into(),
                cabinet_token: token.clone(),
                ..IntranetConfig::default()
            })
            .unwrap();
        let mut team = Self {
            dir,
            host,
            client,
            db,
            token,
            server: None,
        };
        team.online();
        team
    }

    fn online(&mut self) {
        self.offline();
        let server = crate::intranet::HostServer::start(self.db.clone(), "127.0.0.1:0").unwrap();
        let mut cfg = self.client.load_intranet_config().unwrap();
        cfg.host_url = format!("http://{}", server.local_addr());
        cfg.cabinet_token = self.token.clone();
        self.client.save_intranet_config(&cfg).unwrap();
        self.server = Some(server);
    }

    fn offline(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
        let mut cfg = self.client.load_intranet_config().unwrap();
        cfg.host_url = "http://127.0.0.1:1".into();
        self.client.save_intranet_config(&cfg).unwrap();
    }

    fn url(&self) -> String {
        self.client.load_intranet_config().unwrap().host_url
    }

    fn ask(&mut self, request: RpcRequest) -> RpcResponse {
        let cfg = self.host.load_intranet_config().unwrap();
        crate::intranet::process_rpc(
            &mut self.host,
            &cfg,
            RpcRequest::V2 {
                level: sharedfolder::LEVEL,
                request: Box::new(request),
            },
        )
        .unwrap()
    }
}

impl Drop for Team {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
    }
}

fn v2_sync(since: Option<i64>, known_epoch: &str) -> RpcRequest {
    RpcRequest::Sync {
        since_revision: since,
        known_epoch: known_epoch.into(),
    }
}

/// B25. Host and client at the same revision: a V2 `Sync` with no epoch
/// gets the first snapshot with identities, and with the epoch "up to
/// date". The client takes the host's identities.
#[test]
fn f2b_b25_same_revision_client_gets_its_first_22_snapshot() {
    let mut team = Team::new(true);
    team.client.sync_from_host(false).unwrap();
    let revision = team.host.cabinet_revision().unwrap();
    let RpcResponse::Snapshot(snapshot) = team.ask(v2_sync(Some(revision), "")) else {
        panic!("a snapshot")
    };
    let epoch = snapshot.ident_epoch.clone();
    assert!(!epoch.is_empty());
    crate::idents::check(&snapshot, None).unwrap();
    assert_eq!(snapshot.idents.len(), idents(&team.host).len());
    match team.ask(v2_sync(Some(revision), &epoch)) {
        RpcResponse::UpToDate {
            epoch: told, level, ..
        } => {
            assert_eq!(told, epoch);
            assert_eq!(level, sharedfolder::LEVEL);
        }
        _ => panic!("up to date with the same epoch"),
    }
    team.client.sync_from_host(false).unwrap();
    assert_eq!(idents(&team.client), idents(&team.host));
    assert_eq!(origin(&team.client).0, "adopted");
    assert_eq!(origin(&team.client).2.as_deref(), Some(epoch.as_str()));
}

/// B26. The snapshot of a `Mutated` answer that does not check: nothing is
/// learned, no frame is kept and the change stays queued.
#[test]
fn f2b_b26_invalid_mutated_snapshot_keeps_the_entry() {
    let mut team = Team::new(true);
    team.client.sync_from_host(false).unwrap();
    team.offline();
    let doc = team.client.list_documents("").unwrap()[0].id;
    team.client.save_note(doc, "Nota do cliente").unwrap();
    assert_eq!(team.client.list_offline_outbox().unwrap().len(), 1);
    team.online();
    *hooks::CORRUPT_MUTATED.lock().unwrap() = Some(team.db.clone());
    let revision = team.client.cabinet_revision().unwrap();
    let error = team.client.sync_from_host(false).unwrap_err().to_string();
    assert!(error.contains("identidade"), "{error}");
    assert_eq!(
        team.client.list_offline_outbox().unwrap().len(),
        1,
        "queued"
    );
    let learned: i64 = team
        .client
        .conn
        .query_row("SELECT count(*) FROM offline_id_map", [], |r| r.get(0))
        .unwrap();
    assert_eq!(learned, 0, "nothing learned");
    assert_eq!(team.client.cabinet_revision().unwrap(), revision);
    *hooks::CORRUPT_MUTATED.lock().unwrap() = None;
}

/// B27. A request of an older Ponte (bare, not V2) gets 503 before the
/// library is opened, even when it is locked; the older client keeps its
/// queue (503 is "out of reach" there) and the portable export, which
/// opens the library as a service, still works.
#[test]
fn f2b_b27_legacy_rpc_gets_503_without_opening() {
    let team = Team::new(true);
    let locker = rusqlite::Connection::open(&team.db).unwrap();
    locker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap();
    for body in [
        r#"{"Sync":{"since_revision":null}}"#,
        r#""Ping""#,
        r#"{"FetchAttachmentBytes":{"id":1}}"#,
    ] {
        let response = client
            .post(format!("{}/api/v1/rpc", team.url()))
            .header("Authorization", format!("Bearer {}", team.token))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), 503, "{body}");
        let text = response.text().unwrap();
        assert!(text.contains("foi atualizado"), "{text}");
    }
    locker.execute_batch("ROLLBACK;").unwrap();
    drop(locker);
    let out = team.dir.path().join("copia.pontecopia");
    crate::portable_copy::export(&team.db, &out, KEY).unwrap();
    assert!(out.is_file());
}

/// B28. The host's epoch is a nonce kept in its library: a raw backup of
/// the 22 library brings the same epoch back; a 21 backup gets a new one,
/// and a client at the same revision then takes a whole snapshot.
#[test]
fn f2b_b28_host_epoch_is_a_persisted_nonce() {
    let mut team = Team::new(true);
    let revision = team.host.cabinet_revision().unwrap();
    let RpcResponse::Snapshot(first) = team.ask(v2_sync(None, "")) else {
        panic!()
    };
    let epoch = first.ident_epoch.clone();
    let backup22 = team.dir.path().join("backup22.sqlite3");
    team.host
        .conn
        .execute("VACUUM INTO ?1", [backup22.display().to_string()])
        .unwrap();
    let RpcResponse::Snapshot(again) = team.ask(v2_sync(None, "")) else {
        panic!()
    };
    assert_eq!(again.ident_epoch, epoch, "kept");
    // A raw 22 backup put back.
    team.offline();
    let db = team.db.clone();
    let reopen = |team: &mut Team, from: &Path| {
        team.host = Store::open(std::path::Path::new(":memory:")).unwrap();
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
        }
        std::fs::copy(from, &db).unwrap();
        team.host = Store::open(&db).unwrap();
    };
    reopen(&mut team, &backup22);
    let RpcResponse::Snapshot(restored) = team.ask(v2_sync(None, "")) else {
        panic!()
    };
    assert_eq!(restored.ident_epoch, epoch, "the same epoch");
    // A 21 backup put back.
    let backup21 = team.dir.path().join("backup21.sqlite3");
    std::fs::copy(&backup22, &backup21).unwrap();
    crate::idents::test_support::downgrade_to_21(&backup21).unwrap();
    reopen(&mut team, &backup21);
    let RpcResponse::Snapshot(fresh) = team.ask(v2_sync(None, "")) else {
        panic!()
    };
    assert!(!fresh.ident_epoch.is_empty());
    assert_ne!(fresh.ident_epoch, epoch, "a new nonce");
    assert_eq!(team.host.cabinet_revision().unwrap(), revision);
    assert!(
        matches!(
            team.ask(v2_sync(Some(revision), &epoch)),
            RpcResponse::Snapshot(_)
        ),
        "a client with the old epoch takes a whole snapshot"
    );
}

/// B29. A portable copy of a 22 library carries its triples and epoch,
/// and the library made from it keeps them.
#[test]
fn f2b_b29_portable_22_round_trip_keeps_triples_and_epoch() {
    let t = promoted();
    let epoch = origin(&t.a).2.expect("an epoch");
    let out = t.dir.path().join("a.pontecopia");
    crate::portable_copy::export(&t.dir.path().join("a.sqlite3"), &out, KEY).unwrap();
    let home = t.dir.path().join("restaurados");
    std::fs::create_dir_all(&home).unwrap();
    let place = crate::recovery::Place::new(Some(&home), &t.dir.path().join("a.sqlite3"));
    let built = crate::portable_copy::import(
        &out,
        KEY,
        &place,
        chrono::NaiveDate::from_ymd_opt(2026, 11, 23)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap(),
    )
    .unwrap();
    let restored = Store::open(&crate::paths::db_path_in(&built.folder)).unwrap();
    assert_eq!(idents(&restored), idents(&t.a), "the same triples");
    assert_eq!(origin(&restored).2.as_deref(), Some(epoch.as_str()));
    assert_eq!(origin(&restored).0, "provisional");
}

// ------------------------------------------------------------------ B30 --

/// B30. The cost of the identities on a large library: building and
/// checking the triples, and the bytes they add to the file. Run with
/// `--ignored --nocapture`; prints the measures.
#[test]
#[ignore]
fn f2b_cost_budget() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("grande.sqlite3")).unwrap();
    let doc = store
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    // 50k rows: tasks inserted through the product path in one go.
    let template = {
        let mut task = crate::tasks::Task::new(doc);
        task.title = "Tarefa de carga".into();
        task
    };
    let id = store.save_task(&template).unwrap();
    let payload: Vec<u8> = store
        .conn
        .query_row("SELECT payload FROM tasks WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    {
        let tx = store.conn.transaction().unwrap();
        for n in 0..50_000 {
            tx.execute(
                "INSERT INTO tasks(document_id,payload,uid) VALUES(?1,?2,?3)",
                rusqlite::params![doc, payload, format!("carga-{n}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    let rows = idents(&store).len();
    let start = std::time::Instant::now();
    let plain = store.export_snapshot().unwrap();
    let export = start.elapsed();
    let start = std::time::Instant::now();
    let with = store.export_with_idents("epoca-de-teste").unwrap();
    let export_with = start.elapsed();
    let start = std::time::Instant::now();
    crate::idents::check(&with, None).unwrap();
    let check = start.elapsed();
    let bytes_plain = serde_json::to_vec(&plain).unwrap().len();
    let bytes_with = serde_json::to_vec(&with).unwrap().len();
    eprintln!(
        "B30: {rows} rows; export {export:?}, export with identities {export_with:?}, check {check:?}; JSON {bytes_plain} → {bytes_with} bytes (+{} bytes)",
        bytes_with - bytes_plain
    );
    assert_eq!(with.idents.len(), rows);
    let extra = export_with.saturating_sub(export) + check;
    assert!(
        extra.as_secs_f64() <= export.as_secs_f64() * 0.10 + 0.5,
        "time: +{extra:?} over {export:?}"
    );
}
