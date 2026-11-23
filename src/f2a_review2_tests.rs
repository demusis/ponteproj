//! F2a, second review (`revisao-f2a-rodada-2.md`): the regression tests of
//! F2A-05, F2A-11, F2A-07, F2A-08 and the MINOR item. Like the first round's,
//! they use only the product, `idents::test_support` and the hooks of
//! `idents::hooks`, so they also run against the reviewed head (b3f9bf5).
#![allow(clippy::all)]

use crate::idents::{hooks, test_support};
use crate::intranet::{IntranetConfig, IntranetMode};
use crate::storage::Store;
use std::path::{Path, PathBuf};

const KEY: &str = "senha-de-teste-123";

fn folder_config(member: &str, files: Vec<String>) -> IntranetConfig {
    IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: files,
        folder_key: KEY.into(),
        ..IntranetConfig::default()
    }
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

fn copies(db: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(db.parent().unwrap().join("backups"))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A library of schema 21 at `path`, in shared-folder mode, with one
/// document and a note still queued.
fn library_21_with_queue(path: &Path) {
    {
        let mut store = Store::open(path).unwrap();
        let drive = path
            .parent()
            .unwrap()
            .join("Drive")
            .join("ponte-sync.ponte");
        store
            .save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
            .unwrap();
        let doc = store
            .import_capture(&capture("SEFAZ-PRO-2026/00501", "Um."))
            .unwrap()
            .document_id;
        store.save_note(doc, "nota na fila").unwrap();
    }
    test_support::downgrade_to_21(path).unwrap();
}

/// F2A-05. The pointer and the product read order analyses with ONE
/// converter: also for the times the Ponte accepts that SQLite's own date
/// functions do not read (an offset beyond ±14:00, a leap second). The key
/// the SQL side computes for each time is compared directly with chrono's
/// (computed here, independently), and the pointer with the product read
/// after every analysis.
#[test]
fn f2a_05_the_time_key_is_chronos_for_far_offsets_and_leap_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
    let receipt = store
        .import_capture(&capture("SEFAZ-PRO-2026/00501", "Um."))
        .unwrap();
    let doc = receipt.document_id;
    store
        .save_analysis(
            doc,
            &crate::model::Analysis {
                capture_id: receipt.capture_id,
                summary: "base".into(),
                reason: "r".into(),
                model: "modelo-de-teste".into(),
                created_at: "2020-01-01T00:00:00.000Z".into(),
                priority: "normal".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let expression = crate::idents::analysis_time("?1");
    for at in [
        "2026-11-09T20:30:00Z",
        "2026-11-10T12:00:00+15:00",
        "2026-11-09T20:59:59.999999-00:00",
        "2026-11-10T10:00:00-23:59",
        "2016-12-31T23:59:60Z",
        "2016-12-31T23:59:59.999999Z",
        "2016-12-31T23:59:60.5+00:00",
        "2026-11-10T09:00:00.123+01:00",
        "2026-11-09T21:00:00.000001Z",
    ] {
        let expected = chrono::DateTime::parse_from_rfc3339(at)
            .ok()
            .map(|t| t.timestamp_micros());
        let sql: Option<i64> = store
            .conn
            .query_row(&format!("SELECT {expression}"), [at], |r| r.get(0))
            .unwrap();
        assert_eq!(sql, expected, "chave de {at:?}");
        store
            .conn
            .execute(
                "INSERT INTO analyses(document_id,capture_id,payload,created_at)
                   SELECT document_id,capture_id,payload,?1 FROM analyses WHERE document_id=?2 ORDER BY id LIMIT 1",
                rusqlite::params![at, doc],
            )
            .unwrap();
        let rule = test_support::current_analysis(&store.conn, doc).unwrap();
        let pointer: Option<i64> = store
            .conn
            .query_row(
                "SELECT analysis_id FROM current_pointer WHERE document_id=?1",
                [doc],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pointer, rule, "depois da análise de {at:?}");
    }
}

/// F2A-11 (order). An update stopped after its copy; the next opening has
/// room for the update's own writes but not for another copy: the copy of
/// the first attempt, proved to be this very state, is used, and the
/// library moves to 22 with one copy.
#[test]
fn f2a_11_an_earlier_copy_is_looked_for_before_the_room_is_counted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    library_21_with_queue(&path);
    hooks::BEFORE_MIGRATION_COMMIT.with(|h| {
        *h.borrow_mut() = Some(Box::new(|_: &Path| anyhow::bail!("falha injetada (teste)")))
    });
    let _ = Store::open(&path);
    hooks::BEFORE_MIGRATION_COMMIT.with(|h| h.borrow_mut().take());
    assert_eq!(test_support::user_version(&path).unwrap(), 21);
    assert_eq!(copies(&path).len(), 1, "{:?}", copies(&path));
    let size = crate::auto_backup::needed_space(&path).0;
    hooks::FREE_SPACE.with(|h| *h.borrow_mut() = Some(Ok(size + size / 2)));
    let opened = Store::open(&path);
    hooks::FREE_SPACE.with(|h| h.borrow_mut().take());
    opened.expect("a abertura segue");
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
    assert_eq!(copies(&path).len(), 1, "{:?}", copies(&path));
}

/// `path` as a network path (`\\localhost\C$\…`) when this machine serves
/// its drive that way.
fn as_network_path(path: &Path) -> Option<PathBuf> {
    let text = std::path::absolute(path).ok()?.display().to_string();
    let (drive, rest) = text.split_once(":\\")?;
    let unc = PathBuf::from(format!(r"\\localhost\{drive}$\{rest}"));
    unc.exists().then_some(unc)
}

/// F2A-11 (path). A library opened by a network path (UNC) migrates: the
/// copy beside it is checked by the same kind of path the library was
/// opened by.
#[test]
fn f2a_11_a_library_opened_by_a_network_path_migrates() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("acervo.sqlite3");
    library_21_with_queue(&local);
    let Some(unc) = as_network_path(&local) else {
        eprintln!(
            "sem caminho de rede para {}: teste não aplicável",
            local.display()
        );
        return;
    };
    let store = Store::open(&unc).expect("abre pelo caminho de rede");
    assert_eq!(test_support::user_version(&local).unwrap(), 22);
    assert_eq!(copies(&local).len(), 1, "{:?}", copies(&local));
    drop(store);
}

