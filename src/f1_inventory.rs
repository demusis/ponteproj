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

fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
}

fn sync_in(store: &mut Store, files: Vec<String>) -> crate::intranet::FolderSyncReport {
    let mut cfg = store.load_intranet_config().unwrap();
    cfg.folder_files = files;
    store.save_intranet_config(&cfg).unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
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

/// C4: a task that reaches a library without uid (a copy written before
/// task uids) gets a RANDOM one at adoption: two devices adopting the same
/// copy give it two different uids.
#[test]
fn c4_a_task_without_uid_gets_a_different_uid_on_each_device() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = Store::open(&dir.path().join("s.sqlite3")).unwrap();
    let doc = source
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let mut task = crate::tasks::Task::new(doc);
    task.title = "Tarefa antiga".into();
    source.save_task(&task).unwrap();
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
        task_by_title(&store, "Tarefa antiga").unwrap()
    };
    let (one_, two) = (adopt("1.sqlite3"), adopt("2.sqlite3"));
    assert_eq!(one_.id, two.id, "same row id");
    assert_ne!(one_.uid, two.uid, "different uids for the same task");
}

// ------------------------------------------------- IdMap coverage (unit) --

/// What `IdMap::remap` translates inside the AI sources of a change: the
/// numeric fields and the names `C{capture}` / `C{capture}F{file}P{page}`.
/// An attachment source `A{attachment}H{hash}` keeps the old attachment id,
/// and a fact's `source_id` keeps the old source name.
#[test]
#[ignore = "R2-B2/inventário: IdMap não traduz A{id}H nem Fact.source_id (defeito do código atual)"]
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

/// R2-B3 against 0.50, versions: the current version is a pointer
/// (`latest_capture_id`), set by whichever import is APPLIED last. B imports
/// an older version offline, A a newer one and publishes first: B's import is
/// replayed after A's, so the OLDER version becomes current, the same on
/// both devices. Then trashing the current version falls back to the highest
/// id (ORDER BY id DESC), again the same on both.
#[test]
fn r2_b3_the_current_version_follows_the_order_of_arrival_but_is_the_same_everywhere() {
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
        older.text,
        "arrival order elects the older capture"
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
#[ignore = "R2-B4: espelho perdedor sobrescrito sem cópia guardada (defeito do código atual)"]
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
    let report = f.sync_a(Where::Both);
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
#[ignore = "R2-B7: apagar de vez grava a versão depois de apagar o alvo (defeito do código atual)"]
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
}

/// R2-B7, refused change marked applied: B's note goes only to the pen
/// drive; A's conflicting note goes to the Drive, which becomes the newest
/// copy. A merges both: B's note is refused (version conflict), becomes a
/// card on A only, and its uid is recorded as applied. B never learns: its
/// queue was emptied when it wrote the pen drive, and the card lives on A.
#[test]
#[ignore = "R2-B7: recusa vira cartão só em quem juntou e é marcada aplicada; a origem nunca sabe (defeito do código atual)"]
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
#[ignore = "R2-B8: retomada de --import-plan por request: num alvo ambíguo (defeito do código atual)"]
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
