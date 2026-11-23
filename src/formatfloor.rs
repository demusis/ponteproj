//! The minimum format of each shared folder this device has seen (F2b of
//! Frente F): once a device read or wrote a file of format 9 (or the stub
//! that points to one) of a lineage, an older file of that lineage is never
//! again a base for it, and it never writes one there.
//!
//! It lives in a file of the device ([`FILE`]), outside every library, so
//! that a recovered library, an imported portable copy or a restored backup
//! cannot lower it. It only rises, and is written atomically (a temporary
//! file, then a rename).
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The name of the file, in the folder of the device.
pub const FILE: &str = "formato-minimo.json";

/// Which kind of device exchanges through a shared folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FolderRole {
    /// A computer: it may promote the folder to format 9.
    #[default]
    Desktop,
    /// The phone core: it never promotes; a stub is a refusal that asks the
    /// person to choose the new file.
    Phone,
}

/// The floor of one lineage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Floor {
    pub format: u32,
    #[serde(default)]
    pub level: u32,
}

fn read_all(path: &Path) -> Result<BTreeMap<String, Floor>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "O arquivo {} (o formato mínimo das pastas que este aparelho já viu) está danificado. Nada foi sincronizado; peça ajuda ao suporte.",
                path.display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error).with_context(|| format!("Não foi possível ler {}.", path.display())),
    }
}

/// The floor of `lineage` (format 0 when this device never saw one).
pub fn read(path: &Path, lineage: &str) -> Result<Floor> {
    if lineage.trim().is_empty() {
        return Ok(Floor::default());
    }
    Ok(read_all(path)?.get(lineage).copied().unwrap_or_default())
}

/// Runs `work` holding the lock of the floor file at `path` (a file
/// beside it, created exclusively): raising the floor and the last phase of
/// writing an 8 are never interleaved, in this process or another. A lock
/// left by a process that died is taken over after two minutes.
pub fn with_lock<T>(path: &Path, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock = PathBuf::from(format!("{}.trava", path.display()));
    if let Some(parent) = lock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let started = std::time::Instant::now();
    let held = loop {
        match try_lock(&lock)? {
            Some(file) => break file,
            None => {
                anyhow::ensure!(
                    started.elapsed() < std::time::Duration::from_secs(60),
                    "Outra sincronização deste aparelho está em andamento; tente de novo em instantes."
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    };
    let result = work();
    // Closing the handle releases the lock (the file itself stays).
    drop(held);
    result
}

/// The lock of the operating system on `lock`, held while the returned file
/// is open and released by the system when the process ends, however it
/// ends: never taken over by age. `None` while another holder has it.
fn try_lock(lock: &Path) -> Result<Option<std::fs::File>> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // No sharing: a second open fails while this one lives.
        options.share_mode(0);
        match options.open(lock) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.raw_os_error() == Some(32) || error.raw_os_error() == Some(5) => {
                Ok(None)
            }
            Err(error) => {
                Err(error).with_context(|| format!("Não foi possível usar {}.", lock.display()))
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let file = options
            .open(lock)
            .with_context(|| format!("Não foi possível usar {}.", lock.display()))?;
        // SAFETY: a valid descriptor of a file this function holds open.
        let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if taken {
            return Ok(Some(file));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(error).with_context(|| format!("Não foi possível usar {}.", lock.display()))
        }
    }
}

/// Raises the floor of `lineage` to `floor` (never lowers it): read,
/// changed and replaced under the lock of the file.
pub fn raise(path: &Path, lineage: &str, floor: Floor) -> Result<()> {
    if lineage.trim().is_empty() {
        return Ok(());
    }
    with_lock(path, || raise_locked(path, lineage, floor))
}

fn raise_locked(path: &Path, lineage: &str, floor: Floor) -> Result<()> {
    let mut all = read_all(path)?;
    let now = all.get(lineage).copied().unwrap_or_default();
    if (now.format, now.level) >= (floor.format, floor.level) {
        return Ok(());
    }
    all.insert(lineage.to_owned(), floor);
    let parent = path
        .parent()
        .context("Caminho inválido para o formato mínimo das pastas.")?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".{FILE}.tmp-{}", uuid::Uuid::new_v4().simple()));
    let fail = || format!("Não foi possível gravar {}.", path.display());
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        std::io::Write::write_all(&mut file, &serde_json::to_vec_pretty(&all)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written.with_context(fail)
}

impl crate::storage::Store {
    /// Keeps what must outlive a library (the floor of each folder) in
    /// `dir` instead of the default place.
    pub fn set_device_dir(&mut self, dir: PathBuf) {
        self.device_dir = Some(dir);
    }

    /// This store exchanges as the phone core does: it never promotes a
    /// folder, and a stub asks the person to choose the new file.
    pub fn set_phone_role(&mut self) {
        self.folder_role = FolderRole::Phone;
    }

    /// Where the floor of the folders lives: the folder of the device (the
    /// main folder of the profile, which a restored library does not
    /// replace). Tests of this crate keep one beside each library, so two
    /// libraries of one test are two devices.
    pub(crate) fn floor_file(&self) -> Option<PathBuf> {
        if let Some(dir) = &self.device_dir {
            return Some(dir.join(FILE));
        }
        #[cfg(any(test, feature = "test-support"))]
        {
            let db = self.conn.path().filter(|p| !p.is_empty())?;
            Some(PathBuf::from(format!("{db}-{FILE}")))
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            let resolution = crate::paths::resolution().ok()?;
            Some(resolution.home.unwrap_or(resolution.data_dir).join(FILE))
        }
    }

    /// The minimum format of `lineage` this device has seen.
    pub fn format_floor(&self, lineage: &str) -> Result<u32> {
        match self.floor_file() {
            Some(path) => Ok(read(&path, lineage)?.format),
            None => Ok(0),
        }
    }

    /// Runs `write` (an 8 of `lineage`) under the lock of the floor, which
    /// is read again inside it: refused when it is 9.
    pub(crate) fn under_floor_lock<T>(
        &self,
        lineage: &str,
        write: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let Some(path) = self.floor_file() else {
            return write();
        };
        with_lock(&path, || {
            anyhow::ensure!(
                read(&path, lineage)?.format < crate::sharedfolder::FORMAT_9,
                "Esta pasta já foi atualizada para o formato novo do Ponte; a cópia no formato anterior não foi gravada. Sincronize de novo."
            );
            write()
        })
    }

    /// Raises the floor of `lineage` to format 9.
    pub(crate) fn raise_floor_to_nine(&self, lineage: &str) -> Result<()> {
        match self.floor_file() {
            Some(path) => raise(
                &path,
                lineage,
                Floor {
                    format: crate::sharedfolder::FORMAT_9,
                    level: crate::sharedfolder::LEVEL,
                },
            ),
            None => Ok(()),
        }
    }
}
