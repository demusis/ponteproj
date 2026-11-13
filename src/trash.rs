//! The trash: reversible deletion of documents, versions, tasks and
//! attachments (payloads and links stay in place), restoration, and the
//! permanent purge, on request or after the retention chosen by the person.
use crate::{model::Capture, protection, storage::Store, tasks::Task};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Document,
    Capture,
    Task,
    Attachment,
}
impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Capture => "capture",
            Self::Task => "task",
            Self::Attachment => "attachment",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "Documento",
            Self::Capture => "Captura",
            Self::Task => "Tarefa",
            Self::Attachment => "Anexo",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "document" => Ok(Self::Document),
            "capture" => Ok(Self::Capture),
            "task" => Ok(Self::Task),
            "attachment" => Ok(Self::Attachment),
            _ => anyhow::bail!("Tipo de item inválido na lixeira."),
        }
    }
}
#[derive(Clone)]
pub struct Target {
    pub kind: Kind,
    pub id: i64,
}
pub struct Plan {
    pub target: Target,
    pub document: Option<i64>,
    pub label: String,
    pub explanation: String,
    pub blocker: Option<String>,
    pub replacement: Option<i64>,
}
#[derive(Clone)]
pub struct Entry {
    pub target: Target,
    pub document: Option<i64>,
    pub label: String,
    pub deleted_at: String,
    pub parent_trashed: bool,
    /// What to look for: the number and title of a document, the title of
    /// a task, the file name of an attachment.
    pub name: String,
    /// What it hangs from, in words ("Documento: ORG-… — título",
    /// "Tarefa sem documento", "Anexo da tarefa …"); may be empty.
    pub belongs: String,
}

