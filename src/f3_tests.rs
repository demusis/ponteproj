//! F3.0a1 of Frente F: acceptance tests of the level barrier, the floor of
//! (format, level) observed and the refs every F3 producer writes
//! (`desenho-f3-v2.md` §1 and §2.1, `desenho-f3-v3-adendo.md` §1, §3 and
//! §12). Written before the code: each test says why it fails on `deb7fbb`,
//! or that it is a guard (green there and kept green).
#![allow(clippy::all)]

use crate::idents::hooks;
use crate::intranet::{IntranetConfig, IntranetMode, IntranetMutation, RpcRequest, RpcResponse};
use crate::sharedfolder::{self, Found};
use crate::storage::Store;
use std::path::{Path, PathBuf};

const KEY: &str = "senha-de-teste-123";
/// The level of format 9 that F3 writes (the base, F2b, writes 1).
const F3_LEVEL: u32 = 2;

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

fn try_sync(store: &mut Store) -> anyhow::Result<crate::intranet::FolderSyncReport> {
    let cfg = store.load_intranet_config().unwrap();
    store.sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
}

fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    try_sync(store).unwrap()
}

/// The level written in the plain header of the file at `path`, whatever it
/// is (read as a build that knows every level would).
fn level(path: &Path) -> u32 {
    sharedfolder::read_header_as(path, sharedfolder::NEWEST_FORMAT, u32::MAX)
        .unwrap()
        .unwrap()
        .level
}

fn copy_at(path: &Path) -> sharedfolder::Copy {
    match sharedfolder::read_any(path, KEY).unwrap() {
        Some(Found::Copy(copy)) => copy,
        _ => panic!("{} is not a copy", path.display()),
    }
}

fn aside_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("-divergente-"))
        .collect()
}

/// Two computers, A and B, sharing a library through two mirrors (two
/// configured paths in two folders), promoted to format 9 by A.
struct Mirrors {
    dir: tempfile::TempDir,
    paths: [PathBuf; 2],
    a: Store,
    b: Store,
}

