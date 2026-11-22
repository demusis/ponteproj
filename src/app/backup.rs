//! Ajustes ▸ Cópia de segurança, the part that runs by itself: the automatic
//! copy of the day, on a thread of its own (never the window's worker, never
//! before the window is up), and what the card shows about it.
use super::PonteApp;
use crate::auto_backup::{self, RunReport, Summary};
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
        false
    }
}
