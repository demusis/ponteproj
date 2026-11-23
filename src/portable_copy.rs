//! "Cópia portátil (com senha)": the library in one file, `.pontecopia`, that
//! opens on any computer with the password the person chose.
//!
//! The content is the one the shared folder carries (the snapshot of every
//! item, plus the bytes of the PDFs and attachments, packed by
//! `sharedfolder::pack`), encrypted the same way (PBKDF2-HMAC-SHA256, then
//! AES-256-GCM authenticating the header), but in a file of its own kind:
//!
//! ```text
//! PONTE-COPIA1\n
//! {"formato":1,"esquema":21,…,"sal":"…","nonce":"…"}\n
//! <AES-256-GCM of the packed content>
//! ```
//!
//! No sync reader accepts it (they all want `PONTE-SYNC1\n`, and say what this
//! file is when they meet it; the phone compiles the same reader), and this
//! reader refuses a sync file and a local copy, saying what to do instead.
//!
//! Exporting estimates the size before reading anything into memory and
//! refuses early above [`MAX_LIBRARY_BYTES`]; the file is written aside and
//! published under its final name without ever replacing a file. Importing
//! reads the file once (bounded), authenticates it with the password and
//! unpacks it before anything is created; only then a recovery library is
//! built from it in a new, clean data folder (`recovery`), detached from any
//! synchronization, to look at and then adopt.
use crate::intranet::CabinetSnapshot;
use crate::recovery::{self, Built, Place};
use crate::sharedfolder::{self, Payload, PORTABLE_MAGIC};
use crate::storage::Store;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The format of the portable copy this build writes and reads.
pub const FORMAT: u32 = 1;
/// The extension of the file.
pub const EXTENSION: &str = "pontecopia";
/// The largest file read or written (the same bound as the shared folder's).
pub const MAX_FILE_BYTES: u64 = sharedfolder::MAX_FILE_BYTES;
/// The largest library exported, measured as the bytes of every value the
/// export carries (sealed as stored, so a little more than the content), in
/// the very read snapshot that is exported, before anything is read into
/// memory. The export holds the content about three times at its peak (the
/// items, their serialized form and the packed, encrypted body; the file is
/// then written from that body): 200 MB of content keeps the peak near
/// 600 MB.
pub const MAX_LIBRARY_BYTES: u64 = 200 * 1024 * 1024;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;

/// What travels besides the items, in the portable copy's own section (never
/// in a sync file).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Extras {
    /// The completed import receipts: a retry of an import already completed
    /// (from the browser, from WhatsApp) never brings back what the person
    /// deleted for good afterwards.
    #[serde(default)]
    recibos_de_importacao: Vec<(String, String, String)>,
}

/// What does not travel in a portable copy, as the card says it.
pub const NOT_CARRIED: &[&str] = &[
    "a chave e os modelos da IA e a autorização de envio à IA",
    "as contas de e-mail e as senhas guardadas",
    "a configuração de sincronização (pasta compartilhada, senha da pasta, computador principal e sua chave), as alterações que esperavam envio e os conflitos",
    "os lembretes deste computador e o registro dos avisos já dados",
    "a aparência (tema e tamanho do texto)",
    "os resultados guardados da conferência das minutas",
    "as preferências locais: prazo da Lixeira, atendimentos automáticos e os modelos pessoais antigos",
];

/// The plain part of the file (authenticated with the content).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub formato: u32,
    /// The library schema of the computer that wrote it.
    pub esquema: i64,
    /// When it was written (UTC, RFC 3339).
    pub gravada_em: String,
    /// Who wrote it (the member name of that computer).
    pub gravada_por: String,
    pub documentos: usize,
    pub sal: String,
    pub nonce: String,
}

/// What an export wrote.
#[derive(Debug)]
pub struct Exported {
    pub path: PathBuf,
    pub bytes: u64,
    pub documents: usize,
}

pub fn validate_password(password: &str) -> Result<()> {
    ensure!(
        password.chars().count() >= 8
            && password.len() <= 256
            && !password.chars().any(char::is_control),
        "A senha da cópia portátil precisa ter pelo menos 8 caracteres (e no máximo 256 bytes, sem caracteres de controle)."
    );
    Ok(())
}

/// The bytes of every value the export carries, read inside the caller's
/// read transaction (the same state that is then exported).
fn content_bytes(conn: &rusqlite::Connection) -> Result<u64> {
    let mut total = 0u64;
    let tables = crate::intranet::LIBRARY_INVENTORY
        .iter()
        .map(|(_, table, ..)| *table)
        .chain(["item_versions", "team_events", "import_receipts"]);
    for table in tables {
        let columns: Vec<String> = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        if columns.is_empty() {
            continue;
        }
        let sum = columns
            .iter()
            .map(|c| format!("coalesce(sum(length(\"{c}\")),0)"))
            .collect::<Vec<_>>()
            .join("+");
        let bytes: i64 = conn.query_row(&format!("SELECT {sum} FROM {table}"), [], |r| r.get(0))?;
        total += bytes.max(0) as u64;
    }
    Ok(total)
}

