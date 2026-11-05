//! Área operacional "Atendimentos" (Controle do gabinete do SAIOP) e importador `.xlsx` da aba `PROCESSOS`.
//! Preserva atendimentos/passagens pelo gabinete independentes de captura, quatro dimensões de situação/prioridade,
//! indicadores calculados por eventos (sem depender de `DATEDIF` da planilha), rastreabilidade de lote e reversão.

use crate::{
    protection::{seal, unseal},
    storage::Store,
    tasks::Task,
};
use anyhow::{bail, ensure, Context, Result};
use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Utc};
use eframe::egui::{self, Color32, RichText};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const ACCENT: Color32 = Color32::from_rgb(24, 84, 97);
const WARNING: Color32 = Color32::from_rgb(153, 88, 16);
const ERROR: Color32 = Color32::from_rgb(168, 40, 45);

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
    #[serde(default)]
    pub events: Vec<SaiopEvent>,
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
            events: Vec::new(),
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
        self.already_registered.iter().filter(|r| r.is_none()).count()
    }
    /// Copy of the preview keeping only the rows not yet in the control.
    pub fn only_new(&self) -> Self {
        let keep: Vec<bool> = self.already_registered.iter().map(|r| r.is_none()).collect();
        let items: Vec<_> = self
            .items
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(item, _)| item.clone())
            .collect();
        Self {
            flagged_count: items.iter().filter(|(a, _)| !a.review_flags.is_empty()).count(),
            linked_documents: items.iter().filter(|(a, _)| a.document_id.is_some()).count(),
            already_registered: vec![None; items.len()],
            items,
            ..self.clone()
        }
    }
}

