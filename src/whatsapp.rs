//! Messages copied from a WhatsApp conversation: the reader of the copied
//! text and what a record of the kind "whatsapp" holds. Pure: no window.
//!
//! The same reader exists in JavaScript (`extension/whatsapp-core.js`) with
//! the same rules and the same test vectors. The copied text is untrusted:
//! it is parsed by hand (no backtracking), only ever shown as text, and
//! never interpreted.
//!
//! Rules (see the contract of the 0.32 work):
//! * a line starts a message when it begins with `[`, has a `]` within its
//!   first 48 characters and the text between them is a valid time and a
//!   valid date, in either order (`hh:mm[:ss]` with an optional AM/PM
//!   marker, `d/m/aa` or `d/m/aaaa`, separators `/`, `.` or `-`). The
//!   order is always day/month; a date that is not valid that way (month
//!   13, 31/02) does not start a message, it is never read as month/day;
//! * the author is what comes before the first `: ` (or a final `:`); a
//!   line without author after the bracket is a system line;
//! * the following lines belong to the message until the next line that
//!   starts one; lines before the first message are kept as one system line
//!   without a date.
use crate::manual::ManualFields;
use crate::model::{ImportReceipt, MANUAL_PENDING, MAX_CAPTURE_TEXT_BYTES};
use crate::storage::Store;
use anyhow::{bail, Result};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Most messages one record holds.
pub const MAX_MESSAGES: usize = 500;
/// Longest chat name kept (characters).
pub const MAX_CHAT_CHARS: usize = 200;
/// A message header (`[...]`) never needs more characters than this.
const HEADER_CHARS: usize = 48;

const ERR_NONE: &str = "Não encontrei mensagens do WhatsApp neste texto. Selecione as mensagens na conversa, copie e cole aqui.";
const ERR_COUNT: &str = "São mais de 500 mensagens. Cole a conversa em partes.";
const ERR_SIZE: &str =
    "O texto passa do limite de um registro (500 KB). Cole a conversa em partes.";

/// One message of the conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// Local time as WhatsApp showed it; `None` for the text found before
    /// the first message.
    pub when: Option<NaiveDateTime>,
    /// `None` for a system line ("X adicionou Y").
    pub author: Option<String>,
    /// The text; continuation lines are joined with `\n`.
    pub text: String,
}

/// What a paste holds, oldest message first (as copied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub messages: Vec<Message>,
    /// Authors, in order of first appearance.
    pub participants: Vec<String>,
    /// Time of the first and of the last dated message (by position).
    pub first: Option<NaiveDateTime>,
    pub last: Option<NaiveDateTime>,
}

fn is_mark(c: char) -> bool {
    matches!(
        c,
        '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// Line ends as `\n`, direction marks and control characters (except the
/// tab) removed.
fn normalise(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' | '\u{2028}' | '\u{2029}' => out.push('\n'),
            '\t' => out.push(c),
            c if is_mark(c) || c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The chat name as kept in the record: marks removed, controls and runs of
/// whitespace become one space, trimmed, the first 200 characters, trimmed
/// again (it never ends with a space).
pub fn clean_chat(chat: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for c in chat.chars().filter(|c| !is_mark(*c)) {
        if c.is_control() || c.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(c);
        }
    }
    let cut: String = out.chars().take(MAX_CHAT_CHARS).collect();
    cut.trim_end().to_owned()
}

// ---- the header of a message line ----

fn digits(c: &[char], p: &mut usize, min: usize, max: usize) -> Option<u32> {
    let start = *p;
    let mut value = 0u32;
    while *p < c.len() && *p - start < max && c[*p].is_ascii_digit() {
        value = value * 10 + c[*p].to_digit(10)?;
        *p += 1;
    }
    (*p - start >= min).then_some(value)
}

fn skip_space(c: &[char], p: &mut usize) {
    while *p < c.len() && c[*p].is_whitespace() {
        *p += 1;
    }
}

/// `a. m.`, `p.m.`, `AM`, `pm`…: `Some(true)` for the afternoon.
fn marker(c: &[char], p: &mut usize) -> Option<bool> {
    let mut q = *p;
    skip_space(c, &mut q);
    let pm = match c.get(q)?.to_ascii_lowercase() {
        'a' => false,
        'p' => true,
        _ => return None,
    };
    q += 1;
    // `a.m.`, `a. m.` (a space only after the first dot), `am`.
    if c.get(q) == Some(&'.') {
        q += 1;
        skip_space(c, &mut q);
    }
    if !c.get(q)?.eq_ignore_ascii_case(&'m') {
        return None;
    }
    q += 1;
    if c.get(q) == Some(&'.') {
        q += 1;
    }
    if c.get(q).is_some_and(|x| x.is_alphanumeric()) {
        return None;
    }
    *p = q;
    Some(pm)
}

fn time(c: &[char], p: &mut usize) -> Option<NaiveTime> {
    let hour = digits(c, p, 1, 2)?;
    if c.get(*p) != Some(&':') {
        return None;
    }
    *p += 1;
    let minute = digits(c, p, 2, 2)?;
    let mut second = 0;
    if c.get(*p) == Some(&':') {
        *p += 1;
        second = digits(c, p, 2, 2)?;
    }
    let hour = match marker(c, p) {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour % 12 + if pm { 12 } else { 0 }
        }
        None => hour,
    };
    NaiveTime::from_hms_opt(hour, minute, second)
}

fn date(c: &[char], p: &mut usize) -> Option<NaiveDate> {
    let sep = |p: &mut usize| -> Option<()> {
        matches!(c.get(*p), Some('/' | '.' | '-')).then(|| *p += 1)
    };
    let day = digits(c, p, 1, 2)?;
    sep(p)?;
    let month = digits(c, p, 1, 2)?;
    sep(p)?;
    let start = *p;
    let year = digits(c, p, 2, 4)?;
    let year = match *p - start {
        2 => 2000 + year,
        4 if (1900..=2100).contains(&year) => year,
        _ => return None,
    };
    NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)
}

