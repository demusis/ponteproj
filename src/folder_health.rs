//! What the person should know about the shared-folder file besides the
//! exchange itself, without any change of what is written to it:
//!
//! * how big the file is and how much room is left before the 512 MB limit
//!   (and the warning, from 70%, that says what to do);
//! * the files beside it that look like copies made by the cloud client when
//!   two devices wrote at once (`ponte-sync (1).ponte`, "cópia em conflito",
//!   OneDrive's `ponte-sync-COMPUTADOR.ponte`), which may hold changes of
//!   another device;
//! * the `-divergente-` copies the Ponte keeps aside, once they are old
//!   enough for the person to decide to delete them (never deleted by
//!   itself);
//! * what never travels between devices.
//!
//! The decisions are pure functions of names, sizes and ages; the few
//! functions that touch the disk only list a folder, read the first bytes of a
//! file or delete what the person confirmed.
use crate::sharedfolder::{MAX_FILE_BYTES, SYNC_MAGIC};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// From this share of the limit the person is warned.
pub const WARN_PERCENT: u64 = 70;
/// A `-divergente-` copy older than this is offered for cleaning.
pub const OLD_ARCHIVE_DAYS: u64 = 90;
/// The most entries of a folder that are looked at (a folder of the cloud
/// client may hold thousands of unrelated files).
const MAX_ENTRIES: usize = 5000;

/// What to do when the file nears or reaches the limit: the same words in the
/// warning and in the failure.
pub const ADVICE: &str = "Para liberar espaço, esvazie a Lixeira (Ajustes ▸ Lixeira) e remova os anexos grandes que não precisam ir para os outros aparelhos.";

/// What does not travel between the devices of the library, verified against
/// what the file carries (`CabinetSnapshot`): each of these stays where it was
/// set.
pub const NOT_SYNCED: &str = "Não vão para os outros aparelhos: a chave e as configurações da IA, as contas de e-mail, os lembretes e avisos deste aparelho, as conferências de minuta, a permissão de enviar documentos à IA, o prazo de guarda da Lixeira, a aparência e os filtros guardados.";

