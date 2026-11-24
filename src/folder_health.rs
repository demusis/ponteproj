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
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// From this share of the limit the person is warned.
pub const WARN_PERCENT: u64 = 70;
/// A `-divergente-` copy older than this is offered for cleaning.
pub const OLD_ARCHIVE_DAYS: u64 = 90;
/// What is beside the shared files is listed again at most this often (and
/// at once when the folders change or an exchange ends).
pub const SCAN_INTERVAL: Duration = Duration::from_secs(60);
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
/// that what is added may not fit (a smaller library still can be written).
/// `None` below 70%.
pub fn size_warning(size: u64) -> Option<String> {
    if size.saturating_mul(100) < MAX_FILE_BYTES.saturating_mul(WARN_PERCENT) {
        return None;
    }
    let state = if size >= MAX_FILE_BYTES {
        "Ele chegou ao limite: o que for acrescentado talvez não caiba."
    } else {
        "Perto do limite, o que for acrescentado pode não caber."
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

/// When a scan of the folders is due. `listed` is the age of the last result
/// and whether it was made for the folders now configured. At most one scan
/// runs at a time; otherwise one is due when there is no result, the folders
/// changed, something asked for a new one (an exchange ended, a deletion) or
/// the result is [`SCAN_INTERVAL`] old.
pub fn scan_due(listed: Option<(Duration, bool)>, in_flight: bool, invalidated: bool) -> bool {
    if in_flight {
        return false;
    }
    match listed {
        None => true,
        Some((age, same_folders)) => invalidated || !same_folders || age >= SCAN_INTERVAL,
    }
}

/// What identifies a file as the one that was listed: its size, its moment of
/// modification and a SHA-256 of its header (the plain first lines of a
/// transport file). Checked again, through the handle that deletes it, just
/// before it is deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub size: u64,
    pub modified_ns: u128,
    pub header_sha256: String,
}

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
    /// What the file was when it was listed (see [`Identity`]).
    pub identity: Option<Identity>,
}

/// A file named like a copy the Ponte keeps aside, old enough to be offered,
/// that the checks do not confirm as one: shown, never offered for deletion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unrecognised {
    pub name: String,
    pub reason: &'static str,
}

const NOT_PONTE: &str = "não é um arquivo de sincronização do Ponte";
const IN_USE: &str = "é um arquivo de sincronização em uso neste computador";
const OTHER_LIBRARY: &str = "é de outro acervo";
const NO_REFERENCE: &str = "o arquivo da pasta não pôde ser lido para comparar o acervo";
const CODE_MISMATCH: &str = "o conteúdo não confere com o código do nome";
const TOO_LARGE: &str = "é grande demais para ser um arquivo de sincronização";
const UNREADABLE: &str = "não pôde ser lido";

/// What is beside one shared file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderScan {
    /// The shared file, as configured.
    pub path: PathBuf,
    /// Its size, when it exists.
    pub size: Option<u64>,
    /// Files that look like copies made by the cloud client.
    pub conflict_copies: Vec<Found>,
    /// `-divergente-` copies older than [`OLD_ARCHIVE_DAYS`] that every check
    /// confirms as copies the Ponte kept aside of this library's file.
    pub old_archives: Vec<Found>,
    /// Old files named like those that the checks do not confirm.
    pub unrecognised: Vec<Unrecognised>,
}

/// What was already learned of a file's content (path, size and moment of
/// modification, then whether the SHA-256 of the whole file matches the code
/// in its name), so that a scan every minute does not read a large file
/// again and again.
#[derive(Default)]
pub struct HashMemo(Mutex<HashMap<(PathBuf, u64, u128), bool>>);

/// The first bytes of a transport file: the magic, the header line and its
/// newline. `None` when the file does not start like one.
fn read_head(file: &mut File) -> Option<(crate::sharedfolder::Header, String)> {
    const MAX_HEAD: u64 = 64 * 1024 + 64;
    let mut bytes = Vec::new();
    file.take(MAX_HEAD).read_to_end(&mut bytes).ok()?;
    if !bytes.starts_with(SYNC_MAGIC) {
        return None;
    }
    let rest = &bytes[SYNC_MAGIC.len()..];
    let line = rest.iter().position(|b| *b == b'\n')?;
    let header = serde_json::from_slice(&rest[..line]).ok()?;
    let head = &bytes[..SYNC_MAGIC.len() + line + 1];
    Some((header, sha256_hex(head)))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn modified_ns(metadata: &std::fs::Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_nanos())
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

/// The 16 hexadecimal digits of the name of a copy kept aside
/// (`<nome>-divergente-<código>.ponte`): the start of the SHA-256 of its
/// content, written by `sharedfolder::keep_aside`.
fn aside_code(name: &str) -> Option<String> {
    let (stem, _) = stem_and_ext(name)?;
    let at = stem.rfind("-divergente-")?;
    let code = &stem[at + "-divergente-".len()..];
    (code.len() == 16 && code.chars().all(|c| c.is_ascii_hexdigit())).then(|| code.to_owned())
}

/// The path as it is compared to tell whether two names reach the same file:
/// the folder resolved (links and short names), the file name as written,
/// all lowercase.
fn normal_path(path: &Path) -> String {
    let resolved = match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) if !folder.as_os_str().is_empty() => {
            std::fs::canonicalize(folder)
                .map(|f| f.join(name))
                .unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    };
    crate::paths::readable(&resolved)
        .to_string_lossy()
        .to_lowercase()
}