fn mirrors() -> Mirrors {
    on();
    let dir = tempfile::tempdir().unwrap();
    let paths = [
        dir.path().join("Drive").join("ponte-sync.ponte"),
        dir.path().join("Pendrive").join("ponte-sync.ponte"),
    ];
    for path in &paths {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    let files: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", files.clone()))
        .unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    sync(&mut b);
    let report = sync(&mut a);
    assert!(report.promoted, "A promotes: {:?}", report.promotion_wait);
    sync(&mut b);
    Mirrors { dir, paths, a, b }
}

impl Mirrors {
    fn nines(&self) -> [PathBuf; 2] {
        [
            sharedfolder::nine_path(&self.paths[0]),
            sharedfolder::nine_path(&self.paths[1]),
        ]
    }

    fn lineage(&self) -> String {
        self.a.load_intranet_config().unwrap().folder_lineage
    }

    fn floor_file(&self, who: &str) -> PathBuf {
        PathBuf::from(format!(
            "{}-{}",
            self.dir.path().join(format!("{who}.sqlite3")).display(),
            crate::formatfloor::FILE
        ))
    }

    fn floor(&self, who: &str) -> (u32, u32) {
        let floor = crate::formatfloor::read(&self.floor_file(who), &self.lineage()).unwrap();
        (floor.format, floor.level)
    }

    /// The state a folder of F2b leaves: both files of format 9, both
    /// stubs at the old names and both guards at level 1, and the floor of
    /// each device at (9, 1).
    fn as_f2b(&self) {
        for path in self.nines() {
            sharedfolder::reseal_for_test(&path, KEY, 1, |_, _| {}).unwrap();
        }
        for path in &self.paths {
            sharedfolder::reseal_stub_for_test(path, KEY, 1).unwrap();
        }
        for who in ["a", "b"] {
            let guard = PathBuf::from(format!(
                "{}-guarda.ponte",
                self.dir.path().join(format!("{who}.sqlite3")).display()
            ));
            if guard.exists() {
                sharedfolder::reseal_for_test(&guard, KEY, 1, |_, _| {}).unwrap();
            }
            let mut all = std::collections::BTreeMap::new();
            all.insert(
                self.lineage(),
                crate::formatfloor::Floor {
                    format: 9,
                    level: 1,
                },
            );
            std::fs::write(self.floor_file(who), serde_json::to_vec(&all).unwrap()).unwrap();
        }
    }
}

// ------------------------------------------------- adendo §1 (R2-01) --

/// R2-01. The floor is the (format, level) pair observed: the highest level
/// read, then the level read back after a write; a 9 below it is a donor,
/// never the base, and is rewritten; every 9 below the level of this build
/// is a write target even when its content is equal.
///
/// Red on `deb7fbb`: the build writes level 1, so the identical 9/1 mirrors
/// are left as they are (the exchange has nothing to do) and no fault can
/// fire; (a) fails at its first assertion. Red [v2] (LEVEL=2 alone): the
/// floor is stamped (9,2) at the read, and the repeat refuses its own base.
#[test]
fn f3_0a1_floor_follows_observed_level() {
    // (a) A reads a 9/1 from two mirrors; a fault fires before the first
    // write (another writer touched mirror 1 meanwhile).
    let mut t = mirrors();
    t.as_f2b();
    let [n1, n2] = t.nines();
    let old_nine = std::fs::read(&n1).unwrap();
    let touched = n1.clone();
    hooks::BEFORE_FOLDER_WRITE.with(|h| {
        *h.borrow_mut() = Some(Box::new(move || {
            sharedfolder::reseal_for_test(&touched, KEY, 1, |_, _| {}).unwrap();
        }))
    });
    let first = try_sync(&mut t.a);
    assert!(
        first.is_err(),
        "(a) the 9/1 mirrors are written (and the fault stops that): {:?}",
        first.map(|r| r.written_to)
    );
    assert_eq!(t.floor("a"), (9, 1), "(a) the floor keeps the level read");
    assert_eq!((level(&n1), level(&n2)), (1, 1), "(a) nothing written");
    let repeat = sync(&mut t.a);
    assert_eq!(
        (level(&n1), level(&n2)),
        (F3_LEVEL, F3_LEVEL),
        "(a) both mirrors, the identical one included: {:?}",
        repeat.written_to
    );
    assert_eq!(t.floor("a"), (9, F3_LEVEL), "(a) the floor read back");
    assert_eq!(
        copy_at(&n1).content_identity(),
        copy_at(&n2).content_identity()
    );

    // (c) A 9/1 restored later into the set becomes a donor: kept aside and
    // rewritten, never the base.
    let title = t.a.list_documents("").unwrap()[0].title.clone();
    std::fs::write(&n1, &old_nine).unwrap();
    assert_eq!(level(&n1), 1);
    sync(&mut t.a);
    assert_eq!(level(&n1), F3_LEVEL, "(c) the restored 9/1 is rewritten");
    assert_eq!(
        copy_at(&n1).content_identity(),
        copy_at(&n2).content_identity()
    );
    assert!(
        !aside_in(n1.parent().unwrap()).is_empty(),
        "(c) the restored 9/1 was kept aside first"
    );
    assert_eq!(t.a.list_documents("").unwrap()[0].title, title);
    // B, still at (9,1), reads both at level 2 and follows.
    sync(&mut t.b);
    assert_eq!(
        t.floor("b"),
        (9, F3_LEVEL),
        "(c) B's floor follows the read"
    );

    // (b) Fresh 9/1 mirrors; the write succeeds on one mirror and faults on
    // the other (changed meanwhile).
    let mut t = mirrors();
    t.as_f2b();
    let [n1, n2] = t.nines();
    let (p1, p2) = (n1.clone(), n2.clone());
    hooks::BEFORE_RENAME.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |path: &Path| {
            let other = if path == p1.as_path() { &p2 } else { &p1 };
            sharedfolder::reseal_for_test(other, KEY, 1, |_, _| {}).unwrap();
        }))
    });
    let faulted = try_sync(&mut t.a);
    assert!(
        faulted.is_err(),
        "(b) the second mirror faults: {:?}",
        faulted.map(|r| r.written_to)
    );
    let (written, stale) = if level(&n1) == F3_LEVEL {
        (n1.clone(), n2.clone())
    } else {
        (n2.clone(), n1.clone())
    };
    assert_eq!((level(&written), level(&stale)), (F3_LEVEL, 1));
    assert_eq!(t.floor("a"), (9, F3_LEVEL), "(b) the floor read back");
    // A writer of level 1 then makes the stale mirror NEWER than the base,
    // with other content: it must still never be the base.
    let title = t.a.list_documents("").unwrap()[0].title.clone();
    sharedfolder::reseal_for_test(&stale, KEY, 1, |header, payload| {
        header.revision += 1;
        payload.snapshot.revision += 1;
        for capture in &mut payload.snapshot.captures {
            capture.capture.title = "Título de uma cópia antiga".into();
        }
    })
    .unwrap();
    let next = sync(&mut t.a);
    assert_eq!(
        t.a.list_documents("").unwrap()[0].title,
        title,
        "(b) the stale mirror is never the base"
    );
    assert_eq!(
        (level(&written), level(&stale)),
        (F3_LEVEL, F3_LEVEL),
        "(b) the stale mirror is rewritten: {:?}",
        next.written_to
    );
    assert_eq!(
        copy_at(&stale).content_identity(),
        copy_at(&written).content_identity()
    );
    assert!(
        !aside_in(stale.parent().unwrap()).is_empty(),
        "(b) the stale mirror was kept aside first"
    );
}