/// A size as the person reads it: `512 MB`, `12,3 MB`, `340 KB`.
pub fn format_size(bytes: u64) -> String {
    let value = bytes as f64;
    if bytes >= 1024 * 1024 {
        let mb = value / (1024.0 * 1024.0);
        if mb >= 100.0 || (mb - mb.round()).abs() < 0.05 {
            format!("{:.0} MB", mb)
        } else {
            format!("{mb:.1} MB").replace('.', ",")
        }
    } else if bytes >= 1024 {
        format!("{:.0} KB", value / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

/// The limit as it is written in the messages.
fn limit_text() -> String {
    format_size(MAX_FILE_BYTES)
}

/// Whole percent of the limit used (never above 100).
pub fn percent_used(size: u64) -> u64 {
    (size.saturating_mul(100) / MAX_FILE_BYTES).min(100)
}

/// Bytes left before the limit.
pub fn remaining(size: u64) -> u64 {
    MAX_FILE_BYTES.saturating_sub(size)
}

/// The line under the file: how big it is and how much room is left.
pub fn size_line(size: u64) -> String {
    let left = remaining(size);
    if left == 0 {
        format!(
            "Tamanho do arquivo: {} de {} (100%). Sem espaço restante.",
            format_size(size),
            limit_text()
        )
    } else {
        format!(
            "Tamanho do arquivo: {} de {} ({}%). Cabem mais {}.",
            format_size(size),
            limit_text(),
            percent_used(size),
            format_size(left)
        )
    }
}

/// The warning from 70% of the limit, with what to do; at the limit it says
/// that the exchange cannot write any more. `None` below 70%.
pub fn size_warning(size: u64) -> Option<String> {
    if size.saturating_mul(100) < MAX_FILE_BYTES.saturating_mul(WARN_PERCENT) {
        return None;
    }
    let state = if size >= MAX_FILE_BYTES {
        "A sincronização não consegue mais gravar nele."
    } else {
        "Perto do limite, a sincronização deixa de conseguir gravar."
    };
    Some(format!(
        "O arquivo da pasta está com {} de {} ({}%). {state} {ADVICE}",
        format_size(size),
        limit_text(),
        percent_used(size)
    ))
}

/// The failure when the library does not fit: nothing was written. `size` is
/// what it would take, when known.
pub fn too_big_message(size: Option<u64>) -> String {
    let would = size
        .map(|s| format!(" (daria {})", format_size(s)))
        .unwrap_or_default();
    format!(
        "O acervo passa de {} e não cabe no arquivo de sincronização{would}. Nada foi gravado. {ADVICE}",
        limit_text()
    )
}

/// A lower bound of what the library takes in the file, from what the
/// snapshot already says about its PDFs and attachments (their size), before
/// any of them is read. A library whose bound already passes the limit cannot
/// be written, so it is refused without reading the files.
pub fn blob_bound(snapshot: &crate::intranet::CabinetSnapshot) -> u64 {
    // Each blob also carries its number and length in the file.
    const FRAME: u64 = 16;
    let pdfs: u64 = snapshot
        .files_meta
        .iter()
        .map(|m| m.summary.size as u64 + FRAME)
        .sum();
    let attachments: u64 = snapshot
        .attachments
        .iter()
        .map(|a| a.meta.size as u64 + FRAME)
        .sum();
    pdfs.saturating_add(attachments)
}

// ------------------------------------------------------------ names

/// How a file beside the shared file is classified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sibling {
    /// A copy that the cloud client (or someone) made of the file, and that
    /// may hold changes of another device.
    ConflictCopy,
    /// A copy the Ponte kept aside before rewriting the folder.
    Divergent,
}

const CONFLICT_WORDS: [&str; 4] = [
    "cópia em conflito",
    "copia em conflito",
    "conflicted copy",
    "copia en conflicto",
];

/// The names the Ponte itself gives to files beside a library or a copy: never
/// taken for a conflict copy.
const OWN_MARKS: [&str; 3] = ["-divergente-", "-guarda", "-pendente"];

/// Splits `name` into stem and extension, both lowercase; `None` without one.
fn stem_and_ext(name: &str) -> Option<(String, String)> {
    let lower = name.to_lowercase();
    let dot = lower.rfind('.')?;
    Some((lower[..dot].to_owned(), lower[dot + 1..].to_owned()))
}

/// True for `" (N)"`, N being digits only.
fn numbered(rest: &str) -> bool {
    let Some(inner) = rest.strip_prefix(" (").and_then(|r| r.strip_suffix(')')) else {
        return false;
    };
    !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit())
}

/// OneDrive names the copy of a file two computers wrote at once after the
/// computer: `ponte-sync-DESKTOP-ABC123.ponte`. Only a suffix made of capitals,
/// digits, `-` and `_` with at least one letter counts (the way Windows writes
/// a computer name), so a name the person gave (`ponte-sync-antigo.ponte`) is
/// left alone.
fn computer_suffix(rest: &str, original_rest: &str) -> bool {
    let Some(name) = rest.strip_prefix('-') else {
        return false;
    };
    let original = &original_rest[1..];
    !name.is_empty()
        && name.len() == original.len()
        && original
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && original.chars().any(|c| c.is_ascii_uppercase())
        && !original.starts_with('-')
        && !original.ends_with('-')
}

/// What `candidate` is, a file beside `real` (both bare file names), or
/// `None` when it is neither. Conservative: the file itself, the Ponte's own
/// auxiliary files and anything that is not a `.ponte` of the same name are
/// never matched.
pub fn classify(real: &str, candidate: &str) -> Option<Sibling> {
    let (real_stem, real_ext) = stem_and_ext(real)?;
    let (stem, ext) = stem_and_ext(candidate)?;
    if ext != real_ext || candidate.eq_ignore_ascii_case(real) || candidate.starts_with('.') {
        return None;
    }
    // The original case of the part after the stem, for the computer name.
    let original_stem = &candidate[..candidate.len() - ext.len() - 1];
    // "Cópia de ponte-sync.ponte" / "Copy of ponte-sync.ponte": a duplicate
    // made by hand in the cloud's own page.
    for prefix in ["cópia de ", "copia de ", "copy of "] {
        if stem
            .strip_prefix(prefix)
            .is_some_and(|rest| rest == real_stem)
        {
            return Some(Sibling::ConflictCopy);
        }
    }
    let rest = stem.strip_prefix(real_stem.as_str())?;
    if rest.is_empty() {
        return None;
    }
    let original_rest = original_stem.get(real_stem.len()..)?;
    if OWN_MARKS.iter().any(|mark| rest.starts_with(mark)) {
        return if is_divergent_rest(rest) {
            Some(Sibling::Divergent)
        } else {
            None
        };
    }
    if rest.contains(".tmp-") {
        return None;
    }
    if numbered(rest)
        || CONFLICT_WORDS.iter().any(|word| rest.contains(word))
        || computer_suffix(rest, original_rest)
    {
        return Some(Sibling::ConflictCopy);
    }
    None
}

/// `-divergente-` followed by the 16 hexadecimal digits the Ponte writes.
fn is_divergent_rest(rest: &str) -> bool {
    rest.strip_prefix("-divergente-")
        .is_some_and(|code| code.len() == 16 && code.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Whether a file this old is offered for cleaning.
pub fn is_old(age: Duration) -> bool {
    age > Duration::from_secs(OLD_ARCHIVE_DAYS * 24 * 60 * 60)
}

// ------------------------------------------------------------ disk

/// A file found beside the shared file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    /// Whole days since the file was last written or made here (the later of
    /// the two), when the system says.
    pub age_days: Option<u64>,
    /// When the file is a Ponte file whose header could be read: what it says
    /// of itself (`revisão N • gravada por X em D`).
    pub description: Option<String>,
}

/// What is beside one shared file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderScan {
    /// The shared file, as configured.
    pub path: PathBuf,
    /// Its size, when it exists.
    pub size: Option<u64>,
    /// Files that look like copies made by the cloud client.
    pub conflict_copies: Vec<Found>,
    /// `-divergente-` copies older than [`OLD_ARCHIVE_DAYS`].
    pub old_archives: Vec<Found>,
}

/// True when the file starts like a transport file of the Ponte (a name
/// alone never makes a copy: whatever else is called that is not listed).
fn starts_like_sync_file(path: &Path) -> bool {
    let mut magic = vec![0_u8; SYNC_MAGIC.len()];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok()
        && magic == SYNC_MAGIC
}

fn age_of(metadata: &std::fs::Metadata, now: SystemTime) -> Option<Duration> {
    let modified = metadata.modified().ok();
    let created = metadata.created().ok();
    // Copying a file keeps its modification moment: the later of the two is
    // when this copy was made.
    let newest = match (modified, created) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }?;
    Some(now.duration_since(newest).unwrap_or_default())
}

