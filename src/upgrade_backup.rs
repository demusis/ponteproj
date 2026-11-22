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
    /// A NEW file that receives the same report that the output carries (an
    /// existing path is refused, see [`check_result_file`]).
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
    backup_named(
        database,
        destination,
        &format!("antes-de-instalar-{label}-{stamp}"),
        INSTALLER_PREFIX,
    )
}

/// The start of the names of the installers' copies.
const INSTALLER_PREFIX: &str = "antes-de-instalar-";

/// [`backup_library`] under any name: one consistent, verified copy of
/// `database` in `destination`, named `<stem>.sqlite3` (then `<stem>-2`…),
/// never over a file that is there. `family` is the start of the names of
/// this kind of copy: only unfinished copies of the same family, more than a
/// day old, are cleared away from the folder. Used by the automatic and the
/// manual copies of Ajustes ▸ Cópia de segurança too.
pub(crate) fn backup_named(
    database: &Path,
    destination: &Path,
    stem: &str,
    family: &str,
) -> Result<Outcome> {
    debug_assert!(stem.starts_with(family));
    if !library_exists(database)? {
        return Ok(Outcome::NoLibrary);
    }
    std::fs::create_dir_all(destination).with_context(|| {
        format!(
            "Não foi possível criar ou usar a pasta da cópia de segurança ({}).",
            destination.display()
        )
    })?;
    remove_abandoned_partials(destination, family, std::time::SystemTime::now());
    let first_choice = free_name(destination, stem)?;
    let mut partial = Partial::create(&first_choice)?;
    copy_consistently(database, &partial.path)?;
    verify(&partial.path)?;
    // Another run of the same second and label may have taken the name since
    // it was chosen: the copy is published under a name nobody has, never over
    // a copy that is there.
    let final_path = publish_without_replacing(&partial.path, destination, stem)?;
    partial.remove();
    partial.disarm();
    Ok(Outcome::BackedUp(final_path))
}

/// Whether the library file is there. `Path::exists` folds every error into
/// "no", which would turn a library on a drive that cannot be reached right now
/// into "first installation, nothing to copy" and let the installer go on without
/// a copy. Here "there is no library" is only answered when the place it would be
/// in is reachable; anything else that goes wrong is an error.
fn library_exists(database: &Path) -> Result<bool> {
    match std::fs::metadata(database) {
        Ok(meta) => {
            ensure!(
                meta.is_file(),
                "O acervo em {} não é um arquivo.",
                database.display()
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure_place_is_reachable(database)?;
            Ok(false)
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "Não foi possível verificar o acervo em {} (o disco ou a pasta está acessível?).",
                database.display()
            )
        }),
    }
}

/// A library file that is not there is a first installation only if the nearest
/// folder above it that exists can be read: a first installation has no data
/// folder yet, but its parent (`%LOCALAPPDATA%`) is there; a drive or share that
/// is gone has no folder at all, and one that answers with an error is no better.
fn ensure_place_is_reachable(database: &Path) -> Result<()> {
    let absolute = std::path::absolute(database).unwrap_or_else(|_| database.to_path_buf());
    for ancestor in absolute.ancestors().skip(1) {
        match std::fs::metadata(ancestor) {
            Ok(meta) if meta.is_dir() => return Ok(()),
            Ok(_) => bail!(
                "{} não é uma pasta; o acervo não pode estar em {}.",
                ancestor.display(),
                database.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Não foi possível acessar {} para procurar o acervo.",
                        ancestor.display()
                    )
                })
            }
        }
    }
    bail!(
        "Não foi possível alcançar a pasta do acervo ({}): confira o disco ou a pasta definida em PONTEPROJ_DATA_DIR.",
        database.display()
    )
}

/// The `n`-th name for a copy of this stem (for the installers,
/// `antes-de-instalar-<label>-<stamp>`): `<stem>.sqlite3`, then the same with
/// `-2`, `-3`…
fn candidate_name(destination: &Path, stem: &str, n: u32) -> PathBuf {
    let suffix = if n == 1 {
        String::new()
    } else {
        format!("-{n}")
    };
    destination.join(format!("{stem}{suffix}.sqlite3"))
}

