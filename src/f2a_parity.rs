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

/// The content of the shared file at `path`, raw: the snapshot and the
/// journal as they are; `applied` as the operation each applied uid stands
/// for in the journal (`variant|label`, sorted; a uid without its entry is
/// `<fora do diário>`); the files carried, by id and SHA-256. Checked here:
/// the file is format 8 with content schema 21, and the exporter's order of
/// `requests` (by `request_id`) and of `item_versions` (by kind and item)
/// holds.
pub(crate) fn content_of(path: &Path) -> Value {
    use sha2::Digest;
    let copy = crate::sharedfolder::read(path, KEY).unwrap().unwrap();
    assert_eq!(
        (copy.header.format, copy.header.schema_version),
        (8, 21),
        "{}",
        path.display()
    );
    let by_uid: BTreeMap<&str, String> = copy
        .payload
        .journal
        .iter()
        .map(|e| {
            (
                e.uid.as_str(),
                format!("{}|{}", variant_name(&e.mutation), e.label),
            )
        })
        .collect();
    let mut applied: Vec<String> = copy
        .payload
        .applied
        .iter()
        .map(|uid| {
            by_uid
                .get(uid.as_str())
                .cloned()
                .unwrap_or_else(|| "<fora do diário>".into())
        })
        .collect();
    applied.sort();
    let digest = |bytes: &[u8]| format!("{:x}", sha2::Sha256::digest(bytes));
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
    let snapshot = serde_json::to_value(&copy.payload.snapshot).unwrap();
    let requests: Vec<String> = snapshot["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["request_id"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        requests.windows(2).all(|w| w[0] <= w[1]),
        "pedidos fora da ordem de request_id"
    );
    let versions: Vec<(String, i64)> = snapshot["item_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v[0].as_str().unwrap().to_owned(), v[1].as_i64().unwrap()))
        .collect();
    assert!(
        versions.windows(2).all(|w| w[0] <= w[1]),
        "versões fora da ordem de tipo e item"
    );
    serde_json::json!({
        "applied": applied,
        "blobs": blobs,
        "journal": serde_json::to_value(&copy.payload.journal).unwrap(),
        "snapshot": snapshot,
    })
}

fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        Value::Object(map) => map.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// `content` made comparable across runs and builds, without hiding what
/// does not vary:
///
/// - a derived key (`work_templates::version_key` of a uid, the item of a
///   uid-addressed kind in `item_versions`) is checked against its uid and
///   written as `<chave:UID>`; any other number is kept exactly;
/// - `requests` and `item_versions`, exported in the order of random values
///   (the request ids, the derived keys) whose order rule `content_of`
///   checked, are put in a canonical order that keeps every relation they
///   encode: requests by document, version and the order their ids first
///   appear in the content; versions by kind and the order their uid (or
///   number) first appears;
/// - only the strings that differ between `content` and `other` (the same
///   script run again by the same build) are masked: a random id by `<v-N>`,
///   numbered by first appearance, the same value always by the same mark
///   (so every link between items stays visible); a moment of the run
///   ("now") by `<agora>`, whose equality to another one is only timing. Every other
///   string, a fixed moment of the script included, is compared exactly.
pub(crate) fn comparable(content: &Value, other: &Value) -> Value {
    fn tie_keys(value: &Value, keys: &BTreeMap<i64, String>) -> Value {
        match value {
            Value::Number(n) => match n.as_i64().and_then(|n| keys.get(&n)) {
                Some(uid) => Value::String(format!("<chave:{uid}>")),
                None => value.clone(),
            },
            Value::Array(items) => Value::Array(items.iter().map(|v| tie_keys(v, keys)).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), tie_keys(v, keys)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    fn prepared(content: &Value) -> Value {
        let mut all = Vec::new();
        strings(content, &mut all);
        let keys: BTreeMap<i64, String> = all
            .iter()
            .map(|s| (crate::work_templates::version_key(s), s.clone()))
            .collect();
        let mut value = content.clone();
        // Only `item_versions` holds items by derived key.
        let versions = tie_keys(&value["snapshot"]["item_versions"], &keys);
        for entry in versions.as_array().unwrap() {
            if let Some(n) = entry[1].as_i64() {
                assert!(
                    n < 1 << 32,
                    "chave {n} de {} que não é de nenhum uid do conteúdo",
                    entry[0]
                );
            }
        }
        value["snapshot"]["item_versions"] = versions;
        // Canonical orders, by where things first appear elsewhere.
        let mut rest = value.clone();
        rest["snapshot"]["item_versions"] = Value::Null;
        rest["snapshot"]["requests"] = Value::Null;
        let mut order = Vec::new();
        strings(&rest, &mut order);
        let place = |s: &str| order.iter().position(|o| o == s).unwrap_or(usize::MAX);
        let mut requests = value["snapshot"]["requests"].as_array().unwrap().clone();
        requests.sort_by_key(|r| {
            (
                r["document_id"].as_i64(),
                r["capture_id"].as_i64(),
                place(r["request_id"].as_str().unwrap_or("")),
            )
        });
        value["snapshot"]["requests"] = Value::Array(requests);
        let mut versions = value["snapshot"]["item_versions"]
            .as_array()
            .unwrap()
            .clone();
        versions.sort_by_key(|v| {
            let item = match &v[1] {
                Value::String(s) => {
                    let uid = s.trim_start_matches("<chave:").trim_end_matches('>');
                    (1, place(uid) as i64)
                }
                other => (0, other.as_i64().unwrap_or(0)),
            };
            (v[0].as_str().unwrap_or("").to_owned(), item)
        });
        value["snapshot"]["item_versions"] = Value::Array(versions);
        value
    }
    fn volatile(a: &Value, b: &Value, out: &mut std::collections::BTreeSet<String>) {
        match (a, b) {
            (Value::String(x), Value::String(y)) if x != y => {
                let inner = |s: &str| {
                    s.strip_prefix("<chave:")
                        .and_then(|s| s.strip_suffix('>'))
                        .map(str::to_owned)
                };
                match (inner(x), inner(y)) {
                    (Some(x), Some(y)) if x != y => {
                        out.insert(x);
                    }
                    _ => {
                        out.insert(x.clone());
                    }
                }
            }
            (Value::Array(x), Value::Array(y)) => {
                assert_eq!(x.len(), y.len(), "as duas execuções diferem na forma");
                x.iter().zip(y).for_each(|(x, y)| volatile(x, y, out));
            }
            (Value::Object(x), Value::Object(y)) => {
                for (k, v) in x {
                    volatile(v, &y[k], out);
                }
            }
            _ => {}
        }
    }
    fn mask(
        value: &Value,
        moving: &std::collections::BTreeSet<String>,
        marks: &mut BTreeMap<String, String>,
    ) -> Value {
        let mut mark = |s: &str| -> String {
            let next = format!("<v-{}>", marks.len() + 1);
            marks.entry(s.to_owned()).or_insert(next).clone()
        };
        match value {
            Value::String(s) => {
                if let Some(uid) = s.strip_prefix("<chave:").and_then(|s| s.strip_suffix('>')) {
                    if moving.contains(uid) {
                        return Value::String(format!("<chave:{}>", mark(uid)));
                    }
                }
                if !moving.contains(s) {
                    value.clone()
                } else if regex::Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}")
                    .unwrap()
                    .is_match(s)
                {
                    // A moment of this run ("now"): two of them being equal
                    // or not is a coincidence of timing, not a link.
                    Value::String("<agora>".into())
                } else {
                    Value::String(mark(s))
                }
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| mask(v, moving, marks)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), mask(v, moving, marks)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    let (a, b) = (prepared(content), prepared(other));
    let mut moving = std::collections::BTreeSet::new();
    volatile(&a, &b, &mut moving);
    mask(&a, &moving, &mut BTreeMap::new())
}

/// The script run twice by this build, made comparable (see [`comparable`]).
pub(crate) fn scripted_content() -> Value {
    let run = || {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Drive").join("ponte-sync.ponte");
        scripted_file(dir.path(), &file);
        content_of(&file)
    };
    let (first, second) = (run(), run());
    comparable(&first, &second)
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
    std::fs::create_dir_all(&out).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    scripted_file(dir.path(), &file);
    std::fs::copy(&file, out.join(format!("paridade-{name}.ponte"))).unwrap();
    std::fs::write(
        out.join(format!("paridade-{name}.json")),
        serde_json::to_string_pretty(&scripted_content()).unwrap() + "\n",
    )
    .unwrap();
}

/// The same script, here: the file it publishes carries the content the
/// build without F2a published (`paridade-0.51.json`, written by fc6342e),
/// compared as `comparable` says (only what varies between two runs of the
/// same build is masked).
#[test]
fn f2a_08_payload_is_the_one_of_0_51() {
    let here = scripted_content();
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
/// adopts it (its snapshot, exported again, is the file's, exactly),
/// changes it and publishes again in format 8; the change is in the file it
/// wrote. Answers that file's content.
fn joins_and_answers(fixture_name: &str, member: &str) -> (tempfile::TempDir, PathBuf, Value) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::copy(fixture(fixture_name), &file).unwrap();
    let taken = content_of(&file);
    let mut store = Store::open(&dir.path().join("novo.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config(member, vec![file.display().to_string()]))
        .unwrap();
    sync(&mut store);
    // Everything of the file was adopted, as it is.
    let adopted = serde_json::to_value(store.export_snapshot().unwrap()).unwrap();
    for (section, value) in taken["snapshot"].as_object().unwrap() {
        assert_eq!(&adopted[section], value, "retrato adotado: {section}");
    }
    let doc1 = doc(&store, &number("a", 1));
    let note = format!("Nota de {member}");
    store.save_note(doc1, &note).unwrap();
    let mut new = crate::tasks::Task::new(doc1);
    new.title = format!("Tarefa de {member}");
    store.save_task(&new).unwrap();
    sync(&mut store);
    let answered = content_of(&file);
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
