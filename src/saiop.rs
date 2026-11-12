//! Área operacional "Atendimentos" (Controle do gabinete do SAIOP) e importador `.xlsx` da aba `PROCESSOS`.
//! Preserva atendimentos/passagens pelo gabinete independentes de captura, quatro dimensões de situação/prioridade,
//! indicadores calculados por eventos (sem depender de `DATEDIF` da planilha), rastreabilidade de lote e reversão.

use crate::{
    model::Capture,
    protection::{seal, unseal},
    storage::Store,
    tasks::Task,
};
use anyhow::{bail, ensure, Context, Result};
use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Utc};
#[cfg(feature = "desktop")]
use eframe::egui::{self, RichText};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const STATUS_OPTIONS: &[(&str, &str)] = &[
    ("a_distribuir", "A distribuir"),
    ("em_analise", "Em análise"),
    ("em_elaboracao", "Em elaboração"),
    ("aguardando_deliberacao", "Aguardando deliberação"),
    ("aguardando_resposta", "Aguardando resposta"),
    ("suspenso", "Suspenso (Stand By)"),
    ("encerrado", "Encerrado"),
    ("cancelado", "Cancelado"),
];

pub const PRIORITY_OPTIONS: &[(&str, &str)] = &[
    ("baixa", "Baixa"),
    ("normal", "Normal"),
    ("alta", "Alta"),
    ("urgente", "Urgente"),
];

pub const TRI_STATE_OPTIONS: &[(&str, &str)] = &[
    ("nao_informado", "Não informado"),
    ("sim", "Sim"),
    ("nao", "Não"),
];

pub const DUE_KIND_OPTIONS: &[(&str, &str)] = &[
    ("oficial", "Prazo oficial"),
    ("meta_interna", "Meta interna"),
    ("cobranca_retorno", "Cobrança de retorno"),
    ("a_confirmar", "Texto / a confirmar"),
];

pub const OUTCOME_OPTIONS: &[(&str, &str)] = &[
    ("", "Em aberto / sem desfecho de etapa"),
    ("encaminhado", "Encaminhado / Tramitado"),
    ("arquivado_local", "Arquivado no controle local"),
    ("respondido", "Respondido"),
    ("ciencia_registrada", "Ciência registrada"),
];

pub const DOC_TYPE_OPTIONS: &[&str] = &[
    "",
    "Despacho",
    "CI",
    "Ofício",
    "Informação Técnica",
    "Parecer",
    "Nota Técnica",
    "Termo",
];

pub fn status_label(status: &str) -> &str {
    STATUS_OPTIONS
        .iter()
        .find(|(k, _)| *k == status)
        .map(|(_, label)| *label)
        .unwrap_or(status)
}

pub fn priority_label(priority: &str) -> &str {
    PRIORITY_OPTIONS
        .iter()
        .find(|(k, _)| *k == priority)
        .map(|(_, label)| *label)
        .unwrap_or(priority)
}

pub fn tri_state_label(value: &str) -> &str {
    TRI_STATE_OPTIONS
        .iter()
        .find(|(k, _)| *k == value)
        .map(|(_, label)| *label)
        .unwrap_or(value)
}

pub fn due_kind_label(value: &str) -> &str {
    DUE_KIND_OPTIONS
        .iter()
        .find(|(k, _)| *k == value)
        .map(|(_, label)| *label)
        .unwrap_or(value)
}

