//! Writes a file so a failure never leaves the destination truncated: the
//! bytes go to a sibling temporary file, are flushed to disk, and only then
//! replace the destination.
use std::io::Write;
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "sem nome de arquivo")
        })?
        .to_string_lossy()
        .into_owned();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
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
}
