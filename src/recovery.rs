//! "Restaurar cópia…": a recovery library, built from a copy in a data folder
//! of its own, looked at in a window of its own, and adopted (or not) by
//! writing one pointer.
//!
//! The library in use is never written to, renamed, replaced or deleted
//! here. Restoring always BUILDS a new folder, a sibling of the main one
//! (`PonteProj-recuperacao-AAAAMMDD-HHMMSS`), created exclusively by this run:
//!
//! 1. the chosen file is recognised (a Ponte library, not a portable copy nor
//!    a sync file) and its schema checked from a read-only connection: a copy
//!    of a newer Ponte is refused before anything is created;
//! 2. it is copied with SQLite's online backup from a read-only connection
//!    (the engine of `upgrade_backup`: a WAL beside it is read coherently and
//!    the original is never modified) and the copy passes `quick_check`;
//! 3. every sealed payload of every table of the copy is opened with this
//!    Windows account: a copy of another account (or damaged) is refused;
//! 4. it is detached from every synchronization (see [`SYNC_TABLES`]) before
//!    the normal code opens it; the sidecars of the original (guard copy,
//!    history note, pending copies) are never brought along;
//! 5. only then `Store::open` migrates it (an older copy is upgraded in the
//!    new folder alone) and the configuration becomes "Só neste computador",
//!    with no folder, no address, a new key and no queue;
//! 6. the marker [`MARKER_FILE`] is written last: a folder without it is not
//!    a recovery library. Any failure removes the folder this run created and
//!    leaves everything else as it was.
//!
//! The window that shows it gets `PONTEPROJ_DATA_DIR` (its own lock, its own
//! requests: it neither hijacks nor is hijacked by the window of the library
//! in use, and the browser keeps writing to the library in use). Adopting it
//! is one atomic write of the pointer (`paths::write_pointer`); the previous
//! library stays whole in its folder and can be chosen again.
use crate::paths;
use crate::storage::Store;
use anyhow::{bail, ensure, Context, Result};
use chrono::NaiveDateTime;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Written last in a recovery folder: what it was built from.
pub const MARKER_FILE: &str = paths::RECOVERY_MARKER;
/// Held by every window of the Ponte in the folder of its library
/// (`single_instance`).
const WINDOW_LOCK: &str = "janela.lock";
/// Held by `ponteproj --serve` in the folder of the library it serves.
pub const SERVER_LOCK: &str = "servidor.lock";
const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";
/// The word in the names of the recovery folders.
const FOLDER_WORD: &str = "recuperacao";
/// This computer's preferences, carried into the new folder as they are
/// (appearance and reminder settings: the machine's, not the library's).
const MACHINE_FILES: &[&str] = &["aparencia.txt", "lembretes.txt"];

/// What a recovery library leaves behind of the synchronization of the copy:
/// every row of these tables (when the copy has them), with what they hold.
/// The role is never taken from the copy.
pub const SYNC_TABLES: &[(&str, &str)] = &[
    (
        "intranet_config",
        "a configuração de sincronização (modo, pastas e senha da pasta, endereço, chave e papel de computador principal)",
    ),
    ("offline_outbox", "as alterações que esperavam envio"),
    ("offline_conflicts", "os conflitos de edição"),
    ("offline_id_map", "a tradução de números da fila de envio"),
    ("offline_watermark", "as marcas de números da fila de envio"),
    ("offline_frame_journal", "o diário de números da fila de envio"),
    ("host_generation", "a história de computador principal"),
    (
        "mutation_receipts",
        "as respostas dadas a outros computadores",
    ),
    (
        "applied_mutations",
        "o registro das alterações recebidas de outros computadores",
    ),
    ("client_cache", "o esquema da última cópia da equipe"),
];

/// What the chosen file is.
#[derive(Debug, PartialEq, Eq)]
pub enum FileKind {
    /// A SQLite database (a library, or something else).
    Database,
    /// A portable copy (`.pontecopia`).
    Portable,
    /// The transport file of a shared folder.
    SyncFile,
    Other,
}

pub fn sniff(path: &Path) -> Result<FileKind> {
    let mut head = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(16).read_to_end(&mut head))
        .with_context(|| format!("Não foi possível ler {}.", path.display()))?;
    Ok(if head.starts_with(SQLITE_MAGIC) {
        FileKind::Database
    } else if head.starts_with(crate::sharedfolder::PORTABLE_MAGIC) {
        FileKind::Portable
    } else if head.starts_with(crate::sharedfolder::SYNC_MAGIC) {
        FileKind::SyncFile
    } else {
        FileKind::Other
    })
}

/// Said when a portable copy is chosen to restore.
pub const PORTABLE_HINT: &str = "Este arquivo é uma cópia portátil (.pontecopia), não uma cópia deste computador. Use «Importar cópia portátil…» em Ajustes ▸ Cópia de segurança. Nada foi criado.";
/// Said when a sync file is chosen to restore or import.
pub const SYNC_HINT: &str = "Este arquivo é o arquivo de sincronização de uma pasta compartilhada (ponte-sync.ponte), não uma cópia de segurança. Para usá-lo, conecte este computador à pasta em Ajustes ▸ Equipe e sincronização. Nada foi criado.";

/// The marker of a recovery folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub versao: u32,
    /// `copia-local` or `copia-portatil`.
    pub tipo: String,
    /// The file it was built from.
    pub origem: String,
    /// When that copy was made (local, `AAAA-MM-DDTHH:MM:SS`), when known.
    #[serde(default)]
    pub copia_de: String,
    /// When this folder was built (local, `AAAA-MM-DDTHH:MM:SS`).
    pub montado_em: String,
    /// The main folder (where the pointer lives) of the library it was
    /// restored from; `None` when `PONTEPROJ_DATA_DIR` was in force.
    #[serde(default)]
    pub pasta_principal: Option<String>,
    pub documentos: usize,
    /// The schema of the copy before it was upgraded here.
    pub esquema_da_copia: i64,
    /// What was taken out of the copy to detach it (in words).
    #[serde(default)]
    pub desligado: Vec<String>,
    /// Written true only when the folder is finished: detached, verified and
    /// about to get its final name. A folder without it is never opened.
    #[serde(default)]
    pub completo: bool,
}

impl Marker {
    pub fn origin_name(&self) -> String {
        Path::new(&self.origem)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.origem.clone())
    }

    /// "a cópia «x», feita em 22/11/2026 às 09:14"
    pub fn describe(&self) -> String {
        let kind = if self.tipo == "copia-portatil" {
            "a cópia portátil"
        } else {
            "a cópia"
        };
        if self.copia_de.is_empty() {
            format!("{kind} «{}»", self.origin_name())
        } else {
            format!(
                "{kind} «{}», feita em {}",
                self.origin_name(),
                crate::auto_backup::human_moment(&self.copia_de)
            )
        }
    }

    pub fn home(&self) -> Option<PathBuf> {
        self.pasta_principal.as_ref().map(PathBuf::from)
    }
}

/// The marker of a finished recovery folder (`None` for any other folder,
/// and for one whose build did not finish).
pub fn read_marker(folder: &Path) -> Option<Marker> {
    let marker: Marker =
        serde_json::from_slice(&std::fs::read(folder.join(MARKER_FILE)).ok()?).ok()?;
    (marker.completo && paths::ensure_openable(folder).is_ok()).then_some(marker)
}

/// Where a recovery folder is made and what it carries from this computer.
#[derive(Clone, Debug)]
pub struct Place {
    /// The main folder (pointer), when the pointer is in use.
    pub home: Option<PathBuf>,
    /// The folder of the library open now (its machine preferences are
    /// carried along).
    pub current: PathBuf,
}

impl Place {
    pub fn new(home: Option<&Path>, current: &Path) -> Self {
        Self {
            home: home.map(Path::to_path_buf),
            current: current.to_path_buf(),
        }
    }

    /// The recovery folders are siblings of the main folder (or of the
    /// folder in use, under `PONTEPROJ_DATA_DIR`), named after it.
    fn anchor(&self) -> &Path {
        self.home.as_deref().unwrap_or(&self.current)
    }

    fn parent(&self) -> PathBuf {
        match self.anchor().parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        }
    }

    fn base_name(&self) -> String {
        self.anchor()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "PonteProj".to_owned())
    }
}

/// The folder this run creates. It is built under a staging name
/// (`…-recuperacao-AAAAMMDD-HHMMSS.montando`, with a sentinel file inside)
/// that no ordinary opening accepts (`paths::ensure_openable`), and only
/// [`NewFolder::publish`] gives it its recovery name, once it is detached,
/// verified and marked. Until then it is removed with everything in it (all
/// of it this run's) if anything fails.
struct NewFolder {
    path: PathBuf,
    /// The recovery name it gets when published (`-2`, `-3`… if taken).
    stem: PathBuf,
    keep: bool,
}