pub fn outcome_label(value: &str) -> &str {
    OUTCOME_OPTIONS
        .iter()
        .find(|(k, _)| *k == value)
        .map(|(_, label)| *label)
        .unwrap_or(value)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaiopEvent {
    pub when: String,
    pub actor: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSpreadsheetRow {
    pub sheet_name: String,
    pub row_number: usize,
    pub cells: BTreeMap<String, String>,
    pub formulas: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaiopAttendance {
    pub id: i64,
    #[serde(default)]
    pub batch_id: Option<i64>,
    #[serde(default)]
    pub source_row: Option<usize>,
    pub reference_raw: String,
    pub reference_norm: String,
    #[serde(default)]
    pub sigadoc_code: String,
    pub subject: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub entry_date: String,
    #[serde(default)]
    pub entry_date_raw: String,
    pub assignee: String,
    #[serde(default)]
    pub received_from: String,
    #[serde(default = "default_tri_state")]
    pub uas_return: String,
    #[serde(default)]
    pub uas_raw: String,
    #[serde(default)]
    pub due_date: String,
    #[serde(default)]
    pub due_date_raw: String,
    #[serde(default = "default_due_kind")]
    pub due_kind: String,
    #[serde(default)]
    pub observation: String,
    #[serde(default)]
    pub verification_raw: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default)]
    pub priority_reason: String,
    #[serde(default = "default_tri_state")]
    pub signed_status: String,
    #[serde(default)]
    pub exit_date: String,
    #[serde(default)]
    pub exit_date_raw: String,
    #[serde(default)]
    pub planned_doc_type: String,
    #[serde(default)]
    pub sent_to: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub scheduled: bool,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub attachment_relation: String,
    #[serde(default)]
    pub review_flags: Vec<String>,
    #[serde(default)]
    pub document_id: Option<i64>,
    #[serde(default)]
    pub task_ids: Vec<i64>,
    pub stage_changed_at: String,
    #[serde(default)]
    pub closed_at: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub human_edited: bool,
    /// A spreadsheet row that a SIGADOC import later linked to its
    /// document: it is that document's atendimento now, so undoing the
    /// spreadsheet keeps it.
    #[serde(default)]
    pub linked_by_import: bool,
    #[serde(default)]
    pub events: Vec<SaiopEvent>,
    /// Ids of the proposals of analyses this atendimento was made or
    /// completed from (schema 16).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposal_ids: Vec<String>,
}

fn default_tri_state() -> String {
    "nao_informado".into()
}

fn default_due_kind() -> String {
    "oficial".into()
}

fn default_status() -> String {
    "a_distribuir".into()
}

fn default_priority() -> String {
    "normal".into()
}

impl SaiopAttendance {
    pub fn new_manual() -> Self {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let now = Utc::now().to_rfc3339();
        Self {
            id: 0,
            batch_id: None,
            source_row: None,
            reference_raw: String::new(),
            reference_norm: String::new(),
            sigadoc_code: String::new(),
            subject: String::new(),
            tags: Vec::new(),
            entry_date: today.clone(),
            entry_date_raw: today.clone(),
            assignee: String::new(),
            received_from: String::new(),
            uas_return: default_tri_state(),
            uas_raw: String::new(),
            due_date: String::new(),
            due_date_raw: String::new(),
            due_kind: default_due_kind(),
            observation: String::new(),
            verification_raw: String::new(),
            status: "em_analise".into(),
            priority: "normal".into(),
            priority_reason: String::new(),
            signed_status: default_tri_state(),
            exit_date: String::new(),
            exit_date_raw: String::new(),
            planned_doc_type: String::new(),
            sent_to: String::new(),
            outcome: String::new(),
            scheduled: false,
            sensitive: false,
            attachment_relation: String::new(),
            review_flags: Vec::new(),
            document_id: None,
            task_ids: Vec::new(),
            stage_changed_at: today,
            closed_at: String::new(),
            created_at: now.clone(),
            updated_at: now,
            human_edited: true,
            linked_by_import: false,
            events: Vec::new(),
            proposal_ids: Vec::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        !matches!(self.status.as_str(), "encerrado" | "cancelado")
    }

    /// Idade do atendimento: dias corridos da entrada até o encerramento confirmado, ou até `today` enquanto aberto.
    pub fn age_days(&self, today: NaiveDate) -> Option<i64> {
        let start = NaiveDate::parse_from_str(self.entry_date.trim(), "%Y-%m-%d").ok()?;
        let end = if !self.is_open() {
            NaiveDate::parse_from_str(self.closed_at.trim(), "%Y-%m-%d").unwrap_or(today)
        } else {
            today
        };
        Some((end - start).num_days().max(0))
    }

    /// Tempo na etapa atual: dias desde a última mudança de etapa (`stage_changed_at` ou `entry_date`).
    pub fn stage_days(&self, today: NaiveDate) -> Option<i64> {
        let base = NaiveDate::parse_from_str(self.stage_changed_at.trim(), "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(self.entry_date.trim(), "%Y-%m-%d"))
            .ok()?;
        let end = if !self.is_open() {
            NaiveDate::parse_from_str(self.closed_at.trim(), "%Y-%m-%d").unwrap_or(today)
        } else {
            today
        };
        Some((end - base).num_days().max(0))
    }

    /// Espera externa: período desde o envio que exige retorno até hoje (enquanto aguardando retorno).
    pub fn external_wait_days(&self, today: NaiveDate) -> Option<i64> {
        if !self.is_open() {
            return None;
        }
        if self.status != "aguardando_resposta" && self.uas_return != "sim" {
            return None;
        }
        let sent = NaiveDate::parse_from_str(self.exit_date.trim(), "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(self.stage_changed_at.trim(), "%Y-%m-%d"))
            .ok()?;
        Some((today - sent).num_days().max(0))
    }

    /// Atraso em dias em relação ao prazo confirmado (`> 0` significa vencido, `0` vence hoje, `< 0` dias restantes).
    pub fn due_delta_days(&self, today: NaiveDate) -> Option<i64> {
        if !self.is_open() {
            return None;
        }
        let due = NaiveDate::parse_from_str(self.due_date.trim(), "%Y-%m-%d").ok()?;
        Some((today - due).num_days())
    }

    pub fn composed_sigadoc_url(&self) -> Option<String> {
        let candidate = if !self.sigadoc_code.trim().is_empty() {
            self.sigadoc_code.trim()
        } else {
            self.reference_norm.trim()
        };
        if candidate.is_empty() {
            return None;
        }
        if candidate.starts_with("https://www.sigadoc.mt.gov.br/")
            || candidate.starts_with("https://sigadoc.mt.gov.br/")
        {
            return Some(candidate.to_owned());
        }
        // Only compose safe HTTPS SIGADOC URL when code has standard characters
        if candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | '_'))
            && candidate.contains('-')
        {
            Some(format!(
                "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla={candidate}"
            ))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaiopImportBatch {
    pub id: i64,
    pub file_name: String,
    pub file_sha256: String,
    pub sheet_name: String,
    pub imported_at: String,
    pub actor: String,
    pub total_rows: usize,
    pub flagged_rows: usize,
}

/// An import whose automatic atendimento could not be recorded.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoAttendanceFailure {
    #[serde(default)]
    pub document_id: i64,
    pub number: String,
    pub message: String,
    pub at: String,
}

#[derive(Clone, Debug)]
pub struct SaiopImportPreview {
    pub file_name: String,
    pub file_sha256: String,
    pub sheet_name: String,
    pub headers: Vec<String>,
    pub items: Vec<(SaiopAttendance, RawSpreadsheetRow)>,
    pub ignored_formula_rows: usize,
    pub repeated_references: usize,
    pub flagged_count: usize,
    pub linked_documents: usize,
    pub tramitado_keep_open: bool,
    /// For each item, the id of an attendance already in the control with the
    /// same reference and entry date (a re-import of an updated spreadsheet).
    pub already_registered: Vec<Option<i64>>,
}

impl SaiopImportPreview {
    pub fn new_rows(&self) -> usize {
        self.already_registered
            .iter()
            .filter(|r| r.is_none())
            .count()
    }
    /// Copy of the preview keeping only the rows not yet in the control.
    pub fn only_new(&self) -> Self {
        let keep: Vec<bool> = self
            .already_registered
            .iter()
            .map(|r| r.is_none())
            .collect();
        let items: Vec<_> = self
            .items
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(item, _)| item.clone())
            .collect();
        Self {
            flagged_count: items
                .iter()
                .filter(|(a, _)| !a.review_flags.is_empty())
                .count(),
            linked_documents: items
                .iter()
                .filter(|(a, _)| a.document_id.is_some())
                .count(),
            already_registered: vec![None; items.len()],
            items,
            ..self.clone()
        }
    }
}

pub fn normalize_reference(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// What the indexed `reference_norm` column stores: a digest of the
/// normalized reference, so that document numbers never sit in clear in
/// the library file. The readable value lives in the sealed payload.
pub fn reference_key(normalized: &str) -> String {
    if normalized.is_empty() {
        return String::new();
    }
    let mut hasher = Sha256::new();
    hasher.update(b"ponte-saiop-reference\n");
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Mapeia o rótulo original da coluna M (`Verificação`) nas 4 dimensões independentes do Ponte.
pub fn map_verification_dimensions(
    raw_label: &str,
    tramitado_keep_open_if_return: bool,
    uas_return: &str,
) -> (String, String, String, bool, bool, String) {
    let norm = raw_label.trim().to_uppercase();
    let mut status = "em_analise".to_owned();
    let mut priority = "normal".to_owned();
    let mut outcome = String::new();
    let mut scheduled = false;
    let mut sensitive = false;
    let mut attachment_relation = String::new();

    if norm.is_empty() {
        status = "a_distribuir".into();
    } else if norm.contains("TRAMITADO") {
        outcome = "encaminhado".into();
        if tramitado_keep_open_if_return && uas_return == "sim" {
            status = "aguardando_resposta".into();
        } else {
            status = "encerrado".into();
        }
    } else if norm.contains("ARQUIVADO") {
        outcome = "arquivado_local".into();
        status = "encerrado".into();
    } else if norm.contains("DELIBERA") {
        status = "aguardando_deliberacao".into();
    } else if norm.contains("AGUARDANDO RESPOSTA") || norm.contains("AGUARDANDO RETORNO") {
        status = "aguardando_resposta".into();
    } else if norm.contains("STAND BY") || norm.contains("SUSPENSO") || norm.contains("SOBRESTADO")
    {
        status = "suspenso".into();
    } else if norm.contains("PENDENTE") || norm.contains("VERIFICA") {
        status = "a_distribuir".into();
    } else if norm.contains("ANDAMENTO") || norm.contains("ELABORA") {
        status = "em_elaboracao".into();
    }

    if norm.contains("URGENTE") {
        priority = "urgente".into();
    } else if norm.contains("PRAZO") || norm.contains("ALTA") {
        priority = "alta".into();
    }

    if norm.contains("AGENDADO") {
        scheduled = true;
    }
    if norm.contains("SENSÍVEL") || norm.contains("SENSIVEL") || norm.contains("SIGILOSO") {
        sensitive = true;
    }
    if norm.contains("APENSADO") {
        attachment_relation = "apensado".into();
    } else if norm.contains("JUNTADO") {
        attachment_relation = "juntado".into();
    }

    (
        status,
        priority,
        outcome,
        scheduled,
        sensitive,
        attachment_relation,
    )
}

/// Converte valor de célula de data do Excel (serial numérico ou texto `DD/MM/AAAA`) sem corrigir silenciosamente anos atípicos.
pub fn parse_spreadsheet_date(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    // Caso 1: Serial de data do Excel (ex.: 45658 ou 45658.0)
    if let Ok(serial_f) = trimmed.replace(',', ".").parse::<f64>() {
        let serial = serial_f.floor() as i64;
        if (10000..=80000).contains(&serial) {
            let base = NaiveDate::from_ymd_opt(1899, 12, 30).expect("valid base date");
            if let Some(date) = base.checked_add_signed(ChronoDuration::days(serial)) {
                let iso = date.format("%Y-%m-%d").to_string();
                let year = date.year();
                let flag = if !(2015..=2027).contains(&year) {
                    Some(format!("Data com ano atípico ({year}): '{trimmed}'"))
                } else {
                    None
                };
                return (Some(iso), flag);
            }
        }
    }
    // Caso 2: Data em formato ISO YYYY-MM-DD
    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let year = date.year();
        let flag = if !(2015..=2027).contains(&year) {
            Some(format!("Data com ano atípico ({year}): '{trimmed}'"))
        } else {
            None
        };
        return (Some(date.format("%Y-%m-%d").to_string()), flag);
    }
    // Caso 3: Data em formato brasileiro DD/MM/YYYY ou DD/MM/YY
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.len() == 3 {
        if let (Ok(d), Ok(m), Ok(y_raw)) = (
            parts[0].trim().parse::<u32>(),
            parts[1].trim().parse::<u32>(),
            parts[2].trim().parse::<i32>(),
        ) {
            if parts[2].trim().len() < 4 {
                return (
                    None,
                    Some(format!(
                        "Data em texto com ano incompleto ('{trimmed}') — confirmar antes de converter"
                    )),
                );
            }
            if let Some(date) = NaiveDate::from_ymd_opt(y_raw, m, d) {
                let iso = date.format("%Y-%m-%d").to_string();
                let flag = if !(2015..=2027).contains(&y_raw) {
                    Some(format!("Data com ano atípico ({y_raw}): '{trimmed}'"))
                } else {
                    None
                };
                return (Some(iso), flag);
            }
        }
    }
    (
        None,
        Some(format!(
            "Valor não reconhecido como data calendário ('{trimmed}')"
        )),
    )
}

impl Store {
    pub(crate) fn ensure_saiop(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS saiop_batches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_sha256 TEXT NOT NULL UNIQUE,
                payload BLOB NOT NULL,
                imported_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS saiop_attendances (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                batch_id INTEGER REFERENCES saiop_batches(id) ON DELETE SET NULL,
                source_row INTEGER,
                document_id INTEGER REFERENCES documents(id) ON DELETE SET NULL,
                reference_norm TEXT NOT NULL,
                payload BLOB NOT NULL,
                raw_import BLOB,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_saiop_attendances_ref ON saiop_attendances(reference_norm);
            CREATE INDEX IF NOT EXISTS idx_saiop_attendances_batch ON saiop_attendances(batch_id);
            CREATE TABLE IF NOT EXISTS saiop_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS saiop_auto_failures (
                document_id INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
                payload BLOB NOT NULL
            );",
        )?;
        // Libraries from before 0.19.2 kept the normalized reference in clear
        // in that column; it becomes the digest, once. Best effort: a row
        // that cannot be read (another Windows account) is left as it is.
        let plain: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare(
                "SELECT id, payload FROM saiop_attendances
                 WHERE reference_norm <> '' AND (length(reference_norm) <> 64 OR reference_norm GLOB '*[^0-9a-f]*')",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, blob) in plain {
            let Ok(bytes) = unseal(&blob) else { continue };
            let Ok(att) = serde_json::from_slice::<SaiopAttendance>(&bytes) else {
                continue;
            };
            self.conn.execute(
                "UPDATE saiop_attendances SET reference_norm = ?1 WHERE id = ?2",
                params![reference_key(&att.reference_norm), id],
            )?;
        }
        Ok(())
    }

    /// Whether a SIGADOC import records an atendimento by itself (default: yes).
    pub fn auto_attendance_enabled(&self) -> Result<bool> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM saiop_settings WHERE key='auto_attendance'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value.as_deref() != Some("0"))
    }

    pub fn set_auto_attendance(&mut self, enabled: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO saiop_settings(key,value) VALUES('auto_attendance',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [if enabled { "1" } else { "0" }],
        )?;
        Ok(())
    }

    /// Records the passage of a freshly imported SIGADOC document: links an
    /// atendimento that already carries its number (from the spreadsheet,
    /// say) or creates one with what the capture offers. Documents joined
    /// to another are skipped. Idempotent, so a replayed import is harmless.
    pub(crate) fn attendance_for_import(
        &mut self,
        document_id: i64,
        capture: &Capture,
    ) -> Result<()> {
        if !self.auto_attendance_enabled()? || crate::dossier::detect_juntada(capture).is_some() {
            return Ok(());
        }
        let reference_norm = normalize_reference(&capture.document_number);
        ensure!(!reference_norm.is_empty(), "Documento sem número.");
        // Never queued on its own: the import that caused it is what travels,
        // and its replay recreates it (also after a retry here).
        let previous = self.suppress_remote;
        self.suppress_remote = true;
        let result = (|| -> Result<()> {
            let existing = self.list_saiop_attendances()?;
            if existing
                .iter()
                .any(|a| a.document_id == Some(document_id) && a.reference_norm == reference_norm)
            {
                return Ok(());
            }
            let now = Utc::now().to_rfc3339();
            let actor = self.active_actor();
            if let Some(mut unlinked) = existing
                .into_iter()
                .find(|a| a.document_id.is_none() && a.reference_norm == reference_norm)
            {
                let version = self
                    .item_version("saiop", unlinked.id)
                    .ok()
                    .map(|v| v.version);
                unlinked.document_id = Some(document_id);
                unlinked.events.push(SaiopEvent {
                    when: now,
                    actor,
                    kind: "entrada".into(),
                    detail: format!(
                        "Ligado ao documento {} importado do SIGADOC",
                        capture.document_number
                    ),
                });
                unlinked.linked_by_import = true;
                self.save_saiop_attendance_by(&unlinked, version, false)?;
                return Ok(());
            }
            let mut att = SaiopAttendance::new_manual();
            att.reference_raw = capture.document_number.clone();
            att.reference_norm = reference_norm.clone();
            att.sigadoc_code = capture.document_number.clone();
            att.subject = capture.title.trim().chars().take(300).collect();
            att.document_id = Some(document_id);
            att.assignee = self
                .load_intranet_config()
                .map(|cfg| cfg.member_name)
                .unwrap_or_default();
            att.received_from = [
                "sender",
                "remetente",
                "subscritor",
                "origin",
                "origem",
                "movement_source",
            ]
            .iter()
            .find_map(|key| capture.metadata.get(*key))
            .map(|v| v.trim().chars().take(200).collect())
            .unwrap_or_default();
            if let Some(kind) = capture.metadata.get("document_type") {
                att.tags.push(kind.trim().to_owned());
            }
            att.events.push(SaiopEvent {
                when: now,
                actor,
                kind: "entrada".into(),
                detail: format!(
                    "Criado automaticamente na importação de {} do SIGADOC",
                    capture.document_number
                ),
            });
            self.save_saiop_attendance(&att, None)?;
            Ok(())
        })();
        self.suppress_remote = previous;
        result
    }

    /// Records how the automatic atendimento of an import went. The import
    /// may run in the process that serves the browser extension, which has
    /// no window: a failure is kept here and shown in Atendimentos, where it
    /// can be retried. Recording is best effort and never fails the import.
    pub(crate) fn note_auto_attendance(
        &mut self,
        document_id: i64,
        number: &str,
        outcome: Result<()>,
    ) {
        let _ = self.store_auto_attendance_outcome(document_id, number, outcome);
    }

    fn store_auto_attendance_outcome(
        &mut self,
        document_id: i64,
        number: &str,
        outcome: Result<()>,
    ) -> Result<()> {
        match outcome {
            Ok(()) => {
                self.conn.execute(
                    "DELETE FROM saiop_auto_failures WHERE document_id = ?1",
                    params![document_id],
                )?;
            }
            Err(error) => {
                let failure = AutoAttendanceFailure {
                    document_id,
                    number: number.to_owned(),
                    message: format!("{error:#}"),
                    at: Utc::now().to_rfc3339(),
                };
                self.conn.execute(
                    "INSERT INTO saiop_auto_failures(document_id, payload) VALUES(?1, ?2)
                     ON CONFLICT(document_id) DO UPDATE SET payload = excluded.payload",
                    params![document_id, seal(&serde_json::to_vec(&failure)?)?],
                )?;
            }
        }
        Ok(())
    }

    /// Imports whose automatic atendimento could not be recorded, oldest
    /// first (documents in the trash are left out).
    pub fn auto_attendance_failures(&self) -> Result<Vec<AutoAttendanceFailure>> {
        self.cached("saiop_auto_failures", || {
            let mut statement = self.conn.prepare(
                "SELECT f.document_id, f.payload FROM saiop_auto_failures f
                 WHERE NOT EXISTS(SELECT 1 FROM trash x WHERE x.kind = 'document' AND x.item_id = f.document_id)
                 ORDER BY f.rowid",
            )?;
            let rows = statement
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut out = Vec::new();
            for (document_id, blob) in rows {
                // A record this Windows account cannot read says nothing useful.
                if let Ok(mut failure) = unseal(&blob).and_then(|plain| {
                    Ok(serde_json::from_slice::<AutoAttendanceFailure>(&plain)?)
                })
                {
                    failure.document_id = document_id;
                    out.push(failure);
                }
            }
            Ok(out)
        })
    }

    /// Tries the automatic atendimento of an import again (the setting must
    /// be on). Succeeds also when it already exists.
    pub fn retry_auto_attendance(&mut self, document_id: i64) -> Result<()> {
        ensure!(
            self.auto_attendance_enabled()?,
            "Ative \"Criar atendimento ao importar do SIGADOC\" em Ajustes ▸ Importação do SIGADOC para tentar de novo, ou dispense o aviso."
        );
        let capture = self.load_document(document_id)?.capture;
        let outcome = self.attendance_for_import(document_id, &capture);
        let message = outcome.as_ref().err().map(|e| format!("{e:#}"));
        self.note_auto_attendance(document_id, &capture.document_number, outcome);
        match message {
            Some(message) => bail!("{message}"),
            None => Ok(()),
        }
    }

    /// Forgets a failure the person chose not to retry.
    pub fn dismiss_auto_attendance_failure(&mut self, document_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM saiop_auto_failures WHERE document_id = ?1",
            params![document_id],
        )?;
        Ok(())
    }

    /// Every atendimento, newest first, memoized until the library changes.
    pub fn list_saiop_attendances(&self) -> Result<Vec<SaiopAttendance>> {
        self.cached("saiop_attendances", || {
            self.list_saiop_attendances_uncached()
        })
    }
    fn list_saiop_attendances_uncached(&self) -> Result<Vec<SaiopAttendance>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, batch_id, source_row, document_id, payload
             FROM saiop_attendances
             ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, batch_id, source_row, doc_id, blob) = row?;
            let mut item: SaiopAttendance = serde_json::from_slice(&unseal(&blob)?)
                .context("Não foi possível ler um atendimento: o registro está inválido.")?;
            item.id = id;
            item.batch_id = batch_id;
            item.source_row = source_row.map(|r| r as usize);
            item.document_id = doc_id;
            out.push(item);
        }
        Ok(out)
    }

    pub fn list_saiop_batches(&self) -> Result<Vec<SaiopImportBatch>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, payload FROM saiop_batches ORDER BY id DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            let mut batch: SaiopImportBatch = serde_json::from_slice(&unseal(&blob)?).context(
                "Não foi possível ler uma importação de planilha: o registro está inválido.",
            )?;
            batch.id = id;
            out.push(batch);
        }
        Ok(out)
    }

    pub fn load_saiop_raw_row(&self, attendance_id: i64) -> Result<Option<RawSpreadsheetRow>> {
        let raw: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT raw_import FROM saiop_attendances WHERE id = ?1",
                params![attendance_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        match raw {
            Some(blob) => Ok(Some(serde_json::from_slice(&unseal(&blob)?)?)),
            None => Ok(None),
        }
    }

    /// Saves an atendimento typed or changed by a person (it becomes
    /// "edited", so undoing its spreadsheet import keeps it). Goes through
    /// `record_mutation`; on a client computer it is sent to the main one.
    pub fn save_saiop_attendance(
        &mut self,
        attendance: &SaiopAttendance,
        expected_version: Option<i64>,
    ) -> Result<i64> {
        self.save_saiop_attendance_by(attendance, expected_version, true)
    }

    /// `by_person: false` is for changes the program makes by itself (linking
    /// an imported document): they do not mark the row as edited.
    fn save_saiop_attendance_by(
        &mut self,
        attendance: &SaiopAttendance,
        expected_version: Option<i64>,
        by_person: bool,
    ) -> Result<i64> {
        ensure!(
            !attendance.reference_raw.trim().is_empty() || !attendance.subject.trim().is_empty(),
            "Informe a referência (processo, documento ou código provisório) ou o assunto do atendimento."
        );
        ensure!(
            STATUS_OPTIONS.iter().any(|(k, _)| *k == attendance.status),
            "Situação do atendimento inválida."
        );
        ensure!(
            PRIORITY_OPTIONS
                .iter()
                .any(|(k, _)| *k == attendance.priority),
            "Prioridade do atendimento inválida."
        );
        ensure!(
            attendance.proposal_ids.len() <= 8
                && attendance
                    .proposal_ids
                    .iter()
                    .all(|id| crate::model::valid_proposal_id(id)),
            "Identificação de proposta inválida no atendimento."
        );
        if let Some(cfg) = self.client_config()? {
            if !attendance.proposal_ids.is_empty() {
                self.ensure_host_stores_proposals(&cfg)?;
            }
            return match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveSaiopAttendance {
                    attendance: attendance.clone(),
                    expected_version,
                },
            )? {
                crate::intranet::MutationReply::Id(id) => Ok(id),
                _ => bail!(
                    "O computador principal respondeu de forma inesperada ao salvar o atendimento."
                ),
            };
        }

        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let id = Self::write_attendance(&tx, &actor, attendance, expected_version, by_person)?;
        tx.commit()?;
        Ok(id)
    }

    /// Writes an atendimento inside the caller's transaction (events for
    /// status and assignee changes, version check, team log).
    fn write_attendance(
        tx: &rusqlite::Transaction,
        actor: &str,
        attendance: &SaiopAttendance,
        expected_version: Option<i64>,
        by_person: bool,
    ) -> Result<i64> {
        let actor = actor.to_owned();
        let now = Utc::now().to_rfc3339();
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut next = attendance.clone();
        next.reference_raw = next.reference_raw.trim().to_owned();
        next.reference_norm = normalize_reference(&next.reference_raw);
        next.subject = next.subject.trim().to_owned();
        next.assignee = next.assignee.trim().to_owned();
        next.updated_at = now.clone();
        if by_person {
            next.human_edited = true;
        }

        if next.id > 0 {
            let existing_blob: Vec<u8> = tx
                .query_row(
                    "SELECT payload FROM saiop_attendances WHERE id = ?1",
                    params![next.id],
                    |row| row.get(0),
                )
                .optional()?
                .context("Atendimento não encontrado no acervo.")?;
            let prev: SaiopAttendance = serde_json::from_slice(&unseal(&existing_blob)?)?;
            if prev.status != next.status {
                next.stage_changed_at = today.clone();
                if next.status == "encerrado" && next.closed_at.trim().is_empty() {
                    next.closed_at = today.clone();
                } else if next.is_open() {
                    next.closed_at.clear();
                }
                next.events.push(SaiopEvent {
                    when: now.clone(),
                    actor: actor.clone(),
                    kind: "situacao".into(),
                    detail: format!(
                        "Situação alterada de '{}' para '{}'",
                        status_label(&prev.status),
                        status_label(&next.status)
                    ),
                });
            }
            if prev.assignee != next.assignee {
                next.events.push(SaiopEvent {
                    when: now.clone(),
                    actor: actor.clone(),
                    kind: "distribuicao".into(),
                    detail: format!(
                        "Responsável alterado de '{}' para '{}'",
                        if prev.assignee.is_empty() {
                            "sem responsável"
                        } else {
                            &prev.assignee
                        },
                        if next.assignee.is_empty() {
                            "sem responsável"
                        } else {
                            &next.assignee
                        }
                    ),
                });
            }
            let encoded = seal(&serde_json::to_vec(&next)?)?;
            tx.execute(
                "UPDATE saiop_attendances
                 SET document_id = ?1, reference_norm = ?2, payload = ?3, updated_at = ?4
                 WHERE id = ?5",
                params![
                    next.document_id,
                    reference_key(&next.reference_norm),
                    encoded,
                    now,
                    next.id
                ],
            )?;
            crate::intranet::record_mutation(
                tx,
                &actor,
                "saiop",
                next.id,
                expected_version,
                "save_saiop_attendance",
                next.document_id,
                &format!(
                    "Atendimento {} ({}) atualizado",
                    if next.reference_raw.is_empty() {
                        "sem número"
                    } else {
                        &next.reference_raw
                    },
                    status_label(&next.status)
                ),
            )?;
            Ok(next.id)
        } else {
            next.created_at = now.clone();
            if next.stage_changed_at.trim().is_empty() {
                next.stage_changed_at = today;
            }
            next.events.push(SaiopEvent {
                when: now.clone(),
                actor: actor.clone(),
                kind: "entrada".into(),
                detail: "Atendimento registrado no controle do gabinete".into(),
            });
            let placeholder = seal(b"{}")?;
            tx.execute(
                "INSERT INTO saiop_attendances (batch_id, source_row, document_id, reference_norm, payload, raw_import, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                params![
                    next.batch_id,
                    next.source_row.map(|r| r as i64),
                    next.document_id,
                    reference_key(&next.reference_norm),
                    placeholder,
                    now
                ],
            )?;
            let id = tx.last_insert_rowid();
            next.id = id;
            let encoded = seal(&serde_json::to_vec(&next)?)?;
            tx.execute(
                "UPDATE saiop_attendances SET payload = ?1 WHERE id = ?2",
                params![encoded, id],
            )?;
            crate::intranet::record_mutation(
                tx,
                &actor,
                "saiop",
                id,
                None,
                "create_saiop_attendance",
                next.document_id,
                &format!(
                    "Novo atendimento registrado: {}",
                    if next.reference_raw.is_empty() {
                        &next.subject
                    } else {
                        &next.reference_raw
                    }
                ),
            )?;
            Ok(id)
        }
    }

    /// Prepara a prévia de importação da aba `PROCESSOS` de um arquivo `.xlsx` sem gravar no banco.
    pub fn preview_saiop_xlsx(
        &self,
        path: &Path,
        tramitado_keep_open: bool,
    ) -> Result<SaiopImportPreview> {
        let size = std::fs::metadata(path)
            .with_context(|| format!("Não foi possível ler a planilha {}.", path.display()))?
            .len();
        ensure!(
            size <= MAX_XLSX_FILE_BYTES,
            "A planilha tem {} MB; o limite é {} MB. Salve uma cópia só com a aba PROCESSOS.",
            size / (1024 * 1024),
            MAX_XLSX_FILE_BYTES / (1024 * 1024)
        );
        let bytes = std::fs::read(path)
            .with_context(|| format!("Não foi possível ler a planilha {}.", path.display()))?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("planilha.xlsx")
            .to_owned();
        self.preview_saiop_xlsx_bytes(&file_name, &bytes, tramitado_keep_open)
    }

    pub fn preview_saiop_xlsx_bytes(
        &self,
        file_name: &str,
        bytes: &[u8],
        tramitado_keep_open: bool,
    ) -> Result<SaiopImportPreview> {
        let file_sha256 = format!("{:x}", Sha256::digest(bytes));
        let sheet = parse_xlsx_processos_sheet(bytes)?;

        // Map existing documents by exact normalized number for safe association suggestion
        let docs = self.list_documents("")?;
        let mut doc_by_number: BTreeMap<String, i64> = BTreeMap::new();
        for doc in docs {
            doc_by_number.insert(normalize_reference(&doc.document_number), doc.id);
        }

        // First pass: count reference occurrences and distinct entry dates to detect re-entries
        let mut ref_dates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for row in &sheet.rows {
            let ref_norm =
                normalize_reference(row.cells.get("D").map(String::as_str).unwrap_or(""));
            if !ref_norm.is_empty() {
                let date_b = row
                    .cells
                    .get("B")
                    .map(|s| s.trim().to_owned())
                    .unwrap_or_default();
                ref_dates.entry(ref_norm).or_default().insert(date_b);
            }
        }

        // Attendances already in the control, by (reference, entry date): an
        // updated copy of the same spreadsheet must not duplicate them.
        let mut existing: BTreeMap<(String, String), i64> = BTreeMap::new();
        for att in self.list_saiop_attendances()? {
            if !att.reference_norm.is_empty() {
                existing
                    .entry((att.reference_norm.clone(), att.entry_date.trim().to_owned()))
                    .or_insert(att.id);
            }
        }
        let mut already_registered = Vec::new();

        let now = Utc::now().to_rfc3339();
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut items = Vec::new();
        let mut ignored_formula_rows = 0usize;
        let mut repeated_references = 0usize;
        let mut flagged_count = 0usize;
        let mut linked_documents = 0usize;

        for raw in sheet.rows {
            let col_b = raw.cells.get("B").map(|s| s.trim()).unwrap_or("");
            let col_c = raw.cells.get("C").map(|s| s.trim()).unwrap_or("");
            let col_d = raw.cells.get("D").map(|s| s.trim()).unwrap_or("");
            let col_f = raw.cells.get("F").map(|s| s.trim()).unwrap_or("");
            let col_g = raw.cells.get("G").map(|s| s.trim()).unwrap_or("");
            let col_i = raw.cells.get("I").map(|s| s.trim()).unwrap_or("");
            let col_j = raw.cells.get("J").map(|s| s.trim()).unwrap_or("");
            let col_k = raw.cells.get("K").map(|s| s.trim()).unwrap_or("");
            let col_l = raw.cells.get("L").map(|s| s.trim()).unwrap_or("");
            let col_m = raw.cells.get("M").map(|s| s.trim()).unwrap_or("");
            let col_n = raw.cells.get("N").map(|s| s.trim()).unwrap_or("");
            let col_o = raw.cells.get("O").map(|s| s.trim()).unwrap_or("");
            let col_p = raw.cells.get("P").map(|s| s.trim()).unwrap_or("");
            let col_q = raw.cells.get("Q").map(|s| s.trim()).unwrap_or("");
            let col_r = raw.cells.get("R").map(|s| s.trim()).unwrap_or("");
            let col_s = raw.cells.get("S").map(|s| s.trim()).unwrap_or("");

            // Rows containing only prepared formulas (e.g. E/F/G/H) without operational data are ignored
            let has_operational_data = ![
                col_b, col_c, col_d, col_i, col_j, col_k, col_l, col_m, col_n, col_o, col_p, col_q,
                col_r,
            ]
            .iter()
            .all(|v| v.is_empty());
            if !has_operational_data {
                ignored_formula_rows += 1;
                continue;
            }

            let mut flags = Vec::new();
            let ref_norm = normalize_reference(col_d);
            if ref_norm.is_empty() {
                flags.push("Identificador (Coluna D) ausente".into());
            } else if let Some(dates) = ref_dates.get(&ref_norm) {
                if dates.len() > 1 {
                    repeated_references += 1;
                    flags.push(format!(
                        "Referência com múltiplas passagens pelo gabinete ({} datas de entrada distintas)",
                        dates.len()
                    ));
                }
            }

            let (entry_iso, entry_flag) = parse_spreadsheet_date(col_b);
            if let Some(f) = entry_flag {
                flags.push(format!("Entrada (Col B): {f}"));
            }
            let entry_date = entry_iso.unwrap_or_else(|| col_b.to_owned());

            // Col J: Retorno para UAS? (Sim, Não, vazio -> nao_informado; datas nessa coluna exigem revisão)
            let uas_norm = col_j.to_uppercase();
            let uas_return = if uas_norm.is_empty() {
                "nao_informado".to_owned()
            } else if uas_norm == "SIM" || uas_norm == "S" {
                "sim".to_owned()
            } else if uas_norm == "NÃO" || uas_norm == "NAO" || uas_norm == "N" {
                "nao".to_owned()
            } else {
                flags.push(format!(
                    "Valor atípico/data na coluna 'Retorno para UAS?' (Col J): '{col_j}'"
                ));
                "nao_informado".to_owned()
            };

            // Col K: Prazo de retorno
            let (due_iso, due_flag) = parse_spreadsheet_date(col_k);
            let (due_date, due_kind) = match due_iso {
                Some(iso) => {
                    if let Some(f) = due_flag {
                        flags.push(format!("Prazo (Col K): {f}"));
                    }
                    (iso, "oficial".to_owned())
                }
                None => {
                    if !col_k.is_empty() {
                        flags.push(format!(
                            "Prazo textual ou ambíguo pendente de confirmação (Col K): '{col_k}'"
                        ));
                    }
                    (String::new(), "a_confirmar".to_owned())
                }
            };

            // Col N: Assinado? (Vazio NÃO significa "Não")
            let signed_norm = col_n.to_uppercase();
            let signed_status = if signed_norm.is_empty() {
                "nao_informado".to_owned()
            } else if signed_norm == "SIM" || signed_norm == "S" {
                "sim".to_owned()
            } else if signed_norm == "NÃO" || signed_norm == "NAO" || signed_norm == "N" {
                "nao".to_owned()
            } else {
                flags.push(format!("Valor ambíguo em 'Assinado?' (Col N): '{col_n}'"));
                "nao_informado".to_owned()
            };

            // Col O: Data de trâmite (saída)
            let (exit_iso, exit_flag) = parse_spreadsheet_date(col_o);
            if let Some(f) = exit_flag {
                flags.push(format!("Data de trâmite (Col O): {f}"));
            }
            let exit_date = exit_iso.unwrap_or_else(|| col_o.to_owned());

            if let (Ok(d_in), Ok(d_out)) = (
                NaiveDate::parse_from_str(&entry_date, "%Y-%m-%d"),
                NaiveDate::parse_from_str(&exit_date, "%Y-%m-%d"),
            ) {
                if d_out < d_in {
                    flags.push(format!(
                        "Data de saída ({exit_date}) anterior à data de entrada ({entry_date})"
                    ));
                }
            }

            if !col_s.is_empty() {
                flags.push(format!(
                    "Coluna S preenchida ('{col_s}') — significado pendente de confirmação"
                ));
            }

            let (status, priority, outcome, scheduled, sensitive, attachment_relation) =
                map_verification_dimensions(col_m, tramitado_keep_open, &uas_return);

            let document_id = if !ref_norm.is_empty() {
                doc_by_number.get(&ref_norm).copied()
            } else {
                None
            };
            if document_id.is_some() {
                linked_documents += 1;
            }
            if !flags.is_empty() {
                flagged_count += 1;
            }

            let sigadoc_code = if !col_g.is_empty() {
                col_g.to_owned()
            } else if !col_f.is_empty() {
                col_f.to_owned()
            } else {
                String::new()
            };

            let closed_at = if status == "encerrado" {
                if NaiveDate::parse_from_str(&exit_date, "%Y-%m-%d").is_ok() {
                    exit_date.clone()
                } else {
                    today.clone()
                }
            } else {
                String::new()
            };

            let stage_changed_at = if NaiveDate::parse_from_str(&exit_date, "%Y-%m-%d").is_ok() {
                exit_date.clone()
            } else if NaiveDate::parse_from_str(&entry_date, "%Y-%m-%d").is_ok() {
                entry_date.clone()
            } else {
                today.clone()
            };

            let attendance = SaiopAttendance {
                id: 0,
                batch_id: None,
                source_row: Some(raw.row_number),
                reference_raw: col_d.to_owned(),
                reference_norm: ref_norm,
                sigadoc_code,
                subject: col_r.to_owned(),
                tags: Vec::new(),
                entry_date,
                entry_date_raw: col_b.to_owned(),
                assignee: col_c.to_owned(),
                received_from: col_i.to_owned(),
                uas_return,
                uas_raw: col_j.to_owned(),
                due_date,
                due_date_raw: col_k.to_owned(),
                due_kind,
                observation: col_l.to_owned(),
                verification_raw: col_m.to_owned(),
                status,
                priority,
                priority_reason: if col_m.is_empty() {
                    String::new()
                } else {
                    format!("Importado de Verificação: {col_m}")
                },
                signed_status,
                exit_date,
                exit_date_raw: col_o.to_owned(),
                planned_doc_type: col_p.to_owned(),
                sent_to: col_q.to_owned(),
                outcome,
                scheduled,
                sensitive,
                attachment_relation,
                review_flags: flags,
                document_id,
                task_ids: Vec::new(),
                stage_changed_at,
                closed_at,
                created_at: now.clone(),
                updated_at: now.clone(),
                human_edited: false,
                linked_by_import: false,
                events: vec![SaiopEvent {
                    when: now.clone(),
                    actor: self.active_actor(),
                    kind: "importacao_planilha".into(),
                    detail: format!(
                        "Importado da aba {} (linha {}) do arquivo {}",
                        sheet.sheet_name, raw.row_number, file_name
                    ),
                }],
                proposal_ids: Vec::new(),
            };
            already_registered.push(if attendance.reference_norm.is_empty() {
                None
            } else {
                existing
                    .get(&(
                        attendance.reference_norm.clone(),
                        attendance.entry_date.trim().to_owned(),
                    ))
                    .copied()
            });
            items.push((attendance, raw));
        }

        Ok(SaiopImportPreview {
            file_name: file_name.to_owned(),
            file_sha256,
            sheet_name: sheet.sheet_name,
            headers: sheet.headers,
            items,
            ignored_formula_rows,
            repeated_references,
            flagged_count,
            linked_documents,
            tramitado_keep_open,
            already_registered,
        })
    }

    pub fn commit_saiop_import(
        &mut self,
        preview: &SaiopImportPreview,
    ) -> Result<SaiopImportBatch> {
        if let Some(cfg) = self.client_config()? {
            return match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::ImportSaiopBatch {
                    file_name: preview.file_name.clone(),
                    file_sha256: preview.file_sha256.clone(),
                    sheet_name: preview.sheet_name.clone(),
                    items: preview.items.clone(),
                },
            )? {
                crate::intranet::MutationReply::SaiopBatch(batch) => Ok(batch),
                _ => bail!(
                    "O computador principal respondeu de forma inesperada ao importar a planilha."
                ),
            };
        }
        self.commit_saiop_import_raw(
            &preview.file_name,
            &preview.file_sha256,
            &preview.sheet_name,
            &preview.items,
        )
    }

    /// Writes an import batch here, without team routing: called by
    /// `commit_saiop_import` and by the main computer for its clients.
    /// Refuses a file already imported (same SHA-256).
    pub(crate) fn commit_saiop_import_raw(
        &mut self,
        file_name: &str,
        file_sha256: &str,
        sheet_name: &str,
        items: &[(SaiopAttendance, RawSpreadsheetRow)],
    ) -> Result<SaiopImportBatch> {
        ensure!(
            !items.is_empty(),
            "Não há linhas de atendimento válidas para importar."
        );
        let actor = self.active_actor();
        let now = Utc::now().to_rfc3339();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let duplicate: Option<i64> = tx
            .query_row(
                "SELECT id FROM saiop_batches WHERE file_sha256 = ?1",
                params![file_sha256],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing_id) = duplicate {
            bail!(
                "Esta planilha já foi importada (importação nº {existing_id}). Para não duplicar atendimentos, a importação foi bloqueada."
            );
        }

        let flagged_rows = items
            .iter()
            .filter(|(att, _)| !att.review_flags.is_empty())
            .count();

        let mut batch = SaiopImportBatch {
            id: 0,
            file_name: file_name.to_owned(),
            file_sha256: file_sha256.to_owned(),
            sheet_name: sheet_name.to_owned(),
            imported_at: now.clone(),
            actor: actor.clone(),
            total_rows: items.len(),
            flagged_rows,
        };

        let placeholder = seal(b"{}")?;
        tx.execute(
            "INSERT INTO saiop_batches (file_sha256, payload, imported_at) VALUES (?1, ?2, ?3)",
            params![file_sha256, placeholder, now],
        )?;
        let batch_id = tx.last_insert_rowid();
        batch.id = batch_id;
        let batch_blob = seal(&serde_json::to_vec(&batch)?)?;
        tx.execute(
            "UPDATE saiop_batches SET payload = ?1 WHERE id = ?2",
            params![batch_blob, batch_id],
        )?;

        for (att, raw) in items {
            let mut record = att.clone();
            record.batch_id = Some(batch_id);
            let raw_blob = seal(&serde_json::to_vec(raw)?)?;
            tx.execute(
                "INSERT INTO saiop_attendances (batch_id, source_row, document_id, reference_norm, payload, raw_import, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    batch_id,
                    record.source_row.map(|r| r as i64),
                    record.document_id,
                    reference_key(&record.reference_norm),
                    placeholder,
                    raw_blob,
                    now
                ],
            )?;
            let id = tx.last_insert_rowid();
            record.id = id;
            let att_blob = seal(&serde_json::to_vec(&record)?)?;
            tx.execute(
                "UPDATE saiop_attendances SET payload = ?1 WHERE id = ?2",
                params![att_blob, id],
            )?;
            crate::intranet::record_mutation(
                &tx,
                &actor,
                "saiop",
                id,
                None,
                "import_saiop_row",
                record.document_id,
                &format!(
                    "Linha {} importada no lote #{}",
                    record.source_row.unwrap_or(0),
                    batch_id
                ),
            )?;
        }

        crate::intranet::record_mutation(
            &tx,
            &actor,
            "saiop_batch",
            batch_id,
            None,
            "import_saiop_batch",
            None,
            &format!(
                "Lote #{} importado ({} atendimentos da aba {})",
                batch_id,
                items.len(),
                sheet_name
            ),
        )?;
        tx.commit()?;
        Ok(batch)
    }

    /// Undoes an imported batch for good: deletes the rows nobody touched and
    /// keeps (detached from the batch) those edited, turned into tasks or
    /// holding attachments, as decided by `kept_on_undo`. Returns
    /// (removed, kept). On a client computer it is sent to the main one.
    pub fn undo_saiop_batch(&mut self, batch_id: i64) -> Result<(usize, usize)> {
        if let Some(cfg) = self.client_config()? {
            return match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::UndoSaiopBatch { batch_id },
            )? {
                crate::intranet::MutationReply::Counts(removed, kept) => Ok((removed, kept)),
                _ => bail!("O computador principal respondeu de forma inesperada ao desfazer a importação."),
            };
        }

        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let mut rows = Vec::new();
        {
            let mut stmt =
                tx.prepare("SELECT id, payload FROM saiop_attendances WHERE batch_id = ?1")?;
            let mapped = stmt.query_map(params![batch_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
            for r in mapped {
                rows.push(r?);
            }
        }

        let mut removed = 0usize;
        let mut preserved = 0usize;
        for (id, blob) in rows {
            let att: SaiopAttendance = serde_json::from_slice(&unseal(&blob)?)?;
            if kept_on_undo(&tx, id, &att)? {
                preserved += 1;
                tx.execute(
                    "UPDATE saiop_attendances SET batch_id = NULL WHERE id = ?1",
                    params![id],
                )?;
            } else {
                removed += 1;
                tx.execute("DELETE FROM saiop_attendances WHERE id = ?1", params![id])?;
                tx.execute(
                    "DELETE FROM item_versions WHERE kind = 'saiop' AND item_id = ?1",
                    params![id],
                )?;
            }
        }
        tx.execute("DELETE FROM saiop_batches WHERE id = ?1", params![batch_id])?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "saiop_batch",
            batch_id,
            None,
            "undo_saiop_batch",
            None,
            &format!(
                "Lote #{batch_id} revertido ({removed} removidos, {preserved} preservados por terem edição local)"
            ),
        )?;
        tx.commit()?;
        Ok((removed, preserved))
    }

    /// Creates a task for an atendimento, linked to its document, and records
    /// it in the atendimento's timeline, all in one write (also when the
    /// change goes to the team). Without a linked document there is no task:
    /// the step is only recorded in the timeline and `None` comes back.
    pub fn create_task_for_saiop(
        &mut self,
        attendance_id: i64,
        title: &str,
        priority: &str,
        due_date: &str,
        assignee: &str,
    ) -> Result<Option<i64>> {
        let att = self
            .list_saiop_attendances()?
            .into_iter()
            .find(|a| a.id == attendance_id)
            .context("Atendimento não encontrado.")?;
        let version = self
            .item_version("saiop", attendance_id)
            .ok()
            .map(|v| v.version);
        let task = att.document_id.map(|doc_id| Task {
            id: 0,
            document_id: Some(doc_id),
            title: title.trim().to_owned(),
            action: "examinar".into(),
            status: "pendente".into(),
            priority: if matches!(priority, "baixa" | "normal" | "alta" | "urgente") {
                priority.to_owned()
            } else {
                "normal".into()
            },
            due_date: due_date.trim().to_owned(),
            notes: format!(
                "Referência do gabinete: {}",
                if att.reference_raw.is_empty() {
                    &att.subject
                } else {
                    &att.reference_raw
                }
            ),
            completed_at: None,
            due_source: "definido pelo usuario".into(),
            dependencies: Vec::new(),
            reminder_date: String::new(),
            checklist: Vec::new(),
            outcome: String::new(),
            assignee: assignee.trim().to_owned(),
            proposal_id: String::new(),
        });
        if let Some(task) = &task {
            task.validate()?;
        }
        if let Some(cfg) = self.client_config()? {
            return match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::CreateTaskForSaiop {
                    attendance_id,
                    task,
                    title: title.trim().to_owned(),
                    expected_version: version,
                },
            )? {
                crate::intranet::MutationReply::Id(id) => Ok((id > 0).then_some(id)),
                _ => bail!("O computador principal respondeu de forma inesperada ao criar a tarefa do atendimento."),
            };
        }
        self.write_task_for_saiop(attendance_id, task.as_ref(), title, version)
    }

    /// The single transaction behind [`Store::create_task_for_saiop`], also
    /// run by the main computer and when a queued change is replayed.
    pub(crate) fn write_task_for_saiop(
        &mut self,
        attendance_id: i64,
        task: Option<&Task>,
        title: &str,
        expected_version: Option<i64>,
    ) -> Result<Option<i64>> {
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut att = load_attendance_tx(&tx, attendance_id)?;
        let task_id = match task {
            Some(task) => {
                ensure!(
                    task.id == 0
                        && task.document_id.is_some()
                        && att.document_id == task.document_id,
                    "O atendimento mudou de documento; abra-o de novo e repita."
                );
                let saved = crate::tasks::write_task(&tx, &actor, task, None)?;
                if !att.task_ids.contains(&saved.id) {
                    att.task_ids.push(saved.id);
                }
                saved.id
            }
            None => 0,
        };
        att.events.push(SaiopEvent {
            when: Utc::now().to_rfc3339(),
            actor: actor.clone(),
            kind: "tarefa".into(),
            detail: if task_id > 0 {
                format!("Tarefa #{task_id} criada: {}", title.trim())
            } else {
                format!("Providência registrada no atendimento: {}", title.trim())
            },
        });
        Self::write_attendance(&tx, &actor, &att, expected_version, true)?;
        tx.commit()?;
        Ok((task_id > 0).then_some(task_id))
    }

    /// Removes one atendimento for good, with its attachments (there is no
    /// trash for atendimentos). Tasks it created stay with their documents.
    /// On a client computer the request goes to the main computer.
    pub fn delete_saiop_attendance(&mut self, id: i64) -> Result<()> {
        if let Some(cfg) = self.client_config()? {
            return match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::DeleteSaiopAttendance { id },
            )? {
                crate::intranet::MutationReply::Unit => Ok(()),
                _ => bail!("O computador principal respondeu de forma inesperada ao excluir o atendimento."),
            };
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (document_id, blob): (Option<i64>, Vec<u8>) = tx
            .query_row(
                "SELECT document_id, payload FROM saiop_attendances WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .context("Atendimento não encontrado (já excluído?).")?;
        let att: SaiopAttendance = serde_json::from_slice(&unseal(&blob)?)?;
        tx.execute(
            "DELETE FROM trash WHERE kind='attachment' AND item_id IN (SELECT id FROM attachments WHERE owner_kind='saiop' AND owner_id=?1)",
            params![id],
        )?;
        let removed_files = tx.execute(
            "DELETE FROM attachments WHERE owner_kind='saiop' AND owner_id=?1",
            params![id],
        )?;
        tx.execute("DELETE FROM saiop_attendances WHERE id = ?1", params![id])?;
        tx.execute(
            "DELETE FROM item_versions WHERE kind = 'saiop' AND item_id = ?1",
            params![id],
        )?;
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "saiop",
            id,
            None,
            "delete_saiop_attendance",
            document_id,
            &format!(
                "Atendimento {} excluído{}",
                if att.reference_raw.is_empty() {
                    "sem número"
                } else {
                    &att.reference_raw
                },
                if removed_files > 0 {
                    format!(
                        " (com {})",
                        crate::labels::count(removed_files, "anexo", "anexos")
                    )
                } else {
                    String::new()
                }
            ),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// What `undo_saiop_batch` would do, as (removed, kept), without doing it.
    pub fn undo_saiop_batch_plan(&self, batch_id: i64) -> Result<(usize, usize)> {
        let mut removed = 0;
        let mut preserved = 0;
        for att in self.list_saiop_attendances()? {
            if att.batch_id == Some(batch_id) {
                if kept_on_undo(&self.conn, att.id, &att)? {
                    preserved += 1;
                } else {
                    removed += 1;
                }
            }
        }
        Ok((removed, preserved))
    }
}

/// One atendimento as stored, read inside a transaction.
fn load_attendance_tx(conn: &rusqlite::Connection, id: i64) -> Result<SaiopAttendance> {
    let (batch_id, source_row, document_id, blob) = conn
        .query_row(
            "SELECT batch_id, source_row, document_id, payload FROM saiop_attendances WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            },
        )
        .optional()?
        .context("Atendimento não encontrado no acervo.")?;
    let mut att: SaiopAttendance = serde_json::from_slice(&unseal(&blob)?)
        .context("Não foi possível ler um atendimento: o registro está inválido.")?;
    att.id = id;
    att.batch_id = batch_id;
    att.source_row = source_row.map(|r| r as usize);
    att.document_id = document_id;
    Ok(att)
}

/// Whether undoing a spreadsheet import keeps this atendimento instead of
/// deleting it: someone edited it, made a task from it or attached files to
/// it, or a SIGADOC import linked it to its document (nothing would create
/// that document's atendimento again). The preview and the undo itself both
/// ask here, so they never disagree.
fn kept_on_undo(conn: &rusqlite::Connection, id: i64, att: &SaiopAttendance) -> Result<bool> {
    if att.human_edited || att.linked_by_import || !att.task_ids.is_empty() {
        return Ok(true);
    }
    let attachments: i64 = conn.query_row(
        "SELECT count(*) FROM attachments a WHERE a.owner_kind='saiop' AND a.owner_id=?1
           AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='attachment' AND t.item_id=a.id)",
        params![id],
        |r| r.get(0),
    )?;
    Ok(attachments > 0)
}

