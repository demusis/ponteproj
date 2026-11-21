//! `ponteproj --copia-antes-de-atualizar`: the safety copy of the library that
//! the installers make before replacing the program.
//!
//! The installers used to copy `acervo.sqlite3` and its `-wal` as plain files,
//! from a fixed folder. This command does it with the program itself: it finds
//! the library exactly as the window does (`PONTEPROJ_DATA_DIR` included), uses
//! SQLite's online backup (so the committed content still sitting in the WAL is
//! part of the copy, and a Ponte that is open does not matter), checks the copy
//! and only then gives it its final name. It never migrates, never writes to
//! the library and never refuses a library of a newer schema: a consistent copy
//! does not need to understand the schema.
use crate::paths;
use anyhow::{bail, ensure, Context, Result};
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The command-line option that starts this mode.
pub const COMMAND: &str = "--copia-antes-de-atualizar";
/// Written beside the copies (and wherever `--resultado` says) so that a caller
/// that cannot read this process's output still learns what happened.
pub const RESULT_FILE_NAME: &str = "ultima-copia-antes-de-atualizar.txt";
/// How long a library that stays locked is waited for.
const BUSY_LIMIT: Duration = Duration::from_secs(60);

/// What the command found.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// There is no library yet (a first installation): nothing to copy.
    NoLibrary,
    /// The copy, checked, under its final name.
    BackedUp(PathBuf),
}

/// The options after `--copia-antes-de-atualizar`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Folder of the copy; the `backups` folder of the library when absent.
    pub destination: Option<PathBuf>,
    /// Text in the file name, `antes-de-instalar-<rótulo>-<data>.sqlite3`; the
    /// version of this program (`vX.Y.Z`) when absent.
    pub label: Option<String>,
    /// A file that receives the same report that the output carries.
    pub result_file: Option<PathBuf>,
}

pub fn parse_options(args: &[String]) -> Result<Options> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rotulo" => {
                let label = args.next().context("Informe o texto depois de --rotulo")?;
                ensure!(
                    !label.is_empty()
                        && label
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')),
                    "O texto de --rotulo só pode ter letras, números, ponto, hífen e sublinhado."
                );
                options.label = Some(label.clone());
            }
            "--resultado" => {
                let file = args
                    .next()
                    .context("Informe o arquivo depois de --resultado")?;
                options.result_file = Some(PathBuf::from(file));
            }
            other if other.starts_with("--") => {
                bail!("Opção desconhecida: {other}. Uso: {COMMAND} [PASTA] [--rotulo TEXTO] [--resultado ARQUIVO]")
            }
            folder => {
                ensure!(
                    options.destination.is_none(),
                    "Informe uma só pasta de destino."
                );
                options.destination = Some(PathBuf::from(folder));
            }
        }
    }
    Ok(options)
}