fn too_big(bytes: u64, limit: u64) -> String {
    format!(
        "O acervo tem cerca de {} MB de conteúdo, mais do que cabe numa cópia portátil (até {} MB). Nada foi gravado. Use «Fazer cópia agora» (que só abre nesta conta do Windows) ou leve para a Lixeira e esvazie PDFs e anexos de que não precisa mais.",
        bytes.div_ceil(1024 * 1024),
        limit / (1024 * 1024)
    )
}

/// Every task of the library travels, read strictly: the snapshot leaves out
/// a task it cannot read when its document is in the Lixeira (the tolerant
/// reading of the synchronization); a portable copy refuses instead, naming
/// it. Read in the export's own transaction.
fn ensure_every_task_travels(store: &Store, snapshot: &CabinetSnapshot) -> Result<()> {
    let exported: std::collections::BTreeSet<i64> = snapshot.tasks.iter().map(|t| t.id).collect();
    let all: Vec<i64> = store
        .conn
        .prepare("SELECT id FROM tasks ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if let Some(missing) = all.iter().find(|id| !exported.contains(id)) {
        bail!(
            "A tarefa nº {missing} não pode ser lida (o conteúdo está danificado ou foi gravado em outra conta do Windows). A cópia portátil não foi gravada, para não perder essa tarefa sem avisar. Restaure o documento dela da Lixeira e corrija ou exclua a tarefa, ou use «Fazer cópia agora»."
        );
    }
    Ok(())
}

/// Every item of the Lixeira must travel with the item it holds: one that
/// could not be read (the snapshot leaves it out) refuses the export instead
/// of a copy that silently loses it.
fn ensure_trash_is_whole(snapshot: &CabinetSnapshot) -> Result<()> {
    for row in &snapshot.trash {
        let (present, what) = match row.kind.as_str() {
            "document" => (
                snapshot.documents.iter().any(|d| d.id == row.item_id),
                "o documento",
            ),
            "capture" => (
                snapshot.captures.iter().any(|c| c.id == row.item_id),
                "a versão de documento",
            ),
            "task" => (
                snapshot.tasks.iter().any(|t| t.id == row.item_id),
                "a tarefa",
            ),
            "attachment" => (
                snapshot.attachments.iter().any(|a| a.id == row.item_id),
                "o anexo",
            ),
            _ => (true, ""),
        };
        ensure!(
            present,
            "A Lixeira tem {what} nº {} que não pode ser lido (o conteúdo está danificado ou foi gravado em outra conta do Windows). A cópia portátil não foi gravada, para não perder esse item sem avisar. Exclua esse item da Lixeira de vez (Ajustes ▸ Lixeira) e tente de novo, ou use «Fazer cópia agora».",
            row.item_id
        );
    }
    Ok(())
}

/// The header line and the encrypted body of the file (magic, header line,
/// body). The body is `[u64 n][n bytes of the packed items][u64 m][m bytes of
/// the extras]`, encrypted in place: no second copy of it is made.
fn seal_file(
    password: &str,
    esquema: i64,
    gravada_por: &str,
    gravada_em: &str,
    payload: &Payload,
    blobs: &sharedfolder::Blobs,
    extras: &Extras,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let salt = sharedfolder::random(SALT_BYTES)?;
    let nonce_bytes = sharedfolder::random(NONCE_BYTES)?;
    let header = Header {
        formato: FORMAT,
        esquema,
        gravada_em: gravada_em.to_owned(),
        gravada_por: gravada_por.to_owned(),
        documentos: payload.snapshot.documents.len(),
        sal: STANDARD.encode(&salt),
        nonce: STANDARD.encode(&nonce_bytes),
    };
    let header_bytes = serde_json::to_vec(&header)?;
    ensure!(
        !header_bytes.contains(&b'\n'),
        "Não foi possível montar a cópia portátil."
    );
    let packed = sharedfolder::pack(payload, blobs)?;
    let extras = serde_json::to_vec(extras)?;
    let mut body = Vec::with_capacity(16 + packed.len() + extras.len() + 16);
    body.extend_from_slice(&(packed.len() as u64).to_le_bytes());
    body.extend_from_slice(&packed);
    drop(packed);
    body.extend_from_slice(&(extras.len() as u64).to_le_bytes());
    body.extend_from_slice(&extras);
    let key = ring::aead::LessSafeKey::new(
        ring::aead::UnboundKey::new(
            &ring::aead::AES_256_GCM,
            &sharedfolder::derive_key(password, &salt),
        )
        .map_err(|_| anyhow::anyhow!("Senha da cópia portátil inválida."))?,
    );
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce_bytes)
        .map_err(|_| anyhow::anyhow!("Não foi possível proteger a cópia portátil."))?;
    key.seal_in_place_append_tag(nonce, ring::aead::Aad::from(&header_bytes), &mut body)
        .map_err(|_| anyhow::anyhow!("Não foi possível proteger a cópia portátil."))?;
    Ok((header_bytes, body))
}

