//! F2a (Frente F), review item F2A-08: behaviour unchanged, proved against
//! the build without F2a (0.51 final, `f0-consertos` fc6342e).
//!
//! This module uses only what that build already has (`f1_inventory::fill`
//! made `pub(crate)` there too), so the same file runs on both builds:
//!
//! - `f2a_08_write_fixtures` (ignored, with `F2A_PARIDADE` = a folder and
//!   `F2A_NOME` = `0.51` or `22`) writes, for a scripted library that makes
//!   every change variant, the shared file it publishes
//!   (`paridade-<nome>.ponte`, format 8) and its payload, normalized
//!   (`paridade-<nome>.json`);
//! - `f2a_08_payload_is_the_one_of_0_51` runs the same script here and
//!   compares the normalized payload with `tests/fixtures/paridade-0.51.json`
//!   (written by fc6342e);
//! - `f2a_08_a_library_takes_the_0_51_file_and_answers_in_format_8` and
//!   `f2a_08_a_library_takes_the_22_file_and_answers_in_format_8` join a new
//!   library to the file the OTHER build wrote, adopt it, change it and
//!   publish again: run on each build, they are the exchange in both
//!   directions (a 21 library on fc6342e, a 22 library on this branch).
#![allow(clippy::all)]

use crate::intranet::{IntranetConfig, IntranetMode, IntranetMutation};
use crate::storage::Store;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) const KEY: &str = "senha-de-teste-123";

fn folder_config(member: &str, files: Vec<String>) -> IntranetConfig {
    IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: files,
        folder_key: KEY.into(),
        ..IntranetConfig::default()
    }
}

