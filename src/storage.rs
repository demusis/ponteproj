//! The library: SQLite schema, migrations, write guards and the document
//! API. `Store` is the only way in; every stored payload is sealed with DPAPI
//! (`protection::seal`) and every change goes through
//! `intranet::record_mutation`. Expensive reads are memoized until the
//! library changes (`Store::cached`).
use crate::model::*;
use crate::protection::{seal, unseal};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

pub(crate) const SCHEMA_VERSION: i64 = 10;
const MAX_HUMAN_BYTES: usize = 500 * 1024;

pub struct Store {
    pub(crate) conn: Connection,
    pub(crate) suppress_remote: bool,
    pub(crate) actor_override: String,
    /// Mutations applied locally and queued because the host was unreachable,
    /// not yet reported to the person. The UI drains it with
    /// `take_offline_queued` so every such save is announced once.
    pub(crate) offline_queued: usize,
    /// Set by the background synchronization when the host did not answer.
    /// While it lasts, saves are queued at once instead of waiting for a
    /// connection timeout on the interface thread.
    pub(crate) host_unreachable_until: Option<std::time::Instant>,
    /// True while concluding the tasks of joined documents, so the nested
    /// saves do not start cascades of their own.
    pub(crate) cascade_suppressed: bool,
    /// Human-readable results of the last cascade, drained by the UI.
    pub(crate) cascade_notes: Vec<String>,
    /// Results of expensive reads (every row is unsealed with DPAPI), kept
    /// until the library changes. See [`Store::cached`].
    pub(crate) read_cache: std::cell::RefCell<ReadCache>,
}

#[derive(Serialize, Deserialize)]
struct DraftPayload {
    #[serde(default)]
    materials: Vec<crate::evidence::Source>,
    kind: String,
    body: String,
}

/// Memo of expensive reads. Valid while the library is unchanged: `stamp`
/// combines the rows this connection has written (`total_changes()`) with
/// SQLite's `data_version`, which moves when any other connection (the team
/// server thread, a synchronization) commits.
#[derive(Default)]
pub(crate) struct ReadCache {
    stamp: Option<(i64, i64)>,
    values: std::collections::HashMap<String, Box<dyn std::any::Any>>,
}

/// Distinct reads kept at once; a search typed letter by letter must not grow
/// the memo without bound.
const READ_CACHE_ENTRIES: usize = 64;