/// `<library folder>\backups`: where Ajustes ▸ Cópia de segurança also writes.
pub fn default_destination(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// The whole command, with the library folder and the clock given: copies the
/// library found in `data_dir` as the options say.
pub fn execute(
    options: &Options,
    data_dir: &Path,
    program_version: &str,
    stamp: &str,
) -> Result<Outcome> {
    let destination = options
        .destination
        .clone()
        .unwrap_or_else(|| default_destination(data_dir));
    let label = options
        .label
        .clone()
        .unwrap_or_else(|| format!("v{program_version}"));
    backup_library(&paths::db_path_in(data_dir), &destination, &label, stamp)
}

/// One consistent, verified copy of the library file `database` in the folder
/// `destination`, named `antes-de-instalar-<label>-<stamp>.sqlite3`. A failure
/// leaves nothing behind in the folder; the library is only read.
pub fn backup_library(
    database: &Path,
    destination: &Path,
    label: &str,
    stamp: &str,
) -> Result<Outcome> {
    if !database.exists() {
        return Ok(Outcome::NoLibrary);
    }
    ensure!(
        database.is_file(),
        "O acervo em {} não é um arquivo.",
        database.display()
    );
    std::fs::create_dir_all(destination).with_context(|| {
        format!(
            "Não foi possível criar ou usar a pasta da cópia de segurança ({}).",
            destination.display()
        )
    })?;
    let final_path = free_name(destination, label, stamp)?;
    let mut partial = Partial::new(append(&final_path, ".parcial"));
    copy_consistently(database, &partial.path)?;
    verify(&partial.path)?;
    std::fs::rename(&partial.path, &final_path).with_context(|| {
        format!(
            "Não foi possível dar o nome final à cópia ({}).",
            final_path.display()
        )
    })?;
    partial.disarm();
    Ok(Outcome::BackedUp(final_path))
}

/// `antes-de-instalar-<label>-<stamp>.sqlite3`, or the same with `-2`, `-3`…
/// when a copy of that second is already there.
fn free_name(destination: &Path, label: &str, stamp: &str) -> Result<PathBuf> {
    for n in 1..=200 {
        let suffix = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        let path = destination.join(format!("antes-de-instalar-{label}-{stamp}{suffix}.sqlite3"));
        if !path.exists() && !append(&path, ".parcial").exists() {
            return Ok(path);
        }
    }
    bail!(
        "Já existem cópias demais com este nome em {}.",
        destination.display()
    )
}

fn append(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The unfinished copy: removed (with whatever SQLite put beside it) unless it
/// was finished.
struct Partial {
    path: PathBuf,
    armed: bool,
}

impl Partial {
    fn new(path: PathBuf) -> Self {
        let partial = Partial { path, armed: true };
        partial.remove();
        partial
    }
    fn remove(&self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(append(&self.path, suffix));
        }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for Partial {
    fn drop(&mut self) {
        if self.armed {
            self.remove();
        }
    }
}

/// SQLite's online backup from a read-only connection: a snapshot of what is
/// committed, WAL included, taken while another process may be writing.
fn copy_consistently(database: &Path, target_path: &Path) -> Result<()> {
    let source = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Não foi possível abrir o acervo para copiá-lo.")?;
    source.busy_timeout(Duration::from_secs(5))?;
    let mut target = Connection::open(target_path)
        .context("Não foi possível criar o arquivo da cópia de segurança.")?;
    {
        let backup = Backup::new(&source, &mut target)
            .context("Não foi possível iniciar a cópia consistente do acervo.")?;
        let started = Instant::now();
        loop {
            match backup
                .step(512)
                .context("Não foi possível criar uma cópia de segurança consistente.")?
            {
                StepResult::Done => break,
                StepResult::More => {}
                _ => {
                    ensure!(
                        started.elapsed() < BUSY_LIMIT,
                        "O acervo ficou ocupado por mais de {} segundos; tente de novo com o Ponte fechado.",
                        BUSY_LIMIT.as_secs()
                    );
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    target
        .close()
        .map_err(|(_, e)| e)
        .context("Não foi possível concluir o arquivo da cópia de segurança.")?;
    Ok(())
}

/// `PRAGMA quick_check` on the copy; anything but `ok` fails.
fn verify(copy: &Path) -> Result<()> {
    let conn =
        Connection::open(copy).context("Não foi possível reabrir a cópia para conferi-la.")?;
    let mut statement = conn.prepare("PRAGMA quick_check")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<String>>>()
        .context("A conferência da cópia falhou.")?;
    ensure!(
        rows == ["ok"],
        "A conferência da cópia encontrou problemas: {}",
        rows.join("; ")
    );
    drop(statement);
    conn.close()
        .map_err(|(_, e)| e)
        .context("Não foi possível fechar a cópia depois de conferi-la.")?;
    Ok(())
}

/// The report: first line `ok`, `sem-acervo` or `erro`, then the detail.
fn report(outcome: &Result<Outcome>) -> String {
    match outcome {
        Ok(Outcome::BackedUp(path)) => format!("ok\n{}\n", path.display()),
        Ok(Outcome::NoLibrary) => {
            "sem-acervo\nAinda não há acervo neste computador; não há o que copiar.\n".to_owned()
        }
        Err(error) => format!("erro\n{error:#}\n"),
    }
}

/// The command as `main` runs it: copies, prints the path of the copy, writes
/// the report where the options and the destination folder say. Fails (exit
/// code 1, message on the error output) when there is no good copy of a library
/// that exists.
pub fn run(args: &[String]) -> Result<()> {
    let options = parse_options(args)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let outcome = paths::data_dir_path()
        .and_then(|dir| execute(&options, &dir, env!("CARGO_PKG_VERSION"), &stamp));
    finish(&options, outcome)
}

fn finish(options: &Options, outcome: Result<Outcome>) -> Result<()> {
    let text = report(&outcome);
    let mut files = Vec::new();
    if let Some(file) = &options.result_file {
        files.push(file.clone());
    }
    if let Ok(Outcome::BackedUp(path)) = &outcome {
        if let Some(folder) = path.parent() {
            files.push(folder.join(RESULT_FILE_NAME));
        }
    }
    for file in files {
        if let Err(error) = std::fs::write(&file, &text) {
            eprintln!(
                "Não foi possível gravar o resultado em {}: {error}",
                file.display()
            );
        }
    }
    match outcome {
        Ok(Outcome::BackedUp(path)) => {
            println!("{}", path.display());
            Ok(())
        }
        Ok(Outcome::NoLibrary) => {
            println!("Ainda não há acervo neste computador; não há o que copiar.");
            Ok(())
        }
        Err(error) => Err(error.context("Não foi possível fazer a cópia de segurança do acervo")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: &str = "20261120-120000";

    fn library_with(path: &Path, rows: usize) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        conn.execute_batch("CREATE TABLE notas (id INTEGER PRIMARY KEY, texto TEXT)")
            .unwrap();
        for n in 0..rows {
            conn.execute(
                "INSERT INTO notas (texto) VALUES (?1)",
                [format!("nota {n}")],
            )
            .unwrap();
        }
        conn
    }

    fn rows_in(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT count(*) FROM notas", [], |r| r.get(0))
            .unwrap()
    }

    fn files_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn backed_up(outcome: Outcome) -> PathBuf {
        match outcome {
            Outcome::BackedUp(path) => path,
            Outcome::NoLibrary => panic!("esperava uma cópia"),
        }
    }

    #[test]
    fn the_default_library_is_copied_into_its_backups_folder() {
        let local = tempfile::tempdir().unwrap();
        let data_dir = paths::resolve_data_dir(None, || Ok(local.path().to_path_buf())).unwrap();
        assert_eq!(data_dir, local.path().join("PonteProj"));
        std::fs::create_dir_all(&data_dir).unwrap();
        drop(library_with(&paths::db_path_in(&data_dir), 3));
        let copy = backed_up(execute(&Options::default(), &data_dir, "0.48.2", STAMP).unwrap());
        assert_eq!(copy.parent().unwrap(), data_dir.join("backups"));
        assert_eq!(
            copy.file_name().unwrap().to_string_lossy(),
            "antes-de-instalar-v0.48.2-20261120-120000.sqlite3"
        );
        assert_eq!(rows_in(&copy), 3);
        // Only the finished copy is in the folder.
        assert_eq!(
            files_in(&data_dir.join("backups")),
            vec!["antes-de-instalar-v0.48.2-20261120-120000.sqlite3".to_owned(),]
        );
    }

    #[test]
    fn a_relocated_library_is_the_one_copied() {
        let root = tempfile::tempdir().unwrap();
        let default_dir = root.path().join("Local").join("PonteProj");
        let relocated = root.path().join("outro-disco").join("acervo-do-gabinete");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::create_dir_all(&relocated).unwrap();
        drop(library_with(&paths::db_path_in(&default_dir), 1));
        drop(library_with(&paths::db_path_in(&relocated), 7));
        let data_dir = paths::resolve_data_dir(Some(relocated.clone().into()), || {
            Ok(root.path().join("Local"))
        })
        .unwrap();
        let copy = backed_up(execute(&Options::default(), &data_dir, "0.48.2", STAMP).unwrap());
        assert_eq!(copy.parent().unwrap(), relocated.join("backups"));
        assert_eq!(rows_in(&copy), 7);
        assert!(!default_dir.join("backups").exists());
    }

    #[test]
    fn without_a_library_nothing_is_copied_or_created() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("PonteProj");
        let outcome = execute(&Options::default(), &data_dir, "0.48.2", STAMP).unwrap();
        assert_eq!(outcome, Outcome::NoLibrary);
        assert!(!data_dir.exists());
        // A folder with other files but no library is the same.
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(data_dir.join("outro.txt"), "x").unwrap();
        let outcome = execute(&Options::default(), &data_dir, "0.48.2", STAMP).unwrap();
        assert_eq!(outcome, Outcome::NoLibrary);
        assert_eq!(files_in(&data_dir), vec!["outro.txt".to_owned()]);
    }

    #[test]
    fn committed_content_still_in_the_wal_is_in_the_copy_while_the_library_stays_open() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("acervo.sqlite3");
        // The writer stays open, as a running Ponte would, and never
        // checkpoints: the rows live in the -wal file only.
        let writer = library_with(&database, 25);
        let wal = append(&database, "-wal");
        assert!(std::fs::metadata(&wal).unwrap().len() > 0);
        let before = std::fs::read(&database).unwrap();
        assert_eq!(
            Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap()
                .query_row("SELECT count(*) FROM notas", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            25
        );
        let copy =
            backed_up(backup_library(&database, &root.path().join("copias"), "v1", STAMP).unwrap());
        assert_eq!(rows_in(&copy), 25);
        // The copy stands alone (no -wal beside it) and the library was only read.
        assert_eq!(files_in(&root.path().join("copias")).len(), 1);
        assert_eq!(std::fs::read(&database).unwrap(), before);
        // The open library goes on working afterwards.
        writer
            .execute("INSERT INTO notas (texto) VALUES ('depois')", [])
            .unwrap();
        assert_eq!(rows_in(&database), 26);
        assert_eq!(rows_in(&copy), 25);
    }

    #[test]
    fn a_library_of_a_newer_schema_is_copied_untouched() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("acervo.sqlite3");
        let conn = library_with(&database, 4);
        let newer = crate::storage::SCHEMA_VERSION + 50;
        conn.pragma_update(None, "user_version", newer).unwrap();
        conn.close().unwrap();
        let before = std::fs::read(&database).unwrap();
        let copy =
            backed_up(backup_library(&database, &root.path().join("copias"), "v1", STAMP).unwrap());
        assert_eq!(rows_in(&copy), 4);
        let copied_version: i64 = Connection::open(&copy)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(copied_version, newer);
        assert_eq!(std::fs::read(&database).unwrap(), before);
    }

    #[test]
    fn an_unusable_destination_fails_and_leaves_nothing() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("acervo.sqlite3");
        drop(library_with(&database, 2));
        // The destination "folder" is a file: it can be neither created nor used.
        let blocker = root.path().join("bloqueado");
        std::fs::write(&blocker, "sou um arquivo").unwrap();
        let error = backup_library(&database, &blocker.join("backups"), "v1", STAMP).unwrap_err();
        assert!(
            format!("{error:#}").contains("pasta da cópia de segurança"),
            "{error:#}"
        );
        assert!(backup_library(&database, &blocker, "v1", STAMP).is_err());
        assert_eq!(std::fs::read(&blocker).unwrap(), b"sou um arquivo");
        assert_eq!(files_in(root.path()).len(), 2); // the library and the file
    }

    #[test]
    fn a_library_that_is_not_a_database_fails_and_leaves_no_partial_file() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("acervo.sqlite3");
        std::fs::write(&database, vec![b'x'; 4096]).unwrap();
        let folder = root.path().join("copias");
        assert!(backup_library(&database, &folder, "v1", STAMP).is_err());
        assert_eq!(files_in(&folder), Vec::<String>::new());
    }

    #[test]
    fn a_second_copy_in_the_same_second_gets_another_name() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("acervo.sqlite3");
        drop(library_with(&database, 1));
        let folder = root.path().join("copias");
        let first = backed_up(backup_library(&database, &folder, "v1", STAMP).unwrap());
        let second = backed_up(backup_library(&database, &folder, "v1", STAMP).unwrap());
        assert_ne!(first, second);
        assert_eq!(files_in(&folder).len(), 2);
    }

    #[test]
    fn options_are_read_and_the_label_cannot_be_a_path() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_options(&[]).unwrap(), Options::default());
        let options = parse_options(&args(&[
            "D:/pasta",
            "--rotulo",
            "v0.49.0",
            "--resultado",
            "R.txt",
        ]))
        .unwrap();
        assert_eq!(options.destination, Some(PathBuf::from("D:/pasta")));
        assert_eq!(options.label.as_deref(), Some("v0.49.0"));
        assert_eq!(options.result_file, Some(PathBuf::from("R.txt")));
        assert!(parse_options(&args(&["--rotulo", "..\\x"])).is_err());
        assert!(parse_options(&args(&["--rotulo"])).is_err());
        assert!(parse_options(&args(&["--desconhecida"])).is_err());
        assert!(parse_options(&args(&["a", "b"])).is_err());
    }

    #[test]
    fn the_report_is_written_for_the_caller_and_failures_stay_failures() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("acervo.sqlite3");
        drop(library_with(&database, 1));
        let result = root.path().join("resultado.txt");
        let options = Options {
            destination: Some(root.path().join("copias")),
            label: None,
            result_file: Some(result.clone()),
        };
        let outcome = backup_library(&database, &root.path().join("copias"), "v1", STAMP);
        let copy = match &outcome {
            Ok(Outcome::BackedUp(path)) => path.clone(),
            other => panic!("{other:?}"),
        };
        finish(&options, outcome).unwrap();
        let text = std::fs::read_to_string(&result).unwrap();
        assert_eq!(text, format!("ok\n{}\n", copy.display()));
        assert_eq!(
            std::fs::read_to_string(copy.parent().unwrap().join(RESULT_FILE_NAME)).unwrap(),
            text
        );
        let failure = finish(&options, Err(anyhow::anyhow!("sem espaço")));
        assert!(format!("{:#}", failure.unwrap_err()).contains("sem espaço"));
        let text = std::fs::read_to_string(&result).unwrap();
        assert!(text.starts_with("erro\n"), "{text}");
        assert!(text.contains("sem espaço"));
        finish(&options, Ok(Outcome::NoLibrary)).unwrap();
        assert!(std::fs::read_to_string(&result)
            .unwrap()
            .starts_with("sem-acervo\n"));
    }
}
