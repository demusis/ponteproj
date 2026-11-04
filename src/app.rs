//! Native desktop workspace. Document content is only persisted through `Store`.
use crate::ai::{self, AiConfig, AiProvider, AiSettings, ModelInfo};
use crate::model::{Analysis, DocumentDetail, DocumentSummary};
use crate::storage::Store;
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(24, 84, 97);
const WARNING: Color32 = Color32::from_rgb(153, 88, 16);
const ERROR: Color32 = Color32::from_rgb(168, 40, 45);
const SUCCESS: Color32 = Color32::from_rgb(20, 118, 72);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocTaskFilter {
    #[default]
    All,
    OpenAny,
    OpenOnTime,
    Overdue,
    Completed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocPriorityFilter {
    #[default]
    All,
    Urgent,
    High,
    Normal,
    Low,
    Unclassified,
}

impl DocPriorityFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "Todas as prioridades",
            Self::Urgent => "Urgente",
            Self::High => "Alta",
            Self::Normal => "Normal",
            Self::Low => "Baixa",
            Self::Unclassified => "Não classificada",
        }
    }

    fn matches(self, priority: &str) -> bool {
        match self {
            Self::All => true,
            Self::Urgent => priority == "urgente",
            Self::High => priority == "alta",
            Self::Normal => priority == "normal",
            Self::Low => priority == "baixa",
            Self::Unclassified => priority.is_empty() || priority == "não classificada",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DocTaskStatusSummary {
    open_on_time: usize,
    overdue: usize,
    completed: usize,
    due_today: usize,
    nearest_due: Option<String>,
}

impl DocTaskStatusSummary {
    fn from_tasks(tasks: &[crate::tasks::Task], document_id: i64, today_iso: &str) -> Self {
        let mut summary = Self::default();
        for task in tasks.iter().filter(|t| t.document_id == document_id) {
            if task.status == "concluida" {
                summary.completed += 1;
            } else {
                let is_overdue = !task.due_date.is_empty() && task.due_date.as_str() < today_iso;
                if is_overdue {
                    summary.overdue += 1;
                } else {
                    summary.open_on_time += 1;
                    if task.due_date == today_iso {
                        summary.due_today += 1;
                    }
                }
                if !task.due_date.is_empty() {
                    match &summary.nearest_due {
                        Some(current) if current.as_str() <= task.due_date.as_str() => {}
                        _ => summary.nearest_due = Some(task.due_date.clone()),
                    }
                }
            }
        }
        summary
    }

    fn matches(&self, filter: DocTaskFilter) -> bool {
        match filter {
            DocTaskFilter::All => true,
            DocTaskFilter::OpenAny => self.overdue > 0 || self.open_on_time > 0,
            DocTaskFilter::OpenOnTime => self.overdue == 0 && self.open_on_time > 0,
            DocTaskFilter::Overdue => self.overdue > 0,
            DocTaskFilter::Completed => {
                self.overdue == 0 && self.open_on_time == 0 && self.completed > 0
            }
        }
    }

    fn stripe_color(&self) -> Color32 {
        if self.overdue > 0 {
            ERROR
        } else if self.due_today > 0 {
            WARNING
        } else if self.open_on_time > 0 {
            ACCENT
        } else if self.completed > 0 {
            SUCCESS
        } else {
            Color32::from_rgb(160, 170, 180)
        }
    }

    fn badge(&self) -> (String, Color32, Color32) {
        if self.overdue > 0 {
            let mut label = if self.overdue == 1 {
                "⚠ 1 tarefa atrasada".to_owned()
            } else {
                format!("⚠ {} tarefas atrasadas", self.overdue)
            };
            if self.open_on_time > 0 {
                label.push_str(&format!(" (+{} no prazo)", self.open_on_time));
            }
            if let Some(due) = &self.nearest_due {
                label.push_str(&format!(" • Prazo: {}", crate::labels::date(due)));
            }
            (label, ERROR, Color32::from_rgb(252, 232, 233))
        } else if self.open_on_time > 0 {
            let mut label = if self.due_today > 0 {
                format!("⏰ {} em aberto (vence hoje)", self.open_on_time)
            } else if self.open_on_time == 1 {
                "● 1 em aberto (no prazo)".to_owned()
            } else {
                format!("● {} em aberto (no prazo)", self.open_on_time)
            };
            if let Some(due) = &self.nearest_due {
                if self.due_today == 0 {
                    label.push_str(&format!(" • Prazo: {}", crate::labels::date(due)));
                }
            }
            if self.completed > 0 {
                label.push_str(&format!(" • ✓ {}", self.completed));
            }
            if self.due_today > 0 {
                (label, WARNING, Color32::from_rgb(254, 243, 224))
            } else {
                (label, ACCENT, Color32::from_rgb(228, 243, 246))
            }
        } else if self.completed > 0 {
            let label = if self.completed == 1 {
                "✓ 1 tarefa concluída".to_owned()
            } else {
                format!("✓ {} tarefas concluídas", self.completed)
            };
            (label, SUCCESS, Color32::from_rgb(228, 246, 236))
        } else {
            (
                "○ Sem tarefas".to_owned(),
                Color32::from_rgb(100, 112, 125),
                Color32::from_rgb(238, 240, 242),
            )
        }
    }
}

pub fn run(initial_document: Option<i64>) -> Result<()> {
    let db_path = crate::paths::db_path()?;
    let store = Store::open(&db_path).context("Não foi possível abrir a biblioteca protegida")?;
    let mut app = PonteApp::new(store, db_path);
    app.intranet.initialize(&mut app.store, &app.db_path);
    app.load_ai_config();
    app.refresh();
    if let Some(id) = initial_document {
        app.select(id);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!(
                "Ponte {} • Assistente documental",
                env!("CARGO_PKG_VERSION")
            ))
            .with_icon(crate::icon::app_window_icon())
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([900.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Ponte — Assistente documental",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            install_symbol_fallback(&cc.egui_ctx);
            cc.egui_ctx.global_style_mut(|style| {
                style.spacing.item_spacing = egui::vec2(9.0, 8.0);
                style.spacing.button_padding = egui::vec2(11.0, 7.0);
                style.visuals.selection.bg_fill = ACCENT;
                style.visuals.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
            });
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Não foi possível iniciar a janela: {error}"))
}

/// The bundled egui fonts lack some arrows (e.g. "↳"). Windows ships Segoe UI
/// Symbol; when present it is only appended as the last fallback.
fn install_symbol_fallback(ctx: &egui::Context) {
    let Some(windows) = std::env::var_os("WINDIR") else {
        return;
    };
    let path = PathBuf::from(windows).join("Fonts").join("seguisym.ttf");
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "segoe-ui-symbol".into(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("segoe-ui-symbol".into());
    }
    ctx.set_fonts(fonts);
}

const JOINED_FG: Color32 = Color32::from_rgb(88, 64, 138);
const JOINED_BG: Color32 = Color32::from_rgb(237, 232, 248);

fn pill(ui: &mut egui::Ui, text: impl Into<String>, fg: Color32, bg: Color32, size: f32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text.into()).size(size).color(fg).strong());
        });
}

/// Pill text for a juntado document, or None for an ordinary one.
fn joined_label(doc: &DocumentSummary) -> Option<String> {
    match (&doc.joined_to_number, doc.is_joined) {
        (Some(number), _) if doc.joined_to_id.is_some() => Some(format!("↳ Juntado a {number}")),
        (Some(number), _) => Some(format!("↳ Juntado a {number} (não importado)")),
        (None, true) => Some("↳ Documento juntado".into()),
        (None, false) => None,
    }
}

/// Orders the visible list so juntados sit right below their visible parent,
/// keeping the original order otherwise. Returns each document with its depth.
fn group_joined<'a>(docs: &[&'a DocumentSummary]) -> Vec<(&'a DocumentSummary, usize)> {
    use std::collections::BTreeSet;
    let visible: BTreeSet<i64> = docs.iter().map(|d| d.id).collect();
    let parent_of = |d: &DocumentSummary| {
        d.joined_to_id
            .filter(|parent| *parent != d.id && visible.contains(parent))
    };
    let mut children: BTreeMap<i64, Vec<&'a DocumentSummary>> = BTreeMap::new();
    for doc in docs {
        if let Some(parent) = parent_of(doc) {
            children.entry(parent).or_default().push(*doc);
        }
    }
    fn place<'a>(
        doc: &'a DocumentSummary,
        depth: usize,
        children: &BTreeMap<i64, Vec<&'a DocumentSummary>>,
        placed: &mut BTreeSet<i64>,
        out: &mut Vec<(&'a DocumentSummary, usize)>,
    ) {
        if !placed.insert(doc.id) {
            return;
        }
        out.push((doc, depth));
        for child in children.get(&doc.id).into_iter().flatten() {
            place(*child, depth + 1, children, placed, out);
        }
    }
    let mut placed = BTreeSet::new();
    let mut out = Vec::with_capacity(docs.len());
    for doc in docs.iter().filter(|d| parent_of(d).is_none()) {
        place(*doc, 0, &children, &mut placed, &mut out);
    }
    // Defensive: a cycle of juntadas has no root; keep every document listed.
    for doc in docs {
        place(*doc, 0, &children, &mut placed, &mut out);
    }
    out
}

/// The four views of an open document. Everything technical lives in `More`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Document,
    Notes,
    More,
}

/// Size limits of the document list: wide enough for the divider to visibly
/// follow the drag, while the central area keeps at least 40% of the window.
const LIBRARY_DEFAULT_WIDTH: f32 = 320.0;
const LIBRARY_MIN_WIDTH: f32 = 180.0;
const LIBRARY_MAX_FRACTION: f32 = 0.6;

/// The resizable left panel with the document list, shared by the window and
/// by the layout test so both use the same limits.
fn library_panel(window_width: f32) -> egui::Panel {
    let max = (window_width * LIBRARY_MAX_FRACTION).max(LIBRARY_MIN_WIDTH);
    egui::Panel::left("library")
        .default_size(LIBRARY_DEFAULT_WIDTH)
        .min_size(LIBRARY_MIN_WIDTH)
        .max_size(max)
        .resizable(true)
}

/// Small "ⓘ" that explains on hover, instead of a permanent paragraph.
fn info(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new("ⓘ").color(Color32::from_rgb(120, 130, 140)))
        .on_hover_text(text);
}

struct Editor {
    materials: Vec<crate::evidence::Source>,
    note: String,
    saved_note: String,
    note_version: Option<i64>,
    body: String,
    saved_body: String,
    kind: String,
    saved_kind: String,
    capture_id: i64,
    saved_capture_id: i64,
    origin: String,
    instructions: String,
}

impl Editor {
    fn new(detail: &DocumentDetail, note_version: Option<i64>) -> Self {
        let capture_id = detail.summary.latest_capture_id;
        Self {
            materials: Vec::new(),
            note: detail.note.clone(),
            saved_note: detail.note.clone(),
            note_version,
            body: String::new(),
            saved_body: String::new(),
            kind: "resposta".into(),
            saved_kind: "resposta".into(),
            capture_id,
            saved_capture_id: capture_id,
            origin: "Editor vazio".into(),
            instructions: String::new(),
        }
    }

    fn note_dirty(&self) -> bool {
        self.note != self.saved_note
    }

    fn draft_dirty(&self) -> bool {
        self.body != self.saved_body
            || (!self.body.is_empty()
                && (self.kind != self.saved_kind || self.capture_id != self.saved_capture_id))
    }

    fn dirty(&self) -> bool {
        self.note_dirty() || self.draft_dirty()
    }

    fn mark_draft_saved(&mut self) {
        self.saved_body = self.body.clone();
        self.saved_kind = self.kind.clone();
        self.saved_capture_id = self.capture_id;
    }
}

struct Replacement {
    materials: Vec<crate::evidence::Source>,
    document_id: i64,
    capture_id: i64,
    kind: String,
    body: String,
    origin: String,
    saved: bool,
    from_suggestion: bool,
}

#[derive(Clone)]
struct Suggestion {
    materials: Vec<crate::evidence::Source>,
    document_id: i64,
    number: String,
    capture_id: i64,
    kind: String,
    body: String,
}

enum WorkResult {
    Sources {
        document_id: i64,
        result: std::result::Result<crate::evidence::Prepared, String>,
    },
    Analysis {
        document_id: i64,
        result: std::result::Result<Analysis, String>,
    },
    Draft {
        materials: Vec<crate::evidence::Source>,
        document_id: i64,
        number: String,
        capture_id: i64,
        kind: String,
        result: std::result::Result<String, String>,
    },
    Backup(std::result::Result<PathBuf, String>),
}

struct Worker {
    receiver: Receiver<WorkResult>,
    label: String,
    started: Instant,
}

struct Notice {
    text: String,
    error: bool,
    since: Instant,
}

/// Success notices leave by themselves; errors stay until closed.
const NOTICE_LIFETIME: Duration = Duration::from_secs(7);

/// Something not yet saved, listed when the person tries to close the window.
struct Pending {
    label: String,
    go: PendingTarget,
}

#[derive(Clone, Copy)]
enum PendingTarget {
    Document(i64, Tab),
    Tasks,
    Saiop,
    Suggestion,
    Settings,
    None,
}