/// Writes `parts` one after the other in a new file at `path`, without ever
/// replacing a file: written to a temporary file of its own beside it,
/// flushed, then given its name by a move that fails when the name is taken
/// (`atomic_file::move_no_replace`). On any failure the temporary file goes
/// and nothing is left under the name.
fn write_new(path: &Path, parts: &[&[u8]]) -> Result<()> {
    let exists = || {
        anyhow::anyhow!(
            "Já existe um arquivo em {}; escolha outro nome (o Ponte nunca grava por cima de um arquivo).",
            path.display()
        )
    };
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(exists());
    }
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let temp = parent.join(format!(
        ".ponte-copia-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        for part in parts {
            file.write_all(part)?;
        }
        file.sync_all()
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(error).with_context(|| {
            format!(
                "Não foi possível gravar a cópia portátil em {} (o disco tem espaço e permite gravar?).",
                parent.display()
            )
        });
    }
    match crate::atomic_file::move_no_replace(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            if error.kind() == std::io::ErrorKind::AlreadyExists
                || std::fs::symlink_metadata(path).is_ok()
            {
                Err(exists())
            } else {
                Err(error).with_context(|| {
                    format!(
                        "Não foi possível dar o nome final à cópia portátil ({}).",
                        path.display()
                    )
                })
            }
        }
    }
}

/// "Exportar cópia portátil…": the library at `database` in a new file at
/// `path`, encrypted with `password`. The library is only read, in one read
/// transaction: the size is measured in it before anything is read into
/// memory, and the snapshot, the bytes of the files and the import receipts
/// all come from that same state.
pub fn export(database: &Path, path: &Path, password: &str) -> Result<Exported> {
    export_limited(database, path, password, MAX_LIBRARY_BYTES, &|| {})
}

/// [`export`] with the limit given and a step run right after the size was
/// measured (the tests commit a change there, from another connection).
fn export_limited(
    database: &Path,
    path: &Path,
    password: &str,
    limit: u64,
    after_measure: &dyn Fn(),
) -> Result<Exported> {
    validate_password(password)?;
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION)),
        "O nome da cópia portátil precisa terminar em .{EXTENSION}."
    );
    if std::fs::symlink_metadata(path).is_ok() {
        bail!(
            "Já existe um arquivo em {}; escolha outro nome (o Ponte nunca grava por cima de um arquivo).",
            path.display()
        );
    }
    let store = Store::open_service(database)?;
    let member = store
        .load_intranet_config()
        .map(|c| c.member_name)
        .unwrap_or_default();
    let read = store.conn.unchecked_transaction()?;
    // The first read of the transaction: from here on it sees one state.
    let measured = content_bytes(&store.conn)?;
    after_measure();
    ensure!(measured <= limit, "{}", too_big(measured, limit));
    // A library of schema 22 sends the identity of every item and the epoch
    // of its identities along (F2b): the library made from the copy keeps
    // them.
    let snapshot = if crate::idents::has_idents(&store.conn)? {
        let epoch = crate::idents::origin(&store.conn)?.2.unwrap_or_default();
        store.export_with_idents(&epoch)?
    } else {
        store.export_snapshot()?
    };
    ensure_trash_is_whole(&snapshot)?;
    ensure_every_task_travels(&store, &snapshot)?;
    let blobs = crate::intranet::collect_blobs(&store, &snapshot)?;
    let extras = Extras {
        recibos_de_importacao: store
            .conn
            .prepare(
                "SELECT request_id, manifest, completed_at FROM import_receipts WHERE state='done' ORDER BY request_id",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?,
    };
    drop(read);
    let documents = snapshot.documents.len();
    let payload = Payload {
        snapshot,
        applied: Vec::new(),
        journal: Vec::new(),
        refs_since: None,
    };
    let written_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let (header, body) = seal_file(
        password,
        crate::storage::SCHEMA_VERSION,
        &member,
        &written_at,
        &payload,
        &blobs,
        &extras,
    )?;
    drop((payload, blobs));
    let total = (PORTABLE_MAGIC.len() + header.len() + 1 + body.len()) as u64;
    ensure!(
        total <= MAX_FILE_BYTES,
        "{}",
        too_big(total, MAX_FILE_BYTES)
    );
    write_new(path, &[PORTABLE_MAGIC, &header, b"\n", &body])?;
    Ok(Exported {
        path: path.to_path_buf(),
        bytes: total,
        documents,
    })
}

/// An authenticated, unpacked portable copy (nothing written anywhere yet).
pub struct Opened {
    pub header: Header,
    pub snapshot: CabinetSnapshot,
    pub blobs: sharedfolder::Blobs,
    extras: Extras,
}

const DAMAGED: &str = "A cópia portátil está incompleta ou danificada (o arquivo terminou de ser copiado?). Nada foi criado.";

