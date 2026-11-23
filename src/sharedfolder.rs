//! "Pasta compartilhada": the same person using the Ponte on more than one
//! computer exchanges the library through a transport file kept in one or
//! more synced folders (Google Drive, OneDrive, a pen drive, a network share).
//!
//! The file never contains the SQLite acervo (DPAPI-sealed, machine-bound,
//! and SQLite in a synced folder corrupts): it carries a `CabinetSnapshot`
//! plus the PDF and attachment bytes, encrypted with a passphrase the person
//! chooses (PBKDF2-HMAC-SHA256 → AES-256-GCM, from `ring`, already compiled
//! for TLS). Every copy also lists which queued changes it already
//! incorporates (`applied`) and keeps a journal of recent changes, so two
//! copies that diverged (two computers wrote without seeing each other) can
//! be reconciled without losing anything.
use crate::intranet::{CabinetSnapshot, IntranetMutation};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

/// Format 8 (schema 21) carries the projects and the project uid of tasks. A
/// Ponte that only knows format 7 refuses such a file with the "versão mais
/// nova" message instead of rewriting it without them. A copy of an older
/// format loads with no projects (the field has a default), and applying it
/// keeps the ones the library has.
///
/// Format 7 (schema 20) carries the library settings (the holidays shared by
/// every computer and phone of the library). A Ponte that only knows format 6
/// refuses such a file with the "versão mais nova" message instead of
/// rewriting it without them. A copy of an older format loads with no
/// settings (the field has a default), and applying it keeps the ones the
/// library has.
///
/// Format 5 (schema 17) carries the categories and the category uids of tasks
/// and documents. A Ponte that only knows format 4 refuses such a file with
/// the "versão mais nova" message instead of rewriting it without them. A
/// copy of an older format loads with no categories (the field has a
/// default).
///
/// Format 4 (schema 16) carries the proposals of the AI: the facts, proposals
/// and run of the analyses, and the proposal ids of tasks, uses and
/// atendimentos. A Ponte that only knows format 3 refuses such a file with
/// the "versão mais nova" message instead of rewriting it without them.
/// Files of formats 1 to 3 are still read: format 3 (schema 15) carries the
/// recursos (a copy without `resources` loads with none); format 2 may carry
/// tasks without document, `document_id: null`; a copy of an older format
/// loads with no facts, proposals or proposal ids (every new field has a
/// default).
pub const FORMAT: u32 = 8;
/// Format 9 (schema 22, F2b of Frente F) carries the identity of every item
/// (`CabinetSnapshot::idents`, sorted triples) and the epoch they were cut
/// in. It lives in a file of its own beside the old one
/// ([`nine_path`]), and the old name then holds a stub that points to it
/// (`Header::points_to`): a Ponte that knows formats up to 8 refuses both
/// with the "versão mais nova" message, before writing anything. Until a
/// folder is promoted, this build still writes format 8 there.
pub const FORMAT_9: u32 = 9;
/// The newest format this build reads.
pub const NEWEST_FORMAT: u32 = FORMAT_9;
/// The level of the format 9 this build writes and reads (a reader refuses
/// a higher one: "Atualize o Ponte"). Level 2 (F3 of Frente F): every change
/// carries the proof of the items it names (`idrefs::Refs`); a Ponte of
/// level 1 (F2b) refuses the file, and a main computer of this build refuses
/// the requests of one.
pub const LEVEL: u32 = 2;
/// What the file of format 9 adds to the stem of the configured name.
pub const NINE_SUFFIX: &str = "-formato9";
/// How every transport file of a shared folder starts.
pub const SYNC_MAGIC: &[u8] = b"PONTE-SYNC1\n";
const MAGIC: &[u8] = SYNC_MAGIC;
/// How every portable copy (`.pontecopia`, Ajustes ▸ Cópia de segurança)
/// starts. No sync reader accepts it (they all want [`SYNC_MAGIC`]); this one
/// says what the file is instead of only refusing it.
pub const PORTABLE_MAGIC: &[u8] = b"PONTE-COPIA1\n";
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// Journal retention: enough for a few weeks of one person's work.
pub const MAX_JOURNAL_ENTRIES: usize = 500;
pub const MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;
const PBKDF2_ITERATIONS: u32 = 600_000;
const KEY_BYTES: usize = 32;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;
const CONTAINER_VERSION: u8 = 1;

/// Plain part of the file: enough to compare copies without the passphrase.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Header {
    pub format: u32,
    pub schema_version: i64,
    /// Identity of the library this copy belongs to (set at first write).
    pub lineage: String,
    pub revision: i64,
    /// Computer that wrote this copy (`folder_writer` of its configuration).
    pub writer: String,
    pub writer_name: String,
    pub written_at: String,
    pub salt: String,
    pub nonce: String,
    /// Level of format 9 (absent, 0, before it).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level: u32,
    /// A stub: the name of the file of format 9 beside it (empty otherwise).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub points_to: String,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Whether this build can take the content of a copy with `header`: a file
/// of format 8 up to the content schema of this build, one of format 9 up
/// to its library schema.
pub fn schema_readable(header: &Header) -> bool {
    header.schema_version
        <= if header.format >= FORMAT_9 {
            crate::storage::LIBRARY_SCHEMA
        } else {
            crate::storage::SCHEMA_VERSION
        }
}

/// Where the file of format 9 of the configured path `path` lives: beside
/// it, `<stem>-formato9.ponte`.
pub fn nine_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ponte-sync");
    path.with_file_name(format!("{stem}{NINE_SUFFIX}.ponte"))
}

