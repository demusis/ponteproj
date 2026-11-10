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
use crate::theme::{self, icon};
use crate::widgets::{self, Tone};
use eframe::egui::{self, Color32, RichText};
use std::collections::{BTreeMap, BTreeSet};


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
    /// The short window of the "E-mails" button: only the message list of
    /// the last account, folder and filter used.
    quick: bool,
    /// A listing asked for by the "E-mails" button: account, folder and
    /// filter. It becomes a request (and the folder it will fill is set)
    /// only when nothing else is running.
    pending: Option<(MailAccount, String, Filter)>,
    /// The folder and filter of the listing in progress, remembered in the
    /// account once it succeeds.
    remember: Option<(String, String, Filter)>,
    /// Top of the window contents this frame (see the message list height).
    contents_top: f32,
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
    /// The outcome of the last import in one line (text, all went well).
    result: Option<(String, bool)>,
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

    /// The full window (accounts, folders, filter, messages).
    pub fn open_full(&mut self) {
        self.open = true;
        self.quick = false;
        self.pending = None;
    }

    /// The "E-mails" button: lists at once the messages of the account used
    /// last, in its last folder and filter. Without an account or a
    /// password for this session, the full window opens instead, saying
    /// what is missing.
    pub fn open_quick(&mut self, store: &Store, busy: MailBusy) {
        // An exchange with the mailbox is under way (a listing or a download
        // from the full window): the window only shows it; changing account,
        // folder or filter now would mix its answer with another request.
        // An account being edited stays in front, too.
        if busy == MailBusy::Mail || self.form.is_some() {
            self.open = true;
            return;
        }
        self.open = true;
        self.reload_accounts(store);
        // The account listed last; with none listed yet, the one selected.
        let selected = self.selected.clone();
        let Some(account) = self
            .accounts
            .iter()
            .max_by_key(|a| (a.last_used.clone(), Some(&a.uid) == selected.as_ref()))
            .cloned()
        else {
            self.quick = false;
            self.set_notice("Cadastre uma conta de e-mail para começar.", false);
            return;
        };
        if self.selected.as_deref() != Some(account.uid.as_str()) {
            self.selected = Some(account.uid.clone());
            self.reset_mailbox();
        }
        let Some(effective) = self.effective_account().filter(|a| !a.password.is_empty()) else {
            self.quick = false;
            self.set_notice(
                "Digite a senha de app desta conta (ou marque para lembrá-la neste computador).",
                true,
            );
            return;
        };
        self.quick = true;
        self.notice = None;
        self.folder_raw = account.folder.clone();
        self.filter = Some(account.filter);
        self.pending = Some((effective, account.folder, account.filter));
    }

    /// A listing of `folder`, remembered in the account when it succeeds.
    fn list_action(&mut self, account: MailAccount, folder: String, filter: Filter) -> MailAction {
        self.listing_folder = Some(folder.clone());
        self.remember = Some((account.uid.clone(), folder.clone(), filter));
        MailAction::ListMessages {
            account,
            folder,
            filter,
        }
    }

    /// After a successful listing, the account keeps its folder and filter.
    pub fn remember_choice(&mut self, store: &mut Store) {
        if let Some((uid, folder, filter)) = self.remember.take() {
            if let Err(e) = store.remember_mail_choice(&uid, &folder, filter) {
                self.set_notice(format!("Não foi possível lembrar a pasta: {e}"), true);
            } else if let Some(account) = self.accounts.iter_mut().find(|a| a.uid == uid) {
                account.folder = folder;
                account.filter = filter;
                account.last_used = chrono::Utc::now().to_rfc3339();
            }
        }
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
        self.result = None;
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
            Err(e) => {
                self.remember = None;
                self.listing_folder = None;
                self.set_notice(e, true);
            }
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
            self.summary.push((crate::theme::warning(), line));
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
                        self.summary.push((crate::theme::warning(), line));
                    } else {
                        let mut line = format!("✓ {title} — {} anexo(s)", done.attachments_added);
                        if !done.attachments_skipped.is_empty() {
                            line.push_str(&format!(
                                ", {} não anexado(s)",
                                done.attachments_skipped.len()
                            ));
                        }
                        self.summary.push((crate::theme::success(), line));
                    }
                    for skipped in &done.attachments_skipped {
                        self.summary.push((
                            crate::theme::warning(),
                            format!("    não anexado: {}", truncate(skipped, 120)),
                        ));
                    }
                    for note in &done.notes {
                        self.summary
                            .push((crate::theme::warning(), format!("    {}", truncate(note, 160))));
                    }
                    ok += 1;
                }
                Err(e) => {
                    self.summary
                        .push((crate::theme::error(), format!("✗ {}: {e}", truncate(&subject, 80))));
                    failed += 1;
                }
            }
        }
        // The outcome goes where the person is looking, above the list
        // (see `result_block`), not in the notice line at the top.
        self.notice = None;
        self.result = Some(if failed == 0 {
            (
                if ok == 1 {
                    "✓ 1 mensagem importada.".to_string()
                } else {
                    format!("✓ {ok} mensagens importadas.")
                },
                true,
            )
        } else if ok == 0 {
            (
                if failed == 1 {
                    "✗ A mensagem não foi importada.".to_string()
                } else {
                    format!("✗ Nenhuma das {failed} mensagens foi importada.")
                },
                false,
            )
        } else {
            (format!("⚠ {ok} importada(s), {failed} com erro."), false)
        });
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
        // A listing asked for while something else ran goes now.
        if busy == MailBusy::Free {
            if let Some((account, folder, filter)) = self.pending.take() {
                action = self.list_action(account, folder, filter);
            }
        }
        let title = if self.quick {
            "E-mails"
        } else {
            "Importar do e-mail"
        };
        egui::Window::new(title)
            .id(egui::Id::NULL.with("mail_window"))
            .open(&mut open)
            .default_size([820.0, 600.0])
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let shown = self.ui(ui, store, busy);
                        if !matches!(shown, MailAction::None) {
                            action = shown;
                        }
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
                // Nothing asked for is sent after the window is closed.
                self.pending = None;
            }
        }
        action
    }

    /// The same contents as the window, drawn in place (the Ajustes area
    /// embeds it). Never touches the network: it returns the request.
    pub fn ui(&mut self, ui: &mut egui::Ui, store: &mut Store, busy: MailBusy) -> MailAction {
        if !self.accounts_loaded {
            self.reload_accounts(store);
        }
        if self.pending_check {
            self.pending_check = false;
            self.refresh_imported(store);
        }
        if self.quick {
            self.quick_contents(ui, busy)
        } else {
            self.contents(ui, store, busy)
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, store: &mut Store, state: MailBusy) -> MailAction {
        // Where the contents start, to measure what is above the list
        // independently of the outer scroll position.
        self.contents_top = ui.cursor().top();
        let busy = state != MailBusy::Free;
        let mut action = MailAction::None;

        // Notice line (kept at the top).
        if let Some((msg, error)) = self.notice.clone() {
            if notice_line(ui, &msg, error) {
                self.notice = None;
            }
            ui.add_space(theme::S2);
        }
        busy_note(ui, state, false);

        self.accounts_section(ui, store, busy);
        ui.add_space(theme::S3);
        if self.form.is_none() {
            if let a @ (MailAction::ListFolders(_) | MailAction::ListMessages { .. }) =
                self.folder_section(ui, busy)
            {
                action = a;
            }
            ui.add_space(theme::S3);
            if self.messages_loaded {
                if let a @ MailAction::Fetch { .. } = self.messages_section(ui, busy) {
                    action = a;
                }
            }
        }
        action
    }

    /// The short window: one line saying where the list comes from, then
    /// the list itself.
    fn quick_contents(&mut self, ui: &mut egui::Ui, state: MailBusy) -> MailAction {
        self.contents_top = ui.cursor().top();
        let busy = state != MailBusy::Free;
        let mut action = MailAction::None;
        if let Some((msg, error)) = self.notice.clone() {
            if notice_line(ui, &msg, error) {
                self.notice = None;
            }
            ui.add_space(theme::S2);
        }
        busy_note(ui, state, true);
        let account = self.effective_account();
        let filter = self.filter.unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            let name = account.as_ref().map(|a| a.name.clone()).unwrap_or_default();
            ui.label(RichText::new(name).font(theme::font_strong(theme::SIZE_BODY)));
            widgets::pill(ui, crate::imap::folder_display_name(&self.folder_raw), Tone::Neutral);
            widgets::pill(ui, filter.label(), Tone::Neutral);
            if ui
                .add_enabled(
                    !busy && account.is_some(),
                    widgets::secondary(format!("{} Atualizar", icon::SYNC)),
                )
                .clicked()
            {
                if let Some(a) = account.clone() {
                    let folder = self.folder_raw.clone();
                    action = self.list_action(a, folder, filter);
                }
            }
            if ui
                .add_enabled(!busy, widgets::quiet("Trocar pasta ou conta…"))
                .on_hover_text("Abre a janela completa: contas, pastas e filtro.")
                .clicked()
            {
                self.quick = false;
            }
        });
        ui.add_space(theme::S2);
        if self.messages_loaded {
            if let a @ MailAction::Fetch { .. } = self.messages_section(ui, busy) {
                action = a;
            }
        } else if !busy && self.notice.is_some() {
            widgets::muted(
                ui,
                "A lista não pôde ser lida. Use Atualizar ou Trocar pasta ou conta…",
            );
        }
        action
    }

    // -- 1. Accounts ------------------------------------------------------

    fn accounts_section(&mut self, ui: &mut egui::Ui, store: &mut Store, busy: bool) {
        let mut new_account = false;
        let mut edit = false;
        let mut delete = false;
        let mut changed_to: Option<String> = None;
        widgets::card(ui, |ui| {
            widgets::section(ui, "Conta de e-mail");
            ui.add_space(theme::S2);
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
                    if ui
                        .add(widgets::secondary(format!("{} Nova conta", icon::PLUS)))
                        .clicked()
                    {
                        new_account = true;
                    }
                    if ui
                        .add_enabled(self.selected.is_some(), widgets::quiet("Editar"))
                        .clicked()
                    {
                        edit = true;
                    }
                    if ui
                        .add_enabled(self.selected.is_some(), widgets::danger("Excluir"))
                        .clicked()
                    {
                        delete = true;
                    }
                });
            });
            if let Some(uid) = changed_to.take() {
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
                ui.add_space(theme::S2);
                soft_note(
                    ui,
                    Tone::Warn,
                    icon::WARNING,
                    &format!(
                        "Excluir a conta “{name}” deste computador? A senha guardada também é apagada."
                    ),
                );
                ui.add_space(theme::S1);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!busy, widgets::danger("Sim, excluir"))
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
                                Err(e) => self.set_notice(
                                    format!("Não foi possível excluir a conta: {e}"),
                                    true,
                                ),
                            }
                        }
                    }
                    if ui.add(widgets::secondary("Cancelar")).clicked() {
                        self.confirm_delete_account = false;
                    }
                });
            }

            if self.form.is_some() {
                ui.add_space(theme::S3);
                self.form_ui(ui, store);
            } else if let Some(uid) = self.selected.clone() {
                // A remembered-less account needs the password once per session.
                let stored_empty = self
                    .accounts
                    .iter()
                    .find(|a| a.uid == uid)
                    .is_some_and(|a| a.password.is_empty());
                if stored_empty {
                    ui.add_space(theme::S2);
                    widgets::field_label(ui, "Senha de app desta sessão");
                    let entry = self.session_passwords.entry(uid).or_default();
                    ui.add(
                        egui::TextEdit::singleline(entry)
                            .password(true)
                            .desired_width(280.0),
                    );
                }
            }
            ui.add_space(theme::S2);
            widgets::small_muted(
                ui,
                "No Gmail pessoal, ative a verificação em duas etapas e crie uma senha de app em Conta Google ▸ Segurança ▸ Senhas de app. A senha normal não funciona. O Ponte só lê; nada é marcado como lido, movido ou apagado.",
            );
        });
    }

    fn form_ui(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        let mut save = false;
        let mut cancel = false;
        if let Some(form) = self.form.as_mut() {
            egui::Frame::new()
                .fill(theme::c().sunken)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.columns(2, |cols| {
                        let width = cols[0].available_width();
                        widgets::field_label(&mut cols[0], "Nome");
                        cols[0].add(egui::TextEdit::singleline(&mut form.account.name).desired_width(width));
                        let width = cols[1].available_width();
                        widgets::field_label(&mut cols[1], "Pasta (a última listada)");
                        cols[1].add(egui::TextEdit::singleline(&mut form.account.folder).desired_width(width));
                    });
                    ui.add_space(theme::S2);
                    ui.columns(2, |cols| {
                        let width = cols[0].available_width();
                        widgets::field_label(&mut cols[0], "Servidor IMAP");
                        cols[0].add(egui::TextEdit::singleline(&mut form.account.host).desired_width(width));
                        widgets::field_label(&mut cols[1], "Porta");
                        cols[1].add(egui::TextEdit::singleline(&mut form.port_text).desired_width(70.0));
                    });
                    ui.add_space(theme::S2);
                    ui.columns(2, |cols| {
                        let width = cols[0].available_width();
                        widgets::field_label(&mut cols[0], "Usuário (e-mail)");
                        cols[0].add(egui::TextEdit::singleline(&mut form.account.user).desired_width(width));
                        let width = cols[1].available_width();
                        widgets::field_label(&mut cols[1], "Senha de app");
                        cols[1].add(
                            egui::TextEdit::singleline(&mut form.account.password)
                                .password(true)
                                .desired_width(width),
                        );
                    });
                    ui.add_space(theme::S2);
                    ui.checkbox(&mut form.account.remember, "Lembrar a senha neste computador")
                        .on_hover_text(
                            "A senha fica guardada, selada, no acervo deste computador. Sem marcar, ela é pedida a cada sessão.",
                        );
                    ui.add_space(theme::S3);
                    ui.horizontal(|ui| {
                        if ui.add(widgets::primary("Salvar")).clicked() {
                            save = true;
                        }
                        if ui.add(widgets::secondary("Cancelar")).clicked() {
                            cancel = true;
                        }
                        if form.dirty() {
                            widgets::pill(ui, "Não salvo", Tone::Warn);
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
        let account = self.effective_account();
        widgets::card(ui, |ui| {
            widgets::section(ui, "Pasta e filtro");
            ui.add_space(theme::S2);
            ui.horizontal_wrapped(|ui| {
                let can_connect = account.is_some() && !busy;
                let connect = widgets::secondary(format!("{} Conectar e listar pastas", icon::SYNC));
                if ui.add_enabled(can_connect, connect).clicked() {
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
                    widgets::muted(ui, "Salve uma conta para começar.");
                }
            });
            let mut list_request = None;
            if !self.folders.is_empty() {
                ui.add_space(theme::S2);
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
                        widgets::field_label(ui, "Pasta");
                        egui::ComboBox::from_id_salt("mail_folder")
                            .width(260.0)
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
                        ui.add_space(theme::S2);
                        widgets::field_label(ui, "Filtro");
                        for f in [Filter::All, Filter::Flagged, Filter::Unseen] {
                            if widgets::chip(ui, *filter == f, f.label()).clicked() {
                                *filter = f;
                            }
                        }
                    });
                    let can_list = account.is_some() && !busy && !self.folder_raw.is_empty();
                    if ui
                        .add_enabled(can_list, widgets::secondary("Listar mensagens"))
                        .clicked()
                    {
                        if let Some(a) = account.clone() {
                            list_request = Some((a, self.folder_raw.clone(), *filter));
                        }
                    }
                });
                if folder_changed {
                    self.clear_messages();
                }
                if let Some((a, folder, filter)) = list_request {
                    action = self.list_action(a, folder, filter);
                }
                widgets::small_muted(ui, "Listadas as 100 mais recentes.");
            }
        });
        action
    }

    // -- 3. Messages ------------------------------------------------------

    /// The outcome of the last import, in one soft-coloured frame with the
    /// details below it, above the message list so it is always in sight.
    fn result_block(&mut self, ui: &mut egui::Ui) {
        let Some((headline, ok)) = self.result.clone() else {
            return;
        };
        // The text keeps its old glyph; here the glyph becomes an icon.
        let partial = headline.starts_with('⚠');
        let text = headline
            .trim_start_matches(['✓', '✗', '⚠'])
            .trim_start()
            .to_owned();
        let (tone, glyph) = if ok {
            (Tone::Ok, icon::CHECK)
        } else if partial {
            (Tone::Warn, icon::WARNING)
        } else {
            (Tone::Danger, icon::ERROR)
        };
        let (fg, bg) = tone.colors();
        let mut close = false;
        egui::Frame::new()
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if widgets::icon_button(ui, icon::CLOSE, "Fechar o resultado").clicked() {
                        close = true;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(glyph).size(18.0).color(fg));
                                ui.label(
                                    RichText::new(text)
                                        .font(theme::font_strong(theme::SIZE_BODY))
                                        .color(fg),
                                );
                            });
                            if !self.summary.is_empty() {
                                egui::ScrollArea::vertical()
                                    .id_salt("mail_result")
                                    .max_height(110.0)
                                    .auto_shrink([false, true])
                                    .show(ui, |ui| {
                                        for (color, line) in &self.summary {
                                            let line = line
                                                .trim_start_matches(['✓', '✗', '⚠'])
                                                .trim_start_matches(' ');
                                            ui.add(
                                                egui::Label::new(RichText::new(line).color(*color))
                                                    .wrap(),
                                            );
                                        }
                                    });
                            }
                        });
                    });
                });
            });
        if close {
            self.result = None;
            self.summary.clear();
        }
        ui.add_space(theme::S2);
    }

    fn messages_section(&mut self, ui: &mut egui::Ui, busy: bool) -> MailAction {
        let mut action = MailAction::None;
        self.result_block(ui);
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::section(ui, "Mensagens");
                if !self.messages.is_empty() {
                    widgets::pill(ui, self.messages.len().to_string(), Tone::Neutral);
                }
            });
            if self.messages.is_empty() {
                ui.add_space(theme::S2);
                widgets::muted(ui, "Nenhuma mensagem encontrada com esse filtro.");
                return;
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add(widgets::quiet("Marcar todas")).clicked() {
                    self.checked = self.messages.iter().map(|m| m.uid).collect();
                }
                if ui.add(widgets::quiet("Desmarcar")).clicked() {
                    self.checked.clear();
                }
                if ui.add(widgets::quiet("Marcar só as não importadas")).clicked() {
                    self.checked = self
                        .messages
                        .iter()
                        .filter(|m| !self.imported.contains(&m.uid))
                        .map(|m| m.uid)
                        .collect();
                }
            });
            // The list takes the height the window has left, keeping room
            // for the footer and the result lines below it; it never gets
            // shorter than a few rows.
            let row = ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y;
            let below = ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0 + 48.0;
            // The window's visible height minus what is above the list, both
            // independent of the outer scroll (inside it the available height
            // is unbounded), so the list does not resize while scrolling.
            let visible = ui.clip_rect().height() - (ui.cursor().top() - self.contents_top);
            let list_height = (visible - below).max(row * 12.0);
            let p = theme::c();
            egui::ScrollArea::vertical()
                .id_salt("mail_messages")
                .max_height(list_height)
                .auto_shrink([false, true])
                .min_scrolled_height(list_height.min(260.0))
                .show(ui, |ui| {
                    for (index, m) in self.messages.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            let mut on = self.checked.contains(&m.uid);
                            if ui.checkbox(&mut on, "").changed() {
                                if on {
                                    self.checked.insert(m.uid);
                                } else {
                                    self.checked.remove(&m.uid);
                                }
                            }
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - 250.0).max(180.0));
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(truncate(&m.from, 40))
                                            .font(theme::font_strong(theme::SIZE_BODY)),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&m.from);
                                let subject = if m.subject.trim().is_empty() {
                                    "(sem assunto)".to_string()
                                } else {
                                    truncate(&m.subject, 80)
                                };
                                let text = if m.seen {
                                    RichText::new(subject).color(p.muted)
                                } else {
                                    RichText::new(subject)
                                };
                                ui.add(egui::Label::new(text).truncate())
                                    .on_hover_text(&m.subject);
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    widgets::small_muted(ui, m.date.clone());
                                    widgets::small_muted(ui, crate::labels::file_size(m.size as u64));
                                    if self.imported.contains(&m.uid) {
                                        ui.horizontal(|ui| {
                                            widgets::pill(ui, "Já no acervo", Tone::Warn)
                                        });
                                    }
                                    if m.flagged {
                                        ui.label(RichText::new(icon::STAR).size(16.0).color(p.warn));
                                    }
                                },
                            );
                        });
                    }
                });
            ui.add_space(theme::S2);
            let count = self.checked.len();
            let can_fetch = count > 0 && !busy;
            let mut fetch = false;
            ui.horizontal(|ui| {
                widgets::muted(
                    ui,
                    if count == 1 {
                        "1 marcada".to_string()
                    } else {
                        format!("{count} marcadas")
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let text = if count == 0 {
                        "Importar".to_string()
                    } else {
                        format!("Importar {count}")
                    };
                    if ui.add_enabled(can_fetch, widgets::primary(text)).clicked() {
                        fetch = true;
                    }
                });
            });
            if fetch {
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
                            self.summary.push((crate::theme::warning(), line));
                            self.result = Some((
                                "⚠ Nada foi importado: as mensagens marcadas são grandes demais."
                                    .into(),
                                false,
                            ));
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
        });
        action
    }
}

