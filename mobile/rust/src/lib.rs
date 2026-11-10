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
        "due_date": task.due_date,
        "due_label": ponteproj::labels::date(&task.due_date),
        "overdue": open && !task.due_date.is_empty() && task.due_date.as_str() < today.as_str(),
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

/// A new task on a document: `{"document_id":1,"title":"...","action":"responder","due_date":"AAAA-MM-DD"}`.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_createTask<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_json_text: JString<'local>,
) -> jstring {
    answer(&mut env, |env| {
        let input = json_arg(env, &task_json_text)?;
        with_store(|store| {
            let document_id = input["document_id"].as_i64().context("Documento da tarefa ausente.")?;
            let mut task = ponteproj::tasks::Task::new(document_id);
            task.title = input["title"].as_str().unwrap_or("").trim().to_owned();
            if let Some(action) = input["action"].as_str().filter(|a| !a.is_empty()) {
                task.action = action.to_owned();
            }
            task.due_date = input["due_date"].as_str().unwrap_or("").trim().to_owned();
            let id = store.save_task(&task)?;
            Ok(json!({ "id": id }))
        })
    })
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
        Ok(json!({"actions": actions, "manual_kinds": kinds}))
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
