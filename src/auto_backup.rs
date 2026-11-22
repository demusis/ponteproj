//! The copies of Ajustes ▸ Cópia de segurança: the automatic copy of the day
//! and the one the person asks for ("Fazer cópia agora").
//!
//! Both are made by the verified engine of the installers' copy
//! (`upgrade_backup::backup_named`): SQLite's online backup from a read-only
//! connection (the committed content still in the WAL is part of the copy), a
//! `quick_check` of the copy, and a final name that never replaces a file.
//!
//! The automatic copies live in a folder of their own,
//! `<pasta do acervo>\backups\automaticas`, with a small manifest
//! (`manifesto.json`) that records every copy this feature made: name,
//! publication sequence, moment, size, SHA-256 and whether it is a good copy.
//! A copy is good only when it passes the check a restore requires
//! (`recovery::verify_sealed_strict`: every sealed value of the library opens
//! with this Windows account); a copy that does not is kept, named in the card
//! with what is damaged, and never counts as a good one.
//!
//! Retention only ever looks at the manifest, and only right after a new good
//! copy was made and recorded: a file is deleted only when the manifest lists
//! it, its name is one this feature gives, it is a plain file inside that
//! folder, and its size and SHA-256 are still the ones recorded (a file put in
//! its place is left alone). Kept: the [`KEEP_RECENT`] most recent good copies
//! by publication sequence (never by the clock), always the one just made, and
//! the oldest copy of each of the last [`ANCHOR_WEEKS`] weeks. When the clock
//! went back (a copy in the manifest is newer than now), nothing is pruned in
//! that run. Manual copies, the installers' copies and anything else are
//! never touched.
//!
//! A copy needs room: before it starts, the volume must have the size of the
//! library (with its WAL) plus a reserve for the library's own writes, the
//! larger of [`RESERVE_MIN`] and a quarter of the library ([`needed_space`]).
//! Otherwise nothing is copied and the card says "sem espaço".
//!
//! Every copy is announced in the manifest (`pendente`) before it starts: a
//! copy published by a run that died before recording it is found by that
//! record at the next run and recorded then (never orphaned, never copied
//! twice in a day).
//!
//! Whether the copy is on and how the last one went is kept per computer in
//! `copia-automatica.json`, in the library folder (never in the library: a
//! restored library would bring an old answer back). The lock of the folder
//! is held from the check of "is it due?" to the record of the result, and
//! the switch on/off takes it too.
use crate::atomic_file::write_atomic;
use crate::upgrade_backup::{self, Outcome};
use anyhow::{bail, Context, Result};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};

/// The folder of the automatic copies, inside the `backups` folder.
pub const FOLDER: &str = "automaticas";
/// The manifest of the automatic copies, inside [`FOLDER`].
pub const MANIFEST: &str = "manifesto.json";
/// The state of the automatic copy, in the library folder.
pub const STATE_FILE: &str = "copia-automatica.json";
/// How every automatic copy's name starts.
const PREFIX: &str = "automatica-";
/// How every manual copy's name starts (the name older versions gave too).
const MANUAL_PREFIX: &str = "ponte-";
/// Held while a copy is being made, so two windows never make it at once.
const LOCK_FILE: &str = "copia-em-andamento.lock";
/// The most recent good automatic copies always kept.
pub const KEEP_RECENT: usize = 7;
/// Weeks back for which the oldest copy of the week is also kept.
pub const ANCHOR_WEEKS: i64 = 4;
/// The least room left for the library's own writes after a copy.
pub const RESERVE_MIN: u64 = 256 * 1024 * 1024;
const MANIFEST_VERSION: u32 = 1;

/// `<pasta do acervo>\backups`: the installers' and the manual copies.
pub fn backups_folder(data_dir: &Path) -> PathBuf {
    upgrade_backup::default_destination(data_dir)
}

/// `<pasta do acervo>\backups\automaticas`.
pub fn folder(data_dir: &Path) -> PathBuf {
    backups_folder(data_dir).join(FOLDER)
}

/// One copy recorded in the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// File name inside the folder of the automatic copies.
    pub arquivo: String,
    /// Publication sequence (grows, never reused, even after a deletion).
    pub sequencia: u64,
    /// Local moment of the copy, `AAAA-MM-DDTHH:MM:SS`.
    pub criada_em: String,
    /// Size of the copy in bytes.
    pub bytes: u64,
    /// SHA-256 of the copy as published: checked again before deleting it.
    #[serde(default)]
    pub sha256: String,
    /// The copy passes everything a restore requires (a good copy).
    pub verificada: bool,
    /// What made it not a good copy (the table and the record).
    #[serde(default)]
    pub problema: String,
    /// Which program made it (`Ponte 0.50.0`).
    pub origem: String,
}

impl Entry {
    fn moment(&self) -> Option<NaiveDateTime> {
        NaiveDateTime::parse_from_str(&self.criada_em, "%Y-%m-%dT%H:%M:%S").ok()
    }
}

/// A copy announced before it starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub sequencia: u64,
    /// The name it is published under (`<nome>.sqlite3`, `<nome>-2.sqlite3`…).
    pub nome: String,
    pub iniciada_em: String,
}

/// The record of the automatic copies.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub versao: u32,
    pub proxima_sequencia: u64,
    pub copias: Vec<Entry>,
    #[serde(default)]
    pub pendente: Option<Pending>,
}

impl Manifest {
    fn new() -> Self {
        Self {
            versao: MANIFEST_VERSION,
            proxima_sequencia: 1,
            copias: Vec::new(),
            pendente: None,
        }
    }
}

/// How the last automatic copy went.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastResult {
    /// Local moment, `AAAA-MM-DDTHH:MM:SS`.
    pub quando: String,
    pub ok: bool,
    /// The copy made, or what went wrong.
    pub mensagem: String,
}

