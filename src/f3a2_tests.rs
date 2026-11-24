//! F3.0a2 of Frente F: acceptance tests of the resolution of the refs at
//! every receiver, the host's durable insertion log and the receipt asked
//! before any resolution (`desenho-f3-v3-adendo.md` §3, §5, §6 and §12;
//! `desenho-f3-v2.md` §2.1, §2.2). Written before the code: each test says
//! why it fails on `ea82162` (F3.0a1), or that it is a guard (green there,
//! red only on v2 as written, and kept green).
#![allow(clippy::all)]

use crate::idents::hooks;
use crate::idmap::IdKind;
use crate::idrefs::{Proof, Ref};
use crate::intranet::{IntranetConfig, IntranetMode, OutboxEntry};
use crate::storage::Store;
use std::path::{Path, PathBuf};

const KEY: &str = "senha-de-teste-123";

// ---------------------------------------------------------------- helpers --

/// A main computer and one client, in the protocol V2 at this build's level.
pub(crate) struct Team {
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) host: Store,
    pub(crate) client: Store,
    pub(crate) db: PathBuf,
    token: String,
    server: Option<crate::intranet::HostServer>,
}

impl Team {
    pub(crate) fn new() -> Self {
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

    /// Starts (again) the main computer's server: a new process, as after
    /// a crash.
    pub(crate) fn online(&mut self) {
        self.offline();
        let server = crate::intranet::HostServer::start(self.db.clone(), "127.0.0.1:0").unwrap();
        let mut cfg = self.client.load_intranet_config().unwrap();
        cfg.host_url = format!("http://{}", server.local_addr());
        cfg.cabinet_token = self.token.clone();
        self.client.save_intranet_config(&cfg).unwrap();
        self.server = Some(server);
    }

    pub(crate) fn offline(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
        let mut cfg = self.client.load_intranet_config().unwrap();
        cfg.host_url = "http://127.0.0.1:1".into();
        self.client.save_intranet_config(&cfg).unwrap();
    }

    pub(crate) fn cards(&self) -> usize {
        self.client.list_sync_conflicts().unwrap().len()
    }
}

impl Drop for Team {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
    }
}

pub(crate) fn capture(number: &str, text: &str) -> crate::model::Capture {
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

pub(crate) fn ident(store: &Store, kind: &str, row: i64) -> String {
    store
        .conn
        .query_row(
            "SELECT ident FROM item_ident WHERE kind=?1 AND row_id=?2",
            rusqlite::params![kind, row],
            |r| r.get(0),
        )
        .unwrap()
}

pub(crate) fn count(store: &Store, sql: &str) -> i64 {
    store.conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// Rewrites the last queued change of `store` with `edit` (as a damaged
/// change, or an older Ponte's, would have been queued).
pub(crate) fn edit_last_queued(store: &Store, edit: impl FnOnce(&mut OutboxEntry)) {
    let mut entry = store.list_offline_outbox().unwrap().pop().unwrap();
    edit(&mut entry);
    store
        .conn
        .execute(
            "UPDATE offline_outbox SET payload=?1 WHERE id=?2",
            rusqlite::params![
                crate::protection::seal(&serde_json::to_vec(&entry).unwrap()).unwrap(),
                entry.id
            ],
        )
        .unwrap();
}

fn task_document(store: &Store, task: i64) -> Option<i64> {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.id == task)
        .unwrap()
        .document_id
}

fn description(store: &Store, doc: i64, attachment: i64) -> String {
    store
        .list_attachments(crate::attachments::Owner::Document(doc))
        .unwrap()
        .into_iter()
        .find(|a| a.id == attachment)
        .unwrap()
        .description
        .trim()
        .to_owned()
}

// ------------------------------------------- adendo §3 (R2-03) --

/// R2-03 / frozen rules 4–7. The receiver resolves each site by its proof:
/// an `Ident` to the row holding it; a `Frame` number only in the very
/// frame it was captured in (same epoch and revision); never through the
/// number. The base resolves in the same frame and must name the same item
/// as the mutation. A `Some` whose sites do not match the change, or a
/// journal entry without refs above `refs_since`, is damaged: a card.
///
/// Red on `ea82162`: nothing calls `Refs::check` or resolves a ref, so the
/// task named by `Frame` is moved although the host moved on (r+1), the
/// plain edit whose base names Y overwrites X (`plain_edit_conflict` drops
/// the base's id), the entry with one ref too many is sent, and the
/// ref-less journal entry above `refs_since` is replayed.
#[test]
fn f3_0a2_frame_and_base_never_fall_back_to_number() {
    let mut t = Team::new();
    let x = t.host.list_documents("").unwrap()[0].id;
    let other = t
        .host
        .import_capture(&capture("SEFAZ-PRO-2026/00777", "Outro documento."))
        .unwrap()
        .document_id;
    let task = t
        .host
        .save_task(&crate::tasks::Task {
            title: "Tarefa sem registro".into(),
            ..crate::tasks::Task::new(other)
        })
        .unwrap();
    let a1 = t
        .host
        .add_attachment(
            crate::attachments::Owner::Document(x),
            "x.pdf",
            "velha",
            b"%PDF-1.4 anexo x",
        )
        .unwrap();
    let a2 = t
        .host
        .add_attachment(
            crate::attachments::Owner::Document(x),
            "y.pdf",
            "velha",
            b"%PDF-1.4 anexo y",
        )
        .unwrap();
    t.client.sync_from_host(true).unwrap();
    assert_eq!(task_document(&t.client, task), Some(other));

    // The task named by `Frame` (a row without identity), the document by
    // its identity, captured at revision r.
    let as_frame = |entry: &mut OutboxEntry| {
        let refs = entry.refs.as_mut().expect("refs");
        for site in &mut refs.mutation {
            if site.kind == IdKind::Task {
                site.proof = Proof::Frame;
            }
        }
    };
    // (a) At r+1 on the main computer: a card, nothing moves.
    t.offline();
    t.client.relink_task(task, Some(x), None).unwrap();
    edit_last_queued(&t.client, as_frame);
    t.host.save_note(x, "O principal escreveu depois.").unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(t.cards(), 1, "the Frame number of another frame is a card");
    assert_eq!(task_document(&t.host, task), Some(other), "no task moved");
    assert!(t.client.list_offline_outbox().unwrap().is_empty());

    // (b) At r (nobody wrote since the capture): it applies.
    t.offline();
    t.client.relink_task(task, Some(x), None).unwrap();
    edit_last_queued(&t.client, as_frame);
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(t.cards(), 1, "no new card");
    assert_eq!(task_document(&t.host, task), Some(x), "applied at r");

    // (c) A versionless plain edit of X whose base names Y (same old value
    // on both): the base resolves to another item, so a card.
    t.offline();
    t.client.update_attachment_description(a1, "nova").unwrap();
    let y_ident = ident(&t.client, "attachment", a2);
    edit_last_queued(&t.client, |entry| {
        entry.base = Some(crate::intranet::IntranetMutation::UpdateAttachment {
            id: a2,
            description: "velha".into(),
        });
        entry.refs.as_mut().unwrap().base = vec![Ref {
            kind: IdKind::Attachment,
            row: a2,
            proof: Proof::Ident(y_ident.clone()),
        }];
    });
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(t.cards(), 2, "base on another item: a card");
    assert_eq!(description(&t.host, x, a1), "velha", "X unchanged");
    assert_eq!(description(&t.host, x, a2), "velha", "Y unchanged");

    // (d) A `Some` with one site too many: damaged, a card.
    t.offline();
    t.client.save_note(x, "Nota com provas a mais").unwrap();
    edit_last_queued(&t.client, |entry| {
        let refs = entry.refs.as_mut().unwrap();
        let extra = refs.mutation[0].clone();
        refs.mutation.push(extra);
    });
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(t.cards(), 3, "wrong site count: a card");
    assert_ne!(
        t.host.load_document(x).unwrap().note,
        "Nota com provas a mais"
    );
}

/// One shared file, A and B, promoted by A (level of this build).
pub(crate) fn two() -> (tempfile::TempDir, PathBuf, Store, Store) {
    hooks::PROMOTION.with(|c| c.set(true));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let files = vec![path.display().to_string()];
    let config = |member: &str| IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: files.clone(),
        folder_key: KEY.into(),
        ..IntranetConfig::default()
    };
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&config("Casa")).unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    fsync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&config("Trabalho")).unwrap();
    fsync(&mut b);
    assert!(fsync(&mut a).promoted);
    fsync(&mut b);
    (dir, path, a, b)
}