/// The separator between the two halves: a comma and/or spaces, at least one.
fn between(c: &[char], p: &mut usize) -> Option<()> {
    let start = *p;
    skip_space(c, p);
    if c.get(*p) == Some(&',') {
        *p += 1;
    }
    skip_space(c, p);
    (*p > start).then_some(())
}

fn stamp(inner: &str) -> Option<NaiveDateTime> {
    let c: Vec<char> = inner.chars().collect();
    let finish = |p: &mut usize| {
        skip_space(&c, p);
        *p == c.len()
    };
    let mut p = 0;
    if let Some(t) = time(&c, &mut p) {
        if between(&c, &mut p).is_some() {
            if let Some(d) = date(&c, &mut p) {
                if finish(&mut p) {
                    return Some(d.and_time(t));
                }
            }
        }
    }
    let mut p = 0;
    let d = date(&c, &mut p)?;
    between(&c, &mut p)?;
    let t = time(&c, &mut p)?;
    finish(&mut p).then(|| d.and_time(t))
}

/// When `line` starts a message: its time and what follows the bracket.
fn header(line: &str) -> Option<(NaiveDateTime, &str)> {
    if !line.starts_with('[') {
        return None;
    }
    let end = line
        .char_indices()
        .skip(1)
        .take(HEADER_CHARS)
        .find(|(_, c)| *c == ']')?
        .0;
    Some((stamp(&line[1..end])?, &line[end + 1..]))
}

/// Author and first line of the text after the bracket.
fn author_and_text(rest: &str) -> (Option<String>, String) {
    let rest = rest.trim_start();
    if let Some(at) = rest.find(": ") {
        let author = rest[..at].trim();
        if !author.is_empty() {
            return (Some(author.to_owned()), rest[at + 2..].trim().to_owned());
        }
    } else if let Some(name) = rest.trim_end().strip_suffix(':') {
        let author = name.trim();
        if !author.is_empty() {
            return (Some(author.to_owned()), String::new());
        }
    }
    (None, rest.trim().to_owned())
}