/// The automatic copy on this computer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default = "default_true")]
    pub ativa: bool,
    /// Day (`AAAA-MM-DD`) of the last copy that was made.
    #[serde(default)]
    pub ultimo_dia: String,
    #[serde(default)]
    pub ultimo_resultado: Option<LastResult>,
}

fn default_true() -> bool {
    true
}

impl Default for State {
    fn default() -> Self {
        Self {
            ativa: true,
            ultimo_dia: String::new(),
            ultimo_resultado: None,
        }
    }
}

/// The state, or the default one (on, never made) when there is none or it
/// cannot be read.
pub fn load_state(data_dir: &Path) -> State {
    std::fs::read(data_dir.join(STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_state(data_dir: &Path, state: &State) -> Result<()> {
    write_atomic(
        &data_dir.join(STATE_FILE),
        &serde_json::to_vec_pretty(state)?,
    )
    .context("Não foi possível gravar a escolha da cópia automática.")
}

/// Turns the automatic copy on or off. Takes the lock of the copies, so the
/// choice is never overwritten by a copy recording its result.
pub fn set_enabled(data_dir: &Path, on: bool) -> Result<()> {
    let folder = folder(data_dir);
    let _lock = match std::fs::create_dir_all(&folder) {
        Ok(()) => match lock(&folder)? {
            Some(lock) => Some(lock),
            None => bail!("A cópia automática está sendo feita agora; tente de novo em instantes."),
        },
        // No folder (a disk that is gone): no copy can be running either.
        Err(_) => None,
    };
    let mut state = load_state(data_dir);
    state.ativa = on;
    save_state(data_dir, &state)
}

fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

fn moment_text(moment: NaiveDateTime) -> String {
    moment.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Whether the copy of `today` is still to be made.
pub fn due(state: &State, today: NaiveDate) -> bool {
    state.ativa && state.ultimo_dia != day_text(today)
}

/// What one run did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunReport {
    /// The new copy (or the one of today found recorded late), published.
    pub copy: Option<PathBuf>,
    /// Names of the old automatic copies removed by retention.
    pub removed: Vec<String>,
    /// Something to tell although the copy was made.
    pub warning: Option<String>,
    /// Nothing was done because another run is making the copy right now.
    pub busy: bool,
}

impl RunReport {
    fn warn(&mut self, text: String) {
        self.warning = Some(match self.warning.take() {
            Some(before) => format!("{before} {text}"),
            None => text,
        });
    }
}

/// Whether `name` is a name this feature gives:
/// `automatica-AAAAMMDD-HHMMSS[-N].sqlite3`, a plain name.
fn owned_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(PREFIX)
        .and_then(|r| r.strip_suffix(".sqlite3"))
    else {
        return false;
    };
    let (stamp, suffix) = match rest.get(15..) {
        Some(after) => (&rest[..15], after),
        None => return false,
    };
    let stamp_ok = stamp.len() == 15
        && stamp
            .char_indices()
            .all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() });
    let suffix_ok = suffix.is_empty()
        || suffix
            .strip_prefix('-')
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    stamp_ok && suffix_ok
}

/// What makes a manifest one this feature wrote and may act on.
fn validate(manifest: &Manifest) -> std::result::Result<(), String> {
    if manifest.versao != MANIFEST_VERSION {
        return Err(format!("versão {} desconhecida", manifest.versao));
    }
    let mut sequences = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for entry in &manifest.copias {
        if !owned_name(&entry.arquivo) {
            return Err(format!("nome inesperado «{}»", entry.arquivo));
        }
        if entry.moment().is_none() {
            return Err(format!("momento ilegível em «{}»", entry.arquivo));
        }
        if !entry.sha256.is_empty()
            && !(entry.sha256.len() == 64 && entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(format!("identificação inválida em «{}»", entry.arquivo));
        }
        if entry.sequencia >= manifest.proxima_sequencia || !sequences.insert(entry.sequencia) {
            return Err(format!("sequência inválida em «{}»", entry.arquivo));
        }
        if !names.insert(entry.arquivo.as_str()) {
            return Err(format!("«{}» aparece duas vezes", entry.arquivo));
        }
    }
    if let Some(pending) = &manifest.pendente {
        let probe = format!("{}.sqlite3", pending.nome);
        if !owned_name(&probe) || pending.sequencia >= manifest.proxima_sequencia {
            return Err("cópia anunciada inválida".to_owned());
        }
    }
    Ok(())
}

/// The manifest as found: `Err` holds the reason when it exists but cannot be
/// read or is not one this feature wrote (the copies it listed are then never
/// deleted automatically).
fn read_manifest(path: &Path) -> std::result::Result<Manifest, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            validate(&manifest)?;
            Ok(manifest)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::new()),
        Err(error) => Err(error.to_string()),
    }
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(manifest)?)
        .context("Não foi possível gravar o registro das cópias automáticas.")
}

/// The lock of the folder: `None` when another process holds it.
fn lock(folder: &Path) -> Result<Option<File>> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    match options.open(folder.join(LOCK_FILE)) {
        Ok(file) => Ok(Some(file)),
        Err(error) if cfg!(windows) && error.raw_os_error() == Some(32) => Ok(None),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Não foi possível usar a pasta das cópias automáticas ({}).",
                folder.display()
            )
        }),
    }
}

/// Free space for this process on the volume of `path`.
pub fn free_space(path: &Path) -> std::io::Result<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0u64;
        // SAFETY: a NUL-terminated path and valid output pointers.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(available)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(u64::MAX)
    }
}

