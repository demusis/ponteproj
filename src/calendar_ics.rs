//! Original, deliberately bounded iCalendar interoperability. Floating local
//! times and DATE values only; unsupported semantics are refused before import.
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
    ensure!(value.len() == 15 && value.ends_with("00"), "Só horários locais com precisão de minuto são aceitos; UTC e fusos precisam ser convertidos antes da importação.");
    let parsed = chrono::NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")?;
    ensure!(parsed.format("%Y%m%dT%H%M%S").to_string() == value, "Horário ICS inválido.");
    Ok(parsed.format("%Y-%m-%dT%H:%M").to_string())
}

pub fn parse_calendar(text: &str) -> Result<Vec<ImportedEntry>> {
    ensure!(text.len() <= 4_000_000, "Calendário grande demais (máximo 4 MB).");
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        if raw.starts_with([' ', '\t']) {
            lines.last_mut().context("Continuação ICS sem linha anterior.")?.push_str(&raw[1..]);
        } else if !raw.is_empty() { lines.push(raw.to_owned()); }
    }
    ensure!(lines.first().map(String::as_str) == Some("BEGIN:VCALENDAR") && lines.last().map(String::as_str) == Some("END:VCALENDAR"), "Arquivo não é um VCALENDAR completo.");
    let mut entries = Vec::new();
    let mut current: Option<ImportedEntry> = None;
    let mut fields = std::collections::BTreeSet::new();
    // Everything the importer cannot represent is listed, never dropped.
    let mut unsupported: Vec<String> = Vec::new();
    let mut skip_depth = 0usize;
    for raw in &lines[1..lines.len()-1] {
        let (key, value) = raw.split_once(':').context("Linha ICS inválida.")?;
        if skip_depth > 0 {
            if key == "BEGIN" { skip_depth += 1; } else if key == "END" { skip_depth -= 1; }
            continue;
        }
        if key == "BEGIN" {
            if current.is_none() && matches!(value, "VEVENT" | "VTODO") {
                current = Some(ImportedEntry { event: value == "VEVENT", ..Default::default() });
                fields.clear();
            } else {
                unsupported.push(format!("componente {value}"));
                skip_depth = 1;
            }
            continue;
        }
        if key == "END" {
            let entry = current.take().context("Fim de entrada sem início.")?;
            ensure!(value == if entry.event { "VEVENT" } else { "VTODO" }, "Fim de entrada incompatível.");
            if !unsupported.is_empty() { continue; }
            ensure!(!entry.uid.is_empty() && !entry.title.trim().is_empty(), "Entrada sem UID ou título.");
            ensure!(entry.start.is_empty() == entry.end.is_empty(), "Entrada deve ter início e fim, ou nenhum dos dois.");
            ensure!(!entry.event || !entry.start.is_empty(), "Evento sem intervalo de horário.");
            ensure!(entry.end.is_empty() || entry.end > entry.start, "Fim deve ser posterior ao início.");
            entries.push(entry); ensure!(entries.len() <= 500, "Máximo de 500 entradas por importação."); continue;
        }
        if let Some(entry) = current.as_mut() {
            let known = matches!(key, "UID" | "SUMMARY" | "DESCRIPTION" | "RELATED-TO" | "DTSTART" | "DTEND" | "DUE;VALUE=DATE"
                | "STATUS" | "DTSTAMP" | "CREATED" | "LAST-MODIFIED" | "SEQUENCE");
            if !known { unsupported.push(format!("propriedade {key}")); continue; }
            ensure!(fields.insert(key.to_owned()), "Propriedade repetida: {key}.");
            match key {
                "UID" => entry.uid = unescape(value)?, "SUMMARY" => entry.title = unescape(value)?,
                "DESCRIPTION" => entry.notes = unescape(value)?, "RELATED-TO" => entry.related_to = unescape(value)?,
                "DTSTART" => entry.start = parse_time(value)?, "DTEND" => entry.end = parse_time(value)?,
                "DUE;VALUE=DATE" => entry.due = parse_date(value)?,
                "STATUS" => match value { "COMPLETED" => entry.completed = true, "NEEDS-ACTION" | "CONFIRMED" | "IN-PROCESS" => {}, _ => bail!("Situação não suportada: {value}.") },
                "DTSTAMP" | "CREATED" | "LAST-MODIFIED" | "SEQUENCE" => {},
                _ => unreachable!(),
            }
        } else if matches!(key, "VERSION" | "PRODID" | "CALSCALE") {
            if key == "VERSION" { ensure!(value == "2.0", "Versão ICS não suportada."); }
        } else {
            unsupported.push(format!("propriedade de calendário {key}"));
        }
    }
    ensure!(current.is_none() && skip_depth == 0, "Entrada incompleta.");
    if !unsupported.is_empty() {
        unsupported.dedup();
        bail!("Itens não suportados (nenhuma entrada foi importada): {}.", unsupported.join("; "));
    }
    let mut uids = std::collections::BTreeSet::new();
    ensure!(entries.iter().all(|entry| uids.insert(entry.uid.clone())), "UID repetido no arquivo.");
    // Rejoin Ponte work blocks to their task without duplicating the task.
    let blocks: Vec<_> = entries.iter().filter(|e| e.event && !e.related_to.is_empty()).cloned().collect();
    for block in blocks {
        let task = entries.iter_mut().find(|e| !e.event && e.uid == block.related_to).context("Bloco de trabalho sem tarefa relacionada no arquivo.")?;
        ensure!(task.start.is_empty(), "Mais de um bloco para a mesma tarefa ainda não é suportado.");
        task.start = block.start; task.end = block.end;
        entries.retain(|e| e.uid != block.uid);
    }
    Ok(entries)
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
    fn rrule_and_unknown_items_are_listed_not_dropped() {
        let text = wrap("BEGIN:VEVENT\r\nUID:e1\r\nSUMMARY:Rep\r\nDTSTART:20261118T090000\r\nDTEND:20261118T100000\r\nRRULE:FREQ=DAILY\r\nX-FOO:1\r\nEND:VEVENT\r\nBEGIN:VJOURNAL\r\nUID:j\r\nEND:VJOURNAL\r\nBEGIN:VTODO\r\nUID:t\r\nSUMMARY:T\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VTODO\r\n");
        let msg = parse_calendar(&text).unwrap_err().to_string();
        for needle in ["RRULE", "X-FOO", "VJOURNAL", "VALARM"] { assert!(msg.contains(needle), "{needle} em {msg}"); }
        let cal_prop = format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:x\r\nEND:VCALENDAR\r\n");
        assert!(parse_calendar(&cal_prop).unwrap_err().to_string().contains("X-WR-CALNAME"));
        let tz = wrap("BEGIN:VTIMEZONE\r\nTZID:X\r\nBEGIN:STANDARD\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n");
        assert!(parse_calendar(&tz).unwrap_err().to_string().contains("VTIMEZONE"));
    }
    #[test]
    fn all_day_vs_timed() {
        let due = wrap("BEGIN:VTODO\r\nUID:a\r\nSUMMARY:A\r\nDUE;VALUE=DATE:20261120\r\nEND:VTODO\r\n");
        assert_eq!(parse_calendar(&due).unwrap()[0].due, "2026-11-20");
        let timed_due = wrap("BEGIN:VTODO\r\nUID:a\r\nSUMMARY:A\r\nDUE:20261120T090000\r\nEND:VTODO\r\n");
        assert!(parse_calendar(&timed_due).is_err());
        let all_day_event = wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:A\r\nDTSTART;VALUE=DATE:20261120\r\nDTEND;VALUE=DATE:20261121\r\nEND:VEVENT\r\n");
        assert!(parse_calendar(&all_day_event).unwrap_err().to_string().contains("DTSTART;VALUE=DATE"));
        let timed = wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:A\r\nDTSTART:20261120T090000\r\nDTEND:20261120T100000\r\nEND:VEVENT\r\n");
        assert_eq!(parse_calendar(&timed).unwrap()[0].start, "2026-11-20T09:00");
    }
    #[test]
    fn event_ending_at_midnight_is_an_open_end() {
        let text = wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Noite\r\nDTSTART:20261120T220000\r\nDTEND:20261121T000000\r\nEND:VEVENT\r\n");
        let e = parse_calendar(&text).unwrap();
        assert_eq!(e[0].end, "2026-11-21T00:00");
        assert!(e[0].end > e[0].start);
        let same = wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:N\r\nDTSTART:20261121T000000\r\nDTEND:20261121T000000\r\nEND:VEVENT\r\n");
        assert!(parse_calendar(&same).is_err());
    }
    #[test]
    fn unsupported_semantics_are_never_silently_dropped() {
        for property in ["RRULE:FREQ=DAILY", "DTSTART;TZID=America/Cuiaba:20261118T090000", "ATTENDEE:mailto:a@example.com", "DTSTART:20261118T090000Z"] {
            let text = format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:test\r\nSUMMARY:Teste\r\n{property}\r\nEND:VTODO\r\nEND:VCALENDAR\r\n");
            assert!(parse_calendar(&text).is_err());
        }
    }
}
