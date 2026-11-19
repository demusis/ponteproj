//! Days that are not worked in the planning: the national holidays, those of
//! the state the person chose and the days they registered. The last two
//! belong to the library (the `holidays` library setting, so they are the same
//! on every computer and phone that syncs it); `feriados.txt` (the phone: the
//! `holidays` preference) is only a copy kept so that the planning is right
//! before the library opens.
use chrono::{Datelike, NaiveDate, Weekday};
use std::borrow::Cow;
use std::sync::{Mutex, RwLock};

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
fn national(date: NaiveDate) -> Option<&'static str> {
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

/// The 27 states: code and full name, in the order of the picker.
pub const STATES: [(&str, &str); 27] = [
    ("AC", "Acre"),
    ("AL", "Alagoas"),
    ("AP", "Amapá"),
    ("AM", "Amazonas"),
    ("BA", "Bahia"),
    ("CE", "Ceará"),
    ("DF", "Distrito Federal"),
    ("ES", "Espírito Santo"),
    ("GO", "Goiás"),
    ("MA", "Maranhão"),
    ("MT", "Mato Grosso"),
    ("MS", "Mato Grosso do Sul"),
    ("MG", "Minas Gerais"),
    ("PA", "Pará"),
    ("PB", "Paraíba"),
    ("PR", "Paraná"),
    ("PE", "Pernambuco"),
    ("PI", "Piauí"),
    ("RJ", "Rio de Janeiro"),
    ("RN", "Rio Grande do Norte"),
    ("RS", "Rio Grande do Sul"),
    ("RO", "Rondônia"),
    ("RR", "Roraima"),
    ("SC", "Santa Catarina"),
    ("SP", "São Paulo"),
    ("SE", "Sergipe"),
    ("TO", "Tocantins"),
];

/// Full name of a state code ("SP" gives "São Paulo").
pub fn state_name(uf: &str) -> Option<&'static str> {
    STATES.iter().find(|(code, _)| *code == uf).map(|(_, n)| *n)
}

/// When a state holiday falls.
#[derive(Clone, Copy)]
enum When {
    /// Every year on month/day, from the given year on (0 = always).
    Fixed(u32, u32, i32),
    /// This many days after Easter Sunday.
    AfterEaster(i64),
}

/// State holidays that are not national (the same table is in `Holidays.kt`).
fn state_table(uf: &str) -> &'static [(When, &'static str)] {
    use When::{AfterEaster, Fixed};
    match uf {
        "AC" => &[
            (Fixed(1, 23, 0), "Dia do Evangélico"),
            (Fixed(6, 15, 0), "Aniversário do Acre"),
            (Fixed(9, 5, 0), "Dia da Amazônia"),
            (Fixed(11, 17, 0), "Tratado de Petrópolis"),
        ],
        "AL" => &[
            (Fixed(6, 24, 0), "São João"),
            (Fixed(6, 29, 0), "São Pedro"),
            (Fixed(9, 16, 0), "Emancipação de Alagoas"),
        ],
        "AP" => &[
            (Fixed(3, 19, 0), "São José"),
            (Fixed(9, 13, 0), "Criação do Amapá"),
        ],
        "AM" => &[(Fixed(9, 5, 0), "Elevação do Amazonas a Província")],
        "BA" => &[(Fixed(7, 2, 0), "Independência da Bahia")],
        "CE" => &[(Fixed(3, 25, 0), "Data Magna do Ceará")],
        "DF" => &[(Fixed(11, 30, 0), "Dia do Evangélico")],
        "ES" => &[(AfterEaster(8), "Nossa Senhora da Penha")],
        "MA" => &[(Fixed(7, 28, 0), "Adesão do Maranhão")],
        "MS" => &[(Fixed(10, 11, 0), "Criação de Mato Grosso do Sul")],
        "PA" => &[(Fixed(8, 15, 0), "Adesão do Pará")],
        "PB" => &[(Fixed(8, 5, 0), "Fundação da Paraíba")],
        "PE" => &[
            (Fixed(3, 6, 2017), "Data Magna de Pernambuco"),
            (Fixed(6, 24, 0), "São João"),
        ],
        "PI" => &[(Fixed(10, 19, 0), "Dia do Piauí")],
        "RJ" => &[(Fixed(4, 23, 0), "São Jorge")],
        "RN" => &[(Fixed(10, 3, 0), "Mártires de Cunhaú e Uruaçu")],
        "RO" => &[
            (Fixed(1, 4, 0), "Criação de Rondônia"),
            (Fixed(6, 18, 0), "Dia do Evangélico"),
        ],
        "RR" => &[(Fixed(10, 5, 0), "Criação de Roraima")],
        "RS" => &[(Fixed(9, 20, 0), "Revolução Farroupilha")],
        "SE" => &[(Fixed(7, 8, 0), "Emancipação de Sergipe")],
        "SP" => &[(Fixed(7, 9, 0), "Revolução Constitucionalista")],
        "TO" => &[
            (Fixed(10, 5, 0), "Criação do Tocantins"),
            (Fixed(9, 8, 0), "Nossa Senhora da Natividade"),
        ],
        _ => &[],
    }
}

