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
fn a_project_with_a_date_the_older_rule_accepted_can_still_be_edited() {
    let (_dir, mut store) = store();
    // Stored before dates were limited to the years 1900 to 9999.
    let mut old = Project::new("Antigo");
    old.uid = "1".repeat(32);
    old.start_date = "1899-06-01".into();
    old.target_date = "1899-12-31".into();
    apply_projects(&store.conn, &[old.clone()]).unwrap();
    assert!(store.list_projects().unwrap()[0].target().is_some());

    // Renaming and archiving revalidate dates nobody changed: accepted.
    let mut edited = store.list_projects().unwrap()[0].clone();
    edited.name = "Antigo renomeado".into();
    edited.status = ProjectStatus::Arquivado;
    let saved = store.save_project(&edited).unwrap();
    assert_eq!(saved.name, "Antigo renomeado");
    assert_eq!(saved.target_date, "1899-12-31");

    // A date that is changed (or new) follows the strict rule.
    let mut changed = saved.clone();
    changed.target_date = "1850-01-01".into();
    assert!(store.save_project(&changed).is_err());
    changed.target_date = "2030-01-01".into();
    assert!(store.save_project(&changed).is_ok());
    let mut fresh = Project::new("Novo");
    fresh.target_date = "1899-12-31".into();
    assert!(store.save_project(&fresh).is_err());

    // A change replayed from another computer is not lost over such a date.
    let mut remote = Project::new("De outro computador");
    remote.uid = "2".repeat(32);
    remote.start_date = "1800-01-01".into();
    store.save_project_applied(&remote, None).unwrap();

    // It still shows in the overview, the target date included.
    let mut overview_old = Project::new("Visível");
    overview_old.uid = "3".repeat(32);
    overview_old.target_date = "1899-12-31".into();
    assert_eq!(
        overview_old.target(),
        chrono::NaiveDate::from_ymd_opt(1899, 12, 31)
    );
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
        expected_generation: String::new(),
        mutation_uid: String::new(),
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
            no_project: false,
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
                no_project: true,
            },
            client_schema: crate::storage::SCHEMA_VERSION,
            expected_generation: String::new(),
            mutation_uid: String::new(),
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

// -- replication regressions -----------------------------------------------

/// Rewrites the copy at `path` as a Ponte of schema 20 (format 7) writes it:
/// no projects, no project on the tasks, no project versions.
fn rewrite_as_schema_20(path: &std::path::Path, writer: &str, revision: i64) {
    let mut copy = crate::sharedfolder::read(path, KEY).unwrap().unwrap();
    copy.payload.snapshot.projects.clear();
    for task in &mut copy.payload.snapshot.tasks {
        task.project_uid.clear();
    }
    copy.payload
        .snapshot
        .item_versions
        .retain(|(kind, _, _)| kind != "project");
    copy.payload.snapshot.schema_version = crate::intranet::PROJECTS_SCHEMA - 1;
    let h = &copy.header;
    let request = crate::sharedfolder::WriteRequest {
        lineage: &h.lineage,
        revision,
        writer,
        writer_name: "Antigo",
        schema_version: crate::intranet::PROJECTS_SCHEMA - 1,
    };
    crate::sharedfolder::write(path, KEY, &request, &copy.payload, &copy.blobs).unwrap();
}

fn read_copy(path: &std::path::Path) -> crate::sharedfolder::Copy {
    crate::sharedfolder::read(path, KEY).unwrap().unwrap()
}