/// Every path this computer is configured to use as a shared file or mirror,
/// in the form [`normal_path`] compares.
/// The file of format 9 beside each one (`<nome>-formato9.ponte`) is in use
/// too.
fn configured_set(paths: &[String]) -> BTreeSet<String> {
    paths
        .iter()
        .flat_map(|p| {
            let path = Path::new(p);
            [
                normal_path(path),
                normal_path(&crate::sharedfolder::nine_path(path)),
            ]
        })
        .collect()
}

/// The lineage of the library the shared file at `real` belongs to (read
/// from the file of format 9 beside it when the name holds no file).
fn lineage_of(real: &Path) -> Option<String> {
    [real.to_path_buf(), crate::sharedfolder::nine_path(real)]
        .iter()
        .find_map(|path| crate::sharedfolder::read_header(path).ok().flatten())
        .map(|header| header.lineage)
}

/// The file that holds the library for the configured path `real`: since
/// the format 9, the file `<nome>-formato9.ponte` beside it (the name itself
/// then holds only a small stub that points to it); before it, `real`.
fn live_file(real: &Path) -> PathBuf {
    let nine = crate::sharedfolder::nine_path(real);
    if std::fs::metadata(&nine).is_ok_and(|m| m.is_file()) {
        nine
    } else {
        real.to_path_buf()
    }
}

/// The checks that need nothing from the content: not a path this computer
/// uses, and of the lineage of this library's file. `Err(reason)` when the
/// file must not be offered.
fn provenance_without_content(
    path: &Path,
    configured: &BTreeSet<String>,
    expected_lineage: Option<&str>,
    header_lineage: &str,
) -> Result<(), &'static str> {
    if configured.contains(&normal_path(path)) {
        return Err(IN_USE);
    }
    match expected_lineage {
        None => Err(NO_REFERENCE),
        Some(expected) if expected != header_lineage => Err(OTHER_LIBRARY),
        Some(_) => Ok(()),
    }
}

/// True when the SHA-256 of everything `file` holds starts with `code`.
fn content_matches_code(file: &mut File, code: &str) -> std::io::Result<bool> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    let (_, sha) = crate::auto_backup::identity_of(file)?;
    Ok(sha.starts_with(&code.to_lowercase()))
}

