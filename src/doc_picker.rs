//! Choosing a document of the library by searching for it, wherever a screen
//! links something to a document. Nobody knows a document's number by heart:
//! the person types part of the number, of the subject or of the text and
//! picks from the list.
use crate::{model::DocumentSummary, storage::Store, theme, widgets};
use eframe::egui;
use std::time::{Duration, Instant};

/// Nothing is searched for fewer characters than this.
const MIN_CHARS: usize = 2;
/// Rows shown; the rest is counted in "e mais N".
const SHOWN: usize = 8;
/// The search waits for a pause in the typing.
const PAUSE: Duration = Duration::from_millis(200);

/// The first `shown` items and how many were left out.
fn limited<T>(mut all: Vec<T>, shown: usize) -> (Vec<T>, usize) {
    let more = all.len().saturating_sub(shown);
    all.truncate(shown);
    (all, more)
}

/// The highlighted row after an arrow key, going round at the ends.
fn moved(current: usize, total: usize, down: bool) -> usize {
    if total == 0 {
        0
    } else if down {
        (current + 1) % total
    } else {
        (current + total - 1) % total
    }
}

/// A search field with the matching documents listed under it.
#[derive(Default)]
pub struct DocumentPicker {
    query: String,
    /// The text the list on screen answers.
    searched: String,
    edited_at: Option<Instant>,
    results: Vec<DocumentSummary>,
    more: usize,
    error: String,
    highlight: usize,
}

impl DocumentPicker {
    /// Back to an empty field.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn run(&mut self, store: &Store) {
        self.edited_at = None;
        self.searched = self.query.clone();
        self.highlight = 0;
        self.error.clear();
        let text = self.query.trim();
        if text.chars().count() < MIN_CHARS {
            self.results.clear();
            self.more = 0;
            return;
        }
        match store.list_documents(text) {
            Ok(documents) => (self.results, self.more) = limited(documents, SHOWN),
            Err(error) => {
                self.results.clear();
                self.more = 0;
                self.error = format!("Não foi possível buscar no acervo: {error}");
            }
        }
    }

    /// Development only: the field already searched, for the pictures.
    #[cfg(feature = "shots")]
    pub fn shots_query(&mut self, store: &Store, text: &str) {
        self.query = text.to_owned();
        self.run(store);
    }

    /// Draws the field and the list; returns the document the person chose.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        hint: &str,
    ) -> Option<DocumentSummary> {
        let p = theme::c();
        let width = ui.available_width();
        let field = widgets::search(ui, &mut self.query, hint, width);
        if self.query != self.searched {
            let since = *self.edited_at.get_or_insert_with(Instant::now);
            if since.elapsed() >= PAUSE {
                self.run(store);
            } else {
                ui.ctx()
                    .request_repaint_after(PAUSE.saturating_sub(since.elapsed()));
            }
        }
        let mut chosen: Option<usize> = None;
        // Arrows and Enter act on the list while the field has the keyboard.
        if field.has_focus() || field.lost_focus() {
            let (up, down, enter) = ui.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowUp),
                    input.key_pressed(egui::Key::ArrowDown),
                    input.key_pressed(egui::Key::Enter),
                )
            });
            if up || down {
                self.highlight = moved(self.highlight, self.results.len(), down);
            }
            if enter {
                // Enter never picks from the list of an older text.
                if self.query != self.searched {
                    self.run(store);
                }
                if self.highlight < self.results.len() {
                    chosen = Some(self.highlight);
                }
            }
        }
        let typed = self.query.trim().chars().count();
        if !self.error.is_empty() {
            ui.label(egui::RichText::new(&self.error).color(p.danger));
        } else if typed >= MIN_CHARS && self.query == self.searched && self.results.is_empty() {
            widgets::small_muted(
                ui,
                format!("Nenhum documento encontrado para “{}”.", self.query.trim()),
            );
        } else if !self.results.is_empty() {
            egui::Frame::NONE
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.outline))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width((width - 10.0).max(60.0));
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (index, document) in self.results.iter().enumerate() {
                        let title = if document.title.trim().is_empty() {
                            "Sem assunto"
                        } else {
                            document.title.trim()
                        };
                        let mut job = egui::text::LayoutJob::default();
                        job.append(
                            &document.document_number,
                            0.0,
                            egui::TextFormat {
                                font_id: theme::font_strong(theme::SIZE_SMALL),
                                color: p.primary,
                                ..Default::default()
                            },
                        );
                        job.append(
                            title,
                            8.0,
                            egui::TextFormat {
                                font_id: egui::FontId::proportional(theme::SIZE_SMALL),
                                color: p.text,
                                ..Default::default()
                            },
                        );
                        // Full width, text at the left.
                        let row = ui
                            .with_layout(
                                egui::Layout::top_down_justified(egui::Align::LEFT),
                                |ui| {
                                    ui.add(
                                        egui::Button::selectable(index == self.highlight, job)
                                            .truncate()
                                            .min_size(egui::vec2(0.0, 26.0)),
                                    )
                                },
                            )
                            .inner
                            .on_hover_text(format!("{} — {title}", document.document_number));
                        if row.clicked() {
                            chosen = Some(index);
                        }
                    }
                    if self.more > 0 {
                        widgets::small_muted(
                            ui,
                            format!("e mais {}; escreva mais para afinar a busca.", self.more),
                        );
                    }
                });
        }
        let picked = chosen.and_then(|index| self.results.get(index).cloned());
        if picked.is_some() {
            self.clear();
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_cut_and_the_rest_counted() {
        assert_eq!(limited(vec![1, 2, 3], 8), (vec![1, 2, 3], 0));
        assert_eq!(limited((1..=11).collect::<Vec<_>>(), 8).1, 3);
        assert_eq!(limited(Vec::<i32>::new(), 8), (vec![], 0));
    }

    #[test]
    fn the_highlight_goes_round() {
        assert_eq!(moved(0, 3, true), 1);
        assert_eq!(moved(2, 3, true), 0);
        assert_eq!(moved(0, 3, false), 2);
        assert_eq!(moved(5, 0, true), 0);
    }

    #[test]
    fn searching_finds_by_number_and_subject_and_ignores_short_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let capture = crate::model::sample_capture();
        let number = capture.document_number.clone();
        store.import_capture(&capture).unwrap();
        let mut picker = DocumentPicker {
            query: number[..6].to_owned(),
            ..Default::default()
        };
        picker.run(&store);
        assert_eq!(picker.results.len(), 1);
        assert_eq!(picker.results[0].document_number, number);
        picker.query = "x".into();
        picker.run(&store);
        assert!(picker.results.is_empty());
        picker.query = "não existe nada assim".into();
        picker.run(&store);
        assert!(picker.results.is_empty() && picker.error.is_empty());
    }
}