#[cfg(test)]
thread_local! {
    /// Tests: stop the build right after the copy, before the detach, as a
    /// killed process would (nothing is cleaned up).
    static KILL_BEFORE_DETACH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static KILLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl NewFolder {
    fn create(place: &Place, now: NaiveDateTime) -> Result<Self> {
        let parent = place.parent();
        let stem = format!(
            "{}-{FOLDER_WORD}-{}",
            place.base_name(),
            now.format("%Y%m%d-%H%M%S")
        );
        for n in 1..=100 {
            let name = if n == 1 {
                stem.clone()
            } else {
                format!("{stem}-{n}")
            };
            let path = parent.join(format!("{name}{}", paths::STAGING_SUFFIX));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    let folder = Self {
                        path,
                        stem: parent.join(&stem),
                        keep: false,
                    };
                    std::fs::write(
                        folder.path.join(paths::STAGING_SENTINEL),
                        "Acervo de recuperação em montagem: não abra esta pasta.\n",
                    )
                    .context("Não foi possível preparar a pasta do acervo de recuperação.")?;
                    return Ok(folder);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "Não foi possível criar a pasta do acervo de recuperação em {}. Confira se o disco está acessível e tem espaço. Nada foi alterado.",
                            parent.display()
                        )
                    })
                }
            }
        }
        bail!(
            "Já existem pastas de recuperação demais com este nome em {}.",
            parent.display()
        )
    }

    /// The finished folder gets its recovery name, never over a folder that
    /// is there. The sentinel goes first: a folder still under its staging
    /// name is refused by its name alone.
    fn publish(&mut self) -> Result<PathBuf> {
        std::fs::remove_file(self.path.join(paths::STAGING_SENTINEL))
            .context("Não foi possível concluir a pasta do acervo de recuperação.")?;
        for n in 1..=100 {
            let mut name = self.stem.as_os_str().to_owned();
            if n > 1 {
                name.push(format!("-{n}"));
            }
            let candidate = PathBuf::from(name);
            match crate::atomic_file::move_no_replace(&self.path, &candidate) {
                Ok(()) => {
                    self.path = candidate.clone();
                    self.keep = true;
                    return Ok(candidate);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) if candidate.exists() => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                        "Não foi possível dar o nome final à pasta do acervo de recuperação ({}).",
                        candidate.display()
                    )
                    })
                }
            }
        }
        bail!("Já existem pastas de recuperação demais com este nome.")
    }
}

impl Drop for NewFolder {
    fn drop(&mut self) {
        #[cfg(test)]
        if KILLED.with(|k| k.get()) {
            return;
        }
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// A recovery library, built.
#[derive(Clone, Debug)]
pub struct Built {
    pub folder: PathBuf,
    pub marker: Marker,
}

fn moment(time: NaiveDateTime) -> String {
    time.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// The schema of a library file, read without writing (a library of a newer
/// Ponte, or a database that is not a library, is refused here, before
/// anything is created).
fn check_source(conn: &Connection, source: &Path) -> Result<i64> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let unreadable = |error: rusqlite::Error| {
        anyhow::anyhow!(
            "Não foi possível ler a cópia {} ({error}). Se ela está num pendrive, num CD ou numa pasta só de leitura, copie-a antes para uma pasta deste computador. Nada foi criado.",
            source.display()
        )
    };
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(unreadable)?;
    let has_documents: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='documents')",
            [],
            |r| r.get(0),
        )
        .map_err(unreadable)?;
    ensure!(
        version > 0 && has_documents,
        "{} é um banco de dados, mas não um acervo do Ponte. Nada foi criado.",
        source.display()
    );
    ensure!(
        version <= crate::storage::SCHEMA_VERSION,
        "Esta cópia foi feita por uma versão mais nova do Ponte (versão {version} do acervo; este Ponte conhece até a {}). Atualize o Ponte e tente de novo. Nada foi criado.",
        crate::storage::SCHEMA_VERSION
    );
    Ok(version)
}

/// The chosen copy, opened only to be read, with SQLite's locking: a writer
/// that starts meanwhile is seen, and the committed changes still in a WAL
/// beside it are part of what is read (SQLite may create its `-shm` there). A
/// side file that cannot be looked at, or is not a plain file, makes the
/// state of the copy ambiguous: it is refused. One read transaction holds the
/// same state from the schema check to the end of the copy.
fn open_source(source: &Path) -> Result<Connection> {
    for side in ["-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{side}", source.display()));
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => bail!(
                "Não foi possível conferir {} ao lado da cópia; o estado dela é incerto. Copie a cópia (com os arquivos de mesmo nome terminados em -wal e -shm, se existirem) para uma pasta deste computador e tente de novo. Nada foi criado.",
                path.display()
            ),
        }
    }
    let conn = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| {
        format!(
            "Não foi possível abrir a cópia {}. Se ela está num pendrive, num CD ou numa pasta só de leitura, copie-a antes para uma pasta deste computador. Nada foi criado.",
            source.display()
        )
    })?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch("BEGIN")?;
    Ok(conn)
}

/// A copy this feature just published and nobody else knows yet (established
/// static): opened as immutable, so SQLite creates nothing beside it.
pub(crate) fn open_own_copy(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        immutable_uri(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| {
        format!(
            "Não foi possível reabrir a cópia {} para conferi-la.",
            path.display()
        )
    })
}

/// `file:///C:/…?immutable=1`, with the characters a URI cannot carry escaped.
fn immutable_uri(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = paths::readable(&absolute)
        .to_string_lossy()
        .replace('\\', "/");
    let mut escaped = String::new();
    for c in text.chars() {
        match c {
            '%' => escaped.push_str("%25"),
            '?' => escaped.push_str("%3f"),
            '#' => escaped.push_str("%23"),
            c => escaped.push(c),
        }
    }
    if escaped.starts_with("//") {
        format!("file:{escaped}?immutable=1")
    } else {
        format!("file:///{escaped}?immutable=1")
    }
}

/// A connection to the copy being prepared, with the function the guard
/// triggers of the library call.
fn open_raw(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .context("Não foi possível abrir o acervo de recuperação em montagem.")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.create_scalar_function(
        "ponte_client_schema",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
            | rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
        |_| Ok(crate::storage::SCHEMA_VERSION),
    )?;
    Ok(conn)
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn tables(conn: &Connection) -> Result<Vec<String>> {
    Ok(conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )?)
}

/// What a table holds, in words, for the message of a payload that does not
/// open.
fn table_label(table: &str) -> &'static str {
    match table {
        "files" => "os PDFs",
        "attachments" => "os anexos",
        "attachment_texts" => "os textos dos anexos",
        "captures" => "o conteúdo dos documentos",
        "notes" => "as notas",
        "drafts" => "as minutas",
        "analyses" => "as análises",
        "tasks" => "as tarefas",
        "trash" => "a lixeira",
        "mail_accounts" => "as contas de e-mail",
        "intranet_config" => "a configuração de sincronização",
        "ai_config" => "a configuração da IA",
        "projects" => "os projetos",
        "resources" | "resource_uses" => "os recursos",
        "work_templates"
        | "work_template_versions"
        | "instruction_blocks"
        | "instruction_block_versions"
        | "template_examples"
        | "draft_generations" => "os modelos",
        _ => "parte dos dados",
    }
}

fn columns_of(conn: &Connection, table: &str) -> Result<Vec<(String, String)>> {
    Ok(conn
        .prepare(&format!(
            "SELECT name, upper(type) FROM pragma_table_info('{}')",
            table.replace('\'', "''")
        ))?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Opens every sealed payload of every table (every row, every column, not a
/// sample) with this Windows account, wherever a value carries the mark of a
/// sealed payload. Used on a copy before it is migrated (its schema may be
/// older). Answers how many were opened.
pub fn verify_sealed(conn: &Connection) -> Result<usize> {
    let mut opened = 0usize;
    for table in tables(conn)? {
        for (column, _) in columns_of(conn, &table)? {
            let sql = format!(
                "SELECT rowid, {c} FROM {t} WHERE typeof({c})='blob' AND substr({c},1,6)=X'504F4E54452D'",
                c = quoted(&column),
                t = quoted(&table)
            );
            let mut statement = conn.prepare(&sql)?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let rowid: i64 = row.get(0)?;
                let bytes: Vec<u8> = row.get(1)?;
                if !crate::protection::looks_sealed(&bytes) {
                    continue;
                }
                if crate::protection::unseal(&bytes).is_err() {
                    bail!(foreign_message(&SealProblem {
                        table: table.clone(),
                        rowid,
                        opened,
                    }));
                }
                opened += 1;
            }
        }
    }
    Ok(opened)
}

/// One stored value that should open with this Windows account and does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealProblem {
    pub table: String,
    pub rowid: i64,
    /// Sealed values opened before this one.
    pub opened: usize,
}

impl std::fmt::Display for SealProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (tabela {}, registro nº {}) não podem ser abertos nesta conta do Windows",
            table_label(&self.table),
            self.table,
            self.rowid
        )
    }
}