/// Lists the folder of `real` and tells what is beside it. A folder that
/// cannot be read gives an empty scan: the exchange reports that on its own.
/// `configured` is [`configured_set`] of the paths this computer uses.
///
/// This reads the cloud's folder: call it from a worker, never from the
/// thread that draws.
fn scan_one(
    real: &Path,
    now: SystemTime,
    configured: &BTreeSet<String>,
    memo: &HashMemo,
) -> FolderScan {
    let mut out = FolderScan {
        path: real.to_path_buf(),
        // The size of the file that holds the library (the one of format 9
        // once the folder moved to it, never its small stub).
        size: std::fs::metadata(live_file(real))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len()),
        ..FolderScan::default()
    };
    let expected_lineage = lineage_of(real);
    let nine = crate::sharedfolder::nine_path(real);
    let (Some(real_name), Some(nine_name), Some(folder)) = (
        real.file_name().and_then(|n| n.to_str()),
        nine.file_name().and_then(|n| n.to_str()),
        real.parent().filter(|p| !p.as_os_str().is_empty()),
    ) else {
        return out;
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return out;
    };
    for entry in entries.flatten().take(MAX_ENTRIES) {
        let name = entry.file_name().to_string_lossy().into_owned();
        // A copy of the old name or of the file of format 9 beside it; the
        // file of format 9 itself is the library, never a copy.
        if name.eq_ignore_ascii_case(nine_name) {
            continue;
        }
        let Some(kind) = classify(nine_name, &name).or_else(|| classify(real_name, &name)) else {
            continue;
        };
        let path = entry.path();
        // Not followed: a link is not a copy.
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let age = age_of(&metadata, now);
        if kind == Sibling::Divergent && !age.is_some_and(is_old) {
            continue;
        }
        let head = File::open(&path)
            .ok()
            .and_then(|mut file| read_head(&mut file).map(|head| (file, head)));
        let Some((mut file, (header, head_sha))) = head else {
            // A name alone never makes a copy: a conflict copy that is not a
            // Ponte file is not listed; an old file named like a kept copy
            // is shown, as not recognised.
            if kind == Sibling::Divergent {
                out.unrecognised.push(Unrecognised {
                    name,
                    reason: NOT_PONTE,
                });
            }
            continue;
        };
        let found = Found {
            description: Some(crate::sharedfolder::describe(&header)),
            size: metadata.len(),
            age_days: age.map(|a| a.as_secs() / (24 * 60 * 60)),
            identity: modified_ns(&metadata).map(|modified_ns| Identity {
                size: metadata.len(),
                modified_ns,
                header_sha256: head_sha,
            }),
            name: name.clone(),
            path,
        };
        match kind {
            Sibling::ConflictCopy => out.conflict_copies.push(found),
            Sibling::Divergent => {
                let verdict = recognise(
                    &mut file,
                    &found,
                    &header.lineage,
                    configured,
                    expected_lineage.as_deref(),
                    memo,
                );
                match verdict {
                    Ok(()) => out.old_archives.push(found),
                    Err(reason) => out.unrecognised.push(Unrecognised { name, reason }),
                }
            }
        }
    }
    out.conflict_copies.sort_by(|a, b| a.name.cmp(&b.name));
    out.old_archives.sort_by(|a, b| a.name.cmp(&b.name));
    out.unrecognised.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Whether an old file named like a copy kept aside is one: not a path this
/// computer uses, of the lineage of this library's file, and with the
/// content the name says (the code is the start of its SHA-256). A file that
/// is in use is rewritten at each exchange, so its content never matches the
/// code of its name.
fn recognise(
    file: &mut File,
    found: &Found,
    header_lineage: &str,
    configured: &BTreeSet<String>,
    expected_lineage: Option<&str>,
    memo: &HashMemo,
) -> Result<(), &'static str> {
    provenance_without_content(&found.path, configured, expected_lineage, header_lineage)?;
    if found.size > MAX_FILE_BYTES {
        return Err(TOO_LARGE);
    }
    let code = aside_code(&found.name).ok_or(CODE_MISMATCH)?;
    let key = found
        .identity
        .as_ref()
        .map(|i| (found.path.clone(), i.size, i.modified_ns));
    if let Some(known) = key
        .as_ref()
        .and_then(|k| memo.0.lock().ok().and_then(|m| m.get(k).copied()))
    {
        return if known { Ok(()) } else { Err(CODE_MISMATCH) };
    }
    let matches = content_matches_code(file, &code).map_err(|_| UNREADABLE)?;
    if let Some(key) = key {
        if let Ok(mut memo) = memo.0.lock() {
            memo.insert(key, matches);
        }
    }
    if matches {
        Ok(())
    } else {
        Err(CODE_MISMATCH)
    }
}

