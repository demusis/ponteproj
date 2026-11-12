//! Recursos: rooms, books, equipment and what is done with them (a booking
//! or a loan with a start and an end date-time).
//!
//! A resource and a use are addressed by a random `uid` (like the work
//! templates), never by row number, so a booking made on one computer keeps
//! its identity on every other and the queue of offline changes needs no
//! number translation for them. A use may point to a document, an
//! atendimento or a task by number; those numbers are translated like the
//! ones of any other change (see `idmap`).
//!
//! Times are local wall time written `AAAA-MM-DDTHH:MM`, compared as text.
//! A use occupies a half-open interval `[start, end)`: a use that starts
//! when another ends does not clash with it. Everything that needs "now"
//! takes it as a parameter, so the rules are tested without the clock.
//!
//! Nothing personal is kept in clear in the library: names, who, times and
//! notes live only in the sealed payload; in clear there are the uid, the
//! status, the uid of the resource and the document link.
use crate::{
    intranet::{record_mutation, IntranetMutation},
    protection::{seal, unseal},
    storage::Store,
    work_templates::version_key,
};
use anyhow::{bail, ensure, Context, Result};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RESOURCE_KINDS: [&str; 4] = ["sala", "equipamento", "livro", "outro"];

/// Starts the text of the error of a use that clashes with another one; the
/// screens strip it and show the sentence after it.
pub const HORARIO_OCUPADO: &str = "[horario-ocupado] ";

const MAX_NAME: usize = 120;
const MAX_LOCATION: usize = 200;
const MAX_IDENTIFICATION: usize = 120;
const MAX_NOTES: usize = 4000;
const MAX_WHO: usize = 120;
const MAX_SECTOR: usize = 120;
const MAX_PURPOSE: usize = 300;
pub const MAX_QUANTITY: u32 = 9999;
const MOMENT_FORMAT: &str = "%Y-%m-%dT%H:%M";

/// "Sala", "Equipamento", "Livro", "Outro".
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "sala" => "Sala",
        "equipamento" => "Equipamento",
        "livro" => "Livro",
        _ => "Outro",
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceStatus {
    #[default]
    #[serde(rename = "ativo")]
    Active,
    #[serde(rename = "fora_de_uso")]
    Inactive,
}

impl ResourceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ativo",
            Self::Inactive => "fora_de_uso",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "ativo" => Ok(Self::Active),
            "fora_de_uso" => Ok(Self::Inactive),
            _ => bail!("Situação do recurso inválida."),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Ativo",
            Self::Inactive => "Fora de uso",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UseStatus {
    #[default]
    #[serde(rename = "reservado")]
    Reserved,
    #[serde(rename = "em_uso")]
    InUse,
    #[serde(rename = "devolvido")]
    Returned,
    #[serde(rename = "cancelado")]
    Cancelled,
}