pub fn normalize_reference(raw: &str) -> String {
    raw.trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
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
            CREATE INDEX IF NOT EXISTS idx_saiop_attendances_batch ON saiop_attendances(batch_id);",
        )?;
        Ok(())
    }

    pub fn list_saiop_attendances(&self) -> Result<Vec<SaiopAttendance>> {
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
                .context("Registro de atendimento SAIOP inválido")?;
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
            let mut batch: SaiopImportBatch = serde_json::from_slice(&unseal(&blob)?)
                .context("Registro de lote SAIOP inválido")?;
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

    pub fn save_saiop_attendance(
        &mut self,
        attendance: &SaiopAttendance,
        expected_version: Option<i64>,
    ) -> Result<i64> {
        ensure!(
            !attendance.reference_raw.trim().is_empty() || !attendance.subject.trim().is_empty(),
            "Informe a referência (processo/documento/código provisório) ou o assunto do atendimento."
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
        if let Some(cfg) = self.client_config()? {
            return match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SaveSaiopAttendance {
                    attendance: attendance.clone(),
                    expected_version,
                },
            )? {
                crate::intranet::MutationReply::Id(id) => Ok(id),
                _ => bail!("Resposta inesperada ao salvar atendimento no anfitrião."),
            };
        }

        let actor = self.active_actor();
        let now = Utc::now().to_rfc3339();
        let today = Local::now().format("%Y-%m-%d").to_string();

        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let mut next = attendance.clone();
        next.reference_raw = next.reference_raw.trim().to_owned();
        next.reference_norm = normalize_reference(&next.reference_raw);
        next.subject = next.subject.trim().to_owned();
        next.assignee = next.assignee.trim().to_owned();
        next.updated_at = now.clone();
        next.human_edited = true;

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
                    next.reference_norm,
                    encoded,
                    now,
                    next.id
                ],
            )?;
            crate::intranet::record_mutation(
                &tx,
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
            tx.commit()?;
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
                    next.reference_norm,
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
                &tx,
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
            tx.commit()?;
            Ok(id)
        }
    }

    /// Prepara a prévia de importação da aba `PROCESSOS` de um arquivo `.xlsx` sem gravar no banco.
    pub fn preview_saiop_xlsx(
        &self,
        path: &Path,
        tramitado_keep_open: bool,
    ) -> Result<SaiopImportPreview> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Não foi possível ler a planilha em {}", path.display()))?;
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
            let ref_norm = normalize_reference(row.cells.get("D").map(String::as_str).unwrap_or(""));
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
                col_b, col_c, col_d, col_i, col_j, col_k, col_l, col_m, col_n, col_o, col_p,
                col_q, col_r,
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
                events: vec![SaiopEvent {
                    when: now.clone(),
                    actor: self.active_actor(),
                    kind: "importacao_planilha".into(),
                    detail: format!(
                        "Importado da aba {} (linha {}) do arquivo {}",
                        sheet.sheet_name, raw.row_number, file_name
                    ),
                }],
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

    pub fn commit_saiop_import(&mut self, preview: &SaiopImportPreview) -> Result<SaiopImportBatch> {
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
                _ => bail!("Resposta inesperada ao importar lote no anfitrião."),
            };
        }
        self.commit_saiop_import_raw(
            &preview.file_name,
            &preview.file_sha256,
            &preview.sheet_name,
            &preview.items,
        )
    }

    pub fn commit_saiop_import_raw(
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
                "Este arquivo (.xlsx) já foi importado no lote #{existing_id} (mesmo hash SHA-256). A reimportação idêntica foi bloqueada para evitar duplicidade."
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
                    record.reference_norm,
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

    /// Reverte um lote importado, removendo os atendimentos que não sofreram edição manual posterior e preservando os editados.
    pub fn undo_saiop_batch(&mut self, batch_id: i64) -> Result<(usize, usize)> {
        if let Some(cfg) = self.client_config()? {
            return match self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::UndoSaiopBatch { batch_id },
            )? {
                crate::intranet::MutationReply::Counts(removed, kept) => Ok((removed, kept)),
                _ => bail!("Resposta inesperada ao reverter lote no anfitrião."),
            };
        }

        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let mut rows = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, payload FROM saiop_attendances WHERE batch_id = ?1",
            )?;
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
            if att.human_edited || !att.task_ids.is_empty() {
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

    /// Cria uma tarefa vinculada a um atendimento do gabinete e ao documento
    /// importado. Sem documento vinculado não há tarefa: a providência fica
    /// registrada apenas na linha do tempo e o retorno é `None`.
    pub fn create_task_for_saiop(
        &mut self,
        attendance_id: i64,
        title: &str,
        priority: &str,
        due_date: &str,
        assignee: &str,
    ) -> Result<Option<i64>> {
        let mut attendances = self.list_saiop_attendances()?;
        let att = attendances
            .iter_mut()
            .find(|a| a.id == attendance_id)
            .context("Atendimento não encontrado.")?;
        let version = self
            .item_version("saiop", attendance_id)
            .ok()
            .map(|v| v.version);

        let task_id = if let Some(doc_id) = att.document_id {
            let task = Task {
                id: 0,
                document_id: doc_id,
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
            };
            let id = self.save_task(&task)?;
            if !att.task_ids.contains(&id) {
                att.task_ids.push(id);
            }
            id
        } else {
            0
        };
        att.events.push(SaiopEvent {
            when: Utc::now().to_rfc3339(),
            actor: self.active_actor(),
            kind: "tarefa".into(),
            detail: if task_id > 0 {
                format!("Tarefa #{task_id} criada: {}", title.trim())
            } else {
                format!("Providência registrada no atendimento: {}", title.trim())
            },
        });
        self.save_saiop_attendance(att, version)?;
        Ok((task_id > 0).then_some(task_id))
    }

    /// What `undo_saiop_batch` would do, without doing it.
    pub fn undo_saiop_batch_plan(&self, batch_id: i64) -> Result<(usize, usize)> {
        let mut removed = 0;
        let mut preserved = 0;
        for att in self.list_saiop_attendances()? {
            if att.batch_id == Some(batch_id) {
                if att.human_edited || !att.task_ids.is_empty() {
                    preserved += 1;
                } else {
                    removed += 1;
                }
            }
        }
        Ok((removed, preserved))
    }
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
        parse_shared_strings(std::str::from_utf8(ss_xml).context("sharedStrings.xml não é UTF-8")?)
    } else {
        Vec::new()
    };

    let (sheet_name, sheet_path) = resolve_processos_sheet_path(&entries)?;
    let sheet_bytes = entries
        .get(&sheet_path)
        .or_else(|| entries.get(sheet_path.trim_start_matches('/')))
        .with_context(|| format!("Aba '{sheet_name}' ({sheet_path}) não encontrada no arquivo .xlsx"))?;
    let sheet_xml =
        std::str::from_utf8(sheet_bytes).context("XML da planilha não está em UTF-8 válido")?;

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
                if let (Some(id), Some(target)) = (
                    xml_attr(&tag, "Id"),
                    xml_attr(&tag, "Target"),
                ) {
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
    bail!("Não foi possível localizar nenhuma aba de planilha no arquivo .xlsx.")
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
        let after_t = xml_fragment.as_bytes().get(abs + 2).copied().unwrap_or(b' ');
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

/// Lê entradas de um arquivo ZIP (`.xlsx`) a partir do Central Directory e descompacta blocos Stored (0) ou Deflate (8).
fn read_zip_entries(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    ensure!(bytes.len() >= 22, "Arquivo .xlsx truncado ou inválido.");
    // Localiza End of Central Directory (EOCD) signature 0x06054b50
    let min_eocd = bytes.len().saturating_sub(65557);
    let mut eocd_pos = None;
    for i in (min_eocd..=bytes.len() - 22).rev() {
        if bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            eocd_pos = Some(i);
            break;
        }
    }
    let eocd = eocd_pos.context("Assinatura ZIP não encontrada no arquivo .xlsx.")?;
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
            "Cabeçalho Central Directory do .xlsx inválido."
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
            "Nome de entrada ZIP truncado."
        );
        let name = String::from_utf8_lossy(&bytes[pos + 46..pos + 46 + name_len]).into_owned();
        pos += 46 + name_len + extra_len + comment_len;

        if name.ends_with('/') || uncomp_size > 64 * 1024 * 1024 {
            continue;
        }
        ensure!(
            local_offset + 30 <= bytes.len()
                && bytes[local_offset..local_offset + 4] == [0x50, 0x4b, 0x03, 0x04],
            "Cabeçalho local ZIP inválido."
        );
        let l_name =
            u16::from_le_bytes([bytes[local_offset + 26], bytes[local_offset + 27]]) as usize;
        let l_extra =
            u16::from_le_bytes([bytes[local_offset + 28], bytes[local_offset + 29]]) as usize;
        let data_start = local_offset + 30 + l_name + l_extra;
        ensure!(
            data_start + comp_size <= bytes.len(),
            "Dados da entrada ZIP truncados."
        );
        let comp_slice = &bytes[data_start..data_start + comp_size];

        let decoded = match method {
            0 => comp_slice.to_vec(),
            8 => inflate_raw(comp_slice, uncomp_size)?,
            other => bail!("Método de compressão ZIP não suportado ({other}) em {name}."),
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
        ensure!(self.byte_pos < self.data.len(), "Fluxo Deflate truncado.");
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
            ensure!(len <= 15, "Comprimento Huffman inválido.");
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
        bail!("Código Huffman inválido no fluxo Deflate.")
    }
}

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
                    "Bloco Stored truncado."
                );
                let len = u16::from_le_bytes([
                    reader.data[reader.byte_pos],
                    reader.data[reader.byte_pos + 1],
                ]) as usize;
                reader.byte_pos += 4;
                ensure!(
                    reader.byte_pos + len <= reader.data.len(),
                    "Dados Stored truncados."
                );
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
                                let prev = *all_lens
                                    .last()
                                    .context("Repetição Huffman sem comprimento anterior.")?;
                                let repeat = reader.read_bits(2)? as usize + 3;
                                all_lens.extend(std::iter::repeat(prev).take(repeat));
                            }
                            17 => {
                                let repeat = reader.read_bits(3)? as usize + 3;
                                all_lens.extend(std::iter::repeat(0).take(repeat));
                            }
                            18 => {
                                let repeat = reader.read_bits(7)? as usize + 11;
                                all_lens.extend(std::iter::repeat(0).take(repeat));
                            }
                            _ => bail!("Símbolo inválido na árvore de comprimentos."),
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
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let len_idx = sym - 257;
                        ensure!(len_idx < LEN_BASE.len(), "Código de comprimento inválido.");
                        let length = LEN_BASE[len_idx]
                            + reader.read_bits(LEN_EXTRA[len_idx])? as usize;
                        let dist_sym = dist_tree.decode(&mut reader)? as usize;
                        ensure!(dist_sym < DIST_BASE.len(), "Código de distância inválido.");
                        let distance = DIST_BASE[dist_sym]
                            + reader.read_bits(DIST_EXTRA[dist_sym])? as usize;
                        ensure!(
                            distance > 0 && distance <= out.len(),
                            "Distância LZ77 fora da janela."
                        );
                        for _ in 0..length {
                            let b = out[out.len() - distance];
                            out.push(b);
                        }
                    }
                }
            }
            _ => bail!("Tipo de bloco Deflate reservado/inválido."),
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

