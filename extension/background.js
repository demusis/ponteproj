"use strict";
importScripts("capture-core.js", "files.js");
const Core = globalThis.PonteCapture;
// Keep the exact downloaded bytes for retries. Fast path: two bounded bundles in
// this worker's memory. Chrome stops idle workers, so the same two are also kept
// in chrome.storage.session (memory only, cleared when the browser closes; the
// extension never writes to disk). A bundle that does not fit is simply not stored.
const bundles = new Map();
const SESSION_KEY = "ponte.bundles";
const SESSION_BUDGET = 8 * 1024 * 1024; // the session area allows about 10 MB in total
const COMMAND = "importar";
// The last imports of this browser session, for the popup. Small records only:
// number, subject, time, result, document code. Never text, never the note.
// chrome.storage.session stays closed to content scripts: its access level is never widened.
const RECENT_KEY = "ponte.recent";
const RECENT_MAX = 8;
const RECENT_OUTCOMES = ["imported", "already", "failed"];
// What the Ponte can do, asked just before an import that carries a note or priority.
const ACTIONS_TTL_MS = 60000;
let actionsSeen = null;

function authorizedTab(sender) {
  return sender.id === chrome.runtime.id && Number.isInteger(sender.tab?.id) && sender.frameId === 0 && Core.allowedOrigin(sender.url) && Core.allowedOrigin(sender.tab.url);
}
function authorizedPopup(sender) {
  return sender.id === chrome.runtime.id && !sender.tab && sender.url === chrome.runtime.getURL("popup.html");
}
function validNumber(value) { return typeof value === "string" && value.length >= 1 && value.length <= 200 && !/[\u0000-\u001f\u007f]/.test(value); }
function validId(value) { return Number.isSafeInteger(value) && value > 0; }
function shortText(value, max) { return typeof value === "string" ? value.slice(0, max) : ""; }
// A refusal in words (unknown extension, library that will not open) is passed on as it came.
function hostRefusal(response) {
  if (response?.protocol_version === 1 && response?.ok === false && typeof response?.error?.message === "string" && response.error.message) return Core.errorResult(typeof response.error.code === "string" ? response.error.code.slice(0, 100) : "HOST_REFUSED", response.error.message.slice(0, 2000));
  return null;
}
const schemaError = () => Core.errorResult("HOST_SCHEMA", "O Ponte respondeu em um formato que esta extensão não reconhece. Atualize os dois pelo instalador do Ponte.");
// One parser per native action; each returns only fields it has checked.
const PARSERS = {
  ping(response) {
    const refusal = hostRefusal(response);
    if (refusal) return refusal;
    if (response?.protocol_version !== 1 || response?.ok !== true) return schemaError();
    const actions = Array.isArray(response.actions) ? response.actions.filter(value => typeof value === "string" && value.length <= 40).slice(0, 10) : [];
    return { protocol_version: 1, ok: true, app_version: shortText(response.app_version, 40), actions };
  },
  lookup(response) {
    const refusal = hostRefusal(response);
    if (refusal) return refusal;
    if (response?.protocol_version !== 1 || response?.ok !== true || typeof response.found !== "boolean") return schemaError();
    if (!response.found) return { protocol_version: 1, ok: true, found: false };
    if (!validId(response.document_id)) return schemaError();
    return { protocol_version: 1, ok: true, found: true, document_id: response.document_id, imported_at: shortText(response.imported_at, 100) };
  },
  open_document(response) {
    const refusal = hostRefusal(response);
    if (refusal) return refusal;
    if (response?.protocol_version !== 1 || response?.ok !== true || !validId(response.document_id)) return schemaError();
    return { protocol_version: 1, ok: true, document_id: response.document_id, ui_opened: response.ui_opened === true };
  }
};
function nativeCall(request, timeoutMs = 45000) {
  return new Promise(resolve => {
    let done = false;
    const finish = value => { if (!done) { done = true; clearTimeout(timer); resolve(value); } };
    const timer = setTimeout(() => finish(Core.errorResult("HOST_TIMEOUT", "O Ponte demorou para confirmar. Clique em Repetir: nada será duplicado.")), timeoutMs);
    try {
      chrome.runtime.sendNativeMessage(Core.HOST, request, response => {
        const error = chrome.runtime.lastError;
        if (error) { finish(Core.nativeError(error.message || "")); return; }
        if (PARSERS[request.action]) { finish(PARSERS[request.action](response)); return; }
        try { finish(Core.validateReceipt(response, request.capture.request_id)); }
        catch (error) { finish(Core.errorResult(error.code || "HOST_SCHEMA", error.message)); }
      });
    } catch { finish(Core.nativeError("")); }
  });
}
async function storedBundles() {
  try {
    const value = (await chrome.storage.session.get(SESSION_KEY))?.[SESSION_KEY];
    return Array.isArray(value) ? value.filter(item => item && typeof item.key === "string" && typeof item.snapshot === "string" && item.packet) : [];
  } catch { return []; }
}
// One change of the stored list at a time: two tabs importing together
// must not lose each other's entry.
let storageTurn = Promise.resolve();
function inTurn(work) {
  const run = storageTurn.then(work, work);
  storageTurn = run.catch(() => {});
  return run;
}
function storeBundle(key, snapshot, packet) { return inTurn(() => storeBundleNow(key, snapshot, packet)); }
function forgetBundle(key) { return inTurn(() => forgetBundleNow(key)); }
async function storeBundleNow(key, snapshot, packet) {
  try {
    const entry = { key, snapshot, packet };
    if (JSON.stringify(entry).length > SESSION_BUDGET) return;
    let list = [...(await storedBundles()).filter(item => item.key !== key), entry].slice(-2);
    // Drop the older one when both together would not fit.
    if (list.length === 2 && JSON.stringify(list).length > SESSION_BUDGET) list = [entry];
    await chrome.storage.session.set({ [SESSION_KEY]: list });
  } catch { /* not stored: a retry downloads the files again */ }
}
async function forgetBundleNow(key) {
  try {
    const list = (await storedBundles()).filter(item => item.key !== key);
    await chrome.storage.session.set({ [SESSION_KEY]: list });
  } catch { /* nothing to do */ }
}
async function findBundle(key) {
  if (bundles.has(key)) return bundles.get(key);
  const stored = (await storedBundles()).find(item => item.key === key);
  if (!stored) return null;
  const entry = { snapshot: stored.snapshot, packet: stored.packet, stored: true };
  bundles.set(key, entry);
  return entry;
}
function rememberActions(result) {
  if (result?.ok === true && Array.isArray(result.actions)) actionsSeen = { at: Date.now(), actions: result.actions };
}
// Note and priority are rebuilt key by key from the validated message.
function cleanExtras(extras) {
  const clean = {};
  if (Object.hasOwn(extras, "note")) clean.note = extras.note;
  if (Object.hasOwn(extras, "priority")) clean.priority = extras.priority;
  return clean;
}
// { send } extras go along; { skipped } this Ponte does not take them; { error } the Ponte did not answer.
async function extrasPlan(extras) {
  if (!extras) return { send: null, skipped: false };
  let actions;
  if (actionsSeen && Date.now() - actionsSeen.at < ACTIONS_TTL_MS) actions = actionsSeen.actions;
  else {
    const ping = await nativeCall({ protocol_version: 1, action: "ping" }, 15000);
    if (ping?.ok !== true) return { error: ping };
    rememberActions(ping);
    actions = ping.actions;
  }
  return actions.includes("import_extras") ? { send: extras, skipped: false } : { send: null, skipped: true };
}
// Only checked fields of the Ponte's answer about the extras reach the page.
function finishExtras(result, plan) {
  if (result?.ok !== true) return result;
  const message = typeof result.extras_message === "string" ? result.extras_message.trim().slice(0, 500) : "";
  const applied = result.extras_applied === true;
  delete result.extras_applied; delete result.extras_message; delete result.extras_skipped;
  if (plan.skipped) result.extras_skipped = true;
  else if (plan.send) { result.extras_applied = applied; if (message) result.extras_message = message; }
  return result;
}
// The retry key covers the page and the extras: both must be the same as in the first attempt.
function requestSnapshot(message, extras) {
  return JSON.stringify(message.capture) + (extras ? "\n" + JSON.stringify(extras) : "");
}
async function importCapture(message) {
  const extras = Object.hasOwn(message, "extras") ? cleanExtras(message.extras) : null;
  if (!message.capture.references.some(r => ["original", "attachment"].includes(r.kind))) {
    const plan = await extrasPlan(extras);
    if (plan.error) return plan.error;
    const request = { protocol_version: 1, action: "import_capture", capture: message.capture };
    if (plan.send) request.extras = plan.send;
    return finishExtras(await nativeCall(request), plan);
  }
  const key = message.capture.request_id;
  const snapshot = requestSnapshot(message, extras);
  let entry = await findBundle(key);
  if (entry && entry.snapshot !== snapshot) return Core.errorResult("REQUEST_CHANGED", "A página mudou desde a última tentativa. Atualize a página e importe de novo.");
  if (!entry) {
    if (bundles.size >= 2) bundles.delete(bundles.keys().next().value);
    entry = { snapshot, packet: globalThis.PonteFiles.prepare(message.capture, Core) };
    bundles.set(key, entry);
  }
  let packet;
  try { packet = await entry.packet; }
  catch (error) { bundles.delete(key); throw error; }
  if (!entry.stored) { entry.stored = true; await storeBundle(key, snapshot, packet); }
  const plan = await extrasPlan(extras);
  if (plan.error) return plan.error;
  const result = finishExtras(await nativeCall(plan.send ? { ...packet, extras: plan.send } : packet), plan);
  // Tell the page which PDFs could not be downloaded; the app also lists them.
  if (result?.ok === true && packet.capture?.coverage?.pending) {
    const before = new Set(message.capture.coverage.pending);
    result.file_notes = packet.capture.coverage.pending.filter(note => !before.has(note)).slice(0, 10);
  }
  // Delivered: the copy kept in session storage for a retry is no longer needed.
  if (result?.ok === true) await forgetBundle(key);
  return result;
}
async function storedRecent() {
  try {
    const value = (await chrome.storage.session.get(RECENT_KEY))?.[RECENT_KEY];
    if (!Array.isArray(value)) return [];
    return value.filter(item => item && typeof item.number === "string" && typeof item.title === "string" && typeof item.time === "string" && RECENT_OUTCOMES.includes(item.outcome)).slice(0, RECENT_MAX).map(item => {
      const clean = { number: item.number.slice(0, 200), title: item.title.slice(0, 120), time: item.time.slice(0, 40), outcome: item.outcome };
      if (validId(item.document_id)) clean.document_id = item.document_id;
      return clean;
    });
  } catch { return []; }
}
function recordRecent(capture, result) {
  const ok = result?.ok === true;
  const outcome = !ok ? "failed" : ["unchanged", "legacy_duplicate"].includes(Core.importOutcome(result)) ? "already" : "imported";
  const record = { number: capture.document_number.slice(0, 200), title: shortText(capture.title, 120), time: new Date().toISOString(), outcome };
  if (ok && validId(result.document_id)) record.document_id = result.document_id;
  return inTurn(async () => {
    try { await chrome.storage.session.set({ [RECENT_KEY]: [record, ...(await storedRecent())].slice(0, RECENT_MAX) }); }
    catch { /* the list is only a convenience */ }
  });
}
function clearRecent() {
  return inTurn(async () => { try { await chrome.storage.session.set({ [RECENT_KEY]: [] }); } catch { /* nothing to do */ } });
}
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  (async () => {
    if (message?.action === "ponte.ping") {
      if (!Core.exactKeys(message, ["action"]) || !(authorizedTab(sender) || authorizedPopup(sender))) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      const answer = await nativeCall({ protocol_version: 1, action: "ping" });
      rememberActions(answer);
      return answer;
    }
    // The list of recent imports belongs to the popup alone.
    if (message?.action === "ponte.recent" || message?.action === "ponte.recent_clear") {
      if (!Core.exactKeys(message, ["action"]) || !authorizedPopup(sender)) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      if (message.action === "ponte.recent_clear") { await clearRecent(); return { protocol_version: 1, ok: true }; }
      return { protocol_version: 1, ok: true, items: await storedRecent() };
    }
    if (message?.action === "ponte.lookup") {
      if (!Core.exactKeys(message, ["action", "document_number"]) || !authorizedTab(sender)) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      if (!validNumber(message.document_number)) return Core.errorResult("INVALID_REQUEST", "Número de documento inválido.");
      return nativeCall({ protocol_version: 1, action: "lookup", document_number: message.document_number }, 15000);
    }
    if (message?.action === "ponte.open") {
      if (!Core.exactKeys(message, ["action", "document_id"]) || !(authorizedTab(sender) || authorizedPopup(sender))) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      if (!validId(message.document_id)) return Core.errorResult("INVALID_REQUEST", "Documento inválido.");
      return nativeCall({ protocol_version: 1, action: "open_document", document_id: message.document_id }, 15000);
    }
    if (!authorizedTab(sender)) return Core.errorResult("SENDER_DENIED", "A importação exige a aba principal do SIGADOC na origem autorizada.");
    if (!Core.exactKeys(message, Object.hasOwn(message || {}, "extras") ? ["action", "capture", "extras"] : ["action", "capture"]) || message.action !== "ponte.import_capture") return Core.errorResult("ACTION_DENIED", "Ação não permitida pela extensão.");
    try {
      Core.validateCapture(message.capture);
      if (Object.hasOwn(message, "extras")) {
        Core.validateExtras(message.extras);
        if (Core.bytes(JSON.stringify({ protocol_version: 1, action: "import_capture", capture: message.capture, extras: message.extras })) > Core.MAX_PAYLOAD_BYTES) throw new Error("payload");
      }
    }
    catch (error) { return error.message === "payload" ? Core.errorResult("PAYLOAD_TOO_LARGE", "O documento com a nota é grande demais para importar de uma vez. Nada foi importado.") : Core.errorResult(error.code || "CAPTURE_SCHEMA", error.message); }
    let result;
    try { result = await importCapture(message); }
    catch (error) { await recordRecent(message.capture, null); throw error; }
    await recordRecent(message.capture, result);
    return result;
  })().then(sendResponse, () => sendResponse(Core.errorResult("EXTENSION_ERROR", "A extensão não conseguiu processar o pedido. Atualize a página e tente de novo.")));
  return true;
});
// Keyboard shortcut: forward to the active tab only when it is a SIGADOC page.
chrome.commands?.onCommand.addListener(command => {
  if (command !== COMMAND) return;
  try {
    // The address of a tab is only visible for pages the extension has host access to.
    chrome.tabs.query({ active: true, currentWindow: true }, tabs => {
      void chrome.runtime.lastError;
      const tab = Array.isArray(tabs) ? tabs[0] : null;
      if (!tab || !Number.isInteger(tab.id) || !Core.allowedOrigin(tab.url)) return;
      chrome.tabs.sendMessage(tab.id, { action: "ponte.shortcut_import" }, { frameId: 0 }, () => { void chrome.runtime.lastError; });
    });
  } catch { /* no active tab */ }
});