/// What a transport file holds: a copy, or a stub saying the folder moved
/// to the file of format 9 (`moved_to`, a bare file name beside it).
// A copy is read once and used in place: boxing it would only add noise.
#[allow(clippy::large_enum_variant)]
pub enum Found {
    Copy(Copy),
    Moved { header: Header, moved_to: String },
}

/// One change carried along so that a diverged copy can be reconciled.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub uid: String,
    pub created_at: String,
    pub actor: String,
    pub writer: String,
    pub label: String,
    pub mutation: IntranetMutation,
    #[serde(default)]
    pub base: Option<IntranetMutation>,
    /// Revision of the copy right after this change was incorporated.
    pub revision: i64,
    /// What the change created, with the numbers of this copy (its
    /// `mutation` uses them too), so that a copy that numbered differently
    /// can translate later changes.
    #[serde(default)]
    pub created: Vec<crate::idmap::CreatedId>,
    /// The identities of the rows the change inserted, by slot (format 9
    /// only), so that a replay gives them to the rows it inserts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub idents: Vec<crate::idents::SlotIdent>,
    /// The proofs of the items the change and its base name (level 2);
    /// `None`: written by an older Ponte, legacy up to
    /// [`Payload::refs_since`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<crate::idrefs::Refs>,
    /// Taken by the legacy route (no proofs: queued by an older Ponte, or an
    /// old copy's change): legacy whatever its revision. A change replayed
    /// in a later write lands above `refs_since`, where `None` alone would
    /// read as damaged (F3.0a2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy: bool,
}

/// Encrypted part of the file.
#[derive(Serialize, Deserialize)]
pub struct Payload {
    pub snapshot: CabinetSnapshot,
    /// `uid`s of every queued change incorporated in this lineage.
    pub applied: Vec<String>,
    pub journal: Vec<JournalEntry>,
    /// The revision of the first write of level 2 of this lineage, carried
    /// by every later write: a journal entry without proofs at or below it
    /// is legacy; above it, a damaged change (F3, adendo §3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs_since: Option<i64>,
}

/// PDF and attachment bytes by id, matching the snapshot's `files_meta`
/// and `attachments` (checked by SHA-256 when applied).
#[derive(Default)]
pub struct Blobs {
    pub pdfs: BTreeMap<i64, Vec<u8>>,
    pub attachments: BTreeMap<i64, Vec<u8>>,
}

pub struct Copy {
    pub path: PathBuf,
    pub header: Header,
    pub payload: Payload,
    pub blobs: Blobs,
}

/// What a snapshot holds, as a stable id: a SHA-256 of its canonical form
/// (keys in order). The same content gives the same id on every computer,
/// whatever the path, the salt or the moment it was written, so copies that
/// tie on revision and moment can be told apart the same way everywhere, and
/// a computer can tell that the copy at its own revision is not the content it
/// holds.
pub fn snapshot_identity(snapshot: &CabinetSnapshot) -> String {
    let canonical = serde_json::to_value(snapshot)
        .and_then(|value| serde_json::to_vec(&value))
        .unwrap_or_default();
    let digest = ring::digest::digest(&ring::digest::SHA256, &canonical);
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

impl Copy {
    /// See [`snapshot_identity`].
    pub fn content_identity(&self) -> String {
        snapshot_identity(&self.payload.snapshot)
    }
}

/// Orders copies newest first: higher revision, then later moment. Copies
/// that tie on both are ordered by the identity of their content and then by
/// who wrote them, never by the order of the folders in this computer's list,
/// so every computer takes the same one as the base.
pub fn sort_newest_first(copies: &mut Vec<Copy>) {
    let ties = |copies: &[Copy], index: usize| {
        let this = &copies[index].header;
        copies.iter().enumerate().any(|(other, copy)| {
            other != index
                && copy.header.revision == this.revision
                && copy.header.written_at == this.written_at
        })
    };
    let mut keyed: Vec<((i64, String, String, String), Copy)> = Vec::new();
    let tied: Vec<bool> = (0..copies.len()).map(|i| ties(copies, i)).collect();
    for (index, copy) in std::mem::take(copies).into_iter().enumerate() {
        let identity = if tied[index] {
            copy.content_identity()
        } else {
            String::new()
        };
        keyed.push((
            (
                copy.header.revision,
                copy.header.written_at.clone(),
                identity,
                copy.header.writer.clone(),
            ),
            copy,
        ));
    }
    keyed.sort_by(|a, b| b.0.cmp(&a.0));
    copies.extend(keyed.into_iter().map(|(_, copy)| copy));
}

/// Where [`keep_aside`] puts the copy at `path`: beside it, named by what it
/// holds (a SHA-256 prefix), so the same content is never archived twice and
/// the name cannot be taken for a working file.
pub fn aside_path(path: &Path) -> Result<PathBuf> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Não foi possível ler {}.", path.display()))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let short: String = digest
        .as_ref()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ponte-sync");
    Ok(path.with_file_name(format!("{stem}-divergente-{short}.ponte")))
}

/// The SHA-256 of the file at `path`, in hex: two copies with the same
/// bytes are the same copy.
pub fn file_digest(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Não foi possível ler {}.", path.display()))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    Ok(digest.as_ref().iter().map(|b| format!("{b:02x}")).collect())
}

