//! Ajustes ▸ Cópia de segurança, the part that is not the card: the
//! automatic copy of the day, on a thread of its own (never the window's
//! worker, never before the window is up), and which library this window has
//! open: a recovery library being looked at (with its banner and "Usar este
//! acervo neste computador"), or a library that stopped being the one in use.
use super::{PonteApp, WorkKind, WorkResult};
use crate::auto_backup::{self, RunReport, Summary};
use crate::recovery::{self, Library, Marker};
use crate::theme::{self, S1, S2};
use crate::widgets;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// How often an open window looks whether the copy of the day is due (a
/// window left open overnight makes the next day's copy too).
const CHECK_EVERY: Duration = Duration::from_secs(60);
/// After a failure, the next try of this window (the next opening tries at
/// once).
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

type Outcome = std::result::Result<Option<RunReport>, String>;

#[derive(Default)]
pub(super) struct AutoCopy {
    /// This window makes the automatic copies: the real window of the library
    /// in use (not a test, not a recovery library being looked at).
    pub(super) allowed: bool,
    running: Option<Receiver<Outcome>>,
    checked: Option<Instant>,
    failed_at: Option<Instant>,
    /// What the card shows; read again after each copy.
    pub(super) summary: Option<Summary>,
}

impl AutoCopy {
    pub(super) fn running(&self) -> bool {
        self.running.is_some()
    }
}

