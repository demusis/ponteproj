//! A week of work: execution blocks and bookings, with deadlines kept separate.
use super::today::{Asked, TodayData};
use crate::resources::parse_moment;
use crate::theme::{self, S1, S2};
use crate::widgets::{self, Tone};
use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui;

fn monday(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

fn touches(start: &str, end: &str, day: NaiveDate) -> bool {
    let (Ok(start), Ok(end)) = (parse_moment(start), parse_moment(end)) else {
        return false;
    };
    let from = day.and_hms_opt(0, 0, 0).unwrap();
    start < from + Duration::days(1) && end > from
}

pub(super) fn show(ui: &mut egui::Ui, data: &TodayData, today: NaiveDate, asked: &mut Option<Asked>) {
    let id = egui::Id::new("planning-week-start");
    let mut start = ui.data(|d| d.get_temp::<NaiveDate>(id)).unwrap_or_else(|| monday(today));
    ui.horizontal(|ui| {
        if ui.button("◀ Semana anterior").clicked() { start -= Duration::days(7); }
        widgets::section(ui, format!("{} a {}", start.format("%d/%m"), (start + Duration::days(6)).format("%d/%m/%Y")));
        if ui.button("Próxima semana ▶").clicked() { start += Duration::days(7); }
        if ui.button("Esta semana").clicked() { start = monday(today); }
    });
    ui.data_mut(|d| d.insert_temp(id, start));
    widgets::muted(ui, "Horários locais. Agendar o trabalho não altera o prazo de entrega. Clique em uma tarefa para planejar ou reagendar.");
    ui.add_space(S2);
    for offset in 0..7 {
        let day = start + Duration::days(offset);
        let iso = day.to_string();
        widgets::card(ui, |ui| {
            widgets::section(ui, super::calendar::long_date(day));
            if day == today { widgets::pill(ui, "Hoje", Tone::Info); }
            let mut rows: Vec<(String, String, Asked)> = Vec::new();
            for task in &data.planning_tasks {
                if !task.is_done() && touches(&task.scheduled_start, &task.scheduled_end, day) {
                    rows.push((task.scheduled_start.clone(), format!("{}–{} · {}", task.scheduled_start.get(11..).unwrap_or(""), task.scheduled_end.get(11..).unwrap_or(""), task.title), Asked::OpenTask(task.clone())));
                }
            }
            for item in &data.planning_uses {
                if item.status.occupying() && touches(&item.start, &item.end, day) {
                    let resource = data.planning_resources.iter().find(|r| r.uid == item.resource_uid).map_or("Recurso", |r| r.name.as_str());
                    rows.push((item.start.clone(), format!("{}–{} · {} · {}", item.start.get(11..).unwrap_or(""), item.end.get(11..).unwrap_or(""), resource, item.purpose), Asked::OpenUse(item.uid.clone())));
                }
            }
            rows.sort_by(|a,b| a.0.cmp(&b.0));
            if rows.is_empty() { widgets::muted(ui, "Sem trabalho agendado nem reservas."); }
            for (_, label, action) in rows {
                if ui.add(widgets::quiet(label)).clicked() { *asked = Some(action); }
            }
            let deadlines: Vec<_> = data.dated_tasks.iter().filter(|t| t.due_date == iso).collect();
            let attendances: Vec<_> = data.dated_attendances.iter().filter(|a| a.due_date == iso).collect();
            if !deadlines.is_empty() || !attendances.is_empty() {
                ui.add_space(S1);
                widgets::overline(ui, "Prazos de entrega", Some(theme::c().warn));
                for task in deadlines {
                    if ui.add(widgets::quiet(&task.title)).clicked() { *asked = Some(Asked::OpenTask(task.clone())); }
                }
                for attendance in attendances {
                    if ui.add(widgets::quiet(format!("Atendimento · {}", attendance.subject))).clicked() { *asked = Some(Asked::OpenAttendance(attendance.id)); }
                }
            }
        });
        ui.add_space(S2);
    }
    widgets::section(ui, "Ainda não planejadas");
    let mut unscheduled: Vec<_> = data.planning_tasks.iter().filter(|t| !t.is_done() && t.scheduled_start.is_empty()).collect();
    unscheduled.sort_by(|a,b| (a.due_date.is_empty(), &a.due_date, &a.title).cmp(&(b.due_date.is_empty(), &b.due_date, &b.title)));
    if unscheduled.is_empty() { widgets::muted(ui, "Todas as tarefas abertas têm um horário reservado."); }
    for task in unscheduled {
        let due = if task.due_date.is_empty() { "sem prazo".to_owned() } else { format!("prazo {}", crate::labels::date(&task.due_date)) };
        if ui.add(widgets::quiet(format!("{} · {} · {} min", task.title, due, task.estimate_minutes))).clicked() {
            *asked = Some(Asked::OpenTask(task.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn week_crosses_year_and_sunday_belongs_to_previous_monday() {
        assert_eq!(monday(NaiveDate::from_ymd_opt(2027,1,3).unwrap()).to_string(), "2026-12-28");
    }
    #[test]
    fn midnight_end_does_not_occupy_following_day() {
        let day = NaiveDate::from_ymd_opt(2026,11,18).unwrap();
        assert!(!touches("2026-11-17T23:00", "2026-11-18T00:00", day));
        assert!(touches("2026-11-17T23:00", "2026-11-18T01:00", day));
        assert!(!touches("invalid", "invalid", day));
    }
}