// ============================================================================
// Leitor puro em Rust de arquivos `.xlsx` (ZIP + RFC 1951 Deflate + XML)
// ============================================================================

struct ParsedSheet {
    sheet_name: String,
    headers: Vec<String>,
    rows: Vec<RawSpreadsheetRow>,
}

fn parse_xlsx_processos_sheet(bytes: &[u8]) -> Result<ParsedSheet> {
    let entries = read_zip_entries(bytes)?;
    let shared_strings = if let Some(ss_xml) = entries.get("xl/sharedStrings.xml") {
        parse_shared_strings(
            std::str::from_utf8(ss_xml)
                .context("A planilha está danificada ou não é um arquivo .xlsx válido.")?,
        )
    } else {
        Vec::new()
    };

    let (sheet_name, sheet_path) = resolve_processos_sheet_path(&entries)?;
    let sheet_bytes = entries
        .get(&sheet_path)
        .or_else(|| entries.get(sheet_path.trim_start_matches('/')))
        .with_context(|| format!("A aba '{sheet_name}' não foi encontrada na planilha."))?;
    let sheet_xml = std::str::from_utf8(sheet_bytes)
        .context("A planilha está danificada ou não é um arquivo .xlsx válido.")?;

    let (headers, rows) = parse_worksheet_xml(&sheet_name, sheet_xml, &shared_strings);
    Ok(ParsedSheet {
        sheet_name,
        headers,
        rows,
    })
}

fn resolve_processos_sheet_path(entries: &BTreeMap<String, Vec<u8>>) -> Result<(String, String)> {
    let mut target_by_rid = BTreeMap::new();
    if let Some(rels_bytes) = entries.get("xl/_rels/workbook.xml.rels") {
        if let Ok(rels_xml) = std::str::from_utf8(rels_bytes) {
            for tag in extract_xml_tags(rels_xml, "Relationship") {
                if let (Some(id), Some(target)) = (xml_attr(&tag, "Id"), xml_attr(&tag, "Target")) {
                    let full = if target.starts_with("xl/") || target.starts_with("/xl/") {
                        target.trim_start_matches('/').to_owned()
                    } else {
                        format!("xl/{}", target.trim_start_matches('/'))
                    };
                    target_by_rid.insert(id, full);
                }
            }
        }
    }

    if let Some(wb_bytes) = entries.get("xl/workbook.xml") {
        if let Ok(wb_xml) = std::str::from_utf8(wb_bytes) {
            let mut sheets = Vec::new();
            for tag in extract_xml_tags(wb_xml, "sheet") {
                let name = xml_attr(&tag, "name").unwrap_or_default();
                let rid = xml_attr(&tag, "r:id")
                    .or_else(|| xml_attr(&tag, "id"))
                    .unwrap_or_default();
                if !name.is_empty() {
                    let path = target_by_rid
                        .get(&rid)
                        .cloned()
                        .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", sheets.len() + 1));
                    sheets.push((name, path));
                }
            }
            if let Some(found) = sheets
                .iter()
                .find(|(name, _)| name.trim().eq_ignore_ascii_case("PROCESSOS"))
            {
                return Ok(found.clone());
            }
            if let Some(first) = sheets.into_iter().next() {
                return Ok(first);
            }
        }
    }

    if entries.contains_key("xl/worksheets/sheet1.xml") {
        return Ok(("PROCESSOS".into(), "xl/worksheets/sheet1.xml".into()));
    }
    bail!("Não foi possível encontrar nenhuma aba na planilha.")
}

fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut pos = 0;
    while let Some(start) = xml[pos..].find("<si") {
        let abs_start = pos + start;
        let Some(open_end) = xml[abs_start..].find('>') else {
            break;
        };
        let content_start = abs_start + open_end + 1;
        let Some(end) = xml[content_start..].find("</si>") else {
            break;
        };
        let si_body = &xml[content_start..content_start + end];
        strings.push(extract_all_t_elements(si_body));
        pos = content_start + end + 5;
    }
    strings
}

fn extract_all_t_elements(xml_fragment: &str) -> String {
    let mut out = String::new();
    let mut pos = 0;
    while let Some(start) = xml_fragment[pos..].find("<t") {
        let abs = pos + start;
        let after_t = xml_fragment
            .as_bytes()
            .get(abs + 2)
            .copied()
            .unwrap_or(b' ');
        if after_t != b'>' && after_t != b' ' && after_t != b'/' {
            pos = abs + 2;
            continue;
        }
        let Some(close_bracket) = xml_fragment[abs..].find('>') else {
            break;
        };
        if xml_fragment[abs..abs + close_bracket].ends_with('/') {
            pos = abs + close_bracket + 1;
            continue;
        }
        let text_start = abs + close_bracket + 1;
        let Some(end_t) = xml_fragment[text_start..].find("</t>") else {
            break;
        };
        out.push_str(&unescape_xml(&xml_fragment[text_start..text_start + end_t]));
        pos = text_start + end_t + 4;
    }
    out
}

fn parse_worksheet_xml(
    sheet_name: &str,
    xml: &str,
    shared_strings: &[String],
) -> (Vec<String>, Vec<RawSpreadsheetRow>) {
    let mut headers = Vec::new();
    let mut rows = Vec::new();
    let mut pos = 0;
    let mut auto_row = 0usize;

    while let Some(r_start) = xml[pos..].find("<row") {
        let abs_r = pos + r_start;
        let after_row = xml.as_bytes().get(abs_r + 4).copied().unwrap_or(b' ');
        if after_row != b'>' && after_row != b' ' && after_row != b'/' {
            pos = abs_r + 4;
            continue;
        }
        let Some(r_open_end) = xml[abs_r..].find('>') else {
            break;
        };
        let row_tag = &xml[abs_r..abs_r + r_open_end + 1];
        auto_row += 1;
        let row_number = xml_attr(row_tag, "r")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(auto_row);

        if row_tag.ends_with("/>") {
            pos = abs_r + r_open_end + 1;
            continue;
        }
        let body_start = abs_r + r_open_end + 1;
        let Some(r_end) = xml[body_start..].find("</row>") else {
            break;
        };
        let row_body = &xml[body_start..body_start + r_end];
        pos = body_start + r_end + 6;

        let (cells, formulas) = parse_row_cells(row_body, shared_strings);
        if row_number == 1 {
            for col in 'A'..='S' {
                let key = col.to_string();
                headers.push(cells.get(&key).cloned().unwrap_or_default());
            }
        } else if !cells.is_empty() || !formulas.is_empty() {
            rows.push(RawSpreadsheetRow {
                sheet_name: sheet_name.to_owned(),
                row_number,
                cells,
                formulas,
            });
        }
    }
    (headers, rows)
}

fn parse_row_cells(
    row_xml: &str,
    shared_strings: &[String],
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut cells = BTreeMap::new();
    let mut formulas = BTreeMap::new();
    let mut pos = 0;
    let mut col_idx = 0usize;

    while let Some(c_start) = row_xml[pos..].find("<c") {
        let abs_c = pos + c_start;
        let next_ch = row_xml.as_bytes().get(abs_c + 2).copied().unwrap_or(b' ');
        if next_ch != b'>' && next_ch != b' ' && next_ch != b'/' {
            pos = abs_c + 2;
            continue;
        }
        let Some(c_open_end) = row_xml[abs_c..].find('>') else {
            break;
        };
        let c_tag = &row_xml[abs_c..abs_c + c_open_end + 1];
        let col_ref = xml_attr(c_tag, "r")
            .map(|r| {
                r.chars()
                    .take_while(|ch| ch.is_ascii_alphabetic())
                    .collect::<String>()
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| column_index_to_letters(col_idx));
        col_idx += 1;

        let cell_type = xml_attr(c_tag, "t").unwrap_or_default();
        if c_tag.ends_with("/>") {
            pos = abs_c + c_open_end + 1;
            continue;
        }
        let c_body_start = abs_c + c_open_end + 1;
        let Some(c_end) = row_xml[c_body_start..].find("</c>") else {
            break;
        };
        let c_body = &row_xml[c_body_start..c_body_start + c_end];
        pos = c_body_start + c_end + 4;

        if let Some(formula) = extract_tag_text(c_body, "f") {
            if !formula.trim().is_empty() {
                formulas.insert(col_ref.clone(), formula);
            }
        }

        let value = if cell_type == "inlineStr" {
            extract_all_t_elements(c_body)
        } else if let Some(v_raw) = extract_tag_text(c_body, "v") {
            if cell_type == "s" {
                v_raw
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|idx| shared_strings.get(idx).cloned())
                    .unwrap_or(v_raw)
            } else if cell_type == "b" {
                if v_raw.trim() == "1" {
                    "Sim".into()
                } else {
                    "Não".into()
                }
            } else {
                v_raw
            }
        } else {
            String::new()
        };

        if !value.trim().is_empty() {
            cells.insert(col_ref, value);
        }
    }
    (cells, formulas)
}