/// Lists the folder of `real` and tells what is beside it. A folder that
/// cannot be read gives an empty scan: the exchange reports that on its own.
pub fn scan(real: &Path, now: SystemTime) -> FolderScan {
    let mut out = FolderScan {
        path: real.to_path_buf(),
        size: std::fs::metadata(real)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len()),
        ..FolderScan::default()
    };
    let (Some(real_name), Some(folder)) = (
        real.file_name().and_then(|n| n.to_str()),
        real.parent().filter(|p| !p.as_os_str().is_empty()),
    ) else {
        return out;
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return out;
    };
    for entry in entries.flatten().take(MAX_ENTRIES) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(kind) = classify(real_name, &name) else {
            continue;
        };
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() || !starts_like_sync_file(&path) {
            continue;
        }
        let age = age_of(&metadata, now);
        let found = Found {
            description: crate::sharedfolder::read_header(&path)
                .ok()
                .flatten()
                .map(|header| crate::sharedfolder::describe(&header)),
            size: metadata.len(),
            age_days: age.map(|a| a.as_secs() / (24 * 60 * 60)),
            name,
            path,
        };
        match kind {
            Sibling::ConflictCopy => out.conflict_copies.push(found),
            Sibling::Divergent if age.is_some_and(is_old) => out.old_archives.push(found),
            Sibling::Divergent => {}
        }
    }
    out.conflict_copies.sort_by(|a, b| a.name.cmp(&b.name));
    out.old_archives.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// What the warning about copies of the file says. `None` when there are none.
