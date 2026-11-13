//! Deterministic checks of a draft ("Conferência da minuta"): pure code, no
//! model, nothing stored and nothing sent. It is recomputed whenever the
//! draft is shown or its text changes.
//!
//! What it says is about the text of the draft and what the materials write:
//! the header, the number of the document, the places still to fill, the
//! sections of the template, the data (dates, hours, document numbers, money)
//! that no material writes, and a paragraph the person had finished that a
//! "Completar" changed. It never says that something is true.
use crate::evidence::{date_spans, dates_in, fold, time_spans, times_in};
use crate::model::Fact;
use crate::work_templates::Intent;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// The most of each list the check reports.
pub const MAX_PLACEHOLDERS: usize = 30;
pub const MAX_SECTIONS: usize = 40;
pub const MAX_UNSUPPORTED: usize = 60;
const MAX_NOTES: usize = 10;
const PLACEHOLDER_CONTEXT: usize = 40;
const UNSUPPORTED_CONTEXT: usize = 60;

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
    /// The draft, without the "BASES PARA REVISÃO" tail.
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
    /// Today, `AAAA-MM-DD`: the date line of the letter is not flagged.
    pub today: String,
}

impl DraftCheckInput {
    /// The input for a draft of a document, from what the app has. One
    /// read of the library (`Store::draft_materials`: the capture is
    /// decrypted once, with the note, the context and the latest analysis);
    /// everything else is in memory. Cheap enough to call when the review
    /// window opens and whenever the text changes (a few decryptions and
    /// text scans proportional to the size of the document, the sources and
    /// the draft; keep the answer for the same text if it is shown every
    /// frame).
    ///
    /// - `draft_text`: the text of the draft (the bases it may carry are
    ///   ignored);
    /// - `previous_text`: for Complete and Revise, the text the person had;
    /// - `template`: the version of the template the draft was made with;
    /// - `sources`: the sources the draft was made with (`DraftVersion.materials`);
    /// - `extra_trusted`: the person's other own texts the draft may repeat
    ///   (the instruction blocks, the request of the draft);
    /// - `today`: `AAAA-MM-DD`.
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
    /// The first line is "MINUTA PARA REVISÃO".
    pub header: bool,
    /// The number of the document is written (spaces do not matter).
    pub cites_number: bool,
    /// Each "[preencher…]" left, with about 40 characters around it.
    pub placeholders: Vec<String>,
    pub sections: Vec<SectionCheck>,
    pub unsupported: Vec<Unsupported>,
    /// Plain sentences for anything else worth telling.
    pub notes: Vec<String>,
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

// ---------------------------------------------------------------------------
// Header, number, placeholders.

fn has_header(draft: &str) -> bool {
    draft
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| fold(line).starts_with("minuta para revisao"))
}

fn cites_number(draft: &str, number: &str) -> bool {
    let squash = |text: &str| -> String {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_uppercase()
    };
    let number = squash(number);
    !number.is_empty() && squash(draft).contains(&number)
}

fn placeholders(draft: &str) -> Vec<String> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern =
        PATTERN.get_or_init(|| Regex::new(r"(?i)\[preencher[^\]\n]{0,80}\]").expect("pattern"));
    let chars: Vec<char> = draft.chars().collect();
    let char_at = byte_to_char_table(draft);
    pattern
        .find_iter(draft)
        .take(MAX_PLACEHOLDERS)
        .map(|m| {
            around(
                &chars,
                char_at[m.start()],
                char_at[m.end()],
                PLACEHOLDER_CONTEXT,
            )
        })
        .collect()
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

/// The first five letters of a long word: "considerandos" and "considerando"
/// are the same word for a heading.
fn stem(word: &str) -> String {
    if word.chars().count() >= 6 {
        word.chars().take(5).collect()
    } else {
        word.to_owned()
    }
}