/// Where an item stands in the library, for telling the person before a
/// choice that cannot work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemState {
    Active,
    /// In the Lixeira since `deleted_at` (RFC 3339).
    Trashed {
        deleted_at: String,
    },
    /// Not in the library at all (nor in the Lixeira).
    Missing,
}
pub fn contains(conn: &Connection, kind: Kind, id: i64) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM trash WHERE kind=?1 AND item_id=?2)",
        params![kind.key(), id],
        |r| r.get(0),
    )?)
}
pub fn require_active_document(conn: &Connection, id: i64) -> Result<()> {
    ensure!(
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0)
        )?,
        "Documento não encontrado."
    );
    ensure!(
        !contains(conn, Kind::Document, id)?,
        "Documento na lixeira. Restaure-o antes de usar ou importar novamente."
    );
    Ok(())
}
pub fn require_active_capture(conn: &Connection, id: i64) -> Result<i64> {
    let doc: i64 = conn
        .query_row("SELECT document_id FROM captures WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .context("Versão importada não encontrada.")?;
    require_active_document(conn, doc)?;
    ensure!(
        !contains(conn, Kind::Capture, id)?,
        "Versão importada na lixeira. Restaure-a antes de usar."
    );
    Ok(doc)
}
/// The task must exist and be active; returns its document (`None` for a task
/// without document).
pub fn require_active_task(conn: &Connection, id: i64) -> Result<Option<i64>> {
    let doc: Option<i64> = conn
        .query_row("SELECT document_id FROM tasks WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .context("Tarefa não encontrada.")?;
    if let Some(doc) = doc {
        require_active_document(conn, doc)?;
    }
    ensure!(
        !contains(conn, Kind::Task, id)?,
        "Tarefa na lixeira. Restaure-a antes de editar."
    );
    Ok(doc)
}
pub(crate) fn register_refs(
    conn: &Connection,
    kind: &str,
    owner: i64,
    document: i64,
    base: i64,
    materials: &[crate::evidence::Source],
) -> Result<()> {
    let mut ids = BTreeSet::from([base]);
    ids.extend(materials.iter().map(|s| s.capture_id));
    for id in ids {
        conn.execute("INSERT OR IGNORE INTO capture_refs(capture_id,owner_kind,owner_id,owner_document) SELECT ?1,?2,?3,?4 WHERE EXISTS(SELECT 1 FROM captures WHERE id=?1)",params![id,kind,owner,document])?;
    }
    Ok(())
}
struct TaskNode {
    id: i64,
    document_id: Option<i64>,
    dependencies: Vec<i64>,
}
fn all_tasks(conn: &Connection) -> Result<Vec<TaskNode>> {
    let count: i64 = conn.query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))?;
    ensure!(count <= 10000, "Há tarefas demais para esta operação.");
    let mut statement = conn.prepare("SELECT id,document_id,payload FROM tasks")?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (id, doc, bytes) = row?;
        let mut task: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
        task.id = id;
        task.document_id = doc;
        Ok(TaskNode {
            id,
            document_id: doc,
            dependencies: task.dependencies,
        })
    })
    .collect()
}
fn active_tasks(conn: &Connection) -> Result<Vec<TaskNode>> {
    all_tasks(conn)?
        .into_iter()
        .filter_map(|task| {
            match (
                match task.document_id {
                    Some(document) => contains(conn, Kind::Document, document),
                    None => Ok(false),
                },
                contains(conn, Kind::Task, task.id),
            ) {
                (Ok(false), Ok(false)) => Some(Ok(task)),
                (Ok(_), Ok(_)) => None,
                (Err(e), _) | (_, Err(e)) => Some(Err(e)),
            }
        })
        .collect()
}
/// The document an item hangs from (`None` for a task without document and its
/// attachments, and for attachments of atendimentos and examples) and its label.
fn label(conn: &Connection, kind: Kind, id: i64) -> Result<(Option<i64>, String)> {
    match kind {
        Kind::Document => {
            let bytes:Vec<u8>=conn.query_row("SELECT c.payload FROM documents d JOIN captures c ON c.id=d.latest_capture_id WHERE d.id=?1",[id],|r|r.get(0))?;
            let capture: Capture = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            Ok((
                Some(id),
                format!("{} — {}", capture.document_number, capture.title),
            ))
        }
        Kind::Capture => {
            let (doc, bytes): (i64, Vec<u8>) = conn.query_row(
                "SELECT document_id,payload FROM captures WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let capture: Capture = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            Ok((
                Some(doc),
                format!(
                    "Versão importada #{id} — {} — {}",
                    capture.document_number,
                    crate::labels::datetime(&capture.captured_at)
                ),
            ))
        }
        Kind::Task => {
            let (doc, bytes): (Option<i64>, Vec<u8>) = conn.query_row(
                "SELECT document_id,payload FROM tasks WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let task: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            Ok((doc, format!("Tarefa #{id} — {}", task.title)))
        }
        Kind::Attachment => {
            let (owner_kind, owner_id, doc, bytes): (String, i64, Option<i64>, Vec<u8>) = conn
                .query_row(
                    "SELECT owner_kind,owner_id,document_id,metadata FROM attachments WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?
                .context("Anexo não encontrado.")?;
            let meta: crate::attachments::AttachmentMeta =
                serde_json::from_slice(&protection::unseal(&bytes)?)?;
            // No document: an atendimento or example (no trash) or a task
            // without document.
            let owner = match owner_kind.as_str() {
                "task" => format!("da tarefa #{owner_id}"),
                "saiop" => format!("do atendimento #{owner_id}"),
                "example" => format!("do exemplo #{owner_id}"),
                _ => {
                    let doc = doc.unwrap_or(0);
                    label(conn, Kind::Document, doc)
                        .map(|(_, text)| format!("do documento {text}"))
                        .unwrap_or_else(|_| format!("do documento #{doc}"))
                }
            };
            Ok((doc, format!("Anexo “{}” {owner}", meta.name)))
        }
    }
}
/// "ORG-… — title" of a document, whether or not it is in the Lixeira.
fn document_text(conn: &Connection, id: i64) -> Option<String> {
    label(conn, Kind::Document, id).ok().map(|(_, text)| text)
}
/// The title of a task, whether or not it is in the Lixeira.
fn task_title(conn: &Connection, id: i64) -> Option<String> {
    let bytes: Vec<u8> = conn
        .query_row("SELECT payload FROM tasks WHERE id=?1", [id], |r| r.get(0))
        .optional()
        .ok()??;
    let task: Task = serde_json::from_slice(&protection::unseal(&bytes).ok()?).ok()?;
    Some(task.title)
}
/// What the person looks for in the Lixeira: the main text of an item and
/// what it hangs from. Display only; `label` (stored in the history) stays.
fn describe(conn: &Connection, kind: Kind, id: i64) -> Result<(String, String)> {
    let of_document = |doc: Option<i64>| match doc.and_then(|d| document_text(conn, d)) {
        Some(text) => format!("Documento: {text}"),
        None => "Tarefa sem documento".to_owned(),
    };
    Ok(match kind {
        Kind::Document => (
            document_text(conn, id).context("Documento não encontrado.")?,
            String::new(),
        ),
        Kind::Capture => {
            let (doc, bytes): (i64, Vec<u8>) = conn.query_row(
                "SELECT document_id,payload FROM captures WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let capture: Capture = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            let owner = document_text(conn, doc)
                .map(|text| format!("Documento: {text}"))
                .unwrap_or_default();
            (
                format!(
                    "Versão importada de {} em {}",
                    capture.document_number,
                    crate::labels::datetime(&capture.captured_at)
                ),
                owner,
            )
        }
        Kind::Task => {
            let (doc, bytes): (Option<i64>, Vec<u8>) = conn.query_row(
                "SELECT document_id,payload FROM tasks WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let task: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            (task.title, of_document(doc))
        }
        Kind::Attachment => {
            let (owner_kind, owner_id, doc, bytes): (String, i64, Option<i64>, Vec<u8>) = conn
                .query_row(
                    "SELECT owner_kind,owner_id,document_id,metadata FROM attachments WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?
                .context("Anexo não encontrado.")?;
            let meta: crate::attachments::AttachmentMeta =
                serde_json::from_slice(&protection::unseal(&bytes)?)?;
            let owner = match owner_kind.as_str() {
                "task" => match task_title(conn, owner_id) {
                    Some(title) => format!("Anexo da tarefa “{title}”"),
                    None => "Anexo de uma tarefa".to_owned(),
                },
                "saiop" => "Anexo de um atendimento".to_owned(),
                "example" => "Anexo de um exemplo".to_owned(),
                _ => match doc.and_then(|d| document_text(conn, d)) {
                    Some(text) => format!("Anexo do documento {text}"),
                    None => "Anexo de um documento".to_owned(),
                },
            };
            (meta.name, owner)
        }
    })
}
/// Lowercase without accents, for searching.
pub fn fold(value: &str) -> String {
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
/// True when every word of `query` is in the number, title, file name or
/// owner of the entry (case and accents ignored). An empty query matches.
pub fn entry_matches(entry: &Entry, query: &str) -> bool {
    let haystack = fold(&format!("{} {}", entry.name, entry.belongs));
    fold(query)
        .split_whitespace()
        .all(|word| haystack.contains(word))
}
/// The entries to list for `query`, in order, each with whether it is shown
/// under a document that is also in the Lixeira. A document comes first and
/// its items right after it. With a query, a document stays when it or any
/// item under it matches (a matching document shows all its items, otherwise
/// only the matching ones).
pub fn arrange(entries: Vec<Entry>, query: &str) -> Vec<(Entry, bool)> {
    let (children, tops): (Vec<Entry>, Vec<Entry>) =
        entries.into_iter().partition(|e| e.parent_trashed);
    let mut children: Vec<Option<Entry>> = children.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for top in tops {
        let own = entry_matches(&top, query);
        let mut mine = Vec::new();
        if top.target.kind == Kind::Document {
            for slot in children.iter_mut() {
                if slot
                    .as_ref()
                    .is_some_and(|c| c.document == Some(top.target.id))
                {
                    if let Some(child) = slot.take() {
                        if own || entry_matches(&child, query) {
                            mine.push(child);
                        }
                    }
                }
            }
        }
        if own || !mine.is_empty() {
            out.push((top, false));
            out.extend(mine.into_iter().map(|c| (c, true)));
        }
    }
    // Items whose document is not in the list: shown alone.
    for child in children.into_iter().flatten() {
        if entry_matches(&child, query) {
            out.push((child, false));
        }
    }
    out
}
fn deletion_plan(conn: &Connection, target: Target) -> Result<Plan> {
    let (doc, label) = label(conn, target.kind, target.id)?;
    if doc.is_none() && target.kind == Kind::Attachment {
        let owner: String = conn.query_row(
            "SELECT owner_kind FROM attachments WHERE id=?1",
            [target.id],
            |r| r.get(0),
        )?;
        ensure!(
            owner == "task",
            "Anexos de atendimentos e de exemplos não vão para a Lixeira: use “Excluir de vez” onde o anexo aparece."
        );
    }
    if let Some(doc) = doc {
        require_active_document(conn, doc)?;
    }
    let doc_id = doc.unwrap_or(0);
    ensure!(
        !contains(conn, target.kind, target.id)?,
        "O item já está na lixeira."
    );
    let mut plan = Plan {
        target: target.clone(),
        document: doc,
        label,
        explanation: String::new(),
        blocker: None,
        replacement: None,
    };
    match target.kind {
        Kind::Capture => {
            let refs: i64 = conn.query_row(
                "SELECT count(*) FROM capture_refs WHERE capture_id=?1",
                [target.id],
                |r| r.get(0),
            )?;
            let remaining:Option<i64>=conn.query_row("SELECT c.id FROM captures c WHERE c.document_id=?1 AND c.id<>?2 AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='capture' AND t.item_id=c.id) ORDER BY c.id DESC LIMIT 1",params![doc_id,target.id],|r|r.get(0)).optional()?;
            let current: i64 = conn.query_row(
                "SELECT latest_capture_id FROM documents WHERE id=?1",
                [doc_id],
                |r| r.get(0),
            )?;
            if refs > 0 {
                plan.blocker=Some(format!("Esta versão importada é usada em {}. Ela é mantida para preservar esses registros.",if refs==1{"1 registro salvo (minuta, análise ou fonte)".to_owned()}else{format!("{refs} registros salvos (minutas, análises ou fontes)")}));
            } else if remaining.is_none() {
                plan.blocker=Some("Esta é a única versão importada ativa. Para retirar todo o conteúdo do acervo, exclua o documento.".into());
            }
            if current == target.id {
                plan.replacement = remaining;
                plan.explanation=format!("Esta é a versão em uso. Após a exclusão, a ficha usará a importação #{}. Os PDFs e os dados da versão excluída ficam guardados na lixeira.",remaining.unwrap_or(current));
            } else {
                plan.explanation="A versão sairá do histórico ativo; os PDFs e o conteúdo ficam guardados na lixeira para restauração.".into();
            }
        }
        Kind::Task | Kind::Document => {
            let tasks = active_tasks(conn)?;
            let removing: BTreeSet<i64> = tasks
                .iter()
                .filter(|t| {
                    if target.kind == Kind::Task {
                        t.id == target.id
                    } else {
                        t.document_id == doc
                    }
                })
                .map(|t| t.id)
                .collect();
            let dependents = tasks
                .iter()
                .filter(|t| {
                    !removing.contains(&t.id)
                        && t.dependencies.iter().any(|id| removing.contains(id))
                })
                .count();
            if dependents > 0 {
                plan.blocker = Some(if dependents == 1 {
                    "Há 1 tarefa ativa que depende deste item. Revise as dependências antes de excluir.".to_owned()
                } else {
                    format!("Há {dependents} tarefas ativas que dependem deste item. Revise as dependências antes de excluir.")
                });
            }
            plan.explanation=if target.kind==Kind::Document{"O documento sairá do acervo e suas tarefas e anexos ficarão ocultos. Versões importadas, PDFs, anexos, minutas, notas, textos lidos dos PDFs e vínculos ficam guardados para restauração. Itens já excluídos separadamente continuam na lixeira."}else{"A tarefa sairá da lista. Seus campos, checklist, resultado e anexos ficam guardados na lixeira."}.into();
        }
        Kind::Attachment => {
            plan.explanation = "O anexo sairá da lista. O arquivo fica guardado na lixeira e pode ser restaurado a qualquer momento.".into();
        }
    }
    Ok(plan)
}
impl Store {
    pub(crate) fn ensure_trash(&mut self) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS trash(kind TEXT NOT NULL CHECK(kind IN ('document','capture','task','attachment')),item_id INTEGER NOT NULL,document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,deleted_at TEXT NOT NULL,PRIMARY KEY(kind,item_id)); CREATE INDEX IF NOT EXISTS trash_document ON trash(document_id); CREATE TABLE IF NOT EXISTS capture_refs(capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE,owner_kind TEXT NOT NULL CHECK(owner_kind IN ('draft','analysis')),owner_id INTEGER NOT NULL,owner_document INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,PRIMARY KEY(capture_id,owner_kind,owner_id)); CREATE TABLE IF NOT EXISTS trash_settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);")?;
        // Libraries created before 0.11.0 have a trash table whose CHECK does
        // not know 'attachment'. SQLite cannot alter a CHECK, so the table is
        // rebuilt once, keeping every row.
        let definition: String = tx.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='trash'",
            [],
            |r| r.get(0),
        )?;
        // Schema 14: a task without document (and its attachments) is trashed
        // with no document, so `document_id` may be NULL.
        if !definition.contains("'attachment'")
            || definition.contains("document_id INTEGER NOT NULL")
        {
            // Triggers of other tables mention `trash`; SQLite refuses to
            // rename a table while any trigger points at a missing one, so
            // they go first. `ensure_write_guards` recreates all of them.
            let triggers = tx
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'ponte_%'",
                )?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for name in triggers {
                tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {name};"))?;
            }
            tx.execute_batch(
                "CREATE TABLE trash_v10(kind TEXT NOT NULL CHECK(kind IN ('document','capture','task','attachment')),item_id INTEGER NOT NULL,document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,deleted_at TEXT NOT NULL,PRIMARY KEY(kind,item_id));
                 INSERT INTO trash_v10(kind,item_id,document_id,deleted_at) SELECT kind,item_id,document_id,deleted_at FROM trash;
                 DROP TABLE trash;
                 ALTER TABLE trash_v10 RENAME TO trash;
                 CREATE INDEX IF NOT EXISTS trash_document ON trash(document_id);",
            )?;
        }
        if previous < 8 {
            for (table, kind) in [("drafts", "draft"), ("analyses", "analysis")] {
                let mut statement = tx.prepare(&format!(
                    "SELECT id,document_id,capture_id,payload FROM {table}"
                ))?;
                let rows = statement
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, Vec<u8>>(3)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(statement);
                for (id, doc, base, bytes) in rows {
                    let value: serde_json::Value =
                        serde_json::from_slice(&protection::unseal(&bytes)?)?;
                    let materials: Vec<crate::evidence::Source> = value
                        .get("materials")
                        .cloned()
                        .map(serde_json::from_value)
                        .transpose()?
                        .unwrap_or_default();
                    register_refs(&tx, kind, id, doc, base, &materials)?;
                }
            }
        }
        // Never lowers the number a later step has already written.
        if previous < 8 {
            tx.pragma_update(None, "user_version", 8)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn trash_plan(&self, target: Target) -> Result<Plan> {
        deletion_plan(&self.conn, target)
    }
    /// Days an item stays in the trash before being removed for good;
    /// 0 means never (the default).
    pub fn trash_retention_days(&self) -> Result<u32> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM trash_settings WHERE key='retention_days'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value.and_then(|v| v.trim().parse().ok()).unwrap_or(0))
    }
    /// Sets the retention of this computer. Not synchronized: in a team, only
    /// the main computer purges, with its own value.
    pub fn set_trash_retention_days(&mut self, days: u32) -> Result<()> {
        ensure!(
            days <= 3660,
            "Prazo da lixeira grande demais (máximo de 10 anos)."
        );
        self.conn.execute(
            "INSERT INTO trash_settings(key,value) VALUES('retention_days',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [days.to_string()],
        )?;
        Ok(())
    }
    /// Items whose stay in the trash exceeded `days`, documents first so
    /// that their children go with them in one step.
    pub fn expired_trash(&self, days: u32) -> Result<Vec<Target>> {
        ensure!(days > 0, "Sem prazo, nada vence.");
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(i64::from(days))).to_rfc3339();
        let mut statement = self.conn.prepare(
            "SELECT kind,item_id FROM trash WHERE deleted_at < ?1
             ORDER BY CASE kind WHEN 'document' THEN 0 ELSE 1 END, deleted_at, item_id",
        )?;
        let rows = statement.query_map([cutoff], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        rows.map(|row| {
            let (kind, id) = row?;
            Ok(Target {
                kind: Kind::parse(&kind)?,
                id,
            })
        })
        .collect()
    }
    /// Removes for good every item that outlived the retention. A client
    /// computer leaves this to the main computer. Returns how many went.
    pub fn purge_expired_trash(&mut self, days: u32) -> Result<usize> {
        if self
            .load_intranet_config()
            .is_ok_and(|cfg| cfg.mode == crate::intranet::IntranetMode::Client)
        {
            return Ok(0);
        }
        let mut purged = 0;
        for target in self.expired_trash(days)? {
            // A child may already be gone with its document.
            if !contains(&self.conn, target.kind, target.id)? {
                continue;
            }
            self.purge_trash_item(target)?;
            purged += 1;
        }
        Ok(purged)
    }
    /// Empties the trash: every item goes for good, documents first so that
    /// their children go with them. An item that cannot be removed stays and
    /// its reason is returned; the others still go.
    pub fn purge_all_trash(&mut self) -> Result<(usize, Vec<String>)> {
        let targets: Vec<Target> = {
            let mut statement = self.conn.prepare(
                "SELECT kind,item_id FROM trash
                 ORDER BY CASE kind WHEN 'document' THEN 0 WHEN 'task' THEN 1 ELSE 2 END, deleted_at, item_id",
            )?;
            let rows =
                statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            rows.map(|row| {
                let (kind, id) = row?;
                Ok(Target {
                    kind: Kind::parse(&kind)?,
                    id,
                })
            })
            .collect::<Result<_>>()?
        };
        let mut purged = 0;
        let mut failures = Vec::new();
        for target in targets {
            // A child may already be gone with its document or its task.
            if !contains(&self.conn, target.kind, target.id)? {
                continue;
            }
            match self.purge_trash_item(target) {
                Ok(_) => purged += 1,
                Err(error) => failures.push(error.to_string()),
            }
        }
        Ok((purged, failures))
    }
    /// Deletes one trashed item for good: a document with everything it
    /// holds, a capture with its files, a task with its attachments, or one
    /// attachment. On a client computer the request goes to the main one.
    pub fn purge_trash_item(&mut self, target: Target) -> Result<Option<i64>> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::PurgeTrash {
                    kind: target.kind.key().into(),
                    id: target.id,
                },
            )? {
                crate::intranet::MutationReply::Id(doc) => return Ok((doc != 0).then_some(doc)),
                _ => anyhow::bail!("Resposta inesperada do computador principal ao apagar o item."),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let document: Option<i64> = tx
            .query_row(
                "SELECT document_id FROM trash WHERE kind=?1 AND item_id=?2",
                params![target.kind.key(), target.id],
                |r| r.get(0),
            )
            .optional()?
            .context("O item não está na lixeira.")?;
        let (_, text) = label(&tx, target.kind, target.id)?;
        match target.kind {
            Kind::Document => {
                tx.execute(
                    "DELETE FROM draft_conferences WHERE document_identity IN (SELECT hex(identity_hash) FROM documents WHERE id=?1)",
                    [target.id],
                )?;
                tx.execute("DELETE FROM documents WHERE id=?1", [target.id])?;
            }
            Kind::Capture => {
                let current: Option<i64> = tx
                    .query_row(
                        "SELECT latest_capture_id FROM documents WHERE id=?1",
                        [document],
                        |r| r.get(0),
                    )
                    .optional()?;
                ensure!(
                    current != Some(target.id),
                    "Esta versão é a que está em uso; ela não pode ser apagada."
                );
                tx.execute("DELETE FROM captures WHERE id=?1", [target.id])?;
            }
            Kind::Task => {
                tx.execute(
                    "DELETE FROM trash WHERE kind='attachment' AND item_id IN (SELECT id FROM attachments WHERE owner_kind='task' AND owner_id=?1)",
                    [target.id],
                )?;
                tx.execute(
                    "DELETE FROM attachments WHERE owner_kind='task' AND owner_id=?1",
                    [target.id],
                )?;
                tx.execute("DELETE FROM tasks WHERE id=?1", [target.id])?;
            }
            Kind::Attachment => {
                tx.execute("DELETE FROM attachments WHERE id=?1", [target.id])?;
            }
        }
        tx.execute(
            "DELETE FROM trash WHERE kind=?1 AND item_id=?2",
            params![target.kind.key(), target.id],
        )?;
        tx.execute(
            "DELETE FROM item_versions WHERE kind=?1 AND item_id=?2",
            params![target.kind.key(), target.id],
        )?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            target.kind.key(),
            target.id,
            None,
            "purge_trash",
            if target.kind != Kind::Document {
                document
            } else {
                None
            },
            &format!("Apagou de vez da lixeira: {text}"),
        )?;
        tx.commit()?;
        Ok(document)
    }
    /// Moves an item to the trash (reversible), unless `deletion_plan` finds a
    /// blocker. Trashing the version in use promotes another one. Returns the
    /// document the item belongs to; on a client computer it is sent to the
    /// main one.
    pub fn trash_item(&mut self, target: Target) -> Result<Option<i64>> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::TrashItem {
                    kind: target.kind.key().into(),
                    id: target.id,
                },
            )? {
                crate::intranet::MutationReply::Id(doc) => return Ok((doc != 0).then_some(doc)),
                _ => {
                    anyhow::bail!("Resposta inesperada do computador principal ao excluir o item.")
                }
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let plan = deletion_plan(&tx, target.clone())?;
        ensure!(
            plan.blocker.is_none(),
            "{}",
            plan.blocker.as_deref().unwrap_or("Exclusão bloqueada.")
        );
        tx.execute(
            "INSERT INTO trash(kind,item_id,document_id,deleted_at) VALUES(?1,?2,?3,?4)",
            params![
                target.kind.key(),
                target.id,
                plan.document,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        if let Some(replacement) = plan.replacement {
            tx.execute(
                "UPDATE documents SET latest_capture_id=?1 WHERE id=?2",
                params![replacement, plan.document],
            )?;
        }
        crate::intranet::record_mutation(
            &tx,
            &actor,
            target.kind.key(),
            target.id,
            None,
            "trash_item",
            plan.document,
            &format!("Moveu para a lixeira: {}", plan.label),
        )?;
        tx.commit()?;
        Ok(plan.document)
    }
    /// Whether a document is usable, in the Lixeira (and since when) or not
    /// in the library. Read only.
    /// A failure to read is an error, never "Missing".
    pub fn document_state(&self, id: i64) -> Result<ItemState> {
        self.item_state("documents", Kind::Document, id)
    }
    /// The same for a task by itself (its document is not looked at).
    pub fn task_state(&self, id: i64) -> Result<ItemState> {
        self.item_state("tasks", Kind::Task, id)
    }
    fn item_state(&self, table: &str, kind: Kind, id: i64) -> Result<ItemState> {
        let exists = self.conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
            [id],
            |r| r.get::<_, bool>(0),
        )?;
        let deleted_at: Option<String> = self
            .conn
            .query_row(
                "SELECT deleted_at FROM trash WHERE kind=?1 AND item_id=?2",
                params![kind.key(), id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match (exists, deleted_at) {
            (_, Some(deleted_at)) => ItemState::Trashed { deleted_at },
            (true, None) => ItemState::Active,
            (false, None) => ItemState::Missing,
        })
    }
    /// "ORG-… — title" of a document, even one in the Lixeira.
    pub fn document_text(&self, id: i64) -> Option<String> {
        document_text(&self.conn, id)
    }
    /// The title of a task, even one in the Lixeira.
    pub fn task_title(&self, id: i64) -> Option<String> {
        task_title(&self.conn, id)
    }
    /// The document a task hangs from (`None`: no document, or no task).
    pub fn task_document(&self, id: i64) -> Option<i64> {
        self.conn
            .query_row("SELECT document_id FROM tasks WHERE id=?1", [id], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()
            .ok()
            .flatten()
            .flatten()
    }
    /// What is in the Lixeira, memoized until the library changes (reading
    /// each item decrypts it; the list is drawn on every frame).
    pub fn trash_entries(&self) -> Result<Vec<Entry>> {
        self.cached("trash_entries", || self.trash_entries_uncached())
    }
    fn trash_entries_uncached(&self) -> Result<Vec<Entry>> {
        let mut statement=self.conn.prepare("SELECT kind,item_id,document_id,deleted_at FROM trash ORDER BY deleted_at DESC,kind,item_id")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (kind, id, document, deleted_at) = row?;
            let kind = Kind::parse(&kind)?;
            let (_, label) = label(&self.conn, kind, id)?;
            let (name, belongs) =
                describe(&self.conn, kind, id).unwrap_or_else(|_| (label.clone(), String::new()));
            let parent_trashed = match document {
                Some(document) if kind != Kind::Document => {
                    contains(&self.conn, Kind::Document, document)?
                }
                _ => false,
            };
            Ok(Entry {
                target: Target { kind, id },
                document,
                label,
                deleted_at,
                parent_trashed,
                name,
                belongs,
            })
        })
        .collect()
    }
    pub fn restore_item(&mut self, target: Target) -> Result<Option<i64>> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::RestoreItem {
                    kind: target.kind.key().into(),
                    id: target.id,
                },
            )? {
                crate::intranet::MutationReply::Id(doc) => return Ok((doc != 0).then_some(doc)),
                _ => {
                    anyhow::bail!("Resposta inesperada do computador principal ao restaurar item.")
                }
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let document: Option<i64> = tx
            .query_row(
                "SELECT document_id FROM trash WHERE kind=?1 AND item_id=?2",
                params![target.kind.key(), target.id],
                |r| r.get(0),
            )
            .optional()?
            .context("O item não está na lixeira.")?;
        if let (true, Some(document)) = (target.kind != Kind::Document, document) {
            require_active_document(&tx, document)?;
        }
        if !matches!(target.kind, Kind::Capture | Kind::Attachment) {
            let all = all_tasks(&tx)?;
            let mut restoring = BTreeSet::new();
            for task in &all {
                if (target.kind == Kind::Task && task.id == target.id)
                    || (target.kind == Kind::Document
                        && task.document_id == document
                        && !contains(&tx, Kind::Task, task.id)?)
                {
                    restoring.insert(task.id);
                }
            }
            let active = active_tasks(&tx)?
                .into_iter()
                .map(|t| t.id)
                .collect::<BTreeSet<_>>();
            for task in all.iter().filter(|t| restoring.contains(&t.id)) {
                ensure!(
                    task.dependencies
                        .iter()
                        .all(|id| active.contains(id) || restoring.contains(id)),
                    "Restaure primeiro as tarefas ou documentos usados como dependências."
                );
            }
        }
        tx.execute(
            "DELETE FROM trash WHERE kind=?1 AND item_id=?2",
            params![target.kind.key(), target.id],
        )?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            target.kind.key(),
            target.id,
            None,
            "restore_item",
            document,
            &format!(
                "Restaurou da lixeira: {} #{}",
                target.kind.label(),
                target.id
            ),
        )?;
        tx.commit()?;
        Ok(document)
    }
}
#[cfg(feature = "desktop")]
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    pub pending: Option<Target>,
    message: String,
    /// Items were removed for good; the app refreshes its lists.
    purged: bool,
    /// What the person typed in the search box of the list.
    query: String,
    /// A permanent deletion waiting for the person's confirmation.
    ask: Option<Ask>,
}