/// Keeps a copy that is about to be replaced: the same file, beside it, under
/// a name that says it diverged. Returns where it went.
pub fn keep_aside(path: &Path) -> Result<PathBuf> {
    let aside = aside_path(path)?;
    if aside.exists() {
        // Kept by an earlier attempt (the folder could not be rewritten and
        // the exchange came back): the same content, so there is one archive
        // however many times it is retried. Anything else under that name is
        // not touched and is not taken for the archive.
        let same = aside.is_file()
            && std::fs::read(&aside)
                .ok()
                .zip(std::fs::read(path).ok())
                .is_some_and(|(a, b)| a == b);
        ensure!(same, "Já existe um arquivo em {}.", aside.display());
        return Ok(aside);
    }
    // Written beside it under a temporary name and renamed only when read
    // back whole: an interruption never leaves a partial file at the
    // archive's name (which would then refuse every later attempt).
    let fail = || format!("Não foi possível guardar uma cópia de {}.", path.display());
    let bytes = std::fs::read(path).with_context(fail)?;
    let part = PathBuf::from(format!("{}.parte", aside.display()));
    let written = (|| -> std::io::Result<bool> {
        {
            let mut file = std::fs::File::create(&part)?;
            std::io::Write::write_all(&mut file, &bytes)?;
            file.sync_all()?;
        }
        Ok(std::fs::read(&part)? == bytes)
    })();
    match written {
        Ok(true) => {}
        Ok(false) => {
            let _ = std::fs::remove_file(&part);
            anyhow::bail!("{} A cópia gravada não confere.", fail());
        }
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            return Err(error).with_context(fail);
        }
    }
    if let Err(error) = std::fs::rename(&part, &aside) {
        let _ = std::fs::remove_file(&part);
        return Err(error).with_context(fail);
    }
    Ok(aside)
}

pub(crate) fn random(bytes: usize) -> Result<Vec<u8>> {
    use ring::rand::SecureRandom;
    let mut out = vec![0_u8; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut out)
        .map_err(|_| anyhow::anyhow!("Não foi possível proteger o arquivo de sincronização."))?;
    Ok(out)
}

pub(crate) fn derive_key(password: &str, salt: &[u8]) -> [u8; KEY_BYTES] {
    let mut key = [0_u8; KEY_BYTES];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(PBKDF2_ITERATIONS).expect("non-zero"),
        salt,
        password.as_bytes(),
        &mut key,
    );
    key
}

pub fn validate_password(password: &str) -> Result<()> {
    ensure!(
        password.chars().count() >= 8
            && password.len() <= 256
            && !password.chars().any(char::is_control),
        "A senha de sincronização precisa ter pelo menos 8 caracteres."
    );
    Ok(())
}

/// Reads only the plain header. `None` when the file does not exist.
pub fn read_header(path: &Path) -> Result<Option<Header>> {
    read_header_up_to(path, NEWEST_FORMAT)
}

const DAMAGED: &str = "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.";

/// [`read_header`] for a reader that knows the formats up to `newest`: this
/// is how every build refuses the file of a newer one (and how the tests
/// stand in for an older build: `read_header_up_to(path, 8)` is the very
/// check of a Ponte 0.50/0.51, which refuses a file of format 9 or a stub
/// before it reads anything else).
pub(crate) fn read_header_up_to(path: &Path, newest: u32) -> Result<Option<Header>> {
    let Some(line) = header_line(path)? else {
        return Ok(None);
    };
    let header: Header = serde_json::from_slice(&line[MAGIC.len()..]).context(DAMAGED)?;
    ensure!(
        (1..=newest).contains(&header.format) && header.level <= LEVEL,
        "O arquivo de sincronização foi gravado por uma versão mais nova do Ponte. Atualize o Ponte."
    );
    Ok(Some(header))
}

/// The plain start of the file, as it is on disk: the magic and the header
/// line (salt and nonce included), without the line break. `None` when the
/// file does not exist. What a writer compares, byte for byte, right before
/// it replaces the file: any other writer in between changes it.
pub fn header_line(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("Não foi possível ler {}.", path.display()))
        }
    };
    let mut magic = vec![0_u8; MAGIC.len()];
    file.read_exact(&mut magic).with_context(|| {
        format!(
            "{} não é um arquivo de sincronização do Ponte.",
            path.display()
        )
    })?;
    ensure!(
        !PORTABLE_MAGIC.starts_with(&magic),
        "{} é uma cópia portátil do Ponte (.pontecopia), não um arquivo de sincronização: não pode ser usado como pasta compartilhada. Para abri-la, use Ajustes ▸ Cópia de segurança ▸ Importar cópia portátil…",
        path.display()
    );
    ensure!(
        magic == MAGIC,
        "{} não é um arquivo de sincronização do Ponte.",
        path.display()
    );
    let mut line = magic;
    let mut reader = std::io::BufReader::new(file);
    let mut byte = [0_u8; 1];
    loop {
        reader.read_exact(&mut byte).context(DAMAGED)?;
        if byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
        ensure!(line.len() <= MAGIC.len() + 64 * 1024, DAMAGED);
    }
    Ok(Some(line))
}

/// The body of a stub.
#[derive(Serialize, Deserialize)]
struct StubBody {
    moved_to: String,
}

/// Whether `name` is a bare file name of a transport file (what a stub may
/// point to: beside it, never elsewhere).
fn bare_ponte_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != "."
        && name != ".."
        && name.to_lowercase().ends_with(".ponte")
}

