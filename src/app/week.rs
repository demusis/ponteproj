//! A week of work: execution blocks and bookings, with deadlines kept separate.
use super::today::{day_time_label, working_day, Asked, TodayData};
use crate::resources::parse_moment;
use crate::theme::{self, S1, S2};
use crate::widgets::{self, Tone};
use crate::workload::{self, Load};
use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui;
use std::collections::{HashMap, HashSet};

use crate::tasks::Task;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Link {
    prerequisite: i64,
    dependent: i64,
    wrong_order: bool,
    concluded: bool,
}

// Only dependency IDs create links; parent_task_id describes a different relationship.
fn dependency_layout(
    tasks: &[Task],
    visible: &HashSet<i64>,
) -> (Vec<Link>, HashMap<i64, Vec<i64>>) {
    let by_id: HashMap<_, _> = tasks.iter().map(|task| (task.id, task)).collect();
    let mut links = Vec::new();
    let mut outside = HashMap::new();
    for task in tasks.iter().filter(|task| visible.contains(&task.id)) {
        for &id in &task.dependencies {
            if visible.contains(&id) {
                if let Some(previous) = by_id.get(&id) {
                    let wrong_order = !previous.is_done()
                        && matches!((parse_moment(&task.scheduled_start), parse_moment(&previous.scheduled_end)),
                            (Ok(start), Ok(end)) if start < end);
                    links.push(Link {
                        prerequisite: id,
                        dependent: task.id,
                        wrong_order,
                        concluded: previous.is_done(),
                    });
                }
            } else {
                outside.entry(task.id).or_insert_with(Vec::new).push(id);
            }
        }
    }
    (links, outside)
}

fn outside_dependency_label(task: &Task) -> String {
    let status = if task.is_done() {
        "concluída".to_owned()
    } else if let (Ok(start), Ok(end)) = (
        parse_moment(&task.scheduled_start),
        parse_moment(&task.scheduled_end),
    ) {
        if end > start {
            format!("planejada em {}", start.format("%d/%m"))
        } else {
            "sem horário".to_owned()
        }
    } else {
        "sem horário".to_owned()
    };
    format!("↳ depende de: {} ({status})", task.title)
}

fn arrow_geometry(from: egui::Rect, to: egui::Rect) -> [egui::Pos2; 4] {
    let a = from.center_bottom();
    let b = to.center_top();
    let bend = ((b.y - a.y).abs() * 0.5).max(24.0);
    [a, a + egui::vec2(0.0, bend), b - egui::vec2(0.0, bend), b]
}