/// The refusal of a copy with a value that does not open here.
pub fn foreign_message(problem: &SealProblem) -> String {
    format!(
        "Esta cópia foi feita em outra conta do Windows, ou está danificada: {problem} ({} itens protegidos abertos antes deste). Uma cópia deste computador só abre na mesma conta do Windows em que foi feita. Nada foi criado. Se você tem uma cópia portátil (.pontecopia), use «Importar cópia portátil…».",
        problem.opened
    )
}

/// The schema-aware check, on a library of the current schema: every value of
/// every column that this schema keeps sealed (every BLOB column, except the
/// `…_hash` ones, which hold digests) must carry the mark of a sealed payload
/// AND open with this Windows account. Damage that changed the mark itself
/// is caught too. The first value that fails is answered.
pub fn verify_sealed_strict(conn: &Connection) -> Result<std::result::Result<usize, SealProblem>> {
    let mut opened = 0usize;
    for table in tables(conn)? {
        for (column, kind) in columns_of(conn, &table)? {
            if kind != "BLOB" || column.ends_with("_hash") {
                continue;
            }
            let sql = format!(
                "SELECT rowid, {c} FROM {t} WHERE {c} IS NOT NULL",
                c = quoted(&column),
                t = quoted(&table)
            );
            let mut statement = conn.prepare(&sql)?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let rowid: i64 = row.get(0)?;
                let good = match row.get_ref(1)? {
                    rusqlite::types::ValueRef::Blob(bytes) => {
                        crate::protection::looks_sealed(bytes)
                            && crate::protection::unseal(bytes).is_ok()
                    }
                    _ => false,
                };
                if !good {
                    return Ok(Err(SealProblem {
                        table,
                        rowid,
                        opened,
                    }));
                }
                opened += 1;
            }
        }
    }
    Ok(Ok(opened))
}

/// The member name of the copy's configuration, when it can be read (it is
/// the only thing of the configuration a recovery library keeps).
fn member_name(conn: &Connection) -> Option<String> {
    if !table_exists(conn, "intranet_config").ok()? {
        return None;
    }
    let blob: Vec<u8> = conn
        .query_row("SELECT payload FROM intranet_config WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()
        .ok()??;
    let plain = crate::protection::unseal(&blob).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&plain).ok()?;
    let name = value.get("member_name")?.as_str()?.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// Takes every piece of synchronization out of the copy (see
/// [`SYNC_TABLES`]), in one transaction. Answers what was there, in words.
pub fn detach(conn: &Connection) -> Result<Vec<String>> {
    let tx = conn.unchecked_transaction()?;
    let mut cleared = Vec::new();
    for (table, label) in SYNC_TABLES {
        if !table_exists(&tx, table)? {
            continue;
        }
        let rows = tx
            .execute(&format!("DELETE FROM {}", quoted(table)), [])
            .with_context(|| format!("Não foi possível desligar a sincronização ({table})."))?;
        if rows > 0 {
            cleared.push(format!("{label} ({rows})"));
        }
    }
    tx.commit()?;
    Ok(cleared)
}

/// What the build of a recovery folder starts from.
pub(crate) struct Source<'a> {
    pub tipo: &'a str,
    pub origem: &'a Path,
    pub copia_de: String,
    pub esquema: i64,
}

/// Steps 3 to 6 on the library file `db` already in the new folder.
pub(crate) fn finish_recovery_library(
    folder: &Path,
    db: &Path,
    place: &Place,
    source: Source<'_>,
    now: NaiveDateTime,
) -> Result<Marker> {
    #[cfg(test)]
    if KILL_BEFORE_DETACH.with(|k| k.get()) {
        KILLED.with(|k| k.set(true));
        bail!("montagem interrompida (teste)");
    }
    let (member, cleared) = {
        let conn = open_raw(db)?;
        verify_sealed(&conn)?;
        let member = member_name(&conn);
        let cleared = detach(&conn)?;
        conn.close()
            .map_err(|(_, e)| e)
            .context("Não foi possível concluir o desligamento da sincronização.")?;
        (member, cleared)
    };
    let documents = {
        let mut store = Store::open(db)
            .context("O acervo de recuperação não pôde ser aberto depois de montado.")?;
        let mut config = crate::intranet::IntranetConfig::default();
        config.host_url.clear();
        if let Some(member) = member {
            config.member_name = member;
        }
        store.save_intranet_config(&config)?;
        let check = store.load_intranet_config()?;
        ensure!(
            check.mode == crate::intranet::IntranetMode::Local
                && check.folder_paths().is_empty()
                && store.list_offline_outbox()?.is_empty(),
            "O acervo de recuperação não ficou desligado da sincronização; nada foi criado."
        );
        // After the migration, in the schema of this build: every value
        // this schema keeps sealed opens, whatever happened to its mark.
        if let Err(problem) = verify_sealed_strict(&store.conn)? {
            bail!(foreign_message(&problem));
        }
        store.list_documents("")?.len()
    };
    // Nothing of the original's sync sidecars may live here.
    for suffix in ["-guarda.ponte", "-geracao.json"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", db.display()));
        ensure!(
            !sidecar.exists(),
            "O acervo de recuperação ganhou um arquivo de sincronização ({}); nada foi criado.",
            sidecar.display()
        );
    }
    for name in MACHINE_FILES {
        let from = place.current.join(name);
        if from.is_file() {
            let _ = std::fs::copy(&from, folder.join(name));
        }
    }
    // The holidays of the library are the ones in force: this computer's old
    // local list is not handed over to it.
    let _ = std::fs::write(folder.join("feriados.semeado"), "1\n");
    let marker = Marker {
        versao: 1,
        tipo: source.tipo.to_owned(),
        origem: source.origem.display().to_string(),
        copia_de: source.copia_de,
        montado_em: moment(now),
        pasta_principal: place.home.as_ref().map(|h| h.display().to_string()),
        documentos: documents,
        esquema_da_copia: source.esquema,
        desligado: cleared,
        completo: true,
    };
    crate::atomic_file::write_atomic(
        &folder.join(MARKER_FILE),
        &serde_json::to_vec_pretty(&marker)?,
    )
    .context("Não foi possível concluir o acervo de recuperação.")?;
    Ok(marker)
}

/// Creates a new recovery folder for `place` and runs `fill` in it; the
/// folder goes away (with everything `fill` put there) if `fill` fails.
pub(crate) fn build_in_new_folder(
    place: &Place,
    now: NaiveDateTime,
    fill: impl FnOnce(&Path) -> Result<Marker>,
) -> Result<Built> {
    let mut folder = NewFolder::create(place, now)?;
    let marker = fill(&folder.path)?;
    let published = folder.publish()?;
    Ok(Built {
        folder: published,
        marker,
    })
}

/// "Restaurar cópia…" of a copy made on this computer (a `.sqlite3`): a new
/// recovery folder, detached and verified. The source is only read.
pub fn restore_local_copy(source: &Path, place: &Place, now: NaiveDateTime) -> Result<Built> {
    match sniff(source)? {
        FileKind::Database => {}
        FileKind::Portable => bail!(PORTABLE_HINT),
        FileKind::SyncFile => bail!(SYNC_HINT),
        FileKind::Other => bail!(
            "{} não é uma cópia do acervo do Ponte. As cópias deste computador terminam em .sqlite3 e ficam na pasta backups do acervo. Nada foi criado.",
            source.display()
        ),
    }
    let reader = open_source(source)?;
    let schema = check_source(&reader, source)?;
    // Read inside the same transaction as the copy below.
    let copia_de = std::fs::metadata(source)
        .and_then(|m| m.modified())
        .map(|t| moment(chrono::DateTime::<chrono::Local>::from(t).naive_local()))
        .unwrap_or_default();
    build_in_new_folder(place, now, |folder| {
        let db = paths::db_path_in(folder);
        let partial = folder.join("acervo-em-montagem.sqlite3");
        crate::upgrade_backup::copy_from(&reader, &partial)?;
        crate::upgrade_backup::verify(&partial)?;
        crate::atomic_file::move_no_replace(&partial, &db)
            .context("Não foi possível dar o nome final ao acervo de recuperação.")?;
        finish_recovery_library(
            folder,
            &db,
            place,
            Source {
                tipo: "copia-local",
                origem: source,
                copia_de,
                esquema: schema,
            },
            now,
        )
    })
}

