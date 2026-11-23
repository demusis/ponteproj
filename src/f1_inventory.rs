//! F1 (Frente F): characterization tests of where a local row id lives and
//! whether it is the same on every device. They document the behaviour of
//! 0.50.0 and change nothing in it.
//!
//! Tests marked `#[ignore]` reproduce a defect of the current code: they
//! assert the behaviour a fix must give, so they FAIL when run with
//! `--ignored` (the failure message says what happened instead).
//!
//! Every helper here copies the patterns of `renumber::tests` (two libraries
//! sharing folders, a principal and a client on a private port).
#![allow(clippy::all)]

use crate::intranet::{IntranetConfig, IntranetMode};
use crate::storage::Store;
use std::collections::BTreeMap;

const KEY: &str = "senha-de-teste-123";

/// Dumps the schema of a freshly opened library (every table, column, key,
/// foreign key, whether the id is AUTOINCREMENT, and the triggers) to the
/// file named by `F1_SCHEMA_OUT`, for the inventory. Always passes.
#[test]
fn f1_dump_schema() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("s.sqlite3")).unwrap();
    let conn = &store.conn;
    let mut out = String::new();
    let tables: Vec<(String, String)> = conn
        .prepare("SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for (name, sql) in &tables {
        let auto = sql.to_uppercase().contains("AUTOINCREMENT");
        out.push_str(&format!(
            "## {name}{}\n",
            if auto { "  [AUTOINCREMENT]" } else { "" }
        ));
        let cols: Vec<(String, String, i64)> = conn
            .prepare(&format!("PRAGMA table_info({name})"))
            .unwrap()
            .query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get(5)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for (col, ty, pk) in cols {
            out.push_str(&format!(
                "  {col} {ty}{}\n",
                if pk > 0 {
                    format!(" PK{pk}")
                } else {
                    String::new()
                }
            ));
        }
        let fks: Vec<(String, String, String, String)> = conn
            .prepare(&format!("PRAGMA foreign_key_list({name})"))
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(3)?,
                    r.get(2)?,
                    r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    r.get(6)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for (from, table, to, on_delete) in fks {
            out.push_str(&format!(
                "  FK {from} -> {table}({to}) ON DELETE {on_delete}\n"
            ));
        }
    }
    let triggers: Vec<(String, String)> = conn
        .prepare("SELECT name, tbl_name FROM sqlite_master WHERE type='trigger' ORDER BY name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    out.push_str("## triggers\n");
    for (name, table) in triggers {
        out.push_str(&format!("  {name} on {table}\n"));
    }
    if let Ok(path) = std::env::var("F1_SCHEMA_OUT") {
        std::fs::write(path, &out).unwrap();
    }
    assert!(tables.len() > 30, "{out}");
}

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

/// Since F0 (R2-B4) an exchange names the items it keeps aside because the
/// result does not cover them. The plain helpers below fail on such a note,
/// so every scenario here also proves there is no false alarm; the tests
/// that expect one use the `_raw` helpers.
fn quiet(report: crate::intranet::FolderSyncReport) -> crate::intranet::FolderSyncReport {
    assert!(
        !report
            .notes
            .iter()
            .any(|n| n.contains("no resultado desta sincronização")),
        "unexpected loss notice: {:?}",
        report.notes
    );
    report
}

fn sync_raw(store: &mut Store) -> crate::intranet::FolderSyncReport {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
}

fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    quiet(sync_raw(store))
}

fn sync_in_raw(store: &mut Store, files: Vec<String>) -> crate::intranet::FolderSyncReport {
    let mut cfg = store.load_intranet_config().unwrap();
    cfg.folder_files = files;
    store.save_intranet_config(&cfg).unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
}

fn sync_in(store: &mut Store, files: Vec<String>) -> crate::intranet::FolderSyncReport {
    quiet(sync_in_raw(store, files))
}

/// Two computers sharing one folder, in sync on a library with one SIGADOC
/// document.
struct Pair {
    _dir: tempfile::TempDir,
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
    Pair { _dir: dir, a, b }
}

#[derive(Clone, Copy)]
enum Where {
    Both,
    Drive,
    Pen,
}

/// Two folders (a Drive and a pen drive) and two computers joined to both.
struct Folders {
    dir: tempfile::TempDir,
    drive: std::path::PathBuf,
    pen: std::path::PathBuf,
    a: Store,
    b: Store,
}

impl Folders {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let both = vec![drive.display().to_string(), pen.display().to_string()];
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", both.clone()))
            .unwrap();
        a.import_capture(&crate::model::sample_capture()).unwrap();
        let mut f = Self {
            dir,
            drive,
            pen,
            a,
            b: Store::open(std::path::Path::new(":memory:")).unwrap(),
        };
        f.sync_a(Where::Both);
        let mut b = Store::open(&f.dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", both))
            .unwrap();
        f.b = b;
        f.sync_b(Where::Both);
        f
    }

    fn files(&self, at: Where) -> Vec<String> {
        let drive = self.drive.display().to_string();
        let pen = self.pen.display().to_string();
        match at {
            Where::Both => vec![drive, pen],
            Where::Drive => vec![drive],
            Where::Pen => vec![pen],
        }
    }

    fn sync_a(&mut self, at: Where) -> crate::intranet::FolderSyncReport {
        let files = self.files(at);
        sync_in(&mut self.a, files)
    }

    fn sync_b(&mut self, at: Where) -> crate::intranet::FolderSyncReport {
        let files = self.files(at);
        sync_in(&mut self.b, files)
    }
}

/// A principal computer (host) and a client, on a private port.
struct Team {
    _dir: tempfile::TempDir,
    host: Store,
    client: Store,
    db: std::path::PathBuf,
    token: String,
    server: Option<crate::intranet::HostServer>,
}

impl Team {
    fn new() -> Self {
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
        host.import_capture(&crate::model::sample_capture())
            .unwrap();
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
            _dir: dir,
            host,
            client,
            db,
            token,
            server: None,
        };
        team.online();
        team.client.sync_from_host(false).unwrap();
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
}

impl Drop for Team {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
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

fn number(tag: &str, n: u32) -> String {
    let base = match tag {
        "a" => 100,
        "b" => 200,
        "c" => 300,
        _ => 900,
    };
    format!("SEFAZ-PRO-2026/{:05}", base + n)
}

fn doc_by_number(store: &Store, number: &str) -> i64 {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .unwrap_or_else(|| panic!("sem {number}"))
        .id
}

fn task_by_title(store: &Store, title: &str) -> Option<crate::tasks::Task> {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.title == title)
}

fn one(store: &Store, sql: &str, value: rusqlite::types::Value) -> Option<i64> {
    use rusqlite::OptionalExtension;
    store
        .conn
        .query_row(sql, [value], |r| r.get::<_, i64>(0))
        .optional()
        .unwrap()
}

/// What `fill` made that is addressed by a uid (row ids are found by it).
#[derive(Clone, Debug, Default)]
struct Made {
    tag: String,
    resource: String,
    resource_use: String,
    project: String,
    category: String,
    template: String,
    block: String,
    example: String,
    generation: String,
}

