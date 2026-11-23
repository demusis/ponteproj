//! F2b review (`revisao-f2b.md`): one regression test per finding
//! F2B-01 to F2B-11, written to run against the reviewed head (9c61cb9) as
//! well (F2B-05 and F2B-09 are tests of the phone: `SyncIoTest` and the
//! phone core).
#![allow(clippy::all)]

use crate::idents::hooks;
use crate::intranet::{IntranetConfig, IntranetMode, IntranetMutation, RpcRequest, RpcResponse};
use crate::sharedfolder::{self, Found, JournalEntry};
use crate::storage::Store;
use std::path::{Path, PathBuf};

const KEY: &str = "senha-de-teste-123";

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

fn try_sync(store: &mut Store) -> anyhow::Result<crate::intranet::FolderSyncReport> {
    let cfg = store.load_intranet_config().unwrap();
    store.sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
}

fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    try_sync(store).unwrap()
}

fn copy_at(path: &Path) -> sharedfolder::Copy {
    match sharedfolder::read_any(path, KEY).unwrap() {
        Some(Found::Copy(copy)) => copy,
        _ => panic!("{} is not a copy", path.display()),
    }
}

fn is_stub(path: &Path) -> bool {
    matches!(
        sharedfolder::read_any(path, KEY),
        Ok(Some(Found::Moved { .. }))
    )
}

fn format_of(path: &Path) -> u32 {
    sharedfolder::read_header(path).unwrap().unwrap().format
}

/// A and B share one folder, in sync at format 8 on one SIGADOC document.
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
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    sync(&mut b);
    Two { dir, drive, a, b }
}

fn two() -> Two {
    two_named("ponte-sync.ponte")
}

fn promoted() -> Two {
    let mut t = two();
    assert!(sync(&mut t.a).promoted);
    sync(&mut t.b);
    t
}

fn nine(t: &Two) -> PathBuf {
    sharedfolder::nine_path(&t.drive)
}

fn entry(
    uid: &str,
    mutation: IntranetMutation,
    created: Vec<crate::idmap::CreatedId>,
) -> JournalEntry {
    JournalEntry {
        uid: uid.into(),
        created_at: "2026-11-20T12:00:00Z".into(),
        actor: "Pessoa do aparelho antigo".into(),
        writer: "aparelho-antigo".into(),
        label: "Alteração antiga".into(),
        mutation,
        base: None,
        revision: 1000,
        created,
        idents: Vec::new(),
        refs: None,
    }
}

/// An older Ponte writes its 8 at `path`: the 9's content plus `entries`.
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
    std::fs::read(path).unwrap()
}

fn origin(store: &Store) -> (String, Option<String>, Option<String>) {
    crate::idents::origin(&store.conn).unwrap()
}

// ---------------------------------------------------------------- F2B-01 --

/// F2B-01. An old copy imports a new version of a document the folder
/// already has (its receipt names the document and its triage task, which
/// it did NOT create), then sends that triage task to the Lixeira, and
/// changes the holidays. Nothing of it touches the existing items: three
/// cards, no new version, the triage task where it was, the holidays as
/// they were.
#[test]
fn f2b_r01_an_old_copy_never_acts_on_an_item_it_did_not_create() {
    let mut t = promoted();
    let doc = t.a.list_documents("").unwrap()[0].id;
    let triage =
        t.a.list_tasks()
            .unwrap()
            .into_iter()
            .find(|task| task.document_id == Some(doc))
            .expect("the triage task")
            .id;
    t.a.set_library_setting("holidays", "uf MT\n").unwrap();
    sync(&mut t.a);
    let versions = |s: &Store| -> i64 {
        s.conn
            .query_row(
                "SELECT count(*) FROM captures WHERE document_id=?1",
                [doc],
                |r| r.get(0),
            )
            .unwrap()
    };
    let before = versions(&t.a);
    let mut again = crate::model::sample_capture();
    again.text = "Versão nova vista só no aparelho antigo".into();
    again.captured_at = "2026-11-21T12:00:00Z".into();
    use crate::idmap::{CreatedId, IdKind};
    old_writer_over(
        &t.drive,
        &nine(&t),
        vec![
            entry(
                "antigo-reimporta",
                IntranetMutation::ImportCapture { capture: again },
                vec![
                    CreatedId::new(IdKind::Document, "", doc),
                    CreatedId::new(IdKind::Capture, "", 77),
                    CreatedId::new(IdKind::Task, "triage", triage),
                ],
            ),
            entry(
                "antigo-lixeira",
                IntranetMutation::TrashItem {
                    kind: "task".into(),
                    id: triage,
                },
                vec![],
            ),
            entry(
                "antigo-feriados",
                IntranetMutation::SaveSetting {
                    key: "holidays".into(),
                    value: "uf SP\n".into(),
                    seed: false,
                },
                vec![],
            ),
        ],
    );
    sync(&mut t.a);
    assert_eq!(
        versions(&t.a),
        before,
        "no new version on the existing record"
    );
    assert!(
        t.a.list_tasks()
            .unwrap()
            .iter()
            .any(|task| task.id == triage),
        "the triage task was not sent to the Lixeira"
    );
    assert_eq!(
        t.a.library_setting("holidays").unwrap().as_deref(),
        Some("uf MT\n"),
        "the holidays were not overwritten"
    );
    let cards =
        t.a.list_sync_conflicts()
            .unwrap()
            .into_iter()
            .filter(|c| c.reason.contains("versão anterior do Ponte"))
            .count();
    assert_eq!(cards, 3);
}

