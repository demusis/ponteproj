//! Native desktop workspace. Document content is only persisted through `Store`.
use crate::ai::{self, AiConfig, AiProvider, AiSettings, ApiKind, ModelInfo};
use crate::chatgpt::{ChatGptSession, SharedSession};
use crate::model::{Analysis, DocumentDetail, DocumentSummary};
use crate::storage::Store;
use crate::theme::{self, icon, S1, S2, S3};
use crate::widgets::{self, Tone};
use anyhow::{Context, Result};

mod backup;
pub(crate) mod calendar;
mod calendar_exchange;
mod conference;
mod ficha;
mod library;
mod minuta;
mod projects;
mod proposals;
mod queue;
mod repetition;
mod search;
mod settings;
mod today;
#[cfg(feature = "shots")]
mod tour;
mod week;
mod windows;
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Public repository with the installer, the source and the release notes.
const PROJECT_URL: &str = "https://github.com/demusis/ponteproj";
/// Version of the browser extension shipped with this build (extension/manifest.json).
const EXTENSION_VERSION: &str = "0.4.1";

/// "1 documento" / "3 documentos".
fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocTaskFilter {
    #[default]
    All,
    OpenAny,
    OpenOnTime,
    Overdue,
    Completed,
}

/// Where the documents in the list came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocOriginFilter {
    #[default]
    All,
    Sigadoc,
    Manual,
}

impl DocOriginFilter {
    fn matches(self, manual: bool) -> bool {
        match self {
            Self::All => true,
            Self::Sigadoc => !manual,
            Self::Manual => manual,
        }
    }
}

/// Order of the document list. `Recent` is the library's own order (last
/// seen first); the others reorder the filtered list before grouping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DocSort {
    #[default]
    Recent,
    Deadline,
    Priority,
    Number,
    Title,
}

impl DocSort {
    const ALL: [DocSort; 5] = [
        DocSort::Recent,
        DocSort::Deadline,
        DocSort::Priority,
        DocSort::Number,
        DocSort::Title,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Recent => "Mais recentes primeiro",
            Self::Deadline => "Prazo mais próximo primeiro",
            Self::Priority => "Prioridade (urgente primeiro)",
            Self::Number => "Número do documento",
            Self::Title => "Assunto (A–Z)",
        }
    }

    /// Stable sort: ties keep the "most recent first" order of the library.
    fn apply(self, docs: &mut [&DocumentSummary], nearest_due: impl Fn(i64) -> Option<String>) {
        fn priority_rank(priority: &str) -> u8 {
            match priority {
                "urgente" => 0,
                "alta" => 1,
                "normal" => 2,
                "baixa" => 3,
                _ => 4,
            }
        }
        match self {
            Self::Recent => {}
            Self::Deadline => docs.sort_by_cached_key(|d| match nearest_due(d.id) {
                Some(due) => (0u8, due),
                None => (1, String::new()),
            }),
            Self::Priority => docs.sort_by_key(|d| priority_rank(&d.priority)),
            Self::Number => docs.sort_by_cached_key(|d| d.document_number.to_uppercase()),
            Self::Title => docs.sort_by_cached_key(|d| d.title.trim().to_lowercase()),
        }
    }
}

/// "Registro manual" window: creates a record or saves a new version of one.
#[derive(Default)]
struct ManualForm {
    open: bool,
    /// Document being edited; `None` when creating.
    id: Option<i64>,
    number: String,
    fields: crate::manual::ManualFields,
    original: crate::manual::ManualFields,
    message: String,
    /// The step "Colar conversa do WhatsApp", when it is showing.
    paste: Option<PasteStep>,
}

/// What the person pastes and what is read from it.
struct PasteStep {
    /// Showing in the window (hidden when the person went back to the form;
    /// what was pasted is kept).
    visible: bool,
    text: String,
    chat: String,
    title: String,
    /// The title was typed by the person: it is no longer replaced by the
    /// suggestion.
    title_edited: bool,
    /// Reading of `text`; recomputed when the text changes.
    parsed: Option<Result<crate::whatsapp::Conversation, String>>,
}

impl PasteStep {
    fn new() -> Self {
        Self {
            visible: true,
            text: String::new(),
            chat: String::new(),
            title: String::new(),
            title_edited: false,
            parsed: None,
        }
    }

    /// Reads the pasted text again and, unless the person wrote the title,
    /// suggests one.
    fn reread(&mut self) {
        // Never lay out or read more than a record can hold.
        let limit = 2 * crate::model::MAX_CAPTURE_TEXT_BYTES + 1;
        if self.text.len() > limit {
            let mut cut = limit;
            while !self.text.is_char_boundary(cut) {
                cut -= 1;
            }
            self.text.truncate(cut);
        }
        self.parsed = if self.text.trim().is_empty() {
            None
        } else {
            Some(crate::whatsapp::parse(&self.text).map_err(|e| e.to_string()))
        };
        if !self.title_edited {
            self.title = match &self.parsed {
                Some(Ok(conversation)) => {
                    crate::whatsapp::suggested_title(&self.chat, conversation)
                }
                _ => String::new(),
            };
        }
    }
}

impl ManualForm {
    fn open_new(&mut self) {
        // A record being typed is never replaced by a blank one.
        if self.open && self.dirty() {
            return;
        }
        let fields = crate::manual::ManualFields {
            kind: "memorando".into(),
            ..Default::default()
        };
        *self = Self {
            open: true,
            original: fields.clone(),
            fields,
            ..Default::default()
        };
    }

    fn open_edit(&mut self, detail: &DocumentDetail) {
        let fields = crate::manual::ManualFields::from_capture(&detail.capture);
        *self = Self {
            open: true,
            id: Some(detail.summary.id),
            number: detail.summary.document_number.clone(),
            original: fields.clone(),
            fields,
            message: String::new(),
            paste: None,
        };
    }

    fn dirty(&self) -> bool {
        self.open
            && (self.fields != self.original
                || self
                    .paste
                    .as_ref()
                    .is_some_and(|p| !p.text.trim().is_empty()))
    }
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

/// What the derived part of a periodic refresh reads (see
/// `PonteApp::refresh_polled`): when it is the same as at the last refresh,
/// what was built then is still right.
#[derive(Clone, Debug, PartialEq)]
struct RefreshKey {
    /// `Store::library_stamp`; `None` when it could not be read, and then
    /// nothing is taken for current.
    stamp: Option<(i64, i64)>,
    day: String,
    minute: String,
    /// Days without a move after which a follow-up is due (Atendimentos).
    stale_days: i64,
    /// The holiday calendar the planning counts working days with.
    holidays: crate::holidays::HolidayConfig,
    selected: Option<i64>,
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
        for task in tasks.iter().filter(|t| t.document_id == Some(document_id)) {
            summary.add(task, today_iso);
        }
        summary
    }

    /// The summary of every document that has tasks, in one pass over the
    /// tasks (a document without any is not in the map: its summary is the
    /// default one).
    fn by_document(
        tasks: &[crate::tasks::Task],
        today_iso: &str,
    ) -> std::collections::HashMap<i64, Self> {
        let mut summaries = std::collections::HashMap::<i64, Self>::new();
        for task in tasks {
            if let Some(document) = task.document_id {
                summaries.entry(document).or_default().add(task, today_iso);
            }
        }
        summaries
    }

    fn add(&mut self, task: &crate::tasks::Task, today_iso: &str) {
        if task.status == "concluida" {
            self.completed += 1;
        } else {
            let is_overdue = !task.due_date.is_empty() && task.due_date.as_str() < today_iso;
            if is_overdue {
                self.overdue += 1;
            } else {
                self.open_on_time += 1;
                if task.due_date == today_iso {
                    self.due_today += 1;
                }
            }
            if !task.due_date.is_empty() {
                match &self.nearest_due {
                    Some(current) if current.as_str() <= task.due_date.as_str() => {}
                    _ => self.nearest_due = Some(task.due_date.clone()),
                }
            }
        }
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

    /// Short label and tone of the pill in the document list.
    fn short_pill(&self) -> (String, Tone) {
        if self.overdue > 0 {
            let text = if self.overdue == 1 {
                "1 tarefa atrasada".to_owned()
            } else {
                format!("{} tarefas atrasadas", self.overdue)
            };
            (text, Tone::Danger)
        } else if self.open_on_time > 0 {
            if self.due_today > 0 {
                let text = if self.due_today == 1 {
                    "Vence hoje".to_owned()
                } else {
                    format!("{} vencem hoje", self.due_today)
                };
                (text, Tone::Warn)
            } else if self.open_on_time == 1 {
                ("1 em aberto, no prazo".to_owned(), Tone::Info)
            } else {
                (
                    format!("{} em aberto, no prazo", self.open_on_time),
                    Tone::Info,
                )
            }
        } else if self.completed > 0 {
            let text = if self.completed == 1 {
                "1 tarefa concluída".to_owned()
            } else {
                format!("{} tarefas concluídas", self.completed)
            };
            (text, Tone::Ok)
        } else {
            ("Sem tarefas".to_owned(), Tone::Neutral)
        }
    }

    /// The full label and tone of the pill in the ficha header: the text of
    /// `badge` without its leading symbol.
    fn pill_parts(&self) -> (String, Tone) {
        let (label, _, _) = self.badge();
        let text = label
            .trim_start_matches(['⚠', '⏰', '●', '✓', '○', ' '])
            .replace(" • ✓ ", " • concluídas: ")
            .replace(" • ", " · ");
        let tone = if self.overdue > 0 {
            Tone::Danger
        } else if self.open_on_time > 0 {
            if self.due_today > 0 {
                Tone::Warn
            } else {
                Tone::Info
            }
        } else if self.completed > 0 {
            Tone::Ok
        } else {
            Tone::Neutral
        };
        (text, tone)
    }

    /// The label under a document card: text, text colour and background.
    /// The worst situation wins: overdue, then due today, then open on
    /// time, then all done, then no tasks.
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
            (label, crate::theme::error(), crate::theme::c().danger_soft)
        } else if self.open_on_time > 0 {
            let mut label = if self.due_today > 0 {
                // Only the tasks due today are counted as such; the other
                // open ones follow in brackets, as in the overdue label.
                let mut label = if self.due_today == 1 {
                    "⏰ 1 vence hoje".to_owned()
                } else {
                    format!("⏰ {} vencem hoje", self.due_today)
                };
                let later = self.open_on_time - self.due_today;
                if later > 0 {
                    label.push_str(&format!(" (+{later} no prazo)"));
                }
                label
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
                (label, crate::theme::warning(), crate::theme::c().warn_soft)
            } else {
                (label, crate::theme::on_time(), crate::theme::on_time_bg())
            }
        } else if self.completed > 0 {
            let label = if self.completed == 1 {
                "✓ 1 tarefa concluída".to_owned()
            } else {
                format!("✓ {} tarefas concluídas", self.completed)
            };
            (label, crate::theme::success(), crate::theme::c().ok_soft)
        } else {
            (
                "○ Sem tarefas".to_owned(),
                crate::theme::c().muted,
                crate::theme::c().sunken,
            )
        }
    }
}

pub fn run(initial_document: Option<i64>) -> Result<()> {
    run_opening(initial_document, false)
}

/// Opens the window, on a document or on the "Modelos" area.
pub fn run_opening(initial_document: Option<i64>, open_templates: bool) -> Result<()> {
    // Where the library is (the pointer of the main folder, or
    // PONTEPROJ_DATA_DIR), read once for the whole process.
    let resolution = crate::paths::resolution()?;
    let db_path = crate::paths::db_path()?;
    // Why this window opened here, when a switch of library opened it.
    let opening_notice = std::env::var(crate::paths::OPENING_NOTICE_ENV).ok();
    std::env::remove_var(crate::paths::OPENING_NOTICE_ENV);
    // A document the browser just imported is shown only in the library that
    // imported it: its number means another document in any other.
    let expected = std::env::var_os(crate::paths::REQUEST_LIBRARY_ENV).map(PathBuf::from);
    let (initial_document, other_library) = crate::paths::document_for_this_library(
        initial_document,
        expected.as_deref(),
        &resolution.data_dir,
    );
    // One window per library: a second start hands its request to the window
    // already open and leaves. If that window does not answer, this start
    // opens its own, as older versions always did.
    let data_dir = crate::paths::data_dir()?;
    #[cfg(feature = "shots")]
    let alone = crate::shots::active();
    #[cfg(not(feature = "shots"))]
    let alone = false;
    let window = if alone {
        None
    } else {
        match crate::single_instance::acquire(&data_dir) {
            crate::single_instance::Window::Mine(guard) => Some(guard),
            crate::single_instance::Window::Other => {
                let request = match initial_document {
                    Some(id) => crate::single_instance::Request::Document(id),
                    None if open_templates => crate::single_instance::Request::Templates,
                    None => crate::single_instance::Request::Focus,
                };
                if crate::single_instance::hand_over(&data_dir, request) {
                    return Ok(());
                }
                None
            }
            crate::single_instance::Window::Unknown => None,
        }
    };
    let store = Store::open(&db_path).context("Não foi possível abrir o acervo protegido")?;
    // Temporary copies of attachments opened in other programs do not outlive a
    // session: what crashed or older sessions left goes now (never another
    // window's live folder), this one's goes at close.
    crate::attachments::sweep_stale_temp();
    let mut app = PonteApp::new(store, db_path);
    app.library = backup::LibraryPlace::detect(&resolution, &data_dir);
    app.requests_dir = window.is_some().then_some(data_dir);
    // A recovery library being looked at gets no automatic copies.
    app.auto_copy.allowed = !alone && !app.library.only_looking();
    // Nor any exchange: a library that is not the one in use is opened only
    // to be looked at, whatever its saved configuration says.
    app.apply_transport();
    app.intranet.initialize(&mut app.store, &app.db_path);
    if let Some(notice) = other_library {
        app.library.notice = Some(notice);
    }
    if let Some(notice) = opening_notice {
        app.error(notice);
    }
    // Personal templates of versions before 0.20 become work templates.
    match app.store.migrate_personal_templates() {
        Ok((0, problems)) if problems.is_empty() => {}
        Ok((count, problems)) => {
            if problems.is_empty() {
                app.message(if count == 1 {
                    "1 modelo pessoal de versões anteriores foi levado para a área Modelos.".to_owned()
                } else {
                    format!("{count} modelos pessoais de versões anteriores foram levados para a área Modelos.")
                });
            } else {
                app.error(format!(
                    "{}; não foi possível converter os demais: {}",
                    plural(
                        count,
                        "modelo antigo foi convertido",
                        "modelos antigos foram convertidos"
                    ),
                    problems.join("; ")
                ));
            }
        }
        Err(error) => app.error(format!(
            "Os modelos pessoais antigos não puderam ser convertidos: {error}"
        )),
    }
    // E-mails imported by 0.22–0.23 (numbered MANUAL-) get their own numbering.
    match crate::storage::renumber_imported_emails(&mut app.store) {
        Ok(0) => {}
        Ok(count) => app.message(if count == 1 {
            "1 e-mail importado antes desta versão passou a ter número próprio (EMAIL-ano/sequência).".to_owned()
        } else {
            format!("{count} e-mails importados antes desta versão passaram a ter número próprio (EMAIL-ano/sequência).")
        }),
        Err(error) => app.error(format!(
            "Os e-mails importados não puderam receber número próprio: {error}"
        )),
    }
    app.load_ai_config();
    app.refresh();
    // An empty library opens on the welcome screen, not on an empty day.
    if app.documents.is_empty() {
        app.area = Area::Documents;
    }
    if open_templates {
        app.area = Area::Templates;
    }
    if let Some(id) = initial_document {
        app.select(id);
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title(format!(
            "Ponte {} • {}",
            env!("CARGO_PKG_VERSION"),
            if app.library.only_looking() {
                "Acervo de recuperação (só para olhar)"
            } else {
                "Assistente documental"
            }
        ))
        .with_icon(crate::icon::app_window_icon())
        .with_inner_size(window_size())
        .with_min_inner_size(crate::theme::min_window_size(app.text_size));
    // The pictures of the screens are taken away from the desktop, without
    // taking the keyboard from whoever is working.
    #[cfg(feature = "shots")]
    let viewport = if crate::shots::active() {
        viewport
            .with_position([-4000.0, -4000.0])
            .with_active(false)
            .with_taskbar(false)
    } else {
        viewport
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Ponte — Assistente documental",
        options,
        Box::new(move |cc| {
            crate::theme::install(&cc.egui_ctx, app.appearance);
            crate::theme::set_text_size(&cc.egui_ctx, app.text_size);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Não foi possível iniciar a janela: {error}"))?;
    // Held until the window closes.
    drop(window);
    // A switch of library: this window closed (its lock released above), the
    // pointer was written, and the Ponte opens again on the chosen library.
    if let Some(notice) = backup::take_relaunch() {
        crate::recovery::open_library_in_use(notice)?;
    }
    Ok(())
}

/// A pill in the colours of a tone (kept as the name the screens already use).
fn pill(ui: &mut egui::Ui, text: impl Into<String>, tone: Tone) -> egui::Response {
    widgets::pill(ui, text, tone)
}

/// A soft-coloured line for a warning or a remark inside a card.
fn notice_line(ui: &mut egui::Ui, tone: Tone, text: impl Into<String>) {
    let (fg, bg) = tone.colors();
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text.into()).color(fg)).wrap());
        });
}

