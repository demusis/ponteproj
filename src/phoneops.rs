//! A file the phone app took for a record (F3.1b2, `desenho-f3-v3-adendo.md`
//! §10 and §12 rules 19-23): it goes to that record, proved by its identity
//! and by the library space, or to nothing. A refused file is kept in a card
//! of this library, in the same transaction as the refusal; a refusal met
//! later, while an exchange replays the queue, is bound to its operation in
//! the transaction of its card. The deletion of a record is told only when a
//! same-epoch adoption proved it (`gone_documents`).
use crate::intranet::{IntranetMutation, SyncConflict};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeSet;

pub const GONE_ELSEWHERE: &str = "O registro foi apagado em outro aparelho.";
pub const IN_TRASH: &str = "O registro está na Lixeira.";
pub const NOT_HERE: &str = "O registro não está mais disponível neste aparelho.";

/// How long a deletion proved by an adoption is remembered.
const GONE_DAYS: i64 = 30;

/// Whether this module works in the library on `tx`: only in one of schema
/// 22 (one left at 21 for this opening works as before and keeps the schema
/// of Ponte 0.51), where its table and columns are made the first time they
/// are needed (`gone_documents`, and what `phone_operations` came to hold).
pub(crate) fn ready(tx: &Connection) -> Result<bool> {
    if !crate::idents::has_idents(tx)? {
        return Ok(false);
    }
    let made: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('phone_operations') WHERE name='recovery_op')
            AND EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='gone_documents')",
        [],
        |r| r.get(0),
    )?;
    if made {
        return Ok(true);
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS gone_documents(ident TEXT PRIMARY KEY, gone_at TEXT NOT NULL);",
    )?;
    for (column, kind) in [
        // An operation that queued nothing was applied where it was made.
        ("outcome", "TEXT NOT NULL DEFAULT 'accepted'"),
        ("outbox_uid", "TEXT"),
        ("card_id", "INTEGER"),
        ("document_ident", "TEXT"),
        ("recovery_op", "TEXT"),
    ] {
        let kept: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('phone_operations') WHERE name=?1)",
            [column],
            |r| r.get(0),
        )?;
        if !kept {
            tx.execute_batch(&format!(
                "ALTER TABLE phone_operations ADD COLUMN {column} {kind};"
            ))?;
        }
    }
    Ok(true)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The space of this library's numbers: it changes whenever a number held
/// on a screen of the app may name another record (a new library, or an
/// adoption of another epoch). Empty in a library left at schema 21.
pub(crate) fn space(conn: &Connection) -> Result<String> {
    if !crate::idents::has_idents(conn)? {
        return Ok(String::new());
    }
    let value = match crate::idents::meta(conn, "library_space")? {
        Some(value) => value,
        None => renew_space(conn)?,
    };
    Ok(format!("{value:016x}"))
}

fn renew_space(conn: &Connection) -> Result<i64> {
    let bytes: [u8; 8] = crate::sharedfolder::random(8)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Falha ao sortear o espaço do acervo."))?;
    let value = (i64::from_le_bytes(bytes) & i64::MAX).max(1);
    crate::idents::set_meta(conn, "library_space", value)?;
    Ok(value)
}

/// What an adoption needs to know of this library before it starts: the
/// epoch of its identities (none while they are provisional) and the
/// identities of its records.
pub(crate) struct Before {
    epoch: Option<String>,
    documents: BTreeSet<String>,
}

