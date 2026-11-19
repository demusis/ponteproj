//! The settings area: sections, AI configuration and the ChatGPT sign-in.
use super::{pill, save_appearance, Area, ChatGptLogin, Leave, NetTest, PonteApp, SettingsSection};
use crate::ai::{self, AiProvider, ModelInfo};
use crate::chatgpt;
use crate::theme::{self, icon, S1, S2, S3};
use crate::widgets::{self, Tone};
use chrono::Datelike;
use eframe::egui::{self, RichText};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// One line of "Outros dias sem expediente" while it is being edited.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DayRow {
    /// ISO date (or the raw text while it is not a valid date).
    date: String,
    /// Every year: only the day and the month count.
    yearly: bool,
    name: String,
}

/// The state and the extra days off as shown in Ajustes. What is complete
/// (a state, valid dates) goes to the library, to the local copy
/// (`feriados.txt`) and to the planning.
#[derive(Default)]
pub(super) struct HolidayDraft {
    uf: Option<&'static str>,
    rows: Vec<DayRow>,
    /// What the draft showed when it was last taken from the library or
    /// saved to it: the person is editing while the draft differs from this.
    base_uf: Option<&'static str>,
    base_rows: Vec<DayRow>,
    /// The text that could not be saved last time: it is not tried again
    /// until it changes or the section is left (not at every frame).
    failed: Option<crate::holidays::HolidayConfig>,
}

fn row_is_complete(row: &DayRow) -> bool {
    chrono::NaiveDate::parse_from_str(&row.date, "%Y-%m-%d").is_ok()
}

impl HolidayDraft {
    /// Reads `feriados.txt` and makes it the configuration in use (until the
    /// library, once open, says its own).
    pub(super) fn load() -> Self {
        // Tests build the app too: they must neither read this machine's
        // file nor change the configuration other tests are using.
        #[cfg(test)]
        let config = crate::holidays::config();
        #[cfg(not(test))]
        let config = {
            let config = crate::holidays::HolidayConfig::load();
            crate::holidays::set_config(config.clone());
            config
        };
        Self::from_config(&config)
    }

    /// The draft that shows `config`, with nothing being edited.
    pub(super) fn from_config(config: &crate::holidays::HolidayConfig) -> Self {
        let year = chrono::Local::now().year();
        let rows: Vec<DayRow> = config
            .custom
            .iter()
            .map(|day| {
                let shown_year = match day.year {
                    Some(year) => year,
                    // 29/02 needs a leap year to be a valid date.
                    None if chrono::NaiveDate::from_ymd_opt(year, day.month, day.day).is_some() => {
                        year
                    }
                    None => 2028,
                };
                DayRow {
                    date: format!("{shown_year:04}-{:02}-{:02}", day.month, day.day),
                    yearly: day.year.is_none(),
                    name: day.name.clone(),
                }
            })
            .collect();
        Self {
            uf: config.uf,
            base_uf: config.uf,
            base_rows: rows.clone(),
            rows,
            failed: None,
        }
    }

    /// Whether the person changed something that was not saved yet (a row
    /// still without a valid date counts: it must not be thrown away).
    pub(super) fn editing(&self) -> bool {
        self.uf != self.base_uf || self.rows != self.base_rows
    }

    /// Marks what is shown now as saved. A row still without a valid date
    /// was not saved (it is not part of the configuration): it stays
    /// unsaved, so a refresh from the library never erases it.
    fn mark_saved(&mut self) {
        self.base_uf = self.uf;
        self.base_rows = self
            .rows
            .iter()
            .filter(|row| row_is_complete(row))
            .cloned()
            .collect();
        self.failed = None;
    }

    /// What goes to the planning: rows with a valid date only.
    fn config(&self) -> crate::holidays::HolidayConfig {
        let custom = self
            .rows
            .iter()
            .filter_map(|row| {
                let date = chrono::NaiveDate::parse_from_str(&row.date, "%Y-%m-%d").ok()?;
                Some(crate::holidays::CustomDay {
                    year: (!row.yearly).then_some(date.year()),
                    month: date.month(),
                    day: date.day(),
                    name: row.name.replace(['\n', '\r'], " ").trim().to_owned(),
                })
            })
            .collect();
        crate::holidays::HolidayConfig {
            uf: self.uf,
            custom,
        }
    }
}

/// Whether this computer already handed its own holidays over to a library
/// (a small file next to `feriados.txt`). Tests never touch the real file.
pub(super) fn holiday_seed_marker_read() -> bool {
    #[cfg(not(test))]
    {
        crate::paths::data_dir()
            .map(|dir| dir.join("feriados.semeado").exists())
            .unwrap_or(false)
    }
    #[cfg(test)]
    false
}

fn holiday_seed_marker_write() {
    #[cfg(not(test))]
    if let Ok(dir) = crate::paths::data_dir() {
        let _ = std::fs::write(
            dir.join("feriados.semeado"),
            "1
",
        );
    }
}

/// Keeps `feriados.txt` as a copy of what is in force. Tests never touch the
/// real file of this machine.
fn save_holiday_cache(config: &crate::holidays::HolidayConfig) {
    #[cfg(not(test))]
    config.save();
    #[cfg(test)]
    let _ = config;
}

impl PonteApp {
    /// One block of the Ajustes area: what it is for, and its buttons.
    fn settings_card(ui: &mut egui::Ui, text: &str, add: impl FnOnce(&mut egui::Ui)) {
        widgets::card(ui, |ui| {
            if !text.is_empty() {
                ui.add(egui::Label::new(RichText::new(text).color(theme::c().muted)).wrap());
                ui.add_space(S1);
            }
            ui.horizontal_wrapped(add);
        });
    }

