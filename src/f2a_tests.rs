//! F2a (Frente F): acceptance tests of the library schema 22 — identities
//! of the items, the log of the rows a change inserts, the current-note and
//! current-analysis pointers, and the 21→22 migration with its fence.
//! Named after `desenho-f2-v2.md` §4 (A1–A11). Nothing reads the identities
//! yet: every behaviour must stay as it was (A11).
#![allow(clippy::all)]

use crate::idents::{test_support, Rekeyed, SlotIdent};
use crate::intranet::{IntranetConfig, IntranetMode, OutboxEntry};
use crate::storage::Store;
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const KEY: &str = "senha-de-teste-123";
const GUARD_MESSAGE: &str = "Use a versão atual do Ponte para salvar alterações.";

// ---------------------------------------------------------------- helpers --

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

/// Two computers sharing one folder, in sync on a library with one SIGADOC
/// document (as `f1_inventory::pair`).
struct Pair {
    dir: tempfile::TempDir,
    drive: PathBuf,
    a: Store,
    b: Store,
}

fn pair() -> Pair {
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
    Pair { dir, drive, a, b }
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

fn number(n: u32) -> String {
    format!("SEFAZ-PRO-2026/{:05}", 500 + n)
}

fn ident(store: &Store, kind: &str, row: i64) -> Option<String> {
    crate::idents::ident_of(&store.conn, kind, row).unwrap()
}

fn all_idents(conn: &Connection) -> BTreeMap<(String, i64), String> {
    conn.prepare("SELECT kind,row_id,ident FROM item_ident")
        .unwrap()
        .query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn assert_invariants(store: &Store, who: &str) {
    let problems = test_support::invariant_problems(&store.conn).unwrap();
    assert!(problems.is_empty(), "{who}: {problems:?}");
}

fn outbox(store: &Store) -> Vec<OutboxEntry> {
    store.list_offline_outbox().unwrap()
}

fn one(conn: &Connection, sql: &str, value: i64) -> Option<i64> {
    use rusqlite::OptionalExtension;
    conn.query_row(sql, [value], |r| r.get::<_, Option<i64>>(0))
        .optional()
        .unwrap()
        .flatten()
}

/// A raw connection as a Ponte of schema `schema` has it (only the guard
/// function, nothing else).
fn raw_as(path: &Path, schema: i64) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    conn.create_scalar_function(
        "ponte_client_schema",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
            | rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
        move |_| Ok(schema),
    )
    .unwrap();
    conn
}

fn schema_rows(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    conn.prepare(r"SELECT type,name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite\_%' ESCAPE '\' ORDER BY type,name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn tables(conn: &Connection) -> Vec<String> {
    conn.prepare(r"SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite\_%' ESCAPE '\' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn counts(conn: &Connection) -> BTreeMap<String, i64> {
    tables(conn)
        .into_iter()
        .map(|t| {
            let n = conn
                .query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))
                .unwrap();
            (t, n)
        })
        .collect()
}

/// Every row of `table`, each as text, sorted.
fn dump(conn: &Connection, table: &str) -> Vec<String> {
    let mut statement = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
    let columns = statement.column_count();
    let mut rows: Vec<String> = statement
        .query_map([], |r| {
            let mut text = Vec::new();
            for i in 0..columns {
                text.push(format!("{:?}", r.get::<_, rusqlite::types::Value>(i)?));
            }
            Ok(text.join("|"))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    rows.sort();
    rows
}

/// The copies the 21→22 step made beside the library at `db`.
fn migration_copies(db: &Path) -> Vec<PathBuf> {
    let folder = db.parent().unwrap().join("backups");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    let name = p.file_name().unwrap().to_string_lossy().into_owned();
                    name.starts_with("antes-do-acervo-22-") && name.ends_with(".sqlite3")
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// A library (Local mode) with documents, versions, notes, an analysis, a
/// task, an attachment and a document in the Lixeira; its path.
fn library_with_content(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name).join("acervo.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let first = store.import_capture(&capture(&number(1), "Um.")).unwrap();
    store
        .import_capture(&capture(&number(1), "Um, versão nova."))
        .unwrap();
    let second = store.import_capture(&capture(&number(2), "Dois.")).unwrap();
    store.save_note(first.document_id, "nota 1").unwrap();
    store.save_note(first.document_id, "nota 2").unwrap();
    store
        .save_analysis(
            first.document_id,
            &crate::model::Analysis {
                capture_id: first.capture_id,
                summary: "análise".into(),
                reason: "r".into(),
                model: "modelo-de-teste".into(),
                created_at: "2026-11-10T10:00:00.000Z".into(),
                priority: "normal".into(),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .add_attachment(
            crate::attachments::Owner::Document(first.document_id),
            "nota.txt",
            "",
            b"conteudo do anexo",
        )
        .unwrap();
    store
        .save_note(second.document_id, "nota do segundo")
        .unwrap();
    store
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: second.document_id,
        })
        .unwrap();
    path
}

/// Replays `entries` (queued in `source`) into `target` as the exchange
/// replays a queue: numbers translated change by change, each change
/// applied with the identities it carries.
fn replay(source: &Store, target: &mut Store, entries: &[OutboxEntry]) -> Vec<Rekeyed> {
    let mut ids = crate::idmap::load(&source.conn).unwrap();
    let mut out = Vec::new();
    for entry in entries {
        let mutation = ids
            .remap(&entry.mutation, source, Some(&*target))
            .unwrap_or_else(|e| panic!("{}: {e}", entry.label));
        let (reply, rekeyed) = target
            .execute_carrying(&entry.actor, mutation.clone(), &entry.idents)
            .unwrap_or_else(|e| panic!("{}: {e}", entry.label));
        let created = crate::idmap::created_ids(&*target, &mutation, &reply).unwrap();
        ids.learn(&entry.created, Some(&created));
        out.push(rekeyed);
    }
    out
}

fn kinds(idents: &[SlotIdent]) -> BTreeSet<(String, String)> {
    idents
        .iter()
        .map(|s| (s.kind.clone(), s.slot.clone()))
        .collect()
}

#[test]
fn f2a_the_21_helper_gives_the_schema_of_ponte_0_51() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    drop(Store::open(&path).unwrap());
    test_support::downgrade_to_21(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let here: BTreeSet<(String, String, Option<String>)> = schema_rows(&conn).into_iter().collect();
    let real: BTreeSet<(String, String, Option<String>)> = test_support::schema_21()
        .into_iter()
        .map(|(kind, name, _, sql)| (kind, name, sql))
        .collect();
    assert_eq!(here, real);
    assert_eq!(test_support::user_version(&path).unwrap(), 21);
}

// --------------------------------------------------------------------- A --

/// A1. An import of a new version of a known document finds the document,
/// its triage task and its atendimento: only the rows it inserted get the
/// identities the change carries, here and in a replay. The log of
/// inserted rows is per connection (a TEMP trigger on a main table).
#[test]
fn f2a_b1_only_inserted_rows_get_carried_idents() {
    let mut p = pair();
    let doc_a = p.a.list_documents("").unwrap()[0].id;
    let triage = |s: &Store, doc: i64| {
        one(
            &s.conn,
            "SELECT min(id) FROM tasks WHERE document_id=?1 AND action='triar'",
            doc,
        )
        .expect("tarefa de triagem")
    };
    let attendance = |s: &Store, doc: i64| {
        one(
            &s.conn,
            "SELECT min(id) FROM saiop_attendances WHERE document_id=?1",
            doc,
        )
        .expect("atendimento da importação")
    };
    // Offline (shared-folder mode, not synced): a new version of D.
    let mut v2 = crate::model::sample_capture();
    v2.text = "Texto novo do mesmo documento.".into();
    v2.captured_at = "2026-11-20T10:00:00Z".into();
    let receipt = p.a.import_capture(&v2).unwrap();
    assert_eq!(receipt.document_id, doc_a);
    let entry = outbox(&p.a).pop().unwrap();
    let carried = kinds(&entry.idents);
    assert!(
        carried.contains(&("capture".into(), "".into())),
        "{carried:?}"
    );
    for (kind, _) in &carried {
        assert!(
            kind == "capture" || kind == "file",
            "achado (não inserido) recebeu identidade: {carried:?}"
        );
    }
    let capture_ident = entry
        .idents
        .iter()
        .find(|s| s.kind == "capture")
        .unwrap()
        .ident
        .clone();
    assert_eq!(
        ident(&p.a, "capture", receipt.capture_id).as_deref(),
        Some(capture_ident.as_str())
    );

    // Replay into B: D, T and S keep their identities; the new version
    // takes the carried one.
    let doc_b = p.b.list_documents("").unwrap()[0].id;
    let before = (
        ident(&p.b, "document", doc_b).unwrap(),
        ident(&p.b, "task", triage(&p.b, doc_b)).unwrap(),
        ident(&p.b, "saiop", attendance(&p.b, doc_b)).unwrap(),
    );
    let rekeyed = replay(&p.a, &mut p.b, std::slice::from_ref(&entry));
    assert_eq!(rekeyed[0].skipped, 0);
    let after = (
        ident(&p.b, "document", doc_b).unwrap(),
        ident(&p.b, "task", triage(&p.b, doc_b)).unwrap(),
        ident(&p.b, "saiop", attendance(&p.b, doc_b)).unwrap(),
    );
    assert_eq!(before, after);
    let new_capture = one(
        &p.b.conn,
        "SELECT max(id) FROM captures WHERE document_id=?1",
        doc_b,
    )
    .unwrap();
    assert_eq!(ident(&p.b, "capture", new_capture), Some(capture_ident));
    assert_invariants(&p.a, "A");
    assert_invariants(&p.b, "B");

    // The log sees this connection's inserts only.
    let other_path = p.dir.path().join("a.sqlite3");
    crate::idents::start_log(&p.a.conn).unwrap();
    {
        let mut other = Store::open(&other_path).unwrap();
        other
            .import_capture(&capture(&number(9), "Inserido por outra conexão."))
            .unwrap();
    }
    assert!(crate::idents::logged(&p.a.conn).unwrap().is_empty());
    p.a.save_note(doc_a, "nota desta conexão").unwrap();
    let logged = crate::idents::logged(&p.a.conn).unwrap();
    crate::idents::stop_log(&p.a.conn);
    assert!(logged.iter().any(|(kind, _)| kind == "note"), "{logged:?}");
}

/// A2. X, queued in A as document 7, replayed into B where document 7 is
/// Y: X gets another row with X's identity, and Y keeps its own.
#[test]
fn f2a_b1_a_reused_row_id_never_receives_another_creation_s_ident() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
        .unwrap();
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    for n in 1..=6 {
        a.import_capture(&capture(&number(n), "de A")).unwrap();
        b.import_capture(&capture(&number(10 + n), "de B")).unwrap();
    }
    let x = a.import_capture(&capture(&number(7), "X, de A")).unwrap();
    let y = b.import_capture(&capture(&number(17), "Y, de B")).unwrap();
    assert_eq!((x.document_id, y.document_id), (7, 7));
    let entry = outbox(&a).pop().unwrap();
    let x_ident = entry
        .idents
        .iter()
        .find(|s| s.kind == "document" && s.slot.is_empty())
        .expect("a criação de X leva a identidade do documento")
        .ident
        .clone();
    assert_eq!(ident(&a, "document", 7).as_deref(), Some(x_ident.as_str()));
    let b_before = all_idents(&b.conn);
    let rekeyed = replay(&a, &mut b, std::slice::from_ref(&entry));
    assert_eq!(rekeyed[0].skipped, 0);
    let x_in_b = b
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number(7))
        .unwrap()
        .id;
    assert_ne!(x_in_b, 7);
    assert_eq!(ident(&b, "document", x_in_b), Some(x_ident.clone()));
    let b_after = all_idents(&b.conn);
    for (key, value) in &b_before {
        assert_eq!(b_after.get(key), Some(value), "{key:?} mudou");
    }
    assert_eq!(b_after.values().filter(|v| **v == x_ident).count(), 1);
    assert_invariants(&b, "B");
}

/// A3. Two offline notes on D are one queue entry; it carries the identity
/// of the note it keeps (the second), and a replay gives the note it
/// inserts that identity. The first note's identity is nowhere in B.
#[test]
fn f2a_b1_coalesced_notes_carry_the_surviving_note() {
    let mut p = pair();
    let doc = p.a.list_documents("").unwrap()[0].id;
    p.a.save_note(doc, "N1").unwrap();
    p.a.save_note(doc, "N2").unwrap();
    let notes: Vec<i64> =
        p.a.conn
            .prepare("SELECT id FROM notes WHERE document_id=?1 ORDER BY id")
            .unwrap()
            .query_map([doc], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
    assert_eq!(notes.len(), 2);
    let (n1, n2) = (
        ident(&p.a, "note", notes[0]).unwrap(),
        ident(&p.a, "note", notes[1]).unwrap(),
    );
    let entries = outbox(&p.a);
    assert_eq!(entries.len(), 1, "as duas notas viram uma alteração");
    let note: Vec<&SlotIdent> = entries[0]
        .idents
        .iter()
        .filter(|s| s.kind == "note")
        .collect();
    assert_eq!(note.len(), 1, "{:?}", entries[0].idents);
    assert_eq!(note[0].slot, "note");
    assert_eq!(note[0].ident, n2);
    replay(&p.a, &mut p.b, &entries);
    let doc_b = p.b.list_documents("").unwrap()[0].id;
    let inserted = one(
        &p.b.conn,
        "SELECT max(id) FROM notes WHERE document_id=?1",
        doc_b,
    )
    .unwrap();
    assert_eq!(ident(&p.b, "note", inserted), Some(n2));
    assert!(!all_idents(&p.b.conn).values().any(|v| *v == n1));
    assert_invariants(&p.b, "B");
}

/// A4. The phone's atomic creation (a record and its files in one
/// transaction, queued inside it) carries the identities of the document,
/// the version and each attachment, and a replay keeps all four.
#[test]
fn f2a_b1_phone_atomic_creation_logs_its_inserts() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let mut phone = Store::open(&dir.path().join("celular.sqlite3")).unwrap();
    phone
        .save_intranet_config(&folder_config("Celular", vec![drive.display().to_string()]))
        .unwrap();
    let fields = crate::manual::ManualFields {
        kind: "whatsapp".into(),
        title: "Conversa com Servidor A — 04/11/2026".into(),
        origin: "Servidor A".into(),
        document_date: "2026-11-04".into(),
        text: "mensagem".into(),
    };
    let record = crate::record_import::NewRecord {
        request_id: crate::record_import::whatsapp_request_id(&fields.origin, &fields.text),
        fields,
        metadata: BTreeMap::new(),
    };
    let png = |seed: u8| {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', seed, 1, 2, 3]);
        bytes
    };
    let files = [
        crate::record_import::NewAttachment {
            name: "foto1.png".into(),
            bytes: png(1),
            description: String::new(),
        },
        crate::record_import::NewAttachment {
            name: "foto2.png".into(),
            bytes: png(2),
            description: String::new(),
        },
    ];
    let created = phone
        .create_record_for_operation(&record, &files, Some("operacao-1"))
        .unwrap();
    let crate::record_import::Created::New { document_id, .. } = created else {
        panic!("{created:?}")
    };
    let entries = outbox(&phone);
    assert_eq!(entries.len(), 3, "registro e dois anexos");
    let capture_id = one(
        &phone.conn,
        "SELECT latest_capture_id FROM documents WHERE id=?1",
        document_id,
    )
    .unwrap();
    let find = |entry: &OutboxEntry, kind: &str| -> String {
        entry
            .idents
            .iter()
            .find(|s| s.kind == kind && s.slot.is_empty())
            .unwrap_or_else(|| panic!("sem {kind}: {:?}", entry.idents))
            .ident
            .clone()
    };
    let document = find(&entries[0], "document");
    let version = find(&entries[0], "capture");
    assert_eq!(
        ident(&phone, "document", document_id),
        Some(document.clone())
    );
    assert_eq!(ident(&phone, "capture", capture_id), Some(version.clone()));
    let attachments: Vec<i64> = phone
        .conn
        .prepare("SELECT id FROM attachments ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(attachments.len(), 2);
    let mut carried = vec![document.clone(), version.clone()];
    for (entry, id) in entries[1..].iter().zip(&attachments) {
        let attachment = find(entry, "attachment");
        assert_eq!(ident(&phone, "attachment", *id), Some(attachment.clone()));
        carried.push(attachment);
    }
    // Replayed into another library (a computer with its own document 1).
    let mut computer = Store::open(&dir.path().join("computador.sqlite3")).unwrap();
    computer
        .import_capture(&crate::model::sample_capture())
        .unwrap();
    let rekeyed = replay(&phone, &mut computer, &entries);
    assert!(rekeyed.iter().all(|r| r.skipped == 0));
    let held: BTreeSet<String> = all_idents(&computer.conn).into_values().collect();
    for value in &carried {
        assert!(held.contains(value), "{value} não chegou");
    }
    assert_invariants(&phone, "celular");
    assert_invariants(&computer, "computador");
}

/// A5. A 21 library with a document in the Lixeira that has notes and an
/// analysis migrates: the pointers are filled without touching `documents`
/// (whose trash guard stays installed) and follow restore and purge.
#[test]
fn f2a_b6_trashed_document_with_note_and_analysis_migrates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acervo.sqlite3");
    let (doc, notes, analysis) = {
        let mut store = Store::open(&path).unwrap();
        let receipt = store.import_capture(&capture(&number(1), "Um.")).unwrap();
        let doc = receipt.document_id;
        store.save_note(doc, "nota 1").unwrap();
        store.save_note(doc, "nota 2").unwrap();
        for (summary, at) in [
            ("depois", "2026-11-10T10:00:00.000Z"),
            ("antes", "2026-11-05T10:00:00.000Z"),
        ] {
            store
                .save_analysis(
                    doc,
                    &crate::model::Analysis {
                        capture_id: receipt.capture_id,
                        summary: summary.into(),
                        reason: "r".into(),
                        model: "modelo-de-teste".into(),
                        created_at: at.into(),
                        priority: "normal".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Document,
                id: doc,
            })
            .unwrap();
        let notes = one(
            &store.conn,
            "SELECT max(id) FROM notes WHERE document_id=?1",
            doc,
        );
        // The current analysis is the one made last (the first saved).
        let analysis = one(
            &store.conn,
            "SELECT min(id) FROM analyses WHERE document_id=?1",
            doc,
        );
        (doc, notes, analysis)
    };
    test_support::downgrade_to_21(&path).unwrap();
    let guard_before: String = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='ponte_item_active_documents'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut store = Store::open(&path).unwrap();
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
    let pointer = |s: &Store| -> Option<(Option<i64>, Option<i64>)> {
        use rusqlite::OptionalExtension;
        s.conn
            .query_row(
                "SELECT note_id,analysis_id FROM current_pointer WHERE document_id=?1",
                [doc],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
    };
    assert_eq!(pointer(&store), Some((notes, analysis)));
    let guard_after: String = store
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='ponte_item_active_documents'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(guard_before, guard_after);
    assert_invariants(&store, "migrado");
    store
        .restore_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: doc,
        })
        .unwrap();
    assert_eq!(pointer(&store), Some((notes, analysis)));
    store
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: doc,
        })
        .unwrap();
    store
        .purge_trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: doc,
        })
        .unwrap();
    assert_eq!(pointer(&store), None);
    assert_invariants(&store, "depois de apagar");
}