impl Store {
    /// Library state the read memo is tied to, or `None` when it cannot be
    /// read (the memo is then bypassed).
    fn library_stamp(&self) -> Option<(i64, i64)> {
        self.conn
            .query_row(
                "SELECT total_changes(), (SELECT data_version FROM pragma_data_version)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    /// Returns the value of `load` for `key`, computing it only when the
    /// library changed since it was last computed. `load` must be a pure read
    /// of the library (same data, same result); it may itself use `cached`.
    pub(crate) fn cached<T: Clone + 'static>(
        &self,
        key: &str,
        load: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let Some(stamp) = self.library_stamp() else {
            return load();
        };
        {
            let mut cache = self.read_cache.borrow_mut();
            if cache.stamp != Some(stamp) {
                cache.values.clear();
                cache.stamp = Some(stamp);
            }
            if let Some(value) = cache.values.get(key).and_then(|v| v.downcast_ref::<T>()) {
                return Ok(value.clone());
            }
        }
        let value = load()?;
        let mut cache = self.read_cache.borrow_mut();
        if cache.stamp == Some(stamp) {
            if cache.values.len() >= READ_CACHE_ENTRIES {
                cache.values.clear();
            }
            cache.values.insert(key.to_owned(), Box::new(value.clone()));
        }
        Ok(value)
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).context("Não foi possível criar a pasta do banco.")?;
        }
        let mut conn = Connection::open(path).context("Não foi possível abrir o banco local.")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.create_scalar_function(
            "ponte_client_schema",
            0,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
                | rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
            |_| Ok(SCHEMA_VERSION),
        )?;
        // Check the version before changing any persistent database pragma.
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(
            version <= SCHEMA_VERSION,
            "Banco criado por versão mais recente; atualize o aplicativo antes de abri-lo."
        );
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA secure_delete=ON;",
        )?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(
            version <= SCHEMA_VERSION,
            "Banco criado por versão mais recente."
        );
        if version == 0 {
            let tables: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0))?;
            ensure!(
                tables == 0,
                "Banco sem versão contém tabelas desconhecidas; não será modificado."
            );
            tx.execute_batch(
                "CREATE TABLE documents (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    identity_hash BLOB NOT NULL UNIQUE,
                    latest_capture_id INTEGER,
                    last_seen_at TEXT NOT NULL
                );
                CREATE TABLE captures (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                    content_hash BLOB NOT NULL,
                    payload BLOB NOT NULL,
                    created_at TEXT NOT NULL,
                    UNIQUE(document_id,content_hash)
                );
                CREATE TABLE requests (
                    request_id TEXT PRIMARY KEY,
                    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                    capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE
                );
                CREATE TABLE notes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                    payload BLOB NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE drafts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                    capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE,
                    payload BLOB NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE analyses (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                    capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE,
                    payload BLOB NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX captures_document ON captures(document_id,id);
                CREATE INDEX notes_document ON notes(document_id,id);
                CREATE INDEX drafts_document ON drafts(document_id,id);
                CREATE INDEX analyses_document ON analyses(document_id,id);
                PRAGMA user_version=1;",
            )?;
        }
        tx.commit()?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        let mut store = Self {
            conn,
            suppress_remote: false,
            actor_override: String::new(),
            offline_queued: 0,
            host_unreachable_until: None,
            cascade_suppressed: false,
            cascade_notes: Vec::new(),
            read_cache: Default::default(),
        };
        store.ensure_tasks()?;
        store.ensure_files()?;
        store.ensure_work_context()?;
        store.ensure_page_text()?;
        store.ensure_dossier()?;
        store.ensure_templates()?;
        store.ensure_trash()?;
        store.ensure_intranet()?;
        store.ensure_saiop()?;
        store.ensure_ai_config()?;
        store.ensure_attachments()?;
        store.ensure_write_guards()?;
        // Lazy, idempotent: libraries imported before this feature gain the
        // juntada links too. Unreadable content never prevents opening.
        let _ = store.link_juntadas();
        Ok(store)
    }

    fn ensure_write_guards(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        for table in [
            "documents",
            "captures",
            "requests",
            "notes",
            "drafts",
            "analyses",
            "tasks",
            "work_context",
            "files",
            "page_text",
            "document_links",
            "personal_templates",
            "trash",
            "capture_refs",
            "intranet_config",
            "item_versions",
            "team_events",
            "offline_outbox",
            "offline_conflicts",
            "saiop_batches",
            "saiop_attendances",
            "ai_config",
            "attachments",
        ] {
            for operation in ["INSERT", "UPDATE", "DELETE"] {
                let name = format!("ponte_guard_{table}_{}", operation.to_lowercase());
                tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}; CREATE TRIGGER {name} BEFORE {operation} ON {table} BEGIN SELECT CASE WHEN ponte_client_schema() < {SCHEMA_VERSION} THEN RAISE(ABORT,'Use a versão atual do Ponte para salvar alterações.') END; END;"))?;
            }
        }
        for (table, column) in [
            ("captures", "document_id"),
            ("requests", "document_id"),
            ("notes", "document_id"),
            ("drafts", "document_id"),
            ("analyses", "document_id"),
            ("tasks", "document_id"),
            ("work_context", "document_id"),
            ("capture_refs", "owner_document"),
            ("attachments", "document_id"),
        ] {
            for operation in ["INSERT", "UPDATE"] {
                let name = format!("ponte_active_{table}_{}", operation.to_lowercase());
                tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}; CREATE TRIGGER {name} BEFORE {operation} ON {table} WHEN EXISTS(SELECT 1 FROM trash WHERE kind='document' AND item_id=NEW.{column}) BEGIN SELECT RAISE(ABORT,'Documento na lixeira; restaure antes de editar.'); END;"))?;
            }
        }
        for (table, column, kind) in [
            ("documents", "id", "document"),
            ("captures", "id", "capture"),
            ("tasks", "id", "task"),
            ("files", "capture_id", "capture"),
            ("requests", "capture_id", "capture"),
            ("drafts", "capture_id", "capture"),
            ("analyses", "capture_id", "capture"),
        ] {
            let name = format!("ponte_item_active_{table}");
            tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}; CREATE TRIGGER {name} BEFORE UPDATE ON {table} WHEN EXISTS(SELECT 1 FROM trash WHERE kind='{kind}' AND item_id=NEW.{column}) BEGIN SELECT RAISE(ABORT,'Item na lixeira; restaure antes de editar.'); END;"))?;
        }
        for operation in ["INSERT", "UPDATE"] {
            for (table,condition) in [
                ("files","EXISTS(SELECT 1 FROM captures c JOIN trash t ON (t.kind='document' AND t.item_id=c.document_id) OR (t.kind='capture' AND t.item_id=c.id) WHERE c.id=NEW.capture_id)"),
                ("page_text","EXISTS(SELECT 1 FROM files f JOIN captures c ON c.id=f.capture_id JOIN trash t ON (t.kind='document' AND t.item_id=c.document_id) OR (t.kind='capture' AND t.item_id=c.id) WHERE f.id=NEW.file_id)"),
                ("document_links","EXISTS(SELECT 1 FROM trash t WHERE t.kind='document' AND t.item_id IN (NEW.source_id,NEW.target_id))")
            ] {
                let name=format!("ponte_parent_active_{table}_{}",operation.to_lowercase());
                tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}; CREATE TRIGGER {name} BEFORE {operation} ON {table} WHEN {condition} BEGIN SELECT RAISE(ABORT,'Item ou documento na lixeira; restaure antes de editar.'); END;"))?;
            }
        }
        for (table, kind) in [("drafts", "draft"), ("analyses", "analysis")] {
            let name = format!("ponte_refs_cleanup_{table}");
            tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}; CREATE TRIGGER {name} AFTER DELETE ON {table} BEGIN DELETE FROM capture_refs WHERE owner_kind='{kind}' AND owner_id=OLD.id; END;"))?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Imports a page captured by the extension. A new document or new content
    /// is stored as a version; identical content stores nothing. The first
    /// import of a document also creates its triage task, its automatic
    /// atendimento and its juntada links. Idempotent by `request_id`; on a
    /// client computer it is sent to the main one.
    pub fn import_capture(&mut self, capture: &Capture) -> Result<ImportReceipt> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::ImportCapture {
                    capture: capture.clone(),
                },
            )? {
                crate::intranet::MutationReply::Receipt(r) => return Ok(r),
                _ => anyhow::bail!("Resposta inesperada do anfitrião ao importar captura."),
            }
        }
        self.import_capture_with_files(capture, &[])
    }
    pub(crate) fn import_capture_with_files(
        &mut self,
        capture: &Capture,
        files: &[crate::files::PreparedFile],
    ) -> Result<ImportReceipt> {
        capture.validate()?;
        let actor = self.active_actor();
        let request_key = uuid::Uuid::parse_str(&capture.request_id)?
            .hyphenated()
            .to_string();
        let seen = now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // A manual record created without a number (connected computer, or
        // first creation here) gets the next one now, inside the transaction.
        let pending_manual = capture.is_manual() && capture.document_number == MANUAL_PENDING;
        let assigned;
        let capture: &Capture = if pending_manual {
            ensure!(
                files.is_empty(),
                "Registro manual não leva PDFs importados."
            );
            assigned = with_manual_number(capture, &next_manual_number(&tx)?);
            &assigned
        } else {
            capture
        };
        let identity_hash = identity_hash(capture);
        let content_hash = content_hash(capture)?;
        let payload = encode(capture)?;
        let prior: Option<(i64, i64, Vec<u8>, Vec<u8>)> = tx.query_row(
            "SELECT r.document_id,r.capture_id,d.identity_hash,c.content_hash FROM requests r
             JOIN documents d ON d.id=r.document_id JOIN captures c ON c.id=r.capture_id WHERE r.request_id=?1",
            [&request_key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        ).optional()?;
        if let Some((document_id, capture_id, old_identity, old_content)) = prior {
            // A replayed manual creation was numbered on its first arrival;
            // the stored copy is the one that counts.
            ensure!(
                pending_manual || (old_identity == identity_hash && old_content == content_hash),
                "request_id já utilizado para outra captura; gere um novo UUID."
            );
            crate::trash::require_active_capture(&tx, capture_id)?;
            tx.commit()?;
            return Ok(ImportReceipt {
                request_id: capture.request_id.clone(),
                document_id,
                capture_id,
                duplicate: true,
                outcome: Some(ImportOutcome::Confirmed),
                existing_captured_at: None,
                previous_version: false,
            });
        }
        let mut new_document = false;
        let document_id: i64 = match tx
            .query_row(
                "SELECT id FROM documents WHERE identity_hash=?1",
                [&identity_hash],
                |row| row.get(0),
            )
            .optional()?
        {
            Some(id) => id,
            None => {
                tx.execute(
                    "INSERT INTO documents(identity_hash,last_seen_at) VALUES(?1,?2)",
                    params![identity_hash, seen],
                )?;
                new_document = true;
                tx.last_insert_rowid()
            }
        };
        crate::trash::require_active_document(&tx, document_id)?;
        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT id,created_at FROM captures WHERE document_id=?1 AND content_hash=?2",
                params![document_id, content_hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((id, _)) = &existing {
            crate::trash::require_active_capture(&tx, *id)?;
        }
        let latest: Option<i64> = tx
            .query_row(
                "SELECT latest_capture_id FROM documents WHERE id=?1",
                [document_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if let Some((capture_id, created_at)) = &existing {
            if latest == Some(*capture_id) {
                // The page shows exactly what is already current: nothing to
                // store, not even the request. The person is warned instead.
                tx.commit()?;
                return Ok(ImportReceipt {
                    request_id: capture.request_id.clone(),
                    document_id,
                    capture_id: *capture_id,
                    duplicate: true,
                    outcome: Some(ImportOutcome::Unchanged),
                    existing_captured_at: Some(created_at.clone()),
                    previous_version: false,
                });
            }
        }
        // Identical to an older version: that version becomes current again,
        // without storing anything new.
        let duplicate = existing.is_some();
        let capture_id = match &existing {
            Some((id, _)) => *id,
            None => {
                tx.execute("INSERT INTO captures(document_id,content_hash,payload,created_at) VALUES(?1,?2,?3,?4)", params![document_id, content_hash, payload, seen])?;
                tx.last_insert_rowid()
            }
        };
        tx.execute(
            "UPDATE documents SET latest_capture_id=?1,last_seen_at=?2 WHERE id=?3",
            params![capture_id, seen, document_id],
        )?;
        tx.execute(
            "INSERT INTO requests(request_id,document_id,capture_id) VALUES(?1,?2,?3)",
            params![request_key, document_id, capture_id],
        )?;
        crate::tasks::ensure_document_task(&tx, document_id)?;
        if !duplicate {
            crate::files::save_files(&tx, capture_id, files)?;
            crate::intranet::record_mutation(
                &tx,
                &actor,
                "document",
                document_id,
                None,
                "import_capture",
                Some(document_id),
                &format!(
                    "Importou {} (captura #{capture_id})",
                    capture.document_number
                ),
            )?;
        }
        tx.commit()
            .context("Falha ao confirmar a captura; tente novamente com o mesmo request_id.")?;
        // The capture is already committed; a failure here must not undo the
        // import. The next open/import retries the idempotent reconciliation.
        let _ = self.link_juntadas();
        // First arrival of a SIGADOC document: its passage through the
        // cabinet is recorded in Atendimentos (when the person wants that).
        // Runs on whichever computer holds the library, never queued on
        // its own: a replay of the import recreates it deterministically.
        if new_document && !duplicate && !capture.is_manual() {
            let _ = self.attendance_for_import(document_id, capture);
        }
        Ok(ImportReceipt {
            request_id: capture.request_id.clone(),
            document_id,
            capture_id,
            duplicate,
            outcome: Some(if duplicate {
                ImportOutcome::Unchanged
            } else {
                ImportOutcome::Imported
            }),
            existing_captured_at: existing.map(|(_, created_at)| created_at),
            previous_version: duplicate,
        })
    }

    /// Active documents matching `query` (empty: all), memoized per query
    /// until the library changes.
    pub fn list_documents(&self, query: &str) -> Result<Vec<DocumentSummary>> {
        self.cached(&format!("documents\u{1}{query}"), || {
            self.list_documents_uncached(query)
        })
    }
    fn list_documents_uncached(&self, query: &str) -> Result<Vec<DocumentSummary>> {
        ensure!(query.len() <= 8192, "Busca grande demais.");
        let terms = tokens(query);
        let mut out = Vec::new();
        let mut details = self
            .document_ids()?
            .into_iter()
            .map(|id| self.load_document(id))
            .collect::<Result<Vec<_>>>()?;
        // Juntada relations are computed over the whole active library so that
        // a search or filter never changes a parent's count of joined pieces.
        let by_number: BTreeMap<String, i64> = details
            .iter()
            .map(|d| {
                (
                    d.summary.document_number.trim().to_uppercase(),
                    d.summary.id,
                )
            })
            .collect();
        let mut children = BTreeMap::<i64, usize>::new();
        for detail in &mut details {
            let summary = &mut detail.summary;
            summary.joined_to_id = summary
                .joined_to_number
                .as_ref()
                .and_then(|number| by_number.get(number).copied())
                .filter(|parent| *parent != summary.id);
            if let Some(parent) = summary.joined_to_id {
                *children.entry(parent).or_default() += 1;
            }
        }
        for mut detail in details {
            detail.summary.joined_children_count =
                children.get(&detail.summary.id).copied().unwrap_or(0);
            if terms.is_empty() || {
                let searchable = normalize(&search_text(&detail));
                let mut matched = terms
                    .iter()
                    .map(|term| searchable.contains(term))
                    .collect::<Vec<_>>();
                if !matched.iter().all(|value| *value) {
                    for file in self.list_files(detail.summary.latest_capture_id)? {
                        for (_, page) in self.page_texts_for_file(file.id)? {
                            let text = normalize(&page.text);
                            for (index, term) in terms.iter().enumerate() {
                                matched[index] |= text.contains(term);
                            }
                        }
                        if matched.iter().all(|value| *value) {
                            break;
                        }
                    }
                }
                matched.iter().all(|value| *value)
            } {
                out.push(detail.summary);
            }
        }
        Ok(out)
    }

    pub fn load_document(&self, id: i64) -> Result<DocumentDetail> {
        crate::trash::require_active_document(&self.conn, id)?;
        let (latest_capture_id, last_seen_at, blob): (i64, String, Vec<u8>) = self.conn.query_row(
            "SELECT d.latest_capture_id,d.last_seen_at,c.payload FROM documents d JOIN captures c ON c.id=d.latest_capture_id AND c.document_id=d.id WHERE d.id=?1",
            [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        ).optional()?.context("Documento não encontrado.")?;
        let capture: Capture = decode(&blob)?;
        let note: String = self
            .conn
            .query_row(
                "SELECT payload FROM notes WHERE document_id=?1 ORDER BY id DESC LIMIT 1",
                [id],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();
        let analysis: Option<Analysis> = self
            .conn
            .query_row(
                "SELECT payload FROM analyses WHERE document_id=?1 ORDER BY id DESC LIMIT 1",
                [id],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .map(|bytes| decode(&bytes))
            .transpose()?;
        let mut stmt = self.conn.prepare("SELECT id,capture_id,payload,created_at FROM drafts WHERE document_id=?1 ORDER BY id DESC")?;
        let rows = stmt.query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut drafts = Vec::new();
        for row in rows {
            let (draft_id, capture_id, bytes, created_at) = row?;
            let payload: DraftPayload = decode(&bytes)?;
            drafts.push(DraftVersion {
                id: draft_id,
                kind: payload.kind,
                body: payload.body,
                capture_id,
                created_at,
                stale: capture_id != latest_capture_id || self.sources_stale(&payload.materials),
                materials: payload.materials,
            });
        }
        let work_context = self.load_work_context(id)?;
        let juntada = crate::dossier::detect_juntada(&capture);
        let joined_to_number = juntada
            .as_ref()
            .and_then(|j| j.parent_document_number.clone());
        let joined_to_id = match &joined_to_number {
            Some(number) => self.active_document_by_number(number)?.filter(|p| *p != id),
            None => None,
        };
        let summary = DocumentSummary {
            is_joined: juntada.is_some(),
            joined_to_number,
            joined_to_id,
            // Requires the whole library; filled by list_documents/joined_children.
            joined_children_count: 0,
            manual: capture.is_manual(),
            id,
            document_number: capture.document_number.clone(),
            title: capture.title.clone(),
            last_seen_at,
            latest_capture_id,
            subjects: if !work_context.subjects.trim().is_empty() {
                work_context
                    .subjects
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            } else {
                analysis
                    .as_ref()
                    .map(|a| a.subjects.clone())
                    .unwrap_or_default()
            },
            priority: if !work_context.priority.is_empty() {
                work_context.priority.clone()
            } else {
                analysis
                    .as_ref()
                    .map(|a| a.priority.clone())
                    .unwrap_or_else(|| "não classificada".into())
            },
            analysis_stale: analysis.as_ref().is_some_and(|a| {
                a.capture_id != latest_capture_id
                    || self.sources_stale(&a.materials)
                    || work_context.analysis_text(&note) != a.human_context
            }),
        };
        Ok(DocumentDetail {
            summary,
            capture,
            note,
            drafts,
            analysis,
            work_context,
        })
    }

    pub fn save_note(&mut self, id: i64, note: &str) -> Result<()> {
        self.save_note_checked(id, note, None)
    }

    pub fn save_note_checked(
        &mut self,
        id: i64,
        note: &str,
        expected_version: Option<i64>,
    ) -> Result<()> {
        ensure!(note.len() <= MAX_HUMAN_BYTES, "Nota excede 500 KiB.");
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveNote {
                    document_id: id,
                    note: note.to_owned(),
                    expected_version,
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let payload = encode(&note)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_document(&tx, id)?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "note",
            id,
            expected_version,
            "save_note",
            Some(id),
            &format!("Atualizou notas do documento #{id}"),
        )?;
        tx.execute(
            "INSERT INTO notes(document_id,payload,created_at) VALUES(?1,?2,?3)",
            params![id, payload, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn save_draft(&mut self, id: i64, capture_id: i64, kind: &str, body: &str) -> Result<i64> {
        self.save_draft_with_sources(id, capture_id, kind, body, &[])
    }
    pub fn save_draft_with_sources(
        &mut self,
        id: i64,
        capture_id: i64,
        kind: &str,
        body: &str,
        materials: &[crate::evidence::Source],
    ) -> Result<i64> {
        crate::evidence::validate(materials, &[])?;
        ensure!(valid_kind(kind), "Tipo de minuta inválido.");
        ensure!(
            !body.trim().is_empty() && body.len() <= MAX_HUMAN_BYTES,
            "Minuta vazia ou excede 500 KiB."
        );
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveDraft {
                    document_id: id,
                    capture_id,
                    kind: kind.to_owned(),
                    body: body.to_owned(),
                    materials: materials.to_vec(),
                },
            )? {
                crate::intranet::MutationReply::Id(draft_id) => return Ok(draft_id),
                _ => anyhow::bail!("Resposta inesperada do anfitrião ao salvar minuta."),
            }
        }
        let actor = self.active_actor();
        let payload = encode(&DraftPayload {
            materials: materials.to_vec(),
            kind: kind.into(),
            body: body.into(),
        })?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_capture(&tx, id, capture_id)?;
        tx.execute(
            "INSERT INTO drafts(document_id,capture_id,payload,created_at) VALUES(?1,?2,?3,?4)",
            params![id, capture_id, payload, now()],
        )?;
        let draft_id = tx.last_insert_rowid();
        crate::trash::register_refs(&tx, "draft", draft_id, id, capture_id, materials)?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "draft",
            draft_id,
            None,
            "save_draft",
            Some(id),
            &format!("Salvou minuta ({kind}) versão #{draft_id}"),
        )?;
        tx.commit()?;
        Ok(draft_id)
    }

    pub fn save_analysis(&mut self, id: i64, analysis: &Analysis) -> Result<()> {
        validate_analysis(analysis)?;
        crate::evidence::validate(&analysis.materials, &analysis.citations)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveAnalysis {
                    document_id: id,
                    analysis: analysis.clone(),
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let payload = encode(analysis)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_capture(&tx, id, analysis.capture_id)?;
        tx.execute(
            "INSERT INTO analyses(document_id,capture_id,payload,created_at) VALUES(?1,?2,?3,?4)",
            params![id, analysis.capture_id, payload, analysis.created_at],
        )?;
        let analysis_id = tx.last_insert_rowid();
        crate::trash::register_refs(
            &tx,
            "analysis",
            analysis_id,
            id,
            analysis.capture_id,
            &analysis.materials,
        )?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "analysis",
            analysis_id,
            None,
            "save_analysis",
            Some(id),
            &format!("Registrou análise assistida no documento #{id}"),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes a document and everything it holds for good, bypassing the
    /// trash (the interface uses `trash_item`). Goes through
    /// `record_mutation`; on a client computer it is sent to the main one.
    pub fn delete_document(&mut self, id: i64) -> Result<()> {
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::DeleteDocument { id },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            tx.execute("DELETE FROM documents WHERE id=?1", [id])? == 1,
            "Documento não encontrado."
        );
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "document",
            id,
            None,
            "delete_document",
            Some(id),
            &format!("Removeu documento #{id}"),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn backup(&self, path: &Path) -> Result<()> {
        ensure!(
            !path.exists(),
            "Destino de backup já existe; escolha outro arquivo."
        );
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).context("Não foi possível criar a pasta de backup.")?;
        }
        // SQLite's online backup includes committed WAL pages as a consistent snapshot.
        self.conn
            .backup("main", path, rusqlite::backup::NO_PROGRESS)
            .context("Não foi possível criar um backup consistente.")?;
        Ok(())
    }

    /// Documents whose text resembles document `id`, memoized until the
    /// library changes.
    pub fn similar_documents(&self, id: i64, limit: usize) -> Result<Vec<DocumentSummary>> {
        self.cached(&format!("similar\u{1}{id}\u{1}{limit}"), || {
            self.similar_documents_uncached(id, limit)
        })
    }
    fn similar_documents_uncached(&self, id: i64, limit: usize) -> Result<Vec<DocumentSummary>> {
        let target = self.load_document(id)?;
        if limit == 0 {
            return Ok(vec![]);
        }
        let mut documents = vec![target];
        for other in self
            .document_ids()?
            .into_iter()
            .filter(|other| *other != id)
        {
            documents.push(self.load_document(other)?);
        }
        let term_sets: Vec<BTreeMap<String, usize>> = documents
            .iter()
            .map(|d| lexical_terms(&similarity_text(d)))
            .collect();
        let mut frequency = BTreeMap::<String, usize>::new();
        for terms in &term_sets {
            for term in terms.keys() {
                *frequency.entry(term.clone()).or_default() += 1;
            }
        }
        let vectors: Vec<BTreeMap<String, f64>> = term_sets
            .iter()
            .map(|terms| {
                terms
                    .iter()
                    .map(|(word, count)| {
                        let idf = 1.0
                            + ((documents.len() as f64 + 1.0)
                                / (*frequency.get(word).unwrap_or(&1) as f64 + 1.0))
                                .ln();
                        (word.clone(), (1.0 + (*count as f64).ln()) * idf)
                    })
                    .collect()
            })
            .collect();
        let norm = |v: &BTreeMap<String, f64>| v.values().map(|x| x * x).sum::<f64>().sqrt();
        let target_norm = norm(&vectors[0]);
        if target_norm == 0.0 {
            return Ok(vec![]);
        }
        let mut scored = Vec::new();
        for index in 1..documents.len() {
            let dot = vectors[0]
                .iter()
                .map(|(word, value)| value * vectors[index].get(word).unwrap_or(&0.0))
                .sum::<f64>();
            let denominator = target_norm * norm(&vectors[index]);
            if dot > 0.0 && denominator > 0.0 {
                scored.push((dot / denominator, index));
            }
        }
        scored.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| documents[a.1].summary.id.cmp(&documents[b.1].summary.id))
        });
        Ok(scored
            .into_iter()
            .take(limit.min(50))
            .map(|(_, index)| documents[index].summary.clone())
            .collect())
    }

    /// Number of saves queued for the host since the last call.
    pub fn take_offline_queued(&mut self) -> usize {
        std::mem::take(&mut self.offline_queued)
    }

    /// What the last conclusion cascaded to joined documents, if anything.
    pub fn take_cascade_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.cascade_notes)
    }

    /// Latest result of the background synchronization. An unreachable host
    /// is remembered for a few seconds, so the next save is queued at once.
    pub fn note_host_reachability(&mut self, reachable: bool) {
        self.host_unreachable_until =
            (!reachable).then(|| std::time::Instant::now() + std::time::Duration::from_secs(12));
    }

    pub(crate) fn host_known_unreachable(&self) -> bool {
        self.host_unreachable_until
            .is_some_and(|until| std::time::Instant::now() < until)
    }

    /// Active (not trashed) local document with this official number.
    pub(crate) fn active_document_by_number(&self, number: &str) -> Result<Option<i64>> {
        let hash = identity_hash_for_number(number);
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT d.id FROM documents d WHERE d.identity_hash=?1 AND d.latest_capture_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='document' AND t.item_id=d.id)",
                [hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    pub(crate) fn document_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT d.id FROM documents d WHERE NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='document' AND t.item_id=d.id) ORDER BY d.last_seen_at DESC,d.id DESC")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }
}

fn require_document(conn: &Connection, id: i64) -> Result<()> {
    ensure!(
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE id=?1)",
            [id],
            |row| row.get::<_, bool>(0)
        )?,
        "Documento não encontrado."
    );
    crate::trash::require_active_document(conn, id)?;
    Ok(())
}

