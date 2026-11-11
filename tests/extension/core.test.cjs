"use strict";
const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const vm = require("node:vm");
const Core = require("../../extension/capture-core.js");
const UUID = "51ab5b2e-9e1b-40ee-9e60-1de2aaacaa72";
function capture(changes = {}) {
  return { schema_version: 1, request_id: UUID, source_url: Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=ORG-PRO-2026%2F01234", document_number: "ORG-PRO-2026/01234", title: "Aquisição de material", captured_at: "2026-11-01T12:00:00.000Z", text: "Texto de origem.", metadata: {}, references: [], coverage: { adapter: Core.ADAPTER, recognized: ["Campo explícito"], pending: ["DOM autenticado não validado"] }, ...changes };
}
function receipt(requestId = UUID) { return { protocol_version: 1, ok: true, request_id: requestId, document_id: 1, capture_id: 2, duplicate: false, message: "Salvo", ui_opened: false }; }
test("manifest uses fixed public key/ID, restricted isolated MV3, no external messages", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "../../extension/manifest.json"), "utf8"));
  const stableKey = fs.readFileSync(path.join(__dirname, "../../extension-key.txt"), "utf8").trim();
  assert.equal(manifest.key, stableKey);
  const digest = crypto.createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest().subarray(0, 16).toString("hex");
  assert.equal(digest.replace(/[0-9a-f]/g, char => String.fromCharCode(97 + parseInt(char, 16))), "ecllnakmppjinmednoikimbpnbgbimhg");
  assert.equal(manifest.manifest_version, 3);
  assert.deepEqual([...manifest.permissions].sort(), ["nativeMessaging", "scripting", "storage"]);
  assert.deepEqual(manifest.host_permissions, [Core.ORIGIN + "/*"]);
  assert.equal(manifest.content_scripts[0].world, "ISOLATED");
  assert.equal(manifest.content_scripts[0].all_frames, false);
  assert.equal(manifest.externally_connectable, undefined);
});
test("URL sanitizer keeps only official document-number parameters and exact HTTPS host", () => {
  const url = Core.cleanUrl(Core.ORIGIN + "/siga;jsessionid=private/doc?token=secret&PHPSESSID=secret&sigla=ORG-PRO-2026%2F01234&documento=opaque-session&code=auth#secret");
  assert.equal(url, Core.ORIGIN + "/siga/doc?sigla=ORG-PRO-2026%2F01234");
  for (const url of ["http://www.sigadoc.mt.gov.br/", "https://sigadoc.mt.gov.br/", "https://www.sigadoc.mt.gov.br.attacker.test/", "https://user:password@www.sigadoc.mt.gov.br/", "javascript:alert(1)", "https://www.sigadoc.mt.gov.br:444/"]) assert.equal(Core.allowedOrigin(url), false);
});
test("capture validation rejects invented identity, unknown keys, secrets in URLs and invalid calendar dates", () => {
  assert.equal(Core.validateCapture(capture()).request_id, UUID);
  for (const changes of [{ document_number: "Sem número 2026" }, { document_number: "ORG-PRO-2026/99999" }, { title: "" }, { request_id: "random" }, { source_url: Core.ORIGIN + "/?token=secret" }, { metadata: { password: "secret" } }, { captured_at: "2026-02-31T12:00:00Z" }, { extra: true }]) assert.throws(() => Core.validateCapture(capture(changes)), Core.CaptureError);
  assert.equal(Core.documentNumber("Documento nº ORG-PRO-2026/01234"), "ORG-PRO-2026/01234");
  assert.equal(Core.documentNumber("12345/2026"), "12345/2026");
  assert.equal(Core.documentNumber("Resultado de pesquisa 2026"), null);
});
test("SIGA via sigla preserves source identity and matches its canonical base only on exibir", () => {
  const source = Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=ORG-PRO-2026%2F01234-A";
  const via = capture({ source_url: source });
  assert.equal(Core.cleanUrl(source + "&linkVolta=history.back()&token=secret"), source);
  assert.equal(Core.validateCapture(via).document_number, "ORG-PRO-2026/01234");
  assert.equal(Core.identity(via), Core.identity(capture()));
  assert.equal(Core.documentNumber("ORG-PRO-2026/01234-A"), null);
  for (const source_url of [
    Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=ORG-PRO-2026%2F99999-A",
    Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=ORG-PRO-2026%2F01234-AB",
    Core.ORIGIN + "/outra-rota?sigla=ORG-PRO-2026%2F01234-A"
  ]) assert.throws(() => Core.validateCapture(capture({ source_url })), Core.CaptureError);
});
test("UTF-8 text and full native envelope limits reject without truncation", () => {
  const allowed = capture({ text: "é".repeat(Core.MAX_TEXT_BYTES / 2) });
  assert.equal(Core.validateCapture(allowed).text.length, Core.MAX_TEXT_BYTES / 2);
  assert.throws(() => Core.validateCapture(capture({ text: allowed.text + "é" })), error => error.code === "TEXT_TOO_LARGE");
  const references = Array.from({ length: 400 }, (_, i) => ({ label: "A".repeat(200), kind: "attachment", url: Core.ORIGIN + "/" + "x".repeat(1600) + i }));
  assert.throws(() => Core.validateCapture(capture({ text: allowed.text, references })), error => error.code === "PAYLOAD_TOO_LARGE");
  assert.equal(allowed.text.length, Core.MAX_TEXT_BYTES / 2);
});
test("receipt confirmation requires committed host receipt for the captured request", () => {
  assert.equal(Core.validateReceipt(receipt(), UUID).ok, true);
  assert.throws(() => Core.validateReceipt(receipt(crypto.randomUUID()), UUID), error => error.code === "HOST_SCHEMA");
  assert.throws(() => Core.validateReceipt({ protocol_version: 1, ok: true }, UUID), error => error.code === "HOST_SCHEMA");
  // Outcome: optional (older hosts), but only the three known values.
  const unchanged = { ...receipt(), duplicate: true, outcome: "unchanged", message: "Importação ignorada: nada foi gravado." };
  assert.equal(Core.validateReceipt(unchanged, UUID).outcome, "unchanged");
  assert.equal(Core.importOutcome(unchanged), "unchanged");
  assert.equal(Core.importOutcome(receipt()), "imported");
  assert.equal(Core.importOutcome({ ...receipt(), duplicate: true }), "legacy_duplicate");
  for (const outcome of ["x", 1, null]) assert.throws(() => Core.validateReceipt({ ...receipt(), outcome }, UUID), error => error.code === "HOST_SCHEMA");
  assert.equal(Core.nativeError("Specified native messaging host not found.").error.code, "HOST_MISSING");
  assert.equal(Core.nativeError("Access to the specified native messaging host is forbidden.").error.code, "HOST_FORBIDDEN");
  // Each failure the browser reports gets its own advice; an unknown one keeps the browser's words.
  const exited = Core.nativeError("Native host has exited.").error;
  assert.equal(exited.code, "HOST_EXITED");
  assert.match(exited.message, /versão mais recente/);
  assert.equal(Core.nativeError("Failed to start native messaging host.").error.code, "HOST_START");
  assert.equal(Core.nativeError("Error when communicating with the native messaging host.").error.code, "HOST_PROTOCOL");
  const unknown = Core.nativeError("Something new happened.").error;
  assert.equal(unknown.code, "HOST_CONNECTION");
  assert.match(unknown.message, /Something new happened/);
  assert.equal(Core.nativeError("").error.message, "Não recebemos a confirmação do Ponte. Clique em Repetir: nada será duplicado.");
  assert.equal(Core.nativeError(undefined).error.code, "HOST_CONNECTION");
  for (const text of ["Native host has exited.", "x".repeat(500)]) assert.ok(Core.nativeError(text).error.message.length <= 2000);
});
test("lost acknowledgment retries immutable original snapshot with same UUID; double clicks coalesce", async () => {
  const sent = [];
  let resolve;
  const session = new Core.ImportSession(value => { sent.push(value); return new Promise(done => { resolve = done; }); });
  const initial = capture();
  const first = session.run(initial);
  assert.equal(session.state, "sending");
  assert.equal(session.run(capture()), first);
  initial.text = "Mutated after click";
  resolve(Core.nativeError("Native host has exited."));
  const lost = await first;
  assert.equal(lost.result.ok, false);
  assert.equal(sent[0].text, "Texto de origem.");
  assert.ok(Object.isFrozen(sent[0]));
  const retry = session.run(capture({ request_id: crypto.randomUUID(), text: "New page text" }));
  assert.equal(sent[1].request_id, UUID);
  assert.equal(sent[1].text, "Texto de origem.");
  resolve(receipt());
  assert.equal((await retry).result.ok, true);
  assert.equal(session.state, "success");
});
test("new document receives a distinct request and mismatched host ack never succeeds", async () => {
  const sent = [];
  const session = new Core.ImportSession(async value => { sent.push(value); return receipt("wrong-request"); });
  assert.equal((await session.run(capture())).result.error.code, "HOST_SCHEMA");
  const nextId = crypto.randomUUID();
  await session.run(capture({ request_id: nextId, document_number: "ORG-PRO-2026/01235", source_url: Core.ORIGIN + "/doc?sigla=ORG-PRO-2026%2F01235" }));
  assert.equal(sent[1].request_id, nextId);
});
function workerFixture(filesApi, options = {}) {
  let listener, commandListener;
  const calls = [], tabMessages = [];
  const store = options.store || {};
  const defaultReply = request => {
    if (request.action === "ping") return { protocol_version: 1, ok: true, app_version: "0.29.0", actions: ["lookup", "open_document"] };
    if (request.action === "lookup") return { protocol_version: 1, ok: true, found: false };
    if (request.action === "open_document") return { protocol_version: 1, ok: true, document_id: request.document_id, ui_opened: true };
    return receipt(request.capture.request_id);
  };
  const chrome = {
    runtime: {
      id: "ecllnakmppjinmednoikimbpnbgbimhg", lastError: null,
      getURL: name => "chrome-extension://ecllnakmppjinmednoikimbpnbgbimhg/" + name,
      onMessage: { addListener: value => { listener = value; } },
      sendNativeMessage: (host, request, callback) => {
        calls.push({ host, request });
        const custom = options.native ? options.native(request, calls.length) : undefined;
        if (custom && custom.nativeError) { chrome.runtime.lastError = { message: custom.nativeError }; callback(undefined); chrome.runtime.lastError = null; return; }
        callback(custom === undefined ? defaultReply(request) : custom);
      }
    },
    // chrome.storage.session stand-in: memory only, shared between workers through options.store.
    storage: { session: {
      get: async key => { if (options.storageThrows) throw new Error("storage unavailable"); return key in store ? { [key]: JSON.parse(JSON.stringify(store[key])) } : {}; },
      set: async value => { if (options.storageThrows) throw new Error("QUOTA_BYTES quota exceeded"); Object.assign(store, JSON.parse(JSON.stringify(value))); }
    } },
    commands: { onCommand: { addListener: value => { commandListener = value; } } },
    tabs: {
      query: (_, callback) => callback(options.tabs || []),
      sendMessage: (id, message, opts, callback) => { tabMessages.push({ id, message, opts }); if (callback) callback(); }
    }
  };
  const context = { chrome, importScripts: () => {}, PonteCapture: Core, PonteFiles: filesApi, PonteWhatsApp: require("../../extension/whatsapp-core.js"), setTimeout, clearTimeout, JSON, Promise };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../../extension/background.js"), "utf8"), context);
  const sender = { id: chrome.runtime.id, frameId: 0, url: Core.ORIGIN + "/doc", tab: { id: 7, url: Core.ORIGIN + "/doc" } };
  const invoke = (message, who = sender) => new Promise(resolve => listener(message, who, resolve));
  const command = name => commandListener(name);
  return { invoke, calls, sender, chrome, store, tabMessages, command };
}
test("worker checks sender origin/frame/action/schema before every native call", async () => {
  const worker = workerFixture();
  const message = { action: "ponte.import_capture", capture: capture() };
  assert.equal((await worker.invoke(message, { ...worker.sender, url: "https://attacker.test/" })).error.code, "SENDER_DENIED");
  assert.equal((await worker.invoke(message, { ...worker.sender, frameId: 1 })).error.code, "SENDER_DENIED");
  assert.equal((await worker.invoke(message, { ...worker.sender, tab: { id: 7, url: "https://attacker.test/" } })).error.code, "SENDER_DENIED");
  assert.equal((await worker.invoke({ action: "open_url", url: Core.ORIGIN })).error.code, "ACTION_DENIED");
  assert.equal((await worker.invoke({ action: "ponte.import_capture", capture: capture({ source_url: Core.ORIGIN + "/?access_token=secret" }) })).error.code, "SOURCE_URL");
  assert.equal(worker.calls.length, 0);
  assert.equal((await worker.invoke(message)).ok, true);
  assert.equal(worker.calls.length, 1);
  assert.equal(worker.calls[0].host, "com.ponteproj.assistente");
  assert.equal(worker.calls[0].request.action, "import_capture");
  assert.equal(worker.calls[0].request.protocol_version, 1);
});
test("popup can ping but cannot import or issue arbitrary native commands", async () => {
  const worker = workerFixture();
  const popup = { id: worker.chrome.runtime.id, url: worker.chrome.runtime.getURL("popup.html") };
  assert.equal((await worker.invoke({ action: "ponte.ping" }, popup)).ok, true);
  assert.equal((await worker.invoke({ action: "ponte.import_capture", capture: capture() }, popup)).error.code, "SENDER_DENIED");
  assert.equal((await worker.invoke({ action: "ponte.ping", native_action: "open_app" }, popup)).error.code, "SENDER_DENIED");
  assert.equal(worker.calls.length, 1);
});