    /// The reminder of deadlines (a Windows notice once a day): on/off, the
    /// time, and a test notice.
    fn reminders_card(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, |ui| {
            widgets::section(ui, "Lembretes de prazos");
            let mut on = self.reminders.enabled();
            if ui
                .checkbox(&mut on, "Ativar resumo diário e avisos das tarefas")
                .changed()
            {
                self.reminders.set_enabled(on);
            }
            ui.add_enabled_ui(on, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Horário do resumo diário");
                    let (mut hour, mut minute) = self.reminders.time();
                    egui::ComboBox::from_id_salt("reminder-hour")
                        .width(56.0)
                        .selected_text(format!("{hour:02}"))
                        .show_ui(ui, |ui| {
                            for h in 0..24u32 {
                                ui.selectable_value(&mut hour, h, format!("{h:02}"));
                            }
                        });
                    ui.label(":");
                    egui::ComboBox::from_id_salt("reminder-minute")
                        .width(56.0)
                        .selected_text(format!("{minute:02}"))
                        .show_ui(ui, |ui| {
                            for m in (0..60u32).step_by(5) {
                                ui.selectable_value(&mut minute, m, format!("{m:02}"));
                            }
                        });
                    self.reminders.set_time(hour, minute);
                });
            });
            widgets::small_muted(
                ui,
                "O Ponte mostra um aviso do Windows com a quantidade de tarefas e atendimentos que vencem, de reservas de recursos para hoje e de devoluções atrasadas. Só enquanto o Ponte está aberto ou minimizado.",
            );
            widgets::small_muted(ui, "Os avisos individuais são escolhidos na tarefa. Contam a partir do início agendado ou, sem agendamento, das 9h do dia do prazo. Em Hoje, você pode abrir a tarefa, dispensar ou adiar o aviso por dez minutos.");
            ui.add_space(S1);
            if ui
                .add(widgets::quiet("Mostrar um aviso de teste"))
                .clicked()
            {
                self.reminders.test_notification();
            }
            widgets::small_muted(
                ui,
                "Se nada aparecer, veja Configurações do Windows ▸ Sistema ▸ Notificações.",
            );
        });
    }

    /// The state and the extra days off. They belong to the library: the same
    /// on every computer and phone that syncs it.
    fn holidays_card(&mut self, ui: &mut egui::Ui) {
        let mut remove: Option<usize> = None;
        widgets::card(ui, |ui| {
            widgets::section(ui, "Feriados");
            widgets::small_muted(
                ui,
                "Os feriados nacionais já contam. Estaduais e os dias que você cadastrar valem para todos os aparelhos que sincronizam este acervo.",
            );
            ui.add_space(S1);
            ui.horizontal(|ui| {
                ui.label("Estado");
                let shown = self
                    .holiday_draft
                    .uf
                    .and_then(crate::holidays::state_name)
                    .unwrap_or("(nenhum)");
                egui::ComboBox::from_id_salt("holiday-state")
                    .width(220.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.holiday_draft.uf, None, "(nenhum)");
                        for (code, full) in crate::holidays::STATES {
                            ui.selectable_value(&mut self.holiday_draft.uf, Some(code), full);
                        }
                    });
            });
            if let Some(uf) = self.holiday_draft.uf {
                let lines = crate::holidays::state_holiday_lines(uf);
                if lines.is_empty() {
                    widgets::small_muted(
                        ui,
                        "Este estado não tem feriado estadual além dos nacionais.",
                    );
                }
                for (when, name) in lines {
                    widgets::small_muted(ui, format!("{when}  {name}"));
                }
            }
            ui.add_space(S2);
            widgets::section(ui, "Outros dias sem expediente");
            widgets::small_muted(
                ui,
                "Recesso, ponto facultativo, feriado da cidade. Com “Todo ano”, só o dia e o mês contam.",
            );
            for (i, row) in self.holiday_draft.rows.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    crate::labels::date_field(ui, ("holiday-date", i), &mut row.date);
                    ui.checkbox(&mut row.yearly, "Todo ano");
                    ui.add(
                        egui::TextEdit::singleline(&mut row.name)
                            .hint_text("Nome")
                            .desired_width(200.0),
                    );
                    if widgets::icon_button(ui, icon::TRASH, "Remover este dia").clicked() {
                        remove = Some(i);
                    }
                });
            }
            ui.add_space(S1);
            if ui
                .add(widgets::quiet(format!("{} Adicionar um dia", icon::PLUS)))
                .clicked()
            {
                self.holiday_draft.rows.push(DayRow::default());
            }
        });
        if let Some(i) = remove {
            self.holiday_draft.rows.remove(i);
        }
        // Applied once nothing is being typed, so that a name is not
        // reprocessed at every letter.
        if !ui.ctx().egui_wants_keyboard_input() {
            self.apply_holiday_draft_with(false);
        }
    }

    /// Saves and uses the edited state and days, when they changed. Also
    /// called when the section is left or the app closes, so that a field
    /// still focused at that moment is not lost. Only what the person edited
    /// is written to the library: a draft that merely shows an older value
    /// never overwrites what another computer saved.
    pub(super) fn apply_holiday_draft(&mut self) {
        self.apply_holiday_draft_with(true);
    }

    /// `retry`: also try again a text whose save failed before (leaving the
    /// section, closing the app). While the person is in the section, the
    /// same failing text is not tried at every frame.
    fn apply_holiday_draft_with(&mut self, retry: bool) {
        if !self.holiday_draft.editing() {
            return;
        }
        let wanted = self.holiday_draft.config();
        if wanted == crate::holidays::config() {
            // Nothing to write; an incomplete row stays as being edited.
            self.holiday_draft.mark_saved();
            return;
        }
        if !retry && self.holiday_draft.failed.as_ref() == Some(&wanted) {
            return;
        }
        if let Err(error) = crate::holidays::save_to_library(&mut self.store, &wanted) {
            // Not saved: what the person typed stays in the draft (and is
            // never replaced by a refresh) until it can be saved.
            self.holiday_draft.failed = Some(wanted);
            self.error(format!(
                "Não foi possível salvar os feriados no acervo: {error}"
            ));
            return;
        }
        save_holiday_cache(&wanted);
        crate::holidays::set_config(wanted);
        self.holiday_draft.mark_saved();
        // Projections, Semana and Calendário are rebuilt from the library.
        self.refresh();
    }

    /// Brings the holidays of the library to this computer: after the library
    /// opens and at every refresh, so also after each synchronization. The
    /// library wins; when it has none yet, what this computer kept goes into
    /// it. Returns whether the configuration in use changed.
    pub(super) fn sync_holidays_with_library(&mut self) -> bool {
        let local = crate::holidays::config();
        let library = match crate::holidays::library_config(&self.store) {
            Ok(library) => library,
            Err(_) => return false,
        };
        let Some(wanted) = library else {
            // Nobody set it yet: this computer's own is handed over as a
            // seed, which only fills a library with no value (here and on
            // every copy it reaches), tried once per state of the library (a
            // main computer that cannot store it is not asked every few
            // seconds). Joining a folder or a main computer that has none
            // drops it from this copy: it is handed over again here.
            if !self.holiday_seed_done && local != crate::holidays::HolidayConfig::new() {
                let revision = self.store.cabinet_revision().ok();
                if self.holiday_migration_revision != Some(revision) {
                    self.holiday_migration_revision = Some(revision);
                    let _ = crate::holidays::seed_library(&mut self.store, &local);
                }
            }
            return false;
        };
        // The library has a value: the hand-over, if any, is over for good.
        if !self.holiday_seed_done {
            self.holiday_seed_done = true;
            holiday_seed_marker_write();
        }
        if wanted == local {
            return false;
        }
        save_holiday_cache(&wanted);
        crate::holidays::set_config(wanted.clone());
        // Another computer changed it: the draft shows it too, unless the
        // person is in the middle of an edit (their save then wins).
        if !self.holiday_draft.editing() {
            self.holiday_draft = HolidayDraft::from_config(&wanted);
        }
        true
    }

    /// One line of the list of sections, on the left of Ajustes.
    fn settings_nav_item(ui: &mut egui::Ui, glyph: &str, label: &str, selected: bool) -> bool {
        let p = theme::c();
        let size = egui::vec2(ui.available_width(), 36.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if ui.is_rect_visible(rect) {
            if selected {
                ui.painter().rect_filled(rect, 6.0, p.primary_soft);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 6.0, p.sunken);
            }
            let color = if selected { p.primary } else { p.text };
            ui.painter().text(
                egui::pos2(rect.left() + 12.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                glyph,
                theme::font(18.0),
                if selected { p.primary } else { p.muted },
            );
            ui.painter().text(
                egui::pos2(rect.left() + 40.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                if selected {
                    theme::font_strong(theme::SIZE_BODY)
                } else {
                    theme::font(theme::SIZE_BODY)
                },
                color,
            );
        }
        response.clicked()
    }

    /// Goes to a section of Ajustes. Leaving the AI section saves it first;
    /// an invalid AI configuration keeps the person there.
    fn open_settings(&mut self, section: SettingsSection) {
        let leaving_ai = self.area == Area::Settings
            && self.settings_section == SettingsSection::Ai
            && section != SettingsSection::Ai;
        if leaving_ai && !self.leave_ai_settings() {
            return;
        }
        if self.settings_section == SettingsSection::Holidays {
            self.apply_holiday_draft();
        }
        self.settings_section = section;
        if self.area != Area::Settings {
            self.navigate(Leave::Settings);
        }
    }

    /// Other parts of the app still ask for the old windows by raising their
    /// flags (status bar, "Ir para", the tour): each request becomes a visit
    /// to the section of Ajustes that now holds that content.
    pub(super) fn route_settings_requests(&mut self) {
        // "Abrir Equipe e sincronização" in the note of the Modelos list.
        if self.templates.take_open_team() {
            self.intranet.open = true;
        }
        if self.intranet.open {
            self.intranet.open = false;
            self.open_settings(SettingsSection::Team);
        }
        // "Abrir a Lixeira" on a conflict whose item was deleted.
        if self.intranet.take_open_trash() {
            self.trash.open = true;
        }
        if self.trash.open {
            self.trash.open = false;
            self.open_settings(SettingsSection::Trash);
        }
        // "Gerenciar categorias…" in the picker of a task or a document.
        if self.tasks.take_open_categories() | self.work_context.take_manage_categories() {
            self.open_settings(SettingsSection::Categories);
        }
        if self.settings_open {
            self.settings_open = false;
            self.open_settings(SettingsSection::Ai);
        }
    }

    /// The Ajustes area: what used to be scattered in the "Menu".
    pub(super) fn settings_area(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::c();
        egui::Panel::left("settings-nav")
            .exact_size(264.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(10, 14)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    widgets::heading(ui, "Ajustes");
                });
                ui.add_space(S2);
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut chosen = None;
                for section in SettingsSection::ALL {
                    if Self::settings_nav_item(
                        ui,
                        section.icon(),
                        section.label(),
                        self.settings_section == section,
                    ) {
                        chosen = Some(section);
                    }
                }
                if let Some(section) = chosen {
                    self.open_settings(section);
                }
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ui, |ui| {
                let section = self.settings_section;
                widgets::heading(ui, section.label());
                widgets::muted(ui, section.summary());
                ui.add_space(S2);
                let area = egui::ScrollArea::vertical()
                    .id_salt(("settings-section", section.label()))
                    .auto_shrink([false, false]);
                // Development only: the pictures scroll to the block they show.
                #[cfg(feature = "shots")]
                let area = if self.shots_ficha_scroll > 0.0 {
                    area.vertical_scroll_offset(self.shots_ficha_scroll)
                } else {
                    area
                };
                area.show(ui, |ui| {
                    // The section follows the window; the room of the
                    // scroll bar stays free.
                    ui.set_max_width((ui.available_width() - 14.0).max(240.0));
                    self.settings_section_body(ui, &ctx, section);
                    ui.add_space(S3);
                });
            });
    }

    fn settings_section_body(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        section: SettingsSection,
    ) {
        use crate::widgets::{primary, secondary};
        match section {
            SettingsSection::Team => {
                if self.intranet.ui(ui, &mut self.store, &self.db_path) {
                    self.refresh();
                }
            }
            SettingsSection::Mail => {
                // The panel says what is running: its own exchange with the
                // mailbox, or something else to wait for. Its requests go
                // out through `mail_window` (see `MailPanel::ui_embedded`).
                let busy = match &self.worker {
                    None => crate::mail_ui::MailBusy::Free,
                    Some(worker) if worker.kind == super::WorkKind::Mail => {
                        crate::mail_ui::MailBusy::Mail
                    }
                    Some(_) => crate::mail_ui::MailBusy::Other,
                };
                widgets::card(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new("Liste as mensagens de uma pasta da sua conta e importe as que marcar, com os anexos, como registros do tipo E-mail recebido. O Ponte só lê a caixa: nada é marcado como lido, movido ou apagado. No Gmail, use uma senha de app.")
                                .color(theme::c().muted),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(S2);
                if ui
                    .add(secondary("Importar um e-mail salvo em arquivo (.eml)…"))
                    .on_hover_text("Um e-mail salvo como arquivo vira um registro do tipo E-mail recebido, com os anexos.")
                    .clicked()
                {
                    self.import_eml_dialog();
                }
                ui.add_space(S3);
                self.mail.ui_embedded(ui, &mut self.store, busy);
            }
            SettingsSection::Import => {
                widgets::card(ui, |ui| {
                    widgets::section(ui, "Atendimentos");
                    crate::saiop::SaiopPanel::auto_attendance_setting(ui, &mut self.store);
                });
            }
            SettingsSection::Reminders => self.reminders_card(ui),
            SettingsSection::Holidays => self.holidays_card(ui),
            SettingsSection::Ai => self.ai_settings(ui),
            SettingsSection::Backup => {
                let idle = self.worker.is_none();
                let has_backup = self.backup_path.is_some();
                let mut backup = false;
                let mut export = false;
                let mut copy_backup = false;
                // Beside the library file; nothing is created or looked up on disk here.
                let folder = self.db_path.parent().map(|dir| dir.join("backups"));
                let last_copy = self
                    .backup_path
                    .as_ref()
                    .and_then(|path| std::fs::metadata(path).ok())
                    .and_then(|meta| meta.modified().ok())
                    .map(|time| {
                        chrono::DateTime::<chrono::Local>::from(time)
                            .format("%d/%m/%Y às %H:%M")
                            .to_string()
                    });
                let mut open_folder = false;
                widgets::card(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new("Guarda uma cópia protegida do acervo na pasta de cópias deste computador. Ela só abre na mesma conta do Windows.")
                                .color(theme::c().muted),
                        )
                        .wrap(),
                    );
                    ui.add_space(S1);
                    ui.horizontal_wrapped(|ui| {
                        backup = ui.add_enabled(idle, primary("Fazer cópia agora")).clicked();
                        if has_backup {
                            copy_backup = ui
                                .add(widgets::quiet("Copiar o caminho da última cópia"))
                                .clicked();
                        }
                    });
                    if let Some(folder) = &folder {
                        ui.add_space(S2);
                        widgets::field_label(ui, "Onde ficam as cópias");
                        ui.horizontal_wrapped(|ui| {
                            widgets::small_muted(ui, folder.display().to_string());
                            open_folder = ui
                                .add(widgets::quiet(format!("{} Abrir pasta", icon::FOLDER)))
                                .clicked();
                        });
                    }
                    if let Some(date) = &last_copy {
                        widgets::small_muted(ui, format!("Última cópia: {date}."));
                    }
                });
                if open_folder {
                    // The folder only exists after the first copy.
                    match &folder {
                        Some(folder) if folder.is_dir() => {
                            crate::windows_ui::reveal_in_explorer(folder)
                        }
                        _ => self.message("Ainda não há cópias: a pasta é criada na primeira."),
                    }
                }
                ui.add_space(S3);
                widgets::section(ui, "Exportação");
                Self::settings_card(
                    ui,
                    "Grava documentos, tarefas, atendimentos, recursos e usos de recursos em cinco arquivos .csv (sem textos, notas ou anexos), prontos para os painéis do CERES ou para o Excel.",
                    |ui| {
                        export = ui
                            .add_enabled(idle, secondary("Exportar dados para CSV…"))
                            .clicked();
                    },
                );
                if backup {
                    self.backup(ctx);
                }
                if export {
                    self.export_csv(ctx);
                }
                ui.add_space(S3);
                self.calendar_exchange(ui);
                if copy_backup {
                    if let Some(path) = &self.backup_path {
                        ctx.copy_text(path.display().to_string());
                        self.message("Caminho copiado.");
                    }
                }
            }
            SettingsSection::Categories => {
                let shown = self.category_manager.ui(ui, &mut self.store);
                if shown.changed {
                    self.refresh();
                }
                match shown.notice {
                    Some((text, true)) => self.error(text),
                    Some((text, false)) => self.message(text),
                    None => {}
                }
            }
            SettingsSection::Trash => {
                let busy = self.worker.is_some()
                    || self.pdf_reader.busy()
                    || self.exit_confirm
                    || self.replacement.is_some();
                if let Some(change) = self.trash.ui(ui, &mut self.store, busy) {
                    self.apply_trash_change(change);
                }
            }
            SettingsSection::Appearance => {
                let mut chosen = self.appearance;
                widgets::card(ui, |ui| {
                    widgets::field_label(ui, "Tema");
                    ui.horizontal(|ui| {
                        for option in [
                            theme::Appearance::System,
                            theme::Appearance::Light,
                            theme::Appearance::Dark,
                        ] {
                            if widgets::chip(ui, chosen == option, option.label()).clicked() {
                                chosen = option;
                            }
                        }
                    });
                });
                if chosen != self.appearance {
                    self.appearance = chosen;
                    theme::set_appearance(ctx, chosen);
                    save_appearance(chosen, self.text_size);
                }
                ui.add_space(S3);
                let mut size = self.text_size;
                widgets::card(ui, |ui| {
                    widgets::field_label(ui, "Tamanho do texto");
                    ui.horizontal(|ui| {
                        for option in theme::TextSize::ALL {
                            if widgets::chip(ui, size == option, option.label()).clicked() {
                                size = option;
                            }
                        }
                    });
                    widgets::small_muted(
                        ui,
                        "Aumenta as letras, os ícones e os botões de todo o Ponte.",
                    );
                });
                if size != self.text_size {
                    self.text_size = size;
                    theme::set_text_size(ctx, size);
                    save_appearance(self.appearance, size);
                }
            }
            SettingsSection::About => {
                let mut about = false;
                let mut guide = false;
                Self::settings_card(
                    ui,
                    &format!(
                        "Ponte {} • Assistente documental. Os dados ficam neste computador, protegidos pela sua conta do Windows.",
                        env!("CARGO_PKG_VERSION")
                    ),
                    |ui| {
                        guide = ui
                            .add(primary(format!("{} Guia do usuário", icon::BOOK)))
                            .on_hover_text("Abre o guia no navegador. Funciona sem internet.")
                            .clicked();
                        about = ui.add(secondary("Autores e licença…")).clicked();
                    },
                );
                if about {
                    self.about_open = true;
                }
                if guide {
                    self.open_guide();
                }
                self.update_card(ui, ctx);
            }
        }
    }

    /// The "Atualização" card of Sobre o Ponte. The check runs only when the
    /// person clicks; nothing is downloaded or installed by the app.
    fn update_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::update_check::{self, Outcome};
        update_check::poll(&mut self.update);
        let running = self.update.running();
        let result = self.update.last.clone();
        let mut check = false;
        let mut open: Option<String> = None;
        ui.add_space(S2);
        widgets::card(ui, |ui| {
            widgets::section(ui, "Atualização");
            widgets::muted(
                ui,
                format!("Versão instalada: {}", env!("CARGO_PKG_VERSION")),
            );
            ui.add_space(S1);
            ui.horizontal(|ui| {
                let label = if matches!(result, Some(Outcome::Failed(_))) {
                    "Tentar de novo"
                } else {
                    "Verificar se há versão nova"
                };
                check = ui
                    .add_enabled(!running, widgets::secondary(label))
                    .clicked();
                if running {
                    ui.spinner();
                    widgets::muted(ui, "Consultando…");
                }
            });
            widgets::small_muted(
                ui,
                "Só ao clicar, o Ponte consulta a página de versões no GitHub (github.com/demusis/ponteproj). Nada do acervo é enviado.",
            );
            let Some(result) = &result else { return };
            ui.add_space(S2);
            let (tone, glyph, text) = match result {
                Outcome::UpToDate { latest } => (
                    Tone::Ok,
                    icon::CHECK,
                    format!("Você já tem a versão mais recente ({latest})."),
                ),
                Outcome::Newer {
                    latest, published, ..
                } => (
                    Tone::Info,
                    icon::INFO,
                    if published.is_empty() {
                        format!("Há uma versão nova: {latest}.")
                    } else {
                        format!("Há uma versão nova: {latest}, publicada em {published}.")
                    },
                ),
                Outcome::Failed(message) => (Tone::Danger, icon::WARNING, message.clone()),
            };
            let (fg, bg) = tone.colors();
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(glyph).size(18.0).color(fg));
                        ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                    });
                });
            if let Outcome::Newer { url, .. } = result {
                ui.add_space(S2);
                if ui
                    .add(widgets::primary(format!(
                        "{} Abrir a página de download",
                        icon::DOWNLOAD
                    )))
                    .clicked()
                {
                    open = Some(update_check::safe_release_url(url));
                }
                widgets::small_muted(
                    ui,
                    "Feche o Ponte antes de instalar. O instalador guarda uma cópia de segurança do acervo antes de atualizar.",
                );
            }
        });
        if check {
            update_check::start(&mut self.update, ctx);
        }
        if let Some(url) = open {
            if let Err(error) = crate::windows_ui::open_url(&url) {
                self.error(format!("Não foi possível abrir a página: {error:#}"));
            }
        }
    }

    /// The "Inteligência artificial" section of Ajustes. Leaving the section
    /// saves what was changed (see `leave_ai_settings`).
    fn ai_settings(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let mut save = false;
        let mut refresh = false;
        let mut provider_changed = false;
        widgets::card(ui, |ui| {
            widgets::section(ui, "Onde a IA funciona");
            let before = self.ai.provider;
            let options = [
                (
                    AiProvider::Local,
                    "Ollama neste computador",
                    "Modelo instalado neste computador.",
                    "Nada sai deste computador",
                    Tone::Ok,
                    "Modelo instalado neste computador. Nada sai deste computador.",
                ),
                (
                    AiProvider::Cloud,
                    "Ollama Cloud",
                    "Modelos grandes nos servidores da Ollama, com a chave da sua conta.",
                    "Envia os textos à Ollama",
                    Tone::Warn,
                    "Modelos grandes nos servidores da Ollama (ollama.com), com a chave da sua conta.",
                ),
                (
                    AiProvider::Remote,
                    "Outro servidor",
                    "Servidor do órgão que funciona como o Ollama. O endereço começa com https.",
                    "Envia os textos ao servidor informado",
                    Tone::Warn,
                    "Servidor do órgão que funciona como o Ollama. O endereço deve começar com https.",
                ),
                (
                    AiProvider::ChatGpt,
                    "ChatGPT (assinatura)",
                    "Usa a cota da sua assinatura ChatGPT Plus ou Pro, entrando com a sua conta.",
                    "Envia os textos à OpenAI",
                    Tone::Warn,
                    "Usa a cota da sua assinatura ChatGPT Plus ou Pro, entrando com a sua conta. Os textos vão para os servidores da OpenAI.",
                ),
            ];
            for (value, title, line, tag, tone, tip) in options {
                if option_card(ui, self.ai.provider == value, title, line, tag, tone, tip) {
                    self.ai.provider = value;
                }
                ui.add_space(S1);
            }
            provider_changed = before != self.ai.provider;
            ui.add_space(S2);
            // Nothing answered at the address of the local Ollama (the state
            // the last model listing left behind).
            let no_answer = self.ai.provider == AiProvider::Local
                && self.models_error.as_deref().is_some_and(|error| {
                    error.starts_with("Não foi possível conectar ao servidor de IA")
                        || error.starts_with("O servidor de IA demorou demais")
                        || error.starts_with("Não houve resposta")
                });
            match self.ai.provider {
                AiProvider::ChatGpt => {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(format!("{} Nesta opção, os dados saem do seu computador e do órgão.", icon::WARNING)).color(theme::c().warn));
                        ui.label("A cada clique em “Analisar” ou “Gerar com IA”, o texto do documento, as notas, o contexto e as fontes selecionadas são enviados aos servidores da OpenAI, nos Estados Unidos, sob os termos e a política de privacidade da OpenAI. Nada é enviado ao importar, só quando você clica. Use apenas com documentos que podem sair do órgão; avalie sigilo e LGPD antes.");
                        widgets::small_muted(ui, "O Ponte não recebe acesso às suas conversas, à memória nem à cobrança do ChatGPT. O uso é descontado da cota da sua assinatura (Plus ou Pro); o limite por aplicativo pode ser ajustado em ChatGPT ▸ Configurações ▸ Uso. Quando a cota acaba, o Ponte avisa e não usa outro provedor por conta própria.");
                    });
                    ui.add_space(4.0);
                    let signed_in = self.ai.chatgpt.signed_in();
                    let sharing = self.ai.chatgpt.sharing();
                    let account = self.ai.chatgpt.account_label.clone();
                    let in_progress = self.chatgpt_login.is_some();
                    ui.horizontal_wrapped(|ui| {
                        if in_progress {
                            ui.spinner();
                            ui.label("Aguardando a autorização no navegador…");
                            if ui.button("Cancelar").clicked() {
                                self.cancel_chatgpt_login();
                            }
                        } else if signed_in {
                            ui.label(format!("Conectado: {account}"));
                            if sharing {
                                pill(ui, "Uso do plano liberado", Tone::Ok);
                            } else {
                                pill(ui, "Sem permissão de uso do plano", Tone::Warn);
                            }
                            if ui.button("Entrar de novo…").on_hover_text("Abre o navegador para renovar a autorização ou liberar o uso do plano.").clicked() {
                                self.start_chatgpt_login(ctx);
                            }
                            if ui.button("Sair").on_hover_text("Apaga a sessão deste computador e pede à OpenAI que a encerre.").clicked() {
                                self.chatgpt_sign_out();
                            }
                        } else {
                            if ui.button("Entrar com o ChatGPT…").on_hover_text("Abre o navegador na página da OpenAI. Entre com a conta da sua assinatura e autorize o Ponte, inclusive o uso do plano.").clicked() {
                                self.start_chatgpt_login(ctx);
                            }
                            if !account.is_empty() {
                                widgets::small_muted(ui, format!("Última conta: {account}"));
                            }
                        }
                    });
                    if let Some(login) = &self.chatgpt_login {
                        let url = login.authorize_url.clone();
                        ui.horizontal_wrapped(|ui| {
                            widgets::small_muted(
                                ui,
                                "Se o navegador não abriu, copie o endereço e cole nele:",
                            );
                            if ui.add(widgets::quiet("Copiar endereço")).clicked() {
                                ctx.copy_text(url);
                            }
                        });
                    }
                    let login_failed = self.chatgpt_login_error.is_some();
                    if let Some(error) = &self.chatgpt_login_error {
                        // The sentence in red; the technical cause (a second
                        // line, when there is one) muted.
                        let (sentence, detail) = error
                            .split_once(char::from(10))
                            .unwrap_or((error.as_str(), ""));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon::ERROR).color(theme::c().danger));
                            ui.add(
                                egui::Label::new(RichText::new(sentence).color(theme::c().danger))
                                    .wrap(),
                            );
                        });
                        if !detail.is_empty() {
                            widgets::small_muted(ui, detail);
                        }
                    }
                    // The test of the way to OpenAI, quiet; next to the error
                    // when the sign-in failed.
                    self.net_test_ui(ui, ctx, login_failed && !signed_in);
                    if self.chatgpt_login_error.is_some() && !in_progress && !signed_in {
                        // The way out when the OpenAI page itself refuses the
                        // sign-in of this computer ("Agent registration session
                        // changed; restart sign-in"): close its tabs and, if it
                        // persists, register this computer afresh.
                        widgets::small_muted(ui, "Se a página da OpenAI mostrar “Agent registration session changed; restart sign-in”, feche as abas da OpenAI abertas no navegador e clique de novo em “Entrar com o ChatGPT…”. Se continuar, recomece o registro:");
                        if ui
                            .add(widgets::quiet("Recomeçar o registro deste computador"))
                            .on_hover_text("Esquece o registro anterior deste computador na OpenAI e faz um novo na próxima entrada. Não apaga nada do acervo.")
                            .clicked()
                        {
                            self.restart_chatgpt_registration();
                        }
                    }
                    widgets::small_muted(ui, "Requer assinatura ChatGPT Plus ou Pro (conta gratuita entra, mas não libera o uso do plano). O login é feito no navegador, em auth.openai.com, e retorna ao Ponte neste computador.");
                }
                AiProvider::Local => {
                    let label = widgets::field_label(ui, "Endereço do Ollama");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ai.local_url)
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(label.id);
                    if no_answer {
                        widgets::small_muted(ui, "Padrão: http://127.0.0.1:11434.");
                    } else {
                        widgets::small_muted(ui, "Padrão: http://127.0.0.1:11434. Instale o Ollama em ollama.com/download e baixe um modelo (ex.: ollama pull qwen3:4b-instruct).");
                    }
                    if self.ai_leaves() {
                        super::notice_line(
                            ui,
                            Tone::Warn,
                            "O modelo escolhido roda na nuvem (ollama.com): o que você enviar à IA sai deste computador, mesmo com o Ollama instalado aqui.",
                        );
                    }
                }
                AiProvider::Cloud => {
                    widgets::small_muted(ui, format!("Endereço: {} (fixo)", ai::CLOUD_URL));
                    let label = widgets::field_label(ui, "Chave da sua conta");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ai.cloud_key)
                            .password(true)
                            .hint_text("cole aqui a chave gerada em ollama.com")
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(label.id);
                    ui.horizontal_wrapped(|ui| {
                        widgets::small_muted(ui, "Crie a chave em");
                        ui.hyperlink_to(
                            RichText::new("ollama.com/settings/keys").size(theme::SIZE_SMALL),
                            "https://ollama.com/settings/keys",
                        );
                        widgets::small_muted(ui, "(conta gratuita por uso ou plano mensal).");
                    });
                    ui.colored_label(theme::c().warn, "Os textos enviados à IA saem deste computador para os servidores da Ollama (EUA, com possível roteamento pela Europa). A Ollama declara não registrar nem treinar com os dados.");
                }
                AiProvider::Remote => {
                    let label = widgets::field_label(ui, "Endereço do servidor (HTTPS)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ai.remote_url)
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(label.id);
                    let label = widgets::field_label(ui, "Chave do servidor (opcional)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ai.remote_token)
                            .password(true)
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(label.id);
                    ui.colored_label(
                        theme::c().warn,
                        "Os textos enviados à IA saem deste computador para esse servidor.",
                    );
                }
            }
            ui.add_space(S2);
            widgets::field_label(ui, "Modelo de IA");
            let listed = self.models_match_current() && !self.models.is_empty();
            let fetching = self.model_fetch.is_some();
            ui.horizontal_wrapped(|ui| {
                if listed {
                    let models = self.models.clone();
                    let current = self.ai.model().trim().to_owned();
                    let selected = models
                        .iter()
                        .find(|m| m.name == current)
                        .map(ModelInfo::label)
                        .unwrap_or_else(|| {
                            if current.is_empty() {
                                "Escolha um modelo".into()
                            } else {
                                current.clone()
                            }
                        });
                    egui::ComboBox::from_id_salt("ai-model")
                        .selected_text(selected)
                        .width(320.0)
                        .show_ui(ui, |ui| {
                            let model = self.ai.model_mut();
                            for info in &models {
                                ui.selectable_value(model, info.name.clone(), info.label());
                            }
                        });
                } else {
                    let model = self.ai.model_mut();
                    ui.add(
                        egui::TextEdit::singleline(model)
                            .hint_text("nome do modelo")
                            .desired_width(320.0),
                    );
                }
                // With the guidance block showing, its own button retries.
                if !no_answer {
                    let button = widgets::secondary(if listed {
                        format!("{} Atualizar lista", icon::SYNC)
                    } else {
                        "Listar modelos do servidor".to_owned()
                    });
                    if ui
                        .add_enabled(!fetching, button)
                        .on_hover_text(
                            "Consulta o servidor configurado e lista os modelos que ele oferece.",
                        )
                        .clicked()
                    {
                        refresh = true;
                    }
                }
                if fetching {
                    ui.spinner();
                    widgets::small_muted(ui, "consultando…");
                }
            });
            if no_answer {
                let (fg, bg) = Tone::Warn.colors();
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(format!(
                                "O Ollama não respondeu em {}.",
                                self.ai.base_url().trim()
                            ))
                            .font(theme::font_strong(theme::SIZE_BODY))
                            .color(fg),
                        );
                        ui.add_space(S1);
                        for step in [
                            "1. Instale o Ollama: ollama.com/download.",
                            "2. Abra o Ollama.",
                            "3. Baixe um modelo. No terminal, execute: ollama pull qwen3:4b-instruct",
                        ] {
                            ui.add(egui::Label::new(RichText::new(step).color(fg)).wrap());
                        }
                        ui.add_space(S2);
                        if ui
                            .add_enabled(!fetching, widgets::secondary("Tentar de novo"))
                            .on_hover_text(
                                "Consulta o servidor configurado e lista os modelos que ele oferece.",
                            )
                            .clicked()
                        {
                            refresh = true;
                        }
                    });
            } else if let Some(error) = &self.models_error {
                let (fg, bg) = Tone::Warn.colors();
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon::WARNING).size(18.0).color(fg));
                            ui.add(egui::Label::new(RichText::new(error).color(fg)).wrap());
                        });
                    });
            } else if listed {
                widgets::small_muted(
                    ui,
                    if self.models.len() == 1 {
                        "1 modelo oferecido por este servidor.".to_owned()
                    } else {
                        format!(
                            "{} modelos oferecidos por este servidor.",
                            self.models.len()
                        )
                    },
                );
            } else if self.model_fetch.is_none() {
                widgets::small_muted(
                    ui,
                    "Clique em “Listar modelos do servidor” para escolher entre os disponíveis.",
                );
            }
            ui.add_space(6.0);
            let remember_label = if self.ai.provider == AiProvider::ChatGpt {
                "Guardar a sessão do ChatGPT neste computador"
            } else {
                "Guardar a chave neste computador"
            };
            ui.checkbox(&mut self.ai.remember_keys, remember_label)
                .on_hover_text("A configuração fica no acervo, protegida pela sua conta do Windows. Desmarcada, a chave (ou a sessão do ChatGPT) vale só até fechar o Ponte.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let dirty = self.ai_config_dirty();
                if ui.add_enabled(dirty, widgets::primary("Salvar")).clicked() {
                    save = true;
                }
                if dirty {
                    pill(ui, "Não salvo", Tone::Warn);
                    widgets::small_muted(ui, "Também é salvo ao sair desta seção.");
                } else {
                    widgets::small_muted(ui, "Configuração salva.");
                }
            });
            ui.separator();
            widgets::small_muted(ui, "O servidor só é acessado ao listar modelos, analisar ou gerar uma sugestão. O aplicativo não solicita sua senha do SIGADOC.");
            widgets::small_muted(ui, "A análise é salva no acervo. A sugestão de minuta só é salva por sua ação explícita.");
        });
        self.ai_test_card(ui);
        if provider_changed {
            self.models_error = None;
            if !self.models_match_current() {
                self.fetch_models(ctx);
            }
        }
        if refresh {
            self.models_for = None;
            self.fetch_models(ctx);
        }
        if save && self.save_ai_config() {
            self.message("Configuração de IA salva.");
        }
    }

    /// The card "Testar a IA com casos fictícios": runs the fictitious cases
    /// of the program against the configured AI and says how it did.
    fn ai_test_card(&mut self, ui: &mut egui::Ui) {
        let running = matches!(&self.worker, Some(w) if w.kind == super::WorkKind::AiTest);
        let busy = self.worker.is_some();
        let unsaved = self.ai_config_dirty();
        let no_model = self.ai.model().trim().is_empty();
        let mut start = false;
        let mut cancel = false;
        let mut save = false;
        let mut copy = false;
        widgets::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = S2;
            widgets::section(ui, "Testar a IA com casos fictícios");
            ui.add(
                egui::Label::new(
                    RichText::new("Envia à IA configurada alguns documentos fictícios, que acompanham o Ponte, e confere se ela leu e propôs o que se esperava. Além das análises, o teste gera minutas fictícias e confere os critérios delas: por isso leva mais tempo. Nada do seu acervo é enviado e nada é salvo nele.")
                        .color(theme::c().muted),
                )
                .wrap(),
            );
            self.ai_destination(ui);
            ui.horizontal(|ui| {
                if running {
                    ui.spinner();
                    let step = self
                        .worker
                        .as_ref()
                        .map(|w| w.latest.clone())
                        .unwrap_or_default();
                    ui.label(if step.is_empty() {
                        "Testando…".to_owned()
                    } else {
                        format!("Testando… {step}")
                    });
                    cancel = ui
                        .add_enabled(
                            !self.worker.as_ref().is_some_and(super::Worker::cancelling),
                            widgets::secondary("Cancelar"),
                        )
                        .clicked();
                } else {
                    let button = ui.add_enabled(
                        !busy && !unsaved && !no_model,
                        widgets::secondary("Testar a IA"),
                    );
                    let button = if busy {
                        button.on_disabled_hover_text("Espere o trabalho em andamento terminar.")
                    } else if unsaved {
                        button.on_disabled_hover_text("Salve a configuração da IA antes de testar.")
                    } else if no_model {
                        button.on_disabled_hover_text("Escolha o modelo da IA antes de testar.")
                    } else {
                        button.on_hover_text("Roda os casos fictícios com o modelo configurado. Pode levar alguns minutos.")
                    };
                    start = button.clicked();
                }
            });
            match &self.ai_test_report {
                None => {}
                Some(Err(error)) => {
                    super::notice_line(
                        ui,
                        Tone::Danger,
                        format!("Não foi possível testar a IA: {error}"),
                    );
                }
                Some(Ok(report)) => {
                    ui.separator();
                    let (passed, total) = (report.passed(), report.total());
                    for (title, cases, (group_passed, group_total)) in ai_test_groups(report) {
                        if let Some(title) = title {
                            ui.horizontal_wrapped(|ui| {
                                widgets::section(ui, title);
                                pill(
                                    ui,
                                    ai_test_summary(group_passed, group_total),
                                    ai_test_tone(group_passed, group_total),
                                );
                            });
                        }
                        for case in cases {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(&case.name)
                                        .font(theme::font_strong(theme::SIZE_BODY)),
                                );
                                pill(
                                    ui,
                                    ai_test_summary(case.passed, case.total),
                                    ai_test_tone(case.passed, case.total),
                                );
                                widgets::small_muted(
                                    ui,
                                    format!(
                                        "{} • {:.1} s",
                                        crate::labels::count(
                                            case.calls as usize,
                                            "chamada",
                                            "chamadas"
                                        ),
                                        case.seconds
                                    ),
                                );
                            });
                            for failure in &case.failures {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("• {failure}"))
                                            .size(theme::SIZE_SMALL)
                                            .color(theme::c().muted),
                                    )
                                    .wrap(),
                                );
                            }
                        }
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Total").font(theme::font_strong(theme::SIZE_BODY)));
                        pill(
                            ui,
                            ai_test_summary(passed, total),
                            ai_test_tone(passed, total),
                        );
                    });
                    widgets::small_muted(
                        ui,
                        "O resultado traz só os casos fictícios: pode ser guardado e enviado a quem cuida da instalação.",
                    );
                    ui.horizontal(|ui| {
                        save = ui.add(widgets::secondary("Salvar o resultado…")).clicked();
                        copy = ui.add(widgets::quiet("Copiar")).clicked();
                    });
                }
            }
        });
        if cancel {
            self.cancel_worker();
        }
        if start {
            self.request_ai_test(ui.ctx());
        }
        if copy {
            if let Some(Ok(report)) = &self.ai_test_report {
                ui.ctx().copy_text(report.text());
                self.message("Resultado copiado.");
            }
        }
        if save {
            self.save_ai_test();
        }
    }

    /// "Salvar o resultado…": a text file where the person chooses (the
    /// report holds no real data).
    fn save_ai_test(&mut self) {
        let Some(Ok(report)) = &self.ai_test_report else {
            return;
        };
        let text = report.text();
        let Some(mut path) = crate::windows_ui::save_file(
            "Salvar o resultado do teste da IA",
            "Teste da IA.txt",
            &[("Arquivo de texto", "*.txt")],
        ) else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("txt");
        }
        match std::fs::write(&path, text) {
            Ok(()) => self.message(format!("Resultado salvo em {}.", path.display())),
            Err(error) => self.error(format!("Não foi possível salvar o resultado: {error}")),
        }
    }

    /// The button "Testar a IA": asks first, like any sending to an outside
    /// AI, but once per destination in this session (the cases are not a
    /// document of the acervo, so nothing is recorded in it).
    pub(super) fn request_ai_test(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.settings() {
            self.error(error.to_string());
            return;
        }
        let (_, destination) = self.ai_destination_text();
        let asked = super::queue::needs_consent(
            self.ai_leaves(),
            !self.ai_test_consented.contains(&destination),
        );
        if asked {
            self.send_confirm = Some(self.send_question(
                super::queue::PendingSend::Test,
                0,
                "casos fictícios".to_owned(),
                0,
            ));
            return;
        }
        self.start_ai_test(ctx);
    }

    pub(super) fn start_ai_test(&mut self, ctx: &egui::Context) {
        let settings = match self.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.error(error.to_string());
                return;
            }
        };
        self.ai_test_report = None;
        self.spawn(
            "Testando a IA com casos fictícios…".into(),
            super::WorkKind::AiTest,
            move |reporter| {
                super::WorkResult::AiTest(
                    crate::ai_eval::run_live(&settings, &reporter).map_err(|e| e.to_string()),
                )
            },
            ctx,
        );
    }

    /// Leaving the AI section saves what was changed, as closing its window
    /// used to. False when the configuration is invalid: the person stays.
    pub(super) fn leave_ai_settings(&mut self) -> bool {
        if !self.ai_config_dirty() {
            return true;
        }
        if self.save_ai_config() {
            self.message("Configuração de IA salva.");
            true
        } else {
            false
        }
    }

    /// True when the listed models came from the provider and endpoint now
    /// configured (otherwise the list is stale and is not shown).
    pub(super) fn models_match_current(&self) -> bool {
        self.models_for.as_ref().is_some_and(|(provider, base)| {
            *provider == self.ai.provider
                && base.trim_end_matches('/') == self.ai.base_url().trim().trim_end_matches('/')
        })
    }

    /// Opens the browser on OpenAI's sign-in page and waits, in the
    /// background, for the authorization to come back to the local port.
    /// Button, progress and result of "Testar a conexão com a OpenAI".
    fn net_test_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, after_error: bool) {
        if let Some(NetTest::Running(receiver)) = &self.net_test {
            match receiver.try_recv() {
                Ok(report) => self.net_test = Some(NetTest::Done(report)),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.net_test = Some(NetTest::Done(crate::netdiag::NetReport {
                        lines: Vec::new(),
                        verdict: "O teste da conexão foi interrompido. Tente de novo.".into(),
                    }));
                }
            }
        }
        let running = matches!(self.net_test, Some(NetTest::Running(_)));
        ui.horizontal_wrapped(|ui| {
            if running {
                ui.spinner();
                widgets::muted(ui, "Testando a conexão com a OpenAI…");
            } else if ui
                .add(widgets::quiet("Testar a conexão com a OpenAI"))
                .on_hover_text(if after_error {
                    "Confere o caminho até a OpenAI (nome, firewall, proxy, certificados) e diz o que fazer. Não envia sua conta nem seus documentos."
                } else {
                    "Confere o caminho até a OpenAI e diz o que fazer se algo bloquear. Não envia sua conta nem seus documentos."
                })
                .clicked()
            {
                self.start_net_test(ctx);
            }
        });
        let mut close = false;
        if let Some(NetTest::Done(report)) = &self.net_test {
            widgets::card(ui, |ui| {
                for line in &report.lines {
                    widgets::small_muted(ui, line.as_str());
                }
                ui.add_space(S2);
                ui.add(egui::Label::new(RichText::new(report.verdict.as_str())).wrap());
                ui.add_space(S2);
                ui.horizontal(|ui| {
                    if ui.add(widgets::quiet("Copiar")).on_hover_text("Copia o relatório para enviar a quem cuida do Ponte. Não contém chaves nem dados da conta.").clicked() {
                        ui.ctx().copy_text(report.text());
                    }
                    if ui.add(widgets::quiet("Fechar")).clicked() {
                        close = true;
                    }
                });
            });
        }
        if close {
            self.net_test = None;
        }
    }

    /// Runs the test on a thread of its own: it waits on the network.
    fn start_net_test(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        match std::thread::Builder::new()
            .name("ponte-net-test".into())
            .spawn(move || {
                let _ = sender.send(crate::netdiag::diagnose(&crate::netdiag::OPENAI_HOSTS));
                repaint.request_repaint();
            }) {
            Ok(_) => self.net_test = Some(NetTest::Running(receiver)),
            Err(error) => {
                self.net_test = Some(NetTest::Done(crate::netdiag::NetReport {
                    lines: Vec::new(),
                    verdict: format!("Não foi possível iniciar o teste: {error}"),
                }));
            }
        }
    }

    fn start_chatgpt_login(&mut self, ctx: &egui::Context) {
        if self.chatgpt_login.is_some() {
            return;
        }
        self.chatgpt_login_error = None;
        if self.ai.chatgpt_host_id.trim().is_empty() {
            self.ai.chatgpt_host_id = chatgpt::new_host_id();
        }
        let login = match chatgpt::Login::start(&self.ai.chatgpt_host_id, Some(&self.ai.chatgpt)) {
            Ok(login) => login,
            Err(error) => {
                self.chatgpt_login_error = Some(error.to_string());
                return;
            }
        };
        let authorize_url = login.authorize_url.clone();
        if let Err(error) = crate::windows_ui::open_url(&authorize_url) {
            self.chatgpt_login_error = Some(error.to_string());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        match std::thread::Builder::new()
            .name("ponte-chatgpt-login".into())
            .spawn(move || {
                let mut issued = None;
                let result = login
                    .wait_keeping(
                        &flag,
                        chatgpt::LOGIN_TIMEOUT,
                        crate::windows_ui::open_url,
                        &mut issued,
                    )
                    .map_err(|e| e.to_string());
                let _ = sender.send((result, issued));
                repaint.request_repaint();
            }) {
            Ok(_) => {
                self.chatgpt_login = Some(ChatGptLogin {
                    receiver,
                    cancel,
                    authorize_url,
                });
            }
            Err(error) => {
                self.chatgpt_login_error =
                    Some(format!("Não foi possível iniciar o login: {error}"));
            }
        }
    }

    /// A fresh registration of this computer at OpenAI on the next sign-in:
    /// a new host identification and no issued client id. For a sign-in the
    /// OpenAI page refuses because of a registration left half-way.
    fn restart_chatgpt_registration(&mut self) {
        if self.chatgpt_login.is_some() || self.ai.chatgpt.signed_in() {
            return;
        }
        self.ai.chatgpt_host_id = chatgpt::new_host_id();
        let mut session = self.ai.chatgpt.without_tokens();
        session.client_id.clear();
        self.set_chatgpt_session(session);
        self.chatgpt_login_error = None;
        if self.save_ai_config() {
            self.message("Registro recomeçado. Clique em “Entrar com o ChatGPT…”.".to_owned());
        }
    }

    fn cancel_chatgpt_login(&mut self) {
        if let Some(login) = self.chatgpt_login.take() {
            login.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Forgets the tokens here and asks OpenAI to revoke them (in the
    /// background; the local part never waits for the network).
    fn chatgpt_sign_out(&mut self) {
        let session = self.ai.chatgpt.clone();
        self.set_chatgpt_session(session.without_tokens());
        self.models_for = None;
        self.models.clear();
        if self.save_ai_config() {
            self.message("Sessão do ChatGPT encerrada neste computador. Se quiser, confira em ChatGPT ▸ Configurações ▸ Aplicativos conectados.");
        }
        if session.signed_in() {
            let _ = std::thread::Builder::new()
                .name("ponte-chatgpt-signout".into())
                .spawn(move || {
                    let _ = chatgpt::sign_out(&session);
                });
        }
    }
}

/// One place the AI can run: a card with a radio (the chosen one tinted and
/// outlined), a title, one muted line and a pill saying whether the texts
/// leave this computer. Returns true when it was clicked.
fn option_card(
    ui: &mut egui::Ui,
    selected: bool,
    title: &str,
    line: &str,
    tag: &str,
    tone: Tone,
    tip: &str,
) -> bool {
    let p = theme::c();
    let (fill, stroke) = if selected {
        (p.primary_soft, egui::Stroke::new(1.5, p.primary))
    } else {
        (p.surface, egui::Stroke::new(1.0, p.outline))
    };
    let response = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                let center = egui::pos2(dot.center().x, dot.top() + 10.0);
                let ring = if selected { p.primary } else { p.muted };
                ui.painter()
                    .circle_stroke(center, 8.0, egui::Stroke::new(1.5, ring));
                if selected {
                    ui.painter().circle_filled(center, 4.5, p.primary);
                }
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(title).font(theme::font_strong(theme::SIZE_BODY)),
                            )
                            .selectable(false),
                        );
                        widgets::pill(ui, tag, tone);
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(line).size(theme::SIZE_SMALL).color(p.muted),
                        )
                        .selectable(false),
                    );
                });
            });
        })
        .response;
    // Reachable with Tab; Space or Enter chooses it, and the focus shows.
    let choice = ui
        .interact(
            response.rect,
            ui.auto_id_with(("ai-provider", title)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip);
    if choice.has_focus() {
        widgets::outline(ui, choice.rect.expand(2.0), p.primary);
    }
    choice.clicked()
}

