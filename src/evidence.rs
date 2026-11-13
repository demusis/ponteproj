//! Source snapshots and literal citation verification. No network access.
use crate::storage::Store;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub document_id: i64,
    pub capture_id: i64,
    pub label: String,
    pub file_id: Option<i64>,
    pub page: Option<u32>,
    pub method: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub source_id: String,
    pub quote: String,
    pub claim: String,
}
#[derive(Clone, Default)]
pub struct Prepared {
    pub sources: Vec<Source>,
    pub pending: Vec<String>,
}
fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn validate(sources: &[Source], citations: &[Citation]) -> Result<()> {
    ensure!(
        sources.len() <= 128 && citations.len() <= 32,
        "Há fontes ou citações demais."
    );
    let mut ids = BTreeSet::new();
    let mut size = 0;
    for source in sources {
        ensure!(
            ids.insert(&source.id)
                && source.document_id > 0
                && source.capture_id > 0
                && source.id.len() <= 128
                && source.label.len() <= 1000
                && !source.text.trim().is_empty()
                && source.text.len() <= 500 * 1024,
            "Fonte inválida ou duplicada."
        );
        ensure!(
            ["captura", "pdf", "ocr"].contains(&source.method.as_str()),
            "Forma de obtenção da fonte inválida."
        );
        ensure!(
            source.method == "captura"
                || (source.file_id.is_some_and(|id| id > 0) && source.page.is_some_and(|p| p > 0)),
            "Página ou arquivo ausente na fonte."
        );
        size += source.text.len();
    }
    ensure!(
        size <= 500 * 1024,
        "As fontes selecionadas passam de 500 KB; escolha menos páginas."
    );
    for citation in citations {
        let source = sources
            .iter()
            .find(|s| s.id == citation.source_id)
            .ok_or_else(|| anyhow::anyhow!("A IA citou uma fonte que não foi selecionada."))?;
        let quote = normalized(&citation.quote);
        ensure!(
            citation.quote.len() <= 8000
                && (12..=2000).contains(&quote.len())
                && !citation.claim.trim().is_empty()
                && citation.claim.len() <= 4000,
            "Citação vazia ou fora dos limites."
        );
        ensure!(
            normalized(&source.text).contains(&quote),
            "O trecho citado pela IA não foi encontrado literalmente na fonte indicada; a resposta não foi aceita."
        );
    }
    Ok(())
}
// ---------------------------------------------------------------------------
// Field verification: pure, conservative readers of what a quote says. When
// unsure they return nothing; a value that is not read here is never
// "conferido".

/// Whether `quote` is written, literally (white space collapsed, case
/// sensitive), where a fact says it is: `source_id == ""` is the document
/// text or the human context, otherwise the text of that selected source.
pub fn find_quote(
    quote: &str,
    capture_text: &str,
    human_context: &str,
    sources: &[Source],
    source_id: &str,
) -> bool {
    quote_origin(quote, capture_text, human_context, sources, source_id)
        != crate::model::Origin::Nowhere
}

/// Lowercase without accents, for comparing words.
pub(crate) fn fold(word: &str) -> String {
    word.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

fn digit_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    end
}

fn alpha_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && chars[end].is_alphabetic() {
        end += 1;
    }
    end
}

/// The alphabetic word that starts at `start`, folded, and where it ends.
fn word_at(chars: &[char], start: usize) -> (String, usize) {
    let end = alpha_end(chars, start);
    (fold(&chars[start..end].iter().collect::<String>()), end)
}

fn skip_spaces(chars: &[char], mut at: usize) -> usize {
    while at < chars.len() && chars[at].is_whitespace() {
        at += 1;
    }
    at
}

/// The word after `at` (spaces skipped), folded, and where it ends; empty
/// when something else follows.
fn next_word(chars: &[char], at: usize) -> (String, usize) {
    let start = skip_spaces(chars, at);
    if start < chars.len() && chars[start].is_alphabetic() {
        word_at(chars, start)
    } else {
        (String::new(), start)
    }
}

/// The word (folded) or the symbol `$`/`§` before `at`, looking back over
/// spaces, dots and colons ("art. 9" gives "art"), and where it starts.
fn prev_token_at(chars: &[char], at: usize) -> (String, usize) {
    let mut k = at;
    while k > 0 && (chars[k - 1].is_whitespace() || matches!(chars[k - 1], '.' | ':')) {
        k -= 1;
    }
    if k == 0 {
        return (String::new(), at);
    }
    match chars[k - 1] {
        '$' => ("$".into(), k - 1),
        '§' => ("§".into(), k - 1),
        c if c.is_alphabetic() => {
            let end = k;
            while k > 0 && chars[k - 1].is_alphabetic() {
                k -= 1;
            }
            let token = fold(&chars[k..end].iter().collect::<String>())
                .trim_end_matches(['º', '°', 'ª'])
                .to_owned();
            (token, k)
        }
        _ => (String::new(), at),
    }
}

fn prev_token(chars: &[char], at: usize) -> String {
    prev_token_at(chars, at).0
}

/// A number may start at `at` only when it is not the tail of a longer
/// code: not after a letter or digit, and not after a separator that joins
/// it to one ("ORG-OFI-2026/00431", "123.456.789-09").
fn starts_alone(chars: &[char], at: usize) -> bool {
    if at == 0 {
        return true;
    }
    let before = chars[at - 1];
    if before.is_alphanumeric() || before == '_' || before == '+' {
        return false;
    }
    if at < 2 {
        return true;
    }
    let beyond = chars[at - 2];
    match before {
        '/' | '-' | '.' | ',' => !beyond.is_alphanumeric(),
        ':' => !beyond.is_ascii_digit(),
        _ => true,
    }
}

/// A date may end at `at` only when no longer code goes on.
fn ends_alone(chars: &[char], at: usize) -> bool {
    match chars.get(at) {
        None => true,
        Some(c) if c.is_alphanumeric() => false,
        Some('/' | '-' | '.' | ',') => !chars.get(at + 1).is_some_and(char::is_ascii_digit),
        Some(_) => true,
    }
}

const MONTHS: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "marco",
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

/// Month number of a folded word: the full name or the three-letter form.
fn month_of(word: &str) -> Option<u32> {
    MONTHS
        .iter()
        .position(|name| *name == word || (word.len() == 3 && name.starts_with(word)))
        .map(|index| index as u32 + 1)
}

struct Hit<T> {
    start: usize,
    end: usize,
    value: T,
}

fn year_ok(year: i32) -> bool {
    (1900..=2100).contains(&year)
}

fn iso(year: i32, month: u32, day: u32) -> Option<String> {
    if !year_ok(year) {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .map(|date| date.format("%Y-%m-%d").to_string())
}

fn text_of(chars: &[char], from: usize, to: usize) -> String {
    chars[from..to].iter().collect()
}

/// `dd/mm/aaaa`, `dd-mm-aaaa`, `dd.mm.aaaa`, `dd/mm/aa` (with `/` or `-`
/// only: "2.1.10" is an item number) and `aaaa-mm-dd`.
fn numeric_date_at(chars: &[char], start: usize) -> Option<Hit<String>> {
    let a_end = digit_end(chars, start);
    let separator = *chars.get(a_end).filter(|c| matches!(c, '/' | '-' | '.'))?;
    let b_start = a_end + 1;
    let b_end = digit_end(chars, b_start);
    if chars.get(b_end) != Some(&separator) {
        return None;
    }
    let c_start = b_end + 1;
    let c_end = digit_end(chars, c_start);
    let (a, b, c) = (a_end - start, b_end - b_start, c_end - c_start);
    let candidate = format!(
        "{}{separator}{}{separator}{}",
        text_of(chars, start, a_end),
        text_of(chars, b_start, b_end),
        text_of(chars, c_start, c_end)
    );
    let iso_shape = a == 4 && b == 2 && c == 2 && separator == '-';
    let day_first =
        (1..=2).contains(&a) && (1..=2).contains(&b) && (c == 4 || (c == 2 && separator != '.'));
    if !iso_shape && !day_first {
        return None;
    }
    let value = crate::labels::parse_typed_date(&candidate)?;
    let year: i32 = value[..4].parse().ok()?;
    (year_ok(year) && ends_alone(chars, c_end)).then_some(Hit {
        start,
        end: c_end,
        value,
    })
}

/// Skips what may stand between the parts of a written date: spaces with an
/// optional "de", a slash or a dash.
fn date_gap(chars: &[char], at: usize) -> Option<usize> {
    if matches!(chars.get(at), Some('/' | '-')) {
        return Some(at + 1);
    }
    let after_spaces = skip_spaces(chars, at);
    if after_spaces == at {
        return None;
    }
    let (word, end) = word_at(chars, after_spaces);
    if word == "de" {
        let next = skip_spaces(chars, end);
        return (next > end).then_some(next);
    }
    Some(after_spaces)
}

/// "12 de novembro de 2026", "12 nov. 2026", "1º de março de 2026". The
/// year is required (four digits): "12 de novembro" says no year.
fn written_date_at(chars: &[char], start: usize) -> Option<Hit<String>> {
    let day_end = digit_end(chars, start);
    if day_end - start > 2 {
        return None;
    }
    let day: u32 = text_of(chars, start, day_end).parse().ok()?;
    let mut at = day_end;
    let ordinal = match chars.get(at) {
        Some('º' | '°' | 'ª') => true,
        Some('o') => !chars.get(at + 1).is_some_and(|c| c.is_alphabetic()),
        _ => false,
    };
    if ordinal {
        at += 1;
    }
    at = date_gap(chars, at)?;
    if !chars.get(at).is_some_and(|c| c.is_alphabetic()) {
        return None;
    }
    let (word, mut end) = word_at(chars, at);
    let month = month_of(&word)?;
    if word.len() == 3 && chars.get(end) == Some(&'.') {
        end += 1;
    }
    let year_start = date_gap(chars, end)?;
    let year_end = digit_end(chars, year_start);
    if year_end - year_start != 4 || !ends_alone(chars, year_end) {
        return None;
    }
    let year: i32 = text_of(chars, year_start, year_end).parse().ok()?;
    iso(year, month, day).map(|value| Hit {
        start,
        end: year_end,
        value,
    })
}

fn scan_dates(chars: &[char]) -> Vec<Hit<String>> {
    let mut hits = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if chars[at].is_ascii_digit() && (at == 0 || !chars[at - 1].is_ascii_digit()) {
            if starts_alone(chars, at) {
                if let Some(hit) = numeric_date_at(chars, at).or_else(|| written_date_at(chars, at))
                {
                    at = hit.end;
                    hits.push(hit);
                    continue;
                }
            }
            at = digit_end(chars, at);
        } else {
            at += 1;
        }
    }
    hits
}