// ------------------------------------------------- v2 §1: level 2 --

/// v2 §1. A file this build writes is at level 2, and the floor of the
/// device that wrote it, and of the one that read it, is (9, 2).
/// Red on `deb7fbb`: written at level 1, floor (9, 1).
#[test]
fn f3_0_written_file_is_level_2_and_floor_refuses_level_1_after() {
    let t = mirrors();
    for path in t.nines() {
        assert_eq!(level(&path), F3_LEVEL, "{}", path.display());
        assert_eq!(copy_at(&path).header.level, F3_LEVEL);
    }
    assert_eq!(t.floor("a"), (9, F3_LEVEL));
    assert_eq!(t.floor("b"), (9, F3_LEVEL));
}

/// Parity (green on `deb7fbb`): a reader of F2b (format 9, level 1)
/// refuses a file of level 2 before reading anything else, and this build
/// refuses a level above its own.
#[test]
fn f3_0_old_reader_refuses_level_2() {
    let t = mirrors();
    let [n1, n2] = t.nines();
    sharedfolder::reseal_for_test(&n2, KEY, F3_LEVEL, |_, _| {}).unwrap();
    let refused = sharedfolder::read_header_as(&n2, sharedfolder::FORMAT_9, 1).unwrap_err();
    assert!(
        refused.to_string().contains("Atualize o Ponte"),
        "{refused}"
    );
    sharedfolder::reseal_for_test(&n1, KEY, sharedfolder::LEVEL + 1, |_, _| {}).unwrap();
    let refused = sharedfolder::read_header(&n1).unwrap_err();
    assert!(
        refused.to_string().contains("Atualize o Ponte"),
        "{refused}"
    );
}

// ------------------------------------------- v2 §1: the host's barrier --

/// A main computer on a private port, with one document.
struct Host {
    _dir: tempfile::TempDir,
    store: Store,
    url: String,
    token: String,
    doc: i64,
    server: crate::intranet::HostServer,
}

