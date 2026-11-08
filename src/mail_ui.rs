//! "Importar do e-mail" window: pick a saved account, a folder and a filter,
//! see the newest messages, tick some and import them.
//!
//! Network work never runs here. `MailPanel::show` returns a `MailAction`
//! request; the app runs it on a worker thread and hands the outcome back
//! through `accept_folders`, `accept_messages` and `accept_imports`.
use crate::{
    imap::{Filter, Folder, MailAccount, MessageSummary, DEFAULT_PORT},
    mail::MailImport,
    storage::Store,
};
use eframe::egui::{self, Color32, RichText};
use std::collections::{BTreeMap, BTreeSet};

const SUCCESS: Color32 = Color32::from_rgb(30, 120, 60);
const WARNING: Color32 = Color32::from_rgb(153, 88, 16);
const ERROR: Color32 = Color32::from_rgb(168, 40, 45);

/// A request for the app to run on a worker thread.
pub enum MailAction {
    None,
    /// Connect and list folders of this account (also validates the credentials).
    ListFolders(MailAccount),
    /// List the newest messages of a folder.
    ListMessages {
        account: MailAccount,
        folder: String,
        filter: Filter,
    },
    /// Fetch these messages (uids) for import.
    Fetch {
        account: MailAccount,
        folder: String,
        uids: Vec<u32>,
    },
}

/// Why the window's controls may be locked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MailBusy {
    Free,
    /// A mail-server request is running.
    Mail,
    /// Some other operation of the app is running.
    Other,
}

/// The inline account form (creating or editing).
struct AccountForm {
    account: MailAccount,
    port_text: String,
    /// Values when the form was opened, to tell whether there are edits.
    original: (MailAccount, String),
}

impl AccountForm {
    fn new(account: MailAccount) -> Self {
        let port_text = account.port.to_string();
        Self {
            original: (account.clone(), port_text.clone()),
            account,
            port_text,
        }
    }
    fn dirty(&self) -> bool {
        self.account != self.original.0 || self.port_text != self.original.1
    }
}

#[derive(Default)]
pub struct MailPanel {
    pub open: bool,
    accounts: Vec<MailAccount>,
    accounts_loaded: bool,
    selected: Option<String>,
    form: Option<AccountForm>,
    /// Typed passwords of accounts that are not remembered, by account uid.
    session_passwords: BTreeMap<String, String>,
    folders: Vec<Folder>,
    /// `raw` name of the chosen folder.
    folder_raw: String,
    /// Folder whose listing was requested (set when "Listar" is clicked).
    listing_folder: Option<String>,
    /// Folder the listed messages belong to: uids only mean something
    /// inside it, so a fetch always uses this one, whatever the combo shows.
    messages_folder: String,
    filter: Option<Filter>,
    messages: Vec<MessageSummary>,
    messages_loaded: bool,
    checked: BTreeSet<u32>,
    /// Uids whose Message-ID is already in the library (computed once).
    imported: BTreeSet<u32>,
    summary: Vec<(Color32, String)>,
    /// Warning lines about messages left out of the running import (too
    /// large); they join the result list when the import answers.
    skipped_lines: Vec<String>,
    /// "Excluir" clicked on the account; waiting for confirmation.
    confirm_delete_account: bool,
    notice: Option<(String, bool)>,
    /// New messages arrived: compute `imported` on the next frame.
    pending_check: bool,
}

