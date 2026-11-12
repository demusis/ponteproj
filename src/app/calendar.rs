//! The month grid shared by "Hoje" (deadlines) and "Recursos" (uses): the
//! month arithmetic, the Portuguese names, the header with the arrows and
//! the grid itself. What goes inside each day is drawn by the caller.
use crate::theme::{self, icon};
use crate::widgets;
use chrono::{Datelike, NaiveDate};
use eframe::egui;

pub(crate) const MONTHS: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// Weekday initials of the calendar, Sunday first.
pub(crate) const WEEKDAY_INITIALS: [&str; 7] = ["D", "S", "T", "Q", "Q", "S", "S"];

/// "novembro de 2026".
pub(crate) fn month_title(year: i32, month: u32) -> String {
    format!("{} de {year}", MONTHS[(month.clamp(1, 12) - 1) as usize])
}

/// The month `delta` months away from `(year, month)`.
pub(crate) fn shift_month((year, month): (i32, u32), delta: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + delta;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

/// The weeks that cover a month, Sunday first (4 to 6 rows of seven dates);
/// the first and last rows carry days of the neighbouring months.
pub(crate) fn month_weeks(year: i32, month: u32) -> Vec<[NaiveDate; 7]> {
    use chrono::Duration;
    let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return Vec::new();
    };
    let (next_year, next_month) = shift_month((year, month), 1);
    let Some(next_first) = NaiveDate::from_ymd_opt(next_year, next_month, 1) else {
        return Vec::new();
    };
    let last = next_first - Duration::days(1);
    let mut start = first - Duration::days(i64::from(first.weekday().num_days_from_sunday()));
    let mut weeks = Vec::new();
    while start <= last {
        weeks.push(std::array::from_fn(|i| start + Duration::days(i as i64)));
        start += Duration::days(7);
    }
    weeks
}

/// "terça-feira, 10 de novembro".
pub(crate) fn long_date(date: NaiveDate) -> String {
    const DAYS: [&str; 7] = [
        "segunda-feira",
        "terça-feira",
        "quarta-feira",
        "quinta-feira",
        "sexta-feira",
        "sábado",
        "domingo",
    ];
    format!(
        "{}, {} de {}",
        DAYS[date.weekday().num_days_from_monday() as usize],
        date.day(),
        MONTHS[date.month0() as usize]
    )
}

pub(crate) const LINE_H: f32 = 18.0;
pub(crate) const HEAD_H: f32 = 26.0;
pub(crate) const DAY_NUMBER_H: f32 = 28.0;
pub(crate) const MIN_ROW_H: f32 = 84.0;

/// One line of the grid: text cut with an ellipsis at the width of the rectangle.
pub(crate) fn paint_line(
    painter: &egui::Painter,
    rect: egui::Rect,
    text: &str,
    color: egui::Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        theme::font(theme::SIZE_SMALL),
        color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 8.0).max(1.0));
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// The arrows, the month name and "Hoje" over the grid. Returns the month
/// on show after the clicks.
pub(crate) fn month_nav(
    ui: &mut egui::Ui,
    (mut year, mut month): (i32, u32),
    today: NaiveDate,
) -> (i32, u32) {
    ui.horizontal(|ui| {
        if widgets::icon_button(ui, icon::LEFT, "Mês anterior").clicked() {
            (year, month) = shift_month((year, month), -1);
        }
        ui.allocate_ui_with_layout(
            egui::vec2(170.0, 30.0),
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| widgets::section(ui, month_title(year, month)),
        );
        if widgets::icon_button(ui, icon::RIGHT, "Próximo mês").clicked() {
            (year, month) = shift_month((year, month), 1);
        }
        if ui
            .add(widgets::quiet("Hoje"))
            .on_hover_text("Voltar ao mês atual")
            .clicked()
        {
            (year, month) = (today.year(), today.month());
        }
    });
    (year, month)
}

/// Height of a week row when the grid may use `avail_h` and has `rows` weeks.
pub(crate) fn grid_row_h(avail_h: f32, rows: usize) -> f32 {
    ((avail_h - 44.0 - HEAD_H) / rows.max(1) as f32).max(MIN_ROW_H)
}

/// How many deadline lines fit in a day cell of height `row_h`.
pub(crate) fn lines_that_fit(row_h: f32) -> usize {
    (((row_h - DAY_NUMBER_H - 2.0) / LINE_H).floor() as usize).max(1)
}