fn monday(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

fn touches(start: &str, end: &str, day: NaiveDate) -> bool {
    if start.is_empty() || end.is_empty() {
        return false;
    }
    let (Ok(start), Ok(end)) = (parse_moment(start), parse_moment(end)) else {
        return false;
    };
    let from = day.and_hms_opt(0, 0, 0).unwrap();
    start < from + Duration::days(1) && end > from
}

pub(super) struct WeekCache {
    start: NaiveDate,
    links: Vec<Link>,
    outside: HashMap<i64, Vec<i64>>,
    warnings: HashMap<i64, Vec<i64>>,
    unscheduled: Vec<usize>,
    rows: Vec<Vec<(String, String, Asked)>>,
    loads: [Load; 7],
    total: Load,
}

fn build_week_cache(data: &TodayData, start: NaiveDate) -> WeekCache {
    let projections = &data.projections;
    let referenced: HashSet<i64> = data
        .planning_tasks
        .iter()
        .flat_map(|task| task.dependencies.iter().copied())
        .collect();
    let visible: HashSet<i64> = data
        .planning_tasks
        .iter()
        .filter(|task| {
            (!task.is_done() || referenced.contains(&task.id))
                && (0..7).any(|offset| {
                    let day = start + Duration::days(offset);
                    touches(&task.scheduled_start, &task.scheduled_end, day)
                        || projections.get(&task.id).is_some_and(|(a, b)| {
                            working_day(day)
                                && *a < (day + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap()
                                && *b > day.and_hms_opt(0, 0, 0).unwrap()
                        })
                })
        })
        .map(|task| task.id)
        .collect();
    let (links, outside) = dependency_layout(&data.planning_tasks, &visible);
    let mut warnings: HashMap<i64, Vec<i64>> = HashMap::new();
    for link in links.iter().filter(|link| link.wrong_order) {
        warnings
            .entry(link.dependent)
            .or_default()
            .push(link.prerequisite);
    }
    let mut unscheduled: Vec<_> = data
        .planning_tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_done() && t.scheduled_start.is_empty())
        .map(|(index, _)| index)
        .collect();
    unscheduled.sort_by(|&a, &b| {
        let (a, b) = (&data.planning_tasks[a], &data.planning_tasks[b]);
        (a.due_date.is_empty(), &a.due_date, &a.title).cmp(&(
            b.due_date.is_empty(),
            &b.due_date,
            &b.title,
        ))
    });
    let rows = (0..7)
        .map(|offset| {
            let day = start + Duration::days(offset);
            let mut rows: Vec<(String, String, Asked)> = Vec::new();
            for task in &data.planning_tasks {
                if visible.contains(&task.id)
                    && touches(&task.scheduled_start, &task.scheduled_end, day)
                {
                    rows.push((
                        task.scheduled_start.clone(),
                        format!(
                            "{}{} · {}",
                            if task.is_done() { "✓ " } else { "" },
                            day_time_label(
                                parse_moment(&task.scheduled_start).unwrap(),
                                parse_moment(&task.scheduled_end).unwrap(),
                                day
                            ),
                            task.title
                        ),
                        Asked::OpenTask(task.clone()),
                    ));
                }
                if let Some(&(a, b)) = projections.get(&task.id) {
                    let from = day.and_hms_opt(0, 0, 0).unwrap();
                    if working_day(day) && a < from + Duration::days(1) && b > from {
                        rows.push((
                            a.format("%Y-%m-%dT%H:%M").to_string(),
                            format!(
                                "{} · ⇢ {} (previsto)",
                                day_time_label(a, b, day),
                                task.title
                            ),
                            Asked::OpenTask(task.clone()),
                        ));
                    }
                }
            }
            for item in &data.planning_uses {
                if item.status.occupying() && touches(&item.start, &item.end, day) {
                    let resource = data
                        .planning_resources
                        .iter()
                        .find(|r| r.uid == item.resource_uid)
                        .map_or("Recurso", |r| r.name.as_str());
                    rows.push((
                        item.start.clone(),
                        format!(
                            "{} · {} · {}",
                            day_time_label(
                                parse_moment(&item.start).unwrap(),
                                parse_moment(&item.end).unwrap(),
                                day
                            ),
                            resource,
                            item.purpose
                        ),
                        Asked::OpenUse(item.uid.clone()),
                    ));
                }
            }
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            rows
        })
        .collect();
    let (loads, total) = workload::week_load(&data.planning_tasks, start, working_day);
    WeekCache {
        loads,
        total,
        rows,
        warnings,
        unscheduled,
        start,
        links,
        outside,
    }
}

pub(super) fn show(
    ui: &mut egui::Ui,
    data: &TodayData,
    today: NaiveDate,
    asked: &mut Option<Asked>,
) {
    let id = egui::Id::unique("planning-week-start");
    let mut start = ui
        .data(|d| d.get_temp::<NaiveDate>(id))
        .unwrap_or_else(|| monday(today));
    ui.horizontal(|ui| {
        if ui.button("◀ Semana anterior").clicked() {
            start -= Duration::days(7);
        }
        widgets::section(
            ui,
            format!(
                "{} a {}",
                start.format("%d/%m"),
                (start + Duration::days(6)).format("%d/%m/%Y")
            ),
        );
        if ui.button("Próxima semana ▶").clicked() {
            start += Duration::days(7);
        }
        if ui.button("Esta semana").clicked() {
            start = monday(today);
        }
    });
    ui.data_mut(|d| d.insert_temp(id, start));
    widgets::muted(ui, "Horários locais. Agendar o trabalho não altera o prazo de entrega. Clique em uma tarefa para planejar ou reagendar.");
    ui.add_space(S2);
    let mut cache = data.week_cache.borrow_mut();
    if cache.as_ref().is_none_or(|cache| cache.start != start) {
        *cache = Some(build_week_cache(data, start));
    }
    let cache = cache.as_ref().unwrap();
    week_load_line(ui, cache.total);
    ui.add_space(S2);
    let projections = &data.projections;
    let links = &cache.links;
    let outside = &cache.outside;
    let by_id: HashMap<_, _> = data
        .planning_tasks
        .iter()
        .map(|task| (task.id, task))
        .collect();
    let mut blocks: HashMap<i64, Vec<egui::Rect>> = HashMap::new();
    let warnings = &cache.warnings;
    let background_arrows = ui.painter().add(egui::Shape::Noop);
    let mut card_arrows = Vec::new();
    for offset in 0..7 {
        let day = start + Duration::days(offset);
        let iso = day.to_string();
        let card = widgets::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let arrow_slot = ui.painter().add(egui::Shape::Noop);
            ui.horizontal_wrapped(|ui| {
                widgets::section(ui, super::calendar::long_date(day));
                day_load_label(ui, cache.loads[offset as usize]);
                if let Some(name) = crate::holidays::holiday(day) {
                    ui.label(
                        egui::RichText::new(format!("· feriado: {name}")).color(theme::c().warn),
                    );
                }
            });
            if day == today { widgets::pill(ui, "Hoje", Tone::Info); }
            let rows = &cache.rows[offset as usize];
            if rows.is_empty() { widgets::muted(ui, "Sem trabalho agendado nem reservas."); }
            for (_, label, action) in rows {
                let task_id = if let Asked::OpenTask(task) = action { Some(task.id) } else { None };
                let done = matches!(action, Asked::OpenTask(task) if task.is_done());
                let response = if done { ui.add(egui::Button::new(egui::RichText::new(label).color(theme::c().muted)).frame(false)) } else { ui.add(widgets::quiet(label)) };
                if let Some(id) = task_id {
                    blocks.entry(id).or_default().push(response.rect);
                }
                if response.clicked() { *asked = Some(action.clone()); }
                if let Some(id) = task_id {
                    if let Some(ids) = outside.get(&id) {
                        for prerequisite in ids.iter().filter_map(|id| by_id.get(id)) {
                            let marker = ui.add(widgets::quiet(outside_dependency_label(prerequisite)))
                                .on_hover_text("Abrir a tarefa de que esta depende");
                            if marker.clicked() { *asked = Some(Asked::OpenTask((*prerequisite).clone())); }
                        }
                    }
                    for prerequisite in warnings.get(&id).into_iter().flatten() {
                        if let Some(previous) = by_id.get(prerequisite) {
                            let marker = ui.add(widgets::quiet(format!("⚠ começa antes de terminar {}", previous.title)))
                                .on_hover_text("Esta tarefa começa antes do término agendado do pré-requisito ainda aberto.");
                            if marker.clicked() { *asked = by_id.get(&id).map(|task| Asked::OpenTask((*task).clone())); }
                        }
                    }
                }
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
            arrow_slot
        });
        card_arrows.push((card.inner, card.response.rect));
        ui.add_space(S2);
    }
    // Paint each curve below card contents, and also in the gaps between cards.
    let mut arrow_shapes = Vec::new();
    for link in links {
        if let (Some(from), Some(to)) =
            (blocks.get(&link.prerequisite), blocks.get(&link.dependent))
        {
            let points = arrow_geometry(*from.last().unwrap(), to[0]);
            let color = if link.wrong_order {
                theme::c().warn
            } else {
                theme::c().muted
            };
            let color = color.gamma_multiply(if link.concluded { 0.35 } else { 0.65 });
            let stroke = egui::Stroke::new(if link.concluded { 0.8 } else { 1.3 }, color);
            arrow_shapes.push(
                egui::epaint::CubicBezierShape::from_points_stroke(
                    points,
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                )
                .into(),
            );
            let tip = points[3];
            arrow_shapes.push(egui::Shape::line_segment(
                [tip + egui::vec2(-4.0, -7.0), tip],
                stroke,
            ));
            arrow_shapes.push(egui::Shape::line_segment(
                [tip + egui::vec2(4.0, -7.0), tip],
                stroke,
            ));
        }
    }
    ui.painter()
        .set(background_arrows, egui::Shape::Vec(arrow_shapes.clone()));
    for (slot, rect) in card_arrows {
        ui.painter()
            .with_clip_rect(rect)
            .set(slot, egui::Shape::Vec(arrow_shapes.clone()));
    }
    widgets::section(ui, "Ainda não planejadas");
    let unscheduled = &cache.unscheduled;
    if unscheduled.is_empty() {
        widgets::muted(ui, "Todas as tarefas abertas têm um horário reservado.");
    }
    let toggle_id = egui::Id::unique("week-show-all-unscheduled");
    let mut show_all = ui.data(|d| d.get_temp::<bool>(toggle_id)).unwrap_or(false);
    if unscheduled.len() > 30
        && ui
            .button(if show_all {
                "Mostrar menos".to_owned()
            } else {
                format!("Mostrar todas ({})", unscheduled.len())
            })
            .clicked()
    {
        show_all = !show_all;
        ui.data_mut(|d| d.insert_temp(toggle_id, show_all));
    }
    for &index in unscheduled
        .iter()
        .take(if show_all { usize::MAX } else { 30 })
    {
        let task = &data.planning_tasks[index];
        let due = if task.due_date.is_empty() {
            "sem prazo".to_owned()
        } else {
            format!("prazo {}", crate::labels::date(&task.due_date))
        };
        let estimate = estimate_label(task.estimate_minutes);
        let suffix = if estimate.is_empty() {
            String::new()
        } else {
            format!(" · {estimate}")
        };
        let projected = projections
            .get(&task.id)
            .map(|(a, b)| format!(" · previsto {}–{}", a.format("%d/%m"), b.format("%d/%m")))
            .unwrap_or_default();
        if ui
            .add(widgets::quiet(format!(
                "{} · {}{}{}",
                task.title, due, suffix, projected
            )))
            .clicked()
        {
            *asked = Some(Asked::OpenTask(task.clone()));
        }
    }
}

