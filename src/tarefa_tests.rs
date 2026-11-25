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

pub(crate) fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
}

pub(crate) fn task_version(store: &Store, id: i64) -> i64 {
    store.item_version("task", id).unwrap().version
}

pub(crate) fn get(store: &Store, id: i64) -> Task {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

/// Two computers of the same person (same member name) on one folder, both
/// with the same task and a document.
pub(crate) struct Pair {
    _dir: tempfile::TempDir,
    pub(crate) drive: std::path::PathBuf,
    pub(crate) a: Store,
    pub(crate) b: Store,
    pub(crate) task: i64,
    pub(crate) document: i64,
}

const TITLE: &str = "BNBO: solicitar o cruzamento dos BOs recusados";

pub(crate) fn pair() -> Pair {
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
    b.save_intranet_config(&folder_config(OWNER, files))
        .unwrap();
    sync(&mut b);
    sync(&mut a);
    assert_eq!(task_version(&a, task), task_version(&b, task));
    Pair {
        _dir: dir,
        drive,
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
    let project =
        p.a.save_project(&crate::projects::Project::new("Projeto X"))
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
    assert_eq!(
        get(&p.a, id).title,
        "BNBO: solicitar o cruzamento — revisado"
    );
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
    assert!(
        error.contains("Conflito") || error.contains("alterada"),
        "{error}"
    );
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

/// A real edit elsewhere and the person's typing in other fields: "Juntar
/// as duas versões" brings what changed there into the form, keeps what
/// was typed, leaves it unsaved for review, and the save keeps both. The
/// sentence says where it came from in words that make sense when the
/// name is the person's own.
#[test]
fn juntar_brings_the_other_edit_into_the_form_and_keeps_the_typing() {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    panel.set_draft_title("o título que eu digitei");
    let mut there = get(&p.b, id);
    there.due_date = "2026-12-01".into();
    there.notes = "anotado no outro computador".into();
    let v = task_version(&p.b, id);
    p.b.save_task_checked(&there, Some(v)).unwrap();
    sync(&mut p.b);
    sync(&mut p.a);

    let error = panel.save_draft(&mut p.a).unwrap_err();
    assert!(
        error.contains("com o seu nome (CARLO) em outro computador, no celular ou em outra tela"),
        "{error}"
    );
    assert!(
        error.contains("Prazo") && error.contains("Observações"),
        "{error}"
    );
    assert!(
        !error.contains("Título"),
        "the title did not change there: {error}"
    );
    let conflict = panel.edit_conflict().expect("the version found is kept");
    assert_eq!(conflict.current.due_date, "2026-12-01");
    // Saving again before choosing writes nothing over the other edit.
    assert!(panel.save_draft(&mut p.a).is_err());
    assert_eq!(get(&p.a, id).title, TITLE);

    assert!(panel.merge_conflict());
    assert!(panel.edit_conflict().is_none());
    assert!(
        panel.dirty(),
        "joined, it waits for the person to review and save"
    );
    assert_eq!(
        panel.editing_title().as_deref(),
        Some("o título que eu digitei")
    );
    panel
        .save_draft(&mut p.a)
        .expect("on the current version now");
    let now = get(&p.a, id);
    assert_eq!(now.title, "o título que eu digitei");
    assert_eq!(now.due_date, "2026-12-01");
    assert_eq!(now.notes, "anotado no outro computador");
    sync(&mut p.a);
    sync(&mut p.b);
    let on_b = get(&p.b, id);
    assert_eq!(on_b.title, "o título que eu digitei");
    assert_eq!(on_b.due_date, "2026-12-01");
}

/// The same field changed on both sides: the join keeps the person's
/// value and names the other one, so nothing is lost unseen.
#[test]
fn a_field_changed_on_both_sides_is_named_with_the_other_value() {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    panel.set_draft_title("o meu título");
    let mut there = get(&p.b, id);
    there.title = "o título de lá".into();
    let v = task_version(&p.b, id);
    p.b.save_task_checked(&there, Some(v)).unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    assert!(panel.save_draft(&mut p.a).is_err());
    assert!(panel.merge_conflict());
    assert_eq!(panel.editing_title().as_deref(), Some("o meu título"));
    // The notice of the join says what the other side had.
    let notice = panel.message_for_test();
    assert!(
        notice.contains("Título (lá estava “o título de lá”)"),
        "{notice}"
    );
    panel.save_draft(&mut p.a).unwrap();
    assert_eq!(get(&p.a, id).title, "o meu título");
}

/// "Ficar com a versão de lá" (and "Descartar as minhas" in the question)
/// shows the task as saved now; the typing goes only by that choice.
#[test]
fn ficar_com_a_versao_de_la_shows_the_saved_task() {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    panel.set_draft_title("digitado");
    let mut there = get(&p.b, id);
    there.due_date = "2026-12-01".into();
    let v = task_version(&p.b, id);
    p.b.save_task_checked(&there, Some(v)).unwrap();
    sync(&mut p.b);
    sync(&mut p.a);
    assert!(panel.save_draft(&mut p.a).is_err());
    assert!(panel.take_theirs());
    assert!(!panel.dirty());
    assert!(panel.edit_conflict().is_none());
    panel.set_draft_title("agora sim");
    panel.save_draft(&mut p.a).unwrap();
    let now = get(&p.a, id);
    assert_eq!(
        (now.title.as_str(), now.due_date.as_str()),
        ("agora sim", "2026-12-01")
    );
}

/// The refusal of the library itself (what projects, notes and the other
/// screens show) says where the edit came from when the name is the
/// person's own, and keeps naming another member as before.
#[test]
fn the_refusal_names_the_place_when_the_author_is_the_person_itself() {
    let mut p = pair();
    let id = p.task;
    let base = task_version(&p.a, id);
    bump_without_change(&mut p.a, id);
    let mut t = get(&p.a, id);
    t.notes = "x".into();
    let own =
        p.a.save_task_checked(&t, Some(base))
            .unwrap_err()
            .to_string();
    assert!(
        own.starts_with("Conflito de edição simultânea: este registro já foi atualizado com o seu nome (CARLO) em outro computador, no celular ou em outra tela (versão #"),
        "{own}"
    );
    let tx = p.a.conn.transaction().unwrap();
    crate::intranet::record_mutation(&tx, "Ana", "task", id, None, "teste", None, "").unwrap();
    tx.commit().unwrap();
    let other =
        p.a.save_task_checked(&t, Some(base + 1))
            .unwrap_err()
            .to_string();
    assert!(
        other.starts_with(
            "Conflito de edição simultânea: este registro já foi atualizado por Ana (versão #"
        ),
        "{other}"
    );
}

/// Main computer and a computer of the same person, offline in between:
/// two saves queued there are merged into one change, and the main
/// computer leaves the task at the version they left there.
#[test]
fn the_main_computer_leaves_merged_saves_at_the_senders_version() {
    let dir = tempfile::tempdir().unwrap();
    let host_db = dir.path().join("host.sqlite3");
    let token = crate::intranet::generate_token();
    let mut host = Store::open(&host_db).unwrap();
    host.save_intranet_config(&IntranetConfig {
        mode: IntranetMode::Host,
        member_name: OWNER.into(),
        bind_addr: "127.0.0.1:0".into(),
        cabinet_token: token.clone(),
        ..IntranetConfig::default()
    })
    .unwrap();
    let mut t = Task::blank(None);
    t.title = TITLE.into();
    let id = host.save_task(&t).unwrap();
    let mut server = crate::intranet::HostServer::start(host_db.clone(), "127.0.0.1:0").unwrap();
    let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
    client
        .save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Client,
            member_name: OWNER.into(),
            host_url: format!("http://{}", server.local_addr()),
            cabinet_token: token,
            ..IntranetConfig::default()
        })
        .unwrap();
    assert!(client.sync_from_host(false).unwrap());
    server.stop();
    let mut base = task_version(&client, id);
    for notes in ["primeira", "segunda"] {
        let mut t = get(&client, id);
        t.notes = notes.into();
        client.save_task_checked(&t, Some(base)).unwrap();
        base = task_version(&client, id);
    }
    assert_eq!(client.list_offline_outbox().unwrap().len(), 1, "merged");
    let mut server = crate::intranet::HostServer::start(host_db.clone(), "127.0.0.1:0").unwrap();
    let mut cfg = client.load_intranet_config().unwrap();
    cfg.host_url = format!("http://{}", server.local_addr());
    client.save_intranet_config(&cfg).unwrap();
    assert!(client.sync_from_host(false).unwrap());
    assert!(client.list_offline_outbox().unwrap().is_empty());
    assert_eq!(task_version(&host, id), base);
    assert_eq!(task_version(&client, id), base);
    let mut t = get(&client, id);
    t.notes = "terceira".into();
    client
        .save_task_checked(&t, Some(base))
        .expect("no other edit happened");
    server.stop();
}

// ------------------------------------------ review 3 of the fixes --

/// A form open on A saves `n` times; B saves the same task `n` times (a
/// deadline, then notes) and exchanges; A exchanges (its change becomes a
/// card and A takes B's task, on the same version number the form holds).
fn same_count_collision(n: usize) -> (Pair, TaskPanel, i64) {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    for i in 0..n {
        panel.set_draft_title(&format!("titulo de A {i}"));
        panel.save_draft(&mut p.a).unwrap();
    }
    let mut base = task_version(&p.b, id);
    for i in 0..n {
        let mut t = get(&p.b, id);
        if i == 0 {
            t.due_date = "2026-12-01".into();
        } else {
            t.notes = format!("nota de B {i}");
        }
        p.b.save_task_checked(&t, Some(base)).unwrap();
        base = task_version(&p.b, id);
    }
    sync(&mut p.b);
    sync(&mut p.a);
    assert_eq!(get(&p.a, id).due_date, "2026-12-01", "A took B's task");
    (p, panel, id)
}

fn assert_collision_refused(n: usize) {
    let (mut p, mut panel, id) = same_count_collision(n);
    panel.set_draft_title("titulo de A final");
    let saved = panel.save_draft(&mut p.a);
    assert!(
        saved.is_err(),
        "{n}x{n}: the other computer's edit must not be overwritten: {saved:?}"
    );
    assert!(panel.edit_conflict().is_some(), "the conflict path");
    assert!(panel.dirty(), "the typing stays");
    let now = get(&p.a, id);
    assert_eq!(now.due_date, "2026-12-01", "nothing written");
    if n > 1 {
        assert_eq!(now.notes, format!("nota de B {}", n - 1));
    }
    // "Juntar as duas versões" keeps both, and the save goes through.
    assert!(panel.merge_conflict());
    panel
        .save_draft(&mut p.a)
        .expect("joined, on the current version");
    let now = get(&p.a, id);
    assert_eq!(now.title, "titulo de A final");
    assert_eq!(now.due_date, "2026-12-01");
}

/// Review 3, finding 1: one save on each computer, the same version
/// number with different contents. The form takes the conflict path.
#[test]
fn rev3_one_save_on_each_side_takes_the_conflict_path() {
    assert_collision_refused(1);
}

/// Review 3, finding 1: two saves on each computer (the case the `leaves`
/// fix opened). The form takes the conflict path.
#[test]
fn rev3_two_saves_on_each_side_take_the_conflict_path() {
    assert_collision_refused(2);
}

/// Review 3, finding 1, guard: only the version moved (twice), nothing of
/// the task changed: the form saves on the first try.
#[test]
fn rev3_only_the_version_moved_still_saves_at_once() {
    let mut p = pair();
    let id = p.task;
    let mut panel = TaskPanel::default();
    assert!(panel.open_task(&get(&p.a, id), &p.a));
    panel.set_draft_title("revisado");
    bump_without_change(&mut p.a, id);
    bump_without_change(&mut p.a, id);
    panel
        .save_draft(&mut p.a)
        .expect("nothing changed but the version");
    assert_eq!(get(&p.a, id).title, "revisado");
    assert!(!panel.dirty());
}

/// Review 3, finding 2: a project created and edited twice before the
/// first exchange keeps the version its form holds, and saves again.
#[test]
fn rev3_a_project_created_and_edited_before_the_first_exchange() {
    let mut p = pair();
    let project =
        p.a.save_project(&crate::projects::Project::new("Projeto X"))
            .unwrap();
    let mut held = p.a.project_version(&project.uid).unwrap();
    let mut edited = p.a.get_project(&project.uid).unwrap().unwrap();
    for objective in ["um", "dois"] {
        edited.objective = objective.into();
        edited = p.a.save_project_checked(&edited, Some(held)).unwrap();
        held = p.a.project_version(&project.uid).unwrap();
    }
    assert_eq!(p.a.list_offline_outbox().unwrap().len(), 1, "merged");
    sync(&mut p.a);
    assert_eq!(p.a.project_version(&project.uid).unwrap(), held);
    edited.objective = "três".into();
    p.a.save_project_checked(&edited, Some(held))
        .expect("no other edit of the project happened");
    sync(&mut p.a);
    sync(&mut p.b);
    assert_eq!(
        p.b.project_version(&project.uid).unwrap(),
        p.a.project_version(&project.uid).unwrap()
    );
}

/// Review 3, finding 2: the same for a category.
#[test]
fn rev3_a_category_created_and_edited_before_the_first_exchange() {
    let mut p = pair();
    let made =
        p.a.save_category(&crate::categories::Category::new("Urgente", 1))
            .unwrap();
    let mut held = p.a.category_version(&made.uid).unwrap();
    let mut edited = made.clone();
    for color in [2u8, 3] {
        edited.color = color;
        edited = p.a.save_category_checked(&edited, Some(held)).unwrap();
        held = p.a.category_version(&made.uid).unwrap();
    }
    sync(&mut p.a);
    assert_eq!(p.a.category_version(&made.uid).unwrap(), held);
    edited.color = 4;
    p.a.save_category_checked(&edited, Some(held))
        .expect("no other edit of the category happened");
}

fn resource(name: &str) -> crate::resources::Resource {
    crate::resources::Resource {
        name: name.into(),
        ..Default::default()
    }
}

/// Review 3, finding 2: the same for a resource and for a use of it.
#[test]
fn rev3_resources_created_and_edited_before_the_first_exchange() {
    let mut p = pair();
    let uid = p.a.save_resource(&resource("Sala 1"), None).unwrap();
    let mut held = p.a.resource_version(&uid).unwrap();
    let get_res = |s: &Store| {
        s.list_resources()
            .unwrap()
            .into_iter()
            .find(|r| r.uid == uid)
            .unwrap()
    };
    for notes in ["um", "dois"] {
        let mut r = get_res(&p.a);
        r.notes = notes.into();
        p.a.save_resource(&r, Some(held)).unwrap();
        held = p.a.resource_version(&uid).unwrap();
    }
    let mut item = crate::resources::ResourceUse {
        resource_uid: uid.clone(),
        who: "Servidor C".into(),
        start: "2031-02-10T09:00".into(),
        end: "2031-02-10T11:00".into(),
        ..Default::default()
    };
    let use_uid = p.a.save_resource_use(&item, None).unwrap();
    let mut use_held = p.a.resource_use_version(&use_uid).unwrap();
    item.uid = use_uid.clone();
    for purpose in ["um", "dois"] {
        item.purpose = purpose.into();
        p.a.save_resource_use(&item, Some(use_held)).unwrap();
        use_held = p.a.resource_use_version(&use_uid).unwrap();
    }
    sync(&mut p.a);
    assert_eq!(p.a.resource_version(&uid).unwrap(), held);
    assert_eq!(p.a.resource_use_version(&use_uid).unwrap(), use_held);
    let mut r = get_res(&p.a);
    r.notes = "três".into();
    p.a.save_resource(&r, Some(held))
        .expect("no other edit of the resource happened");
    item.purpose = "três".into();
    p.a.save_resource_use(&item, Some(use_held))
        .expect("no other edit of the use happened");
}

/// Review 3, missing tests: `leaves` for the context of a document and
/// for an atendimento (two saves, an exchange, a third save).
#[test]
fn rev3_context_and_atendimento_keep_their_version() {
    let mut p = pair();
    let doc = p.document;
    let mut base = p.a.item_version("work_context", doc).unwrap().version;
    for notes in ["um", "dois"] {
        let mut ctx = p.a.load_work_context(doc).unwrap();
        ctx.notes = notes.into();
        p.a.save_work_context_checked(doc, &ctx, Some(base))
            .unwrap();
        base = p.a.item_version("work_context", doc).unwrap().version;
    }
    let mut att = crate::saiop::SaiopAttendance::new_manual();
    att.reference_raw = "ORG-OFI-2026/00042".into();
    att.due_kind = "oficial".into();
    att.due_date = "2026-12-01".into();
    let att_id = p.a.save_saiop_attendance(&att, None).unwrap();
    sync(&mut p.a);
    let mut abase = p.a.item_version("saiop", att_id).unwrap().version;
    let find = |s: &Store| {
        s.list_saiop_attendances()
            .unwrap()
            .into_iter()
            .find(|a| a.id == att_id)
            .unwrap()
    };
    for subject in ["um", "dois"] {
        let mut a = find(&p.a);
        a.subject = subject.into();
        p.a.save_saiop_attendance(&a, Some(abase)).unwrap();
        abase = p.a.item_version("saiop", att_id).unwrap().version;
    }
    sync(&mut p.a);
    assert_eq!(p.a.item_version("work_context", doc).unwrap().version, base);
    assert_eq!(p.a.item_version("saiop", att_id).unwrap().version, abase);
    let mut ctx = p.a.load_work_context(doc).unwrap();
    ctx.notes = "três".into();
    p.a.save_work_context_checked(doc, &ctx, Some(base))
        .expect("no other edit of the context happened");
    let mut a = find(&p.a);
    a.subject = "três".into();
    p.a.save_saiop_attendance(&a, Some(abase))
        .expect("no other edit of the atendimento happened");
}

/// Review 3, missing tests: `leaves` through another copy's journal. B
/// saves the task twice through a pen drive only; A exchanges through both
/// files and replays B's change from the pen drive's journal into the
/// Drive. Every copy leaves the task at B's version, and B saves again.
#[test]
fn rev3_leaves_through_another_copys_journal() {
    let mut p = pair();
    let id = p.task;
    let pen = p
        .drive
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Pendrive")
        .join("ponte-sync.ponte");
    std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
    let only = |store: &mut Store, files: &[&std::path::Path]| {
        let mut cfg = store.load_intranet_config().unwrap();
        cfg.folder_files = files.iter().map(|f| f.display().to_string()).collect();
        store.save_intranet_config(&cfg).unwrap();
        sync(store)
    };
    let drive = p.drive.clone();
    only(&mut p.b, &[&drive, &pen]);
    only(&mut p.a, &[&drive, &pen]);
    let held = save_twice(&mut p.b, id);
    only(&mut p.b, &[&pen]);
    assert_eq!(task_version(&p.b, id), held);
    only(&mut p.a, &[&drive, &pen]);
    assert_eq!(get(&p.a, id).notes, "segunda");
    assert_eq!(task_version(&p.a, id), held, "A has B's version");
    only(&mut p.b, &[&drive, &pen]);
    assert_eq!(task_version(&p.b, id), held, "B's version never goes back");
    let mut t = get(&p.b, id);
    t.notes = "terceira".into();
    p.b.save_task_checked(&t, Some(held))
        .expect("no other edit happened");
}

/// Review 3, missing tests: `leaves` through the replay of an old copy
/// (format 8): a task it created and then edited twice (one change that
/// leaves it at version 3) is left at version 3 here too.
#[test]
fn rev3_leaves_through_the_old_copy_replay() {
    use crate::intranet::IntranetMutation;
    use crate::sharedfolder;
    let mut p = pair();
    let nine = sharedfolder::nine_path(&p.drive);
    let copy = match sharedfolder::read_any(&nine, KEY).unwrap() {
        Some(sharedfolder::Found::Copy(copy)) => copy,
        _ => panic!("no 9"),
    };
    let mut payload = copy.payload;
    payload.snapshot.idents.clear();
    payload.snapshot.ident_epoch.clear();
    for e in payload.journal.iter_mut() {
        e.idents.clear();
    }
    let entry = |uid: &str,
                 mutation: IntranetMutation,
                 created: Vec<crate::idmap::CreatedId>,
                 leaves: Option<i64>| sharedfolder::JournalEntry {
        uid: uid.into(),
        created_at: "2026-11-20T12:00:00Z".into(),
        actor: OWNER.into(),
        writer: "aparelho-antigo".into(),
        label: "Tarefa".into(),
        mutation,
        base: None,
        revision: 1000,
        created,
        idents: Vec::new(),
        refs: None,
        legacy: false,
        leaves,
    };
    let mut task = Task::blank(None);
    task.title = "Tarefa da cópia antiga".into();
    let mut edited = task.clone();
    edited.id = 4501;
    edited.notes = "editada duas vezes".into();
    let made = entry(
        "antiga-cria",
        IntranetMutation::SaveTask {
            task,
            expected_version: None,
            no_project: false,
        },
        vec![crate::idmap::CreatedId::new(
            crate::idmap::IdKind::Task,
            "",
            4501,
        )],
        None,
    );
    let edit = entry(
        "antiga-edita",
        IntranetMutation::SaveTask {
            task: edited,
            expected_version: Some(1),
            no_project: false,
        },
        Vec::new(),
        Some(3),
    );
    for e in [&made, &edit] {
        payload.applied.push(e.uid.clone());
    }
    payload.journal.extend([made, edit]);
    sharedfolder::write(
        &p.drive,
        KEY,
        &sharedfolder::WriteRequest {
            lineage: &copy.header.lineage,
            revision: copy.header.revision + 1,
            writer: "aparelho-antigo",
            writer_name: "Aparelho antigo",
            schema_version: 21,
        },
        &payload,
        &copy.blobs,
    )
    .unwrap();
    sync(&mut p.a);
    let replayed =
        p.a.list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.title == "Tarefa da cópia antiga")
            .expect("replayed");
    assert_eq!(replayed.notes, "editada duas vezes");
    assert_eq!(task_version(&p.a, replayed.id), 3);
}