/// The room a copy of the library `database` needs: its size (with its WAL)
/// and the reserve left for the library's own writes.
pub fn needed_space(database: &Path) -> (u64, u64) {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let copy = size(database) + size(&PathBuf::from(format!("{}-wal", database.display())));
    (copy, RESERVE_MIN.max(copy / 4))
}

/// SHA-256 of a file, read in pieces.
fn file_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// The check a restore requires, on a copy just published (nobody else knows
/// it yet: opened as immutable, nothing is created beside it). `Ok(None)`: a
/// good copy; `Ok(Some(problem))`: a copy kept but not a good one.
fn check_copy(path: &Path) -> Result<Option<String>> {
    let conn = crate::recovery::open_own_copy(path)?;
    Ok(match crate::recovery::verify_sealed_strict(&conn)? {
        Ok(_) => None,
        Err(problem) => Some(format!("{problem}.")),
    })
}

/// The automatic copy of the library `database`, now (local time `now`), in
/// [`folder`]`(data_dir)`, then retention. Takes the lock itself; does not
/// look at the state (see [`run_and_record`]).
pub fn run(database: &Path, data_dir: &Path, now: NaiveDateTime) -> Result<RunReport> {
    run_with(database, data_dir, now, &free_space)
}

/// [`run`] with the measure of free space given (the tests stand in for a
/// full disk).
pub fn run_with(
    database: &Path,
    data_dir: &Path,
    now: NaiveDateTime,
    space: &dyn Fn(&Path) -> std::io::Result<u64>,
) -> Result<RunReport> {
    let folder = create_folder(data_dir)?;
    let Some(_lock) = lock(&folder)? else {
        return Ok(RunReport {
            busy: true,
            ..Default::default()
        });
    };
    run_locked(database, &folder, now, space)
}

fn create_folder(data_dir: &Path) -> Result<PathBuf> {
    let folder = folder(data_dir);
    std::fs::create_dir_all(&folder).with_context(|| {
        format!(
            "Não foi possível criar a pasta das cópias automáticas ({}). Confira se o disco está acessível e tem espaço.",
            folder.display()
        )
    })?;
    Ok(folder)
}

/// Records a copy published under `path` (a new one, or one a run that died
/// published): its identity and whether it is a good copy.
fn record(manifest: &mut Manifest, path: &Path, sequence: u64, moment: &str) -> Result<Entry> {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let problem = check_copy(path)?;
    let entry = Entry {
        arquivo: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sequencia: sequence,
        criada_em: moment.to_owned(),
        bytes,
        sha256: file_sha256(path).context("Não foi possível identificar a cópia publicada.")?,
        verificada: problem.is_none(),
        problema: problem.unwrap_or_default(),
        origem: format!("Ponte {}", env!("CARGO_PKG_VERSION")),
    };
    manifest.copias.push(entry.clone());
    manifest.proxima_sequencia = manifest.proxima_sequencia.max(sequence + 1);
    Ok(entry)
}

/// The copies a run that died before recording them published: found by the
/// name it announced, recorded now. Answers the most recent one found.
fn reconcile(manifest: &mut Manifest, folder: &Path) -> Result<Option<Entry>> {
    let Some(pending) = manifest.pendente.clone() else {
        return Ok(None);
    };
    let mut found = None;
    let mut sequence = pending.sequencia;
    for n in 1..=200u32 {
        let name = if n == 1 {
            format!("{}.sqlite3", pending.nome)
        } else {
            format!("{}-{n}.sqlite3", pending.nome)
        };
        let path = folder.join(&name);
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            continue;
        }
        if manifest.copias.iter().any(|e| e.arquivo == name) {
            continue;
        }
        let entry = record(manifest, &path, sequence, &pending.iniciada_em)?;
        sequence = manifest.proxima_sequencia;
        found = Some(entry);
    }
    manifest.pendente = None;
    Ok(found)
}