fn document_idents(conn: &Connection) -> Result<BTreeSet<String>> {
    let mut statement = conn.prepare("SELECT ident FROM item_ident WHERE kind='document'")?;
    let rows = statement.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn before_adoption(tx: &Connection) -> Result<Before> {
    if !crate::idents::has_idents(tx)? {
        return Ok(Before {
            epoch: None,
            documents: BTreeSet::new(),
        });
    }
    let (origin, _, epoch) = crate::idents::origin(tx)?;
    let epoch = epoch.filter(|e| origin != "provisional" && !e.is_empty());
    let documents = match epoch {
        Some(_) => document_idents(tx)?,
        None => BTreeSet::new(),
    };
    Ok(Before { epoch, documents })
}

/// At the end of the adoption of a copy of epoch `epoch` (empty: a copy
/// without identities): another epoch is another frame of numbers, so the
/// space changes and nothing is proved deleted (frozen rule 21); the same
/// one proves deleted every record that had an identity and was retired.
pub(crate) fn after_adoption(tx: &Connection, before: Before, epoch: &str) -> Result<()> {
    if !ready(tx)? {
        return Ok(());
    }
    let after = if epoch.is_empty() {
        let (origin, _, kept) = crate::idents::origin(tx)?;
        kept.filter(|e| origin != "provisional" && !e.is_empty())
    } else {
        Some(epoch.to_owned())
    };
    if after != before.epoch {
        renew_space(tx)?;
    } else if before.epoch.is_some() {
        let now_here = document_idents(tx)?;
        let at = now();
        for ident in before.documents.difference(&now_here) {
            tx.execute(
                "INSERT OR REPLACE INTO gone_documents(ident,gone_at) VALUES(?1,?2)",
                params![ident, at],
            )?;
        }
    }
    let cutoff = (Utc::now() - chrono::Duration::days(GONE_DAYS))
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    tx.execute("DELETE FROM gone_documents WHERE gone_at < ?1", [cutoff])?;
    Ok(())
}

/// What the app says of a refused file for the record of identity `ident`.
pub(crate) fn refusal_message(conn: &Connection, ident: Option<&str>) -> Result<&'static str> {
    let Some(ident) = ident.filter(|i| !i.is_empty()) else {
        return Ok(NOT_HERE);
    };
    if !ready(conn)? {
        return Ok(NOT_HERE);
    }
    let gone: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM gone_documents WHERE ident=?1)",
        [ident],
        |r| r.get(0),
    )?;
    if gone {
        return Ok(GONE_ELSEWHERE);
    }
    let trashed: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM item_ident i JOIN trash t ON t.kind='document' AND t.item_id=i.row_id
         WHERE i.kind='document' AND i.ident=?1)",
        [ident],
        |r| r.get(0),
    )?;
    Ok(if trashed { IN_TRASH } else { NOT_HERE })
}

/// The record a file for document `document_id` goes to, inside the
/// attachment's transaction: with an identity, the live record holding it;
/// with a space, only this library's. `None`: refused (frozen rule 23).
pub(crate) fn target(
    conn: &Connection,
    document_id: i64,
    ident: Option<&str>,
    library_space: Option<&str>,
) -> Result<Option<i64>> {
    if let Some(held) = library_space.map(str::trim).filter(|s| !s.is_empty()) {
        if held != space(conn)? {
            return Ok(None);
        }
    }
    let Some(ident) = ident.map(str::trim).filter(|i| !i.is_empty()) else {
        return Ok(Some(document_id));
    };
    let row: Option<i64> = conn
        .query_row(
            "SELECT row_id FROM item_ident WHERE kind='document' AND ident=?1",
            [ident],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row.filter(|row| crate::trash::require_active_document(conn, *row).is_ok()))
}

/// Whether `operation` is a recovery of a card (see [`recover_card`]).
pub(crate) fn recovering(conn: &Connection, operation: Option<&str>) -> Result<bool> {
    let Some(operation) = operation else {
        return Ok(false);
    };
    if !ready(conn)? {
        return Ok(false);
    }
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM phone_operations WHERE recovery_op=?1)",
        [operation],
        |r| r.get(0),
    )?)
}