fn column_index_to_letters(mut index: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push((b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = (index / 26) - 1;
    }
    out.into_iter().rev().collect()
}

fn extract_xml_tags(xml: &str, tag_name: &str) -> Vec<String> {
    let pattern = format!("<{tag_name}");
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(idx) = xml[pos..].find(&pattern) {
        let abs = pos + idx;
        let after = xml
            .as_bytes()
            .get(abs + pattern.len())
            .copied()
            .unwrap_or(b' ');
        if after != b' ' && after != b'>' && after != b'/' {
            pos = abs + pattern.len();
            continue;
        }
        let Some(end) = xml[abs..].find('>') else {
            break;
        };
        out.push(xml[abs..abs + end + 1].to_owned());
        pos = abs + end + 1;
    }
    out
}

fn extract_tag_text(xml: &str, tag_name: &str) -> Option<String> {
    let pattern = format!("<{tag_name}");
    let start = xml.find(&pattern)?;
    let open_end = xml[start..].find('>')?;
    if xml[start..start + open_end].ends_with('/') {
        return None;
    }
    let content_start = start + open_end + 1;
    let close_pattern = format!("</{tag_name}>");
    let end = xml[content_start..].find(&close_pattern)?;
    Some(unescape_xml(&xml[content_start..content_start + end]))
}

fn xml_attr(tag: &str, attr_name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pattern = format!("{attr_name}={quote}");
        for (idx, _) in tag.match_indices(&pattern) {
            if idx > 0 {
                let prev = tag.as_bytes()[idx - 1];
                if prev != b' ' && prev != b'\t' && prev != b'\n' && prev != b'\r' {
                    continue;
                }
            }
            let val_start = idx + pattern.len();
            let val_end = tag[val_start..].find(quote)?;
            return Some(unescape_xml(&tag[val_start..val_start + val_end]));
        }
    }
    None
}

fn unescape_xml(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            let mut entity = String::new();
            while let Some(&c) = chars.peek() {
                chars.next();
                if c == ';' {
                    break;
                }
                entity.push(c);
                if entity.len() > 12 {
                    break;
                }
            }
            match entity.as_str() {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    if let Ok(code) = u32::from_str_radix(&entity[2..], 16) {
                        if let Some(decoded) = char::from_u32(code) {
                            out.push(decoded);
                        }
                    }
                }
                _ if entity.starts_with('#') => {
                    if let Ok(code) = entity[1..].parse::<u32>() {
                        if let Some(decoded) = char::from_u32(code) {
                            out.push(decoded);
                        }
                    }
                }
                _ => {
                    out.push('&');
                    out.push_str(&entity);
                    out.push(';');
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Largest .xlsx accepted, and the most its parts may add up to unpacked.
const MAX_XLSX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_XLSX_EXPANDED_BYTES: usize = 256 * 1024 * 1024;

/// Lê entradas de um arquivo ZIP (`.xlsx`) a partir do Central Directory e descompacta blocos Stored (0) ou Deflate (8).
pub(crate) fn read_zip_entries(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut total = 0usize;
    ensure!(
        bytes.len() >= 22,
        "A planilha está danificada ou não é um arquivo .xlsx válido."
    );
    // Localiza End of Central Directory (EOCD) signature 0x06054b50
    let min_eocd = bytes.len().saturating_sub(65557);
    let mut eocd_pos = None;
    for i in (min_eocd..=bytes.len() - 22).rev() {
        if bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            eocd_pos = Some(i);
            break;
        }
    }
    let eocd = eocd_pos.context("A planilha está danificada ou não é um arquivo .xlsx válido.")?;
    let total_entries = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
    let cd_offset = u32::from_le_bytes([
        bytes[eocd + 16],
        bytes[eocd + 17],
        bytes[eocd + 18],
        bytes[eocd + 19],
    ]) as usize;

    let mut entries = BTreeMap::new();
    let mut pos = cd_offset;
    for _ in 0..total_entries {
        ensure!(
            pos + 46 <= bytes.len() && bytes[pos..pos + 4] == [0x50, 0x4b, 0x01, 0x02],
            "A planilha está danificada ou não é um arquivo .xlsx válido."
        );
        let method = u16::from_le_bytes([bytes[pos + 10], bytes[pos + 11]]);
        let comp_size = u32::from_le_bytes([
            bytes[pos + 20],
            bytes[pos + 21],
            bytes[pos + 22],
            bytes[pos + 23],
        ]) as usize;
        let uncomp_size = u32::from_le_bytes([
            bytes[pos + 24],
            bytes[pos + 25],
            bytes[pos + 26],
            bytes[pos + 27],
        ]) as usize;
        let name_len = u16::from_le_bytes([bytes[pos + 28], bytes[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[pos + 30], bytes[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([bytes[pos + 32], bytes[pos + 33]]) as usize;
        let local_offset = u32::from_le_bytes([
            bytes[pos + 42],
            bytes[pos + 43],
            bytes[pos + 44],
            bytes[pos + 45],
        ]) as usize;

        ensure!(
            pos + 46 + name_len <= bytes.len(),
            "A planilha está danificada ou não é um arquivo .xlsx válido."
        );
        let name = String::from_utf8_lossy(&bytes[pos + 46..pos + 46 + name_len]).into_owned();
        pos += 46 + name_len + extra_len + comment_len;

        if name.ends_with('/') || uncomp_size > 64 * 1024 * 1024 {
            continue;
        }
        ensure!(
            local_offset + 30 <= bytes.len()
                && bytes[local_offset..local_offset + 4] == [0x50, 0x4b, 0x03, 0x04],
            "A planilha está danificada ou não é um arquivo .xlsx válido."
        );
        let l_name =
            u16::from_le_bytes([bytes[local_offset + 26], bytes[local_offset + 27]]) as usize;
        let l_extra =
            u16::from_le_bytes([bytes[local_offset + 28], bytes[local_offset + 29]]) as usize;
        let data_start = local_offset + 30 + l_name + l_extra;
        ensure!(
            data_start + comp_size <= bytes.len(),
            "A planilha está danificada ou não é um arquivo .xlsx válido."
        );
        let comp_slice = &bytes[data_start..data_start + comp_size];

        total += uncomp_size;
        ensure!(
            total <= MAX_XLSX_EXPANDED_BYTES,
            "A planilha descompactada passa de {} MB; divida o arquivo ou salve só a aba PROCESSOS.",
            MAX_XLSX_EXPANDED_BYTES / (1024 * 1024)
        );
        let decoded = match method {
            0 => comp_slice.to_vec(),
            8 => inflate_raw(comp_slice, uncomp_size)?,
            _ => bail!("A planilha usa um tipo de compactação que o Ponte não reconhece. Salve-a de novo como .xlsx no Excel."),
        };
        entries.insert(name, decoded);
    }
    Ok(entries)
}

// Descompressor RFC 1951 (Deflate) autocontido em Rust puro para leitura de `.xlsx`
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u16> {
        ensure!(
            self.byte_pos < self.data.len(),
            "A planilha está danificada ou não é um arquivo .xlsx válido."
        );
        let bit = ((self.data[self.byte_pos] >> self.bit_pos) & 1) as u16;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(bit)
    }

    fn read_bits(&mut self, count: u8) -> Result<u32> {
        let mut val = 0u32;
        for i in 0..count {
            val |= (self.read_bit()? as u32) << i;
        }
        Ok(val)
    }

    fn align_byte(&mut self) {
        if self.bit_pos > 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }
}

struct HuffmanTree {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl HuffmanTree {
    fn from_lengths(lengths: &[u8]) -> Result<Self> {
        let mut counts = [0u16; 16];
        for &len in lengths {
            ensure!(
                len <= 15,
                "A planilha está danificada ou não é um arquivo .xlsx válido."
            );
            if len > 0 {
                counts[len as usize] += 1;
            }
        }
        let mut offsets = [0u16; 16];
        let mut total = 0u16;
        for bits in 1..=15 {
            offsets[bits] = total;
            total += counts[bits];
        }
        let mut symbols = vec![0u16; total as usize];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let idx = offsets[len as usize] as usize;
                symbols[idx] = sym as u16;
                offsets[len as usize] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..=15 {
            code |= reader.read_bit()? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        bail!("A planilha está danificada ou não é um arquivo .xlsx válido.")
    }
}

/// A ZIP entry that inflates beyond the size it declares is refused: it is
/// either damaged or built to exhaust memory (a "zip bomb").
const INFLATE_OVERFLOW: &str =
    "A planilha tem uma parte compactada maior do que declara; o arquivo parece danificado.";

/// Inflates one Deflate stream, never producing more than `expected_size`
/// bytes (the size the ZIP directory declares).
fn inflate_raw(input: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    const LEN_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    const CL_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    let mut reader = BitReader::new(input);
    let mut out = Vec::with_capacity(expected_size.min(16 * 1024 * 1024));

    loop {
        let bfinal = reader.read_bit()?;
        let btype = reader.read_bits(2)?;
        match btype {
            0 => {
                reader.align_byte();
                ensure!(
                    reader.byte_pos + 4 <= reader.data.len(),
                    "A planilha está danificada ou não é um arquivo .xlsx válido."
                );
                let len = u16::from_le_bytes([
                    reader.data[reader.byte_pos],
                    reader.data[reader.byte_pos + 1],
                ]) as usize;
                reader.byte_pos += 4;
                ensure!(
                    reader.byte_pos + len <= reader.data.len(),
                    "A planilha está danificada ou não é um arquivo .xlsx válido."
                );
                ensure!(out.len() + len <= expected_size, "{INFLATE_OVERFLOW}");
                out.extend_from_slice(&reader.data[reader.byte_pos..reader.byte_pos + len]);
                reader.byte_pos += len;
            }
            1 | 2 => {
                let (lit_tree, dist_tree) = if btype == 1 {
                    let mut lit_lens = vec![0u8; 288];
                    lit_lens[0..=143].fill(8);
                    lit_lens[144..=255].fill(9);
                    lit_lens[256..=279].fill(7);
                    lit_lens[280..=287].fill(8);
                    let dist_lens = vec![5u8; 32];
                    (
                        HuffmanTree::from_lengths(&lit_lens)?,
                        HuffmanTree::from_lengths(&dist_lens)?,
                    )
                } else {
                    let hlit = reader.read_bits(5)? as usize + 257;
                    let hdist = reader.read_bits(5)? as usize + 1;
                    let hclen = reader.read_bits(4)? as usize + 4;
                    let mut cl_lens = [0u8; 19];
                    for i in 0..hclen {
                        cl_lens[CL_ORDER[i]] = reader.read_bits(3)? as u8;
                    }
                    let cl_tree = HuffmanTree::from_lengths(&cl_lens)?;
                    let mut all_lens = Vec::with_capacity(hlit + hdist);
                    while all_lens.len() < hlit + hdist {
                        let sym = cl_tree.decode(&mut reader)?;
                        match sym {
                            0..=15 => all_lens.push(sym as u8),
                            16 => {
                                let prev = *all_lens.last().context(
                                    "A planilha está danificada ou não é um arquivo .xlsx válido.",
                                )?;
                                let repeat = reader.read_bits(2)? as usize + 3;
                                all_lens.extend(std::iter::repeat_n(prev, repeat));
                            }
                            17 => {
                                let repeat = reader.read_bits(3)? as usize + 3;
                                all_lens.extend(std::iter::repeat_n(0, repeat));
                            }
                            18 => {
                                let repeat = reader.read_bits(7)? as usize + 11;
                                all_lens.extend(std::iter::repeat_n(0, repeat));
                            }
                            _ => bail!(
                                "A planilha está danificada ou não é um arquivo .xlsx válido."
                            ),
                        }
                    }
                    (
                        HuffmanTree::from_lengths(&all_lens[..hlit])?,
                        HuffmanTree::from_lengths(&all_lens[hlit..hlit + hdist])?,
                    )
                };

                loop {
                    let sym = lit_tree.decode(&mut reader)? as usize;
                    if sym < 256 {
                        ensure!(out.len() < expected_size, "{INFLATE_OVERFLOW}");
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let len_idx = sym - 257;
                        ensure!(
                            len_idx < LEN_BASE.len(),
                            "A planilha está danificada ou não é um arquivo .xlsx válido."
                        );
                        let length =
                            LEN_BASE[len_idx] + reader.read_bits(LEN_EXTRA[len_idx])? as usize;
                        let dist_sym = dist_tree.decode(&mut reader)? as usize;
                        ensure!(
                            dist_sym < DIST_BASE.len(),
                            "A planilha está danificada ou não é um arquivo .xlsx válido."
                        );
                        let distance =
                            DIST_BASE[dist_sym] + reader.read_bits(DIST_EXTRA[dist_sym])? as usize;
                        ensure!(
                            distance > 0 && distance <= out.len(),
                            "A planilha está danificada ou não é um arquivo .xlsx válido."
                        );
                        ensure!(out.len() + length <= expected_size, "{INFLATE_OVERFLOW}");
                        for _ in 0..length {
                            let b = out[out.len() - distance];
                            out.push(b);
                        }
                    }
                }
            }
            _ => bail!("A planilha está danificada ou não é um arquivo .xlsx válido."),
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

// ============================================================================
// Interface gráfica "Atendimentos" (Controle do gabinete do SAIOP)
// ============================================================================

#[cfg(feature = "desktop")]
use crate::theme::{self, c, icon, S1, S2, S3, S4};
#[cfg(feature = "desktop")]
use crate::widgets::{self, fold, form_section, labeled, pick, Tone};

#[cfg(feature = "desktop")]
/// Width of the side panel (the ficha).
const FICHA_WIDTH: f32 = 440.0;
#[cfg(feature = "desktop")]
const ROW_HEIGHT: f32 = 40.0;

#[cfg(feature = "desktop")]
/// The pill colour of a situation.
fn status_tone(status: &str) -> Tone {
    match status {
        "em_analise" | "em_elaboracao" => Tone::Info,
        "aguardando_resposta" | "suspenso" => Tone::Warn,
        "aguardando_deliberacao" => Tone::Violet,
        "encerrado" => Tone::Ok,
        _ => Tone::Neutral,
    }
}

#[cfg(feature = "desktop")]
/// `dd/mm`, with the year only when it is not this year's.
fn short_date(iso: &str, today: NaiveDate) -> String {
    match NaiveDate::parse_from_str(iso.trim(), "%Y-%m-%d") {
        Ok(d) if d.year() == today.year() => d.format("%d/%m").to_string(),
        Ok(d) => d.format("%d/%m/%y").to_string(),
        Err(_) => crate::labels::date(iso),
    }
}

#[cfg(feature = "desktop")]
/// The deadline of an atendimento as a pill (text, tone, hover).
fn due_pill(att: &SaiopAttendance, today: NaiveDate) -> (String, Tone, Option<String>) {
    if !att.due_date.is_empty() {
        let short = short_date(&att.due_date, today);
        match att.due_delta_days(today) {
            Some(delta) if delta > 0 => (
                format!("Vencido · {short}"),
                Tone::Danger,
                Some(format!(
                    "Vencido há {}",
                    crate::labels::count(delta as usize, "dia", "dias")
                )),
            ),
            Some(0) => ("Hoje".to_owned(), Tone::Warn, Some("Vence hoje".to_owned())),
            Some(delta) => (
                short,
                Tone::Info,
                Some(format!(
                    "Vence em {}",
                    crate::labels::count((-delta) as usize, "dia", "dias")
                )),
            ),
            None => (short, Tone::Info, None),
        }
    } else if !att.due_date_raw.is_empty() {
        (
            format!("Conferir: {}", att.due_date_raw),
            Tone::Warn,
            Some("O prazo veio como texto na planilha; confira e informe a data.".to_owned()),
        )
    } else {
        ("Sem prazo".to_owned(), Tone::Neutral, None)
    }
}

#[cfg(feature = "desktop")]
/// A message of the screen: one soft-coloured line with a close button.
/// Returns true when it was closed.
fn notice_line(ui: &mut egui::Ui, text: &str, tone: Tone) -> bool {
    let (fg, bg) = tone.colors();
    let mut closed = false;
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let glyph = if tone == Tone::Danger {
                    icon::ERROR
                } else {
                    icon::INFO
                };
                ui.label(RichText::new(glyph).size(16.0).color(fg));
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - 36.0).max(80.0));
                    ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(ui, icon::CLOSE, "Fechar").clicked() {
                        closed = true;
                    }
                });
            });
        });
    closed
}

#[cfg(feature = "desktop")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SaiopQueue {
    All,
    MyPending,
    Unassigned,
    Overdue,
    DueToday,
    DueSoon,
    WaitingResponse,
    UasReturn,
    WaitingSignature,
    StaleDays,
    NeedsReview,
}

#[cfg(feature = "desktop")]
impl SaiopQueue {
    fn label(self) -> &'static str {
        match self {
            Self::All => "Todos",
            Self::MyPending => "Minhas pendências",
            Self::Unassigned => "Sem responsável",
            Self::Overdue => "Prazo vencido",
            Self::DueToday => "Vence hoje",
            Self::DueSoon => "Vence nos próximos dias",
            Self::WaitingResponse => "Aguardando resposta",
            Self::UasReturn => "Retorno para UAS",
            Self::WaitingSignature => "Aguardando assinatura",
            Self::StaleDays => "Sem atualização no período",
            Self::NeedsReview => "Com alerta de revisão",
        }
    }
}

#[cfg(feature = "desktop")]
/// Sortable columns of the grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SaiopSort {
    Reference,
    Subject,
    Assignee,
    Entry,
    Status,
    Due,
    Age,
    Destination,
}

#[cfg(feature = "desktop")]
impl SaiopSort {
    fn title(self) -> &'static str {
        match self {
            Self::Reference => "Referência",
            Self::Subject => "Assunto",
            Self::Assignee => "Responsável",
            Self::Entry => "Entrada",
            Self::Status => "Situação",
            Self::Due => "Prazo",
            Self::Age => "Tempo",
            Self::Destination => "Destino / Saída",
        }
    }
    /// Width the column asks for (the subject: its minimum).
    fn width(self) -> f32 {
        match self {
            Self::Reference => 175.0,
            Self::Subject => 170.0,
            Self::Assignee => 120.0,
            Self::Entry => 90.0,
            Self::Status => 180.0,
            Self::Due => 140.0,
            Self::Age => 110.0,
            Self::Destination => 150.0,
        }
    }
    /// A key that sorts naturally for this column.
    fn key(self, att: &SaiopAttendance, today: NaiveDate) -> (String, i64) {
        match self {
            Self::Reference => (att.reference_norm.clone(), 0),
            Self::Subject => (att.subject.to_lowercase(), 0),
            Self::Assignee => (att.assignee.to_lowercase(), 0),
            Self::Entry => (att.entry_date.clone(), 0),
            Self::Status => (status_label(&att.status).to_owned(), 0),
            Self::Due => (
                if att.due_date.is_empty() {
                    "9999".to_owned()
                } else {
                    att.due_date.clone()
                },
                0,
            ),
            Self::Age => (String::new(), -att.age_days(today).unwrap_or(0)),
            Self::Destination => (
                if att.sent_to.is_empty() {
                    att.exit_date.clone()
                } else {
                    att.sent_to.to_lowercase()
                },
                0,
            ),
        }
    }
}

#[cfg(feature = "desktop")]
/// What the two numbers of the "Tempo" column mean.
const TIME_HELP: &str = "Dias desde a entrada • dias fora do setor, aguardando resposta";

#[cfg(feature = "desktop")]
/// The columns of the table for the width it has, in display order, with the
/// width each asks for (the subject's is its minimum: it takes what is left).
/// Referência, Assunto, Situação and Prazo always show; then, as room allows,
/// Responsável, Entrada, Tempo and Destino / Saída, in this order.
fn visible_columns(width: f32) -> Vec<(SaiopSort, f32)> {
    let mut shown = vec![
        SaiopSort::Reference,
        SaiopSort::Subject,
        SaiopSort::Status,
        SaiopSort::Due,
    ];
    let mut used: f32 = shown.iter().map(|c| c.width()).sum();
    for extra in [
        SaiopSort::Assignee,
        SaiopSort::Entry,
        SaiopSort::Age,
        SaiopSort::Destination,
    ] {
        if used + extra.width() > width {
            break;
        }
        used += extra.width();
        shown.push(extra);
    }
    [
        SaiopSort::Reference,
        SaiopSort::Subject,
        SaiopSort::Assignee,
        SaiopSort::Entry,
        SaiopSort::Status,
        SaiopSort::Due,
        SaiopSort::Age,
        SaiopSort::Destination,
    ]
    .into_iter()
    .filter(|c| shown.contains(c))
    .map(|c| (c, c.width()))
    .collect()
}

#[cfg(feature = "desktop")]
/// What an empty table has to say (see `SaiopPanel::table`).
struct EmptyCtx {
    /// Atendimentos in the whole library.
    total: usize,
    /// Number of the document the list is narrowed to, when it is.
    document: Option<(i64, String)>,
    /// Atendimentos of that document.
    in_document: usize,
    /// The search text, trimmed.
    query: String,
    /// The other active filters, named for the person.
    active: Vec<String>,
}

