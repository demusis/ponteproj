//! The document list on the left (filters, grouping, rows) and the welcome screen.
use super::{
    notice_line, pill, plural, DocOriginFilter, DocPriorityFilter, DocSort, DocTaskFilter,
    DocTaskStatusSummary, Editor, Leave, PonteApp, WorkKind,
};
use crate::model::DocumentSummary;
use crate::theme::{self, icon, S1, S2, S3, S4};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};
use std::collections::BTreeMap;
use std::time::Instant;

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

    /// First screen: how to import, and whether the browser bridge is ready.
    /// First screen: how to import, and whether the browser bridge is ready.
    pub(super) fn welcome(&mut self, ui: &mut egui::Ui) {
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
}
