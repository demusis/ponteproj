"use strict";
const { test, before, after } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const Core = require("../../extension/capture-core.js");
const WA = require("../../extension/whatsapp-core.js");
const extensionRequire = createRequire(path.join(__dirname, "../../extension/package.json"));
let playwright;
try { playwright = process.env.PONTEPROJ_PLAYWRIGHT_DIR ? require(process.env.PONTEPROJ_PLAYWRIGHT_DIR) : extensionRequire("playwright"); }
catch { /* the DOM tests are skipped without Playwright */ }
const ext = name => path.join(__dirname, "../../extension/" + name);
const read = name => fs.readFileSync(ext(name), "utf8");
const vectors = JSON.parse(fs.readFileSync(path.join(__dirname, "whatsapp-vetores.json"), "utf8"));
const plainOf = value => JSON.parse(JSON.stringify(value));

// ------------------------------------------------------------------ shared vectors
test("every shared vector: reader, participants, first/last, record text, origin and title", () => {
  assert.equal(vectors.version, 2);
  assert.ok(vectors.cases.length >= 59);
  for (const item of vectors.cases) {
    if (item.error) {
      assert.throws(() => WA.parseCopied(item.input), error => error instanceof WA.WhatsAppError && error.code === "NOT_WHATSAPP", item.name);
      continue;
    }
    const expect = item.expect;
    const messages = WA.parseCopied(item.input);
    assert.deepEqual(plainOf(messages), expect.messages, item.name);
    assert.deepEqual(WA.participants(messages), expect.participants, item.name);
    const { first, last } = WA.firstLast(messages);
    assert.equal(first, expect.first, item.name);
    assert.equal(last, expect.last, item.name);
    assert.equal(WA.recordText(messages), expect.record_text, item.name);
    assert.equal(WA.cleanChatName(item.chat), expect.origin, item.name);
    assert.equal(WA.suggestedTitle(item.chat, messages), expect.title, item.name);
    // The same input becomes a valid record of the contract.
    const record = WA.buildRecord({ title: "", origin: item.chat, messages });
    assert.equal(record.text, expect.record_text, item.name);
    assert.equal(record.origin, expect.origin, item.name);
    assert.equal(record.title, expect.title, item.name);
    assert.equal(record.document_date, expect.last.slice(0, 10), item.name);
    assert.deepEqual(plainOf(record.meta), { wa_chat: expect.origin, wa_first: expect.first, wa_last: expect.last, wa_messages: String(expect.messages.length) }, item.name);
    assert.doesNotThrow(() => WA.validateRecord(record), item.name);
  }
});

// ------------------------------------------------------------------ record and attachments
const validRecord = () => WA.buildRecord({ title: "Conversa de teste", origin: "Unidade A", messages: WA.parseCopied("[14:02, 10/11/2026] Servidor A: Oi\n[14:03, 10/11/2026] Servidor B: Olá") });
test("record validation: exact keys at every level, types, limits and characters", () => {
  assert.doesNotThrow(() => WA.validateRecord(validRecord()));
  const bad = change => { const record = validRecord(); change(record); assert.throws(() => WA.validateRecord(record), error => error instanceof WA.WhatsAppError && error.code === "RECORD_INVALID"); };
  bad(record => { record.extra = 1; });
  bad(record => { delete record.origin; });
  bad(record => { record.kind = "email"; });
  bad(record => { record.title = "ab"; });
  bad(record => { record.title = "x".repeat(501); });
  bad(record => { record.title = " com espaço"; });
  bad(record => { record.title = "Com\ncontrole"; });
  bad(record => { record.title = "Com \u202e marca"; });
  bad(record => { record.origin = "x".repeat(201); record.meta.wa_chat = record.origin; });
  bad(record => { record.meta.wa_chat = "Outra"; });
  bad(record => { record.meta.imported_from = "whatsapp-web"; });
  bad(record => { delete record.meta.wa_first; });
  bad(record => { record.meta.wa_first = "2026-11-10 14:02"; });
  bad(record => { record.meta.wa_last = "2026-02-31T10:00"; });
  bad(record => { record.meta.wa_messages = "0"; });
  bad(record => { record.meta.wa_messages = "501"; });
  bad(record => { record.meta.wa_messages = 2; });
  bad(record => { record.document_date = "2026-11-09"; });
  bad(record => { record.document_date = "10/11/2026"; });
  bad(record => { record.text = ""; });
  bad(record => { record.text = "a\u0000b"; });
  bad(record => { record.text = "a\rb"; });
  bad(record => { record.text = "\ud800 solto"; });
  bad(record => { record.text = "x".repeat(500 * 1024 + 1); });
  bad(record => { record.meta = { ...record.meta, wa_chat: "x".repeat(201) }; });
  // Tabs and line ends are allowed in the text, 500 KiB exactly too.
  const edge = validRecord();
  edge.text = "a\tb\nc";
  assert.doesNotThrow(() => WA.validateRecord(edge));
  edge.text = "é".repeat(256 * 1024 / 2 * 2 / 2 * 2 - 0).slice(0, 250 * 1024);
  assert.equal(new TextEncoder().encode(edge.text).length, 500 * 1024);
  assert.doesNotThrow(() => WA.validateRecord(edge));
  assert.throws(() => WA.validateRecord(null));
  assert.throws(() => WA.validateRecord([]));
});
test("record building: limits of messages and text, repair of lone surrogates, no control characters", () => {
  const message = n => ({ when: "2026-11-10T14:02", author: "Servidor A", text: "m" + n });
  assert.equal(WA.buildRecord({ title: "Título", origin: "", messages: Array.from({ length: 500 }, (_, n) => message(n)) }).meta.wa_messages, "500");
  assert.throws(() => WA.buildRecord({ title: "Título", origin: "", messages: Array.from({ length: 501 }, (_, n) => message(n)) }), error => error.code === "TOO_MANY" && /501 mensagens; o limite é 500/.test(error.message));
  assert.throws(() => WA.buildRecord({ title: "Título", origin: "", messages: [] }), error => error.code === "NO_MESSAGES");
  assert.throws(() => WA.buildRecord({ title: "Título", origin: "", messages: [{ ...message(1), text: "x".repeat(500 * 1024 + 1) }] }), error => error.code === "TOO_BIG");
  assert.throws(() => WA.buildRecord({ title: "ab", origin: "", messages: [message(1)] }), error => error.code === "TITLE_SHORT");
  assert.throws(() => WA.buildRecord({ title: "x".repeat(501), origin: "", messages: [message(1)] }), error => error.code === "TITLE_LONG");
  const record = WA.buildRecord({ title: "T\u202eítulo \ud800", origin: "O\u200erigem", messages: [{ when: "2026-11-10T14:02", author: "A\nB", text: "a\u0007b \udc00 c  " }] });
  assert.equal(record.title, "Título \ufffd");
  assert.equal(record.origin, "Origem");
  assert.equal(record.text, "[10/11/2026 14:02] A B: ab \ufffd c");
  assert.doesNotThrow(() => WA.validateRecord(record));
  assert.equal(WA.buildRecord({ title: "   ", origin: "Unidade A", messages: [message(1)] }).title, "Conversa com Unidade A \u2014 10/11/2026");
  assert.throws(() => WA.buildRecord({ title: "Título", origin: "", messages: [{ when: "10/11/2026", author: null, text: "x" }] }), error => error.code === "RECORD_INVALID");
});
const b64 = size => Buffer.alloc(size, 1).toString("base64");
test("attachment rules: at most 10, 8 MiB each, 16 MiB together, sanitised names, strict base64", () => {
  const item = (name, size = 10) => ({ name, data_base64: b64(size) });
  assert.equal(WA.validateAttachments([]), 0);
  assert.equal(WA.validateAttachments([item("a.pdf", 5), item("b.png", 7)]), 12);
  assert.doesNotThrow(() => WA.validateAttachments(Array.from({ length: 10 }, (_, n) => item(`f${n}.pdf`))));
  const refuses = (list, pattern) => assert.throws(() => WA.validateAttachments(list), error => error.code === "ATTACHMENT_INVALID" && pattern.test(error.message));
  refuses(Array.from({ length: 11 }, (_, n) => item(`f${n}.pdf`)), /11 arquivos/);
  refuses([{ name: "a.pdf" }], /nome inválido/);
  refuses([{ name: "a.pdf", data_base64: b64(3), extra: 1 }], /nome inválido/);
  refuses([item("../a.pdf")], /nome inválido/);
  refuses([item("a\\b.pdf")], /nome inválido/);
  refuses([item("a\u202e.pdf")], /nome inválido/);
  refuses([item("a\u0000.pdf")], /nome inválido/);
  refuses([item("x".repeat(181))], /nome inválido/);
  refuses([item("")], /nome inválido/);
  refuses([item("a.pdf"), item("a.pdf")], /nome inválido/);
  refuses([{ name: "a.pdf", data_base64: "não é base64" }], /vazio ou danificado/);
  refuses([{ name: "a.pdf", data_base64: "QUJD=" }], /vazio ou danificado/);
  refuses([{ name: "a.pdf", data_base64: "" }], /vazio ou danificado/);
  refuses([item("a.pdf", 8 * 1024 * 1024 + 1)], /passa de 8 MB/);
  refuses([item("a.pdf", 6 * 1024 * 1024), item("b.pdf", 6 * 1024 * 1024), item("c.pdf", 6 * 1024 * 1024)], /passam de 16 MB/);
  assert.equal(WA.validateAttachments([item("a.pdf", 8 * 1024 * 1024), item("b.pdf", 8 * 1024 * 1024)]), 16 * 1024 * 1024);
  refuses("nada", /não puderam ser enviados/);
  assert.equal(WA.sanitizeFileName("C:\\Users\\x\\Relatório final.pdf"), "Relatório final.pdf");
  assert.equal(WA.sanitizeFileName("../../etc/passwd"), "passwd");
  assert.equal(WA.sanitizeFileName("a\u202e\u0007b\u200f.txt"), "ab.txt");
  assert.equal(WA.sanitizeFileName(""), "arquivo");
  assert.equal(WA.sanitizeFileName(".."), "arquivo");
  const long = WA.sanitizeFileName("n".repeat(300) + ".docx");
  assert.ok(Array.from(long).length <= 180 && long.endsWith(".docx"));
  assert.equal(WA.uniqueName("a.pdf", ["a.pdf"]), "a (2).pdf");
  assert.equal(WA.uniqueName("a.pdf", ["a.pdf", "a (2).pdf"]), "a (3).pdf");
  assert.equal(WA.uniqueName("b.pdf", ["a.pdf"]), "b.pdf");
  assert.equal(WA.toBase64(Uint8Array.from([1, 2, 3, 4])), "AQIDBA==");
});
test("photo types: only what the Ponte accepts, webp recognised to be left out; only blobs of this page", () => {
  const head = (...bytes) => Uint8Array.from([...bytes, ...new Array(16).fill(0)]);
  assert.equal(WA.sniffImageType(head(0xff, 0xd8, 0xff, 0xe0)), "jpg");
  assert.equal(WA.sniffImageType(head(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)), "png");
  assert.equal(WA.sniffImageType(head(0x47, 0x49, 0x46, 0x38, 0x39, 0x61)), "gif");
  assert.equal(WA.sniffImageType(head(0x42, 0x4d)), "bmp");
  assert.equal(WA.sniffImageType(head(0x49, 0x49, 0x2a, 0x00)), "tif");
  assert.equal(WA.sniffImageType(Uint8Array.from([0x52, 0x49, 0x46, 0x46, 1, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, 0])), "webp");
  assert.ok(!WA.ACCEPTED_PHOTO_TYPES.includes("webp"));
  assert.equal(WA.sniffImageType(head(0x25, 0x50, 0x44, 0x46)), null);
  assert.equal(WA.sniffImageType(Uint8Array.from([0xff, 0xd8])), null);
  for (const good of ["blob:https://web.whatsapp.com/3b2f6b1c-0d9e-4a7b-8f3a-1c2d3e4f5a6b"]) assert.equal(WA.isOwnBlobUrl(good), true);
  for (const bad of ["blob:https://outside.test/3b2f6b1c", "blob:http://web.whatsapp.com/abc", "blob:https://web.whatsapp.com.evil.test/abc", "blob:https://web.whatsapp.com:444/abc", "https://web.whatsapp.com/abc", "data:image/png;base64,AAAA", "blob:null/abc", "blob:https://web.whatsapp.com/", "blob:https://user@web.whatsapp.com/abc", "blob:https://web.whatsapp.com/a b", "blob:https://web.whatsapp.com/a/../b", "", null, 5]) assert.equal(WA.isOwnBlobUrl(bad), false, String(bad));
  assert.equal(WA.allowedOrigin("https://web.whatsapp.com/"), true);
  for (const bad of ["http://web.whatsapp.com/", "https://web.whatsapp.com.evil.test/", "https://user:x@web.whatsapp.com/", "https://www.sigadoc.mt.gov.br/", "https://web.whatsapp.com:8443/", "javascript:alert(1)"]) assert.equal(WA.allowedOrigin(bad), false, bad);
});

