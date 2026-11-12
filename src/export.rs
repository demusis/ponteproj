//! Export of the main data as CSV files, one per subject, for other tools
//! (the gabinete's CERES panels import `.csv` directly: one row per record,
//! first row with the column titles). Only administrative fields go out:
//! no document text, notes, drafts, observations or attachments.
//!
//! A cell that begins with `=`, `+`, `-` or `@` (after the spaces and tabs
//! that are dropped) would be run as a formula by Excel and LibreOffice, so
//! it goes out with an apostrophe in front (`'=1+1`). No column of the five
//! files legitimately begins with those characters: numbers, hours and ISO
//! dates begin with a digit, and none is negative.
//!
//! Resources have no "tipo": they are told apart by name, `Local` and
//! `Identificação`.
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
    pub resources: usize,
    pub resource_uses: usize,
}

pub struct Table {
    pub headers: Vec<&'static str>,
    pub rows: Vec<Vec<String>>,
}

/// Writes `documentos.csv`, `tarefas.csv`, `atendimentos.csv`, `recursos.csv`
/// and `usos-de-recursos.csv` in a new dated folder under `exportados`.
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
    let resources = store.list_resources()?;
    let resource_uses = resource_uses_table(
        &resources,
        &store.list_resource_uses()?,
        &document_numbers(store)?,
        &attendance_references(store)?,
        &crate::resources::now_moment(),
    );
    let resources = resources_table(&resources);
    for (name, table) in [
        ("documentos.csv", &documents),
        ("tarefas.csv", &tasks),
        ("atendimentos.csv", &attendances),
        ("recursos.csv", &resources),
        ("usos-de-recursos.csv", &resource_uses),
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
        resources: resources.rows.len(),
        resource_uses: resource_uses.rows.len(),
    })
}

fn document_numbers(store: &Store) -> Result<BTreeMap<i64, String>> {
    Ok(store
        .list_documents("")?
        .into_iter()
        .map(|d| (d.id, d.document_number))
        .collect())
}

fn attendance_references(store: &Store) -> Result<BTreeMap<i64, String>> {
    Ok(store
        .list_saiop_attendances()?
        .into_iter()
        .map(|a| (a.id, a.reference_raw.trim().to_owned()))
        .collect())
}

/// `recursos.csv`: one row per resource (no notes).
pub fn resources_table(resources: &[crate::resources::Resource]) -> Table {
    let rows = resources
        .iter()
        .map(|r| {
            vec![
                r.uid.clone(),
                r.name.trim().to_owned(),
                r.quantity.to_string(),
                r.location.trim().to_owned(),
                r.identification.trim().to_owned(),
                r.status.label().to_owned(),
                iso_date(&r.created_at),
            ]
        })
        .collect();
    Table {
        headers: vec![
            "ID",
            "Nome",
            "Quantidade",
            "Local",
            "Identificação",
            "Situação",
            "Criado em",
        ],
        rows,
    }
}