impl UseStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reservado",
            Self::InUse => "em_uso",
            Self::Returned => "devolvido",
            Self::Cancelled => "cancelado",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "reservado" => Ok(Self::Reserved),
            "em_uso" => Ok(Self::InUse),
            "devolvido" => Ok(Self::Returned),
            "cancelado" => Ok(Self::Cancelled),
            _ => bail!("Situação do uso inválida."),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Reserved => "Reservado",
            Self::InUse => "Em uso",
            Self::Returned => "Devolvido",
            Self::Cancelled => "Cancelado",
        }
    }
    /// Reserved or in use: the uses that take the resource.
    pub fn occupying(self) -> bool {
        matches!(self, Self::Reserved | Self::InUse)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Resource {
    /// `work_templates::new_uid()`; empty = new.
    pub uid: String,
    pub name: String,
    pub kind: String,
    pub quantity: u32,
    /// "Onde fica".
    pub location: String,
    /// Tombo, patrimônio, ISBN…
    pub identification: String,
    pub notes: String,
    pub status: ResourceStatus,
    /// RFC 3339, set by the store.
    pub created_at: String,
    pub updated_at: String,
}

impl Default for Resource {
    fn default() -> Self {
        Self {
            uid: String::new(),
            name: String::new(),
            kind: "outro".into(),
            quantity: 1,
            location: String::new(),
            identification: String::new(),
            notes: String::new(),
            status: ResourceStatus::Active,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceUse {
    pub uid: String,
    pub resource_uid: String,
    pub quantity: u32,
    /// "Quem".
    pub who: String,
    /// "Setor".
    pub sector: String,
    /// "Finalidade".
    pub purpose: String,
    /// Local wall time `AAAA-MM-DDTHH:MM`.
    pub start: String,
    /// Same format; after `start`.
    pub end: String,
    /// Same format; only with `Returned`.
    pub returned_at: Option<String>,
    pub status: UseStatus,
    pub notes: String,
    pub document_id: Option<i64>,
    pub saiop_id: Option<i64>,
    pub task_id: Option<i64>,
    /// The active actor when it was created, set by the store.
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Default for ResourceUse {
    fn default() -> Self {
        Self {
            uid: String::new(),
            resource_uid: String::new(),
            quantity: 1,
            who: String::new(),
            sector: String::new(),
            purpose: String::new(),
            start: String::new(),
            end: String::new(),
            returned_at: None,
            status: UseStatus::Reserved,
            notes: String::new(),
            document_id: None,
            saiop_id: None,
            task_id: None,
            created_by: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Time

/// Reads `AAAA-MM-DDTHH:MM` and nothing else.
pub fn parse_moment(text: &str) -> Result<NaiveDateTime> {
    let parsed = NaiveDateTime::parse_from_str(text, MOMENT_FORMAT).ok();
    match parsed {
        Some(moment) if text.len() == 16 && format_moment(moment) == text => Ok(moment),
        _ => bail!("Horário inválido: “{text}”. Informe data e hora, como 10/11/2026 14:30."),
    }
}

pub fn format_moment(moment: NaiveDateTime) -> String {
    moment.format(MOMENT_FORMAT).to_string()
}

/// Local wall time now, to the minute.
pub fn now_moment() -> String {
    format_moment(chrono::Local::now().naive_local())
}

/// `dd/mm/aaaa HH:MM` of a moment; what cannot be read goes out as it is.
pub fn moment_label(text: &str) -> String {
    match parse_moment(text) {
        Ok(moment) => moment.format("%d/%m/%Y %H:%M").to_string(),
        Err(_) => text.to_owned(),
    }
}

/// `AAAA-MM-DD` part of a moment, or empty.
pub fn moment_date(text: &str) -> String {
    parse_moment(text)
        .map(|m| m.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// `HH:MM` part of a moment, or empty.
pub fn moment_time(text: &str) -> String {
    parse_moment(text)
        .map(|m| m.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Hours from `from` to `to` (never negative), when both can be read.
pub fn hours_between(from: &str, to: &str) -> Option<f64> {
    let from = parse_moment(from).ok()?;
    let to = parse_moment(to).ok()?;
    Some((to - from).num_minutes().max(0) as f64 / 60.0)
}

// ---------------------------------------------------------------------------
// Validation

fn check_text(value: &str, max: usize, what: &str, multiline: bool) -> Result<()> {
    ensure!(
        value.chars().count() <= max,
        "{what}: no máximo {max} caracteres."
    );
    ensure!(
        !value
            .chars()
            .any(|c| c.is_control() && !(multiline && (c == '\n' || c == '\t'))),
        "{what}: tem caracteres inválidos."
    );
    Ok(())
}

fn check_uid(uid: &str) -> Result<()> {
    ensure!(
        uid.len() == 32 && uid.bytes().all(|b| b.is_ascii_hexdigit()),
        "O código do item é inválido."
    );
    Ok(())
}

impl Resource {
    /// Trims the texts the way the store keeps them.
    pub fn normalized(&self) -> Self {
        let mut out = self.clone();
        out.name = out.name.trim().to_owned();
        // The kind is no longer chosen by anyone, but stays in the record:
        // what is there is kept, an empty one becomes "outro".
        out.kind = out.kind.trim().to_owned();
        if out.kind.is_empty() {
            out.kind = "outro".to_owned();
        }
        out.location = out.location.trim().to_owned();
        out.identification = out.identification.trim().to_owned();
        out.notes = out.notes.trim().to_owned();
        out
    }

    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        ensure!(!name.is_empty(), "Informe o nome do recurso.");
        check_text(name, MAX_NAME, "Nome do recurso", false)?;
        // Hidden field kept for the computers that still show it: any short
        // text is accepted and kept as it is.
        check_text(self.kind.trim(), 40, "Tipo", false)?;
        ensure!(
            (1..=MAX_QUANTITY).contains(&self.quantity),
            "A quantidade do recurso deve ficar entre 1 e {MAX_QUANTITY}."
        );
        check_text(self.location.trim(), MAX_LOCATION, "Onde fica", false)?;
        check_text(
            self.identification.trim(),
            MAX_IDENTIFICATION,
            "Identificação",
            false,
        )?;
        check_text(self.notes.trim(), MAX_NOTES, "Observações", true)?;
        Ok(())
    }

    /// Whether two versions say the same thing (dates the store sets aside,
    /// and the hidden `kind`).
    pub fn same_content(&self, other: &Self) -> bool {
        let strip = |r: &Self| {
            let mut r = r.normalized();
            r.kind.clear();
            r.created_at.clear();
            r.updated_at.clear();
            r
        };
        strip(self) == strip(other)
    }
}

impl ResourceUse {
    /// Trims the texts and settles `returned_at` the way the store keeps them.
    pub fn normalized(&self) -> Self {
        let mut out = self.clone();
        out.resource_uid = out.resource_uid.trim().to_owned();
        out.who = out.who.trim().to_owned();
        out.sector = out.sector.trim().to_owned();
        out.purpose = out.purpose.trim().to_owned();
        out.notes = out.notes.trim().to_owned();
        out.start = out.start.trim().to_owned();
        out.end = out.end.trim().to_owned();
        out.returned_at = match (out.status, out.returned_at.take()) {
            (UseStatus::Returned, Some(at)) if !at.trim().is_empty() => Some(at.trim().to_owned()),
            _ => None,
        };
        out
    }

    /// The rules that need no library.
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.resource_uid.trim().is_empty(), "Escolha o recurso.");
        ensure!(
            (1..=MAX_QUANTITY).contains(&self.quantity),
            "A quantidade deve ficar entre 1 e {MAX_QUANTITY}."
        );
        let who = self.who.trim();
        ensure!(!who.is_empty(), "Informe quem vai usar o recurso.");
        check_text(who, MAX_WHO, "Quem", false)?;
        check_text(self.sector.trim(), MAX_SECTOR, "Setor", false)?;
        check_text(self.purpose.trim(), MAX_PURPOSE, "Finalidade", false)?;
        check_text(self.notes.trim(), MAX_NOTES, "Observações", true)?;
        let start = parse_moment(self.start.trim())?;
        let end = parse_moment(self.end.trim())?;
        ensure!(end > start, "O fim previsto precisa ser depois do início.");
        match (self.status, self.returned_at.as_deref().map(str::trim)) {
            (UseStatus::Returned, None | Some("")) => {
                bail!("Informe quando o recurso foi devolvido.")
            }
            (UseStatus::Returned, Some(at)) => {
                ensure!(
                    parse_moment(at)? >= start,
                    "A devolução não pode ser antes do início."
                );
            }
            _ => {}
        }
        Ok(())
    }

    /// The interval this use takes the resource for at `now`, `None` when it
    /// takes nothing (returned, cancelled). An item in use and not returned
    /// is still out: until `max(end, now)`.
    pub fn occupies_at(&self, now: &str) -> Option<(String, String)> {
        match self.status {
            UseStatus::Reserved => Some((self.start.clone(), self.end.clone())),
            UseStatus::InUse => {
                let until = if self.end.as_str() >= now {
                    &self.end
                } else {
                    now
                };
                Some((self.start.clone(), until.to_owned()))
            }
            UseStatus::Returned | UseStatus::Cancelled => None,
        }
    }

    /// In use and past its expected end.
    pub fn overdue_at(&self, now: &str) -> bool {
        self.status == UseStatus::InUse && self.end.as_str() < now
    }

    /// Whether two versions say the same thing (who saved it and when aside).
    pub fn same_content(&self, other: &Self) -> bool {
        let strip = |u: &Self| {
            let mut u = u.normalized();
            u.created_by.clear();
            u.created_at.clear();
            u.updated_at.clear();
            u
        };
        strip(self) == strip(other)
    }
}

// ---------------------------------------------------------------------------
// Clashes and counts (pure)

fn intersects(a: &(String, String), b: &(String, String)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// The most the other uses take at once inside `[start, end)`, and the uses
/// taking it at the first instant that happens.
fn peak<'a>(
    resource_uid: &str,
    uses: &'a [ResourceUse],
    except_uid: &str,
    window: &(String, String),
    now: &str,
) -> (u32, Vec<&'a ResourceUse>) {
    let relevant: Vec<(&ResourceUse, (String, String))> = uses
        .iter()
        .filter(|u| {
            u.resource_uid == resource_uid && (except_uid.is_empty() || u.uid != except_uid)
        })
        .filter_map(|u| u.occupies_at(now).map(|interval| (u, interval)))
        .filter(|(_, interval)| intersects(interval, window))
        .collect();
    let mut instants: Vec<&str> = vec![window.0.as_str()];
    instants.extend(
        relevant
            .iter()
            .map(|(_, i)| i.0.as_str())
            .filter(|s| *s > window.0.as_str()),
    );
    instants.sort_unstable();
    instants.dedup();
    let mut best: (u32, Vec<&ResourceUse>) = (0, Vec::new());
    for at in instants {
        let members: Vec<&ResourceUse> = relevant
            .iter()
            .filter(|(_, i)| i.0.as_str() <= at && at < i.1.as_str())
            .map(|(u, _)| *u)
            .collect();
        let sum: u32 = members.iter().map(|u| u.quantity).sum();
        if sum > best.0 {
            best = (sum, members);
        }
    }
    best.1
        .sort_by(|a, b| (&a.start, &a.uid).cmp(&(&b.start, &b.uid)));
    best
}

/// The other uses that stop `candidate` from fitting: empty when it fits
/// (or when it takes nothing). The quantity taken at any instant of the
/// candidate's interval, counting the candidate, may not pass the
/// resource's quantity.
pub fn clashes(
    resource: &Resource,
    uses: &[ResourceUse],
    candidate: &ResourceUse,
    now: &str,
) -> Vec<ResourceUse> {
    let Some(window) = candidate.occupies_at(now) else {
        return Vec::new();
    };
    let (peak_quantity, members) = peak(&resource.uid, uses, &candidate.uid, &window, now);
    if peak_quantity + candidate.quantity > resource.quantity {
        members.into_iter().cloned().collect()
    } else {
        Vec::new()
    }
}

/// What is left of the resource during `[start, end)`.
pub fn free_quantity(
    resource: &Resource,
    uses: &[ResourceUse],
    start: &str,
    end: &str,
    except_uid: &str,
    now: &str,
) -> u32 {
    let window = (start.to_owned(), end.to_owned());
    let (taken, _) = peak(&resource.uid, uses, except_uid, &window, now);
    resource.quantity.saturating_sub(taken)
}

/// The sentence of the refusal: who holds the resource and until when.
pub fn clash_sentence(resource: &Resource, clashing: &[ResourceUse], now: &str) -> String {
    let Some(first) = clashing.first() else {
        return format!(
            "O recurso “{}” não tem quantidade livre nesse horário.",
            resource.name
        );
    };
    let more = match clashing.len() - 1 {
        0 => String::new(),
        1 => " (e mais 1 uso nesse horário)".to_owned(),
        n => format!(" (e mais {n} usos nesse horário)"),
    };
    if first.overdue_at(now) {
        // Its occupation runs "until now", which would read as "free in a
        // minute": say what is true, that it was not given back.
        return format!(
            "O recurso “{}” já está em uso por {}, com devolução atrasada desde {}{more}.",
            resource.name,
            first.who.trim(),
            moment_label(&first.end)
        );
    }
    let until = first
        .occupies_at(now)
        .map(|(_, end)| moment_label(&end))
        .unwrap_or_default();
    let held = if first.status == UseStatus::InUse {
        "em uso"
    } else {
        "reservado"
    };
    format!(
        "O recurso “{}” já está {held} por {} até {until}{more}.",
        resource.name,
        first.who.trim()
    )
}

/// The refusal text, with the marker the screens strip.
pub fn clash_error(resource: &Resource, clashing: &[ResourceUse], now: &str) -> String {
    format!(
        "{HORARIO_OCUPADO}{}",
        clash_sentence(resource, clashing, now)
    )
}

/// The text of an error without the clash marker.
pub fn strip_marker(error: &str) -> &str {
    match error.find(HORARIO_OCUPADO) {
        Some(at) => &error[at + HORARIO_OCUPADO.len()..],
        None => error,
    }
}

/// When the quantity of a resource cannot go down to `quantity`: the first
/// instant (`now` or later) at which the occupying uses need more, how much
/// they need, and the use that tips it over.
pub fn quantity_blocker(
    uses: &[ResourceUse],
    quantity: u32,
    now: &str,
) -> Option<(String, u32, ResourceUse)> {
    let live: Vec<(&ResourceUse, (String, String))> = uses
        .iter()
        .filter_map(|u| u.occupies_at(now).map(|i| (u, i)))
        .filter(|(_, i)| i.1.as_str() > now)
        .collect();
    let mut instants: Vec<&str> = vec![now];
    instants.extend(live.iter().map(|(_, i)| i.0.as_str()).filter(|s| *s > now));
    instants.sort_unstable();
    instants.dedup();
    for at in instants {
        let mut members: Vec<&ResourceUse> = live
            .iter()
            .filter(|(_, i)| i.0.as_str() <= at && at < i.1.as_str())
            .map(|(u, _)| *u)
            .collect();
        let needed: u32 = members.iter().map(|u| u.quantity).sum();
        if needed > quantity {
            members.sort_by(|a, b| (&a.start, &a.uid).cmp(&(&b.start, &b.uid)));
            let tipping = members.last().map(|u| (*u).clone())?;
            return Some((at.to_owned(), needed, tipping));
        }
    }
    None
}

/// What the day shows at a glance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DayCounts {
    /// Reserved or in use, with its interval touching today.
    pub reservations_today: usize,
    /// In use and past the expected end.
    pub overdue: usize,
}

pub fn day_counts(uses: &[ResourceUse], today: NaiveDate, now: &str) -> DayCounts {
    let from = format!("{}T00:00", today.format("%Y-%m-%d"));
    let next = today + Duration::days(1);
    let to = format!("{}T00:00", next.format("%Y-%m-%d"));
    let day = (from, to);
    let mut counts = DayCounts::default();
    for item in uses {
        if item.overdue_at(now) {
            counts.overdue += 1;
        }
        if item
            .occupies_at(now)
            .is_some_and(|interval| intersects(&interval, &day))
        {
            counts.reservations_today += 1;
        }
    }
    counts
}

// ---------------------------------------------------------------------------
// Snapshot rows (team sync)

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceRow {
    pub id: i64,
    pub uid: String,
    pub status: String,
    pub value: Resource,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceUseRow {
    pub id: i64,
    pub uid: String,
    pub resource_uid: String,
    pub status: String,
    pub document_id: Option<i64>,
    pub value: ResourceUse,
    pub updated_at: String,
}

/// Both tables, for the team snapshot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourcesSnapshot {
    #[serde(default)]
    pub resources: Vec<ResourceRow>,
    #[serde(default)]
    pub uses: Vec<ResourceUseRow>,
}

fn decode<T: for<'a> Deserialize<'a>>(blob: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(&unseal(blob)?)?)
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    seal(&serde_json::to_vec(value)?)
}

fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Every use of one resource, read inside a transaction.
fn uses_of(conn: &Connection, resource_uid: &str) -> Result<Vec<ResourceUse>> {
    let mut statement =
        conn.prepare("SELECT uid,payload FROM resource_uses WHERE resource_uid=?1")?;
    let rows = statement
        .query_map([resource_uid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (uid, blob) in rows {
        let mut item: ResourceUse = decode(&blob)?;
        item.uid = uid;
        item.resource_uid = resource_uid.to_owned();
        out.push(item);
    }
    Ok(out)
}

fn load_resource(conn: &Connection, uid: &str) -> Result<Option<Resource>> {
    let blob: Option<Vec<u8>> = conn
        .query_row("SELECT payload FROM resources WHERE uid=?1", [uid], |r| {
            r.get(0)
        })
        .optional()?;
    match blob {
        Some(blob) => {
            let mut resource: Resource = decode(&blob)?;
            resource.uid = uid.to_owned();
            Ok(Some(resource))
        }
        None => Ok(None),
    }
}

impl Store {
    pub(crate) fn ensure_resources(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS resources(
                id INTEGER PRIMARY KEY,
                uid TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL CHECK(status IN ('ativo','fora_de_uso')),
                payload BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS resource_uses(
                id INTEGER PRIMARY KEY,
                uid TEXT NOT NULL UNIQUE,
                resource_uid TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('reservado','em_uso','devolvido','cancelado')),
                document_id INTEGER REFERENCES documents(id) ON DELETE SET NULL,
                payload BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS resource_uses_resource ON resource_uses(resource_uid);",
        )?;
        Ok(())
    }

    // -- reads --------------------------------------------------------------

    /// Every resource, by name, memoized until the library changes.
    pub fn list_resources(&self) -> Result<Vec<Resource>> {
        self.cached("resources", || self.list_resources_uncached())
    }

    fn list_resources_uncached(&self) -> Result<Vec<Resource>> {
        let mut statement = self
            .conn
            .prepare("SELECT uid,status,payload FROM resources")?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (uid, status, blob) in rows {
            let mut resource: Resource = decode(&blob)
                .context("Não foi possível ler um recurso: o registro está inválido.")?;
            resource.uid = uid;
            if let Ok(status) = ResourceStatus::parse(&status) {
                resource.status = status;
            }
            out.push(resource);
        }
        out.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.uid.cmp(&b.uid))
        });
        Ok(out)
    }

    /// Every use, by start, memoized until the library changes. The document
    /// link comes from the library's column; a task or an atendimento that
    /// no longer exists is dropped from the link.
    pub fn list_resource_uses(&self) -> Result<Vec<ResourceUse>> {
        self.cached("resource_uses", || self.list_resource_uses_uncached())
    }

    fn list_resource_uses_uncached(&self) -> Result<Vec<ResourceUse>> {
        let ids = |sql: &str| -> Result<BTreeSet<i64>> {
            let mut statement = self.conn.prepare(sql)?;
            let rows = statement
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<BTreeSet<_>>>()?;
            Ok(rows)
        };
        let tasks = ids("SELECT id FROM tasks")?;
        let attendances = ids("SELECT id FROM saiop_attendances")?;
        let mut statement = self
            .conn
            .prepare("SELECT uid,resource_uid,status,document_id,payload FROM resource_uses")?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (uid, resource_uid, status, document_id, blob) in rows {
            let mut item: ResourceUse =
                decode(&blob).context("Não foi possível ler um uso: o registro está inválido.")?;
            item.uid = uid;
            item.resource_uid = resource_uid;
            if let Ok(status) = UseStatus::parse(&status) {
                item.status = status;
            }
            item.document_id = document_id;
            item.task_id = item.task_id.filter(|id| tasks.contains(id));
            item.saiop_id = item.saiop_id.filter(|id| attendances.contains(id));
            out.push(item);
        }
        out.sort_by(|a, b| (&a.start, &a.uid).cmp(&(&b.start, &b.uid)));
        Ok(out)
    }

    /// The version a screen hands back as `expected_version`; 0 when new.
    pub fn resource_version(&self, uid: &str) -> Result<i64> {
        Ok(self.item_version("resource", version_key(uid))?.version)
    }

    pub fn resource_use_version(&self, uid: &str) -> Result<i64> {
        Ok(self.item_version("resource_use", version_key(uid))?.version)
    }

    pub(crate) fn resource_exists(&self, uid: &str) -> bool {
        self.conn
            .query_row("SELECT 1 FROM resources WHERE uid=?1", [uid], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub(crate) fn resource_use_exists(&self, uid: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM resource_uses WHERE uid=?1",
                [uid],
                |_| Ok(()),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    // -- writes ---------------------------------------------------------------

    /// Creates or changes a resource; returns its uid. Lowering the quantity
    /// below what current and future uses need is refused.
    pub fn save_resource(
        &mut self,
        resource: &Resource,
        expected_version: Option<i64>,
    ) -> Result<String> {
        let mut resource = resource.normalized();
        if resource.uid.is_empty() {
            resource.uid = new_uid();
        }
        check_uid(&resource.uid)?;
        resource.validate()?;
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_resources(&cfg)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveResource {
                    resource: resource.clone(),
                    expected_version,
                },
            )?;
            return Ok(resource.uid);
        }
        let actor = self.active_actor();
        let uid = resource.uid.clone();
        let timestamp = now_stamp();
        let now = now_moment();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = load_resource(&tx, &uid)?;
        let mut stored = resource.clone();
        stored.updated_at = timestamp.clone();
        let details = match &previous {
            Some(before) => {
                stored.created_at = before.created_at.clone();
                if stored.quantity < before.quantity {
                    let uses = uses_of(&tx, &uid)?;
                    if let Some((at, needed, blocking)) =
                        quantity_blocker(&uses, stored.quantity, &now)
                    {
                        bail!(
                            "A quantidade não pode ficar em {}: em {} o uso de {} faz o recurso precisar de {}. Cancele ou ajuste esse uso antes.",
                            stored.quantity,
                            moment_label(&at),
                            blocking.who,
                            needed
                        );
                    }
                }
                tx.execute(
                    "UPDATE resources SET status=?1,payload=?2,updated_at=?3 WHERE uid=?4",
                    params![stored.status.as_str(), encode(&stored)?, timestamp, uid],
                )?;
                format!("Alterou o recurso \"{}\"", stored.name)
            }
            None => {
                stored.created_at = timestamp.clone();
                tx.execute(
                    "INSERT INTO resources(uid,status,payload,updated_at) VALUES(?1,?2,?3,?4)",
                    params![uid, stored.status.as_str(), encode(&stored)?, timestamp],
                )?;
                format!("Cadastrou o recurso \"{}\"", stored.name)
            }
        };
        record_mutation(
            &tx,
            &actor,
            "resource",
            version_key(&uid),
            expected_version,
            "save_resource",
            None,
            &details,
        )?;
        tx.commit()?;
        Ok(uid)
    }

    /// Removes a resource that never had a use; otherwise it is marked
    /// "Fora de uso", which keeps its history.
    pub fn delete_resource(&mut self, uid: &str, expected_version: Option<i64>) -> Result<()> {
        check_uid(uid)?;
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_resources(&cfg)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::DeleteResource {
                    uid: uid.to_owned(),
                    expected_version,
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resource = load_resource(&tx, uid)?.context("Recurso não encontrado (já apagado?).")?;
        let uses: i64 = tx.query_row(
            "SELECT count(*) FROM resource_uses WHERE resource_uid=?1",
            [uid],
            |r| r.get(0),
        )?;
        ensure!(
            uses == 0,
            "Este recurso tem usos registrados e não pode ser apagado. Marque-o como Fora de uso para que ele deixe de aparecer nas reservas."
        );
        tx.execute("DELETE FROM resources WHERE uid=?1", [uid])?;
        record_mutation(
            &tx,
            &actor,
            "resource",
            version_key(uid),
            expected_version,
            "delete_resource",
            None,
            &format!("Apagou o recurso \"{}\"", resource.name),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Creates or changes a use; returns its uid. Inside the write
    /// transaction, a reserved or in-use booking that does not fit in the
    /// resource's quantity is refused with an error that starts with
    /// [`HORARIO_OCUPADO`].
    pub fn save_resource_use(
        &mut self,
        item: &ResourceUse,
        expected_version: Option<i64>,
    ) -> Result<String> {
        let mut item = item.normalized();
        if item.uid.is_empty() {
            item.uid = new_uid();
        }
        check_uid(&item.uid)?;
        item.validate()?;
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_resources(&cfg)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveResourceUse {
                    item: item.clone(),
                    expected_version,
                },
            )?;
            return Ok(item.uid);
        }
        let actor = self.active_actor();
        let uid = item.uid.clone();
        let timestamp = now_stamp();
        let now = now_moment();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resource = load_resource(&tx, &item.resource_uid)?.context(
            "O recurso deste uso não foi encontrado. Ele pode ter sido apagado em outro computador.",
        )?;
        let blob: Option<Vec<u8>> = tx
            .query_row(
                "SELECT payload FROM resource_uses WHERE uid=?1",
                [&uid],
                |r| r.get(0),
            )
            .optional()?;
        let previous: Option<ResourceUse> = match blob {
            Some(blob) => Some(decode(&blob)?),
            None => None,
        };
        let mut stored = item.clone();
        stored.updated_at = timestamp.clone();
        match &previous {
            Some(before) => {
                stored.created_by = before.created_by.clone();
                stored.created_at = before.created_at.clone();
            }
            None => {
                stored.created_by = actor.clone();
                stored.created_at = timestamp.clone();
            }
        }
        // Links that point to nothing here are dropped, never kept as a
        // number that means something else.
        let exists = |sql: &str, id: Option<i64>| -> Result<Option<i64>> {
            match id {
                Some(id) => Ok(tx.query_row(sql, [id], |_| Ok(())).optional()?.map(|()| id)),
                None => Ok(None),
            }
        };
        stored.document_id = exists("SELECT 1 FROM documents WHERE id=?1", stored.document_id)?;
        stored.task_id = exists("SELECT 1 FROM tasks WHERE id=?1", stored.task_id)?;
        stored.saiop_id = exists(
            "SELECT 1 FROM saiop_attendances WHERE id=?1",
            stored.saiop_id,
        )?;
        if stored.status.occupying() {
            // A use that keeps taking the same things it already took is
            // not checked again: editing a note of a loan that is overdue
            // must not fail because a later booking was made meanwhile.
            let unchanged = previous.as_ref().is_some_and(|before| {
                before.status == stored.status
                    && before.resource_uid == stored.resource_uid
                    && before.quantity == stored.quantity
                    && before.start == stored.start
                    && before.end == stored.end
            });
            let reactivated = previous.as_ref().is_none_or(|b| !b.status.occupying());
            if reactivated {
                ensure!(
                    resource.status == ResourceStatus::Active,
                    "O recurso “{}” está Fora de uso e não aceita novas reservas. Marque-o como Ativo para reservar.",
                    resource.name
                );
            }
            if !unchanged {
                // The cap is for what takes the resource now: a closed use,
                // or one that keeps what it already took, keeps its quantity
                // even when the resource was lowered after it.
                ensure!(
                    stored.quantity <= resource.quantity,
                    "A quantidade passa do que o recurso tem ({}).",
                    resource.quantity
                );
                let uses = uses_of(&tx, &resource.uid)?;
                let clashing = clashes(&resource, &uses, &stored, &now);
                if !clashing.is_empty() {
                    bail!("{}", clash_error(&resource, &clashing, &now));
                }
            }
        }
        let period = format!(
            "{} a {}",
            moment_label(&stored.start),
            moment_label(&stored.end)
        );
        let details = match &previous {
            None => match stored.status {
                UseStatus::InUse => {
                    format!("Registrou o uso de \"{}\" ({period})", resource.name)
                }
                UseStatus::Returned => {
                    format!(
                        "Registrou o uso devolvido de \"{}\" ({period})",
                        resource.name
                    )
                }
                UseStatus::Cancelled => {
                    format!(
                        "Registrou uma reserva cancelada de \"{}\" ({period})",
                        resource.name
                    )
                }
                UseStatus::Reserved => format!("Reservou \"{}\" ({period})", resource.name),
            },
            Some(before) if before.status != stored.status => match stored.status {
                UseStatus::Returned => {
                    format!("Registrou a devolução de \"{}\" ({period})", resource.name)
                }
                UseStatus::Cancelled => {
                    format!("Cancelou a reserva de \"{}\" ({period})", resource.name)
                }
                UseStatus::InUse => {
                    format!("Registrou o uso de \"{}\" ({period})", resource.name)
                }
                UseStatus::Reserved => {
                    format!("Reativou a reserva de \"{}\" ({period})", resource.name)
                }
            },
            Some(_) => format!("Alterou o uso de \"{}\" ({period})", resource.name),
        };
        if previous.is_some() {
            tx.execute(
                "UPDATE resource_uses SET resource_uid=?1,status=?2,document_id=?3,payload=?4,updated_at=?5 WHERE uid=?6",
                params![
                    stored.resource_uid,
                    stored.status.as_str(),
                    stored.document_id,
                    encode(&stored)?,
                    timestamp,
                    uid
                ],
            )?;
        } else {
            tx.execute(
                "INSERT INTO resource_uses(uid,resource_uid,status,document_id,payload,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    uid,
                    stored.resource_uid,
                    stored.status.as_str(),
                    stored.document_id,
                    encode(&stored)?,
                    timestamp
                ],
            )?;
        }
        record_mutation(
            &tx,
            &actor,
            "resource_use",
            version_key(&uid),
            expected_version,
            "save_resource_use",
            stored.document_id,
            &details,
        )?;
        tx.commit()?;
        Ok(uid)
    }

    /// Permanent removal of a use; there is no trash for it.
    pub fn delete_resource_use(&mut self, uid: &str, expected_version: Option<i64>) -> Result<()> {
        check_uid(uid)?;
        if let Some(cfg) = self.client_config()? {
            self.ensure_host_stores_resources(&cfg)?;
            self.remote_mutate(
                &cfg,
                IntranetMutation::DeleteResourceUse {
                    uid: uid.to_owned(),
                    expected_version,
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (resource_uid, document_id, blob): (String, Option<i64>, Vec<u8>) = tx
            .query_row(
                "SELECT resource_uid,document_id,payload FROM resource_uses WHERE uid=?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .context("Uso não encontrado (já apagado?).")?;
        let item: ResourceUse = decode(&blob)?;
        let name = load_resource(&tx, &resource_uid)?
            .map(|r| r.name)
            .unwrap_or_default();
        tx.execute("DELETE FROM resource_uses WHERE uid=?1", [uid])?;
        record_mutation(
            &tx,
            &actor,
            "resource_use",
            version_key(uid),
            expected_version,
            "delete_resource_use",
            document_id,
            &format!(
                "Apagou o uso de \"{name}\" ({} a {})",
                moment_label(&item.start),
                moment_label(&item.end)
            ),
        )?;
        tx.commit()?;
        Ok(())
    }

    // -- team sync --------------------------------------------------------------

    pub(crate) fn export_resources(&self) -> Result<ResourcesSnapshot> {
        let mut out = ResourcesSnapshot::default();
        let mut statement = self
            .conn
            .prepare("SELECT id,uid,status,payload,updated_at FROM resources ORDER BY id")?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, uid, status, blob, updated_at) in rows {
            let mut value: Resource = decode(&blob)?;
            value.uid = uid.clone();
            out.resources.push(ResourceRow {
                id,
                uid,
                status,
                value,
                updated_at,
            });
        }
        let mut statement = self.conn.prepare(
            "SELECT id,uid,resource_uid,status,document_id,payload,updated_at FROM resource_uses ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Vec<u8>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, uid, resource_uid, status, document_id, blob, updated_at) in rows {
            let mut value: ResourceUse = decode(&blob)?;
            value.uid = uid.clone();
            value.resource_uid = resource_uid.clone();
            value.document_id = document_id;
            out.uses.push(ResourceUseRow {
                id,
                uid,
                resource_uid,
                status,
                document_id,
                value,
                updated_at,
            });
        }
        Ok(out)
    }
}

/// Writes both tables from a snapshot (the tables were emptied by the
/// caller), re-sealing the payloads for this Windows account.
pub(crate) fn apply_resources(tx: &Connection, snapshot: &ResourcesSnapshot) -> Result<()> {
    for row in &snapshot.resources {
        tx.execute(
            "INSERT INTO resources(id,uid,status,payload,updated_at) VALUES(?1,?2,?3,?4,?5)",
            params![
                row.id,
                row.uid,
                row.status,
                encode(&row.value)?,
                row.updated_at
            ],
        )?;
    }
    for row in &snapshot.uses {
        tx.execute(
            "INSERT INTO resource_uses(id,uid,resource_uid,status,document_id,payload,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                row.id,
                row.uid,
                row.resource_uid,
                row.status,
                row.document_id,
                encode(&row.value)?,
                row.updated_at
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-10T12:00";
    const FAR: &str = "2099-01-05";

    fn room() -> Resource {
        Resource {
            uid: "a".repeat(32),
            name: "Sala de reuniões do 2º andar".into(),
            kind: "sala".into(),
            ..Resource::default()
        }
    }

    fn booking(uid: &str, start: &str, end: &str) -> ResourceUse {
        ResourceUse {
            uid: uid.into(),
            resource_uid: "a".repeat(32),
            who: "Servidor B".into(),
            start: start.into(),
            end: end.into(),
            ..ResourceUse::default()
        }
    }

    fn at(day: &str, time: &str) -> String {
        format!("{day}T{time}")
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("recursos.sqlite3")).unwrap();
        (dir, store)
    }

    fn new_resource(store: &mut Store, name: &str, quantity: u32) -> Resource {
        let uid = store
            .save_resource(
                &Resource {
                    name: name.into(),
                    kind: "equipamento".into(),
                    quantity,
                    ..Resource::default()
                },
                None,
            )
            .unwrap();
        store
            .list_resources()
            .unwrap()
            .into_iter()
            .find(|r| r.uid == uid)
            .unwrap()
    }

    fn use_of(resource: &Resource, who: &str, start: &str, end: &str) -> ResourceUse {
        ResourceUse {
            resource_uid: resource.uid.clone(),
            who: who.into(),
            start: at(FAR, start),
            end: at(FAR, end),
            ..ResourceUse::default()
        }
    }

    fn found(store: &Store, uid: &str) -> ResourceUse {
        store
            .list_resource_uses()
            .unwrap()
            .into_iter()
            .find(|u| u.uid == uid)
            .unwrap()
    }

    #[test]
    fn moments_are_read_strictly_and_shown_as_people_write_them() {
        assert!(parse_moment("2026-11-10T14:30").is_ok());
        for bad in [
            "",
            "2026-11-10",
            "2026-11-10 14:30",
            "2026-11-10T14:30:00",
            "2026-1-10T14:30",
            "2026-11-10T24:00",
            "2026-02-30T10:00",
            "10/11/2026 14:30",
        ] {
            assert!(parse_moment(bad).is_err(), "{bad}");
        }
        let moment = parse_moment("2026-11-10T09:05").unwrap();
        assert_eq!(format_moment(moment), "2026-11-10T09:05");
        assert!(parse_moment(&now_moment()).is_ok());
        assert_eq!(moment_label("2026-11-10T09:05"), "10/11/2026 09:05");
        assert_eq!(moment_label("sem data"), "sem data");
        assert_eq!(moment_date("2026-11-10T09:05"), "2026-11-10");
        assert_eq!(moment_time("2026-11-10T09:05"), "09:05");
        assert_eq!(
            hours_between("2026-11-10T09:00", "2026-11-10T10:30"),
            Some(1.5)
        );
        assert_eq!(
            hours_between("2026-11-10T10:00", "2026-11-10T09:00"),
            Some(0.0)
        );
        assert_eq!(hours_between("x", "2026-11-10T09:00"), None);
    }

    #[test]
    fn intervals_are_half_open_so_borders_do_not_clash() {
        let resource = room();
        let held = booking("b1", "2026-11-20T09:00", "2026-11-20T10:00");
        let uses = vec![held.clone()];
        let candidate = |start: &str, end: &str| booking("novo", start, end);
        let clashing = |start: &str, end: &str| {
            clashes(
                &resource,
                &uses,
                &candidate(&format!("2026-11-20T{start}"), &format!("2026-11-20T{end}")),
                NOW,
            )
            .len()
        };
        assert_eq!(clashing("09:30", "10:30"), 1, "overlaps the end");
        assert_eq!(clashing("08:30", "09:30"), 1, "overlaps the start");
        assert_eq!(clashing("09:15", "09:45"), 1, "inside");
        assert_eq!(clashing("08:00", "11:00"), 1, "around");
        assert_eq!(clashing("09:00", "10:00"), 1, "the same");
        assert_eq!(clashing("10:00", "11:00"), 0, "starts when the other ends");
        assert_eq!(clashing("08:00", "09:00"), 0, "ends when the other starts");
        assert_eq!(clashing("11:00", "12:00"), 0, "after");
        // The use being edited does not clash with itself.
        let mut same = held.clone();
        same.end = "2026-11-20T10:30".into();
        assert!(clashes(&resource, &uses, &same, NOW).is_empty());
        // What is returned or cancelled takes nothing, on either side.
        for status in [UseStatus::Returned, UseStatus::Cancelled] {
            let mut gone = held.clone();
            gone.status = status;
            gone.returned_at = Some("2026-11-20T09:30".into());
            let uses = vec![gone.clone()];
            let overlapping = candidate("2026-11-20T09:30", "2026-11-20T10:30");
            assert!(clashes(&resource, &uses, &overlapping, NOW).is_empty());
            assert!(clashes(&resource, std::slice::from_ref(&held), &gone, NOW).is_empty());
        }
        // Another resource's uses are none of its business.
        let mut elsewhere = held.clone();
        elsewhere.resource_uid = "b".repeat(32);
        assert!(clashes(
            &resource,
            &[elsewhere],
            &candidate("2026-11-20T09:00", "2026-11-20T10:00"),
            NOW
        )
        .is_empty());
    }

    #[test]
    fn quantity_lets_uses_run_in_parallel_up_to_the_total() {
        let mut resource = room();
        resource.quantity = 3;
        let mut two = booking("u1", "2026-11-20T09:00", "2026-11-20T11:00");
        two.quantity = 2;
        let one = booking("u2", "2026-11-20T10:00", "2026-11-20T12:00");
        let uses = vec![two.clone(), one.clone()];
        let candidate = |start: &str, end: &str| {
            booking(
                "novo",
                &format!("2026-11-20T{start}"),
                &format!("2026-11-20T{end}"),
            )
        };
        assert!(clashes(&resource, &uses, &candidate("09:00", "10:00"), NOW).is_empty());
        let both = clashes(&resource, &uses, &candidate("10:00", "11:00"), NOW);
        assert_eq!(both.len(), 2, "all that hold it at the full moment");
        assert_eq!(both[0].uid, "u1");
        assert!(clashes(&resource, &uses, &candidate("11:00", "12:00"), NOW).is_empty());
        // Two units do not fit where one is left.
        let mut pair = candidate("09:00", "10:00");
        pair.quantity = 2;
        assert_eq!(clashes(&resource, &uses, &pair, NOW).len(), 1);
        let free = |start: &str, end: &str, except: &str| {
            free_quantity(
                &resource,
                &uses,
                &format!("2026-11-20T{start}"),
                &format!("2026-11-20T{end}"),
                except,
                NOW,
            )
        };
        assert_eq!(free("08:00", "09:00", ""), 3);
        assert_eq!(free("09:00", "10:00", ""), 1);
        assert_eq!(free("10:00", "11:00", ""), 0);
        assert_eq!(free("09:00", "12:00", ""), 0, "the worst moment counts");
        assert_eq!(
            free("09:00", "12:00", "u1"),
            2,
            "without the one being edited"
        );
        assert_eq!(free("12:00", "13:00", ""), 3);
    }

    #[test]
    fn an_item_in_use_is_out_until_it_comes_back() {
        let resource = room();
        let mut loan = booking("l1", "2026-11-10T09:00", "2026-11-10T10:00");
        loan.status = UseStatus::InUse;
        // Not yet at its end: until the end.
        assert_eq!(
            loan.occupies_at("2026-11-10T09:30"),
            Some(("2026-11-10T09:00".into(), "2026-11-10T10:00".into()))
        );
        assert!(!loan.overdue_at("2026-11-10T09:30"));
        assert!(!loan.overdue_at("2026-11-10T10:00"), "exactly at the end");
        // Past its end: still out, until now.
        assert_eq!(
            loan.occupies_at(NOW),
            Some(("2026-11-10T09:00".into(), NOW.into()))
        );
        assert!(loan.overdue_at(NOW));
        let uses = vec![loan.clone()];
        let during = booking("n1", "2026-11-10T10:30", "2026-11-10T11:00");
        assert_eq!(clashes(&resource, &uses, &during, NOW).len(), 1);
        let after = booking("n2", "2026-11-10T12:00", "2026-11-10T13:00");
        assert!(clashes(&resource, &uses, &after, NOW).is_empty());
        // The same loan, back: nothing is taken and nothing is late.
        loan.status = UseStatus::Returned;
        loan.returned_at = Some("2026-11-10T11:30".into());
        assert_eq!(loan.occupies_at(NOW), None);
        assert!(!loan.overdue_at(NOW));
        // A reservation is never "overdue", only a loan is.
        let reserved = booking("r1", "2026-11-09T09:00", "2026-11-09T10:00");
        assert!(!reserved.overdue_at(NOW));
        // The wording names who holds it and until when.
        let held = booking("h1", "2026-11-10T09:00", "2026-11-10T16:00");
        assert_eq!(
            clash_error(&resource, std::slice::from_ref(&held), NOW),
            "[horario-ocupado] O recurso “Sala de reuniões do 2º andar” já está reservado por Servidor B até 10/11/2026 16:00."
        );
        let more = clash_sentence(&resource, &[held.clone(), held.clone(), held], NOW);
        assert!(
            more.ends_with("16:00 (e mais 2 usos nesse horário)."),
            "{more}"
        );
        assert_eq!(strip_marker("[horario-ocupado] Frase."), "Frase.",);
    }

    #[test]
    fn days_count_what_is_booked_today_and_what_is_late() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let reserved = |uid: &str, start: &str, end: &str| booking(uid, start, end);
        let mut late = reserved("late", "2026-11-03T09:00", "2026-11-03T10:00");
        late.status = UseStatus::InUse;
        let mut returned = reserved("back", "2026-11-10T09:00", "2026-11-10T10:00");
        returned.status = UseStatus::Returned;
        returned.returned_at = Some("2026-11-10T09:50".into());
        let mut running = reserved("run", "2026-11-10T08:00", "2026-11-10T18:00");
        running.status = UseStatus::InUse;
        let uses = vec![
            reserved("in", "2026-11-10T09:00", "2026-11-10T10:00"),
            reserved("ends-at-midnight", "2026-11-09T23:00", "2026-11-10T00:00"),
            reserved("crosses-midnight", "2026-11-10T22:00", "2026-11-11T01:00"),
            reserved("tomorrow", "2026-11-11T09:00", "2026-11-11T10:00"),
            late,
            returned,
            running,
        ];
        let counts = day_counts(&uses, today, NOW);
        assert_eq!(
            counts,
            DayCounts {
                // in, crosses-midnight, the loan still out (until now), the
                // one running; not the one that ended at midnight, tomorrow's
                // or the returned one.
                reservations_today: 4,
                overdue: 1,
            }
        );
        assert_eq!(day_counts(&[], today, NOW), DayCounts::default());
    }

    #[test]
    fn the_quantity_cannot_go_below_what_current_and_future_uses_need() {
        let mut one = booking("u1", "2026-11-20T09:00", "2026-11-20T11:00");
        one.quantity = 2;
        let mut two = booking("u2", "2026-11-20T10:00", "2026-11-20T12:00");
        two.quantity = 1;
        let mut past = booking("old", "2026-11-01T09:00", "2026-11-01T11:00");
        past.quantity = 9;
        let mut cancelled = booking("c", "2026-11-20T10:00", "2026-11-20T12:00");
        cancelled.quantity = 9;
        cancelled.status = UseStatus::Cancelled;
        let uses = vec![one, two, past, cancelled];
        assert_eq!(quantity_blocker(&uses, 3, NOW), None);
        let (at, needed, blocking) = quantity_blocker(&uses, 2, NOW).unwrap();
        assert_eq!((at.as_str(), needed), ("2026-11-20T10:00", 3));
        assert_eq!(blocking.uid, "u2", "the one that tips it over");
        assert_eq!(
            quantity_blocker(&uses, 1, NOW).unwrap().0,
            "2026-11-20T09:00"
        );
        assert_eq!(quantity_blocker(&[], 1, NOW), None);
    }

    #[test]
    fn what_is_typed_is_checked_before_it_is_kept() {
        let mut resource = room();
        assert!(resource.validate().is_ok());
        resource.name = "   ".into();
        assert!(resource.validate().is_err());
        resource.name = "x".repeat(121);
        assert!(resource.validate().is_err());
        resource.name = "Projetor".into();
        // The kind is hidden: any short text is accepted, an empty one
        // becomes "outro", and a very long one is refused.
        resource.kind = "carro".into();
        assert!(resource.validate().is_ok());
        resource.kind = "x".repeat(41);
        assert!(resource.validate().is_err());
        resource.kind = "  ".into();
        assert_eq!(resource.normalized().kind, "outro");
        resource.kind = "equipamento".into();
        for quantity in [0, MAX_QUANTITY + 1] {
            resource.quantity = quantity;
            assert!(resource.validate().is_err());
        }
        resource.quantity = MAX_QUANTITY;
        assert!(resource.validate().is_ok());
        resource.location = "x".repeat(201);
        assert!(resource.validate().is_err());
        resource.location = "Sala 12\u{7}".into();
        assert!(resource.validate().is_err(), "control characters");
        resource.location.clear();
        resource.notes = "linha 1\nlinha 2".into();
        assert!(resource.validate().is_ok(), "notes may have line breaks");

        let ok = booking("u1", "2026-11-20T09:00", "2026-11-20T10:00");
        assert!(ok.validate().is_ok());
        let wrong = |change: &dyn Fn(&mut ResourceUse)| {
            let mut item = ok.clone();
            change(&mut item);
            item.validate().unwrap_err().to_string()
        };
        assert!(wrong(&|u| u.who = " ".into()).contains("quem"));
        assert!(wrong(&|u| u.who = "x".repeat(121)).contains("Quem"));
        assert!(wrong(&|u| u.sector = "x".repeat(121)).contains("Setor"));
        assert!(wrong(&|u| u.purpose = "x".repeat(301)).contains("Finalidade"));
        assert!(wrong(&|u| u.quantity = 0).contains("quantidade"));
        assert!(wrong(&|u| u.resource_uid.clear()).contains("recurso"));
        assert!(wrong(&|u| u.start = "20/11/2026 09:00".into()).contains("inválido"));
        assert!(wrong(&|u| u.end = u.start.clone()).contains("depois do início"));
        assert!(wrong(&|u| u.end = "2026-11-20T08:00".into()).contains("depois do início"));
        assert!(wrong(&|u| u.status = UseStatus::Returned).contains("devolvido"));
        assert!(wrong(&|u| {
            u.status = UseStatus::Returned;
            u.returned_at = Some("2026-11-20T08:59".into());
        })
        .contains("antes do início"));
        let mut back = ok.clone();
        back.status = UseStatus::Returned;
        back.returned_at = Some("2026-11-20T09:00".into());
        assert!(back.validate().is_ok(), "returned exactly at the start");
        // Other statuses clear the return.
        let mut reserved = ok.clone();
        reserved.returned_at = Some("2026-11-20T09:30".into());
        assert_eq!(reserved.normalized().returned_at, None);
        assert_eq!(back.normalized().returned_at, back.returned_at);
        assert_eq!(UseStatus::parse("em_uso").unwrap(), UseStatus::InUse);
        assert_eq!(UseStatus::InUse.as_str(), "em_uso");
        assert!(UseStatus::parse("x").is_err() && ResourceStatus::parse("x").is_err());
        assert_eq!(ResourceStatus::Inactive.label(), "Fora de uso");
        assert_eq!(kind_label("livro"), "Livro");
    }

    #[test]
    fn resources_and_uses_are_kept_listed_and_sealed_at_rest() {
        let (dir, mut store) = store();
        let projector = new_resource(&mut store, "Projetor MARCADOR_RECURSO_NOME", 1);
        assert_eq!(projector.uid.len(), 32);
        assert!(!projector.created_at.is_empty());
        assert_eq!(store.resource_version(&projector.uid).unwrap(), 1);
        assert_eq!(store.resource_version(&"f".repeat(32)).unwrap(), 0, "new");
        let auditorium = new_resource(&mut store, "Auditório", 1);
        let books = new_resource(&mut store, "manual de redação oficial", 3);
        let names: Vec<String> = store
            .list_resources()
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(
            names,
            [
                "Auditório",
                "manual de redação oficial",
                "Projetor MARCADOR_RECURSO_NOME"
            ],
            "by name, case aside"
        );
        let mut item = use_of(&projector, "Servidor A MARCADOR_USO_QUEM", "09:00", "10:00");
        item.purpose = "MARCADOR_USO_FINALIDADE".into();
        item.notes = "MARCADOR_USO_NOTA".into();
        item.sector = "Setor Administrativo".into();
        let uid = store.save_resource_use(&item, None).unwrap();
        let saved = found(&store, &uid);
        assert_eq!(saved.status, UseStatus::Reserved);
        assert!(!saved.created_by.is_empty() && !saved.created_at.is_empty());
        assert_eq!(store.resource_use_version(&uid).unwrap(), 1);
        // A later use of an earlier hour is listed first.
        let early = store
            .save_resource_use(&use_of(&auditorium, "Servidor C", "07:00", "08:00"), None)
            .unwrap();
        let order: Vec<String> = store
            .list_resource_uses()
            .unwrap()
            .into_iter()
            .map(|u| u.uid)
            .collect();
        assert_eq!(order, [early.clone(), uid.clone()]);
        // A use of a resource that is not here, or of more than it has.
        let mut orphan = item.clone();
        orphan.uid.clear();
        orphan.resource_uid = "e".repeat(32);
        assert!(store.save_resource_use(&orphan, None).is_err());
        let mut too_many = use_of(&books, "Servidor A", "09:00", "10:00");
        too_many.quantity = 4;
        assert!(store
            .save_resource_use(&too_many, None)
            .unwrap_err()
            .to_string()
            .contains("tem (3)"));
        // Changing keeps who made it and when.
        let mut changed = saved.clone();
        changed.created_by = "outra pessoa".into();
        changed.notes = "MARCADOR_USO_NOTA 2".into();
        store.save_resource_use(&changed, Some(1)).unwrap();
        let after = found(&store, &uid);
        assert_eq!(after.created_by, saved.created_by);
        assert_eq!(after.created_at, saved.created_at);
        assert_eq!(after.notes, "MARCADOR_USO_NOTA 2");
        store.delete_resource_use(&early, None).unwrap();
        assert_eq!(store.list_resource_uses().unwrap().len(), 1);
        assert!(store.delete_resource_use(&early, None).is_err());
        // The team log names the resource, never who or the notes.
        let events: Vec<String> = store
            .list_team_events(50)
            .unwrap()
            .into_iter()
            .map(|e| e.details)
            .collect();
        assert!(events.iter().any(|d| d.contains("Reservou \"Projetor")));
        assert!(events.iter().all(|d| !d.contains("MARCADOR_USO")));
        drop(store);
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            for marker in [
                "MARCADOR_RECURSO_NOME",
                "MARCADOR_USO_QUEM",
                "MARCADOR_USO_FINALIDADE",
                "MARCADOR_USO_NOTA",
                "Setor Administrativo",
                FAR,
            ] {
                assert!(
                    !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
                    "{marker} in clear"
                );
            }
        }
    }

    #[test]
    fn a_stale_expected_version_is_refused_and_nothing_changes() {
        let (_dir, mut store) = store();
        let mut resource = new_resource(&mut store, "Auditório", 1);
        resource.name = "Auditório novo".into();
        store.save_resource(&resource, Some(1)).unwrap();
        let stale = resource.clone();
        let error = store
            .save_resource(&stale, Some(1))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Conflito de edição simultânea"), "{error}");
        assert_eq!(store.list_resources().unwrap()[0].name, "Auditório novo");
        let uid = store
            .save_resource_use(&use_of(&resource, "Servidor A", "09:00", "10:00"), None)
            .unwrap();
        let mut item = found(&store, &uid);
        item.who = "Servidor B".into();
        store.save_resource_use(&item, Some(1)).unwrap();
        item.who = "Servidor C".into();
        let error = store
            .save_resource_use(&item, Some(1))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Conflito de edição simultânea"), "{error}");
        assert_eq!(found(&store, &uid).who, "Servidor B");
        assert_eq!(store.resource_use_version(&uid).unwrap(), 2);
    }

    #[test]
    fn a_resource_with_uses_is_marked_out_of_use_not_deleted() {
        let (_dir, mut store) = store();
        let used = new_resource(&mut store, "Auditório", 1);
        let unused = new_resource(&mut store, "Projetor", 1);
        let uid = store
            .save_resource_use(&use_of(&used, "Servidor A", "09:00", "10:00"), None)
            .unwrap();
        let error = store
            .delete_resource(&used.uid, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Fora de uso"), "{error}");
        assert_eq!(store.list_resources().unwrap().len(), 2);
        store.delete_resource(&unused.uid, None).unwrap();
        assert_eq!(store.list_resources().unwrap().len(), 1);
        assert!(store.delete_resource(&unused.uid, None).is_err());
        // Out of use: the history stays, new bookings are refused, and what
        // was already made can still be returned.
        let mut out = used.clone();
        out.status = ResourceStatus::Inactive;
        store.save_resource(&out, None).unwrap();
        let error = store
            .save_resource_use(&use_of(&out, "Servidor B", "11:00", "12:00"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Fora de uso"), "{error}");
        let mut kept = found(&store, &uid);
        kept.notes = "ainda vale".into();
        store.save_resource_use(&kept, None).unwrap();
        kept.status = UseStatus::Returned;
        kept.returned_at = Some(at(FAR, "09:30"));
        store.save_resource_use(&kept, None).unwrap();
        // A returned or cancelled one may even be recorded for it, but not
        // brought back.
        let mut gone = use_of(&out, "Servidor B", "11:00", "12:00");
        gone.status = UseStatus::Cancelled;
        let gone_uid = store.save_resource_use(&gone, None).unwrap();
        let mut back = found(&store, &gone_uid);
        back.status = UseStatus::Reserved;
        assert!(store.save_resource_use(&back, None).is_err());
        store.delete_resource_use(&uid, None).unwrap();
        store.delete_resource_use(&gone_uid, None).unwrap();
        store.delete_resource(&used.uid, None).unwrap();
        assert!(store.list_resources().unwrap().is_empty());
    }

    #[test]
    fn the_quantity_cannot_be_lowered_below_what_the_uses_need() {
        let (_dir, mut store) = store();
        let chairs = new_resource(&mut store, "Cadeiras empilháveis", 40);
        let mut item = use_of(&chairs, "Servidor A", "09:00", "10:00");
        item.quantity = 30;
        store.save_resource_use(&item, None).unwrap();
        let mut lower = chairs.clone();
        lower.quantity = 20;
        let error = store.save_resource(&lower, None).unwrap_err().to_string();
        assert!(
            error.contains("Servidor A") && error.contains("precisar de 30"),
            "{error}"
        );
        assert_eq!(store.list_resources().unwrap()[0].quantity, 40);
        lower.quantity = 30;
        store.save_resource(&lower, None).unwrap();
        // A use of more than the resource has is refused; raising is free.
        item.uid.clear();
        item.quantity = 31;
        assert!(store.save_resource_use(&item, None).is_err());
        lower.quantity = 50;
        store.save_resource(&lower, None).unwrap();
        // Once it is returned, the quantity may go down.
        let uid = store.list_resource_uses().unwrap()[0].uid.clone();
        let mut back = found(&store, &uid);
        back.status = UseStatus::Returned;
        back.returned_at = Some(at(FAR, "09:30"));
        store.save_resource_use(&back, None).unwrap();
        lower.quantity = 1;
        store.save_resource(&lower, None).unwrap();
    }

    #[test]
    fn a_booking_that_does_not_fit_is_refused_inside_the_write() {
        let (_dir, mut store) = store();
        let room = new_resource(&mut store, "Sala de reuniões do 2º andar", 1);
        let first = store
            .save_resource_use(&use_of(&room, "Servidor B", "09:00", "10:00"), None)
            .unwrap();
        let error = store
            .save_resource_use(&use_of(&room, "Servidor C", "09:30", "10:30"), None)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(HORARIO_OCUPADO), "{error}");
        assert!(
            error.ends_with("já está reservado por Servidor B até 05/01/2099 10:00."),
            "{error}"
        );
        assert_eq!(store.list_resource_uses().unwrap().len(), 1, "nothing kept");
        // Border, another day and another resource are fine.
        store
            .save_resource_use(&use_of(&room, "Servidor C", "10:00", "11:00"), None)
            .unwrap();
        let other = new_resource(&mut store, "Auditório", 1);
        store
            .save_resource_use(&use_of(&other, "Servidor C", "09:00", "10:00"), None)
            .unwrap();
        // A loan "right now" takes it as well.
        let started = format_moment(chrono::Local::now().naive_local());
        let mut now_use = ResourceUse {
            resource_uid: other.uid.clone(),
            who: "Servidor A".into(),
            start: started.clone(),
            end: format_moment(chrono::Local::now().naive_local() + Duration::hours(2)),
            status: UseStatus::InUse,
            ..ResourceUse::default()
        };
        let uid = store.save_resource_use(&now_use, None).unwrap();
        now_use.uid.clear();
        assert!(store
            .save_resource_use(&now_use, None)
            .unwrap_err()
            .to_string()
            .contains("em uso por Servidor A"));
        // Cancelling the first frees its slot; editing it again re-checks.
        let mut cancel = found(&store, &first);
        cancel.status = UseStatus::Cancelled;
        store.save_resource_use(&cancel, None).unwrap();
        store
            .save_resource_use(&use_of(&room, "Servidor C", "09:30", "09:45"), None)
            .unwrap();
        cancel.status = UseStatus::Reserved;
        let error = store
            .save_resource_use(&cancel, None)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(HORARIO_OCUPADO), "{error}");
        let _ = uid;
    }

    #[test]
    fn editing_a_loan_that_is_overdue_is_not_blocked_by_later_bookings() {
        let (_dir, mut store) = store();
        let projector = new_resource(&mut store, "Projetor", 1);
        let past = chrono::Local::now().naive_local() - Duration::hours(5);
        let loan = ResourceUse {
            resource_uid: projector.uid.clone(),
            who: "Servidor A".into(),
            start: format_moment(past),
            end: format_moment(past + Duration::hours(1)),
            status: UseStatus::InUse,
            ..ResourceUse::default()
        };
        // Registering it now, already late, clashes with nothing here.
        let uid = store.save_resource_use(&loan, None).unwrap();
        assert_eq!(store.list_resource_uses().unwrap()[0].uid, uid);
        // A later booking that ends before the loan was taken back counts as
        // a clash (it is still out)...
        let mut between = loan.clone();
        between.uid.clear();
        between.who = "Servidor B".into();
        between.status = UseStatus::Reserved;
        between.start = format_moment(past + Duration::hours(2));
        between.end = format_moment(past + Duration::hours(3));
        assert!(store
            .save_resource_use(&between, None)
            .unwrap_err()
            .to_string()
            .starts_with(HORARIO_OCUPADO));
        // ...and a note on the loan itself never is.
        let mut noted = found(&store, &uid);
        noted.notes = "Devolve amanhã".into();
        store.save_resource_use(&noted, None).unwrap();
        // Handing it back frees everything.
        noted.status = UseStatus::Returned;
        noted.returned_at = Some(format_moment(chrono::Local::now().naive_local()));
        store.save_resource_use(&noted, None).unwrap();
        store.save_resource_use(&between, None).unwrap();
    }

    #[test]
    fn links_follow_the_document_and_vanish_with_it() {
        let (_dir, mut store) = store();
        let projector = new_resource(&mut store, "Projetor", 1);
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let task = store.list_tasks().unwrap().remove(0);
        let attendance = store.list_saiop_attendances().unwrap().remove(0);
        let mut item = use_of(&projector, "Servidor A", "09:00", "10:00");
        item.document_id = Some(receipt.document_id);
        item.task_id = Some(task.id);
        item.saiop_id = Some(attendance.id);
        let uid = store.save_resource_use(&item, None).unwrap();
        let saved = found(&store, &uid);
        assert_eq!(saved.document_id, Some(receipt.document_id));
        assert_eq!(saved.task_id, Some(task.id));
        assert_eq!(saved.saiop_id, Some(attendance.id));
        // A link to something that is not there is not kept.
        let mut stray = use_of(&projector, "Servidor A", "11:00", "12:00");
        stray.document_id = Some(9999);
        stray.task_id = Some(9999);
        stray.saiop_id = Some(9999);
        let stray_uid = store.save_resource_use(&stray, None).unwrap();
        let stray = found(&store, &stray_uid);
        assert_eq!(
            (stray.document_id, stray.task_id, stray.saiop_id),
            (None, None, None)
        );
        // Deleting the document leaves the use, without the links.
        store.delete_document(receipt.document_id).unwrap();
        let after = found(&store, &uid);
        assert_eq!(after.document_id, None);
        assert_eq!(after.task_id, None, "its task went with it");
        assert_eq!(after.saiop_id, Some(attendance.id), "the atendimento stays");
        assert_eq!(after.who, "Servidor A");
    }

    #[test]
    fn closed_uses_and_unchanged_ones_keep_their_quantity_when_the_resource_is_lowered() {
        let (_dir, mut store) = store();
        let chairs = new_resource(&mut store, "Cadeiras empilháveis", 5);
        // A use of 3 that was handed back, and a reservation of 3 that
        // ended in the past: neither stops the quantity going down to 2.
        let mut back = use_of(&chairs, "Servidor A", "09:00", "10:00");
        back.quantity = 3;
        let back_uid = store.save_resource_use(&back, None).unwrap();
        let mut done = found(&store, &back_uid);
        done.status = UseStatus::Returned;
        done.returned_at = Some(at(FAR, "09:30"));
        store.save_resource_use(&done, None).unwrap();
        let mut old = back.clone();
        old.uid.clear();
        old.start = "2020-01-05T09:00".into();
        old.end = "2020-01-05T10:00".into();
        let old_uid = store.save_resource_use(&old, None).unwrap();
        let mut lower = chairs.clone();
        lower.quantity = 2;
        store.save_resource(&lower, None).unwrap();
        // Their notes can still be edited, and a closed use may be changed.
        let mut noted = found(&store, &back_uid);
        noted.notes = "Devolvidas no armário".into();
        store.save_resource_use(&noted, None).unwrap();
        noted.quantity = 4;
        store.save_resource_use(&noted, None).unwrap();
        let mut noted_old = found(&store, &old_uid);
        noted_old.notes = "Evento passado".into();
        store.save_resource_use(&noted_old, None).unwrap();
        assert_eq!(found(&store, &old_uid).quantity, 3);
        // What takes the resource now still has to fit.
        noted_old.end = "2020-01-05T11:00".into();
        let error = store
            .save_resource_use(&noted_old, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("tem (2)"), "{error}");
        let mut fresh = use_of(&chairs, "Servidor B", "09:00", "10:00");
        fresh.quantity = 3;
        assert!(store.save_resource_use(&fresh, None).is_err());
        // Bringing a closed use back also needs it to fit.
        let mut reopened = found(&store, &back_uid);
        reopened.status = UseStatus::Reserved;
        assert!(store.save_resource_use(&reopened, None).is_err());
    }

    #[test]
    fn a_loan_past_its_end_is_said_to_be_late_not_free_in_a_minute() {
        let resource = room();
        let mut loan = booking("l1", "2026-11-11T09:00", "2026-11-11T17:00");
        loan.status = UseStatus::InUse;
        let late = "2026-11-11T18:00";
        assert_eq!(
            clash_sentence(&resource, std::slice::from_ref(&loan), late),
            "O recurso “Sala de reuniões do 2º andar” já está em uso por Servidor B, com devolução atrasada desde 11/11/2026 17:00."
        );
        let more = clash_sentence(&resource, &[loan.clone(), loan.clone()], late);
        assert!(
            more.ends_with("desde 11/11/2026 17:00 (e mais 1 uso nesse horário)."),
            "{more}"
        );
        // Not late yet: until its end, as before.
        assert_eq!(
            clash_sentence(&resource, &[loan], "2026-11-11T16:00"),
            "O recurso “Sala de reuniões do 2º andar” já está em uso por Servidor B até 11/11/2026 17:00."
        );
    }

    #[test]
    fn the_hidden_kind_is_kept_as_it_is_and_never_makes_a_difference() {
        let (_dir, mut store) = store();
        // A resource made by a 0.33.0 computer, with a kind chosen there.
        let mut old = Resource {
            name: "Auditório".into(),
            kind: "sala".into(),
            ..Resource::default()
        };
        let uid = store.save_resource(&old, None).unwrap();
        old.uid = uid.clone();
        assert_eq!(store.list_resources().unwrap()[0].kind, "sala");
        // Edited here (the screens hand the record back as it was read).
        let mut edited = store.list_resources().unwrap().remove(0);
        edited.location = "Térreo".into();
        store.save_resource(&edited, Some(1)).unwrap();
        let saved = store.list_resources().unwrap().remove(0);
        assert_eq!(
            (saved.kind.as_str(), saved.location.as_str()),
            ("sala", "Térreo")
        );
        // A kind this version has never heard of travels untouched.
        let mut strange = saved.clone();
        strange.kind = "veículo".into();
        store.save_resource(&strange, None).unwrap();
        assert_eq!(store.list_resources().unwrap()[0].kind, "veículo");
        // New ones are written as "outro".
        let fresh = new_resource(&mut store, "Projetor", 1);
        assert_eq!(fresh.kind, "equipamento", "this helper sets one on purpose");
        let plain = store
            .save_resource(
                &Resource {
                    name: "Chave do arquivo".into(),
                    ..Resource::default()
                },
                None,
            )
            .unwrap();
        let plain = store
            .list_resources()
            .unwrap()
            .into_iter()
            .find(|r| r.uid == plain)
            .unwrap();
        assert_eq!(plain.kind, "outro");
        // Only the kind differs: the same content.
        let mut other = saved.clone();
        other.kind = "livro".into();
        assert!(saved.same_content(&other));
        other.name = "Outro nome".into();
        assert!(!saved.same_content(&other));
    }
}
