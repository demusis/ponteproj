//! E-mails saved as `.eml` (Gmail "Mostrar original ▸ Baixar original",
//! Outlook "Salvar como"), imported as manual records of the kind
//! "E-mail recebido": the message becomes a document with its headers and
//! text, and its files become attachments. Nothing here talks to any mail
//! server; the file is read once, here, and the record is sealed like any
//! other.
//!
//! The decoder is deliberately small and works on bytes: RFC 2822 headers
//! (folded, RFC 2047 encoded words), MIME multipart with nesting (boundaries
//! anchored at line starts), base64 and quoted-printable, UTF-8,
//! Windows-1252/ISO-8859-1 and UTF-16, RFC 2231 parameters (encoded and
//! split). It prefers the plain-text rendering of each alternative and
//! falls back to the HTML part stripped of tags; several text parts of a
//! mixed message are kept in order. Anything it cannot read is said in a
//! note, never silently dropped.
//!
//! The same message imported twice is recognized by its Message-ID (the
//! import's `request_id` derives from it), so nothing is duplicated; files
//! that could not be attached the first time are attached on a re-import.
use crate::{
    attachments::Owner,
    manual::ManualFields,
    model::{ImportOutcome, ImportReceipt, MANUAL_PENDING, MAX_CAPTURE_TEXT_BYTES},
    storage::Store,
};
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_EML_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PARTS: usize = 200;
const MAX_DEPTH: usize = 8;
/// A metadata value of the record (the capture allows 64 KiB each).
const MAX_META_BYTES: usize = 4000;

/// One file carried by the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailAttachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

/// What a `.eml` says, decoded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedMail {
    pub subject: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    /// The Date header as written.
    pub date_raw: String,
    /// The Date header as `AAAA-MM-DD`, when it parses.
    pub date: String,
    pub message_id: String,
    /// Plain text of the message (text parts in order; HTML stripped when
    /// there is no plain rendering).
    pub body: String,
    pub attachments: Vec<MailAttachment>,
    /// What could not be read or was left out.
    pub notes: Vec<String>,
}

struct Part<'a> {
    headers: Vec<(String, String)>,
    body: &'a [u8],
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Splits headers (unfolded) from the body at the first blank line.
fn split_part(bytes: &[u8]) -> Part<'_> {
    let mut headers = Vec::new();
    let mut position = 0usize;
    let mut current: Option<(String, String)> = None;
    while position < bytes.len() {
        let end = bytes[position..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| position + i)
            .unwrap_or(bytes.len());
        let line = &bytes[position..end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let next = (end + 1).min(bytes.len());
        if line.is_empty() {
            position = next;
            break;
        }
        let text = String::from_utf8_lossy(line).into_owned();
        if text.starts_with(' ') || text.starts_with('\t') {
            if let Some((_, value)) = current.as_mut() {
                value.push(' ');
                value.push_str(text.trim());
            }
        } else {
            if let Some(done) = current.take() {
                headers.push(done);
            }
            if let Some((name, value)) = text.split_once(':') {
                current = Some((name.trim().to_owned(), value.trim().to_owned()));
            }
        }
        position = next;
        if end >= bytes.len() {
            break;
        }
    }
    if let Some(done) = current.take() {
        headers.push(done);
    }
    Part {
        headers,
        body: &bytes[position.min(bytes.len())..],
    }
}

/// `type/subtype` and its parameters (lower-cased names). Quoted values
/// may contain `;`. RFC 2231 continuations (`name*0*=…; name*1*=…`) are
/// joined under `name*` (or `name` when none of the pieces is encoded).
fn parse_content_type(value: &str) -> (String, BTreeMap<String, String>) {
    let mut fields: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in value.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ';' if !quoted => fields.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    fields.push(current);
    let mime = fields
        .first()
        .map(|m| m.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let mut params: BTreeMap<String, String> = BTreeMap::new();
    // (name, index, encoded, value) for split parameters.
    let mut pieces: Vec<(String, usize, bool, String)> = Vec::new();
    for field in fields.iter().skip(1) {
        let Some((name, raw)) = field.split_once('=') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let raw = raw.trim();
        let value = raw
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(raw)
            .to_owned();
        let encoded = name.ends_with('*');
        let base = name.trim_end_matches('*');
        match base.rsplit_once('*') {
            Some((stem, index)) if index.bytes().all(|b| b.is_ascii_digit()) => {
                pieces.push((stem.to_owned(), index.parse().unwrap_or(0), encoded, value));
            }
            _ => {
                params.insert(name, value);
            }
        }
    }
    pieces.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut joined: BTreeMap<String, (bool, String)> = BTreeMap::new();
    for (stem, _, encoded, value) in pieces {
        let entry = joined.entry(stem).or_insert((false, String::new()));
        // Unencoded pieces of an encoded parameter are literal: percent
        // signs in them are escaped so the final decoding leaves them.
        if encoded {
            entry.0 = true;
            entry.1.push_str(&value);
        } else {
            entry.1.push_str(&value.replace('%', "%25"));
        }
    }
    for (stem, (encoded, value)) in joined {
        if encoded {
            params.entry(format!("{stem}*")).or_insert(value);
        } else {
            params
                .entry(stem)
                .or_insert_with(|| value.replace("%25", "%"));
        }
    }
    (mime, params)
}

/// A parameter that may come RFC 2231-encoded (`name*=utf-8''...`) or as
/// an RFC 2047 encoded word.
fn param(params: &BTreeMap<String, String>, name: &str) -> Option<String> {
    if let Some(extended) = params.get(&format!("{name}*")) {
        let mut pieces = extended.splitn(3, '\'');
        let first = pieces.next().unwrap_or("");
        let (charset, encoded) = match (pieces.next(), pieces.next()) {
            (Some(_language), Some(rest)) => (first, rest),
            _ => ("utf-8", extended.as_str()),
        };
        let bytes = percent_decode(encoded.as_bytes());
        return Some(decode_charset(&bytes, charset));
    }
    params.get(name).map(|v| decode_encoded_words(v))
}

fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("00");
            out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    out
}

