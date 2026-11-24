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
        link: None,
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
    // Without it the edit could only go by its number: damaged (F3.1b1
    // ends the F3.0a2 allowance).
    assert!(Refs::check(&edit, None, &Refs::default()).is_err());
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
                link: None,
            },
            Ref {
                kind: IdKind::Document,
                row: 7,
                proof: Proof::Unprovable,
                link: None,
            },
        ],
        ..good
    };
    let back: Refs = serde_json::from_value(json(&proofs)).unwrap();
    assert_eq!(back, proofs);
}

// ------------------------------------------------- review of F3.0a1 --

fn guard_of(t: &Mirrors, who: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}-guarda.ponte",
        t.dir.path().join(format!("{who}.sqlite3")).display()
    ))
}

/// Review F3.0a1, finding 2. The computer opens 9/2 mirrors and its guard
/// is a 9/1 of the same lineage holding a task no copy and no journal has
/// (the library itself was restored without it). The guard is demoted to a
/// donor; before it is replaced by the 9/2 just written, it is kept aside
/// byte for byte, as a guard of format 8 is.
///
/// Red on 5553b64: the demoted guard is skipped by the donors' archive (it
/// is the guard) and the guard's own archive only covers format 8: it is
/// overwritten and the task is gone.
#[test]
fn f3_0a_demoted_9_1_guard_is_kept_aside_before_it_is_replaced() {
    let mut t = mirrors();
    let mut task = crate::tasks::Task::blank(None);
    task.title = "Tarefa comum".into();
    t.a.save_task(&task).unwrap();
    sync(&mut t.a);
    let guard = guard_of(&t, "a");
    assert_eq!(level(&guard), F3_LEVEL);
    sharedfolder::reseal_for_test(&guard, KEY, 1, |_, payload| {
        let snapshot = &mut payload.snapshot;
        let mut only = snapshot.tasks[0].clone();
        only.id = snapshot.tasks.iter().map(|t| t.id).max().unwrap() + 1000;
        only.uid = uuid::Uuid::new_v4().to_string();
        only.title = "Só na guarda 9/1".into();
        snapshot.idents.push(crate::idents::IdentRow(
            "task".into(),
            only.id,
            uuid::Uuid::new_v4().simple().to_string(),
        ));
        snapshot.tasks.push(only);
    })
    .unwrap();
    let before = std::fs::read(&guard).unwrap();
    // (Kept aside earlier: the guard of format 8 the promotion replaced.)
    let earlier = aside_in(t.dir.path());
    // The archive cannot be made (its name is taken): the guard stays.
    let blocked = sharedfolder::aside_path(&guard).unwrap();
    std::fs::create_dir(&blocked).unwrap();
    let refused = try_sync(&mut t.a);
    assert!(refused.is_err(), "{:?}", refused.map(|r| r.notes));
    assert_eq!(
        std::fs::read(&guard).unwrap(),
        before,
        "the guard is intact"
    );
    std::fs::remove_dir(&blocked).unwrap();
    let report = sync(&mut t.a);
    assert_eq!(level(&guard), F3_LEVEL, "the guard is rewritten at 9/2");
    let kept: Vec<String> = aside_in(t.dir.path())
        .into_iter()
        .filter(|n| !earlier.contains(n))
        .collect();
    assert_eq!(
        kept.len(),
        1,
        "the demoted guard was kept aside: {kept:?} {:?}",
        report.notes
    );
    assert_eq!(
        std::fs::read(t.dir.path().join(&kept[0])).unwrap(),
        before,
        "byte for byte"
    );
}

impl Mirrors {
    fn set_floor(&self, who: &str, level: u32) {
        let mut all = std::collections::BTreeMap::new();
        all.insert(
            self.lineage(),
            crate::formatfloor::Floor { format: 9, level },
        );
        std::fs::write(self.floor_file(who), serde_json::to_vec(&all).unwrap()).unwrap();
    }
}

