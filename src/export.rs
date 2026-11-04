//! Export of the main data as CSV files, one per subject, for other tools
//! (the gabinete's CERES panels import `.csv` directly: one row per record,
//! first row with the column titles). Only administrative fields go out:
//! no document text, notes, drafts, observations or attachments.
use crate::storage::Store;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Separator Brazilian Excel expects; CERES sniffs it.
const SEPARATOR: char = ';';

pub struct ExportReport {
    pub folder: PathBuf,
    pub documents: usize,
    pub tasks: usize,
    pub attendances: usize,
}

pub struct Table {
    pub headers: Vec<&'static str>,
    pub rows: Vec<Vec<String>>,
}

/// Writes `documentos.csv`, `tarefas.csv` and `atendimentos.csv` in a new
/// dated folder under `exportados`.
pub fn export_all(store: &Store) -> Result<ExportReport> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let folder = crate::paths::data_dir()?
        .join("exportados")
        .join(format!("csv-{stamp}"));
    std::fs::create_dir_all(&folder)
        .with_context(|| format!("Não foi possível criar a pasta {}", folder.display()))?;
    export_to(store, &folder)
}

pub fn export_to(store: &Store, folder: &Path) -> Result<ExportReport> {
    let documents = documents_table(store)?;
    let tasks = tasks_table(store)?;
    let attendances = attendances_table(store)?;
    for (name, table) in [
        ("documentos.csv", &documents),
        ("tarefas.csv", &tasks),
        ("atendimentos.csv", &attendances),
    ] {
        let path = folder.join(name);
        std::fs::write(&path, render_csv(table))
            .with_context(|| format!("Não foi possível gravar {}", path.display()))?;
    }
    Ok(ExportReport {
        folder: folder.to_path_buf(),
        documents: documents.rows.len(),
        tasks: tasks.rows.len(),
        attendances: attendances.rows.len(),
    })
}

/// UTF-8 with BOM (Excel opens it with accents), CRLF, fields quoted when
/// they carry the separator, quotes or line breaks.
pub fn render_csv(table: &Table) -> String {
    let mut out = String::from("\u{feff}");
    let line = |fields: &[String]| -> String {
        fields
            .iter()
            .map(|f| field(f))
            .collect::<Vec<_>>()
            .join(&SEPARATOR.to_string())
    };
    let headers: Vec<String> = table.headers.iter().map(|h| h.to_string()).collect();
    out.push_str(&line(&headers));
    out.push_str("\r\n");
    for row in &table.rows {
        out.push_str(&line(row));
        out.push_str("\r\n");
    }
    out
}

fn field(value: &str) -> String {
    let clean: String = value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    let clean = clean.trim();
    if clean.contains(SEPARATOR) || clean.contains('"') || clean.contains('\n') {
        format!("\"{}\"", clean.replace('"', "\"\""))
    } else {
        clean.to_owned()
    }
}

/// `AAAA-MM-DD` from what the library stores (RFC 3339, ISO date or the
/// `dd/mm/aaaa` typed by people); anything else goes out as it is.
fn iso_date(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    if let Ok(at) = chrono::DateTime::parse_from_rfc3339(value) {
        return at.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string();
    }
    if chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() {
        return value.to_owned();
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%d/%m/%Y") {
        return date.format("%Y-%m-%d").to_string();
    }
    if let Some(prefix) = value.get(..10) {
        if chrono::NaiveDate::parse_from_str(prefix, "%Y-%m-%d").is_ok() {
            return prefix.to_owned();
        }
    }
    value.to_owned()
}

/// First day of the month of an ISO date ("AAAA-MM-01"), the form the
/// panels use for monthly series.
fn month_of(iso: &str) -> String {
    match iso.get(..7) {
        Some(prefix) if chrono::NaiveDate::parse_from_str(&format!("{prefix}-01"), "%Y-%m-%d").is_ok() => {
            format!("{prefix}-01")
        }
        _ => String::new(),
    }
}

fn yes_no(value: bool) -> String {
    if value { "Sim" } else { "Não" }.to_owned()
}