impl PonteApp {
    /// The folder of the library this window has open.
    pub(super) fn data_dir(&self) -> PathBuf {
        self.db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    /// Starts the copy of the day when it is due, and takes the result of
    /// the one that ran. Called on every frame; looks at the disk once a
    /// minute.
    pub(super) fn tick_auto_copy(&mut self, ctx: &eframe::egui::Context) {
        if let Some(receiver) = &self.auto_copy.running {
            match receiver.try_recv() {
                Ok(outcome) => {
                    self.auto_copy.running = None;
                    self.accept_auto_copy(outcome);
                }
                Err(TryRecvError::Disconnected) => {
                    self.auto_copy.running = None;
                    self.accept_auto_copy(Err(
                        "a cópia foi interrompida antes de terminar".to_owned()
                    ));
                }
                Err(TryRecvError::Empty) => {}
            }
            return;
        }
        if !self.auto_copy.allowed
            || self.maintenance_running()
            || self
                .auto_copy
                .checked
                .is_some_and(|at| at.elapsed() < CHECK_EVERY)
            || self
                .auto_copy
                .failed_at
                .is_some_and(|at| at.elapsed() < RETRY_AFTER)
        {
            return;
        }
        self.auto_copy.checked = Some(Instant::now());
        let data_dir = self.data_dir();
        let now = chrono::Local::now().naive_local();
        if !auto_backup::due(&auto_backup::load_state(&data_dir), now.date()) {
            return;
        }
        let database = self.db_path.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        let started = std::thread::Builder::new()
            .name("ponte-copia-automatica".into())
            .spawn(move || {
                let outcome = auto_backup::run_and_record(&database, &data_dir, now)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender.send(outcome);
                repaint.request_repaint();
            });
        match started {
            Ok(_) => self.auto_copy.running = Some(receiver),
            Err(error) => self.accept_auto_copy(Err(error.to_string())),
        }
    }

    fn accept_auto_copy(&mut self, outcome: Outcome) {
        match outcome {
            Ok(Some(RunReport {
                warning: Some(warning),
                ..
            })) => self.error(format!("Cópia automática do acervo: {warning}")),
            Ok(_) => {}
            Err(error) => {
                self.auto_copy.failed_at = Some(Instant::now());
                self.error(format!(
                    "A cópia automática do acervo não foi feita: {error}. O Ponte tenta de novo mais tarde; veja Ajustes ▸ Cópia de segurança."
                ));
            }
        }
        self.auto_copy.summary = Some(auto_backup::summary(&self.data_dir()));
    }

    /// What the card shows, read once and after every copy.
    pub(super) fn auto_copy_summary(&mut self) -> Summary {
        if self.auto_copy.summary.is_none() {
            self.auto_copy.summary = Some(auto_backup::summary(&self.data_dir()));
        }
        self.auto_copy.summary.clone().unwrap_or_default()
    }

    pub(super) fn set_auto_copy(&mut self, on: bool) {
        match auto_backup::set_enabled(&self.data_dir(), on) {
            Ok(()) => {
                self.auto_copy.failed_at = None;
                self.auto_copy.checked = None;
                self.message(if on {
                    "A cópia automática está ligada: o Ponte faz uma por dia."
                } else {
                    "A cópia automática está desligada. As cópias já feitas continuam guardadas."
                });
            }
            Err(error) => self.error(format!("{error:#}")),
        }
        self.auto_copy.summary = None;
    }

    /// A restore, an import, a manual copy or a portable export is running,
    /// or the window is switching libraries: no automatic copy starts.
    pub(super) fn maintenance_running(&self) -> bool {
        self.library.switching_to.is_some()
            || self.worker.as_ref().is_some_and(|worker| {
                matches!(
                    worker.kind,
                    WorkKind::Recovery | WorkKind::Backup | WorkKind::OpenExport
                )
            })
    }
}

/// What the portable copy's form is asking.
#[derive(Default, Clone, PartialEq, Eq)]
pub(super) enum PortableMode {
    #[default]
    Closed,
    Export,
    /// The chosen file waits for its password.
    Import(PathBuf),
}

/// The password form of the portable copy. The password lives here only
/// while it is typed; it is never saved.
#[derive(Default)]
pub(super) struct PortableForm {
    pub(super) mode: PortableMode,
    pub(super) password: String,
    pub(super) confirm: String,
}

/// The text of the switch of library, in the confirmation.
pub(super) const ADOPTION_TEXT: &str = "O Ponte vai fechar e abrir de novo no acervo escolhido. A partir de então o Ponte (a janela, o navegador e o instalador) passa a usar esse acervo. O acervo atual não é apagado nem alterado: fica guardado na pasta dele e pode voltar a ser usado em Ajustes ▸ Cópia de segurança ▸ Acervos deste computador. Feche antes as outras janelas do Ponte. As versões anteriores do Ponte sempre abrem o acervo principal: antes de voltar a uma versão anterior, volte a usar o acervo principal aqui.";

/// A password handed to the worker: overwritten when dropped.
pub(super) struct Secret(pub(super) String);

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// The fields of the portable copy's password (their undo history is
/// forgotten on every exit from the form).
pub(super) fn password_field(n: usize) -> egui::Id {
    egui::Id::NULL.with(("ponte-senha-da-copia-portatil", n))
}

/// Overwrites the text before it is dropped (zeros are valid UTF-8).
fn wipe(text: &mut String) {
    // SAFETY: every byte becomes 0, which keeps the string valid UTF-8.
    unsafe { text.as_bytes_mut().fill(0) };
    text.clear();
}

/// How often the window looks at the pointer again.
const PLACE_CHECK_EVERY: Duration = Duration::from_secs(5);
/// How long a switch of library waits, after the close, for exchanges and
/// work already dispatched to end.
const SWITCH_WAIT: Duration = Duration::from_secs(20);

/// Which library this window has open, as the person needs to know it.
#[derive(Default)]
pub(super) struct LibraryPlace {
    /// Opened by "Restaurar cópia…" to look at a recovery library.
    pub(super) inspecting: bool,
    pub(super) marker: Option<Marker>,
    /// The main folder (where the pointer lives); `None` under an explicit
    /// `PONTEPROJ_DATA_DIR`.
    pub(super) home: Option<PathBuf>,
    /// This window's library is the one the computer uses.
    pub(super) in_use: bool,
    /// Defensive: the selection changed under this window (another user of
    /// the computer, an older Ponte). The window then only looks: no
    /// exchange, no Lixeira expiry, no reminders, no automatic copies.
    pub(super) superseded_by: Option<PathBuf>,
    /// The pointer could not be followed when the window opened.
    pub(super) notice: Option<String>,
    checked: Option<Instant>,
    pub(super) confirm_adopt: bool,
    /// The library of the list whose switch is being confirmed.
    pub(super) confirm_switch: Option<PathBuf>,
    dismissed: bool,
    /// The library this window is switching to: it is going through its
    /// close; when it has closed, the pointer is written and the Ponte opens
    /// again on it.
    pub(super) switching_to: Option<PathBuf>,
    /// The libraries of this computer, read when Ajustes shows them.
    pub(super) libraries: Option<Vec<Library>>,
}

impl LibraryPlace {
    /// What the start of this process found.
    pub(super) fn detect(resolution: &crate::paths::Resolution, data_dir: &Path) -> Self {
        let inspecting = std::env::var_os(crate::paths::RECOVERY_WINDOW_ENV).is_some();
        let marker = recovery::read_marker(data_dir);
        let home = if inspecting {
            marker.as_ref().and_then(Marker::home)
        } else {
            resolution.home.clone()
        };
        let now = home
            .as_ref()
            .map(|home| crate::paths::follow_pointer(home).data_dir);
        let in_use = match &now {
            Some(now) => crate::paths::same_folder(now, data_dir),
            None => !inspecting,
        };
        Self {
            inspecting,
            marker,
            home,
            in_use,
            // Read again here, before anything starts: a selection that
            // changed since this process resolved it makes it only look.
            superseded_by: now.filter(|_| !inspecting && !in_use),
            notice: resolution.notice.clone(),
            ..Default::default()
        }
    }

