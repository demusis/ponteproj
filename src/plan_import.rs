//! `--import-plan`: a manual record (or an existing document) plus 1 to 100
//! linked tasks, created from a JSON file, resumable and idempotent by
//! `request_id`.
//!
//! What the module keeps, and why:
//!
//! - Nothing is written until the file has been read, parsed and checked
//!   without the library (`parse_plan`, `check_plan`), and a dry run writes
//!   nothing at all, not even the receipt table.
//! - The receipt (`plan_import_runs`, a LOCAL table: it is not part of the
//!   synchronised snapshot) holds only what survives a synchronisation: the
//!   request id, a digest of the canonical plan and how far the run got. The
//!   record is found through `requests` (request id -> document, which
//!   travels in snapshots) and the tasks through their uids (derived from
//!   the request id and the position in the plan). Local ids, the manual
//!   number and the titles are read again on every run.
//! - A rerun never recreates what the person deleted and never updates what
//!   already exists: it creates only what was never created.
use crate::{
    manual::{ManualFields, KINDS},
    model::{Capture, MAX_CAPTURE_TEXT_BYTES},
    record_import::{validate_record, Created, NewRecord},
    storage::Store,
    tasks::{Task, ACTIONS},
    trash::Kind,
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

/// Most tasks one plan may create.
pub const MAX_TASKS: usize = 100;
const PRIORITIES: [&str; 4] = ["baixa", "normal", "alta", "urgente"];

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub request_id: String,
    pub document: Option<PlanDocument>,
    pub document_number: Option<String>,
    pub tasks: Vec<PlanTask>,
    #[serde(default)]
    pub complete_triage: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDocument {
    pub title: String,
    pub text: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub categories: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanTask {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub due_date: String,
    #[serde(default = "normal")]
    pub priority: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub checklist: Vec<(bool, String)>,
    #[serde(default)]
    pub scheduled_start: String,
    #[serde(default)]
    pub scheduled_end: String,
    #[serde(default)]
    pub estimate_minutes: u32,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub project: String,
}
fn normal() -> String {
    "normal".into()
}

const PLAN_FIELDS: [&str; 5] = [
    "request_id",
    "document",
    "document_number",
    "tasks",
    "complete_triage",
];
const DOCUMENT_FIELDS: [&str; 7] = [
    "title",
    "text",
    "type",
    "date",
    "origin",
    "note",
    "categories",
];
const TASK_FIELDS: [&str; 12] = [
    "title",
    "action",
    "notes",
    "due_date",
    "priority",
    "assignee",
    "checklist",
    "scheduled_start",
    "scheduled_end",
    "estimate_minutes",
    "categories",
    "project",
];

/// Said once in any message that names a task: the list starts at 1.
const COUNT_NOTE: &str = " (nas mensagens, tasks[1] é a primeira tarefa da lista: a contagem começa em 1)";
fn with_count_note(message: String) -> String {
    if message.contains("tasks[") && !message.contains("a contagem começa em 1") {
        format!("{message}{COUNT_NOTE}")
    } else {
        message
    }
}
fn invalid(message: String) -> anyhow::Error {
    anyhow!(with_count_note(message))
}

/// The text of the file: UTF-8 (with or without BOM) or UTF-16 with BOM (what
/// the redirection of Windows PowerShell 5 writes). Anything else is refused
/// with the way to save it right.
pub fn decode_text(bytes: &[u8]) -> Result<String> {
    const HOW: &str = "Salve o arquivo como UTF-8 (no Bloco de Notas: Salvar como > Codificação UTF-8; no PowerShell: Set-Content -Encoding utf8 ou [System.IO.File]::WriteAllText(caminho, texto)).";
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec())
            .map_err(|_| anyhow!("O arquivo tem a marca UTF-8 mas traz bytes que não são UTF-8. {HOW}"));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let little = bytes[0] == 0xFF;
        let body = &bytes[2..];
        ensure!(
            body.len() % 2 == 0,
            "O arquivo UTF-16 está truncado (número ímpar de bytes). {HOW}"
        );
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        return String::from_utf16(&units)
            .map_err(|_| anyhow!("O arquivo UTF-16 tem caracteres inválidos. {HOW}"));
    }
    ensure!(
        !bytes.contains(&0),
        "O arquivo parece estar em UTF-16 sem marca de ordem de bytes. {HOW}"
    );
    String::from_utf8(bytes.to_vec())
        .map_err(|_| anyhow!("O arquivo não está em UTF-8 (talvez ANSI/Windows-1252). {HOW}"))
}

/// Reads a plan from the bytes of a file; every complaint is a Portuguese
/// sentence with the line and column of the problem and, for the shape of
/// the plan, the JSON field it is about.
pub fn parse_plan(bytes: &[u8]) -> Result<Plan> {
    let text = decode_text(bytes)?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| anyhow!("JSON inválido: {}", syntax_message(&e)))?;
    match serde_json::from_str::<Plan>(&text) {
        Ok(plan) => Ok(plan),
        Err(error) => {
            let place = format!("linha {}, coluna {}", error.line(), error.column());
            let detail = shape_problem(&value)
                .unwrap_or_else(|| serde_message(&error.to_string()));
            Err(invalid(format!("Plano inválido: {detail} ({place})")))
        }
    }
}

