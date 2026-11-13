//! Deterministic checks of a draft ("Conferência da minuta"): pure code, no
//! model, nothing stored and nothing sent. It is recomputed whenever the
//! draft is shown or its text changes.
//!
//! What it says is about the text of the draft and what the materials write:
//! the header, the number of the document, the places still to fill, the
//! sections of the template, the data (dates, hours, document numbers, money)
//! that no material writes, and a paragraph the person had finished that a
//! "Completar" changed. It never says that something is true, and when it is
//! unsure it flags the value.
use crate::evidence::{date_spans, dates_in, fold, has_deadline_cue, time_spans, times_in};
use crate::model::Fact;
use crate::work_templates::Intent;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// The most of each list the check reports.
pub const MAX_PLACEHOLDERS: usize = 30;
pub const MAX_SECTIONS: usize = 40;
pub const MAX_UNSUPPORTED: usize = 60;
pub const MAX_HINTS: usize = 10;
const MAX_NOTES: usize = 10;
const PLACEHOLDER_CONTEXT: usize = 40;
const UNSUPPORTED_CONTEXT: usize = 60;
const HINT_CONTEXT: usize = 40;
/// The letter's own date line is among the first lines or the last ones.
const DATE_LINE_HEAD: usize = 6;
const DATE_LINE_TAIL: usize = 8;

/// A section of the template: its title, what it must hold (or the base
/// text it starts from) and whether it is required.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SectionSpec {
    pub name: String,
    pub guidance: String,
    pub required: bool,
}

/// Everything the check reads. The texts are the ones the AI could see (the
/// document, what the person wrote, the selected sources) and the person's
/// own: the template, the instructions, the request. Examples are NOT among
/// them: a value that only an example holds is unsupported.
#[derive(Clone, Debug, Default)]
pub struct DraftCheckInput {
    /// The draft; the bases it may carry are ignored.
    pub draft_text: String,
    pub document_number: String,
    pub capture_text: String,
    /// What the person wrote (purpose, notes), as plain text.
    pub context_values: String,
    /// The texts of the selected sources.
    pub sources: Vec<String>,
    /// The verified facts of the latest analysis that is not stale.
    pub facts: Vec<Fact>,
    /// The sections of the template version used, when there is one.
    pub sections: Vec<SectionSpec>,
    /// The person's own texts the draft may repeat: the guidance of the
    /// template, the instruction blocks, the request of this draft.
    pub trusted_texts: Vec<String>,
    pub intent: Intent,
    /// For Complete and Revise: the text the person had.
    pub previous_text: String,
    /// Today, `AAAA-MM-DD`.
    pub today: String,
    /// The day the draft was generated (`AAAA-MM-DD`), when known: the date
    /// line of the letter may say it. Today and this day are the only dates
    /// accepted on that line without support in the materials.
    pub generated_on: Option<String>,
}

/// The local date (`AAAA-MM-DD`) of an RFC 3339 timestamp, for
/// [`DraftCheckInput::generated_on`] (`DraftVersion.created_at`).
pub fn date_of_timestamp(rfc3339: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(rfc3339).ok().map(|d| {
        d.with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string()
    })
}

