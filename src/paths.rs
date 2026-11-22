//! Where the library lives: `PONTEPROJ_DATA_DIR`, or the library this
//! computer uses, named by the pointer `acervo-em-uso.json` in
//! `%LOCALAPPDATA%\PonteProj` (the main folder), or that main folder itself.
//!
//! Every way in (the window, the browser's native host, `--serve`, the
//! command-line imports and exports, `--copia-antes-de-atualizar`) finds the
//! folder here, so they all follow the pointer. The pointer is written whole
//! or not at all ([`write_pointer`]); a pointer that cannot be read, or that
//! names a folder without a library, is not followed: the main folder opens
//! and [`Resolution::notice`] says why (never a silent, empty library). An
//! explicit `PONTEPROJ_DATA_DIR` always wins over the pointer.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The pointer, in the main folder.
pub const POINTER_FILE: &str = "acervo-em-uso.json";
/// Set (with `PONTEPROJ_DATA_DIR`) on the window the Ponte opens to look at a
/// recovery library.
pub const RECOVERY_WINDOW_ENV: &str = "PONTEPROJ_JANELA_DE_RECUPERACAO";
/// The variable that names the folder of the library explicitly.
pub const OVERRIDE_ENV: &str = "PONTEPROJ_DATA_DIR";

/// Where this process finds its library, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// The folder of the library.
    pub data_dir: PathBuf,
    /// The main folder, where the pointer lives; `None` when
    /// `PONTEPROJ_DATA_DIR` decided (the pointer is then not used).
    pub home: Option<PathBuf>,
    /// The pointer was there and could not be followed: what to tell.
    pub notice: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Pointer {
    versao: u32,
    pasta: String,
}

/// What the main folder says about the library in use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointerState {
    /// No pointer: the main folder's library is the one in use.
    Absent,
    Points(PathBuf),
    /// There, but unreadable: the reason.
    Broken(String),
}

/// The largest pointer read (it holds one path).
const POINTER_LIMIT: u64 = 64 * 1024;

