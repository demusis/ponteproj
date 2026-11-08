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

fn badges(scope: Scope, status: Status, current: u32, pending: bool) -> String {
    let mut text = format!("{} · {}", scope.label(), status.label());
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
        self.template_dirty() || self.block_dirty() || self.example_dirty()
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
        self.load_template(template, true);
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

    fn load_template(&mut self, mut template: WorkTemplate, unsaved: bool) {
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
        self.block_edit = None;
        self.example_edit = None;
        self.confirm = None;
        self.dup_name = None;
        self.test_selected.clear();
        self.test_result = None;
    }

    fn load_block(&mut self, mut block: InstructionBlock, unsaved: bool) {
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
        let exp = expected(store, "template", &template.uid);
        match store.save_template(&template, exp) {
            Ok(saved) => {
                self.load_template(saved, false);
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
        let exp = expected(store, "template", &uid);
        match store.set_template_status(&uid, Status::Published, exp) {
            Ok(saved) => {
                let n = saved.current_version;
                self.load_template(saved, false);
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
        let exp = expected(store, "template", &uid);
        match store.set_template_status(&uid, status, exp) {
            Ok(saved) => {
                self.load_template(saved, false);
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
        let exp = expected(store, "block", &block.uid);
        match store.save_block(&block, exp) {
            Ok(saved) => {
                self.load_block(saved, false);
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
        let exp = expected(store, "block", &uid);
        match store.set_block_status(&uid, status, exp) {
            Ok(saved) => {
                self.load_block(saved, false);
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
        if let Some(status) = status {
            example.status = status;
        }
        let exp = expected(store, "example", &example.uid);
        match store.save_example(&example, exp) {
            Ok(saved) => {
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
            ui.label("ⓘ").on_hover_text(
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
                            t.scope,
                            t.status,
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
                        self.load_template(t, false);
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
                            self.load_template(t, false);
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
                            let uid = self
                                .edit
                                .as_ref()
                                .map(|t| t.uid.clone())
                                .unwrap_or_default();
                            match store.duplicate_template(&uid, name.trim()) {
                                Ok(copy) => {
                                    self.load_template(copy, false);
                                    self.stale = true;
                                    self.notice("Cópia criada como rascunho pessoal.", false);
                                }
                                Err(e) => self.notice(format!("{e:#}"), true),
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
                ui.strong("Instruções (biblioteca)");
                self.block_list(ui, member);
            });
    }

    fn block_list(&mut self, ui: &mut egui::Ui, member: &str) {
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
                    b.scope,
                    b.status,
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
                self.load_block(b, false);
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
                self.load_block(InstructionBlock::new("", "", ""), false);
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
                Tab::Examples => self.tab_examples(ui, store, member),
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
            if ui
                .button(format!("Publicar versão {draft_number}"))
                .clicked()
            {
                self.publish_template(store);
            }
            if ui.button("Descartar alterações").clicked() {
                match self.stored().cloned() {
                    Some(t) => self.load_template(t, false),
                    None => {
                        self.edit = None;
                        self.base.clear();
                    }
                }
                self.notice("Alterações descartadas.", false);
            }
            let can_use = has_doc
                && stored
                    .as_ref()
                    .is_some_and(|t| t.published_version().is_some());
            if can_use && ui.button("Usar este modelo na minuta").clicked() {
                if let Some(t) = &stored {
                    *action = Some(TemplatesAction::UseTemplate { uid: t.uid.clone() });
                }
            }
        });
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
        ui.horizontal_wrapped(|ui| {
            do_save = ui.button("Salvar rascunho").clicked();
            do_publish = ui
                .button(format!("Publicar versão {draft_number}"))
                .clicked();
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

    fn block_refs(&mut self, ui: &mut egui::Ui, member: &str) {
        ui.strong("Instruções deste modelo");
        ui.small("As alterações valem no rascunho; use \"Salvar rascunho\" no alto.");
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

    fn tab_examples(&mut self, ui: &mut egui::Ui, store: &mut Store, _member: &str) {
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
            if self.example_dirty() {
                self.notice(
                    "Salve ou descarte as alterações do exemplo antes de trocar.",
                    true,
                );
            } else {
                self.example_base = json(&e);
                self.example_edit = Some(e);
                self.confirm = None;
            }
        }
        if ui.button("Novo exemplo").clicked() {
            if self.example_dirty() {
                self.notice(
                    "Salve ou descarte as alterações do exemplo antes de trocar.",
                    true,
                );
            } else {
                self.example_edit = Some(Example {
                    template_uid: uid.clone(),
                    ..Default::default()
                });
                self.example_base.clear();
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
        ui.horizontal_wrapped(|ui| {
            save = ui.button("Salvar exemplo").clicked();
            approve = ui.button("Aprovar").clicked();
            reject = ui.button("Rejeitar").clicked();
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
                store
                    .examples_of(&t.uid)?
                    .into_iter()
                    .filter(|e| e.status == ExampleStatus::Approved)
                    .rev()
                    .take(crate::prompting::MAX_EXAMPLES)
                    .collect()
            };
            let request = crate::prompting::DraftRequest {
                capture: &doc.2,
                kind: &t.kind,
                intent: Intent::New,
                local_context: "",
                request_guidance: &self.test_guidance,
                template: Some((t, v)),
                blocks: resolved,
                examples: &examples,
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
                conflicts: assembled.conflicts.iter().map(|c| c.describe()).collect(),
                system: assembled.system,
            })
        })();
        self.test_result = Some(result.map_err(|e| format!("{e:#}")));
    }
}
