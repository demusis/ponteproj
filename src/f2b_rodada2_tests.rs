//! F2b, second review (`revisao-f2b-rodada-2.md`): one regression test per
//! finding R2-01 to R2-08, written to run against 6c70a5b as well (R2-05
//! and part of R2-06 are tests of the phone core).
#![allow(clippy::all)]

use crate::idents::hooks;
use crate::intranet::{IntranetConfig, IntranetMode, IntranetMutation};
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

fn origin(store: &Store) -> String {
    crate::idents::origin(&store.conn).unwrap().0
}

struct Two {
    dir: tempfile::TempDir,
    drive: PathBuf,
    a: Store,
    b: Store,
}

fn two() -> Two {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
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

fn standalone_task(title: &str) -> IntranetMutation {
    let mut task = crate::tasks::Task::standalone();
    task.title = title.into();
    IntranetMutation::SaveTask {
        task,
        expected_version: None,
        no_project: false,
    }
}

/// Every rename onto `target` fails as a full disk would (its temporary
/// file is gone), for as long as `while_on` says so; others go on.
fn failing_renames(target: PathBuf, while_on: std::rc::Rc<std::cell::Cell<bool>>) {
    hooks::BEFORE_RENAME.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |path: &Path| {
            if while_on.get() {
                failing_renames(target.clone(), while_on.clone());
            }
            if path != target || !while_on.get() {
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

fn archived_copy_of(folder: &Path, bytes: &[u8]) -> bool {
    std::fs::read_dir(folder)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .any(|p| {
            p.to_string_lossy().contains("-divergente-") && std::fs::read(&p).unwrap() == bytes
        })
}

// ---------------------------------------------------------------- R2-01 --

/// R2-01. The stub cannot be written at the old name (a full disk at that
/// instant): the old name still holds its file, never nothing.
#[test]
fn f2b_r2_01_the_old_name_is_never_left_empty() {
    let mut t = two();
    let before = std::fs::read(&t.drive).unwrap();
    let on = std::rc::Rc::new(std::cell::Cell::new(true));
    failing_renames(t.drive.clone(), on.clone());
    let _ = try_sync(&mut t.a);
    on.set(false);
    hooks::BEFORE_RENAME.with(|h| *h.borrow_mut() = None);
    assert!(t.drive.is_file(), "the old name is not empty");
    assert_eq!(
        std::fs::read(&t.drive).unwrap(),
        before,
        "it still holds the 8"
    );
    // And the next exchange puts the stub there.
    sync(&mut t.a);
    assert!(is_stub(&t.drive));
}

/// R2-01. A Ponte 0.51 with no lineage of its own found nothing usable and
/// started a NEW library at the old name: it is kept aside, named, and the
/// stub goes back (never an exchange stopped by "another library").
#[test]
fn f2b_r2_01_a_new_library_at_the_old_name_is_kept_and_the_stub_returns() {
    let mut t = promoted();
    let elsewhere = t.dir.path().join("Outro").join("ponte-sync.ponte");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    hooks::PROMOTION.with(|c| c.set(false));
    let mut old = Store::open(&t.dir.path().join("antigo.sqlite3")).unwrap();
    old.save_intranet_config(&folder_config(
        "Antigo",
        vec![elsewhere.display().to_string()],
    ))
    .unwrap();
    old.import_capture(&crate::model::sample_capture()).unwrap();
    sync(&mut old);
    on();
    std::fs::copy(&elsewhere, &t.drive).unwrap();
    let stranger = std::fs::read(&t.drive).unwrap();
    let report = sync(&mut t.a);
    assert!(is_stub(&t.drive), "the stub is back");
    assert!(
        archived_copy_of(t.drive.parent().unwrap(), &stranger),
        "the other library's file is kept, byte for byte"
    );
    assert!(
        report.notes.iter().any(|n| n.contains("OUTRO acervo")),
        "{:?}",
        report.notes
    );
}

// ---------------------------------------------------------------- R2-02 --

/// R2-02. A promotion published the 9 on the Drive and not on the pen
/// drive, and the pen drive still refuses writes at the next exchange:
/// nothing is adopted and no old name gets its stub until every configured
/// mirror holds its 9; then both are finished.
#[test]
fn f2b_r2_02_a_resume_finishes_every_9_before_anything_else() {
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
    let on = std::rc::Rc::new(std::cell::Cell::new(true));
    failing_renames(sharedfolder::nine_path(&pen), on.clone());
    assert!(try_sync(&mut a).is_err(), "the promotion stops");
    assert!(
        try_sync(&mut a).is_err(),
        "and so does the resume, the pen drive still refusing"
    );
    assert_eq!(format_of(&drive), 8, "no stub yet on the Drive");
    assert_eq!(format_of(&pen), 8);
    assert!(
        !matches!(origin(&a).as_str(), "stamped" | "adopted"),
        "nothing adopted"
    );
    on.set(false);
    hooks::BEFORE_RENAME.with(|h| *h.borrow_mut() = None);
    sync(&mut a);
    for path in [&drive, &pen] {
        assert!(is_stub(path), "{}", path.display());
        assert_eq!(format_of(&sharedfolder::nine_path(path)), 9);
    }
}

// ---------------------------------------------------------------- R2-03 --

/// R2-03. A holder of the floor's lock that takes long (and whose lock
/// file looks old) is never overtaken: a second holder waits for it.
#[test]
fn f2b_r2_03_a_living_holder_of_the_floor_lock_is_never_overtaken() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(crate::formatfloor::FILE);
    let lock = PathBuf::from(format!("{}.trava", file.display()));
    let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));
    let first = {
        let (file, lock, order) = (file.clone(), lock.clone(), order.clone());
        std::thread::spawn(move || {
            crate::formatfloor::with_lock(&file, || {
                // The lock file looks five minutes old.
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&lock) {
                    let _ = f.set_modified(
                        std::time::SystemTime::now() - std::time::Duration::from_secs(300),
                    );
                }
                order.lock().unwrap().push("primeiro-começa");
                std::thread::sleep(std::time::Duration::from_millis(1500));
                order.lock().unwrap().push("primeiro-termina");
                Ok(())
            })
            .unwrap()
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(400));
    crate::formatfloor::with_lock(&file, || {
        order.lock().unwrap().push("segundo");
        Ok(())
    })
    .unwrap();
    first.join().unwrap();
    assert_eq!(
        *order.lock().unwrap(),
        ["primeiro-começa", "primeiro-termina", "segundo"]
    );
}

// ---------------------------------------------------------------- R2-04 --

/// R2-04. A device that already took a 9 sees, later, only an older 9 of
/// another epoch (another device's cut): it is refused like any older copy.
#[test]
fn f2b_r2_04_an_older_9_of_another_epoch_is_refused_after_the_first() {
    let mut t = promoted();
    t.a.save_note(
        t.a.list_documents("").unwrap()[0].id,
        "Nota que sobe a revisão",
    )
    .unwrap();
    sync(&mut t.a);
    sync(&mut t.b);
    let current = copy_at(&nine(&t));
    let mut payload = current.payload;
    payload.snapshot.ident_epoch = "uma-outra-epoca-de-outro-corte".into();
    sharedfolder::write_nine(
        &nine(&t),
        KEY,
        &sharedfolder::WriteRequest {
            lineage: &current.header.lineage,
            revision: current.header.revision - 1,
            writer: "outro-promotor",
            writer_name: "Outro",
            schema_version: current.header.schema_version,
        },
        &payload,
        &current.blobs,
    )
    .unwrap();
    let error = try_sync(&mut t.b).unwrap_err().to_string();
    assert!(error.contains("ainda não recebeu"), "{error}");
}

// ---------------------------------------------------------------- R2-06 --

/// R2-06. An older Ponte saved the holidays after the cut: the change is
/// not applied, and its card shows the value, to be redone.
#[test]
fn f2b_r2_06_a_card_of_an_old_setting_shows_its_value() {
    let mut t = promoted();
    old_writer_over(
        &t.drive,
        &nine(&t),
        vec![entry(
            "antigo-feriados",
            IntranetMutation::SaveSetting {
                key: "holidays".into(),
                value: "uf RJ\ndia 2026-12-24 Véspera de Natal\n".into(),
                seed: false,
            },
            vec![],
        )],
    );
    sync(&mut t.a);
    let card =
        t.a.list_sync_conflicts()
            .unwrap()
            .into_iter()
            .find(|c| c.mutation_uid == "antigo-feriados")
            .expect("a card");
    assert!(
        card.reason.contains("dia 2026-12-24 Véspera de Natal"),
        "{}",
        card.reason
    );
}

// ---------------------------------------------------------------- R2-07 --

/// R2-07. The log of inserted rows fails after an old change was applied:
/// the exchange stops before publishing; no card, nothing marked; the next
/// exchange takes the change in once.
#[test]
fn f2b_r2_07_a_failing_insert_log_stops_the_exchange() {
    let mut t = promoted();
    old_writer_over(
        &t.drive,
        &nine(&t),
        vec![entry(
            "antigo-tarefa",
            standalone_task("Tarefa do aparelho antigo"),
            vec![crate::idmap::CreatedId::new(
                crate::idmap::IdKind::Task,
                "",
                93,
            )],
        )],
    );
    hooks::FAIL_LOG.with(|c| c.set(true));
    assert!(try_sync(&mut t.a).is_err(), "the exchange stops");
    hooks::FAIL_LOG.with(|c| c.set(false));
    assert!(
        !t.a.list_sync_conflicts()
            .unwrap()
            .iter()
            .any(|c| c.mutation_uid == "antigo-tarefa"),
        "never a card for a change that was applied"
    );
    assert_eq!(format_of(&t.drive), 8, "the old copy is still there");
    sync(&mut t.a);
    let made =
        t.a.list_tasks()
            .unwrap()
            .into_iter()
            .filter(|x| x.title == "Tarefa do aparelho antigo")
            .count();
    assert_eq!(made, 1);
}

// ---------------------------------------------------------------- R2-08 --

/// R2-08. A change made online is applied on the main computer, its answer
/// does not check, and applying it here would fail (its document is in this
/// copy's Lixeira): the request stays queued under its identity, and the
/// next synchronization gets the receipt — one effect on the main computer.
#[test]
fn f2b_r2_08_an_unchecked_online_answer_keeps_the_request() {
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
    let doc = host
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let mut server = crate::intranet::HostServer::start(db.clone(), "127.0.0.1:0").unwrap();
    let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
    client
        .save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "SERVIDOR-A".into(),
            host_url: format!("http://{}", server.local_addr()),
            cabinet_token: token,
            ..IntranetConfig::default()
        })
        .unwrap();
    client.sync_from_host(false).unwrap();
    // Here only: the document goes to this copy's Lixeira.
    client.suppress_remote = true;
    client
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: doc,
        })
        .unwrap();
    client.suppress_remote = false;
    *hooks::CORRUPT_MUTATED.lock().unwrap() = Some(db.clone());
    let notes = |s: &Store| -> i64 {
        s.conn
            .query_row(
                "SELECT count(*) FROM notes WHERE document_id=?1",
                [doc],
                |r| r.get(0),
            )
            .unwrap()
    };
    let _ = client.save_note(doc, "Nota feita on-line");
    *hooks::CORRUPT_MUTATED.lock().unwrap() = None;
    let queued = client.list_offline_outbox().unwrap();
    assert_eq!(queued.len(), 1, "the request is kept");
    assert!(queued[0].attempted && !queued[0].uid.is_empty());
    let _ = client.sync_from_host(false);
    let host_now = Store::open(&db).unwrap();
    assert_eq!(notes(&host_now), 1, "one effect on the main computer");
    server.stop();
}