pub(crate) fn fsync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
}

fn guard_of(dir: &Path, who: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}-guarda.ponte",
        dir.join(format!("{who}.sqlite3")).display()
    ))
}

fn nine(path: &Path) -> crate::sharedfolder::Copy {
    match crate::sharedfolder::read_any(&crate::sharedfolder::nine_path(path), KEY).unwrap() {
        Some(crate::sharedfolder::Found::Copy(copy)) => copy,
        _ => panic!("no copy of format 9"),
    }
}

/// R2-03, the journal half / frozen rule 4. A journal entry without refs
/// written above the lineage's `refs_since` (and not marked legacy) is a
/// damaged change: replayed from this computer's guard it becomes a card,
/// never a note written by number.
///
/// Red on `ea82162`: the refs of a journal entry are never looked at, so
/// the entry is replayed and the note is written.
#[test]
fn f3_0a2_refless_journal_entry_after_refs_since_is_a_card() {
    let (dir, path, mut a, mut b) = two();
    let doc = b.list_documents("").unwrap()[0].id;
    b.save_note(doc, "Nota de B").unwrap();
    fsync(&mut b);
    let since = nine(&path).payload.refs_since.expect("refs_since");
    let guard = guard_of(dir.path(), "b");
    let at = crate::sharedfolder::read_header(&guard)
        .unwrap()
        .unwrap()
        .revision;
    assert!(at > since, "B wrote after the first write of level 2");
    // A ref-less entry at B's revision, only in B's guard.
    crate::sharedfolder::reseal_for_test(&guard, KEY, crate::sharedfolder::LEVEL, |_, payload| {
        let entry = serde_json::json!({
            "uid": "f3a2-sem-provas-0000000000000001",
            "created_at": "2026-11-23T10:00:00Z",
            "actor": "Trabalho",
            "writer": "outro",
            "label": "Nota sem provas",
            "mutation": {"SaveNote": {"document_id": doc, "note": "Nota escrita pelo número", "expected_version": null}},
            "revision": at,
        });
        payload
            .journal
            .push(serde_json::from_value(entry).unwrap());
    })
    .unwrap();
    a.save_note(doc, "Nota de A").unwrap();
    fsync(&mut a);
    fsync(&mut b);
    assert_ne!(
        b.load_document(doc).unwrap().note,
        "Nota escrita pelo número"
    );
    let cards = b.list_sync_conflicts().unwrap();
    let card = cards
        .iter()
        .find(|c| c.mutation_uid == "f3a2-sem-provas-0000000000000001")
        .unwrap_or_else(|| panic!("{cards:?}"));
    // Damaged (rule 4), not any other refusal.
    assert!(
        card.reason.starts_with(crate::idrefs::DAMAGED),
        "{}",
        card.reason
    );
}

