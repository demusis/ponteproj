//! Tests of the projects in the library: storage, tasks, export and every
//! way the library is shared (shared folder, main computer and clients).
use super::*;
use crate::intranet::{
    generate_token, process_rpc, HostServer, IntranetConfig, IntranetMode, RpcRequest, RpcResponse,
};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    (dir, store)
}

fn make(store: &mut Store, name: &str) -> Project {
    store.save_project(&Project::new(name)).unwrap()
}

fn task_in(store: &mut Store, title: &str, project: &str) -> i64 {
    let mut task = Task::standalone();
    task.title = title.into();
    task.project_uid = project.into();
    store.save_task(&task).unwrap()
}

fn stored(store: &Store, id: i64) -> Task {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

fn by_title(store: &Store, title: &str) -> Task {
    store
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.title == title)
        .unwrap_or_else(|| panic!("task {title} not found"))
}

fn names(store: &Store) -> Vec<String> {
    store
        .list_projects()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect()
}

#[test]
fn projects_are_created_renamed_archived_and_deleted() {
    let (_dir, mut store) = store();
    assert!(store.list_projects().unwrap().is_empty());
    let reform = make(&mut store, "  Reforma   da sede ");
    assert_eq!(reform.name, "Reforma da sede");
    assert_eq!(reform.uid.len(), 32);
    assert!(is_valid_uid(&reform.uid));
    assert_eq!(reform.status, ProjectStatus::Ativo);
    assert!(!reform.created_at.is_empty() && !reform.updated_at.is_empty());
    make(&mut store, "Análise");
    // Sorted by name ignoring accents and case.
    assert_eq!(names(&store), ["Análise", "Reforma da sede"]);

    // Rename and edit keep the uid and the creation stamp.
    let mut edited = reform.clone();
    edited.name = "Reforma e mudança".into();
    edited.objective = "Mudar para o prédio novo".into();
    edited.assignee = "Ana".into();
    edited.start_date = "2026-11-01".into();
    edited.target_date = "2026-12-20".into();
    let saved = store.save_project(&edited).unwrap();
    assert_eq!(saved.uid, reform.uid);
    assert_eq!(saved.created_at, reform.created_at);
    assert_eq!(store.list_projects().unwrap().len(), 2);
    let got = store.get_project(&reform.uid).unwrap().unwrap();
    assert_eq!(got.objective, "Mudar para o prédio novo");
    assert_eq!(got.target_date, "2026-12-20");
    assert!(store.get_project(&"c".repeat(32)).unwrap().is_none());

    // Archiving is a save with the status.
    let mut archived = got.clone();
    archived.status = ProjectStatus::Arquivado;
    store.save_project(&archived).unwrap();
    assert_eq!(
        store.get_project(&reform.uid).unwrap().unwrap().status,
        ProjectStatus::Arquivado
    );

    // Delete.
    store.delete_project(&reform.uid).unwrap();
    assert_eq!(names(&store), ["Análise"]);
    assert!(store
        .delete_project(&reform.uid)
        .unwrap_err()
        .to_string()
        .contains("não encontrado"));
    assert!(store.delete_project("curto").is_err());
}

