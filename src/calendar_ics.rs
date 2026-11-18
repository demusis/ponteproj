//! iCalendar export and reviewed, partial import of representable entries.
use anyhow::{bail, ensure, Context, Result};
use chrono::NaiveDate;
use crate::{resources::{Resource, ResourceUse}, tasks::Task};

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace("\r\n", "\n").replace('\r', "\n")
        .replace('\n', "\\n").replace(';', "\\;").replace(',', "\\,")
}

fn line(out: &mut String, value: &str) {
    let mut width = 0;
    for ch in value.chars() {
        if width + ch.len_utf8() > 75 { out.push_str("\r\n "); width = 1; }
        out.push(ch); width += ch.len_utf8();
    }
    out.push_str("\r\n");
}

fn date(value: &str) -> Result<String> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").context("Data de calendário inválida.")?;
    ensure!(date.to_string() == value, "Data deve usar AAAA-MM-DD.");
    Ok(date.format("%Y%m%d").to_string())
}
fn moment(value: &str) -> Result<String> {
    Ok(crate::resources::parse_moment(value)?.format("%Y%m%dT%H%M%S").to_string())
}

/// The SUMMARY of a resource use: its "Finalidade" or, when that is empty (it
/// is optional), "Reserva de <recurso>". An empty SUMMARY would make Ponte's
/// own importer refuse the whole file.
fn use_summary(item: &ResourceUse, resources: &[Resource]) -> String {
    let purpose = item.purpose.trim();
    if !purpose.is_empty() { return escape(purpose); }
    let name = resources.iter().find(|r| r.uid == item.resource_uid).map(|r| r.name.trim()).filter(|n| !n.is_empty());
    escape(&format!("Reserva de {}", name.unwrap_or("recurso")))
}

/// `resources` only gives names to the reservations without finalidade.
pub fn export_calendar(tasks: &[Task], uses: &[ResourceUse], resources: &[Resource], private: bool) -> Result<String> {
    let mut out = String::new();
    for header in ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Ponte//Agenda//PT-BR", "CALSCALE:GREGORIAN"] { line(&mut out, header); }
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    for task in tasks {
        ensure!(!task.uid.is_empty(), "Salve a tarefa antes de exportar: falta sua identificação estável.");
        line(&mut out, "BEGIN:VTODO");
        line(&mut out, &format!("UID:{}@ponte", escape(&task.uid)));
        line(&mut out, &format!("DTSTAMP:{stamp}"));
        line(&mut out, &format!("SUMMARY:{}", if private { "Tarefa reservada".to_owned() } else { escape(&task.title) }));
        // RFC5545 does not permit a DATE DUE alongside a DATE-TIME DTSTART.
        // Keep the contractual deadline as DATE; carry the work block as a
        // separate event sharing a RELATED-TO reference below.
        if !task.due_date.is_empty() { line(&mut out, &format!("DUE;VALUE=DATE:{}", date(&task.due_date)?)); }
        line(&mut out, if task.is_done() { "STATUS:COMPLETED" } else { "STATUS:NEEDS-ACTION" });
        if !private && !task.notes.is_empty() { line(&mut out, &format!("DESCRIPTION:{}", escape(&task.notes))); }
        line(&mut out, "END:VTODO");
        if !task.scheduled_start.is_empty() {
            line(&mut out, "BEGIN:VEVENT");
            line(&mut out, &format!("UID:{}-work@ponte", escape(&task.uid)));
            line(&mut out, &format!("DTSTAMP:{stamp}"));
            line(&mut out, &format!("RELATED-TO:{}@ponte", escape(&task.uid)));
            line(&mut out, &format!("SUMMARY:{}", if private { "Trabalho reservado".to_owned() } else { escape(&task.title) }));
            line(&mut out, &format!("DTSTART:{}", moment(&task.scheduled_start)?));
            line(&mut out, &format!("DTEND:{}", moment(&task.scheduled_end)?));
            line(&mut out, "END:VEVENT");
        }
    }
    for item in uses.iter().filter(|u| u.status.occupying()) {
        ensure!(!item.uid.is_empty(), "A reserva ainda não foi salva.");
        line(&mut out, "BEGIN:VEVENT");
        line(&mut out, &format!("UID:{}@ponte", escape(&item.uid)));
        line(&mut out, &format!("DTSTAMP:{stamp}"));
        line(&mut out, &format!("SUMMARY:{}", if private { "Recurso reservado".to_owned() } else { use_summary(item, resources) }));
        line(&mut out, &format!("DTSTART:{}", moment(&item.start)?));
        line(&mut out, &format!("DTEND:{}", moment(&item.end)?));
        line(&mut out, "END:VEVENT");
    }
    line(&mut out, "END:VCALENDAR");
    Ok(out)
}