/// A6. Restore routes: product recovery of a 22 library keeps the
/// identities and drops `ident_state`; a portable import gives every row
/// one identity; a role change keeps the identities.
#[test]
fn f2a_m3_restore_routes_leave_every_row_identified_and_provisional() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_with_content(dir.path(), "PonteProj");
    let home = path.parent().unwrap().to_path_buf();
    let source_idents = {
        let store = Store::open(&path).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO ident_state(id,origin,lineage,epoch) VALUES(1,'stamped','linhagem','epoca')",
                [],
            )
            .unwrap();
        all_idents(&store.conn)
    };
    assert!(!source_idents.is_empty());
    let now = chrono::Local::now().naive_local();
    let place = crate::recovery::Place::new(Some(&home), &home);
    let built = crate::recovery::restore_local_copy(&path, &place, now).unwrap();
    {
        let recovered = Store::open(&crate::paths::db_path_in(&built.folder)).unwrap();
        assert_eq!(all_idents(&recovered.conn), source_idents);
        let state: i64 = recovered
            .conn
            .query_row("SELECT count(*) FROM ident_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(state, 0);
        assert_invariants(&recovered, "recuperado");
    }
    // Portable copy (it carries schema 21 content) imported.
    let portable = dir.path().join("copia.pontecopia");
    crate::portable_copy::export(&path, &portable, "senha-da-copia-portatil-123").unwrap();
    let later = now + chrono::Duration::seconds(5);
    let imported =
        crate::portable_copy::import(&portable, "senha-da-copia-portatil-123", &place, later)
            .unwrap();
    {
        let store = Store::open(&crate::paths::db_path_in(&imported.folder)).unwrap();
        assert_invariants(&store, "importado");
        assert!(!all_idents(&store.conn).is_empty());
    }
    // Local → Host → Local.
    let mut store = Store::open(&path).unwrap();
    let before = all_idents(&store.conn);
    for mode in [IntranetMode::Host, IntranetMode::Local] {
        let mut cfg = store.load_intranet_config().unwrap();
        cfg.mode = mode;
        cfg.member_name = "ANFITRIAO".into();
        cfg.bind_addr = "127.0.0.1:0".into();
        cfg.cabinet_token = crate::intranet::generate_token();
        store.save_intranet_config(&cfg).unwrap();
        drop(store);
        store = Store::open(&path).unwrap();
        assert_eq!(all_idents(&store.conn), before);
    }
}

