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
  return { schema_version: 1, request_id: UUID, source_url: Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=SEPLAG-PRO-2026%2F01234", document_number: "SEPLAG-PRO-2026/01234", title: "Aquisição de material", captured_at: "2026-11-01T12:00:00.000Z", text: "Texto de origem.", metadata: {}, references: [], coverage: { adapter: Core.ADAPTER, recognized: ["Campo explícito"], pending: ["DOM autenticado não validado"] }, ...changes };
}
function receipt(requestId = UUID) { return { protocol_version: 1, ok: true, request_id: requestId, document_id: 1, capture_id: 2, duplicate: false, message: "Salvo", ui_opened: false }; }
test("manifest uses fixed public key/ID, restricted isolated MV3, no external messages", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "../../extension/manifest.json"), "utf8"));
  const stableKey = fs.readFileSync(path.join(__dirname, "../../extension-key.txt"), "utf8").trim();
  assert.equal(manifest.key, stableKey);
  const digest = crypto.createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest().subarray(0, 16).toString("hex");
  assert.equal(digest.replace(/[0-9a-f]/g, char => String.fromCharCode(97 + parseInt(char, 16))), "ecllnakmppjinmednoikimbpnbgbimhg");
  assert.equal(manifest.manifest_version, 3);
  assert.deepEqual(manifest.permissions, ["nativeMessaging"]);
  assert.deepEqual(manifest.host_permissions, [Core.ORIGIN + "/*"]);
  assert.equal(manifest.content_scripts[0].world, "ISOLATED");
  assert.equal(manifest.content_scripts[0].all_frames, false);
  assert.equal(manifest.externally_connectable, undefined);
});
test("URL sanitizer keeps only official document-number parameters and exact HTTPS host", () => {
  const url = Core.cleanUrl(Core.ORIGIN + "/siga;jsessionid=private/doc?token=secret&PHPSESSID=secret&sigla=SEPLAG-PRO-2026%2F01234&documento=opaque-session&code=auth#secret");
  assert.equal(url, Core.ORIGIN + "/siga/doc?sigla=SEPLAG-PRO-2026%2F01234");
  for (const url of ["http://www.sigadoc.mt.gov.br/", "https://sigadoc.mt.gov.br/", "https://www.sigadoc.mt.gov.br.attacker.test/", "https://user:password@www.sigadoc.mt.gov.br/", "javascript:alert(1)", "https://www.sigadoc.mt.gov.br:444/"]) assert.equal(Core.allowedOrigin(url), false);
});
test("capture validation rejects invented identity, unknown keys, secrets in URLs and invalid calendar dates", () => {
  assert.equal(Core.validateCapture(capture()).request_id, UUID);
  for (const changes of [{ document_number: "Sem número 2026" }, { document_number: "SEPLAG-PRO-2026/99999" }, { title: "" }, { request_id: "random" }, { source_url: Core.ORIGIN + "/?token=secret" }, { metadata: { password: "secret" } }, { captured_at: "2026-02-31T12:00:00Z" }, { extra: true }]) assert.throws(() => Core.validateCapture(capture(changes)), Core.CaptureError);
  assert.equal(Core.documentNumber("Documento nº SEPLAG-PRO-2026/01234"), "SEPLAG-PRO-2026/01234");
  assert.equal(Core.documentNumber("12345/2026"), "12345/2026");
  assert.equal(Core.documentNumber("Resultado de pesquisa 2026"), null);
});
test("SIGA via sigla preserves source identity and matches its canonical base only on exibir", () => {
  const source = Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=SEPLAG-PRO-2026%2F01234-A";
  const via = capture({ source_url: source });
  assert.equal(Core.cleanUrl(source + "&linkVolta=history.back()&token=secret"), source);
  assert.equal(Core.validateCapture(via).document_number, "SEPLAG-PRO-2026/01234");
  assert.equal(Core.identity(via), Core.identity(capture()));
  assert.equal(Core.documentNumber("SEPLAG-PRO-2026/01234-A"), null);
  for (const source_url of [
    Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=SEPLAG-PRO-2026%2F99999-A",
    Core.ORIGIN + "/sigaex/app/expediente/doc/exibir?sigla=SEPLAG-PRO-2026%2F01234-AB",
    Core.ORIGIN + "/outra-rota?sigla=SEPLAG-PRO-2026%2F01234-A"
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
  await session.run(capture({ request_id: nextId, document_number: "SEPLAG-PRO-2026/01235", source_url: Core.ORIGIN + "/doc?sigla=SEPLAG-PRO-2026%2F01235" }));
  assert.equal(sent[1].request_id, nextId);
});
function workerFixture(filesApi) {
  let listener;
  const calls = [];
  const chrome = { runtime: { id: "ecllnakmppjinmednoikimbpnbgbimhg", getURL: name => "chrome-extension://ecllnakmppjinmednoikimbpnbgbimhg/" + name, onMessage: { addListener: value => { listener = value; } }, sendNativeMessage: (host, request, callback) => { calls.push({ host, request }); callback(request.action === "ping" ? { protocol_version: 1, ok: true } : receipt(request.capture.request_id)); } } };
  const context = { chrome, importScripts: () => {}, PonteCapture: Core, PonteFiles: filesApi, setTimeout, clearTimeout };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../../extension/background.js"), "utf8"), context);
  const sender = { id: chrome.runtime.id, frameId: 0, url: Core.ORIGIN + "/doc", tab: { id: 7, url: Core.ORIGIN + "/doc" } };
  const invoke = (message, who = sender) => new Promise(resolve => listener(message, who, resolve));
  return { invoke, calls, sender, chrome };
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
  value.metadata.movement_source=Core.ORIGIN+"/sigaex/app/expediente/doc/exibirAntigo?sigla=SEPLAGPRO202601234";
  assert.doesNotThrow(()=>Core.validateCapture(value));
});