#[cfg(feature = "desktop")]
pub struct SaiopPanel {
    query: String,
    assignee_filter: String,
    status_filter: String,
    origin_filter: String,
    dest_filter: String,
    queue: SaiopQueue,
    due_soon_days: i64,
    stale_days_threshold: i64,
    filters_open: bool,
    import_open: bool,
    sort: SaiopSort,
    sort_ascending: bool,
    /// "Ligar a um documento": the search that finds it.
    link_picker: crate::doc_picker::DocumentPicker,
    selected_id: Option<i64>,
    editor: Option<SaiopAttendance>,
    saved_snapshot: Option<SaiopAttendance>,
    base_version: Option<i64>,
    xlsx_path: String,
    tramitado_keep_open: bool,
    import_preview: Option<SaiopImportPreview>,
    pending_undo: Option<(SaiopImportBatch, usize, usize)>,
    new_task_title: String,
    new_event_note: String,
    message: String,
    error: bool,
    /// Document selected in the library, when there is one: new entries are
    /// linked to it and the person may narrow the list to it.
    context_document: Option<(i64, String)>,
    /// The list is narrowed to this document's atendimentos, only when the
    /// person asked (never inherited from the library).
    document_filter: Option<i64>,
    /// The next frame gives the keyboard focus to the search box.
    focus_search: bool,
    /// The next frame gives the keyboard focus to the ficha's first field.
    focus_ficha: bool,
    /// "Anexos" block of the atendimento being edited.
    attachments: crate::attachments::AttachmentsPanel,
    /// "Excluir…" was clicked for this atendimento; waiting for confirmation.
    confirm_delete: Option<i64>,
    /// Task the person asked to open in the Tarefas panel.
    open_task: Option<Task>,
    /// A use of a resource to open, or a new one to start, in Recursos.
    use_request: Option<crate::resources_ui::UseRequest>,
    /// Development only: how far the ficha is scrolled in the pictures.
    #[cfg(feature = "shots")]
    shots_scroll: f32,
}

#[cfg(feature = "desktop")]
impl Default for SaiopPanel {
    fn default() -> Self {
        Self {
            query: String::new(),
            assignee_filter: String::new(),
            status_filter: String::new(),
            origin_filter: String::new(),
            dest_filter: String::new(),
            queue: SaiopQueue::All,
            due_soon_days: 5,
            stale_days_threshold: 15,
            filters_open: false,
            import_open: false,
            sort: SaiopSort::Entry,
            sort_ascending: false,
            link_picker: Default::default(),
            selected_id: None,
            editor: None,
            saved_snapshot: None,
            base_version: None,
            xlsx_path: String::new(),
            tramitado_keep_open: true,
            import_preview: None,
            pending_undo: None,
            new_task_title: String::new(),
            new_event_note: String::new(),
            message: String::new(),
            error: false,
            context_document: None,
            document_filter: None,
            focus_search: false,
            focus_ficha: false,
            attachments: Default::default(),
            confirm_delete: None,
            open_task: None,
            use_request: None,
            #[cfg(feature = "shots")]
            shots_scroll: 0.0,
        }
    }
}

#[cfg(feature = "desktop")]
impl SaiopPanel {
    pub fn dirty(&self) -> bool {
        self.editor != self.saved_snapshot
    }

    /// Task the person clicked "Abrir" on in the ficha, for the app to show
    /// in the Tarefas panel.
    pub fn take_open_task(&mut self) -> Option<Task> {
        self.open_task.take()
    }

    /// What the person asked about the uses of resources in the ficha
    /// ("Abrir" a use, "Reservar recurso…"), for the app to serve.
    pub fn take_use_request(&mut self) -> Option<crate::resources_ui::UseRequest> {
        self.use_request.take()
    }

    /// Tells the panel which document the library has open (id and number).
    /// Called by the app before `show`. It only pre-links a new entry and
    /// offers "Só os de …": the list is never narrowed by it.
    pub fn set_context_document(&mut self, document: Option<(i64, String)>) {
        self.context_document = document;
    }

    /// Shows every atendimento again (ends the narrowing to one document).
    pub fn clear_document_filter(&mut self) {
        self.document_filter = None;
    }

    /// Escape: closes the import window when it is idle, else the ficha when
    /// it has no unsaved changes. True when it did something or when
    /// something is open that could not be closed.
    pub fn key_escape(&mut self) -> bool {
        if self.import_open {
            if self.import_preview.is_none() && self.pending_undo.is_none() {
                self.import_open = false;
            }
            return true;
        }
        if self.editor.is_some() {
            if !self.dirty() {
                self.close_ficha();
            }
            return true;
        }
        false
    }

    /// Ctrl+S: with a ficha open and unsaved, what "Salvar" does. True when
    /// a save was attempted.
    pub fn key_save(&mut self, store: &mut Store) -> bool {
        let unsaved = self.editor.as_ref().is_some_and(|e| e.id == 0) || self.dirty();
        if self.editor.is_none() || !unsaved {
            return false;
        }
        self.save_current(store);
        true
    }

    /// Ctrl+F: the next frame puts the keyboard in the search box.
    pub fn key_find(&mut self) {
        self.focus_search = true;
    }

    /// Ctrl+N: what "Nova entrada" does. True when handled.
    pub fn key_new(&mut self, store: &Store) -> bool {
        self.new_entry(store, self.context_document.clone());
        true
    }

    /// The atendimento in the ficha, when it is saved.
    pub fn open_attendance_id(&self) -> Option<i64> {
        self.editor.as_ref().filter(|e| e.id > 0).map(|e| e.id)
    }

    fn close_ficha(&mut self) {
        self.confirm_delete = None;
        self.editor = None;
        self.saved_snapshot = None;
        self.selected_id = None;
        self.focus_ficha = false;
    }

    /// "Nova entrada": a blank ficha, linked to `document` when given.
    fn new_entry(&mut self, store: &Store, document: Option<(i64, String)>) {
        if self.dirty() {
            self.set_notice(
                "Salve ou descarte o atendimento em edição antes de iniciar outro.",
                true,
            );
            return;
        }
        let mut fresh = SaiopAttendance::new_manual();
        if let Ok(cfg) = store.load_intranet_config() {
            fresh.assignee = cfg.member_name;
        }
        // With a document, the new entry is a passage of that document:
        // linked, with its number ("Desligar" undoes it).
        if let Some((id, number)) = document {
            fresh.document_id = Some(id);
            fresh.reference_norm = normalize_reference(&number);
            fresh.reference_raw = number;
        }
        self.load_into_editor(store, fresh);
    }

    /// Clears the search, the filters and the queue (not the narrowing to a
    /// document: that has its own "Ver todos").
    fn clear_filters(&mut self) {
        self.query.clear();
        self.assignee_filter.clear();
        self.origin_filter.clear();
        self.dest_filter.clear();
        self.status_filter.clear();
        self.queue = SaiopQueue::All;
    }

    /// The setting "Criar atendimento ao importar do SIGADOC", for Ajustes.
    /// Saves on change.
    pub fn auto_attendance_setting(ui: &mut egui::Ui, store: &mut Store) {
        // On a connected computer the import is recorded by the main one,
        // with its own setting: the box would have no effect here.
        let on_client = store
            .load_intranet_config()
            .is_ok_and(|cfg| cfg.mode == crate::intranet::IntranetMode::Client);
        let mut auto = store.auto_attendance_enabled().unwrap_or(true);
        let response = ui.add_enabled(
            !on_client,
            egui::Checkbox::new(
                &mut auto,
                RichText::new("Criar atendimento ao importar do SIGADOC")
                    .size(theme::SIZE_SMALL)
                    .color(c().muted),
            ),
        );
        let response = if on_client {
            response.on_disabled_hover_text("Definido no computador principal: é ele que registra as importações da equipe. Sem conexão, a cópia local segue a configuração daqui até a próxima sincronização.")
        } else {
            response.on_hover_text("Na primeira importação de cada documento do SIGADOC, registra aqui a entrada dele no gabinete: referência, assunto, data, responsável e ligação ao documento. Documentos juntados a outro não entram. Você pode editar ou excluir o atendimento depois.")
        };
        let error_id = ui.make_persistent_id("saiop-auto-attendance-error");
        if response.changed() {
            let message = match store.set_auto_attendance(auto) {
                Ok(()) => String::new(),
                Err(e) => format!("Não foi possível salvar a preferência: {e}"),
            };
            ui.data_mut(|d| d.insert_temp(error_id, message));
        }
        let error: String = ui.data(|d| d.get_temp(error_id)).unwrap_or_default();
        if !error.is_empty() {
            let (fg, _) = Tone::Danger.colors();
            ui.label(RichText::new(error).size(theme::SIZE_SMALL).color(fg));
        }
    }

    fn set_notice(&mut self, msg: impl Into<String>, error: bool) {
        self.message = msg.into();
        self.error = error;
    }

    /// Opens an attendance in the ficha, unless another edit is pending.
    pub fn focus_attendance(&mut self, store: &Store, id: i64) -> bool {
        if self.dirty() {
            return self.selected_id == Some(id);
        }
        let Ok(list) = store.list_saiop_attendances() else {
            return false;
        };
        let Some(att) = list.into_iter().find(|a| a.id == id) else {
            return false;
        };
        self.load_into_editor(store, att);
        true
    }

    fn load_into_editor(&mut self, store: &Store, att: SaiopAttendance) {
        self.base_version = if att.id > 0 {
            store.item_version("saiop", att.id).ok().map(|v| v.version)
        } else {
            None
        };
        self.selected_id = if att.id > 0 { Some(att.id) } else { None };
        self.saved_snapshot = Some(att.clone());
        self.editor = Some(att);
        self.focus_ficha = true;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, store: &mut Store) -> Option<i64> {
        let mut open_document = None;
        let today = Local::now().date_naive();
        let all_items = match store.list_saiop_attendances() {
            Ok(list) => list,
            Err(e) => {
                notice_line(
                    ui,
                    &format!("Não foi possível carregar os atendimentos: {e}"),
                    Tone::Danger,
                );
                return None;
            }
        };

        let distinct_refs: BTreeSet<&str> = all_items
            .iter()
            .map(|a| a.reference_norm.as_str())
            .filter(|r| !r.is_empty())
            .collect();
        let open_count = all_items.iter().filter(|a| a.is_open()).count();
        let overdue_count = all_items
            .iter()
            .filter(|a| a.due_delta_days(today).is_some_and(|d| d > 0))
            .count();

        // The ficha is a panel on the right; the rest is drawn after it.
        if self.editor.is_some() {
            self.show_ficha(ui, store, &all_items, &mut open_document);
        }
        // The area draws to the edges of the window (the ficha touches them);
        // the rest keeps its room inside what the panel left.
        let room = ui.available_rect_before_wrap();
        let room = egui::Rect::from_min_max(
            room.min + egui::vec2(20.0, 14.0),
            room.max - egui::vec2(20.0, 0.0),
        );
        let mut content = ui.new_child(egui::UiBuilder::new().max_rect(room));
        let ui = &mut content;

        ui.spacing_mut().item_spacing.y = S2;
        // ---- Area header ---------------------------------------------------
        ui.horizontal(|ui| {
            widgets::heading(ui, "Atendimentos");
            crate::help_ui::help(ui, format!(
                    "Controle dos atendimentos do gabinete: {} no total, {}.",
                    crate::labels::count(all_items.len(), "atendimento", "atendimentos"),
                    crate::labels::count(distinct_refs.len(), "referência distinta", "referências distintas")
                ));
            ui.add_space(S2);
            widgets::pill(ui, format!("{open_count} em aberto"), Tone::Info);
            if overdue_count > 0 {
                widgets::pill(ui, format!("{overdue_count} com prazo vencido"), Tone::Danger);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(widgets::primary(format!("{} Nova entrada", icon::PLUS)))
                    .on_hover_text("Registra um atendimento novo. Com um documento aberto, o atendimento já vem ligado a ele.")
                    .clicked()
                {
                    // With a document open in the library, the new entry is a
                    // passage of that document (whatever the list filter).
                    self.new_entry(store, self.context_document.clone());
                }
                if ui
                    .add(widgets::secondary("Importar planilha"))
                    .on_hover_text("Lê a aba PROCESSOS de um arquivo .xlsx do controle do gabinete.")
                    .clicked()
                {
                    self.import_open = true;
                }
            });
        });

