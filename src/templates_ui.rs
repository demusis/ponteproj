//! "Modelos de trabalho" panel: list, structure editor, instruction library,
//! examples, references and a preview of what a template adds to a request.
//! Nothing here talks to the AI; the preview only assembles the text.
use crate::{
    model::Capture,
    storage::Store,
    work_templates::{
        detect_variables, owner_key, version_key, BlockMode, BlockRef, Example, ExampleStatus,
        InstructionBlock, Intent, Reference, Scope, Section, Status, TemplateVersion, Variable,
        WorkTemplate, MAX_BLOCKS_PER_TEMPLATE,
    },
};
use crate::{
    theme::{self, icon},
    widgets::{self, Tone},
};
use eframe::egui::{self, RichText};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const KINDS: [&str; 3] = ["resposta", "encaminhamento", "complementacao"];

/// What the panel asks the application to do.
pub enum TemplatesAction {
    UseTemplate { uid: String },
    None,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Structure,
    Blocks,
    Examples,
    References,
    Test,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Template,
    Block,
    Example,
}

enum Op {
    Up,
    Down,
    Remove,
}

struct Preview {
    trusted: usize,
    blocks: usize,
    examples: usize,
    files: usize,
    conflicts: Vec<String>,
    system: String,
}

#[derive(Default)]
pub struct TemplatesPanel {
    templates: Vec<WorkTemplate>,
    blocks: Vec<InstructionBlock>,
    loaded: Option<Instant>,
    stale: bool,
    filter_kind: String,
    show_archived: bool,
    only_mine: bool,
    message: String,
    error: bool,
    tab: Tab,
    edit: Option<WorkTemplate>,
    base: String,
    block_edit: Option<InstructionBlock>,
    block_base: String,
    example_edit: Option<Example>,
    example_base: String,
    /// Versions of the stored items as they were when loaded into the
    /// editors. Saves check against these, not against the store at click
    /// time, so a colleague's change that arrived by sync in the meantime is
    /// refused instead of silently overwritten.
    template_version_seen: Option<i64>,
    block_version_seen: Option<i64>,
    example_version_seen: Option<i64>,
    /// Discard clicked on unsaved changes; waiting for confirmation.
    confirm_discard_template: bool,
    confirm_discard_block: bool,
    confirm_discard_example: bool,
    attachments: crate::attachments::AttachmentsPanel,
    file_review: Option<(i64, String)>,
    /// "Extrair de novo…" clicked on a reviewed text; waiting for confirmation.
    confirm_reextract: Option<i64>,
    /// Text records of the open example's files: (example id, attachment
    /// ids, records), rebuilt when the list or a record changes.
    file_texts: Option<(
        i64,
        Vec<i64>,
        Vec<Option<crate::extraction::AttachmentText>>,
    )>,
    /// A PDF of an example to open in the Ponte reader: (attachment, label).
    pdf_request: Option<(i64, String)>,
    confirm: Option<Confirm>,
    dup_name: Option<String>,
    test_selected: BTreeSet<String>,
    test_guidance: String,
    test_result: Option<Result<Preview, String>>,
    /// Text of the search box above the list.
    search: String,
    /// Focus the search box on the next frame (Ctrl+F).
    focus_search: bool,
    /// "New template" asked by the keyboard; started on the next frame.
    pending_new: bool,
}

/// Lowercase without accents, for comparing names.
fn fold(value: &str) -> String {
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

/// True when `name` contains `query`, ignoring case and accents. An empty
/// query matches everything.
fn name_matches(name: &str, query: &str) -> bool {
    let query = fold(query.trim());
    query.is_empty() || fold(name).contains(&query)
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn expected(store: &Store, kind: &str, uid: &str) -> Option<i64> {
    if uid.is_empty() {
        return None;
    }
    store
        .item_version(kind, version_key(uid))
        .ok()
        .map(|m| m.version)
}

/// Feminine badge for instructions ("Compartilhada com a equipe"), matching
/// the instruction editor; templates keep the masculine labels.
fn scope_label_f(scope: Scope) -> &'static str {
    match scope {
        Scope::Personal => "Pessoal",
        Scope::Shared => "Compartilhada com a equipe",
    }
}

fn status_label_f(status: Status) -> &'static str {
    match status {
        Status::Draft => "Rascunho",
        Status::Published => "Publicada",
        Status::Archived => "Arquivada",
    }
}

/// Width of the three icon buttons (up, down, remove) of a list row.
const OPS_WIDTH: f32 = 3.0 * 30.0 + 2.0 * theme::S1;
/// Room taken by an "Obrigatória" checkbox next to a field.
const CHECK_WIDTH: f32 = 112.0;

fn tone_of_status(status: Status) -> Tone {
    match status {
        Status::Draft => Tone::Warn,
        Status::Published => Tone::Ok,
        Status::Archived => Tone::Neutral,
    }
}

fn short_scope(scope: Scope) -> &'static str {
    match scope {
        Scope::Personal => "Pessoal",
        Scope::Shared => "Equipe",
    }
}

fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    widgets::field_label(ui, label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    ui.add_space(theme::S2);
}

fn text_area(ui: &mut egui::Ui, label: &str, value: &mut String, rows: usize) {
    widgets::field_label(ui, label);
    ui.add(
        egui::TextEdit::multiline(value)
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(theme::S2);
}

/// Move up, move down and remove, as icon buttons.
fn list_ops(ui: &mut egui::Ui, index: usize, len: usize) -> Option<Op> {
    ui.horizontal(|ui| {
        let mut op = None;
        ui.spacing_mut().item_spacing.x = theme::S1;
        if ui
            .add_enabled_ui(index > 0, |ui| widgets::icon_button(ui, icon::UP, "Subir"))
            .inner
            .clicked()
        {
            op = Some(Op::Up);
        }
        if ui
            .add_enabled_ui(index + 1 < len, |ui| {
                widgets::icon_button(ui, icon::DOWN, "Descer")
            })
            .inner
            .clicked()
        {
            op = Some(Op::Down);
        }
        if widgets::icon_button(ui, icon::TRASH, "Remover").clicked() {
            op = Some(Op::Remove);
        }
        op
    })
    .inner
}

fn string_list(ui: &mut egui::Ui, id: &str, items: &mut Vec<String>, hint: &str, max: usize) {
    let mut pending: Option<(usize, Op)> = None;
    let len = items.len();
    for (i, item) in items.iter_mut().enumerate() {
        ui.push_id((id, i), |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(item)
                        .hint_text(hint)
                        .desired_width(ui.available_width() - OPS_WIDTH - theme::S2),
                );
                if let Some(op) = list_ops(ui, i, len) {
                    pending = Some((i, op));
                }
            });
        });
    }
    if let Some((i, op)) = pending {
        apply_op(items, i, op);
    }
    if items.len() < max
        && ui
            .add(widgets::secondary(format!("{} Adicionar", icon::PLUS)))
            .clicked()
    {
        items.push(String::new());
    }
}

/// "v3 · rascunho pendente" under a list row.
fn version_text(current: u32, pending: bool) -> String {
    let mut text = String::new();
    if current > 0 {
        text.push_str(&format!("v{current}"));
    }
    if pending {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        text.push_str("rascunho pendente");
    }
    text
}

/// One row of a list: thin line below, soft fill and a bar on the left when
/// selected. `pad` is the side padding.
fn pick_row(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    selected: bool,
    pad: i8,
    add: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let p = theme::c();
    let full = ui.available_width();
    let shape = ui.painter().add(egui::Shape::Noop);
    let rect = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(pad, 8))
        .show(ui, |ui| {
            ui.set_width((full - 2.0 * f32::from(pad)).max(0.0));
            ui.style_mut().interaction.selectable_labels = false;
            ui.spacing_mut().item_spacing.y = theme::S1;
            add(ui);
        })
        .response
        .rect;
    let response = ui
        .interact(rect, ui.make_persistent_id(id), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if selected {
        Some(p.primary_soft)
    } else if response.hovered() {
        Some(p.sunken)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().set(
            shape,
            egui::Shape::rect_filled(rect, egui::CornerRadius::ZERO, fill),
        );
    }
    if selected {
        let bar = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 3.0, rect.bottom()),
        );
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::ZERO, p.primary);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, p.outline),
    );
    response
}

/// The name of a row, semibold when selected.
fn row_name(ui: &mut egui::Ui, name: &str, selected: bool) {
    let font = if selected {
        theme::font_strong(theme::SIZE_BODY)
    } else {
        theme::font(theme::SIZE_BODY)
    };
    ui.add(egui::Label::new(RichText::new(name).font(font)).truncate());
}

/// A short question in a soft frame with a confirm and a cancel button.
/// `Some(true)` = confirmed, `Some(false)` = cancelled.
fn confirm_line(ui: &mut egui::Ui, text: &str, yes: &str, no: &str) -> Option<bool> {
    let (fg, bg) = Tone::Warn.colors();
    let mut out = None;
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} {text}", icon::WARNING)).color(fg));
                if ui.add(widgets::danger(yes)).clicked() {
                    out = Some(true);
                }
                if ui.add(widgets::secondary(no)).clicked() {
                    out = Some(false);
                }
            });
        });
    out
}

fn pill_for_example(ui: &mut egui::Ui, status: ExampleStatus) {
    let tone = match status {
        ExampleStatus::Unverified => Tone::Warn,
        ExampleStatus::Approved => Tone::Ok,
        ExampleStatus::Rejected => Tone::Neutral,
    };
    widgets::pill(ui, status.label(), tone);
}

fn apply_op<T>(items: &mut Vec<T>, index: usize, op: Op) {
    match op {
        Op::Up if index > 0 => items.swap(index, index - 1),
        Op::Down if index + 1 < items.len() => items.swap(index, index + 1),
        Op::Remove => {
            items.remove(index);
        }
        _ => {}
    }
}