/// A7. After the migration every table refuses INSERT, UPDATE and DELETE
/// from a Ponte of schema 21 (a process that opened the library before).
#[test]
fn f2a_m4_every_table_is_fenced_after_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_with_content(dir.path(), "PonteProj");
    {
        let mut store = Store::open(&path).unwrap();
        store
            .save_intranet_config(&folder_config(
                "Casa",
                vec![dir
                    .path()
                    .join("Drive")
                    .join("ponte-sync.ponte")
                    .display()
                    .to_string()],
            ))
            .unwrap();
        store.save_note(1, "nota na fila").unwrap();
    }
    test_support::downgrade_to_21(&path).unwrap();
    drop(Store::open(&path).unwrap());
    let old = raw_as(&path, 21);
    old.execute_batch("BEGIN").unwrap();
    let mut fenced_by_text = Vec::new();
    for table in tables(&old) {
        let error = old
            .execute(&format!("INSERT INTO \"{table}\" DEFAULT VALUES"), [])
            .expect_err(&format!("INSERT em {table} passou"));
        assert!(
            error.to_string().contains(GUARD_MESSAGE),
            "{table}: {error}"
        );
        let rows: i64 = old
            .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
                r.get(0)
            })
            .unwrap();
        let column: String = old
            .query_row(
                &format!("SELECT name FROM pragma_table_info('{table}') LIMIT 1"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        for (operation, sql) in [
            (
                "update",
                format!("UPDATE \"{table}\" SET \"{column}\"=\"{column}\""),
            ),
            ("delete", format!("DELETE FROM \"{table}\"")),
        ] {
            if rows > 0 {
                let error = old
                    .execute(&sql, [])
                    .expect_err(&format!("{operation} em {table} passou"));
                assert!(
                    error.to_string().contains(GUARD_MESSAGE),
                    "{table}: {error}"
                );
            } else {
                // No row to fire on: the guard itself is checked.
                let name = format!("ponte_guard_v22_{table}_{operation}");
                let sql: String = old
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1",
                        [&name],
                        |r| r.get(0),
                    )
                    .unwrap_or_else(|_| panic!("sem {name}"));
                assert!(sql.contains("ponte_client_schema() < 22"), "{sql}");
                fenced_by_text.push(format!("{table}.{operation}"));
            }
        }
    }
    old.execute_batch("ROLLBACK").unwrap();
    for table in [
        "import_receipts",
        "offline_outbox",
        "offline_watermark",
        "item_ident",
        "current_pointer",
    ] {
        assert!(tables(&old).contains(&table.to_owned()), "{table}");
    }
    eprintln!("guardas conferidas pelo texto (tabela vazia): {fenced_by_text:?}");
}