/// Reads and decrypts a copy, or a stub (format 9, `points_to`), whose
/// header the passphrase authenticates like any copy's. `None` when the file
/// does not exist.
pub fn read_any(path: &Path, password: &str) -> Result<Option<Found>> {
    let Some(header) = read_header(path)? else {
        return Ok(None);
    };
    let size = std::fs::metadata(path)?.len();
    ensure!(
        size <= MAX_FILE_BYTES,
        "O arquivo de sincronização passa de 512 MB e não pode ser aberto."
    );
    let bytes =
        std::fs::read(path).with_context(|| format!("Não foi possível ler {}.", path.display()))?;
    let header_end = MAGIC.len()
        + bytes[MAGIC.len()..]
            .iter()
            .position(|b| *b == b'\n')
            .context(DAMAGED)?;
    let header_bytes = bytes[MAGIC.len()..header_end].to_vec();
    let mut body = bytes[header_end + 1..].to_vec();
    let salt = STANDARD.decode(&header.salt).context(DAMAGED)?;
    let nonce = STANDARD.decode(&header.nonce).context(DAMAGED)?;
    ensure!(nonce.len() == NONCE_BYTES, DAMAGED);
    let key = ring::aead::LessSafeKey::new(
        ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &derive_key(password, &salt))
            .map_err(|_| anyhow::anyhow!("Senha de sincronização inválida."))?,
    );
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce)
        .map_err(|_| anyhow::anyhow!(DAMAGED))?;
    let plain = key
        .open_in_place(nonce, ring::aead::Aad::from(&header_bytes), &mut body)
        .map_err(|_| anyhow::anyhow!("Senha de sincronização incorreta, ou arquivo danificado ou ainda sendo gravado pela nuvem. Confira a senha e espere a pasta terminar de sincronizar."))?;
    if !header.points_to.is_empty() {
        ensure!(header.format >= FORMAT_9, DAMAGED);
        let stub = unpack_stub(plain)?;
        ensure!(
            stub.moved_to == header.points_to && bare_ponte_name(&stub.moved_to),
            DAMAGED
        );
        return Ok(Some(Found::Moved {
            header,
            moved_to: stub.moved_to,
        }));
    }
    let (payload, blobs) = unpack(plain)?;
    Ok(Some(Found::Copy(Copy {
        path: path.to_path_buf(),
        header,
        payload,
        blobs,
    })))
}

/// Reads and decrypts a copy. `None` when the file does not exist. A stub
/// is refused, saying where the copy went.
pub fn read(path: &Path, password: &str) -> Result<Option<Copy>> {
    match read_any(path, password)? {
        None => Ok(None),
        Some(Found::Copy(copy)) => Ok(Some(copy)),
        Some(Found::Moved { moved_to, .. }) => anyhow::bail!(
            "O arquivo {} foi atualizado para o formato novo do Ponte; o acervo está agora em {moved_to}, na mesma pasta.",
            path.display()
        ),
    }
}

/// Everything the caller decides about a new copy.
pub struct WriteRequest<'a> {
    pub lineage: &'a str,
    pub revision: i64,
    pub writer: &'a str,
    pub writer_name: &'a str,
    pub schema_version: i64,
}

/// What the plain header of a file says besides the request: its format,
/// level and, for a stub, where the copy went.
struct Shape<'a> {
    format: u32,
    level: u32,
    points_to: &'a str,
}

/// The error of a writer that found the destination changed by another
/// writer between the moment it was read and the rename: the exchange stops
/// (nothing adopted), and the next one reads it again.
#[derive(Debug)]
pub struct ChangedMeanwhile(pub PathBuf);

impl std::fmt::Display for ChangedMeanwhile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "O arquivo {} mudou durante a sincronização (outro aparelho gravou nele). Nada foi substituído nem adotado; sincronize de novo.",
            self.0.display()
        )
    }
}

impl std::error::Error for ChangedMeanwhile {}

/// Encrypts and writes atomically (temporary file, then rename), so a cloud
/// client never sees a half-written copy. Returns the header written.
pub fn write(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    payload: &Payload,
    blobs: &Blobs,
) -> Result<Header> {
    let written_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    write_stamped(path, password, request, payload, blobs, &written_at)
}

/// [`write`] with the moment of the header given: two copies written within
/// the same second carry the same one (the tests need that to be exact).
pub(crate) fn write_stamped(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    payload: &Payload,
    blobs: &Blobs,
    written_at: &str,
) -> Result<Header> {
    write_shaped(
        path,
        password,
        request,
        pack(payload, blobs)?,
        &Shape {
            format: FORMAT,
            level: 0,
            points_to: "",
        },
        written_at,
        None,
    )
}

/// [`write`] of a copy of format 9.
pub fn write_nine(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    payload: &Payload,
    blobs: &Blobs,
) -> Result<Header> {
    write_checked(
        path, password, request, payload, blobs, FORMAT_9, None, None,
    )
}

/// [`write`] in `format` (8 or 9) that first compares, right before the
/// rename, the plain start of the file at `path` with `expected` (what was
/// read at the start of the exchange: `Some(None)` for "no file"): any
/// difference fails with [`ChangedMeanwhile`] and leaves the file as it is.
/// `written_at`: the moment of the exchange, the same in every file it
/// writes (now when `None`).
#[allow(clippy::too_many_arguments)]
pub fn write_checked(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    payload: &Payload,
    blobs: &Blobs,
    format: u32,
    expected: Option<&Option<Vec<u8>>>,
    written_at: Option<&str>,
) -> Result<Header> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let written_at = written_at.unwrap_or(&now);
    let nine = format >= FORMAT_9;
    write_shaped(
        path,
        password,
        request,
        pack(payload, blobs)?,
        &Shape {
            format: if nine { FORMAT_9 } else { FORMAT },
            level: if nine { LEVEL } else { 0 },
            points_to: "",
        },
        written_at,
        expected,
    )
}

/// Writes a stub at `path` that points to `moved_to` (a file name beside
/// it), comparing the destination as [`write_checked`] does.
pub fn write_stub(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    moved_to: &str,
    expected: Option<&Option<Vec<u8>>>,
) -> Result<Header> {
    ensure!(
        bare_ponte_name(moved_to),
        "Nome inválido para o arquivo do formato novo."
    );
    let json = serde_json::to_vec(&StubBody {
        moved_to: moved_to.to_owned(),
    })?;
    let mut body = Vec::with_capacity(json.len() + 32);
    body.push(CONTAINER_VERSION);
    put_u64(&mut body, json.len() as u64);
    body.extend_from_slice(&json);
    put_u64(&mut body, 0);
    put_u64(&mut body, 0);
    let written_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    write_shaped(
        path,
        password,
        request,
        body,
        &Shape {
            format: FORMAT_9,
            level: LEVEL,
            points_to: moved_to,
        },
        &written_at,
        expected,
    )
}