/// What "apagar de vez" was asked for.
#[cfg(feature = "desktop")]
#[derive(Clone)]
enum Ask {
    One { target: Target, name: String },
    Expired { days: u32, count: usize },
    All { count: usize },
}

/// Choices for "apagar de vez após": days, 0 = never.
pub const RETENTION_CHOICES: [u32; 6] = [0, 7, 30, 90, 180, 365];

pub fn retention_label(days: u32) -> String {
    match days {
        0 => "Nunca".to_owned(),
        7 => "7 dias".to_owned(),
        30 => "30 dias".to_owned(),
        90 => "90 dias".to_owned(),
        180 => "180 dias".to_owned(),
        365 => "1 ano".to_owned(),
        other => format!("{other} dias"),
    }
}
pub struct Change {
    pub target: Target,
    /// The document of the item; `None` for a task without document.
    pub document: Option<i64>,
    pub restored: bool,
}
#[cfg(feature = "desktop")]
impl Panel {
    pub fn request(&mut self, target: Target) {
        self.pending = Some(target);
        self.message.clear();
    }
    /// Development only: the Lixeira as the tour pictures it. "aviso" shows
    /// the notice of the retention setting, "busca" a search, anything else
    /// the plain list.
    #[cfg(feature = "shots")]
    pub fn shots_open(&mut self, view: &str) {
        self.pending = None;
        self.message.clear();
        self.query.clear();
        self.ask = None;
        match view {
            "aviso" => {
                self.message = "Itens excluídos há mais de 7 dias serão apagados de vez ao abrir o Ponte e uma vez por dia.".into();
            }
            "busca" => self.query = "escala".into(),
            "apagar" => self.ask = Some(Ask::All { count: 4 }),
            _ => {}
        }
    }
    /// A question of the Lixeira is open (move to the trash or delete for good).
    pub fn asking(&self) -> bool {
        self.pending.is_some() || self.ask.is_some()
    }
    pub fn take_purged(&mut self) -> bool {
        std::mem::take(&mut self.purged)
    }