struct PonteApp {
    draft_tools: crate::draft_tools::DraftTools,
    sources: crate::evidence::SourcesPanel,
    pdf_reader: crate::pdf_reader::PdfPanel,
    dossier: crate::dossier::DossierPanel,
    work_context: crate::work_context::ContextPanel,
    tasks: crate::tasks_ui::TaskPanel,
    saiop: crate::saiop::SaiopPanel,
    intranet: crate::intranet::IntranetPanel,
    tasks_open: bool,
    saiop_open: bool,
    store: Store,
    db_path: PathBuf,
    documents: Vec<DocumentSummary>,
    selected_id: Option<i64>,
    detail: Option<DocumentDetail>,
    similar: Vec<DocumentSummary>,
    /// Active documents juntados to the open document (whole library).
    joined_children: Vec<DocumentSummary>,
    editors: BTreeMap<i64, Editor>,
    query: String,
    search_changed: Option<Instant>,
    last_refresh: Instant,
    tab: Tab,
    settings_open: bool,
    /// AI configuration being edited (and used by AI actions).
    ai: AiConfig,
    /// What is saved in the library; differs from `ai` while unsaved.
    ai_saved: AiConfig,
    /// Models listed by the server, with the provider and endpoint they came from.
    models: Vec<ModelInfo>,
    models_for: Option<(AiProvider, String)>,
    models_error: Option<String>,
    model_fetch: Option<Receiver<std::result::Result<Vec<ModelInfo>, String>>>,
    worker: Option<Worker>,
    suggestion: Option<Suggestion>,
    suggestion_open: bool,
    replacement: Option<Replacement>,
    trash: crate::trash::Panel,
    exit_confirm: bool,
    allow_close: bool,
    notice: Option<Notice>,
    backup_path: Option<PathBuf>,
    doc_task_filter: DocTaskFilter,
    doc_priority_filter: DocPriorityFilter,
    filters_open: bool,
    /// Browser-bridge installation check, refreshed every few seconds while
    /// the welcome screen is visible.
    install_status: Option<(Instant, crate::windows_ui::InstallStatus)>,
}

impl PonteApp {
    fn new(store: Store, db_path: PathBuf) -> Self {
        Self {
            draft_tools: Default::default(),
            sources: Default::default(),
            pdf_reader: Default::default(),
            dossier: Default::default(),
            work_context: Default::default(),
            tasks: Default::default(),
            saiop: Default::default(),
            intranet: Default::default(),
            // The welcome screen (or the opened document) is the first thing seen.
            tasks_open: false,
            saiop_open: false,
            store,
            db_path,
            documents: Vec::new(),
            selected_id: None,
            detail: None,
            similar: Vec::new(),
            joined_children: Vec::new(),
            editors: BTreeMap::new(),
            query: String::new(),
            search_changed: None,
            last_refresh: Instant::now(),
            tab: Tab::Overview,
            settings_open: false,
            ai: AiConfig::default(),
            ai_saved: AiConfig::default(),
            models: Vec::new(),
            models_for: None,
            models_error: None,
            model_fetch: None,
            worker: None,
            suggestion: None,
            suggestion_open: false,
            replacement: None,
            trash: Default::default(),
            exit_confirm: false,
            allow_close: false,
            notice: None,
            backup_path: None,
            doc_task_filter: DocTaskFilter::All,
            doc_priority_filter: DocPriorityFilter::All,
            filters_open: false,
            install_status: None,
        }
    }

