//! Writes a file so a failure never leaves the destination truncated: the
//! bytes go to a sibling temporary file, are flushed to disk, and only then
//! replace the destination.
use std::io::Write;
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if path.file_name().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sem nome de arquivo",
        ));
    }
    // A short name of its own, independent of the destination's: a valid
    // destination name of 255 UTF-16 units leaves no room to add to it.
    let temp = path.with_file_name(format!(".ponte-{}.tmp", uuid::Uuid::new_v4().simple()));
    // `create_new`: never opens (and so never later removes) a file that was
    // already there.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    // From here the file is this call's own.
    let result = (|| {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Gives `from` the name `to` in the same folder (a file or a folder) and
/// fails, with [`std::io::ErrorKind::AlreadyExists`], when `to` exists: it
/// never replaces anything. `std::fs::rename` is not this: on Windows it
/// replaces an existing file (see the test below).
///
/// On Windows this is one `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`
/// (the system refuses a name that is taken, atomically); with
/// `MOVEFILE_WRITE_THROUGH` the move is on disk when it returns.
pub fn move_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        }
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() };
        let (a, b) = (wide(from), wide(to));
        // SAFETY: both are NUL-terminated UTF-16 strings that outlive the call.
        let ok = unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), MOVEFILE_WRITE_THROUGH) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if std::fs::symlink_metadata(to).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "o destino já existe",
            ));
        }
        if std::fs::symlink_metadata(from)?.is_dir() {
            return std::fs::rename(from, to);
        }
        // A link fails when the name is taken; then the old name goes.
        std::fs::hard_link(from, to)?;
        std::fs::remove_file(from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_never_replaces_a_file_or_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("novo.txt");
        let to = dir.path().join("existente.txt");
        std::fs::write(&from, b"novo").unwrap();
        std::fs::write(&to, b"de outra pessoa").unwrap();
        let error = move_no_replace(&from, &to).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists, "{error}");
        assert_eq!(std::fs::read(&to).unwrap(), b"de outra pessoa");
        assert_eq!(std::fs::read(&from).unwrap(), b"novo");
        // A free name works, and the old one is gone.
        let free = dir.path().join("livre.txt");
        move_no_replace(&from, &free).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&free).unwrap(), b"novo");
        // Folders too.
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(a.join("dentro")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        assert!(move_no_replace(&a, &b).is_err());
        assert!(a.join("dentro").is_dir());
        move_no_replace(&a, &dir.path().join("c")).unwrap();
        assert!(dir.path().join("c").join("dentro").is_dir());
    }

    /// Why `move_no_replace` exists: the standard rename replaces a file that
    /// is there on Windows, so it can never be the "do not overwrite" step.
    #[cfg(windows)]
    #[test]
    fn the_standard_rename_replaces_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("novo.txt");
        let to = dir.path().join("existente.txt");
        std::fs::write(&from, b"novo").unwrap();
        std::fs::write(&to, b"antigo").unwrap();
        std::fs::rename(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"novo");
    }

    #[test]
    fn replaces_the_destination_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saida.txt");
        std::fs::write(&path, b"antigo").unwrap();
        write_atomic(&path, b"novo").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"novo");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_write_keeps_the_previous_file_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        // The destination is a directory: the final rename fails after the temp file was written.
        let path = dir.path().join("destino");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("dentro.txt"), b"x").unwrap();
        assert!(write_atomic(&path, b"novo").is_err());
        assert!(path.join("dentro.txt").exists());
        let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1, "temporary file removed");
    }

    #[test]
    fn a_destination_name_at_the_windows_limit_still_works() {
        let dir = tempfile::tempdir().unwrap();
        // 250 units: the old sibling name added about 38 to it.
        let path = dir.path().join(format!("{}.txt", "a".repeat(246)));
        write_atomic(&path, b"um").unwrap();
        write_atomic(&path, b"dois").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"dois");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // The longest name Windows accepts, in UTF-16 units.
        let path = dir.path().join(format!("{}.txt", "é".repeat(251)));
        write_atomic(&path, b"tres").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"tres");
    }
}