/// One library this computer can use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    pub folder: PathBuf,
    /// `None` for the main library (and for a folder the pointer names that
    /// is not a recovery folder).
    pub marker: Option<Marker>,
    pub main: bool,
    pub in_use: bool,
}

impl Library {
    pub fn title(&self) -> String {
        match &self.marker {
            _ if self.main => "Acervo principal".to_owned(),
            Some(marker) => format!(
                "Acervo restaurado em {} ({})",
                crate::auto_backup::human_moment(&marker.montado_em),
                marker.describe()
            ),
            None => "Acervo".to_owned(),
        }
    }
}

/// The libraries of the main folder `home`: the main one, the recovery
/// folders built from it (siblings with a marker that names it), and the one
/// the pointer names if it is none of these. The one in use is marked.
pub fn libraries(home: &Path) -> Vec<Library> {
    let in_use = paths::follow_pointer(home).data_dir;
    let mut found = vec![Library {
        folder: home.to_path_buf(),
        marker: None,
        main: true,
        in_use: paths::same_folder(&in_use, home),
    }];
    let prefix = format!(
        "{}-{FOLDER_WORD}-",
        home.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    if let Some(Ok(entries)) = home.parent().map(std::fs::read_dir) {
        let mut recovered: Vec<Library> = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| {
                let folder = e.path();
                let marker = read_marker(&folder)?;
                let mine = marker.home().is_some_and(|h| paths::same_folder(&h, home));
                (mine && paths::db_path_in(&folder).is_file()).then(|| Library {
                    in_use: paths::same_folder(&in_use, &folder),
                    folder,
                    marker: Some(marker),
                    main: false,
                })
            })
            .collect();
        recovered.sort_by(|a, b| {
            let at = |l: &Library| l.marker.as_ref().map(|m| m.montado_em.clone());
            at(b).cmp(&at(a))
        });
        found.extend(recovered);
    }
    if !found.iter().any(|l| l.in_use) {
        found.push(Library {
            marker: read_marker(&in_use),
            folder: in_use,
            main: false,
            in_use: true,
        });
    }
    found
}

/// Whether another process holds `file` in `dir` (a window's or a server's
/// lock): its exclusive handle makes this open fail with a sharing violation.
fn held_by_another(dir: &Path, file: &str) -> bool {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    match options.open(dir.join(file)) {
        Ok(_) => false,
        Err(error) => cfg!(windows) && error.raw_os_error() == Some(32),
    }
}

/// `ponteproj --serve`: held while it serves the library in `dir`, so the
/// library in use is not switched under it.
pub fn hold_server_lock(dir: &Path) -> Option<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(dir.join(SERVER_LOCK)).ok()
}

/// Refuses while another process of the Ponte uses a library of this
/// computer: another window (on any library of `home`, the main one or a
/// restored one), or a main computer without window (`--serve`) on any of
/// them. `own` is the folder of the window asking (its own lock is its own).
fn ensure_nobody_else_uses(home: &Path, target: &Path, own: Option<&Path>) -> Result<()> {
    let mut folders: Vec<PathBuf> = libraries(home).into_iter().map(|l| l.folder).collect();
    for extra in [Some(target), own].into_iter().flatten() {
        if !folders.iter().any(|f| paths::same_folder(f, extra)) {
            folders.push(extra.to_path_buf());
        }
    }
    for folder in &folders {
        let mine = own.is_some_and(|own| paths::same_folder(own, folder));
        if !mine && held_by_another(folder, WINDOW_LOCK) {
            bail!(
                "Feche a outra janela do Ponte para trocar de acervo (ela está aberta no acervo em {}). Nada foi trocado.",
                folder.display()
            );
        }
        if held_by_another(folder, SERVER_LOCK) {
            bail!(
                "Feche o Ponte que está servindo como computador principal sem janela (ponteproj --serve, no acervo em {}) para trocar de acervo. Nada foi trocado.",
                folder.display()
            );
        }
    }
    Ok(())
}

/// "Usar este acervo neste computador": the pointer of `home` names `folder`
/// (the main library or one of its recovery libraries). Nothing else changes.
/// Refused while another process of the Ponte is using a library of this
/// computer (see [`ensure_nobody_else_uses`]); `own` is the folder of the
/// window asking.
pub fn adopt(home: &Path, folder: &Path, own: Option<&Path>) -> Result<()> {
    ensure_nobody_else_uses(home, folder, own)?;
    ensure!(
        paths::same_folder(home, folder)
            || read_marker(folder)
                .and_then(|m| m.home())
                .is_some_and(|h| paths::same_folder(&h, home)),
        "{} não é um acervo deste computador.",
        folder.display()
    );
    paths::write_pointer(home, folder)
}

/// Opens a window of the Ponte on the recovery library in `folder`, apart
/// from the window of the library in use.
pub fn open_window(folder: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("Não foi possível localizar o programa do Ponte.")?;
    std::process::Command::new(exe)
        .env(paths::OVERRIDE_ENV, folder)
        .env(paths::RECOVERY_WINDOW_ENV, "1")
        .spawn()
        .context("Não foi possível abrir a janela do acervo de recuperação.")?;
    Ok(())
}

/// Opens a window of the Ponte on the library in use (as the Start menu does).
pub fn open_library_in_use() -> Result<()> {
    let exe = std::env::current_exe().context("Não foi possível localizar o programa do Ponte.")?;
    std::process::Command::new(exe)
        .env_remove(paths::OVERRIDE_ENV)
        .env_remove(paths::RECOVERY_WINDOW_ENV)
        .spawn()
        .context("Não foi possível abrir o Ponte no acervo em uso.")?;
    Ok(())
}