/// Reads the file once, from one handle, at most [`MAX_FILE_BYTES`], and
/// opens it with `password`. Refuses what is not a portable copy, saying what
/// it is. Writes nothing.
pub fn open(path: &Path, password: &str) -> Result<Opened> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
        .with_context(|| format!("Não foi possível ler {}.", path.display()))?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE_BYTES,
        "A cópia portátil passa de {} MB e não pode ser aberta. Nada foi criado.",
        MAX_FILE_BYTES / (1024 * 1024)
    );
    if !bytes.starts_with(PORTABLE_MAGIC) {
        if bytes.starts_with(sharedfolder::SYNC_MAGIC) {
            bail!(recovery::SYNC_HINT);
        }
        if bytes.starts_with(b"SQLite format 3\0") {
            bail!("Este arquivo é uma cópia deste computador (.sqlite3), não uma cópia portátil. Use «Restaurar cópia…» em Ajustes ▸ Cópia de segurança. Nada foi criado.");
        }
        bail!(
            "{} não é uma cópia portátil do Ponte. Nada foi criado.",
            path.display()
        );
    }
    let rest = &bytes[PORTABLE_MAGIC.len()..];
    let line_end = rest
        .iter()
        .take(64 * 1024)
        .position(|b| *b == b'\n')
        .context(DAMAGED)?;
    let header_bytes = &rest[..line_end];
    let header: Header = serde_json::from_slice(header_bytes).context(DAMAGED)?;
    ensure!(
        header.formato == FORMAT,
        "Esta cópia portátil foi gravada por uma versão mais nova do Ponte. Atualize o Ponte e tente de novo. Nada foi criado."
    );
    ensure!(
        header.esquema <= crate::storage::SCHEMA_VERSION,
        "Esta cópia portátil foi feita por uma versão mais nova do Ponte (versão {} do acervo; este Ponte conhece até a {}). Atualize o Ponte e tente de novo. Nada foi criado.",
        header.esquema,
        crate::storage::SCHEMA_VERSION
    );
    let salt = STANDARD.decode(&header.sal).context(DAMAGED)?;
    let nonce = STANDARD.decode(&header.nonce).context(DAMAGED)?;
    ensure!(nonce.len() == NONCE_BYTES, DAMAGED);
    let header_bytes = header_bytes.to_vec();
    let body_start = PORTABLE_MAGIC.len() + line_end + 1;
    // The body is decrypted where it is: the file's own buffer, no copy.
    bytes.drain(..body_start);
    let key = ring::aead::LessSafeKey::new(
        ring::aead::UnboundKey::new(
            &ring::aead::AES_256_GCM,
            &sharedfolder::derive_key(password, &salt),
        )
        .map_err(|_| anyhow::anyhow!("Senha da cópia portátil inválida."))?,
    );
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(&nonce)
        .map_err(|_| anyhow::anyhow!(DAMAGED))?;
    let plain = key
        .open_in_place(nonce, ring::aead::Aad::from(&header_bytes), &mut bytes)
        .map_err(|_| {
            anyhow::anyhow!(
                "Senha incorreta, ou a cópia portátil está danificada. Confira a senha (maiúsculas e minúsculas contam). Nada foi criado."
            )
        })?;
    let section = |at: usize| -> Result<(usize, usize)> {
        let len = plain
            .get(at..at + 8)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes)
            .context(DAMAGED)?;
        let len = usize::try_from(len).map_err(|_| anyhow::anyhow!(DAMAGED))?;
        let end = (at + 8).checked_add(len).context(DAMAGED)?;
        ensure!(end <= plain.len(), DAMAGED);
        Ok((at + 8, end))
    };
    let (start, end) = section(0)?;
    let (extras_start, extras_end) = section(end)?;
    ensure!(extras_end == plain.len(), DAMAGED);
    let extras: Extras = serde_json::from_slice(&plain[extras_start..extras_end])
        .map_err(|_| anyhow::anyhow!(DAMAGED))?;
    let (payload, blobs) =
        sharedfolder::unpack(&plain[start..end]).map_err(|_| anyhow::anyhow!(DAMAGED))?;
    drop(bytes);
    // Every file the snapshot lists comes in the copy, once.
    for meta in &payload.snapshot.files_meta {
        ensure!(blobs.pdfs.contains_key(&meta.id), DAMAGED);
    }
    for row in &payload.snapshot.attachments {
        ensure!(blobs.attachments.contains_key(&row.id), DAMAGED);
    }
    ensure!(
        blobs.pdfs.len() == payload.snapshot.files_meta.len()
            && blobs.attachments.len() == payload.snapshot.attachments.len(),
        DAMAGED
    );
    Ok(Opened {
        header,
        snapshot: payload.snapshot,
        blobs,
        extras,
    })
}

