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
struct Team {
    _dir: tempfile::TempDir,
    host: Store,
    client: Store,
    db: PathBuf,
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

    /// Starts (again) the main computer's server: a new process, as after
    /// a crash.
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

    fn cards(&self) -> usize {
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

fn count(store: &Store, sql: &str) -> i64 {
    store.conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// Rewrites the last queued change of `store` with `edit` (as a damaged
/// change, or an older Ponte's, would have been queued).
fn edit_last_queued(store: &Store, edit: impl FnOnce(&mut OutboxEntry)) {
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
fn two() -> (tempfile::TempDir, PathBuf, Store, Store) {
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

fn fsync(store: &mut Store) -> crate::intranet::FolderSyncReport {
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
    assert!(
        b.list_sync_conflicts()
            .unwrap()
            .iter()
            .any(|c| c.mutation_uid == "f3a2-sem-provas-0000000000000001"),
        "{:?}",
        b.list_sync_conflicts().unwrap()
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
/// and the note resolves to X by C's identity.
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
    *hooks::CRASH_BEFORE_REKEY.lock().unwrap() = Some(t.db.clone());
    let first = t.client.sync_from_host(false);
    *hooks::CRASH_BEFORE_REKEY.lock().unwrap() = None;
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
    assert_eq!(ident(&t.host, "document", on_host[0]), mine, "C's identity");
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
    *hooks::CORRUPT_MUTATED.lock().unwrap() = Some(t.db.clone());
    let lost = t.client.add_attachment(
        crate::attachments::Owner::Document(doc),
        "perdido.pdf",
        "resposta perdida",
        b"%PDF-1.4 anexo",
    );
    *hooks::CORRUPT_MUTATED.lock().unwrap() = None;
    assert!(lost.is_err());
    let queued = t.client.list_offline_outbox().unwrap();
    assert_eq!(queued.len(), 1);
    assert!(queued[0].attempted);
    assert_eq!(count(&t.host, "SELECT count(*) FROM attachments"), 1);
    t.host.delete_document(doc).unwrap();
    t.client.sync_from_host(false).unwrap();
    assert_eq!(t.cards(), 0, "{:?}", t.client.list_sync_conflicts());
    assert!(t.client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(count(&t.host, "SELECT count(*) FROM attachments"), 0);
    assert!(t.client.list_documents("").unwrap().is_empty());
}
