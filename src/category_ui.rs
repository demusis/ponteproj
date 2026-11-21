//! Categories on the screens: the palette, the chips, the picker used by the
//! task form and the document ficha, the filter and the manager in Ajustes.
//! The rules (what the list may hold, what an item may have) live in
//! `categories`; here is only what the person sees.
use crate::categories::{name_key, Category, MAX_CATEGORIES, MAX_NAME, MAX_PER_ITEM, PALETTE_SIZE};
use crate::storage::Store;
use crate::theme::{self, c, icon, S1, S2, S3};
use crate::widgets::{self, Tone};
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense};
use std::collections::BTreeSet;

/// Base colour of each palette entry; the tints for each theme come from it.
const HUES: [(u8, u8, u8); PALETTE_SIZE as usize] = [
    (30, 150, 160),  // teal
    (50, 110, 210),  // blue
    (110, 80, 200),  // violet
    (190, 70, 170),  // magenta
    (210, 60, 70),   // red
    (220, 100, 30),  // orange
    (200, 150, 10),  // amber
    (110, 150, 30),  // olive
    (40, 160, 90),   // green
    (100, 115, 125), // slate
];

/// Surface of the dark theme, which the tints are mixed into.
const DARK_SURFACE: (u8, u8, u8) = (26, 36, 40);

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Color32::from_rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// (text, background, dot) of a palette entry in the light or dark theme.
pub fn swatch_colors(index: u8, dark: bool) -> (Color32, Color32, Color32) {
    let hue = HUES[usize::from(index.min(PALETTE_SIZE - 1))];
    let dot = Color32::from_rgb(hue.0, hue.1, hue.2);
    if dark {
        (
            mix((255, 255, 255), hue, 0.25),
            mix(DARK_SURFACE, hue, 0.22),
            dot,
        )
    } else {
        (
            mix((0, 0, 0), hue, 0.5),
            mix((255, 255, 255), hue, 0.16),
            dot,
        )
    }
}

fn colors_of(category: &Category) -> (Color32, Color32, Color32) {
    if category.archived {
        let p = c();
        (p.muted, p.sunken, p.muted)
    } else {
        swatch_colors(category.color, theme::is_dark())
    }
}

/// The name cut for a compact place; the hover has the whole name.
fn shorten(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_owned()
    } else {
        let cut: String = name.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    }
}

/// A small rounded label with the tint of a category.
fn chip_label(
    ui: &mut egui::Ui,
    text: String,
    fg: Color32,
    bg: Color32,
    sense: Sense,
) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::font_strong(12.0), fg);
    let size = galley.size() + egui::vec2(16.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
        ui.painter()
            .galley(rect.min + egui::vec2(8.0, 2.0), galley, fg);
    }
    response
}

/// One category as a small chip: tinted background and readable text; an
/// archived one is muted.
pub fn category_chip(ui: &mut egui::Ui, category: &Category) -> egui::Response {
    let (fg, bg, _) = colors_of(category);
    let response = chip_label(ui, shorten(&category.name, 24), fg, bg, Sense::hover());
    if category.archived {
        response.on_hover_text(format!("{} (arquivada)", category.name))
    } else {
        response.on_hover_text(category.name.clone())
    }
}

/// The categories of an item that exist in `all`, in the order of the item.
fn known<'a>(uids: &[String], all: &'a [Category]) -> Vec<&'a Category> {
    uids.iter()
        .filter_map(|uid| all.iter().find(|c| &c.uid == uid))
        .collect()
}

/// The chips of an item, wrapping to more lines.
pub fn category_chips(ui: &mut egui::Ui, uids: &[String], all: &[Category]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(S1 + 2.0, S1);
        for category in known(uids, all) {
            category_chip(ui, category);
        }
    });
}

