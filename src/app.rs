//! Native desktop workspace. Document content is only persisted through `Store`.
use crate::ai::{self, AiConfig, AiProvider, AiSettings, ApiKind, ModelInfo};
use crate::chatgpt::{self, ChatGptSession, SharedSession};
use crate::model::{Analysis, DocumentDetail, DocumentSummary};
use crate::storage::Store;
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, RichText};
use crate::theme::{self, icon, S1, S2, S3, S4};
use crate::widgets::{self, Tone};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Public repository with the installer, the source and the release notes.
const PROJECT_URL: &str = "https://github.com/demusis/ponteproj";
/// Version of the browser extension shipped with this build (extension/manifest.json).
const EXTENSION_VERSION: &str = "0.1.11";

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
}

impl ManualForm {
    fn open_new(&mut self) {
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
        };
    }

    fn dirty(&self) -> bool {
        self.open && self.fields != self.original
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
                (format!("{} em aberto, no prazo", self.open_on_time), Tone::Info)
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
            .trim_start_matches(|ch: char| {
                matches!(ch, '⚠' | '⏰' | '●' | '✓' | '○' | ' ')
            })
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
    let db_path = crate::paths::db_path()?;
    let store = Store::open(&db_path).context("Não foi possível abrir o acervo protegido")?;
    // Temporary copies of attachments opened in other programs do not outlive a session.
    crate::attachments::clear_temp_dir();
    let mut app = PonteApp::new(store, db_path);
    app.intranet.initialize(&mut app.store, &app.db_path);
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
                    plural(count, "modelo antigo foi convertido", "modelos antigos foram convertidos"),
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
    if open_templates {
        app.area = Area::Templates;
    }
    if let Some(id) = initial_document {
        app.select(id);
    }
    let viewport = egui::ViewportBuilder::default()
            .with_title(format!(
                "Ponte {} • Assistente documental",
                env!("CARGO_PKG_VERSION")
            ))
            .with_icon(crate::icon::app_window_icon())
            .with_inner_size(window_size())
            .with_min_inner_size([900.0, 620.0]);
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
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Não foi possível iniciar a janela: {error}"))
}


/// The sender of an e-mail for the document list: the address first, then
/// the name ("Funadesp <f@x.br>" → "f@x.br · Funadesp").
fn sender_line(from: &str) -> String {
    let from = from.trim();
    match (from.rfind('<'), from.rfind('>')) {
        (Some(open), Some(close)) if open < close => {
            let address = from[open + 1..close].trim();
            let name = from[..open].trim().trim_matches('"').trim();
            if name.is_empty() || name.eq_ignore_ascii_case(address) {
                address.to_owned()
            } else {
                format!("{address} · {name}")
            }
        }
        _ => from.to_owned(),
    }
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

/// "dd/mm" of an RFC 3339 moment or a plain date, empty when unreadable.
fn short_date(value: &str) -> String {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value.trim()) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%d/%m")
            .to_string();
    }
    let day = value.trim().get(..10).unwrap_or("");
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| d.format("%d/%m").to_string())
        .unwrap_or_default()
}

/// The pill of a task's deadline: late, today, on time or none.
fn due_pill(ui: &mut egui::Ui, task: &crate::tasks::Task, today_iso: &str) {
    if task.due_date.is_empty() {
        pill(ui, "Sem prazo", Tone::Neutral);
        return;
    }
    let date = crate::labels::date(&task.due_date);
    if task.overdue() {
        pill(ui, format!("Atrasada · {date}"), Tone::Danger);
    } else if task.due_date == today_iso {
        pill(ui, "Vence hoje", Tone::Warn);
    } else {
        pill(ui, date, Tone::Info);
    }
}

/// Side margin of the library panel, which the rows of the list bleed into.
const LIST_BLEED: f32 = 14.0;

/// What one row of the document list shows.
struct DocRow<'a> {
    document: &'a DocumentSummary,
    number: String,
    summary: &'a DocTaskStatusSummary,
    attachments: usize,
    indent: f32,
    selected: bool,
    editing: bool,
    joined: Option<String>,
}

/// One document of the list: number and date, title, then its pills. The
/// whole row is the click target; rows are told apart by thin lines.
fn document_row(ui: &mut egui::Ui, row: &DocRow) -> egui::Response {
    let p = theme::c();
    let avail = ui.available_rect_before_wrap();
    let left = avail.left() - LIST_BLEED;
    let right = avail.right() + LIST_BLEED;
    let pad = S4;
    let content_rect = egui::Rect::from_min_max(
        egui::pos2(left + pad + row.indent, avail.top() + S3 - 2.0),
        egui::pos2(right - pad, avail.bottom()),
    );
    let background = ui.painter().add(egui::Shape::Noop);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    content.spacing_mut().item_spacing = egui::vec2(S2, S1);
    let document = row.document;
    {
        // Number on the left (cut with "…" when long), date on the right.
        let date = short_date(&document.last_seen_at);
        let width = content.available_width();
        let date_galley = content.painter().layout_no_wrap(
            date,
            theme::font(theme::SIZE_SMALL),
            p.muted,
        );
        let mut job = egui::text::LayoutJob::simple_singleline(
            row.number.clone(),
            theme::font_strong(theme::SIZE_SMALL),
            p.primary,
        );
        job.wrap.max_width = (width - date_galley.size().x - S2).max(40.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let number_galley = content.painter().layout_job(job);
        let height = number_galley.size().y.max(date_galley.size().y);
        let (line, _) = content.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        content
            .painter()
            .galley(line.left_top(), number_galley, p.primary);
        content.painter().galley(
            egui::pos2(line.right() - date_galley.size().x, line.top()),
            date_galley,
            p.muted,
        );
    }
    content.add(
        egui::Label::new(RichText::new(&document.title).font(theme::font(theme::SIZE_BODY)))
            .wrap()
            .selectable(false),
    );
    content.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(S1 + 2.0, S1);
        let (text, tone) = row.summary.short_pill();
        pill(ui, text, tone);
        if row.editing {
            pill(ui, "Em edição", Tone::Warn);
        }
        if document.manual {
            pill(
                ui,
                if document.email { "E-mail" } else { "Manual" },
                Tone::Neutral,
            );
        }
        if let Some(text) = &row.joined {
            pill(
                ui,
                text.trim_start_matches('\u{21B3}').trim().to_owned(),
                Tone::Violet,
            );
        }
        if document.joined_children_count > 0 {
            pill(
                ui,
                plural(document.joined_children_count as usize, "juntado", "juntados"),
                Tone::Violet,
            );
        }
        if row.attachments > 0 {
            pill(
                ui,
                format!("{} {}", icon::CLIP, row.attachments),
                Tone::Neutral,
            )
            .on_hover_text("Anexos juntados a este documento");
        }
        match document.priority.as_str() {
            "urgente" => {
                pill(ui, "Prioridade urgente", Tone::Danger);
            }
            "alta" => {
                pill(ui, "Prioridade alta", Tone::Warn);
            }
            _ => {}
        }
        if document.analysis_stale {
            pill(ui, "análise desatualizada", Tone::Warn).on_hover_text(
                "O documento, o contexto ou as notas mudaram depois da última análise.",
            );
        }
    });
    let rect = egui::Rect::from_min_max(
        egui::pos2(left, avail.top()),
        egui::pos2(right, content.min_rect().bottom() + S3 - 2.0),
    );
    // Only the width of the panel is claimed; the click target and the
    // painted row reach the panel edges.
    ui.allocate_rect(
        egui::Rect::from_min_max(
            egui::pos2(avail.left(), rect.top()),
            egui::pos2(avail.right(), rect.bottom()),
        ),
        egui::Sense::hover(),
    );
    let response = ui
        .interact(rect, ui.make_persistent_id("row"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut shapes = Vec::new();
    if row.selected {
        shapes.push(egui::Shape::rect_filled(rect, 0.0, p.primary_soft));
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            0.0,
            p.primary,
        ));
    } else if response.hovered() {
        shapes.push(egui::Shape::rect_filled(rect, 0.0, p.sunken));
    }
    shapes.push(egui::Shape::hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, p.outline),
    ));
    ui.painter().set(background, egui::Shape::Vec(shapes));
    response
}

/// A discreet button that opens an https link of the SIGADOC in the browser;
/// anything else is shown as plain text with the reason on hover.
fn link_button(ui: &mut egui::Ui, label: &str, raw_url: &str) {
    // Only links present in the currently rendered import reach this function.
    if allowed_sigadoc_url(raw_url) {
        if ui
            .add(widgets::quiet(format!("{} {label}", icon::OPEN_OUT)))
            .on_hover_text("Abre no navegador (exige login no SIGADOC).")
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(raw_url));
        }
    } else {
        safe_link(ui, label, raw_url);
    }
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
            place(child, depth + 1, children, placed, out);
        }
    }
    let mut placed = BTreeSet::new();
    let mut out = Vec::with_capacity(docs.len());
    for doc in docs.iter().filter(|d| parent_of(d).is_none()) {
        place(doc, 0, &children, &mut placed, &mut out);
    }
    // Defensive: a cycle of juntadas has no root; keep every document listed.
    for doc in docs {
        place(doc, 0, &children, &mut placed, &mut out);
    }
    out
}

/// The areas of the window, chosen in the rail on the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    Documents,
    Tasks,
    Saiop,
    Templates,
    Settings,
}

/// The window opens at this size (fixed while the pictures of the screens are taken).
fn window_size() -> [f32; 2] {
    #[cfg(feature = "shots")]
    if crate::shots::active() {
        return crate::shots::SIZE;
    }
    [1240.0, 820.0]
}

/// The sections of the Ajustes area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSection {
    Team,
    Mail,
    Ai,
    Backup,
    Trash,
    Appearance,
    About,
}

impl SettingsSection {
    const ALL: [Self; 7] = [
        Self::Team,
        Self::Mail,
        Self::Ai,
        Self::Backup,
        Self::Trash,
        Self::Appearance,
        Self::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Team => "Equipe e sincronização",
            Self::Mail => "E-mail",
            Self::Ai => "Inteligência artificial",
            Self::Backup => "Cópia de segurança e exportação",
            Self::Trash => "Lixeira",
            Self::Appearance => "Aparência",
            Self::About => "Sobre o Ponte",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::Team => "Como este computador compartilha o acervo com outros computadores e com o celular.",
            Self::Mail => "Contas de e-mail para importar mensagens como registros.",
            Self::Ai => "Opcional. Onde a análise e as minutas são geradas.",
            Self::Backup => "Guardar uma cópia do acervo e levar os dados para planilhas.",
            Self::Trash => "O que foi excluído fica aqui por um tempo e pode ser restaurado.",
            Self::Appearance => "Tema claro ou escuro.",
            Self::About => "Versão, guia do usuário, autores e licença.",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Team => icon::TEAM,
            Self::Mail => icon::MAIL,
            Self::Ai => icon::AI,
            Self::Backup => icon::DATABASE,
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

fn load_appearance() -> crate::theme::Appearance {
    appearance_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| crate::theme::Appearance::from_key(text.trim()))
        .unwrap_or(crate::theme::Appearance::System)
}

fn save_appearance(appearance: crate::theme::Appearance) {
    if let Some(path) = appearance_path() {
        let _ = std::fs::write(path, appearance.key());
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
    },
    Backup(std::result::Result<PathBuf, String>),
    Export(std::result::Result<crate::export::ExportReport, String>),
    MailFolders(std::result::Result<Vec<crate::imap::Folder>, String>),
    MailMessages(std::result::Result<Vec<crate::imap::MessageSummary>, String>),
    /// Raw messages by uid, to be imported on the window's thread.
    MailFetched(
        std::result::Result<crate::imap::Fetched, String>,
        crate::mail::MailSource,
    ),
}

/// What the background worker is doing; the screen adapts buttons to it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkKind {
    Sources,
    Analysis,
    Draft,
    Backup,
    Export,
    /// Talking to a mailbox (folders, list, fetch).
    Mail,
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
    Templates,
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
    /// The "Atendimentos" screen.
    Saiop,
    /// The "Modelos" screen.
    Templates,
    /// The "Ajustes" screen.
    Settings,
    /// The "Documento" screen with whatever document is selected.
    Documents,
    /// Open this document (sidebar, "Ir para o documento").
    Select(i64),
    /// Start a blank task for this document.
    NewTask(i64),
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
}