/// Review 3, finding 3: a `leaves` that is not sound (not past the step,
/// negative, too far ahead, at the edge of the number) is ignored: the
/// replay takes its own step. A sound one is kept.
#[test]
fn rev3_an_unsound_leaves_is_ignored() {
    use crate::intranet::{replaying_to, IntranetMutation};
    let mut p = pair();
    let id = p.task;
    // (leaves as an offset from the version, or an absolute value; the
    // version the save must leave, as an offset)
    let cases: [(Result<i64, i64>, i64); 7] = [
        (Err(i64::MAX), 1),
        (Err(i64::MIN), 1),
        (Err(-3), 1),
        (Err(0), 1),
        (Ok(1), 1),
        (Ok(2_000_000), 1),
        (Ok(3), 3),
    ];
    for (leaves, ahead) in cases {
        let v = task_version(&p.a, id);
        let leaves = match leaves {
            Ok(offset) => v + offset,
            Err(absolute) => absolute,
        };
        let mut t = get(&p.a, id);
        t.notes = format!("{leaves}");
        let mutation = IntranetMutation::SaveTask {
            task: t.clone(),
            expected_version: Some(v),
            no_project: false,
        };
        replaying_to(&mutation, Some(leaves), || {
            p.a.save_task_checked(&t, Some(v)).unwrap();
        });
        assert_eq!(task_version(&p.a, id), v + ahead, "leaves {leaves}");
    }
}