/// A file refused at once: kept in a card that holds the change as it was
/// asked (the bytes with it), and its operation recorded as refused with
/// that card, in the caller's transaction (frozen rule 20). Answers the card.
#[allow(clippy::too_many_arguments)]
pub(crate) fn park(
    tx: &Connection,
    actor: &str,
    document_id: i64,
    ident: Option<&str>,
    name: &str,
    description: &str,
    data: &[u8],
    operation: Option<&str>,
) -> Result<i64> {
    use crate::idrefs::{Proof, Ref, Refs};
    let ident = ident.map(str::trim).filter(|i| !i.is_empty());
    let message = refusal_message(tx, ident)?;
    let conflict = SyncConflict {
        id: 0,
        created_at: now(),
        actor: actor.to_owned(),
        item_label: format!("Anexo {}", name.trim()),
        local_summary: format!("Arquivo guardado neste aparelho: {}", name.trim()),
        host_summary: message.to_owned(),
        reason: format!("{message} O arquivo não foi anexado a nenhum registro."),
        mutation: IntranetMutation::AddAttachment {
            owner_kind: "document".into(),
            owner_id: document_id,
            name: name.trim().into(),
            description: description.trim().into(),
            data_base64: STANDARD.encode(data),
        },
        base: None,
        unresolved: true,
        identity: Vec::new(),
        mutation_uid: uuid::Uuid::new_v4().simple().to_string(),
        resolved_at: String::new(),
        // The record by the identity the app held, never by the row that
        // has the number here now.
        refs: Some(Refs {
            mutation: vec![Ref {
                kind: crate::idmap::IdKind::Document,
                row: document_id,
                proof: ident.map_or(Proof::Unprovable, |i| Proof::Ident(i.to_owned())),
                link: None,
            }],
            frame: crate::idrefs::frame_tag(tx)?,
            ..Refs::default()
        }),
    };
    let card = crate::intranet::insert_conflict(tx, &conflict)?
        .context("O cartão do arquivo recusado não pôde ser guardado.")?;
    if let Some(operation) = operation {
        crate::storage::record_operation(
            tx,
            operation,
            "attach",
            document_id,
            None,
            Some(data.len() as i64),
        )?;
        ready(tx)?;
        tx.execute(
            "UPDATE phone_operations SET outcome='refused', card_id=?2, document_ident=?3 WHERE operation_id=?1",
            params![operation, card, ident],
        )?;
    }
    Ok(card)
}

/// The operation that queued `outbox_uid` is waiting for the exchange.
pub(crate) fn bind_outbox(
    tx: &Connection,
    operation: &str,
    outbox_uid: &str,
    ident: Option<&str>,
) -> Result<()> {
    if !ready(tx)? {
        return Ok(());
    }
    tx.execute(
        "UPDATE phone_operations SET outcome='pending', outbox_uid=?2, document_ident=?3 WHERE operation_id=?1",
        params![operation, outbox_uid, ident],
    )?;
    Ok(())
}

/// In the transaction that stores card `card` of the queued change `uid`:
/// its operation is refused, with that card (frozen rule 20).
pub(crate) fn bind_card(conn: &Connection, uid: &str, card: i64) -> Result<()> {
    if !uid.is_empty() && ready(conn)? {
        conn.execute(
            "UPDATE phone_operations SET outcome='refused', card_id=?2 WHERE outbox_uid=?1",
            params![uid, card],
        )?;
    }
    Ok(())
}

/// In the transaction that removes the queued change `uid` after it went
/// through: its operation is accepted, unless a card refused it.
pub(crate) fn bind_sent(conn: &Connection, uid: &str) -> Result<()> {
    if !ready(conn)? {
        return Ok(());
    }
    conn.execute(
        "UPDATE phone_operations SET outcome='accepted' WHERE outbox_uid=?1 AND outcome='pending'",
        [uid],
    )?;
    Ok(())
}

/// The queued change `old` is now `new`.
pub(crate) fn rename_outbox(conn: &Connection, old: &str, new: &str) -> Result<()> {
    if !ready(conn)? {
        return Ok(());
    }
    conn.execute(
        "UPDATE phone_operations SET outbox_uid=?2 WHERE outbox_uid=?1",
        params![old, new],
    )?;
    Ok(())
}

/// In the transaction of operation `operation`: the card it recovers is
/// resolved with it (adendo §10 rule 6).
pub(crate) fn recovered(conn: &Connection, operation: &str) -> Result<()> {
    if !ready(conn)? {
        return Ok(());
    }
    let cards: Vec<i64> = conn
        .prepare(
            "SELECT card_id FROM phone_operations WHERE recovery_op=?1 AND card_id IS NOT NULL",
        )?
        .query_map([operation], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for card in cards {
        crate::intranet::retire_conflict(conn, card)?;
    }
    conn.execute(
        "UPDATE phone_operations SET outcome='recovered' WHERE recovery_op=?1",
        [operation],
    )?;
    Ok(())
}

/// A file kept in a card that still waits for the person.
pub struct CardFile {
    pub name: String,
    pub description: String,
    pub bytes: Vec<u8>,
}

/// A refused file waiting for the person, as the recovery screen lists it.
pub struct Refused {
    pub operation_id: String,
    pub card_id: i64,
    pub name: String,
    pub size: usize,
    pub message: &'static str,
}

/// Card `card` while it waits, when it keeps the file of a refused phone
/// operation (no other card is touched by the recovery screen).
fn waiting_card(conn: &Connection, card: i64) -> Result<Option<SyncConflict>> {
    if !ready(conn)? {
        return Ok(None);
    }
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT c.payload FROM offline_conflicts c WHERE c.id=?1 AND EXISTS(
               SELECT 1 FROM phone_operations o WHERE o.card_id=c.id AND o.outcome='refused')",
            [card],
            |r| r.get(0),
        )
        .optional()?;
    Ok(blob
        .and_then(|b| crate::protection::unseal(&b).ok())
        .and_then(|b| serde_json::from_slice::<SyncConflict>(&b).ok())
        .filter(|c| c.resolved_at.is_empty()))
}