fn unique<T: PartialEq>(values: impl Iterator<Item = T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// The dates a text writes, as ISO `AAAA-MM-DD`, in order. Shapes:
/// dd/mm/aaaa, dd-mm-aaaa, dd.mm.aaaa, dd/mm/aa, aaaa-mm-dd, "12 de
/// novembro de 2026", "12 nov. 2026", "1º de março de 2026". Never a
/// relative expression, a day without month and year, an invalid date, or
/// digits inside a document number.
pub fn dates_in(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    unique(scan_dates(&chars).into_iter().map(|hit| hit.value))
}

/// The dates a text writes with where each one is, in characters:
/// `(ISO, start, end)`. As [`dates_in`], with every occurrence.
pub fn date_spans(text: &str) -> Vec<(String, usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    scan_dates(&chars)
        .into_iter()
        .map(|hit| (hit.value, hit.start, hit.end))
        .collect()
}

const HOUR_WORDS: [&str; 6] = ["h", "hs", "hrs", "hr", "hora", "horas"];
/// A number of hours after these words is a length, not a time of day.
const LENGTH_BEFORE: [&str; 15] = [
    "por",
    "durante",
    "cerca",
    "aproximadamente",
    "duracao",
    "total",
    "mais",
    "menos",
    "em",
    "dentro",
    "ha",
    "carga",
    "prazo",
    "tempo",
    "demora",
];
const LENGTH_AFTER: [&str; 16] = [
    "min",
    "minutos",
    "diarias",
    "diaria",
    "semanais",
    "semanal",
    "mensais",
    "mensal",
    "aula",
    "aulas",
    "extras",
    "extra",
    "trabalhadas",
    "letivas",
    "lectivas",
    "uteis",
];
const LENGTH_AFTER_DE: [&str; 9] = [
    "trabalho",
    "duracao",
    "curso",
    "treinamento",
    "capacitacao",
    "aula",
    "atividade",
    "estudo",
    "formacao",
];
const RATIO_WORDS: [&str; 7] = [
    "proporcao",
    "razao",
    "escala",
    "relacao",
    "placar",
    "taxa",
    "versao",
];

/// A time of day that starts at `start` (a digit run of one or two digits):
/// `14h`, `14h30`, `14 horas`, `14:30`.
fn time_at(chars: &[char], start: usize) -> Option<Hit<String>> {
    let hour_end = digit_end(chars, start);
    if hour_end - start > 2 {
        return None;
    }
    let hours = text_of(chars, start, hour_end);
    if chars.get(hour_end) == Some(&':') {
        let minutes_end = digit_end(chars, hour_end + 1);
        if minutes_end - (hour_end + 1) != 2 {
            return None;
        }
        // "10:30:45" is a clock reading with seconds, not a time to meet.
        if chars.get(minutes_end) == Some(&':')
            && chars.get(minutes_end + 1).is_some_and(char::is_ascii_digit)
        {
            return None;
        }
        let mut end = minutes_end;
        if chars.get(end).is_some_and(|c| c.is_alphabetic()) {
            let (word, word_end) = word_at(chars, end);
            if !HOUR_WORDS.contains(&word.as_str()) {
                return None;
            }
            end = word_end;
        }
        if RATIO_WORDS.contains(&prev_token(chars, start).as_str()) {
            return None;
        }
        let minutes = text_of(chars, hour_end + 1, minutes_end);
        let value = crate::labels::parse_typed_time(&format!("{hours}:{minutes}"))?;
        return Some(Hit { start, end, value });
    }
    let spaced = matches!(chars.get(hour_end), Some(' ' | '\u{a0}'));
    let word_start = hour_end + usize::from(spaced);
    if !chars.get(word_start).is_some_and(|c| c.is_alphabetic()) {
        return None;
    }
    let (word, word_end) = word_at(chars, word_start);
    if !HOUR_WORDS.contains(&word.as_str()) {
        return None;
    }
    let mut end = word_end;
    let mut minutes = "00".to_owned();
    if word == "h" && !spaced {
        let minutes_end = digit_end(chars, word_end);
        if minutes_end > word_end {
            if minutes_end - word_end != 2 {
                return None;
            }
            minutes = text_of(chars, word_end, minutes_end);
            end = minutes_end;
        }
    }
    // Lengths of time ("por 2 horas", "8 horas diárias") are not times.
    let before = prev_token(chars, start);
    if LENGTH_BEFORE.contains(&before.as_str()) {
        return None;
    }
    let (after, after_end) = next_word(chars, end);
    if LENGTH_AFTER.contains(&after.as_str()) {
        return None;
    }
    if after == "de" && LENGTH_AFTER_DE.contains(&next_word(chars, after_end).0.as_str()) {
        return None;
    }
    if before == "de" {
        // "cerca de 2h", "mais de 3 horas": a length.
        let (earlier, _) = prev_token_at(chars, prev_token_at(chars, start).1);
        if ["cerca", "aproximadamente", "mais", "menos", "pelo", "no"].contains(&earlier.as_str()) {
            return None;
        }
        if (word == "hora" || word == "horas") && !["as", "a", "ate", "e"].contains(&after.as_str())
        {
            return None;
        }
    }
    let value = crate::labels::parse_typed_time(&format!("{hours}:{minutes}"))?;
    Some(Hit { start, end, value })
}

/// "das 14 às 17h": the first hour has no mark of its own.
fn bare_range_start(chars: &[char], start: usize) -> Option<Hit<String>> {
    let end = digit_end(chars, start);
    if end - start > 2 || !["das", "de", "entre"].contains(&prev_token(chars, start).as_str()) {
        return None;
    }
    let (joiner, joiner_end) = next_word(chars, end);
    if !["as", "a", "ate", "e"].contains(&joiner.as_str()) {
        return None;
    }
    let second = skip_spaces(chars, joiner_end);
    if !chars.get(second).is_some_and(char::is_ascii_digit) || !starts_alone(chars, second) {
        return None;
    }
    time_at(chars, second)?;
    let hours = text_of(chars, start, end);
    let value = crate::labels::parse_typed_time(&format!("{hours}:00"))?;
    Some(Hit { start, end, value })
}

fn scan_times(chars: &[char]) -> Vec<Hit<String>> {
    let mut hits = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if chars[at].is_ascii_digit() && (at == 0 || !chars[at - 1].is_ascii_digit()) {
            if starts_alone(chars, at) {
                if let Some(hit) = time_at(chars, at).or_else(|| bare_range_start(chars, at)) {
                    at = hit.end;
                    hits.push(hit);
                    continue;
                }
            }
            at = digit_end(chars, at);
        } else {
            at += 1;
        }
    }
    hits
}

/// The times of day a text writes, as `HH:MM`, in order: "14h", "14h30",
/// "14:30", "14 horas", "às 9h", "das 14h às 17h" (two times), "das 14 às
/// 17h". A bare number is never a time; neither are lengths of time ("por 2
/// horas"), ratios, or digits inside a document number.
pub fn times_in(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    unique(scan_times(&chars).into_iter().map(|hit| hit.value))
}

/// The times a text writes with where each one is, in characters:
/// `(HH:MM, start, end)`. As [`times_in`], with every occurrence.
pub fn time_spans(text: &str) -> Vec<(String, usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    scan_times(&chars)
        .into_iter()
        .map(|hit| (hit.value, hit.start, hit.end))
        .collect()
}

const NUMBER_WORDS: [(&str, u32); 24] = [
    ("um", 1),
    ("uma", 1),
    ("dois", 2),
    ("duas", 2),
    ("tres", 3),
    ("quatro", 4),
    ("cinco", 5),
    ("seis", 6),
    ("sete", 7),
    ("oito", 8),
    ("nove", 9),
    ("dez", 10),
    ("onze", 11),
    ("doze", 12),
    ("treze", 13),
    ("quatorze", 14),
    ("catorze", 14),
    ("quinze", 15),
    ("dezesseis", 16),
    ("dezeseis", 16),
    ("dezessete", 17),
    ("dezesete", 17),
    ("dezoito", 18),
    ("dezenove", 19),
];
const TENS: [(&str, u32); 9] = [
    ("vinte", 20),
    ("trinta", 30),
    ("quarenta", 40),
    ("cinquenta", 50),
    ("sessenta", 60),
    ("setenta", 70),
    ("oitenta", 80),
    ("noventa", 90),
    ("cem", 100),
];

/// Words that follow a number without it counting things: units of time,
/// money and measure.
const QUANTITY_UNITS: [&str; 33] = [
    "dias", "dia", "horas", "hora", "hs", "h", "minutos", "minuto", "min", "segundos", "anos",
    "ano", "meses", "mes", "semanas", "semana", "reais", "real", "centavos", "por", "cento", "km",
    "m", "m2", "kg", "g", "mb", "gb", "kb", "vezes", "vez", "mil", "milhoes",
];
/// Words before a number that make it an identifier, not a quantity.
const QUANTITY_BLOCK_BEFORE: [&str; 52] = [
    "art",
    "artigo",
    "artigos",
    "arts",
    "paragrafo",
    "par",
    "inciso",
    "alinea",
    "item",
    "no",
    "n",
    "numero",
    "num",
    "processo",
    "protocolo",
    "oficio",
    "lei",
    "decreto",
    "portaria",
    "resolucao",
    "instrucao",
    "sala",
    "bloco",
    "andar",
    "turma",
    "pagina",
    "pag",
    "p",
    "fl",
    "fls",
    "folha",
    "folhas",
    "anexo",
    "sei",
    "dia",
    "mes",
    "ano",
    "ramal",
    "telefone",
    "tel",
    "fone",
    "cep",
    "cpf",
    "cnpj",
    "rg",
    "matricula",
    "id",
    "codigo",
    "versao",
    "$",
    "§",
    "as",
];
const FUNCTION_WORDS: [&str; 24] = [
    "de", "do", "da", "dos", "das", "e", "ou", "para", "por", "em", "no", "na", "nos", "nas",
    "ate", "que", "com", "sem", "mais", "menos", "a", "o", "ao", "mil",
];

/// The folded words of a text, with their positions.
fn words_of(chars: &[char]) -> Vec<(usize, usize, String)> {
    let mut words = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if chars[at].is_alphabetic() {
            let end = alpha_end(chars, at);
            words.push((at, end, fold(&text_of(chars, at, end))));
            at = end;
        } else {
            at += 1;
        }
    }
    words
}

/// The value of the number written in words at `words[at]` ("duas",
/// "vinte", "vinte e cinco") and how many words it takes.
fn number_words(words: &[(usize, usize, String)], at: usize) -> Option<(u32, usize)> {
    let word = words[at].2.as_str();
    if let Some((_, tens)) = TENS.iter().find(|(name, _)| *name == word) {
        if *tens < 100 && words.get(at + 1).is_some_and(|w| w.2 == "e") {
            if let Some(units) = words
                .get(at + 2)
                .and_then(|w| NUMBER_WORDS.iter().find(|(name, _)| *name == w.2))
                .map(|(_, v)| *v)
                .filter(|v| (1..=9).contains(v))
            {
                return Some((tens + units, 3));
            }
        }
        return Some((*tens, 1));
    }
    NUMBER_WORDS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, value)| (*value, 1))
}

/// The text inside the parenthesis that follows `at` ("(vinte)"), when one
/// opens there, and where it ends.
fn parenthesized(chars: &[char], at: usize) -> Option<(Vec<char>, usize)> {
    let open = skip_spaces(chars, at);
    if chars.get(open) != Some(&'(') {
        return None;
    }
    let close = (open + 1..chars.len().min(open + 60)).find(|&k| chars[k] == ')')?;
    Some((chars[open + 1..close].to_vec(), close + 1))
}