/// F2A-07. The free space cannot be measured, at every opening: the library
/// still opens, as it was (schema 21), with a notice that says why, its
/// queue intact and usable (a change saved meanwhile is queued too); the
/// first opening that can measure moves it to 22 with the whole queue.
#[test]
fn f2a_07_a_persistent_measure_failure_opens_the_library_at_21() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    library_21_with_queue(&path);
    let mut initial = None;
    for attempt in 0..3 {
        hooks::FREE_SPACE
            .with(|h| *h.borrow_mut() = Some(Err(std::io::Error::other("volume indisponível"))));
        let opened = Store::open(&path);
        hooks::FREE_SPACE.with(|h| h.borrow_mut().take());
        let mut store = opened.expect("o acervo abre mesmo sem medir o espaço");
        let notice = store.take_upgrade_notice().expect("um aviso diz por quê");
        assert!(
            notice.contains("não foi possível medir o espaço livre"),
            "{notice}"
        );
        assert_eq!(test_support::user_version(&path).unwrap(), 21);
        let queued = store.list_offline_outbox().unwrap().len();
        let initial = *initial.get_or_insert(queued);
        assert!(initial >= 1);
        assert_eq!(queued, initial + attempt, "a fila mudou");
        // A new record each time (never merged with a queued change).
        store
            .import_capture(&capture(
                &format!("SEFAZ-PRO-2026/0060{attempt}"),
                "Feito com o acervo na versão anterior.",
            ))
            .unwrap();
        assert_eq!(store.list_offline_outbox().unwrap().len(), queued + 1);
    }
    assert!(copies(&path).is_empty(), "{:?}", copies(&path));
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.take_upgrade_notice(), None);
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
    assert_eq!(
        store.list_offline_outbox().unwrap().len(),
        initial.unwrap() + 3
    );
    let problems = test_support::invariant_problems(&store.conn).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
}

/// MINOR (F2A-09). `--import-plan --dry-run` reads a library of schema 21 as
/// it is: the simulation runs, and the library is still 21, with no copy.
#[test]
fn f2a_09_a_dry_run_reads_a_21_library_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    library_21_with_queue(&path);
    let plan = dir.path().join("plano.json");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("import-plan-exemplo.json"),
        &plan,
    )
    .unwrap();
    let options = crate::plan_import::CliOptions {
        file: plan.display().to_string(),
        dry_run: true,
        output: None,
    };
    let mut out = Vec::new();
    crate::plan_import::execute(&options, &path, &mut out).expect("a simulação roda");
    assert!(!out.is_empty());
    assert_eq!(test_support::user_version(&path).unwrap(), 21);
    assert!(copies(&path).is_empty(), "{:?}", copies(&path));
}

/// F2A-08. The invariants are checked after every step of the F1 tests
/// (`test_support::check_f1_step`, on their threads): a change queued on
/// such a thread is checked, and a broken invariant is caught.
#[test]
fn f2a_08_the_f1_steps_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    let run = move |break_it: bool| {
        let path = path.clone();
        std::thread::Builder::new()
            .name("f1_inventory::sonda".into())
            .spawn(move || {
                let mut store = Store::open(&path).unwrap();
                let drive = path
                    .parent()
                    .unwrap()
                    .join("Drive")
                    .join("ponte-sync.ponte");
                store
                    .save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
                    .unwrap();
                let doc = store
                    .import_capture(&capture("SEFAZ-PRO-2026/00501", "Um."))
                    .unwrap()
                    .document_id;
                if break_it {
                    store
                        .conn
                        .execute("DELETE FROM item_ident WHERE kind='document'", [])
                        .unwrap();
                }
                store.save_note(doc, "nota").unwrap();
            })
            .unwrap()
            .join()
    };
    let before = test_support::F1_STEPS_CHECKED.load(std::sync::atomic::Ordering::Relaxed);
    run(false).expect("passos sem problema");
    assert!(
        test_support::F1_STEPS_CHECKED.load(std::sync::atomic::Ordering::Relaxed) > before,
        "nenhum passo conferido"
    );
    assert!(run(true).is_err(), "o problema passou despercebido");
}
