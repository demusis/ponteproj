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
use eframe::egui::{self, Color32, RichText};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(24, 84, 97);
const WARNING: Color32 = Color32::from_rgb(153, 88, 16);
const ERROR: Color32 = Color32::from_rgb(168, 40, 45);
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

fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn multiline(ui: &mut egui::Ui, label: &str, value: &mut String, rows: usize) {
    ui.label(label);
    ui.add(
        egui::TextEdit::multiline(value)
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    );
}

fn list_ops(ui: &mut egui::Ui, index: usize, len: usize) -> Option<Op> {
    let mut op = None;
    if ui
        .add_enabled(index > 0, egui::Button::new("↑").small())
        .clicked()
    {
        op = Some(Op::Up);
    }
    if ui
        .add_enabled(index + 1 < len, egui::Button::new("↓").small())
        .clicked()
    {
        op = Some(Op::Down);
    }
    if ui.small_button("Remover").clicked() {
        op = Some(Op::Remove);
    }
    op
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

fn string_list(ui: &mut egui::Ui, id: &str, items: &mut Vec<String>, hint: &str, max: usize) {
    let mut pending: Option<(usize, Op)> = None;
    let len = items.len();
    for (i, item) in items.iter_mut().enumerate() {
        ui.push_id((id, i), |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(item)
                        .hint_text(hint)
                        .desired_width(ui.available_width() - 190.0),
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
    if items.len() < max && ui.button("Adicionar").clicked() {
        items.push(String::new());
    }
}

fn badges((scope, status): (&'static str, &'static str), current: u32, pending: bool) -> String {
    let mut text = format!("{scope} · {status}");
    if current > 0 {
        text.push_str(&format!(" · v{current}"));
    }
    if pending {
        text.push_str(" · rascunho pendente");
    }
    text
}

impl TemplatesPanel {
    pub fn dirty(&self) -> bool {
        self.template_dirty()
            || self.block_dirty()
            || self.example_dirty()
            || self.attachments.dirty()
            || self.file_review.is_some()
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
        let member = store
            .load_intranet_config()
            .map(|c| c.member_name)
            .unwrap_or_default();
        let mut action = TemplatesAction::None;

        ui.horizontal(|ui| {
            ui.heading("Modelos de trabalho");
            crate::help_ui::help(ui,
                "Modelos guardam estrutura, instruções, exemplos e referências. Os publicados \
                 ficam disponíveis no editor de minutas; os compartilhados valem para toda a equipe.",
            );
        });
        ui.separator();

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                self.left_column(ui, store, &member);
            });
            ui.separator();
            ui.vertical(|ui| {
                if let Some(a) = self.right_column(ui, store, &member, open_document.as_ref()) {
                    action = a;
                }
            });
        });
        action
    }

    fn left_column(&mut self, ui: &mut egui::Ui, store: &mut Store, member: &str) {
        egui::ScrollArea::vertical()
            .id_salt("tpl_left")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Tipo");
                    let current = if self.filter_kind.is_empty() {
                        "Todos"
                    } else {
                        crate::labels::draft_kind(&self.filter_kind)
                    };
                    egui::ComboBox::from_id_salt("tpl_kind_filter")
                        .selected_text(current)
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
                });
                ui.checkbox(&mut self.show_archived, "Mostrar arquivados");
                ui.checkbox(&mut self.only_mine, "Só os meus");
                ui.add_space(4.0);

                let mine = owner_key(member);
                let mut pick: Option<WorkTemplate> = None;
                let selected = self.edit.as_ref().map(|t| t.uid.clone());
                for t in &self.templates {
                    if !t.visible_to(member)
                        || (!self.filter_kind.is_empty() && t.kind != self.filter_kind)
                        || (!self.show_archived && t.status == Status::Archived)
                        || (self.only_mine && owner_key(&t.owner) != mine)
                    {
                        continue;
                    }
                    let is_sel = selected.as_deref() == Some(t.uid.as_str());
                    if ui.selectable_label(is_sel, &t.name).clicked() && !is_sel {
                        pick = Some(t.clone());
                    }
                    ui.label(
                        RichText::new(badges(
                            (t.scope.label(), t.status.label()),
                            t.current_version,
                            t.draft_version().is_some() && t.current_version > 0,
                        ))
                        .small()
                        .weak(),
                    );
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

                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Novo modelo").clicked() {
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
                    let stored = self.stored().cloned();
                    if ui
                        .add_enabled(stored.is_some(), egui::Button::new("Duplicar…"))
                        .clicked()
                    {
                        self.dup_name = stored.as_ref().map(|t| format!("Cópia de {}", t.name));
                    }
                    if let Some(t) = &stored {
                        let wanted = if t.status == Status::Archived {
                            ui.button("Desarquivar")
                                .clicked()
                                .then_some(Status::Published)
                        } else {
                            ui.button("Arquivar").clicked().then_some(Status::Archived)
                        };
                        if let Some(status) = wanted {
                            if self.dirty() {
                                self.notice(
                                    "Salve ou descarte as alterações antes de arquivar ou desarquivar.",
                                    true,
                                );
                            } else {
                                self.set_status(store, status);
                            }
                        }
                        if ui.button("Excluir").clicked() {
                            self.confirm = Some(Confirm::Template);
                        }
                    }
                });
                if let Some(name) = self.dup_name.as_mut() {
                    ui.add(egui::TextEdit::singleline(name).hint_text("Nome da cópia"));
                    let name = name.clone();
                    ui.horizontal(|ui| {
                        if ui.button("Criar cópia").clicked() {
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
                                match store.duplicate_template(&uid, name.trim()) {
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
                        if ui.button("Cancelar").clicked() {
                            self.dup_name = None;
                        }
                    });
                }
                if self.confirm == Some(Confirm::Template) {
                    ui.colored_label(
                        WARNING,
                        "Excluir este modelo de vez, com exemplos e versões?",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Sim, excluir").clicked() {
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
                                    self.notice("Modelo excluído.", false);
                                }
                                Err(e) => self.notice(format!("{e:#}"), true),
                            }
                            self.confirm = None;
                        }
                        if ui.button("Cancelar").clicked() {
                            self.confirm = None;
                        }
                    });
                }

                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.strong("Instruções (biblioteca)");
                    crate::help_ui::help(ui,
                        "Instruções são orientações para a IA que valem para vários modelos, \
                         escritas uma vez e reaproveitadas: \"Use linguagem simples\", \
                         \"Cite a norma aplicável\", \"Não prometa prazos\". Crie e publique a \
                         instrução aqui; depois, no modelo, aba Instruções, inclua-a como \
                         obrigatória (sempre vai), opcional (você marca ao gerar) ou condicional \
                         (vale só no caso descrito). Mudar a instrução não altera minutas já feitas.",
                    );
                });
                ui.label(
                    RichText::new("Orientações reutilizáveis que os modelos podem incluir.")
                        .small()
                        .weak(),
                );
                self.block_list(ui, store, member);
            });
    }

    fn block_list(&mut self, ui: &mut egui::Ui, store: &Store, member: &str) {
        let mut pick: Option<InstructionBlock> = None;
        let selected = self.block_edit.as_ref().map(|b| b.uid.clone());
        for b in &self.blocks {
            if !b.visible_to(member) || (!self.show_archived && b.status == Status::Archived) {
                continue;
            }
            let is_sel = selected.as_deref() == Some(b.uid.as_str());
            if ui.selectable_label(is_sel, &b.name).clicked() && !is_sel {
                pick = Some(b.clone());
            }
            ui.label(
                RichText::new(badges(
                    (scope_label_f(b.scope), status_label_f(b.status)),
                    b.current_version,
                    b.draft_version().is_some() && b.current_version > 0,
                ))
                .small()
                .weak(),
            );
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
        if ui.button("Nova instrução").clicked() {
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
    }

    fn right_column(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        member: &str,
        doc: Option<&(i64, String, Capture)>,
    ) -> Option<TemplatesAction> {
        if !self.message.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(if self.error { ERROR } else { ACCENT }, &self.message);
                if ui.small_button("Fechar").clicked() {
                    self.message.clear();
                }
            });
        }
        if self.edit.is_none() {
            ui.label("Escolha um modelo na lista ou crie um novo.");
            return None;
        }
        let mut action = None;
        self.action_row(ui, store, doc.is_some(), &mut action);
        ui.separator();
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Structure, "Estrutura"),
                (Tab::Blocks, "Instruções"),
                (Tab::Examples, "Exemplos"),
                (Tab::References, "Referências"),
                (Tab::Test, "Testar"),
            ] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("tpl_right")
            .show(ui, |ui| match self.tab {
                Tab::Structure => self.tab_structure(ui),
                Tab::Blocks => self.tab_blocks(ui, store, member),
                Tab::Examples => self.tab_examples(ui, store),
                Tab::References => self.tab_references(ui),
                Tab::Test => self.tab_test(ui, store, doc),
            });
        action
    }

    fn action_row(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        has_doc: bool,
        action: &mut Option<TemplatesAction>,
    ) {
        let stored = self.stored().cloned();
        if let Some(t) = &stored {
            if let Some(v) = t.published_version() {
                ui.label(format!(
                    "Publicada: versão {} por {} em {}",
                    v.version,
                    v.author,
                    crate::labels::datetime(&v.created_at)
                ));
            }
        }
        let draft_number = self
            .edit
            .as_ref()
            .and_then(|t| t.versions.last())
            .map(|v| v.version)
            .unwrap_or(1);
        if stored.as_ref().is_some_and(|t| t.draft_version().is_some()) {
            ui.label(format!("Rascunho: versão {draft_number}"));
        }
        if self.template_dirty() {
            ui.colored_label(WARNING, "● alterações não salvas");
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Salvar rascunho").clicked() {
                self.save_template(store);
            }
            // A published template without a draft has nothing new to publish;
            // the store would refuse, so say it up front.
            let nothing_new = !self.template_dirty()
                && stored.as_ref().is_some_and(|t| {
                    t.published_version().is_some() && t.draft_version().is_none()
                });
            if ui
                .add_enabled(
                    !nothing_new,
                    egui::Button::new(format!("Publicar versão {draft_number}")),
                )
                .on_disabled_hover_text(
                    "Não há alterações a publicar. Altere o modelo e salve o rascunho antes.",
                )
                .clicked()
            {
                self.publish_template(store);
            }
            if ui.button("Descartar alterações").clicked() {
                if self.template_dirty() {
                    self.confirm_discard_template = true;
                } else {
                    self.discard_template(store);
                }
            }
            let can_use = has_doc
                && stored.as_ref().is_some_and(|t| {
                    t.status == Status::Published && t.published_version().is_some()
                });
            if can_use && ui.button("Usar este modelo na minuta").clicked() {
                if let Some(t) = &stored {
                    *action = Some(TemplatesAction::UseTemplate { uid: t.uid.clone() });
                }
            }
        });
        if self.confirm_discard_template {
            ui.colored_label(
                WARNING,
                "Descartar todas as alterações não salvas do modelo?",
            );
            ui.horizontal(|ui| {
                if ui.button("Sim, descartar").clicked() {
                    self.discard_template(store);
                }
                if ui.button("Cancelar").clicked() {
                    self.confirm_discard_template = false;
                }
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

    // -- tabs ---------------------------------------------------------------

    fn tab_structure(&mut self, ui: &mut egui::Ui) {
        let Some(t) = self.edit.as_mut() else { return };
        let Some(v) = t.versions.last_mut() else {
            return;
        };
        text_row(ui, "Nome", &mut t.name);
        text_row(ui, "Finalidade", &mut t.purpose);
        ui.horizontal(|ui| {
            ui.label("Tipo de minuta");
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
        });
        multiline(
            ui,
            "Critérios de uso (quando usar este modelo)",
            &mut t.usage_criteria,
            3,
        );
        ui.horizontal(|ui| {
            ui.label("Quem vê");
            ui.radio_value(&mut t.scope, Scope::Personal, "Pessoal");
            ui.radio_value(&mut t.scope, Scope::Shared, "Compartilhado com a equipe");
        });

        ui.add_space(6.0);
        ui.strong("Seções");
        let mut pending: Option<(usize, Op)> = None;
        let len = v.sections.len();
        for (i, s) in v.sections.iter_mut().enumerate() {
            ui.push_id(("section", i), |ui| {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut s.title)
                                .hint_text("Título da seção")
                                .desired_width(260.0),
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
            });
        }
        if let Some((i, op)) = pending {
            apply_op(&mut v.sections, i, op);
        }
        if ui.button("Adicionar seção").clicked() {
            v.sections.push(Section::default());
        }

        ui.add_space(6.0);
        ui.strong("Variáveis");
        ui.small("Trechos como {{nome}} nas seções são preenchidos pela pessoa.");
        let mut remove: Option<usize> = None;
        for (i, var) in v.variables.iter_mut().enumerate() {
            ui.push_id(("var", i), |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut var.name)
                            .hint_text("nome")
                            .desired_width(130.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut var.label)
                            .hint_text("como aparece")
                            .desired_width(220.0),
                    );
                    ui.checkbox(&mut var.required, "Obrigatória");
                    if ui.small_button("Remover").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            v.variables.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.button("Adicionar variável").clicked() {
                v.variables.push(Variable::default());
            }
            if ui.button("Detectar variáveis").clicked() {
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

        ui.add_space(6.0);
        ui.strong("Critérios de conferência");
        ui.small("O que conferir antes de concluir a minuta.");
        string_list(
            ui,
            "criteria",
            &mut v.criteria,
            "ex.: confirmar o prazo informado",
            crate::work_templates::MAX_CRITERIA,
        );

        ui.add_space(6.0);
        text_row(ui, "Nota desta versão", &mut v.change_note);
    }

    fn tab_references(&mut self, ui: &mut egui::Ui) {
        let Some(v) = self.edit.as_mut().and_then(|t| t.versions.last_mut()) else {
            return;
        };
        ui.small("Referências são indicadas à pessoa; o Ponte não as consulta sozinho.");
        let mut pending: Option<(usize, Op)> = None;
        let len = v.references.len();
        for (i, r) in v.references.iter_mut().enumerate() {
            ui.push_id(("ref", i), |ui| {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut r.label)
                                .hint_text("Nome")
                                .desired_width(240.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut r.locator)
                                .hint_text("Onde encontrar (número, endereço, pasta)")
                                .desired_width(300.0),
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
            });
        }
        if let Some((i, op)) = pending {
            apply_op(&mut v.references, i, op);
        }
        if v.references.len() < crate::work_templates::MAX_REFERENCES
            && ui.button("Adicionar referência").clicked()
        {
            v.references.push(Reference::default());
        }
    }

    fn tab_blocks(&mut self, ui: &mut egui::Ui, store: &mut Store, member: &str) {
        ui.columns(2, |cols| {
            self.block_editor(&mut cols[0], store);
            self.block_refs(&mut cols[1], member);
        });
    }

    fn block_editor(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        ui.strong("Instrução da biblioteca");
        let dirty = self.block_dirty();
        let Some(b) = self.block_edit.as_mut() else {
            ui.label("Escolha uma instrução na lista à esquerda ou crie uma nova.");
            return;
        };
        let stored_status = if b.uid.is_empty() {
            None
        } else {
            Some(b.status)
        };
        let draft_number = b.versions.last().map(|v| v.version).unwrap_or(1);
        text_row(ui, "Nome", &mut b.name);
        text_row(ui, "Tema", &mut b.topic);
        ui.horizontal(|ui| {
            ui.radio_value(&mut b.scope, Scope::Personal, "Pessoal");
            ui.radio_value(&mut b.scope, Scope::Shared, "Compartilhada");
        });
        if let Some(v) = b.versions.last_mut() {
            multiline(ui, "Texto da instrução", &mut v.text, 8);
            text_row(ui, "Nota desta versão", &mut v.change_note);
        }
        if dirty {
            ui.colored_label(WARNING, "● alterações não salvas");
        }
        let mut do_save = false;
        let mut do_publish = false;
        let mut do_status: Option<Status> = None;
        let mut do_delete = false;
        let mut do_discard = false;
        ui.horizontal_wrapped(|ui| {
            do_save = ui.button("Salvar rascunho da instrução").clicked();
            do_publish = ui
                .button(format!("Publicar versão {draft_number}"))
                .clicked();
            do_discard = ui.button("Descartar").clicked();
            match stored_status {
                Some(Status::Archived) => {
                    do_status = ui
                        .button("Desarquivar")
                        .clicked()
                        .then_some(Status::Published)
                }
                Some(_) => do_status = ui.button("Arquivar").clicked().then_some(Status::Archived),
                None => {}
            }
            if do_status.is_some() && self.dirty() {
                do_status = None;
                self.notice(
                    "Salve ou descarte as alterações antes de arquivar ou desarquivar.",
                    true,
                );
            }
            if stored_status.is_some() && ui.button("Excluir").clicked() {
                do_delete = true;
            }
        });
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
        if self.confirm_discard_block {
            ui.colored_label(
                WARNING,
                "Descartar as alterações não salvas desta instrução?",
            );
            ui.horizontal(|ui| {
                if ui.button("Sim, descartar").clicked() {
                    self.discard_block(store);
                }
                if ui.button("Cancelar").clicked() {
                    self.confirm_discard_block = false;
                }
            });
        }
        if self.confirm == Some(Confirm::Block) {
            ui.colored_label(WARNING, "Excluir esta instrução de vez?");
            ui.horizontal(|ui| {
                if ui.button("Sim, excluir").clicked() {
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
                            self.notice("Instrução excluída.", false);
                        }
                        Err(e) => self.notice(format!("{e:#}"), true),
                    }
                    self.confirm = None;
                }
                if ui.button("Cancelar").clicked() {
                    self.confirm = None;
                }
            });
        }
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

    fn block_refs(&mut self, ui: &mut egui::Ui, member: &str) {
        ui.strong("Instruções deste modelo");
        ui.small(
            "As alterações valem no rascunho do modelo; use \"Salvar rascunho\" no alto, \
             não o botão da instrução.",
        );
        let blocks = &self.blocks;
        let Some(v) = self.edit.as_mut().and_then(|t| t.versions.last_mut()) else {
            return;
        };
        let mut pending: Option<(usize, Op)> = None;
        let len = v.block_refs.len();
        for (i, r) in v.block_refs.iter_mut().enumerate() {
            let name = blocks
                .iter()
                .find(|b| b.uid == r.block_uid)
                .map(|b| b.name.as_str())
                .unwrap_or("(instrução removida)");
            ui.push_id(("bref", i), |ui| {
                ui.group(|ui| {
                    ui.label(RichText::new(name).strong());
                    ui.small(match r.version {
                        Some(n) => format!("v{n}"),
                        None => "versão publicada atual".to_owned(),
                    });
                    let mut kind = match r.mode {
                        BlockMode::Mandatory => 0,
                        BlockMode::Optional => 1,
                        BlockMode::Conditional { .. } => 2,
                    };
                    let before = kind;
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
                    if let Some(op) = list_ops(ui, i, len) {
                        pending = Some((i, op));
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
        if v.block_refs.len() >= MAX_BLOCKS_PER_TEMPLATE {
            ui.small(format!(
                "Máximo de {MAX_BLOCKS_PER_TEMPLATE} instruções por modelo."
            ));
        } else {
            let mut add: Option<String> = None;
            egui::ComboBox::from_id_salt("bref_add")
                .selected_text("Adicionar instrução ao modelo")
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

    fn tab_examples(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        let Some(uid) = self.stored().map(|t| t.uid.clone()) else {
            ui.label("Salve o rascunho do modelo para cadastrar exemplos.");
            return;
        };
        ui.small(
            "Só exemplos aprovados são mostrados à IA; os fatos deles nunca valem para o caso.",
        );
        let examples = match store.examples_of(&uid) {
            Ok(list) => list,
            Err(e) => {
                ui.colored_label(ERROR, format!("{e:#}"));
                return;
            }
        };
        let mut pick: Option<Example> = None;
        let selected = self.example_edit.as_ref().map(|e| e.uid.clone());
        for e in &examples {
            ui.horizontal(|ui| {
                let is_sel = selected.as_deref() == Some(e.uid.as_str());
                if ui.selectable_label(is_sel, &e.title).clicked() && !is_sel {
                    pick = Some(e.clone());
                }
                let color = if e.status == ExampleStatus::Unverified {
                    WARNING
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.colored_label(color, e.status.label());
            });
        }
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
        if ui.button("Novo exemplo").clicked() {
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
        ui.separator();
        let dirty = self.example_dirty();
        let Some(e) = self.example_edit.as_mut() else {
            return;
        };
        let is_new = e.uid.is_empty();
        text_row(ui, "Título", &mut e.title);
        multiline(ui, "Demanda", &mut e.demand, 4);
        multiline(ui, "Resposta revisada", &mut e.reply, 8);
        multiline(ui, "Explicação das escolhas", &mut e.explanation, 3);
        ui.label("Comentários");
        string_list(
            ui,
            "ex_comments",
            &mut e.comments,
            "ex.: aproveitar a organização das seções; não reutilizar fatos nem fundamentação",
            10,
        );
        ui.label("Origem");
        ui.add(
            egui::TextEdit::singleline(&mut e.origin)
                .hint_text("ex.: ofício de 2024, anonimizado")
                .desired_width(f32::INFINITY),
        );
        if dirty {
            ui.colored_label(WARNING, "● alterações não salvas");
        }
        let (mut save, mut approve, mut reject, mut delete) = (false, false, false, false);
        let mut discard = false;
        ui.horizontal_wrapped(|ui| {
            save = ui.button("Salvar exemplo").clicked();
            approve = ui.button("Aprovar").clicked();
            reject = ui.button("Rejeitar").clicked();
            discard = ui.button("Descartar").clicked();
            if !is_new {
                delete = ui.button("Excluir").clicked();
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
        if self.confirm_discard_example {
            ui.colored_label(WARNING, "Descartar as alterações não salvas deste exemplo?");
            ui.horizontal(|ui| {
                if ui.button("Sim, descartar").clicked() {
                    self.discard_example(store);
                }
                if ui.button("Cancelar").clicked() {
                    self.confirm_discard_example = false;
                }
            });
        }
        if delete {
            self.confirm = Some(Confirm::Example);
        }
        if self.confirm == Some(Confirm::Example) {
            ui.colored_label(WARNING, "Excluir este exemplo de vez?");
            ui.horizontal(|ui| {
                if ui.button("Sim, excluir").clicked() {
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
                            self.notice("Exemplo excluído.", false);
                        }
                        Err(e) => self.notice(format!("{e:#}"), true),
                    }
                    self.confirm = None;
                }
                if ui.button("Cancelar").clicked() {
                    self.confirm = None;
                }
            });
        }
        let example_id = self
            .example_edit
            .as_ref()
            .filter(|e| !e.uid.is_empty())
            .map(|e| e.id)
            .filter(|id| *id > 0);
        if let Some(example_id) = example_id {
            self.example_files_section(ui, store, example_id);
        }
    }

    /// Files attached to a saved example, with the state of their text.
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
        ui.separator();
        ui.strong("Arquivos do exemplo");
        ui.small(
            "Os arquivos mostram à IA a forma de uma resposta real. Só o texto extraído ou conferido de um exemplo aprovado é enviado; os fatos dos arquivos nunca valem para o caso. PDF com texto, .txt e .docx têm extração; PDF digitalizado precisa de OCR, que esta versão não faz para exemplos.",
        );
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
                ui.colored_label(ERROR, format!("{e:#}"));
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
            ui.horizontal_wrapped(|ui| {
                ui.label(&meta.name);
                match &record {
                    None => {
                        ui.colored_label(WARNING, "Texto ainda não extraído");
                    }
                    Some(t) if !t.current_for(&meta.sha256) => {
                        ui.colored_label(WARNING, "Texto desatualizado (extraia de novo)");
                    }
                    Some(t) => {
                        use crate::extraction::TextStatus;
                        let warn = matches!(t.status, TextStatus::Empty | TextStatus::Failed);
                        if warn {
                            ui.colored_label(WARNING, t.status.label());
                        } else {
                            ui.label(t.status.label());
                        }
                        if t.status == TextStatus::Reviewed && !t.reviewed_by.is_empty() {
                            ui.small(format!("por {}", t.reviewed_by));
                        }
                        if !t.note.is_empty() {
                            ui.small(&t.note);
                        }
                    }
                }
                let reviewed = record
                    .as_ref()
                    .is_some_and(|t| t.status == crate::extraction::TextStatus::Reviewed);
                if reviewed {
                    // A person's correction is not thrown away by one click.
                    if self.confirm_reextract == Some(meta.id) {
                        ui.small("Substitui o texto conferido.");
                        if ui.small_button("Confirmar").clicked() {
                            extract = Some((meta.id, meta.name.clone()));
                        }
                        if ui.small_button("Cancelar").clicked() {
                            self.confirm_reextract = None;
                        }
                    } else if ui.small_button("Extrair de novo…").clicked() {
                        self.confirm_reextract = Some(meta.id);
                    }
                } else if ui.small_button("Extrair texto").clicked() {
                    extract = Some((meta.id, meta.name.clone()));
                }
                if let Some(t) = record {
                    if (t.status.usable() || t.status == crate::extraction::TextStatus::Empty)
                        && ui.small_button("Conferir texto…").clicked()
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
                    if ui.button("Marcar como conferido").clicked() {
                        review = Some((*id, text.clone()));
                    }
                    if ui.button("Cancelar").clicked() {
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
            ui.label("Abra um documento na Ficha para testar o modelo com ele.");
            return;
        };
        ui.label(format!("Documento: {}", doc.1));
        ui.small("Nada é enviado à IA nesta prévia.");
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
        if !optional.is_empty() {
            ui.strong("Instruções opcionais ou condicionais");
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
        }
        text_row(ui, "Pedido desta minuta", &mut self.test_guidance);
        if ui.button("Montar prévia").clicked() {
            self.run_preview(store, doc);
        }
        match &self.test_result {
            None => {}
            Some(Err(msg)) => {
                ui.colored_label(ERROR, msg);
            }
            Some(Ok(p)) => {
                ui.label(format!(
                    "Tamanho da parte de instruções: {}",
                    crate::labels::file_size(p.trusted as u64)
                ));
                ui.label(format!("Instruções incluídas: {}", p.blocks));
                ui.label(format!(
                    "Exemplos aprovados que seriam enviados: {}",
                    p.examples
                ));
                ui.label(format!("Arquivos de exemplo: {}", p.files));
                for c in &p.conflicts {
                    ui.colored_label(WARNING, c);
                }
                let mut text = p.system.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(14)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
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