// ---------------------------------------------------------------- F2B-02 --

/// F2B-02. Right before the stub takes the old name, another device writes
/// a new 8 there. Its file is never replaced unseen: it stays at its name,
/// and the next exchange takes its change in and puts the stub back.
#[test]
fn f2b_r02_a_file_written_while_the_stub_goes_in_is_never_lost() {
    let mut t = two();
    let drive = t.drive.clone();
    let nine_file = nine(&t);
    let mut task = crate::tasks::Task::standalone();
    task.title = "Tarefa de B gravada no último instante".into();
    // B (an older writer) publishes its 8 right before the stub takes the
    // old name.
    at_rename_of(drive.clone(), move || {
        old_writer_over(
            &drive,
            &nine_file,
            vec![entry(
                "b-ultimo-instante",
                IntranetMutation::SaveTask {
                    task: task.clone(),
                    expected_version: None,
                    no_project: false,
                },
                vec![crate::idmap::CreatedId::new(
                    crate::idmap::IdKind::Task,
                    "",
                    91,
                )],
            )],
        );
    });
    let _ = try_sync(&mut t.a);
    let _ = try_sync(&mut t.a);
    assert!(
        t.a.list_tasks()
            .unwrap()
            .iter()
            .any(|x| x.title == "Tarefa de B gravada no último instante"),
        "B's change reached the result"
    );
    assert!(is_stub(&t.drive), "and the stub is in place at last");
}

/// Runs `action` right before the next rename onto `target` (the other
/// renames go on as usual).
fn at_rename_of(target: PathBuf, action: impl FnOnce() + 'static) {
    let action = std::cell::RefCell::new(Some(Box::new(action) as Box<dyn FnOnce()>));
    fn arm(target: PathBuf, action: std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnOnce()>>>>) {
        hooks::BEFORE_RENAME.with(|h| {
            *h.borrow_mut() = Some(Box::new(move |path: &Path| {
                if path == target {
                    if let Some(run) = action.borrow_mut().take() {
                        run();
                    }
                } else {
                    arm(target.clone(), action.clone());
                }
            }))
        });
    }
    arm(target, std::rc::Rc::new(action));
}

/// The next publication of `target` fails as a full disk or a refused
/// write would (its temporary file is gone when it is renamed); the other
/// files are written as usual.
fn fail_publishing(target: PathBuf) {
    hooks::BEFORE_RENAME.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |path: &Path| {
            if path != target {
                fail_publishing(target.clone());
                return;
            }
            let name = target.file_name().unwrap().to_string_lossy().into_owned();
            for entry in std::fs::read_dir(target.parent().unwrap()).unwrap() {
                let entry = entry.unwrap();
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!(".{name}.tmp-"))
                {
                    std::fs::remove_file(entry.path()).unwrap();
                }
            }
        }))
    });
}

// ---------------------------------------------------------------- F2B-03 --

/// F2B-03. The file of format 9 cannot be published (a full disk, a refused
/// write): the promotion stops and declares
/// nothing; the guard, the floor and the origin stay as they were. With two
/// mirrors, a stop after the first still leaves the second as an 8, and the
/// next exchange finishes: every mirror ends with its 9 and its stub.
#[test]
fn f2b_r03_a_promotion_that_cannot_publish_declares_nothing() {
    let mut t = two();
    let blocked = nine(&t);
    fail_publishing(blocked.clone());
    let result = try_sync(&mut t.a);
    assert!(
        result.as_ref().map_or(true, |r| !r.promoted),
        "nothing declared"
    );
    assert!(result.is_err(), "the exchange says it stopped");
    let guard = PathBuf::from(format!(
        "{}-guarda.ponte",
        t.dir.path().join("a.sqlite3").display()
    ));
    assert_eq!(format_of(&guard), 8, "the guard is still the last 8");
    assert_ne!(origin(&t.a).0, "stamped");
    assert_eq!(format_of(&t.drive), 8, "the old file is untouched");
    hooks::BEFORE_RENAME.with(|h| *h.borrow_mut() = None);
    sync(&mut t.a);
    assert!(is_stub(&t.drive) && format_of(&blocked) == 9);
}