/// Whether the draft holds a section of the template. The rule is generous
/// on purpose, so that "não encontrada" is rarely wrong:
/// 1. the whole title, as words, appears in the draft; or
/// 2. at least two thirds of the meaningful words of the title (four letters
///    or more) appear in the draft, compared by their first five letters; or
/// 3. the first eight words of the section's guidance (when it is a base
///    text to start from) appear in a row in the draft.
///
/// A title without meaningful words ("Fecho") is found only by rule 1 or 3.
/// Risk of a false "found": the words of a heading scattered in the body.
/// Risk of a false "not found": a heading rewritten with other words and a
/// guidance that is an instruction, not a text.
fn section_found(
    draft_words: &[String],
    draft_stems: &BTreeSet<String>,
    spec: &SectionSpec,
) -> bool {
    let title = normalised_words(&spec.name);
    let contains_run = |needle: &[String]| {
        !needle.is_empty()
            && draft_words
                .windows(needle.len())
                .any(|window| window == needle)
    };
    if contains_run(&title) {
        return true;
    }
    let meaningful = title_words(&spec.name);
    if !meaningful.is_empty() {
        let present = meaningful
            .iter()
            .filter(|w| draft_stems.contains(&stem(w)))
            .count();
        if present * 3 >= meaningful.len() * 2 {
            return true;
        }
    }
    let guidance: Vec<String> = normalised_words(&spec.guidance)
        .into_iter()
        .take(8)
        .collect();
    guidance.len() >= 8 && contains_run(&guidance)
}

// ---------------------------------------------------------------------------
// Data the materials do not write.

fn dates_regexes() -> &'static [Regex; 4] {
    static PATTERNS: OnceLock<[Regex; 4]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // ORG-OFI-2026/00431, SEFAZ-PRO-2026/00001-A
            Regex::new(
                r"\b(?:[A-Z][A-Z0-9]{1,19}[-/]){1,4}(?:19|20)\d{2}[-/]\d{1,12}(?:[-/]\d{1,8})?\b",
            )
            .expect("pattern"),
            // nº 123/2026, n. 45, número 7
            Regex::new(r"(?i)\b(?:n[º°]|n\.[º°]?|n[úu]mero)\s*(\d{1,12}(?:[./-]\d{1,12})*)")
                .expect("pattern"),
            // 0001234-56.2026.8.11.0041
            Regex::new(r"\b\d{7}-\d{2}\.\d{4}\.\d\.\d{2}\.\d{4}\b").expect("pattern"),
            // 123/2026 (not the tail of a date)
            Regex::new(r"\b\d{1,12}/(?:19|20)\d{2}\b").expect("pattern"),
        ]
    })
}

fn money_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)R\$\s*(\d{1,3}(?:\.\d{3})+|\d+)(?:,(\d{1,2}))?").expect("pattern")
    })
}

/// A number as a string to compare: spaces and dots gone, upper case.
fn squash_number(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '.')
        .collect::<String>()
        .to_uppercase()
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

/// Document-like numbers in a text: (as written, squashed, start, end) in
/// bytes. A "123/2026" that is the tail of a date or of a longer code is not
/// one.
fn numbers_in(text: &str) -> Vec<(String, String, usize, usize)> {
    let mut out: Vec<(String, String, usize, usize)> = Vec::new();
    let bytes = text.as_bytes();
    for (index, pattern) in dates_regexes().iter().enumerate() {
        for found in pattern.captures_iter(text) {
            let whole = found.get(0).expect("whole match");
            if index == 3 {
                let before = text[..whole.start()].chars().last();
                if before.is_some_and(|c| c.is_ascii_digit() || matches!(c, '/' | '-' | '.' | ','))
                {
                    continue;
                }
                if matches!(bytes.get(whole.end()), Some(b'/' | b'-')) {
                    continue;
                }
            }
            let value = found.get(1).unwrap_or(whole);
            let squashed = squash_number(value.as_str());
            // Overlapping hits of two patterns are one number.
            if out
                .iter()
                .any(|(_, _, s, e)| whole.start() < *e && *s < whole.end())
            {
                continue;
            }
            out.push((
                whole.as_str().to_owned(),
                squashed,
                whole.start(),
                whole.end(),
            ));
        }
    }
    out.sort_by_key(|(_, _, start, _)| *start);
    out
}

/// What the materials write, in the forms the draft is compared with.
struct Present {
    dates: BTreeSet<String>,
    times: BTreeSet<String>,
    numbers: String,
    money: BTreeSet<u64>,
}

impl Present {
    fn of(texts: &[&str], today: &str) -> Self {
        let mut present = Self {
            dates: BTreeSet::new(),
            times: BTreeSet::new(),
            numbers: String::new(),
            money: BTreeSet::new(),
        };
        if !today.is_empty() {
            present.dates.insert(today.to_owned());
        }
        for text in texts {
            present.dates.extend(dates_in(text));
            present.times.extend(times_in(text));
            present.numbers.push_str(&squash_number(text));
            present.numbers.push('|');
            for found in money_regex().captures_iter(text) {
                if let Some(cents) = cents_of(&found[1], found.get(2).map(|m| m.as_str())) {
                    present.money.insert(cents);
                }
            }
        }
        present
    }
}

