//! The settings area: sections, AI configuration and the ChatGPT sign-in.
use super::{pill, save_appearance, Area, ChatGptLogin, Leave, PonteApp, SettingsSection};
use crate::ai::{self, AiProvider, ModelInfo};
use crate::chatgpt;
use crate::theme::{self, icon, S1, S2, S3};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

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
                .checkbox(&mut on, "Lembrar dos prazos uma vez por dia")
                .changed()
            {
                self.reminders.set_enabled(on);
            }
            ui.add_enabled_ui(on, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Horário");
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
                "O Ponte mostra um aviso do Windows com a quantidade de tarefas e atendimentos que vencem. Só enquanto o Ponte está aberto ou minimizado.",
            );
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
                egui::ScrollArea::vertical()
                    .id_salt(("settings-section", section.label()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
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
                    "Grava documentos, tarefas e atendimentos em três arquivos .csv (sem textos, notas ou anexos), prontos para os painéis do CERES ou para o Excel.",
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
                if copy_backup {
                    if let Some(path) = &self.backup_path {
                        ctx.copy_text(path.display().to_string());
                        self.message("Caminho copiado.");
                    }
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
                    if let Some(error) = &self.chatgpt_login_error {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon::ERROR).color(theme::c().danger));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(error.as_str()).color(theme::c().danger),
                                )
                                .wrap(),
                            );
                        });
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
                let result = login
                    .wait(&flag, chatgpt::LOGIN_TIMEOUT, crate::windows_ui::open_url)
                    .map_err(|e| e.to_string());
                let _ = sender.send(result);
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