impl TemplatesPanel {
    pub fn dirty(&self) -> bool {
        self.template_dirty()
            || self.block_dirty()
            || self.example_dirty()
            || self.attachments.dirty()
            || self.file_review.is_some()
    }

    /// Ctrl+F: focus the search box of the list on the next frame.
    pub fn key_find(&mut self) {
        self.focus_search = true;
    }

    /// Ctrl+S: does what the visible "Salvar rascunho" does. True when a
    /// save was attempted.
    pub fn key_save(&mut self, store: &mut Store) -> bool {
        if self.tab == Tab::Blocks && self.block_dirty() {
            self.save_block(store);
            true
        } else if self.template_dirty() {
            self.save_template(store);
            true
        } else {
            // An instruction edited on another tab is saved there, in view.
            false
        }
    }

    /// Ctrl+N: what "Novo modelo" does. With unsaved changes it refuses with
    /// the usual message; otherwise the new template opens on the next frame.
    pub fn key_new(&mut self) -> bool {
        if self.dirty() {
            self.notice(
                "Salve ou descarte as alterações antes de trocar de modelo.",
                true,
            );
        } else {
            self.pending_new = true;
        }
        true
    }

    /// Esc: closes an open inline form or confirmation. True when it closed
    /// something.
    pub fn key_escape(&mut self) -> bool {
        let open = self.dup_name.is_some()
            || self.confirm.is_some()
            || self.confirm_discard_template
            || self.confirm_discard_block
            || self.confirm_discard_example
            || self.confirm_reextract.is_some();
        self.dup_name = None;
        self.confirm = None;
        self.confirm_discard_template = false;
        self.confirm_discard_block = false;
        self.confirm_discard_example = false;
        self.confirm_reextract = None;
        open
    }

    fn template_dirty(&self) -> bool {
        self.edit
            .as_ref()
            .is_some_and(|t| t.uid.is_empty() && self.base.is_empty() || json(t) != self.base)
    }

    fn block_dirty(&self) -> bool {
        self.block_edit.as_ref().is_some_and(|b| {
            b.uid.is_empty() && self.block_base.is_empty() || json(b) != self.block_base
        })
    }

    fn example_dirty(&self) -> bool {
        self.example_edit.as_ref().is_some_and(|e| {
            e.uid.is_empty() && self.example_base.is_empty() || json(e) != self.example_base
        })
    }

    /// Opens the panel on a new personal draft template seeded with `body`.
    pub fn start_from_text(&mut self, kind: &str, body: &str) {
        let kind = if KINDS.contains(&kind) {
            kind
        } else {
            "resposta"
        };
        let mut template = WorkTemplate::new("Novo modelo", kind, body);
        template.scope = Scope::Personal;
        self.install_template(template, true, None, false);
        self.tab = Tab::Structure;
        self.notice(
            "Modelo novo a partir do texto da minuta. Dê um nome e salve.",
            false,
        );
    }

    fn notice(&mut self, msg: impl Into<String>, error: bool) {
        self.message = msg.into();
        self.error = error;
    }

    /// Opens a template, switching away from whatever instruction or example
    /// was being edited.
    fn load_template(&mut self, store: &Store, template: WorkTemplate, unsaved: bool) {
        // Read again with its version: the list may be a few seconds old,
        // and old content stamped with the new version would overwrite a
        // colleague's change without the check noticing.
        let template = fresh_template(store, template);
        let seen = expected(store, "template", &template.uid);
        self.install_template(template, unsaved, seen, false);
    }

    /// Reloads the same template after a save, publish or discard. The
    /// instruction and example editors keep their unsaved work.
    fn reload_template(&mut self, store: &Store, template: WorkTemplate) {
        let template = fresh_template(store, template);
        let seen = expected(store, "template", &template.uid);
        self.install_template(template, false, seen, true);
    }

    fn install_template(
        &mut self,
        mut template: WorkTemplate,
        unsaved: bool,
        seen: Option<i64>,
        keep_editors: bool,
    ) {
        // The editor always works on a draft version. A published template
        // gets a working copy of its last version; it only becomes a stored
        // version when something actually changes (the Store skips a copy
        // identical to the published one), and its block references follow
        // the blocks' current published versions until it is published.
        if template.draft_version().is_none() {
            if let Some(mut last) = template.versions.last().cloned() {
                for reference in &mut last.block_refs {
                    reference.version = None;
                }
                template.versions.push(TemplateVersion {
                    version: last.version + 1,
                    published: false,
                    change_note: String::new(),
                    ..last
                });
            }
        }
        self.base = if unsaved {
            String::new()
        } else {
            json(&template)
        };
        self.edit = Some(template);
        self.template_version_seen = seen;
        self.confirm_discard_template = false;
        if !keep_editors {
            self.block_edit = None;
            self.block_base.clear();
            self.block_version_seen = None;
            self.example_edit = None;
            self.example_base.clear();
            self.example_version_seen = None;
            self.confirm_discard_block = false;
            self.confirm_discard_example = false;
            self.close_example_files();
            self.confirm = None;
        }
        self.dup_name = None;
        self.test_selected.clear();
        self.test_result = None;
    }

    fn load_block(&mut self, store: &Store, block: InstructionBlock, unsaved: bool) {
        let mut block = if block.uid.is_empty() {
            block
        } else {
            store
                .block_by_uid(&block.uid)
                .ok()
                .flatten()
                .unwrap_or(block)
        };
        self.block_version_seen = expected(store, "block", &block.uid);
        self.confirm_discard_block = false;
        if block.draft_version().is_none() {
            if let Some(last) = block.versions.last().cloned() {
                block.versions.push(crate::work_templates::BlockVersion {
                    version: last.version + 1,
                    published: false,
                    change_note: String::new(),
                    ..last
                });
            }
        }
        self.block_base = if unsaved { String::new() } else { json(&block) };
        self.block_edit = Some(block);
        self.confirm = None;
    }

