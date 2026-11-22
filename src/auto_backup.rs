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
//! sequence, moment, size and that it was verified. Retention only ever looks
//! at the manifest: a file is deleted only when the manifest lists it, its
//! name is one this feature gives, it is a plain file inside that folder, and
//! a new copy was just made and recorded. Manual copies, the installers'
//! copies and anything else (even a file that looks like an automatic copy
//! but is not in the manifest) are never touched. Kept: the
//! [`KEEP_RECENT`] most recent copies plus the oldest copy of each of the last
//! [`ANCHOR_WEEKS`] weeks.
//!
//! Whether the copy is on and how the last one went is kept per computer in
//! `copia-automatica.json`, in the library folder (never in the library: a
//! restored library would bring an old answer back).
use crate::atomic_file::write_atomic;
use crate::upgrade_backup::{self, Outcome};
use anyhow::{Context, Result};
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
/// The most recent automatic copies always kept.
pub const KEEP_RECENT: usize = 7;
/// Weeks back for which the oldest copy of the week is also kept.
pub const ANCHOR_WEEKS: i64 = 4;

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
    /// Growing number of the copy (never reused, even after a deletion).
    pub sequencia: u64,
    /// Local moment of the copy, `AAAA-MM-DDTHH:MM:SS`.
    pub criada_em: String,
    /// Size of the copy in bytes.
    pub bytes: u64,
    /// The copy passed `quick_check` before it got its name.
    pub verificada: bool,
    /// Which program made it (`Ponte 0.50.0`).
    pub origem: String,
}

impl Entry {
    fn moment(&self) -> Option<NaiveDateTime> {
        NaiveDateTime::parse_from_str(&self.criada_em, "%Y-%m-%dT%H:%M:%S").ok()
    }
}

/// The record of the automatic copies.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub versao: u32,
    pub proxima_sequencia: u64,
    pub copias: Vec<Entry>,
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
    /// Day (`AAAA-MM-DD`) of the last copy that worked.
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

/// Turns the automatic copy on or off.
pub fn set_enabled(data_dir: &Path, on: bool) -> Result<()> {
    let mut state = load_state(data_dir);
    state.ativa = on;
    save_state(data_dir, &state)
}

fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

/// Whether the copy of `today` is still to be made.
pub fn due(state: &State, today: NaiveDate) -> bool {
    state.ativa && state.ultimo_dia != day_text(today)
}

/// What one run did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunReport {
    /// The new copy, verified and recorded.
    pub copy: Option<PathBuf>,
    /// Names of the old automatic copies removed by retention.
    pub removed: Vec<String>,
    /// Something to tell although the copy was made.
    pub warning: Option<String>,
    /// Nothing was done because another run is making the copy right now.
    pub busy: bool,
}

/// The manifest as found: `Err` holds the reason when it exists but cannot be
/// read (the copies it listed are then never deleted automatically).
fn read_manifest(path: &Path) -> std::result::Result<Manifest, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Manifest {
            versao: 1,
            proxima_sequencia: 1,
            copias: Vec::new(),
        }),
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