    fn install_status(&mut self) -> crate::windows_ui::InstallStatus {
        let stale = self
            .install_status
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(10));
        if stale {
            self.install_status = Some((Instant::now(), crate::windows_ui::installation_status()));
        }
        self.install_status
            .as_ref()
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    }

    /// First screen: how to import, and whether the browser bridge is ready.
    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(32.0);
        ui.heading("Seu espaço de trabalho documental");
        ui.label("Escolha um documento na lista à esquerda, ou importe um novo pelo navegador.");
        ui.add_space(16.0);
        ui.strong("Para importar um documento");
        ui.label("1. Abra o documento no SIGADOC, no Chrome ou no Edge.");
        ui.label("2. Clique em “Importar para o assistente”, no canto da página.");
        ui.label("3. O documento aparece aqui com o texto, os PDFs e o histórico.");
        ui.add_space(16.0);
        let status = self.install_status();
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong("Ligação com o navegador");
            let ok = status.browser_bridge_ok();
            let browsers = match (status.chrome_registered, status.edge_registered) {
                (true, true) => "Chrome e Edge",
                (true, false) => "Chrome",
                (false, true) => "Edge",
                (false, false) => "nenhum navegador",
            };
            if ok {
                ui.colored_label(SUCCESS, format!("✓ O Ponte está registrado para o {browsers}."));
                ui.small("Se o botão “Importar para o assistente” não aparece no SIGADOC, a extensão ainda não foi carregada no navegador: siga os passos abaixo.");
            } else if status.chrome_registered || status.edge_registered {
                ui.colored_label(WARNING, format!("⚠ Registrado para o {browsers}, mas o programa apontado não foi encontrado. Reinstale o Ponte."));
            } else {
                ui.colored_label(ERROR, "✗ O navegador ainda não foi ligado ao Ponte. Execute o instalador neste computador, com a sua conta do Windows.");
            }
            match &status.extension_dir {
                Some(dir) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.small("Extensão do navegador:");
                        ui.small(dir.display().to_string());
                        if ui.small_button("Copiar caminho").on_hover_text("Cole este caminho em “Carregar sem compactação”, na página de extensões do navegador.").clicked() {
                            ui.ctx().copy_text(dir.display().to_string());
                            self.message("Caminho copiado. No navegador, abra a página de extensões, ative o Modo do desenvolvedor, clique em “Carregar sem compactação” e cole o caminho.");
                        }
                        if ui.small_button("Abrir pasta").clicked() {
                            crate::windows_ui::reveal_in_explorer(dir);
                        }
                    });
                }
                None => {
                    ui.small("A pasta da extensão do navegador não foi encontrada ao lado do programa.");
                }
            }
        });
        ui.add_space(12.0);
        ui.small("A IA é opcional e só funciona quando você clica. Assinaturas e tramitação continuam no SIGADOC.");
    }

    fn message(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: false,
            since: Instant::now(),
        });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: true,
            since: Instant::now(),
        });
    }

    /// Everything unsaved or still running, with where to go to deal with it.
    fn pending_items(&self) -> Vec<Pending> {
        let number = |id: i64| {
            self.documents
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.document_number.clone())
                .unwrap_or_else(|| format!("documento #{id}"))
        };
        let mut out = Vec::new();
        for (id, editor) in &self.editors {
            if editor.note_dirty() {
                out.push(Pending {
                    label: format!("Notas de {} não salvas", number(*id)),
                    go: PendingTarget::Document(*id, Tab::Notes),
                });
            }
            if editor.draft_dirty() {
                out.push(Pending {
                    label: format!("Minuta de {} não salva", number(*id)),
                    go: PendingTarget::Document(*id, Tab::Notes),
                });
            }
        }
        for id in self.work_context.dirty_documents() {
            out.push(Pending {
                label: format!("Contexto de trabalho de {} não salvo", number(id)),
                go: PendingTarget::Document(id, Tab::Overview),
            });
        }
        for id in self.dossier.dirty_documents() {
            out.push(Pending {
                label: format!("Vínculo de {} em edição", number(id)),
                go: PendingTarget::Document(id, Tab::More),
            });
        }
        if self.draft_tools.dirty() {
            out.push(Pending {
                label: "Modelo pessoal de minuta em edição".into(),
                go: self
                    .selected_id
                    .map(|id| PendingTarget::Document(id, Tab::Notes))
                    .unwrap_or(PendingTarget::None),
            });
        }
        if self.tasks.dirty() {
            out.push(Pending {
                label: "Tarefa em edição no painel Tarefas".into(),
                go: PendingTarget::Tasks,
            });
        }
        if self.saiop.dirty() {
            out.push(Pending {
                label: "Atendimento em edição em Processos e demandas".into(),
                go: PendingTarget::Saiop,
            });
        }
        if let Some(suggestion) = &self.suggestion {
            out.push(Pending {
                label: format!("Sugestão de minuta de {} ainda não revisada", suggestion.number),
                go: PendingTarget::Suggestion,
            });
        }
        if self.ai_config_dirty() {
            out.push(Pending {
                label: "Configuração de IA não salva".into(),
                go: PendingTarget::Settings,
            });
        }
        if let Some(worker) = &self.worker {
            out.push(Pending {
                label: format!("Operação em andamento: {}", worker.label),
                go: PendingTarget::None,
            });
        }
        if self.pdf_reader.busy() {
            out.push(Pending {
                label: "Leitura de PDF em andamento".into(),
                go: PendingTarget::None,
            });
        }
        out
    }

    fn go_to(&mut self, target: PendingTarget) {
        match target {
            PendingTarget::Document(id, tab) => {
                self.select(id);
                self.tab = tab;
            }
            PendingTarget::Tasks => {
                self.tasks_open = true;
                self.saiop_open = false;
            }
            PendingTarget::Saiop => {
                self.saiop_open = true;
                self.tasks_open = false;
            }
            PendingTarget::Suggestion => self.suggestion_open = true,
            PendingTarget::Settings => self.settings_open = true,
            PendingTarget::None => {}
        }
    }

    /// Color, short label and full description of the team/network state.
    fn network_status(&self) -> (Color32, String, String) {
        let full = self.intranet.banner_text(&self.store);
        let Ok(cfg) = self.store.load_intranet_config() else {
            return (Color32::from_rgb(140, 150, 160), "Só neste computador".into(), full);
        };
        let conflicts = self.store.list_sync_conflicts().map(|v| v.len()).unwrap_or(0);
        let outbox = self.store.list_offline_outbox().map(|v| v.len()).unwrap_or(0);
        if conflicts > 0 {
            return (
                ERROR,
                format!("⚠ {conflicts} conflito(s) de edição"),
                format!("{full}\n\nAbra Rede e equipe para decidir campo a campo."),
            );
        }
        match cfg.mode {
            crate::intranet::IntranetMode::Local => {
                (Color32::from_rgb(140, 150, 160), "Só neste computador".into(), full)
            }
            crate::intranet::IntranetMode::Host => {
                if self.intranet.host_running() {
                    (SUCCESS, "Computador principal ativo".into(), full)
                } else {
                    (WARNING, "Computador principal parado".into(), full)
                }
            }
            crate::intranet::IntranetMode::Client => {
                if self.intranet.offline {
                    (
                        WARNING,
                        if outbox > 0 {
                            format!("Sem conexão • {outbox} a enviar")
                        } else {
                            "Sem conexão com o principal".into()
                        },
                        full,
                    )
                } else if let Some((at, _)) = self.intranet.last_sync_finished {
                    let secs = at.elapsed().as_secs();
                    let when = if secs < 60 {
                        "agora".to_owned()
                    } else {
                        format!("há {} min", secs / 60)
                    };
                    (SUCCESS, format!("Conectado • atualizado {when}"), full)
                } else {
                    (ACCENT, "Conectando ao principal…".into(), full)
                }
            }
        }
    }

    fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        self.intranet.refresh_client(&mut self.store, &self.db_path);
        match self.store.list_documents(&self.query) {
            Ok(documents) => self.documents = documents,
            Err(error) => self.error(format!("Falha ao atualizar a biblioteca: {error}")),
        }
        if let Some(id) = self.selected_id {
            self.reload(id);
        }
    }

    fn reload(&mut self, id: i64) {
        let note_version = self
            .store
            .item_version("note", id)
            .ok()
            .map(|meta| meta.version);
        match self.store.load_document(id) {
            Ok(detail) => {
                let editor = self
                    .editors
                    .entry(id)
                    .or_insert_with(|| Editor::new(&detail, note_version));
                // Refresh never replaces edits that have not been explicitly saved.
                if !editor.note_dirty() {
                    editor.note = detail.note.clone();
                    editor.saved_note = detail.note.clone();
                    editor.note_version = note_version;
                }
                if editor.body.is_empty() && !editor.draft_dirty() {
                    editor.capture_id = detail.summary.latest_capture_id;
                    editor.saved_capture_id = editor.capture_id;
                }
                if self.selected_id == Some(id) {
                    let mut detail = detail;
                    // An unfiltered library list already covers every document.
                    let children = if self.query.trim().is_empty() {
                        Ok(self
                            .documents
                            .iter()
                            .filter(|d| d.joined_to_id == Some(id))
                            .cloned()
                            .collect())
                    } else {
                        self.store.joined_children(id)
                    };
                    match children {
                        Ok(children) => self.joined_children = children,
                        Err(error) => {
                            self.joined_children.clear();
                            self.error(format!("Falha ao ler documentos juntados: {error}"));
                        }
                    }
                    detail.summary.joined_children_count = self.joined_children.len();
                    self.detail = Some(detail);
                    match self.store.similar_documents(id, 5) {
                        Ok(items) => self.similar = items,
                        Err(error) => self.error(format!("Falha ao buscar similares: {error}")),
                    }
                }
            }
            Err(error) => self.error(format!("Não foi possível carregar o documento: {error}")),
        }
    }

    fn select(&mut self, id: i64) {
        self.tasks_open = false;
        self.saiop_open = false;
        self.selected_id = Some(id);
        self.detail = None;
        self.similar.clear();
        self.joined_children.clear();
        self.reload(id);
    }

    fn settings(&self) -> Result<AiSettings> {
        anyhow::ensure!(
            !self.ai.model().trim().is_empty(),
            "Escolha o modelo em Configurar IA."
        );
        self.ai.chat_settings()
    }

    /// Reads the saved configuration once, at start. Unreadable content
    /// (another Windows account) keeps the defaults and says so.
    fn load_ai_config(&mut self) {
        match self.store.load_ai_config() {
            Ok(config) => {
                self.ai = config.clone();
                self.ai_saved = config;
            }
            Err(error) => self.error(format!("Configuração de IA não carregada: {error}")),
        }
    }

    fn ai_config_dirty(&self) -> bool {
        self.ai != self.ai_saved
    }

    /// Persists the configuration being edited. Returns false (error shown)
    /// when it is invalid or could not be written.
    fn save_ai_config(&mut self) -> bool {
        if let Err(error) = self.ai.validate() {
            self.error(error.to_string());
            return false;
        }
        match self.store.save_ai_config(&self.ai) {
            Ok(()) => {
                self.ai_saved = self.ai.clone();
                true
            }
            Err(error) => {
                self.error(format!("Configuração de IA não salva: {error}"));
                false
            }
        }
    }

    /// Asks the configured server for its models, in the background.
    fn fetch_models(&mut self, ctx: &egui::Context) {
        if self.model_fetch.is_some() {
            return;
        }
        let settings = match self.ai.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.models_error = Some(error.to_string());
                return;
            }
        };
        let key = (self.ai.provider, settings.base_url.clone());
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        match std::thread::Builder::new()
            .name("ponte-models".into())
            .spawn(move || {
                let result = ai::list_models(&settings).map_err(|e| e.to_string());
                let _ = sender.send(result);
                repaint.request_repaint();
            }) {
            Ok(_) => {
                self.model_fetch = Some(receiver);
                self.models_for = Some(key);
                self.models_error = None;
            }
            Err(error) => {
                self.models_error = Some(format!("Não foi possível consultar os modelos: {error}"))
            }
        }
    }

    fn poll_models(&mut self) {
        let received = self.model_fetch.as_ref().map(|r| r.try_recv());
        match received {
            Some(Ok(Ok(models))) => {
                self.model_fetch = None;
                if models.is_empty() {
                    self.models_error = Some(match self.ai.provider {
                        AiProvider::Local => "O Ollama deste computador não tem modelos instalados. No terminal, execute: ollama pull qwen3:4b-instruct".into(),
                        _ => "O servidor não ofereceu nenhum modelo.".into(),
                    });
                }
                // Keep a sensible model selected: the current one when offered,
                // otherwise the first of the list.
                let current = self.ai.model().trim().to_owned();
                if !models.iter().any(|m| m.name == current) {
                    if let Some(first) = models.first() {
                        *self.ai.model_mut() = first.name.clone();
                    }
                }
                self.models = models;
            }
            Some(Ok(Err(error))) => {
                self.model_fetch = None;
                self.models.clear();
                self.models_error = Some(error);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.model_fetch = None;
                self.models_error = Some("A consulta de modelos terminou sem resposta.".into());
            }
            _ => {}
        }
    }

    fn spawn(
        &mut self,
        label: String,
        task: impl FnOnce() -> WorkResult + Send + 'static,
        ctx: &egui::Context,
    ) {
        if self.worker.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        match std::thread::Builder::new()
            .name("ponte-worker".into())
            .spawn(move || {
                let result = task();
                let _ = sender.send(result);
                repaint.request_repaint();
            }) {
            Ok(_) => {
                self.worker = Some(Worker {
                    receiver,
                    label,
                    started: Instant::now(),
                })
            }
            Err(error) => self.error(format!("Não foi possível iniciar a operação: {error}")),
        }
    }

    fn prepare_sources(&mut self, document_id: i64, documents: Vec<i64>, ctx: &egui::Context) {
        let path = self.db_path.clone();
        self.spawn(
            "Preparando fontes localmente".into(),
            move || WorkResult::Sources {
                document_id,
                result: Store::open(&path)
                    .and_then(|store| store.prepare_sources(&documents))
                    .map_err(|e| e.to_string()),
            },
            ctx,
        );
    }
    fn analyze(&mut self, detail: &DocumentDetail, ctx: &egui::Context) {
        let sources = match self.sources.selected(detail.summary.id, &self.store) {
            Ok(value) => value,
            Err(e) => {
                self.error(e.to_string());
                return;
            }
        };
        // Pending edits are saved first: the analysis records the context it
        // used, so it must match what is stored.
        if !self.save_pending_inputs(detail.summary.id) {
            return;
        }
        let note = self.editors[&detail.summary.id].note.clone();
        let human_context = match self.store.load_work_context(detail.summary.id) {
            Ok(value) => value.analysis_text(&note),
            Err(e) => {
                self.error(format!("Falha ao ler contexto: {e}"));
                return;
            }
        };
        let settings = match self.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.error(error.to_string());
                self.open_ai_settings(ctx);
                return;
            }
        };
        let document_id = detail.summary.id;
        let capture_id = detail.summary.latest_capture_id;
        let capture = detail.capture.clone();
        self.spawn(
            format!("Analisando {}", detail.summary.document_number),
            move || WorkResult::Analysis {
                document_id,
                result: ai::analyze_with_sources(
                    &settings,
                    &capture,
                    capture_id,
                    &human_context,
                    &sources,
                )
                .map_err(|e| e.to_string()),
            },
            ctx,
        );
    }

    /// Saves unsaved context and notes of this document before an AI action.
    /// Returns false (with the error shown) when something could not be saved.
    fn save_pending_inputs(&mut self, id: i64) -> bool {
        let mut saved = Vec::new();
        match self.work_context.save_pending(&mut self.store, id) {
            Ok(true) => saved.push("contexto"),
            Ok(false) => {}
            Err(error) => {
                self.error(format!("Não foi possível salvar o contexto: {error}"));
                return false;
            }
        }
        if self.editors.get(&id).is_some_and(Editor::note_dirty) {
            self.save_note(id);
            if self.editors.get(&id).is_some_and(Editor::note_dirty) {
                // save_note already showed the error.
                return false;
            }
            saved.push("notas");
        }
        if !saved.is_empty() {
            self.message(format!("{} salvo(s) antes de acionar a IA.", saved.join(" e ")));
            self.reload(id);
        }
        true
    }

    fn generate(&mut self, detail: &DocumentDetail, ctx: &egui::Context) {
        if !self.save_pending_inputs(detail.summary.id) {
            return;
        }
        let sources = match self.sources.selected(detail.summary.id, &self.store) {
            Ok(value) => value,
            Err(e) => {
                self.error(e.to_string());
                return;
            }
        };
        let settings = match self.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.error(error.to_string());
                self.open_ai_settings(ctx);
                return;
            }
        };
        let document_id = detail.summary.id;
        let editor = &self.editors[&document_id];
        let local_context = match self.store.load_work_context(document_id) {
            Ok(value) => value.analysis_text(&editor.note),
            Err(e) => {
                self.error(format!("Falha ao ler contexto: {e}"));
                return;
            }
        };
        let context = format!("Contexto local e notas do usuário (conferir na fonte):\n{}\n\nOrientação para esta minuta:\n{}", local_context, editor.instructions);
        let kind = editor.kind.clone();
        let capture = detail.capture.clone();
        let capture_id = detail.summary.latest_capture_id;
        let number = detail.summary.document_number.clone();
        self.spawn(
            format!("Gerando minuta de {}", number),
            move || {
                let result =
                    ai::generate_reply_with_sources(&settings, &capture, &kind, &context, &sources)
                        .map_err(|e| e.to_string());
                WorkResult::Draft {
                    materials: sources,
                    document_id,
                    number,
                    capture_id,
                    kind,
                    result,
                }
            },
            ctx,
        );
    }

    fn backup(&mut self, ctx: &egui::Context) {
        let db_path = self.db_path.clone();
        self.spawn(
            "Criando backup protegido".into(),
            move || {
                let result = (|| -> Result<PathBuf> {
                    let directory = crate::paths::data_dir()?.join("backups");
                    std::fs::create_dir_all(&directory)?;
                    let path = directory.join(format!(
                        "ponte-{}-{}.sqlite3",
                        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
                        uuid::Uuid::new_v4()
                    ));
                    Store::open(&db_path)?.backup(&path)?;
                    Ok(path)
                })();
                WorkResult::Backup(result.map_err(|e| e.to_string()))
            },
            ctx,
        );
    }

    fn poll_worker(&mut self) {
        let received = self
            .worker
            .as_ref()
            .map(|worker| worker.receiver.try_recv());
        let result = match received {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => {
                self.worker = None;
                self.error("A operação terminou sem resultado. Você pode tentar novamente.");
                return;
            }
            _ => return,
        };
        self.worker = None;
        match result {
            WorkResult::Sources {
                document_id,
                result,
            } => {
                self.sources.accept(document_id, result);
            }
            WorkResult::Analysis {
                document_id,
                result,
            } => match result {
                Ok(analysis) => match self.store.save_analysis(document_id, &analysis) {
                    Ok(()) => {
                        self.message("Análise salva e vinculada à captura utilizada. Confira as conclusões na fonte.");
                        self.refresh();
                    }
                    Err(error) => self.error(format!(
                        "A análise foi obtida, mas não pôde ser salva: {error}"
                    )),
                },
                Err(error) => self.error(format!("Análise não concluída: {error}")),
            },
            WorkResult::Draft {
                materials,
                document_id,
                number,
                capture_id,
                kind,
                result,
            } => match result {
                Ok(body) => {
                    self.suggestion = Some(Suggestion {
                        materials,
                        document_id,
                        number,
                        capture_id,
                        kind,
                        body,
                    });
                    self.suggestion_open = true;
                    self.message("Sugestão pronta para revisão. Salve uma versão quando o texto estiver adequado.");
                }
                Err(error) => self.error(format!("Minuta não gerada: {error}")),
            },
            WorkResult::Backup(result) => match result {
                Ok(path) => {
                    self.message(format!("Backup protegido concluído: {}", path.display()));
                    self.backup_path = Some(path);
                }
                Err(error) => self.error(format!("Backup não concluído: {error}")),
            },
        }
    }

    fn replace_editor(&mut self, replacement: Replacement) {
        if self
            .editors
            .get(&replacement.document_id)
            .is_some_and(Editor::draft_dirty)
        {
            self.replacement = Some(replacement);
        } else {
            self.apply_replacement(replacement);
        }
    }

    fn apply_replacement(&mut self, replacement: Replacement) {
        let id = replacement.document_id;
        if replacement.from_suggestion {
            self.suggestion = None;
            self.suggestion_open = false;
        }
        if self.selected_id != Some(id) {
            self.select(id);
        }
        if let Some(editor) = self.editors.get_mut(&id) {
            editor.materials = replacement.materials;
            editor.body = replacement.body;
            editor.kind = replacement.kind;
            editor.capture_id = replacement.capture_id;
            editor.origin = replacement.origin;
            if replacement.saved {
                editor.mark_draft_saved();
            } else {
                // An explicit save creates a new version for templates and AI suggestions.
                editor.saved_body.clear();
            }
            self.tab = Tab::Notes;
        }
    }

    fn save_note(&mut self, id: i64) {
        let Some(editor) = self.editors.get(&id) else {
            return;
        };
        let note = editor.note.clone();
        let expected_version = editor.note_version;
        match self.store.save_note_checked(id, &note, expected_version) {
            Ok(()) => {
                let new_version = self
                    .store
                    .item_version("note", id)
                    .ok()
                    .map(|meta| meta.version);
                let editor = self.editors.get_mut(&id).expect("editor exists");
                editor.saved_note = editor.note.clone();
                editor.note_version = new_version;
                self.message("Notas salvas.");
                self.reload(id);
            }
            Err(error) => self.error(format!("Notas não salvas: {error}")),
        }
    }

    fn save_draft(&mut self, id: i64) {
        let Some(editor) = self.editors.get(&id) else {
            return;
        };
        if editor.body.trim().is_empty() {
            self.error("Escreva a minuta antes de salvar.");
            return;
        }
        match self.store.save_draft_with_sources(
            id,
            editor.capture_id,
            &editor.kind,
            &editor.body,
            &editor.materials,
        ) {
            Ok(version) => {
                let editor = self.editors.get_mut(&id).expect("editor exists");
                editor.mark_draft_saved();
                editor.origin = format!("Versão salva #{version}");
                self.message(format!(
                    "Versão #{version} salva. O histórico anterior foi preservado."
                ));
                self.reload(id);
            }
            Err(error) => self.error(format!("Minuta não salva: {error}")),
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("PONTE").color(ACCENT).strong());
            ui.label("Assistente documental");
            if ui
                .selectable_label(self.saiop_open, "Processos e demandas")
                .clicked()
            {
                self.saiop_open = true;
                self.tasks_open = false;
            }
            if ui
                .selectable_label(self.tasks_open && !self.saiop_open, "Tarefas")
                .clicked()
            {
                self.tasks_open = true;
                self.saiop_open = false;
            }
            if ui
                .selectable_label(!self.tasks_open && !self.saiop_open, "Documento")
                .clicked()
            {
                self.tasks_open = false;
                self.saiop_open = false;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut backup = false;
                ui.menu_button("Menu ▾", |ui| {
                    ui.set_min_width(230.0);
                    if ui.button("Rede e equipe…").clicked() {
                        self.intranet.open = true;
                        ui.close();
                    }
                    if ui.button("Configurar IA…").clicked() {
                        self.open_ai_settings(ui.ctx());
                        ui.close();
                    }
                    if ui.button("Lixeira…").clicked() {
                        self.trash.open = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(self.worker.is_none(), egui::Button::new("Fazer backup agora"))
                        .on_hover_text("Cópia protegida do acervo na pasta de backups deste computador.")
                        .clicked()
                    {
                        backup = true;
                        ui.close();
                    }
                    if ui
                        .button("Atualizar lista")
                        .on_hover_text("A lista já se atualiza sozinha a cada poucos segundos.")
                        .clicked()
                    {
                        self.refresh();
                        ui.close();
                    }
                    ui.separator();
                    ui.small(format!("Ponte {}", env!("CARGO_PKG_VERSION")));
                });
                if backup {
                    self.backup(ui.ctx());
                }
                let (color, short, full) = self.network_status();
                let response = ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("● {short}")).color(color).size(13.0),
                        )
                        .frame(false),
                    )
                    .on_hover_text(full);
                if response.clicked() {
                    self.intranet.open = true;
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Documentos");
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Buscar número, assunto ou texto…")
                    .desired_width(f32::INFINITY),
            )
            .accessible_name("Buscar documentos na biblioteca");
        if response.changed() {
            self.search_changed = Some(Instant::now());
        }

        let all_tasks = self.store.list_tasks().unwrap_or_default();
        let today_iso = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut summaries = BTreeMap::new();
        let mut count_open_any = 0usize;
        let mut count_on_time = 0usize;
        let mut count_overdue = 0usize;
        let mut count_completed = 0usize;

        for doc in &self.documents {
            let s = DocTaskStatusSummary::from_tasks(&all_tasks, doc.id, &today_iso);
            if s.matches(DocTaskFilter::OpenAny) {
                count_open_any += 1;
            }
            if s.matches(DocTaskFilter::OpenOnTime) {
                count_on_time += 1;
            }
            if s.matches(DocTaskFilter::Overdue) {
                count_overdue += 1;
            }
            if s.matches(DocTaskFilter::Completed) {
                count_completed += 1;
            }
            summaries.insert(doc.id, s);
        }

        let filters_active = self.doc_task_filter != DocTaskFilter::All
            || self.doc_priority_filter != DocPriorityFilter::All;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let label = if self.filters_open { "Filtrar ▴" } else { "Filtrar ▾" };
            if ui
                .add_enabled(!self.documents.is_empty() || filters_active, egui::Button::new(label).small())
                .clicked()
            {
                self.filters_open = !self.filters_open;
            }
            if count_overdue > 0 && !self.filters_open {
                if ui
                    .add(egui::Button::new(RichText::new(format!("⚠ {count_overdue} atrasado(s)")).color(ERROR).size(12.0)).small())
                    .on_hover_text("Mostrar só documentos com tarefa atrasada")
                    .clicked()
                {
                    self.doc_task_filter = DocTaskFilter::Overdue;
                    self.filters_open = true;
                }
            }
            if (filters_active || !self.query.is_empty()) && ui.small_button("Limpar").clicked() {
                self.doc_task_filter = DocTaskFilter::All;
                self.doc_priority_filter = DocPriorityFilter::All;
                if !self.query.is_empty() {
                    self.query.clear();
                    self.search_changed = Some(Instant::now());
                }
            }
        });
        if self.filters_open || filters_active {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                ui.selectable_value(
                    &mut self.doc_task_filter,
                    DocTaskFilter::All,
                    format!("Todos ({})", self.documents.len()),
                );
                ui.selectable_value(
                    &mut self.doc_task_filter,
                    DocTaskFilter::OpenAny,
                    format!("Com tarefa aberta ({count_open_any})"),
                );
                ui.selectable_value(
                    &mut self.doc_task_filter,
                    DocTaskFilter::OpenOnTime,
                    RichText::new(format!("● No prazo ({count_on_time})")).color(ACCENT),
                );
                ui.selectable_value(
                    &mut self.doc_task_filter,
                    DocTaskFilter::Overdue,
                    RichText::new(format!("⚠ Atrasados ({count_overdue})")).color(ERROR),
                );
                ui.selectable_value(
                    &mut self.doc_task_filter,
                    DocTaskFilter::Completed,
                    RichText::new(format!("✓ Concluídos ({count_completed})")).color(SUCCESS),
                );
            });
            egui::ComboBox::from_id_salt("doc_priority_filter_combo")
                .selected_text(self.doc_priority_filter.label())
                .width(175.0)
                .show_ui(ui, |ui| {
                    for pf in [
                        DocPriorityFilter::All,
                        DocPriorityFilter::Urgent,
                        DocPriorityFilter::High,
                        DocPriorityFilter::Normal,
                        DocPriorityFilter::Low,
                        DocPriorityFilter::Unclassified,
                    ] {
                        ui.selectable_value(&mut self.doc_priority_filter, pf, pf.label());
                    }
                });
        }

        let filtered_docs: Vec<&DocumentSummary> = self
            .documents
            .iter()
            .filter(|doc| {
                let task_ok = summaries
                    .get(&doc.id)
                    .is_none_or(|s| s.matches(self.doc_task_filter));
                let prio_ok = self.doc_priority_filter.matches(&doc.priority);
                task_ok && prio_ok
            })
            .collect();

        if filtered_docs.len() != self.documents.len() {
            ui.small(format!(
                "{} de {} documento(s)",
                filtered_docs.len(),
                self.documents.len()
            ));
        }
        ui.separator();
        let mut selected = None;
        egui::ScrollArea::vertical().id_salt("document-list").show(ui, |ui| {
            for (document, depth) in group_joined(&filtered_docs) {
                ui.push_id(document.id, |ui| {
                    let joined = joined_label(document);
                    // Hierarchical indent: deeper under a visible parent,
                    // lighter when the parent is not in the list/library.
                    let indent = if depth > 0 {
                        18.0 * depth.min(4) as f32
                    } else if joined.is_some() {
                        12.0
                    } else {
                        0.0
                    };
                    let editing = self.editors.get(&document.id).is_some_and(Editor::dirty);
                    let number = if editing {
                        format!("{} • edição", document.document_number)
                    } else {
                        document.document_number.clone()
                    };
                    let summary = summaries.get(&document.id).cloned().unwrap_or_default();
                    let stripe = summary.stripe_color();
                    let (badge_text, badge_fg, badge_bg) = summary.badge();

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if indent > 0.0 {
                            ui.add_space(indent - 6.0);
                            ui.label(RichText::new("↳").size(15.0).color(JOINED_FG))
                                .on_hover_text(joined.clone().unwrap_or_default());
                        }
                        // Barra vertical sinalizadora (vermelho = atrasada, âmbar/azul = no prazo, verde = concluída).
                        // Painted after the card so it matches the card's height.
                        let stripe_left = ui.cursor().min;
                        // The stripe's own width plus the usual gap before the card.
                        ui.add_space(5.0 + ui.spacing().item_spacing.x);

                        // The title wraps instead of being cut; the card grows with it.
                        let label = format!("{}\n{}", number, document.title);
                        let response = ui.add(
                            egui::Button::new(RichText::new(label).size(14.0))
                                .wrap()
                                .min_size(egui::vec2(ui.available_width(), 60.0))
                                .selected(self.selected_id == Some(document.id)),
                        );
                        if response.clicked() {
                            selected = Some(document.id);
                        }
                        let stripe_rect = egui::Rect::from_min_size(
                            stripe_left,
                            egui::vec2(5.0, response.rect.height()),
                        );
                        ui.painter().rect_filled(stripe_rect, 2.5, stripe);
                    });

                    ui.horizontal_wrapped(|ui| {
                        if indent > 0.0 {
                            ui.add_space(indent + 11.0);
                        }
                        pill(ui, badge_text, badge_fg, badge_bg, 11.5);
                        if let Some(text) = &joined {
                            pill(ui, text, JOINED_FG, JOINED_BG, 11.5);
                        }
                        if document.joined_children_count > 0 {
                            pill(
                                ui,
                                format!("📎 {} juntado(s)", document.joined_children_count),
                                JOINED_FG,
                                JOINED_BG,
                                11.5,
                            );
                        }
                        if matches!(document.priority.as_str(), "alta" | "urgente") {
                            ui.small(crate::labels::priority(&document.priority));
                        }
                        if document.analysis_stale {
                            ui.small("análise desatualizada").on_hover_text("O documento, o contexto ou as notas mudaram depois da última análise.");
                        }
                    });
                    ui.add_space(6.0);
                });
            }
            if filtered_docs.is_empty() {
                if self.documents.is_empty() {
                    ui.label(if self.query.is_empty() {
                        "Nenhum documento importado ainda."
                    } else {
                        "Nenhum documento encontrado."
                    });
                    if self.query.is_empty() {
                        ui.add_space(8.0);
                        ui.small("Abra um documento no SIGADOC e clique em “Importar para o assistente”.");
                    }
                } else {
                    ui.label("Nenhum documento corresponde à busca ou aos filtros.");
                    if ui.button("Limpar busca e filtros").clicked() {
                        self.doc_task_filter = DocTaskFilter::All;
                        self.doc_priority_filter = DocPriorityFilter::All;
                        if !self.query.is_empty() {
                            self.query.clear();
                            self.search_changed = Some(Instant::now());
                        }
                    }
                }
            }
        });
        if let Some(id) = selected {
            self.select(id);
        }
    }

    fn document(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        let doc_tasks = self.store.list_tasks().unwrap_or_default();
        let today_iso = chrono::Local::now().format("%Y-%m-%d").to_string();
        let task_summary = DocTaskStatusSummary::from_tasks(&doc_tasks, id, &today_iso);
        let (badge_text, badge_fg, badge_bg) = task_summary.badge();

        ui.horizontal_wrapped(|ui| {
            ui.heading(&detail.summary.document_number);
            egui::Frame::new()
                .fill(badge_bg)
                .corner_radius(5.0)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(RichText::new(badge_text).size(12.5).color(badge_fg).strong());
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(self.worker.is_none(), egui::Button::new("Excluir…"))
                    .clicked()
                {
                    self.request_trash(crate::trash::Target {
                        kind: crate::trash::Kind::Document,
                        id,
                    });
                }
            });
        });
        ui.add(egui::Label::new(RichText::new(&detail.summary.title).size(18.0)).wrap());
        let mut open_joined = None;
        if let Some(text) = joined_label(&detail.summary) {
            ui.horizontal_wrapped(|ui| {
                let text = match &detail.summary.joined_to_number {
                    Some(number) if detail.summary.joined_to_id.is_some() => {
                        format!("↳ Juntado ao documento {number}")
                    }
                    _ => text,
                };
                pill(ui, text, JOINED_FG, JOINED_BG, 12.5);
                match detail.summary.joined_to_id {
                    Some(parent) => {
                        if ui
                            .button("Abrir documento principal")
                            .on_hover_text("Abre a ficha do documento ao qual este foi juntado")
                            .clicked()
                        {
                            open_joined = Some(parent);
                        }
                    }
                    None if detail.summary.joined_to_number.is_some() => {
                        ui.small("Documento principal ainda não importado; importe-o pela extensão para vincular automaticamente.");
                    }
                    None => {
                        ui.small("A via indica juntada, mas o número do documento principal não aparece nesta captura.");
                    }
                }
            });
        }
        if !self.joined_children.is_empty() {
            ui.horizontal_wrapped(|ui| {
                pill(
                    ui,
                    format!("📎 {} documento(s) juntado(s)", self.joined_children.len()),
                    JOINED_FG,
                    JOINED_BG,
                    12.5,
                );
                for child in &self.joined_children {
                    if ui
                        .small_button(format!("↳ {}", child.document_number))
                        .on_hover_text(format!("Abrir {} — {}", child.document_number, child.title))
                        .clicked()
                    {
                        open_joined = Some(child.id);
                    }
                }
            });
        }
        if let Some(target) = open_joined {
            // Opening another document starts a fresh frame for that ficha.
            self.select(target);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(format!(
                "Captura #{} • {}",
                detail.summary.latest_capture_id,
                display_date(&detail.capture.captured_at)
            ));
            safe_link(ui, "Abrir documento no SIGADOC", &detail.capture.source_url);
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Overview, "Resumo");
            ui.selectable_value(&mut self.tab, Tab::Document, "Documento");
            let editor = &self.editors[&id];
            let notes_label = if editor.note_dirty() || editor.draft_dirty() {
                RichText::new("Anotações e minutas ●").color(WARNING)
            } else {
                RichText::new("Anotações e minutas")
            };
            ui.selectable_value(&mut self.tab, Tab::Notes, notes_label);
            ui.selectable_value(&mut self.tab, Tab::More, "Mais");
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt(("document-content", id, self.tab as u8)).show(ui, |ui| {
            match self.tab {
                Tab::Overview => self.overview(ui, detail),
                Tab::Document => self.document_tab(ui, detail),
                Tab::Notes => {
                    self.notes(ui, detail);
                    ui.add_space(14.0);
                    ui.separator();
                    self.drafts(ui, detail);
                }
                Tab::More => self.more_tab(ui, detail),
            }
        });
    }

    fn section<R>(
        ui: &mut egui::Ui,
        id: i64,
        title: &str,
        default_open: bool,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<R> {
        egui::CollapsingHeader::new(RichText::new(title).size(15.0).strong())
            .id_salt(("doc-section", id, title))
            .default_open(default_open)
            .show(ui, add)
            .body_returned
    }

    /// Everything imported from the SIGADOC page, in reading order.
    fn document_tab(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        Self::section(ui, id, "Texto importado", true, |ui| {
            ui.horizontal(|ui| {
                ui.small("Texto da página e dos PDFs lidos.");
                info(ui, "Parágrafos, listas e tabelas mantêm a organização da página. Para a aparência original, abra o PDF na seção Arquivos.");
            });
            if detail.capture.text.is_empty() {
                ui.colored_label(WARNING, "Esta importação não trouxe texto. Abra o conteúdo no SIGADOC e importe de novo.");
            } else {
                let mut text = detail.capture.text.as_str();
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(24).code_editor());
                });
            }
        });
        let has_files = self
            .store
            .list_files(detail.summary.latest_capture_id)
            .map(|f| !f.is_empty())
            .unwrap_or(false);
        Self::section(ui, id, "Arquivos (PDF)", has_files, |ui| self.files_list(ui, detail));
        Self::section(ui, id, "Dados do SIGADOC", false, |ui| {
            crate::work_context::provenance(ui, detail);
        });
        Self::section(ui, id, "Histórico de tramitação", false, |ui| {
            crate::dossier::show_history(ui, &detail.capture, id);
        });
    }

    /// Local relations, versions and technical details.
    fn more_tab(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        Self::section(ui, id, "Vínculos locais e versões importadas", true, |ui| {
            if let Some(other) = self.dossier.show(ui, &mut self.store, id) {
                self.select(other);
            }
            if let Some(target) = self.dossier.take_trash_request() {
                self.request_trash(target);
            }
        });
        Self::section(ui, id, "O que foi importado e referências", false, |ui| {
            self.coverage_and_references(ui, detail);
        });
        Self::section(ui, id, "Dados técnicos", false, |ui| {
            self.technical_data(ui, detail);
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        self.document_summary(ui, detail);
        self.document_tasks(ui, detail);
        self.saiop_links(ui, detail);
        ui.separator();
        self.analysis_section(ui, detail);
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Antes de usar a IA");
            info(ui, "O contexto e as fontes abaixo são opcionais. Quando preenchidos, a análise e a minuta levam em conta o que você informou.");
        });
        if self
            .work_context
            .show(ui, &mut self.store, detail.summary.id)
        {
            self.reload(detail.summary.id);
        }
        if let Some(documents) =
            self.sources
                .show(ui, &self.store, detail.summary.id, self.worker.is_some())
        {
            self.prepare_sources(detail.summary.id, documents, ui.ctx());
        }
        ui.separator();
        ui.collapsing("Documentos parecidos na biblioteca", |ui| {
            ui.horizontal(|ui| {
                ui.small("Semelhança por palavras em comum.");
                info(ui, "Não indica vínculo oficial entre os documentos.");
            });
            let mut selected = None;
            for similar in &self.similar {
                if ui
                    .link(format!("{} — {}", similar.document_number, similar.title))
                    .clicked()
                {
                    selected = Some(similar.id);
                }
            }
            if self.similar.is_empty() {
                ui.label("Nenhum documento parecido encontrado.");
            }
            if let Some(id) = selected {
                self.select(id);
            }
        });
    }

    /// Attendances of the cabinet control that point at this document.
    fn saiop_links(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let Ok(all) = self.store.list_saiop_attendances() else {
            return;
        };
        let mine: Vec<_> = all
            .into_iter()
            .filter(|a| a.document_id == Some(detail.summary.id))
            .collect();
        if mine.is_empty() {
            return;
        }
        let today = chrono::Local::now().date_naive();
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(format!(
                "Processos e demandas • {} atendimento(s) deste documento",
                mine.len()
            ));
            for att in mine {
                ui.horizontal_wrapped(|ui| {
                    ui.label(if att.reference_raw.is_empty() {
                        att.subject.clone()
                    } else {
                        att.reference_raw.clone()
                    });
                    ui.label(crate::saiop::status_label(&att.status));
                    if !att.assignee.is_empty() {
                        ui.small(format!("resp.: {}", att.assignee));
                    }
                    if let Some(delta) = att.due_delta_days(today) {
                        if delta > 0 {
                            ui.colored_label(ERROR, format!("vencido há {delta} d"));
                        } else if delta == 0 {
                            ui.colored_label(WARNING, "vence hoje");
                        } else {
                            ui.small(format!("prazo {}", crate::labels::date(&att.due_date)));
                        }
                    }
                    if ui.small_button("Abrir").clicked() {
                        if self.saiop.focus_attendance(&self.store, att.id) {
                            self.saiop_open = true;
                            self.tasks_open = false;
                        } else {
                            self.error("Há um atendimento com alterações não salvas em Processos e demandas. Salve ou descarte antes de abrir outro.");
                        }
                    }
                });
            }
        });
    }

    fn analysis_section(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Análise assistida por IA");
            let busy = self.worker.is_some();
            let label = if busy && self.worker.as_ref().is_some_and(|w| w.label.starts_with("Analisando")) {
                format!("Analisando… {} s", self.worker.as_ref().map(|w| w.started.elapsed().as_secs()).unwrap_or(0))
            } else {
                "Analisar e sugerir providências".to_owned()
            };
            if ui
                .add_enabled(!busy, egui::Button::new(label))
                .on_hover_text(self.ai_disclosure_text(false))
                .clicked()
            {
                self.analyze(detail, ui.ctx());
            }
            self.ai_destination(ui);
        });
        if let Some(analysis) = &detail.analysis {
            let stale = detail.summary.analysis_stale
                || self.work_context.document_dirty(detail.summary.id)
                || self.editors[&detail.summary.id].note_dirty();
            if stale {
                ui.colored_label(WARNING, format!("Análise da captura #{}: a captura, o contexto ou as notas mudaram. Salve as edições e analise novamente.", analysis.capture_id));
            }
            ui.label(&analysis.summary);
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!(
                    "Prioridade sugerida: {}",
                    crate::labels::priority(&analysis.priority)
                ));
                if !analysis.subjects.is_empty() {
                    ui.label(analysis.subjects.join(" • "));
                }
            });
            ui.label(format!("Motivo: {}", analysis.reason));
            if !analysis.suggested_actions.is_empty() {
                ui.strong("Providências sugeridas — revisão necessária");
                for suggestion in &analysis.suggested_actions {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.strong(&suggestion.action);
                        ui.label(&suggestion.reason);
                        ui.label(format!("Base indicada: {}", suggestion.evidence));
                        if ui
                            .add_enabled(
                                !stale,
                                egui::Button::new(format!(
                                    "Preparar tarefa: {}",
                                    suggestion.action
                                )),
                            )
                            .clicked()
                        {
                            if self.tasks.prepare_suggestion(
                                detail.summary.id,
                                suggestion,
                                &analysis.priority,
                            ) {
                                self.tasks_open = true;
                            } else {
                                self.error(
                                    "Salve ou cancele a tarefa em edição antes de preparar outra.",
                                );
                            }
                        }
                    });
                }
            }
            if !analysis.citations.is_empty() {
                ui.strong("Trechos conferidos nas fontes selecionadas");
                ui.small("O programa confere a presença literal dos trechos; a interpretação da IA ainda exige revisão.");
                for citation in &analysis.citations {
                    if let Some(source) = analysis
                        .materials
                        .iter()
                        .find(|s| s.id == citation.source_id)
                    {
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.label(&citation.claim);
                            ui.strong(&source.label);
                            ui.label(format!("Trecho: {}", citation.quote));
                            ui.small(format!(
                                "Fonte {} • captura #{} • método {}",
                                source.id, source.capture_id, source.method
                            ));
                            ui.collapsing("Texto da fonte utilizada", |ui| {
                                let mut text = source.text.as_str();
                                ui.add(
                                    egui::TextEdit::multiline(&mut text)
                                        .desired_rows(6)
                                        .desired_width(f32::INFINITY),
                                );
                            });
                        });
                    }
                }
            }
            if !analysis.missing_context.is_empty() {
                ui.strong("Informações a conferir");
                for missing in &analysis.missing_context {
                    ui.label(format!("• {missing}"));
                }
            }
            if !analysis.human_context.is_empty() {
                ui.collapsing("Contexto salvo utilizado nesta análise", |ui| {
                    crate::work_context::show_analysis_context(ui, &analysis.human_context);
                });
            }
            if let Some(deadline) = &analysis.deadline_candidate {
                ui.colored_label(
                    WARNING,
                    format!("Prazo candidato: {deadline}. Confirme a data e a regra na fonte."),
                );
            }
            ui.small(format!(
                "Modelo {} • {}",
                analysis.model,
                display_date(&analysis.created_at)
            ));
        } else {
            ui.small("Ainda sem análise. Importar, buscar e anotar funcionam sem IA.");
        }
    }

    fn files_list(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        match self.store.list_files(detail.summary.latest_capture_id) {
            Ok(files) if files.is_empty() => {
                ui.label("Nenhum PDF veio com esta importação.");
                ui.small("Se o documento tem PDF no SIGADOC, importe de novo pela extensão para tentar baixá-lo.");
            }
            Ok(files) => {
                ui.horizontal_wrapped(|ui| {
                    info(ui, "“Ler no Ponte” abre a cópia local, sem precisar do SIGADOC. “Abrir no SIGADOC” vai à versão oficial no navegador (exige login). “Exportar” cria uma cópia legível na pasta local \"exportados\".");
                    ui.small("Cópias baixadas na importação, guardadas protegidas no acervo.");
                });
                for file in files {
                    let origin = detail
                        .capture
                        .references
                        .iter()
                        .find(|r| r.url == file.url);
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(&file.label);
                            if let Some(reference) = origin {
                                ui.small(crate::labels::downloaded_kind(&reference.kind));
                            }
                            ui.small(format!("{} KB", file.size.div_ceil(1024)));
                            ui.small(&file.text_status);
                        });
                        ui.horizontal_wrapped(|ui| {
                            if let Some(reference) = origin {
                                safe_link(ui, "Abrir no SIGADOC", &reference.url);
                            }
                            if ui.button("Ler no Ponte").clicked() {
                                self.pdf_reader.open(
                                    &self.store,
                                    file.id,
                                    detail.summary.latest_capture_id,
                                    &file.label,
                                    ui.ctx(),
                                );
                            }
                            if ui.button("Exportar e abrir pasta").clicked() {
                                match self
                                    .store
                                    .export_pdf(file.id, detail.summary.latest_capture_id)
                                {
                                    Ok(path) => {
                                        self.message(format!("PDF exportado: {}", path.display()));
                                        if let Some(parent) = path.parent() {
                                            crate::windows_ui::reveal_in_explorer(parent);
                                        }
                                    }
                                    Err(e) => self.error(format!("PDF não exportado: {e}")),
                                }
                            }
                            ui.small(format!("identificação {}…", &file.sha256[..file.sha256.len().min(12)]))
                                .on_hover_text(format!("SHA-256 completo: {}", file.sha256));
                        });
                    });
                }
            }
            Err(e) => {
                ui.colored_label(WARNING, format!("Falha ao ler arquivos: {e}"));
            }
        }
    }

    fn coverage_and_references(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        ui.label(format!(
            "{} caracteres de texto • {} referência(s)",
            detail.capture.text.chars().count(),
            detail.capture.references.len()
        ));
        if detail.capture.coverage.recognized.is_empty() {
            ui.colored_label(WARNING, "Nenhuma parte da página foi marcada como reconhecida.");
        } else {
            ui.label("Reconhecido na página:");
            for item in &detail.capture.coverage.recognized {
                ui.label(format!("• {item}"));
            }
        }
        if !detail.capture.coverage.pending.is_empty() {
            ui.colored_label(WARNING, "Observações da importação:");
            for pending in &detail.capture.coverage.pending {
                ui.label(format!("• {pending}"));
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.strong("Links encontrados na página");
            info(ui, "Abrem o SIGADOC no navegador e exigem login. Os PDFs que foram baixados na importação têm a cópia local em Documento ▸ Arquivos.");
        });
        if detail.capture.references.is_empty() {
            ui.small("Nenhum link foi identificado nesta página.");
        }
        // Only consult the stored files when there is a link to match them to.
        let downloaded = if detail.capture.references.is_empty() {
            Vec::new()
        } else {
            self.store
                .list_files(detail.summary.latest_capture_id)
                .unwrap_or_default()
        };
        let mut open_file = None;
        for (index, reference) in detail.capture.references.iter().enumerate() {
            ui.push_id(("reference", index), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.small(reference_kind(&reference.kind));
                    safe_link(ui, &reference.label, &reference.url);
                    if let Some(file) = downloaded.iter().find(|f| f.url == reference.url) {
                        if ui
                            .small_button("cópia baixada: ler no Ponte")
                            .on_hover_text("Este PDF foi baixado na importação; abre a cópia local no leitor do Ponte.")
                            .clicked()
                        {
                            open_file = Some((file.id, file.label.clone()));
                        }
                    }
                });
            });
        }
        if let Some((file_id, label)) = open_file {
            self.pdf_reader.open(
                &self.store,
                file_id,
                detail.summary.latest_capture_id,
                &label,
                ui.ctx(),
            );
        }
    }

    fn technical_data(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        egui::Grid::new(("technical", detail.summary.id))
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Importação em uso");
                ui.label(format!(
                    "#{} • obtida em {}",
                    detail.summary.latest_capture_id,
                    display_date(&detail.capture.captured_at)
                ));
                ui.end_row();
                ui.label("Página de origem");
                ui.add(egui::Label::new(&detail.capture.source_url).wrap());
                ui.end_row();
                ui.label("Leitor da página");
                ui.label(&detail.capture.coverage.adapter);
                ui.end_row();
                for key in ["movement_source", "downloaded_files_sha256"] {
                    if let Some(value) = detail.capture.metadata.get(key) {
                        ui.label(crate::work_context::metadata_label(key));
                        ui.add(egui::Label::new(value).wrap());
                        ui.end_row();
                    }
                }
            });
        let other: Vec<_> = detail
            .capture
            .metadata
            .iter()
            .filter(|(key, _)| {
                !["movement_history", "movement_source", "downloaded_files_sha256"]
                    .contains(&key.as_str())
            })
            .collect();
        if !other.is_empty() {
            ui.collapsing("Todos os campos lidos da página", |ui| {
                for (key, value) in other {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("{}:", crate::work_context::metadata_label(key)));
                        ui.label(value);
                    });
                }
            });
        }
    }

    fn document_summary(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let metadata = &detail.capture.metadata;
        ui.group(|ui|{
            ui.set_width(ui.available_width());
            ui.horizontal(|ui|{
                ui.strong("Resumo do documento");
                info(ui,"Os dados oficiais são um retrato do momento da importação. Finalidade, assuntos e prioridade locais não alteram o SIGADOC.");
            });
            ui.horizontal_wrapped(|ui|{
                ui.label(format!("Tipo: {}",metadata.get("document_type").map(String::as_str).unwrap_or("não disponível na captura")));
                ui.separator();ui.label(format!("Data documental: {}",metadata.get("document_date").map(String::as_str).unwrap_or("não disponível na captura")));
                if let Some(value)=metadata.get("medium"){ui.separator();ui.label(value);}
            });
            if let Some(value)=metadata.get("document_model"){ui.add(egui::Label::new(format!("Modelo do SIGADOC: {value}")).wrap());}
            if let Some(value)=metadata.get("via_status"){ui.add(egui::Label::new(format!("Situação no SIGADOC: {value}")).wrap());}
            ui.label(format!("Finalidade local salva: {}",crate::work_context::purpose_label(&detail.work_context.purpose)));
            if !detail.work_context.subjects.trim().is_empty(){ui.add(egui::Label::new(format!("Assuntos confirmados: {}",detail.work_context.subjects)).wrap());}
            if !detail.work_context.priority.is_empty(){ui.label(format!("Prioridade documental local: {}",crate::tasks::priority_label(&detail.work_context.priority)));}
            if let Some(value)=metadata.get("classification"){
                let code=value.split(" - ").next().unwrap_or(value);
                ui.collapsing(format!("Classificação documental oficial: {code}"),|ui|{ui.label(value);ui.small("Código do SIGADOC; distinto de assuntos, finalidade e prioridade locais.");});
            }
            match self.store.list_files(detail.summary.latest_capture_id){
                Ok(files)=>{
                    ui.small(format!("{} PDF(s) • {} caracteres de texto • {} referência(s)",files.len(),detail.capture.text.chars().count(),detail.capture.references.len()));
                    if !detail.capture.coverage.pending.is_empty(){
                        ui.horizontal(|ui|{
                            ui.colored_label(WARNING,format!("{} observação(ões) da importação",detail.capture.coverage.pending.len()));
                            if ui.small_button("ver").clicked(){self.tab=Tab::More;}
                        });
                    }
                    let original=files.iter().find(|file|detail.capture.references.iter().any(|r|r.url==file.url&&r.kind=="original"));
                    ui.horizontal_wrapped(|ui|{
                        if let Some(file)=original {if ui.button("Ler PDF (cópia no Ponte)").on_hover_text("PDF do documento baixado na importação, aberto no leitor do Ponte sem precisar do SIGADOC. O mesmo arquivo está em Documento ▸ Arquivos, com a opção de abri-lo no SIGADOC.").clicked(){self.pdf_reader.open(&self.store,file.id,detail.summary.latest_capture_id,&file.label,ui.ctx());}}
                        if ui.button("Completar contexto").on_hover_text("Finalidade, assuntos, prioridade e orientações que só você sabe.").clicked(){self.work_context.reveal(detail.summary.id);}
                    });
                },Err(error)=>{ui.label(format!("Não foi possível conferir os arquivos locais: {error}"));}
            }
        });
    }
    fn document_tasks(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        ui.group(|ui|{
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui|{
                ui.strong("Tarefas deste documento");
                if ui.button("Nova tarefa").clicked(){
                    if self.tasks.start_new(detail.summary.id){self.tasks_open=true;self.saiop_open=false;}
                    else{self.error("Há outra tarefa com alterações não salvas. Salve ou cancele essa edição no painel de tarefas.");}
                }
                if ui.button("Ver todas").clicked(){self.tasks.focus_document(detail.summary.id);self.tasks_open=true;self.saiop_open=false;}
            });
            match self.store.list_tasks(){
                Ok(tasks)=>{
                    let tasks=tasks.into_iter().filter(|task|task.document_id==detail.summary.id).collect::<Vec<_>>();
                    let pending=tasks.iter().filter(|task|task.status!="concluida").count();
                    if tasks.is_empty(){ui.small("Nenhuma tarefa ainda.");}
                    else{ui.small(format!("{} pendente(s) • {} concluída(s)",pending,tasks.len()-pending));}
                    if pending>5{ui.small("As cinco mais urgentes estão abaixo; \"Ver todas\" mostra o restante.");}
                    for task in tasks.into_iter().filter(|task|task.status!="concluida").take(5){
                        ui.horizontal_wrapped(|ui|{
                            ui.label(&task.title);ui.label(crate::tasks::status_label(&task.status));
                            ui.label(format!("Prioridade da tarefa: {}",crate::tasks::priority_label(&task.priority)));
                            if !task.due_date.is_empty(){let date=chrono::NaiveDate::parse_from_str(&task.due_date,"%Y-%m-%d").map(|date|date.format("%d/%m/%Y").to_string()).unwrap_or_else(|_|task.due_date.clone());ui.label(format!("Prazo local: {date}"));}
                            if task.overdue(){ui.colored_label(ERROR,"Atrasada");}
                            if ui.add_enabled(!self.tasks.task_dirty(task.id),egui::Button::new("✓ Concluir").small()).on_hover_text("Marca a tarefa como concluída. Nada é enviado ao SIGADOC.").clicked(){
                                match self.store.complete_task(task.id){Ok(done)=>{self.tasks.forget_task(task.id);let mut text=format!("Tarefa concluída: {}",done.title);for note in self.store.take_cascade_notes(){text.push_str(" • ");text.push_str(&note);}self.message(text);},Err(e)=>self.error(format!("Não foi possível concluir a tarefa: {e}"))}
                            }
                            if ui.button("Editar").clicked(){if self.tasks.focus_task(&task){self.tasks_open=true;}else{self.error("Há outra tarefa com alterações não salvas. Salve ou cancele essa edição no painel de tarefas.");}}
                        });
                    }
                },Err(error)=>{ui.label(format!("Não foi possível ler as tarefas: {error}"));}
            }
        });
    }

    fn notes(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        let mut save = false;
        let mut discard = false;
        {
            let editor = self.editors.get_mut(&id).expect("editor exists");
            ui.horizontal_wrapped(|ui| {
                ui.strong("Notas");
                info(ui, "Suas anotações e correções sobre este documento. Ficam só na biblioteca protegida deste computador (ou do gabinete, se estiver em equipe).");
                save = ui
                    .add_enabled(editor.note_dirty(), egui::Button::new("Salvar notas"))
                    .clicked();
                discard = ui
                    .add_enabled(editor.note_dirty(), egui::Button::new("Desfazer alterações"))
                    .on_hover_text("Volta ao texto salvo pela última vez.")
                    .clicked();
                if editor.note_dirty() {
                    ui.colored_label(WARNING, "● não salvas");
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut editor.note)
                    .desired_width(f32::INFINITY)
                    .desired_rows(12)
                    .hint_text("Registre observações, pontos a verificar e correções…"),
            );
        }
        if save {
            self.save_note(id);
        }
        if discard {
            let editor = self.editors.get_mut(&id).expect("editor exists");
            editor.note = editor.saved_note.clone();
            self.message("Notas restauradas para a última gravação.");
        }
    }

    fn drafts(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let editor = &self.editors[&detail.summary.id];
        let export_capture = if editor.capture_id == detail.summary.latest_capture_id {
            detail.capture.clone()
        } else {
            match self
                .store
                .load_capture(detail.summary.id, editor.capture_id)
            {
                Ok(c) => c,
                Err(e) => {
                    ui.label(format!("Captura da minuta não disponível: {e}"));
                    return;
                }
            }
        };
        if let Some((kind, body, origin)) = self.draft_tools.show(
            ui,
            &mut self.store,
            &detail.capture,
            &export_capture,
            &editor.body,
            &editor.kind,
            &detail.drafts,
        ) {
            self.replace_editor(Replacement {
                materials: Vec::new(),
                document_id: detail.summary.id,
                capture_id: detail.summary.latest_capture_id,
                kind,
                body,
                origin,
                saved: false,
                from_suggestion: false,
            });
        }
        let id = detail.summary.id;
        let mut template = false;
        let mut generate = false;
        let mut save = false;
        let mut loaded = None;
        let mut export = false;
        let mut compare: Option<crate::model::DraftVersion> = None;
        let busy_generating = self
            .worker
            .as_ref()
            .is_some_and(|w| w.label.starts_with("Gerando"));
        let generating_label = self
            .worker
            .as_ref()
            .filter(|w| w.label.starts_with("Gerando"))
            .map(|w| format!("Gerando… {} s", w.started.elapsed().as_secs()));
        let disclosure = self.ai_disclosure_text(true);
        let suggestion_pending = self.suggestion.is_some();
        {
            let editor = self.editors.get_mut(&id).expect("editor exists");
            ui.horizontal_wrapped(|ui| {
                ui.strong("Minuta");
                info(ui, "Texto editável para revisão. Confira fatos, destinatário e fundamentos antes de usar no SIGADOC. Nada é enviado ao SIGADOC por aqui.");
                ui.label("Tipo:");
                egui::ComboBox::from_id_salt("draft-kind")
                    .selected_text(kind_label(&editor.kind).to_owned())
                    .show_ui(ui, |ui| {
                        for kind in ["resposta", "encaminhamento", "complementacao"] {
                            ui.selectable_value(&mut editor.kind, kind.into(), kind_label(kind));
                        }
                    });
                template = ui
                    .button("Começar por um modelo")
                    .on_hover_text("Preenche o editor com um texto-base sem usar IA.")
                    .clicked();
                let generate_response = ui
                    .add_enabled(
                        !busy_generating && !suggestion_pending,
                        egui::Button::new(generating_label.clone().unwrap_or_else(|| "Gerar sugestão com IA".into())),
                    )
                    .on_hover_text(&disclosure)
                    .on_disabled_hover_text(if suggestion_pending {
                        "Há uma sugestão pronta aguardando revisão (barra inferior)."
                    } else {
                        "Aguarde a geração em andamento."
                    });
                generate = generate_response.clicked();
            });
            ui.add(
                egui::TextEdit::multiline(&mut editor.instructions)
                    .desired_rows(1)
                    .desired_width(f32::INFINITY)
                    .hint_text("Orientação para a IA (opcional): ex. solicitar a informação que falta, com tom objetivo."),
            );
        }
        ui.add_space(6.0);
        {
            let editor = self.editors.get_mut(&id).expect("editor exists");
            ui.horizontal_wrapped(|ui| {
                save = ui
                    .add_enabled(
                        editor.draft_dirty() && !editor.body.trim().is_empty(),
                        egui::Button::new("Salvar nova versão"),
                    )
                    .clicked();
                if ui
                    .add_enabled(!editor.body.is_empty(), egui::Button::new("Copiar texto"))
                    .on_hover_text("Copiar não salva uma versão.")
                    .clicked()
                {
                    ui.ctx().copy_text(editor.body.clone());
                }
                export = ui
                    .add_enabled(!editor.body.trim().is_empty(), egui::Button::new("Exportar (RTF)"))
                    .on_hover_text("Cria uma cópia legível fora do acervo protegido, para abrir no Word ou similar. Não salva uma versão.")
                    .clicked();
                if !detail.drafts.is_empty() {
                    ui.menu_button("Comparar com versão ▾", |ui| {
                        for draft in &detail.drafts {
                            if ui
                                .button(format!(
                                    "#{} • {} • {}",
                                    draft.id,
                                    kind_label(&draft.kind),
                                    display_date(&draft.created_at)
                                ))
                                .clicked()
                            {
                                compare = Some(draft.clone());
                                ui.close();
                            }
                        }
                    });
                }
                if editor.draft_dirty() {
                    ui.colored_label(WARNING, "● não salva");
                }
                ui.small(&editor.origin);
            });
            if editor.capture_id != detail.summary.latest_capture_id
                || self.store.sources_stale(&editor.materials)
            {
                ui.colored_label(WARNING, "Este texto foi feito sobre uma importação anterior ou fontes que mudaram. Revise antes de aproveitar.");
            }
            ui.add(
                egui::TextEdit::multiline(&mut editor.body)
                    .desired_rows(16)
                    .desired_width(f32::INFINITY)
                    .hint_text("Escreva livremente ou comece por um modelo…"),
            );
        }
        ui.add_space(6.0);
        ui.collapsing(format!("Versões salvas ({})", detail.drafts.len()), |ui| {
            for draft in &detail.drafts {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Abrir").clicked() {
                        loaded = Some(draft.clone());
                    }
                    ui.label(format!(
                        "#{} • {} • {}",
                        draft.id,
                        kind_label(&draft.kind),
                        display_date(&draft.created_at)
                    ));
                    if draft.stale || draft.capture_id != detail.summary.latest_capture_id {
                        ui.colored_label(WARNING, "importação anterior");
                    }
                });
            }
            if detail.drafts.is_empty() {
                ui.small("As versões aparecerão aqui após “Salvar nova versão”.");
            }
        });
        if export {
            let body = self.editors[&id].body.clone();
            match crate::draft_tools::export(&export_capture, &body) {
                Ok(path) => {
                    self.message(format!("Cópia exportada: {}", path.display()));
                    if let Some(parent) = path.parent() {
                        crate::windows_ui::reveal_in_explorer(parent);
                    }
                }
                Err(e) => self.error(format!("Minuta não exportada: {e}")),
            }
        }
        if let Some(draft) = compare {
            let body = self.editors[&id].body.clone();
            self.draft_tools.open_comparison(
                format!("Versão #{} • {}", draft.id, display_date(&draft.created_at)),
                draft.body,
                body,
            );
        }
        if template {
            self.replace_editor(Replacement {
                document_id: id,
                capture_id: detail.summary.latest_capture_id,
                kind: self.editors[&id].kind.clone(),
                body: ai::template_reply(&detail.capture, &self.editors[&id].kind),
                origin: "Modelo sem IA — revisar campos".into(),
                saved: false,
                from_suggestion: false,
                materials: Vec::new(),
            });
        }
        if generate {
            self.generate(detail, ui.ctx());
        }
        if save {
            self.save_draft(id);
        }
        if let Some(draft) = loaded {
            self.replace_editor(Replacement {
                document_id: id,
                capture_id: draft.capture_id,
                kind: draft.kind,
                body: draft.body,
                origin: format!("Versão salva #{}", draft.id),
                saved: true,
                from_suggestion: false,
                materials: draft.materials,
            });
        }
    }

    fn ai_destination_text(&self) -> (String, String) {
        let provider = match self.ai.provider {
            AiProvider::Local => "Ollama neste computador",
            AiProvider::Cloud => "Ollama Cloud",
            AiProvider::Remote => "servidor remoto",
        };
        let destination = url::Url::parse(self.ai.base_url().trim())
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_else(|_| "endereço ainda não configurado".into());
        (provider.to_owned(), destination)
    }

    /// Full statement of what an AI action sends and where (shown on hover
    /// and in the configuration window).
    fn ai_disclosure_text(&self, with_instructions: bool) -> String {
        let (provider, destination) = self.ai_destination_text();
        let model = if self.ai.model().trim().is_empty() {
            "não escolhido"
        } else {
            self.ai.model().trim()
        };
        let sent = if with_instructions {
            "o documento importado, o contexto, suas notas, a orientação e as fontes selecionadas"
        } else {
            "o documento importado, o contexto, as notas salvas e as fontes selecionadas"
        };
        format!("Ao clicar, envia {sent} ao {provider} ({destination}). Modelo: {model}. O resultado é uma sugestão para a sua revisão.")
    }

    /// One short, always-visible line saying where the AI runs.
    fn ai_destination(&self, ui: &mut egui::Ui) {
        let (provider, destination) = self.ai_destination_text();
        let text = match self.ai.provider {
            AiProvider::Local => format!("{provider}, sem envio externo"),
            _ => format!("envia dados ao {provider}: {destination}"),
        };
        ui.small(text)
            .on_hover_text(self.ai_disclosure_text(false));
    }

    /// Opens "Configurar IA" and asks the server for its models.
    fn open_ai_settings(&mut self, ctx: &egui::Context) {
        self.settings_open = true;
        if !self.models_match_current() {
            self.fetch_models(ctx);
        }
    }

    /// True when the listed models came from the provider and endpoint now
    /// configured (otherwise the list is stale and is not shown).
    fn models_match_current(&self) -> bool {
        self.models_for.as_ref().is_some_and(|(provider, base)| {
            *provider == self.ai.provider
                && base.trim_end_matches('/') == self.ai.base_url().trim().trim_end_matches('/')
        })
    }

    fn configuration(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        let mut save = false;
        let mut refresh = false;
        let mut provider_changed = false;
        egui::Window::new("Configurar IA").open(&mut open).default_width(600.0).resizable(true).show(ctx, |ui| {
            ui.strong("Onde a IA roda");
            ui.horizontal_wrapped(|ui| {
                let before = self.ai.provider;
                ui.radio_value(&mut self.ai.provider, AiProvider::Local, "Ollama neste computador")
                    .on_hover_text("Modelo instalado neste computador. Nada sai da máquina.");
                ui.radio_value(&mut self.ai.provider, AiProvider::Cloud, "Ollama Cloud")
                    .on_hover_text("Modelos grandes nos servidores da Ollama (ollama.com), com a chave da sua conta.");
                ui.radio_value(&mut self.ai.provider, AiProvider::Remote, "Outro servidor HTTPS")
                    .on_hover_text("Servidor do órgão compatível com a API de chat do Ollama.");
                provider_changed = before != self.ai.provider;
            });
            ui.add_space(4.0);
            match self.ai.provider {
                AiProvider::Local => {
                    let label = ui.label("Endereço do Ollama");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.local_url).desired_width(f32::INFINITY)).labelled_by(label.id);
                    ui.small("Padrão: http://127.0.0.1:11434. Instale o Ollama em ollama.com/download e baixe um modelo (ex.: ollama pull qwen3:4b-instruct).");
                }
                AiProvider::Cloud => {
                    ui.label(format!("Endereço: {} (fixo)", ai::CLOUD_URL));
                    let label = ui.label("Chave de API da sua conta");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.cloud_key).password(true).hint_text("cole aqui a chave gerada em ollama.com").desired_width(f32::INFINITY)).labelled_by(label.id);
                    ui.horizontal_wrapped(|ui| {
                        ui.small("Crie a chave em");
                        ui.hyperlink_to(RichText::new("ollama.com/settings/keys").small(), "https://ollama.com/settings/keys");
                        ui.small("(conta gratuita por uso ou plano mensal).");
                    });
                    ui.colored_label(WARNING, "Os textos enviados à IA saem deste computador para os servidores da Ollama (EUA, com possível roteamento pela Europa). A Ollama declara não registrar nem treinar com os dados.");
                }
                AiProvider::Remote => {
                    let label = ui.label("Endereço do servidor (HTTPS)");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.remote_url).desired_width(f32::INFINITY)).labelled_by(label.id);
                    let label = ui.label("Token (opcional)");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.remote_token).password(true).desired_width(f32::INFINITY)).labelled_by(label.id);
                    ui.colored_label(WARNING, "Os textos enviados à IA saem deste computador para esse servidor.");
                }
            }
            ui.add_space(6.0);
            ui.strong("Modelo");
            let listed = self.models_match_current() && !self.models.is_empty();
            ui.horizontal_wrapped(|ui| {
                if listed {
                    let models = self.models.clone();
                    let current = self.ai.model().trim().to_owned();
                    let selected = models
                        .iter()
                        .find(|m| m.name == current)
                        .map(ModelInfo::label)
                        .unwrap_or_else(|| if current.is_empty() { "Escolha um modelo".into() } else { current.clone() });
                    egui::ComboBox::from_id_salt("ai-model")
                        .selected_text(selected)
                        .width(320.0)
                        .show_ui(ui, |ui| {
                            let model = self.ai.model_mut();
                            for info in &models {
                                ui.selectable_value(model, info.name.clone(), info.label());
                            }
                        });
                } else {
                    let model = self.ai.model_mut();
                    ui.add(egui::TextEdit::singleline(model).hint_text("nome do modelo").desired_width(320.0));
                }
                let fetching = self.model_fetch.is_some();
                let button = egui::Button::new(if listed { "↻ Atualizar lista" } else { "Listar modelos do servidor" });
                if ui.add_enabled(!fetching, button).on_hover_text("Consulta o servidor configurado e lista os modelos que ele oferece.").clicked() {
                    refresh = true;
                }
                if fetching {
                    ui.spinner();
                    ui.small("consultando…");
                }
            });
            if let Some(error) = &self.models_error {
                ui.colored_label(WARNING, format!("⚠ {error}"));
            } else if listed {
                ui.small(format!("{} modelo(s) oferecido(s) por este servidor.", self.models.len()));
            } else if self.model_fetch.is_none() {
                ui.small("Clique em “Listar modelos do servidor” para escolher entre os disponíveis.");
            }
            ui.add_space(6.0);
            ui.checkbox(&mut self.ai.remember_keys, "Guardar a chave neste computador")
                .on_hover_text("A configuração fica no acervo, cifrada com a DPAPI da sua conta do Windows. Desmarcada, a chave vale só até fechar o Ponte.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let dirty = self.ai_config_dirty();
                if ui.add_enabled(dirty, egui::Button::new("Salvar")).clicked() {
                    save = true;
                }
                if dirty {
                    ui.small("alterações não salvas (também são salvas ao fechar esta janela)");
                } else {
                    ui.small("configuração salva");
                }
            });
            ui.separator();
            ui.small("O servidor só é acessado ao listar modelos, analisar ou gerar uma sugestão. A chave também pode vir da variável PONTEPROJ_AI_TOKEN. O aplicativo não solicita sua senha do SIGADOC.");
            ui.small("A análise é salva na biblioteca. A sugestão de minuta só é salva por sua ação explícita.");
        });
        if provider_changed {
            self.models_error = None;
            if !self.models_match_current() {
                self.fetch_models(ctx);
            }
        }
        if refresh {
            self.models_for = None;
            self.fetch_models(ctx);
        }
        if save && self.save_ai_config() {
            self.message("Configuração de IA salva.");
        }
        if !open && self.ai_config_dirty() {
            // Closing saves; an invalid configuration keeps the window open.
            if self.save_ai_config() {
                self.message("Configuração de IA salva.");
            } else {
                open = true;
            }
        }
        self.settings_open = open;
    }

    fn suggestion_window(&mut self, ctx: &egui::Context) {
        if !self.suggestion_open {
            return;
        }
        let Some(suggestion) = &self.suggestion else {
            return;
        };
        let mut open = true;
        let mut use_text = false;
        let mut discard = false;
        egui::Window::new("Revisar sugestão de minuta")
            .open(&mut open)
            .default_size([720.0, 570.0])
            .show(ctx, |ui| {
                ui.strong(format!(
                    "{} • {}",
                    suggestion.number,
                    kind_label(&suggestion.kind)
                ));
                ui.small(format!(
                    "Baseada na captura #{} • ainda não salva",
                    suggestion.capture_id
                ));
                ui.label("Leia o texto e leve ao editor para corrigir e salvar uma versão.");
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .show(ui, |ui| {
                        let mut text = suggestion.body.as_str();
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20),
                        );
                    });
                ui.horizontal(|ui| {
                    use_text = ui.button("Levar ao editor").clicked();
                    if ui.button("Copiar sugestão").clicked() {
                        ui.ctx().copy_text(suggestion.body.clone());
                    }
                    discard = ui.button("Descartar sugestão").clicked();
                });
            });
        self.suggestion_open = open;
        if discard {
            self.suggestion = None;
            self.suggestion_open = false;
        }
        if use_text {
            if let Some(suggestion) = self.suggestion.clone() {
                self.replace_editor(Replacement {
                    document_id: suggestion.document_id,
                    capture_id: suggestion.capture_id,
                    kind: suggestion.kind,
                    body: suggestion.body,
                    origin: "Sugestão de IA revisável".into(),
                    saved: false,
                    from_suggestion: true,
                    materials: suggestion.materials,
                });
                self.suggestion_open = false;
            }
        }
    }

    fn request_trash(&mut self, target: crate::trash::Target) {
        use crate::trash::Kind;
        if self.worker.is_some() || self.pdf_reader.busy() {
            self.error("Aguarde a operação em andamento antes de excluir itens.");
            return;
        }
        let plan = match self.store.trash_plan(target.clone()) {
            Ok(plan) => plan,
            Err(error) => {
                self.error(error.to_string());
                return;
            }
        };
        let blocked = match target.kind {
            Kind::Document => {
                self.draft_tools.dirty()
                    || self.editors.get(&plan.document).is_some_and(Editor::dirty)
                    || self.work_context.document_dirty(plan.document)
                    || self.dossier.document_dirty(plan.document)
                    || self.tasks.document_dirty(plan.document)
                    || self
                        .suggestion
                        .as_ref()
                        .is_some_and(|s| s.document_id == plan.document)
            }
            Kind::Capture => self.editors.values().any(|e| {
                e.draft_dirty()
                    && (e.capture_id == target.id
                        || e.materials.iter().any(|s| s.capture_id == target.id))
            }),
            Kind::Task => self.tasks.task_dirty(target.id),
        };
        if blocked {
            self.error("Há alterações ou sugestão ainda não salvas relacionadas a este item. Salve ou cancele essa edição antes de excluir.");
            return;
        }
        self.trash.request(target);
    }
    fn apply_trash_change(&mut self, change: crate::trash::Change) {
        use crate::trash::Kind;
        if !change.restored {
            match change.target.kind {
                Kind::Document => {
                    self.editors.remove(&change.document);
                    self.sources.forget_document(change.document);
                    self.dossier.forget_document(change.document);
                    self.work_context.forget_document(change.document);
                    self.tasks.forget_document(change.document);
                    if self.selected_id == Some(change.document) {
                        self.selected_id = None;
                        self.detail = None;
                        self.similar.clear();
                    }
                }
                Kind::Capture => {
                    self.sources.forget_document(change.document);
                    if self.selected_id == Some(change.document) {
                        self.reload(change.document);
                    }
                }
                Kind::Task => self.tasks.forget_task(change.target.id),
            }
            self.message(
                "Item movido para a lixeira local. É possível restaurá-lo pelo botão Lixeira.",
            );
        } else {
            if self.selected_id == Some(change.document) {
                self.reload(change.document);
            }
            self.message(if change.target.kind == Kind::Capture {
                "Captura restaurada no histórico. A captura em uso foi mantida."
            } else {
                "Item restaurado. A biblioteca e as tarefas foram atualizadas."
            });
        }
        self.refresh();
    }

    fn confirmations(&mut self, ctx: &egui::Context) {
        if self.replacement.is_some() {
            let mut replace = false;
            let mut cancel = false;
            egui::Window::new("Substituir a minuta no editor?").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("O editor contém alterações não salvas. Salve uma versão se quiser preservá-las.");
                ui.horizontal(|ui| { cancel = ui.button("Voltar e preservar edição").clicked(); replace = ui.button("Substituir edição não salva").clicked(); });
            });
            if cancel {
                self.replacement = None;
            }
            if replace {
                if let Some(replacement) = self.replacement.take() {
                    self.apply_replacement(replacement);
                }
            }
        }
        if self.exit_confirm {
            let mut close = false;
            let mut cancel = false;
            let mut go: Option<PendingTarget> = None;
            let pending = self.pending_items();
            egui::Window::new("Fechar o Ponte?")
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Ao fechar agora, isto será perdido:");
                    for item in &pending {
                        ui.horizontal(|ui| {
                            ui.label(format!("• {}", item.label));
                            if !matches!(item.go, PendingTarget::None) && ui.small_button("Ir para").clicked() {
                                go = Some(item.go);
                            }
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        cancel = ui.button("Voltar e salvar").clicked();
                        close = ui.button("Fechar mesmo assim").clicked();
                    });
                });
            if let Some(target) = go {
                self.exit_confirm = false;
                self.go_to(target);
            }
            if cancel {
                self.exit_confirm = false;
            }
            if close {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for PonteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.intranet.attach_repaint(&ctx);
        self.poll_worker();
        self.poll_models();
        let queued = self.store.take_offline_queued();
        if queued > 0 {
            self.message(if queued == 1 {
                "Salvo neste computador. O computador principal está fora de alcance; a alteração será enviada automaticamente ao reconectar.".to_owned()
            } else {
                format!("{queued} alterações salvas neste computador. O computador principal está fora de alcance; serão enviadas automaticamente ao reconectar.")
            });
        }
        if self
            .search_changed
            .is_some_and(|changed| changed.elapsed() > Duration::from_millis(250))
        {
            self.search_changed = None;
            self.refresh();
        } else if self.last_refresh.elapsed() > Duration::from_secs(4) {
            self.refresh();
        }
        ctx.request_repaint_after(Duration::from_millis(300));
        self.draft_tools.comparison(&ctx);
        self.pdf_reader.show(&ctx, &mut self.store);
        let pending = self.draft_tools.dirty()
            || self.pdf_reader.busy()
            || self.dossier.dirty()
            || self.work_context.dirty()
            || self.tasks.dirty()
            || self.saiop.dirty()
            || self.editors.values().any(Editor::dirty)
            || self.worker.is_some()
            || self.suggestion.is_some();
        if !self.allow_close && pending && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_confirm = true;
        }
        let enabled =
            self.replacement.is_none() && self.trash.pending.is_none() && !self.exit_confirm;
        egui::Panel::top("header").show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| self.header(ui));
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            if let Some(worker) = &self.worker {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!(
                        "{} • {} s",
                        worker.label,
                        worker.started.elapsed().as_secs()
                    ));
                });
            }
            if self.intranet.syncing() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.small("Sincronizando com o computador principal…");
                });
            }
            if let Some(suggestion) = &self.suggestion {
                ui.horizontal(|ui| {
                    ui.label(format!("Sugestão pronta: {}", suggestion.number));
                    if ui
                        .add_enabled(enabled, egui::Button::new("Revisar sugestão"))
                        .clicked()
                    {
                        self.suggestion_open = true;
                    }
                });
            }
            let mut dismiss = self
                .notice
                .as_ref()
                .is_some_and(|n| !n.error && n.since.elapsed() > NOTICE_LIFETIME);
            if let Some(notice) = &self.notice {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(if notice.error { ERROR } else { ACCENT }, &notice.text);
                    dismiss |= ui.small_button("Fechar").clicked();
                });
            }
            if dismiss {
                self.notice = None;
            }
            if let Some(path) = &self.backup_path {
                if ui.small_button("Copiar caminho do último backup").clicked() {
                    ctx.copy_text(path.display().to_string());
                }
            }
        });
        library_panel(ui.max_rect().width()).show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| self.sidebar(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if self.saiop_open {
                    if let Some(id) = self.saiop.show(ui, &mut self.store) {
                        self.select(id);
                    }
                } else if self.tasks_open {
                    if let Some(id) = self.tasks.show(ui, &mut self.store, self.selected_id) { self.select(id); }
                    if let Some(target)=self.tasks.take_trash_request(){self.request_trash(target);}
                } else if let Some(detail) = self.detail.take() {
                    let id = detail.summary.id;
                    self.document(ui, &detail);
                    if self.detail.is_none() && self.selected_id == Some(id) { self.detail = Some(detail); }
                } else {
                    self.welcome(ui);
                }
            });
        });
        if enabled {
            self.configuration(&ctx);
            self.suggestion_window(&ctx);
            if self.intranet.show(&ctx, &mut self.store, &self.db_path) {
                self.refresh();
            }
        }
        self.confirmations(&ctx);
        if let Some(change) = self.trash.show(
            &ctx,
            &mut self.store,
            self.worker.is_some()
                || self.pdf_reader.busy()
                || self.exit_confirm
                || self.replacement.is_some(),
        ) {
            self.apply_trash_change(change);
        }
    }

    fn persist_egui_memory(&self) -> bool {
        // Avoid accidental persistence of any widget memory that could contain content.
        false
    }
}