fn truncate(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= max {
        flat
    } else {
        let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

impl MailPanel {
    pub fn dirty(&self) -> bool {
        self.form.as_ref().is_some_and(AccountForm::dirty)
    }

    fn set_notice(&mut self, msg: impl Into<String>, error: bool) {
        self.notice = Some((msg.into(), error));
    }

    fn reload_accounts(&mut self, store: &Store) {
        match store.list_mail_accounts() {
            Ok(list) => {
                self.accounts = list;
                if let Some(uid) = &self.selected {
                    if !self.accounts.iter().any(|a| &a.uid == uid) {
                        self.selected = None;
                    }
                }
                if self.selected.is_none() {
                    self.selected = self.accounts.first().map(|a| a.uid.clone());
                    self.reset_mailbox();
                }
            }
            Err(e) => self.set_notice(
                format!("Não foi possível ler as contas de e-mail: {e}"),
                true,
            ),
        }
        self.accounts_loaded = true;
    }

    /// Forget folders, messages and results (the account or folder changed).
    fn reset_mailbox(&mut self) {
        self.folders.clear();
        self.folder_raw.clear();
        self.clear_messages();
    }

    fn clear_messages(&mut self) {
        self.messages.clear();
        self.messages_loaded = false;
        self.checked.clear();
        self.imported.clear();
        self.summary.clear();
        self.skipped_lines.clear();
    }

    /// The selected account with the session password filled in when needed.
    fn effective_account(&self) -> Option<MailAccount> {
        let uid = self.selected.as_ref()?;
        let mut account = self.accounts.iter().find(|a| &a.uid == uid)?.clone();
        if account.password.is_empty() {
            if let Some(p) = self.session_passwords.get(uid) {
                account.password = p.clone();
            }
        }
        Some(account)
    }

    pub fn accept_folders(&mut self, result: Result<Vec<Folder>, String>) {
        match result {
            Ok(list) => {
                self.folders = list;
                self.clear_messages();
                let wanted = self
                    .effective_account()
                    .map(|a| a.folder)
                    .unwrap_or_default();
                let selectable = || self.folders.iter().filter(|f| !f.no_select);
                let pick = selectable()
                    .find(|f| f.raw == wanted || f.name == wanted)
                    .or_else(|| selectable().next())
                    .map(|f| f.raw.clone());
                self.folder_raw = pick.unwrap_or_default();
                let n = self.folders.iter().filter(|f| !f.no_select).count();
                self.set_notice(format!("Conectado. {n} pasta(s) disponível(is)."), false);
            }
            Err(e) => self.set_notice(e, true),
        }
    }

    pub fn accept_messages(&mut self, result: Result<Vec<MessageSummary>, String>) {
        match result {
            Ok(list) => {
                self.clear_messages();
                self.messages = list;
                self.messages_loaded = true;
                self.messages_folder = self
                    .listing_folder
                    .take()
                    .unwrap_or_else(|| self.folder_raw.clone());
                self.notice = None;
                // `imported` is filled by `refresh_imported`, which needs the
                // store; it runs on the next frame (see `show`).
                self.imported.clear();
                self.pending_check = true;
            }
            Err(e) => self.set_notice(e, true),
        }
    }

    /// The download of the ticked messages failed before any was imported.
    pub fn accept_fetch_error(&mut self, error: String) {
        self.skipped_lines.clear();
        self.set_notice(
            format!("Não foi possível baixar as mensagens: {error}"),
            true,
        );
    }

    /// One entry per fetched uid: the import outcome or the error.
    pub fn accept_imports(&mut self, results: Vec<(u32, Result<MailImport, String>)>) {
        self.summary.clear();
        for line in std::mem::take(&mut self.skipped_lines) {
            self.summary.push((WARNING, line));
        }
        let (mut ok, mut failed) = (0usize, 0usize);
        for (uid, result) in results {
            let subject = self
                .messages
                .iter()
                .find(|m| m.uid == uid)
                .map(|m| m.subject.clone())
                .unwrap_or_default();
            match result {
                Ok(done) => {
                    let title = if done.subject.trim().is_empty() {
                        subject
                    } else {
                        done.subject.clone()
                    };
                    let title = truncate(&title, 80);
                    self.checked.remove(&uid);
                    self.imported.insert(uid);
                    if done.already_imported {
                        let line = if done.attachments_added > 0 {
                            format!(
                                "{title}: já estava no acervo; {} anexo(s) que faltava(m) acrescentado(s)",
                                done.attachments_added
                            )
                        } else {
                            format!("{title}: já estava no acervo")
                        };
                        self.summary.push((WARNING, line));
                    } else {
                        let mut line = format!("✓ {title} — {} anexo(s)", done.attachments_added);
                        if !done.attachments_skipped.is_empty() {
                            line.push_str(&format!(
                                ", {} não anexado(s)",
                                done.attachments_skipped.len()
                            ));
                        }
                        self.summary.push((SUCCESS, line));
                    }
                    for skipped in &done.attachments_skipped {
                        self.summary.push((
                            WARNING,
                            format!("    não anexado: {}", truncate(skipped, 120)),
                        ));
                    }
                    for note in &done.notes {
                        self.summary
                            .push((WARNING, format!("    {}", truncate(note, 160))));
                    }
                    ok += 1;
                }
                Err(e) => {
                    self.summary
                        .push((ERROR, format!("✗ {}: {e}", truncate(&subject, 80))));
                    failed += 1;
                }
            }
        }
        if failed == 0 {
            let text = if ok == 1 {
                "1 mensagem importada.".to_string()
            } else {
                format!("{ok} mensagens importadas.")
            };
            self.set_notice(text, false);
        } else {
            self.set_notice(
                format!("{ok} importada(s), {failed} com erro. Veja a lista abaixo."),
                true,
            );
        }
    }

    fn refresh_imported(&mut self, store: &Store) {
        self.imported.clear();
        for m in &self.messages {
            if matches!(store.mail_already_imported(&m.message_id), Ok(Some(_))) {
                self.imported.insert(m.uid);
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, store: &mut Store, busy: MailBusy) -> MailAction {
        if !self.open {
            return MailAction::None;
        }
        if !self.accounts_loaded {
            self.reload_accounts(store);
        }
        if self.pending_check {
            self.pending_check = false;
            self.refresh_imported(store);
        }
        let mut open = true;
        let mut action = MailAction::None;
        egui::Window::new("Importar do e-mail")
            .open(&mut open)
            .default_size([820.0, 600.0])
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        action = self.contents(ui, store, busy);
                    });
            });
        if !open {
            if self.dirty() {
                self.set_notice(
                    "Há alterações não salvas na conta: clique em Salvar ou em Cancelar antes de fechar.",
                    true,
                );
            } else {
                self.open = false;
            }
        }
        action
    }

    fn contents(&mut self, ui: &mut egui::Ui, store: &mut Store, state: MailBusy) -> MailAction {
        let busy = state != MailBusy::Free;
        let mut action = MailAction::None;

        // 4. Notice line (kept at the top).
        if let Some((msg, error)) = self.notice.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(if error { ERROR } else { SUCCESS }, msg);
                if ui
                    .small_button("✕")
                    .on_hover_text("Fechar o aviso")
                    .clicked()
                {
                    self.notice = None;
                }
            });
            ui.separator();
        }
        match state {
            MailBusy::Free => {}
            MailBusy::Mail => {
                ui.colored_label(
                    WARNING,
                    "Aguarde: o Ponte está conversando com o servidor de e-mail…",
                );
            }
            MailBusy::Other => {
                ui.colored_label(WARNING, "Aguarde a operação em andamento.");
            }
        }

        self.accounts_section(ui, store, busy);
        ui.separator();
        if self.form.is_none() {
            if let a @ (MailAction::ListFolders(_) | MailAction::ListMessages { .. }) =
                self.folder_section(ui, busy)
            {
                action = a;
            }
            ui.separator();
            if self.messages_loaded {
                if let a @ MailAction::Fetch { .. } = self.messages_section(ui, busy) {
                    action = a;
                }
            }
        }
        action
    }

    // -- 1. Accounts ------------------------------------------------------

    fn accounts_section(&mut self, ui: &mut egui::Ui, store: &mut Store, busy: bool) {
        ui.strong("Conta de e-mail");
        let mut new_account = false;
        let mut edit = false;
        let mut delete = false;
        let mut changed_to: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            let label = self
                .selected
                .as_ref()
                .and_then(|uid| self.accounts.iter().find(|a| &a.uid == uid))
                .map(|a| format!("{} — {}", a.name, a.user))
                .unwrap_or_else(|| "(nenhuma conta salva)".into());
            ui.add_enabled_ui(self.form.is_none() && !busy, |ui| {
                egui::ComboBox::from_id_salt("mail_account")
                    .width(320.0)
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for a in &self.accounts {
                            let is_sel = self.selected.as_deref() == Some(a.uid.as_str());
                            if ui
                                .selectable_label(is_sel, format!("{} — {}", a.name, a.user))
                                .clicked()
                                && !is_sel
                            {
                                changed_to = Some(a.uid.clone());
                            }
                        }
                    });
                if ui.button("Nova conta…").clicked() {
                    new_account = true;
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Editar…"))
                    .clicked()
                {
                    edit = true;
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Excluir"))
                    .clicked()
                {
                    delete = true;
                }
            });
        });
        if let Some(uid) = changed_to {
            self.selected = Some(uid);
            self.confirm_delete_account = false;
            self.reset_mailbox();
        }
        if new_account || edit {
            self.confirm_delete_account = false;
        }
        if new_account {
            self.form = Some(AccountForm::new(MailAccount {
                host: "imap.gmail.com".into(),
                port: DEFAULT_PORT,
                folder: "INBOX".into(),
                ..MailAccount::default()
            }));
        }
        if edit {
            if let Some(mut account) = self
                .selected
                .as_ref()
                .and_then(|uid| self.accounts.iter().find(|a| &a.uid == uid))
                .cloned()
            {
                if account.password.is_empty() {
                    if let Some(p) = self.session_passwords.get(&account.uid) {
                        account.password = p.clone();
                    }
                }
                self.form = Some(AccountForm::new(account));
            }
        }
        if delete {
            self.confirm_delete_account = true;
        }
        if self.confirm_delete_account && self.form.is_none() {
            let name = self
                .selected
                .as_ref()
                .and_then(|uid| self.accounts.iter().find(|a| &a.uid == uid))
                .map(|a| a.name.clone())
                .unwrap_or_default();
            ui.colored_label(
                WARNING,
                format!(
                    "Excluir a conta “{name}” deste computador? A senha guardada também é apagada."
                ),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Sim, excluir"))
                    .clicked()
                {
                    self.confirm_delete_account = false;
                    if let Some(uid) = self.selected.clone() {
                        match store.delete_mail_account(&uid) {
                            Ok(()) => {
                                self.session_passwords.remove(&uid);
                                self.selected = None;
                                self.reload_accounts(store);
                                self.reset_mailbox();
                                self.set_notice("Conta excluída.", false);
                            }
                            Err(e) => self
                                .set_notice(format!("Não foi possível excluir a conta: {e}"), true),
                        }
                    }
                }
                if ui.button("Cancelar").clicked() {
                    self.confirm_delete_account = false;
                }
            });
        }

        if self.form.is_some() {
            self.form_ui(ui, store);
        } else if let Some(uid) = self.selected.clone() {
            // A remembered-less account needs the password once per session.
            let stored_empty = self
                .accounts
                .iter()
                .find(|a| a.uid == uid)
                .is_some_and(|a| a.password.is_empty());
            if stored_empty {
                ui.horizontal(|ui| {
                    ui.label("Senha de app desta sessão");
                    let entry = self.session_passwords.entry(uid).or_default();
                    ui.add(
                        egui::TextEdit::singleline(entry)
                            .password(true)
                            .desired_width(240.0),
                    );
                });
            }
        }
        ui.small(
            "No Gmail pessoal, ative a verificação em duas etapas e crie uma senha de app em Conta Google ▸ Segurança ▸ Senhas de app. A senha normal não funciona. O Ponte só lê; nada é marcado como lido, movido ou apagado.",
        );
    }

    fn form_ui(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        let mut save = false;
        let mut cancel = false;
        if let Some(form) = self.form.as_mut() {
            ui.group(|ui| {
                egui::Grid::new("mail_account_form")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Nome");
                        ui.add(egui::TextEdit::singleline(&mut form.account.name).desired_width(300.0));
                        ui.end_row();
                        ui.label("Servidor IMAP");
                        ui.add(egui::TextEdit::singleline(&mut form.account.host).desired_width(300.0));
                        ui.end_row();
                        ui.label("Porta");
                        ui.add(egui::TextEdit::singleline(&mut form.port_text).desired_width(70.0));
                        ui.end_row();
                        ui.label("Usuário (e-mail)");
                        ui.add(egui::TextEdit::singleline(&mut form.account.user).desired_width(300.0));
                        ui.end_row();
                        ui.label("Senha de app");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.account.password)
                                .password(true)
                                .desired_width(300.0),
                        );
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut form.account.remember, "Lembrar a senha neste computador")
                            .on_hover_text(
                                "A senha fica guardada, selada, no acervo deste computador. Sem marcar, ela é pedida a cada sessão.",
                            );
                        ui.end_row();
                        ui.label("Pasta padrão");
                        ui.add(egui::TextEdit::singleline(&mut form.account.folder).desired_width(300.0));
                        ui.end_row();
                    });
                ui.horizontal(|ui| {
                    if ui.button("Salvar").clicked() {
                        save = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                    if form.dirty() {
                        ui.colored_label(WARNING, "● não salvo");
                    }
                });
            });
        }
        if cancel {
            self.form = None;
        }
        if save {
            let Some(form) = self.form.as_ref() else {
                return;
            };
            let mut account = form.account.clone();
            match form.port_text.trim().parse::<u16>() {
                Ok(p) => account.port = p,
                Err(_) => {
                    self.set_notice("Porta inválida.", true);
                    return;
                }
            }
            account.name = account.name.trim().to_string();
            account.host = account.host.trim().to_string();
            account.user = account.user.trim().to_string();
            account.folder = account.folder.trim().to_string();
            match store.save_mail_account(&account) {
                Ok(saved) => {
                    if saved.remember || saved.password.is_empty() {
                        self.session_passwords.remove(&saved.uid);
                    } else {
                        self.session_passwords
                            .insert(saved.uid.clone(), saved.password.clone());
                    }
                    self.form = None;
                    self.selected = Some(saved.uid.clone());
                    self.reload_accounts(store);
                    self.selected = Some(saved.uid);
                    self.reset_mailbox();
                    self.set_notice("Conta salva.", false);
                }
                Err(e) => self.set_notice(format!("Não foi possível salvar a conta: {e}"), true),
            }
        }
    }

    // -- 2. Folder and filter ---------------------------------------------

    fn folder_section(&mut self, ui: &mut egui::Ui, busy: bool) -> MailAction {
        let mut action = MailAction::None;
        ui.strong("Pasta e filtro");
        let account = self.effective_account();
        ui.horizontal_wrapped(|ui| {
            let can_connect = account.is_some() && !busy;
            if ui
                .add_enabled(can_connect, egui::Button::new("Conectar e listar pastas"))
                .clicked()
            {
                if let Some(a) = account.clone() {
                    if a.password.is_empty() {
                        self.notice = Some((
                            "Digite a senha de app desta conta antes de conectar.".into(),
                            true,
                        ));
                    } else {
                        action = MailAction::ListFolders(a);
                    }
                }
            }
            if account.is_none() {
                ui.label("Salve uma conta para começar.");
            }
        });
        if !self.folders.is_empty() {
            let filter = self.filter.get_or_insert(Filter::All);
            let current = self
                .folders
                .iter()
                .find(|f| f.raw == self.folder_raw)
                .map(|f| f.name.clone())
                .unwrap_or_else(|| "(escolha)".into());
            let mut folder_changed = false;
            ui.horizontal_wrapped(|ui| {
                // While the server is being read, the choice stays put.
                ui.add_enabled_ui(!busy, |ui| {
                    ui.label("Pasta");
                    egui::ComboBox::from_id_salt("mail_folder")
                        .width(280.0)
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for f in self.folders.iter().filter(|f| !f.no_select) {
                                if ui
                                    .selectable_label(self.folder_raw == f.raw, &f.name)
                                    .clicked()
                                    && self.folder_raw != f.raw
                                {
                                    self.folder_raw = f.raw.clone();
                                    folder_changed = true;
                                }
                            }
                        });
                    ui.label("Filtro");
                    egui::ComboBox::from_id_salt("mail_filter")
                        .selected_text(filter.label())
                        .show_ui(ui, |ui| {
                            for f in [Filter::All, Filter::Flagged, Filter::Unseen] {
                                ui.selectable_value(filter, f, f.label());
                            }
                        });
                });
                let can_list = account.is_some() && !busy && !self.folder_raw.is_empty();
                if ui
                    .add_enabled(can_list, egui::Button::new("Listar mensagens"))
                    .clicked()
                {
                    if let Some(a) = account.clone() {
                        self.listing_folder = Some(self.folder_raw.clone());
                        action = MailAction::ListMessages {
                            account: a,
                            folder: self.folder_raw.clone(),
                            filter: *filter,
                        };
                    }
                }
            });
            if folder_changed {
                self.clear_messages();
            }
            ui.small("Listadas as 100 mais recentes.");
        }
        action
    }

    // -- 3. Messages ------------------------------------------------------

    fn messages_section(&mut self, ui: &mut egui::Ui, busy: bool) -> MailAction {
        let mut action = MailAction::None;
        ui.strong("Mensagens");
        if self.messages.is_empty() {
            ui.label("Nenhuma mensagem encontrada com esse filtro.");
        } else {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Marcar todas").clicked() {
                    self.checked = self.messages.iter().map(|m| m.uid).collect();
                }
                if ui.button("Desmarcar").clicked() {
                    self.checked.clear();
                }
                if ui.button("Marcar só as não importadas").clicked() {
                    self.checked = self
                        .messages
                        .iter()
                        .filter(|m| !self.imported.contains(&m.uid))
                        .map(|m| m.uid)
                        .collect();
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("mail_messages")
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Grid::new("mail_messages_grid")
                        .num_columns(7)
                        .striped(true)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            for h in ["", "Data", "De", "Assunto", "", "Tamanho", "No acervo"] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            for m in &self.messages {
                                let mut on = self.checked.contains(&m.uid);
                                if ui.checkbox(&mut on, "").changed() {
                                    if on {
                                        self.checked.insert(m.uid);
                                    } else {
                                        self.checked.remove(&m.uid);
                                    }
                                }
                                let text = |s: String| {
                                    let t = RichText::new(s);
                                    if m.seen {
                                        t
                                    } else {
                                        t.strong()
                                    }
                                };
                                ui.label(text(m.date.clone()));
                                ui.label(text(truncate(&m.from, 32))).on_hover_text(&m.from);
                                let subject = if m.subject.trim().is_empty() {
                                    "(sem assunto)".to_string()
                                } else {
                                    truncate(&m.subject, 60)
                                };
                                ui.label(text(subject)).on_hover_text(&m.subject);
                                if m.flagged {
                                    ui.colored_label(WARNING, "★");
                                } else {
                                    ui.label("");
                                }
                                ui.label(crate::labels::file_size(m.size as u64));
                                if self.imported.contains(&m.uid) {
                                    ui.colored_label(WARNING, "já importada");
                                } else {
                                    ui.label("");
                                }
                                ui.end_row();
                            }
                        });
                });
            let count = self.checked.len();
            let can_fetch = count > 0 && !busy;
            if ui
                .add_enabled(
                    can_fetch,
                    egui::Button::new(if count == 1 {
                        "Importar 1 marcada".to_string()
                    } else {
                        format!("Importar {count} marcadas")
                    }),
                )
                .clicked()
            {
                if let (Some(account), false) =
                    (self.effective_account(), self.messages_folder.is_empty())
                {
                    let limit = crate::imap::MAX_MESSAGE_BYTES;
                    // Messages above the size limit stay for the .eml path.
                    let too_big: Vec<String> = self
                        .messages
                        .iter()
                        .filter(|m| self.checked.contains(&m.uid) && m.size > limit)
                        .map(|m| truncate(&m.subject, 40))
                        .collect();
                    let uids: Vec<u32> = self
                        .messages
                        .iter()
                        .filter(|m| self.checked.contains(&m.uid) && m.size <= limit)
                        .map(|m| m.uid)
                        .collect();
                    // The warning goes to the result list (the import answer
                    // replaces the notice line) and the left-out messages are
                    // unticked so a second click does not repeat it.
                    self.skipped_lines.clear();
                    if !too_big.is_empty() {
                        let line = format!(
                            "Mensagens acima de {} MB ficam de fora (salve-as como .eml): {}.",
                            limit / (1024 * 1024),
                            too_big.join("; ")
                        );
                        self.checked.retain(|uid| {
                            !self
                                .messages
                                .iter()
                                .any(|m| m.uid == *uid && m.size > limit)
                        });
                        if uids.is_empty() {
                            self.summary.clear();
                            self.summary.push((WARNING, line));
                        } else {
                            self.skipped_lines.push(line);
                        }
                    }
                    if !uids.is_empty() {
                        action = MailAction::Fetch {
                            account,
                            folder: self.messages_folder.clone(),
                            uids,
                        };
                    }
                }
            }
        }
        if !self.summary.is_empty() {
            ui.separator();
            ui.strong("Resultado da importação");
            for (color, line) in &self.summary {
                ui.colored_label(*color, line);
            }
        }
        action
    }
}