#[test]
fn the_rules_of_names_dates_and_the_limit_are_enforced() {
    let (_dir, mut store) = store();
    make(&mut store, "Orientação");
    for name in ["orientacao", "  ORIENTAÇÃO ", "Orientacao"] {
        let error = store
            .save_project(&Project::new(name))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Já existe um projeto com este nome.");
    }
    assert!(store.save_project(&Project::new("   ")).is_err());
    assert!(store
        .save_project(&Project::new(&"x".repeat(MAX_NAME + 1)))
        .is_err());
    assert!(store
        .save_project(&Project::new(&"y".repeat(MAX_NAME)))
        .is_ok());
    let mut backwards = Project::new("Datas");
    backwards.start_date = "2026-12-01".into();
    backwards.target_date = "2026-11-01".into();
    assert!(store.save_project(&backwards).is_err());
    // Saving a project again with its own name is not a duplicate.
    let mut same = store.list_projects().unwrap()[0].clone();
    same.assignee = "Beto".into();
    assert!(store.save_project(&same).is_ok());
    // The library holds at most MAX_PROJECTS.
    let have = store.list_projects().unwrap().len();
    for n in have..MAX_PROJECTS {
        make(&mut store, &format!("Projeto {n}"));
    }
    let error = store
        .save_project(&Project::new("Mais um"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("500 projetos"), "{error}");
    assert!(store.save_project(&same).is_ok());
}

#[test]
fn versions_refuse_a_stale_edit_and_a_deleted_project_is_not_brought_back() {
    let (_dir, mut store) = store();
    let project = make(&mut store, "Mudança");
    let v1 = store.project_version(&project.uid).unwrap();
    assert!(v1 > 0);
    let mut edited = project.clone();
    edited.assignee = "Ana".into();
    store.save_project_checked(&edited, Some(v1)).unwrap();
    let v2 = store.project_version(&project.uid).unwrap();
    assert_eq!(v2, v1 + 1);
    edited.assignee = "Beto".into();
    let error = store
        .save_project_checked(&edited, Some(v1))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Conflito de edição"), "{error}");
    assert_eq!(
        store.get_project(&project.uid).unwrap().unwrap().assignee,
        "Ana"
    );
    store.delete_project(&project.uid).unwrap();
    // An edit made from an older view of it does not resurrect it.
    for version in [None, Some(v2)] {
        let error = store
            .save_project_checked(&edited, version)
            .unwrap_err()
            .to_string();
        assert!(!error.is_empty());
        assert!(store.list_projects().unwrap().is_empty());
    }
}

#[test]
fn a_task_keeps_only_an_existing_project() {
    let (_dir, mut store) = store();
    let a = make(&mut store, "A");
    let b = make(&mut store, "B");
    let ghost = "0123456789abcdef0123456789abcdef";
    let id = task_in(&mut store, "Com projeto", &a.uid);
    assert_eq!(stored(&store, id).project_uid, a.uid);
    let lost = task_in(&mut store, "Projeto inexistente", ghost);
    assert!(stored(&store, lost).project_uid.is_empty());
    // A malformed uid is refused.
    let mut bad = Task::standalone();
    bad.title = "Ruim".into();
    bad.project_uid = "abc".into();
    assert!(store.save_task(&bad).is_err());
    // Move to another project, then to none.
    let mut task = stored(&store, id);
    task.project_uid = b.uid.clone();
    store.save_task(&task).unwrap();
    assert_eq!(stored(&store, id).project_uid, b.uid);
    task.project_uid.clear();
    store.save_task(&task).unwrap();
    assert!(stored(&store, id).project_uid.is_empty());
    // The payload keeps the field only when there is something in it.
    assert!(!serde_json::to_string(&Task::standalone())
        .unwrap()
        .contains("project_uid"));
    // A task read from the library never shows a project that is gone, even
    // when its row still holds the uid.
    let id = task_in(&mut store, "Órfã", &a.uid);
    store
        .conn
        .execute("DELETE FROM projects WHERE uid=?1", [&a.uid])
        .unwrap();
    assert!(stored(&store, id).project_uid.is_empty());
    assert!(store.project_tasks(&a.uid).unwrap().is_empty());
}

#[test]
fn a_new_subtask_inherits_the_project_of_its_parent_once() {
    let (_dir, mut store) = store();
    let a = make(&mut store, "A");
    let b = make(&mut store, "B");
    let parent = task_in(&mut store, "Mãe", &a.uid);
    let child = |store: &mut Store, title: &str, project: &str| {
        let mut task = Task::standalone();
        task.title = title.into();
        task.parent_task_id = Some(parent);
        task.project_uid = project.into();
        store.save_task(&task).unwrap()
    };
    let inherited = child(&mut store, "Herda", "");
    assert_eq!(stored(&store, inherited).project_uid, a.uid);
    let given = child(&mut store, "Própria", &b.uid);
    assert_eq!(stored(&store, given).project_uid, b.uid);
    // Changing the parent's project later does not rewrite the subtasks.
    let mut mother = stored(&store, parent);
    mother.project_uid = b.uid.clone();
    store.save_task(&mother).unwrap();
    assert_eq!(stored(&store, inherited).project_uid, a.uid);
    // An edit of the subtask that clears the project is kept.
    let mut edited = stored(&store, inherited);
    edited.project_uid.clear();
    store.save_task(&edited).unwrap();
    assert!(stored(&store, inherited).project_uid.is_empty());
    // A parent without project gives none.
    mother.project_uid.clear();
    store.save_task(&mother).unwrap();
    let plain = child(&mut store, "Sem nada", "");
    assert!(stored(&store, plain).project_uid.is_empty());
    // Sending the very same new subtask again (same uid) is the same one,
    // even after the project of the parent changed.
    let mut again = Task::standalone();
    again.title = "Repetida".into();
    again.parent_task_id = Some(parent);
    mother.project_uid = a.uid.clone();
    store.save_task(&mother).unwrap();
    let first = store.save_task(&again).unwrap();
    assert_eq!(stored(&store, first).project_uid, a.uid);
    mother.project_uid = b.uid.clone();
    store.save_task(&mother).unwrap();
    assert_eq!(store.save_task(&again).unwrap(), first);
}

#[test]
fn deleting_a_project_keeps_its_tasks_with_no_project() {
    let (_dir, mut store) = store();
    let a = make(&mut store, "A");
    let b = make(&mut store, "B");
    let one = task_in(&mut store, "Um", &a.uid);
    let two = task_in(&mut store, "Dois", &a.uid);
    let other = task_in(&mut store, "Outro", &b.uid);
    let trashed = task_in(&mut store, "Na lixeira", &a.uid);
    store
        .trash_item(crate::trash::Target {
            kind: crate::trash::Kind::Task,
            id: trashed,
        })
        .unwrap();
    store.delete_project(&a.uid).unwrap();
    assert_eq!(names(&store), ["B"]);
    for id in [one, two] {
        assert!(stored(&store, id).project_uid.is_empty());
    }
    assert_eq!(stored(&store, other).project_uid, b.uid);
    // The task in the trash comes back with no project too.
    store
        .restore_item(crate::trash::Target {
            kind: crate::trash::Kind::Task,
            id: trashed,
        })
        .unwrap();
    assert!(stored(&store, trashed).project_uid.is_empty());
    assert_eq!(store.list_tasks().unwrap().len(), 4);
    // The stored payload lost the uid in the same transaction.
    let raw: Vec<u8> = store
        .conn
        .query_row("SELECT payload FROM tasks WHERE id=?1", [one], |r| r.get(0))
        .unwrap();
    let task: Task = serde_json::from_slice(&unseal(&raw).unwrap()).unwrap();
    assert!(task.project_uid.is_empty());
}

#[test]
fn the_overview_matches_the_rules_over_the_library() {
    let (_dir, mut store) = store();
    let mut project = Project::new("Mudança");
    project.target_date = "2026-11-25".into();
    let project = store.save_project(&project).unwrap();
    let mut done = Task::standalone();
    done.title = "Feita".into();
    done.project_uid = project.uid.clone();
    done.status = "concluida".into();
    store.save_task(&done).unwrap();
    let mut late = Task::standalone();
    late.title = "Atrasada".into();
    late.project_uid = project.uid.clone();
    late.due_date = "2026-11-10".into();
    store.save_task(&late).unwrap();
    let mut planned = Task::standalone();
    planned.title = "Planejada".into();
    planned.project_uid = project.uid.clone();
    planned.due_date = "2026-11-24".into();
    planned.scheduled_start = "2026-11-23T09:00".into();
    planned.scheduled_end = "2026-11-27T18:00".into();
    store.save_task(&planned).unwrap();
    task_in(&mut store, "De fora", "");
    let today = NaiveDate::from_ymd_opt(2026, 11, 19).unwrap();
    let overview = store.project_overview(today).unwrap();
    assert_eq!(overview.len(), 1);
    let item = &overview[0];
    assert_eq!(item.project.uid, project.uid);
    assert_eq!(
        item.progress,
        Progress {
            total: 3,
            done: 1,
            late: 1,
            next_due: Some("2026-11-10".into()),
        }
    );
    assert_eq!(item.risk.deliveries_at_risk, 1);
    assert_eq!(
        item.risk.target_at_risk,
        NaiveDateTime::parse_from_str("2026-11-27T18:00", "%Y-%m-%dT%H:%M").ok()
    );
    let tasks = store.list_tasks().unwrap();
    assert_eq!(timeline(&project.uid, &tasks).len(), 3);
    assert_eq!(store.project_tasks(&project.uid).unwrap().len(), 3);
}

#[test]
fn snapshots_and_backups_carry_the_projects_and_the_project_of_tasks() {
    let (dir, mut a) = store();
    let project = make(&mut a, "Mudança");
    let id = task_in(&mut a, "Caixas", &project.uid);
    let snapshot = a.export_snapshot().unwrap();
    assert_eq!(snapshot.projects.len(), 1);
    assert_eq!(snapshot.schema_version, 21);
    assert_eq!(
        snapshot
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .project_uid,
        project.uid
    );
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.apply_snapshot(&snapshot, |_, _| unreachable!(), |_| unreachable!())
        .unwrap();
    assert_eq!(names(&b), ["Mudança"]);
    assert_eq!(by_title(&b, "Caixas").project_uid, project.uid);
    // A copy from a current Ponte is the truth, empty or not.
    let mut empty = snapshot.clone();
    empty.projects.clear();
    for task in &mut empty.tasks {
        task.project_uid.clear();
    }
    b.apply_snapshot(&empty, |_, _| unreachable!(), |_| unreachable!())
        .unwrap();
    assert!(b.list_projects().unwrap().is_empty());
    // A copy from a Ponte before the projects (no field, schema 20) keeps
    // the ones this library has, and the project of its tasks.
    let mut old = snapshot.clone();
    old.projects.clear();
    for task in &mut old.tasks {
        task.project_uid.clear();
    }
    old.schema_version = crate::intranet::PROJECTS_SCHEMA - 1;
    let mut json = serde_json::to_value(&old).unwrap();
    json.as_object_mut().unwrap().remove("projects");
    let old: crate::intranet::CabinetSnapshot = serde_json::from_value(json).unwrap();
    assert!(old.projects.is_empty());
    a.apply_snapshot(&old, |_, _| unreachable!(), |_| unreachable!())
        .unwrap();
    assert_eq!(names(&a), ["Mudança"]);
    assert_eq!(by_title(&a, "Caixas").project_uid, project.uid);
    // The backup is the library itself.
    let copy = dir.path().join("backup.sqlite3");
    a.backup(&copy).unwrap();
    let restored = Store::open(&copy).unwrap();
    assert_eq!(names(&restored), ["Mudança"]);
    assert_eq!(by_title(&restored, "Caixas").project_uid, project.uid);
}

#[test]
fn the_csv_exports_list_the_projects_and_the_project_of_each_task() {
    let (dir, mut store) = store();
    let mut project = Project::new("Mudança");
    project.assignee = "Ana".into();
    project.target_date = "2026-12-20".into();
    let project = store.save_project(&project).unwrap();
    task_in(&mut store, "Caixas", &project.uid);
    task_in(&mut store, "Avulsa", "");
    let folder = dir.path().join("csv");
    std::fs::create_dir_all(&folder).unwrap();
    let report = crate::export::export_to(&store, &folder).unwrap();
    assert_eq!(report.projects, 1);
    let tasks = std::fs::read_to_string(folder.join("tarefas.csv")).unwrap();
    let lines: Vec<&str> = tasks.lines().collect();
    assert!(lines[0].ends_with(";Projeto"), "{}", lines[0]);
    let caixas = lines.iter().find(|l| l.contains("Caixas")).unwrap();
    assert!(caixas.ends_with("Mudança"), "{caixas}");
    let avulsa = lines.iter().find(|l| l.contains("Avulsa")).unwrap();
    assert!(!avulsa.contains("Mudança"));
    let projects = std::fs::read_to_string(folder.join("projetos.csv")).unwrap();
    assert!(
        projects.contains("Projeto;Situação;Responsável"),
        "{projects}"
    );
    assert!(
        projects.contains("Mudança;Ativo;Ana;;2026-12-20;1;0;"),
        "{projects}"
    );
}

// -- sync -------------------------------------------------------------

const KEY: &str = "senha-de-teste-123";

fn folder_config(member: &str, files: &[String]) -> IntranetConfig {
    IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: files.to_vec(),
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

fn pair(dir: &std::path::Path) -> (Store, Store, String) {
    let folder = dir.join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    let files = vec![folder.display().to_string()];
    let mut a = Store::open(&dir.join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Servidor A", &files))
        .unwrap();
    sync(&mut a);
    let mut b = Store::open(&dir.join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Servidor B", &files))
        .unwrap();
    sync(&mut b);
    (a, b, folder.display().to_string())
}

#[test]
fn the_shared_folder_carries_projects_and_their_tasks_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, mut b, _) = pair(dir.path());
    let reform = make(&mut a, "Reforma");
    task_in(&mut a, "Pintar", &reform.uid);
    task_in(&mut a, "Avulsa", "");
    sync(&mut a);
    sync(&mut b);
    assert_eq!(b.list_projects().unwrap(), a.list_projects().unwrap());
    assert_eq!(by_title(&b, "Pintar").project_uid, reform.uid);
    assert!(by_title(&b, "Avulsa").project_uid.is_empty());

    // B edits the project, makes another one, and sends a task to it.
    let mut edited = b.get_project(&reform.uid).unwrap().unwrap();
    edited.name = "Reforma geral".into();
    edited.status = ProjectStatus::Pausado;
    b.save_project(&edited).unwrap();
    let extra = make(&mut b, "Extra");
    let mut avulsa = by_title(&b, "Avulsa");
    avulsa.project_uid = extra.uid.clone();
    b.save_task(&avulsa).unwrap();
    // A subtask of a task of the project, made on B, inherits it.
    let mut sub = Task::standalone();
    sub.title = "Subtarefa".into();
    sub.parent_task_id = Some(by_title(&b, "Pintar").id);
    b.save_task(&sub).unwrap();
    sync(&mut b);
    sync(&mut a);
    assert_eq!(names(&a), ["Extra", "Reforma geral"]);
    assert_eq!(
        a.get_project(&reform.uid).unwrap().unwrap().status,
        ProjectStatus::Pausado
    );
    assert_eq!(by_title(&a, "Avulsa").project_uid, extra.uid);
    assert_eq!(by_title(&a, "Subtarefa").project_uid, reform.uid);

    // A deletes a project: it leaves the tasks on both computers, and the
    // tasks stay.
    a.delete_project(&extra.uid).unwrap();
    sync(&mut a);
    sync(&mut b);
    assert_eq!(names(&b), ["Reforma geral"]);
    assert!(by_title(&b, "Avulsa").project_uid.is_empty());
    assert!(a.list_sync_conflicts().unwrap().is_empty());
    assert!(b.list_sync_conflicts().unwrap().is_empty());
}

#[test]
fn a_project_deleted_elsewhere_is_not_brought_back_and_its_late_tasks_lose_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, mut b, _) = pair(dir.path());
    let gone = make(&mut a, "Apagado");
    let kept = make(&mut a, "Ficou");
    sync(&mut a);
    sync(&mut b);
    // B works offline: edits the project A deletes, and files a new task in
    // it and another in a project that stays.
    a.delete_project(&gone.uid).unwrap();
    sync(&mut a);
    let mut edit = b.get_project(&gone.uid).unwrap().unwrap();
    edit.assignee = "Beto".into();
    b.save_project(&edit).unwrap();
    task_in(&mut b, "Nova em apagado", &gone.uid);
    task_in(&mut b, "Nova em outro", &kept.uid);
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert_eq!(names(store), ["Ficou"], "not resurrected");
        assert!(by_title(store, "Nova em apagado").project_uid.is_empty());
        assert_eq!(by_title(store, "Nova em outro").project_uid, kept.uid);
    }
}