#[test]
fn a_schema_20_mirror_with_the_higher_revision_does_not_erase_the_projects_of_a_schema_21_one() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("Drive1").join("ponte-sync.ponte");
    let f2 = dir.path().join("Drive2").join("ponte-sync.ponte");
    std::fs::create_dir_all(f1.parent().unwrap()).unwrap();
    std::fs::create_dir_all(f2.parent().unwrap()).unwrap();
    let files = |paths: &[&std::path::Path]| {
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    };

    // An existing folder written by Ponte builds that know nothing of projects.
    let mut z = Store::open(&dir.path().join("z.sqlite3")).unwrap();
    z.save_intranet_config(&folder_config("Servidor Z", &files(&[&f1])))
        .unwrap();
    sync(&mut z);
    rewrite_as_schema_20(&f1, "old-z", read_copy(&f1).header.revision);
    std::fs::copy(&f1, &f2).unwrap();

    // A made a project (and deleted another) while standalone, then joined
    // the folder: the merge carries them into a schema 21 copy with no
    // project change in its journal.
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    let kept = make(&mut a, "Reforma");
    let gone = make(&mut a, "Apagado");
    a.delete_project(&gone.uid).unwrap();
    a.save_intranet_config(&folder_config("Servidor A", &files(&[&f1])))
        .unwrap();
    // A library with its own items joining a folder is asked first (and,
    // confirmed, keeps a verified copy of itself before anything changes).
    let cfg = a.load_intranet_config().unwrap();
    let asked = a
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap_err()
        .to_string();
    assert!(asked.starts_with("[confirmar-substituicao]"), "{asked}");
    a.sync_with_folders(&cfg, KEY, true, &crate::ai::Reporter::silent())
        .unwrap();
    assert!(dir.path().join("copias-antes-da-substituicao").is_dir());
    assert_eq!(names(&a), ["Reforma"]);
    let mirror_of_a = read_copy(&f1);
    assert_eq!(mirror_of_a.header.schema_version, 21);
    assert_eq!(mirror_of_a.payload.snapshot.projects.len(), 1);
    assert!(mirror_of_a.payload.journal.iter().all(|e| !matches!(
        e.mutation,
        IntranetMutation::SaveProject { .. } | IntranetMutation::DeleteProject { .. }
    )));
    let version_of_gone = a.project_version(&gone.uid).unwrap();
    assert!(version_of_gone > 0, "the deletion is remembered");

    // B (a Ponte of schema 20) edits something else in its own mirror, which
    // ends up with the higher revision.
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Servidor B", &files(&[&f2])))
        .unwrap();
    sync(&mut b);
    task_in(&mut b, "Edição de B", "");
    sync(&mut b);
    rewrite_as_schema_20(&f2, "old-b", mirror_of_a.header.revision + 10);

    // C sees both mirrors, takes B's as the base, and has no projects.
    let mut c = Store::open(&dir.path().join("c.sqlite3")).unwrap();
    c.save_intranet_config(&folder_config("Servidor C", &files(&[&f1, &f2])))
        .unwrap();
    sync(&mut c);
    assert_eq!(names(&c), ["Reforma"], "C keeps A's project");
    assert_eq!(c.project_version(&gone.uid).unwrap(), version_of_gone);
    for path in [&f1, &f2] {
        let written = read_copy(path);
        assert_eq!(written.header.schema_version, 21);
        assert_eq!(written.payload.snapshot.projects.len(), 1, "{path:?}");
    }
    assert_eq!(by_title(&c, "Edição de B").title, "Edição de B");

    // And A, on its next exchange, still has what it made.
    sync(&mut a);
    assert_eq!(names(&a), ["Reforma"]);
    assert_eq!(a.get_project(&kept.uid).unwrap().unwrap().uid, kept.uid);
    assert!(a.list_sync_conflicts().unwrap().is_empty());
    assert!(c.list_sync_conflicts().unwrap().is_empty());
}

/// Rewrites a copy with `change` applied to its payload, keeping its
/// lineage, schema and blobs.
fn rewrite_copy(
    path: &std::path::Path,
    writer: &str,
    revision: i64,
    change: impl FnOnce(&mut crate::sharedfolder::Payload),
) {
    let mut copy = read_copy(path);
    change(&mut copy.payload);
    let request = crate::sharedfolder::WriteRequest {
        lineage: &copy.header.lineage,
        revision,
        writer,
        writer_name: writer,
        schema_version: copy.header.schema_version,
    };
    crate::sharedfolder::write(path, KEY, &request, &copy.payload, &copy.blobs).unwrap();
}

fn folder_paths(dir: &std::path::Path, names: &[&str]) -> Vec<std::path::PathBuf> {
    names
        .iter()
        .map(|name| {
            let path = dir.join(name).join("ponte-sync.ponte");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            path
        })
        .collect()
}