    /// The retention setting and the manual purge, at the top of the list.
    fn retention_row(&mut self, ui: &mut eframe::egui::Ui, store: &mut Store, busy: bool) {
        use crate::{theme, widgets};
        use eframe::egui;
        let on_client = store
            .load_intranet_config()
            .is_ok_and(|cfg| cfg.mode == crate::intranet::IntranetMode::Client);
        let mut days = store.trash_retention_days().unwrap_or(0);
        widgets::card(ui, |ui| {
            widgets::field_label(ui, "Apagar de vez os itens excluídos há mais de");
            ui.horizontal_wrapped(|ui| {
                let before = days;
                ui.add_enabled_ui(!on_client, |ui| {
                    egui::ComboBox::from_id_salt("trash-retention")
                        .selected_text(retention_label(days))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for choice in RETENTION_CHOICES {
                                ui.selectable_value(&mut days, choice, retention_label(choice));
                            }
                        });
                });
                if days != before {
                    match store.set_trash_retention_days(days) {
                        Ok(()) => {
                            self.message = if days == 0 {
                                "A lixeira não será esvaziada sozinha.".into()
                            } else {
                                format!(
                                    "Itens excluídos há mais de {} serão apagados de vez ao abrir o Ponte e uma vez por dia.",
                                    retention_label(days)
                                )
                            }
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                }
                if on_client {
                    widgets::small_muted(ui, "Definido no computador principal.");
                } else if days > 0 {
                    // The Ponte does this by itself when it opens and once a day;
                    // the button only spares the wait, so it shows when there is
                    // something past the time.
                    let expired = store.expired_trash(days).map(|v| v.len()).unwrap_or(0);
                    if expired > 0 {
                        let text = if expired == 1 {
                            "Apagar já o item que passou do prazo…".to_owned()
                        } else {
                            format!("Apagar já os {expired} itens que passaram do prazo…")
                        };
                        if ui
                            .add_enabled(!busy, widgets::secondary(text))
                            .on_hover_text("O Ponte faz essa limpeza sozinho ao abrir e uma vez por dia. Use este botão para não esperar.")
                            .clicked()
                        {
                            self.ask = Some(Ask::Expired {
                                days,
                                count: expired,
                            });
                        }
                    } else {
                        widgets::small_muted(ui, "Nenhum item passou desse prazo até agora.");
                    }
                }
            });
            ui.add_space(theme::S1);
            widgets::small_muted(
                ui,
                if days == 0 {
                    "Itens excluídos podem ser restaurados a qualquer momento; a lixeira não é esvaziada sozinha e os arquivos continuam ocupando espaço no acervo."
                } else {
                    "Um documento apagado de vez leva junto versões, PDFs, notas, minutas, tarefas e anexos. Cópias de segurança anteriores não são afetadas."
                },
            );
        });
    }

