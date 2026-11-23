//! F2a, review `revisao-f2a.md`: one regression test per accepted finding
//! (F2A-01 … F2A-07; F2A-08 is `f2a_parity`). They use only the product and
//! the test hooks of `idents::hooks`, so the same file was run against the
//! reviewed head (318c92f, with the same hook points) to show each fails
//! there.
#![allow(clippy::all)]

use crate::idents::{hooks, test_support};
use crate::intranet::{IntranetConfig, IntranetMode};
use crate::storage::Store;
use rusqlite::Connection;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

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

fn sync(store: &mut Store) -> anyhow::Result<crate::intranet::FolderSyncReport> {
    let cfg = store.load_intranet_config()?;
    store.sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
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

/// A connection of this build to the library at `path`, outside the store.
fn raw(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    conn.create_scalar_function(
        "ponte_client_schema",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
            | rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
        |_| Ok(22i64),
    )
    .unwrap();
    crate::idents::register_functions(&conn).unwrap();
    conn
}

fn scalar(conn: &Connection, sql: &str) -> Option<i64> {
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0))
        .unwrap()
}

fn ident(conn: &Connection, kind: &str, row: i64) -> Option<String> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT ident FROM item_ident WHERE kind=?1 AND row_id=?2",
        rusqlite::params![kind, row],
        |r| r.get(0),
    )
    .optional()
    .unwrap()
}

fn temp_triggers(store: &Store) -> Vec<String> {
    store
        .conn
        .prepare("SELECT name FROM sqlite_temp_master WHERE type='trigger'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn assert_no_log(store: &Store, step: &str) {
    let left = temp_triggers(store);
    assert!(left.is_empty(), "{step}: gatilhos TEMP ficaram: {left:?}");
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

fn notes_of(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut rows: Vec<String> = conn
        .prepare("SELECT id||'|'||document_id||'|'||hex(payload) FROM notes")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    rows.sort();
    rows
}

/// F2A-01. A draft is saved (and queued) here; between the commit of its
/// row and the capture of what it created, another connection replaces it
/// by another draft under the same number (as an adoption reusing numbers
/// does). The queue entry must never carry the identity of the newcomer:
/// it carries the identity the draft was given when it was created, or
/// none.
#[test]
fn f2a_01_a_number_reused_after_the_commit_never_lends_its_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    let mut store = Store::open(&path).unwrap();
    store
        .save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
        .unwrap();
    let receipt = store
        .import_capture(&capture("SEFAZ-PRO-2026/00501", "Um."))
        .unwrap();
    let seen: Rc<RefCell<Option<(i64, String, String)>>> = Default::default();
    let keep = seen.clone();
    let db = path.clone();
    hooks::AFTER_LOCAL_COMMIT.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            let other = raw(&db);
            let row = scalar(&other, "SELECT max(id) FROM drafts").unwrap();
            let created = ident(&other, "draft", row).unwrap();
            other
                .execute_batch(&format!(
                    "DELETE FROM drafts WHERE id={row};
                     INSERT INTO drafts(id,document_id,capture_id,payload,created_at)
                       VALUES({row},{doc},{cap},X'00','2026-11-23T10:00:00.000Z');",
                    doc = receipt.document_id,
                    cap = receipt.capture_id
                ))
                .unwrap();
            let newcomer = ident(&other, "draft", row).unwrap();
            *keep.borrow_mut() = Some((row, created, newcomer));
        }));
    });
    store
        .save_draft(
            receipt.document_id,
            receipt.capture_id,
            "resposta",
            "Minuta X",
        )
        .unwrap();
    hooks::AFTER_LOCAL_COMMIT.with(|h| h.borrow_mut().take());
    let (_, created, newcomer) = seen.borrow_mut().take().expect("o gancho rodou");
    assert_ne!(created, newcomer);
    let entry = store.list_offline_outbox().unwrap().pop().unwrap();
    let carried = serde_json::to_value(&entry).unwrap();
    let carried = serde_json::to_string(&carried["idents"]).unwrap_or_default();
    assert!(
        !carried.contains(&newcomer),
        "a fila leva a identidade da outra minuta: {carried}"
    );
    assert!(
        carried.contains(&created),
        "a fila não leva a identidade da minuta criada: {carried}"
    );
}

