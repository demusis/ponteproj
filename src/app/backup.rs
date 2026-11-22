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

    /// A restore or an import is running: nothing else copies the library.
    pub(super) fn maintenance_running(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| worker.kind == WorkKind::Recovery)
    }
}

/// How often the window looks at the pointer again.
const PLACE_CHECK_EVERY: Duration = Duration::from_secs(5);

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
    /// The computer now uses this other library (this window stays on its own).
    pub(super) superseded_by: Option<PathBuf>,
    /// The pointer could not be followed when the window opened.
    pub(super) notice: Option<String>,
    checked: Option<Instant>,
    pub(super) confirm_adopt: bool,
    dismissed: bool,
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
        let in_use = match &home {
            Some(home) => {
                crate::paths::same_folder(&crate::paths::follow_pointer(home).data_dir, data_dir)
            }
            None => !inspecting,
        };
        Self {
            inspecting,
            marker,
            home,
            in_use,
            notice: resolution.notice.clone(),
            ..Default::default()
        }
    }

    /// A recovery library being looked at, not (yet) the one in use: no
    /// automatic copies are made of it.
    pub(super) fn only_looking(&self) -> bool {
        self.inspecting && !self.in_use
    }
}

impl PonteApp {
    /// Looks at the pointer again every few seconds: a window whose library
    /// stopped being the one in use says so, and a recovery window learns
    /// that it was adopted.
    pub(super) fn tick_library_place(&mut self) {
        if self
            .library
            .checked
            .is_some_and(|at| at.elapsed() < PLACE_CHECK_EVERY)
        {
            return;
        }
        self.library.checked = Some(Instant::now());
        let Some(home) = self.library.home.clone() else {
            return;
        };
        let now = crate::paths::follow_pointer(&home).data_dir;
        let mine = crate::paths::same_folder(&now, &self.data_dir());
        if self.library.inspecting {
            if mine && !self.library.in_use {
                self.auto_copy.allowed = true;
            }
            self.library.in_use = mine;
        } else {
            self.library.in_use = mine;
            let changed = self.library.superseded_by.as_deref() != Some(now.as_path());
            self.library.superseded_by = (!mine).then_some(now);
            if changed {
                self.library.dismissed = false;
            }
        }
    }

    /// The band at the top of the window: a recovery library, a library that
    /// is no longer the one in use, or a pointer that could not be followed.
    pub(super) fn library_banner(&mut self, ui: &mut egui::Ui) {
        enum Band {
            Looking(Marker),
            Adopted,
            Superseded(PathBuf),
            Notice(String),
        }
        let place = &self.library;
        let band = if let Some(notice) = place.notice.clone() {
            Some(Band::Notice(notice))
        } else if place.dismissed {
            None
        } else if let (true, Some(marker)) = (place.inspecting, place.marker.clone()) {
            Some(if place.in_use {
                Band::Adopted
            } else {
                Band::Looking(marker)
            })
        } else {
            place.superseded_by.clone().map(Band::Superseded)
        };
        let Some(band) = band else {
            return;
        };
        let palette = theme::c();
        let (fill, stroke) = match band {
            Band::Notice(_) | Band::Superseded(_) => (palette.danger_soft, palette.danger),
            Band::Looking(_) => (palette.warn_soft, palette.warn),
            Band::Adopted => (palette.ok_soft, palette.ok),
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
                            "O Ponte agora abre o acervo em {}. Esta janela continua no acervo anterior, que fica guardado como está; feche-a para não trabalhar no acervo errado.",
                            now.display()
                        ));
                        ui.horizontal_wrapped(|ui| {
                            open_in_use = ui
                                .add(widgets::primary("Abrir o acervo em uso e fechar esta janela"))
                                .clicked();
                            dismiss = ui.add(widgets::quiet("Continuar aqui por enquanto")).clicked();
                        });
                    }
                    Band::Adopted => {
                        strong(ui, "Este acervo restaurado é agora o acervo deste computador");
                        wrap(ui, "O acervo anterior continua guardado, inteiro, na pasta dele; para voltar a ele, use Ajustes ▸ Cópia de segurança ▸ Acervos deste computador. Se outra janela do Ponte ainda estiver aberta no acervo anterior, feche-a. Este acervo continua sem sincronização até você conectá-lo em Ajustes ▸ Equipe e sincronização.".to_owned());
                        dismiss = ui.add(widgets::quiet("Entendi")).clicked();
                    }
                    Band::Looking(marker) => {
                        strong(ui, "Acervo de recuperação, só para olhar");
                        wrap(ui, format!(
                            "Montado a partir de {}, em {}. Ele não sincroniza com nada («Só neste computador») e não é o acervo que este computador usa: o acervo em uso continua como estava. Olhe, copie textos ou exporte o que precisar.",
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
                            wrap(ui, "A partir de agora o Ponte (a próxima abertura, o navegador e o instalador) passa a usar este acervo. O acervo atual não é apagado nem alterado: fica guardado na pasta dele e pode voltar a ser usado em Ajustes ▸ Cópia de segurança ▸ Acervos deste computador. Este acervo continua sem sincronização; para voltar a sincronizar, conecte-o em Ajustes ▸ Equipe e sincronização.".to_owned());
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
            self.use_library(&folder);
        }
        if open_in_use {
            match recovery::open_library_in_use() {
                Ok(()) => ui
                    .ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error) => self.error(format!("{error:#}")),
            }
        }
    }

    /// "Usar este acervo": the pointer names `folder`. Nothing else changes.
    pub(super) fn use_library(&mut self, folder: &Path) {
        let Some(home) = self.library.home.clone() else {
            self.error("Este Ponte usa a pasta definida em PONTEPROJ_DATA_DIR: a escolha do acervo não vale aqui.");
            return;
        };
        match recovery::adopt(&home, folder) {
            Ok(()) => {
                self.library.libraries = None;
                self.library.checked = None;
                self.library.dismissed = false;
                self.tick_library_place();
                if crate::paths::same_folder(folder, &self.data_dir()) {
                    self.message("Pronto: este acervo passa a ser o deste computador. O acervo anterior ficou guardado como estava.");
                } else {
                    self.message(format!(
                        "Pronto: o Ponte passa a usar o acervo em {}. Esta janela continua no acervo atual até ser fechada; nada foi apagado.",
                        folder.display()
                    ));
                }
            }
            Err(error) => self.error(format!("{error:#}")),
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