// ============================================================================

/// A titled block of the atendimento form: label on the left, field on the
/// right, one row per field.
fn form_section(
    ui: &mut egui::Ui,
    title: &str,
    id: impl egui::AsIdSalt,
    rows: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(6.0);
    ui.strong(title);
    egui::Grid::new(id).num_columns(2).spacing([8.0, 4.0]).show(ui, rows);
}
// Interface gráfica "Atendimentos" (Controle do gabinete do SAIOP)
// ============================================================================

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

/// Sortable columns of the grid.
#[derive(Clone, Copy, PartialEq, Eq)]
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

impl SaiopSort {
    fn title(self) -> &'static str {
        match self {
            Self::Reference => "Referência",
            Self::Subject => "Assunto",
            Self::Assignee => "Responsável",
            Self::Entry => "Entrada",
            Self::Status => "Situação",
            Self::Due => "Prazo",
            Self::Age => "Idade / Espera",
            Self::Destination => "Destino / Saída",
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
                if att.due_date.is_empty() { "9999".to_owned() } else { att.due_date.clone() },
                0,
            ),
            Self::Age => (String::new(), -att.age_days(today).unwrap_or(0)),
            Self::Destination => (
                if att.sent_to.is_empty() { att.exit_date.clone() } else { att.sent_to.to_lowercase() },
                0,
            ),
        }
    }
}

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
    link_number: String,
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
}

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
            link_number: String::new(),
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
        }
    }
}

impl SaiopPanel {
    pub fn dirty(&self) -> bool {
        self.editor != self.saved_snapshot
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
    }

    pub fn show(&mut self, ui: &mut egui::Ui, store: &mut Store) -> Option<i64> {
        let mut open_document = None;
        let today = Local::now().date_naive();
        let all_items = match store.list_saiop_attendances() {
            Ok(list) => list,
            Err(e) => {
                ui.colored_label(ERROR, format!("Erro ao carregar Atendimentos: {e}"));
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

        ui.horizontal_wrapped(|ui| {
            ui.heading("Atendimentos");
            ui.label(RichText::new("ⓘ").color(Color32::from_rgb(120, 130, 140)))
                .on_hover_text(format!(
                    "Controle operacional do gabinete (SAIOP). {} atendimento(s) no total, {} referência(s) distinta(s).",
                    all_items.len(),
                    distinct_refs.len()
                ));
            ui.label(format!("{open_count} em aberto"));
            if overdue_count > 0 {
                ui.colored_label(ERROR, format!("• {overdue_count} com prazo vencido"));
            }
            if ui
                .button("Importar planilha…")
                .on_hover_text("Lê a aba PROCESSOS de um arquivo .xlsx do controle do gabinete.")
                .clicked()
            {
                self.import_open = true;
            }
            if ui.button("Registrar nova entrada").clicked() {
                if self.dirty() {
                    self.set_notice(
                        "Salve ou descarte o atendimento em edição antes de iniciar outro.",
                        true,
                    );
                } else {
                    let mut fresh = SaiopAttendance::new_manual();
                    if let Ok(cfg) = store.load_intranet_config() {
                        fresh.assignee = cfg.member_name;
                    }
                    self.load_into_editor(store, fresh);
                }
            }
        });

        if !self.message.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(if self.error { ERROR } else { ACCENT }, &self.message);
                if ui.small_button("Fechar").clicked() {
                    self.message.clear();
                }
            });
        }