fn require_capture(conn: &Connection, id: i64, capture_id: i64) -> Result<()> {
    ensure!(
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM captures WHERE document_id=?1 AND id=?2)",
            params![id, capture_id],
            |row| row.get::<_, bool>(0)
        )?,
        "A captura não pertence a este documento."
    );
    ensure!(
        crate::trash::require_active_capture(conn, capture_id)? == id,
        "A captura não pertence ao documento."
    );
    Ok(())
}

fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    seal(&serde_json::to_vec(value)?)
        .context("Não foi possível proteger o conteúdo para a conta Windows atual.")
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let plain = unseal(bytes)
        .context("Não foi possível abrir conteúdo protegido; use a conta Windows que o gravou.")?;
    serde_json::from_slice(&plain)
        .context("Conteúdo protegido está corrompido ou tem formato incompatível.")
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn identity_hash(capture: &Capture) -> Vec<u8> {
    identity_hash_for_number(&capture.document_number)
}

fn identity_hash_for_number(number: &str) -> Vec<u8> {
    let number = number.split_whitespace().collect::<String>().to_uppercase();
    // Manual records live under their own origin: the same digits in an
    // official number name a different document.
    let origin = if is_manual_number(&number) {
        "ponte://manual"
    } else {
        "https://www.sigadoc.mt.gov.br"
    };
    Sha256::digest(format!("{origin}\n{number}").as_bytes()).to_vec()
}

