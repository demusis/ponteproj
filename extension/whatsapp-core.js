(function (root, factory) {
  const api = factory(typeof module === "object" && module.exports ? require("./capture-core.js") : root.PonteCapture);
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.PonteWhatsApp = api;
})(globalThis, function (Core) {
  "use strict";
  // Pure logic of the WhatsApp Web feature: the reader of WhatsApp's copied-text format
  // (same rules and same test vectors as the Rust reader of the Ponte), the record and
  // attachment rules of the contract with the Ponte, and the reader of the WhatsApp Web page. Nothing here touches the network, storage or the real page: the
  // page adapter works on a Document/Element passed in by the caller.
  const ORIGIN = "https://web.whatsapp.com";
  const MAX_MESSAGES = 500;
  const MAX_TEXT_BYTES = 500 * 1024;
  const TITLE_MIN = 3, TITLE_MAX = 500, ORIGIN_MAX = 200, META_MAX = 200;
  const MAX_ATTACHMENTS = 10;
  const MAX_FILE_BYTES = 8 * 1024 * 1024;
  const MAX_TOTAL_BYTES = 16 * 1024 * 1024;
  const MAX_NAME_CHARS = 180;
  const YEAR_MIN = 2000, YEAR_MAX = 2099;
  const META_KEYS = ["wa_chat", "wa_first", "wa_last", "wa_messages"];
  const RECORD_KEYS = ["kind", "title", "origin", "document_date", "text", "meta"];
  class WhatsAppError extends Error {
    constructor(code, message) { super(message); this.name = "WhatsAppError"; this.code = code; }
  }
  function fail(code, message) { throw new WhatsAppError(code, message); }
  const plain = value => !!value && typeof value === "object" && !Array.isArray(value) && [Object.prototype, null].includes(Object.getPrototypeOf(value));
  const exactKeys = (value, keys) => plain(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
  const utf8Length = value => new TextEncoder().encode(value).length;
  const chars = value => Array.from(value).length;
  // A lone surrogate is not valid text for the Ponte's JSON parser (same repair as the note of capture-core.js).
  const LONE_SURROGATE_RE = /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;
  function wellFormed(text) {
    if (Core && typeof Core.wellFormed === "function") return Core.wellFormed(text);
    return typeof text.toWellFormed === "function" ? text.toWellFormed() : text.replace(new RegExp(LONE_SURROGATE_RE.source, "g"), "\ufffd");
  }
  // Direction marks WhatsApp inserts, the byte-order mark and zero-width characters.
  const MARKS_RE = /[\u200e\u200f\u202a-\u202e\u2066-\u2069\ufeff]/g;
  const CONTROL_RE = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/g;
  const HAS_CONTROL_RE = /[\u0000-\u001f\u007f-\u009f]/;
  const BIDI_AND_ZERO_RE = /[\u200b-\u200f\u202a-\u202e\u2060-\u2069\ufeff]/g;

  // ---------------------------------------------------------------- the reader of copied text
  const TIME_SRC = String.raw`(\d{1,2}):(\d{2})(?::(\d{2}))?(?:\s*([ap])\s*\.?\s*m\.?)?`;
  const DATE_SRC = String.raw`(\d{1,2})[/.\-](\d{1,2})[/.\-](\d{4}|\d{2})`;
  const TIME_FIRST_RE = new RegExp(`^\\s*${TIME_SRC}\\s*[,;]?\\s*${DATE_SRC}\\s*$`, "i");
  const DATE_FIRST_RE = new RegExp(`^\\s*${DATE_SRC}\\s*[,;]?\\s*${TIME_SRC}\\s*$`, "i");
  const pad = (value, size = 2) => String(value).padStart(size, "0");
  // dd/mm only; never mm/dd. Returns "AAAA-MM-DDTHH:MM" or null when anything is impossible.
  function whenFrom(day, month, year, hour, minute, second, marker) {
    let y = Number(year);
    if (year.length === 2) y += 2000;
    const mo = Number(month), d = Number(day);
    let h = Number(hour);
    const mi = Number(minute);
    if (second !== undefined && Number(second) > 59) return null;
    if (mi > 59) return null;
    if (marker) {
      if (h < 1 || h > 12) return null;
      h = h % 12 + (marker.toLowerCase() === "p" ? 12 : 0);
    } else if (h > 23) return null;
    if (y < YEAR_MIN || y > YEAR_MAX || mo < 1 || mo > 12 || d < 1) return null;
    if (d > new Date(Date.UTC(y, mo, 0)).getUTCDate()) return null;
    return `${pad(y, 4)}-${pad(mo)}-${pad(d)}T${pad(h)}:${pad(mi)}`;
  }
  function cleanLines(text) {
    return String(text).replace(MARKS_RE, "").replace(/\r\n?/g, "\n").replace(CONTROL_RE, "");
  }
  // One line that starts a message: "[hh:mm, dd/mm/aaaa] Nome: texto" (or the date-first form).
  // Returns { when, author, text } or null when the line does not start a message.
  function parseHeader(line) {
    if (typeof line !== "string" || line[0] !== "[") return null;
    const close = line.indexOf("]");
    if (close < 0) return null;
    const inner = line.slice(1, close).replace(/[\u00a0\u2009\u202f]/g, " ");
    let when = null;
    let match = TIME_FIRST_RE.exec(inner);
    if (match) when = whenFrom(match[5], match[6], match[7], match[1], match[2], match[3], match[4]);
    else if ((match = DATE_FIRST_RE.exec(inner))) when = whenFrom(match[1], match[2], match[3], match[4], match[5], match[6], match[7]);
    if (!when) return null;
    let rest = line.slice(close + 1).replace(/[ \t]+$/, "");
    if (rest[0] === " " || rest[0] === "\u00a0") rest = rest.slice(1);
    const split = /^(.+?)(?:: |:$)(.*)$/.exec(rest);
    if (split && split[1].trim()) return { when, author: split[1].trim(), text: split[2] };
    return { when, author: null, text: rest };
  }
  function finishText(lines) {
    const out = lines.map(line => line.replace(/[ \t]+$/, ""));
    while (out.length && out[out.length - 1] === "") out.pop();
    return out.join("\n");
  }
  // The text WhatsApp puts on the clipboard -> messages, oldest first as copied.
  // Refuses (code NOT_WHATSAPP) only when no line starts a message.
  function parseCopied(input) {
    const lines = cleanLines(input).split("\n");
    const messages = [];
    let current = null;
    let any = false;
    for (const raw of lines) {
      const line = raw.replace(/[ \t]+$/, "");
      const head = parseHeader(line);
      if (head) {
        any = true;
        if (current) current.text = finishText(current.lines);
        current = { when: head.when, author: head.author, lines: [head.text] };
        messages.push(current);
      } else if (current) current.lines.push(line);
      else if (line.trim() !== "") { current = { when: null, author: null, lines: [line] }; messages.push(current); }
    }
    if (!any) fail("NOT_WHATSAPP", "O texto colado não parece ser de mensagens do WhatsApp. Selecione as mensagens no WhatsApp, copie (Ctrl+C) e cole aqui.");
    return messages.map(message => ({ when: message.when, author: message.author, text: finishText(message.lines) }));
  }

  // ---------------------------------------------------------------- record
  const dateOf = when => `${when.slice(8, 10)}/${when.slice(5, 7)}/${when.slice(0, 4)}`;
  const timeOf = when => when.slice(11, 16);
  const formatWhen = when => when ? `${dateOf(when)} ${timeOf(when)}` : "sem data";
  const datedMessages = messages => messages.filter(message => typeof message.when === "string");
  function firstLast(messages) {
    const dated = datedMessages(messages);
    return { first: dated.length ? dated[0].when : "", last: dated.length ? dated[dated.length - 1].when : "" };
  }
  function participants(messages) {
    const names = [];
    for (const message of messages) if (message.author && !names.includes(message.author)) names.push(message.author);
    return names;
  }
  // Exactly `[dd/mm/aaaa hh:mm] Nome: texto`; a system line is `[...] — texto`; a line without date `[sem data] — texto`.
  function recordText(messages) {
    return messages.map(message => {
      const stamp = message.when ? `[${formatWhen(message.when)}]` : "[sem data]";
      return `${stamp} ${message.author ? message.author + ": " : "— "}${message.text}`;
    }).join("\n").replace(/[ \t]+$/gm, "");
  }
  function cleanChatName(value) {
    let name = wellFormed(String(value || "")).replace(MARKS_RE, "").replace(CONTROL_RE, " ").replace(/\s+/g, " ").trim();
    if (chars(name) > ORIGIN_MAX) name = Array.from(name).slice(0, ORIGIN_MAX).join("").trim();
    return name;
  }
  function cleanTitle(value) {
    return wellFormed(String(value || "")).replace(MARKS_RE, "").replace(CONTROL_RE, " ").replace(/\s+/g, " ").trim();
  }
  // "Conversa com <conversa> — dd/mm/aaaa" (date of the last dated message).
  function suggestedTitle(chat, messages) {
    const name = cleanChatName(chat);
    const { last } = firstLast(Array.isArray(messages) ? messages : []);
    const base = name ? `Conversa com ${name}` : "Conversa do WhatsApp";
    return last ? `${base} \u2014 ${dateOf(last)}` : base;
  }
  // The messages as the record will hold them: clean text, no marks, no control characters.
  function normalizeMessage(message) {
    if (!plain(message) || (message.when !== null && !(typeof message.when === "string" && /^\d{4}-\d\d-\d\dT\d\d:\d\d$/.test(message.when))) || typeof message.text !== "string" || !(message.author === null || typeof message.author === "string")) fail("RECORD_INVALID", "As mensagens não puderam ser organizadas. Tente de novo.");
    const text = finishText(cleanLines(wellFormed(message.text)).split("\n"));
    let author = message.author === null ? "" : cleanLines(wellFormed(message.author)).replace(/\s+/g, " ").trim();
    return { when: message.when, author: author || null, text };
  }
  // The `record` object of the contract. Throws WhatsAppError with a sentence for the person.
  function buildRecord({ title, origin, messages }) {
    if (!Array.isArray(messages) || !messages.length) fail("NO_MESSAGES", "Não há mensagens para guardar.");
    if (messages.length > MAX_MESSAGES) fail("TOO_MANY", `Há ${messages.length} mensagens; o limite é ${MAX_MESSAGES}. Escolha um trecho menor.`);
    const clean = messages.map(normalizeMessage);
    const text = recordText(clean);
    if (utf8Length(text) > MAX_TEXT_BYTES) fail("TOO_BIG", "O trecho tem texto demais (limite de 500 KB). Escolha um trecho menor.");
    const name = cleanChatName(origin);
    let heading = cleanTitle(title);
    if (!heading) heading = suggestedTitle(name, clean);
    if (chars(heading) < TITLE_MIN) fail("TITLE_SHORT", "O título precisa ter pelo menos 3 caracteres.");
    if (chars(heading) > TITLE_MAX) fail("TITLE_LONG", "O título passou de 500 caracteres. Escreva um título mais curto.");
    const { first, last } = firstLast(clean);
    return { kind: "whatsapp", title: heading, origin: name, document_date: last ? last.slice(0, 10) : "", text, meta: { wa_chat: name, wa_first: first, wa_last: last, wa_messages: String(clean.length) } };
  }
  const validDate = value => {
    const match = /^(\d{4})-(\d\d)-(\d\d)$/.exec(value);
    return !!match && new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]))).toISOString().slice(0, 10) === value;
  };
  const validMinute = value => /^\d{4}-\d\d-\d\dT([01]\d|2[0-3]):[0-5]\d$/.test(value) && validDate(value.slice(0, 10));
  // Exact keys at every level, types, limits and characters; nothing is repaired here.
  function validateRecord(record) {
    const bad = what => fail("RECORD_INVALID", `Os dados da conversa não puderam ser enviados (${what}). Escolha as mensagens de novo.`);
    if (!exactKeys(record, RECORD_KEYS) || record.kind !== "whatsapp") bad("formato");
    for (const key of ["title", "origin", "document_date", "text"]) if (typeof record[key] !== "string") bad(key);
    const texts = [record.title, record.origin, record.document_date, record.text];
    if (texts.some(value => LONE_SURROGATE_RE.test(value))) bad("texto inválido");
    if (HAS_CONTROL_RE.test(record.title + record.origin + record.document_date) || /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/.test(record.text) || /[\u200e\u200f\u202a-\u202e\u2066-\u2069\ufeff]/.test(record.title + record.origin + record.text)) bad("caracteres de controle");
    if (chars(record.title) < TITLE_MIN || chars(record.title) > TITLE_MAX || record.title !== record.title.trim()) bad("título");
    if (chars(record.origin) > ORIGIN_MAX || record.origin !== record.origin.trim()) bad("nome da conversa");
    if (record.document_date !== "" && !validDate(record.document_date)) bad("data");
    if (!record.text || utf8Length(record.text) > MAX_TEXT_BYTES) bad("texto");
    if (!exactKeys(record.meta, META_KEYS)) bad("informações");
    for (const key of META_KEYS) if (typeof record.meta[key] !== "string" || chars(record.meta[key]) > META_MAX || HAS_CONTROL_RE.test(record.meta[key])) bad(key);
    if (record.meta.wa_chat !== record.origin) bad("conversa");
    for (const key of ["wa_first", "wa_last"]) if (record.meta[key] !== "" && !validMinute(record.meta[key])) bad(key);
    if (!/^[1-9]\d{0,2}$/.test(record.meta.wa_messages) || Number(record.meta.wa_messages) > MAX_MESSAGES) bad("quantidade");
    if (record.document_date !== record.meta.wa_last.slice(0, 10)) bad("data");
    return record;
  }

  // ---------------------------------------------------------------- attachments
  // Last part of a path, no control or direction characters, at most 180 characters (the extension is kept).
  function sanitizeFileName(value) {
    let name = wellFormed(String(value || "")).split(/[\\/]/).pop().replace(CONTROL_RE, "").replace(BIDI_AND_ZERO_RE, "").replace(/\s+/g, " ").trim().replace(/[. ]+$/, "");
    if (!name || /^\.+$/.test(name)) name = "arquivo";
    if (chars(name) > MAX_NAME_CHARS) {
      const dot = name.lastIndexOf(".");
      const ext = dot > 0 && chars(name.slice(dot)) <= 20 ? name.slice(dot) : "";
      name = Array.from(name.slice(0, ext ? dot : name.length)).slice(0, MAX_NAME_CHARS - chars(ext)).join("").trim() + ext;
    }
    return name;
  }
  // Same name twice becomes "nome (2).ext".
  function uniqueName(name, taken) {
    if (!taken.includes(name)) return name;
    const dot = name.lastIndexOf(".");
    const stem = dot > 0 ? name.slice(0, dot) : name, ext = dot > 0 ? name.slice(dot) : "";
    for (let n = 2; ; n++) {
      const candidate = sanitizeFileName(`${stem} (${n})${ext}`);
      if (!taken.includes(candidate)) return candidate;
    }
  }
  function toBase64(bytes) {
    let binary = "";
    for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode.apply(null, bytes.subarray(i, i + 8192));
    return btoa(binary);
  }
  const BASE64_RE = /^[A-Za-z0-9+/]*={0,2}$/;
  // Decoded size of a canonical base64 string, or -1 when it is not one.
  function decodedLength(value) {
    if (typeof value !== "string" || value.length % 4 !== 0 || !BASE64_RE.test(value)) return -1;
    return value.length / 4 * 3 - (value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0);
  }
  // The list sent with the record: [{ name, data_base64 }], at most 10, each at most 8 MiB, all at most 16 MiB.
  function validateAttachments(list) {
    const bad = text => fail("ATTACHMENT_INVALID", text);
    if (!Array.isArray(list)) bad("Os arquivos não puderam ser enviados. Tente de novo.");
    if (list.length > MAX_ATTACHMENTS) bad(`São ${list.length} arquivos; o limite é ${MAX_ATTACHMENTS}. Tire alguns.`);
    let total = 0;
    const names = [];
    for (const item of list) {
      if (!exactKeys(item, ["name", "data_base64"]) || typeof item.name !== "string" || item.name === "" || item.name !== sanitizeFileName(item.name) || names.includes(item.name)) bad("Um dos arquivos tem nome inválido. Tire-o e junte de novo.");
      const size = decodedLength(item.data_base64);
      if (size < 1) bad(`O arquivo ${item.name} está vazio ou danificado.`);
      if (size > MAX_FILE_BYTES) bad(`O arquivo ${item.name} passa de 8 MB.`);
      total += size;
      names.push(item.name);
    }
    if (total > MAX_TOTAL_BYTES) bad("Os arquivos juntos passam de 16 MB. Tire alguns.");
    return total;
  }
  // Photo types the Ponte accepts (png, jpg, gif, bmp, tif); "webp" is recognised only to be left out.
  function sniffImageType(bytes) {
    const at = (index, ...values) => values.every((value, i) => bytes[index + i] === value);
    if (bytes.length < 12) return null;
    if (at(0, 0xff, 0xd8, 0xff)) return "jpg";
    if (at(0, 0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)) return "png";
    if (at(0, 0x47, 0x49, 0x46, 0x38) && (bytes[4] === 0x37 || bytes[4] === 0x39) && bytes[5] === 0x61) return "gif";
    if (at(0, 0x42, 0x4d)) return "bmp";
    if (at(0, 0x49, 0x49, 0x2a, 0x00) || at(0, 0x4d, 0x4d, 0x00, 0x2a)) return "tif";
    if (at(0, 0x52, 0x49, 0x46, 0x46) && at(8, 0x57, 0x45, 0x42, 0x50)) return "webp";
    return null;
  }
  const ACCEPTED_PHOTO_TYPES = ["jpg", "png", "gif", "bmp", "tif"];
  // Only pictures of this very page: blob: URLs created by https://web.whatsapp.com.
  function isOwnBlobUrl(value) {
    return typeof value === "string" && /^blob:https:\/\/web\.whatsapp\.com\/[0-9A-Za-z._~-]{1,100}$/.test(value);
  }
  function allowedOrigin(value) {
    try { const url = new URL(value); return url.origin === ORIGIN && !url.username && !url.password; } catch { return false; }
  }

  // ---------------------------------------------------------------- page adapter (WhatsApp Web)
  // Built on what was seen on the real page (11/11/2026). The facts this code relies on,
  // to be re-checked when WhatsApp changes:
  //  1. Everything of the conversation is inside #main; a message is a [role="row"] that
  //     contains a [data-id] element and a [data-testid="msg-container"].
  //  2. A text message holds div.copyable-text[data-pre-plain-text="[hh:mm, dd/mm/aaaa] Autor: "]
  //     with the text in a [data-testid="selectable-text"] (class selectable-text) descendant;
  //     the same div also holds an aria-hidden filler with the time, so the text is read ONLY
  //     from the selectable-text part. Emoji are <img alt>.
  //  3. A photo row has [data-testid="image-thumb"] with a data: thumbnail and the real picture
  //     as src="blob:https://web.whatsapp.com/<uuid>"; only that blob is taken. A photo or a
  //     document without caption has no data-pre-plain-text: its time is in [data-testid="msg-meta"],
  //     the date comes from the nearest row that has one. A document row has
  //     [data-testid="document-thumb"] and its name in a div[title]; it is never fetched.
  //  4. The chat name is the text of [data-testid="conversation-info-header-chat-title"] (the
  //     "(você)" label is a sibling, [data-testid="you-label"]).
  //  5. An author label of a row without caption is a span[aria-label] ending in ":".
  // CSS class names are obfuscated and never used (except copyable-text/selectable-text).
  // These functions only read the DOM they are given, only when called (the caller calls them
  // inside a click). They never write, click or scroll. Any doubt returns null (the panel then
  // offers the paste box).
  const ATTR = "data-pre-plain-text";
  const MAX_ROWS = 3000, MAX_DEPTH = 60;
  const BLOCK_TAGS = new Set(["DIV", "P", "LI", "UL", "OL", "BLOCKQUOTE", "PRE", "H1", "H2", "H3", "TABLE", "TR"]);
  const SKIP_TAGS = new Set(["SCRIPT", "STYLE", "TEMPLATE", "NOSCRIPT"]);
  const SELECTABLE = '[data-testid="selectable-text"], .selectable-text';
  function textOf(node, out, depth) {
    if (depth > MAX_DEPTH) return;
    for (const child of node.childNodes) {
      if (child.nodeType === 3) { out.push(child.nodeValue || ""); continue; }
      if (child.nodeType !== 1) continue;
      const tag = String(child.tagName).toUpperCase();
      if (SKIP_TAGS.has(tag) || child.hasAttribute("hidden") || child.getAttribute("aria-hidden") === "true") continue;
      if (tag === "IMG") {
        // Emoji are pictures with the emoji as alt text; a blob: picture is a photo, not text.
        if (!String(child.getAttribute("src") || "").startsWith("blob:")) out.push(child.getAttribute("alt") || "");
        continue;
      }
      if (tag === "BR") { out.push("\n"); continue; }
      const block = BLOCK_TAGS.has(tag);
      if (block && out.length && !/\n$/.test(out[out.length - 1])) out.push("\n");
      textOf(child, out, depth + 1);
      if (block && !/\n$/.test(out[out.length - 1] || "")) out.push("\n");
    }
  }
  // The text of a bubble: only its selectable-text parts (outermost ones), never the whole bubble.
  // Returns null when there is no such part.
  function messageText(element) {
    const parts = Array.from(element.querySelectorAll(SELECTABLE)).filter(part => { const up = part.parentElement && part.parentElement.closest(SELECTABLE); return !up || !element.contains(up); });
    if (!parts.length) return null;
    const out = [];
    parts.forEach((part, index) => { if (index) out.push("\n"); textOf(part, out, 0); });
    return finishText(cleanLines(out.join("").replace(/ /g, " ")).split("\n")).replace(/\n{3,}/g, "\n\n");
  }
  // Only the picture of this page (blob:) inside the photo part of the row; thumbnails (data:) are never taken.
  function photosOf(row) {
    const urls = [];
    for (const image of row.querySelectorAll('[data-testid="image-thumb"] img')) {
      const src = image.getAttribute("src");
      if (isOwnBlobUrl(src) && !urls.includes(src)) urls.push(src);
    }
    return urls;
  }
  function chatName(main) {
    try {
      const title = main.querySelector('[data-testid="conversation-info-header-chat-title"]');
      if (!title) return "";
      let name = title.textContent || "";
      const you = title.querySelector('[data-testid="you-label"]');
      if (you) name = name.replace(you.textContent || "", "");
      return cleanChatName(name);
    } catch { return ""; }
  }
  function clock(row) {
    const meta = row.querySelector('[data-testid="msg-meta"]');
    const match = meta && /(\d{1,2}):(\d{2})/.exec(meta.textContent || "");
    if (!match || Number(match[1]) > 23 || Number(match[2]) > 59) return "";
    return `${pad(match[1])}:${match[2]}`;
  }
  function labelOf(row) {
    for (const element of row.querySelectorAll("span[aria-label]")) {
      const label = String(element.getAttribute("aria-label") || "").trim();
      if (label.endsWith(":") && label.length > 1) { const name = cleanChatName(label.slice(0, -1)); if (name) return name; }
    }
    return null;
  }
  // `wanted(row)` chooses the rows. Returns { chat, messages, skipped, bare } or null.
  function readRows(doc, wanted) {
    let main;
    let rows;
    try {
      main = doc.querySelector("#main");
      if (!main) return null;
      rows = Array.from(main.querySelectorAll('[role="row"]')).filter(row => row.querySelector("[data-id]"));
    } catch { return null; }
    if (!rows.length || rows.length > MAX_ROWS) return null;
    // First pass over all rows: the prefix of text rows (also gives the dates of rows without one).
    const info = [];
    for (const row of rows) {
      const marked = row.querySelectorAll(`[${ATTR}]`);
      if (marked.length > 1 || (marked.length === 1 && marked[0].querySelector(`[${ATTR}]`))) return null;
      let head = null;
      if (marked.length === 1) {
        const attribute = String(marked[0].getAttribute(ATTR) || "");
        if (/[\r\n]/.test(attribute)) return null;
        head = parseHeader(cleanLines(attribute));
      }
      info.push({ row, element: marked.length ? marked[0] : null, head });
    }
    const dated = info.map(item => item.head && item.head.when ? item.head.when.slice(0, 10) : "");
    const dateFor = index => {
      for (let i = index - 1; i >= 0; i--) if (dated[i]) return dated[i];
      for (let i = index + 1; i < dated.length; i++) if (dated[i]) return dated[i];
      return "";
    };
    const messages = [];
    let skipped = 0, bare = 0, textRows = 0, withText = 0;
    info.forEach((item, index) => {
      if (!wanted(item.row)) return;
      const { row, element, head } = item;
      const images = photosOf(row);
      const document_ = row.querySelector('[data-testid="document-thumb"]');
      if (element) {
        textRows++;
        if (!head) { messages.push(null); return; }
        let text = messageText(element);
        if (text !== null) withText++;
        text = text || "";
        if (document_) text += (text ? "\n" : "") + documentLine(row);
        if (!text) text = images.length ? "(foto)" : "(sem texto)";
        messages.push({ when: head.when, author: head.author, text, images });
        return;
      }
      if (!images.length && !document_) { skipped++; return; }
      const time = clock(row);
      const day = dateFor(index);
      const when = time && day ? `${day}T${time}` : null;
      const author = labelOf(row);
      if (!author) bare++;
      messages.push({ when, author, text: images.length ? "(foto)" : documentLine(row), images });
    });
    // A row whose prefix does not parse, or text rows without any selectable text: not the structure we know.
    if (messages.includes(null) || (textRows > 0 && withText === 0)) return null;
    return { chat: chatName(main), messages, skipped, bare };
  }
  function documentLine(row) {
    const named = row.querySelector("div[title]");
    const name = named ? cleanChatName(named.getAttribute("title")) : "";
    return name ? `(arquivo: ${name})` : "(arquivo)";
  }
  // The messages the person selected: every row the selection range touches.
  function readSelection(doc, range) {
    if (!range || typeof range.intersectsNode !== "function") return readRows(doc, () => false);
    return readRows(doc, row => { try { return range.intersectsNode(row); } catch { return false; } });
  }
  // The messages visible on the screen: rows whose message box meets the visible part of the message list.
  function readVisible(doc, viewport) {
    let area = viewport;
    try {
      const list = doc.querySelector('[data-testid="conversation-panel-messages"]') || doc.querySelector("#main");
      const box = list && list.getBoundingClientRect();
      if (box && box.width > 0 && box.height > 0) area = { left: Math.max(viewport.left, box.left), top: Math.max(viewport.top, box.top), right: Math.min(viewport.right, box.right), bottom: Math.min(viewport.bottom, box.bottom) };
    } catch { /* viewport only */ }
    return readRows(doc, row => {
      try {
        const box = (row.querySelector('[data-testid="msg-container"]') || row).getBoundingClientRect();
        return box.width > 0 && box.height > 0 && box.right > area.left && box.left < area.right && box.bottom > area.top && box.top < area.bottom;
      } catch { return false; }
    });
  }
  // "12 mensagens de 10/11/2026 14:02 a 10/11/2026 16:40 · 2 fotos"
  function summary(messages, photos = 0) {
    const { first, last } = firstLast(messages);
    const count = messages.length === 1 ? "1 mensagem" : `${messages.length} mensagens`;
    const span = first && last ? (first === last ? ` de ${formatWhen(first)}` : ` de ${formatWhen(first)} a ${formatWhen(last)}`) : "";
    const pictures = photos ? ` \u00b7 ${photos === 1 ? "1 foto" : photos + " fotos"}` : "";
    return count + span + pictures;
  }
  return { ORIGIN, MAX_MESSAGES, MAX_TEXT_BYTES, TITLE_MIN, TITLE_MAX, ORIGIN_MAX, MAX_ATTACHMENTS, MAX_FILE_BYTES, MAX_TOTAL_BYTES, MAX_NAME_CHARS, META_KEYS, ACCEPTED_PHOTO_TYPES, WhatsAppError, parseHeader, parseCopied, recordText, suggestedTitle, cleanChatName, cleanTitle, buildRecord, validateRecord, firstLast, participants, formatWhen, summary, sanitizeFileName, uniqueName, toBase64, decodedLength, validateAttachments, sniffImageType, isOwnBlobUrl, allowedOrigin, readSelection, readVisible, wellFormed };
});