impl Leave {
    /// Intents that replace the task being edited, wherever they come from.
    fn replaces_task(&self) -> bool {
        matches!(
            self,
            Leave::NewTask(_) | Leave::OpenTask(_) | Leave::EditTask(_) | Leave::Suggestion { .. }
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
    receiver: Receiver<std::result::Result<ChatGptSession, String>>,
    cancel: Arc<AtomicBool>,
    authorize_url: String,
}

struct PonteApp {
    draft_tools: crate::draft_tools::DraftTools,
    /// The "Modelos" screen.
    templates: crate::templates_ui::TemplatesPanel,
    /// The generation record being shown in "Material utilizado".
    material: Option<(i64, String)>,
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
    intranet: crate::intranet::IntranetPanel,
    /// The area on screen (the rail on the left chooses it).
    area: Area,
    /// Open tasks that are late or due today, shown on the rail.
    task_badge: usize,
    appearance: crate::theme::Appearance,
    settings_section: SettingsSection,
    #[cfg(feature = "shots")]
    tour: Option<crate::shots::Tour>,
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
    doc_task_filter: DocTaskFilter,
    doc_priority_filter: DocPriorityFilter,
    doc_origin_filter: DocOriginFilter,
    doc_sort: DocSort,
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
            mail: Default::default(),
            attachments: Default::default(),
            sources: Default::default(),
            pdf_reader: Default::default(),
            dossier: Default::default(),
            work_context: Default::default(),
            tasks: Default::default(),
            saiop: Default::default(),
            intranet: Default::default(),
            // The welcome screen (or the opened document) is the first thing seen.
            area: Area::Documents,
            task_badge: 0,
            appearance: load_appearance(),
            settings_section: SettingsSection::Team,
            #[cfg(feature = "shots")]
            tour: crate::shots::Tour::start(),
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
            doc_task_filter: DocTaskFilter::All,
            doc_priority_filter: DocPriorityFilter::All,
            doc_origin_filter: DocOriginFilter::All,
            doc_sort: DocSort::Recent,
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
                            format!("foram acrescentados {} anexos que faltavam", result.attachments_added)
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

    /// The "Importar do e-mail" window. Every exchange with the mailbox
    /// runs on the worker thread; the window only asks and receives.
    fn mail_window(&mut self, ctx: &egui::Context) {
        if !self.mail.open {
            return;
        }
        // The window says what is running: its own exchange with the
        // mailbox, or something else (an AI draft, a backup) to wait for.
        let busy = match &self.worker {
            None => crate::mail_ui::MailBusy::Free,
            Some(worker) if worker.kind == WorkKind::Mail => crate::mail_ui::MailBusy::Mail,
            Some(_) => crate::mail_ui::MailBusy::Other,
        };
        let action = self.mail.show(ctx, &mut self.store, busy);
        match action {
            crate::mail_ui::MailAction::None => {}
            crate::mail_ui::MailAction::ListFolders(account) => {
                self.spawn(
                    format!("Conectando a {}…", account.host),
                    WorkKind::Mail,
                    move |reporter| {
                        WorkResult::MailFolders(
                            crate::imap::list_folders(&account, &reporter)
                                .map_err(|e| format!("{e:#}")),
                        )
                    },
                    ctx,
                );
            }
            crate::mail_ui::MailAction::ListMessages {
                account,
                folder,
                filter,
            } => {
                self.spawn(
                    format!("Listando mensagens de {folder}…"),
                    WorkKind::Mail,
                    move |reporter| {
                        WorkResult::MailMessages(
                            crate::imap::list_messages(&account, &folder, filter, 100, &reporter)
                                .map_err(|e| format!("{e:#}")),
                        )
                    },
                    ctx,
                );
            }
            crate::mail_ui::MailAction::Fetch {
                account,
                folder,
                uids,
            } => {
                // The records say in which account the messages arrived.
                let source = crate::mail::MailSource::Account {
                    name: account.name.clone(),
                    address: account.user.trim().to_owned(),
                };
                self.spawn(
                    format!("Baixando {}…", plural(uids.len(), "mensagem", "mensagens")),
                    WorkKind::Mail,
                    move |reporter| {
                        WorkResult::MailFetched(
                            crate::imap::fetch_messages(&account, &folder, &uids, &reporter)
                                .map_err(|e| format!("{e:#}")),
                            source,
                        )
                    },
                    ctx,
                );
            }
        }
    }

    /// Shows the user guide (compiled into the program) in the browser.
    fn open_guide(&mut self) {
        match crate::guide::open() {
            Ok(_) => self.message("Guia do usuário aberto no navegador."),
            Err(error) => self.error(format!("Não foi possível abrir o guia: {error:#}")),
        }
    }

    /// First screen: how to import, and whether the browser bridge is ready.
    /// First screen: how to import, and whether the browser bridge is ready.
    fn welcome(&mut self, ui: &mut egui::Ui) {
        let status = self.install_status();
        let p = theme::c();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(780.0);
                ui.spacing_mut().item_spacing.y = S4;
                ui.add_space(S2);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::heading(ui, "Seu espaço de trabalho documental");
                    widgets::muted(ui, "Escolha um documento na lista à esquerda, ou importe um novo pelo navegador.");
                });
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S3;
                    widgets::section(ui, "Para importar um documento");
                    let steps = [
                        "Abra o documento no SIGADOC, no Chrome ou no Edge.",
                        "Clique em “Importar para o assistente”, no canto da página.",
                        "O documento aparece aqui com o texto, os PDFs e o histórico.",
                    ];
                    for (index, step) in steps.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(26.0, 26.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(rect.center(), 13.0, p.primary_soft);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                (index + 1).to_string(),
                                theme::font_strong(theme::SIZE_BODY),
                                p.primary,
                            );
                            ui.add(egui::Label::new(*step).wrap());
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        widgets::muted(ui, "Primeira vez por aqui?");
                        if ui
                            .add(widgets::secondary(format!("{} Abrir o guia do usuário", icon::BOOK)))
                            .on_hover_text("Guia curto, em dez seções, aberto no navegador. Também em Ajuda, na barra lateral.")
                            .clicked()
                        {
                            self.open_guide();
                        }
                    });
                });
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S2;
                    widgets::section(ui, "Algo que não está no SIGADOC?");
                    ui.add(egui::Label::new("Uma ligação, um memorando ou um papel podem virar um registro manual, marcado como tal e com tarefas, notas e anexos como qualquer documento.").wrap());
                    ui.add(egui::Label::new("E-mails entram prontos, com os anexos: direto da sua conta de e-mail (o Ponte lista as mensagens e importa as que você marcar, sem alterar nada na conta) ou de um arquivo .eml salvo.").wrap());
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add(widgets::primary(format!("{} Criar registro manual…", icon::PLUS)))
                            .clicked()
                        {
                            self.manual_form.open_new();
                        }
                        if ui
                            .add(widgets::secondary(format!("{} Importar da conta de e-mail…", icon::MAIL)))
                            .on_hover_text("Lista as mensagens da sua conta de e-mail (Gmail com senha de app, por exemplo) para marcar e importar. Só leitura: nada é marcado, movido ou apagado na conta. Na primeira vez, o Ponte pede os dados da conta; depois, eles ficam em Ajustes ▸ E-mail.")
                            .clicked()
                        {
                            let busy = match &self.worker {
                                None => crate::mail_ui::MailBusy::Free,
                                Some(worker) if worker.kind == WorkKind::Mail => {
                                    crate::mail_ui::MailBusy::Mail
                                }
                                Some(_) => crate::mail_ui::MailBusy::Other,
                            };
                            self.mail.open_quick(&self.store, busy);
                        }
                        if ui
                            .add(widgets::secondary("Importar e-mail de um arquivo (.eml)…"))
                            .on_hover_text("Lê um e-mail salvo como arquivo .eml (no Gmail: Mostrar original ▸ Baixar original; no Outlook: Salvar como). Vira um registro do tipo E-mail recebido (número EMAIL-ano/sequência), com os anexos. Nada é lido da sua conta de e-mail.")
                            .clicked()
                        {
                            self.import_eml_dialog();
                        }
                    });
                });
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S2;
                    widgets::section(ui, "Ligação com o navegador");
                    let ok = status.browser_bridge_ok();
                    let browsers = match (status.chrome_registered, status.edge_registered) {
                        (true, true) => "Chrome e Edge",
                        (true, false) => "Chrome",
                        (false, true) => "Edge",
                        (false, false) => "nenhum navegador",
                    };
                    if ok {
                        ui.horizontal_wrapped(|ui| {
                            pill(ui, "Pronto", Tone::Ok);
                            ui.label(format!("O Ponte está registrado para o {browsers}."));
                        });
                        widgets::small_muted(ui, "Se o botão “Importar para o assistente” não aparece no SIGADOC, a extensão ainda não foi carregada no navegador: siga os passos abaixo.");
                    } else if status.chrome_registered || status.edge_registered {
                        notice_line(ui, Tone::Warn, format!("Registrado para o {browsers}, mas o programa apontado não foi encontrado. Reinstale o Ponte."));
                    } else {
                        notice_line(ui, Tone::Danger, "O navegador ainda não foi ligado ao Ponte. Execute o instalador neste computador, com a sua conta do Windows.");
                    }
                    match &status.extension_dir {
                        Some(dir) => {
                            widgets::field_label(ui, "Extensão do navegador");
                            ui.add(
                                egui::Label::new(
                                    RichText::new(dir.display().to_string()).color(p.muted),
                                )
                                .wrap(),
                            );
                            ui.horizontal_wrapped(|ui| {
                                if ui.add(widgets::secondary(format!("{} Copiar caminho", icon::COPY))).on_hover_text("Cole este caminho em “Carregar sem compactação”, na página de extensões do navegador.").clicked() {
                                    ui.ctx().copy_text(dir.display().to_string());
                                    self.message("Caminho copiado. No navegador, abra a página de extensões, ative o Modo do desenvolvedor, clique em “Carregar sem compactação” e cole o caminho.");
                                }
                                if ui.add(widgets::secondary(format!("{} Abrir pasta", icon::FOLDER))).clicked() {
                                    crate::windows_ui::reveal_in_explorer(dir);
                                }
                            });
                        }
                        None => {
                            widgets::small_muted(ui, "A pasta da extensão do navegador não foi encontrada ao lado do programa.");
                        }
                    }
                });
                widgets::small_muted(ui, "A IA é opcional e só funciona quando você clica. Assinaturas e tramitação continuam no SIGADOC.");
            });
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
        if self.templates.dirty() {
            out.push(Pending {
                label: "Modelo de trabalho em edição".into(),
                go: PendingTarget::Templates,
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
                    if unsent == 1 { "1 alteração ainda não enviada".to_owned() } else { format!("{unsent} alterações ainda não enviadas") }
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
        // Leaving the AI section of Ajustes saves it; an invalid
        // configuration keeps the person there.
        if self.area == Area::Settings
            && self.settings_section == SettingsSection::Ai
            && !matches!(leave, Leave::Settings)
            && !self.leave_ai_settings()
        {
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

    fn perform_leave(&mut self, leave: Leave) {
        match leave {
            Leave::Saiop => {
                self.area = Area::Saiop;
            }
            Leave::Templates => {
                self.area = Area::Templates;
            }
            Leave::Settings => self.area = Area::Settings,
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
        }
    }

    /// Opens the read-only window with this document's text and PDFs.
    fn open_consult(&mut self, document_id: i64) {
        match self.store.load_document(document_id) {
            Ok(detail) => self.consult = Some(Consult { detail }),
            Err(error) => self.error(format!(
                "Não foi possível abrir o documento para consulta: {error}"
            )),
        }
    }

    /// Authors, version, licence and where to find the project.
    /// Authors, version, licence and where to find the project.
    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        egui::Window::new("Sobre o Ponte")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = S2;
                ui.label(
                    RichText::new("PONTE")
                        .font(theme::font_strong(24.0))
                        .color(theme::c().primary),
                );
                ui.label("Assistente documental para o SIGADOC");
                widgets::small_muted(
                    ui,
                    format!(
                        "Versão {} • extensão do navegador {}",
                        env!("CARGO_PKG_VERSION"),
                        EXTENSION_VERSION
                    ),
                );
                ui.add_space(S2);
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::overline(ui, "Autoria", None);
                    ui.label("Carlo Ralph De Musis");
                    ui.label("João Gabriel Bezerra Estrela");
                });
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::overline(ui, "Licença", None);
                    ui.add(egui::Label::new("MIT. O código-fonte é aberto e pode ser usado, copiado e modificado livremente, mantendo o aviso de autoria.").wrap());
                });
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::overline(ui, "Código, instalador e notas de versão", None);
                    ui.hyperlink_to(PROJECT_URL, PROJECT_URL);
                });
                ui.add(
                    egui::Label::new(
                        RichText::new("O Ponte guarda os documentos no seu computador, protegidos pela sua conta do Windows. Nada é enviado ao SIGADOC nem à nuvem.")
                            .size(theme::SIZE_SMALL)
                            .color(theme::c().muted),
                    )
                    .wrap(),
                );
                ui.add_space(S1);
                if ui.add(widgets::primary("Fechar")).clicked() {
                    close = true;
                }
            });
        if !open || close {
            self.about_open = false;
        }
    }

    fn consult_window(&mut self, ctx: &egui::Context) {
        let Some(consult) = self.consult.take() else {
            return;
        };
        let detail = &consult.detail;
        let mut open = true;
        let mut close = false;
        egui::Window::new(format!("Consulta • {}", detail.summary.document_number))
            .open(&mut open)
            .default_size([680.0, 560.0])
            .min_size([360.0, 240.0])
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui.add(widgets::secondary("Fechar")).clicked() {
                            close = true;
                        }
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            widgets::title(ui, &detail.summary.title);
                            ui.add(
                                egui::Label::new(
                                    RichText::new("Só leitura: a tarefa continua aberta atrás desta janela. Para editar notas ou minutas, use “Ir para o documento”.")
                                        .size(theme::SIZE_SMALL)
                                        .color(theme::c().muted),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
                ui.add_space(S2);
                egui::ScrollArea::vertical()
                    .id_salt("consult-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S3;
                        widgets::card(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = S2;
                            widgets::section(ui, "Texto importado");
                            if detail.capture.text.is_empty() {
                                notice_line(ui, Tone::Warn, "Esta importação não trouxe texto. Abra o conteúdo no SIGADOC e importe de novo.");
                            } else {
                                let mut text = detail.capture.text.as_str();
                                egui::ScrollArea::horizontal()
                                    .id_salt("consult-text")
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::multiline(&mut text)
                                                .font(egui::TextStyle::Monospace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(18)
                                                .code_editor(),
                                        );
                                    });
                            }
                        });
                        widgets::card(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = S2;
                            widgets::section(ui, "Arquivos (PDF)");
                            self.files_list(ui, detail);
                        });
                    });
            });
        if open && !close {
            self.consult = Some(consult);
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
                format!("⚠ {}", plural(conflicts, "conflito de edição", "conflitos de edição")),
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
                    (crate::theme::success(), "computador principal ativo".into(), full)
                } else {
                    (crate::theme::warning(), "computador principal parado".into(), full)
                }
            }
            crate::intranet::IntranetMode::SharedFolder => {
                if self.intranet.syncing() {
                    (crate::theme::accent(), "sincronizando com a pasta…".into(), full)
                } else if self.intranet.offline {
                    (crate::theme::warning(), pending("pasta fora de alcance"), full)
                } else if outbox > 0 {
                    (crate::theme::warning(), format!("{outbox} a enviar"), full)
                } else if let Some((at, _)) = self.intranet.last_sync_finished {
                    (crate::theme::success(), format!("em dia {}", since(at)), full)
                } else {
                    (crate::theme::accent(), "pasta ainda não sincronizada".into(), full)
                }
            }
            crate::intranet::IntranetMode::Client => {
                if self.intranet.offline {
                    (crate::theme::warning(), pending("sem conexão com o principal"), full)
                } else if let Some((at, _)) = self.intranet.last_sync_finished {
                    (
                        crate::theme::success(),
                        format!("conectado • atualizado {}", since(at)),
                        full,
                    )
                } else {
                    (crate::theme::accent(), "conectando ao principal…".into(), full)
                }
            }
        }
    }

    fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        self.intranet.refresh_client(&mut self.store, &self.db_path);
        match self.store.list_documents(&self.query) {
            Ok(documents) => self.documents = documents,
            Err(error) => self.error(format!("Não foi possível atualizar a lista de documentos: {error}")),
        }
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.task_badge = self
            .store
            .list_tasks()
            .map(|tasks| {
                tasks
                    .iter()
                    .filter(|t| {
                        t.completed_at.is_none()
                            && !t.due_date.is_empty()
                            && t.due_date.as_str() <= today.as_str()
                    })
                    .count()
            })
            .unwrap_or(0);
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
                            self.error(format!("Não foi possível ler os documentos juntados: {error}"));
                        }
                    }
                    detail.summary.joined_children_count = self.joined_children.len();
                    self.detail = Some(detail);
                    match self.store.similar_documents(id, 5) {
                        Ok(items) => self.similar = items,
                        Err(error) => self.error(format!("Não foi possível buscar documentos parecidos: {error}")),
                    }
                }
            }
            Err(error) => self.error(format!("Não foi possível carregar o documento: {error}")),
        }
    }

    fn select(&mut self, id: i64) {
        self.area = Area::Documents;
        self.selected_id = Some(id);
        self.detail = None;
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
            Err(error) => self.error(format!("Não foi possível carregar a configuração da IA: {error}")),
        }
    }

    /// Opens the browser on OpenAI's sign-in page and waits, in the
    /// background, for the authorization to come back to the local port.
    fn start_chatgpt_login(&mut self, ctx: &egui::Context) {
        if self.chatgpt_login.is_some() {
            return;
        }
        self.chatgpt_login_error = None;
        if self.ai.chatgpt_host_id.trim().is_empty() {
            self.ai.chatgpt_host_id = chatgpt::new_host_id();
        }
        let login = match chatgpt::Login::start(&self.ai.chatgpt_host_id, Some(&self.ai.chatgpt)) {
            Ok(login) => login,
            Err(error) => {
                self.chatgpt_login_error = Some(error.to_string());
                return;
            }
        };
        let authorize_url = login.authorize_url.clone();
        if let Err(error) = crate::windows_ui::open_url(&authorize_url) {
            self.chatgpt_login_error = Some(error.to_string());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        match std::thread::Builder::new()
            .name("ponte-chatgpt-login".into())
            .spawn(move || {
                let result = login
                    .wait(&flag, chatgpt::LOGIN_TIMEOUT, crate::windows_ui::open_url)
                    .map_err(|e| e.to_string());
                let _ = sender.send(result);
                repaint.request_repaint();
            }) {
            Ok(_) => {
                self.chatgpt_login = Some(ChatGptLogin {
                    receiver,
                    cancel,
                    authorize_url,
                });
            }
            Err(error) => {
                self.chatgpt_login_error =
                    Some(format!("Não foi possível iniciar o login: {error}"));
            }
        }
    }

    fn poll_chatgpt_login(&mut self, ctx: &egui::Context) {
        let received = self
            .chatgpt_login
            .as_ref()
            .map(|login| login.receiver.try_recv());
        match received {
            Some(Ok(Ok(session))) => {
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
            Some(Ok(Err(error))) => {
                self.chatgpt_login = None;
                self.chatgpt_login_error = Some(error);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.chatgpt_login = None;
                self.chatgpt_login_error = Some("O login não foi concluído. Tente de novo.".into());
            }
            _ => {}
        }
    }

    fn cancel_chatgpt_login(&mut self) {
        if let Some(login) = self.chatgpt_login.take() {
            login.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Forgets the tokens here and asks OpenAI to revoke them (in the
    /// background; the local part never waits for the network).
    fn chatgpt_sign_out(&mut self) {
        let session = self.ai.chatgpt.clone();
        self.set_chatgpt_session(session.without_tokens());
        self.models_for = None;
        self.models.clear();
        if self.save_ai_config() {
            self.message("Sessão do ChatGPT encerrada neste computador. Se quiser, confira em ChatGPT ▸ Configurações ▸ Aplicativos conectados.");
        }
        if session.signed_in() {
            let _ = std::thread::Builder::new()
                .name("ponte-chatgpt-signout".into())
                .spawn(move || {
                    let _ = chatgpt::sign_out(&session);
                });
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
                self.error(format!("Não foi possível salvar a configuração da IA: {error}"));
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
                self.models_error = Some("Não houve resposta ao listar os modelos. Tente de novo.".into());
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
    ) {
        if self.worker.is_some() {
            return;
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
                })
            }
            Err(error) => self.error(format!("Não foi possível iniciar a operação: {error}")),
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
        self.spawn(
            format!("Analisando {}…", detail.summary.document_number),
            WorkKind::Analysis,
            move |reporter| WorkResult::Analysis {
                document_id,
                result: ai::analyze_with_sources(
                    &settings,
                    &capture,
                    capture_id,
                    &human_context,
                    &sources,
                    &reporter,
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
                return;
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
                    return;
                }
                Ok(None) => {
                    self.error("O modelo escolhido não existe mais.");
                    return;
                }
                Err(error) => {
                    self.error(format!("Não foi possível ler o modelo: {error}"));
                    return;
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
        let job = ai::DraftJob {
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
        };
        let number = detail.summary.document_number.clone();
        self.spawn(
            format!("Gerando minuta de {}…", number),
            WorkKind::Draft,
            move |reporter| {
                let outcome = ai::generate_draft(&settings, &job, &reporter);
                WorkResult::Draft {
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
            },
            ctx,
        );
    }

    fn backup(&mut self, ctx: &egui::Context) {
        let db_path = self.db_path.clone();
        self.spawn(
            "Criando a cópia de segurança protegida…".into(),
            WorkKind::Backup,
            move |_| {
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

    /// Writes the CSV files in the background and opens the folder.
    fn export_csv(&mut self, ctx: &egui::Context) {
        let db_path = self.db_path.clone();
        self.spawn(
            "Exportando dados para CSV…".into(),
            WorkKind::Export,
            move |_| {
                let result = Store::open(&db_path)
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
            } => {
                // Every generation is recorded, accepted or refused.
                if let Err(error) = self.store.record_generation(&record) {
                    self.error(format!("Não foi possível registrar esta geração no histórico: {error}"));
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
                        });
                        self.suggestion_open = true;
                        self.message("Sugestão pronta para revisão. Salve uma versão quando o texto estiver adequado.");
                    }
                    Err(error) => self.error(format!("Não foi possível gerar a minuta: {error}")),
                }
            }
            WorkResult::Backup(result) => match result {
                Ok(path) => {
                    self.message(format!("Cópia de segurança concluída: {}", path.display()));
                    self.backup_path = Some(path);
                }
                Err(error) => self.error(format!("Não foi possível fazer a cópia de segurança: {error}")),
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
                        if let Some(id) = last_document {
                            // Through the guard: an unsaved task is never hidden.
                            self.navigate(Leave::Select(id));
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
            WorkResult::Export(result) => match result {
                Ok(report) => {
                    self.message(format!(
                        "Exportação concluída: {}, {} e {}, em {}. No CERES, envie cada arquivo .csv como um conjunto.",
                        plural(report.documents as usize, "documento", "documentos"),
                        plural(report.tasks as usize, "tarefa", "tarefas"),
                        plural(report.attendances as usize, "atendimento", "atendimentos"),
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
            ui.add_space(10.0);
            let notes_dirty = self.editors.values().any(Editor::dirty);
            if rail_item(
                ui,
                icon::DOCUMENTS,
                "Documentos",
                self.area == Area::Documents,
                0,
                notes_dirty,
            )
            .on_hover_text("Documentos importados e registros, com a ficha de cada um. (Ctrl+1)")
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
                tasks.on_hover_text("Há uma tarefa com alterações não salvas. (Ctrl+2)")
            } else if self.task_badge > 0 {
                tasks.on_hover_text(format!(
                    "{} ou para hoje. (Ctrl+2)",
                    plural(
                        self.task_badge as usize,
                        "tarefa atrasada",
                        "tarefas atrasadas"
                    )
                ))
            } else {
                tasks.on_hover_text("Tarefas de todos os documentos. (Ctrl+2)")
            };
            if tasks.clicked() {
                self.go_tasks();
            }
            if rail_item(
                ui,
                icon::ATTENDANCES,
                "Atendimentos",
                self.area == Area::Saiop,
                0,
                self.saiop.dirty(),
            )
            .on_hover_text("Controle de entradas e prazos do setor. (Ctrl+3)")
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
            .on_hover_text("Modelos de trabalho: estrutura, instruções, exemplos e referências para as minutas. (Ctrl+4)")
            .clicked()
            {
                self.navigate(Leave::Templates);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(2.0);
                if rail_item(ui, icon::HELP, "Ajuda", false, 0, false)
                    .on_hover_text("Abre o guia do usuário no navegador. Funciona sem internet.")
                    .clicked()
                {
                    self.open_guide();
                }
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
            });
        });
    }

    /// Ctrl+1 to Ctrl+4 change the area, like the rail.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let pressed = |key: Key| ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, key));
        if pressed(Key::Num1) {
            self.navigate(Leave::Documents);
        } else if pressed(Key::Num2) {
            self.go_tasks();
        } else if pressed(Key::Num3) {
            self.navigate(Leave::Saiop);
        } else if pressed(Key::Num4) {
            self.navigate(Leave::Templates);
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
        self.area = Area::Tasks;
    }

    /// Room around each area. Areas with their own side panels draw to the
    /// edge of the window and keep the room inside.
    fn area_margin(&self) -> egui::Margin {
        match self.area {
            Area::Tasks | Area::Saiop | Area::Settings => egui::Margin::ZERO,
            _ => egui::Margin::symmetric(20, 14),
        }
    }

    /// One block of the Ajustes area: what it is for, and its buttons.
    fn settings_card(ui: &mut egui::Ui, text: &str, add: impl FnOnce(&mut egui::Ui)) {
        widgets::card(ui, |ui| {
            if !text.is_empty() {
                ui.add(egui::Label::new(RichText::new(text).color(theme::c().muted)).wrap());
                ui.add_space(S1);
            }
            ui.horizontal_wrapped(add);
        });
    }

    /// One line of the list of sections, on the left of Ajustes.
    fn settings_nav_item(ui: &mut egui::Ui, glyph: &str, label: &str, selected: bool) -> bool {
        let p = theme::c();
        let size = egui::vec2(ui.available_width(), 36.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if ui.is_rect_visible(rect) {
            if selected {
                ui.painter().rect_filled(rect, 6.0, p.primary_soft);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 6.0, p.sunken);
            }
            let color = if selected { p.primary } else { p.text };
            ui.painter().text(
                egui::pos2(rect.left() + 12.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                glyph,
                theme::font(18.0),
                if selected { p.primary } else { p.muted },
            );
            ui.painter().text(
                egui::pos2(rect.left() + 40.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                if selected {
                    theme::font_strong(theme::SIZE_BODY)
                } else {
                    theme::font(theme::SIZE_BODY)
                },
                color,
            );
        }
        response.clicked()
    }

    /// Goes to a section of Ajustes. Leaving the AI section saves it first;
    /// an invalid AI configuration keeps the person there.
    fn open_settings(&mut self, section: SettingsSection) {
        let leaving_ai = self.area == Area::Settings
            && self.settings_section == SettingsSection::Ai
            && section != SettingsSection::Ai;
        if leaving_ai && !self.leave_ai_settings() {
            return;
        }
        self.settings_section = section;
        if self.area != Area::Settings {
            self.navigate(Leave::Settings);
        }
    }

    /// Other parts of the app still ask for the old windows by raising their
    /// flags (status bar, "Ir para", the tour): each request becomes a visit
    /// to the section of Ajustes that now holds that content.
    fn route_settings_requests(&mut self) {
        if self.intranet.open {
            self.intranet.open = false;
            self.open_settings(SettingsSection::Team);
        }
        if self.trash.open {
            self.trash.open = false;
            self.open_settings(SettingsSection::Trash);
        }
        if self.settings_open {
            self.settings_open = false;
            self.open_settings(SettingsSection::Ai);
        }
    }

    /// The Ajustes area: what used to be scattered in the "Menu".
    fn settings_area(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::c();
        egui::Panel::left("settings-nav")
            .exact_size(264.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(10, 14)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    widgets::heading(ui, "Ajustes");
                });
                ui.add_space(S2);
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut chosen = None;
                for section in SettingsSection::ALL {
                    if Self::settings_nav_item(
                        ui,
                        section.icon(),
                        section.label(),
                        self.settings_section == section,
                    ) {
                        chosen = Some(section);
                    }
                }
                if let Some(section) = chosen {
                    self.open_settings(section);
                }
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ui, |ui| {
                let section = self.settings_section;
                widgets::heading(ui, section.label());
                widgets::muted(ui, section.summary());
                ui.add_space(S2);
                egui::ScrollArea::vertical()
                    .id_salt(("settings-section", section.label()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(820.0);
                        self.settings_section_body(ui, &ctx, section);
                        ui.add_space(S3);
                    });
            });
    }

    fn settings_section_body(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        section: SettingsSection,
    ) {
        use crate::widgets::{primary, secondary};
        match section {
            SettingsSection::Team => {
                if self.intranet.ui(ui, &mut self.store, &self.db_path) {
                    self.refresh();
                }
            }
            SettingsSection::Mail => {
                let mut open_mail = false;
                let mut import_eml = false;
                Self::settings_card(
                    ui,
                    "O Ponte se conecta à sua conta de e-mail (IMAP), lista as mensagens de uma pasta e importa as que você marcar, com os anexos, como registros do tipo E-mail recebido. Para cadastrar uma conta, informe o servidor de e-mail, o usuário e a senha; no Gmail, use uma senha de app. As contas ficam guardadas no acervo deste computador. O Ponte só lê a caixa de e-mail: nada é marcado como lido, movido ou apagado na conta.",
                    |ui| {
                        open_mail = ui
                            .add(primary(format!("{} Contas e importação", icon::MAIL)))
                            .clicked();
                        import_eml = ui
                            .add(secondary("Importar e-mail de um arquivo (.eml)…"))
                            .on_hover_text("Um e-mail salvo como arquivo vira um registro do tipo E-mail recebido, com os anexos.")
                            .clicked();
                    },
                );
                if open_mail {
                    self.mail.open_full();
                }
                if import_eml {
                    self.import_eml_dialog();
                }
            }
            SettingsSection::Ai => self.ai_settings(ui),
            SettingsSection::Backup => {
                let idle = self.worker.is_none();
                let has_backup = self.backup_path.is_some();
                let mut backup = false;
                let mut export = false;
                let mut copy_backup = false;
                widgets::section(ui, "Cópia de segurança");
                Self::settings_card(
                    ui,
                    "Guarda uma cópia protegida do acervo na pasta “backups” deste computador. Ela só abre na mesma conta do Windows.",
                    |ui| {
                        backup = ui
                            .add_enabled(idle, primary("Fazer cópia agora"))
                            .clicked();
                        if has_backup {
                            copy_backup = ui
                                .add(widgets::quiet("Copiar o caminho da última cópia"))
                                .clicked();
                        }
                    },
                );
                ui.add_space(S3);
                widgets::section(ui, "Exportação");
                Self::settings_card(
                    ui,
                    "Grava documentos, tarefas e atendimentos em três arquivos .csv (sem textos, notas ou anexos), prontos para os painéis do CERES ou para o Excel.",
                    |ui| {
                        export = ui
                            .add_enabled(idle, secondary("Exportar dados para CSV…"))
                            .clicked();
                    },
                );
                if backup {
                    self.backup(ctx);
                }
                if export {
                    self.export_csv(ctx);
                }
                if copy_backup {
                    if let Some(path) = &self.backup_path {
                        ctx.copy_text(path.display().to_string());
                        self.message("Caminho copiado.");
                    }
                }
            }
            SettingsSection::Trash => {
                let busy = self.worker.is_some()
                    || self.pdf_reader.busy()
                    || self.exit_confirm
                    || self.replacement.is_some();
                if let Some(change) = self.trash.ui(ui, &mut self.store, busy) {
                    self.apply_trash_change(change);
                }
            }
            SettingsSection::Appearance => {
                let mut chosen = self.appearance;
                widgets::card(ui, |ui| {
                    widgets::field_label(ui, "Tema");
                    ui.horizontal(|ui| {
                        for option in [
                            theme::Appearance::System,
                            theme::Appearance::Light,
                            theme::Appearance::Dark,
                        ] {
                            if widgets::chip(ui, chosen == option, option.label()).clicked() {
                                chosen = option;
                            }
                        }
                    });
                });
                if chosen != self.appearance {
                    self.appearance = chosen;
                    theme::set_appearance(ctx, chosen);
                    save_appearance(chosen);
                }
            }
            SettingsSection::About => {
                let mut about = false;
                let mut guide = false;
                Self::settings_card(
                    ui,
                    &format!(
                        "Ponte {} • Assistente documental. Os dados ficam neste computador, protegidos pela sua conta do Windows.",
                        env!("CARGO_PKG_VERSION")
                    ),
                    |ui| {
                        guide = ui
                            .add(primary(format!("{} Guia do usuário", icon::BOOK)))
                            .on_hover_text("Abre o guia no navegador. Funciona sem internet.")
                            .clicked();
                        about = ui.add(secondary("Autores e licença…")).clicked();
                    },
                );
                if about {
                    self.about_open = true;
                }
                if guide {
                    self.open_guide();
                }
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = theme::c();
        let mut new_manual = false;
        let mut new_eml = false;
        let mut new_mail = false;
        ui.horizontal(|ui| {
            widgets::heading(ui, "Documentos");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = widgets::primary(format!("{} Novo {}", icon::PLUS, icon::DOWN));
                egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                    ui.set_min_width(250.0);
                    if ui
                        .button("Registro manual…")
                        .on_hover_text("Cria um documento que não existe no SIGADOC: ligação, memorando, e-mail, papel. Fica marcado como manual e é compartilhado com a equipe como os demais.")
                        .clicked()
                    {
                        new_manual = true;
                        ui.close();
                    }
                    if ui
                        .button("Importar e-mail de um arquivo (.eml)…")
                        .on_hover_text("Lê um e-mail salvo como arquivo .eml (no Gmail: Mostrar original ▸ Baixar original; no Outlook: Salvar como). Vira um registro do tipo E-mail recebido (número EMAIL-ano/sequência), com os anexos. Nada é lido da sua conta de e-mail.")
                        .clicked()
                    {
                        new_eml = true;
                        ui.close();
                    }
                    if ui
                        .button("Importar da conta de e-mail…")
                        .on_hover_text("Lista as mensagens da sua conta de e-mail na última pasta e filtro usados (por exemplo, as com estrela), para marcar e importar. A configuração fica em Ajustes ▸ E-mail.")
                        .clicked()
                    {
                        new_mail = true;
                        ui.close();
                    }
                });
            });
        });
        if new_manual {
            self.manual_form.open_new();
        }
        if new_eml {
            self.import_eml_dialog();
        }
        if new_mail {
            let busy = match &self.worker {
                None => crate::mail_ui::MailBusy::Free,
                Some(worker) if worker.kind == WorkKind::Mail => crate::mail_ui::MailBusy::Mail,
                Some(_) => crate::mail_ui::MailBusy::Other,
            };
            self.mail.open_quick(&self.store, busy);
        }
        ui.add_space(S1);
        let search_width = ui.available_width();
        let response = widgets::search(
            ui,
            &mut self.query,
            "Buscar número, assunto ou texto",
            search_width,
        )
        .accessible_name("Buscar documentos no acervo");
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
        let attachment_counts = self.store.attachment_counts("document").unwrap_or_default();

        let filters_active = self.doc_task_filter != DocTaskFilter::All
            || self.doc_priority_filter != DocPriorityFilter::All
            || self.doc_origin_filter != DocOriginFilter::All;
        // Filters that only the "Filtros" chip shows; the chip stays filled
        // while one of them (or the panel) is on.
        let hidden_filter_on = matches!(
            self.doc_task_filter,
            DocTaskFilter::OpenOnTime | DocTaskFilter::Completed
        ) || self.doc_priority_filter != DocPriorityFilter::All
            || self.doc_origin_filter != DocOriginFilter::All;
        ui.add_space(S1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(S1 + 2.0, S1 + 2.0);
            if widgets::chip(ui, self.doc_task_filter == DocTaskFilter::All, "Todos").clicked() {
                self.doc_task_filter = DocTaskFilter::All;
            }
            if widgets::chip(ui, self.doc_task_filter == DocTaskFilter::OpenAny, "Com tarefa")
                .on_hover_text(format!("Documentos com tarefa aberta ({count_open_any})"))
                .clicked()
            {
                self.doc_task_filter = DocTaskFilter::OpenAny;
            }
            if count_overdue > 0 || self.doc_task_filter == DocTaskFilter::Overdue {
                if widgets::chip(
                    ui,
                    self.doc_task_filter == DocTaskFilter::Overdue,
                    format!("Atrasados · {count_overdue}"),
                )
                .on_hover_text("Mostrar só documentos com tarefa atrasada")
                .clicked()
                {
                    self.doc_task_filter = DocTaskFilter::Overdue;
                }
            }
            let label = format!(
                "Filtros {}",
                if self.filters_open { icon::UP } else { icon::DOWN }
            );
            let enabled = !self.documents.is_empty() || filters_active;
            if widgets::chip(ui, self.filters_open || hidden_filter_on, label).clicked()
                && enabled
            {
                self.filters_open = !self.filters_open;
            }
        });
        if self.filters_open || hidden_filter_on {
            ui.add_space(S1);
            egui::Frame::new()
                .fill(p.sunken)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = egui::vec2(S1 + 2.0, S1 + 2.0);
                    widgets::field_label(ui, "Tarefas");
                    ui.horizontal_wrapped(|ui| {
                        if widgets::chip(
                            ui,
                            self.doc_task_filter == DocTaskFilter::OpenOnTime,
                            format!("No prazo · {count_on_time}"),
                        )
                        .clicked()
                        {
                            self.doc_task_filter = DocTaskFilter::OpenOnTime;
                        }
                        if widgets::chip(
                            ui,
                            self.doc_task_filter == DocTaskFilter::Completed,
                            format!("Concluídos · {count_completed}"),
                        )
                        .clicked()
                        {
                            self.doc_task_filter = DocTaskFilter::Completed;
                        }
                    });
                    ui.add_space(S1);
                    widgets::field_label(ui, "Prioridade");
                    egui::ComboBox::from_id_salt("doc_priority_filter_combo")
                        .selected_text(self.doc_priority_filter.label())
                        .width(ui.available_width() - 8.0)
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
                    ui.add_space(S1);
                    widgets::field_label(ui, "Ordem");
                    egui::ComboBox::from_id_salt("doc_sort_combo")
                        .selected_text(self.doc_sort.label())
                        .width(ui.available_width() - 8.0)
                        .show_ui(ui, |ui| {
                            for sort in DocSort::ALL {
                                ui.selectable_value(&mut self.doc_sort, sort, sort.label());
                            }
                        })
                        .response
                        .on_hover_text(
                            "Ordena a lista; documentos juntados continuam abaixo do principal.",
                        );
                    let manual_count = self.documents.iter().filter(|d| d.manual).count();
                    if manual_count > 0 || self.doc_origin_filter != DocOriginFilter::All {
                        ui.add_space(S1);
                        widgets::field_label(ui, "Origem");
                        ui.horizontal_wrapped(|ui| {
                            for (value, label) in [
                                (DocOriginFilter::All, "Todos".to_owned()),
                                (DocOriginFilter::Sigadoc, "SIGADOC".to_owned()),
                                (DocOriginFilter::Manual, format!("Manuais ({manual_count})")),
                            ] {
                                if widgets::chip(ui, self.doc_origin_filter == value, label)
                                    .clicked()
                                {
                                    self.doc_origin_filter = value;
                                }
                            }
                        });
                    }
                });
        }

        let mut filtered_docs: Vec<&DocumentSummary> = self
            .documents
            .iter()
            .filter(|doc| {
                let task_ok = summaries
                    .get(&doc.id)
                    .is_none_or(|s| s.matches(self.doc_task_filter));
                let prio_ok = self.doc_priority_filter.matches(&doc.priority);
                let origin_ok = self.doc_origin_filter.matches(doc.manual);
                task_ok && prio_ok && origin_ok
            })
            .collect();
        self.doc_sort.apply(&mut filtered_docs, |id| {
            summaries.get(&id).and_then(|s| s.nearest_due.clone())
        });

        ui.add_space(S1);
        let mut clear = false;
        ui.horizontal(|ui| {
            let count = if filtered_docs.len() == self.documents.len() {
                plural(self.documents.len(), "documento", "documentos")
            } else {
                format!(
                    "{} de {}",
                    filtered_docs.len(),
                    plural(self.documents.len(), "documento", "documentos")
                )
            };
            widgets::small_muted(ui, count);
            if filters_active || !self.query.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(widgets::quiet("Limpar")).clicked() {
                        clear = true;
                    }
                });
            }
        });
        if clear {
            self.doc_task_filter = DocTaskFilter::All;
            self.doc_priority_filter = DocPriorityFilter::All;
            self.doc_origin_filter = DocOriginFilter::All;
            if !self.query.is_empty() {
                self.query.clear();
                self.search_changed = Some(Instant::now());
            }
        }
        ui.add_space(S1);
        // The rows run edge to edge: the line above them spans the panel.
        let line_y = ui.cursor().top();
        let panel_x = ui.max_rect().expand2(egui::vec2(LIST_BLEED, 0.0)).x_range();
        ui.painter()
            .hline(panel_x, line_y, egui::Stroke::new(1.0, p.outline));
        let mut selected = None;
        egui::ScrollArea::vertical().id_salt("document-list").show(ui, |ui| {
            ui.set_clip_rect(ui.clip_rect().expand2(egui::vec2(LIST_BLEED, 0.0)));
            ui.spacing_mut().item_spacing.y = 0.0;
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
                    // An e-mail is recognized by who sent it, not by its
                    // internal number (which stays in the hover and the ficha).
                    let number = if document.email && !document.email_from.trim().is_empty() {
                        sender_line(&document.email_from)
                    } else {
                        document.document_number.clone()
                    };
                    let summary = summaries.get(&document.id).cloned().unwrap_or_default();
                    let row = DocRow {
                        document,
                        number,
                        summary: &summary,
                        attachments: attachment_counts.get(&document.id).copied().unwrap_or(0),
                        indent,
                        selected: self.selected_id == Some(document.id),
                        editing,
                        joined,
                    };
                    let response = document_row(ui, &row);
                    let response = if document.email {
                        response.on_hover_text(format!(
                            "{} • importado em {}",
                            document.document_number,
                            crate::labels::datetime(&document.added_at)
                        ))
                    } else {
                        response
                    };
                    if response.clicked() {
                        selected = Some(document.id);
                    }
                });
            }
            if filtered_docs.is_empty() {
                ui.add_space(S3);
                if self.documents.is_empty() {
                    widgets::muted(
                        ui,
                        if self.query.is_empty() {
                            "Nenhum documento importado ainda."
                        } else {
                            "Nenhum documento encontrado."
                        },
                    );
                    if self.query.is_empty() {
                        ui.add_space(S2);
                        ui.add(
                            egui::Label::new(
                                RichText::new("Abra um documento no SIGADOC e clique em “Importar para o assistente”.")
                                    .size(theme::SIZE_SMALL)
                                    .color(p.muted),
                            )
                            .wrap(),
                        );
                    }
                } else {
                    widgets::muted(ui, "Nenhum documento corresponde à busca ou aos filtros.");
                    ui.add_space(S2);
                    if ui.add(widgets::secondary("Limpar busca e filtros")).clicked() {
                        self.doc_task_filter = DocTaskFilter::All;
                        self.doc_priority_filter = DocPriorityFilter::All;
                        self.doc_origin_filter = DocOriginFilter::All;
                        if !self.query.is_empty() {
                            self.query.clear();
                            self.search_changed = Some(Instant::now());
                        }
                    }
                }
            }
        });
        if let Some(id) = selected {
            self.navigate(Leave::Select(id));
        }
    }

    fn document(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        let p = theme::c();
        let doc_tasks = self.store.list_tasks().unwrap_or_default();
        let today_iso = chrono::Local::now().format("%Y-%m-%d").to_string();
        let task_summary = DocTaskStatusSummary::from_tasks(&doc_tasks, id, &today_iso);
        let (badge_text, badge_tone) = task_summary.pill_parts();
        let source_link_ok = !detail.summary.manual && allowed_sigadoc_url(&detail.capture.source_url);

        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(&detail.summary.document_number)
                        .font(theme::font_strong(theme::SIZE_SMALL))
                        .color(p.muted),
                )
                .selectable(false),
            );
            pill(ui, badge_text, badge_tone);
            if detail.summary.manual {
                let kind = if detail.summary.email {
                    "E-mail importado"
                } else {
                    "Registro manual"
                };
                pill(ui, kind, Tone::Neutral);
            }
            match detail.summary.priority.as_str() {
                "urgente" => {
                    pill(ui, "Prioridade urgente", Tone::Danger);
                }
                "alta" => {
                    pill(ui, "Prioridade alta", Tone::Warn);
                }
                "normal" | "baixa" => {
                    pill(
                        ui,
                        format!(
                            "Prioridade {}",
                            crate::labels::priority(&detail.summary.priority).to_lowercase()
                        ),
                        Tone::Neutral,
                    );
                }
                _ => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(self.worker.is_none(), widgets::danger("Excluir…"))
                    .clicked()
                {
                    self.request_trash(crate::trash::Target {
                        kind: crate::trash::Kind::Document,
                        id,
                    });
                }
                if detail.summary.manual
                    && ui
                        .add(widgets::secondary(format!("{} Editar registro", icon::EDIT)))
                        .on_hover_text("Altera tipo, título, origem, data ou texto. Cada edição fica guardada como uma versão.")
                        .clicked()
                {
                    self.manual_form.open_edit(detail);
                }
                if !detail.summary.manual
                    && ui
                        .add_enabled(
                            source_link_ok,
                            widgets::secondary(format!("{} Abrir no SIGADOC", icon::OPEN_OUT)),
                        )
                        .on_hover_text("Abre a página do documento no SIGADOC, no navegador (exige login).")
                        .on_disabled_hover_text("Link indisponível: somente endereços HTTPS do SIGADOC presentes nesta importação podem ser abertos.")
                        .clicked()
                {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(&detail.capture.source_url));
                }
            });
        });
        ui.add_space(S1);
        widgets::title(ui, &detail.summary.title);
        let mut open_joined = None;
        if let Some(text) = joined_label(&detail.summary) {
            ui.horizontal_wrapped(|ui| {
                let text = match &detail.summary.joined_to_number {
                    Some(number) if detail.summary.joined_to_id.is_some() => {
                        format!("Juntado ao documento {number}")
                    }
                    _ => text.trim_start_matches('\u{21B3}').trim().to_owned(),
                };
                pill(ui, text, Tone::Violet);
                match detail.summary.joined_to_id {
                    Some(parent) => {
                        if ui
                            .add(widgets::secondary("Abrir documento principal"))
                            .on_hover_text("Abre a ficha do documento ao qual este foi juntado")
                            .clicked()
                        {
                            open_joined = Some(parent);
                        }
                    }
                    None if detail.summary.joined_to_number.is_some() => {
                        widgets::small_muted(ui, "Documento principal ainda não importado; importe-o pela extensão para vincular automaticamente.");
                    }
                    None => {
                        widgets::small_muted(ui, "A via indica juntada, mas o número do documento principal não aparece nesta importação.");
                    }
                }
            });
        }
        if !self.joined_children.is_empty() {
            ui.horizontal_wrapped(|ui| {
                pill(
                    ui,
                    plural(
                        self.joined_children.len(),
                        "documento juntado",
                        "documentos juntados",
                    ),
                    Tone::Violet,
                );
                for child in &self.joined_children {
                    if ui
                        .add(widgets::quiet(format!("{} {}", icon::JOINED, child.document_number)))
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
        if detail.summary.manual {
            let versions = self
                .store
                .capture_versions(id)
                .map(|v| v.len())
                .unwrap_or(1);
            let by = detail
                .capture
                .metadata
                .get("registered_by")
                .cloned()
                .unwrap_or_else(|| "esta equipe".into());
            ui.horizontal_wrapped(|ui| {
                widgets::small_muted(ui, format!(
                    "Registro criado no Ponte por {by} · versão {versions}, salva em {} · sem página no SIGADOC",
                    display_date(&detail.capture.captured_at)
                ));
                if detail.summary.document_number == crate::model::MANUAL_PENDING {
                    pill(ui, "Número definitivo será dado pelo computador principal na próxima sincronização", Tone::Warn);
                }
            });
        } else {
            widgets::small_muted(
                ui,
                format!(
                    "Importação nº {} · importado em {}",
                    detail.summary.latest_capture_id,
                    display_date(&detail.capture.captured_at)
                ),
            );
        }
        ui.add_space(S2);
        let notes_dirty = {
            let editor = &self.editors[&id];
            editor.note_dirty() || editor.draft_dirty()
        };
        widgets::tab_row(ui, |ui| {
            if widgets::tab(ui, self.tab == Tab::Overview, "Resumo", false).clicked() {
                self.tab = Tab::Overview;
            }
            if widgets::tab(ui, self.tab == Tab::Document, "Documento", false).clicked() {
                self.tab = Tab::Document;
            }
            let notes_tab = widgets::tab(ui, self.tab == Tab::Notes, "Notas e minutas", notes_dirty);
            if notes_dirty {
                notes_tab.clone().on_hover_text("Há alterações não salvas.");
            }
            if notes_tab.clicked() {
                self.tab = Tab::Notes;
            }
            if widgets::tab(ui, self.tab == Tab::More, "Mais", false).clicked() {
                self.tab = Tab::More;
            }
        });
        ui.add_space(S2);
        egui::ScrollArea::vertical()
            .id_salt(("document-content", id, self.tab as u8))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = S4;
                match self.tab {
                    Tab::Overview => self.overview(ui, detail),
                    Tab::Document => self.document_tab(ui, detail),
                    Tab::Notes => {
                        self.notes(ui, detail);
                        self.drafts(ui, detail);
                    }
                    Tab::More => self.more_tab(ui, detail),
                }
                ui.add_space(S2);
            });
    }

    fn section<R>(
        ui: &mut egui::Ui,
        id: i64,
        title: &str,
        default_open: bool,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<R> {
        widgets::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = S2;
            let state_id = ui.make_persistent_id(("doc-section", id, title));
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                state_id,
                default_open,
            );
            let (_, _, body) = state
                .show_header(ui, |ui| {
                    widgets::section(ui, title);
                })
                .body(|ui| {
                    ui.spacing_mut().item_spacing.y = S2;
                    add(ui)
                });
            body.map(|body| body.inner)
        })
    }

    /// Everything imported from the SIGADOC page, in reading order.
    /// Everything imported from the SIGADOC page, in reading order.
    fn document_tab(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        let manual = detail.summary.manual;
        let text_title = if manual {
            "Texto do registro"
        } else {
            "Texto importado"
        };
        Self::section(ui, id, text_title, true, |ui| {
            ui.horizontal_wrapped(|ui| {
                if manual {
                    widgets::small_muted(ui, "Texto digitado por quem criou o registro. Para alterá-lo, use “Editar registro” no topo.");
                } else {
                    widgets::small_muted(ui, "Texto da página e dos PDFs lidos.");
                    info(ui, "Parágrafos, listas e tabelas mantêm a organização da página. Para a aparência original, abra o PDF na seção Arquivos.");
                }
            });
            if detail.capture.text.is_empty() {
                if manual {
                    widgets::muted(ui, "Registro sem texto. Use “Editar registro” para acrescentar, ou anexe o arquivo original abaixo.");
                } else {
                    notice_line(ui, Tone::Warn, "Esta importação não trouxe texto. Abra o conteúdo no SIGADOC e importe de novo.");
                }
            } else {
                let mut text = detail.capture.text.as_str();
                let width = ui.available_width().min(900.0);
                let edit = egui::TextEdit::multiline(&mut text)
                    .desired_width(width)
                    .desired_rows(24);
                // Typed text reads in the ordinary font; the imported page keeps
                // the alignment of its tables, so it stays in fixed width.
                let edit = if manual {
                    edit
                } else {
                    edit.font(egui::TextStyle::Monospace).code_editor()
                };
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.add(edit);
                });
            }
        });
        let has_files = self
            .store
            .list_files(detail.summary.latest_capture_id)
            .map(|f| !f.is_empty())
            .unwrap_or(false);
        if !manual {
            Self::section(ui, id, "Arquivos (PDF)", has_files, |ui| {
                self.files_list(ui, detail)
            });
        }
        let has_attachments = self
            .store
            .list_attachments(crate::attachments::Owner::Document(id))
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let busy = self.worker.is_some();
        Self::section(ui, id, "Anexos", has_attachments, |ui| {
            ui.horizontal_wrapped(|ui| {
                info(ui, "Arquivos do seu computador juntados a este documento (ofício em Word, foto de um despacho, e-mail). Ficam guardados protegidos no acervo e visíveis para a equipe. Nada é enviado ao SIGADOC.");
                widgets::small_muted(ui, "Arquivos anexados por pessoas; os PDFs do SIGADOC ficam na seção acima.");
            });
            self.attachments.show(
                ui,
                &mut self.store,
                crate::attachments::Owner::Document(id),
                busy,
            );
        });
        if let Some((attachment, label)) = self.attachments.take_pdf_request() {
            self.pdf_reader
                .open_attachment(&self.store, attachment, &label, ui.ctx());
        }
        if let Some(target) = self.attachments.take_trash_request() {
            self.request_trash(target);
        }
        if manual {
            Self::section(ui, id, "Dados do registro", true, |ui| {
                manual_facts(ui, detail)
            });
        } else {
            Self::section(ui, id, "Dados do SIGADOC", false, |ui| {
                crate::work_context::provenance(ui, detail);
            });
            Self::section(ui, id, "Histórico de tramitação", false, |ui| {
                crate::dossier::show_history(ui, &detail.capture, id);
            });
        }
    }

    /// The "Registro manual" window.
    /// The "Registro manual" window.
    fn manual_window(&mut self, ctx: &egui::Context) {
        if !self.manual_form.open {
            return;
        }
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let editing = self.manual_form.id.is_some();
        let title = if editing {
            format!("Editar registro manual {}", self.manual_form.number)
        } else {
            "Novo registro manual".to_owned()
        };
        egui::Window::new(title)
            .open(&mut open)
            .default_width(640.0)
            .resizable(true)
            .show(ctx, |ui| {
                let form = &mut self.manual_form;
                ui.spacing_mut().item_spacing.y = S2;
                notice_line(ui, Tone::Warn, "Registro criado por você, fora do SIGADOC. Ele fica marcado como manual em todo o Ponte e é compartilhado com a equipe como os demais documentos.");
                if !editing {
                    widgets::small_muted(ui, "Recebe um número próprio (MANUAL-ano/sequência) ao salvar. Anexe o arquivo original em Documento ▸ Anexos depois de salvar; aqui, resuma o que importa.");
                }
                ui.add_space(S1);
                widgets::field_label(ui, "Tipo");
                egui::ComboBox::from_id_salt("manual-kind")
                    .selected_text(crate::manual::kind_label(&form.fields.kind))
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (key, label) in crate::manual::KINDS {
                            ui.selectable_value(&mut form.fields.kind, (*key).to_string(), *label);
                        }
                    });
                widgets::field_label(ui, "Título");
                ui.add(egui::TextEdit::singleline(&mut form.fields.title).hint_text("ex.: Ligação do gabinete sobre a reforma").desired_width(f32::INFINITY));
                ui.columns(2, |columns| {
                    widgets::field_label(&mut columns[0], "Origem (de quem, de onde)");
                    columns[0].add(egui::TextEdit::singleline(&mut form.fields.origin).hint_text("órgão, setor ou pessoa").desired_width(f32::INFINITY));
                    widgets::field_label(&mut columns[1], "Data do documento");
                    crate::labels::date_field(&mut columns[1], "manual-document-date", &mut form.fields.document_date);
                });
                widgets::field_label(ui, "Texto (o que foi pedido, decidido ou recebido)");
                ui.add(
                    egui::TextEdit::multiline(&mut form.fields.text)
                        .desired_rows(12)
                        .desired_width(f32::INFINITY),
                );
                if !form.message.is_empty() {
                    notice_line(ui, Tone::Danger, form.message.clone());
                }
                ui.add_space(S1);
                ui.horizontal(|ui| {
                    let can_save = !editing || form.dirty();
                    save = ui.add_enabled(can_save, widgets::primary(if editing { "Salvar nova versão" } else { "Criar registro" })).clicked();
                    cancel = ui.add(widgets::secondary("Cancelar")).clicked();
                    if editing && !form.dirty() {
                        widgets::small_muted(ui, "sem alterações");
                    }
                });
            });
        if save {
            let fields = self.manual_form.fields.clone();
            let result = match self.manual_form.id {
                None => self
                    .store
                    .create_manual_document(&fields)
                    .map(|r| (r, true)),
                Some(id) => self
                    .store
                    .update_manual_document(id, &fields)
                    .map(|r| (r, false)),
            };
            match result {
                Ok((receipt, created)) => {
                    self.manual_form = ManualForm::default();
                    self.refresh();
                    self.select(receipt.document_id);
                    self.tab = Tab::Document;
                    let number = self
                        .documents
                        .iter()
                        .find(|d| d.id == receipt.document_id)
                        .map(|d| d.document_number.clone())
                        .unwrap_or_default();
                    if created {
                        self.message(format!("Registro manual {number} criado. Anexe o arquivo original em Documento ▸ Anexos, se houver."));
                    } else if crate::manual::unchanged(&receipt) {
                        self.message("Nada mudou no registro; nenhuma versão nova foi salva.");
                    } else {
                        self.message(format!("Nova versão do registro {number} salva. As anteriores ficam em Mais ▸ Versões."));
                    }
                }
                Err(error) => self.manual_form.message = error.to_string(),
            }
        } else if cancel {
            self.manual_form = ManualForm::default();
        } else if !open {
            if self.manual_form.dirty() {
                // The X never discards silently.
                self.manual_form.message =
                    "Há alterações não salvas: clique em Salvar ou em Cancelar para descartar."
                        .into();
            } else {
                self.manual_form = ManualForm::default();
            }
        }
    }

    /// Local relations, versions and technical details.
    fn more_tab(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        Self::section(
            ui,
            id,
            "Vínculos locais e versões importadas",
            true,
            |ui| {
                if let Some(other) = self.dossier.show(ui, &mut self.store, id) {
                    self.select(other);
                }
                if let Some(target) = self.dossier.take_trash_request() {
                    self.request_trash(target);
                }
            },
        );
        if !detail.summary.manual {
            Self::section(ui, id, "O que foi importado e referências", false, |ui| {
                self.coverage_and_references(ui, detail);
            });
        }
        Self::section(ui, id, "Dados técnicos", false, |ui| {
            self.technical_data(ui, detail);
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        ui.spacing_mut().item_spacing = egui::vec2(S4, S4);
        if ui.available_width() >= 980.0 {
            ui.columns(2, |columns| {
                self.analysis_section(&mut columns[0], detail);
                self.document_summary(&mut columns[0], detail);
                self.document_tasks(&mut columns[1], detail);
                self.saiop_links(&mut columns[1], detail);
                self.overview_note(&mut columns[1], detail);
            });
        } else {
            self.analysis_section(ui, detail);
            self.document_summary(ui, detail);
            self.document_tasks(ui, detail);
            self.saiop_links(ui, detail);
            self.overview_note(ui, detail);
        }
        ui.horizontal(|ui| {
            widgets::overline(ui, "Antes de usar a IA", None);
            info(ui, "O contexto e as fontes abaixo são opcionais. Quando preenchidos, a análise e a minuta levam em conta o que você informou.");
        });
        let changed = widgets::card(ui, |ui| {
            self.work_context
                .show(ui, &mut self.store, detail.summary.id)
        });
        if changed {
            self.reload(detail.summary.id);
        }
        let documents = widgets::card(ui, |ui| {
            self.sources
                .show(ui, &self.store, detail.summary.id, self.worker.is_some())
        });
        if let Some(documents) = documents {
            self.prepare_sources(detail.summary.id, documents, ui.ctx());
        }
        widgets::card(ui, |ui| {
            egui::CollapsingHeader::new(
                RichText::new("Documentos parecidos no acervo")
                    .font(theme::font_strong(theme::SIZE_SECTION)),
            )
            .id_salt(("similar", detail.summary.id))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::small_muted(ui, "Semelhança por palavras em comum.");
                    info(ui, "Não indica vínculo oficial entre os documentos.");
                });
                let mut selected = None;
                for similar in &self.similar {
                    if ui
                        .add(widgets::quiet(format!(
                            "{} — {}",
                            similar.document_number, similar.title
                        )))
                        .clicked()
                    {
                        selected = Some(similar.id);
                    }
                }
                if self.similar.is_empty() {
                    widgets::muted(ui, "Nenhum documento parecido encontrado.");
                }
                if let Some(id) = selected {
                    self.select(id);
                }
            });
        });
    }

    /// The saved note of the document, with the way to edit it.
    fn overview_note(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let note = self
            .editors
            .get(&detail.summary.id)
            .map(|editor| editor.note.clone())
            .unwrap_or_else(|| detail.note.clone());
        let mut edit = false;
        titled_card(ui, "Nota", |_| {}, |ui| {
            if note.trim().is_empty() {
                widgets::muted(ui, "Nenhuma nota ainda.");
            } else {
                ui.add(
                    egui::Label::new(RichText::new(note.trim()).color(theme::c().muted)).wrap(),
                );
            }
            if ui.add(widgets::quiet("Editar nota")).clicked() {
                edit = true;
            }
        });
        if edit {
            self.tab = Tab::Notes;
        }
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
        let mut open_attendance = None;
        let title = if mine.len() == 1 {
            "Atendimento"
        } else {
            "Atendimentos"
        };
        titled_card(ui, title, |_| {}, |ui| {
            for (index, att) in mine.iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::quiet("Abrir")).clicked() {
                            open_attendance = Some(att.id);
                        }
                        let tone = match att.status.as_str() {
                            "concluida" => Tone::Ok,
                            "aguardando" => Tone::Violet,
                            _ => Tone::Info,
                        };
                        pill(ui, crate::saiop::status_label(&att.status), tone);
                        if let Some(delta) = att.due_delta_days(today) {
                            if delta > 0 {
                                pill(ui, format!("vencido há {delta} d"), Tone::Danger);
                            } else if delta == 0 {
                                pill(ui, "vence hoje", Tone::Warn);
                            } else {
                                pill(
                                    ui,
                                    format!("prazo {}", crate::labels::date(&att.due_date)),
                                    Tone::Neutral,
                                );
                            }
                        }
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            let name = if att.reference_raw.is_empty() {
                                att.subject.clone()
                            } else {
                                att.reference_raw.clone()
                            };
                            ui.add(egui::Label::new(name).wrap());
                            if !att.assignee.is_empty() {
                                widgets::small_muted(ui, format!("resp.: {}", att.assignee));
                            }
                        });
                    });
                });
            }
        });
        if let Some(att_id) = open_attendance {
            if self.saiop.focus_attendance(&self.store, att_id) {
                self.area = Area::Saiop;
            } else {
                self.error("Há um atendimento com alterações não salvas em Atendimentos. Salve ou descarte antes de abrir outro.");
            }
        }
    }

    fn analysis_section(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let busy = self.worker.is_some();
        let has_analysis = detail.analysis.is_some();
        let label = match &self.worker {
            Some(worker) if worker.kind == WorkKind::Analysis => {
                format!("Analisando… {}", worker.short_step())
            }
            _ if has_analysis => "Analisar de novo".to_owned(),
            _ => format!("{} Analisar", icon::AI),
        };
        let hover = self.ai_disclosure_text(false);
        let mut analyze = false;
        titled_card(
            ui,
            "Análise",
            |ui| {
                let button = if has_analysis {
                    widgets::quiet(label)
                } else {
                    widgets::primary(label)
                };
                if ui
                    .add_enabled(!busy, button)
                    .on_hover_text(hover)
                    .clicked()
                {
                    analyze = true;
                }
            },
            |ui| {
                let Some(analysis) = &detail.analysis else {
                    widgets::muted(
                        ui,
                        "Ainda sem análise. Importar, buscar e anotar funcionam sem IA.",
                    );
                    self.ai_destination(ui);
                    return;
                };
                let stale = detail.summary.analysis_stale
                    || self.work_context.document_dirty(detail.summary.id)
                    || self.editors[&detail.summary.id].note_dirty();
                if stale {
                    notice_line(ui, Tone::Warn, format!("Análise da importação nº {}: o documento, o contexto ou as notas mudaram. Salve as edições e analise novamente.", analysis.capture_id));
                }
                ui.add(egui::Label::new(&analysis.summary).wrap());
                if let Some(deadline) = &analysis.deadline_candidate {
                    ui.horizontal_wrapped(|ui| {
                        pill(ui, format!("Prazo: {deadline}"), Tone::Warn);
                        widgets::small_muted(ui, "Confirme a data e a regra na fonte.");
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    widgets::field_label(ui, "Prioridade sugerida");
                    let tone = match analysis.priority.as_str() {
                        "urgente" => Tone::Danger,
                        "alta" => Tone::Warn,
                        _ => Tone::Neutral,
                    };
                    pill(ui, crate::labels::priority(&analysis.priority), tone);
                    if !analysis.subjects.is_empty() {
                        widgets::muted(ui, analysis.subjects.join(" • "));
                    }
                });
                ui.add(egui::Label::new(format!("Motivo: {}", analysis.reason)).wrap());
                let mut prepare: Option<crate::model::ActionSuggestion> = None;
                let mut open_task: Option<crate::tasks::Task> = None;
                if !analysis.suggested_actions.is_empty() {
                    ui.separator();
                    widgets::field_label(ui, "Providências sugeridas (revise antes de usar)");
                    // A suggestion already turned into a task of this document
                    // is shown folded, with the task, so it is not prepared twice
                    // by mistake (another one can still be prepared on purpose).
                    let document_tasks: Vec<crate::tasks::Task> = self
                        .store
                        .list_tasks()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|t| t.document_id == detail.summary.id)
                        .collect();
                    for (index, suggestion) in analysis.suggested_actions.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        let existing = document_tasks
                            .iter()
                            .find(|t| t.action == suggestion.action);
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Min),
                                |ui| {
                                    if let Some(task) = existing {
                                        if ui
                                            .add_enabled(!stale, widgets::quiet("Preparar outra"))
                                            .on_hover_text(
                                                "Cria mais uma tarefa com esta mesma providência.",
                                            )
                                            .clicked()
                                        {
                                            prepare = Some(suggestion.clone());
                                        }
                                        if ui.add(widgets::quiet("Abrir tarefa")).clicked() {
                                            open_task = Some(task.clone());
                                        }
                                    } else if ui
                                        .add_enabled(!stale, widgets::secondary("Criar tarefa"))
                                        .on_hover_text(format!(
                                            "Prepara uma tarefa: {}",
                                            crate::labels::task_action(&suggestion.action)
                                        ))
                                        .clicked()
                                    {
                                        prepare = Some(suggestion.clone());
                                    }
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(crate::labels::task_action(&suggestion.action)).font(
                                                        theme::font_strong(theme::SIZE_BODY),
                                                    ),
                                                )
                                                .wrap(),
                                            );
                                            match existing {
                                                Some(task) => {
                                                    ui.horizontal_wrapped(|ui| {
                                                        pill(ui, "Tarefa criada", Tone::Ok);
                                                        widgets::small_muted(
                                                            ui,
                                                            task.title.as_str(),
                                                        );
                                                    });
                                                }
                                                None => {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(&suggestion.reason)
                                                                .color(theme::c().muted),
                                                        )
                                                        .wrap(),
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(format!(
                                                                "Base indicada: {}",
                                                                suggestion.evidence
                                                            ))
                                                            .size(theme::SIZE_SMALL)
                                                            .color(theme::c().muted),
                                                        )
                                                        .wrap(),
                                                    );
                                                }
                                            }
                                        },
                                    );
                                },
                            );
                        });
                    }
                }
                if let Some(task) = open_task {
                    self.navigate(Leave::OpenTask(task));
                }
                if let Some(suggestion) = prepare {
                    self.navigate(Leave::Suggestion {
                        document_id: detail.summary.id,
                        suggestion,
                        priority: analysis.priority.clone(),
                    });
                }
                if !analysis.citations.is_empty() {
                    ui.separator();
                    widgets::field_label(ui, "Trechos conferidos nas fontes selecionadas");
                    widgets::small_muted(ui, "O programa confere a presença literal dos trechos; a interpretação da IA ainda exige revisão.");
                    for citation in &analysis.citations {
                        if let Some(source) = analysis
                            .materials
                            .iter()
                            .find(|s| s.id == citation.source_id)
                        {
                            egui::Frame::new()
                                .fill(theme::c().sunken)
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add(egui::Label::new(&citation.claim).wrap());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&source.label)
                                                .font(theme::font_strong(theme::SIZE_BODY)),
                                        )
                                        .wrap(),
                                    );
                                    ui.add(
                                        egui::Label::new(format!("Trecho: {}", citation.quote))
                                            .wrap(),
                                    );
                                    widgets::small_muted(ui, format!(
                                        "Fonte {} • importação nº {} • método {}",
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
                    ui.separator();
                    widgets::field_label(ui, "Informações a conferir");
                    for missing in &analysis.missing_context {
                        ui.add(egui::Label::new(format!("• {missing}")).wrap());
                    }
                }
                if !analysis.human_context.is_empty() {
                    ui.collapsing("Contexto salvo utilizado nesta análise", |ui| {
                        crate::work_context::show_analysis_context(ui, &analysis.human_context);
                    });
                }
                widgets::small_muted(
                    ui,
                    format!(
                        "Modelo de IA {} • {}",
                        analysis.model,
                        display_date(&analysis.created_at)
                    ),
                );
                self.ai_destination(ui);
            },
        );
        if analyze {
            self.analyze(detail, ui.ctx());
        }
    }

    /// Attendances of the cabinet control that point at this document.

    fn files_list(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        match self.store.list_files(detail.summary.latest_capture_id) {
            Ok(files) if files.is_empty() => {
                widgets::muted(ui, "Nenhum PDF veio com esta importação.");
                widgets::small_muted(ui, "Se o documento tem PDF no SIGADOC, importe de novo pela extensão para tentar baixá-lo.");
            }
            Ok(files) => {
                ui.horizontal_wrapped(|ui| {
                    info(ui, "“Ler no Ponte” abre a cópia local, sem precisar do SIGADOC. “Abrir no SIGADOC” vai à versão oficial no navegador (exige login). “Exportar e abrir pasta” cria uma cópia legível na pasta “exportados” deste computador.");
                    widgets::small_muted(ui, "Cópias baixadas na importação, guardadas protegidas no acervo.");
                });
                for (index, file) in files.into_iter().enumerate() {
                    if index > 0 {
                        ui.separator();
                    }
                    let origin = detail.capture.references.iter().find(|r| r.url == file.url);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(icon::PDF)
                                .size(18.0)
                                .color(theme::c().primary),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(&file.label)
                                    .font(theme::font_strong(theme::SIZE_BODY)),
                            )
                            .wrap(),
                        );
                        if let Some(reference) = origin {
                            pill(
                                ui,
                                crate::labels::downloaded_kind(&reference.kind),
                                Tone::Neutral,
                            );
                        }
                        pill(ui, format!("{} KB", file.size.div_ceil(1024)), Tone::Neutral);
                        widgets::small_muted(ui, file.text_status.as_str());
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(widgets::secondary("Ler no Ponte")).clicked() {
                            self.pdf_reader.open(
                                &self.store,
                                file.id,
                                detail.summary.latest_capture_id,
                                &file.label,
                                ui.ctx(),
                            );
                        }
                        if ui
                            .add(widgets::secondary("Exportar e abrir pasta"))
                            .clicked()
                        {
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
                                Err(e) => self.error(format!("Não foi possível exportar o PDF: {e}")),
                            }
                        }
                        if let Some(reference) = origin {
                            link_button(ui, "Abrir no SIGADOC", &reference.url);
                        }
                        widgets::small_muted(
                            ui,
                            format!(
                                "identificação {}…",
                                &file.sha256[..file.sha256.len().min(12)]
                            ),
                        )
                        .on_hover_text(format!("Identificação completa do arquivo (SHA-256): {}", file.sha256));
                    });
                }
            }
            Err(e) => {
                notice_line(ui, Tone::Warn, format!("Não foi possível ler os arquivos: {e}"));
            }
        }
    }

    fn coverage_and_references(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        ui.label(format!(
            "{} caracteres de texto • {}",
            detail.capture.text.chars().count(),
            plural(detail.capture.references.len(), "referência", "referências")
        ));
        if detail.capture.coverage.recognized.is_empty() {
            notice_line(ui, Tone::Warn, "Nenhuma parte da página foi marcada como reconhecida.");
        } else {
            ui.label("Reconhecido na página:");
            for item in &detail.capture.coverage.recognized {
                ui.label(format!("• {item}"));
            }
        }
        if !detail.capture.coverage.pending.is_empty() {
            widgets::field_label(ui, "Observações da importação:");
            for pending in &detail.capture.coverage.pending {
                ui.label(format!("• {pending}"));
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            widgets::field_label(ui, "Links encontrados na página");
            info(ui, "Abrem o SIGADOC no navegador e exigem login. Os PDFs que foram baixados na importação têm a cópia local em Documento ▸ Arquivos.");
        });
        if detail.capture.references.is_empty() {
            widgets::small_muted(ui, "Nenhum link foi identificado nesta página.");
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
                    pill(ui, reference_kind(&reference.kind), Tone::Neutral);
                    link_button(ui, &reference.label, &reference.url);
                    if let Some(file) = downloaded.iter().find(|f| f.url == reference.url) {
                        if ui
                            .add(widgets::secondary("Ler cópia baixada no Ponte"))
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
                ![
                    "movement_history",
                    "movement_source",
                    "downloaded_files_sha256",
                ]
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
        titled_card(
            ui,
            "Dados do documento",
            |ui| {
                info(ui,"Os dados oficiais são um retrato do momento da importação. Finalidade, assuntos e prioridade locais não alteram o SIGADOC.");
            },
            |ui| {
                let none = "não disponível na importação";
                let mut rows: Vec<(&str, String)> = vec![
                    (
                        "Tipo",
                        metadata
                            .get("document_type")
                            .cloned()
                            .unwrap_or_else(|| none.into()),
                    ),
                    (
                        "Data documental",
                        metadata
                            .get("document_date")
                            .cloned()
                            .unwrap_or_else(|| none.into()),
                    ),
                ];
                if let Some(value) = metadata.get("medium") {
                    rows.push(("Suporte", value.clone()));
                }
                if detail.summary.email {
                    // Where the e-mail arrived: the account it was downloaded
                    // from, or (from a .eml file, or imported before this was
                    // kept) the addresses it was sent to.
                    let account = metadata.get("mail_account").filter(|a| !a.trim().is_empty());
                    match (account, metadata.get("mail_account_name")) {
                        (Some(address), Some(name)) if !name.trim().is_empty() => {
                            rows.push(("Recebido na conta", format!("{name} ({address})")));
                        }
                        (Some(address), _) => rows.push(("Recebido na conta", address.clone())),
                        (None, _) => match metadata.get("mail_to").filter(|t| !t.trim().is_empty()) {
                            Some(to) => rows.push(("Destinatário", to.clone())),
                            None => rows.push(("Conta de recebimento", "não registrada".to_owned())),
                        },
                    }
                    if let Some(from) = metadata.get("mail_from").filter(|f| !f.trim().is_empty()) {
                        rows.push(("Remetente", from.clone()));
                    }
                }
                if let Some(value) = metadata.get("document_model") {
                    rows.push(("Modelo do SIGADOC", value.clone()));
                }
                if let Some(value) = metadata.get("via_status") {
                    rows.push(("Situação no SIGADOC", value.clone()));
                }
                rows.push((
                    "Finalidade",
                    crate::work_context::purpose_label(&detail.work_context.purpose).to_owned(),
                ));
                if !detail.work_context.subjects.trim().is_empty() {
                    rows.push(("Assuntos", detail.work_context.subjects.clone()));
                }
                if !detail.work_context.priority.is_empty() {
                    rows.push((
                        "Prioridade local",
                        crate::tasks::priority_label(&detail.work_context.priority).to_owned(),
                    ));
                }
                match self.store.list_files(detail.summary.latest_capture_id) {
                    Ok(files) => {
                        rows.push((
                            "Arquivos",
                            format!(
                                "{} • {} caracteres de texto • {}",
                                plural(files.len(), "PDF", "PDFs"),
                                detail.capture.text.chars().count(),
                                plural(detail.capture.references.len(), "referência", "referências")
                            ),
                        ));
                        facts(ui, ("doc-facts", detail.summary.id), &rows);
                        if let Some(value) = metadata.get("classification") {
                            let code = value.split(" - ").next().unwrap_or(value);
                            ui.collapsing(
                                format!("Classificação documental oficial: {code}"),
                                |ui| {
                                    ui.add(egui::Label::new(value.as_str()).wrap());
                                    widgets::small_muted(ui, "Código do SIGADOC; distinto de assuntos, finalidade e prioridade locais.");
                                },
                            );
                        }
                        if !detail.capture.coverage.pending.is_empty() {
                            ui.horizontal(|ui| {
                                pill(
                                    ui,
                                    format!(
                                        "{} da importação",
                                        plural(
                                            detail.capture.coverage.pending.len(),
                                            "observação",
                                            "observações"
                                        )
                                    ),
                                    Tone::Warn,
                                );
                                if ui.add(widgets::quiet("Ver")).clicked() {
                                    self.tab = Tab::More;
                                }
                            });
                        }
                        let original = files.iter().find(|file| {
                            detail
                                .capture
                                .references
                                .iter()
                                .any(|r| r.url == file.url && r.kind == "original")
                        });
                        ui.horizontal(|ui| {
                            if let Some(file) = original {
                                if ui
                                    .add(widgets::secondary("Ler o PDF"))
                                    .on_hover_text("PDF do documento baixado na importação, aberto no leitor do Ponte sem precisar do SIGADOC. O mesmo arquivo está em Documento ▸ Arquivos, com a opção de abri-lo no SIGADOC.")
                                    .clicked()
                                {
                                    self.pdf_reader.open(
                                        &self.store,
                                        file.id,
                                        detail.summary.latest_capture_id,
                                        &file.label,
                                        ui.ctx(),
                                    );
                                }
                            }
                            if ui
                                .add(widgets::quiet("Completar contexto"))
                                .on_hover_text("Finalidade, assuntos, prioridade e orientações que só você sabe.")
                                .clicked()
                            {
                                self.work_context.reveal(detail.summary.id);
                            }
                        });
                    }
                    Err(error) => {
                        facts(ui, ("doc-facts", detail.summary.id), &rows);
                        notice_line(
                            ui,
                            Tone::Warn,
                            format!("Não foi possível conferir os arquivos locais: {error}"),
                        );
                    }
                }
            },
        );
    }

    fn document_tasks(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let mut new_task = false;
        let mut see_all = false;
        let mut complete: Option<i64> = None;
        let mut edit: Option<crate::tasks::Task> = None;
        let today_iso = chrono::Local::now().format("%Y-%m-%d").to_string();
        titled_card(
            ui,
            "Tarefas",
            |ui| {
                if ui
                    .add(widgets::primary(format!("{} Nova tarefa", icon::PLUS)))
                    .clicked()
                {
                    new_task = true;
                }
                if ui.add(widgets::quiet("Ver todas")).clicked() {
                    see_all = true;
                }
            },
            |ui| match self.store.list_tasks() {
                Ok(tasks) => {
                    let tasks = tasks
                        .into_iter()
                        .filter(|task| task.document_id == detail.summary.id)
                        .collect::<Vec<_>>();
                    let pending = tasks
                        .iter()
                        .filter(|task| task.status != "concluida")
                        .count();
                    if tasks.is_empty() {
                        widgets::muted(ui, "Nenhuma tarefa ainda.");
                    } else {
                        widgets::small_muted(
                            ui,
                            format!(
                                "{} • {}",
                                plural(pending, "pendente", "pendentes"),
                                plural(tasks.len() - pending, "concluída", "concluídas")
                            ),
                        );
                    }
                    if pending > 5 {
                        widgets::small_muted(
                            ui,
                            "As cinco mais urgentes estão abaixo; “Ver todas” mostra o restante.",
                        );
                    }
                    // Same order and signals as the Tarefas panel: a task after
                    // what it depends on, a mark while something before it is open.
                    let deps = crate::tasks::Dependencies::new(&tasks);
                    let open: Vec<&crate::tasks::Task> = tasks
                        .iter()
                        .filter(|task| task.status != "concluida")
                        .collect();
                    let open = deps.order(&open);
                    for (index, task) in open.into_iter().take(5).enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        let blockers = deps.open_blockers(task.id);
                        let blocked = !blockers.is_empty();
                        let names = blockers
                            .iter()
                            .map(|t| t.title.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let indent = 18.0 * deps.level(task.id).min(4) as f32;
                        let editing = self.tasks.task_dirty(task.id);
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.add(widgets::quiet("Editar")).clicked() {
                                        edit = Some(task.clone());
                                    }
                                    due_pill(ui, task, &today_iso);
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            if indent > 0.0 {
                                                ui.add_space(indent);
                                            }
                                            let done = egui::Button::new(
                                                RichText::new(icon::CHECK)
                                                    .size(14.0)
                                                    .color(theme::c().muted),
                                            )
                                            .min_size(egui::vec2(24.0, 24.0))
                                            .corner_radius(egui::CornerRadius::same(5));
                                            if ui
                                                .add_enabled(!editing && !blocked, done)
                                                .on_hover_text(
                                                    "Concluir: marca a tarefa como concluída. Nada é enviado ao SIGADOC.",
                                                )
                                                .on_disabled_hover_text(if blocked {
                                                    format!("Conclua antes: {names}")
                                                } else {
                                                    "Salve ou cancele a edição desta tarefa antes de concluí-la."
                                                        .to_owned()
                                                })
                                                .clicked()
                                            {
                                                complete = Some(task.id);
                                            }
                                            ui.with_layout(
                                                egui::Layout::top_down(egui::Align::Min),
                                                |ui| {
                                                    ui.add(egui::Label::new(&task.title).wrap());
                                                    let mut detail_line = format!(
                                                        "{} · Prioridade {}",
                                                        crate::tasks::status_label(&task.status),
                                                        crate::tasks::priority_label(&task.priority)
                                                            .to_lowercase()
                                                    );
                                                    if blocked {
                                                        detail_line.push_str(&format!(
                                                            " · {} depende de: {names}",
                                                            icon::LOCK
                                                        ));
                                                    }
                                                    let line = ui.add(
                                                        egui::Label::new(
                                                            RichText::new(detail_line)
                                                                .size(theme::SIZE_SMALL)
                                                                .color(theme::c().muted),
                                                        )
                                                        .wrap(),
                                                    );
                                                    if blocked {
                                                        line.on_hover_text(format!(
                                                            "Aguardando: {names}"
                                                        ));
                                                    }
                                                },
                                            );
                                        },
                                    );
                                },
                            );
                        });
                    }
                }
                Err(error) => {
                    widgets::muted(ui, format!("Não foi possível ler as tarefas: {error}"));
                }
            },
        );
        if new_task {
            self.navigate(Leave::NewTask(detail.summary.id));
        }
        if see_all {
            self.tasks.focus_document(detail.summary.id);
            self.area = Area::Tasks;
        }
        if let Some(task_id) = complete {
            match self.store.complete_task(task_id) {
                Ok(done) => {
                    self.tasks.forget_task(task_id);
                    let mut text = format!("Tarefa concluída: {}", done.title);
                    for note in self.store.take_cascade_notes() {
                        text.push_str(" • ");
                        text.push_str(&note);
                    }
                    self.message(text);
                }
                Err(e) => self.error(format!("Não foi possível concluir a tarefa: {e}")),
            }
        }
        if let Some(task) = edit {
            self.navigate(Leave::EditTask(task));
        }
    }

    fn notes(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        let mut save = false;
        let mut discard = false;
        let dirty = self.editors[&id].note_dirty();
        titled_card(
            ui,
            "Notas",
            |ui| {
                if ui
                    .add_enabled(dirty, widgets::primary(format!("{} Salvar notas", icon::SAVE)))
                    .clicked()
                {
                    save = true;
                }
                if ui
                    .add_enabled(dirty, widgets::quiet("Desfazer alterações"))
                    .on_hover_text("Volta ao texto salvo pela última vez.")
                    .clicked()
                {
                    discard = true;
                }
                if dirty {
                    pill(ui, "não salvas", Tone::Warn);
                }
                info(ui, "Suas anotações e correções sobre este documento. Ficam só no acervo protegido deste computador (ou do gabinete, se estiver em equipe).");
            },
            |ui| {
                let editor = self.editors.get_mut(&id).expect("editor exists");
                ui.add(
                    egui::TextEdit::multiline(&mut editor.note)
                        .desired_width(f32::INFINITY)
                        .desired_rows(10)
                        .hint_text("Registre observações, pontos a verificar e correções…"),
                );
            },
        );
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
                    widgets::muted(ui, format!("Não foi possível ler a importação da minuta: {e}"));
                    return;
                }
            }
        };
        let id = detail.summary.id;
        let mut template = false;
        let mut generate: Option<crate::work_templates::Intent> = None;
        let mut save_as_template = false;
        let mut material: Option<String> = None;
        let mut save = false;
        let mut loaded = None;
        // Published work templates of the editor's kind, visible to this member.
        let member = self
            .store
            .load_intranet_config()
            .map(|c| c.member_name)
            .unwrap_or_default();
        let current_kind = self.editors[&id].kind.clone();
        let offered: Vec<crate::work_templates::WorkTemplate> = self
            .store
            .list_templates()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.offered_for(&current_kind, &member))
            .collect();
        let blocks = if offered.is_empty() {
            Vec::new()
        } else {
            self.store.list_blocks().unwrap_or_default()
        };
        // A lexical hint: the offered template whose words match the
        // document's, shown only while no template is chosen.
        let suggested = if self.editors[&id].template_uid.is_none() && !offered.is_empty() {
            self.store
                .suggest_template(&detail.capture, &current_kind, &member)
                .ok()
                .flatten()
        } else {
            None
        };
        let mut export = false;
        let mut compare: Option<crate::model::DraftVersion> = None;
        let busy_generating = self
            .worker
            .as_ref()
            .is_some_and(|w| w.kind == WorkKind::Draft);
        let generating_label = self
            .worker
            .as_ref()
            .filter(|w| w.kind == WorkKind::Draft)
            .map(|w| format!("Gerando… {}", w.short_step()));
        let disclosure = self.ai_disclosure_text(true);
        let suggestion_pending = self.suggestion.is_some();
        titled_card(
            ui,
            "Minuta",
            |ui| {
                info(ui, "Texto editável para revisão. Confira fatos, destinatário e fundamentos antes de usar no SIGADOC. Nada é enviado ao SIGADOC por aqui.");
            },
            |ui| {
                {
                    let editor = self.editors.get_mut(&id).expect("editor exists");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = S3;
                        widgets::field_label(ui, "Tipo");
                        egui::ComboBox::from_id_salt("draft-kind")
                            .selected_text(kind_label(&editor.kind).to_owned())
                            .show_ui(ui, |ui| {
                                for kind in ["resposta", "encaminhamento", "complementacao"] {
                                    ui.selectable_value(&mut editor.kind, kind.into(), kind_label(kind));
                                }
                            });
                        widgets::field_label(ui, "Modelo de trabalho");
                        // A template no longer offered (other kind, archived or
                        // unpublished meanwhile) is dropped, not kept hidden.
                        if editor
                            .template_uid
                            .as_ref()
                            .is_some_and(|uid| !offered.iter().any(|t| &t.uid == uid))
                        {
                            editor.template_uid = None;
                            editor.selected_blocks.clear();
                        }
                        let chosen = editor
                            .template_uid
                            .as_ref()
                            .and_then(|uid| offered.iter().find(|t| &t.uid == uid));
                        let chosen_name = chosen
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| "Nenhum".into());
                        egui::ComboBox::from_id_salt("draft-template")
                            .selected_text(chosen_name)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(editor.template_uid.is_none(), "Nenhum")
                                    .clicked()
                                {
                                    editor.template_uid = None;
                                    editor.selected_blocks.clear();
                                }
                                for candidate in &offered {
                                    let selected = editor.template_uid.as_deref() == Some(&candidate.uid);
                                    let response = ui.selectable_label(selected, &candidate.name);
                                    let response = if candidate.purpose.is_empty() {
                                        response
                                    } else {
                                        response.on_hover_text(&candidate.purpose)
                                    };
                                    if response.clicked() {
                                        editor.template_uid = Some(candidate.uid.clone());
                                        editor.selected_blocks.clear();
                                    }
                                }
                            });
                        template = ui
                            .add(widgets::secondary("Começar por um modelo"))
                            .on_hover_text("Preenche o editor com a estrutura do modelo escolhido (ou um texto-base) sem usar IA.")
                            .clicked();
                        save_as_template = ui
                            .add_enabled(
                                !editor.body.trim().is_empty(),
                                widgets::quiet("Guardar como modelo"),
                            )
                            .on_hover_text("Leva este texto para a área Modelos como rascunho de um novo modelo de trabalho.")
                            .clicked();
                    });
                    if let Some(ranked) = &suggested {
                        ui.horizontal_wrapped(|ui| {
                            widgets::small_muted(ui, format!(
                                "Modelo sugerido pelas palavras do documento: {} ({}% de semelhança)",
                                ranked.item.name,
                                ranked.percent()
                            ))
                            .on_hover_text("Comparação lexical entre o documento e o nome, a finalidade, os critérios de uso e os exemplos aprovados de cada modelo publicado. É só uma sugestão.");
                            if ui.add(widgets::quiet("Usar este modelo")).clicked() {
                                editor.template_uid = Some(ranked.item.uid.clone());
                                editor.selected_blocks.clear();
                            }
                        });
                    }
                    // Optional and conditional blocks of the chosen template.
                    if let Some(version) = editor
                        .template_uid
                        .as_ref()
                        .and_then(|uid| offered.iter().find(|t| &t.uid == uid))
                        .and_then(|t| t.published_version())
                    {
                        use crate::work_templates::BlockMode;
                        let optional: Vec<_> = version
                            .block_refs
                            .iter()
                            .filter(|r| r.mode != BlockMode::Mandatory)
                            .collect();
                        if !optional.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                widgets::field_label(ui, "Instruções opcionais");
                                for reference in optional {
                                    let name = blocks
                                        .iter()
                                        .find(|b| b.uid == reference.block_uid)
                                        .map(|b| b.name.clone())
                                        .unwrap_or_else(|| "instrução".into());
                                    let label = match &reference.mode {
                                        BlockMode::Conditional { condition } => {
                                            format!("{name} (se: {condition})")
                                        }
                                        _ => name,
                                    };
                                    let mut on = editor.selected_blocks.contains(&reference.block_uid);
                                    if ui.checkbox(&mut on, label).changed() {
                                        if on {
                                            editor.selected_blocks.insert(reference.block_uid.clone());
                                        } else {
                                            editor.selected_blocks.remove(&reference.block_uid);
                                        }
                                    }
                                }
                            });
                        }
                    }
                    widgets::field_label(ui, "Pedido desta minuta (opcional)");
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.instructions)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .hint_text("Ex.: solicitar a informação que falta, com tom objetivo."),
                    );
                    ui.horizontal_wrapped(|ui| {
                        use crate::work_templates::Intent;
                        let can_generate = !busy_generating && !suggestion_pending;
                        let has_text = !editor.body.trim().is_empty();
                        let disabled_reason = if suggestion_pending {
                            "Há uma sugestão pronta aguardando revisão (barra inferior)."
                        } else {
                            "Aguarde a geração em andamento."
                        };
                        if ui
                            .add_enabled(
                                can_generate,
                                widgets::primary(
                                    generating_label
                                        .clone()
                                        .unwrap_or_else(|| format!("{} Gerar com IA", icon::AI)),
                                ),
                            )
                            .on_hover_text(&disclosure)
                            .on_disabled_hover_text(disabled_reason)
                            .clicked()
                        {
                            generate = Some(Intent::New);
                        }
                        if ui
                            .add_enabled(
                                can_generate && has_text,
                                widgets::secondary("Completar"),
                            )
                            .on_hover_text("Completar texto atual: a IA preserva o que você escreveu, preenche os [preencher] e acrescenta as seções que faltam.")
                            .on_disabled_hover_text(if has_text { disabled_reason } else { "Escreva ou carregue um texto antes." })
                            .clicked()
                        {
                            generate = Some(Intent::Complete);
                        }
                        let has_request = !editor.instructions.trim().is_empty();
                        if ui
                            .add_enabled(
                                can_generate && has_text && has_request,
                                widgets::secondary("Revisar"),
                            )
                            .on_hover_text("Revisar texto atual: a IA altera só o necessário para atender ao pedido; o resultado abre lado a lado com o texto atual.")
                            .on_disabled_hover_text(if !has_text {
                                "Escreva ou carregue um texto antes."
                            } else if !has_request {
                                "Escreva em \"Pedido desta minuta\" o que deve ser revisado."
                            } else {
                                disabled_reason
                            })
                            .clicked()
                        {
                            generate = Some(Intent::Revise);
                        }
                    });
                }
                {
                    let editor = self.editors.get_mut(&id).expect("editor exists");
                    if editor.capture_id != detail.summary.latest_capture_id
                        || self.store.sources_stale(&editor.materials)
                    {
                        notice_line(ui, Tone::Warn, "Este texto foi feito sobre uma importação anterior ou fontes que mudaram. Revise antes de aproveitar.");
                    }
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.body)
                            .desired_rows(16)
                            .desired_width(f32::INFINITY)
                            .hint_text("Escreva livremente ou comece por um modelo…"),
                    );
                    ui.horizontal_wrapped(|ui| {
                        save = ui
                            .add_enabled(
                                editor.draft_dirty() && !editor.body.trim().is_empty(),
                                widgets::primary(format!("{} Salvar nova versão", icon::SAVE)),
                            )
                            .clicked();
                        if ui
                            .add_enabled(
                                !editor.body.is_empty(),
                                widgets::secondary(format!("{} Copiar", icon::COPY)),
                            )
                            .on_hover_text("Copiar texto. Não salva uma versão.")
                            .clicked()
                        {
                            ui.ctx().copy_text(editor.body.clone());
                        }
                        export = ui
                            .add_enabled(
                                !editor.body.trim().is_empty(),
                                widgets::secondary("Exportar (RTF)"),
                            )
                            .on_hover_text("Cria uma cópia legível fora do acervo protegido, para abrir no Word ou similar. Não salva uma versão.")
                            .clicked();
                        if !detail.drafts.is_empty() {
                            egui::containers::menu::MenuButton::from_button(widgets::secondary(
                                format!("Comparar com versão {}", icon::DOWN),
                            ))
                            .ui(ui, |ui| {
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
                            pill(ui, "não salva", Tone::Warn);
                        }
                        widgets::small_muted(ui, editor.origin.as_str());
                    });
                }
                ui.collapsing(format!("Versões salvas ({})", detail.drafts.len()), |ui| {
                    for (index, draft) in detail.drafts.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.add(widgets::secondary("Abrir")).clicked() {
                                loaded = Some(draft.clone());
                            }
                            ui.label(format!(
                                "#{} • {} • {}",
                                draft.id,
                                kind_label(&draft.kind),
                                display_date(&draft.created_at)
                            ));
                            if draft.stale || draft.capture_id != detail.summary.latest_capture_id {
                                pill(ui, "importação anterior", Tone::Warn);
                            }
                            if let Some(provenance) = &draft.provenance {
                                if let Some(uid) = &provenance.generation_uid {
                                    if ui
                                        .add(widgets::quiet("Material utilizado"))
                                        .on_hover_text("Modelo, instruções, pedido, exemplos, fontes e IA que produziram este texto.")
                                        .clicked()
                                    {
                                        material = Some(uid.clone());
                                    }
                                } else if !provenance.template_name.is_empty() {
                                    widgets::small_muted(
                                        ui,
                                        format!("modelo {}", provenance.template_name),
                                    );
                                }
                            }
                        });
                    }
                    if detail.drafts.is_empty() {
                        widgets::small_muted(
                            ui,
                            "As versões aparecerão aqui após “Salvar nova versão”.",
                        );
                    }
                });
            },
        );
        if export {
            let body = self.editors[&id].body.clone();
            match crate::draft_tools::export(&export_capture, &body) {
                Ok(path) => {
                    self.message(format!("Cópia exportada: {}", path.display()));
                    if let Some(parent) = path.parent() {
                        crate::windows_ui::reveal_in_explorer(parent);
                    }
                }
                Err(e) => self.error(format!("Não foi possível exportar a minuta: {e}")),
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
        if let Some(uid) = material {
            self.material = Some((id, uid));
        }
        if template {
            let editor_kind = self.editors[&id].kind.clone();
            let chosen = self.editors[&id]
                .template_uid
                .as_ref()
                .and_then(|uid| offered.iter().find(|t| &t.uid == uid));
            let (body, origin) = match chosen.and_then(|t| t.published_version().map(|v| (t, v))) {
                Some((found, version)) => (
                    crate::prompting::skeleton(version, &detail.capture),
                    format!("Estrutura do modelo \"{}\" — revisar campos", found.name),
                ),
                None => (
                    ai::template_reply(&detail.capture, &editor_kind),
                    "Modelo sem IA — revisar campos".into(),
                ),
            };
            let provenance = chosen.map(|t| crate::model::DraftProvenance {
                template_uid: Some(t.uid.clone()),
                template_version: Some(t.current_version),
                template_name: t.name.clone(),
                ..Default::default()
            });
            self.replace_editor(Replacement {
                document_id: id,
                capture_id: detail.summary.latest_capture_id,
                kind: editor_kind,
                body,
                origin,
                saved: false,
                from_suggestion: false,
                materials: Vec::new(),
                provenance,
            });
        }
        if save_as_template {
            if self.templates.dirty() {
                self.error("Há um modelo com alterações não salvas na área Modelos. Salve ou descarte antes de criar outro.");
            } else {
                let (kind, body) = {
                    let editor = &self.editors[&id];
                    (editor.kind.clone(), editor.body.clone())
                };
                self.templates.start_from_text(&kind, &body);
                self.navigate(Leave::Templates);
            }
        }
        if let Some(intent) = generate {
            self.generate(detail, intent, ui.ctx());
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
                provenance: draft.provenance,
            });
        }
    }

    fn ai_destination_text(&self) -> (String, String) {
        let provider = match self.ai.provider {
            AiProvider::Local => "Ollama neste computador",
            AiProvider::Cloud => "Ollama Cloud",
            AiProvider::Remote => "servidor remoto",
            AiProvider::ChatGpt => "ChatGPT (OpenAI)",
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
        format!("Ao clicar, envia {sent} ao {provider} ({destination}). Modelo de IA: {model}. O resultado é uma sugestão para a sua revisão.")
    }

    /// One short, always-visible line saying where the AI runs.
    fn ai_destination(&self, ui: &mut egui::Ui) {
        let (provider, destination) = self.ai_destination_text();
        let text = match self.ai.provider {
            AiProvider::Local => format!("{provider}, sem envio externo"),
            _ => format!("envia dados ao {provider}: {destination}"),
        };
        widgets::small_muted(ui, text).on_hover_text(self.ai_disclosure_text(false));
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

    /// The "Inteligência artificial" section of Ajustes. Leaving the section
    /// saves what was changed (see `leave_ai_settings`).
    fn ai_settings(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let mut save = false;
        let mut refresh = false;
        let mut provider_changed = false;
        widgets::card(ui, |ui| {
            widgets::section(ui, "Onde a IA funciona");
            ui.horizontal_wrapped(|ui| {
                let before = self.ai.provider;
                ui.radio_value(&mut self.ai.provider, AiProvider::Local, "Ollama neste computador")
                    .on_hover_text("Modelo instalado neste computador. Nada sai deste computador.");
                ui.radio_value(&mut self.ai.provider, AiProvider::Cloud, "Ollama Cloud")
                    .on_hover_text("Modelos grandes nos servidores da Ollama (ollama.com), com a chave da sua conta.");
                ui.radio_value(&mut self.ai.provider, AiProvider::Remote, "Outro servidor")
                    .on_hover_text("Servidor do órgão que funciona como o Ollama. O endereço deve começar com https.");
                ui.radio_value(&mut self.ai.provider, AiProvider::ChatGpt, "ChatGPT (assinatura)")
                    .on_hover_text("Usa a cota da sua assinatura ChatGPT Plus ou Pro, entrando com a sua conta. Os textos vão para os servidores da OpenAI.");
                provider_changed = before != self.ai.provider;
            });
            ui.add_space(4.0);
            match self.ai.provider {
                AiProvider::ChatGpt => {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.colored_label(crate::theme::warning(), "⚠ Nesta opção, os dados saem do seu computador e do órgão.");
                        ui.label("A cada clique em “Analisar” ou “Gerar com IA”, o texto do documento, as notas, o contexto e as fontes selecionadas são enviados aos servidores da OpenAI, nos Estados Unidos, sob os termos e a política de privacidade da OpenAI. Nada é enviado ao importar, só quando você clica. Use apenas com documentos que podem sair do órgão; avalie sigilo e LGPD antes.");
                        ui.small("O Ponte não recebe acesso às suas conversas, à memória nem à cobrança do ChatGPT. O uso é descontado da cota da sua assinatura (Plus ou Pro); o limite por aplicativo pode ser ajustado em ChatGPT ▸ Configurações ▸ Uso. Quando a cota acaba, o Ponte avisa e não usa outro provedor por conta própria.");
                    });
                    ui.add_space(4.0);
                    let signed_in = self.ai.chatgpt.signed_in();
                    let sharing = self.ai.chatgpt.sharing();
                    let account = self.ai.chatgpt.account_label.clone();
                    let in_progress = self.chatgpt_login.is_some();
                    ui.horizontal_wrapped(|ui| {
                        if in_progress {
                            ui.spinner();
                            ui.label("Aguardando a autorização no navegador…");
                            if ui.button("Cancelar").clicked() {
                                self.cancel_chatgpt_login();
                            }
                        } else if signed_in {
                            ui.label(format!("Conectado: {account}"));
                            if sharing {
                                ui.colored_label(crate::theme::success(), "✓ uso do plano liberado");
                            } else {
                                ui.colored_label(crate::theme::warning(), "⚠ sem permissão de uso do plano");
                            }
                            if ui.button("Entrar de novo…").on_hover_text("Abre o navegador para renovar a autorização ou liberar o uso do plano.").clicked() {
                                self.start_chatgpt_login(ctx);
                            }
                            if ui.button("Sair").on_hover_text("Apaga a sessão deste computador e pede à OpenAI que a encerre.").clicked() {
                                self.chatgpt_sign_out();
                            }
                        } else {
                            if ui.button("Entrar com o ChatGPT…").on_hover_text("Abre o navegador na página da OpenAI. Entre com a conta da sua assinatura e autorize o Ponte, inclusive o uso do plano.").clicked() {
                                self.start_chatgpt_login(ctx);
                            }
                            if !account.is_empty() {
                                ui.small(format!("última conta: {account}"));
                            }
                        }
                    });
                    if let Some(login) = &self.chatgpt_login {
                        let url = login.authorize_url.clone();
                        ui.horizontal_wrapped(|ui| {
                            ui.small("Se o navegador não abriu, copie o endereço e cole nele:");
                            if ui.small_button("Copiar endereço").clicked() {
                                ctx.copy_text(url);
                            }
                        });
                    }
                    if let Some(error) = &self.chatgpt_login_error {
                        ui.colored_label(crate::theme::error(), format!("✗ {error}"));
                    }
                    ui.small("Requer assinatura ChatGPT Plus ou Pro (conta gratuita entra, mas não libera o uso do plano). O login é feito no navegador, em auth.openai.com, e retorna ao Ponte neste computador.");
                }
                AiProvider::Local => {
                    let label = ui.label("Endereço do Ollama");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.local_url).desired_width(f32::INFINITY)).labelled_by(label.id);
                    ui.small("Padrão: http://127.0.0.1:11434. Instale o Ollama em ollama.com/download e baixe um modelo (ex.: ollama pull qwen3:4b-instruct).");
                }
                AiProvider::Cloud => {
                    ui.label(format!("Endereço: {} (fixo)", ai::CLOUD_URL));
                    let label = ui.label("Chave da sua conta");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.cloud_key).password(true).hint_text("cole aqui a chave gerada em ollama.com").desired_width(f32::INFINITY)).labelled_by(label.id);
                    ui.horizontal_wrapped(|ui| {
                        ui.small("Crie a chave em");
                        ui.hyperlink_to(RichText::new("ollama.com/settings/keys").small(), "https://ollama.com/settings/keys");
                        ui.small("(conta gratuita por uso ou plano mensal).");
                    });
                    ui.colored_label(crate::theme::warning(), "Os textos enviados à IA saem deste computador para os servidores da Ollama (EUA, com possível roteamento pela Europa). A Ollama declara não registrar nem treinar com os dados.");
                }
                AiProvider::Remote => {
                    let label = ui.label("Endereço do servidor (HTTPS)");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.remote_url).desired_width(f32::INFINITY)).labelled_by(label.id);
                    let label = ui.label("Chave do servidor (opcional)");
                    ui.add(egui::TextEdit::singleline(&mut self.ai.remote_token).password(true).desired_width(f32::INFINITY)).labelled_by(label.id);
                    ui.colored_label(crate::theme::warning(), "Os textos enviados à IA saem deste computador para esse servidor.");
                }
            }
            ui.add_space(6.0);
            ui.strong("Modelo de IA");
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
                ui.colored_label(crate::theme::warning(), format!("⚠ {error}"));
            } else if listed {
                ui.small(if self.models.len() == 1 {
                    "1 modelo oferecido por este servidor.".to_owned()
                } else {
                    format!("{} modelos oferecidos por este servidor.", self.models.len())
                });
            } else if self.model_fetch.is_none() {
                ui.small("Clique em “Listar modelos do servidor” para escolher entre os disponíveis.");
            }
            ui.add_space(6.0);
            let remember_label = if self.ai.provider == AiProvider::ChatGpt {
                "Guardar a sessão do ChatGPT neste computador"
            } else {
                "Guardar a chave neste computador"
            };
            ui.checkbox(&mut self.ai.remember_keys, remember_label)
                .on_hover_text("A configuração fica no acervo, protegida pela sua conta do Windows. Desmarcada, a chave (ou a sessão do ChatGPT) vale só até fechar o Ponte.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let dirty = self.ai_config_dirty();
                if ui.add_enabled(dirty, widgets::primary("Salvar")).clicked() {
                    save = true;
                }
                if dirty {
                    pill(ui, "Não salvo", Tone::Warn);
                    widgets::small_muted(ui, "Também é salvo ao sair desta seção.");
                } else {
                    widgets::small_muted(ui, "Configuração salva.");
                }
            });
            ui.separator();
            widgets::small_muted(ui, "O servidor só é acessado ao listar modelos, analisar ou gerar uma sugestão. O aplicativo não solicita sua senha do SIGADOC.");
            widgets::small_muted(ui, "A análise é salva no acervo. A sugestão de minuta só é salva por sua ação explícita.");
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
    }

    /// Leaving the AI section saves what was changed, as closing its window
    /// used to. False when the configuration is invalid: the person stays.
    fn leave_ai_settings(&mut self) -> bool {
        if !self.ai_config_dirty() {
            return true;
        }
        if self.save_ai_config() {
            self.message("Configuração de IA salva.");
            true
        } else {
            false
        }
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
                ui.spacing_mut().item_spacing.y = S2;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&suggestion.number)
                            .font(theme::font_strong(theme::SIZE_SECTION)),
                    );
                    pill(ui, kind_label(&suggestion.kind), Tone::Neutral);
                    pill(ui, "ainda não salva", Tone::Warn);
                });
                widgets::small_muted(
                    ui,
                    format!("Baseada na importação nº {}", suggestion.capture_id),
                );
                ui.label("Leia o texto e leve ao editor para corrigir e salvar uma versão.");
                widgets::card(ui, |ui| {
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
                });
                ui.horizontal(|ui| {
                    use_text = ui.add(widgets::primary("Levar ao editor")).clicked();
                    if ui.add(widgets::secondary(format!("{} Copiar sugestão", icon::COPY))).clicked() {
                        ui.ctx().copy_text(suggestion.body.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        discard = ui.add(widgets::danger("Descartar sugestão")).clicked();
                    });
                });
            });
        self.suggestion_open = open;
        if discard {
            self.suggestion = None;
            self.suggestion_open = false;
        }
        if use_text {
            if let Some(suggestion) = self.suggestion.clone() {
                // A revision is shown next to the text it replaces.
                if suggestion.intent == crate::work_templates::Intent::Revise {
                    self.draft_tools.open_comparison(
                        "Texto anterior".into(),
                        suggestion.previous_text.clone(),
                        suggestion.body.clone(),
                    );
                }
                self.replace_editor(Replacement {
                    document_id: suggestion.document_id,
                    capture_id: suggestion.capture_id,
                    kind: suggestion.kind,
                    body: suggestion.body,
                    origin: match suggestion.intent {
                        crate::work_templates::Intent::New => "Sugestão de IA revisável".into(),
                        crate::work_templates::Intent::Complete => {
                            "Texto completado pela IA — revisar".into()
                        }
                        crate::work_templates::Intent::Revise => {
                            "Texto revisado pela IA — comparar e revisar".into()
                        }
                    },
                    saved: false,
                    from_suggestion: true,
                    materials: suggestion.materials,
                    provenance: suggestion.provenance,
                });
                self.suggestion_open = false;
            }
        }
    }

    /// The window "Material utilizado": everything a generation used, as
    /// recorded at the time, so the text can be read with its basis.
    fn material_window(&mut self, ctx: &egui::Context) {
        let Some((document_id, uid)) = self.material.clone() else {
            return;
        };
        let record = match self.store.generation_by_uid(document_id, &uid) {
            Ok(Some(record)) => record,
            Ok(None) => {
                self.material = None;
                self.error("O registro desta geração não está neste acervo.");
                return;
            }
            Err(error) => {
                self.material = None;
                self.error(format!("Não foi possível ler o registro: {error}"));
                return;
            }
        };
        let mut open = true;
        egui::Window::new("Material utilizado na geração")
            .open(&mut open)
            .default_size([700.0, 560.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S3;
                    widgets::card(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S2;
                        let template = match &record.template_uid {
                            Some(_) => format!(
                                "{} (versão {})",
                                record.template_name,
                                record.template_version.unwrap_or(0)
                            ),
                            None => "nenhum".to_owned(),
                        };
                        facts(
                            ui,
                            "material-facts",
                            &[
                                ("O que foi pedido", record.intent.label().to_owned()),
                                ("Modelo de trabalho", template),
                                ("IA", format!("{} ({})", record.model, record.provider)),
                                (
                                    "Feito",
                                    format!(
                                        "por {} em {}",
                                        record.created_by,
                                        display_date(&record.created_at)
                                    ),
                                ),
                            ],
                        );
                        match &record.outcome {
                            crate::work_templates::Outcome::Accepted => {
                                ui.horizontal_wrapped(|ui| {
                                    pill(ui, "Aceito", Tone::Ok);
                                    ui.label("Texto aceito pelo programa e levado à revisão.");
                                });
                            }
                            crate::work_templates::Outcome::Refused { reason } => {
                                notice_line(ui, Tone::Warn, format!("Resultado: recusado — {reason}"));
                            }
                        }
                    });
                    if !record.request_guidance.trim().is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Pedido desta minuta");
                            ui.add(egui::Label::new(&record.request_guidance).wrap());
                        });
                    }
                    if !record.blocks.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Instruções aplicadas");
                            for block in &record.blocks {
                                ui.collapsing(
                                    format!("{} (versão {}, {})", block.name, block.version, block.mode),
                                    |ui| {
                                        ui.add(egui::Label::new(&block.text).wrap());
                                    },
                                );
                            }
                        });
                    }
                    if !record.example_uids.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(
                                ui,
                                format!(
                                    "Exemplos aprovados mostrados: {}",
                                    record.example_uids.len()
                                ),
                            );
                            widgets::small_muted(ui, "Os exemplos mostram só a forma; seus fatos não valem para este caso.");
                        });
                    }
                    if !record.sources.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Fontes selecionadas");
                            for source in &record.sources {
                                ui.add(
                                    egui::Label::new(format!("{} — {}", source.id, source.label))
                                        .wrap(),
                                );
                            }
                            widgets::small_muted(ui, "Um trecho citado foi encontrado literalmente na fonte; isso não comprova a conclusão.");
                        });
                    }
                    if !record.conflicts_shown.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Avisos de conflito exibidos antes de gerar");
                            for conflict in &record.conflicts_shown {
                                notice_line(ui, Tone::Warn, conflict.as_str());
                            }
                        });
                    }
                    widgets::small_muted(
                        ui,
                        format!(
                            "Instruções enviadas: {} • dados: {} • código das instruções: {}",
                            crate::labels::file_size(record.system_bytes as u64),
                            crate::labels::file_size(record.data_bytes as u64),
                            record.system_sha256.chars().take(12).collect::<String>()
                        ),
                    );
                });
            });
        if !open {
            self.material = None;
        }
    }

    /// The window "Material utilizado": everything a generation used, as
    /// recorded at the time, so the text can be read with its basis.

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
                self.templates.dirty()
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
        if self.worker.is_some()
            || self.pdf_reader.busy()
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
                        count as usize,
                        "item foi apagado de vez",
                        "itens foram apagados de vez"
                    ),
                    crate::trash::retention_label(days)
                ));
                self.refresh();
            }
            Err(error) => self.error(format!("Não foi possível esvaziar a lixeira automaticamente: {error}")),
        }
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
                Kind::Attachment => {}
            }
            self.message(if change.target.kind == Kind::Attachment {
                "Anexo movido para a Lixeira. É possível restaurá-lo em Ajustes ▸ Lixeira."
            } else {
                "Item movido para a Lixeira. É possível restaurá-lo em Ajustes ▸ Lixeira."
            });
        } else {
            if self.selected_id == Some(change.document) {
                self.reload(change.document);
            }
            self.message(if change.target.kind == Kind::Capture {
                "Importação restaurada no histórico. A importação em uso foi mantida."
            } else {
                "Item restaurado. Os documentos e as tarefas foram atualizados."
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
        if self.leave.is_some() {
            let title = self
                .tasks
                .editing_title()
                .unwrap_or_else(|| "tarefa".to_owned());
            let mut save = false;
            let mut discard = false;
            let mut stay = false;
            egui::Window::new("Tarefa com alterações não salvas")
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("A tarefa “{title}” tem alterações não salvas."));
                    ui.small("Salvar e continuar grava a tarefa e vai para onde você clicou. Descartar alterações volta ao que estava salvo (tarefa nova é apagada). Voltar à tarefa deixa tudo como está.");
                    if let Some(error) = &self.leave_error {
                        ui.add_space(4.0);
                        ui.colored_label(crate::theme::error(), error);
                        ui.small("Corrija na ficha da tarefa (Voltar à tarefa) ou descarte as alterações.");
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        save = ui.button("Salvar e continuar").clicked();
                        discard = ui.button("Descartar alterações").clicked();
                        stay = ui.button("Voltar à tarefa").clicked();
                    });
                });
            if stay {
                self.leave = None;
                self.leave_error = None;
                self.area = Area::Tasks;
            }
            if save {
                match self.tasks.save_draft(&mut self.store) {
                    Ok(()) => {
                        if let Some(leave) = self.leave.take() {
                            self.leave_error = None;
                            self.perform_leave(leave);
                        }
                    }
                    Err(error) => self.leave_error = Some(error),
                }
            }
            if discard {
                self.tasks.discard_draft();
                if let Some(leave) = self.leave.take() {
                    self.leave_error = None;
                    self.perform_leave(leave);
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
                    ui.label("Ainda há pendências. Ao fechar agora, elas serão perdidas:");
                    for item in &pending {
                        ui.horizontal(|ui| {
                            ui.label(format!("• {}", item.label));
                            if !matches!(item.go, PendingTarget::None)
                                && ui.small_button("Ir para").clicked()
                            {
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
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::attachments::clear_temp_dir();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::theme::sync(&ctx);
        self.intranet.attach_repaint(&ctx);
        self.poll_worker();
        self.poll_models();
        self.poll_chatgpt_login(&ctx);
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
            .is_some_and(|changed| changed.elapsed() > Duration::from_millis(250))
        {
            self.search_changed = None;
            self.refresh();
        } else if self.last_refresh.elapsed() > Duration::from_secs(4) {
            self.refresh();
        }
        self.sweep_trash();
        ctx.request_repaint_after(Duration::from_millis(300));
        self.draft_tools.comparison(&ctx);
        self.pdf_reader.show(&ctx, &mut self.store);
        let pending = self.templates.dirty()
            || self.mail.dirty()
            || self.pdf_reader.busy()
            || self.dossier.dirty()
            || self.work_context.dirty()
            || self.tasks.dirty()
            || self.saiop.dirty()
            || self.manual_form.dirty()
            || self.intranet.unsent_folder_changes(&self.store) > 0
            || self.editors.values().any(Editor::dirty)
            || self.worker.is_some()
            || self.suggestion.is_some();
        if !self.allow_close && pending && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_confirm = true;
        }
        let enabled = self.replacement.is_none()
            && self.trash.pending.is_none()
            && !self.exit_confirm
            && self.leave.is_none();
        if enabled {
            self.shortcuts(&ctx);
            self.route_settings_requests();
        }
        let palette = crate::theme::c();
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
                    if matches!(worker.kind, WorkKind::Analysis | WorkKind::Draft) {
                        cancel = ui
                            .add_enabled(
                                !worker.cancelling(),
                                egui::Button::new("Cancelar").small(),
                            )
                            .on_hover_text("Interrompe a IA na próxima etapa. Nada é salvo.")
                            .clicked();
                    }
                });
            }
            if cancel {
                self.cancel_worker();
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
        if let Some(notice) = &self.notice {
            dismiss |= crate::widgets::toast(&ctx, &notice.text, notice.error, None)
                != crate::widgets::ToastAction::None;
        }
        if dismiss {
            self.notice = None;
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
                if self.area == Area::Settings {
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
                } else if self.area == Area::Saiop {
                    let context = self.selected_id.and_then(|id| {
                        self.documents
                            .iter()
                            .find(|d| d.id == id)
                            .map(|d| (id, d.document_number.clone()))
                    });
                    self.saiop.set_context_document(context);
                    if let Some(id) = self.saiop.show(ui, &mut self.store) {
                        self.select(id);
                    }
                    if let Some(task) = self.saiop.take_open_task() {
                        if self.tasks.focus_task(&task) {
                            self.area = Area::Tasks;
                        } else {
                            self.error("Há uma tarefa com alterações não salvas no painel Tarefas. Salve ou descarte antes de abrir outra.");
                        }
                    }
                } else if self.area == Area::Tasks {
                    if let Some(id) = self.tasks.show(ui, &mut self.store, self.selected_id) { self.navigate(Leave::Select(id)); }
                    if let Some(intent) = self.tasks.take_blocked() {
                        self.navigate(match intent {
                            crate::tasks_ui::Intent::Open(task) => Leave::OpenTask(task),
                            crate::tasks_ui::Intent::New(document_id) => Leave::NewTask(document_id),
                        });
                    }
                    if let Some(document_id) = self.tasks.take_consult_request() { self.open_consult(document_id); }
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
            self.mail_window(&ctx);
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
        if self.trash.take_purged() {
            self.refresh();
        }
        #[cfg(feature = "shots")]
        self.tour_frame(&ctx);
    }

    fn persist_egui_memory(&self) -> bool {
        // Avoid accidental persistence of any widget memory that could contain content.
        false
    }
}


/// Development only: the screens pictured by `--shots`, in order. Each step
/// sets everything it needs, so any of them can be taken alone.
#[cfg(feature = "shots")]
impl PonteApp {
    const TOUR: &'static [&'static str] = &[
        "documentos-inicio",
        "documentos-filtros",
        "ficha-resumo",
        "ficha-documento",
        "ficha-notas",
        "ficha-mais",
        "ficha-manual",
        "ficha-email",
        "ficha-juntada",
        "tarefas",
        "tarefa-aberta",
        "atendimentos",
        "atendimento-aberto",
        "modelos",
        "ajustes",
        "janela-rede",
        "janela-ia",
        "janela-email",
        "janela-lixeira",
        "janela-registro-manual",
        "janela-sobre",
        "janela-consulta",
    ];

    /// The screens of the app itself, then the views each panel declares
    /// (`SHOTS` in its own file), then everything again in the dark theme.
    fn tour_names() -> Vec<String> {
        let mut names: Vec<String> = Self::TOUR.iter().map(|name| (*name).to_owned()).collect();
        let panels: [(&str, &[&str]); 5] = [
            ("tarefas", crate::tasks_ui::SHOTS),
            ("atendimentos", crate::saiop::SHOTS),
            ("modelos", crate::templates_ui::SHOTS),
            ("email", crate::mail_ui::SHOTS),
            ("rede", crate::intranet::SHOTS),
        ];
        for (prefix, views) in panels {
            names.extend(views.iter().map(|view| format!("{prefix}-{view}")));
        }
        let dark: Vec<String> = names.iter().map(|name| format!("escuro-{name}")).collect();
        names.extend(dark);
        names
    }

    fn tour_frame(&mut self, ctx: &egui::Context) {
        let Some(mut tour) = self.tour.take() else {
            return;
        };
        ctx.request_repaint();
        if let crate::shots::Tick::Begin(index) = tour.tick(ctx) {
            let names = Self::tour_names();
            match names.get(index).map(String::as_str) {
                None => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                Some(name) if crate::shots::Tour::wanted(name) => {
                    self.tour_setup(name, ctx);
                    tour.named(index, name);
                }
                Some(_) => tour.skip(index),
            }
        }
        self.tour = Some(tour);
    }

    fn tour_document(&self) -> Option<i64> {
        self.store
            .list_documents("")
            .ok()?
            .into_iter()
            .find(|d| d.document_number == "ORG-OFI-2026/00431")
            .map(|d| d.id)
    }

    fn tour_setup(&mut self, name: &str, ctx: &egui::Context) {
        // Nothing left over from the step before.
        self.intranet.open = false;
        self.settings_open = false;
        self.mail.open = false;
        self.trash.open = false;
        self.manual_form.open = false;
        self.about_open = false;
        self.consult = None;
        self.filters_open = false;
        self.notice = None;
        let dark = name.starts_with("escuro-");
        crate::theme::set_appearance(
            ctx,
            if dark {
                crate::theme::Appearance::Dark
            } else {
                crate::theme::Appearance::Light
            },
        );
        let name = name.strip_prefix("escuro-").unwrap_or(name);
        let document = self.tour_document();
        match name {
            "documentos-inicio" => {
                self.area = Area::Documents;
                self.selected_id = None;
                self.detail = None;
            }
            "documentos-filtros" => {
                self.area = Area::Documents;
                self.selected_id = None;
                self.detail = None;
                self.filters_open = true;
            }
            "ficha-manual" | "ficha-email" | "ficha-juntada" => {
                let found = self
                    .store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| match name {
                        "ficha-manual" => d.manual && !d.email,
                        "ficha-email" => d.email,
                        _ => d.joined_children_count > 0,
                    })
                    .map(|d| d.id);
                if let Some(id) = found {
                    self.area = Area::Documents;
                    self.select(id);
                }
                self.tab = if name == "ficha-juntada" {
                    Tab::Overview
                } else {
                    Tab::Document
                };
            }
            "janela-consulta" => {
                self.area = Area::Documents;
                if let Some(id) = document {
                    self.open_consult(id);
                }
            }
            "ficha-resumo" | "ficha-documento" | "ficha-notas" | "ficha-mais" => {
                if let Some(id) = document {
                    self.select(id);
                }
                self.tab = match name {
                    "ficha-documento" => Tab::Document,
                    "ficha-notas" => Tab::Notes,
                    "ficha-mais" => Tab::More,
                    _ => Tab::Overview,
                };
            }
            "tarefas" => {
                self.tasks = Default::default();
                self.area = Area::Tasks;
            }
            "tarefa-aberta" => {
                self.tasks = Default::default();
                let task = self
                    .store
                    .list_tasks()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|t| t.overdue());
                if let Some(task) = task {
                    self.perform_leave(Leave::OpenTask(task));
                }
                self.area = Area::Tasks;
            }
            "atendimentos" => {
                self.saiop = Default::default();
                self.area = Area::Saiop;
            }
            "atendimento-aberto" => {
                self.saiop = Default::default();
                let first = self
                    .store
                    .list_saiop_attendances()
                    .unwrap_or_default()
                    .first()
                    .map(|a| a.id);
                if let Some(id) = first {
                    self.saiop.focus_attendance(&self.store, id);
                }
                self.area = Area::Saiop;
            }
            "modelos" => self.area = Area::Templates,
            "ajustes" => self.area = Area::Settings,
            "janela-rede" => {
                self.area = Area::Settings;
                self.intranet.open = true;
            }
            "janela-ia" => {
                self.area = Area::Settings;
                self.open_ai_settings(ctx);
            }
            "janela-email" => {
                self.area = Area::Settings;
                self.mail.open_full();
            }
            "janela-lixeira" => {
                self.area = Area::Settings;
                self.trash.open = true;
            }
            "janela-registro-manual" => {
                self.area = Area::Documents;
                self.manual_form.open_new();
            }
            "janela-sobre" => {
                self.area = Area::Settings;
                self.about_open = true;
            }
            other => {
                // Views declared by the panels themselves.
                if let Some(view) = other.strip_prefix("tarefas-") {
                    self.tasks = Default::default();
                    self.area = Area::Tasks;
                    self.tasks.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("atendimentos-") {
                    self.saiop = Default::default();
                    self.area = Area::Saiop;
                    self.saiop.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("modelos-") {
                    self.templates = Default::default();
                    self.area = Area::Templates;
                    self.templates.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("email-") {
                    self.mail = Default::default();
                    self.area = Area::Settings;
                    self.mail.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("rede-") {
                    self.area = Area::Settings;
                    self.intranet.shots_open(&mut self.store, view);
                }
            }
        }
    }
}

fn display_date(value: &str) -> String {
    crate::labels::datetime(value)
}

/// "Dados do registro" of a manual record.
/// "Dados do registro" of a manual record.
fn manual_facts(ui: &mut egui::Ui, detail: &DocumentDetail) {
    let metadata = &detail.capture.metadata;
    let value = |key: &str| {
        metadata
            .get(key)
            .cloned()
            .unwrap_or_else(|| "não informado".into())
    };
    widgets::small_muted(
        ui,
        "Informado por quem criou o registro; nada disto vem do SIGADOC.",
    );
    facts(
        ui,
        ("manual-facts", detail.summary.id),
        &[
            ("Número no Ponte", detail.summary.document_number.clone()),
            ("Tipo", value("document_type")),
            ("Origem", value("origin")),
            (
                "Data do documento",
                metadata
                    .get("document_date")
                    .map(|d| crate::labels::date(d))
                    .unwrap_or_else(|| "não informada".into()),
            ),
            ("Registrado por", value("registered_by")),
            ("Última versão", display_date(&detail.capture.captured_at)),
        ],
    );
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
        ui.label(label).on_hover_text("Link indisponível: somente endereços HTTPS do SIGADOC presentes nesta importação podem ser abertos.");
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
        allowed_sigadoc_url, library_panel, DocPriorityFilter, DocSort, DocTaskFilter,
        DocTaskStatusSummary, LIBRARY_DEFAULT_WIDTH, LIBRARY_MAX_FRACTION,
        LIBRARY_MIN_WIDTH,
    };
    use crate::tasks::Task;

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
            email: false,
            email_from: String::new(),
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
            Some("↳ Juntado a ORG-DIC-2026/97546")
        );
        assert_eq!(
            super::joined_label(&docs[4]).as_deref(),
            Some("↳ Juntado a ORG-DIC-2026/99999 (não importado)")
        );
        let mut via_only = summary(9, "X-2026/1", None);
        via_only.is_joined = true;
        assert_eq!(
            super::joined_label(&via_only).as_deref(),
            Some("↳ Documento juntado")
        );
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
        assert_eq!(s1.badge().1, crate::theme::error());
        assert!(s1.badge().0.contains("atrasada"));

        let s2 = DocTaskStatusSummary::from_tasks(&tasks, 2, today);
        assert!(s2.matches(DocTaskFilter::OpenOnTime));
        assert!(s2.matches(DocTaskFilter::OpenAny));
        assert!(!s2.matches(DocTaskFilter::Overdue));
        assert_eq!(s2.badge().1, crate::theme::on_time());
        assert_ne!(crate::theme::on_time(), crate::theme::success(), "open and done never share a colour");
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
    use super::sender_line;

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