/// A8. A failure inside the 21→22 transaction leaves a valid 21 library with
/// the schema (and guards) it had, and the verified copy.
#[test]
fn f2a_m4_failure_inside_the_migration_keeps_21_and_its_guards() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_with_content(dir.path(), "PonteProj");
    test_support::downgrade_to_21(&path).unwrap();
    let schema_before = schema_rows(&Connection::open(&path).unwrap());
    let counts_before = counts(&Connection::open(&path).unwrap());
    crate::idents::hooks::BEFORE_MIGRATION_COMMIT.with(|h| {
        *h.borrow_mut() = Some(Box::new(|_: &Path| anyhow::bail!("falha injetada (teste)")));
    });
    let error = Store::open(&path).err().expect("a abertura devia falhar");
    crate::idents::hooks::BEFORE_MIGRATION_COMMIT.with(|h| h.borrow_mut().take());
    assert!(
        format!("{error:#}").contains("A atualização do acervo parou em"),
        "{error:#}"
    );
    assert_eq!(test_support::user_version(&path).unwrap(), 21);
    assert_eq!(
        schema_rows(&Connection::open(&path).unwrap()),
        schema_before
    );
    let copies = migration_copies(&path);
    assert_eq!(copies.len(), 1, "{copies:?}");
    crate::upgrade_backup::verify(&copies[0]).unwrap();
    assert_eq!(test_support::user_version(&copies[0]).unwrap(), 21);
    assert_eq!(
        counts(&Connection::open(&copies[0]).unwrap()),
        counts_before
    );
    // Opened again, it migrates.
    drop(Store::open(&path).unwrap());
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
}