/// The state's holidays as lines for the settings: when, and the name.
pub fn state_holiday_lines(uf: &str) -> Vec<(String, &'static str)> {
    let mut lines: Vec<(u32, u32, String, &'static str)> = state_table(uf)
        .iter()
        .map(|(when, name)| match *when {
            When::Fixed(m, d, _) => (m, d, format!("{d:02}/{m:02}"), *name),
            When::AfterEaster(n) => (13, 0, format!("{n} dias depois da Páscoa"), *name),
        })
        .collect();
    lines.sort_by_key(|(m, d, _, _)| (*m, *d));
    lines.into_iter().map(|(_, _, l, n)| (l, n)).collect()
}

fn state_holiday(uf: &str, date: NaiveDate) -> Option<&'static str> {
    state_table(uf).iter().find_map(|(when, name)| match *when {
        When::Fixed(m, d, from) => {
            (date.month() == m && date.day() == d && date.year() >= from).then_some(*name)
        }
        When::AfterEaster(n) => ((date - easter(date.year())).num_days() == n).then_some(*name),
    })
}

/// A non-working day the person registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomDay {
    /// `None`: every year.
    pub year: Option<i32>,
    pub month: u32,
    pub day: u32,
    pub name: String,
}

impl CustomDay {
    fn matches(&self, date: NaiveDate) -> bool {
        date.month() == self.month
            && date.day() == self.day
            && self.year.is_none_or(|y| y == date.year())
    }

    /// The name shown; an empty name becomes a generic one.
    fn label(&self) -> &str {
        if self.name.is_empty() {
            "Dia sem expediente"
        } else {
            &self.name
        }
    }
}

/// The person's choices: a state and extra days off. They live in the library
/// ([`adopt_library`], [`save_to_library`]); the local file is a cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HolidayConfig {
    /// State code, as in [`STATES`].
    pub uf: Option<&'static str>,
    pub custom: Vec<CustomDay>,
}

/// Name of the small local file, next to `lembretes.txt`.
const FILE_NAME: &str = "feriados.txt";

impl HolidayConfig {
    pub const fn new() -> Self {
        Self {
            uf: None,
            custom: Vec::new(),
        }
    }

