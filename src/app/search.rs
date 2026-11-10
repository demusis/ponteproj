//! Search across the whole library — documents, tasks and atendimentos — in
//! one box (Ctrl+K), over whatever area is on screen.
use super::PonteApp;
use eframe::egui;

/// What the search box holds between frames.
#[derive(Default)]
pub(super) struct GlobalSearch {
    /// The box is on screen.
    pub open: bool,
}

impl PonteApp {
    /// Draws the search box when it is open. Called on every frame.
    pub(super) fn global_search(&mut self, _ctx: &egui::Context) {}
}