fn documents_table(store: &Store) -> Result<Table> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let tasks = store.list_tasks()?;
    let attachments = store.attachment_counts("document").unwrap_or_default();
    let mut rows = Vec::new();
    for summary in store.list_documents("")? {
        let detail = store.load_document(summary.id)?;
        let metadata = &detail.capture.metadata;
        let context = &detail.work_context;
        let meta = |key: &str| metadata.get(key).map(String::as_str).unwrap_or("").to_owned();
        let pick = |own: &str, key: &str| {
            if own.trim().is_empty() {
                meta(key)
            } else {
                own.trim().to_owned()
            }
        };
        let own_tasks: Vec<_> = tasks.iter().filter(|t| t.document_id == summary.id).collect();
        let open = own_tasks.iter().filter(|t| t.status != "concluida").count();
        let overdue = own_tasks
            .iter()
            .filter(|t| t.status != "concluida" && !t.due_date.is_empty() && t.due_date < today)
            .count();
        let done = own_tasks.len() - open;
        let next_due = own_tasks
            .iter()
            .filter(|t| t.status != "concluida" && !t.due_date.is_empty())
            .map(|t| t.due_date.as_str())
            .min()
            .unwrap_or("")
            .to_owned();
        let imported = iso_date(&summary.last_seen_at);
        rows.push(vec![
            summary.id.to_string(),
            summary.document_number.clone(),
            summary.title.clone(),
            "SIGADOC".into(),
            pick(&context.document_type, "document_type"),
            iso_date(&meta("document_date")),
            imported.clone(),
            month_of(&imported),
            pick(&context.process_number, "process_number"),
            pick(&context.sender, "origin"),
            iso_date(&pick(&context.received_date, "received_at")),
            crate::work_context::purpose_label(&context.purpose).to_owned(),
            summary.subjects.join("; "),
            crate::labels::priority(&summary.priority).to_owned(),
            summary.joined_to_number.clone().unwrap_or_default(),
            summary.joined_children_count.to_string(),
            open.to_string(),
            overdue.to_string(),
            done.to_string(),
            next_due,
            attachments.get(&summary.id).copied().unwrap_or(0).to_string(),
        ]);
    }
    Ok(Table {
        headers: vec![
            "ID",
            "Documento",
            "Título",
            "Origem",
            "Tipo",
            "Data documental",
            "Importado em",
            "Mês de importação",
            "Processo",
            "Remetente",
            "Recebido em",
            "Finalidade",
            "Assuntos",
            "Prioridade",
            "Juntado a",
            "Documentos juntados",
            "Tarefas abertas",
            "Tarefas atrasadas",
            "Tarefas concluídas",
            "Próximo prazo",
            "Anexos",
        ],
        rows,
    })
}