fn shown(paths: &[&std::path::Path]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

#[test]
fn a_pending_delete_is_not_undone_by_the_older_copy_of_a_project_grafted_from_the_donor() {
    let dir = tempfile::tempdir().unwrap();
    let [f1, d] = <[_; 2]>::try_from(folder_paths(dir.path(), &["Drive1", "Drive2"])).unwrap();
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    let project = make(&mut a, "Antigo");
    a.save_intranet_config(&folder_config("Servidor A", &shown(&[&f1])))
        .unwrap();
    sync(&mut a);
    // An older mirror, with the project at version 1: the donor.
    std::fs::copy(&f1, &d).unwrap();
    // A renames it (version 2) and publishes; the copy A keeps beside its
    // library is then lost, and the project is deleted while offline.
    let mut renamed = a.get_project(&project.uid).unwrap().unwrap();
    renamed.name = "Renomeado".into();
    a.save_project(&renamed).unwrap();
    sync(&mut a);
    assert_eq!(a.project_version(&project.uid).unwrap(), 2);
    let guard = dir.path().join("a.sqlite3-guarda.ponte");
    let _ = std::fs::remove_file(guard);
    a.delete_project(&project.uid).unwrap();
    assert!(a.list_projects().unwrap().is_empty());
    // The folder A wrote is now seen as a copy of the old format with the
    // higher revision; the donor still holds the project at version 1.
    let revision = read_copy(&f1).header.revision;
    rewrite_as_schema_20(&f1, "old-writer", revision + 10);
    let mut cfg = a.load_intranet_config().unwrap();
    cfg.folder_files = shown(&[&f1, &d]);
    a.save_intranet_config(&cfg).unwrap();
    sync(&mut a);
    assert!(a.list_projects().unwrap().is_empty(), "it stays deleted");
    assert!(a.list_sync_conflicts().unwrap().is_empty());
    assert!(a.list_offline_outbox().unwrap().is_empty());
    for path in [&f1, &d] {
        let written = read_copy(path);
        assert!(written.payload.snapshot.projects.is_empty(), "{path:?}");
    }
}

#[test]
fn a_change_the_donor_refused_is_replayed_not_skipped_as_applied() {
    let dir = tempfile::tempdir().unwrap();
    let [f1, f2, f3] =
        <[_; 3]>::try_from(folder_paths(dir.path(), &["Drive1", "Drive2", "Drive3"])).unwrap();
    let mut author = Store::open(&dir.path().join("author.sqlite3")).unwrap();
    let project = make(&mut author, "Antigo");
    author
        .save_intranet_config(&folder_config("Autor", &shown(&[&f1])))
        .unwrap();
    sync(&mut author);
    let revision = read_copy(&f1).header.revision;
    let version = author.project_version(&project.uid).unwrap();
    // A rename that some copy carries in its journal.
    let mut renamed = project.clone();
    renamed.name = "Renomeado".into();
    let change = crate::sharedfolder::JournalEntry {
        uid: "e".repeat(32),
        created_at: "2026-01-01T00:00:00Z".into(),
        actor: "Outro".into(),
        writer: "w-outro".into(),
        label: "Projeto renomeado".into(),
        mutation: IntranetMutation::SaveProject {
            project: renamed,
            expected_version: Some(version),
        },
        base: None,
        revision,
        created: Vec::new(),
    };
    std::fs::copy(&f1, &f2).unwrap();
    std::fs::copy(&f1, &f3).unwrap();
    // The copy that has it in its journal (the oldest).
    let entry = change.clone();
    rewrite_copy(&f3, "w-outro", revision, |payload| {
        payload.applied.push(entry.uid.clone());
        payload.journal.push(entry);
    });
    // The donor refused it (a conflict on its side): the change is in its
    // `applied`, never in its journal, and its projects do not have it.
    let uid = change.uid.clone();
    rewrite_copy(&f2, "w-doador", revision + 5, |payload| {
        payload.applied.push(uid);
    });
    // The base: a copy of the old format with the highest revision.
    rewrite_as_schema_20(&f1, "old-writer", revision + 10);
    let mut other = Store::open(&dir.path().join("other.sqlite3")).unwrap();
    other
        .save_intranet_config(&folder_config("Outro", &shown(&[&f1, &f2, &f3])))
        .unwrap();
    sync(&mut other);
    let found = other.get_project(&project.uid).unwrap().unwrap();
    assert_eq!(found.name, "Renomeado", "the change was not dropped");
    assert!(other.list_sync_conflicts().unwrap().is_empty());
}

fn live_url(server: &HostServer) -> String {
    format!("http://{}", server.local_addr())
}

/// The client reaches (or stops reaching) its main computer.
fn point_at(client: &mut Store, url: &str) {
    let mut cfg = client.load_intranet_config().unwrap();
    cfg.host_url = url.into();
    client.save_intranet_config(&cfg).unwrap();
}

const DEAD_HOST: &str = "http://127.0.0.1:9";

/// What a Ponte of schema 20 leaves in a library: no projects table, no
/// project on the tasks, nothing recorded of the copy it holds. Opening it
/// again with this build upgrades it.
fn as_left_by_schema_20(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.create_scalar_function(
        "ponte_client_schema",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        |_| Ok(crate::storage::LIBRARY_SCHEMA),
    )
    .unwrap();
    conn.execute_batch("DROP TABLE projects; DROP TABLE client_cache; PRAGMA user_version=20;")
        .unwrap();
    let rows = conn
        .prepare("SELECT id,payload FROM tasks")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for (id, blob) in rows {
        let plain = crate::protection::unseal(&blob).unwrap();
        let mut task: Task = serde_json::from_slice(&plain).unwrap();
        task.project_uid.clear();
        conn.execute(
            "UPDATE tasks SET payload=?1 WHERE id=?2",
            rusqlite::params![
                crate::protection::seal(&serde_json::to_vec(&task).unwrap()).unwrap(),
                id
            ],
        )
        .unwrap();
    }
}

type UpgradedClient = (
    tempfile::TempDir,
    Store,
    IntranetConfig,
    HostServer,
    Store,
    Project,
    i64,
);

/// A client library, synced with a host that has a project and a task in it,
/// then left as a schema 20 build leaves it and upgraded by opening it here.
fn upgraded_client_with_old_cache() -> UpgradedClient {
    let (dir, mut host, cfg, server, mut client) = host_with_server();
    let project = make(&mut host, "Reforma");
    let id = task_in(&mut host, "Pintar", &project.uid);
    client.sync_from_host(true).unwrap();
    assert_eq!(stored(&client, id).project_uid, project.uid);
    drop(client);
    let path = dir.path().join("client.sqlite3");
    as_left_by_schema_20(&path);
    let client = Store::open(&path).unwrap();
    // The premise: what it holds knows nothing of projects, at the very
    // revision the main computer is at.
    assert!(client.list_projects().unwrap().is_empty());
    assert!(stored(&client, id).project_uid.is_empty());
    (dir, host, cfg, server, client, project, id)
}

#[test]
fn an_upgraded_client_asks_for_a_full_copy_even_when_the_revision_is_the_same() {
    let (_dir, host, _cfg, mut server, mut client, project, id) = upgraded_client_with_old_cache();
    assert!(client.sync_from_host(false).unwrap(), "a full copy came");
    assert_eq!(names(&client), ["Reforma"]);
    assert_eq!(
        client.list_projects().unwrap(),
        host.list_projects().unwrap()
    );
    assert_eq!(stored(&client, id).project_uid, project.uid);
    // Settled: the next poll is the usual cheap one.
    assert!(!client.sync_from_host(false).unwrap());
    server.stop();
}

/// Switches the library to Local mode, makes a task and an edit there, and
/// goes back to the main computer. Returns the id of the new task.
fn work_in_local_mode(client: &mut Store, edited: i64) -> i64 {
    let back = client.load_intranet_config().unwrap();
    let mut local = back.clone();
    local.mode = IntranetMode::Local;
    client.save_intranet_config(&local).unwrap();
    let mut mine = Task::standalone();
    mine.title = "Só aqui".into();
    let id = client.save_task(&mine).unwrap();
    let mut task = stored(client, edited);
    task.notes = "Anotada em modo local".into();
    client.save_task(&task).unwrap();
    client.save_intranet_config(&back).unwrap();
    id
}

#[test]
fn an_upgraded_client_at_the_same_revision_takes_only_the_projects_and_keeps_its_local_work() {
    let (_dir, host, _cfg, mut server, mut client, project, id) = upgraded_client_with_old_cache();
    let local_id = work_in_local_mode(&mut client, id);
    assert!(client.sync_from_host(false).unwrap());
    // What the old copy could not carry arrived.
    assert_eq!(
        client.list_projects().unwrap(),
        host.list_projects().unwrap()
    );
    assert_eq!(stored(&client, id).project_uid, project.uid);
    assert_eq!(
        client.project_version(&project.uid).unwrap(),
        host.project_version(&project.uid).unwrap()
    );
    // Nothing else was replaced.
    assert_eq!(by_title(&client, "Só aqui").id, local_id);
    assert_eq!(stored(&client, id).notes, "Anotada em modo local");
    assert!(!host
        .list_tasks()
        .unwrap()
        .iter()
        .any(|t| t.title == "Só aqui"));
    // Done once.
    assert!(!client.sync_from_host(false).unwrap());
    assert_eq!(by_title(&client, "Só aqui").id, local_id);
    server.stop();
}

#[test]
fn an_upgraded_client_still_takes_the_full_copy_when_the_revision_moved() {
    let (_dir, mut host, _cfg, mut server, mut client, project, id) =
        upgraded_client_with_old_cache();
    work_in_local_mode(&mut client, id);
    task_in(&mut host, "Nova no anfitrião", "");
    assert!(client.sync_from_host(false).unwrap());
    assert_eq!(client.list_tasks().unwrap(), host.list_tasks().unwrap());
    assert_eq!(stored(&client, id).project_uid, project.uid);
    assert!(!client.sync_from_host(false).unwrap());
    server.stop();
}

#[test]
fn a_task_saved_right_after_the_upgrade_keeps_the_link_the_old_copy_never_showed() {
    let (_dir, host, _cfg, mut server, mut client, project, id) = upgraded_client_with_old_cache();
    // Before any refresh: the screen shows the task without a project.
    let mut seen = stored(&client, id);
    seen.notes = "Depois da atualização".into();
    let version = client.item_version("task", id).unwrap().version;
    client.save_task_checked(&seen, Some(version)).unwrap();
    let on_host = stored(&host, id);
    assert_eq!(on_host.notes, "Depois da atualização");
    assert_eq!(on_host.project_uid, project.uid, "the link was not touched");
    server.stop();
}

#[test]
fn a_task_edit_queued_before_the_upgrade_does_not_clear_the_link_when_replayed() {
    let (_dir, host, _cfg, mut server, mut client, project, id) = upgraded_client_with_old_cache();
    let live = live_url(&server);
    point_at(&mut client, DEAD_HOST);
    let mut seen = stored(&client, id);
    seen.notes = "Feita sem rede".into();
    client.save_task(&seen).unwrap();
    assert_eq!(client.list_offline_outbox().unwrap().len(), 1);
    // As an older Ponte wrote it: no word about the project.
    let queued = serde_json::to_string(&client.list_offline_outbox().unwrap()[0].mutation).unwrap();
    assert!(!queued.contains("no_project") && !queued.contains("project_uid"));
    point_at(&mut client, &live);
    client.sync_from_host(false).unwrap();
    let on_host = stored(&host, id);
    assert_eq!(on_host.notes, "Feita sem rede");
    assert_eq!(on_host.project_uid, project.uid);
    assert_eq!(stored(&client, id).project_uid, project.uid);
    assert!(client.list_sync_conflicts().unwrap().is_empty());
    server.stop();
}

#[test]
fn only_an_explicit_removal_takes_the_project_off_a_task_through_the_main_computer() {
    let (_dir, mut host, cfg, _server, _client) = host_with_server();
    let project = make(&mut host, "Reforma");
    let id = task_in(&mut host, "Pintar", &project.uid);
    let send = |host: &mut Store, task: Task, no_project: bool| {
        process_rpc(
            host,
            &cfg,
            RpcRequest::Mutate {
                actor: "Servidor B".into(),
                mutation: IntranetMutation::SaveTask {
                    task,
                    expected_version: None,
                    no_project,
                },
                client_schema: crate::storage::SCHEMA_VERSION,
                expected_generation: String::new(),
                mutation_uid: String::new(),
            },
        )
        .unwrap();
    };
    let mut edit = stored(&host, id);
    edit.notes = "Sem mexer no projeto".into();
    edit.project_uid.clear();
    send(&mut host, edit.clone(), false);
    assert_eq!(stored(&host, id).project_uid, project.uid);
    assert_eq!(stored(&host, id).notes, "Sem mexer no projeto");
    send(&mut host, edit.clone(), true);
    assert!(stored(&host, id).project_uid.is_empty());
    // A flag that says nothing is not written (what an older build sends).
    let json = serde_json::to_string(&IntranetMutation::SaveTask {
        task: edit,
        expected_version: None,
        no_project: false,
    })
    .unwrap();
    assert!(!json.contains("no_project"));
    assert!(matches!(
        serde_json::from_str::<IntranetMutation>(&json).unwrap(),
        IntranetMutation::SaveTask {
            no_project: false,
            ..
        }
    ));
}

#[test]
fn a_person_removing_the_project_of_a_task_still_clears_it_everywhere() {
    let (_dir, host, _cfg, mut server, mut client) = host_with_server();
    let project = make(&mut client, "Reforma");
    let id = task_in(&mut client, "Pintar", &project.uid);
    assert_eq!(stored(&host, id).project_uid, project.uid);
    let mut task = stored(&client, id);
    task.project_uid.clear();
    client.save_task(&task).unwrap();
    assert!(stored(&host, id).project_uid.is_empty());
    assert!(stored(&client, id).project_uid.is_empty());
    // And offline, queued and replayed (together with edits of the notes
    // around it, which take its place in the queue).
    let live = live_url(&server);
    let mut task = stored(&client, id);
    task.project_uid = project.uid.clone();
    client.save_task(&task).unwrap();
    assert_eq!(stored(&host, id).project_uid, project.uid);
    point_at(&mut client, DEAD_HOST);
    let mut task = stored(&client, id);
    task.notes = "Primeiro".into();
    client.save_task(&task).unwrap();
    let mut task = stored(&client, id);
    task.project_uid.clear();
    client.save_task(&task).unwrap();
    let mut task = stored(&client, id);
    task.notes = "Depois".into();
    client.save_task(&task).unwrap();
    assert_eq!(client.list_offline_outbox().unwrap().len(), 1, "coalesced");
    point_at(&mut client, &live);
    client.sync_from_host(false).unwrap();
    assert!(
        stored(&host, id).project_uid.is_empty(),
        "the removal stays"
    );
    assert_eq!(stored(&host, id).notes, "Depois");
    server.stop();
}

fn position(outbox: &[crate::intranet::OutboxEntry], is: fn(&IntranetMutation) -> bool) -> usize {
    outbox.iter().position(|e| is(&e.mutation)).unwrap()
}

#[test]
fn a_task_assigned_to_a_project_created_after_its_first_queued_edit_follows_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, mut b, _) = pair(dir.path());
    let id = task_in(&mut a, "Alvo", "");
    sync(&mut a);
    sync(&mut b);
    // Offline: edit the existing task, create a project, assign the task.
    let mut task = stored(&a, id);
    task.notes = "Anotação".into();
    a.save_task(&task).unwrap();
    let project = make(&mut a, "Novo projeto");
    let mut task = stored(&a, id);
    task.project_uid = project.uid.clone();
    a.save_task(&task).unwrap();
    let outbox = a.list_offline_outbox().unwrap();
    assert_eq!(outbox.len(), 2, "the two saves of the task are one");
    let created = position(&outbox, |m| {
        matches!(m, IntranetMutation::SaveProject { .. })
    });
    let assigned = position(&outbox, |m| matches!(m, IntranetMutation::SaveTask { .. }));
    assert!(
        created < assigned,
        "the project exists before the task names it"
    );
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        let found = stored(store, id);
        assert_eq!(found.project_uid, project.uid);
        assert_eq!(found.notes, "Anotação");
        assert!(store.list_sync_conflicts().unwrap().is_empty());
    }
}