/// F2B-03, two mirrors: the second cannot take its 9.
#[test]
fn f2b_r03_two_mirrors_end_with_their_9_and_stub_or_stay_8() {
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
    a.import_capture(&crate::model::sample_capture()).unwrap();
    sync(&mut a);
    let pen_nine = sharedfolder::nine_path(&pen);
    fail_publishing(pen_nine.clone());
    // Whatever the first mirror got, the exchange does not declare success
    // and the second keeps its 8.
    let first = try_sync(&mut a);
    assert!(first.as_ref().map_or(true, |r| !r.promoted));
    assert_eq!(format_of(&pen), 8);
    hooks::BEFORE_RENAME.with(|h| *h.borrow_mut() = None);
    let _ = try_sync(&mut a);
    let _ = try_sync(&mut a);
    for path in [&drive, &pen] {
        assert!(is_stub(path), "{} has its stub", path.display());
        assert_eq!(format_of(&sharedfolder::nine_path(path)), 9);
    }
}

// ---------------------------------------------------------------- F2B-04 --

/// F2B-04. B went on exchanging in 8 (its recorded revision is past the
/// revision of the first 9). The first 9 is still taken: the format
/// decides, not the revision.
#[test]
fn f2b_r04_a_higher_8_revision_never_blocks_the_first_9() {
    let mut t = two();
    assert!(sync(&mut t.a).promoted);
    let revision = copy_at(&nine(&t)).header.revision;
    let mut cfg = t.b.load_intranet_config().unwrap();
    cfg.synced_revision = revision + 5;
    t.b.save_intranet_config(&cfg).unwrap();
    sync(&mut t.b);
    assert_eq!(origin(&t.b).0, "adopted");
}

// ---------------------------------------------------------------- F2B-06 --

/// F2B-06. The process stops after the stub replaced the old copy and
/// before the local adoption: the card made from the old copy was already
/// recorded. And a guard of format 8 that does not open with this password
/// is kept aside, never replaced as it is.
#[test]
fn f2b_r06_cards_are_recorded_before_the_old_copy_is_replaced() {
    let mut t = promoted();
    old_writer_over(
        &t.drive,
        &nine(&t),
        vec![entry(
            "antigo-cartao",
            IntranetMutation::AddAttachment {
                owner_kind: "document".into(),
                owner_id: t.a.list_documents("").unwrap()[0].id,
                name: "oficio.txt".into(),
                description: "Anexo do aparelho antigo".into(),
                data_base64: "dGV4dG8=".into(),
            },
            vec![crate::idmap::CreatedId::new(
                crate::idmap::IdKind::Attachment,
                "",
                55,
            )],
        )],
    );
    hooks::BEFORE_ADOPTION
        .with(|h| *h.borrow_mut() = Some(Box::new(|| panic!("processo encerrado (teste)"))));
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| try_sync(&mut t.a)));
    assert!(crashed.is_err(), "the process stopped");
    hooks::BEFORE_ADOPTION.with(|h| *h.borrow_mut() = None);
    assert!(
        t.a.list_sync_conflicts()
            .unwrap()
            .iter()
            .any(|c| c.mutation_uid == "antigo-cartao"),
        "the card was recorded before the old copy left its name"
    );
}