/// Creates one item of every kind the snapshot carries, through the public
/// API (in shared-folder mode each one is applied here and queued). `tag`
/// keeps two fills apart.
fn fill(store: &mut Store, tag: &str) -> Made {
    use crate::attachments::Owner;
    use base64::Engine;
    let pdf_url = format!("https://www.sigadoc.mt.gov.br/arquivos/{tag}.pdf");
    // Document with two versions (captures) and requests.
    let main = store
        .import_capture(&capture(&number(tag, 1), "Primeira versão do texto."))
        .unwrap();
    let mut v2 = capture(&number(tag, 1), "Segunda versão do texto, mais nova.");
    v2.captured_at = "2026-11-03T12:00:00Z".into();
    let main2 = store.import_capture(&v2).unwrap();
    assert_eq!(main.document_id, main2.document_id);
    let doc = main.document_id;
    // Another document with a PDF (file) and its OCR page.
    let mut with_pdf = capture(&number(tag, 2), "Documento com PDF.");
    with_pdf.references.push(crate::model::DocumentReference {
        label: "PDF".into(),
        url: pdf_url.clone(),
        kind: "attachment".into(),
    });
    let bundle = store
        .import_bundle(
            &with_pdf,
            &[crate::files::FileInput {
                url: pdf_url,
                data_base64: base64::engine::general_purpose::STANDARD
                    .encode(include_bytes!("../tests/fixtures/document.pdf")),
            }],
        )
        .unwrap();
    let doc2 = bundle.document_id;
    let file = store.list_files(bundle.capture_id).unwrap()[0].id;
    store
        .save_page_text(
            file,
            0,
            &crate::pdf_reader::PageText {
                text: format!("texto reconhecido {tag}"),
                language: "por".into(),
                created_at: "2026-11-05T10:00:00Z".into(),
            },
        )
        .unwrap();
    // Note, draft, analysis (with a source and a citation), work context.
    store.save_note(doc, &format!("Nota {tag}")).unwrap();
    let draft = store
        .save_draft(doc, main2.capture_id, "resposta", &format!("Minuta {tag}"))
        .unwrap();
    let source_text = "Segunda versão do texto, mais nova.".to_owned();
    let analysis = crate::model::Analysis {
        capture_id: main2.capture_id,
        summary: format!("Resumo {tag}"),
        reason: format!("Justificativa {tag}"),
        model: "modelo-de-teste".into(),
        created_at: "2026-11-05T10:00:00Z".into(),
        priority: "normal".into(),
        materials: vec![crate::evidence::Source {
            id: format!("C{}", main2.capture_id),
            document_id: doc,
            capture_id: main2.capture_id,
            label: "Captura".into(),
            file_id: None,
            page: None,
            method: "captura".into(),
            text: source_text,
        }],
        citations: vec![crate::evidence::Citation {
            source_id: format!("C{}", main2.capture_id),
            quote: "Segunda versão".into(),
            claim: "há uma segunda versão".into(),
        }],
        ..Default::default()
    };
    store.save_analysis(doc, &analysis).unwrap();
    store
        .save_work_context(
            doc,
            &crate::work_context::WorkContext {
                notes: format!("Contexto {tag}"),
                ..Default::default()
            },
        )
        .unwrap();
    // A link between the two documents.
    store
        .add_document_link(doc, doc2, "relacionado", &format!("base {tag}"))
        .unwrap();
    // Tasks: one of the document, a subtask that depends on it, a standalone
    // one, one that goes to the Lixeira.
    let mut task = crate::tasks::Task::new(doc);
    task.title = format!("T-{tag}-doc");
    let task_id = store.save_task(&task).unwrap();
    let mut sub = crate::tasks::Task::new(doc);
    sub.title = format!("T-{tag}-sub");
    sub.parent_task_id = Some(task_id);
    sub.dependencies = vec![task_id];
    store.save_task(&sub).unwrap();
    let mut alone = crate::tasks::Task::standalone();
    alone.title = format!("T-{tag}-alone");
    store.save_task(&alone).unwrap();
    let mut doomed = crate::tasks::Task::new(doc);
    doomed.title = format!("T-{tag}-trash");
    let doomed_id = store.save_task(&doomed).unwrap();
    store
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Task,
            id: doomed_id,
        })
        .unwrap();
    // Attachments of a document and of a task.
    store
        .add_attachment(
            Owner::Document(doc),
            &format!("anexo-{tag}.pdf"),
            "do documento",
            format!("%PDF-1.4 anexo {tag}").as_bytes(),
        )
        .unwrap();
    store
        .add_attachment(
            Owner::Task(task_id),
            &format!("tarefa-{tag}.pdf"),
            "da tarefa",
            format!("%PDF-1.4 anexo da tarefa {tag}").as_bytes(),
        )
        .unwrap();
    // An atendimento linked to the document and the task, with an attachment.
    let mut att = crate::saiop::SaiopAttendance::new_manual();
    att.reference_raw = format!("PROC-{tag}");
    att.reference_norm = crate::saiop::normalize_reference(&att.reference_raw);
    att.subject = format!("At-{tag}");
    att.document_id = Some(doc);
    att.task_ids = vec![task_id];
    let att_id = store.save_saiop_attendance(&att, None).unwrap();
    store
        .add_attachment(
            Owner::Attendance(att_id),
            &format!("atendimento-{tag}.pdf"),
            "do atendimento",
            format!("%PDF-1.4 anexo do atendimento {tag}").as_bytes(),
        )
        .unwrap();
    // A spreadsheet batch with one row.
    let mut row = crate::saiop::SaiopAttendance::new_manual();
    row.reference_raw = format!("LOTE-{tag}");
    row.reference_norm = crate::saiop::normalize_reference(&row.reference_raw);
    row.subject = format!("Lote-{tag}");
    row.source_row = Some(2);
    let preview = crate::saiop::SaiopImportPreview {
        file_name: format!("planilha-{tag}.xlsx"),
        file_sha256: format!("{:0>64}", tag),
        sheet_name: "Plan1".into(),
        headers: vec![],
        items: vec![(
            row,
            crate::saiop::RawSpreadsheetRow {
                sheet_name: "Plan1".into(),
                row_number: 2,
                ..Default::default()
            },
        )],
        ignored_formula_rows: 0,
        repeated_references: 0,
        flagged_count: 0,
        linked_documents: 0,
        tramitado_keep_open: false,
        already_registered: vec![None],
    };
    store.commit_saiop_import(&preview).unwrap();
    // Recurso and a use linked to document, atendimento and task.
    let resource = store
        .save_resource(
            &crate::resources::Resource {
                name: format!("Projetor {tag}"),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let resource_use = store
        .save_resource_use(
            &crate::resources::ResourceUse {
                resource_uid: resource.clone(),
                who: format!("Servidor {tag}"),
                start: "2099-01-05T09:00".into(),
                end: "2099-01-05T10:00".into(),
                document_id: Some(doc),
                saiop_id: Some(att_id),
                task_id: Some(task_id),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    // Project and category.
    let project = store
        .save_project(&crate::projects::Project::new(&format!("Projeto {tag}")))
        .unwrap()
        .uid;
    let category = store
        .save_category(&crate::categories::Category::new(&format!("Cat {tag}"), 1))
        .unwrap()
        .uid;
    // Template, block, example (with an attachment) and a generation record.
    let block = store
        .save_block(
            &crate::work_templates::InstructionBlock::new(
                &format!("Bloco {tag}"),
                "Estilo",
                "Use frases curtas.",
            ),
            None,
        )
        .unwrap()
        .uid;
    let template = store
        .save_template(
            &crate::work_templates::WorkTemplate::new(
                &format!("Modelo {tag}"),
                "resposta",
                "Ref. {{numero_documento}}",
            ),
            None,
        )
        .unwrap();
    let example = store
        .save_example(
            &crate::work_templates::Example {
                template_uid: template.uid.clone(),
                title: format!("Exemplo {tag}"),
                demand: "demanda".into(),
                reply: "resposta".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    store
        .add_attachment(
            Owner::Example(example.id),
            &format!("exemplo-{tag}.pdf"),
            "do exemplo",
            format!("%PDF-1.4 anexo do exemplo {tag}").as_bytes(),
        )
        .unwrap();
    let generation = uuid::Uuid::new_v4().simple().to_string();
    store
        .record_generation(&crate::work_templates::GenerationRecord {
            uid: generation.clone(),
            document_id: doc,
            capture_id: main2.capture_id,
            draft_id: Some(draft),
            template_name: "nenhum".into(),
            outcome: crate::work_templates::Outcome::Refused {
                reason: "teste".into(),
            },
            ..Default::default()
        })
        .unwrap();
    Made {
        tag: tag.into(),
        resource,
        resource_use,
        project,
        category,
        template: template.uid,
        block,
        example: example.uid,
        generation,
    }
}

/// The row id of every item `fill` made, found in `store` by what does not
/// change (number, title, name, uid), kind by kind.
fn locate(store: &Store, made: &Made) -> BTreeMap<&'static str, i64> {
    use crate::attachments::Owner;
    use rusqlite::types::Value as V;
    let tag = made.tag.as_str();
    let mut out = BTreeMap::new();
    let doc = doc_by_number(store, &number(tag, 1));
    let doc2 = doc_by_number(store, &number(tag, 2));
    out.insert("document", doc);
    out.insert("document(pdf)", doc2);
    let latest = one(
        store,
        "SELECT latest_capture_id FROM documents WHERE id=?1",
        V::Integer(doc),
    )
    .unwrap();
    out.insert("capture(current)", latest);
    out.insert(
        "capture(older)",
        one(
            store,
            &format!("SELECT id FROM captures WHERE document_id=?1 AND id<>{latest}"),
            V::Integer(doc),
        )
        .unwrap(),
    );
    let cap2 = one(
        store,
        "SELECT latest_capture_id FROM documents WHERE id=?1",
        V::Integer(doc2),
    )
    .unwrap();
    out.insert("capture(pdf)", cap2);
    let file = one(
        store,
        "SELECT id FROM files WHERE capture_id=?1",
        V::Integer(cap2),
    )
    .unwrap();
    out.insert("file", file);
    out.insert(
        "page_text(file key)",
        one(
            store,
            "SELECT file_id FROM page_text WHERE file_id=?1",
            V::Integer(file),
        )
        .unwrap(),
    );
    out.insert(
        "note",
        one(
            store,
            "SELECT max(id) FROM notes WHERE document_id=?1",
            V::Integer(doc),
        )
        .unwrap(),
    );
    out.insert(
        "draft",
        one(
            store,
            "SELECT id FROM drafts WHERE document_id=?1",
            V::Integer(doc),
        )
        .unwrap(),
    );
    out.insert(
        "analysis",
        one(
            store,
            "SELECT id FROM analyses WHERE document_id=?1",
            V::Integer(doc),
        )
        .unwrap(),
    );
    out.insert(
        "work_context(doc key)",
        one(
            store,
            "SELECT document_id FROM work_context WHERE document_id=?1",
            V::Integer(doc),
        )
        .unwrap(),
    );
    out.insert(
        "link",
        one(
            store,
            &format!("SELECT id FROM document_links WHERE source_id=?1 AND target_id={doc2}"),
            V::Integer(doc),
        )
        .unwrap(),
    );
    let task = task_by_title(store, &format!("T-{tag}-doc")).unwrap().id;
    out.insert("task", task);
    let sub = task_by_title(store, &format!("T-{tag}-sub")).unwrap();
    out.insert("task(sub)", sub.id);
    assert_eq!(sub.parent_task_id, Some(task), "parent follows");
    assert_eq!(sub.dependencies, vec![task], "dependency follows");
    out.insert(
        "task(alone)",
        task_by_title(store, &format!("T-{tag}-alone")).unwrap().id,
    );
    // A trashed task is not listed: find it in the trash.
    let trashed: Vec<i64> = store
        .conn
        .prepare("SELECT item_id FROM trash WHERE kind='task' AND document_id=?1")
        .unwrap()
        .query_map([doc], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(trashed.len(), 1, "one trashed task of {tag}");
    out.insert("task(trash)", trashed[0]);
    let find_att = |owner: Owner, name: &str| {
        store
            .list_attachments(owner)
            .unwrap()
            .into_iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("sem anexo {name}"))
            .id
    };
    out.insert(
        "attachment(document)",
        find_att(Owner::Document(doc), &format!("anexo-{tag}.pdf")),
    );
    out.insert(
        "attachment(task)",
        find_att(Owner::Task(task), &format!("tarefa-{tag}.pdf")),
    );
    let atts = store.list_saiop_attendances().unwrap();
    let att = atts
        .iter()
        .find(|a| a.subject == format!("At-{tag}"))
        .unwrap();
    assert_eq!(att.task_ids, vec![task], "atendimento task_ids follow");
    assert_eq!(att.document_id, Some(doc));
    out.insert("saiop", att.id);
    out.insert(
        "attachment(saiop)",
        find_att(Owner::Attendance(att.id), &format!("atendimento-{tag}.pdf")),
    );
    let row = atts
        .iter()
        .find(|a| a.subject == format!("Lote-{tag}"))
        .unwrap();
    out.insert("saiop(batch row)", row.id);
    let batch = store
        .list_saiop_batches()
        .unwrap()
        .into_iter()
        .find(|b| b.file_name == format!("planilha-{tag}.xlsx"))
        .unwrap();
    assert_eq!(row.batch_id, Some(batch.id));
    out.insert("saiop_batch", batch.id);
    let by_uid = |table: &str, uid: &str| {
        one(
            store,
            &format!("SELECT id FROM {table} WHERE uid=?1"),
            V::Text(uid.into()),
        )
        .unwrap_or_else(|| panic!("sem {table} {uid}"))
    };
    out.insert("resource(row)", by_uid("resources", &made.resource));
    out.insert(
        "resource_use(row)",
        by_uid("resource_uses", &made.resource_use),
    );
    let use_ = store
        .list_resource_uses()
        .unwrap()
        .into_iter()
        .find(|u| u.uid == made.resource_use)
        .unwrap();
    assert_eq!(
        (use_.document_id, use_.saiop_id, use_.task_id),
        (Some(doc), Some(att.id), Some(task))
    );
    out.insert("project(row)", by_uid("projects", &made.project));
    out.insert("category(row)", by_uid("categories", &made.category));
    out.insert("template(row)", by_uid("work_templates", &made.template));
    out.insert("block(row)", by_uid("instruction_blocks", &made.block));
    let example = by_uid("template_examples", &made.example);
    out.insert("example(row)", example);
    out.insert(
        "attachment(example)",
        find_att(Owner::Example(example), &format!("exemplo-{tag}.pdf")),
    );
    out.insert(
        "generation(row)",
        by_uid("draft_generations", &made.generation),
    );
    out
}

/// The snapshot of a library, section by section, as JSON (bookkeeping that
/// is not an item left out: revision, schema, generation, team events).
fn sections(store: &Store) -> BTreeMap<String, serde_json::Value> {
    let value = serde_json::to_value(store.export_snapshot().unwrap()).unwrap();
    let mut out = BTreeMap::new();
    for (key, section) in value.as_object().unwrap() {
        match key.as_str() {
            "revision" | "schema_version" | "generation" | "team_events" => {}
            "templates" | "resources" => {
                for (inner, rows) in section.as_object().unwrap() {
                    out.insert(format!("{key}.{inner}"), rows.clone());
                }
            }
            _ => {
                out.insert(key.clone(), section.clone());
            }
        }
    }
    out
}

fn diff(before: &BTreeMap<&'static str, i64>, after: &BTreeMap<&'static str, i64>) -> Vec<String> {
    before
        .iter()
        .filter(|(k, v)| after.get(*k) != Some(v))
        .map(|(k, v)| format!("{k}: {v} -> {:?}", after.get(k)))
        .collect()
}

// --------------------------------------------------------------------- C1 --

/// C1, shared folder. A fills one item of every kind after the first
/// exchange (so each one is queued and replayed into the copy), publishes,
/// and B adopts. Every section of the snapshot (hence every id, every id
/// embedded in a payload and every content) is identical on both, and every
/// item is found under the same id on both.
#[test]
fn c1_a_published_item_has_the_same_id_on_every_device_shared_folder() {
    let mut p = pair();
    let made = fill(&mut p.a, "a");
    sync(&mut p.a);
    sync(&mut p.b);
    assert_eq!(locate(&p.a, &made), locate(&p.b, &made));
    let (sa, sb) = (sections(&p.a), sections(&p.b));
    for (key, value) in &sa {
        assert_eq!(Some(value), sb.get(key), "section {key} differs");
    }
    assert_eq!(sa.len(), sb.len());
    eprintln!("C1 folder ids: {:?}", locate(&p.a, &made));
}

/// C1, first publication: a library that writes the very first copy
/// publishes its own ids, and the device that joins takes them as they are.
#[test]
fn c1_the_first_publication_carries_the_publisher_s_own_ids() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    // Made while the library was still alone (local mode): nothing queued.
    let made = fill(&mut a, "a");
    let alone = locate(&a, &made);
    a.save_intranet_config(&folder_config("Casa", files.clone()))
        .unwrap();
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    sync(&mut b);
    assert_eq!(locate(&a, &made), alone, "the publisher keeps its ids");
    assert_eq!(
        locate(&b, &made),
        alone,
        "the joiner takes the publisher's ids"
    );
    assert_eq!(sections(&a), sections(&b));
}

/// C1, team: an item the client creates online goes to the principal, which
/// numbers it; the client adopts the principal's snapshot, so the id the
/// client shows is the principal's.
#[test]
fn c1_a_published_item_has_the_same_id_on_every_device_team() {
    let mut t = Team::new();
    let made = fill(&mut t.client, "a");
    assert!(
        t.client.list_offline_outbox().unwrap().is_empty(),
        "sent online"
    );
    t.client.sync_from_host(false).unwrap();
    assert_eq!(locate(&t.client, &made), locate(&t.host, &made));
    let (sh, sc) = (sections(&t.host), sections(&t.client));
    for (key, value) in &sh {
        assert_eq!(Some(value), sc.get(key), "section {key} differs");
    }
}

// --------------------------------------------------------------------- C2 --

/// C2, later revisions: once published, A's items keep their ids (and every
/// row of A's snapshot stays byte for byte) after B publishes more items on
/// top, on A and on B.
#[test]
fn c2_a_published_item_keeps_its_id_across_later_publications_by_another_device() {
    let mut p = pair();
    let made = fill(&mut p.a, "a");
    sync(&mut p.a);
    let published = locate(&p.a, &made);
    let rows_then = sections(&p.a);
    sync(&mut p.b);
    let made_b = fill(&mut p.b, "b");
    sync(&mut p.b);
    sync(&mut p.a);
    assert_eq!(locate(&p.a, &made), published);
    assert_eq!(locate(&p.b, &made), published);
    assert_eq!(locate(&p.a, &made_b), locate(&p.b, &made_b));
    // Row containment: every row of A's items at that revision is still
    // there, unchanged (item_versions and the trash included).
    let rows_now = sections(&p.a);
    for (key, rows) in &rows_then {
        let now = rows_now[key].as_array().cloned().unwrap_or_default();
        for row in rows.as_array().cloned().unwrap_or_default() {
            // The revision counter of the library is bookkeeping, not an item.
            if key == "item_versions" && row[0] == "cabinet" {
                continue;
            }
            assert!(now.contains(&row), "{key}: row changed or vanished: {row}");
        }
    }
}

/// C2, divergent copies: A publishes its items only to the pen drive (they
/// are published there, with ids), B publishes its own only to the Drive and
/// makes the Drive the newer copy. When the two copies meet, the Drive is the
/// base and A's journal is replayed on it: A's PUBLISHED items get new ids,
/// on A and on B. This is the case where a published (kind, id) changes.
#[test]
fn c2_a_published_item_of_the_losing_copy_changes_id_in_a_divergent_merge() {
    let mut f = Folders::new();
    let made_a = fill(&mut f.a, "a");
    f.sync_a(Where::Pen);
    let in_pen = locate(&f.a, &made_a);
    let made_b = fill(&mut f.b, "b");
    f.sync_b(Where::Drive);
    // More changes on the Drive so it is the newest copy.
    let doc = f.b.list_documents("").unwrap()[0].id;
    for n in 0..5 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.b.save_task(&task).unwrap();
    }
    f.sync_b(Where::Drive);
    let in_drive = locate(&f.b, &made_b);
    f.sync_a(Where::Both);
    f.sync_b(Where::Both);
    let merged_a = locate(&f.a, &made_a);
    assert_eq!(
        merged_a,
        locate(&f.b, &made_a),
        "both devices agree after the merge"
    );
    assert_eq!(
        locate(&f.a, &made_b),
        in_drive,
        "the winner's items keep their ids"
    );
    let changed = diff(&in_pen, &merged_a);
    eprintln!("C2 ids published in the pen drive that changed in the merge: {changed:#?}");
    assert!(
        !changed.is_empty(),
        "the losing copy's published ids changed"
    );
}

/// C2, reuse: a document published with id k, then purged and published
/// without it. In shared-folder mode every exchange builds a fresh library
/// from the copy, so AUTOINCREMENT only knows the ids present in the copy:
/// the next document created anywhere takes k again. (kind, id) then names
/// another item at a later revision of the same lineage.
#[test]
fn c2_a_purged_id_is_given_to_another_item_in_shared_folder_mode() {
    let mut p = pair();
    let x =
        p.a.import_capture(&capture(&number("a", 1), "Documento X"))
            .unwrap()
            .document_id;
    sync(&mut p.a);
    let target = crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id: x,
    };
    p.a.trash_item(target).unwrap();
    p.a.purge_trash_item(crate::trash::Target {
        kind: crate::trash::Kind::Document,
        id: x,
    })
    .unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    p.b.import_capture(&capture(&number("b", 1), "Documento Y"))
        .unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    let y = doc_by_number(&p.a, &number("b", 1));
    assert_eq!(doc_by_number(&p.b, &number("b", 1)), y);
    assert_eq!(y, x, "the purged id was given to another document");
    // The version row of the purged X now describes Y (see R2-B7).
    let meta = p.a.item_version("document", y).unwrap();
    eprintln!("C2 reuse: X and Y share id {x}; item_versions(document,{y}) = {meta:?}");
}

/// C2, team: the principal keeps its own AUTOINCREMENT counters, so a purged
/// id is not given again there (contrast with the shared folder).
#[test]
fn c2_the_principal_never_reuses_a_purged_document_id() {
    let mut t = Team::new();
    let x = t
        .host
        .import_capture(&capture(&number("a", 1), "Documento X"))
        .unwrap()
        .document_id;
    t.host
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: x,
        })
        .unwrap();
    t.host
        .purge_trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Document,
            id: x,
        })
        .unwrap();
    let y = t
        .host
        .import_capture(&capture(&number("b", 1), "Documento Y"))
        .unwrap()
        .document_id;
    assert!(y > x, "x={x} y={y}");
}

