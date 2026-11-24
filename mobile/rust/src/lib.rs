//! JNI entry points of the Ponte core for the Android prototype ("Ponte de
//! bolso"). The phone is one more member of the shared-folder mode: the app
//! copies `ponte-sync.ponte` from Google Drive into a private working file, this core runs
//! the very same synchronization the desktop runs on that local copy, and
//! the app writes the copy back to Drive only when nobody changed it there
//! meanwhile.
//!
//! Every function answers a JSON string, `{"ok":true,...}` or
//! `{"ok":false,"error":"..."}` (an error can also carry a `"code"`:
//! `"target_changed"`, `"target_gone"` with a `"card_id"`, `"file_refused"`);
//! nothing panics across the JNI boundary.
use anyhow::{Context, Result};
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;
use ponteproj::intranet::{IntranetConfig, IntranetMode};
use ponteproj::storage::Store;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

/// How long the id of a committed operation is kept (see `operationStatus`).
const OPERATION_DAYS: i64 = 30;

/// The one library of this app, opened by `init`.
static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn with_store<T>(work: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    let mut guard = STORE
        .lock()
        .map_err(|_| anyhow::anyhow!("O acervo ficou indisponível; feche e abra o aplicativo."))?;
    let store = guard
        .as_mut()
        .context("O acervo ainda não foi aberto.")?;
    work(store)
}

/// The answer for an error: `{"ok":false,"error":"..."}`, plus `"code"` for the
/// errors the app tells apart: `target_changed`; `target_gone` with the
/// `card_id` that keeps the refused file (F3.1b2); `file_refused`, the file
/// itself, which no record would take. Any other error carries no code: the
/// app keeps the file it holds (`revisao-f3-1b2.md` finding 1).
fn error_answer(error: &anyhow::Error) -> Value {
    use ponteproj::attachments::{FileRefused, TargetChanged, TargetGone};
    let mut value = json!({"ok": false, "error": format!("{error:#}")});
    let find = |is: &dyn Fn(&(dyn std::error::Error + 'static)) -> bool| error.chain().any(is);
    if find(&|e| e.is::<TargetChanged>()) {
        value["code"] = json!("target_changed");
    }
    if find(&|e| e.is::<FileRefused>()) {
        value["code"] = json!("file_refused");
    }
    if let Some(gone) = error.chain().find_map(|e| e.downcast_ref::<TargetGone>()) {
        value["code"] = json!("target_gone");
        value["card_id"] = json!(gone.card_id);
    }
    value
}

/// Runs `work`, turning its result (or a panic) into the JSON answer.
fn answer(env: &mut JNIEnv, work: impl FnOnce(&mut JNIEnv) -> Result<Value>) -> jstring {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(env)));
    let value = match outcome {
        Ok(Ok(mut value)) => {
            if let Some(map) = value.as_object_mut() {
                map.insert("ok".into(), json!(true));
            }
            value
        }
        Ok(Err(error)) => error_answer(&error),
        Err(_) => json!({"ok": false, "error": "Falha interna no núcleo do Ponte."}),
    };
    env.new_string(value.to_string())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn text(env: &mut JNIEnv, value: &JString) -> Result<String> {
    Ok(env.get_string(value).context("Texto inválido vindo do aplicativo.")?.into())
}

/// Opens (or creates) the library in the app's private folder, sealed with
/// the key the app keeps in the Android Keystore, and makes it a member of
/// the shared-folder mode under this phone's name.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_init<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
    library_key: JByteArray<'local>,
    member_name: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let dir = PathBuf::from(text(env, &data_dir)?);
        let member = text(env, &member_name)?;
        let key = env
            .convert_byte_array(&library_key)
            .context("Chave do acervo inválida.")?;
        let mut notice: Option<String> = None;
        let mut guard = STORE
            .lock()
            .map_err(|_| anyhow::anyhow!("O acervo ficou indisponível."))?;
        if guard.is_none() {
            // The core makes temporary libraries under its data folder (a
            // merge of diverged copies): on Android that is the app's own
            // private folder, never the default user folders.
            std::env::set_var("PONTEPROJ_DATA_DIR", &dir);
            // The phone (not Windows) keeps the key given by the app; the
            // Windows host build, used only by tests, has no such call.
            #[cfg(windows)]
            let _ = &key;
            #[cfg(not(windows))]
            ponteproj::protection::set_platform_key(&key)?;
            let mut store = Store::open(&dir.join("acervo.sqlite3"))?;
            // The minimum format of the folder this phone has seen lives in
            // the app's private folder, outside the library (F2b).
            store.set_device_dir(dir.clone());
            // Opened as it was (its update could not be done now): said to
            // the app, which shows it; the next opening tries again.
            notice = store.take_upgrade_notice();
            let mut cfg = store.load_intranet_config()?;
            if cfg.mode != IntranetMode::SharedFolder || cfg.folder_writer.is_empty() {
                cfg = IntranetConfig {
                    mode: IntranetMode::SharedFolder,
                    member_name: member.trim().to_owned(),
                    folder_files: vec![dir.join("ponte-sync.ponte").display().to_string()],
                    // The passphrase is typed in each session, never kept.
                    remember_folder_key: false,
                    folder_writer: uuid::Uuid::new_v4().simple().to_string(),
                    ..cfg
                };
                store.save_intranet_config(&cfg)?;
            }
            // Once per process start: the operation ids older than a month
            // (the app asks about one only after a process death) go away.
            let _ = store.prune_operations(OPERATION_DAYS);
            *guard = Some(store);
        }
        Ok(match notice {
            Some(text) => json!({ "aviso": text }),
            None => json!({}),
        })
    })
}

/// One exchange with the local copy of the transport file at
/// `transport_path` (the app copied it from Drive) and, when there is one,
/// the copy this phone wrote but could not confirm on Drive yet
/// (`pending_path`): the core reads both, so a change that never reached
/// Drive is merged again. `written` says whether the copy changed and must
/// go back to Drive.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_sync<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    transport_path: JString<'local>,
    pending_path: JString<'local>,
    passphrase: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let path = text(env, &transport_path)?;
        let pending = text(env, &pending_path)?;
        let key = text(env, &passphrase)?;
        anyhow::ensure!(!key.is_empty(), "Digite a frase-senha da sincronização.");
        with_store(|store| sync_paths(store, &path, &pending, &key))
    })
}

/// The exchange itself, on plain paths (no JVM needed, so tests run it).
///
/// The core reads the whole transport file into native memory to decrypt it
/// (AES-GCM is opened in one piece) and rewrites `path` atomically (temporary
/// name, then rename), so a failed run leaves `path` exactly as it was and no
/// partial file. When the copy changed, the answer carries `bytes`, the length
/// of the file now at `path`, so the app can check what it will upload.
fn sync_paths(store: &mut Store, path: &str, pending: &str, key: &str) -> Result<Value> {
    // The phone never promotes the folder to the shared file of format 9,
    // and a stub asks the person to pick the new file (F2b).
    store.set_phone_role();
    anyhow::ensure!(
        std::path::Path::new(path).is_file(),
        "O arquivo de sincronização não foi encontrado."
    );
    let mut files = vec![path.to_owned()];
    // Every pending copy the app keeps, one per line: the one kept by an
    // earlier exchange and any output staged beside it when that one could
    // not be replaced ("pendente-novo"). Each is read again, so what only a
    // staged copy holds is merged (or kept aside) like the rest.
    //
    // Copies with the same content are read once (the later one follows the
    // fate of the first), and at most MAX_PENDING are read: the oldest. The
    // others stay exactly where they are, unsettled, for the next exchanges
    // (never set aside: they may hold work not sent yet), with a notice; so a
    // run of failed uploads never stops the exchanges.
    let (pendings, twins, waiting) = active_pendings(path, pending)?;
    files.extend(pendings);
    // The paths are an input of this exchange, not something saved up front:
    // a file this build refuses (newer format or schema) must change nothing
    // here, and saving the settings would also reset the settled ID marks.
    // A good exchange stores them with the rest of its state.
    let mut cfg = store.load_intranet_config()?;
    cfg.folder_files = files;
    let reporter = ponteproj::ai::Reporter::silent();
    let report = match store.sync_with_folders(&cfg, key, false, &reporter) {
        Ok(report) => report,
        // Joining a library: the phone never creates documents on
        // its own, so with nothing waiting to be sent it simply
        // adopts the copy in the folder.
        Err(error)
            if error.to_string().starts_with("[confirmar-substituicao]")
                && store.list_offline_outbox()?.is_empty() =>
        {
            store.sync_with_folders(&cfg, key, true, &reporter)?
        }
        Err(error) => return Err(error),
    };
    // A copy read once for its twin is settled with it; one left for the
    // next exchange is not settled. Nor is one that could not be rewritten
    // (its archive failed, or the write did): it is as it was, and may hold
    // the only copy of some work; the working copy among them keeps every
    // pending copy (nothing reached Drive).
    let mut unsettled = report.unsettled.clone();
    unsettled.extend(waiting.iter().cloned());
    for path in &report.unwritten {
        if !unsettled.contains(path) {
            unsettled.push(path.clone());
        }
    }
    for (twin, first) in &twins {
        if unsettled.iter().any(|u| u == first) && !unsettled.contains(twin) {
            unsettled.push(twin.clone());
        }
    }
    // Written means: the copy at `path` changed (the pending one is
    // rewritten along with it).
    let written = report
        .written_to
        .iter()
        .any(|w| std::path::Path::new(w) == std::path::Path::new(path));
    let mut answer = json!({
        "written": written,
        "summary": report.summary(),
        "waiting": store.list_offline_outbox()?.len(),
        "conflicts": report.conflicts,
        // False when a copy with work the exchange could not merge was left
        // as it is: the app must keep the pending copy then.
        //
        // Also false when the working copy needed a rewrite that did not
        // succeed: the pending copy may then be the newest one there is, and
        // nothing has reached Drive yet.
        //
        // And false unless every copy read is proved to be in the result:
        // rewritten with it, equal to it, or kept aside.
        "pending_settled": report.preserve_failed.is_empty()
            && unsettled.is_empty()
            && !report
                .unwritten
                .iter()
                .any(|w| std::path::Path::new(w) == std::path::Path::new(path)),
        // What the person must know about what was kept aside (the items
        // and where the copy is), in the computer's words.
        "notes": waiting_notice(&waiting).into_iter().chain(report.notes.iter().cloned()).collect::<Vec<_>>(),
        // Each copy read whose content is not proved to be in the result:
        // the app keeps exactly these and may let the others go (every
        // other pending copy is settled on its own).
        "unsettled": unsettled,
        // How many copies could not be rewritten (also in the summary).
        "unwritten": report.unwritten.len(),
        // Records of this phone that the copy numbered differently (also in
        // the summary, in one sentence).
        "renumbered": report
            .renumbered
            .iter()
            .map(|n| json!({
                "title": n.title,
                "from": n.from,
                "to": n.to,
                "sentence": n.sentence(),
            }))
            .collect::<Vec<_>>(),
    });
    // The holidays are the library's: what another device saved is now in
    // force here, and the app refreshes its own copy from this.
    if let Some(holidays) = holidays_after_sync(store) {
        answer["holidays"] = holidays["text"].clone();
    }
    if written {
        answer["bytes"] = json!(std::fs::metadata(path)?.len());
    }
    Ok(answer)
}

/// How many pending copies one exchange reads (the transport file is the
/// other path of the list, whose limit is 8).
const MAX_PENDING: usize = 6;

/// The pending copies listed by the app (one per line) that this exchange
/// reads: distinct paths that exist, one per content (the twins of a copy, as
/// (twin, first), follow its fate), at most [`MAX_PENDING`], the oldest
/// first; and the ones left for the next exchanges (untouched).
fn active_pendings(
    work: &str,
    listed: &str,
) -> Result<(Vec<String>, Vec<(String, String)>, Vec<String>)> {
    let mut found: Vec<(std::time::SystemTime, String)> = Vec::new();
    for one in listed.lines().map(str::trim).filter(|p| !p.is_empty()) {
        if one == work || found.iter().any(|(_, f)| f == one) {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(one) {
            found.push((meta.modified().unwrap_or(std::time::UNIX_EPOCH), one.to_owned()));
        }
    }
    found.sort();
    let mut kept: Vec<(String, String)> = Vec::new();
    let mut twins = Vec::new();
    let mut waiting = Vec::new();
    for (_, one) in found {
        if kept.len() == MAX_PENDING {
            waiting.push(one);
            continue;
        }
        let digest = ponteproj::sharedfolder::file_digest(std::path::Path::new(&one))?;
        if let Some((first, _)) = kept.iter().find(|(_, d)| *d == digest) {
            twins.push((one, first.clone()));
            continue;
        }
        kept.push((one, digest));
    }
    Ok((kept.into_iter().map(|(p, _)| p).collect(), twins, waiting))
}

/// What the person is told about pending copies left for the next
/// exchanges.
fn waiting_notice(waiting: &[String]) -> Option<String> {
    match waiting.len() {
        0 => None,
        1 => Some("1 cópia pendente deste aparelho espera a próxima sincronização (cada sincronização confere no máximo 6); nada dela foi perdido.".to_owned()),
        n => Some(format!("{n} cópias pendentes deste aparelho esperam as próximas sincronizações (cada sincronização confere no máximo 6); nada delas foi perdido.")),
    }
}

/// The documents of the library, newest first, with their notes and what the
/// list shows of each: type, date, where it came from, open tasks, attachments.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listDocuments<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
            // Open tasks by document: how many, and the earliest due date.
            let mut open = std::collections::HashMap::<i64, (usize, String)>::new();
            for task in store.list_tasks()?.iter().filter(|t| !t.is_done()) {
                // Tasks without a document count for no document.
                let Some(document) = task.document_id else {
                    continue;
                };
                let entry = open.entry(document).or_default();
                entry.0 += 1;
                if !task.due_date.is_empty() && (entry.1.is_empty() || task.due_date < entry.1) {
                    entry.1 = task.due_date.clone();
                }
            }
            let mut documents = Vec::new();
            for summary in store.list_documents("")? {
                let (open_tasks, due) = open.get(&summary.id).cloned().unwrap_or_default();
                let mut entry = json!({
                    "id": summary.id,
                    "number": summary.document_number,
                    "title": summary.title,
                    "manual": summary.manual,
                    // A record whose creation still waits to be sent: its
                    // number may change on the next exchange.
                    "provisional": summary.provisional,
                    "email": summary.email,
                    "email_from": summary.email_from,
                    "categories": summary.categories,
                    "open_tasks": open_tasks,
                    "task_due_label": ponteproj::labels::date(&due),
                    "task_overdue": !due.is_empty() && due.as_str() < today.as_str(),
                    "task_due_today": due == today,
                    "attachments": store
                        .list_attachments(ponteproj::attachments::Owner::Document(summary.id))
                        .map(|list| list.len())
                        .unwrap_or(0),
                });
                // A note that cannot be read is told, never shown as empty
                // (editing an "empty" note would replace the real one).
                match store.load_document(summary.id) {
                    Ok(detail) => {
                        let meta = &detail.capture.metadata;
                        let field = |key: &str| meta.get(key).cloned().unwrap_or_default();
                        entry["note"] = json!(detail.note);
                        entry["type"] = json!(field("document_type"));
                        entry["date"] = json!(ponteproj::labels::date(&field("document_date")));
                        entry["origin"] = json!(field("origin"));
                    }
                    Err(error) => entry["note_error"] = json!(format!("{error:#}")),
                }
                documents.push(entry);
            }
            Ok(json!({ "documents": documents }))
        })
    })
}

/// Saves a note; it waits in this phone's queue for the next exchange.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_saveNote<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    document_id: jlong,
    note: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let note = text(env, &note)?;
        with_store(|store| {
            store.save_note(document_id, &note)?;
            Ok(json!({}))
        })
    })
}

/// The notices of records this phone shows under another number since the
/// last exchange, until the person dismisses them. They live in the phone's
/// own library, so they do not depend on the status line of an exchange:
/// `{"notices":[{"id","title","from","to","sentence","document_id","at"}]}`,
/// newest first (`document_id` is 0 when the record cannot be opened).
fn renumber_notices_json(store: &Store) -> Result<Value> {
    let notices = store
        .pending_renumber_notices()?
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "title": n.title,
                "from": n.from,
                "to": n.to,
                "sentence": n.sentence(),
                "document_id": store.renumber_notice_document(n).unwrap_or(0),
                "at": n.at,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "notices": notices }))
}

/// The pending renumber notices (see [`renumber_notices_json`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_renumberNotices<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| with_store(|store| renumber_notices_json(store)))
}

/// Marks one notice as read (`id` from the list; an unknown or already read
/// one is not an error). Answers what is still pending:
/// `{"remaining":n}`.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_dismissRenumberNotice<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    id: jlong,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            store.dismiss_renumber_notice(id)?;
            Ok(json!({ "remaining": store.pending_renumber_notices()?.len() }))
        })
    })
}

// ---------------------------------------------------------------------------
// Version 2: the document file, tasks, a record from a photo, e-mail.
// Everything is built on the same Store functions the desktop uses.

/// JSON text of a JNI string parameter.
fn json_arg(env: &mut JNIEnv, value: &JString) -> Result<Value> {
    serde_json::from_str(&text(env, value)?).context("Dados inválidos vindos do aplicativo.")
}

fn task_json(task: &ponteproj::tasks::Task, numbers: &TaskNames) -> Value {
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    task_json_on(task, numbers, &today)
}

/// `task_json` as of the given day (`AAAA-MM-DD`), so tests can fix the date.
fn task_json_on(
    task: &ponteproj::tasks::Task,
    numbers: &TaskNames,
    today: &str,
) -> Value {
    let open = !task.is_done();
    json!({
        "id": task.id,
        "uid": task.uid,
        // 0 and an empty number: a task without a document.
        "document_id": task.document_id.unwrap_or(0),
        "document_number": task
            .document_id
            .and_then(|id| numbers.documents.get(&id).cloned())
            .unwrap_or_default(),
        "title": task.title,
        "action": task.action,
        "action_label": ponteproj::tasks::action_label(&task.action),
        "status": task.status,
        "status_label": ponteproj::tasks::status_label(&task.status),
        "priority": task.priority,
        "priority_label": ponteproj::tasks::priority_label(&task.priority),
        "due_date": task.due_date,
        "scheduled_start": task.scheduled_start,
        "scheduled_end": task.scheduled_end,
        "estimate_minutes": task.estimate_minutes,
        "reminder_offsets_minutes": task.reminder_offsets_minutes,
        "parent_task_id": task.parent_task_id,
        "dependencies": task.dependencies,
        "due_label": ponteproj::labels::date(&task.due_date),
        "overdue": open && !task.due_date.is_empty() && task.due_date.as_str() < today,
        "due_today": open && task.due_date == today,
        "done": !open,
        // What it would be if it were open (for a task reopened on the phone before the next reload).
        "late_if_open": !task.due_date.is_empty() && task.due_date.as_str() < today,
        "today_if_open": task.due_date == today,
        // The group of the task lists, and the short wording of the due chip.
        "bucket": task.bucket(today).key(),
        "due_chip": if task.due_date.is_empty() { String::new() } else { ponteproj::labels::due_relative(&task.due_date, today) },
        "notes": task.notes,
        // Uids of the categories; names and colours come from `listCategories`.
        "categories": task.categories,
        // The project of the task (empty: none) and its name.
        "project_uid": task.project_uid,
        "project_name": numbers.projects.get(&task.project_uid).cloned().unwrap_or_default(),
    })
}

/// What the task JSON needs besides the task itself: the numbers of the
/// documents and the names of the projects, by uid.
#[derive(Default)]
struct TaskNames {
    documents: std::collections::HashMap<i64, String>,
    projects: std::collections::HashMap<String, String>,
}

fn project_names(store: &Store) -> Result<std::collections::HashMap<String, String>> {
    Ok(store
        .list_projects()?
        .into_iter()
        .map(|p| (p.uid, p.name))
        .collect())
}

fn document_numbers(store: &Store) -> Result<TaskNames> {
    Ok(TaskNames {
        documents: store
            .list_documents("")?
            .into_iter()
            .map(|d| (d.id, d.document_number))
            .collect(),
        projects: project_names(store)?,
    })
}

/// Everything the document screen shows.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_getDocument<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    document_id: jlong,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let detail = store.load_document(document_id)?;
            let meta = &detail.capture.metadata;
            let field = |key: &str| meta.get(key).cloned().unwrap_or_default();
            let files = store
                .list_files(detail.summary.latest_capture_id)?
                .into_iter()
                .map(|f| json!({"id": f.id, "label": f.label, "size": f.size,
                                "capture_id": detail.summary.latest_capture_id}))
                .collect::<Vec<_>>();
            let attachments = store
                .list_attachments(ponteproj::attachments::Owner::Document(document_id))?
                .into_iter()
                .map(|a| json!({"id": a.id, "name": a.name, "kind": a.kind, "size": a.size,
                                "description": a.description}))
                .collect::<Vec<_>>();
            let numbers = document_numbers(store)?;
            let tasks = store
                .list_tasks()?
                .iter()
                .filter(|t| t.document_id == Some(document_id))
                .map(|t| task_json(t, &numbers))
                .collect::<Vec<_>>();
            let analysis = detail.analysis.as_ref().map(|a| {
                json!({
                    "summary": a.summary,
                    "priority": a.priority,
                    "reason": a.reason,
                    "deadline": a.deadline_candidate,
                    "suggested": a.suggested_actions.iter()
                        .map(|s| json!({"action": s.action, "reason": s.reason}))
                        .collect::<Vec<_>>(),
                })
            });
            Ok(json!({
                "id": document_id,
                "number": detail.summary.document_number,
                "title": detail.summary.title,
                "manual": detail.summary.manual,
                "email": detail.summary.email,
                "email_from": detail.summary.email_from,
                "type": field("document_type"),
                "date": field("document_date"),
                "origin": field("origin"),
                "text": detail.capture.text,
                "note": detail.note,
                "analysis": analysis,
                "files": files,
                "attachments": attachments,
                "tasks": tasks,
                "categories": store.document_categories(document_id)?,
                "categories_version": work_context_version(store, document_id),
                "proof": attach_target(store, document_id)?,
            }))
        })
    })
}

/// Writes a document PDF (`kind` "pdf", with its capture) or an attachment
/// (`kind` "attachment") into `dest_dir` (the app cache) and answers its path,
/// so the app can hand it to the phone's viewer.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_exportFile<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    kind: JString<'local>,
    id: jlong,
    capture_id: jlong,
    dest_dir: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let kind = text(env, &kind)?;
        let dir = PathBuf::from(text(env, &dest_dir)?);
        with_store(|store| {
            let (name, bytes) = match kind.as_str() {
                "pdf" => (format!("documento-{id}.pdf"), store.pdf_bytes(id, capture_id)?),
                "attachment" => {
                    let (meta, bytes) = store.attachment_bytes(id)?;
                    (meta.name, bytes)
                }
                other => anyhow::bail!("Tipo de arquivo desconhecido: {other}"),
            };
            // Only a plain file name lands in the app's folder.
            let safe: String = name
                .chars()
                .map(|c| if c.is_alphanumeric() || " .-_()".contains(c) { c } else { '_' })
                .collect();
            let safe = if safe.trim().is_empty() { format!("arquivo-{id}") } else { safe };
            // Long names would pass the file system's limit: cut, keeping
            // the extension the viewer needs.
            let safe = if safe.chars().count() > 120 {
                let (stem, ext) = match safe.rfind('.') {
                    Some(dot) if safe.len() - dot <= 10 => (&safe[..dot], &safe[dot..]),
                    _ => (safe.as_str(), ""),
                };
                format!("{}{ext}", stem.chars().take(110).collect::<String>())
            } else {
                safe
            };
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(&safe);
            std::fs::write(&path, &bytes)?;
            let mime = match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("pdf") => "application/pdf",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("png") => "image/png",
                Some("txt") => "text/plain",
                Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                Some("eml") => "message/rfc822",
                _ => "application/octet-stream",
            };
            Ok(json!({"path": path.display().to_string(), "name": safe, "mime": mime}))
        })
    })
}

/// How many concluded tasks the phone lists.
const DONE_LIMIT: usize = 30;

/// The tasks in display order: by group (late, today, upcoming, no deadline,
/// concluded); upcoming by date; the concluded newest first, at most 30.
fn ordered_tasks(
    mut tasks: Vec<ponteproj::tasks::Task>,
    numbers: &TaskNames,
    today: &str,
) -> Vec<Value> {
    tasks.sort_by(|a, b| {
        let (x, y) = (a.bucket(today), b.bucket(today));
        x.slot().cmp(&y.slot()).then_with(|| match x {
            ponteproj::tasks::Bucket::Upcoming => a.due_date.cmp(&b.due_date),
            ponteproj::tasks::Bucket::Done => b.id.cmp(&a.id),
            _ => std::cmp::Ordering::Equal,
        })
    });
    let mut done = 0;
    tasks
        .iter()
        .filter(|t| {
            if t.is_done() {
                done += 1;
                done <= DONE_LIMIT
            } else {
                true
            }
        })
        .map(|t| task_json_on(t, numbers, today))
        .collect()
}

/// Open tasks first (overdue, then due today, then by date), concluded last.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listTasks<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let numbers = document_numbers(store)?;
            let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
            let tasks = ordered_tasks(store.list_tasks()?, &numbers, &today);
            Ok(json!({ "tasks": tasks }))
        })
    })
}

/// Complete task graph, including all concluded tasks, for parent selection and subtasks.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listTaskRelations<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let numbers = document_numbers(store)?;
            let tasks = store.list_tasks()?.iter().map(|t| task_json(t, &numbers)).collect::<Vec<_>>();
            Ok(json!({ "tasks": tasks }))
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_completeTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_id: jlong,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            store.complete_task(task_id)?;
            Ok(json!({}))
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_reopenTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_id: jlong,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            store.reopen_task(task_id)?;
            Ok(json!({}))
        })
    })
}

/// A new task: `{"document_id":1,"title":"...","action":"responder","due_date":"AAAA-MM-DD"}`.
/// A missing or zero `document_id` makes a task without document.
/// `project_uid`: a uid puts the task in that project; absent, a subtask
/// takes the project of its parent; an explicit `null` or empty text creates
/// it with NO project (even under a parent that has one).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_createTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_json_text: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &task_json_text)?;
        with_store(|store| create_task(store, &input))
    })
}

fn create_task(store: &mut Store, input: &Value) -> Result<Value> {
    // `project_uid`: absent, a subtask takes the project of its parent; an
    // explicit null or empty text, the task is created with NO project.
    let no_project = match input.get("project_uid") {
        None => false,
        Some(value) if value.is_null() => true,
        Some(value) => value.as_str().is_some_and(|text| text.trim().is_empty()),
    };
    // Missing or 0: no document. A negative number is a mistake, not "none".
    let document_id = match input["document_id"].as_i64() {
        Some(id) if id < 0 => anyhow::bail!("Documento inválido."),
        Some(id) if id > 0 => Some(id),
        _ => None,
    };
    let mut task = ponteproj::tasks::Task::blank(document_id);
    task.title = input["title"].as_str().unwrap_or("").trim().to_owned();
    if let Some(action) = input["action"].as_str().filter(|a| !a.is_empty()) {
        task.action = action.to_owned();
    }
    task.due_date = valid_due(input["due_date"].as_str().unwrap_or(""))?;
    apply_planning(&mut task, input)?;
    // The responsible person, when the phone knows the person's name.
    task.assignee = input["assignee"].as_str().unwrap_or("").trim().chars().take(80).collect();
    if let Some(uids) = uid_list(&input["categories"]) {
        task.categories = uids;
    }
    // The project of the task; a subtask without one takes its parent's.
    apply_project(&mut task, input)?;
    let id = if no_project && task.parent_task_id.is_some() {
        store.save_task_without_project(&task)?
    } else {
        store.save_task(&task)?
    };
    Ok(json!({ "id": id }))
}

/// The `project_uid` of `input`, when it has one: a uid puts the task in that
/// project (the core drops one that does not exist), an empty text or null
/// takes it out. Absent: the task keeps what it has.
fn apply_project(task: &mut ponteproj::tasks::Task, input: &Value) -> Result<()> {
    if let Some(value) = input.get("project_uid") {
        task.project_uid = if value.is_null() {
            String::new()
        } else {
            value.as_str().context("Projeto inválido.")?.trim().to_owned()
        };
    }
    Ok(())
}

fn apply_planning(task: &mut ponteproj::tasks::Task, input: &Value) -> Result<()> {
    for (key, target) in [("scheduled_start", &mut task.scheduled_start), ("scheduled_end", &mut task.scheduled_end)] {
        if let Some(value) = input.get(key) {
            *target = value.as_str().context("Horário inválido.")?.to_owned();
        }
    }
    if let Some(value) = input.get("estimate_minutes") {
        task.estimate_minutes = u32::try_from(value.as_u64().context("Estimativa inválida.")?)?;
    }
    if let Some(value) = input.get("reminder_offsets_minutes") {
        task.reminder_offsets_minutes = value.as_array().context("Avisos inválidos.")?.iter().map(|v| Ok(u32::try_from(v.as_u64().context("Aviso inválido.")?)?)).collect::<Result<Vec<_>>>()?;
    }
    if let Some(value) = input.get("dependencies") {
        task.dependencies = value.as_array().context("Dependências inválidas.")?.iter()
            .map(|id| id.as_i64().context("Dependência inválida."))
            .collect::<Result<Vec<_>>>()?;
    }
    if let Some(value) = input.get("parent_task_id") {
        task.parent_task_id = if value.is_null() { None } else { Some(value.as_i64().context("Tarefa pai inválida.")?) };
    }
    Ok(())
}

/// A due date as the core keeps it: empty, or `AAAA-MM-DD`.
fn valid_due(value: &str) -> Result<String> {
    let value = value.trim();
    anyhow::ensure!(
        value.is_empty() || chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok(),
        "O prazo informado não é uma data válida."
    );
    Ok(value.to_owned())
}

/// What the phone says when the task changed under the form (the core's text
/// is written for the computer).
const TASK_CHANGED: &str =
    "Esta tarefa foi alterada depois que você a abriu. Abra-a de novo para ver a versão atual.";

fn task_conflict(error: anyhow::Error) -> anyhow::Error {
    if format!("{error:#}").starts_with("Conflito de edição simultânea") {
        anyhow::anyhow!(TASK_CHANGED)
    } else {
        error
    }
}

/// The active task `id` (neither it nor its document in the trash).
fn find_task(store: &Store, id: i64) -> Result<ponteproj::tasks::Task> {
    store
        .list_tasks()?
        .into_iter()
        .find(|t| t.id == id)
        .context("Tarefa não encontrada. Ela pode ter sido excluída em outro aparelho.")
}

/// Everything the task screen shows or edits, plus the `version` a save must
/// present back.
fn get_task(store: &Store, id: i64) -> Result<Value> {
    let task = find_task(store, id)?;
    let documents = store.list_documents("")?;
    let title = task
        .document_id
        .and_then(|d| documents.iter().find(|x| x.id == d))
        .map(|d| d.title.clone())
        .unwrap_or_default();
    let numbers = TaskNames {
        documents: documents.iter().map(|d| (d.id, d.document_number.clone())).collect(),
        projects: project_names(store)?,
    };
    let mut value = task_json(&task, &numbers);
    value["document_title"] = json!(title);
    value["assignee"] = json!(task.assignee);
    value["reminder_date"] = json!(task.reminder_date);
    value["checklist"] = json!(task
        .checklist
        .iter()
        .map(|(done, text)| json!({"done": done, "text": text}))
        .collect::<Vec<_>>());
    value["version"] = json!(store.item_version("task", id)?.version);
    Ok(value)
}

/// Saves the editable fields present in `input` (`id`, `version`, `title`,
/// `action`, `priority`, `status`, `due_date`, `assignee`, `notes`) over the
/// stored task; everything else (reminder, checklist, outcome, document)
/// stays as stored. A stale `version` saves nothing.
fn save_task(store: &mut Store, input: &Value) -> Result<Value> {
    let id = input["id"].as_i64().context("Tarefa ausente.")?;
    let version = input["version"].as_i64().context("Versão da tarefa ausente.")?;
    let mut task = find_task(store, id)?;
    apply_planning(&mut task, input)?;
    let text = |key: &str| input[key].as_str().map(str::to_owned);
    if let Some(title) = text("title") {
        task.title = title.trim().to_owned();
    }
    if let Some(action) = text("action").filter(|a| !a.is_empty()) {
        task.action = action;
    }
    if let Some(priority) = text("priority").filter(|p| !p.is_empty()) {
        task.priority = priority;
    }
    if let Some(due) = text("due_date") {
        task.due_date = valid_due(&due)?;
    }
    if let Some(assignee) = text("assignee") {
        task.assignee = assignee.trim().to_owned();
    }
    if let Some(notes) = text("notes") {
        task.notes = notes;
    }
    if let Some(status) = text("status").filter(|s| !s.is_empty()) {
        apply_status(&mut task, &status)?;
    }
    if let Some(uids) = uid_list(&input["categories"]) {
        task.categories = uids;
    }
    apply_project(&mut task, input)?;
    // A `project_uid` that is present and empty (or null) is the person
    // taking the task out of its project: that travels with the change, even
    // when this copy of the task has no project to take out (another copy may
    // have given it one meanwhile). Absent, the link is left alone.
    let clears_project = input.get("project_uid").is_some() && task.project_uid.is_empty();
    anyhow::ensure!(!task.title.is_empty(), "Informe o título da tarefa.");
    anyhow::ensure!(
        ["baixa", "normal", "alta", "urgente"].contains(&task.priority.as_str()),
        "A prioridade da tarefa é inválida."
    );
    if clears_project {
        store.save_task_clearing_project(&task, Some(version))
    } else {
        store.save_task_checked(&task, Some(version))
    }
    .map_err(task_conflict)?;
    Ok(json!({ "id": id, "version": store.item_version("task", id)?.version }))
}

/// The situations the desktop knows, in the order its form lists them.
const STATUSES: [&str; 4] = ["pendente", "em analise", "aguardando", "concluida"];

/// Sets the situation as the desktop form does: only one of its four values,
/// and the conclusion date is left to the core (it stamps it for "concluida"
/// and clears it for any other situation when the task is saved).
fn apply_status(task: &mut ponteproj::tasks::Task, status: &str) -> Result<()> {
    anyhow::ensure!(STATUSES.contains(&status), "A situação da tarefa é inválida.");
    if task.status != status {
        task.status = status.to_owned();
        task.completed_at = None;
    }
    Ok(())
}

/// The strings of a JSON array (`None` when the value is not an array).
fn uid_list(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()
        .map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
}

fn category_json(c: &ponteproj::categories::Category) -> Value {
    json!({"uid": c.uid, "name": c.name, "color": c.color, "archived": c.archived})
}

/// Every category, archived ones too (an item may still carry one).
fn list_categories(store: &Store) -> Result<Value> {
    let list = store.list_categories()?.iter().map(category_json).collect::<Vec<_>>();
    Ok(json!({ "categories": list }))
}

/// Creates a category (the colour is a palette index, 0 to 9); the core
/// refuses an empty or long name, a repeated one and a full list.
fn create_category(store: &mut Store, name: &str, color: i64) -> Result<Value> {
    anyhow::ensure!(
        (0..i64::from(ponteproj::categories::PALETTE_SIZE)).contains(&color),
        "A cor da categoria é inválida."
    );
    let saved = store.save_category(&ponteproj::categories::Category::new(name, color as u8))?;
    Ok(json!({ "category": category_json(&saved) }))
}

/// The version of a document's work context (where its categories live).
fn work_context_version(store: &Store, document_id: i64) -> i64 {
    store.item_version("work_context", document_id).map(|m| m.version).unwrap_or(0)
}

/// What the phone says when the document changed under its ficha.
const DOCUMENT_CHANGED: &str =
    "Este documento foi alterado depois que você abriu a ficha. Abra-a de novo para ver a versão atual.";