pub fn conflict_warning(scan: &FolderScan) -> Option<String> {
    if scan.conflict_copies.is_empty() {
        return None;
    }
    let names: Vec<&str> = scan
        .conflict_copies
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    let (what, verb) = if names.len() == 1 {
        ("Há um arquivo", "parece uma cópia")
    } else {
        ("Há arquivos", "parecem cópias")
    };
    Some(format!(
        "{what} ao lado de {} que {verb} feita pela nuvem quando dois aparelhos gravaram ao mesmo tempo: {}. O Ponte não os usa, e eles podem ter alterações de outro aparelho que não chegaram ao acervo. Não apague agora: sincronize também nos outros aparelhos (cada um recupera o que gravou) e confira se nada faltou no acervo. Guarde a cópia até ter certeza, como a cópia “-divergente-” (README, seção 8.5); depois pode apagá-la.",
        scan.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("o arquivo da pasta"),
        names.join(", ")
    ))
}

/// What the confirmation to clean old `-divergente-` copies says; it names
/// every file.
pub fn archives_question(found: &[Found]) -> String {
    let names: Vec<String> = found
        .iter()
        .map(|f| match f.age_days {
            Some(days) => format!("{} ({}, {days} dias)", f.name, format_size(f.size)),
            None => format!("{} ({})", f.name, format_size(f.size)),
        })
        .collect();
    format!(
        "Apagar {}: {}? São cópias de segurança de um arquivo que divergiu e têm mais de {OLD_ARCHIVE_DAYS} dias. Só apague se já conferiu que nada faltou no acervo. Elas não vão para a Lixeira do Windows e não podem ser trazidas de volta.",
        if found.len() == 1 {
            "esta cópia antiga"
        } else {
            "estas cópias antigas"
        },
        names.join("; ")
    )
}