pub fn read_pointer(home: &Path) -> PointerState {
    let path = home.join(POINTER_FILE);
    let bytes = match std::fs::File::open(&path) {
        Ok(file) => {
            use std::io::Read;
            let mut bytes = Vec::new();
            if let Err(error) = file.take(POINTER_LIMIT).read_to_end(&mut bytes) {
                return PointerState::Broken(error.to_string());
            }
            bytes
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return PointerState::Absent,
        Err(error) => return PointerState::Broken(error.to_string()),
    };
    match serde_json::from_slice::<Pointer>(&bytes) {
        Ok(pointer) if pointer.versao == 1 && Path::new(&pointer.pasta).is_absolute() => {
            PointerState::Points(PathBuf::from(pointer.pasta))
        }
        Ok(_) => PointerState::Broken("conteúdo inesperado".to_owned()),
        Err(error) => PointerState::Broken(error.to_string()),
    }
}

/// Two names of the same folder (case and links do not tell them apart).
pub fn same_folder(a: &Path, b: &Path) -> bool {
    let normal = |p: &Path| {
        let p = std::fs::canonicalize(p)
            .or_else(|_| std::path::absolute(p))
            .unwrap_or_else(|_| p.to_path_buf());
        if cfg!(windows) {
            PathBuf::from(p.to_string_lossy().to_lowercase())
        } else {
            p
        }
    };
    normal(a) == normal(b)
}

/// The library in use as the pointer of the main folder `home` says.
pub fn follow_pointer(home: &Path) -> Resolution {
    let main = Resolution {
        data_dir: home.to_path_buf(),
        home: Some(home.to_path_buf()),
        notice: None,
    };
    match read_pointer(home) {
        PointerState::Absent => main,
        PointerState::Broken(reason) => Resolution {
            notice: Some(format!(
                "O arquivo que diz qual acervo este computador usa ({}) não pôde ser lido ({reason}). O Ponte abriu o acervo principal, em {}. Se você usava um acervo restaurado, escolha-o de novo em Ajustes ▸ Cópia de segurança ▸ Acervos deste computador.",
                home.join(POINTER_FILE).display(),
                home.display()
            )),
            ..main
        },
        PointerState::Points(target) if same_folder(&target, home) => main,
        PointerState::Points(target) if db_path_in(&target).is_file() => Resolution {
            data_dir: target,
            ..main
        },
        PointerState::Points(target) => Resolution {
            notice: Some(format!(
                "O acervo que este computador usa ({}) não foi encontrado (a pasta foi apagada ou movida, ou o disco não está acessível). O Ponte abriu o acervo principal, em {}. Se a pasta voltar, escolha-a de novo em Ajustes ▸ Cópia de segurança ▸ Acervos deste computador.",
                target.display(),
                home.display()
            )),
            ..main
        },
    }
}

/// Where the library is: `override_dir` is the value of `PONTEPROJ_DATA_DIR`;
/// `local_data_dir` is where the Windows profile keeps local application data
/// (asked only when there is no override). Creates nothing.
pub fn resolve(
    override_dir: Option<OsString>,
    local_data_dir: impl FnOnce() -> Result<PathBuf>,
) -> Result<Resolution> {
    Ok(match override_dir {
        Some(path) => Resolution {
            data_dir: PathBuf::from(path),
            home: None,
            notice: None,
        },
        None => follow_pointer(&local_data_dir()?.join("PonteProj")),
    })
}

/// The folder of the library, without creating it (see [`resolve`]).
pub fn resolve_data_dir(
    override_dir: Option<OsString>,
    local_data_dir: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    resolve(override_dir, local_data_dir).map(|r| r.data_dir)
}

fn local_profile() -> Result<PathBuf> {
    Ok(directories::BaseDirs::new()
        .context("Perfil local do usuário indisponível")?
        .data_local_dir()
        .to_path_buf())
}

/// Where the library is as the disk says now (the pointer read again).
pub fn fresh_resolution() -> Result<Resolution> {
    resolve(std::env::var_os(OVERRIDE_ENV), local_profile)
}

static RESOLVED: OnceLock<Resolution> = OnceLock::new();

/// Where this process's library is: read once, at the first need, and then
/// kept for the whole process (a window opened on a library stays on it, and
/// its exports, temporary files and reminders stay beside it, even if the
/// pointer changes meanwhile).
pub fn resolution() -> Result<Resolution> {
    if let Some(resolved) = RESOLVED.get() {
        return Ok(resolved.clone());
    }
    let resolved = fresh_resolution()?;
    Ok(RESOLVED.get_or_init(|| resolved).clone())
}

/// The folder of the library as this process sees it, without creating it
/// (what a command that must not touch a library that does not exist yet needs).
pub fn data_dir_path() -> Result<PathBuf> {
    resolution().map(|r| r.data_dir)
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

/// A path as people read it: without the `\\?\` that `canonicalize` puts in
/// front on Windows.
pub fn readable(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Makes `target` the library this computer uses: the pointer in the main
/// folder `home` is replaced whole (written aside, then renamed over the old
/// one). Refused when `target` holds no library. Pointing at `home` itself
/// goes back to the main library. Nothing else is written, moved or removed.
pub fn write_pointer(home: &Path, target: &Path) -> Result<()> {
    if !db_path_in(target).is_file() {
        bail!(
            "Não há acervo em {}; o Ponte continua usando o acervo de antes.",
            target.display()
        );
    }
    let target = readable(
        &std::fs::canonicalize(target)
            .or_else(|_| std::path::absolute(target))
            .unwrap_or_else(|_| target.to_path_buf()),
    );
    std::fs::create_dir_all(home).context("Não foi possível criar a pasta principal do Ponte.")?;
    let bytes = serde_json::to_vec_pretty(&Pointer {
        versao: 1,
        pasta: target.to_string_lossy().into_owned(),
    })?;
    crate::atomic_file::write_atomic(&home.join(POINTER_FILE), &bytes).with_context(|| {
        format!(
            "Não foi possível gravar a escolha do acervo em {}; o Ponte continua usando o acervo de antes.",
            home.display()
        )
    })
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
        // Nor does following a pointer of a main folder that is not there.
        let home = base.path().join("PonteProj");
        assert_eq!(follow_pointer(&home).data_dir, home);
        assert!(!home.exists());
    }

    fn library_in(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(db_path_in(dir), b"acervo").unwrap();
    }

    #[test]
    fn the_pointer_is_followed_and_switching_back_works() {
        let local = tempfile::tempdir().unwrap();
        let home = local.path().join("PonteProj");
        library_in(&home);
        let restored = local.path().join("PonteProj-recuperacao-20261122-101500");
        library_in(&restored);
        let resolved = || resolve(None, || Ok(local.path().to_path_buf())).unwrap();
        assert_eq!(resolved().data_dir, home);
        write_pointer(&home, &restored).unwrap();
        let now = resolved();
        assert!(same_folder(&now.data_dir, &restored));
        assert!(!now.data_dir.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(now.home.as_deref(), Some(home.as_path()));
        assert_eq!(now.notice, None);
        // And back to the main library, which was never touched.
        write_pointer(&home, &home).unwrap();
        assert_eq!(resolved().data_dir, home);
        assert_eq!(std::fs::read(db_path_in(&home)).unwrap(), b"acervo");
        // The override still wins over any pointer.
        write_pointer(&home, &restored).unwrap();
        let other = local.path().join("outra");
        let forced = resolve(Some(other.clone().into()), || unreachable!()).unwrap();
        assert_eq!(forced.data_dir, other);
        assert_eq!(forced.home, None);
    }

    #[test]
    fn a_pointer_to_a_folder_that_is_gone_opens_the_main_library_and_says_so() {
        let local = tempfile::tempdir().unwrap();
        let home = local.path().join("PonteProj");
        library_in(&home);
        let restored = local.path().join("PonteProj-recuperacao-1");
        library_in(&restored);
        write_pointer(&home, &restored).unwrap();
        std::fs::remove_dir_all(&restored).unwrap();
        let resolved = follow_pointer(&home);
        assert_eq!(resolved.data_dir, home);
        let notice = resolved.notice.unwrap();
        assert!(notice.contains("não foi encontrado"), "{notice}");
        // Nothing was created where the pointer pointed.
        assert!(!restored.exists());
    }

    #[test]
    fn a_broken_pointer_opens_the_main_library_and_says_so() {
        let local = tempfile::tempdir().unwrap();
        let home = local.path().join("PonteProj");
        library_in(&home);
        std::fs::write(home.join(POINTER_FILE), b"{ meio arquivo").unwrap();
        let resolved = follow_pointer(&home);
        assert_eq!(resolved.data_dir, home);
        assert!(resolved.notice.unwrap().contains("não pôde ser lido"));
        // A relative path is not followed either.
        std::fs::write(
            home.join(POINTER_FILE),
            br#"{"versao":1,"pasta":"relativa"}"#,
        )
        .unwrap();
        assert!(follow_pointer(&home).notice.is_some());
    }

    #[test]
    fn a_pointer_to_a_folder_without_a_library_is_never_written() {
        let local = tempfile::tempdir().unwrap();
        let home = local.path().join("PonteProj");
        library_in(&home);
        let empty = local.path().join("vazia");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(write_pointer(&home, &empty).is_err());
        assert_eq!(read_pointer(&home), PointerState::Absent);
    }
}