/// Replaces the categories of a document, refusing when it changed since the
/// ficha was read (`version` is `categories_version` of `getDocument`).
fn set_document_categories(
    store: &mut Store,
    document_id: i64,
    uids: &[String],
    version: i64,
) -> Result<Value> {
    store.load_document(document_id).context("Documento não encontrado.")?;
    anyhow::ensure!(work_context_version(store, document_id) == version, DOCUMENT_CHANGED);
    store.set_document_categories(document_id, uids).map_err(|error| {
        if format!("{error:#}").starts_with("Conflito de edição simultânea") {
            anyhow::anyhow!(DOCUMENT_CHANGED)
        } else {
            error
        }
    })?;
    Ok(json!({
        "categories": store.document_categories(document_id)?,
        "categories_version": work_context_version(store, document_id),
    }))
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listCategories<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| with_store(|store| list_categories(store)))
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_createCategory<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
    color: jlong,
) -> jstring {
    answer(&mut env, |env| {
        let name = text(env, &name)?;
        with_store(|store| create_category(store, &name, color))
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_setDocumentCategories<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    document_id: jlong,
    uids_json: JString<'local>,
    version: jlong,
) -> jstring {
    answer(&mut env, |env| {
        let uids = uid_list(&json_arg(env, &uids_json)?)
            .context("Dados inválidos vindos do aplicativo.")?;
        with_store(|store| set_document_categories(store, document_id, &uids, version))
    })
}

// ---------------------------------------------------------------------------
// Projects: an optional grouping of tasks. Read-only listing and detail (with
// the numbers of the rules of `ponteproj::projects`), save and delete.

/// Moment of a timeline row as the core keeps planned moments: `AAAA-MM-DDTHH:MM`.
fn moment(value: chrono::NaiveDateTime) -> String {
    value.format("%Y-%m-%dT%H:%M").to_string()
}

fn project_json(p: &ponteproj::projects::Project, version: i64) -> Value {
    json!({
        "uid": p.uid,
        "name": p.name,
        "objective": p.objective,
        "assignee": p.assignee,
        "status": p.status.key(),
        "status_label": p.status.label(),
        "start_date": p.start_date,
        "target_date": p.target_date,
        "start_label": ponteproj::labels::date(&p.start_date),
        "target_label": ponteproj::labels::date(&p.target_date),
        "created_at": p.created_at,
        "updated_at": p.updated_at,
        // What a save must present back.
        "version": version,
    })
}

fn progress_json(p: &ponteproj::projects::Progress) -> Value {
    json!({
        "total": p.total,
        "done": p.done,
        "late": p.late,
        "next_due": p.next_due,
        "next_due_label": p.next_due.as_deref().map(ponteproj::labels::date).unwrap_or_default(),
    })
}

fn risk_json(r: &ponteproj::projects::ProjectRisk) -> Value {
    json!({
        "deliveries_at_risk": r.deliveries_at_risk,
        // The latest planned or projected end when it falls after the target
        // date of the project (`AAAA-MM-DDTHH:MM`), otherwise null.
        "target_at_risk": r.target_at_risk.map(moment),
    })
}

/// Every project, by name, each with its progress and risk as of `today`.
fn list_projects(store: &Store, today: chrono::NaiveDate) -> Result<Value> {
    let list = store
        .project_overview(today)?
        .iter()
        .map(|item| {
            let version = store.project_version(&item.project.uid).unwrap_or(0);
            let mut row = project_json(&item.project, version);
            row["progress"] = progress_json(&item.progress);
            row["risk"] = risk_json(&item.risk);
            row
        })
        .collect::<Vec<_>>();
    Ok(json!({ "projects": list }))
}

fn timeline_json(rows: &[ponteproj::projects::TimelineRow]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            json!({
                "id": row.id,
                "uid": row.uid,
                "title": row.title,
                "done": row.done,
                "start": row.period.map(|p| moment(p.0)),
                "end": row.period.map(|p| moment(p.1)),
                // The period came from the projection, not from a fixed plan.
                "projected": row.projected,
                "due": row.due.map(|d| d.format("%Y-%m-%d").to_string()),
                // The period ends after the due date.
                "late": row.late,
            })
        })
        .collect()
}

/// What the phone says when the project is not there (the core's text is
/// written for the computer).
const PROJECT_GONE: &str =
    "Projeto não encontrado. Ele pode ter sido excluído em outro aparelho.";

/// What the phone says when the project changed under the form.
const PROJECT_CHANGED: &str =
    "Este projeto foi alterado depois que você o abriu. Abra-o de novo para ver a versão atual.";

/// One project, its tasks (the task JSON of every other list), and its
/// timeline, as of `today`.
fn get_project(store: &Store, uid: &str, today: chrono::NaiveDate) -> Result<Value> {
    let overview = store
        .project_overview(today)?
        .into_iter()
        .find(|item| item.project.uid == uid)
        .context(PROJECT_GONE)?;
    let numbers = document_numbers(store)?;
    let all = store.list_tasks()?;
    let day = today.format("%Y-%m-%d").to_string();
    let mut mine: Vec<_> = all.iter().filter(|t| t.project_uid == uid).cloned().collect();
    // Open tasks first (by due date, the ones without a date after them),
    // concluded last.
    mine.sort_by(|a, b| {
        a.is_done()
            .cmp(&b.is_done())
            .then_with(|| a.due_date.is_empty().cmp(&b.due_date.is_empty()))
            .then_with(|| a.due_date.cmp(&b.due_date))
            .then_with(|| a.id.cmp(&b.id))
    });
    let tasks = mine.iter().map(|t| task_json_on(t, &numbers, &day)).collect::<Vec<_>>();
    let rows = ponteproj::projects::timeline(uid, &all);
    let span = ponteproj::projects::timeline_span(&rows)
        .map(|(start, end)| json!({ "start": moment(start), "end": moment(end) }));
    let version = store.project_version(uid).unwrap_or(0);
    let mut project = project_json(&overview.project, version);
    project["progress"] = progress_json(&overview.progress);
    project["risk"] = risk_json(&overview.risk);
    Ok(json!({
        "project": project,
        "tasks": tasks,
        "timeline": timeline_json(&rows),
        "span": span,
    }))
}

fn project_error(error: anyhow::Error) -> anyhow::Error {
    let text = format!("{error:#}");
    if text.starts_with("Conflito de edição simultânea") {
        anyhow::anyhow!(PROJECT_CHANGED)
    } else if text.starts_with("Projeto não encontrado") {
        anyhow::anyhow!(PROJECT_GONE)
    } else {
        error
    }
}

/// Creates (no `uid`) or changes a project. The fields present in `input`
/// among `name`, `objective`, `assignee`, `status` (`ativo`, `pausado`,
/// `concluido`, `arquivado`), `start_date` and `target_date` (`AAAA-MM-DD` or
/// empty) are written over the stored project; for an existing one the
/// `version` read by `getProject` or `listProjects` is required, and a
/// project changed meanwhile saves nothing. Answers `{uid, version, project}`.
fn save_project(store: &mut Store, input: &Value) -> Result<Value> {
    // No `uid` (absent, null or empty) creates a project; one that is present
    // but is not a text is a mistake, never "a new project".
    let uid = match input.get("uid") {
        None => String::new(),
        Some(value) if value.is_null() => String::new(),
        Some(value) => value
            .as_str()
            .context("Identificação do projeto inválida.")?
            .trim()
            .to_owned(),
    };
    let mut project = if uid.is_empty() {
        ponteproj::projects::Project::default()
    } else {
        store.get_project(&uid)?.context(PROJECT_GONE)?
    };
    let text = |key: &str| input[key].as_str().map(str::to_owned);
    if let Some(name) = text("name") {
        project.name = name;
    }
    if let Some(objective) = text("objective") {
        project.objective = objective;
    }
    if let Some(assignee) = text("assignee") {
        project.assignee = assignee.trim().to_owned();
    }
    if let Some(status) = text("status") {
        project.status = ponteproj::projects::ProjectStatus::parse(&status)
            .context("A situação do projeto é inválida.")?;
    }
    for (key, target) in [("start_date", &mut project.start_date), ("target_date", &mut project.target_date)] {
        if let Some(value) = input.get(key) {
            *target = valid_due(value.as_str().context("Data inválida.")?)?;
        }
    }
    let saved = if uid.is_empty() {
        store.save_project(&project).map_err(project_error)?
    } else {
        let version = input["version"].as_i64().context("Versão do projeto ausente.")?;
        store.save_project_checked(&project, Some(version)).map_err(project_error)?
    };
    let version = store.project_version(&saved.uid)?;
    let stored = store.get_project(&saved.uid)?.unwrap_or(saved);
    Ok(json!({
        "uid": stored.uid,
        "version": version,
        "project": project_json(&stored, version),
    }))
}

/// Deletes a project; its tasks stay, with no project (`released` tells how
/// many).
fn delete_project(store: &mut Store, uid: &str) -> Result<Value> {
    store.get_project(uid)?.context(PROJECT_GONE)?;
    let released = store.project_tasks(uid)?.len();
    store.delete_project(uid).map_err(project_error)?;
    Ok(json!({ "released": released }))
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listProjects<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| list_projects(store, chrono::Local::now().date_naive()))
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_getProject<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    uid: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let uid = text(env, &uid)?;
        with_store(|store| get_project(store, &uid, chrono::Local::now().date_naive()))
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_saveProject<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    project_json_text: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &project_json_text)?;
        with_store(|store| save_project(store, &input))
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_deleteProject<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    uid: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let uid = text(env, &uid)?;
        with_store(|store| delete_project(store, &uid))
    })
}

/// Makes `config` the one the planning of the core uses, and answers its text
/// (what the app keeps as its own copy).
fn use_holidays(config: ponteproj::holidays::HolidayConfig) -> Value {
    let text = config.to_text();
    ponteproj::holidays::set_config(config);
    json!({ "text": text })
}

/// The holidays belong to the library, so they are the same on every computer
/// and phone that syncs it. The library wins. `local` is the text the app
/// kept until now: it is what the phone uses while the library has none.
/// `seed` is the same text, offered only while this phone has not handed it
/// over (the app passes an empty one afterwards): when the library has none
/// yet, it goes in as a seed, which every copy applies only where there is no
/// value, never over what another device shared. Answers the text now in
/// force and `library`, whether the library has a value (the hand-over is
/// then over).
fn adopt_holidays(store: &mut Store, local: &str, seed: &str) -> Value {
    let offered = ponteproj::holidays::HolidayConfig::parse(seed);
    // A failure to write only means the library keeps none for now.
    let (config, library) = ponteproj::holidays::adopt_library(store, &offered, true)
        .unwrap_or((offered, false));
    let config = if library {
        config
    } else {
        ponteproj::holidays::HolidayConfig::parse(local)
    };
    let mut answer = use_holidays(config);
    answer["library"] = json!(library);
    answer
}

/// The person's choice, from Ajustes: into the library (and so to the other
/// devices) and into the planning.
fn save_holidays(store: &mut Store, text: &str) -> Result<Value> {
    let config = ponteproj::holidays::HolidayConfig::parse(text);
    ponteproj::holidays::save_to_library(store, &config)?;
    Ok(use_holidays(config))
}

/// After an exchange: what the library says, when it says anything.
fn holidays_after_sync(store: &Store) -> Option<Value> {
    let config = ponteproj::holidays::library_config(store).ok().flatten()?;
    Some(use_holidays(config))
}

/// Takes the holidays of the library (or, with none yet, puts the app's own
/// text into it) and makes them the planning's. Answers `{text}`.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_adoptHolidays<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    local_text: JString<'local>,
    seed_text: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let local = text(env, &local_text)?;
        let seed = text(env, &seed_text)?;
        with_store(|store| Ok(adopt_holidays(store, &local, &seed)))
    })
}

/// Saves the holidays chosen in Ajustes in the library. Answers `{text}`.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_setHolidays<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    text_lines: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let chosen = text(env, &text_lines)?;
        with_store(|store| save_holidays(store, &chosen))
    })
}

/// Moves a task to `document_id`, or (0) takes it out of its document.
fn relink_task(store: &mut Store, task_id: i64, document_id: i64, version: i64) -> Result<Value> {
    anyhow::ensure!(document_id >= 0, "Documento inválido.");
    find_task(store, task_id)?;
    store
        .relink_task(task_id, (document_id > 0).then_some(document_id), Some(version))
        .map_err(task_conflict)?;
    Ok(json!({ "id": task_id, "version": store.item_version("task", task_id)?.version }))
}

/// What the phone says when a task cannot go to the Lixeira because others
/// hang from it (the core's text is written for the computer).
const TASK_HAS_DEPENDENTS: &str =
    "Há tarefas que dependem desta ou são subtarefas dela. Desfaça esses vínculos antes de excluir.";

/// Sends the task to the Lixeira, as the desktop's "Excluir" does: the same
/// `Store::trash_item`, refused by the same plan when other active tasks
/// depend on it or are its subtasks. `version` is the one the screen read; a
/// stale one deletes nothing. The change is queued for the shared folder like
/// any other.
fn delete_task(store: &mut Store, id: i64, version: i64) -> Result<Value> {
    use ponteproj::trash::{Kind, Target};
    find_task(store, id)?;
    let target = Target { kind: Kind::Task, id };
    let plan = store.trash_plan(target.clone())?;
    if plan.blocker.is_some() {
        anyhow::bail!(TASK_HAS_DEPENDENTS);
    }
    // `trash_item` takes no version: compare it right before, as the desktop does.
    anyhow::ensure!(store.item_version("task", id)?.version == version, TASK_CHANGED);
    store.trash_item(target)?;
    Ok(json!({ "id": id }))
}

/// Brings a task back from the Lixeira (the "Desfazer" right after deleting
/// it). Dependencies that no longer exist are dropped by the core.
fn restore_task(store: &mut Store, id: i64) -> Result<Value> {
    use ponteproj::trash::{ItemState, Kind, Target};
    match store.task_state(id)? {
        ItemState::Trashed { .. } => {}
        ItemState::Active => return Ok(json!({ "id": id, "version": store.item_version("task", id)?.version })),
        ItemState::Missing => anyhow::bail!("Esta tarefa não está mais no acervo."),
    }
    store
        .restore_item(Target { kind: Kind::Task, id })
        .map_err(|e| {
            let text = format!("{e:#}");
            if text.contains("Restaure primeiro") {
                anyhow::anyhow!("Não foi possível desfazer: uma tarefa ligada a esta também foi excluída. Restaure-a no computador, na Lixeira.")
            } else {
                e
            }
        })?;
    Ok(json!({ "id": id, "version": store.item_version("task", id)?.version }))
}

/// Moves a task to the Lixeira (see [`delete_task`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_deleteTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_id: jlong,
    version: jlong,
) -> jstring {
    answer(&mut env, |_| with_store(|store| delete_task(store, task_id, version)))
}

/// Takes a task back from the Lixeira (see [`restore_task`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_restoreTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_id: jlong,
) -> jstring {
    answer(&mut env, |_| with_store(|store| restore_task(store, task_id)))
}

/// One task as the task screen reads it.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_getTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_id: jlong,
) -> jstring {
    answer(&mut env, |_| with_store(|store| get_task(store, task_id)))
}

/// Saves the edited fields of a task (see [`save_task`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_saveTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_json_text: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &task_json_text)?;
        with_store(|store| save_task(store, &input))
    })
}

/// Links, moves or unlinks a task (`document_id` 0: no document).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_relinkTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_id: jlong,
    document_id: jlong,
    version: jlong,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| relink_task(store, task_id, document_id, version))
    })
}

/// What "Hoje" shows, as of the current day.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_today<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
            let mut lists = today_lists(store, &today)?;
            lists["resources"] = resource_counts(store, &today, &ponteproj::resources::now_moment())?;
            add_today_words(&mut lists);
            Ok(lists)
        })
    })
}

/// The deliveries at risk as of the current day (`ponteproj::risk`):
/// `{"items":[{"task_id","title","due_date","kind","end","cause_id","cause_title"}]}`.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_atRisk<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| at_risk(store, chrono::Local::now().date_naive()))
    })
}

/// `kind` is `late_planned`, `late_projected`, `missing_plan` or
/// `missing_estimate`; `end` is `AAAA-MM-DDTHH:MM` for the late kinds, "" for
/// the others. The task to open is `cause_id`.
fn at_risk(store: &Store, today: chrono::NaiveDate) -> Result<Value> {
    use ponteproj::risk::RiskKind;
    let items = ponteproj::risk::deliveries_at_risk(&store.list_tasks()?, today)
        .into_iter()
        .map(|risk| {
            let kind = match risk.kind {
                RiskKind::LatePlanned => "late_planned",
                RiskKind::LateProjected => "late_projected",
                RiskKind::MissingPlan => "missing_plan",
                RiskKind::MissingEstimate => "missing_estimate",
            };
            json!({
                "task_id": risk.task_id,
                "title": risk.title,
                "due_date": risk.due_date,
                "kind": kind,
                "end": risk.end.map(|end| end.format("%Y-%m-%dT%H:%M").to_string()).unwrap_or_default(),
                "cause_id": risk.cause_id,
                "cause_title": risk.cause_title,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "items": items }))
}

/// The counts of "Hoje" from its lists.
fn today_counts_of(lists: &Value) -> ponteproj::labels::TodayCounts {
    let n = |v: &Value| v.as_array().map_or(0, Vec::len);
    let r = |key: &str| lists["resources"][key].as_u64().unwrap_or(0) as usize;
    ponteproj::labels::TodayCounts {
        late: n(&lists["late"]),
        today: n(&lists["today"]),
        attendances: n(&lists["attendances"]),
        reservations: r("reservations_today"),
        overdue_returns: r("overdue"),
    }
}

/// Adds the words of "Hoje": `summary` (the line of the screen), `notice` (the
/// sentence of the daily notice, "" when nothing is due) and `badge` (late
/// plus due today tasks, the number on the Tarefas tab).
fn add_today_words(lists: &mut Value) {
    let counts = today_counts_of(lists);
    lists["summary"] = json!(counts.summary());
    lists["notice"] = json!(counts.notice().unwrap_or_default());
    lists["badge"] = json!(counts.late + counts.today);
}

/// How many tasks "Próximos dias" lists.
const UPCOMING_LIMIT: usize = 6;

/// The lists of "Hoje" as of `today` (`AAAA-MM-DD`). The rules are those of
/// the desktop "Hoje" area (`src/app/today.rs`, desktop-only, so re-expressed
/// here): `split_tasks` for the tasks (open = not concluded and no
/// `completed_at`; a due date before today is late, equal is today, after is
/// upcoming; late and upcoming sorted by date, upcoming cut to 6) and `due_attendances` for the atendimentos
/// (open per `SaiopAttendance::is_open`, with a due date that is today or
/// past, oldest first; the reference wording is that of the desktop calendar).
fn today_lists(store: &Store, today: &str) -> Result<Value> {
    let numbers = document_numbers(store)?;
    let mut late = Vec::new();
    let mut due_today = Vec::new();
    let mut upcoming = Vec::new();
    for task in store.list_tasks()? {
        if task.completed_at.is_some() || task.status == "concluida" || task.due_date.is_empty() {
            continue;
        }
        match task.due_date.as_str().cmp(today) {
            std::cmp::Ordering::Less => late.push(task),
            std::cmp::Ordering::Equal => due_today.push(task),
            std::cmp::Ordering::Greater => upcoming.push(task),
        }
    }
    late.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    upcoming.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    upcoming.truncate(UPCOMING_LIMIT);
    let rows = |list: &[ponteproj::tasks::Task]| {
        list.iter().map(|t| task_json_on(t, &numbers, today)).collect::<Vec<_>>()
    };
    let mut attendances: Vec<_> = store
        .list_saiop_attendances()?
        .into_iter()
        .filter(|a| a.is_open() && !a.due_date.is_empty() && a.due_date.as_str() <= today)
        .collect();
    attendances.sort_by(|a, b| a.due_date.cmp(&b.due_date));
    let attendances = attendances
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "reference": attendance_reference(a),
                "subject": a.subject,
                "status_label": ponteproj::saiop::status_label(&a.status),
                "due_date": a.due_date,
                "due_chip": ponteproj::labels::due_relative(&a.due_date, today),
                "late": a.due_date.as_str() < today,
                "due_today": a.due_date == today,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "late": rows(&late),
        "today": rows(&due_today),
        "upcoming": rows(&upcoming),
        "attendances": attendances,
    }))
}

/// The task actions and the kinds of manual record, for the app's choices.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_choices<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        let actions = ponteproj::tasks::ACTIONS
            .iter()
            .map(|a| json!({"key": a, "label": ponteproj::tasks::action_label(a)}))
            .collect::<Vec<_>>();
        let kinds = ponteproj::manual::KINDS
            .iter()
            .map(|(key, label)| json!({"key": key, "label": label}))
            .collect::<Vec<_>>();
        let priorities = ["baixa", "normal", "alta", "urgente"]
            .iter()
            .map(|p| json!({"key": p, "label": ponteproj::tasks::priority_label(p)}))
            .collect::<Vec<_>>();
        Ok(json!({"actions": actions, "manual_kinds": kinds, "priorities": priorities, "limits": limits_json()}))
    })
}

/// A manual record made from a photo of a paper:
/// `{"kind":"papel","title":"...","origin":"...","date":"AAAA-MM-DD","text":"..."}`
/// plus the JPEG at `image_path`, attached to it.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_createPhotoRecord<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    fields_json: JString<'local>,
    image_path: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let mut input = json_arg(env, &fields_json)?;
        let image = text(env, &image_path)?;
        with_store(|store| {
            create_photo_record(store, &mut input, &image)
        })
    })
}

/// What the Kotlin side needs to know about the core's limits, so the screens
/// and the streaming caps never carry a copy of the numbers that could drift.
fn limits_json() -> Value {
    use ponteproj::attachments as a;
    json!({
        "max_attachment_bytes": a::MAX_ATTACHMENT_BYTES,
        "max_per_owner": a::MAX_PER_OWNER,
        "max_name_chars": a::MAX_NAME_CHARS,
        "max_description_chars": a::MAX_DESCRIPTION_CHARS,
        "max_text_bytes": ponteproj::model::MAX_CAPTURE_TEXT_BYTES,
        "max_title_bytes": MAX_TITLE_BYTES,
    })
}

/// The core refuses titles over this many bytes (`ManualFields::validate`).
const MAX_TITLE_BYTES: usize = 1000;

/// Reads a working file to attach it, never more than the core's limit plus
/// one byte: a file past the limit is refused with the core's own message
/// before it is held in memory (`file_refused`: no record would take it). A
/// file that cannot be read is no answer about the file.
fn read_attachment_file(path: &str, name: &str) -> Result<Vec<u8>> {
    use ponteproj::attachments::{size_message, FileRefused, MAX_ATTACHMENT_BYTES};
    use std::io::Read;
    let too_big = |size: u64| anyhow::Error::new(FileRefused(size_message(name, size)));
    let file = std::fs::File::open(path).context("O arquivo não pôde ser lido.")?;
    if let Ok(meta) = file.metadata() {
        if meta.len() > MAX_ATTACHMENT_BYTES as u64 {
            return Err(too_big(meta.len()));
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_ATTACHMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .context("O arquivo não pôde ser lido.")?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(too_big(bytes.len() as u64));
    }
    Ok(bytes)
}

/// Attaches the file at `path` to a document with the very function the
/// desktop uses (`Store::add_attachment`): same limits, type checks (by name
/// and by content), duplicate and per-document checks, same texts. (The plain
/// call, for the tests; the app's entry point is [`attach_file_with`].)
#[cfg(test)]
fn attach_file(store: &mut Store, document_id: i64, path: &str, name: &str, description: &str) -> Result<Value> {
    attach_file_with(store, document_id, path, name, description, "")
}

/// The optional string field `key` of the app's JSON (absent, empty or not a
/// string: none).
fn optional_text(input: &Value, key: &str) -> Option<String> {
    input[key].as_str().map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

/// [`attach_file`] with the app's `options` (`{}`, an empty string or an
/// unparsable one: nothing to say, exactly the plain call):
/// - `operation_id`: the idempotency key. An id that was committed adds nothing
///   and answers what the first call answered (the file is not even read); a new
///   one is written in the same transaction as the attachment and its queue entry.
/// - `expected_number`: the number the person saw when choosing the document;
///   if the document is gone, in the trash or has another number nothing is
///   written and the error carries `code: target_changed`.
/// - `document_ident` and `library_space` (F3.1b2, from [`attach_target`]): the
///   file goes to the record holding that identity in that space; otherwise
///   nothing is attached, the file is kept in a card and the error carries
///   `code: target_gone` with its `card_id` (also on a repeat).
fn attach_file_with(
    store: &mut Store,
    document_id: i64,
    path: &str,
    name: &str,
    description: &str,
    options: &str,
) -> Result<Value> {
    let options: Value = serde_json::from_str(options).unwrap_or(Value::Null);
    let operation = optional_text(&options, "operation_id");
    let expected = optional_text(&options, "expected_number");
    if let Some(id) = &operation {
        if let Some(done) = store.operation_record(id)? {
            anyhow::ensure!(
                done.kind == "attach",
                "Este identificador de operação já foi usado por outra operação."
            );
            // Refused into a card, at once or later at an exchange: the same
            // card and words, never "attached".
            if let (Some(card_id), true) =
                (done.card_id, done.attachment_id.is_none() || done.outcome == "refused")
            {
                let message = store
                    .refusal_message(done.document_ident.as_deref(), done.refusal_reason.as_deref())?;
                return Err(ponteproj::attachments::TargetGone { card_id, message }.into());
            }
            return Ok(json!({
                "attachment_id": done.attachment_id,
                "name": name.trim(),
                "size": done.size,
            }));
        }
    }
    let ident = optional_text(&options, "document_ident");
    let space = optional_text(&options, "library_space");
    let bytes = read_attachment_file(path, name)?;
    let id = if operation.is_none() && expected.is_none() && ident.is_none() && space.is_none() {
        store.add_attachment(
            ponteproj::attachments::Owner::Document(document_id),
            name,
            description,
            &bytes,
        )?
    } else {
        store.attach_to_document(&ponteproj::attachments::AttachRequest {
            document_id,
            name,
            description,
            data: &bytes,
            operation_id: operation.as_deref(),
            expected_number: expected.as_deref(),
            document_ident: ident.as_deref(),
            library_space: space.as_deref(),
        })?
    };
    Ok(json!({"attachment_id": id, "name": name.trim(), "size": bytes.len()}))
}

/// A manual record from a photo: the old entry point, now a case of [`create_record`].
fn create_photo_record(store: &mut Store, input: &mut Value, image_path: &str) -> Result<Value> {
    let title = input["title"].as_str().unwrap_or("").trim().to_owned();
    let has_text = !input["text"].as_str().unwrap_or("").trim().is_empty();
    if let Some(map) = input.as_object_mut() {
        map.insert(
            "file_name".into(),
            json!(format!("foto-{}.jpg", chrono::Local::now().format("%Y-%m-%d-%H%M%S"))),
        );
        map.insert("file_description".into(), json!("Foto tirada no celular"));
        if !has_text {
            map.insert("text".into(), json!(format!("Registro feito no celular a partir de uma foto: {title}.")));
        }
    }
    create_record(store, input, image_path)
}

/// A manual record from what the person chose or received:
/// `{"kind","title","origin","date","text","file_name","file_description"}` and
/// optionally a file (`file_path` empty: none). The file becomes the record's
/// attachment, the text its text; at least one of the two is needed. The file
/// is read and checked BEFORE the record exists, so a file the core refuses
/// leaves nothing behind; the record and its file are then stored by the core in
/// one transaction (nothing at all if the file cannot be stored), so there is never
/// a record without the file the person meant to keep.
fn create_record(store: &mut Store, input: &Value, file_path: &str) -> Result<Value> {
    let str_of = |key: &str| input[key].as_str().unwrap_or("").trim().to_owned();
    // The app's idempotency key: a committed operation creates nothing again.
    let operation = optional_text(input, "operation_id");
    if let Some(id) = &operation {
        if let Some(done) = store.operation_record(id)? {
            // The id of an attachment is not the id of a record: nothing is
            // written and the document of that other operation is not
            // answered as if a record had been created.
            anyhow::ensure!(
                done.kind == "create_record",
                "Este identificador de operação já foi usado por outra operação."
            );
            return Ok(json!({ "document_id": done.document_id }));
        }
    }
    let file = if file_path.is_empty() {
        None
    } else {
        let name = str_of("file_name");
        let bytes = read_attachment_file(file_path, &name)?;
        Some((name, str_of("file_description"), bytes))
    };
    create_record_with(store, input, file)
}

/// [`create_record`] with the file (name, description, bytes) in hand.
fn create_record_with(store: &mut Store, input: &Value, file: Option<(String, String, Vec<u8>)>) -> Result<Value> {
    use ponteproj::attachments::{validate_description, validate_file};
    use ponteproj::record_import::{Created, NewAttachment, NewRecord};
    let str_of = |key: &str| input[key].as_str().unwrap_or("").trim().to_owned();
    let operation = optional_text(input, "operation_id");
    let title = str_of("title");
    let mut text = str_of("text");
    if let Some((name, description, bytes)) = &file {
        validate_file(name, bytes)?;
        validate_description(description)?;
    }
    anyhow::ensure!(
        file.is_some() || !text.is_empty(),
        "Não há nada para guardar: escreva um texto ou escolha um arquivo."
    );
    if text.is_empty() {
        text = format!("Registro feito no celular a partir de um arquivo recebido: {title}.");
    }
    let fields = ponteproj::manual::ManualFields {
        kind: if str_of("kind").is_empty() { "papel".into() } else { str_of("kind") },
        title,
        origin: str_of("origin"),
        document_date: str_of("date"),
        text,
    };
    let Some((name, description, bytes)) = file else {
        let Some(operation) = &operation else {
            let receipt = store.create_manual_document(&fields)?;
            return Ok(json!({ "document_id": receipt.document_id }));
        };
        // With an operation id the record is made by the same single-transaction
        // operation as a record with a file (without any file), so the row of
        // the id is committed with the record.
        let record = NewRecord {
            fields,
            metadata: Default::default(),
            request_id: uuid::Uuid::new_v4().to_string(),
        };
        return match store.create_record_for_operation(&record, &[], Some(operation))? {
            Created::New { document_id, .. } | Created::Already { document_id } => Ok(json!({ "document_id": document_id })),
        };
    };
    // The record and its file are written by the core in ONE transaction:
    // if the file cannot be stored, there is no record, nothing is queued for
    // the other computers and nothing has to be cleaned up afterwards.
    let record = NewRecord {
        fields,
        metadata: Default::default(),
        request_id: uuid::Uuid::new_v4().to_string(),
    };
    let attachment = NewAttachment { name, bytes, description };
    match store.create_record_for_operation(&record, &[attachment], operation.as_deref())? {
        Created::New { document_id, .. } | Created::Already { document_id } => Ok(json!({ "document_id": document_id })),
    }
}

/// A manual record from a file and/or a text (see [`create_record`]); `file_path`
/// is "" for a record made of text only.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_createRecord<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    fields_json: JString<'local>,
    file_path: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &fields_json)?;
        let file = text(env, &file_path)?;
        with_store(|store| create_record(store, &input, &file))
    })
}

/// Attaches a file (a private working copy at `path`, named `name`) to a document.
/// `options_json` is an object with the optional `operation_id` and
/// `expected_number` (see [`attach_file_with`]); `{}`, an empty or an unparsable
/// string is the plain call. Answer: `{attachment_id, name, size}`, or
/// `{ok:false, code:"target_changed", error}`.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_attachFile<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    document_id: jlong,
    path: JString<'local>,
    name: JString<'local>,
    description: JString<'local>,
    options_json: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let path = text(env, &path)?;
        let name = text(env, &name)?;
        let description = text(env, &description)?;
        let options = text(env, &options_json)?;
        with_store(|store| attach_file_with(store, document_id, &path, &name, &description, &options))
    })
}

/// Whether the operation `{"operation_id":"<uuid>"}` was committed:
/// `{done:true, document_id:N, kind:"record"}` for a created record,
/// `{done:true, document_id:N, kind:"attach"}` for a file attached to an existing
/// document, or `{done:false}` (also for an unknown, empty or unreadable id).
/// Read only.
fn operation_status(store: &Store, input: &str) -> Result<Value> {
    let input: Value = serde_json::from_str(input).unwrap_or(Value::Null);
    let done = match optional_text(&input, "operation_id") {
        Some(id) => store.operation_record(&id)?,
        None => None,
    };
    Ok(match done {
        Some(done) => {
            let mut answer = json!({"done": true, "document_id": done.document_id});
            let kind = match done.kind.as_str() {
                "create_record" => Some("record"),
                "attach" => Some("attach"),
                _ => None,
            };
            if let Some(kind) = kind {
                answer["kind"] = json!(kind);
            }
            if done.kind == "attach" {
                answer["outcome"] = json!(done.outcome);
                if let Some(card) = done.card_id {
                    answer["card_id"] = json!(card);
                }
                if done.outcome == "refused" {
                    answer["message"] = json!(store
                        .refusal_message(done.document_ident.as_deref(), done.refusal_reason.as_deref())?);
                }
            }
            answer
        }
        None => json!({"done": false}),
    })
}

/// `{"operation_id":"<uuid>"}` -> `{ok:true, done:bool[, document_id:N, kind:"record"|"attach"]}`,
/// and for a file attached, its `outcome` (`accepted`, `pending` until an
/// exchange, `refused` with its `card_id` and `message`):
/// after a process death, the app asks whether the save it had started was
/// committed, and whether it made a record or attached a file.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_operationStatus<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    json: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = text(env, &json)?;
        with_store(|store| operation_status(store, &input))
    })
}

// ---------------------------------------------------------------------------
// F3.1b2: a photo or file the app holds for a record (`PendingAttach`,
// `desenho-f3-v3-adendo.md` §10): it goes to that record, proved by its
// identity and the library space, or it is kept in a card that the recovery
// screen offers to make a record of, to attach to another record, or to drop.

/// The proof of record `document_id` the app keeps with a photo taken for
/// it: `{ident, library_space}` (both empty when there is none).
fn attach_target(store: &Store, document_id: i64) -> Result<Value> {
    let (ident, space) = store.attach_proof(document_id)?;
    Ok(json!({"ident": ident, "library_space": space}))
}

/// The attachments refused into a card and still waiting for the person:
/// `{items:[{operation_id, card_id, name, size, message}]}`, oldest first.
fn refused_attachments(store: &Store) -> Result<Value> {
    let items = store
        .refused_attachments()?
        .into_iter()
        .map(|r| {
            json!({"operation_id": r.operation_id, "card_id": r.card_id, "name": r.name,
                   "size": r.size, "message": r.message})
        })
        .collect::<Vec<_>>();
    Ok(json!({ "items": items }))
}

/// The operation id a recovery action must carry (adendo §10 rule 6: it is
/// persisted by the app before the call, and makes the call idempotent).
fn recovery_operation(input: &Value) -> Result<String> {
    optional_text(input, "operation_id")
        .context("Falta o identificador da operação de recuperação.")
}

/// "Criar registro com esta foto": a new record (`fields` as for
/// `createRecord`) holding the file of card `card_id`; the card is resolved
/// in the record's transaction. A repeat of the operation creates nothing.
fn create_record_from_card(store: &mut Store, card_id: i64, input: &Value) -> Result<Value> {
    let operation = recovery_operation(input)?;
    if store.operation_record(&operation)?.is_some() {
        return create_record(store, input, "");
    }
    let file = store.card_file(card_id)?;
    store.recover_card(card_id, &operation)?;
    create_record_with(store, input, Some((file.name, file.description, file.bytes)))
}

/// "Anexar a outro registro…": the file of card `card_id` to record
/// `document_id`, chosen explicitly and sent with its own proof (`options`
/// as for `attachFile`); the card is resolved in the attachment's
/// transaction. A repeat attaches nothing; a record that is gone too
/// writes nothing and leaves the card.
fn attach_from_card(store: &mut Store, card_id: i64, document_id: i64, options: &str) -> Result<Value> {
    let parsed: Value = serde_json::from_str(options).unwrap_or(Value::Null);
    let operation = recovery_operation(&parsed)?;
    if let Some(done) = store.operation_record(&operation)? {
        anyhow::ensure!(
            done.kind == "attach" && done.attachment_id.is_some(),
            "Este identificador de operação já foi usado por outra operação."
        );
        return Ok(json!({"attachment_id": done.attachment_id, "size": done.size}));
    }
    let file = store.card_file(card_id)?;
    store.recover_card(card_id, &operation)?;
    let id = store.attach_to_document(&ponteproj::attachments::AttachRequest {
        document_id,
        name: &file.name,
        description: &file.description,
        data: &file.bytes,
        operation_id: Some(&operation),
        expected_number: optional_text(&parsed, "expected_number").as_deref(),
        document_ident: optional_text(&parsed, "document_ident").as_deref(),
        library_space: optional_text(&parsed, "library_space").as_deref(),
    })?;
    Ok(json!({"attachment_id": id, "size": file.bytes.len()}))
}

/// "Descartar": card `card_id` is resolved and its file let go.
fn discard_card(store: &mut Store, card_id: i64) -> Result<Value> {
    store.discard_card(card_id)?;
    Ok(json!({}))
}

/// The refused files waiting for the person (see [`refused_attachments`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_refusedAttachments<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| with_store(|store| refused_attachments(store)))
}

/// "Criar registro com esta foto" (see [`create_record_from_card`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_createRecordFromCard<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    card_id: jlong,
    fields_json: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &fields_json)?;
        with_store(|store| create_record_from_card(store, card_id, &input))
    })
}

/// "Anexar a outro registro…" (see [`attach_from_card`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_attachFromCard<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    card_id: jlong,
    document_id: jlong,
    options_json: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let options = text(env, &options_json)?;
        with_store(|store| attach_from_card(store, card_id, document_id, &options))
    })
}

/// "Descartar" (see [`discard_card`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_discardCard<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    card_id: jlong,
) -> jstring {
    answer(&mut env, |_| with_store(|store| discard_card(store, card_id)))
}

// ---------------------------------------------------------------------------
// The responsible people, the atendimentos (read-only) and the search over the
// whole library. Pure functions over the store, tested on the host.

/// Lowercase without accents, for comparing text: the core's one fold.
fn fold(value: &str) -> String {
    ponteproj::labels::fold(value)
}

/// How many names `assignees` sends. The app filters them as the person types
/// and shows at most eight of what is left, so the pool is larger than that.
const ASSIGNEE_POOL: usize = 40;