        // Imports (often made from the browser, with no window to tell) whose
        // automatic atendimento could not be recorded.
        let failures = store.auto_attendance_failures().unwrap_or_default();
        if !failures.is_empty() {
            let (fg, bg) = Tone::Warn.colors();
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon::WARNING).size(16.0).color(fg));
                        ui.label(
                            RichText::new(format!(
                                "{} sem o atendimento automático",
                                crate::labels::count(failures.len(), "importação", "importações")
                            ))
                            .font(theme::font_strong(theme::SIZE_BODY))
                            .color(fg),
                        );
                    });
                    for failure in failures.iter().take(5) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&failure.number).font(theme::font_strong(theme::SIZE_BODY)))
                                .on_hover_text(format!(
                                    "Importado {}",
                                    crate::labels::elapsed(&failure.at).unwrap_or_default()
                                ));
                            widgets::small_muted(ui, &failure.message);
                            if ui.add(widgets::secondary("Tentar de novo")).clicked() {
                                match store.retry_auto_attendance(failure.document_id) {
                                    Ok(()) => self.set_notice(
                                        format!("Atendimento de {} registrado.", failure.number),
                                        false,
                                    ),
                                    Err(e) => {
                                        self.set_notice(format!("Não foi possível registrar o atendimento: {e:#}"), true)
                                    }
                                }
                            }
                            if ui
                                .add(widgets::quiet("Dispensar"))
                                .on_hover_text("Tira o aviso da tela. Se quiser, registre o atendimento à mão.")
                                .clicked()
                            {
                                if let Err(e) =
                                    store.dismiss_auto_attendance_failure(failure.document_id)
                                {
                                    self.set_notice(format!("Não foi possível dispensar o aviso: {e}"), true);
                                }
                            }
                        });
                    }
                    if failures.len() > 5 {
                        widgets::small_muted(ui, format!("e mais {}…", failures.len() - 5));
                    }
                });
        }

        if !self.message.is_empty() {
            let tone = if self.error { Tone::Danger } else { Tone::Info };
            if notice_line(ui, &self.message.clone(), tone) {
                self.message.clear();
            }
        }

        self.import_window(ui, store);

        let current_member = store
            .load_intranet_config()
            .map(|c| c.member_name)
            .unwrap_or_default()
            .to_lowercase();
        // Queue membership is independent of the text filters, so the combo
        // can show how many items each queue holds.
        let due_soon_days = self.due_soon_days;
        let stale_days_threshold = self.stale_days_threshold;
        let in_queue = |att: &SaiopAttendance, queue: SaiopQueue| match queue {
            SaiopQueue::All => true,
            SaiopQueue::MyPending => {
                att.is_open()
                    && !current_member.is_empty()
                    && att.assignee.to_lowercase().contains(&current_member)
            }
            SaiopQueue::Unassigned => att.is_open() && att.assignee.trim().is_empty(),
            SaiopQueue::Overdue => att.due_delta_days(today).is_some_and(|d| d > 0),
            SaiopQueue::DueToday => att.due_delta_days(today) == Some(0),
            SaiopQueue::DueSoon => att
                .due_delta_days(today)
                .is_some_and(|d| d < 0 && (-d) <= due_soon_days),
            SaiopQueue::WaitingResponse => att.status == "aguardando_resposta",
            SaiopQueue::UasReturn => att.is_open() && att.uas_return == "sim",
            SaiopQueue::WaitingSignature => att.is_open() && att.signed_status != "sim",
            SaiopQueue::StaleDays => {
                att.is_open()
                    && att
                        .stage_days(today)
                        .is_some_and(|d| d >= stale_days_threshold)
            }
            SaiopQueue::NeedsReview => !att.review_flags.is_empty(),
        };
        let queues = [
            SaiopQueue::All,
            SaiopQueue::MyPending,
            SaiopQueue::Unassigned,
            SaiopQueue::Overdue,
            SaiopQueue::DueToday,
            SaiopQueue::DueSoon,
            SaiopQueue::WaitingResponse,
            SaiopQueue::UasReturn,
            SaiopQueue::WaitingSignature,
            SaiopQueue::StaleDays,
            SaiopQueue::NeedsReview,
        ];
        let queue_count =
            |queue: SaiopQueue| all_items.iter().filter(|a| in_queue(a, queue)).count();
        let advanced_active = !self.assignee_filter.trim().is_empty()
            || !self.origin_filter.trim().is_empty()
            || !self.dest_filter.trim().is_empty()
            || !self.status_filter.is_empty();

        // ---- Queue, search, filters ------------------------------------------
        // Wrapped: with the ficha open the row is narrow, and what does not
        // fit goes to a second line instead of running under the panel.
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("saiop-queue")
                .selected_text(format!(
                    "Fila: {} ({})",
                    self.queue.label(),
                    queue_count(self.queue)
                ))
                .width(250.0)
                .show_ui(ui, |ui| {
                    for q in queues {
                        ui.selectable_value(
                            &mut self.queue,
                            q,
                            format!("{} ({})", q.label(), queue_count(q)),
                        );
                    }
                });
            let mut reserve = 130.0;
            if self.queue == SaiopQueue::DueSoon {
                widgets::muted(ui, "até");
                ui.add(
                    egui::DragValue::new(&mut self.due_soon_days)
                        .range(1..=60)
                        .suffix(" dias"),
                );
                reserve += 110.0;
            } else if self.queue == SaiopQueue::StaleDays {
                widgets::muted(ui, "parado há");
                ui.add(
                    egui::DragValue::new(&mut self.stale_days_threshold)
                        .range(1..=365)
                        .suffix(" dias"),
                );
                reserve += 130.0;
            }
            let clearable =
                advanced_active || !self.query.is_empty() || self.queue != SaiopQueue::All;
            if clearable {
                reserve += 70.0;
            }
            let width = (ui.available_width() - reserve).max(160.0);
            let search = widgets::search(
                ui,
                &mut self.query,
                "Buscar referência, assunto ou observação",
                width,
            );
            if self.focus_search {
                self.focus_search = false;
                search.request_focus();
            }
            if widgets::chip(
                ui,
                self.filters_open || advanced_active,
                format!(
                    "{} Filtros {}",
                    icon::FILTER,
                    if self.filters_open {
                        icon::UP
                    } else {
                        icon::DOWN
                    }
                ),
            )
            .clicked()
            {
                self.filters_open = !self.filters_open;
            }
            if clearable && ui.add(widgets::quiet("Limpar")).clicked() {
                self.clear_filters();
            }
        });
        if self.filters_open || advanced_active {
            widgets::card(ui, |ui| {
                ui.columns(4, |cols| {
                    labeled(&mut cols[0], "Responsável", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.assignee_filter)
                                .hint_text("Nome…")
                                .desired_width(f32::INFINITY),
                        );
                    });
                    labeled(&mut cols[1], "Recebido de", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.origin_filter)
                                .hint_text("Origem…")
                                .desired_width(f32::INFINITY),
                        );
                    });
                    labeled(&mut cols[2], "Enviado para", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.dest_filter)
                                .hint_text("Destino…")
                                .desired_width(f32::INFINITY),
                        );
                    });
                    labeled(&mut cols[3], "Situação", |ui| {
                        pick(
                            ui,
                            "saiop-status-filter",
                            if self.status_filter.is_empty() {
                                "Todas".to_owned()
                            } else {
                                status_label(&self.status_filter).to_owned()
                            },
                            |ui| {
                                ui.selectable_value(
                                    &mut self.status_filter,
                                    String::new(),
                                    "Todas",
                                );
                                for (k, label) in STATUS_OPTIONS {
                                    ui.selectable_value(
                                        &mut self.status_filter,
                                        (*k).to_owned(),
                                        *label,
                                    );
                                }
                            },
                        );
                    });
                });
                // Offered, never imposed: the document open in the library.
                if let (Some((context_id, context_number)), None) =
                    (self.context_document.clone(), self.document_filter)
                {
                    ui.add_space(S1);
                    if ui
                        .add(widgets::quiet(format!("Só os de {context_number}")))
                        .on_hover_text(
                            "Mostra só os atendimentos do documento aberto em Documentos.",
                        )
                        .clicked()
                    {
                        self.document_filter = Some(context_id);
                    }
                }
            });
        }

        // The list is narrowed to one document: said plainly, with the way out.
        if let Some(document) = self.document_filter {
            let number = all_items
                .iter()
                .find(|a| a.document_id == Some(document) && !a.reference_raw.is_empty())
                .map(|a| a.reference_raw.clone())
                .or_else(|| {
                    self.context_document
                        .as_ref()
                        .filter(|d| d.0 == document)
                        .map(|d| d.1.clone())
                });
            let p = c();
            egui::Frame::new()
                .fill(p.primary_soft)
                .stroke(egui::Stroke::new(1.0, p.primary))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let text = match &number {
                            Some(n) => format!("Mostrando só os atendimentos de {n}"),
                            None => "Mostrando só os atendimentos de um documento".to_owned(),
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(text)
                                    .font(theme::font_strong(theme::SIZE_BODY))
                                    .color(p.primary),
                            )
                            .truncate(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(widgets::quiet("Ver todos")).clicked() {
                                self.clear_document_filter();
                            }
                        });
                    });
                });
        }

        let q_norm = self.query.trim().to_lowercase();
        let resp_norm = self.assignee_filter.trim().to_lowercase();
        let orig_norm = self.origin_filter.trim().to_lowercase();
        let dest_norm = self.dest_filter.trim().to_lowercase();

        let mut filtered: Vec<&SaiopAttendance> = all_items
            .iter()
            .filter(|att| {
                if let Some(document) = self.document_filter {
                    if att.document_id != Some(document) {
                        return false;
                    }
                }
                if !self.status_filter.is_empty() && att.status != self.status_filter {
                    return false;
                }
                if !resp_norm.is_empty() && !att.assignee.to_lowercase().contains(&resp_norm) {
                    return false;
                }
                if !orig_norm.is_empty() && !att.received_from.to_lowercase().contains(&orig_norm) {
                    return false;
                }
                if !dest_norm.is_empty() && !att.sent_to.to_lowercase().contains(&dest_norm) {
                    return false;
                }
                if !q_norm.is_empty() {
                    let hay = format!(
                        "{} {} {} {} {}",
                        att.reference_raw,
                        att.subject,
                        att.observation,
                        att.verification_raw,
                        att.assignee
                    )
                    .to_lowercase();
                    if !hay.contains(&q_norm) {
                        return false;
                    }
                }
                in_queue(att, self.queue)
            })
            .collect();
        // The columns depend on the room the table has; a sorted column that
        // is hidden falls back to "Entrada" (without forgetting the choice).
        let columns = visible_columns(ui.available_width() - 12.0);
        let (sort, ascending) = if columns.iter().any(|(col, _)| *col == self.sort) {
            (self.sort, self.sort_ascending)
        } else {
            (SaiopSort::Entry, false)
        };
        filtered.sort_by_cached_key(|att| sort.key(att, today));
        if !ascending {
            filtered.reverse();
        }

        // ---- Table -----------------------------------------------------------
        let empty = EmptyCtx {
            total: all_items.len(),
            document: self.document_filter.map(|id| {
                let number = all_items
                    .iter()
                    .find(|a| a.document_id == Some(id) && !a.reference_raw.is_empty())
                    .map(|a| a.reference_raw.clone())
                    .or_else(|| {
                        self.context_document
                            .as_ref()
                            .filter(|d| d.0 == id)
                            .map(|d| d.1.clone())
                    })
                    .unwrap_or_default();
                (id, number)
            }),
            in_document: self
                .document_filter
                .map(|id| {
                    all_items
                        .iter()
                        .filter(|a| a.document_id == Some(id))
                        .count()
                })
                .unwrap_or(0),
            query: self.query.trim().to_owned(),
            active: {
                let mut active = Vec::new();
                if self.queue != SaiopQueue::All {
                    active.push(format!("fila “{}”", self.queue.label()));
                }
                if !self.status_filter.is_empty() {
                    active.push(format!("situação “{}”", status_label(&self.status_filter)));
                }
                if !self.assignee_filter.trim().is_empty() {
                    active.push(format!("responsável “{}”", self.assignee_filter.trim()));
                }
                if !self.origin_filter.trim().is_empty() {
                    active.push(format!("recebido de “{}”", self.origin_filter.trim()));
                }
                if !self.dest_filter.trim().is_empty() {
                    active.push(format!("enviado para “{}”", self.dest_filter.trim()));
                }
                active
            },
        };
        let below = 30.0;
        let table_height = (ui.available_height() - below).max(120.0);
        self.table(ui, store, &filtered, &columns, &empty, today, table_height);

        let count_text = if filtered.len() != all_items.len() {
            format!(
                "{} de {}. Clique no título de uma coluna para ordenar.",
                filtered.len(),
                crate::labels::count(all_items.len(), "atendimento", "atendimentos")
            )
        } else {
            format!(
                "{}. Clique no título de uma coluna para ordenar.",
                crate::labels::count(all_items.len(), "atendimento", "atendimentos")
            )
        };
        widgets::small_muted(ui, count_text);

        open_document
    }

    /// The table of atendimentos, in one card.
    #[allow(clippy::too_many_arguments)]
    fn table(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        filtered: &[&SaiopAttendance],
        columns: &[(SaiopSort, f32)],
        empty: &EmptyCtx,
        today: NaiveDate,
        height: f32,
    ) {
        use egui::{pos2, vec2, Rect, Sense, Stroke, UiBuilder};
        let p = c();
        let min_total: f32 = columns.iter().map(|(_, w)| w).sum();
        // What the header marks: the sorted column, or "Entrada" when the
        // chosen one is hidden.
        let (sorted_by, sorted_ascending) = if columns.iter().any(|(col, _)| *col == self.sort) {
            (self.sort, self.sort_ascending)
        } else {
            (SaiopSort::Entry, false)
        };
        widgets::card_frame()
            .inner_margin(egui::Margin::ZERO)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::both()
                    .id_salt("saiop-grid-scroll")
                    .auto_shrink([false, true])
                    .max_height(height)
                    .show(ui, |ui| {
                        // The subject takes whatever the table has to spare.
                        let avail = ui.available_width();
                        let extra = (avail - min_total).max(0.0);
                        let widths: Vec<f32> = columns
                            .iter()
                            .map(|(col, w)| if *col == SaiopSort::Subject { w + extra } else { *w })
                            .collect();
                        let total = widths.iter().sum::<f32>();
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        // Header row
                        let (head, _) = ui.allocate_exact_size(vec2(total, 36.0), Sense::hover());
                        let mut x = head.left();
                        for ((column, _), w) in columns.iter().zip(&widths) {
                            let cell = Rect::from_min_size(pos2(x + 6.0, head.top()), vec2(w - 8.0, head.height()));
                            x += w;
                            let mut child = ui.new_child(
                                UiBuilder::new()
                                    .max_rect(cell)
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            );
                            let arrow = if sorted_by == *column {
                                format!(" {}", if sorted_ascending { icon::UP } else { icon::DOWN })
                            } else {
                                String::new()
                            };
                            let header = child.add(
                                egui::Button::new(
                                    RichText::new(format!("{}{arrow}", column.title()))
                                        .font(theme::font_strong(theme::SIZE_SMALL))
                                        .color(p.muted),
                                )
                                .frame(false),
                            );
                            let header = if *column == SaiopSort::Age {
                                header.on_hover_text(format!("{TIME_HELP}. Clique para ordenar."))
                            } else {
                                header.on_hover_text("Ordenar por esta coluna")
                            };
                            let clicked = header.clicked();
                            if clicked {
                                if sorted_by == *column {
                                    self.sort = *column;
                                    self.sort_ascending = !sorted_ascending;
                                } else {
                                    self.sort = *column;
                                    self.sort_ascending =
                                        !matches!(column, SaiopSort::Entry | SaiopSort::Age);
                                }
                            }
                        }
                        ui.painter()
                            .hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0, p.outline));

                        for att in filtered {
                            let is_sel = self.selected_id == Some(att.id);
                            // The row answers by the atendimento's own id, so a
                            // click never lands on a neighbour when the list
                            // changes under the pointer.
                            let (row, _) =
                                ui.allocate_exact_size(vec2(total, ROW_HEIGHT), Sense::hover());
                            let response =
                                ui.interact(row, ui.scope_id().with(("saiop-row", att.id)), Sense::click());
                            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                            if is_sel {
                                ui.painter().rect_filled(row, 0.0, p.primary_soft);
                                ui.painter().rect_filled(
                                    Rect::from_min_size(row.min, vec2(3.0, row.height())),
                                    0.0,
                                    p.primary,
                                );
                            } else if response.hovered() {
                                ui.painter().rect_filled(row, 0.0, p.bg);
                            }
                            ui.painter()
                                .hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, p.outline));

                            let mut x = row.left();
                            for ((column, _), w) in columns.iter().zip(&widths) {
                                let cell = Rect::from_min_size(
                                    pos2(x + 12.0, row.top()),
                                    vec2(w - 16.0, row.height()),
                                );
                                x += w;
                                let mut child = ui.new_child(
                                    UiBuilder::new()
                                        .max_rect(cell)
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );
                                child.set_clip_rect(cell.intersect(ui.clip_rect()));
                                let text = |child: &mut egui::Ui, t: String, color: egui::Color32| {
                                    child.add(
                                        egui::Label::new(RichText::new(t).color(color))
                                            .truncate()
                                            .selectable(false),
                                    );
                                };
                                match column {
                                    SaiopSort::Reference => {
                                        let (label, color) = if att.reference_raw.is_empty() {
                                            ("(sem referência)".to_owned(), p.muted)
                                        } else {
                                            (att.reference_raw.clone(), p.text)
                                        };
                                        child.add(
                                            egui::Label::new(
                                                RichText::new(label)
                                                    .font(theme::font_strong(theme::SIZE_BODY))
                                                    .color(color),
                                            )
                                            .truncate()
                                            .selectable(false),
                                        );
                                    }
                                    SaiopSort::Subject => {
                                        let r = child.add(
                                            egui::Label::new(
                                                RichText::new(att.subject.clone()).color(p.text),
                                            )
                                            .truncate()
                                            .selectable(false),
                                        );
                                        if !att.subject.is_empty() {
                                            r.on_hover_text(att.subject.clone());
                                        }
                                    }
                                    SaiopSort::Assignee => {
                                        if att.assignee.is_empty() {
                                            text(&mut child, "—".to_owned(), p.muted);
                                        } else {
                                            text(&mut child, att.assignee.clone(), p.text);
                                        }
                                    }
                                    SaiopSort::Entry => {
                                        if att.entry_date.is_empty() {
                                            text(&mut child, "—".to_owned(), p.muted);
                                        } else {
                                            text(&mut child, crate::labels::date(&att.entry_date), p.text);
                                        }
                                    }
                                    SaiopSort::Status => {
                                        child.horizontal(|ui| { widgets::pill(ui, status_label(&att.status), status_tone(&att.status)); });
                                    }
                                    SaiopSort::Due => {
                                        let (label, tone, hover) = due_pill(att, today);
                                        let r = child.horizontal(|ui| widgets::pill(ui, label, tone)).inner;
                                        if let Some(hover) = hover {
                                            r.on_hover_text(hover);
                                        }
                                    }
                                    SaiopSort::Age => {
                                        // Idade e espera calculadas por eventos (nunca pela fórmula DATEDIF cacheada)
                                        let age = att
                                            .age_days(today)
                                            .map(|d| format!("{d} d"))
                                            .unwrap_or_else(|| "—".into());
                                        let wait = att
                                            .external_wait_days(today)
                                            .map(|w| format!(" • {w} d fora"))
                                            .unwrap_or_default();
                                        let r = child.add(
                                            egui::Label::new(RichText::new(format!("{age}{wait}")).color(p.muted))
                                                .truncate()
                                                .selectable(false),
                                        );
                                        r.on_hover_text(TIME_HELP);
                                    }
                                    SaiopSort::Destination => {
                                        let t = if att.sent_to.is_empty() {
                                            crate::labels::date(&att.exit_date)
                                        } else {
                                            att.sent_to.clone()
                                        };
                                        text(&mut child, if t.is_empty() { "—".to_owned() } else { t }, p.muted);
                                    }
                                }
                            }
                            if response.clicked() {
                                if self.dirty() {
                                    self.set_notice(
                                        "Salve ou descarte as alterações na ficha antes de trocar de linha.",
                                        true,
                                    );
                                } else {
                                    self.load_into_editor(store, (*att).clone());
                                }
                            }
                        }
                        if filtered.is_empty() {
                            ui.spacing_mut().item_spacing = vec2(S2, S2);
                            ui.add_space(S4);
                            ui.horizontal_wrapped(|ui| {
                                ui.add_space(S4);
                                self.empty_state(ui, store, empty);
                            });
                            ui.add_space(S4);
                        }
                    });
            });
    }

    /// What the empty table says, and the way out of it.
    fn empty_state(&mut self, ui: &mut egui::Ui, store: &Store, empty: &EmptyCtx) {
        let only_search = !empty.query.is_empty() && empty.active.is_empty();
        if empty.total == 0 {
            widgets::muted(
                ui,
                "Ainda não há atendimentos. Registre uma nova entrada ou importe a planilha do controle.",
            );
        } else if let Some((id, number)) = empty.document.clone().filter(|_| empty.in_document == 0)
        {
            widgets::muted(ui, "Este documento não tem atendimentos.");
            if ui.add(widgets::quiet("Ver todos")).clicked() {
                self.clear_document_filter();
            }
            if !self.dirty()
                && ui
                    .add(widgets::quiet("Nova entrada"))
                    .on_hover_text("Registra um atendimento já ligado a este documento.")
                    .clicked()
            {
                self.new_entry(store, Some((id, number)));
            }
        } else if only_search && empty.document.is_none() {
            widgets::muted(
                ui,
                format!("Nenhum atendimento encontrado para “{}”.", empty.query),
            );
            if ui.add(widgets::quiet("Limpar a busca")).clicked() {
                self.query.clear();
            }
        } else {
            let mut active = Vec::new();
            if !empty.query.is_empty() {
                active.push(format!("busca “{}”", empty.query));
            }
            active.extend(empty.active.iter().cloned());
            if let Some((_, number)) = &empty.document {
                active.push(format!("só os de {number}"));
            }
            widgets::muted(ui, format!("Nenhum atendimento com {}.", active.join(", ")));
            if ui.add(widgets::quiet("Limpar filtros")).clicked() {
                self.clear_filters();
            }
            if empty.document.is_some() && ui.add(widgets::quiet("Ver todos")).clicked() {
                self.clear_document_filter();
            }
        }
    }

    /// "Importar planilha do controle": file, preview, commit, earlier imports.
    fn import_window(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        let p = c();
        let mut close_import = false;
        let mut import_open = self.import_open;
        egui::Window::new("Importar planilha do controle")
            .open(&mut import_open)
            .default_width(860.0)
            .resizable(true)
            .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = S2;
            widgets::small_muted(ui, "Escolha o arquivo .xlsx do controle do gabinete. A aba PROCESSOS é lida; nada é salvo antes de você confirmar.");
            widgets::overline(ui, "Arquivo", None);
            ui.horizontal(|ui| {
                if ui.add(widgets::secondary(format!("{} Escolher planilha…", icon::FOLDER))).clicked() {
                    if let Some(path) = crate::windows_ui::pick_file(
                        "Escolher a planilha do controle (aba PROCESSOS)",
                        &[("Planilhas Excel", "*.xlsx"), ("Todos os arquivos", "*.*")],
                    ) {
                        self.xlsx_path = path.display().to_string();
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.xlsx_path)
                        .hint_text("ou cole o caminho do arquivo .xlsx")
                        .desired_width(ui.available_width()),
                );
            });
            ui.checkbox(
                &mut self.tramitado_keep_open,
                "“Tramitado” com retorno da UAS fica aberto",
            )
            .on_hover_text("Quando a planilha diz “Tramitado” e “Retorno para UAS = Sim”, o atendimento entra como “Aguardando resposta” em vez de “Encerrado”.");
            let read_button = if self.import_preview.is_some() {
                widgets::secondary("Ler a planilha")
            } else {
                widgets::primary("Ler a planilha")
            };
            if ui.add(read_button).clicked() {
                let path = Path::new(self.xlsx_path.trim().trim_matches('"'));
                match store.preview_saiop_xlsx(path, self.tramitado_keep_open) {
                    Ok(prev) => {
                        self.set_notice(
                            format!(
                                "Prévia pronta: {} na aba {}. Confira abaixo antes de importar.",
                                crate::labels::count(prev.items.len(), "atendimento encontrado", "atendimentos encontrados"),
                                prev.sheet_name
                            ),
                            false,
                        );
                        self.import_preview = Some(prev);
                    }
                    Err(e) => self.set_notice(e.to_string(), true),
                }
            }

            if let Some(prev) = self.import_preview.clone() {
                ui.add_space(S1);
                ui.horizontal(|ui| {
                    widgets::overline(ui, "Prévia", None);
                    widgets::muted(ui, format!("{} (aba {})", prev.file_name, prev.sheet_name));
                });
                let new_rows = prev.new_rows();
                let repeated_rows = prev.items.len() - new_rows;
                ui.horizontal_wrapped(|ui| {
                    widgets::pill(ui, crate::labels::count(prev.items.len(), "linha lida", "linhas lidas"), Tone::Neutral);
                    widgets::pill(ui, crate::labels::count(new_rows, "nova", "novas"), Tone::Ok);
                    if repeated_rows > 0 {
                        widgets::pill(ui, crate::labels::count(repeated_rows, "já registrada", "já registradas"), Tone::Warn);
                    }
                    if prev.flagged_count > 0 {
                        widgets::pill(ui, crate::labels::count(prev.flagged_count, "com alerta para conferir", "com alerta para conferir"), Tone::Warn);
                    }
                    if prev.linked_documents > 0 {
                        widgets::pill(ui, format!("{} a documentos importados", crate::labels::count(prev.linked_documents, "ligada", "ligadas")), Tone::Info);
                    }
                    if prev.ignored_formula_rows > 0 {
                        widgets::small_muted(ui, crate::labels::count(prev.ignored_formula_rows, "linha vazia ignorada", "linhas vazias ignoradas"));
                    }
                });
                widgets::card_frame().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::both()
                        .id_salt("saiop-preview-rows")
                        .max_height(240.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            egui::Grid::new("saiop-preview-grid")
                                .min_col_width(60.0)
                                .spacing([S4, S2])
                                .show(ui, |ui| {
                                    for title in ["Linha", "Referência", "Assunto", "Responsável", "Entrada", "Situação", "Prazo", "Alertas", "No controle"] {
                                        ui.label(RichText::new(title).font(theme::font_strong(theme::SIZE_SMALL)).color(p.muted));
                                    }
                                    ui.end_row();
                                    for ((att, raw), existing) in prev.items.iter().zip(&prev.already_registered) {
                                        widgets::muted(ui, raw.row_number.to_string());
                                        ui.label(RichText::new(if att.reference_raw.is_empty() { "(sem referência)" } else { &att.reference_raw }).font(theme::font_strong(theme::SIZE_BODY)));
                                        let subject: String = att.subject.chars().take(40).collect();
                                        ui.label(subject).on_hover_text(&att.subject);
                                        ui.label(if att.assignee.is_empty() { "—" } else { &att.assignee });
                                        ui.label(crate::labels::date(&att.entry_date));
                                        widgets::pill(ui, status_label(&att.status), status_tone(&att.status));
                                        if att.due_date.is_empty() {
                                            if att.due_date_raw.is_empty() {
                                                widgets::muted(ui, "—");
                                            } else {
                                                widgets::pill(ui, format!("conferir: {}", att.due_date_raw), Tone::Warn);
                                            }
                                        } else {
                                            ui.label(crate::labels::date(&att.due_date));
                                        }
                                        if att.review_flags.is_empty() {
                                            widgets::muted(ui, "—");
                                        } else {
                                            widgets::pill(ui, format!("{} {}", icon::WARNING, att.review_flags.len()), Tone::Warn)
                                                .on_hover_text(att.review_flags.join("\n"));
                                        }
                                        match existing {
                                            Some(id) => { widgets::pill(ui, format!("já registrada (#{id})"), Tone::Warn); }
                                            None => { widgets::pill(ui, "nova", Tone::Ok); }
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                });
                ui.horizontal_wrapped(|ui| {
                    let mut to_commit: Option<SaiopImportPreview> = None;
                    if repeated_rows > 0 {
                        if ui
                            .add_enabled(new_rows > 0, widgets::primary(format!("Importar só as novas ({new_rows})")))
                            .clicked()
                        {
                            to_commit = Some(prev.only_new());
                        }
                        if ui
                            .add(widgets::secondary(format!("Importar todas ({}) e duplicar as já registradas", prev.items.len())))
                            .clicked()
                        {
                            to_commit = Some(prev.clone());
                        }
                    } else if ui.add(widgets::primary(format!("Importar {}", crate::labels::count(prev.items.len(), "atendimento", "atendimentos")))).clicked() {
                        to_commit = Some(prev.clone());
                    }
                    if ui.add(widgets::quiet("Cancelar")).clicked() {
                        self.import_preview = None;
                    }
                    if let Some(chosen) = to_commit {
                        match store.commit_saiop_import(&chosen) {
                            Ok(batch) => {
                                self.set_notice(
                                    format!(
                                        "Importação concluída: {} de {}.",
                                        crate::labels::count(batch.total_rows, "atendimento", "atendimentos"), batch.file_name
                                    ),
                                    false,
                                );
                                self.import_preview = None;
                                close_import = true;
                            }
                            Err(e) => self.set_notice(format!("Não foi possível importar: {e}"), true),
                        }
                    }
                });
            }

            if let Ok(batches) = store.list_saiop_batches() {
                if !batches.is_empty() {
                    ui.separator();
                    ui.collapsing(format!("Importações anteriores ({})", batches.len()), |ui| {
                    for b in batches {
                        ui.horizontal_wrapped(|ui| {
                            widgets::small_muted(ui, format!(
                                "{} • {} (aba {}) • {}, {} com alerta • por {}",
                                crate::labels::datetime(&b.imported_at),
                                b.file_name,
                                b.sheet_name,
                                crate::labels::count(b.total_rows, "linha", "linhas"),
                                b.flagged_rows,
                                b.actor
                            ));
                            let pending_here = self.pending_undo.as_ref().is_some_and(|(p, _, _)| p.id == b.id);
                            if !pending_here && ui.add(widgets::danger("Desfazer importação…")).clicked() {
                                match store.undo_saiop_batch_plan(b.id) {
                                    Ok((removed, kept)) => self.pending_undo = Some((b.clone(), removed, kept)),
                                    Err(e) => self.set_notice(format!("Não foi possível conferir a importação: {e}"), true),
                                }
                            }
                        });
                        if let Some((pending, removed, kept)) = self.pending_undo.clone() {
                            if pending.id == b.id {
                                let (fg, bg) = Tone::Warn.colors();
                                egui::Frame::new()
                                    .fill(bg)
                                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add(egui::Label::new(RichText::new(format!(
                                        "Desfazer a importação de {}? Atendimentos removidos: {}. Atendimentos mantidos, por terem sido editados ou terem tarefa: {}.",
                                        pending.file_name, removed, kept
                                    )).color(fg)).wrap());
                                    ui.horizontal(|ui| {
                                        if ui.add(widgets::danger("Desfazer importação")).clicked() {
                                            match store.undo_saiop_batch(pending.id) {
                                                Ok((removed, kept)) => self.set_notice(
                                                    format!(
                                                        "Importação desfeita. Atendimentos removidos: {removed}. Mantidos, por terem sido editados ou terem tarefa: {kept}."
                                                    ),
                                                    false,
                                                ),
                                                Err(e) => self.set_notice(format!("Não foi possível desfazer: {e}"), true),
                                            }
                                            self.pending_undo = None;
                                        }
                                        if ui.add(widgets::secondary("Cancelar")).clicked() {
                                            self.pending_undo = None;
                                        }
                                    });
                                });
                            }
                        }
                    }
                    });
                }
            }
        });
        self.import_open = import_open && !close_import;
    }

    /// The ficha of the atendimento being edited: a panel on the right with
    /// its buttons fixed at the bottom.
    fn show_ficha(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        all_items: &[SaiopAttendance],
        open_document: &mut Option<i64>,
    ) {
        let Some(mut ed) = self.editor.clone() else {
            return;
        };
        let p = c();
        let today = Local::now().date_naive();
        let mut save_clicked = false;
        let mut revert_clicked = false;
        let mut create_task_clicked = false;
        let mut close_clicked = false;
        let mut delete_confirmed = false;
        let is_dirty = self.dirty();
        let width = FICHA_WIDTH.min(ui.available_width() * 0.62);

        egui::Panel::right("saiop-ficha")
            .exact_size(width)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.surface))
            .show(ui, |ui| {
                // ---- Footer, fixed at the bottom ---------------------------------
                egui::Panel::bottom("saiop-ficha-foot")
                    .frame(
                        egui::Frame::new()
                            .fill(p.surface)
                            .inner_margin(egui::Margin::symmetric(16, 10)),
                    )
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S2;
                        if self.confirm_delete == Some(ed.id) && ed.id > 0 {
                            let (fg, bg) = Tone::Warn.colors();
                            egui::Frame::new()
                                .fill(bg)
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                                .inner_margin(egui::Margin::symmetric(12, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "Apagar de vez o atendimento {}? Ele não vai para a Lixeira e seus anexos também são apagados. As tarefas e o documento ligado permanecem.",
                                                if ed.reference_raw.is_empty() { "sem referência" } else { &ed.reference_raw }
                                            ))
                                            .color(fg),
                                        )
                                        .wrap(),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui.add(widgets::danger("Apagar de vez")).clicked() {
                                            delete_confirmed = true;
                                        }
                                        if ui.add(widgets::secondary("Cancelar")).clicked() {
                                            self.confirm_delete = None;
                                        }
                                    });
                                });
                        }
                        ui.horizontal(|ui| {
                            save_clicked = ui
                                .add_enabled(is_dirty || ed.id == 0, widgets::primary("Salvar"))
                                .clicked();
                            if ed.is_open() && ed.id > 0 {
                                if ui
                                    .add(widgets::secondary("Encerrar"))
                                    .on_hover_text("Marca como encerrado e salva.")
                                    .clicked()
                                {
                                    ed.status = "encerrado".into();
                                    ed.closed_at = Local::now().format("%Y-%m-%d").to_string();
                                    save_clicked = true;
                                }
                            } else if !ed.is_open()
                                && ui
                                    .add(widgets::secondary("Reabrir"))
                                    .on_hover_text("Volta para “Em análise” e salva.")
                                    .clicked()
                            {
                                ed.status = "em_analise".into();
                                ed.closed_at.clear();
                                save_clicked = true;
                            }
                            revert_clicked = ui
                                .add_enabled(is_dirty, widgets::quiet("Descartar alterações"))
                                .clicked();
                            if ed.id > 0 {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_enabled(!is_dirty, widgets::danger("Apagar de vez…"))
                                            .on_hover_text("Apaga o atendimento sem passar pela Lixeira. As tarefas criadas a partir dele continuam nos documentos.")
                                            .on_disabled_hover_text("Salve ou descarte as alterações antes de apagar.")
                                            .clicked()
                                        {
                                            self.confirm_delete = Some(ed.id);
                                        }
                                    },
                                );
                            }
                        });
                    });

                // ---- Header ------------------------------------------------------
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = S2;
                        ui.horizontal(|ui| {
                            let reserve = 40.0 + if is_dirty { 90.0 } else { 0.0 };
                            ui.scope(|ui| {
                                ui.set_max_width((ui.available_width() - reserve).max(80.0));
                                let title = if ed.id > 0 {
                                    if ed.reference_raw.is_empty() {
                                        "Atendimento sem referência".to_owned()
                                    } else {
                                        ed.reference_raw.clone()
                                    }
                                } else {
                                    "Novo atendimento".to_owned()
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(title).font(theme::font_strong(17.0)),
                                    )
                                    .truncate(),
                                );
                            });
                            if is_dirty {
                                widgets::pill(ui, "não salvo", Tone::Warn);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add_enabled_ui(!is_dirty, |ui| {
                                        widgets::icon_button(ui, icon::CLOSE, "Fechar ficha")
                                    })
                                    .inner
                                    .on_disabled_hover_text("Salve ou descarte as alterações antes de fechar.")
                                    .clicked()
                                {
                                    close_clicked = true;
                                }
                            });
                        });
                        if !ed.subject.trim().is_empty() {
                            ui.add(egui::Label::new(&ed.subject).wrap());
                        }
                        ui.horizontal_wrapped(|ui| {
                            widgets::pill(ui, status_label(&ed.status), status_tone(&ed.status));
                            let (label, tone, hover) = due_pill(&ed, today);
                            let r = widgets::pill(ui, label, tone);
                            if let Some(hover) = hover {
                                r.on_hover_text(hover);
                            }
                            if !ed.reference_norm.is_empty() {
                                let same_ref_count = all_items
                                    .iter()
                                    .filter(|a| a.reference_norm == ed.reference_norm)
                                    .count();
                                if same_ref_count > 1 {
                                    widgets::pill(
                                        ui,
                                        format!("{same_ref_count} passagens desta referência"),
                                        Tone::Violet,
                                    )
                                    .on_hover_text("A mesma referência entrou no gabinete mais de uma vez. Cada passagem é um atendimento.");
                                }
                            }
                            if let Some(doc_id) = ed.document_id {
                                if ui
                                    .add(widgets::quiet(format!("{} abrir o documento", icon::OPEN_OUT)))
                                    .on_hover_text("Abre o documento importado ligado a este atendimento.")
                                    .clicked()
                                {
                                    *open_document = Some(doc_id);
                                }
                            }
                        });
                    });

                // ---- Form --------------------------------------------------------
                #[allow(unused_mut)]
                let mut detail_area = egui::ScrollArea::vertical()
                    .id_salt("saiop-detail-scroll")
                    .auto_shrink([false, false]);
                #[cfg(feature = "shots")]
                if self.shots_scroll > 0.0 {
                    detail_area = detail_area.vertical_scroll_offset(self.shots_scroll);
                }
                detail_area
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(16, 0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = S2;
                                self.ficha_form(ui, store, &mut ed, &mut save_clicked, &mut create_task_clicked);
                                ui.add_space(S4);
                            });
                    });
            });

        self.editor = Some(ed.clone());

        if delete_confirmed {
            self.confirm_delete = None;
            match store.delete_saiop_attendance(ed.id) {
                Ok(()) => {
                    self.editor = None;
                    self.saved_snapshot = None;
                    self.selected_id = None;
                    self.set_notice(
                        format!(
                            "Atendimento {} apagado.",
                            if ed.reference_raw.is_empty() {
                                "sem referência"
                            } else {
                                &ed.reference_raw
                            }
                        ),
                        false,
                    );
                }
                Err(e) => {
                    self.set_notice(format!("Não foi possível apagar o atendimento: {e}"), true)
                }
            }
        } else if close_clicked {
            self.close_ficha();
        } else if revert_clicked {
            self.editor = self.saved_snapshot.clone();
        } else if create_task_clicked {
            let title = if self.new_task_title.trim().is_empty() {
                format!(
                    "Analisar atendimento {}",
                    if ed.reference_raw.is_empty() {
                        &ed.subject
                    } else {
                        &ed.reference_raw
                    }
                )
            } else {
                self.new_task_title.trim().to_owned()
            };
            // Unsaved edits of the ficha are saved first; they are never discarded.
            let saved_first = if self.dirty() {
                match store.save_saiop_attendance(&ed, self.base_version) {
                    Ok(_) => true,
                    Err(e) => {
                        self.set_notice(format!("Não foi possível salvar o atendimento antes de criar a tarefa: {e}"), true);
                        false
                    }
                }
            } else {
                true
            };
            if saved_first {
                match store.create_task_for_saiop(
                    ed.id,
                    &title,
                    &ed.priority,
                    &ed.due_date,
                    &ed.assignee,
                ) {
                    Ok(created) => {
                        self.new_task_title.clear();
                        if let Ok(updated_list) = store.list_saiop_attendances() {
                            if let Some(reloaded) = updated_list.into_iter().find(|a| a.id == ed.id)
                            {
                                self.load_into_editor(store, reloaded);
                                self.focus_ficha = false;
                            }
                        }
                        match created {
                            Some(_) => self.set_notice(
                                format!("Tarefa criada na área Tarefas: {title}"),
                                false,
                            ),
                            None => self.set_notice(
                                "Providência registrada na linha do tempo. Nenhuma tarefa foi criada porque este atendimento não está ligado a um documento importado.",
                                false,
                            ),
                        }
                    }
                    Err(e) => {
                        self.set_notice(format!("Não foi possível criar a tarefa: {e}"), true)
                    }
                }
            }
        } else if save_clicked {
            self.save_current(store);
        }
    }

    /// "Salvar": writes the ficha's atendimento and reloads it as saved.
    fn save_current(&mut self, store: &mut Store) {
        let Some(ed) = self.editor.clone() else {
            return;
        };
        match store.save_saiop_attendance(&ed, self.base_version) {
            Ok(saved_id) => {
                if let Ok(updated_list) = store.list_saiop_attendances() {
                    if let Some(reloaded) = updated_list.into_iter().find(|a| a.id == saved_id) {
                        self.load_into_editor(store, reloaded);
                        // The person is in the ficha already: no focus jump.
                        self.focus_ficha = false;
                    }
                }
                self.set_notice("Atendimento salvo.", false);
            }
            Err(e) => self.set_notice(format!("Não foi possível salvar o atendimento: {e}"), true),
        }
    }

    /// The fields of the ficha, in sections.
    fn ficha_form(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        ed: &mut SaiopAttendance,
        save_clicked: &mut bool,
        create_task_clicked: &mut bool,
    ) {
        let ed_id = ed.id;

        if !ed.review_flags.is_empty() {
            let (fg, bg) = Tone::Warn.colors();
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format!(
                            "{} Alertas de conferência ({})",
                            icon::WARNING,
                            ed.review_flags.len()
                        ))
                        .font(theme::font_strong(theme::SIZE_BODY))
                        .color(fg),
                    );
                    for flag in &ed.review_flags {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("• {flag}"))
                                    .size(theme::SIZE_SMALL)
                                    .color(fg),
                            )
                            .wrap(),
                        );
                    }
                    if ui
                        .add(widgets::secondary("Conferido: limpar alertas"))
                        .on_hover_text("Os alertas somem ao salvar o atendimento.")
                        .clicked()
                    {
                        ed.review_flags.clear();
                    }
                });
        }

        form_section(ui, "Situação e prazo", |ui| {
            ui.columns(2, |cols| {
                labeled(&mut cols[0], "Situação", |ui| {
                    pick(
                        ui,
                        "ed-saiop-status",
                        status_label(&ed.status).to_owned(),
                        |ui| {
                            for (k, label) in STATUS_OPTIONS {
                                ui.selectable_value(&mut ed.status, (*k).to_owned(), *label);
                            }
                        },
                    );
                });
                labeled(&mut cols[1], "Prioridade", |ui| {
                    pick(
                        ui,
                        "ed-saiop-priority",
                        priority_label(&ed.priority).to_owned(),
                        |ui| {
                            for (k, label) in PRIORITY_OPTIONS {
                                ui.selectable_value(&mut ed.priority, (*k).to_owned(), *label);
                            }
                        },
                    );
                });
            });
            labeled(ui, "Prazo", |ui| {
                crate::labels::date_field(ui, ("saiop-due", ed_id), &mut ed.due_date);
            });
            labeled(ui, "Tipo de prazo", |ui| {
                pick(
                    ui,
                    "ed-saiop-due-kind",
                    due_kind_label(&ed.due_kind).to_owned(),
                    |ui| {
                        for (k, label) in DUE_KIND_OPTIONS {
                            ui.selectable_value(&mut ed.due_kind, (*k).to_owned(), *label);
                        }
                    },
                );
            });
            labeled(ui, "Responsável", |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 130.0).max(80.0);
                    ui.add(egui::TextEdit::singleline(&mut ed.assignee).desired_width(width));
                    if ui.add(widgets::quiet("Atribuir a mim")).clicked() {
                        if let Ok(cfg) = store.load_intranet_config() {
                            ed.assignee = cfg.member_name;
                        }
                    }
                });
            });
            labeled(ui, "Motivo da prioridade", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut ed.priority_reason)
                        .desired_width(f32::INFINITY),
                );
            });
        });

        form_section(ui, "Identificação", |ui| {
            labeled(ui, "Referência", |ui| {
                if ed.document_id.is_some() {
                    ui.add_enabled(
                        false,
                        egui::TextEdit::singleline(&mut ed.reference_raw)
                            .desired_width(f32::INFINITY),
                    )
                    .on_disabled_hover_text(
                        "É o número do documento ligado. Para mudar, desligue do documento.",
                    );
                } else {
                    let field = ui
                        .add(egui::TextEdit::singleline(&mut ed.reference_raw).desired_width(f32::INFINITY))
                        .on_hover_text("Número do processo ou documento, ou um código provisório. Informe a referência ou o assunto.");
                    if std::mem::take(&mut self.focus_ficha) {
                        field.request_focus();
                    }
                }
            });
            // Link with an imported document
            if ed.document_id.is_some() {
                if ui.add(widgets::quiet("Desligar do documento")).clicked() {
                    ed.document_id = None;
                }
            } else {
                // Nobody knows a document's number by heart: the person
                // searches and picks.
                widgets::field_label(ui, "Ligar a um documento do acervo");
                if let Some(doc) =
                    self.link_picker
                        .show(ui, store, "Buscar por número, assunto ou texto")
                {
                    // The linked document's number becomes the reference.
                    ed.document_id = Some(doc.id);
                    ed.reference_norm = normalize_reference(&doc.document_number);
                    ed.reference_raw = doc.document_number;
                }
                widgets::small_muted(
                    ui,
                    "Opcional. Escolha o documento na lista; a referência passa a ser o número dele.",
                );
                if let Some(url) = ed.composed_sigadoc_url() {
                    ui.hyperlink_to("Abrir no SIGADOC", url).on_hover_text(
                        "Endereço montado a partir da referência; confira na página.",
                    );
                }
            }
            labeled(ui, "Assunto", |ui| {
                let field = ui
                    .add(egui::TextEdit::singleline(&mut ed.subject).desired_width(f32::INFINITY));
                // Linked to a document the reference is fixed: this is the
                // first field to type in.
                if ed.document_id.is_some() && std::mem::take(&mut self.focus_ficha) {
                    field.request_focus();
                }
            });
            labeled(ui, "Entrada", |ui| {
                crate::labels::date_field(ui, ("saiop-entry", ed_id), &mut ed.entry_date);
            });
            labeled(ui, "Recebido de", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut ed.received_from).desired_width(f32::INFINITY),
                );
            });
            labeled(ui, "Observações", |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut ed.observation)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
            });
        });

        fold(
            ui,
            "Condições e relações",
            ("saiop-fold-cond", ed_id),
            false,
            |ui| {
                ui.spacing_mut().item_spacing.y = S2;
                ui.columns(2, |cols| {
                    labeled(&mut cols[0], "Retorno para UAS?", |ui| {
                        pick(
                            ui,
                            "ed-saiop-uas",
                            tri_state_label(&ed.uas_return).to_owned(),
                            |ui| {
                                for (k, label) in TRI_STATE_OPTIONS {
                                    ui.selectable_value(
                                        &mut ed.uas_return,
                                        (*k).to_owned(),
                                        *label,
                                    );
                                }
                            },
                        );
                    });
                    labeled(&mut cols[1], "Assinado?", |ui| {
                        pick(
                            ui,
                            "ed-saiop-signed",
                            tri_state_label(&ed.signed_status).to_owned(),
                            |ui| {
                                for (k, label) in TRI_STATE_OPTIONS {
                                    ui.selectable_value(
                                        &mut ed.signed_status,
                                        (*k).to_owned(),
                                        *label,
                                    );
                                }
                            },
                        );
                    });
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut ed.scheduled, "Agendado");
                    ui.checkbox(&mut ed.sensitive, "Documento sensível");
                });
                labeled(ui, "Apensado / juntado", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut ed.attachment_relation)
                            .desired_width(f32::INFINITY),
                    )
                    .on_hover_text(
                        "Relação de apensamento ou juntada informada na planilha ou por você.",
                    );
                });
            },
        );

        fold(ui, "Resultado", ("saiop-fold-result", ed_id), false, |ui| {
            ui.spacing_mut().item_spacing.y = S2;
            labeled(ui, "Entrega prevista", |ui| {
                pick(
                    ui,
                    "ed-saiop-doctype",
                    if ed.planned_doc_type.is_empty() {
                        "Não definida".to_owned()
                    } else {
                        ed.planned_doc_type.clone()
                    },
                    |ui| {
                        for opt in DOC_TYPE_OPTIONS {
                            ui.selectable_value(
                                &mut ed.planned_doc_type,
                                (*opt).to_owned(),
                                if opt.is_empty() {
                                    "Não definida"
                                } else {
                                    *opt
                                },
                            );
                        }
                    },
                );
            });
            labeled(ui, "Resultado da etapa", |ui| {
                pick(
                    ui,
                    "ed-saiop-outcome",
                    outcome_label(&ed.outcome).to_owned(),
                    |ui| {
                        for (k, label) in OUTCOME_OPTIONS {
                            ui.selectable_value(&mut ed.outcome, (*k).to_owned(), *label);
                        }
                    },
                );
            });
            labeled(ui, "Data de saída", |ui| {
                crate::labels::date_field(ui, ("saiop-exit", ed_id), &mut ed.exit_date);
            });
            labeled(ui, "Enviado para", |ui| {
                ui.add(egui::TextEdit::singleline(&mut ed.sent_to).desired_width(f32::INFINITY));
            });
            if !ed.verification_raw.is_empty() {
                widgets::small_muted(
                    ui,
                    format!("Verificação na planilha: {}", ed.verification_raw),
                );
            }
        });

        // Tarefas criadas a partir deste atendimento. Tarefas pertencem
        // a documentos, por isso só há criação com documento ligado.
        if ed.id > 0 {
            let linked: Vec<Task> = if ed.task_ids.is_empty() {
                Vec::new()
            } else {
                store
                    .list_tasks()
                    .map(|tasks| {
                        tasks
                            .into_iter()
                            .filter(|t| ed.task_ids.contains(&t.id))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            fold(
                ui,
                &format!("Tarefas deste atendimento ({})", linked.len()),
                ("saiop-fold-tasks", ed_id),
                !linked.is_empty(),
                |ui| {
                    if linked.is_empty() {
                        widgets::small_muted(
                            ui,
                            "Nenhuma tarefa criada a partir deste atendimento.",
                        );
                    }
                    for task in linked {
                        ui.horizontal_wrapped(|ui| {
                            let done = task.completed_at.is_some();
                            if done {
                                ui.label(RichText::new(icon::CHECK).color(c().ok));
                            } else {
                                ui.label(RichText::new(icon::CLOCK).color(c().muted));
                            }
                            ui.label(
                                RichText::new(&task.title)
                                    .font(theme::font_strong(theme::SIZE_BODY)),
                            );
                            widgets::small_muted(ui, crate::labels::task_status(&task.status));
                            if !task.due_date.is_empty() {
                                widgets::small_muted(
                                    ui,
                                    format!("prazo {}", crate::labels::date(&task.due_date)),
                                );
                            }
                            if ui
                                .add(widgets::quiet("Abrir"))
                                .on_hover_text("Abre a tarefa na área Tarefas.")
                                .clicked()
                            {
                                self.open_task = Some(task.clone());
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let can_create = ed.document_id.is_some();
                        let width = (ui.available_width() - 120.0).max(80.0);
                        ui.add_enabled(
                            can_create,
                            egui::TextEdit::singleline(&mut self.new_task_title)
                                .hint_text("Título da nova tarefa (opcional)…")
                                .desired_width(width),
                        );
                        if ui
                            .add_enabled(can_create, widgets::secondary("Criar tarefa"))
                            .on_hover_text("Cria, na área Tarefas, uma tarefa do documento ligado com a prioridade, o prazo e o responsável deste atendimento. Sem título, ela se chama “Analisar atendimento …”.")
                            .on_disabled_hover_text("Tarefas pertencem a documentos: ligue o atendimento a um documento importado (acima) para criar uma tarefa.")
                            .clicked()
                        {
                            *create_task_clicked = true;
                        }
                    });
                },
            );

            // Uses of rooms, books and equipment tied to this atendimento.
            let link = crate::resources_ui::UseLink::Attendance(ed.id);
            let uses = crate::resources_ui::linked_uses(store, &link);
            fold(
                ui,
                &format!("Recursos usados ({})", uses.len()),
                ("saiop-fold-resources", ed_id),
                !uses.is_empty(),
                |ui| {
                    if uses.is_empty() {
                        widgets::small_muted(ui, "Nenhum recurso usado neste atendimento.");
                    }
                    for item in &uses {
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(widgets::quiet("Abrir"))
                                        .on_hover_text("Abre o uso na área Recursos.")
                                        .clicked()
                                    {
                                        self.use_request = Some(
                                            crate::resources_ui::UseRequest::Open(item.uid.clone()),
                                        );
                                    }
                                    widgets::pill(ui, item.label.clone(), item.tone);
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(egui::Label::new(item.text.as_str()).wrap());
                                        },
                                    );
                                },
                            );
                        });
                    }
                    if ui
                        .add(widgets::secondary("Reservar recurso…"))
                        .on_hover_text("Reserva ou empresta uma sala, um livro ou um equipamento para este atendimento.")
                        .clicked()
                    {
                        self.use_request = Some(crate::resources_ui::UseRequest::Reserve(link));
                    }
                },
            );
        }

        fold(ui, "Anexos", ("saiop-fold-attach", ed_id), false, |ui| {
            if ed.id > 0 {
                self.attachments.show(
                    ui,
                    store,
                    crate::attachments::Owner::Attendance(ed.id),
                    false,
                );
            } else {
                widgets::small_muted(ui, "Salve o atendimento para poder anexar arquivos a ele.");
            }
        });

        // Histórico de eventos e registro original da planilha
        if ed.id > 0 {
            fold(
                ui,
                "Linha do tempo",
                ("saiop-fold-events", ed_id),
                false,
                |ui| {
                    ui.horizontal(|ui| {
                    let width = (ui.available_width() - 150.0).max(80.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_event_note)
                            .hint_text("Registrar evento datado (ex.: cobrado retorno por telefone)…")
                            .desired_width(width),
                    );
                    if ui
                        .add(widgets::secondary("Adicionar evento"))
                        .on_hover_text("Registra o evento e salva o atendimento, inclusive outras alterações desta ficha.")
                        .clicked()
                        && !self.new_event_note.trim().is_empty()
                    {
                        ed.events.push(SaiopEvent {
                            when: Utc::now().to_rfc3339(),
                            actor: store.active_actor(),
                            kind: "anotacao".into(),
                            detail: self.new_event_note.trim().to_owned(),
                        });
                        self.new_event_note.clear();
                        *save_clicked = true;
                    }
                });
                    for ev in ed.events.iter().rev() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "• {} • {} • {}: {}",
                                    crate::labels::datetime(&ev.when),
                                    ev.actor,
                                    crate::labels::saiop_event_kind(&ev.kind),
                                    ev.detail
                                ))
                                .size(theme::SIZE_SMALL)
                                .color(c().muted),
                            )
                            .wrap(),
                        );
                    }
                },
            );
            if let Ok(Some(raw_row)) = store.load_saiop_raw_row(ed.id) {
                fold(
                    ui,
                    &format!(
                        "Linha original da planilha (aba {}, linha {})",
                        raw_row.sheet_name, raw_row.row_number
                    ),
                    ("saiop-fold-raw", ed_id),
                    false,
                    |ui| {
                        egui::Grid::new(("saiop-raw-row", ed.id))
                            .num_columns(2)
                            .spacing([S3, S1])
                            .show(ui, |ui| {
                                for (column, value) in &raw_row.cells {
                                    if value.trim().is_empty() {
                                        continue;
                                    }
                                    widgets::small_muted(
                                        ui,
                                        format!(
                                            "{} ({column})",
                                            crate::labels::spreadsheet_column(column)
                                        ),
                                    );
                                    ui.add(egui::Label::new(value).wrap());
                                    ui.end_row();
                                }
                                for (column, formula) in &raw_row.formulas {
                                    widgets::small_muted(ui, format!("Fórmula em {column}"));
                                    ui.add(egui::Label::new(formula).wrap());
                                    ui.end_row();
                                }
                            });
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn build_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for &(name, data) in entries {
            let local_offset = out.len() as u32;
            let name_bytes = name.as_bytes();
            let size = data.len() as u32;
            // Local file header
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // method 0 = Stored
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // crc
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(data);

            // Central directory header
            cd.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // method 0
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&size.to_le_bytes());
            cd.extend_from_slice(&size.to_le_bytes());
            cd.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&local_offset.to_le_bytes());
            cd.extend_from_slice(name_bytes);
        }
        let cd_offset = out.len() as u32;
        let cd_size = cd.len() as u32;
        out.extend_from_slice(&cd);
        // EOCD
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn imports_processos_sheet_preserving_reentries_unknown_signed_flags_and_supports_rollback() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();

        let workbook_xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
                <sheets>
                    <sheet name="CADASTROS" sheetId="1" r:id="rId1"/>
                    <sheet name="PROCESSOS" sheetId="2" r:id="rId2"/>
                </sheets>
            </workbook>"#;
        let rels_xml = br#"<?xml version="1.0" encoding="UTF-8"?>
            <Relationships>
                <Relationship Id="rId1" Target="worksheets/sheet1.xml"/>
                <Relationship Id="rId2" Target="worksheets/sheet2.xml"/>
            </Relationships>"#;
        let shared_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <sst>
                <si><t>SES-PRO-2026/00100</t></si>
                <si><t>SERVIDOR-B</t></si>
                <si><t>SERVIDOR-A</t></si>
                <si><t>Tramitado</t></si>
                <si><t>Sim</t></si>
                <si><t>Aquisição de insumos hospitalares</t></si>
                <si><t>PRAZO DIA 18</t></si>
                <si><t>Urgente</t></si>
            </sst>"#
            .as_bytes();
        // Row 2: First entry of SES-PRO-2026/00100 (2026-01-10 = serial 46032), Tramitado + Retorno UAS = Sim, empty Assinado (must stay nao_informado!)
        // Row 3: Re-entry of SES-PRO-2026/00100 on a later date (2026-02-15 = serial 46068), textual deadline "PRAZO DIA 18", Assinado = Sim
        // Row 4: Prepared formula-only row (must be ignored!)
        let sheet2_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <worksheet>
                <sheetData>
                    <row r="1">
                        <c r="B1" t="inlineStr"><is><t>Data</t></is></c>
                        <c r="C1" t="inlineStr"><is><t>Responsável</t></is></c>
                        <c r="D1" t="inlineStr"><is><t>Nº processo SIGADOC</t></is></c>
                    </row>
                    <row r="2">
                        <c r="B2"><v>46032</v></c>
                        <c r="C2" t="s"><v>1</v></c>
                        <c r="D2" t="s"><v>0</v></c>
                        <c r="H2"><f>DATEDIF(B2,TODAY(),"d")</f><v>999</v></c>
                        <c r="J2" t="s"><v>4</v></c>
                        <c r="M2" t="s"><v>3</v></c>
                        <c r="R2" t="s"><v>5</v></c>
                    </row>
                    <row r="3">
                        <c r="B3"><v>46068</v></c>
                        <c r="C3" t="s"><v>2</v></c>
                        <c r="D3" t="s"><v>0</v></c>
                        <c r="K3" t="s"><v>6</v></c>
                        <c r="M3" t="s"><v>7</v></c>
                        <c r="N3" t="s"><v>4</v></c>
                        <c r="R3" t="s"><v>5</v></c>
                    </row>
                    <row r="4">
                        <c r="E4"><f>HYPERLINK(F4,D4)</f></c>
                        <c r="H4"><f>DATEDIF(B4,TODAY(),"d")</f></c>
                    </row>
                </sheetData>
            </worksheet>"#
            .as_bytes();

        let xlsx = build_stored_zip(&[
            ("xl/workbook.xml", workbook_xml),
            ("xl/_rels/workbook.xml.rels", rels_xml),
            ("xl/sharedStrings.xml", shared_xml),
            ("xl/worksheets/sheet2.xml", sheet2_xml),
        ]);

        let preview = store
            .preview_saiop_xlsx_bytes("Controle_SAIOP.xlsx", &xlsx, true)
            .unwrap();
        assert_eq!(preview.sheet_name, "PROCESSOS");
        assert_eq!(preview.items.len(), 2);
        assert_eq!(preview.ignored_formula_rows, 1);
        assert_eq!(preview.repeated_references, 2);

        // Row 2: empty Assinado? -> nao_informado (NEVER "nao"); Tramitado + UAS return -> aguardando_resposta
        let (first, raw1) = &preview.items[0];
        assert_eq!(first.reference_norm, "SES-PRO-2026/00100");
        assert_eq!(first.entry_date, "2026-01-10");
        assert_eq!(first.signed_status, "nao_informado");
        assert_eq!(first.uas_return, "sim");
        assert_eq!(first.status, "aguardando_resposta");
        assert_eq!(first.outcome, "encaminhado");
        assert_eq!(
            raw1.formulas.get("H").map(String::as_str),
            Some("DATEDIF(B2,TODAY(),\"d\")")
        );

        // Row 3: re-entry preserved separately, textual deadline flagged without losing original string
        let (second, _) = &preview.items[1];
        assert_eq!(second.reference_norm, "SES-PRO-2026/00100");
        assert_eq!(second.entry_date, "2026-02-15");
        assert_eq!(second.due_date_raw, "PRAZO DIA 18");
        assert_eq!(second.due_kind, "a_confirmar");
        assert_eq!(second.signed_status, "sim");
        assert_eq!(second.priority, "urgente");

        assert_eq!(preview.already_registered, vec![None, None]);
        assert_eq!(preview.new_rows(), 2);
        let batch = store.commit_saiop_import(&preview).unwrap();
        assert_eq!(batch.total_rows, 2);

        // Re-importing the exact same file is blocked idempotently
        assert!(store.commit_saiop_import(&preview).is_err());

        // An updated copy of the spreadsheet (different bytes, same rows) is
        // recognized row by row, and "only new" imports nothing.
        let updated = build_stored_zip(&[
            ("xl/workbook.xml", workbook_xml),
            ("xl/_rels/workbook.xml.rels", rels_xml),
            ("xl/sharedStrings.xml", shared_xml),
            ("xl/worksheets/sheet2.xml", sheet2_xml),
            ("docProps/app.xml", b"<Properties/>"),
        ]);
        let again = store
            .preview_saiop_xlsx_bytes("Controle_SAIOP_v2.xlsx", &updated, true)
            .unwrap();
        assert_ne!(again.file_sha256, preview.file_sha256);
        assert!(again.already_registered.iter().all(Option::is_some));
        assert_eq!(again.new_rows(), 0);
        assert!(again.only_new().items.is_empty());
        assert_eq!(store.undo_saiop_batch_plan(batch.id).unwrap(), (2, 0));

        // Edit one of the two imported attendances manually, then rollback the batch:
        // the edited one must be preserved while the unedited one is removed!
        let mut saved = store.list_saiop_attendances().unwrap();
        assert_eq!(saved.len(), 2);
        let mut to_edit = saved.remove(0);
        to_edit.observation = "Revisado manualmente pelo gabinete".into();
        let ver = store.item_version("saiop", to_edit.id).unwrap().version;
        store.save_saiop_attendance(&to_edit, Some(ver)).unwrap();

        let (removed, preserved) = store.undo_saiop_batch(batch.id).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(preserved, 1);
        let remaining = store.list_saiop_attendances().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].observation,
            "Revisado manualmente pelo gabinete"
        );
    }

    #[test]
    fn importing_a_sigadoc_document_records_its_attendance_once() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        assert!(store.auto_attendance_enabled().unwrap(), "on by default");
        let capture = crate::model::sample_capture();
        let receipt = store.import_capture(&capture).unwrap();
        let list = store.list_saiop_attendances().unwrap();
        assert_eq!(list.len(), 1);
        let att = &list[0];
        assert_eq!(att.document_id, Some(receipt.document_id));
        assert_eq!(att.reference_raw, capture.document_number);
        assert_eq!(att.subject, capture.title.trim());
        assert_eq!(att.status, "em_analise");
        assert!(
            att.events
                .iter()
                .any(|e| e.detail.contains("automaticamente")),
            "{:?}",
            att.events
        );

        // A new version of the same document adds nothing.
        let mut again = capture.clone();
        again.request_id = uuid::Uuid::new_v4().to_string();
        again.text.push_str("\nnova versão");
        store.import_capture(&again).unwrap();
        assert_eq!(store.list_saiop_attendances().unwrap().len(), 1);

        // An atendimento that already carries the number (from the
        // spreadsheet) is linked instead of duplicated.
        let mut manual = SaiopAttendance::new_manual();
        manual.reference_raw = "SEFAZ-PRO-2026/00123".into();
        manual.reference_norm = normalize_reference(&manual.reference_raw);
        manual.subject = "Da planilha".into();
        let manual_id = store.save_saiop_attendance(&manual, None).unwrap();
        let mut other = crate::model::sample_capture();
        other.document_number = "SEFAZ-PRO-2026/00123".into();
        other.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00123"
                .into();
        other.request_id = uuid::Uuid::new_v4().to_string();
        let receipt = store.import_capture(&other).unwrap();
        let list = store.list_saiop_attendances().unwrap();
        assert_eq!(list.len(), 2);
        let linked = list.iter().find(|a| a.id == manual_id).unwrap();
        assert_eq!(linked.document_id, Some(receipt.document_id));
        assert_eq!(linked.subject, "Da planilha", "the person's entry is kept");

        // Switched off: nothing is created.
        store.set_auto_attendance(false).unwrap();
        let mut third = crate::model::sample_capture();
        third.document_number = "SEFAZ-PRO-2026/00124".into();
        third.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00124"
                .into();
        third.request_id = uuid::Uuid::new_v4().to_string();
        store.import_capture(&third).unwrap();
        assert_eq!(store.list_saiop_attendances().unwrap().len(), 2);
    }

    #[test]
    fn inflating_never_produces_more_than_the_declared_size() {
        // One final stored block of 8 bytes.
        let data = b"PLANILHA";
        let mut stream = vec![0x01, 8, 0, !8u8, 0xff];
        stream.extend_from_slice(data);
        assert_eq!(inflate_raw(&stream, 8).unwrap(), data);
        let error = inflate_raw(&stream, 4).unwrap_err().to_string();
        assert!(error.contains("maior do que declara"), "{error}");
    }

    #[test]
    fn deleting_an_attendance_removes_it_with_its_attachments() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let mut att = SaiopAttendance::new_manual();
        att.subject = "Pedido por telefone".into();
        let id = store.save_saiop_attendance(&att, None).unwrap();
        let owner = crate::attachments::Owner::Attendance(id);
        store
            .add_attachment(owner, "nota.txt", "anotação", b"texto simples")
            .unwrap();
        assert_eq!(store.list_attachments(owner).unwrap().len(), 1);
        assert_eq!(store.attachment_counts("saiop").unwrap()[&id], 1);

        // A single attachment can be removed for good (no trash for these).
        let second = store
            .add_attachment(owner, "foto.png", "", b"\x89PNG\r\n\x1a\n....")
            .unwrap();
        assert_eq!(store.list_attachments(owner).unwrap().len(), 2);
        assert!(
            store
                .trash_item(crate::trash::Target {
                    kind: crate::trash::Kind::Attachment,
                    id: second
                })
                .is_err(),
            "attachments of atendimentos do not go to the trash"
        );
        // An atendimento without a document has no document id to log.
        store
            .update_attachment_description(second, "foto do ofício")
            .unwrap();
        assert_eq!(
            store.list_attachments(owner).unwrap()[1].description,
            "foto do ofício"
        );
        store.delete_attachment(second).unwrap();
        assert_eq!(store.list_attachments(owner).unwrap().len(), 1);

        store.delete_saiop_attendance(id).unwrap();
        assert!(store.list_saiop_attendances().unwrap().is_empty());
        assert!(store.attachment_counts("saiop").unwrap().is_empty());
        assert!(
            store.delete_saiop_attendance(id).is_err(),
            "second delete reports it is gone"
        );
    }
    #[test]
    fn a_task_created_from_an_atendimento_is_written_with_its_record_or_not_at_all() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let att = store.list_saiop_attendances().unwrap().remove(0);
        assert_eq!(att.document_id, Some(receipt.document_id));
        let tasks_before = store.list_tasks().unwrap().len();

        let task_id = store
            .create_task_for_saiop(att.id, "Responder ofício", "alta", "", "Ana")
            .unwrap()
            .expect("a document is linked");
        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), tasks_before + 1);
        let task = tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.document_id, Some(receipt.document_id));
        assert_eq!(task.priority, "alta");
        let att = store
            .list_saiop_attendances()
            .unwrap()
            .into_iter()
            .find(|a| a.id == att.id)
            .unwrap();
        assert_eq!(att.task_ids, vec![task_id]);
        assert!(att
            .events
            .iter()
            .any(|e| e.kind == "tarefa" && e.detail.contains("Responder ofício")));

        // The atendimento changed meanwhile (stale version): neither the
        // task nor the record is written.
        let stale = store.item_version("saiop", att.id).unwrap().version - 1;
        let mut task = task.clone();
        task.id = 0;
        task.title = "Não deve ficar".into();
        assert!(store
            .write_task_for_saiop(att.id, Some(&task), "Não deve ficar", Some(stale))
            .is_err());
        assert_eq!(store.list_tasks().unwrap().len(), tasks_before + 1);
        assert!(!store
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.title == "Não deve ficar"));
    }
    #[test]
    fn a_failed_automatic_attendance_is_kept_for_the_person_to_retry() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER block_att BEFORE INSERT ON saiop_attendances
                 BEGIN SELECT RAISE(ABORT, 'disco cheio'); END;",
            )
            .unwrap();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        assert!(store.list_saiop_attendances().unwrap().is_empty());
        let failures = store.auto_attendance_failures().unwrap();
        assert_eq!(failures.len(), 1, "the import itself went through");
        assert_eq!(failures[0].document_id, receipt.document_id);
        assert!(failures[0].message.contains("disco cheio"), "{failures:?}");

        assert!(store.retry_auto_attendance(receipt.document_id).is_err());
        assert_eq!(store.auto_attendance_failures().unwrap().len(), 1);
        store.conn.execute_batch("DROP TRIGGER block_att;").unwrap();
        store.retry_auto_attendance(receipt.document_id).unwrap();
        assert!(store.auto_attendance_failures().unwrap().is_empty());
        assert_eq!(store.list_saiop_attendances().unwrap().len(), 1);

        // Dismissing forgets the notice without creating anything.
        store
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER block_att BEFORE INSERT ON saiop_attendances
                 BEGIN SELECT RAISE(ABORT, 'disco cheio'); END;",
            )
            .unwrap();
        let mut other = crate::model::sample_capture();
        other.document_number = "SEFAZ-PRO-2026/00555".into();
        other.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00555"
                .into();
        other.request_id = uuid::Uuid::new_v4().to_string();
        let second = store.import_capture(&other).unwrap();
        assert_eq!(store.auto_attendance_failures().unwrap().len(), 1);
        store
            .dismiss_auto_attendance_failure(second.document_id)
            .unwrap();
        assert!(store.auto_attendance_failures().unwrap().is_empty());
    }
}

