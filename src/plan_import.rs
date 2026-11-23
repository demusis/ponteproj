//! `--import-plan`: a manual record (or an existing document) plus 1 to 100
//! linked tasks, created from a JSON file, resumable and idempotent by
//! `request_id`.
//!
//! What the module keeps, and why:
//!
//! - Nothing is written until the file has been read, parsed and checked
//!   without the library (`parse_plan`, `check_plan`), and a dry run writes
//!   nothing at all, not even the receipt table.
//! - The receipt (`plan_import_log`, a LOCAL table: it is not part of the
//!   synchronised snapshot) is RESERVED before the first write, in a
//!   transaction of its own, with the request id UNIQUE. It holds only what
//!   survives a synchronisation: the request id, a digest of the canonical
//!   plan, who holds the reservation and how far the run got (step markers
//!   written before and after each step). The record is found through
//!   `requests` (request id -> document, which travels in snapshots) and the
//!   tasks through their uids (derived from the request id and the position
//!   in the plan). Local ids, the manual number and the titles are read again
//!   on every run.
//! - A rerun creates what was not created yet and, normally, leaves alone
//!   what the person deleted or edited. It is NOT a guarantee in every
//!   window (see "Limites" in `CONTRIBUINDO.md`): the plan is never one
//!   transaction, and a task purged for good between its write and its
//!   progress mark is created again.
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
const COUNT_NOTE: &str =
    " (nas mensagens, tasks[1] é a primeira tarefa da lista: a contagem começa em 1)";
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
        return String::from_utf8(rest.to_vec()).map_err(|_| {
            anyhow!("O arquivo tem a marca UTF-8 mas traz bytes que não são UTF-8. {HOW}")
        });
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let little = bytes[0] == 0xFF;
        let body = &bytes[2..];
        ensure!(
            body.len().is_multiple_of(2),
            "O arquivo UTF-16 está truncado (número ímpar de bytes). {HOW}"
        );
        let units: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
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
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| anyhow!("JSON inválido: {}", syntax_message(&e)))?;
    match serde_json::from_str::<Plan>(&text) {
        Ok(plan) => Ok(plan),
        Err(error) => {
            let place = format!("linha {}, coluna {}", error.line(), error.column());
            let detail = shape_problem(&value).unwrap_or_else(|| serde_message(&error.to_string()));
            Err(invalid(format!("Plano inválido: {detail} ({place})")))
        }
    }
}