#[test]
fn the_same_name_created_offline_on_two_computers_keeps_both_projects() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, mut b, _) = pair(dir.path());
    let on_a = make(&mut a, "Mudança");
    task_in(&mut a, "Tarefa de A", &on_a.uid);
    let on_b = make(&mut b, "Mudança");
    task_in(&mut b, "Tarefa de B", &on_b.uid);
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    for store in [&a, &b] {
        let mut found = names(store);
        found.sort();
        assert_eq!(found, ["Mudança", "Mudança (2)"]);
        assert!(store.list_sync_conflicts().unwrap().is_empty());
        let name_of = |title: &str| {
            let uid = by_title(store, title).project_uid;
            store.get_project(&uid).unwrap().unwrap().name
        };
        assert_eq!(name_of("Tarefa de A"), "Mudança");
        assert_eq!(name_of("Tarefa de B"), "Mudança (2)");
    }
}

#[test]
fn a_copy_from_a_ponte_before_the_projects_keeps_the_projects_and_their_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, _, folder) = pair(dir.path());
    let project = make(&mut a, "Reforma");
    task_in(&mut a, "Pintar", &project.uid);
    sync(&mut a);
    let path = std::path::PathBuf::from(&folder);
    let mut copy = crate::sharedfolder::read(&path, KEY).unwrap().unwrap();
    // What an older Ponte (schema 20) writes: no projects, tasks without the
    // field.
    copy.payload.snapshot.projects.clear();
    for task in &mut copy.payload.snapshot.tasks {
        task.project_uid.clear();
    }
    copy.payload.snapshot.schema_version = crate::intranet::PROJECTS_SCHEMA - 1;
    let h = &copy.header;
    let request = crate::sharedfolder::WriteRequest {
        lineage: &h.lineage,
        revision: h.revision + 1,
        writer: "old-writer",
        writer_name: "Antigo",
        schema_version: crate::intranet::PROJECTS_SCHEMA - 1,
    };
    crate::sharedfolder::write(&path, KEY, &request, &copy.payload, &copy.blobs).unwrap();
    // The upgraded computer exchanges again: nothing it had is lost, and what
    // it writes carries the projects once more.
    sync(&mut a);
    assert_eq!(names(&a), ["Reforma"]);
    assert_eq!(by_title(&a, "Pintar").project_uid, project.uid);
    let written = crate::sharedfolder::read(&path, KEY).unwrap().unwrap();
    assert_eq!(written.payload.snapshot.projects.len(), 1);
    assert_eq!(written.header.schema_version, 21);
    assert_eq!(written.header.format, 8);
    assert_eq!(
        written
            .payload
            .snapshot
            .tasks
            .iter()
            .find(|t| t.title == "Pintar")
            .unwrap()
            .project_uid,
        project.uid
    );
}

