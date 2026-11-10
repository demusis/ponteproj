//! Secondary windows and dialogs: about, consult, manual record, e-mail and confirmations.
use super::{
    display_date, facts, notice_line, plural, Area, Consult, ManualForm, PendingTarget, PonteApp,
    Tab, WorkKind, WorkResult, EXTENSION_VERSION, PROJECT_URL,
};
use crate::model::DocumentDetail;
use crate::theme::{self, S1, S2, S3};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};

/// "Dados do registro" of a manual record.
/// "Dados do registro" of a manual record.
pub(super) fn manual_facts(ui: &mut egui::Ui, detail: &DocumentDetail) {
    let metadata = &detail.capture.metadata;
    let value = |key: &str| {
        metadata
            .get(key)
            .cloned()
            .unwrap_or_else(|| "não informado".into())
    };
    widgets::small_muted(
        ui,
        "Informado por quem criou o registro; nada disto vem do SIGADOC.",
    );
    facts(
        ui,
        ("manual-facts", detail.summary.id),
        &[
            ("Número no Ponte", detail.summary.document_number.clone()),
            ("Tipo", value("document_type")),
            ("Origem", value("origin")),
            (
                "Data do documento",
                metadata
                    .get("document_date")
                    .map(|d| crate::labels::date(d))
                    .unwrap_or_else(|| "não informada".into()),
            ),
            ("Registrado por", value("registered_by")),
            ("Última versão", display_date(&detail.capture.captured_at)),
        ],
    );
}