fn tasks_table(store: &Store) -> Result<Table> {
    let today = chrono::Local::now().date_naive();
    let numbers: BTreeMap<i64, String> = store
        .list_documents("")?
        .into_iter()
        .map(|d| (d.id, d.document_number))
        .collect();
    let attachments = store.attachment_counts("task").unwrap_or_default();
    let mut rows = Vec::new();
    for task in store.list_tasks()? {
        let due = iso_date(&task.due_date);
        let overdue = task.overdue();
        let late_days = if overdue {
            chrono::NaiveDate::parse_from_str(&due, "%Y-%m-%d")
                .map(|d| (today - d).num_days().max(0).to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        rows.push(vec![
            task.id.to_string(),
            numbers.get(&task.document_id).cloned().unwrap_or_default(),
            task.title.clone(),
            crate::tasks::action_label(&task.action).to_owned(),
            crate::tasks::status_label(&task.status).to_owned(),
            crate::tasks::priority_label(&task.priority).to_owned(),
            task.assignee.trim().to_owned(),
            due.clone(),
            month_of(&due),
            yes_no(overdue),
            late_days,
            iso_date(task.completed_at.as_deref().unwrap_or("")),
            task.dependencies.len().to_string(),
            attachments.get(&task.id).copied().unwrap_or(0).to_string(),
        ]);
    }
    Ok(Table {
        headers: vec![
            "ID",
            "Documento",
            "Tarefa",
            "Ação",
            "Situação",
            "Prioridade",
            "Responsável",
            "Prazo",
            "Mês do prazo",
            "Atrasada",
            "Dias de atraso",
            "Concluída em",
            "Dependências",
            "Anexos",
        ],
        rows,
    })
}

fn attendances_table(store: &Store) -> Result<Table> {
    let numbers: BTreeMap<i64, String> = store
        .list_documents("")?
        .into_iter()
        .map(|d| (d.id, d.document_number))
        .collect();
    let mut rows = Vec::new();
    for item in store.list_saiop_attendances()? {
        let entry = iso_date(&item.entry_date);
        rows.push(vec![
            item.id.to_string(),
            item.reference_raw.trim().to_owned(),
            item.sigadoc_code.trim().to_owned(),
            item.subject.trim().to_owned(),
            entry.clone(),
            month_of(&entry),
            item.assignee.trim().to_owned(),
            item.received_from.trim().to_owned(),
            crate::saiop::status_label(&item.status).to_owned(),
            crate::saiop::priority_label(&item.priority).to_owned(),
            iso_date(&item.due_date),
            crate::saiop::due_kind_label(&item.due_kind).to_owned(),
            crate::saiop::tri_state_label(&item.uas_return).to_owned(),
            crate::saiop::tri_state_label(&item.signed_status).to_owned(),
            iso_date(&item.exit_date),
            item.sent_to.trim().to_owned(),
            item.outcome.trim().to_owned(),
            item.document_id
                .and_then(|id| numbers.get(&id).cloned())
                .unwrap_or_default(),
            iso_date(&item.created_at),
            iso_date(&item.closed_at),
        ]);
    }
    Ok(Table {
        headers: vec![
            "ID",
            "Referência",
            "Código SIGADOC",
            "Assunto",
            "Data de entrada",
            "Mês de entrada",
            "Responsável",
            "Recebido de",
            "Situação",
            "Prioridade",
            "Prazo",
            "Tipo de prazo",
            "Retorno UAS",
            "Assinado",
            "Data de saída",
            "Enviado para",
            "Resultado",
            "Documento ligado",
            "Criado em",
            "Encerrado em",
        ],
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal reader of the dialect written above, to prove round trips.
    fn parse(csv: &str) -> Vec<Vec<String>> {
        let text = csv.strip_prefix('\u{feff}').unwrap_or(csv);
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut cell = String::new();
        let mut quoted = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                ('"', _) => quoted = !quoted,
                (SEPARATOR, false) => row.push(std::mem::take(&mut cell)),
                ('\r', false) => {}
                ('\n', false) => {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                }
                (c, _) => cell.push(c),
            }
        }
        rows
    }

    #[test]
    fn csv_quotes_separators_quotes_and_line_breaks_and_dates_become_iso() {
        let table = Table {
            headers: vec!["A", "B"],
            rows: vec![vec!["com; separador".into(), "diz \"olá\"\ne linha".into()], vec!["x\u{7}y".into(), " espaço ".into()]],
        };
        let text = render_csv(&table);
        assert!(text.starts_with("\u{feff}A;B\r\n"));
        let rows = parse(&text);
        assert_eq!(rows[1], ["com; separador", "diz \"olá\"\ne linha"]);
        assert_eq!(rows[2], ["xy", "espaço"]);
        assert_eq!(iso_date("2026-11-04T15:38:12.345Z").len(), 10);
        assert_eq!(iso_date("13/11/2026"), "2026-11-13");
        assert_eq!(iso_date("2026-11-13"), "2026-11-13");
        assert_eq!(iso_date("sem data"), "sem data");
        assert_eq!(month_of("2026-11-13"), "2026-11-01");
        assert_eq!(month_of(""), "");
    }

    #[test]
    fn export_writes_three_files_with_documents_tasks_and_attendances() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let receipt = store.import_capture(&crate::model::sample_capture()).unwrap();
        let mut task = store.list_tasks().unwrap().remove(0);
        task.title = "Responder; com ponto e vírgula".into();
        task.due_date = "2020-01-10".into();
        task.assignee = "MARIA".into();
        store.save_task(&task).unwrap();
        let folder = dir.path().join("saida");
        std::fs::create_dir_all(&folder).unwrap();
        let report = export_to(&store, &folder).unwrap();
        assert_eq!((report.documents, report.tasks, report.attendances), (1, 1, 0));
        let documents = parse(&std::fs::read_to_string(folder.join("documentos.csv")).unwrap());
        assert_eq!(documents[0][1], "Documento");
        let header = &documents[0];
        let row = &documents[1];
        let column = |name: &str| row[header.iter().position(|h| h == name).unwrap()].clone();
        assert_eq!(column("Documento"), "SEFAZ-PRO-2026/00001");
        assert_eq!(column("Origem"), "SIGADOC");
        assert_eq!(column("Tarefas abertas"), "1");
        assert_eq!(column("Tarefas atrasadas"), "1");
        assert_eq!(column("Próximo prazo"), "2020-01-10");
        assert_eq!(column("ID"), receipt.document_id.to_string());
        let tasks = parse(&std::fs::read_to_string(folder.join("tarefas.csv")).unwrap());
        let header = &tasks[0];
        let row = &tasks[1];
        let column = |name: &str| row[header.iter().position(|h| h == name).unwrap()].clone();
        assert_eq!(column("Tarefa"), "Responder; com ponto e vírgula");
        assert_eq!(column("Atrasada"), "Sim");
        assert!(column("Dias de atraso").parse::<i64>().unwrap() > 1000);
        assert_eq!(column("Mês do prazo"), "2020-01-01");
        assert_eq!(column("Responsável"), "MARIA");
        let attendances = std::fs::read_to_string(folder.join("atendimentos.csv")).unwrap();
        assert_eq!(parse(&attendances).len(), 1, "header only");
        // No document text leaves with the export.
        let all = [
            std::fs::read_to_string(folder.join("documentos.csv")).unwrap(),
            std::fs::read_to_string(folder.join("tarefas.csv")).unwrap(),
        ]
        .join("");
        assert!(!all.contains("papel e material"));
    }
}