// --------------------------------------------------------------------- C3 --

/// C3, nobody else wrote: an item created here and replayed into the copy
/// keeps the id it had before the exchange (the copy and this library had the
/// same items, so the replay numbers alike).
#[test]
fn c3_a_local_creation_keeps_its_id_when_nobody_else_wrote() {
    let mut p = pair();
    let made = fill(&mut p.a, "a");
    let before = locate(&p.a, &made);
    sync(&mut p.a);
    let after = locate(&p.a, &made);
    eprintln!("C3 (alone) changed: {:#?}", diff(&before, &after));
    assert_eq!(before, after);
}

/// C3, another device published first: every kind numbered by AUTOINCREMENT
/// after the other device's items gets a new id when replayed; uid-addressed
/// kinds (projects, categories, recursos...) also get new row ids.
#[test]
fn c3_a_local_creation_gets_a_new_id_when_another_device_published_first() {
    let mut p = pair();
    let made_b = fill(&mut p.b, "b");
    sync(&mut p.b);
    let made = fill(&mut p.a, "a");
    let before = locate(&p.a, &made);
    sync(&mut p.a);
    let after = locate(&p.a, &made);
    let changed = diff(&before, &after);
    eprintln!(
        "C3 (after another device) before: {before:?}\nafter: {after:?}\nchanged: {changed:#?}"
    );
    let unchanged: Vec<&&str> = before.keys().filter(|k| before[*k] == after[*k]).collect();
    eprintln!("C3 unchanged kinds: {unchanged:?}");
    assert!(!changed.is_empty());
    sync(&mut p.b);
    assert_eq!(locate(&p.b, &made), after);
    assert_eq!(locate(&p.a, &made_b), locate(&p.b, &made_b));
}

/// C3, team client offline: items created without connection have local
/// ids; when the queue is sent, the principal numbers them and the client
/// adopts the principal's ids.
#[test]
fn c3_a_team_client_s_offline_creation_takes_the_principal_s_ids() {
    let mut t = Team::new();
    t.offline();
    let made = fill(&mut t.client, "a");
    let before = locate(&t.client, &made);
    // The principal creates items of its own meanwhile.
    let made_host = fill(&mut t.host, "b");
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert!(
        t.client.list_sync_conflicts().unwrap().is_empty(),
        "{:?}",
        t.client.list_sync_conflicts().unwrap()
    );
    let after = locate(&t.client, &made);
    assert_eq!(after, locate(&t.host, &made));
    assert_eq!(locate(&t.client, &made_host), locate(&t.host, &made_host));
    eprintln!("C3 team changed: {:#?}", diff(&before, &after));
    assert!(!diff(&before, &after).is_empty());
}

// --------------------------------------------------------------------- C4 --

/// C4: the uids that exist travel unchanged (tasks, recursos, uses,
/// projects, categories, templates, blocks, examples, generations), and a
/// request id travels in `requests`.
#[test]
fn c4_the_stable_identifiers_that_exist_travel_unchanged() {
    let mut p = pair();
    let made = fill(&mut p.a, "a");
    let task_uid_before = task_by_title(&p.a, "T-a-doc").unwrap().uid;
    let requests_before: Vec<String> =
        p.a.export_snapshot()
            .unwrap()
            .requests
            .into_iter()
            .map(|r| r.request_id)
            .collect();
    sync(&mut p.a);
    sync(&mut p.b);
    assert_eq!(task_by_title(&p.b, "T-a-doc").unwrap().uid, task_uid_before);
    let requests_after: Vec<String> =
        p.b.export_snapshot()
            .unwrap()
            .requests
            .into_iter()
            .map(|r| r.request_id)
            .collect();
    assert_eq!(requests_before, requests_after);
    // locate() finds every uid-addressed item by its uid on B.
    let _ = locate(&p.b, &made);
}

/// C4 / F0: a task that reaches a library without uid (a copy written before
/// task uids) got a RANDOM one at adoption in 0.50.0, so two devices adopting
/// the same copy gave it two different uids. It now gets one derived from the
/// copy: the same on both, and different for two tasks.
#[test]
fn c4_a_task_without_uid_gets_the_same_uid_on_each_device() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = Store::open(&dir.path().join("s.sqlite3")).unwrap();
    let doc = source
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let mut task = crate::tasks::Task::new(doc);
    task.title = "Tarefa antiga".into();
    source.save_task(&task).unwrap();
    let mut other = crate::tasks::Task::new(doc);
    other.title = "Outra tarefa antiga".into();
    source.save_task(&other).unwrap();
    let mut snapshot = source.export_snapshot().unwrap();
    for task in &mut snapshot.tasks {
        task.uid.clear();
    }
    let adopt = |name: &str| {
        let mut store = Store::open(&dir.path().join(name)).unwrap();
        store
            .apply_snapshot(
                &snapshot,
                |_, _| anyhow::bail!("sem PDF"),
                |_| anyhow::bail!("sem anexo"),
            )
            .unwrap();
        (
            task_by_title(&store, "Tarefa antiga").unwrap(),
            task_by_title(&store, "Outra tarefa antiga").unwrap(),
        )
    };
    let ((one_, other_one), (two, other_two)) = (adopt("1.sqlite3"), adopt("2.sqlite3"));
    assert_eq!(one_.id, two.id, "same row id");
    assert!(!one_.uid.is_empty());
    assert_eq!(one_.uid, two.uid, "the same uid for the same task");
    assert_eq!(other_one.uid, other_two.uid);
    assert_ne!(one_.uid, other_one.uid, "two tasks, two uids");
    assert!(uuid::Uuid::parse_str(&one_.uid).is_ok(), "{}", one_.uid);
}

// ------------------------------------------------- IdMap coverage (unit) --