fn without_position(message: &str) -> &str {
    message.rsplit_once(" at line ").map_or(message, |(head, _)| head)
}
fn syntax_message(error: &serde_json::Error) -> String {
    let raw = error.to_string();
    let head = without_position(&raw);
    let said = if head.contains("EOF while parsing") {
        "o texto termina antes do fim do JSON (falta fechar { [ ou aspas)".to_owned()
    } else if head.contains("expected value") {
        "esperava um valor (texto entre aspas, número, true, false, null, [ ou {)".to_owned()
    } else if head.contains("expected `,` or `}`") {
        "esperava vírgula ou } (falta uma vírgula entre campos ou sobrou texto)".to_owned()
    } else if head.contains("expected `,` or `]`") {
        "esperava vírgula ou ] (falta uma vírgula entre itens ou sobrou texto)".to_owned()
    } else if head.contains("expected `:`") {
        "esperava dois-pontos (:) depois do nome do campo".to_owned()
    } else if head.contains("key must be a string") {
        "o nome do campo precisa estar entre aspas duplas".to_owned()
    } else if head.contains("trailing comma") {
        "vírgula sobrando antes de } ou ]".to_owned()
    } else if head.contains("trailing characters") {
        "há texto depois do fim do JSON".to_owned()
    } else if head.contains("control character") {
        "quebra de linha ou tabulação dentro de um texto: escreva \\n ou \\t".to_owned()
    } else if head.contains("invalid escape") {
        "sequência de escape inválida dentro de um texto (para uma barra invertida escreva \\\\)".to_owned()
    } else if head.contains("expected ident") {
        "palavra inválida (use true, false ou null, sem aspas)".to_owned()
    } else if head.contains("number") {
        "número inválido".to_owned()
    } else if head.contains("unicode") || head.contains("surrogate") || head.contains("hex escape") {
        "escape \\u inválido dentro de um texto".to_owned()
    } else {
        format!("erro de sintaxe ({head})")
    };
    format!("{said}, na linha {}, coluna {}", error.line(), error.column())
}
/// The typed deserializer's own complaint, in Portuguese, when the walk over
/// the plan found nothing more specific.
fn serde_message(raw: &str) -> String {
    let head = without_position(raw);
    let quoted = |prefix: &str| {
        head.strip_prefix(prefix)
            .and_then(|rest| rest.split('`').next())
            .map(str::to_owned)
    };
    if let Some(name) = quoted("unknown field `") {
        format!("campo desconhecido \"{name}\"")
    } else if let Some(name) = quoted("missing field `") {
        format!("falta o campo obrigatório \"{name}\"")
    } else if let Some(name) = quoted("duplicate field `") {
        format!("campo repetido \"{name}\"")
    } else {
        format!("valor de tipo ou formato inválido ({head})")
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "nulo",
        Value::Bool(_) => "verdadeiro/falso",
        Value::Number(_) => "número",
        Value::String(_) => "texto",
        Value::Array(_) => "lista",
        Value::Object(_) => "objeto",
    }
}
fn unknown_field(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    path: &str,
) -> Option<String> {
    object.keys().find(|key| !allowed.contains(&key.as_str())).map(|key| {
        format!(
            "{path}: campo desconhecido \"{key}\" (confira a grafia); os campos aceitos são: {}",
            allowed.join(", ")
        )
    })
}
fn wrong_type(path: &str, expected: &str, found: &Value) -> String {
    format!("{path}: esperava {expected}, mas encontrei {}", json_kind(found))
}
fn texts(object: &serde_json::Map<String, Value>, keys: &[&str], path: &str) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .filter(|value| !value.is_string())
            .map(|value| wrong_type(&format!("{path}.{key}"), "um texto entre aspas", value))
    })
}
fn text_list(object: &serde_json::Map<String, Value>, key: &str, path: &str) -> Option<String> {
    let value = object.get(key)?;
    let Some(items) = value.as_array() else {
        return Some(wrong_type(&format!("{path}.{key}"), "uma lista de textos", value));
    };
    items.iter().enumerate().find_map(|(index, item)| {
        (!item.is_string()).then(|| {
            wrong_type(&format!("{path}.{key}[{}]", index + 1), "um texto entre aspas", item)
        })
    })
}