test("worker keeps downloaded bundle immutable for retry and rejects changed request", async () => {
  let downloads = 0;
  const worker = workerFixture({prepare: async value => { downloads++; return {protocol_version:1,action:'import_bundle',capture:value,files:[]}; }});
  const value = capture({references:[{url:Core.ORIGIN+'/test.pdf',label:'PDF',kind:'original'}]});
  const message = {action:'ponte.import_capture',capture:value};
  assert.equal((await worker.invoke(message)).ok,true);
  assert.equal((await worker.invoke(message)).ok,true);
  assert.equal(downloads,1);
  assert.equal(worker.calls[0].request.action,'import_bundle');
  const changed = {action:message.action,capture:{...value,text:'Alterado'}};
  assert.equal((await worker.invoke(changed)).error.code,'REQUEST_CHANGED');
});

test("history metadata cannot nominate a different document or destination",()=>{
  const value=capture({metadata:{movement_source:Core.ORIGIN+"/sigaex/app/expediente/doc/exibirAntigo?sigla=OUTRO202601234"}});
  assert.throws(()=>Core.validateCapture(value),/eventos divergente/);
  value.metadata.movement_source=Core.ORIGIN+"/sigaex/app/expediente/doc/exibirAntigo?sigla=ORGPRO202601234";
  assert.doesNotThrow(()=>Core.validateCapture(value));
});