    /// Lines `uf SP`, `anual 09-07 Nome`, `dia 2026-03-19 Nome`. Anything
    /// unreadable is skipped; unknown lines are ignored.
    pub fn parse(text: &str) -> Self {
        let mut config = Self::new();
        for line in text.lines() {
            let line = line.trim();
            let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let rest = rest.trim_start();
            let (value, name) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let name = name.trim().to_owned();
            match key {
                "uf" => {
                    config.uf = STATES.iter().find(|(c, _)| *c == value).map(|(c, _)| *c);
                }
                "anual" => {
                    if let Some((m, d)) = value.split_once('-') {
                        if let (Ok(m), Ok(d)) = (m.parse::<u32>(), d.parse::<u32>()) {
                            // 2024 is a leap year: 02-29 is accepted.
                            if NaiveDate::from_ymd_opt(2024, m, d).is_some() {
                                config.custom.push(CustomDay {
                                    year: None,
                                    month: m,
                                    day: d,
                                    name,
                                });
                            }
                        }
                    }
                }
                "dia" => {
                    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
                        config.custom.push(CustomDay {
                            year: Some(date.year()),
                            month: date.month(),
                            day: date.day(),
                            name,
                        });
                    }
                }
                _ => {}
            }
        }
        config
    }

    pub fn to_text(&self) -> String {
        let mut text = String::new();
        if let Some(uf) = self.uf {
            text.push_str(&format!("uf {uf}\n"));
        }
        for day in &self.custom {
            let name = day.name.replace(['\n', '\r'], " ");
            let name = name.trim();
            let line = match day.year {
                None => format!("anual {:02}-{:02} {name}", day.month, day.day),
                Some(y) => format!("dia {y:04}-{:02}-{:02} {name}", day.month, day.day),
            };
            text.push_str(line.trim_end());
            text.push('\n');
        }
        text
    }

    fn path() -> Option<std::path::PathBuf> {
        crate::paths::data_dir().ok().map(|dir| dir.join(FILE_NAME))
    }

    /// The local copy. Unreadable or missing file: no state and no extra days.
    pub fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// Best effort: a failure to write is not shown to the person.
    pub fn save(&self) {
        if let Some(path) = Self::path() {
            let _ = std::fs::write(path, self.to_text());
        }
    }
}

/// What the library says: `None` when nobody ever set it (a cleared
/// configuration is `Some` of an empty one).
pub fn library_config(store: &crate::storage::Store) -> anyhow::Result<Option<HolidayConfig>> {
    Ok(store
        .library_setting(crate::library_settings::HOLIDAYS)?
        .map(|text| HolidayConfig::parse(&text)))
}

/// Puts `config` in the library, for every computer and phone that syncs it.
pub fn save_to_library(
    store: &mut crate::storage::Store,
    config: &HolidayConfig,
) -> anyhow::Result<()> {
    store.set_library_setting(crate::library_settings::HOLIDAYS, &config.to_text())
}

/// The configuration this device should use now. The library wins; when it
/// has none yet, the device's own (`local`, the file or preference it kept
/// until now) goes into it, once, so the person does not set it again.
pub fn adopt_library(
    store: &mut crate::storage::Store,
    local: &HolidayConfig,
) -> anyhow::Result<HolidayConfig> {
    if let Some(config) = library_config(store)? {
        return Ok(config);
    }
    if *local != HolidayConfig::new() {
        save_to_library(store, local)?;
    }
    Ok(local.clone())
}

/// The configuration the planning consults (process-wide).
static CONFIG: RwLock<HolidayConfig> = RwLock::new(HolidayConfig::new());

/// Tests that change the process-wide configuration take this first.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Makes `config` the one the planning uses from now on.
pub fn set_config(config: HolidayConfig) {
    if let Ok(mut guard) = CONFIG.write() {
        *guard = config;
    }
}

/// The configuration in use.
pub fn config() -> HolidayConfig {
    CONFIG.read().map(|c| c.clone()).unwrap_or_default()
}

/// Names of the person's days, kept for the whole run so that [`holiday`]
/// can hand out `&'static str`. Few and short: one per distinct name shown.
fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let Ok(mut names) = NAMES.lock() else {
        return "Dia sem expediente";
    };
    if let Some(known) = names.iter().find(|n| **n == name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}

/// The holiday on `date` under `config`: national, then the state's, then
/// the person's own days.
pub fn holiday_with(date: NaiveDate, config: &HolidayConfig) -> Option<Cow<'static, str>> {
    if let Some(name) = national(date) {
        return Some(Cow::Borrowed(name));
    }
    if let Some(name) = config.uf.and_then(|uf| state_holiday(uf, date)) {
        return Some(Cow::Borrowed(name));
    }
    config
        .custom
        .iter()
        .find(|day| day.matches(date))
        .map(|day| Cow::Owned(day.label().to_owned()))
}