/// A9. The copy is the state the migration locked: another writer waits,
/// the copy has the counts read under the lock, and restoring it through
/// the product gives the tables of the library before the migration, but
/// for the synchronization the restore takes out (`SYNC_TABLES`).
#[test]
fn f2a_m4_copy_is_the_locked_state_and_restores_through_the_product() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_with_content(dir.path(), "PonteProj");
    {
        let mut store = Store::open(&path).unwrap();
        store
            .save_intranet_config(&folder_config(
                "Casa",
                vec![dir
                    .path()
                    .join("Drive")
                    .join("ponte-sync.ponte")
                    .display()
                    .to_string()],
            ))
            .unwrap();
        store.save_note(1, "nota na fila").unwrap();
    }
    test_support::downgrade_to_21(&path).unwrap();
    let before_conn = Connection::open(&path).unwrap();
    let before_counts = counts(&before_conn);
    let before: BTreeMap<String, Vec<String>> = tables(&before_conn)
        .into_iter()
        .map(|t| {
            let rows = dump(&before_conn, &t);
            (t, rows)
        })
        .collect();
    drop(before_conn);
    let waited = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = waited.clone();
    crate::idents::hooks::BEFORE_MIGRATION_COMMIT.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |db| {
            let other = raw_as(db, 22);
            other
                .busy_timeout(std::time::Duration::from_millis(0))
                .unwrap();
            let error = other.execute_batch("BEGIN IMMEDIATE").err();
            seen.set(error.is_some_and(|e| e.to_string().contains("locked")));
            Ok(())
        }));
    });
    let opened = Store::open(&path);
    crate::idents::hooks::BEFORE_MIGRATION_COMMIT.with(|h| h.borrow_mut().take());
    drop(opened.unwrap());
    assert!(waited.get(), "outro processo gravou durante a migração");
    let copies = migration_copies(&path);
    assert_eq!(copies.len(), 1, "{copies:?}");
    let copy_conn = Connection::open(&copies[0]).unwrap();
    assert_eq!(counts(&copy_conn), before_counts);
    drop(copy_conn);
    // Restored through "Restaurar cópia…".
    let home = path.parent().unwrap().to_path_buf();
    let built = crate::recovery::restore_local_copy(
        &copies[0],
        &crate::recovery::Place::new(Some(&home), &home),
        chrono::Local::now().naive_local(),
    )
    .unwrap();
    let restored = Connection::open(crate::paths::db_path_in(&built.folder)).unwrap();
    let sync_tables: BTreeSet<&str> = crate::recovery::SYNC_TABLES
        .iter()
        .map(|(t, _)| *t)
        .collect();
    let mut differ = Vec::new();
    for (table, rows) in &before {
        if dump(&restored, table) != *rows {
            differ.push(table.clone());
        }
    }
    for table in &differ {
        assert!(
            sync_tables.contains(table.as_str()),
            "{table} mudou na restauração"
        );
    }
    assert!(differ.contains(&"offline_outbox".to_owned()), "{differ:?}");
    assert_eq!(
        test_support::user_version(&crate::paths::db_path_in(&built.folder)).unwrap(),
        22
    );
}