/// A card with a title; `actions` are drawn on the right of the title row
/// (the first one added ends up at the far right).
fn titled_card<R>(
    ui: &mut egui::Ui,
    title: &str,
    actions: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    widgets::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = S2;
        ui.horizontal(|ui| {
            widgets::section(ui, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
        });
        body(ui)
    })
}

/// Label and value pairs, labels muted, values wrapping.
fn facts(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, rows: &[(&str, String)]) {
    let value_width = (ui.available_width() - 140.0 - S3).max(160.0);
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([S3, S1 + 2.0])
        .min_col_width(130.0)
        .max_col_width(value_width)
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(*label).color(theme::c().muted));
                ui.add(egui::Label::new(value.as_str()).wrap());
                ui.end_row();
            }
        });
}

/// The areas of the window, chosen in the rail on the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    Today,
    Documents,
    Tasks,
    Projects,
    Saiop,
    Templates,
    Resources,
    Settings,
}

/// The window opens at this size (fixed while the pictures of the screens are taken).
fn window_size() -> [f32; 2] {
    #[cfg(feature = "shots")]
    if crate::shots::active() {
        return crate::shots::size();
    }
    [1240.0, 820.0]
}

/// The sections of the Ajustes area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSection {
    Team,
    Mail,
    Import,
    Reminders,
    Holidays,
    Ai,
    Backup,
    Trash,
    Categories,
    Appearance,
    About,
}

impl SettingsSection {
    const ALL: [Self; 11] = [
        Self::Team,
        Self::Mail,
        Self::Import,
        Self::Reminders,
        Self::Holidays,
        Self::Ai,
        Self::Backup,
        Self::Categories,
        Self::Trash,
        Self::Appearance,
        Self::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Team => "Equipe e sincronização",
            Self::Mail => "E-mail",
            Self::Import => "Importação do SIGADOC",
            Self::Reminders => "Lembretes",
            Self::Holidays => "Feriados",
            Self::Ai => "Inteligência artificial",
            Self::Backup => "Cópia de segurança e exportação",
            Self::Categories => "Categorias",
            Self::Trash => "Lixeira",
            Self::Appearance => "Aparência",
            Self::About => "Sobre o Ponte",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::Team => {
                "Como este computador compartilha o acervo com outros computadores e com o celular."
            }
            Self::Mail => "Contas de e-mail para importar mensagens como registros.",
            Self::Import => "O que o Ponte faz quando um documento é importado pelo navegador.",
            Self::Reminders => "Um aviso do Windows, uma vez por dia, com o que vence.",
            Self::Holidays => "Dias em que não há expediente, para o planejamento do trabalho.",
            Self::Ai => "Opcional. Onde a análise e as minutas são geradas.",
            Self::Backup => "Guardar uma cópia do acervo e levar os dados para planilhas.",
            Self::Categories => "Palavras suas para organizar tarefas e documentos.",
            Self::Trash => "O que foi excluído fica aqui por um tempo e pode ser restaurado.",
            Self::Appearance => "Tema claro ou escuro.",
            Self::About => "Versão, guia do usuário, autores e licença.",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Team => icon::TEAM,
            Self::Mail => icon::MAIL,
            Self::Import => icon::DOWNLOAD,
            Self::Reminders => icon::BELL,
            Self::Holidays => icon::CALENDAR,
            Self::Ai => icon::AI,
            Self::Backup => icon::DATABASE,
            Self::Categories => icon::TAG,
            Self::Trash => icon::TRASH,
            Self::Appearance => icon::SUN,
            Self::About => icon::INFO,
        }
    }
}

/// Where the choice of theme is kept: a plain word in the data folder (it says
/// nothing about the library).
fn appearance_path() -> Option<PathBuf> {
    crate::paths::data_dir()
        .ok()
        .map(|dir| dir.join("aparencia.txt"))
}

/// The file holds two words: the theme and the text size (older versions
/// wrote only the first).
fn load_appearance() -> (crate::theme::Appearance, crate::theme::TextSize) {
    let text = appearance_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let mut words = text.split_whitespace();
    (
        crate::theme::Appearance::from_key(words.next().unwrap_or("")),
        crate::theme::TextSize::from_key(words.next().unwrap_or("")),
    )
}

fn save_appearance(appearance: crate::theme::Appearance, size: crate::theme::TextSize) {
    if let Some(path) = appearance_path() {
        let _ = std::fs::write(path, format!("{} {}", appearance.key(), size.key()));
    }
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
    crate::help_ui::help(ui, text);
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
    /// "Pedido desta minuta": what the person asks the AI (trusted layer).
    instructions: String,
    /// The work template chosen for generation, and its optional and
    /// conditional blocks the person ticked.
    template_uid: Option<String>,
    selected_blocks: std::collections::BTreeSet<String>,
    /// Where the text in the editor came from, saved with the version.
    provenance: Option<crate::model::DraftProvenance>,
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
            template_uid: None,
            selected_blocks: Default::default(),
            provenance: None,
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
    provenance: Option<crate::model::DraftProvenance>,
}

#[derive(Clone)]
struct Suggestion {
    materials: Vec<crate::evidence::Source>,
    document_id: i64,
    number: String,
    capture_id: i64,
    kind: String,
    body: String,
    intent: crate::work_templates::Intent,
    /// The editor text when the generation was asked (shown side by side
    /// for a revision).
    previous_text: String,
    provenance: Option<crate::model::DraftProvenance>,
    /// Why the conference of the criteria was not made with the generation.
    conference_skipped: Option<String>,
}

// A draft result carries the generation record and the text; the other
// results are small. One value is sent per job, so boxing would only add
// noise.
#[allow(clippy::large_enum_variant)]
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
        intent: crate::work_templates::Intent,
        previous_text: String,
        /// What was sent, kept whether or not the text was accepted.
        record: crate::work_templates::GenerationRecord,
        result: std::result::Result<String, String>,
        /// The conference of the criteria made with the generation.
        conference: Option<crate::ai::DraftConference>,
        conference_skipped: Option<String>,
    },
    /// "Conferir a minuta".
    Conference {
        document_id: i64,
        result: std::result::Result<crate::ai::DraftConference, String>,
    },
    /// "Fazer cópia agora": the copy, and what makes it not a good one.
    Backup(std::result::Result<(PathBuf, Option<String>), String>),
    /// "Restaurar cópia…" and "Importar cópia portátil…": the recovery
    /// library built.
    Recovery(std::result::Result<crate::recovery::Built, String>),
    /// "Exportar cópia portátil…": the file written.
    PortableExport(std::result::Result<crate::portable_copy::Exported, String>),
    Export(std::result::Result<crate::export::ExportReport, String>),
    MailFolders(std::result::Result<Vec<crate::imap::Folder>, String>),
    MailMessages(std::result::Result<Vec<crate::imap::MessageSummary>, String>),
    /// Raw messages by uid, to be imported on the window's thread.
    MailFetched(
        std::result::Result<crate::imap::Fetched, String>,
        crate::mail::MailSource,
    ),
    /// The result of "Testar a IA com casos fictícios".
    AiTest(std::result::Result<crate::ai_eval::Report, String>),
}

/// What the background worker is doing; the screen adapts buttons to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkKind {
    Sources,
    Analysis,
    Draft,
    Backup,
    /// Building a recovery library (Ajustes ▸ Cópia de segurança).
    Recovery,
    Export,
    /// Talking to a mailbox (folders, list, fetch).
    Mail,
    /// "Testar a IA com casos fictícios".
    AiTest,
    /// "Conferir a minuta".
    Conference,
}

struct Worker {
    receiver: Receiver<WorkResult>,
    label: String,
    kind: WorkKind,
    started: Instant,
    /// Steps reported by the task ("Recebendo a resposta… (1.200 caracteres)").
    progress: Receiver<String>,
    latest: String,
    cancel: Arc<AtomicBool>,
}

impl Worker {
    fn cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Short text for a button: the current step without counts.
    fn short_step(&self) -> String {
        if self.cancelling() {
            return "cancelando…".into();
        }
        let step = self
            .latest
            .split(" (")
            .next()
            .unwrap_or("")
            .trim_end_matches('…');
        if step.is_empty() {
            format!("{} s", self.started.elapsed().as_secs())
        } else if let Some(attempt) = step.strip_prefix("Tentativa ") {
            format!(
                "tentativa {}",
                attempt.split(':').next().unwrap_or("").trim()
            )
        } else {
            format!(
                "{}, {} s",
                step.to_lowercase(),
                self.started.elapsed().as_secs()
            )
        }
    }
}

/// The record of a generation and the conference of its text, read once for
/// the window "Material utilizado".
struct MaterialCache {
    document_id: i64,
    uid: String,
    record: crate::work_templates::GenerationRecord,
    conference: Option<crate::ai::DraftConference>,
}

struct Notice {
    text: String,
    error: bool,
    since: Instant,
    /// One button on the notice ("Desfazer") and what it does.
    action: Option<(String, NoticeAction)>,
}

/// What the button of a notice does.
#[derive(Clone)]
enum NoticeAction {
    /// Undo "concluir": the task is open again (as "pendente").
    ReopenTask(i64),
    /// Undo "excluir": the item comes back from the Lixeira.
    Restore(crate::trash::Target),
    /// Open Ajustes ▸ Lixeira.
    OpenTrash,
}

/// Where the person was before a document was opened for them, so the ficha
/// can offer the way back.
#[derive(Clone, PartialEq, Eq)]
enum ReturnTo {
    Today,
    Task(i64),
    Attendance(i64),
    /// The use (by uid) of a resource the document was opened from.
    Resources(String),
}

/// Success notices leave by themselves; errors stay until closed.
const NOTICE_LIFETIME: Duration = Duration::from_secs(7);
/// Seconds between two polls of the synchronization (and of what depends on
/// the library; see `PonteApp::refresh_polled`).
const REFRESH_EVERY: Duration = Duration::from_secs(4);
/// The search is read this long after the last key.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);
/// How often the thread of `watch_other_starts` looks for a request.
const REQUEST_LOOK_EVERY: Duration = Duration::from_millis(250);

/// How long the window may go without a frame when nobody touches it. Work in
/// the background and the files of other starts wake it by themselves; what
/// is left is time: the next poll (every `REFRESH_EVERY`), and the labels
/// that count minutes ("há 2 min" in the status bar), which want a frame at
/// least every second while the window is in use and as often as the poll
/// when it is not. Only what must show seconds, or end at a moment, asks for
/// less: a running job (its seconds and steps), a notice about to leave, a
/// search waiting for its pause.
fn idle_wait(
    since_refresh: Duration,
    focused: bool,
    job_running: bool,
    notice_left: Option<Duration>,
    search_waiting: Option<Duration>,
) -> Duration {
    // So that the moment has passed when the frame comes.
    const MARGIN: Duration = Duration::from_millis(10);
    let cap = if focused {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(5)
    };
    let mut wait = (REFRESH_EVERY.saturating_sub(since_refresh) + MARGIN).min(cap);
    if job_running {
        wait = wait.min(Duration::from_millis(300));
    }
    if let Some(left) = notice_left {
        wait = wait.min(left + MARGIN);
    }
    if let Some(elapsed) = search_waiting {
        wait = wait.min(SEARCH_DEBOUNCE.saturating_sub(elapsed) + MARGIN);
    }
    wait
}

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
    Templates,
    Resources,
    Mail,
    Suggestion,
    Settings,
    Network,
    None,
}

/// Where the person wants to go while a task edit is unsaved. Held until
/// they decide whether to save, discard or stay.
#[derive(Clone)]
enum Leave {
    /// The "Hoje" screen.
    Today,
    /// Open the week planner after reaching Hoje.
    Week(egui::Context),
    /// Open a new manual record after reaching Documentos.
    NewManual,
    /// Open a new attendance after reaching Atendimentos.
    NewAttendance,
    /// Open a new reservation after reaching Recursos.
    NewReservation,
    /// The "Projetos" screen.
    Projects,
    /// A new project, in the form of the "Projetos" screen.
    NewProject,
    /// This project (uid), open in "Projetos".
    Project(String),
    /// A new task for this project (uid), in the form of Tarefas.
    NewProjectTask(String),
    /// The "Atendimentos" screen.
    Saiop,
    /// The "Modelos" screen.
    Templates,
    /// The "Recursos" screen.
    Resources,
    /// The "Ajustes" screen.
    Settings,
    /// The "Documento" screen with whatever document is selected.
    Documents,
    /// Open this document (sidebar, "Ir para o documento").
    Select(i64),
    /// Start a blank task for this document.
    NewTask(Option<i64>),
    /// Start a task without document with the title and deadline typed in
    /// the quick capture of Ctrl+K (`due_date` is `AAAA-MM-DD` or empty).
    NewTaskCaptured { title: String, due_date: String },
    /// Open this task in the form, keeping the panel's view and filter.
    OpenTask(crate::tasks::Task),
    /// Open this task from its document's Resumo, filtering the panel to it.
    EditTask(crate::tasks::Task),
    /// Prepare a task from an AI suggestion of this document.
    Suggestion {
        document_id: i64,
        suggestion: crate::model::ActionSuggestion,
        priority: String,
    },
    /// Prepare a task from a proposal of an analysis of this document.
    TaskProposal {
        document_id: i64,
        proposal: crate::model::TaskProposal,
    },
}

impl Leave {
    /// Intents that replace the task being edited, wherever they come from.
    fn replaces_task(&self) -> bool {
        matches!(
            self,
            Leave::NewTask(_)
                | Leave::NewTaskCaptured { .. }
                | Leave::NewProjectTask(_)
                | Leave::OpenTask(_)
                | Leave::EditTask(_)
                | Leave::Suggestion { .. }
                | Leave::TaskProposal { .. }
        )
    }
}

/// A document shown read-only over the Tarefas panel, so the person can
/// read it while writing the task.
struct Consult {
    detail: DocumentDetail,
}

/// "Entrar com o ChatGPT" in progress: the browser is open and a thread
/// waits for the callback.
struct ChatGptLogin {
    /// The outcome and, when OpenAI issued one on the way, the client id of
    /// this computer's registration (kept even if the sign-in failed).
    receiver: Receiver<(std::result::Result<ChatGptSession, String>, Option<String>)>,
    cancel: Arc<AtomicBool>,
    authorize_url: String,
}

/// "Testar a conexão com a OpenAI": running on its own thread, or done.
enum NetTest {
    Running(Receiver<crate::netdiag::NetReport>),
    Done(crate::netdiag::NetReport),
}

