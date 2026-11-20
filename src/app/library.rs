//! The document list on the left (filters, grouping, rows) and the welcome screen.
use super::{
    notice_line, pill, plural, DocOriginFilter, DocPriorityFilter, DocSort, DocTaskFilter,
    DocTaskStatusSummary, Editor, Leave, PonteApp, WorkKind,
};
use crate::categories::Category;
use crate::model::DocumentSummary;
use crate::theme::{self, icon, S1, S2, S3, S4};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

/// Side margin of the library panel, which the rows of the list bleed into.
const LIST_BLEED: f32 = 14.0;

/// Everything the document list derives from the library and from its
/// controls. The list is rebuilt only when one of these changes, not on every
/// frame: the library (`stamp`: a save here, a synchronization or the team
/// server there), the day (the tasks' deadlines are compared with it), the
/// filters and the order, and the documents the list was read for.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DocListKey {
    /// `Store::library_stamp`; `None` when it could not be read, and then the
    /// list is never taken for current.
    stamp: Option<(i64, i64)>,
    today: String,
    task: DocTaskFilter,
    priority: DocPriorityFilter,
    origin: DocOriginFilter,
    category: crate::category_ui::CategoryFilter,
    sort: DocSort,
    /// Moves every time `PonteApp::documents` is replaced (a search changes
    /// the documents without changing the library).
    documents_gen: u64,
    documents_len: usize,
}

/// How many documents each task filter (and the origin one) would list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DocCounts {
    open_any: usize,
    on_time: usize,
    overdue: usize,
    completed: usize,
    manual: usize,
}

/// The document list, derived once per change of its key.
pub(super) struct DocListView {
    key: DocListKey,
    summaries: HashMap<i64, DocTaskStatusSummary>,
    /// The summary of a document with no task.
    no_tasks: DocTaskStatusSummary,
    attachment_counts: BTreeMap<i64, usize>,
    counts: DocCounts,
    /// Positions in `PonteApp::documents` of what the filters leave, in the
    /// chosen order.
    visible: Vec<usize>,
}

impl DocListView {
    fn build(
        key: DocListKey,
        documents: &[DocumentSummary],
        tasks: &[crate::tasks::Task],
        attachment_counts: BTreeMap<i64, usize>,
    ) -> Self {
        let summaries = DocTaskStatusSummary::by_document(tasks, &key.today);
        let no_tasks = DocTaskStatusSummary::default();
        let mut counts = DocCounts::default();
        for doc in documents {
            let summary = summaries.get(&doc.id).unwrap_or(&no_tasks);
            counts.open_any += usize::from(summary.matches(DocTaskFilter::OpenAny));
            counts.on_time += usize::from(summary.matches(DocTaskFilter::OpenOnTime));
            counts.overdue += usize::from(summary.matches(DocTaskFilter::Overdue));
            counts.completed += usize::from(summary.matches(DocTaskFilter::Completed));
            counts.manual += usize::from(doc.manual);
        }
        let mut view = Self {
            key,
            summaries,
            no_tasks,
            attachment_counts,
            counts,
            visible: Vec::new(),
        };
        let mut visible: Vec<&DocumentSummary> = documents
            .iter()
            .filter(|doc| {
                view.summary_of(doc.id).matches(view.key.task)
                    && view.key.priority.matches(&doc.priority)
                    && view.key.origin.matches(doc.manual)
                    && view.key.category.matches(&doc.categories)
            })
            .collect();
        view.key
            .sort
            .apply(&mut visible, |id| view.summary_of(id).nearest_due.clone());
        // `apply` reorders references into `documents`; their positions are
        // what is kept.
        let position: HashMap<i64, usize> = documents
            .iter()
            .enumerate()
            .map(|(index, doc)| (doc.id, index))
            .collect();
        view.visible = visible.iter().map(|doc| position[&doc.id]).collect();
        view
    }