/// A10. A process that opened the library at 21 and paused there never
/// migrates it again after another process did, and a Ponte of schema 21
/// that had it open can no longer write.
#[test]
fn f2a_m4_a_process_that_started_at_21_never_writes_after_another_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_with_content(dir.path(), "PonteProj");
    test_support::downgrade_to_21(&path).unwrap();
    let old = raw_as(&path, 21);
    old.execute(
        "UPDATE documents SET last_seen_at=last_seen_at WHERE id=1",
        [],
    )
    .expect("um Ponte 21 grava num acervo 21");
    let after_p2: std::rc::Rc<
        std::cell::RefCell<
            Option<(
                BTreeMap<(String, i64), String>,
                Vec<(String, String, Option<String>)>,
            )>,
        >,
    > = Default::default();
    let keep = after_p2.clone();
    let db = path.clone();
    crate::idents::hooks::AFTER_ENSURE.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            let p2 = Store::open(&db).unwrap();
            *keep.borrow_mut() = Some((all_idents(&p2.conn), schema_rows(&p2.conn)));
        }));
    });
    let p1 = Store::open(&path);
    crate::idents::hooks::AFTER_ENSURE.with(|h| h.borrow_mut().take());
    let p1 = p1.unwrap();
    let (idents, schema) = after_p2.borrow_mut().take().expect("P2 migrou");
    assert_eq!(test_support::user_version(&path).unwrap(), 22);
    assert_eq!(all_idents(&p1.conn), idents, "P1 refez a migração");
    assert_eq!(schema_rows(&p1.conn), schema);
    assert_eq!(migration_copies(&path).len(), 1, "P1 fez outra cópia");
    // The old Ponte that had it open: its writes are refused.
    for sql in [
        "UPDATE documents SET last_seen_at=last_seen_at WHERE id=1",
        "INSERT INTO import_receipts DEFAULT VALUES",
        "DELETE FROM notes",
    ] {
        let error = old.execute(sql, []).expect_err(sql);
        assert!(error.to_string().contains(GUARD_MESSAGE), "{sql}: {error}");
    }
}