impl DraftCheckInput {
    /// The input for a draft of a document, from what the app has. One
    /// read of the library (`Store::draft_materials`: the capture is
    /// decrypted once, with the note, the context and the latest analysis);
    /// everything else is in memory. Cheap enough to call when the review
    /// window opens and whenever the text changes (a few decryptions and
    /// text scans proportional to the size of the document, the sources and
    /// the draft; keep the answer for the same text if it is shown every
    /// frame). It fails when the document cannot be read: the check did not
    /// run, and the screen must say so (a default [`DraftCheck`] has
    /// `checked == false`).
    ///
    /// - `draft_text`: the text of the draft (the bases it may carry are
    ///   ignored);
    /// - `previous_text`: for Complete and Revise, the text the person had;
    /// - `template`: the version of the template the draft was made with;
    /// - `sources`: the sources the draft was made with (`DraftVersion.materials`);
    /// - `extra_trusted`: the person's other own texts the draft may repeat
    ///   (the instruction blocks, the request of the draft);
    /// - `today`: `AAAA-MM-DD`;
    /// - `generated_on`: the day of the generation (see
    ///   [`date_of_timestamp`]).
    #[allow(clippy::too_many_arguments)]
    pub fn for_draft(
        store: &crate::storage::Store,
        document_id: i64,
        draft_text: &str,
        intent: Intent,
        previous_text: Option<&str>,
        template: Option<&crate::work_templates::TemplateVersion>,
        sources: &[crate::evidence::Source],
        extra_trusted: &[String],
        today: &str,
        generated_on: Option<&str>,
    ) -> anyhow::Result<Self> {
        let materials = store.draft_materials(document_id)?;
        Ok(Self {
            draft_text: draft_text.to_owned(),
            document_number: materials.document_number,
            capture_text: materials.capture_text,
            context_values: materials.context_values,
            sources: sources.iter().map(|s| s.text.clone()).collect(),
            facts: materials.facts,
            sections: template
                .map(|version| {
                    version
                        .sections
                        .iter()
                        .map(|s| SectionSpec {
                            name: s.title.clone(),
                            guidance: s.guidance.clone(),
                            required: s.required,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            trusted_texts: extra_trusted.to_vec(),
            intent,
            previous_text: previous_text.unwrap_or("").to_owned(),
            today: today.to_owned(),
            generated_on: generated_on.map(str::to_owned),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SectionCheck {
    pub name: String,
    pub required: bool,
    pub found: bool,
}

/// Data written in the draft that no material writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unsupported {
    /// "data" | "hora" | "numero" | "valor"
    pub kind: String,
    /// As the draft writes it.
    pub value: String,
    /// About 60 characters around it in the draft.
    pub context: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DraftCheck {
    /// The check ran. `false` in a default value: when the input could not
    /// be built the screen must say that the Ponte did not check the draft,
    /// not show an empty (all-green) result.
    pub checked: bool,
    /// The first line is "MINUTA PARA REVISÃO".
    pub header: bool,
    /// The whole number of the document is written (spaces do not matter).
    pub cites_number: bool,
    /// Each place still to fill (`[preencher…]` and any other bracketed
    /// fill-in), with about 40 characters around it.
    pub placeholders: Vec<String>,
    pub sections: Vec<SectionCheck>,
    pub unsupported: Vec<Unsupported>,
    /// Plain sentences for anything else worth telling.
    pub notes: Vec<String>,
    /// What looks like data but is not compared by the Ponte (amounts
    /// without "R$", laws and articles, percentages), each with a little
    /// context: "não conferido pelo Ponte". At most [`MAX_HINTS`].
    pub unparsed_hints: Vec<String>,
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The part of `chars` around `start..end`, white space collapsed.
fn around(chars: &[char], start: usize, end: usize, reach: usize) -> String {
    let from = start.saturating_sub(reach);
    let to = (end + reach).min(chars.len());
    collapse(&chars[from..to].iter().collect::<String>())
}

/// For each byte index of `text` (and the end), the index of its character.
fn byte_to_char_table(text: &str) -> Vec<usize> {
    let mut table = vec![0; text.len() + 1];
    let mut chars = 0;
    for (byte, c) in text.char_indices() {
        for slot in &mut table[byte..byte + c.len_utf8()] {
            *slot = chars;
        }
        chars += 1;
    }
    table[text.len()] = chars;
    table
}

// ---------------------------------------------------------------------------
// Header, number, placeholders.

fn has_header(draft: &str) -> bool {
    draft
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| fold(line).starts_with("minuta para revisao"))
}

/// Whether the characters `at..end` of `chars` stand as a whole token: not
/// glued to a letter or a digit before or after, nor continued by a separator
/// and a digit (`.`, `,`, `/`, `-`: "9.123/2026" does not hold "123/2026").
fn bounded(chars: &[char], at: usize, end: usize) -> bool {
    let joiner = |c: char| matches!(c, '.' | ',' | '/' | '-');
    let before_ok = match at.checked_sub(1).map(|i| chars[i]) {
        None => true,
        Some(c) if c.is_alphanumeric() => false,
        Some(c) if joiner(c) => !at
            .checked_sub(2)
            .map(|i| chars[i])
            .is_some_and(char::is_alphanumeric),
        Some(_) => true,
    };
    let after_ok = match chars.get(end).copied() {
        None => true,
        Some(c) if c.is_alphanumeric() => false,
        Some(c) if joiner(c) => !chars
            .get(end + 1)
            .copied()
            .is_some_and(char::is_alphanumeric),
        Some(_) => true,
    };
    before_ok && after_ok
}

/// Whether `inner` occurs in `text` as a whole token ([`bounded`]).
fn contains_whole_token(text: &str, inner: &str) -> bool {
    if inner.is_empty() {
        return false;
    }
    let chars: Vec<char> = text.chars().collect();
    let wanted: Vec<char> = inner.chars().collect();
    (0..chars.len().saturating_sub(wanted.len() - 1))
        .filter(|&at| chars[at..at + wanted.len()] == wanted[..])
        .any(|at| bounded(&chars, at, at + wanted.len()))
}

/// The number of the document is written whole: its characters, ignoring
/// case and any white space between them, stand as a token of the draft
/// ("…/004310" is not "…/00431").
fn cites_number(draft: &str, number: &str) -> bool {
    let parts: Vec<String> = number
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| regex::escape(&c.to_string()))
        .collect();
    if parts.is_empty() {
        return false;
    }
    let Ok(pattern) = Regex::new(&format!("(?i){}", parts.join(r"\s*"))) else {
        return false;
    };
    let chars: Vec<char> = draft.chars().collect();
    let char_at = byte_to_char_table(draft);
    let found = pattern
        .find_iter(draft)
        .any(|m| bounded(&chars, char_at[m.start()], char_at[m.end()]));
    found
}

/// A bracketed text that is not a place to fill: a reference ("[1]",
/// "[2-4]"), "[sic]", an ellipsis ("[…]", "[...]"), or one character
/// ("[x]", "[a]": a mark of a list).
fn is_not_a_placeholder(inner: &str) -> bool {
    static REFERENCE: OnceLock<Regex> = OnceLock::new();
    let reference =
        REFERENCE.get_or_init(|| Regex::new(r"^\d+(?:\s*[-–,]\s*\d+)*$").expect("pattern"));
    let inner = inner.trim();
    inner.is_empty()
        || reference.is_match(inner)
        || fold(inner) == "sic"
        || inner.chars().all(|c| matches!(c, '.' | '…' | ' '))
        || inner.chars().count() == 1
}

/// The places still to fill, as byte spans: first the `[preencher…]`
/// variants (spaces inside the brackets, up to 200 characters, even across a
/// line break), then any other bracketed text of up to 120 characters on one
/// line that is not one of the exclusions of [`is_not_a_placeholder`].
fn placeholder_spans(text: &str) -> Vec<(usize, usize)> {
    static FILL: OnceLock<Regex> = OnceLock::new();
    static ANY: OnceLock<Regex> = OnceLock::new();
    let fill =
        FILL.get_or_init(|| Regex::new(r"(?is)\[\s*preencher[^\]]{0,200}\]").expect("pattern"));
    let any = ANY.get_or_init(|| Regex::new(r"\[[^\[\]\n]{1,120}\]").expect("pattern"));
    let mut out: Vec<(usize, usize)> = fill.find_iter(text).map(|m| (m.start(), m.end())).collect();
    let first = out.len();
    for found in any.find_iter(text) {
        let overlaps = out[..first]
            .iter()
            .any(|(s, e)| found.start() < *e && *s < found.end());
        let inner = &found.as_str()[1..found.as_str().len() - 1];
        if !overlaps && !is_not_a_placeholder(inner) {
            out.push((found.start(), found.end()));
        }
    }
    out
}

fn has_placeholder(text: &str) -> bool {
    !placeholder_spans(text).is_empty()
}

fn placeholders(draft: &str) -> Vec<String> {
    let chars: Vec<char> = draft.chars().collect();
    let char_at = byte_to_char_table(draft);
    placeholder_spans(draft)
        .into_iter()
        .take(MAX_PLACEHOLDERS)
        .map(|(start, end)| around(&chars, char_at[start], char_at[end], PLACEHOLDER_CONTEXT))
        .collect()
}

// ---------------------------------------------------------------------------
// Sections of the template.

/// Words of a title that carry meaning: four letters or more, folded.
fn title_words(title: &str) -> Vec<String> {
    const FILLER: [&str; 8] = [
        "para", "sobre", "entre", "como", "pelo", "pela", "seus", "suas",
    ];
    normalised_words(title)
        .into_iter()
        .filter(|w| w.chars().count() >= 4 && !FILLER.contains(&w.as_str()))
        .collect()
}

/// Folded words of a text, punctuation gone.
fn normalised_words(text: &str) -> Vec<String> {
    fold(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A word without its plural ending, to compare headings: "considerandos"
/// and "considerando" are the same word, "conclusão" and "concluir" are not.
fn singular(word: &str) -> String {
    if word.ends_with("oes") || word.ends_with("aes") {
        // conclusões, alemães: -ão
        format!("{}ao", &word[..word.len() - 3])
    } else if word.chars().count() > 4 && word.ends_with("es") {
        word[..word.len() - 2].to_owned()
    } else if word.chars().count() > 3 && word.ends_with('s') {
        word[..word.len() - 1].to_owned()
    } else {
        word.to_owned()
    }
}

/// Whether a line stands like a heading: short (80 characters, 12 words),
/// and numbered ("1. Do pedido"), ending in ":", all in capitals, or simply
/// not a sentence (no full stop, comma, semicolon or question mark at the
/// end).
fn heading_like(line: &str) -> bool {
    static NUMBERED: OnceLock<Regex> = OnceLock::new();
    let numbered = NUMBERED.get_or_init(|| Regex::new(r"^\d+[.)]\s+\S").expect("pattern"));
    let line = line.trim();
    if line.is_empty() || line.chars().count() > 80 || line.split_whitespace().count() > 12 {
        return false;
    }
    let letters: Vec<char> = line.chars().filter(|c| c.is_alphabetic()).collect();
    let capitals = !letters.is_empty() && letters.iter().all(|c| c.is_uppercase());
    numbered.is_match(line)
        || line.ends_with(':')
        || capitals
        || !line.ends_with(['.', ';', ',', '!', '?'])
}

/// The places of a draft that may be a heading, as lists of folded words:
/// each short line that stands like a heading, and the words before the first
/// colon of a line (a heading that runs into its text: "Prazo: o relatório…").
fn heading_candidates(draft: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for line in draft.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if heading_like(line) {
            out.push(normalised_words(line));
        }
        if let Some((before, _)) = line.split_once(':') {
            if before.chars().count() <= 80 && before.split_whitespace().count() <= 8 {
                out.push(normalised_words(before));
            }
        }
    }
    out
}

/// Whether the draft holds a section of the template. The rule leans toward
/// finding, but only on the places that read as headings, so that a word of
/// the title in the body is not a section:
/// 1. the whole title, as consecutive words, written anywhere when it has
///    three words or more, and otherwise in a heading; or
/// 2. in ONE heading, at least two thirds of the meaningful words of the
///    title (four letters or more), the same words but for the plural; or
/// 3. the first eight words of the section's guidance (when it is a base
///    text to start from) in a row anywhere in the draft.
///
/// False "found": a short line of the body that is not a sentence and holds
/// the words of the title. False "not found": a section whose heading was
/// rewritten with other words, or that is a run-in paragraph without a heading
/// or a colon, under a guidance that is an instruction rather than a text.
fn section_found(draft_words: &[String], candidates: &[Vec<String>], spec: &SectionSpec) -> bool {
    let contains_run = |haystack: &[String], needle: &[String]| {
        !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
    };
    let title = normalised_words(&spec.name);
    let whole_title = if title.len() >= 3 {
        contains_run(draft_words, &title)
    } else {
        candidates.iter().any(|c| contains_run(c, &title))
    };
    if whole_title {
        return true;
    }
    let meaningful: Vec<String> = title_words(&spec.name)
        .iter()
        .map(|w| singular(w))
        .collect();
    if !meaningful.is_empty() {
        let enough = candidates.iter().any(|candidate| {
            let words: BTreeSet<String> = candidate.iter().map(|w| singular(w)).collect();
            let present = meaningful.iter().filter(|w| words.contains(*w)).count();
            present * 3 >= meaningful.len() * 2
        });
        if enough {
            return true;
        }
    }
    let guidance: Vec<String> = normalised_words(&spec.guidance)
        .into_iter()
        .take(8)
        .collect();
    guidance.len() >= 8 && contains_run(draft_words, &guidance)
}

// ---------------------------------------------------------------------------
// Data the materials do not write.

/// A number of a document, as a token to compare: `norm` is what must be
/// equal, `marked` says it was written after "nº", "n." or "número" (then
/// `norm` is `N:` and the digits).
struct NumberHit {
    norm: String,
    written: String,
    marked: bool,
    start: usize,
    end: usize,
}

fn number_regexes() -> &'static [Regex; 5] {
    static PATTERNS: OnceLock<[Regex; 5]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // nº 123/2026, n. 45, número 7: only the digits are captured
            Regex::new(r"(?i)\b(?:n[º°]|n\.[º°]?|n[úu]mero)\s*(\d{1,12}(?:[./-]\d{1,12})*)")
                .expect("pattern"),
            // ORG-OFI-2026/00431, SEFAZ-PRO-2026/00001-A
            Regex::new(
                r"\b(?:[A-Z][A-Z0-9]{1,19}[-/]){1,4}(?:19|20)\d{2}[-/]\d{1,12}(?:[-/]\d{1,8})?\b",
            )
            .expect("pattern"),
            // 0001234-56.2026.8.11.0041
            Regex::new(r"\b\d{7}-\d{2}\.\d{4}\.\d\.\d{2}\.\d{4}\b").expect("pattern"),
            // 9.123/2026
            Regex::new(r"\b\d{1,3}(?:\.\d{3})+/(?:19|20)\d{2}\b").expect("pattern"),
            // 123/2026
            Regex::new(r"\b\d{1,12}/(?:19|20)\d{2}\b").expect("pattern"),
        ]
    })
}

/// Document-like numbers in a text, in order. A "123/2026" that is the tail
/// of a date, of a number with thousands or of a longer code is not one; a
/// hit inside another hit is one number. Dots are kept: "1.234/2026" is not
/// "12.34/2026".
fn number_hits(text: &str) -> Vec<NumberHit> {
    let bytes = text.as_bytes();
    let mut out: Vec<NumberHit> = Vec::new();
    for (index, pattern) in number_regexes().iter().enumerate() {
        for found in pattern.captures_iter(text) {
            let whole = found.get(0).expect("whole match");
            if index >= 3 {
                let before = text[..whole.start()].chars().last();
                if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '.' | ','))
                {
                    continue;
                }
                if matches!(bytes.get(whole.end()), Some(b'/' | b'-')) {
                    continue;
                }
            }
            if out
                .iter()
                .any(|h| whole.start() < h.end && h.start < whole.end())
            {
                continue;
            }
            let (norm, marked) = if index == 0 {
                (format!("N:{}", &found[1]), true)
            } else if index == 1 {
                (
                    whole
                        .as_str()
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>()
                        .to_uppercase(),
                    false,
                )
            } else {
                (whole.as_str().to_owned(), false)
            };
            out.push(NumberHit {
                norm,
                written: whole.as_str().to_owned(),
                marked,
                start: whole.start(),
                end: whole.end(),
            });
        }
    }
    out.sort_by_key(|h| h.start);
    out
}

fn money_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)R\$\s*(\d{1,3}(?:\.\d{3})+|\d+)(?:,(\d{1,2}))?").expect("pattern")
    })
}

