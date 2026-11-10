//! The document screen: summary, files, notes, technical data and the cards of each tab.
use super::library::joined_label;
use super::windows::manual_facts;
use super::{
    display_date, facts, info, notice_line, pill, plural, titled_card, Area, DocTaskStatusSummary,
    Leave, PonteApp, ReturnTo, Tab, WorkKind,
};
use crate::model::DocumentDetail;
use crate::theme::{self, icon, S1, S2, S4};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};

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

pub(super) fn allowed_sigadoc_url(raw_url: &str) -> bool {
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

impl PonteApp {
    /// On the ficha: the way back to where the person came from.
    pub(super) fn return_bar(&mut self, ui: &mut egui::Ui) {
        let Some(origin) = self.return_to else {
            return;
        };
        let label = match origin {
            ReturnTo::Today => "Voltar a Hoje",
            ReturnTo::Task(_) => "Voltar à tarefa",
            ReturnTo::Attendance(_) => "Voltar ao atendimento",
        };
        if !ui
            .add(widgets::quiet(format!("{} {label}", icon::BACK)))
            .clicked()
        {
            return;
        }
        match origin {
            ReturnTo::Today => self.navigate(Leave::Today),
            ReturnTo::Task(id) => {
                let task = self
                    .store
                    .list_tasks()
                    .ok()
                    .and_then(|tasks| tasks.into_iter().find(|t| t.id == id));
                match task {
                    Some(task) => self.navigate(Leave::OpenTask(task)),
                    None => self.go_tasks(),
                }
            }
            ReturnTo::Attendance(id) => {
                if self.saiop.focus_attendance(&self.store, id) {
                    self.navigate(Leave::Saiop);
                } else {
                    self.error("Há um atendimento com alterações não salvas em Atendimentos. Salve ou descarte antes de abrir outro.");
                }
            }
        }
    }

    pub(super) fn document(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        self.return_bar(ui);
        let id = detail.summary.id;
        let p = theme::c();
        let doc_tasks = self.store.list_tasks().unwrap_or_default();
        let today_iso = chrono::Local::now().format("%Y-%m-%d").to_string();
        let task_summary = DocTaskStatusSummary::from_tasks(&doc_tasks, id, &today_iso);
        let (badge_text, badge_tone) = task_summary.pill_parts();
        let source_link_ok =
            !detail.summary.manual && allowed_sigadoc_url(&detail.capture.source_url);

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
                        .add(widgets::quiet(format!(
                            "{} {}",
                            icon::JOINED,
                            child.document_number
                        )))
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
            let notes_tab =
                widgets::tab(ui, self.tab == Tab::Notes, "Notas e minutas", notes_dirty);
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

    pub(super) fn section<R>(
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
        titled_card(
            ui,
            "Nota",
            |_| {},
            |ui| {
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
            },
        );
        if edit {
            self.tab = Tab::Notes;
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
                    let account = metadata
                        .get("mail_account")
                        .filter(|a| !a.trim().is_empty());
                    match (account, metadata.get("mail_account_name")) {
                        (Some(address), Some(name)) if !name.trim().is_empty() => {
                            rows.push(("Recebido na conta", format!("{name} ({address})")));
                        }
                        (Some(address), _) => rows.push(("Recebido na conta", address.clone())),
                        (None, _) => match metadata.get("mail_to").filter(|t| !t.trim().is_empty())
                        {
                            Some(to) => rows.push(("Destinatário", to.clone())),
                            None => {
                                rows.push(("Conta de recebimento", "não registrada".to_owned()))
                            }
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
                                plural(
                                    detail.capture.references.len(),
                                    "referência",
                                    "referências"
                                )
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
        titled_card(
            ui,
            title,
            |_| {},
            |ui| {
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
            },
        );
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
                if ui.add_enabled(!busy, button).on_hover_text(hover).clicked() {
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
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
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
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(crate::labels::task_action(
                                                &suggestion.action,
                                            ))
                                            .font(theme::font_strong(theme::SIZE_BODY)),
                                        )
                                        .wrap(),
                                    );
                                    match existing {
                                        Some(task) => {
                                            ui.horizontal_wrapped(|ui| {
                                                pill(ui, "Tarefa criada", Tone::Ok);
                                                widgets::small_muted(ui, task.title.as_str());
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
                                });
                            });
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
                                    widgets::small_muted(
                                        ui,
                                        format!(
                                            "Fonte {} • importação nº {} • método {}",
                                            source.id, source.capture_id, source.method
                                        ),
                                    );
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

    pub(super) fn files_list(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
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
                        pill(
                            ui,
                            format!("{} KB", file.size.div_ceil(1024)),
                            Tone::Neutral,
                        );
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
                                Err(e) => {
                                    self.error(format!("Não foi possível exportar o PDF: {e}"))
                                }
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
                        .on_hover_text(format!(
                            "Identificação completa do arquivo (SHA-256): {}",
                            file.sha256
                        ));
                    });
                }
            }
            Err(e) => {
                notice_line(
                    ui,
                    Tone::Warn,
                    format!("Não foi possível ler os arquivos: {e}"),
                );
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

    fn coverage_and_references(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        ui.label(format!(
            "{} caracteres de texto • {}",
            detail.capture.text.chars().count(),
            plural(detail.capture.references.len(), "referência", "referências")
        ));
        if detail.capture.coverage.recognized.is_empty() {
            notice_line(
                ui,
                Tone::Warn,
                "Nenhuma parte da página foi marcada como reconhecida.",
            );
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

    pub(super) fn notes(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let id = detail.summary.id;
        let mut save = false;
        let mut discard = false;
        let dirty = self.editors[&id].note_dirty();
        titled_card(
            ui,
            "Notas",
            |ui| {
                if ui
                    .add_enabled(
                        dirty,
                        widgets::primary(format!("{} Salvar notas", icon::SAVE)),
                    )
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
}