    fn summary_of(&self, id: i64) -> &DocTaskStatusSummary {
        self.summaries.get(&id).unwrap_or(&self.no_tasks)
    }
}

impl PonteApp {
    fn doc_list_key(&self, today: &str) -> DocListKey {
        DocListKey {
            stamp: self.store.library_stamp(),
            today: today.to_owned(),
            task: self.doc_task_filter,
            priority: self.doc_priority_filter,
            origin: self.doc_origin_filter,
            category: self.doc_category_filter.clone(),
            sort: self.doc_sort,
            documents_gen: self.documents_gen,
            documents_len: self.documents.len(),
        }
    }

    /// Makes `self.doc_list` current: rebuilt only when the library, the day,
    /// the filters, the order or the documents changed since it was built.
    pub(super) fn ensure_doc_list(&mut self, today: &str) {
        let key = self.doc_list_key(today);
        if key.stamp.is_some() && self.doc_list.as_ref().is_some_and(|view| view.key == key) {
            return;
        }
        let tasks = self.store.list_tasks();
        let attachments = self.store.attachment_counts("document");
        let mut key = key;
        if tasks.is_err() || attachments.is_err() {
            // Not kept as current: the next frame reads again, as it did
            // when nothing was kept.
            key.stamp = None;
        }
        self.doc_list = Some(DocListView::build(
            key,
            &self.documents,
            &tasks.unwrap_or_default(),
            attachments.unwrap_or_default(),
        ));
    }
}

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
    /// Every category of the library, to draw the chips of the row.
    categories: &'a [Category],
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
        let date_galley =
            content
                .painter()
                .layout_no_wrap(date, theme::font(theme::SIZE_SMALL), p.muted);
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
        let (line, _) =
            content.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
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
                if document.email {
                    "E-mail"
                } else if document.whatsapp {
                    "WhatsApp"
                } else {
                    "Manual"
                },
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
                plural(document.joined_children_count, "juntado", "juntados"),
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
        crate::category_ui::category_chips_compact(ui, &document.categories, row.categories, 2);
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

/// Pill text for a juntado document, or None for an ordinary one.
pub(super) fn joined_label(doc: &DocumentSummary) -> Option<String> {
    match (&doc.joined_to_number, doc.is_joined) {
        (Some(number), _) if doc.joined_to_id.is_some() => Some(format!("↳ Juntado a {number}")),
        (Some(number), _) => Some(format!("↳ Juntado a {number} (não importado)")),
        (None, true) => Some("↳ Documento juntado".into()),
        (None, false) => None,
    }
}