/// "12 de 14 conferências" (or "1 de 1 conferência").
fn ai_test_summary(passed: u32, total: u32) -> String {
    format!(
        "{passed} de {total} {}",
        if total == 1 {
            "conferência"
        } else {
            "conferências"
        }
    )
}

/// All checks passed, most of them, or fewer than half.
fn ai_test_tone(passed: u32, total: u32) -> Tone {
    if total == 0 {
        Tone::Neutral
    } else if passed >= total {
        Tone::Ok
    } else if passed * 2 >= total {
        Tone::Warn
    } else {
        Tone::Danger
    }
}

/// Checks passed and checks made, over some cases.
fn ai_test_subtotal(cases: &[crate::ai_eval::CaseResult]) -> (u32, u32) {
    cases.iter().fold((0, 0), |(passed, total), case| {
        (passed + case.passed, total + case.total)
    })
}

/// The groups the card shows: without drafts one list with no title;
/// with them "Análises" and "Minutas", each with its subtotal.
type TestGroup<'a> = (
    Option<&'static str>,
    &'a [crate::ai_eval::CaseResult],
    (u32, u32),
);

fn ai_test_groups(report: &crate::ai_eval::Report) -> Vec<TestGroup<'_>> {
    if report.drafts.is_empty() {
        return vec![(None, &report.cases, ai_test_subtotal(&report.cases))];
    }
    vec![
        (
            Some("Análises"),
            &report.cases,
            ai_test_subtotal(&report.cases),
        ),
        (
            Some("Minutas"),
            &report.drafts,
            ai_test_subtotal(&report.drafts),
        ),
    ]
}