/// A whole parenthesis that spells one number ("vinte e cinco").
fn spelled_number(inner: &[char]) -> Option<u32> {
    let words = words_of(inner);
    if words.is_empty() || inner.iter().any(char::is_ascii_digit) {
        return None;
    }
    number_words(&words, 0).and_then(|(value, used)| (used == words.len()).then_some(value))
}

/// The quantities a text writes: digits that are not part of a date, time or
/// document number, nor a year, phone, money, percentage, measure or length
/// of time; and number words (two to one hundred) only next to the digits
/// ("20 (vinte)", "vinte (20)") or before a noun ("duas salas"). "um" and
/// "uma" alone are articles and give nothing. A digit and a word that
/// disagree ("20 (trinta)") give nothing.
pub fn quantities_in(text: &str) -> Vec<u32> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut spans: Vec<(usize, usize)> = scan_dates(&chars)
        .into_iter()
        .map(|h| (h.start, h.end))
        .collect();
    spans.extend(scan_times(&chars).into_iter().map(|h| (h.start, h.end)));
    let inside = |at: usize| spans.iter().any(|(s, e)| *s <= at && at < *e);
    let mut found: Vec<(usize, u32)> = Vec::new();
    let mut at = 0;
    while at < n {
        if !(chars[at].is_ascii_digit() && (at == 0 || !chars[at - 1].is_ascii_digit())) {
            at += 1;
            continue;
        }
        let start = at;
        let end = digit_end(&chars, start);
        at = end;
        if end - start > 4 || inside(start) || !starts_alone(&chars, start) {
            continue;
        }
        let value: u32 = text_of(&chars, start, end).parse().unwrap_or(0);
        if value == 0 || (end - start == 4 && (1900..=2100).contains(&value)) {
            continue;
        }
        match chars.get(end) {
            Some(c) if c.is_alphabetic() || *c == '%' => continue,
            Some('/' | '-' | '.' | ',' | ':')
                if chars.get(end + 1).is_some_and(char::is_ascii_digit) =>
            {
                continue
            }
            _ => {}
        }
        // "(65)": an area code.
        if start > 0 && chars[start - 1] == '(' && chars.get(end) == Some(&')') {
            continue;
        }
        if QUANTITY_BLOCK_BEFORE.contains(&prev_token(&chars, start).as_str()) {
            continue;
        }
        let (word, word_end) = next_word(&chars, end);
        if QUANTITY_UNITS.contains(&word.as_str()) {
            continue;
        }
        if matches!(word.as_str(), "de" | "do" | "da")
            && month_of(&next_word(&chars, word_end).0).is_some()
        {
            continue;
        }
        if let Some((spelled, _)) = parenthesized(&chars, end)
            .and_then(|(inner, after)| spelled_number(&inner).map(|v| (v, after)))
        {
            if spelled == value {
                found.push((start, value));
            }
            continue;
        }
        found.push((start, value));
    }
    // Number words.
    let words = words_of(&chars);
    let mut w = 0;
    while w < words.len() {
        let Some((value, used)) = number_words(&words, w) else {
            w += 1;
            continue;
        };
        let (first, last) = (words[w].0, words[w + used - 1].1);
        let article = used == 1 && matches!(words[w].2.as_str(), "um" | "uma");
        w += used;
        if inside(first) {
            continue;
        }
        // Inside parentheses: counted with the digits it spells, or not at all.
        let open = (0..first).rev().find(|&i| !chars[i].is_whitespace());
        let close = (last..n).find(|&i| !chars[i].is_whitespace());
        if open.is_some_and(|i| chars[i] == '(') && close.is_some_and(|i| chars[i] == ')') {
            continue;
        }
        // "vinte (20)": the digits after the word.
        if let Some((inner, _)) = parenthesized(&chars, last) {
            let digits: String = inner.iter().collect::<String>().trim().to_owned();
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                if digits.parse::<u32>().ok() == Some(value) {
                    found.push((first, value));
                }
                continue;
            }
        }
        if article || QUANTITY_BLOCK_BEFORE.contains(&prev_token(&chars, first).as_str()) {
            continue;
        }
        // Before a noun: the next word, with only spaces between.
        let after = skip_spaces(&chars, last);
        if after == last || !chars.get(after).is_some_and(|c| c.is_alphabetic()) {
            continue;
        }
        let (noun, _) = word_at(&chars, after);
        if noun.chars().count() < 3
            || FUNCTION_WORDS.contains(&noun.as_str())
            || QUANTITY_UNITS.contains(&noun.as_str())
            || month_of(&noun).is_some()
        {
            continue;
        }
        found.push((first, value));
    }
    found.sort_by_key(|(position, _)| *position);
    unique(found.into_iter().map(|(_, value)| value))
}

// ---------------------------------------------------------------------------
// Which value a quote says. A value is "conferido" only when the quote says
// exactly that value and nothing that could be taken for it: one date, one
// time or one adjacent range, one quantity. Anything ambiguous is not
// checked (the fact is kept; the person fills the field).

const DEADLINE_CUES: [&str; 7] = [
    "ate",
    "prazo",
    "limite",
    "vence",
    "vencem",
    "vencimento",
    "impreterivelmente",
];
/// Words that, right before a date, say it is where a count starts or the
/// date of a paper: never a deadline.
const START_OF_COUNT: [&[&str]; 17] = [
    &["contar", "de"],
    &["contar", "do"],
    &["contar", "da"],
    &["contados", "de"],
    &["contados", "do"],
    &["contado", "de"],
    &["contado", "do"],
    &["partir", "de"],
    &["partir", "do"],
    &["partir", "da"],
    &["desde"],
    &["desde", "o"],
    &["datado", "de"],
    &["datado", "em"],
    &["datada", "de"],
    &["datada", "em"],
    &["datado", "do"],
];
const DOCUMENT_WORDS: [&str; 16] = [
    "oficio",
    "memorando",
    "despacho",
    "mail",
    "email",
    "documento",
    "circular",
    "comunicado",
    "portaria",
    "ata",
    "carta",
    "nota",
    "mensagem",
    "protocolo",
    "processo",
    "datado",
];