/// F2A-02. The current analysis of D1 is chosen and its content read: a
/// commit by another connection right after the choice that gives its
/// number to an analysis of D2 must not make D1 show D2's analysis.
#[test]
fn f2a_02_the_current_analysis_and_its_content_come_from_one_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let analyse = |store: &mut Store, receipt: &crate::model::ImportReceipt, summary: &str| {
        store
            .save_analysis(
                receipt.document_id,
                &crate::model::Analysis {
                    capture_id: receipt.capture_id,
                    summary: summary.into(),
                    reason: "r".into(),
                    model: "modelo-de-teste".into(),
                    created_at: "2026-11-10T10:00:00.000Z".into(),
                    priority: "normal".into(),
                    ..Default::default()
                },
            )
            .unwrap();
    };
    let d1 = store
        .import_capture(&capture("SEFAZ-PRO-2026/00501", "Um."))
        .unwrap();
    let d2 = store
        .import_capture(&capture("SEFAZ-PRO-2026/00502", "Dois."))
        .unwrap();
    analyse(&mut store, &d1, "Análise de D1");
    analyse(&mut store, &d2, "Análise de D2");
    let conn = raw(&path);
    let a = scalar(
        &conn,
        &format!(
            "SELECT id FROM analyses WHERE document_id={}",
            d1.document_id
        ),
    )
    .unwrap();
    let b = scalar(
        &conn,
        &format!(
            "SELECT id FROM analyses WHERE document_id={}",
            d2.document_id
        ),
    )
    .unwrap();
    let db = path.clone();
    hooks::AFTER_ANALYSIS_SELECT.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            raw(&db)
                .execute_batch(&format!(
                    "DELETE FROM analyses WHERE id={a};
                     INSERT INTO analyses(id,document_id,capture_id,payload,created_at)
                       SELECT {a},document_id,capture_id,payload,created_at FROM analyses WHERE id={b};"
                ))
                .unwrap();
        }));
    });
    let shown = store.load_document(d1.document_id);
    hooks::AFTER_ANALYSIS_SELECT.with(|h| h.borrow_mut().take());
    let shown = shown.expect("a ficha abre").analysis.map(|x| x.summary);
    assert_eq!(shown.as_deref(), Some("Análise de D1"));
}

/// F2A-03. No log of inserted rows (no TEMP trigger) stays open after any
/// public write that opens one: the atomic record of the phone (new, the
/// same request again with and without another operation id, with files),
/// the phone's attachment (and its refusal), every change variant queued
/// in shared-folder mode, a duplicate import, an exchange.
#[test]
fn f2a_03_no_insert_log_survives_a_public_write() {
    use crate::record_import::{NewAttachment, NewRecord};
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config("Celular", vec![drive.display().to_string()]))
        .unwrap();
    assert_no_log(&store, "abertura");
    let fields = crate::manual::ManualFields {
        kind: "whatsapp".into(),
        title: "Conversa com Servidor A — 04/11/2026".into(),
        origin: "Servidor A".into(),
        document_date: "2026-11-04".into(),
        text: "mensagem".into(),
    };
    let record = NewRecord {
        request_id: crate::record_import::whatsapp_request_id(&fields.origin, &fields.text),
        fields,
        metadata: Default::default(),
    };
    let png = |seed: u8| {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', seed, 1, 2, 3]);
        bytes
    };
    let files = [NewAttachment {
        name: "foto.png".into(),
        bytes: png(1),
        description: String::new(),
    }];
    let created = store
        .create_record_for_operation(&record, &files, Some("operacao-1"))
        .unwrap();
    assert_no_log(&store, "registro novo");
    // The same request again: its receipt exists (the import returns before
    // queueing anything).
    store
        .create_record_for_operation(&record, &files, None)
        .unwrap();
    assert_no_log(&store, "mesmo pedido, sem operação");
    let _ = store.create_record_for_operation(&record, &files, Some("operacao-2"));
    assert_no_log(&store, "mesmo pedido, outra operação");
    let crate::record_import::Created::New { document_id, .. } = created else {
        panic!("{created:?}")
    };
    let number = store
        .load_document(document_id)
        .unwrap()
        .capture
        .document_number;
    let request =
        |name: &'static str, number: Option<&'static str>| crate::attachments::AttachRequest {
            document_id,
            name,
            description: "",
            data: b"conteudo do anexo do celular",
            operation_id: None,
            expected_number: number,
        };
    let leaked: &'static str = Box::leak(number.into_boxed_str());
    store
        .attach_to_document(&request("a.txt", Some(leaked)))
        .unwrap();
    assert_no_log(&store, "anexo do celular");
    let _ = store.attach_to_document(&request("b.txt", Some("OUTRO-NUMERO")));
    assert_no_log(&store, "anexo do celular recusado");
    crate::f2a_parity::every_variant(&mut store, "a");
    assert_no_log(&store, "todas as variantes");
    let mut again = crate::model::sample_capture();
    again.document_number = "SEFAZ-PRO-2026/00101".into();
    let _ = store.import_capture(&again);
    assert_no_log(&store, "importação repetida");
    sync(&mut store).unwrap();
    assert_no_log(&store, "troca pela pasta");
}