/// The automatic copy of the library `database`, now (local time `now`), in
/// [`folder`]`(data_dir)`, then retention. Does not look at the state: the
/// caller decides whether it is due ([`run_and_record`] does both).
pub fn run(database: &Path, data_dir: &Path, now: NaiveDateTime) -> Result<RunReport> {
    let folder = folder(data_dir);
    std::fs::create_dir_all(&folder).with_context(|| {
        format!(
            "Não foi possível criar a pasta das cópias automáticas ({}). Confira se o disco está acessível e tem espaço.",
            folder.display()
        )
    })?;
    let Some(_lock) = lock(&folder)? else {
        return Ok(RunReport {
            busy: true,
            ..Default::default()
        });
    };
    let manifest_path = folder.join(MANIFEST);
    let mut report = RunReport::default();
    let mut manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => Some(manifest),
        Err(reason) => {
            report.warning = Some(format!(
                "O registro das cópias automáticas ({}) não pôde ser lido ({reason}); ele foi guardado ao lado e as cópias anteriores não serão apagadas automaticamente.",
                manifest_path.display()
            ));
            None
        }
    };
    let stem = format!("{PREFIX}{}", now.format("%Y%m%d-%H%M%S"));
    let path = match upgrade_backup::backup_named(database, &folder, &stem, PREFIX)? {
        Outcome::BackedUp(path) => path,
        Outcome::NoLibrary => anyhow::bail!(
            "Não há acervo em {} para copiar.",
            database.display()
        ),
    };
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    report.copy = Some(path);
    let mut manifest = match manifest.take() {
        Some(manifest) => manifest,
        None => {
            // Kept for whoever wants to look at it; a new record starts with
            // the copy just made.
            let aside = folder.join(format!(
                "manifesto-ilegivel-{}.json",
                now.format("%Y%m%d-%H%M%S")
            ));
            let _ = std::fs::rename(&manifest_path, aside);
            Manifest {
                versao: 1,
                proxima_sequencia: 1,
                copias: Vec::new(),
            }
        }
    };
    let sequence = manifest.proxima_sequencia.max(
        manifest
            .copias
            .iter()
            .map(|e| e.sequencia + 1)
            .max()
            .unwrap_or(1),
    );
    manifest.proxima_sequencia = sequence + 1;
    manifest.versao = 1;
    manifest.copias.push(Entry {
        arquivo: name,
        sequencia: sequence,
        criada_em: now.format("%Y-%m-%dT%H:%M:%S").to_string(),
        bytes,
        verificada: true,
        origem: format!("Ponte {}", env!("CARGO_PKG_VERSION")),
    });
    // Recorded before anything is removed: a copy that is not in the record
    // is never a reason to delete another one.
    if let Err(error) = write_manifest(&manifest_path, &manifest) {
        report.warning = Some(format!(
            "A cópia foi feita, mas o registro das cópias não pôde ser gravado ({error:#}); nenhuma cópia antiga foi apagada."
        ));
        return Ok(report);
    }
    // Never after a manifest that could not be read: what it listed is unknown.
    if report.warning.is_some() {
        return Ok(report);
    }
    let remove = plan_retention(&manifest.copias, now.date(), |entry| {
        owned_file(&folder, entry).is_some()
    });
    let mut gone = Vec::new();
    for index in remove {
        let entry = &manifest.copias[index];
        if let Some(file) = owned_file(&folder, entry) {
            if std::fs::remove_file(&file).is_ok() {
                gone.push(index);
                report.removed.push(entry.arquivo.clone());
            }
        }
    }
    if !gone.is_empty() {
        let mut index = 0;
        manifest.copias.retain(|_| {
            let keep = !gone.contains(&index);
            index += 1;
            keep
        });
        if let Err(error) = write_manifest(&manifest_path, &manifest) {
            report.warning = Some(format!(
                "As cópias antigas foram apagadas, mas o registro não pôde ser atualizado ({error:#})."
            ));
        }
    }
    Ok(report)
}

/// The file of a recorded copy, only when it is one this feature may delete:
/// a plain name (no folder in it) of the automatic family, ending in
/// `.sqlite3`, of a plain file (not a link, not a folder) inside `folder`.
fn owned_file(folder: &Path, entry: &Entry) -> Option<PathBuf> {
    let name = entry.arquivo.as_str();
    let plain = !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != "."
        && name != ".."
        && name.starts_with(PREFIX)
        && name.ends_with(".sqlite3");
    if !plain || !entry.verificada {
        return None;
    }
    let path = folder.join(name);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    (meta.is_file() && path.parent() == Some(folder)).then_some(path)
}

