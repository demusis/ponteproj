//! The pieces every screen is built from: buttons by role, pills, cards,
//! tabs, chips, the search field, the floating notice and the items of the
//! area rail. Colours and sizes come from `theme`.
use crate::theme::{self, c};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Margin, Response, RichText, Sense, Stroke, StrokeKind, Ui,
    WidgetText,
};

/// What a pill or a status says, by colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Info,
    Ok,
    Warn,
    Danger,
    Violet,
    Primary,
}

impl Tone {
    /// (text, background)
    pub fn colors(self) -> (Color32, Color32) {
        let p = c();
        match self {
            Tone::Neutral => (p.muted, p.sunken),
            Tone::Info => (p.info, p.info_soft),
            Tone::Ok => (p.ok, p.ok_soft),
            Tone::Warn => (p.warn, p.warn_soft),
            Tone::Danger => (p.danger, p.danger_soft),
            Tone::Violet => (p.violet, p.violet_soft),
            Tone::Primary => (p.primary, p.primary_soft),
        }
    }
}

/// A small rounded label: situation, deadline, kind.
pub fn pill(ui: &mut Ui, text: impl Into<String>, tone: Tone) -> Response {
    let (fg, bg) = tone.colors();
    pill_colored(ui, text, fg, bg)
}

pub fn pill_colored(ui: &mut Ui, text: impl Into<String>, fg: Color32, bg: Color32) -> Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(text.into())
                        .font(theme::font_strong(12.0))
                        .color(fg),
                )
                .selectable(false),
            );
        })
        .response
}

/// The one main action of a screen or panel.
pub fn primary(text: impl Into<String>) -> egui::Button<'static> {
    let p = c();
    egui::Button::new(
        RichText::new(text.into())
            .font(theme::font_strong(theme::SIZE_BODY))
            .color(p.on_primary),
    )
    .fill(p.primary)
    .stroke(Stroke::NONE)
}

/// An ordinary action: outlined, same weight as the text.
pub fn secondary(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).font(theme::font_strong(theme::SIZE_BODY)))
}

/// A discreet action inside a card or a row: text only, in the accent colour.
pub fn quiet(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.into())
            .font(theme::font_strong(theme::SIZE_BODY))
            .color(c().primary),
    )
    .frame(false)
}

/// Deleting and other actions that are hard to undo: red text, no frame.
pub fn danger(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.into())
            .font(theme::font_strong(theme::SIZE_BODY))
            .color(c().danger),
    )
    .frame(false)
}

/// A button that is only an icon; `hint` says what it does on hover.
pub fn icon_button(ui: &mut Ui, icon: &str, hint: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(icon).size(18.0).color(c().muted))
            .frame(false)
            .min_size(egui::vec2(30.0, 30.0)),
    )
    .on_hover_text(hint)
}

/// A white box with an outline and room inside, as wide as the place it is in.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn card_frame() -> egui::Frame {
    let p = c();
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS_CARD))
        .inner_margin(Margin::same(16))
}

/// The big title of a document or of a panel.
pub fn title(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(
        egui::Label::new(RichText::new(text.into()).font(theme::font_strong(theme::SIZE_TITLE)))
            .wrap(),
    )
}

/// The name of an area, at the top of it.
pub fn heading(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).font(theme::font_strong(theme::SIZE_HEADING)))
}

/// The title of a card or of a block inside a screen.
pub fn section(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).font(theme::font_strong(theme::SIZE_SECTION)))
}

/// Small capitals over a group of rows ("ATRASADAS", "PARA HOJE").
pub fn overline(ui: &mut Ui, text: &str, color: Option<Color32>) -> Response {
    ui.label(
        RichText::new(text.to_uppercase())
            .font(theme::font_strong(12.0))
            .color(color.unwrap_or(c().muted)),
    )
}

/// The name of a field, above it.
pub fn field_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(
        RichText::new(text)
            .font(theme::font_strong(theme::SIZE_SMALL))
            .color(c().muted),
    )
}

pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).color(c().muted))
}

pub fn small_muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(
        RichText::new(text.into())
            .size(theme::SIZE_SMALL)
            .color(c().muted),
    )
}

/// One tab of an underlined row. Put the tabs inside `ui.horizontal`.
pub fn tab(ui: &mut Ui, selected: bool, label: impl Into<WidgetText>, dirty: bool) -> Response {
    let p = c();
    let color = if selected { p.primary } else { p.muted };
    let font = if selected {
        theme::font_strong(theme::SIZE_BODY)
    } else {
        theme::font(theme::SIZE_BODY)
    };
    let text: WidgetText = label.into();
    let galley = ui
        .painter()
        .layout_no_wrap(text.text().to_owned(), font, color);
    let dot = if dirty { 12.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + dot, 34.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() && !selected {
            p.text
        } else {
            color
        };
        let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0 - 1.0);
        ui.painter().galley_with_override_text_color(pos, galley, color);
        if dirty {
            ui.painter().circle_filled(
                egui::pos2(rect.right() - 4.0, rect.center().y - 1.0),
                3.5,
                p.warn,
            );
        }
        if selected {
            let line = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - 3.0),
                rect.right_bottom(),
            );
            ui.painter().rect_filled(line, 1.5, p.primary);
        }
    }
    response
}

/// A row of underlined tabs with the line under them.
pub fn tab_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::S5;
        add(ui);
    });
    let rect = ui.max_rect();
    let y = ui.cursor().top() - ui.spacing().item_spacing.y;
    ui.painter()
        .hline(rect.x_range(), y, Stroke::new(1.0, c().outline));
}