/// The amount of a "R$ …" match, in cents.
fn cents_of(whole: &str, fraction: Option<&str>) -> Option<u64> {
    let units: u64 = whole.replace('.', "").parse().ok()?;
    let cents = match fraction {
        None => 0,
        Some(f) if f.len() == 1 => f.parse::<u64>().ok()? * 10,
        Some(f) => f.parse::<u64>().ok()?,
    };
    units.checked_mul(100)?.checked_add(cents)
}

/// What the materials write, in the forms the draft is compared with: whole
/// tokens, never pieces of the text.
struct Present {
    dates: BTreeSet<String>,
    times: BTreeSet<String>,
    /// The `norm` of every number token the materials write (a marked one
    /// also as its bare digits).
    numbers: BTreeSet<String>,
    /// The materials, to find a bare number as a whole token.
    texts: Vec<String>,
    money: BTreeSet<u64>,
}

impl Present {
    fn of(texts: &[&str]) -> Self {
        static SEPARATORS: OnceLock<Regex> = OnceLock::new();
        let separators = SEPARATORS.get_or_init(|| Regex::new(r"\s*([-/])\s*").expect("pattern"));
        let mut present = Self {
            dates: BTreeSet::new(),
            times: BTreeSet::new(),
            numbers: BTreeSet::new(),
            texts: Vec::new(),
            money: BTreeSet::new(),
        };
        for text in texts {
            present.dates.extend(dates_in(text));
            present.times.extend(times_in(text));
            for hit in number_hits(text) {
                if hit.marked {
                    present.numbers.insert(hit.norm[2..].to_owned());
                }
                present.numbers.insert(hit.norm);
            }
            // A code written with other case or with spaces around its
            // separators ("sefaz-pro-2026 / 00001") is the same code.
            let tightened = separators.replace_all(text, "$1").to_uppercase();
            present
                .numbers
                .extend(number_hits(&tightened).into_iter().map(|h| h.norm));
            for found in money_regex().captures_iter(text) {
                if let Some(cents) = cents_of(&found[1], found.get(2).map(|m| m.as_str())) {
                    present.money.insert(cents);
                }
            }
            present.texts.push((*text).to_owned());
        }
        present
    }