/// Which recorded copies retention removes (indexes into `entries`), with
/// today's date. Only copies that are verified, whose moment can be read and
/// that `present` says are there take part: the [`KEEP_RECENT`] most recent
/// are kept, and the oldest one of each of the last [`ANCHOR_WEEKS`] weeks
/// (Monday to Sunday, this week included). Everything else that takes part is
/// removed; what does not take part is never removed.
pub fn plan_retention(
    entries: &[Entry],
    today: NaiveDate,
    present: impl Fn(&Entry) -> bool,
) -> Vec<usize> {
    let mut candidates: Vec<(usize, NaiveDateTime, u64)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.verificada && present(e))
        .filter_map(|(i, e)| Some((i, e.moment()?, e.sequencia)))
        .collect();
    // Newest first.
    candidates.sort_by_key(|c| std::cmp::Reverse((c.1, c.2)));
    let mut keep: std::collections::BTreeSet<usize> = candidates
        .iter()
        .take(KEEP_RECENT)
        .map(|(i, _, _)| *i)
        .collect();
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

/// The run of the day as the window does it: when it is due, the copy, then
/// the state with its result. `Ok(None)`: nothing was due (off, already made
/// today, or another window is making it).
pub fn run_and_record(
    database: &Path,
    data_dir: &Path,
    now: NaiveDateTime,
) -> Result<Option<RunReport>> {
    let state = load_state(data_dir);
    if !due(&state, now.date()) {
        return Ok(None);
    }
    let outcome = run(database, data_dir, now);
    if matches!(&outcome, Ok(report) if report.busy) {
        return Ok(None);
    }
    let mut state = load_state(data_dir);
    let moment = now.format("%Y-%m-%dT%H:%M:%S").to_string();
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
    // The result is told to the person in any case; a state that cannot be
    // written only means the copy is tried again at the next opening.
    let _ = save_state(data_dir, &state);
    outcome.map(Some)
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
    /// Automatic copies recorded and present.
    pub count: usize,
    pub bytes: u64,
    /// Moment of the most recent of them.
    pub newest: Option<String>,
    pub state: State,
}