/// The settled ambiguity of `refs_since` (F3.0a2, "Desvios"): an entry an
/// older Ponte queued (no refs) and replayed by this build in a later write
/// lands in the journal above `refs_since`; it is marked legacy there, so
/// no receiver takes it for a damaged change, and the change is not lost.
///
/// Red on `ea82162`: the journal entry carries no mark.
#[test]
fn f3_0a2_legacy_queue_entry_is_marked_legacy_in_the_journal() {
    let (_dir, path, mut a, mut b) = two();
    let doc = b.list_documents("").unwrap()[0].id;
    b.save_note(doc, "Nota da fila antiga").unwrap();
    edit_last_queued(&b, |entry| entry.refs = None);
    let uid = b.list_offline_outbox().unwrap()[0].uid.clone();
    let report = fsync(&mut b);
    assert_eq!(report.conflicts, 0);
    let copy = nine(&path);
    let entry = copy
        .payload
        .journal
        .iter()
        .find(|e| e.uid == uid)
        .expect("in the journal");
    assert!(entry.revision > copy.payload.refs_since.unwrap());
    let raw = serde_json::to_value(entry).unwrap();
    assert!(raw.get("refs").is_none(), "{raw}");
    assert_eq!(raw.get("legacy"), Some(&serde_json::json!(true)), "{raw}");
    fsync(&mut a);
    assert_eq!(a.load_document(doc).unwrap().note, "Nota da fila antiga");
    assert!(a.list_sync_conflicts().unwrap().is_empty());
}

// ------------------------------------------- adendo §5 (R2-05) --

/// R2-05 / frozen rules 9 and 10. C creates X offline, then a note on X.
/// The main computer's effect commits and it falls before the rekey (a
/// crash: the connection drops with no answer), then it opens again. The
/// retry completes the rekey from the durable log, never executes again,
/// and the note resolves to X by C's identity (bound to X's, F3.0b).
///
/// Red on `ea82162`: the drain sends `idents: Vec::new()`
/// (`intranet.rs` ~4480), so X gets an identity of the main computer's
/// own and the note goes by number (the crash point is never reached).
#[test]
fn f3_0a2_host_crash_after_effect_before_rekey() {
    let mut t = Team::new();
    t.offline();
    let number = "SEFAZ-PRO-2026/00901";
    let x = t
        .client
        .import_capture(&capture(number, "Criado sem conexão."))
        .unwrap()
        .document_id;
    t.client.save_note(x, "Nota em X").unwrap();
    let mine = ident(&t.client, "document", x);
    t.online();
    hooks::CRASH_BEFORE_REKEY.arm(&t.db);
    let first = t.client.sync_from_host(false);
    let fired = !hooks::CRASH_BEFORE_REKEY.disarm(&t.db);
    assert!(fired, "the main computer fell at the rekey");
    // Before it opens again: X is there with the identity it got at the
    // insert, and its log is pending.
    let fallen: Vec<i64> = t
        .host
        .list_documents("")
        .unwrap()
        .into_iter()
        .filter(|d| d.document_number == number)
        .map(|d| d.id)
        .collect();
    assert_eq!(fallen.len(), 1, "the effect committed");
    assert_ne!(ident(&t.host, "document", fallen[0]), mine);
    assert!(count(&t.host, "SELECT count(*) FROM carry_log WHERE row_id>0") > 0);
    t.online();
    t.client.sync_from_host(false).unwrap();
    let on_host: Vec<i64> = t
        .host
        .list_documents("")
        .unwrap()
        .into_iter()
        .filter(|d| d.document_number == number)
        .map(|d| d.id)
        .collect();
    assert_eq!(on_host.len(), 1, "X is unique");
    // F3.0b: C is bound to the identity the insert gave X, never put in
    // its place (a Sync in between may have published it).
    let there = ident(&t.host, "document", on_host[0]);
    assert_ne!(there, mine);
    assert_eq!(
        crate::idents::alias_of(&t.host.conn, "document", &mine).unwrap(),
        Some(there),
        "C bound to X's identity"
    );
    assert_eq!(t.host.load_document(on_host[0]).unwrap().note, "Nota em X");
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    assert!(first.is_err(), "the main computer fell before the rekey");
    assert_eq!(count(&t.host, "SELECT count(*) FROM carry_log"), 0);
}

// ------------------------------------------- adendo §6 (R2-06) --

/// R2-06 / frozen rule 11. The answer to an `AddAttachment` is lost (it
/// does not check), the main computer deletes the document, and the retry
/// follows. The receipt is asked before any resolution: no card, no second
/// execution.
///
/// [v2]-guard: green on `ea82162` (the host finds its receipt); red on v2
/// as written, where the drain resolves the document's identity first and
/// makes a card.
#[test]
fn f3_0a2_lost_reply_target_deleted_retry_acknowledged() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    hooks::CORRUPT_MUTATED.arm(&t.db);
    let lost = t.client.add_attachment(
        crate::attachments::Owner::Document(doc),
        "perdido.pdf",
        "resposta perdida",
        b"%PDF-1.4 anexo",
    );
    hooks::CORRUPT_MUTATED.disarm(&t.db);
    assert!(lost.is_err());
    let queued = t.client.list_offline_outbox().unwrap();
    assert_eq!(queued.len(), 1);
    assert!(queued[0].attempted);
    assert_eq!(count(&t.host, "SELECT count(*) FROM attachments"), 1);
    t.host.delete_document(doc).unwrap();
    assert_eq!(hooks::rpc_count(&t.db, "Mutate"), 1);
    assert_eq!(hooks::rpc_count(&t.db, "ReceiptOf"), 0);
    t.client.sync_from_host(false).unwrap();
    // Settled by its receipt alone: asked once, never sent again.
    assert_eq!(hooks::rpc_count(&t.db, "ReceiptOf"), 1);
    assert_eq!(hooks::rpc_count(&t.db, "Mutate"), 1, "no second execution");
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(count(&t.host, "SELECT count(*) FROM attachments"), 0);
    assert!(t.client.list_documents("").unwrap().is_empty());
}