/// A11. Behaviour unchanged: across a folder scenario with every kind of
/// item, the invariants hold after every step; the shared file is still
/// format 8 with schema 21 and its content does not depend on the F2a
/// tables; every change, applied here and then replayed elsewhere, gives the
/// rows it inserts the same identities. (The F1 and F0 tests run unchanged
/// in the same `cargo test --lib`.)
#[test]
fn f2a_behaviour_unchanged() {
    let mut p = pair();
    assert_invariants(&p.a, "A, início");
    assert_invariants(&p.b, "B, início");
    let made_a = crate::f1_inventory::fill(&mut p.a, "a");
    let _ = made_a;
    assert_invariants(&p.a, "A, depois de preencher");
    let queued_a = outbox(&p.a);
    assert!(queued_a.len() > 10);

    // Every change replayed into B, before any exchange.
    let mut replica = {
        let path = p.dir.path().join("replica.sqlite3");
        crate::upgrade_backup::copy_consistently(&p.dir.path().join("b.sqlite3"), &path).unwrap();
        Store::open(&path).unwrap()
    };
    let rekeyed = replay(&p.a, &mut replica, &queued_a);
    let mut variants = BTreeSet::new();
    for (entry, result) in queued_a.iter().zip(&rekeyed) {
        let variant = serde_json::to_value(&entry.mutation).unwrap();
        variants.insert(match variant {
            serde_json::Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
            other => other.to_string(),
        });
        assert_eq!(result.skipped, 0, "{}", entry.label);
        assert_eq!(
            result.applied.len(),
            entry.idents.len(),
            "{}: {:?} → {:?}",
            entry.label,
            entry.idents,
            result.applied
        );
        for (slot, row) in &result.applied {
            assert_eq!(
                ident(&replica, &slot.kind, *row).as_deref(),
                Some(slot.ident.as_str()),
                "{}",
                entry.label
            );
        }
    }
    let carried: usize = queued_a.iter().map(|e| e.idents.len()).sum();
    assert!(carried >= 15, "{carried}");
    assert_invariants(&replica, "réplica");
    eprintln!("variantes reaplicadas ({}): {variants:?}", variants.len());

    // The exchange's own replay gives the queue's identities to the rows
    // of its temporary library.
    sync(&mut p.a);
    assert_invariants(&p.a, "A, depois de sincronizar");
    sync(&mut p.b);
    assert_invariants(&p.b, "B, depois de sincronizar");
    crate::f1_inventory::fill(&mut p.b, "b");
    assert_invariants(&p.b, "B, depois de preencher");
    let queued_b: Vec<String> = outbox(&p.b)
        .into_iter()
        .flat_map(|e| e.idents.into_iter().map(|s| s.ident))
        .collect();
    assert!(!queued_b.is_empty());
    let in_temp: std::rc::Rc<std::cell::RefCell<BTreeSet<String>>> = Default::default();
    let keep = in_temp.clone();
    crate::idents::hooks::PEEK_TEMP.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |store: &Store| {
            keep.borrow_mut()
                .extend(all_idents(&store.conn).into_values());
            assert!(test_support::invariant_problems(&store.conn)
                .unwrap()
                .is_empty());
        }))
    });
    // A wrote after B's last read: B's exchange builds a temporary library.
    p.a.save_note(p.a.list_documents("").unwrap()[0].id, "nota de A")
        .unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    crate::idents::hooks::PEEK_TEMP.with(|h| h.borrow_mut().take());
    let in_temp = in_temp.borrow();
    for value in &queued_b {
        assert!(
            in_temp.contains(value),
            "{value} não está no acervo temporário"
        );
    }
    assert_invariants(&p.b, "B, depois da troca");
    sync(&mut p.a);
    assert_invariants(&p.a, "A, depois da troca");

    // Lixeira, restore and purge.
    let doc = p.a.list_documents("").unwrap()[0].id;
    let target = || crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id: doc,
    };
    p.a.trash_item(target()).unwrap();
    assert_invariants(&p.a, "A, Lixeira");
    p.a.restore_item(target()).unwrap();
    p.a.trash_item(target()).unwrap();
    p.a.purge_trash_item(target()).unwrap();
    assert_invariants(&p.a, "A, apagado de vez");
    sync(&mut p.a);
    sync(&mut p.b);
    assert_invariants(&p.a, "A, fim");
    assert_invariants(&p.b, "B, fim");

    // The shared file: format 8, schema 21, no identity in it.
    let header = crate::sharedfolder::read_header(&p.drive).unwrap().unwrap();
    assert_eq!((header.format, header.schema_version), (8, 21));
    let copy = crate::sharedfolder::read(&p.drive, KEY).unwrap().unwrap();
    let text = serde_json::to_string(&copy.payload.journal).unwrap()
        + &serde_json::to_string(&copy.payload.snapshot).unwrap();
    assert!(!text.contains("ident"), "o arquivo leva identidades");
    // The exported content does not depend on the F2a tables.
    let with = serde_json::to_vec(&p.a.export_snapshot().unwrap()).unwrap();
    p.a.conn
        .execute_batch(
            "SAVEPOINT sem_f2a;
             DROP TABLE item_ident; DROP TABLE current_pointer; DROP TABLE ident_state;",
        )
        .unwrap();
    let without = serde_json::to_vec(&p.a.export_snapshot().unwrap());
    p.a.conn
        .execute_batch("ROLLBACK TO sem_f2a; RELEASE sem_f2a;")
        .unwrap();
    assert_eq!(with, without.unwrap());
    assert_invariants(&p.a, "A, depois da comparação");

    // A team: principal and client.
    let team_dir = tempfile::tempdir().unwrap();
    let db = team_dir.path().join("host.sqlite3");
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
    host.import_capture(&crate::model::sample_capture())
        .unwrap();
    let mut server = crate::intranet::HostServer::start(db.clone(), "127.0.0.1:0").unwrap();
    let mut client = Store::open(&team_dir.path().join("client.sqlite3")).unwrap();
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
    assert_invariants(&client, "cliente");
    let doc = client.list_documents("").unwrap()[0].id;
    client.save_note(doc, "nota do cliente").unwrap();
    client.sync_from_host(false).unwrap();
    server.stop();
    assert_invariants(&client, "cliente, depois de enviar");
    let host = Store::open(&db).unwrap();
    assert_invariants(&host, "principal");
}