#[test]
fn a_computer_that_upgrades_offline_takes_what_another_shared_and_adds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, mut b, _) = pair(dir.path());
    // B shared a project and tasks while A (still on its old library) was
    // away; A's migration made no project of its own.
    task_in(&mut a, "Antiga de A", "");
    assert!(a.list_projects().unwrap().is_empty());
    let shared = make(&mut b, "Compartilhado");
    task_in(&mut b, "Tarefa de B", &shared.uid);
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert_eq!(names(store), ["Compartilhado"]);
        assert_eq!(by_title(store, "Tarefa de B").project_uid, shared.uid);
        assert!(by_title(store, "Antiga de A").project_uid.is_empty());
    }
}

fn host_with_server() -> (tempfile::TempDir, Store, IntranetConfig, HostServer, Store) {
    let dir = tempfile::tempdir().unwrap();
    let host_path = dir.path().join("host.sqlite3");
    let token = generate_token();
    let mut host = Store::open(&host_path).unwrap();
    let cfg = IntranetConfig {
        mode: IntranetMode::Host,
        member_name: "Servidor A".into(),
        cabinet_token: token.clone(),
        ..IntranetConfig::default()
    };
    host.save_intranet_config(&cfg).unwrap();
    let server = HostServer::start(host_path, "127.0.0.1:0").unwrap();
    let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
    client
        .save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "Servidor B".into(),
            host_url: format!("http://{}", server.local_addr()),
            cabinet_token: token,
            ..IntranetConfig::default()
        })
        .unwrap();
    (dir, host, cfg, server, client)
}