// ------------------------------------------------ revisão F3.0a2 --
//
// Each test reproduces a finding of `revisao-f3-0a2.md`; each is red on
// `3d26e5f` (said test by test) and green with its fix.

impl Team {
    /// Another client of the same main computer, synchronized once.
    pub(crate) fn join(&mut self, member: &str) -> Store {
        let mut other = Store::open(&self._dir.path().join(format!("{member}.sqlite3"))).unwrap();
        other
            .save_intranet_config(&IntranetConfig {
                mode: IntranetMode::Client,
                member_name: member.into(),
                host_url: format!("http://{}", self.server.as_ref().unwrap().local_addr()),
                cabinet_token: self.token.clone(),
                ..IntranetConfig::default()
            })
            .unwrap();
        other.sync_from_host(false).unwrap();
        other
    }

    fn client_db(&self) -> PathBuf {
        self._dir.path().join("client.sqlite3")
    }
}

fn task_of(store: &Store, id: i64) -> crate::tasks::Task {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

fn host_task_titled(store: &Store, title: &str) -> crate::tasks::Task {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.title == title)
        .unwrap()
}

pub(crate) fn note_of(store: &Store, doc: i64) -> String {
    store.load_document(doc).unwrap().note.trim().to_owned()
}

/// One shared file, A and B, promoted by A, with nothing in the library.
fn two_empty() -> (tempfile::TempDir, PathBuf, Store, Store) {
    hooks::PROMOTION.with(|c| c.set(true));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let files = vec![path.display().to_string()];
    let config = |member: &str| IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: files.clone(),
        folder_key: KEY.into(),
        ..IntranetConfig::default()
    };
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&config("Casa")).unwrap();
    fsync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&config("Trabalho")).unwrap();
    fsync(&mut b);
    fsync(&mut a);
    fsync(&mut b);
    (dir, path, a, b)
}

/// Finding 1 (BLOCKER) / frozen rules 5, 6 and 23. A 9/2 base with an
/// epoch and no triple at all (everything was deleted) is still a 9: a
/// replayed change with `Some(refs)` is checked and resolved, so a `Frame`
/// number of another frame becomes a card, never a note written by number.
///
/// Red on `3d26e5f`: `use_refs` is the base having triples, so the refs are
/// dropped, the change is taken as legacy and the note lands by number.
#[test]
fn f3_0a2r_empty_nine_base_still_resolves_the_proofs() {
    let (dir, path, mut a, mut b) = two_empty();
    let x = b
        .import_capture(&capture("SEFAZ-PRO-2026/00950", "Criado em B."))
        .unwrap()
        .document_id;
    b.save_note(x, "Nota de B").unwrap();
    fsync(&mut b);
    fsync(&mut a);
    let on_a = a.list_documents("").unwrap()[0].id;
    a.delete_document(on_a).unwrap();
    for attendance in a.list_saiop_attendances().unwrap() {
        a.delete_saiop_attendance(attendance.id).unwrap();
    }
    fsync(&mut a);
    let base = nine(&path);
    assert!(
        base.payload.snapshot.idents.is_empty(),
        "no triple left: {:?}",
        base.payload.snapshot.idents
    );
    assert!(!base.payload.snapshot.ident_epoch.is_empty(), "still a 9");
    assert_eq!(base.header.level, crate::sharedfolder::LEVEL);
    // B's guard still holds the creation and the note; under new uids they
    // are changes the base never applied, and the note's proof becomes a
    // `Frame` number (of B's frame, not of the copy being built).
    let guard = guard_of(dir.path(), "b");
    // Written over that empty 9/2, B's changes still carry their proofs.
    let written = crate::sharedfolder::read(&guard, KEY).unwrap().unwrap();
    let mine: Vec<_> = written
        .payload
        .journal
        .iter()
        .filter(|e| e.actor == "Trabalho")
        .map(|e| (e.label.clone(), e.refs.is_some(), e.legacy))
        .collect();
    assert_eq!(mine.len(), 2, "{mine:?}");
    assert!(
        mine.iter().all(|(_, refs, legacy)| *refs && !legacy),
        "{mine:?}"
    );
    let note_uid = "f3a2r-nota-pelo-numero-000000001".to_owned();
    crate::sharedfolder::reseal_for_test(&guard, KEY, crate::sharedfolder::LEVEL, |_, payload| {
        for entry in &mut payload.journal {
            match &mut entry.mutation {
                crate::intranet::IntranetMutation::ImportCapture { .. } => {
                    entry.uid = "f3a2r-criacao-de-novo-0000000001".into();
                }
                crate::intranet::IntranetMutation::SaveNote { note, .. } => {
                    entry.uid = note_uid.clone();
                    *note = "Nota escrita pelo número".into();
                    for site in &mut entry.refs.as_mut().expect("refs").mutation {
                        site.proof = Proof::Frame;
                    }
                }
                _ => {}
            }
        }
    })
    .unwrap();
    fsync(&mut b);
    let docs = b.list_documents("").unwrap();
    assert!(
        docs.iter()
            .all(|d| note_of(&b, d.id) != "Nota escrita pelo número"),
        "nothing written by number"
    );
    let cards = b.list_sync_conflicts().unwrap();
    let card = cards
        .iter()
        .find(|c| c.mutation_uid == note_uid)
        .unwrap_or_else(|| panic!("{cards:?}"));
    assert!(
        card.reason.starts_with(crate::idrefs::UNPROVEN),
        "{}",
        card.reason
    );
}