/// The name of the holiday (or day off) on `date` with the configuration in
/// use, if there is one.
pub fn holiday(date: NaiveDate) -> Option<&'static str> {
    if let Some(name) = national(date) {
        return Some(name);
    }
    let guard = CONFIG.read().ok()?;
    match holiday_with(date, &guard)? {
        Cow::Borrowed(name) => Some(name),
        Cow::Owned(name) => Some(intern(&name)),
    }
}

/// Monday to Friday and not a holiday, under the given configuration.
pub fn is_working_day_with(date: NaiveDate, config: &HolidayConfig) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && holiday_with(date, config).is_none()
}

/// Monday to Friday and not a holiday (national, state or the person's own).
pub fn is_working_day(date: NaiveDate) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && holiday(date).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_library_wins_and_a_local_choice_migrates_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::storage::Store::open(&dir.path().join("a.sqlite3")).unwrap();
        let local = HolidayConfig::parse(
            "uf SP
anual 05-02 Recesso
",
        );
        // Nothing set: nothing to take, and an empty local choice writes nothing.
        assert_eq!(library_config(&store).unwrap(), None);
        assert_eq!(
            adopt_library(&mut store, &HolidayConfig::new()).unwrap(),
            HolidayConfig::new()
        );
        assert_eq!(library_config(&store).unwrap(), None);
        // The local choice goes into the library, the first time.
        assert_eq!(adopt_library(&mut store, &local).unwrap(), local);
        assert_eq!(library_config(&store).unwrap(), Some(local.clone()));
        // Later, the library wins over whatever this device kept.
        let other = HolidayConfig::parse(
            "uf RJ
",
        );
        assert_eq!(adopt_library(&mut store, &other).unwrap(), local);
        // Cleared on purpose stays cleared; an old local file does not return.
        save_to_library(&mut store, &HolidayConfig::new()).unwrap();
        assert_eq!(
            adopt_library(&mut store, &local).unwrap(),
            HolidayConfig::new()
        );
    }
    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    fn name(date: NaiveDate, config: &HolidayConfig) -> Option<String> {
        holiday_with(date, config).map(|n| n.into_owned())
    }
    fn none() -> HolidayConfig {
        HolidayConfig::new()
    }
    fn uf(code: &str) -> HolidayConfig {
        HolidayConfig::parse(&format!("uf {code}\n"))
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
        assert_eq!(national(d(2026, 2, 16)), Some("Carnaval"));
        assert_eq!(national(d(2026, 2, 17)), Some("Carnaval"));
        assert_eq!(national(d(2026, 2, 18)), None);
        assert_eq!(national(d(2026, 4, 3)), Some("Sexta-feira Santa"));
        assert_eq!(national(d(2026, 6, 4)), Some("Corpus Christi"));
    }
    #[test]
    fn fixed_holidays_and_consciencia_negra_rule() {
        assert_eq!(national(d(2026, 4, 21)), Some("Tiradentes"));
        assert_eq!(national(d(2026, 12, 25)), Some("Natal"));
        assert_eq!(national(d(2023, 11, 20)), None);
        assert_eq!(national(d(2024, 11, 20)), Some("Dia da Consciência Negra"));
        assert_eq!(national(d(2026, 11, 20)), Some("Dia da Consciência Negra"));
    }
    #[test]
    fn working_days() {
        assert!(is_working_day_with(d(2026, 4, 22), &none()));
        assert!(!is_working_day_with(d(2026, 4, 21), &none()));
        assert!(!is_working_day_with(d(2026, 4, 25), &none()));
        assert!(!is_working_day_with(d(2026, 11, 20), &none()));
    }
    #[test]
    fn the_file_round_trips_and_garbage_is_ignored() {
        let config = HolidayConfig::parse(
            "uf SP\nanual 09-07 Dia da cidade\ndia 2026-03-19 Ponte  do santo\nanual 02-29\n",
        );
        assert_eq!(config.uf, Some("SP"));
        assert_eq!(config.custom.len(), 3);
        assert_eq!(config.custom[1].name, "Ponte  do santo");
        assert_eq!(config.custom[2].name, "");
        assert_eq!(HolidayConfig::parse(&config.to_text()), config);
        let garbage = HolidayConfig::parse(
            "lixo\nuf XX\nanual 13-40 x\nanual abc\ndia ontem x\ndia\nuf\n\u{0}\u{0}\n   \nfoo bar baz",
        );
        assert_eq!(garbage, HolidayConfig::new());
        assert_eq!(HolidayConfig::parse(""), HolidayConfig::new());
    }
    #[test]
    fn sao_paulo_day_counts_only_in_sao_paulo() {
        // 9/7/2026 is a Thursday.
        assert!(is_working_day_with(d(2026, 7, 9), &none()));
        assert!(is_working_day_with(d(2026, 7, 9), &uf("RJ")));
        let sp = uf("SP");
        assert!(!is_working_day_with(d(2026, 7, 9), &sp));
        assert_eq!(
            name(d(2026, 7, 9), &sp).as_deref(),
            Some("Revolução Constitucionalista")
        );
    }
    #[test]
    fn espirito_santo_penha_is_the_monday_after_easter_week() {
        // Easter 2026 is 5 April: the Monday eight days later is 13 April.
        let es = uf("ES");
        assert_eq!(
            name(d(2026, 4, 13), &es).as_deref(),
            Some("Nossa Senhora da Penha")
        );
        assert_eq!(name(d(2026, 4, 13), &none()), None);
        assert_eq!(name(d(2026, 4, 12), &es), None);
    }
    #[test]
    fn pernambuco_data_magna_starts_in_2017() {
        let pe = uf("PE");
        assert_eq!(name(d(2016, 3, 6), &pe), None);
        assert!(name(d(2017, 3, 6), &pe).is_some());
        assert!(name(d(2026, 6, 24), &pe).is_some());
    }
    #[test]
    fn national_comes_first_and_every_state_is_known() {
        assert_eq!(STATES.len(), 27);
        let config = HolidayConfig::parse("uf MG\nanual 04-21 Outro\n");
        assert_eq!(name(d(2026, 4, 21), &config).as_deref(), Some("Tiradentes"));
        for (code, full) in STATES {
            assert_eq!(state_name(code), Some(full));
        }
        assert!(state_holiday_lines("GO").is_empty());
        assert_eq!(state_holiday_lines("AC").len(), 4);
        assert_eq!(state_holiday_lines("SP")[0].0, "09/07");
    }
    #[test]
    fn custom_days_annual_and_single() {
        let config =
            HolidayConfig::parse("anual 03-19 São José\ndia 2026-05-04 Recesso\nanual 06-10\n");
        assert_eq!(name(d(2027, 3, 19), &config).as_deref(), Some("São José"));
        assert_eq!(name(d(2026, 5, 4), &config).as_deref(), Some("Recesso"));
        assert_eq!(name(d(2027, 5, 4), &config), None);
        assert_eq!(
            name(d(2026, 6, 10), &config).as_deref(),
            Some("Dia sem expediente")
        );
        assert!(!is_working_day_with(d(2026, 5, 4), &config));
        assert!(is_working_day_with(d(2026, 5, 5), &config));
    }
    #[test]
    fn the_process_wide_config_feeds_holiday_and_is_working_day() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A date no other test of the crate uses.
        let date = d(2031, 3, 17);
        assert_eq!(holiday(date), None);
        set_config(HolidayConfig::parse("dia 2031-03-17 Recesso do órgão\n"));
        assert_eq!(holiday(date), Some("Recesso do órgão"));
        assert!(!is_working_day(date));
        assert_eq!(config().custom.len(), 1);
        set_config(HolidayConfig::new());
        assert_eq!(holiday(date), None);
    }
}