/// What `IdMap::remap` translates inside the AI sources of a change: the
/// numeric fields and the names `C{capture}` / `C{capture}F{file}P{page}`.
/// An attachment source `A{attachment}H{hash}` keeps the old attachment id,
/// and a fact's `source_id` keeps the old source name.
#[test]
fn idmap_translates_attachment_source_names_and_fact_sources() {
    use crate::idmap::{CreatedId, IdKind, IdMap};
    let mut map = IdMap::default();
    map.learn(
        &[
            CreatedId::new(IdKind::Capture, "", 5),
            CreatedId::new(IdKind::Attachment, "", 7),
        ],
        Some(&[
            CreatedId::new(IdKind::Capture, "", 50),
            CreatedId::new(IdKind::Attachment, "", 70),
        ]),
    );
    let source = |id: &str| crate::evidence::Source {
        id: id.into(),
        document_id: 1,
        capture_id: 5,
        label: "x".into(),
        file_id: None,
        page: None,
        method: "captura".into(),
        text: "texto".into(),
    };
    let analysis = crate::model::Analysis {
        capture_id: 5,
        materials: vec![source("C5"), source("A7Hdeadbeef")],
        citations: vec![
            crate::evidence::Citation {
                source_id: "C5".into(),
                quote: "texto".into(),
                claim: "c".into(),
            },
            crate::evidence::Citation {
                source_id: "A7Hdeadbeef".into(),
                quote: "texto".into(),
                claim: "c".into(),
            },
        ],
        facts: vec![crate::model::Fact {
            source_id: "C5".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let frame = crate::intranet::CabinetSnapshot::default();
    let out = map
        .remap(
            &crate::intranet::IntranetMutation::SaveAnalysis {
                document_id: 1,
                analysis,
            },
            &frame,
            None,
        )
        .unwrap();
    let crate::intranet::IntranetMutation::SaveAnalysis { analysis, .. } = out else {
        unreachable!()
    };
    let names: Vec<&str> = analysis.materials.iter().map(|s| s.id.as_str()).collect();
    let cited: Vec<&str> = analysis
        .citations
        .iter()
        .map(|c| c.source_id.as_str())
        .collect();
    eprintln!(
        "materials {names:?} citations {cited:?} fact {:?}",
        analysis.facts[0].source_id
    );
    assert_eq!(
        names,
        ["C50", "A70Hdeadbeef"],
        "attachment source name not translated"
    );
    assert_eq!(cited, ["C50", "A70Hdeadbeef"]);
    assert_eq!(
        analysis.facts[0].source_id, "C50",
        "fact source not translated"
    );
}

/// The part of the same check that holds today: the capture source and its
/// citation follow the capture.
#[test]
fn idmap_translates_capture_source_names_and_their_citations() {
    use crate::idmap::{CreatedId, IdKind, IdMap};
    let mut map = IdMap::default();
    map.learn(
        &[CreatedId::new(IdKind::Capture, "", 5)],
        Some(&[CreatedId::new(IdKind::Capture, "", 50)]),
    );
    let analysis = crate::model::Analysis {
        capture_id: 5,
        materials: vec![crate::evidence::Source {
            id: "C5".into(),
            document_id: 1,
            capture_id: 5,
            label: "x".into(),
            file_id: None,
            page: None,
            method: "captura".into(),
            text: "texto".into(),
        }],
        citations: vec![crate::evidence::Citation {
            source_id: "C5".into(),
            quote: "texto".into(),
            claim: "c".into(),
        }],
        ..Default::default()
    };
    let out = map
        .remap(
            &crate::intranet::IntranetMutation::SaveAnalysis {
                document_id: 1,
                analysis,
            },
            &crate::intranet::CabinetSnapshot::default(),
            None,
        )
        .unwrap();
    let crate::intranet::IntranetMutation::SaveAnalysis { analysis, .. } = out else {
        unreachable!()
    };
    assert_eq!(analysis.capture_id, 50);
    assert_eq!(analysis.materials[0].id, "C50");
    assert_eq!(analysis.citations[0].source_id, "C50");
}

// --------------------------------------------------------------------- C5 --

/// R2-B2 against 0.50: a team client imports through the native host with a
/// note, while it has older changes queued and the principal has items of
/// its own on the same ids. The note must land on the imported document on
/// both computers. Not reproducible today: a client with a queue (or
/// pending numbers) applies the import locally and queues it, so the id the
/// native host gets is this library's; online, the client adopts the
/// principal's snapshot before answering, so the principal's id is its own.
#[test]
fn r2_b2_the_native_host_s_note_lands_on_the_imported_document_on_a_team_client() {
    let mut t = Team::new();
    t.offline();
    // Y: made by the client offline (local id 2 here).
    t.client
        .import_capture(&capture(&number("b", 1), "Documento Y do cliente"))
        .unwrap();
    // Z: made by the principal meanwhile (id 2 there).
    t.host
        .import_capture(&capture(&number("c", 1), "Documento Z do principal"))
        .unwrap();
    t.online();
    let x = capture(&number("a", 1), "Documento X pela extensão");
    let request = serde_json::json!({
        "protocol_version": 1,
        "action": "import_capture",
        "capture": x,
        "extras": {"note": "Nota da extensão"},
    });
    let (reply, _) =
        crate::native::handle_request(&serde_json::to_vec(&request).unwrap(), &mut t.client);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["extras_applied"], true, "{reply}");
    t.client.sync_from_host(false).unwrap();
    for (who, store) in [("principal", &t.host), ("cliente", &t.client)] {
        let x = doc_by_number(store, &number("a", 1));
        let z = doc_by_number(store, &number("c", 1));
        let y = doc_by_number(store, &number("b", 1));
        assert!(
            store
                .load_document(x)
                .unwrap()
                .note
                .contains("Nota da extensão"),
            "{who}"
        );
        assert!(
            store.load_document(z).unwrap().note.is_empty(),
            "{who}: nota em Z"
        );
        assert!(
            store.load_document(y).unwrap().note.is_empty(),
            "{who}: nota em Y"
        );
    }
}

/// R2-B2, online variant: the client's copy is behind (the principal made Z
/// since), nothing is queued. The principal numbers X after Z, the client
/// adopts the principal's snapshot and only then applies the note by the
/// principal's id, which is now its own.
#[test]
fn r2_b2_online_the_reply_id_is_the_client_s_id_after_adoption() {
    let mut t = Team::new();
    t.host
        .import_capture(&capture(&number("c", 1), "Documento Z do principal"))
        .unwrap();
    let x = capture(&number("a", 1), "Documento X pela extensão");
    let request = serde_json::json!({
        "protocol_version": 1,
        "action": "import_capture",
        "capture": x,
        "extras": {"note": "Nota da extensão"},
    });
    let (reply, _) =
        crate::native::handle_request(&serde_json::to_vec(&request).unwrap(), &mut t.client);
    assert_eq!(reply["ok"], true, "{reply}");
    let id = reply["document_id"].as_i64().unwrap();
    assert_eq!(id, doc_by_number(&t.client, &number("a", 1)));
    assert_eq!(id, doc_by_number(&t.host, &number("a", 1)));
    t.client.sync_from_host(false).unwrap();
    assert!(t
        .host
        .load_document(id)
        .unwrap()
        .note
        .contains("Nota da extensão"));
}

fn current_text(store: &Store) -> String {
    let doc = store.list_documents("").unwrap()[0].id;
    store.load_document(doc).unwrap().capture.text
}

/// R2-B3, versions: the current version is a pointer (`latest_capture_id`).
/// In 0.50.0 it was set by whichever import was APPLIED last: B imports an
/// older version offline, A a newer one and publishes first, B's import is
/// replayed after A's and the OLDER version became current. Since F0 the
/// pointer follows the capture time: the newer one stays current, the same
/// on both devices, and trashing it promotes the newest remaining one.
#[test]
fn r2_b3_the_current_version_follows_the_capture_time_and_is_the_same_everywhere() {
    let mut p = pair();
    let sample = crate::model::sample_capture();
    let mut newer = sample.clone();
    newer.request_id = uuid::Uuid::new_v4().to_string();
    newer.text = "Versão nova (A, capturada dia 10).".into();
    newer.captured_at = "2026-11-10T12:00:00Z".into();
    let mut older = sample.clone();
    older.request_id = uuid::Uuid::new_v4().to_string();
    older.text = "Versão antiga (B, capturada dia 05).".into();
    older.captured_at = "2026-11-05T12:00:00Z".into();
    p.a.import_capture(&newer).unwrap();
    sync(&mut p.a);
    p.b.import_capture(&older).unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    assert_eq!(current_text(&p.a), current_text(&p.b));
    eprintln!("R2-B3 current after merge: {}", current_text(&p.a));
    assert_eq!(
        current_text(&p.a),
        newer.text,
        "the capture time elects the newer capture"
    );
    // Trash the current one: the fallback is the highest remaining id.
    let doc = p.a.list_documents("").unwrap()[0].id;
    let current = p.a.load_document(doc).unwrap().summary.id;
    let _ = current;
    let latest = one(
        &p.a,
        "SELECT latest_capture_id FROM documents WHERE id=?1",
        rusqlite::types::Value::Integer(doc),
    )
    .unwrap();
    p.a.trash_item(crate::trash::Target {
        kind: crate::trash::Kind::Capture,
        id: latest,
    })
    .unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    assert_eq!(current_text(&p.a), current_text(&p.b));
    eprintln!("R2-B3 current after trashing it: {}", current_text(&p.a));
    assert_eq!(current_text(&p.a), older.text, "the newest remaining one");
}

/// R2-B3 against 0.50, notes: the current note is the note row with the
/// highest id. B writes a note offline, A writes another and publishes first.
#[test]
fn r2_b3_notes_written_on_two_devices() {
    let mut p = pair();
    let doc = p.a.list_documents("").unwrap()[0].id;
    p.b.save_note(doc, "Nota de B (escrita antes)").unwrap();
    p.a.save_note(doc, "Nota de A (escrita depois)").unwrap();
    sync(&mut p.a);
    let report = sync(&mut p.b);
    sync(&mut p.a);
    let (na, nb) = (
        p.a.load_document(doc).unwrap().note,
        p.b.load_document(doc).unwrap().note,
    );
    eprintln!(
        "R2-B3 notes: A={na:?} B={nb:?} conflicts on B={} report.conflicts={}",
        p.b.list_sync_conflicts().unwrap().len(),
        report.conflicts
    );
    assert_eq!(na, nb, "both devices show the same current note");
}

/// R2-B4: a library that first publishes into an empty pen drive (a forked
/// library: same lineage, its own items, nothing in its journal) has an item
/// that no other copy has. When a device that sees a newer Drive copy and
/// this pen drive exchanges, the pen drive is rewritten from the Drive: its
/// item is in no journal and the revisions differ, so it is neither replayed
/// nor kept aside. The item must survive somewhere (library or a copy kept
/// aside).
#[test]
fn r2_b4_a_losing_mirror_with_an_item_in_no_journal_is_kept_aside() {
    let mut f = Folders::new();
    // B: a forked library (alone, local mode), with Y, that then publishes
    // into an empty pen drive under the same lineage.
    let lineage = crate::sharedfolder::read_header(&f.drive)
        .unwrap()
        .unwrap()
        .lineage;
    let dir = f.dir.path().join("forked");
    std::fs::create_dir_all(&dir).unwrap();
    let mut forked = Store::open(&dir.join("c.sqlite3")).unwrap();
    forked
        .import_capture(&capture(
            &number("c", 1),
            "Documento Y só do acervo bifurcado",
        ))
        .unwrap();
    std::fs::remove_file(&f.pen).unwrap();
    let mut cfg = folder_config("Bifurcado", f.files(Where::Pen));
    cfg.folder_lineage = lineage;
    forked.save_intranet_config(&cfg).unwrap();
    sync(&mut forked);
    // A makes the Drive the newest copy, then exchanges with both.
    let doc = f.a.list_documents("").unwrap()[0].id;
    for n in 0..8 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.a.save_task(&task).unwrap();
    }
    f.sync_a(Where::Drive);
    let files = f.files(Where::Both);
    let report = sync_in_raw(&mut f.a, files);
    eprintln!("R2-B4 report notes: {:?}", report.notes);
    let in_a =
        f.a.list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.document_number == number("c", 1));
    let pen_dir = f.pen.parent().unwrap();
    let aside: Vec<String> = std::fs::read_dir(pen_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "ponte-sync.ponte")
        .collect();
    let pen_has_y = crate::sharedfolder::read(&f.pen, KEY)
        .unwrap()
        .unwrap()
        .payload
        .snapshot
        .captures
        .iter()
        .any(|c| c.capture.document_number == number("c", 1));
    eprintln!("R2-B4: Y in A={in_a}, Y in pen={pen_has_y}, files kept aside={aside:?}");
    assert!(
        in_a || pen_has_y || !aside.is_empty(),
        "Y vanished from every copy A sees"
    );
}

/// R2-B7, purge: `purge_trash_item` deletes the item and its version row,
/// then calls `record_mutation`, which writes a NEW version row for the id
/// that no longer exists. The row travels in the snapshot and is inherited by
/// whatever item takes the id next (see the reuse test of C2).
#[test]
fn r2_b7_a_purge_leaves_no_version_row_for_the_purged_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("s.sqlite3")).unwrap();
    let doc = store
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let mut task = crate::tasks::Task::new(doc);
    task.title = "Para apagar".into();
    let id = store.save_task(&task).unwrap();
    let target = || crate::trash::Target {
        kind: crate::trash::Kind::Task,
        id,
    };
    store.trash_item(target()).unwrap();
    store.purge_trash_item(target()).unwrap();
    let rows = store
        .export_snapshot()
        .unwrap()
        .item_versions
        .into_iter()
        .filter(|(kind, item, _)| kind == "task" && *item == id)
        .collect::<Vec<_>>();
    assert!(
        rows.is_empty(),
        "version row left for purged task {id}: {rows:?}"
    );
    // The library itself keeps no row either (not only the copy it exports).
    let in_table: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM item_versions WHERE kind='task' AND item_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(in_table, 0, "version row left in the table for task {id}");
}

/// R2-B7, refused change marked applied: B's note goes only to the pen
/// drive; A's conflicting note goes to the Drive, which becomes the newest
/// copy. A merges both: B's note is refused (version conflict), becomes a
/// card on A only, and its uid is recorded as applied. B never learns: its
/// queue was emptied when it wrote the pen drive, and the card lives on A.
#[test]
fn r2_b7_a_refused_foreign_change_reaches_its_origin() {
    let mut f = Folders::new();
    let doc = f.a.list_documents("").unwrap()[0].id;
    f.b.save_note(doc, "Nota de B, só no pendrive").unwrap();
    f.sync_b(Where::Pen);
    f.a.save_note(doc, "Nota de A, no Drive").unwrap();
    for n in 0..5 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.a.save_task(&task).unwrap();
    }
    f.sync_a(Where::Drive);
    let report = f.sync_a(Where::Both);
    let cards_a = f.a.list_sync_conflicts().unwrap();
    f.sync_b(Where::Both);
    let cards_b = f.b.list_sync_conflicts().unwrap();
    eprintln!(
        "R2-B7 refused: report.conflicts={} cards on A={} cards on B={} B outbox={} note now={:?}",
        report.conflicts,
        cards_a.len(),
        cards_b.len(),
        f.b.list_offline_outbox().unwrap().len(),
        f.b.load_document(doc).unwrap().note
    );
    assert_eq!(cards_a.len(), 1, "the merger got the card");
    assert!(
        !cards_b.is_empty() || f.b.load_document(doc).unwrap().note.contains("Nota de B"),
        "B, the origin, never learns its note was refused"
    );
}