/// Text in `charset` as a String. Unknown charsets are read as UTF-8.
fn decode_charset(bytes: &[u8], charset: &str) -> String {
    let charset = charset.trim().to_ascii_lowercase();
    match charset.as_str() {
        "iso-8859-1" | "latin1" | "iso8859-1" | "windows-1252" | "cp1252" | "iso-8859-15"
        | "us-ascii" | "ascii" => bytes
            .iter()
            .map(|&b| crate::extraction::windows_1252(b))
            .collect(),
        "utf-16" | "utf-16le" | "utf-16be" => {
            let (little, body) = if let Some(body) = bytes.strip_prefix(b"\xFF\xFE") {
                (true, body)
            } else if let Some(body) = bytes.strip_prefix(b"\xFE\xFF") {
                (false, body)
            } else {
                (charset != "utf-16be", bytes)
            };
            let units: Vec<u16> = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| {
                    if little {
                        u16::from_le_bytes(*p)
                    } else {
                        u16::from_be_bytes(*p)
                    }
                })
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn decode_base64(bytes: &[u8]) -> Vec<u8> {
    use base64::Engine;
    let compact: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let trimmed: Vec<u8> = compact.iter().copied().take_while(|&b| b != b'=').collect();
    base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&trimmed))
        .unwrap_or_default()
}

fn decode_quoted_printable(bytes: &[u8], header_mode: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'=' => {
                // Soft line break: "=" then optional blanks, then CRLF or LF.
                let mut look = index + 1;
                while look < bytes.len() && (bytes[look] == b' ' || bytes[look] == b'\t') {
                    look += 1;
                }
                if look >= bytes.len() {
                    index = look;
                    continue;
                }
                if bytes[look] == b'\r' || bytes[look] == b'\n' {
                    if bytes[look] == b'\r' && look + 1 < bytes.len() && bytes[look + 1] == b'\n' {
                        look += 1;
                    }
                    index = look + 1;
                    continue;
                }
                if index + 2 < bytes.len()
                    && bytes[index + 1].is_ascii_hexdigit()
                    && bytes[index + 2].is_ascii_hexdigit()
                {
                    let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("3F");
                    out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                    index += 3;
                    continue;
                }
                out.push(b'=');
                index += 1;
            }
            b'_' if header_mode => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    out
}

/// RFC 2047: `=?charset?B?...?=` and `=?charset?Q?...?=` inside a header.
/// Adjacent words of the same charset are decoded together, since some
/// encoders split one multibyte character across two words.
fn decode_encoded_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut pending: Option<(String, Vec<u8>)> = None; // (charset, bytes)
    let flush = |pending: &mut Option<(String, Vec<u8>)>, out: &mut String| {
        if let Some((charset, bytes)) = pending.take() {
            out.push_str(&decode_charset(&bytes, &charset));
        }
    };
    while let Some(start) = rest.find("=?") {
        let literal = &rest[..start];
        // Whitespace between two encoded words is not text.
        if !(pending.is_some() && literal.trim().is_empty()) {
            flush(&mut pending, &mut out);
            out.push_str(literal);
        }
        let after = &rest[start + 2..];
        let parsed = after.find('?').and_then(|charset_end| {
            let charset = &after[..charset_end];
            let tail = &after[charset_end + 1..];
            let encoding_end = tail.find('?')?;
            let encoding = &tail[..encoding_end];
            let tail = &tail[encoding_end + 1..];
            let text_end = tail.find("?=")?;
            Some((charset, encoding, &tail[..text_end], &tail[text_end + 2..]))
        });
        let Some((charset, encoding, encoded, remainder)) = parsed else {
            flush(&mut pending, &mut out);
            out.push_str(&rest[start..]);
            return out;
        };
        let bytes = match encoding.to_ascii_uppercase().as_str() {
            "B" => decode_base64(encoded.as_bytes()),
            "Q" => decode_quoted_printable(encoded.as_bytes(), true),
            _ => encoded.as_bytes().to_vec(),
        };
        let charset = charset.trim().to_ascii_lowercase();
        match pending.as_mut() {
            Some((current, buffer)) if *current == charset => buffer.extend_from_slice(&bytes),
            _ => {
                flush(&mut pending, &mut out);
                pending = Some((charset, bytes));
            }
        }
        rest = remainder;
    }
    flush(&mut pending, &mut out);
    out.push_str(rest);
    out
}

/// The bytes of a part after its transfer encoding.
fn decode_body(part: &Part<'_>) -> Vec<u8> {
    let encoding = header(&part.headers, "Content-Transfer-Encoding")
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match encoding.as_str() {
        "base64" => decode_base64(part.body),
        "quoted-printable" => decode_quoted_printable(part.body, false),
        _ => part.body.to_vec(),
    }
}