/// The most names tried for one label and second.
const MAX_NAMES: u32 = 200;

/// The first name no copy has yet (a hint: it is only claimed by
/// [`publish_without_replacing`]).
fn free_name(destination: &Path, stem: &str) -> Result<PathBuf> {
    for n in 1..=MAX_NAMES {
        let path = candidate_name(destination, stem, n);
        if !path.exists() {
            return Ok(path);
        }
    }
    bail!(
        "Já existem cópias demais com este nome em {}.",
        destination.display()
    )
}

/// Gives the finished copy `partial` its final name without ever replacing a
/// file that is there: `rename` replaces an existing destination, so two runs
/// of the same second that both saw a name free would let the second wipe the
/// first (verified) copy. The name is claimed atomically instead, and on a
/// collision the next one (`-2`, `-3`…) is tried. Answers the name used.
///
/// A hard link from the partial to the name fails if the name exists and shows
/// the name already complete; the partial is removed by the caller. Where hard
/// links are not supported (a network share, a synchronised folder) the name is
/// claimed by creating it exclusively (an empty file only this run owns) and
/// the finished copy then replaces that file.
fn publish_without_replacing(partial: &Path, destination: &Path, stem: &str) -> Result<PathBuf> {
    for n in 1..=MAX_NAMES {
        let candidate = candidate_name(destination, stem, n);
        match std::fs::hard_link(partial, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(error)
                if candidate.exists() || error.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                continue;
            }
            Err(_) => {}
        }
        // No hard links here: claim the name, then fill it.
        if claim_and_fill(partial, &candidate)? {
            return Ok(candidate);
        }
    }
    bail!(
        "Já existem cópias demais com este nome em {}.",
        destination.display()
    )
}

/// Creates `candidate` exclusively (an empty file only this run owns) and
/// replaces it with the finished copy. `false`: the name was taken.
fn claim_and_fill(partial: &Path, candidate: &Path) -> Result<bool> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(candidate)
    {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Não foi possível dar o nome final à cópia ({}).",
                    candidate.display()
                )
            })
        }
    }
    // The claimed file is empty and this run's: replacing it is safe.
    if let Err(error) = std::fs::rename(partial, candidate) {
        let _ = std::fs::remove_file(candidate);
        return Err(error).with_context(|| {
            format!(
                "Não foi possível dar o nome final à cópia ({}).",
                candidate.display()
            )
        });
    }
    Ok(true)
}

fn append(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Clears away what a run that died (power cut, killed Setup) left in the folder
/// of the copies: its own unfinished copies (`<family>*.parcial`, with SQLite's
/// side files; for the installers `antes-de-instalar-*.parcial`) once they are
/// more than a day old. A run that is going on now, or ended a moment ago, is
/// far younger than that.
fn remove_abandoned_partials(destination: &Path, family: &str, now: std::time::SystemTime) {
    let Ok(entries) = std::fs::read_dir(destination) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_partial_of(&name, family) {
            continue;
        }
        let abandoned = entry
            .metadata()
            .ok()
            .filter(|meta| meta.is_file())
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= ABANDONED_AFTER);
        if abandoned {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Whether `name` is an unfinished copy of the family as [`Partial::create`]
/// names it, `<family>….sqlite3.<8 hex>.parcial`, or one of SQLite's side files
/// of such a copy. Nothing else is ever cleared away.
fn is_partial_of(name: &str, family: &str) -> bool {
    let Some(base) = ["-wal", "-shm", "-journal", ""]
        .iter()
        .find_map(|side| name.strip_suffix(&format!(".parcial{side}")))
    else {
        return false;
    };
    let Some((final_name, unique)) = base.rsplit_once('.') else {
        return false;
    };
    name.starts_with(family)
        && final_name.ends_with(".sqlite3")
        && unique.len() == 8
        && unique.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The unfinished copy: removed (with whatever SQLite put beside it) unless it
/// was finished.
struct Partial {
    path: PathBuf,
    armed: bool,
}

/// How long an unfinished copy of an earlier run is left alone before the next
/// run clears it away.
const ABANDONED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

impl Partial {
    /// A new, empty file for the copy, with a name of this run alone
    /// (`<final>.<random>.parcial`) and created exclusively: another run, or what
    /// an older run left behind, is never reused or removed by this one.
    fn create(final_path: &Path) -> Result<Self> {
        for _ in 0..20 {
            let unique = uuid::Uuid::new_v4().simple().to_string();
            let path = append(final_path, &format!(".{}.parcial", &unique[..8]));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Partial { path, armed: true }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "Não foi possível criar o arquivo da cópia de segurança em {}.",
                            path.display()
                        )
                    })
                }
            }
        }
        bail!(
            "Não foi possível escolher um nome para a cópia de segurança em {}.",
            final_path.display()
        )
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