/// Compact chips for a row of a list, in the line the caller is already
/// laying out: at most `max`, then "+N" with the rest in the hover.
pub fn category_chips_compact(ui: &mut egui::Ui, uids: &[String], all: &[Category], max: usize) {
    let items = known(uids, all);
    for category in items.iter().take(max) {
        category_chip(ui, category);
    }
    if items.len() > max {
        let rest: Vec<&str> = items[max..].iter().map(|c| c.name.as_str()).collect();
        let p = c();
        chip_label(
            ui,
            format!("+{}", rest.len()),
            p.muted,
            p.sunken,
            Sense::hover(),
        )
        .on_hover_text(rest.join("\n"));
    }
}

/// A round colour dot of the palette.
fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.0, color);
}

/// The ten colours as round buttons; returns the one clicked.
pub fn swatches(ui: &mut egui::Ui, current: u8) -> Option<u8> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(S1 + 2.0, S1 + 2.0);
        for index in 0..PALETTE_SIZE {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
            let (_, _, color) = swatch_colors(index, theme::is_dark());
            ui.painter().circle_filled(rect.center(), 8.0, color);
            if index == current {
                ui.painter()
                    .circle_stroke(rect.center(), 10.0, egui::Stroke::new(2.0, c().text));
            } else if response.hovered() {
                ui.painter()
                    .circle_stroke(rect.center(), 10.0, egui::Stroke::new(1.0, c().muted));
            }
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                chosen = Some(index);
            }
        }
    });
    chosen
}

// ---- rules of the controls (no screen needed) ----

/// Which categories narrow a list: an item matches when it has ANY of the
/// chosen ones, or none at all when "Sem categoria" is on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CategoryFilter {
    pub chosen: BTreeSet<String>,
    pub none: bool,
}

/// No narrowing, for places that need a filter by reference.
pub static NO_CATEGORY_FILTER: CategoryFilter = CategoryFilter {
    chosen: BTreeSet::new(),
    none: false,
};

impl CategoryFilter {
    pub fn is_active(&self) -> bool {
        self.none || !self.chosen.is_empty()
    }

    pub fn clear(&mut self) {
        self.chosen.clear();
        self.none = false;
    }

    pub fn matches(&self, item: &[String]) -> bool {
        if !self.is_active() {
            return true;
        }
        if item.is_empty() {
            return self.none;
        }
        item.iter().any(|uid| self.chosen.contains(uid))
    }
}

/// Whether one more category fits on the item.
pub fn can_add(selected: &[String]) -> bool {
    selected.len() < MAX_PER_ITEM
}

/// Checks or unchecks `uid`; refuses a new one past the limit. Returns
/// whether the list changed.
pub fn toggle(selected: &mut Vec<String>, uid: &str) -> bool {
    if let Some(position) = selected.iter().position(|u| u == uid) {
        selected.remove(position);
        true
    } else if can_add(selected) {
        selected.push(uid.to_owned());
        true
    } else {
        false
    }
}

/// The name to offer in "Criar “…”": what was typed, when it is a usable
/// name and no category (archived ones included) has it, ignoring case and
/// accents.
pub fn create_offer(query: &str, all: &[Category]) -> Option<String> {
    let name = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return None;
    }
    let key = name_key(&name);
    if all.iter().any(|c| c.name_key() == key) {
        return None;
    }
    Some(name)
}

/// The colour a new category gets: the first one no category uses, or the
/// least used one when all ten are taken.
pub fn next_color(all: &[Category]) -> u8 {
    let mut uses = [0usize; PALETTE_SIZE as usize];
    for category in all {
        if category.color < PALETTE_SIZE {
            uses[usize::from(category.color)] += 1;
        }
    }
    let least = uses.iter().copied().min().unwrap_or(0);
    uses.iter().position(|n| *n == least).unwrap_or(0) as u8
}