#[cfg(test)]
mod ai_test_tests {
    use super::*;
    use crate::ai_eval::{score, CaseResult};

    #[test]
    fn the_summary_reads_as_a_sentence_and_the_tone_follows_the_share() {
        assert_eq!(ai_test_summary(12, 14), "12 de 14 conferências");
        assert_eq!(ai_test_summary(1, 1), "1 de 1 conferência");
        assert_eq!(ai_test_tone(14, 14), Tone::Ok);
        assert_eq!(ai_test_tone(12, 14), Tone::Warn);
        assert_eq!(ai_test_tone(3, 14), Tone::Danger);
        assert_eq!(ai_test_tone(0, 0), Tone::Neutral);
    }

    #[test]
    fn the_totals_add_up_every_case() {
        let case = |name: &str, passed, total| CaseResult {
            name: name.into(),
            passed,
            total,
            failures: Vec::new(),
            calls: 2,
            seconds: 1.5,
        };
        let report = score(vec![case("A", 12, 14), case("B", 5, 5)]);
        assert_eq!((report.passed(), report.total()), (17, 19));
        let groups = ai_test_groups(&report);
        assert_eq!(groups.len(), 1, "no drafts: one list, no title");
        assert_eq!(groups[0].0, None);
        assert_eq!(groups[0].2, (17, 19));
    }