/// The first thing wrong with the SHAPE of the plan (unknown or missing
/// fields, wrong types), with the JSON path.
fn shape_problem(root: &Value) -> Option<String> {
    let Some(plan) = root.as_object() else {
        return Some("o arquivo deve conter um objeto JSON ({ ... }) com request_id, document (ou document_number) e tasks".into());
    };
    if let Some(message) = unknown_field(plan, &PLAN_FIELDS, "plano") {
        return Some(message);
    }
    match plan.get("request_id") {
        None | Some(Value::Null) => {
            return Some("request_id: campo obrigatório (um UUID, como 47f3f62f-404c-4931-ae04-bc7eb1108c07)".into())
        }
        Some(value) if !value.is_string() => {
            return Some(wrong_type("request_id", "um texto entre aspas (um UUID)", value))
        }
        _ => {}
    }
    match plan.get("document_number") {
        None | Some(Value::Null) | Some(Value::String(_)) => {}
        Some(value) => return Some(wrong_type("document_number", "um texto entre aspas", value)),
    }
    match plan.get("complete_triage") {
        None | Some(Value::Bool(_)) => {}
        Some(value) => {
            return Some(wrong_type("complete_triage", "true ou false (sem aspas)", value))
        }
    }
    match plan.get("document") {
        None | Some(Value::Null) => {}
        Some(Value::Object(document)) => {
            if let Some(message) = unknown_field(document, &DOCUMENT_FIELDS, "document") {
                return Some(message);
            }
            for required in ["title", "text", "type"] {
                if !document.contains_key(required) {
                    return Some(format!("document.{required}: campo obrigatório"));
                }
            }
            if let Some(message) = texts(
                document,
                &["title", "text", "type", "date", "origin", "note"],
                "document",
            )
            .or_else(|| text_list(document, "categories", "document"))
            {
                return Some(message);
            }
        }
        Some(value) => return Some(wrong_type("document", "um objeto { ... }", value)),
    }
    let tasks = match plan.get("tasks") {
        None | Some(Value::Null) => return Some("tasks: campo obrigatório (uma lista com 1 a 100 tarefas)".into()),
        Some(Value::Array(tasks)) => tasks,
        Some(value) => return Some(wrong_type("tasks", "uma lista [ ... ] de tarefas", value)),
    };
    for (index, task) in tasks.iter().enumerate() {
        let path = format!("tasks[{}]", index + 1);
        let Some(task) = task.as_object() else {
            return Some(wrong_type(&path, "um objeto { ... }", &tasks[index]));
        };
        if let Some(message) = unknown_field(task, &TASK_FIELDS, &path) {
            return Some(message);
        }
        if let Some(message) = texts(
            task,
            &[
                "title",
                "action",
                "notes",
                "due_date",
                "priority",
                "assignee",
                "scheduled_start",
                "scheduled_end",
                "project",
            ],
            &path,
        ) {
            return Some(message);
        }
        if let Some(value) = task.get("estimate_minutes") {
            if !value.as_u64().is_some_and(|n| n <= u64::from(u32::MAX)) {
                return Some(format!(
                    "{path}.estimate_minutes: deve ser um número inteiro de 0 a 525600, sem aspas e sem casas decimais"
                ));
            }
        }
        if let Some(message) = text_list(task, "categories", &path) {
            return Some(message);
        }
        if let Some(value) = task.get("checklist") {
            let Some(items) = value.as_array() else {
                return Some(wrong_type(&format!("{path}.checklist"), "uma lista de pares [concluído, \"texto\"]", value));
            };
            for (n, item) in items.iter().enumerate() {
                let good = item
                    .as_array()
                    .is_some_and(|pair| pair.len() == 2 && pair[0].is_boolean() && pair[1].is_string());
                if !good {
                    return Some(format!(
                        "{path}.checklist[{}]: use um par [true ou false, \"texto do item\"]",
                        n + 1
                    ));
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// What can be checked without the library
// ---------------------------------------------------------------------------

/// A plan that passed every check that does not need the library.
pub struct Checked {
    /// The request id as the library keeps it: lowercase, hyphenated.
    pub request: String,
    /// Digest of the canonical plan (see `canonical_digest`).
    pub digest: String,
    /// The record to create (a plan with `document`).
    pub record: Option<NewRecord>,
    /// The tasks, with their deterministic uids; the categories and the
    /// project are filled in later, from the library.
    pub tasks: Vec<Task>,
}

fn uid(request: &str, index: usize) -> String {
    let hash = Sha256::digest(format!("ponte-plan\n{request}\n{index}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 15) | 0x80;
    bytes[8] = (bytes[8] & 63) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}
fn iso_date(value: &str) -> bool {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .is_ok_and(|d| d.format("%Y-%m-%d").to_string() == value)
}
fn iso_datetime(value: &str) -> Option<chrono::NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .ok()
        .filter(|d| d.format("%Y-%m-%dT%H:%M").to_string() == value)
}
/// The number as the library compares it: no spaces, upper case.
fn normalised_number(number: &str) -> String {
    number.split_whitespace().collect::<String>().to_uppercase()
}
fn is_clean(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

/// The plan reduced to what it means: the request id lowercase and
/// hyphenated, the number normalised. The structs serialise in a fixed field
/// order and fill the defaults in, so key order, whitespace and omitted
/// defaults of the file do not change the digest.
fn canonical_digest(plan: &Plan, request: &str) -> Result<String> {
    let mut canonical = plan.clone();
    canonical.request_id = request.to_owned();
    canonical.document_number = canonical.document_number.as_deref().map(normalised_number);
    let mut hasher = Sha256::new();
    hasher.update(b"ponte-plan-digest-v2\n");
    hasher.update(serde_json::to_vec(&canonical)?);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Every rule of the plan that does not need the library. The first problem
/// is reported, naming the JSON field and, for a task, its position (counted
/// from 1).
pub fn check_plan(plan: &Plan) -> Result<Checked> {
    let request = uuid::Uuid::parse_str(plan.request_id.trim())
        .map_err(|_| {
            anyhow!("request_id: informe um UUID válido, como 47f3f62f-404c-4931-ae04-bc7eb1108c07")
        })?
        .hyphenated()
        .to_string();
    match (&plan.document, &plan.document_number) {
        (Some(_), Some(_)) => bail!("Informe só um dos campos: document (cria um registro novo) ou document_number (usa um registro existente), não os dois."),
        (None, None) => bail!("Informe document (cria um registro novo) ou document_number (usa um registro existente)."),
        _ => {}
    }
    if let Some(number) = &plan.document_number {
        ensure!(
            !number.trim().is_empty() && number.len() <= 160 && is_clean(number),
            "document_number: informe o número do documento, como MANUAL-2026/0004 (até 160 caracteres, sem quebras de linha)."
        );
        ensure!(
            !plan.complete_triage,
            "complete_triage: só vale com um registro novo (document). Com document_number o registro já existe e a triagem dele não é alterada; remova complete_triage."
        );
    }
    ensure!(
        !plan.tasks.is_empty(),
        "tasks: o plano não tem nenhuma tarefa; informe de 1 a {MAX_TASKS}."
    );
    ensure!(
        plan.tasks.len() <= MAX_TASKS,
        "tasks: o plano tem {} tarefas; o máximo é {MAX_TASKS} por plano. Divida em planos menores, cada um com o seu request_id.",
        plan.tasks.len()
    );
    let mut record = None;
    if let Some(doc) = &plan.document {
        record = Some(check_document(doc, &request)?);
    }
    let mut tasks = Vec::with_capacity(plan.tasks.len());
    for (index, input) in plan.tasks.iter().enumerate() {
        tasks.push(check_task(input, index, &request)?);
    }
    Ok(Checked {
        digest: canonical_digest(plan, &request)?,
        request,
        record,
        tasks,
    })
}

fn check_document(doc: &PlanDocument, request: &str) -> Result<NewRecord> {
    ensure!(
        KINDS.iter().any(|(key, _)| *key == doc.kind),
        "document.type: valor inválido \"{}\"; use um de: {}",
        doc.kind,
        KINDS.iter().map(|(key, _)| *key).collect::<Vec<_>>().join(", ")
    );
    let title = doc.title.trim();
    ensure!(
        title.chars().count() >= 3 && title.len() <= 1000 && is_clean(title),
        "document.title: informe um título com pelo menos 3 caracteres, até 1000 bytes e sem quebras de linha."
    );
    ensure!(
        doc.origin.len() <= 1000 && is_clean(&doc.origin),
        "document.origin: até 1000 bytes e sem quebras de linha."
    );
    ensure!(
        doc.date.is_empty() || iso_date(&doc.date),
        "document.date: data inválida \"{}\"; use o formato AAAA-MM-DD (por exemplo 2026-11-22), com zeros à esquerda.",
        doc.date
    );
    ensure!(
        doc.text.len() <= MAX_CAPTURE_TEXT_BYTES,
        "document.text: o texto passa de 500 KB; resuma-o ou anexe o arquivo original depois."
    );
    ensure!(
        doc.note.len() <= 500 * 1024,
        "document.note: a nota passa de 500 KB."
    );
    ensure!(
        doc.categories.len() <= crate::categories::MAX_PER_ITEM,
        "document.categories: no máximo {} categorias por documento.",
        crate::categories::MAX_PER_ITEM
    );
    let record = NewRecord {
        fields: ManualFields {
            kind: doc.kind.clone(),
            title: doc.title.clone(),
            origin: doc.origin.clone(),
            document_date: doc.date.clone(),
            text: doc.text.clone(),
        },
        request_id: request.to_owned(),
        ..Default::default()
    };
    // The library's own rules, as a second net.
    validate_record(&record).map_err(|e| anyhow!("document: {e}"))?;
    Ok(record)
}

fn check_task(input: &PlanTask, index: usize, request: &str) -> Result<Task> {
    let bare = format!("tasks[{}]", index + 1);
    let bad = |field: &str, message: String| invalid(format!("{bare}.{field}: {message}"));
    if input.title.trim().is_empty() {
        return Err(bad("title", "obrigatório; informe o título da tarefa.".into()));
    }
    if input.title.len() > 1000 {
        return Err(bad("title", "o título passa de 1000 bytes.".into()));
    }
    if input.notes.len() > 32000 {
        return Err(bad("notes", "as observações passam de 32000 bytes.".into()));
    }
    if input.action.is_empty() || !ACTIONS.contains(&input.action.as_str()) {
        return Err(bad(
            "action",
            format!(
                "{}; use uma de: {}",
                if input.action.is_empty() {
                    "obrigatória".to_owned()
                } else {
                    format!("valor inválido \"{}\"", input.action)
                },
                ACTIONS.join(", ")
            ),
        ));
    }
    if !PRIORITIES.contains(&input.priority.as_str()) {
        return Err(bad(
            "priority",
            format!(
                "valor inválido \"{}\"; use um de: {} (omitida vale normal)",
                input.priority,
                PRIORITIES.join(", ")
            ),
        ));
    }
    if !input.due_date.is_empty() && !iso_date(&input.due_date) {
        return Err(bad(
            "due_date",
            format!(
                "data inválida \"{}\"; use o formato AAAA-MM-DD (por exemplo 2026-12-01), com zeros à esquerda.",
                input.due_date
            ),
        ));
    }
    if input.assignee.len() > 256 || !is_clean(&input.assignee) {
        return Err(bad("assignee", "até 256 bytes e sem quebras de linha.".into()));
    }
    if input.scheduled_start.is_empty() != input.scheduled_end.is_empty() {
        return Err(bad(
            "scheduled_start",
            "informe scheduled_start e scheduled_end juntos, ou nenhum dos dois.".into(),
        ));
    }
    if !input.scheduled_start.is_empty() {
        let start = iso_datetime(&input.scheduled_start).ok_or_else(|| {
            bad(
                "scheduled_start",
                format!("data e hora inválidas \"{}\"; use AAAA-MM-DDTHH:MM (por exemplo 2026-12-01T09:30).", input.scheduled_start),
            )
        })?;
        let end = iso_datetime(&input.scheduled_end).ok_or_else(|| {
            bad(
                "scheduled_end",
                format!("data e hora inválidas \"{}\"; use AAAA-MM-DDTHH:MM (por exemplo 2026-12-01T10:30).", input.scheduled_end),
            )
        })?;
        if end <= start {
            return Err(bad("scheduled_end", "deve ser posterior a scheduled_start.".into()));
        }
    }
    if input.estimate_minutes > 525_600 {
        return Err(bad("estimate_minutes", "no máximo 525600 minutos.".into()));
    }
    if input.checklist.len() > 100 {
        return Err(bad("checklist", "no máximo 100 itens.".into()));
    }
    if let Some(n) = input.checklist.iter().position(|(_, text)| text.len() > 1000) {
        return Err(bad(&format!("checklist[{}]", n + 1), "o item passa de 1000 bytes.".into()));
    }
    if input.categories.len() > crate::categories::MAX_PER_ITEM {
        return Err(bad(
            "categories",
            format!("no máximo {} categorias por tarefa.", crate::categories::MAX_PER_ITEM),
        ));
    }
    let mut task = Task::new(0);
    task.document_id = None;
    task.uid = uid(request, index);
    task.title = input.title.clone();
    task.action = input.action.clone();
    task.notes = input.notes.clone();
    task.due_date = input.due_date.clone();
    task.priority = input.priority.clone();
    task.assignee = input.assignee.clone();
    task.checklist = input.checklist.clone();
    task.scheduled_start = input.scheduled_start.clone();
    task.scheduled_end = input.scheduled_end.clone();
    task.estimate_minutes = input.estimate_minutes;
    // The library's own rules, as a second net.
    task.validate().map_err(|e| invalid(format!("{bare}: {e}")))?;
    Ok(task)
}

// ---------------------------------------------------------------------------
// The answer
// ---------------------------------------------------------------------------

/// Where an item of the plan stands now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    /// It exists and is in use.
    Present,
    /// It is in the Lixeira; it is not recreated or restored.
    Trashed,
    /// It was created before and no longer exists (deleted for good); it is
    /// not recreated.
    Missing,
    /// It has never been created (a dry run, or what a run is about to do).
    ToCreate,
}
impl ItemState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Present => "presente",
            Self::Trashed => "na Lixeira",
            Self::Missing => "não existe mais",
            Self::ToCreate => "ainda não criado",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentOutcome {
    pub state: ItemState,
    pub state_label: String,
    /// The record already existed (`document_number`), not made by the plan.
    pub existing: bool,
    pub id: Option<i64>,
    pub number: Option<String>,
    pub title: Option<String>,
    pub created_now: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskOutcome {
    /// Position in the plan, counted from 1.
    pub index: usize,
    /// The identity that survives synchronisation.
    pub uid: String,
    pub state: ItemState,
    pub state_label: String,
    pub id: Option<i64>,
    pub title: String,
    pub created_now: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanResult {
    pub ok: bool,
    pub request_id: String,
    pub dry_run: bool,
    /// Something was written by THIS run (never true for a dry run or for a
    /// complete plan that was only reported).
    pub created: bool,
    /// The library follows a team (shared folder or main computer): ids and
    /// the number are the ones of this copy and may change after the next
    /// synchronisation. `request_id` (for the record) and each task's `uid`
    /// are the stable identities.
    pub ids_provisional: bool,
    pub document: DocumentOutcome,
    pub tasks: Vec<TaskOutcome>,
    /// `nao_solicitada`, `sera_concluida` (dry run or pending),
    /// `concluida_agora`, `ja_concluida_antes` (an earlier run did it; the
    /// task is not touched again) or `nao_encontrada` (no open triage).
    pub triage: String,
}

// ---------------------------------------------------------------------------
// The library
// ---------------------------------------------------------------------------

struct PlanRun {
    digest: String,
    document_ready: bool,
    tasks_created: usize,
    triage_done: bool,
}
struct DocRow {
    id: i64,
    trashed: bool,
    capture: Capture,
}
struct TaskRow {
    id: i64,
    document_id: Option<i64>,
    trashed: bool,
    title: String,
}
/// A name or uid of a category or project resolved against the library.
struct Entry {
    uid: String,
    name: String,
    archived: bool,
}

impl Store {
    fn plan_run(&self, request: &str) -> Result<Option<PlanRun>> {
        let table: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='plan_import_runs')",
            [],
            |r| r.get(0),
        )?;
        if !table {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT digest,document_ready,tasks_created,triage_done FROM plan_import_runs WHERE request_id=?1",
                [request],
                |r| {
                    Ok(PlanRun {
                        digest: r.get(0)?,
                        document_ready: r.get::<_, i64>(1)? != 0,
                        tasks_created: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                        triage_done: r.get::<_, i64>(3)? != 0,
                    })
                },
            )
            .optional()?)
    }
    /// Writes the receipt, once, AFTER the first thing was written: a plan
    /// that failed before that leaves nothing behind and can be corrected
    /// and run again with the same request id.
    fn start_plan_run(&mut self, request: &str, digest: &str, document_ready: bool) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS plan_import_runs(
                request_id TEXT PRIMARY KEY,
                digest TEXT NOT NULL,
                document_ready INTEGER NOT NULL DEFAULT 0,
                tasks_created INTEGER NOT NULL DEFAULT 0,
                triage_done INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            )",
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO plan_import_runs(request_id,digest,document_ready,created_at) VALUES(?1,?2,?3,?4)",
            rusqlite::params![
                request,
                digest,
                i64::from(document_ready),
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        let stored: String = tx.query_row(
            "SELECT digest FROM plan_import_runs WHERE request_id=?1",
            [request],
            |r| r.get(0),
        )?;
        ensure!(stored == digest, CHANGED_PLAN);
        tx.commit()?;
        Ok(())
    }
    fn note_plan_progress(&self, request: &str, column: &str, value: i64) -> Result<()> {
        // `column` is one of ours, never from the file.
        self.conn.execute(
            &format!("UPDATE plan_import_runs SET {column}=MAX({column},?2) WHERE request_id=?1"),
            rusqlite::params![request, value],
        )?;
        Ok(())
    }

    fn doc_row(&self, id: i64) -> Result<Option<DocRow>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT c.payload FROM documents d JOIN captures c ON c.id=d.latest_capture_id AND c.document_id=d.id WHERE d.id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(blob) = blob else {
            return Ok(None);
        };
        let capture: Capture = serde_json::from_slice(&crate::protection::unseal(&blob)?)
            .context("O conteúdo do documento está danificado ou em formato incompatível.")?;
        Ok(Some(DocRow {
            id,
            trashed: crate::trash::contains(&self.conn, Kind::Document, id)?,
            capture,
        }))
    }
    fn task_row(&self, uid: &str) -> Result<Option<TaskRow>> {
        let row: Option<(i64, Option<i64>, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT id,document_id,payload FROM tasks WHERE uid=?1 ORDER BY id LIMIT 1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((id, document_id, blob)) = row else {
            return Ok(None);
        };
        let task: Task = serde_json::from_slice(&crate::protection::unseal(&blob)?)
            .context("O conteúdo da tarefa está danificado ou em formato incompatível.")?;
        let trashed = crate::trash::contains(&self.conn, Kind::Task, id)?
            || match document_id {
                Some(document) => crate::trash::contains(&self.conn, Kind::Document, document)?,
                None => false,
            };
        Ok(Some(TaskRow {
            id,
            document_id,
            trashed,
            title: task.title,
        }))
    }
    /// The document a request created, through `requests` (which travels in
    /// the snapshots, unlike the local id).
    fn document_of_request(&self, request: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT document_id FROM requests WHERE request_id=?1",
                [request],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Categories (or projects) by name, accents and case ignored; a uid is
    /// always accepted (archived or not), a name only for the ones not
    /// archived.
    fn resolve_entry(
        value: &str,
        entries: &[Entry],
        field: &str,
        what: &str,
    ) -> Result<String> {
        let wanted = value.trim();
        if wanted.is_empty() {
            return Err(invalid(format!(
                "{field}: informe o nome ou o identificador (uid) da {what}."
            )));
        }
        let uid = wanted.to_lowercase();
        if let Some(found) = entries.iter().find(|e| e.uid == uid) {
            return Ok(found.uid.clone());
        }
        let key = crate::labels::fold(wanted);
        let same: Vec<&Entry> = entries
            .iter()
            .filter(|e| crate::labels::fold(&e.name) == key)
            .collect();
        let active: Vec<&Entry> = same.iter().copied().filter(|e| !e.archived).collect();
        match active.len() {
            1 => Ok(active[0].uid.clone()),
            0 if same.is_empty() => Err(invalid(format!(
                "{field}: {what} desconhecida \"{wanted}\" (a busca ignora acentos e maiúsculas); confira o nome na lista do Ponte ou informe o uid."
            ))),
            0 => Err(invalid(format!(
                "{field}: a {what} \"{wanted}\" está arquivada e não é oferecida para itens novos; para usá-la mesmo assim, informe o uid dela ({}).",
                same[0].uid
            ))),
            n => Err(invalid(format!(
                "{field}: {n} {what}s se chamam \"{wanted}\"; informe o uid da escolhida ({}).",
                active.iter().map(|e| e.uid.as_str()).collect::<Vec<_>>().join(", ")
            ))),
        }
    }
    fn resolve_all(
        values: &[String],
        entries: &[Entry],
        field: &str,
        what: &str,
    ) -> Result<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let uid =
                Self::resolve_entry(value, entries, &format!("{field}[{}]", index + 1), what)?;
            if out.contains(&uid) {
                return Err(invalid(format!(
                    "{field}[{}]: a {what} \"{}\" aparece mais de uma vez.",
                    index + 1,
                    value.trim()
                )));
            }
            out.push(uid);
        }
        Ok(out)
    }
    /// The categories and the project the pending parts of the plan name.
    /// Only what is still to be created is resolved: a finished plan never
    /// fails because the person renamed a category since.
    fn resolve_references(
        &self,
        plan: &Plan,
        tasks: &mut [Task],
        pending: &[usize],
        document_too: bool,
    ) -> Result<Vec<String>> {
        let categories: Vec<Entry> = self
            .list_categories()?
            .into_iter()
            .map(|c| Entry {
                uid: c.uid,
                name: c.name,
                archived: c.archived,
            })
            .collect();
        let mut document_categories = Vec::new();
        if let (true, Some(doc)) = (document_too, &plan.document) {
            document_categories =
                Self::resolve_all(&doc.categories, &categories, "document.categories", "categoria")?;
        }
        let projects: Vec<Entry> = self
            .list_projects()?
            .into_iter()
            .map(|p| Entry {
                archived: p.status == crate::projects::ProjectStatus::Arquivado,
                uid: p.uid,
                name: p.name,
            })
            .collect();
        for &index in pending {
            let input = &plan.tasks[index];
            let path = format!("tasks[{}]", index + 1);
            tasks[index].categories = Self::resolve_all(
                &input.categories,
                &categories,
                &format!("{path}.categories"),
                "categoria",
            )?;
            if !input.project.is_empty() {
                tasks[index].project_uid = Self::resolve_entry(
                    &input.project,
                    &projects,
                    &format!("{path}.project"),
                    "projeto",
                )?;
            }
            crate::categories::check_item_categories(&tasks[index].categories)
                .map_err(|e| invalid(format!("{path}.categories: {e}")))?;
        }
        Ok(document_categories)
    }

    /// Where the document of the plan stands, from stable identities only.
    fn look_at_document(
        &self,
        plan: &Plan,
        checked: &Checked,
        run: Option<&PlanRun>,
        tasks: &[(Option<TaskRow>, ItemState)],
    ) -> Result<(ItemState, Option<DocRow>)> {
        let found = if plan.document.is_some() {
            self.document_of_request(&checked.request)?
                .map(|id| self.doc_row(id))
                .transpose()?
                .flatten()
        } else {
            let number = plan.document_number.as_deref().unwrap_or_default();
            match self.document_by_number_any(number)? {
                Some((id, _)) => self.doc_row(id)?,
                // The number may have changed with a synchronisation: the
                // tasks of this plan know the document they hang from.
                None => match tasks
                    .iter()
                    .filter_map(|(row, _)| row.as_ref().and_then(|r| r.document_id))
                    .next()
                {
                    Some(id) if run.is_some() => self.doc_row(id)?,
                    _ => None,
                },
            }
        };
        Ok(match found {
            Some(row) if row.trashed => (ItemState::Trashed, Some(row)),
            Some(row) => (ItemState::Present, Some(row)),
            None if run.is_some() => (ItemState::Missing, None),
            None => (ItemState::ToCreate, None),
        })
    }
    fn look_at_tasks(
        &self,
        checked: &Checked,
        run: Option<&PlanRun>,
    ) -> Result<Vec<(Option<TaskRow>, ItemState)>> {
        let done = run.map_or(0, |r| r.tasks_created);
        checked
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| {
                let row = self.task_row(&task.uid)?;
                let state = match &row {
                    Some(r) if r.trashed => ItemState::Trashed,
                    Some(_) => ItemState::Present,
                    None if index < done => ItemState::Missing,
                    None => ItemState::ToCreate,
                };
                Ok((row, state))
            })
            .collect()
    }

    /// The current state of everything the plan names, plus whatever the
    /// triage did in this run.
    #[allow(clippy::too_many_arguments)]
    fn plan_outcome(
        &self,
        plan: &Plan,
        checked: &Checked,
        dry_run: bool,
        created_now: &Written,
        triage: &str,
    ) -> Result<PlanResult> {
        let run = self.plan_run(&checked.request)?;
        let task_rows = self.look_at_tasks(checked, run.as_ref())?;
        let (doc_state, doc_row) = self.look_at_document(plan, checked, run.as_ref(), &task_rows)?;
        let document = DocumentOutcome {
            state: doc_state,
            state_label: doc_state.label().to_owned(),
            existing: plan.document_number.is_some(),
            id: doc_row.as_ref().map(|d| d.id),
            number: doc_row.as_ref().map(|d| d.capture.document_number.clone()),
            title: doc_row
                .as_ref()
                .map(|d| d.capture.title.clone())
                .or_else(|| plan.document.as_ref().map(|d| d.title.trim().to_owned())),
            created_now: created_now.document,
        };
        let tasks = task_rows
            .into_iter()
            .enumerate()
            .map(|(index, (row, state))| TaskOutcome {
                index: index + 1,
                uid: checked.tasks[index].uid.clone(),
                state,
                state_label: state.label().to_owned(),
                id: row.as_ref().map(|r| r.id),
                title: row
                    .map(|r| r.title)
                    .unwrap_or_else(|| checked.tasks[index].title.clone()),
                created_now: created_now.tasks.contains(&index),
            })
            .collect();
        Ok(PlanResult {
            ok: true,
            request_id: checked.request.clone(),
            dry_run,
            created: created_now.any(),
            ids_provisional: self.client_config()?.is_some(),
            document,
            tasks,
            triage: triage.to_owned(),
        })
    }

    /// Runs a plan: validates it, and creates what has never been created.
    ///
    /// The order is: the checks that need no library; the receipt of an
    /// earlier run (a finished plan is only reported, with the current state
    /// of each item); the references by name (only for what is still to be
    /// created); then the writes. Nothing is written for an invalid plan or
    /// a dry run.
    ///
    /// Atomicity, by mode (see `CONTRIBUINDO.md`): on a library used alone or
    /// on the main computer the tasks are one transaction (all or none); in
    /// shared-folder mode and on a computer connected to a main computer each
    /// task is its own change (queued or sent, like the screens do), so an
    /// interruption can leave some of them, and the same command finishes
    /// the job.
    pub fn import_plan(&mut self, plan: &Plan, dry_run: bool) -> Result<PlanResult> {
        let checked = check_plan(plan)?;
        let run = self.plan_run(&checked.request)?;
        if let Some(run) = &run {
            ensure!(run.digest == checked.digest, CHANGED_PLAN);
        }
        let task_rows = self.look_at_tasks(&checked, run.as_ref())?;
        let (doc_state, doc_row) =
            self.look_at_document(plan, &checked, run.as_ref(), &task_rows)?;
        let new_document = plan.document.is_some();

        // A request id that the library already knows without a receipt is
        // an interrupted run of this plan, or someone else's id.
        if let (None, Some(row), Some(doc)) = (&run, &doc_row, &plan.document) {
            let kind = row.capture.metadata.get("manual_kind").map(String::as_str);
            ensure!(
                row.capture.title == doc.title.trim()
                    && row.capture.text == doc.text.trim_end()
                    && kind == Some(doc.kind.as_str()),
                "request_id: este identificador já foi usado, neste acervo, para outro registro ({}). Use um request_id novo para este plano.",
                row.capture.document_number
            );
        }
        if run.is_none() && !new_document {
            let number = plan.document_number.as_deref().unwrap_or_default().trim();
            match doc_state {
                ItemState::Present => {}
                ItemState::Trashed => bail!(
                    "document_number: o documento {number} está na Lixeira; restaure-o antes de criar tarefas nele."
                ),
                _ => bail!(
                    "document_number: nenhum documento com o número {number} neste acervo (a busca ignora maiúsculas, minúsculas e espaços). Confira o número na ficha do documento."
                ),
            }
        }

        let done = run.as_ref().map_or(0, |r| r.tasks_created);
        let pending_tasks: Vec<usize> = task_rows
            .iter()
            .enumerate()
            .filter(|(index, (_, state))| {
                *state == ItemState::ToCreate && (run.is_none() || *index >= done)
            })
            .map(|(index, _)| index)
            .collect();
        let document_pending = new_document
            && (doc_state == ItemState::ToCreate
                || run.as_ref().is_some_and(|r| !r.document_ready));
        let triage_pending =
            new_document && plan.complete_triage && !run.as_ref().is_some_and(|r| r.triage_done);
        let nothing_to_do = !document_pending && pending_tasks.is_empty() && !triage_pending;

        if nothing_to_do {
            let triage = triage_label(plan, new_document, true);
            return self.plan_outcome(plan, &checked, dry_run, &Written::default(), triage);
        }
        // Something is left to create: the record has to be there to hold it.
        if run.is_some() {
            match doc_state {
                ItemState::Trashed => bail!(
                    "O plano ainda tem o que criar, mas o documento dele está na Lixeira. Restaure-o (ou conclua o que falta à mão) e repita o comando; nada é criado numa Lixeira."
                ),
                ItemState::Missing => bail!(
                    "O plano ainda tinha o que criar, mas o documento dele não existe mais (foi excluído definitivamente) e não é recriado. Use um request_id novo para um plano novo."
                ),
                _ => {}
            }
        }

        let mut tasks = checked.tasks.clone();
        let document_categories = self.resolve_references(
            plan,
            &mut tasks,
            &pending_tasks,
            document_pending,
        )?;
        if dry_run {
            let triage = triage_label(plan, new_document, false);
            return self.plan_outcome(plan, &checked, true, &Written::default(), triage);
        }

        // ---- writes ------------------------------------------------------
        let mut written = Written::default();
        let mut receipt_exists = run.is_some();
        let document_id = if new_document {
            let record = checked.record.as_ref().expect("a plan with document has its record");
            let id = if doc_state == ItemState::ToCreate {
                let created = self
                    .create_record_with_attachments(record, &[])
                    .map_err(|e| anyhow!("document: não foi possível criar o registro: {e}"))?;
                match created {
                    Created::New { document_id, .. } => {
                        written.document = true;
                        document_id
                    }
                    Created::Already { document_id } => document_id,
                }
            } else {
                doc_row.as_ref().expect("a present document has its row").id
            };
            if !receipt_exists {
                self.start_plan_run(&checked.request, &checked.digest, false)?;
                receipt_exists = true;
            }
            if run.as_ref().is_none_or(|r| !r.document_ready) {
                // Only into what is still empty: never over what the person
                // wrote since an interrupted run.
                let doc = plan.document.as_ref().expect("new document");
                if !doc.note.is_empty() && self.load_document(id)?.note.is_empty() {
                    self.save_note(id, &doc.note)?;
                    written.document_content = true;
                }
                if !document_categories.is_empty() && self.document_categories(id)?.is_empty() {
                    self.set_document_categories(id, &document_categories)?;
                    written.document_content = true;
                }
                self.note_plan_progress(&checked.request, "document_ready", 1)?;
            }
            id
        } else {
            doc_row.as_ref().expect("an existing document was checked").id
        };

        if !pending_tasks.is_empty() {
            let mut to_write: Vec<(usize, Task)> = pending_tasks
                .iter()
                .map(|&index| {
                    let mut task = tasks[index].clone();
                    task.document_id = Some(document_id);
                    (index, task)
                })
                .collect();
            if self.client_config()?.is_none() {
                // One transaction for all of them: every task or none.
                let batch: Vec<Task> = to_write.iter().map(|(_, task)| task.clone()).collect();
                self.save_repeated_tasks_counted(&batch).map_err(|e| {
                    anyhow!(
                        "Nenhuma tarefa foi gravada (a gravação é tudo ou nada neste modo): {e}"
                    )
                })?;
                written.tasks.extend(to_write.iter().map(|(index, _)| *index));
                if !receipt_exists {
                    self.start_plan_run(&checked.request, &checked.digest, true)?;
                }
            } else {
                for (index, task) in to_write.drain(..) {
                    self.save_task(&task).map_err(|e| {
                        invalid(format!(
                            "tasks[{}]: não foi possível gravar a tarefa: {e}. As tarefas anteriores já foram gravadas; repita o mesmo comando para continuar de onde parou, sem duplicar nada.",
                            index + 1
                        ))
                    })?;
                    written.tasks.push(index);
                    if !receipt_exists {
                        self.start_plan_run(&checked.request, &checked.digest, !new_document)?;
                        receipt_exists = true;
                    }
                    self.note_plan_progress(&checked.request, "tasks_created", index as i64 + 1)?;
                }
            }
            self.note_plan_progress(&checked.request, "tasks_created", checked.tasks.len() as i64)?;
        } else if receipt_exists {
            self.note_plan_progress(&checked.request, "tasks_created", checked.tasks.len() as i64)?;
        }

        // The triage is completed last, when the tasks exist: a record whose
        // only task is concluded would look finished to the team meanwhile.
        let mut triage = triage_label(plan, new_document, true);
        if triage_pending {
            triage = self.complete_automatic_triage(document_id, &checked)?;
            if triage == "concluida_agora" {
                written.document_content = true;
            }
            self.note_plan_progress(&checked.request, "triage_done", 1)?;
        }
        self.plan_outcome(plan, &checked, false, &written, triage)
    }

    /// Concludes the triage task the record was born with: the open task of
    /// the document with the action "triar" that has the lowest id and is not
    /// one of the plan's own.
    fn complete_automatic_triage(
        &mut self,
        document_id: i64,
        checked: &Checked,
    ) -> Result<&'static str> {
        let own: BTreeSet<i64> = checked
            .tasks
            .iter()
            .filter_map(|t| self.task_row(&t.uid).ok().flatten().map(|r| r.id))
            .collect();
        let Some(id) = Store::triage_task_in(&self.conn, document_id)? else {
            return Ok("nao_encontrada");
        };
        if own.contains(&id) {
            return Ok("nao_encontrada");
        }
        let Some(task) = self.list_tasks()?.into_iter().find(|t| t.id == id) else {
            return Ok("nao_encontrada");
        };
        if task.is_done() {
            return Ok("ja_concluida_antes");
        }
        self.complete_task(id)?;
        Ok("concluida_agora")
    }
}

const CHANGED_PLAN: &str = "request_id: este identificador já foi usado por um plano diferente (o conteúdo do arquivo mudou desde a primeira execução). Para repetir o plano use o arquivo original; para um plano novo use um request_id novo.";

#[derive(Default)]
struct Written {
    document: bool,
    document_content: bool,
    tasks: Vec<usize>,
}
impl Written {
    fn any(&self) -> bool {
        self.document || self.document_content || !self.tasks.is_empty()
    }
}
fn triage_label(plan: &Plan, new_document: bool, settled: bool) -> &'static str {
    if !new_document || !plan.complete_triage {
        "nao_solicitada"
    } else if settled {
        "ja_concluida_antes"
    } else {
        "sera_concluida"
    }
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

/// What follows `--import-plan` on the command line.
#[derive(Debug, PartialEq, Eq)]
pub struct CliOptions {
    pub file: String,
    pub dry_run: bool,
    /// Where to write the result (a NEW file): the release build has no
    /// console, so a script reads it from here.
    pub output: Option<String>,
}
pub const USAGE: &str = "--import-plan ARQUIVO.json [--dry-run] [--saida RESULTADO.json]";

/// The options may come in any order; the file is the one argument that is
/// not an option.
pub fn parse_cli(args: &[String]) -> Result<CliOptions> {
    let mut file = None;
    let mut dry_run = false;
    let mut output = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--dry-run" => {
                ensure!(!dry_run, "--dry-run foi informado duas vezes. Uso: {USAGE}");
                dry_run = true;
            }
            "--saida" => {
                ensure!(output.is_none(), "--saida foi informado duas vezes. Uso: {USAGE}");
                match rest.next() {
                    Some(value) if !value.starts_with("--") => output = Some(value.clone()),
                    _ => bail!("Informe o arquivo depois de --saida. Uso: {USAGE}"),
                }
            }
            other if other.starts_with("--") => {
                bail!("Opção desconhecida: {other}. Uso: {USAGE}")
            }
            other => {
                ensure!(
                    file.is_none(),
                    "Informe um só arquivo de plano (recebi também {other}). Uso: {USAGE}"
                );
                file = Some(other.to_owned());
            }
        }
    }
    let file = file.ok_or_else(|| anyhow!("Informe o nome do arquivo do plano. Uso: {USAGE}"))?;
    Ok(CliOptions {
        file,
        dry_run,
        output,
    })
}