#[test]
fn the_queue_of_a_client_puts_a_project_before_the_task_that_names_it() {
    let (_dir, host, _cfg, mut server, mut client) = host_with_server();
    let live = live_url(&server);
    let id = task_in(&mut client, "Alvo", "");
    point_at(&mut client, DEAD_HOST);
    // Offline: edit the existing task, create a project, assign the task.
    let mut task = stored(&client, id);
    task.notes = "Anotação".into();
    client.save_task(&task).unwrap();
    let project = make(&mut client, "Novo");
    let mut task = stored(&client, id);
    task.project_uid = project.uid.clone();
    client.save_task(&task).unwrap();
    let outbox = client.list_offline_outbox().unwrap();
    assert_eq!(outbox.len(), 2);
    assert!(matches!(
        outbox[0].mutation,
        IntranetMutation::SaveProject { .. }
    ));
    assert!(matches!(
        &outbox[1].mutation,
        IntranetMutation::SaveTask { task, .. } if task.project_uid == project.uid
    ));
    point_at(&mut client, &live);
    client.sync_from_host(false).unwrap();
    assert_eq!(stored(&host, id).project_uid, project.uid);
    assert_eq!(stored(&host, id).notes, "Anotação");
    assert!(client.list_sync_conflicts().unwrap().is_empty());
    server.stop();
}