/// Latin-1 named entities (what Portuguese HTML mail actually uses).
fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "aacute" => 'á',
        "agrave" => 'à',
        "atilde" => 'ã',
        "acirc" => 'â',
        "auml" => 'ä',
        "eacute" => 'é',
        "egrave" => 'è',
        "ecirc" => 'ê',
        "iacute" => 'í',
        "icirc" => 'î',
        "oacute" => 'ó',
        "ograve" => 'ò',
        "otilde" => 'õ',
        "ocirc" => 'ô',
        "ouml" => 'ö',
        "uacute" => 'ú',
        "ucirc" => 'û',
        "uuml" => 'ü',
        "ccedil" => 'ç',
        "ntilde" => 'ñ',
        "Aacute" => 'Á',
        "Agrave" => 'À',
        "Atilde" => 'Ã',
        "Acirc" => 'Â',
        "Eacute" => 'É',
        "Ecirc" => 'Ê',
        "Iacute" => 'Í',
        "Oacute" => 'Ó',
        "Otilde" => 'Õ',
        "Ocirc" => 'Ô',
        "Uacute" => 'Ú',
        "Ccedil" => 'Ç',
        "ordm" => 'º',
        "ordf" => 'ª',
        "sect" => '§',
        "deg" => '°',
        "ndash" => '–',
        "mdash" => '—',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "hellip" => '…',
        "laquo" => '«',
        "raquo" => '»',
        "copy" => '©',
        "reg" => '®',
        "euro" => '€',
        "bull" => '•',
        _ => return None,
    })
}

fn push_entities(out: &mut String, text: &str) {
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        // An entity is short: look no further than a dozen bytes.
        let mut window_end = (start + 12).min(rest.len());
        while !rest.is_char_boundary(window_end) {
            window_end -= 1;
        }
        let Some(end) = rest[start..window_end].find(';') else {
            out.push('&');
            rest = &rest[start + 1..];
            continue;
        };
        let entity = &rest[start + 1..start + end];
        let decoded = named_entity(entity).or_else(|| {
            entity
                .strip_prefix('#')
                .and_then(|n| {
                    if let Some(hex) = n.strip_prefix('x').or(n.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        n.parse().ok()
                    }
                })
                .and_then(char::from_u32)
        });
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[start..start + end + 1]),
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
}

/// HTML as plain text: block elements become line breaks, tags vanish,
/// entities are decoded, scripts, styles and the head are dropped.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut skip_until: Option<&'static str> = None;
    while let Some(start) = rest.find('<') {
        if skip_until.is_none() {
            push_entities(&mut out, &rest[..start]);
        }
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start + 1..start + end];
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let closing = tag.starts_with('/');
        if let Some(waiting) = skip_until {
            if closing && name == waiting {
                skip_until = None;
            }
        } else {
            match name.as_str() {
                "script" if !closing => skip_until = Some("script"),
                "style" if !closing => skip_until = Some("style"),
                "head" if !closing => skip_until = Some("head"),
                "br" => out.push('\n'),
                "p" | "div" | "li" => out.push('\n'),
                "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "table" | "ul"
                | "ol"
                    if closing =>
                {
                    out.push('\n')
                }
                "td" | "th" if closing => out.push('\t'),
                _ => {}
            }
        }
        rest = &rest[start + end + 1..];
    }
    if skip_until.is_none() {
        push_entities(&mut out, rest);
    }
    // Collapse the whitespace HTML ignores, keep paragraph breaks.
    let mut tidy = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        tidy.push_str(&line);
        tidy.push('\n');
    }
    tidy.trim().to_owned()
}

/// The sections of a multipart body: the bytes between boundary lines,
/// which are `--boundary` at the start of a line. Nothing is re-encoded,
/// so 8-bit and binary children keep their bytes.
fn multipart_sections<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let marker = format!("--{boundary}");
    let marker = marker.as_bytes();
    let mut sections = Vec::new();
    let mut open: Option<usize> = None; // start of the current section
    let mut position = 0usize;
    while position <= body.len() {
        let line_end = body[position..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| position + i)
            .unwrap_or(body.len());
        let line = &body[position..line_end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(marker) {
            let after = &line[marker.len()..];
            let is_end = after.starts_with(b"--");
            if let Some(start) = open.take() {
                // The CRLF before the boundary belongs to the boundary.
                let mut end = position;
                if end > start && body[end - 1] == b'\n' {
                    end -= 1;
                    if end > start && body[end - 1] == b'\r' {
                        end -= 1;
                    }
                }
                sections.push(&body[start..end]);
                if sections.len() > MAX_PARTS {
                    break;
                }
            }
            if is_end {
                break;
            }
            open = Some((line_end + 1).min(body.len()));
        }
        if line_end >= body.len() {
            break;
        }
        position = line_end + 1;
    }
    sections
}

/// Decodes a `.eml`.
pub fn parse(bytes: &[u8]) -> Result<ParsedMail> {
    ensure!(
        bytes.len() as u64 <= MAX_EML_BYTES,
        "O arquivo .eml passa de {} MB.",
        MAX_EML_BYTES / (1024 * 1024)
    );
    let root = split_part(bytes);
    ensure!(
        header(&root.headers, "From").is_some()
            || header(&root.headers, "Subject").is_some()
            || header(&root.headers, "Date").is_some(),
        "O arquivo não parece um e-mail (.eml): não tem remetente, assunto nem data."
    );
    let text_header = |name: &str| {
        clean_header(
            &header(&root.headers, name)
                .map(decode_encoded_words)
                .unwrap_or_default(),
        )
    };
    let mut parsed = ParsedMail {
        subject: text_header("Subject"),
        from: text_header("From"),
        to: text_header("To"),
        cc: text_header("Cc"),
        date_raw: text_header("Date"),
        message_id: header(&root.headers, "Message-ID")
            .unwrap_or("")
            .trim()
            .trim_matches(|c| c == '<' || c == '>')
            .chars()
            .filter(|c| !c.is_control())
            .take(500)
            .collect(),
        ..Default::default()
    };
    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(&parsed.date_raw) {
        parsed.date = date.format("%Y-%m-%d").to_string();
    }
    let mut texts: Vec<(String, String)> = Vec::new(); // (mime, text)
    let mut count = 0usize;
    walk(&root, 0, &mut count, &mut texts, &mut parsed)?;
    // Every text part of the message, in order; an alternative already
    // chose its best rendering.
    let mut body = String::new();
    for (mime, text) in &texts {
        let piece = if mime == "text/html" {
            html_to_text(text)
        } else {
            text.replace("\r\n", "\n")
        };
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(piece);
    }
    parsed.body = body;
    if parsed.body.is_empty() && parsed.attachments.is_empty() {
        parsed
            .notes
            .push("A mensagem não tem texto legível nem anexos.".into());
    }
    Ok(parsed)
}

