//! The user guide. `GUIA_DO_USUARIO.md` is compiled into the program, so the
//! guide always matches the installed version and needs no network; "Guia do
//! usuário" in the menu turns it into a web page and opens it in the browser,
//! where links work and the page can be printed.
//!
//! The converter understands only what the guide uses: headings, paragraphs,
//! bullet and numbered lists (with one nested level), tables, quotes, rules
//! and, inside a line, bold, italics, code and links.
#[cfg(feature = "desktop")]
use anyhow::{Context, Result};
#[cfg(feature = "desktop")]
use std::path::PathBuf;

pub const GUIDE: &str = include_str!("../GUIA_DO_USUARIO.md");
#[cfg(feature = "desktop")]
/// Writes the guide as a web page in the data folder and opens it with the
/// program the person uses for web pages.
pub fn open() -> Result<PathBuf> {
    let dir = crate::paths::data_dir()?.join("guia");
    std::fs::create_dir_all(&dir).context("Não foi possível criar a pasta do guia.")?;
    let path = dir.join("guia-do-usuario.html");
    std::fs::write(&path, to_html(GUIDE, env!("CARGO_PKG_VERSION")))
        .context("Não foi possível gravar o guia.")?;
    crate::windows_ui::open_with_default_app(&path)
        .context("O Windows não conseguiu abrir o guia no navegador.")?;
    Ok(path)
}

const STYLE: &str = "body{font:16px/1.55 system-ui,'Segoe UI',sans-serif;color:#1c2733;background:#f4f6f8;margin:0}\
header{background:#185461;color:#fff;padding:14px 24px;font-weight:600}header small{font-weight:400;opacity:.85;margin-left:8px}\
.page{max-width:900px;margin:0 auto;padding:8px 24px 48px;background:#fff;box-shadow:0 0 24px #0001}\
nav{background:#eef5f6;border-left:4px solid #185461;padding:10px 18px;margin:18px 0;font-size:15px}nav ol{margin:6px 0;padding-left:22px;columns:2}nav li{margin:0;padding:3px 0;break-inside:avoid}\
h1{font-size:28px;margin:22px 0 8px;color:#185461}h2{font-size:21px;margin:34px 0 8px;padding-top:10px;border-top:1px solid #dfe5ea;color:#185461}h3{font-size:17px;margin:20px 0 6px}\
p{margin:10px 0}li{margin:5px 0}a{color:#12579e}\
code{background:#eef1f4;border-radius:4px;padding:1px 5px;font:14px Consolas,monospace}\
table{border-collapse:collapse;width:100%;margin:14px 0;font-size:15px}th,td{border:1px solid #cfd8df;padding:7px 10px;text-align:left;vertical-align:top}th{background:#eef5f6}\
blockquote{margin:12px 0;padding:6px 16px;border-left:4px solid #cfd8df;color:#45525f}hr{border:0;border-top:1px solid #dfe5ea;margin:26px 0}\
@media print{body{background:#fff}header{background:#fff;color:#000;border-bottom:1px solid #000}.page{box-shadow:none;max-width:none}nav{display:none}h2{break-after:avoid}tr{break-inside:avoid}}";

/// The whole page: a header with the version, a list of the sections and the
/// guide itself.
pub fn to_html(markdown: &str, version: &str) -> String {
    let (body, sections) = render(markdown);
    let mut contents = String::new();
    if !sections.is_empty() {
        contents.push_str("<nav><strong>Neste guia</strong><ol>");
        for (id, title) in &sections {
            // Section titles already carry their number ("3. Importar…").
            let title = title
                .split_once(". ")
                .filter(|(number, _)| number.chars().all(|c| c.is_ascii_digit()))
                .map_or(title.as_str(), |(_, rest)| rest);
            contents.push_str(&format!("<li><a href=\"#{id}\">{}</a></li>", escape(title)));
        }
        contents.push_str("</ol></nav>");
    }
    format!(
        "<!doctype html>\n<html lang=\"pt-BR\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>Ponte — Guia do usuário</title><style>{STYLE}</style></head>\
<body><header>Ponte<small>Guia do usuário • versão {}</small></header>\
<div class=\"page\">{contents}{body}</div></body></html>\n",
        escape(version)
    )
}

