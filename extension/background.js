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
async function importCapture(message) {
  if (!message.capture.references.some(r => ["original", "attachment"].includes(r.kind))) {
    return nativeCall({ protocol_version: 1, action: "import_capture", capture: message.capture });
  }
  const key = message.capture.request_id;
  const snapshot = JSON.stringify(message.capture);
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
  const result = await nativeCall(packet);
  // Tell the page which PDFs could not be downloaded; the app also lists them.
  if (result?.ok === true && packet.capture?.coverage?.pending) {
    const before = new Set(message.capture.coverage.pending);
    result.file_notes = packet.capture.coverage.pending.filter(note => !before.has(note)).slice(0, 10);
  }
  // Delivered: the copy kept in session storage for a retry is no longer needed.
  if (result?.ok === true) await forgetBundle(key);
  return result;
}
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  (async () => {
    if (message?.action === "ponte.ping") {
      if (!Core.exactKeys(message, ["action"]) || !(authorizedTab(sender) || authorizedPopup(sender))) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      return nativeCall({ protocol_version: 1, action: "ping" });
    }
    if (message?.action === "ponte.lookup") {
      if (!Core.exactKeys(message, ["action", "document_number"]) || !authorizedTab(sender)) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      if (!validNumber(message.document_number)) return Core.errorResult("INVALID_REQUEST", "Número de documento inválido.");
      return nativeCall({ protocol_version: 1, action: "lookup", document_number: message.document_number }, 15000);
    }
    if (message?.action === "ponte.open") {
      if (!Core.exactKeys(message, ["action", "document_id"]) || !authorizedTab(sender)) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      if (!validId(message.document_id)) return Core.errorResult("INVALID_REQUEST", "Documento inválido.");
      return nativeCall({ protocol_version: 1, action: "open_document", document_id: message.document_id }, 15000);
    }
    if (!authorizedTab(sender)) return Core.errorResult("SENDER_DENIED", "A importação exige a aba principal do SIGADOC na origem autorizada.");
    if (!Core.exactKeys(message, ["action", "capture"]) || message.action !== "ponte.import_capture") return Core.errorResult("ACTION_DENIED", "Ação não permitida pela extensão.");
    try { Core.validateCapture(message.capture); }
    catch (error) { return Core.errorResult(error.code || "CAPTURE_SCHEMA", error.message); }
    return importCapture(message);
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