    /// A note in a soft-coloured frame with a close button. True when closed.
    fn message_line(&mut self, ui: &mut eframe::egui::Ui) -> bool {
        use crate::{theme, widgets};
        use eframe::egui;
        let (fg, bg) = widgets::Tone::Info.colors();
        let mut close = false;
        egui::Frame::new()
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // `horizontal` fixes the row to the height of its content;
                // a bare `with_layout` here would take all the height left.
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(theme::icon::INFO).size(18.0).color(fg));
                    let text_width = (ui.available_width() - 36.0).max(80.0);
                    ui.scope(|ui| {
                        ui.set_max_width(text_width);
                        ui.add(
                            egui::Label::new(egui::RichText::new(&self.message).color(fg)).wrap(),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, theme::icon::CLOSE, "Fechar o aviso").clicked()
                        {
                            close = true;
                        }
                    });
                });
            });
        close
    }

    /// The content of the trash window, drawn in place (the Ajustes area
    /// embeds it): retention, the notice and the list of deleted items.
    pub fn ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        store: &mut Store,
        busy: bool,
    ) -> Option<Change> {
        use crate::{theme, widgets};
        use eframe::egui;
        let confirming = self.asking();
        let mut change = None;
        self.retention_row(ui, store, busy || confirming);
        if !self.message.is_empty() {
            ui.add_space(theme::S2);
            if self.message_line(ui) {
                self.message.clear();
            }
        }
        ui.add_space(theme::S3);
        let retention = store.trash_retention_days().unwrap_or(0);
        let local = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .map(|date| date.with_timezone(&chrono::Local))
                .ok()
        };
        let when = |text: &str| match local(text) {
            Some(date) => date.format("%d/%m/%Y %H:%M").to_string(),
            None => text.to_owned(),
        };
        let entries = match store.trash_entries() {
            Ok(entries) => entries,
            Err(error) => {
                widgets::card(ui, |ui| {
                    widgets::muted(ui, format!("Não foi possível ler a lixeira: {error}"));
                });
                return change;
            }
        };
        let total = entries.len();
        if total > 0 {
            ui.horizontal(|ui| {
                widgets::search(
                    ui,
                    &mut self.query,
                    "Buscar por número, título ou nome do arquivo",
                    380.0,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !busy && !confirming,
                            widgets::danger(format!("{} Esvaziar a lixeira…", theme::icon::TRASH)),
                        )
                        .on_hover_text(
                            "Apaga de vez tudo o que está na lixeira. O Ponte pergunta antes.",
                        )
                        .clicked()
                    {
                        self.ask = Some(Ask::All { count: total });
                    }
                });
            });
        }
        let shown = arrange(entries, &self.query);
        if total > 0 {
            ui.add_space(theme::S1);
            widgets::small_muted(
                ui,
                if self.query.trim().is_empty() {
                    if total == 1 {
                        "1 item na lixeira.".to_owned()
                    } else {
                        format!("{total} itens na lixeira.")
                    }
                } else if shown.len() == 1 {
                    format!("Mostrando 1 de {total} itens.")
                } else {
                    format!("Mostrando {} de {total} itens.", shown.len())
                },
            );
            ui.add_space(theme::S2);
        }
        widgets::card(ui, |ui| {
            if total > 0 && shown.is_empty() {
                ui.add_space(theme::S3);
                ui.vertical_centered(|ui| {
                    widgets::muted(
                        ui,
                        format!("Nada na lixeira corresponde a “{}”.", self.query.trim()),
                    );
                });
                ui.add_space(theme::S3);
                return;
            }
            if total == 0 {
                ui.add_space(theme::S4);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(theme::icon::TRASH)
                            .size(32.0)
                            .color(theme::c().muted),
                    );
                    widgets::muted(ui, "A lixeira está vazia.");
                });
                ui.add_space(theme::S4);
                return;
            }
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, (entry, nested)) in shown.into_iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        let expires = match (retention, local(&entry.deleted_at)) {
                            (0, _) => "Não é apagado sozinho.".to_owned(),
                            (days, Some(date)) => format!(
                                "Será apagado de vez em {}.",
                                (date + chrono::Duration::days(i64::from(days)))
                                    .format("%d/%m/%Y")
                            ),
                            (_, None) => String::new(),
                        };
                        ui.horizontal(|ui| {
                            if nested {
                                ui.add_space(theme::S4);
                            }
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - 290.0).max(200.0));
                                ui.horizontal_wrapped(|ui| {
                                    widgets::pill(
                                        ui,
                                        kind_name(entry.target.kind),
                                        widgets::Tone::Neutral,
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&entry.name)
                                                .font(theme::font_strong(theme::SIZE_BODY)),
                                        )
                                        .wrap(),
                                    );
                                });
                                if !entry.belongs.is_empty() {
                                    widgets::small_muted(ui, entry.belongs.clone());
                                }
                                widgets::small_muted(
                                    ui,
                                    format!("Excluído em {}. {expires}", when(&entry.deleted_at)),
                                );
                                if entry.parent_trashed {
                                    widgets::small_muted(ui, "Restaure o documento primeiro para recuperar este item individualmente.");
                                }
                                if entry.target.kind == Kind::Capture {
                                    widgets::small_muted(ui, "Restaurar devolve a versão ao histórico; não troca a versão em uso.");
                                }
                                if entry.target.kind == Kind::Attachment {
                                    widgets::small_muted(ui, "Restaurar devolve o arquivo à lista de anexos do documento ou da tarefa.");
                                }
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(
                                            !busy && !entry.parent_trashed && !confirming,
                                            widgets::secondary(format!(
                                                "{} Restaurar",
                                                theme::icon::UNDO
                                            )),
                                        )
                                        .clicked()
                                    {
                                        match store.restore_item(entry.target.clone()) {
                                            Ok(document) => {
                                                self.message = "Item restaurado. O acervo foi atualizado.".into();
                                                change = Some(Change {
                                                    target: entry.target.clone(),
                                                    document,
                                                    restored: true,
                                                });
                                            }
                                            Err(error) => self.message = error.to_string(),
                                        }
                                    }
                                    if ui
                                        .add_enabled(
                                            !busy && !confirming,
                                            widgets::danger("Apagar de vez…"),
                                        )
                                        .on_hover_text("Apaga este item para sempre. O Ponte pergunta antes.")
                                        .clicked()
                                    {
                                        self.ask = Some(Ask::One {
                                            target: entry.target.clone(),
                                            name: entry.name.clone(),
                                        });
                                    }
                                },
                            );
                        });
                    }
                });
        });
        change
    }

    pub fn show(
        &mut self,
        ctx: &eframe::egui::Context,
        store: &mut Store,
        busy: bool,
    ) -> Option<Change> {
        use crate::{theme, widgets};
        use eframe::egui;
        let mut change = None;
        if let Some(target) = self.pending.clone() {
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("Excluir para a lixeira?")
                .collapsible(false)
                .resizable(false)
                .default_width(590.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    match store.trash_plan(target.clone()) {
                        Ok(plan) => {
                            widgets::section(ui, &plan.label);
                            ui.add_space(theme::S1);
                            ui.label(&plan.explanation);
                            widgets::small_muted(ui, "A exclusão é local e reversível. Nada será excluído ou tramitado no SIGADOC; arquivos exportados e cópias de segurança permanecem.");
                            if let Some(blocker) = &plan.blocker {
                                ui.add_space(theme::S2);
                                let (fg, bg) = widgets::Tone::Danger.colors();
                                egui::Frame::new()
                                    .fill(bg)
                                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(theme::icon::ERROR)
                                                    .size(18.0)
                                                    .color(fg),
                                            );
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(blocker).color(fg),
                                                )
                                                .wrap(),
                                            );
                                        });
                                    });
                            }
                            ui.add_space(theme::S3);
                            ui.horizontal(|ui| {
                                cancel = ui.add(widgets::secondary("Cancelar")).clicked();
                                let p = theme::c();
                                let delete = egui::Button::new(
                                    egui::RichText::new(format!(
                                        "{} Mover para a lixeira",
                                        theme::icon::TRASH
                                    ))
                                    .font(theme::font_strong(theme::SIZE_BODY))
                                    .color(p.on_primary),
                                )
                                .fill(p.danger)
                                .stroke(egui::Stroke::NONE);
                                confirm = ui
                                    .add_enabled(!busy && plan.blocker.is_none(), delete)
                                    .clicked();
                            });
                        }
                        Err(error) => {
                            ui.label(error.to_string());
                            ui.add_space(theme::S2);
                            cancel = ui.add(widgets::secondary("Fechar")).clicked();
                        }
                    }
                    if !self.message.is_empty() {
                        ui.add_space(theme::S2);
                        widgets::muted(ui, &self.message);
                    }
                });
            if cancel {
                self.pending = None;
            }
            if confirm {
                match store.trash_item(target.clone()) {
                    Ok(document) => {
                        self.pending = None;
                        self.message =
                            "Item movido para a lixeira. Use Restaurar para recuperá-lo.".into();
                        change = Some(Change {
                            target,
                            document,
                            restored: false,
                        });
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
        }
        if let Some(ask) = self.ask.clone() {
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("Apagar de vez?")
                .collapsible(false)
                .resizable(false)
                .default_width(590.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let (title, body) = purge_question(&ask);
                    widgets::section(ui, &title);
                    ui.add_space(theme::S1);
                    ui.label(body);
                    ui.add_space(theme::S1);
                    widgets::small_muted(ui, "Não há como desfazer nem restaurar depois. Com a equipe ou o celular, o que for apagado some também dos outros aparelhos na sincronização seguinte. Cópias de segurança já feitas não são afetadas e nada muda no SIGADOC.");
                    ui.add_space(theme::S3);
                    ui.horizontal(|ui| {
                        cancel = ui.add(widgets::secondary("Cancelar")).clicked();
                        let p = theme::c();
                        let delete = egui::Button::new(
                            egui::RichText::new(format!("{} Apagar de vez", theme::icon::TRASH))
                                .font(theme::font_strong(theme::SIZE_BODY))
                                .color(p.on_primary),
                        )
                        .fill(p.danger)
                        .stroke(egui::Stroke::NONE);
                        confirm = ui.add_enabled(!busy, delete).clicked();
                    });
                });
            if cancel {
                self.ask = None;
            }
            if confirm {
                self.ask = None;
                let done = match ask {
                    Ask::One { target, .. } => {
                        store.purge_trash_item(target).map(|_| (1, Vec::new()))
                    }
                    Ask::Expired { days, .. } => store
                        .purge_expired_trash(days)
                        .map(|count| (count, Vec::new())),
                    Ask::All { .. } => store.purge_all_trash(),
                };
                match done {
                    Ok((count, failures)) => {
                        self.purged = count > 0;
                        self.message = purge_result(count, &failures);
                    }
                    Err(error) => {
                        // Part of the work may have been done before the failure.
                        self.purged = true;
                        self.message = format!("Não foi possível apagar: {error}");
                    }
                }
            }
        }
        if self.open {
            let mut open = self.open;
            egui::Window::new("Lixeira local")
                .open(&mut open)
                .default_size([830.0, 560.0])
                .show(ctx, |ui| {
                    if let Some(done) = self.ui(ui, store, busy) {
                        change = Some(done);
                    }
                });
            self.open = open;
        }
        change
    }
}