    fn refresh(&mut self, store: &Store) {
        let old = self
            .loaded
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3));
        if old && !self.stale {
            return;
        }
        self.loaded = Some(Instant::now());
        self.stale = false;
        match store.list_templates() {
            Ok(list) => self.templates = list,
            Err(e) => self.notice(format!("Não foi possível carregar os modelos: {e:#}"), true),
        }
        match store.list_blocks() {
            Ok(list) => self.blocks = list,
            Err(e) => self.notice(
                format!("Não foi possível carregar as instruções: {e:#}"),
                true,
            ),
        }
    }

    fn stored(&self) -> Option<&WorkTemplate> {
        let uid = &self.edit.as_ref()?.uid;
        self.templates.iter().find(|t| &t.uid == uid)
    }

    // -- actions ------------------------------------------------------------

    fn save_template(&mut self, store: &mut Store) -> bool {
        let Some(template) = self.edit.clone() else {
            return false;
        };
        match store.save_template(&template, self.template_version_seen) {
            Ok(saved) => {
                self.reload_template(store, saved);
                self.stale = true;
                self.notice("Rascunho salvo.", false);
                true
            }
            Err(e) => {
                self.notice(format!("{e:#}"), true);
                false
            }
        }
    }

    fn publish_template(&mut self, store: &mut Store) {
        if !self.save_template(store) {
            return;
        }
        let Some(uid) = self.edit.as_ref().map(|t| t.uid.clone()) else {
            return;
        };
        match store.set_template_status(&uid, Status::Published, self.template_version_seen) {
            Ok(saved) => {
                let n = saved.current_version;
                self.reload_template(store, saved);
                self.stale = true;
                self.notice(format!("Versão {n} publicada."), false);
            }
            Err(e) => self.notice(format!("{e:#}"), true),
        }
    }

    fn set_status(&mut self, store: &mut Store, status: Status) {
        let Some(uid) = self.edit.as_ref().map(|t| t.uid.clone()) else {
            return;
        };
        match store.set_template_status(&uid, status, self.template_version_seen) {
            Ok(saved) => {
                self.reload_template(store, saved);
                self.stale = true;
                self.notice(
                    if status == Status::Archived {
                        "Modelo arquivado."
                    } else {
                        "Modelo desarquivado."
                    },
                    false,
                );
            }
            Err(e) => self.notice(format!("{e:#}"), true),
        }
    }

    fn save_block(&mut self, store: &mut Store) -> bool {
        let Some(block) = self.block_edit.clone() else {
            return false;
        };
        match store.save_block(&block, self.block_version_seen) {
            Ok(saved) => {
                self.load_block(store, saved, false);
                self.stale = true;
                self.notice("Rascunho da instrução salvo.", false);
                true
            }
            Err(e) => {
                self.notice(format!("{e:#}"), true);
                false
            }
        }
    }

    fn block_status(&mut self, store: &mut Store, status: Status, save_first: bool) {
        if save_first && !self.save_block(store) {
            return;
        }
        let Some(uid) = self.block_edit.as_ref().map(|b| b.uid.clone()) else {
            return;
        };
        match store.set_block_status(&uid, status, self.block_version_seen) {
            Ok(saved) => {
                self.load_block(store, saved, false);
                self.stale = true;
                self.notice("Instrução atualizada.", false);
            }
            Err(e) => self.notice(format!("{e:#}"), true),
        }
    }

    fn save_example(&mut self, store: &mut Store, status: Option<ExampleStatus>) {
        let Some(mut example) = self.example_edit.clone() else {
            return;
        };
        // Approving edited text: the text is saved first (the store sends an
        // edited approved example back to "não conferido"), then approved,
        // so the approval names who looked at this text.
        if status == Some(ExampleStatus::Approved) && self.example_dirty() {
            let mut edited = example.clone();
            edited.status = ExampleStatus::Unverified;
            match store.save_example(&edited, self.example_version_seen) {
                Ok(saved) => {
                    self.example_version_seen = expected(store, "example", &saved.uid);
                    example.uid = saved.uid;
                    example.id = saved.id;
                }
                Err(e) => {
                    self.notice(format!("{e:#}"), true);
                    return;
                }
            }
        }
        if let Some(status) = status {
            example.status = status;
        }
        match store.save_example(&example, self.example_version_seen) {
            Ok(saved) => {
                self.example_version_seen = expected(store, "example", &saved.uid);
                self.example_base = json(&saved);
                self.example_edit = Some(saved);
                self.stale = true;
                self.notice("Exemplo salvo.", false);
            }
            Err(e) => self.notice(format!("{e:#}"), true),
        }
    }

    // -- layout -------------------------------------------------------------

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        open_document: Option<(i64, String, Capture)>,
    ) -> TemplatesAction {
        self.refresh(store);
        if std::mem::take(&mut self.pending_new) {
            self.start_new(store);
        }
        let member = store
            .load_intranet_config()
            .map(|c| c.member_name)
            .unwrap_or_default();
        let mut action = TemplatesAction::None;
        let p = theme::c();

        egui::Panel::left("tpl_list")
            .exact_size(300.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(egui::Stroke::new(1.0, p.outline))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CARD))
                    .inner_margin(egui::Margin::same(0)),
            )
            .show(ui, |ui| {
                self.left_column(ui, store, &member);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 16,
                ..Default::default()
            }))
            .show(ui, |ui| {
                if let Some(a) = self.right_column(ui, store, &member, open_document.as_ref()) {
                    action = a;
                }
            });
        action
    }

    /// "Novo modelo": an empty personal template, unless there is unsaved work.
    fn start_new(&mut self, store: &Store) {
        if self.dirty() {
            self.notice(
                "Salve ou descarte as alterações antes de trocar de modelo.",
                true,
            );
        } else {
            let mut t = WorkTemplate::new("", "resposta", "");
            t.scope = Scope::Personal;
            self.load_template(store, t, false);
        }
    }

    fn left_column(&mut self, ui: &mut egui::Ui, store: &mut Store, member: &str) {
        egui::ScrollArea::vertical()
            .id_salt("tpl_left")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = theme::S2;
                        ui.horizontal(|ui| {
                            widgets::heading(ui, "Modelos");
                            crate::help_ui::help(ui,
                                "Modelos guardam estrutura, instruções, exemplos e referências. Os publicados \
                                 ficam disponíveis no editor de minutas; os compartilhados valem para toda a equipe.",
                            );
                        });
                        if ui
                            .add_sized(
                                [ui.available_width(), 34.0],
                                widgets::primary(format!("{} Novo modelo", icon::PLUS)),
                            )
                            .clicked()
                        {
                            self.start_new(store);
                        }
                        let width = ui.available_width();
                        let found = widgets::search(ui, &mut self.search, "Buscar modelo", width);
                        if std::mem::take(&mut self.focus_search) {
                            found.request_focus();
                        }
                        ui.add_space(theme::S1);
                        widgets::field_label(ui, "Tipo");
                        let current = if self.filter_kind.is_empty() {
                            "Todos"
                        } else {
                            crate::labels::draft_kind(&self.filter_kind)
                        };
                        egui::ComboBox::from_id_salt("tpl_kind_filter")
                            .selected_text(current)
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.filter_kind, String::new(), "Todos");
                                for k in KINDS {
                                    ui.selectable_value(
                                        &mut self.filter_kind,
                                        k.to_owned(),
                                        crate::labels::draft_kind(k),
                                    );
                                }
                            });
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(theme::S2, theme::S2);
                            if widgets::chip(ui, self.show_archived, "Mostrar arquivados").clicked()
                            {
                                self.show_archived = !self.show_archived;
                            }
                            if widgets::chip(ui, self.only_mine, "Só os meus").clicked() {
                                self.only_mine = !self.only_mine;
                            }
                        });
                    });
                // The line above the first row.
                let y = ui.cursor().top();
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    y,
                    egui::Stroke::new(1.0, theme::c().outline),
                );

                let mine = owner_key(member);
                let mut pick: Option<WorkTemplate> = None;
                let selected = self.edit.as_ref().map(|t| t.uid.clone());
                let mut shown = 0;
                let block_hits = self
                    .blocks
                    .iter()
                    .filter(|b| {
                        b.visible_to(member)
                            && (self.show_archived || b.status != Status::Archived)
                            && name_matches(&b.name, &self.search)
                    })
                    .count();
                for (i, t) in self.templates.iter().enumerate() {
                    if !name_matches(&t.name, &self.search)
                        || !t.visible_to(member)
                        || (!self.filter_kind.is_empty() && t.kind != self.filter_kind)
                        || (!self.show_archived && t.status == Status::Archived)
                        || (self.only_mine && owner_key(&t.owner) != mine)
                    {
                        continue;
                    }
                    shown += 1;
                    let is_sel = selected.as_deref() == Some(t.uid.as_str());
                    let response = pick_row(ui, ("tpl", i), is_sel, 16, |ui| {
                        row_name(ui, &t.name, is_sel);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(theme::S1, theme::S1);
                            widgets::pill(ui, short_scope(t.scope), Tone::Neutral);
                            widgets::pill(ui, t.status.label(), tone_of_status(t.status));
                            let version = version_text(
                                t.current_version,
                                t.draft_version().is_some() && t.current_version > 0,
                            );
                            if !version.is_empty() {
                                widgets::small_muted(ui, version);
                            }
                        });
                    })
                    .on_hover_text(t.scope.label());
                    if response.clicked() && !is_sel {
                        pick = Some(t.clone());
                    }
                }
                if shown == 0 {
                    let searching = !self.search.trim().is_empty();
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if searching && block_hits == 0 {
                                widgets::muted(
                                    ui,
                                    format!(
                                        "Nenhum modelo encontrado para \"{}\".",
                                        self.search.trim()
                                    ),
                                );
                                if ui.add(widgets::quiet("Limpar a busca")).clicked() {
                                    self.search.clear();
                                }
                            } else if !searching {
                                widgets::small_muted(ui, "Nenhum modelo para mostrar.");
                            }
                        });
                }
                if let Some(t) = pick {
                    if self.dirty() {
                        self.notice(
                            "Salve ou descarte as alterações antes de trocar de modelo.",
                            true,
                        );
                    } else {
                        self.load_template(store, t, false);
                    }
                }

                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 20,
                        bottom: 8,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = theme::S1;
                        ui.horizontal(|ui| {
                            widgets::overline(ui, "Instruções (biblioteca)", None);
                            crate::help_ui::help(ui,
                                "Instruções são orientações para a IA que valem para vários modelos, \
                                 escritas uma vez e reaproveitadas: \"Use linguagem simples\", \
                                 \"Cite a norma aplicável\", \"Não prometa prazos\". Crie e publique a \
                                 instrução aqui; depois, no modelo, aba Instruções, inclua-a como \
                                 obrigatória (sempre vai), opcional (você marca ao gerar) ou condicional \
                                 (vale só no caso descrito). Mudar a instrução não altera minutas já feitas.",
                            );
                        });
                        widgets::small_muted(
                            ui,
                            "Orientações reutilizáveis que os modelos podem incluir.",
                        );
                    });
                let y = ui.cursor().top();
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    y,
                    egui::Stroke::new(1.0, theme::c().outline),
                );
                self.block_list(ui, store, member);
            });
    }

    fn block_list(&mut self, ui: &mut egui::Ui, store: &Store, member: &str) {
        let mut pick: Option<InstructionBlock> = None;
        let selected = self.block_edit.as_ref().map(|b| b.uid.clone());
        for (i, b) in self.blocks.iter().enumerate() {
            if !name_matches(&b.name, &self.search)
                || !b.visible_to(member)
                || (!self.show_archived && b.status == Status::Archived)
            {
                continue;
            }
            let is_sel = selected.as_deref() == Some(b.uid.as_str());
            let response = pick_row(ui, ("blk", i), is_sel, 16, |ui| {
                row_name(ui, &b.name, is_sel);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(theme::S1, theme::S1);
                    widgets::pill(ui, short_scope(b.scope), Tone::Neutral);
                    widgets::pill(ui, status_label_f(b.status), tone_of_status(b.status));
                    let version = version_text(
                        b.current_version,
                        b.draft_version().is_some() && b.current_version > 0,
                    );
                    if !version.is_empty() {
                        widgets::small_muted(ui, version);
                    }
                });
            })
            .on_hover_text(scope_label_f(b.scope));
            if response.clicked() && !is_sel {
                pick = Some(b.clone());
            }
        }
        if let Some(b) = pick {
            if self.block_dirty() {
                self.notice(
                    "Salve ou descarte as alterações da instrução antes de trocar.",
                    true,
                );
            } else {
                self.load_block(store, b, false);
                self.tab = Tab::Blocks;
            }
        }
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(16, 8))
            .show(ui, |ui| {
                if ui
                    .add(widgets::quiet(format!("{} Nova instrução", icon::PLUS)))
                    .clicked()
                {
                    if self.block_dirty() {
                        self.notice(
                            "Salve ou descarte as alterações da instrução antes de trocar.",
                            true,
                        );
                    } else {
                        self.load_block(store, InstructionBlock::new("", "", ""), false);
                        self.tab = Tab::Blocks;
                    }
                }
            });
    }

    /// The panel's message, in a soft frame with a close button.
    fn message_line(&mut self, ui: &mut egui::Ui) {
        if self.message.is_empty() {
            return;
        }
        let tone = if self.error { Tone::Danger } else { Tone::Ok };
        let (fg, bg) = tone.colors();
        let mark = if self.error { icon::ERROR } else { icon::CHECK };
        let mut close = false;
        egui::Frame::new()
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::containers::Sides::new().shrink_left().wrap().show(
                    ui,
                    |ui| {
                        ui.label(RichText::new(mark).size(16.0).color(fg));
                        ui.add(egui::Label::new(RichText::new(&self.message).color(fg)).wrap());
                    },
                    |ui| {
                        if widgets::icon_button(ui, icon::CLOSE, "Fechar").clicked() {
                            close = true;
                        }
                    },
                );
            });
        ui.add_space(theme::S2);
        if close {
            self.message.clear();
        }
    }

    fn right_column(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        member: &str,
        doc: Option<&(i64, String, Capture)>,
    ) -> Option<TemplatesAction> {
        self.message_line(ui);
        if self.edit.is_none() {
            self.empty_state(ui, store);
            return None;
        }
        let mut action = None;
        self.header(ui, store, doc.is_some(), &mut action);
        ui.add_space(theme::S2);
        let block_dirty = self.block_dirty();
        let example_dirty = self.example_area_dirty();
        widgets::tab_row(ui, |ui| {
            for (tab, label, dirty) in [
                (Tab::Structure, "Estrutura", false),
                (Tab::Blocks, "Instruções", block_dirty),
                (Tab::Examples, "Exemplos", example_dirty),
                (Tab::References, "Referências", false),
                (Tab::Test, "Testar", false),
            ] {
                if widgets::tab(ui, self.tab == tab, label, dirty).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(theme::S3);
        egui::ScrollArea::vertical()
            .id_salt("tpl_right")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Room on both sides so card borders are not cut by the
                // scroll area and the scroll bar does not cover them.
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 2,
                        right: 14,
                        top: 2,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        // The forms follow the window; the room of the
                        // scroll bar stays free.
                        ui.set_max_width((ui.available_width() - 14.0).max(240.0));
                        ui.spacing_mut().item_spacing.y = theme::S3;
                        match self.tab {
                            Tab::Structure => self.tab_structure(ui),
                            Tab::Blocks => self.tab_blocks(ui, store, member),
                            Tab::Examples => self.tab_examples(ui, store),
                            Tab::References => self.tab_references(ui),
                            Tab::Test => self.tab_test(ui, store, doc),
                        }
                        ui.add_space(theme::S4);
                    });
            });
        action
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, store: &Store) {
        ui.add_space((ui.available_height() * 0.28).max(theme::S5));
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = theme::S2;
            ui.label(
                RichText::new(icon::TEMPLATES)
                    .size(44.0)
                    .color(theme::c().muted),
            );
            widgets::muted(ui, "Escolha um modelo na lista ou crie um novo.");
            ui.add_space(theme::S1);
            if ui
                .add(widgets::primary(format!("{} Novo modelo", icon::PLUS)))
                .clicked()
            {
                self.start_new(store);
            }
        });
    }

    /// Name, status, the actions on the template and the confirmations.
    fn header(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        has_doc: bool,
        action: &mut Option<TemplatesAction>,
    ) {
        let stored = self.stored().cloned();
        let dirty = self.template_dirty();
        let Some(edit) = self.edit.as_ref() else {
            return;
        };
        let name = edit.name.clone();
        let scope = edit.scope;
        let draft_number = edit.versions.last().map(|v| v.version).unwrap_or(1);
        let has_draft = stored.as_ref().is_some_and(|t| t.draft_version().is_some());
        let archived = stored
            .as_ref()
            .is_some_and(|t| t.status == Status::Archived);
        // A published template without a draft has nothing new to publish;
        // the store would refuse, so say it up front.
        let nothing_new = !dirty
            && stored
                .as_ref()
                .is_some_and(|t| t.published_version().is_some() && t.draft_version().is_none());
        let can_use = has_doc
            && stored
                .as_ref()
                .is_some_and(|t| t.status == Status::Published && t.published_version().is_some());

        let mut do_dup = false;
        let mut do_status: Option<Status> = None;
        let mut do_delete = false;
        egui::containers::Sides::new().shrink_left().wrap().show(
            ui,
            |ui| {
                widgets::title(
                    ui,
                    if name.trim().is_empty() {
                        "Novo modelo"
                    } else {
                        &name
                    },
                );
            },
            |ui| {
                let (button, _) =
                    egui::containers::menu::MenuButton::from_button(widgets::secondary(icon::MORE))
                        .ui(ui, |ui| {
                            ui.set_min_width(190.0);
                            if ui
                                .add_enabled(
                                    stored.is_some(),
                                    egui::Button::new(format!("{} Duplicar…", icon::COPY)),
                                )
                                .clicked()
                            {
                                do_dup = true;
                                ui.close();
                            }
                            if stored.is_some() {
                                let (label, wanted) = if archived {
                                    ("Desarquivar", Status::Published)
                                } else {
                                    ("Arquivar", Status::Archived)
                                };
                                if ui.button(label).clicked() {
                                    do_status = Some(wanted);
                                    ui.close();
                                }
                                ui.separator();
                                if ui.add(widgets::danger("Apagar de vez…")).clicked() {
                                    do_delete = true;
                                    ui.close();
                                }
                            }
                        });
                button.on_hover_text("Mais ações");
            },
        );
        ui.add_space(theme::S1);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(theme::S2, theme::S1);
            widgets::pill(ui, scope.label(), Tone::Neutral);
            if let Some(t) = &stored {
                widgets::pill(ui, t.status.label(), tone_of_status(t.status));
            }
            if has_draft {
                widgets::pill(ui, format!("Rascunho: versão {draft_number}"), Tone::Warn);
            }
            if dirty {
                widgets::pill(
                    ui,
                    format!("{} Alterações não salvas", icon::WARNING),
                    Tone::Warn,
                );
            }
        });
        if let Some(v) = stored.as_ref().and_then(|t| t.published_version()) {
            widgets::small_muted(
                ui,
                format!(
                    "Publicada: versão {} por {} em {}",
                    v.version,
                    v.author,
                    crate::labels::datetime(&v.created_at)
                ),
            );
        }
        ui.add_space(theme::S2);

        let mut do_save = false;
        let mut do_publish = false;
        let mut do_discard = false;
        let publish_primary = !dirty && !nothing_new;
        ui.horizontal_wrapped(|ui| {
            let save = if dirty {
                widgets::primary("Salvar rascunho")
            } else {
                widgets::secondary("Salvar rascunho")
            };
            do_save = ui.add(save).clicked();
            let publish_text = format!("Publicar versão {draft_number}");
            let publish = if publish_primary {
                widgets::primary(publish_text)
            } else {
                widgets::secondary(publish_text)
            };
            do_publish = ui
                .add_enabled(!nothing_new, publish)
                .on_disabled_hover_text(
                    "Não há alterações a publicar. Altere o modelo e salve o rascunho antes.",
                )
                .clicked();
            do_discard = ui.add(widgets::quiet("Descartar alterações")).clicked();
            if can_use
                && ui
                    .add(widgets::secondary("Usar este modelo na minuta"))
                    .clicked()
            {
                if let Some(t) = &stored {
                    *action = Some(TemplatesAction::UseTemplate { uid: t.uid.clone() });
                }
            }
        });

        if do_save {
            self.save_template(store);
        }
        if do_publish {
            self.publish_template(store);
        }
        if do_discard {
            if self.template_dirty() {
                self.confirm_discard_template = true;
            } else {
                self.discard_template(store);
            }
        }
        if do_dup {
            self.dup_name = stored.as_ref().map(|t| format!("Cópia de {}", t.name));
        }
        if let Some(status) = do_status {
            if self.dirty() {
                self.notice(
                    "Salve ou descarte as alterações antes de arquivar ou desarquivar.",
                    true,
                );
            } else {
                self.set_status(store, status);
            }
        }
        if do_delete {
            self.confirm = Some(Confirm::Template);
        }

        if self.confirm_discard_template {
            ui.add_space(theme::S2);
            match confirm_line(
                ui,
                "Descartar todas as alterações não salvas do modelo?",
                "Descartar",
                "Cancelar",
            ) {
                Some(true) => self.discard_template(store),
                Some(false) => self.confirm_discard_template = false,
                None => {}
            }
        }
        if self.dup_name.is_some() {
            ui.add_space(theme::S2);
            let mut create = false;
            let mut cancel = false;
            let mut copy_name = String::new();
            if let Some(name) = self.dup_name.as_mut() {
                widgets::field_label(ui, "Nome da cópia");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text("Nome da cópia")
                            .desired_width(360.0),
                    );
                    create = ui.add(widgets::secondary("Criar cópia")).clicked();
                    cancel = ui.add(widgets::quiet("Cancelar")).clicked();
                });
                copy_name = name.clone();
            }
            if create {
                if self.dirty() {
                    self.notice(
                        "Salve ou descarte as alterações antes de trocar de modelo.",
                        true,
                    );
                } else {
                    let uid = self
                        .edit
                        .as_ref()
                        .map(|t| t.uid.clone())
                        .unwrap_or_default();
                    match store.duplicate_template(&uid, copy_name.trim()) {
                        Ok(copy) => {
                            self.load_template(store, copy, false);
                            self.stale = true;
                            self.notice("Cópia criada como rascunho pessoal.", false);
                        }
                        Err(e) => self.notice(format!("{e:#}"), true),
                    }
                }
                self.dup_name = None;
            }
            if cancel {
                self.dup_name = None;
            }
        }
        if self.confirm == Some(Confirm::Template) {
            ui.add_space(theme::S2);
            match confirm_line(
                ui,
                "Apagar este modelo de vez, com exemplos e versões? Não há como desfazer.",
                "Apagar de vez",
                "Cancelar",
            ) {
                Some(true) => {
                    let uid = self
                        .edit
                        .as_ref()
                        .map(|t| t.uid.clone())
                        .unwrap_or_default();
                    match store.delete_template(&uid) {
                        Ok(()) => {
                            self.edit = None;
                            self.base.clear();
                            self.example_edit = None;
                            self.close_example_files();
                            self.stale = true;
                            self.notice("Modelo apagado.", false);
                        }
                        Err(e) => self.notice(format!("{e:#}"), true),
                    }
                    self.confirm = None;
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
    }

    // -- tabs ---------------------------------------------------------------

    fn tab_structure(&mut self, ui: &mut egui::Ui) {
        let Some(t) = self.edit.as_mut() else { return };
        let Some(v) = t.versions.last_mut() else {
            return;
        };
        widgets::card(ui, |ui| {
            widgets::section(ui, "Dados do modelo");
            ui.add_space(theme::S1);
            text_field(ui, "Nome", &mut t.name);
            text_field(ui, "Finalidade", &mut t.purpose);
            widgets::field_label(ui, "Tipo de minuta");
            egui::ComboBox::from_id_salt("tpl_kind")
                .selected_text(crate::labels::draft_kind(&t.kind))
                .show_ui(ui, |ui| {
                    for k in KINDS {
                        ui.selectable_value(
                            &mut t.kind,
                            k.to_owned(),
                            crate::labels::draft_kind(k),
                        );
                    }
                });
            ui.add_space(theme::S2);
            text_area(
                ui,
                "Critérios de uso (quando usar este modelo)",
                &mut t.usage_criteria,
                3,
            );
            widgets::field_label(ui, "Quem vê");
            ui.horizontal(|ui| {
                ui.radio_value(&mut t.scope, Scope::Personal, "Pessoal");
                ui.radio_value(&mut t.scope, Scope::Shared, "Compartilhado com a equipe");
            });
        });

        widgets::card(ui, |ui| {
            widgets::section(ui, "Seções");
            ui.add_space(theme::S1);
            let mut pending: Option<(usize, Op)> = None;
            let len = v.sections.len();
            for (i, s) in v.sections.iter_mut().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                ui.push_id(("section", i), |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut s.title)
                                .hint_text("Título da seção")
                                .desired_width(
                                    ui.available_width()
                                        - OPS_WIDTH
                                        - CHECK_WIDTH
                                        - 2.0 * theme::S2,
                                ),
                        );
                        ui.checkbox(&mut s.required, "Obrigatória");
                        if let Some(op) = list_ops(ui, i, len) {
                            pending = Some((i, op));
                        }
                    });
                    ui.add(
                        egui::TextEdit::multiline(&mut s.guidance)
                            .hint_text("O que a seção deve conter, ou o texto-base")
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                });
            }
            if let Some((i, op)) = pending {
                apply_op(&mut v.sections, i, op);
            }
            ui.add_space(theme::S1);
            if ui
                .add(widgets::secondary(format!(
                    "{} Adicionar seção",
                    icon::PLUS
                )))
                .clicked()
            {
                v.sections.push(Section::default());
            }
        });

        widgets::card(ui, |ui| {
            widgets::section(ui, "Variáveis");
            widgets::small_muted(
                ui,
                "Trechos como {{nome}} nas seções são preenchidos pela pessoa.",
            );
            ui.add_space(theme::S1);
            let mut remove: Option<usize> = None;
            for (i, var) in v.variables.iter_mut().enumerate() {
                ui.push_id(("var", i), |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut var.name)
                                .hint_text("nome")
                                .desired_width(150.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut var.label)
                                .hint_text("como aparece")
                                .desired_width(
                                    ui.available_width() - CHECK_WIDTH - 30.0 - 2.0 * theme::S2,
                                ),
                        );
                        ui.checkbox(&mut var.required, "Obrigatória");
                        if widgets::icon_button(ui, icon::TRASH, "Remover").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                v.variables.remove(i);
            }
            ui.add_space(theme::S1);
            ui.horizontal(|ui| {
                if ui
                    .add(widgets::secondary(format!(
                        "{} Adicionar variável",
                        icon::PLUS
                    )))
                    .clicked()
                {
                    v.variables.push(Variable::default());
                }
                if ui.add(widgets::quiet("Detectar variáveis")).clicked() {
                    let all: String = v
                        .sections
                        .iter()
                        .map(|s| format!("{}\n{}\n", s.title, s.guidance))
                        .collect();
                    for found in detect_variables(&all) {
                        if !v.variables.iter().any(|x| x.name == found.name) {
                            v.variables.push(found);
                        }
                    }
                }
            });
        });

        widgets::card(ui, |ui| {
            widgets::section(ui, "Critérios de conferência");
            widgets::small_muted(ui, "O que conferir antes de concluir a minuta.");
            ui.add_space(theme::S1);
            string_list(
                ui,
                "criteria",
                &mut v.criteria,
                "ex.: confirmar o prazo informado",
                crate::work_templates::MAX_CRITERIA,
            );
        });

        widgets::card(ui, |ui| {
            text_field(ui, "Nota desta versão", &mut v.change_note);
        });
    }

    fn tab_references(&mut self, ui: &mut egui::Ui) {
        let Some(v) = self.edit.as_mut().and_then(|t| t.versions.last_mut()) else {
            return;
        };
        widgets::card(ui, |ui| {
            widgets::section(ui, "Referências");
            widgets::small_muted(
                ui,
                "Referências são indicadas à pessoa; o Ponte não as consulta sozinho.",
            );
            ui.add_space(theme::S1);
            let mut pending: Option<(usize, Op)> = None;
            let len = v.references.len();
            for (i, r) in v.references.iter_mut().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                ui.push_id(("ref", i), |ui| {
                    ui.horizontal(|ui| {
                        let room = ui.available_width() - OPS_WIDTH - 2.0 * theme::S2;
                        ui.add(
                            egui::TextEdit::singleline(&mut r.label)
                                .hint_text("Nome")
                                .desired_width(room * 0.4),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut r.locator)
                                .hint_text("Onde encontrar (número, endereço, pasta)")
                                .desired_width(room * 0.6),
                        );
                        if let Some(op) = list_ops(ui, i, len) {
                            pending = Some((i, op));
                        }
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut r.note)
                            .hint_text("Observação")
                            .desired_width(f32::INFINITY),
                    );
                });
            }
            if let Some((i, op)) = pending {
                apply_op(&mut v.references, i, op);
            }
            if v.references.len() < crate::work_templates::MAX_REFERENCES {
                ui.add_space(theme::S1);
                if ui
                    .add(widgets::secondary(format!(
                        "{} Adicionar referência",
                        icon::PLUS
                    )))
                    .clicked()
                {
                    v.references.push(Reference::default());
                }
            }
        });
    }

    fn tab_blocks(&mut self, ui: &mut egui::Ui, store: &mut Store, member: &str) {
        // The instruction being edited comes first: it was just picked from
        // the library on the left.
        if self.block_edit.is_some() {
            self.block_editor(ui, store);
            self.block_refs(ui, member);
        } else {
            self.block_refs(ui, member);
            self.block_editor(ui, store);
        }
    }

    fn block_editor(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        let dirty = self.block_dirty();
        let confirm_discard = self.confirm_discard_block;
        let confirm_delete = self.confirm == Some(Confirm::Block);
        let mut do_save = false;
        let mut do_publish = false;
        let mut do_status: Option<Status> = None;
        let mut do_delete = false;
        let mut do_discard = false;
        let mut discard_answer: Option<bool> = None;
        let mut delete_answer: Option<bool> = None;
        let mut has_block = true;
        widgets::card(ui, |ui| {
            let Some(b) = self.block_edit.as_mut() else {
                has_block = false;
                widgets::section(ui, "Instrução");
                widgets::muted(
                    ui,
                    "Escolha uma instrução na lista ao lado ou crie uma nova.",
                );
                return;
            };
            let stored_status = if b.uid.is_empty() {
                None
            } else {
                Some(b.status)
            };
            let draft_number = b.versions.last().map(|v| v.version).unwrap_or(1);
            ui.horizontal(|ui| {
                widgets::section(ui, "Instrução");
                if dirty {
                    widgets::pill(
                        ui,
                        format!("{} Alterações não salvas", icon::WARNING),
                        Tone::Warn,
                    );
                }
            });
            ui.add_space(theme::S1);
            text_field(ui, "Nome", &mut b.name);
            text_field(ui, "Tema", &mut b.topic);
            widgets::field_label(ui, "Quem vê");
            ui.horizontal(|ui| {
                ui.radio_value(&mut b.scope, Scope::Personal, "Pessoal");
                ui.radio_value(&mut b.scope, Scope::Shared, "Compartilhada");
            });
            ui.add_space(theme::S2);
            if let Some(v) = b.versions.last_mut() {
                text_area(ui, "Texto da instrução", &mut v.text, 8);
                text_field(ui, "Nota desta versão", &mut v.change_note);
            }
            egui::containers::Sides::new().shrink_left().wrap().show(
                ui,
                |ui| {
                    do_save = ui
                        .add(widgets::secondary("Salvar rascunho da instrução"))
                        .clicked();
                    do_publish = ui
                        .add(widgets::secondary(format!(
                            "Publicar versão {draft_number}"
                        )))
                        .clicked();
                    do_discard = ui.add(widgets::quiet("Descartar")).clicked();
                    match stored_status {
                        Some(Status::Archived) => {
                            do_status = ui
                                .add(widgets::quiet("Desarquivar"))
                                .clicked()
                                .then_some(Status::Published)
                        }
                        Some(_) => {
                            do_status = ui
                                .add(widgets::quiet("Arquivar"))
                                .clicked()
                                .then_some(Status::Archived)
                        }
                        None => {}
                    }
                },
                |ui| {
                    if stored_status.is_some()
                        && ui.add(widgets::danger("Apagar de vez…")).clicked()
                    {
                        do_delete = true;
                    }
                },
            );
            if confirm_discard {
                ui.add_space(theme::S2);
                discard_answer = confirm_line(
                    ui,
                    "Descartar as alterações não salvas desta instrução?",
                    "Descartar",
                    "Cancelar",
                );
            }
            if confirm_delete {
                ui.add_space(theme::S2);
                delete_answer = confirm_line(
                    ui,
                    "Apagar esta instrução de vez? Não há como desfazer.",
                    "Apagar de vez",
                    "Cancelar",
                );
            }
        });
        if !has_block {
            return;
        }
        if do_status.is_some() && self.dirty() {
            do_status = None;
            self.notice(
                "Salve ou descarte as alterações antes de arquivar ou desarquivar.",
                true,
            );
        }
        if do_save {
            self.save_block(store);
        }
        if do_publish {
            self.block_status(store, Status::Published, true);
        }
        if let Some(s) = do_status {
            self.block_status(store, s, false);
        }
        if do_delete {
            self.confirm = Some(Confirm::Block);
        }
        if do_discard {
            if dirty {
                self.confirm_discard_block = true;
            } else {
                self.discard_block(store);
            }
        }
        match discard_answer {
            Some(true) => self.discard_block(store),
            Some(false) => self.confirm_discard_block = false,
            None => {}
        }
        match delete_answer {
            Some(true) => {
                let uid = self
                    .block_edit
                    .as_ref()
                    .map(|b| b.uid.clone())
                    .unwrap_or_default();
                match store.delete_block(&uid) {
                    Ok(()) => {
                        self.block_edit = None;
                        self.block_base.clear();
                        self.stale = true;
                        self.notice("Instrução apagada.", false);
                    }
                    Err(e) => self.notice(format!("{e:#}"), true),
                }
                self.confirm = None;
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }

    fn block_refs(&mut self, ui: &mut egui::Ui, member: &str) {
        let blocks = &self.blocks;
        let Some(v) = self.edit.as_mut().and_then(|t| t.versions.last_mut()) else {
            return;
        };
        widgets::card(ui, |ui| {
            widgets::section(ui, "Instruções deste modelo");
            widgets::small_muted(
                ui,
                "As alterações valem no rascunho do modelo; use \"Salvar rascunho\" no alto, \
                 não o botão da instrução.",
            );
            ui.add_space(theme::S1);
            let mut pending: Option<(usize, Op)> = None;
            let len = v.block_refs.len();
            for (i, r) in v.block_refs.iter_mut().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                let name = blocks
                    .iter()
                    .find(|b| b.uid == r.block_uid)
                    .map(|b| b.name.as_str())
                    .unwrap_or("(instrução removida)");
                ui.push_id(("bref", i), |ui| {
                    let version = match r.version {
                        Some(n) => format!("v{n}"),
                        None => "versão publicada atual".to_owned(),
                    };
                    egui::containers::Sides::new()
                        .shrink_left()
                        .truncate()
                        .show(
                            ui,
                            |ui| {
                                ui.label(
                                    RichText::new(name).font(theme::font_strong(theme::SIZE_BODY)),
                                );
                                widgets::small_muted(ui, version);
                            },
                            |ui| {
                                if let Some(op) = list_ops(ui, i, len) {
                                    pending = Some((i, op));
                                }
                            },
                        );
                    let mut kind = match r.mode {
                        BlockMode::Mandatory => 0,
                        BlockMode::Optional => 1,
                        BlockMode::Conditional { .. } => 2,
                    };
                    let before = kind;
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("bmode", i))
                            .selected_text(r.mode.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut kind, 0, "Obrigatória");
                                ui.selectable_value(&mut kind, 1, "Opcional");
                                ui.selectable_value(&mut kind, 2, "Condicional");
                            });
                        if kind != before {
                            r.mode = match kind {
                                0 => BlockMode::Mandatory,
                                1 => BlockMode::Optional,
                                _ => BlockMode::Conditional {
                                    condition: String::new(),
                                },
                            };
                        }
                        if let BlockMode::Conditional { condition } = &mut r.mode {
                            ui.add(
                                egui::TextEdit::singleline(condition)
                                    .hint_text("Condição (quando usar)")
                                    .desired_width(f32::INFINITY),
                            );
                        }
                    });
                });
            }
            if let Some((i, op)) = pending {
                apply_op(&mut v.block_refs, i, op);
            }
            let available: Vec<&InstructionBlock> = blocks
                .iter()
                .filter(|b| {
                    b.status == Status::Published
                        && b.published_version().is_some()
                        && b.visible_to(member)
                        && !v.block_refs.iter().any(|r| r.block_uid == b.uid)
                })
                .collect();
            ui.add_space(theme::S1);
            if v.block_refs.len() >= MAX_BLOCKS_PER_TEMPLATE {
                widgets::small_muted(
                    ui,
                    format!("Máximo de {MAX_BLOCKS_PER_TEMPLATE} instruções por modelo."),
                );
            } else {
                let mut add: Option<String> = None;
                egui::ComboBox::from_id_salt("bref_add")
                    .selected_text(format!("{} Adicionar instrução ao modelo", icon::PLUS))
                    .show_ui(ui, |ui| {
                        for b in &available {
                            if ui.selectable_label(false, &b.name).clicked() {
                                add = Some(b.uid.clone());
                            }
                        }
                        if available.is_empty() {
                            ui.label("Nenhuma instrução publicada disponível.");
                        }
                    });
                if let Some(uid) = add {
                    v.block_refs.push(BlockRef {
                        block_uid: uid,
                        version: None,
                        mode: BlockMode::Mandatory,
                    });
                }
            }
        });
    }

    fn tab_examples(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        let Some(uid) = self.stored().map(|t| t.uid.clone()) else {
            widgets::card(ui, |ui| {
                widgets::muted(ui, "Salve o rascunho do modelo para cadastrar exemplos.");
            });
            return;
        };
        let examples = match store.examples_of(&uid) {
            Ok(list) => list,
            Err(e) => {
                widgets::card(ui, |ui| {
                    let (fg, bg) = Tone::Danger.colors();
                    widgets::pill_colored(ui, format!("{e:#}"), fg, bg);
                });
                return;
            }
        };
        let mut pick: Option<Example> = None;
        let mut new_example = false;
        let selected = self.example_edit.as_ref().map(|e| e.uid.clone());
        widgets::card(ui, |ui| {
            widgets::section(ui, "Exemplos do modelo");
            widgets::small_muted(
                ui,
                "Só exemplos aprovados são mostrados à IA; os fatos deles nunca valem para o caso.",
            );
            ui.add_space(theme::S1);
            if examples.is_empty() {
                widgets::muted(ui, "Nenhum exemplo cadastrado.");
            }
            for (i, e) in examples.iter().enumerate() {
                let is_sel = selected.as_deref() == Some(e.uid.as_str());
                let response = pick_row(ui, ("ex", i), is_sel, 8, |ui| {
                    egui::containers::Sides::new()
                        .shrink_left()
                        .truncate()
                        .show(
                            ui,
                            |ui| row_name(ui, &e.title, is_sel),
                            |ui| pill_for_example(ui, e.status),
                        );
                });
                if response.clicked() && !is_sel {
                    pick = Some(e.clone());
                }
            }
            ui.add_space(theme::S2);
            if ui
                .add(widgets::secondary(format!("{} Novo exemplo", icon::PLUS)))
                .clicked()
            {
                new_example = true;
            }
        });
        if let Some(e) = pick {
            if self.example_area_dirty() {
                self.notice(
                    "Salve ou descarte as alterações do exemplo (ou dos seus arquivos) antes de trocar.",
                    true,
                );
            } else {
                self.close_example_files();
                self.example_version_seen = expected(store, "example", &e.uid);
                self.example_base = json(&e);
                self.example_edit = Some(e);
                self.confirm_discard_example = false;
                self.confirm = None;
            }
        }
        if new_example {
            if self.example_area_dirty() {
                self.notice(
                    "Salve ou descarte as alterações do exemplo (ou dos seus arquivos) antes de trocar.",
                    true,
                );
            } else {
                self.close_example_files();
                let fresh = Example {
                    template_uid: uid.clone(),
                    ..Default::default()
                };
                // Same as "Nova instrução": the empty form is the baseline, so
                // it only counts as changed once something is typed.
                self.example_base = json(&fresh);
                self.example_edit = Some(fresh);
                self.example_version_seen = None;
                self.confirm_discard_example = false;
            }
        }
        let dirty = self.example_dirty();
        if self.example_edit.is_none() {
            return;
        }
        let confirm_discard = self.confirm_discard_example;
        let confirm_delete = self.confirm == Some(Confirm::Example);
        let (mut save, mut approve, mut reject, mut delete) = (false, false, false, false);
        let mut discard = false;
        let mut discard_answer: Option<bool> = None;
        let mut delete_answer: Option<bool> = None;
        widgets::card(ui, |ui| {
            let Some(e) = self.example_edit.as_mut() else {
                return;
            };
            let is_new = e.uid.is_empty();
            ui.horizontal(|ui| {
                widgets::section(ui, "Exemplo");
                if dirty {
                    widgets::pill(
                        ui,
                        format!("{} Alterações não salvas", icon::WARNING),
                        Tone::Warn,
                    );
                }
            });
            ui.add_space(theme::S1);
            text_field(ui, "Título", &mut e.title);
            text_area(ui, "Demanda", &mut e.demand, 4);
            text_area(ui, "Resposta revisada", &mut e.reply, 8);
            text_area(ui, "Explicação das escolhas", &mut e.explanation, 3);
            widgets::field_label(ui, "Comentários");
            string_list(
                ui,
                "ex_comments",
                &mut e.comments,
                "ex.: aproveitar a organização das seções; não reutilizar fatos nem fundamentação",
                10,
            );
            ui.add_space(theme::S2);
            widgets::field_label(ui, "Origem");
            ui.add(
                egui::TextEdit::singleline(&mut e.origin)
                    .hint_text("ex.: ofício de 2024, anonimizado")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(theme::S2);
            egui::containers::Sides::new().shrink_left().wrap().show(
                ui,
                |ui| {
                    save = ui.add(widgets::secondary("Salvar exemplo")).clicked();
                    approve = ui.add(widgets::secondary("Aprovar")).clicked();
                    reject = ui.add(widgets::quiet("Rejeitar")).clicked();
                    discard = ui.add(widgets::quiet("Descartar")).clicked();
                },
                |ui| {
                    if !is_new {
                        delete = ui.add(widgets::danger("Apagar de vez…")).clicked();
                    }
                },
            );
            if confirm_discard {
                ui.add_space(theme::S2);
                discard_answer = confirm_line(
                    ui,
                    "Descartar as alterações não salvas deste exemplo?",
                    "Descartar",
                    "Cancelar",
                );
            }
            if confirm_delete {
                ui.add_space(theme::S2);
                delete_answer = confirm_line(
                    ui,
                    "Apagar este exemplo de vez? Não há como desfazer.",
                    "Apagar de vez",
                    "Cancelar",
                );
            }
        });
        if save {
            self.save_example(store, None);
        } else if approve {
            self.save_example(store, Some(ExampleStatus::Approved));
        } else if reject {
            self.save_example(store, Some(ExampleStatus::Rejected));
        }
        if discard {
            if dirty {
                self.confirm_discard_example = true;
            } else {
                self.discard_example(store);
            }
        }
        match discard_answer {
            Some(true) => self.discard_example(store),
            Some(false) => self.confirm_discard_example = false,
            None => {}
        }
        if delete {
            self.confirm = Some(Confirm::Example);
        }
        match delete_answer {
            Some(true) => {
                let uid = self
                    .example_edit
                    .as_ref()
                    .map(|e| e.uid.clone())
                    .unwrap_or_default();
                match store.delete_example(&uid) {
                    Ok(()) => {
                        self.example_edit = None;
                        self.example_base.clear();
                        self.close_example_files();
                        self.stale = true;
                        self.notice("Exemplo apagado.", false);
                    }
                    Err(e) => self.notice(format!("{e:#}"), true),
                }
                self.confirm = None;
            }
            Some(false) => self.confirm = None,
            None => {}
        }
        let example_id = self
            .example_edit
            .as_ref()
            .filter(|e| !e.uid.is_empty())
            .map(|e| e.id)
            .filter(|id| *id > 0);
        if let Some(example_id) = example_id {
            widgets::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.example_files_section(ui, store, example_id);
            });
        }
    }

    fn discard_template(&mut self, store: &Store) {
        self.confirm_discard_template = false;
        match self.stored().cloned() {
            Some(t) => self.reload_template(store, t),
            None => {
                self.edit = None;
                self.base.clear();
            }
        }
        self.notice("Alterações descartadas.", false);
    }

    /// Goes back to the stored instruction; a new, never-saved one is closed.
    fn discard_block(&mut self, store: &Store) {
        self.confirm_discard_block = false;
        let uid = self
            .block_edit
            .as_ref()
            .map(|b| b.uid.clone())
            .unwrap_or_default();
        match self.blocks.iter().find(|b| b.uid == uid).cloned() {
            Some(b) if !uid.is_empty() => self.load_block(store, b, false),
            _ => {
                self.block_edit = None;
                self.block_base.clear();
                self.block_version_seen = None;
            }
        }
        self.notice("Alterações da instrução descartadas.", false);
    }

    /// Goes back to the stored example; a new, never-saved one is closed.
    fn discard_example(&mut self, store: &Store) {
        self.confirm_discard_example = false;
        let uid = self
            .example_edit
            .as_ref()
            .map(|e| e.uid.clone())
            .unwrap_or_default();
        let stored = if uid.is_empty() {
            None
        } else {
            self.stored()
                .and_then(|t| store.examples_of(&t.uid).ok())
                .and_then(|list| list.into_iter().find(|e| e.uid == uid))
        };
        match stored {
            Some(e) => {
                self.example_version_seen = expected(store, "example", &e.uid);
                self.example_base = json(&e);
                self.example_edit = Some(e);
            }
            None => {
                self.example_edit = None;
                self.example_base.clear();
                self.example_version_seen = None;
                self.close_example_files();
            }
        }
        self.notice("Alterações do exemplo descartadas.", false);
    }

    /// Edits in the example's own fields or in its files (descriptions, a
    /// text being reviewed).
    fn example_area_dirty(&self) -> bool {
        self.example_dirty() || self.attachments.dirty() || self.file_review.is_some()
    }

    /// Forgets the file state of the example that was open.
    fn close_example_files(&mut self) {
        self.attachments.forget();
        self.file_review = None;
        self.confirm_reextract = None;
        self.file_texts = None;
    }

    /// A PDF of an example the person asked to read in the Ponte.
    pub fn take_pdf_request(&mut self) -> Option<(i64, String)> {
        self.pdf_request.take()
    }

    fn example_files_section(&mut self, ui: &mut egui::Ui, store: &mut Store, example_id: i64) {
        widgets::section(ui, "Arquivos do exemplo");
        widgets::small_muted(
            ui,
            "Os arquivos mostram à IA a forma de uma resposta real. Só o texto extraído ou conferido de um exemplo aprovado é enviado; os fatos dos arquivos nunca valem para o caso. A extração funciona com PDF com texto, .txt e .docx. PDF digitalizado (só imagem) não é lido nesta versão.",
        );
        ui.add_space(theme::S2);
        self.attachments.show(
            ui,
            store,
            crate::attachments::Owner::Example(example_id),
            false,
        );
        if let Some(request) = self.attachments.take_pdf_request() {
            self.pdf_request = Some(request);
        }
        let metas = match store.list_attachments(crate::attachments::Owner::Example(example_id)) {
            Ok(m) => m,
            Err(e) => {
                let (fg, bg) = Tone::Danger.colors();
                widgets::pill_colored(ui, format!("{e:#}"), fg, bg);
                return;
            }
        };
        // The text records are unsealed once per change, not every frame.
        let ids: Vec<i64> = metas.iter().map(|m| m.id).collect();
        let cached = match &self.file_texts {
            Some((cached_example, cached_ids, _)) => {
                *cached_example == example_id && *cached_ids == ids
            }
            None => false,
        };
        if !cached {
            let records = ids
                .iter()
                .map(|id| store.attachment_text(*id).ok().flatten())
                .collect();
            self.file_texts = Some((example_id, ids.clone(), records));
        }
        let records: Vec<Option<crate::extraction::AttachmentText>> = self
            .file_texts
            .as_ref()
            .map(|(_, _, records)| records.clone())
            .unwrap_or_default();
        let mut extract: Option<(i64, String)> = None;
        let mut open_review: Option<(i64, String)> = None;
        for (meta, record) in metas.iter().zip(records.iter()) {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&meta.name).font(theme::font_strong(theme::SIZE_BODY)));
                match &record {
                    None => {
                        widgets::pill(ui, "Texto ainda não extraído", Tone::Warn);
                    }
                    Some(t) if !t.current_for(&meta.sha256) => {
                        widgets::pill(ui, "Texto desatualizado (extraia de novo)", Tone::Warn);
                    }
                    Some(t) => {
                        use crate::extraction::TextStatus;
                        let warn = matches!(t.status, TextStatus::Empty | TextStatus::Failed);
                        widgets::pill(
                            ui,
                            t.status.label(),
                            if warn { Tone::Warn } else { Tone::Neutral },
                        );
                        if t.status == TextStatus::Reviewed && !t.reviewed_by.is_empty() {
                            widgets::small_muted(ui, format!("por {}", t.reviewed_by));
                        }
                        if !t.note.is_empty() {
                            widgets::small_muted(ui, &t.note);
                        }
                    }
                }
                let reviewed = record
                    .as_ref()
                    .is_some_and(|t| t.status == crate::extraction::TextStatus::Reviewed);
                if reviewed {
                    // A person's correction is not thrown away by one click.
                    if self.confirm_reextract == Some(meta.id) {
                        widgets::small_muted(ui, "Extrair de novo substitui o texto já conferido.");
                        if ui.add(widgets::danger("Substituir")).clicked() {
                            extract = Some((meta.id, meta.name.clone()));
                        }
                        if ui.add(widgets::secondary("Cancelar")).clicked() {
                            self.confirm_reextract = None;
                        }
                    } else if ui.add(widgets::quiet("Extrair de novo…")).clicked() {
                        self.confirm_reextract = Some(meta.id);
                    }
                } else if ui.add(widgets::secondary("Extrair texto")).clicked() {
                    extract = Some((meta.id, meta.name.clone()));
                }
                if let Some(t) = record {
                    if (t.status.usable() || t.status == crate::extraction::TextStatus::Empty)
                        && ui.add(widgets::quiet("Conferir texto…")).clicked()
                    {
                        open_review = Some((meta.id, t.text.clone()));
                    }
                }
            });
        }
        if let Some((id, name)) = extract {
            self.confirm_reextract = None;
            self.file_texts = None;
            match store.extract_attachment_text(id) {
                Ok(t) => self.notice(
                    format!("Texto extraído de “{name}”: {}", t.status.label()),
                    false,
                ),
                Err(e) => self.notice(format!("{e:#}"), true),
            }
        }
        if let Some(r) = open_review {
            // One review at a time: opening another would drop the text being
            // corrected.
            if self.file_review.is_some() {
                self.notice(
                    "Conclua ou cancele a conferência do texto aberto antes de abrir outro.",
                    true,
                );
            } else {
                self.file_review = Some(r);
            }
        }
        let mut done = false;
        let mut review: Option<(i64, String)> = None;
        if let Some((id, text)) = self.file_review.as_mut() {
            if metas.iter().any(|m| m.id == *id) {
                ui.add_space(theme::S2);
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(text)
                                .desired_rows(8)
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add(widgets::secondary("Marcar como conferido"))
                        .clicked()
                    {
                        review = Some((*id, text.clone()));
                    }
                    if ui.add(widgets::quiet("Cancelar")).clicked() {
                        done = true;
                    }
                });
            } else {
                done = true;
            }
        }
        if let Some((id, text)) = review {
            match store.review_attachment_text(id, &text) {
                Ok(_) => {
                    self.notice("Texto marcado como conferido.", false);
                    self.file_texts = None;
                    done = true;
                }
                Err(e) => self.notice(format!("{e:#}"), true),
            }
        }
        if done {
            self.file_review = None;
        }
    }

    fn tab_test(&mut self, ui: &mut egui::Ui, store: &Store, doc: Option<&(i64, String, Capture)>) {
        let Some(doc) = doc else {
            widgets::card(ui, |ui| {
                widgets::section(ui, "Testar o modelo");
                widgets::muted(
                    ui,
                    "Nenhum documento aberto. O teste usa o documento selecionado na área Documentos.",
                );
            });
            return;
        };
        let optional: Vec<(String, String)> = self
            .edit
            .as_ref()
            .and_then(|t| t.versions.last())
            .map(|v| {
                v.block_refs
                    .iter()
                    .filter(|r| r.mode != BlockMode::Mandatory)
                    .map(|r| {
                        let name = self
                            .blocks
                            .iter()
                            .find(|b| b.uid == r.block_uid)
                            .map(|b| b.name.clone())
                            .unwrap_or_else(|| r.block_uid.clone());
                        (r.block_uid.clone(), name)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut build = false;
        widgets::card(ui, |ui| {
            widgets::section(ui, "Testar o modelo");
            let title = doc.2.title.trim();
            widgets::muted(
                ui,
                if title.is_empty() {
                    format!("Documento: {}", doc.1)
                } else {
                    format!("Documento: {} — {title}", doc.1)
                },
            );
            widgets::small_muted(ui, "Nada é enviado à IA nesta prévia.");
            ui.add_space(theme::S2);
            if !optional.is_empty() {
                widgets::field_label(ui, "Instruções opcionais ou condicionais");
                for (uid, name) in &optional {
                    let mut on = self.test_selected.contains(uid);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            self.test_selected.insert(uid.clone());
                        } else {
                            self.test_selected.remove(uid);
                        }
                    }
                }
                ui.add_space(theme::S2);
            }
            text_field(ui, "Pedido desta minuta", &mut self.test_guidance);
            build = ui.add(widgets::secondary("Montar prévia")).clicked();
        });
        if build {
            self.run_preview(store, doc);
        }
        match &self.test_result {
            None => {}
            Some(Err(msg)) => {
                let (fg, bg) = Tone::Danger.colors();
                widgets::pill_colored(ui, msg.clone(), fg, bg);
            }
            Some(Ok(p)) => {
                widgets::card(ui, |ui| {
                    widgets::section(ui, "O que seria enviado");
                    ui.add_space(theme::S1);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(theme::S2, theme::S2);
                        widgets::pill(
                            ui,
                            format!(
                                "Tamanho das instruções: {}",
                                crate::labels::file_size(p.trusted as u64)
                            ),
                            Tone::Neutral,
                        );
                        widgets::pill(
                            ui,
                            format!("Instruções incluídas: {}", p.blocks),
                            Tone::Neutral,
                        );
                        widgets::pill(
                            ui,
                            format!("Exemplos aprovados que seriam enviados: {}", p.examples),
                            Tone::Neutral,
                        );
                        widgets::pill(
                            ui,
                            format!("Arquivos de exemplo: {}", p.files),
                            Tone::Neutral,
                        );
                    });
                    for c in &p.conflicts {
                        ui.add_space(theme::S1);
                        let (fg, bg) = Tone::Warn.colors();
                        egui::Frame::new()
                            .fill(bg)
                            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                            .inner_margin(egui::Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{} {c}", icon::WARNING)).color(fg),
                                    )
                                    .wrap(),
                                );
                            });
                    }
                    ui.add_space(theme::S2);
                    let mut text = p.system.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_rows(14)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                });
            }
        }
    }

    fn run_preview(&mut self, store: &Store, doc: &(i64, String, Capture)) {
        let result = (|| -> anyhow::Result<Preview> {
            let t = self
                .edit
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Nenhum modelo aberto."))?;
            let v = t
                .versions
                .last()
                .ok_or_else(|| anyhow::anyhow!("O modelo não tem versão."))?;
            let resolved = crate::prompting::resolve_blocks(v, &self.blocks, &self.test_selected)?;
            let blocks = resolved.len();
            let examples: Vec<Example> = if t.uid.is_empty() {
                Vec::new()
            } else {
                // The same choice the generation makes for this document.
                crate::prompting::fit_examples(store.rank_examples(&t.uid, &doc.2)?)
            };
            let mut example_files = Vec::new();
            for example in &examples {
                example_files.extend(store.example_files(example)?);
            }
            let request = crate::prompting::DraftRequest {
                capture: &doc.2,
                kind: &t.kind,
                intent: Intent::New,
                local_context: "",
                request_guidance: &self.test_guidance,
                template: Some((t, v)),
                blocks: resolved,
                examples: &examples,
                example_files: &example_files,
                sources: &[],
                current_text: "",
                model: "",
                provider: "",
            };
            // The same base the generation uses, so the preview is faithful.
            let base = crate::ai::draft_base_system(&t.kind, &doc.2);
            let assembled = crate::prompting::assemble(&base, &request)?;
            Ok(Preview {
                trusted: assembled.trusted_bytes,
                blocks,
                examples: examples.len(),
                files: example_files.len(),
                conflicts: assembled.conflicts.iter().map(|c| c.describe()).collect(),
                system: assembled.system,
            })
        })();
        self.test_result = Some(result.map_err(|e| format!("{e:#}")));
    }
}