fn run_locked(
    database: &Path,
    folder: &Path,
    now: NaiveDateTime,
    space: &dyn Fn(&Path) -> std::io::Result<u64>,
) -> Result<RunReport> {
    let manifest_path = folder.join(MANIFEST);
    let mut report = RunReport::default();
    let (mut manifest, readable) = match read_manifest(&manifest_path) {
        Ok(manifest) => (manifest, true),
        Err(reason) => {
            // Kept for whoever wants to look at it; a new record starts.
            let aside = folder.join(format!(
                "manifesto-ilegivel-{}.json",
                now.format("%Y%m%d-%H%M%S")
            ));
            let _ = std::fs::rename(&manifest_path, &aside);
            report.warn(format!(
                "O registro das cópias automáticas não pôde ser usado ({reason}); ele foi guardado em {} e as cópias anteriores não serão apagadas automaticamente.",
                aside.display()
            ));
            (Manifest::new(), false)
        }
    };
    let today = now.date();
    // A copy of today published by a run that died before recording it: it is
    // the copy of today.
    let late = reconcile(&mut manifest, folder)?;
    let current = match late.filter(|e| e.moment().is_some_and(|m| m.date() == today)) {
        Some(entry) => {
            write_manifest(&manifest_path, &manifest)?;
            entry
        }
        None => {
            let (copy, reserve) = needed_space(database);
            let free = space(folder).with_context(|| {
                format!(
                    "Não foi possível saber o espaço livre em {}.",
                    folder.display()
                )
            })?;
            if free < copy.saturating_add(reserve) {
                bail!(
                    "Sem espaço para a cópia automática em {}: há {} livres, e ela precisa de {} para a cópia mais {} de reserva para o próprio acervo continuar gravando. Nada foi copiado; libere espaço no disco.",
                    folder.display(),
                    human_size(free),
                    human_size(copy),
                    human_size(reserve)
                );
            }
            let sequence = manifest.proxima_sequencia;
            let stem = format!("{PREFIX}{}", now.format("%Y%m%d-%H%M%S"));
            manifest.pendente = Some(Pending {
                sequencia: sequence,
                nome: stem.clone(),
                iniciada_em: moment_text(now),
            });
            manifest.proxima_sequencia = sequence + 1;
            // Announced before it starts: if this run dies after publishing,
            // the next one finds and records it.
            write_manifest(&manifest_path, &manifest)?;
            let path = match upgrade_backup::backup_named(database, folder, &stem, PREFIX)? {
                Outcome::BackedUp(path) => path,
                Outcome::NoLibrary => bail!("Não há acervo em {} para copiar.", database.display()),
            };
            report.copy = Some(path.clone());
            let entry = record(&mut manifest, &path, sequence, &moment_text(now))?;
            manifest.pendente = None;
            // Recorded before anything is removed: a copy that is not in the
            // record is never a reason to delete another one.
            if let Err(error) = write_manifest(&manifest_path, &manifest) {
                report.warn(format!(
                    "A cópia foi feita, mas o registro das cópias não pôde ser gravado ({error:#}); nenhuma cópia antiga foi apagada."
                ));
                return Ok(report);
            }
            entry
        }
    };
    report.copy = Some(folder.join(&current.arquivo));
    if !current.verificada {
        report.warn(format!(
            "A cópia de hoje foi guardada, mas não conta como cópia boa: {} Nenhuma cópia anterior foi apagada.",
            current.problema
        ));
        return Ok(report);
    }
    // Never after a manifest that could not be read: what it listed is unknown.
    if !readable {
        return Ok(report);
    }
    // The clock went back: the calendar cannot be trusted in this run.
    let latest_other = manifest
        .copias
        .iter()
        .filter(|e| e.sequencia != current.sequencia)
        .filter_map(Entry::moment)
        .max();
    if latest_other.is_some_and(|latest| latest > now) {
        return Ok(report);
    }
    let remove = plan_retention(&manifest.copias, today, current.sequencia, |entry| {
        owned_file(folder, entry).is_some()
    });
    let mut forgotten = Vec::new();
    for index in remove {
        let entry = &manifest.copias[index];
        let Some(file) = owned_file(folder, entry) else {
            continue;
        };
        if !same_identity(&file, entry) {
            // Not the copy that was recorded under this name: not ours.
            forgotten.push(index);
            report.warn(format!(
                "{} não é mais a cópia registrada com esse nome e foi deixado como está.",
                file.display()
            ));
            continue;
        }
        if std::fs::remove_file(&file).is_ok() {
            forgotten.push(index);
            report.removed.push(entry.arquivo.clone());
        }
    }
    if !forgotten.is_empty() {
        let mut index = 0;
        manifest.copias.retain(|_| {
            let keep = !forgotten.contains(&index);
            index += 1;
            keep
        });
        if let Err(error) = write_manifest(&manifest_path, &manifest) {
            report.warn(format!(
                "As cópias antigas foram apagadas, mas o registro não pôde ser atualizado ({error:#})."
            ));
        }
    }
    Ok(report)
}

/// Whether the file is still the copy recorded: same size, same SHA-256.
fn same_identity(file: &Path, entry: &Entry) -> bool {
    !entry.sha256.is_empty()
        && std::fs::metadata(file).is_ok_and(|m| m.len() == entry.bytes)
        && file_sha256(file).is_ok_and(|hash| hash == entry.sha256)
}

/// The file of a recorded copy, only when it is one this feature may delete:
/// a name this feature gives, of a plain file (not a link, not a folder)
/// inside `folder`.
fn owned_file(folder: &Path, entry: &Entry) -> Option<PathBuf> {
    if !owned_name(&entry.arquivo) {
        return None;
    }
    let path = folder.join(&entry.arquivo);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    meta.is_file().then_some(path)
}

/// Which recorded copies retention removes (indexes into `entries`), with
/// today's date and the copy just published (`current`). Only good copies
/// whose moment can be read and that `present` says are there take part: the
/// [`KEEP_RECENT`] with the highest publication sequence are kept, always
/// `current`, and the oldest one of each of the last [`ANCHOR_WEEKS`] weeks
/// (Monday to Sunday, this week included). Everything else that takes part is
/// removed; what does not take part is never removed.
pub fn plan_retention(
    entries: &[Entry],
    today: NaiveDate,
    current: u64,
    present: impl Fn(&Entry) -> bool,
) -> Vec<usize> {
    let mut candidates: Vec<(usize, NaiveDateTime, u64)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.verificada && present(e))
        .filter_map(|(i, e)| Some((i, e.moment()?, e.sequencia)))
        .collect();
    // Latest publication first.
    candidates.sort_by_key(|c| std::cmp::Reverse(c.2));
    let mut keep: std::collections::BTreeSet<usize> = candidates
        .iter()
        .take(KEEP_RECENT)
        .map(|(i, _, _)| *i)
        .collect();
    keep.extend(
        candidates
            .iter()
            .filter(|(_, _, seq)| *seq == current)
            .map(|(i, _, _)| *i),
    );
    let this_monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    for week in 0..ANCHOR_WEEKS {
        let start = this_monday - chrono::Duration::weeks(week);
        let end = start + chrono::Duration::days(7);
        if let Some((index, _, _)) = candidates
            .iter()
            .filter(|(_, at, _)| at.date() >= start && at.date() < end)
            .min_by_key(|(_, at, seq)| (*at, *seq))
        {
            keep.insert(*index);
        }
    }
    candidates
        .iter()
        .map(|(i, _, _)| *i)
        .filter(|i| !keep.contains(i))
        .collect()
}