/// Reads copied WhatsApp messages.
pub fn parse(copied: &str) -> Result<Conversation> {
    if copied.len() > 2 * MAX_CAPTURE_TEXT_BYTES {
        bail!(ERR_SIZE);
    }
    let text = normalise(copied);
    let mut messages: Vec<Message> = Vec::new();
    let mut preamble: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        if let Some((when, rest)) = header(line) {
            if messages.is_empty() {
                let lead = take_preamble(&mut preamble);
                if !lead.is_empty() {
                    messages.push(Message {
                        when: None,
                        author: None,
                        text: lead,
                    });
                }
            }
            if messages.len() >= MAX_MESSAGES {
                bail!(ERR_COUNT);
            }
            let (author, first_line) = author_and_text(rest);
            messages.push(Message {
                when: Some(when),
                author,
                text: first_line,
            });
        } else if let Some(last) = messages.last_mut() {
            last.text.push('\n');
            last.text.push_str(line.trim_end());
        } else {
            preamble.push(line.trim_end());
        }
    }
    if !messages.iter().any(|m| m.when.is_some()) {
        bail!(ERR_NONE);
    }
    for message in &mut messages {
        let kept = message.text.trim_end().len();
        message.text.truncate(kept);
    }
    let mut participants: Vec<String> = Vec::new();
    for author in messages.iter().filter_map(|m| m.author.as_ref()) {
        if !participants.contains(author) {
            participants.push(author.clone());
        }
    }
    let dated = || messages.iter().filter_map(|m| m.when);
    let conversation = Conversation {
        first: dated().next(),
        last: dated().next_back(),
        participants,
        messages,
    };
    if record_text(&conversation).len() > MAX_CAPTURE_TEXT_BYTES {
        bail!(ERR_SIZE);
    }
    Ok(conversation)
}

/// The lines found before the first message, without blank lines at the
/// ends, as one text.
fn take_preamble(lines: &mut Vec<&str>) -> String {
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    let text = match (first, last) {
        (Some(a), Some(b)) => lines[a..=b].join("\n"),
        _ => String::new(),
    };
    lines.clear();
    text
}

// ---- the record ----

/// `dd/mm/aaaa hh:mm`.
fn show(when: NaiveDateTime) -> String {
    when.format("%d/%m/%Y %H:%M").to_string()
}

/// The text of the record: one block per message, oldest first:
/// `[dd/mm/aaaa hh:mm] Nome: texto`, continuation lines unprefixed, a system
/// line as `[dd/mm/aaaa hh:mm] — texto`, the text before the first message
/// as `[sem data] — texto`. No line ends with a space.
///
/// EVERY continuation line of a message is written indented by four spaces
/// (blank ones stay empty), so in a stored record a line at column 0 that
/// starts with `[dd/mm/aaaa hh:mm]` or `[sem data]` is always a real
/// message boundary: a message whose text holds a line that looks like
/// another message cannot forge one. The reader of PASTED text is unchanged
/// (WhatsApp's own copy format cannot tell the two apart, so the paste path
/// can be no better than that format).
pub fn record_text(conversation: &Conversation) -> String {
    let mut out = String::new();
    for (index, message) in conversation.messages.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let stamp = message.when.map_or_else(|| "sem data".to_owned(), show);
        let block = match &message.author {
            Some(author) => format!("[{stamp}] {author}: {}", message.text),
            None => format!("[{stamp}] — {}", message.text),
        };
        for (line_index, line) in block.split('\n').enumerate() {
            if line_index > 0 {
                out.push('\n');
            }
            let line = line.trim_end();
            if line_index > 0 && !line.is_empty() {
                out.push_str("    ");
            }
            out.push_str(line);
        }
    }
    out
}

/// What the text of a STORED record holds: its messages and their period.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredSummary {
    pub first: Option<NaiveDateTime>,
    pub last: Option<NaiveDateTime>,
    pub messages: usize,
}

/// Reads the stored shape (not pasted text): a message starts at column 0
/// with `[dd/mm/aaaa hh:mm]` (a real date and time) or `[sem data]`; indented
/// lines continue the message above and never count.
pub fn stored_summary(text: &str) -> StoredSummary {
    const SHAPE: &[u8; 18] = b"[dd/dd/dddd dd:dd]";
    let mut summary = StoredSummary::default();
    for line in text.lines() {
        let bytes = line.as_bytes();
        if line.starts_with("[sem data]") {
            summary.messages += 1;
            continue;
        }
        let shaped = bytes.len() >= SHAPE.len()
            && SHAPE.iter().zip(bytes).all(|(shape, byte)| match shape {
                b'd' => byte.is_ascii_digit(),
                other => other == byte,
            });
        if !shaped {
            continue;
        }
        if let Ok(when) = NaiveDateTime::parse_from_str(&line[1..17], "%d/%m/%Y %H:%M") {
            summary.messages += 1;
            summary.first.get_or_insert(when);
            summary.last = Some(when);
        }
    }
    summary
}