/// A busy library (the window is writing) in words, not as `database is locked`.
fn friendly(error: anyhow::Error) -> anyhow::Error {
    let busy = error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(failure, _))
                if matches!(
                    failure.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                )
        )
    });
    if busy {
        anyhow!("O acervo está ocupado (o Ponte provavelmente está gravando). Tente de novo em alguns segundos; repetir o mesmo comando é seguro e continua de onde parou, sem duplicar nada.")
    } else {
        error
    }
}

fn read_limited(file: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .with_context(|| format!("Não foi possível abrir o arquivo do plano: {file}"))?
        .take(crate::native::MAX_FRAME as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("Não foi possível ler o arquivo do plano: {file}"))?;
    ensure!(
        bytes.len() <= crate::native::MAX_FRAME,
        "O arquivo do plano passa de {} MB.",
        crate::native::MAX_FRAME / (1024 * 1024)
    );
    Ok(bytes)
}

/// `--saida` only names a file that does not exist yet (so it can never be
/// the library or anything else of the person), in a folder that does.
fn check_output(path: &str) -> Result<()> {
    let path = Path::new(path);
    ensure!(
        !path.exists(),
        "O arquivo de saída {} já existe; informe um arquivo novo (ele nunca é sobrescrito).",
        path.display()
    );
    let folder = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure!(
        folder.is_dir(),
        "A pasta do arquivo de saída ({}) não existe.",
        folder.display()
    );
    Ok(())
}