/// Finding 2 (BLOCKER) / frozen rule 10. The main computer commits the
/// effect of a carried creation and its log, then its rekey fails with an
/// error (it still answers). That is not a refusal: the client keeps the
/// entry (attempted), asks its receipt next time and the creation gets its
/// identity from the log; the note on it lands, no card.
///
/// Red on `3d26e5f`: the error after the effect is answered 400, so the
/// client cards the creation and drops it; the note becomes a card too.
#[test]
fn f3_0a2r_failure_after_the_effect_is_not_a_refusal() {
    let mut t = Team::new();
    t.offline();
    let number = "SEFAZ-PRO-2026/00951";
    let x = t
        .client
        .import_capture(&capture(number, "Criado sem conexão."))
        .unwrap()
        .document_id;
    t.client.save_note(x, "Nota em X").unwrap();
    let mine = ident(&t.client, "document", x);
    t.online();
    hooks::FAIL_REKEY_AT.arm(&t.db);
    let first = t.client.sync_from_host(false);
    let fired = !hooks::FAIL_REKEY_AT.disarm(&t.db);
    assert!(fired, "the rekey failed on the main computer");
    assert!(first.is_err(), "nothing settled yet");
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    let queue = t.client.list_offline_outbox().unwrap();
    assert_eq!(queue.len(), 2, "{queue:?}");
    assert!(queue[0].attempted);
    assert!(
        count(&t.host, "SELECT count(*) FROM carry_log WHERE row_id>0") > 0,
        "the log is pending"
    );
    t.client.sync_from_host(false).unwrap();
    let on_host: Vec<i64> = t
        .host
        .list_documents("")
        .unwrap()
        .into_iter()
        .filter(|d| d.document_number == number)
        .map(|d| d.id)
        .collect();
    assert_eq!(on_host.len(), 1, "X is unique");
    // F3.0b: C bound to the identity X kept there, never in its place.
    let there = ident(&t.host, "document", on_host[0]);
    assert_eq!(
        crate::idents::alias_of(&t.host.conn, "document", &mine).unwrap(),
        Some(there)
    );
    assert_eq!(note_of(&t.host, on_host[0]), "Nota em X");
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    assert_eq!(count(&t.host, "SELECT count(*) FROM carry_log"), 0);
}

/// Finding 2, the client's window: an entry is marked attempted, under a
/// valid uid, before its `Mutate` leaves (a crash right after the send
/// leaves it asking its receipt first).
///
/// Red on `3d26e5f`: the entry is marked attempted only after a lost
/// answer.
#[test]
fn f3_0a2r_attempted_is_persisted_before_the_send() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    t.offline();
    t.client.save_note(doc, "Nota da fila").unwrap();
    t.online();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None::<(bool, String)>));
    let (client_db, into) = (t.client_db(), seen.clone());
    hooks::arm_before_mutate(
        &t.db,
        Box::new(move |_| {
            let conn = rusqlite::Connection::open(&client_db).unwrap();
            let blob: Vec<u8> = conn
                .query_row("SELECT payload FROM offline_outbox", [], |r| r.get(0))
                .unwrap();
            let entry: OutboxEntry =
                serde_json::from_slice(&crate::protection::unseal(&blob).unwrap()).unwrap();
            *into.lock().unwrap() = Some((entry.attempted, entry.uid));
        }),
    );
    t.client.sync_from_host(false).unwrap();
    let (attempted, uid) = seen.lock().unwrap().clone().expect("the hook ran");
    assert!(attempted, "attempted before the send");
    assert_eq!(uid.len(), 32, "{uid}");
    assert_eq!(note_of(&t.host, doc), "Nota da fila");
}

/// Finding 3 (HIGH) / frozen rules 6, 9 and 11. A creates a task online:
/// the main computer saves it with identity H and the answer is lost, so A
/// applies it here with C. B takes H and edits the task (proving H) before
/// A asks the receipt. The receipt never replaces H (B may hold it): B's
/// next edit, still proving H, lands.
///
/// Red on `3d26e5f`: `rekey_inserted` gives the task C on the main
/// computer without a new revision; B is told it is up to date and its
/// edit proving H is refused.
#[test]
fn f3_0a2r_late_receipt_keeps_the_identity_already_published() {
    let mut t = Team::new();
    let mut b = t.join("SERVIDOR-B");
    let doc = t.client.list_documents("").unwrap()[0].id;
    hooks::DROP_REPLY.arm(&t.db);
    let local = t
        .client
        .save_task(&crate::tasks::Task {
            title: "Tarefa de A".into(),
            ..crate::tasks::Task::new(doc)
        })
        .unwrap();
    assert!(!hooks::DROP_REPLY.disarm(&t.db), "answer dropped");
    let queue = t.client.list_offline_outbox().unwrap();
    assert_eq!(queue.len(), 1);
    assert!(
        queue[0].attempted && !queue[0].idents.is_empty(),
        "{queue:?}"
    );
    let c = ident(&t.client, "task", local);
    let on_host = host_task_titled(&t.host, "Tarefa de A").id;
    let h = ident(&t.host, "task", on_host);
    assert_ne!(c, h);
    // B takes H and edits the task, between A's two requests.
    b.sync_from_host(false).unwrap();
    assert_eq!(ident(&b, "task", on_host), h);
    let mut theirs = task_of(&b, on_host);
    theirs.notes = "Primeira edição de B".into();
    b.save_task(&theirs).unwrap();
    // A asks its receipt.
    t.client.sync_from_host(false).unwrap();
    assert_eq!(ident(&t.host, "task", on_host), h, "H kept");
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    // B, told it is up to date or not, edits again proving H.
    b.sync_from_host(false).unwrap();
    let mut theirs = task_of(&b, on_host);
    theirs.notes = "Segunda edição de B".into();
    b.save_task(&theirs).unwrap();
    assert_eq!(task_of(&t.host, on_host).notes, "Segunda edição de B");
}

