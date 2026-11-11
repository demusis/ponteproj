(function () {
  "use strict";
  const Core = globalThis.PonteCapture;
  if (!Core || !Core.allowedOrigin(location.href) || window.top !== window || globalThis.__ponteprojContentStarted) return;
  globalThis.__ponteprojContentStarted = true;
  const host = document.createElement("div");
  host.id = "ponteproj-import-assistente";
  host.setAttribute("data-ponteproj-ui", "true");
  const shadow = host.attachShadow({ mode: "closed" });
  const style = document.createElement("style");
  style.textContent = ":host{all:initial;position:fixed;right:18px;bottom:18px;z-index:2147483646;font:14px/1.45 system-ui,sans-serif;color:#172b45}section{width:min(340px,calc(100vw - 40px));padding:14px;border:1px solid #a9b8cb;border-radius:12px;background:#fff;box-shadow:0 6px 24px #10264033}section.idle{width:min(260px,calc(100vw - 40px));padding:10px 12px;opacity:.9}button{width:100%;border:0;border-radius:7px;background:#1257a6;color:#fff;padding:11px 12px;font:600 14px system-ui;cursor:pointer}button:focus-visible,a:focus-visible{outline:3px solid #d99500;outline-offset:3px}button:disabled{background:#667588;cursor:default}p{margin:9px 0 0;overflow-wrap:anywhere;font-size:12px}.identity{font-weight:600}.idle .identity{margin-top:0;font-weight:500;color:#4c5d72}.idle button,.idle p.status,.idle details{display:none}p.error{color:#9b2c2c}p.success{color:#1d6b3a}p.warning{color:#8a5a00;font-weight:600}a{color:#1257a6}details{margin-top:7px;font-size:11px}summary{cursor:pointer}ul{padding-left:18px}li{margin-top:4px}[hidden]{display:none!important}p.note{color:#52657a}button.link{display:inline;width:auto;padding:0;background:none;color:#1257a6;font:inherit;font-weight:600;text-decoration:underline}button.link:disabled{background:none;color:#667588}.idle p.ponte,.idle p.layout,.idle p.open-row,.idle p.open-note,.idle .known{display:none}.known{height:64px;overflow:hidden}.extras-row{height:24px;margin-top:6px;overflow:hidden;font-size:12px}.extras-row.off{visibility:hidden}.extras-box{margin-top:2px;font-size:12px}.extras-box label{display:block;margin-top:8px;font-weight:600}textarea,select{display:block;width:100%;box-sizing:border-box;margin-top:3px;padding:6px 8px;border:1px solid #a9b8cb;border-radius:6px;background:#fff;color:#172b45;font:12px/1.4 system-ui,sans-serif}textarea{resize:vertical}textarea:focus-visible,select:focus-visible{outline:3px solid #d99500;outline-offset:1px}textarea:disabled,select:disabled{background:#eef1f5;color:#52657a}.extras-count{margin:3px 0 0;text-align:right;color:#8a5a00}.idle .extras-row,.idle .extras-box{display:none}";
  const panel = document.createElement("section");
  panel.setAttribute("aria-label", "Importação para o Ponte");
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "Importar para o assistente";
  const identity = document.createElement("p");
  identity.className = "identity";
  const status = document.createElement("p");
  status.className = "status";
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const details = document.createElement("details");
  const summary = document.createElement("summary");
  summary.textContent = "O que será importado";
  const coverage = document.createElement("ul");
  details.append(summary, coverage);
  const layoutLine = document.createElement("p");
  layoutLine.className = "layout warning";
  layoutLine.textContent = Core.UNRECOGNIZED_TEXT;
  layoutLine.hidden = true;
  // What the Ponte already knows about this document, shown before any click.
  const ponteLine = document.createElement("p");
  ponteLine.className = "ponte";
  ponteLine.setAttribute("aria-live", "polite");
  ponteLine.hidden = true;
  const ponteText = document.createElement("span");
  const retryLink = document.createElement("button");
  retryLink.type = "button";
  retryLink.className = "link";
  retryLink.textContent = "Tentar de novo";
  ponteLine.append(ponteText, " ", retryLink);
  const openRow = document.createElement("p");
  openRow.className = "open-row";
  openRow.hidden = true;
  const openLink = document.createElement("button");
  openLink.type = "button";
  openLink.className = "link";
  openLink.textContent = "Abrir no Ponte";
  openRow.append(openLink);
  const openNote = document.createElement("p");
  openNote.className = "open-note";
  openNote.setAttribute("aria-live", "polite");
  openNote.hidden = true;
  // What the Ponte knows takes the same room whatever the answer, so the
  // size of the panel tells the page nothing about the library.
  const known = document.createElement("div");
  known.className = "known";
  known.append(ponteLine, openRow);
  // Note and priority for the document on screen. The link keeps its room
  // (hidden, not removed) when the Ponte cannot take them, so the panel's size
  // does not depend on the Ponte's answer; the controls only appear when the person opens them.
  const extrasRow = document.createElement("div");
  extrasRow.className = "extras-row off";
  const extrasLink = document.createElement("button");
  extrasLink.type = "button";
  extrasLink.className = "link";
  extrasLink.textContent = "Adicionar nota ou prioridade";
  extrasLink.setAttribute("aria-expanded", "false");
  extrasRow.append(extrasLink);
  const extrasBox = document.createElement("div");
  extrasBox.className = "extras-box";
  extrasBox.hidden = true;
  const noteLabel = document.createElement("label");
  noteLabel.textContent = "Nota para o Ponte";
  const noteArea = document.createElement("textarea");
  noteArea.rows = 3;
  noteArea.placeholder = "Ex.: responder até sexta; falar com o setor de protocolo";
  noteLabel.append(noteArea);
  const noteCount = document.createElement("p");
  noteCount.className = "extras-count";
  noteCount.hidden = true;
  const priorityLabel = document.createElement("label");
  priorityLabel.textContent = "Prioridade";
  const prioritySelect = document.createElement("select");
  for (const [value, text] of [["", "Não mudar"], ["baixa", "Baixa"], ["normal", "Normal"], ["alta", "Alta"], ["urgente", "Urgente"]]) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = text;
    prioritySelect.append(option);
  }
  priorityLabel.append(prioritySelect);
  extrasBox.append(noteLabel, noteCount, priorityLabel);
  panel.append(button, extrasRow, extrasBox, identity, layoutLine, known, status, openNote, details);
  shadow.append(style, panel);
  let enriching = false;
  let snapshot = null;
  let pageError = "";
  let notice = null;
  let timer = null;
  let lastUrl = location.href;
  // Ponte status check: one at a time, once per document, again after an import.
  let ponteActions = null;
  let ponteState = null;
  let checking = false;
  let checkAgain = false;
  let checkedNumber = null;
  let checkTimer = null;
  let openBusy = false;
  let openMessage = "";
  // Extras: the controls belong to the document with `extrasNumber`; `sendExtras` is
  // what the running attempt carries and `retryExtras` what a retry must carry again.
  let extrasOpen = false;
  let extrasNumber = null;
  let sendExtras = null;
  let retryExtras = null;
  const CHECK_DELAY_MS = 400, CHECK_TIMEOUT_MS = 5000;
  const send = message => new Promise(resolve => {
    try { chrome.runtime.sendMessage(message, response => { if (chrome.runtime.lastError) resolve(null); else resolve(response); }); }
    catch { resolve(null); }
  });
  const session = new Core.ImportSession(capture => new Promise(resolve => {
    try {
      chrome.runtime.sendMessage(sendExtras ? { action: "ponte.import_capture", capture, extras: { ...sendExtras } } : { action: "ponte.import_capture", capture }, response => {
        if (chrome.runtime.lastError) resolve(Core.nativeError(""));
        else resolve(response);
      });
    } catch { resolve(Core.errorResult("EXTENSION_RELOADED", "A extensão foi atualizada. Atualize a página do SIGADOC (F5) e importe de novo.")); }
  }));
  function render() {
    if (!host.isConnected && document.body) document.body.append(host);
    const currentIdentity = snapshot ? Core.identity(snapshot) : null;
    const sentIdentity = session.capture ? Core.identity(session.capture) : null;
    const same = currentIdentity && currentIdentity === sentIdentity;
    // Outside a document page the panel shrinks to a one-line hint; the
    // button, status and details only make sense on a ficha.
    const idle = !snapshot && session.state !== "sending" && !notice;
    panel.classList.toggle("idle", idle);
    const view = snapshot && ponteState && ponteState.number === snapshot.document_number ? Core.statusView(ponteState.outcome) : Core.statusView(null);
    const importedNow = Boolean(notice && notice.result.ok && same);
    button.disabled = !snapshot || enriching || session.state === "sending";
    button.textContent = session.state === "sending" ? "Aguardando o Ponte…" : session.state === "error" && same ? "Repetir importação" : view.importLabel;
    layoutLine.hidden = !(snapshot && snapshot.layout_unrecognized);
    // Note and priority: offered only when the last answer of the Ponte says it takes them.
    const canExtras = Boolean(ponteActions?.includes("import_extras"));
    const locked = session.state === "sending" || enriching || (session.state === "error" && same);
    extrasRow.classList.toggle("off", !canExtras);
    extrasLink.disabled = !canExtras;
    extrasLink.textContent = extrasOpen ? "Ocultar nota e prioridade" : "Adicionar nota ou prioridade";
    extrasLink.setAttribute("aria-expanded", String(extrasOpen));
    extrasBox.hidden = !(canExtras && extrasOpen);
    noteArea.disabled = locked;
    prioritySelect.disabled = locked;
    const noteChars = Array.from(noteArea.value).length;
    noteCount.hidden = noteChars <= 1800;
    noteCount.textContent = `${noteChars} de ${Core.MAX_NOTE_CHARS}`;
    // After an import the receipt says it all; the "already in Ponte" line would repeat it.
    const missing = Boolean(snapshot && ponteState && ponteState.number === snapshot.document_number && ponteState.outcome.kind === "missing" && !importedNow);
    const showLine = Boolean(snapshot && (view.text || missing) && !(view.open && importedNow));
    ponteLine.hidden = !showLine;
    ponteLine.className = `ponte ${missing ? "note" : view.tone}`.trim();
    ponteText.textContent = missing ? "Ainda não está no Ponte." : view.text;
    retryLink.hidden = !view.retry;
    const target = openTarget(view);
    openRow.hidden = !target;
    openLink.disabled = openBusy;
    openNote.textContent = openMessage;
    openNote.hidden = !openMessage || !target;
    identity.textContent = snapshot ? `${snapshot.document_number} — ${snapshot.title}` : (pageError && !/ficha|documento individual|lista|pesquisa/i.test(pageError) ? pageError : "Abra um documento do SIGADOC para importar");
    let message = pageError || "Importa o texto, os PDFs e o histórico deste documento para o Ponte.";
    let tone = pageError ? "error" : "";
    if (session.state === "sending") { message = `Enviando ${session.capture.document_number}… aguarde a confirmação do Ponte.`; tone = ""; }
    else if (notice) {
      const prefix = same ? "" : `${notice.capture.document_number}: `;
      if (notice.result.ok) {
        const outcome = Core.importOutcome(notice.result);
        const opened = notice.result.ui_opened === false ? " Abra o Ponte para ver o documento." : " O Ponte abriu o documento.";
        if (outcome === "unchanged") {
          // Nothing was stored; the Ponte explains why and what to do.
          message = prefix + notice.result.message;
          tone = "warning";
        } else if (outcome === "legacy_duplicate") {
          message = prefix + "Este documento já estava importado; nada mudou." + opened;
          tone = "warning";
        } else {
          message = prefix + (outcome === "confirmed" ? "Importação confirmada." : "Importado.") + opened;
          const notes = Array.isArray(notice.result.file_notes) ? notice.result.file_notes : [];
          if (notes.length) message += ` ${Core.filesNotDownloaded(notes.length)}: ${notes.join("; ")}`;
          tone = "success";
        }
        const extrasText = Core.extrasNotice(notice.result, notice.extras);
        if (extrasText) message += ` ${extrasText}`;
      } else { message = prefix + notice.result.error.message; tone = "error"; }
    }
    status.textContent = message;
    status.className = `status ${tone}`.trim();
    coverage.replaceChildren();
    const entries = snapshot ? [...snapshot.coverage.recognized.map(value => `✓ ${value}`), ...snapshot.coverage.pending.map(value => `• ${value}`)] : [];
    for (const text of entries) { const item = document.createElement("li"); item.textContent = text; coverage.append(item); }
  }
  function refresh() {
    timer = null;
    try { snapshot = Core.extractDocument(document, location.href); pageError = ""; }
    catch (error) { snapshot = null; pageError = error.message || "Ficha documental não reconhecida."; }
    if (!snapshot) { checkedNumber = null; ponteState = null; openMessage = ""; clearTimeout(checkTimer); checkTimer = null; }
    else queueCheck(false);
    // The note and priority belong to the document they were written for.
    if (snapshot) {
      if (extrasNumber !== null && extrasNumber !== snapshot.document_number) clearExtras();
      extrasNumber = snapshot.document_number;
    }
    render();
  }
  function clearExtras() {
    noteArea.value = "";
    prioritySelect.value = "";
    extrasOpen = false;
  }
  extrasLink.addEventListener("click", () => { extrasOpen = !extrasOpen; render(); if (extrasOpen) noteArea.focus(); });
  // The limit counts characters, not UTF-16 units: enforced here, not by the control.
  noteArea.addEventListener("input", () => { const limited = Core.limitNote(noteArea.value); if (limited !== noteArea.value) noteArea.value = limited; render(); });
  function openTarget(view) {
    if (!snapshot || !ponteActions?.includes("open_document")) return 0;
    const sent = session.capture ? Core.identity(session.capture) : null;
    if (notice && notice.result.ok && sent === Core.identity(snapshot) && Number.isSafeInteger(notice.result.document_id)) return notice.result.document_id;
    return view.open && ponteState?.outcome?.kind === "found" ? ponteState.outcome.document_id : 0;
  }
  // A page that keeps changing its number cannot make the Ponte answer
  // more than a few times a minute.
  const checkTimes = [];
  function queueCheck(force) {
    if (!snapshot || (!force && checkedNumber === snapshot.document_number)) return;
    const now = Date.now();
    while (checkTimes.length && now - checkTimes[0] > 60000) checkTimes.shift();
    if (checkTimes.length >= 10) { checkedNumber = snapshot.document_number; ponteState = { number: checkedNumber, outcome: { kind: "hidden" } }; return; }
    checkTimes.push(now);
    clearTimeout(checkTimer);
    checkTimer = setTimeout(runCheck, CHECK_DELAY_MS);
  }
  async function doCheck(number) {
    const ping = await send({ action: "ponte.ping" });
    ponteActions = ping?.ok === true && Array.isArray(ping.actions) ? ping.actions : null;
    const lookup = ponteActions?.includes("lookup") ? await send({ action: "ponte.lookup", document_number: number }) : null;
    return Core.checkOutcome(ping, lookup);
  }
  async function runCheck() {
    checkTimer = null;
    if (!snapshot) return;
    if (checking) { checkAgain = true; return; }
    const number = snapshot.document_number;
    checking = true;
    checkedNumber = number;
    openMessage = "";
    let late = false;
    const limit = setTimeout(() => { late = true; ponteState = { number, outcome: { kind: "hidden" } }; render(); }, CHECK_TIMEOUT_MS);
    let outcome;
    try { outcome = await doCheck(number); } catch { outcome = { kind: "hidden" }; }
    clearTimeout(limit);
    checking = false;
    if (!late) ponteState = { number, outcome };
    if (checkAgain) { checkAgain = false; queueCheck(true); }
    render();
  }
  retryLink.addEventListener("click", () => { ponteActions = null; queueCheck(true); });
  openLink.addEventListener("click", async () => {
    const id = openTarget(Core.statusView(ponteState && snapshot && ponteState.number === snapshot.document_number ? ponteState.outcome : null));
    if (!id || openBusy) return;
    openBusy = true; openMessage = ""; render();
    const response = await send({ action: "ponte.open", document_id: id });
    openBusy = false;
    if (response?.ok === true) openMessage = response.ui_opened === false ? "Abra o Ponte para ver o documento." : "";
    else openMessage = typeof response?.error?.message === "string" && response.error.message ? response.error.message : "O Ponte não respondeu. Abra o Ponte e tente de novo.";
    render();
  });
  // Keyboard shortcut, forwarded by the background: the same as a click on the button.
  chrome.runtime.onMessage?.addListener((message, sender) => {
    if (sender?.id === chrome.runtime.id && !sender.tab && Core.exactKeys(message, ["action"]) && message.action === "ponte.shortcut_import") button.click();
  });
  function schedule() { if (timer === null) timer = setTimeout(refresh, 180); }
  button.addEventListener("click", async () => {
    // Re-read immediately: the DOM may have changed since the observer ran.
    refresh();
    if (!snapshot || enriching || session.state === "sending") return;
    let capture, extras;
    const retrying = session.state === "error" && session.capture && Core.identity(snapshot) === Core.identity(session.capture);
    try {
      capture = retrying ? session.capture : Core.buildCapture(snapshot);
      // A retry carries exactly the note and priority of the first attempt.
      // Only what the person can see is sent: the area must be available and open.
      // Values typed earlier stay in memory and come back with the area.
      const seen = Boolean(ponteActions?.includes("import_extras")) && extrasOpen;
      extras = retrying ? retryExtras : seen ? Core.buildExtras(noteArea.value, prioritySelect.value) : null;
    }
    catch (error) { notice = null; pageError = error.message; status.textContent = pageError; return; }
    if (!capture.metadata.movement_history && !(session.state === "error" && capture === session.capture) && globalThis.PonteHistory) {
      enriching = true; render(); status.textContent = "Lendo o histórico de tramitação…";
      try {await globalThis.PonteHistory.enrich(capture); Core.validateCapture(capture);} catch (error) {enriching=false;pageError=error.message;render();return;}
      enriching = false;
    }
    sendExtras = retryExtras = extras;
    const promise = session.run(capture);
    notice = null;
    openMessage = "";
    render();
    notice = { ...(await promise), extras };
    // Delivered: the note and priority were passed on; the controls start empty again.
    // Cleared only when the Ponte kept them; otherwise the person can copy or send again.
    if (notice.result.ok && extras && notice.result.extras_applied === true && notice.capture.document_number === extrasNumber) clearExtras();
    if (notice.result.ok) retryExtras = null;
    sendExtras = null;
    refresh();
    queueCheck(true);
  });
  const observer = new MutationObserver(mutations => {
    if (!host.isConnected) { schedule(); return; }
    if (mutations.some(mutation => mutation.target !== host && !host.contains(mutation.target) && !(mutation.type === "childList" && [...mutation.addedNodes, ...mutation.removedNodes].every(node => node === host)))) schedule();
  });
  observer.observe(document.documentElement, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["hidden", "aria-hidden", "data-document-number", "data-document-title", "data-document-text"] });
  window.addEventListener("popstate", schedule);
  window.addEventListener("hashchange", schedule);
  setInterval(() => { if (lastUrl !== location.href) { lastUrl = location.href; schedule(); } }, 1000);
  refresh();
})();