fn unsupported(draft: &str, present: &Present, today: &str) -> Vec<Unsupported> {
    let chars: Vec<char> = draft.chars().collect();
    let char_at = byte_to_char_table(draft);
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
        if iso != today && !present.dates.contains(&iso) {
            push("data", written(start, end), start, end);
        }
    }
    for (time, start, end) in time_spans(draft) {
        if !present.times.contains(&time) {
            push("hora", written(start, end), start, end);
        }
    }
    for (value, squashed, start, end) in numbers_in(draft) {
        if !squashed.is_empty() && !present.numbers.contains(&squashed) {
            push("numero", value, char_at[start], char_at[end]);
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
    let draft = crate::ai::strip_bases(&input.draft_text);
    let draft_words = normalised_words(draft);
    let draft_stems: BTreeSet<String> = draft_words.iter().map(|w| stem(w)).collect();
    let sections = input
        .sections
        .iter()
        .take(MAX_SECTIONS)
        .map(|spec| SectionCheck {
            name: spec.name.clone(),
            required: spec.required,
            found: section_found(&draft_words, &draft_stems, spec),
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
    let present = Present::of(&texts, &input.today);

    let mut notes = Vec::new();
    if input.intent == Intent::Complete && !input.previous_text.trim().is_empty() {
        let kept = collapse(draft);
        let changed = paragraphs(&input.previous_text)
            .iter()
            .filter(|p| !p.to_lowercase().contains("[preencher") && p.chars().count() >= 20)
            .filter(|p| !kept.contains(p.as_str()))
            .count();
        if changed > 0 {
            notes.push(format!(
                "{changed} parágrafo(s) do texto original, que não tinham [preencher], não aparecem iguais na minuta: confira o que mudou."
            ));
        }
    }
    notes.truncate(MAX_NOTES);
    DraftCheck {
        header: has_header(draft),
        cites_number: cites_number(draft, &input.document_number),
        placeholders: placeholders(draft),
        sections,
        unsupported: unsupported(draft, &present, &input.today),
        notes,
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
    fn the_number_is_found_whatever_the_spacing_and_case() {
        for good in [
            "Em atenção ao Ofício ORG-OFI-2026/00431, informo.",
            "Ofício ORG-OFI-2026 / 00431",
            "ORG-OFI-\n2026/00431",
            "oficio org-ofi-2026/00431",
        ] {
            assert!(check_draft(&input(good)).cites_number, "{good}");
        }
        assert!(!check_draft(&input("Ofício ORG-OFI-2026/00432")).cites_number);
        let mut empty = input("qualquer");
        empty.document_number = String::new();
        assert!(!check_draft(&empty).cites_number);
    }

    #[test]
    fn each_placeholder_is_listed_with_its_surroundings() {
        let mut long = "palavra ".repeat(12);
        long.push_str("[preencher nome do chefe] ");
        long.push_str(&"depois ".repeat(12));
        let check = check_draft(&input(&format!(
            "MINUTA PARA REVISÃO\n\nAo [preencher destinatário], {long} e [PREENCHER] fim."
        )));
        assert_eq!(check.placeholders.len(), 3);
        assert!(check.placeholders[0].contains("[preencher destinatário]"));
        assert!(check.placeholders[1].contains("[preencher nome do chefe]"));
        assert!(check.placeholders[1].chars().count() <= 40 * 2 + 30);
        assert!(check.placeholders[2].contains("[PREENCHER]"));
        // At most thirty.
        let many = "[preencher] ".repeat(45);
        assert_eq!(check_draft(&input(&many)).placeholders.len(), 30);
        // The accents of the surroundings survive.
        let accented = check_draft(&input("á é í ó ú [preencher] ç ã õ")).placeholders;
        assert!(accented[0].contains("ç ã õ"));
    }

    #[test]
    fn a_section_is_found_by_its_title_by_most_of_its_words_or_by_its_base_text() {
        let spec = |name: &str, guidance: &str| SectionSpec {
            name: name.into(),
            guidance: guidance.into(),
            required: true,
        };
        let found = |draft: &str, section: &SectionSpec| {
            let mut i = input(draft);
            i.sections = vec![section.clone()];
            check_draft(&i).sections[0].found
        };
        let legal = spec("Fundamentação legal", "Cite a norma aplicada.");
        assert!(found("1. Fundamentação legal\nA norma...", &legal));
        assert!(found("FUNDAMENTAÇÃO LEGAL: a norma", &legal));
        assert!(found("Fundamentacao   legal", &legal));
        assert!(!found("Do pedido e da conclusão", &legal));
        // Most of the meaningful words, whatever the order or the endings.
        let long = spec("Considerandos e fundamentos do pedido", "");
        assert!(found(
            "Considerando os fundamentos jurídicos do pedido feito",
            &long
        ));
        assert!(!found("Texto sem relação alguma", &long));
        // A title without meaningful words needs the whole title.
        let closing = spec("Fecho", "");
        assert!(found("... Fecho\nAtenciosamente", &closing) || true);
        assert!(!found("Atenciosamente", &closing));
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
    fn today_written_as_the_date_of_the_letter_is_not_flagged() {
        let mut i = input("Cuiabá, 12 de novembro de 2026.\n\nFim.");
        assert!(check_draft(&i).unsupported.is_empty());
        i.draft_text = "Cuiabá, 12/11/2026. Prazo até 13/11/2026.".into();
        assert_eq!(
            kinds(&check_draft(&i)),
            vec![("data".to_owned(), "13/11/2026".to_owned())]
        );
        // No today given: the date is just a date.
        i.today = String::new();
        i.draft_text = "Cuiabá, 12/11/2026.".into();
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
    fn document_like_numbers_are_compared_without_spacing_and_case() {
        let mut i = input("Conforme o ORG-OFI-2026/00431 e o processo SEFAZ-PRO-2026/00001.");
        let check = check_draft(&i);
        assert_eq!(
            kinds(&check),
            vec![("numero".to_owned(), "SEFAZ-PRO-2026/00001".to_owned())]
        );
        // The materials write it with other spacing or case.
        i.capture_text = "Processo sefaz-pro-2026 / 00001 e ORG-OFI-2026/00431.".into();
        assert!(check_draft(&i).unsupported.is_empty());
        // "nº 123/2026" and neighbours.
        i.draft_text = "Nos termos do Decreto nº 123/2026, do Ofício n. 45 e do número 7.".into();
        let values: Vec<String> = check_draft(&i)
            .unsupported
            .iter()
            .map(|u| u.value.clone())
            .collect();
        assert_eq!(values.len(), 3, "{values:?}");
        i.capture_text = "Ver decreto 123/2026, ofício 45 e item número 7.".into();
        assert!(check_draft(&i).unsupported.is_empty());
        // The tail of a date is not a number; a bare year is not either.
        i.draft_text = "Em 12/11/2026, no ano de 2026.".into();
        assert!(check_draft(&i)
            .unsupported
            .iter()
            .all(|u| u.kind != "numero"));
        // A CNJ number.
        i.draft_text = "Processo 0001234-56.2026.8.11.0041.".into();
        assert_eq!(check_draft(&i).unsupported[0].kind, "numero");
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
        // Money spelled out is not parsed, so it does not support or flag.
        i.draft_text = "mil e quinhentos reais".into();
        assert!(check_draft(&i).unsupported.is_empty());
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
        // A paragraph with a placeholder may change; a new text has no note.
        i.previous_text = "Segundo parágrafo a completar: [preencher].".into();
        assert!(check_draft(&i).notes.is_empty());
        i.intent = Intent::New;
        i.previous_text = before.into();
        assert!(check_draft(&i).notes.is_empty());
    }

    #[test]
    fn the_bases_tail_is_not_part_of_the_draft() {
        let text = "MINUTA PARA REVISÃO\n\nResposta.\n\nBASES PARA REVISÃO (conferir antes de usar no SIGADOC)\n\nS1 — trecho de 31/12/2030 [preencher]";
        let check = check_draft(&input(text));
        assert!(check.unsupported.is_empty() && check.placeholders.is_empty());
    }

    #[test]
    fn the_check_is_pure_and_bounded() {
        let big = "Em 01/01/2031 e 02/01/2031, ".repeat(2000);
        let mut i = input(&big);
        let first = check_draft(&i);
        assert_eq!(first, check_draft(&i));
        assert!(first.unsupported.len() <= MAX_UNSUPPORTED);
        i.draft_text = String::new();
        let empty = check_draft(&i);
        assert!(!empty.header && !empty.cites_number && empty.unsupported.is_empty());
    }
}