/// Next free `MANUAL-AAAA/NNNN` for this year, looking at every document
/// (trashed ones included, so a number is never reused). Runs inside the
/// import transaction, so two creations cannot pick the same number.
fn next_manual_number(tx: &rusqlite::Transaction<'_>) -> Result<String> {
    let year = chrono::Local::now().format("%Y").to_string();
    let prefix = format!("{MANUAL_PREFIX}{year}/");
    let mut statement = tx
        .prepare("SELECT c.payload FROM documents d JOIN captures c ON c.id=d.latest_capture_id")?;
    let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
    let mut highest = 0_u32;
    for row in rows {
        let capture: Capture = decode(&row?)?;
        if let Some(rest) = capture
            .document_number
            .trim()
            .to_uppercase()
            .strip_prefix(&prefix)
        {
            if let Ok(sequence) = rest.parse::<u32>() {
                highest = highest.max(sequence);
            }
        }
    }
    Ok(format!("{prefix}{:04}", highest + 1))
}

fn with_manual_number(capture: &Capture, number: &str) -> Capture {
    let mut assigned = capture.clone();
    assigned.document_number = number.to_owned();
    assigned.source_url = format!("{MANUAL_ORIGIN}{number}");
    assigned
}

fn content_hash(capture: &Capture) -> Result<Vec<u8>> {
    let mut canonical = capture.clone();
    canonical.request_id.clear();
    canonical.captured_at.clear();
    canonical.document_number = capture
        .document_number
        .split_whitespace()
        .collect::<String>()
        .to_uppercase();
    // Manual records carry their own origin, not a SIGADOC address.
    if !canonical.is_manual() {
        canonical.source_url = canonical_url(&canonical.source_url)?;
    }
    for reference in &mut canonical.references {
        reference.url = canonical_url(&reference.url)?;
    }
    canonical
        .references
        .sort_by(|a, b| (&a.kind, &a.url, &a.label).cmp(&(&b.kind, &b.url, &b.label)));
    canonical.coverage.recognized.sort();
    canonical.coverage.recognized.dedup();
    canonical.coverage.pending.sort();
    canonical.coverage.pending.dedup();
    Ok(Sha256::digest(serde_json::to_vec(&canonical)?).to_vec())
}