/// A dry run must not change the library: it refuses a library that the
/// opening would have to migrate (the window does that), and one that does
/// not exist yet.
fn check_readable_as_is(db: &Path) -> Result<()> {
    ensure!(
        db.exists(),
        "Ainda não há acervo neste computador (abra o Ponte uma vez); sem ele, --dry-run não tem o que consultar."
    );
    let conn = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| friendly(e.into()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| friendly(e.into()))?;
    ensure!(
        version >= crate::storage::SCHEMA_VERSION,
        "O acervo é de uma versão mais antiga do Ponte; abra o Ponte uma vez para atualizá-lo antes de usar --dry-run (a simulação não altera nada)."
    );
    Ok(())
}

/// The command: reads the file, validates it, runs the plan on the library
/// at `db` and writes the result as JSON to `stdout` (and to `--saida`). On
/// failure the error is returned (the caller prints it to the error output
/// and exits 1) and, when `--saida` was given, written there too as
/// `{"ok":false,"erro":"..."}`.
pub fn execute(options: &CliOptions, db: &Path, stdout: &mut dyn Write) -> Result<()> {
    if let Some(output) = &options.output {
        check_output(output)?;
    }
    let outcome = (|| -> Result<PlanResult> {
        let plan = parse_plan(&read_limited(&options.file)?)?;
        // Everything that needs no library is settled before it is opened.
        check_plan(&plan)?;
        if options.dry_run {
            check_readable_as_is(db)?;
        }
        let mut store = Store::open(db).map_err(friendly)?;
        store.import_plan(&plan, options.dry_run).map_err(friendly)
    })();
    let json = match &outcome {
        Ok(result) => serde_json::to_string(result)?,
        Err(error) => serde_json::to_string(&serde_json::json!({
            "ok": false,
            "erro": format!("{error:#}"),
        }))?,
    };
    if let Some(output) = &options.output {
        crate::atomic_file::write_atomic(Path::new(output), json.as_bytes()).with_context(
            || format!("Não foi possível gravar o resultado em {output}"),
        )?;
    }
    if outcome.is_ok() {
        // A console that is not there (the release build) is not a failure of
        // a plan that was already written.
        let _ = writeln!(stdout, "{json}");
    }
    outcome.map(|_| ())
}

/// `ponteproj.exe --import-plan ...` (the arguments after the command).
pub fn run_cli(args: &[String]) -> Result<()> {
    let options = parse_cli(args)?;
    let db = crate::paths::db_path_in(&crate::paths::data_dir_path()?);
    execute(&options, &db, &mut std::io::stdout())
}