/// `usos-de-recursos.csv`: one row per use (no notes). `documents` and
/// `attendances` name the linked ones by number; `now` (`AAAA-MM-DDTHH:MM`)
/// says what is overdue. The screens pass the slice they show.
pub fn resource_uses_table(
    resources: &[crate::resources::Resource],
    uses: &[crate::resources::ResourceUse],
    documents: &BTreeMap<i64, String>,
    attendances: &BTreeMap<i64, String>,
    now: &str,
) -> Table {
    use crate::resources::{hours_between, moment_date, moment_time};
    let rows = uses
        .iter()
        .map(|u| {
            let resource = resources.iter().find(|r| r.uid == u.resource_uid);
            let until = u.returned_at.as_deref().unwrap_or(&u.end);
            let hours = hours_between(&u.start, until)
                .map(|h| format!("{h:.1}"))
                .unwrap_or_default();
            let returned = u.returned_at.as_deref().unwrap_or("");
            let start_date = moment_date(&u.start);
            vec![
                u.uid.clone(),
                resource
                    .map(|r| r.name.trim().to_owned())
                    .unwrap_or_default(),
                u.quantity.to_string(),
                u.who.trim().to_owned(),
                u.sector.trim().to_owned(),
                u.purpose.trim().to_owned(),
                start_date.clone(),
                moment_time(&u.start),
                moment_date(&u.end),
                moment_time(&u.end),
                moment_date(returned),
                moment_time(returned),
                hours,
                month_of(&start_date),
                u.status.label().to_owned(),
                yes_no(u.overdue_at(now)),
                u.document_id
                    .and_then(|id| documents.get(&id).cloned())
                    .unwrap_or_default(),
                u.saiop_id
                    .and_then(|id| attendances.get(&id).cloned())
                    .unwrap_or_default(),
                yes_no(u.task_id.is_some()),
            ]
        })
        .collect();
    Table {
        headers: vec![
            "ID",
            "Recurso",
            "Quantidade",
            "Quem",
            "Setor",
            "Finalidade",
            "Início (data)",
            "Início (hora)",
            "Fim previsto (data)",
            "Fim previsto (hora)",
            "Devolvido em (data)",
            "Devolvido em (hora)",
            "Duração em horas",
            "Mês",
            "Situação",
            "Atrasado",
            "Documento ligado",
            "Atendimento ligado",
            "Tarefa ligada",
        ],
        rows,
    }
}

/// The text of `usos-de-recursos.csv` for the uses given.
pub fn resource_uses_csv(
    resources: &[crate::resources::Resource],
    uses: &[crate::resources::ResourceUse],
    documents: &BTreeMap<i64, String>,
    attendances: &BTreeMap<i64, String>,
    now: &str,
) -> String {
    render_csv(&resource_uses_table(
        resources,
        uses,
        documents,
        attendances,
        now,
    ))
}

