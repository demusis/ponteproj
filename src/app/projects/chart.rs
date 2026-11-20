//! The timeline of a project ("Linha do tempo"): a Gantt-like chart drawn with
//! the painter. The geometry (dates to x, bars, header ticks, shaded days) is
//! pure and tested; `show` only paints what it gives.
use super::super::today::{day_month, list_card, row};
use crate::projects::{self, TimelineRow};
use crate::theme::{self, c, S1, S2, S3};
use crate::widgets::{self, Tone};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use eframe::egui::{self, RichText};

/// Width of the column with the titles.
pub(super) const LEFT_W: f32 = 240.0;
/// Height of one task row and of the header of the time area.
pub(super) const ROW_H: f32 = 30.0;
pub(super) const HEAD_H: f32 = 40.0;
/// A day is never narrower than this, so bars stay visible; a long project
/// scrolls sideways instead.
pub(super) const MIN_DAY_W: f32 = 14.0;
/// A bar is never narrower than this (a task of a few hours still shows).
pub(super) const MIN_BAR_W: f32 = 6.0;
/// From this day width on, each day has its own number in the header.
const DAY_LABELS_FROM: f32 = 22.0;
/// A "today" further than this many days from the tasks does not stretch the
/// chart.
const TODAY_REACH_DAYS: i64 = 14;

/// Days on the horizontal axis: `days` columns of `day_w` points from `first`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Scale {
    pub first: NaiveDate,
    pub days: i64,
    pub day_w: f32,
}

impl Scale {
    /// The columns from `first` to `last` (both included), as wide as needed
    /// to fill `view_w`, but never narrower than [`MIN_DAY_W`].
    pub(super) fn new(first: NaiveDate, last: NaiveDate, view_w: f32) -> Self {
        let days = (last - first).num_days().max(0) + 1;
        Self {
            first,
            days,
            day_w: (view_w / days as f32).max(MIN_DAY_W),
        }
    }

    pub(super) fn width(&self) -> f32 {
        self.days as f32 * self.day_w
    }

    /// Left edge of a day.
    pub(super) fn x_of_date(&self, day: NaiveDate) -> f32 {
        (day - self.first).num_days() as f32 * self.day_w
    }

    /// Position of a moment (the day is a column; the hour moves inside it).
    pub(super) fn x_of(&self, at: NaiveDateTime) -> f32 {
        let origin = self.first.and_hms_opt(0, 0, 0).expect("midnight exists");
        (at - origin).num_seconds() as f32 / 86_400.0 * self.day_w
    }

    /// Left and right edge of the bar of a period, inside the chart and at
    /// least [`MIN_BAR_W`] wide.
    pub(super) fn bar(&self, period: (NaiveDateTime, NaiveDateTime)) -> (f32, f32) {
        let width = self.width();
        let mut x0 = self.x_of(period.0).clamp(0.0, width);
        let mut x1 = self.x_of(period.1).clamp(0.0, width);
        if x1 - x0 < MIN_BAR_W {
            if x0 + MIN_BAR_W <= width {
                x1 = x0 + MIN_BAR_W;
            } else {
                x1 = width;
                x0 = (width - MIN_BAR_W).max(0.0);
            }
        }
        (x0, x1)
    }

    /// Where a deadline is marked: the end of its day, which is where a bar
    /// that ends on time stops.
    pub(super) fn deadline_x(&self, day: NaiveDate) -> f32 {
        self.x_of_date(day + Duration::days(1))
    }

    /// Middle of a day.
    pub(super) fn day_center(&self, day: NaiveDate) -> f32 {
        self.x_of_date(day) + self.day_w / 2.0
    }

    /// Whether the day is one of the columns.
    pub(super) fn contains(&self, day: NaiveDate) -> bool {
        let n = (day - self.first).num_days();
        (0..self.days).contains(&n)
    }
}