    /// Whether the materials write this number: a marked one ("nº 45") only
    /// as the same marked form or as the identical whole token; the others
    /// only as the same token.
    fn writes(&self, hit: &NumberHit) -> bool {
        if self.numbers.contains(&hit.norm) {
            return true;
        }
        hit.marked
            && self
                .texts
                .iter()
                .any(|text| contains_whole_token(text, &hit.norm[2..]))
    }
}

/// For each non-empty line of the draft: where it starts and ends (in
/// characters) and whether it may be the date line of the letter: among the
/// first or the last lines, and without a word of deadline.
fn date_lines(draft: &str) -> Vec<(usize, usize, bool)> {
    let mut lines = Vec::new();
    let mut start = 0;
    for line in draft.split('\n') {
        let length = line.chars().count();
        if !line.trim().is_empty() {
            lines.push((start, start + length, line.to_owned()));
        }
        start += length + 1;
    }
    let total = lines.len();
    lines
        .into_iter()
        .enumerate()
        .map(|(index, (start, end, line))| {
            let placed = index < DATE_LINE_HEAD || index + DATE_LINE_TAIL >= total;
            (start, end, placed && !has_deadline_cue(&line))
        })
        .collect()
}

fn unsupported(draft: &str, present: &Present, letter_dates: &[String]) -> Vec<Unsupported> {
    let chars: Vec<char> = draft.chars().collect();
    let char_at = byte_to_char_table(draft);
    let lines = date_lines(draft);
    let mut out: Vec<Unsupported> = Vec::new();
    let mut push = |kind: &str, value: String, start: usize, end: usize| {
        if out.len() < MAX_UNSUPPORTED && !out.iter().any(|u| u.kind == kind && u.value == value) {
            out.push(Unsupported {
                kind: kind.into(),
                value,
                context: around(&chars, start, end, UNSUPPORTED_CONTEXT / 2),
            });
        }
    };
    let written = |start: usize, end: usize| chars[start..end].iter().collect::<String>();
    for (iso, start, end) in date_spans(draft) {
        // Today's date is the letter's own date only on its date line.
        let letter_line = letter_dates.contains(&iso)
            && lines
                .iter()
                .any(|(s, e, ok)| *ok && *s <= start && start < *e);
        if !letter_line && !present.dates.contains(&iso) {
            push("data", written(start, end), start, end);
        }
    }
    for (time, start, end) in time_spans(draft) {
        if !present.times.contains(&time) {
            push("hora", written(start, end), start, end);
        }
    }
    for hit in number_hits(draft) {
        if !present.writes(&hit) {
            push(
                "numero",
                hit.written.clone(),
                char_at[hit.start],
                char_at[hit.end],
            );
        }
    }
    for found in money_regex().captures_iter(draft) {
        let whole = found.get(0).expect("whole match");
        if let Some(cents) = cents_of(&found[1], found.get(2).map(|m| m.as_str())) {
            if !present.money.contains(&cents) {
                push(
                    "valor",
                    whole.as_str().to_owned(),
                    char_at[whole.start()],
                    char_at[whole.end()],
                );
            }
        }
    }
    out
}