/// Orders the visible list so juntados sit right below their visible parent,
/// keeping the original order otherwise. Returns each document with its depth.
pub(super) fn group_joined<'a>(docs: &[&'a DocumentSummary]) -> Vec<(&'a DocumentSummary, usize)> {
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

/// The sender of an e-mail for the document list: the address first, then
/// the name ("Funadesp <f@x.br>" → "f@x.br · Funadesp").
pub(super) fn sender_line(from: &str) -> String {
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

impl PonteApp {
    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
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
        if std::mem::take(&mut self.focus_search) {
            response.request_focus();
        }
        if response.changed() {
            self.search_changed = Some(Instant::now());
        }

        let today_iso = chrono::Local::now().format("%Y-%m-%d").to_string();
        // One pass over the tasks, once per change of the library, the day,
        // the documents or the controls (not once per frame).
        self.ensure_doc_list(&today_iso);
        let counts = self
            .doc_list
            .as_ref()
            .map(|view| view.counts)
            .unwrap_or_default();
        let (count_open_any, count_on_time, count_overdue, count_completed) = (
            counts.open_any,
            counts.on_time,
            counts.overdue,
            counts.completed,
        );
        let all_categories = self.store.list_categories().unwrap_or_default();

        let filters_active = self.doc_task_filter != DocTaskFilter::All
            || self.doc_priority_filter != DocPriorityFilter::All
            || self.doc_origin_filter != DocOriginFilter::All
            || self.doc_category_filter.is_active();
        // Filters that only the "Filtros" chip shows; the chip stays filled
        // while one of them (or the panel) is on.
        let hidden_filter_on = matches!(
            self.doc_task_filter,
            DocTaskFilter::OpenOnTime | DocTaskFilter::Completed
        ) || self.doc_priority_filter != DocPriorityFilter::All
            || self.doc_origin_filter != DocOriginFilter::All
            || self.doc_category_filter.is_active();
        ui.add_space(S1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(S1 + 2.0, S1 + 2.0);
            if widgets::chip(ui, self.doc_task_filter == DocTaskFilter::All, "Todos").clicked() {
                self.doc_task_filter = DocTaskFilter::All;
            }
            if widgets::chip(
                ui,
                self.doc_task_filter == DocTaskFilter::OpenAny,
                "Com tarefa",
            )
            .on_hover_text(format!("Documentos com tarefa aberta ({count_open_any})"))
            .clicked()
            {
                self.doc_task_filter = DocTaskFilter::OpenAny;
            }
            if (count_overdue > 0 || self.doc_task_filter == DocTaskFilter::Overdue)
                && widgets::chip(
                    ui,
                    self.doc_task_filter == DocTaskFilter::Overdue,
                    format!("Atrasados · {count_overdue}"),
                )
                .on_hover_text("Mostrar só documentos com tarefa atrasada")
                .clicked()
            {
                self.doc_task_filter = DocTaskFilter::Overdue;
            }
            let label = format!(
                "Filtros {}",
                if self.filters_open {
                    icon::UP
                } else {
                    icon::DOWN
                }
            );
            let enabled = !self.documents.is_empty() || filters_active;
            if widgets::chip(ui, self.filters_open || hidden_filter_on, label).clicked() && enabled
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
                    if !all_categories.is_empty() || self.doc_category_filter.is_active() {
                        ui.add_space(S1);
                        widgets::field_label(ui, "Categorias");
                        crate::category_ui::category_filter_button(
                            ui,
                            "documents",
                            &mut self.doc_category_filter,
                            &all_categories,
                        );
                    }
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
                    let manual_count = counts.manual;
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

        // A chip clicked above changes the list in this same frame.
        self.ensure_doc_list(&today_iso);
        let list_view = self.doc_list.as_ref();
        let no_tasks = DocTaskStatusSummary::default();
        let filtered_docs: Vec<&DocumentSummary> = list_view
            .map(|view| view.visible.iter().map(|&i| &self.documents[i]).collect())
            .unwrap_or_default();
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
            self.doc_category_filter.clear();
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
                    let summary = list_view.map_or(&no_tasks, |view| view.summary_of(document.id));
                    let row = DocRow {
                        document,
                        number,
                        summary,
                        attachments: list_view
                            .and_then(|view| view.attachment_counts.get(&document.id))
                            .copied()
                            .unwrap_or(0),
                        indent,
                        selected: self.selected_id == Some(document.id),
                        editing,
                        joined,
                        categories: &all_categories,
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
                        self.doc_category_filter.clear();
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

    /// First screen: a checklist of first steps (shown in full while the
    /// library is empty, only while a required step is pending afterwards)
    /// and the other ways to bring a document in.
    pub(super) fn welcome(&mut self, ui: &mut egui::Ui) {
        let status = self.install_status();
        // The whole library (memoized), not the list narrowed by the search.
        let all_documents = self.store.list_documents("").unwrap_or_default();
        let inputs = FirstSteps {
            bridge_ok: status.browser_bridge_ok(),
            has_sigadoc_document: all_documents.iter().any(|d| !d.manual),
        };
        let library_empty = all_documents.is_empty();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(780.0);
                ui.spacing_mut().item_spacing.y = S4;
                ui.add_space(S2);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::heading(ui, "Seu espaço de trabalho documental");
                    widgets::muted(
                        ui,
                        if library_empty {
                            "Siga os primeiros passos abaixo para importar o seu primeiro documento."
                        } else {
                            "Escolha um documento na lista à esquerda."
                        },
                    );
                });
                if library_empty || inputs.required_pending() {
                    self.first_steps_card(ui, &inputs, &status);
                }
                self.other_sources_card(ui);
                widgets::small_muted(ui, "A IA é opcional e só funciona quando você clica. Assinaturas e tramitação continuam no SIGADOC.");
            });
    }

    /// The card "Primeiros passos": one row per step, the pending ones open.
    fn first_steps_card(
        &mut self,
        ui: &mut egui::Ui,
        inputs: &FirstSteps,
        status: &crate::windows_ui::InstallStatus,
    ) {
        let p = theme::c();
        widgets::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = S3;
            ui.horizontal(|ui| {
                widgets::section(ui, "Primeiros passos");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(widgets::secondary(format!("{} Abrir o guia do usuário", icon::BOOK)))
                        .on_hover_text("Guia curto, em dez seções, aberto no navegador. Também em Ajuda, na barra lateral.")
                        .clicked()
                    {
                        self.open_guide();
                    }
                });
            });
            for step in FirstStep::ALL {
                let done = inputs.is_done(step);
                let expanded = inputs.is_expanded(step);
                let (title, detail) = match step {
                    FirstStep::Bridge => (
                        "Ponte ligado ao navegador",
                        if done {
                            format!("Registrado para o {}.", browsers_label(status))
                        } else {
                            String::new()
                        },
                    ),
                    FirstStep::Extension => (
                        "Extensão instalada no Chrome ou no Edge",
                        if done {
                            "Confirmada: já chegou um documento por ela.".to_owned()
                        } else {
                            String::new()
                        },
                    ),
                    FirstStep::FirstDocument => (
                        "Primeiro documento importado",
                        if done {
                            "Já há documentos do SIGADOC no acervo.".to_owned()
                        } else {
                            String::new()
                        },
                    ),
                };
                step_row(ui, done, title, &detail);
                if !expanded {
                    continue;
                }
                help_indent(ui, |ui| match step {
                    FirstStep::Bridge => {
                        if status.chrome_registered || status.edge_registered {
                            notice_line(ui, Tone::Warn, format!("Registrado para o {}, mas o programa apontado não foi encontrado. Reinstale o Ponte.", browsers_label(status)));
                        } else {
                            notice_line(ui, Tone::Danger, "O navegador ainda não foi ligado ao Ponte. Execute o instalador neste computador, com a sua conta do Windows.");
                        }
                    }
                    FirstStep::Extension => self.extension_help(ui, status),
                    FirstStep::FirstDocument => {
                        let steps = [
                            "Abra o documento no SIGADOC, no Chrome ou no Edge.",
                            "Clique em “Importar para o assistente”, no canto da página.",
                            "O documento aparece aqui com o texto, os PDFs e o histórico.",
                        ];
                        for (index, text) in steps.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(26.0, 26.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .circle_filled(rect.center(), 13.0, p.primary_soft);
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    (index + 1).to_string(),
                                    theme::font_strong(theme::SIZE_BODY),
                                    p.primary,
                                );
                                ui.add(egui::Label::new(*text).wrap());
                            });
                        }
                    }
                });
            }
            ui.separator();
            // Optional items: never counted as pending.
            optional_row(ui, "Inteligência artificial (opcional)");
            ui.horizontal(|ui| {
                help_indent(ui, |ui| {
                    widgets::small_muted(
                        ui,
                        "Para resumir e sugerir providências. Só age quando você clica.",
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(widgets::quiet("Configurar")).clicked() {
                        self.settings_open = true;
                    }
                });
            });
            optional_row(ui, "Mais de um computador ou celular (opcional)");
            ui.horizontal(|ui| {
                help_indent(ui, |ui| {
                    widgets::small_muted(
                        ui,
                        "Para levar o acervo ao celular ou dividir o trabalho do gabinete.",
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(widgets::quiet("Configurar")).clicked() {
                        self.intranet.open = true;
                    }
                });
            });
        });
    }

    /// Help of the extension step: what to look for, the store link and the
    /// folder to load by hand.
    fn extension_help(&mut self, ui: &mut egui::Ui, status: &crate::windows_ui::InstallStatus) {
        let p = theme::c();
        ui.add(
            egui::Label::new(
                "No SIGADOC, o botão “Importar para o assistente” aparece no canto da página.",
            )
            .wrap(),
        );
        widgets::small_muted(ui, "O Ponte só sabe que a extensão funciona depois que o primeiro documento chega por ela.");
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(widgets::primary(format!("{} Abrir a loja de extensões", icon::LINK)))
                .on_hover_text("Abre a página da extensão na Chrome Web Store. No Edge, a loja pede para permitir extensões de outras lojas.")
                .clicked()
            {
                if let Err(error) = crate::windows_ui::open_url(&store_url()) {
                    self.error(format!("Não foi possível abrir a loja: {error:#}"));
                }
            }
        });
        egui::CollapsingHeader::new("Instalar pela pasta (alternativa)")
            .id_salt("welcome-extension-folder")
            .show(ui, |ui| match &status.extension_dir {
                Some(dir) => {
                    widgets::small_muted(ui, "Na página de extensões do navegador, ative o Modo do desenvolvedor, clique em “Carregar sem compactação” e escolha esta pasta.");
                    ui.add(
                        egui::Label::new(RichText::new(dir.display().to_string()).color(p.muted))
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
            });
    }

    /// The card "Algo que não está no SIGADOC?": manual records and e-mail.
    fn other_sources_card(&mut self, ui: &mut egui::Ui) {
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
    }
}

