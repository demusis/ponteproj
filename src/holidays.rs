//! Brazilian national holidays: days that are not worked in the planning.
use chrono::{Datelike, NaiveDate, Weekday};

/// Easter Sunday of `year` (anonymous Gregorian algorithm, Meeus/Jones/Butcher).
fn easter(year: i32) -> NaiveDate {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32).unwrap()
}

/// The name of the national holiday on `date`, if it is one.
pub fn holiday(date: NaiveDate) -> Option<&'static str> {
    let fixed = match (date.month(), date.day()) {
        (1, 1) => Some("Confraternização Universal"),
        (4, 21) => Some("Tiradentes"),
        (5, 1) => Some("Dia do Trabalho"),
        (9, 7) => Some("Independência"),
        (10, 12) => Some("Nossa Senhora Aparecida"),
        (11, 2) => Some("Finados"),
        (11, 15) => Some("Proclamação da República"),
        (11, 20) if date.year() >= 2024 => Some("Dia da Consciência Negra"),
        (12, 25) => Some("Natal"),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    let offset = (date - easter(date.year())).num_days();
    match offset {
        -48 | -47 => Some("Carnaval"),
        -2 => Some("Sexta-feira Santa"),
        60 => Some("Corpus Christi"),
        _ => None,
    }
}

/// Monday to Friday and not a national holiday.
pub fn is_working_day(date: NaiveDate) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && holiday(date).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    #[test]
    fn easter_dates() {
        assert_eq!(easter(2024), d(2024, 3, 31));
        assert_eq!(easter(2025), d(2025, 4, 20));
        assert_eq!(easter(2026), d(2026, 4, 5));
        assert_eq!(easter(2027), d(2027, 3, 28));
    }
    #[test]
    fn movable_holidays_2026() {
        assert_eq!(holiday(d(2026, 2, 16)), Some("Carnaval"));
        assert_eq!(holiday(d(2026, 2, 17)), Some("Carnaval"));
        assert_eq!(holiday(d(2026, 2, 18)), None);
        assert_eq!(holiday(d(2026, 4, 3)), Some("Sexta-feira Santa"));
        assert_eq!(holiday(d(2026, 6, 4)), Some("Corpus Christi"));
    }
    #[test]
    fn fixed_holidays_and_consciencia_negra_rule() {
        assert_eq!(holiday(d(2026, 4, 21)), Some("Tiradentes"));
        assert_eq!(holiday(d(2026, 12, 25)), Some("Natal"));
        assert_eq!(holiday(d(2023, 11, 20)), None);
        assert_eq!(holiday(d(2024, 11, 20)), Some("Dia da Consciência Negra"));
        assert_eq!(holiday(d(2026, 11, 20)), Some("Dia da Consciência Negra"));
    }
    #[test]
    fn working_days() {
        assert!(is_working_day(d(2026, 4, 22)));
        assert!(!is_working_day(d(2026, 4, 21)));
        assert!(!is_working_day(d(2026, 4, 25)));
        assert!(!is_working_day(d(2026, 11, 20)));
    }
}