/// Body of the page plus the second-level headings, as (anchor, title).
fn render(markdown: &str) -> (String, Vec<(String, String)>) {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = String::new();
    let mut sections = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            i += 1;
        } else if let Some((level, text)) = heading(trimmed) {
            if level == 2 {
                let id = format!("s{}", sections.len() + 1);
                out.push_str(&format!("<h2 id=\"{id}\">{}</h2>\n", inline(text)));
                sections.push((id, text.to_owned()));
            } else {
                out.push_str(&format!("<h{level}>{}</h{level}>\n", inline(text)));
            }
            i += 1;
        } else if trimmed == "---" {
            out.push_str("<hr>\n");
            i += 1;
        } else if trimmed.starts_with('|') {
            let start = i;
            while i < lines.len() && lines[i].trim().starts_with('|') {
                i += 1;
            }
            out.push_str(&table(&lines[start..i]));
        } else if trimmed.starts_with("> ") || trimmed == ">" {
            let mut quote = String::new();
            while i < lines.len() && lines[i].trim().starts_with('>') {
                if !quote.is_empty() {
                    quote.push(' ');
                }
                quote.push_str(lines[i].trim().trim_start_matches('>').trim());
                i += 1;
            }
            out.push_str(&format!("<blockquote>{}</blockquote>\n", inline(&quote)));
        } else if marker(lines[i]).is_some() {
            i = list(&lines, i, &mut out);
        } else {
            let mut text = String::new();
            while i < lines.len() && !starts_block(lines[i]) {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(lines[i].trim());
                i += 1;
            }
            out.push_str(&format!("<p>{}</p>\n", inline(&text)));
        }
    }
    (out, sections)
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    if (1..=3).contains(&level) {
        line[level..]
            .strip_prefix(' ')
            .map(|text| (level, text.trim()))
    } else {
        None
    }
}

/// Where a paragraph ends: a blank line or the start of another block.
fn starts_block(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty()
        || heading(trimmed).is_some()
        || trimmed == "---"
        || trimmed.starts_with('|')
        || trimmed.starts_with('>')
        || marker(line).is_some()
}

/// A list item: (indentation, numbered?, text after the marker).
fn marker(line: &str) -> Option<(usize, bool, &str)> {
    let rest = line.trim_start();
    let indent = line.len() - rest.len();
    if let Some(text) = rest.strip_prefix("- ") {
        return Some((indent, false, text.trim()));
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        if let Some(text) = rest[digits..].strip_prefix(". ") {
            return Some((indent, true, text.trim()));
        }
    }
    None
}

/// One list starting at `lines[i]`, with items indented further as a nested
/// list of the item above. Returns the index after the list.
fn list(lines: &[&str], mut i: usize, out: &mut String) -> usize {
    let Some((base, ordered, _)) = marker(lines[i]) else {
        return i + 1;
    };
    let tag = |ordered: bool| if ordered { "ol" } else { "ul" };
    out.push_str(&format!("<{}>", tag(ordered)));
    while i < lines.len() {
        let Some((indent, _, text)) = marker(lines[i]) else {
            break;
        };
        if indent != base {
            break;
        }
        out.push_str(&format!("<li>{}", inline(text)));
        i += 1;
        if let Some((nested_indent, nested_ordered, _)) = lines.get(i).and_then(|line| marker(line))
        {
            if nested_indent > base {
                out.push_str(&format!("<{}>", tag(nested_ordered)));
                while let Some((inner, _, text)) = lines.get(i).and_then(|line| marker(line)) {
                    if inner <= base {
                        break;
                    }
                    out.push_str(&format!("<li>{}</li>", inline(text)));
                    i += 1;
                }
                out.push_str(&format!("</{}>", tag(nested_ordered)));
            }
        }
        out.push_str("</li>");
    }
    out.push_str(&format!("</{}>\n", tag(ordered)));
    i
}

fn table(rows: &[&str]) -> String {
    let cells = |row: &str| -> Vec<String> {
        row.trim()
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_owned())
            .collect()
    };
    let is_rule = |row: &str| {
        cells(row)
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'))
    };
    let mut out = String::from("<table>");
    let mut header = rows.len() > 1 && is_rule(rows[1]);
    for row in rows {
        if is_rule(row) {
            continue;
        }
        let tag = if header { "th" } else { "td" };
        out.push_str("<tr>");
        for cell in cells(row) {
            out.push_str(&format!("<{tag}>{}</{tag}>", inline(&cell)));
        }
        out.push_str("</tr>");
        header = false;
    }
    out.push_str("</table>\n");
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Only web addresses become links; anything else stays as written.
fn web_address(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '"' || c == '<' || c == '>')
}

/// Position of `needle` in `chars` at or after `from`.
fn find(chars: &[char], from: usize, needle: &[char]) -> Option<usize> {
    if needle.is_empty() || from > chars.len() {
        return None;
    }
    (from..chars.len().saturating_sub(needle.len() - 1))
        .find(|&at| chars[at..at + needle.len()] == *needle)
}