struct PonteApp {
    draft_tools: crate::draft_tools::DraftTools,
    /// The "Modelos" screen.
    templates: crate::templates_ui::TemplatesPanel,
    /// The generation record being shown in "Material utilizado".
    material: Option<(i64, String)>,
    /// What that window read from the library, once.
    material_cache: Option<MaterialCache>,
    /// The document whose analysis shows its "Material utilizado".
    analysis_material: Option<i64>,
    /// The conference panel of the draft in the editor and of the suggestion
    /// being reviewed (the check runs again only when the text changes).
    review_editor: Option<conference::DraftReview>,
    review_suggestion: Option<conference::DraftReview>,
    /// The document whose reading failed (told once, not at each refresh).
    load_error_for: Option<i64>,
    /// What the Propostas block reads from the library, once per load.
    proposal_view: Option<proposals::ProposalView>,
    /// The one AI job waiting for the running one.
    queued: Option<queue::QueuedJob>,
    /// The question "Enviar este documento a …?" waiting for an answer.
    send_confirm: Option<queue::SendConfirm>,
    /// The last "Testar a IA com casos fictícios" and the destinations the
    /// person agreed to send the fictitious cases to in this session.
    ai_test_report: Option<std::result::Result<crate::ai_eval::Report, String>>,
    ai_test_consented: std::collections::BTreeSet<String>,
    /// The "Importar do e-mail" window.
    mail: crate::mail_ui::MailPanel,
    /// "Anexos" block of the open document.
    attachments: crate::attachments::AttachmentsPanel,
    sources: crate::evidence::SourcesPanel,
    pdf_reader: crate::pdf_reader::PdfPanel,
    dossier: crate::dossier::DossierPanel,
    work_context: crate::work_context::ContextPanel,
    tasks: crate::tasks_ui::TaskPanel,
    saiop: crate::saiop::SaiopPanel,
    /// The "Recursos" area.
    resources: crate::resources_ui::ResourcesPanel,
    /// The "Projetos" area.
    projects: projects::ProjectsPanel,
    /// Returns overdue, for the badge of the rail (read on each refresh).
    resources_overdue: usize,
    intranet: crate::intranet::IntranetPanel,
    /// The area on screen (the rail on the left chooses it).
    area: Area,
    /// What the "Hoje" area shows, read on each refresh.
    today: today::TodayData,
    /// The once-a-day Windows notice of what is due.
    reminders: crate::reminders::Reminders,
    /// The state and the extra days off, as edited in Ajustes.
    holiday_draft: settings::HolidayDraft,
    /// Library revision at the last attempt to put this computer's own
    /// holidays into the library (see `sync_holidays_with_library`).
    holiday_migration_revision: Option<Option<i64>>,
    /// Whether this computer's own holidays were already handed to a library
    /// (it has a value now): remembered in a small file, so the hand-over
    /// happens at most once per computer.
    holiday_seed_done: bool,
    /// The way back offered on the ficha, and the one waiting for the next
    /// document to be opened.
    return_to: Option<ReturnTo>,
    return_after_select: Option<ReturnTo>,
    /// Ctrl+F asked for the search box of the document list.
    focus_search: bool,
    /// The search across the whole library (Ctrl+K).
    global_search: search::GlobalSearch,
    calendar_exchange: calendar_exchange::CalendarExchange,
    repetition: repetition::Repetition,
    /// "Is there a newer version?" in Ajustes ▸ Sobre o Ponte.
    update: crate::update_check::UpdateState,
    /// Open tasks that are late or due today, shown on the rail.
    task_badge: usize,
    appearance: crate::theme::Appearance,
    text_size: crate::theme::TextSize,
    settings_section: SettingsSection,
    #[cfg(feature = "shots")]
    tour: Option<crate::shots::Tour>,
    /// Development only: how far the ficha is scrolled in the pictures.
    #[cfg(feature = "shots")]
    shots_ficha_scroll: f32,
    /// Development only: the conference panel of the saved draft is shown open.
    #[cfg(feature = "shots")]
    shots_conference_open: bool,
    /// Navigation waiting for a decision about the unsaved task.
    leave: Option<Leave>,
    /// Why "Salvar e continuar" failed, shown inside the question.
    leave_error: Option<String>,
    /// Read-only document window opened from the task form.
    consult: Option<Consult>,
    /// "Sobre o Ponte" window (authors, version, licence).
    about_open: bool,
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
    /// Where other starts leave their requests (open a document, come to
    /// the front); `None` when this window does not answer for the library.
    requests_dir: Option<std::path::PathBuf>,
    requests_checked: Instant,
    /// The thread that wakes the window when another start leaves a request
    /// is running.
    requests_watched: bool,
    /// Last automatic sweep of the trash (at start and once a day).
    last_trash_purge: Option<Instant>,
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
    /// ChatGPT session shared with worker threads (token refresh happens there).
    chatgpt_shared: SharedSession,
    chatgpt_login: Option<ChatGptLogin>,
    chatgpt_login_error: Option<String>,
    net_test: Option<NetTest>,
    /// A worker used the ChatGPT session: copy it back and save when done.
    chatgpt_sync_pending: bool,
    worker: Option<Worker>,
    suggestion: Option<Suggestion>,
    suggestion_open: bool,
    replacement: Option<Replacement>,
    trash: crate::trash::Panel,
    exit_confirm: bool,
    allow_close: bool,
    notice: Option<Notice>,
    backup_path: Option<PathBuf>,
    /// The automatic copy of the day (Ajustes ▸ Cópia de segurança).
    auto_copy: backup::AutoCopy,
    /// Which library this window has open (a recovery library, the one in use).
    library: backup::LibraryPlace,
    /// The password form of the portable copy (export or import).
    portable: backup::PortableForm,
    doc_task_filter: DocTaskFilter,
    doc_priority_filter: DocPriorityFilter,
    doc_origin_filter: DocOriginFilter,
    doc_category_filter: crate::category_ui::CategoryFilter,
    category_manager: crate::category_ui::CategoryManager,
    doc_sort: DocSort,
    /// The document list derived from `documents`, the tasks and the controls
    /// (see `library::DocListView`), and the counter that tells it the
    /// documents were replaced.
    doc_list: Option<library::DocListView>,
    documents_gen: u64,
    /// The library and the search the list in `documents` was read for, and
    /// what the derived part of the last refresh read (see `refresh_polled`).
    documents_for: Option<((i64, i64), String)>,
    derived_for: Option<RefreshKey>,
    manual_form: ManualForm,
    filters_open: bool,
    /// Browser-bridge installation check, refreshed every few seconds while
    /// the welcome screen is visible.
    install_status: Option<(Instant, crate::windows_ui::InstallStatus)>,
}

impl PonteApp {
    fn new(store: Store, db_path: PathBuf) -> Self {
        Self {
            draft_tools: Default::default(),
            templates: Default::default(),
            material: None,
            material_cache: None,
            analysis_material: None,
            review_editor: None,
            review_suggestion: None,
            load_error_for: None,
            proposal_view: None,
            queued: None,
            send_confirm: None,
            ai_test_report: None,
            ai_test_consented: Default::default(),
            mail: Default::default(),
            attachments: Default::default(),
            sources: Default::default(),
            pdf_reader: Default::default(),
            dossier: Default::default(),
            work_context: Default::default(),
            tasks: Default::default(),
            saiop: Default::default(),
            resources: Default::default(),
            projects: Default::default(),
            resources_overdue: 0,
            intranet: Default::default(),
            // The welcome screen (or the opened document) is the first thing seen.
            area: Area::Today,
            reminders: crate::reminders::Reminders::load(),
            // Reads feriados.txt and puts it to work before the first projection.
            holiday_draft: settings::HolidayDraft::load(),
            holiday_migration_revision: None,
            holiday_seed_done: settings::holiday_seed_marker_read(),
            today: Default::default(),
            return_to: None,
            return_after_select: None,
            focus_search: false,
            global_search: Default::default(),
            calendar_exchange: Default::default(),
            repetition: Default::default(),
            update: Default::default(),
            task_badge: 0,
            appearance: load_appearance().0,
            text_size: load_appearance().1,
            settings_section: SettingsSection::Team,
            #[cfg(feature = "shots")]
            tour: crate::shots::Tour::start(),
            #[cfg(feature = "shots")]
            shots_ficha_scroll: 0.0,
            #[cfg(feature = "shots")]
            shots_conference_open: false,
            leave: None,
            leave_error: None,
            consult: None,
            about_open: false,
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
            requests_dir: None,
            requests_checked: Instant::now(),
            requests_watched: false,
            last_trash_purge: None,
            tab: Tab::Overview,
            settings_open: false,
            ai: AiConfig::default(),
            ai_saved: AiConfig::default(),
            models: Vec::new(),
            models_for: None,
            models_error: None,
            model_fetch: None,
            chatgpt_shared: Arc::new(Mutex::new(ChatGptSession::default())),
            chatgpt_login: None,
            chatgpt_login_error: None,
            net_test: None,
            chatgpt_sync_pending: false,
            worker: None,
            suggestion: None,
            suggestion_open: false,
            replacement: None,
            trash: Default::default(),
            exit_confirm: false,
            allow_close: false,
            notice: None,
            backup_path: None,
            auto_copy: Default::default(),
            library: Default::default(),
            portable: Default::default(),
            doc_task_filter: DocTaskFilter::All,
            doc_priority_filter: DocPriorityFilter::All,
            doc_origin_filter: DocOriginFilter::All,
            doc_category_filter: Default::default(),
            category_manager: Default::default(),
            doc_sort: DocSort::Recent,
            doc_list: None,
            documents_gen: 0,
            documents_for: None,
            derived_for: None,
            manual_form: ManualForm::default(),
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

    /// Asks for a `.eml` file and imports it as a manual record with its
    /// attachments, then opens the record.
    fn import_eml_dialog(&mut self) {
        if self.worker.is_some() {
            self.error("Aguarde a operação em andamento antes de importar um e-mail.");
            return;
        }
        let Some(path) = crate::windows_ui::pick_file(
            "Importar e-mail",
            &[
                ("E-mail salvo (.eml)", "*.eml"),
                ("Todos os arquivos", "*.*"),
            ],
        ) else {
            return;
        };
        match self.store.import_eml(&path) {
            Ok(result) => {
                self.refresh();
                self.select(result.receipt.document_id);
                self.tab = Tab::Document;
                let mut message = if result.already_imported && result.attachments_added > 0 {
                    format!(
                        "Este e-mail já estava no acervo (“{}”); {}.",
                        result.subject,
                        if result.attachments_added == 1 {
                            "foi acrescentado o anexo que faltava".to_owned()
                        } else {
                            format!(
                                "foram acrescentados {} anexos que faltavam",
                                result.attachments_added
                            )
                        }
                    )
                } else if result.already_imported {
                    format!(
                        "Este e-mail já estava no acervo (“{}”); nada foi duplicado.",
                        result.subject
                    )
                } else {
                    format!(
                        "E-mail “{}” importado, com {}.",
                        result.subject,
                        plural(result.attachments_added, "anexo", "anexos")
                    )
                };
                for note in result.notes.iter().chain(result.attachments_skipped.iter()) {
                    message.push(' ');
                    message.push_str(note);
                }
                // A note is information; a file that could not be attached,
                // or a second message under the same Message-ID, deserves
                // the warning colour.
                if result.attachments_skipped.is_empty() && !result.separate_record {
                    self.message(message);
                } else {
                    self.error(message);
                }
            }
            Err(error) => self.error(format!("Não foi possível importar o e-mail: {error:#}")),
        }
    }

    /// Shows the user guide (compiled into the program) in the browser.
    fn open_guide(&mut self) {
        match crate::guide::open() {
            Ok(_) => self.message("Guia do usuário aberto no navegador."),
            Err(error) => self.error(format!("Não foi possível abrir o guia: {error:#}")),
        }
    }

    fn message(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: false,
            since: Instant::now(),
            action: None,
        });
    }