/// Review 3, finding 3: a version at the edge of the number is refused
/// with a sentence, never an overflow.
#[test]
fn rev3_the_next_save_never_overflows() {
    let mut p = pair();
    let id = p.task;
    p.a.conn
        .execute(
            "UPDATE item_versions SET version=?1 WHERE kind='task' AND item_id=?2",
            rusqlite::params![i64::MAX, id],
        )
        .unwrap();
    let t = get(&p.a, id);
    let saved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        p.a.save_task_checked(&t, Some(i64::MAX))
    }));
    match saved {
        Ok(result) => assert!(result.is_err(), "refused"),
        Err(_) => panic!("the next save overflowed"),
    }
}

/// Review 3: a creation merged with later edits and then with its deletion
/// is queued as a deletion without `leaves` (the item is gone; the file of
/// format 8 stays the one 0.51 writes).
#[test]
fn rev3_a_deletion_never_carries_leaves() {
    let mut p = pair();
    let project =
        p.a.save_project(&crate::projects::Project::new("Projeto efêmero"))
            .unwrap();
    let held = p.a.project_version(&project.uid).unwrap();
    let mut edited = project.clone();
    edited.objective = "um".into();
    p.a.save_project_checked(&edited, Some(held)).unwrap();
    let held = p.a.project_version(&project.uid).unwrap();
    p.a.delete_project_checked(&project.uid, Some(held))
        .unwrap();
    let queue = p.a.list_offline_outbox().unwrap();
    let entry = queue
        .iter()
        .find(|e| {
            matches!(&e.mutation, crate::intranet::IntranetMutation::DeleteProject { uid, .. } if *uid == project.uid)
        })
        .expect("the deletion is queued");
    assert_eq!(entry.leaves, None);
    sync(&mut p.a);
    assert!(p.a.get_project(&project.uid).unwrap().is_none());
}