/// The template as the store has it now; the given one when it was never
/// saved or cannot be read.
fn fresh_template(store: &Store, template: WorkTemplate) -> WorkTemplate {
    if template.uid.is_empty() {
        return template;
    }
    store
        .template_by_uid(&template.uid)
        .ok()
        .flatten()
        .unwrap_or(template)
}

/// Development only: the views of this screen pictured by `--shots`, by
/// name. The tour calls `shots_open` with each one on a fresh panel.
#[cfg(feature = "shots")]
pub const SHOTS: &[&str] = &[
    "lista",
    "estrutura",
    "instrucoes",
    "exemplos",
    "referencias",
    "testar",
    "rascunho",
    "instrucao",
    "busca",
    "busca-vazia",
];

#[cfg(feature = "shots")]
impl TemplatesPanel {
    /// Puts the panel in the state called `view` (see `SHOTS`).
    pub fn shots_open(&mut self, store: &mut crate::storage::Store, view: &str) {
        self.refresh(store);
        let find =
            |panel: &Self, name: &str| panel.templates.iter().find(|t| t.name == name).cloned();
        let tab = match view {
            "instrucoes" | "instrucao" => Tab::Blocks,
            "exemplos" => Tab::Examples,
            "referencias" => Tab::References,
            "testar" => Tab::Test,
            _ => Tab::Structure,
        };
        match view {
            "lista" => {}
            "busca" => self.search = "informacoes".into(),
            "busca-vazia" => self.search = "parecer jurídico".into(),
            "rascunho" => {
                if let Some(t) = find(self, "Encaminhamento a setor") {
                    self.load_template(store, t, false);
                }
            }
            _ => {
                let Some(t) = find(self, "Resposta a pedido de informações") else {
                    return;
                };
                self.load_template(store, t, false);
                self.tab = tab;
                if view == "exemplos" {
                    let uid = self
                        .edit
                        .as_ref()
                        .map(|t| t.uid.clone())
                        .unwrap_or_default();
                    let existing = store.examples_of(&uid).unwrap_or_default();
                    let example = match existing.into_iter().next() {
                        Some(e) => Some(e),
                        None => store
                            .save_example(
                                &Example {
                                    template_uid: uid,
                                    title: "Resposta sobre prazo de atendimento".into(),
                                    demand: "O requerente pergunta o prazo para resposta.".into(),
                                    reply: "Prezado(a), informamos que o prazo é de vinte dias."
                                        .into(),
                                    origin: "ofício de 2024, anonimizado".into(),
                                    ..Default::default()
                                },
                                None,
                            )
                            .ok(),
                    };
                    if let Some(e) = example {
                        self.example_version_seen = expected(store, "example", &e.uid);
                        self.example_base = json(&e);
                        self.example_edit = Some(e);
                    }
                }
                if view == "instrucao" {
                    if let Some(b) = self.blocks.first().cloned() {
                        self.load_block(store, b, false);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_filter_ignores_case_and_accents() {
        assert!(name_matches(
            "Resposta a pedido de informações",
            "INFORMACOES"
        ));
        assert!(name_matches("Resposta a pedido de informações", " pedido "));
        assert!(name_matches("Orientação", "orientacao"));
        assert!(name_matches("Qualquer", ""));
        assert!(name_matches("Qualquer", "   "));
        assert!(!name_matches("Encaminhamento a setor", "informacoes"));
    }

    #[test]
    fn escape_closes_inline_forms_only_when_open() {
        let mut panel = TemplatesPanel::default();
        assert!(!panel.key_escape());
        panel.dup_name = Some("Cópia de Modelo A".into());
        assert!(panel.key_escape());
        assert!(panel.dup_name.is_none());
        assert!(!panel.key_escape());
        panel.confirm = Some(Confirm::Template);
        panel.confirm_discard_template = true;
        assert!(panel.key_escape());
        assert!(panel.confirm.is_none() && !panel.confirm_discard_template);
    }

    #[test]
    fn new_is_refused_with_unsaved_changes() {
        let mut panel = TemplatesPanel::default();
        assert!(panel.key_new());
        assert!(panel.pending_new);
        assert!(!panel.error);

        let mut panel = TemplatesPanel::default();
        let mut t = WorkTemplate::new("Modelo A", "resposta", "Texto");
        t.scope = Scope::Personal;
        panel.edit = Some(t);
        // Never saved: counts as unsaved work.
        assert!(panel.dirty());
        assert!(panel.key_new());
        assert!(!panel.pending_new);
        assert!(panel.error);
        assert!(panel.message.contains("Salve ou descarte"));
    }

    #[test]
    fn find_requests_focus() {
        let mut panel = TemplatesPanel::default();
        panel.key_find();
        assert!(panel.focus_search);
    }
}
