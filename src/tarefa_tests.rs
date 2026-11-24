//! A task form open while the shared folder is exchanged (0.52.0, defect
//! seen on the owner's computers): the version the form expects, the
//! version the exchange leaves in this library, and the question of the
//! unsaved task ("Salvar e continuar"). Tests only.
#![allow(clippy::all)]

use crate::intranet::{IntranetConfig, IntranetMode};
use crate::storage::Store;
use crate::tasks::Task;
use crate::tasks_ui::TaskPanel;

const KEY: &str = "senha-de-teste-123";
/// The owner's name, the same on every computer.
const OWNER: &str = "CARLO";

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

fn task_version(store: &Store, id: i64) -> i64 {
    store.item_version("task", id).unwrap().version
}

fn get(store: &Store, id: i64) -> Task {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

/// Two computers of the same person (same member name) on one folder, both
/// with the same task and a document.
struct Pair {
    _dir: tempfile::TempDir,
    a: Store,
    b: Store,
    task: i64,
    document: i64,
}

const TITLE: &str = "BNBO: solicitar o cruzamento dos BOs recusados";

fn pair() -> Pair {
    crate::idents::hooks::PROMOTION.with(|c| c.set(true));
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config(OWNER, files.clone()))
        .unwrap();
    let document = a
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    let mut t = Task::blank(None);
    t.title = TITLE.into();
    let task = a.save_task(&t).unwrap();
    sync(&mut a);
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config(OWNER, files)).unwrap();
    sync(&mut b);
    sync(&mut a);
    assert_eq!(task_version(&a, task), task_version(&b, task));
    Pair {
        _dir: dir,
        a,
        b,
        task,
        document,
    }
}

/// Saves the open task twice between exchanges, each save on the version
/// the previous one left (what the form does), and gives that version.
fn save_twice(store: &mut Store, id: i64) -> i64 {
    let mut base = task_version(store, id);
    for notes in ["primeira", "segunda"] {
        let mut t = get(store, id);
        t.notes = notes.into();
        store.save_task_checked(&t, Some(base)).unwrap();
        base = task_version(store, id);
    }
    base
}

/// (a) The cause. Two saves of the open task between exchanges are merged
/// into one queued change; the folder replays it once, so the exchange
/// brought this library's version of the task BACK (3 → 2), below the
/// version the open form holds. Nobody else touched the task, yet the next
/// save was refused as "Conflito de edição simultânea … por CARLO".
#[test]
fn an_exchange_after_two_saves_keeps_the_version_the_form_holds() {
    let mut p = pair();
    let id = p.task;
    let base = save_twice(&mut p.a, id);
    sync(&mut p.a);
    assert_eq!(
        task_version(&p.a, id),
        base,
        "this computer's own change, back from the folder, is no edit by anyone"
    );
    let mut t = get(&p.a, id);
    t.notes = "terceira".into();
    p.a.save_task_checked(&t, Some(base))
        .expect("no simultaneous edit happened");
    // The other computer gets the same number, so its next edit is the
    // one that moves it on.
    sync(&mut p.a);
    sync(&mut p.b);
    assert_eq!(task_version(&p.b, id), task_version(&p.a, id));
    assert_eq!(get(&p.b, id).notes, "terceira");
}

/// (a) What the version going back made possible: the same number reached
/// again by a real edit of another computer, and the form that still held
/// it saved over that edit without any question.
#[test]
fn another_computers_edit_is_never_overwritten_by_a_form_opened_before_it() {
    let mut p = pair();
    let id = p.task;
    let base = save_twice(&mut p.a, id);
    sync(&mut p.a);
    sync(&mut p.b);
    // The other computer (the same person, the same name) changes the deadline.
    let mut there = get(&p.b, id);
    there.due_date = "2026-12-01".into();
    let v = task_version(&p.b, id);
    p.b.save_task_checked(&there, Some(v)).unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    assert_eq!(get(&p.a, id).due_date, "2026-12-01");
    // The form of this computer, opened before that edit, still holds
    // `base` and the task as it was then.
    let mut t = get(&p.a, id);
    t.due_date.clear();
    t.notes = "terceira".into();
    let saved = p.a.save_task_checked(&t, Some(base));
    assert!(
        saved.is_err(),
        "a save made on top of the version before the other edit must be refused"
    );
    assert_eq!(get(&p.a, id).due_date, "2026-12-01");
}