/// What became of the old name when a stub was to replace it.
pub enum Stubbed {
    /// The stub is there; what the name held (if anything) was copied aside,
    /// verified, at this path first.
    Installed(Option<PathBuf>),
    /// The name changed since it was read (another writer): it was left as
    /// it is, and the next exchange takes that file in as an old copy.
    Changed,
}

/// Replaces the file at `path`, which held `expected` (its plain start as
/// read at the start of the exchange, `None`: no file), by a stub pointing
/// to `moved_to`. The name is never left empty: what it holds is COPIED
/// aside (verified) first, and the stub then takes its place in one replace,
/// after the start of the file is compared once more. A cloud folder offers
/// no atomic compare-and-swap: an older writer that still wins the last
/// instant is not prevented but detected later, since any file of format 8
/// at the old name is an old copy for every device that has seen the 9 (kept
/// aside, taken in, and the stub put back).
pub fn install_stub(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    moved_to: &str,
    expected: &Option<Vec<u8>>,
) -> Result<Stubbed> {
    let aside = if expected.is_some() {
        Some(keep_aside(path)?)
    } else {
        None
    };
    match write_stub(path, password, request, moved_to, Some(expected)) {
        Ok(_) => Ok(Stubbed::Installed(aside)),
        Err(error) if error.is::<ChangedMeanwhile>() => Ok(Stubbed::Changed),
        Err(error) => Err(error),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_shaped(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    mut body: Vec<u8>,
    shape: &Shape<'_>,
    written_at: &str,
    expected: Option<&Option<Vec<u8>>>,
) -> Result<Header> {
    ensure!(!password.is_empty(), "Informe a senha de sincronização.");
    let salt = random(SALT_BYTES)?;
    let nonce_bytes = random(NONCE_BYTES)?;
    let header = Header {
        format: shape.format,
        schema_version: request.schema_version,
        lineage: request.lineage.to_owned(),
        revision: request.revision,
        writer: request.writer.to_owned(),
        writer_name: request.writer_name.to_owned(),
        written_at: written_at.to_owned(),
        salt: STANDARD.encode(&salt),
        nonce: STANDARD.encode(&nonce_bytes),
        level: shape.level,
        points_to: shape.points_to.to_owned(),
    };
    let header_bytes = serde_json::to_vec(&header)?;
    ensure!(!header_bytes.contains(&b'\n'), DAMAGED);
    let key = ring::aead::LessSafeKey::new(
        ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &derive_key(password, &salt))
            .map_err(|_| anyhow::anyhow!("Senha de sincronização inválida."))?,
    );
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce_bytes)
        .map_err(|_| anyhow::anyhow!("Não foi possível proteger o arquivo de sincronização."))?;
    key.seal_in_place_append_tag(nonce, ring::aead::Aad::from(&header_bytes), &mut body)
        .map_err(|_| anyhow::anyhow!("Não foi possível proteger o arquivo de sincronização."))?;
    let total = MAGIC.len() as u64 + header_bytes.len() as u64 + 1 + body.len() as u64;
    ensure!(
        total <= MAX_FILE_BYTES,
        "O acervo passa de 512 MB e não cabe no arquivo de sincronização."
    );
    let parent = path
        .parent()
        .context("Caminho do arquivo de sincronização inválido.")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("A pasta {} não está acessível.", parent.display()))?;
    let temp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("ponte-sync.ponte"),
        uuid::Uuid::new_v4().simple()
    ));
    let changed = std::cell::Cell::new(false);
    let result = (|| -> Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(MAGIC)?;
        file.write_all(&header_bytes)?;
        file.write_all(b"\n")?;
        file.write_all(&body)?;
        file.sync_all()?;
        drop(file);
        #[cfg(any(test, feature = "test-support"))]
        if let Some(hook) = crate::idents::hooks::BEFORE_RENAME.with(|h| h.borrow_mut().take()) {
            hook(path);
        }
        match expected {
            None => std::fs::rename(&temp, path)?,
            // A new name: published without replacing anything (whoever
            // put a file there first keeps it).
            Some(None) => {
                if let Err(error) = crate::atomic_file::move_no_replace(&temp, path) {
                    if std::fs::symlink_metadata(path).is_ok() {
                        changed.set(true);
                        anyhow::bail!(ChangedMeanwhile(path.to_path_buf()));
                    }
                    return Err(error.into());
                }
            }
            Some(Some(_)) => {
                if header_line(path).ok().as_ref() != expected {
                    changed.set(true);
                    anyhow::bail!(ChangedMeanwhile(path.to_path_buf()));
                }
                std::fs::rename(&temp, path)?;
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    if changed.get() {
        return Err(ChangedMeanwhile(path.to_path_buf()).into());
    }
    result.with_context(|| format!("Não foi possível gravar {}.", path.display()))?;
    Ok(header)
}

/// Whether two configured paths are one file (same folder and name, case
/// apart), or one is the other's file of format 9: such a list would let a
/// copy be read twice or a 9 be taken for an 8. Names the pair.
pub fn aliases(paths: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    let key = |path: &Path| -> String {
        let parent = path
            .parent()
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
            .unwrap_or_default();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        format!("{}\u{0}{name}", parent.display().to_string().to_lowercase())
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (index, path) in paths.iter().enumerate() {
        for one in [key(path), key(&nine_path(path))] {
            match seen.get(&one) {
                Some(other) if *other != index => {
                    return Some((paths[*other].clone(), path.clone()));
                }
                _ => {
                    seen.insert(one, index);
                }
            }
        }
    }
    None
}

/// The words cloud clients put in the name of a conflict copy.
const CONFLICT_WORDS: [&str; 4] = [
    "cópia em conflito",
    "copia em conflito",
    "conflicted copy",
    "copia en conflicto",
];

/// Whether `candidate` (a bare file name) is a copy that a cloud client made
/// of `real` (`ponte-sync (1).ponte`, `… (cópia em conflito …).ponte`,
/// `Cópia de ponte-sync.ponte`): the same rule as the folder check of 0.51,
/// conservative (the Ponte's own files and other names never match).
pub fn is_conflict_copy(real: &str, candidate: &str) -> bool {
    let split = |name: &str| -> Option<(String, String)> {
        let lower = name.to_lowercase();
        let dot = lower.rfind('.')?;
        Some((lower[..dot].to_owned(), lower[dot + 1..].to_owned()))
    };
    let (Some((real_stem, real_ext)), Some((stem, ext))) = (split(real), split(candidate)) else {
        return false;
    };
    if ext != real_ext || candidate.eq_ignore_ascii_case(real) || candidate.starts_with('.') {
        return false;
    }
    for prefix in ["cópia de ", "copia de ", "copy of "] {
        if stem.strip_prefix(prefix) == Some(real_stem.as_str()) {
            return true;
        }
    }
    let Some(rest) = stem.strip_prefix(real_stem.as_str()) else {
        return false;
    };
    if rest.is_empty() || rest.starts_with('-') || rest.contains(".tmp-") {
        return false;
    }
    let numbered = rest
        .strip_prefix(" (")
        .and_then(|r| r.strip_suffix(')'))
        .is_some_and(|inner| !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()));
    numbered || CONFLICT_WORDS.iter().any(|word| rest.contains(word))
}

/// The conflict copies (see [`is_conflict_copy`]) of `path` beside it.
pub fn conflict_copies_of(path: &Path) -> Vec<String> {
    let (Some(folder), Some(real)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return Vec::new();
    };
    let mut found: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| is_conflict_copy(real, name))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn pack(payload: &Payload, blobs: &Blobs) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(payload)?;
    let mut out = Vec::with_capacity(json.len() + 1024);
    out.push(CONTAINER_VERSION);
    put_u64(&mut out, json.len() as u64);
    out.extend_from_slice(&json);
    for section in [&blobs.pdfs, &blobs.attachments] {
        put_u64(&mut out, section.len() as u64);
        for (id, bytes) in section {
            put_i64(&mut out, *id);
            put_u64(&mut out, bytes.len() as u64);
            out.extend_from_slice(bytes);
        }
    }
    Ok(out)
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        ensure!(
            self.at + len <= self.data.len(),
            "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo."
        );
        let slice = &self.data[self.at..self.at + len];
        self.at += len;
        Ok(slice)
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }
}