/// The first and last day of the chart: the span of the timeline with a day
/// of room on each side, widened to hold the target date and (when it is near
/// the tasks) today. `None` when the project has no dates at all.
pub(super) fn chart_range(
    span: Option<(NaiveDateTime, NaiveDateTime)>,
    today: NaiveDate,
    target: Option<NaiveDate>,
) -> Option<(NaiveDate, NaiveDate)> {
    let (start, end) = span?;
    // A span that ends at midnight sharp does not touch the day that begins there.
    // Stored dates are not trusted: near chrono's limits the plain `+`/`-` panic.
    let day = Duration::days(1);
    let last_used = end
        .checked_sub_signed(Duration::seconds(1))
        .unwrap_or(end)
        .date()
        .max(start.date());
    let mut first = start.date().checked_sub_signed(day).unwrap_or(start.date());
    let mut last = last_used.checked_add_signed(day).unwrap_or(last_used);
    if let Some(target) = target {
        first = first.min(target);
        last = last.max(target.checked_add_signed(day).unwrap_or(target));
    }
    let reach = Duration::days(TODAY_REACH_DAYS);
    let near_first = first.checked_sub_signed(reach).unwrap_or(NaiveDate::MIN);
    let near_last = last.checked_add_signed(reach).unwrap_or(NaiveDate::MAX);
    if today >= near_first && today <= near_last {
        first = first.min(today);
        last = last.max(today);
    }
    Some((first, last))
}

/// A label of the header of the time area.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Tick {
    /// Left edge of the day it belongs to.
    pub x: f32,
    pub label: String,
    /// A week mark ("dd/mm" on the Monday, or on the first day); otherwise a
    /// day number.
    pub week: bool,
}

/// The labels of the header: "dd/mm" on every Monday (and on the first day),
/// plus the day numbers when the days are wide enough to carry them.
pub(super) fn header_ticks(scale: &Scale) -> Vec<Tick> {
    let mut ticks = Vec::new();
    for n in 0..scale.days {
        let day = scale.first + Duration::days(n);
        let x = n as f32 * scale.day_w;
        if n == 0 || day.weekday() == Weekday::Mon {
            ticks.push(Tick {
                x,
                label: day.format("%d/%m").to_string(),
                week: true,
            });
        }
        if scale.day_w >= DAY_LABELS_FROM {
            ticks.push(Tick {
                x,
                label: day.format("%d").to_string(),
                week: false,
            });
        }
    }
    ticks
}

/// The stretches (left edge, width) of days that are not worked, with
/// consecutive days merged. `working` says whether a day is worked.
pub(super) fn shaded_days(scale: &Scale, working: impl Fn(NaiveDate) -> bool) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::new();
    for n in 0..scale.days {
        let day = scale.first + Duration::days(n);
        if working(day) {
            continue;
        }
        let x = n as f32 * scale.day_w;
        match out.last_mut() {
            Some((from, width)) if (*from + *width - x).abs() < 0.01 => *width += scale.day_w,
            _ => out.push((x, scale.day_w)),
        }
    }
    out
}

/// How the bar of a row is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BarKind {
    Normal,
    Done,
    Late,
}

/// The colour kind of a row and whether its bar is hatched (a projection,
/// not a fixed plan). A concluded task is muted whatever else it is.
pub(super) fn bar_style(row: &TimelineRow) -> (BarKind, bool) {
    let kind = if row.done {
        BarKind::Done
    } else if row.late {
        BarKind::Late
    } else {
        BarKind::Normal
    };
    (kind, row.projected)
}

/// What the hover of a row says.
pub(super) fn row_tooltip(row: &TimelineRow) -> String {
    let mut lines = vec![row.title.clone()];
    lines.push(match row.period {
        Some((start, end)) => format!(
            "Período{}: {} – {}",
            if row.projected { " (previsto)" } else { "" },
            start.format("%d/%m %H:%M"),
            end.format("%d/%m %H:%M")
        ),
        None => "Sem período planejado".to_owned(),
    });
    lines.push(match row.due {
        Some(due) => format!("Prazo: {}", due.format("%d/%m/%Y")),
        None => "Sem prazo".to_owned(),
    });
    if row.done {
        lines.push("Concluída".to_owned());
    } else if row.late {
        lines.push("O período termina depois do prazo.".to_owned());
    }
    lines.join("\n")
}