/// Time of the 21→22 step on a library of 5,000 documents (each with a
/// note): run with `--ignored --nocapture`.
#[test]
#[ignore]
fn f2a_migration_time_on_a_large_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("PonteProj").join("acervo.sqlite3");
    let documents: u32 = std::env::var("F2A_DOCUMENTOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    let started = std::time::Instant::now();
    {
        let mut store = Store::open(&path).unwrap();
        for n in 0..documents {
            let receipt = store
                .import_capture(&capture(
                    &format!("SEFAZ-PRO-2025/{:05}", n + 1),
                    &format!("Texto do documento {n}."),
                ))
                .unwrap();
            store.save_note(receipt.document_id, "nota").unwrap();
        }
    }
    eprintln!("gerar {documents} documentos: {:?}", started.elapsed());
    test_support::downgrade_to_21(&path).unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    let rows: i64 = counts(&Connection::open(&path).unwrap()).values().sum();
    let started = std::time::Instant::now();
    let store = Store::open(&path).unwrap();
    let elapsed = started.elapsed();
    let idents: i64 = store
        .conn
        .query_row("SELECT count(*) FROM item_ident", [], |r| r.get(0))
        .unwrap();
    eprintln!(
        "migração 21→22: {elapsed:?} para {documents} documentos ({rows} linhas, {} MB, {idents} identidades)",
        size / (1024 * 1024)
    );
    assert_invariants(&store, "grande");
    let started = std::time::Instant::now();
    drop(store);
    let _again = Store::open(&path).unwrap();
    eprintln!("abertura seguinte (já 22): {:?}", started.elapsed());
}