pub(crate) fn unpack(plain: &[u8]) -> Result<(Payload, Blobs)> {
    let mut cursor = Cursor { data: plain, at: 0 };
    ensure!(
        cursor.take(1)?[0] == CONTAINER_VERSION,
        "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo."
    );
    let json_len = cursor.u64()? as usize;
    let payload: Payload = serde_json::from_slice(cursor.take(json_len)?)
        .context("O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.")?;
    let mut blobs = Blobs::default();
    for section in [&mut blobs.pdfs, &mut blobs.attachments] {
        let count = cursor.u64()? as usize;
        ensure!(count <= 100_000, "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.");
        for _ in 0..count {
            let id = cursor.i64()?;
            let len = cursor.u64()? as usize;
            section.insert(id, cursor.take(len)?.to_vec());
        }
    }
    ensure!(
        cursor.at == plain.len(),
        "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo."
    );
    Ok((payload, blobs))
}

fn unpack_stub(plain: &[u8]) -> Result<StubBody> {
    let mut cursor = Cursor { data: plain, at: 0 };
    ensure!(cursor.take(1)?[0] == CONTAINER_VERSION, DAMAGED);
    let json_len = cursor.u64()? as usize;
    let stub: StubBody = serde_json::from_slice(cursor.take(json_len)?).context(DAMAGED)?;
    ensure!(
        cursor.u64()? == 0 && cursor.u64()? == 0 && cursor.at == plain.len(),
        DAMAGED
    );
    Ok(stub)
}

/// Writes the copy at `path` again at format 9 and `level`, as a Ponte of
/// that level would have (tests of F3: a file of F2b is a 9 at level 1),
/// after `edit` changed its header or payload. The moment, writer and
/// revision stay those of the header unless `edit` changes them; the salt
/// and nonce are new, so the plain start of the file changes.
#[cfg(any(test, feature = "test-support"))]
pub fn reseal_for_test(
    path: &Path,
    password: &str,
    level: u32,
    edit: impl FnOnce(&mut Header, &mut Payload),
) -> Result<()> {
    let Some(Found::Copy(mut copy)) = read_any(path, password)? else {
        anyhow::bail!("{} não é uma cópia.", path.display());
    };
    edit(&mut copy.header, &mut copy.payload);
    let header = copy.header.clone();
    write_shaped(
        path,
        password,
        &WriteRequest {
            lineage: &header.lineage,
            revision: header.revision,
            writer: &header.writer,
            writer_name: &header.writer_name,
            schema_version: header.schema_version,
        },
        pack(&copy.payload, &copy.blobs)?,
        &Shape {
            format: FORMAT_9,
            level,
            points_to: "",
        },
        &header.written_at,
        None,
    )?;
    Ok(())
}