    #[test]
    fn analyses_and_drafts_are_two_groups_with_their_own_subtotals() {
        let case = |name: &str, passed, total| CaseResult {
            name: name.into(),
            passed,
            total,
            failures: Vec::new(),
            calls: 2,
            seconds: 1.5,
        };
        let mut report = score(vec![case("A", 12, 14), case("B", 5, 5)]);
        report.drafts = vec![case("M1", 8, 9), case("M2", 3, 6)];
        let groups = ai_test_groups(&report);
        assert_eq!(groups.len(), 2);
        assert_eq!((groups[0].0, groups[0].2), (Some("Análises"), (17, 19)));
        assert_eq!((groups[1].0, groups[1].2), (Some("Minutas"), (11, 15)));
        assert_eq!(groups[1].1.len(), 2);
        // The grand total is the sum of the two.
        assert_eq!((report.passed(), report.total()), (28, 34));
    }
}

#[cfg(test)]
mod holiday_library_tests {
    use super::*;
    use crate::holidays::{ConfigGuard, HolidayConfig};
    use crate::library_settings::HOLIDAYS;
    use crate::storage::Store;

    /// An app on a fresh library, with the process-wide configuration empty.
    /// The lock keeps the configuration out of the other tests' way.
    fn app(dir: &tempfile::TempDir) -> PonteApp {
        crate::holidays::set_config(HolidayConfig::new());
        let path = dir.path().join("acervo.sqlite3");
        PonteApp::new(Store::open(&path).unwrap(), path)
    }