    /// The window only looks at its library (a recovery library that is not
    /// the one in use, or a library whose selection changed under it): no
    /// exchange, no automatic copy, no Lixeira expiry, no reminders, no
    /// mailbox, no test notice.
    pub(super) fn only_looking(&self) -> bool {
        (self.inspecting && !self.in_use) || self.superseded_by.is_some()
    }
}

/// What the closing process does once its window is gone: `Some(notice)`
/// means open the Ponte again (on the library in use), telling `notice` if
/// there is one.
static RELAUNCH: std::sync::Mutex<Option<Option<String>>> = std::sync::Mutex::new(None);

/// Asked by `run_opening` once the window is closed and its lock released.
pub(super) fn take_relaunch() -> Option<Option<String>> {
    RELAUNCH.lock().ok().and_then(|mut r| r.take())
}

impl PonteApp {
    /// Looks at the pointer again every few seconds. A window whose
    /// selection changed under it (it never switches on its own and lives on:
    /// see [`PonteApp::use_library`]) only looks from then on.
    pub(super) fn tick_library_place(&mut self) {
        if self
            .library
            .checked
            .is_some_and(|at| at.elapsed() < PLACE_CHECK_EVERY)
            || self.library.switching_to.is_some()
        {
            return;
        }
        self.library.checked = Some(Instant::now());
        let Some(home) = self.library.home.clone() else {
            return;
        };
        let now = crate::paths::follow_pointer(&home).data_dir;
        let mine = crate::paths::same_folder(&now, &self.data_dir());
        self.library.in_use = mine;
        if !self.library.inspecting && !mine && self.library.superseded_by.is_none() {
            self.library.superseded_by = Some(now);
            self.library.dismissed = false;
        }
        if self.library.only_looking() {
            self.auto_copy.allowed = false;
        }
        self.apply_transport();
    }

    /// Only the library in use exchanges with a folder or a main computer: a
    /// window that only looks keeps every change here and runs no
    /// synchronization and no main computer, at the Store and in the panel.
    /// Transport is only ever turned off here; a library takes its sync role
    /// back only by being opened again as the library in use.
    pub(super) fn apply_transport(&mut self) {
        if self.library.only_looking() && !self.store.transport_off() {
            self.store.set_transport_off(true);
            self.intranet.set_transport_off(true);
        }
    }