/// What the chart needs from the screen.
pub(super) struct Input<'a> {
    pub uid: &'a str,
    pub rows: &'a [TimelineRow],
    pub target: Option<NaiveDate>,
    pub today: NaiveDate,
}

/// Draws the chart, the list of tasks without a period and the legend.
/// Returns the id of the task the person clicked.
pub(super) fn show(ui: &mut egui::Ui, input: &Input) -> Option<i64> {
    let gridded: Vec<&TimelineRow> = input.rows.iter().filter(|r| r.period.is_some()).collect();
    let unplanned: Vec<&TimelineRow> = input.rows.iter().filter(|r| r.period.is_none()).collect();
    let mut clicked = None;
    let range = chart_range(
        projects::timeline_span(input.rows),
        input.today,
        input.target,
    );
    match range {
        Some((first, last)) if !gridded.is_empty() => {
            clicked = draw_chart(ui, input, &gridded, first, last);
            ui.add_space(S1);
            legend(ui);
        }
        _ => {
            widgets::card(ui, |ui| {
                widgets::muted(
                    ui,
                    if input.rows.is_empty() {
                        "O projeto ainda não tem tarefas para mostrar na linha do tempo."
                    } else {
                        "Nenhuma tarefa deste projeto tem período planejado ou previsto ainda. Planeje o período na tarefa, ou use dependências e estimativa para o Ponte prever."
                    },
                );
            });
        }
    }
    if !unplanned.is_empty() {
        ui.add_space(S3);
        widgets::overline(ui, "Sem período planejado", None);
        ui.add_space(S1);
        list_card(ui, |ui| {
            for (index, task) in unplanned.iter().enumerate() {
                let was = row(
                    ui,
                    ("timeline-unplanned", task.id),
                    index + 1 == unplanned.len(),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = S3;
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    match (task.done, task.due) {
                                        (true, _) => widgets::pill(ui, "Concluída", Tone::Ok),
                                        (false, Some(due)) => widgets::pill(
                                            ui,
                                            format!("Prazo {}", day_month(&due.to_string())),
                                            Tone::Info,
                                        ),
                                        (false, None) => {
                                            widgets::pill(ui, "Sem prazo", Tone::Neutral)
                                        }
                                    };
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&task.title)
                                                        .font(theme::font_strong(theme::SIZE_BODY)),
                                                )
                                                .selectable(false)
                                                .truncate(),
                                            );
                                        },
                                    );
                                },
                            );
                        });
                    },
                );
                if was {
                    clicked = Some(task.id);
                }
            }
        });
        ui.add_space(S1);
        widgets::small_muted(
            ui,
            "Para aparecer no gráfico, a tarefa precisa de início e fim planejados, ou de dependências e estimativa para o Ponte prever.",
        );
    }
    clicked
}

fn legend(ui: &mut egui::Ui) {
    widgets::small_muted(
        ui,
        "Barra cheia: período planejado. Hachurada: previsto pelas dependências. Losango: prazo. Linha cheia: hoje. Tracejada: data-alvo do projeto. Faixas cinza: dias sem expediente.",
    );
}

