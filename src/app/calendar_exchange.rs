use super::PonteApp;
use crate::{calendar_ics, tasks::Task, widgets};
use eframe::egui;
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct CalendarExchange {
    include_titles: bool,
    pending: Vec<calendar_ics::ImportedEntry>,
    skipped: Vec<calendar_ics::SkippedEntry>,
    general_ignored: bool,
}

fn imported_uid(source: &str) -> String {
    if let Some(uid) = source.strip_suffix("@ponte") {
        if uuid::Uuid::parse_str(uid).is_ok() {
            return uid.to_owned();
        }
    }
    let digest = Sha256::digest(format!("ponte-calendar-import:{source}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Uuid::from_bytes(bytes).to_string()
}

impl PonteApp {
    pub(super) fn calendar_exchange(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, |ui| {
            widgets::section(ui, "Calendário (.ics)");
            widgets::muted(ui, "Exporta tarefas e blocos de trabalho separados, além das reservas ativas. O arquivo é uma cópia pontual para outro programa; alterações feitas lá não voltam sozinhas.");
            ui.checkbox(
                &mut self.calendar_exchange.include_titles,
                "Incluir títulos, finalidades e notas no arquivo exportado",
            );
            let export = ui.button("Exportar calendário…").clicked();
            let import = ui.button("Conferir arquivo de calendário…").clicked();
            if export {
                let result = (|| -> anyhow::Result<()> {
                    let text = calendar_ics::export_calendar(
                        &self.store.list_tasks()?,
                        &self.store.list_resource_uses()?,
                        &self.store.list_resources()?,
                        !self.calendar_exchange.include_titles,
                    )?;
                    if let Some(path) = crate::windows_ui::save_file(
                        "Exportar calendário",
                        "ponte-agenda.ics",
                        &[("Calendário", "*.ics")],
                    ) {
                        crate::atomic_file::write_atomic(&path, text.as_bytes())?;
                        self.message("Calendário exportado.");
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    self.error(error.to_string());
                }
            }
            if import {
                if let Some(path) =
                    crate::windows_ui::pick_file("Conferir calendário", &[("Calendário", "*.ics")])
                {
                    let result = (|| -> anyhow::Result<_> {
                        anyhow::ensure!(
                            std::fs::metadata(&path)?.len() <= 4_000_000,
                            "Máximo de 4 MB por calendário."
                        );
                        calendar_ics::parse_calendar(&std::fs::read_to_string(path)?)
                    })();
                    match result {
                        Ok(review) => {
                            self.calendar_exchange.pending = review.entries;
                            self.calendar_exchange.skipped = review.skipped;
                            self.calendar_exchange.general_ignored = review.general_ignored;
                        }
                        Err(error) => self.error(error.to_string()),
                    }
                }
            }
            widgets::small_muted(ui, "Eventos entram como tarefas; não reservam recursos. Confira os detalhes antes de importar.");
            if self.calendar_exchange.general_ignored {
                ui.label("informações gerais do calendário ignoradas");
            }
            if !self.calendar_exchange.skipped.is_empty() {
                ui.collapsing(
                    format!("Não importadas ({})", self.calendar_exchange.skipped.len()),
                    |ui| {
                        for entry in &self.calendar_exchange.skipped {
                            ui.label(format!("{}: {}", entry.title, entry.reason));
                        }
                    },
                );
            }
            let mut save = None;
            let mut save_all = false;
            if !self.calendar_exchange.pending.is_empty()
                && ui
                    .button(format!(
                        "Importar todas as {}",
                        self.calendar_exchange.pending.len()
                    ))
                    .clicked()
            {
                save_all = true;
            }
            for (index, entry) in self.calendar_exchange.pending.iter().enumerate() {
                ui.separator();
                ui.label(&entry.title);
                ui.label(format!(
                    "Prazo: {} · Início: {} · Fim: {}",
                    crate::labels::date(&entry.due),
                    entry.start,
                    entry.end
                ));
                if !entry.notes.is_empty() {
                    ui.collapsing("Notas", |ui| {
                        ui.label(&entry.notes);
                    });
                }
                if !entry.details.is_empty() {
                    ui.label(format!(
                        "Detalhes da importação: {}",
                        entry.details.join("; ")
                    ));
                }
                if ui.button("Importar esta entrada como tarefa").clicked() {
                    save = Some(index);
                }
            }
            if save_all || save.is_some() {
                let indexes: Vec<usize> = if save_all {
                    (0..self.calendar_exchange.pending.len()).collect()
                } else {
                    vec![save.unwrap()]
                };
                let mut created = 0;
                let mut existing = 0;
                let mut done = Vec::new();
                let mut known_uids: std::collections::HashSet<_> = match self.store.list_tasks() {
                    Ok(tasks) => tasks.into_iter().map(|task| task.uid).collect(),
                    Err(error) => {
                        self.error(error.to_string());
                        return;
                    }
                };
                for index in indexes {
                    let entry = self.calendar_exchange.pending[index].clone();
                    let result = (|| -> anyhow::Result<bool> {
                        let uid = imported_uid(&entry.uid);
                        if known_uids.contains(&uid) {
                            return Ok(false);
                        }
                        let mut task = Task::standalone();
                        task.uid = uid.clone();
                        task.title = entry.title;
                        task.notes = entry.notes;
                        task.due_date = entry.due;
                        task.scheduled_start = entry.start;
                        task.scheduled_end = entry.end;
                        if entry.completed {
                            task.status = "concluida".into();
                        }
                        self.store.save_task(&task)?;
                        known_uids.insert(uid);
                        Ok(true)
                    })();
                    match result {
                        Ok(was_created) => {
                            if was_created {
                                created += 1;
                            } else {
                                existing += 1;
                            }
                            done.push(index);
                        }
                        Err(error) => {
                            self.error(error.to_string());
                            break;
                        }
                    }
                }
                for index in done.into_iter().rev() {
                    self.calendar_exchange.pending.remove(index);
                }
                if created + existing > 0 {
                    self.refresh();
                    self.message(format!(
                        "{created} entrada(s) importada(s); {existing} já estava(m) no acervo."
                    ));
                }
            }
            if (!self.calendar_exchange.pending.is_empty()
                || !self.calendar_exchange.skipped.is_empty()
                || self.calendar_exchange.general_ignored)
                && ui
                    .button("Fechar conferência sem importar as restantes")
                    .clicked()
            {
                self.calendar_exchange.pending.clear();
                self.calendar_exchange.skipped.clear();
                self.calendar_exchange.general_ignored = false;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_identity_is_stable_and_exported_uuid_is_preserved() {
        assert_eq!(
            imported_uid("external@example.com"),
            imported_uid("external@example.com")
        );
        assert_ne!(imported_uid("a"), imported_uid("b"));
        let uid = uuid::Uuid::new_v4().to_string();
        assert_eq!(imported_uid(&format!("{uid}@ponte")), uid);
    }
}