fn without_position(message: &str) -> &str {
    message
        .rsplit_once(" at line ")
        .map_or(message, |(head, _)| head)
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
        "sequência de escape inválida dentro de um texto (para uma barra invertida escreva \\\\)"
            .to_owned()
    } else if head.contains("expected ident") {
        "palavra inválida (use true, false ou null, sem aspas)".to_owned()
    } else if head.contains("number") {
        "número inválido".to_owned()
    } else if head.contains("unicode") || head.contains("surrogate") || head.contains("hex escape")
    {
        "escape \\u inválido dentro de um texto".to_owned()
    } else {
        format!("erro de sintaxe ({head})")
    };
    format!(
        "{said}, na linha {}, coluna {}",
        error.line(),
        error.column()
    )
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
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map(|key| {
            format!(
            "{path}: campo desconhecido \"{key}\" (confira a grafia); os campos aceitos são: {}",
            allowed.join(", ")
        )
        })
}
fn wrong_type(path: &str, expected: &str, found: &Value) -> String {
    format!(
        "{path}: esperava {expected}, mas encontrei {}",
        json_kind(found)
    )
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
        return Some(wrong_type(
            &format!("{path}.{key}"),
            "uma lista de textos",
            value,
        ));
    };
    items.iter().enumerate().find_map(|(index, item)| {
        (!item.is_string()).then(|| {
            wrong_type(
                &format!("{path}.{key}[{}]", index + 1),
                "um texto entre aspas",
                item,
            )
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
        None | Some(Value::Null) => return Some(
            "request_id: campo obrigatório (um UUID, como 47f3f62f-404c-4931-ae04-bc7eb1108c07)"
                .into(),
        ),
        Some(value) if !value.is_string() => {
            return Some(wrong_type(
                "request_id",
                "um texto entre aspas (um UUID)",
                value,
            ))
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
            return Some(wrong_type(
                "complete_triage",
                "true ou false (sem aspas)",
                value,
            ))
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
        None | Some(Value::Null) => {
            return Some("tasks: campo obrigatório (uma lista com 1 a 100 tarefas)".into())
        }
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
                return Some(wrong_type(
                    &format!("{path}.checklist"),
                    "uma lista de pares [concluído, \"texto\"]",
                    value,
                ));
            };
            for (n, item) in items.iter().enumerate() {
                let good = item.as_array().is_some_and(|pair| {
                    pair.len() == 2 && pair[0].is_boolean() && pair[1].is_string()
                });
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
        KINDS
            .iter()
            .map(|(key, _)| *key)
            .collect::<Vec<_>>()
            .join(", ")
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
        return Err(bad(
            "title",
            "obrigatório; informe o título da tarefa.".into(),
        ));
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
        return Err(bad(
            "assignee",
            "até 256 bytes e sem quebras de linha.".into(),
        ));
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
            return Err(bad(
                "scheduled_end",
                "deve ser posterior a scheduled_start.".into(),
            ));
        }
    }
    if input.estimate_minutes > 525_600 {
        return Err(bad("estimate_minutes", "no máximo 525600 minutos.".into()));
    }
    if input.checklist.len() > 100 {
        return Err(bad("checklist", "no máximo 100 itens.".into()));
    }
    if let Some(n) = input
        .checklist
        .iter()
        .position(|(_, text)| text.len() > 1000)
    {
        return Err(bad(
            &format!("checklist[{}]", n + 1),
            "o item passa de 1000 bytes.".into(),
        ));
    }
    if input.categories.len() > crate::categories::MAX_PER_ITEM {
        return Err(bad(
            "categories",
            format!(
                "no máximo {} categorias por tarefa.",
                crate::categories::MAX_PER_ITEM
            ),
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
    task.validate()
        .map_err(|e| invalid(format!("{bare}: {e}")))?;
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
    /// plan that was only reported).
    pub created: bool,
    /// This computer holds no receipt of the plan, but the library already
    /// shows its effects: it was applied on another computer of the same
    /// shared library. Nothing was written here.
    pub applied_elsewhere: bool,
    /// The library follows a team (shared folder or main computer): ids and
    /// the number are the ones of this copy and may change after the next
    /// synchronisation. `request_id` (for the record) and each task's `uid`
    /// are the stable identities.
    pub ids_provisional: bool,
    pub document: DocumentOutcome,
    pub tasks: Vec<TaskOutcome>,
    /// The note of the record: `nao_solicitada`, `sera_feita`, `feita`,
    /// `interrompida_nao_repetida` (started by a run that was interrupted: it
    /// is not written again) or `nao_verificavel` (applied elsewhere).
    pub note: String,
    /// The categories of the record; same values as `note`.
    pub categories: String,
    /// `nao_solicitada`, `sera_concluida` (dry run or pending),
    /// `concluida_agora`, `ja_concluida_antes`, `nao_encontrada` (the
    /// automatic triage of the record was not found open: nothing was
    /// concluded), `interrompida_nao_repetida` or `nao_verificavel`.
    pub triage: String,
    /// One sentence for each step (note, categories, triage) that was started
    /// by a run that was interrupted: it is not performed again, and the
    /// person must check it and finish it by hand.
    pub pendencias_manuais: Vec<String>,
}

// ---------------------------------------------------------------------------
// The library
// ---------------------------------------------------------------------------

/// What the person has to check by hand: a step whose "started" mark is
/// there may or may not have had its effect, and it is never repeated.
fn manual_steps(plan: &Plan, run: Option<&PlanRun>, elsewhere: bool) -> Vec<String> {
    let Some(run) = run.filter(|_| !elsewhere) else {
        return Vec::new();
    };
    let mut steps = Vec::new();
    let doc = plan.document.as_ref();
    if run.note_step == STEP_STARTED && doc.is_some_and(|d| !d.note.is_empty()) {
        steps.push("A nota do registro pode não ter sido gravada: confira na ficha do documento e, se faltar, cole o texto de document.note do plano.".to_owned());
    }
    if run.categories_step == STEP_STARTED && doc.is_some_and(|d| !d.categories.is_empty()) {
        steps.push("As categorias do registro podem não ter sido aplicadas: confira na ficha do documento e, se faltarem, marque as de document.categories do plano.".to_owned());
    }
    if run.triage_step == STEP_STARTED && plan.complete_triage {
        steps.push("A conclusão da triagem automática pode não ter sido feita: confira a tarefa \"Triar documento\" do registro e, se ainda estiver aberta, conclua-a.".to_owned());
    }
    steps
}

const CHANGED_PLAN: &str = "request_id: este identificador já foi usado por um plano diferente (o conteúdo do arquivo mudou desde a primeira execução). Para repetir o plano use o arquivo original; para um plano novo use um request_id novo.";
const PLAN_TABLE: &str = "CREATE TABLE IF NOT EXISTS plan_import_log(
                request_id TEXT PRIMARY KEY,
                digest TEXT NOT NULL,
                doc_created INTEGER NOT NULL DEFAULT 0,
                tasks_created INTEGER NOT NULL DEFAULT 0,
                note_step INTEGER NOT NULL DEFAULT 0,
                categories_step INTEGER NOT NULL DEFAULT 0,
                triage_step INTEGER NOT NULL DEFAULT 0,
                triage_uid TEXT NOT NULL DEFAULT '',
                target TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            )";

/// The steps of the record that are written once: 0 not started, 1 started,
/// 2 done. A step that was started is never performed again, whatever the
/// person did since (the note or the categories may have been removed on
/// purpose).
const STEP_STARTED: i64 = 1;
const STEP_DONE: i64 = 2;
/// The triage ended without concluding anything (the automatic task was not
/// found open).
const STEP_NOT_FOUND: i64 = 3;

/// The local receipt of a plan.
#[derive(Clone)]
struct PlanRun {
    digest: String,
    doc_created: bool,
    tasks_created: usize,
    note_step: i64,
    categories_step: i64,
    triage_step: i64,
    triage_uid: String,
    /// Stable identity of the document a `document_number` plan targets.
    target: String,
}
struct DocRow {
    id: i64,
    trashed: bool,
    capture: Capture,
}
struct TaskRow {
    id: i64,
    trashed: bool,
    title: String,
    /// The document the task hangs from.
    document_id: Option<i64>,
}
/// A name or uid of a category or project resolved against the library.
struct Entry {
    uid: String,
    name: String,
    archived: bool,
}
/// What is left to do for a plan, and where each item stands.
struct State {
    tasks: Vec<(Option<TaskRow>, ItemState)>,
    doc_state: ItemState,
    doc: Option<DocRow>,
    /// The document the NUMBER of the file names now (`document_number`).
    number_doc: Option<(i64, bool)>,
    pending_tasks: Vec<usize>,
    doc_pending: bool,
    note_pending: bool,
    categories_pending: bool,
    triage_pending: bool,
}
impl State {
    fn nothing_pending(&self) -> bool {
        !self.doc_pending
            && !self.note_pending
            && !self.categories_pending
            && !self.triage_pending
            && self.pending_tasks.is_empty()
    }
}
#[derive(Default)]
struct Written {
    document: bool,
    content: bool,
    tasks: Vec<usize>,
    triage_concluded: bool,
}
impl Written {
    fn any(&self) -> bool {
        self.document || self.content || !self.tasks.is_empty() || self.triage_concluded
    }
}

/// Whether a record is the one a plan creates: same title, text and type.
fn is_record_of_plan(row: &DocRow, doc: &PlanDocument) -> bool {
    let kind = row.capture.metadata.get("manual_kind").map(String::as_str);
    row.capture.title == doc.title.trim()
        && row.capture.text == doc.text.trim_end()
        && kind == Some(doc.kind.as_str())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(text: &str) -> Option<Vec<u8>> {
    text.len().is_multiple_of(2).then_some(())?;
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()
}
impl Store {
    // ---- the receipt ---------------------------------------------------------

    fn plan_run(&self, request: &str) -> Result<Option<PlanRun>> {
        let table: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='plan_import_log')",
            [],
            |r| r.get(0),
        )?;
        if !table {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT digest,doc_created,tasks_created,note_step,categories_step,triage_step,triage_uid,target FROM plan_import_log WHERE request_id=?1",
                [request],
                |r| {
                    Ok(PlanRun {
                        digest: r.get(0)?,
                        doc_created: r.get::<_, i64>(1)? != 0,
                        tasks_created: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                        note_step: r.get(3)?,
                        categories_step: r.get(4)?,
                        triage_step: r.get(5)?,
                        triage_uid: r.get(6)?,
                        target: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }
    /// Reserves the request id, durably, BEFORE anything is written: the row
    /// (request id, digest of the normalised plan) is committed in a
    /// transaction of its own, and the id is UNIQUE. Answers whether the row is
    /// new. A row that exists with the same digest is a previous run that was
    /// interrupted (the caller holds the lock of the import, so nobody else is
    /// running it) and is resumed; a different digest is refused.
    fn reserve_plan_run(&mut self, request: &str, digest: &str, target: &str) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(PLAN_TABLE)?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT digest FROM plan_import_log WHERE request_id=?1",
                [request],
                |r| r.get(0),
            )
            .optional()?;
        let fresh = match stored {
            None => {
                tx.execute(
                    "INSERT INTO plan_import_log(request_id,digest,target,created_at) VALUES(?1,?2,?3,?4)",
                    rusqlite::params![request, digest, target, chrono::Utc::now().to_rfc3339()],
                )?;
                true
            }
            Some(stored) => {
                ensure!(stored == digest, CHANGED_PLAN);
                false
            }
        };
        tx.commit()?;
        Ok(fresh)
    }
    /// Deletes the reservation of a run that wrote nothing at all, so a
    /// corrected file can use the same request id.
    fn abandon_plan_run(&self, request: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM plan_import_log WHERE request_id=?1 AND doc_created=0 AND tasks_created=0 AND note_step=0 AND categories_step=0 AND triage_step=0 AND triage_uid=''",
            [request],
        )?;
        Ok(())
    }
    /// `column` is one of ours, never from the file.
    fn mark_plan(&self, request: &str, column: &str, value: i64) -> Result<()> {
        self.conn.execute(
            &format!("UPDATE plan_import_log SET {column}=MAX({column},?2) WHERE request_id=?1"),
            rusqlite::params![request, value],
        )?;
        Ok(())
    }
    fn set_triage_uid(&self, request: &str, uid: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE plan_import_log SET triage_uid=?2 WHERE request_id=?1",
            rusqlite::params![request, uid],
        )?;
        Ok(())
    }

    // ---- reading the library -------------------------------------------------

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
            trashed,
            title: task.title,
            document_id,
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
    /// A stable identity of a document, for a plan that targets it by number:
    /// the id of a request that created it (it travels in the snapshots) or,
    /// when it has none, its identity hash (for an official document the
    /// number itself, which does not change).
    fn identity_of_document(&self, id: i64) -> Result<String> {
        let request: Option<String> = self
            .conn
            .query_row(
                "SELECT request_id FROM requests WHERE document_id=?1 ORDER BY request_id LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(request) = request {
            return Ok(format!("request:{request}"));
        }
        let hash: Vec<u8> = self.conn.query_row(
            "SELECT identity_hash FROM documents WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        Ok(format!("identity:{}", hex(&hash)))
    }
    fn resolve_target(&self, target: &str) -> Result<Option<i64>> {
        if let Some(request) = target.strip_prefix("request:") {
            return self.document_of_request(request);
        }
        let Some(hash) = target.strip_prefix("identity:").and_then(unhex) else {
            return Ok(None);
        };
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM documents WHERE identity_hash=?1",
                [hash],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// The only task of a document that was just created: the automatic
    /// triage the record is born with (see `ensure_document_task`). It is the
    /// only thing that tells it from a task the person adds later with the
    /// same title and action.
    fn sole_automatic_task(&self, document_id: i64) -> Result<Option<String>> {
        let rows: Vec<(String, Vec<u8>)> = self
            .conn
            .prepare("SELECT uid,payload FROM tasks WHERE document_id=?1")?
            .query_map([document_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let [(uid, blob)] = rows.as_slice() else {
            return Ok(None);
        };
        let task: Task = serde_json::from_slice(&crate::protection::unseal(blob)?)?;
        Ok((task.action == "triar").then(|| uid.clone()))
    }
    /// Whether the library already shows an effect of the plan: its record's
    /// `requests` row, or a task with one of its uids (in use or in the
    /// Lixeira).
    fn plan_effects(&self, plan: &Plan, checked: &Checked) -> Result<bool> {
        if plan.document.is_some() && self.document_of_request(&checked.request)?.is_some() {
            return Ok(true);
        }
        for task in &checked.tasks {
            if self.task_row(&task.uid)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Categories (or projects) by name, accents and case ignored; a uid is
    /// always accepted (archived or not), a name only for the ones not
    /// archived.
    fn resolve_entry(value: &str, entries: &[Entry], field: &str, what: &str) -> Result<String> {
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
                active
                    .iter()
                    .map(|e| e.uid.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
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
        state: &State,
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
        if let (true, Some(doc)) = (state.categories_pending, &plan.document) {
            document_categories = Self::resolve_all(
                &doc.categories,
                &categories,
                "document.categories",
                "categoria",
            )?;
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
        for &index in &state.pending_tasks {
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

    // ---- where everything stands ---------------------------------------------

    /// Where the plan stands, from stable identities only. `elsewhere`: the
    /// effects are in the library but this computer holds no receipt, so what
    /// is not there was not made here (and is not made now).
    fn plan_state(
        &self,
        plan: &Plan,
        checked: &Checked,
        run: Option<&PlanRun>,
        elsewhere: bool,
    ) -> Result<State> {
        let new_document = plan.document.is_some();
        let number_doc = match &plan.document_number {
            Some(number) => self.document_by_number_any(number)?,
            None => None,
        };
        let found = if new_document {
            self.document_of_request(&checked.request)?
        } else if let Some(run) = run.filter(|r| !r.target.is_empty()) {
            self.resolve_target(&run.target)?
        } else {
            number_doc.map(|(id, _)| id)
        };
        let doc = found.map(|id| self.doc_row(id)).transpose()?.flatten();
        let created_before = run.is_some_and(|r| !new_document || r.doc_created);
        let doc_state = match &doc {
            Some(row) if row.trashed => ItemState::Trashed,
            Some(_) => ItemState::Present,
            None if elsewhere || created_before => ItemState::Missing,
            None => ItemState::ToCreate,
        };
        let done = run.map_or(0, |r| r.tasks_created);
        let tasks: Vec<_> = checked
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| {
                let row = self.task_row(&task.uid)?;
                let state = match &row {
                    Some(r) if r.trashed => ItemState::Trashed,
                    Some(_) => ItemState::Present,
                    None if elsewhere || index < done => ItemState::Missing,
                    None => ItemState::ToCreate,
                };
                Ok((row, state))
            })
            .collect::<Result<_>>()?;
        let pending_tasks = if elsewhere {
            Vec::new()
        } else {
            tasks
                .iter()
                .enumerate()
                .filter(|(_, (_, state))| *state == ItemState::ToCreate)
                .map(|(index, _)| index)
                .collect()
        };
        let step = |select: fn(&PlanRun) -> i64| run.map_or(0, select);
        let doc_plan = plan.document.as_ref();
        Ok(State {
            doc_pending: !elsewhere && new_document && doc_state == ItemState::ToCreate,
            note_pending: !elsewhere
                && doc_plan.is_some_and(|d| !d.note.is_empty())
                && step(|r| r.note_step) == 0,
            categories_pending: !elsewhere
                && doc_plan.is_some_and(|d| !d.categories.is_empty())
                && step(|r| r.categories_step) == 0,
            triage_pending: !elsewhere
                && new_document
                && plan.complete_triage
                && step(|r| r.triage_step) == 0,
            pending_tasks,
            tasks,
            doc_state,
            doc,
            number_doc,
        })
    }

    /// The answer: the current state of everything the plan names, plus what
    /// this run did.
    fn plan_outcome(
        &self,
        plan: &Plan,
        checked: &Checked,
        dry_run: bool,
        written: &Written,
        elsewhere: bool,
    ) -> Result<PlanResult> {
        let run = self.plan_run(&checked.request)?;
        let state = self.plan_state(plan, checked, run.as_ref(), elsewhere)?;
        let new_document = plan.document.is_some();
        let document = DocumentOutcome {
            state: state.doc_state,
            state_label: state.doc_state.label().to_owned(),
            existing: plan.document_number.is_some(),
            id: state.doc.as_ref().map(|d| d.id),
            number: state
                .doc
                .as_ref()
                .map(|d| d.capture.document_number.clone()),
            title: state
                .doc
                .as_ref()
                .map(|d| d.capture.title.clone())
                .or_else(|| plan.document.as_ref().map(|d| d.title.trim().to_owned())),
            created_now: written.document,
        };
        let tasks = state
            .tasks
            .into_iter()
            .enumerate()
            .map(|(index, (row, item))| TaskOutcome {
                index: index + 1,
                uid: checked.tasks[index].uid.clone(),
                state: item,
                state_label: item.label().to_owned(),
                id: row.as_ref().map(|r| r.id),
                title: row
                    .map(|r| r.title)
                    .unwrap_or_else(|| checked.tasks[index].title.clone()),
                created_now: written.tasks.contains(&index),
            })
            .collect();
        let doc_plan = plan.document.as_ref();
        let write_step = |wanted: bool, select: fn(&PlanRun) -> i64| -> &'static str {
            if !wanted {
                "nao_solicitada"
            } else if elsewhere {
                "nao_verificavel"
            } else {
                match run.as_ref().map_or(0, select) {
                    0 => "sera_feita",
                    STEP_STARTED => "interrompida_nao_repetida",
                    _ => "feita",
                }
            }
        };
        let triage = if !(new_document && plan.complete_triage) {
            "nao_solicitada"
        } else if elsewhere {
            "nao_verificavel"
        } else {
            match run.as_ref().map_or(0, |r| r.triage_step) {
                0 => "sera_concluida",
                STEP_STARTED => "interrompida_nao_repetida",
                STEP_DONE if written.triage_concluded => "concluida_agora",
                STEP_DONE => "ja_concluida_antes",
                _ => "nao_encontrada",
            }
        };
        Ok(PlanResult {
            ok: true,
            request_id: checked.request.clone(),
            dry_run,
            created: written.any(),
            applied_elsewhere: elsewhere,
            ids_provisional: self.client_config()?.is_some(),
            document,
            tasks,
            note: write_step(doc_plan.is_some_and(|d| !d.note.is_empty()), |r| {
                r.note_step
            })
            .to_owned(),
            categories: write_step(doc_plan.is_some_and(|d| !d.categories.is_empty()), |r| {
                r.categories_step
            })
            .to_owned(),
            triage: triage.to_owned(),
            pendencias_manuais: manual_steps(plan, run.as_ref(), elsewhere),
        })
    }

    /// Refuses, before writing, a plan that still has something to create
    /// but whose record is not where the receipt says.
    fn guard_pending(&self, plan: &Plan, state: &State, run: Option<&PlanRun>) -> Result<()> {
        let Some(run) = run else {
            return Ok(());
        };
        self.guard_resumed_target(plan, state, run)?;
        match state.doc_state {
            ItemState::Trashed => bail!(
                "O plano ainda tem o que criar, mas o documento dele está na Lixeira. Restaure-o (ou conclua o que falta à mão) e repita o comando; nada é criado numa Lixeira."
            ),
            ItemState::Missing => {
                if let (Some(number), Some((other, _))) =
                    (&plan.document_number, state.number_doc)
                {
                    let held = self
                        .doc_row(other)?
                        .map(|d| format!("{} — {}", d.capture.document_number, d.capture.title))
                        .unwrap_or_default();
                    bail!(
                        "document_number: o documento original deste plano não existe mais, mas o número {} agora pertence a outro documento ({held}). Nada foi gravado, para não colocar as tarefas no documento errado. Crie o que falta à mão.",
                        number.trim()
                    );
                }
                bail!(
                    "O plano ainda tinha o que criar, mas o documento dele não existe mais (foi excluído definitivamente) e não é recriado. Use um request_id novo para um plano novo."
                );
            }
            _ => {}
        }
        if let (Some(number), Some((other, _)), Some(doc)) =
            (&plan.document_number, state.number_doc, &state.doc)
        {
            if !run.target.is_empty() && other != doc.id {
                bail!(
                    "document_number: o documento deste plano agora é {} — {}, mas o número {} que o arquivo usa passou a pertencer a outro documento. Nada foi gravado, para não colocar as tarefas no documento errado. Crie o que falta à mão.",
                    doc.capture.document_number,
                    doc.capture.title,
                    number.trim()
                );
            }
        }
        Ok(())
    }

    /// A resumed run writes only on the record the plan created or targeted.
    /// What the receipt holds (`request:`/`identity:`, or the plan's own
    /// request id for a record it creates) is not proof enough: after an
    /// exchange between copies the same request id can name another record
    /// (two records that came with the same request id, one of them refused).
    /// So the record found must also be the plan's: the one it created has
    /// the plan's title, text and type; the one it targeted by number has
    /// that number, now or before on this computer. A task of the plan that
    /// was already written must hang from that same record. Otherwise nothing
    /// is written.
    fn guard_resumed_target(&self, plan: &Plan, state: &State, run: &PlanRun) -> Result<()> {
        let Some(doc) = &state.doc else {
            return Ok(());
        };
        let found = format!("{} — {}", doc.capture.document_number, doc.capture.title);
        if let Some(record) = &plan.document {
            ensure!(
                is_record_of_plan(doc, record),
                "request_id: a retomada deste plano encontrou, pelo identificador do pedido, outro registro ({found}), não o que o plano criou (\"{}\"). Isso acontece quando dois registros chegam à sincronização com o mesmo identificador e um deles é recusado. Nada foi gravado, para não colocar as tarefas no registro errado. Confira os cartões de conflito e crie o que falta à mão.",
                record.title.trim()
            );
        } else if let (Some(number), false) = (&plan.document_number, run.target.is_empty()) {
            let wanted = crate::storage::identity_hash_for_number(number);
            let has_number = crate::storage::identity_hash_for_number(&doc.capture.document_number)
                == wanted
                || self
                    .previous_numbers(doc.id)
                    .iter()
                    .any(|old| crate::storage::identity_hash_for_number(old) == wanted);
            ensure!(
                has_number,
                "document_number: a retomada deste plano encontrou, pelo recibo guardado, o documento {found}, que não tem nem teve neste computador o número {} do arquivo. O documento original pode ter sido recusado numa sincronização (dois registros com o mesmo identificador de pedido). Nada foi gravado, para não colocar as tarefas no documento errado. Confira os cartões de conflito e crie o que falta à mão.",
                number.trim()
            );
        }
        for (index, (row, _)) in state.tasks.iter().enumerate() {
            if let Some(row) = row {
                ensure!(
                    row.document_id == Some(doc.id),
                    "tasks[{}]: a tarefa \"{}\" deste plano já foi gravada em outro documento, não em {found}. Nada foi gravado, para não dividir o plano entre dois documentos. Confira o documento da tarefa e crie o que falta à mão.",
                    index + 1,
                    row.title
                );
            }
        }
        Ok(())
    }

    /// The document of a first run on an existing record, as the app finds it
    /// by number; the two ways it can be missing are told apart.
    fn existing_document_to_use(&self, plan: &Plan) -> Result<i64> {
        let number = plan.document_number.as_deref().unwrap_or_default().trim();
        match self.document_by_number_any(number)? {
            Some((id, false)) => Ok(id),
            Some((_, true)) => bail!(
                "document_number: o documento {number} está na Lixeira; restaure-o antes de criar tarefas nele."
            ),
            None => bail!(
                "document_number: nenhum documento com o número {number} neste acervo (a busca ignora maiúsculas, minúsculas e espaços). Confira o número na ficha do documento."
            ),
        }
    }

    /// The answer for a plan that this computer holds no receipt for but
    /// whose effects are in the library: it was applied on another computer
    /// of the same shared library. Nothing is written.
    fn applied_elsewhere(
        &self,
        plan: &Plan,
        checked: &Checked,
        dry_run: bool,
    ) -> Result<PlanResult> {
        if let (Some(doc), Some(id)) = (&plan.document, self.document_of_request(&checked.request)?)
        {
            if let Some(row) = self.doc_row(id)? {
                ensure!(
                    is_record_of_plan(&row, doc),
                    "request_id: este identificador já foi usado, neste acervo, para outro registro ({}). Use um request_id novo para este plano.",
                    row.capture.document_number
                );
            }
        }
        self.plan_outcome(plan, checked, dry_run, &Written::default(), true)
    }

    // ---- running a plan --------------------------------------------------------

    /// Runs a plan: validates it, reserves its request id, and creates what
    /// has never been created.
    ///
    /// The order is: the checks that need no library; the receipt of an
    /// earlier run (a plan with nothing left is only reported, with the
    /// current state of each item); the references by name (only for what is
    /// still to be created); the reservation of the request id (committed,
    /// exclusive, before any effect); then the writes. Nothing is written for
    /// an invalid plan or a dry run. A run that fails before writing anything
    /// gives the reservation back.
    ///
    /// Atomicity, by mode (see `CONTRIBUINDO.md`): on a library used alone or
    /// on the main computer the tasks are one transaction (all or none); in
    /// shared-folder mode and on a computer connected to a main computer each
    /// task is its own change (queued or sent, like the screens do), so an
    /// interruption can leave some of them, and the same command finishes
    /// the job. The plan as a whole is never one transaction.
    pub fn import_plan(&mut self, plan: &Plan, dry_run: bool) -> Result<PlanResult> {
        let checked = check_plan(plan)?;
        let run = self.plan_run(&checked.request)?;
        if let Some(run) = &run {
            ensure!(run.digest == checked.digest, CHANGED_PLAN);
        }
        if run.is_none() && self.plan_effects(plan, &checked)? {
            return self.applied_elsewhere(plan, &checked, dry_run);
        }
        let mut target = String::new();
        if run.is_none() && plan.document_number.is_some() {
            target = self.identity_of_document(self.existing_document_to_use(plan)?)?;
        }
        let state = self.plan_state(plan, &checked, run.as_ref(), false)?;
        if state.nothing_pending() {
            return self.plan_outcome(plan, &checked, dry_run, &Written::default(), false);
        }
        self.guard_pending(plan, &state, run.as_ref())?;
        // The references are settled before anything is reserved, so a plan
        // that names a category wrongly leaves no trace at all.
        self.resolve_references(plan, &mut checked.tasks.clone(), &state)?;
        if dry_run {
            return self.plan_outcome(plan, &checked, true, &Written::default(), false);
        }

        let reserved_now = self.reserve_plan_run(&checked.request, &checked.digest, &target)?;
        let result = self.apply_plan(plan, &checked);
        if result.is_err() && reserved_now && !self.plan_effects(plan, &checked).unwrap_or(true) {
            let _ = self.abandon_plan_run(&checked.request);
        }
        result
    }

    fn apply_plan(&mut self, plan: &Plan, checked: &Checked) -> Result<PlanResult> {
        let request = checked.request.as_str();
        // What the reservation found may differ from what was read before it.
        let run = self
            .plan_run(request)?
            .context("A reserva do plano não foi encontrada.")?;
        let state = self.plan_state(plan, checked, Some(&run), false)?;
        let mut written = Written::default();
        if state.nothing_pending() {
            return self.plan_outcome(plan, checked, false, &written, false);
        }
        self.guard_pending(plan, &state, Some(&run))?;
        let mut tasks = checked.tasks.clone();
        let document_categories = self.resolve_references(plan, &mut tasks, &state)?;
        let new_document = plan.document.is_some();

        let document_id = if new_document {
            let record = checked
                .record
                .as_ref()
                .expect("a plan with document has its record");
            let id = if state.doc_pending {
                match self
                    .create_record_with_attachments(record, &[])
                    .map_err(|e| anyhow!("document: não foi possível criar o registro: {e}"))?
                {
                    Created::New { document_id, .. } => {
                        written.document = true;
                        document_id
                    }
                    // The reservation is ours (and so is the lock of the
                    // import), so this is the record an earlier, interrupted
                    // run of this very plan made.
                    Created::Already { document_id } => document_id,
                }
            } else {
                state
                    .doc
                    .as_ref()
                    .expect("a present document has its row")
                    .id
            };
            // The automatic triage is identified only here, in the run that
            // created the record, right after creating it: it is then the
            // only task of the document. It is never inferred on a resume.
            if written.document && plan.complete_triage {
                if let Some(uid) = self.sole_automatic_task(id)? {
                    self.set_triage_uid(request, &uid)?;
                }
            }
            if !run.doc_created {
                self.mark_plan(request, "doc_created", 1)?;
            }
            let doc = plan.document.as_ref().expect("new document");
            if state.note_pending {
                self.mark_plan(request, "note_step", STEP_STARTED)?;
                self.save_note(id, &doc.note)?;
                self.mark_plan(request, "note_step", STEP_DONE)?;
                written.content = true;
            }
            if state.categories_pending {
                self.mark_plan(request, "categories_step", STEP_STARTED)?;
                self.set_document_categories(id, &document_categories)?;
                self.mark_plan(request, "categories_step", STEP_DONE)?;
                written.content = true;
            }
            id
        } else {
            state
                .doc
                .as_ref()
                .expect("an existing document was checked")
                .id
        };

        if !state.pending_tasks.is_empty() {
            let to_write: Vec<(usize, Task)> = state
                .pending_tasks
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
                written
                    .tasks
                    .extend(to_write.iter().map(|(index, _)| *index));
            } else {
                for (index, task) in &to_write {
                    self.save_task(task).map_err(|e| {
                        invalid(format!(
                            "tasks[{}]: não foi possível gravar a tarefa: {e}. As tarefas anteriores já foram gravadas; repita o mesmo comando para continuar de onde parou.",
                            index + 1
                        ))
                    })?;
                    written.tasks.push(*index);
                    self.mark_plan(request, "tasks_created", *index as i64 + 1)?;
                }
            }
            self.mark_plan(request, "tasks_created", checked.tasks.len() as i64)?;
        } else if run.tasks_created < checked.tasks.len() {
            self.mark_plan(request, "tasks_created", checked.tasks.len() as i64)?;
        }

        // The triage is concluded last, when the tasks exist: a record whose
        // only task is concluded would look finished to the team meanwhile.
        if state.triage_pending {
            self.mark_plan(request, "triage_step", STEP_STARTED)?;
            let (step, concluded) = self.conclude_automatic_triage(request)?;
            self.mark_plan(request, "triage_step", step)?;
            written.triage_concluded = concluded;
        }
        self.plan_outcome(plan, checked, false, &written, false)
    }

    /// Concludes the automatic triage of the record this plan created, and
    /// only that task: the one whose uid was kept when the record was made.
    /// If it is not found open, nothing is concluded.
    fn conclude_automatic_triage(&mut self, request: &str) -> Result<(i64, bool)> {
        let uid = self
            .plan_run(request)?
            .map(|r| r.triage_uid)
            .unwrap_or_default();
        if uid.is_empty() {
            return Ok((STEP_NOT_FOUND, false));
        }
        let Some(row) = self.task_row(&uid)?.filter(|r| !r.trashed) else {
            return Ok((STEP_NOT_FOUND, false));
        };
        let open = self
            .list_tasks()?
            .into_iter()
            .any(|t| t.id == row.id && !t.is_done());
        if !open {
            return Ok((STEP_NOT_FOUND, false));
        }
        self.complete_task(row.id)?;
        Ok((STEP_DONE, true))
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
                ensure!(
                    output.is_none(),
                    "--saida foi informado duas vezes. Uso: {USAGE}"
                );
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

/// Exit code of a plan that WAS applied but whose result could not be
/// written to the `--saida` file (the JSON is on the standard output).
pub const EXIT_APPLIED_UNREPORTED: i32 = 3;
/// Exit code of a plan that was applied but left steps (the note, the
/// categories or the triage of the record) that the person must check and
/// finish by hand; they are listed in `pendencias_manuais`.
pub const EXIT_APPLIED_MANUAL_STEPS: i32 = 4;

/// The import was applied, but the result could not be written where
/// `--saida` said. It must never be reported as a plain failure.
#[derive(Debug)]
pub struct AppliedButUnreported {
    pub path: String,
    pub reason: String,
}
impl std::fmt::Display for AppliedButUnreported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "A importação FOI aplicada, mas o resultado não pôde ser gravado em {}: {}. O resultado completo está na saída padrão. Pode ter ficado um arquivo incompleto nesse caminho; apague-o antes de reutilizar o nome. Não repita o comando por engano: repetir só informa o estado atual.",
            self.path, self.reason
        )
    }
}
impl std::error::Error for AppliedButUnreported {}

/// The plan was applied (the record and the tasks), but some steps are left
/// for the person.
#[derive(Debug)]
pub struct AppliedWithManualSteps {
    pub steps: Vec<String>,
}
impl std::fmt::Display for AppliedWithManualSteps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "O plano foi aplicado (o registro e as tarefas), mas há etapas a conferir e concluir à mão: {}",
            self.steps.join(" ")
        )
    }
}
impl std::error::Error for AppliedWithManualSteps {}

/// `--saida` names a file that must not exist yet (so it can never be the
/// library or anything else of the person). It is created exclusively
/// BEFORE the library is touched: a plan that cannot report its result is not
/// applied.
fn create_output(path: &str) -> Result<std::fs::File> {
    let target = Path::new(path);
    let folder = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure!(
        folder.is_dir(),
        "A pasta do arquivo de saída ({}) não existe.",
        folder.display()
    );
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                anyhow!(
                    "O arquivo de saída {} já existe; informe um arquivo novo (ele nunca é sobrescrito). Nada foi importado.",
                    target.display()
                )
            } else {
                anyhow!(
                    "Não foi possível criar o arquivo de saída {}: {error}. Nada foi importado.",
                    target.display()
                )
            }
        })
}

/// The lock of the import: held, with no sharing, for the whole of an
/// `--import-plan` that writes, on a file of the data folder (as the window
/// holds `janela.lock`). The system releases it when the process ends,
/// however it ends. A second import cannot take it and stops at once, so two
/// runs never interleave and a receipt left "started" is always a run that
/// ended.
fn lock_import(db: &Path) -> Result<std::fs::File> {
    let folder = db
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(folder).context("Não foi possível criar a pasta do acervo.")?;
    crate::single_instance::open_exclusive(&folder.join("importacao-plano.lock")).map_err(
        |error| {
            if cfg!(windows) && error.raw_os_error() == Some(32) {
                anyhow!("Já há uma importação de plano em andamento neste computador; tente de novo quando ela terminar. Nada foi gravado.")
            } else {
                anyhow!("Não foi possível reservar a importação de plano neste computador: {error}")
            }
        },
    )
}

/// A dry run must not change the library: it refuses a library that the
/// opening would have to migrate (the window does that), and one that does
/// not exist yet.
fn check_readable_as_is(db: &Path) -> Result<()> {
    ensure!(
        db.exists(),
        "Ainda não há acervo neste computador (abra o Ponte uma vez); sem ele, --dry-run não tem o que consultar."
    );
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
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

/// The command: creates the `--saida` file, reads the file, validates it,
/// takes the lock of the import (not for `--dry-run`, which writes nothing),
/// runs the plan on the library at `db` and prints the result as JSON to
/// `stdout` (and writes it to `--saida`). On failure the error is returned
/// (the caller prints it to the error output and exits 1) and, when `--saida`
/// was given, written there too as `{"ok":false,"erro":"..."}`. A plan that
/// was applied but whose result could not be written to `--saida` returns
/// [`AppliedButUnreported`] (exit code [`EXIT_APPLIED_UNREPORTED`]); a plan
/// applied with steps left for the person returns [`AppliedWithManualSteps`]
/// (exit code [`EXIT_APPLIED_MANUAL_STEPS`]); both after the JSON went to
/// `stdout`. In a dry run nothing was imported, so a result that cannot be
/// written is a plain failure that says so.
pub fn execute(options: &CliOptions, db: &Path, stdout: &mut dyn Write) -> Result<()> {
    execute_with(options, db, stdout, &|file, bytes| {
        file.write_all(bytes)?;
        file.sync_all()
    })
}

/// [`execute`] with the way the result reaches the `--saida` file as a
/// parameter (the tests make it fail).
fn execute_with(
    options: &CliOptions,
    db: &Path,
    stdout: &mut dyn Write,
    write_result: &dyn Fn(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<()> {
    let mut output = match &options.output {
        Some(path) => Some((path, create_output(path)?)),
        None => None,
    };
    let outcome = (|| -> Result<PlanResult> {
        let plan = parse_plan(&read_limited(&options.file)?)?;
        // Everything that needs no library is settled before it is opened.
        check_plan(&plan)?;
        let _lock = if options.dry_run {
            check_readable_as_is(db)?;
            None
        } else {
            Some(lock_import(db)?)
        };
        let mut store = Store::open(db).map_err(friendly)?;
        store.import_plan(&plan, options.dry_run).map_err(friendly)
    })();
    match outcome {
        Ok(result) => {
            let json = serde_json::to_string(&result)?;
            // A console that is not there (the release build) is not a
            // failure of a plan that was already written.
            let _ = writeln!(stdout, "{json}");
            if let Some((path, file)) = &mut output {
                if let Err(error) = write_result(file, json.as_bytes()) {
                    if options.dry_run {
                        bail!(
                            "Simulação (--dry-run): nada foi importado. O resultado não pôde ser gravado em {path}: {error}. Pode ter ficado um arquivo incompleto nesse caminho; apague-o antes de reutilizar o nome."
                        );
                    }
                    return Err(AppliedButUnreported {
                        path: (*path).clone(),
                        reason: error.to_string(),
                    }
                    .into());
                }
            }
            if !options.dry_run && !result.pendencias_manuais.is_empty() {
                return Err(AppliedWithManualSteps {
                    steps: result.pendencias_manuais,
                }
                .into());
            }
            Ok(())
        }
        Err(error) => {
            if let Some((_, file)) = &mut output {
                let report = serde_json::json!({"ok": false, "erro": format!("{error:#}")});
                // Best effort: the error itself is what the caller gets.
                let _ = write_result(file, report.to_string().as_bytes());
            }
            Err(error)
        }
    }
}

/// `ponteproj.exe --import-plan ...` (the arguments after the command).
pub fn run_cli(args: &[String]) -> Result<()> {
    let options = parse_cli(args)?;
    let db = crate::paths::db_path_in(&crate::paths::data_dir_path()?);
    execute(&options, &db, &mut std::io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intranet::{HostServer, IntranetConfig, IntranetMode};
    use crate::trash::Target;
    use std::collections::BTreeSet;

    const FIXTURE: &str = include_str!("../tests/fixtures/import-plan-exemplo.json");
    const REQUEST: &str = "47f3f62f-404c-4931-ae04-bc7eb1108c07";
    const REQUEST_UPPER: &str = "47F3F62F-404C-4931-AE04-BC7EB1108C07";

    fn plan() -> Plan {
        serde_json::from_str(FIXTURE).unwrap()
    }
    /// A plan of `n` tasks, no triage, no categories.
    fn big_plan(n: usize) -> Plan {
        let mut plan = plan();
        plan.complete_triage = false;
        plan.tasks = (1..=n)
            .map(|i| PlanTask {
                title: format!("Tarefa {i}"),
                action: "examinar".into(),
                notes: String::new(),
                due_date: String::new(),
                priority: "normal".into(),
                assignee: String::new(),
                checklist: vec![],
                scheduled_start: String::new(),
                scheduled_end: String::new(),
                estimate_minutes: 0,
                categories: vec![],
                project: String::new(),
            })
            .collect();
        plan
    }
    fn library() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        (dir, store)
    }
    fn shared_folder(store: &mut Store) {
        let config = IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: "Membro de teste".into(),
            folder_files: vec!["ponte-sync-teste.ponte".into()],
            folder_writer: uuid::Uuid::new_v4().simple().to_string(),
            ..store.load_intranet_config().unwrap()
        };
        store.save_intranet_config(&config).unwrap();
    }
    fn message(plan: &Plan) -> String {
        check_plan(plan)
            .err()
            .expect("the plan must be refused")
            .to_string()
    }
    fn run_error(store: &mut Store, plan: &Plan) -> String {
        store.import_plan(plan, false).unwrap_err().to_string()
    }
    fn receipt_table(store: &Store) -> bool {
        store
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='plan_import_log')",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    /// Everything an invalid plan or a dry run must not touch (the last
    /// element is the number of rows of the receipt table).
    fn traces(store: &Store) -> (i64, usize, usize, usize, usize, i64) {
        (
            store.cabinet_revision().unwrap(),
            store.list_team_events(500).unwrap().len(),
            store.list_offline_outbox().unwrap().len(),
            store.list_documents("").unwrap().len(),
            store.list_tasks().unwrap().len(),
            if receipt_table(store) {
                store
                    .conn
                    .query_row("SELECT count(*) FROM plan_import_log", [], |r| r.get(0))
                    .unwrap()
            } else {
                0
            },
        )
    }
    fn ids(result: &PlanResult) -> Vec<Option<i64>> {
        result.tasks.iter().map(|t| t.id).collect()
    }
    fn edit_fixture(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
        edit(&mut value);
        serde_json::to_vec(&value).unwrap()
    }
    fn fail_on(store: &Store, table: &str, when: &str) {
        store
            .conn
            .execute_batch(&format!(
                "CREATE TRIGGER falha_simulada BEFORE INSERT ON {table} WHEN {when} BEGIN SELECT RAISE(ABORT,'falha simulada'); END;"
            ))
            .unwrap();
    }
    fn stop_failing(store: &Store) {
        store
            .conn
            .execute_batch("DROP TRIGGER falha_simulada")
            .unwrap();
    }
    fn trash(store: &mut Store, kind: Kind, id: i64) {
        store.trash_item(Target { kind, id }).unwrap();
    }
    fn purge(store: &mut Store, kind: Kind, id: i64) {
        trash(store, kind, id);
        store.purge_trash_item(Target { kind, id }).unwrap();
    }

    // ---- the happy path and the rerun ---------------------------------------

    #[test]
    fn creates_the_record_its_tasks_and_a_rerun_reports_the_current_state() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        assert!(first.ok && first.created && !first.dry_run && !first.ids_provisional);
        assert_eq!(first.document.state, ItemState::Present);
        assert!(first.document.created_now && !first.document.existing);
        let number = first.document.number.clone().unwrap();
        assert!(number.starts_with("EMAIL-"), "{number}");
        assert_eq!(first.triage, "concluida_agora");
        assert!(first
            .tasks
            .iter()
            .all(|t| t.state == ItemState::Present && t.created_now && t.id.is_some()));
        assert_eq!(first.tasks[1].index, 2);
        let document_id = first.document.id.unwrap();
        // The record, its note, the plan's tasks and the automatic triage.
        let detail = store.load_document(document_id).unwrap();
        assert_eq!(detail.note, "Acompanhar o prazo.");
        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 3);
        assert!(tasks.iter().all(|t| t.document_id == Some(document_id)));
        assert!(tasks
            .iter()
            .find(|t| t.title == "Triar documento")
            .unwrap()
            .is_done());
        assert_eq!(tasks.iter().filter(|t| t.is_done()).count(), 1);

        let again = store.import_plan(&plan(), false).unwrap();
        assert!(!again.created && !again.document.created_now);
        assert_eq!(again.document.id, first.document.id);
        assert_eq!(again.document.number, first.document.number);
        assert_eq!(ids(&again), ids(&first));
        assert_eq!(again.triage, "ja_concluida_antes");
        assert!(again.tasks.iter().all(|t| !t.created_now));
        assert_eq!(store.list_tasks().unwrap().len(), 3);
        assert_eq!(store.list_documents("").unwrap().len(), 1);
    }

    #[test]
    fn a_rerun_answers_from_the_receipt_even_if_references_changed_and_overwrites_nothing() {
        let (_dir, mut store) = library();
        let category = store
            .save_category(&crate::categories::Category::new("Atenção", 1))
            .unwrap();
        let mut input = plan();
        input.document.as_mut().unwrap().categories = vec!["ATENÇÃO".into()];
        input.tasks[0].categories = vec!["atencao".into()];
        let first = store.import_plan(&input, false).unwrap();
        assert!(first.created);
        let document_id = first.document.id.unwrap();
        assert_eq!(
            store.document_categories(document_id).unwrap(),
            vec![category.uid.clone()]
        );

        // The person renames the category, rewrites the note and edits a task.
        let mut renamed = category.clone();
        renamed.name = "Outro nome".into();
        store.save_category(&renamed).unwrap();
        store
            .save_note(document_id, "Minha nota, escrita depois.")
            .unwrap();
        let mut edited = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == first.tasks[0].id.unwrap())
            .unwrap();
        edited.title = "Título que a pessoa mudou".into();
        edited.notes = "Observação da pessoa".into();
        edited.categories.clear();
        store.save_task(&edited).unwrap();

        // The same file: no error about the category, and nothing is updated.
        let again = store.import_plan(&input, false).unwrap();
        assert!(!again.created);
        assert_eq!(again.tasks[0].title, "Título que a pessoa mudou");
        let tasks = store.list_tasks().unwrap();
        let task = tasks
            .iter()
            .find(|t| t.id == again.tasks[0].id.unwrap())
            .unwrap();
        assert_eq!(task.notes, "Observação da pessoa");
        assert!(task.categories.is_empty(), "the categories are not reset");
        assert_eq!(
            store.load_document(document_id).unwrap().note,
            "Minha nota, escrita depois."
        );
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn a_rerun_reports_the_trash_and_purges_and_recreates_nothing() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        // One task in the Lixeira, the other deleted for good.
        trash(&mut store, Kind::Task, first.tasks[0].id.unwrap());
        purge(&mut store, Kind::Task, first.tasks[1].id.unwrap());
        let again = store.import_plan(&plan(), false).unwrap();
        assert!(!again.created);
        assert_eq!(again.tasks[0].state, ItemState::Trashed);
        assert_eq!(again.tasks[0].state_label, "na Lixeira");
        assert_eq!(again.tasks[0].id, first.tasks[0].id);
        assert_eq!(again.tasks[0].title, "Examinar a solicitação");
        assert_eq!(again.tasks[1].state, ItemState::Missing);
        assert_eq!(again.tasks[1].state_label, "não existe mais");
        assert_eq!(again.tasks[1].id, None);
        assert_eq!(again.document.state, ItemState::Present);
        // Nothing came back, and the Lixeira still has the task.
        let live: Vec<_> = store
            .list_tasks()
            .unwrap()
            .iter()
            .map(|t| t.title.clone())
            .collect();
        assert_eq!(live, vec!["Triar documento".to_owned()]);
        assert_eq!(store.trash_entries().unwrap().len(), 1);

        // The record itself in the Lixeira: every item says so.
        trash(&mut store, Kind::Document, first.document.id.unwrap());
        let again = store.import_plan(&plan(), false).unwrap();
        assert_eq!(again.document.state, ItemState::Trashed);
        assert_eq!(again.document.number, first.document.number);
        assert_eq!(again.tasks[0].state, ItemState::Trashed);
        // And for good.
        store
            .purge_trash_item(Target {
                kind: Kind::Document,
                id: first.document.id.unwrap(),
            })
            .unwrap();
        let again = store.import_plan(&plan(), false).unwrap();
        assert!(!again.created);
        assert_eq!(again.document.state, ItemState::Missing);
        assert_eq!(again.document.id, None);
        assert!(again.tasks.iter().all(|t| t.state == ItemState::Missing));
        assert!(store.list_documents("").unwrap().is_empty());
        assert!(store.list_tasks().unwrap().is_empty());
    }

    #[test]
    fn ids_and_the_number_are_read_again_after_the_local_ids_are_rewritten_by_sql() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        // What a synchronisation does: the same items under other local ids.
        store
            .conn
            .execute_batch(
                "PRAGMA foreign_keys=OFF;
                 UPDATE tasks SET id=id+1000, document_id=document_id+1000;
                 UPDATE requests SET document_id=document_id+1000;
                 UPDATE captures SET document_id=document_id+1000;
                 UPDATE notes SET document_id=document_id+1000;
                 UPDATE work_context SET document_id=document_id+1000;
                 UPDATE documents SET id=id+1000;
                 PRAGMA foreign_keys=ON;",
            )
            .unwrap();
        let again = store.import_plan(&plan(), false).unwrap();
        assert!(!again.created);
        assert_eq!(again.document.id, Some(first.document.id.unwrap() + 1000));
        assert_eq!(
            ids(&again),
            ids(&first)
                .iter()
                .map(|id| id.map(|i| i + 1000))
                .collect::<Vec<_>>()
        );
        assert!(again.tasks.iter().all(|t| t.state == ItemState::Present));
        assert_eq!(store.list_tasks().unwrap().len(), 3);
        assert_eq!(store.list_documents("").unwrap().len(), 1);
    }

    #[test]
    fn once_the_receipt_exists_another_plan_with_the_same_request_is_refused_and_an_equal_file_is_the_same_plan(
    ) {
        let (_dir, mut store) = library();
        store.import_plan(&plan(), false).unwrap();
        let mut other = plan();
        other.tasks[1].title = "Preparar outra resposta".into();
        let said = run_error(&mut store, &other);
        assert!(
            said.contains("request_id")
                && said.contains("plano diferente")
                && said.contains("request_id novo"),
            "{said}"
        );
        assert_eq!(store.list_tasks().unwrap().len(), 3);

        // The same plan written another way: upper-case uuid, other key order,
        // no whitespace, a default spelled out.
        let reordered = format!(
            r#"{{"tasks":[{{"action":"examinar","title":"Examinar a solicitação","priority":"alta","assignee":"Equipe","due_date":"2026-12-01","checklist":[[false,"Conferir os documentos"]]}},{{"priority":"normal","action":"responder","title":"Preparar resposta"}}],"complete_triage":true,"document":{{"type":"email","title":"Solicitação de informações","text":"Verificar os dados recebidos e preparar a resposta.","note":"Acompanhar o prazo.","origin":"Secretaria","date":"2026-11-22","categories":[]}},"request_id":"{REQUEST_UPPER}"}}"#
        );
        let same = parse_plan(reordered.as_bytes()).unwrap();
        let again = store.import_plan(&same, false).unwrap();
        assert!(!again.created, "{again:?}");
    }

    // ---- nothing is left behind ---------------------------------------------

    #[test]
    fn an_invalid_plan_leaves_nothing_and_the_request_id_stays_usable() {
        for queued in [false, true] {
            let (_dir, mut store) = library();
            if queued {
                shared_folder(&mut store);
            }
            let before = traces(&store);
            let mut bad = plan();
            bad.tasks[1].categories = vec!["Não existe".into()];
            let said = run_error(&mut store, &bad);
            assert!(said.contains("tasks[2].categories[1]"), "{said}");
            let mut bad = plan();
            bad.tasks[0].due_date = "22/11/2026".into();
            assert!(run_error(&mut store, &bad).contains("tasks[1].due_date"));
            assert_eq!(traces(&store), before, "queued: {queued}");
            assert!(
                !receipt_table(&store),
                "not even the table, queued: {queued}"
            );

            // A failure of the very first write leaves nothing either.
            fail_on(&store, "documents", "1");
            assert!(store.import_plan(&plan(), false).is_err());
            assert_eq!(traces(&store), before, "first write, queued: {queued}");
            stop_failing(&store);

            // The corrected plan runs with the same request id.
            let done = store.import_plan(&plan(), false).unwrap();
            assert!(done.created, "queued: {queued}");
        }
    }

    #[test]
    fn a_dry_run_writes_nothing_and_reports_what_it_would_do() {
        for queued in [false, true] {
            let (_dir, mut store) = library();
            if queued {
                shared_folder(&mut store);
            }
            let before = traces(&store);
            let dry = store.import_plan(&plan(), true).unwrap();
            assert!(dry.dry_run && !dry.created && !dry.document.created_now);
            assert_eq!(dry.document.state, ItemState::ToCreate);
            assert_eq!(dry.document.id, None);
            assert_eq!(
                dry.document.title.as_deref(),
                Some("Solicitação de informações")
            );
            assert!(dry
                .tasks
                .iter()
                .all(|t| t.state == ItemState::ToCreate && t.id.is_none()));
            assert_eq!(dry.triage, "sera_concluida");
            assert_eq!(traces(&store), before, "queued: {queued}");
            assert!(
                !receipt_table(&store),
                "not even the table, queued: {queued}"
            );

            // After the real run, a dry run only reports.
            let real = store.import_plan(&plan(), false).unwrap();
            let after = traces(&store);
            let dry = store.import_plan(&plan(), true).unwrap();
            assert!(dry.dry_run && !dry.created);
            assert_eq!(ids(&dry), ids(&real));
            assert_eq!(traces(&store), after);
        }
    }

    // ---- interruptions --------------------------------------------------------

    /// Makes a write of the receipt fail (a process that died right there).
    fn fail_receipt_update(store: &Store, when: &str) {
        store.conn.execute_batch(PLAN_TABLE).unwrap();
        store
            .conn
            .execute_batch(&format!(
                "CREATE TRIGGER falha_simulada BEFORE UPDATE ON plan_import_log WHEN {when} BEGIN SELECT RAISE(ABORT,'falha simulada'); END;"
            ))
            .unwrap();
    }

    #[test]
    fn a_failure_exactly_between_the_record_and_the_next_receipt_update_cannot_mix_two_plans() {
        for queued in [false, true] {
            let (_dir, mut store) = library();
            if queued {
                shared_folder(&mut store);
            }
            // The record is created, then the receipt update that follows it dies.
            fail_receipt_update(&store, "NEW.doc_created=1");
            assert!(store.import_plan(&plan(), false).is_err());
            stop_failing(&store);
            assert_eq!(store.list_documents("").unwrap().len(), 1);
            assert_eq!(store.list_tasks().unwrap().len(), 1, "only the triage");

            // Another plan with the same request id (same title, text and
            // type, other tasks) is refused: the reservation holds the digest.
            let mut other = plan();
            other.tasks[0].title = "Outra tarefa".into();
            other.document.as_mut().unwrap().origin = "Outra origem".into();
            let said = run_error(&mut store, &other);
            assert!(said.contains("plano diferente"), "{said}");
            assert_eq!(store.list_tasks().unwrap().len(), 1);

            // The same plan finds its record by the request and finishes.
            let done = store.import_plan(&plan(), false).unwrap();
            assert!(
                done.created && !done.document.created_now,
                "queued: {queued}"
            );
            assert_eq!(store.list_documents("").unwrap().len(), 1);
            assert_eq!(store.list_tasks().unwrap().len(), 3);
            assert_eq!(done.triage, "concluida_agora");
        }
    }

    #[test]
    fn a_request_reserved_by_a_run_that_died_right_after_the_reservation_refuses_another_plan() {
        let (_dir, mut store) = library();
        // The record cannot be created, and the run dies before it can give
        // the reservation back (the delete fails too).
        fail_on(&store, "documents", "1");
        store.conn.execute_batch(PLAN_TABLE).unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER sem_apagar BEFORE DELETE ON plan_import_log BEGIN SELECT RAISE(ABORT,'morreu'); END;",
            )
            .unwrap();
        assert!(store.import_plan(&plan(), false).is_err());
        store.conn.execute_batch("DROP TRIGGER sem_apagar").unwrap();
        stop_failing(&store);
        assert_eq!(traces(&store).5, 1, "the reservation is there");
        assert!(store.list_documents("").unwrap().is_empty());

        let mut other = plan();
        other.tasks[1].title = "Outro plano, mesmo request_id".into();
        let said = run_error(&mut store, &other);
        assert!(said.contains("plano diferente"), "{said}");
        assert!(store.list_documents("").unwrap().is_empty());

        // The original plan goes on from the reservation.
        let done = store.import_plan(&plan(), false).unwrap();
        assert!(done.created && done.document.created_now);
        assert_eq!(store.list_tasks().unwrap().len(), 3);
    }

    #[test]
    fn a_run_that_fails_before_any_effect_gives_the_request_back() {
        for queued in [false, true] {
            let (_dir, mut store) = library();
            if queued {
                shared_folder(&mut store);
            }
            fail_on(&store, "documents", "1");
            assert!(store.import_plan(&plan(), false).is_err());
            assert_eq!(traces(&store).5, 0, "the reservation was deleted");
            stop_failing(&store);
            // The id can be used for another (corrected) plan.
            let mut corrected = plan();
            corrected.tasks[1].title = "Resposta corrigida".into();
            assert!(store.import_plan(&corrected, false).unwrap().created);
        }
    }

    /// The mutual exclusion comes from the system, for the whole import. (The
    /// lock is a share-mode-0 open, as the window's, so it exists on Windows.)
    #[cfg(windows)]
    #[test]
    fn a_second_import_cannot_take_the_lock_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        drop(Store::open(&db).unwrap());
        let file = dir.path().join("plano.json");
        std::fs::write(&file, FIXTURE).unwrap();
        let options = |dry_run: bool| CliOptions {
            file: file.to_str().unwrap().to_owned(),
            dry_run,
            output: None,
        };
        // Another import is running: it holds the lock.
        let held = lock_import(&db).unwrap();
        let said = execute(&options(false), &db, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("Já há uma importação de plano em andamento neste computador")
                && said.contains("tente de novo quando ela terminar"),
            "{said}"
        );
        assert!(Store::open(&db)
            .unwrap()
            .list_documents("")
            .unwrap()
            .is_empty());
        // A dry run writes nothing, so it does not need the lock.
        let mut out = Vec::new();
        execute(&options(true), &db, &mut out).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap()["dry_run"],
            true
        );
        // When the holder ends (the system releases the lock), the import runs.
        drop(held);
        execute(&options(false), &db, &mut Vec::new()).unwrap();
        assert_eq!(
            Store::open(&db).unwrap().list_documents("").unwrap().len(),
            1
        );
        assert!(lock_import(&db).is_ok(), "released again at the end");
    }

    #[test]
    fn a_row_left_started_by_an_interrupted_run_is_resumed_and_has_no_owner_or_expiry() {
        let (_dir, mut store) = library();
        let checked = check_plan(&plan()).unwrap();
        // The run that reserved the request ended without writing more.
        assert!(store
            .reserve_plan_run(&checked.request, &checked.digest, "")
            .unwrap());
        let done = store.import_plan(&plan(), false).unwrap();
        assert!(done.created && done.document.created_now);
        assert_eq!(store.list_tasks().unwrap().len(), 3);
        // The same digest reserves again without a new row; another one never.
        assert!(!store
            .reserve_plan_run(&checked.request, &checked.digest, "")
            .unwrap());
        assert!(store
            .reserve_plan_run(&checked.request, "outro-resumo", "")
            .is_err());
        let columns: Vec<String> = store
            .conn
            .prepare("SELECT name FROM pragma_table_info('plan_import_log')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(columns
            .iter()
            .all(|c| !c.contains("lease") && !c.contains("owner")));
    }

    #[test]
    fn the_triage_is_never_inferred_on_a_resume() {
        let (_dir, mut store) = library();
        // The record is created; the run dies before its triage uid is kept.
        fail_receipt_update(&store, "NEW.triage_uid<>''");
        assert!(store.import_plan(&plan(), false).is_err());
        stop_failing(&store);
        let id = store.list_documents("").unwrap()[0].id;
        let checked = check_plan(&plan()).unwrap();
        assert_eq!(
            store
                .plan_run(&checked.request)
                .unwrap()
                .unwrap()
                .triage_uid,
            ""
        );
        // The automatic triage is purged, and the person adds ONE task of
        // their own with the action "triar": now the only such task.
        let automatic: i64 = store
            .conn
            .query_row("SELECT id FROM tasks WHERE document_id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        purge(&mut store, Kind::Task, automatic);
        let theirs = store.save_task(&Task::new(id)).unwrap();
        let done = store.import_plan(&plan(), false).unwrap();
        assert_eq!(done.triage, "nao_encontrada");
        assert!(store
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.id == theirs && !t.is_done()));
    }

    #[test]
    fn a_started_step_is_reported_for_manual_completion_with_its_own_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        let file = dir.path().join("plano.json");
        std::fs::write(&file, FIXTURE).unwrap();
        let out = dir.path().join("resultado.json");
        // The note step is marked as started, and the note is never written.
        {
            let mut store = Store::open(&db).unwrap();
            fail_on(&store, "notes", "1");
            assert!(store.import_plan(&plan(), false).is_err());
            stop_failing(&store);
        }
        let options = |dry_run: bool, output: Option<&std::path::Path>| CliOptions {
            file: file.to_str().unwrap().to_owned(),
            dry_run,
            output: output.map(|p| p.to_str().unwrap().to_owned()),
        };
        let mut stdout = Vec::new();
        let error = execute(&options(false, Some(&out)), &db, &mut stdout).unwrap_err();
        let said = error.to_string();
        assert!(
            error.downcast_ref::<AppliedWithManualSteps>().is_some()
                && said.contains("foi aplicado")
                && said.contains("A nota do registro pode não ter sido gravada"),
            "{said}"
        );
        assert_ne!(EXIT_APPLIED_MANUAL_STEPS, EXIT_APPLIED_UNREPORTED);
        assert!(![0, 1].contains(&EXIT_APPLIED_MANUAL_STEPS));
        let printed: Value = serde_json::from_slice(&stdout).unwrap();
        let written: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(printed, written);
        assert_eq!(printed["ok"], true);
        assert_eq!(printed["note"], "interrompida_nao_repetida");
        assert_eq!(printed["pendencias_manuais"].as_array().unwrap().len(), 1);
        // A simulation applies nothing, so it exits 0 and only lists them.
        let mut stdout = Vec::new();
        execute(&options(true, None), &db, &mut stdout).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&stdout).unwrap()["pendencias_manuais"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // A plan applied without leftovers exits 0 (see the other command tests).
    }

    #[test]
    fn a_dry_run_whose_result_cannot_be_written_says_nothing_was_imported() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        drop(Store::open(&db).unwrap());
        let file = dir.path().join("plano.json");
        std::fs::write(&file, FIXTURE).unwrap();
        let options = CliOptions {
            file: file.to_str().unwrap().to_owned(),
            dry_run: true,
            output: Some(
                dir.path()
                    .join("simulacao.json")
                    .to_str()
                    .unwrap()
                    .to_owned(),
            ),
        };
        let error = execute_with(&options, &db, &mut Vec::new(), &|_, _| {
            Err(std::io::Error::other("disco cheio"))
        })
        .unwrap_err();
        let said = error.to_string();
        assert!(
            error.downcast_ref::<AppliedButUnreported>().is_none()
                && error.downcast_ref::<AppliedWithManualSteps>().is_none()
                && said.contains("nada foi importado")
                && said.contains("disco cheio")
                && said.contains("arquivo incompleto")
                && !said.contains("FOI aplicada"),
            "{said}"
        );
        assert!(Store::open(&db)
            .unwrap()
            .list_documents("")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_note_whose_step_was_started_is_never_written_again() {
        for queued in [false, true] {
            let (_dir, mut store) = library();
            if queued {
                shared_folder(&mut store);
            }
            let mut input = plan();
            input.document.as_mut().unwrap().note = "Nota do plano".into();
            fail_on(&store, "notes", "1");
            assert!(store.import_plan(&input, false).is_err());
            stop_failing(&store);
            assert_eq!(store.list_documents("").unwrap().len(), 1);
            assert_eq!(store.list_tasks().unwrap().len(), 1);
            let done = store.import_plan(&input, false).unwrap();
            assert!(
                done.created && !done.document.created_now,
                "queued: {queued}"
            );
            assert_eq!(done.note, "interrompida_nao_repetida");
            assert_eq!(
                done.pendencias_manuais.len(),
                1,
                "{:?}",
                done.pendencias_manuais
            );
            assert_eq!(
                store.load_document(done.document.id.unwrap()).unwrap().note,
                "",
                "not written again"
            );
            assert_eq!(store.list_tasks().unwrap().len(), 3);
            assert_eq!(done.triage, "concluida_agora");
            // And a later run still says so.
            let again = store.import_plan(&input, false).unwrap();
            assert!(!again.created);
            assert_eq!(again.note, "interrompida_nao_repetida");
        }
    }

    #[test]
    fn a_note_the_person_removed_between_an_interrupted_run_and_the_rerun_stays_removed() {
        let (_dir, mut store) = library();
        let mut input = plan();
        input.document.as_mut().unwrap().note = "Nota do plano".into();
        // The note is written and marked done; the run dies before the next step.
        store
            .save_category(&crate::categories::Category::new("Atenção", 1))
            .unwrap();
        input.document.as_mut().unwrap().categories = vec!["Atenção".into()];
        fail_receipt_update(&store, "NEW.categories_step=1");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let id = store.list_documents("").unwrap()[0].id;
        assert_eq!(store.load_document(id).unwrap().note, "Nota do plano");
        // The person removes it on purpose.
        store.save_note(id, "").unwrap();
        let done = store.import_plan(&input, false).unwrap();
        assert_eq!(done.note, "feita");
        assert_eq!(
            store.load_document(id).unwrap().note,
            "",
            "not written again"
        );
        assert_eq!(done.categories, "feita");
        assert_eq!(store.document_categories(id).unwrap().len(), 1);
    }

    #[test]
    fn categories_the_person_removed_after_a_started_step_are_not_applied_again() {
        let (_dir, mut store) = library();
        let mut input = plan();
        store
            .save_category(&crate::categories::Category::new("Atenção", 1))
            .unwrap();
        input.document.as_mut().unwrap().categories = vec!["Atenção".into()];
        // The categories are written; marking the step done dies.
        fail_receipt_update(&store, "NEW.categories_step=2");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let id = store.list_documents("").unwrap()[0].id;
        assert_eq!(store.document_categories(id).unwrap().len(), 1);
        store.set_document_categories(id, &[]).unwrap();
        let done = store.import_plan(&input, false).unwrap();
        assert_eq!(done.categories, "interrompida_nao_repetida");
        assert!(store.document_categories(id).unwrap().is_empty());
        assert_eq!(store.list_tasks().unwrap().len(), 3);
    }

    #[test]
    fn only_the_automatic_triage_of_the_record_is_concluded() {
        let (_dir, mut store) = library();
        // The run dies before the triage step.
        fail_receipt_update(&store, "NEW.triage_step=1");
        assert!(store.import_plan(&plan(), false).is_err());
        stop_failing(&store);
        let id = store.list_documents("").unwrap()[0].id;
        // The automatic triage is purged, and the person adds another task
        // "Triar documento" with the action "triar" (what the form proposes).
        let checked = check_plan(&plan()).unwrap();
        let uid = store
            .plan_run(&checked.request)
            .unwrap()
            .unwrap()
            .triage_uid;
        assert!(!uid.is_empty());
        let automatic = store.task_row(&uid).unwrap().unwrap().id;
        purge(&mut store, Kind::Task, automatic);
        let theirs = store.save_task(&Task::new(id)).unwrap();
        let done = store.import_plan(&plan(), false).unwrap();
        assert_eq!(done.triage, "nao_encontrada");
        let open: Vec<_> = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .filter(|t| !t.is_done())
            .map(|t| t.id)
            .collect();
        assert!(open.contains(&theirs), "the person's task is still open");
        assert_eq!(open.len(), 3, "theirs and the two of the plan");
        let again = store.import_plan(&plan(), false).unwrap();
        assert_eq!(again.triage, "nao_encontrada");
        assert!(!again.created);
    }

    #[test]
    fn a_task_written_before_its_progress_mark_is_not_recreated_while_it_is_in_the_trash() {
        let input = big_plan(4);
        let (_dir, mut store) = library();
        shared_folder(&mut store);
        // Task 2 is written; the progress mark that follows it dies.
        fail_receipt_update(&store, "NEW.tasks_created=2");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let second = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.title == "Tarefa 2")
            .unwrap();
        trash(&mut store, Kind::Task, second.id);
        let done = store.import_plan(&input, false).unwrap();
        assert_eq!(done.tasks[1].state, ItemState::Trashed);
        let fresh: Vec<_> = done
            .tasks
            .iter()
            .filter(|t| t.created_now)
            .map(|t| t.index)
            .collect();
        assert_eq!(fresh, vec![3, 4]);
        assert_eq!(store.trash_entries().unwrap().len(), 1);
    }

    /// The accepted limit: only the PERMANENT purge of a task that was written
    /// before its progress mark is forgotten, and then the rerun creates it again.
    #[test]
    fn known_limit_a_task_purged_for_good_before_its_progress_mark_is_created_again() {
        let input = big_plan(4);
        let (_dir, mut store) = library();
        shared_folder(&mut store);
        fail_receipt_update(&store, "NEW.tasks_created=2");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let second = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.title == "Tarefa 2")
            .unwrap();
        purge(&mut store, Kind::Task, second.id);
        let done = store.import_plan(&input, false).unwrap();
        assert_eq!(done.tasks[1].state, ItemState::Present);
        assert!(done.tasks[1].created_now, "created again: the known limit");
    }

    #[test]
    fn a_task_write_that_fails_before_the_next_task_is_finished_by_the_same_command_without_duplicates(
    ) {
        let input = big_plan(5);
        // One transaction: none of the tasks is there after a failure.
        let (_dir, mut store) = library();
        fail_on(
            &store,
            "tasks",
            "NEW.uid IS NOT NULL AND (SELECT count(*) FROM tasks)>=4",
        );
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        assert_eq!(store.list_tasks().unwrap().len(), 1, "only the triage");
        let done = store.import_plan(&input, false).unwrap();
        assert!(done.created && done.tasks.iter().all(|t| t.created_now));
        assert_eq!(store.list_tasks().unwrap().len(), 6);

        // One change per task (shared folder): the first three stay, the
        // rerun creates the other two.
        let (_dir, mut store) = library();
        shared_folder(&mut store);
        fail_on(&store, "tasks", "(SELECT count(*) FROM tasks)>=4");
        let said = run_error(&mut store, &input);
        assert!(
            said.contains("tasks[4]") && said.contains("repita o mesmo comando"),
            "{said}"
        );
        stop_failing(&store);
        assert_eq!(store.list_tasks().unwrap().len(), 4);
        let done = store.import_plan(&input, false).unwrap();
        let fresh: Vec<_> = done
            .tasks
            .iter()
            .filter(|t| t.created_now)
            .map(|t| t.index)
            .collect();
        assert_eq!(fresh, vec![4, 5]);
        assert!(done.tasks.iter().all(|t| t.state == ItemState::Present));
        assert_eq!(store.list_tasks().unwrap().len(), 6);
        // And a third run only reports.
        assert!(!store.import_plan(&input, false).unwrap().created);
    }

    #[test]
    fn a_task_purged_after_its_progress_was_marked_is_not_recreated_by_the_rerun() {
        let input = big_plan(5);
        let (_dir, mut store) = library();
        shared_folder(&mut store);
        fail_on(&store, "tasks", "(SELECT count(*) FROM tasks)>=4");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let rows: Vec<_> = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .filter(|t| t.title == "Tarefa 2")
            .collect();
        purge(&mut store, Kind::Task, rows[0].id);
        let done = store.import_plan(&input, false).unwrap();
        assert_eq!(done.tasks[1].state, ItemState::Missing);
        assert_eq!(done.tasks.iter().filter(|t| t.created_now).count(), 2);
        assert_eq!(
            store.list_tasks().unwrap().len(),
            5,
            "triage + 4 live plan tasks"
        );
    }

    #[test]
    fn an_unfinished_plan_whose_record_is_gone_says_so_instead_of_recreating_it() {
        let input = big_plan(3);
        let (_dir, mut store) = library();
        shared_folder(&mut store);
        fail_on(&store, "tasks", "(SELECT count(*) FROM tasks)>=3");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let id = store.list_documents("").unwrap()[0].id;
        trash(&mut store, Kind::Document, id);
        let said = run_error(&mut store, &input);
        assert!(said.contains("Lixeira"), "{said}");
        store
            .purge_trash_item(Target {
                kind: Kind::Document,
                id,
            })
            .unwrap();
        let said = run_error(&mut store, &input);
        assert!(
            said.contains("não existe mais") && said.contains("request_id novo"),
            "{said}"
        );
        assert!(store.list_documents("").unwrap().is_empty());
    }

    // ---- the modes ------------------------------------------------------------

    #[test]
    fn shared_folder_mode_queues_what_the_screens_queue() {
        let (_dir, mut by_plan) = library();
        shared_folder(&mut by_plan);
        let mut input = plan();
        by_plan
            .save_category(&crate::categories::Category::new("Atenção", 1))
            .unwrap();
        input.document.as_mut().unwrap().categories = vec!["atencao".into()];
        let result = by_plan.import_plan(&input, false).unwrap();
        assert!(result.ids_provisional);

        // The same thing, done the way the screens do it.
        let (_dir, mut by_hand) = library();
        shared_folder(&mut by_hand);
        let category = by_hand
            .save_category(&crate::categories::Category::new("Atenção", 1))
            .unwrap();
        let checked = check_plan(&input).unwrap();
        let created = by_hand
            .create_record_with_attachments(checked.record.as_ref().unwrap(), &[])
            .unwrap();
        let Created::New { document_id, .. } = created else {
            panic!("a new record")
        };
        by_hand
            .save_note(document_id, "Acompanhar o prazo.")
            .unwrap();
        by_hand
            .set_document_categories(document_id, &[category.uid])
            .unwrap();
        for task in &checked.tasks {
            let mut task = task.clone();
            task.document_id = Some(document_id);
            by_hand.save_task(&task).unwrap();
        }
        let triage = Store::triage_task_in(&by_hand.conn, document_id)
            .unwrap()
            .unwrap();
        by_hand.complete_task(triage).unwrap();

        let labels = |store: &Store| {
            store
                .list_offline_outbox()
                .unwrap()
                .into_iter()
                .map(|entry| entry.label)
                .collect::<Vec<_>>()
        };
        let queued = labels(&by_plan);
        assert_eq!(queued, labels(&by_hand));
        assert!(queued.iter().any(|l| l.starts_with("E-mail")));
        assert!(queued.len() >= 1 + 1 + 1 + 2, "{queued:?}");
        assert_eq!(
            by_plan.list_team_events(500).unwrap().len(),
            by_hand.list_team_events(500).unwrap().len()
        );
        // Each queued task carries the identity the plan derived.
        let uids: BTreeSet<String> = by_plan
            .list_offline_outbox()
            .unwrap()
            .into_iter()
            .filter_map(|entry| match entry.mutation {
                crate::intranet::IntranetMutation::SaveTask { task, .. } => Some(task.uid),
                _ => None,
            })
            .collect();
        for task in &check_plan(&input).unwrap().tasks {
            assert!(uids.contains(&task.uid));
        }
    }

    fn team() -> (tempfile::TempDir, Store, HostServer, Store) {
        let dir = tempfile::tempdir().unwrap();
        let host_path = dir.path().join("host.sqlite3");
        let token = crate::intranet::generate_token();
        let mut host = Store::open(&host_path).unwrap();
        let mut host_config = IntranetConfig {
            mode: IntranetMode::Host,
            member_name: "ANFITRIAO".into(),
            bind_addr: "127.0.0.1:0".into(),
            host_url: "http://127.0.0.1:7878".into(),
            cabinet_token: token.clone(),
            auto_start_host: true,
            ..IntranetConfig::default()
        };
        host.save_intranet_config(&host_config).unwrap();
        let server = HostServer::start(host_path, "127.0.0.1:0").unwrap();
        host_config.bind_addr = server.local_addr().to_string();
        host.save_intranet_config(&host_config).unwrap();
        let mut client = Store::open(&dir.path().join("client.sqlite3")).unwrap();
        client
            .save_intranet_config(&IntranetConfig {
                mode: IntranetMode::Client,
                member_name: "CLIENTE".into(),
                bind_addr: "127.0.0.1:0".into(),
                host_url: format!("http://{}", server.local_addr()),
                cabinet_token: token,
                ..IntranetConfig::default()
            })
            .unwrap();
        (dir, host, server, client)
    }

    #[test]
    fn a_connected_computer_sends_the_record_and_each_task_to_the_main_one_and_a_rerun_only_reports(
    ) {
        let (_dir, host, _server, mut client) = team();
        let first = client.import_plan(&plan(), false).unwrap();
        assert!(first.created && first.ids_provisional);
        assert!(first.tasks.iter().all(|t| t.state == ItemState::Present));
        assert_eq!(first.triage, "concluida_agora");
        // The main computer has everything.
        assert_eq!(host.list_documents("").unwrap().len(), 1);
        assert_eq!(host.list_tasks().unwrap().len(), 3);
        // A rerun on the client only reports, and finds the same items.
        let before = host.list_tasks().unwrap().len();
        let again = client.import_plan(&plan(), false).unwrap();
        assert!(!again.created);
        assert_eq!(ids(&again), ids(&first));
        assert_eq!(host.list_tasks().unwrap().len(), before);
    }

    #[test]
    fn a_computer_that_cannot_reach_the_main_one_queues_the_changes_locally_without_replaying_them()
    {
        let (_dir, mut client) = library();
        client
            .save_intranet_config(&IntranetConfig {
                mode: IntranetMode::Client,
                member_name: "CLIENTE".into(),
                host_url: "http://127.0.0.1:1".into(),
                cabinet_token: crate::intranet::generate_token(),
                ..IntranetConfig::default()
            })
            .unwrap();
        let first = client.import_plan(&plan(), false).unwrap();
        assert!(first.created && first.ids_provisional);
        assert_eq!(client.list_tasks().unwrap().len(), 3);
        let queued = client.list_offline_outbox().unwrap().len();
        assert!(queued > 2, "{queued}");
        let again = client.import_plan(&plan(), false).unwrap();
        assert!(!again.created);
        assert_eq!(ids(&again), ids(&first));
        assert_eq!(client.list_offline_outbox().unwrap().len(), queued);
    }

    // ---- an existing document ---------------------------------------------------

    fn existing_plan(number: &str) -> Plan {
        let mut input = big_plan(2);
        input.request_id = uuid::Uuid::new_v4().to_string();
        input.document = None;
        input.document_number = Some(number.into());
        input
    }

    #[test]
    fn tasks_go_to_an_existing_document_found_like_the_app_finds_it() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        let number = first.document.number.clone().unwrap();
        let document_id = first.document.id.unwrap();
        // Spaces and case do not matter.
        let spelled = format!("  {} ", number.to_lowercase().replace('/', " /"));
        let input = existing_plan(&spelled);
        let before = traces(&store);
        let dry = store.import_plan(&input, true).unwrap();
        assert!(dry.document.existing && dry.document.state == ItemState::Present);
        assert_eq!(dry.document.id, Some(document_id));
        assert_eq!(
            traces(&store),
            before,
            "a dry run on an existing record writes nothing"
        );
        let done = store.import_plan(&input, false).unwrap();
        assert!(done.created && !done.document.created_now);
        assert_eq!(done.document.number.as_deref(), Some(number.as_str()));
        assert_eq!(done.triage, "nao_solicitada");
        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 5);
        assert!(tasks.iter().all(|t| t.document_id == Some(document_id)));
        // The triage of the record is as the first plan left it, and the
        // first plan's tasks are untouched.
        assert_eq!(tasks.iter().filter(|t| t.is_done()).count(), 1);
        // A rerun reports, whatever happened to the tasks since.
        trash(&mut store, Kind::Task, done.tasks[0].id.unwrap());
        let again = store.import_plan(&input, false).unwrap();
        assert!(!again.created);
        assert_eq!(again.tasks[0].state, ItemState::Trashed);
        assert_eq!(again.tasks[1].state, ItemState::Present);
    }

    #[test]
    fn an_existing_document_that_is_missing_or_in_the_trash_is_told_apart() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        let number = first.document.number.clone().unwrap();
        let said = run_error(&mut store, &existing_plan("MANUAL-2099/9999"));
        assert!(
            said.contains("document_number") && said.contains("nenhum documento"),
            "{said}"
        );
        trash(&mut store, Kind::Document, first.document.id.unwrap());
        let said = run_error(&mut store, &existing_plan(&number));
        assert!(
            said.contains("document_number") && said.contains("Lixeira"),
            "{said}"
        );
        let before = traces(&store);
        assert!(store.import_plan(&existing_plan(&number), true).is_err());
        assert_eq!(traces(&store), before);
    }

    #[test]
    fn an_existing_plan_finds_its_record_by_its_stable_identity_after_the_number_changed() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        let number = first.document.number.clone().unwrap();
        let input = existing_plan(&number);
        let done = store.import_plan(&input, false).unwrap();
        // The number the file names no longer exists (renumbered by a sync).
        store
            .conn
            .execute(
                "UPDATE documents SET identity_hash=randomblob(32) WHERE id=?1",
                [first.document.id.unwrap()],
            )
            .unwrap();
        let again = store.import_plan(&input, false).unwrap();
        assert!(!again.created);
        assert_eq!(again.document.id, first.document.id);
        assert_eq!(ids(&again), ids(&done));
    }

    #[test]
    fn complete_triage_with_document_number_is_refused_with_a_reason() {
        let mut input = existing_plan("MANUAL-2026/0001");
        input.complete_triage = true;
        let said = message(&input);
        assert!(
            said.contains("complete_triage") && said.contains("registro novo"),
            "{said}"
        );
    }

    // ---- the file --------------------------------------------------------------

    #[test]
    fn files_in_utf8_with_bom_and_utf16_are_read() {
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(FIXTURE.as_bytes());
        let mut little = vec![0xFF, 0xFE];
        little.extend(FIXTURE.encode_utf16().flat_map(u16::to_le_bytes));
        let mut big = vec![0xFE, 0xFF];
        big.extend(FIXTURE.encode_utf16().flat_map(u16::to_be_bytes));
        for bytes in [with_bom, little, big] {
            let plan = parse_plan(&bytes).unwrap();
            assert_eq!(
                plan.document.as_ref().unwrap().title,
                "Solicitação de informações"
            );
            assert_eq!(plan.tasks[0].title, "Examinar a solicitação");
            assert_eq!(plan.request_id, REQUEST);
        }
    }

    #[test]
    fn other_encodings_are_refused_saying_how_to_save_the_file() {
        let ansi: Vec<u8> = FIXTURE
            .chars()
            .map(|c| match c {
                'ç' => 0xE7,
                'ã' => 0xE3,
                c => c as u8,
            })
            .collect();
        let said = parse_plan(&ansi).unwrap_err().to_string();
        assert!(
            said.contains("UTF-8") && said.contains("Salve o arquivo"),
            "{said}"
        );
        let no_mark: Vec<u8> = FIXTURE.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let said = parse_plan(&no_mark).unwrap_err().to_string();
        assert!(said.contains("UTF-16") && said.contains("UTF-8"), "{said}");
        let said = parse_plan(&[0xFF, 0xFE, 0x41]).unwrap_err().to_string();
        assert!(said.contains("UTF-16"), "{said}");
    }

    #[test]
    fn syntax_errors_come_in_portuguese_with_line_and_column() {
        let said = parse_plan(b"{\n  \"request_id\": \"x\",\n}")
            .unwrap_err()
            .to_string();
        assert!(
            said.starts_with("JSON inválido")
                && said.contains("vírgula sobrando")
                && said.contains("linha 3, coluna 1"),
            "{said}"
        );
        let said = parse_plan(b"{\"tasks\": [").unwrap_err().to_string();
        assert!(
            said.contains("termina antes do fim") && said.contains("linha 1"),
            "{said}"
        );
        let said = parse_plan(b"{request_id: 1}").unwrap_err().to_string();
        assert!(said.contains("entre aspas duplas"), "{said}");
        let said = parse_plan(b"[]").unwrap_err().to_string();
        assert!(said.contains("objeto JSON"), "{said}");
        assert!(!said.to_lowercase().contains("expected"), "{said}");
    }

    #[test]
    fn unknown_fields_wrong_types_and_missing_fields_name_the_json_field() {
        let said = |edit: &dyn Fn(&mut Value)| {
            let bytes = edit_fixture(|v| edit(v));
            parse_plan(&bytes).unwrap_err().to_string()
        };
        let text = said(&|v| v["tasks"][1]["prioridade"] = "alta".into());
        assert!(
            text.contains("tasks[2]")
                && text.contains("campo desconhecido \"prioridade\"")
                && text.contains("priority")
                && text.contains("linha ")
                && text.contains("a contagem começa em 1"),
            "{text}"
        );
        let text = said(&|v| v["documento"] = "x".into());
        assert!(text.contains("campo desconhecido \"documento\""), "{text}");
        let text = said(&|v| v["document"]["titulo"] = "x".into());
        assert!(
            text.contains("document: campo desconhecido \"titulo\""),
            "{text}"
        );
        let text = said(&|v| v["tasks"][0]["estimate_minutes"] = "10".into());
        assert!(
            text.contains("tasks[1].estimate_minutes") && text.contains("inteiro"),
            "{text}"
        );
        let text = said(&|v| v["tasks"][0]["estimate_minutes"] = (-5).into());
        assert!(text.contains("tasks[1].estimate_minutes"), "{text}");
        let text = said(&|v| v["tasks"][1]["checklist"] = serde_json::json!([{"texto": "a"}]));
        assert!(
            text.contains("tasks[2].checklist[1]") && text.contains("par"),
            "{text}"
        );
        let text = said(&|v| v["tasks"][0]["title"] = 5.into());
        assert!(
            text.contains("tasks[1].title") && text.contains("texto"),
            "{text}"
        );
        let text = said(&|v| v["complete_triage"] = "sim".into());
        assert!(
            text.contains("complete_triage") && text.contains("true ou false"),
            "{text}"
        );
        let text = said(&|v| {
            v.as_object_mut().unwrap().remove("request_id");
        });
        assert!(text.contains("request_id: campo obrigatório"), "{text}");
        let text = said(&|v| {
            v["document"].as_object_mut().unwrap().remove("title");
        });
        assert!(text.contains("document.title: campo obrigatório"), "{text}");
        let text = said(&|v| {
            v.as_object_mut().unwrap().remove("tasks");
        });
        assert!(text.contains("tasks: campo obrigatório"), "{text}");
    }

    #[test]
    fn the_number_of_tasks_is_between_one_and_one_hundred() {
        let said = message(&big_plan(0));
        assert!(
            said.contains("tasks") && said.contains("nenhuma tarefa"),
            "{said}"
        );
        assert!(check_plan(&big_plan(1)).is_ok());
        assert!(check_plan(&big_plan(100)).is_ok());
        let said = message(&big_plan(101));
        assert!(
            said.contains("101") && said.contains("máximo é 100"),
            "{said}"
        );
    }

    #[test]
    fn errors_name_the_field_the_value_and_the_format() {
        let said = message(&Plan {
            request_id: "isto não é um uuid".into(),
            ..plan()
        });
        assert!(
            said.contains("request_id") && said.contains("UUID"),
            "{said}"
        );
        let mut bad = plan();
        bad.tasks[1].due_date = "22/11/2026".into();
        let said = message(&bad);
        assert!(
            said.starts_with("tasks[2].due_date")
                && said.contains("AAAA-MM-DD")
                && said.contains("\"22/11/2026\"")
                && said.contains("a contagem começa em 1"),
            "{said}"
        );
        assert!(!said.contains("dd/mm/aaaa"), "{said}");
        let mut bad = plan();
        bad.document.as_mut().unwrap().date = "22/11/2026".into();
        let said = message(&bad);
        assert!(
            said.starts_with("document.date") && said.contains("AAAA-MM-DD"),
            "{said}"
        );
        let mut bad = plan();
        bad.document.as_mut().unwrap().date = "2026-2-3".into();
        assert!(message(&bad).contains("com zeros à esquerda"));
        let mut bad = plan();
        bad.tasks[0].due_date = "2026-02-30".into();
        assert!(message(&bad).starts_with("tasks[1].due_date"));
        let mut bad = plan();
        bad.tasks[1].action = "inventar".into();
        let said = message(&bad);
        assert!(
            said.starts_with("tasks[2].action") && said.contains("examinar"),
            "{said}"
        );
        let mut bad = plan();
        bad.tasks[0].title = "   ".into();
        assert!(message(&bad).starts_with("tasks[1].title"));
        let mut bad = plan();
        bad.tasks[0].priority = "máxima".into();
        assert!(message(&bad).starts_with("tasks[1].priority"));
        let mut bad = plan();
        bad.tasks[0].scheduled_start = "2026-12-01T09:00".into();
        assert!(message(&bad).starts_with("tasks[1].scheduled_start"));
        bad.tasks[0].scheduled_end = "2026-12-01T08:00".into();
        assert!(message(&bad).starts_with("tasks[1].scheduled_end"));
        bad.tasks[0].scheduled_end = "01/12/2026 10:00".into();
        assert!(message(&bad).contains("AAAA-MM-DDTHH:MM"));
        let mut bad = plan();
        bad.tasks[0].estimate_minutes = 600_000;
        assert!(message(&bad).starts_with("tasks[1].estimate_minutes"));
        let mut bad = plan();
        bad.document.as_mut().unwrap().kind = "carta".into();
        let said = message(&bad);
        assert!(
            said.starts_with("document.type") && said.contains("memorando"),
            "{said}"
        );
        let mut bad = plan();
        bad.document.as_mut().unwrap().title = "ab".into();
        assert!(message(&bad).starts_with("document.title"));
        let mut both = plan();
        both.document_number = Some("MANUAL-2026/0001".into());
        assert!(message(&both).contains("só um"));
        let mut neither = plan();
        neither.document = None;
        assert!(message(&neither).contains("document_number"));
    }

    // ---- categories and projects --------------------------------------------------

    #[test]
    fn categories_and_projects_are_found_by_name_or_uid_and_never_skipped() {
        let (_dir, mut store) = library();
        let attention = store
            .save_category(&crate::categories::Category::new("Atenção", 1))
            .unwrap();
        let archived = store
            .save_category(&crate::categories::Category {
                archived: true,
                ..crate::categories::Category::new("Antiga", 2)
            })
            .unwrap();
        let project = store
            .save_project(&crate::projects::Project {
                name: "Obra Á".into(),
                ..Default::default()
            })
            .unwrap();
        let old_project = store
            .save_project(&crate::projects::Project {
                name: "Projeto velho".into(),
                status: crate::projects::ProjectStatus::Arquivado,
                ..Default::default()
            })
            .unwrap();
        let before = traces(&store);
        let mut refused = |edit: &dyn Fn(&mut Plan)| {
            let mut input = plan();
            edit(&mut input);
            store.import_plan(&input, true).unwrap_err().to_string()
        };
        let said = refused(&|p| p.tasks[1].categories = vec!["Inexistente".into()]);
        assert!(
            said.contains("tasks[2].categories[1]") && said.contains("categoria desconhecida"),
            "{said}"
        );
        let said = refused(&|p| p.tasks[0].categories = vec!["antiga".into()]);
        assert!(
            said.contains("arquivada") && said.contains(&archived.uid),
            "{said}"
        );
        let said = refused(&|p| p.document.as_mut().unwrap().categories = vec!["x".into()]);
        assert!(said.starts_with("document.categories[1]"), "{said}");
        let said = refused(&|p| p.tasks[0].project = "Outra".into());
        assert!(
            said.contains("tasks[1].project") && said.contains("projeto desconhecida"),
            "{said}"
        );
        let said = refused(&|p| p.tasks[0].project = "projeto velho".into());
        assert!(said.contains("arquivada"), "{said}");
        let said = refused(&|p| p.tasks[0].categories = vec!["Atenção".into(), "ATENCAO".into()]);
        assert!(
            said.contains("tasks[1].categories[2]") && said.contains("mais de uma vez"),
            "{said}"
        );
        assert_eq!(traces(&store), before);

        // Accents and case are ignored; a uid is always accepted, even archived.
        let mut input = plan();
        input.document.as_mut().unwrap().categories = vec!["ATENÇÃO".into()];
        input.tasks[0].categories = vec!["atencao".into(), archived.uid.to_uppercase()];
        input.tasks[0].project = "obra a".into();
        input.tasks[1].project = old_project.uid.clone();
        let done = store.import_plan(&input, false).unwrap();
        let tasks = store.list_tasks().unwrap();
        let first = tasks
            .iter()
            .find(|t| t.id == done.tasks[0].id.unwrap())
            .unwrap();
        assert_eq!(
            first.categories,
            vec![attention.uid.clone(), archived.uid.clone()]
        );
        assert_eq!(first.project_uid, project.uid);
        let second = tasks
            .iter()
            .find(|t| t.id == done.tasks[1].id.unwrap())
            .unwrap();
        assert_eq!(second.project_uid, old_project.uid);
        assert_eq!(
            store
                .document_categories(done.document.id.unwrap())
                .unwrap(),
            vec![attention.uid]
        );
    }

    #[test]
    fn two_entries_with_the_same_name_are_ambiguous_and_the_uid_settles_it() {
        // The library refuses two names alike on one computer, but two
        // members can create the same name offline and meet in a sync.
        let entry = |uid: &str, name: &str| Entry {
            uid: uid.into(),
            name: name.into(),
            archived: false,
        };
        let entries = [
            entry(&"a".repeat(32), "Dupla"),
            entry(&"b".repeat(32), "dupla"),
        ];
        let said = Store::resolve_entry("DUPLA", &entries, "tasks[3].categories[1]", "categoria")
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("tasks[3].categories[1]")
                && said.contains("2 categorias")
                && said.contains(&"b".repeat(32)),
            "{said}"
        );
        assert_eq!(
            Store::resolve_entry(&"B".repeat(32), &entries, "x", "categoria").unwrap(),
            "b".repeat(32)
        );
    }

    // ---- the command line -----------------------------------------------------------

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_options_may_come_in_any_order() {
        let expected = CliOptions {
            file: "plano.json".into(),
            dry_run: true,
            output: Some("saida.json".into()),
        };
        for order in [
            vec!["plano.json", "--dry-run", "--saida", "saida.json"],
            vec!["--dry-run", "plano.json", "--saida", "saida.json"],
            vec!["--saida", "saida.json", "--dry-run", "plano.json"],
            vec!["--saida", "saida.json", "plano.json", "--dry-run"],
        ] {
            assert_eq!(parse_cli(&words(&order)).unwrap(), expected, "{order:?}");
        }
        assert_eq!(
            parse_cli(&words(&["plano.json"])).unwrap(),
            CliOptions {
                file: "plano.json".into(),
                dry_run: false,
                output: None
            }
        );
        for (bad, part) in [
            (vec![], "Informe o nome do arquivo"),
            (vec!["--dry-run"], "Informe o nome do arquivo"),
            (vec!["a.json", "b.json"], "um só arquivo"),
            (vec!["a.json", "--dry-run", "--dry-run"], "duas vezes"),
            (vec!["a.json", "--saida"], "depois de --saida"),
            (vec!["a.json", "--saida", "--dry-run"], "depois de --saida"),
            (vec!["a.json", "--rapido"], "Opção desconhecida: --rapido"),
        ] {
            let said = parse_cli(&words(&bad)).unwrap_err().to_string();
            assert!(
                said.contains(part) && said.contains("Uso:"),
                "{bad:?}: {said}"
            );
        }
    }

    #[test]
    fn the_command_prints_the_result_writes_the_output_file_and_fails_in_words() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        let file = dir.path().join("plano.json");
        std::fs::write(&file, FIXTURE).unwrap();
        let path = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
        let options = |dry: bool, output: Option<&str>| CliOptions {
            file: file.to_str().unwrap().to_owned(),
            dry_run: dry,
            output: output.map(path),
        };

        // A dry run needs a library that is already there and current.
        let mut out = Vec::new();
        let said = execute(&options(true, None), &db, &mut out)
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("Ainda não há acervo") && out.is_empty(),
            "{said}"
        );
        assert!(!db.exists(), "nothing was created");

        // A real run creates the library, prints JSON and writes the file.
        let mut out = Vec::new();
        execute(&options(false, Some("saida1.json")), &db, &mut out).unwrap();
        let printed: Value = serde_json::from_slice(&out).unwrap();
        let written: Value =
            serde_json::from_slice(&std::fs::read(path("saida1.json")).unwrap()).unwrap();
        assert_eq!(printed, written);
        assert_eq!(printed["ok"], true);
        assert_eq!(printed["created"], true);
        assert_eq!(printed["document"]["state"], "present");
        assert_eq!(printed["tasks"][1]["state_label"], "presente");

        // The dry run now reports and writes nothing.
        let mut out = Vec::new();
        execute(&options(true, None), &db, &mut out).unwrap();
        let printed: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(printed["dry_run"], true);
        assert_eq!(printed["created"], false);

        // `--saida` never overwrites, and needs its folder.
        let said = execute(&options(false, Some("saida1.json")), &db, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("já existe") && said.contains("nunca é sobrescrito"),
            "{said}"
        );
        let said = execute(
            &CliOptions {
                output: Some(path("sem-pasta/x.json")),
                ..options(false, None)
            },
            &db,
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(said.contains("não existe"), "{said}");

        // A failure is returned, prints nothing and is written to the file.
        let bad = dir.path().join("ruim.json");
        std::fs::write(
            &bad,
            edit_fixture(|v| v["tasks"][0]["due_date"] = "22/11/2026".into()),
        )
        .unwrap();
        let mut out = Vec::new();
        let failing = CliOptions {
            file: bad.to_str().unwrap().to_owned(),
            dry_run: false,
            output: Some(path("erro.json")),
        };
        let said = execute(&failing, &db, &mut out).unwrap_err().to_string();
        assert!(
            said.starts_with("tasks[1].due_date") && out.is_empty(),
            "{said}"
        );
        let report: Value =
            serde_json::from_slice(&std::fs::read(path("erro.json")).unwrap()).unwrap();
        assert_eq!(report["ok"], false);
        assert!(report["erro"]
            .as_str()
            .unwrap()
            .starts_with("tasks[1].due_date"));

        // A file that is not there says so, with its name.
        let missing = CliOptions {
            file: path("nao-existe.json"),
            ..options(false, None)
        };
        let said = execute(&missing, &db, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("Não foi possível abrir o arquivo do plano")
                && said.contains("nao-existe.json"),
            "{said}"
        );
        let store = Store::open(&db).unwrap();
        assert_eq!(store.list_documents("").unwrap().len(), 1);
    }

    #[test]
    fn a_dry_run_refuses_a_library_that_opening_would_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        drop(Store::open(&db).unwrap());
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
        drop(conn);
        let said = check_readable_as_is(&db).unwrap_err().to_string();
        assert!(
            said.contains("versão mais antiga") && said.contains("--dry-run"),
            "{said}"
        );
        let version = rusqlite::Connection::open(&db)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, 3, "left as it was");
    }

    #[test]
    fn a_busy_library_is_reported_in_portuguese() {
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".into()),
        );
        let wrapped =
            anyhow::Error::new(busy).context("Não foi possível abrir o acervo deste computador.");
        let said = friendly(wrapped).to_string();
        assert!(
            said.contains("ocupado") && said.contains("sem duplicar"),
            "{said}"
        );
        assert!(!said.contains("database is locked"));
        assert_eq!(friendly(anyhow!("outra coisa")).to_string(), "outra coisa");
    }

    #[test]
    fn the_example_file_is_a_valid_plan() {
        let plan = parse_plan(FIXTURE.as_bytes()).unwrap();
        let checked = check_plan(&plan).unwrap();
        assert_eq!(checked.tasks.len(), 2);
        assert_eq!(checked.request, REQUEST);
    }

    // ---- another computer, a renumbered target, the output file ---------------

    #[test]
    fn a_plan_applied_on_another_computer_is_reported_and_nothing_is_written() {
        // This library holds the effects of the plan but no receipt: what a
        // second computer of the same shared library sees after a sync.
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        store
            .conn
            .execute_batch("DROP TABLE plan_import_log")
            .unwrap();
        // One task is in the Lixeira, another was deleted for good there.
        trash(&mut store, Kind::Task, first.tasks[0].id.unwrap());
        let before = traces(&store);
        for dry in [false, true] {
            let answer = store.import_plan(&plan(), dry).unwrap();
            assert!(answer.applied_elsewhere && !answer.created);
            assert_eq!(answer.dry_run, dry);
            assert_eq!(answer.document.state, ItemState::Present);
            assert_eq!(answer.document.id, first.document.id);
            assert_eq!(answer.tasks[0].state, ItemState::Trashed);
            assert_eq!(answer.tasks[1].state, ItemState::Present);
            assert_eq!(answer.note, "nao_verificavel");
            assert_eq!(answer.triage, "nao_verificavel");
            assert_eq!(traces(&store), before);
            assert!(!receipt_table(&store), "no receipt is written for it");
        }
        purge(&mut store, Kind::Task, first.tasks[1].id.unwrap());
        let answer = store.import_plan(&plan(), false).unwrap();
        assert!(answer.applied_elsewhere);
        assert_eq!(answer.tasks[1].state, ItemState::Missing, "not recreated");
        assert_eq!(store.list_tasks().unwrap().len(), 1, "only the triage");
    }

    #[test]
    fn a_plan_on_an_existing_document_is_recognised_as_applied_elsewhere_by_its_task_uids() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        let input = existing_plan(&first.document.number.clone().unwrap());
        let done = store.import_plan(&input, false).unwrap();
        assert!(done.created);
        store
            .conn
            .execute_batch("DROP TABLE plan_import_log")
            .unwrap();
        let before = traces(&store);
        let answer = store.import_plan(&input, false).unwrap();
        assert!(answer.applied_elsewhere && !answer.created);
        assert!(answer.tasks.iter().all(|t| t.state == ItemState::Present));
        assert_eq!(ids(&answer), ids(&done));
        assert_eq!(traces(&store), before);
    }

    #[test]
    fn a_request_id_that_the_library_knows_for_another_record_is_refused_not_adopted() {
        let (_dir, mut store) = library();
        let mut foreign = check_plan(&plan()).unwrap().record.unwrap();
        foreign.fields.title = "Outro registro qualquer".into();
        store.create_record_with_attachments(&foreign, &[]).unwrap();
        let said = run_error(&mut store, &plan());
        assert!(said.contains("outro registro"), "{said}");
        assert_eq!(store.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn a_renumbered_target_whose_old_number_belongs_to_another_document_is_refused_before_writing()
    {
        let (_dir, mut store) = library();
        let a = store.import_plan(&plan(), false).unwrap();
        let mut other = plan();
        other.request_id = uuid::Uuid::new_v4().to_string();
        other.document.as_mut().unwrap().title = "Outro documento do acervo".into();
        let b = store.import_plan(&other, false).unwrap();
        let number = a.document.number.clone().unwrap();
        // A plan on document A, interrupted after its first task.
        shared_folder(&mut store);
        let input = existing_plan(&number);
        fail_on(&store, "tasks", "(SELECT count(*) FROM tasks)>=7");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let before = store.list_tasks().unwrap().len();
        // A synchronisation renumbers A, and the old number is now B's.
        let old_hash: Vec<u8> = store
            .conn
            .query_row(
                "SELECT identity_hash FROM documents WHERE id=?1",
                [a.document.id.unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE documents SET identity_hash=randomblob(32) WHERE id=?1",
                [a.document.id.unwrap()],
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE documents SET identity_hash=?2 WHERE id=?1",
                rusqlite::params![b.document.id.unwrap(), old_hash],
            )
            .unwrap();
        let said = run_error(&mut store, &input);
        assert!(
            said.contains("document_number")
                && said.contains("outro documento")
                && said.contains("Nada foi gravado"),
            "{said}"
        );
        assert_eq!(store.list_tasks().unwrap().len(), before);
        assert!(store
            .list_tasks()
            .unwrap()
            .iter()
            .filter(|t| t.document_id == b.document.id)
            .all(|t| !t.title.starts_with("Tarefa ")));
    }

    #[test]
    fn a_target_that_is_gone_while_its_number_belongs_to_another_document_is_refused() {
        let (_dir, mut store) = library();
        let a = store.import_plan(&plan(), false).unwrap();
        let mut other = plan();
        other.request_id = uuid::Uuid::new_v4().to_string();
        other.document.as_mut().unwrap().title = "Outro documento do acervo".into();
        let b = store.import_plan(&other, false).unwrap();
        let input = existing_plan(&a.document.number.clone().unwrap());
        shared_folder(&mut store);
        fail_on(&store, "tasks", "(SELECT count(*) FROM tasks)>=7");
        assert!(store.import_plan(&input, false).is_err());
        stop_failing(&store);
        let old_hash: Vec<u8> = store
            .conn
            .query_row(
                "SELECT identity_hash FROM documents WHERE id=?1",
                [a.document.id.unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        purge(&mut store, Kind::Document, a.document.id.unwrap());
        store
            .conn
            .execute(
                "UPDATE documents SET identity_hash=?2 WHERE id=?1",
                rusqlite::params![b.document.id.unwrap(), old_hash],
            )
            .unwrap();
        let before = store.list_tasks().unwrap().len();
        let said = run_error(&mut store, &input);
        assert!(
            said.contains("original deste plano não existe mais")
                && said.contains("outro documento"),
            "{said}"
        );
        assert_eq!(store.list_tasks().unwrap().len(), before);
    }

    #[test]
    fn a_target_without_a_creation_request_is_identified_by_its_identity_hash() {
        let (_dir, mut store) = library();
        let a = store.import_plan(&plan(), false).unwrap();
        let id = a.document.id.unwrap();
        // A document with no request row (an older import).
        store
            .conn
            .execute("DELETE FROM requests WHERE document_id=?1", [id])
            .unwrap();
        let identity = store.identity_of_document(id).unwrap();
        assert!(identity.starts_with("identity:"), "{identity}");
        assert_eq!(store.resolve_target(&identity).unwrap(), Some(id));
        let by_request = {
            store
                .conn
                .execute(
                    "INSERT INTO requests(request_id,document_id,capture_id) SELECT '11111111-1111-4111-8111-111111111111',?1,latest_capture_id FROM documents WHERE id=?1",
                    [id],
                )
                .unwrap();
            store.identity_of_document(id).unwrap()
        };
        assert_eq!(by_request, "request:11111111-1111-4111-8111-111111111111");
        assert_eq!(store.resolve_target(&by_request).unwrap(), Some(id));
    }

    #[test]
    fn the_output_file_is_created_before_the_import_and_an_unwritable_one_does_not_hide_it() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("acervo.sqlite3");
        let file = dir.path().join("plano.json");
        std::fs::write(&file, FIXTURE).unwrap();
        let out = dir.path().join("resultado.json");
        let options = CliOptions {
            file: file.to_str().unwrap().to_owned(),
            dry_run: false,
            output: Some(out.to_str().unwrap().to_owned()),
        };
        // The result cannot be written once the import is done.
        let mut stdout = Vec::new();
        let error = execute_with(&options, &db, &mut stdout, &|_, _| {
            Err(std::io::Error::other("disco cheio"))
        })
        .unwrap_err();
        let said = error.to_string();
        assert!(
            error.downcast_ref::<AppliedButUnreported>().is_some()
                && said.contains("FOI aplicada")
                && said.contains("disco cheio")
                && said.contains("resultado.json"),
            "{said}"
        );
        assert_ne!(EXIT_APPLIED_UNREPORTED, 1);
        assert_ne!(EXIT_APPLIED_UNREPORTED, 0);
        // The JSON went to the standard output, and the import is in the library.
        let printed: Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(printed["created"], true);
        let store = Store::open(&db).unwrap();
        assert_eq!(store.list_documents("").unwrap().len(), 1);
        // The file had been created exclusively before the import.
        assert!(out.exists());

        // A file that already exists stops everything before the library is touched.
        let db2 = dir.path().join("outro.sqlite3");
        let again = CliOptions {
            output: options.output.clone(),
            ..CliOptions {
                file: options.file.clone(),
                dry_run: false,
                output: None,
            }
        };
        let said = execute(&again, &db2, &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("já existe") && said.contains("Nada foi importado"),
            "{said}"
        );
        assert!(!db2.exists(), "the library was not even opened");

        // A path that cannot be created fails early too.
        let blocked = CliOptions {
            output: Some(
                dir.path()
                    .join("pasta-que-nao-existe/r.json")
                    .to_str()
                    .unwrap()
                    .to_owned(),
            ),
            ..CliOptions {
                file: options.file.clone(),
                dry_run: false,
                output: None,
            }
        };
        assert!(execute(&blocked, &db2, &mut Vec::new()).is_err());
        assert!(!db2.exists());
    }

    // ---- what a replayed SaveTask does (it bounds the two-computers case) -------

    /// The replay of a queued `SaveTask` whose uid already exists (what the
    /// main computer or a shared folder does when two computers ran the same
    /// plan) does not duplicate the task: equal content answers the existing
    /// id; different content goes through `build_merged_mutation` with no
    /// common ancestor, and the replayed values win the scalar fields where
    /// they differ.
    #[test]
    fn a_replayed_save_task_with_an_existing_uid_merges_into_it_and_the_plan_values_win() {
        let (_dir, mut store) = library();
        let first = store.import_plan(&plan(), false).unwrap();
        let id = first.tasks[0].id.unwrap();
        let checked = check_plan(&plan()).unwrap();
        let mut replayed = checked.tasks[0].clone();
        replayed.document_id = first.document.id;
        let replay = |store: &mut Store, task: &Task| {
            store
                .execute_mutation(
                    "OUTRO",
                    crate::intranet::IntranetMutation::SaveTask {
                        task: task.clone(),
                        expected_version: None,
                        no_project: false,
                    },
                )
                .unwrap()
        };
        // Same content: nothing changes, the existing id comes back.
        let before = store.list_tasks().unwrap();
        match replay(&mut store, &replayed) {
            crate::intranet::MutationReply::Id(found) => assert_eq!(found, id),
            other => panic!("{other:?}"),
        }
        assert_eq!(store.list_tasks().unwrap(), before);

        // Someone edited the task meanwhile.
        let mut edited = before.iter().find(|t| t.id == id).unwrap().clone();
        edited.title = "Título editado pela pessoa".into();
        edited.priority = "baixa".into();
        edited.notes = "Observação da pessoa".into();
        edited.status = "concluida".into();
        edited.checklist.push((false, "Item da pessoa".into()));
        store.save_task(&edited).unwrap();
        replay(&mut store, &replayed);
        let merged = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 3, "no duplicate");
        // The plan's value wins where both differ: title, priority and status.
        assert_eq!(merged.title, "Examinar a solicitação");
        assert_eq!(merged.priority, "alta");
        assert_eq!(merged.status, "pendente");
        // Text and lists are merged, so the person's additions stay.
        assert!(
            merged.notes.contains("Observação da pessoa"),
            "{}",
            merged.notes
        );
        assert!(merged
            .checklist
            .iter()
            .any(|(_, text)| text == "Item da pessoa"));
    }
}