#[derive(Clone, Debug, Default)]
pub struct ImportedEntry {
    pub uid: String,
    pub title: String,
    pub notes: String,
    pub start: String,
    pub end: String,
    pub due: String,
    pub completed: bool,
    pub related_to: String,
    pub details: Vec<String>,
    event: bool,
}

fn unescape(value: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' { out.push(ch); continue; }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'), Some(c @ ('\\' | ';' | ',')) => out.push(c),
            _ => bail!("Escape inválido no calendário."),
        }
    }
    Ok(out)
}

fn parse_date(value: &str) -> Result<String> {
    ensure!(value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit()), "Data ICS inválida.");
    Ok(NaiveDate::parse_from_str(value, "%Y%m%d")?.to_string())
}
fn parse_time(value: &str) -> Result<String> {
    let utc = value.ends_with('Z');
    let raw = value.strip_suffix('Z').unwrap_or(value);
    ensure!(raw.len() == 15 && raw.ends_with("00"), "horário com precisão inferior a um minuto não suportado");
    let parsed = chrono::NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%S")?;
    let local = if utc { parsed.and_utc().with_timezone(&chrono::Local).naive_local() } else { parsed };
    ensure!(local.format("%S").to_string() == "00", "horário local com segundos não suportado");
    Ok(local.format("%Y-%m-%dT%H:%M").to_string())
}

#[derive(Clone, Debug)]
pub struct SkippedEntry { pub title: String, pub reason: String }
#[derive(Clone, Debug, Default)]
pub struct CalendarReview {
    pub entries: Vec<ImportedEntry>,
    pub skipped: Vec<SkippedEntry>,
    pub general_ignored: bool,
}
// Keep the existing round-trip callers convenient.
impl std::ops::Deref for CalendarReview {
    type Target = [ImportedEntry];
    fn deref(&self) -> &Self::Target { &self.entries }
}
fn add_detail(details: &mut Vec<String>, detail: &str) {
    if !details.iter().any(|d| d == detail) { details.push(detail.to_owned()); }
}

