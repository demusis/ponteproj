//! JNI entry points of the Ponte core for the Android prototype ("Ponte de
//! bolso"). The phone is one more member of the shared-folder mode: the app
//! copies `ponte-sync.ponte` from Google Drive into its cache, this core runs
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
        with_store(|store| {
            let mut files = vec![path.clone()];
            if !pending.is_empty() && std::path::Path::new(&pending).exists() {
                files.push(pending.clone());
            }
            let mut cfg = store.load_intranet_config()?;
            if cfg.folder_files != files {
                cfg.folder_files = files;
                store.save_intranet_config(&cfg)?;
            }
            let reporter = ponteproj::ai::Reporter::silent();
            let report = match store.sync_with_folders(&cfg, &key, false, &reporter) {
                Ok(report) => report,
                // Joining a library: the phone never creates documents on
                // its own, so with nothing waiting to be sent it simply
                // adopts the copy in the folder.
                Err(error)
                    if error.to_string().starts_with("[confirmar-substituicao]")
                        && store.list_offline_outbox()?.is_empty() =>
                {
                    store.sync_with_folders(&cfg, &key, true, &reporter)?
                }
                Err(error) => return Err(error),
            };
            // Written means: the copy at `transport_path` changed (the
            // pending one is rewritten along with it).
            let written = report
                .written_to
                .iter()
                .any(|w| std::path::Path::new(w) == std::path::Path::new(&path));
            Ok(json!({
                "written": written,
                "summary": report.summary(),
                "waiting": store.list_offline_outbox()?.len(),
                "conflicts": report.conflicts,
            }))
        })
    })
}

/// The documents of the library, newest first, with their notes.
#[no_mangle]
pub extern "system" fn Java_io_github_demusis_ponte_PonteCore_listDocuments<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    answer(&mut env, |_| {
        with_store(|store| {
            let mut documents = Vec::new();
            for summary in store.list_documents("")? {
                // A note that cannot be read is told, never shown as empty
                // (editing an "empty" note would replace the real one).
                let mut entry = json!({
                    "id": summary.id,
                    "number": summary.document_number,
                    "title": summary.title,
                });
                match store.load_document(summary.id) {
                    Ok(detail) => entry["note"] = json!(detail.note),
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
