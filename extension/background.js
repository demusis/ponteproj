"use strict";
importScripts("capture-core.js", "files.js");
const Core = globalThis.PonteCapture;
// Keep the exact downloaded bytes for retries while this worker remains alive.
// Only two bounded bundles are retained; nothing is written to browser storage.
const bundles = new Map();

function authorizedTab(sender) {
  return sender.id === chrome.runtime.id && Number.isInteger(sender.tab?.id) && sender.frameId === 0 && Core.allowedOrigin(sender.url) && Core.allowedOrigin(sender.tab.url);
}
function authorizedPopup(sender) {
  return sender.id === chrome.runtime.id && !sender.tab && sender.url === chrome.runtime.getURL("popup.html");
}
function nativeCall(request) {
  return new Promise(resolve => {
    let done = false;
    const finish = value => { if (!done) { done = true; clearTimeout(timer); resolve(value); } };
    const timer = setTimeout(() => finish(Core.errorResult("HOST_TIMEOUT", "O Ponte demorou para confirmar. Clique em Repetir: nada será duplicado.")), 45000);
    try {
      chrome.runtime.sendNativeMessage(Core.HOST, request, response => {
        const error = chrome.runtime.lastError;
        if (error) { finish(Core.nativeError(error.message || "")); return; }
        if (request.action === "ping") {
          if (response?.protocol_version !== 1 || response?.ok !== true) finish(Core.errorResult("HOST_SCHEMA", "O Ponte respondeu em um formato que esta extensão não reconhece. Atualize os dois pelo instalador do Ponte."));
          else finish({ protocol_version: 1, ok: true, app_version: typeof response.app_version === "string" ? response.app_version.slice(0, 40) : "" });
          return;
        }
        try { finish(Core.validateReceipt(response, request.capture.request_id)); }
        catch (error) { finish(Core.errorResult(error.code || "HOST_SCHEMA", error.message)); }
      });
    } catch { finish(Core.nativeError("")); }
  });
}
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  (async () => {
    if (message?.action === "ponte.ping") {
      if (!Core.exactKeys(message, ["action"]) || !(authorizedTab(sender) || authorizedPopup(sender))) return Core.errorResult("SENDER_DENIED", "Origem da solicitação não autorizada.");
      return nativeCall({ protocol_version: 1, action: "ping" });
    }
    if (!authorizedTab(sender)) return Core.errorResult("SENDER_DENIED", "A importação exige a aba principal do SIGADOC na origem autorizada.");
    if (!Core.exactKeys(message, ["action", "capture"]) || message.action !== "ponte.import_capture") return Core.errorResult("ACTION_DENIED", "Ação não permitida pela extensão.");
    try { Core.validateCapture(message.capture); }
    catch (error) { return Core.errorResult(error.code || "CAPTURE_SCHEMA", error.message); }
    if (!message.capture.references.some(r => ["original", "attachment"].includes(r.kind))) {
      return nativeCall({ protocol_version: 1, action: "import_capture", capture: message.capture });
    }
    const key = message.capture.request_id;
    const snapshot = JSON.stringify(message.capture);
    if (bundles.has(key) && bundles.get(key).snapshot !== snapshot) return Core.errorResult("REQUEST_CHANGED", "A página mudou desde a última tentativa. Atualize a página e importe de novo.");
    if (!bundles.has(key)) {
      if (bundles.size >= 2) bundles.delete(bundles.keys().next().value);
      bundles.set(key, {snapshot, packet:globalThis.PonteFiles.prepare(message.capture, Core)});
    }
    const packet = await bundles.get(key).packet;
    const result = await nativeCall(packet);
    // Tell the page which PDFs could not be downloaded; the app also lists them.
    if (result?.ok === true && packet.capture?.coverage?.pending) {
      const before = new Set(message.capture.coverage.pending);
      result.file_notes = packet.capture.coverage.pending.filter(note => !before.has(note)).slice(0, 10);
    }
    return result;
  })().then(sendResponse, () => sendResponse(Core.errorResult("EXTENSION_ERROR", "A extensão não conseguiu processar o pedido. Atualize a página e tente de novo.")));
  return true;
});