#[test]
fn clearing_the_project_on_purpose_is_queued_even_when_the_copy_had_none() {
    let (_dir, mut host, _cfg, mut server, mut client) = host_with_server();
    let live = live_url(&server);
    let project = make(&mut host, "Reforma");
    let id = task_in(&mut client, "Alvo", "");
    let other = task_in(&mut client, "Outra", "");
    point_at(&mut client, DEAD_HOST);
    let flag_of = |store: &Store, id: i64| {
        store
            .list_offline_outbox()
            .unwrap()
            .iter()
            .rev()
            .find_map(|e| match &e.mutation {
                IntranetMutation::SaveTask {
                    task, no_project, ..
                } if task.id == id => Some(*no_project),
                _ => None,
            })
            .unwrap()
    };
    // The cached copy has no link: a plain save says nothing about it, the
    // explicit one says "none".
    let mut plain = stored(&client, other);
    plain.notes = "Só a anotação".into();
    client.save_task(&plain).unwrap();
    assert!(!flag_of(&client, other));
    let mut cleared = stored(&client, id);
    cleared.notes = "Tirada do projeto".into();
    let version = client.item_version("task", id).unwrap().version;
    client
        .save_task_clearing_project(&cleared, Some(version))
        .unwrap();
    assert!(flag_of(&client, id));
    // Meanwhile another computer put the task in a project: the explicit
    // removal still wins when the queue is replayed, the plain one does not.
    let mut linked = stored(&host, id);
    linked.project_uid = project.uid.clone();
    host.save_task(&linked).unwrap();
    let mut linked = stored(&host, other);
    linked.project_uid = project.uid.clone();
    host.save_task(&linked).unwrap();
    point_at(&mut client, &live);
    client.sync_from_host(false).unwrap();
    // Both saves hit a newer task: each waits for the person, who merges.
    for conflict in client.list_sync_conflicts().unwrap() {
        client
            .resolve_sync_conflict(conflict.id, crate::intranet::ConflictResolution::Merge)
            .unwrap();
    }
    assert!(stored(&host, id).project_uid.is_empty(), "explicit removal");
    assert_eq!(
        stored(&host, id).notes.lines().next(),
        Some("Tirada do projeto")
    );
    assert_eq!(stored(&host, other).project_uid, project.uid, "plain save");
    server.stop();
}