    /// A success notice with one button, such as "Desfazer".
    fn message_with_action(&mut self, text: impl Into<String>, label: &str, action: NoticeAction) {
        self.notice = Some(Notice {
            text: text.into(),
            error: false,
            since: Instant::now(),
            action: Some((label.to_owned(), action)),
        });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: true,
            since: Instant::now(),
            action: None,
        });
    }

    /// The button of a notice was pressed.
    fn run_notice_action(&mut self, action: NoticeAction) {
        match action {
            NoticeAction::ReopenTask(id) => match self.tasks.undo_conclude(&mut self.store, id) {
                Ok(()) => {
                    self.refresh();
                    self.message("Tarefa reaberta.");
                }
                Err(error) => self.error(format!("Não foi possível reabrir a tarefa: {error}")),
            },
            NoticeAction::OpenTrash => {
                self.navigate(Leave::Settings);
                if self.area == Area::Settings {
                    self.settings_section = SettingsSection::Trash;
                }
            }
            NoticeAction::Restore(target) => match self.store.restore_item(target.clone()) {
                Ok(document) => {
                    self.apply_trash_change(crate::trash::Change {
                        target,
                        document,
                        restored: true,
                    });
                    self.refresh();
                }
                Err(error) => self.error(format!("Não foi possível restaurar: {error}")),
            },
        }
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
        if self.templates.dirty() {
            out.push(Pending {
                label: "Modelo de trabalho em edição".into(),
                go: PendingTarget::Templates,
            });
        }
        if self.resources.dirty() {
            out.push(Pending {
                label: "Recurso ou uso em edição no painel Recursos".into(),
                go: PendingTarget::Resources,
            });
        }
        if self.mail.dirty() {
            out.push(Pending {
                label: "Conta de e-mail em edição".into(),
                go: PendingTarget::Mail,
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
                label: "Atendimento em edição no painel Atendimentos".into(),
                go: PendingTarget::Saiop,
            });
        }
        if let Some(suggestion) = &self.suggestion {
            out.push(Pending {
                label: format!(
                    "Sugestão de minuta de {} ainda não revisada",
                    suggestion.number
                ),
                go: PendingTarget::Suggestion,
            });
        }
        if self.ai_config_dirty() {
            out.push(Pending {
                label: "Configuração de IA não salva".into(),
                go: PendingTarget::Settings,
            });
        }
        if self.attachments.dirty() {
            out.push(Pending {
                label: "Descrição de anexo não salva (Documento ▸ Anexos)".into(),
                go: self
                    .selected_id
                    .map(|id| PendingTarget::Document(id, Tab::Document))
                    .unwrap_or(PendingTarget::None),
            });
        }
        if self.tasks.attachments_dirty() {
            out.push(Pending {
                label: "Descrição de anexo não salva na tarefa aberta".into(),
                go: PendingTarget::Tasks,
            });
        }
        if self.manual_form.dirty() {
            out.push(Pending {
                label: "Registro manual em edição (janela aberta)".into(),
                go: PendingTarget::None,
            });
        }
        let unsent = self.intranet.unsent_folder_changes(&self.store);
        if unsent > 0 {
            out.push(Pending {
                label: format!(
                    "{} à pasta compartilhada (botão Sincronizar, no rodapé)",
                    if unsent == 1 {
                        "1 alteração ainda não enviada".to_owned()
                    } else {
                        format!("{unsent} alterações ainda não enviadas")
                    }
                ),
                go: PendingTarget::Network,
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
                self.area = Area::Tasks;
            }
            PendingTarget::Saiop => {
                self.area = Area::Saiop;
            }
            PendingTarget::Templates => {
                self.area = Area::Templates;
            }
            PendingTarget::Resources => {
                self.area = Area::Resources;
            }
            PendingTarget::Mail => self.mail.open_full(),
            PendingTarget::Suggestion => self.suggestion_open = true,
            PendingTarget::Settings => self.settings_open = true,
            PendingTarget::Network => self.intranet.open = true,
            PendingTarget::None => {}
        }
    }

    /// Goes somewhere, unless that would hide or replace an unsaved task:
    /// then the question "save, discard or stay?" is shown first and the
    /// move happens after the answer.
    fn navigate(&mut self, leave: Leave) {
        if self.area == Area::Settings && self.settings_section == SettingsSection::Holidays {
            self.apply_holiday_draft();
        }
        // Leaving the AI section of Ajustes saves it; an invalid
        // configuration keeps the person there.
        if self.area == Area::Settings
            && self.settings_section == SettingsSection::Ai
            && !matches!(leave, Leave::Settings)
            && !self.leave_ai_settings()
        {
            self.return_after_select = None;
            return;
        }
        let hides_visible_edit = self.area == Area::Tasks && self.tasks.dirty();
        let replaces_edit = leave.replaces_task() && self.tasks.dirty();
        if hides_visible_edit || replaces_edit {
            self.leave_error = None;
            self.leave = Some(leave);
        } else {
            self.perform_leave(leave);
        }
    }

    fn cancel_pending_leave(&mut self) {
        self.leave = None;
        self.leave_error = None;
        self.return_after_select = None;
        self.area = Area::Tasks;
    }

    fn complete_pending_leave(&mut self) {
        if let Some(leave) = self.leave.take() {
            self.leave_error = None;
            self.perform_leave(leave);
        }
    }

    fn perform_leave(&mut self, leave: Leave) {
        // The way back belongs to the document just opened for the person;
        // going anywhere else ends it.
        if !matches!(leave, Leave::Select(_)) {
            self.return_to = None;
            self.return_after_select = None;
        }
        match leave {
            Leave::Week(ctx) => {
                self.area = Area::Today;
                ctx.data_mut(|data| data.insert_temp(egui::Id::unique("today-week-view"), true));
            }
            Leave::NewManual => {
                self.area = Area::Documents;
                self.manual_form.open_new();
            }
            Leave::NewAttendance => {
                self.area = Area::Saiop;
                self.saiop.key_new(&self.store);
            }
            Leave::NewReservation => {
                self.area = Area::Resources;
                self.resources.new_reservation(&self.store);
            }
            Leave::Projects => {
                self.area = Area::Projects;
            }
            Leave::NewProject => {
                self.area = Area::Projects;
                self.projects.start_new_project();
            }
            Leave::Project(uid) => {
                self.projects.open_project(&uid);
                self.area = Area::Projects;
            }
            Leave::NewProjectTask(uid) => {
                if self.tasks.start_new_in_project(&uid) {
                    self.area = Area::Tasks;
                } else {
                    self.error("Há outra tarefa com alterações não salvas. Salve ou cancele essa edição no painel Tarefas.");
                }
            }
            Leave::Saiop => {
                self.area = Area::Saiop;
            }
            Leave::Templates => {
                self.area = Area::Templates;
            }
            Leave::Resources => {
                self.area = Area::Resources;
            }
            Leave::Settings => self.area = Area::Settings,
            Leave::Today => self.area = Area::Today,
            Leave::Documents => {
                self.area = Area::Documents;
            }
            Leave::Select(id) => self.select(id),
            Leave::NewTask(document_id) => {
                if self.tasks.start_new(document_id) {
                    self.area = Area::Tasks;
                } else {
                    self.error("Há outra tarefa com alterações não salvas. Salve ou cancele essa edição no painel Tarefas.");
                }
            }
            Leave::NewTaskCaptured { title, due_date } => {
                if self.tasks.start_new_captured(&title, &due_date) {
                    self.area = Area::Tasks;
                } else {
                    self.error("Há outra tarefa com alterações não salvas. Salve ou cancele essa edição no painel Tarefas.");
                }
            }
            Leave::OpenTask(task) => {
                if self.tasks.open_task(&task, &self.store) {
                    self.area = Area::Tasks;
                } else {
                    self.error("Há outra tarefa com alterações não salvas. Salve ou cancele essa edição no painel Tarefas.");
                }
            }
            Leave::EditTask(task) => {
                if self.tasks.focus_task(&task) {
                    self.area = Area::Tasks;
                } else {
                    self.error("Há outra tarefa com alterações não salvas. Salve ou cancele essa edição no painel Tarefas.");
                }
            }
            Leave::Suggestion {
                document_id,
                suggestion,
                priority,
            } => {
                if self
                    .tasks
                    .prepare_suggestion(document_id, &suggestion, &priority)
                {
                    self.area = Area::Tasks;
                } else {
                    self.error("Salve ou cancele a tarefa em edição antes de preparar outra.");
                }
            }
            Leave::TaskProposal {
                document_id,
                proposal,
            } => {
                if self.tasks.prepare_proposal(document_id, &proposal) {
                    self.area = Area::Tasks;
                } else {
                    self.error("Salve ou cancele a tarefa em edição antes de preparar outra.");
                }
            }
        }
    }

    /// Color, short label and full description of the team/network state.
    fn network_status(&self) -> (Color32, String, String) {
        let full = self.intranet.banner_text(&self.store);
        let Ok(cfg) = self.store.load_intranet_config() else {
            return (
                Color32::from_rgb(140, 150, 160),
                "Só neste computador".into(),
                full,
            );
        };
        let conflicts = self
            .store
            .list_sync_conflicts()
            .map(|v| v.len())
            .unwrap_or(0);
        let outbox = self
            .store
            .list_offline_outbox()
            .map(|v| v.len())
            .unwrap_or(0);
        let full = format!("{full}\n\nClique para abrir Ajustes ▸ Equipe e sincronização.");
        if conflicts > 0 {
            return (
                crate::theme::error(),
                format!(
                    "⚠ {}",
                    plural(conflicts, "conflito de edição", "conflitos de edição")
                ),
                format!("{full}\nDecida campo a campo."),
            );
        }
        // Short texts live in the status bar, so they skip the mode's name:
        // the hover sentence and the window say it.
        let since = |at: Instant| {
            let secs = at.elapsed().as_secs();
            if secs < 60 {
                "agora".to_owned()
            } else {
                format!("há {} min", secs / 60)
            }
        };
        let pending = |text: &str| {
            if outbox > 0 {
                format!("{text} • {outbox} a enviar")
            } else {
                text.to_owned()
            }
        };
        match cfg.mode {
            crate::intranet::IntranetMode::Local => (
                Color32::from_rgb(140, 150, 160),
                "só neste computador".into(),
                full,
            ),
            crate::intranet::IntranetMode::Host => {
                if self.intranet.host_running() {
                    (
                        crate::theme::success(),
                        "computador principal ativo".into(),
                        full,
                    )
                } else {
                    (
                        crate::theme::warning(),
                        "computador principal parado".into(),
                        full,
                    )
                }
            }
            crate::intranet::IntranetMode::SharedFolder => {
                if self.intranet.syncing() {
                    (
                        crate::theme::accent(),
                        "sincronizando com a pasta…".into(),
                        full,
                    )
                } else if self.intranet.offline {
                    (
                        crate::theme::warning(),
                        pending("pasta fora de alcance"),
                        full,
                    )
                } else if outbox > 0 {
                    (crate::theme::warning(), format!("{outbox} a enviar"), full)
                } else if let Some((at, _)) = self.intranet.last_sync_finished {
                    (
                        crate::theme::success(),
                        format!("em dia {}", since(at)),
                        full,
                    )
                } else {
                    (
                        crate::theme::accent(),
                        "pasta ainda não sincronizada".into(),
                        full,
                    )
                }
            }
            crate::intranet::IntranetMode::Client => {
                if self.intranet.offline {
                    (
                        crate::theme::warning(),
                        pending("sem conexão com o principal"),
                        full,
                    )
                } else if let Some((at, _)) = self.intranet.last_sync_finished {
                    (
                        crate::theme::success(),
                        format!("conectado • atualizado {}", since(at)),
                        full,
                    )
                } else {
                    (
                        crate::theme::accent(),
                        "conectando ao principal…".into(),
                        full,
                    )
                }
            }
        }
    }

    /// Replaces the documents of the list; what is derived from them (the
    /// list as drawn) is rebuilt on its own.
    fn set_documents(&mut self, documents: Vec<DocumentSummary>) {
        self.documents = documents;
        self.documents_gen = self.documents_gen.wrapping_add(1);
    }

    /// Brings the window up to date with the library, whatever changed: the
    /// synchronization step, the list of documents and everything derived
    /// from the library (Hoje, the badges, the open document). Called after
    /// an action of the person; the periodic poll is `refresh_polled`.
    fn refresh(&mut self) {
        self.poll_sync();
        self.rebuild(true);
    }

    /// The poll of every few seconds: the synchronization step always runs,
    /// the rest only when something it reads changed since the last time (the
    /// library, the day, the minute for the returns of resources, the late
    /// threshold of the follow-ups, the holiday calendar).
    fn refresh_polled(&mut self) {
        self.poll_sync();
        self.rebuild(false);
    }

    fn poll_sync(&mut self) {
        self.last_refresh = Instant::now();
        self.intranet.refresh_client(&mut self.store, &self.db_path);
        // The holidays are the library's: what another computer saved arrives
        // with the synchronization, before the projections below are built.
        self.sync_holidays_with_library();
    }

    /// What the derived part of a refresh reads besides the open document.
    fn refresh_key(&self, stamp: Option<(i64, i64)>) -> RefreshKey {
        RefreshKey {
            stamp,
            day: chrono::Local::now().format("%Y-%m-%d").to_string(),
            // The returns of resources are late by the minute.
            minute: crate::resources::now_moment(),
            stale_days: self.saiop.stale_days(),
            holidays: crate::holidays::config(),
            selected: self.selected_id,
        }
    }

    /// Reads the list of documents and rebuilds what derives from the
    /// library; unless `force`, each only when what it reads changed since it
    /// was last done.
    fn rebuild(&mut self, force: bool) {
        // Read first: a commit that lands while rebuilding shows as a change
        // at the next poll.
        let stamp = self.store.library_stamp();
        self.refresh_documents_if_changed(stamp, force);
        let key = self.refresh_key(stamp);
        if !force && stamp.is_some() && self.derived_for.as_ref() == Some(&key) {
            return;
        }
        let today = key.day.clone();
        self.today = today::TodayData::load_with(&self.store, &today, self.saiop.stale_days());
        self.task_badge = self.today.pending_tasks();
        let mut complete = self.today.error.is_none();
        match self.overdue_returns() {
            Some(count) => self.resources_overdue = count,
            None => {
                self.resources_overdue = 0;
                complete = false;
            }
        }
        if let Some(id) = self.selected_id {
            complete &= self.reload(id);
        }
        // Remembered only once every read worked: a section that failed is
        // read again at the next poll, not served from the cache until the
        // day, the minute or the library change.
        self.derived_for = complete.then_some(key);
    }

    fn refresh_documents_if_changed(&mut self, stamp: Option<(i64, i64)>, force: bool) {
        let loaded_for = stamp.map(|stamp| (stamp, self.query.clone()));
        if !force && loaded_for.is_some() && self.documents_for == loaded_for {
            return;
        }
        match self.store.list_documents(&self.query) {
            Ok(documents) => {
                self.set_documents(documents);
                self.documents_for = loaded_for;
            }
            Err(error) => self.error(format!(
                "Não foi possível atualizar a lista de documentos: {error}"
            )),
        }
    }

    /// The search text changed: only the list of documents is read again
    /// (and the pieces joined to the open one, which depend on it). Nothing
    /// else a refresh builds depends on the search.
    fn refresh_documents_for_search(&mut self) {
        let stamp = self.store.library_stamp();
        self.refresh_documents_if_changed(stamp, false);
        if let Some(id) = self.selected_id {
            self.reload_joined_children(id);
        }
    }

    /// The documents juntados to `id`, for its page. An unfiltered library
    /// list already covers every document.
    fn joined_children_of(&self, id: i64) -> Result<Vec<DocumentSummary>> {
        if self.query.trim().is_empty() {
            Ok(self
                .documents
                .iter()
                .filter(|d| d.joined_to_id == Some(id))
                .cloned()
                .collect())
        } else {
            self.store.joined_children(id)
        }
    }

    fn reload_joined_children(&mut self, id: i64) {
        match self.joined_children_of(id) {
            Ok(children) => self.joined_children = children,
            Err(error) => {
                self.joined_children.clear();
                self.error(format!(
                    "Não foi possível ler os documentos juntados: {error}"
                ));
            }
        }
        if let Some(detail) = &mut self.detail {
            if detail.summary.id == id {
                detail.summary.joined_children_count = self.joined_children.len();
            }
        }
    }

    /// Returns past their expected end, for the badge of "Recursos"; `None`
    /// when the uses could not be read.
    fn overdue_returns(&self) -> Option<usize> {
        let now = crate::resources::now_moment();
        let today = crate::resources::parse_moment(&now)
            .map(|moment| moment.date())
            .unwrap_or_else(|_| chrono::Local::now().date_naive());
        self.store
            .list_resource_uses()
            .map(|uses| crate::resources::day_counts(&uses, today, &now).overdue)
            .ok()
    }

    /// Reads the document again into the editor and the page; `false` when it
    /// could not be read.
    fn reload(&mut self, id: i64) -> bool {
        let note_version = self
            .store
            .item_version("note", id)
            .ok()
            .map(|meta| meta.version);
        match self.store.load_document(id) {
            Ok(detail) => {
                if self.load_error_for == Some(id) {
                    self.load_error_for = None;
                }
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
                    let children = self.joined_children_of(id);
                    match children {
                        Ok(children) => self.joined_children = children,
                        Err(error) => {
                            self.joined_children.clear();
                            self.error(format!(
                                "Não foi possível ler os documentos juntados: {error}"
                            ));
                        }
                    }
                    detail.summary.joined_children_count = self.joined_children.len();
                    self.detail = Some(detail);
                    self.refresh_proposal_view();
                    match self.store.similar_documents(id, 5) {
                        Ok(items) => self.similar = items,
                        Err(error) => self.error(format!(
                            "Não foi possível buscar documentos parecidos: {error}"
                        )),
                    }
                }
                true
            }
            Err(error) => {
                self.document_unavailable(id, &error.to_string());
                false
            }
        }
    }

    /// The open document could not be read. When it is gone or in the
    /// Lixeira the selection is cleared, the list refreshed and the person
    /// told once, in plain words (the periodic refresh no longer asks for
    /// it); any other failure keeps an error with the technical text as a
    /// second, muted line.
    fn document_unavailable(&mut self, id: i64, technical: &str) {
        let state = self.store.document_state(id).ok();
        let failure = queue::load_failure(state.as_ref());
        let (text, action) = queue::load_failure_text(failure);
        if failure == queue::LoadFailure::Other {
            // Said once: the periodic refresh does not repeat it until the
            // document has been read again.
            if self.load_error_for != Some(id) {
                self.load_error_for = Some(id);
                // The technical text goes on a second line of the same notice.
                self.error(format!("{text}\n{technical}"));
            }
            return;
        }
        if self.selected_id == Some(id) {
            self.selected_id = None;
            self.detail = None;
            self.proposal_view = None;
            self.similar.clear();
            self.joined_children.clear();
            self.return_to = None;
        }
        if let Ok(documents) = self.store.list_documents(&self.query) {
            self.set_documents(documents);
        }
        match action {
            Some(label) => self.message_with_action(text, label, NoticeAction::OpenTrash),
            None => self.message(text),
        }
    }

    fn select(&mut self, id: i64) {
        self.return_to = self.return_after_select.take();
        self.area = Area::Documents;
        self.selected_id = Some(id);
        self.detail = None;
        self.proposal_view = None;
        self.similar.clear();
        self.joined_children.clear();
        self.reload(id);
    }

    fn settings(&self) -> Result<AiSettings> {
        anyhow::ensure!(
            !self.ai.model().trim().is_empty(),
            "Escolha o modelo da IA em Ajustes ▸ Inteligência artificial."
        );
        Ok(self.with_chatgpt_session(self.ai.chat_settings()?))
    }

    /// Hands the shared ChatGPT session to settings that will use it.
    fn with_chatgpt_session(&self, mut settings: AiSettings) -> AiSettings {
        if settings.kind == ApiKind::ChatGpt {
            settings.chatgpt = Some(self.chatgpt_shared.clone());
        }
        settings
    }

    /// Replaces the ChatGPT session everywhere it lives.
    fn set_chatgpt_session(&mut self, session: ChatGptSession) {
        if let Ok(mut shared) = self.chatgpt_shared.lock() {
            *shared = session.clone();
        }
        self.ai.chatgpt = session;
    }

    /// After a worker used the session: a refreshed token (rotating refresh
    /// token included) must be the one saved, without touching other edits
    /// the person may have pending in the configuration window.
    fn sync_chatgpt_session(&mut self) {
        let Ok(shared) = self.chatgpt_shared.lock().map(|s| s.clone()) else {
            return;
        };
        if shared == self.ai.chatgpt {
            return;
        }
        let was_signed_in = self.ai.chatgpt.signed_in();
        self.ai.chatgpt = shared.clone();
        let mut to_save = self.ai_saved.clone();
        to_save.chatgpt = shared.clone();
        if self.store.save_ai_config(&to_save).is_ok() {
            self.ai_saved = to_save;
        }
        if was_signed_in && !shared.signed_in() {
            self.error(
                "A sessão do ChatGPT foi encerrada. Entre de novo em Ajustes ▸ Inteligência artificial.",
            );
        }
    }

    /// Reads the saved configuration once, at start. Unreadable content
    /// (another Windows account) keeps the defaults and says so.
    fn load_ai_config(&mut self) {
        match self.store.load_ai_config() {
            Ok(config) => {
                self.ai = config.clone();
                self.ai_saved = config;
                let session = self.ai.chatgpt.clone();
                self.set_chatgpt_session(session);
            }
            Err(error) => self.error(format!(
                "Não foi possível carregar a configuração da IA: {error}"
            )),
        }
    }

    fn poll_chatgpt_login(&mut self, ctx: &egui::Context) {
        let received = self
            .chatgpt_login
            .as_ref()
            .map(|login| login.receiver.try_recv());
        match received {
            Some(Ok((Ok(session), _))) => {
                self.chatgpt_login = None;
                let label = session.account_label.clone();
                let sharing = session.sharing();
                self.set_chatgpt_session(session);
                self.ai.provider = AiProvider::ChatGpt;
                // The session is saved at once: the refresh token rotates and
                // must not be lost with an unsaved window.
                if self.save_ai_config() {
                    self.message(if sharing {
                        format!("Conectado ao ChatGPT como {label}. Escolha o modelo e salve.")
                    } else {
                        format!("Conectado ao ChatGPT como {label}, mas sem permissão de uso do plano. Clique em “Entrar de novo…” e marque a permissão.")
                    });
                }
                self.models_for = None;
                self.fetch_models(ctx);
            }
            Some(Ok((Err(error), issued))) => {
                self.chatgpt_login = None;
                // The registration of this computer was made even though the
                // sign-in did not finish: keep its client id, or the next try
                // would register again and OpenAI would refuse it.
                let kept = match issued {
                    Some(client_id) if self.ai.chatgpt.client_id.trim().is_empty() => {
                        let mut session = self.ai.chatgpt.clone();
                        session.client_id = client_id;
                        self.set_chatgpt_session(session);
                        self.save_ai_config()
                    }
                    _ => false,
                };
                self.chatgpt_login_error = Some(if kept {
                    format!("{error} O registro deste computador foi guardado: a próxima tentativa continua dele.")
                } else {
                    error
                });
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.chatgpt_login = None;
                self.chatgpt_login_error = Some("O login não foi concluído. Tente de novo.".into());
            }
            _ => {}
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
                self.error(format!(
                    "Não foi possível salvar a configuração da IA: {error}"
                ));
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
            Ok(settings) => self.with_chatgpt_session(settings),
            Err(error) => {
                self.models_error = Some(error.to_string());
                return;
            }
        };
        if settings.kind == ApiKind::ChatGpt {
            self.chatgpt_sync_pending = true;
        }
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
                self.models_error = Some(format!("Não foi possível listar os modelos: {error}"))
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
                        AiProvider::ChatGpt => "Sua conta do ChatGPT não ofereceu nenhum modelo para uso por aplicativos.".into(),
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
                self.models_error =
                    Some("Não houve resposta ao listar os modelos. Tente de novo.".into());
            }
            _ => {}
        }
    }

    fn spawn(
        &mut self,
        label: String,
        kind: WorkKind,
        task: impl FnOnce(ai::Reporter) -> WorkResult + Send + 'static,
        ctx: &egui::Context,
    ) -> bool {
        if self.worker.is_some() {
            // Never silent: the person is told why nothing started. AI jobs
            // wait in the queue before they get here (`run_ai_job`).
            self.error(
                "Há outro trabalho em andamento. Espere terminar, ou cancele o atual, e tente de novo.",
            );
            return false;
        }
        if self.ai.provider == AiProvider::ChatGpt {
            self.chatgpt_sync_pending = true;
        }
        let (sender, receiver) = mpsc::channel();
        let (progress_sender, progress) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let reporter = ai::Reporter::new(progress_sender, cancel.clone());
        let repaint = ctx.clone();
        match std::thread::Builder::new()
            .name("ponte-worker".into())
            .spawn(move || {
                let result = task(reporter);
                let _ = sender.send(result);
                repaint.request_repaint();
            }) {
            Ok(_) => {
                self.worker = Some(Worker {
                    receiver,
                    label,
                    kind,
                    started: Instant::now(),
                    progress,
                    latest: String::new(),
                    cancel,
                });
                true
            }
            Err(error) => {
                self.error(format!("Não foi possível iniciar a operação: {error}"));
                false
            }
        }
    }

    /// Asks the running AI action to stop at its next check; the thread
    /// ends by itself and the result is reported as cancelled.
    fn cancel_worker(&mut self) {
        if let Some(worker) = &self.worker {
            worker.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn prepare_sources(&mut self, document_id: i64, documents: Vec<i64>, ctx: &egui::Context) {
        let path = self.db_path.clone();
        self.spawn(
            "Preparando as fontes neste computador…".into(),
            WorkKind::Sources,
            move |_| WorkResult::Sources {
                document_id,
                result: Store::open_service(&path)
                    .and_then(|store| store.prepare_sources(&documents))
                    .map_err(|e| e.to_string()),
            },
            ctx,
        );
    }
    /// "Analisar": asks first when the document would leave this computer for
    /// the first time (once per document and destination), then starts.
    fn analyze(&mut self, detail: &DocumentDetail, ctx: &egui::Context) {
        match self.send_plan(
            queue::PendingSend::Analysis,
            detail,
            ai::DraftSends::default(),
        ) {
            queue::SendPlan::Go => self.analyze_now(detail, ctx),
            queue::SendPlan::Ask(question) => self.send_confirm = Some(*question),
            queue::SendPlan::Stop => {}
        }
    }

    fn analyze_now(&mut self, detail: &DocumentDetail, ctx: &egui::Context) {
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
                self.error(format!("Não foi possível ler o contexto: {e}"));
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
        let number = detail.summary.document_number.clone();
        let context = human_context.clone();
        let consent = self.consent_of(&settings, document_id, &sources);
        // The model is only told WHETHER the document already has an
        // atendimento, never what is in it.
        let extras = ai::AnalyzeExtras {
            has_attendance: self
                .store
                .list_saiop_attendances()
                .map(|all| all.iter().any(|a| a.document_id == Some(document_id)))
                .unwrap_or(false),
            ..Default::default()
        };
        self.run_ai_job(queue::QueuedJob {
            key: queue::QueueKey {
                kind: WorkKind::Analysis,
                document_id,
            },
            label: format!("Analisando {number}…"),
            waiting: format!("analisar {number}"),
            capture_id,
            context,
            consent,
            draft_guard: None,
            task: Box::new(move |reporter| WorkResult::Analysis {
                document_id,
                result: ai::analyze_document(
                    &settings,
                    &capture,
                    capture_id,
                    &human_context,
                    &sources,
                    extras,
                    &reporter,
                )
                .map_err(|e| e.to_string()),
            }),
            ctx: ctx.clone(),
        });
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
            self.message(format!(
                "Salvos antes de acionar a IA: {}.",
                saved.join(" e ")
            ));
            self.reload(id);
        }
        true
    }

    /// Asks the AI for a draft with the chosen intent: the editor's template,
    /// ticked blocks and request form the trusted layer; the document, notes,
    /// sources and approved examples are data.
    fn generate(
        &mut self,
        detail: &DocumentDetail,
        intent: crate::work_templates::Intent,
        ctx: &egui::Context,
    ) {
        if self.ai_leaves() {
            let Some(job) = self.draft_job(detail, intent) else {
                return;
            };
            match self.send_plan(queue::PendingSend::Draft(intent), detail, job.sends()) {
                queue::SendPlan::Go => {}
                queue::SendPlan::Ask(question) => {
                    self.send_confirm = Some(*question);
                    return;
                }
                queue::SendPlan::Stop => return,
            }
        }
        self.generate_now(detail, intent, ctx);
    }

    /// What a draft would send: the job as `generate_now` would build it
    /// (without saving anything first), `None` with the reason told when it
    /// cannot be built.
    fn draft_job(
        &mut self,
        detail: &DocumentDetail,
        intent: crate::work_templates::Intent,
    ) -> Option<ai::DraftJob> {
        let sources = match self.sources.selected(detail.summary.id, &self.store) {
            Ok(value) => value,
            Err(e) => {
                self.error(e.to_string());
                return None;
            }
        };
        let document_id = detail.summary.id;
        let (kind, note, request_guidance, selected_blocks, current_text, template_uid) = {
            let editor = &self.editors[&document_id];
            (
                editor.kind.clone(),
                editor.note.clone(),
                editor.instructions.clone(),
                editor.selected_blocks.clone(),
                editor.body.clone(),
                editor.template_uid.clone(),
            )
        };
        let local_context = match self.store.load_work_context(document_id) {
            Ok(value) => value.analysis_text(&note),
            Err(e) => {
                self.error(format!("Não foi possível ler o contexto: {e}"));
                return None;
            }
        };
        let member = self
            .store
            .load_intranet_config()
            .map(|c| c.member_name)
            .unwrap_or_default();
        let template = match &template_uid {
            Some(uid) => match self.store.template_by_uid(uid) {
                Ok(Some(found)) if found.offered_for(&kind, &member) => {
                    let version = found
                        .published_version()
                        .cloned()
                        .expect("offered templates have a published version");
                    Some((found, version))
                }
                Ok(Some(_)) => {
                    self.error("O modelo escolhido não está publicado para este tipo de minuta. Escolha outro ou publique-o na área Modelos.");
                    return None;
                }
                Ok(None) => {
                    self.error("O modelo escolhido não existe mais.");
                    return None;
                }
                Err(error) => {
                    self.error(format!("Não foi possível ler o modelo: {error}"));
                    return None;
                }
            },
            None => None,
        };
        // Only approved examples travel: the ones most like this document,
        // at most three and within the size limit (newest first when none
        // resembles it).
        let (blocks, examples) = match &template {
            Some((found, _)) => (
                self.store.list_blocks().unwrap_or_default(),
                crate::prompting::fit_examples(
                    self.store
                        .rank_examples(&found.uid, &detail.capture)
                        .unwrap_or_default(),
                ),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let capture_id = detail.summary.latest_capture_id;
        let mut example_files = Vec::new();
        for example in &examples {
            example_files.extend(self.store.example_files(example).unwrap_or_default());
        }
        Some(ai::DraftJob {
            document_id,
            capture_id,
            capture: detail.capture.clone(),
            kind: kind.clone(),
            intent,
            local_context: format!(
                "Contexto local e notas do usuário (conferir na fonte):\n{local_context}"
            ),
            request_guidance,
            template,
            blocks,
            selected_blocks,
            examples,
            example_files,
            sources: sources.clone(),
            current_text: current_text.clone(),
            verified_facts: self.store.verified_facts_for(document_id, capture_id),
        })
    }

    fn generate_now(
        &mut self,
        detail: &DocumentDetail,
        intent: crate::work_templates::Intent,
        ctx: &egui::Context,
    ) {
        if !self.save_pending_inputs(detail.summary.id) {
            return;
        }
        let settings = match self.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.error(error.to_string());
                self.open_ai_settings(ctx);
                return;
            }
        };
        let Some(job) = self.draft_job(detail, intent) else {
            return;
        };
        let document_id = job.document_id;
        let capture_id = job.capture_id;
        let kind = job.kind.clone();
        let current_text = job.current_text.clone();
        let sources = job.sources.clone();
        let consent = self.consent_of(&settings, document_id, &sources);
        let number = detail.summary.document_number.clone();
        let context = queue::note_and_context(&self.store, &self.editors, document_id);
        self.run_ai_job(queue::QueuedJob {
            key: queue::QueueKey {
                kind: WorkKind::Draft,
                document_id,
            },
            label: format!("Gerando minuta de {number}…"),
            waiting: format!("gerar minuta de {number}"),
            capture_id,
            context: context.unwrap_or_default(),
            consent,
            draft_guard: None,
            task: Box::new(move |reporter| {
                let outcome = ai::generate_draft(&settings, &job, &reporter);
                WorkResult::Draft {
                    conference: outcome.conference,
                    conference_skipped: outcome.conference_skipped,
                    materials: sources,
                    document_id,
                    number,
                    capture_id,
                    kind,
                    intent,
                    previous_text: current_text,
                    record: outcome.record,
                    result: outcome.result,
                }
            }),
            ctx: ctx.clone(),
        });
    }

    fn backup(&mut self, ctx: &egui::Context) {
        if self.copy_job_blocked() {
            return;
        }
        let db_path = self.db_path.clone();
        let data_dir = self.data_dir();
        self.spawn(
            "Criando a cópia de segurança protegida…".into(),
            WorkKind::Backup,
            move |_| {
                // Verified like the automatic and the installers' copies, from
                // a read-only connection.
                let result = crate::auto_backup::manual_copy(&db_path, &data_dir);
                WorkResult::Backup(result.map_err(|e| format!("{e:#}")))
            },
            ctx,
        );
    }

    /// Writes the CSV files in the background and opens the folder.
    fn export_csv(&mut self, ctx: &egui::Context) {
        let db_path = self.db_path.clone();
        self.spawn(
            "Exportando dados para CSV…".into(),
            WorkKind::Export,
            move |_| {
                let result = Store::open_service(&db_path)
                    .and_then(|store| crate::export::export_all(&store))
                    .map_err(|e| e.to_string());
                WorkResult::Export(result)
            },
            ctx,
        );
    }

    fn poll_worker(&mut self) {
        if let Some(worker) = &mut self.worker {
            // Keep only the latest step; the channel may hold several.
            if let Some(step) = worker.progress.try_iter().last() {
                worker.latest = step;
            }
        }
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
        let cancelled = self.worker.as_ref().is_some_and(Worker::cancelling);
        self.worker = None;
        if cancelled {
            // The task stopped at the person's request: a plain notice, not an error.
            let what = match &result {
                WorkResult::Analysis { .. } => "Análise cancelada; nada foi salvo.",
                WorkResult::Draft { .. } => "Geração da minuta cancelada; nada foi alterado.",
                WorkResult::AiTest(_) => "Teste da IA cancelado.",
                WorkResult::Conference { .. } => "Conferência da minuta cancelada.",
                _ => "Operação cancelada.",
            };
            let finished_anyway = match &result {
                WorkResult::Analysis { result, .. } => result.is_ok(),
                WorkResult::Draft { result, .. } => result.is_ok(),
                _ => false,
            };
            if !finished_anyway {
                self.message(what);
                return;
            }
            // It completed before noticing the cancellation: keep the result.
        }
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
                        self.message("Análise salva e vinculada à importação usada. Confira as conclusões na fonte.");
                        self.refresh();
                    }
                    Err(error) => self.error(format!(
                        "A IA respondeu, mas a análise não pôde ser salva: {error}"
                    )),
                },
                Err(error) => self.error(format!("Não foi possível concluir a análise: {error}")),
            },
            WorkResult::Draft {
                materials,
                document_id,
                number,
                capture_id,
                kind,
                intent,
                previous_text,
                record,
                result,
                conference,
                conference_skipped,
            } => {
                // The conference made with the generation is kept for this
                // exact text (a cache of the AI's opinion, local only).
                if let Some(conference) = &conference {
                    if let Err(error) = self.store.save_draft_conference(document_id, conference) {
                        self.error(format!("Não foi possível guardar a conferência: {error}"));
                    }
                    self.review_editor = None;
                    self.review_suggestion = None;
                }
                // Every generation is recorded, accepted or refused.
                if let Err(error) = self.store.record_generation(&record) {
                    self.error(format!(
                        "Não foi possível registrar esta geração no histórico: {error}"
                    ));
                }
                match result {
                    Ok(body) => {
                        let provenance = crate::model::DraftProvenance {
                            generation_uid: Some(record.uid.clone()),
                            template_uid: record.template_uid.clone(),
                            template_version: record.template_version,
                            template_name: record.template_name.clone(),
                            model: record.model.clone(),
                            provider: record.provider.clone(),
                            request_guidance: record.request_guidance.clone(),
                        };
                        self.suggestion = Some(Suggestion {
                            materials,
                            document_id,
                            number,
                            capture_id,
                            kind,
                            body,
                            intent,
                            previous_text,
                            provenance: Some(provenance),
                            conference_skipped,
                        });
                        self.suggestion_open = true;
                        self.message("Sugestão pronta para revisão. Salve uma versão quando o texto estiver adequado.");
                    }
                    Err(error) => self.error(format!("Não foi possível gerar a minuta: {error}")),
                }
            }
            WorkResult::Backup(result) => match result {
                Ok((path, None)) => {
                    self.message(format!("Cópia de segurança concluída: {}", path.display()));
                    self.backup_path = Some(path);
                }
                Ok((path, Some(problem))) => {
                    self.error(format!(
                        "A cópia foi guardada em {}, mas não conta como cópia boa: {problem} Uma restauração dela seria recusada.",
                        path.display()
                    ));
                    self.backup_path = Some(path);
                }
                Err(error) => self.error(format!(
                    "Não foi possível fazer a cópia de segurança: {error}"
                )),
            },
            WorkResult::MailFolders(result) => self.mail.accept_folders(result),
            WorkResult::MailMessages(result) => {
                self.mail.accept_messages(result);
                self.mail.remember_choice(&mut self.store);
            }
            WorkResult::MailFetched(result, source) => match result {
                Ok(fetched) => {
                    let mut outcomes = Vec::with_capacity(fetched.len());
                    let mut last_document = None;
                    for (uid, bytes) in fetched {
                        let outcome = bytes.and_then(|bytes| {
                            self.store
                                .import_mail_bytes(&bytes, &source)
                                .map_err(|e| e.to_string())
                        });
                        if let Ok(import) = &outcome {
                            last_document = Some(import.receipt.document_id);
                        }
                        outcomes.push((uid, outcome));
                    }
                    let imported = outcomes.iter().filter(|(_, o)| o.is_ok()).count();
                    self.mail.accept_imports(outcomes);
                    if imported > 0 {
                        self.refresh();
                        // From Ajustes ▸ E-mail the person stays there, where the
                        // result of each message is listed; from the quick window
                        // the imported document opens.
                        if self.area != Area::Settings {
                            if let Some(id) = last_document {
                                // Through the guard: an unsaved task is never hidden.
                                self.navigate(Leave::Select(id));
                            }
                        }
                        self.message(if imported == 1 {
                            "1 mensagem importada como e-mail.".to_owned()
                        } else {
                            format!("{imported} mensagens importadas como e-mails.")
                        });
                    }
                }
                Err(error) => self.mail.accept_fetch_error(error),
            },
            WorkResult::Conference {
                document_id,
                result,
            } => match result {
                Ok(conference) => {
                    match self.store.save_draft_conference(document_id, &conference) {
                        Ok(()) => {
                            self.review_editor = None;
                            self.review_suggestion = None;
                            self.message(
                                "Conferência pronta. É a opinião da IA: confira os trechos.",
                            );
                        }
                        Err(error) => {
                            self.error(format!("Não foi possível guardar a conferência: {error}"));
                        }
                    }
                }
                Err(error) => self.error(format!("Não foi possível conferir a minuta: {error}")),
            },
            WorkResult::AiTest(result) => {
                match &result {
                    Ok(_) => self.message("Teste da IA concluído. O resultado está em Ajustes ▸ Inteligência artificial."),
                    Err(error) => self.error(format!("Não foi possível testar a IA: {error}")),
                }
                self.ai_test_report = Some(result);
            }
            WorkResult::Recovery(result) => self.accept_recovery(result),
            WorkResult::PortableExport(result) => self.accept_portable_export(result),
            WorkResult::Export(result) => match result {
                Ok(report) => {
                    self.message(format!(
                        "Exportação concluída: {}, {}, {}, {}, {} e {}, em {}. No CERES, envie cada arquivo .csv como um conjunto.",
                        plural(report.documents, "documento", "documentos"),
                        plural(report.tasks, "tarefa", "tarefas"),
                        plural(report.projects, "projeto", "projetos"),
                        plural(report.attendances, "atendimento", "atendimentos"),
                        plural(report.resources, "recurso", "recursos"),
                        plural(report.resource_uses, "uso de recurso", "usos de recursos"),
                        report.folder.display()
                    ));
                    crate::windows_ui::reveal_in_explorer(&report.folder);
                }
                Err(error) => self.error(format!("Não foi possível exportar os dados: {error}")),
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
            editor.provenance = replacement.provenance;
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
            Err(error) => self.error(format!("Não foi possível salvar as notas: {error}")),
        }
    }

    /// "Desfazer alterações": drops the unsaved text and shows the note as
    /// the library has it now, with its current version (another connection
    /// may have saved it since this editor was loaded).
    fn discard_note(&mut self, id: i64) {
        if let Some(editor) = self.editors.get_mut(&id) {
            editor.note = editor.saved_note.clone();
        }
        self.reload_note_if_clean(id);
    }

    /// The note editor holds no unsaved change: takes the text and the version
    /// from the library, so a later save checks against what is really there.
    fn reload_note_if_clean(&mut self, id: i64) {
        if self.editors.get(&id).is_some_and(|e| !e.note_dirty()) {
            self.reload(id);
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
        match self.store.save_draft_full(
            id,
            editor.capture_id,
            &editor.kind,
            &editor.body,
            &editor.materials,
            editor.provenance.as_ref(),
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
            Err(error) => self.error(format!("Não foi possível salvar a minuta: {error}")),
        }
    }

    /// The rail on the left: one item per area, settings and help at the bottom.
    fn rail(&mut self, ui: &mut egui::Ui) {
        use crate::theme::icon;
        use crate::widgets::rail_item;
        let p = crate::theme::c();
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("PONTE")
                    .font(crate::theme::font_strong(13.5))
                    .extra_letter_spacing(1.6)
                    .color(p.rail_on),
            );
            ui.add_space(6.0);
        });
        egui::Panel::bottom("rail-bottom")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.vertical_centered(|ui| {
                    ui.add_space(2.0);
                if rail_item(
                    ui,
                    icon::SETTINGS,
                    "Ajustes",
                    self.area == Area::Settings,
                    0,
                    false,
                )
                .on_hover_text("Equipe e sincronização, e-mail, IA, cópia de segurança, lixeira e aparência.")
                .clicked()
                {
                    self.navigate(Leave::Settings);
                }
                if rail_item(ui, icon::HELP, "Ajuda", false, 0, false)
                    .on_hover_text("Abre o guia do usuário no navegador. Funciona sem internet.")
                    .clicked()
                {
                    self.open_guide();
                }
                });
            });
        // The areas scroll when the window is too short for all of them;
        // Ajustes and Ajuda stay in place under them.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.vertical_centered(|ui| {
                    if rail_item(ui, icon::SEARCH, "Buscar", self.global_search.open, 0, false)
                        .on_hover_text("Busca em todo o acervo: documentos, tarefas, atendimentos e recursos. (Ctrl+K)")
                        .clicked()
                    {
                        self.global_search.open = true;
                    }
                    // Late returns of resources count like attendances with a
                    // due deadline; today's reservations do not (they are not late).
                    let today_badge = self.today.pending_tasks()
                        + self.today.attendances.len()
                        + self.today.overdue_uses.len();
                    if rail_item(
                        ui,
                        icon::TODAY,
                        "Hoje",
                        self.area == Area::Today,
                        today_badge,
                        false,
                    )
                    .on_hover_text("O que pede atenção agora: tarefas atrasadas e de hoje, atendimentos com prazo vencido, devoluções de recursos atrasadas e reservas de hoje. (Ctrl+1)")
                    .clicked()
                    {
                        self.navigate(Leave::Today);
                    }
                    let notes_dirty = self.editors.values().any(Editor::dirty);
                    if rail_item(
                        ui,
                        icon::DOCUMENTS,
                        "Documentos",
                        self.area == Area::Documents,
                        0,
                        notes_dirty,
                    )
                    .on_hover_text("Documentos importados e registros, com a ficha de cada um. (Ctrl+2)")
                    .clicked()
                    {
                        self.navigate(Leave::Documents);
                    }
                    let tasks = rail_item(
                        ui,
                        icon::TASKS,
                        "Tarefas",
                        self.area == Area::Tasks,
                        self.task_badge,
                        self.tasks.dirty(),
                    );
                    let tasks = if self.tasks.dirty() {
                        tasks.on_hover_text("Há uma tarefa com alterações não salvas. (Ctrl+3)")
                    } else if self.task_badge > 0 {
                        tasks.on_hover_text(format!(
                            "{} ou para hoje. (Ctrl+3)",
                            plural(
                                self.task_badge,
                                "tarefa atrasada",
                                "tarefas atrasadas"
                            )
                        ))
                    } else {
                        tasks.on_hover_text("Tarefas de todos os documentos. (Ctrl+3)")
                    };
                    if tasks.clicked() {
                        self.go_tasks();
                    }
                    if rail_item(
                        ui,
                        icon::FOLDER,
                        "Projetos",
                        self.area == Area::Projects,
                        0,
                        false,
                    )
                    .on_hover_text("Tarefas reunidas por objetivo, com progresso, riscos e linha do tempo. (Ctrl+7)")
                    .clicked()
                    {
                        self.navigate(Leave::Projects);
                    }
                    if rail_item(
                        ui,
                        icon::ATTENDANCES,
                        "Atendimentos",
                        self.area == Area::Saiop,
                        0,
                        self.saiop.dirty(),
                    )
                    .on_hover_text("Controle de entradas e prazos do setor. (Ctrl+4)")
                    .clicked()
                    {
                        self.navigate(Leave::Saiop);
                    }
                    if rail_item(
                        ui,
                        icon::TEMPLATES,
                        "Modelos",
                        self.area == Area::Templates,
                        0,
                        self.templates.dirty(),
                    )
            .on_hover_text("Modelos de trabalho: estrutura, instruções, exemplos e referências para as minutas. (Ctrl+5)")
                    .clicked()
                    {
                        self.navigate(Leave::Templates);
                    }
                    let resources = rail_item(
                        ui,
                        icon::RESOURCES,
                        "Recursos",
                        self.area == Area::Resources,
                        self.resources_overdue,
                        self.resources.dirty(),
                    );
                    let resources = if self.resources.dirty() {
                        resources.on_hover_text("Há um recurso ou uso com alterações não salvas. (Ctrl+6)")
                    } else if self.resources_overdue > 0 {
                        resources.on_hover_text(format!(
                            "{} (Ctrl+6)",
                            plural(
                                self.resources_overdue,
                                "devolução atrasada",
                                "devoluções atrasadas"
                            )
                        ))
                    } else {
                        resources.on_hover_text("Salas, livros e equipamentos: reservas, usos e devoluções. (Ctrl+6)")
                    };
                    if resources.clicked() {
                        self.navigate(Leave::Resources);
                    }
                        });
                    });
            });
    }

    /// The keyboard, in one place. Ctrl+1 to Ctrl+7 change the area, like the
    /// rail; Ctrl+F goes to the search of the area; Ctrl+N creates; Ctrl+S
    /// saves what is being edited; Esc closes the window or panel in front.
    /// Nothing here discards unsaved work: each key goes through the same
    /// path as the button it stands for.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        // AltGr arrives as Ctrl+Alt on Windows and types characters (², £):
        // never a shortcut.
        if ctx.input(|input| input.modifiers.alt) {
            return;
        }
        let pressed = |key: Key| ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, key));
        if pressed(Key::Num1) {
            self.navigate(Leave::Today);
        } else if pressed(Key::Num2) {
            self.navigate(Leave::Documents);
        } else if pressed(Key::Num3) {
            self.go_tasks();
        } else if pressed(Key::Num4) {
            self.navigate(Leave::Saiop);
        } else if pressed(Key::Num5) {
            self.navigate(Leave::Templates);
        } else if pressed(Key::Num6) {
            self.navigate(Leave::Resources);
        } else if pressed(Key::Num7) {
            self.navigate(Leave::Projects);
        } else if pressed(Key::K) {
            self.global_search.open = true;
        } else if pressed(Key::F) {
            match self.area {
                Area::Tasks => self.tasks.key_find(),
                Area::Saiop => self.saiop.key_find(),
                Area::Documents => self.focus_search = true,
                Area::Templates => self.templates.key_find(),
                Area::Resources => self.resources.key_find(),
                Area::Projects => self.projects.key_find(),
                Area::Today | Area::Settings => {
                    self.focus_search = true;
                    self.navigate(Leave::Documents);
                }
            }
        } else if pressed(Key::N) {
            match self.area {
                Area::Tasks => {
                    self.tasks.key_new(self.selected_id);
                }
                Area::Saiop => {
                    self.saiop.key_new(&self.store);
                }
                Area::Today | Area::Documents => {
                    if !self.manual_form.open {
                        self.manual_form.open_new();
                    }
                }
                Area::Templates => {
                    self.templates.key_new();
                }
                Area::Resources => {
                    self.resources.key_new(&self.store);
                }
                Area::Projects => self.projects.start_new_project(),
                Area::Settings => {}
            }
        } else if pressed(Key::S) {
            match self.area {
                Area::Tasks => {
                    self.tasks.key_save(&mut self.store);
                }
                Area::Saiop => {
                    self.saiop.key_save(&mut self.store);
                }
                Area::Documents => {
                    self.key_save_ficha();
                }
                Area::Settings => {
                    if self.settings_section == SettingsSection::Ai
                        && self.ai_config_dirty()
                        && self.save_ai_config()
                    {
                        self.message("Configuração de IA salva.");
                    }
                }
                Area::Templates => {
                    self.templates.key_save(&mut self.store);
                }
                Area::Resources => {
                    self.resources.key_save(&mut self.store);
                }
                Area::Today | Area::Projects => {}
            }
        } else if !egui::Popup::is_any_open(ctx)
            && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            // A window in front closes first; then the panel of the area.
            if self.global_search.open {
                self.global_search.open = false;
            } else if self.about_open {
                self.about_open = false;
            } else if self.consult.is_some() {
                self.consult = None;
            } else if self.manual_form.open || self.mail.is_window_open() {
                // Their own buttons close them (they may hold unsaved work).
            } else {
                match self.area {
                    Area::Tasks => {
                        self.tasks.key_escape();
                    }
                    Area::Saiop => {
                        self.saiop.key_escape();
                    }
                    Area::Templates => {
                        self.templates.key_escape();
                    }
                    Area::Resources => {
                        self.resources.key_escape();
                    }
                    Area::Projects => {
                        self.projects.key_escape();
                    }
                    _ => {}
                }
            }
        }
    }

    /// The Tarefas area never hides an unsaved task, so it needs no question;
    /// only the AI section of Ajustes is saved on the way out.
    fn go_tasks(&mut self) {
        if self.area == Area::Settings
            && self.settings_section == SettingsSection::Ai
            && !self.leave_ai_settings()
        {
            return;
        }
        self.return_to = None;
        self.return_after_select = None;
        self.area = Area::Tasks;
    }

    /// Room around each area. Areas with their own side panels draw to the
    /// edge of the window and keep the room inside.
    fn area_margin(&self) -> egui::Margin {
        match self.area {
            Area::Tasks | Area::Saiop | Area::Resources | Area::Settings => egui::Margin::ZERO,
            _ => egui::Margin::symmetric(20, 14),
        }
    }

    fn ai_destination_text(&self) -> (String, String) {
        // A cloud model asked of the server on this computer is answered by
        // ollama.com: that is where the data goes.
        let cloud_on_local = self.ai.provider == AiProvider::Local && self.ai_leaves();
        let provider = match self.ai.provider {
            AiProvider::Local if cloud_on_local => "Ollama em nuvem",
            AiProvider::Local => "Ollama neste computador",
            AiProvider::Cloud => "Ollama Cloud",
            AiProvider::Remote => "servidor remoto",
            AiProvider::ChatGpt => "ChatGPT (OpenAI)",
        };
        let destination = if cloud_on_local {
            "https://ollama.com".to_owned()
        } else {
            url::Url::parse(self.ai.base_url().trim())
                .map(|url| url.origin().ascii_serialization())
                .unwrap_or_else(|_| "endereço ainda não configurado".into())
        };
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
            "o texto do documento (com número, título, tipo, datas, remetente, destinatário e interessado, quando constam), as notas e o contexto salvos, a orientação desta minuta, os exemplos aprovados do modelo de trabalho e as fontes selecionadas"
        } else {
            "o texto do documento (com número, título, tipo, datas, remetente, destinatário e interessado, quando constam), as notas e o contexto salvos e as fontes selecionadas"
        };
        let leaves = if self.ai_leaves() {
            ""
        } else {
            " Nada sai deste computador."
        };
        format!(
            "Ao clicar, envia ao {provider} ({destination}): {sent}. Não envia: {} Modelo de IA: {model}.{leaves} O resultado é uma sugestão para a sua revisão.",
            queue::NOT_SENT_LINE
        )
    }

    /// The short line that says where the AI runs.
    fn ai_line_text(&self) -> String {
        let (provider, destination) = self.ai_destination_text();
        if self.ai_leaves() {
            format!("envia dados ao {provider}: {destination}")
        } else {
            format!("{provider}, sem envio externo")
        }
    }

    /// One short, always-visible line saying where the AI runs.
    fn ai_destination(&self, ui: &mut egui::Ui) {
        widgets::small_muted(ui, self.ai_line_text()).on_hover_text(self.ai_disclosure_text(false));
    }

    /// Opens "Configurar IA" and asks the server for its models.
    fn open_ai_settings(&mut self, ctx: &egui::Context) {
        self.settings_open = true;
        if !self.models_match_current() {
            self.fetch_models(ctx);
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
                let document = plan.document.unwrap_or_default();
                self.templates.dirty()
                    || self.editors.get(&document).is_some_and(Editor::dirty)
                    || self.work_context.document_dirty(document)
                    || self.dossier.document_dirty(document)
                    || self.tasks.document_dirty(document)
                    || self
                        .suggestion
                        .as_ref()
                        .is_some_and(|s| s.document_id == document)
            }
            Kind::Capture => self.editors.values().any(|e| {
                e.draft_dirty()
                    && (e.capture_id == target.id
                        || e.materials.iter().any(|s| s.capture_id == target.id))
            }),
            Kind::Task => self.tasks.task_dirty(target.id),
            Kind::Attachment => false,
        };
        if blocked {
            self.error("Há alterações ou sugestão ainda não salvas relacionadas a este item. Salve ou cancele essa edição antes de excluir.");
            return;
        }
        self.trash.request(target);
    }
    /// Removes for good what outlived the trash retention: once at start
    /// and then daily, never while another operation runs.
    fn sweep_trash(&mut self) {
        const DAILY: Duration = Duration::from_secs(24 * 60 * 60);
        // A recovery library being looked at keeps its Lixeira as the copy
        // had it: nothing expires before the person decides to use it.
        if self.worker.is_some()
            || self.pdf_reader.busy()
            || self.library.only_looking()
            || !self.last_trash_purge.is_none_or(|at| at.elapsed() > DAILY)
        {
            return;
        }
        self.last_trash_purge = Some(Instant::now());
        let days = match self.store.trash_retention_days() {
            Ok(days) if days > 0 => days,
            _ => return,
        };
        match self.store.purge_expired_trash(days) {
            Ok(0) => {}
            Ok(count) => {
                self.message(format!(
                    "Lixeira: {} por estar na lixeira há mais de {}.",
                    plural(
                        count,
                        "item foi apagado de vez",
                        "itens foram apagados de vez"
                    ),
                    crate::trash::retention_label(days)
                ));
                self.refresh();
            }
            Err(error) => self.error(format!(
                "Não foi possível esvaziar a lixeira automaticamente: {error}"
            )),
        }
    }

    fn apply_trash_change(&mut self, change: crate::trash::Change) {
        use crate::trash::Kind;
        if !change.restored {
            match change.target.kind {
                Kind::Document => {
                    if let Some(document) = change.document {
                        self.editors.remove(&document);
                        self.sources.forget_document(document);
                        self.dossier.forget_document(document);
                        self.work_context.forget_document(document);
                        self.tasks.forget_document(document);
                        if self.selected_id == Some(document) {
                            self.selected_id = None;
                            self.detail = None;
                            self.similar.clear();
                        }
                    }
                }
                Kind::Capture => {
                    if let Some(document) = change.document {
                        self.sources.forget_document(document);
                        if self.selected_id == Some(document) {
                            self.reload(document);
                        }
                    }
                }
                Kind::Task => self.tasks.forget_task(change.target.id),
                Kind::Attachment => {}
            }
            self.message_with_action(
                if change.target.kind == Kind::Attachment {
                    "Anexo movido para a Lixeira."
                } else {
                    "Item movido para a Lixeira."
                },
                "Desfazer",
                NoticeAction::Restore(change.target.clone()),
            );
        } else {
            if let Some(document) = change.document {
                if self.selected_id == Some(document) {
                    self.reload(document);
                }
            }
            self.message(if change.target.kind == Kind::Capture {
                "Importação restaurada no histórico. A importação em uso foi mantida."
            } else {
                "Item restaurado. Os documentos e as tarefas foram atualizados."
            });
        }
        self.refresh();
    }
}

