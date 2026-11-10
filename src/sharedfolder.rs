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

pub const FORMAT: u32 = 1;
const MAGIC: &[u8] = b"PONTE-SYNC1\n";
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
}

/// Encrypted part of the file.
#[derive(Serialize, Deserialize)]
pub struct Payload {
    pub snapshot: CabinetSnapshot,
    /// `uid`s of every queued change incorporated in this lineage.
    pub applied: Vec<String>,
    pub journal: Vec<JournalEntry>,
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

fn random(bytes: usize) -> Result<Vec<u8>> {
    use ring::rand::SecureRandom;
    let mut out = vec![0_u8; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut out)
        .map_err(|_| anyhow::anyhow!("Não foi possível proteger o arquivo de sincronização."))?;
    Ok(out)
}

fn derive_key(password: &str, salt: &[u8]) -> [u8; KEY_BYTES] {
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
        magic == MAGIC,
        "{} não é um arquivo de sincronização do Ponte.",
        path.display()
    );
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        file.read_exact(&mut byte)
            .context("O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.")?;
        if byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
        ensure!(
            line.len() <= 64 * 1024,
            "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo."
        );
    }
    let header: Header =
        serde_json::from_slice(&line).context("O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.")?;
    ensure!(
        header.format == FORMAT,
        "O arquivo de sincronização foi gravado por uma versão mais nova do Ponte. Atualize o Ponte."
    );
    Ok(Some(header))
}

/// Reads and decrypts a copy. `None` when the file does not exist.
pub fn read(path: &Path, password: &str) -> Result<Option<Copy>> {
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
            .context("O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.")?;
    let header_bytes = bytes[MAGIC.len()..header_end].to_vec();
    let mut body = bytes[header_end + 1..].to_vec();
    let salt = STANDARD
        .decode(&header.salt)
        .context("O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.")?;
    let nonce = STANDARD
        .decode(&header.nonce)
        .context("O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.")?;
    ensure!(nonce.len() == NONCE_BYTES, "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.");
    let key = ring::aead::LessSafeKey::new(
        ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &derive_key(password, &salt))
            .map_err(|_| anyhow::anyhow!("Senha de sincronização inválida."))?,
    );
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce)
        .map_err(|_| anyhow::anyhow!("O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo."))?;
    let plain = key
        .open_in_place(nonce, ring::aead::Aad::from(&header_bytes), &mut body)
        .map_err(|_| anyhow::anyhow!("Senha de sincronização incorreta, ou arquivo danificado ou ainda sendo gravado pela nuvem. Confira a senha e espere a pasta terminar de sincronizar."))?;
    let (payload, blobs) = unpack(plain)?;
    Ok(Some(Copy {
        path: path.to_path_buf(),
        header,
        payload,
        blobs,
    }))
}

/// Everything the caller decides about a new copy.
pub struct WriteRequest<'a> {
    pub lineage: &'a str,
    pub revision: i64,
    pub writer: &'a str,
    pub writer_name: &'a str,
    pub schema_version: i64,
}

/// Encrypts and writes atomically (temporary file, then rename), so a cloud
/// client never sees a half-written copy. Returns the header written.
pub fn write(
    path: &Path,
    password: &str,
    request: &WriteRequest<'_>,
    payload: &Payload,
    blobs: &Blobs,
) -> Result<Header> {
    ensure!(!password.is_empty(), "Informe a senha de sincronização.");
    let salt = random(SALT_BYTES)?;
    let nonce_bytes = random(NONCE_BYTES)?;
    let header = Header {
        format: FORMAT,
        schema_version: request.schema_version,
        lineage: request.lineage.to_owned(),
        revision: request.revision,
        writer: request.writer.to_owned(),
        writer_name: request.writer_name.to_owned(),
        written_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        salt: STANDARD.encode(&salt),
        nonce: STANDARD.encode(&nonce_bytes),
    };
    let header_bytes = serde_json::to_vec(&header)?;
    ensure!(!header_bytes.contains(&b'\n'), "O arquivo de sincronização está incompleto ou danificado. Espere a nuvem terminar de sincronizar e tente de novo.");
    let mut body = pack(payload, blobs)?;
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
    let result = (|| -> Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(MAGIC)?;
        file.write_all(&header_bytes)?;
        file.write_all(b"\n")?;
        file.write_all(&body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.with_context(|| format!("Não foi possível gravar {}.", path.display()))?;
    Ok(header)
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn pack(payload: &Payload, blobs: &Blobs) -> Result<Vec<u8>> {
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

fn unpack(plain: &[u8]) -> Result<(Payload, Blobs)> {
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

/// Keeps the journal within retention: newest entries first by revision.
pub fn trim_journal(journal: &mut Vec<JournalEntry>) {
    journal.sort_by(|a, b| (a.revision, &a.created_at).cmp(&(b.revision, &b.created_at)));
    while journal.len() > MAX_JOURNAL_ENTRIES {
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
        let mut future = raw.clone();
        let text = String::from_utf8_lossy(&future).replace("\"format\":1", "\"format\":9");
        future = text.into_bytes();
        std::fs::write(&path, &future).unwrap();
        assert!(read_header(&path)
            .unwrap_err()
            .to_string()
            .contains("Atualize"));
        assert!(
            validate_password("curta").is_err() && validate_password("senha-forte-123").is_ok()
        );
    }
}