const plainOf = value => JSON.parse(JSON.stringify(value));
const NUMBER = "ORG-OFI-2026/00431";
test("manifest 0.4.0: nativeMessaging, storage and scripting, same key and hosts, WhatsApp Web only as an optional host, one shortcut command", () => {
  const read = name => JSON.parse(fs.readFileSync(path.join(__dirname, "../../extension/" + name), "utf8"));
  const manifest = read("manifest.json");
  assert.equal(manifest.version, "0.4.0");
  assert.equal(read("package.json").version, "0.4.0");
  assert.equal(manifest.key, fs.readFileSync(path.join(__dirname, "../../extension-key.txt"), "utf8").trim());
  assert.deepEqual([...manifest.permissions].sort(), ["nativeMessaging", "scripting", "storage"]);
  assert.deepEqual(manifest.host_permissions, [Core.ORIGIN + "/*"]);
  assert.deepEqual(manifest.optional_host_permissions, ["https://web.whatsapp.com/*"]);
  assert.equal(manifest.content_scripts.length, 1);
  assert.deepEqual(manifest.content_scripts[0].js, ["capture-core.js", "history.js", "content.js"]);
  assert.deepEqual(manifest.content_scripts[0].matches, [Core.ORIGIN + "/*"]);
  assert.equal(manifest.commands.importar.suggested_key.default, "Alt+Shift+I");
  assert.equal(manifest.commands.importar.description, "Importar o documento aberto para o Ponte");
  assert.deepEqual(Object.keys(manifest.commands), ["importar"]);
});
test("source never touches persistent storage, eval, innerHTML or logging", () => {
  for (const name of ["background.js", "content.js", "files.js", "history.js", "popup.js", "capture-core.js", "whatsapp-core.js", "whatsapp.js"]) {
    const source = fs.readFileSync(path.join(__dirname, "../../extension/" + name), "utf8");
    assert.ok(!/storage\.(local|sync)|\beval\(|new Function|innerHTML|console\./.test(source), name);
  }
});
test("lookup is accepted only from the content script of the SIGADOC top frame; open also from the popup", async () => {
  const worker = workerFixture();
  const lookup = { action: "ponte.lookup", document_number: NUMBER };
  const popup = { id: worker.chrome.runtime.id, url: worker.chrome.runtime.getURL("popup.html") };
  const strangers = [{ ...worker.sender, url: "https://attacker.test/" }, { ...worker.sender, frameId: 1 }, { ...worker.sender, tab: { id: 7, url: "https://attacker.test/" } }, { ...worker.sender, id: "other-extension" }, { id: worker.chrome.runtime.id, url: worker.chrome.runtime.getURL("other.html") }, { id: worker.chrome.runtime.id, url: worker.chrome.runtime.getURL("popup.html"), tab: { id: 3, url: "https://example.org/" } }];
  for (const who of [popup, ...strangers]) assert.equal((await worker.invoke(lookup, who)).error.code, "SENDER_DENIED");
  for (const who of strangers) assert.equal((await worker.invoke({ action: "ponte.open", document_id: 5 }, who)).error.code, "SENDER_DENIED");
  // Extra fields are refused, not forwarded.
  assert.equal((await worker.invoke({ ...lookup, native_action: "open_app" })).error.code, "SENDER_DENIED");
  assert.equal((await worker.invoke({ action: "ponte.open", document_id: 5, extra: 1 })).error.code, "SENDER_DENIED");
  assert.equal(worker.calls.length, 0);
});
test("lookup builds the native message from scratch and returns only checked fields", async () => {
  const worker = workerFixture(undefined, { native: request => request.action === "lookup" ? { protocol_version: 1, ok: true, found: true, document_id: 123, imported_at: "10/11/2026 14:30", secret: "x", path: "C:\\dados" } : undefined });
  const answer = await worker.invoke({ action: "ponte.lookup", document_number: NUMBER });
  assert.deepEqual(plainOf(answer), { protocol_version: 1, ok: true, found: true, document_id: 123, imported_at: "10/11/2026 14:30" });
  assert.deepEqual(plainOf(worker.calls.map(call => call.request)), [{ protocol_version: 1, action: "lookup", document_number: NUMBER }]);
  assert.equal(worker.calls[0].host, "com.ponteproj.assistente");
  assert.deepEqual(plainOf(await workerFixture().invoke({ action: "ponte.lookup", document_number: NUMBER })), { protocol_version: 1, ok: true, found: false });
});
test("lookup and open refuse bad inputs before calling the native host", async () => {
  const worker = workerFixture();
  for (const document_number of ["", "x".repeat(201), 5, null, undefined, { a: 1 }, "A\u0000B"]) assert.equal((await worker.invoke({ action: "ponte.lookup", document_number })).error.code, "INVALID_REQUEST", String(document_number));
  for (const document_id of [0, -1, 1.5, "7", null, 2 ** 53, Infinity, NaN]) assert.equal((await worker.invoke({ action: "ponte.open", document_id })).error.code, "INVALID_REQUEST", String(document_id));
  assert.equal(worker.calls.length, 0);
  assert.equal((await worker.invoke({ action: "ponte.lookup", document_number: "x".repeat(200) })).ok, true);
  assert.deepEqual(plainOf(await worker.invoke({ action: "ponte.open", document_id: 123 })), { protocol_version: 1, ok: true, document_id: 123, ui_opened: true });
  assert.deepEqual(plainOf(worker.calls.map(call => call.request).slice(1)), [{ protocol_version: 1, action: "open_document", document_id: 123 }]);
});
test("answers of an old Ponte and of unknown documents are passed on in words; malformed ones are refused", async () => {
  const refusal = (code, message) => ({ protocol_version: 1, ok: false, error: { code, message } });
  let reply;
  const worker = workerFixture(undefined, { native: () => reply });
  reply = refusal("unsupported_action", "Ação desconhecida.");
  assert.deepEqual((await worker.invoke({ action: "ponte.lookup", document_number: NUMBER })).error, { code: "unsupported_action", message: "Ação desconhecida." });
  reply = refusal("unknown_document", "Este documento não está mais no acervo.");
  assert.deepEqual((await worker.invoke({ action: "ponte.open", document_id: 9 })).error, { code: "unknown_document", message: "Este documento não está mais no acervo." });
  for (const bad of [{ protocol_version: 1, ok: true }, { protocol_version: 1, ok: true, found: true }, { protocol_version: 1, ok: true, found: true, document_id: 0 }, { protocol_version: 2, ok: true, found: false }, null]) {
    reply = bad === null ? { ok: "yes" } : bad;
    assert.equal((await worker.invoke({ action: "ponte.lookup", document_number: NUMBER })).error.code, "HOST_SCHEMA");
  }
  reply = { protocol_version: 1, ok: true, document_id: "7" };
  assert.equal((await worker.invoke({ action: "ponte.open", document_id: 7 })).error.code, "HOST_SCHEMA");
  const unreachable = workerFixture(undefined, { native: () => ({ nativeError: "Specified native messaging host not found." }) });
  assert.equal((await unreachable.invoke({ action: "ponte.lookup", document_number: NUMBER })).error.code, "HOST_MISSING");
});
test("ping reports the actions of the Ponte; an old Ponte reports none", async () => {
  const popupOf = worker => ({ id: worker.chrome.runtime.id, url: worker.chrome.runtime.getURL("popup.html") });
  const current = workerFixture();
  assert.deepEqual(plainOf(await current.invoke({ action: "ponte.ping" }, popupOf(current))), { protocol_version: 1, ok: true, app_version: "0.29.0", actions: ["lookup", "open_document"] });
  const old = workerFixture(undefined, { native: () => ({ protocol_version: 1, ok: true, app_version: "0.28.0" }) });
  assert.deepEqual(plainOf(await old.invoke({ action: "ponte.ping" }, popupOf(old))), { protocol_version: 1, ok: true, app_version: "0.28.0", actions: [] });
  assert.deepEqual(plainOf(old.calls[0].request), { protocol_version: 1, action: "ping" });
});
test("status texts for each case are pure functions of the answers", () => {
  const ok = { protocol_version: 1, ok: true, actions: ["lookup", "open_document"] };
  const found = Core.checkOutcome(ok, { ok: true, found: true, document_id: 123, imported_at: "10/11/2026 14:30" });
  assert.deepEqual(Core.statusView(found), { text: "Já está no Ponte — importado em 10/11/2026 14:30.", tone: "success", retry: false, open: true, importLabel: "Importar de novo" });
  assert.equal(Core.statusView(Core.checkOutcome({ ...ok, actions: ["lookup"] }, { ok: true, found: true, document_id: 1, imported_at: "" })).open, false);
  assert.equal(Core.statusView(Core.checkOutcome({ ...ok, actions: ["lookup"] }, { ok: true, found: true, document_id: 1, imported_at: "" })).text, "Já está no Ponte.");
  const missing = Core.statusView(Core.checkOutcome(ok, { ok: true, found: false }));
  assert.deepEqual([missing.text, missing.open, missing.importLabel], ["", false, "Importar para o assistente"]);
  const oldView = Core.statusView(Core.checkOutcome({ ok: true }, null));
  assert.deepEqual([oldView.text, oldView.open, oldView.retry, oldView.importLabel], ["Atualize o Ponte para ver se o documento já foi importado.", false, false, "Importar para o assistente"]);
  assert.equal(Core.checkOutcome({ ok: true, actions: [] }, null).kind, "old");
  assert.equal(Core.checkOutcome(ok, { ok: false, error: { code: "invalid_request", message: "x" } }).kind, "old");
  assert.equal(Core.checkOutcome(ok, { ok: false, error: { code: "unsupported_action", message: "x" } }).kind, "old");
  const missingHost = Core.statusView(Core.checkOutcome(Core.nativeError("Specified native messaging host not found."), null));
  assert.deepEqual([missingHost.tone, missingHost.retry, missingHost.open], ["warning", true, false]);
  assert.match(missingHost.text, /^O Ponte não foi encontrado neste computador\. Abra o Ponte uma vez/);
  // The generic "click Repetir" text makes no sense before any click.
  const lost = Core.statusView(Core.checkOutcome(Core.nativeError(""), null));
  assert.equal(lost.text, "O Ponte não respondeu. Abra o Ponte e tente de novo.");
  assert.equal(Core.statusView(Core.checkOutcome(null, null)).retry, true);
  assert.equal(Core.checkOutcome(ok, { ok: false, error: { code: "library_locked", message: "O acervo está fechado." } }).message, "O acervo está fechado.");
  assert.equal(Core.statusView(null).text, "");
  assert.equal(Core.statusView(Core.checkOutcome(ok, { ok: true })).text, "");
});
test("plural of files not downloaded is written out", () => {
  assert.equal(Core.filesNotDownloaded(1), "1 arquivo não baixado");
  assert.equal(Core.filesNotDownloaded(2), "2 arquivos não baixados");
  assert.equal(Core.filesNotDownloaded(10), "10 arquivos não baixados");
  assert.ok(!/\(s\)/.test(fs.readFileSync(path.join(__dirname, "../../extension/content.js"), "utf8")));
});
test("user-visible texts say importação, not captura", () => {
  const failures = [{ ...capture(), schema_version: 2 }, { ...capture(), request_id: "x" }, { ...capture(), captured_at: "x" }].map(value => { try { Core.validateCapture(value); } catch (error) { return error.message; } });
  for (const message of failures) assert.ok(message && !/captura/i.test(message), message);
  const text = fs.readFileSync(path.join(__dirname, "../../extension/popup.html"), "utf8") + fs.readFileSync(path.join(__dirname, "../../extension/popup.js"), "utf8");
  assert.ok(!/captura|capturar/i.test(text));
});
function bundleMessage(id = UUID) {
  return { action: "ponte.import_capture", capture: capture({ request_id: id, references: [{ url: Core.ORIGIN + "/test.pdf", label: "PDF", kind: "original" }] }) };
}
function countingFiles(extra = "") {
  const state = { downloads: 0 };
  state.api = { prepare: async value => { state.downloads++; return { protocol_version: 1, action: "import_bundle", capture: value, files: [{ url: Core.ORIGIN + "/test.pdf", data_base64: "JVBERi0=" + extra }] }; } };
  return state;
}
test("retry data survives a stopped service worker through session storage only", async () => {
  const store = {};
  const first = countingFiles();
  const worker = workerFixture(first.api, { store, native: () => ({ nativeError: "Native host has exited." }) });
  assert.equal((await worker.invoke(bundleMessage())).ok, false);
  assert.equal(first.downloads, 1);
  assert.equal(store["ponte.bundles"].length, 1);
  // A new worker (the old one was stopped) retries without downloading again, and delivers.
  const second = countingFiles();
  const restarted = workerFixture(second.api, { store });
  const result = await restarted.invoke(bundleMessage());
  assert.equal(result.ok, true);
  assert.equal(second.downloads, 0);
  assert.equal(restarted.calls[0].request.action, "import_bundle");
  assert.equal(restarted.calls[0].request.files[0].data_base64, "JVBERi0=");
  // Delivered: nothing is kept any more, and a changed page is still refused.
  assert.deepEqual(store["ponte.bundles"], []);
  const changed = bundleMessage();
  changed.capture.text = "Alterado";
  assert.equal((await workerFixture(countingFiles().api, { store: { "ponte.bundles": [{ key: UUID, snapshot: JSON.stringify(bundleMessage().capture), packet: { capture: {}, files: [] } }] } }).invoke(changed)).error.code, "REQUEST_CHANGED");
});
test("only the last two bundles are kept in session storage", async () => {
  const store = {};
  const worker = workerFixture(countingFiles().api, { store, native: () => ({ nativeError: "Native host has exited." }) });
  const ids = [UUID, crypto.randomUUID(), crypto.randomUUID()];
  for (const id of ids) await worker.invoke(bundleMessage(id));
  assert.deepEqual(store["ponte.bundles"].map(item => item.key), ids.slice(1));
});
test("a bundle too big for session storage is not stored and the retry downloads again", async () => {
  const store = {};
  const first = countingFiles("A".repeat(9 * 1024 * 1024));
  const worker = workerFixture(first.api, { store, native: () => ({ nativeError: "Native host has exited." }) });
  assert.equal((await worker.invoke(bundleMessage())).ok, false);
  assert.equal(store["ponte.bundles"], undefined);
  // Same worker: the in-memory fast path still avoids a second download.
  assert.equal((await worker.invoke(bundleMessage())).ok, false);
  assert.equal(first.downloads, 1);
  const second = countingFiles();
  assert.equal((await workerFixture(second.api, { store }).invoke(bundleMessage())).ok, true);
  assert.equal(second.downloads, 1);
});
test("session storage that fails never breaks the import", async () => {
  const state = countingFiles();
  const worker = workerFixture(state.api, { storageThrows: true });
  assert.equal((await worker.invoke(bundleMessage())).ok, true);
  assert.equal((await worker.invoke(bundleMessage())).ok, true);
  assert.equal(state.downloads, 1);
  assert.equal((await workerFixture(countingFiles().api, { storageThrows: true }).invoke(bundleMessage())).ok, true);
});
test("the keyboard command reaches only an active SIGADOC tab", async () => {
  const sigadoc = [{ id: 5, url: Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=x" }];
  const worker = workerFixture(undefined, { tabs: sigadoc });
  worker.command("importar");
  assert.deepEqual(plainOf(worker.tabMessages), [{ id: 5, message: { action: "ponte.shortcut_import" }, opts: { frameId: 0 } }]);
  worker.command("outro");
  assert.equal(worker.tabMessages.length, 1);
  for (const tabs of [[{ id: 6, url: "https://example.org/" }], [{ id: 6, url: "https://www.sigadoc.mt.gov.br.attacker.test/" }], [{ id: 6 }], [], [{ url: Core.ORIGIN + "/" }]]) {
    const other = workerFixture(undefined, { tabs });
    other.command("importar");
    assert.equal(other.tabMessages.length, 0, JSON.stringify(tabs));
  }
});

// ---- 0.3.0: note and priority at import, recent imports ----
const WITH_EXTRAS = ["lookup", "open_document", "import_extras"];
const pingReply = actions => ({ protocol_version: 1, ok: true, app_version: "0.31.0", actions });
const popupOf = worker => ({ id: worker.chrome.runtime.id, url: worker.chrome.runtime.getURL("popup.html") });
const importReply = request => ({ ...receipt(request.capture.request_id), document_id: 41 });
function extrasNative(actions, extra = {}) {
  return request => request.action === "ping" ? pingReply(actions) : { ...importReply(request), ...extra };
}
const importMessage = (extras, id = UUID) => ({ action: "ponte.import_capture", capture: capture({ request_id: id }), ...(extras ? { extras } : {}) });
test("extras: exact shape, trimmed note up to 2000 characters, no control characters, four priorities", () => {
  assert.equal(Core.buildExtras("", ""), null);
  assert.equal(Core.buildExtras("   \n ", ""), null);
  assert.deepEqual(plainOf(Core.buildExtras("  responder até sexta \r\n  e ligar\t", "")), { note: "responder até sexta \n  e ligar" });
  assert.deepEqual(plainOf(Core.buildExtras("", "alta")), { priority: "alta" });
  assert.deepEqual(plainOf(Core.buildExtras("x\u0000y\u0007z", "urgente")), { note: "xyz", priority: "urgente" });
  for (const priority of ["baixa", "normal", "alta", "urgente"]) assert.deepEqual(plainOf(Core.validateExtras({ priority })), { priority });
  assert.doesNotThrow(() => Core.validateExtras({ note: "a".repeat(2000) }));
  assert.doesNotThrow(() => Core.validateExtras({ note: "😀".repeat(2000) }));
  // Unpaired surrogates (a cut emoji from a paste) are repaired when building and refused when validating.
  const cut = "nota " + "😀".slice(0, 1);
  const repaired = Core.buildExtras(cut, "").note;
  assert.ok(!/[\ud800-\udfff]/.test(repaired) && repaired.startsWith("nota"));
  assert.equal(JSON.parse(JSON.stringify(repaired)), repaired);
  assert.deepEqual(plainOf(Core.buildExtras("a😀b", "")), { note: "a😀b" });
  for (const note of ["x\ud83d", "\ude00x", "a\ud83dB"]) assert.throws(() => Core.validateExtras({ note }), error => error.code === "EXTRAS_INVALID");
  assert.equal(Core.limitNote("😀".repeat(2100)).length, 4000);
  assert.equal(Core.limitNote("abc"), "abc");
  for (const bad of [{}, null, [], "x", { note: "" }, { note: " a" }, { note: "a " }, { note: "a".repeat(2001) }, { note: "😀".repeat(2001) }, { note: 5 }, { note: "a\u0000b" }, { note: "a\rb" }, { note: "a\u007fb" }, { note: "a\u0085b" }, { priority: "Alta" }, { priority: "" }, { priority: "critica" }, { priority: 1 }, { note: "a", extra: 1 }, { note: "a", priority: "alta", more: true }, { note: undefined, priority: "alta" }]) {
    assert.throws(() => Core.validateExtras(bad), error => error.code === "EXTRAS_INVALID", JSON.stringify(bad));
  }
  assert.equal(Core.buildExtras("a".repeat(2000), "").note.length, 2000);
});
test("extras notice: the sentence after an import for each case", () => {
  const ok = extra => ({ protocol_version: 1, ok: true, ...extra });
  assert.equal(Core.extrasNotice(ok({ extras_applied: true }), { note: "a", priority: "alta" }), "Nota e prioridade enviadas.");
  assert.equal(Core.extrasNotice(ok({ extras_applied: true }), { note: "a" }), "Nota enviada.");
  assert.equal(Core.extrasNotice(ok({ extras_applied: true }), { priority: "alta" }), "Prioridade enviada.");
  assert.equal(Core.extrasNotice(ok({ extras_applied: false, extras_message: "A nota não coube no documento." }), { note: "a" }), "A nota não coube no documento.");
  assert.equal(Core.extrasNotice(ok({ extras_applied: false }), { note: "a", priority: "alta" }), "A nota e a prioridade não foram aplicadas.");
  assert.equal(Core.extrasNotice(ok({ extras_skipped: true }), { note: "a" }), "O Ponte instalado não recebe nota nem prioridade; a importação foi feita sem elas. Atualize o Ponte.");
  assert.equal(Core.extrasNotice(ok({}), null), "");
  assert.equal(Core.extrasNotice({ protocol_version: 1, ok: false, error: {} }, { note: "a" }), "");
});
test("an import with extras asks the Ponte first, builds the message from scratch and sends extras only to a Ponte that takes them", async () => {
  const worker = workerFixture(undefined, { native: extrasNative(WITH_EXTRAS, { extras_applied: true, secret: "x" }) });
  const result = await worker.invoke(importMessage({ priority: "alta", note: "Responder até sexta" }));
  assert.equal(result.ok, true);
  assert.equal(result.extras_applied, true);
  assert.equal(result.extras_skipped, undefined);
  assert.deepEqual(plainOf(worker.calls.map(call => call.request.action)), ["ping", "import_capture"]);
  assert.deepEqual(plainOf(worker.calls[1].request), { protocol_version: 1, action: "import_capture", capture: capture(), extras: { note: "Responder até sexta", priority: "alta" } });
  assert.deepEqual(Object.keys(worker.calls[1].request.extras), ["note", "priority"]);
  // The answer about the Ponte is kept for a minute: the next import does not ask again.
  await worker.invoke(importMessage({ priority: "baixa" }, crypto.randomUUID()));
  assert.deepEqual(plainOf(worker.calls.map(call => call.request.action)), ["ping", "import_capture", "import_capture"]);
  assert.deepEqual(plainOf(worker.calls[2].request.extras), { priority: "baixa" });
  // Without extras nothing is asked and no field is added.
  const plain = workerFixture(undefined, { native: extrasNative(WITH_EXTRAS) });
  assert.equal((await plain.invoke(importMessage(null))).extras_applied, undefined);
  assert.deepEqual(plainOf(plain.calls.map(call => call.request)), [{ protocol_version: 1, action: "import_capture", capture: capture() }]);
});
test("an older Ponte never receives extras: the import is made without them and the result says so", async () => {
  for (const actions of [["lookup", "open_document"], []]) {
    const worker = workerFixture(undefined, { native: extrasNative(actions) });
    const result = await worker.invoke(importMessage({ note: "Nota", priority: "urgente" }));
    assert.equal(result.ok, true);
    assert.equal(result.extras_skipped, true);
    assert.equal(result.extras_applied, undefined);
    assert.ok(worker.calls.every(call => !("extras" in call.request)));
    assert.equal(worker.calls.at(-1).request.action, "import_capture");
    assert.equal(Core.extrasNotice(result, { note: "Nota" }), "O Ponte instalado não recebe nota nem prioridade; a importação foi feita sem elas. Atualize o Ponte.");
  }
  // The Ponte answers `extras_*` only for what was sent: invented fields do not reach the page.
  const odd = workerFixture(undefined, { native: extrasNative(["lookup"], { extras_applied: true, extras_message: "x" }) });
  const answer = await odd.invoke(importMessage({ note: "n" }));
  assert.equal(answer.extras_applied, undefined);
  assert.equal(answer.extras_message, undefined);
  // A Ponte that cannot be reached for the question gets nothing, and the person is told why.
  const gone = workerFixture(undefined, { native: () => ({ nativeError: "Specified native messaging host not found." }) });
  assert.equal((await gone.invoke(importMessage({ note: "n" }))).error.code, "HOST_MISSING");
  assert.equal(gone.calls.length, 1);
  assert.equal(gone.calls[0].request.action, "ping");
});
test("a refusal or message of the Ponte about extras is passed on cleaned", async () => {
  const worker = workerFixture(undefined, { native: extrasNative(WITH_EXTRAS, { extras_applied: false, extras_message: "  A prioridade não foi aplicada.  " }) });
  const result = await worker.invoke(importMessage({ priority: "alta" }));
  assert.equal(result.extras_applied, false);
  assert.equal(result.extras_message, "A prioridade não foi aplicada.");
  const refusing = workerFixture(undefined, { native: request => request.action === "ping" ? pingReply(WITH_EXTRAS) : { protocol_version: 1, ok: false, error: { code: "invalid_request", message: "A nota é longa demais." } } });
  assert.deepEqual(plainOf((await refusing.invoke(importMessage({ note: "n" }))).error), { code: "invalid_request", message: "A nota é longa demais." });
});
test("extras from the page are validated like every other input, before any native call", async () => {
  const worker = workerFixture(undefined, { native: extrasNative(WITH_EXTRAS) });
  const bad = [{}, { note: "" }, { note: " a " }, { note: "a".repeat(2001) }, { priority: "ALTA" }, { priority: "x" }, { note: "a", other: 1 }, { note: 1 }, null, "alta", ["alta"]];
  for (const extras of bad) assert.equal((await worker.invoke({ action: "ponte.import_capture", capture: capture(), extras })).error.code, "EXTRAS_INVALID", JSON.stringify(extras));
  assert.equal((await worker.invoke({ action: "ponte.import_capture", capture: capture(), extras: { note: "a" }, more: 1 })).error.code, "ACTION_DENIED");
  assert.equal((await worker.invoke({ action: "ponte.import_capture", extras: { note: "a" } })).error.code, "ACTION_DENIED");
  assert.equal((await worker.invoke({ action: "ponte.import_capture", capture: capture(), extras: undefined })).error.code, "EXTRAS_INVALID");
  assert.equal((await worker.invoke(importMessage({ note: "a" }), popupOf(worker))).error.code, "SENDER_DENIED");
  assert.equal((await worker.invoke(importMessage({ note: "a" }), { ...worker.sender, url: "https://attacker.test/" })).error.code, "SENDER_DENIED");
  assert.equal(worker.calls.length, 0);
});
function extrasBundle(id = UUID) {
  const message = bundleMessage(id);
  return { ...message, extras: { note: "Falar com o protocolo", priority: "alta" } };
}
test("the retry sends exactly the same extras, refuses changed ones and keeps them only in session memory until delivered", async () => {
  const store = {};
  const state = countingFiles();
  let failures = 1;
  const worker = workerFixture(state.api, { store, native: request => request.action === "ping" ? pingReply(WITH_EXTRAS) : failures-- > 0 ? { nativeError: "Native host has exited." } : { ...importReply(request), extras_applied: true } });
  assert.equal((await worker.invoke(extrasBundle())).ok, false);
  assert.equal(store["ponte.bundles"].length, 1);
  assert.ok(store["ponte.bundles"][0].snapshot.includes("Falar com o protocolo"));
  // The page changed, or the extras did: refused, nothing sent.
  const sentBefore = worker.calls.length;
  const otherNote = extrasBundle(); otherNote.extras = { note: "Outra nota", priority: "alta" };
  const otherPriority = extrasBundle(); otherPriority.extras = { note: "Falar com o protocolo", priority: "baixa" };
  const noExtras = bundleMessage();
  const noNote = extrasBundle(); noNote.extras = { priority: "alta" };
  for (const message of [otherNote, otherPriority, noExtras, noNote]) assert.equal((await worker.invoke(message)).error.code, "REQUEST_CHANGED");
  const pageChanged = extrasBundle(); pageChanged.capture.text = "Alterado";
  assert.equal((await worker.invoke(pageChanged)).error.code, "REQUEST_CHANGED");
  assert.equal(worker.calls.length, sentBefore);
  // The same request in a restarted worker: same files, same extras, delivered once.
  const second = countingFiles();
  const restarted = workerFixture(second.api, { store, native: extrasNative(WITH_EXTRAS, { extras_applied: true }) });
  const result = await restarted.invoke(extrasBundle());
  assert.equal(result.ok, true);
  assert.equal(result.extras_applied, true);
  assert.equal(second.downloads, 0);
  const sent = restarted.calls.at(-1).request;
  assert.equal(sent.action, "import_bundle");
  assert.deepEqual(plainOf(sent.extras), { note: "Falar com o protocolo", priority: "alta" });
  assert.equal(sent.files[0].data_base64, "JVBERi0=");
  // Delivered: no copy of the note is kept.
  assert.deepEqual(store["ponte.bundles"], []);
  assert.ok(!JSON.stringify(store).includes("Falar com o protocolo"));
});
test("a bundle import sends extras to a Ponte that takes them and leaves them out for one that does not", async () => {
  const state = countingFiles();
  const yes = workerFixture(state.api, { native: extrasNative(WITH_EXTRAS, { extras_applied: true }) });
  await yes.invoke(extrasBundle());
  assert.deepEqual(plainOf(yes.calls.map(call => call.request.action)), ["ping", "import_bundle"]);
  assert.deepEqual(Object.keys(yes.calls[1].request), ["protocol_version", "action", "capture", "files", "extras"]);
  const no = workerFixture(countingFiles().api, { native: extrasNative(["lookup"]) });
  const result = await no.invoke(extrasBundle());
  assert.equal(result.extras_skipped, true);
  assert.ok(!("extras" in no.calls.at(-1).request));
  assert.equal(no.calls.at(-1).request.action, "import_bundle");
});
test("recent imports: newest first, at most 8, small fields only, cleared on request", async () => {
  const store = {};
  const worker = workerFixture(undefined, { store, native: extrasNative(WITH_EXTRAS, { extras_applied: true }) });
  const popup = popupOf(worker);
  assert.deepEqual(plainOf(await worker.invoke({ action: "ponte.recent" }, popup)), { protocol_version: 1, ok: true, items: [] });
  for (let i = 1; i <= 10; i++) {
    const number = `ORG-PRO-2026/${String(i).padStart(5, "0")}`;
    const message = { action: "ponte.import_capture", capture: capture({ request_id: crypto.randomUUID(), document_number: number, source_url: Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=" + encodeURIComponent(number), title: i === 10 ? "T".repeat(300) : `Assunto ${i}`, text: "TEXTO SECRETO DO DOCUMENTO" }), extras: { note: "NOTA PRIVADA " + i } };
    assert.equal((await worker.invoke(message)).ok, true);
  }
  const answer = plainOf(await worker.invoke({ action: "ponte.recent" }, popup));
  assert.equal(answer.items.length, 8);
  assert.deepEqual(answer.items.map(item => item.number), [10, 9, 8, 7, 6, 5, 4, 3].map(i => `ORG-PRO-2026/${String(i).padStart(5, "0")}`));
  assert.deepEqual(Object.keys(answer.items[1]).sort(), ["document_id", "number", "outcome", "time", "title"]);
  assert.deepEqual([answer.items[1].title, answer.items[1].outcome, answer.items[1].document_id], ["Assunto 9", "imported", 41]);
  assert.ok(!Number.isNaN(Date.parse(answer.items[1].time)));
  assert.equal(answer.items[0].title.length, 120);
  const stored = JSON.stringify(store["ponte.recent"]);
  assert.ok(!stored.includes("TEXTO SECRETO") && !stored.includes("NOTA PRIVADA"));
  assert.equal(store["ponte.recent"].length, 8);
  assert.deepEqual(plainOf(await worker.invoke({ action: "ponte.recent_clear" }, popup)), { protocol_version: 1, ok: true });
  assert.deepEqual(plainOf((await worker.invoke({ action: "ponte.recent" }, popup)).items), []);
});
test("recent imports record each result: imported, already in the Ponte, not completed", async () => {
  let reply;
  const worker = workerFixture(undefined, { native: request => ({ ...reply, ...(reply.ok && reply.request_id ? { request_id: request.capture.request_id } : {}) }) });
  const popup = popupOf(worker);
  const run = async answer => { reply = answer;return worker.invoke({ action: "ponte.import_capture", capture: capture({ request_id: crypto.randomUUID() }) }); };
  await run({ ...receipt(), outcome: "imported" });
  await run({ ...receipt(), duplicate: true, outcome: "unchanged", message: "Nada mudou." });
  await run({ ...receipt(), duplicate: true });
  await run({ protocol_version: 1, ok: false, error: { code: "HOST_X", message: "Falhou." } });
  const items = plainOf((await worker.invoke({ action: "ponte.recent" }, popup)).items);
  assert.deepEqual(items.map(item => item.outcome), ["failed", "already", "already", "imported"]);
  assert.equal(items[0].document_id, undefined);
  assert.equal(items[1].document_id, 1);
  // A failure before the Ponte answers is a "not completed" row too.
  const lost = workerFixture({ prepare: async () => { throw new Error("falha"); } });
  assert.equal((await lost.invoke(bundleMessage())).error.code, "EXTENSION_ERROR");
  assert.equal(plainOf((await lost.invoke({ action: "ponte.recent" }, popupOf(lost))).items)[0].outcome, "failed");
  // An import refused before it started leaves no row.
  const refused = workerFixture();
  await refused.invoke({ action: "ponte.import_capture", capture: capture({ source_url: Core.ORIGIN + "/?access_token=x" }) });
  assert.deepEqual(plainOf((await refused.invoke({ action: "ponte.recent" }, popupOf(refused))).items), []);
});
test("the list of recent imports belongs to the popup; open is for the popup and the SIGADOC tab", async () => {
  const worker = workerFixture(undefined, { store: { "ponte.recent": [{ number: "ORG-PRO-2026/00001", title: "Assunto", time: new Date().toISOString(), outcome: "imported", document_id: 3 }] } });
  const popup = popupOf(worker);
  for (const action of ["ponte.recent", "ponte.recent_clear"]) {
    for (const who of [worker.sender, { ...worker.sender, url: "https://attacker.test/" }, { ...worker.sender, frameId: 1 }, { ...worker.sender, id: "other-extension" }, { id: worker.chrome.runtime.id, url: worker.chrome.runtime.getURL("other.html") }, { ...popup, tab: { id: 2, url: Core.ORIGIN + "/" } }, { id: "other-extension", url: popup.url }]) {
      assert.equal((await worker.invoke({ action }, who)).error.code, "SENDER_DENIED", action);
    }
    assert.equal((await worker.invoke({ action, extra: 1 }, popup)).error.code, "SENDER_DENIED");
  }
  assert.equal((await worker.invoke({ action: "ponte.recent" }, popup)).items.length, 1);
  // Open: popup and content script accepted with the same input checks; nobody else.
  for (const who of [popup, worker.sender]) {
    assert.deepEqual(plainOf(await worker.invoke({ action: "ponte.open", document_id: 3 }, who)), { protocol_version: 1, ok: true, document_id: 3, ui_opened: true });
    for (const document_id of [0, -1, "3", 1.5, null]) assert.equal((await worker.invoke({ action: "ponte.open", document_id }, who)).error.code, "INVALID_REQUEST");
    assert.equal((await worker.invoke({ action: "ponte.open", document_id: 3, extra: 1 }, who)).error.code, "SENDER_DENIED");
  }
  // The popup still cannot look documents up or import.
  assert.equal((await worker.invoke({ action: "ponte.lookup", document_number: NUMBER }, popup)).error.code, "SENDER_DENIED");
  assert.equal((await worker.invoke(importMessage(null), popup)).error.code, "SENDER_DENIED");
});
test("the recent list is never opened to content scripts and the popup shows it as plain text", () => {
  const read = name => fs.readFileSync(path.join(__dirname, "../../extension/" + name), "utf8");
  for (const name of ["background.js", "content.js", "popup.js", "capture-core.js"]) assert.ok(!/setAccessLevel/.test(read(name)), name);
  assert.ok(!/ponte\.recent|storage\.session/.test(read("content.js") + read("capture-core.js")));
  assert.ok(/Importações recentes/.test(read("popup.html")) && /Nenhuma importação nesta sessão do navegador\./.test(read("popup.html")) && /Limpar a lista/.test(read("popup.html")));
  assert.ok(/Atualize o Ponte para enviar nota e prioridade ao importar\./.test(read("popup.html")));
});