/// The title and the explanation of the question before deleting for good.
#[cfg(feature = "desktop")]
fn purge_question(ask: &Ask) -> (String, String) {
    match ask {
        Ask::One { target, name } => (
            name.clone(),
            match target.kind {
                Kind::Document => "Este documento será apagado de vez, com as versões importadas, PDFs, notas, minutas, tarefas e anexos dele.",
                Kind::Task => "Esta tarefa será apagada de vez, com os anexos dela.",
                Kind::Capture => "Esta versão importada será apagada de vez, com os arquivos dela.",
                Kind::Attachment => "Este anexo será apagado de vez.",
            }
            .to_owned(),
        ),
        Ask::Expired { days, count } => (
            if *count == 1 {
                "1 item que passou do prazo".to_owned()
            } else {
                format!("{count} itens que passaram do prazo")
            },
            format!(
                "Será apagado de vez o que foi excluído há mais de {}. Um documento leva junto versões, PDFs, notas, minutas, tarefas e anexos.",
                retention_label(*days)
            ),
        ),
        Ask::All { count } => (
            "Esvaziar a lixeira".to_owned(),
            if *count == 1 {
                "O único item da lixeira será apagado de vez. Um documento leva junto versões, PDFs, notas, minutas, tarefas e anexos.".to_owned()
            } else {
                format!("Os {count} itens da lixeira serão apagados de vez. Um documento leva junto versões, PDFs, notas, minutas, tarefas e anexos.")
            },
        ),
    }
}

/// What to tell the person after deleting for good.
#[cfg(feature = "desktop")]
fn purge_result(count: usize, failures: &[String]) -> String {
    let done = match count {
        0 => "Nada foi apagado.".to_owned(),
        1 => "1 item apagado de vez.".to_owned(),
        many => format!("{many} itens apagados de vez."),
    };
    match failures {
        [] => done,
        [only] => format!("{done} 1 item continua na lixeira: {only}"),
        [first, ..] => format!(
            "{done} {} itens continuam na lixeira. O primeiro motivo: {first}",
            failures.len()
        ),
    }
}