/// The words (folded) of `text`, split at anything that is not a letter.
fn folded_words(text: &str) -> Vec<String> {
    fold(text)
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether the text before a date (at most its last 40 characters count for
/// a cue) introduces it as a deadline.
fn introduced_as_deadline(before: &str) -> bool {
    let collapsed = before.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut words = folded_words(&collapsed);
    if words.last().is_some_and(|w| w == "dia") {
        words.pop();
    }
    if START_OF_COUNT.iter().any(|pattern| {
        words.len() >= pattern.len()
            && words[words.len() - pattern.len()..]
                .iter()
                .zip(pattern.iter())
                .all(|(a, b)| a == b)
    }) {
        return false;
    }
    // "Ofício nº 12/2026 de 01/11/2026": the date of the paper.
    if words
        .last()
        .is_some_and(|w| matches!(w.as_str(), "de" | "em" | "do"))
    {
        let near = &words[words.len().saturating_sub(4)..words.len() - 1];
        if near.iter().any(|w| DOCUMENT_WORDS.contains(&w.as_str()))
            && !near.iter().any(|w| DEADLINE_CUES.contains(&w.as_str()))
        {
            return false;
        }
    }
    let tail: String = collapsed
        .chars()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    folded_words(&tail)
        .iter()
        .any(|w| DEADLINE_CUES.contains(&w.as_str()))
}

/// The one date a quote writes, as ISO; nothing when it writes none or more
/// than one. For a "prazo" the date must also be introduced as a deadline
/// ("até", "prazo de", "limite"…) and not as where a count starts ("a contar
/// de") or as the date of the paper ("Ofício de 01/11/2026").
pub fn date_of_quote(quote: &str, kind: &str) -> Option<String> {
    let chars: Vec<char> = quote.chars().collect();
    let hits = scan_dates(&chars);
    let [hit] = hits.as_slice() else {
        return None;
    };
    if kind == "prazo" && !introduced_as_deadline(&text_of(&chars, 0, hit.start)) {
        return None;
    }
    Some(hit.value.clone())
}

/// The time of day a quote writes and, when written as one adjacent pair
/// ("das 14h às 17h", "de 14h a 17h", "14h às 17h", "14h–17h", "entre 14h e
/// 17h"), the end. One time alone is a start. Two ranges, a stray extra
/// time, or a pair that does not end after it starts give nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeRead {
    pub start: Option<String>,
    pub end: Option<String>,
}

pub fn times_of_quote(quote: &str) -> TimeRead {
    let chars: Vec<char> = quote.chars().collect();
    let hits = scan_times(&chars);
    match hits.as_slice() {
        [one] => TimeRead {
            start: Some(one.value.clone()),
            end: None,
        },
        [first, second] => {
            let between = fold(&text_of(&chars, first.end, second.start));
            let joiner = between
                .trim_matches(|c: char| c.is_whitespace() || c == ',')
                .to_owned();
            let adjacent = matches!(
                joiner.as_str(),
                "as" | "a" | "ate" | "e" | "-" | "–" | "—" | "ate as" | "as ate"
            );
            if adjacent && second.value > first.value {
                TimeRead {
                    start: Some(first.value.clone()),
                    end: Some(second.value.clone()),
                }
            } else {
                TimeRead::default()
            }
        }
        _ => TimeRead::default(),
    }
}

/// The times a fact's quote may check: [`times_of_quote`], and none for a
/// "data_hora" fact whose quote does not write exactly one date.
pub fn fact_times(quote: &str, kind: &str) -> TimeRead {
    if kind == "data_hora" && scan_dates(&quote.chars().collect::<Vec<_>>()).len() != 1 {
        return TimeRead::default();
    }
    times_of_quote(quote)
}

/// The one quantity a quote writes.
pub fn quantity_of_quote(quote: &str) -> Option<u32> {
    match quantities_in(quote).as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// The quantity written right before the name of a resource: the number
/// (digits or words, with an optional "(vinte)"), up to two words between
/// ("20 novas cadeiras", "duas de cadeiras") and then the name. Nothing when
/// the name is not there or no number stands before it.
pub fn quantity_before(quote: &str, what: &str) -> Option<u32> {
    let chars: Vec<char> = quote.chars().collect();
    let folded: Vec<char> = chars
        .iter()
        .flat_map(|c| fold(&c.to_string()).chars().collect::<Vec<_>>())
        .collect();
    if folded.len() != chars.len() {
        return None;
    }
    let wanted: Vec<char> = fold(&what.split_whitespace().collect::<Vec<_>>().join(" "))
        .chars()
        .collect();
    if wanted.is_empty() || wanted.len() > folded.len() {
        return None;
    }
    let position = (0..=folded.len() - wanted.len()).find(|&i| {
        folded[i..i + wanted.len()] == wanted[..] && (i == 0 || !folded[i - 1].is_alphanumeric())
    })?;
    let mut end = position;
    let mut between = 0;
    loop {
        while end > 0 && chars[end - 1].is_whitespace() {
            end -= 1;
        }
        if end == 0 {
            return None;
        }
        let last = chars[end - 1];
        if last == ')' {
            let open = (0..end).rev().find(|&i| chars[i] == '(')?;
            let inner: Vec<char> = chars[open + 1..end - 1].to_vec();
            let digits: String = inner.iter().collect::<String>().trim().to_owned();
            let inner_value = if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                digits.parse::<u32>().ok()
            } else {
                spelled_number(&inner)
            }?;
            // The number before the parenthesis, when there is one.
            let outer = number_ending_at(&chars, open);
            return match outer {
                Some(value) if value != inner_value => None,
                _ => (inner_value > 0).then_some(inner_value),
            };
        }
        if last.is_ascii_digit() || last.is_alphabetic() {
            if let Some(value) = number_ending_at(&chars, end) {
                return (value > 0).then_some(value);
            }
            if last.is_ascii_digit() {
                return None;
            }
            // A word between the number and the name: not a conjunction or a
            // preposition that starts another phrase ("30 pessoas e cadeiras").
            between += 1;
            if between > 2 {
                return None;
            }
            let word_end = end;
            while end > 0 && chars[end - 1].is_alphabetic() {
                end -= 1;
            }
            let word = fold(&text_of(&chars, end, word_end));
            if FUNCTION_WORDS.contains(&word.as_str())
                && !matches!(word.as_str(), "de" | "do" | "da")
            {
                return None;
            }
            continue;
        }
        return None;
    }
}

/// The number (digits, or number words) whose last character is just before
/// `end` (spaces skipped).
fn number_ending_at(chars: &[char], mut end: usize) -> Option<u32> {
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }
    if chars[end - 1].is_ascii_digit() {
        let mut start = end;
        while start > 0 && chars[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if end - start > 4 || !starts_alone(chars, start) {
            return None;
        }
        if QUANTITY_BLOCK_BEFORE.contains(&prev_token(chars, start).as_str()) {
            return None;
        }
        let value: u32 = text_of(chars, start, end).parse().ok()?;
        if (end - start == 4 && (1900..=2100).contains(&value))
            || matches!(
                chars.get(start.wrapping_sub(1)),
                Some('/' | ':' | '.' | ',')
            )
        {
            return None;
        }
        return Some(value);
    }
    if chars[end - 1].is_alphabetic() {
        let words = words_of(&chars[..end]);
        for used in [3_usize, 1] {
            if words.len() < used {
                continue;
            }
            let at = words.len() - used;
            if let Some((value, taken)) = number_words(&words, at) {
                if taken == used && words[at + used - 1].1 == end {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// The values of what the person wrote (purpose, notes: the text of the
/// work context, which arrives as JSON), as plain lines. Text that is not
/// JSON is returned as it is. Quotes are searched here, never in the
/// punctuation of the JSON.
pub fn context_values(human_context: &str) -> String {
    fn walk(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(text) => out.push(text.clone()),
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            serde_json::Value::Object(map) => map.values().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    match serde_json::from_str::<serde_json::Value>(human_context) {
        Ok(value @ (serde_json::Value::Object(_) | serde_json::Value::Array(_))) => {
            let mut lines = Vec::new();
            walk(&value, &mut lines);
            lines.join("\n")
        }
        _ => human_context.to_owned(),
    }
}

/// Where a quote is written: the document text first, then the selected
/// source the fact names, and only then what the person wrote.
pub fn quote_origin(
    quote: &str,
    capture_text: &str,
    human_context: &str,
    sources: &[Source],
    source_id: &str,
) -> crate::model::Origin {
    use crate::model::Origin;
    let quote = normalized(quote);
    if quote.is_empty() {
        return Origin::Nowhere;
    }
    if source_id.is_empty() {
        if normalized(capture_text).contains(&quote) {
            Origin::Document
        } else if normalized(&context_values(human_context)).contains(&quote) {
            Origin::Context
        } else {
            Origin::Nowhere
        }
    } else if sources
        .iter()
        .find(|source| source.id == source_id)
        .is_some_and(|source| normalized(&source.text).contains(&quote))
    {
        Origin::Source
    } else {
        Origin::Nowhere
    }
}

/// Checks the claims of a fact against its quote and sets `fact.checked`
/// (also returned). The quote must be written where `source_id` says
/// ([`quote_origin`]; what the person wrote counts only as origin
/// "contexto") and measure 8 to 600 characters once the white space is
/// collapsed. A claimed value is "conferido" only when the quote says
/// exactly it, by the rules above ([`date_of_quote`], [`times_of_quote`],
/// [`quantity_of_quote`]); otherwise it is cleared (set to `None`) and the
/// fact is kept with that check false. A fact found only in the context
/// never has a checked value. A date or time is first read the way a person
/// types it, so `"9:30"` becomes `"09:30"`. When the quote is not found
/// nothing about the fact is checked and all its claims are cleared. A
/// "data_hora" fact with two dates has no checked time either.
pub fn check_fact(
    fact: &mut crate::model::Fact,
    capture_text: &str,
    human_context: &str,
    sources: &[Source],
) -> crate::model::FactCheck {
    use crate::model::{FactCheck, Origin, FACT_QUOTE_MAX, FACT_QUOTE_MIN};
    let length = normalized(&fact.quote).chars().count();
    let origin = if (FACT_QUOTE_MIN..=FACT_QUOTE_MAX).contains(&length) {
        quote_origin(
            &fact.quote,
            capture_text,
            human_context,
            sources,
            &fact.source_id,
        )
    } else {
        Origin::Nowhere
    };
    let mut checked = FactCheck {
        quote: origin != Origin::Nowhere,
        origin,
        ..FactCheck::default()
    };
    if !origin.is_written() {
        fact.date = None;
        fact.time = None;
        fact.end_time = None;
        fact.quantity = None;
        fact.checked = checked;
        return checked;
    }
    let read_date = date_of_quote(&fact.quote, &fact.kind);
    fact.date = fact
        .date
        .take()
        .and_then(|d| crate::labels::parse_typed_date(&d))
        .filter(|d| Some(d) == read_date.as_ref());
    checked.date = fact.date.is_some();
    let times = fact_times(&fact.quote, &fact.kind);
    let read_time = |value: Option<String>, wanted: &Option<String>| {
        value
            .and_then(|t| crate::labels::parse_typed_time(&t))
            .filter(|t| Some(t) == wanted.as_ref())
    };
    fact.time = read_time(fact.time.take(), &times.start);
    fact.end_time = read_time(fact.end_time.take(), &times.end);
    checked.time = fact.time.is_some();
    checked.end_time = fact.end_time.is_some();
    let quantity = quantity_of_quote(&fact.quote);
    fact.quantity = fact.quantity.take().filter(|q| Some(*q) == quantity);
    checked.quantity = fact.quantity.is_some();
    fact.checked = checked;
    checked
}

/// Checks an analysis that came from storage or from another computer again
/// against the document: every fact is run through [`check_fact`] with the
/// text of the capture, the stored materials and what the person wrote
/// (`human_context_values`, plain text or the work context as stored), and
/// whatever no longer holds is cleared, in the facts and in what the program
/// filled in the proposals (deadlines, starts, ends, quantities, citations of
/// facts). A proposal left with nothing to stand on is removed. Returns how
/// many things were cleared or removed (0: nothing changed).
pub fn recheck_analysis(
    analysis: &mut crate::model::Analysis,
    capture_text: &str,
    human_context_values: &str,
) -> u32 {
    let collapse = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let sources = analysis.materials.clone();
    let mut cleared = 0u32;
    for fact in &mut analysis.facts {
        let before = (
            fact.checked,
            fact.date.clone(),
            fact.time.clone(),
            fact.end_time.clone(),
            fact.quantity,
        );
        let after = check_fact(fact, capture_text, human_context_values, &sources);
        let lost = |was: bool, is: bool| u32::from(was && !is);
        cleared += lost(before.0.quote, after.quote)
            + lost(before.0.date || before.1.is_some(), fact.date.is_some())
            + lost(before.0.time || before.2.is_some(), fact.time.is_some())
            + lost(
                before.0.end_time || before.3.is_some(),
                fact.end_time.is_some(),
            )
            + lost(
                before.0.quantity || before.4.is_some(),
                fact.quantity.is_some(),
            )
            + u32::from(before.0.origin.is_written() && !after.origin.is_written() && after.quote);
    }
    let facts = analysis.facts.clone();
    let fact_of = |id: &str| facts.iter().find(|f| f.id == id);
    let written = |f: &crate::model::Fact| f.checked.quote && f.checked.origin.is_written();
    let due = |due_fact: &mut Option<String>,
               due_date: &mut Option<String>,
               due_source: &mut String,
               cleared: &mut u32| {
        let Some(id) = due_fact.clone() else {
            if due_date.is_some() || !due_source.is_empty() {
                *due_date = None;
                due_source.clear();
                *cleared += 1;
            }
            return;
        };
        let holds = fact_of(&id).is_some_and(|f| {
            matches!(f.kind.as_str(), "prazo" | "data_hora")
                && written(f)
                && f.checked.date
                && f.date == *due_date
                && collapse(&f.quote) == collapse(due_source)
        });
        if !holds {
            *due_fact = None;
            *due_date = None;
            due_source.clear();
            *cleared += 1;
        }
    };
    let keep_cited = |list: &mut Vec<String>, cleared: &mut u32| {
        let before = list.len();
        list.retain(|id| fact_of(id).is_some_and(|f| f.checked.quote));
        *cleared += (before - list.len()) as u32;
    };
    if let Some(proposals) = &mut analysis.proposals {
        let mut tasks = Vec::new();
        for mut task in std::mem::take(&mut proposals.tasks) {
            keep_cited(&mut task.facts, &mut cleared);
            due(
                &mut task.due_fact,
                &mut task.due_date,
                &mut task.due_source,
                &mut cleared,
            );
            if task.facts.is_empty() {
                cleared += 1;
            } else {
                tasks.push(task);
            }
        }
        proposals.tasks = tasks;
        if let Some(a) = &mut proposals.attendance {
            keep_cited(&mut a.facts, &mut cleared);
            due(
                &mut a.due_fact,
                &mut a.due_date,
                &mut a.due_source,
                &mut cleared,
            );
        }
        let mut reservations = Vec::new();
        for mut r in std::mem::take(&mut proposals.reservations) {
            keep_cited(&mut r.facts, &mut cleared);
            let what_ok = fact_of(&r.what_fact).is_some_and(|f| {
                f.kind == "recurso"
                    && written(f)
                    && collapse(&f.quote)
                        .to_lowercase()
                        .contains(&collapse(&r.what).to_lowercase())
            });
            if !what_ok {
                cleared += 1;
                continue;
            }
            if r.quantity > 1 {
                let quote = fact_of(&r.what_fact)
                    .map(|f| f.quote.clone())
                    .unwrap_or_default();
                if quantity_before(&quote, &r.what) != Some(r.quantity) {
                    r.quantity = 1;
                    cleared += 1;
                }
            }
            let when = r.when_fact.as_deref().and_then(fact_of);
            let (start, end) = match when {
                Some(f) if f.kind == "data_hora" && written(f) => {
                    let start = match (&f.date, &f.time) {
                        (Some(d), Some(t)) if f.checked.date && f.checked.time => {
                            Some(format!("{d}T{t}"))
                        }
                        _ => None,
                    };
                    let end = match (&start, &f.date, &f.end_time) {
                        (Some(_), Some(d), Some(e)) if f.checked.end_time => {
                            Some(format!("{d}T{e}"))
                        }
                        _ => None,
                    };
                    (start, end)
                }
                _ => (None, None),
            };
            if r.when_fact.is_some()
                && when.is_none_or(|f| f.kind != "data_hora" || !f.checked.quote)
            {
                r.when_fact = None;
                cleared += 1;
            }
            if r.start.is_some() && r.start != start {
                cleared += 1;
            }
            if r.end.is_some() && r.end != end {
                cleared += 1;
            }
            r.start = start;
            r.end = end;
            reservations.push(r);
        }
        proposals.reservations = reservations;
        if proposals.is_empty() && proposals.nothing_reason.trim().is_empty() {
            proposals.nothing_reason = "As propostas foram retiradas porque o documento não confirma mais os fatos em que se apoiavam.".into();
        }
    }
    // The candidate deadline is the quote of a prazo fact with a checked date.
    if let Some(candidate) = analysis.deadline_candidate.clone() {
        let holds = analysis.facts.iter().any(|f| {
            f.kind == "prazo"
                && written(f)
                && f.checked.date
                && collapse(&f.quote) == collapse(&candidate)
        });
        if !holds {
            analysis.deadline_candidate = None;
            cleared += 1;
        }
    }
    let before = analysis.citations.len();
    analysis.citations.retain(|c| {
        sources
            .iter()
            .find(|s| s.id == c.source_id)
            .is_some_and(|s| normalized(&s.text).contains(&normalized(&c.quote)))
    });
    cleared += (before - analysis.citations.len()) as u32;
    cleared
}

/// The id of the source made of an attachment: `A<attachment>H<8 hex of its
/// hash>`, so that a replaced or removed file is known to have changed.
pub fn attachment_source_id(attachment_id: i64, sha256: &str) -> String {
    format!("A{attachment_id}H{}", &sha256[..sha256.len().min(8)])
}

/// The attachment and hash prefix a source id of [`attachment_source_id`]
/// names; `None` for any other source.
fn parse_attachment_source_id(source: &Source) -> Option<(i64, String)> {
    if source.method != "captura" || source.file_id.is_some() {
        return None;
    }
    let rest = source.id.strip_prefix('A')?;
    let (id, sha) = rest.split_once('H')?;
    let sha_ok = !sha.is_empty() && sha.chars().all(|c| c.is_ascii_hexdigit());
    sha_ok
        .then(|| id.parse::<i64>().ok())
        .flatten()
        .map(|id| (id, sha.to_owned()))
}

/// Whether a source is an attachment of the document (not the text of a
/// capture nor a page of a PDF of the SIGADOC).
pub fn is_attachment_source(source: &Source) -> bool {
    parse_attachment_source_id(source).is_some()
}

impl Store {
    pub fn prepare_sources(&self, documents: &[i64]) -> Result<Prepared> {
        ensure!(
            !documents.is_empty() && documents.len() <= 8,
            "Selecione entre um e oito documentos."
        );
        let mut prepared = Prepared::default();
        let mut total = 0usize;
        let mut ids = BTreeSet::new();
        for &id in documents {
            ensure!(ids.insert(id), "Documento repetido na seleção.");
            let detail = self.load_document(id)?;
            let capture = detail.summary.latest_capture_id;
            let text = &detail.capture.text;
            if !text.trim().is_empty() {
                total += text.len();
                prepared.sources.push(Source {
                    id: format!("C{capture}"),
                    document_id: id,
                    capture_id: capture,
                    label: format!(
                        "{} — texto da captura (página não determinada)",
                        detail.capture.document_number
                    ),
                    file_id: None,
                    page: None,
                    method: "captura".into(),
                    text: text.clone(),
                });
            }
            for file in self.list_files(capture)? {
                let bytes = self.pdf_bytes(file.id, capture)?;
                let pages = std::panic::catch_unwind(|| {
                    pdf_extract::extract_text_from_mem_by_pages(&bytes)
                })
                .ok()
                .and_then(Result::ok);
                match pages {
                    Some(pages) => {
                        for (index, text) in pages.into_iter().enumerate() {
                            if index >= 200
                                || prepared.sources.len() >= 512
                                || total + text.len() > 2 * 1024 * 1024
                            {
                                prepared.pending.push(format!("{}: algumas páginas não foram preparadas (limite de preparação).",file.label));
                                break;
                            }
                            if !text.trim().is_empty() {
                                total += text.len();
                                prepared.sources.push(Source {
                                    id: format!("C{capture}F{}P{}", file.id, index + 1),
                                    document_id: id,
                                    capture_id: capture,
                                    label: format!(
                                        "{} — {} — página {}",
                                        detail.capture.document_number,
                                        file.label,
                                        index + 1
                                    ),
                                    file_id: Some(file.id),
                                    page: Some(index as u32 + 1),
                                    method: "pdf".into(),
                                    text,
                                });
                            }
                        }
                    }
                    None => prepared.pending.push(format!(
                        "{}: não foi possível extrair páginas de texto; confira o leitor e o OCR.",
                        file.label
                    )),
                }
                // Saved OCR is opt-in in the source selector, separate from native PDF text.
                let page_texts = self.page_texts_for_file(file.id)?;
                for (page, value) in page_texts {
                    {
                        if !value.text.trim().is_empty() {
                            if total + value.text.len() > 2 * 1024 * 1024
                                || prepared.sources.len() >= 512
                            {
                                prepared.pending.push(format!(
                                    "{}: o OCR de mais páginas não foi preparado (limite de preparação).",
                                    file.label
                                ));
                                break;
                            }
                            total += value.text.len();
                            prepared.sources.push(Source {
                                id: format!("C{capture}F{}P{}OCR", file.id, page + 1),
                                document_id: id,
                                capture_id: capture,
                                label: format!(
                                    "{} — {} — página {} — OCR {} (conferir)",
                                    detail.capture.document_number,
                                    file.label,
                                    page + 1,
                                    value.language
                                ),
                                file_id: Some(file.id),
                                page: Some(page + 1),
                                method: "ocr".into(),
                                text: value.text,
                            });
                        }
                    }
                }
            }
            // The files the person attached to the document, whose text is read
            // here, in memory, and only when asked: nothing is stored.
            for (number, meta) in self
                .list_attachments(crate::attachments::Owner::Document(id))?
                .into_iter()
                .enumerate()
            {
                let (stored, bytes) = self.attachment_bytes(meta.id)?;
                let extracted = crate::extraction::extract(&stored.name, &bytes);
                if !extracted.status.usable() || extracted.text.trim().is_empty() {
                    prepared.pending.push(format!(
                        "Anexo {}: {}: {}.",
                        number + 1,
                        stored.name,
                        extracted.status.label()
                    ));
                    continue;
                }
                if prepared.sources.len() >= 512
                    || extracted.text.len() > 400 * 1024
                    || total + extracted.text.len() > 2 * 1024 * 1024
                {
                    prepared.pending.push(format!(
                        "Anexo {}: {}: não foi preparado (limite de tamanho).",
                        number + 1,
                        stored.name
                    ));
                    continue;
                }
                total += extracted.text.len();
                prepared.sources.push(Source {
                    id: attachment_source_id(meta.id, &stored.sha256),
                    document_id: id,
                    capture_id: capture,
                    label: format!(
                        "{} — Anexo {}: {}",
                        detail.capture.document_number,
                        number + 1,
                        stored.name
                    ),
                    file_id: None,
                    page: None,
                    // An older Ponte accepts only "captura", "pdf" and "ocr" in
                    // what is synced: an attachment is kept as a "captura"
                    // source whose id says what it is.
                    method: "captura".into(),
                    text: extracted.text,
                });
            }
            for pending in detail.capture.coverage.pending {
                prepared
                    .pending
                    .push(format!("{}: {pending}", detail.capture.document_number));
            }
        }
        ensure!(
            total <= 6 * 1024 * 1024,
            "A preparação passou do limite; escolha menos documentos."
        );
        Ok(prepared)
    }
    pub fn sources_stale(&self, sources: &[Source]) -> bool {
        sources.iter().any(|source| {
            let current: rusqlite::Result<i64> = self.conn.query_row(
                "SELECT d.latest_capture_id FROM documents d WHERE d.id=?1 AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='document' AND t.item_id=d.id)",
                [source.document_id],
                |r| r.get(0),
            );
            if current.ok() != Some(source.capture_id) {
                return true;
            }
            if let Some((attachment, sha)) = parse_attachment_source_id(source) {
                // An attachment source is current while the file is still an
                // active attachment of the document with the same content.
                let active = self
                    .list_attachments(crate::attachments::Owner::Document(source.document_id))
                    .unwrap_or_default();
                return !active
                    .iter()
                    .any(|a| a.id == attachment && a.sha256.starts_with(&sha));
            }
            if source.method == "ocr" {
                return match (source.file_id, source.page) {
                    (Some(file), Some(page)) => self
                        .load_page_text(file, page - 1)
                        .ok()
                        .flatten()
                        .is_none_or(|text| text.text != source.text),
                    _ => true,
                };
            }
            false
        })
    }
}
#[cfg(feature = "desktop")]
#[derive(Default)]
struct Draft {
    documents: BTreeSet<i64>,
    prepared: Option<Prepared>,
    selected: BTreeSet<String>,
    message: String,
}
#[cfg(feature = "desktop")]
#[derive(Default)]
pub struct SourcesPanel {
    drafts: BTreeMap<i64, Draft>,
}
#[cfg(feature = "desktop")]
impl SourcesPanel {
    pub fn forget_document(&mut self, id: i64) {
        self.drafts.remove(&id);
        for d in self.drafts.values_mut() {
            d.documents.remove(&id);
        }
    }
    pub fn accept(&mut self, id: i64, result: std::result::Result<Prepared, String>) {
        let draft = self.drafts.entry(id).or_default();
        match result {
            Ok(value) => {
                draft.prepared = Some(value);
                draft.selected.clear();
                draft.message = "Marque os textos ou páginas que deseja enviar à IA.".into();
            }
            Err(e) => draft.message = e,
        }
    }
    pub fn selected(&self, id: i64, store: &Store) -> Result<Vec<Source>> {
        let Some(draft) = self.drafts.get(&id) else {
            return Ok(vec![]);
        };
        let sources = draft
            .prepared
            .as_ref()
            .map(|p| {
                p.sources
                    .iter()
                    .filter(|s| draft.selected.contains(&s.id))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        validate(&sources, &[])?;
        ensure!(
            !store.sources_stale(&sources),
            "As fontes selecionadas estão desatualizadas; prepare as páginas de novo."
        );
        Ok(sources)
    }
    pub fn show(
        &mut self,
        ui: &mut eframe::egui::Ui,
        store: &Store,
        id: i64,
        busy: bool,
    ) -> Option<Vec<i64>> {
        let draft = self.drafts.entry(id).or_default();
        draft.documents.insert(id);
        let mut request = None;
        eframe::egui::CollapsingHeader::new(eframe::egui::RichText::new("Outros documentos e páginas para a IA").font(crate::theme::font_strong(crate::theme::SIZE_SECTION))).show(ui,|ui|{
            crate::widgets::small_muted(ui,"Opcional: marque documentos já importados e, depois de preparar, as páginas que a IA deve considerar.").on_hover_text("A seleção vale para a próxima análise ou minuta; análises anteriores guardam as fontes que usaram. O documento atual e o contexto são sempre incluídos. Texto obtido por OCR exige conferência.");
            match store.list_documents(""){Ok(documents)=>{for document in documents{if document.id!=id{let mut checked=draft.documents.contains(&document.id);if ui.add_enabled(!busy,eframe::egui::Checkbox::new(&mut checked,format!("{} — {}",document.document_number,document.title))).changed(){if checked{draft.documents.insert(document.id);}else{draft.documents.remove(&document.id);}draft.prepared=None;draft.selected.clear();}}}},Err(e)=>{ui.label(format!("Não foi possível ler os documentos: {e}"));}}
            if ui.add_enabled(!busy,eframe::egui::Button::new(if busy {"Preparando…"} else {"Preparar páginas"})).on_hover_text("Lê os PDFs dos documentos marcados neste computador para você escolher as páginas.").clicked(){request=Some(draft.documents.iter().copied().collect());}
            if !draft.message.is_empty(){ui.label(&draft.message);}
            if let Some(prepared)=&draft.prepared {
                for pending in &prepared.pending{ui.small(format!("Cobertura: {pending}"));}
                eframe::egui::ScrollArea::vertical().id_salt(("sources",id)).max_height(260.0).show(ui,|ui|{for source in &prepared.sources{
                    let mut selected=draft.selected.contains(&source.id);if ui.add_enabled(!busy,eframe::egui::Checkbox::new(&mut selected,format!("{} • {} bytes",source.label,source.text.len()))).changed(){if selected{draft.selected.insert(source.id.clone());}else{draft.selected.remove(&source.id);}}
                    ui.push_id(&source.id,|ui|{ui.collapsing("Conferir texto antes de selecionar",|ui|{let mut text=source.text.as_str();ui.add(eframe::egui::TextEdit::multiline(&mut text).desired_rows(5).desired_width(f32::INFINITY));});});
                }});
                let size:usize=prepared.sources.iter().filter(|s|draft.selected.contains(&s.id)).map(|s|s.text.len()).sum();ui.label(format!("{} • {} KB adicionais",if draft.selected.len()==1{"1 fonte selecionada".to_owned()}else{format!("{} fontes selecionadas",draft.selected.len())},size.div_ceil(1024)));
            }
        });
        request
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn citations_must_match_selected_source_and_literal_text() {
        let source = Source {
            id: "C1F2P3".into(),
            document_id: 1,
            capture_id: 1,
            label: "Arquivo — página 3".into(),
            file_id: Some(2),
            page: Some(3),
            method: "pdf".into(),
            text: "Solicita-se resposta até 30/11/2026.\nApenas informação fictícia.".into(),
        };
        let mut citation = Citation {
            source_id: source.id.clone(),
            quote: "Solicita-se resposta até 30/11/2026.".into(),
            claim: "Prazo transcrito".into(),
        };
        validate(std::slice::from_ref(&source), &[citation.clone()]).unwrap();
        citation.quote = "O prazo é 15/11/2026 e é obrigatório".into();
        assert!(validate(std::slice::from_ref(&source), &[citation.clone()]).is_err());
        citation.source_id = "C99".into();
        assert!(validate(&[source], &[citation]).is_err());
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    #[test]
    fn source_snapshots_make_analysis_stale_when_antecedent_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("evidence.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let first = crate::model::sample_capture();
        let a = store.import_capture(&first).unwrap();
        let mut other = first.clone();
        other.request_id = uuid::Uuid::new_v4().to_string();
        other.document_number = "TESTE-OFI-2026/00099".into();
        other.source_url="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00099".into();
        other.text = "Recebido apenas para ciência e download, sem solicitação de resposta.".into();
        let b = store.import_capture(&other).unwrap();
        let sources = store.prepare_sources(&[b.document_id]).unwrap().sources;
        let citation = Citation {
            source_id: sources[0].id.clone(),
            quote: other.text.clone(),
            claim: "Finalidade explícita no antecedente".into(),
        };
        let analysis = crate::model::Analysis {
            capture_id: a.capture_id,
            subjects: vec!["Ciência".into()],
            priority: "normal".into(),
            reason: "Apenas ciência".into(),
            summary: "Documento para ciência".into(),
            model: "test".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            materials: sources,
            citations: vec![citation],
            ..Default::default()
        };
        store.save_analysis(a.document_id, &analysis).unwrap();
        store
            .save_draft_with_sources(
                a.document_id,
                a.capture_id,
                "resposta",
                "Minuta fictícia",
                &analysis.materials,
            )
            .unwrap();
        assert!(!store.load_document(a.document_id).unwrap().drafts[0].stale);
        assert!(
            !store
                .load_document(a.document_id)
                .unwrap()
                .summary
                .analysis_stale
        );
        other.request_id = uuid::Uuid::new_v4().to_string();
        other.text.push_str(" Informação nova.");
        store.import_capture(&other).unwrap();
        assert!(
            store
                .load_document(a.document_id)
                .unwrap()
                .summary
                .analysis_stale
        );
        assert!(store.load_document(a.document_id).unwrap().drafts[0].stale);
        let mut forged = analysis;
        forged.citations[0].quote = "Trecho inventado que não está na fonte".into();
        assert!(store.save_analysis(a.document_id, &forged).is_err());
    }
}

#[cfg(test)]
mod field_tests {
    use super::*;
    use crate::model::Fact;

    fn source(id: &str, text: &str) -> Source {
        Source {
            id: id.into(),
            document_id: 1,
            capture_id: 1,
            label: "Fonte".into(),
            file_id: None,
            page: None,
            method: "captura".into(),
            text: text.into(),
        }
    }

    #[test]
    fn find_quote_is_literal_collapses_white_space_and_respects_the_source() {
        let text = "O setor solicita\n  a reserva da sala 3\tpara a reunião.";
        assert!(find_quote(
            "solicita a reserva da sala 3 para a reunião",
            text,
            "",
            &[],
            ""
        ));
        // Case sensitive, no paraphrase, never empty.
        assert!(!find_quote("SOLICITA A RESERVA", text, "", &[], ""));
        assert!(!find_quote("reserva a sala", text, "", &[], ""));
        assert!(!find_quote("   ", text, "", &[], ""));
        // The human context counts for "".
        assert!(find_quote(
            "Pediram a sala por telefone",
            text,
            "Nota: Pediram a sala por telefone.",
            &[],
            ""
        ));
        // A source id searches that source only.
        let sources = [
            source("C1F2P1", "Anexo: prazo até 30/11/2026."),
            source("C1F2P2", "Outra página"),
        ];
        assert!(find_quote(
            "prazo até 30/11/2026",
            text,
            "",
            &sources,
            "C1F2P1"
        ));
        assert!(!find_quote(
            "prazo até 30/11/2026",
            text,
            "",
            &sources,
            "C1F2P2"
        ));
        assert!(!find_quote(
            "prazo até 30/11/2026",
            text,
            "",
            &sources,
            "C9"
        ));
        assert!(!find_quote(
            "solicita a reserva",
            text,
            "",
            &sources,
            "C1F2P1"
        ));
    }

    #[test]
    fn numeric_and_written_dates_are_read_in_every_brazilian_shape() {
        for (text, expected) in [
            ("Prazo: 30/11/2026.", "2026-11-30"),
            ("até 30-11-2026", "2026-11-30"),
            ("em 30.11.2026", "2026-11-30"),
            ("dia 5/3/2026", "2026-03-05"),
            ("dia 05/03/2026", "2026-03-05"),
            ("em 30/11/26", "2026-11-30"),
            ("em 30/11/99", "1999-11-30"),
            ("data 2026-11-30", "2026-11-30"),
            ("12 de novembro de 2026", "2026-11-12"),
            ("12 de Novembro de 2026", "2026-11-12"),
            ("12 DE NOVEMBRO DE 2026", "2026-11-12"),
            ("12 nov. 2026", "2026-11-12"),
            ("12 nov 2026", "2026-11-12"),
            ("12 de nov. de 2026", "2026-11-12"),
            ("12/nov/2026", "2026-11-12"),
            ("1º de março de 2026", "2026-03-01"),
            ("1° de março de 2026", "2026-03-01"),
            ("1o de marco de 2026", "2026-03-01"),
            ("1 de MARÇO de 2026", "2026-03-01"),
            ("15 de setembro de 2026", "2026-09-15"),
            ("3 de fevereiro de 2027", "2027-02-03"),
            ("29 de fevereiro de 2028", "2028-02-29"),
            ("12 dez 2026", "2026-12-12"),
            ("12 de dez. de 2026", "2026-12-12"),
            ("7 de jan. de 2026", "2026-01-07"),
            ("9 abr 2026", "2026-04-09"),
            ("9 de mai de 2026", "2026-05-09"),
            ("9 de jun de 2026", "2026-06-09"),
            ("9 de jul de 2026", "2026-07-09"),
            ("9 de ago de 2026", "2026-08-09"),
            ("9 de set de 2026", "2026-09-09"),
            ("9 de out de 2026", "2026-10-09"),
            ("12\nde novembro\nde 2026", "2026-11-12"),
        ] {
            assert_eq!(dates_in(text), [expected], "{text}");
        }
    }

    #[test]
    fn several_dates_come_in_order_without_repeats() {
        assert_eq!(
            dates_in("De 10/11/2026 a 12 de novembro de 2026, e de novo em 10/11/2026."),
            ["2026-11-10", "2026-11-12"]
        );
        assert_eq!(
            dates_in("Reunião em 02/12/2026 (prazo 30/11/2026)."),
            ["2026-12-02", "2026-11-30"]
        );
    }

    #[test]
    fn invalid_incomplete_and_relative_dates_give_nothing() {
        for text in [
            "31/02/2026",
            "29/02/2027",
            "31/04/2026",
            "32/01/2026",
            "00/01/2026",
            "12/13/2026",
            "2026-13-01",
            "2026-02-30",
            "01/01/1899",
            "30 de fevereiro de 2026",
            "12 de novembro",
            "12 de novembro de 26",
            "novembro de 2026",
            "dia 15",
            "no dia 15 próximo",
            "hoje",
            "amanhã",
            "até o fim da próxima semana",
            "daqui a 10 dias",
            "12 de foo de 2026",
            "2026/11/30",
            "",
        ] {
            assert!(dates_in(text).is_empty(), "{text}: {:?}", dates_in(text));
        }
    }

    #[test]
    fn digits_inside_document_numbers_and_codes_are_not_dates() {
        for text in [
            "ORG-OFI-2026/00431",
            "Ofício ORG-OFI-2026/00431",
            "SEI-12/03/2026",
            "CPF 123.456.789-09",
            "CNPJ 12.345.678/0001-90",
            "Processo 00123/2026-12",
            "Telefone (65) 3613-2026",
            "Item 2.1.10 do edital",
            "versão v1.2.2026",
            "protocolo 12/03/2026-45",
            "código 12/03/2026.7",
            "ORG-2026-11-12",
        ] {
            assert!(dates_in(text).is_empty(), "{text}: {:?}", dates_in(text));
        }
        // A real date next to a number is still read.
        assert_eq!(
            dates_in(
                "Ofício ORG-OFI-2026/00431, de 12/11/2026, recebido em 13 de novembro de 2026."
            ),
            ["2026-11-12", "2026-11-13"]
        );
        assert_eq!(dates_in("(até 12/11/2026)"), ["2026-11-12"]);
        assert_eq!(dates_in("prazo:12/11/2026"), ["2026-11-12"]);
    }

    #[test]
    fn times_are_read_in_the_ways_people_write_them() {
        for (text, expected) in [
            ("às 14h", vec!["14:00"]),
            ("14h30", vec!["14:30"]),
            ("às 9h", vec!["09:00"]),
            ("às 9h30", vec!["09:30"]),
            ("9:30", vec!["09:30"]),
            ("09:30", vec!["09:30"]),
            ("14:30", vec!["14:30"]),
            ("14 horas", vec!["14:00"]),
            ("às 14 horas", vec!["14:00"]),
            ("14hs", vec!["14:00"]),
            ("14 hs", vec!["14:00"]),
            ("14hrs", vec!["14:00"]),
            ("14 h", vec!["14:00"]),
            ("1 hora", vec!["01:00"]),
            ("das 14h às 17h", vec!["14:00", "17:00"]),
            ("das 14h30 às 17h30", vec!["14:30", "17:30"]),
            ("das 14:00 às 17:00", vec!["14:00", "17:00"]),
            ("das 14 às 17 horas", vec!["14:00", "17:00"]),
            ("das 14 às 17h", vec!["14:00", "17:00"]),
            ("entre 9 e 11h", vec!["09:00", "11:00"]),
            ("de 8 a 10h", vec!["08:00", "10:00"]),
            ("às 14h do dia 30/11/2026", vec!["14:00"]),
            ("em 30/11/2026 às 8h15.", vec!["08:15"]),
            ("00:00", vec!["00:00"]),
            ("23h59", vec!["23:59"]),
            ("14h e 14h", vec!["14:00"]),
        ] {
            assert_eq!(times_in(text), expected, "{text}");
        }
    }

    #[test]
    fn numbers_that_are_not_times_give_nothing() {
        for text in [
            "art. 9",
            "artigo 14",
            "item 14",
            "sala 9",
            "9",
            "14",
            "0930",
            "25h",
            "24h",
            "24:00",
            "9:75",
            "14h75",
            "14h3",
            "10:30:45",
            "proporção 1:30",
            "versão 2:15",
            "por 2 horas",
            "durante 3 horas",
            "cerca de 2h",
            "8 horas diárias",
            "40 horas semanais",
            "48 horas",
            "reunião de 2 horas de duração",
            "2h30min",
            "14hoje",
            "x14h",
            "ORG-OFI-2026/00431",
            "CPF 123.456.789-09",
            "R$ 1.500,00",
            "das 14 e 17",
            "",
        ] {
            assert!(times_in(text).is_empty(), "{text}: {:?}", times_in(text));
        }
        // Minutes spelled out are not read: only the hour, never a guess.
        assert_eq!(times_in("14 horas e 30 minutos"), ["14:00"]);
    }

    #[test]
    fn quantities_come_from_digits_and_from_number_words_in_context() {
        for (text, expected) in [
            ("Solicito 15 cadeiras.", vec![15]),
            ("20 (vinte) cadeiras", vec![20]),
            ("vinte (20) cadeiras", vec![20]),
            ("vinte e cinco (25) pessoas", vec![25]),
            ("duas salas", vec![2]),
            ("Precisamos de duas salas e três projetores", vec![2, 3]),
            ("vinte e cinco pessoas", vec![25]),
            ("cem convites", vec![100]),
            ("DUAS SALAS", vec![2]),
            ("Solicito 15 cadeiras e 3 mesas", vec![15, 3]),
            ("um (1) projetor", vec![1]),
            ("1 (um) projetor", vec![1]),
            ("trinta cadeiras", vec![30]),
            ("dezoito vagas", vec![18]),
            ("quatorze lugares", vec![14]),
            ("5 notebooks no dia 12/11/2026 às 9h", vec![5]),
        ] {
            assert_eq!(quantities_in(text), expected, "{text}");
        }
    }

    #[test]
    fn identifiers_years_phones_money_dates_and_lengths_are_not_quantities() {
        for text in [
            "sala 3",
            "Sala 12 do bloco 2",
            "art. 9",
            "§ 3º",
            "item 4",
            "página 7",
            "ORG-OFI-2026/00431",
            "Ofício nº 431",
            "em 2026",
            "R$ 1.500,00",
            "R$ 20",
            "(65) 99999-0000",
            "tel. 3613-2026",
            "CPF 123.456.789-09",
            "CEP 78005-100",
            "20%",
            "10 dias",
            "5 anos",
            "30 minutos",
            "2 horas",
            "10 km",
            "das 14h às 17h",
            "às 9h30",
            "30/11/2026",
            "12 de novembro",
            "10 de novembro de 2026",
            "3ª reunião",
            "dois dias",
            "dez horas",
            "dois mil reais",
            "dia quinze",
            "uma sala",
            "um projetor",
            "20 (trinta) cadeiras",
            "vinte (30) cadeiras",
            "duas de manhã",
            "dez de novembro",
            "(vinte)",
            "0 cadeiras",
            "12345 cadeiras",
            "",
        ] {
            assert!(
                quantities_in(text).is_empty(),
                "{text}: {:?}",
                quantities_in(text)
            );
        }
    }

    fn fact(quote: &str) -> Fact {
        Fact {
            id: "f1".into(),
            kind: "data_hora".into(),
            statement: "Reunião".into(),
            quote: quote.into(),
            ..Fact::default()
        }
    }

    const TEXT: &str = "Solicitamos a reserva de 20 (vinte) cadeiras para o dia 12 de novembro de 2026, das 14h às 17h, na sala 3. O prazo para resposta é até a próxima sexta.";

    #[test]
    fn check_fact_confirms_only_the_values_the_quote_itself_writes() {
        let quote = "20 (vinte) cadeiras para o dia 12 de novembro de 2026, das 14h às 17h";
        let mut f = fact(quote);
        f.date = Some("2026-11-12".into());
        f.time = Some("14:00".into());
        f.end_time = Some("17:00".into());
        f.quantity = Some(20);
        let checked = check_fact(&mut f, TEXT, "", &[]);
        assert!(
            checked.quote && checked.date && checked.time && checked.end_time && checked.quantity
        );
        assert_eq!(f.checked, checked);
        assert_eq!(f.date.as_deref(), Some("2026-11-12"));
        assert_eq!(f.quantity, Some(20));

        // Values the quote does not write are cleared; the fact stays.
        let mut f = fact(quote);
        f.date = Some("2026-11-13".into());
        f.time = Some("15:00".into());
        f.end_time = Some("18:00".into());
        f.quantity = Some(25);
        let checked = check_fact(&mut f, TEXT, "", &[]);
        assert!(checked.quote);
        assert!(!checked.date && !checked.time && !checked.end_time && !checked.quantity);
        assert!(
            f.date.is_none() && f.time.is_none() && f.end_time.is_none() && f.quantity.is_none()
        );

        // Claims are read the way people type them.
        let mut f = fact(quote);
        f.date = Some("12/11/2026".into());
        f.time = Some("14h".into());
        let checked = check_fact(&mut f, TEXT, "", &[]);
        assert!(checked.date && checked.time);
        assert_eq!(f.date.as_deref(), Some("2026-11-12"));
        assert_eq!(f.time.as_deref(), Some("14:00"));

        // No claim, nothing to check besides the quote.
        let mut f = fact(quote);
        let checked = check_fact(&mut f, TEXT, "", &[]);
        assert!(checked.quote && !checked.date && !checked.time);

        // A date in the document but outside the quote is not confirmed.
        let mut f = fact("O prazo para resposta é até a próxima sexta.");
        f.date = Some("2026-11-12".into());
        let checked = check_fact(&mut f, TEXT, "", &[]);
        assert!(checked.quote && !checked.date && f.date.is_none());
    }

    #[test]
    fn check_fact_refuses_quotes_that_are_not_there_or_have_the_wrong_size() {
        let mut f = fact("uma frase que o documento não contém de jeito nenhum");
        f.date = Some("2026-11-12".into());
        f.quantity = Some(20);
        let checked = check_fact(&mut f, TEXT, "", &[]);
        assert_eq!(checked, crate::model::FactCheck::default());
        assert!(f.date.is_none() && f.quantity.is_none());
        // Too short (8 characters is the least), even though it is there.
        let mut f = fact("sala 3");
        assert!(!check_fact(&mut f, TEXT, "", &[]).quote);
        let mut f = fact("na sala 3");
        assert!(check_fact(&mut f, TEXT, "", &[]).quote);
        // Too long: the program does not accept 600+ characters as one excerpt.
        let long = "palavra ".repeat(100);
        let mut f = fact(&long);
        assert!(!check_fact(&mut f, &long, "", &[]).quote);
        // The source is the one the fact names.
        let sources = [source("C1F2P1", "Anexo: reunião em 30/11/2026, às 9h30.")];
        let mut f = fact("reunião em 30/11/2026, às 9h30");
        f.source_id = "C1F2P1".into();
        f.date = Some("2026-11-30".into());
        f.time = Some("9:30".into());
        let checked = check_fact(&mut f, TEXT, "", &sources);
        assert!(checked.quote && checked.date && checked.time);
        assert_eq!(f.time.as_deref(), Some("09:30"));
        let mut f = fact("reunião em 30/11/2026, às 9h30");
        assert!(!check_fact(&mut f, TEXT, "", &sources).quote);
        // The human context counts for the document.
        let mut f = fact("Nota do servidor: pediram por telefone");
        assert!(check_fact(&mut f, TEXT, "Nota do servidor: pediram por telefone.", &[]).quote);
    }

    #[test]
    fn a_pair_that_does_not_end_after_it_starts_checks_neither_time() {
        let mut f = fact("das 17h às 14h na sala principal");
        f.time = Some("17:00".into());
        f.end_time = Some("14:00".into());
        let checked = check_fact(&mut f, "das 17h às 14h na sala principal", "", &[]);
        assert!(!checked.time && !checked.end_time);
        assert!(f.time.is_none() && f.end_time.is_none());
    }

    // -- which value a quote says ------------------------------------------

    #[test]
    fn a_date_counts_only_when_it_is_the_one_date_and_a_prazo_is_introduced_as_one() {
        let deadline = |q: &str| date_of_quote(q, "prazo");
        let any = |q: &str| date_of_quote(q, "data_hora");
        let some = |d: &str| Some(d.to_owned());
        // Introduced as a deadline.
        for quote in [
            "responder até 20/11/2026",
            "entregar o relatório até o dia 20/11/2026",
            "prazo final: 20/11/2026",
            "prazo para resposta: 20 de novembro de 2026",
            "prazo de 20/11/2026",
            "limite 20/11/2026",
            "o pagamento vence em 20/11/2026",
            "vencimento 20/11/2026",
            "impreterivelmente em 20/11/2026",
            "no máximo até 20/11/2026",
            "CONFIRMAR ATÉ 20/11/2026",
        ] {
            assert_eq!(deadline(quote), some("2026-11-20"), "{quote}");
        }
        // Where a count starts, the date of the paper, no cue: not a deadline.
        for quote in [
            "responder no prazo de 10 dias a contar de 03/11/2026",
            "prazo de 10 dias contados de 03/11/2026",
            "prazo de 10 dias contados a partir de 03/11/2026",
            "a partir de 20/11/2026",
            "desde 20/11/2026",
            "datado de 20/11/2026",
            "Ofício nº 12/2026 de 20/11/2026",
            "Memorando de 20/11/2026",
            "o e-mail de 20/11/2026",
            "reunião em 20/11/2026",
            "Prazo encerrado há semanas; vamos nos reunir novamente em 20/11/2026",
        ] {
            assert_eq!(deadline(quote), None, "{quote}");
        }
        // The same words without the deadline rule for other kinds.
        assert_eq!(any("reunião em 20/11/2026"), some("2026-11-20"));
        assert_eq!(
            date_of_quote("Ofício de 01/11/2026", "outro"),
            some("2026-11-01")
        );
        // Two dates in one quote: none is checked, whatever the kind.
        for quote in [
            "Ofício de 01/11/2026 … responder até 20/11/2026",
            "Reunião em 16/11/2026 às 14h; confirmar até 12/11/2026 às 18h",
            "de 10/11/2026 a 12/11/2026",
        ] {
            assert_eq!(deadline(quote), None, "{quote}");
            assert_eq!(any(quote), None, "{quote}");
        }
        // No date at all, or a relative one.
        assert_eq!(deadline("responder até o fim da próxima semana"), None);
        assert_eq!(any("ORG-OFI-2026/00431"), None);
    }

    #[test]
    fn a_time_counts_alone_or_as_one_adjacent_pair() {
        let read = |q: &str| times_of_quote(q);
        let pair = |a: &str, b: &str| TimeRead {
            start: Some(a.to_owned()),
            end: Some(b.to_owned()),
        };
        let one = |a: &str| TimeRead {
            start: Some(a.to_owned()),
            end: None,
        };
        for quote in [
            "das 14h às 17h",
            "de 14h a 17h",
            "14h às 17h",
            "14h–17h",
            "14h - 17h",
            "entre 14h e 17h",
            "das 14 às 17h",
            "sala 3, das 14:00 às 17:00",
        ] {
            assert_eq!(read(quote), pair("14:00", "17:00"), "{quote}");
        }
        assert_eq!(read("às 14h"), one("14:00"));
        assert_eq!(read("no dia 16/11/2026 às 9h30"), one("09:30"));
        // Two ranges, stray extra times, a pair that goes backwards: neither.
        for quote in [
            "das 8h às 12h e das 14h às 18h",
            "Reunião em 16/11/2026 às 14h; confirmar até 12/11/2026 às 18h",
            "às 14h e às 16h",
            "das 17h às 14h",
            "às 9h, 10h e 11h",
            "reunião às 14h, café às 16h, fim às 17h",
        ] {
            assert_eq!(read(quote), TimeRead::default(), "{quote}");
        }
        // A data_hora fact with two dates has no checked time at all.
        assert_eq!(
            fact_times("dia 16/11/2026 e dia 17/11/2026, às 14h", "data_hora"),
            TimeRead::default()
        );
        assert_eq!(
            fact_times("dia 16/11/2026, às 14h", "data_hora"),
            one("14:00")
        );
    }

    #[test]
    fn a_quantity_is_the_only_one_or_the_number_written_before_the_resource() {
        assert_eq!(quantity_of_quote("20 (vinte) cadeiras"), Some(20));
        assert_eq!(quantity_of_quote("20 (vinte) cadeiras e 2 veículos"), None);
        assert_eq!(quantity_of_quote("empréstimo de cadeiras"), None);
        let before = |q: &str, what: &str| quantity_before(q, what);
        assert_eq!(
            before("20 (vinte) cadeiras e 2 veículos", "cadeiras"),
            Some(20)
        );
        assert_eq!(
            before("20 (vinte) cadeiras e 2 veículos", "veículos"),
            Some(2)
        );
        assert_eq!(
            before("empréstimo de 20 (vinte) cadeiras", "cadeiras"),
            Some(20)
        );
        assert_eq!(before("vinte (20) cadeiras", "cadeiras"), Some(20));
        assert_eq!(before("duas salas", "salas"), Some(2));
        assert_eq!(before("20 novas cadeiras", "cadeiras"), Some(20));
        assert_eq!(before("20 de cadeiras", "cadeiras"), Some(20));
        assert_eq!(before("30 pessoas e cadeiras", "cadeiras"), None);
        assert_eq!(before("cadeiras 20", "cadeiras"), None);
        assert_eq!(before("a cadeira", "cadeira"), None);
        assert_eq!(
            before("20 (trinta) cadeiras", "cadeiras"),
            None,
            "they disagree"
        );
        assert_eq!(before("sala 3 de reuniões", "reuniões"), None);
        assert_eq!(before("o ano de 2026 cadeiras", "cadeiras"), None);
        assert_eq!(before("20 cadeiras", "mesas"), None);
        assert_eq!(before("20 CADEIRAS", "cadeiras"), Some(20));
    }

    #[test]
    fn what_the_person_wrote_is_searched_by_its_values_and_is_never_a_checked_value() {
        use crate::model::Origin;
        let context = r#"{"informado_pelo_usuario":{"purpose":"responder","received_date":"2026-11-10","notes":"Por telefone, pediram a sala 3 em 19/11/2026 às 15h."},"notas_salvas":""}"#;
        // The punctuation of the JSON is not text.
        assert_eq!(
            quote_origin(
                r#"received_date":"2026-11-10"#,
                "texto do documento",
                context,
                &[],
                ""
            ),
            Origin::Nowhere
        );
        assert_eq!(
            quote_origin(
                "pediram a sala 3 em 19/11/2026",
                "texto do documento",
                context,
                &[],
                ""
            ),
            Origin::Context
        );
        assert_eq!(
            quote_origin("texto do documento", "texto do documento", context, &[], ""),
            Origin::Document
        );
        assert!(context_values("Nota simples sobre a sala").contains("sala"));
        // A fact found only there keeps its quote but no value.
        let mut f = fact("pediram a sala 3 em 19/11/2026 às 15h");
        f.date = Some("2026-11-19".into());
        f.time = Some("15:00".into());
        let checked = check_fact(&mut f, "texto do documento", context, &[]);
        assert!(checked.quote && checked.origin == Origin::Context);
        assert!(!checked.date && !checked.time && f.date.is_none() && f.time.is_none());
        // The same words in the document are checked.
        let mut f = fact("pediram a sala 3 em 19/11/2026 às 15h");
        f.date = Some("2026-11-19".into());
        f.time = Some("15:00".into());
        let checked = check_fact(
            &mut f,
            "Por telefone, pediram a sala 3 em 19/11/2026 às 15h.",
            context,
            &[],
        );
        assert!(checked.date && checked.time && checked.origin == Origin::Document);
        // A selected source says "fonte".
        let sources = [source("C1F2P1", "Anexo: reunião em 30/11/2026, às 9h30.")];
        let mut f = fact("reunião em 30/11/2026, às 9h30");
        f.source_id = "C1F2P1".into();
        assert_eq!(check_fact(&mut f, "", "", &sources).origin, Origin::Source);
    }

    #[test]
    fn an_analysis_is_checked_again_against_the_document() {
        use crate::model::{sample_proposed_analysis, sample_proposed_capture};
        let capture = sample_proposed_capture();
        // The document still says what the analysis says: nothing changes.
        let mut analysis = sample_proposed_analysis(1);
        let before = analysis.clone();
        assert_eq!(recheck_analysis(&mut analysis, &capture.text, ""), 0);
        assert_eq!(analysis.facts, before.facts);
        assert_eq!(analysis.proposals, before.proposals);
        assert_eq!(analysis.deadline_candidate, before.deadline_candidate);
        // Another document (a tampered or replaced one): nothing holds.
        let mut analysis = sample_proposed_analysis(1);
        let cleared = recheck_analysis(&mut analysis, "Outro texto, sem nenhum dos trechos.", "");
        assert!(cleared >= 5, "{cleared}");
        assert!(analysis.facts.iter().all(|f| !f.checked.quote));
        assert!(analysis
            .facts
            .iter()
            .all(|f| f.date.is_none() && f.quantity.is_none()));
        let proposals = analysis.proposals.as_ref().unwrap();
        assert!(proposals.tasks.is_empty() && proposals.reservations.is_empty());
        assert!(!proposals.nothing_reason.is_empty() || proposals.attendance.is_some());
        assert_eq!(analysis.deadline_candidate, None);
        crate::storage::validate_analysis(&analysis).unwrap();
        // Only the deadline fact is gone: the task keeps standing on its other
        // facts but loses the date, the reservation keeps its resource.
        let mut analysis = sample_proposed_analysis(1);
        analysis.proposals.as_mut().unwrap().tasks[0]
            .facts
            .push("f2".into());
        let text = "Solicitamos a reserva de 20 (vinte) cadeiras no dia 12 de novembro de 2026, das 14h às 17h.";
        recheck_analysis(&mut analysis, text, "");
        let proposals = analysis.proposals.as_ref().unwrap();
        assert_eq!(proposals.tasks.len(), 1);
        assert!(proposals.tasks[0].due_date.is_none() && proposals.tasks[0].due_fact.is_none());
        assert_eq!(proposals.tasks[0].facts, vec!["f2".to_owned()]);
        assert_eq!(proposals.reservations[0].quantity, 20);
        assert_eq!(
            proposals.reservations[0].start.as_deref(),
            Some("2026-11-12T14:00")
        );
        crate::storage::validate_analysis(&analysis).unwrap();
        // A forged date, time or quantity that the document does not support.
        let mut analysis = sample_proposed_analysis(1);
        let text = "Solicitamos a reserva de 20 (vinte) cadeiras no dia 12 de novembro de 2026, das 14h às 17h, responder até 30/11/2026.";
        analysis.facts[2].quote = "no dia 12 de novembro de 2026, das 14h às 17h".into();
        assert_eq!(recheck_analysis(&mut analysis, text, ""), 0);
        analysis.facts[1].quote = "reserva de 20 (vinte) cadeiras e 2 veículos".into();
        let text = "Solicitamos a reserva de 20 (vinte) cadeiras e 2 veículos no dia 12 de novembro de 2026, das 14h às 17h, responder até 30/11/2026.";
        let cleared = recheck_analysis(&mut analysis, text, "");
        assert!(cleared >= 1);
        assert_eq!(analysis.facts[1].quantity, None);
        assert_eq!(
            analysis.proposals.as_ref().unwrap().reservations[0].quantity,
            20,
            "still the number written before the name"
        );
    }
}
