//! JNI entry points of the Ponte core for the Android prototype ("Ponte de
//! bolso"). The phone is one more member of the shared-folder mode: the app
//! copies `ponte-sync.ponte` from Google Drive into a private working file, this core runs
//! the very same synchronization the desktop runs on that local copy, and
//! the app writes the copy back to Drive only when nobody changed it there
//! meanwhile.
//!
//! Every function answers a JSON string, `{"ok":true,...}` or
//! `{"ok":false,"error":"..."}`; nothing panics across the JNI boundary.
use anyhow::{Context, Result};
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;
use ponteproj::intranet::{IntranetConfig, IntranetMode};
use ponteproj::storage::Store;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

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
        Ok(Err(error)) => json!({"ok": false, "error": format!("{error:#}")}),
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
            *guard = Some(store);
        }
        Ok(json!({}))
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
    anyhow::ensure!(
        std::path::Path::new(path).is_file(),
        "O arquivo de sincronização não foi encontrado."
    );
    let mut files = vec![path.to_owned()];
    if !pending.is_empty() && std::path::Path::new(pending).exists() {
        files.push(pending.to_owned());
    }
    let mut cfg = store.load_intranet_config()?;
    if cfg.folder_files != files {
        cfg.folder_files = files;
        store.save_intranet_config(&cfg)?;
    }
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
    });
    if written {
        answer["bytes"] = json!(std::fs::metadata(path)?.len());
    }
    Ok(answer)
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
            for task in store.list_tasks()?.iter().filter(|t| t.status != "concluida") {
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
                    "email": summary.email,
                    "email_from": summary.email_from,
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

// ---------------------------------------------------------------------------
// Version 2: the document file, tasks, a record from a photo, e-mail.
// Everything is built on the same Store functions the desktop uses.

/// JSON text of a JNI string parameter.
fn json_arg(env: &mut JNIEnv, value: &JString) -> Result<Value> {
    serde_json::from_str(&text(env, value)?).context("Dados inválidos vindos do aplicativo.")
}

fn task_json(task: &ponteproj::tasks::Task, numbers: &std::collections::HashMap<i64, String>) -> Value {
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    task_json_on(task, numbers, &today)
}

/// `task_json` as of the given day (`AAAA-MM-DD`), so tests can fix the date.
fn task_json_on(
    task: &ponteproj::tasks::Task,
    numbers: &std::collections::HashMap<i64, String>,
    today: &str,
) -> Value {
    let open = task.status != "concluida";
    json!({
        "id": task.id,
        // 0 and an empty number: a task without a document.
        "document_id": task.document_id.unwrap_or(0),
        "document_number": task
            .document_id
            .and_then(|id| numbers.get(&id).cloned())
            .unwrap_or_default(),
        "title": task.title,
        "action": task.action,
        "action_label": ponteproj::tasks::action_label(&task.action),
        "status": task.status,
        "status_label": ponteproj::tasks::status_label(&task.status),
        "priority": task.priority,
        "priority_label": ponteproj::tasks::priority_label(&task.priority),
        "due_date": task.due_date,
        "due_label": ponteproj::labels::date(&task.due_date),
        "overdue": open && !task.due_date.is_empty() && task.due_date.as_str() < today,
        "due_today": open && task.due_date == today,
        "notes": task.notes,
    })
}

fn document_numbers(store: &Store) -> Result<std::collections::HashMap<i64, String>> {
    Ok(store
        .list_documents("")?
        .into_iter()
        .map(|d| (d.id, d.document_number))
        .collect())
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

/// Open tasks first (overdue, then due today, then by date), concluded last.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listTasks<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let numbers = document_numbers(store)?;
            let tasks = store
                .list_tasks()?
                .iter()
                .map(|t| task_json(t, &numbers))
                .collect::<Vec<_>>();
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
    let document_id = input["document_id"].as_i64().filter(|id| *id > 0);
    let mut task = ponteproj::tasks::Task::blank(document_id);
    task.title = input["title"].as_str().unwrap_or("").trim().to_owned();
    if let Some(action) = input["action"].as_str().filter(|a| !a.is_empty()) {
        task.action = action.to_owned();
    }
    task.due_date = valid_due(input["due_date"].as_str().unwrap_or(""))?;
    let id = store.save_task(&task)?;
    Ok(json!({ "id": id }))
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
    "Esta tarefa foi alterada em outro aparelho. Abra-a de novo para ver a versão atual.";

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
    let numbers = documents.iter().map(|d| (d.id, d.document_number.clone())).collect();
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
/// `action`, `priority`, `due_date`, `assignee`, `notes`) over the stored task;
/// everything else (dependencies, reminder, checklist, outcome, document,
/// status) stays as stored. A stale `version` saves nothing.
fn save_task(store: &mut Store, input: &Value) -> Result<Value> {
    let id = input["id"].as_i64().context("Tarefa ausente.")?;
    let version = input["version"].as_i64().context("Versão da tarefa ausente.")?;
    let mut task = find_task(store, id)?;
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
    anyhow::ensure!(!task.title.is_empty(), "Informe o título da tarefa.");
    anyhow::ensure!(
        ["baixa", "normal", "alta", "urgente"].contains(&task.priority.as_str()),
        "A prioridade da tarefa é inválida."
    );
    store.save_task_checked(&task, Some(version)).map_err(task_conflict)?;
    Ok(json!({ "id": id, "version": store.item_version("task", id)?.version }))
}

/// Moves a task to `document_id`, or (0) takes it out of its document.
fn relink_task(store: &mut Store, task_id: i64, document_id: i64, version: i64) -> Result<Value> {
    find_task(store, task_id)?;
    store
        .relink_task(task_id, (document_id > 0).then_some(document_id), Some(version))
        .map_err(task_conflict)?;
    Ok(json!({ "id": task_id, "version": store.item_version("task", task_id)?.version }))
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
            today_lists(store, &today)
        })
    })
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
            let reference = if !a.reference_raw.is_empty() {
                a.reference_raw.as_str()
            } else if !a.subject.is_empty() {
                a.subject.as_str()
            } else {
                "Sem referência"
            };
            json!({
                "id": a.id,
                "reference": reference,
                "subject": a.subject,
                "status_label": ponteproj::saiop::status_label(&a.status),
                "due_date": a.due_date,
                "late": a.due_date.as_str() < today,
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
        Ok(json!({"actions": actions, "manual_kinds": kinds, "priorities": priorities}))
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
        let input = json_arg(env, &fields_json)?;
        let image = PathBuf::from(text(env, &image_path)?);
        let bytes = std::fs::read(&image).context("A foto não pôde ser lida.")?;
        // Checked before the record exists, so a photo that cannot be
        // attached never leaves a record without it.
        anyhow::ensure!(!bytes.is_empty(), "A foto está vazia.");
        anyhow::ensure!(
            bytes.len() <= ponteproj::attachments::MAX_ATTACHMENT_BYTES,
            "A foto passa do limite de anexos ({} MB).",
            ponteproj::attachments::MAX_ATTACHMENT_BYTES / (1024 * 1024)
        );
        with_store(|store| {
            let str_of = |key: &str| input[key].as_str().unwrap_or("").trim().to_owned();
            let title = str_of("title");
            let mut text = str_of("text");
            if text.is_empty() {
                text = format!("Registro feito no celular a partir de uma foto: {title}.");
            }
            let fields = ponteproj::manual::ManualFields {
                kind: if str_of("kind").is_empty() { "papel".into() } else { str_of("kind") },
                title,
                origin: str_of("origin"),
                document_date: str_of("date"),
                text,
            };
            let receipt = store.create_manual_document(&fields)?;
            let name = format!(
                "foto-{}.jpg",
                chrono::Local::now().format("%Y-%m-%d-%H%M%S")
            );
            if let Err(error) = store.add_attachment(
                ponteproj::attachments::Owner::Document(receipt.document_id),
                &name,
                "Foto tirada no celular",
                &bytes,
            ) {
                // No record without its photo: the new one goes to the
                // trash (recoverable there) and the person can try again.
                let _ = store.trash_item(ponteproj::trash::Target {
                    kind: ponteproj::trash::Kind::Document,
                    id: receipt.document_id,
                });
                return Err(error.context("A foto não pôde ser anexada; o registro não foi criado."));
            }
            Ok(json!({ "document_id": receipt.document_id }))
        })
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
        assert!(create_task(&mut store, &json!({"title": ""})).is_err());
        assert!(create_task(&mut store, &json!({"title": "x", "due_date": "31/12"})).is_err());
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
}