/// "Importar cópia portátil…": authenticates and unpacks the file first,
/// then builds a recovery library from it in a new, clean data folder,
/// detached from any synchronization. A wrong password, a damaged file or
/// anything that is not a portable copy creates nothing.
pub fn import(path: &Path, password: &str, place: &Place, now: NaiveDateTime) -> Result<Built> {
    let opened = open(path, password)?;
    let copia_de = chrono::DateTime::parse_from_rfc3339(&opened.header.gravada_em)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .naive_local()
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default();
    if !opened.snapshot.idents.is_empty() {
        crate::idents::check_triples(&opened.snapshot)?;
    }
    recovery::build_in_new_folder(place, now, |folder| {
        let db = crate::paths::db_path_in(folder);
        {
            let mut store = Store::open(&db)?;
            let blobs = &opened.blobs;
            store.apply_snapshot_and(
                &opened.snapshot,
                |file_id, _| blobs.pdfs.get(&file_id).cloned().context(DAMAGED),
                |id| blobs.attachments.get(&id).cloned().context(DAMAGED),
                |tx| {
                    // The numbers of this library are its own reference.
                    crate::idmap::reset_frame(tx, true)?;
                    // Imports already completed stay completed.
                    for (request, manifest, completed) in &opened.extras.recibos_de_importacao {
                        tx.execute(
                            "INSERT OR IGNORE INTO import_receipts(request_id,manifest,completed_at,state) VALUES(?1,?2,?3,'done')",
                            rusqlite::params![request, manifest, completed],
                        )?;
                    }
                    Ok(())
                },
            )?;
        }
        let marker = recovery::finish_recovery_library(
            folder,
            &db,
            place,
            recovery::Source {
                tipo: "copia-portatil",
                origem: path,
                copia_de,
                esquema: opened.header.esquema,
            },
            now,
        )?;
        // The epoch the copy carried stays with its identities (the
        // recovery cleared the origin: the library is provisional).
        if !opened.snapshot.ident_epoch.is_empty() {
            let store = Store::open(&db)?;
            crate::idents::set_origin(
                &store.conn,
                "provisional",
                None,
                Some(&opened.snapshot.ident_epoch),
            )?;
        }
        Ok(marker)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intranet::{IntranetConfig, IntranetMode};

    const PASSWORD: &str = "senha longa da copia";

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

    fn folders(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A library with one of every kind of item the snapshot carries.
    fn full_library(db: &Path) -> Store {
        let mut store = Store::open(db).unwrap();
        let first = store
            .import_capture(&capture("SEFAZ-PRO-2026/00001"))
            .unwrap();
        let second = store
            .import_capture(&capture("SEFAZ-PRO-2026/00002"))
            .unwrap();
        // Note history: two saved versions of the note.
        store.save_note(first.document_id, "Primeira nota").unwrap();
        store.save_note(first.document_id, "Nota revista").unwrap();
        // Draft versions.
        store
            .save_draft(first.document_id, first.capture_id, "resposta", "Minuta 1")
            .unwrap();
        store
            .save_draft(first.document_id, first.capture_id, "resposta", "Minuta 2")
            .unwrap();
        // An attachment (bytes travel in the copy).
        store
            .add_attachment(
                crate::attachments::Owner::Document(first.document_id),
                "oficio.pdf",
                "cópia",
                b"%PDF-1.4 anexo sintetico",
            )
            .unwrap();
        // Tasks, one of them standalone.
        let mut task = crate::tasks::Task::new(first.document_id);
        task.title = "Responder o ofício".into();
        store.save_task(&task).unwrap();
        let mut alone = crate::tasks::Task::standalone();
        alone.title = "Tarefa sem documento".into();
        store.save_task(&alone).unwrap();
        // A project and a category.
        store
            .save_project(&crate::projects::Project::new("Projeto da reforma"))
            .unwrap();
        store
            .save_category(&crate::categories::Category::new("Urgente", 1))
            .unwrap();
        // Holidays (a library setting).
        store
            .set_library_setting("holidays", "uf MT\nanual 09-07 Independência\n")
            .unwrap();
        // A work template and an instruction block.
        let block = crate::work_templates::InstructionBlock::new(
            "Linguagem",
            "Linguagem e estilo",
            "Use frases curtas.",
        );
        store.save_block(&block, None).unwrap();
        let template = crate::work_templates::WorkTemplate::new(
            "Ofício de resposta",
            "resposta",
            "Ref. {{numero_documento}}",
        );
        store.save_template(&template, None).unwrap();
        // Trash contents: the second document goes to the Lixeira, with a
        // task of its own.
        let mut archived = crate::tasks::Task::new(second.document_id);
        archived.title = "Tarefa do documento arquivado".into();
        store.save_task(&archived).unwrap();
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Document,
                id: second.document_id,
            })
            .unwrap();
        store
    }

    #[test]
    fn a_round_trip_keeps_every_kind_of_item() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("PonteProj");
        std::fs::create_dir_all(&home).unwrap();
        let source = full_library(&crate::paths::db_path_in(&home));
        let before = source.export_snapshot().unwrap();
        drop(source);
        let file = dir.path().join("pendrive").join("acervo.pontecopia");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let exported = export(&crate::paths::db_path_in(&home), &file, PASSWORD).unwrap();
        assert_eq!(exported.documents, 2);
        assert!(std::fs::read(&file).unwrap().starts_with(PORTABLE_MAGIC));
        // Nothing of the content is in the clear.
        let raw = std::fs::read(&file).unwrap();
        assert!(!raw.windows(13).any(|w| w == b"Primeira nota"));

        // Another computer: an empty main folder.
        let other = dir.path().join("outro").join("PonteProj");
        std::fs::create_dir_all(&other).unwrap();
        let built = import(&file, PASSWORD, &Place::new(Some(&other), &other), now()).unwrap();
        assert_eq!(built.marker.tipo, "copia-portatil");
        let restored = Store::open(&crate::paths::db_path_in(&built.folder)).unwrap();
        let after = restored.export_snapshot().unwrap();
        // Every kind of item, count by count and content by content.
        macro_rules! same {
            ($($field:ident),*) => {$(
                assert_eq!(
                    serde_json::to_value(&after.$field).unwrap(),
                    serde_json::to_value(&before.$field).unwrap(),
                    stringify!($field)
                );
            )*};
        }
        same!(
            documents,
            captures,
            requests,
            notes,
            drafts,
            analyses,
            tasks,
            work_contexts,
            files_meta,
            page_texts,
            document_links,
            trash,
            capture_refs,
            attachments,
            saiop_batches,
            saiop_attendances,
            templates,
            resources,
            categories,
            settings,
            projects,
            item_versions,
            team_events
        );
        assert!(!before.trash.is_empty());
        assert!(!before.templates.templates.is_empty() && !before.templates.blocks.is_empty());
        assert!(before.notes.len() >= 2 && before.drafts.len() >= 2);
        assert!(!before.projects.is_empty() && !before.categories.is_empty());
        assert!(!before.settings.is_empty());
        assert_eq!(after.revision, before.revision);
        let (_, bytes) = restored.attachment_bytes(after.attachments[0].id).unwrap();
        assert_eq!(bytes, b"%PDF-1.4 anexo sintetico");
        // It is a recovery library: alone, nothing waiting.
        let cfg = restored.load_intranet_config().unwrap();
        assert_eq!(cfg.mode, IntranetMode::Local);
        assert!(restored.list_offline_outbox().unwrap().is_empty());
    }

    #[test]
    fn a_wrong_password_or_a_damaged_file_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("PonteProj");
        std::fs::create_dir_all(&home).unwrap();
        drop(full_library(&crate::paths::db_path_in(&home)));
        let file = dir.path().join("acervo.pontecopia");
        export(&crate::paths::db_path_in(&home), &file, PASSWORD).unwrap();
        let place = Place::new(Some(&home), &home);
        let before = folders(dir.path());
        let error = import(&file, "outra senha qualquer", &place, now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Senha incorreta"), "{error}");
        assert_eq!(folders(dir.path()), before);
        // Truncated, and with one byte changed.
        let bytes = std::fs::read(&file).unwrap();
        let cut = dir.path().join("cortada.pontecopia");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        let changed = dir.path().join("alterada.pontecopia");
        let mut other = bytes.clone();
        let middle = other.len() / 2;
        other[middle] ^= 1;
        std::fs::write(&changed, &other).unwrap();
        let before = folders(dir.path());
        for bad in [&cut, &changed] {
            assert!(import(bad, PASSWORD, &place, now()).is_err());
        }
        assert_eq!(folders(dir.path()), before);
    }

    #[test]
    fn the_sync_reader_and_the_portable_reader_refuse_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("PonteProj");
        std::fs::create_dir_all(&home).unwrap();
        drop(full_library(&crate::paths::db_path_in(&home)));
        let portable = dir.path().join("acervo.pontecopia");
        export(&crate::paths::db_path_in(&home), &portable, PASSWORD).unwrap();
        // The sync reader, under the portable file's own name and under the
        // name of a sync file, with the right password: refused, saying what
        // the file is.
        let renamed = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(renamed.parent().unwrap()).unwrap();
        std::fs::copy(&portable, &renamed).unwrap();
        for path in [&portable, &renamed] {
            let error = sharedfolder::read(path, PASSWORD)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("cópia portátil"), "{error}");
            assert!(sharedfolder::read_header(path).is_err());
        }
        // A folder computer pointed at it gets the same refusal and nothing changes.
        let mut folder_store = Store::open(&dir.path().join("pasta.sqlite3")).unwrap();
        folder_store
            .save_intranet_config(&IntranetConfig {
                mode: IntranetMode::SharedFolder,
                member_name: "Casa".into(),
                folder_files: vec![renamed.display().to_string()],
                folder_key: PASSWORD.into(),
                ..IntranetConfig::default()
            })
            .unwrap();
        let cfg = folder_store.load_intranet_config().unwrap();
        let error = folder_store
            .sync_with_folders(&cfg, PASSWORD, true, &crate::ai::Reporter::silent())
            .unwrap_err()
            .to_string();
        assert!(error.contains("cópia portátil"), "{error}");
        assert_eq!(
            std::fs::read(&renamed).unwrap(),
            std::fs::read(&portable).unwrap()
        );
        assert!(folder_store.list_documents("").unwrap().is_empty());

        // The portable reader refuses a sync file and a local copy.
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        let sync_file = dir.path().join("Drive2").join("ponte-sync.ponte");
        a.save_intranet_config(&IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: "A".into(),
            folder_files: vec![sync_file.display().to_string()],
            folder_key: PASSWORD.into(),
            ..IntranetConfig::default()
        })
        .unwrap();
        a.import_capture(&capture("SEFAZ-PRO-2026/00009")).unwrap();
        let cfg_a = a.load_intranet_config().unwrap();
        std::fs::create_dir_all(sync_file.parent().unwrap()).unwrap();
        a.sync_with_folders(&cfg_a, PASSWORD, false, &crate::ai::Reporter::silent())
            .unwrap();
        let place = Place::new(Some(&home), &home);
        let before = folders(dir.path());
        let error = import(&sync_file, PASSWORD, &place, now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Equipe e sincronização"), "{error}");
        let local = dir.path().join("local.sqlite3");
        a.backup(&local).unwrap();
        let error = import(&local, PASSWORD, &place, now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Restaurar cópia"), "{error}");
        // And "Restaurar cópia…" refuses the portable file.
        let error = recovery::restore_local_copy(&portable, &place, now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Importar cópia portátil"), "{error}");
        let mut after = folders(dir.path());
        after.retain(|n| n != "local.sqlite3");
        assert_eq!(after, before);
    }

    #[test]
    fn an_existing_file_is_never_replaced_and_short_passwords_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        drop(full_library(&db));
        let file = dir.path().join("acervo.pontecopia");
        std::fs::write(&file, b"de outra pessoa").unwrap();
        assert!(export(&db, &file, PASSWORD).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"de outra pessoa");
        let fresh = dir.path().join("nova.pontecopia");
        assert!(export(&db, &fresh, "curta").is_err());
        assert!(!fresh.exists());
        assert!(export(&db, &dir.path().join("nome.txt"), PASSWORD).is_err());
        // The publication itself never replaces: a file that appears at the
        // name while the copy is written wins.
        let raced = dir.path().join("corrida.pontecopia");
        std::fs::write(&raced, b"chegou antes").unwrap();
        assert!(write_new(&raced, &[b"copia"]).is_err());
        assert_eq!(std::fs::read(&raced).unwrap(), b"chegou antes");
        let leftovers: Vec<String> = folders(dir.path())
            .into_iter()
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// An attachment of `size` bytes, in a document of its own.
    fn attachment_of(db: &Path, number: &str, size: usize) {
        let mut store = Store::open(db).unwrap();
        let receipt = store.import_capture(&capture(number)).unwrap();
        let mut bytes = b"%PDF-1.4\n".to_vec();
        bytes.extend((0..size).map(|n| (n % 251) as u8));
        store
            .add_attachment(
                crate::attachments::Owner::Document(receipt.document_id),
                "grande.pdf",
                "anexo grande",
                &bytes,
            )
            .unwrap();
    }

    #[test]
    fn a_library_over_the_limit_is_refused_before_it_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        attachment_of(&db, "SEFAZ-PRO-2026/00001", 300 * 1024);
        let file = dir.path().join("acervo.pontecopia");
        let error = export_limited(&db, &file, PASSWORD, 256 * 1024, &|| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("mais do que cabe"), "{error}");
        assert!(!file.exists());
        assert!(folders(dir.path()).iter().all(|n| !n.ends_with(".tmp")));
        // Under the limit, it goes.
        export_limited(&db, &file, PASSWORD, 2 * 1024 * 1024, &|| {}).unwrap();
        assert!(file.exists());
    }

    #[test]
    fn what_is_measured_is_what_is_exported() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("PonteProj");
        std::fs::create_dir_all(&home).unwrap();
        let db = crate::paths::db_path_in(&home);
        attachment_of(&db, "SEFAZ-PRO-2026/00001", 10 * 1024);
        let file = dir.path().join("acervo.pontecopia");
        // A large attachment arrives right after the measure (another
        // connection, the browser for instance): it is not in this export,
        // which is exactly the state that was measured.
        let late = || attachment_of(&db, "SEFAZ-PRO-2026/00002", 400 * 1024);
        export_limited(&db, &file, PASSWORD, 256 * 1024, &late).unwrap();
        let other = dir.path().join("outro").join("PonteProj");
        std::fs::create_dir_all(&other).unwrap();
        let built = import(&file, PASSWORD, &Place::new(Some(&other), &other), now()).unwrap();
        let restored = Store::open(&crate::paths::db_path_in(&built.folder)).unwrap();
        assert_eq!(restored.list_documents("").unwrap().len(), 1);
        // The library itself has both.
        assert_eq!(
            Store::open(&db).unwrap().list_documents("").unwrap().len(),
            2
        );
    }

    #[test]
    fn an_unreadable_task_inside_an_archived_document_refuses_the_export() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        let store = full_library(&db);
        // The task of the document in the Lixeira (no trash row of its own).
        let task: i64 = store
            .conn
            .query_row(
                "SELECT t.id FROM tasks t JOIN trash r ON r.kind='document' AND r.item_id=t.document_id",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut damaged = crate::protection::seal(b"{}").unwrap();
        let last = damaged.len() - 1;
        damaged[last] ^= 0x5a;
        let guards: Vec<String> = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='trigger' AND tbl_name='tasks'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for guard in guards {
            store
                .conn
                .execute_batch(&format!("DROP TRIGGER {guard}"))
                .unwrap();
        }
        store
            .conn
            .execute(
                "UPDATE tasks SET payload=?1 WHERE id=?2",
                rusqlite::params![damaged, task],
            )
            .unwrap();
        drop(store);
        let file = dir.path().join("acervo.pontecopia");
        let error = export(&db, &file, PASSWORD).unwrap_err().to_string();
        assert!(error.contains(&format!("A tarefa nº {task}")), "{error}");
        assert!(!file.exists());
    }

    #[test]
    fn the_import_receipts_count_in_the_size_of_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        let store = Store::open(&db).unwrap();
        // Many completed receipts, nothing else.
        for n in 0..400 {
            store
                .conn
                .execute(
                    "INSERT INTO import_receipts(request_id,manifest,completed_at,state) VALUES(?1,?2,'2026-01-01','done')",
                    rusqlite::params![format!("pedido-{n}"), "x".repeat(600)],
                )
                .unwrap();
        }
        drop(store);
        let file = dir.path().join("acervo.pontecopia");
        let error = export_limited(&db, &file, PASSWORD, 128 * 1024, &|| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("mais do que cabe"), "{error}");
        assert!(!file.exists());
    }

    #[test]
    fn a_trashed_item_that_cannot_be_read_refuses_the_export() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        let mut store = full_library(&db);
        let mut task = crate::tasks::Task::standalone();
        task.title = "Tarefa que vai estragar".into();
        let task = store.save_task(&task).unwrap();
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id: task,
            })
            .unwrap();
        // Its sealed payload is damaged (the reader skips a trashed task it
        // cannot read; the copy would silently lose it).
        let mut damaged = crate::protection::seal(b"{}").unwrap();
        let last = damaged.len() - 1;
        damaged[last] ^= 0x5a;
        // The guards forbid editing what is in the Lixeira: the damage comes
        // from outside the program (the next opening puts them back).
        let guards: Vec<String> = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='trigger' AND tbl_name='tasks'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for guard in guards {
            store
                .conn
                .execute_batch(&format!("DROP TRIGGER {guard}"))
                .unwrap();
        }
        store
            .conn
            .execute(
                "UPDATE tasks SET payload=?1 WHERE id=?2",
                rusqlite::params![damaged, task],
            )
            .unwrap();
        drop(store);
        let file = dir.path().join("acervo.pontecopia");
        let error = export(&db, &file, PASSWORD).unwrap_err().to_string();
        assert!(error.contains(&format!("a tarefa nº {task}")), "{error}");
        assert!(!file.exists());
    }

    #[test]
    fn an_import_completed_before_the_copy_stays_completed_after_it() {
        use crate::record_import::{Created, NewAttachment, NewRecord};
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        let mut store = Store::open(&db).unwrap();
        let fields = crate::manual::ManualFields {
            kind: "whatsapp".into(),
            title: "Conversa com Servidor A — 04/11/2026".into(),
            origin: "Servidor A".into(),
            document_date: "2026-11-04".into(),
            text: "[04/11/2026 10:00] Servidor A: bom dia".into(),
        };
        let record = NewRecord {
            request_id: crate::record_import::whatsapp_request_id(&fields.origin, &fields.text),
            fields,
            metadata: Default::default(),
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', 1, 1, 2, 3]);
        let files = [NewAttachment {
            name: "foto.png".into(),
            bytes: png,
            description: String::new(),
        }];
        let Created::New { document_id, .. } = store
            .create_record_with_attachments(&record, &files)
            .unwrap()
        else {
            panic!("esperava um registro novo")
        };
        // The person deletes the file for good.
        let id = store
            .list_attachments(crate::attachments::Owner::Document(document_id))
            .unwrap()[0]
            .id;
        let target = crate::trash::Target {
            kind: crate::trash::Kind::Attachment,
            id,
        };
        store.trash_item(target.clone()).unwrap();
        store.purge_trash_item(target).unwrap();
        drop(store);
        let file = dir.path().join("acervo.pontecopia");
        export(&db, &file, PASSWORD).unwrap();
        let other = dir.path().join("outro").join("PonteProj");
        std::fs::create_dir_all(&other).unwrap();
        let built = import(&file, PASSWORD, &Place::new(Some(&other), &other), now()).unwrap();
        let mut restored = Store::open(&crate::paths::db_path_in(&built.folder)).unwrap();
        // The same import, sent again (the extension retries): nothing comes back.
        let again = restored
            .create_record_with_attachments(&record, &files)
            .unwrap();
        assert!(matches!(again, Created::Already { .. }), "{again:?}");
        let restored_doc = restored.list_documents("").unwrap()[0].id;
        assert!(restored
            .list_attachments(crate::attachments::Owner::Document(restored_doc))
            .unwrap()
            .is_empty());
    }
}
