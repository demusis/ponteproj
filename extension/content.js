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
  style.textContent = ":host{all:initial;position:fixed;right:18px;bottom:18px;z-index:2147483646;font:14px/1.45 system-ui,sans-serif;color:#172b45}section{width:min(340px,calc(100vw - 40px));padding:14px;border:1px solid #a9b8cb;border-radius:12px;background:#fff;box-shadow:0 6px 24px #10264033}section.idle{width:min(260px,calc(100vw - 40px));padding:10px 12px;opacity:.9}button{width:100%;border:0;border-radius:7px;background:#1257a6;color:#fff;padding:11px 12px;font:600 14px system-ui;cursor:pointer}button:focus-visible,a:focus-visible{outline:3px solid #d99500;outline-offset:3px}button:disabled{background:#667588;cursor:default}p{margin:9px 0 0;overflow-wrap:anywhere;font-size:12px}.identity{font-weight:600}.idle .identity{margin-top:0;font-weight:500;color:#4c5d72}.idle button,.idle p.status,.idle details{display:none}p.error{color:#9b2c2c}p.success{color:#1d6b3a}a{color:#1257a6}details{margin-top:7px;font-size:11px}summary{cursor:pointer}ul{padding-left:18px}li{margin-top:4px}";
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
  panel.append(button, identity, status, details);
  shadow.append(style, panel);
  let enriching = false;
  let snapshot = null;
  let pageError = "";
  let notice = null;
  let timer = null;
  let lastUrl = location.href;
  const session = new Core.ImportSession(capture => new Promise(resolve => {
    try {
      chrome.runtime.sendMessage({ action: "ponte.import_capture", capture }, response => {
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
    button.disabled = !snapshot || enriching || session.state === "sending";
    button.textContent = session.state === "sending" ? "Aguardando o Ponte…" : session.state === "error" && same ? "Repetir importação" : "Importar para o assistente";
    identity.textContent = snapshot ? `${snapshot.document_number} — ${snapshot.title}` : (pageError && !/ficha|documento individual|lista|pesquisa/i.test(pageError) ? pageError : "Abra um documento do SIGADOC para importar");
    let message = pageError || "Importa o texto, os PDFs e o histórico deste documento para o Ponte.";
    let tone = pageError ? "error" : "";
    if (session.state === "sending") { message = `Enviando ${session.capture.document_number}… aguarde a confirmação do Ponte.`; tone = ""; }
    else if (notice) {
      const prefix = same ? "" : `${notice.capture.document_number}: `;
      if (notice.result.ok) {
        message = prefix + (notice.result.duplicate ? "Este documento já estava importado; nada mudou." : "Importado.") + (notice.result.ui_opened === false ? " Abra o Ponte para ver o documento." : " O Ponte abriu o documento.");
        const notes = Array.isArray(notice.result.file_notes) ? notice.result.file_notes : [];
        if (notes.length) message += ` ${notes.length} arquivo(s) não baixado(s): ${notes.join("; ")}`;
        tone = "success";
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
    render();
  }
  function schedule() { if (timer === null) timer = setTimeout(refresh, 180); }
  button.addEventListener("click", async () => {
    // Re-read immediately: the DOM may have changed since the observer ran.
    refresh();
    if (!snapshot || enriching || session.state === "sending") return;
    let capture;
    try { capture = session.state === "error" && session.capture && Core.identity(snapshot) === Core.identity(session.capture) ? session.capture : Core.buildCapture(snapshot); }
    catch (error) { notice = null; pageError = error.message; status.textContent = pageError; return; }
    if (!capture.metadata.movement_history && !(session.state === "error" && capture === session.capture) && globalThis.PonteHistory) {
      enriching = true; render(); status.textContent = "Lendo o histórico de tramitação…";
      try {await globalThis.PonteHistory.enrich(capture); Core.validateCapture(capture);} catch (error) {enriching=false;pageError=error.message;render();return;}
      enriching = false;
    }
    const promise = session.run(capture);
    notice = null;
    render();
    notice = await promise;
    refresh();
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