/// Finding 3, the dependents: A edits its task offline (proving C) before
/// its receipt is asked. C is bound to H by the receipt and the slot the
/// creation inserted (never by replacing H): the edit lands on the task,
/// and H stays.
///
/// Red on `3d26e5f`: the task's identity on the main computer becomes C.
#[test]
fn f3_0a2r_late_receipt_binds_the_dependents_without_rekey() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    hooks::DROP_REPLY.arm(&t.db);
    let local = t
        .client
        .save_task(&crate::tasks::Task {
            title: "Tarefa de A".into(),
            ..crate::tasks::Task::new(doc)
        })
        .unwrap();
    let on_host = host_task_titled(&t.host, "Tarefa de A").id;
    let h = ident(&t.host, "task", on_host);
    let mut mine = task_of(&t.client, local);
    mine.notes = "Notas de A".into();
    t.client.save_task(&mine).unwrap();
    assert_eq!(t.client.list_offline_outbox().unwrap().len(), 2);
    t.client.sync_from_host(false).unwrap();
    assert_eq!(ident(&t.host, "task", on_host), h, "H kept");
    assert_eq!(task_of(&t.host, on_host).notes, "Notas de A");
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
}

/// Finding 4 (HIGH) / frozen rule 9. C imports offline a capture new to
/// its library and unlinks the triage task made with it. The main computer
/// has that capture as an older version, and no task linked to the
/// document: the import there is a duplicate that inserts a triage task.
/// A duplicate gives no identity: the task keeps its own, the unlink of
/// C's triage task proves nothing there and becomes a card, and the task
/// stays linked. Both when the answer arrives and after a fall before the
/// rekey (completed from the log).
///
/// Red on `3d26e5f`: the inserted task takes C's triage identity (by the
/// rekey, or by `complete_carry` as the only task in the log) and the
/// unlink lands.
#[test]
fn f3_0a2r_duplicate_import_lends_no_identity() {
    duplicate_import_case(false);
}

#[test]
fn f3_0a2r_duplicate_import_lends_no_identity_after_a_fall() {
    duplicate_import_case(true);
}

fn duplicate_import_case(fall: bool) {
    let mut t = Team::new();
    let number = "SEFAZ-PRO-2026/00960";
    let older = capture(number, "Primeira versão.");
    let doc = t.host.import_capture(&older).unwrap().document_id;
    let mut newer = capture(number, "Segunda versão.");
    newer.captured_at = "2026-11-05T12:00:00Z".into();
    t.host.import_capture(&newer).unwrap();
    for task in t.host.list_tasks().unwrap() {
        if task.document_id == Some(doc) {
            t.host.relink_task(task.id, None, None).unwrap();
        }
    }
    t.offline();
    let mut again = older.clone();
    again.request_id = uuid::Uuid::new_v4().to_string();
    let mine = t.client.import_capture(&again).unwrap().document_id;
    let triage = t
        .client
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|task| task.document_id == Some(mine))
        .expect("triage task")
        .id;
    let triage_ident = ident(&t.client, "task", triage);
    t.client.relink_task(triage, None, None).unwrap();
    t.online();
    if fall {
        hooks::CRASH_BEFORE_REKEY.arm(&t.db);
        assert!(t.client.sync_from_host(false).is_err());
        assert!(!hooks::CRASH_BEFORE_REKEY.disarm(&t.db), "fell");
        t.online();
    }
    t.client.sync_from_host(false).unwrap();
    let linked: Vec<i64> = t
        .host
        .list_tasks()
        .unwrap()
        .into_iter()
        .filter(|task| task.document_id == Some(doc))
        .map(|task| task.id)
        .collect();
    assert_eq!(linked.len(), 1, "the duplicate's triage task, still linked");
    assert_ne!(ident(&t.host, "task", linked[0]), triage_ident);
    let held: i64 = t
        .host
        .conn
        .query_row(
            "SELECT count(*) FROM item_ident WHERE ident=?1",
            [&triage_ident],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(held, 0, "C's triage identity is on no row");
    let cards = t.client.list_sync_conflicts().unwrap();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(matches!(
        cards[0].mutation,
        crate::intranet::IntranetMutation::RelinkTask { .. }
    ));
}

/// Finding 5 (HIGH) / frozen rule 7. A resource use is the item of its
/// change by its uid: moving its link from document A to B (or dropping
/// it) offline is an edit of the same use, not a base about another item.
///
/// Red on `3d26e5f`: the first site (A in the base, B or none in the
/// change) is compared, so the edit becomes a card.
#[test]
fn f3_0a2r_resource_use_relink_is_the_same_item() {
    let mut t = Team::new();
    let a_doc = t.host.list_documents("").unwrap()[0].id;
    let b_doc = t
        .host
        .import_capture(&capture("SEFAZ-PRO-2026/00970", "Documento B."))
        .unwrap()
        .document_id;
    let room = t
        .host
        .save_resource(
            &crate::resources::Resource {
                name: "Sala de reuniões".into(),
                kind: "sala".into(),
                quantity: 1,
                ..crate::resources::Resource::default()
            },
            None,
        )
        .unwrap();
    let uid = t
        .host
        .save_resource_use(
            &crate::resources::ResourceUse {
                resource_uid: room,
                who: "Servidor A".into(),
                start: "2099-01-05T09:00".into(),
                end: "2099-01-05T10:00".into(),
                document_id: Some(a_doc),
                ..crate::resources::ResourceUse::default()
            },
            None,
        )
        .unwrap();
    t.client.sync_from_host(true).unwrap();
    let use_on = |store: &Store| {
        store
            .list_resource_uses()
            .unwrap()
            .into_iter()
            .find(|u| u.uid == uid)
            .unwrap()
    };
    for target in [Some(b_doc), None] {
        t.offline();
        let mut item = use_on(&t.client);
        item.document_id = target;
        t.client.save_resource_use(&item, None).unwrap();
        t.online();
        t.client.sync_from_host(false).unwrap();
        assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
        assert_eq!(use_on(&t.host).document_id, target);
    }
}