/// The names used as responsible person of the tasks and of the atendimentos,
/// most used first. Names that differ only in capitals, accents or spaces are one
/// person and count together; the spelling shown is the most frequent one (on a tie,
/// the first in alphabetical order). Equal counts come in alphabetical order of the
/// folded name. At most `limit` names.
fn assignee_names(
    tasks: &[ponteproj::tasks::Task],
    attendances: &[ponteproj::saiop::SaiopAttendance],
    limit: usize,
) -> Vec<String> {
    let mut seen = std::collections::HashMap::<String, (usize, std::collections::HashMap<String, usize>)>::new();
    let names = tasks
        .iter()
        .map(|t| t.assignee.as_str())
        .chain(attendances.iter().map(|a| a.assignee.as_str()));
    for raw in names {
        let spelled = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if spelled.is_empty() {
            continue;
        }
        let entry = seen.entry(fold(&spelled)).or_default();
        entry.0 += 1;
        *entry.1.entry(spelled).or_default() += 1;
    }
    let mut ranked: Vec<(usize, String, String)> = seen
        .into_iter()
        .map(|(key, (total, spellings))| {
            let best = spellings
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                .map(|(spelling, _)| spelling)
                .unwrap_or_default();
            (total, key, best)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    ranked.into_iter().take(limit).map(|(_, _, name)| name).collect()
}

/// `{names:[...]}`: the suggestions for the Responsável field.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_assignees<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let names = assignee_names(&store.list_tasks()?, &store.list_saiop_attendances()?, ASSIGNEE_POOL);
            Ok(json!({ "names": names }))
        })
    })
}

/// What an atendimento is called in a list: its reference, else its subject.
fn attendance_reference(a: &ponteproj::saiop::SaiopAttendance) -> &str {
    if !a.reference_raw.is_empty() {
        a.reference_raw.as_str()
    } else if !a.subject.is_empty() {
        a.subject.as_str()
    } else {
        "Sem referência"
    }
}

/// Every atendimento for the list: the open ones first (late, then due today, then by
/// deadline, then without one; equal deadlines keep the library's order, newest first),
/// the closed ones after (most recently closed first).
fn attendance_rows(store: &Store, today: &str) -> Result<Value> {
    let (mut open, mut closed): (Vec<_>, Vec<_>) = store
        .list_saiop_attendances()?
        .into_iter()
        .partition(|a| a.is_open());
    let by_deadline = |a: &ponteproj::saiop::SaiopAttendance| (a.due_date.trim().is_empty(), a.due_date.trim().to_owned());
    open.sort_by_key(by_deadline);
    closed.sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
    let open_count = open.len();
    let rows = open
        .iter()
        .chain(closed.iter())
        .map(|a| attendance_item_json(a, today))
        .collect::<Vec<_>>();
    Ok(json!({ "attendances": rows, "open": open_count }))
}

/// An atendimento as a row of a list.
fn attendance_item_json(a: &ponteproj::saiop::SaiopAttendance, today: &str) -> Value {
    let is_open = a.is_open();
    let due = a.due_date.trim();
    json!({
        "id": a.id,
        "reference": attendance_reference(a),
        "subject": a.subject,
        "status": a.status,
        "status_label": ponteproj::saiop::status_label(&a.status),
        "open": is_open,
        "due_date": due,
        "due_label": ponteproj::labels::date(due),
        "due_chip": if due.is_empty() { String::new() } else { ponteproj::labels::due_relative(due, today) },
        "late": is_open && !due.is_empty() && due < today,
        "due_today": is_open && due == today,
        "assignee": a.assignee,
        "observation": a.observation,
    })
}

/// The follow-ups ("cobranças") to make today, the most overdue first: each row is an atendimento
/// row plus `follow_up` (why it is here) and `follow_up_days` (days overdue, 0 is today). `channels`
/// are the contacts the computer offers when registering one. Nothing here reads or changes the deadline.
fn follow_up_rows(store: &Store, today: &str) -> Result<Value> {
    use ponteproj::saiop as s;
    let day = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").context("Data inválida.")?;
    let all = store.list_saiop_attendances()?;
    let rows = s::follow_ups_due_today(all.iter(), day, s::DEFAULT_FOLLOW_UP_QUIET_DAYS)
        .into_iter()
        .filter_map(|f| {
            let a = all.iter().find(|a| a.id == f.attendance_id)?;
            let mut row = attendance_item_json(a, today);
            row["follow_up"] = json!(f.reason);
            row["follow_up_days"] = json!(f.overdue_days);
            Some(row)
        })
        .collect::<Vec<_>>();
    Ok(json!({ "count": rows.len(), "channels": s::FOLLOW_UP_CHANNELS, "follow_ups": rows }))
}

/// The atendimentos to chase today (see [`follow_up_rows`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listFollowUps<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
            follow_up_rows(store, &today)
        })
    })
}

/// A date `AAAA-MM-DD` of the input, when there is one.
fn optional_follow_up_date(input: &Value, key: &str) -> Result<Option<chrono::NaiveDate>> {
    match input[key].as_str().map(str::trim).filter(|d| !d.is_empty()) {
        None => Ok(None),
        Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(Some)
            .context("Informe a data da próxima cobrança."),
    }
}

fn follow_up_id(input: &Value) -> Result<i64> {
    input["id"].as_i64().filter(|id| *id > 0).context("Atendimento inválido.")
}

/// Registers a contact in the atendimento's timeline: `{"id":1,"contact":"Cobrado por telefone",
/// "next_date":"AAAA-MM-DD"}` (`contact` empty: "Cobrado por telefone"; `next_date` optional).
fn register_follow_up(store: &mut Store, input: &Value) -> Result<Value> {
    let id = follow_up_id(input)?;
    let next = optional_follow_up_date(input, "next_date")?;
    store.register_follow_up_contact(id, input["contact"].as_str().unwrap_or(""), next)?;
    Ok(json!({}))
}

/// Only sets the next follow-up: `{"id":1,"next_date":"AAAA-MM-DD"}` (required).
fn schedule_follow_up(store: &mut Store, input: &Value) -> Result<Value> {
    let id = follow_up_id(input)?;
    let next = optional_follow_up_date(input, "next_date")?.context("Informe a data da próxima cobrança.")?;
    store.schedule_next_follow_up(id, next)?;
    Ok(json!({}))
}

/// "Registrar cobrança": a contact in the timeline, and maybe the next date (see [`register_follow_up`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_registerFollowUp<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    json_text: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &json_text)?;
        with_store(|store| register_follow_up(store, &input))
    })
}

/// "Próxima cobrança em…": only the date (see [`schedule_follow_up`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_scheduleFollowUp<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    json_text: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &json_text)?;
        with_store(|store| schedule_follow_up(store, &input))
    })
}

/// All the atendimentos, ordered for the list (see [`attendance_rows`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listAttendances<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
            attendance_rows(store, &today)
        })
    })
}

/// A label and its text, for the ficha.
fn field(label: &str, value: &str) -> Option<Value> {
    let value = value.trim();
    (!value.is_empty()).then(|| json!({"label": label, "value": value}))
}

/// A yes/no/not-informed answer is shown only when somebody informed it.
fn tri_state_field(label: &str, value: &str) -> Option<Value> {
    (value != "nao_informado").then(|| field(label, ponteproj::saiop::tri_state_label(value))).flatten()
}

/// The ficha of an atendimento, read-only, with the labels the desktop ficha uses
/// (`src/saiop.rs`, `ficha_form`): what is stored and not empty, in the desktop's order.
fn get_attendance(store: &Store, id: i64, today: &str) -> Result<Value> {
    use ponteproj::attachments::Owner;
    use ponteproj::saiop as s;
    let a = store
        .list_saiop_attendances()?
        .into_iter()
        .find(|a| a.id == id)
        .context("Atendimento não encontrado. Ele pode ter sido excluído em outro aparelho.")?;
    let is_open = a.is_open();
    let due = a.due_date.trim();
    let documents = store.list_documents("")?;
    let linked = a.document_id.and_then(|d| documents.iter().find(|x| x.id == d));
    let tasks = store.list_tasks()?;
    let linked_tasks = a
        .task_ids
        .iter()
        .filter_map(|t| tasks.iter().find(|x| x.id == *t))
        .map(|t| {
            json!({
                "id": t.id,
                "title": t.title,
                "done": t.completed_at.is_some() || t.status == "concluida",
                "due_label": ponteproj::labels::date(&t.due_date),
            })
        })
        .collect::<Vec<_>>();
    let attachments = store
        .list_attachments(Owner::Attendance(id))?
        .into_iter()
        .map(|m| json!({"id": m.id, "name": m.name, "kind": m.kind, "size": m.size, "description": m.description}))
        .collect::<Vec<_>>();
    // Newest first, as the desktop timeline shows them.
    let events = a
        .events
        .iter()
        .rev()
        .map(|e| {
            json!({
                "when": ponteproj::labels::datetime(&e.when),
                "actor": e.actor,
                "kind": ponteproj::labels::saiop_event_kind(&e.kind),
                "detail": e.detail,
            })
        })
        .collect::<Vec<_>>();
    let mut flags = Vec::new();
    if a.scheduled {
        flags.push("Agendado");
    }
    if a.sensitive {
        flags.push("Documento sensível");
    }
    let fields = [
        field("Situação", s::status_label(&a.status)),
        field("Prioridade", s::priority_label(&a.priority)),
        field("Motivo da prioridade", &a.priority_reason),
        field("Prazo", &ponteproj::labels::date(due)),
        field("Tipo de prazo", s::due_kind_label(&a.due_kind)).filter(|_| !due.is_empty()),
        field("Responsável", &a.assignee),
        field("Entrada", &ponteproj::labels::date(&a.entry_date)),
        field("Recebido de", &a.received_from),
        field("Observações", &a.observation),
        tri_state_field("Retorno para UAS?", &a.uas_return),
        tri_state_field("Assinado?", &a.signed_status),
        field("Apensado / juntado", &a.attachment_relation),
        field("Entrega prevista", &a.planned_doc_type),
        field("Resultado da etapa", s::outcome_label(&a.outcome)).filter(|_| !a.outcome.is_empty()),
        field("Data de saída", &ponteproj::labels::date(&a.exit_date)),
        field("Enviado para", &a.sent_to),
        field("Verificação na planilha", &a.verification_raw),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    Ok(json!({
        "id": a.id,
        "reference": attendance_reference(&a),
        "subject": a.subject,
        "status_label": s::status_label(&a.status),
        "open": is_open,
        "due_label": ponteproj::labels::date(due),
        "late": is_open && !due.is_empty() && due < today,
        "due_today": is_open && due == today,
        "fields": fields,
        "flags": flags,
        "review_flags": a.review_flags,
        "document_id": linked.map(|d| d.id).unwrap_or(0),
        "document_number": linked.map(|d| d.document_number.clone()).unwrap_or_default(),
        "document_title": linked.map(|d| d.title.clone()).unwrap_or_default(),
        "tasks": linked_tasks,
        "events": events,
        "attachments": attachments,
    }))
}

/// One atendimento, to read (nothing here writes).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_getAttendance<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    attendance_id: jlong,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
            get_attendance(store, attendance_id, &today)
        })
    })
}

/// A resource of the register, for the phone (read-only). `notes` are shown on the detail screen,
/// as on the computer; nothing here is logged.
fn resource_json(r: &ponteproj::resources::Resource) -> Value {
    json!({
        "uid": r.uid,
        "name": r.name,
        "quantity": r.quantity,
        "location": r.location,
        "identification": r.identification,
        "notes": r.notes,
        "status": r.status.as_str(),
        "status_label": r.status.label(),
        "active": r.status == ponteproj::resources::ResourceStatus::Active,
    })
}

/// Every resource, by name (the core's order).
fn resource_rows(store: &Store) -> Result<Value> {
    let rows = store.list_resources()?.iter().map(resource_json).collect::<Vec<_>>();
    Ok(json!({ "resources": rows }))
}

/// Every use with the names the list and the detail need (resource, linked document, atendimento
/// and task). A link that no longer exists comes as 0 / empty. `now` is `AAAA-MM-DDTHH:MM`.
fn resource_use_rows(store: &Store, now: &str) -> Result<Value> {
    let resources = store.list_resources()?;
    let documents = store.list_documents("")?;
    let attendances = store.list_saiop_attendances()?;
    let tasks = store.list_tasks()?;
    let rows = store
        .list_resource_uses()?
        .iter()
        .map(|u| {
            let resource = resources.iter().find(|r| r.uid == u.resource_uid);
            let document = u.document_id.and_then(|d| documents.iter().find(|x| x.id == d));
            let attendance = u.saiop_id.and_then(|a| attendances.iter().find(|x| x.id == a));
            let task = u.task_id.and_then(|t| tasks.iter().find(|x| x.id == t));
            json!({
                "uid": u.uid,
                "resource_uid": u.resource_uid,
                "resource_name": resource.map(|r| r.name.as_str()).unwrap_or(""),
                "quantity": u.quantity,
                "who": u.who,
                "sector": u.sector,
                "purpose": u.purpose,
                "start": u.start,
                "end": u.end,
                "returned_at": u.returned_at,
                "status": u.status.as_str(),
                "status_label": u.status.label(),
                "notes": u.notes,
                "overdue": u.overdue_at(now),
                "period": ponteproj::resources::period_text(&u.start, &u.end),
                "document_id": document.map(|d| d.id).unwrap_or(0),
                "document_number": document.map(|d| d.document_number.as_str()).unwrap_or(""),
                "document_title": document.map(|d| d.title.as_str()).unwrap_or(""),
                "attendance_id": attendance.map(|a| a.id).unwrap_or(0),
                "attendance_reference": attendance.map(attendance_reference).unwrap_or(""),
                "task_id": task.map(|t| t.id).unwrap_or(0),
                "task_title": task.map(|t| t.title.as_str()).unwrap_or(""),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "uses": rows }))
}

/// The Recursos screens, everything decided as of `now` (`AAAA-MM-DDTHH:MM`):
/// - `uses`: every use (see [`resource_use_rows`], with `period` and `overdue`);
/// - `resources`: every resource with `in_use_now`, `free_now`, `usage_line`,
///   `free_text`, `today` (uids of the uses holding it today, by start) and
///   `next` (uids of what is out or still ahead, by start);
/// - `groups`: the uses that match `query`, in the desktop's groups, each
///   `{key, title, late, uids}`, only the ones with something in them;
/// - `rows`: uids of the resources that match `query`, the active ones first.
fn resource_view(store: &Store, query: &str, now: &str) -> Result<Value> {
    use ponteproj::resources::{day_occupation, group_uses, query_matches, taken_now};
    let day = chrono::NaiveDate::parse_from_str(&now[..10.min(now.len())], "%Y-%m-%d").context("Data inválida.")?;
    let day_text = day.format("%Y-%m-%d").to_string();
    let resources = store.list_resources()?;
    let uses = store.list_resource_uses()?;
    let mut view = resource_use_rows(store, now)?;
    let documents = store.list_documents("")?;
    let attendances = store.list_saiop_attendances()?;
    let tasks = store.list_tasks()?;
    // What the desktop searches a use by, plus what only the phone showed: the linked
    // document (number, title), the atendimento (reference) and the task (title).
    let extra_of = |u: &ponteproj::resources::ResourceUse| {
        let document = u.document_id.and_then(|d| documents.iter().find(|x| x.id == d));
        let attendance = u.saiop_id.and_then(|a| attendances.iter().find(|x| x.id == a));
        let task = u.task_id.and_then(|t| tasks.iter().find(|x| x.id == t));
        format!(
            "{} {} {} {}",
            document.map(|d| d.document_number.as_str()).unwrap_or(""),
            document.map(|d| d.title.as_str()).unwrap_or(""),
            attendance.map(attendance_reference).unwrap_or(""),
            task.map(|t| t.title.as_str()).unwrap_or("")
        )
    };
    let matching: Vec<&ponteproj::resources::ResourceUse> = uses
        .iter()
        .filter(|u| {
            let resource = resources.iter().find(|r| r.uid == u.resource_uid);
            query_matches(u, resource, query, &extra_of(u))
        })
        .collect();
    let groups = group_uses(&matching, day, now);
    let group_rows = groups
        .sections()
        .iter()
        .filter(|(_, _, list)| !list.is_empty())
        .map(|(key, title, list)| {
            json!({
                "key": key, "title": title, "late": *key == "atrasados",
                "uids": list.iter().map(|u| u.uid.as_str()).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let mut resource_rows = Vec::new();
    for r in &resources {
        let taken = taken_now(&uses, &r.uid, now);
        let free = r.quantity.saturating_sub(taken);
        let mine: Vec<&ponteproj::resources::ResourceUse> = uses.iter().filter(|u| u.resource_uid == r.uid).collect();
        let own = group_uses(&mine, day, now);
        let mut ahead: Vec<&ponteproj::resources::ResourceUse> = own.sections()[..5].iter().flat_map(|(_, _, l)| l.iter().copied()).collect();
        ahead.sort_by(|a, b| (&a.start, &a.end).cmp(&(&b.start, &b.end)));
        let today_uses = day_occupation(&uses, &r.uid, "", &day_text, now);
        let mut json = resource_json(r);
        json["in_use_now"] = json!(taken);
        json["free_now"] = json!(free);
        json["usage_line"] = json!(format!("{taken} de {} em uso agora", r.quantity));
        json["free_text"] = json!(ponteproj::labels::count(free as usize, "livre", "livres"));
        json["today"] = json!(today_uses.iter().map(|u| u.uid.as_str()).collect::<Vec<_>>());
        json["next"] = json!(ahead.iter().map(|u| u.uid.as_str()).collect::<Vec<_>>());
        resource_rows.push(json);
    }
    let folded = ponteproj::labels::fold(query);
    let hit = |r: &ponteproj::resources::Resource| {
        let haystack = ponteproj::labels::fold(&format!("{} {} {}", r.name, r.location, r.identification));
        folded.split_whitespace().all(|w| haystack.contains(w))
    };
    let mut rows: Vec<&str> = resources.iter().filter(|r| hit(r) && r.status == ponteproj::resources::ResourceStatus::Active).map(|r| r.uid.as_str()).collect();
    rows.extend(resources.iter().filter(|r| hit(r) && r.status != ponteproj::resources::ResourceStatus::Active).map(|r| r.uid.as_str()));
    view["resources"] = json!(resource_rows);
    view["groups"] = json!(group_rows);
    view["rows"] = json!(rows);
    Ok(view)
}

/// The two counts of Hoje: reservations of the day and overdue returns (counts only).
fn resource_counts(store: &Store, today: &str, now: &str) -> Result<Value> {
    let day = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").context("Data inválida.")?;
    let counts = ponteproj::resources::day_counts(&store.list_resource_uses()?, day, now);
    Ok(json!({ "reservations_today": counts.reservations_today, "overdue": counts.overdue }))
}

/// The resources register (read-only).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listResources<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| with_store(|store| resource_rows(store)))
}

/// Every use of a resource, with the "overdue" flag computed now.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listResourceUses<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| resource_use_rows(store, &ponteproj::resources::now_moment()))
    })
}

/// The Recursos screens for the text typed (empty: everything), as of now.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_resourceView<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    query: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let query = text(env, &query)?;
        with_store(|store| resource_view(store, &query, &ponteproj::resources::now_moment()))
    })
}

/// The ids, in order, of the documents the core's search finds for `query`
/// (words, case and accents ignored): the same search as the global one. An
/// empty query is every document.
fn document_ids(store: &Store, query: &str) -> Result<Value> {
    let text: String = query.trim().chars().take(SEARCH_MAX_CHARS).collect();
    let ids = store.list_documents(&text)?.iter().map(|d| d.id).collect::<Vec<_>>();
    Ok(json!({ "ids": ids }))
}

/// The ids of the documents that match the text of the Documentos tab or of the picker.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_documentIds<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    query: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let query = text(env, &query)?;
        with_store(|store| document_ids(store, &query))
    })
}

/// Rows per group of the search; the rest is counted in `more`.
const SEARCH_GROUP: usize = 8;
/// Fewer characters than this do not search (as on the computer).
const SEARCH_MIN_CHARS: usize = 2;
/// A longer text is cut: nobody looks for more than this.
const SEARCH_MAX_CHARS: usize = 200;

/// Every word of the (already folded) `needle` appears in some of the fields.
fn fields_match(needle: &str, fields: &[&str]) -> bool {
    let haystack = fold(&fields.join("\n"));
    needle.split_whitespace().all(|word| haystack.contains(word))
}

/// At most `SEARCH_GROUP` rows and how many were left out.
fn group(rows: Vec<Value>) -> Value {
    let more = rows.len().saturating_sub(SEARCH_GROUP);
    json!({ "rows": rows.into_iter().take(SEARCH_GROUP).collect::<Vec<_>>(), "more": more })
}

/// The search over the whole library, with the rules of the desktop's (`src/app/search.rs`,
/// desktop-only, so re-expressed here): documents by the core's own search (number,
/// subject, text, notes, PDFs); tasks by title, notes and responsible and atendimentos by
/// reference, subject, observation and responsible, both without caring for capitals or
/// accents and with every word present; open tasks before concluded ones. The text is
/// used for this search only: it is not logged and nothing keeps it.
fn search_all(store: &Store, query: &str, today: &str) -> Result<Value> {
    let text: String = query.trim().chars().take(SEARCH_MAX_CHARS).collect();
    anyhow::ensure!(
        text.chars().count() >= SEARCH_MIN_CHARS,
        "Digite ao menos 2 letras para buscar."
    );
    let needle = fold(&text);
    let documents = store
        .list_documents(&text)?
        .into_iter()
        .map(|d| {
            json!({
                "id": d.id,
                "number": d.document_number,
                "title": d.title,
                "kind": if d.email { "E-mail" } else if d.manual { "Manual" } else { "" },
            })
        })
        .collect::<Vec<_>>();
    let numbers = document_numbers(store)?;
    let (done, open): (Vec<_>, Vec<_>) = store
        .list_tasks()?
        .into_iter()
        .filter(|t| fields_match(&needle, &[&t.title, &t.notes, &t.assignee]))
        .partition(|t| t.completed_at.is_some() || t.status == "concluida");
    let tasks = open
        .iter()
        .chain(done.iter())
        .map(|t| task_json_on(t, &numbers, today))
        .collect::<Vec<_>>();
    let attendances = store
        .list_saiop_attendances()?
        .into_iter()
        .filter(|a| fields_match(&needle, &[&a.reference_raw, &a.subject, &a.observation, &a.assignee]))
        .map(|a| {
            json!({
                "id": a.id,
                "reference": attendance_reference(&a),
                "subject": a.subject,
                "status_label": ponteproj::saiop::status_label(&a.status),
                "open": a.is_open(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "documents": group(documents),
        "tasks": group(tasks),
        "attendances": group(attendances),
    }))
}

/// The search over documents, tasks and atendimentos (see [`search_all`]).
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_search<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    query: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let query = text(env, &query)?;
        let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
        with_store(|store| search_all(store, &query, &today))
    })
}

// -- e-mail (IMAP), as in the desktop "Importar do e-mail" ------------------

fn account_of(store: &Store, uid: &str, password: &str) -> Result<ponteproj::imap::MailAccount> {
    let mut account = store
        .list_mail_accounts()?
        .into_iter()
        .find(|a| a.uid == uid)
        .context("Conta de e-mail não encontrada.")?;
    if account.password.is_empty() {
        account.password = password.to_owned();
    }
    anyhow::ensure!(!account.password.is_empty(), "Digite a senha de app desta conta.");
    Ok(account)
}

fn filter_key(filter: ponteproj::imap::Filter) -> &'static str {
    match filter {
        ponteproj::imap::Filter::Flagged => "estrela",
        ponteproj::imap::Filter::Unseen => "nao_lidas",
        ponteproj::imap::Filter::All => "todas",
    }
}

fn filter_of(value: &str) -> ponteproj::imap::Filter {
    match value {
        "estrela" => ponteproj::imap::Filter::Flagged,
        "nao_lidas" => ponteproj::imap::Filter::Unseen,
        _ => ponteproj::imap::Filter::All,
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listMailAccounts<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let accounts = store
                .list_mail_accounts()?
                .into_iter()
                .map(|a| json!({"uid": a.uid, "name": a.name, "host": a.host, "port": a.port,
                                "user": a.user, "remember": a.remember,
                                "has_password": !a.password.is_empty(), "folder": a.folder,
                                "filter": filter_key(a.filter), "last_used": a.last_used}))
                .collect::<Vec<_>>();
            Ok(json!({ "accounts": accounts }))
        })
    })
}

/// `{"uid":"" (new) or "...","name","host","port","user","password","remember","folder"}`.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_saveMailAccount<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    account_json: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &account_json)?;
        with_store(|store| {
            let str_of = |key: &str| input[key].as_str().unwrap_or("").to_owned();
            let mut account = ponteproj::imap::MailAccount {
                uid: str_of("uid"),
                name: str_of("name").trim().to_owned(),
                host: str_of("host").trim().to_owned(),
                port: u16::try_from(input["port"].as_u64().unwrap_or(993))
                    .ok()
                    .filter(|p| *p > 0)
                    .context("Porta inválida (use um número entre 1 e 65535; no Gmail, 993).")?,
                user: str_of("user").trim().to_owned(),
                password: str_of("password"),
                remember: input["remember"].as_bool().unwrap_or(false),
                folder: if str_of("folder").is_empty() { "INBOX".into() } else { str_of("folder") },
                ..Default::default()
            };
            // Editing without retyping the password keeps the stored one.
            if account.password.is_empty() && !account.uid.is_empty() {
                if let Some(stored) = store.list_mail_accounts()?.into_iter().find(|a| a.uid == account.uid) {
                    account.password = stored.password;
                }
            }
            let saved = store.save_mail_account(&account)?;
            Ok(json!({ "uid": saved.uid }))
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_deleteMailAccount<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    uid: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let uid = text(env, &uid)?;
        with_store(|store| {
            store.delete_mail_account(&uid)?;
            Ok(json!({}))
        })
    })
}

/// The folders of an account. The network exchange runs without holding the
/// library, so the app stays usable meanwhile.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listMailFolders<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    uid: JString<'local>,
    password: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let uid = text(env, &uid)?;
        let password = text(env, &password)?;
        let account = with_store(|store| account_of(store, &uid, &password))?;
        let folders = ponteproj::imap::list_folders(&account, &ponteproj::ai::Reporter::silent())?
            .into_iter()
            .filter(|f| !f.no_select)
            .map(|f| json!({"raw": f.raw, "name": f.name}))
            .collect::<Vec<_>>();
        Ok(json!({ "folders": folders }))
    })
}

/// The newest messages of a folder (`filter`: "todas", "estrela", "nao_lidas"),
/// each marked when it is already in the library.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listMailMessages<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    uid: JString<'local>,
    password: JString<'local>,
    folder: JString<'local>,
    filter: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let uid = text(env, &uid)?;
        let password = text(env, &password)?;
        let folder = text(env, &folder)?;
        let filter = filter_of(&text(env, &filter)?);
        let account = with_store(|store| account_of(store, &uid, &password))?;
        let messages = ponteproj::imap::list_messages(
            &account,
            &folder,
            filter,
            100,
            &ponteproj::ai::Reporter::silent(),
        )?;
        let listed = with_store(|store| {
            let _ = store.remember_mail_choice(&uid, &folder, filter);
            Ok(messages
                .iter()
                .map(|m| {
                    let imported = matches!(store.mail_already_imported(&m.message_id), Ok(Some(_)));
                    json!({"uid": m.uid, "from": m.from, "subject": m.subject, "date": m.date,
                           "size": m.size, "flagged": m.flagged, "imported": imported})
                })
                .collect::<Vec<_>>())
        })?;
        Ok(json!({ "messages": listed }))
    })
}