fn display_date(value: &str) -> String {
    crate::labels::datetime(value)
}

fn kind_label(kind: &str) -> &str {
    crate::labels::draft_kind(kind)
}

fn reference_kind(kind: &str) -> &str {
    crate::labels::reference_kind(kind)
}

fn safe_link(ui: &mut egui::Ui, label: &str, raw_url: &str) {
    // Only links present in the currently rendered capture reach this function.
    if allowed_sigadoc_url(raw_url) {
        ui.hyperlink_to(label, raw_url);
    } else {
        ui.label(label).on_hover_text("Link indisponível: somente endereços HTTPS do SIGADOC presentes nesta captura podem ser abertos.");
    }
}

fn allowed_sigadoc_url(raw_url: &str) -> bool {
    url::Url::parse(raw_url).ok().is_some_and(|url| {
        url.scheme() == "https"
            && matches!(
                url.host_str(),
                Some("www.sigadoc.mt.gov.br" | "sigadoc.mt.gov.br")
            )
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none_or(|port| port == 443)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        allowed_sigadoc_url, library_panel, DocPriorityFilter, DocTaskFilter,
        DocTaskStatusSummary, ACCENT, ERROR, LIBRARY_DEFAULT_WIDTH, LIBRARY_MAX_FRACTION,
        LIBRARY_MIN_WIDTH, SUCCESS, WARNING,
    };
    use crate::tasks::Task;

    fn summary(id: i64, number: &str, parent: Option<(i64, &str)>) -> crate::model::DocumentSummary {
        crate::model::DocumentSummary {
            id,
            document_number: number.into(),
            title: format!("Documento {id}"),
            last_seen_at: String::new(),
            latest_capture_id: id,
            subjects: vec![],
            priority: "normal".into(),
            analysis_stale: false,
            is_joined: parent.is_some(),
            joined_to_number: parent.map(|(_, n)| n.to_owned()),
            joined_to_id: parent.map(|(p, _)| p).filter(|p| *p > 0),
            joined_children_count: 0,
        }
    }

    /// Reproduces the window layout (top/bottom/left panels inside the root
    /// Ui, then the central panel) with synthetic pointer input, and checks
    /// that dragging the divider resizes the library and that the central
    /// area never believes it is wider than the window.
    #[test]
    fn library_divider_drags_and_central_width_fits_the_window() {
        use eframe::egui;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let left_width = std::cell::Cell::new(0.0f32);
        let central_width = std::cell::Cell::new(0.0f32);
        let run = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                        egui::Panel::top("header").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.heading("PONTE");
                            });
                        });
                        egui::Panel::bottom("status").show(ui, |ui| {
                            ui.small("status");
                        });
                        let left = library_panel(ui.max_rect().width()).show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut String::new())
                                        .desired_width(f32::INFINITY),
                                );
                                egui::ScrollArea::vertical().id_salt("document-list").show(ui, |ui| {
                                    for i in 0..5 {
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::Button::new(format!("documento {i}"))
                                                    .wrap()
                                                    .min_size(egui::vec2(ui.available_width(), 60.0)),
                                            );
                                        });
                                    }
                                });
                            });
                        left_width.set(left.response.rect.width());
                        egui::CentralPanel::default().show(ui, |ui| {
                            central_width.set(ui.available_width());
                            egui::ScrollArea::vertical().id_salt("document-content").show(ui, |ui| {
                                ui.group(|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Tipo: DECLARAÇÃO");
                                        ui.separator();
                                        ui.label("Documento Eletrônico");
                                    });
                                });
                            });
                        });
            });
            output.drop_without_applying_deltas();
        };
        run(vec![]);
        run(vec![]);
        let initial = left_width.get();
        assert!(
            (initial - LIBRARY_DEFAULT_WIDTH).abs() < 2.0,
            "largura inicial {initial}"
        );
        assert!(
            central_width.get() <= screen.width() - initial + 1.0,
            "painel central acha que tem {}px, janela {}px, lateral {initial}px",
            central_width.get(),
            screen.width()
        );
        // Press on the divider and drag 100 px to the left (stays above the minimum).
        assert!(initial - 100.0 > LIBRARY_MIN_WIDTH);
        let divider = egui::pos2(initial, 400.0);
        run(vec![egui::Event::PointerMoved(divider)]);
        run(vec![egui::Event::PointerButton {
            pos: divider,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        for step in 1..=10 {
            let pos = egui::pos2(initial - 10.0 * step as f32, 400.0);
            run(vec![egui::Event::PointerMoved(pos)]);
        }
        run(vec![egui::Event::PointerButton {
            pos: egui::pos2(initial - 100.0, 400.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        run(vec![]);
        assert!(
            (left_width.get() - (initial - 100.0)).abs() < 2.0,
            "a divisória não acompanhou o arrasto: {initial} -> {}",
            left_width.get()
        );
        // Dragging far to the right stops at 60% of the window.
        let divider = egui::pos2(left_width.get(), 400.0);
        run(vec![egui::Event::PointerMoved(divider)]);
        run(vec![egui::Event::PointerButton {
            pos: divider,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        run(vec![egui::Event::PointerMoved(egui::pos2(1100.0, 400.0))]);
        run(vec![egui::Event::PointerButton {
            pos: egui::pos2(1100.0, 400.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        run(vec![]);
        let max = screen.width() * LIBRARY_MAX_FRACTION;
        assert!(
            (left_width.get() - max).abs() < 2.0,
            "máximo esperado {max}, obtido {}",
            left_width.get()
        );
    }

    #[test]
    fn sidebar_groups_joined_documents_right_below_their_visible_parent() {
        let parent = "SESP-DIC-2026/97546";
        // Most recent first: the juntado child arrives before its parent.
        let docs = [
            summary(3, "SESP-DES-2026/39019", Some((2, parent))),
            summary(1, "SEFAZ-PRO-2026/00001", None),
            summary(2, parent, None),
            summary(4, "SESP-OFI-2026/00010", Some((2, parent))),
            summary(5, "SESP-OFI-2026/00011", Some((0, "SESP-DIC-2026/99999"))),
            summary(6, "SESP-OFI-2026/00012", Some((3, "SESP-DES-2026/39019"))),
        ];
        let refs: Vec<_> = docs.iter().collect();
        let order: Vec<(i64, usize)> = super::group_joined(&refs)
            .into_iter()
            .map(|(d, depth)| (d.id, depth))
            .collect();
        assert_eq!(order, vec![(1, 0), (2, 0), (3, 1), (6, 2), (4, 1), (5, 0)]);

        // Parent filtered out: children stay in place, at the top level.
        let without_parent: Vec<_> = docs.iter().filter(|d| d.id != 2).collect();
        let order: Vec<i64> = super::group_joined(&without_parent)
            .into_iter()
            .map(|(d, _)| d.id)
            .collect();
        assert_eq!(order, vec![3, 6, 1, 4, 5]);

        // A cycle never hides documents.
        let cycle = [
            summary(7, "A-2026/1", Some((8, "B-2026/1"))),
            summary(8, "B-2026/1", Some((7, "A-2026/1"))),
        ];
        let refs: Vec<_> = cycle.iter().collect();
        assert_eq!(super::group_joined(&refs).len(), 2);

        assert_eq!(
            super::joined_label(&docs[0]).as_deref(),
            Some("↳ Juntado a SESP-DIC-2026/97546")
        );
        assert_eq!(
            super::joined_label(&docs[4]).as_deref(),
            Some("↳ Juntado a SESP-DIC-2026/99999 (não importado)")
        );
        let mut via_only = summary(9, "X-2026/1", None);
        via_only.is_joined = true;
        assert_eq!(super::joined_label(&via_only).as_deref(), Some("↳ Documento juntado"));
        assert_eq!(super::joined_label(&docs[1]), None);
    }

    #[test]
    fn document_task_status_summary_and_filters_classify_overdue_ontime_and_completed() {
        let today = "2026-11-03";
        let mut t_overdue = Task::new(1);
        t_overdue.due_date = "2026-10-25".into();

        let mut t_ontime = Task::new(2);
        t_ontime.due_date = "2026-11-10".into();

        let mut t_today = Task::new(3);
        t_today.due_date = "2026-11-03".into();

        let mut t_done = Task::new(4);
        t_done.status = "concluida".into();

        let tasks = vec![t_overdue, t_ontime, t_today, t_done];

        let s1 = DocTaskStatusSummary::from_tasks(&tasks, 1, today);
        assert!(s1.matches(DocTaskFilter::Overdue));
        assert!(s1.matches(DocTaskFilter::OpenAny));
        assert!(!s1.matches(DocTaskFilter::OpenOnTime));
        assert!(!s1.matches(DocTaskFilter::Completed));
        assert_eq!(s1.stripe_color(), ERROR);
        assert!(s1.badge().0.contains("atrasada"));

        let s2 = DocTaskStatusSummary::from_tasks(&tasks, 2, today);
        assert!(s2.matches(DocTaskFilter::OpenOnTime));
        assert!(s2.matches(DocTaskFilter::OpenAny));
        assert!(!s2.matches(DocTaskFilter::Overdue));
        assert_eq!(s2.stripe_color(), ACCENT);
        assert!(s2.badge().0.contains("no prazo"));

        let s3 = DocTaskStatusSummary::from_tasks(&tasks, 3, today);
        assert!(s3.matches(DocTaskFilter::OpenOnTime));
        assert_eq!(s3.stripe_color(), WARNING);
        assert!(s3.badge().0.contains("vence hoje"));

        let s4 = DocTaskStatusSummary::from_tasks(&tasks, 4, today);
        assert!(s4.matches(DocTaskFilter::Completed));
        assert!(!s4.matches(DocTaskFilter::OpenAny));
        assert_eq!(s4.stripe_color(), SUCCESS);
        assert!(s4.badge().0.contains("concluída"));

        assert!(DocPriorityFilter::Urgent.matches("urgente"));
        assert!(!DocPriorityFilter::Urgent.matches("normal"));
        assert!(DocPriorityFilter::Unclassified.matches("não classificada"));
    }

    #[test]
    fn captured_links_only_open_https_sigadoc() {
        for accepted in [
            "https://www.sigadoc.mt.gov.br/documento/42",
            "https://sigadoc.mt.gov.br:443/documento?id=42",
        ] {
            assert!(allowed_sigadoc_url(accepted));
        }
        for rejected in [
            "http://www.sigadoc.mt.gov.br/documento/42",
            "https://www.sigadoc.mt.gov.br.example.org/documento/42",
            "https://example.org/?redirect=https://www.sigadoc.mt.gov.br",
            "https://user:secret@www.sigadoc.mt.gov.br/documento/42",
            "https://www.sigadoc.mt.gov.br:8443/documento/42",
            "file:///C:/Users/documento.pdf",
            "javascript:alert(1)",
            "/documento/42",
        ] {
            assert!(
                !allowed_sigadoc_url(rejected),
                "accepted unsafe URL: {rejected}"
            );
        }
    }
}