/// The configuration as a build of F2b leaves it: no field of F3 (a build
/// that does not know them drops them when it saves).
fn forget_f3_config_fields(store: &mut Store) {
    let mut value = serde_json::to_value(store.load_intranet_config().unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("synced_level");
    store
        .save_intranet_config(&serde_json::from_value(value).unwrap())
        .unwrap();
}

/// Exchanges `store` with only the configured path `only`, as a Ponte of
/// F2b would: the files written there and the guard are left at 9/1, the
/// floor at (9, 1), and the configuration without the fields of F3.
fn exchange_as_f2b(t: &mut Mirrors, who: &str, only: &Path) {
    let store = if who == "a" { &mut t.a } else { &mut t.b };
    let full = store.load_intranet_config().unwrap();
    let mut narrow = full.clone();
    narrow.folder_files = vec![only.display().to_string()];
    store.save_intranet_config(&narrow).unwrap();
    sync(store);
    let mut after = store.load_intranet_config().unwrap();
    after.folder_files = full.folder_files;
    store.save_intranet_config(&after).unwrap();
    forget_f3_config_fields(store);
    sharedfolder::reseal_for_test(&sharedfolder::nine_path(only), KEY, 1, |_, _| {}).unwrap();
    let guard = guard_of(t, who);
    if guard.exists() {
        sharedfolder::reseal_for_test(&guard, KEY, 1, |_, _| {}).unwrap();
    }
    t.set_floor(who, 1);
}

/// Review F3.0a1, finding 3. Two mirrors parted at 9/1: a Ponte of F2b on
/// A moved the pendrive and A's guard two revisions ahead (two tasks),
/// while another device took only the Drive to 9/2, at a lower revision.
/// A, updated, sees the 9/2: its floor rises to 2, the pendrive and the
/// guard are demoted. The last adoption of A was at level 1 (proved by its
/// guard: the revision and content it recorded), so the lower revision of
/// the 9/2 is a change of level, not a stale cloud copy: the 9/2 is the
/// base and the 9/1 goes through Q6 (the two tasks come back). An older 9/2
/// after that is refused as before (same level).
///
/// Red on 5553b64: the revision check (Drive below A's revision) stops
/// every exchange at the same point, forever.
#[test]
fn f3_0a_higher_local_9_1_revision_does_not_block_the_move_to_9_2() {
    let mut t = mirrors();
    t.as_f2b();
    forget_f3_config_fields(&mut t.a);
    forget_f3_config_fields(&mut t.b);
    let [drive, pendrive] = t.nines();
    let start = t.a.load_intranet_config().unwrap().synced_revision;
    let pendrive_name = t.paths[1].clone();
    for title in ["Primeira do pendrive", "Segunda do pendrive"] {
        let mut task = crate::tasks::Task::blank(None);
        task.title = title.into();
        t.a.save_task(&task).unwrap();
        exchange_as_f2b(&mut t, "a", &pendrive_name);
    }
    assert_eq!(level(&pendrive), 1);
    let local = t.a.load_intranet_config().unwrap().synced_revision;
    assert_eq!(copy_at(&pendrive).header.revision, local);
    assert!(local >= start + 2, "{start} {local}");
    // B (updated) takes only the Drive to 9/2, with nothing new.
    let mut narrow = t.b.load_intranet_config().unwrap();
    let both = narrow.folder_files.clone();
    narrow.folder_files = vec![t.paths[0].display().to_string()];
    t.b.save_intranet_config(&narrow).unwrap();
    sync(&mut t.b);
    narrow.folder_files = both;
    let _ = narrow;
    assert_eq!(level(&drive), F3_LEVEL);
    let on_drive = copy_at(&drive).header.revision;
    assert!(on_drive < local, "Drive {on_drive}, A {local}");
    let older_drive = std::fs::read(&drive).unwrap();

    let moved = try_sync(&mut t.a);
    let report = moved.unwrap_or_else(|e| panic!("A moves to 9/2: {e}"));
    assert_eq!(t.floor("a"), (9, F3_LEVEL));
    let titles: Vec<String> =
        t.a.list_tasks()
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
    for title in ["Primeira do pendrive", "Segunda do pendrive"] {
        assert!(
            titles.iter().any(|t| t == title),
            "{title}: {titles:?} {:?}",
            report.notes
        );
    }
    assert_eq!((level(&drive), level(&pendrive)), (F3_LEVEL, F3_LEVEL));
    assert_eq!(
        copy_at(&drive).content_identity(),
        copy_at(&pendrive).content_identity()
    );
    // Same level now: an older 9/2 is a stale cloud copy, refused.
    let now = t.a.load_intranet_config().unwrap().synced_revision;
    assert_eq!(copy_at(&drive).header.revision, now);
    std::fs::write(&drive, &older_drive).unwrap();
    std::fs::copy(&drive, &pendrive).unwrap();
    let stale = try_sync(&mut t.a);
    assert!(
        stale.as_ref().is_err_and(|e| e
            .to_string()
            .contains("ainda não recebeu a versão mais recente")),
        "{:?}",
        stale.map(|r| r.written_to)
    );
}

/// Review F3.0a1, finding 4. A task at some version v; two edits queued by
/// a Ponte of F2b (no proofs) were merged into one entry expecting v, and
/// the local copy is at v+2. A third edit, made after the update, has
/// proofs, so it is never merged into the old entry and goes after it. With
/// nobody else editing, the exchange replays the old entry once (v+1) and
/// then the new one, which must expect what the old entry leaves: no card,
/// and the last edit is what the task holds.
///
/// Red on 5553b64: the new entry expects the local v+2 and becomes a card
/// of a concurrent edit that never happened.
#[test]
fn f3_0a_old_merged_entry_then_new_edit_of_the_same_task_without_a_card() {
    let (_dir, mut a, path) = alone();
    let mut task = crate::tasks::Task::blank(None);
    task.title = "Tarefa de base".into();
    let id = a.save_task(&task).unwrap();
    sync(&mut a);
    let start = a.item_version("task", id).unwrap().version;
    let current = |store: &Store| {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    };
    for title in ["Primeira edição antiga", "Segunda edição antiga"] {
        let mut edit = current(&a);
        edit.title = title.into();
        a.save_task(&edit).unwrap();
    }
    // As a Ponte of F2b queued them: one merged entry, no proofs.
    let mut queued = a.list_offline_outbox().unwrap();
    assert_eq!(queued.len(), 1, "merged");
    let mut old = queued.pop().unwrap();
    old.refs = None;
    a.conn
        .execute(
            "UPDATE offline_outbox SET payload=?1 WHERE id=?2",
            rusqlite::params![
                crate::protection::seal(&serde_json::to_vec(&old).unwrap()).unwrap(),
                old.id
            ],
        )
        .unwrap();
    crate::f3a2_tests::unmapped(&a);
    assert_eq!(a.item_version("task", id).unwrap().version, start + 2);
    let mut edit = current(&a);
    edit.title = "Edição depois da atualização".into();
    a.save_task(&edit).unwrap();
    assert_eq!(a.list_offline_outbox().unwrap().len(), 2, "kept apart");

    let report = sync(&mut a);
    assert_eq!(
        (report.conflicts, a.list_sync_conflicts().unwrap().len()),
        (0, 0),
        "{:?} {:?}",
        report.notes,
        a.list_sync_conflicts().unwrap()
    );
    assert_eq!(current(&a).title, "Edição depois da atualização");
    let copy = copy_at(&sharedfolder::nine_path(&path));
    let there = copy
        .payload
        .snapshot
        .tasks
        .iter()
        .find(|t| t.id == id)
        .unwrap();
    assert_eq!(there.title, "Edição depois da atualização");
    assert!(a.list_offline_outbox().unwrap().is_empty());
}

/// Review F3.0a1, finding 5 (first link). A computer joins (no lineage in
/// its configuration yet): the first configured path is authenticated at
/// 9/2 and then a later path cannot be read. What was read is recorded at
/// once, as for a computer that already knows the lineage.
///
/// Red on 5553b64: the level is recorded only for the lineage of the
/// configuration, empty at the first link, and the error comes first.
#[test]
fn f3_0a_first_link_records_the_level_read_before_a_later_path_fails() {
    let t = mirrors();
    let damaged = t.dir.path().join("Outra").join("ponte-sync.ponte");
    std::fs::create_dir_all(damaged.parent().unwrap()).unwrap();
    std::fs::write(&damaged, b"PONTE-SYNC isto nao e uma copia").unwrap();
    let mut c = Store::open(&t.dir.path().join("c.sqlite3")).unwrap();
    c.save_intranet_config(&folder_config(
        "Terceiro",
        vec![
            t.paths[0].display().to_string(),
            damaged.display().to_string(),
        ],
    ))
    .unwrap();
    assert!(try_sync(&mut c).is_err(), "the damaged path stops it");
    assert_eq!(t.floor("c"), (9, F3_LEVEL), "the 9/2 read is recorded");
}

/// Review F3.0a1, finding 5 (no 9/2 confirmed). An exchange with nothing
/// new on 9/1 mirrors whose every rewrite fails with an ordinary error:
/// no 9/2 of the folder is confirmed, so the guard is not written at 9/2
/// either (the floor stays (9, 1), and a guard above it would say a level
/// no mirror has). Then a Ponte of F2b moves a mirror ahead: it is the
/// base, as any newer 9/1 is while the floor is (9, 1).
///
/// Red on 5553b64: the guard is rewritten at 9/2.
#[test]
// Windows files: read-only is only ever set back to writable here.
#[allow(clippy::permissions_set_readonly_false)]
fn f3_0a_guard_stays_9_1_while_no_9_2_mirror_is_confirmed() {
    let mut t = mirrors();
    t.as_f2b();
    let nines = t.nines();
    let guard = guard_of(&t, "a");
    assert_eq!(level(&guard), 1);
    let set_readonly = |on: bool| {
        for path in &nines {
            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_readonly(on);
            std::fs::set_permissions(path, permissions).unwrap();
        }
    };
    set_readonly(true);
    let failed = try_sync(&mut t.a);
    set_readonly(false);
    let failed = failed.unwrap_or_else(|e| panic!("nothing new: not an error ({e})"));
    assert_eq!(failed.unwritten.len(), 2, "{:?}", failed.notes);
    assert_eq!((level(&nines[0]), level(&nines[1])), (1, 1));
    assert_eq!(t.floor("a"), (9, 1));
    assert_eq!(level(&guard), 1, "no 9/2 confirmed: the guard stays 9/1");
    // A Ponte of F2b moves a mirror ahead.
    sharedfolder::reseal_for_test(&nines[0], KEY, 1, |header, payload| {
        header.revision += 1;
        payload.snapshot.revision += 1;
        for capture in &mut payload.snapshot.captures {
            capture.capture.title = "Título gravado pela F2b".into();
        }
    })
    .unwrap();
    sync(&mut t.a);
    assert_eq!(
        t.a.list_documents("").unwrap()[0].title,
        "Título gravado pela F2b"
    );
    assert_eq!((level(&nines[0]), level(&nines[1])), (F3_LEVEL, F3_LEVEL));
    assert_eq!(t.floor("a"), (9, F3_LEVEL));
}

/// Review F3.0a1, finding 5 (the guard read authenticated). The floor says
/// (9, 1), the mirrors are 9/1 and one of them is newer, but this
/// computer's guard is a 9/2 of the lineage: read authenticated before the
/// base is chosen, its level raises the floor, so no 9/1 is the base.
///
/// Red on 5553b64: only the mirrors and stubs count; the newer 9/1 becomes
/// the base.
#[test]
fn f3_0a_authenticated_9_2_guard_raises_the_floor_before_the_base_is_chosen() {
    let mut t = mirrors();
    t.as_f2b();
    let guard = guard_of(&t, "a");
    sharedfolder::reseal_for_test(&guard, KEY, F3_LEVEL, |_, _| {}).unwrap();
    let [n1, _] = t.nines();
    let title = t.a.list_documents("").unwrap()[0].title.clone();
    sharedfolder::reseal_for_test(&n1, KEY, 1, |header, payload| {
        header.revision += 1;
        payload.snapshot.revision += 1;
        for capture in &mut payload.snapshot.captures {
            capture.capture.title = "Título de uma cópia antiga".into();
        }
    })
    .unwrap();
    let _ = try_sync(&mut t.a);
    assert_eq!(t.floor("a"), (9, F3_LEVEL), "the guard's level");
    assert_eq!(
        t.a.list_documents("").unwrap()[0].title,
        title,
        "no 9/1 is the base"
    );
}

/// Review F3.0a1, finding 6. An oracle independent of the walker: for a
/// sample of each variant with numbers, every optional field filled, the
/// full list of (kind, number) sites is written here by hand, in the
/// documented order; the translation is learned from that list (not from
/// the walker), and the translated fields are checked one by one
/// (dependencies, parent, polymorphic owners, batch, sources and the names
/// that follow them).
///
/// Green on 5553b64 (the walker is right by inspection): it is the guard
/// the old test was not. With a walker that skips a task's dependencies,
/// `f3_0_walker_is_exhaustive` still passes and this one fails (see the
/// report of the review fixes).
#[test]
fn f3_0a_walker_pins_every_site_of_full_samples() {
    use crate::idmap::{CreatedId, IdKind as K, IdMap};
    use IntranetMutation as M;
    let (_dir, mut a, _path) = alone();
    crate::f2a_parity::every_variant(&mut a, "a");
    let made: Vec<IntranetMutation> = a
        .list_offline_outbox()
        .unwrap()
        .into_iter()
        .map(|e| e.mutation)
        .collect();
    let sample = |name: &str| -> IntranetMutation {
        made.iter()
            .find(|m| crate::f2a_parity::variant_name(m) == name)
            .unwrap_or_else(|| panic!("no sample of {name}"))
            .clone()
    };
    let source = |id: &str, document_id, capture_id, file_id| crate::evidence::Source {
        id: id.into(),
        document_id,
        capture_id,
        label: String::new(),
        file_id,
        page: None,
        method: String::new(),
        text: String::new(),
    };
    let task = |id, document, parent, deps: Vec<i64>| {
        let mut t = crate::tasks::Task::blank(document);
        t.id = id;
        t.parent_task_id = parent;
        t.dependencies = deps;
        t
    };
    let attendance = |id, batch, document, tasks: Vec<i64>| {
        let mut att = match sample("SaveSaiopAttendance") {
            M::SaveSaiopAttendance { attendance, .. } => attendance,
            _ => unreachable!(),
        };
        att.id = id;
        att.batch_id = batch;
        att.document_id = document;
        att.task_ids = tasks;
        att
    };
    let mut analysis = crate::model::Analysis::default();
    analysis.capture_id = 204;
    analysis.materials = vec![
        source("C204", 106, 204, None),
        source("C205F302P1OCR", 107, 205, Some(302)),
        source("A402Hcd34", 108, 206, None),
    ];
    for name in ["C204", "C205F302P1OCR", "A402Hcd34"] {
        analysis.citations.push(crate::evidence::Citation {
            source_id: name.into(),
            quote: String::new(),
            claim: String::new(),
        });
        analysis.facts.push(crate::model::Fact {
            source_id: name.into(),
            ..Default::default()
        });
    }
    let mut record = crate::work_templates::GenerationRecord::default();
    record.document_id = 120;
    record.capture_id = 208;
    record.draft_id = Some(950);
    for (id, document_id, capture_id, file_id) in [
        ("C208", 120, 208, None),
        ("C209F304P3", 121, 209, Some(304)),
        ("A408Hef56", 122, 210, None),
    ] {
        record.sources.push(crate::work_templates::UsedSource {
            id: id.into(),
            label: String::new(),
            document_id,
            capture_id,
            file_id,
        });
    }
    let mut resource_use = match sample("SaveResourceUse") {
        M::SaveResourceUse { item, .. } => item,
        _ => unreachable!(),
    };
    resource_use.document_id = Some(119);
    resource_use.saiop_id = Some(705);
    resource_use.task_id = Some(515);
    let row = match sample("ImportSaiopBatch") {
        M::ImportSaiopBatch { items, .. } => items[0].1.clone(),
        _ => unreachable!(),
    };
    let attach = |owner_kind: &str, owner_id| M::AddAttachment {
        owner_kind: owner_kind.into(),
        owner_id,
        name: "a.txt".into(),
        description: String::new(),
        data_base64: String::new(),
    };
    let trash = |kind: &str, id| M::TrashItem {
        kind: kind.into(),
        id,
    };
    // (sample, its sites by hand, in walk order)
    let pinned: Vec<(IntranetMutation, Vec<(K, i64)>)> = vec![
        (
            M::SaveNote {
                document_id: 101,
                note: String::new(),
                expected_version: Some(1),
            },
            vec![(K::Document, 101)],
        ),
        (
            M::SaveDraft {
                document_id: 103,
                capture_id: 201,
                kind: String::new(),
                body: String::new(),
                materials: vec![
                    source("C201", 103, 201, None),
                    source("C202F301P2", 104, 202, Some(301)),
                    source("A401Hab12", 105, 203, None),
                ],
                provenance: None,
            },
            vec![
                (K::Document, 103),
                (K::Capture, 201),
                (K::Document, 103),
                (K::Capture, 201),
                (K::Document, 104),
                (K::Capture, 202),
                (K::File, 301),
                (K::Document, 105),
                (K::Capture, 203),
                (K::Attachment, 401),
            ],
        ),
        (
            M::SaveAnalysis {
                document_id: 106,
                analysis,
            },
            vec![
                (K::Document, 106),
                (K::Capture, 204),
                (K::Document, 106),
                (K::Capture, 204),
                (K::Document, 107),
                (K::Capture, 205),
                (K::File, 302),
                (K::Document, 108),
                (K::Capture, 206),
                (K::Attachment, 402),
            ],
        ),
        (
            M::SaveTask {
                task: task(501, Some(109), Some(502), vec![503, 504]),
                expected_version: Some(2),
                no_project: false,
            },
            vec![
                (K::Task, 501),
                (K::Document, 109),
                (K::Task, 502),
                (K::Task, 503),
                (K::Task, 504),
            ],
        ),
        (
            M::RelinkTask {
                task_id: 505,
                document_id: Some(110),
                expected_version: None,
            },
            vec![(K::Task, 505), (K::Document, 110)],
        ),
        (
            M::AddDocumentLink {
                source: 111,
                target: 112,
                kind: String::new(),
                reason: String::new(),
            },
            vec![(K::Document, 111), (K::Document, 112)],
        ),
        (trash("document", 113), vec![(K::Document, 113)]),
        (trash("capture", 207), vec![(K::Capture, 207)]),
        (trash("task", 506), vec![(K::Task, 506)]),
        (trash("attachment", 403), vec![(K::Attachment, 403)]),
        (trash("note", 9), vec![]),
        (
            M::SaveSaiopAttendance {
                attendance: attendance(701, Some(801), Some(115), vec![507, 508]),
                expected_version: None,
            },
            vec![
                (K::Saiop, 701),
                (K::SaiopBatch, 801),
                (K::Document, 115),
                (K::Task, 507),
                (K::Task, 508),
            ],
        ),
        (
            M::ImportSaiopBatch {
                file_name: String::new(),
                file_sha256: String::new(),
                sheet_name: String::new(),
                items: vec![
                    (attendance(0, None, Some(116), vec![509]), row.clone()),
                    (attendance(0, None, None, vec![510, 511]), row),
                ],
            },
            vec![
                (K::Document, 116),
                (K::Task, 509),
                (K::Task, 510),
                (K::Task, 511),
            ],
        ),
        (
            M::CreateTaskForSaiop {
                attendance_id: 703,
                task: Some(task(0, Some(117), Some(512), vec![513])),
                title: String::new(),
                expected_version: None,
            },
            vec![
                (K::Saiop, 703),
                (K::Document, 117),
                (K::Task, 512),
                (K::Task, 513),
            ],
        ),
        (attach("document", 118), vec![(K::Document, 118)]),
        (attach("task", 514), vec![(K::Task, 514)]),
        (attach("saiop", 704), vec![(K::Saiop, 704)]),
        (attach("example", 901), vec![(K::Example, 901)]),
        (attach("outro", 902), vec![]),
        (
            M::SaveResourceUse {
                item: resource_use,
                expected_version: None,
            },
            vec![(K::Document, 119), (K::Saiop, 705), (K::Task, 515)],
        ),
        (
            M::RecordGeneration { record },
            vec![
                (K::Document, 120),
                (K::Capture, 208),
                (K::Draft, 950),
                (K::Document, 120),
                (K::Capture, 208),
                (K::Document, 121),
                (K::Capture, 209),
                (K::File, 304),
                (K::Document, 122),
                (K::Capture, 210),
                (K::Attachment, 408),
            ],
        ),
    ];
    for (mutation, expected) in &pinned {
        let name = crate::f2a_parity::variant_name(mutation);
        assert_eq!(&crate::idrefs::sites(mutation), expected, "{name}");
        // The translation comes from the list above, not from the walker.
        let here: Vec<CreatedId> = expected
            .iter()
            .map(|(kind, id)| CreatedId::new(*kind, format!("{id}"), *id))
            .collect();
        let there: Vec<CreatedId> = expected
            .iter()
            .map(|(kind, id)| CreatedId::new(*kind, format!("{id}"), id + 1000))
            .collect();
        let mut map = IdMap::default();
        map.learn(&here, Some(&there));
        let moved = map.remap(mutation, &a, None).unwrap();
        // Field by field.
        match &moved {
            M::SaveTask { task, .. } => {
                assert_eq!(
                    (
                        task.id,
                        task.document_id,
                        task.parent_task_id,
                        &task.dependencies
                    ),
                    (1501, Some(1109), Some(1502), &vec![1503, 1504])
                );
            }
            M::CreateTaskForSaiop {
                attendance_id,
                task: Some(task),
                ..
            } => {
                assert_eq!(*attendance_id, 1703);
                assert_eq!(
                    (
                        task.id,
                        task.document_id,
                        task.parent_task_id,
                        &task.dependencies
                    ),
                    (0, Some(1117), Some(1512), &vec![1513])
                );
            }
            M::SaveSaiopAttendance { attendance, .. } => {
                assert_eq!(
                    (
                        attendance.id,
                        attendance.batch_id,
                        attendance.document_id,
                        &attendance.task_ids
                    ),
                    (1701, Some(1801), Some(1115), &vec![1507, 1508])
                );
            }
            M::ImportSaiopBatch { items, .. } => {
                assert_eq!(
                    (items[0].0.document_id, &items[0].0.task_ids),
                    (Some(1116), &vec![1509])
                );
                assert_eq!(
                    (items[1].0.document_id, &items[1].0.task_ids),
                    (None, &vec![1510, 1511])
                );
            }
            M::AddAttachment {
                owner_kind,
                owner_id,
                ..
            } => {
                let want = match owner_kind.as_str() {
                    "document" => 1118,
                    "task" => 1514,
                    "saiop" => 1704,
                    "example" => 1901,
                    _ => 902,
                };
                assert_eq!(*owner_id, want, "owner {owner_kind}");
            }
            M::TrashItem { kind, id } => {
                let want = match kind.as_str() {
                    "document" => 1113,
                    "capture" => 1207,
                    "task" => 1506,
                    "attachment" => 1403,
                    _ => 9,
                };
                assert_eq!(*id, want, "trash {kind}");
            }
            M::SaveResourceUse { item, .. } => {
                assert_eq!(
                    (item.document_id, item.saiop_id, item.task_id),
                    (Some(1119), Some(1705), Some(1515))
                );
            }
            M::RelinkTask {
                task_id,
                document_id,
                ..
            } => assert_eq!((*task_id, *document_id), (1505, Some(1110))),
            M::AddDocumentLink { source, target, .. } => {
                assert_eq!((*source, *target), (1111, 1112))
            }
            M::SaveNote { document_id, .. } => assert_eq!(*document_id, 1101),
            M::SaveDraft {
                document_id,
                capture_id,
                materials,
                ..
            } => {
                assert_eq!((*document_id, *capture_id), (1103, 1201));
                let got: Vec<(&str, i64, i64, Option<i64>)> = materials
                    .iter()
                    .map(|s| (s.id.as_str(), s.document_id, s.capture_id, s.file_id))
                    .collect();
                assert_eq!(
                    got,
                    vec![
                        ("C1201", 1103, 1201, None),
                        ("C1202F1301P2", 1104, 1202, Some(1301)),
                        ("A1401Hab12", 1105, 1203, None),
                    ]
                );
            }
            M::SaveAnalysis {
                document_id,
                analysis,
            } => {
                assert_eq!((*document_id, analysis.capture_id), (1106, 1204));
                let got: Vec<(&str, i64, i64, Option<i64>)> = analysis
                    .materials
                    .iter()
                    .map(|s| (s.id.as_str(), s.document_id, s.capture_id, s.file_id))
                    .collect();
                let names = ["C1204", "C1205F1302P1OCR", "A1402Hcd34"];
                assert_eq!(
                    got,
                    vec![
                        (names[0], 1106, 1204, None),
                        (names[1], 1107, 1205, Some(1302)),
                        (names[2], 1108, 1206, None),
                    ]
                );
                // The names that use the sources follow them.
                let cited: Vec<&str> = analysis
                    .citations
                    .iter()
                    .map(|c| c.source_id.as_str())
                    .collect();
                let facts: Vec<&str> = analysis
                    .facts
                    .iter()
                    .map(|f| f.source_id.as_str())
                    .collect();
                assert_eq!((cited, facts), (names.to_vec(), names.to_vec()));
            }
            M::RecordGeneration { record } => {
                assert_eq!(
                    (record.document_id, record.capture_id, record.draft_id),
                    (1120, 1208, Some(1950))
                );
                let got: Vec<(&str, i64, i64, Option<i64>)> = record
                    .sources
                    .iter()
                    .map(|s| (s.id.as_str(), s.document_id, s.capture_id, s.file_id))
                    .collect();
                assert_eq!(
                    got,
                    vec![
                        ("C1208", 1120, 1208, None),
                        ("C1209F1304P3", 1121, 1209, Some(1304)),
                        ("A1408Hef56", 1122, 1210, None),
                    ]
                );
            }
            other => panic!("not checked: {other:?}"),
        }
    }
}