    /// What another computer's synchronization does to the library file.
    fn another_connection_sets(dir: &tempfile::TempDir, text: &str) {
        let mut other = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        other.set_library_setting(HOLIDAYS, text).unwrap();
    }

    #[test]
    fn what_another_computer_saved_reaches_this_one_and_its_draft() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        assert!(!app.sync_holidays_with_library());
        another_connection_sets(&dir, "uf MG\nanual 05-02 Recesso\n");
        assert!(app.sync_holidays_with_library());
        assert_eq!(crate::holidays::config().uf, Some("MG"));
        assert_eq!(app.holiday_draft.uf, Some("MG"));
        assert_eq!(app.holiday_draft.rows.len(), 1);
        assert!(!app.holiday_draft.editing());
        // Nothing new: nothing to do.
        assert!(!app.sync_holidays_with_library());
    }

    #[test]
    fn a_stale_draft_never_overwrites_what_another_computer_saved() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        another_connection_sets(&dir, "uf SP\n");
        assert!(app.sync_holidays_with_library());
        // Ajustes is open and shows SP; the other computer then saves RJ.
        another_connection_sets(&dir, "uf RJ\n");
        assert!(app.sync_holidays_with_library());
        // The screen applying its draft (as it does every frame) writes nothing.
        app.apply_holiday_draft();
        assert_eq!(
            app.store.library_setting(HOLIDAYS).unwrap().as_deref(),
            Some("uf RJ\n")
        );
        assert_eq!(app.holiday_draft.uf, Some("RJ"));
    }

    #[test]
    fn an_edit_here_goes_to_the_library() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        app.holiday_draft.uf = Some("BA");
        app.holiday_draft.rows.push(DayRow {
            date: "2031-05-02".into(),
            yearly: false,
            name: "Recesso".into(),
        });
        assert!(app.holiday_draft.editing());
        app.apply_holiday_draft();
        assert!(!app.holiday_draft.editing());
        let stored = app.store.library_setting(HOLIDAYS).unwrap().unwrap();
        assert_eq!(
            HolidayConfig::parse(&stored),
            crate::holidays::config(),
            "the library and the planning agree"
        );
        assert_eq!(crate::holidays::config().uf, Some("BA"));
        assert_eq!(crate::holidays::config().custom.len(), 1);
        // Clearing everything is saved as a value, not as "never set".
        app.holiday_draft.uf = None;
        app.holiday_draft.rows.clear();
        app.apply_holiday_draft();
        assert_eq!(
            app.store.library_setting(HOLIDAYS).unwrap().as_deref(),
            Some("")
        );
    }

    #[test]
    fn a_blank_row_being_filled_survives_a_refresh_from_the_library() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        app.holiday_draft.rows.push(DayRow::default());
        // The screen applies the draft: the blank row is not a day yet.
        app.apply_holiday_draft();
        assert!(app.holiday_draft.editing());
        // Another computer saves, the refresh brings it: the row stays.
        another_connection_sets(
            &dir, "uf MG
",
        );
        assert!(app.sync_holidays_with_library());
        assert_eq!(app.holiday_draft.rows.len(), 1);
        // Once it has a valid date the row is saved with the rest.
        app.holiday_draft.rows[0].date = "2031-05-02".into();
        app.holiday_draft.rows[0].name = "Recesso".into();
        app.apply_holiday_draft();
        assert!(!app.holiday_draft.editing());
        assert_eq!(crate::holidays::config().custom.len(), 1);
    }

    #[test]
    fn a_failed_save_keeps_the_input_and_is_retried_on_change_or_leave() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        app.holiday_draft.rows.push(DayRow {
            date: "2031-05-02".into(),
            yearly: false,
            name: "x".repeat(crate::library_settings::MAX_VALUE + 1),
        });
        app.apply_holiday_draft();
        // Nothing was stored, the person's input is still there and is not
        // replaced by what the library says.
        assert_eq!(app.store.library_setting(HOLIDAYS).unwrap(), None);
        assert_eq!(app.holiday_draft.rows.len(), 1);
        assert!(app.holiday_draft.editing());
        another_connection_sets(
            &dir, "uf MG
",
        );
        app.sync_holidays_with_library();
        assert_eq!(app.holiday_draft.rows.len(), 1);
        // Fixing the name makes the next application save it.
        app.holiday_draft.rows[0].name = "Recesso".into();
        app.apply_holiday_draft_with(false);
        assert!(!app.holiday_draft.editing());
        assert_eq!(crate::holidays::config().custom.len(), 1);
    }

    #[test]
    fn the_hand_over_of_this_computers_choice_happens_once() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        // Once the library has a value, the hand-over is over for good.
        another_connection_sets(
            &dir, "uf SC
",
        );
        assert!(app.sync_holidays_with_library());
        assert!(app.holiday_seed_done);
        // A library without a value (a restored older copy) is not seeded
        // again by this computer, and what it has is not overwritten.
        let other = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        other
            .conn
            .execute("DELETE FROM library_settings", [])
            .unwrap();
        drop(other);
        app.holiday_migration_revision = None;
        app.sync_holidays_with_library();
        assert_eq!(app.store.library_setting(HOLIDAYS).unwrap(), None);
    }

    #[test]
    fn this_computers_own_choice_moves_into_an_empty_library_once() {
        let _guard = ConfigGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut app = app(&dir);
        // What feriados.txt held before this version.
        crate::holidays::set_config(HolidayConfig::parse("uf PR\n"));
        assert!(!app.sync_holidays_with_library());
        assert_eq!(
            app.store.library_setting(HOLIDAYS).unwrap().as_deref(),
            Some("uf PR\n")
        );
        // Now the library wins over whatever this device keeps.
        another_connection_sets(&dir, "uf SC\n");
        assert!(app.sync_holidays_with_library());
        assert_eq!(crate::holidays::config().uf, Some("SC"));
    }
}