/// Header text as a single line without control characters.
fn clean_header(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn walk(
    part: &Part<'_>,
    depth: usize,
    count: &mut usize,
    texts: &mut Vec<(String, String)>,
    parsed: &mut ParsedMail,
) -> Result<()> {
    *count += 1;
    ensure!(*count <= MAX_PARTS, "O e-mail tem partes demais.");
    ensure!(depth <= MAX_DEPTH, "O e-mail tem partes aninhadas demais.");
    let (mime, params) = parse_content_type(
        header(&part.headers, "Content-Type").unwrap_or("text/plain; charset=us-ascii"),
    );
    let (disposition, disposition_params) =
        parse_content_type(header(&part.headers, "Content-Disposition").unwrap_or(""));
    let filename = param(&disposition_params, "filename")
        .or_else(|| param(&params, "name"))
        .map(|n| clean_header(&n))
        .filter(|n| !n.is_empty());
    if mime.starts_with("multipart/") {
        let Some(boundary) = params.get("boundary").filter(|b| !b.is_empty()) else {
            parsed
                .notes
                .push("Uma parte multipart sem delimitador foi ignorada.".into());
            return Ok(());
        };
        let mut alternative_texts: Vec<(String, String)> = Vec::new();
        for section in multipart_sections(part.body, boundary) {
            let child = split_part(section);
            if mime == "multipart/alternative" {
                walk(&child, depth + 1, count, &mut alternative_texts, parsed)?;
            } else {
                walk(&child, depth + 1, count, texts, parsed)?;
            }
        }
        if mime == "multipart/alternative" {
            // One rendering of the same content: keep the best one only.
            if let Some(best) = alternative_texts
                .iter()
                .find(|(m, _)| m == "text/plain")
                .or_else(|| alternative_texts.first())
            {
                texts.push(best.clone());
            }
        }
        return Ok(());
    }
    let data = decode_body(part);
    let is_attachment =
        disposition == "attachment" || (filename.is_some() && !mime.starts_with("text/"));
    if is_attachment || mime == "message/rfc822" {
        let name = filename.unwrap_or_else(|| {
            let extension = match mime.as_str() {
                "message/rfc822" => "eml",
                "application/pdf" => "pdf",
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/gif" => "gif",
                "text/plain" => "txt",
                "text/csv" => "csv",
                "application/msword" => "doc",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
                "application/vnd.ms-excel" => "xls",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
                _ => "bin",
            };
            format!("anexo-{}.{extension}", parsed.attachments.len() + 1)
        });
        if data.is_empty() {
            parsed
                .notes
                .push(format!("O anexo “{name}” veio vazio ou ilegível."));
            return Ok(());
        }
        parsed.attachments.push(MailAttachment { name, mime, data });
        return Ok(());
    }
    if mime == "text/plain" || mime == "text/html" {
        let charset = params
            .get("charset")
            .cloned()
            .unwrap_or_else(|| "utf-8".into());
        texts.push((mime, decode_charset(&data, &charset)));
        return Ok(());
    }
    // Inline images without a name, calendar invitations and the like.
    if !mime.is_empty() && !data.is_empty() {
        parsed.notes.push(format!(
            "Uma parte do tipo {mime} sem nome de arquivo foi deixada de fora."
        ));
    }
    Ok(())
}

/// A UUID that follows from the message: the same `.eml` imported twice
/// is the same request, so nothing is duplicated.
fn request_id_for(parsed: &ParsedMail, bytes: &[u8]) -> String {
    if parsed.message_id.is_empty() {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        return uuid_from_digest(hasher);
    }
    request_id_for_message_id(&parsed.message_id)
}

/// The request id a message with this Message-ID gets, wherever it is
/// imported from (a `.eml` file or the mailbox).
pub(crate) fn request_id_for_message_id(message_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ponte-mail:");
    hasher.update(
        message_id
            .trim()
            .trim_matches(|c| c == '<' || c == '>')
            .as_bytes(),
    );
    uuid_from_digest(hasher)
}

/// The request id of a message that shares its Message-ID with a different
/// one already in the library: tied to the content, so importing it again
/// is still recognized.
fn request_id_for_content(message_id: &str, digest: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ponte-mail-content:");
    hasher.update(message_id.trim().as_bytes());
    hasher.update(b"\n");
    hasher.update(digest.as_bytes());
    uuid_from_digest(hasher)
}

/// What a message says and carries: the record text and the content of
/// every attachment. Two messages with the same Message-ID are the same
/// message only when this matches (a Message-ID is not secret: anyone on
/// the thread has it).
fn content_digest(text: &str, attachments: &[MailAttachment]) -> String {
    let mut files: Vec<String> = attachments
        .iter()
        .map(|a| format!("{:x}", Sha256::digest(&a.data)))
        .collect();
    files.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"ponte-mail-digest:");
    hasher.update(text.as_bytes());
    for file in files {
        hasher.update(b"\n");
        hasher.update(file.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn uuid_from_digest(hasher: Sha256) -> String {
    let digest = hasher.finalize();
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&digest[..16]);
    // Version 8 (custom), RFC 4122 variant: a valid UUID string.
    raw[6] = (raw[6] & 0x0f) | 0x80;
    raw[8] = (raw[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(raw).to_string()
}

/// `text` cut at a character boundary so it has at most `max` bytes.
fn clip_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// The record's text: a short header block, then the message.
fn record_text(parsed: &ParsedMail) -> (String, Option<String>) {
    let mut text = String::new();
    for (label, value) in [
        ("De", &parsed.from),
        ("Para", &parsed.to),
        ("Cc", &parsed.cc),
        ("Data", &parsed.date_raw),
        ("Assunto", &parsed.subject),
    ] {
        if !value.is_empty() {
            text.push_str(&format!("{label}: {value}\n"));
        }
    }
    if !parsed.attachments.is_empty() {
        let names: Vec<&str> = parsed.attachments.iter().map(|a| a.name.as_str()).collect();
        text.push_str(&format!("Anexos: {}\n", names.join("; ")));
    }
    text.push('\n');
    text.push_str(&parsed.body);
    if text.len() <= MAX_CAPTURE_TEXT_BYTES {
        return (text, None);
    }
    let mut clipped = clip_bytes(&text, MAX_CAPTURE_TEXT_BYTES - 64);
    clipped.push_str("\n[…texto cortado em 500 KiB…]");
    (
        clipped,
        Some("O texto da mensagem foi cortado em 500 KiB.".to_owned()),
    )
}

/// How a message reached the Ponte (kept in the record, read by people and
/// by the AI).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailSource {
    /// A `.eml` file chosen by the person.
    File,
    /// Downloaded from one of the person's mailboxes (IMAP): the name they
    /// gave it and its address, kept so the record says where it arrived.
    Account { name: String, address: String },
}

impl MailSource {
    pub fn label(&self) -> &'static str {
        match self {
            MailSource::File => "arquivo .eml",
            MailSource::Account { .. } => "conta de e-mail (IMAP)",
        }
    }
}

/// Result of importing one `.eml`.
#[derive(Clone, Debug)]
pub struct MailImport {
    pub receipt: ImportReceipt,
    pub subject: String,
    /// True when the message was already in the library (same Message-ID
    /// and same content).
    pub already_imported: bool,
    /// True when another message with the same Message-ID but different
    /// content was there: this one became a record of its own.
    pub separate_record: bool,
    pub attachments_added: usize,
    /// Files that could not be attached, with the reason.
    pub attachments_skipped: Vec<String>,
    pub notes: Vec<String>,
}

impl Store {
    /// Imports a `.eml` file as a manual record "E-mail recebido" with its
    /// attachments. On a client computer the record and the attachments go
    /// to the main one like any import.
    pub fn import_eml(&mut self, path: &std::path::Path) -> Result<MailImport> {
        let size = std::fs::metadata(path)
            .with_context(|| format!("Não foi possível ler {}", path.display()))?
            .len();
        ensure!(
            size <= MAX_EML_BYTES,
            "O arquivo tem {} MB; o limite é {} MB.",
            size / (1024 * 1024),
            MAX_EML_BYTES / (1024 * 1024)
        );
        let bytes = std::fs::read(path)
            .with_context(|| format!("Não foi possível ler {}", path.display()))?;
        self.import_eml_bytes(&bytes)
    }

    pub fn import_eml_bytes(&mut self, bytes: &[u8]) -> Result<MailImport> {
        self.import_mail_bytes(bytes, &MailSource::File)
    }

    /// Imports one message, recording where it came from.
    pub fn import_mail_bytes(&mut self, bytes: &[u8], source: &MailSource) -> Result<MailImport> {
        let parsed = parse(bytes)?;
        let (text, clipped) = record_text(&parsed);
        let digest = content_digest(&text, &parsed.attachments);
        let title = if parsed.subject.chars().count() >= 3 {
            parsed.subject.clone()
        } else {
            format!(
                "E-mail de {}",
                if parsed.from.is_empty() {
                    "remetente não informado"
                } else {
                    parsed.from.as_str()
                }
            )
        };
        let fields = ManualFields {
            kind: "email".into(),
            title: clip_bytes(&title, 1000),
            origin: clip_bytes(&parsed.from, 1000),
            document_date: parsed.date.clone(),
            text: text.clone(),
        };
        fields.validate()?;
        let mut extra = BTreeMap::new();
        for (key, value) in [
            ("mail_from", &parsed.from),
            ("mail_to", &parsed.to),
            ("mail_cc", &parsed.cc),
            ("mail_message_id", &parsed.message_id),
            ("mail_date", &parsed.date_raw),
        ] {
            if !value.is_empty() {
                extra.insert(key.to_owned(), clip_bytes(value, MAX_META_BYTES));
            }
        }
        extra.insert("imported_from".to_owned(), source.label().to_owned());
        if let MailSource::Account { name, address } = source {
            if !address.trim().is_empty() {
                extra.insert(
                    "mail_account".to_owned(),
                    clip_bytes(address.trim(), MAX_META_BYTES),
                );
            }
            if !name.trim().is_empty() {
                extra.insert(
                    "mail_account_name".to_owned(),
                    clip_bytes(name.trim(), MAX_META_BYTES),
                );
            }
        }
        extra.insert("mail_digest".to_owned(), digest.clone());
        let actor = self.active_actor();
        let capture = fields.capture_with(
            MANUAL_PENDING,
            &actor,
            request_id_for(&parsed, bytes),
            extra.clone(),
        );
        let mut receipt = self.import_capture(&capture)?;
        let mut notes = parsed.notes.clone();
        if let Some(note) = clipped {
            notes.push(note);
        }
        // Same Message-ID: the stored record answers whether it is the same
        // message. Only then may this import complete its attachments.
        let mut separate_record = false;
        let mut may_attach = true;
        if receipt.effective_outcome() != ImportOutcome::Imported {
            match self.load_document(receipt.document_id) {
                Ok(detail) => {
                    let same = match detail.capture.metadata.get("mail_digest") {
                        Some(stored) => *stored == digest,
                        // Imported before the digest existed: the text,
                        // which lists the attachments by name, decides.
                        None => detail.capture.text.trim() == text.trim(),
                    };
                    if !same {
                        let capture = fields.capture_with(
                            MANUAL_PENDING,
                            &actor,
                            request_id_for_content(&parsed.message_id, &digest),
                            extra,
                        );
                        receipt = self.import_capture(&capture)?;
                        separate_record = true;
                        notes.push(
                            "Já havia no acervo outro e-mail com o mesmo identificador (Message-ID) e conteúdo diferente; este foi guardado como registro separado. Confira a origem.".to_owned(),
                        );
                    }
                }
                Err(_) => {
                    may_attach = false;
                    notes.push(
                        "O registro já existente não pôde ser conferido neste computador; os anexos não foram acrescentados.".to_owned(),
                    );
                }
            }
        }
        let already_imported = receipt.effective_outcome() != ImportOutcome::Imported;
        // Files already there (by content) are skipped quietly, so a second
        // import completes what a first one could not attach.
        let present = self
            .list_attachments(Owner::Document(receipt.document_id))
            .unwrap_or_default();
        let mut added = 0usize;
        let mut skipped = Vec::new();
        for attachment in parsed.attachments.iter().filter(|_| may_attach) {
            let sha256 = format!("{:x}", Sha256::digest(&attachment.data));
            if present.iter().any(|a| a.sha256 == sha256) {
                continue;
            }
            let description = clip_bytes(&format!("Anexo do e-mail ({})", attachment.mime), 200);
            match self.add_attachment(
                Owner::Document(receipt.document_id),
                &sanitize_name(&attachment.name),
                &description,
                &attachment.data,
            ) {
                Ok(_) => added += 1,
                Err(error) => skipped.push(format!("{}: {error}", attachment.name)),
            }
        }
        Ok(MailImport {
            receipt,
            subject: fields.title,
            already_imported,
            separate_record,
            attachments_added: added,
            attachments_skipped: skipped,
            notes,
        })
    }
}

fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "anexo".to_owned()
    } else {
        cleaned
            .chars()
            .take(crate::attachments::MAX_NAME_CHARS)
            .collect()
    }
}