pub fn parse_calendar(text: &str) -> Result<CalendarReview> {
    ensure!(text.len() <= 4_000_000, "Calendário grande demais (máximo 4 MB).");
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        if raw.starts_with([' ', '\t']) {
            lines.last_mut().context("Continuação ICS sem linha anterior.")?.push_str(&raw[1..]);
        } else if !raw.is_empty() { lines.push(raw.to_owned()); }
    }
    ensure!(lines.first().map(String::as_str) == Some("BEGIN:VCALENDAR") && lines.last().map(String::as_str) == Some("END:VCALENDAR"), "Arquivo não é um VCALENDAR completo.");
    let mut review = CalendarReview::default();
    let mut current: Option<ImportedEntry> = None;
    let mut reasons = Vec::new();
    let mut fields = std::collections::BTreeSet::new();
    let mut skip_depth = 0usize;
    let mut all_day = false;
    let mut all_day_end = String::new();
    let mut count = 0usize;
    for raw in &lines[1..lines.len()-1] {
        let (key, value) = raw.split_once(':').context("Linha ICS inválida.")?;
        if skip_depth > 0 {
            if key == "BEGIN" { skip_depth += 1; } else if key == "END" { skip_depth -= 1; }
            continue;
        }
        if key == "BEGIN" {
            if current.is_none() && matches!(value, "VEVENT" | "VTODO") {
                count += 1; ensure!(count <= 500, "Máximo de 500 entradas por importação.");
                current = Some(ImportedEntry { event: value == "VEVENT", ..Default::default() });
                fields.clear(); reasons.clear(); all_day = false; all_day_end.clear();
            } else {
                if let Some(entry) = current.as_mut() {
                    add_detail(&mut entry.details, if value == "VALARM" { "alarmes" } else { value });
                } else if value == "VTIMEZONE" { review.general_ignored = true; }
                else { review.skipped.push(SkippedEntry { title: format!("Componente {value}"), reason: "componente não suportado".into() }); }
                skip_depth = 1;
            }
            continue;
        }
        if key == "END" {
            let mut entry = current.take().context("Fim de entrada sem início.")?;
            ensure!(value == if entry.event { "VEVENT" } else { "VTODO" }, "Fim de entrada incompatível.");
            if entry.uid.is_empty() { add_detail(&mut reasons, "entrada sem UID"); }
            if entry.title.trim().is_empty() { add_detail(&mut reasons, "entrada sem título"); }
            if !all_day && !all_day_end.is_empty() { add_detail(&mut reasons, "fim de dia inteiro incompatível com início"); }
            if all_day {
                if !entry.end.is_empty() || !entry.start.is_empty() { add_detail(&mut reasons, "horários incompatíveis com evento de dia inteiro"); }
                if !entry.event { add_detail(&mut reasons, "início em dia inteiro de tarefa não suportado"); }
                if !all_day_end.is_empty() {
                    let next = NaiveDate::parse_from_str(&entry.due, "%Y-%m-%d").ok().and_then(|d| d.succ_opt()).map(|d| d.to_string());
                    if next.as_deref() != Some(all_day_end.as_str()) { add_detail(&mut reasons, "evento de vários dias não pode ser representado por um único prazo"); }
                }
                add_detail(&mut entry.details, "evento de dia inteiro importado como tarefa com prazo nesse dia");
            } else {
                if entry.start.is_empty() != entry.end.is_empty() { add_detail(&mut reasons, "início e fim são necessários"); }
                if entry.event && entry.start.is_empty() { add_detail(&mut reasons, "evento sem intervalo de horário"); }
                if !entry.end.is_empty() && entry.end <= entry.start { add_detail(&mut reasons, "fim deve ser posterior ao início"); }
            }
            if !reasons.is_empty() && !entry.details.is_empty() { reasons.push(format!("detalhes omitidos: {}", entry.details.join("; "))); }
            if reasons.is_empty() { review.entries.push(entry); }
            else { review.skipped.push(SkippedEntry { title: if entry.title.trim().is_empty() { "Entrada sem título".into() } else { entry.title }, reason: reasons.join("; ") }); }
            continue;
        }
        if let Some(entry) = current.as_mut() {
            let name = key.split(';').next().unwrap_or(key);
            if matches!(name, "RRULE" | "RDATE" | "EXDATE") { add_detail(&mut reasons, "repetição: Ponte importaria apenas uma ocorrência"); continue; }
            if matches!(name, "UID" | "SUMMARY" | "DESCRIPTION" | "RELATED-TO" | "DTSTART" | "DTEND" | "DUE" | "STATUS" | "LOCATION") && !fields.insert(name.to_owned()) {
                add_detail(&mut reasons, &format!("propriedade repetida: {name}")); continue;
            }
            let params = key.split(';').skip(1).collect::<Vec<_>>();
            if matches!(name, "DTSTART" | "DTEND" | "DUE") && params.iter().any(|p| p.to_ascii_uppercase().starts_with("TZID=")) {
                add_detail(&mut reasons, "fuso horário não suportado"); continue;
            }
            let date_value = params.iter().any(|p| p.eq_ignore_ascii_case("VALUE=DATE"));
            if matches!(name, "UID" | "SUMMARY" | "DESCRIPTION" | "RELATED-TO" | "DTSTART" | "DTEND" | "DUE" | "STATUS" | "LOCATION") && !params.is_empty() && !(date_value && params.len() == 1 && matches!(name, "DTSTART" | "DTEND" | "DUE")) {
                add_detail(&mut reasons, &format!("parâmetros não suportados em {name}")); continue;
            }
            let result: Result<()> = (|| {
                match name {
                    "UID" => entry.uid = unescape(value)?, "SUMMARY" => entry.title = unescape(value)?,
                    "DESCRIPTION" => { let description = unescape(value)?; if entry.notes.is_empty() { entry.notes = description; } else { entry.notes = format!("{description}\n{}", entry.notes); } },
                    "LOCATION" => { let location = unescape(value)?; if !entry.notes.is_empty() { entry.notes.push('\n'); } entry.notes.push_str(&format!("Local: {location}")); },
                    "RELATED-TO" => entry.related_to = unescape(value)?,
                    "DTSTART" if date_value => { all_day = true; entry.due = parse_date(value)?; },
                    "DTEND" if date_value => all_day_end = parse_date(value)?,
                    "DTSTART" => entry.start = parse_time(value)?, "DTEND" => entry.end = parse_time(value)?,
                    "DUE" if date_value => entry.due = parse_date(value)?,
                    "DUE" => bail!("prazo com horário não suportado"),
                    "STATUS" => match value { "COMPLETED" => entry.completed = true, "NEEDS-ACTION" | "CONFIRMED" | "IN-PROCESS" => {}, _ => bail!("situação não suportada: {value}") },
                    "ATTENDEE" => add_detail(&mut entry.details, "convidados"),
                    "ORGANIZER" => add_detail(&mut entry.details, "organizador"),
                    "DTSTAMP" | "CREATED" | "LAST-MODIFIED" | "SEQUENCE" | "TRANSP" | "CLASS" => add_detail(&mut entry.details, name),
                    _ => add_detail(&mut entry.details, &format!("propriedade {name}")),
                }
                Ok(())
            })();
            if let Err(error) = result { add_detail(&mut reasons, &format!("{name}: {error}")); }
        } else if matches!(key, "VERSION" | "PRODID" | "CALSCALE" | "METHOD" | "X-WR-CALNAME" | "X-WR-TIMEZONE" | "X-WR-CALDESC") {
            review.general_ignored = true;
        } else {
            review.skipped.push(SkippedEntry { title: format!("Propriedade do calendário {key}"), reason: "propriedade não suportada".into() });
        }
    }
    ensure!(current.is_none() && skip_depth == 0, "Entrada incompleta.");
    let mut uids = std::collections::BTreeSet::new();
    review.entries.retain(|entry| {
        if uids.insert(entry.uid.clone()) { true } else {
            review.skipped.push(SkippedEntry { title: entry.title.clone(), reason: "UID repetido no arquivo".into() }); false
        }
    });
    let blocks: Vec<_> = review.entries.iter().filter(|e| e.event && !e.related_to.is_empty()).cloned().collect();
    for block in blocks {
        if let Some(task) = review.entries.iter_mut().find(|e| !e.event && e.uid == block.related_to && e.start.is_empty() && e.title == block.title && block.notes.is_empty() && block.due.is_empty() && !block.completed) {
            task.start = block.start.clone(); task.end = block.end.clone();
            for detail in &block.details { add_detail(&mut task.details, detail); }
            review.entries.retain(|e| e.uid != block.uid);
        } else if let Some(entry) = review.entries.iter_mut().find(|e| e.uid == block.uid) {
            add_detail(&mut entry.details, "relação com outra entrada não incorporada");
        }
    }
    Ok(review)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_folds_unicode_escapes_injection_and_rejoins_work() {
        let mut task = Task::standalone(); task.uid = "test-task".into();
        task.title = format!("{};\\,\nBEGIN:VEVENT", "ação".repeat(40));
        task.due_date = "2026-11-20".into(); task.scheduled_start = "2026-11-18T09:00".into(); task.scheduled_end = "2026-11-18T10:30".into();
        let text = export_calendar(&[task.clone()], &[], &[], false).unwrap();
        assert!(text.split("\r\n").all(|l| l.len() <= 75));
        let imported = parse_calendar(&text).unwrap();
        assert_eq!(imported.len(), 1); assert_eq!(imported[0].title, task.title);
        assert_eq!(imported[0].due, task.due_date); assert_eq!(imported[0].start, task.scheduled_start);
        assert!(!export_calendar(&[task], &[], &[], true).unwrap().contains("ação"));
    }
    fn task(uid: &str) -> Task { let mut t = Task::standalone(); t.uid = uid.into(); t.title = "Teste".into(); t }

    #[test]
    fn a_use_without_finalidade_exports_a_summary_and_imports_back() {
        let resource = Resource { uid: "res-1".into(), name: "Sala 2; térreo".into(), ..Resource::default() };
        let mut item = ResourceUse { uid: "use-1".into(), resource_uid: "res-1".into(), start: "2026-11-18T09:00".into(), end: "2026-11-18T10:00".into(), ..ResourceUse::default() };
        assert!(item.purpose.is_empty() && item.status.occupying());
        for blank in ["", "   "] {
            item.purpose = blank.into();
            let text = export_calendar(&[], &[item.clone()], &[resource.clone()], false).unwrap();
            assert!(!text.contains("SUMMARY:\r\n"), "{text}");
            let imported = parse_calendar(&text).unwrap();
            assert_eq!(imported.len(), 1);
            assert_eq!(imported[0].title, "Reserva de Sala 2; térreo");
            assert_eq!((imported[0].start.as_str(), imported[0].end.as_str()), ("2026-11-18T09:00", "2026-11-18T10:00"));
        }
        // The resource is not in the list (or has no name): still a usable summary.
        let text = export_calendar(&[], &[item.clone()], &[], false).unwrap();
        assert_eq!(parse_calendar(&text).unwrap()[0].title, "Reserva de recurso");
        // A finalidade, when there is one, still wins; private mode is unchanged.
        item.purpose = "Audiência".into();
        assert_eq!(parse_calendar(&export_calendar(&[], &[item.clone()], &[resource.clone()], false).unwrap()).unwrap()[0].title, "Audiência");
        let private = export_calendar(&[], &[item], &[resource], true).unwrap();
        assert_eq!(parse_calendar(&private).unwrap()[0].title, "Recurso reservado");
    }
    fn wrap(body: &str) -> String { format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n") }

    #[test]
    fn every_line_ends_with_crlf() {
        let mut t = task("u1"); t.due_date = "2026-11-20".into();
        let text = export_calendar(&[t], &[], &[], false).unwrap();
        assert!(text.ends_with("\r\n"));
        assert_eq!(text.matches('\n').count(), text.matches("\r\n").count());
        assert!(text.starts_with("BEGIN:VCALENDAR\r\n"));
    }
    #[test]
    fn escaping_round_trips() {
        let s = "a\\b;c,d\ne\r\nf\rg";
        assert_eq!(escape(s), "a\\\\b\\;c\\,d\\ne\\nf\\ng");
        assert_eq!(unescape(&escape(s)).unwrap(), "a\\b;c,d\ne\nf\ng");
        assert_eq!(unescape("x\\Ny").unwrap(), "x\ny");
        assert!(unescape("x\\q").is_err());
    }
    #[test]
    fn folding_never_splits_code_points_and_respects_75_octets() {
        let value = format!("SUMMARY:{} 😀 {}", "Atenção à ação previdenciária ".repeat(6), "ç😀é".repeat(20));
        let mut out = String::new(); line(&mut out, &value);
        assert!(out.contains("\r\n "));
        let physical: Vec<&str> = out.strip_suffix("\r\n").unwrap().split("\r\n").collect();
        assert!(physical.len() > 2);
        assert!(physical.iter().all(|l| l.len() <= 75), "{physical:?}");
        let rebuilt: String = physical.iter().enumerate().map(|(i, l)| if i == 0 { *l } else { &l[1..] }).collect();
        assert_eq!(rebuilt, value);
    }
    #[test]
    fn unfolding_accepts_space_and_tab() {
        let text = wrap("BEGIN:VTODO\r\nUID:abc\r\nSUMMARY:Ol\r\n á mu\r\n\tndo\r\nEND:VTODO\r\n");
        let e = parse_calendar(&text).unwrap();
        assert_eq!(e[0].title, "Olá mundo");
    }
    #[test]
    fn due_only_task_round_trips() {
        let mut t = task("only-due"); t.due_date = "2026-11-20".into();
        let text = export_calendar(&[t], &[], &[], false).unwrap();
        assert!(text.contains("DUE;VALUE=DATE:20261120\r\n"));
        let e = parse_calendar(&text).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].due.as_str(), e[0].start.as_str(), e[0].end.as_str()), ("2026-11-20", "", ""));
    }
    #[test]
    fn scheduled_task_round_trips_as_one_entry() {
        let mut t = task("sched"); t.scheduled_start = "2026-11-18T09:00".into(); t.scheduled_end = "2026-11-18T10:30".into();
        let e = parse_calendar(&export_calendar(&[t], &[], &[], false).unwrap()).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].start.as_str(), e[0].end.as_str(), e[0].due.as_str()), ("2026-11-18T09:00", "2026-11-18T10:30", ""));
    }
    #[test]
    fn google_and_outlook_partial_review() {
        let text = wrap("PRODID:example\r\nMETHOD:PUBLISH\r\nX-WR-CALNAME:Agenda\r\nBEGIN:VTIMEZONE\r\nTZID:Example\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Reunião\r\nDTSTART:20261118T090000Z\r\nDTEND:20261118T100000Z\r\nLOCATION:Sala\r\nATTENDEE;CN=Pessoa:mailto:a@example.com\r\nX-FOO:bar\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:2\r\nSUMMARY:Repetido\r\nDTSTART:20261118T090000\r\nDTEND:20261118T100000\r\nRRULE:FREQ=DAILY\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:3\r\nSUMMARY:Dia inteiro\r\nDTSTART;VALUE=DATE:20261120\r\nDTEND;VALUE=DATE:20261121\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:4\r\nSUMMARY:Fuso\r\nDTSTART;TZID=America/Cuiaba:20261118T090000\r\nDTEND;TZID=America/Cuiaba:20261118T100000\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nUID:5\r\nSUMMARY:Tarefa\r\nDUE;VALUE=DATE:20261122\r\nEND:VTODO\r\n");
        let review = parse_calendar(&text).unwrap();
        assert!(review.general_ignored);
        assert_eq!(review.entries.len(), 3); assert_eq!(review.skipped.len(), 2);
        assert!(review.skipped.iter().any(|e| e.reason.contains("repetição")));
        assert!(review.skipped.iter().any(|e| e.reason.contains("fuso horário não suportado")));
        let event = &review.entries[0];
        assert_eq!(event.start, chrono::DateTime::parse_from_rfc3339("2026-11-18T09:00:00Z").unwrap().with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M").to_string());
        assert_eq!(event.notes, "Local: Sala");
        for detail in ["convidados", "alarmes", "propriedade X-FOO"] { assert!(event.details.iter().any(|d| d == detail), "{detail}"); }
        assert_eq!(review.entries[1].due, "2026-11-20");
        assert!(review.entries[1].start.is_empty());
        assert!(review.entries[1].details.iter().any(|d| d.contains("dia inteiro")));
    }
    #[test]
    fn outlook_review_accounts_for_every_entry() {
        let text = wrap("METHOD:PUBLISH\r\nBEGIN:VEVENT\r\nUID:outlook-1\r\nSUMMARY:Consulta\r\nDTSTART:20261201T140000\r\nDTEND:20261201T150000\r\nORGANIZER;CN=Equipe:mailto:a@example.com\r\nCLASS:PUBLIC\r\nTRANSP:OPAQUE\r\nX-MICROSOFT-CDO-BUSYSTATUS:BUSY\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:outlook-2\r\nSUMMARY:Série\r\nDTSTART:20261201T140000\r\nDTEND:20261201T150000\r\nRDATE:20261202T140000\r\nEXDATE:20261203T140000\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nUID:outlook-3\r\nSUMMARY:Prazo\r\nDUE;VALUE=DATE:20261204\r\nEND:VTODO\r\n");
        let review = parse_calendar(&text).unwrap();
        assert_eq!(review.entries.len() + review.skipped.len(), 3);
        assert!(review.entries[0].details.iter().any(|d| d == "organizador"));
        assert!(review.entries[0].details.iter().any(|d| d == "propriedade X-MICROSOFT-CDO-BUSYSTATUS"));
        assert!(review.skipped[0].reason.contains("repetição"));
    }
    #[test]
    fn event_ending_at_midnight_is_an_open_end() {
        let text = wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Noite\r\nDTSTART:20261120T220000\r\nDTEND:20261121T000000\r\nEND:VEVENT\r\n");
        let e = parse_calendar(&text).unwrap();
        assert_eq!(e[0].end, "2026-11-21T00:00");
        assert!(e[0].end > e[0].start);
        let same = wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:N\r\nDTSTART:20261121T000000\r\nDTEND:20261121T000000\r\nEND:VEVENT\r\n");
        assert_eq!(parse_calendar(&same).unwrap().skipped.len(), 1);
    }
}