#[test]
fn an_explicit_removal_survives_the_merge_with_a_link_given_meanwhile_in_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (mut a, mut b, _) = pair(dir.path());
    let id = task_in(&mut a, "Alvo", "");
    let other = task_in(&mut a, "Outra", "");
    sync(&mut a);
    sync(&mut b);
    let project = make(&mut b, "Reforma");
    // A, with the copy that has no link, takes the project off one task on
    // purpose and only edits the notes of the other.
    let mut cleared = stored(&a, id);
    cleared.notes = "Tirada do projeto".into();
    let version = a.item_version("task", id).unwrap().version;
    a.save_task_clearing_project(&cleared, Some(version))
        .unwrap();
    let mut plain = stored(&a, other);
    plain.notes = "Só a anotação".into();
    a.save_task(&plain).unwrap();
    // Meanwhile B puts both in the project.
    for target in [id, other] {
        let mut linked = stored(&b, target);
        linked.project_uid = project.uid.clone();
        b.save_task(&linked).unwrap();
    }
    sync(&mut b);
    sync(&mut a);
    // Both of A's saves meet a newer task: A merges them.
    for conflict in a.list_sync_conflicts().unwrap() {
        a.resolve_sync_conflict(conflict.id, crate::intranet::ConflictResolution::Merge)
            .unwrap();
    }
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert!(stored(store, id).project_uid.is_empty(), "explicit removal");
        assert_eq!(stored(store, other).project_uid, project.uid, "plain save");
        assert_eq!(stored(store, other).notes, "Só a anotação");
    }
}

/// A task and one that depends on it, both open.
fn chained(store: &mut Store) -> (i64, i64) {
    let a = task_in(store, "Primeira", "");
    let mut second = Task::standalone();
    second.title = "Segunda".into();
    second.dependencies = vec![a];
    let b = store.save_task(&second).unwrap();
    (a, b)
}

/// Offline: complete A, complete B (which needs A), create a project and put
/// A in it. Returns the project.
fn complete_chain_then_assign(store: &mut Store, a: i64, b: i64) -> Project {
    store.complete_task(a).unwrap();
    store.complete_task(b).unwrap();
    let project = make(store, "Depois");
    let mut task = stored(store, a);
    task.project_uid = project.uid.clone();
    store.save_task(&task).unwrap();
    project
}

fn assert_chain_queue_in_order(outbox: &[crate::intranet::OutboxEntry], a: i64, b: i64) {
    let of = |id: i64| {
        outbox
            .iter()
            .position(|e| matches!(&e.mutation, IntranetMutation::SaveTask { task, .. } if task.id == id && task.status == "concluida"))
            .unwrap()
    };
    // The entry that completes A stays before the one that completes B.
    assert!(of(a) < of(b), "A is completed before B in the queue");
    let last = outbox.last().unwrap();
    assert!(
        matches!(&last.mutation, IntranetMutation::SaveTask { task, .. } if task.id == a && !task.project_uid.is_empty()),
        "the assignment goes at the end"
    );
}