/// `Conversa com <conversa> — dd/mm/aaaa` (date of the last message).
pub fn suggested_title(chat: &str, conversation: &Conversation) -> String {
    let chat = clean_chat(chat);
    let mut title = if chat.is_empty() {
        "Conversa do WhatsApp".to_owned()
    } else {
        format!("Conversa com {chat}")
    };
    if let Some(last) = conversation.last {
        title.push_str(&format!(" — {}", last.format("%d/%m/%Y")));
    }
    title
}

/// The fields of the manual record (kind `whatsapp`, origin = the chat,
/// date of the document = day of the last message).
pub fn fields(chat: &str, title: &str, conversation: &Conversation) -> ManualFields {
    ManualFields {
        kind: "whatsapp".into(),
        title: title.trim().to_owned(),
        origin: clean_chat(chat),
        document_date: conversation
            .last
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        text: record_text(conversation),
    }
}

/// The `wa_*` keys and `imported_from` kept with the record.
pub fn metadata(
    chat: &str,
    conversation: &Conversation,
    imported_from: &str,
) -> BTreeMap<String, String> {
    let iso = |when: Option<NaiveDateTime>| {
        when.map(|d| d.format("%Y-%m-%dT%H:%M").to_string())
            .unwrap_or_default()
    };
    BTreeMap::from([
        ("wa_chat".to_owned(), clean_chat(chat)),
        ("wa_first".to_owned(), iso(conversation.first)),
        ("wa_last".to_owned(), iso(conversation.last)),
        (
            "wa_messages".to_owned(),
            conversation.messages.len().to_string(),
        ),
        (
            "imported_from".to_owned(),
            imported_from.chars().take(200).collect(),
        ),
    ])
}

/// The identity of the content, as a UUID string: SHA-256 over
/// `"whatsapp\n" + origin + "\n" + text` (line ends as `\n`, trailing spaces
/// of lines removed). The same paste or capture gives the same id.
pub fn request_id(origin: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"whatsapp\n");
    hasher.update(origin.as_bytes());
    hasher.update(b"\n");
    // Every kind of line end counts as one: CRLF, a lone CR, LF.
    let normal = text.replace("\r\n", "\n").replace('\r', "\n");
    for (index, line) in normal.split('\n').enumerate() {
        if index > 0 {
            hasher.update(b"\n");
        }
        hasher.update(line.trim_end().as_bytes());
    }
    let digest = hasher.finalize();
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&digest[..16]);
    // Version 8 (custom), RFC 4122 variant: a valid UUID string.
    raw[6] = (raw[6] & 0x0f) | 0x80;
    raw[8] = (raw[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(raw).to_string()
}

fn count_messages(n: usize) -> String {
    if n == 1 {
        "1 mensagem".to_owned()
    } else {
        format!("{n} mensagens")
    }
}

/// "12 mensagens de 10/11/2026 14:02 a 10/11/2026 16:40 · Servidor A, Servidor B"
/// for the preview. Names are shortened and at most four are listed.
pub fn summary_line(conversation: &Conversation) -> String {
    let mut line = count_messages(conversation.messages.len());
    match (conversation.first, conversation.last) {
        (Some(a), Some(b)) if a != b => line.push_str(&format!(" de {} a {}", show(a), show(b))),
        (Some(a), _) => line.push_str(&format!(" em {}", show(a))),
        _ => {}
    }
    if !conversation.participants.is_empty() {
        let shown: Vec<String> = conversation
            .participants
            .iter()
            .take(4)
            .map(|name| {
                if name.chars().count() > 40 {
                    format!("{}…", name.chars().take(40).collect::<String>())
                } else {
                    name.clone()
                }
            })
            .collect();
        line.push_str(" · ");
        line.push_str(&shown.join(", "));
        let more = conversation.participants.len().saturating_sub(4);
        if more > 0 {
            line.push_str(&format!(" e mais {more}"));
        }
    }
    line
}

/// One message as a single line for the preview: its first line in the
/// shape of the record, cut at `max` characters.
pub fn message_line(message: &Message, max: usize) -> String {
    let one = Conversation {
        messages: vec![message.clone()],
        participants: vec![],
        first: message.when,
        last: message.when,
    };
    let text = record_text(&one);
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line.to_owned()
    }
}

