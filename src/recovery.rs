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
pub const MARKER_FILE: &str = "acervo-de-recuperacao.json";
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

pub fn read_marker(folder: &Path) -> Option<Marker> {
    serde_json::from_slice(&std::fs::read(folder.join(MARKER_FILE)).ok()?).ok()
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

/// The folder this run creates; removed with everything in it (all of it
/// this run's) unless the build finished.
struct NewFolder {
    path: PathBuf,
    keep: bool,
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
            let path = parent.join(name);
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path, keep: false }),
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
}

impl Drop for NewFolder {
    fn drop(&mut self) {
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

/// The chosen copy, opened only to be read. A copy with no WAL beside it is
/// opened as immutable: SQLite then creates nothing beside it (no `-wal`, no
/// `-shm` left on the pendrive). With a WAL beside it, it is opened read-only
/// as usual, so the committed changes still in the WAL are part of what is
/// read (SQLite may need to create its `-shm` there).
fn open_source(source: &Path) -> Result<Connection> {
    let wal = PathBuf::from(format!("{}-wal", source.display()));
    let has_wal = std::fs::metadata(&wal).is_ok_and(|m| m.len() > 0);
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = if has_wal {
        Connection::open_with_flags(source, flags)
    } else {
        Connection::open_with_flags(
            immutable_uri(source),
            flags | OpenFlags::SQLITE_OPEN_URI,
        )
    };
    conn.with_context(|| {
        format!(
            "Não foi possível abrir a cópia {}. Se ela está num pendrive, num CD ou numa pasta só de leitura, copie-a antes para uma pasta deste computador. Nada foi criado.",
            source.display()
        )
    })
}

/// `file:///C:/…?immutable=1`, with the characters a URI cannot carry escaped.
fn immutable_uri(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = paths::readable(&absolute).to_string_lossy().replace('\\', "/");
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

/// Opens every sealed payload of every table (every row, every column, not a
/// sample) with this Windows account. Answers how many were opened.
pub fn verify_sealed(conn: &Connection) -> Result<usize> {
    let mut opened = 0usize;
    for table in tables(conn)? {
        let columns: Vec<String> = conn
            .prepare(&format!("SELECT name FROM pragma_table_info({})", {
                let q = table.replace('\'', "''");
                format!("'{q}'")
            }))?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        for column in columns {
            let sql = format!(
                "SELECT {c} FROM {t} WHERE typeof({c})='blob' AND substr({c},1,6)=X'504F4E54452D'",
                c = quoted(&column),
                t = quoted(&table)
            );
            let mut statement = conn.prepare(&sql)?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let bytes: Vec<u8> = row.get(0)?;
                if !crate::protection::looks_sealed(&bytes) {
                    continue;
                }
                if crate::protection::unseal(&bytes).is_err() {
                    bail!(
                        "Esta cópia foi feita em outra conta do Windows, ou está danificada: {} não puderam ser abertos nesta conta ({opened} itens protegidos abertos antes deste). Uma cópia deste computador só abre na mesma conta do Windows em que foi feita. Nada foi criado. Se você tem uma cópia portátil (.pontecopia), use «Importar cópia portátil…».",
                        table_label(&table)
                    );
                }
                opened += 1;
            }
        }
    }
    Ok(opened)
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
    folder.keep = true;
    Ok(Built {
        folder: folder.path.clone(),
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
    let copia_de = std::fs::metadata(source)
        .and_then(|m| m.modified())
        .map(|t| moment(chrono::DateTime::<chrono::Local>::from(t).naive_local()))
        .unwrap_or_default();
    build_in_new_folder(place, now, |folder| {
        let db = paths::db_path_in(folder);
        let partial = folder.join("acervo-em-montagem.sqlite3");
        crate::upgrade_backup::copy_from(&reader, &partial)?;
        crate::upgrade_backup::verify(&partial)?;
        std::fs::rename(&partial, &db)
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
                let mine = marker
                    .home()
                    .is_some_and(|h| paths::same_folder(&h, home));
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

/// "Usar este acervo neste computador": the pointer of `home` names `folder`
/// (the main library or one of its recovery libraries). Nothing else changes.
pub fn adopt(home: &Path, folder: &Path) -> Result<()> {
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

    fn folders(local: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(local)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
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
            let error = restore_local_copy(&path, &place, now()).unwrap_err().to_string();
            assert!(error.contains(expected), "{name}: {error}");
            assert_eq!(folders(local.path()), before);
        }
        // A database that is not a library.
        let other = local.path().join("outro.sqlite3");
        Connection::open(&other)
            .unwrap()
            .execute_batch("CREATE TABLE x(a); PRAGMA user_version=3;")
            .unwrap();
        let error = restore_local_copy(&other, &place, now()).unwrap_err().to_string();
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
                    .map(|(n, _)| if n == "payload" { "?1".to_owned() } else if n == "id" { "1".to_owned() } else { "'x'".to_owned() })
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
                    &format!("UPDATE {table} SET {column}=?1 WHERE id=(SELECT max(id) FROM {table})"),
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
        // Nothing was left beside the chosen copy.
        assert!(!local.path().join("copia.sqlite3-wal").exists());
        assert!(!local.path().join("copia.sqlite3-shm").exists());
        let live = sha(&db);
        let listed = libraries(&home);
        assert_eq!(listed.len(), 2);
        assert!(listed[0].main && listed[0].in_use);
        assert!(!listed[1].in_use && listed[1].marker.is_some());
        adopt(&home, &built.folder).unwrap();
        assert!(paths::same_folder(
            &paths::follow_pointer(&home).data_dir,
            &built.folder
        ));
        let listed = libraries(&home);
        assert!(!listed[0].in_use && listed[1].in_use);
        assert_eq!(sha(&db), live, "o acervo anterior fica inteiro");
        adopt(&home, &home).unwrap();
        assert_eq!(paths::follow_pointer(&home).data_dir, home);
        // A folder that is not one of this computer's libraries is refused.
        let stranger = local.path().join("estranho");
        std::fs::create_dir_all(&stranger).unwrap();
        std::fs::write(paths::db_path_in(&stranger), b"x").unwrap();
        assert!(adopt(&home, &stranger).is_err());
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
        assert_eq!(r.local_items_summary().unwrap().as_deref(), Some("1 tarefa"));
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
        assert_eq!(crate::sharedfolder::read_header(&file).unwrap(), header_before);
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
        assert!(r.list_tasks().unwrap().iter().all(|t| t.title != "Tarefa feita só aqui"));
        // An empty library joins without the question.
        let mut empty = Store::open(&dir.path().join("e").join("acervo.sqlite3")).unwrap();
        assert_eq!(empty.local_items_summary().unwrap(), None);
        empty.save_intranet_config(&folder_config("Outro")).unwrap();
        let cfg_e = empty.load_intranet_config().unwrap();
        empty.sync_with_folders(&cfg_e, key, false, &reporter).unwrap();
    }

    #[test]
    fn joining_a_main_computer_with_only_a_project_asks_sends_nothing_and_keeps_a_copy() {
        use crate::intranet::{generate_token, HostServer};
        let dir = tempfile::tempdir().unwrap();
        let host_db = dir.path().join("host").join("acervo.sqlite3");
        let token = generate_token();
        let mut host = Store::open(&host_db).unwrap();
        host.import_capture(&capture("SEFAZ-PRO-2026/00007")).unwrap();
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
        assert_eq!(r.local_items_summary().unwrap().as_deref(), Some("1 projeto"));
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
        assert_eq!(host.list_projects().unwrap().len(), host_projects, "nada enviado");
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
}