/// R2-B8: a plan that targets an existing record by number records the
/// target as `request:<id>`. B's record Y shares its request id with A's
/// record X (two isolated mirrors, as an e-mail with the same Message-ID).
/// B's plan is interrupted after its first task; B then exchanges, Y is taken
/// for X (the replay's "prior request" branch), and resuming the plan writes
/// the remaining tasks onto X, a record the plan never targeted.
#[cfg(feature = "desktop")]
#[test]
fn r2_b8_a_resumed_plan_never_writes_on_a_record_it_did_not_target() {
    let mut p = pair();
    let request = uuid::Uuid::new_v4().to_string();
    let mut x = capture(&number("a", 1), "Registro X de A");
    x.request_id = request.clone();
    p.a.import_capture(&x).unwrap();
    sync(&mut p.a);
    let mut y = capture(&number("b", 1), "Registro Y de B");
    y.request_id = request.clone();
    p.b.import_capture(&y).unwrap();
    // B's plan on Y, by number; it fails after the first task.
    let mut plan: crate::plan_import::Plan =
        serde_json::from_str(include_str!("../tests/fixtures/import-plan-exemplo.json")).unwrap();
    plan.request_id = uuid::Uuid::new_v4().to_string();
    plan.document = None;
    plan.document_number = Some(number("b", 1));
    plan.complete_triage = false;
    let tasks_before = p.b.list_tasks().unwrap().len();
    p.b.conn
        .execute_batch(&format!(
            "CREATE TRIGGER falha_simulada BEFORE INSERT ON tasks WHEN (SELECT count(*) FROM tasks)>={} BEGIN SELECT RAISE(ABORT,'falha simulada'); END;",
            tasks_before + 1
        ))
        .unwrap();
    assert!(p.b.import_plan(&plan, false).is_err(), "interrupted");
    p.b.conn
        .execute_batch("DROP TRIGGER falha_simulada")
        .unwrap();
    let target: String =
        p.b.conn
            .query_row(
                "SELECT target FROM plan_import_log WHERE request_id=?1",
                [plan.request_id.to_lowercase()],
                |r| r.get(0),
            )
            .unwrap();
    eprintln!("R2-B8 stored target: {target}");
    sync(&mut p.b);
    let resumed = p.b.import_plan(&plan, false);
    eprintln!(
        "R2-B8 resume: {:?}",
        resumed.as_ref().map(|_| "ok").map_err(|e| e.to_string())
    );
    let message = resumed
        .as_ref()
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("Nada foi gravado"),
        "resume not refused: {message}"
    );
    let x_on_b = doc_by_number(&p.b, &number("a", 1));
    let on_x: Vec<String> =
        p.b.list_tasks()
            .unwrap()
            .into_iter()
            .filter(|t| t.document_id == Some(x_on_b))
            .map(|t| t.title)
            .collect();
    let y_exists =
        p.b.list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.document_number == number("b", 1));
    eprintln!("R2-B8 tasks on X: {on_x:?}; Y still exists on B: {y_exists}");
    let all: Vec<(String, Option<i64>)> =
        p.b.list_tasks()
            .unwrap()
            .into_iter()
            .map(|t| (t.title, t.document_id))
            .collect();
    let cards: Vec<String> =
        p.b.list_sync_conflicts()
            .unwrap()
            .into_iter()
            .map(|c| format!("{} :: {}", c.item_label, c.reason))
            .collect();
    eprintln!(
        "R2-B8 all tasks on B: {all:?}
R2-B8 cards on B: {cards:#?}"
    );
    assert!(
        on_x.iter()
            .all(|t| !plan.tasks.iter().any(|pt| pt.title == *t)),
        "plan tasks written on X: {on_x:?}"
    );
}

// ---------------------------------------------------------------- F0 fixes --

/// The `item_versions` kinds keyed by a row id, with the table of that id
/// (the same list as the product's `ROW_VERSION_KINDS`, kept here so the walk
/// does not trust the code it checks).
const ROW_KINDS: &[(&str, &str)] = &[
    ("document", "documents"),
    ("note", "documents"),
    ("work_context", "documents"),
    ("capture", "captures"),
    ("draft", "drafts"),
    ("analysis", "analyses"),
    ("page_text", "files"),
    ("task", "tasks"),
    ("attachment", "attachments"),
    ("attachment_text", "attachments"),
    ("document_link", "document_links"),
    ("saiop", "saiop_attendances"),
    ("saiop_batch", "saiop_batches"),
];

/// The `item_versions` kinds keyed by something that is never given to
/// another item (a uid hash, a setting key, the library counter).
const KEY_VERSION_KINDS: &[&str] = &[
    "cabinet",
    "setting",
    "project",
    "category",
    "resource",
    "resource_use",
    "template",
    "block",
    "example",
    "generation",
];

/// Every version row of `store` (the table and the snapshot it exports)
/// describes an item that exists; a kind this walk does not know fails, so
/// a new kind must be classified here.
fn assert_no_orphan_versions(store: &Store, who: &str) {
    let rows: Vec<(String, i64)> = store
        .conn
        .prepare("SELECT kind,item_id FROM item_versions")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let exported: Vec<(String, i64)> = store
        .export_snapshot()
        .unwrap()
        .item_versions
        .into_iter()
        .map(|(k, i, _)| (k, i))
        .collect();
    for (place, rows) in [("tabela", rows), ("cópia", exported)] {
        for (kind, id) in rows {
            if let Some((_, table)) = ROW_KINDS.iter().find(|(k, _)| *k == kind) {
                let exists: bool = store
                    .conn
                    .query_row(
                        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
                        [id],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert!(
                    exists,
                    "{who} ({place}): version row ({kind}, {id}) without its item"
                );
            } else {
                assert!(
                    KEY_VERSION_KINDS.contains(&kind.as_str()),
                    "{who}: kind {kind} not classified in the orphan walk"
                );
            }
        }
    }
}

/// F0 (R2-B7a): removing an item for good, by every path that removes one
/// (purge from the Lixeira of a document, a version, a task, an attachment;
/// deleting an atendimento, undoing a batch, deleting an attachment, an
/// example, a template), leaves no version row behind, here, in the copy and
/// on the other device. Walks every kind `fill` makes.
#[test]
fn f0_no_version_row_outlives_its_item_of_any_kind() {
    use crate::trash::{Kind, Target};
    let mut p = pair();
    let made = fill(&mut p.a, "a");
    let ids = locate(&p.a, &made);
    let _ = p.a.extract_attachment_text(ids["attachment(document)"]);
    sync(&mut p.a);
    sync(&mut p.b);
    let kinds: std::collections::BTreeSet<String> =
        p.a.export_snapshot()
            .unwrap()
            .item_versions
            .into_iter()
            .map(|(k, _, _)| k)
            .collect();
    eprintln!("F0 version kinds before removing: {kinds:?}");
    for (kind, _) in ROW_KINDS {
        // A version gets its row when it goes to the Lixeira; a text when
        // it is extracted (not possible from this fake PDF).
        if !["attachment_text", "capture"].contains(kind) {
            assert!(kinds.contains(*kind), "fill makes no {kind}");
        }
    }
    let purge = |store: &mut Store, kind: Kind, id: i64| {
        store.trash_item(Target { kind, id }).unwrap();
        store.purge_trash_item(Target { kind, id }).unwrap();
    };
    p.a.purge_trash_item(Target {
        kind: Kind::Task,
        id: ids["task(trash)"],
    })
    .unwrap();
    purge(&mut p.a, Kind::Capture, ids["capture(older)"]);
    purge(&mut p.a, Kind::Attachment, ids["attachment(document)"]);
    purge(&mut p.a, Kind::Task, ids["task(alone)"]);
    p.a.delete_saiop_attendance(ids["saiop"]).unwrap();
    p.a.undo_saiop_batch(ids["saiop_batch"]).unwrap();
    p.a.delete_attachment(ids["attachment(task)"]).unwrap();
    p.a.delete_example(&made.example).unwrap();
    p.a.delete_template(&made.template).unwrap();
    purge(&mut p.a, Kind::Document, ids["document(pdf)"]);
    purge(&mut p.a, Kind::Document, ids["document"]);
    assert_no_orphan_versions(&p.a, "A antes de sincronizar");
    sync(&mut p.a);
    sync(&mut p.b);
    assert_no_orphan_versions(&p.a, "A");
    assert_no_orphan_versions(&p.b, "B");
}

/// The product cleans exactly the kinds the walk checks.
#[test]
fn f0_the_orphan_walk_and_the_product_know_the_same_row_kinds() {
    assert_eq!(ROW_KINDS, crate::intranet::ROW_VERSION_KINDS);
}

/// F0 (R2-B7a): a version row left behind by 0.50.0 (written straight into
/// the table here) never travels in the copy, so the next item given the id
/// does not inherit it.
#[test]
fn f0_an_orphan_version_row_from_an_older_release_does_not_travel() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("s.sqlite3")).unwrap();
    let meta = crate::intranet::VersionMeta {
        version: 7,
        updated_by: "antigo".into(),
        updated_at: "2026-11-01T00:00:00Z".into(),
    };
    store
        .conn
        .execute(
            "INSERT INTO item_versions(kind,item_id,version,payload) VALUES('task',99,7,?1)",
            [crate::protection::seal(&serde_json::to_vec(&meta).unwrap()).unwrap()],
        )
        .unwrap();
    assert!(!store
        .export_snapshot()
        .unwrap()
        .item_versions
        .iter()
        .any(|(k, i, _)| k == "task" && *i == 99));
}

/// F0 (IdMap): the numbers swap places between the frames (attachment 7 here
/// is 9 there and 9 here is 7 there; capture 5 is 6 and 6 is 5), so every old
/// number is also a valid number on the other side. Each name is translated
/// once, from its own number: sources, citations, facts and the sources of a
/// generation record.
#[test]
fn f0_idmap_translates_attachment_and_fact_sources_when_the_numbers_swap() {
    use crate::idmap::{CreatedId, IdKind, IdMap};
    let mut map = IdMap::default();
    map.learn(
        &[
            CreatedId::new(IdKind::Capture, "", 5),
            CreatedId::new(IdKind::Capture, "b", 6),
            CreatedId::new(IdKind::Attachment, "", 7),
            CreatedId::new(IdKind::Attachment, "b", 9),
        ],
        Some(&[
            CreatedId::new(IdKind::Capture, "", 6),
            CreatedId::new(IdKind::Capture, "b", 5),
            CreatedId::new(IdKind::Attachment, "", 9),
            CreatedId::new(IdKind::Attachment, "b", 7),
        ]),
    );
    let source = |id: &str, capture: i64| crate::evidence::Source {
        id: id.into(),
        document_id: 1,
        capture_id: capture,
        label: "x".into(),
        file_id: None,
        page: None,
        method: "captura".into(),
        text: "texto".into(),
    };
    let names = ["C5", "C6", "A7Hdeadbeef", "A9H0123abcd"];
    let analysis = crate::model::Analysis {
        capture_id: 5,
        materials: vec![
            source(names[0], 5),
            source(names[1], 6),
            source(names[2], 5),
            source(names[3], 6),
        ],
        citations: names
            .iter()
            .map(|n| crate::evidence::Citation {
                source_id: (*n).into(),
                quote: "texto".into(),
                claim: "c".into(),
            })
            .collect(),
        facts: names
            .iter()
            .map(|n| crate::model::Fact {
                source_id: (*n).into(),
                ..Default::default()
            })
            .chain([crate::model::Fact::default()])
            .collect(),
        ..Default::default()
    };
    let frame = crate::intranet::CabinetSnapshot::default();
    let out = map
        .remap(
            &crate::intranet::IntranetMutation::SaveAnalysis {
                document_id: 1,
                analysis,
            },
            &frame,
            None,
        )
        .unwrap();
    let crate::intranet::IntranetMutation::SaveAnalysis { analysis, .. } = out else {
        unreachable!()
    };
    let want = ["C6", "C5", "A9Hdeadbeef", "A7H0123abcd"];
    let got: Vec<&str> = analysis.materials.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(got, want);
    let cited: Vec<&str> = analysis
        .citations
        .iter()
        .map(|c| c.source_id.as_str())
        .collect();
    assert_eq!(cited, want);
    let facts: Vec<&str> = analysis
        .facts
        .iter()
        .map(|f| f.source_id.as_str())
        .collect();
    assert_eq!(facts, ["C6", "C5", "A9Hdeadbeef", "A7H0123abcd", ""]);
    // The sources a generation used follow the same way.
    let used = |id: &str, capture: i64| crate::work_templates::UsedSource {
        id: id.into(),
        label: "anexo".into(),
        document_id: 1,
        capture_id: capture,
        file_id: None,
    };
    let record = crate::work_templates::GenerationRecord {
        uid: "g".into(),
        document_id: 1,
        capture_id: 5,
        sources: vec![used("A7Hdeadbeef", 5), used("A9H0123abcd", 6)],
        ..Default::default()
    };
    let out = map
        .remap(
            &crate::intranet::IntranetMutation::RecordGeneration { record },
            &frame,
            None,
        )
        .unwrap();
    let crate::intranet::IntranetMutation::RecordGeneration { record } = out else {
        unreachable!()
    };
    let used: Vec<&str> = record.sources.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(used, ["A9Hdeadbeef", "A7H0123abcd"]);
}

// ------------------------------------------- F0: plain edits (item 8) --

fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(15));
}