/// What an item of the trash is called to the person.
#[cfg(feature = "desktop")]
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Capture => "Versão importada",
        other => other.label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(kind: Kind, id: i64) -> Target {
        Target { kind, id }
    }
    fn next(store: &mut Store, original: &Capture, text: &str) -> crate::model::ImportReceipt {
        let mut capture = original.clone();
        capture.request_id = uuid::Uuid::new_v4().to_string();
        capture.text = text.into();
        store.import_capture(&capture).unwrap()
    }
    #[test]
    fn document_trash_preserves_content_blocks_replay_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("document.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let capture = crate::model::sample_capture();
        let receipt = store.import_capture(&capture).unwrap();
        store
            .save_note(receipt.document_id, "NOTA_PRESERVADA")
            .unwrap();
        let context = crate::work_context::WorkContext {
            purpose: "ciencia e download".into(),
            ..Default::default()
        };
        store
            .save_work_context(receipt.document_id, &context)
            .unwrap();
        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM captures", [], |r| r.get(0))
            .unwrap();
        store
            .trash_item(target(Kind::Document, receipt.document_id))
            .unwrap();
        assert!(store.list_documents("").unwrap().is_empty());
        assert!(store.list_tasks().unwrap().is_empty());
        assert!(store.load_document(receipt.document_id).is_err());
        assert!(store.save_note(receipt.document_id, "Não gravar").is_err());
        assert!(store
            .save_work_context(receipt.document_id, &context)
            .is_err());
        assert!(store.import_capture(&capture).is_err());
        let mut fresh = capture.clone();
        fresh.request_id = uuid::Uuid::new_v4().to_string();
        assert!(store.import_capture(&fresh).is_err());
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM captures", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            count
        );
        store
            .restore_item(target(Kind::Document, receipt.document_id))
            .unwrap();
        assert_eq!(
            store.load_document(receipt.document_id).unwrap().note,
            "NOTA_PRESERVADA"
        );
        assert_eq!(store.list_tasks().unwrap().len(), 1);
        assert!(store.import_capture(&capture).unwrap().duplicate);
    }
    #[test]
    fn expired_items_are_purged_by_retention_and_recent_ones_stay() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("purge.sqlite3")).unwrap();
        assert_eq!(
            store.trash_retention_days().unwrap(),
            0,
            "never, by default"
        );
        store.set_trash_retention_days(30).unwrap();
        assert_eq!(store.trash_retention_days().unwrap(), 30);

        // Document A: trashed 40 days ago, with a task and an attachment.
        let a = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        store
            .add_attachment(
                crate::attachments::Owner::Document(a.document_id),
                "a.txt",
                "",
                b"conteudo",
            )
            .unwrap();
        store
            .trash_item(target(Kind::Document, a.document_id))
            .unwrap();
        // Document B stays; one of its tasks was trashed 40 days ago, the
        // other 10 days ago.
        let mut other = crate::model::sample_capture();
        other.document_number = "SEFAZ-PRO-2026/00077".into();
        other.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00077"
                .into();
        other.request_id = uuid::Uuid::new_v4().to_string();
        let b = store.import_capture(&other).unwrap();
        let mut old_task = Task::new(b.document_id);
        old_task.title = "Antiga".into();
        let old_task = store.save_task(&old_task).unwrap();
        let mut new_task = Task::new(b.document_id);
        new_task.title = "Recente".into();
        let new_task = store.save_task(&new_task).unwrap();
        store.trash_item(target(Kind::Task, old_task)).unwrap();
        store.trash_item(target(Kind::Task, new_task)).unwrap();
        let old = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        store
            .conn
            .execute(
                "UPDATE trash SET deleted_at=?1 WHERE (kind='document' AND item_id=?2) OR (kind='task' AND item_id=?3)",
                params![old, a.document_id, old_task],
            )
            .unwrap();

        let expired = store.expired_trash(30).unwrap();
        assert_eq!(expired.len(), 2);
        assert_eq!(expired[0].kind, Kind::Document, "documents go first");
        assert_eq!(store.purge_expired_trash(30).unwrap(), 2);
        let captures: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM captures WHERE document_id=?1",
                [a.document_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(captures, 0, "document A and its content are gone");
        assert!(store.attachment_counts("document").unwrap().is_empty());
        assert!(store.load_document(a.document_id).is_err());
        let tasks: i64 = store
            .conn
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            tasks, 2,
            "B's sample task and the recent trashed task remain"
        );
        assert_eq!(
            store.trash_entries().unwrap().len(),
            1,
            "only the recent task is still in the trash"
        );
        assert!(store.restore_item(target(Kind::Task, new_task)).is_ok());
        assert_eq!(store.purge_expired_trash(30).unwrap(), 0);
    }
    #[test]
    fn emptying_the_trash_removes_everything_and_leaves_the_rest_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("esvaziar.sqlite3")).unwrap();
        // Document A goes to the trash with an attachment; one of its tasks was
        // trashed before it.
        let a = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        store
            .add_attachment(
                crate::attachments::Owner::Document(a.document_id),
                "a.txt",
                "",
                b"conteudo",
            )
            .unwrap();
        let mut child = Task::new(a.document_id);
        child.title = "Filha".into();
        let child = store.save_task(&child).unwrap();
        store.trash_item(target(Kind::Task, child)).unwrap();
        store
            .trash_item(target(Kind::Document, a.document_id))
            .unwrap();
        // Document B stays, with one task in the trash and one in use.
        let mut other = crate::model::sample_capture();
        other.document_number = "ORG-PRO-2026/00077".into();
        other.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=ORG-PRO-2026%2F00077"
                .into();
        other.request_id = uuid::Uuid::new_v4().to_string();
        let b = store.import_capture(&other).unwrap();
        let mut gone = Task::new(b.document_id);
        gone.title = "Vai embora".into();
        let gone = store.save_task(&gone).unwrap();
        let mut kept = Task::new(b.document_id);
        kept.title = "Fica".into();
        let kept = store.save_task(&kept).unwrap();
        store.trash_item(target(Kind::Task, gone)).unwrap();
        assert_eq!(store.trash_entries().unwrap().len(), 3);

        // One item alone first: only it goes.
        store.purge_trash_item(target(Kind::Task, gone)).unwrap();
        assert_eq!(store.trash_entries().unwrap().len(), 2);
        assert!(store.purge_trash_item(target(Kind::Task, gone)).is_err());

        let (purged, failures) = store.purge_all_trash().unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(purged, 1, "the task went with its document");
        assert!(store.trash_entries().unwrap().is_empty());
        assert!(store.load_document(a.document_id).is_err());
        assert!(store.load_document(b.document_id).is_ok());
        let titles: Vec<String> = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(|task| task.title)
            .collect();
        assert!(titles.contains(&"Fica".to_owned()));
        assert!(!titles.contains(&"Vai embora".to_owned()));
        let _ = kept;
        assert_eq!(store.purge_all_trash().unwrap().0, 0);
    }
    #[test]
    fn capture_trash_selects_fallback_and_protects_saved_derivatives() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("captures.sqlite3")).unwrap();
        let first = crate::model::sample_capture();
        let a = store.import_capture(&first).unwrap();
        assert!(store
            .trash_item(target(Kind::Capture, a.capture_id))
            .is_err());
        let b = next(&mut store, &first, "Segunda versão fictícia");
        store
            .trash_item(target(Kind::Capture, b.capture_id))
            .unwrap();
        assert_eq!(
            store
                .load_document(a.document_id)
                .unwrap()
                .summary
                .latest_capture_id,
            a.capture_id
        );
        assert_eq!(store.capture_versions(a.document_id).unwrap().len(), 1);
        let mut replay = first.clone();
        replay.text = "Segunda versão fictícia".into();
        replay.request_id = uuid::Uuid::new_v4().to_string();
        assert!(store.import_capture(&replay).is_err());
        store
            .restore_item(target(Kind::Capture, b.capture_id))
            .unwrap();
        assert_eq!(
            store
                .load_document(a.document_id)
                .unwrap()
                .summary
                .latest_capture_id,
            a.capture_id
        );
        assert_eq!(store.capture_versions(a.document_id).unwrap().len(), 2);
        store
            .save_draft(a.document_id, b.capture_id, "resposta", "Minuta fictícia")
            .unwrap();
        assert!(store
            .trash_plan(target(Kind::Capture, b.capture_id))
            .unwrap()
            .blocker
            .is_some());
        assert!(store
            .trash_item(target(Kind::Capture, b.capture_id))
            .is_err());
        let c = next(&mut store, &first, "Terceira versão fictícia");
        let analysis = crate::model::Analysis {
            capture_id: c.capture_id,
            priority: "normal".into(),
            reason: "Teste".into(),
            summary: "Teste".into(),
            model: "test".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            ..Default::default()
        };
        store.save_analysis(a.document_id, &analysis).unwrap();
        assert!(store
            .trash_item(target(Kind::Capture, c.capture_id))
            .is_err());
    }
    #[test]
    fn task_trash_checks_dependencies_and_does_not_recreate_initial_task() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("tasks.sqlite3")).unwrap();
        let capture = crate::model::sample_capture();
        let receipt = store.import_capture(&capture).unwrap();
        let a = store.list_tasks().unwrap().remove(0);
        let mut b = Task::new(receipt.document_id);
        b.dependencies = vec![a.id];
        b.id = store.save_task(&b).unwrap();
        assert!(store.trash_item(target(Kind::Task, a.id)).is_err());
        store.trash_item(target(Kind::Task, b.id)).unwrap();
        store.trash_item(target(Kind::Task, a.id)).unwrap();
        assert!(store.list_tasks().unwrap().is_empty());
        assert!(store.restore_item(target(Kind::Task, b.id)).is_err());
        store.import_capture(&capture).unwrap();
        assert!(store.list_tasks().unwrap().is_empty());
        store.restore_item(target(Kind::Task, a.id)).unwrap();
        store.restore_item(target(Kind::Task, b.id)).unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 2);
        store.trash_item(target(Kind::Task, b.id)).unwrap();
        assert!(store.save_task(&b).is_err());
        store
            .trash_item(target(Kind::Document, receipt.document_id))
            .unwrap();
        assert!(store.restore_item(target(Kind::Task, b.id)).is_err());
        store
            .restore_item(target(Kind::Document, receipt.document_id))
            .unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 1);
    }
    #[test]
    fn migration_backfills_references_and_preserves_encrypted_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let capture = crate::model::sample_capture();
        let a = store.import_capture(&capture).unwrap();
        let b = next(&mut store, &capture, "Nova captura");
        store
            .save_draft(
                a.document_id,
                a.capture_id,
                "resposta",
                "MINUTA_PRIVADA_TRASH_TESTE",
            )
            .unwrap();
        store.conn.execute("DELETE FROM capture_refs", []).unwrap();
        store.conn.pragma_update(None, "user_version", 7).unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert!(store
            .trash_plan(target(Kind::Capture, a.capture_id))
            .unwrap()
            .blocker
            .is_some());
        store
            .trash_item(target(Kind::Capture, b.capture_id))
            .unwrap();
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes
            .windows(b"MINUTA_PRIVADA_TRASH_TESTE".len())
            .any(|b| b == b"MINUTA_PRIVADA_TRASH_TESTE"));
        assert!(store
            .conn
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .optional()
            .unwrap()
            .is_none());
    }
}