        // Importador assistido da planilha PROCESSOS (.xlsx) e gestão de lotes
        let mut close_import = false;
        let mut import_open = self.import_open;
        egui::Window::new("Importar planilha do controle")
            .open(&mut import_open)
            .default_width(860.0)
            .resizable(true)
            .show(ui.ctx(), |ui| {
            ui.small("Escolha o arquivo .xlsx do controle do gabinete. A aba PROCESSOS é lida; nada é gravado antes de você confirmar.");
            ui.horizontal_wrapped(|ui| {
                if ui.button("Escolher planilha…").clicked() {
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
                        .desired_width(360.0),
                );
                ui.checkbox(
                    &mut self.tramitado_keep_open,
                    "“Tramitado” com retorno da UAS fica aberto",
                )
                .on_hover_text("Quando a planilha diz “Tramitado” e “Retorno para UAS = Sim”, o atendimento entra como “Aguardando resposta” em vez de “Encerrado”.");
                if ui.button("Ler a planilha").clicked() {
                    let path = Path::new(self.xlsx_path.trim().trim_matches('"'));
                    match store.preview_saiop_xlsx(path, self.tramitado_keep_open) {
                        Ok(prev) => {
                            self.set_notice(
                                format!(
                                    "Prévia pronta: {} atendimento(s) identificados na aba {}. Confira abaixo antes de confirmar.",
                                    prev.items.len(),
                                    prev.sheet_name
                                ),
                                false,
                            );
                            self.import_preview = Some(prev);
                        }
                        Err(e) => self.set_notice(format!("Falha ao ler planilha: {e}"), true),
                    }
                }
            });

            if let Some(prev) = self.import_preview.clone() {
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.strong(format!(
                        "Prévia: {} (aba {})",
                        prev.file_name, prev.sheet_name
                    ));
                    let new_rows = prev.new_rows();
                    let repeated_rows = prev.items.len() - new_rows;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{} linha(s) lida(s)", prev.items.len()));
                        ui.separator();
                        ui.label(format!("{new_rows} nova(s)"));
                        if repeated_rows > 0 {
                            ui.separator();
                            ui.colored_label(WARNING, format!("{repeated_rows} já registrada(s) no controle"));
                        }
                        if prev.flagged_count > 0 {
                            ui.separator();
                            ui.colored_label(WARNING, format!("{} com alerta para conferir", prev.flagged_count));
                        }
                        if prev.linked_documents > 0 {
                            ui.separator();
                            ui.label(format!("{} ligada(s) a documentos importados", prev.linked_documents));
                        }
                        if prev.ignored_formula_rows > 0 {
                            ui.separator();
                            ui.small(format!("{} linha(s) vazias ignoradas", prev.ignored_formula_rows));
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("saiop-preview-rows")
                        .max_height(240.0)
                        .show(ui, |ui| {
                            egui::Grid::new("saiop-preview-grid").striped(true).min_col_width(60.0).show(ui, |ui| {
                                for title in ["Linha", "Referência", "Assunto", "Responsável", "Entrada", "Situação", "Prazo", "Alertas", "No controle"] {
                                    ui.strong(title);
                                }
                                ui.end_row();
                                for ((att, raw), existing) in prev.items.iter().zip(&prev.already_registered) {
                                    ui.label(raw.row_number.to_string());
                                    ui.label(if att.reference_raw.is_empty() { "(sem referência)" } else { &att.reference_raw });
                                    let subject: String = att.subject.chars().take(40).collect();
                                    ui.label(subject).on_hover_text(&att.subject);
                                    ui.label(if att.assignee.is_empty() { "—" } else { &att.assignee });
                                    ui.label(crate::labels::date(&att.entry_date));
                                    ui.label(status_label(&att.status));
                                    ui.label(if att.due_date.is_empty() {
                                        if att.due_date_raw.is_empty() { "—".to_owned() } else { format!("conferir: {}", att.due_date_raw) }
                                    } else {
                                        crate::labels::date(&att.due_date)
                                    });
                                    if att.review_flags.is_empty() {
                                        ui.label("—");
                                    } else {
                                        ui.colored_label(WARNING, format!("{} ⚠", att.review_flags.len()))
                                            .on_hover_text(att.review_flags.join("\n"));
                                    }
                                    match existing {
                                        Some(id) => { ui.colored_label(WARNING, format!("já registrada (#{id})")); }
                                        None => { ui.label("nova"); }
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    ui.horizontal_wrapped(|ui| {
                        let mut to_commit: Option<SaiopImportPreview> = None;
                        if repeated_rows > 0 {
                            if ui
                                .add_enabled(new_rows > 0, egui::Button::new(format!("Importar só as {new_rows} nova(s)")))
                                .clicked()
                            {
                                to_commit = Some(prev.only_new());
                            }
                            if ui
                                .button(format!("Importar todas ({}) e duplicar as já registradas", prev.items.len()))
                                .clicked()
                            {
                                to_commit = Some(prev.clone());
                            }
                        } else if ui.button(format!("Importar {} atendimento(s)", prev.items.len())).clicked() {
                            to_commit = Some(prev.clone());
                        }
                        if ui.button("Cancelar").clicked() {
                            self.import_preview = None;
                        }
                        if let Some(chosen) = to_commit {
                            match store.commit_saiop_import(&chosen) {
                                Ok(batch) => {
                                    self.set_notice(
                                        format!(
                                            "Importação concluída: {} atendimento(s) de {}.",
                                            batch.total_rows, batch.file_name
                                        ),
                                        false,
                                    );
                                    self.import_preview = None;
                                    close_import = true;
                                }
                                Err(e) => self.set_notice(format!("Importação recusada: {e}"), true),
                            }
                        }
                    });
                });
            }

            if let Ok(batches) = store.list_saiop_batches() {
                if !batches.is_empty() {
                    ui.separator();
                    ui.collapsing(format!("Importações anteriores ({})", batches.len()), |ui| {
                    for b in batches {
                        ui.horizontal_wrapped(|ui| {
                            ui.small(format!(
                                "{} • {} (aba {}) • {} linha(s), {} com alerta • por {}",
                                crate::labels::datetime(&b.imported_at),
                                b.file_name,
                                b.sheet_name,
                                b.total_rows,
                                b.flagged_rows,
                                b.actor
                            ));
                            let pending_here = self.pending_undo.as_ref().is_some_and(|(p, _, _)| p.id == b.id);
                            if !pending_here && ui.small_button("Desfazer importação…").clicked() {
                                match store.undo_saiop_batch_plan(b.id) {
                                    Ok((removed, kept)) => self.pending_undo = Some((b.clone(), removed, kept)),
                                    Err(e) => self.set_notice(format!("Não foi possível conferir a importação: {e}"), true),
                                }
                            }
                        });
                        if let Some((pending, removed, kept)) = self.pending_undo.clone() {
                            if pending.id == b.id {
                                ui.group(|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.colored_label(WARNING, format!(
                                        "Desfazer a importação de {}? {} atendimento(s) serão removidos. {} serão mantidos porque foram editados ou têm tarefa.",
                                        pending.file_name, removed, kept
                                    ));
                                    ui.horizontal(|ui| {
                                        if ui.button("Desfazer importação").clicked() {
                                            match store.undo_saiop_batch(pending.id) {
                                                Ok((removed, kept)) => self.set_notice(
                                                    format!(
                                                        "Importação desfeita: {removed} atendimento(s) removidos e {kept} mantidos por terem edição ou tarefa."
                                                    ),
                                                    false,
                                                ),
                                                Err(e) => self.set_notice(format!("Não foi possível desfazer: {e}"), true),
                                            }
                                            self.pending_undo = None;
                                        }
                                        if ui.button("Cancelar").clicked() {
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
        let queue_count = |queue: SaiopQueue| all_items.iter().filter(|a| in_queue(a, queue)).count();
        let advanced_active = !self.assignee_filter.trim().is_empty()
            || !self.origin_filter.trim().is_empty()
            || !self.dest_filter.trim().is_empty()
            || !self.status_filter.is_empty();

        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Buscar referência, assunto, observação…")
                    .desired_width(240.0),
            );
            egui::ComboBox::from_id_salt("saiop-queue")
                .selected_text(format!("Fila: {} ({})", self.queue.label(), queue_count(self.queue)))
                .width(260.0)
                .show_ui(ui, |ui| {
                    for q in queues {
                        ui.selectable_value(
                            &mut self.queue,
                            q,
                            format!("{} ({})", q.label(), queue_count(q)),
                        );
                    }
                });
            if self.queue == SaiopQueue::DueSoon {
                ui.label("até");
                ui.add(egui::DragValue::new(&mut self.due_soon_days).range(1..=60).suffix(" dias"));
            } else if self.queue == SaiopQueue::StaleDays {
                ui.label("parado há");
                ui.add(egui::DragValue::new(&mut self.stale_days_threshold).range(1..=365).suffix(" dias"));
            }
            let toggle = if self.filters_open { "Filtros ▴" } else { "Filtros ▾" };
            if ui
                .add(egui::Button::new(toggle).small())
                .clicked()
            {
                self.filters_open = !self.filters_open;
            }
            if (advanced_active || !self.query.is_empty() || self.queue != SaiopQueue::All)
                && ui.small_button("Limpar").clicked()
            {
                self.query.clear();
                self.assignee_filter.clear();
                self.origin_filter.clear();
                self.dest_filter.clear();
                self.status_filter.clear();
                self.queue = SaiopQueue::All;
            }
        });
        if self.filters_open || advanced_active {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.assignee_filter)
                        .hint_text("Responsável…")
                        .desired_width(140.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.origin_filter)
                        .hint_text("Recebido de…")
                        .desired_width(140.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.dest_filter)
                        .hint_text("Enviado para…")
                        .desired_width(140.0),
                );
                egui::ComboBox::from_id_salt("saiop-status-filter")
                    .selected_text(if self.status_filter.is_empty() {
                        "Situação: todas"
                    } else {
                        status_label(&self.status_filter)
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.status_filter, String::new(), "Todas");
                        for (k, label) in STATUS_OPTIONS {
                            ui.selectable_value(&mut self.status_filter, (*k).to_owned(), *label);
                        }
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
        let sort = self.sort;
        filtered.sort_by_cached_key(|att| sort.key(att, today));
        if !self.sort_ascending {
            filtered.reverse();
        }

        if filtered.len() != all_items.len() {
            ui.small(format!(
                "{} de {} atendimento(s)",
                filtered.len(),
                all_items.len()
            ));
        }
        ui.separator();

        // Grade (esquerda) e ficha do atendimento selecionado (direita)
        let ficha_open = self.editor.is_some();
        let total_width = ui.available_width();
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
        ui.set_width(if ficha_open { (total_width * 0.5).max(420.0) } else { total_width });
        egui::ScrollArea::both()
            .id_salt("saiop-grid-scroll")
            .max_height(height)
            .show(ui, |ui| {
                egui::Grid::new("saiop-attendances-grid")
                    .striped(true)
                    .min_col_width(70.0)
                    .show(ui, |ui| {
                        for column in [
                            SaiopSort::Reference,
                            SaiopSort::Subject,
                            SaiopSort::Assignee,
                            SaiopSort::Entry,
                            SaiopSort::Status,
                            SaiopSort::Due,
                            SaiopSort::Age,
                            SaiopSort::Destination,
                        ] {
                            let marker = if self.sort == column {
                                if self.sort_ascending { " ▲" } else { " ▼" }
                            } else {
                                ""
                            };
                            if ui
                                .add(egui::Button::new(RichText::new(format!("{}{marker}", column.title())).strong()).frame(false))
                                .on_hover_text("Ordenar por esta coluna")
                                .clicked()
                            {
                                if self.sort == column {
                                    self.sort_ascending = !self.sort_ascending;
                                } else {
                                    self.sort = column;
                                    self.sort_ascending = !matches!(column, SaiopSort::Entry | SaiopSort::Age);
                                }
                            }
                        }
                        ui.end_row();

                        for att in &filtered {
                            let is_sel = self.selected_id == Some(att.id);
                            let ref_label = if att.reference_raw.is_empty() {
                                "(sem referência)".to_owned()
                            } else {
                                att.reference_raw.clone()
                            };
                            if ui.selectable_label(is_sel, ref_label).clicked() {
                                if self.dirty() {
                                    self.set_notice(
                                        "Salve ou descarte as alterações na ficha antes de trocar de linha.",
                                        true,
                                    );
                                } else {
                                    self.load_into_editor(store, (*att).clone());
                                }
                            }
                            let subj_short: String = att.subject.chars().take(46).collect();
                            ui.label(subj_short);
                            ui.label(if att.assignee.is_empty() {
                                "—"
                            } else {
                                &att.assignee
                            });
                            ui.label(if att.entry_date.is_empty() {
                                "—".to_owned()
                            } else {
                                crate::labels::date(&att.entry_date)
                            });
                            ui.label(status_label(&att.status));

                            // Coluna Prazo (explícita quando sem prazo ou com texto ambíguo)
                            let due_text = crate::labels::date(&att.due_date);
                            if !att.due_date.is_empty() {
                                if let Some(delta) = att.due_delta_days(today) {
                                    if delta > 0 {
                                        ui.colored_label(
                                            ERROR,
                                            format!("{due_text} (vencido há {delta} d)"),
                                        );
                                    } else if delta == 0 {
                                        ui.colored_label(WARNING, format!("{due_text} (vence hoje)"));
                                    } else {
                                        ui.label(format!("{due_text} (em {} d)", -delta));
                                    }
                                } else {
                                    ui.label(due_text);
                                }
                            } else if !att.due_date_raw.is_empty() {
                                ui.colored_label(
                                    WARNING,
                                    format!("Conferir: {}", att.due_date_raw),
                                );
                            } else {
                                ui.small("Sem prazo");
                            }

                            // Idade e espera calculadas por eventos (nunca pela fórmula DATEDIF cacheada)
                            let age = att
                                .age_days(today)
                                .map(|d| format!("{d} d"))
                                .unwrap_or_else(|| "—".into());
                            let wait = att
                                .external_wait_days(today)
                                .map(|w| format!(" • {w} d fora"))
                                .unwrap_or_default();
                            ui.label(format!("{age}{wait}"))
                                .on_hover_text("Dias desde a entrada • dias aguardando retorno externo");

                            ui.label(if att.sent_to.is_empty() {
                                crate::labels::date(&att.exit_date)
                            } else {
                                att.sent_to.clone()
                            });
                            ui.end_row();
                        }
                    });
                if filtered.is_empty() {
                    ui.label("Nenhum atendimento nesta fila ou busca.");
                }
            });
        });

        // Ficha do atendimento selecionado
        if let Some(mut ed) = self.editor.clone() {
            ui.separator();
            ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let mut save_clicked = false;
            let mut revert_clicked = false;
            let mut create_task_clicked = false;
            let mut close_clicked = false;
            let is_dirty = self.dirty();

            ui.horizontal_wrapped(|ui| {
                ui.heading(if ed.id > 0 {
                    if ed.reference_raw.is_empty() {
                        "Atendimento sem referência".to_owned()
                    } else {
                        ed.reference_raw.clone()
                    }
                } else {
                    "Novo atendimento".into()
                });
                if is_dirty {
                    ui.colored_label(WARNING, "● não salvo");
                }
            });
            // Actions stay at the top so they are visible without scrolling.
            ui.horizontal_wrapped(|ui| {
                save_clicked = ui.add_enabled(is_dirty || ed.id == 0, egui::Button::new("Salvar")).clicked();
                revert_clicked = ui
                    .add_enabled(is_dirty, egui::Button::new("Descartar alterações"))
                    .clicked();
                if ed.is_open() && ed.id > 0 && ui.button("Encerrar").on_hover_text("Marca como encerrado e salva.").clicked() {
                    ed.status = "encerrado".into();
                    ed.closed_at = Local::now().format("%Y-%m-%d").to_string();
                    save_clicked = true;
                } else if !ed.is_open() && ui.button("Reabrir").on_hover_text("Volta para “Em análise” e salva.").clicked() {
                    ed.status = "em_analise".into();
                    ed.closed_at.clear();
                    save_clicked = true;
                }
                close_clicked = ui
                    .add_enabled(!is_dirty, egui::Button::new("Fechar ficha"))
                    .on_disabled_hover_text("Salve ou descarte as alterações antes de fechar.")
                    .clicked();
            });

            egui::ScrollArea::vertical()
                .id_salt("saiop-detail-scroll")
                .max_height(height - 70.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if let Some(doc_id) = ed.document_id {
                            if ui.button("Abrir documento importado").clicked() {
                                open_document = Some(doc_id);
                            }
                            if ui.small_button("Desligar do documento").clicked() {
                                ed.document_id = None;
                            }
                        } else {
                            if let Some(url) = ed.composed_sigadoc_url() {
                                ui.hyperlink_to("Abrir no SIGADOC", url)
                                    .on_hover_text("Endereço montado a partir da referência; confira na página.");
                            }
                            ui.add(
                                egui::TextEdit::singleline(&mut self.link_number)
                                    .hint_text("número do documento importado")
                                    .desired_width(200.0),
                            );
                            if ui.small_button("Ligar a documento").on_hover_text("Liga este atendimento a um documento já importado, pelo número oficial.").clicked() {
                                let wanted = normalize_reference(&self.link_number);
                                match store.list_documents("") {
                                    Ok(docs) => match docs.into_iter().find(|d| normalize_reference(&d.document_number) == wanted) {
                                        Some(doc) => {
                                            ed.document_id = Some(doc.id);
                                            self.link_number.clear();
                                        }
                                        None => self.set_notice(format!("Nenhum documento importado com o número {}.", self.link_number.trim()), true),
                                    },
                                    Err(e) => self.set_notice(format!("Não foi possível consultar a biblioteca: {e}"), true),
                                }
                            }
                        }
                        if !ed.reference_norm.is_empty() {
                            let same_ref_count = all_items
                                .iter()
                                .filter(|a| a.reference_norm == ed.reference_norm)
                                .count();
                            if same_ref_count > 1 {
                                ui.colored_label(
                                    ACCENT,
                                    format!("{same_ref_count} passagens desta referência"),
                                ).on_hover_text("A mesma referência entrou no gabinete mais de uma vez; cada passagem é um atendimento.");
                            }
                        }
                    });

                    if !ed.review_flags.is_empty() {
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(
                                    WARNING,
                                    format!(
                                        "Alertas de conferência ({}):",
                                        ed.review_flags.len()
                                    ),
                                );
                                if ui
                                    .small_button("Conferido: limpar alertas")
                                    .on_hover_text("Os alertas somem ao salvar o atendimento.")
                                    .clicked()
                                {
                                    ed.review_flags.clear();
                                }
                            });
                            for flag in &ed.review_flags {
                                ui.small(format!("• {flag}"));
                            }
                        });
                    }

                    // One column of label/field rows per section: fields size
                    // to the ficha, so nothing overflows into a neighbour or
                    // gets cut at the edge.
                    let combo_width = (ui.available_width() - 160.0).clamp(150.0, 320.0);
                    let ed_id = ed.id;
                    form_section(ui, "Identificação", ("saiop-form-id", ed_id), |ui| {
                        ui.label("Referência:");
                        ui.add(egui::TextEdit::singleline(&mut ed.reference_raw).desired_width(f32::INFINITY))
                            .on_hover_text("Número do processo ou documento, ou um código provisório. Informe a referência ou o assunto.");
                        ui.end_row();
                        ui.label("Entrada:");
                        crate::labels::date_field(ui, ("saiop-entry", ed_id), &mut ed.entry_date);
                        ui.end_row();
                        ui.label("Responsável:");
                        ui.horizontal(|ui| {
                            let width = (ui.available_width() - 120.0).max(80.0);
                            ui.add(egui::TextEdit::singleline(&mut ed.assignee).desired_width(width));
                            if ui.small_button("Atribuir a mim").clicked() {
                                if let Ok(cfg) = store.load_intranet_config() {
                                    ed.assignee = cfg.member_name;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Recebido de:");
                        ui.add(egui::TextEdit::singleline(&mut ed.received_from).desired_width(f32::INFINITY));
                        ui.end_row();
                    });
                    form_section(ui, "Situação e prioridade", ("saiop-form-status", ed_id), |ui| {
                        ui.label("Situação:");
                        egui::ComboBox::from_id_salt("ed-saiop-status")
                            .selected_text(status_label(&ed.status))
                            .width(combo_width)
                            .show_ui(ui, |ui| {
                                for (k, label) in STATUS_OPTIONS {
                                    ui.selectable_value(&mut ed.status, (*k).to_owned(), *label);
                                }
                            });
                        ui.end_row();
                        ui.label("Prioridade:");
                        egui::ComboBox::from_id_salt("ed-saiop-priority")
                            .selected_text(priority_label(&ed.priority))
                            .width(combo_width)
                            .show_ui(ui, |ui| {
                                for (k, label) in PRIORITY_OPTIONS {
                                    ui.selectable_value(&mut ed.priority, (*k).to_owned(), *label);
                                }
                            });
                        ui.end_row();
                        ui.label("Motivo da prioridade:");
                        ui.add(egui::TextEdit::singleline(&mut ed.priority_reason).desired_width(f32::INFINITY));
                        ui.end_row();
                    });
                    form_section(ui, "Prazo", ("saiop-form-due", ed_id), |ui| {
                        ui.label("Prazo:");
                        crate::labels::date_field(ui, ("saiop-due", ed_id), &mut ed.due_date);
                        ui.end_row();
                        ui.label("Tipo de prazo:");
                        egui::ComboBox::from_id_salt("ed-saiop-due-kind")
                            .selected_text(due_kind_label(&ed.due_kind))
                            .width(combo_width)
                            .show_ui(ui, |ui| {
                                for (k, label) in DUE_KIND_OPTIONS {
                                    ui.selectable_value(&mut ed.due_kind, (*k).to_owned(), *label);
                                }
                            });
                        ui.end_row();
                    });
                    form_section(ui, "Condições e relações", ("saiop-form-conditions", ed_id), |ui| {
                        ui.label("Retorno para UAS?:");
                        egui::ComboBox::from_id_salt("ed-saiop-uas")
                            .selected_text(tri_state_label(&ed.uas_return))
                            .width(combo_width)
                            .show_ui(ui, |ui| {
                                for (k, label) in TRI_STATE_OPTIONS {
                                    ui.selectable_value(&mut ed.uas_return, (*k).to_owned(), *label);
                                }
                            });
                        ui.end_row();
                        ui.label("Assinado?:");
                        egui::ComboBox::from_id_salt("ed-saiop-signed")
                            .selected_text(tri_state_label(&ed.signed_status))
                            .width(combo_width)
                            .show_ui(ui, |ui| {
                                for (k, label) in TRI_STATE_OPTIONS {
                                    ui.selectable_value(&mut ed.signed_status, (*k).to_owned(), *label);
                                }
                            });
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut ed.scheduled, "Agendado");
                            ui.checkbox(&mut ed.sensitive, "Documento sensível");
                        });
                        ui.end_row();
                        ui.label("Apensado / juntado:");
                        ui.add(egui::TextEdit::singleline(&mut ed.attachment_relation).desired_width(f32::INFINITY))
                            .on_hover_text("Relação de apensamento ou juntada informada na planilha ou por você.");
                        ui.end_row();
                    });
                    form_section(ui, "Resultado", ("saiop-form-result", ed_id), |ui| {
                        ui.label("Entrega prevista:");
                        egui::ComboBox::from_id_salt("ed-saiop-doctype")
                            .selected_text(if ed.planned_doc_type.is_empty() {
                                "Não definida"
                            } else {
                                &ed.planned_doc_type
                            })
                            .width(combo_width)
                            .show_ui(ui, |ui| {
                                for opt in DOC_TYPE_OPTIONS {
                                    ui.selectable_value(
                                        &mut ed.planned_doc_type,
                                        (*opt).to_owned(),
                                        if opt.is_empty() { "Não definida" } else { *opt },
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Resultado da etapa:");
                        egui::ComboBox::from_id_salt("ed-saiop-outcome")
                            .selected_text(outcome_label(&ed.outcome))
                            .width(combo_width)
                            .show_ui(ui, |ui| {
                                for (k, label) in OUTCOME_OPTIONS {
                                    ui.selectable_value(&mut ed.outcome, (*k).to_owned(), *label);
                                }
                            });
                        ui.end_row();
                        ui.label("Data de saída:");
                        crate::labels::date_field(ui, ("saiop-exit", ed_id), &mut ed.exit_date);
                        ui.end_row();
                        ui.label("Enviado para:");
                        ui.add(egui::TextEdit::singleline(&mut ed.sent_to).desired_width(f32::INFINITY));
                        ui.end_row();
                    });
                    if !ed.verification_raw.is_empty() {
                        ui.small(format!("Verificação na planilha: {}", ed.verification_raw));
                    }
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label("Assunto:");
                        ui.add(
                            egui::TextEdit::singleline(&mut ed.subject)
                                .desired_width(f32::INFINITY),
                        );
                    });
                    ui.label("Observações:");
                    ui.add(
                        egui::TextEdit::multiline(&mut ed.observation)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );

                    // Tarefa ligada a este atendimento
                    ui.horizontal_wrapped(|ui| {
                        if ed.id > 0 {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_task_title)
                                    .hint_text("Nova tarefa para este atendimento…")
                                    .desired_width(220.0),
                            );
                            if ui
                                .button("Criar tarefa")
                                .on_hover_text(if ed.document_id.is_some() {
                                    "Cria uma tarefa no painel Tarefas ligada ao documento importado."
                                } else {
                                    "Sem documento importado ligado, a providência é registrada só na linha do tempo deste atendimento."
                                })
                                .clicked()
                            {
                                create_task_clicked = true;
                            }
                        }
                    });

                    // Histórico de eventos e registro original da planilha
                    if ed.id > 0 {
                        ui.collapsing("Linha do tempo", |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_event_note)
                                        .hint_text("Registrar evento datado (ex.: cobrado retorno por telefone)…")
                                        .desired_width(360.0),
                                );
                                        if ui
                                    .button("Adicionar evento")
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
                                    save_clicked = true;
                                }
                            });
                            for ev in ed.events.iter().rev() {
                                ui.small(format!(
                                    "• {} • {} • {}: {}",
                                    crate::labels::datetime(&ev.when),
                                    ev.actor,
                                    crate::labels::saiop_event_kind(&ev.kind),
                                    ev.detail
                                ));
                            }
                        });
                        if let Ok(Some(raw_row)) = store.load_saiop_raw_row(ed.id) {
                            ui.collapsing(
                                format!(
                                    "Linha original da planilha (aba {}, linha {})",
                                    raw_row.sheet_name, raw_row.row_number
                                ),
                                |ui| {
                                    egui::Grid::new(("saiop-raw-row", ed.id))
                                        .striped(true)
                                        .num_columns(2)
                                        .show(ui, |ui| {
                                            for (column, value) in &raw_row.cells {
                                                if value.trim().is_empty() {
                                                    continue;
                                                }
                                                ui.label(format!(
                                                    "{} ({column})",
                                                    crate::labels::spreadsheet_column(column)
                                                ));
                                                ui.add(egui::Label::new(value).wrap());
                                                ui.end_row();
                                            }
                                            for (column, formula) in &raw_row.formulas {
                                                ui.label(format!("Fórmula em {column}"));
                                                ui.add(egui::Label::new(formula).wrap());
                                                ui.end_row();
                                            }
                                        });
                                },
                            );
                        }
                    }
                });

            self.editor = Some(ed.clone());

            if close_clicked {
                self.editor = None;
                self.saved_snapshot = None;
                self.selected_id = None;
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
                            self.set_notice(format!("Salve o atendimento antes de criar a tarefa: {e}"), true);
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
                                }
                            }
                            match created {
                                Some(_) => self.set_notice(
                                    format!("Tarefa criada no painel Tarefas: {title}"),
                                    false,
                                ),
                                None => self.set_notice(
                                    "Providência registrada na linha do tempo. Nenhuma tarefa foi criada porque este atendimento não está ligado a um documento importado.",
                                    false,
                                ),
                            }
                        }
                        Err(e) => self.set_notice(format!("Não foi possível criar a tarefa: {e}"), true),
                    }
                }
            } else if save_clicked {
                match store.save_saiop_attendance(&ed, self.base_version) {
                    Ok(saved_id) => {
                        if let Ok(updated_list) = store.list_saiop_attendances() {
                            if let Some(reloaded) =
                                updated_list.into_iter().find(|a| a.id == saved_id)
                            {
                                self.load_into_editor(store, reloaded);
                            }
                        }
                        self.set_notice("Atendimento salvo.", false);
                    }
                    Err(e) => self.set_notice(format!("Atendimento não salvo: {e}"), true),
                }
            }
            });
        }
        });

        open_document
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
                <si><t>CARLO</t></si>
                <si><t>MARIA</t></si>
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
}