// ------------------------------------------------------------------ manifest and sources
test("manifest 0.4.1: optional WhatsApp host, scripting, nothing else changed", () => {
  const manifest = JSON.parse(read("manifest.json"));
  assert.equal(manifest.version, "0.4.1");
  assert.equal(JSON.parse(read("package.json")).version, "0.4.1");
  assert.deepEqual([...manifest.permissions].sort(), ["nativeMessaging", "scripting", "storage"]);
  assert.deepEqual(manifest.optional_host_permissions, ["https://web.whatsapp.com/*"]);
  assert.deepEqual(manifest.host_permissions, ["https://www.sigadoc.mt.gov.br/*"]);
  assert.equal(manifest.key, fs.readFileSync(path.join(__dirname, "../../extension-key.txt"), "utf8").trim());
  assert.deepEqual(manifest.content_scripts.map(entry => entry.matches), [["https://www.sigadoc.mt.gov.br/*"]]);
  assert.ok(!JSON.stringify(manifest.content_scripts).includes("whatsapp"));
  assert.deepEqual(Object.keys(manifest.commands), ["importar"]);
  assert.equal(manifest.externally_connectable, undefined);
  assert.ok(!("web_accessible_resources" in manifest));
});
test("WhatsApp sources: no background reading, no other access, no scripts into the page, no logs", () => {
  const panel = read("whatsapp.js"), core = read("whatsapp-core.js"), background = read("background.js");
  for (const [name, source] of [["whatsapp.js", panel], ["whatsapp-core.js", core]]) {
    assert.ok(!/MutationObserver|IntersectionObserver|setInterval|setTimeout|requestAnimationFrame|requestIdleCallback/.test(source), name);
    assert.ok(!/localStorage|sessionStorage|indexedDB|document\.cookie|XMLHttpRequest|WebSocket|sendBeacon|navigator\.clipboard|storage\./.test(source), name);
    assert.ok(!/innerHTML|outerHTML|insertAdjacentHTML|\beval\(|new Function|console\.|document\.write/.test(source), name);
    assert.ok(!/createElement\(["']script|\.src\s*=|executeScript|world:\s*["']MAIN/.test(source), name);
  }
  // The panel listens on its own host only, never on the page; reads happen in click handlers.
  assert.ok(!/(?:document|window|document\.body|document\.documentElement)\.addEventListener/.test(panel));
  assert.equal((panel.match(/\bfetch\(/g) || []).length, 1);
  assert.ok(/isOwnBlobUrl\(url\)\) throw/.test(panel));
  assert.ok(!/\bfetch\(/.test(core));
  assert.equal((panel.match(/\.click\(/g) || []).length, 1);
  assert.ok(!/\.(focus|scrollIntoView|scrollTo|scrollBy|dispatchEvent|submit)\(/.test(core));
  assert.ok(!/sendMessage\(\{\s*action:\s*["']ponte\.(lookup|import_capture)/.test(panel));
  // The background registers the panel only for this origin, isolated, top frame.
  assert.ok(/matches: \[WA_PATTERN\]/.test(background) && /world: "ISOLATED"/.test(background) && /allFrames: false/.test(background) && /persistAcrossSessions: true/.test(background));
});

// ------------------------------------------------------------------ the background
const WA_URL = "https://web.whatsapp.com/";
const UUID_OK = "3b2f6b1c-0d9e-4a7b-8f3a-1c2d3e4f5a6b";
function worker(options = {}) {
  const handlers = {};
  const calls = [];
  const state = { granted: options.granted !== false, registered: options.registered ? [{ id: "ponte-whatsapp" }] : [], registerCalls: [], removeCalls: [], store: options.store || {}, tabs: options.tabs || [], started: options.started || {}, scriptCalls: [] };
  const ACTIONS = options.actions || ["lookup", "open_document", "import_extras", "import_record"];
  const defaultReply = request => {
    if (request.action === "ping") return { protocol_version: 1, ok: true, app_version: "0.32.0", actions: ACTIONS };
    if (request.action === "import_record") return { protocol_version: 1, ok: true, outcome: "imported", document_id: 42, document_number: "MANUAL-2026/0042", attachments_added: (request.attachments || []).length, extras_applied: Boolean(request.extras), extras_message: "", ui_opened: true, secret: "x" };
    if (request.action === "open_document") return { protocol_version: 1, ok: true, document_id: request.document_id, ui_opened: true };
    if (request.action === "lookup") return { protocol_version: 1, ok: true, found: false };
    return { protocol_version: 1, ok: true, request_id: request.capture.request_id, document_id: 1, capture_id: 2, duplicate: false, message: "Salvo", ui_opened: false };
  };
  const chrome = {
    runtime: {
      id: "ecllnakmppjinmednoikimbpnbgbimhg", lastError: null,
      getURL: name => "chrome-extension://ecllnakmppjinmednoikimbpnbgbimhg/" + name,
      onMessage: { addListener: value => { handlers.message = value; } },
      onStartup: { addListener: value => { handlers.startup = value; } },
      onInstalled: { addListener: value => { handlers.installed = value; } },
      sendNativeMessage: (host, request, callback) => {
        calls.push({ host, request: plainOf(request) });
        const custom = options.native ? options.native(request, calls.length) : undefined;
        callback(custom === undefined ? defaultReply(request) : custom);
      }
    },
    storage: { session: {
      get: async key => key in state.store ? { [key]: plainOf(state.store[key]) } : {},
      set: async value => { Object.assign(state.store, plainOf(value)); }
    } },
    commands: { onCommand: { addListener: value => { handlers.command = value; } } },
    tabs: { query: (query, callback) => callback(query && query.url ? state.tabs.slice() : []), sendMessage: () => {} },
    permissions: {
      contains: async query => { assert.equal(JSON.stringify(query), JSON.stringify({ origins: ["https://web.whatsapp.com/*"] })); return state.granted; },
      remove: async query => { state.removeCalls.push(plainOf(query)); state.granted = false; return true; },
      onRemoved: { addListener: value => { handlers.removed = value; } },
      onAdded: { addListener: value => { handlers.added = value; } }
    },
    scripting: {
      getRegisteredContentScripts: async () => state.registered.slice(),
      unregisterContentScripts: async filter => { assert.equal(JSON.stringify(filter), JSON.stringify({ ids: ["ponte-whatsapp"] })); state.registered = []; },
      registerContentScripts: async list => { state.registerCalls.push(plainOf(list)); state.registered = plainOf(list); },
      executeScript: async call => {
        const tabId = call.target.tabId;
        if (state.started[tabId] === "refuses") throw new Error("cannot access the tab");
        if (typeof call.func === "function") {
          // The first function call per tab is the probe; the one after the files asks the panel to open.
          const opened = state.scriptCalls.some(seen => seen.tabId === tabId && seen.probe);
          state.scriptCalls.push(opened ? { tabId, open: true } : { tabId, probe: true });
          return [{ result: opened ? null : state.started[tabId] === true }];
        }
        state.scriptCalls.push({ tabId, files: plainOf(call.files), world: call.world, allFrames: call.target.allFrames });
        state.started[tabId] = true;
        return [{ result: null }];
      }
    }
  };
  const context = { chrome, importScripts: () => {}, PonteCapture: Core, PonteFiles: { prepare: async () => { throw new Error("no files here"); } }, PonteWhatsApp: WA, setTimeout, clearTimeout, JSON, Promise };
  vm.runInNewContext(read("background.js"), context);
  const id = chrome.runtime.id;
  const senders = {
    whatsapp: { id, frameId: 0, url: WA_URL, tab: { id: 9, url: WA_URL + "?x=1" } },
    sigadoc: { id, frameId: 0, url: Core.ORIGIN + "/doc", tab: { id: 7, url: Core.ORIGIN + "/doc" } },
    popup: { id, url: chrome.runtime.getURL("popup.html") }
  };
  const invoke = (message, who = senders.whatsapp) => new Promise(resolve => handlers.message(message, who, resolve));
  return { invoke, calls, state, senders, chrome, handlers, id };
}
const sampleRecord = (title = "Conversa de teste") => WA.buildRecord({ title, origin: "Unidade A", messages: WA.parseCopied("[14:02, 10/11/2026] Servidor A: Oi\n[14:03, 10/11/2026] Servidor B: Olá") });
const recordMessage = (extra = {}) => ({ action: "ponte.import_record", record: sampleRecord(), attachments: [], ...extra });
const nativeActions = w => w.calls.map(call => call.request.action);

test("import_record: message built from scratch exactly as the contract, answer reduced to checked fields", async () => {
  const w = worker();
  const attachment = { name: "foto-1.png", data_base64: Buffer.from("abc").toString("base64") };
  const result = await w.invoke(recordMessage({ attachments: [attachment], extras: { note: "Responder até sexta", priority: "alta" } }));
  assert.deepEqual(plainOf(result), { protocol_version: 1, ok: true, outcome: "imported", document_id: 42, document_number: "MANUAL-2026/0042", attachments_added: 1, ui_opened: true, extras_applied: true });
  assert.deepEqual(nativeActions(w), ["ping", "import_record"]);
  const request = w.calls[1].request;
  assert.deepEqual(Object.keys(request), ["protocol_version", "action", "record", "attachments", "extras"]);
  assert.deepEqual(request, { protocol_version: 1, action: "import_record", record: sampleRecord(), attachments: [attachment], extras: { note: "Responder até sexta", priority: "alta" } });
  assert.deepEqual(Object.keys(request.record), ["kind", "title", "origin", "document_date", "text", "meta"]);
  assert.deepEqual(Object.keys(request.record.meta), ["wa_chat", "wa_first", "wa_last", "wa_messages"]);
  assert.equal(w.calls[1].host, "com.ponteproj.assistente");
  // No attachments: the field is left out; the answer of the Ponte is cached for a minute (no second ping).
  assert.ok(!("attachments" in (await (async () => { await w.invoke(recordMessage()); return w.calls[2].request; })())));
  assert.deepEqual(nativeActions(w), ["ping", "import_record", "import_record"]);
  assert.ok(!("extras" in w.calls[2].request));
});
test("import_record is never sent to a Ponte that does not list it, nor to an unreachable one", async () => {
  for (const actions of [["lookup", "open_document", "import_extras"], []]) {
    const w = worker({ actions });
    const result = await w.invoke(recordMessage());
    assert.equal(result.ok, false);
    assert.equal(result.error.message, "Atualize o Ponte para guardar conversas do WhatsApp.");
    assert.deepEqual(nativeActions(w), ["ping"]);
  }
  const down = worker({ native: () => ({ nativeError: "x" }) });
  down.chrome.runtime.sendNativeMessage = (host, request, callback) => { down.chrome.runtime.lastError = { message: "Specified native messaging host not found." }; callback(undefined); down.chrome.runtime.lastError = null; };
  assert.equal((await down.invoke(recordMessage())).ok, false);
});
test("extras go only to a Ponte that takes them; an older one gets the record without them", async () => {
  const w = worker({ actions: ["import_record"] });
  const result = await w.invoke(recordMessage({ extras: { priority: "baixa" } }));
  assert.equal(result.ok, true);
  assert.equal(result.extras_skipped, true);
  assert.ok(!("extras" in w.calls[1].request));
  const bad = worker();
  assert.equal((await bad.invoke(recordMessage({ extras: { note: " com espaços " } }))).error.code, "EXTRAS_INVALID");
  assert.equal(bad.calls.length, 0);
});
test("import_record validates before any native call: record, attachments, keys, sizes", async () => {
  const w = worker();
  const code = async message => (await w.invoke(message)).error.code;
  assert.equal(await code({ ...recordMessage(), extra: 1 }), "ACTION_DENIED");
  assert.equal(await code({ action: "ponte.import_record", record: sampleRecord() }), "ACTION_DENIED");
  assert.equal(await code(recordMessage({ record: { ...sampleRecord(), imported_from: "x" } })), "RECORD_INVALID");
  assert.equal(await code(recordMessage({ record: { ...sampleRecord(), kind: "email" } })), "RECORD_INVALID");
  assert.equal(await code(recordMessage({ attachments: [{ name: "../x.pdf", data_base64: "QUJD" }] })), "ATTACHMENT_INVALID");
  assert.equal(await code(recordMessage({ attachments: [{ name: "x.pdf", data_base64: "###" }] })), "ATTACHMENT_INVALID");
  assert.equal(await code(recordMessage({ attachments: Array.from({ length: 11 }, (_, n) => ({ name: `${n}.pdf`, data_base64: "QUJD" })) })), "ATTACHMENT_INVALID");
  assert.equal(await code(recordMessage({ attachments: [{ name: "a.pdf", data_base64: b64(9 * 1024 * 1024) }] })), "ATTACHMENT_INVALID");
  assert.equal(await code(recordMessage({ attachments: [{ name: "a.pdf", data_base64: b64(6 * 1024 * 1024) }, { name: "b.pdf", data_base64: b64(6 * 1024 * 1024) }, { name: "c.pdf", data_base64: b64(6 * 1024 * 1024) }] })), "ATTACHMENT_INVALID");
  assert.equal(await code(recordMessage({ extras: { note: "" } })), "EXTRAS_INVALID");
  assert.equal(await code(recordMessage({ record: "texto" })), "RECORD_INVALID");
  assert.equal(w.calls.length, 0);
  // 16 MiB of files with the largest text still fit in the native frame.
  const big = recordMessage({ attachments: [{ name: "a.pdf", data_base64: b64(8 * 1024 * 1024) }, { name: "b.pdf", data_base64: b64(8 * 1024 * 1024) }] });
  assert.equal((await w.invoke(big)).ok, true);
});
test("sender separation: each message type from each kind of sender, and the permission", async () => {
  const w = worker();
  const lookup = { action: "ponte.lookup", document_number: "ORG-OFI-2026/00431" };
  const capture = { action: "ponte.import_capture", capture: { x: 1 } };
  const open = { action: "ponte.open", document_id: 5 };
  const denied = async (message, who) => assert.equal((await w.invoke(message, who)).error.code, "SENDER_DENIED", JSON.stringify(message).slice(0, 40));
  // The WhatsApp tab: ping, open and import_record only.
  await denied(lookup, w.senders.whatsapp);
  await denied(capture, w.senders.whatsapp);
  await denied({ action: "ponte.recent" }, w.senders.whatsapp);
  await denied({ action: "ponte.recent_clear" }, w.senders.whatsapp);
  await denied({ action: "ponte.whatsapp_on" }, w.senders.whatsapp);
  await denied({ action: "ponte.whatsapp_off" }, w.senders.whatsapp);
  assert.equal(w.calls.length, 0);
  assert.equal((await w.invoke({ action: "ponte.ping" }, w.senders.whatsapp)).ok, true);
  assert.equal((await w.invoke(open, w.senders.whatsapp)).ok, true);
  assert.equal((await w.invoke(recordMessage(), w.senders.whatsapp)).ok, true);
  // The SIGADOC tab and the popup cannot send records or switch the feature.
  for (const who of [w.senders.sigadoc, w.senders.popup]) await denied(recordMessage(), who);
  await denied({ action: "ponte.whatsapp_on" }, w.senders.sigadoc);
  await denied({ action: "ponte.whatsapp_off" }, w.senders.sigadoc);
  const before = w.calls.length;
  // Strangers: other origin, sub-frame, other extension, tab without our origin, popup-like with a tab.
  const strangers = [{ ...w.senders.whatsapp, url: "https://web.whatsapp.com.evil.test/" }, { ...w.senders.whatsapp, frameId: 1 }, { ...w.senders.whatsapp, tab: { id: 9, url: "https://evil.test/" } }, { ...w.senders.whatsapp, id: "other" }, { ...w.senders.whatsapp, url: "http://web.whatsapp.com/" }, { ...w.senders.whatsapp, tab: undefined }, { ...w.senders.whatsapp, tab: { id: "9", url: WA_URL } }, { id: w.id, url: w.chrome.runtime.getURL("popup.html"), tab: { id: 3, url: WA_URL } }];
  for (const who of strangers) { await denied(recordMessage(), who); await denied({ action: "ponte.ping" }, who); await denied(open, who); }
  assert.equal(w.calls.length, before);
  // The SIGADOC tab keeps what it had.
  assert.equal((await w.invoke(lookup, w.senders.sigadoc)).ok, true);
  assert.equal((await w.invoke(open, w.senders.sigadoc)).ok, true);
  assert.equal((await w.invoke({ action: "ponte.ping" }, w.senders.sigadoc)).ok, true);
  // Without the permission the WhatsApp tab gets nothing, at the moment of each request.
  w.state.granted = false;
  const calls = w.calls.length;
  await denied({ action: "ponte.ping" }, w.senders.whatsapp);
  await denied(open, w.senders.whatsapp);
  await denied(recordMessage(), w.senders.whatsapp);
  assert.equal(w.calls.length, calls);
  assert.equal((await w.invoke({ action: "ponte.ping" }, w.senders.popup)).ok, true);
});
test("recent imports gain the conversations: number, title, time and outcome", async () => {
  const w = worker({ native: request => request.action === "import_record" && request.record.title === "Repetida" ? { protocol_version: 1, ok: true, outcome: "already", document_id: 42, document_number: "MANUAL-2026/0042", ui_opened: false } : request.action === "import_record" && request.record.title === "Falha" ? { protocol_version: 1, ok: false, error: { code: "import_failed", message: "O Ponte não conseguiu guardar a conversa." } } : undefined });
  await w.invoke(recordMessage());
  await w.invoke({ ...recordMessage(), record: sampleRecord("Repetida") });
  const failed = await w.invoke({ ...recordMessage(), record: sampleRecord("Falha") });
  assert.deepEqual(plainOf(failed), { protocol_version: 1, ok: false, error: { code: "import_failed", message: "O Ponte não conseguiu guardar a conversa." } });
  const items = (await w.invoke({ action: "ponte.recent" }, w.senders.popup)).items;
  assert.deepEqual(items.map(item => [item.number, item.title, item.outcome, item.document_id]), [["WhatsApp", "Falha", "failed", undefined], ["MANUAL-2026/0042", "Repetida", "already", 42], ["MANUAL-2026/0042", "Conversa de teste", "imported", 42]]);
  assert.ok(items.every(item => !Number.isNaN(Date.parse(item.time))));
  // Only these small fields are kept in session memory: never the text of the conversation.
  assert.ok(!JSON.stringify(w.state.store).includes("Servidor A"));
});
test("native answers about records are checked: unknown outcome or missing id is a schema error", async () => {
  for (const reply of [{ protocol_version: 1, ok: true, outcome: "weird", document_id: 1 }, { protocol_version: 1, ok: true, outcome: "imported" }, { protocol_version: 1, ok: true, outcome: "imported", document_id: -1 }, { protocol_version: 2, ok: true, outcome: "imported", document_id: 1 }, null]) {
    const w = worker({ native: request => request.action === "import_record" ? reply : undefined });
    assert.equal((await w.invoke(recordMessage())).error.code, "HOST_SCHEMA");
  }
  const w = worker({ native: request => request.action === "import_record" ? { protocol_version: 1, ok: false, error: { code: "invalid_attachment", message: "O arquivo x.pdf não é aceito." } } : undefined });
  assert.equal((await w.invoke(recordMessage())).error.message, "O arquivo x.pdf não é aceito.");
});
test("turning WhatsApp Web on places the panel in the tabs already open, once each", async () => {
  // Three tabs: a fresh one, one that already has the panel, one the browser does not let the worker reach.
  const w = worker({ granted: true, tabs: [{ id: 11 }, { id: 12 }, { id: 13 }, { url: "no id" }], started: { 12: true, 13: "refuses" } });
  const reply = await w.invoke({ action: "ponte.whatsapp_on" }, w.senders.popup);
  assert.equal(reply.ok, true);
  assert.equal(reply.open_tabs, 3);
  assert.equal(reply.placed_tabs, 2);
  const files = w.state.scriptCalls.filter(call => call.files);
  assert.deepEqual(files, [{ tabId: 11, files: ["capture-core.js", "whatsapp-core.js", "whatsapp.js"], world: "ISOLATED", allFrames: false }]);
  assert.deepEqual(w.state.scriptCalls.filter(call => call.probe).map(call => call.tabId), [11, 12]);
  assert.deepEqual(w.state.scriptCalls.filter(call => call.open).map(call => call.tabId), [11, 12]);
  // No WhatsApp Web tab open: nothing to place, and it still turns on.
  const none = worker({ granted: true });
  const quiet = await none.invoke({ action: "ponte.whatsapp_on" }, none.senders.popup);
  assert.equal(quiet.ok, true);
  assert.equal(quiet.open_tabs, 0);
  assert.equal(none.state.scriptCalls.length, 0);
  // Without the permission nothing is placed anywhere.
  const denied = worker({ granted: false, tabs: [{ id: 11 }] });
  assert.equal((await denied.invoke({ action: "ponte.whatsapp_on" }, denied.senders.popup)).error.code, "PERMISSION_DENIED");
  assert.equal(denied.state.scriptCalls.length, 0);
});
test("turning WhatsApp Web on and off: scripts registered only with the permission, persistent, isolated", async () => {
  const w = worker({ granted: false });
  assert.equal((await w.invoke({ action: "ponte.whatsapp_on" }, w.senders.popup)).error.code, "PERMISSION_DENIED");
  assert.equal(w.state.registerCalls.length, 0);
  w.state.granted = true;
  assert.equal((await w.invoke({ action: "ponte.whatsapp_on" }, w.senders.popup)).ok, true);
  assert.deepEqual(w.state.registerCalls, [[{ id: "ponte-whatsapp", js: ["capture-core.js", "whatsapp-core.js", "whatsapp.js"], matches: ["https://web.whatsapp.com/*"], runAt: "document_idle", allFrames: false, world: "ISOLATED", persistAcrossSessions: true }]]);
  assert.equal((await w.invoke({ action: "ponte.whatsapp_on", extra: 1 }, w.senders.popup)).error.code, "SENDER_DENIED");
  assert.equal((await w.invoke({ action: "ponte.whatsapp_off" }, w.senders.popup)).ok, true);
  assert.deepEqual(w.state.registered, []);
  assert.deepEqual(w.state.removeCalls, [{ origins: ["https://web.whatsapp.com/*"] }]);
  assert.equal(w.state.granted, false);
  for (const name of ["capture-core.js", "whatsapp-core.js", "whatsapp.js"]) assert.ok(fs.existsSync(ext(name)));
});
test("start-up and update repair the registration only while the permission is granted; revoking removes it", async () => {
  const w = worker({ granted: true, registered: false });
  await w.handlers.startup();
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.equal(w.state.registerCalls.length, 1);
  await w.handlers.startup();
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.equal(w.state.registerCalls.length, 1, "already registered: left alone");
  await w.handlers.installed();
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.equal(w.state.registerCalls.length, 2, "an update refreshes the registration");
  w.state.granted = false;
  await w.handlers.removed();
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.deepEqual(w.state.registered, []);
  const off = worker({ granted: false, registered: false });
  await off.handlers.startup(); await off.handlers.installed();
  await new Promise(resolve => setTimeout(resolve, 20));
  assert.equal(off.state.registerCalls.length, 0, "nobody gets the access by updating");
});

// ------------------------------------------------------------------ the page: adapter, panel, popup
let browser;
before(async () => {
  if (playwright) browser = await playwright.chromium.launch({ headless: true, channel: process.env.PONTEPROJ_BROWSER_CHANNEL || "chrome" });
});
after(async () => { await browser?.close(); });
const options = { skip: !playwright && "Instale a devDependency playwright ou defina PONTEPROJ_PLAYWRIGHT_DIR." };
const fixture = fs.readFileSync(path.join(__dirname, "fixture-whatsapp.html"), "utf8");
async function pageFor(html = fixture, url = WA_URL) {
  const page = await browser.newPage();
  await page.route("**/*", route => route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: html }));
  await page.goto(url);
  await page.addScriptTag({ path: ext("capture-core.js") });
  await page.addScriptTag({ path: ext("whatsapp-core.js") });
  return page;
}
const viewport = { left: 0, top: 0, right: 1280, bottom: 720 };
test("adapter on a synthetic page: rows, authors, text without the time filler, emoji, photos only of this page, chat name", options, async () => {
  const page = await pageFor();
  const found = await page.evaluate(rect => PonteWhatsApp.readVisible(document, rect), viewport);
  // The label "(você)" is not part of the chat name.
  assert.equal(found.chat, "Unidade A");
  assert.deepEqual(found.messages.map(m => [m.when, m.author, m.text, m.images.length]), [
    [null, null, "(foto)", 1],
    ["2026-11-10T14:02", "Servidor A", "Bom dia", 0],
    ["2026-11-10T14:03", "Servidor B", "Combinado 🙂 ok", 0],
    ["2026-11-10T14:05", "Servidor A", "Foto da sala", 1],
    ["2026-11-10T14:06", "Servidor B", "Linha 1\nLinha 2", 0],
    ["2026-11-10T14:07", "Servidor A", "(foto)", 1],
    ["2026-11-10T14:08", "Servidor B", "[14:09, 10/11/2026] Servidor A: mensagem falsa", 0],
    ["2026-11-10T14:09", null, "(arquivo: Ofício.pdf)", 0]
  ]);
  // The row of the audio message is counted, not read; two rows (photo, document) have no author.
  assert.equal(found.skipped, 1);
  assert.equal(found.bare, 2);
  // The first row (a photo) has no earlier dated row: its date cannot be known.
  assert.equal(found.undatedPhotos, 1);
  assert.equal(found.undatedFiles, 0);
  // Text never carries the time of the filler ("Bom dia14:02"); data: thumbnails are never taken.
  assert.ok(!found.messages.some(m => /dd:dd$/.test(m.text)));
  for (const m of found.messages) assert.ok(m.images.every(url => url.startsWith("blob:https://web.whatsapp.com/")));
  // The imitation stays text of its own message: no message was invented.
  assert.equal(found.messages.length, 8);
  // The message far below the screen is not "visible"; a selection over it reads it.
  const all = await page.evaluate(() => { const range = document.createRange(); range.selectNodeContents(document.querySelector("[data-testid=conversation-panel-messages]")); return PonteWhatsApp.readSelection(document, range).messages.map(m => m.text); });
  assert.equal(all.length, 9);
  assert.equal(all[8], "Fora da tela");
  const part = await page.evaluate(() => { const range = document.createRange(); range.setStart(document.getElementById("m2").querySelector("[data-testid=selectable-text]").firstChild, 2); range.setEnd(document.getElementById("m4").querySelector("[data-testid=selectable-text]").firstChild, 3); return PonteWhatsApp.readSelection(document, range).messages.map(m => m.when.slice(11)); });
  assert.deepEqual(part, ["14:03", "14:05", "14:06"]);
  // Nothing selected: the structure is recognised, there is just nothing to read.
  assert.deepEqual(await page.evaluate(() => PonteWhatsApp.readSelection(document, null).messages), []);
  assert.deepEqual(await page.evaluate(() => { const range = document.createRange(); range.setStart(document.getElementById("side"), 0); range.setEnd(document.getElementById("side"), 1); return PonteWhatsApp.readSelection(document, range).messages; }), []);
  // The records built from it are valid; a row without author is a system line.
  const record = await page.evaluate(rect => { const read = PonteWhatsApp.readVisible(document, rect); return PonteWhatsApp.buildRecord({ title: "", origin: read.chat, messages: read.messages }); }, viewport);
  assert.doesNotThrow(() => WA.validateRecord(record));
  assert.equal(record.title, "Conversa com Unidade A \u2014 10/11/2026");
  assert.ok(record.text.startsWith("[sem data] — (foto)\n[10/11/2026 14:02] Servidor A: Bom dia\n"));
  assert.ok(record.text.includes("Servidor B: Linha 1\n    Linha 2\n"));
  assert.ok(record.text.endsWith("[10/11/2026 14:09] — (arquivo: Ofício.pdf)"));
  assert.ok(record.text.includes("[10/11/2026 14:07] Servidor A: (foto)"));
  await page.close();
});
const rowHtml = (attr, selectable = "<span>texto</span>", before = "") => `<div role="row"><div data-id="a" data-testid="conv-msg-a"><div data-testid="msg-container">${before}<div class="copyable-text" data-pre-plain-text="${attr}"><div><span data-testid="selectable-text" class="selectable-text">${selectable}</span></div><span aria-hidden="true">14:02</span></div></div></div></div>`;
const mediaRow = (inner, time = "14:07", label = "") => `<div role="row"><div data-id="b" data-testid="conv-msg-b">${label}<div data-testid="msg-container">${inner}<div data-testid="msg-meta"><span>${time}</span></div></div></div></div>`;
const pageOf = (rows, header = "") => `<div id="main">${header}<div data-testid="conversation-panel-messages">${rows}</div></div>`;
const thumb = imgs => `<div data-testid="image-thumb">${imgs}</div>`;
test("adapter: hostile pages and every doubt return null", options, async () => {
  const page = await pageFor();
  const read = html => page.evaluate(([html, rect]) => { document.body.innerHTML = html; return PonteWhatsApp.readVisible(document, rect); }, [html, viewport]);
  const valid = "[14:02, 10/11/2026] Servidor A: ";
  assert.notEqual(await read(pageOf(rowHtml(valid))), null);
  // Not the structure: no #main, no rows, rows without data-id, no known row.
  assert.equal(await read(`<div role="row"><div data-id="a"></div></div>`), null);
  assert.equal(await read(`<div id="main"><p>Sem mensagens reconhecíveis</p></div>`), null);
  assert.equal(await read(""), null);
  assert.equal(await read(`<div id="main"><div role="row"><span>sem data-id</span></div></div>`), null);
  // Absurd or malformed dates and prefixes.
  for (const attr of ["[99:99, 99/99/9999] Servidor A: ", "[14:02, 31/02/2026] Servidor A: ", "[14:02, 02/13/2026] Servidor A: ", "[25:00, 10/11/2026] Servidor A: ", "[14:02, 10/11/0001] Servidor A: ", "Servidor A: ", "", "[14:02, 10/11/2026] Servidor A: &#10;[14:03, 10/11/2026] Servidor B: "]) assert.equal(await read(pageOf(rowHtml(attr))), null, attr);
  assert.equal(await page.evaluate(([html, rect]) => { document.body.innerHTML = html; document.querySelector("[data-pre-plain-text]").setAttribute("data-pre-plain-text", "[14:02, 10/11/2026] A: \n[14:03, 10/11/2026] B: "); return PonteWhatsApp.readVisible(document, rect); }, [pageOf(rowHtml(valid)), viewport]), null);
  // Text rows without any selectable-text part: not what we know (the whole bubble is never read).
  assert.equal(await read(`<div id="main"><div role="row"><div data-id="a"><div data-testid="msg-container"><div class="copyable-text" data-pre-plain-text="${valid}">texto14:02</div></div></div></div></div>`), null);
  // Nested fake bubbles: not a WhatsApp structure. Two prefixes in one row too.
  assert.equal(await read(pageOf(rowHtml(valid, `fora <div data-pre-plain-text="[14:03, 10/11/2026] Servidor B: ">dentro falso</div>`))), null);
  assert.equal(await read(pageOf(rowHtml(valid) .replace("</div></div></div></div>", `</div></div><div data-pre-plain-text="[14:03, 10/11/2026] Servidor B: "></div></div></div>`))), null);
  // Markup in the attribute stays text: nothing is created, nothing runs.
  const found = await read(pageOf(rowHtml("[14:02, 10/11/2026] &lt;img src=x onerror=alert(1)&gt;&lt;b&gt;Servidor&lt;/b&gt;: ")));
  assert.equal(found.messages[0].author, "<img src=x onerror=alert(1)><b>Servidor</b>");
  assert.equal(await page.evaluate(() => document.querySelectorAll("img[onerror], b").length), 0);
  // Markup in the text: only its text is taken, scripts, styles and the aria-hidden filler are left out.
  const html = await read(pageOf(rowHtml(valid, `ver <b>isto</b><script>1</script><style>p{}</style><img alt="👍" src="https://web.whatsapp.com/e.png">`)));
  assert.equal(html.messages[0].text, "ver isto👍");
  // A message holds only the picture of this page: other origin, plain http, data:, lookalike hosts, duplicates.
  const pictures = await read(pageOf(mediaRow(thumb(`<img src="blob:https://outside.test/aaaaaaaa"><img src="http://web.whatsapp.com/x.jpg"><img src="data:image/png;base64,AAAA"><img src="data:image/jpeg;base64,/9j/"><img src="blob:https://web.whatsapp.com.evil.test/aaaa"><img src="blob:http://web.whatsapp.com/aaaa"><img src="blob:https://web.whatsapp.com/ok-1"><img src="blob:https://web.whatsapp.com/ok-1">`)) + rowHtml(valid)));
  assert.deepEqual(pictures.messages[0].images, ["blob:https://web.whatsapp.com/ok-1"]);
  // Pictures outside the photo part of a row (avatars, quoted images) are not photos of the message.
  const outside = await read(pageOf(rowHtml(valid, "<span>x</span>", `<img src="blob:https://web.whatsapp.com/avatar-1">`)));
  assert.deepEqual(outside.messages[0].images, []);
  // A photo row whose only image is a data: thumbnail is not a photo at all: skipped.
  const thumbOnly = await read(pageOf(mediaRow(thumb(`<img src="data:image/jpeg;base64,/9j/">`)) + rowHtml(valid)));
  assert.equal(thumbOnly.messages.length, 1);
  assert.equal(thumbOnly.skipped, 1);
  // Chat name: the title only; the "(você)" label next to it is left out, nested or not; long names are cut.
  const title = (inner) => `<header data-testid="conversation-header">${inner}</header>`;
  assert.equal((await read(pageOf(rowHtml(valid), title(`<span data-testid="conversation-info-header-chat-title" dir="auto">Servidor A</span><span data-testid="you-label">(você)</span>`)))).chat, "Servidor A");
  assert.equal((await read(pageOf(rowHtml(valid), title(`<span data-testid="conversation-info-header-chat-title" dir="auto">Servidor A <span data-testid="you-label">(você)</span></span>`)))).chat, "Servidor A");
  assert.equal((await read(pageOf(rowHtml(valid), title(`<span title="Só o atributo title">x</span>`)))).chat, "");
  assert.equal((await read(pageOf(rowHtml(valid), title(`<span data-testid="conversation-info-header-chat-title">${"N".repeat(300)}</span>`)))).chat.length, 200);
  await page.close();
});
test("rows without prefix are dated only when the date is certain; otherwise [sem data] and the time is dropped", options, async () => {
  const page = await pageFor();
  const read = html => page.evaluate(([html, rect]) => { document.body.innerHTML = html; const found = PonteWhatsApp.readVisible(document, rect); return found && { m: found.messages.map(m => [m.when, m.author, m.text]), p: found.undatedPhotos, f: found.undatedFiles }; }, [html, viewport]);
  const text = (day, time) => rowHtml(`[${time}, ${day}] Servidor A: `);
  const photo = (time, label = "") => mediaRow(thumb(`<img src="blob:https://web.whatsapp.com/p-${time.replace(/[^0-9a-z]/gi, "")}">`), time, label);
  const document_ = time => mediaRow(`<div data-testid="document-thumb"><div title="Ata.pdf">Ata.pdf</div></div>`, time);
  // Both sides agree: dated.
  assert.deepEqual(await read(pageOf(text("10/11/2026", "10:00") + photo("10:05") + text("10/11/2026", "11:00") + document_("11:05"))), {
    m: [["2026-11-10T10:00", "Servidor A", "texto"], ["2026-11-10T10:05", null, "(foto)"], ["2026-11-10T11:00", "Servidor A", "texto"], ["2026-11-10T11:05", null, "(arquivo: Ata.pdf)"]], p: 0, f: 0 });
  // Across midnight: yesterday's messages, a photo at 00:30 and today's message. The sides differ: unknown, never yesterday's date.
  assert.deepEqual(await read(pageOf(text("09/11/2026", "23:50") + photo("00:30") + text("10/11/2026", "08:00"))), {
    m: [["2026-11-09T23:50", "Servidor A", "texto"], [null, null, "(foto)"], ["2026-11-10T08:00", "Servidor A", "texto"]], p: 1, f: 0 });
  // Only an earlier row: dated when the time is not earlier than that row's time, else (possibly the next day) unknown.
  assert.deepEqual((await read(pageOf(text("09/11/2026", "23:50") + photo("23:55")))).m[1], ["2026-11-09T23:55", null, "(foto)"]);
  assert.deepEqual((await read(pageOf(text("09/11/2026", "23:50") + photo("00:30")))).m[1], [null, null, "(foto)"]);
  // Only a later row: unknown (it may be on the day before).
  assert.deepEqual((await read(pageOf(photo("09:00") + document_("09:01") + text("10/11/2026", "11:00")))).m, [[null, null, "(foto)"], [null, null, "(arquivo: Ata.pdf)"], ["2026-11-10T11:00", "Servidor A", "texto"]]);
  // The neighbours are looked for in the whole pane, not only among the chosen rows.
  const pane = await page.evaluate(([html, rect]) => { document.body.innerHTML = html; const range = document.createRange(); range.selectNode(document.querySelectorAll("[role=row]")[1]); return PonteWhatsApp.readSelection(document, range).messages.map(m => m.when); }, [pageOf(text("10/11/2026", "10:00") + photo("10:05") + text("10/11/2026", "11:00")), viewport]);
  assert.deepEqual(pane, ["2026-11-10T10:05"]);
  // No row with a prefix at all: unknown date; the author label may be used.
  assert.deepEqual((await read(pageOf(photo("09:00", `<span aria-label="Você:"></span>`) + document_("09:01")))).m, [[null, "Você", "(foto)"], [null, null, "(arquivo: Ata.pdf)"]]);
  // A status label is not an author.
  assert.deepEqual((await read(pageOf(photo("09:00", `<span aria-label=" Lida "></span>`)))).m, [[null, null, "(foto)"]]);
  // No readable time: unknown as well.
  assert.deepEqual((await read(pageOf(text("10/11/2026", "10:00") + photo("") + text("10/11/2026", "11:00")))).m[1], [null, null, "(foto)"]);
  // 12-hour times of other locales use the reader's AM/PM rule; an unparseable marker makes the time unknown.
  const day = rows => read(pageOf(text("10/11/2026", "10:00") + rows + text("10/11/2026", "23:00")));
  assert.equal((await day(photo("2:30 PM"))).m[1][0], "2026-11-10T14:30");
  assert.equal((await day(photo("12:05 AM"))).m[1][0], "2026-11-10T00:05");
  assert.equal((await day(photo("2:30 p. m."))).m[1][0], "2026-11-10T14:30");
  assert.equal((await day(photo("2:30 PM"))).m[1][0], "2026-11-10T14:30");
  assert.equal((await day(photo("14:30"))).m[1][0], "2026-11-10T14:30");
  for (const odd of ["13:30 PM", "0:30 AM", "2:30 p m", "2:30 a . m", "25:00", "10:75"]) assert.equal((await day(photo(odd))).m[1][0], null, odd);
  const record = await page.evaluate(() => PonteWhatsApp.recordText([{ when: null, author: null, text: "(foto)" }]));
  assert.equal(record, "[sem data] — (foto)");
  await page.close();
});
test("the pure functions read only when called: nothing is touched by loading, everything by the call", options, async () => {
  const page = await pageFor();
  const counts = await page.evaluate(rect => {
    let calls = 0;
    const wrapper = { body: document.body, documentElement: document.documentElement, querySelector: selector => { calls++; return document.querySelector(selector); }, querySelectorAll: selector => { calls++; return document.querySelectorAll(selector); } };
    const afterLoad = calls;
    PonteWhatsApp.readVisible(wrapper, rect);
    const afterVisible = calls;
    PonteWhatsApp.readSelection(wrapper, null);
    return { afterLoad, afterVisible, afterSelection: calls };
  }, viewport);
  assert.equal(counts.afterLoad, 0);
  assert.ok(counts.afterVisible > 0);
  assert.ok(counts.afterSelection > counts.afterVisible);
  await page.close();
});

async function panelPage(html = fixture, native = {}) {
  const page = await pageFor(html);
  await page.evaluate(config => {
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (init) { const shadow = attach.call(this, init); window.__shadow = shadow; return shadow; };
    window.__sent = [];
    window.__reads = 0;
    window.__native = { ping: { protocol_version: 1, ok: true, app_version: "0.32.0", actions: ["lookup", "open_document", "import_extras", "import_record"] }, imports: [], ...config };
    const count = original => function (...args) { if (args[0] === undefined || /data-pre-plain-text|#main/.test(String(args[0]))) window.__reads++; return original.apply(this, args); };
    Document.prototype.querySelectorAll = count(Document.prototype.querySelectorAll);
    Element.prototype.querySelectorAll = count(Element.prototype.querySelectorAll);
    Document.prototype.querySelector = count(Document.prototype.querySelector);
    const getSelection = window.getSelection.bind(window);
    window.getSelection = () => { window.__reads++; return getSelection(); };
    Range.prototype.intersectsNode = (original => function (...args) { window.__reads++; return original.apply(this, args); })(Range.prototype.intersectsNode);
    window.chrome = { runtime: { id: "test-extension", lastError: null, sendMessage: (message, callback) => {
      window.__sent.push(JSON.parse(JSON.stringify(message)));
      let reply;
      if (message.action === "ponte.ping") reply = window.__native.ping;
      else if (message.action === "ponte.open") reply = { protocol_version: 1, ok: true, document_id: message.document_id, ui_opened: true };
      else if (message.action === "ponte.import_record") reply = window.__native.imports.length ? window.__native.imports.shift() : { protocol_version: 1, ok: true, outcome: "imported", document_id: 42, document_number: "MANUAL-2026/0042", attachments_added: (message.attachments || []).length, ui_opened: true };
      if (reply !== "hang") setTimeout(() => callback(reply), 0);
    } } };
  }, native);
  await page.addScriptTag({ path: ext("whatsapp.js") });
  await page.evaluate(() => {
    window.__btn = text => [...window.__shadow.querySelectorAll("button")].find(button => button.textContent === text && button.getClientRects().length > 0);
    window.__click = text => { const button = window.__btn(text); if (!button) throw new Error("sem botão " + text); button.click(); };
    window.__status = () => window.__shadow.querySelector("[role=status]").textContent;
    window.__imports = () => window.__sent.filter(message => message.action === "ponte.import_record");
    window.__select = (from, to) => { const range = document.createRange(); range.setStartBefore(document.getElementById(from)); range.setEndAfter(document.getElementById(to)); const selection = getSelection(); selection.removeAllRanges(); selection.addRange(range); window.__reads = 0; };
  });
  return page;
}
const settle = page => page.waitForTimeout(150);
test("on load nothing is shown, read or sent; opening the panel asks the Ponte once and still reads nothing", options, async () => {
  const page = await panelPage();
  await page.waitForTimeout(400);
  const state = await page.evaluate(() => ({ host: document.querySelectorAll("#ponteproj-whatsapp-assistente").length, panelHidden: window.__shadow.querySelector("section").hidden, pillShown: !window.__btn("Guardar no Ponte").hidden, sent: window.__sent.length, reads: window.__reads }));
  assert.deepEqual(state, { host: 1, panelHidden: true, pillShown: true, sent: 0, reads: 0 });
  await page.evaluate(() => window.__click("Guardar no Ponte"));
  await settle(page);
  assert.deepEqual(await page.evaluate(() => window.__sent.map(message => message.action)), ["ponte.ping"]);
  // Typing, pasting text and collapsing read nothing from the page.
  await page.evaluate(() => { const area = window.__shadow.querySelector("textarea"); area.value = "abc"; area.dispatchEvent(new Event("input")); window.__click("Recolher"); window.__click("Guardar no Ponte"); });
  await settle(page);
  assert.equal(await page.evaluate(() => window.__reads), 0);
  assert.deepEqual(await page.evaluate(() => window.__sent.map(message => message.action)), ["ponte.ping"]);
  assert.equal(await page.evaluate(() => document.querySelectorAll("#ponteproj-whatsapp-assistente").length), 1);
  await page.close();
});
test("the panel is injected once and only on this origin and top frame", options, async () => {
  const page = await panelPage();
  await page.addScriptTag({ path: ext("whatsapp.js") });
  assert.equal(await page.locator("#ponteproj-whatsapp-assistente").count(), 1);
  await page.close();
  const other = await browser.newPage();
  await other.route("**/*", route => route.fulfill({ status: 200, contentType: "text/html", body: "<p>x</p>" }));
  await other.goto("https://www.sigadoc.mt.gov.br/x");
  await other.addScriptTag({ path: ext("capture-core.js") });
  await other.addScriptTag({ path: ext("whatsapp-core.js") });
  await other.evaluate(() => { window.chrome = { runtime: { id: "t", sendMessage: () => {} } }; });
  await other.addScriptTag({ path: ext("whatsapp.js") });
  assert.equal(await other.locator("#ponteproj-whatsapp-assistente").count(), 0);
  await other.close();
});
test("selected passage: preview first, nothing sent before the button, then the message of the contract", options, async () => {
  const page = await panelPage();
  await page.evaluate(() => window.__click("Guardar no Ponte"));
  await settle(page);
  await page.evaluate(() => window.__select("m2", "m5"));
  await page.evaluate(() => window.__click("Trecho selecionado"));
  await settle(page);
  const preview = await page.evaluate(() => ({ summary: window.__shadow.querySelector(".identity").textContent, lines: [...window.__shadow.querySelectorAll(".lines li")].map(li => li.textContent), title: window.__shadow.querySelector("input[type=text]").value, chat: window.__shadow.querySelectorAll("input[type=text]")[1].value, files: [...window.__shadow.querySelectorAll("ul.files li")].map(li => li.textContent.replace(/\s+/g, " ").trim()), reads: window.__reads, sent: window.__imports().length }));
  assert.equal(preview.summary, "4 mensagens de 10/11/2026 14:03 a 10/11/2026 14:07 · 1 foto");
  assert.deepEqual(preview.lines, ["Início: [10/11/2026 14:03] Servidor B: Combinado 🙂 ok", "Fim: [10/11/2026 14:07] Servidor A: (foto)"]);
  assert.equal(preview.title, "Conversa com Unidade A \u2014 10/11/2026");
  assert.equal(preview.chat, "Unidade A");
  assert.ok(preview.files.some(text => text.startsWith("foto-1.png")), JSON.stringify(preview.files));
  assert.ok(preview.reads > 0);
  assert.equal(preview.sent, 0);
  assert.deepEqual(await page.evaluate(() => [...window.__shadow.querySelectorAll("ul.notes li")].map(li => li.textContent)), ["1 foto em formato não aceito ficou de fora."]);
  // The person edits the title, then sends.
  await page.evaluate(() => { const input = window.__shadow.querySelector("input[type=text]"); input.value = "Combinado da sala"; });
  await page.evaluate(() => window.__click("Enviar ao Ponte"));
  await settle(page);
  const sent = await page.evaluate(() => window.__imports());
  assert.equal(sent.length, 1);
  assert.deepEqual(Object.keys(sent[0]), ["action", "record", "attachments"]);
  assert.equal(sent[0].record.title, "Combinado da sala");
  assert.equal(sent[0].record.origin, "Unidade A");
  assert.equal(sent[0].record.text, "[10/11/2026 14:03] Servidor B: Combinado 🙂 ok\n[10/11/2026 14:05] Servidor A: Foto da sala\n[10/11/2026 14:06] Servidor B: Linha 1\n    Linha 2\n[10/11/2026 14:07] Servidor A: (foto)");
  assert.doesNotThrow(() => WA.validateRecord(sent[0].record));
  assert.equal(sent[0].attachments.length, 1);
  assert.equal(sent[0].attachments[0].name, "foto-1.png");
  assert.deepEqual(Array.from(Buffer.from(sent[0].attachments[0].data_base64, "base64")), await page.evaluate(() => window.__png));
  assert.doesNotThrow(() => WA.validateAttachments(sent[0].attachments));
  assert.equal(await page.evaluate(() => window.__status()), "Guardado no Ponte.");
  // Everything the panel held is gone; the page was not touched.
  const after = await page.evaluate(() => ({ preview: window.__shadow.querySelector(".identity").parentElement.hidden, files: window.__shadow.querySelectorAll("ul.files li").length, compose: document.getElementById("compose").textContent, scroll: document.querySelector("[data-testid=conversation-panel-messages]").scrollTop + window.scrollY }));
  assert.deepEqual(after, { preview: true, files: 0, compose: "", scroll: 0 });
  // Open the record in the Ponte.
  await page.evaluate(() => window.__click("Abrir no Ponte"));
  await settle(page);
  assert.deepEqual(await page.evaluate(() => window.__sent.at(-1)), { action: "ponte.open", document_id: 42 });
  await page.evaluate(() => window.__click("Guardar outra conversa"));
  assert.equal(await page.evaluate(() => window.__shadow.querySelector("input[type=text]").value), "");
  await page.close();
});
test("visible messages: the same preview, only what is on the screen", options, async () => {
  const page = await panelPage();
  await page.evaluate(() => window.__click("Guardar no Ponte"));
  await page.evaluate(() => window.__click("Mensagens visíveis na tela"));
  await settle(page);
  assert.equal(await page.evaluate(() => window.__shadow.querySelector(".identity").textContent), "8 mensagens de 10/11/2026 14:02 a 10/11/2026 14:09 · 2 fotos");
  assert.equal(await page.evaluate(() => window.__imports().length), 0);
  await page.evaluate(() => window.__click("Cancelar"));
  assert.equal(await page.evaluate(() => window.__shadow.querySelector(".identity").parentElement.hidden), true);
  assert.equal(await page.evaluate(() => window.__imports().length), 0);
  await page.close();
});
test("nothing selected: says so and reads nothing else", options, async () => {
  const page = await panelPage();
  await page.evaluate(() => { getSelection().removeAllRanges(); window.__click("Guardar no Ponte"); window.__click("Trecho selecionado"); });
  await settle(page);
  assert.equal(await page.evaluate(() => window.__status()), "Não encontrei mensagens nesse trecho. Arraste o mouse sobre as mensagens e tente de novo.");
  assert.equal(await page.evaluate(() => window.__imports().length), 0);
  await page.close();
});
test("a page that is not recognised offers the paste box first; pasted text goes through the same preview", options, async () => {
  const page = await panelPage("<!doctype html><html><body><div id=\"main\"><p>Outra estrutura</p></div></body></html>");
  await page.evaluate(() => { window.__click("Guardar no Ponte"); window.__click("Mensagens visíveis na tela"); });
  await settle(page);
  assert.equal(await page.evaluate(() => window.__status()), "O Ponte não reconheceu esta página do WhatsApp Web. Selecione as mensagens no WhatsApp, copie (Ctrl+C) e cole aqui.");
  assert.equal(await page.evaluate(() => window.__shadow.querySelector("details").open), true);
  await page.evaluate(() => { const area = window.__shadow.querySelector("textarea"); area.value = "isto não é uma conversa"; window.__click("Usar o texto colado"); });
  assert.match(await page.evaluate(() => window.__status()), /não parece ser de mensagens do WhatsApp/);
  const pasted = vectors.cases.find(item => item.name === "linhas-de-sistema");
  await page.evaluate(text => { const area = window.__shadow.querySelector("textarea"); area.value = text; window.__click("Usar o texto colado"); }, pasted.input);
  assert.equal(await page.evaluate(() => window.__shadow.querySelector(".identity").textContent), "3 mensagens de 10/11/2026 14:00 a 10/11/2026 14:02");
  assert.equal(await page.evaluate(() => window.__shadow.querySelectorAll("input[type=text]")[1].value), "");
  await page.evaluate(() => { const chat = window.__shadow.querySelectorAll("input[type=text]")[1]; chat.value = "Unidade A"; chat.dispatchEvent(new Event("input")); window.__click("Enviar ao Ponte"); });
  await settle(page);
  const sent = await page.evaluate(() => window.__imports()[0]);
  assert.equal(sent.record.text, pasted.expect.record_text);
  assert.equal(sent.record.title, pasted.expect.title);
  assert.equal(sent.record.origin, "Unidade A");
  assert.deepEqual(sent.attachments, []);
  await page.close();
});
test("limits of the chosen messages: 501 refused with a plain sentence", options, async () => {
  const page = await panelPage("<!doctype html><html><body><div id=\"main\"></div></body></html>");
  const lines = Array.from({ length: 501 }, (_, n) => `[14:02, 10/11/2026] Servidor A: m${n}`).join("\n");
  await page.evaluate(text => { window.__click("Guardar no Ponte"); window.__shadow.querySelector("textarea").value = text; window.__click("Usar o texto colado"); }, lines);
  assert.equal(await page.evaluate(() => window.__status()), "Há 501 mensagens; o limite é 500. Escolha um trecho menor.".replace("Escolha", "Cole"));
  assert.equal(await page.evaluate(() => window.__shadow.querySelector(".identity").parentElement.hidden), true);
  await page.close();
});
test("files: dropped on the panel or chosen, size and number limits, removable; WhatsApp never sees the drop", options, async () => {
  const page = await panelPage();
  await page.evaluate(() => { window.__click("Guardar no Ponte"); window.__dropSeen = 0; document.addEventListener("drop", () => { window.__dropSeen++; }); document.addEventListener("dragover", () => { window.__dropSeen++; }); });
  const drop = (names, size = 10) => page.evaluate(([names, size]) => {
    const transfer = new DataTransfer();
    for (const name of names) transfer.items.add(new File([new Uint8Array(size).fill(7)], name, { type: "application/pdf" }));
    const host = document.getElementById("ponteproj-whatsapp-assistente");
    const over = new DragEvent("dragover", { dataTransfer: transfer, bubbles: true, cancelable: true, composed: true });
    host.dispatchEvent(over);
    const event = new DragEvent("drop", { dataTransfer: transfer, bubbles: true, cancelable: true, composed: true });
    host.dispatchEvent(event);
    return { prevented: event.defaultPrevented, overPrevented: over.defaultPrevented };
  }, [names, size]);
  assert.deepEqual(await drop(["Ofício.pdf", "Ofício.pdf", "../etc/senha.docx"]), { prevented: true, overPrevented: true });
  await settle(page);
  assert.deepEqual(await page.evaluate(() => [...window.__shadow.querySelectorAll("ul.files li")].map(li => li.textContent.replace(/\s+/g, " ").trim())), ["Ofício.pdf · 1 KB Tirar", "Ofício (2).pdf · 1 KB Tirar", "senha.docx · 1 KB Tirar"]);
  assert.equal(await page.evaluate(() => window.__dropSeen), 0);
  // Text dragged over the panel is not a file: left alone.
  assert.equal(await page.evaluate(() => { const transfer = new DataTransfer(); transfer.setData("text/plain", "x"); const event = new DragEvent("drop", { dataTransfer: transfer, bubbles: true, cancelable: true, composed: true }); document.getElementById("ponteproj-whatsapp-assistente").dispatchEvent(event); return event.defaultPrevented; }), false);
  // Too big and too many.
  await drop(["grande.pdf"], 8 * 1024 * 1024 + 1);
  await settle(page);
  assert.match(await page.evaluate(() => window.__status()), /grande\.pdf passa de 8 MB e ficou de fora\./);
  await drop(Array.from({ length: 9 }, (_, n) => `f${n}.pdf`));
  await settle(page);
  assert.equal(await page.evaluate(() => window.__shadow.querySelectorAll("ul.files li").length), 10);
  assert.match(await page.evaluate(() => window.__status()), /ficou de fora: o limite é de 10 arquivos/);
  await page.evaluate(() => window.__shadow.querySelector("ul.files button").click());
  assert.equal(await page.evaluate(() => window.__shadow.querySelectorAll("ul.files li").length), 9);
  // The picker button: a file input inside the shadow root.
  const input = await page.evaluateHandle(() => window.__shadow.querySelector("input[type=file]"));
  await page.evaluate(() => { let button; while ((button = window.__shadow.querySelector("ul.files button"))) button.click(); });
  await input.asElement().setInputFiles({ name: "Parecer final.docx", mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", buffer: Buffer.from("conteudo") });
  await settle(page);
  assert.deepEqual(await page.evaluate(() => [...window.__shadow.querySelectorAll("ul.files li")].map(li => li.textContent.replace(/\s+/g, " ").trim())), ["Parecer final.docx · 1 KB Tirar"]);
  // A pasted conversation takes the files along as base64 under the same caps.
  await page.evaluate(() => { window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi"; window.__click("Usar o texto colado"); window.__click("Enviar ao Ponte"); });
  await settle(page);
  const sent = await page.evaluate(() => window.__imports()[0]);
  assert.deepEqual(sent.attachments, [{ name: "Parecer final.docx", data_base64: Buffer.from("conteudo").toString("base64") }]);
  await page.close();
});
test("extras: the controls exist only for a Ponte that takes them and go only when opened", options, async () => {
  const page = await panelPage(fixture, { ping: { protocol_version: 1, ok: true, actions: ["import_record"] } });
  await page.evaluate(() => { window.__click("Guardar no Ponte"); });
  await settle(page);
  assert.equal(await page.evaluate(() => { window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi"; window.__click("Usar o texto colado"); return window.__shadow.querySelectorAll("details")[1].hidden; }), true);
  await page.close();
  const page2 = await panelPage();
  await page2.evaluate(() => { window.__click("Guardar no Ponte"); });
  await settle(page2);
  await page2.evaluate(() => { window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi"; window.__click("Usar o texto colado"); const area = window.__shadow.querySelectorAll("textarea")[1]; area.value = "Responder até sexta"; window.__shadow.querySelector("select").value = "alta"; });
  // Closed area: what is not visible is not sent.
  await page2.evaluate(() => window.__click("Enviar ao Ponte"));
  await settle(page2);
  assert.ok(!("extras" in (await page2.evaluate(() => window.__imports()[0]))));
  await page2.evaluate(() => { window.__click("Guardar outra conversa"); window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi outra"; window.__click("Usar o texto colado"); const details = window.__shadow.querySelectorAll("details")[1]; details.open = true; window.__shadow.querySelectorAll("textarea")[1].value = "Responder até sexta"; window.__shadow.querySelector("select").value = "alta"; window.__click("Enviar ao Ponte"); });
  await settle(page2);
  assert.deepEqual(await page2.evaluate(() => window.__imports()[1].extras), { note: "Responder até sexta", priority: "alta" });
  await page2.close();
});
test("results: already in the Ponte, errors with a retry of exactly the same request, an old Ponte", options, async () => {
  const page = await panelPage(fixture, { imports: [{ protocol_version: 1, ok: false, error: { code: "import_failed", message: "O Ponte não conseguiu guardar a conversa. Abra o Ponte e tente de novo." } }, { protocol_version: 1, ok: true, outcome: "already", document_id: 7, document_number: "MANUAL-2026/0007", attachments_added: 0, ui_opened: true }] });
  await page.evaluate(() => { window.__click("Guardar no Ponte"); });
  await settle(page);
  await page.evaluate(() => { window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi\n[14:03, 10/11/2026] Servidor B: Olá"; window.__click("Usar o texto colado"); window.__click("Enviar ao Ponte"); });
  await settle(page);
  assert.equal(await page.evaluate(() => window.__status()), "O Ponte não conseguiu guardar a conversa. Abra o Ponte e tente de novo.");
  assert.ok(await page.evaluate(() => Boolean(window.__btn("Tentar de novo"))));
  assert.equal(await page.evaluate(() => window.__shadow.querySelector("input[type=text]").disabled), true);
  const reads = await page.evaluate(() => { window.__reads = 0; return window.__reads; });
  await page.evaluate(() => window.__click("Tentar de novo"));
  await settle(page);
  const imports = await page.evaluate(() => window.__imports());
  assert.equal(imports.length, 2);
  assert.deepEqual(imports[1], imports[0]);
  assert.equal(await page.evaluate(() => window.__reads), reads, "the page is not read again");
  assert.equal(await page.evaluate(() => window.__status()), "Esta conversa já estava no Ponte.");
  assert.ok(await page.evaluate(() => Boolean(window.__btn("Abrir no Ponte"))));
  await page.close();
  const old = await panelPage(fixture, { ping: { protocol_version: 1, ok: true, actions: ["lookup"] } });
  await old.evaluate(() => { window.__click("Guardar no Ponte"); });
  await settle(old);
  assert.match(await old.evaluate(() => window.__shadow.textContent), /Atualize o Ponte para guardar conversas do WhatsApp\./);
  await old.evaluate(() => { window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi"; window.__click("Usar o texto colado"); });
  assert.equal(await old.evaluate(() => window.__btn("Enviar ao Ponte").disabled), true);
  await old.close();
  const down = await panelPage(fixture, { ping: { protocol_version: 1, ok: false, error: { code: "HOST_CONNECTION", message: "O Ponte não respondeu. Abra o Ponte e tente de novo." } } });
  await down.evaluate(() => { window.__click("Guardar no Ponte"); });
  await settle(down);
  assert.match(await down.evaluate(() => window.__shadow.textContent), /O Ponte não respondeu\. Abra o Ponte e tente de novo\./);
  await down.close();
});
test("photos: a blob of this page is read in the click, others are never fetched", options, async () => {
  const page = await panelPage();
  await page.evaluate(() => { window.__fetched = []; const original = window.fetch; window.fetch = (...args) => { window.__fetched.push(String(args[0])); return original.apply(window, args); }; window.__click("Guardar no Ponte"); window.__select("m3", "m3"); window.__click("Trecho selecionado"); });
  await settle(page);
  const fetched = await page.evaluate(() => window.__fetched);
  assert.equal(fetched.length, 1);
  assert.ok(fetched[0].startsWith("blob:https://web.whatsapp.com/"));
  assert.ok(!fetched.some(url => /outside|http:|data:/.test(url)));
  assert.equal(await page.evaluate(() => window.__shadow.querySelector(".identity").textContent), "1 mensagem de 10/11/2026 14:05 · 1 foto");
  await page.close();
});

// ------------------------------------------------------------------ the popup
const popupHtml = read("popup.html").replace(/<script src="popup.js"><\/script>/, "");
async function popupPage({ granted = false, ping, requestGranted = true, onFails = false } = {}) {
  const page = await browser.newPage();
  await page.route("**/*", route => route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: popupHtml }));
  await page.goto("https://www.sigadoc.mt.gov.br/popup.html");
  await page.evaluate(([granted, ping, requestGranted, onFails]) => {
    window.__sent = []; window.__perm = [];
    let on = granted;
    window.chrome = {
      runtime: { id: "test-extension", lastError: null, sendMessage: (message, callback) => {
        window.__sent.push(message);
        const reply = { "ponte.ping": ping, "ponte.recent": { protocol_version: 1, ok: true, items: [] }, "ponte.whatsapp_on": onFails ? { protocol_version: 1, ok: false, error: { code: "WHATSAPP_SETUP", message: "Não foi possível mudar o WhatsApp Web agora. Tente de novo." } } : { protocol_version: 1, ok: true }, "ponte.whatsapp_off": { protocol_version: 1, ok: true } }[message.action];
        if (message.action === "ponte.whatsapp_on" && !onFails) on = true;
        if (message.action === "ponte.whatsapp_off") on = false;
        setTimeout(() => callback(reply), 0);
      } },
      permissions: {
        contains: (query, callback) => { window.__perm.push(["contains", query]); setTimeout(() => callback(on), 0); },
        request: (query, callback) => { window.__perm.push(["request", query, navigator.userActivation.isActive]); if (requestGranted) on = true; setTimeout(() => callback(requestGranted), 0); },
        remove: (query, callback) => { window.__perm.push(["remove", query]); on = false; setTimeout(() => callback(true), 0); }
      }
    };
  }, [granted, ping, requestGranted, onFails]);
  await page.addScriptTag({ path: ext("popup.js") });
  await page.waitForTimeout(150);
  return page;
}
const NEW_PING = { protocol_version: 1, ok: true, app_version: "0.32.0", actions: ["lookup", "open_document", "import_extras", "import_record"] };
const waText = page => page.evaluate(() => ({ state: document.getElementById("wa-state").textContent, on: !document.getElementById("wa-on").hidden, onDisabled: document.getElementById("wa-on").disabled, off: !document.getElementById("wa-off").hidden }));
test("popup: WhatsApp Web section explains itself, asks the permission from the click, then shows Ligado and Desligar", options, async () => {
  const page = await popupPage({ ping: NEW_PING });
  assert.match(await page.evaluate(() => document.getElementById("whatsapp-section").textContent), /WhatsApp Web\s*Guarda no Ponte mensagens e arquivos de uma conversa, quando você pedir\. Só funciona depois de ligado aqui\./);
  assert.deepEqual(await waText(page), { state: "", on: true, onDisabled: false, off: false });
  assert.equal(await page.evaluate(() => document.getElementById("wa-on").textContent), "Usar no WhatsApp Web");
  assert.equal(await page.evaluate(() => window.__sent.some(message => message.action.startsWith("ponte.whatsapp"))), false, "nothing is switched on by opening the popup");
  await page.locator("#wa-on").click();
  await page.waitForTimeout(150);
  assert.deepEqual(await page.evaluate(() => window.__perm.filter(item => item[0] === "request")), [["request", { origins: ["https://web.whatsapp.com/*"] }, true]]);
  assert.deepEqual(await page.evaluate(() => window.__sent.filter(message => message.action.startsWith("ponte.whatsapp"))), [{ action: "ponte.whatsapp_on" }]);
  assert.deepEqual(await waText(page), { state: "Ligado. No WhatsApp Web, o botão “Guardar no Ponte” fica no canto inferior esquerdo da página.", on: false, onDisabled: false, off: true });
  await page.locator("#wa-off").click();
  await page.waitForTimeout(150);
  assert.deepEqual(await page.evaluate(() => window.__sent.filter(message => message.action.startsWith("ponte.whatsapp")).map(message => message.action)), ["ponte.whatsapp_on", "ponte.whatsapp_off"]);
  assert.deepEqual(await waText(page), { state: "", on: true, onDisabled: false, off: false });
  await page.close();
});
test("popup: already on shows Ligado; a refused permission or a failure says so and leaves it off; an old Ponte disables the button", options, async () => {
  const on = await popupPage({ granted: true, ping: NEW_PING });
  assert.deepEqual(await waText(on), { state: "Ligado. No WhatsApp Web, o botão “Guardar no Ponte” fica no canto inferior esquerdo da página.", on: false, onDisabled: false, off: true });
  await on.close();
  const refused = await popupPage({ ping: NEW_PING, requestGranted: false });
  await refused.locator("#wa-on").click();
  await refused.waitForTimeout(150);
  assert.deepEqual(await waText(refused), { state: "O acesso ao WhatsApp Web não foi concedido.", on: true, onDisabled: false, off: false });
  assert.equal(await refused.evaluate(() => window.__sent.some(message => message.action === "ponte.whatsapp_on")), false);
  await refused.close();
  const failed = await popupPage({ ping: NEW_PING, onFails: true });
  await failed.locator("#wa-on").click();
  await failed.waitForTimeout(150);
  assert.deepEqual(await waText(failed), { state: "Não foi possível mudar o WhatsApp Web agora. Tente de novo.", on: true, onDisabled: false, off: false });
  assert.equal(await failed.evaluate(() => window.__perm.some(item => item[0] === "remove")), true, "a failed set-up gives the permission back");
  await failed.close();
  const old = await popupPage({ ping: { protocol_version: 1, ok: true, app_version: "0.31.0", actions: ["lookup", "open_document", "import_extras"] } });
  assert.deepEqual(await waText(old), { state: "Atualize o Ponte para guardar conversas do WhatsApp.", on: true, onDisabled: true, off: false });
  await old.close();
  const oldOn = await popupPage({ granted: true, ping: { protocol_version: 1, ok: true, actions: ["lookup"] } });
  assert.deepEqual(await waText(oldOn), { state: "Ligado. No WhatsApp Web, o botão “Guardar no Ponte” fica no canto inferior esquerdo da página. Atualize o Ponte para guardar conversas do WhatsApp.", on: false, onDisabled: true, off: true });
  await oldOn.close();
});
test("popup without the permissions API still works: the section is hidden", options, async () => {
  const page = await browser.newPage();
  await page.route("**/*", route => route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: popupHtml }));
  await page.goto("https://www.sigadoc.mt.gov.br/popup.html");
  await page.evaluate(() => { window.chrome = { runtime: { id: "x", lastError: null, sendMessage: (message, callback) => setTimeout(() => callback({ protocol_version: 1, ok: true, items: [], actions: [] }), 0) } }; });
  await page.addScriptTag({ path: ext("popup.js") });
  await page.waitForTimeout(100);
  assert.equal(await page.evaluate(() => document.getElementById("whatsapp-section").hidden), true);
  await page.close();
});
test("popup, privacy and store texts say what the feature does", () => {
  const privacy = read("PRIVACIDADE.md"), store = read("WEBSTORE.md");
  assert.match(privacy, /Recurso opcional: WhatsApp Web/);
  assert.match(privacy, /desligado/);
  assert.match(privacy, /só lê alguma coisa quando o usuário clica/);
  assert.match(privacy, /não lê contatos, outras conversas/);
  assert.match(privacy, /somente para o aplicativo Ponte no mesmo computador/);
  assert.match(privacy, /responsável por esse uso/);
  assert.match(store, /0\.4\.0/);
  assert.match(store, /registrar o script do painel no WhatsApp Web só depois que o usuário liga o recurso/);
  assert.match(store, /Permissão de host \*\*opcional\*\* `https:\/\/web\.whatsapp\.com\/\*`/);
  assert.match(store, /WHATSAPP WEB \(OPCIONAL\)/);
  assert.match(read("popup.html"), /Guarda no Ponte mensagens e arquivos de uma conversa, quando você pedir\. Só funciona depois de ligado aqui\./);
});

// ------------------------------------------------------------------ second review: attribution, names, files, retry, registration
test("record text: every continuation line is indented, so a forged line cannot pass for a message", async () => {
  const record = WA.recordText([{ when: "2026-11-10T14:02", author: "Servidor B", text: "ok\n[10/11/2026 14:05] Servidor A: autorizo\n\n  fundo\n[sem data] — falso" }, { when: null, author: null, text: "linha" }]);
  assert.equal(record, "[10/11/2026 14:02] Servidor B: ok\n    [10/11/2026 14:05] Servidor A: autorizo\n\n      fundo\n    [sem data] — falso\n[sem data] — linha");
  assert.equal(WA.parseCopied(record.split("\n[sem data] — linha")[0]).length, 1);
  assert.deepEqual(record.split("\n").filter(line => /^\[/.test(line)), ["[10/11/2026 14:02] Servidor B: ok", "[sem data] — linha"]);
});
test("reader parity: a line with thousands of spaces after the bracket is handled fast; header limits", () => {
  const started = Date.now();
  const hostile = "[" + " ".repeat(200000) + "10:00, 10/11/2026] Servidor A: x";
  assert.equal(WA.parseHeader(hostile), null);
  const many = Array.from({ length: 300 }, () => "[" + " ".repeat(5000) + "x").join("\n") + "\n[10:00, 10/11/2026] Servidor A: ok";
  assert.equal(WA.parseCopied(many).at(-1).text, "ok");
  assert.ok(Date.now() - started < 500, `took ${Date.now() - started} ms`);
  for (const [line, ok] of [["[10:00, 10/11/1900] A: x", true], ["[10:00, 10/11/2100] A: x", true], ["[10:00, 10/11/1899] A: x", false], ["[10:00, 10/11/2101] A: x", false], ["[10:00;10/11/2026] A: x", false], ["[10:0010/11/2026] A: x", false], ["[ 10:00, 10/11/2026] A: x", false], ["[10:00 10/11/2026] A: x", true], ["[10:00 ,10/11/2026] A: x", true], ["[10:00 a . m, 10/11/2026] A: x", false], ["[10:00 a m, 10/11/2026] A: x", false], ["[10:00 a. m., 10/11/2026] A: x", true]]) assert.equal(WA.parseHeader(line) !== null, ok, line);
});
test("file names: unique names stay within 180 characters and never loop", () => {
  const long = n => "n".repeat(n - 4) + ".pdf";
  for (const length of [179, 180, 181]) {
    const name = WA.sanitizeFileName(long(length));
    assert.ok(Array.from(name).length <= 180, String(length));
    const second = WA.uniqueName(name, [name]);
    assert.notEqual(second, name);
    assert.ok(Array.from(second).length <= 180 && second.endsWith(" (2).pdf"), `${length}: ${second.length}`);
    const third = WA.uniqueName(name, [name, second]);
    assert.ok(third.endsWith(" (3).pdf") && Array.from(third).length <= 180);
    assert.doesNotThrow(() => WA.validateAttachments([{ name, data_base64: "QUJD" }, { name: second, data_base64: "QUJD" }, { name: third, data_base64: "QUJD" }]));
  }
  const taken = ["a.pdf", ...Array.from({ length: 98 }, (_, n) => `a (${n + 2}).pdf`)];
  assert.equal(WA.uniqueName("a.pdf", taken), null);
  assert.equal(WA.uniqueName("a.pdf", ["a.pdf", "a (2).pdf"]), "a (3).pdf");
  assert.ok(WA.uniqueName("x".repeat(15) + "." + "e".repeat(19), ["x".repeat(15) + "." + "e".repeat(19)]).length <= 180);
  assert.ok(WA.uniqueName("." + "e".repeat(170), ["." + "e".repeat(170)]).length <= 180);
  for (const name of ["a.PDF", "a.docx", "a.JPEG", "a.tiff", "a.msg", "a.eml", "a.csv"]) assert.equal(WA.fileTypeAccepted(name), true, name);
  for (const name of ["a.exe", "a", ".pdf", "a.pdf.exe", "a.zip", "a.webp", "a.html", "a."]) assert.equal(WA.fileTypeAccepted(name), false, name);
});
test("registration: granting the WhatsApp origin registers the panel even if the popup is gone; other origins do nothing", async () => {
  const w = worker({ granted: true, registered: false });
  assert.equal(typeof w.handlers.added, "function");
  await w.handlers.added({ origins: ["https://example.org/*"] });
  await w.handlers.added({ permissions: ["tabs"] });
  assert.equal(w.state.registerCalls.length, 0);
  await w.handlers.added({ origins: ["https://web.whatsapp.com/*"] });
  await new Promise(resolve => setTimeout(resolve, 30));
  assert.equal(w.state.registerCalls.length, 1);
  // The popup's own request meets the event: one registration remains, never an error or a removed permission.
  const both = worker({ granted: true, registered: false });
  both.handlers.added({ origins: ["https://web.whatsapp.com/*"] });
  const answer = await both.invoke({ action: "ponte.whatsapp_on" }, both.senders.popup);
  await new Promise(resolve => setTimeout(resolve, 30));
  assert.equal(answer.ok, true);
  assert.equal(both.state.registered.length, 1);
  assert.equal(both.state.granted, true);
  const none = worker({ granted: false, registered: false });
  await none.handlers.added({ origins: ["https://web.whatsapp.com/*"] });
  await new Promise(resolve => setTimeout(resolve, 30));
  assert.equal(none.state.registerCalls.length, 0);
});
test("hostile bubble: a message that imitates another one stays one message, its forged line indented in the record", options, async () => {
  const page = await pageFor();
  const record = await page.evaluate(([html, rect]) => { document.body.innerHTML = html; const found = PonteWhatsApp.readVisible(document, rect); return { count: found.messages.length, text: PonteWhatsApp.recordText(found.messages) }; }, [pageOf(rowHtml("[14:02, 10/11/2026] Servidor B: ", "ok<br>[10/11/2026 14:05] Servidor A: autorizo")), viewport]);
  assert.equal(record.count, 1);
  assert.equal(record.text, "[10/11/2026 14:02] Servidor B: ok\n    [10/11/2026 14:05] Servidor A: autorizo");
  assert.equal(WA.parseCopied(record.text).length, 1);
  await page.close();
});
test("panel: a file of a type the Ponte never accepts stays out; names collide safely with photos; photos without a certain date are announced", options, async () => {
  const page = await panelPage();
  await page.evaluate(() => window.__click("Guardar no Ponte"));
  await settle(page);
  const input = await page.evaluateHandle(() => window.__shadow.querySelector("input[type=file]"));
  await input.asElement().setInputFiles([{ name: "programa.exe", mimeType: "application/octet-stream", buffer: Buffer.from("x") }, { name: "semextensao", mimeType: "text/plain", buffer: Buffer.from("x") }, { name: "foto-1.png", mimeType: "image/png", buffer: Buffer.from("x") }]);
  await settle(page);
  assert.equal(await page.evaluate(() => window.__status()), "programa.exe não é de um tipo aceito pelo Ponte e ficou de fora. semextensao não é de um tipo aceito pelo Ponte e ficou de fora.");
  await page.evaluate(() => { window.__select("m3", "m3"); window.__click("Trecho selecionado"); });
  await settle(page);
  assert.deepEqual(await page.evaluate(() => [...window.__shadow.querySelectorAll("ul.files li")].map(li => li.textContent.replace(/\s+/g, " ").trim().split(" · ")[0])), ["foto-1.png", "foto-1 (2).png"]);
  await page.evaluate(() => window.__click("Enviar ao Ponte"));
  await settle(page);
  assert.deepEqual((await page.evaluate(() => window.__imports()[0].attachments)).map(item => item.name), ["foto-1.png", "foto-1 (2).png"]);
  await page.close();
  const visible = await panelPage();
  await visible.evaluate(() => { window.__click("Guardar no Ponte"); window.__click("Mensagens visíveis na tela"); });
  await settle(visible);
  assert.deepEqual(await visible.evaluate(() => [...window.__shadow.querySelectorAll("ul.notes li")].map(li => li.textContent)), ["1 foto em formato não aceito ficou de fora.", "1 mensagem de outro tipo ficou de fora.", "1 foto entrou sem data.", "Fotos e arquivos sem legenda entram sem autor."]);
  await visible.close();
});
test("panel: already in the Ponte with files says they were not added again; a refused file does not lock the person in", options, async () => {
  const page = await panelPage(fixture, { imports: [{ protocol_version: 1, ok: true, outcome: "already", document_id: 7, document_number: "MANUAL-2026/0007", attachments_added: 0, ui_opened: true }, { protocol_version: 1, ok: false, error: { code: "invalid_attachment", message: "O arquivo ata.pdf não pôde ser guardado: está danificado." } }, { protocol_version: 1, ok: true, outcome: "imported", document_id: 8, document_number: "MANUAL-2026/0008", attachments_added: 0, ui_opened: true }] });
  await page.evaluate(() => window.__click("Guardar no Ponte"));
  await settle(page);
  const input = await page.evaluateHandle(() => window.__shadow.querySelector("input[type=file]"));
  await input.asElement().setInputFiles({ name: "ata.pdf", mimeType: "application/pdf", buffer: Buffer.from("conteudo") });
  await settle(page);
  await page.evaluate(() => { window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi"; window.__click("Usar o texto colado"); window.__click("Enviar ao Ponte"); });
  await settle(page);
  assert.equal(await page.evaluate(() => window.__status()), "Esta conversa já estava no Ponte. Os arquivos não foram juntados de novo; para juntá-los, abra o registro no Ponte e use Anexos.");
  await page.evaluate(() => { window.__click("Guardar outra conversa"); window.__shadow.querySelector("textarea").value = "[14:02, 10/11/2026] Servidor A: Oi"; window.__click("Usar o texto colado"); });
  await input.asElement().setInputFiles({ name: "ata.pdf", mimeType: "application/pdf", buffer: Buffer.from("conteudo") });
  await settle(page);
  await page.evaluate(() => window.__click("Enviar ao Ponte"));
  await settle(page);
  assert.equal(await page.evaluate(() => window.__status()), "O arquivo ata.pdf não pôde ser guardado: está danificado.");
  const state = await page.evaluate(() => ({ retry: Boolean(window.__btn("Tentar de novo")), send: Boolean(window.__btn("Enviar ao Ponte")) && !window.__btn("Enviar ao Ponte").disabled, title: window.__shadow.querySelector("input[type=text]").disabled, remove: [...window.__shadow.querySelectorAll("ul.files button")].every(button => !button.disabled), pick: !window.__btn("Escolher arquivo…").disabled }));
  assert.deepEqual(state, { retry: false, send: true, title: false, remove: true, pick: true });
  await page.evaluate(() => window.__shadow.querySelector("ul.files button").click());
  await page.evaluate(() => window.__click("Enviar ao Ponte"));
  await settle(page);
  const imports = await page.evaluate(() => window.__imports());
  assert.equal(imports.length, 3);
  assert.equal(imports[1].attachments.length, 1);
  assert.deepEqual(imports[2].attachments, []);
  assert.deepEqual(imports[2].record, imports[1].record);
  assert.equal(await page.evaluate(() => window.__status()), "Guardado no Ponte.");
  await page.close();
});