/// Writes the stub at `path` again at `level`, pointing where it points
/// (tests of F3: the stub an F2b promotion left is at level 1).
#[cfg(test)]
pub(crate) fn reseal_stub_for_test(path: &Path, password: &str, level: u32) -> Result<()> {
    let Some(Found::Moved { header, moved_to }) = read_any(path, password)? else {
        anyhow::bail!("{} não é um aviso do formato novo.", path.display());
    };
    let json = serde_json::to_vec(&StubBody {
        moved_to: moved_to.clone(),
    })?;
    let mut body = Vec::with_capacity(json.len() + 32);
    body.push(CONTAINER_VERSION);
    put_u64(&mut body, json.len() as u64);
    body.extend_from_slice(&json);
    put_u64(&mut body, 0);
    put_u64(&mut body, 0);
    write_shaped(
        path,
        password,
        &WriteRequest {
            lineage: &header.lineage,
            revision: header.revision,
            writer: &header.writer,
            writer_name: &header.writer_name,
            schema_version: header.schema_version,
        },
        body,
        &Shape {
            format: FORMAT_9,
            level,
            points_to: &moved_to,
        },
        &header.written_at,
        None,
    )?;
    Ok(())
}

/// The check of [`read_header_up_to`] as a build that reads formats up to
/// `newest` and levels up to `level` makes it (a Ponte of F2b: 9 and 1).
#[cfg(test)]
pub(crate) fn read_header_as(path: &Path, newest: u32, level: u32) -> Result<Option<Header>> {
    let Some(line) = header_line(path)? else {
        return Ok(None);
    };
    let header: Header = serde_json::from_slice(&line[MAGIC.len()..]).context(DAMAGED)?;
    ensure!(
        (1..=newest).contains(&header.format) && header.level <= level,
        "O arquivo de sincronização foi gravado por uma versão mais nova do Ponte. Atualize o Ponte."
    );
    Ok(Some(header))
}

#[cfg(test)]
thread_local! {
    static JOURNAL_CAP: std::cell::Cell<usize> =
        const { std::cell::Cell::new(MAX_JOURNAL_ENTRIES) };
}

/// A test that needs old entries to leave the journal without writing five
/// hundred changes lowers the cap, for its own thread only.
#[cfg(test)]
pub(crate) fn set_journal_cap_for_test(entries: usize) {
    JOURNAL_CAP.with(|cap| cap.set(entries));
}

fn journal_cap() -> usize {
    #[cfg(test)]
    {
        JOURNAL_CAP.with(|cap| cap.get())
    }
    #[cfg(not(test))]
    {
        MAX_JOURNAL_ENTRIES
    }
}

/// Keeps the journal within retention: newest entries first by revision.
pub fn trim_journal(journal: &mut Vec<JournalEntry>) {
    journal.sort_by(|a, b| (a.revision, &a.created_at).cmp(&(b.revision, &b.created_at)));
    while journal.len() > journal_cap() {
        journal.remove(0);
    }
    loop {
        let bytes: usize = journal
            .iter()
            .map(|e| serde_json::to_vec(e).map(|v| v.len()).unwrap_or(0))
            .sum();
        if bytes <= MAX_JOURNAL_BYTES || journal.is_empty() {
            break;
        }
        journal.remove(0);
    }
}