fn host() -> Host {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("host.sqlite3");
    let token = crate::intranet::generate_token();
    let mut store = Store::open(&db).unwrap();
    store
        .save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "ANFITRIAO".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: "http://127.0.0.1:7878".into(),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        })
        .unwrap();
    let doc = store
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let server = crate::intranet::HostServer::start(db, "127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.local_addr());
    Host {
        _dir: dir,
        store,
        url,
        token,
        doc,
        server,
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.server.stop();
    }
}

impl Host {
    /// Posts `request` as it is (a V2 envelope of any level) and returns the
    /// HTTP status and the answer.
    fn post<T: serde::Serialize>(&self, request: &T) -> (u16, RpcResponse) {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap();
        let response = client
            .post(format!("{}/api/v1/rpc", self.url))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(request)
            .send()
            .unwrap();
        let status = response.status().as_u16();
        (status, response.json().unwrap())
    }

    fn note(&self) -> String {
        self.store
            .conn
            .query_row(
                "SELECT coalesce((SELECT group_concat(id) FROM notes),'')",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
}

fn at_level(level: u32, request: RpcRequest) -> RpcRequest {
    RpcRequest::V2 {
        level,
        request: Box::new(request),
    }
}

/// v2 §1 / frozen rule 1. A main computer of F3 answers 503 to a request of
/// the protocol V2 below level 2 (an F2b client), Mutate and Sync, before
/// changing anything. Red on `deb7fbb`: level 1 is accepted (200) and the
/// note is written.
#[test]
fn f3_0_host_refuses_level_1_mutate_and_sync() {
    let host = host();
    let before = host.note();
    // As an F2b client sends it (raw JSON: the shape of its envelope).
    let (status, answer) = host.post(&serde_json::json!({"V2": {"level": 1, "request": {"Mutate": {
        "actor": "Cliente F2b",
        "mutation": {"SaveNote": {"document_id": host.doc, "note": "Nota de um Ponte de F2b", "expected_version": null}},
        "client_schema": crate::storage::SCHEMA_VERSION,
        "mutation_uid": "uid-de-um-cliente-f2b-0001"
    }}}}));
    assert_eq!(status, 503, "{answer:?}");
    match answer {
        RpcResponse::Error { message } => {
            assert!(
                message.contains("Atualize o Ponte neste computador"),
                "{message}"
            )
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(host.note(), before, "nothing changed");
    let (status, answer) = host.post(&at_level(
        1,
        RpcRequest::Sync {
            since_revision: None,
            known_epoch: String::new(),
        },
    ));
    assert_eq!(status, 503, "{answer:?}");
    assert!(!matches!(answer, RpcResponse::Snapshot(_)));
}

/// Parity (green on `deb7fbb`): a main computer refuses, with 503, a level
/// above its own (an older main computer and a newer client).
#[test]
fn f3_0_old_host_answers_503_to_level_2() {
    let host = host();
    let (status, answer) = host.post(&at_level(sharedfolder::LEVEL + 1, RpcRequest::Ping));
    assert_eq!(status, 503, "{answer:?}");
    match answer {
        RpcResponse::Error { message } => assert!(
            message.contains("Atualize o Ponte no computador principal"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
    // Its own level is served.
    let (status, _) = host.post(&at_level(sharedfolder::LEVEL, RpcRequest::Ping));
    assert_eq!(status, 200);
}

// ----------------------------------------------- adendo §3: the refs --

fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

/// One computer in shared-folder mode on a promoted folder.
fn alone() -> (tempfile::TempDir, Store, PathBuf) {
    on();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", vec![path.display().to_string()]))
        .unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    sync(&mut a);
    let report = sync(&mut a);
    assert!(report.promoted, "{:?}", report.promotion_wait);
    (dir, a, path)
}

fn ident(store: &Store, kind: &str, row: i64) -> String {
    store
        .conn
        .query_row(
            "SELECT ident FROM item_ident WHERE kind=?1 AND row_id=?2",
            rusqlite::params![kind, row],
            |r| r.get(0),
        )
        .unwrap()
}

/// Adendo §3 rule 1 / frozen rule 4 / v2 §2.1. Every F3 producer writes
/// `refs: Some(..)`, one ref per id site of the mutation and of its base,
/// proved by the row's ident and captured before the change ran; the queue
/// entry keeps them through its JSON, the journal of the file written
/// carries them, and the payload says from which revision the lineage
/// carries refs (`refs_since`). Red on `deb7fbb`: nothing writes `refs`.
#[test]
fn f3_0_refs_round_trip() {
    let (_dir, mut a, path) = alone();
    let doc = a.list_documents("").unwrap()[0].id;
    let doc_ident = ident(&a, "document", doc);
    let revision = a.load_intranet_config().unwrap().synced_revision;
    // queue_locally (an edit with a base).
    a.save_note(doc, "Nota com prova").unwrap();
    let outbox = a.list_offline_outbox().unwrap();
    let entry = outbox
        .iter()
        .find(|e| matches!(e.mutation, IntranetMutation::SaveNote { .. }))
        .unwrap();
    let value = json(entry);
    let refs = &value["refs"];
    assert!(refs.is_object(), "an F3 entry writes refs: {value}");
    assert_eq!(
        refs["mutation"],
        serde_json::json!([{"kind": "Document", "row": doc, "proof": {"Ident": doc_ident}}])
    );
    assert_eq!(
        refs["base"],
        serde_json::json!([{"kind": "Document", "row": doc, "proof": {"Ident": doc_ident}}])
    );
    assert_eq!(refs["frame"]["revision"], serde_json::json!(revision));
    // The round trip of the queue keeps them.
    let back: crate::intranet::OutboxEntry = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(json(&back)["refs"], *refs);
    // The atomic queue (record and attachment in one transaction): the
    // import has no site (refs present and empty), the attachment names its
    // record by its ident.
    let mut capture = crate::model::sample_capture();
    capture.document_number = "SEFAZ-PRO-2026/00777".into();
    capture.source_url =
        "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00777"
            .into();
    let input = crate::attachments::AttachmentInput {
        name: "nota.txt",
        description: "",
        data: "Texto simples do anexo.".as_bytes(),
    };
    let (receipt, _) = a
        .import_with_attachments_atomically(&capture, &[input], None)
        .unwrap()
        .unwrap();
    let outbox = a.list_offline_outbox().unwrap();
    let import = outbox
        .iter()
        .find(|e| matches!(&e.mutation, IntranetMutation::ImportCapture { capture } if capture.document_number == "SEFAZ-PRO-2026/00777"))
        .unwrap();
    assert_eq!(json(import)["refs"]["mutation"], serde_json::json!([]));
    let attach = outbox
        .iter()
        .find(|e| matches!(&e.mutation, IntranetMutation::AddAttachment { owner_id, .. } if *owner_id == receipt.document_id))
        .unwrap();
    assert_eq!(
        json(attach)["refs"]["mutation"],
        serde_json::json!([{"kind": "Document", "row": receipt.document_id,
            "proof": {"Ident": ident(&a, "document", receipt.document_id)}}])
    );
    // The file written carries the refs in its journal, and the revision
    // from which the lineage carries them.
    let uid = entry.uid.clone();
    let expected = refs.clone();
    sync(&mut a);
    let copy = copy_at(&sharedfolder::nine_path(&path));
    let journal = copy
        .payload
        .journal
        .iter()
        .find(|e| e.uid == uid)
        .expect("the note is in the journal");
    assert_eq!(json(journal)["refs"], expected);
    let since = json(&copy.payload)["refs_since"].clone();
    assert!(since.is_i64(), "refs_since: {since}");
    assert!(since.as_i64().unwrap() <= copy.header.revision);
}

// ------------------------------------- the walker and Refs::check (API) --
//
// These name the types of F3.0a1 and so cannot exist on `deb7fbb`: they are
// guards of the new code, not red tests.

/// v2 §6 parity: the walk of every variant of `IntranetMutation` (made
/// through the product, `f2a_parity::every_variant`) is the walk
/// `IdMap::remap` translates through, and each queued entry carries one
/// ref per site of its change and of its base, of the site's kind and
/// number. The uid-addressed variants have no site.
#[test]
fn f3_0_walker_is_exhaustive() {
    use crate::idmap::{CreatedId, IdKind, IdMap};
    let (_dir, mut a, _path) = alone();
    crate::f2a_parity::every_variant(&mut a, "a");
    let queued = a.list_offline_outbox().unwrap();
    let names: std::collections::BTreeSet<&str> = queued
        .iter()
        .map(|e| crate::f2a_parity::variant_name(&e.mutation))
        .collect();
    assert!(
        names.len() + 1 >= crate::f2a_parity::enum_variant_count(),
        "{names:?}"
    );
    let head = |name: &str| -> Option<Vec<IdKind>> {
        use IdKind as K;
        Some(match name {
            "SaveNote" | "SaveWorkContext" | "DeleteDocument" => vec![K::Document],
            "SaveDraft" | "SaveAnalysis" | "RecordGeneration" => vec![K::Document, K::Capture],
            "RelinkTask" => vec![K::Task],
            "SavePageText" => vec![K::File],
            "AddDocumentLink" => vec![K::Document, K::Document],
            "SetLinkActive" => vec![K::Link],
            "UndoSaiopBatch" => vec![K::SaiopBatch],
            // (A new atendimento, number 0, has no site of its own.)
            "DeleteSaiopAttendance" | "CreateTaskForSaiop" => vec![K::Saiop],
            "UpdateAttachment"
            | "DeleteAttachment"
            | "ExtractAttachmentText"
            | "ReviewAttachmentText" => vec![K::Attachment],
            "SaveTemplate"
            | "SetTemplateStatus"
            | "DeleteTemplate"
            | "SaveInstructionBlock"
            | "SetBlockStatus"
            | "DeleteBlock"
            | "SaveExample"
            | "DeleteExample"
            | "DeleteResource"
            | "SaveResource"
            | "SaveCategory"
            | "DeleteCategory"
            | "SaveProject"
            | "DeleteProject"
            | "SaveSetting"
            | "DeleteResourceUse"
            | "ImportCapture"
            | "ImportBundle" => vec![],
            _ => return None,
        })
    };
    for entry in &queued {
        let name = crate::f2a_parity::variant_name(&entry.mutation);
        let sites = crate::idrefs::sites(&entry.mutation);
        if let Some(expected) = head(name) {
            if expected.is_empty() {
                assert!(sites.is_empty(), "{name}: {sites:?}");
            } else {
                let kinds: Vec<IdKind> = sites.iter().map(|s| s.0).collect();
                assert!(kinds.starts_with(&expected), "{name}: {kinds:?}");
            }
        }
        // The producer proved each site, of its kind and number.
        let refs = entry
            .refs
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: no refs"));
        crate::idrefs::Refs::check(&entry.mutation, entry.base.as_ref(), refs)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // `remap` walks the same sites: every number moved by 1000 moves
        // every site (and only them).
        let here: Vec<CreatedId> = sites
            .iter()
            .map(|(kind, id)| CreatedId::new(*kind, format!("{id}"), *id))
            .collect();
        let there: Vec<CreatedId> = sites
            .iter()
            .map(|(kind, id)| CreatedId::new(*kind, format!("{id}"), id + 1000))
            .collect();
        let mut map = IdMap::default();
        map.learn(&here, Some(&there));
        let moved = map.remap(&entry.mutation, &a, None).unwrap();
        let shifted: Vec<(IdKind, i64)> = sites.iter().map(|(k, id)| (*k, id + 1000)).collect();
        assert_eq!(crate::idrefs::sites(&moved), shifted, "{name}");
    }
}

/// Adendo §3 rule 2 / frozen rule 5: `Refs::check` holds only for the
/// proofs of this very change and base.
#[test]
fn f3_0a1_refs_check_per_site() {
    use crate::idmap::IdKind;
    use crate::idrefs::{Proof, Ref, Refs};
    let r = |kind, row| Ref {
        kind,
        row,
        proof: Proof::Frame,
    };
    let note = IntranetMutation::RelinkTask {
        task_id: 42,
        document_id: Some(7),
        expected_version: None,
    };
    let good = Refs {
        mutation: vec![r(IdKind::Task, 42), r(IdKind::Document, 7)],
        ..Refs::default()
    };
    assert!(Refs::check(&note, None, &good).is_ok());
    // One site too few, a wrong kind, a wrong number.
    for bad in [
        vec![r(IdKind::Task, 42)],
        vec![r(IdKind::Document, 42), r(IdKind::Document, 7)],
        vec![r(IdKind::Task, 42), r(IdKind::Document, 8)],
    ] {
        let refs = Refs {
            mutation: bad,
            ..Refs::default()
        };
        assert!(Refs::check(&note, None, &refs).is_err(), "{refs:?}");
    }
    // No base, but base proofs; a base, but none of its proofs.
    let with_base = Refs {
        base: vec![r(IdKind::Task, 42)],
        ..good.clone()
    };
    assert!(Refs::check(&note, None, &with_base).is_err());
    assert!(Refs::check(&note, Some(&note), &good).is_err());
    let both = Refs {
        base: good.mutation.clone(),
        ..good.clone()
    };
    assert!(Refs::check(&note, Some(&note), &both).is_ok());
    // Sentinels are no site: a task moved out of any record names only
    // itself, and a number 0 is none.
    let unlinked = IntranetMutation::RelinkTask {
        task_id: 42,
        document_id: None,
        expected_version: None,
    };
    assert_eq!(crate::idrefs::sites(&unlinked), vec![(IdKind::Task, 42)]);
    let zero = IntranetMutation::RelinkTask {
        task_id: 0,
        document_id: Some(0),
        expected_version: None,
    };
    assert!(crate::idrefs::sites(&zero).is_empty());
    // A numbered manual edit names its record, and only it does.
    let mut manual = crate::model::sample_capture();
    manual.source_url = format!("{}MANUAL-2026/0001", crate::model::MANUAL_ORIGIN);
    manual.document_number = "MANUAL-2026/0001".into();
    let edit = IntranetMutation::ImportCapture { capture: manual };
    // Until F3.1b1 captures the target, such an edit without it goes by
    // its number, as before (F3.0a2).
    assert!(Refs::check(&edit, None, &Refs::default()).is_ok());
    let targeted = Refs {
        manual_target: Some(r(IdKind::Document, 3)),
        ..Refs::default()
    };
    assert!(Refs::check(&edit, None, &targeted).is_ok());
    let plain = IntranetMutation::ImportCapture {
        capture: crate::model::sample_capture(),
    };
    assert!(Refs::check(&plain, None, &targeted).is_err());
    assert!(Refs::check(&plain, None, &Refs::default()).is_ok());
    // The proofs travel as JSON unchanged.
    let proofs = Refs {
        mutation: vec![
            Ref {
                kind: IdKind::Task,
                row: 42,
                proof: Proof::Ident("ab".repeat(16)),
            },
            Ref {
                kind: IdKind::Document,
                row: 7,
                proof: Proof::Unprovable,
            },
        ],
        ..good
    };
    let back: Refs = serde_json::from_value(json(&proofs)).unwrap();
    assert_eq!(back, proofs);
}