/// A filter choice: filled when chosen.
pub fn chip(ui: &mut Ui, selected: bool, label: impl Into<String>) -> Response {
    let p = c();
    let label = label.into();
    let button = if selected {
        egui::Button::new(
            RichText::new(label)
                .font(theme::font_strong(13.0))
                .color(p.on_primary),
        )
        .fill(p.primary)
        .stroke(Stroke::NONE)
    } else {
        egui::Button::new(RichText::new(label).size(13.0))
    };
    ui.add(
        button
            .corner_radius(CornerRadius::same(14))
            .min_size(egui::vec2(0.0, 28.0)),
    )
}

/// The search box of an area, with the magnifying glass.
pub fn search(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let p = c();
    // A box of fixed size, whatever the layout around it: glass on the left,
    // text centred on the line, the clear button at the end.
    let width = width.max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(theme::RADIUS),
        p.surface,
        Stroke::new(1.0, p.outline),
        StrokeKind::Inside,
    );
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(9.0, 1.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    inner.spacing_mut().item_spacing.x = 6.0;
    inner.label(RichText::new(theme::icon::SEARCH).size(16.0).color(p.muted));
    let clear = if text.is_empty() { 0.0 } else { 26.0 };
    let mut response = inner.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .frame(egui::Frame::NONE)
            .desired_width((width - 48.0 - clear).max(40.0)),
    );
    if !text.is_empty()
        && inner
            .add(
                egui::Button::new(RichText::new(theme::icon::CLOSE).size(13.0).color(p.muted))
                    .frame(false),
            )
            .on_hover_text("Limpar a busca")
            .clicked()
    {
        text.clear();
        response.mark_changed();
    }
    response
}

/// One area in the rail on the left: icon over name. `badge` is a count on
/// the icon (0 hides it); `dirty` shows the amber dot of unsaved changes.
pub fn rail_item(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    selected: bool,
    badge: usize,
    dirty: bool,
) -> Response {
    let p = c();
    let size = egui::vec2(72.0, 56.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected || response.hovered() {
            let fill = if selected {
                p.rail_selected
            } else {
                p.rail_selected.gamma_multiply(0.5)
            };
            painter.rect_filled(rect, 8.0, fill);
        }
        let color = if selected { p.rail_on } else { p.rail_text };
        painter.text(
            egui::pos2(rect.center().x, rect.top() + 19.0),
            Align2::CENTER_CENTER,
            icon,
            theme::font(22.0),
            color,
        );
        let font = if selected {
            theme::font_strong(11.5)
        } else {
            theme::font(11.5)
        };
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 13.0),
            Align2::CENTER_CENTER,
            label,
            font,
            color,
        );
        if badge > 0 {
            let text = if badge > 99 {
                "99+".to_owned()
            } else {
                badge.to_string()
            };
            let galley = painter.layout_no_wrap(text, theme::font_strong(10.5), Color32::WHITE);
            let w = (galley.size().x + 8.0).max(17.0);
            let center = egui::pos2(rect.center().x + 15.0, rect.top() + 11.0);
            let pill = egui::Rect::from_center_size(center, egui::vec2(w, 17.0));
            painter.rect_filled(pill, 8.5, Color32::from_rgb(200, 52, 58));
            painter.galley(
                egui::pos2(
                    pill.center().x - galley.size().x / 2.0,
                    pill.center().y - galley.size().y / 2.0,
                ),
                galley,
                Color32::WHITE,
            );
        } else if dirty {
            painter.circle_filled(
                egui::pos2(rect.center().x + 15.0, rect.top() + 10.0),
                4.5,
                Color32::from_rgb(240, 170, 60),
            );
        }
    }
    response
}

/// What the person did with a floating notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastAction {
    None,
    Close,
    Action,
}

/// The floating notice in the bottom-right corner. Errors are red and show a
/// close button; `action` adds one text button ("Desfazer").
pub fn toast(ctx: &egui::Context, text: &str, error: bool, action: Option<&str>) -> ToastAction {
    let p = c();
    let mut result = ToastAction::None;
    let (fill, fg) = if error {
        (p.danger_soft, p.danger)
    } else {
        (p.toast, p.toast_text)
    };
    egui::Area::new(egui::Id::unique("ponte-toast"))
        .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -46.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(fill)
                .stroke(if error {
                    Stroke::new(1.0, p.danger)
                } else {
                    Stroke::NONE
                })
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(14, 10))
                .shadow(ui.visuals().popup_shadow)
                .show(ui, |ui| {
                    ui.set_max_width(460.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        if error {
                            ui.label(RichText::new(theme::icon::ERROR).size(18.0).color(fg));
                        }
                        ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                        if let Some(action) = action {
                            let button = egui::Button::new(
                                RichText::new(action)
                                    .font(theme::font_strong(theme::SIZE_BODY))
                                    .color(p.toast_action),
                            )
                            .frame(false);
                            if ui.add(button).clicked() {
                                result = ToastAction::Action;
                            }
                        }
                        let close = egui::Button::new(
                            RichText::new(theme::icon::CLOSE).size(14.0).color(fg),
                        )
                        .frame(false);
                        if ui.add(close).on_hover_text("Fechar").clicked() {
                            result = ToastAction::Close;
                        }
                    });
                });
        });
    result
}

/// A thin outline around a rectangle (selected option, focus).
pub fn outline(ui: &Ui, rect: egui::Rect, color: Color32) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS),
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
}
