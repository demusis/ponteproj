//! The info icon next to a title: a short explanation instead of a permanent
//! paragraph. Hovering shows it (the pointer turns into a hand, so it reads
//! as something to use); clicking keeps it open beside the icon until it is
//! clicked again, for whoever does not hover or reads slowly.
use crate::theme::{self, icon};
use eframe::egui::{self, CursorIcon, RichText, Sense};

pub fn help(ui: &mut egui::Ui, text: impl ToString) {
    let text = text.to_string();
    let response = ui
        .add(
            egui::Label::new(
                RichText::new(icon::INFO)
                    .size(16.0)
                    .color(theme::c().muted),
            )
            .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    let id = response.id.with("open");
    let mut open = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        egui::Frame::new()
            .fill(theme::c().sunken)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&text)
                            .size(theme::SIZE_SMALL)
                            .color(theme::c().muted),
                    )
                    .wrap(),
                )
                .on_hover_text("Clique no ícone de informação para fechar.");
            });
    } else {
        response.on_hover_text(format!("{text}\n\n(Clique para deixar aberto.)"));
    }
}