#[test]
fn a_client_changes_projects_through_the_main_computer() {
    let (_dir, host, _cfg, mut server, mut client) = host_with_server();
    let created = make(&mut client, "Mudança");
    assert_eq!(host.list_projects().unwrap()[0].uid, created.uid);
    // Same rules on the client side, told before anything is sent.
    assert!(client.save_project(&Project::new("mudanca")).is_err());
    let id = task_in(&mut client, "Caixas", &created.uid);
    assert_eq!(stored(&host, id).project_uid, created.uid);
    let mut edit = client.get_project(&created.uid).unwrap().unwrap();
    edit.status = ProjectStatus::Concluido;
    client
        .save_project_checked(&edit, Some(client.project_version(&created.uid).unwrap()))
        .unwrap();
    assert_eq!(
        host.get_project(&created.uid).unwrap().unwrap().status,
        ProjectStatus::Concluido
    );
    assert_eq!(
        client.project_version(&created.uid).unwrap(),
        host.project_version(&created.uid).unwrap()
    );
    client.delete_project(&created.uid).unwrap();
    assert!(host.list_projects().unwrap().is_empty());
    // The task stays, with no project, on the host and on the client.
    assert!(stored(&host, id).project_uid.is_empty());
    assert!(stored(&client, id).project_uid.is_empty());
    server.stop();
}