fn canonical_url(value: &str) -> Result<String> {
    let mut url = validate_sigadoc_url(value)?;
    let mut pairs = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    pairs.sort();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    Ok(url.into())
}

pub(crate) fn validate_analysis(analysis: &Analysis) -> Result<()> {
    ensure!(
        analysis.human_context.len() <= 32 * 1024
            && analysis.suggested_actions.len() <= 8
            && analysis.missing_context.len() <= 16,
        "Contexto ou sugestões excedem o limite."
    );
    for action in &analysis.suggested_actions {
        ensure!(
            crate::tasks::ACTIONS.contains(&action.action.as_str())
                && !action.reason.trim().is_empty()
                && action.reason.len() <= 2000
                && !action.evidence.trim().is_empty()
                && action.evidence.len() <= 2000,
            "Providência sugerida inválida ou sem justificativa/base."
        );
    }
    ensure!(
        analysis
            .missing_context
            .iter()
            .all(|s| !s.trim().is_empty() && s.len() <= 2000),
        "Pendência de contexto inválida."
    );
    ensure!(
        analysis.capture_id > 0,
        "Análise sem captura de origem válida."
    );
    ensure!(
        matches!(
            analysis.priority.as_str(),
            "baixa" | "normal" | "alta" | "urgente"
        ),
        "Prioridade de análise inválida."
    );
    ensure!(
        analysis.subjects.len() <= 32
            && analysis
                .subjects
                .iter()
                .all(|s| !s.trim().is_empty() && s.len() <= 500),
        "Assuntos de análise inválidos."
    );
    ensure!(
        !analysis.reason.trim().is_empty()
            && analysis.reason.len() <= 16 * 1024
            && analysis.summary.len() <= 64 * 1024
            && !analysis.summary.trim().is_empty(),
        "Resumo ou justificativa de análise vazio ou grande demais."
    );
    ensure!(
        analysis
            .deadline_candidate
            .as_ref()
            .is_none_or(|s| !s.trim().is_empty() && s.len() <= 2048),
        "Prazo candidato inválido."
    );
    ensure!(
        !analysis.model.trim().is_empty() && analysis.model.len() <= 512,
        "Modelo da análise inválido."
    );
    DateTime::parse_from_rfc3339(&analysis.created_at).context("Data da análise inválida.")?;
    ensure!(
        serde_json::to_vec(analysis)?.len() <= 128 * 1024,
        "Análise grande demais."
    );
    Ok(())
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

fn tokens(value: &str) -> BTreeSet<String> {
    normalize(value)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn search_text(detail: &DocumentDetail) -> String {
    // This string exists only in memory; no plaintext search index reaches SQLite.
    let mut value = format!(
        "{} {} {} {}",
        detail.capture.document_number, detail.capture.title, detail.capture.text, detail.note
    );
    for (key, field) in &detail.capture.metadata {
        value.push(' ');
        value.push_str(key);
        value.push(' ');
        value.push_str(field);
    }
    value.push(' ');
    value.push_str(&detail.work_context.analysis_text(""));
    for reference in &detail.capture.references {
        value.push(' ');
        value.push_str(&reference.label);
        value.push(' ');
        value.push_str(&reference.url);
    }
    for field in detail
        .capture
        .coverage
        .recognized
        .iter()
        .chain(&detail.capture.coverage.pending)
    {
        value.push(' ');
        value.push_str(field);
    }
    for draft in &detail.drafts {
        value.push(' ');
        value.push_str(&draft.body);
    }
    if let Some(analysis) = &detail.analysis {
        for field in analysis
            .subjects
            .iter()
            .chain([&analysis.summary, &analysis.reason, &analysis.priority])
            .chain(analysis.deadline_candidate.iter())
        {
            value.push(' ');
            value.push_str(field);
        }
    }
    value
}

fn similarity_text(detail: &DocumentDetail) -> String {
    let mut text = format!(
        "{} {} {}",
        detail.capture.title, detail.capture.title, detail.capture.text
    );
    for value in detail.capture.metadata.values() {
        text.push(' ');
        text.push_str(value);
    }
    text
}

fn lexical_terms(value: &str) -> BTreeMap<String, usize> {
    const STOP: &[&str] = &[
        "para",
        "pela",
        "pelo",
        "pelas",
        "pelos",
        "como",
        "com",
        "uma",
        "umas",
        "uns",
        "dos",
        "das",
        "que",
        "de",
        "da",
        "do",
        "em",
        "ao",
        "aos",
        "as",
        "os",
        "se",
        "na",
        "no",
        "nas",
        "nos",
        "por",
        "ser",
        "sao",
        "foi",
        "este",
        "esta",
        "esse",
        "essa",
        "isso",
        "isto",
        "entre",
        "sobre",
        "mais",
        "sem",
        "seu",
        "sua",
        "seus",
        "suas",
        "documento",
        "processo",
        "sigadoc",
        "solicitacao",
    ];
    let mut terms = BTreeMap::new();
    for term in normalize(value).split(|c: char| !c.is_alphanumeric()) {
        if term.chars().count() >= 3
            && !STOP.contains(&term)
            && !term.chars().all(|c| c.is_ascii_digit())
        {
            *terms.entry(term.to_owned()).or_default() += 1;
        }
    }
    terms
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let db = Store::open(&dir.path().join("documents.sqlite3")).unwrap();
        (dir, db)
    }

    #[test]
    fn memoized_reads_follow_writes_from_this_and_other_connections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite3");
        let mut here = Store::open(&path).unwrap();
        let mut there = Store::open(&path).unwrap();
        let receipt = here.import_capture(&sample_capture()).unwrap();
        let before = here.list_tasks().unwrap().len();
        // Same answer while nothing changes (served from memory).
        assert_eq!(here.list_tasks().unwrap().len(), before);

        // A write on this connection is seen at once.
        let mut task = crate::tasks::Task::new(receipt.document_id);
        task.title = "Daqui".into();
        here.save_task(&task).unwrap();
        assert_eq!(here.list_tasks().unwrap().len(), before + 1);

        // So is a write committed by another connection (the team server).
        task.title = "De lá".into();
        there.save_task(&task).unwrap();
        let titles: Vec<String> = here
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles.len(), before + 2);
        assert!(titles.iter().any(|t| t == "De lá"), "{titles:?}");

        // And a change through raw SQL (a migration, a test) as well.
        here.conn
            .execute("DELETE FROM tasks WHERE id=(SELECT max(id) FROM tasks)", [])
            .unwrap();
        assert_eq!(here.list_tasks().unwrap().len(), before + 1);
        // Searches are kept apart.
        assert_eq!(here.list_documents("").unwrap().len(), 1);
        assert!(here
            .list_documents("palavra-que-nao-existe")
            .unwrap()
            .is_empty());
        assert_eq!(here.list_documents("").unwrap().len(), 1);
    }

    fn analysis(capture_id: i64) -> Analysis {
        Analysis {
            capture_id,
            subjects: vec!["material".into()],
            priority: "normal".into(),
            reason: "Sem prazo expresso.".into(),
            deadline_candidate: None,
            summary: "Solicitação de material.".into(),
            model: "synthetic-test".into(),
            created_at: now(),
            ..Default::default()
        }
    }

    #[test]
    fn lost_ack_and_new_request_reuse_the_immutable_capture() {
        let (_dir, mut db) = store();
        let mut c = sample_capture();
        let first = db.import_capture(&c).unwrap();
        assert!(!first.duplicate);
        assert_eq!(first.outcome, Some(ImportOutcome::Imported));
        let retry = db.import_capture(&c).unwrap();
        assert!(retry.duplicate);
        assert_eq!(retry.outcome, Some(ImportOutcome::Confirmed));
        let requests = |db: &Store| {
            db.conn
                .query_row("SELECT count(*) FROM requests", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let seen_before = db
            .load_document(first.document_id)
            .unwrap()
            .summary
            .last_seen_at;
        c.request_id = uuid::Uuid::new_v4().to_string();
        c.captured_at = "2026-11-02T12:00:00Z".into();
        let repeat = db.import_capture(&c).unwrap();
        // Same content, new request: refused as unchanged, nothing written.
        assert!(repeat.duplicate);
        assert_eq!(repeat.outcome, Some(ImportOutcome::Unchanged));
        assert!(!repeat.previous_version);
        assert!(repeat.existing_captured_at.is_some());
        assert_eq!(first.capture_id, repeat.capture_id);
        assert_eq!(first.document_id, repeat.document_id);
        assert_eq!(requests(&db), 1, "an ignored import leaves no trace");
        let detail = db.load_document(first.document_id).unwrap();
        assert_eq!(detail.capture.captured_at, "2026-11-01T12:00:00Z");
        assert_eq!(detail.summary.last_seen_at, seen_before);
        // The first request's UUID cannot be reused for other content.
        c.request_id = first.request_id.clone();
        c.text.push_str(" Novo conteúdo com UUID repetido.");
        assert!(db.import_capture(&c).is_err());
        assert_eq!(db.list_documents("").unwrap().len(), 1);
    }

    #[test]
    fn identical_content_is_ignored_and_an_older_version_can_become_current_again() {
        let (_dir, mut db) = store();
        let mut a = sample_capture();
        let first = db.import_capture(&a).unwrap();
        let mut b = a.clone();
        b.request_id = uuid::Uuid::new_v4().to_string();
        b.text.push_str(" Despacho acrescentado.");
        let second = db.import_capture(&b).unwrap();
        assert_eq!(second.outcome, Some(ImportOutcome::Imported));
        assert_ne!(second.capture_id, first.capture_id);
        let requests = |db: &Store| {
            db.conn
                .query_row("SELECT count(*) FROM requests", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let captures = |db: &Store| {
            db.conn
                .query_row("SELECT count(*) FROM captures", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        assert_eq!((captures(&db), requests(&db)), (2, 2));

        // The page shows the older version again: it becomes current, nothing new stored.
        a.request_id = uuid::Uuid::new_v4().to_string();
        let back = db.import_capture(&a).unwrap();
        assert_eq!(back.outcome, Some(ImportOutcome::Unchanged));
        assert!(back.previous_version);
        assert_eq!(back.capture_id, first.capture_id);
        assert_eq!(
            db.load_document(first.document_id)
                .unwrap()
                .summary
                .latest_capture_id,
            first.capture_id
        );
        assert_eq!((captures(&db), requests(&db)), (2, 3));

        // Once more, identical to what is now current: ignored outright.
        let seen = db
            .load_document(first.document_id)
            .unwrap()
            .summary
            .last_seen_at;
        a.request_id = uuid::Uuid::new_v4().to_string();
        let again = db.import_capture(&a).unwrap();
        assert_eq!(again.outcome, Some(ImportOutcome::Unchanged));
        assert!(!again.previous_version);
        let stored_at: String = db
            .conn
            .query_row(
                "SELECT created_at FROM captures WHERE id=?1",
                [first.capture_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            again.existing_captured_at.as_deref(),
            Some(stored_at.as_str())
        );
        assert_eq!((captures(&db), requests(&db)), (2, 3));
        assert_eq!(
            db.load_document(first.document_id)
                .unwrap()
                .summary
                .last_seen_at,
            seen
        );

        // A receipt from an older team host (no outcome) still behaves as before.
        let legacy: ImportReceipt = serde_json::from_str(
            r#"{"request_id":"x","document_id":1,"capture_id":1,"duplicate":true}"#,
        )
        .unwrap();
        assert_eq!(legacy.effective_outcome(), ImportOutcome::Confirmed);
        assert!(!legacy.previous_version);
    }

    #[test]
    fn new_information_keeps_human_versions_and_marks_old_derivatives_stale() {
        let (_dir, mut db) = store();
        let mut c = sample_capture();
        let first = db.import_capture(&c).unwrap();
        db.save_note(first.document_id, "Nota humana preservada")
            .unwrap();
        db.save_note(first.document_id, "Correção humana preservada")
            .unwrap();
        db.save_draft(
            first.document_id,
            first.capture_id,
            "resposta",
            "Minuta humana",
        )
        .unwrap();
        db.save_analysis(first.document_id, &analysis(first.capture_id))
            .unwrap();
        c.request_id = uuid::Uuid::new_v4().to_string();
        c.text.push_str(" Prazo informado: 10/11/2026.");
        let changed = db.import_capture(&c).unwrap();
        assert_ne!(first.capture_id, changed.capture_id);
        assert!(!changed.duplicate);
        assert_eq!(changed.outcome, Some(ImportOutcome::Imported));
        let detail = db.load_document(first.document_id).unwrap();
        assert_eq!(detail.note, "Correção humana preservada");
        assert!(detail.drafts[0].stale);
        assert!(detail.summary.analysis_stale);
        assert_eq!(detail.analysis.unwrap().capture_id, first.capture_id);
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM notes", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        // Retrying the original acknowledged request cannot roll back the latest observation.
        let original: Capture = decode(
            &db.conn
                .query_row(
                    "SELECT payload FROM captures WHERE id=?1",
                    [first.capture_id],
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            db.import_capture(&original).unwrap().outcome,
            Some(ImportOutcome::Confirmed)
        );
        assert_eq!(
            db.load_document(first.document_id)
                .unwrap()
                .summary
                .latest_capture_id,
            changed.capture_id
        );
    }

    #[test]
    fn all_documentary_content_is_encrypted_and_backup_is_complete() {
        let (dir, mut db) = store();
        let mut c = sample_capture();
        c.title = "MARCADOR_TITULO_CONFIDENCIAL".into();
        c.text = "MARCADOR_TEXTO_CONFIDENCIAL".into();
        let receipt = db.import_capture(&c).unwrap();
        db.save_note(receipt.document_id, "MARCADOR_NOTA_CONFIDENCIAL")
            .unwrap();
        db.save_draft(
            receipt.document_id,
            receipt.capture_id,
            "resposta",
            "MARCADOR_MINUTA_CONFIDENCIAL",
        )
        .unwrap();
        let mut a = analysis(receipt.capture_id);
        a.summary = "MARCADOR_ANALISE_CONFIDENCIAL".into();
        db.save_analysis(receipt.document_id, &a).unwrap();
        let backup = dir.path().join("backup.sqlite3");
        db.backup(&backup).unwrap();
        let mut restored = Store::open(&backup).unwrap();
        assert_eq!(
            restored.load_document(receipt.document_id).unwrap().note,
            "MARCADOR_NOTA_CONFIDENCIAL"
        );
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(path).unwrap();
            for marker in [
                c.title.as_str(),
                c.text.as_str(),
                c.document_number.as_str(),
                "MARCADOR_NOTA_CONFIDENCIAL",
                "MARCADOR_MINUTA_CONFIDENCIAL",
                "MARCADOR_ANALISE_CONFIDENCIAL",
            ] {
                assert!(
                    !bytes
                        .windows(marker.len())
                        .any(|window| window == marker.as_bytes()),
                    "plaintext marker found"
                );
            }
        }
        assert!(db.backup(&backup).is_err());
        restored.delete_document(receipt.document_id).unwrap();
        for table in [
            "documents",
            "captures",
            "requests",
            "notes",
            "drafts",
            "analyses",
        ] {
            assert_eq!(
                restored
                    .conn
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn future_schema_is_rejected_without_overwriting_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 42).unwrap();
        drop(conn);
        assert!(Store::open(&path).is_err());
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            42
        );
    }

    #[test]
    fn search_and_similar_documents_use_decrypted_content_and_prevent_cross_document_derivatives() {
        let (_dir, mut db) = store();
        let c = sample_capture();
        let first = db.import_capture(&c).unwrap();
        let mut related = c.clone();
        related.request_id = uuid::Uuid::new_v4().to_string();
        related.document_number = "SEFAZ-PRO-2026/00002".into();
        related.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00002"
                .into();
        related.text = "Aquisição de papel para escritório e material de expediente.".into();
        let second = db.import_capture(&related).unwrap();
        let mut unrelated = related;
        unrelated.request_id = uuid::Uuid::new_v4().to_string();
        unrelated.document_number = "SEFAZ-PRO-2026/00003".into();
        unrelated.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00003"
                .into();
        unrelated.title = "Pavimentação de rodovia".into();
        unrelated.text = "Obra rodoviária, asfalto e ponte de concreto.".into();
        db.import_capture(&unrelated).unwrap();
        db.save_note(first.document_id, "Aquisição ÚNICA de grampeadores")
            .unwrap();
        assert_eq!(
            db.list_documents("unica grampeadores").unwrap()[0].id,
            first.document_id
        );
        assert_eq!(
            db.similar_documents(first.document_id, 5).unwrap()[0].id,
            second.document_id
        );
        assert!(db
            .save_draft(
                first.document_id,
                second.capture_id,
                "resposta",
                "não deve salvar"
            )
            .is_err());
        assert!(db
            .save_analysis(first.document_id, &analysis(second.capture_id))
            .is_err());
    }
}

#[cfg(test)]
mod writer_guard_tests {
    use super::*;
    #[test]
    fn legacy_connections_can_read_but_cannot_discard_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("guard.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let old = Connection::open(&path).unwrap();
        assert_eq!(
            old.query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(old
            .execute(
                "UPDATE tasks SET payload=zeroblob(1) WHERE document_id=?1",
                [receipt.document_id]
            )
            .is_err());
        store
            .save_note(receipt.document_id, "Nota protegida atual")
            .unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        store.backup(&dir.path().join("backup.sqlite3")).unwrap();
    }
}