impl PonteApp {
    /// Authors, version, licence and where to find the project.
    /// Authors, version, licence and where to find the project.
    pub(super) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        egui::Window::new("Sobre o Ponte")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = S2;
                ui.label(
                    RichText::new("PONTE")
                        .font(theme::font_strong(24.0))
                        .color(theme::c().primary),
                );
                ui.label("Assistente documental para o SIGADOC");
                widgets::small_muted(
                    ui,
                    format!(
                        "Versão {} • extensão do navegador {}",
                        env!("CARGO_PKG_VERSION"),
                        EXTENSION_VERSION
                    ),
                );
                ui.add_space(S2);
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::overline(ui, "Autoria", None);
                    ui.label("Carlo Ralph De Musis");
                    ui.label("João Gabriel Bezerra Estrela");
                });
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::overline(ui, "Licença", None);
                    ui.add(egui::Label::new("MIT. O código-fonte é aberto e pode ser usado, copiado e modificado livremente, mantendo o aviso de autoria.").wrap());
                });
                widgets::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S1;
                    widgets::overline(ui, "Código, instalador e notas de versão", None);
                    ui.hyperlink_to(PROJECT_URL, PROJECT_URL);
                });
                ui.add(
                    egui::Label::new(
                        RichText::new("O Ponte guarda os documentos no seu computador, protegidos pela sua conta do Windows. Nada é enviado ao SIGADOC nem à nuvem.")
                            .size(theme::SIZE_SMALL)
                            .color(theme::c().muted),
                    )
                    .wrap(),
                );
                ui.add_space(S1);
                if ui.add(widgets::primary("Fechar")).clicked() {
                    close = true;
                }
            });
        if !open || close {
            self.about_open = false;
        }
    }

    pub(super) fn consult_window(&mut self, ctx: &egui::Context) {
        let Some(consult) = self.consult.take() else {
            return;
        };
        let detail = &consult.detail;
        let mut open = true;
        let mut close = false;
        egui::Window::new(format!("Consulta • {}", detail.summary.document_number))
            .open(&mut open)
            .default_size([680.0, 560.0])
            .min_size([360.0, 240.0])
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui.add(widgets::secondary("Fechar")).clicked() {
                            close = true;
                        }
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            widgets::title(ui, &detail.summary.title);
                            ui.add(
                                egui::Label::new(
                                    RichText::new("Só leitura: a tarefa continua aberta atrás desta janela. Para editar notas ou minutas, use “Ir para o documento”.")
                                        .size(theme::SIZE_SMALL)
                                        .color(theme::c().muted),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
                ui.add_space(S2);
                egui::ScrollArea::vertical()
                    .id_salt("consult-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S3;
                        widgets::card(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = S2;
                            widgets::section(ui, "Texto importado");
                            if detail.capture.text.is_empty() {
                                notice_line(ui, Tone::Warn, "Esta importação não trouxe texto. Abra o conteúdo no SIGADOC e importe de novo.");
                            } else {
                                let mut text = detail.capture.text.as_str();
                                egui::ScrollArea::horizontal()
                                    .id_salt("consult-text")
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::multiline(&mut text)
                                                .font(egui::TextStyle::Monospace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(18)
                                                .code_editor(),
                                        );
                                    });
                            }
                        });
                        widgets::card(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = S2;
                            widgets::section(ui, "Arquivos (PDF)");
                            self.files_list(ui, detail);
                        });
                    });
            });
        if open && !close {
            self.consult = Some(consult);
        }
    }

    /// Opens the read-only window with this document's text and PDFs.
    pub(super) fn open_consult(&mut self, document_id: i64) {
        match self.store.load_document(document_id) {
            Ok(detail) => self.consult = Some(Consult { detail }),
            Err(error) => self.error(format!(
                "Não foi possível abrir o documento para consulta: {error}"
            )),
        }
    }

    /// The "Registro manual" window.
    /// The "Registro manual" window.
    pub(super) fn manual_window(&mut self, ctx: &egui::Context) {
        if !self.manual_form.open {
            return;
        }
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let editing = self.manual_form.id.is_some();
        let title = if editing {
            format!("Editar registro manual {}", self.manual_form.number)
        } else {
            "Novo registro manual".to_owned()
        };
        egui::Window::new(title)
            .open(&mut open)
            .default_width(640.0)
            .resizable(true)
            .show(ctx, |ui| {
                let form = &mut self.manual_form;
                ui.spacing_mut().item_spacing.y = S2;
                notice_line(ui, Tone::Warn, "Registro criado por você, fora do SIGADOC. Ele fica marcado como manual em todo o Ponte e é compartilhado com a equipe como os demais documentos.");
                if !editing {
                    widgets::small_muted(ui, "Recebe um número próprio (MANUAL-ano/sequência) ao salvar. Anexe o arquivo original em Documento ▸ Anexos depois de salvar; aqui, resuma o que importa.");
                }
                ui.add_space(S1);
                widgets::field_label(ui, "Tipo");
                egui::ComboBox::from_id_salt("manual-kind")
                    .selected_text(crate::manual::kind_label(&form.fields.kind))
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (key, label) in crate::manual::KINDS {
                            ui.selectable_value(&mut form.fields.kind, (*key).to_string(), *label);
                        }
                    });
                widgets::field_label(ui, "Título");
                ui.add(egui::TextEdit::singleline(&mut form.fields.title).hint_text("ex.: Ligação do gabinete sobre a reforma").desired_width(f32::INFINITY));
                ui.columns(2, |columns| {
                    widgets::field_label(&mut columns[0], "Origem (de quem, de onde)");
                    columns[0].add(egui::TextEdit::singleline(&mut form.fields.origin).hint_text("órgão, setor ou pessoa").desired_width(f32::INFINITY));
                    widgets::field_label(&mut columns[1], "Data do documento");
                    crate::labels::date_field(&mut columns[1], "manual-document-date", &mut form.fields.document_date);
                });
                widgets::field_label(ui, "Texto (o que foi pedido, decidido ou recebido)");
                ui.add(
                    egui::TextEdit::multiline(&mut form.fields.text)
                        .desired_rows(12)
                        .desired_width(f32::INFINITY),
                );
                if !form.message.is_empty() {
                    notice_line(ui, Tone::Danger, form.message.clone());
                }
                ui.add_space(S1);
                ui.horizontal(|ui| {
                    let can_save = !editing || form.dirty();
                    save = ui.add_enabled(can_save, widgets::primary(if editing { "Salvar nova versão" } else { "Criar registro" })).clicked();
                    cancel = ui.add(widgets::secondary("Cancelar")).clicked();
                    if editing && !form.dirty() {
                        widgets::small_muted(ui, "sem alterações");
                    }
                });
            });
        if save {
            let fields = self.manual_form.fields.clone();
            let result = match self.manual_form.id {
                None => self
                    .store
                    .create_manual_document(&fields)
                    .map(|r| (r, true)),
                Some(id) => self
                    .store
                    .update_manual_document(id, &fields)
                    .map(|r| (r, false)),
            };
            match result {
                Ok((receipt, created)) => {
                    self.manual_form = ManualForm::default();
                    self.refresh();
                    self.select(receipt.document_id);
                    self.tab = Tab::Document;
                    let number = self
                        .documents
                        .iter()
                        .find(|d| d.id == receipt.document_id)
                        .map(|d| d.document_number.clone())
                        .unwrap_or_default();
                    if created {
                        self.message(format!("Registro manual {number} criado. Anexe o arquivo original em Documento ▸ Anexos, se houver."));
                    } else if crate::manual::unchanged(&receipt) {
                        self.message("Nada mudou no registro; nenhuma versão nova foi salva.");
                    } else {
                        self.message(format!("Nova versão do registro {number} salva. As anteriores ficam em Mais ▸ Versões."));
                    }
                }
                Err(error) => self.manual_form.message = error.to_string(),
            }
        } else if cancel {
            self.manual_form = ManualForm::default();
        } else if !open {
            if self.manual_form.dirty() {
                // The X never discards silently.
                self.manual_form.message =
                    "Há alterações não salvas: clique em Salvar ou em Cancelar para descartar."
                        .into();
            } else {
                self.manual_form = ManualForm::default();
            }
        }
    }

    /// The "Importar do e-mail" window. Every exchange with the mailbox
    /// runs on the worker thread; the window only asks and receives.
    pub(super) fn mail_window(&mut self, ctx: &egui::Context) {
        if !self.mail.open {
            return;
        }
        // The window says what is running: its own exchange with the
        // mailbox, or something else (an AI draft, a backup) to wait for.
        let busy = match &self.worker {
            None => crate::mail_ui::MailBusy::Free,
            Some(worker) if worker.kind == WorkKind::Mail => crate::mail_ui::MailBusy::Mail,
            Some(_) => crate::mail_ui::MailBusy::Other,
        };
        let action = self.mail.show(ctx, &mut self.store, busy);
        match action {
            crate::mail_ui::MailAction::None => {}
            crate::mail_ui::MailAction::ListFolders(account) => {
                self.spawn(
                    format!("Conectando a {}…", account.host),
                    WorkKind::Mail,
                    move |reporter| {
                        WorkResult::MailFolders(
                            crate::imap::list_folders(&account, &reporter)
                                .map_err(|e| format!("{e:#}")),
                        )
                    },
                    ctx,
                );
            }
            crate::mail_ui::MailAction::ListMessages {
                account,
                folder,
                filter,
            } => {
                self.spawn(
                    format!("Listando mensagens de {folder}…"),
                    WorkKind::Mail,
                    move |reporter| {
                        WorkResult::MailMessages(
                            crate::imap::list_messages(&account, &folder, filter, 100, &reporter)
                                .map_err(|e| format!("{e:#}")),
                        )
                    },
                    ctx,
                );
            }
            crate::mail_ui::MailAction::Fetch {
                account,
                folder,
                uids,
            } => {
                // The records say in which account the messages arrived.
                let source = crate::mail::MailSource::Account {
                    name: account.name.clone(),
                    address: account.user.trim().to_owned(),
                };
                self.spawn(
                    format!("Baixando {}…", plural(uids.len(), "mensagem", "mensagens")),
                    WorkKind::Mail,
                    move |reporter| {
                        WorkResult::MailFetched(
                            crate::imap::fetch_messages(&account, &folder, &uids, &reporter)
                                .map_err(|e| format!("{e:#}")),
                            source,
                        )
                    },
                    ctx,
                );
            }
        }
    }

    pub(super) fn confirmations(&mut self, ctx: &egui::Context) {
        if self.replacement.is_some() {
            let mut replace = false;
            let mut cancel = false;
            egui::Window::new("Substituir a minuta no editor?").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("O editor contém alterações não salvas. Salve uma versão se quiser preservá-las.");
                ui.horizontal(|ui| { cancel = ui.button("Voltar e preservar edição").clicked(); replace = ui.button("Substituir edição não salva").clicked(); });
            });
            if cancel {
                self.replacement = None;
            }
            if replace {
                if let Some(replacement) = self.replacement.take() {
                    self.apply_replacement(replacement);
                }
            }
        }
        if self.leave.is_some() {
            let title = self
                .tasks
                .editing_title()
                .unwrap_or_else(|| "tarefa".to_owned());
            let mut save = false;
            let mut discard = false;
            let mut stay = false;
            egui::Window::new("Tarefa com alterações não salvas")
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("A tarefa “{title}” tem alterações não salvas."));
                    ui.small("Salvar e continuar grava a tarefa e vai para onde você clicou. Descartar alterações volta ao que estava salvo (tarefa nova é apagada). Voltar à tarefa deixa tudo como está.");
                    if let Some(error) = &self.leave_error {
                        ui.add_space(4.0);
                        ui.colored_label(crate::theme::error(), error);
                        ui.small("Corrija na ficha da tarefa (Voltar à tarefa) ou descarte as alterações.");
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        save = ui.button("Salvar e continuar").clicked();
                        discard = ui.button("Descartar alterações").clicked();
                        stay = ui.button("Voltar à tarefa").clicked();
                    });
                });
            if stay {
                self.leave = None;
                self.leave_error = None;
                self.return_after_select = None;
                self.area = Area::Tasks;
            }
            if save {
                match self.tasks.save_draft(&mut self.store) {
                    Ok(()) => {
                        if let Some(leave) = self.leave.take() {
                            self.leave_error = None;
                            self.perform_leave(leave);
                        }
                    }
                    Err(error) => self.leave_error = Some(error),
                }
            }
            if discard {
                self.tasks.discard_draft();
                if let Some(leave) = self.leave.take() {
                    self.leave_error = None;
                    self.perform_leave(leave);
                }
            }
        }
        if self.exit_confirm {
            let mut close = false;
            let mut cancel = false;
            let mut go: Option<PendingTarget> = None;
            let pending = self.pending_items();
            egui::Window::new("Fechar o Ponte?")
                .collapsible(false)
                .resizable(false)
                .default_width(520.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Ainda há pendências. Ao fechar agora, elas serão perdidas:");
                    for item in &pending {
                        ui.horizontal(|ui| {
                            ui.label(format!("• {}", item.label));
                            if !matches!(item.go, PendingTarget::None)
                                && ui.small_button("Ir para").clicked()
                            {
                                go = Some(item.go);
                            }
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        cancel = ui.button("Voltar e salvar").clicked();
                        close = ui.button("Fechar mesmo assim").clicked();
                    });
                });
            if let Some(target) = go {
                self.exit_confirm = false;
                self.go_to(target);
            }
            if cancel {
                self.exit_confirm = false;
            }
            if close {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