fn card_file_of(card: &SyncConflict) -> Option<CardFile> {
    match &card.mutation {
        IntranetMutation::AddAttachment {
            owner_kind,
            name,
            description,
            data_base64,
            ..
        } if owner_kind == "document" => Some(CardFile {
            name: name.clone(),
            description: description.clone(),
            bytes: STANDARD.decode(data_base64).ok()?,
        }),
        _ => None,
    }
}

impl crate::storage::Store {
    /// The proof the app keeps with a file for record `document_id`: its
    /// identity (empty without one) and the library space.
    pub fn attach_proof(&self, document_id: i64) -> Result<(String, String)> {
        let ident = crate::idents::ident_of(&self.conn, "document", document_id)?;
        Ok((ident.unwrap_or_default(), space(&self.conn)?))
    }

    /// What the app says of a refused file for the record of `ident`.
    pub fn refusal_message(&self, ident: Option<&str>) -> Result<&'static str> {
        refusal_message(&self.conn, ident)
    }

    /// The refused files whose card still waits, oldest first.
    pub fn refused_attachments(&self) -> Result<Vec<Refused>> {
        if !ready(&self.conn)? {
            return Ok(Vec::new());
        }
        let rows: Vec<(String, i64, Option<String>)> = self
            .conn
            .prepare(
                "SELECT operation_id,card_id,document_ident FROM phone_operations
                 WHERE outcome='refused' AND card_id IS NOT NULL ORDER BY created_at,operation_id",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = Vec::new();
        for (operation_id, card_id, ident) in rows {
            let Some(file) = waiting_card(&self.conn, card_id)?.and_then(|c| card_file_of(&c))
            else {
                continue;
            };
            out.push(Refused {
                operation_id,
                card_id,
                name: file.name,
                size: file.bytes.len(),
                message: refusal_message(&self.conn, ident.as_deref())?,
            });
        }
        Ok(out)
    }

    /// The file card `card_id` keeps, while it waits.
    pub fn card_file(&self, card_id: i64) -> Result<CardFile> {
        waiting_card(&self.conn, card_id)?
            .as_ref()
            .and_then(card_file_of)
            .context(
                "Este arquivo recusado não está mais guardado (já foi recuperado ou descartado).",
            )
    }

    /// Before the recovery `operation` of card `card_id` runs: the card is
    /// resolved in the transaction that commits that operation.
    pub fn recover_card(&mut self, card_id: i64, operation: &str) -> Result<()> {
        ready(&self.conn)?;
        let changed = self.conn.execute(
            "UPDATE phone_operations SET recovery_op=?2 WHERE card_id=?1 AND outcome='refused'",
            params![card_id, operation.trim()],
        )?;
        anyhow::ensure!(
            changed > 0,
            "Este arquivo recusado não está mais guardado (já foi recuperado ou descartado)."
        );
        Ok(())
    }

    /// "Descartar": card `card_id` is resolved and its file let go.
    pub fn discard_card(&mut self, card_id: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        anyhow::ensure!(
            waiting_card(&tx, card_id)?.is_some(),
            "Este arquivo recusado não está mais guardado (já foi recuperado ou descartado)."
        );
        crate::intranet::retire_conflict(&tx, card_id)?;
        tx.execute(
            "UPDATE phone_operations SET outcome='discarded' WHERE card_id=?1",
            [card_id],
        )?;
        tx.commit()?;
        Ok(())
    }
}