#[cfg(test)]
mod related_tests {
    use super::*;
    fn target(kind: Kind, id: i64) -> Target {
        Target { kind, id }
    }
    #[test]
    fn sources_and_cross_document_dependencies_survive_delete_restore() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("related.sqlite3")).unwrap();
        let first = crate::model::sample_capture();
        let a = store.import_capture(&first).unwrap();
        let mut second = first.clone();
        second.request_id = uuid::Uuid::new_v4().to_string();
        second.document_number = "TESTE-OFI-2026/00099".into();
        second.source_url="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00099".into();
        second.text = "Documento fictício relacionado, apenas para ciência.".into();
        let b = store.import_capture(&second).unwrap();
        store
            .add_document_link(
                a.document_id,
                b.document_id,
                "relacionado",
                "Vínculo local fictício",
            )
            .unwrap();
        let tasks = store.list_tasks().unwrap();
        let first_task = tasks
            .iter()
            .find(|t| t.document_id == Some(a.document_id))
            .unwrap();
        let mut other = tasks
            .iter()
            .find(|t| t.document_id == Some(b.document_id))
            .unwrap()
            .clone();
        other.dependencies = vec![first_task.id];
        store.save_task(&other).unwrap();
        assert!(store
            .trash_item(target(Kind::Document, a.document_id))
            .is_err());
        store
            .trash_item(target(Kind::Document, b.document_id))
            .unwrap();
        assert!(store.document_links(a.document_id).unwrap().is_empty());
        store
            .trash_item(target(Kind::Document, a.document_id))
            .unwrap();
        assert!(store
            .restore_item(target(Kind::Document, b.document_id))
            .is_err());
        store
            .restore_item(target(Kind::Document, a.document_id))
            .unwrap();
        store
            .restore_item(target(Kind::Document, b.document_id))
            .unwrap();
        assert_eq!(store.document_links(a.document_id).unwrap().len(), 1);
        let source = store
            .prepare_sources(&[b.document_id])
            .unwrap()
            .sources
            .remove(0);
        let newer = {
            let mut v = second.clone();
            v.request_id = uuid::Uuid::new_v4().to_string();
            v.text.push_str(" Nova versão.");
            store.import_capture(&v).unwrap()
        };
        store
            .save_draft_with_sources(
                a.document_id,
                a.capture_id,
                "resposta",
                "Minuta baseada no antecedente",
                &[source],
            )
            .unwrap();
        assert!(store
            .trash_item(target(Kind::Capture, b.capture_id))
            .is_err());
        assert!(store
            .trash_item(target(Kind::Capture, newer.capture_id))
            .is_ok());
    }
    #[test]
    fn confirmation_is_revalidated_atomically_and_inactive_dependencies_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("race.sqlite3");
        let mut first = Store::open(&path).unwrap();
        let capture = crate::model::sample_capture();
        let a = first.import_capture(&capture).unwrap();
        let mut updated = capture.clone();
        updated.request_id = uuid::Uuid::new_v4().to_string();
        updated.text.push_str(" Outra captura.");
        let b = first.import_capture(&updated).unwrap();
        assert!(first
            .trash_plan(target(Kind::Capture, b.capture_id))
            .unwrap()
            .blocker
            .is_none());
        let mut other = Store::open(&path).unwrap();
        other
            .save_draft(
                a.document_id,
                b.capture_id,
                "resposta",
                "Outra janela salvou uma minuta",
            )
            .unwrap();
        assert!(first
            .trash_item(target(Kind::Capture, b.capture_id))
            .is_err());
        let task = first.list_tasks().unwrap().remove(0);
        first.trash_item(target(Kind::Task, task.id)).unwrap();
        let mut dependent = Task::new(a.document_id);
        dependent.dependencies = vec![task.id];
        assert!(first.save_task(&dependent).is_err());
        assert!(first.list_tasks().unwrap().is_empty());
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    fn entry(
        kind: Kind,
        id: i64,
        document: Option<i64>,
        parent: bool,
        name: &str,
        belongs: &str,
    ) -> Entry {
        Entry {
            target: Target { kind, id },
            document,
            label: String::new(),
            deleted_at: String::new(),
            parent_trashed: parent,
            name: name.into(),
            belongs: belongs.into(),
        }
    }

    #[test]
    fn search_ignores_case_and_accents_and_needs_every_word() {
        let task = entry(
            Kind::Task,
            3,
            Some(1),
            false,
            "Responder ao Setor Jurídico",
            "Documento: ORG-OFI-2026/00377 — Pedido de informações sobre escala",
        );
        assert!(entry_matches(&task, ""));
        assert!(entry_matches(&task, "juridico"));
        assert!(entry_matches(&task, "JURÍDICO escala"));
        assert!(entry_matches(&task, "00377"));
        assert!(entry_matches(&task, "informacoes"));
        assert!(!entry_matches(&task, "juridico veiculos"));
        let file = entry(
            Kind::Attachment,
            4,
            None,
            false,
            "Planilha-de-Veículos.pdf",
            "Anexo da tarefa “Revisar”",
        );
        assert!(entry_matches(&file, "veiculos"));
        assert!(entry_matches(&file, "revisar"));
    }

    #[test]
    fn items_of_a_trashed_document_are_listed_under_it() {
        let entries = vec![
            entry(
                Kind::Task,
                9,
                Some(1),
                true,
                "Preparar a minuta",
                "Documento: A",
            ),
            entry(Kind::Document, 1, Some(1), false, "ORG-1 — Pedido", ""),
            entry(
                Kind::Task,
                8,
                None,
                false,
                "Outra tarefa",
                "Tarefa sem documento",
            ),
        ];
        let all = arrange(entries, "");
        let order: Vec<(i64, bool)> = all.iter().map(|(e, n)| (e.target.id, *n)).collect();
        assert_eq!(order, vec![(1, false), (9, true), (8, false)]);
    }

    #[test]
    fn a_match_inside_a_document_keeps_it_in_sight() {
        let make = || {
            vec![
                entry(
                    Kind::Task,
                    9,
                    Some(1),
                    true,
                    "Preparar a minuta",
                    "Documento: A",
                ),
                entry(Kind::Document, 1, Some(1), false, "ORG-1 — Pedido", ""),
            ]
        };
        let found = arrange(make(), "minuta");
        let order: Vec<(i64, bool)> = found.iter().map(|(e, n)| (e.target.id, *n)).collect();
        assert_eq!(order, vec![(1, false), (9, true)]);
        assert_eq!(arrange(make(), "pedido").len(), 2);
        assert!(arrange(make(), "nada disso").is_empty());
    }
}

#[cfg(all(test, feature = "desktop"))]
mod notice_tests {
    use super::*;

    #[test]
    fn the_notice_is_one_line_high_even_in_a_tall_page() {
        let ctx = eframe::egui::Context::default();
        let mut panel = Panel {
            message: "Itens excluídos há mais de 7 dias serão apagados de vez ao abrir o Ponte e uma vez por dia.".into(),
            ..Panel::default()
        };
        let mut height = f32::MAX;
        let input = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(1400.0, 1900.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            eframe::egui::CentralPanel::default().show(ui, |ui| {
                let top = ui.cursor().min.y;
                panel.message_line(ui);
                height = ui.cursor().min.y - top;
            });
        });
        output.textures_delta.clear();
        assert!(height > 20.0 && height < 60.0, "height {height}");
    }
}