#[test]
fn a_client_working_offline_queues_its_project_changes_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
    client
        .save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "Servidor B".into(),
            host_url: "http://127.0.0.1:9".into(),
            cabinet_token: generate_token(),
            ..IntranetConfig::default()
        })
        .unwrap();
    let created = make(&mut client, "Offline");
    let id = task_in(&mut client, "Criada junto", &created.uid);
    assert_eq!(client.list_offline_outbox().unwrap().len(), 2);
    assert_eq!(stored(&client, id).project_uid, created.uid);
    // An edit and a delete queued on top keep their place.
    let mut edit = client.get_project(&created.uid).unwrap().unwrap();
    edit.assignee = "Ana".into();
    client.save_project(&edit).unwrap();
    client.delete_project(&created.uid).unwrap();
    assert!(client.list_projects().unwrap().is_empty());
    assert!(stored(&client, id).project_uid.is_empty());
    // The delete takes the place of the offline edits of the same project
    // (as for categories); the task queued after it follows it.
    let outbox = client.list_offline_outbox().unwrap();
    assert!(outbox
        .iter()
        .any(|e| matches!(e.mutation, IntranetMutation::DeleteProject { .. })));
    assert!(!outbox
        .iter()
        .any(|e| matches!(e.mutation, IntranetMutation::SaveProject { .. })));
    assert!(matches!(
        outbox.last().unwrap().mutation,
        IntranetMutation::SaveTask { .. }
    ));
}