fn draw_chart(
    ui: &mut egui::Ui,
    input: &Input,
    rows: &[&TimelineRow],
    first: NaiveDate,
    last: NaiveDate,
) -> Option<i64> {
    let p = c();
    let view_w = (ui.available_width() - LEFT_W - 2.0).max(240.0);
    let scale = Scale::new(first, last, view_w);
    let height = HEAD_H + ROW_H * rows.len() as f32;
    let mut clicked = None;
    widgets::card_frame()
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // The titles: a fixed column that does not scroll sideways.
                let (left, _) =
                    ui.allocate_exact_size(egui::vec2(LEFT_W, height), egui::Sense::hover());
                ui.painter().text(
                    egui::pos2(left.left() + S3, left.top() + HEAD_H - 8.0),
                    egui::Align2::LEFT_BOTTOM,
                    "TAREFA",
                    theme::font_strong(11.0),
                    p.muted,
                );
                for (index, row) in rows.iter().enumerate() {
                    let top = left.top() + HEAD_H + ROW_H * index as f32;
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(left.left() + S3, top),
                        egui::vec2(LEFT_W - S3 - S2, ROW_H),
                    );
                    let color = if row.done { p.muted } else { p.text };
                    let response = ui
                        .scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(cell)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&row.title)
                                            .font(theme::font(theme::SIZE_BODY))
                                            .color(color),
                                    )
                                    .selectable(false)
                                    .truncate()
                                    .sense(egui::Sense::click()),
                                )
                            },
                        )
                        .inner
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(row_tooltip(row));
                    if response.clicked() {
                        clicked = Some(row.id);
                    }
                    if index > 0 {
                        ui.painter().hline(
                            left.x_range(),
                            top,
                            egui::Stroke::new(1.0, p.outline.gamma_multiply(0.5)),
                        );
                    }
                }
                ui.painter().vline(
                    left.right(),
                    left.y_range(),
                    egui::Stroke::new(1.0, p.outline),
                );
                // The time area: scrolls sideways; today is brought into view
                // the first time a project is shown.
                let init_id = ui.make_persistent_id(("timeline-scrolled", input.uid));
                let first_time = ui.data(|d| d.get_temp::<bool>(init_id)).is_none();
                let mut area = egui::ScrollArea::horizontal()
                    .id_salt(("project-timeline", input.uid))
                    .auto_shrink([false, true]);
                if first_time {
                    let anchor = if scale.contains(input.today) {
                        scale.x_of_date(input.today)
                    } else {
                        0.0
                    };
                    area = area.horizontal_scroll_offset((anchor - view_w * 0.3).max(0.0));
                    ui.data_mut(|d| d.insert_temp(init_id, true));
                }
                area.show(ui, |ui| {
                    if let Some(id) = paint_grid(ui, input, rows, &scale, height) {
                        clicked = Some(id);
                    }
                });
            });
        });
    clicked
}