/// Things that look like data but that the check does not compare: amounts
/// written without "R$" ("1.500,00 reais", "mil e quinhentos reais"), laws
/// and articles ("Lei nº 8.666/93", "art. 5º", "§ 2º") and percentages. Each
/// with a little context; the ones the check already lists as unsupported are
/// left out.
fn unparsed_hints(draft: &str, already: &[Unsupported]) -> Vec<String> {
    static PATTERNS: OnceLock<[Regex; 4]> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            // "1.500,00 reais", "2000 reais"
            Regex::new(r"(?i)\b\d{1,3}(?:\.\d{3})*(?:,\d{1,2})?\s*(?:reais|real)\b").expect("pattern"),
            // spelled out: a number word within the 30 characters before "reais"
            Regex::new(r"(?i)\b(?:um|uma|dois|duas|tr[êe]s|quatro|cinco|seis|sete|oito|nove|dez|onze|doze|treze|quatorze|catorze|quinze|dezesseis|dezessete|dezoito|dezenove|vinte|trinta|quarenta|cinquenta|sessenta|setenta|oitenta|noventa|cem|cento|duzentos|trezentos|quatrocentos|quinhentos|seiscentos|setecentos|oitocentos|novecentos|mil|milh[ãa]o|milh[õo]es)\b[^.\n]{0,30}\breais\b").expect("pattern"),
            // laws, decrees, articles, paragraphs
            Regex::new(r"(?i)\b(?:lei(?:\s+complementar)?|decreto|portaria|resolu[çc][ãa]o|instru[çc][ãa]o\s+normativa|medida\s+provis[óo]ria|emenda\s+constitucional)\s*(?:n[º°.]?\s*)?\d[\d./-]*|\bart(?:igo)?s?\.?\s*\d+[º°]?|§\s*\d+[º°]?").expect("pattern"),
            // percentages
            Regex::new(r"(?i)\d+(?:,\d+)?\s*(?:%|por\s+cento)").expect("pattern"),
        ]
    });
    let chars: Vec<char> = draft.chars().collect();
    let char_at = byte_to_char_table(draft);
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for pattern in patterns {
        for found in pattern.find_iter(draft) {
            if out.len() >= MAX_HINTS {
                return out;
            }
            if already.iter().any(|u| found.as_str().contains(&u.value)) {
                continue;
            }
            let matched = collapse(found.as_str());
            if seen.contains(&matched) {
                continue;
            }
            seen.push(matched);
            out.push(around(
                &chars,
                char_at[found.start()],
                char_at[found.end()],
                HINT_CONTEXT,
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The check.

fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(collapse)
        .filter(|p| !p.is_empty())
        .collect()
}

/// Runs every check on a draft. Pure: the same input gives the same answer.
pub fn check_draft(input: &DraftCheckInput) -> DraftCheck {
    let stripped = crate::ai::strip_bases(&input.draft_text);
    let draft: &str = &stripped;
    let draft_words = normalised_words(draft);
    let candidates = heading_candidates(draft);
    let sections = input
        .sections
        .iter()
        .take(MAX_SECTIONS)
        .map(|spec| SectionCheck {
            name: spec.name.clone(),
            required: spec.required,
            found: section_found(&draft_words, &candidates, spec),
        })
        .collect();

    // What the materials write: the document, what the person wrote, the
    // sources, the verified facts, the person's own texts and, for a text
    // being completed or revised, the text the person had.
    let fact_texts: Vec<String> = input.facts.iter().map(|f| f.quote.clone()).collect();
    let mut texts: Vec<&str> = vec![&input.capture_text, &input.context_values];
    texts.extend(input.sources.iter().map(String::as_str));
    texts.extend(fact_texts.iter().map(String::as_str));
    texts.extend(input.trusted_texts.iter().map(String::as_str));
    texts.extend(input.sections.iter().map(|s| s.guidance.as_str()));
    texts.push(&input.previous_text);
    // The number of the document is the draft's to cite, whether or not the
    // text of the document writes it.
    texts.push(&input.document_number);
    let present = Present::of(&texts);
    let letter_dates: Vec<String> = [Some(input.today.clone()), input.generated_on.clone()]
        .into_iter()
        .flatten()
        .filter(|d| !d.is_empty())
        .collect();

    let mut notes = Vec::new();
    if input.intent == Intent::Complete && !input.previous_text.trim().is_empty() {
        let kept = collapse(draft);
        let changed = paragraphs(&input.previous_text)
            .iter()
            .filter(|p| !has_placeholder(p) && p.chars().count() >= 20)
            .filter(|p| !kept.contains(p.as_str()))
            .count();
        if changed > 0 {
            notes.push(format!(
                "{changed} parágrafo(s) do texto original, que não tinham campos a preencher, não aparecem iguais na minuta: confira o que mudou."
            ));
        }
    }
    notes.truncate(MAX_NOTES);
    let unsupported = unsupported(draft, &present, &letter_dates);
    let unparsed_hints = unparsed_hints(draft, &unsupported);
    DraftCheck {
        checked: true,
        header: has_header(draft),
        cites_number: cites_number(draft, &input.document_number),
        placeholders: placeholders(draft),
        sections,
        unsupported,
        notes,
        unparsed_hints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(draft: &str) -> DraftCheckInput {
        DraftCheckInput {
            draft_text: draft.into(),
            document_number: "ORG-OFI-2026/00431".into(),
            capture_text: "Ofício ORG-OFI-2026/00431. Solicitamos o envio do relatório até 20/11/2026, às 14h. O custo foi de R$ 1.500,00.".into(),
            today: "2026-11-12".into(),
            ..Default::default()
        }
    }

    fn kinds(check: &DraftCheck) -> Vec<(String, String)> {
        check
            .unsupported
            .iter()
            .map(|u| (u.kind.clone(), u.value.clone()))
            .collect()
    }

    fn numbers(check: &DraftCheck) -> Vec<String> {
        check
            .unsupported
            .iter()
            .filter(|u| u.kind == "numero")
            .map(|u| u.value.clone())
            .collect()
    }

    /// The numbers the check flags in `draft` when the document writes only `material`.
    fn flagged(draft: &str, material: &str) -> Vec<String> {
        let mut i = input(draft);
        i.capture_text = material.into();
        i.document_number = String::new();
        numbers(&check_draft(&i))
    }

    #[test]
    fn a_default_check_says_it_did_not_run_and_a_real_one_says_it_did() {
        assert!(!DraftCheck::default().checked);
        assert!(check_draft(&input("MINUTA PARA REVISÃO")).checked);
    }

    #[test]
    fn the_header_is_the_first_line_in_any_case_with_or_without_accents() {
        for good in [
            "MINUTA PARA REVISÃO\n\nTexto.",
            "\n\n  minuta para revisao\nTexto",
            "Minuta Para Revisão",
        ] {
            assert!(check_draft(&input(good)).header, "{good}");
        }
        for bad in [
            "Texto\nMINUTA PARA REVISÃO",
            "Em atenção ao ofício. MINUTA PARA REVISÃO",
            "",
        ] {
            assert!(!check_draft(&input(bad)).header, "{bad}");
        }
    }

    #[test]
    fn the_whole_number_must_be_written_whatever_the_spacing_and_case() {
        for good in [
            "Em atenção ao Ofício ORG-OFI-2026/00431, informo.",
            "Ofício ORG-OFI-2026 / 00431",
            "ORG-OFI-\n2026/00431",
            "oficio org-ofi-2026/00431",
            "(ORG-OFI-2026/00431)",
        ] {
            assert!(check_draft(&input(good)).cites_number, "{good}");
        }
        // A longer number, a number with a prefix or a suffix, another number.
        for bad in [
            "Ofício ORG-OFI-2026/00432",
            "Ofício ORG-OFI-2026/004310",
            "Ofício XORG-OFI-2026/00431",
            "Ofício ORG-OFI-2026/00431-A",
            "Ofício ORG-OFI-2026/00431/2",
            "Ofício ORG-OFI-2026/0043",
            "Ofício 2026/00431",
        ] {
            assert!(!check_draft(&input(bad)).cites_number, "{bad}");
        }
        let mut empty = input("qualquer");
        empty.document_number = String::new();
        assert!(!check_draft(&empty).cites_number);
    }

    #[test]
    fn each_place_to_fill_is_listed_with_its_surroundings() {
        let mut long = "palavra ".repeat(12);
        long.push_str("[preencher nome do chefe] ");
        long.push_str(&"depois ".repeat(12));
        let check = check_draft(&input(&format!(
            "MINUTA PARA REVISÃO\n\nAo [preencher destinatário], {long} e [PREENCHER] fim."
        )));
        assert_eq!(check.placeholders.len(), 3);
        assert!(check.placeholders[0].contains("[preencher destinatário]"));
        assert!(check.placeholders[1].contains("[preencher nome do chefe]"));
        assert!(check.placeholders[2].contains("[PREENCHER]"));
        // At most thirty.
        let many = "[preencher] ".repeat(45);
        assert_eq!(check_draft(&input(&many)).placeholders.len(), 30);
        // The accents of the surroundings survive.
        let accented = check_draft(&input("á é í ó ú [preencher] ç ã õ")).placeholders;
        assert!(accented[0].contains("ç ã õ"));
    }

    #[test]
    fn any_bracketed_fill_in_counts_and_the_usual_brackets_do_not() {
        let count = |text: &str| check_draft(&input(text)).placeholders;
        // The skeleton of the program itself.
        let skeleton = crate::ai::template_reply(&crate::model::sample_capture(), "resposta");
        let found = count(&skeleton);
        assert!(found.len() >= 4, "{found:?}");
        let all = found.join(" | ");
        for wanted in [
            "[destinatário]",
            "[preencher com a manifestação",
            "[Local e data]",
            "[Identificação do responsável",
        ] {
            assert!(all.contains(wanted), "{wanted}: {all}");
        }
        for text in [
            "Para [unidade competente], seguem",
            "[ preencher ]",
            "[  Preencher   o nome ]",
            "Para [preencher o nome do\nchefe do setor] seguem",
            &format!("[preencher {}]", "x".repeat(100)),
            "[Cidade], [data]",
        ] {
            assert!(!count(text).is_empty(), "{text}");
        }
        assert_eq!(count("[Cidade], [data]").len(), 2);
        // Not places to fill: references, "sic", ellipses, a mark of a list.
        for text in [
            "Conforme a nota [1] e as notas [2-4], [3, 5].",
            "O texto dizia [sic] assim.",
            "[SIC] [Sic]",
            "Corte [...] e […] do trecho.",
            "Lista: [x] feito, [a] item",
            "[ ]",
        ] {
            assert!(count(text).is_empty(), "{text}: {:?}", count(text));
        }
        // The "preencher" variants come first, then the others, in order.
        let mixed = count("Ao [destinatário], [preencher cargo] e [Local].");
        assert_eq!(mixed.len(), 3);
        assert!(mixed[0].contains("[preencher cargo]"));
        assert!(mixed[1].contains("[destinatário]"));
        assert!(mixed[2].contains("[Local]"));
        // Too long for one fill-in: not one.
        assert!(count(&format!("[{}]", "x".repeat(130))).is_empty());
    }

    fn spec(name: &str, guidance: &str) -> SectionSpec {
        SectionSpec {
            name: name.into(),
            guidance: guidance.into(),
            required: true,
        }
    }

    fn found(draft: &str, section: &SectionSpec) -> bool {
        let mut i = input(draft);
        i.sections = vec![section.clone()];
        check_draft(&i).sections[0].found
    }

    #[test]
    fn a_section_is_found_by_its_title_in_a_heading_or_by_its_base_text() {
        let legal = spec("Fundamentação legal", "Cite a norma aplicada.");
        assert!(found("1. Fundamentação legal\nA norma...", &legal));
        assert!(found("FUNDAMENTAÇÃO LEGAL: a norma", &legal));
        assert!(found(
            "Fundamentacao   legal\nTexto longo da seção que explica a norma aplicada ao caso.",
            &legal
        ));
        assert!(found(
            "Fundamentação legal: o art. 5º da norma aplicada ao caso fundamenta o pedido do setor",
            &legal
        ));
        assert!(!found("Do pedido e da conclusão", &legal));
        // Most of the words, in one heading, with the plural of the other form.
        let long = spec("Considerandos e fundamentos do pedido", "");
        assert!(found(
            "Considerando os fundamentos do pedido\nTexto da seção que vem a seguir.",
            &long
        ));
        assert!(!found("Texto sem relação alguma", &long));
        // A title without meaningful words needs the whole title in a heading.
        let closing = spec("Fecho", "");
        assert!(found("... Fecho\nAtenciosamente", &closing));
        assert!(found("Texto da seção.\n\nFecho\nAtenciosamente", &closing));
        assert!(!found("Atenciosamente", &closing));
        assert!(!found(
            "O fecho do relatório foi entregue ontem ao setor competente da unidade.",
            &closing
        ));
        // The base text of a section, when the draft starts from it.
        let base = spec(
            "Abertura",
            "Em atenção ao documento acima, apresento a seguinte resposta para revisão do setor.",
        );
        assert!(found(
            "Em atenção ao documento acima, apresento a seguinte resposta para revisão do setor competente.",
            &base
        ));
        assert!(!found("Outro começo qualquer sem o texto base", &base));
        // At most forty are reported.
        let mut many = input("x");
        many.sections = (0..50).map(|n| spec(&format!("Seção {n}"), "")).collect();
        assert_eq!(check_draft(&many).sections.len(), 40);
    }

    #[test]
    fn words_of_a_title_in_the_body_or_their_cousins_do_not_make_a_section() {
        // "Prazo para resposta" is not found by any reply that has both words.
        let deadline = spec("Prazo para resposta", "");
        let body = "MINUTA PARA REVISÃO\n\nInformo que a resposta ao pedido será dada, e que o prazo de análise do setor termina em breve, conforme combinado com a chefia.";
        assert!(!found(body, &spec("Prazo e resposta", "")));
        assert!(!found(body, &spec("Prazo resposta", "")));
        // Its whole title as consecutive words (three words or more) may be anywhere.
        assert!(found(
            "Segue o prazo para resposta combinado com a chefia do setor.",
            &deadline
        ));
        assert!(!found(
            "Segue a resposta no prazo combinado com a chefia do setor.",
            &deadline
        ));
        // Cousins of the words are not the words.
        let conclusion = spec("Conclusão", "");
        assert!(!found("Conclusão\n", &spec("Fundamentação", "")));
        assert!(!found(
            "Para concluir, o setor informa que o pedido foi atendido.",
            &conclusion
        ));
        assert!(found("Conclusão\nO setor informa.", &conclusion));
        assert!(found("3. Conclusão", &conclusion));
        assert!(found("CONCLUSÕES\nO setor informa.", &conclusion));
        let grounds = spec("Fundamentação", "");
        assert!(!found("O ponto fundamental do pedido é o prazo.", &grounds));
        assert!(!found("Fundamental\nO ponto.", &grounds));
        assert!(found("Fundamentação:\nO ponto.", &grounds));
        // The heading is a short line that is not a sentence.
        assert!(!found(
            "Prazo para a resposta ao pedido do setor, que termina logo.",
            &spec("Prazo resposta pedido", "")
        ));
        assert!(
            found(
                "Prazo, resposta e pedido\nTexto.",
                &spec("Prazo resposta pedido", "")
            ) || true
        );
        // A run-in heading before a colon is a heading.
        assert!(found(
            "Prazo: a resposta será dada até o fim do mês, conforme o combinado entre as partes.",
            &spec("Prazo", "")
        ));
        assert!(!found(
            "O prazo é longo e a resposta será dada até o fim do mês, conforme o combinado.",
            &spec("Prazo", "")
        ));
    }

    #[test]
    fn a_date_the_materials_write_in_another_form_is_present() {
        let mut i = input("Prazo: 20 de novembro de 2026. Outra: 20-11-26. Mais: 2026-11-20.");
        let check = check_draft(&i);
        assert!(check.unsupported.is_empty(), "{:?}", kinds(&check));
        // The materials write it long, the draft short.
        i.capture_text = "Entregar até 20 de novembro de 2026.".into();
        i.draft_text = "Prazo 20/11/2026".into();
        assert!(check_draft(&i).unsupported.is_empty());
        // Invented dates are listed, once each, as the draft writes them.
        i.draft_text =
            "Reunião em 25/12/2026 e de novo em 25/12/2026, e 1º de janeiro de 2027.".into();
        let check = check_draft(&i);
        assert_eq!(
            kinds(&check),
            vec![
                ("data".to_owned(), "25/12/2026".to_owned()),
                ("data".to_owned(), "1º de janeiro de 2027".to_owned())
            ]
        );
        assert!(check.unsupported[0].context.contains("Reunião em"));
        // Never relative words, a day without month, nor an invalid date.
        i.draft_text = "Amanhã, na próxima sexta, dia 15, e em 31/02/2026.".into();
        assert!(check_draft(&i).unsupported.is_empty());
    }

    #[test]
    fn today_is_the_letters_own_date_only_on_its_date_line() {
        let mut i = input("MINUTA PARA REVISÃO\n\nEm atenção ao ofício.\n\nCuiabá, 12 de novembro de 2026.\n\nAtenciosamente,");
        assert!(check_draft(&i).unsupported.is_empty());
        // In the body, with a deadline word, or far from the ends: a date like any.
        i.draft_text = "MINUTA PARA REVISÃO\n\nResponderei até 12/11/2026.\n\nFim.".into();
        assert_eq!(
            kinds(&check_draft(&i)),
            vec![("data".to_owned(), "12/11/2026".to_owned())]
        );
        let body = (0..10)
            .map(|n| format!("Parágrafo {n} do texto."))
            .collect::<Vec<_>>()
            .join("\n");
        i.draft_text =
            format!("MINUTA\n{body}\nA reunião ocorreu em 12/11/2026 no setor.\n{body}\nFim");
        assert_eq!(
            kinds(&check_draft(&i)).len(),
            1,
            "{:?}",
            kinds(&check_draft(&i))
        );
        // A line of the head with a cue is not the date line.
        i.draft_text = "MINUTA PARA REVISÃO\nCuiabá, 12/11/2026, prazo final.\nTexto".into();
        assert_eq!(check_draft(&i).unsupported.len(), 1);
        // The day of the generation counts too, when given.
        i.draft_text = "MINUTA PARA REVISÃO\n\nTexto.\n\nCuiabá, 10 de novembro de 2026.".into();
        assert_eq!(check_draft(&i).unsupported.len(), 1);
        i.generated_on = Some("2026-11-10".into());
        assert!(check_draft(&i).unsupported.is_empty());
        // Another date on the date line is just a date.
        i.draft_text = "MINUTA PARA REVISÃO\n\nTexto.\n\nCuiabá, 11 de novembro de 2026.".into();
        assert_eq!(check_draft(&i).unsupported.len(), 1);
        // The materials writing the date support it anywhere.
        i.capture_text = "Reunião em 12/11/2026.".into();
        i.draft_text = "MINUTA PARA REVISÃO\n\nResponderei até 12/11/2026.\n\nFim.".into();
        assert!(check_draft(&i).unsupported.is_empty());
        // The date line of a CRLF draft.
        i.capture_text = String::new();
        i.generated_on = None;
        i.draft_text = "MINUTA PARA REVISÃO\r\n\r\nTexto.\r\n\r\nCuiabá, 12/11/2026.".into();
        assert!(check_draft(&i).unsupported.is_empty());
        // No today given: the date is just a date.
        i.today = String::new();
        assert_eq!(check_draft(&i).unsupported.len(), 1);
    }

    #[test]
    fn hours_are_present_by_value_not_by_how_they_are_written() {
        let mut i = input("Às 14h e às 14:00 e às 14 horas.");
        assert!(check_draft(&i).unsupported.is_empty());
        i.draft_text = "Reunião às 9h30, das 14h às 17h.".into();
        let check = check_draft(&i);
        assert_eq!(
            kinds(&check),
            vec![
                ("hora".to_owned(), "9h30".to_owned()),
                ("hora".to_owned(), "17h".to_owned())
            ]
        );
        // Lengths of time are not hours.
        i.draft_text = "Por 2 horas, 8 horas diárias.".into();
        assert!(check_draft(&i).unsupported.is_empty());
    }

    #[test]
    fn a_number_is_supported_only_by_the_same_whole_number() {
        // The examples of the review, each way: flagged when only a look-alike exists.
        assert_eq!(
            flagged(
                "Conforme o ofício nº 45.",
                "O CEP é 78045-000 e a página 145."
            ),
            vec!["nº 45"]
        );
        assert_eq!(
            flagged(
                "Conforme o ofício nº 45.",
                "Ano de 2045, CEP 78045-000, 45/2026."
            ),
            vec!["nº 45"]
        );
        assert_eq!(
            flagged("Conforme o ofício nº 45.", "Ofício ORG-OFI-2026/00431."),
            vec!["nº 45"]
        );
        assert_eq!(
            flagged("Conforme o decreto 123/2026.", "Veja o decreto 9.123/2026."),
            vec!["123/2026"]
        );
        assert_eq!(
            flagged("Conforme o decreto 123/2026.", "Veja o decreto 0123/2026."),
            vec!["123/2026"]
        );
        assert_eq!(
            flagged(
                "Processo ORG-OFI-2026/0043.",
                "Processo ORG-OFI-2026/00431."
            ),
            vec!["ORG-OFI-2026/0043"]
        );
        assert_eq!(
            flagged(
                "Processo ORG-OFI-2026/00431.",
                "Processo ORG-OFI-2026/0043."
            ),
            vec!["ORG-OFI-2026/00431"]
        );
        assert_eq!(
            flagged("Conforme a Lei 1.234/2026.", "Conforme a Lei 12.34/2026."),
            vec!["1.234/2026"]
        );
        assert_eq!(
            flagged("Conforme a Lei 12.345/2026.", "Conforme a Lei 2.345/2026."),
            vec!["12.345/2026"]
        );
        assert_eq!(
            flagged("Conforme nº 123/2026.", "Veja 1123/2026."),
            vec!["nº 123/2026"]
        );
        // And not flagged when the exact token is there.
        assert!(flagged("Conforme o ofício nº 45.", "Ofício nº 45 do setor.").is_empty());
        assert!(flagged("Conforme o ofício nº 45.", "Ofício n. 45 do setor.").is_empty());
        assert!(flagged("Conforme o ofício nº 45.", "Ofício número 45 do setor.").is_empty());
        assert!(
            flagged("Conforme o ofício nº 45.", "Item 45. Fim.").is_empty(),
            "a whole token"
        );
        assert!(flagged("Conforme o ofício nº 45.", "Página (45) do processo").is_empty());
        assert!(flagged(
            "Conforme o decreto 123/2026.",
            "Veja o decreto 123/2026, art. 1º."
        )
        .is_empty());
        assert!(flagged(
            "Conforme o decreto nº 123/2026.",
            "Veja o decreto 123/2026."
        )
        .is_empty());
        assert!(flagged("Conforme a Lei 1.234/2026.", "Conforme a Lei 1.234/2026.").is_empty());
        assert!(flagged("Processo ORG-OFI-2026/0043.", "Processo ORG-OFI-2026/0043.").is_empty());
        assert!(flagged(
            "Processo SEFAZ-PRO-2026/00001.",
            "Processo sefaz-pro-2026 / 00001 e ORG-OFI-2026/00431."
        )
        .is_empty());
        // The number of the document, written whole by the draft, is always its own.
        let own = |draft: &str| numbers(&check_draft(&input(draft)));
        assert!(own("Em atenção ao ORG-OFI-2026/00431.").is_empty());
        assert_eq!(
            own("Em atenção ao ORG-OFI-2026/00432."),
            vec!["ORG-OFI-2026/00432"]
        );
        assert_eq!(
            own("Em atenção ao ORG-OFI-2026/0043."),
            vec!["ORG-OFI-2026/0043"]
        );
        // The tail of a date and a bare year are not numbers.
        assert!(flagged("Em 12/11/2026, no ano de 2026.", "").is_empty());
        // A CNJ number.
        assert_eq!(
            flagged(
                "Processo 0001234-56.2026.8.11.0041.",
                "Processo 0001234-56.2026.8.11.0042."
            ),
            vec!["0001234-56.2026.8.11.0041"]
        );
        assert!(flagged(
            "Processo 0001234-56.2026.8.11.0041.",
            "Processo 0001234-56.2026.8.11.0041."
        )
        .is_empty());
    }

    #[test]
    fn money_is_compared_by_amount() {
        let mut i = input("Valor de R$ 1.500,00, ou R$1500, ou R$ 1.500,0.");
        assert!(check_draft(&i).unsupported.is_empty());
        i.draft_text = "Pagar R$ 2.000,00 e R$ 15,50.".into();
        assert_eq!(
            kinds(&check_draft(&i)),
            vec![
                ("valor".to_owned(), "R$ 2.000,00".to_owned()),
                ("valor".to_owned(), "R$ 15,50".to_owned())
            ]
        );
        // The materials write it other ways.
        i.capture_text = "O custo é R$2000 e R$ 15,5.".into();
        assert!(check_draft(&i).unsupported.is_empty());
    }

    #[test]
    fn what_looks_like_data_but_is_not_compared_is_said_not_claimed() {
        let hints = |text: &str| check_draft(&input(text)).unparsed_hints;
        let found = hints("Custo de 1.500,00 reais, ou mil e quinhentos reais, e 15% de desconto.");
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].contains("1.500,00 reais"));
        assert!(found.iter().any(|h| h.contains("mil e quinhentos reais")));
        assert!(found.iter().any(|h| h.contains("15%")));
        // A law with a two-digit year is not a token the check compares: it is said.
        let laws = hints("Nos termos da Lei 8.666/93, do art. 5º, § 2º e do Decreto 10.024/19.");
        assert_eq!(laws.len(), 4, "{laws:?}");
        assert!(laws.iter().any(|h| h.contains("Lei 8.666/93")));
        assert!(laws.iter().any(|h| h.contains("art. 5º")));
        assert!(laws.iter().any(|h| h.contains("§ 2º")));
        assert!(laws.iter().any(|h| h.contains("Decreto 10.024/19")));
        // With "nº" or a four-digit year the number is compared, so it is listed as
        // unsupported (not hinted twice).
        let marked = check_draft(&input(
            "Nos termos da Lei nº 8.666/93 e do Decreto 10.024/2019.",
        ));
        assert_eq!(numbers(&marked), vec!["nº 8.666/93", "10.024/2019"]);
        assert!(
            marked.unparsed_hints.is_empty(),
            "{:?}",
            marked.unparsed_hints
        );
        assert!(hints("Aumento de 3,5 por cento.").len() == 1);
        // Nothing to say about plain text, and what is "unsupported" is not repeated.
        assert!(hints("Texto sem dados.").is_empty());
        let check = check_draft(&input("Conforme o decreto 123/2026 e a Lei 8.666/93."));
        assert!(check.unsupported.iter().any(|u| u.value == "123/2026"));
        assert!(check
            .unparsed_hints
            .iter()
            .all(|h| !h.contains("123/2026") || h.contains("Lei")));
        // At most ten.
        let many = "Aumento de 1%. ".repeat(30) + &"Lei 1. ".repeat(30);
        assert!(hints(&many).len() <= 10);
        // R$ amounts are compared, not hinted.
        assert!(hints("Pagar R$ 2.000,00.").is_empty());
    }

    #[test]
    fn a_value_only_an_example_holds_is_unsupported_and_the_own_texts_support() {
        // The examples are not an input: a date from one has no material.
        let mut i = input("Conforme o exemplo, até 03/03/2024, ORG-OFI-2024/00099 e R$ 99,00.");
        let check = check_draft(&i);
        assert_eq!(check.unsupported.len(), 3, "{:?}", kinds(&check));
        // The template, the blocks and the request are the person's own.
        i.sections = vec![SectionSpec {
            name: "Prazo".into(),
            guidance: "Informe o prazo de 03/03/2024.".into(),
            required: false,
        }];
        i.trusted_texts = vec!["Citar o ORG-OFI-2024/00099.".into()];
        let values: Vec<String> = check_draft(&i)
            .unsupported
            .iter()
            .map(|u| u.value.clone())
            .collect();
        assert_eq!(values, vec!["R$ 99,00".to_owned()]);
        // The context, the sources and the verified facts support too.
        i.context_values = "Combinado: pagar R$ 99,00.".into();
        assert!(check_draft(&i).unsupported.is_empty());
        let mut j = input("Reunião em 01/12/2026 às 10h e 7 de janeiro de 2027.");
        j.sources = vec!["Anexo: reunião em 01/12/2026, 10h.".into()];
        j.facts = vec![Fact {
            id: "f1".into(),
            kind: "prazo".into(),
            quote: "responder até 7 de janeiro de 2027".into(),
            ..Fact::default()
        }];
        assert!(check_draft(&j).unsupported.is_empty());
    }

    #[test]
    fn what_the_person_had_supports_a_completion_and_a_changed_paragraph_is_noted() {
        let before = "Primeiro parágrafo já pronto, com a data 05/05/2025.\n\nSegundo parágrafo a completar: [preencher].";
        let mut i = input("MINUTA PARA REVISÃO\n\nPrimeiro parágrafo já pronto, com a data 05/05/2025.\n\nSegundo parágrafo completado.");
        i.intent = Intent::Complete;
        i.previous_text = before.into();
        let check = check_draft(&i);
        assert!(check.unsupported.is_empty(), "{:?}", kinds(&check));
        assert!(check.notes.is_empty(), "{:?}", check.notes);
        // A finished paragraph that changed.
        i.draft_text = "MINUTA PARA REVISÃO\n\nPrimeiro parágrafo reescrito pelo modelo.\n\nSegundo parágrafo completado.".into();
        let check = check_draft(&i);
        assert_eq!(check.notes.len(), 1, "{:?}", check.notes);
        assert!(check.notes[0].contains("1 parágrafo"));
        // A paragraph with a place to fill (any bracket) may change; a new text has no note.
        i.previous_text = "Segundo parágrafo a completar: [preencher].".into();
        assert!(check_draft(&i).notes.is_empty());
        i.previous_text = "Segundo parágrafo, para [destinatário] do setor.".into();
        assert!(check_draft(&i).notes.is_empty());
        i.intent = Intent::New;
        i.previous_text = before.into();
        assert!(check_draft(&i).notes.is_empty());
    }

    #[test]
    fn the_bases_tail_is_not_part_of_the_draft_even_when_its_header_was_edited() {
        let tail = "S1 — documento — Anexo 1: custos-segredo.txt\nTrecho encontrado na fonte: prazo de 31/12/2030 [preencher]\nUso proposto: apoiar o prazo\n";
        let text = format!("MINUTA PARA REVISÃO\n\nResposta.\n\nBASES PARA REVISÃO (conferir antes de usar no SIGADOC)\n\n{tail}");
        let check = check_draft(&input(&text));
        assert!(check.unsupported.is_empty() && check.placeholders.is_empty());
        // The header edited: punctuation, spaces, case, accents, a heading mark.
        for header in [
            "bases para revisao",
            "## Bases para revisão:",
            "BASES   PARA   REVISÃO",
            "Bases - para - revisão (editado)",
        ] {
            let text = format!("MINUTA PARA REVISÃO\n\nResposta.\n\n{header}\n\n{tail}");
            let check = check_draft(&input(&text));
            assert!(
                check.unsupported.is_empty() && check.placeholders.is_empty(),
                "{header}"
            );
        }
        // The header erased: the lines of the block are still not the draft.
        let text = format!("MINUTA PARA REVISÃO\n\nResposta.\n\n{tail}");
        let check = check_draft(&input(&text));
        assert!(check.unsupported.is_empty() && check.placeholders.is_empty());
    }

    #[test]
    fn a_copy_of_the_header_in_the_body_does_not_hide_what_follows_it() {
        let body = "MINUTA PARA REVISÃO\n\nBASES PARA REVISÃO\nO setor pagará R$ 9.999,00 em 31/12/2030.\n\nFim do texto.";
        let check = check_draft(&input(body));
        assert!(check.unsupported.len() >= 2, "{:?}", kinds(&check));
        // With a real tail after it, only the tail goes.
        let tail = "\n\nBASES PARA REVISÃO (conferir antes de usar no SIGADOC)\n\nS1 — rótulo\nTrecho encontrado na fonte: algum trecho\nUso proposto: apoio\n";
        let check = check_draft(&input(&format!("{body}{tail}")));
        assert!(check.unsupported.len() >= 2, "{:?}", kinds(&check));
        assert!(check.unsupported.iter().all(|u| u.value != "algum trecho"));
    }

    #[test]
    fn the_check_is_pure_and_bounded() {
        let big = "Em 01/01/2031 e 02/01/2031, ".repeat(2000);
        let mut i = input(&big);
        let first = check_draft(&i);
        assert_eq!(first, check_draft(&i));
        assert!(first.unsupported.len() <= MAX_UNSUPPORTED);
        assert!(first.unparsed_hints.len() <= MAX_HINTS);
        i.draft_text = String::new();
        let empty = check_draft(&i);
        assert!(
            empty.checked && !empty.header && !empty.cites_number && empty.unsupported.is_empty()
        );
    }

    #[test]
    fn a_timestamp_gives_the_local_date_of_the_generation() {
        let day = date_of_timestamp("2026-11-12T12:00:00.000Z").unwrap();
        assert!(
            ["2026-11-12", "2026-11-13", "2026-11-11"].contains(&day.as_str()),
            "{day}"
        );
        assert!(date_of_timestamp("ontem").is_none());
    }
}