/// The `i`-th line of a day cell.
pub(crate) fn line_rect(cell: egui::Rect, col_w: f32, i: usize) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            cell.left() + 4.0,
            cell.top() + DAY_NUMBER_H + LINE_H * i as f32,
        ),
        egui::vec2(col_w - 8.0, LINE_H - 2.0),
    )
}

/// A day cell, as the grid hands it to the caller.
pub(crate) struct Cell<'a> {
    pub iso: &'a str,
    pub in_month: bool,
    pub rect: egui::Rect,
    pub col_w: f32,
    pub row_h: f32,
}

/// Draws the grid of a month: weekday letters, weekends sunken, the day
/// number (today in a circle), today's outline and the lines between cells.
/// `clickable(iso)` says which days answer to a click (they are tinted when
/// `selected`); `content` draws what is inside a day and returns true when
/// something in it asked for the day to be opened. Returns the day clicked.
#[allow(clippy::too_many_arguments)]
pub(crate) fn month_grid(
    ui: &mut egui::Ui,
    base: egui::Id,
    (year, month): (i32, u32),
    today: NaiveDate,
    selected: &str,
    row_h: f32,
    clickable: impl Fn(&str) -> bool,
    mut content: impl FnMut(&mut egui::Ui, &egui::Painter, &Cell<'_>) -> bool,
) -> Option<String> {
    let p = theme::c();
    let weeks = month_weeks(year, month);
    let width = ui.available_width();
    let col_w = width / 7.0;
    let rows = weeks.len().max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, HEAD_H + row_h * rows),
        egui::Sense::hover(),
    );
    let painter = ui.painter().clone();
    let radius = theme::RADIUS_CARD;
    painter.rect_filled(rect, egui::CornerRadius::same(radius), p.surface);
    let body_top = rect.top() + HEAD_H;
    for (c, initial) in WEEKDAY_INITIALS.iter().enumerate() {
        painter.text(
            egui::pos2(
                rect.left() + col_w * (c as f32 + 0.5),
                rect.top() + HEAD_H / 2.0,
            ),
            egui::Align2::CENTER_CENTER,
            initial,
            theme::font_strong(theme::SIZE_SMALL),
            p.muted,
        );
    }
    let mut chosen: Option<String> = None;
    let last_row = weeks.len().saturating_sub(1);
    for (r, week) in weeks.iter().enumerate() {
        for (c, date) in week.iter().enumerate() {
            let cell = egui::Rect::from_min_size(
                egui::pos2(rect.left() + col_w * c as f32, body_top + row_h * r as f32),
                egui::vec2(col_w, row_h),
            );
            let in_month = date.month() == month;
            let iso = date.to_string();
            let has_items = in_month && clickable(&iso);
            // Weekends sunken; the open day tinted.
            let weekend = c == 0 || c == 6;
            let fill = if has_items && selected == iso {
                Some(p.primary_soft)
            } else if weekend {
                Some(p.sunken)
            } else {
                None
            };
            if let Some(fill) = fill {
                let corner = |on: bool| if on { radius } else { 0 };
                painter.rect_filled(
                    cell.shrink(0.5),
                    egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: corner(r == last_row && c == 0),
                        se: corner(r == last_row && c == 6),
                    },
                    fill,
                );
            }
            if has_items {
                let response = ui
                    .interact(
                        cell,
                        base.with(("cal-cell", iso.as_str())),
                        egui::Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    chosen = Some(iso.clone());
                }
            }
            // The day number; today inside a filled circle.
            let center = egui::pos2(cell.left() + 17.0, cell.top() + 14.0);
            let number_color = if *date == today {
                painter.circle_filled(center, 11.0, p.primary);
                p.on_primary
            } else if in_month {
                p.text
            } else {
                p.muted
            };
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                date.day(),
                theme::font_strong(13.0),
                number_color,
            );
            let info = Cell {
                iso: &iso,
                in_month,
                rect: cell,
                col_w,
                row_h,
            };
            if content(ui, &painter, &info) {
                chosen = Some(iso.clone());
            }
            if *date == today {
                painter.rect_stroke(
                    cell.shrink(1.0),
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(2.0, p.primary),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
    // Grid lines and the outer border over the fills.
    let line = egui::Stroke::new(1.0, p.outline);
    painter.hline(rect.x_range(), body_top, line);
    for r in 1..weeks.len() {
        painter.hline(rect.x_range(), body_top + row_h * r as f32, line);
    }
    for c in 1..7 {
        painter.vline(
            rect.left() + col_w * c as f32,
            body_top..=rect.bottom(),
            line,
        );
    }
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(radius),
        line,
        egui::StrokeKind::Inside,
    );
    chosen
}