/// Finding 6 (HIGH). An entry an older Ponte queued without uid gets a
/// stable uuid before its first attempt: when that answer is lost, its
/// receipt settles it and the queue goes on.
///
/// Red on `3d26e5f`: the stand-in `legacy-{created_at}-{id}` is not a
/// valid uid, so the retry's `ReceiptOf` is refused and the drain stops
/// there at every exchange.
#[test]
fn f3_0a2r_pre_uid_entry_survives_a_lost_answer() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    t.offline();
    t.client.save_note(doc, "Nota da fila antiga").unwrap();
    edit_last_queued(&t.client, |entry| {
        entry.uid = String::new();
        entry.refs = None;
    });
    t.online();
    hooks::CORRUPT_MUTATED.arm(&t.db);
    assert!(t.client.sync_from_host(false).is_err(), "answer unusable");
    hooks::CORRUPT_MUTATED.disarm(&t.db);
    let queue = t.client.list_offline_outbox().unwrap();
    assert_eq!(queue.len(), 1);
    assert!(queue[0].attempted);
    t.client.sync_from_host(false).unwrap();
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    assert_eq!(note_of(&t.host, doc), "Nota da fila antiga");
    assert_eq!(hooks::rpc_count(&t.db, "Mutate"), 1, "applied once");
}

/// Finding 6, an entry already attempted under an invalid uid: nothing
/// proves whether it was applied. It becomes a card of uncertain state
/// (never sent again by number) and the entries after it go on.
///
/// Red on `3d26e5f`: its `ReceiptOf` is refused and the drain stops.
#[test]
fn f3_0a2r_attempted_invalid_uid_is_an_uncertain_card() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    let other = t
        .host
        .import_capture(&capture("SEFAZ-PRO-2026/00980", "Outro."))
        .unwrap()
        .document_id;
    t.client.sync_from_host(true).unwrap();
    t.offline();
    t.client.save_note(doc, "Nota incerta").unwrap();
    edit_last_queued(&t.client, |entry| {
        entry.uid = "legacy-2026-11-23T10:00:00Z-1".into();
        entry.refs = None;
        entry.attempted = true;
    });
    t.client.save_note(other, "Nota seguinte").unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(note_of(&t.host, other), "Nota seguinte");
    assert_ne!(note_of(&t.host, doc), "Nota incerta");
    let cards = t.client.list_sync_conflicts().unwrap();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(cards[0].unresolved);
    assert!(matches!(
        &cards[0].mutation,
        crate::intranet::IntranetMutation::SaveNote { note, .. } if note == "Nota incerta"
    ));
}

/// Finding 7 (HIGH) / frozen rule 23, online. The client still shows X;
/// another device deleted it. A note saved on X is refused by its proof
/// there: it becomes a card holding the note, never reapplied by number.
///
/// Red on `3d26e5f`: the error goes back to the caller and no card keeps
/// the note.
#[test]
fn f3_0a2r_proof_refusal_online_is_a_card() {
    let mut t = Team::new();
    let x = t
        .host
        .import_capture(&capture("SEFAZ-PRO-2026/00990", "Será apagado."))
        .unwrap()
        .document_id;
    t.client.sync_from_host(true).unwrap();
    t.host.delete_document(x).unwrap();
    assert!(t.client.save_note(x, "Nota sobre X").is_err());
    let cards = t.client.list_sync_conflicts().unwrap();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(cards[0].unresolved, "never by number");
    assert_eq!(cards[0].reason, crate::idrefs::UNPROVEN);
    assert!(matches!(
        &cards[0].mutation,
        crate::intranet::IntranetMutation::SaveNote { note, .. } if note == "Nota sobre X"
    ));
}

/// Finding 7, in the drain: X is deleted on the main computer between the
/// client's read of its frame and the change's arrival. The proof refusal
/// is a card of the original change, unresolved.
///
/// Red on `3d26e5f`: the card takes the path of an ordinary refusal
/// (`unresolved=false`).
#[test]
fn f3_0a2r_proof_refusal_in_the_drain_is_unresolved() {
    let mut t = Team::new();
    let x = t
        .host
        .import_capture(&capture("SEFAZ-PRO-2026/00991", "Será apagado."))
        .unwrap()
        .document_id;
    t.client.sync_from_host(true).unwrap();
    t.offline();
    t.client.save_note(x, "Nota na fila").unwrap();
    t.online();
    hooks::arm_before_mutate(
        &t.db,
        Box::new(move |host| host.delete_document(x).unwrap()),
    );
    t.client.sync_from_host(false).unwrap();
    assert!(!hooks::before_mutate_armed(&t.db), "hook ran");
    let cards = t.client.list_sync_conflicts().unwrap();
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(cards[0].unresolved, "never by number");
    assert_eq!(cards[0].reason, crate::idrefs::UNPROVEN);
    assert!(matches!(
        &cards[0].mutation,
        crate::intranet::IntranetMutation::SaveNote { document_id, note, .. }
            if note == "Nota na fila" && *document_id == x
    ));
}

/// The previous fixer's deviation (rule 23): an entry kept as it is (an
/// older Ponte's) and a later edit of the same item expecting the version
/// it leaves. When the kept one becomes a card, the later one is a card
/// too: it never lands over another device's version.
///
/// Red on `3d26e5f`: the later note expects `v+1`, which is the other
/// device's version, and overwrites it.
#[test]
fn f3_0a2r_follower_of_a_carded_entry_is_carded_in_the_drain() {
    let mut t = Team::new();
    let doc = t.client.list_documents("").unwrap()[0].id;
    t.offline();
    t.client.save_note(doc, "Primeira nota de A").unwrap();
    edit_last_queued(&t.client, |entry| entry.refs = None);
    t.client.save_note(doc, "Segunda nota de A").unwrap();
    assert_eq!(t.client.list_offline_outbox().unwrap().len(), 2);
    t.host.save_note(doc, "Nota de outro aparelho").unwrap();
    t.online();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(note_of(&t.host, doc), "Nota de outro aparelho");
    assert_eq!(t.cards(), 2, "{:?}", t.client.list_sync_conflicts());
}