fn old_request(mutation: IntranetMutation) -> RpcRequest {
    // As a Ponte of schema 20 sends it.
    RpcRequest::Mutate {
        actor: "Servidor B".into(),
        mutation,
        client_schema: crate::intranet::PROJECTS_SCHEMA - 1,
    }
}

#[test]
fn an_older_client_saving_a_task_keeps_the_project_it_cannot_see() {
    let (_dir, mut host, cfg, _server, _client) = host_with_server();
    let project = make(&mut host, "Reforma");
    let id = task_in(&mut host, "Pintar", &project.uid);
    let mut seen = stored(&host, id);
    seen.notes = "Nova observação".into();
    seen.project_uid.clear();
    let reply = process_rpc(
        &mut host,
        &cfg,
        old_request(IntranetMutation::SaveTask {
            task: seen,
            expected_version: None,
        }),
    )
    .unwrap();
    assert!(matches!(reply, RpcResponse::Mutated { .. }));
    let kept = stored(&host, id);
    assert_eq!(kept.notes, "Nova observação");
    assert_eq!(kept.project_uid, project.uid);
    // A current client can still take the project off.
    let mut task = stored(&host, id);
    task.project_uid.clear();
    process_rpc(
        &mut host,
        &cfg,
        RpcRequest::Mutate {
            actor: "Servidor B".into(),
            mutation: IntranetMutation::SaveTask {
                task,
                expected_version: None,
            },
            client_schema: crate::storage::SCHEMA_VERSION,
        },
    )
    .unwrap();
    assert!(stored(&host, id).project_uid.is_empty());
}