/// The run of the day as the window does it, under the lock from the check of
/// "is it due?" to the record of the result. `Ok(None)`: nothing was due
/// (off, already made today, or another window is making it). A result that
/// cannot be recorded is said in the report.
pub fn run_and_record(
    database: &Path,
    data_dir: &Path,
    now: NaiveDateTime,
) -> Result<Option<RunReport>> {
    run_and_record_with(database, data_dir, now, &free_space)
}

pub fn run_and_record_with(
    database: &Path,
    data_dir: &Path,
    now: NaiveDateTime,
    space: &dyn Fn(&Path) -> std::io::Result<u64>,
) -> Result<Option<RunReport>> {
    if !due(&load_state(data_dir), now.date()) {
        return Ok(None);
    }
    let outcome = create_folder(data_dir).and_then(|folder| {
        let Some(lock) = lock(&folder)? else {
            return Ok(None);
        };
        // Again under the lock: another window may have made it meanwhile.
        if !due(&load_state(data_dir), now.date()) {
            return Ok(None);
        }
        Ok(Some((lock, run_locked(database, &folder, now, space))))
    });
    let (lock, outcome) = match outcome {
        Ok(None) => return Ok(None),
        Ok(Some((lock, outcome))) => (Some(lock), outcome),
        Err(error) => (None, Err(error)),
    };
    let mut state = load_state(data_dir);
    let moment = moment_text(now);
    state.ultimo_resultado = Some(match &outcome {
        Ok(report) => {
            state.ultimo_dia = day_text(now.date());
            LastResult {
                quando: moment,
                ok: report.warning.is_none(),
                mensagem: match (&report.warning, &report.copy) {
                    (Some(warning), _) => warning.clone(),
                    (None, Some(copy)) => copy.display().to_string(),
                    (None, None) => String::new(),
                },
            }
        }
        Err(error) => LastResult {
            quando: moment,
            ok: false,
            mensagem: format!("{error:#}"),
        },
    });
    let saved = save_state(data_dir, &state);
    drop(lock);
    match (outcome, saved) {
        (Ok(mut report), Err(error)) => {
            report.warn(format!(
                "O resultado não pôde ser gravado ({error:#}); a cópia pode ser feita de novo na próxima abertura."
            ));
            Ok(Some(report))
        }
        (Ok(report), Ok(())) => Ok(Some(report)),
        (Err(error), Err(saving)) => Err(error.context(format!(
            "e o resultado também não pôde ser gravado ({saving:#})"
        ))),
        (Err(error), Ok(())) => Err(error),
    }
}

/// "Fazer cópia agora": a verified copy in the `backups` folder, named as
/// before (`ponte-<data UTC>-<id>.sqlite3`). Never part of retention.
pub fn manual_copy(database: &Path, data_dir: &Path) -> Result<PathBuf> {
    let stem = format!(
        "{MANUAL_PREFIX}{}-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        uuid::Uuid::new_v4()
    );
    match upgrade_backup::backup_named(database, &backups_folder(data_dir), &stem, MANUAL_PREFIX)? {
        Outcome::BackedUp(path) => Ok(path),
        Outcome::NoLibrary => anyhow::bail!("Não há acervo em {} para copiar.", database.display()),
    }
}

/// What the card shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub folder: PathBuf,
    /// Good automatic copies recorded and present.
    pub count: usize,
    pub bytes: u64,
    /// Moment of the most recent of them.
    pub newest: Option<String>,
    /// The most recent copy kept that is not a good one: when, and what is
    /// damaged.
    pub damaged: Option<(String, String)>,
    pub state: State,
}

pub fn summary(data_dir: &Path) -> Summary {
    let folder = folder(data_dir);
    let mut summary = Summary {
        state: load_state(data_dir),
        ..Default::default()
    };
    if let Ok(manifest) = read_manifest(&folder.join(MANIFEST)) {
        let mut latest_bad: Option<&Entry> = None;
        for entry in &manifest.copias {
            let Some(file) = owned_file(&folder, entry) else {
                continue;
            };
            summary.bytes += std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
            if !entry.verificada {
                if latest_bad.is_none_or(|b| b.sequencia < entry.sequencia) {
                    latest_bad = Some(entry);
                }
                continue;
            }
            summary.count += 1;
            if summary.newest.as_deref() < Some(entry.criada_em.as_str()) {
                summary.newest = Some(entry.criada_em.clone());
            }
        }
        // Only while no good copy came after it.
        let newest_good = manifest
            .copias
            .iter()
            .filter(|e| e.verificada)
            .map(|e| e.sequencia)
            .max();
        summary.damaged = latest_bad
            .filter(|bad| newest_good.is_none_or(|good| good < bad.sequencia))
            .map(|bad| (bad.criada_em.clone(), bad.problema.clone()));
    }
    summary.folder = folder;
    summary
}

/// `AAAA-MM-DDTHH:MM:SS` as `22/11/2026 às 09:14`.
pub fn human_moment(moment: &str) -> String {
    NaiveDateTime::parse_from_str(moment, "%Y-%m-%dT%H:%M:%S")
        .map(|m| m.format("%d/%m/%Y às %H:%M").to_string())
        .unwrap_or_else(|_| moment.to_owned())
}