/// The same chain in the replay of a library's own queue in the folders.
///
/// Red on `3d26e5f`: the later note lands over A's.
#[test]
fn f3_0a2r_follower_of_a_carded_entry_is_carded_in_the_folders() {
    let (_dir, _path, mut a, mut b) = two();
    let doc = b.list_documents("").unwrap()[0].id;
    b.save_note(doc, "Primeira nota de B").unwrap();
    edit_last_queued(&b, |entry| entry.refs = None);
    b.save_note(doc, "Segunda nota de B").unwrap();
    assert_eq!(b.list_offline_outbox().unwrap().len(), 2);
    a.save_note(doc, "Nota de A").unwrap();
    fsync(&mut a);
    fsync(&mut b);
    assert_eq!(note_of(&b, doc), "Nota de A");
    assert_eq!(b.list_sync_conflicts().unwrap().len(), 2);
}

/// Test weakness 3 of the review: a legacy entry (marked in the journal)
/// that arrives from a divergent journal is replayed by number, no card.
///
/// Guard: green on `3d26e5f` (the mark is honored); red if the mark were
/// ignored (the entry, above `refs_since` without refs, would be a card).
#[test]
fn f3_0a2r_legacy_entry_from_a_divergent_journal_is_replayed() {
    let (dir, _path, mut a, mut b) = two();
    let doc = b.list_documents("").unwrap()[0].id;
    b.save_note(doc, "Nota da fila antiga").unwrap();
    edit_last_queued(&b, |entry| entry.refs = None);
    fsync(&mut b);
    // Only in B's guard, under a new uid: a change the folder never saw.
    let guard = guard_of(dir.path(), "b");
    let uid = "f3a2r-legada-divergente-00000001".to_owned();
    crate::sharedfolder::reseal_for_test(&guard, KEY, crate::sharedfolder::LEVEL, |_, payload| {
        let entry = payload
            .journal
            .iter_mut()
            .find(|e| e.legacy)
            .expect("the legacy entry");
        entry.uid = uid.clone();
        entry.label = "Nota legada divergente".into();
        if let crate::intranet::IntranetMutation::SaveNote {
            note,
            expected_version,
            ..
        } = &mut entry.mutation
        {
            *note = "Nota legada divergente".into();
            *expected_version = None;
        }
    })
    .unwrap();
    a.import_capture(&capture("SEFAZ-PRO-2026/00999", "De A."))
        .unwrap();
    fsync(&mut a);
    fsync(&mut b);
    assert!(
        b.list_sync_conflicts().unwrap().is_empty(),
        "{:?}",
        b.list_sync_conflicts()
    );
    assert_eq!(note_of(&b, doc), "Nota legada divergente");
    fsync(&mut a);
    assert_eq!(note_of(&a, doc), "Nota legada divergente");
}

// ------------------------------------- F3.0b, conserto trazido do F3.0a2 --

/// The completion after a fall has the same hazard as the late receipt
/// (review F3.0a2, finding 3): between the effect and the completion, a
/// `Sync` may already have published the identity H the insert gave X.
/// The completion keeps H and binds C (the sender's) to it, first binding
/// wins: B's edit proving H lands after the completion, and A's note,
/// proving C, resolves through the binding.
///
/// Red on `3611b80`: `complete_carry` replaces H by C on the main
/// computer, and B's edit proving H is refused by the proof (a card).
#[test]
fn f3_0b_completion_after_a_fall_keeps_the_published_identity() {
    let mut t = Team::new();
    t.offline();
    let number = "SEFAZ-PRO-2026/00902";
    let x = t
        .client
        .import_capture(&capture(number, "Criado sem conexão."))
        .unwrap()
        .document_id;
    t.client.save_note(x, "Nota em X").unwrap();
    let c = ident(&t.client, "document", x);
    t.online();
    hooks::CRASH_BEFORE_REKEY.arm(&t.db);
    assert!(t.client.sync_from_host(false).is_err());
    assert!(
        !hooks::CRASH_BEFORE_REKEY.disarm(&t.db),
        "fell at the rekey"
    );
    t.online();
    let mut b = t.join("SERVIDOR-B");
    let on_host = t
        .host
        .list_documents("")
        .unwrap()
        .into_iter()
        .find(|d| d.document_number == number)
        .unwrap()
        .id;
    let h = ident(&t.host, "document", on_host);
    assert_ne!(h, c);
    assert_eq!(ident(&b, "document", on_host), h, "B took H");
    // A completes its change and sends its note.
    t.client.sync_from_host(false).unwrap();
    assert_eq!(ident(&t.host, "document", on_host), h, "H kept");
    assert_eq!(t.host.load_document(on_host).unwrap().note, "Nota em X");
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    assert_eq!(count(&t.host, "SELECT count(*) FROM carry_log"), 0);
    // B, still at H, edits X.
    b.sync_from_host(false).unwrap();
    b.save_task(&crate::tasks::Task {
        title: "Tarefa de B em X".into(),
        ..crate::tasks::Task::new(on_host)
    })
    .unwrap();
    assert!(b.list_sync_conflicts().unwrap().is_empty());
    assert_eq!(
        host_task_titled(&t.host, "Tarefa de B em X").document_id,
        Some(on_host)
    );
}