/// The time area: shading, header, lines, bars and deadlines.
fn paint_grid(
    ui: &mut egui::Ui,
    input: &Input,
    rows: &[&TimelineRow],
    scale: &Scale,
    height: f32,
) -> Option<i64> {
    let p = c();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(scale.width(), height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let origin = rect.min;
    let grid_top = origin.y + HEAD_H;
    let faint = p.outline.gamma_multiply(0.5);
    // Days off.
    for (x, width) in shaded_days(scale, crate::holidays::is_working_day) {
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(origin.x + x, origin.y),
                egui::vec2(width, height),
            ),
            0.0,
            p.sunken,
        );
    }
    // Header: week marks on top, day numbers under them.
    for tick in header_ticks(scale) {
        let x = origin.x + tick.x;
        if tick.week {
            painter.vline(
                x,
                egui::Rangef::new(origin.y, origin.y + height),
                egui::Stroke::new(1.0, faint),
            );
            painter.text(
                egui::pos2(x + 4.0, origin.y + 5.0),
                egui::Align2::LEFT_TOP,
                tick.label,
                theme::font_strong(11.0),
                p.muted,
            );
        } else {
            painter.text(
                egui::pos2(x + scale.day_w / 2.0, origin.y + 23.0),
                egui::Align2::CENTER_TOP,
                tick.label,
                theme::font(10.5),
                p.muted,
            );
        }
    }
    painter.hline(rect.x_range(), grid_top, egui::Stroke::new(1.0, p.outline));
    for index in 1..rows.len() {
        painter.hline(
            rect.x_range(),
            grid_top + ROW_H * index as f32,
            egui::Stroke::new(1.0, faint),
        );
    }
    // Today (solid) and the target date of the project (dashed).
    if scale.contains(input.today) {
        let x = origin.x + scale.day_center(input.today);
        painter.vline(
            x,
            egui::Rangef::new(origin.y + 2.0, origin.y + height),
            egui::Stroke::new(1.5, p.info),
        );
    }
    if let Some(target) = input.target.filter(|t| scale.contains(*t)) {
        let x = origin.x + scale.deadline_x(target);
        painter.extend(egui::Shape::dashed_line(
            &[
                egui::pos2(x, origin.y + 2.0),
                egui::pos2(x, origin.y + height),
            ],
            egui::Stroke::new(1.5, p.warn),
            5.0,
            4.0,
        ));
    }
    // The tasks.
    let mut clicked = None;
    for (index, row) in rows.iter().enumerate() {
        let Some(period) = row.period else {
            continue;
        };
        let top = grid_top + ROW_H * index as f32;
        let row_rect =
            egui::Rect::from_min_size(egui::pos2(origin.x, top), egui::vec2(scale.width(), ROW_H));
        if !row_rect.intersects(ui.clip_rect()) {
            continue;
        }
        let (x0, x1) = scale.bar(period);
        let bar = egui::Rect::from_min_max(
            egui::pos2(origin.x + x0, top + 7.0),
            egui::pos2(origin.x + x1, top + ROW_H - 7.0),
        );
        let due_x = row.due.map(|d| origin.x + scale.deadline_x(d));
        let mut hit = bar.expand2(egui::vec2(3.0, 3.0));
        if let Some(x) = due_x {
            hit = hit.union(egui::Rect::from_center_size(
                egui::pos2(x, top + ROW_H / 2.0),
                egui::vec2(14.0, 14.0),
            ));
        }
        let response = ui
            .interact(
                hit,
                ui.make_persistent_id(("timeline-row", row.id)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(row_tooltip(row));
        if response.hovered() {
            painter.rect_filled(row_rect, 0.0, p.primary_soft.gamma_multiply(0.6));
        }
        paint_bar(&painter, bar, row);
        if let Some(x) = due_x {
            // The text colour whatever the bar is, so it shows over a red one too.
            let color = if row.done { p.muted } else { p.text };
            diamond(&painter, egui::pos2(x, top + ROW_H / 2.0), 5.5, color);
        }
        if response.clicked() {
            clicked = Some(row.id);
        }
    }
    clicked
}

fn paint_bar(painter: &egui::Painter, rect: egui::Rect, row: &TimelineRow) {
    let p = c();
    let (kind, hatched) = bar_style(row);
    let color = match kind {
        BarKind::Normal => p.primary,
        BarKind::Done => p.muted.gamma_multiply(0.55),
        BarKind::Late => p.danger,
    };
    let radius = egui::CornerRadius::same(4);
    if hatched {
        painter.rect_filled(rect, radius, color.gamma_multiply(0.2));
        let inner = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        let stroke = egui::Stroke::new(1.0, color.gamma_multiply(0.7));
        let slant = rect.height();
        let mut x = rect.left() - slant;
        while x < rect.right() {
            inner.line_segment(
                [
                    egui::pos2(x, rect.bottom()),
                    egui::pos2(x + slant, rect.top()),
                ],
                stroke,
            );
            x += 6.0;
        }
        painter.rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.5, color),
            egui::StrokeKind::Inside,
        );
    } else {
        painter.rect_filled(rect, radius, color);
    }
}