#[cfg(all(test, feature = "desktop"))]
mod panel_tests {
    use super::*;

    fn shown(width: f32) -> Vec<SaiopSort> {
        visible_columns(width).into_iter().map(|(c, _)| c).collect()
    }

    #[test]
    fn opens_unfiltered_even_when_a_context_document_is_set() {
        let mut panel = SaiopPanel::default();
        panel.set_context_document(Some((7, "ORG-OFI-2026/00431".to_owned())));
        assert_eq!(panel.document_filter, None);
        panel.set_context_document(Some((8, "ORG-OFI-2026/00398".to_owned())));
        assert_eq!(panel.document_filter, None);
        panel.document_filter = Some(8);
        panel.clear_document_filter();
        assert_eq!(panel.document_filter, None);
    }

    #[test]
    fn columns_follow_the_width_in_priority_order() {
        use SaiopSort::*;
        // Narrow: only the four that always show.
        assert_eq!(shown(100.0), vec![Reference, Subject, Status, Due]);
        assert_eq!(shown(700.0), vec![Reference, Subject, Status, Due]);
        // Room for Responsável, then Entrada, then Tempo, then Destino.
        assert_eq!(
            shown(800.0),
            vec![Reference, Subject, Assignee, Status, Due]
        );
        assert_eq!(
            shown(900.0),
            vec![Reference, Subject, Assignee, Entry, Status, Due]
        );
        assert_eq!(
            shown(1000.0),
            vec![Reference, Subject, Assignee, Entry, Status, Due, Age]
        );
        assert_eq!(
            shown(1400.0),
            vec![
                Reference,
                Subject,
                Assignee,
                Entry,
                Status,
                Due,
                Age,
                Destination
            ]
        );
        // The fixed widths never exceed a width that holds them all.
        let total: f32 = visible_columns(1400.0).iter().map(|(_, w)| w).sum();
        assert!(total <= 1400.0);
    }