/// F2A-04 (exchange). The identities of a replayed change cannot be
/// recorded in the temporary library: the exchange stops before writing or
/// adopting anything, the change is not turned into a conflict card, and
/// the next exchange applies it once.
#[test]
fn f2a_04_a_failure_of_the_identities_publishes_nothing_and_cards_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", files.clone()))
        .unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    sync(&mut a).unwrap();
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    sync(&mut b).unwrap();
    let doc = b.list_documents("").unwrap()[0].id;
    b.save_note(doc, "Nota de B").unwrap();
    let before = std::fs::read(&drive).unwrap();
    hooks::FAIL_REKEY.with(|h| h.set(true));
    let failed = sync(&mut b);
    hooks::FAIL_REKEY.with(|h| h.set(false));
    assert!(
        failed.is_err(),
        "a troca seguiu: {:?}",
        failed.map(|r| r.conflicts)
    );
    assert_eq!(
        std::fs::read(&drive).unwrap(),
        before,
        "a pasta foi regravada"
    );
    assert!(b.list_sync_conflicts().unwrap().is_empty(), "virou cartão");
    assert_eq!(b.list_offline_outbox().unwrap().len(), 1, "a fila mudou");
    sync(&mut b).unwrap();
    sync(&mut a).unwrap();
    assert_eq!(a.load_document(doc).unwrap().note, "Nota de B");
    assert!(a.list_sync_conflicts().unwrap().is_empty());
}

/// F2A-04 (origin). The identities of a change applied here cannot be read:
/// the change is still queued (without identities), never lost.
#[test]
fn f2a_04_a_change_whose_identities_cannot_be_read_is_still_queued() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
        .unwrap();
    let doc = store
        .import_capture(&capture("SEFAZ-PRO-2026/00501", "Um."))
        .unwrap()
        .document_id;
    let queued = store.list_offline_outbox().unwrap().len();
    hooks::FAIL_CAPTURE.with(|h| h.set(true));
    let saved = store.save_note(doc, "Nota");
    hooks::FAIL_CAPTURE.with(|h| h.set(false));
    saved.expect("a nota foi gravada e precisa ir para a fila");
    assert_eq!(store.list_offline_outbox().unwrap().len(), queued + 1);
    assert_eq!(store.load_document(doc).unwrap().note, "Nota");
}

/// F2A-05. The pointer orders analyses by their time in microseconds, as
/// the product reads them: after every analysis below (same version), the
/// pointer is the product's current analysis.
#[test]
fn f2a_05_the_analysis_pointer_orders_by_microseconds_like_the_product() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    let mut store = Store::open(&path).unwrap();
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
    let conn = raw(&path);
    for at in [
        "2026-11-10T10:00:00.000100Z",
        "2026-11-10T10:00:00.000000Z",
        "2026-11-10T12:00:00.000050+02:00",
        "2026-11-10 10:00:00.0000999Z",
        "2026-11-10t10:00:00.000101z",
        "2026-11-10T10:00:00.000101-00:00",
        "2026-02-30T10:00:00Z",
        "ontem",
        "2026-11-10T10:00:00",
        "2026-11-10T10:00:00.Z",
        "2026-11-10T10:00:00.000102345Z",
        "  2026-11-10T07:00:00.000103-03:00  ",
    ] {
        conn.execute(
            "INSERT INTO analyses(document_id,capture_id,payload,created_at)
               SELECT document_id,capture_id,payload,?1 FROM analyses WHERE document_id=?2 ORDER BY id LIMIT 1",
            rusqlite::params![at, doc],
        )
        .unwrap();
        let rule = test_support::current_analysis(&conn, doc).unwrap();
        let pointer = scalar(
            &conn,
            &format!("SELECT analysis_id FROM current_pointer WHERE document_id={doc}"),
        );
        assert_eq!(pointer, rule, "depois da análise de {at:?}");
    }
    let problems = test_support::invariant_problems(&conn).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
}

/// A library of schema 21 with one document and a note of text `note`.
fn library_21(path: &Path, note: &str) {
    {
        let mut store = Store::open(path).unwrap();
        let doc = store
            .import_capture(&capture("SEFAZ-PRO-2026/00501", "Um."))
            .unwrap()
            .document_id;
        store.save_note(doc, note).unwrap();
    }
    test_support::downgrade_to_21(path).unwrap();
}