/// "Carga da semana: 26 h de 45 h planejadas", a thin bar and the help.
fn week_load_line(ui: &mut egui::Ui, total: Load) {
    let text = if total.capacity_minutes == 0 {
        if total.planned_minutes == 0 {
            "Carga da semana: sem dias úteis".to_owned()
        } else {
            format!(
                "Carga da semana: {} planejadas, sem dias úteis",
                workload::hours_label(total.planned_minutes)
            )
        }
    } else {
        format!(
            "Carga da semana: {} de {} planejadas",
            workload::hours_label(total.planned_minutes),
            workload::hours_label(total.capacity_minutes)
        )
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(text).color(if total.is_over() {
            theme::c().warn
        } else {
            theme::c().text
        }));
        crate::help_ui::help(
            ui,
            "Soma os períodos planejados das tarefas abertas, dentro e fora do expediente de 9h às 18h, contra os dias úteis da semana (feriados e folgas conforme Ajustes). Tarefas sem período planejado não entram.",
        );
        let width = ui.available_width().clamp(60.0, 160.0);
        load_bar(ui, total, width);
    });
}

/// The thin bar: normal up to 100 %, the warning tone above.
fn load_bar(ui: &mut egui::Ui, load: Load, width: f32) {
    let p = theme::c();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = egui::CornerRadius::same(3);
    ui.painter().rect_filled(rect, radius, p.outline);
    if load.planned_minutes > 0 {
        let fraction = if load.capacity_minutes == 0 {
            1.0
        } else {
            (load.planned_minutes as f32 / load.capacity_minutes as f32).clamp(0.0, 1.0)
        };
        let filled = egui::Rect::from_min_size(rect.min, egui::vec2(width * fraction, 6.0));
        let color = if load.is_over() { p.warn } else { p.primary };
        ui.painter().rect_filled(filled, radius, color);
    }
}