/// F2B-06, the guard: an old guard that does not open is kept aside.
#[test]
fn f2b_r06_an_old_guard_that_does_not_open_is_kept_aside() {
    let mut t = promoted();
    let guard = PathBuf::from(format!(
        "{}-guarda.ponte",
        t.dir.path().join("b.sqlite3").display()
    ));
    let copy = copy_at(&nine(&t));
    let mut payload = copy.payload;
    payload.snapshot.idents.clear();
    payload.snapshot.ident_epoch.clear();
    sharedfolder::write(
        &guard,
        "outra-senha-antiga-123",
        &sharedfolder::WriteRequest {
            lineage: &copy.header.lineage,
            revision: copy.header.revision,
            writer: "b",
            writer_name: "Trabalho",
            schema_version: 21,
        },
        &payload,
        &copy.blobs,
    )
    .unwrap();
    let old = std::fs::read(&guard).unwrap();
    t.b.save_note(t.b.list_documents("").unwrap()[0].id, "Nota de B")
        .unwrap();
    sync(&mut t.b);
    let kept = std::fs::read_dir(t.dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .any(|p| p.to_string_lossy().contains("-divergente-") && std::fs::read(&p).unwrap() == old);
    assert!(kept, "the old guard is kept aside, byte for byte");
}

// ---------------------------------------------------------------- F2B-07 --

/// F2B-07. Two processes raise the floor of different folders at once:
/// none is lost.
#[test]
fn f2b_r07_raising_the_floor_never_loses_another_folder() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(crate::formatfloor::FILE);
    let floor = crate::formatfloor::Floor {
        format: 9,
        level: 1,
    };
    let workers: Vec<_> = (0..4)
        .map(|w| {
            let file = file.clone();
            std::thread::spawn(move || {
                for n in 0..40 {
                    crate::formatfloor::raise(&file, &format!("linhagem-{w}-{n}"), floor).unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    for w in 0..4 {
        for n in 0..40 {
            assert_eq!(
                crate::formatfloor::read(&file, &format!("linhagem-{w}-{n}"))
                    .unwrap()
                    .format,
                9,
                "linhagem-{w}-{n}"
            );
        }
    }
}

// ---------------------------------------------------------------- F2B-08 --

/// F2B-08. A main computer whose library stayed at schema 21 refuses the
/// synchronization and the changes of the protocol V2 before changing
/// anything.
#[test]
fn f2b_r08_a_main_computer_still_at_21_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("principal.sqlite3");
    let doc = {
        let mut store = Store::open(&db).unwrap();
        store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id
    };
    crate::idents::test_support::downgrade_to_21(&db).unwrap();
    hooks::FREE_SPACE.with(|h| *h.borrow_mut() = Some(Ok(0)));
    let mut host = Store::open(&db).unwrap();
    assert_eq!(
        crate::idents::test_support::user_version(&db).unwrap(),
        21,
        "it stayed at 21"
    );
    let cfg = host.load_intranet_config().unwrap();
    let notes = |s: &Store| -> i64 {
        s.conn
            .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
            .unwrap()
    };
    let before = notes(&host);
    let mutate = RpcRequest::V2 {
        level: sharedfolder::LEVEL,
        request: Box::new(RpcRequest::Mutate {
            actor: "Cliente".into(),
            mutation: IntranetMutation::SaveNote {
                document_id: doc,
                note: "Nota que não pode entrar".into(),
                expected_version: None,
            },
            client_schema: 21,
            expected_generation: String::new(),
            mutation_uid: "uid-de-teste-123456".into(),
            idents: Vec::new(),
            refs: None,
        }),
    };
    let answer = crate::intranet::process_rpc(&mut host, &cfg, mutate);
    assert!(
        !matches!(answer, Ok(RpcResponse::Mutated { .. })),
        "not applied"
    );
    assert_eq!(notes(&host), before, "nothing changed");
    let sync = RpcRequest::V2 {
        level: sharedfolder::LEVEL,
        request: Box::new(RpcRequest::Sync {
            since_revision: None,
            known_epoch: String::new(),
        }),
    };
    assert!(crate::intranet::process_rpc(&mut host, &cfg, sync).is_err());
}

// ---------------------------------------------------------------- F2B-10 --

/// F2B-10. The name reserved for the file of format 9 holds a 9 of another
/// library: the exchange goes on in 8, and the promotion waits, naming the
/// file to rename.
#[test]
fn f2b_r10_a_reserved_name_of_another_library_only_postpones() {
    let mut t = two_named("registro.ponte");
    // Another library, same password, promoted in another folder.
    let other = tempfile::tempdir().unwrap();
    let elsewhere = other.path().join("Outra").join("registro.ponte");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    let mut x = Store::open(&other.path().join("x.sqlite3")).unwrap();
    x.save_intranet_config(&folder_config(
        "Outro",
        vec![elsewhere.display().to_string()],
    ))
    .unwrap();
    sync(&mut x);
    assert!(sync(&mut x).promoted);
    std::fs::copy(sharedfolder::nine_path(&elsewhere), nine(&t)).unwrap();
    t.a.save_note(t.a.list_documents("").unwrap()[0].id, "Nota que vai no 8")
        .unwrap();
    let report = sync(&mut t.a);
    assert!(!report.promoted);
    let wait = report.promotion_wait.unwrap_or_default();
    assert!(
        wait.contains("registro-formato9.ponte") && wait.contains("renomeie-o"),
        "{wait}"
    );
    assert_eq!(format_of(&t.drive), 8);
    assert!(
        t.a.list_offline_outbox().unwrap().is_empty(),
        "the 8 exchange went on"
    );
}

// ---------------------------------------------------------------- F2B-11 --

/// F2B-11. A library that once served the protocol V2 (origin `host`)
/// starts a new folder: the folder is promoted all the same.
#[test]
fn f2b_r11_a_library_that_served_v2_still_promotes_a_folder() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let mut store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    store
        .import_capture(&crate::model::sample_capture())
        .unwrap();
    crate::idents::set_origin(&store.conn, "host", None, Some("nonce-do-principal")).unwrap();
    store
        .save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
        .unwrap();
    sync(&mut store);
    let report = sync(&mut store);
    assert!(report.promoted, "{:?}", report.promotion_wait);
    assert_eq!(origin(&store).0, "stamped");
}