fn link_active(store: &Store, id: i64) -> bool {
    store
        .conn
        .query_row("SELECT active FROM document_links WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}

fn link_id(store: &Store, source: i64, target: i64) -> i64 {
    store
        .conn
        .query_row(
            "SELECT id FROM document_links WHERE source_id=?1 AND target_id=?2",
            [source, target],
            |r| r.get(0),
        )
        .unwrap()
}

fn description(store: &Store, doc: i64) -> String {
    store
        .list_attachments(crate::attachments::Owner::Document(doc))
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
        .description
}

/// Two computers with two documents linked and one attachment, in sync.
fn pair_with_link_and_attachment() -> (Pair, i64, i64) {
    let mut p = pair();
    let doc = p.a.list_documents("").unwrap()[0].id;
    let other =
        p.a.import_capture(&capture(&number("a", 1), "Outro documento"))
            .unwrap()
            .document_id;
    p.a.add_document_link(doc, other, "relacionado", "base")
        .unwrap();
    let link = link_id(&p.a, doc, other);
    p.a.add_attachment(
        crate::attachments::Owner::Document(doc),
        "anexo.pdf",
        "descrição inicial",
        b"%PDF-1.4 anexo",
    )
    .unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    assert!(link_active(&p.b, link));
    (p, doc, link)
}

/// F0 (R2-B7, item 8), shared folder: B changes the description of an
/// attachment without exchanging; A, meanwhile, writes another description
/// and publishes first. B's edit was made on a description that is no longer
/// there: it becomes a conflict card on B, and both computers keep A's.
/// (A link's on/off state is a yes/no without a version: an edit based on
/// "active" cannot tell "never touched" from "deactivated and reactivated",
/// so links keep the 0.50.0 behaviour; see the review round.)
#[test]
fn f0_an_older_description_edit_does_not_overwrite_a_newer_one_in_a_folder() {
    let (mut p, doc, _link) = pair_with_link_and_attachment();
    let attachment =
        p.b.list_attachments(crate::attachments::Owner::Document(doc))
            .unwrap()[0]
            .id;
    p.b.update_attachment_description(attachment, "descrição de B (antes)")
        .unwrap();
    p.a.update_attachment_description(attachment, "descrição de A (depois)")
        .unwrap();
    sync(&mut p.a);
    let report = sync(&mut p.b);
    sync(&mut p.a);
    for (who, store) in [("A", &p.a), ("B", &p.b)] {
        assert_eq!(description(store, doc), "descrição de A (depois)", "{who}");
    }
    let cards = p.b.list_sync_conflicts().unwrap();
    assert_eq!(report.conflicts, 1, "{cards:?}");
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(
        cards[0].reason.contains("depois da versão sobre a qual"),
        "{cards:?}"
    );
}

/// Review 3: a computer adds an attachment and a link without exchanging,
/// edits the attachment's description and deactivates the link, then
/// exchanges: its own edits are applied, with no card. Later, the other
/// computer, which has seen that description, changes it again: applied too.
#[test]
fn f0r_own_edits_of_a_new_attachment_or_link_are_never_refused() {
    let mut p = pair();
    let doc = p.b.list_documents("").unwrap()[0].id;
    let other =
        p.b.import_capture(&capture(&number("b", 1), "Outro documento"))
            .unwrap()
            .document_id;
    p.b.add_document_link(doc, other, "relacionado", "base")
        .unwrap();
    let link = link_id(&p.b, doc, other);
    p.b.add_attachment(
        crate::attachments::Owner::Document(doc),
        "anexo.pdf",
        "descrição inicial",
        b"%PDF-1.4 anexo",
    )
    .unwrap();
    let attachment =
        p.b.list_attachments(crate::attachments::Owner::Document(doc))
            .unwrap()[0]
            .id;
    tick();
    p.b.update_attachment_description(attachment, "descrição de B")
        .unwrap();
    p.b.set_link_active(link, false).unwrap();
    // Another computer published meanwhile, so the replay restamps them.
    p.a.save_note(doc, "Nota de A").unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    sync(&mut p.a);
    assert!(
        p.b.list_sync_conflicts().unwrap().is_empty(),
        "{:?}",
        p.b.list_sync_conflicts().unwrap()
    );
    for (who, store) in [("A", &p.a), ("B", &p.b)] {
        assert_eq!(description(store, doc), "descrição de B", "{who}");
        assert!(!link_active(store, link), "{who}");
    }
    let attachment =
        p.a.list_attachments(crate::attachments::Owner::Document(doc))
            .unwrap()[0]
            .id;
    p.a.update_attachment_description(attachment, "descrição de A, depois")
        .unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    assert!(p.a.list_sync_conflicts().unwrap().is_empty());
    assert_eq!(description(&p.b, doc), "descrição de A, depois");
}

/// Review 3, team: a client offline adds an attachment and edits its
/// description; at the next exchange both are sent, with no card.
#[test]
fn f0r_a_team_client_s_own_edit_of_a_new_attachment_is_sent() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    t.offline();
    t.client
        .add_attachment(
            crate::attachments::Owner::Document(doc),
            "anexo.pdf",
            "descrição inicial",
            b"%PDF-1.4 anexo",
        )
        .unwrap();
    let attachment = t
        .client
        .list_attachments(crate::attachments::Owner::Document(doc))
        .unwrap()[0]
        .id;
    tick();
    t.client
        .update_attachment_description(attachment, "descrição do cliente")
        .unwrap();
    t.host
        .import_capture(&capture(&number("c", 1), "Documento do principal"))
        .unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert!(
        t.client.list_sync_conflicts().unwrap().is_empty(),
        "{:?}",
        t.client.list_sync_conflicts().unwrap()
    );
    assert_eq!(description(&t.host, doc), "descrição do cliente");
}

/// F0 (item 8), shared folder, the other order: A changes first and
/// publishes, B receives it and only then changes the same items and
/// exchanges. B's edits were made on the current values: applied, no card.
#[test]
fn f0_a_newer_link_or_description_edit_is_applied_in_a_folder() {
    let (mut p, doc, link) = pair_with_link_and_attachment();
    let attachment =
        p.a.list_attachments(crate::attachments::Owner::Document(doc))
            .unwrap()[0]
            .id;
    p.a.update_attachment_description(attachment, "descrição de A (antes)")
        .unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    tick();
    p.b.set_link_active(link, false).unwrap();
    p.b.update_attachment_description(attachment, "descrição de B (depois)")
        .unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    assert!(p.b.list_sync_conflicts().unwrap().is_empty());
    for (who, store) in [("A", &p.a), ("B", &p.b)] {
        assert!(!link_active(store, link), "{who}");
        assert_eq!(description(store, doc), "descrição de B (depois)", "{who}");
    }
}

/// F0 (item 8), team: a client offline changes the description of an
/// attachment and deactivates a link; the principal, later, changes them
/// itself. When the client comes back, its older edits are not sent over the
/// principal's newer values: they become conflict cards on the client.
#[test]
fn f0_a_team_client_s_older_plain_edit_does_not_overwrite_the_principal_s_newer_one() {
    let mut t = Team::new();
    let doc = t.host.list_documents("").unwrap()[0].id;
    let other = t
        .host
        .import_capture(&capture(&number("a", 1), "Outro documento"))
        .unwrap()
        .document_id;
    t.host
        .add_document_link(doc, other, "relacionado", "base")
        .unwrap();
    let link = link_id(&t.host, doc, other);
    t.host
        .add_attachment(
            crate::attachments::Owner::Document(doc),
            "anexo.pdf",
            "descrição inicial",
            b"%PDF-1.4 anexo",
        )
        .unwrap();
    t.client.sync_from_host(false).unwrap();
    let attachment = t
        .client
        .list_attachments(crate::attachments::Owner::Document(doc))
        .unwrap()[0]
        .id;
    let _ = link;
    t.offline();
    t.client
        .update_attachment_description(attachment, "descrição do cliente (antes)")
        .unwrap();
    assert_eq!(t.client.list_offline_outbox().unwrap().len(), 1);
    t.host
        .update_attachment_description(attachment, "descrição do principal (depois)")
        .unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    for (who, store) in [("principal", &t.host), ("cliente", &t.client)] {
        assert_eq!(
            description(store, doc),
            "descrição do principal (depois)",
            "{who}"
        );
    }
    let cards = t.client.list_sync_conflicts().unwrap();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
}

/// F0 (R2-B8), a plan that creates its record: B's plan creates record Y
/// (with the plan's request id) and is interrupted after its first task. A
/// already published a record X that came with the same request id. When B
/// exchanges, Y is refused (the request id is taken), and the request id now
/// names X. Resuming B's plan must not write on X: it is refused, in
/// Portuguese, before anything is written.
#[cfg(feature = "desktop")]
#[test]
fn f0_a_resumed_plan_that_created_its_record_refuses_another_record_with_its_request_id() {
    let mut p = pair();
    let mut plan: crate::plan_import::Plan =
        serde_json::from_str(include_str!("../tests/fixtures/import-plan-exemplo.json")).unwrap();
    plan.request_id = uuid::Uuid::new_v4().to_string();
    plan.complete_triage = false;
    assert!(plan.document.is_some(), "the example creates a record");
    let mut x = capture(&number("a", 1), "Registro X de A");
    x.request_id = plan.request_id.to_lowercase();
    p.a.import_capture(&x).unwrap();
    sync(&mut p.a);
    // B creates Y with the plan; the second task fails.
    let tasks_before = p.b.list_tasks().unwrap().len();
    p.b.conn
        .execute_batch(&format!(
            "CREATE TRIGGER falha_simulada BEFORE INSERT ON tasks WHEN (SELECT count(*) FROM tasks)>={} BEGIN SELECT RAISE(ABORT,'falha simulada'); END;",
            tasks_before + 2
        ))
        .unwrap();
    assert!(p.b.import_plan(&plan, false).is_err(), "interrupted");
    p.b.conn
        .execute_batch("DROP TRIGGER falha_simulada")
        .unwrap();
    // B's record went into A's (same request id): its automatic triage task
    // is in no copy any more, so B keeps a copy of its library and says so.
    let report = sync_raw(&mut p.b);
    eprintln!(
        "F0 plan: notes {:?} cards {:?}",
        report.notes,
        p.b.list_sync_conflicts()
            .unwrap()
            .iter()
            .map(|c| c.item_label.clone())
            .collect::<Vec<_>>()
    );
    let x_on_b = doc_by_number(&p.b, &number("a", 1));
    let tasks_on_x = |store: &Store| -> Vec<String> {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .filter(|t| t.document_id == Some(x_on_b))
            .map(|t| t.title)
            .collect()
    };
    let before = tasks_on_x(&p.b);
    let note_before = p.b.load_document(x_on_b).unwrap().note;
    let resumed = p.b.import_plan(&plan, false);
    let message = resumed
        .as_ref()
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    eprintln!("F0 resume of a created record: {message}");
    assert!(resumed.is_err(), "the resume wrote on X");
    assert!(message.contains("Nada foi gravado"), "{message}");
    assert_eq!(tasks_on_x(&p.b), before, "nothing new on X");
    assert_eq!(
        p.b.load_document(x_on_b).unwrap().note,
        note_before,
        "note on X"
    );
    // F0 (R2-B4): B's record went into A's in the replay (same request id),
    // with no conflict card, so B kept a copy of its library and named it.
    let title = plan.document.as_ref().unwrap().title.trim().to_owned();
    assert!(
        report.notes.iter().any(|n| n.contains(&title)),
        "B's record is named: {:?}",
        report.notes
    );
}

// ------------------------------------- F0: current by own time (item 5) --

fn version_at(text: &str, at: &str) -> crate::model::Capture {
    let mut c = crate::model::sample_capture();
    c.request_id = uuid::Uuid::new_v4().to_string();
    c.text = text.into();
    c.captured_at = at.into();
    c
}

fn trash_current(store: &mut Store) {
    let doc = store.list_documents("").unwrap()[0].id;
    let latest = one(
        store,
        "SELECT latest_capture_id FROM documents WHERE id=?1",
        rusqlite::types::Value::Integer(doc),
    )
    .unwrap();
    store
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Capture,
            id: latest,
        })
        .unwrap();
}

/// F0 (R2-B3, item 5), within a device: two libraries receive the same four
/// versions of a document in different orders (one of them captured on the
/// 8th arrives last, as an import replayed late). Both show as current the
/// one captured last (the 10th), and after it goes to the Lixeira both
/// promote the one captured on the 9th, not the one that arrived last.
#[test]
fn f0_the_current_version_follows_the_capture_time_not_the_arrival_order() {
    let dir = tempfile::tempdir().unwrap();
    let versions = [
        version_at("Versão do dia 09.", "2026-11-09T12:00:00Z"),
        version_at("Versão do dia 10.", "2026-11-10T12:00:00Z"),
        version_at("Versão do dia 08.", "2026-11-08T12:00:00Z"),
    ];
    let mut results = Vec::new();
    for (name, order) in [("1.sqlite3", [0, 1, 2]), ("2.sqlite3", [2, 1, 0])] {
        let mut store = Store::open(&dir.path().join(name)).unwrap();
        store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        for index in order {
            store.import_capture(&versions[index]).unwrap();
        }
        let current = current_text(&store);
        trash_current(&mut store);
        results.push((current, current_text(&store)));
    }
    for (current, after_trash) in &results {
        assert_eq!(current, "Versão do dia 10.");
        assert_eq!(after_trash, "Versão do dia 09.");
    }
}