/// The categories the picker offers for what was typed: not archived, with
/// the typed text in the name (ignoring case and accents).
pub fn offered<'a>(all: &'a [Category], query: &str) -> Vec<&'a Category> {
    let key = name_key(query);
    all.iter()
        .filter(|c| !c.archived && (key.is_empty() || c.name_key().contains(&key)))
        .collect()
}

/// Creates the category the person typed and puts it on the item.
pub fn create_and_select(
    store: &mut Store,
    name: &str,
    selected: &mut Vec<String>,
    all: &[Category],
) -> Result<Category, String> {
    if !can_add(selected) {
        return Err(format!(
            "Um item pode ter no máximo {MAX_PER_ITEM} categorias."
        ));
    }
    let created = store
        .save_category(&Category::new(name, next_color(all)))
        .map_err(|e| e.to_string())?;
    selected.push(created.uid.clone());
    Ok(created)
}

// ---- the picker ----

/// What the person did in the picker during this frame.
#[derive(Debug, Default)]
pub struct PickerOutput {
    /// The list of uids changed (checked, unchecked or removed).
    pub changed: bool,
    /// A name to create and select (see [`create_and_select`]).
    pub create: Option<String>,
    /// "Gerenciar categorias…" was clicked.
    pub manage: bool,
}

impl PickerOutput {
    /// Serves a request to create: the error text, when the store refuses.
    pub fn finish(
        &mut self,
        store: &mut Store,
        selected: &mut Vec<String>,
        all: &[Category],
    ) -> Result<(), String> {
        if let Some(name) = self.create.take() {
            create_and_select(store, &name, selected, all)?;
            self.changed = true;
        }
        Ok(())
    }
}

/// The chips of the item (click one to take it off) and "+ Categoria",
/// which opens the list to check from, with a search box that also offers
/// to create what is typed.
pub fn category_picker(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    selected: &mut Vec<String>,
    all: &[Category],
) -> PickerOutput {
    let id = ui.make_persistent_id(("category-picker", id_salt));
    let query_id = id.with("query");
    let mut out = PickerOutput::default();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(S1 + 2.0, S1 + 2.0);
        let mut remove = None;
        for uid in selected.iter() {
            let Some(category) = all.iter().find(|c| &c.uid == uid) else {
                continue;
            };
            let (fg, bg, _) = colors_of(category);
            let response = chip_label(
                ui,
                format!("{} ×", shorten(&category.name, 24)),
                fg,
                bg,
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("{}: clique para tirar", category.name));
            if response.clicked() {
                remove = Some(uid.clone());
            }
        }
        if let Some(uid) = remove {
            selected.retain(|u| *u != uid);
            out.changed = true;
        }
        let button = ui.add(widgets::secondary("+ Categoria"));
        if button.clicked() {
            ui.data_mut(|d| d.insert_temp(query_id, String::new()));
        }
        egui::Popup::from_toggle_button_response(&button)
            .id(id.with("popup"))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(260.0);
                let mut query: String = ui.data(|d| d.get_temp(query_id)).unwrap_or_default();
                let first = ui.data(|d| d.get_temp::<bool>(id.with("seen"))).is_none();
                let search = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("Buscar ou criar categoria")
                        .desired_width(f32::INFINITY),
                );
                if first {
                    search.request_focus();
                    ui.data_mut(|d| d.insert_temp(id.with("seen"), true));
                }
                let offer = create_offer(&query, all);
                if search.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && can_add(selected)
                {
                    if let Some(name) = &offer {
                        out.create = Some(name.clone());
                        query.clear();
                    }
                }
                ui.add_space(S1);
                let at_limit = !can_add(selected);
                let rows = offered(all, &query);
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .id_salt(id.with("scroll"))
                    .show(ui, |ui| {
                        for category in &rows {
                            let mut on = selected.contains(&category.uid);
                            ui.horizontal(|ui| {
                                dot(ui, colors_of(category).2);
                                let was = on;
                                let check = ui.add_enabled(
                                    on || !at_limit,
                                    egui::Checkbox::new(&mut on, category.name.as_str()),
                                );
                                if check.changed() && on != was && toggle(selected, &category.uid) {
                                    out.changed = true;
                                }
                            });
                        }
                        if rows.is_empty() && offer.is_none() {
                            widgets::small_muted(
                                ui,
                                if all.iter().all(|c| c.archived) && query.trim().is_empty() {
                                    "Nenhuma categoria ainda. Digite um nome para criar."
                                } else {
                                    "Nenhuma categoria com esse nome."
                                },
                            );
                        }
                    });
                if let Some(name) = &offer {
                    ui.add_space(S1);
                    if ui
                        .add_enabled(
                            !at_limit,
                            widgets::quiet(format!("Criar “{}”", shorten(name, 30))),
                        )
                        .on_hover_text("Cria a categoria e coloca neste item.")
                        .clicked()
                    {
                        out.create = Some(name.clone());
                        query.clear();
                    }
                }
                if at_limit {
                    ui.add_space(S1);
                    widgets::small_muted(
                        ui,
                        format!("Limite de {MAX_PER_ITEM} categorias por item."),
                    );
                }
                ui.separator();
                if ui.add(widgets::quiet("Gerenciar categorias…")).clicked() {
                    out.manage = true;
                    ui.close();
                }
                ui.data_mut(|d| d.insert_temp(query_id, query));
            });
        if !egui::Popup::is_id_open(ui.ctx(), id.with("popup")) {
            ui.data_mut(|d| d.remove_temp::<bool>(id.with("seen")));
        }
    });
    out
}