/// Deletes the old `-divergente-` copies the person confirmed. Each one is
/// checked again first (still a `-divergente-` copy of `real`, still a Ponte
/// file, still old): anything else is left alone. Returns what was deleted
/// and what could not be.
pub fn delete_old_archives(
    real: &Path,
    chosen: &[PathBuf],
    now: SystemTime,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    let Some(real_name) = real.file_name().and_then(|n| n.to_str()) else {
        return (deleted, failed);
    };
    for path in chosen {
        let same_folder = path.parent() == real.parent();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let still_old = std::fs::metadata(path)
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| age_of(&m, now))
            .is_some_and(is_old);
        if !same_folder
            || classify(real_name, name) != Some(Sibling::Divergent)
            || !still_old
            || !starts_like_sync_file(path)
        {
            continue;
        }
        match std::fs::remove_file(path) {
            Ok(()) => deleted.push(path.clone()),
            Err(error) => failed.push(format!("{}: {error}", path.display())),
        }
    }
    (deleted, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "ponte-sync.ponte";

    #[test]
    fn sizes_read_in_megabytes_with_a_comma() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(340 * 1024), "340 KB");
        assert_eq!(format_size(12 * 1024 * 1024 + 300_000), "12,3 MB");
        assert_eq!(format_size(512 * 1024 * 1024), "512 MB");
        assert_eq!(format_size(100 * 1024 * 1024 + 400_000), "100 MB");
        assert_eq!(format_size(3 * 1024 * 1024), "3 MB");
    }

    #[test]
    fn the_size_line_says_the_room_left_and_the_warning_starts_at_70_percent() {
        let mb = 1024 * 1024;
        assert_eq!(
            size_line(120 * mb),
            "Tamanho do arquivo: 120 MB de 512 MB (23%). Cabem mais 392 MB."
        );
        assert_eq!(percent_used(MAX_FILE_BYTES * 2), 100);
        assert_eq!(remaining(MAX_FILE_BYTES + 1), 0);
        assert!(size_line(MAX_FILE_BYTES).contains("Sem espaço restante"));
        // 70% of 512 MB is 358,4 MB.
        assert!(size_warning(358 * mb).is_none());
        let warning = size_warning(359 * mb).unwrap();
        assert!(warning.starts_with("O arquivo da pasta está com 359 MB de 512 MB (70%)."));
        assert!(warning.contains("esvazie a Lixeira") && warning.contains("anexos grandes"));
        let full = size_warning(MAX_FILE_BYTES).unwrap();
        assert!(full.contains("não consegue mais gravar") && full.contains("esvazie a Lixeira"));
    }

    #[test]
    fn the_failure_carries_the_same_advice() {
        let message = too_big_message(Some(600 * 1024 * 1024));
        assert!(
            message.starts_with("O acervo passa de 512 MB e não cabe no arquivo de sincronização")
        );
        assert!(message.contains("daria 600 MB") && message.contains("Nada foi gravado"));
        assert!(message.contains("esvazie a Lixeira") && message.contains("anexos grandes"));
        assert!(!too_big_message(None).contains("daria"));
    }

    #[test]
    fn cloud_conflict_copies_are_recognised() {
        for name in [
            "ponte-sync (1).ponte",
            "ponte-sync (2).ponte",
            "ponte-sync (12).ponte",
            "Ponte-Sync (1).PONTE",
            "ponte-sync (cópia em conflito de Maria 2026-11-20).ponte",
            "ponte-sync (Copia em conflito).ponte",
            "ponte-sync (conflicted copy 2026-11-20).ponte",
            "ponte-sync - conflicted copy.ponte",
            "ponte-sync-DESKTOP-ABC123.ponte",
            "ponte-sync-LAPTOP_2.ponte",
            "Cópia de ponte-sync.ponte",
            "Copy of ponte-sync.ponte",
        ] {
            assert_eq!(
                classify(REAL, name),
                Some(Sibling::ConflictCopy),
                "{name} should be a conflict copy"
            );
        }
    }

    #[test]
    fn nothing_else_is_taken_for_a_conflict_copy() {
        for name in [
            // The file itself, in any case.
            "ponte-sync.ponte",
            "PONTE-SYNC.PONTE",
            // The Ponte's own files.
            "ponte-sync-guarda.ponte",
            "ponte-sync-pendente.ponte",
            "ponte-sync-divergente-curto.ponte",
            // Another extension, another name, a temporary file.
            "ponte-sync (1).txt",
            "ponte-sync (1)",
            "ponte-sync.ponte.tmp-1234",
            ".ponte-sync.ponte.tmp-abcd",
            "outro.ponte",
            "ponte-sync-2.ponte",
            "ponte-sync (a).ponte",
            "ponte-sync ().ponte",
            // A name the person gave is not a computer name.
            "ponte-sync-antigo.ponte",
            "ponte-sync-Backup.ponte",
            "ponte-sync-2026.ponte",
            "ponte-sync--ABC.ponte",
            "meu ponte-sync (1).ponte",
            "ponte-syncx (1).ponte",
        ] {
            assert_eq!(classify(REAL, name), None, "{name} must not match");
        }
        // A copy the Ponte kept aside is never a conflict copy.
        assert_ne!(
            classify(REAL, "ponte-sync-divergente-a1b2c3d4e5f60789.ponte"),
            Some(Sibling::ConflictCopy)
        );
    }

    #[test]
    fn the_divergent_copies_the_ponte_keeps_are_told_apart() {
        assert_eq!(
            classify(REAL, "ponte-sync-divergente-a1b2c3d4e5f60789.ponte"),
            Some(Sibling::Divergent)
        );
        assert_eq!(
            classify(REAL, "Ponte-Sync-Divergente-A1B2C3D4E5F60789.ponte"),
            Some(Sibling::Divergent)
        );
        // Of another file: not ours to list.
        assert_eq!(
            classify(REAL, "outro-divergente-a1b2c3d4e5f60789.ponte"),
            None
        );
        // Not a copy the Ponte wrote (the code is 16 hexadecimal digits).
        assert_eq!(classify(REAL, "ponte-sync-divergente-xyz.ponte"), None);
    }

    #[test]
    fn only_a_copy_older_than_90_days_is_old() {
        let day = Duration::from_secs(24 * 60 * 60);
        assert!(!is_old(day * 90));
        assert!(is_old(day * 90 + Duration::from_secs(1)));
        assert!(!is_old(Duration::ZERO));
    }

    fn sync_file(path: &Path) {
        let mut bytes = SYNC_MAGIC.to_vec();
        bytes.extend_from_slice(b"{}\nresto");
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn the_scan_lists_copies_that_are_ponte_files_and_leaves_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        sync_file(&real);
        sync_file(&dir.path().join("ponte-sync (1).ponte"));
        sync_file(&dir.path().join("ponte-sync-DESKTOP-ABC123.ponte"));
        // The name matches, the content is not a Ponte file.
        std::fs::write(dir.path().join("ponte-sync (2).ponte"), b"outra coisa").unwrap();
        // New divergent copy: not offered yet.
        sync_file(
            &dir.path()
                .join("ponte-sync-divergente-a1b2c3d4e5f60789.ponte"),
        );
        std::fs::write(dir.path().join("ponte-sync-guarda.ponte"), b"x").unwrap();
        let found = scan(&real, SystemTime::now());
        assert_eq!(found.size, Some(SYNC_MAGIC.len() as u64 + 8));
        let names: Vec<&str> = found
            .conflict_copies
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["ponte-sync (1).ponte", "ponte-sync-DESKTOP-ABC123.ponte"]
        );
        assert!(found.old_archives.is_empty());
        let warning = conflict_warning(&found).unwrap();
        assert!(warning.contains("ponte-sync (1).ponte") && warning.contains("DESKTOP-ABC123"));
        assert!(warning.contains("alterações de outro aparelho"));
        assert!(conflict_warning(&FolderScan::default()).is_none());
        // Everything is still there: listing deletes nothing.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 6);
    }

    #[test]
    fn a_divergent_copy_is_old_only_by_the_clock_given() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        sync_file(&real);
        let old = dir
            .path()
            .join("ponte-sync-divergente-a1b2c3d4e5f60789.ponte");
        sync_file(&old);
        // Seen 91 days later it is old; seen now it is not.
        let later = SystemTime::now() + Duration::from_secs(91 * 24 * 60 * 60);
        let found = scan(&real, later);
        assert_eq!(found.old_archives.len(), 1);
        assert_eq!(found.old_archives[0].path, old);
        assert!(found.old_archives[0].age_days.unwrap() >= 90);
        assert!(scan(&real, SystemTime::now()).old_archives.is_empty());
        let question = archives_question(&found.old_archives);
        assert!(question.contains("ponte-sync-divergente-a1b2c3d4e5f60789.ponte"));
        assert!(question.contains("não vão para a Lixeira"));
    }

    #[test]
    fn deleting_asks_again_of_each_file_and_touches_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        sync_file(&real);
        let old = dir
            .path()
            .join("ponte-sync-divergente-a1b2c3d4e5f60789.ponte");
        sync_file(&old);
        let copy = dir.path().join("ponte-sync (1).ponte");
        sync_file(&copy);
        let other = tempfile::tempdir().unwrap();
        let elsewhere = other
            .path()
            .join("ponte-sync-divergente-0123456789abcdef.ponte");
        sync_file(&elsewhere);
        let later = SystemTime::now() + Duration::from_secs(91 * 24 * 60 * 60);
        // The real file, a conflict copy and a file of another folder are not
        // deleted, however they were asked for.
        let (deleted, failed) = delete_old_archives(
            &real,
            &[real.clone(), copy.clone(), elsewhere.clone()],
            later,
        );
        assert!(deleted.is_empty() && failed.is_empty());
        assert!(real.exists() && copy.exists() && elsewhere.exists());
        // Not old yet: kept.
        let (deleted, _) =
            delete_old_archives(&real, std::slice::from_ref(&old), SystemTime::now());
        assert!(deleted.is_empty() && old.exists());
        // Old: deleted, and only that one.
        let (deleted, failed) = delete_old_archives(&real, std::slice::from_ref(&old), later);
        assert_eq!(deleted, std::slice::from_ref(&old));
        assert!(failed.is_empty() && !old.exists());
        assert!(real.exists() && copy.exists());
    }

    #[test]
    fn what_does_not_travel_is_listed() {
        for word in [
            "chave",
            "e-mail",
            "lembretes",
            "conferências de minuta",
            "aparência",
            "filtros guardados",
        ] {
            assert!(NOT_SYNCED.contains(word), "{word}");
        }
    }
}