/// Bytes as the card says them (`340 MB`, `1,2 GB`).
pub fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0).replace('.', ",")
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB").replace('.', ",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    fn library(path: &Path, rows: usize) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; CREATE TABLE notas (id INTEGER PRIMARY KEY, texto TEXT);",
        )
        .unwrap();
        for n in 0..rows {
            conn.execute(
                "INSERT INTO notas (texto) VALUES (?1)",
                [format!("nota {n}")],
            )
            .unwrap();
        }
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn copies_in(folder: &Path) -> Vec<String> {
        names(folder)
            .into_iter()
            .filter(|n| owned_name(n))
            .collect()
    }

    fn entry(name: &str, seq: u64, moment: &str) -> Entry {
        Entry {
            arquivo: name.into(),
            sequencia: seq,
            criada_em: moment.into(),
            bytes: 1,
            sha256: String::new(),
            verificada: true,
            problema: String::new(),
            origem: "teste".into(),
        }
    }

    fn plenty(_: &Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }

    #[test]
    fn the_names_this_feature_gives_and_nothing_else() {
        assert!(owned_name("automatica-20261122-091400.sqlite3"));
        assert!(owned_name("automatica-20261122-091400-2.sqlite3"));
        for name in [
            "automatica-20261122.sqlite3",
            "automatica-20261122-091400-.sqlite3",
            "automatica-20261122-0914000.sqlite3",
            "..\\automatica-20261122-091400.sqlite3",
            "automatica-20261122-091400.sqlite3.parcial",
            "minha-copia.sqlite3",
        ] {
            assert!(!owned_name(name), "{name}");
        }
    }

    #[test]
    fn retention_keeps_seven_recent_and_the_oldest_of_each_of_four_weeks() {
        // One copy a day for 40 days, ending on Sunday 2026-11-22.
        let today = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let entries: Vec<Entry> = (0..40)
            .map(|back| {
                let day = today - chrono::Duration::days(back);
                entry(
                    &format!("automatica-{}-090000.sqlite3", day.format("%Y%m%d")),
                    100 - back as u64,
                    &format!("{}T09:00:00", day.format("%Y-%m-%d")),
                )
            })
            .collect();
        let removed = plan_retention(&entries, today, 100, |_| true);
        let kept: Vec<String> = entries
            .iter()
            .enumerate()
            .filter(|(i, _)| !removed.contains(i))
            .map(|(_, e)| e.criada_em[..10].to_owned())
            .collect();
        assert_eq!(
            kept,
            [
                "2026-11-22",
                "2026-11-21",
                "2026-11-20",
                "2026-11-19",
                "2026-11-18",
                "2026-11-17",
                "2026-11-16", // Monday of this week: also its anchor
                "2026-11-09",
                "2026-11-02",
                "2026-10-26",
            ]
        );
    }

    #[test]
    fn the_recent_ones_are_counted_by_publication_and_the_new_one_is_always_kept() {
        // A clock that ran days ahead for a while: the copies it stamped look
        // newer than the one just made after it was corrected.
        let today = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let mut entries: Vec<Entry> = (0..10)
            .map(|n| {
                entry(
                    &format!("automatica-20261201-0900{n:02}.sqlite3"),
                    n,
                    &format!("2026-12-01T09:00:{n:02}"),
                )
            })
            .collect();
        entries.push(entry(
            "automatica-20261122-090000.sqlite3",
            10,
            "2026-11-22T09:00:00",
        ));
        let removed = plan_retention(&entries, today, 10, |_| true);
        assert!(!removed.contains(&10), "the copy just made stays");
        // The seven of highest sequence are 4..=10: 0..=3 go.
        assert_eq!(removed, vec![3, 2, 1, 0]);
    }

    #[test]
    fn retention_never_takes_what_is_unverified_unreadable_or_absent() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let mut entries: Vec<Entry> = (0..20)
            .map(|n| {
                entry(
                    &format!("automatica-20260101-0900{n:02}.sqlite3"),
                    n,
                    "2026-01-01T09:00:00",
                )
            })
            .collect();
        entries[0].verificada = false;
        entries[1].criada_em = "ontem".into();
        let removed = plan_retention(&entries, today, 19, |e| {
            e.arquivo != "automatica-20260101-090002.sqlite3"
        });
        assert!(!removed.contains(&0) && !removed.contains(&1) && !removed.contains(&2));
        // 17 take part, 7 recent kept, nothing in the last four weeks.
        assert_eq!(removed.len(), 10);
    }

    #[test]
    fn a_daily_copy_is_recorded_with_its_identity_and_old_ones_go_only_from_the_record() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 5);
        let folder = folder(data.path());
        std::fs::create_dir_all(&folder).unwrap();
        // Somebody's files in and around the folder, some with our kind of name.
        let foreign = folder.join("automatica-20200101-000000.sqlite3");
        std::fs::write(&foreign, b"nao e nosso").unwrap();
        std::fs::write(folder.join("minha-copia.sqlite3"), b"x").unwrap();
        let backups = backups_folder(data.path());
        std::fs::write(backups.join("ponte-20200101-000000-a.sqlite3"), b"manual").unwrap();
        std::fs::write(
            backups.join("antes-de-instalar-v0.1.0-20200101-000000.sqlite3"),
            b"instalador",
        )
        .unwrap();
        // Forty days of copies.
        let first = at("2026-10-14 09:00");
        for day in 0..40 {
            let report = run(&db, data.path(), first + chrono::Duration::days(day)).unwrap();
            assert!(report.warning.is_none(), "{:?}", report.warning);
            let copy = report.copy.unwrap();
            assert_eq!(
                Connection::open(&copy)
                    .unwrap()
                    .query_row("SELECT count(*) FROM notas", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                5
            );
        }
        let manifest = read_manifest(&folder.join(MANIFEST)).unwrap();
        // 7 recent + anchors of the 3 earlier weeks (this week's is among the 7).
        assert_eq!(manifest.copias.len(), 10, "{:?}", manifest.copias);
        for e in &manifest.copias {
            assert!(folder.join(&e.arquivo).is_file());
            assert_eq!(e.sha256.len(), 64);
            assert!(e.verificada);
        }
        assert!(manifest.pendente.is_none());
        // Nothing that is not ours was touched.
        assert!(foreign.is_file());
        assert!(folder.join("minha-copia.sqlite3").is_file());
        assert!(backups.join("ponte-20200101-000000-a.sqlite3").is_file());
        assert!(backups
            .join("antes-de-instalar-v0.1.0-20200101-000000.sqlite3")
            .is_file());
        let summary = summary(data.path());
        assert_eq!(summary.count, 10);
        assert!(summary.bytes > 0 && summary.damaged.is_none());
    }

    #[test]
    fn a_record_that_lists_names_it_did_not_give_is_put_aside_and_nothing_is_pruned() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        for day in 0..10 {
            run(
                &db,
                data.path(),
                at("2026-11-01 08:00") + chrono::Duration::days(day),
            )
            .unwrap();
        }
        let folder = folder(data.path());
        let outside = data.path().join("fora.sqlite3");
        std::fs::write(&outside, b"fora").unwrap();
        let mut manifest = read_manifest(&folder.join(MANIFEST)).unwrap();
        manifest
            .copias
            .push(entry("..\\fora.sqlite3", 900, "2020-01-01T00:00:00"));
        manifest.proxima_sequencia = 901;
        write_manifest(&folder.join(MANIFEST), &manifest).unwrap();
        let before = copies_in(&folder).len();
        let report = run(&db, data.path(), at("2026-11-30 08:00")).unwrap();
        assert!(report.removed.is_empty());
        assert!(report.warning.unwrap().contains("nome inesperado"));
        assert_eq!(copies_in(&folder).len(), before + 1);
        assert!(outside.is_file());
        // Another version of the record: the same.
        let mut other = read_manifest(&folder.join(MANIFEST)).unwrap();
        other.versao = 7;
        write_manifest(&folder.join(MANIFEST), &other).unwrap();
        let report = run(&db, data.path(), at("2026-12-01 08:00")).unwrap();
        assert!(report.removed.is_empty());
        assert!(report.warning.unwrap().contains("versão 7"));
    }

    #[test]
    fn a_file_put_in_place_of_a_recorded_copy_is_never_deleted() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        for day in 0..8 {
            run(
                &db,
                data.path(),
                at("2026-10-01 08:00") + chrono::Duration::days(day),
            )
            .unwrap();
        }
        let folder = folder(data.path());
        let manifest = read_manifest(&folder.join(MANIFEST)).unwrap();
        // The oldest recorded copy is replaced by somebody's file (same name).
        let oldest = manifest
            .copias
            .iter()
            .min_by_key(|e| e.sequencia)
            .unwrap()
            .arquivo
            .clone();
        std::fs::write(folder.join(&oldest), b"outro arquivo, de outra pessoa").unwrap();
        // Weeks later, retention would remove it.
        let report = run(&db, data.path(), at("2026-11-20 08:00")).unwrap();
        assert!(folder.join(&oldest).is_file());
        assert_eq!(
            std::fs::read(folder.join(&oldest)).unwrap(),
            b"outro arquivo, de outra pessoa"
        );
        assert!(!report.removed.contains(&oldest));
        assert!(report
            .warning
            .unwrap()
            .contains("não é mais a cópia registrada"));
        // And it is forgotten: never considered again.
        let manifest = read_manifest(&folder.join(MANIFEST)).unwrap();
        assert!(manifest.copias.iter().all(|e| e.arquivo != oldest));
    }

    #[test]
    fn when_the_clock_went_back_nothing_is_pruned() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        for day in 0..20 {
            run(
                &db,
                data.path(),
                at("2026-12-01 08:00") + chrono::Duration::days(day),
            )
            .unwrap();
        }
        let folder = folder(data.path());
        let before = copies_in(&folder);
        // Corrected clock: today is before the copies already made.
        let report = run(&db, data.path(), at("2026-11-22 08:00")).unwrap();
        assert!(report.removed.is_empty());
        let after = copies_in(&folder);
        assert_eq!(after.len(), before.len() + 1);
        assert!(after.contains(
            &report
                .copy
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        ));
    }

    #[test]
    fn a_failed_copy_removes_nothing_and_is_recorded_as_a_failure() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        for day in 0..12 {
            run(
                &db,
                data.path(),
                at("2026-11-01 08:00") + chrono::Duration::days(day),
            )
            .unwrap();
        }
        let folder = folder(data.path());
        let before = names(&folder);
        // The library becomes something that is not a database.
        std::fs::remove_file(&db).unwrap();
        std::fs::write(&db, vec![7u8; 8192]).unwrap();
        let outcome = run_and_record(&db, data.path(), at("2026-12-30 08:00"));
        assert!(outcome.is_err());
        assert_eq!(names(&folder), before, "nada apagado depois de uma falha");
        let state = load_state(data.path());
        let last = state.ultimo_resultado.unwrap();
        assert!(!last.ok && !last.mensagem.is_empty());
        assert_ne!(state.ultimo_dia, "2026-12-30", "o dia não conta como feito");
        // Still due: the next opening tries again.
        assert!(due(
            &load_state(data.path()),
            NaiveDate::from_ymd_opt(2026, 12, 30).unwrap()
        ));
    }

    #[test]
    fn without_room_for_the_copy_and_the_reserve_nothing_is_copied() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 50);
        let (copy, reserve) = needed_space(&db);
        assert!(copy > 0 && reserve == RESERVE_MIN);
        // Room for the copy, not for the reserve.
        let tight = move |_: &Path| -> std::io::Result<u64> { Ok(copy + reserve - 1) };
        let error = run_and_record_with(&db, data.path(), at("2026-11-22 08:00"), &tight)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("Sem espaço"), "{error}");
        assert!(copies_in(&folder(data.path())).is_empty());
        let last = load_state(data.path()).ultimo_resultado.unwrap();
        assert!(!last.ok && last.mensagem.starts_with("Sem espaço"));
        // With room, it is made.
        assert!(
            run_and_record_with(&db, data.path(), at("2026-11-22 09:00"), &plenty)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_copy_published_by_a_run_that_died_is_recorded_once_and_counts_for_the_day() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 2);
        let folder = create_folder(data.path()).unwrap();
        // The run of 08:00 announced its copy and published it, then died
        // before recording it.
        let mut manifest = Manifest::new();
        manifest.pendente = Some(Pending {
            sequencia: 1,
            nome: "automatica-20261122-080000".into(),
            iniciada_em: "2026-11-22T08:00:00".into(),
        });
        manifest.proxima_sequencia = 2;
        write_manifest(&folder.join(MANIFEST), &manifest).unwrap();
        let published =
            upgrade_backup::backup_named(&db, &folder, "automatica-20261122-080000", PREFIX)
                .unwrap();
        assert!(matches!(published, Outcome::BackedUp(_)));
        // The next opening, the same day: recorded, and no second copy.
        let report = run_and_record(&db, data.path(), at("2026-11-22 10:00"))
            .unwrap()
            .unwrap();
        assert!(report.warning.is_none(), "{:?}", report.warning);
        assert_eq!(copies_in(&folder), ["automatica-20261122-080000.sqlite3"]);
        let manifest = read_manifest(&folder.join(MANIFEST)).unwrap();
        assert!(manifest.pendente.is_none());
        assert_eq!(manifest.copias.len(), 1);
        assert_eq!(manifest.copias[0].sequencia, 1);
        assert!(manifest.copias[0].verificada);
    }

    #[test]
    fn a_damaged_item_in_the_library_keeps_the_copy_but_not_as_a_good_one() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        {
            let mut store = crate::storage::Store::open(&db).unwrap();
            let mut capture = crate::model::sample_capture();
            capture.document_number = "SEFAZ-PRO-2026/00001".into();
            let receipt = store.import_capture(&capture).unwrap();
            store.save_note(receipt.document_id, "Nota").unwrap();
        }
        for day in 0..9 {
            let report = run(
                &db,
                data.path(),
                at("2026-10-01 08:00") + chrono::Duration::days(day),
            )
            .unwrap();
            assert!(report.warning.is_none(), "{:?}", report.warning);
        }
        let folder = folder(data.path());
        let good_before = copies_in(&folder);
        // One note's sealed value is damaged in the library itself.
        let conn = Connection::open(&db).unwrap();
        conn.create_scalar_function(
            "ponte_client_schema",
            0,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            |_| Ok(crate::storage::SCHEMA_VERSION),
        )
        .unwrap();
        conn.execute("UPDATE notes SET payload=X'0011223344'", [])
            .unwrap();
        drop(conn);
        let report = run_and_record(&db, data.path(), at("2026-11-20 08:00"))
            .unwrap()
            .unwrap();
        let warning = report.warning.unwrap();
        assert!(warning.contains("não conta como cópia boa"), "{warning}");
        assert!(warning.contains("tabela notes, registro nº"), "{warning}");
        // The copy is kept, nothing was pruned.
        let after = copies_in(&folder);
        assert_eq!(after.len(), good_before.len() + 1);
        for name in &good_before {
            assert!(after.contains(name));
        }
        let summary = summary(data.path());
        assert_eq!(summary.count, good_before.len());
        let (_, what) = summary.damaged.unwrap();
        assert!(what.contains("tabela notes"), "{what}");
        assert!(!load_state(data.path()).ultimo_resultado.unwrap().ok);
    }

    #[test]
    fn once_a_day_and_never_when_off() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        let morning = at("2026-11-22 08:00");
        assert!(run_and_record(&db, data.path(), morning).unwrap().is_some());
        assert!(run_and_record(&db, data.path(), at("2026-11-22 18:00"))
            .unwrap()
            .is_none());
        set_enabled(data.path(), false).unwrap();
        assert!(run_and_record(&db, data.path(), at("2026-11-23 08:00"))
            .unwrap()
            .is_none());
        set_enabled(data.path(), true).unwrap();
        assert!(run_and_record(&db, data.path(), at("2026-11-23 08:00"))
            .unwrap()
            .is_some());
        assert!(load_state(data.path()).ultimo_resultado.unwrap().ok);
    }

    #[cfg(windows)]
    #[test]
    fn a_second_run_or_the_switch_while_one_is_making_the_copy_does_nothing() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        let folder = folder(data.path());
        std::fs::create_dir_all(&folder).unwrap();
        let held = lock(&folder).unwrap().unwrap();
        let report = run(&db, data.path(), at("2026-11-22 08:00")).unwrap();
        assert!(report.busy && report.copy.is_none());
        assert!(run_and_record(&db, data.path(), at("2026-11-22 08:00"))
            .unwrap()
            .is_none());
        assert!(set_enabled(data.path(), false).is_err());
        assert!(load_state(data.path()).ativa);
        drop(held);
        assert!(run(&db, data.path(), at("2026-11-22 08:00"))
            .unwrap()
            .copy
            .is_some());
    }

    #[test]
    fn the_manual_copy_is_verified_and_keeps_its_old_name() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 3);
        let copy = manual_copy(&db, data.path()).unwrap();
        let name = copy.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("ponte-") && name.ends_with(".sqlite3"),
            "{name}"
        );
        assert_eq!(copy.parent().unwrap(), backups_folder(data.path()));
        // Not part of the automatic record.
        assert_eq!(summary(data.path()).count, 0);
    }

    #[test]
    fn sizes_and_moments_read_well() {
        assert_eq!(human_size(340 * 1024 * 1024), "340 MB");
        assert_eq!(human_size(1024 * 1024 + 300_000), "1,3 MB");
        assert_eq!(human_size(1288 * 1024 * 1024), "1,3 GB");
        assert_eq!(human_moment("2026-11-22T09:14:00"), "22/11/2026 às 09:14");
    }
}