/// A short note in a soft-coloured frame: icon and text.
fn soft_note(ui: &mut egui::Ui, tone: Tone, glyph: &str, text: &str) {
    let (fg, bg) = tone.colors();
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(glyph).size(18.0).color(fg));
                ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
            });
        });
}

/// The notice line at the top, with a close button. True when closed.
fn notice_line(ui: &mut egui::Ui, text: &str, error: bool) -> bool {
    let (tone, glyph) = if error {
        (Tone::Danger, icon::ERROR)
    } else {
        (Tone::Ok, icon::CHECK)
    };
    let (fg, bg) = tone.colors();
    let mut close = false;
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, icon::CLOSE, "Fechar o aviso").clicked() {
                    close = true;
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(RichText::new(glyph).size(18.0).color(fg));
                    ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                });
            });
        });
    close
}

/// The "wait" line while something else runs.
fn busy_note(ui: &mut egui::Ui, state: MailBusy, quick: bool) {
    let text = match (state, quick) {
        (MailBusy::Free, _) => return,
        (MailBusy::Mail, false) => "Aguarde: o Ponte está conversando com o servidor de e-mail…",
        (MailBusy::Mail, true) => "Buscando as mensagens…",
        (MailBusy::Other, false) => "Aguarde a operação em andamento.",
        (MailBusy::Other, true) => "Aguarde a operação em andamento; a lista vem em seguida.",
    };
    soft_note(ui, Tone::Warn, icon::CLOCK, text);
    ui.add_space(theme::S2);
}