/// F0 (R2-B3, item 5), two computers: A imports a version captured on the
/// 10th and publishes; B imports one captured on the 5th and publishes after
/// (its import is replayed after A's). The newer capture stays current on
/// both, and the older one is kept in the history.
#[test]
fn f0_an_older_version_replayed_later_does_not_become_current() {
    let mut p = pair();
    let newer = version_at("Versão nova (A, capturada dia 10).", "2026-11-10T12:00:00Z");
    let older = version_at(
        "Versão antiga (B, capturada dia 05).",
        "2026-11-05T12:00:00Z",
    );
    p.a.import_capture(&newer).unwrap();
    sync(&mut p.a);
    p.b.import_capture(&older).unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    for (who, store) in [("A", &p.a), ("B", &p.b)] {
        assert_eq!(current_text(store), newer.text, "{who}");
        let doc = store.list_documents("").unwrap()[0].id;
        let texts: Vec<String> = store
            .capture_versions(doc)
            .unwrap()
            .into_iter()
            .map(|v| v.capture.text)
            .collect();
        assert!(texts.contains(&older.text), "{who}: {texts:?}");
    }
}

/// F0 (R2-B3, item 5), analyses: B analyses a document offline (made on the
/// 5th), A analyses it later (the 10th) and publishes first; B's analysis is
/// replayed after A's. The current analysis is A's, the newer one, on both.
#[test]
fn f0_the_current_analysis_follows_the_time_it_was_made() {
    let mut p = pair();
    let doc = p.a.list_documents("").unwrap()[0].id;
    let capture_id = one(
        &p.a,
        "SELECT latest_capture_id FROM documents WHERE id=?1",
        rusqlite::types::Value::Integer(doc),
    )
    .unwrap();
    let analysis = |summary: &str, at: &str| crate::model::Analysis {
        capture_id,
        summary: summary.into(),
        reason: "r".into(),
        model: "modelo-de-teste".into(),
        created_at: at.into(),
        priority: "normal".into(),
        ..Default::default()
    };
    p.b.save_analysis(
        doc,
        &analysis("Análise de B (dia 05)", "2026-11-05T10:00:00Z"),
    )
    .unwrap();
    p.a.save_analysis(
        doc,
        &analysis("Análise de A (dia 10)", "2026-11-10T10:00:00Z"),
    )
    .unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    sync(&mut p.a);
    for (who, store) in [("A", &p.a), ("B", &p.b)] {
        let current = store.load_document(doc).unwrap().analysis.unwrap();
        assert_eq!(current.summary, "Análise de A (dia 10)", "{who}");
    }
}

/// F0 (R2-B7b): the refusal reaches the author once. After the merge on A
/// refuses B's note, B's next exchange turns it into a card on B, with B's
/// text; later exchanges on both computers do not repeat it (B's guard copy
/// is rewritten without it), and the change is in no copy's `applied` list
/// as if it had been incorporated.
#[test]
fn f0_a_refused_foreign_change_becomes_a_card_of_its_author_once() {
    let mut f = Folders::new();
    let doc = f.a.list_documents("").unwrap()[0].id;
    f.b.save_note(doc, "Nota de B, só no pendrive").unwrap();
    let uid = f.b.list_offline_outbox().unwrap()[0].uid.clone();
    f.sync_b(Where::Pen);
    f.a.save_note(doc, "Nota de A, no Drive").unwrap();
    for n in 0..5 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.a.save_task(&task).unwrap();
    }
    f.sync_a(Where::Drive);
    f.sync_a(Where::Both);
    for path in [&f.drive, &f.pen] {
        let copy = crate::sharedfolder::read(path, KEY).unwrap().unwrap();
        assert!(
            !copy.payload.applied.contains(&uid),
            "{}: refused change marked as incorporated",
            path.display()
        );
    }
    f.sync_b(Where::Both);
    let cards_b = f.b.list_sync_conflicts().unwrap();
    assert_eq!(cards_b.len(), 1, "{cards_b:?}");
    let text = serde_json::to_string(&cards_b[0].mutation).unwrap();
    assert!(text.contains("Nota de B, só no pendrive"), "{text}");
    for _ in 0..2 {
        f.sync_a(Where::Both);
        f.sync_b(Where::Both);
    }
    assert_eq!(f.b.list_sync_conflicts().unwrap().len(), 1, "repeated on B");
    assert_eq!(f.a.list_sync_conflicts().unwrap().len(), 1, "repeated on A");
    assert_eq!(
        f.a.load_document(doc).unwrap().note,
        f.b.load_document(doc).unwrap().note
    );
}

// ------------------------------- F0: nothing dropped unseen (item 4) --

fn aside_files(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains("-divergente-"))
                .collect()
        })
        .unwrap_or_default()
}

/// F0 (R2-B4), the library side: after A rewrote the pen drive without Y
/// (the case above), the forked library that made Y exchanges again and
/// would adopt a result without it. Before replacing its library it keeps a
/// verified copy of it and names Y in the report; Y is not put back.
#[test]
fn f0_a_library_that_would_lose_an_item_keeps_a_copy_and_says_which() {
    let mut f = Folders::new();
    let lineage = crate::sharedfolder::read_header(&f.drive)
        .unwrap()
        .unwrap()
        .lineage;
    let dir = f.dir.path().join("forked");
    std::fs::create_dir_all(&dir).unwrap();
    let mut forked = Store::open(&dir.join("c.sqlite3")).unwrap();
    forked
        .import_capture(&capture(
            &number("c", 1),
            "Documento Y só do acervo bifurcado",
        ))
        .unwrap();
    std::fs::remove_file(&f.pen).unwrap();
    let mut cfg = folder_config("Bifurcado", f.files(Where::Pen));
    cfg.folder_lineage = lineage;
    forked.save_intranet_config(&cfg).unwrap();
    sync(&mut forked);
    let doc = f.a.list_documents("").unwrap()[0].id;
    for n in 0..8 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.a.save_task(&task).unwrap();
    }
    f.sync_a(Where::Drive);
    let files = f.files(Where::Both);
    let report_a = sync_in_raw(&mut f.a, files);
    assert!(
        report_a.notes.iter().any(|n| n.contains(&number("c", 1))),
        "A names Y: {:?}",
        report_a.notes
    );
    let report = sync_raw(&mut forked);
    eprintln!("F0 forked notes: {:?}", report.notes);
    let note = report
        .notes
        .iter()
        .find(|n| n.contains(&number("c", 1)) && n.contains("copias-antes-da-substituicao"))
        .unwrap_or_else(|| panic!("Y not named: {:?}", report.notes));
    assert!(note.contains("copias-antes-da-substituicao"), "{note}");
    let backups: Vec<_> = std::fs::read_dir(dir.join("copias-antes-da-substituicao"))
        .unwrap()
        .collect();
    assert!(!backups.is_empty());
    assert!(
        !forked
            .list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.document_number == number("c", 1)),
        "no automatic re-insertion"
    );
}

/// F0 (R2-B4), no false alarm: A sends a document to the Lixeira and purges
/// it, publishing to the Drive only; B, whose pen drive copy still has it,
/// exchanges with both. The removal is in the journal of the result, so
/// nothing is kept aside and no item is named.
#[test]
fn f0_a_recorded_removal_keeps_nothing_aside() {
    let mut f = Folders::new();
    let backups = f.dir.path().join("copias-antes-da-substituicao");
    let before: Vec<_> = std::fs::read_dir(&backups)
        .map(|d| d.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    let x =
        f.a.import_capture(&capture(&number("a", 1), "Documento X"))
            .unwrap()
            .document_id;
    let mut task = crate::tasks::Task::standalone();
    task.title = "Tarefa avulsa que vai embora".into();
    let task = f.a.save_task(&task).unwrap();
    f.sync_a(Where::Both);
    f.sync_b(Where::Both);
    for (kind, id) in [
        (crate::trash::Kind::Document, x),
        (crate::trash::Kind::Task, task),
    ] {
        let target = || crate::trash::Target { kind, id };
        f.a.trash_item(target()).unwrap();
        f.a.purge_trash_item(target()).unwrap();
    }
    f.sync_a(Where::Drive);
    let report = f.sync_b(Where::Both);
    let report_a = f.sync_a(Where::Both);
    eprintln!("F0 notes B: {:?} A: {:?}", report.notes, report_a.notes);
    for notes in [&report.notes, &report_a.notes] {
        assert!(
            !notes.iter().any(
                |n| n.contains("não estão no resultado") || n.contains("não está no resultado")
            ),
            "{notes:?}"
        );
    }
    assert!(aside_files(f.pen.parent().unwrap()).is_empty());
    assert!(aside_files(f.drive.parent().unwrap()).is_empty());
    let after: Vec<_> = std::fs::read_dir(&backups)
        .map(|d| d.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert_eq!(before, after, "a copy of a library was kept");
    assert!(!f
        .b
        .list_documents("")
        .unwrap()
        .iter()
        .any(|d| d.document_number == number("a", 1)));
}

// ------------------------------------------- F0, review round (revisao-f0) --

/// Review 5: the same older copy merged twice (here, put back in the pen
/// drive as a cloud cache would) does not give the merging computer a second
/// card for the change it already refused.
#[test]
fn f0r_a_refused_change_merged_again_does_not_make_a_second_card() {
    let mut f = Folders::new();
    let doc = f.a.list_documents("").unwrap()[0].id;
    f.b.save_note(doc, "Nota de B, só no pendrive").unwrap();
    f.sync_b(Where::Pen);
    let stale = std::fs::read(&f.pen).unwrap();
    f.a.save_note(doc, "Nota de A, no Drive").unwrap();
    for n in 0..5 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.a.save_task(&task).unwrap();
    }
    f.sync_a(Where::Drive);
    f.sync_a(Where::Both);
    assert_eq!(f.a.list_sync_conflicts().unwrap().len(), 1);
    std::fs::write(&f.pen, &stale).unwrap();
    f.sync_a(Where::Both);
    assert_eq!(
        f.a.list_sync_conflicts().unwrap().len(),
        1,
        "a second card for the same refused change"
    );
}

/// Review 6: a plan creates its record and stops after its first task; the
/// person then corrects the record's title in the editor. Resuming the plan
/// finishes it on that record (the version its request made is the plan's).
#[cfg(feature = "desktop")]
#[test]
fn f0r_a_corrected_plan_record_still_lets_the_plan_resume() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("s.sqlite3")).unwrap();
    let mut plan: crate::plan_import::Plan =
        serde_json::from_str(include_str!("../tests/fixtures/import-plan-exemplo.json")).unwrap();
    plan.request_id = uuid::Uuid::new_v4().to_string();
    plan.complete_triage = false;
    assert!(plan.tasks.len() >= 2);
    let before = store.list_tasks().unwrap().len();
    store
        .conn
        .execute_batch(&format!(
            "CREATE TRIGGER falha_simulada BEFORE INSERT ON tasks WHEN (SELECT count(*) FROM tasks)>={} BEGIN SELECT RAISE(ABORT,'falha simulada'); END;",
            before + 2
        ))
        .unwrap();
    assert!(store.import_plan(&plan, false).is_err(), "interrupted");
    store
        .conn
        .execute_batch("DROP TRIGGER falha_simulada")
        .unwrap();
    let doc = store.list_documents("").unwrap()[0].id;
    let mut fields =
        crate::manual::ManualFields::from_capture(&store.load_document(doc).unwrap().capture);
    fields.title = format!("{} (corrigido)", fields.title);
    store.update_manual_document(doc, &fields).unwrap();
    let resumed = store.import_plan(&plan, false);
    assert!(
        resumed.is_ok(),
        "resume refused: {:?}",
        resumed.err().map(|e| e.to_string())
    );
    let on_doc: Vec<String> = store
        .list_tasks()
        .unwrap()
        .into_iter()
        .filter(|t| t.document_id == Some(doc))
        .map(|t| t.title)
        .collect();
    for task in &plan.tasks {
        assert!(on_doc.contains(&task.title.trim().to_owned()), "{on_doc:?}");
    }
}