/// Human description of a copy for the window.
pub fn describe(header: &Header) -> String {
    format!(
        "revisão {} • gravada por {} em {}",
        header.revision,
        if header.writer_name.is_empty() {
            "?"
        } else {
            &header.writer_name
        },
        crate::labels::datetime(&header.written_at)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snapshot() -> CabinetSnapshot {
        serde_json::from_str(
            r#"{"revision":3,"documents":[],"captures":[],"requests":[],"notes":[],"drafts":[],"analyses":[],"tasks":[],"work_contexts":[],"files_meta":[],"page_texts":[],"document_links":[],"trash":[],"capture_refs":[],"item_versions":[],"team_events":[]}"#,
        )
        .unwrap()
    }

    #[test]
    fn transport_file_round_trips_blobs_and_refuses_wrong_password_truncation_and_future_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Drive").join("ponte-sync.ponte");
        let mut blobs = Blobs::default();
        blobs.pdfs.insert(7, b"%PDF-1.4 conteudo".to_vec());
        blobs.attachments.insert(2, vec![1, 2, 3, 250]);
        let payload = Payload {
            snapshot: empty_snapshot(),
            applied: vec!["a".into(), "b".into()],
            journal: vec![],
            refs_since: None,
        };
        let request = WriteRequest {
            lineage: "lin-1",
            revision: 3,
            writer: "pc-a",
            writer_name: "Casa",
            schema_version: 10,
        };
        let header = write(&path, "senha-forte-123", &request, &payload, &blobs).unwrap();
        assert_eq!(header.revision, 3);
        assert_eq!(read_header(&path).unwrap().unwrap(), header);
        assert!(
            dir.path().join("Drive").read_dir().unwrap().count() == 1,
            "no temp file left"
        );

        let copy = read(&path, "senha-forte-123").unwrap().unwrap();
        assert_eq!(copy.payload.applied, ["a", "b"]);
        assert_eq!(copy.blobs.pdfs[&7], b"%PDF-1.4 conteudo");
        assert_eq!(copy.blobs.attachments[&2], vec![1, 2, 3, 250]);
        assert_eq!(copy.payload.snapshot.revision, 3);

        // The plain file never shows content; the wrong password is refused.
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(8).any(|w| w == b"conteudo"));
        let error = read(&path, "outra-senha-123").err().unwrap().to_string();
        assert!(error.contains("Senha"), "{error}");
        assert!(read(&dir.path().join("nada.ponte"), "x").unwrap().is_none());

        // Truncated or tampered copies are refused, never half-applied.
        std::fs::write(&path, &raw[..raw.len() - 5]).unwrap();
        assert!(read(&path, "senha-forte-123").is_err());
        // The copy was written with format 8 (projects).
        assert_eq!(read_header(&path).unwrap().unwrap().format, 8);
        // Copies written by format 1 (before tasks without document), format
        // 2 (before recursos), format 3 (before the proposals), format 4
        // (before the categories), format 5 (before the planning fields),
        // format 6 (before the library settings) and format 7 (before the
        // projects) are still read.
        for older_format in [1, 2, 3, 4, 5, 6, 7] {
            let older = String::from_utf8_lossy(&raw)
                .replace("\"format\":8", &format!("\"format\":{older_format}"));
            std::fs::write(&path, older.as_bytes()).unwrap();
            assert_eq!(read_header(&path).unwrap().unwrap().format, older_format);
        }
        // A Ponte before the projects (it knows formats up to 7) refuses the
        // file this one writes with the "update" message, and leaves it alone.
        std::fs::write(&path, &raw).unwrap();
        let error = read_header_up_to(&path, 7).unwrap_err().to_string();
        assert!(error.contains("versão mais nova") && error.contains("Atualize"));
        assert_eq!(std::fs::read(&path).unwrap(), raw);
        assert_eq!(read_header_up_to(&path, 8).unwrap().unwrap().format, 8);
        // And this one refuses what a later format writes (format 9 is read
        // since F2b; its stub too, see `nine_files_and_stubs`).
        for newer_format in [10, 11] {
            let newer = String::from_utf8_lossy(&raw)
                .replace("\"format\":8", &format!("\"format\":{newer_format}"));
            std::fs::write(&path, newer.as_bytes()).unwrap();
            assert!(read_header(&path)
                .unwrap_err()
                .to_string()
                .contains("Atualize"));
            assert_eq!(std::fs::read(&path).unwrap(), newer.as_bytes());
        }
        assert!(
            validate_password("curta").is_err() && validate_password("senha-forte-123").is_ok()
        );
    }

    #[test]
    fn nine_files_and_stubs() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("ponte-sync.ponte");
        let nine = nine_path(&old);
        assert_eq!(nine, dir.path().join("ponte-sync-formato9.ponte"));
        let payload = Payload {
            snapshot: empty_snapshot(),
            applied: vec![],
            journal: vec![],
            refs_since: None,
        };
        let request = WriteRequest {
            lineage: "lin-1",
            revision: 3,
            writer: "pc-a",
            writer_name: "Casa",
            schema_version: 22,
        };
        let header = write_nine(
            &nine,
            "senha-forte-123",
            &request,
            &payload,
            &Blobs::default(),
        )
        .unwrap();
        assert_eq!((header.format, header.level), (FORMAT_9, LEVEL));
        write_stub(
            &old,
            "senha-forte-123",
            &request,
            "ponte-sync-formato9.ponte",
            None,
        )
        .unwrap();
        match read_any(&old, "senha-forte-123").unwrap().unwrap() {
            Found::Moved { header, moved_to } => {
                assert_eq!(moved_to, "ponte-sync-formato9.ponte");
                assert_eq!(header.points_to, moved_to);
            }
            Found::Copy(_) => panic!("a stub"),
        }
        assert!(read(&old, "senha-forte-123")
            .err()
            .unwrap()
            .to_string()
            .contains("ponte-sync-formato9.ponte"));
        // A reader of 0.51 (formats up to 8) refuses both, and so does a
        // reader of a lower level.
        for path in [&old, &nine] {
            let error = read_header_up_to(path, 8).unwrap_err().to_string();
            assert!(error.contains("Atualize"), "{error}");
        }
        // The stub never points outside its folder.
        assert!(write_stub(&old, "senha-forte-123", &request, "../x.ponte", None).is_err());
        // A writer that finds the destination changed leaves it alone.
        let before = std::fs::read(&nine).unwrap();
        let error = write_checked(
            &nine,
            "senha-forte-123",
            &request,
            &payload,
            &Blobs::default(),
            FORMAT_9,
            Some(&None),
            None,
        )
        .unwrap_err();
        assert!(error.is::<ChangedMeanwhile>());
        assert_eq!(std::fs::read(&nine).unwrap(), before);
        assert!(
            dir.path().read_dir().unwrap().count() == 2,
            "no temp file left"
        );
    }

    #[test]
    fn aliases_and_cloud_conflict_copies() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("ponte-sync.ponte");
        assert!(aliases(&[a.clone(), dir.path().join("PONTE-SYNC.ponte")]).is_some());
        assert!(aliases(&[a.clone(), dir.path().join("ponte-sync-formato9.ponte")]).is_some());
        assert!(aliases(&[a.clone(), dir.path().join("outro.ponte")]).is_none());
        for name in [
            "ponte-sync (1).ponte",
            "ponte-sync (cópia em conflito de Maria).ponte",
            "Cópia de ponte-sync.ponte",
        ] {
            assert!(is_conflict_copy("ponte-sync.ponte", name), "{name}");
        }
        for name in [
            "ponte-sync.ponte",
            "ponte-sync-formato9.ponte",
            "ponte-sync-guarda.ponte",
            "ponte-sync-divergente-a1b2c3d4e5f60789.ponte",
            "outro (1).ponte",
        ] {
            assert!(!is_conflict_copy("ponte-sync.ponte", name), "{name}");
        }
    }
}
