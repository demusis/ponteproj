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
    let quote = normalized(quote);
    if quote.is_empty() {
        return false;
    }
    if source_id.is_empty() {
        return normalized(capture_text).contains(&quote)
            || normalized(human_context).contains(&quote);
    }
    sources
        .iter()
        .find(|source| source.id == source_id)
        .is_some_and(|source| normalized(&source.text).contains(&quote))
}

/// Lowercase without accents, for comparing words.
fn fold(word: &str) -> String {
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

/// Checks the claims of a fact against its quote and sets `fact.checked`
/// (also returned). The quote must be written where `source_id` says
/// ([`find_quote`]) and measure 8 to 600 characters once the white space is
/// collapsed. Each claimed value (`date`, `time`, `end_time`, `quantity`) is
/// "conferido" only when it is among what the quote itself says
/// ([`dates_in`], [`times_in`], [`quantities_in`]); a claim that is not
/// there is cleared (set to `None`) and the fact is kept with that check
/// false. A date or time is first read the way a person types it, so
/// `"9:30"` becomes `"09:30"`. When the quote is not found nothing about
/// the fact is checked and all its claims are cleared. `end_time` must be
/// after `time` when both are claimed (a reservation does not cross
/// midnight), else it is cleared.
pub fn check_fact(
    fact: &mut crate::model::Fact,
    capture_text: &str,
    human_context: &str,
    sources: &[Source],
) -> crate::model::FactCheck {
    use crate::model::{FactCheck, FACT_QUOTE_MAX, FACT_QUOTE_MIN};
    let length = normalized(&fact.quote).chars().count();
    let quote = (FACT_QUOTE_MIN..=FACT_QUOTE_MAX).contains(&length)
        && find_quote(
            &fact.quote,
            capture_text,
            human_context,
            sources,
            &fact.source_id,
        );
    let mut checked = FactCheck {
        quote,
        ..FactCheck::default()
    };
    if !quote {
        fact.date = None;
        fact.time = None;
        fact.end_time = None;
        fact.quantity = None;
        fact.checked = checked;
        return checked;
    }
    let dates = dates_in(&fact.quote);
    let times = times_in(&fact.quote);
    fact.date = fact
        .date
        .take()
        .and_then(|d| crate::labels::parse_typed_date(&d))
        .filter(|d| dates.contains(d));
    checked.date = fact.date.is_some();
    let read_time = |value: Option<String>| {
        value
            .and_then(|t| crate::labels::parse_typed_time(&t))
            .filter(|t| times.contains(t))
    };
    fact.time = read_time(fact.time.take());
    fact.end_time = read_time(fact.end_time.take());
    if let (Some(start), Some(end)) = (&fact.time, &fact.end_time) {
        if end <= start {
            fact.end_time = None;
        }
    }
    checked.time = fact.time.is_some();
    checked.end_time = fact.end_time.is_some();
    let quantities = quantities_in(&fact.quote);
    fact.quantity = fact.quantity.take().filter(|q| quantities.contains(q));
    checked.quantity = fact.quantity.is_some();
    fact.checked = checked;
    checked
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
    fn an_end_time_must_come_after_the_start() {
        let mut f = fact("das 14h às 17h na sala principal");
        f.time = Some("17:00".into());
        f.end_time = Some("14:00".into());
        let checked = check_fact(&mut f, "das 14h às 17h na sala principal", "", &[]);
        assert!(checked.time && !checked.end_time && f.end_time.is_none());
    }
}