#[test]
fn a_task_assigned_to_a_later_project_does_not_move_behind_the_task_that_needs_it_done_on_a_client()
{
    let (_dir, host, _cfg, mut server, mut client) = host_with_server();
    let live = live_url(&server);
    let (a, b) = chained(&mut client);
    point_at(&mut client, DEAD_HOST);
    let project = complete_chain_then_assign(&mut client, a, b);
    assert_chain_queue_in_order(&client.list_offline_outbox().unwrap(), a, b);
    point_at(&mut client, &live);
    client.sync_from_host(false).unwrap();
    assert!(client.list_offline_outbox().unwrap().is_empty());
    assert!(client.list_sync_conflicts().unwrap().is_empty());
    for store in [&host, &client] {
        assert_eq!(stored(store, b).status, "concluida", "B was accepted");
        assert_eq!(stored(store, a).status, "concluida");
        assert_eq!(stored(store, a).project_uid, project.uid);
    }
    server.stop();
}

#[test]
fn a_task_assigned_to_a_later_project_does_not_move_behind_the_task_that_needs_it_done_in_the_folder(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut a_store, mut b_store, _) = pair(dir.path());
    let (a, b) = chained(&mut a_store);
    sync(&mut a_store);
    sync(&mut b_store);
    let project = complete_chain_then_assign(&mut a_store, a, b);
    assert_chain_queue_in_order(&a_store.list_offline_outbox().unwrap(), a, b);
    sync(&mut a_store);
    sync(&mut b_store);
    for store in [&a_store, &b_store] {
        assert_eq!(stored(store, b).status, "concluida", "B was accepted");
        assert_eq!(stored(store, a).project_uid, project.uid);
        assert!(store.list_sync_conflicts().unwrap().is_empty());
    }
}

fn child_of(parent: i64, title: &str) -> Task {
    let mut task = Task::standalone();
    task.title = title.into();
    task.parent_task_id = Some(parent);
    task
}

#[test]
fn a_subtask_made_offline_keeps_the_project_it_inherited_when_the_parent_moves() {
    let (_dir, mut host, _cfg, mut server, mut client) = host_with_server();
    let live = live_url(&server);
    let p = make(&mut client, "P");
    let q = make(&mut client, "Q");
    let parent = task_in(&mut client, "Pai", &p.uid);
    point_at(&mut client, DEAD_HOST);
    client.save_task(&child_of(parent, "Filha")).unwrap();
    // The one chosen on purpose to have none.
    client
        .save_task_without_project(&child_of(parent, "Avulsa"))
        .unwrap();
    assert_eq!(by_title(&client, "Filha").project_uid, p.uid);
    assert!(by_title(&client, "Avulsa").project_uid.is_empty());
    // The queue carries what the person saw, not a blank to fill in later.
    let outbox = client.list_offline_outbox().unwrap();
    let queued = |title: &str| {
        outbox
            .iter()
            .find_map(|e| match &e.mutation {
                IntranetMutation::SaveTask {
                    task, no_project, ..
                } if task.title == title => Some((task.project_uid.clone(), *no_project)),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(queued("Filha"), (p.uid.clone(), false));
    assert_eq!(queued("Avulsa"), (String::new(), true));
    // Meanwhile the parent moves.
    let mut moved = stored(&host, parent);
    moved.project_uid = q.uid.clone();
    host.save_task(&moved).unwrap();
    point_at(&mut client, &live);
    client.sync_from_host(false).unwrap();
    for store in [&host, &client] {
        assert_eq!(by_title(store, "Filha").project_uid, p.uid);
        assert!(by_title(store, "Avulsa").project_uid.is_empty());
        assert_eq!(stored(store, parent).project_uid, q.uid);
    }
    server.stop();
}

#[test]
fn the_first_publisher_of_a_folder_settles_the_project_of_a_new_subtask_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    let files = vec![folder.display().to_string()];
    // A has its own library, with a parent task in a project, and is the
    // first to publish it.
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    let p = make(&mut a, "P");
    let q = make(&mut a, "Q");
    let parent = task_in(&mut a, "Pai", &p.uid);
    a.save_intranet_config(&folder_config("Servidor A", &files))
        .unwrap();
    sync(&mut a);
    assert!(!a.projects_cache_stale(), "the first copy is of this build");
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Servidor B", &files))
        .unwrap();
    sync(&mut b);
    assert!(!b.projects_cache_stale());
    // A makes a subtask without naming a project: it carries what A saw.
    a.save_task(&child_of(parent, "Filha")).unwrap();
    assert_eq!(by_title(&a, "Filha").project_uid, p.uid);
    let queued = a.list_offline_outbox().unwrap();
    assert!(
        queued.iter().any(|e| matches!(
            &e.mutation,
            IntranetMutation::SaveTask { task, .. } if task.title == "Filha" && task.project_uid == p.uid
        )),
        "the queued create names the project the person saw"
    );
    // B moves the parent and publishes first.
    let mut moved = stored(&b, parent);
    moved.project_uid = q.uid.clone();
    b.save_task(&moved).unwrap();
    sync(&mut b);
    sync(&mut a);
    sync(&mut b);
    for store in [&a, &b] {
        assert_eq!(by_title(store, "Filha").project_uid, p.uid);
        assert_eq!(stored(store, parent).project_uid, q.uid);
    }
}

#[test]
fn repeating_the_creation_of_a_subtask_never_changes_the_project_it_got() {
    let (_dir, mut host, _cfg, _server, _client) = host_with_server();
    let p = make(&mut host, "P");
    let q = make(&mut host, "Q");
    let parent = task_in(&mut host, "Pai", &p.uid);
    let child = child_of(parent, "Filha");
    let create = || IntranetMutation::SaveTask {
        task: child.clone(),
        expected_version: None,
        no_project: false,
    };
    host.execute_mutation("Servidor B", create()).unwrap();
    assert_eq!(by_title(&host, "Filha").project_uid, p.uid);
    // The reply was lost and the same creation arrives again: an empty
    // project in it is a default, not a removal.
    host.execute_mutation("Servidor B", create()).unwrap();
    assert_eq!(by_title(&host, "Filha").project_uid, p.uid);
    // Moved meanwhile by someone: a late retry does not undo it.
    let mut moved = by_title(&host, "Filha");
    moved.project_uid = q.uid.clone();
    host.save_task(&moved).unwrap();
    host.execute_mutation("Servidor B", create()).unwrap();
    assert_eq!(by_title(&host, "Filha").project_uid, q.uid);
    assert_eq!(
        host.list_tasks()
            .unwrap()
            .iter()
            .filter(|t| t.title == "Filha")
            .count(),
        1
    );
}

#[test]
fn a_subtask_created_with_no_project_has_none_even_under_a_parent_in_one() {
    let (_dir, mut store) = store();
    let p = make(&mut store, "P");
    let parent = task_in(&mut store, "Pai", &p.uid);
    store
        .save_task_without_project(&child_of(parent, "Avulsa"))
        .unwrap();
    store.save_task(&child_of(parent, "Herdeira")).unwrap();
    assert!(by_title(&store, "Avulsa").project_uid.is_empty());
    assert_eq!(by_title(&store, "Herdeira").project_uid, p.uid);
    // Only a new task without a project can be created this way.
    let mut named = child_of(parent, "Nomeada");
    named.project_uid = p.uid.clone();
    assert!(store.save_task_without_project(&named).is_err());
    let mut saved = by_title(&store, "Herdeira");
    saved.title = "Outra".into();
    assert!(store.save_task_without_project(&saved).is_err());
}

#[test]
fn schema_21_write_guards_survive_an_older_build_rewriting_the_ones_it_knows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guards.sqlite3");
    drop(Store::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    // An older Ponte that opens the library meanwhile: its connection says 20,
    // and it drops and writes again every guard it knows by name, with 20.
    conn.create_scalar_function(
        "ponte_client_schema",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        |_| Ok(20_i64),
    )
    .unwrap();
    for table in ["item_versions", "tasks", "offline_outbox"] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            let name = format!("ponte_guard_{table}_{}", operation.to_lowercase());
            conn.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}; CREATE TRIGGER {name} BEFORE {operation} ON {table} BEGIN SELECT CASE WHEN ponte_client_schema() < 20 THEN RAISE(ABORT,'Use a versão atual do Ponte para salvar alterações.') END; END;"))
                .unwrap();
        }
    }
    // Its writes are still refused.
    let error = conn
        .execute(
            "INSERT INTO item_versions(kind,item_id,version,payload) VALUES('x',1,1,x'00')",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("Use a versão atual do Ponte"), "{error}");
    // And the guards of the projects exist under both names.
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'ponte_guard%projects%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 6);
}