/// Bold, italics, code and links inside one line; everything else escaped.
fn inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let piece = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '`' => {
                if let Some(end) = find(&chars, i + 1, &['`']) {
                    out.push_str(&format!("<code>{}</code>", escape(&piece(i + 1, end))));
                    i = end + 1;
                    continue;
                }
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                if let Some(end) = find(&chars, i + 2, &['*', '*']) {
                    if end > i + 2 {
                        out.push_str(&format!("<strong>{}</strong>", inline(&piece(i + 2, end))));
                        i = end + 2;
                        continue;
                    }
                }
            }
            '*' if chars.get(i + 1).is_some_and(|c| !c.is_whitespace()) => {
                if let Some(end) = find(&chars, i + 1, &['*']) {
                    if end > i + 1 && !chars[end - 1].is_whitespace() {
                        out.push_str(&format!("<em>{}</em>", inline(&piece(i + 1, end))));
                        i = end + 1;
                        continue;
                    }
                }
            }
            '[' => {
                if let Some(close) = find(&chars, i + 1, &[']', '(']) {
                    if let Some(end) = find(&chars, close + 2, &[')']) {
                        let url = piece(close + 2, end);
                        if web_address(&url) {
                            out.push_str(&format!(
                                "<a href=\"{}\">{}</a>",
                                escape(&url),
                                inline(&piece(i + 1, close))
                            ));
                            i = end + 1;
                            continue;
                        }
                    }
                }
            }
            '<' => {
                if let Some(end) = find(&chars, i + 1, &['>']) {
                    let url = piece(i + 1, end);
                    if web_address(&url) {
                        out.push_str(&format!("<a href=\"{0}\">{0}</a>", escape(&url)));
                        i = end + 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        out.push_str(&escape(&chars[i].to_string()));
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_marks_become_tags_and_everything_else_is_escaped() {
        assert_eq!(
            inline("**Salvar** e `a<b` com *ênfase*"),
            "<strong>Salvar</strong> e <code>a&lt;b</code> com <em>ênfase</em>"
        );
        assert_eq!(
            inline("<script>alert(1)</script> & \"x\""),
            "&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;x&quot;"
        );
        assert_eq!(
            inline("veja <https://exemplo.gov.br/a?b=1&c=2>"),
            "veja <a href=\"https://exemplo.gov.br/a?b=1&amp;c=2\">https://exemplo.gov.br/a?b=1&amp;c=2</a>"
        );
        assert_eq!(
            inline("[Guia](https://exemplo.gov.br)"),
            "<a href=\"https://exemplo.gov.br\">Guia</a>"
        );
        // Only web addresses are linked.
        assert_eq!(
            inline("[x](javascript:alert(1))"),
            "[x](javascript:alert(1))"
        );
        assert_eq!(inline("<file:///c:/x>"), "&lt;file:///c:/x&gt;");
        // Unfinished marks stay as typed.
        assert_eq!(inline("2 * 3 e **sem fim"), "2 * 3 e **sem fim");
        assert_eq!(inline("`aberto"), "`aberto");
    }

    #[test]
    fn blocks_are_recognized() {
        let (body, sections) = render(
            "# Título\n\nParágrafo em\nduas linhas.\n\n## 1. Primeira\n\n1. um\n   - a\n   - b\n2. dois\n\n- solto\n\n| A | B |\n|---|---|\n| 1 | **2** |\n\n> citação\n\n---\n",
        );
        assert!(body.contains("<h1>Título</h1>"), "{body}");
        assert!(body.contains("<p>Parágrafo em duas linhas.</p>"), "{body}");
        assert!(body.contains("<h2 id=\"s1\">1. Primeira</h2>"), "{body}");
        assert!(
            body.contains("<ol><li>um<ul><li>a</li><li>b</li></ul></li><li>dois</li></ol>"),
            "{body}"
        );
        assert!(body.contains("<ul><li>solto</li></ul>"), "{body}");
        assert!(
            body.contains("<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td><strong>2</strong></td></tr></table>"),
            "{body}"
        );
        assert!(body.contains("<blockquote>citação</blockquote>"), "{body}");
        assert!(body.contains("<hr>"), "{body}");
        assert_eq!(sections, vec![("s1".to_owned(), "1. Primeira".to_owned())]);
    }

    #[test]
    fn the_shipped_guide_converts_cleanly() {
        let page = to_html(GUIDE, "9.9.9");
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("versão 9.9.9"));
        assert!(page.contains("<h1>Ponte — Guia do usuário</h1>"));
        // Every section is listed and anchored.
        let (_, sections) = render(GUIDE);
        assert_eq!(sections.len(), 10, "{sections:?}");
        for (id, _) in &sections {
            assert!(
                page.contains(&format!("href=\"#{id}\"")) && page.contains(&format!("id=\"{id}\""))
            );
        }
        assert!(page.contains("<a href=\"https://github.com/demusis/ponteproj/releases/latest/download/Ponte-Setup.exe\">"));
        // Nothing of the source notation is left visible.
        let (body, _) = render(GUIDE);
        assert!(!body.contains("**"), "bold left unconverted");
        assert!(!body.contains("|---"), "table rule left visible");
        assert!(!body.contains("\n## "), "heading left unconverted");
        assert!(!body.contains('`'), "code mark left unconverted");
    }
}
