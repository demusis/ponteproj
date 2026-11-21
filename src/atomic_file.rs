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

#[cfg(test)]
mod tests {
    use super::*;

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
