//! Where the library lives: `PONTEPROJ_DATA_DIR`, or
//! `%LOCALAPPDATA%\PonteProj`.
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::PathBuf;

/// The folder of the library, without creating it. `override_dir` is the value
/// of `PONTEPROJ_DATA_DIR`; `local_data_dir` is where the Windows profile keeps
/// local application data (asked only when there is no override).
pub fn resolve_data_dir(
    override_dir: Option<OsString>,
    local_data_dir: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    Ok(match override_dir {
        Some(path) => PathBuf::from(path),
        None => local_data_dir()?.join("PonteProj"),
    })
}

/// The folder of the library as this process sees it, without creating it
/// (what a command that must not touch a library that does not exist yet needs).
pub fn data_dir_path() -> Result<PathBuf> {
    resolve_data_dir(std::env::var_os("PONTEPROJ_DATA_DIR"), || {
        Ok(directories::BaseDirs::new()
            .context("Perfil local do usuário indisponível")?
            .data_local_dir()
            .to_path_buf())
    })
}

pub fn data_dir() -> Result<PathBuf> {
    let path = data_dir_path()?;
    std::fs::create_dir_all(&path).context("Não foi possível criar a pasta local do acervo")?;
    Ok(path)
}

/// The library file inside the folder of the library.
pub fn db_path_in(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("acervo.sqlite3")
}

pub fn db_path() -> Result<PathBuf> {
    Ok(db_path_in(&data_dir()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_folder_is_ponteproj_under_the_local_profile() {
        let dir = resolve_data_dir(None, || Ok(PathBuf::from("C:/perfil/Local"))).unwrap();
        assert_eq!(dir, PathBuf::from("C:/perfil/Local").join("PonteProj"));
        assert_eq!(
            db_path_in(&dir),
            PathBuf::from("C:/perfil/Local/PonteProj/acervo.sqlite3")
        );
    }

    #[test]
    fn the_override_wins_and_the_profile_is_not_even_asked() {
        let dir = resolve_data_dir(Some(OsString::from("D:/outro/acervo")), || {
            anyhow::bail!("não deveria ser consultado")
        })
        .unwrap();
        assert_eq!(dir, PathBuf::from("D:/outro/acervo"));
    }

    #[test]
    fn resolving_the_folder_creates_nothing() {
        let base = tempfile::tempdir().unwrap();
        let wanted = base.path().join("ainda-nao-existe");
        let dir = resolve_data_dir(Some(wanted.clone().into()), || unreachable!()).unwrap();
        assert_eq!(dir, wanted);
        assert!(!wanted.exists());
    }
}