fn sync(store: &mut Store) {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap();
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

fn one(store: &Store, sql: &str) -> i64 {
    store
        .conn
        .query_row(sql, [], |r| r.get::<_, i64>(0))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn text(store: &Store, sql: &str) -> String {
    store
        .conn
        .query_row(sql, [], |r| r.get::<_, String>(0))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn doc(store: &Store, number: &str) -> i64 {
    store
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .unwrap_or_else(|| panic!("sem {number}"))
        .id
}

fn task(store: &Store, title: &str) -> i64 {
    let all: Vec<(i64, Vec<u8>)> = store
        .conn
        .prepare("SELECT id,payload FROM tasks ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for (id, blob) in all {
        let plain = crate::protection::unseal(&blob).unwrap();
        let value: Value = serde_json::from_slice(&plain).unwrap();
        if value["title"] == title {
            return id;
        }
    }
    panic!("sem a tarefa {title}")
}

/// Every one of the variants of `IntranetMutation`, made through the
/// product in shared-folder mode (each change applied here and queued):
/// `f1_inventory::fill` (21 of them) and the other 22.
pub(crate) fn every_variant(store: &mut Store, tag: &str) {
    use crate::attachments::Owner;
    use crate::trash::{Kind, Target};
    crate::f1_inventory::fill(store, tag);
    let doc1 = doc(store, &number(tag, 1));
    let doc2 = doc(store, &number(tag, 2));
    let task_doc = task(store, &format!("T-{tag}-doc"));
    let alone = task(store, &format!("T-{tag}-alone"));
    let doomed = task(store, &format!("T-{tag}-trash"));
    // RelinkTask, SetLinkActive.
    store.relink_task(alone, Some(doc2), None).unwrap();
    let link = one(
        store,
        &format!("SELECT id FROM document_links WHERE source_id={doc1} AND target_id={doc2}"),
    );
    store.set_link_active(link, false).unwrap();
    // UpdateAttachment, ExtractAttachmentText, ReviewAttachmentText.
    let of_document = store.list_attachments(Owner::Document(doc1)).unwrap()[0].id;
    store
        .update_attachment_description(of_document, &format!("descrição nova {tag}"))
        .unwrap();
    // (Text is extracted only from the files of template examples.)
    let example_row = one(store, "SELECT max(id) FROM template_examples");
    let plain = store
        .add_attachment(
            Owner::Example(example_row),
            &format!("texto-{tag}.txt"),
            "",
            format!("Texto simples do anexo {tag} para extrair.").as_bytes(),
        )
        .unwrap();
    store.extract_attachment_text(plain).unwrap();
    store
        .review_attachment_text(plain, &format!("Texto conferido {tag}."))
        .unwrap();
    // CreateTaskForSaiop, DeleteSaiopAttendance, UndoSaiopBatch.
    let attendance = one(
        store,
        &format!("SELECT min(id) FROM saiop_attendances WHERE document_id={doc1}"),
    );
    store
        .create_task_for_saiop(attendance, &format!("Retorno {tag}"), "normal", "", "")
        .unwrap();
    let mut spare = crate::saiop::SaiopAttendance::new_manual();
    spare.reference_raw = format!("AVULSO-{tag}");
    spare.reference_norm = crate::saiop::normalize_reference(&spare.reference_raw);
    spare.subject = format!("Avulso-{tag}");
    let spare = store.save_saiop_attendance(&spare, None).unwrap();
    store.delete_saiop_attendance(spare).unwrap();
    let batch = one(store, "SELECT max(id) FROM saiop_batches");
    store.undo_saiop_batch(batch).unwrap();
    // SetTemplateStatus, SetBlockStatus, DeleteExample, DeleteTemplate,
    // DeleteBlock.
    let template = text(
        store,
        "SELECT uid FROM work_templates ORDER BY id DESC LIMIT 1",
    );
    let block = text(
        store,
        "SELECT uid FROM instruction_blocks ORDER BY id DESC LIMIT 1",
    );
    let example = text(
        store,
        "SELECT uid FROM template_examples ORDER BY id DESC LIMIT 1",
    );
    store
        .set_template_status(&template, crate::work_templates::Status::Published, None)
        .unwrap();
    store
        .set_block_status(&block, crate::work_templates::Status::Archived, None)
        .unwrap();
    store.delete_example(&example).unwrap();
    store.delete_template(&template).unwrap();
    store.delete_block(&block).unwrap();
    // DeleteResourceUse, DeleteResource, DeleteCategory, DeleteProject, of
    // items made for that (deleting the ones `fill` saved would merge their
    // saves into the deletes in the queue, as the product does).
    let spare_resource = store
        .save_resource(
            &crate::resources::Resource {
                name: format!("Descartável {tag}"),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let kept_resource = text(store, "SELECT uid FROM resources ORDER BY rowid LIMIT 1");
    let spare_use = store
        .save_resource_use(
            &crate::resources::ResourceUse {
                resource_uid: kept_resource,
                who: format!("Descartável {tag}"),
                start: "2099-02-05T09:00".into(),
                end: "2099-02-05T10:00".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let spare_category = store
        .save_category(&crate::categories::Category::new(
            &format!("Descartável {tag}"),
            2,
        ))
        .unwrap()
        .uid;
    let spare_project = store
        .save_project(&crate::projects::Project::new(&format!(
            "Descartável {tag}"
        )))
        .unwrap()
        .uid;
    store.delete_resource_use(&spare_use, None).unwrap();
    store.delete_resource(&spare_resource, None).unwrap();
    store
        .delete_category_checked(&spare_category, None)
        .unwrap();
    store.delete_project_checked(&spare_project, None).unwrap();
    // SaveSetting.
    store.set_library_setting("holidays", "uf SP\n").unwrap();
    // RestoreItem, TrashItem again, PurgeTrash.
    let doomed = || Target {
        kind: Kind::Task,
        id: doomed,
    };
    store.restore_item(doomed()).unwrap();
    store.trash_item(doomed()).unwrap();
    store.purge_trash_item(doomed()).unwrap();
    // DeleteAttachment.
    let of_task = store.list_attachments(Owner::Task(task_doc)).unwrap()[0].id;
    store.delete_attachment(of_task).unwrap();
    // DeleteDocument (of a document made for it).
    let mut extra = crate::model::sample_capture();
    extra.document_number = number(tag, 9);
    extra.title = format!("Documento {}", number(tag, 9));
    extra.text = format!("Documento para apagar {tag}.");
    extra.source_url = format!(
        "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla={}",
        number(tag, 9).replace('/', "%2F")
    );
    let gone = store.import_capture(&extra).unwrap().document_id;
    store.delete_document(gone).unwrap();
}

/// The name of each variant, by an exhaustive match: a variant added to
/// the enum fails to compile here until it is named.
pub(crate) fn variant_name(mutation: &IntranetMutation) -> &'static str {
    use IntranetMutation as M;
    match mutation {
        M::ImportCapture { .. } => "ImportCapture",
        M::ImportBundle { .. } => "ImportBundle",
        M::SaveNote { .. } => "SaveNote",
        M::SaveDraft { .. } => "SaveDraft",
        M::SaveAnalysis { .. } => "SaveAnalysis",
        M::SaveTask { .. } => "SaveTask",
        M::RelinkTask { .. } => "RelinkTask",
        M::SaveWorkContext { .. } => "SaveWorkContext",
        M::SavePageText { .. } => "SavePageText",
        M::AddDocumentLink { .. } => "AddDocumentLink",
        M::SetLinkActive { .. } => "SetLinkActive",
        M::TrashItem { .. } => "TrashItem",
        M::RestoreItem { .. } => "RestoreItem",
        M::DeleteDocument { .. } => "DeleteDocument",
        M::SaveSaiopAttendance { .. } => "SaveSaiopAttendance",
        M::ImportSaiopBatch { .. } => "ImportSaiopBatch",
        M::UndoSaiopBatch { .. } => "UndoSaiopBatch",
        M::DeleteSaiopAttendance { .. } => "DeleteSaiopAttendance",
        M::CreateTaskForSaiop { .. } => "CreateTaskForSaiop",
        M::AddAttachment { .. } => "AddAttachment",
        M::UpdateAttachment { .. } => "UpdateAttachment",
        M::DeleteAttachment { .. } => "DeleteAttachment",
        M::PurgeTrash { .. } => "PurgeTrash",
        M::SaveTemplate { .. } => "SaveTemplate",
        M::SetTemplateStatus { .. } => "SetTemplateStatus",
        M::DeleteTemplate { .. } => "DeleteTemplate",
        M::SaveInstructionBlock { .. } => "SaveInstructionBlock",
        M::SetBlockStatus { .. } => "SetBlockStatus",
        M::DeleteBlock { .. } => "DeleteBlock",
        M::SaveExample { .. } => "SaveExample",
        M::DeleteExample { .. } => "DeleteExample",
        M::RecordGeneration { .. } => "RecordGeneration",
        M::ExtractAttachmentText { .. } => "ExtractAttachmentText",
        M::ReviewAttachmentText { .. } => "ReviewAttachmentText",
        M::SaveResource { .. } => "SaveResource",
        M::DeleteResource { .. } => "DeleteResource",
        M::SaveResourceUse { .. } => "SaveResourceUse",
        M::DeleteResourceUse { .. } => "DeleteResourceUse",
        M::SaveCategory { .. } => "SaveCategory",
        M::DeleteCategory { .. } => "DeleteCategory",
        M::SaveProject { .. } => "SaveProject",
        M::DeleteProject { .. } => "DeleteProject",
        M::SaveSetting { .. } => "SaveSetting",
    }
}

/// The number of variants `pub enum IntranetMutation` declares, read from
/// its source (not from a list kept by hand).
pub(crate) fn enum_variant_count() -> usize {
    let source = include_str!("intranet.rs");
    let start = source
        .find("pub enum IntranetMutation {")
        .expect("enum IntranetMutation");
    let body = &source[start..];
    let end = body.find("\n}\n").expect("fim do enum");
    body[..end]
        .lines()
        .filter(|line| {
            let rest = line.strip_prefix("    ").unwrap_or("");
            rest.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && rest
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .is_some_and(|word| !word.is_empty())
        })
        .count()
}

/// `value` with what differs from run to run (moments, random ids, the
/// writer's identity) replaced by stable marks: moments by `<tempo>`, each
/// random id by `<id-N>` in the order it first appears (the same id gets
/// the same mark, so links between items stay visible).
pub(crate) fn normalized(value: &Value) -> Value {
    fn walk(value: &Value, ids: &mut BTreeMap<String, String>) -> Value {
        match value {
            Value::String(s) => Value::String(mark(s, ids)),
            Value::Array(items) => {
                let mut items: Vec<Value> = items.iter().map(|v| walk(v, ids)).collect();
                // Rows keyed by the digest of a random uid (`item_versions`
                // of uid-addressed kinds) come in the order of that digest:
                // they are compared as a set.
                if items.iter().any(|v| v.to_string().contains("<chave>")) {
                    items.sort_by_key(|v| v.to_string());
                }
                Value::Array(items)
            }
            Value::Number(n) if n.as_i64().is_some_and(|n| n.unsigned_abs() >= 1 << 32) => {
                // `work_templates::version_key` of a random uid.
                Value::String("<chave>".into())
            }
            Value::Object(map) => {
                Value::Object(
                    map.iter()
                        .map(|(k, v)| {
                            let mut v = walk(v, ids);
                            // Exported in the order of their random ids
                            // (`ORDER BY request_id`): compared as a set.
                            if k == "requests" {
                                if let Value::Array(items) = &mut v {
                                    items.sort_by_key(|i| i.to_string());
                                }
                            }
                            (k.clone(), v)
                        })
                        .collect(),
                )
            }
            other => other.clone(),
        }
    }
    fn mark(s: &str, ids: &mut BTreeMap<String, String>) -> String {
        let moment = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}").unwrap();
        let today = {
            let now = chrono::Utc::now().date_naive();
            [-2i64, -1, 0, 1, 2].map(|d| (now + chrono::Duration::days(d)).to_string())
        };
        let random = regex::Regex::new(
            r"^([0-9a-f]{32}|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$",
        )
        .unwrap();
        if moment.is_match(s) {
            "<tempo>".into()
        } else if today.iter().any(|d| s == d) {
            "<data>".into()
        } else if random.is_match(s) {
            let next = format!("<id-{}>", ids.len() + 1);
            ids.entry(s.to_owned()).or_insert(next).clone()
        } else {
            s.to_owned()
        }
    }
    walk(value, &mut BTreeMap::new())
}

/// The payload of the shared file at `path`, normalized: snapshot, applied
/// uids, journal, and the files carried (by name and SHA-256).
pub(crate) fn payload_of(path: &Path) -> Value {
    let copy = crate::sharedfolder::read(path, KEY).unwrap().unwrap();
    assert_eq!(
        (copy.header.format, copy.header.schema_version),
        (8, 21),
        "{}",
        path.display()
    );
    let digest = |bytes: &[u8]| {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(bytes))
    };
    let blobs: Vec<(String, i64, String)> = copy
        .blobs
        .pdfs
        .iter()
        .map(|(id, b)| ("pdf".to_owned(), *id, digest(b)))
        .chain(
            copy.blobs
                .attachments
                .iter()
                .map(|(id, b)| ("anexo".to_owned(), *id, digest(b))),
        )
        .collect();
    normalized(&serde_json::json!({
        "snapshot": serde_json::to_value(&copy.payload.snapshot).unwrap(),
        "applied": copy.payload.applied.len(),
        "journal": serde_json::to_value(&copy.payload.journal).unwrap(),
        "blobs": blobs,
    }))
}

/// The snapshot of the shared file at `path`, alone, normalized (marks
/// numbered within the snapshot).
pub(crate) fn snapshot_of(path: &Path) -> Value {
    let copy = crate::sharedfolder::read(path, KEY).unwrap().unwrap();
    normalized(&serde_json::to_value(&copy.payload.snapshot).unwrap())
}

/// A library that made every variant and published them to a new shared
/// file at `file`.
pub(crate) fn scripted_file(dir: &Path, file: &Path) -> Store {
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let mut store = Store::open(&dir.join("roteiro.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config("Roteiro", vec![file.display().to_string()]))
        .unwrap();
    every_variant(&mut store, "a");
    sync(&mut store);
    store
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Writes the fixtures of this build (see the module): run with
/// `--ignored`, `F2A_PARIDADE` and `F2A_NOME`.
#[test]
#[ignore]
fn f2a_08_write_fixtures() {
    let out = PathBuf::from(std::env::var("F2A_PARIDADE").expect("F2A_PARIDADE"));
    let name = std::env::var("F2A_NOME").expect("F2A_NOME");
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    scripted_file(dir.path(), &file);
    std::fs::create_dir_all(&out).unwrap();
    std::fs::copy(&file, out.join(format!("paridade-{name}.ponte"))).unwrap();
    std::fs::write(
        out.join(format!("paridade-{name}.json")),
        serde_json::to_string_pretty(&payload_of(&file)).unwrap() + "\n",
    )
    .unwrap();
}

/// The same script, here: the file it publishes carries the payload the
/// build without F2a published (`paridade-0.51.json`, written by fc6342e).
#[test]
fn f2a_08_payload_is_the_one_of_0_51() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    scripted_file(dir.path(), &file);
    let here = payload_of(&file);
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture("paridade-0.51.json")).unwrap())
            .unwrap();
    if here != golden {
        let a = serde_json::to_string_pretty(&here).unwrap();
        let b = serde_json::to_string_pretty(&golden).unwrap();
        let first = a
            .lines()
            .zip(b.lines())
            .position(|(x, y)| x != y)
            .unwrap_or(0);
        panic!(
            "o conteúdo difere do de 0.51 na linha {first}:\naqui:  {:?}\n0.51:  {:?}",
            a.lines().nth(first),
            b.lines().nth(first)
        );
    }
}

/// A new library joins the folder through the file written by `writer`,
/// adopts it, changes it and publishes again in format 8; the change is in
/// the file it wrote. Answers that file.
fn joins_and_answers(fixture_name: &str, member: &str) -> (tempfile::TempDir, PathBuf, Value) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::copy(fixture(fixture_name), &file).unwrap();
    let taken = snapshot_of(&file);
    let mut store = Store::open(&dir.path().join("novo.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config(member, vec![file.display().to_string()]))
        .unwrap();
    sync(&mut store);
    // Everything of the file was adopted.
    let adopted = normalized(&serde_json::to_value(store.export_snapshot().unwrap()).unwrap());
    assert_eq!(adopted["documents"], taken["documents"], "documentos");
    assert_eq!(adopted["tasks"], taken["tasks"], "tarefas");
    assert_eq!(adopted, taken, "o retrato adotado");
    let doc1 = doc(&store, &number("a", 1));
    let note = format!("Nota de {member}");
    store.save_note(doc1, &note).unwrap();
    let mut new = crate::tasks::Task::new(doc1);
    new.title = format!("Tarefa de {member}");
    store.save_task(&new).unwrap();
    sync(&mut store);
    let answered = payload_of(&file);
    let text = serde_json::to_string(&answered).unwrap();
    assert!(
        text.contains(&format!("Tarefa de {member}")),
        "a tarefa não foi publicada"
    );
    assert_eq!(store.load_document(doc1).unwrap().note, note);
    (dir, file, answered)
}

/// The file the build without F2a published (fc6342e, a library of schema
/// 21), taken by this build.
#[test]
fn f2a_08_a_library_takes_the_0_51_file_and_answers_in_format_8() {
    let (_dir, file, _) = joins_and_answers("paridade-0.51.ponte", "Acervo 22");
    // Saved for the other direction: fc6342e takes this one.
    if let Ok(out) = std::env::var("F2A_PARIDADE") {
        std::fs::copy(&file, PathBuf::from(out).join("paridade-22.ponte")).unwrap();
    }
}

/// The file this build published after taking the one of fc6342e
/// (`paridade-22.ponte`), taken by a new library; run on fc6342e, a library
/// of schema 21 takes the file of a library of schema 22.
#[test]
fn f2a_08_a_library_takes_the_22_file_and_answers_in_format_8() {
    let (_dir, _, answered) = joins_and_answers("paridade-22.ponte", "Acervo 21");
    let text = serde_json::to_string(&answered).unwrap();
    assert!(
        text.contains("Tarefa de Acervo 22"),
        "a tarefa do acervo 22 se perdeu"
    );
}

/// The script makes every variant of `IntranetMutation` (counted from the
/// enum's source), each queued through the product.
#[test]
fn f2a_08_the_script_makes_every_variant() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config("Roteiro", vec![drive.display().to_string()]))
        .unwrap();
    every_variant(&mut store, "a");
    let made: std::collections::BTreeSet<&str> = store
        .list_offline_outbox()
        .unwrap()
        .iter()
        .map(|entry| variant_name(&entry.mutation))
        .collect();
    assert_eq!(enum_variant_count(), 43);
    assert_eq!(made.len(), enum_variant_count(), "{made:?}");
}