/// The filter of a list: a chip "Categorias" that opens the list to check
/// from, with "Sem categoria". Returns true when the choice changed.
pub fn category_filter_button(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    filter: &mut CategoryFilter,
    all: &[Category],
) -> bool {
    let id = ui.make_persistent_id(("category-filter", id_salt));
    let mut changed = false;
    let count = filter.chosen.len() + usize::from(filter.none);
    let label = if count == 0 {
        format!("Categorias {}", icon::DOWN)
    } else {
        format!("Categorias · {count} {}", icon::DOWN)
    };
    let button = widgets::chip(ui, filter.is_active(), label)
        .on_hover_text("Mostra só o que tem alguma das categorias marcadas.");
    egui::Popup::from_toggle_button_response(&button)
        .id(id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(240.0);
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .id_salt(id.with("scroll"))
                .show(ui, |ui| {
                    let mut ordered: Vec<&Category> = all.iter().filter(|c| !c.archived).collect();
                    ordered.extend(all.iter().filter(|c| c.archived));
                    for category in ordered {
                        let mut on = filter.chosen.contains(&category.uid);
                        ui.horizontal(|ui| {
                            dot(ui, colors_of(category).2);
                            let text = if category.archived {
                                format!("{} (arquivada)", category.name)
                            } else {
                                category.name.clone()
                            };
                            if ui.checkbox(&mut on, text).changed() {
                                if on {
                                    filter.chosen.insert(category.uid.clone());
                                } else {
                                    filter.chosen.remove(&category.uid);
                                }
                                changed = true;
                            }
                        });
                    }
                    if all.is_empty() {
                        widgets::small_muted(ui, "Nenhuma categoria ainda.");
                    }
                });
            ui.separator();
            if ui.checkbox(&mut filter.none, "Sem categoria").changed() {
                changed = true;
            }
            if filter.is_active() && ui.add(widgets::quiet("Limpar")).clicked() {
                filter.clear();
                changed = true;
            }
        });
    changed
}

// ---- the manager (Ajustes) ----

/// What the manager is doing on one row.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RowMode {
    Rename(String),
    Color,
    Delete,
}

/// What the manager asks of the app after a frame.
#[derive(Debug, Default)]
pub struct ManagerOutput {
    /// A category was created, changed or deleted: the lists need a reload.
    pub changed: bool,
    /// A line for the area's notice: (text, is_error).
    pub notice: Option<(String, bool)>,
}