#[test]
fn a_restored_library_joining_an_old_folder_publishes_nothing_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("Drive1").join("ponte-sync.ponte");
    std::fs::create_dir_all(f1.parent().unwrap()).unwrap();
    let files = vec![f1.display().to_string()];
    // A folder written by a Ponte that knows nothing of projects.
    let mut z = Store::open(&dir.path().join("z.sqlite3")).unwrap();
    z.save_intranet_config(&folder_config("Servidor Z", &files))
        .unwrap();
    sync(&mut z);
    rewrite_as_schema_20(&f1, "old-z", read_copy(&f1).header.revision);
    // A restored library (its folder has the marker of a finished recovery
    // library) with a project of its own.
    let folder = dir.path().join("restaurado");
    std::fs::create_dir_all(&folder).unwrap();
    let marker = crate::recovery::Marker {
        versao: 1,
        tipo: "copia-local".into(),
        origem: "copia.sqlite3".into(),
        copia_de: String::new(),
        montado_em: "2026-11-22T10:15:00".into(),
        pasta_principal: None,
        documentos: 0,
        esquema_da_copia: 21,
        desligado: Vec::new(),
        completo: true,
    };
    std::fs::write(
        folder.join(crate::recovery::MARKER_FILE),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    let mut a = Store::open(&crate::paths::db_path_in(&folder)).unwrap();
    make(&mut a, "Projeto restaurado");
    a.save_intranet_config(&folder_config("Restaurado", &files))
        .unwrap();
    let cfg = a.load_intranet_config().unwrap();
    let asked = a
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap_err()
        .to_string();
    assert!(asked.contains("1 projeto"), "{asked}");
    a.sync_with_folders(&cfg, KEY, true, &crate::ai::Reporter::silent())
        .unwrap();
    // What the question said: replaced, nothing of it in the folder.
    assert!(names(&a).is_empty(), "{:?}", names(&a));
    assert!(read_copy(&f1).payload.snapshot.projects.is_empty());
    assert!(folder.join("copias-antes-da-substituicao").is_dir());
}