/// The planned hours of a day beside its date; the warning tone when over.
fn day_load_label(ui: &mut egui::Ui, load: Load) {
    if load.planned_minutes == 0 {
        return;
    }
    let hours = workload::hours_label(load.planned_minutes);
    if load.is_over() {
        ui.label(egui::RichText::new(format!("⚠ {hours}")).color(theme::c().warn))
            .on_hover_text(format!(
                "Dia com mais horas planejadas ({hours}) que o expediente ({}).",
                workload::hours_label(load.capacity_minutes)
            ));
    } else {
        let note = if load.capacity_minutes == 0 {
            "Horas planejadas neste dia, que não tem expediente."
        } else {
            "Horas planejadas neste dia."
        };
        ui.label(egui::RichText::new(hours).color(theme::c().muted))
            .on_hover_text(note);
    }
}

fn estimate_label(minutes: u32) -> String {
    if minutes == 0 {
        String::new()
    } else if minutes.is_multiple_of(1440) {
        let days = minutes / 1440;
        format!("{} {}", days, if days == 1 { "dia" } else { "dias" })
    } else {
        format!("{minutes} min")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn estimate_display() {
        assert_eq!(super::estimate_label(0), "");
        assert_eq!(super::estimate_label(1440), "1 dia");
        assert_eq!(super::estimate_label(2880), "2 dias");
        assert_eq!(super::estimate_label(15), "15 min");
    }

    use super::*;
    #[test]
    fn week_crosses_year_and_sunday_belongs_to_previous_monday() {
        assert_eq!(
            monday(NaiveDate::from_ymd_opt(2027, 1, 3).unwrap()).to_string(),
            "2026-12-28"
        );
    }
    #[test]
    fn midnight_end_does_not_occupy_following_day() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        assert!(!touches("2026-11-17T23:00", "2026-11-18T00:00", day));
        assert!(touches("2026-11-17T23:00", "2026-11-18T01:00", day));
        assert!(!touches("invalid", "invalid", day));
    }
    #[test]
    fn week_dependency_links_and_outside_markers() {
        let mut a = Task::standalone();
        a.id = 1;
        a.scheduled_end = "2026-11-17T11:00".into();
        let mut b = Task::standalone();
        b.id = 2;
        b.scheduled_start = "2026-11-17T10:00".into();
        b.dependencies = vec![1, 3];
        b.parent_task_id = Some(4);
        let mut parent = Task::standalone();
        parent.id = 4;
        let visible = [1, 2, 4].into_iter().collect();
        let (links, outside) = dependency_layout(&[a.clone(), b.clone(), parent.clone()], &visible);
        assert_eq!(
            links,
            vec![Link {
                prerequisite: 1,
                dependent: 2,
                wrong_order: true,
                concluded: false
            }]
        );
        assert_eq!(outside.get(&2), Some(&vec![3]));
        a.status = "concluida".into();
        let (links, _) = dependency_layout(&[a, b, parent], &visible);
        assert!(!links[0].wrong_order);
        assert!(links[0].concluded);
    }

    #[test]
    fn week_outside_dependency_labels() {
        let mut task = Task::standalone();
        task.title = "Preparar".into();
        assert_eq!(
            outside_dependency_label(&task),
            "↳ depende de: Preparar (sem horário)"
        );
        task.scheduled_start = "2026-11-17T10:00".into();
        task.scheduled_end = "2026-11-17T11:00".into();
        assert_eq!(
            outside_dependency_label(&task),
            "↳ depende de: Preparar (planejada em 17/11)"
        );
        task.status = "concluida".into();
        assert_eq!(
            outside_dependency_label(&task),
            "↳ depende de: Preparar (concluída)"
        );
    }

    #[test]
    fn week_arrow_geometry_uses_block_end_and_start() {
        let from = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(20.0, 10.0));
        let to = egui::Rect::from_min_max(egui::pos2(40.0, 80.0), egui::pos2(60.0, 90.0));
        let points = arrow_geometry(from, to);
        assert_eq!(points[0], from.center_bottom());
        assert_eq!(points[3], to.center_top());
    }
}