pub fn summary(data_dir: &Path) -> Summary {
    let folder = folder(data_dir);
    let mut summary = Summary {
        state: load_state(data_dir),
        ..Default::default()
    };
    if let Ok(manifest) = read_manifest(&folder.join(MANIFEST)) {
        for entry in &manifest.copias {
            if let Some(file) = owned_file(&folder, entry) {
                summary.count += 1;
                summary.bytes += std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                if summary.newest.as_deref() < Some(entry.criada_em.as_str()) {
                    summary.newest = Some(entry.criada_em.clone());
                }
            }
        }
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
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE notas (id INTEGER PRIMARY KEY, texto TEXT);")
            .unwrap();
        for n in 0..rows {
            conn.execute("INSERT INTO notas (texto) VALUES (?1)", [format!("nota {n}")])
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

    fn entry(name: &str, seq: u64, moment: &str) -> Entry {
        Entry {
            arquivo: name.into(),
            sequencia: seq,
            criada_em: moment.into(),
            bytes: 1,
            verificada: true,
            origem: "teste".into(),
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
                    &format!("automatica-{}.sqlite3", day.format("%Y%m%d")),
                    100 - back as u64,
                    &format!("{}T09:00:00", day.format("%Y-%m-%d")),
                )
            })
            .collect();
        let removed = plan_retention(&entries, today, |_| true);
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
    fn retention_never_takes_what_is_unverified_unreadable_or_absent() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let mut entries: Vec<Entry> = (0..20)
            .map(|n| entry(&format!("automatica-{n}.sqlite3"), n, "2026-01-01T09:00:00"))
            .collect();
        entries[0].verificada = false;
        entries[1].criada_em = "ontem".into();
        let removed = plan_retention(&entries, today, |e| e.arquivo != "automatica-2.sqlite3");
        assert!(!removed.contains(&0) && !removed.contains(&1) && !removed.contains(&2));
        // 17 take part, 7 recent kept, nothing in the last four weeks.
        assert_eq!(removed.len(), 10);
    }

    #[test]
    fn a_daily_copy_is_verified_recorded_and_old_ones_go_only_from_the_record() {
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
        // A record that lists a file outside the folder and one by a wrong name.
        let outside = data.path().join("fora.sqlite3");
        std::fs::write(&outside, b"fora").unwrap();
        let mut manifest = Manifest {
            versao: 1,
            proxima_sequencia: 1,
            copias: vec![
                entry("..\\fora.sqlite3", 1, "2020-01-01T00:00:00"),
                entry("minha-copia.sqlite3", 2, "2020-01-01T00:00:00"),
            ],
        };
        write_manifest(&folder.join(MANIFEST), &manifest).unwrap();
        // Forty days of copies.
        let first = at("2026-10-14 09:00");
        for day in 0..40 {
            let report = run(&db, data.path(), first + chrono::Duration::days(day)).unwrap();
            let copy = report.copy.unwrap();
            assert_eq!(
                Connection::open(&copy)
                    .unwrap()
                    .query_row("SELECT count(*) FROM notas", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                5
            );
        }
        manifest = read_manifest(&folder.join(MANIFEST)).unwrap();
        let ours: Vec<&Entry> = manifest
            .copias
            .iter()
            .filter(|e| e.arquivo.starts_with("automatica-2026"))
            .collect();
        // 7 recent + anchors of the 3 earlier weeks (this week's is among the 7).
        assert_eq!(ours.len(), 10, "{ours:?}");
        for e in &ours {
            assert!(folder.join(&e.arquivo).is_file());
        }
        // Nothing that is not ours was touched, even when the record listed it.
        assert!(foreign.is_file() && outside.is_file());
        assert!(folder.join("minha-copia.sqlite3").is_file());
        assert!(backups.join("ponte-20200101-000000-a.sqlite3").is_file());
        assert!(backups
            .join("antes-de-instalar-v0.1.0-20200101-000000.sqlite3")
            .is_file());
        // Sequences grow and are never reused.
        let seqs: Vec<u64> = manifest.copias.iter().map(|e| e.sequencia).collect();
        let mut sorted = seqs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), seqs.len());
        let summary = summary(data.path());
        assert_eq!(summary.count, 10);
        assert!(summary.bytes > 0);
    }

    #[test]
    fn a_failed_copy_removes_nothing_and_is_recorded_as_a_failure() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        for day in 0..12 {
            run(&db, data.path(), at("2026-11-01 08:00") + chrono::Duration::days(day)).unwrap();
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
        assert!(due(&load_state(data.path()), NaiveDate::from_ymd_opt(2026, 12, 30).unwrap()));
    }

    #[test]
    fn an_unreadable_record_keeps_every_old_copy() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        for day in 0..10 {
            run(&db, data.path(), at("2026-11-01 08:00") + chrono::Duration::days(day)).unwrap();
        }
        let folder = folder(data.path());
        std::fs::write(folder.join(MANIFEST), b"{ estragado").unwrap();
        let copies_before = names(&folder)
            .into_iter()
            .filter(|n| n.starts_with(PREFIX))
            .count();
        let report = run(&db, data.path(), at("2026-11-20 08:00")).unwrap();
        assert!(report.copy.is_some() && report.removed.is_empty());
        assert!(report.warning.is_some());
        let copies_after = names(&folder)
            .into_iter()
            .filter(|n| n.starts_with(PREFIX))
            .count();
        assert_eq!(copies_after, copies_before + 1);
        assert!(names(&folder).iter().any(|n| n.starts_with("manifesto-ilegivel-")));
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
    fn a_second_run_while_one_is_making_the_copy_does_nothing() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 1);
        let folder = folder(data.path());
        std::fs::create_dir_all(&folder).unwrap();
        let held = lock(&folder).unwrap().unwrap();
        let report = run(&db, data.path(), at("2026-11-22 08:00")).unwrap();
        assert!(report.busy && report.copy.is_none());
        drop(held);
        assert!(run(&db, data.path(), at("2026-11-22 08:00")).unwrap().copy.is_some());
    }

    #[test]
    fn the_manual_copy_is_verified_and_keeps_its_old_name() {
        let data = tempfile::tempdir().unwrap();
        let db = data.path().join("acervo.sqlite3");
        library(&db, 3);
        let copy = manual_copy(&db, data.path()).unwrap();
        let name = copy.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("ponte-") && name.ends_with(".sqlite3"), "{name}");
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