/// Text of a `.eml` for an example file: subject and body only (no
/// addresses, which an example never needs).
pub fn example_text(bytes: &[u8]) -> Result<String> {
    let parsed = parse(bytes)?;
    let mut text = String::new();
    if !parsed.subject.is_empty() {
        text.push_str(&format!("Assunto: {}\n\n", parsed.subject));
    }
    text.push_str(&parsed.body);
    Ok(text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "From: =?UTF-8?Q?Jo=C3=A3o_Silva?= <joao@exemplo.gov.br>\r\n\
To: gabinete@exemplo.gov.br\r\n\
Cc: =?ISO-8859-1?B?TWFyaWEgQ29uY2Vp5+Nv?= <maria@exemplo.gov.br>\r\n\
Subject: =?UTF-8?B?UGVkaWRvIGRlIG1hdGVyaWFsIGRlIGVzY3JpdMOzcmlv?=\r\n\
Date: Tue, 3 Nov 2026 14:05:00 -0300\r\n\
Message-ID: <abc123@exemplo.gov.br>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
\r\n\
--outer\r\n\
Content-Type: multipart/alternative; boundary=\"inner\"\r\n\
\r\n\
--inner\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Prezados,\r\n\
Solicito a reposi=C3=A7=C3=A3o de papel A4 para o setor.\r\n\
Linha longa que termina com quebra suave=\r\n\
=20continuada.\r\n\
--inner\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><body><p>Prezados,</p><p>Solicito a reposi&ccedil;&atilde;o.</p></body></html>\r\n\
--inner--\r\n\
--outer\r\n\
Content-Type: application/pdf; name=\"oficio.pdf\"\r\n\
Content-Disposition: attachment; filename*=utf-8''of%C3%ADcio%2012.pdf\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQKJWFuZXhvIGRlIHRlc3RlCg==\r\n\
--outer\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo>\r\n\
Content-Disposition: inline\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--outer--\r\n";

    #[test]
    fn headers_text_and_attachments_are_decoded() {
        let mail = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(mail.subject, "Pedido de material de escritório");
        assert_eq!(mail.from, "João Silva <joao@exemplo.gov.br>");
        assert_eq!(mail.cc, "Maria Conceição <maria@exemplo.gov.br>");
        assert_eq!(mail.date, "2026-11-03");
        assert_eq!(mail.message_id, "abc123@exemplo.gov.br");
        assert_eq!(
            mail.body,
            "Prezados,\nSolicito a reposição de papel A4 para o setor.\nLinha longa que termina com quebra suave continuada."
        );
        assert_eq!(mail.attachments.len(), 1, "{:?}", mail.notes);
        assert_eq!(mail.attachments[0].name, "ofício 12.pdf");
        assert!(mail.attachments[0].data.starts_with(b"%PDF-1.4"));
        assert!(mail.notes.iter().any(|n| n.contains("image/png")));
    }

    #[test]
    fn html_only_messages_fall_back_to_stripped_text() {
        let bytes: &[u8] = b"From: a@b\r\nSubject: Oi\r\nContent-Type: text/html; charset=iso-8859-1\r\n\r\n<html><head><style>p{}</style></head><body><h1>T\xedtulo</h1><p>Um &amp; dois<br>tr\xeas &eacute; &ordm;</p><script>x()</script></body></html>";
        let mail = parse(bytes).unwrap();
        // Block elements separate paragraphs; a line break stays a line break.
        assert_eq!(mail.body, "Título\n\nUm & dois\ntrês é º");
        assert!(parse(b"isto nao e um e-mail").is_err());
        // A pathological run of ampersands is handled in linear time.
        let big = "&".repeat(200_000);
        let started = std::time::Instant::now();
        assert_eq!(html_to_text(&big).len(), 200_000);
        assert!(started.elapsed().as_secs() < 5);
    }

    #[test]
    fn eight_bit_parts_split_names_and_several_texts_survive() {
        // A Latin-1 8-bit text part inside multipart must keep its bytes,
        // a filename split RFC 2231-style is joined, a tab in the subject
        // does not block anything, and two text parts are both kept.
        let mut eml: Vec<u8> = Vec::new();
        eml.extend_from_slice(b"From: a@b\r\nSubject: Com\ttab =?UTF-8?Q?a=C3?= =?UTF-8?Q?=A7=C3=A3o?=\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n");
        eml.extend_from_slice(b"--b1\r\nContent-Type: text/plain; charset=iso-8859-1\r\nContent-Transfer-Encoding: 8bit\r\n\r\nPrimeira parte com \xe7 e \xe3.\r\n");
        eml.extend_from_slice(b"--b1\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment;\r\n filename*0*=utf-8''relat%C3%B3rio%20final;\r\n filename*1*=%20de%202026.txt\r\nContent-Transfer-Encoding: 8bit\r\n\r\nbytes \xff\xfe crus\r\n");
        eml.extend_from_slice(
            b"--b1\r\nContent-Type: text/plain\r\n\r\nSegunda parte.\r\n--b1--\r\n",
        );
        let mail = parse(&eml).unwrap();
        assert_eq!(mail.subject, "Com tab ação");
        assert_eq!(mail.body, "Primeira parte com ç e ã.\n\nSegunda parte.");
        assert_eq!(mail.attachments.len(), 1, "{:?}", mail.notes);
        assert_eq!(mail.attachments[0].name, "relatório final de 2026.txt");
        assert_eq!(mail.attachments[0].data, b"bytes \xff\xfe crus");
        // A nameless attachment gets a name with the right extension.
        let nameless = b"From: a@b\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment\r\n\r\n%PDF-1.4 x\r\n--x--\r\n";
        let mail = parse(nameless).unwrap();
        assert_eq!(mail.attachments[0].name, "anexo-1.pdf");
    }

    #[test]
    fn emails_numbered_as_manual_records_are_renumbered_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        // A plain manual record and an e-mail as 0.22–0.23 numbered it.
        let plain = crate::manual::ManualFields {
            kind: "ligacao".into(),
            title: "Ligação do gabinete".into(),
            origin: "Gabinete".into(),
            document_date: "2026-11-03".into(),
            text: "Pedido por telefone.".into(),
        };
        let plain = store.create_manual_document(&plain).unwrap();
        let mail = store.import_eml_bytes(SAMPLE.as_bytes()).unwrap();
        let id = mail.receipt.document_id;
        let year = chrono::Local::now().format("%Y").to_string();
        let old_number = format!("MANUAL-{year}/0002");
        {
            let detail = store.load_document(id).unwrap();
            let old = crate::storage::tests_support::renumber_for_test(
                &store,
                id,
                &detail.capture,
                &old_number,
            );
            assert!(old);
        }
        assert_eq!(
            store.load_document(id).unwrap().capture.document_number,
            old_number
        );
        // Another old e-mail, in the trash: left as it is, and no failure.
        let other = SAMPLE.replace("To: gabinete@", "To: outro@");
        let trashed_id = store
            .import_eml_bytes(other.as_bytes())
            .unwrap()
            .receipt
            .document_id;
        let trashed_capture = store.load_document(trashed_id).unwrap().capture;
        assert!(crate::storage::tests_support::renumber_for_test(
            &store,
            trashed_id,
            &trashed_capture,
            &format!("MANUAL-{year}/0003"),
        ));
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Document,
                id: trashed_id,
            })
            .unwrap();
        assert_eq!(
            crate::storage::renumber_imported_emails(&mut store).unwrap(),
            1
        );
        let detail = store.load_document(id).unwrap();
        assert_eq!(detail.capture.document_number, format!("EMAIL-{year}/0001"));
        assert_eq!(
            detail.capture.source_url,
            format!("{}EMAIL-{year}/0001", crate::model::MANUAL_ORIGIN)
        );
        assert_eq!(
            store.list_attachments(Owner::Document(id)).unwrap().len(),
            1
        );
        // The plain one keeps its number; nothing runs twice.
        assert!(store
            .load_document(plain.document_id)
            .unwrap()
            .capture
            .document_number
            .starts_with("MANUAL-"));
        assert_eq!(
            crate::storage::renumber_imported_emails(&mut store).unwrap(),
            0
        );
        // Importing the same message again finds the renumbered record.
        let again = store.import_eml_bytes(SAMPLE.as_bytes()).unwrap();
        assert!(again.already_imported);
        assert_eq!(again.receipt.document_id, id);
    }

    #[test]
    fn a_message_from_the_mailbox_records_the_account() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let source = MailSource::Account {
            name: "Gmail pessoal".into(),
            address: "pessoa@gmail.com".into(),
        };
        let done = store.import_mail_bytes(SAMPLE.as_bytes(), &source).unwrap();
        let metadata = store
            .load_document(done.receipt.document_id)
            .unwrap()
            .capture
            .metadata;
        assert_eq!(metadata["mail_account"], "pessoa@gmail.com");
        assert_eq!(metadata["mail_account_name"], "Gmail pessoal");
        assert_eq!(metadata["imported_from"], "conta de e-mail (IMAP)");
        // From a file there is no account, only the recipients.
        let other = SAMPLE.replace("To: gabinete@", "To: outro@");
        let file = store.import_eml_bytes(other.as_bytes()).unwrap();
        let metadata = store
            .load_document(file.receipt.document_id)
            .unwrap()
            .capture
            .metadata;
        assert!(!metadata.contains_key("mail_account"));
        assert_eq!(metadata["imported_from"], "arquivo .eml");
    }

    #[test]
    fn a_different_message_with_a_known_message_id_never_joins_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let first = store.import_eml_bytes(SAMPLE.as_bytes()).unwrap();
        let original = Owner::Document(first.receipt.document_id);
        assert_eq!(store.list_attachments(original).unwrap().len(), 1);
        // Anyone on the thread knows the Message-ID; the content differs.
        let forged = SAMPLE.replace("To: gabinete@", "To: outro@");
        assert_ne!(forged, SAMPLE);
        let second = store.import_eml_bytes(forged.as_bytes()).unwrap();
        assert!(second.separate_record);
        assert!(!second.already_imported);
        assert_ne!(second.receipt.document_id, first.receipt.document_id);
        assert_eq!(store.list_attachments(original).unwrap().len(), 1);
        assert!(second.notes.iter().any(|n| n.contains("registro separado")));
        // That message again is recognized as itself.
        let third = store.import_eml_bytes(forged.as_bytes()).unwrap();
        assert!(third.already_imported && third.separate_record);
        assert_eq!(third.receipt.document_id, second.receipt.document_id);
        assert_eq!(third.attachments_added, 0);
        assert_eq!(store.list_documents("").unwrap().len(), 2);
    }

    #[test]
    fn an_eml_becomes_a_manual_record_with_attachments_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let first = store.import_eml_bytes(SAMPLE.as_bytes()).unwrap();
        assert!(!first.already_imported);
        assert_eq!(
            first.attachments_added, 1,
            "{:?}",
            first.attachments_skipped
        );
        let detail = store.load_document(first.receipt.document_id).unwrap();
        assert!(detail.capture.is_manual());
        // E-mails have a numbering of their own.
        assert!(
            crate::model::is_email_number(&detail.capture.document_number),
            "{}",
            detail.capture.document_number
        );
        assert!(crate::model::valid_manual_number(
            &detail.capture.document_number
        ));
        assert_eq!(detail.capture.title, "Pedido de material de escritório");
        assert_eq!(detail.capture.metadata["manual_kind"], "email");
        assert_eq!(detail.capture.metadata["document_type"], "E-mail recebido");
        assert_eq!(detail.capture.metadata["document_date"], "2026-11-03");
        assert_eq!(
            detail.capture.metadata["mail_message_id"],
            "abc123@exemplo.gov.br"
        );
        assert!(detail.capture.text.starts_with("De: João Silva"));
        assert!(detail.capture.text.contains("Anexos: ofício 12.pdf"));
        let attachments = store
            .list_attachments(Owner::Document(first.receipt.document_id))
            .unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "ofício 12.pdf");
        assert_eq!(attachments[0].kind, "PDF");
        // The same message again: recognized, nothing duplicated.
        let second = store.import_eml_bytes(SAMPLE.as_bytes()).unwrap();
        assert!(second.already_imported);
        assert_eq!(second.attachments_added, 0);
        assert_eq!(second.receipt.document_id, first.receipt.document_id);
        assert_eq!(store.list_documents("").unwrap().len(), 1);
        assert_eq!(
            store
                .list_attachments(Owner::Document(first.receipt.document_id))
                .unwrap()
                .len(),
            1
        );
        // A re-import completes what was not attached before.
        store
            .delete_attachment(attachments[0].id)
            .or_else(|_| {
                store
                    .trash_item(crate::trash::Target {
                        kind: crate::trash::Kind::Attachment,
                        id: attachments[0].id,
                    })
                    .map(|_| ())
            })
            .unwrap();
        let third = store.import_eml_bytes(SAMPLE.as_bytes()).unwrap();
        assert!(third.already_imported);
        assert_eq!(
            third.attachments_added, 1,
            "{:?}",
            third.attachments_skipped
        );
        // The example text leaves the addresses out.
        let text = example_text(SAMPLE.as_bytes()).unwrap();
        assert!(text.starts_with("Assunto: Pedido"));
        assert!(!text.contains("joao@"));
    }
}
