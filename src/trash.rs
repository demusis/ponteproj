//! Reversible local deletion. Protected payloads and links remain in place.
use crate::{model::Capture, protection, storage::Store, tasks::Task};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Document,
    Capture,
    Task,
}
impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Capture => "capture",
            Self::Task => "task",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "Documento",
            Self::Capture => "Captura",
            Self::Task => "Tarefa",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        match value {
            "document" => Ok(Self::Document),
            "capture" => Ok(Self::Capture),
            "task" => Ok(Self::Task),
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
    pub document: i64,
    pub label: String,
    pub explanation: String,
    pub blocker: Option<String>,
    pub replacement: Option<i64>,
}
pub struct Entry {
    pub target: Target,
    pub document: i64,
    pub label: String,
    pub deleted_at: String,
    pub parent_trashed: bool,
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
        .context("Captura não encontrada.")?;
    require_active_document(conn, doc)?;
    ensure!(
        !contains(conn, Kind::Capture, id)?,
        "Captura na lixeira. Restaure-a antes de usar ou importar novamente."
    );
    Ok(doc)
}
pub fn require_active_task(conn: &Connection, id: i64) -> Result<i64> {
    let doc: i64 = conn
        .query_row("SELECT document_id FROM tasks WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .context("Tarefa não encontrada.")?;
    require_active_document(conn, doc)?;
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
    document_id: i64,
    dependencies: Vec<i64>,
}
fn all_tasks(conn: &Connection) -> Result<Vec<TaskNode>> {
    let count: i64 = conn.query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))?;
    ensure!(
        count <= 10000,
        "Quantidade de tarefas excede o limite de validação."
    );
    let mut statement = conn.prepare("SELECT id,document_id,payload FROM tasks")?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
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
                contains(conn, Kind::Document, task.document_id),
                contains(conn, Kind::Task, task.id),
            ) {
                (Ok(false), Ok(false)) => Some(Ok(task)),
                (Ok(_), Ok(_)) => None,
                (Err(e), _) | (_, Err(e)) => Some(Err(e)),
            }
        })
        .collect()
}
fn label(conn: &Connection, kind: Kind, id: i64) -> Result<(i64, String)> {
    match kind {
        Kind::Document => {
            let bytes:Vec<u8>=conn.query_row("SELECT c.payload FROM documents d JOIN captures c ON c.id=d.latest_capture_id WHERE d.id=?1",[id],|r|r.get(0))?;
            let capture: Capture = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            Ok((
                id,
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
                doc,
                format!(
                    "Captura #{id} — {} — {}",
                    capture.document_number, capture.captured_at
                ),
            ))
        }
        Kind::Task => {
            let (doc, bytes): (i64, Vec<u8>) = conn.query_row(
                "SELECT document_id,payload FROM tasks WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let task: Task = serde_json::from_slice(&protection::unseal(&bytes)?)?;
            Ok((doc, format!("Tarefa #{id} — {}", task.title)))
        }
    }
}
fn deletion_plan(conn: &Connection, target: Target) -> Result<Plan> {
    let (doc, label) = label(conn, target.kind, target.id)?;
    require_active_document(conn, doc)?;
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
            let remaining:Option<i64>=conn.query_row("SELECT c.id FROM captures c WHERE c.document_id=?1 AND c.id<>?2 AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='capture' AND t.item_id=c.id) ORDER BY c.id DESC LIMIT 1",params![doc,target.id],|r|r.get(0)).optional()?;
            let current: i64 = conn.query_row(
                "SELECT latest_capture_id FROM documents WHERE id=?1",
                [doc],
                |r| r.get(0),
            )?;
            if refs > 0 {
                plan.blocker=Some(format!("Esta captura é utilizada em {refs} registro(s) salvo(s) de minuta/análise ou fontes. Ela é preservada para manter essas bases."));
            } else if remaining.is_none() {
                plan.blocker=Some("Esta é a única captura ativa. Para retirar todo o conteúdo da biblioteca, exclua o documento para a lixeira.".into());
            }
            if current == target.id {
                plan.replacement = remaining;
                plan.explanation=format!("Esta é a captura em uso. Após a exclusão, a ficha usará a captura #{}. PDFs e dados da captura excluída ficarão preservados na lixeira.",remaining.unwrap_or(current));
            } else {
                plan.explanation="A captura sairá do histórico ativo; PDFs e conteúdo ficarão preservados para restauração.".into();
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
                plan.blocker=Some(format!("Há {dependents} tarefa(s) ativa(s) que dependem deste item. Revise as dependências antes de excluir."));
            }
            plan.explanation=if target.kind==Kind::Document{"O documento sairá da biblioteca e suas tarefas ficarão ocultas. Capturas, PDFs, minutas, notas, OCR e vínculos serão preservados para restauração. Itens já excluídos individualmente continuarão na lixeira."}else{"A tarefa sairá do painel. Seus campos, checklist e resultado serão preservados na lixeira."}.into();
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
        tx.execute_batch("CREATE TABLE IF NOT EXISTS trash(kind TEXT NOT NULL CHECK(kind IN ('document','capture','task')),item_id INTEGER NOT NULL,document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,deleted_at TEXT NOT NULL,PRIMARY KEY(kind,item_id)); CREATE INDEX IF NOT EXISTS trash_document ON trash(document_id); CREATE TABLE IF NOT EXISTS capture_refs(capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE,owner_kind TEXT NOT NULL CHECK(owner_kind IN ('draft','analysis')),owner_id INTEGER NOT NULL,owner_document INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,PRIMARY KEY(capture_id,owner_kind,owner_id));")?;
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
        tx.pragma_update(None, "user_version", 8)?;
        tx.commit()?;
        Ok(())
    }
    pub fn trash_plan(&self, target: Target) -> Result<Plan> {
        deletion_plan(&self.conn, target)
    }
    pub fn trash_item(&mut self, target: Target) -> Result<i64> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::TrashItem {
                    kind: target.kind.key().into(),
                    id: target.id,
                },
            )? {
                crate::intranet::MutationReply::Id(doc) => return Ok(doc),
                _ => anyhow::bail!("Resposta inesperada do anfitrião ao excluir item."),
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
            Some(plan.document),
            &format!("Moveu para a lixeira: {}", plan.label),
        )?;
        tx.commit()?;
        Ok(plan.document)
    }
    pub fn trash_entries(&self) -> Result<Vec<Entry>> {
        let mut statement=self.conn.prepare("SELECT kind,item_id,document_id,deleted_at FROM trash ORDER BY deleted_at DESC,kind,item_id")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (kind, id, document, deleted_at) = row?;
            let kind = Kind::parse(&kind)?;
            let (_, label) = label(&self.conn, kind, id)?;
            let parent_trashed =
                kind != Kind::Document && contains(&self.conn, Kind::Document, document)?;
            Ok(Entry {
                target: Target { kind, id },
                document,
                label,
                deleted_at,
                parent_trashed,
            })
        })
        .collect()
    }
    pub fn restore_item(&mut self, target: Target) -> Result<i64> {
        if let Some(cfg) = self.client_config()? {
            match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::RestoreItem {
                    kind: target.kind.key().into(),
                    id: target.id,
                },
            )? {
                crate::intranet::MutationReply::Id(doc) => return Ok(doc),
                _ => anyhow::bail!("Resposta inesperada do anfitrião ao restaurar item."),
            }
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let document: i64 = tx
            .query_row(
                "SELECT document_id FROM trash WHERE kind=?1 AND item_id=?2",
                params![target.kind.key(), target.id],
                |r| r.get(0),
            )
            .optional()?
            .context("O item não está na lixeira.")?;
        if target.kind != Kind::Document {
            require_active_document(&tx, document)?;
        }
        if target.kind != Kind::Capture {
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
            Some(document),
            &format!("Restaurou da lixeira: {} #{}", target.kind.label(), target.id),
        )?;
        tx.commit()?;
        Ok(document)
    }
}
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    pub pending: Option<Target>,
    message: String,
}
pub struct Change {
    pub target: Target,
    pub document: i64,
    pub restored: bool,
}
impl Panel {
    pub fn request(&mut self, target: Target) {
        self.pending = Some(target);
        self.message.clear();
    }
    pub fn show(
        &mut self,
        ctx: &eframe::egui::Context,
        store: &mut Store,
        busy: bool,
    ) -> Option<Change> {
        use eframe::egui;
        let mut change = None;
        let confirming = self.pending.is_some();
        if let Some(target) = self.pending.clone() {
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("Excluir para a lixeira?").collapsible(false).resizable(false).default_width(590.0).anchor(egui::Align2::CENTER_CENTER,[0.0,0.0]).show(ctx,|ui|{
                match store.trash_plan(target.clone()){
                    Ok(plan)=>{ui.strong(&plan.label);ui.label(&plan.explanation);ui.small("A exclusão é local e reversível. Nada será excluído ou tramitado no SIGADOC; arquivos exportados e backups permanecem.");if let Some(blocker)=&plan.blocker{ui.colored_label(egui::Color32::DARK_RED,blocker);}ui.horizontal(|ui|{cancel=ui.button("Cancelar").clicked();confirm=ui.add_enabled(!busy&&plan.blocker.is_none(),egui::Button::new("Mover para a lixeira")).clicked();});},Err(error)=>{ui.label(error.to_string());cancel=ui.button("Fechar").clicked();}
                }
                if !self.message.is_empty(){ui.label(&self.message);}
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
        if self.open {
            let mut open = self.open;
            egui::Window::new("Lixeira local").open(&mut open).default_size([830.0,560.0]).show(ctx,|ui|{
                ui.small("Itens excluídos podem ser restaurados a qualquer momento.").on_hover_text("A lixeira não é esvaziada automaticamente e os arquivos continuam ocupando espaço no acervo protegido.");
                if !self.message.is_empty(){ui.label(&self.message);}
                match store.trash_entries(){Ok(entries)=>{
                    if entries.is_empty(){ui.label("A lixeira está vazia.");}
                    egui::ScrollArea::vertical().max_height(420.0).show(ui,|ui|{for entry in entries{ui.group(|ui|{
                        ui.set_width(ui.available_width());
                        ui.strong(format!("{} • {}",entry.target.kind.label(),entry.label));ui.small(format!("Excluído localmente em {}",chrono::DateTime::parse_from_rfc3339(&entry.deleted_at).map(|date|date.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_else(|_|entry.deleted_at.clone())));
                        if entry.parent_trashed{ui.label("Restaure o documento primeiro para recuperar este item individualmente.");}
                        if entry.target.kind==Kind::Capture{ui.small("Restaurar devolve a captura ao histórico; não troca a captura em uso.");}
                        if ui.add_enabled(!busy&&!entry.parent_trashed&&!confirming,egui::Button::new("Restaurar")).clicked(){match store.restore_item(entry.target.clone()){Ok(document)=>{self.message="Item restaurado. A biblioteca e o painel foram atualizados.".into();change=Some(Change{target:entry.target,document,restored:true});},Err(error)=>self.message=error.to_string()}}
                    });}});
                },Err(error)=>{ui.label(format!("Não foi possível ler a lixeira: {error}"));}}
            });
            self.open = open;
        }
        change
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
            .find(|t| t.document_id == a.document_id)
            .unwrap();
        let mut other = tasks
            .iter()
            .find(|t| t.document_id == b.document_id)
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