/// Review 4: A captures a newer version C2 and analyses it offline; B, still
/// on C1, analyses C1 later by the clock and publishes first. After A's
/// exchange the current version is C2, so the analysis shown and the facts
/// given to drafts are those of C2, on both computers.
#[test]
fn f0r_the_current_analysis_is_one_of_the_current_version() {
    let mut p = pair();
    let doc = p.a.list_documents("").unwrap()[0].id;
    let c1 = one(
        &p.b,
        "SELECT latest_capture_id FROM documents WHERE id=?1",
        rusqlite::types::Value::Integer(doc),
    )
    .unwrap();
    let mut newer = crate::model::sample_capture();
    newer.request_id = uuid::Uuid::new_v4().to_string();
    newer.text = "Versão C2: o prazo para resposta é de dez dias.".into();
    newer.captured_at = "2026-11-10T12:00:00Z".into();
    let c2 = p.a.import_capture(&newer).unwrap().capture_id;
    let analysis = |capture_id: i64, summary: &str, at: &str, quote: &str| crate::model::Analysis {
        capture_id,
        summary: summary.into(),
        reason: "r".into(),
        model: "modelo-de-teste".into(),
        created_at: at.into(),
        priority: "normal".into(),
        facts: vec![crate::model::Fact {
            id: "f1".into(),
            kind: "prazo".into(),
            statement: "Há um prazo.".into(),
            quote: quote.into(),
            checked: crate::model::FactCheck {
                quote: true,
                origin: crate::model::Origin::Document,
                ..Default::default()
            },
            ..Default::default()
        }],
        ..Default::default()
    };
    p.a.save_analysis(
        doc,
        &analysis(
            c2,
            "Análise de C2 (A)",
            "2026-11-10T13:00:00Z",
            "prazo para resposta",
        ),
    )
    .unwrap();
    p.b.save_analysis(
        doc,
        &analysis(
            c1,
            "Análise de C1 (B)",
            "2026-11-11T09:00:00Z",
            "texto antigo de C1",
        ),
    )
    .unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    sync(&mut p.b);
    for (who, store) in [("A", &p.a), ("B", &p.b)] {
        let detail = store.load_document(doc).unwrap();
        assert_eq!(detail.capture.text, newer.text, "{who}: current version");
        assert_eq!(
            detail.analysis.as_ref().map(|a| a.summary.as_str()),
            Some("Análise de C2 (A)"),
            "{who}"
        );
        let materials = store.draft_materials(doc).unwrap();
        assert_eq!(materials.facts.len(), 1, "{who}: facts of C2 for drafts");
    }
}

/// Review 2: the forked library's guard copy holds Y, which the result
/// lacks, and keeping it aside fails (its archive name is taken). The
/// exchange stops before anything is written or adopted: the guard and the
/// recorded revision stay as they were, and once the name is free the next
/// exchange keeps the guard aside and goes on.
#[test]
fn f0r_a_guard_that_cannot_be_kept_aside_stops_the_exchange() {
    let mut f = Folders::new();
    let lineage = crate::sharedfolder::read_header(&f.drive)
        .unwrap()
        .unwrap()
        .lineage;
    let dir = f.dir.path().join("forked");
    std::fs::create_dir_all(&dir).unwrap();
    let mut forked = Store::open(&dir.join("c.sqlite3")).unwrap();
    forked
        .import_capture(&capture(
            &number("c", 1),
            "Documento Y só do acervo bifurcado",
        ))
        .unwrap();
    std::fs::remove_file(&f.pen).unwrap();
    let mut cfg = folder_config("Bifurcado", f.files(Where::Pen));
    cfg.folder_lineage = lineage;
    forked.save_intranet_config(&cfg).unwrap();
    sync(&mut forked);
    let doc = f.a.list_documents("").unwrap()[0].id;
    for n in 0..8 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.a.save_task(&task).unwrap();
    }
    f.sync_a(Where::Drive);
    let files = f.files(Where::Both);
    sync_in_raw(&mut f.a, files);
    let guard = dir.join("c.sqlite3-guarda.ponte");
    assert!(guard.is_file(), "the forked library's guard copy");
    let guard_bytes = std::fs::read(&guard).unwrap();
    let revision = forked.load_intranet_config().unwrap().synced_revision;
    let blocker = crate::sharedfolder::aside_path(&guard).unwrap();
    std::fs::create_dir(&blocker).unwrap();
    let cfg = forked.load_intranet_config().unwrap();
    let error = forked
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        error.contains("Nada foi gravado"),
        "the exchange went on: {error:?}"
    );
    assert_eq!(
        std::fs::read(&guard).unwrap(),
        guard_bytes,
        "guard untouched"
    );
    assert_eq!(
        forked.load_intranet_config().unwrap().synced_revision,
        revision,
        "revision advanced"
    );
    std::fs::remove_dir(&blocker).unwrap();
    let report = sync_raw(&mut forked);
    assert!(blocker.is_file(), "guard kept aside: {:?}", report.notes);
    let kept = crate::sharedfolder::read(&blocker, KEY).unwrap().unwrap();
    assert!(kept
        .payload
        .snapshot
        .captures
        .iter()
        .any(|c| c.capture.document_number == number("c", 1)));
}

/// Review 7: A corrects the reference of an atendimento that hangs from no
/// document and publishes; B adopts it. Nothing is reported missing and no
/// copy is kept aside (the atendimento is the same one, by number and
/// creation time, in the same numbering).
#[test]
fn f0r_an_edited_atendimento_reference_is_no_loss() {
    let mut p = pair();
    let mut att = crate::saiop::SaiopAttendance::new_manual();
    att.reference_raw = "PROC-ANTIGA".into();
    att.reference_norm = crate::saiop::normalize_reference(&att.reference_raw);
    att.subject = "Atendimento avulso".into();
    let id = p.a.save_saiop_attendance(&att, None).unwrap();
    sync(&mut p.a);
    sync(&mut p.b);
    let mut edited =
        p.a.list_saiop_attendances()
            .unwrap()
            .into_iter()
            .find(|a| a.id == id)
            .unwrap();
    edited.reference_raw = "PROC-CORRIGIDA".into();
    edited.reference_norm = crate::saiop::normalize_reference(&edited.reference_raw);
    p.a.save_saiop_attendance(&edited, None).unwrap();
    sync(&mut p.a);
    // `sync` fails on any loss notice.
    sync(&mut p.b);
    sync(&mut p.a);
    assert!(p
        .b
        .list_saiop_attendances()
        .unwrap()
        .iter()
        .any(|a| a.reference_raw == "PROC-CORRIGIDA"));
    let backups = p._dir.path().join("copias-antes-da-substituicao");
    assert!(!backups.exists(), "a copy of a library was kept");
}

/// Review 7: A and B each make a manual record offline and both get the
/// same provisional number; A renames its record's automatic triage task and
/// publishes first. B's record is replayed under another number, with a new
/// triage task: B's own triage is in the result (made again by that replay),
/// so nothing is reported missing.
#[test]
fn f0r_a_triage_task_made_again_by_a_replay_is_no_loss() {
    let mut p = pair();
    let fields = |title: &str| crate::manual::ManualFields {
        kind: "ligacao".into(),
        title: title.into(),
        origin: "Gabinete".into(),
        document_date: String::new(),
        text: format!("Texto de {title}"),
    };
    let a_doc =
        p.a.create_manual_document(&fields("Registro de A"))
            .unwrap()
            .document_id;
    let b_doc =
        p.b.create_manual_document(&fields("Registro de B"))
            .unwrap()
            .document_id;
    let mut triage =
        p.a.list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.document_id == Some(a_doc) && t.action == "triar")
            .unwrap();
    triage.title = "Triagem renomeada por A".into();
    p.a.save_task(&triage).unwrap();
    sync(&mut p.a);
    let _ = b_doc;
    // `sync` fails on any loss notice.
    sync(&mut p.b);
    sync(&mut p.a);
    assert!(p
        .b
        .list_documents("")
        .unwrap()
        .iter()
        .any(|d| d.title == "Registro de B"));
    let backups = p._dir.path().join("copias-antes-da-substituicao");
    assert!(!backups.exists(), "a copy of a library was kept");
}

/// Review 8 (measurement, run by hand with `--ignored --nocapture`): the
/// cost of checking this library's items against a result, by a whole
/// export (0.51 F0 first round) and by the narrow projection now used, on a
/// generated library of 5,000 documents and 20,000 tasks.
#[test]
#[ignore = "medição de custo (revisão F0, item 8)"]
fn f0r_measure_the_cost_of_checking_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("grande.sqlite3")).unwrap();
    let started = std::time::Instant::now();
    // Written straight into the tables, in one transaction (the import path
    // is far too slow for this many in a test build); the rows are what the
    // import writes: document, sealed version, request, and sealed tasks.
    {
        let tx = store.conn.transaction().unwrap();
        for n in 0..5_000u32 {
            let mut c = capture(
                &format!("SEFAZ-PRO-2026/{:05}", 10_000 + n),
                &format!(
                    "Texto do documento {n}. {}",
                    "Conteúdo de exemplo. ".repeat(40)
                ),
            );
            c.request_id = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO documents(identity_hash,last_seen_at) VALUES(?1,?2)",
                rusqlite::params![
                    crate::storage::identity_hash_for_number(&c.document_number),
                    c.captured_at
                ],
            )
            .unwrap();
            let doc = tx.last_insert_rowid();
            let payload = crate::protection::seal(&serde_json::to_vec(&c).unwrap()).unwrap();
            tx.execute(
                "INSERT INTO captures(document_id,content_hash,payload,created_at) VALUES(?1,?2,?3,?4)",
                rusqlite::params![doc, uuid::Uuid::new_v4().as_bytes().to_vec(), payload, c.captured_at],
            )
            .unwrap();
            let cap = tx.last_insert_rowid();
            tx.execute(
                "UPDATE documents SET latest_capture_id=?1 WHERE id=?2",
                [cap, doc],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO requests(request_id,document_id,capture_id) VALUES(?1,?2,?3)",
                rusqlite::params![c.request_id, doc, cap],
            )
            .unwrap();
            for k in 0..4 {
                let mut t = crate::tasks::Task::new(doc);
                t.title = format!("Tarefa {k} do documento {n}");
                let payload = crate::protection::seal(&serde_json::to_vec(&t).unwrap()).unwrap();
                tx.execute(
                    "INSERT INTO tasks(document_id,payload,uid) VALUES(?1,?2,?3)",
                    rusqlite::params![doc, payload, t.uid],
                )
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    let documents: Vec<i64> = store
        .list_documents("")
        .unwrap()
        .iter()
        .map(|d| d.id)
        .collect();
    let tasks = store.list_tasks().unwrap().len();
    eprintln!(
        "F0 cost: library of {} documents and {tasks} tasks built in {:?}",
        documents.len(),
        started.elapsed()
    );
    let snapshot = store.export_snapshot().unwrap();
    let result = crate::coverage::ResultIndex::of(&snapshot);
    let check = crate::coverage::Check {
        result: &result,
        explained: &[],
        same_numbering: true,
        replayed: None,
    };
    let exported_bytes = serde_json::to_vec(&snapshot).unwrap().len();
    drop(snapshot);
    for round in 0..2 {
        let t = std::time::Instant::now();
        let whole = store.export_snapshot().unwrap();
        let lost_old =
            crate::coverage::dropped(&crate::coverage::SnapshotHolder(&whole), &check).unwrap();
        let old = t.elapsed();
        drop(whole);
        let t = std::time::Instant::now();
        let holder = crate::coverage::LibraryHolder::new(&store.conn).unwrap();
        let lost_new = crate::coverage::dropped(&holder, &check).unwrap();
        let new = t.elapsed();
        assert!(lost_old.is_empty() && lost_new.is_empty());
        eprintln!("F0 cost round {round}: whole export + check {old:?}; narrow projection {new:?}");
    }
    eprintln!("F0 cost: the whole export of this library is {exported_bytes} bytes as JSON");
}

// ------------------------------------- F0, review round 2 (rodada-2) --

/// Review 2.1: a document has its automatic triage task and B adds a second
/// task with action "triar" (another uid) and publishes it to the pen drive
/// only; B keeps working there until that creation leaves the journal. A
/// moves the Drive on without it. When B comes back to both, nothing replays
/// the second task and the result lacks it: B keeps a copy aside and names
/// it, instead of taking it for the automatic triage task.
#[test]
fn f0r2_a_second_triage_task_is_not_taken_for_the_automatic_one() {
    crate::sharedfolder::set_journal_cap_for_test(3);
    let mut f = Folders::new();
    let doc = f.b.list_documents("").unwrap()[0].id;
    let mut extra = crate::tasks::Task::new(doc);
    extra.title = "Segunda triagem de B".into();
    extra.action = "triar".into();
    f.b.save_task(&extra).unwrap();
    assert_eq!(
        f.b.list_tasks()
            .unwrap()
            .iter()
            .filter(|t| t.document_id == Some(doc) && t.action == "triar")
            .count(),
        2
    );
    f.sync_b(Where::Pen);
    for n in 0..5 {
        let mut task = crate::tasks::Task::standalone();
        task.title = format!("trabalho de B {n}");
        f.b.save_task(&task).unwrap();
        f.sync_b(Where::Pen);
    }
    for n in 0..8 {
        let mut task = crate::tasks::Task::new(doc);
        task.title = format!("enchimento {n}");
        f.a.save_task(&task).unwrap();
    }
    f.sync_a(Where::Drive);
    let files = f.files(Where::Both);
    let report = sync_in_raw(&mut f.b, files);
    let named = report
        .notes
        .iter()
        .any(|n| n.contains("Segunda triagem de B"));
    let still_here = task_by_title(&f.b, "Segunda triagem de B").is_some();
    assert!(
        named || still_here,
        "the second triage task vanished unnamed: {:?}",
        report.notes
    );
    crate::sharedfolder::set_journal_cap_for_test(crate::sharedfolder::MAX_JOURNAL_ENTRIES);
}