/// The required steps of the checklist, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FirstStep {
    Bridge,
    Extension,
    FirstDocument,
}

impl FirstStep {
    const ALL: [FirstStep; 3] = [
        FirstStep::Bridge,
        FirstStep::Extension,
        FirstStep::FirstDocument,
    ];
}

/// Plain inputs of the checklist. The app records nothing about the extension
/// contacting it, so a document from the SIGADOC is the only proof that the
/// extension works (and that the first import happened).
#[derive(Clone, Copy, Debug)]
struct FirstSteps {
    bridge_ok: bool,
    has_sigadoc_document: bool,
}

impl FirstSteps {
    fn is_done(&self, step: FirstStep) -> bool {
        match step {
            FirstStep::Bridge => self.bridge_ok,
            FirstStep::Extension | FirstStep::FirstDocument => self.has_sigadoc_document,
        }
    }

    /// Only a pending step shows its help.
    fn is_expanded(&self, step: FirstStep) -> bool {
        !self.is_done(step)
    }

    fn required_pending(&self) -> bool {
        FirstStep::ALL.iter().any(|s| !self.is_done(*s))
    }
}

/// Address of the extension on the Chrome Web Store, from the identifier the
/// installer already authorises.
fn store_url() -> String {
    format!(
        "https://chromewebstore.google.com/detail/{}",
        crate::native::STORE_EXTENSION_ID.trim()
    )
}