/// The copy has [`BUSY_LIMIT`] to get further than it has ever got. A copy that
/// keeps going (however big the library) keeps setting new highs of "pages
/// copied"; one that is busy, or restarted again and again by a writer, never
/// gets past its best and runs out of time.
struct Stall {
    best: i32,
    since: Instant,
}

impl Stall {
    fn new(now: Instant) -> Self {
        Stall {
            best: 0,
            since: now,
        }
    }

    /// Notes the pages copied so far at `now`; whether the limit has run out.
    fn stalled(&mut self, copied: i32, now: Instant) -> bool {
        if copied > self.best {
            self.best = copied;
            self.since = now;
        }
        now.saturating_duration_since(self.since) >= BUSY_LIMIT
    }
}

/// SQLite's online backup from a read-only connection: a snapshot of what is
/// committed, WAL included, taken while another process may be writing.
pub(crate) fn copy_consistently(database: &Path, target_path: &Path) -> Result<()> {
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
        let mut stall = Stall::new(Instant::now());
        loop {
            let step = backup
                .step(512)
                .context("Não foi possível criar uma cópia de segurança consistente.")?;
            if step == StepResult::Done {
                break;
            }
            // Checked on every turn, not only when the library is busy: a
            // writer that keeps committing makes SQLite restart the copy, and
            // each turn then answers `More`.
            let progress = backup.progress();
            ensure!(
                !stall.stalled(progress.pagecount - progress.remaining, Instant::now()),
                "O acervo ficou ocupado ou em mudança constante por mais de {} segundos; tente de novo com o Ponte fechado.",
                BUSY_LIMIT.as_secs()
            );
            if step != StepResult::More {
                std::thread::sleep(Duration::from_millis(100));
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
pub(crate) fn verify(copy: &Path) -> Result<()> {
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

/// A path in the form used to compare two of them: the case does not tell
/// files apart on Windows.
fn comparable(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

/// The folder with its links resolved when it exists, else as an absolute path.
fn resolved_folder(folder: &Path) -> PathBuf {
    std::fs::canonicalize(folder)
        .or_else(|_| std::path::absolute(folder))
        .unwrap_or_else(|_| folder.to_path_buf())
}

/// Whether `--resultado` may be written at `file`, as the library in `data_dir`
/// is found. The report is a favour to the caller and must never become a way to
/// overwrite data: the file has to be new (an existing path of any kind is
/// refused and left alone), and it may not be the library, its `-wal`/`-shm`, the
/// generation note, or anything else in the library folder (links and `..`
/// resolved) except in the `backups` folder, where the copies are.
pub fn check_result_file(file: &Path, data_dir: &Path) -> Result<()> {
    match std::fs::symlink_metadata(file) {
        Ok(_) => bail!(
            "O arquivo de resultado {} já existe; informe um arquivo novo (ele nunca é sobrescrito).",
            file.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Não foi possível verificar o arquivo de resultado {}.",
                    file.display()
                )
            })
        }
    }
    let name = file
        .file_name()
        .with_context(|| format!("{} não é um nome de arquivo.", file.display()))?;
    let parent = match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let target = std::fs::canonicalize(parent)
        .with_context(|| {
            format!(
                "A pasta do arquivo de resultado ({}) não existe ou não está acessível.",
                parent.display()
            )
        })?
        .join(name);
    let mut folders = vec![resolved_folder(data_dir)];
    // A library file that is itself a link lives (with its -wal) where it points.
    if let Ok(real) = std::fs::canonicalize(paths::db_path_in(data_dir)) {
        if let Some(folder) = real.parent() {
            folders.push(folder.to_path_buf());
        }
    }
    let target = comparable(&target);
    for folder in folders {
        let folder = comparable(&folder);
        ensure!(
            !target.starts_with(&folder) || target.starts_with(folder.join("backups")),
            "O arquivo de resultado {} ficaria na pasta do acervo; escolha outra pasta (ele nunca pode ser o acervo nem ficar ao lado dele).",
            file.display()
        );
    }
    Ok(())
}

/// Writes `text` to a file that must not exist yet (`create_new`): the report the
/// caller asked for never replaces anything.
fn write_new_file(file: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file)?;
    out.write_all(text.as_bytes())?;
    out.sync_all()
}

/// The helper's own note beside the copies: written to a file of its own and
/// renamed over the old one, so a reader sees the old or the new, never half.
fn write_replacing(file: &Path, text: &str) -> std::io::Result<()> {
    let temp = append(file, &format!(".{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let result = write_new_file(&temp, text).and_then(|()| std::fs::rename(&temp, file));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// The command as `main` runs it: copies, prints the path of the copy, writes
/// the report where the options and the destination folder say. Fails (exit
/// code 1, message on the error output) when there is no good copy of a library
/// that exists.
pub fn run(args: &[String]) -> Result<()> {
    let options = parse_options(args)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let data_dir = paths::data_dir_path();
    if let (Some(file), Ok(dir)) = (&options.result_file, &data_dir) {
        // Before anything is done: a report path that would hurt data is a
        // mistake of the caller, not something to find out afterwards.
        check_result_file(file, dir)?;
    }
    let outcome =
        data_dir.and_then(|dir| execute(&options, &dir, env!("CARGO_PKG_VERSION"), &stamp));
    finish(&options, outcome)
}

fn finish(options: &Options, outcome: Result<Outcome>) -> Result<()> {
    let text = report(&outcome);
    if let Some(file) = &options.result_file {
        if let Err(error) = write_new_file(file, &text) {
            eprintln!(
                "Não foi possível gravar o resultado em {}: {error}",
                file.display()
            );
        }
    }
    if let Ok(Outcome::BackedUp(path)) = &outcome {
        if let Some(folder) = path.parent() {
            let note = folder.join(RESULT_FILE_NAME);
            if let Err(error) = write_replacing(&note, &text) {
                eprintln!(
                    "Não foi possível gravar o resultado em {}: {error}",
                    note.display()
                );
            }
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
    fn two_publications_to_the_same_name_keep_both_copies() {
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("a.parcial");
        let second = folder.path().join("b.parcial");
        std::fs::write(&first, b"primeira copia verificada").unwrap();
        std::fs::write(&second, b"segunda copia verificada").unwrap();
        let one = publish_without_replacing(&first, folder.path(), &format!("antes-de-instalar-v1-{STAMP}")).unwrap();
        std::fs::remove_file(&first).unwrap();
        // The second run had also seen the name free: it must take another.
        let two = publish_without_replacing(&second, folder.path(), &format!("antes-de-instalar-v1-{STAMP}")).unwrap();
        std::fs::remove_file(&second).unwrap();
        assert_eq!(
            one.file_name().unwrap().to_string_lossy(),
            "antes-de-instalar-v1-20261120-120000.sqlite3"
        );
        assert_eq!(
            two.file_name().unwrap().to_string_lossy(),
            "antes-de-instalar-v1-20261120-120000-2.sqlite3"
        );
        assert_eq!(std::fs::read(&one).unwrap(), b"primeira copia verificada");
        assert_eq!(std::fs::read(&two).unwrap(), b"segunda copia verificada");
        assert_eq!(files_in(folder.path()).len(), 2);
    }

    #[test]
    fn where_hard_links_do_not_work_the_name_is_claimed_before_it_is_filled() {
        let folder = tempfile::tempdir().unwrap();
        let taken = folder.path().join("antes-de-instalar-v1-x.sqlite3");
        std::fs::write(&taken, b"copia que ja estava la").unwrap();
        let partial = folder.path().join("c.parcial");
        std::fs::write(&partial, b"copia nova").unwrap();
        // A taken name is never replaced.
        assert!(!claim_and_fill(&partial, &taken).unwrap());
        assert_eq!(std::fs::read(&taken).unwrap(), b"copia que ja estava la");
        assert!(partial.exists());
        // A free name is claimed and receives the copy.
        let free = folder.path().join("antes-de-instalar-v1-x-2.sqlite3");
        assert!(claim_and_fill(&partial, &free).unwrap());
        assert_eq!(std::fs::read(&free).unwrap(), b"copia nova");
        assert!(!partial.exists());
        // A copy that cannot be moved leaves no claimed empty file behind.
        let missing = folder.path().join("nao-existe.parcial");
        let other = folder.path().join("antes-de-instalar-v1-x-3.sqlite3");
        assert!(claim_and_fill(&missing, &other).is_err());
        assert!(!other.exists());
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
        let note = copy.parent().unwrap().join(RESULT_FILE_NAME);
        assert_eq!(std::fs::read_to_string(&note).unwrap(), text);
        // The caller's file is new every time; the helper's note is replaced.
        let options = |name: &str| Options {
            result_file: Some(root.path().join(name)),
            ..Options::default()
        };
        let failed = options("falha.txt");
        let failure = finish(&failed, Err(anyhow::anyhow!("sem espaço")));
        assert!(format!("{:#}", failure.unwrap_err()).contains("sem espaço"));
        let text = std::fs::read_to_string(failed.result_file.as_ref().unwrap()).unwrap();
        assert!(text.starts_with("erro\n"), "{text}");
        assert!(text.contains("sem espaço"));
        let none = options("sem-acervo.txt");
        finish(&none, Ok(Outcome::NoLibrary)).unwrap();
        assert!(std::fs::read_to_string(none.result_file.as_ref().unwrap())
            .unwrap()
            .starts_with("sem-acervo\n"));
        // A failure does not touch the note of the last good copy.
        assert!(std::fs::read_to_string(&note).unwrap().starts_with("ok\n"));
        assert!(files_in(copy.parent().unwrap())
            .iter()
            .all(|n| !n.ends_with(".tmp")));
    }

    #[test]
    fn a_report_path_that_exists_is_refused_and_never_touched_even_on_failure() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("dados");
        std::fs::create_dir_all(&data_dir).unwrap();
        let database = paths::db_path_in(&data_dir);
        drop(library_with(&database, 2));
        let before = std::fs::read(&database).unwrap();
        // The library itself, and a file that is already there.
        let other = root.path().join("meu.txt");
        std::fs::write(&other, "meu").unwrap();
        for path in [database.clone(), other.clone()] {
            let error = check_result_file(&path, &data_dir).unwrap_err();
            assert!(format!("{error:#}").contains("já existe"), "{error:#}");
            let options = Options {
                result_file: Some(path),
                ..Options::default()
            };
            // Even when finish is reached (a race), nothing is overwritten.
            let _ = finish(&options, Err(anyhow::anyhow!("falhou")));
        }
        assert_eq!(std::fs::read(&database).unwrap(), before);
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "meu");
    }

    #[test]
    fn a_report_path_inside_the_library_folder_is_refused_except_in_backups() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("dados");
        std::fs::create_dir_all(data_dir.join("backups")).unwrap();
        drop(library_with(&paths::db_path_in(&data_dir), 1));
        let refused = [
            // Sidecars that may not exist right now, the generation note, any file.
            data_dir.join("acervo.sqlite3-wal"),
            data_dir.join("acervo.sqlite3-shm"),
            data_dir.join("acervo.sqlite3-journal"),
            data_dir.join("acervo.geracao.json"),
            data_dir.join("qualquer.txt"),
            // An alias for the same place: `..`.
            data_dir
                .join("backups")
                .join("..")
                .join("acervo.sqlite3-wal"),
        ];
        for path in refused {
            let error = check_result_file(&path, &data_dir).expect_err(&path.display().to_string());
            assert!(
                format!("{error:#}").contains("pasta do acervo"),
                "{error:#}"
            );
            assert!(!path.exists());
        }
        if cfg!(windows) {
            // The case does not tell files apart here.
            assert!(check_result_file(&data_dir.join("ACERVO.SQLITE3-WAL"), &data_dir).is_err());
        }
        // The backups folder and a folder elsewhere are fine.
        check_result_file(&data_dir.join("backups").join("r.txt"), &data_dir).unwrap();
        check_result_file(&root.path().join("r.txt"), &data_dir).unwrap();
        // A folder that does not exist is an error, not something to create.
        assert!(check_result_file(&root.path().join("nada").join("r.txt"), &data_dir).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_report_path_reached_through_a_link_to_the_library_folder_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("dados");
        std::fs::create_dir_all(&data_dir).unwrap();
        drop(library_with(&paths::db_path_in(&data_dir), 1));
        let link = root.path().join("atalho");
        std::os::unix::fs::symlink(&data_dir, &link).unwrap();
        assert!(check_result_file(&link.join("acervo.sqlite3-wal"), &data_dir).is_err());
    }

    #[test]
    fn the_note_beside_the_copies_is_replaced_whole_and_leaves_no_temp_file() {
        let root = tempfile::tempdir().unwrap();
        let note = root.path().join(RESULT_FILE_NAME);
        write_replacing(&note, "um\n").unwrap();
        write_replacing(&note, "dois\n").unwrap();
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "dois\n");
        assert_eq!(files_in(root.path()), vec![RESULT_FILE_NAME.to_owned()]);
        assert!(write_new_file(&note, "x").is_err());
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "dois\n");
    }

    #[test]
    fn a_library_that_cannot_be_reached_is_an_error_not_a_first_installation() {
        let root = tempfile::tempdir().unwrap();
        // A first installation: the data folder is not there yet, its parent is.
        let fresh = root.path().join("Local").join("PonteProj");
        assert_eq!(
            execute(&Options::default(), &fresh, "0.48.2", STAMP).unwrap(),
            Outcome::NoLibrary
        );
        // Something that is not a folder on the way is an error.
        let blocker = root.path().join("bloqueado");
        std::fs::write(&blocker, "x").unwrap();
        let error = backup_library(
            &blocker.join("dados").join("acervo.sqlite3"),
            &root.path().join("copias"),
            "v1",
            STAMP,
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("não é uma pasta"),
            "{error:#}"
        );
        assert!(!root.path().join("copias").exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_library_on_a_drive_that_is_gone_is_an_error() {
        // A drive letter with no drive behind it (the unreachable disk of a
        // PONTEPROJ_DATA_DIR that points to removable storage).
        let gone = ('D'..='Z')
            .map(|letter| PathBuf::from(format!("{letter}:\\")))
            .find(|root| std::fs::metadata(root).is_err())
            .expect("há uma letra de unidade livre");
        let root = tempfile::tempdir().unwrap();
        let error = backup_library(
            &gone.join("Acervo").join("acervo.sqlite3"),
            &root.path().join("copias"),
            "v1",
            STAMP,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("alcançar"), "{error:#}");
    }

    #[test]
    fn the_time_limit_runs_out_when_the_copy_does_not_get_further() {
        let start = Instant::now();
        let at = |secs: u64| start + Duration::from_secs(secs);
        // A copy that a writer restarts over and over: it keeps answering
        // "More" but never gets past its best.
        let mut stall = Stall::new(start);
        assert!(!stall.stalled(100, at(1)));
        assert!(!stall.stalled(20, at(10)));
        assert!(!stall.stalled(150, at(20)));
        assert!(!stall.stalled(30, at(40)));
        assert!(!stall.stalled(140, at(70)));
        assert!(stall.stalled(10, at(81))); // 60 s since the last best (150 at 20 s)
                                            // A big library that keeps going is never cut off.
        let mut stall = Stall::new(start);
        for n in 1..1000 {
            assert!(!stall.stalled(n, at(n as u64 * 5)));
        }
        // Busy from the start, nothing copied.
        let mut stall = Stall::new(start);
        assert!(!stall.stalled(0, at(59)));
        assert!(stall.stalled(0, at(60)));
    }

    #[test]
    fn each_run_has_its_own_unfinished_copy_and_never_touches_another_runs() {
        let root = tempfile::tempdir().unwrap();
        let final_path = root.path().join("antes-de-instalar-v1-x.sqlite3");
        let first = Partial::create(&final_path).unwrap();
        let second = Partial::create(&final_path).unwrap();
        assert_ne!(first.path, second.path);
        assert!(first.path.to_string_lossy().ends_with(".parcial"));
        std::fs::write(&first.path, "do outro").unwrap();
        // The other run's file is still its own; dropping this one leaves it.
        drop(second);
        assert_eq!(std::fs::read_to_string(&first.path).unwrap(), "do outro");
        // A finished copy next to an unfinished one of an earlier run.
        let database = root.path().join("acervo.sqlite3");
        drop(library_with(&database, 1));
        let folder = root.path().join("copias");
        std::fs::create_dir_all(&folder).unwrap();
        let old = folder.join("antes-de-instalar-v0-x.sqlite3.deadbeef.parcial");
        std::fs::write(&old, "velho").unwrap();
        backup_library(&database, &folder, "v1", STAMP).unwrap();
        // Young (not abandoned): the run does not remove it.
        assert!(old.exists());
    }

    #[test]
    fn unfinished_copies_older_than_a_day_are_cleared_at_the_start_of_a_run() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path();
        let make = |name: &str, age: Duration| {
            let path = folder.join(name);
            std::fs::write(&path, "x").unwrap();
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.set_modified(std::time::SystemTime::now() - age)
                .unwrap();
            path
        };
        let day = Duration::from_secs(24 * 60 * 60);
        let old = make("antes-de-instalar-v1-x.sqlite3.aaaaaaaa.parcial", day * 2);
        let old_wal = make(
            "antes-de-instalar-v1-x.sqlite3.aaaaaaaa.parcial-wal",
            day * 2,
        );
        let young = make("antes-de-instalar-v1-y.sqlite3.bbbbbbbb.parcial", day / 2);
        // Not ours, whatever the age: a finished copy, and somebody's file.
        let finished = make("antes-de-instalar-v1-z.sqlite3", day * 30);
        let other = make("outro.parcial", day * 30);
        // Our family's start, but not the shape of a name this helper gives.
        let lookalike = make("antes-de-instalar-meu-arquivo.parcial", day * 30);
        let other_family = make("automatica-20260101-000000.sqlite3.cccccccc.parcial", day * 30);
        remove_abandoned_partials(folder, INSTALLER_PREFIX, std::time::SystemTime::now());
        assert!(!old.exists() && !old_wal.exists());
        assert!(young.exists() && finished.exists() && other.exists());
        assert!(lookalike.exists() && other_family.exists());
    }
}