impl PonteApp {
    /// Another start hands its request over by a file and waits only a few
    /// seconds for the answer, so the window must notice it promptly even
    /// when it has nothing else to redraw: a small thread looks at the file
    /// and wakes the window, instead of redrawing it several times a second.
    fn watch_other_starts(&mut self, ctx: &egui::Context) {
        let Some(dir) = self.requests_dir.clone().filter(|_| !self.requests_watched) else {
            return;
        };
        self.requests_watched = true;
        let wake = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("ponte-other-starts".into())
            .spawn(move || loop {
                std::thread::sleep(REQUEST_LOOK_EVERY);
                if crate::single_instance::request_pending(&dir) {
                    wake.request_repaint();
                }
            });
    }

    /// When the window next needs a frame if nobody touches it (see
    /// [`idle_wait`]).
    fn schedule_idle_repaint(&self, ctx: &egui::Context) {
        let notice_left = self
            .notice
            .as_ref()
            .filter(|n| !n.error)
            .map(|n| NOTICE_LIFETIME.saturating_sub(n.since.elapsed()));
        ctx.request_repaint_after(idle_wait(
            self.last_refresh.elapsed(),
            ctx.input(|i| i.focused),
            self.worker.is_some(),
            notice_left,
            self.search_changed.map(|changed| changed.elapsed()),
        ));
    }