/// (a) The same merge of queued edits exists for the other kinds whose
/// screens hold a version: the note of a document and a project.
#[test]
fn the_note_of_a_document_and_a_project_keep_their_version_too() {
    let mut p = pair();
    let doc = p.document;
    let mut base = p.a.item_version("note", doc).unwrap().version;
    for note in ["um", "dois"] {
        p.a.save_note_checked(doc, note, Some(base)).unwrap();
        base = p.a.item_version("note", doc).unwrap().version;
    }
    let project = p
        .a
        .save_project(&crate::projects::Project::new("Projeto X"))
        .unwrap();
    sync(&mut p.a);
    let mut pbase = p.a.project_version(&project.uid).unwrap();
    let mut edited = p.a.get_project(&project.uid).unwrap().unwrap();
    for objective in ["um", "dois"] {
        edited.objective = objective.into();
        edited = p.a.save_project_checked(&edited, Some(pbase)).unwrap();
        pbase = p.a.project_version(&project.uid).unwrap();
    }
    sync(&mut p.a);
    assert_eq!(p.a.item_version("note", doc).unwrap().version, base);
    p.a.save_note_checked(doc, "três", Some(base))
        .expect("no other edit of the note happened");
    assert_eq!(p.a.project_version(&project.uid).unwrap(), pbase);
    edited.objective = "três".into();
    p.a.save_project_checked(&edited, Some(pbase))
        .expect("no other edit of the project happened");
}

/// Changes the version of the task the way an exchange does when it
/// rewrites a row with the same content: a new version, the same task.
fn bump_without_change(store: &mut Store, id: i64) {
    let tx = store.conn.transaction().unwrap();
    crate::intranet::record_mutation(&tx, OWNER, "task", id, None, "teste", None, "").unwrap();
    tx.commit().unwrap();
}

/// (b) "Salvar e continuar" after a change of version that changed
/// nothing of the task: the save goes through (nothing of anyone is
/// overwritten: the task is exactly what the form opened), and a retry is
/// never a dead end.
#[test]
fn salvar_e_continuar_goes_through_when_only_the_version_changed() {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    panel.set_draft_title("BNBO: solicitar o cruzamento — revisado");
    bump_without_change(&mut p.a, id);
    let first = panel.save_draft(&mut p.a);
    let retry = if first.is_err() {
        panel.save_draft(&mut p.a)
    } else {
        Ok(())
    };
    assert!(
        first.is_ok(),
        "the task did not change: first try {first:?}, retry {retry:?}"
    );
    assert!(!panel.dirty());
    assert_eq!(get(&p.a, id).title, "BNBO: solicitar o cruzamento — revisado");
}

/// (b) The same, through the real path: two saves, an exchange, and then
/// the question of the unsaved task.
#[test]
fn salvar_e_continuar_after_two_saves_and_an_exchange() {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    panel.set_draft_title("primeira");
    panel.save_draft(&mut p.a).unwrap();
    panel.set_draft_title("segunda");
    panel.save_draft(&mut p.a).unwrap();
    sync(&mut p.a);
    panel.set_draft_title("terceira");
    let first = panel.save_draft(&mut p.a);
    let retry = if first.is_err() {
        panel.save_draft(&mut p.a)
    } else {
        Ok(())
    };
    assert!(first.is_ok(), "first {first:?}, retry {retry:?}");
    assert_eq!(get(&p.a, id).title, "terceira");
}

/// (b) A real edit made elsewhere while the form was open: the save is
/// refused (nothing of the other edit is overwritten), the typing stays,
/// and "Descartar alterações" shows the task as it is now, not as it was
/// when the form opened.
#[test]
fn a_real_edit_elsewhere_is_never_overwritten_and_discarding_shows_it() {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    panel.set_draft_title("o título que eu digitei");
    let mut there = get(&p.b, id);
    there.due_date = "2026-12-01".into();
    let v = task_version(&p.b, id);
    p.b.save_task_checked(&there, Some(v)).unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    let error = panel.save_draft(&mut p.a).unwrap_err();
    assert!(error.contains("Conflito") || error.contains("alterada"), "{error}");
    assert!(panel.dirty(), "the typing stays");
    assert_eq!(get(&p.a, id).due_date, "2026-12-01");
    assert_eq!(get(&p.a, id).title, TITLE);
    panel.discard_draft();
    assert!(!panel.dirty());
    // What the form shows after discarding is the task as it is now.
    panel.set_draft_title("de novo");
    panel
        .save_draft(&mut p.a)
        .expect("after discarding, the form holds the current version");
    let now = get(&p.a, id);
    assert_eq!(now.due_date, "2026-12-01", "the other edit survives");
    assert_eq!(now.title, "de novo");
}