fn browsers_label(status: &crate::windows_ui::InstallStatus) -> &'static str {
    match (status.chrome_registered, status.edge_registered) {
        (true, true) => "Chrome e Edge",
        (true, false) => "Chrome",
        (false, true) => "Edge",
        (false, false) => "nenhum navegador",
    }
}

/// One line of the checklist: state icon, title, and a muted detail.
fn step_row(ui: &mut egui::Ui, done: bool, title: &str, detail: &str) {
    let p = theme::c();
    ui.horizontal_wrapped(|ui| {
        let (glyph, color) = if done {
            (icon::CHECK, Tone::Ok.colors().0)
        } else {
            (icon::CIRCLE, p.muted)
        };
        ui.label(
            RichText::new(glyph)
                .color(color)
                .size(theme::SIZE_BODY + 2.0),
        );
        ui.label(RichText::new(title).font(theme::font_strong(theme::SIZE_BODY)));
        if !detail.is_empty() {
            widgets::small_muted(ui, detail);
        }
    });
}

fn optional_row(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icon::CIRCLE)
                .color(theme::c().muted)
                .size(theme::SIZE_BODY + 2.0),
        );
        ui.label(RichText::new(title).font(theme::font_strong(theme::SIZE_BODY)));
    });
}

/// Indents the help of a row under its title.
fn help_indent<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) {
    ui.horizontal(|ui| {
        ui.add_space(S4 + S2);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = S2;
            add(ui)
        });
    });
}