/// Development only: the views of this screen pictured by `--shots`, by
/// name. The tour calls `shots_open` with each one on a fresh panel.
#[cfg(feature = "shots")]
pub const SHOTS: &[&str] = &["contas", "nova-conta", "mensagens"];

#[cfg(feature = "shots")]
impl MailPanel {
    /// Puts the panel in the state called `view` (see `SHOTS`). Nothing
    /// here starts a network call; "mensagens" uses made-up data kept in
    /// memory only.
    pub fn shots_open(&mut self, _store: &mut crate::storage::Store, view: &str) {
        self.open_full();
        match view {
            "nova-conta" => {
                self.form = Some(AccountForm::new(MailAccount {
                    host: "imap.gmail.com".into(),
                    port: DEFAULT_PORT,
                    folder: "INBOX".into(),
                    ..MailAccount::default()
                }));
            }
            "mensagens" => {
                let account = MailAccount {
                    uid: "exemplo".into(),
                    name: "Gmail pessoal".into(),
                    host: "imap.gmail.com".into(),
                    port: DEFAULT_PORT,
                    user: "exemplo@gmail.com".into(),
                    password: "x".into(),
                    folder: "INBOX".into(),
                    ..MailAccount::default()
                };
                self.accounts = vec![account];
                self.accounts_loaded = true;
                self.selected = Some("exemplo".into());
                self.folders = vec![Folder {
                    raw: "INBOX".into(),
                    name: "Caixa de entrada".into(),
                    no_select: false,
                }];
                self.folder_raw = "INBOX".into();
                self.filter = Some(Filter::All);
                self.messages_folder = "INBOX".into();
                let rows = [
                    ("Maria Souza <maria@orgao.gov.br>", "Ofício 123/2026 - solicitação de informações", "2026-11-09", false, false, 184_000),
                    ("Departamento Jurídico", "Re: Processo 2026/00431 - parecer", "2026-11-08", true, true, 2_400_000),
                    ("João Pereira", "", "2026-11-05", true, false, 52_000),
                    ("Secretaria de Segurança Pública", "Convocação para reunião de segunda-feira", "2026-11-03", true, false, 91_000),
                ];
                self.messages = rows
                    .iter()
                    .enumerate()
                    .map(|(i, (from, subject, date, seen, flagged, size))| MessageSummary {
                        uid: i as u32 + 1,
                        from: (*from).into(),
                        subject: (*subject).into(),
                        date: (*date).into(),
                        message_id: format!("<exemplo-{i}@exemplo>"),
                        flagged: *flagged,
                        seen: *seen,
                        size: *size,
                    })
                    .collect();
                self.messages_loaded = true;
                self.imported.insert(3);
                self.checked.insert(1);
                self.checked.insert(2);
                self.result = Some(("✓ 1 mensagem importada.".into(), true));
                self.summary = vec![(
                    crate::theme::success(),
                    "✓ Ofício 122/2026 — 2 anexo(s)".into(),
                )];
            }
            _ => {}
        }
    }
}