/// The "Categorias" section of Ajustes.
#[derive(Default)]
pub struct CategoryManager {
    new_name: String,
    new_color: Option<u8>,
    open: Option<(String, RowMode)>,
}

#[cfg(feature = "shots")]
impl CategoryManager {
    /// Development only: puts the manager in the state called `view`
    /// ("" the plain list, "apagar" the first row asking to be deleted).
    pub fn shots_open(&mut self, store: &Store, view: &str) {
        self.open = None;
        if view == "apagar" {
            let first = store
                .list_categories()
                .ok()
                .and_then(|all| all.into_iter().next());
            self.open = first.map(|c| (c.uid, RowMode::Delete));
        }
    }
}

fn count_text(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What deleting says it will do, in the words of the confirmation line.
pub fn delete_sentence(name: &str, tasks: usize, documents: usize) -> String {
    let what = match (tasks, documents) {
        (0, 0) => "Nenhuma tarefa nem documento usa esta categoria.".to_owned(),
        (t, 0) => format!("Ela sai de {}.", count_text(t, "tarefa", "tarefas")),
        (0, d) => format!("Ela sai de {}.", count_text(d, "documento", "documentos")),
        (t, d) => format!(
            "Ela sai de {} e de {}.",
            count_text(t, "tarefa", "tarefas"),
            count_text(d, "documento", "documentos")
        ),
    };
    format!("Apagar a categoria “{name}”? {what} As tarefas e os documentos continuam.")
}

impl CategoryManager {
    pub fn ui(&mut self, ui: &mut egui::Ui, store: &mut Store) -> ManagerOutput {
        let mut out = ManagerOutput::default();
        let all = match store.list_categories() {
            Ok(all) => all,
            Err(e) => {
                out.notice = Some((format!("Não foi possível ler as categorias: {e}"), true));
                return out;
            }
        };
        let usage = store.category_usage().unwrap_or_default();
        widgets::card(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new("Categorias são palavras que você escolhe para organizar as tarefas e os documentos, como “Urgente” ou “Aguardando resposta”. A mesma lista serve para os dois, e cada tarefa ou documento pode ter várias.")
                        .color(c().muted),
                )
                .wrap(),
            );
            ui.add_space(S1);
            ui.add(
                egui::Label::new(
                    RichText::new("Quando o acervo é compartilhado com uma equipe (pasta compartilhada ou servidor da equipe), a lista é a mesma para todos: o que for criado, renomeado, arquivado ou apagado aqui aparece para todos.")
                        .color(c().muted),
                )
                .wrap(),
            );
        });
        ui.add_space(S2);
        widgets::card(ui, |ui| {
            widgets::section(ui, "Nova categoria");
            ui.add_space(S1);
            let color = self.new_color.unwrap_or_else(|| next_color(&all));
            let mut create = false;
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.new_name)
                        .hint_text("Nome da categoria")
                        .char_limit(MAX_NAME)
                        .desired_width(240.0),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    create = true;
                }
                if ui
                    .add_enabled(!self.new_name.trim().is_empty(), widgets::primary("Criar"))
                    .clicked()
                {
                    create = true;
                }
            });
            ui.add_space(S1);
            if let Some(chosen) = swatches(ui, color) {
                self.new_color = Some(chosen);
            }
            if create {
                match store.save_category(&Category::new(&self.new_name, color)) {
                    Ok(saved) => {
                        out.changed = true;
                        out.notice = Some((format!("Categoria “{}” criada.", saved.name), false));
                        self.new_name.clear();
                        self.new_color = None;
                    }
                    Err(e) => out.notice = Some((e.to_string(), true)),
                }
            }
            if all.len() >= MAX_CATEGORIES {
                widgets::small_muted(
                    ui,
                    format!("A lista já tem {MAX_CATEGORIES} categorias, o máximo."),
                );
            }
        });
        ui.add_space(S2);
        let active: Vec<&Category> = all.iter().filter(|c| !c.archived).collect();
        let archived: Vec<&Category> = all.iter().filter(|c| c.archived).collect();
        widgets::card(ui, |ui| {
            widgets::section(ui, format!("Categorias ({})", active.len()));
            ui.add_space(S1);
            if active.is_empty() {
                widgets::muted(ui, "Nenhuma categoria ainda. Crie a primeira acima.");
            }
            for category in &active {
                self.row(ui, store, category, &usage, &mut out);
            }
        });
        if !archived.is_empty() {
            ui.add_space(S2);
            widgets::card(ui, |ui| {
                egui::CollapsingHeader::new(
                    RichText::new(format!("Arquivadas ({})", archived.len()))
                        .font(theme::font_strong(theme::SIZE_BODY)),
                )
                .id_salt("categories-archived")
                .default_open(false)
                .show(ui, |ui| {
                    widgets::small_muted(
                        ui,
                        "Uma categoria arquivada continua nas tarefas e nos documentos que já a têm, mas não é oferecida para novos.",
                    );
                    ui.add_space(S1);
                    for category in &archived {
                        self.row(ui, store, category, &usage, &mut out);
                    }
                });
            });
        }
        out
    }

    fn row(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        category: &Category,
        usage: &std::collections::HashMap<String, (usize, usize)>,
        out: &mut ManagerOutput,
    ) {
        let (tasks, documents) = usage.get(&category.uid).copied().unwrap_or((0, 0));
        let mode = self
            .open
            .as_ref()
            .filter(|(uid, _)| *uid == category.uid)
            .map(|(_, mode)| mode.clone());
        ui.push_id(&category.uid, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = S2;
                category_chip(ui, category);
                widgets::small_muted(
                    ui,
                    format!(
                        "{} · {}",
                        count_text(tasks, "tarefa", "tarefas"),
                        count_text(documents, "documento", "documentos")
                    ),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = S3;
                    if ui.add(widgets::quiet("Apagar de vez…")).clicked() {
                        self.open = Some((category.uid.clone(), RowMode::Delete));
                    }
                    let (label, hint) = if category.archived {
                        ("Desarquivar", "Volta a oferecer a categoria para novas tarefas e documentos.")
                    } else {
                        ("Arquivar", "Deixa de oferecer a categoria para novos; quem já tem continua com ela.")
                    };
                    if ui.add(widgets::quiet(label)).on_hover_text(hint).clicked() {
                        match store.set_category_archived(&category.uid, !category.archived) {
                            Ok(_) => {
                                out.changed = true;
                                out.notice = Some((
                                    format!(
                                        "Categoria “{}” {}.",
                                        category.name,
                                        if category.archived { "desarquivada" } else { "arquivada" }
                                    ),
                                    false,
                                ));
                            }
                            Err(e) => out.notice = Some((e.to_string(), true)),
                        }
                    }
                    if ui.add(widgets::quiet("Cor")).clicked() {
                        self.open = Some((category.uid.clone(), RowMode::Color));
                    }
                    if ui.add(widgets::quiet("Renomear")).clicked() {
                        self.open =
                            Some((category.uid.clone(), RowMode::Rename(category.name.clone())));
                    }
                });
            });
            match mode {
                Some(RowMode::Rename(mut text)) => {
                    let mut save = false;
                    let mut cancel = false;
                    ui.horizontal(|ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .char_limit(MAX_NAME)
                                .desired_width(240.0),
                        );
                        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            save = true;
                        }
                        save |= ui.add(widgets::primary("Salvar")).clicked();
                        cancel = ui.add(widgets::quiet("Cancelar")).clicked();
                    });
                    if save {
                        let mut changed = category.clone();
                        changed.name = text.clone();
                        match store.save_category(&changed) {
                            Ok(_) => {
                                out.changed = true;
                                out.notice = Some(("Nome da categoria alterado.".to_owned(), false));
                                self.open = None;
                            }
                            Err(e) => {
                                out.notice = Some((e.to_string(), true));
                                self.open = Some((category.uid.clone(), RowMode::Rename(text)));
                            }
                        }
                    } else if cancel {
                        self.open = None;
                    } else {
                        self.open = Some((category.uid.clone(), RowMode::Rename(text)));
                    }
                }
                Some(RowMode::Color) => {
                    if let Some(color) = swatches(ui, category.color) {
                        let mut changed = category.clone();
                        changed.color = color;
                        match store.save_category(&changed) {
                            Ok(_) => {
                                out.changed = true;
                                self.open = None;
                            }
                            Err(e) => out.notice = Some((e.to_string(), true)),
                        }
                    }
                }
                Some(RowMode::Delete) => {
                    let (fg, bg) = Tone::Danger.colors();
                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(CornerRadius::same(theme::RADIUS))
                        .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(delete_sentence(&category.name, tasks, documents))
                                        .color(fg),
                                )
                                .wrap(),
                            );
                            ui.horizontal(|ui| {
                                if ui.add(widgets::danger("Apagar de vez")).clicked() {
                                    match store.delete_category(&category.uid) {
                                        Ok(()) => {
                                            out.changed = true;
                                            out.notice = Some((
                                                format!("Categoria “{}” apagada.", category.name),
                                                false,
                                            ));
                                            self.open = None;
                                        }
                                        Err(e) => out.notice = Some((e.to_string(), true)),
                                    }
                                }
                                if ui.add(widgets::quiet("Cancelar")).clicked() {
                                    self.open = None;
                                }
                            });
                        });
                }
                None => {}
            }
            ui.add_space(S1);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(uid: &str, name: &str, color: u8, archived: bool) -> Category {
        Category {
            uid: uid.into(),
            name: name.into(),
            color,
            archived,
            ..Category::default()
        }
    }

    fn luminance(color: Color32) -> f32 {
        let f = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(color.r()) + 0.7152 * f(color.g()) + 0.0722 * f(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_palette_entry_reads_in_both_themes() {
        for dark in [false, true] {
            for index in 0..PALETTE_SIZE {
                let (fg, bg, _) = swatch_colors(index, dark);
                assert!(
                    contrast(fg, bg) >= 4.5,
                    "entry {index} (dark: {dark}) has too little contrast"
                );
            }
        }
    }

    #[test]
    fn the_filter_matches_any_chosen_or_none() {
        let mut filter = CategoryFilter::default();
        assert!(filter.matches(&[]), "no filter: everything");
        filter.chosen.insert("a".into());
        filter.chosen.insert("b".into());
        assert!(
            filter.matches(&["x".into(), "b".into()]),
            "ANY of the chosen"
        );
        assert!(!filter.matches(&["x".into()]));
        assert!(!filter.matches(&[]), "without category only when asked");
        filter.none = true;
        assert!(filter.matches(&[]));
        assert!(filter.matches(&["a".into()]));
        assert!(!filter.matches(&["x".into()]));
        filter.chosen.clear();
        assert!(filter.matches(&[]));
        assert!(!filter.matches(&["a".into()]));
        filter.clear();
        assert!(!filter.is_active());
    }

    #[test]
    fn an_item_takes_at_most_the_limit() {
        let mut selected: Vec<String> = (0..MAX_PER_ITEM).map(|i| format!("u{i}")).collect();
        assert!(!can_add(&selected));
        assert!(!toggle(&mut selected, "new"), "past the limit: refused");
        assert_eq!(selected.len(), MAX_PER_ITEM);
        assert!(toggle(&mut selected, "u3"), "unchecking is always allowed");
        assert!(can_add(&selected));
        assert!(toggle(&mut selected, "new"));
        assert!(selected.contains(&"new".to_string()));
    }

    #[test]
    fn creating_is_offered_only_when_no_name_matches() {
        let all = vec![cat("1", "Jurídico", 0, false), cat("2", "Antigo", 1, true)];
        assert_eq!(
            create_offer("  Novo   nome ", &all).as_deref(),
            Some("Novo nome")
        );
        assert_eq!(create_offer("juridico", &all), None, "case and accents");
        assert_eq!(create_offer("JURÍDICO", &all), None);
        assert_eq!(
            create_offer("antigo", &all),
            None,
            "archived ones count too"
        );
        assert_eq!(create_offer("   ", &all), None);
        assert_eq!(create_offer(&"x".repeat(MAX_NAME + 1), &all), None);
    }

    #[test]
    fn the_next_color_is_the_first_unused_then_the_least_used() {
        assert_eq!(next_color(&[]), 0);
        let all = vec![cat("1", "A", 0, false), cat("2", "B", 1, false)];
        assert_eq!(next_color(&all), 2);
        let mut full: Vec<Category> = (0..PALETTE_SIZE)
            .map(|i| cat(&format!("{i}"), &format!("C{i}"), i, false))
            .collect();
        full.push(cat("x", "Extra", 0, false));
        assert_eq!(next_color(&full), 1, "all taken: the least used, first");
    }

    #[test]
    fn the_picker_offers_only_live_categories_matching_the_text() {
        let all = vec![
            cat("1", "Jurídico", 0, false),
            cat("2", "Urgente", 1, false),
            cat("3", "Jurídico antigo", 2, true),
        ];
        let names =
            |q: &str| -> Vec<&str> { offered(&all, q).iter().map(|c| c.name.as_str()).collect() };
        assert_eq!(names(""), ["Jurídico", "Urgente"]);
        assert_eq!(names("jur"), ["Jurídico"]);
        assert_eq!(names("zzz"), Vec::<&str>::new());
    }

    #[test]
    fn creating_from_the_picker_saves_and_selects() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("c.sqlite3")).unwrap();
        let mut selected = Vec::new();
        let all = store.list_categories().unwrap();
        let created = create_and_select(&mut store, "Urgente", &mut selected, &all).unwrap();
        assert_eq!(selected, vec![created.uid.clone()]);
        assert_eq!(created.color, 0);
        let all = store.list_categories().unwrap();
        let error = create_and_select(&mut store, "urgente", &mut selected, &all).unwrap_err();
        assert!(!error.is_empty(), "the store's refusal is passed on");
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn the_delete_sentence_says_what_is_lost() {
        let text = delete_sentence("Urgente", 3, 1);
        assert!(text.contains("3 tarefas") && text.contains("1 documento"));
        assert!(delete_sentence("A", 0, 0).contains("Nenhuma tarefa nem documento"));
        assert!(delete_sentence("A", 1, 0).contains("1 tarefa."));
    }

    /// Runs the picker and the manager headless: nothing panics and the
    /// popup stays closed until clicked.
    #[test]
    fn the_controls_draw() {
        let ctx = egui::Context::default();
        let all = vec![cat("1", "Urgente", 0, false), cat("2", "Velha", 3, true)];
        let mut selected = vec!["1".to_owned(), "2".to_owned()];
        let mut filter = CategoryFilter::default();
        let mut manager = CategoryManager::default();
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("d.sqlite3")).unwrap();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let out = category_picker(ui, "t", &mut selected, &all);
            assert!(!out.changed && out.create.is_none() && !out.manage);
            category_chips(ui, &selected, &all);
            category_chips_compact(ui, &selected, &all, 1);
            category_filter_button(ui, "f", &mut filter, &all);
            let shown = manager.ui(ui, &mut store);
            assert!(!shown.changed);
        });
        output.textures_delta.clear();
    }
}