/// The text of `recursos.csv` for the resources given.
pub fn resources_csv(resources: &[crate::resources::Resource]) -> String {
    render_csv(&resources_table(resources))
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
    let guarded;
    let clean = if clean.starts_with(['=', '+', '-', '@']) {
        guarded = format!("'{clean}");
        guarded.as_str()
    } else {
        clean
    };
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
        return at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string();
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
        Some(prefix)
            if chrono::NaiveDate::parse_from_str(&format!("{prefix}-01"), "%Y-%m-%d").is_ok() =>
        {
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
        let meta = |key: &str| {
            metadata
                .get(key)
                .map(String::as_str)
                .unwrap_or("")
                .to_owned()
        };
        let pick = |own: &str, key: &str| {
            if own.trim().is_empty() {
                meta(key)
            } else {
                own.trim().to_owned()
            }
        };
        let own_tasks: Vec<_> = tasks
            .iter()
            .filter(|t| t.document_id == Some(summary.id))
            .collect();
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
            if !summary.manual {
                "SIGADOC"
            } else if summary.email {
                "E-mail"
            } else {
                "Manual"
            }
            .into(),
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
            attachments
                .get(&summary.id)
                .copied()
                .unwrap_or(0)
                .to_string(),
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
            task.document_id
                .and_then(|id| numbers.get(&id).cloned())
                .unwrap_or_default(),
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
    let numbers = document_numbers(store)?;
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
            rows: vec![
                vec!["com; separador".into(), "diz \"olá\"\ne linha".into()],
                vec!["x\u{7}y".into(), " espaço ".into()],
                vec!["=1+1".into(), "\t  +55 65 99999-0000".into()],
                vec!["-2+3".into(), "@SOMA(A1)".into()],
                vec!["=HYPERLINK(\"http://x\";\"a\")".into(), "2026-11-13".into()],
                vec!["1.5".into(), "0".into()],
            ],
        };
        let text = render_csv(&table);
        assert!(text.starts_with("\u{feff}A;B\r\n"));
        let rows = parse(&text);
        assert_eq!(rows[1], ["com; separador", "diz \"olá\"\ne linha"]);
        assert_eq!(rows[2], ["xy", "espaço"]);
        // Formulas are neutralised, also behind spaces and tabs; what the
        // files emit on purpose (ISO dates, numbers) is left as it is.
        assert_eq!(rows[3], ["'=1+1", "'+55 65 99999-0000"]);
        assert_eq!(rows[4], ["'-2+3", "'@SOMA(A1)"]);
        assert_eq!(rows[5], ["'=HYPERLINK(\"http://x\";\"a\")", "2026-11-13"]);
        assert_eq!(rows[6], ["1.5", "0"]);
        assert_eq!(iso_date("2026-11-04T15:38:12.345Z").len(), 10);
        assert_eq!(iso_date("13/11/2026"), "2026-11-13");
        assert_eq!(iso_date("2026-11-13"), "2026-11-13");
        assert_eq!(iso_date("sem data"), "sem data");
        assert_eq!(month_of("2026-11-13"), "2026-11-01");
        assert_eq!(month_of(""), "");
    }

    #[test]
    fn export_writes_five_files_with_documents_tasks_attendances_resources_and_uses() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let mut task = store.list_tasks().unwrap().remove(0);
        task.title = "Responder; com ponto e vírgula".into();
        task.due_date = "2020-01-10".into();
        task.assignee = "SERVIDOR-A".into();
        store.save_task(&task).unwrap();
        let folder = dir.path().join("saida");
        std::fs::create_dir_all(&folder).unwrap();
        let report = export_to(&store, &folder).unwrap();
        // The import records the document's atendimento by itself.
        assert_eq!(
            (report.documents, report.tasks, report.attendances),
            (1, 1, 1)
        );
        assert_eq!((report.resources, report.resource_uses), (0, 0));
        for name in ["recursos.csv", "usos-de-recursos.csv"] {
            let rows = parse(&std::fs::read_to_string(folder.join(name)).unwrap());
            assert_eq!(rows.len(), 1, "{name}: only the header");
        }
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
        assert_eq!(column("Responsável"), "SERVIDOR-A");
        let attendances = std::fs::read_to_string(folder.join("atendimentos.csv")).unwrap();
        assert_eq!(
            parse(&attendances).len(),
            2,
            "header and the automatic atendimento"
        );
        // No document text leaves with the export.
        let all = [
            std::fs::read_to_string(folder.join("documentos.csv")).unwrap(),
            std::fs::read_to_string(folder.join("tarefas.csv")).unwrap(),
        ]
        .join("");
        assert!(!all.contains("papel e material"));
    }
    fn resource_sample() -> (crate::resources::Resource, crate::resources::Resource) {
        let projector = crate::resources::Resource {
            uid: "a".repeat(32),
            name: "Projetor; portátil".into(),
            kind: "equipamento".into(),
            quantity: 2,
            location: "Sala 3".into(),
            identification: "PAT-0001".into(),
            notes: "NOTA_DO_RECURSO_SECRETA".into(),
            created_at: "2026-11-02T10:00:00+00:00".into(),
            ..Default::default()
        };
        let room = crate::resources::Resource {
            uid: "b".repeat(32),
            name: "Sala de reuniões do 2º andar".into(),
            kind: "sala".into(),
            status: crate::resources::ResourceStatus::Inactive,
            ..Default::default()
        };
        (projector, room)
    }

    #[test]
    fn resources_csv_has_the_columns_in_order_and_no_notes() {
        let (projector, room) = resource_sample();
        let text = resources_csv(&[projector, room]);
        assert!(text
            .starts_with("\u{feff}ID;Nome;Quantidade;Local;Identificação;Situação;Criado em\r\n"));
        let rows = parse(&text);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[1],
            [
                "a".repeat(32).as_str(),
                "Projetor; portátil",
                "2",
                "Sala 3",
                "PAT-0001",
                "Ativo",
                "2026-11-02",
            ]
        );
        assert_eq!(rows[2][5], "Fora de uso");
        assert_eq!(rows[2][6], "");
        assert!(!text.contains("Tipo") && !text.contains("Equipamento"));
        assert!(
            text.contains("\"Projetor; portátil\""),
            "the dialect quotes the separator"
        );
        assert!(!text.contains("SECRETA"));
    }

    #[test]
    fn uses_csv_has_the_columns_in_order_the_duration_the_late_flag_and_no_notes() {
        use crate::resources::{ResourceUse, UseStatus};
        let (projector, room) = resource_sample();
        let base_use = |uid: &str, resource: &crate::resources::Resource| ResourceUse {
            uid: uid.into(),
            resource_uid: resource.uid.clone(),
            who: "Servidor A".into(),
            sector: "Setor Jurídico".into(),
            purpose: "Reunião; com ponto e vírgula".into(),
            notes: "NOTA_DO_USO_SECRETA".into(),
            start: "2026-11-10T09:00".into(),
            end: "2026-11-10T10:30".into(),
            ..Default::default()
        };
        let mut late = base_use("l".repeat(32).as_str(), &projector);
        late.status = UseStatus::InUse;
        late.quantity = 2;
        late.document_id = Some(7);
        late.saiop_id = Some(3);
        late.task_id = Some(9);
        let mut returned = base_use("r".repeat(32).as_str(), &room);
        returned.status = UseStatus::Returned;
        returned.returned_at = Some("2026-11-10T11:15".into());
        let reserved = base_use("s".repeat(32).as_str(), &room);
        let documents = BTreeMap::from([(7, "ORG-OFI-2026/00431".to_owned())]);
        let attendances = BTreeMap::from([(3, "ORG-ATD-2026/0007".to_owned())]);
        let text = resource_uses_csv(
            &[projector, room],
            &[late, returned, reserved],
            &documents,
            &attendances,
            "2026-11-10T12:00",
        );
        let rows = parse(&text);
        assert_eq!(
            rows[0],
            [
                "ID",
                "Recurso",
                "Quantidade",
                "Quem",
                "Setor",
                "Finalidade",
                "Início (data)",
                "Início (hora)",
                "Fim previsto (data)",
                "Fim previsto (hora)",
                "Devolvido em (data)",
                "Devolvido em (hora)",
                "Duração em horas",
                "Mês",
                "Situação",
                "Atrasado",
                "Documento ligado",
                "Atendimento ligado",
                "Tarefa ligada",
            ]
        );
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|r| r.len() == rows[0].len()));
        // The loan still out: expected duration, late, with its links.
        assert_eq!(
            rows[1][1..],
            [
                "Projetor; portátil",
                "2",
                "Servidor A",
                "Setor Jurídico",
                "Reunião; com ponto e vírgula",
                "2026-11-10",
                "09:00",
                "2026-11-10",
                "10:30",
                "",
                "",
                "1.5",
                "2026-11-01",
                "Em uso",
                "Sim",
                "ORG-OFI-2026/00431",
                "ORG-ATD-2026/0007",
                "Sim",
            ]
        );
        // The one handed back: the real duration, on time.
        assert_eq!(rows[2][10..13], ["2026-11-10", "11:15", "2.2"]);
        assert_eq!(
            (rows[2][14].as_str(), rows[2][15].as_str()),
            ("Devolvido", "Não")
        );
        assert_eq!(rows[2][16..], ["", "", "Não"]);
        // A reservation is never late.
        assert_eq!(rows[3][15], "Não");
        assert!(!text.contains("SECRETA"), "notes never leave");
        // A use whose resource is gone still goes out.
        let orphan = resource_uses_csv(
            &[],
            &[ResourceUse::default()],
            &documents,
            &attendances,
            "2026-11-10T12:00",
        );
        assert_eq!(parse(&orphan).len(), 2);
    }
}