    #[test]
    fn escape_does_not_close_an_unsaved_ficha() {
        let mut panel = SaiopPanel::default();
        assert!(!panel.key_escape());
        let att = SaiopAttendance::new_manual();
        panel.saved_snapshot = Some(att.clone());
        panel.editor = Some(att);
        // Open and clean: closes.
        assert!(panel.key_escape());
        assert!(panel.editor.is_none());
        // Open and edited: stays, and says something is open.
        let att = SaiopAttendance::new_manual();
        panel.saved_snapshot = Some(att.clone());
        let mut edited = att;
        edited.subject = "Assunto alterado".to_owned();
        panel.editor = Some(edited);
        assert!(panel.dirty());
        assert!(panel.key_escape());
        assert!(panel.editor.is_some());
        assert!(panel.dirty());
    }
}

/// Development only: the views of this screen pictured by `--shots`, by
/// name. The tour calls `shots_open` with each one on a fresh panel.
#[cfg(feature = "shots")]
pub const SHOTS: &[&str] = &[
    "lista",
    "aberto",
    "filtros",
    "importar",
    "filtrada",
    "vazia-busca",
    "ligar",
    "estreita",
    "com-recursos",
];

#[cfg(feature = "shots")]
impl SaiopPanel {
    /// Puts the panel in the state called `view` (see `SHOTS`).
    pub fn shots_open(&mut self, store: &mut crate::storage::Store, view: &str) {
        match view {
            "aberto" => {
                if let Ok(list) = store.list_saiop_attendances() {
                    if let Some(first) = list.into_iter().next() {
                        self.load_into_editor(store, first);
                    }
                }
            }
            "com-recursos" => {
                // The atendimento that has uses of resources, scrolled down
                // to the "Recursos usados" block.
                let linked: std::collections::BTreeSet<i64> = store
                    .list_resource_uses()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|u| u.saiop_id)
                    .collect();
                if let Ok(list) = store.list_saiop_attendances() {
                    if let Some(att) = list.into_iter().find(|a| linked.contains(&a.id)) {
                        self.load_into_editor(store, att);
                    }
                }
                self.shots_scroll = 700.0;
            }
            "filtros" => self.filters_open = true,
            "importar" => self.import_open = true,
            "filtrada" => {
                if let Ok(docs) = store.list_documents("") {
                    if let Some(doc) = docs
                        .into_iter()
                        .find(|d| d.document_number == "ORG-OFI-2026/00431")
                    {
                        self.context_document = Some((doc.id, doc.document_number));
                        self.document_filter = Some(doc.id);
                    }
                }
            }
            "vazia-busca" => self.query = "parecer jurídico".to_owned(),
            "ligar" => {
                // An atendimento shown as not linked yet, with the search already typed.
                if let Ok(list) = store.list_saiop_attendances() {
                    if let Some(att) = list.into_iter().next() {
                        self.load_into_editor(store, att);
                    }
                }
                if let Some(editor) = self.editor.as_mut() {
                    editor.document_id = None;
                }
                self.link_picker.shots_query(store, "veículos");
            }
            "estreita" => {
                // The ficha takes room from the table; the sort is on a
                // column that does not fit, so "Entrada" is marked.
                self.sort = SaiopSort::Destination;
                self.sort_ascending = true;
                if let Ok(list) = store.list_saiop_attendances() {
                    if let Some(first) = list.into_iter().next() {
                        self.load_into_editor(store, first);
                    }
                }
            }
            _ => {}
        }
    }
}