/// `ponteproj --restaurar-copia ARQUIVO`: builds the recovery library without
/// opening the library in use (it works when that library no longer opens).
/// Answers its folder; `main` then opens the window on it.
pub fn run_command(source: &Path) -> Result<PathBuf> {
    // Read now, not kept: the window that follows is opened on the new folder.
    let resolution = paths::fresh_resolution()?;
    let place = Place::new(resolution.home.as_deref(), &resolution.data_dir);
    let built = restore_local_copy(source, &place, chrono::Local::now().naive_local())?;
    println!(
        "Acervo de recuperação montado em {} ({} documentos), desligado de qualquer sincronização. O acervo em uso não foi alterado.",
        built.folder.display(),
        built.marker.documentos
    );
    Ok(built.folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intranet::{IntranetConfig, IntranetMode};

    fn capture(number: &str) -> crate::model::Capture {
        let mut c = crate::model::sample_capture();
        c.document_number = number.into();
        c.source_url = format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla={}",
            number.replace('/', "%2F")
        );
        c
    }

    fn now() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-11-22 10:15:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    /// A main folder with a library of `documents` documents and a task.
    fn main_library(local: &Path, documents: usize) -> (PathBuf, PathBuf) {
        let home = local.join("PonteProj");
        std::fs::create_dir_all(&home).unwrap();
        let db = paths::db_path_in(&home);
        let mut store = Store::open(&db).unwrap();
        for n in 0..documents {
            store
                .import_capture(&capture(&format!("SEFAZ-PRO-2026/{n:05}")))
                .unwrap();
        }
        drop(store);
        (home, db)
    }

    fn sha(path: &Path) -> String {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(std::fs::read(path).unwrap()))
    }

    /// What is in a folder, leaving out SQLite's own side files (a locking
    /// read of a copy may create its `-shm` beside it).
    fn folders(local: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(local)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with("-wal") && !n.ends_with("-shm"))
            .collect();
        names.sort();
        names
    }

    fn copy_of(db: &Path, to: &Path) -> PathBuf {
        let copy = to.join("copia.sqlite3");
        Store::open(db).unwrap().backup(&copy).unwrap();
        copy
    }

    #[test]
    fn a_copy_with_a_wal_beside_it_is_restored_whole_and_left_untouched() {
        let local = tempfile::tempdir().unwrap();
        let (home, _) = main_library(local.path(), 1);
        // The "copy" is a library file whose last changes are still in its WAL.
        let source_dir = local.path().join("pendrive");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("acervo-antigo.sqlite3");
        let mut store = Store::open(&source).unwrap();
        store
            .conn
            .execute_batch("PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        for n in 0..3 {
            store
                .import_capture(&capture(&format!("SEFAZ-PRO-2026/{n:05}")))
                .unwrap();
        }
        let wal = source_dir.join("acervo-antigo.sqlite3-wal");
        assert!(std::fs::metadata(&wal).unwrap().len() > 0);
        // Copy the pair as it is on disk while the writer is still alive.
        let frozen = local.path().join("congelada");
        std::fs::create_dir_all(&frozen).unwrap();
        let main_copy = frozen.join("copia.sqlite3");
        std::fs::copy(&source, &main_copy).unwrap();
        std::fs::copy(&wal, frozen.join("copia.sqlite3-wal")).unwrap();
        drop(store);
        let before = (sha(&main_copy), sha(&frozen.join("copia.sqlite3-wal")));
        let live_before = sha(&paths::db_path_in(&home));
        let built = restore_local_copy(&main_copy, &Place::new(Some(&home), &home), now()).unwrap();
        assert_eq!(built.marker.documentos, 3);
        assert_eq!(
            Store::open(&paths::db_path_in(&built.folder))
                .unwrap()
                .list_documents("")
                .unwrap()
                .len(),
            3
        );
        // The source and the library in use are untouched.
        assert_eq!(
            (sha(&main_copy), sha(&frozen.join("copia.sqlite3-wal"))),
            before
        );
        assert_eq!(sha(&paths::db_path_in(&home)), live_before);
        assert_eq!(
            built.folder.file_name().unwrap().to_string_lossy(),
            "PonteProj-recuperacao-20261122-101500"
        );
        assert_eq!(read_marker(&built.folder), Some(built.marker.clone()));
        // A second restore in the same second gets its own folder.
        let again = restore_local_copy(&main_copy, &Place::new(Some(&home), &home), now()).unwrap();
        assert_ne!(again.folder, built.folder);
    }

    fn v1_library(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE documents (id INTEGER PRIMARY KEY AUTOINCREMENT, identity_hash BLOB NOT NULL UNIQUE, latest_capture_id INTEGER, last_seen_at TEXT NOT NULL);
             CREATE TABLE captures (id INTEGER PRIMARY KEY AUTOINCREMENT, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, content_hash BLOB NOT NULL, payload BLOB NOT NULL, created_at TEXT NOT NULL, UNIQUE(document_id,content_hash));
             CREATE TABLE requests (request_id TEXT PRIMARY KEY, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE);
             CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, payload BLOB NOT NULL, created_at TEXT NOT NULL);
             CREATE TABLE drafts (id INTEGER PRIMARY KEY AUTOINCREMENT, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE, payload BLOB NOT NULL, created_at TEXT NOT NULL);
             CREATE TABLE analyses (id INTEGER PRIMARY KEY AUTOINCREMENT, document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE, capture_id INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE, payload BLOB NOT NULL, created_at TEXT NOT NULL);
             PRAGMA user_version=1;",
        )
        .unwrap();
    }

    #[test]
    fn an_older_copy_is_upgraded_only_in_the_new_folder() {
        let local = tempfile::tempdir().unwrap();
        let (home, _) = main_library(local.path(), 0);
        let source = local.path().join("antiga.sqlite3");
        v1_library(&source);
        let before = sha(&source);
        let built = restore_local_copy(&source, &Place::new(Some(&home), &home), now()).unwrap();
        assert_eq!(built.marker.esquema_da_copia, 1);
        assert_eq!(sha(&source), before, "a cópia original não muda");
        let version: i64 = Connection::open(paths::db_path_in(&built.folder))
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::storage::SCHEMA_VERSION);
    }

    #[test]
    fn a_copy_of_a_newer_ponte_is_refused_before_anything_is_created() {
        let local = tempfile::tempdir().unwrap();
        let (home, db) = main_library(local.path(), 1);
        let source = copy_of(&db, local.path());
        Connection::open(&source)
            .unwrap()
            .pragma_update(None, "user_version", crate::storage::SCHEMA_VERSION + 1)
            .unwrap();
        let before = folders(local.path());
        let error = restore_local_copy(&source, &Place::new(Some(&home), &home), now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("versão mais nova"), "{error}");
        assert_eq!(folders(local.path()), before);
    }

    #[test]
    fn what_is_not_a_library_copy_is_refused_with_what_to_do() {
        let local = tempfile::tempdir().unwrap();
        let (home, _) = main_library(local.path(), 0);
        let place = Place::new(Some(&home), &home);
        let cases: [(&str, &[u8], &str); 3] = [
            ("lixo.sqlite3", b"isto nao e um banco", "não é uma cópia"),
            (
                "copia.pontecopia",
                b"PONTE-COPIA1\n{}\n",
                "Importar cópia portátil",
            ),
            (
                "ponte-sync.ponte",
                b"PONTE-SYNC1\n{}\n",
                "Equipe e sincronização",
            ),
        ];
        for (name, bytes, expected) in cases {
            let path = local.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            let before = folders(local.path());
            let error = restore_local_copy(&path, &place, now())
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{name}: {error}");
            assert_eq!(folders(local.path()), before);
        }
        // A database that is not a library.
        let other = local.path().join("outro.sqlite3");
        Connection::open(&other)
            .unwrap()
            .execute_batch("CREATE TABLE x(a); PRAGMA user_version=3;")
            .unwrap();
        let error = restore_local_copy(&other, &place, now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("não um acervo do Ponte"), "{error}");
    }

    /// Another Windows account seals with another key: its payloads carry the
    /// same prefix and do not open here. Stood in for by payloads whose
    /// protected part is not this account's (DPAPI refuses them exactly as it
    /// refuses another account's).
    fn foreign(bytes: &[u8]) -> Vec<u8> {
        let mut sealed = crate::protection::seal(bytes).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x5a;
        sealed[20] ^= 0x5a;
        sealed
    }

    #[test]
    fn a_copy_of_another_windows_account_is_refused_whatever_holds_the_foreign_payload() {
        let local = tempfile::tempdir().unwrap();
        let (home, db) = main_library(local.path(), 2);
        let place = Place::new(Some(&home), &home);
        // Only the last payload of a single, rarely read table is foreign: a
        // sample of the first rows would miss it.
        for (table, column) in [("captures", "payload"), ("mail_accounts", "payload")] {
            let source = copy_of(&db, local.path());
            let conn = open_raw(&source).unwrap();
            if table == "mail_accounts" {
                let columns: Vec<(String, i64)> = conn
                    .prepare("SELECT name, \"notnull\" FROM pragma_table_info('mail_accounts')")
                    .unwrap()
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .unwrap()
                    .map(|r| r.unwrap())
                    .collect();
                let names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
                let values: Vec<String> = columns
                    .iter()
                    .map(|(n, _)| {
                        if n == "payload" {
                            "?1".to_owned()
                        } else if n == "id" {
                            "1".to_owned()
                        } else {
                            "'x'".to_owned()
                        }
                    })
                    .collect();
                conn.execute(
                    &format!(
                        "INSERT INTO mail_accounts({}) VALUES({})",
                        names.join(","),
                        values.join(",")
                    ),
                    [foreign(b"{}")],
                )
                .unwrap();
            } else {
                conn.execute(
                    &format!(
                        "UPDATE {table} SET {column}=?1 WHERE id=(SELECT max(id) FROM {table})"
                    ),
                    [foreign(b"{}")],
                )
                .unwrap();
            }
            drop(conn);
            let before = folders(local.path());
            let error = restore_local_copy(&source, &place, now())
                .unwrap_err()
                .to_string();
            assert!(error.contains("outra conta do Windows"), "{table}: {error}");
            assert_eq!(folders(local.path()), before, "{table}: nada criado");
            std::fs::remove_file(&source).unwrap();
        }
    }

    fn queue_something(store: &mut Store) {
        // An entry of the queue and a conflict, as a computer that was
        // offline leaves them (the contents do not matter here).
        let sealed = crate::protection::seal(b"{}").unwrap();
        store
            .conn
            .execute(
                "INSERT INTO offline_outbox(created_at,payload) VALUES('2026-01-01',?1)",
                [&sealed],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO offline_conflicts(created_at,payload) VALUES('2026-01-01',?1)",
                [&sealed],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT OR REPLACE INTO host_generation(id,value) VALUES(1,'geracao-antiga')",
                [],
            )
            .unwrap();
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn a_copy_of_a_folder_computer_a_client_or_a_main_computer_opens_alone() {
        let local = tempfile::tempdir().unwrap();
        let (home, _) = main_library(local.path(), 0);
        let place = Place::new(Some(&home), &home);
        for mode in [
            IntranetMode::SharedFolder,
            IntranetMode::Client,
            IntranetMode::Host,
        ] {
            let source_dir = local.path().join(format!("origem-{mode:?}"));
            std::fs::create_dir_all(&source_dir).unwrap();
            let source = source_dir.join("acervo.sqlite3");
            let mut store = Store::open(&source).unwrap();
            store
                .import_capture(&capture("SEFAZ-PRO-2026/00001"))
                .unwrap();
            let mut config = IntranetConfig {
                mode,
                member_name: "Ana".into(),
                ..IntranetConfig::default()
            };
            config.folder_files = vec![local
                .path()
                .join("Drive/ponte-sync.ponte")
                .display()
                .to_string()];
            config.folder_key = "senha-da-pasta".into();
            config.folder_lineage = "linhagem".into();
            config.synced_revision = 40;
            config.host_url = "http://192.168.0.10:8787".into();
            config.bind_addr = "0.0.0.0:8787".into();
            config.auto_start_host = true;
            config.host_generation = "g1".into();
            let old_token = config.cabinet_token.clone();
            store.save_intranet_config(&config).unwrap();
            queue_something(&mut store);
            // The sidecars of the original.
            std::fs::write(source_dir.join("acervo.sqlite3-guarda.ponte"), b"g").unwrap();
            std::fs::write(source_dir.join("acervo.sqlite3-geracao.json"), b"{}").unwrap();
            let copy = source_dir.join("copia.sqlite3");
            store.backup(&copy).unwrap();
            drop(store);

            let built = restore_local_copy(&copy, &place, now()).unwrap();
            assert!(!built.marker.desligado.is_empty());
            let db = paths::db_path_in(&built.folder);
            let mut restored = Store::open(&db).unwrap();
            let cfg = restored.load_intranet_config().unwrap();
            assert_eq!(cfg.mode, IntranetMode::Local, "{mode:?}");
            assert_eq!(cfg.mode.label(), "Só neste computador");
            assert_eq!(cfg.member_name, "Ana");
            assert!(cfg.folder_files.is_empty() && cfg.folder_key.is_empty());
            assert!(cfg.host_url.is_empty());
            assert_ne!(cfg.cabinet_token, old_token);
            assert!(!cfg.auto_start_host);
            assert!(cfg.folder_lineage.is_empty() && cfg.host_generation.is_empty());
            assert_eq!(cfg.synced_revision, 0);
            assert!(restored.list_offline_outbox().unwrap().is_empty());
            assert!(restored.list_sync_conflicts().unwrap().is_empty());
            let generations: i64 = restored
                .conn
                .query_row("SELECT count(*) FROM host_generation", [], |r| r.get(0))
                .unwrap();
            assert_eq!(generations, 0);
            assert!(!built.folder.join("acervo.sqlite3-guarda.ponte").exists());
            assert!(!built.folder.join("acervo.sqlite3-geracao.json").exists());
            assert_eq!(restored.list_documents("").unwrap().len(), 1);
            // The window's synchronization starts nothing.
            let mut panel = crate::intranet::IntranetPanel::default();
            panel.initialize(&mut restored, &db);
            assert!(!panel.sync_threads_running(), "{mode:?}");
            drop(restored);
            assert!(!built.folder.join("acervo.sqlite3-geracao.json").exists());
        }
    }

    #[test]
    fn adopting_writes_only_the_pointer_and_going_back_is_possible() {
        let local = tempfile::tempdir().unwrap();
        let (home, db) = main_library(local.path(), 2);
        let place = Place::new(Some(&home), &home);
        let copy = copy_of(&db, local.path());
        let built = restore_local_copy(&copy, &place, now()).unwrap();
        let live = sha(&db);
        let listed = libraries(&home);
        assert_eq!(listed.len(), 2);
        assert!(listed[0].main && listed[0].in_use);
        assert!(!listed[1].in_use && listed[1].marker.is_some());
        adopt(&home, &built.folder, Some(&built.folder)).unwrap();
        assert!(paths::same_folder(
            &paths::follow_pointer(&home).data_dir,
            &built.folder
        ));
        let listed = libraries(&home);
        assert!(!listed[0].in_use && listed[1].in_use);
        assert_eq!(sha(&db), live, "o acervo anterior fica inteiro");
        adopt(&home, &home, None).unwrap();
        assert_eq!(paths::follow_pointer(&home).data_dir, home);
        // A folder that is not one of this computer's libraries is refused.
        let stranger = local.path().join("estranho");
        std::fs::create_dir_all(&stranger).unwrap();
        std::fs::write(paths::db_path_in(&stranger), b"x").unwrap();
        assert!(adopt(&home, &stranger, None).is_err());
    }

    fn backups_before_replacement(db: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(db.parent().unwrap().join("copias-antes-da-substituicao"))
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "sqlite3"))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn joining_a_folder_with_only_tasks_asks_and_keeps_a_copy_first() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let files = vec![file.display().to_string()];
        let key = "senha-de-teste-123";
        let reporter = crate::ai::Reporter::silent();
        let folder_config = |member: &str| IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: files.clone(),
            folder_key: key.into(),
            ..IntranetConfig::default()
        };
        // The folder already holds a library with one document.
        let mut a = Store::open(&dir.path().join("a").join("acervo.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa")).unwrap();
        a.import_capture(&capture("SEFAZ-PRO-2026/00001")).unwrap();
        let cfg_a = a.load_intranet_config().unwrap();
        a.sync_with_folders(&cfg_a, key, false, &reporter).unwrap();
        // A library with no document at all, only a task (a restored one,
        // detached, then pointed at the folder).
        let db = dir.path().join("r").join("acervo.sqlite3");
        let mut r = Store::open(&db).unwrap();
        let mut task = crate::tasks::Task::standalone();
        task.title = "Tarefa feita só aqui".into();
        r.save_task(&task).unwrap();
        assert_eq!(
            r.local_items_summary().unwrap().as_deref(),
            Some("1 tarefa")
        );
        r.save_intranet_config(&folder_config("Notebook")).unwrap();
        let cfg_r = r.load_intranet_config().unwrap();
        let header_before = crate::sharedfolder::read_header(&file).unwrap();
        let error = r
            .sync_with_folders(&cfg_r, key, false, &reporter)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("[confirmar-substituicao]"), "{error}");
        assert!(error.contains("1 tarefa"), "{error}");
        // Nothing happened: the task is there, no copy yet, the folder as it was.
        assert_eq!(r.list_tasks().unwrap().len(), 1);
        assert!(backups_before_replacement(&db).is_empty());
        assert_eq!(
            crate::sharedfolder::read_header(&file).unwrap(),
            header_before
        );
        // Confirmed: a verified copy first, then the folder's library.
        r.sync_with_folders(&cfg_r, key, true, &reporter).unwrap();
        let kept = backups_before_replacement(&db);
        assert_eq!(kept.len(), 1);
        let safety = Store::open(&kept[0]).unwrap();
        assert!(safety
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Tarefa feita só aqui"));
        assert_eq!(r.list_documents("").unwrap().len(), 1);
        assert!(r
            .list_tasks()
            .unwrap()
            .iter()
            .all(|t| t.title != "Tarefa feita só aqui"));
        // An empty library joins without the question.
        let mut empty = Store::open(&dir.path().join("e").join("acervo.sqlite3")).unwrap();
        assert_eq!(empty.local_items_summary().unwrap(), None);
        empty.save_intranet_config(&folder_config("Outro")).unwrap();
        let cfg_e = empty.load_intranet_config().unwrap();
        empty
            .sync_with_folders(&cfg_e, key, false, &reporter)
            .unwrap();
    }

    #[test]
    fn joining_a_main_computer_with_only_a_project_asks_sends_nothing_and_keeps_a_copy() {
        use crate::intranet::{generate_token, HostServer};
        let dir = tempfile::tempdir().unwrap();
        let host_db = dir.path().join("host").join("acervo.sqlite3");
        let token = generate_token();
        let mut host = Store::open(&host_db).unwrap();
        host.import_capture(&capture("SEFAZ-PRO-2026/00007"))
            .unwrap();
        let mut host_cfg = IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "Principal".into(),
            bind_addr: "127.0.0.1:0".into(),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        };
        host.save_intranet_config(&host_cfg).unwrap();
        let mut server = HostServer::start(host_db.clone(), "127.0.0.1:0").unwrap();
        host_cfg.bind_addr = server.local_addr().to_string();
        host.save_intranet_config(&host_cfg).unwrap();
        let host_projects = host.list_projects().unwrap().len();

        let db = dir.path().join("r").join("acervo.sqlite3");
        let mut r = Store::open(&db).unwrap();
        r.save_project(&crate::projects::Project::new("Projeto feito só aqui"))
            .unwrap();
        assert_eq!(
            r.local_items_summary().unwrap().as_deref(),
            Some("1 projeto")
        );
        r.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "Restaurado".into(),
            host_url: format!("http://{}", server.local_addr()),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        })
        .unwrap();
        let error = r.sync_from_host(false).unwrap_err().to_string();
        assert!(error.starts_with("[confirmar-substituicao]"), "{error}");
        assert!(error.contains("1 projeto"), "{error}");
        assert_eq!(r.list_projects().unwrap().len(), 1);
        assert!(backups_before_replacement(&db).is_empty());
        assert_eq!(
            host.list_projects().unwrap().len(),
            host_projects,
            "nada enviado"
        );
        // Confirmed: kept first, then the main computer's library.
        assert!(r.sync_from_host_confirmed(false, true).unwrap());
        let kept = backups_before_replacement(&db);
        assert_eq!(kept.len(), 1);
        assert!(Store::open(&kept[0])
            .unwrap()
            .list_projects()
            .unwrap()
            .iter()
            .any(|p| p.name == "Projeto feito só aqui"));
        assert_eq!(r.list_documents("").unwrap().len(), 1);
        assert!(r
            .list_projects()
            .unwrap()
            .iter()
            .all(|p| p.name != "Projeto feito só aqui"));
        // From now on it is a member like any other: no question.
        r.sync_from_host(false).unwrap();
        server.stop();
    }

    #[test]
    fn an_interrupted_restore_leaves_nothing_that_opens() {
        let local = tempfile::tempdir().unwrap();
        let (home, _) = main_library(local.path(), 0);
        // A copy of a main computer with autostart: the role must never come
        // back, even from a build that was killed halfway.
        let source_dir = local.path().join("origem");
        std::fs::create_dir_all(&source_dir).unwrap();
        let mut store = Store::open(&source_dir.join("acervo.sqlite3")).unwrap();
        store
            .save_intranet_config(&IntranetConfig {
                mode: IntranetMode::Host,
                member_name: "Principal".into(),
                auto_start_host: true,
                ..IntranetConfig::default()
            })
            .unwrap();
        let copy = source_dir.join("copia.sqlite3");
        store.backup(&copy).unwrap();
        drop(store);
        let place = Place::new(Some(&home), &home);
        KILL_BEFORE_DETACH.with(|k| k.set(true));
        let outcome = restore_local_copy(&copy, &place, now());
        KILL_BEFORE_DETACH.with(|k| k.set(false));
        KILLED.with(|k| k.set(false));
        assert!(outcome.is_err());
        // What the killed build left: the copied library, not yet detached,
        // under the staging name, with its sentinel.
        let left: Vec<PathBuf> = std::fs::read_dir(local.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().contains("recuperacao"))
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
        let staging = &left[0];
        assert!(staging.to_string_lossy().ends_with(paths::STAGING_SUFFIX));
        let raw = Connection::open(paths::db_path_in(staging)).unwrap();
        let rows: i64 = raw
            .query_row("SELECT count(*) FROM intranet_config", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1, "the copy still carries the source's configuration");
        drop(raw);
        // No ordinary route opens it: named explicitly, through the pointer,
        // nor listed for adoption.
        let forced = paths::resolve(Some(staging.clone().into()), || unreachable!()).unwrap();
        assert!(paths::checked_db_path(&forced).is_err());
        assert!(paths::write_pointer(&home, staging).is_err());
        assert!(adopt(&home, staging, None).is_err());
        assert_eq!(libraries(&home).len(), 1);
        // Even renamed by hand to look finished.
        let renamed = local.path().join("PonteProj-recuperacao-20261122-101500");
        std::fs::rename(staging, &renamed).unwrap();
        let forced = paths::resolve(Some(renamed.clone().into()), || unreachable!()).unwrap();
        assert!(paths::checked_db_path(&forced).is_err());
        // A build that finishes publishes under the recovery name, ready.
        std::fs::remove_dir_all(&renamed).unwrap();
        let built = restore_local_copy(&copy, &place, now()).unwrap();
        assert!(!built
            .folder
            .to_string_lossy()
            .ends_with(paths::STAGING_SUFFIX));
        assert!(!built.folder.join(paths::STAGING_SENTINEL).exists());
        assert!(built.marker.completo);
        let forced = paths::resolve(Some(built.folder.clone().into()), || unreachable!()).unwrap();
        assert!(paths::checked_db_path(&forced).is_ok());
    }

    #[test]
    fn a_value_whose_seal_mark_was_damaged_is_found_and_named() {
        let local = tempfile::tempdir().unwrap();
        let (home, db) = main_library(local.path(), 2);
        let source = copy_of(&db, local.path());
        {
            let mut store = Store::open(&source).unwrap();
            let doc = store.list_documents("").unwrap()[0].id;
            store.save_note(doc, "Nota").unwrap();
            // The mark itself is gone: a scan for marked values skips it.
            store
                .conn
                .execute("UPDATE notes SET payload=X'00112233445566778899'", [])
                .unwrap();
        }
        let before = folders(local.path());
        let error = restore_local_copy(&source, &Place::new(Some(&home), &home), now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("tabela notes, registro nº"), "{error}");
        assert!(error.contains("outra conta do Windows"), "{error}");
        assert_eq!(folders(local.path()), before);
    }

    /// An exclusive handle, as a window of the Ponte keeps on its lock.
    fn hold(dir: &Path, file: &str) -> std::fs::File {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        options.open(dir.join(file)).unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn the_library_in_use_is_not_switched_while_another_ponte_uses_one() {
        let local = tempfile::tempdir().unwrap();
        let (home, db) = main_library(local.path(), 1);
        let copy = copy_of(&db, local.path());
        let built = restore_local_copy(&copy, &Place::new(Some(&home), &home), now()).unwrap();
        // The window of the main library is still open: the recovery window
        // may not adopt.
        let other_window = hold(&home, WINDOW_LOCK);
        let own_window = hold(&built.folder, WINDOW_LOCK);
        let error = adopt(&home, &built.folder, Some(&built.folder))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Feche a outra janela do Ponte"), "{error}");
        assert_eq!(paths::follow_pointer(&home).data_dir, home);
        drop(other_window);
        // A main computer without window on the main library: refused too.
        let server = hold_server_lock(&home).unwrap();
        let error = adopt(&home, &built.folder, Some(&built.folder))
            .unwrap_err()
            .to_string();
        assert!(error.contains("--serve"), "{error}");
        drop(server);
        // Alone: the window's own lock is its own.
        adopt(&home, &built.folder, Some(&built.folder)).unwrap();
        drop(own_window);
        assert!(paths::same_folder(
            &paths::follow_pointer(&home).data_dir,
            &built.folder
        ));
    }

    /// A main computer with one document, serving on a free local port.
    fn principal(dir: &Path) -> (Store, crate::intranet::HostServer, String, PathBuf) {
        use crate::intranet::{generate_token, HostServer};
        let host_db = dir.join("host").join("acervo.sqlite3");
        let token = generate_token();
        let mut host = Store::open(&host_db).unwrap();
        host.import_capture(&capture("SEFAZ-PRO-2026/00007"))
            .unwrap();
        let mut cfg = IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "Principal".into(),
            bind_addr: "127.0.0.1:0".into(),
            cabinet_token: token.clone(),
            ..IntranetConfig::default()
        };
        host.save_intranet_config(&cfg).unwrap();
        let server = HostServer::start(host_db.clone(), "127.0.0.1:0").unwrap();
        cfg.bind_addr = server.local_addr().to_string();
        host.save_intranet_config(&cfg).unwrap();
        (host, server, token, host_db)
    }

    fn client_of(server: &crate::intranet::HostServer, token: &str) -> IntranetConfig {
        IntranetConfig {
            mode: IntranetMode::Client,
            member_name: "Restaurado".into(),
            host_url: format!("http://{}", server.local_addr()),
            cabinet_token: token.to_owned(),
            ..IntranetConfig::default()
        }
    }

    /// What the main computer holds, to prove nothing reached it.
    fn state_of(host: &Store) -> (i64, Vec<String>) {
        (
            host.cabinet_revision().unwrap(),
            host.list_documents("")
                .unwrap()
                .into_iter()
                .map(|d| d.document_number)
                .collect(),
        )
    }

    #[test]
    fn a_first_time_client_sends_nothing_before_its_replacement_is_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let (host, mut server, token, _) = principal(dir.path());
        let host_before = state_of(&host);
        // A restored library with a document of its own joins the principal.
        let db = dir.path().join("r").join("acervo.sqlite3");
        let mut r = Store::open(&db).unwrap();
        let own = r.import_capture(&capture("SEFAZ-PRO-2026/00001")).unwrap();
        r.save_intranet_config(&client_of(&server, &token)).unwrap();
        assert!(r.load_intranet_config().unwrap().join_pending());
        // Between saving the configuration and confirming: a real edit…
        r.save_note(own.document_id, "Nota editada antes de confirmar")
            .unwrap();
        // …and a real browser import, through the native host's handler.
        let mut input = Vec::new();
        crate::native::write_frame(
            &mut input,
            &serde_json::json!({
                "protocol_version": 1,
                "action": "import_capture",
                "capture": capture("SEFAZ-PRO-2026/00002"),
            }),
        )
        .unwrap();
        let mut output = Vec::new();
        crate::native::serve(&mut input.as_slice(), &mut output, &mut r, |_| false).unwrap();
        let answer: serde_json::Value = serde_json::from_slice(
            &crate::native::read_frame(&mut output.as_slice())
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(answer["ok"], true, "{answer}");
        // Nothing reached the principal; both stay here, in the queue.
        assert_eq!(state_of(&host), host_before);
        assert_eq!(r.list_documents("").unwrap().len(), 2);
        assert_eq!(r.list_offline_outbox().unwrap().len(), 2);
        assert!(r.load_intranet_config().unwrap().join_pending());
        // Refused (not confirmed): nothing changes on either side.
        let error = r.sync_from_host(false).unwrap_err().to_string();
        assert!(error.starts_with("[confirmar-substituicao]"), "{error}");
        assert_eq!(state_of(&host), host_before);
        assert_eq!(r.list_documents("").unwrap().len(), 2);
        // Confirmed, but the safety copy cannot be made: nothing changes.
        let blocker = dir.path().join("r").join("copias-antes-da-substituicao");
        std::fs::write(&blocker, b"um arquivo no lugar da pasta").unwrap();
        assert!(r.sync_from_host_confirmed(false, true).is_err());
        assert_eq!(state_of(&host), host_before);
        assert_eq!(r.list_documents("").unwrap().len(), 2);
        assert_eq!(r.list_offline_outbox().unwrap().len(), 2);
        assert_eq!(
            r.load_document(own.document_id).unwrap().note,
            "Nota editada antes de confirmar"
        );
        // Confirmed with room for the copy: kept first, then replaced; the
        // two changes become conflicts, never sent.
        std::fs::remove_file(&blocker).unwrap();
        assert!(r.sync_from_host_confirmed(false, true).unwrap());
        assert_eq!(state_of(&host), host_before);
        let numbers: Vec<String> = r
            .list_documents("")
            .unwrap()
            .into_iter()
            .map(|d| d.document_number)
            .collect();
        assert_eq!(numbers, host_before.1);
        assert!(r.list_offline_outbox().unwrap().is_empty());
        assert_eq!(r.list_sync_conflicts().unwrap().len(), 2);
        assert!(!r.load_intranet_config().unwrap().join_pending());
        let kept = backups_before_replacement(&db);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            Store::open(&kept[0])
                .unwrap()
                .list_documents("")
                .unwrap()
                .len(),
            2
        );
        server.stop();
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn a_library_open_only_to_look_at_never_exchanges() {
        let dir = tempfile::tempdir().unwrap();
        let (host, mut server, token, _) = principal(dir.path());
        let host_before = state_of(&host);
        // A client that already worked with the principal (no pending join).
        let db = dir.path().join("c").join("acervo.sqlite3");
        let mut c = Store::open(&db).unwrap();
        c.save_intranet_config(&client_of(&server, &token)).unwrap();
        assert!(c.sync_from_host(true).unwrap());
        let doc = c.list_documents("").unwrap()[0].id;
        // Opened to look at: changes stay here, no exchange runs.
        c.set_transport_off(true);
        c.save_note(doc, "Nota feita só para olhar").unwrap();
        assert_eq!(state_of(&host), host_before);
        assert_eq!(c.list_offline_outbox().unwrap().len(), 1);
        let error = c.sync_from_host(false).unwrap_err().to_string();
        assert_eq!(error, crate::intranet::TRANSPORT_OFF);
        let mut panel = crate::intranet::IntranetPanel::default();
        panel.set_transport_off(true);
        panel.initialize(&mut c, &db);
        assert!(!panel.sync_threads_running());
        // A folder library and a main computer with autostart: nothing starts.
        let folder_db = dir.path().join("f").join("acervo.sqlite3");
        let mut f = Store::open(&folder_db).unwrap();
        f.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: "Casa".into(),
            folder_files: vec![dir
                .path()
                .join("Drive/ponte-sync.ponte")
                .display()
                .to_string()],
            folder_key: "senha-de-teste-123".into(),
            ..IntranetConfig::default()
        })
        .unwrap();
        f.set_transport_off(true);
        let cfg = f.load_intranet_config().unwrap();
        let error = f
            .sync_with_folders(
                &cfg,
                "senha-de-teste-123",
                true,
                &crate::ai::Reporter::silent(),
            )
            .unwrap_err()
            .to_string();
        assert_eq!(error, crate::intranet::TRANSPORT_OFF);
        assert!(!dir.path().join("Drive/ponte-sync.ponte").exists());
        let host_db2 = dir.path().join("h2").join("acervo.sqlite3");
        let mut h2 = Store::open(&host_db2).unwrap();
        h2.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "Outro principal".into(),
            bind_addr: "127.0.0.1:0".into(),
            auto_start_host: true,
            ..IntranetConfig::default()
        })
        .unwrap();
        let mut looking = crate::intranet::IntranetPanel::default();
        looking.set_transport_off(true);
        looking.initialize(&mut h2, &host_db2);
        assert!(!looking.sync_threads_running());
        // The same library, as the one in use, starts its main computer.
        let mut in_use = crate::intranet::IntranetPanel::default();
        in_use.initialize(&mut h2, &host_db2);
        assert!(in_use.sync_threads_running());
        in_use.set_transport_off(true);
        assert!(!in_use.sync_threads_running());
        server.stop();
    }

    #[test]
    fn the_join_question_counts_every_kind_of_item_the_snapshot_replaces() {
        use crate::intranet::{LIBRARY_INVENTORY, SNAPSHOT_BOOKKEEPING};
        let value = serde_json::to_value(crate::intranet::CabinetSnapshot::default()).unwrap();
        let mut fields = Vec::new();
        for (key, inner) in value.as_object().unwrap() {
            match inner.as_object() {
                Some(nested) => fields.extend(nested.keys().map(|k| format!("{key}.{k}"))),
                None => fields.push(key.clone()),
            }
        }
        for field in &fields {
            assert!(
                LIBRARY_INVENTORY.iter().any(|(f, ..)| f == field)
                    || SNAPSHOT_BOOKKEEPING.contains(&field.as_str()),
                "o campo {field} do retrato não é contado"
            );
        }
        // And every counted table exists in a library of this build.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        for (_, table, ..) in LIBRARY_INVENTORY {
            assert!(table_exists(&store.conn, table).unwrap(), "{table}");
        }
        assert_eq!(store.local_items_summary().unwrap(), None);
    }

    #[test]
    fn a_saved_holiday_setting_alone_makes_a_join_ask() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let key = "senha-de-teste-123";
        let config = |member: &str| IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: vec![file.display().to_string()],
            folder_key: key.into(),
            ..IntranetConfig::default()
        };
        let mut a = Store::open(&dir.path().join("a").join("acervo.sqlite3")).unwrap();
        a.save_intranet_config(&config("Casa")).unwrap();
        a.import_capture(&capture("SEFAZ-PRO-2026/00001")).unwrap();
        let cfg_a = a.load_intranet_config().unwrap();
        a.sync_with_folders(&cfg_a, key, false, &crate::ai::Reporter::silent())
            .unwrap();
        // Only a saved calendar, nothing else.
        let mut r = Store::open(&dir.path().join("r").join("acervo.sqlite3")).unwrap();
        r.set_library_setting("holidays", "uf MT\nanual 09-07 Feriado próprio\n")
            .unwrap();
        assert_eq!(
            r.local_items_summary().unwrap().as_deref(),
            Some("1 configuração do acervo (como os feriados)")
        );
        r.save_intranet_config(&config("Outro")).unwrap();
        let cfg_r = r.load_intranet_config().unwrap();
        let error = r
            .sync_with_folders(&cfg_r, key, false, &crate::ai::Reporter::silent())
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("[confirmar-substituicao]"), "{error}");
        assert!(r.library_setting("holidays").unwrap().is_some());
    }
}
