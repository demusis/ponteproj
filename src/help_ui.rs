//! The "ⓘ" next to a title: a short explanation instead of a permanent
//! paragraph. Hovering shows it (the pointer turns into a hand, so it reads
//! as something to use); clicking keeps it open beside the icon until it is
//! clicked again, for whoever does not hover or reads slowly.
use eframe::egui::{self, CursorIcon, RichText, Sense};


pub fn help(ui: &mut egui::Ui, text: impl ToString) {
    let text = text.to_string();
    let response = ui
        .add(egui::Label::new(RichText::new("ⓘ").color(crate::theme::muted())).sense(Sense::click()))
        .on_hover_cursor(CursorIcon::PointingHand);
    let id = response.id.with("open");
    let mut open = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        ui.add(egui::Label::new(RichText::new(&text).small().color(crate::theme::muted())).wrap())
            .on_hover_text("Clique no ⓘ para fechar.");
    } else {
        response.on_hover_text(format!("{text}\n\n(Clique para deixar aberto.)"));
    }
}