/// Scans the folder of each configured shared file (see [`scan_one`]).
pub fn scan_all(paths: &[String], now: SystemTime, memo: &HashMemo) -> Vec<FolderScan> {
    let configured = configured_set(paths);
    paths
        .iter()
        .map(|path| scan_one(Path::new(path), now, &configured, memo))
        .collect()
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

/// What is said of the old files named like a kept copy that the checks do
/// not confirm. They are shown and never offered for deletion. `None` when
/// there are none.
pub fn unrecognised_note(scan: &FolderScan) -> Option<String> {
    if scan.unrecognised.is_empty() {
        return None;
    }
    let list: Vec<String> = scan
        .unrecognised
        .iter()
        .map(|u| format!("{} ({})", u.name, u.reason))
        .collect();
    Some(format!(
        "Há {} com nome de cópia “-divergente-” e mais de {OLD_ARCHIVE_DAYS} dias que o Ponte não reconhece como cópia que ele mesmo guardou: {}. O Ponte não oferece apagá-los; se quiser tirá-los, faça isso você mesmo, depois de conferir o que são.",
        if scan.unrecognised.len() == 1 {
            "um arquivo"
        } else {
            "arquivos"
        },
        list.join("; ")
    ))
}

/// What the confirmation to clean old `-divergente-` copies says; it names
/// every file. Only files every check confirmed are asked about, so only
/// they are called copies the Ponte kept.
pub fn archives_question(found: &[Found]) -> String {
    let names: Vec<String> = found
        .iter()
        .map(|f| match f.age_days {
            Some(days) => format!("{} ({}, {days} dias)", f.name, format_size(f.size)),
            None => format!("{} ({})", f.name, format_size(f.size)),
        })
        .collect();
    let one = found.len() == 1;
    format!(
        "Apagar {}: {}? O Ponte conferiu agora que {} do mesmo acervo deste arquivo, que não {} em uso neste computador e que o conteúdo bate com o código do nome: {} de segurança que ele guardou quando o arquivo divergiu, com mais de {OLD_ARCHIVE_DAYS} dias. Só apague se já conferiu que nada faltou no acervo. Elas não vão para a Lixeira do Windows e não podem ser trazidas de volta. Um arquivo que mudar antes de ser apagado é mantido.",
        if one {
            "esta cópia antiga"
        } else {
            "estas cópias antigas"
        },
        names.join("; "),
        if one { "ele é" } else { "eles são" },
        if one { "está" } else { "estão" },
        if one { "é uma cópia" } else { "são cópias" },
    )
}

/// Deletes the old copies the person confirmed. Each file is opened once, as
/// itself (never through a link, only a regular file really in the folder of
/// `real`, with nobody able to write or replace it while the handle is
/// open), and everything is checked again through that handle: still what it
/// was when listed (size, moment, header), still old, still a copy kept
/// aside of this library's file (the code of its name is the start of its
/// SHA-256, its lineage is the file's, it is not a path this computer uses).
/// The deletion is asked through the same handle. A file that fails a check
/// is left alone and named. Returns what was deleted and the files skipped,
/// with why.
///
/// Reads the cloud's folder: call it from a worker.
pub fn delete_old_archives(
    real: &Path,
    chosen: &[Found],
    paths: &[String],
    now: SystemTime,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    let Some(real_name) = real.file_name().and_then(|n| n.to_str()) else {
        return (deleted, skipped);
    };
    let Some(folder) = real.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return (deleted, skipped);
    };
    let configured = configured_set(paths);
    let expected_lineage = lineage_of(real);
    for found in chosen {
        let name = found.name.as_str();
        let skip = |why: &str| format!("{name}: {why}");
        if found.path.parent() != real.parent()
            || found.path.file_name().and_then(|n| n.to_str()) != Some(name)
            || classify(real_name, name) != Some(Sibling::Divergent)
        {
            skipped.push(skip("não é uma cópia guardada ao lado deste arquivo"));
            continue;
        }
        let Some(listed) = &found.identity else {
            skipped.push(skip("não foi possível identificar o arquivo"));
            continue;
        };
        let mut handle = match crate::auto_backup::open_in_folder(folder, name, true) {
            Ok(handle) => handle,
            Err(error) => {
                skipped.push(skip(&format!(
                    "não foi possível abri-lo com segurança ({error}); foi mantido"
                )));
                continue;
            }
        };
        let Ok(metadata) = handle.metadata() else {
            skipped.push(skip(UNREADABLE));
            continue;
        };
        let Some((header, head_sha)) = read_head(&mut handle) else {
            skipped.push(skip(NOT_PONTE));
            continue;
        };
        let now_identity = Identity {
            size: metadata.len(),
            modified_ns: modified_ns(&metadata).unwrap_or(0),
            header_sha256: head_sha,
        };
        if now_identity != *listed {
            skipped.push(skip("mudou depois de listado; foi mantido"));
            continue;
        }
        if !age_of(&metadata, now).is_some_and(is_old) {
            skipped.push(skip("já não tem mais de 90 dias; foi mantido"));
            continue;
        }
        if let Err(reason) = provenance_without_content(
            &found.path,
            &configured,
            expected_lineage.as_deref(),
            &header.lineage,
        ) {
            skipped.push(skip(&format!("{reason}; foi mantido")));
            continue;
        }
        let matches = aside_code(name)
            .is_some_and(|code| content_matches_code(&mut handle, &code).unwrap_or(false));
        if !matches {
            skipped.push(skip(&format!("{CODE_MISMATCH}; foi mantido")));
            continue;
        }
        match crate::auto_backup::delete_through(handle, &found.path) {
            Ok(()) => deleted.push(found.path.clone()),
            Err(error) => skipped.push(skip(&format!("não foi possível apagar ({error})"))),
        }
    }
    (deleted, skipped)
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
        assert!(full.contains("chegou ao limite") && full.contains("talvez não caiba"));
        assert!(!full.contains("não consegue"));
        assert!(full.contains("esvazie a Lixeira"));
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

    fn header(lineage: &str) -> crate::sharedfolder::Header {
        crate::sharedfolder::Header {
            format: 8,
            schema_version: 21,
            lineage: lineage.into(),
            revision: 3,
            writer: "pc".into(),
            writer_name: "Casa".into(),
            written_at: "2026-01-01T00:00:00Z".into(),
            salt: "AA==".into(),
            nonce: "AA==".into(),
            level: 0,
            points_to: String::new(),
        }
    }

    /// A file that starts like a transport file of the library `lineage`
    /// (the body is whatever follows the header).
    fn sync_file(path: &Path, lineage: &str, body: &[u8]) {
        let mut bytes = SYNC_MAGIC.to_vec();
        bytes.extend_from_slice(&serde_json::to_vec(&header(lineage)).unwrap());
        bytes.push(b'\n');
        bytes.extend_from_slice(body);
        std::fs::write(path, bytes).unwrap();
    }

    const LIBRARY: &str = "acervo-1";

    fn later() -> SystemTime {
        SystemTime::now() + Duration::from_secs(91 * 24 * 60 * 60)
    }

    fn paths(files: &[&Path]) -> Vec<String> {
        files.iter().map(|p| p.display().to_string()).collect()
    }

    /// The shared file of `LIBRARY` and a copy kept aside of it the way the
    /// Ponte does (`keep_aside`), the file then rewritten with other content.
    fn real_and_aside(dir: &Path) -> (PathBuf, PathBuf) {
        let real = dir.join(REAL);
        sync_file(&real, LIBRARY, b"antes da divergencia");
        let aside = crate::sharedfolder::keep_aside(&real).unwrap();
        sync_file(&real, LIBRARY, b"depois, regravado");
        (real, aside)
    }

    #[test]
    fn the_scan_lists_copies_that_are_ponte_files_and_leaves_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        sync_file(&real, LIBRARY, b"x");
        sync_file(&dir.path().join("ponte-sync (1).ponte"), LIBRARY, b"y");
        sync_file(
            &dir.path().join("ponte-sync-DESKTOP-ABC123.ponte"),
            LIBRARY,
            b"z",
        );
        // The name matches, the content is not a Ponte file.
        std::fs::write(dir.path().join("ponte-sync (2).ponte"), b"outra coisa").unwrap();
        // A copy kept aside that is new: neither offered nor said.
        let (_, aside) = real_and_aside(dir.path());
        std::fs::write(dir.path().join("ponte-sync-guarda.ponte"), b"x").unwrap();
        let memo = HashMemo::default();
        let found = scan_all(&paths(&[&real]), SystemTime::now(), &memo)
            .pop()
            .unwrap();
        assert!(found.size.is_some());
        let names: Vec<&str> = found
            .conflict_copies
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["ponte-sync (1).ponte", "ponte-sync-DESKTOP-ABC123.ponte"]
        );
        assert!(found.conflict_copies[0]
            .description
            .as_deref()
            .unwrap()
            .contains("revisão 3"));
        assert!(found.old_archives.is_empty() && found.unrecognised.is_empty());
        let warning = conflict_warning(&found).unwrap();
        assert!(warning.contains("ponte-sync (1).ponte") && warning.contains("DESKTOP-ABC123"));
        assert!(warning.contains("alterações de outro aparelho"));
        assert!(conflict_warning(&FolderScan::default()).is_none());
        // Everything is still there: listing deletes nothing.
        assert!(aside.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 6);
    }

    /// Since the format 9 (0.52.0) the configured name holds only a small
    /// stub and the library lives in `<nome>-formato9.ponte` beside it: the
    /// size is that file's, the file itself is never a copy, the copies the
    /// cloud makes of it are warned about, and an old copy of it kept aside
    /// is offered like the ones of the old name. The stub is in use too.
    #[test]
    fn the_file_of_format_9_is_measured_and_its_copies_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        let nine = crate::sharedfolder::nine_path(&real);
        assert_eq!(nine, dir.path().join("ponte-sync-formato9.ponte"));
        sync_file(&real, LIBRARY, b"stub");
        sync_file(&nine, LIBRARY, &[7_u8; 5000]);
        sync_file(
            &dir.path().join("ponte-sync-formato9 (1).ponte"),
            LIBRARY,
            b"y",
        );
        sync_file(
            &dir.path().join("Cópia de ponte-sync-formato9.ponte"),
            LIBRARY,
            b"y",
        );
        sync_file(&dir.path().join("ponte-sync (1).ponte"), LIBRARY, b"z");
        let aside = crate::sharedfolder::keep_aside(&nine).unwrap();
        sync_file(&nine, LIBRARY, &[8_u8; 5000]);
        let memo = HashMemo::default();
        let found = scan_all(&paths(&[&real]), later(), &memo).pop().unwrap();
        assert_eq!(found.path, real, "the card names the configured file");
        assert_eq!(
            found.size,
            Some(std::fs::metadata(&nine).unwrap().len()),
            "the size is the library's, not the stub's"
        );
        let names: Vec<&str> = found
            .conflict_copies
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Cópia de ponte-sync-formato9.ponte",
                "ponte-sync (1).ponte",
                "ponte-sync-formato9 (1).ponte",
            ]
        );
        assert_eq!(found.old_archives.len(), 1, "{found:?}");
        assert_eq!(found.old_archives[0].path, aside);
        assert!(found.unrecognised.is_empty(), "{found:?}");
        // The file of format 9 is a path in use: never offered for deletion,
        // even when named like a kept copy by hand.
        assert!(configured_set(&paths(&[&real])).contains(&normal_path(&nine)));
        // A folder still at the old format: the old name is measured.
        let old = tempfile::tempdir().unwrap();
        let real8 = old.path().join(REAL);
        sync_file(&real8, LIBRARY, &[1_u8; 300]);
        let found8 = scan_all(&paths(&[&real8]), SystemTime::now(), &memo)
            .pop()
            .unwrap();
        assert_eq!(found8.size, Some(std::fs::metadata(&real8).unwrap().len()));
    }

    #[test]
    fn an_old_copy_kept_aside_by_the_ponte_is_offered_once_every_check_passes() {
        let dir = tempfile::tempdir().unwrap();
        let (real, aside) = real_and_aside(dir.path());
        let memo = HashMemo::default();
        let files = paths(&[&real]);
        let found = scan_all(&files, later(), &memo).pop().unwrap();
        assert_eq!(found.old_archives.len(), 1);
        assert_eq!(found.old_archives[0].path, aside);
        assert!(found.old_archives[0].age_days.unwrap() >= 90);
        assert!(found.old_archives[0].identity.is_some());
        assert!(found.unrecognised.is_empty());
        // The content was read once; the next scan relies on what it learned.
        assert_eq!(memo.0.lock().unwrap().len(), 1);
        assert_eq!(scan_all(&files, later(), &memo).pop().unwrap(), found);
        // Not old yet: nothing is offered.
        assert!(scan_all(&files, SystemTime::now(), &memo)
            .pop()
            .unwrap()
            .old_archives
            .is_empty());
        let question = archives_question(&found.old_archives);
        assert!(question.contains(aside.file_name().unwrap().to_str().unwrap()));
        assert!(question.contains("não vão para a Lixeira"));
        assert!(question.contains("conferiu agora"));
    }

    #[test]
    fn a_live_file_named_like_a_kept_copy_is_shown_and_never_offered() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        sync_file(&real, LIBRARY, b"principal");
        // Another shared file of the same library, configured on this
        // computer, with a name that has the shape of a kept copy.
        let live = dir
            .path()
            .join("ponte-sync-divergente-a1b2c3d4e5f60789.ponte");
        sync_file(&live, LIBRARY, b"vivo");
        let memo = HashMemo::default();
        // Configured: refused whatever its content.
        let scan = scan_all(&paths(&[&real, &live]), later(), &memo).remove(0);
        assert!(scan.old_archives.is_empty());
        assert_eq!(scan.unrecognised.len(), 1);
        assert_eq!(scan.unrecognised[0].reason, IN_USE);
        // Not configured: its content does not match the code in its name.
        let scan = scan_all(&paths(&[&real]), later(), &memo).remove(0);
        assert!(scan.old_archives.is_empty());
        assert_eq!(scan.unrecognised[0].reason, CODE_MISMATCH);
        // A live file made to look kept: content and name agree (the name
        // carries the hash of the content), but it is a configured path.
        let (_, aside) = real_and_aside(dir.path());
        let scan = scan_all(&paths(&[&real, &aside]), later(), &memo).remove(0);
        assert!(scan.old_archives.is_empty());
        assert!(scan.unrecognised.iter().any(|u| u.reason == IN_USE));
        // The note shows them without calling them backups.
        let note = unrecognised_note(&scan).unwrap();
        assert!(note.contains("não reconhece") && note.contains("não oferece apagá-los"));
        assert!(!note.contains("segurança"));
        // Asked for deletion anyway, it stays.
        let wanted = Found {
            path: live.clone(),
            name: live.file_name().unwrap().to_str().unwrap().to_owned(),
            size: 0,
            age_days: None,
            description: None,
            identity: None,
        };
        let (deleted, skipped) = delete_old_archives(&real, &[wanted], &paths(&[&real]), later());
        assert!(deleted.is_empty() && live.exists());
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn a_copy_of_another_library_or_without_a_reference_is_not_offered() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        // A copy kept aside while the file belonged to another library.
        sync_file(&real, "acervo-2", b"de outro acervo");
        let aside = crate::sharedfolder::keep_aside(&real).unwrap();
        sync_file(&real, LIBRARY, b"principal");
        // The name has the shape and the content agrees with it, the
        // library is another one.
        let memo = HashMemo::default();
        let scan = scan_all(&paths(&[&real]), later(), &memo).remove(0);
        assert!(scan.old_archives.is_empty());
        assert_eq!(scan.unrecognised.len(), 1);
        assert_eq!(scan.unrecognised[0].reason, OTHER_LIBRARY);
        assert!(aside.exists());
        // Without the shared file to compare with, nothing is recognised.
        std::fs::remove_file(&real).unwrap();
        let scan = scan_all(&paths(&[&real]), later(), &memo).remove(0);
        assert!(scan.old_archives.is_empty());
        assert_eq!(scan.unrecognised[0].reason, NO_REFERENCE);
    }

    #[test]
    fn something_that_is_not_a_ponte_file_is_shown_as_not_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join(REAL);
        sync_file(&real, LIBRARY, b"principal");
        std::fs::write(
            dir.path()
                .join("ponte-sync-divergente-a1b2c3d4e5f60789.ponte"),
            b"qualquer coisa",
        )
        .unwrap();
        let scan = scan_all(&paths(&[&real]), later(), &HashMemo::default()).remove(0);
        assert!(scan.old_archives.is_empty());
        assert_eq!(scan.unrecognised[0].reason, NOT_PONTE);
    }

    fn offered(real: &Path) -> Vec<Found> {
        scan_all(&paths(&[real]), later(), &HashMemo::default())
            .remove(0)
            .old_archives
    }

    #[test]
    fn deleting_removes_only_the_file_that_was_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let (real, aside) = real_and_aside(dir.path());
        let copy = dir.path().join("ponte-sync (1).ponte");
        sync_file(&copy, LIBRARY, b"copia da nuvem");
        let files = paths(&[&real]);
        let listed = offered(&real);
        assert_eq!(listed.len(), 1);
        // Not old yet when the person answers: kept and named.
        let (deleted, skipped) = delete_old_archives(&real, &listed, &files, SystemTime::now());
        assert!(deleted.is_empty() && aside.exists());
        assert!(skipped[0].contains("90 dias") && skipped[0].contains("foi mantido"));
        // The real file and a conflict copy are never deleted, however asked.
        let mut by_hand = listed[0].clone();
        by_hand.path = real.clone();
        by_hand.name = REAL.into();
        let mut by_hand_copy = listed[0].clone();
        by_hand_copy.path = copy.clone();
        by_hand_copy.name = "ponte-sync (1).ponte".into();
        let (deleted, skipped) =
            delete_old_archives(&real, &[by_hand, by_hand_copy], &files, later());
        assert!(deleted.is_empty() && real.exists() && copy.exists());
        assert_eq!(skipped.len(), 2);
        // Old and unchanged: deleted, and only that one.
        let (deleted, skipped) = delete_old_archives(&real, &listed, &files, later());
        assert_eq!(deleted, std::slice::from_ref(&aside));
        assert!(skipped.is_empty() && !aside.exists());
        assert!(real.exists() && copy.exists());
    }

    #[test]
    fn a_file_that_changed_after_it_was_listed_is_skipped_and_named() {
        let dir = tempfile::tempdir().unwrap();
        let (real, aside) = real_and_aside(dir.path());
        let files = paths(&[&real]);
        let name = aside.file_name().unwrap().to_str().unwrap().to_owned();

        // Another file of the same library put under the same name (a cloud
        // client replacing it): another size and another header.
        let listed = offered(&real);
        let mut replacement = std::fs::read(&aside).unwrap();
        replacement.extend_from_slice(b" e mais");
        std::fs::write(&aside, &replacement).unwrap();
        let (deleted, skipped) = delete_old_archives(&real, &listed, &files, later());
        assert!(deleted.is_empty() && aside.exists());
        assert!(skipped[0].starts_with(&name) && skipped[0].contains("mudou depois de listado"));

        // Same size, other moment: also another file.
        let mut listed = offered(&real);
        assert!(listed.is_empty(), "the content no longer matches its name");
        std::fs::remove_file(&aside).unwrap();
        let (real, aside) = real_and_aside(dir.path());
        listed = offered(&real);
        assert_eq!(listed.len(), 1);
        let identity = listed[0].identity.clone().unwrap();
        listed[0].identity = Some(Identity {
            modified_ns: identity.modified_ns + 1,
            ..identity.clone()
        });
        let (deleted, skipped) = delete_old_archives(&real, &listed, &files, later());
        assert!(deleted.is_empty() && aside.exists());
        assert!(skipped[0].contains("mudou depois de listado"));
        // Same size and moment, another header.
        listed[0].identity = Some(Identity {
            header_sha256: "0".repeat(64),
            ..identity
        });
        let (deleted, skipped) = delete_old_archives(&real, &listed, &files, later());
        assert!(deleted.is_empty() && aside.exists());
        assert!(skipped[0].contains("mudou depois de listado"));
        // Confirmed as it was listed, it goes.
        let listed = offered(&real);
        let (deleted, _) = delete_old_archives(&real, &listed, &files, later());
        assert_eq!(deleted, std::slice::from_ref(&aside));
        assert!(!aside.exists());
    }

    #[test]
    fn a_swapped_file_with_the_same_identity_but_other_content_is_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (real, aside) = real_and_aside(dir.path());
        let files = paths(&[&real]);
        let listed = offered(&real);
        // Same size, same header, same moment: only the body differs, so the
        // content no longer matches the code of the name.
        let metadata = std::fs::metadata(&aside).unwrap();
        let mut bytes = std::fs::read(&aside).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&aside, &bytes).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&aside)
            .unwrap()
            .set_modified(metadata.modified().unwrap())
            .unwrap();
        let (deleted, skipped) = delete_old_archives(&real, &listed, &files, later());
        assert!(deleted.is_empty() && aside.exists());
        assert!(
            skipped[0].contains("não confere com o código"),
            "{skipped:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_followed_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (real, aside) = real_and_aside(dir.path());
        let target = dir.path().join("alvo.bin");
        std::fs::rename(&aside, &target).unwrap();
        std::os::unix::fs::symlink(&target, &aside).unwrap();
        // Listed as nothing, and refused when asked.
        assert!(offered(&real).is_empty());
        let wanted = Found {
            path: aside.clone(),
            name: aside.file_name().unwrap().to_str().unwrap().to_owned(),
            size: 0,
            age_days: None,
            description: None,
            identity: Some(Identity {
                size: 0,
                modified_ns: 0,
                header_sha256: String::new(),
            }),
        };
        let (deleted, skipped) = delete_old_archives(&real, &[wanted], &paths(&[&real]), later());
        assert!(deleted.is_empty() && target.exists() && aside.exists());
        assert_eq!(skipped.len(), 1);
    }

    /// The same as the Unix test, where the system lets a file link be made
    /// (Windows needs a privilege for it; without it there is nothing to test
    /// and the test says nothing).
    #[cfg(windows)]
    #[test]
    fn a_windows_link_is_never_followed_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (real, aside) = real_and_aside(dir.path());
        let target = dir.path().join("alvo.bin");
        std::fs::rename(&aside, &target).unwrap();
        if std::os::windows::fs::symlink_file(&target, &aside).is_err() {
            return;
        }
        assert!(offered(&real).is_empty());
        let wanted = Found {
            path: aside.clone(),
            name: aside.file_name().unwrap().to_str().unwrap().to_owned(),
            size: 0,
            age_days: None,
            description: None,
            identity: Some(Identity {
                size: 0,
                modified_ns: 0,
                header_sha256: String::new(),
            }),
        };
        let (deleted, skipped) = delete_old_archives(&real, &[wanted], &paths(&[&real]), later());
        assert!(deleted.is_empty() && target.exists());
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn a_scan_is_due_once_at_a_time_and_at_most_every_minute() {
        let sec = Duration::from_secs;
        // Never listed: due, unless one is already running.
        assert!(scan_due(None, false, false));
        assert!(!scan_due(None, true, false));
        // Fresh result for the same folders: not due.
        assert!(!scan_due(Some((sec(59), true)), false, false));
        assert!(scan_due(Some((sec(60), true)), false, false));
        // Folders changed, or an exchange ended: due now, still one at a time.
        assert!(scan_due(Some((sec(1), false)), false, false));
        assert!(scan_due(Some((sec(1), true)), false, true));
        assert!(!scan_due(Some((sec(1), false)), true, true));
        assert!(!scan_due(Some((sec(600), true)), true, false));
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