#[cfg(test)]
mod welcome_tests {
    use super::*;

    #[test]
    fn nothing_done_expands_every_required_step() {
        let s = FirstSteps {
            bridge_ok: false,
            has_sigadoc_document: false,
        };
        assert!(s.required_pending());
        for step in FirstStep::ALL {
            assert!(!s.is_done(step));
            assert!(s.is_expanded(step));
        }
    }

    #[test]
    fn bridge_ready_without_documents_keeps_extension_and_import_open() {
        let s = FirstSteps {
            bridge_ok: true,
            has_sigadoc_document: false,
        };
        assert!(s.is_done(FirstStep::Bridge) && !s.is_expanded(FirstStep::Bridge));
        assert!(s.is_expanded(FirstStep::Extension));
        assert!(s.is_expanded(FirstStep::FirstDocument));
        assert!(s.required_pending());
    }

    #[test]
    fn a_sigadoc_document_proves_extension_and_first_import() {
        let s = FirstSteps {
            bridge_ok: true,
            has_sigadoc_document: true,
        };
        assert!(FirstStep::ALL
            .iter()
            .all(|x| s.is_done(*x) && !s.is_expanded(*x)));
        assert!(!s.required_pending());
    }

    #[test]
    fn broken_bridge_stays_pending_even_with_documents() {
        let s = FirstSteps {
            bridge_ok: false,
            has_sigadoc_document: true,
        };
        assert!(s.is_expanded(FirstStep::Bridge));
        assert!(!s.is_expanded(FirstStep::Extension));
        assert!(s.required_pending());
    }

    #[test]
    fn store_url_uses_the_trimmed_identifier() {
        let url = store_url();
        assert!(url.starts_with("https://chromewebstore.google.com/detail/"));
        assert!(!url.contains(char::is_whitespace));
    }
}

#[cfg(test)]
mod doc_list_tests {
    use super::*;
    use crate::storage::Store;
    use crate::tasks::Task;

    fn app(dir: &tempfile::TempDir) -> PonteApp {
        let path = dir.path().join("acervo.sqlite3");
        PonteApp::new(Store::open(&path).unwrap(), path)
    }

    fn task_due(document: i64, due: &str) -> Task {
        let mut task = Task::new(document);
        task.title = format!("Tarefa {due}");
        task.due_date = due.to_owned();
        task
    }

    fn load_documents(app: &mut PonteApp) {
        let documents = app.store.list_documents("").unwrap();
        app.set_documents(documents);
    }

    /// Marks the derived list; the mark survives only while it is not rebuilt.
    fn mark(app: &mut PonteApp) {
        app.doc_list.as_mut().unwrap().no_tasks.completed = 99;
    }
    fn marked(app: &PonteApp) -> bool {
        app.doc_list.as_ref().unwrap().no_tasks.completed == 99
    }

    #[test]
    fn the_list_is_rebuilt_when_the_library_the_day_or_the_controls_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        let receipt = app
            .store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let document = receipt.document_id;
        load_documents(&mut app);
        app.ensure_doc_list("2026-11-20");
        let first = app.doc_list.as_ref().unwrap().counts;
        assert_eq!(first.overdue, 0);