/// F2A-06. Two libraries in one folder are updated in the same second, the
/// second while the copy of the first is written: each gets a copy of its
/// own (never the other's, never a shared partial file), both update.
#[test]
fn f2a_06_two_libraries_in_one_folder_migrate_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let one = dir.path().join("um.sqlite3");
    let two = dir.path().join("dois.sqlite3");
    library_21(&one, "nota do acervo um");
    library_21(&two, "nota do acervo do");
    let (notes_one, notes_two) = (notes_of(&one), notes_of(&two));
    hooks::MIGRATION_STAMP.with(|h| *h.borrow_mut() = Some("20261123-120000".into()));
    let second: Rc<RefCell<Option<anyhow::Result<()>>>> = Default::default();
    let keep = second.clone();
    let other = two.clone();
    hooks::MIGRATION_COPY_WRITTEN.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            *keep.borrow_mut() = Some(Store::open(&other).map(drop));
        }));
    });
    let first = Store::open(&one).map(drop);
    hooks::MIGRATION_COPY_WRITTEN.with(|h| h.borrow_mut().take());
    hooks::MIGRATION_STAMP.with(|h| h.borrow_mut().take());
    let second = second
        .borrow_mut()
        .take()
        .expect("o segundo abriu durante a cópia");
    first.expect("o primeiro acervo atualiza");
    second.expect("o segundo acervo atualiza");
    assert_eq!(test_support::user_version(&one).unwrap(), 22);
    assert_eq!(test_support::user_version(&two).unwrap(), 22);
    let names = copies(&one);
    let finals: Vec<&String> = names.iter().filter(|n| n.ends_with(".sqlite3")).collect();
    assert_eq!(finals.len(), 2, "{names:?}");
    assert_eq!(names.len(), 2, "sobrou arquivo parcial: {names:?}");
    let backups = dir.path().join("backups");
    let mut contents: Vec<Vec<String>> =
        finals.iter().map(|n| notes_of(&backups.join(n))).collect();
    contents.sort();
    let mut expected = vec![notes_one, notes_two];
    expected.sort();
    assert_eq!(contents, expected, "uma cópia não é o acervo de que veio");
}

/// F2A-07 (measure). A measure of free space that fails refuses the
/// update (never "room enough"); too little room refuses it too; nothing is
/// copied or changed either way.
#[test]
fn f2a_07_a_free_space_that_cannot_be_measured_refuses_the_update() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    library_21(&path, "nota");
    hooks::FREE_SPACE
        .with(|h| *h.borrow_mut() = Some(Err(std::io::Error::other("volume indisponível"))));
    // (Since the second review the library opens as it was, with a notice
    // that tells the measure failed or the room is short, apart.)
    let notice = Store::open(&path)
        .ok()
        .and_then(|mut s| s.take_upgrade_notice());
    hooks::FREE_SPACE.with(|h| h.borrow_mut().take());
    let notice = notice.expect("a atualização seguiu sem medir o espaço");
    assert!(
        notice.contains("não foi possível medir o espaço livre"),
        "{notice}"
    );
    hooks::FREE_SPACE.with(|h| *h.borrow_mut() = Some(Ok(1)));
    let notice = Store::open(&path)
        .ok()
        .and_then(|mut s| s.take_upgrade_notice());
    hooks::FREE_SPACE.with(|h| h.borrow_mut().take());
    assert!(notice.expect("faltou recusar").contains("falta espaço"));
    assert_eq!(test_support::user_version(&path).unwrap(), 21);
    assert!(copies(&path).is_empty(), "{:?}", copies(&path));
}

/// F2A-07 (copies). An update that fails after its copy, again and again,
/// keeps ONE copy (the next attempt proves the earlier copy is this very
/// state and uses it) and no partial file.
#[test]
fn f2a_07_updates_that_keep_failing_do_not_pile_up_copies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    library_21(&path, "nota");
    for attempt in 0..3 {
        hooks::BEFORE_MIGRATION_COMMIT.with(|h| {
            *h.borrow_mut() = Some(Box::new(|_: &Path| anyhow::bail!("falha injetada (teste)")))
        });
        let stopped = Store::open(&path)
            .ok()
            .and_then(|mut s| s.take_upgrade_notice());
        assert!(stopped.is_some(), "tentativa {attempt}");
        hooks::BEFORE_MIGRATION_COMMIT.with(|h| h.borrow_mut().take());
    }
    assert_eq!(copies(&path).len(), 1, "{:?}", copies(&path));
    drop(Store::open(&path).unwrap());
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
    assert_eq!(copies(&path).len(), 1, "{:?}", copies(&path));
}