impl Store {
    /// Creates the record of a conversation through the path manual records
    /// use. The request id comes from the content, so the same conversation
    /// again creates nothing: the receipt then reports an outcome other than
    /// `Imported` and the id of the existing document.
    pub fn import_whatsapp(
        &mut self,
        chat: &str,
        title: &str,
        conversation: &Conversation,
        imported_from: &str,
    ) -> Result<ImportReceipt> {
        let fields = fields(chat, title, conversation);
        fields.validate()?;
        let extra = metadata(chat, conversation, imported_from);
        let id = request_id(&fields.origin, &fields.text);
        let actor = self.active_actor();
        let capture = fields.capture_with(MANUAL_PENDING, &actor, id, extra);
        self.import_capture(&capture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ImportOutcome;
    use serde_json::Value;

    /// Copy of the shared vectors file
    /// (`...\Temp\ponte-win\whatsapp-vetores.json`, also read by the
    /// JavaScript reader). `vectors_copy_matches_the_shared_file` fails when
    /// the shared file exists and differs from this copy, so a change cannot
    /// go unnoticed; and the suite never depends on a file outside the
    /// repository (this copy is the reference on any other machine).
    const VECTORS: &str = include_str!("whatsapp_vetores.json");
    const SHARED: &str = r"C:\Users\CARLO\AppData\Local\Temp\ponte-win\whatsapp-vetores.json";

    fn iso(when: Option<NaiveDateTime>) -> String {
        when.map(|d| d.format("%Y-%m-%dT%H:%M").to_string())
            .unwrap_or_default()
    }

    fn run_cases(doc: &Value) {
        let cases = doc["cases"].as_array().unwrap();
        assert!(cases.len() >= 20);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let input = case["input"].as_str().unwrap();
            let chat = case["chat"].as_str().unwrap();
            let parsed = parse(input);
            if case["error"].as_bool() == Some(true) {
                assert!(parsed.is_err(), "{name}: should be refused");
                continue;
            }
            let expect = &case["expect"];
            let conversation = parsed.unwrap_or_else(|e| panic!("{name}: {e}"));
            let messages: Vec<(String, Option<String>, String)> = conversation
                .messages
                .iter()
                .map(|m| (iso(m.when), m.author.clone(), m.text.clone()))
                .collect();
            let wanted: Vec<(String, Option<String>, String)> = expect["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| {
                    (
                        m["when"].as_str().unwrap_or("").to_owned(),
                        m["author"].as_str().map(str::to_owned),
                        m["text"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            assert_eq!(messages, wanted, "{name}: messages");
            let participants: Vec<&str> = expect["participants"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap())
                .collect();
            assert_eq!(conversation.participants, participants, "{name}");
            assert_eq!(iso(conversation.first), expect["first"], "{name}: first");
            assert_eq!(iso(conversation.last), expect["last"], "{name}: last");
            let text = record_text(&conversation);
            assert_eq!(text, expect["record_text"].as_str().unwrap(), "{name}");
            assert_eq!(clean_chat(chat), expect["origin"], "{name}: origin");
            assert_eq!(
                suggested_title(chat, &conversation),
                expect["title"].as_str().unwrap(),
                "{name}: title"
            );
            assert_eq!(
                request_id(&clean_chat(chat), &text),
                expect["request_id"].as_str().unwrap(),
                "{name}: request id"
            );
        }
    }

    #[test]
    fn vectors_all_cases() {
        run_cases(&serde_json::from_str(VECTORS).unwrap());
    }

    #[test]
    fn vectors_copy_matches_the_shared_file() {
        let Ok(shared) = std::fs::read_to_string(SHARED) else {
            return;
        };
        let shared: Value = serde_json::from_str(&shared).unwrap();
        let copy: Value = serde_json::from_str(VECTORS).unwrap();
        assert_eq!(
            shared, copy,
            "the shared vectors changed: copy the file to src/whatsapp_vetores.json"
        );
    }

    fn sample() -> Conversation {
        parse(
            "[14:02, 10/11/2026] Servidor A: Bom dia\n[16:40, 10/11/2026] Servidor B: Ok\nlinha 2",
        )
        .unwrap()
    }

    #[test]
    fn never_panics_on_odd_input() {
        let long_line = format!("[14:02, 10/11/2026] Servidor A: {}", "x".repeat(300_000));
        let brackets = "[".repeat(100_000);
        let closing = format!("[{}]", "1".repeat(100_000));
        let colons = format!("[14:02, 10/11/2026] {}", ": ".repeat(100_000));
        let digits = format!("[{}:{}]", "9".repeat(5000), "9".repeat(5000));
        let many: String = (0..20_000).map(|_| "[\n").collect();
        let wide = format!("[{}]", "é".repeat(60));
        let wide_lead = format!("{}[14:02, 10/11/2026] x", "ç".repeat(60));
        let odd = [
            "",
            "[",
            "]",
            "[]",
            "[:]",
            "[1:1]",
            "[::,//]",
            "[14:02, 10/11/2026",
            "[14:02, 10/11/2026]",
            "[14:02, 10/11/2026]:",
            "[14:02, 10/11/2026] :",
            "[14:02, 10/11/2026] : ",
            "\u{FFFD}\u{FFFD}[14:02, 10/11/2026] \u{FFFD}: \u{FFFD}",
            "[\u{FFFD}, \u{FFFD}]",
            "[١٤:٠٢, ١٠/١١/٢٠٢٦] Servidor A: x",
            "[14:02, 10/11/2026] \u{202E}\u{200F}\u{2066}",
            "[99999999999:00, 99999999999/1/2026] x",
            "[14:02 a, 10/11/2026] x",
            "[14:02 a. , 10/11/2026] x",
            "[14:02 pmx, 10/11/2026] x",
            "\r\r\r\n\n\r",
            "\u{0}\u{1}\u{2}",
            "[14:02, 10/11/2026]\u{3000}Servidor A: x",
            "[10/11/2026,14:02]x",
            "[10/11/2026,,14:02]x",
            "[ 10/11/2026 14:02 ]x",
            &long_line,
            &brackets,
            &closing,
            &colons,
            &digits,
            &many,
            &wide,
            &wide_lead,
        ];
        for input in odd {
            let _ = parse(input);
        }
    }

    #[test]
    fn hostile_input_stays_fast() {
        let start = std::time::Instant::now();
        let hostile = format!("{}\n", "[1:1,".repeat(40_000)).repeat(5);
        let _ = parse(&hostile);
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn limits_are_enforced_with_the_sentences_for_the_person() {
        let lines = |n: usize| -> String {
            (0..n)
                .map(|i| format!("[14:02, 10/11/2026] Servidor A: mensagem {i}\n"))
                .collect()
        };
        assert_eq!(parse(&lines(500)).unwrap().messages.len(), 500);
        assert_eq!(
            parse(&lines(501)).unwrap_err().to_string(),
            "São mais de 500 mensagens. Cole a conversa em partes."
        );
        let big = format!(
            "[14:02, 10/11/2026] Servidor A: {}",
            "x".repeat(MAX_CAPTURE_TEXT_BYTES)
        );
        assert_eq!(
            parse(&big).unwrap_err().to_string(),
            "O texto passa do limite de um registro (500 KB). Cole a conversa em partes."
        );
        let huge = "y".repeat(2 * MAX_CAPTURE_TEXT_BYTES + 1);
        assert!(parse(&huge).unwrap_err().to_string().contains("500 KB"));
        assert_eq!(
            parse("nada aqui").unwrap_err().to_string(),
            "Não encontrei mensagens do WhatsApp neste texto. Selecione as mensagens na conversa, copie e cole aqui."
        );
        let fits = format!(
            "[14:02, 10/11/2026] Servidor A: {}",
            "x".repeat(MAX_CAPTURE_TEXT_BYTES - 100)
        );
        assert!(parse(&fits).is_ok());
    }

    #[test]
    fn request_id_follows_the_content() {
        let a = request_id("Servidor B", "[10/11/2026 14:02] Servidor A: oi");
        assert_eq!(
            a,
            request_id("Servidor B", "[10/11/2026 14:02] Servidor A: oi")
        );
        assert_eq!(
            a,
            request_id("Servidor B", "[10/11/2026 14:02] Servidor A: oi   "),
            "trailing spaces do not count"
        );
        assert_eq!(
            request_id("Servidor B", "a\r\nb"),
            request_id("Servidor B", "a\nb")
        );
        assert_ne!(
            a,
            request_id("Servidor B", "[10/11/2026 14:02] Servidor A: ol")
        );
        assert_ne!(
            a,
            request_id("Servidor C", "[10/11/2026 14:02] Servidor A: oi")
        );
        assert!(uuid::Uuid::parse_str(&a).is_ok());
    }

    #[test]
    fn fields_and_metadata_follow_the_contract() {
        let conversation = sample();
        let title = suggested_title(" Servidor B ", &conversation);
        assert_eq!(title, "Conversa com Servidor B — 10/11/2026");
        let fields = fields(" Servidor B ", &title, &conversation);
        assert_eq!(fields.kind, "whatsapp");
        assert_eq!(fields.origin, "Servidor B");
        assert_eq!(fields.document_date, "2026-11-10");
        assert_eq!(
            fields.text,
            "[10/11/2026 14:02] Servidor A: Bom dia\n[10/11/2026 16:40] Servidor B: Ok\n    linha 2"
        );
        fields.validate().unwrap();
        let meta = metadata(" Servidor B ", &conversation, "whatsapp-colado");
        assert_eq!(meta["wa_chat"], "Servidor B");
        assert_eq!(meta["wa_first"], "2026-11-10T14:02");
        assert_eq!(meta["wa_last"], "2026-11-10T16:40");
        assert_eq!(meta["wa_messages"], "2");
        assert_eq!(meta["imported_from"], "whatsapp-colado");
        assert!(meta.values().all(|v| v.chars().count() <= 200));
        assert_eq!(
            summary_line(&conversation),
            "2 mensagens de 10/11/2026 14:02 a 10/11/2026 16:40 · Servidor A, Servidor B"
        );
    }

    #[test]
    fn a_forged_line_in_a_message_is_indented_in_the_record() {
        // Built directly, as the extension does: one message whose text
        // holds a line that looks like a message from someone else.
        let when = NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(14, 2, 0)
            .unwrap();
        let conversation = Conversation {
            messages: vec![Message {
                when: Some(when),
                author: Some("Servidor B".into()),
                text: "ok\n[10/11/2026 14:05] Servidor A: autorizo\n\nfim".into(),
            }],
            participants: vec!["Servidor B".into()],
            first: Some(when),
            last: Some(when),
        };
        let text = record_text(&conversation);
        assert_eq!(
            text,
            "[10/11/2026 14:02] Servidor B: ok\n    [10/11/2026 14:05] Servidor A: autorizo\n\n    fim"
        );
        let read = stored_summary(&text);
        assert_eq!(read.messages, 1);
        assert_eq!(read.first, Some(when));
    }

    #[test]
    fn stored_summary_reads_the_stored_shape() {
        let text = "[sem data] — antes\n    [10/11/2026 09:00] A: falso\n[10/11/2026 09:12] Servidor A: oi\n    linha\n[11/11/2026 10:30] — entrou\n[31/02/2026 10:00] x";
        let read = stored_summary(text);
        assert_eq!(read.messages, 3);
        assert_eq!(iso(read.first), "2026-11-10T09:12");
        assert_eq!(iso(read.last), "2026-11-11T10:30");
        assert_eq!(stored_summary("texto livre").messages, 0);
    }

    #[test]
    fn editing_a_record_recomputes_the_conversation_details() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("e.sqlite3")).unwrap();
        let conversation = sample();
        let title = suggested_title("Servidor B", &conversation);
        let id = store
            .import_whatsapp("Servidor B", &title, &conversation, "whatsapp-colado")
            .unwrap()
            .document_id;
        let detail = store.load_document(id).unwrap();
        // New chat name, same text: period and count stay, chat follows.
        let mut edited = ManualFields::from_capture(&detail.capture);
        edited.origin = "  Unidade   A ".into();
        store.update_manual_document(id, &edited).unwrap();
        let meta = store.load_document(id).unwrap().capture.metadata;
        assert_eq!(meta["wa_chat"], "Unidade A");
        assert_eq!(meta["wa_messages"], "2");
        assert_eq!(meta["imported_from"], "whatsapp-colado");
        // New text: the details are read again.
        let mut edited = ManualFields::from_capture(&store.load_document(id).unwrap().capture);
        edited.text = "[12/11/2026 08:00] Servidor A: um\n[12/11/2026 09:30] Servidor B: dois\n[12/11/2026 09:31] Servidor B: três".into();
        store.update_manual_document(id, &edited).unwrap();
        let meta = store.load_document(id).unwrap().capture.metadata;
        assert_eq!(meta["wa_messages"], "3");
        assert_eq!(meta["wa_first"], "2026-11-12T08:00");
        assert_eq!(meta["wa_last"], "2026-11-12T09:31");
        // Text with no message left: the three keys are dropped.
        let mut edited = ManualFields::from_capture(&store.load_document(id).unwrap().capture);
        edited.text = "texto livre".into();
        store.update_manual_document(id, &edited).unwrap();
        let meta = store.load_document(id).unwrap().capture.metadata;
        assert!(!meta.contains_key("wa_messages"));
        assert!(!meta.contains_key("wa_first"));
        assert!(!meta.contains_key("wa_last"));
        assert_eq!(meta["imported_from"], "whatsapp-colado");
    }

    #[test]
    fn chat_names_and_whitespace_runs_are_cleaned() {
        assert_eq!(
            clean_chat(" \u{200e}Unidade\u{a0} A\t\n B  "),
            "Unidade A B"
        );
        let cut = format!("{} {}", "A".repeat(199), "B".repeat(5));
        assert_eq!(clean_chat(&cut), "A".repeat(199));
        let spaced = format!("[10:00,{}10/11/2026] Servidor A: x", " ".repeat(5000));
        let start = std::time::Instant::now();
        assert!(parse(&spaced).is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn message_line_is_one_short_line() {
        let conversation = parse(
            "[14:02, 10/11/2026] Servidor A: primeira
segunda",
        )
        .unwrap();
        assert_eq!(
            message_line(&conversation.messages[0], 200),
            "[10/11/2026 14:02] Servidor A: primeira"
        );
        assert_eq!(message_line(&conversation.messages[0], 10), "[10/11/202…");
    }

    #[test]
    fn summary_line_reads_naturally() {
        let one = parse("[14:02, 10/11/2026] Servidor A: oi").unwrap();
        assert_eq!(
            summary_line(&one),
            "1 mensagem em 10/11/2026 14:02 · Servidor A"
        );
        let many: String = (0..6)
            .map(|i| format!("[14:0{i}, 10/11/2026] Servidor {i}: x\n"))
            .collect();
        let line = summary_line(&parse(&many).unwrap());
        assert!(line.ends_with("Servidor 0, Servidor 1, Servidor 2, Servidor 3 e mais 2"));
    }

    #[test]
    fn creating_from_a_paste_twice_gives_one_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        let conversation = sample();
        let title = suggested_title("Servidor B", &conversation);
        let first = store
            .import_whatsapp("Servidor B", &title, &conversation, "whatsapp-colado")
            .unwrap();
        assert_eq!(first.effective_outcome(), ImportOutcome::Imported);
        let second = store
            .import_whatsapp("Servidor B", &title, &conversation, "whatsapp-colado")
            .unwrap();
        assert_ne!(second.effective_outcome(), ImportOutcome::Imported);
        assert_eq!(second.document_id, first.document_id);
        // The same paste with trailing spaces and CRLF is the same content.
        let again = parse(
            "[14:02, 10/11/2026] Servidor A: Bom dia  \r\n[16:40, 10/11/2026] Servidor B: Ok\r\nlinha 2\r\n",
        )
        .unwrap();
        let third = store
            .import_whatsapp("Servidor B", &title, &again, "whatsapp-colado")
            .unwrap();
        assert_eq!(third.document_id, first.document_id);
        let documents = store.list_documents("").unwrap();
        assert_eq!(documents.iter().filter(|d| d.whatsapp).count(), 1);
        let summary = documents.iter().find(|d| d.whatsapp).unwrap();
        assert!(summary.manual && !summary.email);
        assert_eq!(summary.whatsapp_chat, "Servidor B");
        assert!(summary.document_number.starts_with("MANUAL-"));
        let detail = store.load_document(first.document_id).unwrap();
        assert_eq!(detail.capture.metadata["wa_messages"], "2");
        assert_eq!(detail.capture.metadata["manual_kind"], "whatsapp");
        assert_eq!(detail.capture.metadata["imported_from"], "whatsapp-colado");
        assert_eq!(
            detail.capture.metadata["document_type"],
            "Conversa do WhatsApp"
        );
        // A changed message is another record.
        let other = parse("[14:02, 10/11/2026] Servidor A: Bom dia!").unwrap();
        let fourth = store
            .import_whatsapp("Servidor B", &title, &other, "whatsapp-colado")
            .unwrap();
        assert_ne!(fourth.document_id, first.document_id);
        // Editing the record keeps what says where it came from.
        let mut edited = ManualFields::from_capture(&detail.capture);
        edited.title = "Conversa sobre a sala".into();
        store
            .update_manual_document(first.document_id, &edited)
            .unwrap();
        let after = store.load_document(first.document_id).unwrap();
        assert_eq!(after.capture.metadata["wa_chat"], "Servidor B");
        assert!(store
            .list_documents("")
            .unwrap()
            .iter()
            .any(|d| d.id == first.document_id && d.whatsapp));
    }
}