        // Nothing changed: the same list, not built again.
        mark(&mut app);
        app.ensure_doc_list("2026-11-20");
        assert!(marked(&app), "rebuilt with nothing changed");

        // A save on this connection.
        app.store
            .save_task(&task_due(document, "2026-11-01"))
            .unwrap();
        app.ensure_doc_list("2026-11-20");
        assert!(!marked(&app), "a save did not rebuild the list");
        assert_eq!(app.doc_list.as_ref().unwrap().counts.overdue, 1);
        assert_eq!(
            app.doc_list.as_ref().unwrap().summary_of(document).overdue,
            1
        );

        // A commit by another connection (the team server, a synchronization):
        // here the task becomes the second late one, but the document counts
        // once; the summary shows it.
        mark(&mut app);
        let mut other = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        other.save_task(&task_due(document, "2026-11-02")).unwrap();
        app.ensure_doc_list("2026-11-20");
        assert!(!marked(&app), "another connection's commit did not rebuild");
        assert_eq!(
            app.doc_list.as_ref().unwrap().summary_of(document).overdue,
            2
        );

        // The day changes with the library untouched: deadlines are compared
        // with the new day.
        mark(&mut app);
        app.ensure_doc_list("2026-10-01");
        assert!(!marked(&app), "a new day did not rebuild the list");
        let summary = app.doc_list.as_ref().unwrap().summary_of(document);
        assert_eq!(summary.overdue, 0);
        assert!(summary.open_on_time >= 2);

        // A filter or the order, and a new set of documents.
        app.ensure_doc_list("2026-11-20");
        assert_eq!(app.doc_list.as_ref().unwrap().visible.len(), 1);
        mark(&mut app);
        app.doc_task_filter = DocTaskFilter::Completed;
        app.ensure_doc_list("2026-11-20");
        assert!(!marked(&app), "a filter did not rebuild the list");
        assert!(app.doc_list.as_ref().unwrap().visible.is_empty());
        app.doc_task_filter = DocTaskFilter::All;
        app.ensure_doc_list("2026-11-20");
        mark(&mut app);
        app.doc_sort = DocSort::Number;
        app.ensure_doc_list("2026-11-20");
        assert!(!marked(&app), "the order did not rebuild the list");
        mark(&mut app);
        load_documents(&mut app);
        app.ensure_doc_list("2026-11-20");
        assert!(!marked(&app), "new documents did not rebuild the list");
    }

    #[test]
    fn one_pass_summaries_match_the_per_document_ones_and_keep_documents_without_tasks() {
        let today = "2026-11-20";
        let mut tasks = vec![
            task_due(101, "2026-11-01"),
            task_due(101, "2026-12-01"),
            task_due(102, today),
            task_due(102, ""),
        ];
        tasks[1].status = "concluida".into();
        tasks.push(Task::standalone());
        let by_document = DocTaskStatusSummary::by_document(&tasks, today);
        for id in [101, 102] {
            assert_eq!(
                by_document.get(&id),
                Some(&DocTaskStatusSummary::from_tasks(&tasks, id, today))
            );
        }
        assert!(!by_document.contains_key(&103));
        // A document with no task is not listed under "Com tarefa".
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let documents = store.list_documents("").unwrap();
        assert_eq!(documents.len(), 1);
        let key = DocListKey {
            stamp: Some((0, 0)),
            today: today.into(),
            task: DocTaskFilter::OpenAny,
            priority: DocPriorityFilter::All,
            origin: DocOriginFilter::All,
            category: Default::default(),
            sort: DocSort::Recent,
            documents_gen: 0,
            documents_len: 1,
        };
        let view = DocListView::build(key, &documents, &tasks, BTreeMap::new());
        assert!(view.visible.is_empty());
        assert_eq!(view.counts.open_any, 0);
    }
}