/// Downloads the chosen messages (uids as a JSON array) and imports each as
/// an "E-mail recebido" record with its attachments.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_importMail<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    uid: JString<'local>,
    password: JString<'local>,
    folder: JString<'local>,
    uids_json: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let uid = text(env, &uid)?;
        let password = text(env, &password)?;
        let folder = text(env, &folder)?;
        let uids: Vec<u32> = serde_json::from_value(json_arg(env, &uids_json)?)
            .context("Lista de mensagens inválida.")?;
        let account = with_store(|store| account_of(store, &uid, &password))?;
        let fetched = ponteproj::imap::fetch_messages(
            &account,
            &folder,
            &uids,
            &ponteproj::ai::Reporter::silent(),
        )?;
        let source = ponteproj::mail::MailSource::Account {
            name: account.name.clone(),
            address: account.user.trim().to_owned(),
        };
        let results = with_store(|store| {
            Ok(fetched
                .into_iter()
                .map(|(message_uid, bytes)| {
                    match bytes.and_then(|b| {
                        store.import_mail_bytes(&b, &source).map_err(|e| format!("{e:#}"))
                    }) {
                        Ok(done) => json!({"uid": message_uid, "ok": true,
                                           "subject": done.subject,
                                           "already": done.already_imported,
                                           "attachments": done.attachments_added,
                                           "notes": done.notes}),
                        Err(error) => json!({"uid": message_uid, "ok": false, "error": error}),
                    }
                })
                .collect::<Vec<_>>())
        })?;
        Ok(json!({ "results": results }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ponteproj::intranet::{IntranetConfig, IntranetMode};
    use std::path::{Path, PathBuf};

    /// The core reads PONTEPROJ_DATA_DIR; tests take turns.
    static LOCK: Mutex<()> = Mutex::new(());

    /// Leaves the process-wide holidays empty when a test ends, even by a
    /// panic (declared after the lock, so it drops while the lock is held).
    struct ResetHolidays;

    impl Drop for ResetHolidays {
        fn drop(&mut self) {
            ponteproj::holidays::set_config(ponteproj::holidays::HolidayConfig::new());
        }
    }
    const KEY: &str = "frase-de-teste-123";

    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Dir {
            let dir = std::env::temp_dir()
                .join(format!("ponte-mobile-test-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn open(dir: &Path, name: &str, member: &str) -> Store {
        let mut store = Store::open(&dir.join(name)).unwrap();
        let cfg = IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: vec![dir.join("ponte-sync.ponte").display().to_string()],
            folder_writer: uuid::Uuid::new_v4().simple().to_string(),
            ..store.load_intranet_config().unwrap()
        };
        store.save_intranet_config(&cfg).unwrap();
        store
    }

    /// A "computer" library with one document, and its first transport file.
    fn first_transport(dir: &Path) -> (Store, PathBuf, i64) {
        std::env::set_var("PONTEPROJ_DATA_DIR", dir);
        let mut computer = open(dir, "computador.sqlite3", "Computador de teste");
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/capture.json");
        let capture: ponteproj::model::Capture =
            serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        let id = computer.import_capture(&capture).unwrap().document_id;
        let drive = dir.join("drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let mut cfg = computer.load_intranet_config().unwrap();
        cfg.folder_files = vec![drive.display().to_string()];
        computer.save_intranet_config(&cfg).unwrap();
        let reporter = ponteproj::ai::Reporter::silent();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        (computer, drive, id)
    }

    fn leftovers(folder: &Path) -> Vec<String> {
        std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-"))
            .collect()
    }

    #[test]
    fn holidays_set_on_the_computer_reach_the_phone_and_back() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let reporter = ponteproj::ai::Reporter::silent();
        let date = chrono::NaiveDate::from_ymd_opt(2031, 3, 17).unwrap();

        // The computer chose a state and a day off, and sent them.
        computer
            .set_library_setting("holidays", "uf SP\ndia 2031-03-17 Recesso\n")
            .unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();

        // The phone joins: the answer carries the holidays, and they are in force.
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        let work_s = work.display().to_string();
        let first = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(first["holidays"], "uf SP\ndia 2031-03-17 Recesso\n");
        assert_eq!(ponteproj::holidays::holiday(date), Some("Recesso"));

        // The person changes them on the phone; the computer gets them.
        save_holidays(&mut phone, "uf MG\n").unwrap();
        let second = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(second["written"], json!(true));
        assert_eq!(second["holidays"], "uf MG\n");
        assert_eq!(ponteproj::holidays::holiday(date), None);
        std::fs::copy(&work, &drive).unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        assert_eq!(computer.library_setting("holidays").unwrap().as_deref(), Some("uf MG\n"));
    }

    #[test]
    fn a_record_the_copy_numbered_differently_is_told_in_the_sync_answer() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        let work_s = work.display().to_string();
        let joined = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(joined["renumbered"], json!([]));
        let fields = |title: &str| ponteproj::manual::ManualFields {
            kind: "ligacao".into(),
            title: title.into(),
            origin: String::new(),
            document_date: String::new(),
            text: "Texto".into(),
        };
        // The computer publishes a record; the phone, without that copy,
        // created one with the same number, and it waits to be sent.
        computer.create_manual_document(&fields("Do computador")).unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        computer
            .sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent())
            .unwrap();
        let mine = phone.create_manual_document(&fields("Do celular")).unwrap();
        let before = phone.load_document(mine.document_id).unwrap().summary;
        assert!(before.provisional, "waiting to be sent");
        std::fs::copy(&drive, &work).unwrap();
        let answer = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        let after = phone
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.title == "Do celular")
            .unwrap();
        assert!(!after.provisional);
        assert_ne!(after.document_number, before.document_number);
        let told = answer["renumbered"].as_array().unwrap();
        assert_eq!(told.len(), 1, "{answer}");
        assert_eq!(told[0]["from"], json!(before.document_number));
        assert_eq!(told[0]["to"], json!(after.document_number));
        let sentence = format!(
            "O registro “Do celular” passou de {} para {} ao sincronizar.",
            before.document_number, after.document_number
        );
        assert_eq!(told[0]["sentence"], json!(sentence));
        assert!(
            answer["summary"].as_str().unwrap().contains(sentence.trim_end_matches('.')),
            "{answer}"
        );
        // Told once.
        let again = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(again["renumbered"], json!([]));

        // And kept in the phone's library until dismissed, whatever the
        // status line shows afterwards (a failed publication, a later exchange).
        let listed = renumber_notices_json(&phone).unwrap();
        let notices = listed["notices"].as_array().unwrap();
        assert_eq!(notices.len(), 1, "{listed}");
        assert_eq!(notices[0]["sentence"], json!(sentence));
        assert_eq!(notices[0]["from"], json!(before.document_number));
        assert_eq!(notices[0]["to"], json!(after.document_number));
        assert_eq!(notices[0]["title"], json!("Do celular"));
        assert_eq!(notices[0]["document_id"], json!(after.id));
        assert!(notices[0]["id"].as_i64().unwrap() > 0);
        assert!(notices[0]["at"].as_str().unwrap().ends_with('Z'));
        let id = notices[0]["id"].as_i64().unwrap();
        assert_eq!(renumber_notices_json(&phone).unwrap(), listed, "listing changes nothing");
        phone.dismiss_renumber_notice(id).unwrap();
        phone.dismiss_renumber_notice(id).unwrap();
        phone.dismiss_renumber_notice(id + 100).unwrap();
        assert_eq!(renumber_notices_json(&phone).unwrap(), json!({"notices": []}));
        let after_dismissal = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(after_dismissal["renumbered"], json!([]));
        assert_eq!(renumber_notices_json(&phone).unwrap(), json!({"notices": []}));
    }

    #[test]
    fn path_sync_joins_then_sends_a_note_and_reports_the_written_length() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut computer, drive, id) = first_transport(&dir.0);
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);

        // The phone works on a private copy of the Drive file.
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        let work_s = work.display().to_string();
        let first = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(phone.list_documents("").unwrap().len(), 1, "adopted the library");
        assert_eq!(first["written"], json!(false));
        assert!(first.get("bytes").is_none());

        // A note on the phone: the copy changes and its length is reported.
        phone.save_note(id, "Nota do celular").unwrap();
        let second = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(second["written"], json!(true));
        let len = std::fs::metadata(&work).unwrap().len();
        assert_eq!(second["bytes"].as_u64().unwrap(), len);
        assert!(leftovers(&dir.0).is_empty());

        // The result is a valid transport: the computer receives the note.
        std::fs::copy(&work, &drive).unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        computer
            .sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent())
            .unwrap();
        assert_eq!(computer.load_document(id).unwrap().note, "Nota do celular");
    }

    #[test]
    fn the_same_input_gives_equivalent_output_on_either_run() {
        // The core encrypts with a fresh salt and nonce, so two outputs never
        // match byte for byte; what must match is what they say.
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (_computer, drive, id) = first_transport(&dir.0);
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let mut seen = Vec::new();
        for name in ["um", "dois"] {
            let work = dir.0.join(format!("{name}.ponte"));
            std::fs::copy(&drive, &work).unwrap();
            let mut phone = open(&dir.0, &format!("{name}.sqlite3"), "Celular");
            let w = work.display().to_string();
            sync_paths(&mut phone, &w, "", KEY).unwrap();
            phone.save_note(id, "mesma nota").unwrap();
            let answer = sync_paths(&mut phone, &w, "", KEY).unwrap();
            assert_eq!(answer["written"], json!(true));
            let copy = ponteproj::sharedfolder::read(&work, KEY).unwrap().unwrap();
            seen.push((copy.header.lineage.clone(), copy.header.revision));
        }
        assert_eq!(seen[0], seen[1]);
    }

    /// A library with one document (the fixture capture) and nothing else.
    fn library(dir: &Path) -> (Store, i64) {
        std::env::set_var("PONTEPROJ_DATA_DIR", dir);
        let mut store = open(dir, "acervo.sqlite3", "Celular de teste");
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/capture.json");
        let capture: ponteproj::model::Capture =
            serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        let id = store.import_capture(&capture).unwrap().document_id;
        (store, id)
    }

    fn add_task(store: &mut Store, document: Option<i64>, title: &str, due: &str) -> i64 {
        let mut task = ponteproj::tasks::Task::blank(document);
        task.title = title.into();
        task.due_date = due.into();
        store.save_task(&task).unwrap()
    }

    fn titles(list: &Value) -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|t| t["title"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn today_splits_late_today_and_upcoming_on_fixed_dates() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let today = "2026-11-10";
        add_task(&mut store, Some(doc), "ontem", "2026-11-09");
        add_task(&mut store, None, "muito atrasada", "2026-10-01");
        add_task(&mut store, Some(doc), "hoje", "2026-11-10");
        add_task(&mut store, Some(doc), "amanha", "2026-11-11");
        add_task(&mut store, Some(doc), "no limite", "2026-11-17");
        add_task(&mut store, Some(doc), "alem da semana", "2026-11-18");
        add_task(&mut store, Some(doc), "sem prazo", "");
        let done = add_task(&mut store, Some(doc), "concluida", "2026-11-01");
        store.complete_task(done).unwrap();

        let lists = today_lists(&store, today).unwrap();
        assert_eq!(titles(&lists["late"]), ["muito atrasada", "ontem"], "oldest first");
        assert_eq!(titles(&lists["today"]), ["hoje"]);
        assert_eq!(
            titles(&lists["upcoming"]),
            ["amanha", "no limite", "alem da semana"],
            "as on the computer: soonest first, however far"
        );
        assert_eq!(lists["late"][0]["overdue"], json!(true));
        assert_eq!(lists["today"][0]["due_today"], json!(true));
        // A task without document says so by an empty number and id 0.
        assert_eq!(lists["late"][0]["document_id"], json!(0));
        assert!(lists["attendances"].as_array().unwrap().is_empty());
    }

    #[test]
    fn today_upcoming_is_cut_to_six() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        for day in 11..=17 {
            add_task(&mut store, Some(doc), &format!("dia {day}"), &format!("2026-11-{day}"));
        }
        let lists = today_lists(&store, "2026-11-10").unwrap();
        assert_eq!(lists["upcoming"].as_array().unwrap().len(), 6);
        assert_eq!(lists["upcoming"][5]["title"], json!("dia 16"));
    }

    #[test]
    fn today_lists_open_attendances_due_today_or_before() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let mut make = |reference: &str, subject: &str, status: &str, due: &str| {
            let mut a = ponteproj::saiop::SaiopAttendance::new_manual();
            a.reference_raw = reference.into();
            a.subject = subject.into();
            a.status = status.into();
            a.due_date = due.into();
            store.save_saiop_attendance(&a, None).unwrap();
        };
        make("ORG-OFI-2026/00431", "Pedido de apoio", "em_analise", "2026-11-09");
        make("", "Sem referência própria", "a_distribuir", "2026-11-10");
        make("ORG-OFI-2026/00500", "Amanhã", "em_analise", "2026-11-11");
        make("ORG-OFI-2026/00501", "Encerrado", "encerrado", "2026-11-01");
        make("ORG-OFI-2026/00502", "Cancelado", "cancelado", "2026-11-01");
        make("ORG-OFI-2026/00503", "Sem prazo", "em_analise", "");
        let lists = today_lists(&store, "2026-11-10").unwrap();
        let rows = lists["attendances"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["reference"], json!("ORG-OFI-2026/00431"));
        assert_eq!(rows[0]["status_label"], json!("Em análise"));
        assert_eq!(rows[0]["late"], json!(true));
        assert_eq!(rows[1]["reference"], json!("Sem referência própria"));
        assert_eq!(rows[1]["late"], json!(false));
    }

    #[test]
    fn save_task_changes_only_the_sent_fields_and_keeps_the_rest() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let other = add_task(&mut store, Some(doc), "anterior", "");
        let mut task = ponteproj::tasks::Task::blank(Some(doc));
        task.title = "original".into();
        task.reminder_date = "2026-12-01".into();
        task.checklist = vec![(true, "um".into()), (false, "dois".into())];
        task.outcome = "respondido".into();
        task.dependencies = vec![other];
        task.assignee = "Fulana".into();
        let id = store.save_task(&task).unwrap();

        let before = get_task(&store, id).unwrap();
        assert_eq!(before["dependencies"], json!([other]));
        let version = before["version"].as_i64().unwrap();
        let saved = save_task(
            &mut store,
            &json!({"id": id, "version": version, "title": "  novo título ", "priority": "alta",
                    "due_date": "2026-11-20", "notes": "uma nota"}),
        )
        .unwrap();
        assert!(saved["version"].as_i64().unwrap() > version);

        let task = find_task(&store, id).unwrap();
        assert_eq!(task.title, "novo título");
        assert_eq!(task.priority, "alta");
        assert_eq!(task.due_date, "2026-11-20");
        assert_eq!(task.notes, "uma nota");
        // Not sent: kept.
        assert_eq!(task.assignee, "Fulana");
        assert_eq!(task.action, "triar");
        assert_eq!(task.reminder_date, "2026-12-01");
        assert_eq!(task.checklist, vec![(true, "um".to_owned()), (false, "dois".to_owned())]);
        assert_eq!(task.outcome, "respondido");
        assert_eq!(task.dependencies, vec![other]);
        assert_eq!(task.document_id, Some(doc));
        assert_eq!(task.status, "pendente");

        // Clearing the due date and the responsible is a change too.
        let version = saved["version"].as_i64().unwrap();
        save_task(&mut store, &json!({"id": id, "version": version, "due_date": "", "assignee": ""})).unwrap();
        let task = find_task(&store, id).unwrap();
        assert_eq!((task.due_date.as_str(), task.assignee.as_str()), ("", ""));
        assert_eq!(task.title, "novo título");

        let shown = get_task(&store, id).unwrap();
        assert_eq!(shown["checklist"][1]["text"], json!("dois"));
        assert_eq!(shown["document_number"], json!(store.list_documents("").unwrap()[0].document_number));
    }

    #[test]
    fn save_task_refuses_a_stale_version_and_saves_nothing() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let id = add_task(&mut store, Some(doc), "original", "");
        let version = get_task(&store, id).unwrap()["version"].as_i64().unwrap();
        // Another device saves first.
        save_task(&mut store, &json!({"id": id, "version": version, "notes": "do outro aparelho"})).unwrap();
        let error = save_task(
            &mut store,
            &json!({"id": id, "version": version, "title": "meu título"}),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, TASK_CHANGED);
        let task = find_task(&store, id).unwrap();
        assert_eq!(task.title, "original");
        assert_eq!(task.notes, "do outro aparelho");
    }

    #[test]
    fn save_task_validates_title_priority_and_date() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let id = add_task(&mut store, Some(doc), "original", "");
        let v = get_task(&store, id).unwrap()["version"].as_i64().unwrap();
        assert!(save_task(&mut store, &json!({"id": id, "version": v, "title": "   "})).is_err());
        assert!(save_task(&mut store, &json!({"id": id, "version": v, "priority": "máxima"})).is_err());
        assert!(save_task(&mut store, &json!({"id": id, "version": v, "due_date": "amanhã"})).is_err());
        assert_eq!(find_task(&store, id).unwrap().title, "original");
        assert_eq!(get_task(&store, id).unwrap()["version"].as_i64().unwrap(), v);
    }

    #[test]
    fn create_task_without_document_and_with_one() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let free = create_task(&mut store, &json!({"title": " Ligar para o setor ", "action": "acompanhar"})).unwrap();
        let task = find_task(&store, free["id"].as_i64().unwrap()).unwrap();
        assert_eq!(task.document_id, None);
        assert_eq!(task.title, "Ligar para o setor");
        assert_eq!(task.action, "acompanhar");
        let zero = create_task(&mut store, &json!({"document_id": 0, "title": "outra"})).unwrap();
        let task = find_task(&store, zero["id"].as_i64().unwrap()).unwrap();
        assert_eq!(task.document_id, None);
        assert_eq!(task.action, "examinar");
        let linked = create_task(&mut store, &json!({"document_id": doc, "title": "da ficha", "due_date": "2026-11-12"})).unwrap();
        let task = find_task(&store, linked["id"].as_i64().unwrap()).unwrap();
        assert_eq!(task.document_id, Some(doc));
        assert_eq!(task.due_date, "2026-11-12");
        // The responsible person is kept (trimmed), or left empty.
        let named = create_task(&mut store, &json!({"title": "com nome", "assignee": "  Servidor A "})).unwrap();
        assert_eq!(find_task(&store, named["id"].as_i64().unwrap()).unwrap().assignee, "Servidor A");
        assert_eq!(task.assignee, "");
        assert!(create_task(&mut store, &json!({"title": ""})).is_err());
        assert!(create_task(&mut store, &json!({"title": "x", "due_date": "31/12"})).is_err());
    }

    #[test]
    fn at_risk_json_shape_for_projected_and_missing_deliveries() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _doc) = library(&dir.0);
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 19).unwrap();
        assert_eq!(at_risk(&store, today).unwrap(), json!({"items": []}));

        let mut step = ponteproj::tasks::Task::blank(None);
        step.title = "Etapa".into();
        step.scheduled_start = "2026-11-25T09:00".into();
        step.scheduled_end = "2026-11-25T18:00".into();
        let step_id = store.save_task(&step).unwrap();
        let mut late = ponteproj::tasks::Task::blank(None);
        late.title = "Entrega".into();
        late.due_date = "2026-11-25".into();
        late.estimate_minutes = 1440;
        late.dependencies = vec![step_id];
        let late_id = store.save_task(&late).unwrap();
        let mut blocked = ponteproj::tasks::Task::blank(None);
        blocked.title = "Bloqueada".into();
        blocked.due_date = "2026-12-01".into();
        blocked.estimate_minutes = 1440;
        let mut loose = ponteproj::tasks::Task::blank(None);
        loose.title = "Solta".into();
        let loose_id = store.save_task(&loose).unwrap();
        blocked.dependencies = vec![loose_id];
        let blocked_id = store.save_task(&blocked).unwrap();

        let shown = at_risk(&store, today).unwrap();
        assert_eq!(
            shown,
            json!({"items": [
                {
                    "task_id": late_id, "title": "Entrega", "due_date": "2026-11-25",
                    "kind": "late_projected", "end": "2026-11-26T18:00",
                    "cause_id": step_id, "cause_title": "Etapa"
                },
                {
                    "task_id": blocked_id, "title": "Bloqueada", "due_date": "2026-12-01",
                    "kind": "missing_plan", "end": "",
                    "cause_id": loose_id, "cause_title": "Solta"
                },
            ]})
        );
    }

    #[test]
    fn relink_task_both_ways_and_with_a_stale_version() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let id = add_task(&mut store, None, "solta", "");
        let version = |store: &Store| get_task(store, id).unwrap()["version"].as_i64().unwrap();

        let v0 = version(&store);
        relink_task(&mut store, id, doc, v0).unwrap();
        assert_eq!(find_task(&store, id).unwrap().document_id, Some(doc));
        let shown = get_task(&store, id).unwrap();
        assert_eq!(shown["document_id"], json!(doc));
        assert!(!shown["document_title"].as_str().unwrap().is_empty());

        // The old version is now stale.
        let error = relink_task(&mut store, id, 0, v0).unwrap_err().to_string();
        assert_eq!(error, TASK_CHANGED);
        assert_eq!(find_task(&store, id).unwrap().document_id, Some(doc));

        let v1 = version(&store);
        relink_task(&mut store, id, 0, v1).unwrap();
        assert_eq!(find_task(&store, id).unwrap().document_id, None);
        assert_eq!(get_task(&store, id).unwrap()["document_number"], json!(""));
        // Already without document: the core says so, nothing changes.
        let v2 = version(&store);
        assert!(relink_task(&mut store, id, 0, v2).is_err());
        assert_eq!(version(&store), v2);
    }

    #[test]
    fn delete_task_goes_to_the_trash_checks_the_version_and_can_be_restored() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let id = add_task(&mut store, Some(doc), "apagar", "");
        let other = add_task(&mut store, None, "fica", "");
        let version = |store: &Store, id: i64| get_task(store, id).unwrap()["version"].as_i64().unwrap();

        // A stale version deletes nothing (the same words as the other writes).
        let v0 = version(&store, id);
        save_task(&mut store, &json!({"id": id, "version": v0, "notes": "mudou"})).unwrap();
        let error = delete_task(&mut store, id, v0).unwrap_err().to_string();
        assert_eq!(error, TASK_CHANGED);
        assert!(find_task(&store, id).is_ok());

        let v1 = version(&store, id);
        delete_task(&mut store, id, v1).unwrap();
        assert!(find_task(&store, id).is_err(), "out of the lists");
        assert!(get_task(&store, id).is_err());
        assert!(matches!(store.task_state(id).unwrap(), ponteproj::trash::ItemState::Trashed { .. }));
        assert!(find_task(&store, other).is_ok(), "the others stay");
        // Deleting it again says it is gone; nothing else happens.
        assert!(delete_task(&mut store, id, v1).is_err());

        // "Desfazer": back with its fields, document and notes.
        let back = restore_task(&mut store, id).unwrap();
        assert_eq!(back["id"], json!(id));
        let shown = get_task(&store, id).unwrap();
        assert_eq!(shown["title"], "apagar");
        assert_eq!(shown["notes"], "mudou");
        assert_eq!(shown["document_id"], json!(doc));
        // Restoring what is active is harmless; what never existed is refused.
        assert!(restore_task(&mut store, id).is_ok());
        assert!(restore_task(&mut store, 99999).is_err());
    }

    #[test]
    fn delete_task_is_refused_while_another_task_hangs_from_it() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _doc) = library(&dir.0);
        let parent = add_task(&mut store, None, "pai", "");
        let waiting = add_task(&mut store, None, "espera", "");
        let child = add_task(&mut store, None, "filha", "");
        let version = |store: &Store, id: i64| get_task(store, id).unwrap()["version"].as_i64().unwrap();
        let v = version(&store, waiting);
        save_task(&mut store, &json!({"id": waiting, "version": v, "dependencies": [parent]})).unwrap();
        let v = version(&store, child);
        save_task(&mut store, &json!({"id": child, "version": v, "parent_task_id": parent})).unwrap();

        let vp = version(&store, parent);
        let error = delete_task(&mut store, parent, vp).unwrap_err().to_string();
        assert_eq!(error, TASK_HAS_DEPENDENTS);
        assert!(find_task(&store, parent).is_ok(), "nothing was deleted");

        // Taking the links away lets it go, like on the computer.
        let v = version(&store, waiting);
        save_task(&mut store, &json!({"id": waiting, "version": v, "dependencies": []})).unwrap();
        let v = version(&store, child);
        save_task(&mut store, &json!({"id": child, "version": v, "parent_task_id": null})).unwrap();
        let vp = version(&store, parent);
        delete_task(&mut store, parent, vp).unwrap();
        assert!(find_task(&store, parent).is_err());
    }

    #[test]
    fn a_task_deleted_on_the_phone_reaches_the_computer_and_comes_back_with_the_undo() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        let mut task = ponteproj::tasks::Task::blank(None);
        task.title = "Sair do celular".into();
        computer.save_task(&task).unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        let reporter = ponteproj::ai::Reporter::silent();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();

        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        let id = phone.list_tasks().unwrap().into_iter().find(|t| t.title == "Sair do celular").unwrap().id;
        let version = phone.item_version("task", id).unwrap().version;
        delete_task(&mut phone, id, version).unwrap();
        let sent = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(sent["written"], json!(true));
        std::fs::copy(&work, &drive).unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        assert!(
            computer.list_tasks().unwrap().iter().all(|t| t.title != "Sair do celular"),
            "the computer sees it in the Lixeira"
        );
        assert!(computer
            .trash_entries()
            .unwrap()
            .iter()
            .any(|e| e.target.kind == ponteproj::trash::Kind::Task && e.name == "Sair do celular"));

        // Undone on the phone, it comes back on the computer too.
        restore_task(&mut phone, id).unwrap();
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        std::fs::copy(&work, &drive).unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        assert!(computer.list_tasks().unwrap().iter().any(|t| t.title == "Sair do celular"));
    }

    #[test]
    fn the_situation_of_a_task_is_listed_saved_and_kept_consistent_with_the_conclusion() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _doc) = library(&dir.0);
        let id = add_task(&mut store, None, "situação", "2999-01-01");
        let version = |store: &Store| get_task(store, id).unwrap()["version"].as_i64().unwrap();

        let shown = get_task(&store, id).unwrap();
        assert_eq!((shown["status"].as_str(), shown["status_label"].as_str()), (Some("pendente"), Some("Pendente")));
        assert_eq!(shown["done"], json!(false));

        for (status, label) in [("em analise", "Em análise"), ("aguardando", "Aguardando")] {
            let v = version(&store);
            save_task(&mut store, &json!({"id": id, "version": v, "status": status})).unwrap();
            let shown = get_task(&store, id).unwrap();
            assert_eq!(shown["status"], json!(status));
            assert_eq!(shown["status_label"], json!(label));
            assert_eq!(shown["done"], json!(false), "still open");
            assert!(find_task(&store, id).unwrap().completed_at.is_none());
            // The list carries it too.
            let listed = ordered_tasks(store.list_tasks().unwrap(), &TaskNames::default(), "2026-11-20");
            assert_eq!(listed[0]["status"], json!(status));
        }

        // Concluded through the situation: done, dated by the core; and back.
        let v = version(&store);
        save_task(&mut store, &json!({"id": id, "version": v, "status": "concluida"})).unwrap();
        assert_eq!(get_task(&store, id).unwrap()["done"], json!(true));
        assert!(find_task(&store, id).unwrap().completed_at.is_some());
        let v = version(&store);
        save_task(&mut store, &json!({"id": id, "version": v, "status": "pendente"})).unwrap();
        let task = find_task(&store, id).unwrap();
        assert_eq!(task.status, "pendente");
        assert!(task.completed_at.is_none());
        assert_eq!(get_task(&store, id).unwrap()["done"], json!(false));

        // The phone's own conclude/reopen still agree with it.
        store.complete_task(id).unwrap();
        assert_eq!(get_task(&store, id).unwrap()["status"], json!("concluida"));
        store.reopen_task(id).unwrap();
        assert_eq!(get_task(&store, id).unwrap()["status"], json!("pendente"));

        // A made-up situation is refused and saves nothing; an absent or empty one leaves it alone.
        let v = version(&store);
        assert!(save_task(&mut store, &json!({"id": id, "version": v, "status": "sonhando"})).is_err());
        assert_eq!(version(&store), v);
        save_task(&mut store, &json!({"id": id, "version": v, "status": "aguardando"})).unwrap();
        let v = version(&store);
        save_task(&mut store, &json!({"id": id, "version": v, "title": "novo título", "status": ""})).unwrap();
        assert_eq!(find_task(&store, id).unwrap().status, "aguardando");
        let v = version(&store);
        save_task(&mut store, &json!({"id": id, "version": v, "notes": "x"})).unwrap();
        assert_eq!(find_task(&store, id).unwrap().status, "aguardando");
    }

    fn new_uid(store: &mut Store, name: &str, color: i64) -> String {
        create_category(store, name, color).unwrap()["category"]["uid"].as_str().unwrap().to_owned()
    }

    #[test]
    fn categories_list_create_and_reach_tasks() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        assert!(list_categories(&store).unwrap()["categories"].as_array().unwrap().is_empty());
        let made = create_category(&mut store, "  Urgente ", 4).unwrap();
        let uid = made["category"]["uid"].as_str().unwrap().to_owned();
        assert!(!uid.is_empty());
        assert_eq!(made["category"]["name"], "Urgente");
        assert_eq!(made["category"]["color"], 4);
        assert_eq!(made["category"]["archived"], false);
        // Same name ignoring case, empty name and bad colour are refused.
        assert!(create_category(&mut store, "URGENTE", 1).is_err());
        assert!(create_category(&mut store, "  ", 1).is_err());
        assert!(create_category(&mut store, "Outra", 10).is_err());
        assert!(create_category(&mut store, "Outra", -1).is_err());
        assert_eq!(list_categories(&store).unwrap()["categories"].as_array().unwrap().len(), 1);

        let created =
            create_task(&mut store, &json!({"title": "t", "document_id": doc, "categories": [uid]}))
                .unwrap();
        let id = created["id"].as_i64().unwrap();
        let got = get_task(&store, id).unwrap();
        assert_eq!(got["categories"], json!([uid]));
        let numbers = document_numbers(&store).unwrap();
        assert_eq!(task_json(&find_task(&store, id).unwrap(), &numbers)["categories"], json!([uid]));

        // Saving without the field keeps them; with it replaces them; a malformed uid is refused.
        let v = got["version"].as_i64().unwrap();
        let saved = save_task(&mut store, &json!({"id": id, "version": v, "title": "novo"})).unwrap();
        assert_eq!(get_task(&store, id).unwrap()["categories"], json!([uid]));
        let v = saved["version"].as_i64().unwrap();
        assert!(
            save_task(&mut store, &json!({"id": id, "version": v, "categories": ["inexistente"]}))
                .is_err()
        );
        let saved = save_task(&mut store, &json!({"id": id, "version": v, "categories": []})).unwrap();
        assert_eq!(get_task(&store, id).unwrap()["categories"], json!([]));
        let v = saved["version"].as_i64().unwrap();
        save_task(&mut store, &json!({"id": id, "version": v, "categories": [uid]})).unwrap();
        assert_eq!(get_task(&store, id).unwrap()["categories"], json!([uid]));
    }

    #[test]
    fn today_rows_carry_the_categories_of_the_task() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _doc) = library(&dir.0);
        let uid = new_uid(&mut store, "Prazo", 0);
        create_task(&mut store, &json!({"title": "hoje", "due_date": "2026-11-16", "categories": [uid]}))
            .unwrap();
        let lists = today_lists(&store, "2026-11-16").unwrap();
        assert_eq!(lists["today"][0]["categories"], json!([uid]));
    }

    #[test]
    fn a_task_takes_ten_categories_and_not_eleven() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _doc) = library(&dir.0);
        let uids: Vec<String> = (0..11).map(|n| new_uid(&mut store, &format!("c{n}"), 1)).collect();
        assert!(create_task(&mut store, &json!({"title": "a", "categories": uids})).is_err());
        assert!(create_task(&mut store, &json!({"title": "a", "categories": &uids[..10]})).is_ok());
    }

    #[test]
    fn document_categories_are_listed_set_and_checked_by_version() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let a = new_uid(&mut store, "A", 0);
        let b = new_uid(&mut store, "B", 1);
        let v0 = work_context_version(&store, doc);
        let set = set_document_categories(&mut store, doc, &[a.clone(), b.clone()], v0).unwrap();
        assert_eq!(set["categories"], json!([a, b]));
        let v1 = set["categories_version"].as_i64().unwrap();
        assert!(v1 > v0);
        assert_eq!(store.document_categories(doc).unwrap(), vec![a.clone(), b.clone()]);
        assert_eq!(store.list_documents("").unwrap()[0].categories, vec![a.clone(), b.clone()]);
        // A stale version saves nothing and says so in plain words.
        let error = set_document_categories(&mut store, doc, &[], v0).unwrap_err();
        assert!(format!("{error:#}").contains("foi alterado depois"));
        assert_eq!(store.document_categories(doc).unwrap().len(), 2);
        // The current version works; more than ten and unknown documents are refused.
        set_document_categories(&mut store, doc, std::slice::from_ref(&b), v1).unwrap();
        assert_eq!(store.document_categories(doc).unwrap(), vec![b.clone()]);
        let v2 = work_context_version(&store, doc);
        assert!(set_document_categories(&mut store, doc, &vec![a.clone(); 11], v2).is_err());
        assert!(set_document_categories(&mut store, 9999, &[], 0).is_err());
    }

    #[test]
    fn get_task_of_a_trashed_or_unknown_task_says_not_found() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let id = add_task(&mut store, Some(doc), "vai para a lixeira", "");
        assert!(get_task(&store, id).is_ok());
        store
            .trash_item(ponteproj::trash::Target { kind: ponteproj::trash::Kind::Task, id })
            .unwrap();
        for missing in [id, 99_999] {
            let error = get_task(&store, missing).unwrap_err().to_string();
            assert!(error.starts_with("Tarefa não encontrada"), "{error}");
        }
        assert!(save_task(&mut store, &json!({"id": id, "version": 1, "title": "x"})).is_err());
        assert!(relink_task(&mut store, id, doc, 1).is_err());
    }

    #[test]
    fn save_task_needs_a_numeric_version_and_a_valid_action() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let id = add_task(&mut store, Some(doc), "original", "");
        let v = get_task(&store, id).unwrap()["version"].as_i64().unwrap();
        assert!(save_task(&mut store, &json!({"id": id, "title": "x"})).is_err(), "no version");
        assert!(save_task(&mut store, &json!({"id": id, "version": "1", "title": "x"})).is_err(), "text version");
        assert!(save_task(&mut store, &json!({"id": id, "version": null, "title": "x"})).is_err());
        assert!(save_task(&mut store, &json!({"id": id, "version": v, "action": "voar"})).is_err());
        assert!(save_task(&mut store, &json!({"version": v, "title": "x"})).is_err(), "no id");
        assert_eq!(find_task(&store, id).unwrap().title, "original");
        assert_eq!(find_task(&store, id).unwrap().action, "triar");
        assert_eq!(get_task(&store, id).unwrap()["version"].as_i64().unwrap(), v);
    }

    #[test]
    fn relink_and_create_refuse_negative_trashed_and_unknown_documents() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let id = add_task(&mut store, Some(doc), "t", "");
        let v = get_task(&store, id).unwrap()["version"].as_i64().unwrap();
        let error = relink_task(&mut store, id, -1, v).unwrap_err().to_string();
        assert_eq!(error, "Documento inválido.");
        assert!(relink_task(&mut store, id, 99_999, v).is_err(), "unknown document");
        assert_eq!(find_task(&store, id).unwrap().document_id, Some(doc));

        let mut other = ponteproj::manual::ManualFields {
            kind: "papel".into(),
            title: "Outro".into(),
            origin: String::new(),
            document_date: String::new(),
            text: "texto".into(),
        };
        other.title = "Documento na lixeira".into();
        let trashed = store.create_manual_document(&other).unwrap().document_id;
        store
            .trash_item(ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: trashed })
            .unwrap();
        assert!(relink_task(&mut store, id, trashed, v).is_err(), "trashed document");
        assert_eq!(find_task(&store, id).unwrap().document_id, Some(doc));

        let before = store.list_tasks().unwrap().len();
        assert_eq!(
            create_task(&mut store, &json!({"document_id": -3, "title": "x"})).unwrap_err().to_string(),
            "Documento inválido."
        );
        assert!(create_task(&mut store, &json!({"document_id": 99_999, "title": "x"})).is_err());
        assert!(create_task(&mut store, &json!({"document_id": trashed, "title": "x"})).is_err());
        assert_eq!(store.list_tasks().unwrap().len(), before, "nothing was created");
    }

    #[test]
    fn today_leaves_out_tasks_of_a_document_in_the_trash() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        add_task(&mut store, Some(doc), "do documento", "2026-11-09");
        add_task(&mut store, None, "solta", "2026-11-09");
        store
            .trash_item(ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: doc })
            .unwrap();
        let lists = today_lists(&store, "2026-11-10").unwrap();
        assert_eq!(titles(&lists["late"]), ["solta"]);
    }

    #[test]
    fn a_missing_input_path_errors_cleanly_and_creates_nothing() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular");
        let missing = dir.0.join("nao-existe.ponte");
        let error = sync_paths(&mut phone, &missing.display().to_string(), "", KEY)
            .unwrap_err()
            .to_string();
        assert!(error.contains("não foi encontrado"), "{error}");
        assert!(!missing.exists());
    }

    #[test]
    fn a_failed_run_leaves_the_file_untouched_and_no_partial_output() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (_computer, drive, _id) = first_transport(&dir.0);
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let before = std::fs::read(&work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular");
        let w = work.display().to_string();
        // Wrong passphrase.
        assert!(sync_paths(&mut phone, &w, "", "outra-frase-999").is_err());
        assert_eq!(std::fs::read(&work).unwrap(), before);
        // Truncated file.
        std::fs::write(&work, &before[..before.len() - 7]).unwrap();
        assert!(sync_paths(&mut phone, &w, "", KEY).is_err());
        assert_eq!(std::fs::read(&work).unwrap().len(), before.len() - 7);
        assert!(leftovers(&dir.0).is_empty());
    }

    // -- responsible people, atendimentos, search ----------------------------

    fn add_attendance(
        store: &mut Store,
        reference: &str,
        subject: &str,
        status: &str,
        due: &str,
        assignee: &str,
    ) -> i64 {
        let mut a = ponteproj::saiop::SaiopAttendance::new_manual();
        a.reference_raw = reference.into();
        a.subject = subject.into();
        a.status = status.into();
        a.due_date = due.into();
        a.assignee = assignee.into();
        store.save_saiop_attendance(&a, None).unwrap()
    }

    fn task_of(store: &mut Store, title: &str, notes: &str, assignee: &str) -> i64 {
        let mut task = ponteproj::tasks::Task::blank(None);
        task.title = title.into();
        task.notes = notes.into();
        task.assignee = assignee.into();
        store.save_task(&task).unwrap()
    }

    fn names_of(store: &Store, limit: usize) -> Vec<String> {
        assignee_names(&store.list_tasks().unwrap(), &store.list_saiop_attendances().unwrap(), limit)
    }

    #[test]
    fn assignees_count_fold_and_order_by_use() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        // "José" four times in tasks and atendimentos (two spellings), "Ana" three
        // times, "Carla" once, a blank one and an empty one that count for nothing.
        task_of(&mut store, "a", "", "José Silva");
        task_of(&mut store, "b", "", "jose silva");
        task_of(&mut store, "b2", "", "José Silva");
        add_attendance(&mut store, "ORG-1", "x", "em_analise", "", "  José   Silva ");
        task_of(&mut store, "c", "", "Ana");
        task_of(&mut store, "c2", "", "Ana");
        add_attendance(&mut store, "ORG-2", "y", "encerrado", "", "ANA");
        task_of(&mut store, "d", "", "Carla");
        task_of(&mut store, "e", "", "   ");
        task_of(&mut store, "f", "", "");
        let names = names_of(&store, 10);
        // Most used first; the spelling kept is the most frequent one ("José Silva" twice).
        // (Importing the document made a triage task for this phone's own name.)
        assert_eq!(names, ["José Silva", "Ana", "Carla", "Celular de teste"]);
    }

    #[test]
    fn assignees_tie_goes_alphabetically_and_is_cut() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        for name in ["Zélia", "Beto", "Álvaro", "Dora"] {
            task_of(&mut store, "t", "", name);
        }
        // One name with the same weight in two spellings: the alphabetical one wins.
        task_of(&mut store, "t", "", "Ênio");
        task_of(&mut store, "t", "", "Enio");
        task_of(&mut store, "t", "", "Enio");
        task_of(&mut store, "t", "", "Ênio");
        let names = names_of(&store, 3);
        assert_eq!(names, ["Enio", "Álvaro", "Beto"], "equal counts: folded alphabetical order");
        assert_eq!(names_of(&store, 0).len(), 0);
        // The five above and the phone's own name (from the triage task of the import).
        assert_eq!(names_of(&store, 99).len(), 6);
    }

    #[test]
    fn assignees_of_a_library_without_people_are_none() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let store = open(&dir.0, "vazio.sqlite3", "Celular de teste");
        assert!(names_of(&store, 8).is_empty());
        assert!(assignee_names(&[], &[], 8).is_empty());
    }

    #[test]
    fn attendance_list_orders_open_ones_by_urgency_and_closes_the_rest() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        add_attendance(&mut store, "SEM-PRAZO", "s", "em_analise", "", "");
        add_attendance(&mut store, "FUTURO-20", "s", "em_analise", "2026-11-20", "");
        add_attendance(&mut store, "HOJE", "s", "aguardando_resposta", "2026-11-10", "");
        add_attendance(&mut store, "ATRASADO-05", "s", "em_analise", "2026-11-05", "");
        add_attendance(&mut store, "FUTURO-12", "s", "a_distribuir", "2026-11-12", "");
        add_attendance(&mut store, "ATRASADO-01", "s", "em_analise", "2026-11-01", "");
        add_attendance(&mut store, "ENCERRADO", "s", "encerrado", "2026-11-01", "");
        add_attendance(&mut store, "CANCELADO", "s", "cancelado", "", "");
        let list = attendance_rows(&store, "2026-11-10").unwrap();
        let rows = list["attendances"].as_array().unwrap();
        let refs: Vec<&str> = rows.iter().map(|r| r["reference"].as_str().unwrap()).collect();
        assert_eq!(
            &refs[..6],
            ["ATRASADO-01", "ATRASADO-05", "HOJE", "FUTURO-12", "FUTURO-20", "SEM-PRAZO"]
        );
        assert!(refs[6..].contains(&"ENCERRADO") && refs[6..].contains(&"CANCELADO"));
        // The six above and the one the import of the document made (no deadline).
        assert_eq!(list["open"], json!(7));
        assert_eq!(rows[0]["late"], json!(true));
        assert_eq!(rows[2]["due_today"], json!(true));
        assert_eq!(rows[3]["late"], json!(false));
        assert_eq!(rows[0]["due_label"], json!("01/11/2026"));
        assert_eq!(rows[0]["status_label"], json!("Em análise"));
        // A closed one is never late or due today, even with a past deadline.
        let closed = rows.iter().find(|r| r["reference"] == json!("ENCERRADO")).unwrap();
        assert_eq!((closed["open"].clone(), closed["late"].clone()), (json!(false), json!(false)));
    }

    /// An atendimento with a follow-up marked for `next` (`dd/mm/aaaa`) and an official deadline.
    fn add_chase(store: &mut Store, reference: &str, status: &str, next: &str) -> i64 {
        let mut a = ponteproj::saiop::SaiopAttendance::new_manual();
        a.reference_raw = reference.into();
        a.status = status.into();
        a.due_kind = "oficial".into();
        a.due_date = "2026-12-31".into();
        a.events.push(ponteproj::saiop::SaiopEvent {
            when: "2026-11-01T12:00:00+00:00".into(),
            actor: "Ana".into(),
            kind: "proxima_cobranca".into(),
            detail: next.into(),
        });
        store.save_saiop_attendance(&a, None).unwrap()
    }

    #[test]
    fn follow_ups_of_the_day_are_the_open_ones_due_today_or_before_most_overdue_first() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        add_chase(&mut store, "HOJE", "em_analise", "10/11/2026");
        add_chase(&mut store, "ATRASADA", "aguardando_resposta", "05/11/2026");
        add_chase(&mut store, "AMANHA", "em_analise", "11/11/2026");
        add_chase(&mut store, "ENCERRADA", "encerrado", "01/11/2026");
        // Only an official deadline, overdue: not a follow-up.
        add_attendance(&mut store, "SO-PRAZO", "s", "em_analise", "2026-11-01", "");

        let out = follow_up_rows(&store, "2026-11-10").unwrap();
        let rows = out["follow_ups"].as_array().unwrap();
        let refs: Vec<&str> = rows.iter().map(|r| r["reference"].as_str().unwrap()).collect();
        assert_eq!(refs, ["ATRASADA", "HOJE"]);
        assert_eq!(out["count"], json!(2));
        assert_eq!(rows[0]["follow_up_days"], json!(5));
        assert_eq!(rows[0]["follow_up"], json!("Cobrança marcada para 05/11/2026 (5 dias em atraso)"));
        assert_eq!(rows[1]["follow_up_days"], json!(0));
        assert_eq!(rows[1]["follow_up"], json!("Cobrança marcada para hoje"));
        // The row is the list's row: the official deadline is shown as it is.
        assert_eq!(rows[0]["due_date"], json!("2026-12-31"));
        assert_eq!(rows[0]["status_label"], json!("Aguardando resposta"));
        assert_eq!(out["channels"][0], json!("Cobrado por telefone"));
    }

    #[test]
    fn registering_and_scheduling_a_follow_up_add_history_and_keep_the_deadline() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let id = add_chase(&mut store, "ORG-1", "aguardando_resposta", "10/11/2026");
        let stored = |store: &Store| store.list_saiop_attendances().unwrap().into_iter().find(|a| a.id == id).unwrap();
        let before = stored(&store).events.len();
        let tomorrow = (chrono::Local::now().date_naive() + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
        let in_a_week = (chrono::Local::now().date_naive() + chrono::Duration::days(7)).format("%Y-%m-%d").to_string();

        register_follow_up(&mut store, &json!({"id": id, "contact": "Cobrado por e-mail: sem resposta", "next_date": tomorrow})).unwrap();
        let after = stored(&store);
        assert_eq!(after.events.len(), before + 2);
        assert_eq!(after.events[before].kind, "cobranca");
        assert_eq!(after.events[before].detail, "Cobrado por e-mail: sem resposta");
        assert_eq!(after.events[before + 1].kind, "proxima_cobranca");
        assert_eq!((after.due_date.as_str(), after.due_kind.as_str()), ("2026-12-31", "oficial"));
        assert_eq!(after.status, "aguardando_resposta");

        // No contact text: the computer's default; no date: just the contact.
        register_follow_up(&mut store, &json!({"id": id, "contact": "", "next_date": ""})).unwrap();
        let again = stored(&store);
        assert_eq!(again.events.len(), before + 3);
        assert_eq!(again.events.last().unwrap().detail, "Cobrado por telefone");

        schedule_follow_up(&mut store, &json!({"id": id, "next_date": in_a_week})).unwrap();
        let last = stored(&store);
        assert_eq!(last.events.len(), before + 4);
        assert_eq!(last.events.last().unwrap().kind, "proxima_cobranca");
        assert_eq!(last.due_date, "2026-12-31");
    }

    #[test]
    fn a_follow_up_that_cannot_be_recorded_says_why_and_records_nothing() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let id = add_chase(&mut store, "ORG-1", "em_analise", "10/11/2026");
        let closed = add_chase(&mut store, "ORG-2", "encerrado", "10/11/2026");
        let events = |store: &Store, id: i64| store.list_saiop_attendances().unwrap().into_iter().find(|a| a.id == id).unwrap().events.len();
        let (a, b) = (events(&store, id), events(&store, closed));
        let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
        let tomorrow = (chrono::Local::now().date_naive() + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
        let message = |r: Result<Value>| r.unwrap_err().to_string();

        assert!(message(register_follow_up(&mut store, &json!({"id": 0, "contact": "x"}))).contains("inválido"));
        assert!(message(register_follow_up(&mut store, &json!({"id": id, "next_date": "amanhã"}))).contains("próxima cobrança"));
        assert!(message(schedule_follow_up(&mut store, &json!({"id": id}))).contains("próxima cobrança"));
        assert!(message(schedule_follow_up(&mut store, &json!({"id": id, "next_date": today}))).contains("depois de hoje"));
        assert!(message(schedule_follow_up(&mut store, &json!({"id": closed, "next_date": tomorrow}))).contains("encerrado"));
        assert!(message(schedule_follow_up(&mut store, &json!({"id": id + 1000, "next_date": tomorrow}))).contains("não encontrado"));
        assert_eq!((events(&store, id), events(&store, closed)), (a, b));
    }

    #[test]
    fn attendance_ficha_shows_what_is_stored_with_the_desktop_labels() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let number = store.list_documents("").unwrap()[0].document_number.clone();
        let mut a = ponteproj::saiop::SaiopAttendance::new_manual();
        a.reference_raw = "ORG-OFI-2026/00431".into();
        a.subject = "Pedido de apoio".into();
        a.status = "aguardando_resposta".into();
        a.priority = "alta".into();
        a.priority_reason = "Prazo curto".into();
        a.due_date = "2026-11-09".into();
        a.due_kind = "meta_interna".into();
        a.assignee = "Servidor A".into();
        a.received_from = "Servidor B".into();
        a.observation = "Cobrar o retorno.".into();
        a.uas_return = "sim".into();
        a.sensitive = true;
        a.events = vec![
            ponteproj::saiop::SaiopEvent {
                when: "2026-11-01T12:00:00Z".into(),
                actor: "Servidor A".into(),
                kind: "entrada".into(),
                detail: "Chegou".into(),
            },
            ponteproj::saiop::SaiopEvent {
                when: "2026-11-02T12:00:00Z".into(),
                actor: "Servidor A".into(),
                kind: "anotacao".into(),
                detail: "Ligou".into(),
            },
        ];
        let id = store.save_saiop_attendance(&a, None).unwrap();
        // Linking to a document, a task and an attachment.
        let mut saved = store.list_saiop_attendances().unwrap().into_iter().find(|x| x.id == id).unwrap();
        saved.document_id = Some(doc);
        store.save_saiop_attendance(&saved, None).unwrap();
        store
            .add_attachment(ponteproj::attachments::Owner::Attendance(id), "oficio.txt", "Cópia", b"texto do anexo")
            .unwrap();

        let ficha = get_attendance(&store, id, "2026-11-10").unwrap();
        assert_eq!(ficha["reference"], json!("ORG-OFI-2026/00431"));
        assert_eq!(ficha["status_label"], json!("Aguardando resposta"));
        assert_eq!(ficha["late"], json!(true));
        let fields: Vec<(String, String)> = ficha["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["label"].as_str().unwrap().to_owned(), f["value"].as_str().unwrap().to_owned()))
            .collect();
        let get = |label: &str| fields.iter().find(|(l, _)| l == label).map(|(_, v)| v.as_str());
        assert_eq!(get("Prioridade"), Some("Alta"));
        assert_eq!(get("Motivo da prioridade"), Some("Prazo curto"));
        assert_eq!(get("Prazo"), Some("09/11/2026"));
        assert_eq!(get("Tipo de prazo"), Some("Meta interna"));
        assert_eq!(get("Responsável"), Some("Servidor A"));
        assert_eq!(get("Recebido de"), Some("Servidor B"));
        assert_eq!(get("Observações"), Some("Cobrar o retorno."));
        assert_eq!(get("Retorno para UAS?"), Some("Sim"));
        // Not informed or empty: not listed.
        assert_eq!(get("Assinado?"), None);
        assert_eq!(get("Enviado para"), None);
        assert_eq!(get("Resultado da etapa"), None);
        assert_eq!(ficha["flags"], json!(["Documento sensível"]));
        assert_eq!(ficha["document_id"], json!(doc));
        assert_eq!(ficha["document_number"], json!(number));
        // Newest event first, with the desktop's wording for its kind.
        // (The core adds events of its own when saving; ours are among them.)
        let events = ficha["events"].as_array().unwrap();
        let at = |detail: &str| events.iter().position(|e| e["detail"] == json!(detail)).unwrap();
        assert!(at("Ligou") < at("Chegou"), "the note of the 2nd is above the entry of the 1st");
        assert_eq!(events[at("Ligou")]["kind"], json!("Anotação"));
        assert_eq!(events[at("Chegou")]["kind"], json!("Entrada"));
        assert_eq!(ficha["attachments"][0]["name"], json!("oficio.txt"));
        // The attachment opens through the same export the documents use.
        let (meta, bytes) = store.attachment_bytes(ficha["attachments"][0]["id"].as_i64().unwrap()).unwrap();
        assert_eq!((meta.name.as_str(), bytes.as_slice()), ("oficio.txt", &b"texto do anexo"[..]));

        let error = get_attendance(&store, 99_999, "2026-11-10").unwrap_err().to_string();
        assert!(error.starts_with("Atendimento não encontrado"), "{error}");
    }

    fn manual(store: &mut Store, title: &str, text: &str) -> i64 {
        let fields = ponteproj::manual::ManualFields {
            kind: "papel".into(),
            title: title.into(),
            origin: String::new(),
            document_date: String::new(),
            text: text.into(),
        };
        store.create_manual_document(&fields).unwrap().document_id
    }

    fn rows_of(group: &Value, key: &str) -> Vec<String> {
        group["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r[key].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn search_finds_documents_tasks_and_atendimentos_without_caring_for_accents_or_capitals() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        manual(&mut store, "Relatório de gestão", "texto do relatório");
        task_of(&mut store, "Revisar GESTÃO de pessoal", "", "");
        task_of(&mut store, "Outra coisa", "Falar sobre a gestão do contrato", "");
        task_of(&mut store, "Ligar", "", "Gestão Central");
        task_of(&mut store, "Sem relação", "nada", "Fulano");
        add_attendance(&mut store, "ORG-OFI-2026/00431", "Apoio de gestão", "em_analise", "", "");
        add_attendance(&mut store, "ORG-OFI-2026/00500", "Outro", "encerrado", "", "Gestão");
        add_attendance(&mut store, "ORG-OFI-2026/00501", "Sem relação", "em_analise", "", "");

        let found = search_all(&store, "gestao", "2026-11-10").unwrap();
        assert_eq!(rows_of(&found["documents"], "title"), ["Relatório de gestão"]);
        let tasks = rows_of(&found["tasks"], "title");
        assert_eq!(tasks.len(), 3, "title, notes and responsible: {tasks:?}");
        assert!(!tasks.contains(&"Sem relação".to_owned()));
        assert_eq!(found["tasks"]["more"], json!(0));
        // Atendimentos by subject and by responsible; the closed one too.
        assert_eq!(found["attendances"]["rows"].as_array().unwrap().len(), 2);
        assert_eq!(found["documents"]["rows"][0]["kind"], json!("Manual"));

        // Capitals and accents in the query do not matter either.
        let loud = search_all(&store, "  GESTÃO  ", "2026-11-10").unwrap();
        assert_eq!(loud, found);
        // Every word must be present.
        let two = search_all(&store, "gestao contrato", "2026-11-10").unwrap();
        assert_eq!(rows_of(&two["tasks"], "title"), ["Outra coisa"]);
        assert!(two["attendances"]["rows"].as_array().unwrap().is_empty());
        // A reference is found by a part of it.
        let by_ref = search_all(&store, "00431", "2026-11-10").unwrap();
        assert_eq!(rows_of(&by_ref["attendances"], "reference"), ["ORG-OFI-2026/00431"]);
        // Nothing found: three empty groups.
        let none = search_all(&store, "zzzzqq", "2026-11-10").unwrap();
        for key in ["documents", "tasks", "attendances"] {
            assert_eq!(none[key], json!({"rows": [], "more": 0}));
        }
    }

    #[test]
    fn search_cuts_each_group_at_eight_and_counts_the_rest_and_puts_open_tasks_first() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let done = task_of(&mut store, "tarefa concluída 0", "", "");
        store.complete_task(done).unwrap();
        for i in 1..=10 {
            task_of(&mut store, &format!("tarefa aberta {i}"), "", "");
            add_attendance(&mut store, &format!("ORG-{i:03}"), "cota", "em_analise", "", "");
            manual(&mut store, &format!("cota {i}"), "texto");
        }
        let found = search_all(&store, "tarefa", "2026-11-10").unwrap();
        let tasks = rows_of(&found["tasks"], "title");
        assert_eq!(tasks.len(), 8);
        assert_eq!(found["tasks"]["more"], json!(3), "11 found, 8 shown");
        assert!(!tasks.contains(&"tarefa concluída 0".to_owned()), "concluded ones come last");
        let found = search_all(&store, "cota", "2026-11-10").unwrap();
        assert_eq!(found["attendances"]["rows"].as_array().unwrap().len(), 8);
        assert_eq!(found["attendances"]["more"], json!(2));
        assert_eq!(found["documents"]["rows"].as_array().unwrap().len(), 8);
        assert_eq!(found["documents"]["more"], json!(2));
    }

    #[test]
    fn search_refuses_a_short_query_and_cuts_a_huge_one() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        task_of(&mut store, "um", "", "");
        for q in ["", " ", "a", "  a  "] {
            let error = search_all(&store, q, "2026-11-10").unwrap_err().to_string();
            assert_eq!(error, "Digite ao menos 2 letras para buscar.", "{q:?}");
        }
        assert!(search_all(&store, "um", "2026-11-10").is_ok());
        // Longer than the core accepts: cut, not an error.
        assert!(search_all(&store, &"x".repeat(20_000), "2026-11-10").is_ok());
    }

    // ---- attaching a file, and records made from a file and/or a text

    fn work_file(dir: &Path, name: &str, bytes: &[u8]) -> String {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.display().to_string()
    }

    fn attachments_of(store: &Store, document: i64) -> Vec<String> {
        store
            .list_attachments(ponteproj::attachments::Owner::Document(document))
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    #[test]
    fn attach_file_stores_it_with_the_description_and_accepts_the_exact_limit() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, document) = library(&dir.0);
        let pdf = work_file(&dir.0, "w1.bin", b"%PDF-1.4 conteudo");
        let ok = attach_file(&mut store, document, &pdf, "  oficio.pdf ", "Via assinada").unwrap();
        assert_eq!(ok["name"], "oficio.pdf");
        assert_eq!(ok["size"], 17);
        let listed = store.list_attachments(ponteproj::attachments::Owner::Document(document)).unwrap();
        assert_eq!((listed[0].name.as_str(), listed[0].description.as_str()), ("oficio.pdf", "Via assinada"));
        // Exactly the limit goes in; one byte more does not.
        let max = ponteproj::attachments::MAX_ATTACHMENT_BYTES;
        let exact = work_file(&dir.0, "w2.bin", &vec![b'a'; max]);
        assert!(attach_file(&mut store, document, &exact, "limite.txt", "").is_ok());
        let over = work_file(&dir.0, "w3.bin", &vec![b'b'; max + 1]);
        let error = attach_file(&mut store, document, &over, "grande.txt", "").unwrap_err().to_string();
        assert!(error.contains("grande.txt") && error.contains("o limite é 8"), "{error}");
        assert_eq!(attachments_of(&store, document), ["oficio.pdf", "limite.txt"]);
    }

    #[test]
    fn attach_file_refuses_types_by_name_and_by_content_and_empty_files() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, document) = library(&dir.0);
        let program = work_file(&dir.0, "w1.bin", b"MZ\x90\x00 programa");
        let by_name = attach_file(&mut store, document, &program, "instalador.exe", "").unwrap_err().to_string();
        assert!(by_name.contains("Arquivos .exe não são aceitos"), "{by_name}");
        let zip = work_file(&dir.0, "w2.bin", b"PK\x03\x04 zip");
        let archive = attach_file(&mut store, document, &zip, "pasta.zip", "").unwrap_err().to_string();
        assert!(archive.contains("arquivos compactados"), "{archive}");
        // A program renamed to .pdf: the content gives it away.
        let disguised = attach_file(&mut store, document, &program, "falso.pdf", "").unwrap_err().to_string();
        assert!(disguised.contains("programa do Windows"), "{disguised}");
        let text_as_pdf = work_file(&dir.0, "w3.bin", b"isto nao e um pdf");
        let wrong = attach_file(&mut store, document, &text_as_pdf, "outro.pdf", "").unwrap_err().to_string();
        assert!(wrong.contains("não corresponde"), "{wrong}");
        let none = work_file(&dir.0, "w4.bin", b"");
        let empty = attach_file(&mut store, document, &none, "vazio.txt", "").unwrap_err().to_string();
        assert!(empty.contains("está vazio"), "{empty}");
        let no_ext = attach_file(&mut store, document, &text_as_pdf, "semextensao", "").unwrap_err().to_string();
        assert!(no_ext.contains("não tem extensão"), "{no_ext}");
        let missing = attach_file(&mut store, document, "/nao/existe", "a.txt", "").unwrap_err().to_string();
        assert!(missing.contains("não pôde ser lido"), "{missing}");
        assert!(attachments_of(&store, document).is_empty());
    }

    #[test]
    fn attach_file_stops_at_twenty_and_refuses_unknown_and_trashed_documents() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, document) = library(&dir.0);
        for n in 0..20 {
            let path = work_file(&dir.0, "w.bin", format!("conteudo {n}").as_bytes());
            attach_file(&mut store, document, &path, &format!("a{n}.txt"), "").unwrap();
        }
        let path = work_file(&dir.0, "w.bin", b"mais um");
        let error = attach_file(&mut store, document, &path, "a20.txt", "").unwrap_err().to_string();
        assert!(error.contains("já tem 20 anexos"), "{error}");
        assert_eq!(attachments_of(&store, document).len(), 20);

        assert!(attach_file(&mut store, 99_999, &path, "x.txt", "").is_err());
        let other = manual(&mut store, "Outro documento", "texto");
        store
            .trash_item(ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: other })
            .unwrap();
        assert!(attach_file(&mut store, other, &path, "x.txt", "").is_err());
        assert!(attachments_of(&store, other).is_empty());
    }

    fn fields(title: &str, text: &str) -> Value {
        json!({"kind": "outro", "title": title, "origin": "Servidor A", "date": "2026-11-06",
               "text": text, "file_name": "recibo.txt", "file_description": "Recebido de outro aplicativo"})
    }

    #[test]
    fn create_record_from_text_only_from_a_file_only_and_from_both() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let before = store.list_documents("").unwrap().len();

        let only_text = create_record(&mut store, &fields("Aviso por texto", "linha 1\nlinha 2"), "").unwrap();
        let id = only_text["document_id"].as_i64().unwrap();
        assert!(attachments_of(&store, id).is_empty());
        assert_eq!(store.load_document(id).unwrap().capture.text, "linha 1\nlinha 2");

        let file = work_file(&dir.0, "w.bin", b"conteudo do recibo");
        let only_file = create_record(&mut store, &fields("Recibo por arquivo", ""), &file).unwrap();
        let id = only_file["document_id"].as_i64().unwrap();
        assert_eq!(attachments_of(&store, id), ["recibo.txt"]);
        assert!(store.load_document(id).unwrap().capture.text.contains("Recibo por arquivo"));

        let file = work_file(&dir.0, "w.bin", b"outro conteudo");
        let both = create_record(&mut store, &fields("Recibo com legenda", "Legenda escrita"), &file).unwrap();
        let id = both["document_id"].as_i64().unwrap();
        assert_eq!(attachments_of(&store, id), ["recibo.txt"]);
        assert_eq!(store.load_document(id).unwrap().capture.text, "Legenda escrita");
        assert_eq!(store.list_documents("").unwrap().len(), before + 3);

        let nothing = create_record(&mut store, &fields("Sem nada", ""), "").unwrap_err().to_string();
        assert!(nothing.starts_with("Não há nada para guardar"), "{nothing}");
    }

    #[test]
    fn a_refused_file_leaves_no_record_behind() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let before = store.list_documents("").unwrap().len();
        let program = work_file(&dir.0, "w.bin", b"MZ programa");
        let mut bad_name = fields("Programa", "texto");
        bad_name["file_name"] = json!("setup.exe");
        assert!(create_record(&mut store, &bad_name, &program).is_err());
        let over = work_file(&dir.0, "w2.bin", &vec![b'a'; ponteproj::attachments::MAX_ATTACHMENT_BYTES + 1]);
        assert!(create_record(&mut store, &fields("Grande", ""), &over).is_err());
        let empty = work_file(&dir.0, "w3.bin", b"");
        assert!(create_record(&mut store, &fields("Vazio", ""), &empty).is_err());
        let mut long_description = fields("Descrição longa", "");
        long_description["file_description"] = json!("x".repeat(501));
        let file = work_file(&dir.0, "w4.bin", b"ok");
        assert!(create_record(&mut store, &long_description, &file).is_err());
        // An invalid title is refused by the core, also with nothing left over.
        assert!(create_record(&mut store, &fields("ab", ""), &file).is_err());
        assert_eq!(store.list_documents("").unwrap().len(), before);
    }

    /// What a failed import must not leave in the phone's library.
    fn traces(store: &Store) -> (i64, usize, usize, usize) {
        (
            store.cabinet_revision().unwrap(),
            store.list_team_events(500).unwrap().len(),
            store.list_offline_outbox().unwrap().len(),
            store.list_documents("").unwrap().len(),
        )
    }

    #[test]
    fn a_failure_after_the_record_leaves_no_record_and_nothing_queued() {
        use ponteproj::record_import::INJECT_FAILURE_AFTER_RECORD as INJECT;
        use std::sync::atomic::Ordering;
        struct Disarm;
        impl Drop for Disarm {
            fn drop(&mut self) {
                INJECT.store(false, Ordering::SeqCst);
            }
        }
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _disarm = Disarm;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let before = traces(&store);
        let file = work_file(&dir.0, "w.bin", b"conteudo do recibo");
        // The record is written, then the attachment cannot be: the phone
        // (always in shared-folder mode) used to queue the record, then try
        // to delete it, and swallow a failure of that.
        INJECT.store(true, Ordering::SeqCst);
        let error = create_record(&mut store, &fields("Recibo que falha", "texto"), &file).unwrap_err();
        assert!(!format!("{error:#}").is_empty());
        assert_eq!(traces(&store), before, "no record, no journal, no queue");
        assert!(store.trash_entries().unwrap().is_empty());
        // The failure was one-off: the same import now works.
        let made = create_record(&mut store, &fields("Recibo que falha", "texto"), &file).unwrap();
        let id = made["document_id"].as_i64().unwrap();
        assert_eq!(attachments_of(&store, id), ["recibo.txt"]);
    }

    // ---- operation ids: certainty about what a save committed ----

    const OP_A: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a01";
    const OP_B: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a02";

    fn with_operation(mut input: Value, operation: &str) -> Value {
        input["operation_id"] = json!(operation);
        input
    }

    fn status(store: &Store, operation: &str) -> Value {
        operation_status(store, &json!({ "operation_id": operation }).to_string()).unwrap()
    }

    fn number_of(store: &Store, document: i64) -> String {
        store.load_document(document).unwrap().capture.document_number
    }

    fn live(store: &Store) -> usize {
        store.list_documents("").unwrap().len()
    }

    /// The queue as another computer would get it, without clocks and ids.
    fn queue_shape(store: &Store) -> Vec<Value> {
        store
            .list_offline_outbox()
            .unwrap()
            .into_iter()
            .map(|entry| {
                let mut value = serde_json::to_value(&entry).unwrap();
                for key in ["id", "created_at", "uid"] {
                    value.as_object_mut().unwrap().remove(key);
                }
                if let Some(capture) = value["mutation"]["ImportCapture"]["capture"].as_object_mut() {
                    for key in ["request_id", "captured_at"] {
                        capture.remove(key);
                    }
                }
                // The identities of the rows are random: the same kinds and
                // slots are carried, not the same values.
                if let Some(idents) = value.get_mut("idents").and_then(|v| v.as_array_mut()) {
                    for slot in idents {
                        slot["ident"] = Value::from("…");
                    }
                }
                // So are the identities that prove the items a change names
                // (F3): the same sites, kinds and numbers, not the values.
                for list in ["mutation", "base"] {
                    if let Some(refs) = value
                        .get_mut("refs")
                        .and_then(|r| r.get_mut(list))
                        .and_then(|v| v.as_array_mut())
                    {
                        for r in refs {
                            if r["proof"].get("Ident").is_some() {
                                r["proof"]["Ident"] = Value::from("…");
                            }
                        }
                    }
                }
                value
            })
            .collect()
    }

    #[test]
    fn the_same_operation_id_twice_creates_one_record_with_a_file_and_answers_alike() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let before = live(&store);
        let file = work_file(&dir.0, "w.bin", b"conteudo do recibo");
        let input = with_operation(fields("Recibo", "texto"), OP_A);
        assert_eq!(status(&store, OP_A), json!({"done": false}));
        let first = create_record(&mut store, &input, &file).unwrap();
        let id = first["document_id"].as_i64().unwrap();
        assert_eq!(status(&store, OP_A), json!({"done": true, "document_id": id, "kind": "record"}));
        let queued = store.list_offline_outbox().unwrap().len();
        let events = store.list_team_events(500).unwrap().len();
        // Again, also with the working file gone and the text changed: nothing is made.
        std::fs::remove_file(&file).unwrap();
        let second = create_record(&mut store, &with_operation(fields("Outro título", "outro"), OP_A), &file).unwrap();
        assert_eq!(second, first);
        assert_eq!(live(&store), before + 1);
        assert_eq!(attachments_of(&store, id), ["recibo.txt"]);
        assert_eq!(store.list_offline_outbox().unwrap().len(), queued);
        assert_eq!(store.list_team_events(500).unwrap().len(), events);
        // Another id is another operation.
        let file = work_file(&dir.0, "w.bin", b"outro conteudo");
        let other = create_record(&mut store, &with_operation(fields("Recibo 2", "texto"), OP_B), &file).unwrap();
        assert_ne!(other["document_id"], first["document_id"]);
        assert_eq!(live(&store), before + 2);
    }

    #[test]
    fn the_same_operation_id_twice_creates_one_text_record_and_answers_alike() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let before = live(&store);
        let input = with_operation(fields("Aviso", "linha 1\nlinha 2"), OP_A);
        let first = create_record(&mut store, &input, "").unwrap();
        let id = first["document_id"].as_i64().unwrap();
        assert_eq!(status(&store, OP_A), json!({"done": true, "document_id": id, "kind": "record"}));
        assert_eq!(store.load_document(id).unwrap().capture.text, "linha 1\nlinha 2");
        let queued = store.list_offline_outbox().unwrap().len();
        let second = create_record(&mut store, &input, "").unwrap();
        assert_eq!(second, first);
        assert_eq!(live(&store), before + 1);
        assert_eq!(store.list_offline_outbox().unwrap().len(), queued);
    }

    #[test]
    fn the_same_operation_id_twice_attaches_once_and_answers_alike() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, document) = library(&dir.0);
        let path = work_file(&dir.0, "w.bin", b"conteudo anexado");
        let options = json!({"operation_id": OP_A}).to_string();
        let first = attach_file_with(&mut store, document, &path, "oficio.txt", "", &options).unwrap();
        assert_eq!(status(&store, OP_A), json!({"done": true, "document_id": document, "kind": "attach", "outcome": "pending"}));
        let queued = store.list_offline_outbox().unwrap().len();
        let events = store.list_team_events(500).unwrap().len();
        std::fs::remove_file(&path).unwrap();
        let second = attach_file_with(&mut store, document, &path, "oficio.txt", "", &options).unwrap();
        assert_eq!(second, first);
        assert_eq!(attachments_of(&store, document), ["oficio.txt"]);
        assert_eq!(store.list_offline_outbox().unwrap().len(), queued);
        assert_eq!(store.list_team_events(500).unwrap().len(), events);
        // The id of a record is not the id of an attachment.
        let made = create_record(&mut store, &with_operation(fields("Aviso", "texto"), OP_B), "").unwrap();
        assert!(made["document_id"].is_i64());
        let clash = json!({"operation_id": OP_B}).to_string();
        assert!(attach_file_with(&mut store, document, &path, "x.txt", "", &clash).is_err());
    }

    #[test]
    fn an_attach_operation_id_cannot_create_a_record_and_the_status_says_the_kind() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, document) = library(&dir.0);
        let path = work_file(&dir.0, "w.bin", b"conteudo anexado");
        let options = json!({"operation_id": OP_A}).to_string();
        attach_file_with(&mut store, document, &path, "oficio.txt", "", &options).unwrap();
        let made = create_record(&mut store, &with_operation(fields("Aviso", "texto"), OP_B), "").unwrap();
        let record = made["document_id"].as_i64().unwrap();
        // The exact answers of `operationStatus`.
        assert_eq!(status(&store, OP_A), json!({"done": true, "document_id": document, "kind": "attach", "outcome": "pending"}));
        assert_eq!(status(&store, OP_B), json!({"done": true, "document_id": record, "kind": "record"}));
        assert_eq!(status(&store, "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0aff"), json!({"done": false}));
        // `createRecord` with the id of the attach: an error, nothing written
        // (with a file, with a text only, and through the photo entry point).
        let before = (traces(&store), live(&store), attachments_of(&store, document));
        let file = work_file(&dir.0, "novo.bin", b"outro conteudo");
        for (path, kind) in [("", "texto"), (file.as_str(), "arquivo")] {
            let err = create_record(&mut store, &with_operation(fields("Clone", "texto"), OP_A), path).unwrap_err();
            assert!(err.to_string().contains("outra operação"), "{kind}: {err}");
        }
        assert_eq!((traces(&store), live(&store), attachments_of(&store, document)), before);
        assert_eq!(status(&store, OP_A), json!({"done": true, "document_id": document, "kind": "attach", "outcome": "pending"}));
    }

    #[test]
    fn a_failure_in_the_middle_leaves_the_operation_not_done_and_nothing_behind() {
        use ponteproj::record_import::INJECT_FAILURE_AFTER_RECORD as INJECT;
        use std::sync::atomic::Ordering;
        struct Disarm;
        impl Drop for Disarm {
            fn drop(&mut self) {
                INJECT.store(false, Ordering::SeqCst);
            }
        }
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _disarm = Disarm;
        let dir = Dir::new();
        let (mut store, document) = library(&dir.0);
        let before = traces(&store);
        let file = work_file(&dir.0, "w.bin", b"conteudo do recibo");
        // A record with a file, failing after the record was written.
        INJECT.store(true, Ordering::SeqCst);
        assert!(create_record(&mut store, &with_operation(fields("Recibo", "texto"), OP_A), &file).is_err());
        assert_eq!(status(&store, OP_A), json!({"done": false}));
        assert_eq!(traces(&store), before);
        // The retry with the same id works and is then done.
        let made = create_record(&mut store, &with_operation(fields("Recibo", "texto"), OP_A), &file).unwrap();
        assert_eq!(status(&store, OP_A)["document_id"], made["document_id"]);
        // An attachment that cannot be stored.
        let after_create = traces(&store);
        let path = work_file(&dir.0, "w2.bin", b"outro conteudo");
        let options = json!({"operation_id": OP_B}).to_string();
        // Past the twenty of the limit the core refuses it: nothing is written.
        for n in 0..20 {
            let p = work_file(&dir.0, "w3.bin", format!("conteudo {n}").as_bytes());
            attach_file(&mut store, document, &p, &format!("a{n}.txt"), "").unwrap();
        }
        let full = traces(&store);
        assert_ne!(full, after_create);
        assert!(attach_file_with(&mut store, document, &path, "a21.txt", "", &options).is_err());
        assert_eq!(status(&store, OP_B), json!({"done": false}));
        assert_eq!(traces(&store), full);
        assert_eq!(attachments_of(&store, document).len(), 20);
    }

    #[test]
    fn expected_number_protects_the_target_of_an_attachment() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let document = manual(&mut store, "Documento escolhido", "texto");
        let number = number_of(&store, document);
        assert!(number.starts_with("MANUAL-"), "{number}");
        let path = work_file(&dir.0, "w.bin", b"conteudo");
        let options = |number: &str, operation: &str| json!({"expected_number": number, "operation_id": operation}).to_string();

        // The right number: attached, with the row of the operation.
        let ok = attach_file_with(&mut store, document, &path, "a.txt", "", &options(&number, OP_A)).unwrap();
        assert!(ok["attachment_id"].is_i64());
        assert_eq!(status(&store, OP_A)["done"], json!(true));
        // Without an operation id too.
        let path2 = work_file(&dir.0, "w2.bin", b"conteudo 2");
        let only_number = json!({"expected_number": number}).to_string();
        attach_file_with(&mut store, document, &path2, "b.txt", "", &only_number).unwrap();
        assert_eq!(attachments_of(&store, document), ["a.txt", "b.txt"]);

        let nothing_written = |store: &Store, document: i64, expected: usize| {
            assert_eq!(attachments_of(store, document).len(), expected);
        };
        let before = traces(&store);
        let path3 = work_file(&dir.0, "w3.bin", b"conteudo 3");
        // Another number, a document that does not exist, a document in the trash.
        let wrong = attach_file_with(&mut store, document, &path3, "c.txt", "", &options("MANUAL-1999/0001", OP_B)).unwrap_err();
        let answer = error_answer(&wrong);
        assert_eq!(answer["ok"], json!(false));
        assert_eq!(answer["code"], json!("target_changed"));
        assert!(answer["error"].as_str().unwrap().contains("Escolha o documento de novo"), "{answer}");
        let missing = attach_file_with(&mut store, 99_999, &path3, "c.txt", "", &options(&number, OP_B)).unwrap_err();
        assert_eq!(error_answer(&missing)["code"], json!("target_changed"));
        store
            .trash_item(ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: document })
            .unwrap();
        let trashed_before = traces(&store);
        let trashed = attach_file_with(&mut store, document, &path3, "c.txt", "", &options(&number, OP_B)).unwrap_err();
        assert_eq!(error_answer(&trashed)["code"], json!("target_changed"));
        assert_eq!(traces(&store), trashed_before);
        assert_ne!(before, trashed_before, "the trash itself is a change");
        assert_eq!(status(&store, OP_B), json!({"done": false}));
        store
            .restore_item(ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: document })
            .unwrap();
        nothing_written(&store, document, 2);
        // Other errors carry no code.
        let plain = attach_file_with(&mut store, document, "/nao/existe", "d.txt", "", &options(&number, OP_B)).unwrap_err();
        assert!(error_answer(&plain).get("code").is_none());
    }

    #[test]
    fn without_the_optional_fields_everything_is_as_before() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, document) = library(&dir.0);
        // `{}`, an empty string and garbage all mean "nothing to say".
        for (n, options) in ["{}", "", "isto nao e json", "[1,2]", r#"{"operation_id":"","expected_number":""}"#].iter().enumerate() {
            let path_n = work_file(&dir.0, "w.bin", format!("conteudo {n}").as_bytes());
            let done = attach_file_with(&mut store, document, &path_n, &format!("a{n}.txt"), "", options).unwrap();
            assert!(done["attachment_id"].is_i64() && done["size"].is_u64(), "{options}: {done}");
            assert_eq!(done["name"], json!(format!("a{n}.txt")));
        }
        assert_eq!(attachments_of(&store, document).len(), 5);
        let path = work_file(&dir.0, "w.bin", b"conteudo do anexo simples");
        let plain = attach_file(&mut store, document, &path, "plano.txt", "").unwrap();
        assert!(plain["attachment_id"].is_i64());
        // A record without the field: as before, no operation is remembered.
        let made = create_record(&mut store, &fields("Sem operação", "texto"), "").unwrap();
        assert!(made["document_id"].is_i64());
        assert_eq!(status(&store, ""), json!({"done": false}));
        // The status of unknown, empty or unreadable input is "not done".
        assert_eq!(status(&store, "desconhecido"), json!({"done": false}));
        assert_eq!(operation_status(&store, "").unwrap(), json!({"done": false}));
        assert_eq!(operation_status(&store, "{}").unwrap(), json!({"done": false}));
        assert_eq!(operation_status(&store, "lixo").unwrap(), json!({"done": false}));
        // The error answer is the old one when there is no code.
        let answer = error_answer(&anyhow::anyhow!("falhou"));
        assert_eq!(answer, json!({"ok": false, "error": "falhou"}));
    }

    #[test]
    fn what_is_queued_for_an_operation_is_what_is_queued_today() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let plain_dir = Dir::new();
        let (mut plain, plain_doc) = library(&plain_dir.0);
        let plain_queue_before = plain.list_offline_outbox().unwrap().len();
        let file = work_file(&plain_dir.0, "w.bin", b"conteudo do recibo");
        create_record(&mut plain, &fields("Recibo", "texto"), &file).unwrap();
        create_record(&mut plain, &fields("Aviso", "so texto"), "").unwrap();
        attach_file(&mut plain, plain_doc, &file, "anexo.txt", "uma descricao").unwrap();
        let plain_events: Vec<String> = plain.list_team_events(500).unwrap().iter().map(|e| format!("{}|{}|{}", e.actor, e.action, e.details)).collect();

        let op_dir = Dir::new();
        let (mut with_ops, op_doc) = library(&op_dir.0);
        let file = work_file(&op_dir.0, "w.bin", b"conteudo do recibo");
        create_record(&mut with_ops, &with_operation(fields("Recibo", "texto"), OP_A), &file).unwrap();
        create_record(&mut with_ops, &with_operation(fields("Aviso", "so texto"), OP_B), "").unwrap();
        let options = json!({"operation_id": "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a03", "expected_number": number_of(&with_ops, op_doc)}).to_string();
        attach_file_with(&mut with_ops, op_doc, &file, "anexo.txt", "uma descricao", &options).unwrap();
        let op_events: Vec<String> = with_ops.list_team_events(500).unwrap().iter().map(|e| format!("{}|{}|{}", e.actor, e.action, e.details)).collect();

        assert_eq!(queue_shape(&with_ops), queue_shape(&plain));
        assert_eq!(queue_shape(&plain).len() - plain_queue_before, 4, "import + file, import, attachment");
        assert_eq!(op_events, plain_events);
    }

    #[test]
    fn the_operations_older_than_a_month_are_pruned_and_the_recent_ones_stay() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        create_record(&mut store, &with_operation(fields("Aviso", "texto"), OP_A), "").unwrap();
        assert_eq!(store.prune_operations(OPERATION_DAYS).unwrap(), 0);
        assert_eq!(status(&store, OP_A)["done"], json!(true));
        // Everything is older than "tomorrow".
        assert_eq!(store.prune_operations(-1).unwrap(), 1);
        assert_eq!(status(&store, OP_A), json!({"done": false}));
    }

    #[test]
    fn the_photo_record_keeps_working_through_the_general_function() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let jpeg = work_file(&dir.0, "w.bin", &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]);
        let mut input = json!({"kind": "papel", "title": "Recibo de entrega", "origin": "", "date": "", "text": ""});
        let made = create_photo_record(&mut store, &mut input, &jpeg).unwrap();
        let id = made["document_id"].as_i64().unwrap();
        let names = attachments_of(&store, id);
        assert!(names[0].starts_with("foto-") && names[0].ends_with(".jpg"), "{names:?}");
        assert!(store.load_document(id).unwrap().capture.text.contains("a partir de uma foto"));
    }

    #[test]
    fn the_limits_sent_to_the_app_are_the_cores() {
        let limits = limits_json();
        assert_eq!(limits["max_attachment_bytes"], 8 * 1024 * 1024);
        assert_eq!(limits["max_per_owner"], 20);
        assert_eq!(limits["max_name_chars"], 180);
        assert_eq!(limits["max_description_chars"], 500);
        assert_eq!(limits["max_text_bytes"], 500 * 1024);
        assert_eq!(limits["max_title_bytes"], 1000);
    }

    #[test]
    fn resources_and_uses_reach_the_phone_by_sync_and_list_with_their_names() {
        use ponteproj::resources::{Resource, ResourceUse, UseStatus};
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut computer, drive, doc) = first_transport(&dir.0);
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);

        let room = computer
            .save_resource(
                &Resource {
                    name: "Sala de reuniões do 2º andar".into(),
                    kind: "sala".into(),
                    location: "2º andar".into(),
                    notes: "Chave na portaria.".into(),
                    ..Resource::default()
                },
                None,
            )
            .unwrap();
        let laptops = computer
            .save_resource(
                &Resource { name: "Notebook de apresentações".into(), kind: "equipamento".into(), quantity: 2, ..Resource::default() },
                None,
            )
            .unwrap();
        let task = add_task(&mut computer, Some(doc), "Devolver o notebook", "2026-11-10");
        computer
            .save_resource_use(
                &ResourceUse {
                    resource_uid: room.clone(),
                    who: "Servidor B".into(),
                    sector: "Setor Jurídico".into(),
                    purpose: "Reunião de equipe".into(),
                    start: "2099-11-10T14:00".into(),
                    end: "2099-11-10T16:00".into(),
                    document_id: Some(doc),
                    task_id: Some(task),
                    ..ResourceUse::default()
                },
                None,
            )
            .unwrap();
        computer
            .save_resource_use(
                &ResourceUse {
                    resource_uid: laptops.clone(),
                    quantity: 1,
                    who: "Servidor A".into(),
                    start: "2020-01-02T09:00".into(),
                    end: "2020-01-02T12:00".into(),
                    status: UseStatus::InUse,
                    ..ResourceUse::default()
                },
                None,
            )
            .unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent()).unwrap();

        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work.display().to_string(), "", KEY).unwrap();

        let resources = resource_rows(&phone).unwrap();
        let names: Vec<_> = resources["resources"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Notebook de apresentações", "Sala de reuniões do 2º andar"]);
        let r = &resources["resources"][1];
        // A resource is its name and where it is: no tipo travels to the phone.
        for key in ["kind", "kind_label"] {
            assert!(r.get(key).is_none(), "{key}");
        }
        assert_eq!(r["location"], json!("2º andar"));
        assert_eq!(r["status_label"], json!("Ativo"));
        assert_eq!(r["quantity"], json!(1));
        assert_eq!(r["notes"], json!("Chave na portaria."));
        assert_eq!(resources["resources"][0]["quantity"], json!(2));

        let uses = resource_use_rows(&phone, "2026-11-10T10:00").unwrap();
        let rows = uses["uses"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by start: the old loan first.
        let loan = &rows[0];
        assert_eq!(loan["resource_name"], json!("Notebook de apresentações"));
        assert_eq!(loan["status"], json!("em_uso"));
        assert_eq!(loan["status_label"], json!("Em uso"));
        assert_eq!(loan["overdue"], json!(true));
        assert_eq!(loan["returned_at"], Value::Null);
        assert_eq!(loan["document_id"], json!(0));
        for key in ["resource_kind", "resource_kind_label"] {
            assert!(loan.get(key).is_none(), "{key}");
        }
        let booking = &rows[1];
        assert_eq!(booking["resource_name"], json!("Sala de reuniões do 2º andar"));
        assert_eq!(booking["who"], json!("Servidor B"));
        assert_eq!(booking["sector"], json!("Setor Jurídico"));
        assert_eq!(booking["start"], json!("2099-11-10T14:00"));
        assert_eq!(booking["status_label"], json!("Reservado"));
        assert_eq!(booking["overdue"], json!(false));
        let phone_doc = phone.list_documents("").unwrap().remove(0);
        assert_eq!(booking["document_id"], json!(phone_doc.id));
        assert_eq!(booking["document_number"], json!(phone_doc.document_number));
        assert_eq!(booking["task_title"], json!("Devolver o notebook"));
        assert_eq!(booking["attendance_id"], json!(0));
    }

    #[test]
    fn resource_counts_are_the_cores_day_counts_and_come_with_today() {
        use ponteproj::resources::{Resource, ResourceUse, UseStatus};
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        assert_eq!(
            resource_counts(&store, "2026-11-10", "2026-11-10T10:00").unwrap(),
            json!({"reservations_today": 0, "overdue": 0})
        );
        let room = store.save_resource(&Resource { name: "Auditório".into(), kind: "sala".into(), ..Resource::default() }, None).unwrap();
        let projector = store
            .save_resource(&Resource { name: "Projetor".into(), kind: "equipamento".into(), quantity: 3, ..Resource::default() }, None)
            .unwrap();
        let mut put = |resource: &str, start: &str, end: &str, status: UseStatus| {
            store
                .save_resource_use(
                    &ResourceUse {
                        resource_uid: resource.into(),
                        who: "Servidor A".into(),
                        start: start.into(),
                        end: end.into(),
                        status,
                        returned_at: (status == UseStatus::Returned).then(|| end.to_owned()),
                        ..ResourceUse::default()
                    },
                    None,
                )
                .unwrap();
        };
        put(&room, "2099-11-10T14:00", "2099-11-10T16:00", UseStatus::Reserved);
        put(&room, "2099-11-11T14:00", "2099-11-11T16:00", UseStatus::Reserved);
        put(&projector, "2020-11-09T09:00", "2020-11-09T12:00", UseStatus::InUse);
        put(&projector, "2020-11-08T09:00", "2020-11-08T12:00", UseStatus::Returned);
        put(&projector, "2099-11-10T09:00", "2099-11-10T10:00", UseStatus::Cancelled);
        let counts = resource_counts(&store, "2099-11-10", "2099-11-10T10:00").unwrap();
        // The old loan, still out, is overdue; the cancelled and returned ones count for nothing.
        assert_eq!(counts["overdue"], json!(1));
        let expected = ponteproj::resources::day_counts(
            &store.list_resource_uses().unwrap(),
            chrono::NaiveDate::from_ymd_opt(2099, 11, 10).unwrap(),
            "2099-11-10T10:00",
        );
        assert_eq!(counts["reservations_today"], json!(expected.reservations_today));
        assert!(counts["reservations_today"].as_u64().unwrap() >= 1);
        assert!(resource_counts(&store, "not a date", "2099-11-10T10:00").is_err());
    }

    #[test]
    fn tasks_come_ordered_by_group_with_chip_and_the_cap_on_the_concluded() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let today = "2026-11-10";
        add_task(&mut store, Some(doc), "sem prazo", "");
        add_task(&mut store, Some(doc), "depois", "2026-12-25");
        add_task(&mut store, Some(doc), "amanha", "2026-11-11");
        add_task(&mut store, Some(doc), "hoje", "2026-11-10");
        add_task(&mut store, Some(doc), "ontem", "2026-11-09");
        for n in 0..32 {
            let id = add_task(&mut store, None, &format!("feita {n}"), "");
            store.complete_task(id).unwrap();
        }
        let numbers = document_numbers(&store).unwrap();
        let rows = ordered_tasks(store.list_tasks().unwrap(), &numbers, today);
        let open: Vec<_> = rows.iter().filter(|t| t["done"] == json!(false) && t["title"] != json!("Triar documento")).collect();
        let order: Vec<_> = open.iter().map(|t| t["title"].as_str().unwrap()).collect();
        assert_eq!(order, ["ontem", "hoje", "amanha", "depois", "sem prazo"]);
        let buckets: Vec<_> = open.iter().map(|t| t["bucket"].as_str().unwrap()).collect();
        assert_eq!(buckets, ["atrasadas", "hoje", "proximas", "proximas", "sem_prazo"]);
        let chips: Vec<_> = open.iter().map(|t| t["due_chip"].as_str().unwrap()).collect();
        assert_eq!(chips, ["ontem", "hoje", "amanhã", "25/12", ""]);
        let done: Vec<_> = rows.iter().filter(|t| t["bucket"] == json!("concluidas")).collect();
        assert_eq!(done.len(), 30, "the phone's cap");
        assert!(rows.len() >= 35);
        // A late task that is reopened later is late again.
        assert_eq!(open[0]["late_if_open"], json!(true));
    }

    #[test]
    fn a_task_with_a_conclusion_date_counts_as_concluded_everywhere() {
        let mut task = ponteproj::tasks::Task::blank(None);
        task.title = "x".into();
        task.due_date = "2026-11-01".into();
        task.completed_at = Some("2026-11-02T10:00:00".into());
        let json = task_json_on(&task, &TaskNames::default(), "2026-11-10");
        assert_eq!(json["done"], json!(true));
        assert_eq!(json["overdue"], json!(false));
        assert_eq!(json["bucket"], json!("concluidas"));
    }

    #[test]
    fn today_carries_the_words_and_the_badge() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        add_task(&mut store, Some(doc), "a", "2026-11-09");
        add_task(&mut store, Some(doc), "b", "2026-11-10");
        let mut lists = today_lists(&store, "2026-11-10").unwrap();
        lists["resources"] = json!({"reservations_today": 2, "overdue": 0});
        add_today_words(&mut lists);
        assert_eq!(lists["summary"], json!("1 tarefa atrasada · 1 para hoje · 2 reservas de hoje"));
        assert_eq!(lists["notice"], json!("1 tarefa atrasada, 1 para hoje e 2 reservas de hoje."));
        assert_eq!(lists["badge"], json!(2));
        let mut none = json!({"late": [], "today": [], "attendances": []});
        add_today_words(&mut none);
        assert_eq!(none["summary"], json!("Nada vence hoje."));
        assert_eq!(none["notice"], json!(""));
    }

    #[test]
    fn the_resource_view_groups_searches_and_counts_as_the_computer_does() {
        use ponteproj::resources::{Resource, ResourceUse, UseStatus};
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut store, doc) = library(&dir.0);
        let room = store
            .save_resource(&Resource { name: "Sala de reuniões".into(), kind: "sala".into(), ..Resource::default() }, None)
            .unwrap();
        let laptops = store
            .save_resource(&Resource { name: "Notebook".into(), kind: "equipamento".into(), quantity: 2, ..Resource::default() }, None)
            .unwrap();
        let task = add_task(&mut store, None, "Devolver as chaves", "");
        let mut put = |resource: &str, who: &str, start: &str, end: &str, status: UseStatus, task_id: Option<i64>, document: Option<i64>| {
            store
                .save_resource_use(
                    &ResourceUse {
                        resource_uid: resource.into(),
                        who: who.into(),
                        start: start.into(),
                        end: end.into(),
                        status,
                        returned_at: (status == UseStatus::Returned).then(|| end.to_owned()),
                        task_id,
                        document_id: document,
                        ..ResourceUse::default()
                    },
                    None,
                )
                .unwrap();
        };
        // 10/11 at 10:00: a reservation of the morning that passed (still "Hoje"), one in the
        // afternoon, a late loan, a loan out, two closed ones.
        put(&room, "Servidor A", "2026-11-10T08:00", "2026-11-10T09:00", UseStatus::Reserved, None, None);
        put(&room, "Servidor B", "2026-11-10T14:00", "2026-11-10T16:00", UseStatus::Reserved, Some(task), Some(doc));
        put(&laptops, "Servidor C", "2026-11-08T09:00", "2026-11-09T09:00", UseStatus::InUse, None, None);
        put(&laptops, "Servidor D", "2026-11-10T09:00", "2026-11-10T18:00", UseStatus::InUse, None, None);
        put(&room, "Servidor E", "2026-11-01T09:00", "2026-11-01T10:00", UseStatus::Returned, None, None);
        put(&room, "Servidor F", "2026-11-05T09:00", "2026-11-05T10:00", UseStatus::Cancelled, None, None);
        let now = "2026-11-10T10:00";
        let view = resource_view(&store, "", now).unwrap();
        let groups = view["groups"].as_array().unwrap();
        let keys: Vec<_> = groups.iter().map(|g| g["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["atrasados", "em_uso", "hoje", "encerrados"]);
        assert_eq!(groups[3]["title"], json!("Mais antigos e encerrados"));
        assert_eq!(groups[2]["uids"].as_array().unwrap().len(), 2, "the morning reservation stays under today");
        let who_of = |uid: &Value| {
            let uses = view["uses"].as_array().unwrap();
            uses.iter().find(|u| &u["uid"] == uid).unwrap()["who"].as_str().unwrap().to_owned()
        };
        let closed: Vec<_> = groups[3]["uids"].as_array().unwrap().iter().map(who_of).collect();
        assert_eq!(closed, ["Servidor F", "Servidor E"], "closed ones by start, latest first");
        assert!(!view["uses"][0]["period"].as_str().unwrap().is_empty());
        let find = |name: &str| view["resources"].as_array().unwrap().iter().find(|r| r["name"] == json!(name)).unwrap().clone();
        // The notebook: two units, one out and late, one out until 18:00.
        let notebook = find("Notebook");
        assert_eq!(notebook["in_use_now"], json!(2));
        assert_eq!(notebook["free_now"], json!(0));
        assert_eq!(notebook["usage_line"], json!("2 de 2 em uso agora"));
        assert_eq!(notebook["free_text"], json!("0 livres"));
        assert_eq!(find("Sala de reuniões")["today"].as_array().unwrap().len(), 2);
        // The search is the union of the desktop's fields and what only the phone had.
        let by_task = resource_view(&store, "chaves", now).unwrap();
        assert_eq!(by_task["groups"].as_array().unwrap().len(), 1);
        let by_who = resource_view(&store, "  SERVIDOR   b ", now).unwrap();
        assert_eq!(by_who["groups"][0]["uids"].as_array().unwrap().len(), 1);
        assert_eq!(resource_view(&store, "zzz", now).unwrap()["groups"], json!([]));
        assert_eq!(resource_view(&store, "NOTEBOOK", now).unwrap()["rows"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn document_ids_follow_the_core_search() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (store, doc) = library(&dir.0);
        let all = document_ids(&store, "").unwrap();
        assert_eq!(all["ids"], json!([doc]));
        assert_eq!(document_ids(&store, "  zzzxqj  ").unwrap()["ids"], json!([]));
        let number = store.list_documents("").unwrap().remove(0).document_number;
        let shouted = number.to_uppercase();
        assert_eq!(document_ids(&store, &format!(" {shouted} ")).unwrap()["ids"], json!([doc]));
    }

    #[test]
    fn holidays_text_reaches_the_planning() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        // A date no other test uses (a Monday).
        let date = chrono::NaiveDate::from_ymd_opt(2031, 3, 17).unwrap();
        assert_eq!(ponteproj::holidays::holiday(date), None);
        let saved = save_holidays(&mut store, "uf SP\ndia 2031-03-17 Recesso do órgão\nlixo\n").unwrap();
        // The answer is the canonical text the app keeps as its copy.
        assert_eq!(saved["text"], "uf SP\ndia 2031-03-17 Recesso do órgão\n");
        assert_eq!(ponteproj::holidays::holiday(date), Some("Recesso do órgão"));
        assert!(!ponteproj::holidays::is_working_day(date));
        // ... and it is in the library, for every device.
        assert_eq!(
            store.library_setting("holidays").unwrap().as_deref(),
            Some("uf SP\ndia 2031-03-17 Recesso do órgão\n")
        );
        save_holidays(&mut store, "").unwrap();
        assert_eq!(ponteproj::holidays::holiday(date), None);
        assert!(ponteproj::holidays::is_working_day(date));
    }

    #[test]
    fn the_phone_adopts_the_library_holidays_and_migrates_its_own_once() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let date = chrono::NaiveDate::from_ymd_opt(2031, 3, 17).unwrap();
        // The library has none: what the phone kept goes in.
        let first = adopt_holidays(&mut store, "dia 2031-03-17 Recesso\n", "dia 2031-03-17 Recesso\n");
        assert_eq!(first["text"], "dia 2031-03-17 Recesso\n");
        assert_eq!(
            store.library_setting("holidays").unwrap().as_deref(),
            Some("dia 2031-03-17 Recesso\n")
        );
        assert_eq!(ponteproj::holidays::holiday(date), Some("Recesso"));
        assert_eq!(first["library"], json!(true));
        // Later the library wins over whatever the phone kept.
        store.set_library_setting("holidays", "uf RJ\n").unwrap();
        let second = adopt_holidays(&mut store, "dia 2031-03-17 Recesso\n", "dia 2031-03-17 Recesso\n");
        assert_eq!(second["text"], "uf RJ\n");
        assert_eq!(ponteproj::holidays::holiday(date), None);
    }

    #[test]
    fn the_phone_hand_over_never_overwrites_a_shared_value_even_a_cleared_one() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        // Another device shared "nothing, on purpose".
        store.set_library_setting("holidays", "").unwrap();
        let answer = adopt_holidays(&mut store, "uf SP\n", "uf SP\n");
        assert_eq!(answer["text"], "");
        assert_eq!(answer["library"], json!(true));
        assert_eq!(store.library_setting("holidays").unwrap().as_deref(), Some(""));
        // A library with none, and nothing offered: nothing is written.
        let other = Dir::new();
        let (mut empty, _) = library(&other.0);
        let none = adopt_holidays(&mut empty, "uf PR\n", "");
        assert_eq!(none["library"], json!(false));
        assert_eq!(empty.library_setting("holidays").unwrap(), None);
        // ... and the phone keeps using its own copy meanwhile.
        assert_eq!(none["text"], "uf PR\n");
    }

    // -- projects ---------------------------------------------------------

    fn project_uid_of(saved: &Value) -> String {
        saved["uid"].as_str().unwrap().to_owned()
    }

    fn today_of(text: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn a_project_is_created_changed_listed_and_deleted_on_the_phone() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let today = today_of("2026-11-19");
        assert!(list_projects(&store, today).unwrap()["projects"].as_array().unwrap().is_empty());

        // Creating: the core trims the name and fills the stamps.
        let saved = save_project(
            &mut store,
            &json!({"name": "  Mudança   da sede ", "objective": "Levar tudo", "assignee": "Ana",
                    "target_date": "2026-11-24"}),
        )
        .unwrap();
        let uid = project_uid_of(&saved);
        assert_eq!(uid.len(), 32);
        assert_eq!(saved["project"]["name"], "Mudança da sede");
        assert_eq!(saved["project"]["status"], "ativo");
        assert_eq!(saved["project"]["status_label"], "Ativo");
        assert_eq!(saved["project"]["target_label"], "24/11/2026");
        let v1 = saved["version"].as_i64().unwrap();
        assert!(v1 > 0 && saved["project"]["version"] == json!(v1));

        // What the core refuses.
        for bad in [
            json!({"name": "   "}),
            json!({"name": "x", "status": "feito"}),
            json!({"name": "x", "target_date": "24/11/2026"}),
            json!({"name": "x", "start_date": "2026-12-01", "target_date": "2026-11-01"}),
            json!({"name": "mudanca da sede"}),
        ] {
            assert!(save_project(&mut store, &bad).is_err(), "{bad}");
        }
        assert_eq!(store.list_projects().unwrap().len(), 1);

        // Tasks of the project: planned, projected, late and concluded.
        let plan = create_task(
            &mut store,
            &json!({"title": "Planejar", "project_uid": uid, "due_date": "2026-11-30",
                    "scheduled_start": "2026-11-23T09:00", "scheduled_end": "2026-11-24T18:00"}),
        )
        .unwrap()["id"]
            .as_i64()
            .unwrap();
        create_task(
            &mut store,
            &json!({"title": "Executar", "project_uid": uid, "due_date": "2026-11-24",
                    "dependencies": [plan], "estimate_minutes": 1440}),
        )
        .unwrap();
        create_task(&mut store, &json!({"title": "Atrasada", "project_uid": uid, "due_date": "2026-11-10"}))
            .unwrap();
        let done = create_task(&mut store, &json!({"title": "Feita", "project_uid": uid}))
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        store.complete_task(done).unwrap();
        create_task(&mut store, &json!({"title": "Avulsa"})).unwrap();

        // Reading never writes to the library.
        let revision = store.cabinet_revision().unwrap();
        let listed = list_projects(&store, today).unwrap();
        let row = &listed["projects"][0];
        assert_eq!(row["uid"], json!(uid));
        assert_eq!(row["name"], "Mudança da sede");
        assert_eq!(row["objective"], "Levar tudo");
        assert_eq!(row["assignee"], "Ana");
        assert_eq!(row["start_date"], "");
        assert_eq!(row["progress"]["total"], 4);
        assert_eq!(row["progress"]["done"], 1);
        assert_eq!(row["progress"]["late"], 1);
        assert_eq!(row["progress"]["next_due"], "2026-11-10");
        assert_eq!(row["progress"]["next_due_label"], "10/11/2026");
        assert_eq!(row["risk"]["deliveries_at_risk"], 1);
        assert_eq!(row["risk"]["target_at_risk"], "2026-11-25T18:00");

        let got = get_project(&store, &uid, today).unwrap();
        assert_eq!(store.cabinet_revision().unwrap(), revision);
        assert_eq!(got["project"]["uid"], json!(uid));
        assert_eq!(got["project"]["progress"]["total"], 4);
        assert_eq!(got["project"]["risk"]["target_at_risk"], "2026-11-25T18:00");
        let tasks = got["tasks"].as_array().unwrap();
        let titles: Vec<_> = tasks.iter().map(|t| t["title"].as_str().unwrap()).collect();
        // Open ones by due date (the undated after them), concluded last.
        assert_eq!(titles, ["Atrasada", "Executar", "Planejar", "Feita"]);
        assert!(tasks.iter().all(|t| t["project_uid"] == json!(uid)));
        assert!(tasks.iter().all(|t| t["project_name"] == "Mudança da sede"));
        assert_eq!(tasks[0]["overdue"], json!(true));
        let timeline = got["timeline"].as_array().unwrap();
        let rows: Vec<_> = timeline.iter().map(|r| r["title"].as_str().unwrap()).collect();
        assert_eq!(rows, ["Planejar", "Executar", "Atrasada", "Feita"]);
        assert_eq!(timeline[0]["start"], "2026-11-23T09:00");
        assert_eq!(timeline[0]["end"], "2026-11-24T18:00");
        assert_eq!(timeline[0]["projected"], json!(false));
        assert_eq!(timeline[0]["late"], json!(false));
        assert_eq!(timeline[0]["due"], "2026-11-30");
        assert_eq!(timeline[1]["start"], "2026-11-25T09:00");
        assert_eq!(timeline[1]["end"], "2026-11-25T18:00");
        assert_eq!(timeline[1]["projected"], json!(true));
        assert_eq!(timeline[1]["late"], json!(true));
        assert_eq!(timeline[2]["start"], Value::Null);
        assert_eq!(timeline[2]["end"], Value::Null);
        assert_eq!(timeline[2]["due"], "2026-11-10");
        assert_eq!(timeline[3]["done"], json!(true));
        assert_eq!(timeline[3]["due"], Value::Null);
        assert_eq!(got["span"]["start"], "2026-11-10T00:00");
        assert_eq!(got["span"]["end"], "2026-12-01T00:00");
        assert!(get_project(&store, &"f".repeat(32), today).is_err());

        // Changing: the version is required and a stale one saves nothing.
        assert!(save_project(&mut store, &json!({"uid": uid, "name": "Outro"})).is_err());
        let changed = save_project(
            &mut store,
            &json!({"uid": uid, "version": v1, "status": "pausado", "start_date": "2026-11-01"}),
        )
        .unwrap();
        assert_eq!(changed["project"]["status"], "pausado");
        assert_eq!(changed["project"]["status_label"], "Pausado");
        assert_eq!(changed["project"]["name"], "Mudança da sede", "other fields stay");
        assert_eq!(changed["project"]["start_date"], "2026-11-01");
        let v2 = changed["version"].as_i64().unwrap();
        assert_eq!(v2, v1 + 1);
        let stale = save_project(&mut store, &json!({"uid": uid, "version": v1, "name": "Velho"}))
            .unwrap_err()
            .to_string();
        assert_eq!(stale, PROJECT_CHANGED);
        assert_eq!(store.get_project(&uid).unwrap().unwrap().name, "Mudança da sede");
        let archived =
            save_project(&mut store, &json!({"uid": uid, "version": v2, "status": "arquivado"})).unwrap();
        assert_eq!(archived["project"]["status"], "arquivado");

        // The project of a task is set and taken off by the task functions.
        let avulsa = store.list_tasks().unwrap().into_iter().find(|t| t.title == "Avulsa").unwrap();
        let version = store.item_version("task", avulsa.id).unwrap().version;
        save_task(&mut store, &json!({"id": avulsa.id, "version": version, "project_uid": uid})).unwrap();
        assert_eq!(get_task(&store, avulsa.id).unwrap()["project_uid"], json!(uid));
        assert_eq!(get_task(&store, avulsa.id).unwrap()["project_name"], "Mudança da sede");
        let version = store.item_version("task", avulsa.id).unwrap().version;
        save_task(&mut store, &json!({"id": avulsa.id, "version": version, "notes": "x"})).unwrap();
        assert_eq!(get_task(&store, avulsa.id).unwrap()["project_uid"], json!(uid), "absent: kept");
        let version = store.item_version("task", avulsa.id).unwrap().version;
        save_task(&mut store, &json!({"id": avulsa.id, "version": version, "project_uid": ghost()})).unwrap();
        assert_eq!(get_task(&store, avulsa.id).unwrap()["project_uid"], "", "unknown: dropped");
        assert_eq!(get_task(&store, avulsa.id).unwrap()["project_name"], "");
        let version = store.item_version("task", avulsa.id).unwrap().version;
        save_task(&mut store, &json!({"id": avulsa.id, "version": version, "project_uid": uid})).unwrap();
        let version = store.item_version("task", avulsa.id).unwrap().version;
        save_task(&mut store, &json!({"id": avulsa.id, "version": version, "project_uid": null})).unwrap();
        assert_eq!(get_task(&store, avulsa.id).unwrap()["project_uid"], "");
        assert!(create_task(&mut store, &json!({"title": "Ruim", "project_uid": "abc"})).is_err());
        assert!(create_task(&mut store, &json!({"title": "Ruim", "project_uid": 5})).is_err());

        // Deleting keeps the tasks, with no project.
        let gone = delete_project(&mut store, &uid).unwrap();
        assert_eq!(gone["released"], 4);
        assert!(list_projects(&store, today).unwrap()["projects"].as_array().unwrap().is_empty());
        assert_eq!(store.list_tasks().unwrap().len(), 6, "the triage task of the document too");
        assert!(store.list_tasks().unwrap().iter().all(|t| t.project_uid.is_empty()));
        assert_eq!(get_project(&store, &uid, today).unwrap_err().to_string(), PROJECT_GONE);
        assert_eq!(delete_project(&mut store, &uid).unwrap_err().to_string(), PROJECT_GONE);
        assert_eq!(
            save_project(&mut store, &json!({"uid": uid, "version": v2, "name": "Volta"}))
                .unwrap_err()
                .to_string(),
            PROJECT_GONE
        );
    }

    fn ghost() -> &'static str {
        "0123456789abcdef0123456789abcdef"
    }

    #[test]
    fn a_new_subtask_made_on_the_phone_takes_the_project_of_its_parent() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let uid = project_uid_of(&save_project(&mut store, &json!({"name": "Mudança"})).unwrap());
        let parent = create_task(&mut store, &json!({"title": "Mãe", "project_uid": uid}))
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        let child = create_task(&mut store, &json!({"title": "Filha", "parent_task_id": parent}))
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        assert_eq!(get_task(&store, child).unwrap()["project_uid"], json!(uid));
        let other = project_uid_of(&save_project(&mut store, &json!({"name": "Outro"})).unwrap());
        let own = create_task(
            &mut store,
            &json!({"title": "Própria", "parent_task_id": parent, "project_uid": other}),
        )
        .unwrap()["id"]
            .as_i64()
            .unwrap();
        assert_eq!(get_task(&store, own).unwrap()["project_uid"], json!(other));
        // Every task row carries the two fields, empty when there is none.
        let plain = create_task(&mut store, &json!({"title": "Solta"})).unwrap()["id"].as_i64().unwrap();
        let numbers = document_numbers(&store).unwrap();
        let row = task_json(&find_task(&store, plain).unwrap(), &numbers);
        assert_eq!((row["project_uid"].as_str(), row["project_name"].as_str()), (Some(""), Some("")));
    }


    #[test]
    fn creating_a_task_says_what_an_absent_null_or_empty_project_means() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let uid = project_uid_of(&save_project(&mut store, &json!({"name": "Mudança"})).unwrap());
        let parent = create_task(&mut store, &json!({"title": "Mãe", "project_uid": uid}))
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        let project_of = |store: &Store, id: i64| get_task(store, id).unwrap()["project_uid"].clone();
        // Absent: the subtask takes the project of its parent.
        let inherited = create_task(&mut store, &json!({"title": "A", "parent_task_id": parent}))
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        assert_eq!(project_of(&store, inherited), json!(uid));
        // An explicit null or an empty text: no project at all.
        for (title, value) in [("B", json!(null)), ("C", json!("")), ("D", json!("  "))] {
            let id = create_task(
                &mut store,
                &json!({"title": title, "parent_task_id": parent, "project_uid": value}),
            )
            .unwrap()["id"]
                .as_i64()
                .unwrap();
            assert_eq!(project_of(&store, id), json!(""), "{title}");
        }
        // A project named in the request still wins, and a task with no
        // parent has none unless it names one.
        let named = create_task(
            &mut store,
            &json!({"title": "E", "parent_task_id": parent, "project_uid": uid}),
        )
        .unwrap()["id"]
            .as_i64()
            .unwrap();
        assert_eq!(project_of(&store, named), json!(uid));
        let loose = create_task(&mut store, &json!({"title": "F", "project_uid": null}))
            .unwrap()["id"]
            .as_i64()
            .unwrap();
        assert_eq!(project_of(&store, loose), json!(""));
        // A project that is not a text is a mistake.
        assert!(create_task(&mut store, &json!({"title": "G", "project_uid": 7})).is_err());
    }

    #[test]
    fn saving_a_task_with_an_empty_or_null_project_queues_an_explicit_removal() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let uid = project_uid_of(&save_project(&mut store, &json!({"name": "Mudança"})).unwrap());
        let new = |store: &mut Store, title: &str| {
            let id = create_task(store, &json!({"title": title})).unwrap()["id"].as_i64().unwrap();
            let version = store.item_version("task", id).unwrap().version;
            (id, version)
        };
        // What the queue holds for the last save of a task.
        let queued = |store: &Store, id: i64| {
            store
                .list_offline_outbox()
                .unwrap()
                .iter()
                .rev()
                .find_map(|e| match &e.mutation {
                    ponteproj::intranet::IntranetMutation::SaveTask { task, no_project, .. }
                        if task.id == id =>
                    {
                        Some((task.project_uid.clone(), *no_project))
                    }
                    _ => None,
                })
                .unwrap()
        };
        // The copy of the task has no project; the phone sends an explicit
        // empty text, an explicit null, nothing, or a project.
        let (empty, v) = new(&mut store, "vazio");
        save_task(&mut store, &json!({"id": empty, "version": v, "project_uid": ""})).unwrap();
        assert_eq!(queued(&store, empty), (String::new(), true));
        let (nulled, v) = new(&mut store, "nulo");
        save_task(&mut store, &json!({"id": nulled, "version": v, "project_uid": null})).unwrap();
        assert_eq!(queued(&store, nulled), (String::new(), true));
        let (absent, v) = new(&mut store, "ausente");
        save_task(&mut store, &json!({"id": absent, "version": v, "notes": "só isto"})).unwrap();
        assert_eq!(queued(&store, absent), (String::new(), false));
        let (named, v) = new(&mut store, "nomeado");
        save_task(&mut store, &json!({"id": named, "version": v, "project_uid": uid})).unwrap();
        assert_eq!(queued(&store, named), (uid.clone(), false));
        // Taking a task out of a project stays what it was.
        let version = store.item_version("task", named).unwrap().version;
        save_task(&mut store, &json!({"id": named, "version": version, "project_uid": ""})).unwrap();
        assert_eq!(get_task(&store, named).unwrap()["project_uid"], json!(""));
    }

    #[test]
    fn a_uid_that_is_present_but_not_a_text_is_refused_not_taken_for_a_new_project() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        for bad in [json!(7), json!(true), json!([]), json!({"a": 1})] {
            let error = save_project(&mut store, &json!({"uid": bad, "name": "Fantasma"}))
                .unwrap_err()
                .to_string();
            assert!(error.contains("inválida"), "{error}");
        }
        assert!(store.list_projects().unwrap().is_empty(), "nothing was created");
        // Absent, null and empty still mean "a new project".
        for (name, input) in [
            ("Um", json!({"name": "Um"})),
            ("Dois", json!({"uid": null, "name": "Dois"})),
            ("Três", json!({"uid": "", "name": "Três"})),
        ] {
            let saved = save_project(&mut store, &input).unwrap();
            assert_eq!(saved["project"]["name"], name);
        }
        assert_eq!(store.list_projects().unwrap().len(), 3);
    }

    #[test]
    fn a_project_with_a_due_date_at_the_end_of_the_calendar_opens_on_the_phone() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut store, _) = library(&dir.0);
        let uid = project_uid_of(&save_project(&mut store, &json!({"name": "Fim dos tempos"})).unwrap());
        for due in ["+262142-12-31", "2026-11-24"] {
            create_task(&mut store, &json!({"title": due, "project_uid": uid, "due_date": due})).unwrap();
        }
        let shown = get_project(&store, &uid, today_of("2026-11-19")).unwrap();
        assert_eq!(shown["tasks"].as_array().unwrap().len(), 2);
        assert!(shown["span"]["start"].is_string() && shown["span"]["end"].is_string());
    }
    #[test]
    fn a_transport_file_of_a_newer_format_is_refused_by_the_phone_and_left_alone() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, drive, _) = first_transport(&dir.0);
        // The transport this build writes is format 8, schema 21 (projects).
        let header = ponteproj::sharedfolder::read_header(&drive).unwrap().unwrap();
        assert_eq!((header.format, header.schema_version), (8, 21));
        assert_eq!(ponteproj::sharedfolder::FORMAT, 8);
        // What a later build writes is refused with the "update" message
        // (format 9 is read since F2b: format 10 is the later one).
        let raw = std::fs::read(&drive).unwrap();
        let newer = String::from_utf8_lossy(&raw).replacen("\"format\":8", "\"format\":10", 1);
        let work = dir.0.join("work.ponte");
        std::fs::write(&work, newer.as_bytes()).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        let error = sync_paths(&mut phone, &work.display().to_string(), "", KEY)
            .unwrap_err()
            .to_string();
        assert!(error.contains("mais nova") && error.contains("Atualize"), "{error}");
        assert_eq!(std::fs::read(&work).unwrap(), newer.as_bytes(), "nothing rewritten");
        assert!(leftovers(&dir.0).is_empty());
        assert!(phone.list_documents("").unwrap().is_empty(), "nothing adopted");
    }

    /// Everything a refused exchange must leave alone, as plain values.
    fn phone_state(phone: &Store) -> String {
        let cfg = phone.load_intranet_config().unwrap();
        format!(
            "{:?}|{}|{}|{:?}|{}|{}|{:?}",
            cfg.folder_files,
            cfg.synced_revision,
            cfg.folder_lineage,
            phone.id_marks().unwrap(),
            phone.list_offline_outbox().unwrap().len(),
            phone.cabinet_revision().unwrap(),
            phone.list_tasks().unwrap().iter().map(|t| (t.id, t.title.clone())).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_refused_transport_file_changes_nothing_on_the_phone_not_even_its_settings() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work.display().to_string(), "", KEY).unwrap();
        // A local change not yet sent moves the highest numbers past the
        // settled ID marks, which saving the settings would have reset.
        create_task(&mut phone, &json!({"title": "Só no celular"})).unwrap();

        let raw = std::fs::read(&drive).unwrap();
        let text = String::from_utf8_lossy(&raw).into_owned();
        let pending = dir.0.join("pendente.ponte");
        std::fs::write(&pending, &raw).unwrap();
        // A real copy of a later build: sealed by the core, schema 22.
        let copy = ponteproj::sharedfolder::read(&drive, KEY).unwrap().unwrap();
        let schema22 = dir.0.join("schema22.ponte");
        ponteproj::sharedfolder::write(
            &schema22,
            KEY,
            &ponteproj::sharedfolder::WriteRequest {
                lineage: &copy.header.lineage,
                revision: copy.header.revision + 1,
                writer: "outro-computador",
                writer_name: "Outro",
                schema_version: 22,
            },
            &copy.payload,
            &copy.blobs,
        )
        .unwrap();
        let header = ponteproj::sharedfolder::read_header(&schema22).unwrap().unwrap();
        assert_eq!((header.format, header.schema_version), (8, 22));
        for (what, refused) in [
            ("format 10", text.replacen("\"format\":8", "\"format\":10", 1).into_bytes()),
            ("format 8, schema 22", std::fs::read(&schema22).unwrap()),
        ] {
            assert_ne!(refused, raw, "{what}: the file differs");
            // A path this phone never used: the refusal must not store it.
            let other = dir.0.join(format!("outro-{}.ponte", what.len()));
            std::fs::write(&other, &refused).unwrap();
            let before = phone_state(&phone);
            let error = sync_paths(
                &mut phone,
                &other.display().to_string(),
                &pending.display().to_string(),
                KEY,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("mais nov"), "{what}: {error}");
            assert_eq!(phone_state(&phone), before, "{what}: the phone is untouched");
            assert_eq!(std::fs::read(&other).unwrap(), refused, "{what}: file");
            assert_eq!(std::fs::read(&pending).unwrap(), raw, "{what}: pending copy");
        }
        assert!(leftovers(&dir.0).is_empty());
    }

    #[test]
    fn a_pending_copy_with_work_the_core_could_not_keep_is_not_declared_settled() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        // The phone makes work that only reaches the pending copy (the upload
        // to Drive never happened), whose history was then summarized.
        create_task(&mut phone, &json!({"title": "Só no celular"})).unwrap();
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        let pending = dir.0.join("pendente.ponte");
        let copy = ponteproj::sharedfolder::read(&work, KEY).unwrap().unwrap();
        let mut payload = copy.payload;
        payload.journal.clear();
        ponteproj::sharedfolder::write(
            &pending,
            KEY,
            &ponteproj::sharedfolder::WriteRequest {
                lineage: &copy.header.lineage,
                revision: copy.header.revision,
                writer: &copy.header.writer,
                writer_name: &copy.header.writer_name,
                schema_version: copy.header.schema_version,
            },
            &payload,
            &copy.blobs,
        )
        .unwrap();
        let _ = std::fs::remove_file(dir.0.join("celular.sqlite3-guarda.ponte"));
        // Meanwhile the computer moves the Drive copy two revisions ahead.
        let cfg = computer.load_intranet_config().unwrap();
        let reporter = ponteproj::ai::Reporter::silent();
        for title in ["Do computador", "Outra do computador"] {
            let mut task = ponteproj::tasks::Task::blank(None);
            task.title = title.into();
            computer.save_task(&task).unwrap();
            computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        }
        std::fs::copy(&drive, &work).unwrap();
        let before = std::fs::read(&pending).unwrap();

        // The archive cannot be made (its name is taken): the pending copy
        // stays as it is and the core does not call it settled.
        let aside = ponteproj::sharedfolder::aside_path(&pending).unwrap();
        std::fs::create_dir(&aside).unwrap();
        let answer = sync_paths(&mut phone, &work_s, &pending.display().to_string(), KEY).unwrap();
        assert_eq!(answer["pending_settled"], json!(false), "{answer}");
        assert_eq!(std::fs::read(&pending).unwrap(), before, "the only copy is untouched");

        // With the archive possible, the work is kept and the copy is settled.
        std::fs::remove_dir(&aside).unwrap();
        let answer = sync_paths(&mut phone, &work_s, &pending.display().to_string(), KEY).unwrap();
        assert_eq!(answer["pending_settled"], json!(true), "{answer}");
        let kept = ponteproj::sharedfolder::read(&aside, KEY).unwrap().unwrap();
        assert!(kept.payload.snapshot.tasks.iter().any(|t| t.title == "Só no celular"));
        assert!(aside.file_name().unwrap().to_string_lossy().contains("-divergente-"));
    }

    #[test]
    // Windows files: read-only is only ever set back to writable here.
    #[allow(clippy::permissions_set_readonly_false)]
    fn a_pending_copy_that_is_the_newest_is_not_settled_while_the_working_copy_cannot_be_rewritten() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        // An edit is merged into the working copy, kept as the pending copy,
        // and then the upload fails: Drive still has the old file.
        create_task(&mut phone, &json!({"title": "Só no pendente"})).unwrap();
        let sent = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(sent["written"], json!(true));
        let pending = dir.0.join("pendente.ponte");
        std::fs::copy(&work, &pending).unwrap();
        std::fs::copy(&drive, &work).unwrap();
        let before = std::fs::read(&pending).unwrap();

        // Retry with no new edit, the working copy locked: it needs a rewrite
        // that cannot be done, so nothing reached Drive and the pending copy
        // is not settled.
        let mut permissions = std::fs::metadata(&work).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&work, permissions.clone()).unwrap();
        let answer = sync_paths(&mut phone, &work_s, &pending.display().to_string(), KEY).unwrap();
        permissions.set_readonly(false);
        std::fs::set_permissions(&work, permissions).unwrap();
        assert_eq!(answer["written"], json!(false), "{answer}");
        assert_eq!(answer["unwritten"], json!(1), "{answer}");
        assert_eq!(answer["pending_settled"], json!(false), "{answer}");
        assert_eq!(std::fs::read(&pending).unwrap(), before);

        // Writable again: the Drive copy is rewritten and the pending one settles.
        let answer = sync_paths(&mut phone, &work_s, &pending.display().to_string(), KEY).unwrap();
        assert_eq!(answer["written"], json!(true), "{answer}");
        assert_eq!(answer["pending_settled"], json!(true), "{answer}");
    }

    /// F0 (R2-B4): an exchange whose output had to be staged beside an
    /// unsettled pending copy ("pendente-novo.ponte") and never reached Drive.
    /// The next exchange gets both pending copies (one per line) and reads the
    /// staged one too: what only it holds reaches the working copy.
    #[test]
    fn the_staged_pending_copy_is_read_again_by_the_next_exchange() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        // The older pending copy: what Drive had then.
        let pending = dir.0.join("pendente.ponte");
        std::fs::copy(&drive, &pending).unwrap();
        // The phone's task goes into an output that is staged, not uploaded.
        create_task(&mut phone, &json!({"title": "Só no pendente-novo"})).unwrap();
        let sent = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(sent["written"], json!(true));
        let staged = dir.0.join("pendente-novo.ponte");
        std::fs::copy(&work, &staged).unwrap();
        // The phone's own guard copy is lost, and the computer moves Drive on.
        let _ = std::fs::remove_file(dir.0.join("celular.sqlite3-guarda.ponte"));
        let cfg = computer.load_intranet_config().unwrap();
        let mut task = ponteproj::tasks::Task::blank(None);
        task.title = "Do computador".into();
        computer.save_task(&task).unwrap();
        computer
            .sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent())
            .unwrap();
        std::fs::copy(&drive, &work).unwrap();

        let both = format!("{}\n{}", pending.display(), staged.display());
        let answer = sync_paths(&mut phone, &work_s, &both, KEY).unwrap();
        assert_eq!(answer["written"], json!(true), "{answer}");
        let out = ponteproj::sharedfolder::read(&work, KEY).unwrap().unwrap();
        let titles: Vec<&str> = out
            .payload
            .snapshot
            .tasks
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert!(titles.contains(&"Só no pendente-novo"), "{titles:?}");
        assert!(titles.contains(&"Do computador"), "{titles:?}");
        assert_eq!(answer["pending_settled"], json!(true), "{answer}");
    }

    /// Review 1: a staged pending copy, first published by a forked library
    /// (same lineage, empty journal, nothing applied), holds standalone task
    /// X with the same row number and title as another task of the Drive
    /// copy. Drive has the same revision, a later write and one applied
    /// change. A new phone joins with both. The staged copy may be called
    /// settled only if X is proved to be somewhere: in the result, or in a
    /// copy kept aside.
    #[test]
    fn a_staged_copy_is_settled_only_when_what_it_holds_is_proved_kept() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        let reporter = ponteproj::ai::Reporter::silent();
        let cfg = computer.load_intranet_config().unwrap();
        let mut task = ponteproj::tasks::Task::blank(None);
        task.title = "Do computador".into();
        computer.save_task(&task).unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        let shared = ponteproj::sharedfolder::read(&drive, KEY).unwrap().unwrap();
        assert!(!shared.payload.applied.is_empty());
        let (number, title) = {
            let first = &shared.payload.snapshot.tasks[0];
            (first.id, first.title.clone())
        };
        // The forked library: one standalone task X, same number and title as
        // the Drive copy's first task, but another task (its own uid).
        let mut forked = open(&dir.0, "bifurcado.sqlite3", "Bifurcado");
        let mut x = ponteproj::tasks::Task::blank(None);
        x.title = title.clone();
        let x_id = forked.save_task(&x).unwrap();
        assert_eq!(x_id, number, "same row number");
        let x_uid = forked
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == x_id)
            .unwrap()
            .uid;
        let mut snapshot = forked.export_snapshot().unwrap();
        snapshot.revision = shared.header.revision;
        let staged = dir.0.join("pendente-novo.ponte");
        fn request<'a>(
            header: &'a ponteproj::sharedfolder::Header,
            writer: &'a str,
            writer_name: &'a str,
        ) -> ponteproj::sharedfolder::WriteRequest<'a> {
            ponteproj::sharedfolder::WriteRequest {
                lineage: &header.lineage,
                revision: header.revision,
                writer,
                writer_name,
                schema_version: header.schema_version,
            }
        }
        ponteproj::sharedfolder::write(
            &staged,
            KEY,
            &request(&shared.header, "bifurcado", "Bifurcado"),
            &ponteproj::sharedfolder::Payload {
                snapshot,
                applied: Vec::new(),
                journal: Vec::new(),
                refs_since: None,
            },
            &Default::default(),
        )
        .unwrap();
        // Drive is written again (same content and revision), later.
        std::thread::sleep(std::time::Duration::from_millis(20));
        ponteproj::sharedfolder::write(
            &drive,
            KEY,
            &request(
                &shared.header,
                &shared.header.writer,
                &shared.header.writer_name,
            ),
            &shared.payload,
            &shared.blobs,
        )
        .unwrap();
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        let answer = sync_paths(
            &mut phone,
            &work.display().to_string(),
            &staged.display().to_string(),
            KEY,
        )
        .unwrap();
        let has_x = |path: &std::path::Path| {
            ponteproj::sharedfolder::read(path, KEY)
                .ok()
                .flatten()
                .is_some_and(|c| c.payload.snapshot.tasks.iter().any(|t| t.uid == x_uid))
        };
        let kept_aside = std::fs::read_dir(&dir.0).unwrap().any(|e| {
            let path = e.unwrap().path();
            path.to_string_lossy().contains("-divergente-") && has_x(&path)
        });
        if answer["pending_settled"] == json!(true) {
            assert!(
                has_x(&work) || kept_aside,
                "settled although X is nowhere: {answer}"
            );
        }
        assert!(has_x(&work) || has_x(&staged) || kept_aside, "X lost: {answer}");
        assert!(
            answer["notes"].to_string().contains(&title) || has_x(&work),
            "the person is told: {answer}"
        );
    }

    /// Review 9: when the exchange keeps a pending copy aside, the answer
    /// to the app carries the note that says so and names the copy kept.
    #[test]
    fn the_notes_of_an_exchange_reach_the_phone() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        // The phone makes work that only reaches the pending copy (the upload
        // to Drive never happened), whose history was then summarized.
        create_task(&mut phone, &json!({"title": "Só no celular"})).unwrap();
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        let pending = dir.0.join("pendente.ponte");
        let copy = ponteproj::sharedfolder::read(&work, KEY).unwrap().unwrap();
        let mut payload = copy.payload;
        payload.journal.clear();
        ponteproj::sharedfolder::write(
            &pending,
            KEY,
            &ponteproj::sharedfolder::WriteRequest {
                lineage: &copy.header.lineage,
                revision: copy.header.revision,
                writer: &copy.header.writer,
                writer_name: &copy.header.writer_name,
                schema_version: copy.header.schema_version,
            },
            &payload,
            &copy.blobs,
        )
        .unwrap();
        let _ = std::fs::remove_file(dir.0.join("celular.sqlite3-guarda.ponte"));
        // Meanwhile the computer moves the Drive copy two revisions ahead.
        let cfg = computer.load_intranet_config().unwrap();
        let reporter = ponteproj::ai::Reporter::silent();
        for title in ["Do computador", "Outra do computador"] {
            let mut task = ponteproj::tasks::Task::blank(None);
            task.title = title.into();
            computer.save_task(&task).unwrap();
            computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        }
        std::fs::copy(&drive, &work).unwrap();
        // The archive is named after what the pending copy holds now.
        let aside = ponteproj::sharedfolder::aside_path(&pending).unwrap();

        let answer = sync_paths(&mut phone, &work_s, &pending.display().to_string(), KEY).unwrap();
        assert!(aside.is_file(), "{answer}");
        let name = aside.file_name().unwrap().to_string_lossy().into_owned();
        assert!(answer["notes"].to_string().contains(&name), "{answer}");
    }

    /// Review 2.3: two pending copies, one that must be kept aside and
    /// cannot be (its archive name is taken), and a staged one whose content
    /// the result holds. Only the blocked one is reported unsettled.
    #[test]
    fn each_pending_copy_is_settled_on_its_own() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        // The phone makes work that only reaches the pending copy (the upload
        // to Drive never happened), whose history was then summarized.
        create_task(&mut phone, &json!({"title": "Só no celular"})).unwrap();
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        let pending = dir.0.join("pendente.ponte");
        let copy = ponteproj::sharedfolder::read(&work, KEY).unwrap().unwrap();
        let mut payload = copy.payload;
        payload.journal.clear();
        ponteproj::sharedfolder::write(
            &pending,
            KEY,
            &ponteproj::sharedfolder::WriteRequest {
                lineage: &copy.header.lineage,
                revision: copy.header.revision,
                writer: &copy.header.writer,
                writer_name: &copy.header.writer_name,
                schema_version: copy.header.schema_version,
            },
            &payload,
            &copy.blobs,
        )
        .unwrap();
        let _ = std::fs::remove_file(dir.0.join("celular.sqlite3-guarda.ponte"));
        // Meanwhile the computer moves the Drive copy two revisions ahead.
        let cfg = computer.load_intranet_config().unwrap();
        let reporter = ponteproj::ai::Reporter::silent();
        for title in ["Do computador", "Outra do computador"] {
            let mut task = ponteproj::tasks::Task::blank(None);
            task.title = title.into();
            computer.save_task(&task).unwrap();
            computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        }
        std::fs::copy(&drive, &work).unwrap();
        let before = std::fs::read(&pending).unwrap();

        let staged = dir.0.join("pendente-novo.ponte");
        std::fs::copy(&drive, &staged).unwrap();
        let aside = ponteproj::sharedfolder::aside_path(&pending).unwrap();
        std::fs::create_dir(&aside).unwrap();
        let both = format!("{}\n{}", pending.display(), staged.display());
        let answer = sync_paths(&mut phone, &work_s, &both, KEY).unwrap();
        let unsettled: Vec<String> = answer["unsettled"]
            .as_array()
            .unwrap_or_else(|| panic!("no per-copy verdict: {answer}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        assert!(unsettled.contains(&pending.display().to_string()), "{answer}");
        assert!(!unsettled.contains(&staged.display().to_string()), "{answer}");
        assert_eq!(std::fs::read(&pending).unwrap(), before, "the blocked copy is untouched");
        std::fs::remove_dir(&aside).unwrap();
    }

    #[test]
    fn the_projects_of_the_computer_reach_the_phone_and_the_phone_changes_reach_back() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, _) = first_transport(&dir.0);
        let project = computer
            .save_project(&ponteproj::projects::Project::new("Reforma"))
            .unwrap();
        let mut task = ponteproj::tasks::Task::blank(None);
        task.title = "Pintar".into();
        task.project_uid = project.uid.clone();
        computer.save_task(&task).unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        let reporter = ponteproj::ai::Reporter::silent();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();

        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        let today = today_of("2026-11-19");
        let listed = list_projects(&phone, today).unwrap();
        assert_eq!(listed["projects"][0]["name"], "Reforma");
        assert_eq!(listed["projects"][0]["progress"]["total"], 1);
        let shown = get_project(&phone, &project.uid, today).unwrap();
        assert_eq!(shown["tasks"][0]["title"], "Pintar");
        assert_eq!(shown["tasks"].as_array().unwrap().len(), 1);

        // The phone makes a project and files the task in it; the computer
        // gets both, in order.
        let made = save_project(&mut phone, &json!({"name": "Do celular"})).unwrap();
        let uid = project_uid_of(&made);
        let id = phone.list_tasks().unwrap().into_iter().find(|t| t.title == "Pintar").unwrap().id;
        let version = phone.item_version("task", id).unwrap().version;
        save_task(&mut phone, &json!({"id": id, "version": version, "project_uid": uid})).unwrap();
        let sent = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(sent["written"], json!(true));
        std::fs::copy(&work, &drive).unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        assert_eq!(computer.list_projects().unwrap().len(), 2);
        let pintar = computer.list_tasks().unwrap().into_iter().find(|t| t.title == "Pintar").unwrap();
        assert_eq!(pintar.project_uid, uid);
        // The computer deletes the first project: its task stays on the phone.
        computer.delete_project(&project.uid).unwrap();
        computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        std::fs::copy(&drive, &work).unwrap();
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(phone.list_projects().unwrap().len(), 1);
        assert_eq!(phone.list_tasks().unwrap().len(), 2, "the tasks stay");
    }

    /// F2a: a phone library of schema 21 (Ponte de bolso 0.27), with a note
    /// still queued, is moved to schema 22 by the core at opening (every row
    /// gets its identity, the copy is kept beside it) and keeps exchanging
    /// the same format-8 file with a computer.
    #[test]
    fn a_phone_library_of_schema_21_migrates_to_22_and_keeps_syncing() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        let (mut computer, drive, id) = first_transport(&dir.0);
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let work_s = work.display().to_string();
        let path = dir.0.join("celular.sqlite3");
        {
            let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
            sync_paths(&mut phone, &work_s, "", KEY).unwrap();
            phone.save_note(id, "Nota de antes da atualização").unwrap();
        }
        ponteproj::idents::test_support::downgrade_to_21(&path).unwrap();
        assert_eq!(
            ponteproj::idents::test_support::user_version(&path).unwrap(),
            21
        );

        let mut phone = Store::open(&path).unwrap();
        let (version, problems, idents) =
            ponteproj::idents::test_support::check_library(&path).unwrap();
        assert_eq!(version, 22);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(idents > 0);
        let copies: Vec<String> = std::fs::read_dir(dir.0.join("backups"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("antes-do-acervo-22-") && n.ends_with(".sqlite3"))
            .collect();
        assert_eq!(copies.len(), 1, "{copies:?}");

        // The queued note still goes, in the same file format.
        let answer = sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        assert_eq!(answer["written"], json!(true));
        let header = ponteproj::sharedfolder::read_header(&work)
            .unwrap()
            .unwrap();
        assert_eq!((header.format, header.schema_version), (8, 21));
        std::fs::copy(&work, &drive).unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        computer
            .sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent())
            .unwrap();
        assert_eq!(
            computer.load_document(id).unwrap().note,
            "Nota de antes da atualização"
        );
        let (_, problems, _) = ponteproj::idents::test_support::check_library(&path).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
    }

    // ---------------------------------------------------------- F2b --

    fn promote_on() {
        ponteproj::idents::hooks::PROMOTION.with(|c| c.set(true));
    }

    fn phone_idents(store: &Store) -> std::collections::BTreeMap<(String, i64), String> {
        ponteproj::idents::test_support::all_idents(store)
    }

    fn triples_of(path: &Path) -> std::collections::BTreeMap<(String, i64), String> {
        match ponteproj::sharedfolder::read_any(path, KEY).unwrap() {
            Some(ponteproj::sharedfolder::Found::Copy(copy)) => copy
                .payload
                .snapshot
                .idents
                .iter()
                .map(|t| ((t.0.clone(), t.1), t.2.clone()))
                .collect(),
            _ => panic!("not a copy: {}", path.display()),
        }
    }

    /// The computer promotes the folder; the phone had joined it at format 8
    /// and made a task meanwhile. Returns (computer, drive, phone, the 9).
    fn promoted_with_phone(dir: &Path) -> (Store, PathBuf, Store, PathBuf) {
        let (mut computer, drive, _) = first_transport(dir);
        let work = dir.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(dir, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work.display().to_string(), "", KEY).unwrap();
        create_task(&mut phone, &json!({"title": "Tarefa do celular"})).unwrap();
        promote_on();
        let cfg = computer.load_intranet_config().unwrap();
        let report = computer
            .sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent())
            .unwrap();
        assert!(report.promoted, "{:?}", report.promotion_wait);
        let nine = ponteproj::sharedfolder::nine_path(&drive);
        assert!(nine.is_file());
        (computer, drive, phone, nine)
    }

    #[test]
    fn f2b_the_phone_refuses_the_stub_before_any_local_write() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, drive, mut phone, _nine) = promoted_with_phone(&dir.0);
        let lineage = phone.load_intranet_config().unwrap().folder_lineage;
        // The app copied the Drive file it uses, now a stub.
        let work = dir.0.join("work-stub.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let stub = std::fs::read(&work).unwrap();
        let before = phone_state(&phone);
        let error = sync_paths(&mut phone, &work.display().to_string(), "", KEY)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("O arquivo da pasta foi atualizado. Escolha em Ajustes o arquivo ponte-sync-formato9.ponte, na mesma pasta do Drive."),
            "{error}"
        );
        assert_eq!(phone_state(&phone), before, "nothing changed on the phone");
        assert_eq!(std::fs::read(&work).unwrap(), stub, "the stub is left alone");
        assert_eq!(phone.floor(&lineage).unwrap().format, 9, "floor 9");
        assert!(leftovers(&dir.0).is_empty());
    }

    #[test]
    fn f2b_the_phone_chooses_the_new_file_once() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, drive, mut phone, nine) = promoted_with_phone(&dir.0);
        let stub_work = dir.0.join("work-stub.ponte");
        std::fs::copy(&drive, &stub_work).unwrap();
        assert!(sync_paths(&mut phone, &stub_work.display().to_string(), "", KEY).is_err());
        // The person picks ponte-sync-formato9.ponte: the app copies it.
        let work = dir.0.join("work.ponte");
        std::fs::copy(&nine, &work).unwrap();
        let answer = sync_paths(&mut phone, &work.display().to_string(), "", KEY).unwrap();
        assert_eq!(answer["written"], json!(true), "{answer}");
        assert_eq!(answer["pending_settled"], json!(true), "{answer}");
        let header = ponteproj::sharedfolder::read_header(&work).unwrap().unwrap();
        assert_eq!(header.format, 9, "the phone writes the 9");
        assert_eq!(phone_idents(&phone), triples_of(&work));
        assert_eq!(phone.list_offline_outbox().unwrap().len(), 0);
        // Uploaded to the new file; the computer takes the phone's task.
        std::fs::copy(&work, &nine).unwrap();
        let cfg = computer.load_intranet_config().unwrap();
        computer
            .sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent())
            .unwrap();
        assert!(computer
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Tarefa do celular"));
        // The next exchange of the phone uses the same file again.
        std::fs::copy(&nine, &work).unwrap();
        sync_paths(&mut phone, &work.display().to_string(), "", KEY).unwrap();
        assert_eq!(phone_idents(&phone), triples_of(&nine));
        let _ = drive;
    }

    #[test]
    fn f2b_b23_phone_8_pendings_are_kept_after_the_floor() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, drive, mut phone, nine) = promoted_with_phone(&dir.0);
        // Two pending copies of format 8, written before the floor: one
        // holds a task alone (self-contained), the other an attachment to a
        // document that exists (it may be another item now).
        let base = match ponteproj::sharedfolder::read_any(&nine, KEY).unwrap() {
            Some(ponteproj::sharedfolder::Found::Copy(copy)) => copy,
            _ => panic!(),
        };
        let doc = base.payload.snapshot.documents[0].id;
        let write_pending = |path: &Path, uid: &str, mutation: ponteproj::intranet::IntranetMutation, created: Vec<ponteproj::idmap::CreatedId>| {
            let mut payload = ponteproj::sharedfolder::Payload {
                snapshot: base.payload.snapshot.clone(),
                applied: base.payload.applied.clone(),
                journal: base.payload.journal.clone(),
                refs_since: None,
            };
            payload.snapshot.idents.clear();
            payload.snapshot.ident_epoch.clear();
            for e in payload.journal.iter_mut() {
                e.idents.clear();
            }
            payload.applied.push(uid.into());
            payload.journal.push(ponteproj::sharedfolder::JournalEntry {
                uid: uid.into(),
                created_at: "2026-11-20T12:00:00Z".into(),
                actor: "Celular de teste".into(),
                writer: "celular".into(),
                label: "Alteração do celular".into(),
                mutation,
                base: None,
                revision: base.header.revision + 1,
                created,
                idents: Vec::new(),
                refs: None,
                legacy: false,
            });
            ponteproj::sharedfolder::write(
                path,
                KEY,
                &ponteproj::sharedfolder::WriteRequest {
                    lineage: &base.header.lineage,
                    revision: base.header.revision + 1,
                    writer: "celular",
                    writer_name: "Celular",
                    schema_version: 21,
                },
                &payload,
                &base.blobs,
            )
            .unwrap();
        };
        let mut task = ponteproj::tasks::Task::standalone();
        task.title = "Tarefa só na pendente".into();
        let pending = dir.0.join("pendente.ponte");
        write_pending(
            &pending,
            "pendente-tarefa",
            ponteproj::intranet::IntranetMutation::SaveTask {
                task,
                expected_version: None,
                no_project: false,
            },
            vec![ponteproj::idmap::CreatedId {
                kind: ponteproj::idmap::IdKind::Task,
                slot: String::new(),
                id: 501,
            }],
        );
        let staged = dir.0.join("pendente-novo.ponte");
        write_pending(
            &staged,
            "pendente-anexo",
            ponteproj::intranet::IntranetMutation::AddAttachment {
                owner_kind: "document".into(),
                owner_id: doc,
                name: "nota.txt".into(),
                description: "Anexo do celular antigo".into(),
                data_base64: "dGV4dG8=".into(),
            },
            vec![ponteproj::idmap::CreatedId {
                kind: ponteproj::idmap::IdKind::Attachment,
                slot: String::new(),
                id: 502,
            }],
        );
        let before = (std::fs::read(&pending).unwrap(), std::fs::read(&staged).unwrap());
        let _ = drive;
        let work = dir.0.join("work.ponte");
        std::fs::copy(&nine, &work).unwrap();
        let pendings = format!("{}\n{}", pending.display(), staged.display());
        let answer = sync_paths(&mut phone, &work.display().to_string(), &pendings, KEY).unwrap();
        // Review F2B-09: each old pending copy is kept aside, verified, and
        // settled on its own (the app lets it go), never left in the list.
        assert_eq!(answer["pending_settled"], json!(true), "{answer}");
        assert_eq!(
            ponteproj::sharedfolder::read_header(&work).unwrap().unwrap().format,
            9,
            "the write target is the 9 work file"
        );
        assert_eq!(
            (std::fs::read(&pending).unwrap(), std::fs::read(&staged).unwrap()),
            before,
            "the pending copies are left as they are for the app"
        );
        for kept in [&before.0, &before.1] {
            assert!(
                std::fs::read_dir(&dir.0)
                    .unwrap()
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .any(|p| p.to_string_lossy().contains("-divergente-")
                        && std::fs::read(&p).unwrap() == *kept),
                "each one is kept aside, byte for byte"
            );
        }
        assert!(phone
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Tarefa só na pendente"));
        assert!(phone
            .list_sync_conflicts()
            .unwrap()
            .iter()
            .any(|c| c.reason.contains("versão anterior do Ponte")));
    }

    /// Review F2B-09: pending copies that pile up (uploads that keep
    /// failing) never stop the exchanges: copies with the same bytes are read
    /// once, at most six are read (the oldest) and the others are kept aside
    /// with a notice.
    #[test]
    fn f2b_r09_many_pending_copies_never_stop_the_phone() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work.display().to_string(), "", KEY).unwrap();
        let copy = ponteproj::sharedfolder::read(&work, KEY).unwrap().unwrap();
        let mut names = Vec::new();
        for n in 0..9 {
            let path = dir.0.join(format!("pendente-novo-{n}.ponte"));
            ponteproj::sharedfolder::write(
                &path,
                KEY,
                &ponteproj::sharedfolder::WriteRequest {
                    lineage: &copy.header.lineage,
                    revision: copy.header.revision,
                    writer: &copy.header.writer,
                    writer_name: &copy.header.writer_name,
                    schema_version: copy.header.schema_version,
                },
                &copy.payload,
                &copy.blobs,
            )
            .unwrap();
            names.push(path.display().to_string());
        }
        let twin = dir.0.join("pendente.ponte");
        std::fs::copy(&names[0], &twin).unwrap();
        names.push(twin.display().to_string());
        let answer = sync_paths(&mut phone, &work.display().to_string(), &names.join("
"), KEY)
            .expect("the exchange goes on");
        let notes = answer["notes"].as_array().unwrap();
        assert!(
            notes
                .iter()
                .any(|n| n.as_str().unwrap_or("").contains("esperam as próximas sincronizações")),
            "{answer}"
        );
        // Nothing is set aside or removed: the copies not read wait,
        // unsettled, for the next exchanges.
        assert!(names.iter().all(|n| std::path::Path::new(n.as_str()).exists()));
        let unsettled = answer["unsettled"].as_array().unwrap();
        assert!(unsettled.len() >= 3, "{answer}");
    }

    /// Review R2-05: a pending copy past the cap may hold work sent nowhere
    /// else: it is never set aside or removed; it waits, unsettled, and a
    /// later exchange takes its change in.
    #[test]
    fn f2b_r2_05_pending_copies_beyond_the_cap_wait_for_the_next_exchange() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, drive, _) = first_transport(&dir.0);
        let work = dir.0.join("work.ponte");
        std::fs::copy(&drive, &work).unwrap();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular de teste");
        sync_paths(&mut phone, &work.display().to_string(), "", KEY).unwrap();
        let copy = ponteproj::sharedfolder::read(&work, KEY).unwrap().unwrap();
        let mut names = Vec::new();
        for n in 0..7 {
            let mut payload = ponteproj::sharedfolder::Payload {
                snapshot: copy.payload.snapshot.clone(),
                applied: copy.payload.applied.clone(),
                journal: copy.payload.journal.clone(),
                refs_since: None,
            };
            if n == 6 {
                // Only the newest pending copy holds this task.
                let mut task = ponteproj::tasks::Task::standalone();
                task.title = "Tarefa só na sétima pendente".into();
                payload.applied.push("setima-pendente".into());
                payload.journal.push(ponteproj::sharedfolder::JournalEntry {
                    uid: "setima-pendente".into(),
                    created_at: "2026-11-20T12:00:00Z".into(),
                    actor: "Celular de teste".into(),
                    writer: copy.header.writer.clone(),
                    label: "Criar tarefa".into(),
                    mutation: ponteproj::intranet::IntranetMutation::SaveTask {
                        task,
                        expected_version: None,
                        no_project: false,
                    },
                    base: None,
                    revision: copy.header.revision,
                    created: vec![ponteproj::idmap::CreatedId {
                        kind: ponteproj::idmap::IdKind::Task,
                        slot: String::new(),
                        id: 777,
                    }],
                    idents: Vec::new(),
                    refs: None,
                    legacy: false,
                });
            }
            let path = dir.0.join(format!("pendente-novo-{n}.ponte"));
            ponteproj::sharedfolder::write(
                &path,
                KEY,
                &ponteproj::sharedfolder::WriteRequest {
                    lineage: &copy.header.lineage,
                    // Older than the Drive copy: the Drive stays the base and
                    // the task comes from the pending copy's journal.
                    revision: copy.header.revision - 1,
                    writer: &copy.header.writer,
                    writer_name: &copy.header.writer_name,
                    schema_version: copy.header.schema_version,
                },
                &payload,
                &copy.blobs,
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            names.push(path.display().to_string());
        }
        let newest = std::path::PathBuf::from(&names[6]);
        let newest_bytes = std::fs::read(&newest).unwrap();
        let answer = sync_paths(&mut phone, &work.display().to_string(), &names.join("
"), KEY)
            .unwrap();
        assert_eq!(std::fs::read(&newest).unwrap(), newest_bytes, "left as it is");
        let unsettled: Vec<String> = answer["unsettled"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        assert!(unsettled.contains(&names[6]), "{answer}");
        assert!(answer["notes"].to_string().contains("espera a próxima sincronização"), "{answer}");
        // The app lets the settled ones go; the next exchange reads the rest.
        for name in &names[..6] {
            if !unsettled.contains(name) {
                let _ = std::fs::remove_file(name);
            }
        }
        let left: Vec<&String> = names.iter().filter(|n| std::path::Path::new(n.as_str()).exists()).collect();
        let rest = left.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("
");
        sync_paths(&mut phone, &work.display().to_string(), &rest, KEY).unwrap();
        assert!(phone
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Tarefa só na sétima pendente"));
    }

    /// Review R2-06: the phone shows no cards; what an old pending copy held
    /// that was not applied is told in its notes, to be redone.
    #[test]
    fn f2b_r2_06_the_phone_tells_what_an_old_copy_held() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (_computer, _drive, mut phone, nine) = promoted_with_phone(&dir.0);
        let base = match ponteproj::sharedfolder::read_any(&nine, KEY).unwrap() {
            Some(ponteproj::sharedfolder::Found::Copy(copy)) => copy,
            _ => panic!(),
        };
        let mut payload = ponteproj::sharedfolder::Payload {
            snapshot: base.payload.snapshot.clone(),
            applied: base.payload.applied.clone(),
            journal: base.payload.journal.clone(),
            refs_since: None,
        };
        payload.snapshot.idents.clear();
        payload.snapshot.ident_epoch.clear();
        for e in payload.journal.iter_mut() {
            e.idents.clear();
        }
        payload.applied.push("feriados-antigos".into());
        payload.journal.push(ponteproj::sharedfolder::JournalEntry {
            uid: "feriados-antigos".into(),
            created_at: "2026-11-20T12:00:00Z".into(),
            actor: "Celular de teste".into(),
            writer: "celular".into(),
            label: "Feriados".into(),
            mutation: ponteproj::intranet::IntranetMutation::SaveSetting {
                key: "holidays".into(),
                value: "uf GO
dia 2026-12-26 Recesso do celular
".into(),
                seed: false,
            },
            base: None,
            revision: base.header.revision + 1,
            created: Vec::new(),
            idents: Vec::new(),
            refs: None,
            legacy: false,
        });
        let pending = dir.0.join("pendente.ponte");
        ponteproj::sharedfolder::write(
            &pending,
            KEY,
            &ponteproj::sharedfolder::WriteRequest {
                lineage: &base.header.lineage,
                revision: base.header.revision + 1,
                writer: "celular",
                writer_name: "Celular",
                schema_version: 21,
            },
            &payload,
            &base.blobs,
        )
        .unwrap();
        let work = dir.0.join("work.ponte");
        std::fs::copy(&nine, &work).unwrap();
        let answer = sync_paths(
            &mut phone,
            &work.display().to_string(),
            &pending.display().to_string(),
            KEY,
        )
        .unwrap();
        assert!(
            answer["notes"].to_string().contains("Recesso do celular"),
            "{answer}"
        );
    }

    /// Review F3.0a1, finding 1. Drive is at 9/2 and the pending copy is a
    /// 9/1 with a task Drive lacks and its journal no longer explains. Its
    /// archive cannot be made (the name is taken): the pending copy is left
    /// as it is, so the core must report it unsettled by path, or the app
    /// (which keeps only the listed copies, `SyncIo.settledPendings`)
    /// deletes the only copy of that task.
    #[test]
    fn f3_0a_a_9_1_pending_copy_whose_archive_failed_is_listed_unsettled() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let (mut computer, _drive, mut phone, nine) = promoted_with_phone(&dir.0);
        assert_eq!(ponteproj::sharedfolder::read_header(&nine).unwrap().unwrap().level, 2);
        let work = dir.0.join("work.ponte");
        let work_s = work.display().to_string();
        std::fs::copy(&nine, &work).unwrap();
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        // Work that only reached the pending copy (the upload never
        // happened), at the level of an older Ponte, history summarized.
        create_task(&mut phone, &json!({"title": "Só na pendência 9/1"})).unwrap();
        sync_paths(&mut phone, &work_s, "", KEY).unwrap();
        let pending = dir.0.join("pendente.ponte");
        std::fs::copy(&work, &pending).unwrap();
        ponteproj::sharedfolder::reseal_for_test(&pending, KEY, 1, |_, payload| {
            payload.journal.clear();
        })
        .unwrap();
        let _ = std::fs::remove_file(dir.0.join("celular.sqlite3-guarda.ponte"));
        // Meanwhile the computer moves Drive ahead, at 9/2.
        let cfg = computer.load_intranet_config().unwrap();
        let reporter = ponteproj::ai::Reporter::silent();
        for title in ["Do computador", "Outra do computador"] {
            let mut task = ponteproj::tasks::Task::blank(None);
            task.title = title.into();
            computer.save_task(&task).unwrap();
            computer.sync_with_folders(&cfg, KEY, false, &reporter).unwrap();
        }
        std::fs::copy(&nine, &work).unwrap();
        let before = std::fs::read(&pending).unwrap();
        let aside = ponteproj::sharedfolder::aside_path(&pending).unwrap();
        std::fs::create_dir(&aside).unwrap();

        let pending_s = pending.display().to_string();
        let answer = sync_paths(&mut phone, &work_s, &pending_s, KEY).unwrap();
        assert_eq!(answer["pending_settled"], json!(false), "{answer}");
        assert_eq!(std::fs::read(&pending).unwrap(), before, "the only copy is untouched");
        // What the app does with the answer (SyncIo.settledPendings): the
        // working copy listed keeps every pending one; otherwise only the
        // listed ones are kept.
        let unsettled: Vec<String> = answer["unsettled"]
            .as_array()
            .unwrap_or_else(|| panic!("no per-copy verdict: {answer}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        let kept = unsettled.contains(&work_s) || unsettled.contains(&pending_s);
        assert!(kept, "the app would delete the blocked pending copy: {answer}");
        std::fs::remove_dir(&aside).unwrap();
    }

    // -------------------------------------------------------- F3.1a-ii --

    /// `files` as the only shared files of `store`, then one exchange.
    fn sync_files(store: &mut Store, files: &[&Path]) -> ponteproj::intranet::FolderSyncReport {
        let mut cfg = store.load_intranet_config().unwrap();
        cfg.folder_files = files.iter().map(|p| p.display().to_string()).collect();
        store.save_intranet_config(&cfg).unwrap();
        store
            .sync_with_folders(&cfg, KEY, false, &ponteproj::ai::Reporter::silent())
            .unwrap()
    }

    fn record_numbered(store: &Store, number: &str) -> Option<i64> {
        store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.document_number == number)
            .map(|d| d.id)
    }

    fn numbered(number: &str, text: &str) -> ponteproj::model::Capture {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/capture.json"
        );
        let mut capture: ponteproj::model::Capture =
            serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        capture.document_number = number.into();
        capture.title = format!("Documento {number}");
        capture.text = text.into();
        capture.request_id = uuid::Uuid::new_v4().to_string();
        capture.source_url = format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla={}",
            number.replace('/', "%2F")
        );
        capture
    }

    /// F3.1a-ii, the phone path (`desenho-f3-v2.md` §3 F3.1a, F1 C2b). The
    /// phone works on the pen drive's file and holds record X by its number
    /// there. Computer B publishes Y to the Drive, which becomes the newer
    /// copy; when computer A meets both, the Drive is the base and X, made
    /// on the pen drive, is replayed onto it: in the copy X gets another
    /// number and Y gets X's. The phone keeps its own numbers (its adoption
    /// matches the records by identity), so a note it saves through the
    /// number it held lands on X on the computer, and Y is untouched.
    ///
    /// Red on `f7eff3b`: the phone adopts the merged copy verbatim, so the
    /// number it held names Y.
    #[test]
    fn f3_1a_the_phone_keeps_its_numbers_across_a_divergent_merge() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        promote_on();
        let drive = dir.0.join("drive").join("ponte-sync.ponte");
        let pen = dir.0.join("pen").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let mut a = open(&dir.0, "a.sqlite3", "Computador A");
        a.import_capture(&numbered("SEFAZ-PRO-2026/08000", "Primeiro."))
            .unwrap();
        sync_files(&mut a, &[&drive, &pen]);
        let mut b = open(&dir.0, "b.sqlite3", "Computador B");
        sync_files(&mut b, &[&drive]);
        let promoted = sync_files(&mut a, &[&drive, &pen]);
        assert!(promoted.promoted, "{:?}", promoted.promotion_wait);
        sync_files(&mut b, &[&drive]);
        sync_files(&mut a, &[&drive, &pen]);
        sync_files(&mut b, &[&drive]);
        let pen9 = ponteproj::sharedfolder::nine_path(&pen);
        assert!(pen9.is_file());
        // The app copies the pen drive's file, the core exchanges with the
        // copy, and the app uploads it back.
        let work = dir.0.join("work.ponte");
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular");
        let phone_sync = |phone: &mut Store| {
            std::fs::copy(&pen9, &work).unwrap();
            let answer = sync_paths(phone, &work_s, "", KEY).unwrap();
            std::fs::copy(&work, &pen9).unwrap();
            answer
        };
        phone_sync(&mut phone);
        phone_sync(&mut phone);

        const X: &str = "SEFAZ-PRO-2026/08001";
        const Y: &str = "SEFAZ-PRO-2026/08002";
        a.import_capture(&numbered(X, "Documento X, no pen drive."))
            .unwrap();
        sync_files(&mut a, &[&pen]);
        phone_sync(&mut phone);
        let held = record_numbered(&phone, X).expect("X on the phone");

        b.import_capture(&numbered(Y, "Documento Y, no Drive."))
            .unwrap();
        sync_files(&mut b, &[&drive]);
        let first = record_numbered(&b, "SEFAZ-PRO-2026/08000").unwrap();
        for n in 0..3 {
            b.save_note(first, &format!("enchimento {n}")).unwrap();
            sync_files(&mut b, &[&drive]);
        }
        sync_files(&mut a, &[&drive, &pen]);
        let answer = phone_sync(&mut phone);
        assert_eq!(answer["conflicts"], json!(0), "{answer}");
        assert_eq!(
            record_numbered(&phone, X),
            Some(held),
            "the phone keeps X's number"
        );
        assert_ne!(record_numbered(&phone, Y), Some(held));

        // What the app's saveNote does with the number on the screen.
        phone.save_note(held, "Nota do celular").unwrap();
        phone_sync(&mut phone);
        sync_files(&mut a, &[&drive, &pen]);
        let note = |store: &Store, number: &str| {
            store
                .load_document(record_numbered(store, number).unwrap())
                .unwrap()
                .note
        };
        assert_eq!(note(&a, X), "Nota do celular");
        assert_eq!(note(&a, Y), "");
        assert_eq!(note(&phone, X), "Nota do celular");
        assert_eq!(note(&phone, Y), "");
        assert!(a.list_sync_conflicts().unwrap().is_empty());
    }

    // ------------------------------------------------------------ F3.1b1 --

    fn titled(store: &Store, title: &str) -> i64 {
        store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.title == title)
            .unwrap_or_else(|| panic!("sem {title}"))
            .id
    }

    fn texts(store: &Store, id: i64) -> Vec<String> {
        store
            .capture_versions(id)
            .unwrap()
            .into_iter()
            .map(|v| v.capture.text)
            .collect()
    }

    fn manual_fields(title: &str, text: &str) -> ponteproj::manual::ManualFields {
        ponteproj::manual::ManualFields {
            kind: "ligacao".into(),
            title: title.into(),
            origin: "Gabinete".into(),
            document_date: String::new(),
            text: text.into(),
        }
    }

    /// F3.1b1 (adendo §9, frozen rule 18) on the phone. The phone and
    /// computer A each make a manual record offline as the first of the
    /// year: P on the phone, X on A. A edits P; while A's queue is replayed
    /// onto the copy, X and P swap numbers there right before the import's
    /// transaction. The edit is on P (by its identity) on A and on the
    /// phone, which still holds P at its own number; X keeps its one
    /// version; no card.
    ///
    /// Red on `b1de851`: A's edit names no record, and the replay imports
    /// it by the number, which is X's there.
    #[test]
    fn f3_1b1_a_manual_edit_reaches_the_phone_on_its_record() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        promote_on();
        let drive = dir.0.join("drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let mut a = open(&dir.0, "a.sqlite3", "Computador A");
        a.import_capture(&numbered("SEFAZ-PRO-2026/08100", "Primeiro."))
            .unwrap();
        sync_files(&mut a, &[&drive]);
        let mut b = open(&dir.0, "b.sqlite3", "Computador B");
        sync_files(&mut b, &[&drive]);
        assert!(sync_files(&mut a, &[&drive]).promoted);
        sync_files(&mut b, &[&drive]);
        let nine = ponteproj::sharedfolder::nine_path(&drive);
        let work = dir.0.join("work.ponte");
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular");
        let phone_sync = |phone: &mut Store| {
            std::fs::copy(&nine, &work).unwrap();
            let answer = sync_paths(phone, &work_s, "", KEY).unwrap();
            std::fs::copy(&work, &nine).unwrap();
            answer
        };
        phone_sync(&mut phone);
        phone_sync(&mut phone);

        const P: &str = "Registro do celular";
        const X: &str = "Registro de A";
        let held = phone
            .create_manual_document(&manual_fields(P, "Texto do celular"))
            .unwrap()
            .document_id;
        a.create_manual_document(&manual_fields(X, "Texto de A"))
            .unwrap();
        sync_files(&mut a, &[&drive]);
        phone_sync(&mut phone);
        sync_files(&mut a, &[&drive]);
        assert_eq!(titled(&phone, P), held, "the phone keeps its number of P");

        let (p_a, x_a) = (titled(&a, P), titled(&a, X));
        a.update_manual_document(p_a, &manual_fields(P, "Texto do celular, editado em A"))
            .unwrap();
        let idents = phone_idents(&a);
        let (ip, ix) = (
            idents[&("document".to_owned(), p_a)].clone(),
            idents[&("document".to_owned(), x_a)].clone(),
        );
        ponteproj::idents::hooks::before_import_tx(move |conn| {
            use ponteproj::storage::test_numbers::{row_of_ident, swap_numbers};
            swap_numbers(conn, row_of_ident(conn, &ip), row_of_ident(conn, &ix));
        });
        sync_files(&mut a, &[&drive]);
        assert!(
            ponteproj::idents::hooks::BEFORE_IMPORT_TX.with(|h| h.borrow_mut().take().is_none()),
            "the window was used"
        );
        let answer = phone_sync(&mut phone);
        assert_eq!(answer["conflicts"], json!(0), "{answer}");
        assert_eq!(titled(&phone, P), held);
        for store in [&a, &phone] {
            assert_eq!(
                texts(store, titled(store, P)),
                vec!["Texto do celular, editado em A", "Texto do celular"]
            );
            assert_eq!(texts(store, titled(store, X)), vec!["Texto de A"]);
            assert!(store.list_sync_conflicts().unwrap().is_empty());
        }
    }

    // ------------------------------------------------------------ F3.1b2 --

    const OP_C: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a0c";
    const OP_D: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a0d";
    const OP_E: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a0e";
    const OP_F: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a0f";
    const OP_G: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a10";
    const OP_H: &str = "0b1f6c1e-52c5-4f0a-9a52-3c0d6b6f0a11";

    const GONE_ELSEWHERE: &str = "O registro foi apagado em outro aparelho.";
    const IN_TRASH: &str = "O registro está na Lixeira.";
    const NOT_HERE: &str = "O registro não está mais disponível neste aparelho.";

    /// The options the app sends with a `PendingAttach` (adendo §10 rule 1):
    /// its operation, and the record's identity and the library space as
    /// `getDocument` gave them when the person chose the record.
    fn pending(operation: &str, proof: &Value) -> String {
        json!({
            "operation_id": operation,
            "document_ident": proof["ident"],
            "library_space": proof["library_space"],
        })
        .to_string()
    }

    /// The card of an attachment the core refused at once (`target_gone`).
    fn refused_card(answer: &Result<Value>) -> i64 {
        let error = answer.as_ref().expect_err("refused, not attached");
        let said = error_answer(error);
        assert_eq!(said["code"], json!("target_gone"), "{said}");
        said["card_id"]
            .as_i64()
            .unwrap_or_else(|| panic!("a card holds the file: {said}"))
    }

    fn refused_operations(store: &Store) -> Vec<String> {
        refused_attachments(store).unwrap()["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["operation_id"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Adendo §10 (R2-10), `f3_1b2_phone_op_survives_refusals_and_space_change`,
    /// the core's half (the app's half is `PendingAttachTest`). The app holds
    /// record D with its identity and the library space; the library is
    /// replaced and the held number names another record, E. Delivering is
    /// refused: nothing is attached to E, the photo is kept in a card in the
    /// same transaction as the refusal (frozen rules 19, 20, 23), and asking
    /// again after a process death answers the same, with one card. The
    /// recovery screen's three actions: "Anexar a outro registro…" (E, chosen
    /// explicitly) and "Criar registro com esta foto" each make exactly one
    /// effect when repeated and resolve the card; "Descartar" ends it. A
    /// replaced library never says "apagado" (rule 21); a record in the
    /// Lixeira says so.
    ///
    /// Red on `77e24b7`: the core knows neither the identity nor the space,
    /// so the photo goes to E; there is no card and no recovery.
    #[test]
    fn f3_1b2_phone_op_survives_refusals_and_space_change() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular");
        let d = manual(&mut phone, "Registro D", "Texto de D");
        let held = attach_target(&phone, d).unwrap();
        for key in ["ident", "library_space"] {
            assert!(
                held[key].as_str().is_some_and(|v| !v.is_empty()),
                "getDocument gives the {key}: {held}"
            );
        }
        assert_eq!(attach_target(&phone, d).unwrap(), held, "stable");
        drop(phone);

        // The library is replaced (the person chose another file, the app
        // made a new one): the number held on the screen names E now.
        let mut phone = open(&dir.0, "celular-novo.sqlite3", "Celular");
        let e = manual(&mut phone, "Registro E", "Texto de E");
        assert_eq!(e, d, "the held number is reused");
        let to_e = attach_target(&phone, e).unwrap();
        assert_ne!(to_e["library_space"], held["library_space"]);
        assert_ne!(to_e["ident"], held["ident"]);

        let photo = work_file(&dir.0, "foto.bin", b"%PDF-1.4 foto do celular");
        let first = attach_file_with(&mut phone, d, &photo, "foto.pdf", "Foto tirada no celular", &pending(OP_C, &held));
        let card = refused_card(&first);
        assert!(attachments_of(&phone, e).is_empty(), "nothing is attached to E");
        let st = status(&phone, OP_C);
        assert_eq!(st["done"], json!(true), "{st}");
        assert_eq!(st["outcome"], json!("refused"), "{st}");
        assert_eq!(st["card_id"], json!(card), "{st}");
        assert_eq!(st["message"], json!(NOT_HERE), "a replaced library never says apagado: {st}");
        // The process died after the refusal: the same operation again,
        // even with the working file gone, answers the same; one card.
        std::fs::remove_file(&photo).unwrap();
        let again = attach_file_with(&mut phone, d, &photo, "foto.pdf", "Foto tirada no celular", &pending(OP_C, &held));
        assert_eq!(refused_card(&again), card);
        assert_eq!(phone.list_sync_conflicts().unwrap().len(), 1);
        assert!(attachments_of(&phone, e).is_empty());
        let listed = refused_attachments(&phone).unwrap();
        let item = &listed["items"][0];
        assert_eq!(item["operation_id"], json!(OP_C), "{listed}");
        assert_eq!(item["card_id"], json!(card), "{listed}");
        assert_eq!(item["name"], json!("foto.pdf"), "{listed}");
        assert_eq!(item["message"], json!(NOT_HERE), "{listed}");

        // The space alone is checked too: E's identity with the old space.
        let photo = work_file(&dir.0, "foto2.bin", b"%PDF-1.4 outra foto");
        let mixed = json!({"ident": to_e["ident"], "library_space": held["library_space"]});
        let other_card = refused_card(&attach_file_with(&mut phone, e, &photo, "foto2.pdf", "", &pending(OP_D, &mixed)));
        assert!(attachments_of(&phone, e).is_empty(), "a space mismatch never attaches");
        assert_ne!(other_card, card);

        // "Anexar a outro registro…": E, chosen explicitly, by its proof.
        let one = attach_from_card(&mut phone, card, e, &pending(OP_E, &to_e)).unwrap();
        let two = attach_from_card(&mut phone, card, e, &pending(OP_E, &to_e)).unwrap();
        assert_eq!(one, two);
        assert_eq!(attachments_of(&phone, e), ["foto.pdf"]);
        let st = status(&phone, OP_E);
        assert_eq!((st["done"].clone(), st["kind"].clone()), (json!(true), json!("attach")), "{st}");
        assert_eq!(refused_operations(&phone), [OP_D], "the card of OP_C is resolved");

        // "Criar registro com esta foto", twice: one record with the photo.
        let before = live(&phone);
        let input = with_operation(
            json!({"kind": "papel", "title": "Foto recuperada", "origin": "", "date": "", "text": ""}),
            OP_F,
        );
        let made = create_record_from_card(&mut phone, other_card, &input).unwrap();
        assert_eq!(create_record_from_card(&mut phone, other_card, &input).unwrap(), made);
        assert_eq!(live(&phone), before + 1);
        let record = made["document_id"].as_i64().unwrap();
        assert_eq!(attachments_of(&phone, record), ["foto2.pdf"]);
        assert_eq!(attachments_of(&phone, e), ["foto.pdf"]);
        assert!(refused_operations(&phone).is_empty());
        assert_eq!(status(&phone, OP_F)["kind"], json!("record"));

        // A record in the Lixeira: refused with its own words; "Descartar".
        let trashed = manual(&mut phone, "Registro na Lixeira", "Texto");
        let proof = attach_target(&phone, trashed).unwrap();
        phone
            .trash_item(ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: trashed })
            .unwrap();
        let photo = work_file(&dir.0, "foto3.bin", b"%PDF-1.4 terceira foto");
        let card = refused_card(&attach_file_with(&mut phone, trashed, &photo, "foto3.pdf", "", &pending(OP_G, &proof)));
        assert_eq!(status(&phone, OP_G)["message"], json!(IN_TRASH));
        // Attaching from the card to a record that is gone too: nothing is
        // written and the card stays.
        assert!(attach_from_card(&mut phone, card, trashed, &pending(OP_H, &proof)).is_err());
        assert_eq!(status(&phone, OP_H), json!({"done": false}));
        assert_eq!(refused_operations(&phone), [OP_G]);
        discard_card(&mut phone, card).unwrap();
        assert!(refused_operations(&phone).is_empty());
        assert!(attachments_of(&phone, trashed).is_empty());
        assert!(
            attach_from_card(&mut phone, card, e, &pending(OP_H, &to_e)).is_err(),
            "a discarded card holds nothing to attach"
        );
        assert_eq!(attachments_of(&phone, e), ["foto.pdf"]);
    }

    /// Adendo §10 rules 4 and 5, the late refusal. The phone attaches a
    /// photo to R (accepted here, queued); computer A deletes R for good
    /// before the phone exchanges. The exchange refuses the photo into a
    /// card, and the operation is bound to that card in the card's own
    /// transaction (never inferred from the queue). R was retired by a
    /// same-epoch adoption, so the phone says "apagado em outro aparelho",
    /// now and for a stale screen that tries again. A photo of a record that
    /// survives is accepted. "Criar registro com esta foto" reaches A, and R
    /// does not come back.
    ///
    /// Red on `77e24b7`: the operation has no outcome and no card, and the
    /// core keeps no proof of deletion.
    #[test]
    fn f3_1b2_late_refusal_is_bound_to_its_card_and_says_deleted_elsewhere() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        promote_on();
        let drive = dir.0.join("drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let mut a = open(&dir.0, "a.sqlite3", "Computador A");
        a.import_capture(&numbered("SEFAZ-PRO-2026/08200", "Primeiro."))
            .unwrap();
        sync_files(&mut a, &[&drive]);
        let mut b = open(&dir.0, "b.sqlite3", "Computador B");
        sync_files(&mut b, &[&drive]);
        assert!(sync_files(&mut a, &[&drive]).promoted);
        sync_files(&mut b, &[&drive]);
        let nine = ponteproj::sharedfolder::nine_path(&drive);
        let work = dir.0.join("work.ponte");
        let work_s = work.display().to_string();
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular");
        let phone_sync = |phone: &mut Store| {
            std::fs::copy(&nine, &work).unwrap();
            let answer = sync_paths(phone, &work_s, "", KEY).unwrap();
            std::fs::copy(&work, &nine).unwrap();
            answer
        };
        phone_sync(&mut phone);
        phone_sync(&mut phone);
        a.create_manual_document(&manual_fields("Registro R", "Texto de R"))
            .unwrap();
        a.create_manual_document(&manual_fields("Registro S", "Texto de S"))
            .unwrap();
        sync_files(&mut a, &[&drive]);
        phone_sync(&mut phone);
        let (r, s) = (titled(&phone, "Registro R"), titled(&phone, "Registro S"));
        let (to_r, to_s) = (attach_target(&phone, r).unwrap(), attach_target(&phone, s).unwrap());

        let photo = work_file(&dir.0, "foto.bin", b"%PDF-1.4 foto de R");
        attach_file_with(&mut phone, r, &photo, "foto-r.pdf", "", &pending(OP_C, &to_r)).unwrap();
        let photo = work_file(&dir.0, "foto2.bin", b"%PDF-1.4 foto de S");
        attach_file_with(&mut phone, s, &photo, "foto-s.pdf", "", &pending(OP_D, &to_s)).unwrap();
        for op in [OP_C, OP_D] {
            let st = status(&phone, op);
            assert_eq!(st["outcome"], json!("pending"), "queued, not yet accepted: {st}");
        }

        let r_a = titled(&a, "Registro R");
        let target = || ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: r_a };
        a.trash_item(target()).unwrap();
        a.purge_trash_item(target()).unwrap();
        sync_files(&mut a, &[&drive]);
        phone_sync(&mut phone);

        let st = status(&phone, OP_C);
        assert_eq!(st["outcome"], json!("refused"), "{st}");
        let card = st["card_id"]
            .as_i64()
            .unwrap_or_else(|| panic!("bound to its card: {st}"));
        assert_eq!(st["message"], json!(GONE_ELSEWHERE), "{st}");
        assert_eq!(status(&phone, OP_D)["outcome"], json!("accepted"));
        assert_eq!(refused_operations(&phone), [OP_C]);
        // A stale screen tries R again: refused at once, with the same words.
        let photo = work_file(&dir.0, "foto3.bin", b"%PDF-1.4 outra foto de R");
        refused_card(&attach_file_with(&mut phone, r, &photo, "foto-r2.pdf", "", &pending(OP_E, &to_r)));
        assert_eq!(status(&phone, OP_E)["message"], json!(GONE_ELSEWHERE));

        let input = with_operation(
            json!({"kind": "papel", "title": "Foto de R recuperada", "origin": "", "date": "", "text": ""}),
            OP_F,
        );
        let made = create_record_from_card(&mut phone, card, &input).unwrap();
        assert_eq!(create_record_from_card(&mut phone, card, &input).unwrap(), made);
        assert_eq!(refused_operations(&phone), [OP_E]);
        phone_sync(&mut phone);
        sync_files(&mut a, &[&drive]);
        let recovered = titled(&a, "Foto de R recuperada");
        assert_eq!(attachments_of(&a, recovered), ["foto-r.pdf"]);
        assert_eq!(attachments_of(&a, titled(&a, "Registro S")), ["foto-s.pdf"]);
        assert!(
            a.list_documents("").unwrap().iter().all(|d| d.title != "Registro R"),
            "R does not come back"
        );
        // A record made from the card is an operation of its own; only the
        // file for a record carries an outcome.
        let st = status(&phone, OP_F);
        assert_eq!((st["kind"].clone(), st.get("outcome")), (json!("record"), None), "{st}");
    }

    // ------------------------------------------ F3.1b2 review fixes (c) --
    // `revisao-f3-1b2.md` findings 1, 2, 3 and 6, the core's half (the app's
    // half is `PendingAttachFlowTest`).

    /// What the card of a file refused at once says once its record is
    /// back (restored from the Lixeira): never that it is gone.
    const WAS_UNAVAILABLE: &str =
        "O arquivo não foi anexado porque o registro não estava disponível naquele momento.";

    /// Computer A, a phone and the folder they share, with the phone in
    /// step (the probes of `revisao-f3-1b2-sondas.patch`).
    struct Team {
        a: Store,
        phone: Store,
        drive: PathBuf,
        nine: PathBuf,
        work: PathBuf,
    }

    impl Team {
        fn new(dir: &Dir) -> Team {
            std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
            promote_on();
            let drive = dir.0.join("drive").join("ponte-sync.ponte");
            std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
            let mut a = open(&dir.0, "a.sqlite3", "Computador A");
            a.import_capture(&numbered("SEFAZ-PRO-2026/08200", "Primeiro.")).unwrap();
            sync_files(&mut a, &[&drive]);
            let mut b = open(&dir.0, "b.sqlite3", "Computador B");
            sync_files(&mut b, &[&drive]);
            assert!(sync_files(&mut a, &[&drive]).promoted);
            sync_files(&mut b, &[&drive]);
            let nine = ponteproj::sharedfolder::nine_path(&drive);
            let work = dir.0.join("work.ponte");
            let phone = open(&dir.0, "celular.sqlite3", "Celular");
            let mut team = Team { a, phone, drive, nine, work };
            team.phone_sync();
            team.phone_sync();
            team
        }

        fn phone_sync(&mut self) -> Value {
            std::fs::copy(&self.nine, &self.work).unwrap();
            let answer = sync_paths(&mut self.phone, &self.work.display().to_string(), "", KEY).unwrap();
            std::fs::copy(&self.work, &self.nine).unwrap();
            answer
        }

        fn a_sync(&mut self) {
            sync_files(&mut self.a, &[&self.drive]);
        }

        /// Record `title`, made on A and seen by the phone: its id there.
        fn record(&mut self, title: &str) -> i64 {
            self.a.create_manual_document(&manual_fields(title, "Texto")).unwrap();
            self.a_sync();
            self.phone_sync();
            titled(&self.phone, title)
        }
    }

    fn reason_of(store: &Store, card: i64) -> String {
        store
            .list_sync_conflicts()
            .unwrap()
            .into_iter()
            .find(|c| c.id == card)
            .unwrap_or_else(|| panic!("card {card} waits"))
            .reason
    }

    fn listed_message(store: &Store, card: i64) -> Value {
        let listed = refused_attachments(store).unwrap();
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["card_id"] == json!(card))
            .unwrap_or_else(|| panic!("card {card} is listed: {listed}"))["message"]
            .clone()
    }

    /// Finding 2 (MEDIUM), probe P1. The phone attaches to R and to S while
    /// out of reach; A attaches the same bytes to R and fills S with twenty
    /// files. The exchange refuses both photos with the record alive: what
    /// the phone says is the card's own reason (the identical file, the
    /// limit), never that the record is not available.
    ///
    /// Red on `645c23f`: both say "O registro não está mais disponível
    /// neste aparelho.".
    #[test]
    fn f3_1b2c_a_late_refusal_says_its_own_reason() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let mut t = Team::new(&dir);
        let r = t.record("Registro R");
        let s = t.record("Registro S");
        let (to_r, to_s) = (attach_target(&t.phone, r).unwrap(), attach_target(&t.phone, s).unwrap());
        let bytes: &[u8] = b"%PDF-1.4 foto de R";
        let photo = work_file(&dir.0, "foto.bin", bytes);
        attach_file_with(&mut t.phone, r, &photo, "foto-r.pdf", "", &pending(OP_C, &to_r)).unwrap();
        let photo_s = work_file(&dir.0, "foto2.bin", b"%PDF-1.4 foto de S");
        attach_file_with(&mut t.phone, s, &photo_s, "foto-s.pdf", "", &pending(OP_D, &to_s)).unwrap();

        let (r_a, s_a) = (titled(&t.a, "Registro R"), titled(&t.a, "Registro S"));
        t.a.add_attachment(ponteproj::attachments::Owner::Document(r_a), "igual.pdf", "", bytes).unwrap();
        for i in 0..ponteproj::attachments::MAX_PER_OWNER {
            let data = format!("%PDF-1.4 arquivo {i} de S");
            t.a.add_attachment(ponteproj::attachments::Owner::Document(s_a), &format!("s{i}.pdf"), "", data.as_bytes())
                .unwrap();
        }
        t.a_sync();
        t.phone_sync();

        let (st_c, st_d) = (status(&t.phone, OP_C), status(&t.phone, OP_D));
        assert_eq!((st_c["outcome"].clone(), st_d["outcome"].clone()), (json!("refused"), json!("refused")));
        let (card_c, card_d) = (st_c["card_id"].as_i64().unwrap(), st_d["card_id"].as_i64().unwrap());
        let (said_c, said_d) = (reason_of(&t.phone, card_c), reason_of(&t.phone, card_d));
        assert!(said_c.contains("conteúdo idêntico"), "{said_c}");
        assert!(said_d.contains("20 anexos"), "{said_d}");
        for (st, card, said) in [(&st_c, card_c, &said_c), (&st_d, card_d, &said_d)] {
            assert_ne!(st["message"], json!(NOT_HERE), "R and S are alive: {st}");
            assert_eq!(st["message"], json!(said), "the real reason: {st}");
            assert_eq!(listed_message(&t.phone, card), json!(said));
        }
        // A stale screen repeats the operation: the same words.
        let again = attach_file_with(&mut t.phone, r, &photo, "foto-r.pdf", "", &pending(OP_C, &to_r));
        assert!(format!("{:#}", again.unwrap_err()).contains("conteúdo idêntico"));
        assert_eq!(attachments_of(&t.phone, r), ["igual.pdf"]);
    }

    /// Finding 2, probe P2. A photo refused at once because R was in the
    /// Lixeira says so; once R is restored, it says the record was not
    /// available then, never that it is not available now. A record
    /// deleted elsewhere keeps saying so (frozen rule 21).
    ///
    /// Red on `645c23f`: after the restore it says "O registro não está
    /// mais disponível neste aparelho.".
    #[test]
    fn f3_1b2c_a_record_back_from_the_lixeira_is_not_said_unavailable() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let mut t = Team::new(&dir);
        let r = t.record("Registro R");
        let to_r = attach_target(&t.phone, r).unwrap();
        let target = || ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: r };
        t.phone.trash_item(target()).unwrap();
        let photo = work_file(&dir.0, "foto.bin", b"%PDF-1.4 foto de R");
        let card = refused_card(&attach_file_with(&mut t.phone, r, &photo, "foto-r.pdf", "", &pending(OP_C, &to_r)));
        assert_eq!(listed_message(&t.phone, card), json!(IN_TRASH));
        assert_eq!(status(&t.phone, OP_C)["message"], json!(IN_TRASH));
        t.phone.restore_item(target()).unwrap();
        assert_eq!(listed_message(&t.phone, card), json!(WAS_UNAVAILABLE));
        assert_eq!(status(&t.phone, OP_C)["message"], json!(WAS_UNAVAILABLE));
        t.phone_sync();
        t.a_sync();
        t.phone_sync();
        assert_eq!(listed_message(&t.phone, card), json!(WAS_UNAVAILABLE), "across exchanges");
        // "Anexar a outro registro…" to R itself, now that it is back.
        attach_from_card(&mut t.phone, card, r, &pending(OP_E, &to_r)).unwrap();
        assert_eq!(attachments_of(&t.phone, r), ["foto-r.pdf"]);
        assert!(refused_operations(&t.phone).is_empty());
    }

    /// Finding 1 (HIGH), the core's half. The app deletes the photo it
    /// holds only for an answer that says where the file is: attached, kept
    /// in a card (`target_gone` with its `card_id`), or refused for what the
    /// file itself is (`file_refused`: its type, content, size or name,
    /// which no record would take). A refused description is the person's
    /// words, not the file: no code, the photo stays. A record that cannot take
    /// one more file (the limit, the same content already there) keeps it
    /// in a card with that reason. Anything else (the file unreadable, the
    /// library unavailable) carries no code: the app keeps the photo.
    ///
    /// Red on `645c23f`: no refusal carries a code, and the limit and the
    /// identical file are refused with nothing kept.
    #[test]
    fn f3_1b2c_only_a_file_s_own_refusal_or_a_card_lets_the_photo_go() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Dir::new();
        std::env::set_var("PONTEPROJ_DATA_DIR", &dir.0);
        let mut phone = open(&dir.0, "celular.sqlite3", "Celular");
        let d = manual(&mut phone, "Registro D", "Texto de D");
        let e = manual(&mut phone, "Registro E", "Texto de E");
        let (to_d, to_e) = (attach_target(&phone, d).unwrap(), attach_target(&phone, e).unwrap());
        let code = |answer: &Result<Value>| error_answer(answer.as_ref().expect_err("refused")).get("code").cloned();

        // The file itself: nothing written, no card, and the code says so.
        let big = vec![b'%'; ponteproj::attachments::MAX_ATTACHMENT_BYTES + 1];
        let long = "x".repeat(ponteproj::attachments::MAX_DESCRIPTION_CHARS + 1);
        let cases: [(&str, &[u8], &str); 4] = [
            ("programa.exe", b"MZ programa", ""),
            ("foto.pdf", b"", ""),
            ("foto.pdf", b"isto nao e um pdf", ""),
            ("grande.pdf", &big, ""),
        ];
        for (i, (name, bytes, description)) in cases.into_iter().enumerate() {
            let photo = work_file(&dir.0, &format!("caso{i}.bin"), bytes);
            let answer = attach_file_with(&mut phone, d, &photo, name, description, &pending(OP_C, &to_d));
            assert_eq!(code(&answer), Some(json!("file_refused")), "{name}: {answer:?}");
            assert_eq!(status(&phone, OP_C), json!({"done": false}));
        }
        assert!(phone.list_sync_conflicts().unwrap().is_empty(), "nothing kept for a file no record takes");

        // A description refused (too long, or a control character such as a
        // pasted "\r"): the photo is fine, so no code and nothing written.
        for (i, description) in [long.as_str(), "linha um\r\nlinha dois"].into_iter().enumerate() {
            let photo = work_file(&dir.0, &format!("descr{i}.bin"), b"%PDF-1.4 foto");
            let answer = attach_file_with(&mut phone, d, &photo, "foto.pdf", description, &pending(OP_C, &to_d));
            assert_eq!(code(&answer), None, "{description:?}: {answer:?}");
            assert_eq!(status(&phone, OP_C), json!({"done": false}));
        }

        // No answer about the file (it cannot be read here): no code, nothing written.
        let missing = dir.0.join("sumiu.bin").display().to_string();
        let answer = attach_file_with(&mut phone, d, &missing, "foto.pdf", "", &pending(OP_C, &to_d));
        assert_eq!(code(&answer), None, "{answer:?}");
        assert_eq!(status(&phone, OP_C), json!({"done": false}));
        assert_eq!(error_answer(&anyhow::anyhow!("O acervo ficou indisponível; feche e abra o aplicativo.")).get("code"), None);

        // The limit: D is full, so the photo is kept in a card with that reason.
        for i in 0..ponteproj::attachments::MAX_PER_OWNER {
            let photo = work_file(&dir.0, &format!("d{i}.bin"), format!("%PDF-1.4 anexo {i}").as_bytes());
            attach_file(&mut phone, d, &photo, &format!("d{i}.pdf"), "").unwrap();
        }
        let photo = work_file(&dir.0, "cheio.bin", b"%PDF-1.4 a vigesima primeira");
        let first = attach_file_with(&mut phone, d, &photo, "foto-d.pdf", "", &pending(OP_C, &to_d));
        let card = refused_card(&first);
        assert!(format!("{:#}", first.unwrap_err()).contains("20 anexos"));
        let st = status(&phone, OP_C);
        assert_eq!((st["outcome"].clone(), st["card_id"].clone()), (json!("refused"), json!(card)), "{st}");
        assert!(st["message"].as_str().unwrap().contains("20 anexos"), "{st}");
        assert_eq!(listed_message(&phone, card), st["message"]);
        assert_eq!(attachments_of(&phone, d).len(), ponteproj::attachments::MAX_PER_OWNER);
        // Repeated after a process death: the same card.
        assert_eq!(refused_card(&attach_file_with(&mut phone, d, &photo, "foto-d.pdf", "", &pending(OP_C, &to_d))), card);
        // Recovered to E, chosen by the person.
        attach_from_card(&mut phone, card, e, &pending(OP_E, &to_e)).unwrap();
        assert_eq!(attachments_of(&phone, e), ["foto-d.pdf"]);

        // The same content already on E: kept in a card, with that reason.
        let photo = work_file(&dir.0, "igual.bin", b"%PDF-1.4 a vigesima primeira");
        let card = refused_card(&attach_file_with(&mut phone, e, &photo, "outra.pdf", "", &pending(OP_D, &to_e)));
        assert!(listed_message(&phone, card).as_str().unwrap().contains("conteúdo idêntico"));
        assert_eq!(attachments_of(&phone, e), ["foto-d.pdf"]);
        assert_eq!(refused_operations(&phone), [OP_D]);
    }

    /// Finding 3 (LOW). A file attached without an operation (a share of an
    /// older Ponte de bolso, or one queued before this version) and refused
    /// at the exchange is listed for recovery like any other, and each of
    /// the actions works on it.
    ///
    /// Red on `645c23f`: the cards are kept, but nothing lists them.
    #[test]
    fn f3_1b2c_a_refused_file_without_an_operation_is_listed() {
        let _turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetHolidays;
        let dir = Dir::new();
        let mut t = Team::new(&dir);
        let r = t.record("Registro R");
        let photo = work_file(&dir.0, "foto.bin", b"%PDF-1.4 foto de R");
        attach_file(&mut t.phone, r, &photo, "foto-r.pdf", "").unwrap();
        let photo = work_file(&dir.0, "foto2.bin", b"%PDF-1.4 outra foto de R");
        attach_file(&mut t.phone, r, &photo, "foto-r2.pdf", "").unwrap();
        let r_a = titled(&t.a, "Registro R");
        let target = || ponteproj::trash::Target { kind: ponteproj::trash::Kind::Document, id: r_a };
        t.a.trash_item(target()).unwrap();
        t.a.purge_trash_item(target()).unwrap();
        t.a_sync();
        t.phone_sync();

        let listed = refused_attachments(&t.phone).unwrap();
        let items = listed["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "{listed}");
        assert_eq!(items[0]["name"], json!("foto-r.pdf"), "{listed}");
        assert_eq!(items[0]["size"], json!(18), "{listed}");
        assert_eq!(items[1]["name"], json!("foto-r2.pdf"), "{listed}");
        let (first, second) = (items[0]["card_id"].as_i64().unwrap(), items[1]["card_id"].as_i64().unwrap());
        assert_eq!(refused_attachments(&t.phone).unwrap(), listed, "listing twice changes nothing");

        let input = with_operation(
            json!({"kind": "papel", "title": "Foto de R recuperada", "origin": "", "date": "", "text": ""}),
            OP_F,
        );
        let made = create_record_from_card(&mut t.phone, first, &input).unwrap();
        assert_eq!(create_record_from_card(&mut t.phone, first, &input).unwrap(), made);
        let record = made["document_id"].as_i64().unwrap();
        assert_eq!(attachments_of(&t.phone, record), ["foto-r.pdf"]);
        discard_card(&mut t.phone, second).unwrap();
        assert!(refused_attachments(&t.phone).unwrap()["items"].as_array().unwrap().is_empty());
    }
}