    /// The band at the top of the window: a recovery library being looked
    /// at, a selection that changed under this window, or a pointer that
    /// could not be followed.
    pub(super) fn library_banner(&mut self, ui: &mut egui::Ui) {
        enum Band {
            Looking(Marker),
            Superseded(PathBuf),
            Notice(String),
        }
        let place = &self.library;
        let band = if let Some(notice) = place.notice.clone() {
            Some(Band::Notice(notice))
        } else if place.dismissed {
            None
        } else if let Some(now) = place.superseded_by.clone() {
            Some(Band::Superseded(now))
        } else if let (true, Some(marker)) = (place.inspecting, place.marker.clone()) {
            Some(Band::Looking(marker))
        } else {
            None
        };
        let Some(band) = band else {
            return;
        };
        let palette = theme::c();
        let (fill, stroke) = match band {
            Band::Notice(_) | Band::Superseded(_) => (palette.danger_soft, palette.danger),
            Band::Looking(_) => (palette.warn_soft, palette.warn),
        };
        let folder = self.data_dir();
        let mut adopt = false;
        let mut open_folder = false;
        let mut open_in_use = false;
        let mut dismiss = false;
        let mut confirm = self.library.confirm_adopt;
        let can_adopt = self.library.home.is_some();
        egui::Panel::top("acervo-em-uso")
            .frame(
                egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, stroke))
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                let strong = |ui: &mut egui::Ui, text: &str| {
                    ui.label(RichText::new(text).font(theme::font_strong(theme::SIZE_BODY)));
                };
                let wrap = |ui: &mut egui::Ui, text: String| {
                    ui.add(egui::Label::new(text).wrap());
                };
                match &band {
                    Band::Notice(notice) => {
                        strong(ui, "O acervo em uso não pôde ser aberto");
                        wrap(ui, notice.clone());
                        dismiss = ui.add(widgets::quiet("Entendi")).clicked();
                    }
                    Band::Superseded(now) => {
                        strong(ui, "Este computador passou a usar outro acervo");
                        wrap(ui, format!(
                            "A escolha do acervo mudou fora desta janela: o Ponte agora abre o acervo em {}. Esta janela ficou só para olhar o acervo anterior, que fica guardado como está: ela não sincroniza, não esvazia a Lixeira, não dá avisos e não faz cópias automáticas. Feche-a e abra o acervo em uso.",
                            now.display()
                        ));
                        open_in_use = ui
                            .add(widgets::primary("Abrir o acervo em uso e fechar esta janela"))
                            .clicked();
                    }
                    Band::Looking(marker) => {
                        strong(ui, "Acervo de recuperação, só para olhar");
                        wrap(ui, format!(
                            "Montado a partir de {}, em {}. Não é o acervo que este computador usa, e nesta janela ele não sincroniza nem atende outros computadores, seja qual for a configuração guardada nele: o acervo em uso continua como estava. Olhe, copie textos ou exporte o que precisar.",
                            marker.describe(),
                            auto_backup::human_moment(&marker.montado_em)
                        ));
                        ui.add_space(S1);
                        if !can_adopt {
                            widgets::small_muted(ui, format!(
                                "Este Ponte usa a pasta definida em PONTEPROJ_DATA_DIR. Para trabalhar neste acervo, defina PONTEPROJ_DATA_DIR como {}.",
                                folder.display()
                            ));
                        } else if confirm {
                            wrap(ui, format!("{ADOPTION_TEXT} Este acervo continua sem sincronização; para voltar a sincronizar, conecte-o em Ajustes ▸ Equipe e sincronização."));
                            ui.add_space(S1);
                            ui.horizontal_wrapped(|ui| {
                                adopt = ui.add(widgets::primary("Usar este acervo")).clicked();
                                if ui.add(widgets::quiet("Cancelar")).clicked() {
                                    confirm = false;
                                }
                            });
                        }
                        if can_adopt && !confirm {
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add(widgets::primary("Usar este acervo neste computador…"))
                                    .clicked()
                                {
                                    confirm = true;
                                }
                                open_folder = ui.add(widgets::quiet("Abrir a pasta")).clicked();
                            });
                        }
                    }
                }
                ui.add_space(S2 / 2.0);
            });
        self.library.confirm_adopt = confirm;
        if dismiss {
            self.library.notice = None;
            self.library.dismissed = true;
        }
        if open_folder {
            crate::windows_ui::reveal_in_explorer(&folder);
        }
        if adopt {
            self.library.confirm_adopt = false;
            self.use_library(&folder, ui.ctx());
        }
        if open_in_use {
            match recovery::open_library_in_use(None) {
                Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error) => self.error(format!("{error:#}")),
            }
        }
    }

    /// "Usar este acervo" (in the recovery window, or in Acervos deste
    /// computador): this window goes through its normal close (its work, its
    /// exchanges and its main computer stopped, unsaved work resolved by the
    /// close guard; cancelling there changes nothing); only once it closed is
    /// the pointer written, and the Ponte opens again on the chosen library.
    /// A window never goes on running on a library that stopped being the
    /// one in use.
    pub(super) fn use_library(&mut self, folder: &Path, ctx: &egui::Context) {
        let Some(home) = self.library.home.clone() else {
            self.error("Este Ponte usa a pasta definida em PONTEPROJ_DATA_DIR: a escolha do acervo não vale aqui.");
            return;
        };
        // (a) What the person controls must be settled first.
        let refusal = if self.worker.is_some() || self.auto_copy.running() {
            Some("Há um trabalho em andamento nesta janela; espere ele terminar e tente de novo. Nada foi trocado.")
        } else if self.intranet.serving() {
            Some("Este computador está servindo como computador principal; pare de servir em Ajustes ▸ Equipe e sincronização antes de trocar de acervo. Nada foi trocado.")
        } else if crate::intranet::host_requests_in_flight() > 0 {
            Some("O computador principal desta janela ainda está terminando de atender pedidos; espere alguns segundos e tente de novo. Nada foi trocado.")
        } else {
            None
        };
        if let Some(refusal) = refusal {
            self.error(refusal);
            return;
        }
        // Its own lock is exempt only when this window really holds it.
        let own = self.requests_dir.as_ref().map(|_| self.data_dir());
        if let Err(error) = recovery::check_adoption(&home, folder, own.as_deref()) {
            self.error(format!("{error:#}"));
            return;
        }
        // (b) The close guard about unsaved work decides; nothing is stopped
        // before it did (cancelling leaves the window exactly as it is).
        self.library.switching_to = Some(folder.to_path_buf());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// The close guard was answered "Voltar": the switch does not happen. Nothing
    /// was stopped, so nothing is restored.
    pub(super) fn cancel_switch(&mut self) {
        self.library.switching_to = None;
    }

    /// At the end of the close, the guard passed: (c) exchanges and the main
    /// computer are stopped and what was already dispatched is waited for;
    /// (d) only then the pointer is written; (e) the Ponte is asked to open
    /// again, on the chosen library. If dispatched work does not end in time
    /// (or the pointer cannot be written), nothing is written and the Ponte
    /// opens again on the same library, saying why.
    pub(super) fn finish_switch(&mut self) {
        self.finish_switch_waiting(SWITCH_WAIT);
    }

    pub(super) fn finish_switch_waiting(&mut self, limit: Duration) {
        let Some(folder) = self.library.switching_to.take() else {
            return;
        };
        let Some(home) = self.library.home.clone() else {
            return;
        };
        self.store.set_transport_off(true);
        self.intranet.set_transport_off(true);
        self.auto_copy.allowed = false;
        let started = Instant::now();
        let pending = loop {
            let worker_done = self
                .worker
                .as_ref()
                .is_none_or(|w| !matches!(w.receiver.try_recv(), Err(TryRecvError::Empty)));
            let mut busy = Vec::new();
            if crate::intranet::syncs_in_flight() > 0 {
                busy.push("uma sincronização");
            }
            if crate::intranet::host_requests_in_flight() > 0 {
                busy.push("pedidos ao computador principal");
            }
            if !worker_done {
                busy.push("um trabalho desta janela");
            }
            if self.auto_copy.running() {
                busy.push("a cópia automática");
            }
            if busy.is_empty() || started.elapsed() >= limit {
                break busy;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let own = self.requests_dir.as_ref().map(|_| self.data_dir());
        let notice = if !pending.is_empty() {
            Some(format!(
                "O acervo não foi trocado: {} ainda estava em andamento e não terminou a tempo. O Ponte abriu de novo no mesmo acervo; tente trocar de novo daqui a pouco.",
                pending.join(" e ")
            ))
        } else {
            match recovery::adopt(&home, &folder, own.as_deref()) {
                Ok(()) => None,
                Err(error) => Some(format!(
                    "O acervo não foi trocado: {error:#} O Ponte abriu de novo no acervo de antes."
                )),
            }
        };
        if let Ok(mut relaunch) = RELAUNCH.lock() {
            *relaunch = Some(notice);
        }
    }

    /// The libraries of this computer, read once (and after every change).
    pub(super) fn libraries(&mut self) -> Vec<Library> {
        let Some(home) = self.library.home.clone() else {
            return Vec::new();
        };
        if self.library.libraries.is_none() {
            self.library.libraries = Some(recovery::libraries(&home));
        }
        self.library.libraries.clone().unwrap_or_default()
    }

    /// "Restaurar cópia…": the file, then the recovery library on the worker.
    pub(super) fn restore_copy(&mut self, ctx: &egui::Context) {
        if self.copy_job_blocked() {
            return;
        }
        let Some(source) = crate::windows_ui::pick_file(
            "Restaurar cópia do acervo",
            &[
                ("Cópia do acervo", "*.sqlite3"),
                ("Todos os arquivos", "*.*"),
            ],
        ) else {
            return;
        };
        let place = recovery::Place::new(self.library.home.as_deref(), &self.data_dir());
        let now = chrono::Local::now().naive_local();
        self.spawn(
            "Montando o acervo de recuperação (a cópia é conferida inteira)…".into(),
            WorkKind::Recovery,
            move |_| {
                WorkResult::Recovery(
                    recovery::restore_local_copy(&source, &place, now)
                        .map_err(|error| format!("{error:#}")),
                )
            },
            ctx,
        );
    }

    /// Leaves the password form: the texts are overwritten and the fields'
    /// widget state (with its undo history, which holds what was typed) is
    /// forgotten.
    pub(super) fn close_portable_form(&mut self, ctx: &egui::Context) {
        wipe(&mut self.portable.password);
        wipe(&mut self.portable.confirm);
        self.portable.mode = PortableMode::Closed;
        ctx.data_mut(|data| {
            for n in 0..2 {
                data.remove::<egui::text_edit::TextEditState>(password_field(n));
            }
        });
    }

    /// A copy job of the person (manual copy, restore, portable copy) does
    /// not start while the automatic copy of the day is being made.
    pub(super) fn copy_job_blocked(&mut self) -> bool {
        if self.auto_copy.running() {
            self.error("A cópia automática de hoje está sendo feita agora. Espere ela terminar e tente de novo.");
            return true;
        }
        false
    }

    /// "Exportar cópia portátil…" with the password typed twice: where to
    /// save, then the file on the worker.
    pub(super) fn export_portable(&mut self, ctx: &egui::Context) {
        if self.copy_job_blocked() {
            return;
        }
        let password = std::mem::take(&mut self.portable.password);
        let confirm = std::mem::take(&mut self.portable.confirm);
        if password != confirm {
            let mut confirm = confirm;
            wipe(&mut confirm);
            self.portable.password = password;
            self.error("As duas senhas não são iguais. Digite a mesma senha nos dois campos.");
            return;
        }
        if let Err(error) = crate::portable_copy::validate_password(&password) {
            self.portable.password = password;
            self.portable.confirm = confirm;
            self.error(error.to_string());
            return;
        }
        let suggested = format!(
            "acervo-{}.{}",
            chrono::Local::now().format("%Y%m%d-%H%M"),
            crate::portable_copy::EXTENSION
        );
        let Some(mut path) = crate::windows_ui::save_file(
            "Salvar cópia portátil",
            &suggested,
            &[("Cópia portátil do Ponte", "*.pontecopia")],
        ) else {
            self.portable.password = password;
            self.portable.confirm = confirm;
            return;
        };
        if path.extension().is_none() {
            path.set_extension(crate::portable_copy::EXTENSION);
        }
        let mut confirm = confirm;
        wipe(&mut confirm);
        self.close_portable_form(ctx);
        let database = self.db_path.clone();
        let password = Secret(password);
        self.spawn(
            "Gravando a cópia portátil…".into(),
            WorkKind::Backup,
            move |_| {
                WorkResult::PortableExport(
                    crate::portable_copy::export(&database, &path, &password.0)
                        .map_err(|error| format!("{error:#}")),
                )
            },
            ctx,
        );
    }

    pub(super) fn accept_portable_export(
        &mut self,
        result: std::result::Result<crate::portable_copy::Exported, String>,
    ) {
        match result {
            Ok(done) => self.message(format!(
                "Cópia portátil gravada em {} ({}, {}). Guarde a senha: sem ela a cópia não abre, e o Ponte não consegue recuperá-la.",
                done.path.display(),
                super::plural(done.documents, "documento", "documentos"),
                auto_backup::human_size(done.bytes)
            )),
            Err(error) => self.error(format!("A cópia portátil não foi gravada. {error}")),
        }
    }

    /// "Importar cópia portátil…": the file was chosen and the password
    /// typed; the recovery library is built on the worker (the file is
    /// opened with the password before anything is created).
    pub(super) fn import_portable(&mut self, ctx: &egui::Context) {
        if self.copy_job_blocked() {
            return;
        }
        let PortableMode::Import(source) = self.portable.mode.clone() else {
            return;
        };
        let password = std::mem::take(&mut self.portable.password);
        self.close_portable_form(ctx);
        let place = recovery::Place::new(self.library.home.as_deref(), &self.data_dir());
        let now = chrono::Local::now().naive_local();
        let password = Secret(password);
        self.spawn(
            "Abrindo a cópia portátil e montando o acervo de recuperação…".into(),
            WorkKind::Recovery,
            move |_| {
                WorkResult::Recovery(
                    crate::portable_copy::import(&source, &password.0, &place, now)
                        .map_err(|error| format!("{error:#}")),
                )
            },
            ctx,
        );
    }

    pub(super) fn accept_recovery(&mut self, result: std::result::Result<recovery::Built, String>) {
        self.library.libraries = None;
        match result {
            Ok(built) => {
                let documents = super::plural(built.marker.documentos, "documento", "documentos");
                match recovery::open_window(&built.folder) {
                    Ok(()) => self.message(format!(
                        "Acervo de recuperação montado a partir de {} ({documents}), em {}. Ele abriu numa janela separada, só para olhar; o acervo em uso não mudou.",
                        built.marker.describe(),
                        built.folder.display()
                    )),
                    Err(error) => self.error(format!(
                        "O acervo de recuperação foi montado em {}, mas a janela não abriu ({error:#}). Abra-o em Ajustes ▸ Cópia de segurança ▸ Acervos deste computador.",
                        built.folder.display()
                    )),
                }
            }
            Err(error) => self.error(format!("A cópia não foi restaurada. {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;

    /// A main folder with its library and a finished recovery library.
    fn two_libraries(dir: &Path) -> (PathBuf, PathBuf) {
        let home = dir.join("PonteProj");
        std::fs::create_dir_all(&home).unwrap();
        drop(Store::open(&crate::paths::db_path_in(&home)).unwrap());
        let restored = dir.join("PonteProj-recuperacao-20261122-101500");
        std::fs::create_dir_all(&restored).unwrap();
        drop(Store::open(&crate::paths::db_path_in(&restored)).unwrap());
        let marker = Marker {
            versao: 1,
            tipo: "copia-local".into(),
            origem: "copia.sqlite3".into(),
            copia_de: String::new(),
            montado_em: "2026-11-22T10:15:00".into(),
            pasta_principal: Some(home.display().to_string()),
            documentos: 0,
            esquema_da_copia: 21,
            desligado: Vec::new(),
            completo: true,
        };
        std::fs::write(
            restored.join(recovery::MARKER_FILE),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        (home, restored)
    }

    fn window_on(dir: &Path) -> PonteApp {
        let db = crate::paths::db_path_in(dir);
        let mut app = PonteApp::new(Store::open(&db).unwrap(), db);
        app.library = LibraryPlace {
            home: dir.parent().map(|p| p.join("PonteProj")),
            in_use: true,
            ..Default::default()
        };
        app.auto_copy.allowed = true;
        app
    }

    #[test]
    fn a_window_whose_selection_changed_under_it_only_looks() {
        let dir = tempfile::tempdir().unwrap();
        let (home, restored) = two_libraries(dir.path());
        let mut app = window_on(&home);
        // Another program (another user, an older Ponte) moved the pointer.
        crate::paths::write_pointer(&home, &restored).unwrap();
        app.tick_library_place();
        assert!(app.library.superseded_by.is_some());
        assert!(app.library.only_looking());
        assert!(app.store.transport_off());
        assert!(!app.auto_copy.allowed);
        // Detected at startup too, before anything starts.
        let resolution = crate::paths::Resolution {
            data_dir: home.clone(),
            home: Some(home.clone()),
            notice: None,
        };
        assert!(LibraryPlace::detect(&resolution, &home).only_looking());
    }

    /// The switch tests share the counters of dispatched work and the
    /// relaunch request of the process: one at a time.
    static SWITCHES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn switching_closes_the_window_first_and_writes_the_pointer_only_at_the_end() {
        let _one = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let (home, restored) = two_libraries(dir.path());
        let mut app = window_on(&home);
        let ctx = egui::Context::default();
        app.use_library(&restored, &ctx);
        // Asked to close: nothing written, nothing stopped yet.
        assert_eq!(
            app.library.switching_to.as_deref(),
            Some(restored.as_path())
        );
        assert_eq!(crate::paths::follow_pointer(&home).data_dir, home);
        assert!(!app.store.transport_off());
        assert!(app.maintenance_running());
        // "Voltar": nothing changes.
        app.cancel_switch();
        assert!(app.library.switching_to.is_none());
        assert!(!app.store.transport_off());
        assert_eq!(crate::paths::follow_pointer(&home).data_dir, home);
        // Closed for real: stopped, then the pointer moves and the Ponte
        // opens again there.
        app.use_library(&restored, &ctx);
        app.finish_switch_waiting(Duration::from_secs(60));
        assert!(app.store.transport_off());
        assert!(crate::paths::same_folder(
            &crate::paths::follow_pointer(&home).data_dir,
            &restored
        ));
        assert_eq!(take_relaunch(), Some(None));
    }

    #[test]
    fn cancelling_a_switch_leaves_the_window_exactly_as_it_was() {
        let _one = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let (home, restored) = two_libraries(dir.path());
        let mut app = window_on(&home);
        // A folder password kept only for this session (not saved).
        *app.intranet.draft_folder_key() = "senha só desta sessão".into();
        let ctx = egui::Context::default();
        app.use_library(&restored, &ctx);
        app.cancel_switch();
        assert_eq!(
            app.intranet.draft_folder_key().as_str(),
            "senha só desta sessão"
        );
        assert!(!app.store.transport_off());
        assert!(app.auto_copy.allowed);
        assert_eq!(crate::paths::follow_pointer(&home).data_dir, home);
    }

    #[test]
    fn a_switch_waits_for_dispatched_work_and_gives_up_without_writing() {
        let _one = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let (home, restored) = two_libraries(dir.path());
        let mut app = window_on(&home);
        let ctx = egui::Context::default();
        // A synchronization dispatched before the close is still running.
        let running = crate::intranet::InFlight::sync();
        app.use_library(&restored, &ctx);
        app.finish_switch_waiting(Duration::from_millis(200));
        assert_eq!(crate::paths::follow_pointer(&home).data_dir, home);
        let notice = take_relaunch().unwrap().unwrap();
        assert!(notice.contains("não foi trocado"), "{notice}");
        assert!(notice.contains("sincronização"), "{notice}");
        // Once it ended, the switch goes through.
        drop(running);
        let mut app = window_on(&home);
        app.use_library(&restored, &ctx);
        app.finish_switch_waiting(Duration::from_secs(60));
        assert!(crate::paths::same_folder(
            &crate::paths::follow_pointer(&home).data_dir,
            &restored
        ));
        assert_eq!(take_relaunch(), Some(None));
    }

    #[cfg(windows)]
    #[test]
    fn a_window_without_its_own_lock_does_not_skip_another_windows_lock() {
        let dir = tempfile::tempdir().unwrap();
        let (home, restored) = two_libraries(dir.path());
        // The first window on this library holds its lock; this one opened
        // without it (its handoff timed out).
        use std::os::windows::fs::OpenOptionsExt;
        let _first = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(home.join("janela.lock"))
            .unwrap();
        let mut app = window_on(&home);
        app.requests_dir = None;
        app.use_library(&restored, &egui::Context::default());
        assert!(app.library.switching_to.is_none());
        assert_eq!(crate::paths::follow_pointer(&home).data_dir, home);
    }

    #[test]
    fn a_window_that_is_exchanging_or_working_does_not_switch() {
        let dir = tempfile::tempdir().unwrap();
        let (home, restored) = two_libraries(dir.path());
        let mut app = window_on(&home);
        let ctx = egui::Context::default();
        // A job of the worker is running.
        let (sender, receiver) = std::sync::mpsc::channel();
        let (_progress_sender, progress) = std::sync::mpsc::channel();
        app.worker = Some(super::super::Worker {
            receiver,
            label: "teste".into(),
            kind: WorkKind::Export,
            started: Instant::now(),
            progress,
            latest: String::new(),
            cancel: Default::default(),
        });
        app.use_library(&restored, &ctx);
        assert!(app.library.switching_to.is_none());
        assert_eq!(crate::paths::follow_pointer(&home).data_dir, home);
        drop(sender);
    }
}