    /// Another start of the Ponte (the extension after an import, the Start
    /// menu) asked this window to come to the front, maybe on a document.
    fn answer_other_starts(&mut self, ctx: &egui::Context) {
        if self.requests_checked.elapsed() < REQUEST_LOOK_EVERY {
            return;
        }
        self.requests_checked = Instant::now();
        let Some(request) = self
            .requests_dir
            .as_deref()
            .and_then(crate::single_instance::take_request)
        else {
            return;
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
        match request {
            crate::single_instance::Request::Focus => {}
            crate::single_instance::Request::Templates => self.navigate(Leave::Templates),
            crate::single_instance::Request::Document(id) => {
                // The library may have just received it from the extension.
                self.refresh();
                // Through the guard: unsaved work is never hidden.
                self.navigate(Leave::Select(id));
            }
        }
    }
}

impl eframe::App for PonteApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.apply_holiday_draft();
        crate::attachments::clear_temp_dir();
        self.reminders.shutdown();
        // Stopped before the pointer moves: no exchange, no main computer.
        self.intranet.set_transport_off(true);
        self.finish_switch();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::theme::sync(&ctx);
        self.intranet.attach_repaint(&ctx);
        self.poll_worker();
        self.start_queued();
        self.tick_auto_copy(&ctx);
        self.tick_library_place();
        // The reminders are those of the library in use, never of a recovery
        // library being looked at (they would ring twice).
        if !self.library.only_looking() {
            self.reminders.update(&ctx, &self.store);
        }
        if self.reminders.take_open_today() {
            // The person clicked the Windows notice: show what is due.
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            // Through the guard: unsaved work is never hidden. With a question
            // already open (exit, Lixeira, leaving an edit) the window just
            // comes to the front: the question keeps its answer.
            let asking = self.replacement.is_some()
                || self.trash.asking()
                || self.exit_confirm
                || self.leave.is_some();
            if !asking {
                self.navigate(Leave::Today);
            }
        }
        if let Some(text) = self.reminders.take_notice() {
            self.message(text);
        }
        self.poll_models();
        self.poll_chatgpt_login(&ctx);
        self.watch_other_starts(&ctx);
        self.answer_other_starts(&ctx);
        if self.chatgpt_sync_pending && self.worker.is_none() && self.model_fetch.is_none() {
            self.chatgpt_sync_pending = false;
            self.sync_chatgpt_session();
        }
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
            .is_some_and(|changed| changed.elapsed() > SEARCH_DEBOUNCE)
        {
            self.search_changed = None;
            // Only the list of documents depends on the search.
            self.refresh_documents_for_search();
        }
        if self.last_refresh.elapsed() > REFRESH_EVERY {
            self.refresh_polled();
        }
        self.sweep_trash();
        self.draft_tools.comparison(&ctx);
        self.pdf_reader.show(&ctx, &mut self.store);
        let pending = self.templates.dirty()
            || self.mail.dirty()
            || self.pdf_reader.busy()
            || self.dossier.dirty()
            || self.work_context.dirty()
            || self.tasks.dirty()
            || self.saiop.dirty()
            || self.resources.dirty()
            || self.manual_form.dirty()
            || self.intranet.unsent_folder_changes(&self.store) > 0
            || self.editors.values().any(Editor::dirty)
            || self.worker.is_some()
            || self.suggestion.is_some();
        // The password of the portable copy never stays behind a section or
        // an area the person left.
        if self.portable.mode != backup::PortableMode::Closed
            && !(self.area == Area::Settings && self.settings_section == SettingsSection::Backup)
        {
            self.close_portable_form(&ctx);
        }
        if !self.allow_close && pending && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_confirm = true;
        }
        let enabled = self.replacement.is_none()
            && !self.trash.asking()
            && !self.exit_confirm
            && self.send_confirm.is_none()
            && self.leave.is_none();
        if enabled {
            self.shortcuts(&ctx);
            self.route_settings_requests();
        }
        let palette = crate::theme::c();
        self.library_banner(ui);
        egui::Panel::left("rail")
            .exact_size(88.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(palette.rail)
                    .inner_margin(egui::Margin::symmetric(8, 10)),
            )
            .show(ui, |ui| {
                ui.add_enabled_ui(enabled, |ui| self.rail(ui));
            });
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(palette.surface)
                    .stroke(egui::Stroke::new(1.0, palette.outline))
                    .inner_margin(egui::Margin::symmetric(14, 5)),
            )
            .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let mut cancel = false;
            if let Some(worker) = &self.worker {
                ui.horizontal_wrapped(|ui| {
                    ui.spinner();
                    let step = if worker.cancelling() {
                        "cancelando ao fim da etapa atual…".to_owned()
                    } else if worker.latest.is_empty() {
                        String::new()
                    } else {
                        format!("{} • ", worker.latest)
                    };
                    ui.label(format!(
                        "{} • {}{} s",
                        worker.label,
                        step,
                        worker.started.elapsed().as_secs()
                    ));
                    if matches!(
                        worker.kind,
                        WorkKind::Analysis
                            | WorkKind::Draft
                            | WorkKind::AiTest
                            | WorkKind::Conference
                    ) {
                        cancel = ui
                            .add_enabled(
                                !worker.cancelling(),
                                widgets::quiet("Cancelar"),
                            )
                            .on_hover_text("Interrompe a IA na próxima etapa. Nada é salvo.")
                            .clicked();
                    }
                });
            }
            if cancel {
                self.cancel_worker();
            }
            // The AI job waiting for the running one.
            let mut drop_queue = false;
            if let Some(job) = &self.queued {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "Na fila: {} — começa quando terminar o trabalho atual",
                        job.waiting
                    ));
                    drop_queue = ui
                        .add(widgets::quiet("Tirar da fila"))
                        .on_hover_text("O trabalho não será feito. Nada é enviado nem salvo.")
                        .clicked();
                });
            }
            if drop_queue {
                self.drop_queued();
            }
            // Network line: the sync progress while it runs, otherwise a
            // discreet coloured dot with the short status (moved here from
            // the header at the user's request).
            if self.intranet.syncing() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let step = self.intranet.sync_step();
                    if self.intranet.folder_mode(&self.store) {
                        ui.small(format!(
                            "Sincronizando com a pasta compartilhada… {}",
                            if step.is_empty() {
                                "lendo as pastas…"
                            } else {
                                step
                            }
                        ));
                    } else if step.is_empty() {
                        ui.small("Sincronizando com o computador principal…");
                    } else {
                        ui.small(format!("Sincronizando com o computador principal… {step}"));
                    }
                });
            } else {
                ui.horizontal(|ui| {
                let (color, short, full) = self.network_status();
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, color);
                let response = ui
                    .add(
                        egui::Button::new(
                            RichText::new(short)
                                .color(palette.muted)
                                .size(crate::theme::SIZE_SMALL),
                        )
                        .frame(false),
                    )
                    .on_hover_text(full);
                if response.clicked() {
                    self.intranet.open = true;
                }
                // Shared folders: the exchange at hand, beside its status.
                if self.intranet.folder_mode(&self.store) {
                    let unsent = self.intranet.unsent_folder_changes(&self.store);
                    let label = if unsent > 0 {
                        format!("{} Sincronizar ({unsent} a enviar)", crate::theme::icon::SYNC)
                    } else {
                        format!("{} Sincronizar", crate::theme::icon::SYNC)
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(label)
                                    .size(crate::theme::SIZE_SMALL)
                                    .color(palette.primary),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Sincroniza agora com as pastas compartilhadas. O Ponte também faz isso ao abrir, a cada 5 minutos e cerca de 2 minutos depois de uma alteração sua.")
                        .clicked()
                    {
                        self.intranet.request_full_sync();
                        self.refresh();
                    }
                }
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
        });
        // Notices float over the corner: successes leave by themselves,
        // errors stay until closed.
        let mut dismiss = self
            .notice
            .as_ref()
            .is_some_and(|n| !n.error && n.since.elapsed() > NOTICE_LIFETIME);
        let mut pressed = None;
        if let Some(notice) = &self.notice {
            let label = notice.action.as_ref().map(|(label, _)| label.as_str());
            match crate::widgets::toast(&ctx, &notice.text, notice.error, label) {
                crate::widgets::ToastAction::None => {}
                crate::widgets::ToastAction::Close => dismiss = true,
                crate::widgets::ToastAction::Action => {
                    pressed = notice.action.as_ref().map(|(_, action)| action.clone());
                    dismiss = true;
                }
            }
        }
        if dismiss {
            self.notice = None;
        }
        if let Some(action) = pressed {
            self.run_notice_action(action);
        }
        if self.area == Area::Documents {
            library_panel(ui.max_rect().width())
                .frame(
                    egui::Frame::new()
                        .fill(palette.surface)
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| {
                    ui.add_enabled_ui(enabled, |ui| self.sidebar(ui));
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.bg)
                    .inner_margin(self.area_margin()),
            )
            .show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if self.area == Area::Today {
                    self.today_area(ui);
                } else if self.area == Area::Projects {
                    self.projects_area(ui);
                } else if self.area == Area::Settings {
                    self.settings_area(ui);
                } else if self.area == Area::Templates {
                    let open_document = self.detail.as_ref().map(|d| {
                        (
                            d.summary.id,
                            d.summary.document_number.clone(),
                            d.capture.clone(),
                        )
                    });
                    if let Some((attachment, label)) = self.templates.take_pdf_request() {
                        self.pdf_reader
                            .open_attachment(&self.store, attachment, &label, &ctx);
                    }
                    match self.templates.show(ui, &mut self.store, open_document) {
                        crate::templates_ui::TemplatesAction::UseTemplate { uid } => {
                            if let Some(id) = self.selected_id {
                                // The minuta takes the template's kind, or the
                                // editor would drop a template of another kind.
                                let kind = self
                                    .store
                                    .template_by_uid(&uid)
                                    .ok()
                                    .flatten()
                                    .map(|t| t.kind);
                                if let Some(editor) = self.editors.get_mut(&id) {
                                    if let Some(kind) = kind {
                                        editor.kind = kind;
                                    }
                                    editor.template_uid = Some(uid);
                                    editor.selected_blocks.clear();
                                }
                                self.tab = Tab::Notes;
                                self.navigate(Leave::Documents);
                            }
                        }
                        crate::templates_ui::TemplatesAction::None => {}
                    }
                } else if self.area == Area::Resources {
                    match self.resources.show(ui, &mut self.store) {
                        Some(crate::resources_ui::Ask::Document(id)) => {
                            self.return_after_select =
                                self.resources.open_use_uid().map(ReturnTo::Resources);
                            self.navigate(Leave::Select(id));
                        }
                        Some(crate::resources_ui::Ask::Attendance(id)) => {
                            if self.saiop.focus_attendance(&self.store, id) {
                                self.navigate(Leave::Saiop);
                            } else {
                                self.error("Há um atendimento com alterações não salvas em Atendimentos. Salve ou descarte antes de abrir outro.");
                            }
                        }
                        Some(crate::resources_ui::Ask::Task(id)) => {
                            let task = self
                                .store
                                .list_tasks()
                                .ok()
                                .and_then(|tasks| tasks.into_iter().find(|t| t.id == id));
                            match task {
                                Some(task) => self.navigate(Leave::OpenTask(task)),
                                None => self.error("A tarefa ligada não foi encontrada."),
                            }
                        }
                        None => {}
                    }
                    if let Some(text) = self.resources.take_message() {
                        self.message(text);
                    }
                    if self.resources.take_changed() {
                        self.refresh();
                    }
                } else if self.area == Area::Saiop {
                    let context = self.selected_id.and_then(|id| {
                        self.documents
                            .iter()
                            .find(|d| d.id == id)
                            .map(|d| (id, d.document_number.clone()))
                    });
                    self.saiop.set_context_document(context);
                    if let Some(id) = self.saiop.show(ui, &mut self.store) {
                        self.return_after_select =
                            self.saiop.open_attendance_id().map(ReturnTo::Attendance);
                        self.select(id);
                    }
                    if let Some(task) = self.saiop.take_open_task() {
                        if self.tasks.focus_task(&task) {
                            self.area = Area::Tasks;
                        } else {
                            self.error("Há uma tarefa com alterações não salvas no painel Tarefas. Salve ou descarte antes de abrir outra.");
                        }
                    }
                    if let Some(request) = self.saiop.take_use_request() {
                        self.serve_use_request(request);
                    }
                } else if self.area == Area::Tasks {
                    // A batch delete waits for a running job, as a single one does.
                    self.tasks
                        .set_busy(self.worker.is_some() || self.pdf_reader.busy());
                    if let Some(id) = self.tasks.show(ui, &mut self.store, self.selected_id) {
                        self.return_after_select = self.tasks.open_task_id().map(ReturnTo::Task);
                        self.navigate(Leave::Select(id));
                    }
                    // A batch (or its undo) changed tasks: the counters follow at once.
                    if self.tasks.take_batch_changed() {
                        self.refresh();
                    }
                    if let Some(task) = self.tasks.take_concluded() {
                        self.message_with_action(
                            format!("Tarefa concluída: {}", task.title),
                            "Desfazer",
                            NoticeAction::ReopenTask(task.id),
                        );
                    }
                    if let Some(intent) = self.tasks.take_blocked() {
                        self.navigate(match intent {
                            crate::tasks_ui::Intent::Open(task) => Leave::OpenTask(task),
                            crate::tasks_ui::Intent::New(document_id) => Leave::NewTask(document_id),
                        });
                    }
                    if let Some(document_id) = self.tasks.take_consult_request() { self.open_consult(document_id); }
                    if let Some(request) = self.tasks.take_use_request() {
                        self.serve_use_request(request);
                    }
                    if let Some(target)=self.tasks.take_trash_request(){self.request_trash(target);}
                    if let Some((attachment, label)) = self.tasks.take_attachment_pdf_request() {
                        self.pdf_reader.open_attachment(&self.store, attachment, &label, &ctx);
                    }
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
            self.about_window(&ctx);
            self.manual_window(&ctx);
            self.consult_window(&ctx);
            self.suggestion_window(&ctx);
            self.material_window(&ctx);
            self.analysis_material_window(&ctx);
            self.mail_window(&ctx);
            self.global_search(&ctx);
        }
        self.confirmations(&ctx);
        self.send_confirm_window(&ctx);
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
        if self.trash.take_purged() {
            self.refresh();
        }
        #[cfg(feature = "shots")]
        self.tour_frame(&ctx);
        self.schedule_idle_repaint(&ctx);
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

#[cfg(test)]
mod tests {
    use super::{
        ficha::allowed_sigadoc_url, library_panel, DocPriorityFilter, DocSort, DocTaskFilter,
        DocTaskStatusSummary, LIBRARY_DEFAULT_WIDTH, LIBRARY_MAX_FRACTION, LIBRARY_MIN_WIDTH,
    };
    use crate::tasks::Task;

    #[test]
    fn extension_version_matches_the_manifest() {
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("extension/manifest.json"),
        )
        .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(
            manifest["version"].as_str(),
            Some(super::EXTENSION_VERSION),
            "EXTENSION_VERSION em src/app.rs deve acompanhar extension/manifest.json"
        );
    }

    fn app_with_dirty_task() -> (tempfile::TempDir, super::PonteApp) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acervo.sqlite3");
        let store = crate::storage::Store::open(&path).unwrap();
        let mut app = super::PonteApp::new(store, path);
        let suggestion = crate::model::ActionSuggestion {
            action: "Revisar".into(),
            reason: String::new(),
            evidence: String::new(),
        };
        assert!(app.tasks.prepare_suggestion(1, &suggestion, "normal"));
        assert!(app.tasks.dirty());
        app.area = super::Area::Tasks;
        (dir, app)
    }

    #[test]
    fn pending_follow_up_runs_after_discard() {
        let (_dir, mut app) = app_with_dirty_task();
        let ctx = eframe::egui::Context::default();
        app.navigate(super::Leave::Week(ctx.clone()));
        assert!(app.leave.is_some());
        assert!(ctx
            .data(|d| d.get_temp::<bool>(eframe::egui::Id::unique("today-week-view")))
            .is_none());
        app.tasks.discard_draft();
        app.complete_pending_leave();
        assert!(app.area == super::Area::Today);
        assert_eq!(
            ctx.data(|d| d.get_temp::<bool>(eframe::egui::Id::unique("today-week-view"))),
            Some(true)
        );
    }

    #[test]
    fn cancelling_pending_follow_up_keeps_the_edit() {
        let (_dir, mut app) = app_with_dirty_task();
        app.navigate(super::Leave::NewManual);
        assert!(app.leave.is_some());
        app.cancel_pending_leave();
        app.complete_pending_leave();
        assert!(app.area == super::Area::Tasks);
        assert!(app.tasks.dirty());
        assert!(!app.manual_form.open);
    }

    #[test]
    fn document_sort_orders_by_deadline_priority_number_and_title() {
        let mut a = summary(1, "SEFAZ-PRO-2026/00002", None);
        a.title = "zebra".into();
        a.priority = "normal".into();
        let mut b = summary(2, "SEFAZ-PRO-2026/00001", None);
        b.title = "Ágil".into();
        b.priority = "urgente".into();
        let mut c = summary(3, "MANUAL-2026/0001", None);
        c.title = "manual".into();
        let due = |id: i64| match id {
            1 => Some("2026-12-01".to_owned()),
            3 => Some("2026-11-05".to_owned()),
            _ => None,
        };
        let ids =
            |docs: &[&crate::model::DocumentSummary]| docs.iter().map(|d| d.id).collect::<Vec<_>>();

        let mut docs = vec![&a, &b, &c];
        DocSort::Recent.apply(&mut docs, due);
        assert_eq!(ids(&docs), [1, 2, 3], "library order untouched");
        DocSort::Deadline.apply(&mut docs, due);
        assert_eq!(ids(&docs), [3, 1, 2], "no deadline goes last");
        let mut docs = vec![&a, &b, &c];
        DocSort::Priority.apply(&mut docs, due);
        assert_eq!(ids(&docs), [2, 1, 3], "unclassified after normal");
        let mut docs = vec![&a, &b, &c];
        DocSort::Number.apply(&mut docs, due);
        assert_eq!(ids(&docs), [3, 2, 1]);
        let mut docs = vec![&a, &b, &c];
        DocSort::Title.apply(&mut docs, due);
        assert_eq!(
            ids(&docs),
            [3, 1, 2],
            "plain lowercase compare: accents sort after z"
        );
    }

    fn summary(
        id: i64,
        number: &str,
        parent: Option<(i64, &str)>,
    ) -> crate::model::DocumentSummary {
        crate::model::DocumentSummary {
            id,
            document_number: number.into(),
            title: format!("Documento {id}"),
            last_seen_at: String::new(),
            latest_capture_id: id,
            subjects: vec![],
            priority: "normal".into(),
            analysis_stale: false,
            manual: false,
            categories: vec![],
            email: false,
            email_from: String::new(),
            whatsapp: false,
            whatsapp_chat: String::new(),
            added_at: String::new(),
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
                        egui::TextEdit::singleline(&mut String::new()).desired_width(f32::INFINITY),
                    );
                    egui::ScrollArea::vertical()
                        .id_salt("document-list")
                        .show(ui, |ui| {
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
                    egui::ScrollArea::vertical()
                        .id_salt("document-content")
                        .show(ui, |ui| {
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
        let parent = "ORG-DIC-2026/97546";
        // Most recent first: the juntado child arrives before its parent.
        let docs = [
            summary(3, "ORG-DES-2026/39019", Some((2, parent))),
            summary(1, "SEFAZ-PRO-2026/00001", None),
            summary(2, parent, None),
            summary(4, "ORG-OFI-2026/00010", Some((2, parent))),
            summary(5, "ORG-OFI-2026/00011", Some((0, "ORG-DIC-2026/99999"))),
            summary(6, "ORG-OFI-2026/00012", Some((3, "ORG-DES-2026/39019"))),
        ];
        let refs: Vec<_> = docs.iter().collect();
        let order: Vec<(i64, usize)> = super::library::group_joined(&refs)
            .into_iter()
            .map(|(d, depth)| (d.id, depth))
            .collect();
        assert_eq!(order, vec![(1, 0), (2, 0), (3, 1), (6, 2), (4, 1), (5, 0)]);

        // Parent filtered out: children stay in place, at the top level.
        let without_parent: Vec<_> = docs.iter().filter(|d| d.id != 2).collect();
        let order: Vec<i64> = super::library::group_joined(&without_parent)
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
        assert_eq!(super::library::group_joined(&refs).len(), 2);

        assert_eq!(
            super::library::joined_label(&docs[0]).as_deref(),
            Some("↳ Juntado a ORG-DIC-2026/97546")
        );
        assert_eq!(
            super::library::joined_label(&docs[4]).as_deref(),
            Some("↳ Juntado a ORG-DIC-2026/99999 (não importado)")
        );
        let mut via_only = summary(9, "X-2026/1", None);
        via_only.is_joined = true;
        assert_eq!(
            super::library::joined_label(&via_only).as_deref(),
            Some("↳ Documento juntado")
        );
        assert_eq!(super::library::joined_label(&docs[1]), None);
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
        assert_eq!(s1.badge().1, crate::theme::error());
        assert!(s1.badge().0.contains("atrasada"));

        let s2 = DocTaskStatusSummary::from_tasks(&tasks, 2, today);
        assert!(s2.matches(DocTaskFilter::OpenOnTime));
        assert!(s2.matches(DocTaskFilter::OpenAny));
        assert!(!s2.matches(DocTaskFilter::Overdue));
        assert_eq!(s2.badge().1, crate::theme::on_time());
        assert_ne!(
            crate::theme::on_time(),
            crate::theme::success(),
            "open and done never share a colour"
        );
        assert!(s2.badge().0.contains("no prazo"));

        let s3 = DocTaskStatusSummary::from_tasks(&tasks, 3, today);
        assert!(s3.matches(DocTaskFilter::OpenOnTime));
        assert_eq!(s3.badge().1, crate::theme::warning());
        assert_eq!(s3.badge().0, "⏰ 1 vence hoje");

        // One task due today and two later: only one is "due today".
        let mut later_a = Task::new(9);
        later_a.due_date = "2026-12-01".into();
        let mut later_b = Task::new(9);
        later_b.due_date = "2026-12-15".into();
        let mut due_now = Task::new(9);
        due_now.due_date = today.into();
        let mut done = Task::new(9);
        done.status = "concluida".into();
        let mixed =
            DocTaskStatusSummary::from_tasks(&[later_a, later_b, due_now.clone(), done], 9, today);
        assert_eq!(mixed.badge().0, "⏰ 1 vence hoje (+2 no prazo) • ✓ 1");
        assert_eq!(mixed.badge().1, crate::theme::warning());
        let both_today = DocTaskStatusSummary::from_tasks(&[due_now.clone(), due_now], 9, today);
        assert_eq!(both_today.badge().0, "⏰ 2 vencem hoje");

        // An overdue task always brings its date along.
        assert_eq!(s1.badge().0, "⚠ 1 tarefa atrasada • Prazo: 25/10/2026");

        let s4 = DocTaskStatusSummary::from_tasks(&tasks, 4, today);
        assert!(s4.matches(DocTaskFilter::Completed));
        assert!(!s4.matches(DocTaskFilter::OpenAny));
        assert_eq!(s4.badge().1, crate::theme::success());
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

#[cfg(test)]
mod sender_tests {
    use super::library::sender_line;

    #[test]
    fn e_mail_cards_show_address_then_name() {
        assert_eq!(
            sender_line("\"Funadesp\" <funadesp.oficial@yahoo.com.br>"),
            "funadesp.oficial@yahoo.com.br · Funadesp"
        );
        assert_eq!(sender_line("a@b.br"), "a@b.br");
        assert_eq!(sender_line("<a@b.br>"), "a@b.br");
        assert_eq!(sender_line("João Silva <j@x.br>"), "j@x.br · João Silva");
    }
}

#[cfg(test)]
mod refresh_tests {
    use super::PonteApp;
    use crate::holidays::{ConfigGuard, HolidayConfig};
    use crate::storage::Store;
    use crate::tasks::Task;

    fn app(dir: &tempfile::TempDir) -> PonteApp {
        let path = dir.path().join("acervo.sqlite3");
        PonteApp::new(Store::open(&path).unwrap(), path)
    }

    /// Marks what the derived part of a refresh built; the mark survives only
    /// until it is built again.
    fn mark(app: &mut PonteApp) {
        app.today.error = Some("marca".into());
    }
    fn marked(app: &PonteApp) -> bool {
        app.today.error.as_deref() == Some("marca")
    }

    /// An open document whose note editor holds unsaved text while another
    /// connection saved a different note.
    fn note_edited_here_and_saved_elsewhere(dir: &tempfile::TempDir) -> (PonteApp, i64) {
        let mut app = app(dir);
        let document = app
            .store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        app.refresh();
        app.select(document);
        app.editors.get_mut(&document).unwrap().note = "minha edição".into();
        let mut other = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        other.save_note(document, "versão externa").unwrap();
        app.refresh_polled();
        // The poll keeps the unsaved edit but shows the stored note.
        assert_eq!(app.editors[&document].note, "minha edição");
        assert_eq!(app.detail.as_ref().unwrap().note, "versão externa");
        (app, document)
    }

    fn stored_note_version(app: &PonteApp, document: i64) -> Option<i64> {
        app.store
            .item_version("note", document)
            .ok()
            .map(|meta| meta.version)
    }

    #[test]
    fn a_failed_read_is_retried_at_the_next_poll() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        app.store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        app.refresh();
        assert!(app.today.error.is_none());

        // Another connection leaves the library unreadable for the tasks.
        let other = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        other
            .conn
            .execute("ALTER TABLE tasks RENAME TO tasks_fora", [])
            .unwrap();
        app.refresh_polled();
        assert!(app.today.error.is_some(), "the failure is shown");

        // The table is back through this window's own connection (which does
        // not change what the poll watches); the next poll reads again.
        app.store
            .conn
            .execute("ALTER TABLE tasks_fora RENAME TO tasks", [])
            .unwrap();
        app.refresh_polled();
        assert!(
            app.today.error.is_none(),
            "the failed section stayed cached until the key changed"
        );
        // And once everything worked the poll leaves it alone again.
        mark(&mut app);
        app.refresh_polled();
        assert!(marked(&app));
        drop(other);
    }

    #[test]
    fn discarding_the_note_shows_what_another_connection_saved() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let (mut app, document) = note_edited_here_and_saved_elsewhere(&dir);
        app.discard_note(document);
        let editor = &app.editors[&document];
        assert_eq!(editor.note, "versão externa");
        assert_eq!(editor.saved_note, "versão externa");
        assert!(!editor.note_dirty());
        assert!(editor.note_version.is_some());
        assert_eq!(editor.note_version, stored_note_version(&app, document));
        // And the next save checks against the current version: it goes through.
        app.editors.get_mut(&document).unwrap().note = "depois".into();
        app.save_note(document);
        assert_eq!(app.store.load_document(document).unwrap().note, "depois");
    }

    #[test]
    fn typing_the_note_back_to_the_loaded_text_adopts_the_stored_one() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let (mut app, document) = note_edited_here_and_saved_elsewhere(&dir);
        let editor = app.editors.get_mut(&document).unwrap();
        editor.note = editor.saved_note.clone();
        app.reload_note_if_clean(document);
        let editor = &app.editors[&document];
        assert_eq!(editor.note, "versão externa");
        assert_eq!(editor.note_version, stored_note_version(&app, document));
        // A note still being edited is never replaced.
        let dir2 = tempfile::tempdir().unwrap();
        let (mut app2, doc2) = note_edited_here_and_saved_elsewhere(&dir2);
        app2.reload_note_if_clean(doc2);
        assert_eq!(app2.editors[&doc2].note, "minha edição");
    }

    #[test]
    fn the_poll_rebuilds_only_what_changed_and_never_shows_old_data() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        let document = app
            .store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        app.refresh();
        let badge = app.task_badge;
        let documents = app.documents.len();
        assert_eq!(documents, 1);

        // Nothing changed: the poll leaves what was built.
        mark(&mut app);
        app.refresh_polled();
        assert!(marked(&app), "rebuilt with nothing changed");

        // A save here: seen by the next poll.
        let mut task = Task::new(document);
        task.title = "Vence hoje".into();
        task.due_date = chrono::Local::now().format("%Y-%m-%d").to_string();
        app.store.save_task(&task).unwrap();
        app.refresh_polled();
        assert!(!marked(&app), "a save did not rebuild");
        assert_eq!(app.task_badge, badge + 1);

        // A commit of another connection (the team server, a synchronization).
        mark(&mut app);
        let mut other = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let mut again = Task::new(document);
        again.title = "Atrasada".into();
        again.due_date = "2000-01-01".into();
        other.save_task(&again).unwrap();
        app.refresh_polled();
        assert!(!marked(&app), "another connection's commit did not rebuild");
        assert_eq!(app.task_badge, badge + 2);

        // The day (or the minute) changed since the last refresh.
        mark(&mut app);
        app.derived_for.as_mut().unwrap().day = "2000-01-01".into();
        app.refresh_polled();
        assert!(!marked(&app), "a new day did not rebuild");
        mark(&mut app);
        app.derived_for.as_mut().unwrap().minute = "2000-01-01T00:00".into();
        app.refresh_polled();
        assert!(!marked(&app), "a new minute did not rebuild");

        // The holiday calendar the planning counts with.
        mark(&mut app);
        let mut holidays = HolidayConfig::new();
        holidays.uf = Some("MG");
        crate::holidays::set_config(holidays);
        app.refresh_polled();
        assert!(!marked(&app), "a new holiday calendar did not rebuild");

        // An explicit refresh (after an action) always rebuilds.
        mark(&mut app);
        app.refresh();
        assert!(!marked(&app));
    }

    #[test]
    fn a_search_reads_only_the_list_of_documents() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        app.store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        app.refresh();
        assert_eq!(app.documents.len(), 1);
        let generation = app.documents_gen;

        mark(&mut app);
        app.query = "palavra-que-nao-existe".into();
        app.refresh_documents_for_search();
        assert!(app.documents.is_empty());
        assert_ne!(app.documents_gen, generation);
        assert!(marked(&app), "a search rebuilt more than the list");

        // The same search again, library untouched: not read again.
        let generation = app.documents_gen;
        app.refresh_documents_for_search();
        assert_eq!(app.documents_gen, generation);

        app.query.clear();
        app.refresh_documents_for_search();
        assert_eq!(app.documents.len(), 1);
        assert!(marked(&app));

        // And the poll, with the library unchanged, leaves the list of the
        // search it was read for.
        app.query = "palavra-que-nao-existe".into();
        app.refresh_documents_for_search();
        let generation = app.documents_gen;
        app.refresh_polled();
        assert_eq!(app.documents_gen, generation);
        assert!(marked(&app));
    }
}

#[cfg(test)]
mod idle_tests {
    use super::{idle_wait, REFRESH_EVERY};
    use std::time::Duration;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn an_idle_window_wakes_for_the_poll_and_for_the_clock_labels_only() {
        // In use: a frame every second at most, sooner for the poll.
        assert_eq!(idle_wait(MS(0), true, false, None, None), MS(1000));
        assert_eq!(idle_wait(MS(3700), true, false, None, None), MS(310));
        // Not in use: the poll is the only reason.
        assert_eq!(
            idle_wait(MS(1000), false, false, None, None),
            REFRESH_EVERY - MS(1000) + MS(10)
        );
        assert!(idle_wait(MS(0), false, false, None, None) <= Duration::from_secs(5));
        // The poll is due: a frame at once.
        assert_eq!(idle_wait(MS(9000), false, false, None, None), MS(10));
    }

    #[test]
    fn a_job_a_notice_and_a_search_ask_for_the_time_they_need() {
        // A running job shows seconds and steps.
        assert_eq!(idle_wait(MS(0), false, true, None, None), MS(300));
        // A notice that leaves in two seconds, and one about to leave.
        assert_eq!(
            idle_wait(MS(0), false, false, Some(MS(2000)), None),
            MS(2010)
        );
        assert_eq!(idle_wait(MS(0), true, false, Some(MS(0)), None), MS(10));
        // A search waiting for its pause.
        assert_eq!(idle_wait(MS(0), false, false, None, Some(MS(100))), MS(160));
        assert_eq!(idle_wait(MS(0), false, false, None, Some(MS(400))), MS(10));
    }
}
