"use strict";
const check = document.getElementById("check");
const status = document.getElementById("status");
const hint = document.getElementById("hint");
const help = document.getElementById("help");
const oldNote = document.getElementById("old");
const extrasOld = document.getElementById("extras-old");
const recentList = document.getElementById("recent");
const recentEmpty = document.getElementById("recent-empty");
const recentNote = document.getElementById("recent-note");
const recentClear = document.getElementById("recent-clear");
const shortcut = document.getElementById("shortcut");
const extensionId = document.getElementById("extension-id");
const copyId = document.getElementById("copy-id");
extensionId.textContent = chrome.runtime.id;
copyId.addEventListener("click", () => {
  navigator.clipboard?.writeText(chrome.runtime.id).then(() => { copyId.textContent = "copiado"; setTimeout(() => { copyId.textContent = "copiar"; }, 1500); }).catch(() => {});
});
// The key may have been changed by the person; show the one in use.
try {
  chrome.commands?.getAll(commands => {
    const command = (commands || []).find(item => item.name === "importar");
    if (command && !chrome.runtime.lastError) shortcut.textContent = command.shortcut ? `Atalho: ${command.shortcut} — pode ser trocado em chrome://extensions/shortcuts` : "Atalho: nenhum definido — pode ser escolhido em chrome://extensions/shortcuts";
  });
} catch { /* the text above stays */ }
let ponteActions = null;
let recentItems = [];
function show(ok, text, extra, old, noExtras) {
  oldNote.hidden = !old;
  extrasOld.hidden = !noExtras;
  status.textContent = text;
  status.className = `status ${ok ? "ok" : "bad"}`;
  hint.textContent = extra || "";
  help.open = !ok;
}
function verify() {
  check.disabled = true;
  status.textContent = "Verificando a ligação com o Ponte…";
  status.className = "status";
  chrome.runtime.sendMessage({ action: "ponte.ping" }, response => {
    check.disabled = false;
    ponteActions = !chrome.runtime.lastError && response?.ok === true && Array.isArray(response.actions) ? response.actions : null;
    if (chrome.runtime.lastError) show(false, "✗ A extensão não respondeu.", "Feche e abra o navegador. Se continuar, remova e carregue a extensão de novo.");
    else if (response?.ok === true && response.protocol_version === 1) show(true, `✓ Ligado ao Ponte${response.app_version ? " " + response.app_version : ""}.`, "Abra um documento no SIGADOC e clique em “Importar para o assistente”.", !(Array.isArray(response.actions) && response.actions.includes("lookup")), Array.isArray(response.actions) && response.actions.includes("lookup") && !response.actions.includes("import_extras"));
    else show(false, "✗ Não foi possível ligar ao Ponte.", response?.error?.message || "Tente de novo em alguns segundos.");
    renderRecent();
  });
}
const OUTCOME_TEXT = { imported: "Importado", already: "Já estava no Ponte", failed: "Não concluída" };
function clock(iso) {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
}
function renderRecent() {
  recentList.replaceChildren();
  recentEmpty.hidden = recentItems.length > 0;
  recentClear.hidden = recentItems.length === 0;
  const canOpen = Array.isArray(ponteActions) && ponteActions.includes("open_document");
  for (const item of recentItems) {
    const row = document.createElement("li");
    const line = document.createElement("div");
    line.className = "line";
    const number = document.createElement("span");
    number.textContent = item.number;
    const time = document.createElement("span");
    time.textContent = clock(item.time);
    line.append(number, time);
    const subject = document.createElement("div");
    subject.className = "subject";
    subject.textContent = item.title.length > 70 ? item.title.slice(0, 69) + "…" : item.title;
    subject.title = item.title;
    const result = document.createElement("div");
    result.className = "result " + item.outcome;
    result.append(OUTCOME_TEXT[item.outcome] || "");
    if (item.outcome !== "failed" && Number.isSafeInteger(item.document_id) && canOpen) {
      const open = document.createElement("button");
      open.type = "button";
      open.className = "link";
      open.textContent = "Abrir no Ponte";
      open.addEventListener("click", () => openDocument(item.document_id, open));
      result.append(open);
    }
    row.append(line, subject, result);
    recentList.append(row);
  }
}
function loadRecent() {
  chrome.runtime.sendMessage({ action: "ponte.recent" }, response => {
    recentItems = !chrome.runtime.lastError && response?.ok === true && Array.isArray(response.items) ? response.items : [];
    renderRecent();
  });
}
function openDocument(id, control) {
  control.disabled = true;
  recentNote.hidden = true;
  chrome.runtime.sendMessage({ action: "ponte.open", document_id: id }, response => {
    control.disabled = false;
    const failure = chrome.runtime.lastError || response?.ok !== true;
    recentNote.textContent = failure ? (typeof response?.error?.message === "string" && response.error.message ? response.error.message : "O Ponte não respondeu. Abra o Ponte e tente de novo.") : response.ui_opened === false ? "Abra o Ponte para ver o documento." : "";
    recentNote.hidden = !recentNote.textContent;
  });
}
recentClear.addEventListener("click", () => {
  chrome.runtime.sendMessage({ action: "ponte.recent_clear" }, () => { void chrome.runtime.lastError; recentItems = []; recentNote.hidden = true; renderRecent(); });
});
check.addEventListener("click", verify);
verify();
loadRecent();