fn diamond(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: egui::Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(center.x, center.y - radius),
            egui::pos2(center.x + radius, center.y),
            egui::pos2(center.x, center.y + radius),
            egui::pos2(center.x - radius, center.y),
        ],
        color,
        egui::Stroke::new(1.0, c().surface),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M").unwrap()
    }

    fn row(period: Option<(&str, &str)>, due: Option<&str>) -> TimelineRow {
        TimelineRow {
            id: 1,
            uid: String::new(),
            title: "Tarefa".into(),
            done: false,
            period: period.map(|(a, b)| (at(a), at(b))),
            projected: false,
            due: due.map(day),
            late: false,
        }
    }

    #[test]
    fn dates_map_to_columns_and_hours_move_inside_them() {
        // 2026-11-02 is a Monday.
        let scale = Scale::new(day("2026-11-02"), day("2026-11-08"), 70.0);
        assert_eq!(scale.days, 7);
        assert!(scale.day_w >= MIN_DAY_W);
        let w = scale.day_w;
        assert_eq!(scale.x_of_date(day("2026-11-02")), 0.0);
        assert_eq!(scale.x_of_date(day("2026-11-04")), 2.0 * w);
        assert!((scale.x_of(at("2026-11-03T12:00")) - 1.5 * w).abs() < 0.001);
        assert!((scale.x_of(at("2026-11-03T06:00")) - 1.25 * w).abs() < 0.001);
        assert_eq!(scale.width(), 7.0 * w);
        assert!((scale.deadline_x(day("2026-11-03")) - 2.0 * w).abs() < 0.001);
        assert!((scale.day_center(day("2026-11-02")) - 0.5 * w).abs() < 0.001);
        assert!(scale.contains(day("2026-11-08")) && !scale.contains(day("2026-11-09")));
    }

    #[test]
    fn a_short_chart_fills_the_view_and_a_long_one_keeps_a_minimum_day() {
        let short = Scale::new(day("2026-11-02"), day("2026-11-04"), 300.0);
        assert!((short.day_w - 100.0).abs() < 0.001);
        assert!((short.width() - 300.0).abs() < 0.001);
        let long = Scale::new(day("2026-01-01"), day("2026-12-31"), 600.0);
        assert_eq!(long.day_w, MIN_DAY_W);
        assert!(
            long.width() > 600.0,
            "the area scrolls instead of shrinking"
        );
        let one = Scale::new(day("2026-11-02"), day("2026-11-02"), 200.0);
        assert_eq!(one.days, 1);
    }

    #[test]
    fn bars_follow_the_period_and_never_vanish() {
        let scale = Scale::new(day("2026-11-02"), day("2026-11-08"), 700.0);
        let w = scale.day_w;
        let (x0, x1) = scale.bar((at("2026-11-03T00:00"), at("2026-11-05T00:00")));
        assert!((x0 - w).abs() < 0.001 && (x1 - 3.0 * w).abs() < 0.001);
        // A task of one hour is still a visible bar.
        let (x0, x1) = scale.bar((at("2026-11-03T09:00"), at("2026-11-03T10:00")));
        assert!(x1 - x0 >= MIN_BAR_W - 0.001);
        // At the very end it is pushed inside instead of cut.
        let (x0, x1) = scale.bar((at("2026-11-09T00:00"), at("2026-11-09T00:30")));
        assert!((x1 - scale.width()).abs() < 0.001);
        assert!(x1 - x0 >= MIN_BAR_W - 0.001 && x0 >= 0.0);
    }

    #[test]
    fn the_range_has_a_day_of_room_and_holds_the_target() {
        let span = Some((at("2026-11-04T09:00"), at("2026-11-06T18:00")));
        assert_eq!(
            chart_range(span, day("2026-11-05"), None),
            Some((day("2026-11-03"), day("2026-11-07")))
        );
        // A span ending at midnight sharp does not take the day that begins there.
        let sharp = Some((at("2026-11-04T00:00"), at("2026-11-06T00:00")));
        assert_eq!(
            chart_range(sharp, day("2026-11-05"), None),
            Some((day("2026-11-03"), day("2026-11-06")))
        );
        // The target date is always inside, and its own day too.
        assert_eq!(
            chart_range(span, day("2026-11-05"), Some(day("2026-11-20"))),
            Some((day("2026-11-03"), day("2026-11-21")))
        );
        assert_eq!(
            chart_range(span, day("2026-11-05"), Some(day("2026-10-20"))),
            Some((day("2026-10-20"), day("2026-11-07")))
        );
        // Today stretches the range only when it is near the tasks.
        assert_eq!(
            chart_range(span, day("2026-11-12"), None),
            Some((day("2026-11-03"), day("2026-11-12")))
        );
        assert_eq!(
            chart_range(span, day("2027-03-01"), None),
            Some((day("2026-11-03"), day("2026-11-07")))
        );
        assert_eq!(chart_range(None, day("2026-11-05"), None), None);
        // A stored date at chrono's limit must not panic the chart.
        let range = chart_range(span, day("2026-11-05"), Some(NaiveDate::MAX)).unwrap();
        assert_eq!(range.1, NaiveDate::MAX);
    }

    #[test]
    fn the_header_has_a_week_mark_on_each_monday_and_day_numbers_when_wide() {
        // Sunday 1/11 to Tuesday 10/11: Mondays are the 2nd and the 9th.
        let wide = Scale::new(day("2026-11-01"), day("2026-11-10"), 1000.0);
        let ticks = header_ticks(&wide);
        let weeks: Vec<&str> = ticks
            .iter()
            .filter(|t| t.week)
            .map(|t| t.label.as_str())
            .collect();
        assert_eq!(weeks, ["01/11", "02/11", "09/11"]);
        let days = ticks.iter().filter(|t| !t.week).count();
        assert_eq!(days, 10);
        let monday = ticks.iter().find(|t| t.label == "02/11").unwrap();
        assert!((monday.x - wide.day_w).abs() < 0.001);
        let narrow = Scale::new(day("2026-09-01"), day("2026-12-31"), 600.0);
        assert!(narrow.day_w < DAY_LABELS_FROM);
        assert!(header_ticks(&narrow).iter().all(|t| t.week));
    }

    #[test]
    fn days_off_are_merged_into_stretches() {
        let scale = Scale::new(day("2026-11-02"), day("2026-11-15"), 1400.0);
        let weekends = shaded_days(&scale, |d| {
            !matches!(d.weekday(), Weekday::Sat | Weekday::Sun)
        });
        assert_eq!(weekends.len(), 2);
        let w = scale.day_w;
        assert!((weekends[0].0 - 5.0 * w).abs() < 0.001);
        assert!((weekends[0].1 - 2.0 * w).abs() < 0.001);
        assert!((weekends[1].0 - 12.0 * w).abs() < 0.001);
        // A holiday in the middle of the week is a stretch of its own.
        let holiday = shaded_days(&scale, |d| d != day("2026-11-04"));
        assert_eq!(holiday, vec![(2.0 * w, w)]);
        assert!(shaded_days(&scale, |_| true).is_empty());
    }

    #[test]
    fn the_bar_colour_and_the_hatching_are_decided_apart() {
        let mut r = row(Some(("2026-11-03T09:00", "2026-11-04T09:00")), None);
        assert_eq!(bar_style(&r), (BarKind::Normal, false));
        r.projected = true;
        assert_eq!(bar_style(&r), (BarKind::Normal, true));
        r.late = true;
        assert_eq!(bar_style(&r), (BarKind::Late, true));
        r.projected = false;
        assert_eq!(bar_style(&r), (BarKind::Late, false));
        r.done = true;
        assert_eq!(bar_style(&r), (BarKind::Done, false), "concluded is muted");
    }

    #[test]
    fn the_hover_says_the_period_the_deadline_and_whether_it_is_a_projection() {
        let mut r = row(
            Some(("2026-11-03T09:00", "2026-11-04T18:00")),
            Some("2026-11-04"),
        );
        let text = row_tooltip(&r);
        assert!(text.starts_with("Tarefa\n"));
        assert!(text.contains("Período: 03/11 09:00 – 04/11 18:00"));
        assert!(text.contains("Prazo: 04/11/2026"));
        assert!(!text.contains("previsto"));
        r.projected = true;
        r.late = true;
        let text = row_tooltip(&r);
        assert!(text.contains("Período (previsto):"));
        assert!(text.contains("termina depois do prazo"));
        let bare = row_tooltip(&row(None, None));
        assert!(bare.contains("Sem período planejado") && bare.contains("Sem prazo"));
    }
}