#[test]
fn applying_a_project_with_a_taken_name_renames_it_and_ignores_the_limit() {
    let (_dir, mut host, _cfg, _server, _client) = host_with_server();
    make(&mut host, "Mudança");
    let mut clash = Project::new("MUDANÇA");
    clash.uid = "b".repeat(32);
    host.execute_mutation(
        "Servidor B",
        IntranetMutation::SaveProject {
            project: clash,
            expected_version: None,
        },
    )
    .unwrap();
    let mut found = names(&host);
    found.sort();
    assert_eq!(found, ["MUDANÇA (2)", "Mudança"]);
    let long = "x".repeat(MAX_NAME);
    make(&mut host, &long);
    let mut clash = Project::new(&long);
    clash.uid = "c".repeat(32);
    host.execute_mutation(
        "Servidor B",
        IntranetMutation::SaveProject {
            project: clash,
            expected_version: None,
        },
    )
    .unwrap();
    assert!(names(&host).iter().all(|n| n.chars().count() <= MAX_NAME));
    assert!(names(&host).contains(&format!("{} (2)", "x".repeat(MAX_NAME - 4))));
    // Past the limit: replayed changes are not refused.
    for n in 0..MAX_PROJECTS {
        let mut p = Project::new(&format!("Extra {n}"));
        p.uid = format!("{n:032x}");
        host.execute_mutation(
            "Servidor B",
            IntranetMutation::SaveProject {
                project: p,
                expected_version: None,
            },
        )
        .unwrap();
    }
    assert!(host.list_projects().unwrap().len() > MAX_PROJECTS);
    // The person in the interface is still refused.
    assert!(host.save_project(&Project::new("Mais um")).is_err());
}

#[test]
fn deleting_a_project_already_deleted_is_not_a_conflict_when_applied() {
    let (_dir, mut host, _cfg, _server, _client) = host_with_server();
    let made = make(&mut host, "Prazo");
    host.delete_project(&made.uid).unwrap();
    host.execute_mutation(
        "Servidor B",
        IntranetMutation::DeleteProject {
            uid: made.uid.clone(),
            expected_version: None,
        },
    )
    .unwrap();
    // The interface keeps telling it.
    assert!(host.delete_project(&made.uid).is_err());
}
