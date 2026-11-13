(function () {
  "use strict";
  // Panel of the optional WhatsApp Web feature. Registered by the background only after the
  // person turned the feature on in the popup. It reads NOTHING by itself: no timers, no
  // observers, no listeners on the page. The page is read only inside the click handlers of
  // "Trecho selecionado", "Mensagens visíveis na tela" and "Usar o texto colado" (pasted text
  // is not read from the page at all). Nothing leaves the page before "Enviar ao Ponte".
  const Core = globalThis.PonteCapture;
  const WA = globalThis.PonteWhatsApp;
  if (!Core || !WA || !WA.allowedOrigin(location.href) || window.top !== window || globalThis.__ponteprojWhatsAppStarted) return;
  globalThis.__ponteprojWhatsAppStarted = true;

  // Bottom-left, 80 px from the left edge: the send and microphone buttons are at the bottom
  // right of the chat and the compose box is at the bottom centre; the icons of settings and
  // profile are at the very bottom left, hence the offset. The lower part of the chat list
  // has no controls, so it is the quietest place; the messages being chosen stay uncovered.
  const style = document.createElement("style");
  style.textContent = ":host{all:initial;position:fixed;left:80px;bottom:16px;z-index:2147483646;font:14px/1.45 system-ui,sans-serif;color:#172b45}*{box-sizing:border-box}section{width:min(360px,calc(100vw - 100px));max-height:calc(100vh - 40px);overflow:auto;padding:14px;border:1px solid #a9b8cb;border-radius:12px;background:#fff;box-shadow:0 6px 24px #10264033;user-select:none}h2{margin:0;font-size:15px}.head{display:flex;justify-content:space-between;align-items:center;gap:8px}button{border:0;border-radius:7px;background:#1257a6;color:#fff;padding:9px 12px;font:600 13px system-ui;cursor:pointer}button.wide{display:block;width:100%;margin-top:8px}button.quiet{background:#eef1f5;color:#172b45}button:focus-visible,summary:focus-visible,input:focus-visible,textarea:focus-visible,select:focus-visible{outline:3px solid #d99500;outline-offset:2px}button:disabled{background:#667588;color:#fff;cursor:default}button.link{display:inline;width:auto;padding:0;background:none;color:#1257a6;font:inherit;font-weight:600;text-decoration:underline}button.pill{padding:10px 18px;border-radius:999px;border:2px solid #fff;font-size:14px;box-shadow:0 4px 18px #10264066;animation:ponte-chama 1.2s ease-in-out 3}@keyframes ponte-chama{50%{box-shadow:0 0 0 8px #1257a655}}@media (prefers-reduced-motion:reduce){button.pill{animation:none}}p{margin:9px 0 0;font-size:12px;overflow-wrap:anywhere}p.error{color:#9b2c2c}p.success{color:#1d6b3a}p.warning{color:#8a5a00;font-weight:600}p.note{color:#52657a}label{display:block;margin-top:9px;font-size:12px;font-weight:600}input,textarea,select{display:block;width:100%;margin-top:3px;padding:6px 8px;border:1px solid #a9b8cb;border-radius:6px;background:#fff;color:#172b45;font:12px/1.4 system-ui,sans-serif;user-select:text}textarea{resize:vertical}input:disabled,textarea:disabled,select:disabled{background:#eef1f5;color:#52657a}ul{margin:6px 0 0;padding-left:18px;font-size:12px}li{margin-top:3px;overflow-wrap:anywhere}details{margin-top:9px;font-size:12px}summary{cursor:pointer;font-weight:600}.zone{margin-top:10px;padding:10px;border:2px dashed #a9b8cb;border-radius:8px;font-size:12px;text-align:center}.zone.over{border-color:#1257a6;background:#eef4fb}.zone button{margin-top:6px}.lines{font-size:12px;color:#52657a}.row{display:flex;gap:8px}.row button{flex:1}[hidden]{display:none!important}";
  const host = document.createElement("div");
  host.id = "ponteproj-whatsapp-assistente";
  host.setAttribute("data-ponteproj-ui", "true");
  const shadow = host.attachShadow({ mode: "closed" });

  function make(tag, props = {}, ...children) {
    const element = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (key === "text") element.textContent = value;
      else if (key in element && key !== "type") element[key] = value;
      else element.setAttribute(key, value);
    }
    if (props.type) element.setAttribute("type", props.type);
    element.append(...children);
    return element;
  }
  const button = (text, className = "") => make("button", { type: "button", textContent: text, className });
  const plural = (n, one, many) => n === 1 ? `1 ${one}` : `${n} ${many}`;
  function sizeText(bytes) { return bytes < 1048576 ? `${Math.max(1, Math.round(bytes / 1024))} KB` : `${(bytes / 1048576).toFixed(1).replace(".", ",")} MB`; }

  // ---- elements (created once; render() only changes what they show)
  const pill = button("Guardar no Ponte", "pill");
  pill.setAttribute("aria-label", "Abrir o Ponte para guardar uma conversa do WhatsApp");
  const panel = make("section", { "aria-label": "Guardar conversa do WhatsApp no Ponte" });
  const collapse = button("Recolher", "quiet");
  panel.append(make("div", { className: "head" }, make("h2", { text: "Guardar no Ponte" }), collapse));
  const intro = make("p", { className: "note", text: "Escolha as mensagens desta conversa. O Ponte só lê o que você escolher, quando você clicar." });
  const oldLine = make("p", { className: "warning" });
  const retryPing = button("Tentar de novo", "link");
  const pingLine = make("p", { className: "warning" }, make("span"), " ", retryPing);

  const chooseBox = make("div");
  const selectButton = button("Trecho selecionado", "wide");
  const selectHint = make("p", { className: "note", text: "Arraste o mouse sobre as mensagens e clique aqui." });
  const visibleButton = button("Mensagens visíveis na tela", "wide quiet");
  const pasteBox = make("details");
  const pasteArea = make("textarea", { rows: 4, placeholder: "Cole aqui as mensagens copiadas do WhatsApp (Ctrl+V)" });
  const pasteButton = button("Usar o texto colado", "wide quiet");
  pasteBox.append(make("summary", { text: "Colar mensagens copiadas" }), pasteArea, pasteButton);
  chooseBox.append(selectButton, selectHint, visibleButton, pasteBox);

  const previewBox = make("div");
  const summaryLine = make("p", { className: "identity" });
  summaryLine.style.fontWeight = "600";
  const linesList = make("ul", { className: "lines" });
  const notesList = make("ul", { className: "notes" });
  const titleLabel = make("label", { text: "Título" });
  const titleInput = make("input", { type: "text", maxLength: 500 });
titleLabel.append(titleInput);
  const chatLabel = make("label", { text: "Conversa com" });
  const chatInput = make("input", { type: "text", maxLength: 200 });
  chatLabel.append(chatInput);
  const extrasBox = make("details");
  const noteLabel = make("label", { text: "Nota para o Ponte" });
  const noteArea = make("textarea", { rows: 3, placeholder: "Ex.: responder até sexta; falar com o setor de protocolo" });
  noteLabel.append(noteArea);
  const priorityLabel = make("label", { text: "Prioridade" });
  const prioritySelect = make("select");
  for (const [value, text] of [["", "Não mudar"], ["baixa", "Baixa"], ["normal", "Normal"], ["alta", "Alta"], ["urgente", "Urgente"]]) prioritySelect.append(make("option", { value, textContent: text }));
  priorityLabel.append(prioritySelect);
  extrasBox.append(make("summary", { text: "Nota ou prioridade" }), noteLabel, priorityLabel);
  const sendButton = button("Enviar ao Ponte", "wide");
  const cancelButton = button("Cancelar", "wide quiet");
  previewBox.append(summaryLine, linesList, notesList, titleLabel, chatLabel, extrasBox, sendButton, cancelButton);

  const filesBox = make("div");
  const zone = make("div", { className: "zone" });
  const zoneText = make("div", { text: "Solte aqui arquivos baixados do WhatsApp (PDF, Word, imagens…)" });
  const pickButton = button("Escolher arquivo…", "quiet");
  const fileInput = make("input", { type: "file", multiple: true });
  fileInput.hidden = true;
  zone.append(zoneText, pickButton, fileInput);
  const filesList = make("ul", { className: "files" });
  filesBox.append(zone, filesList);

  const status = make("p", { role: "status" });
  status.setAttribute("aria-live", "polite");
  const doneBox = make("p");
  const openButton = button("Abrir no Ponte", "link");
  const againButton = button("Guardar outra conversa", "link");
  doneBox.append(openButton, " ", againButton);
  panel.append(intro, oldLine, pingLine, chooseBox, previewBox, filesBox, status, doneBox);
  shadow.append(style, pill, panel);

  // ---- state: memory of this page only, never written anywhere
  let isOpen = false;
  let pinged = false;
  let pinging = false;
  let ponteActions = null;       // null: unknown or unreachable
  let pingMessage = "";
  let view = "start";            // "start" | "preview" | "done"
  let busy = false;              // reading, or waiting for the Ponte
  let draft = null;              // { messages }
  let attachments = [];          // { name, bytes, kind: "foto" | "arquivo" }
  let previewNotes = [];
  let fileNotes = [];
  let retryRequest = null;       // exactly the request that failed; the page is not read again
  let sentExtras = null;
  let result = null;             // answer of the Ponte for the last delivery
  let message = { text: "", tone: "" };
  let recognized = true;
  let suggested = "";            // the title offered last: follows the conversation name until the person edits it

  const send = request => new Promise(resolve => {
    try { chrome.runtime.sendMessage(request, response => { if (chrome.runtime.lastError) resolve(null); else resolve(response); }); }
    catch { resolve(null); }
  });
  const say = (text, tone = "") => { message = { text, tone }; };
  const canImport = () => Array.isArray(ponteActions) && ponteActions.includes("import_record");
  const canExtras = () => Array.isArray(ponteActions) && ponteActions.includes("import_extras");
  const canOpen = () => Array.isArray(ponteActions) && ponteActions.includes("open_document");
  const totalBytes = () => attachments.reduce((sum, item) => sum + item.bytes.length, 0);

  function render() {
    if (!host.isConnected && document.body) document.body.append(host);
    pill.hidden = isOpen;
    panel.hidden = !isOpen;
    if (!isOpen) return;
    const locked = busy || Boolean(retryRequest);
    intro.hidden = view !== "start";
    oldLine.textContent = "Atualize o Ponte para guardar conversas do WhatsApp.";
    oldLine.hidden = !(Array.isArray(ponteActions) && !canImport());
    pingLine.hidden = !(pingMessage && ponteActions === null && !pinging);
    pingLine.firstChild.textContent = pingMessage;
    chooseBox.hidden = view !== "start";
    selectButton.disabled = busy;
    visibleButton.disabled = busy;
    pasteButton.disabled = busy;
    pasteBox.open = pasteBox.open || !recognized;
    previewBox.hidden = view !== "preview" || !draft;
    if (draft) {
      summaryLine.textContent = WA.summary(draft.messages, attachments.filter(item => item.kind === "foto").length);
      linesList.replaceChildren();
      const shown = draft.messages.length > 1 ? [draft.messages[0], draft.messages[draft.messages.length - 1]] : draft.messages;
      shown.forEach((item, index) => {
        const line = WA.recordText([item]).replace(/\n\s*/g, " / ");
        linesList.append(make("li", { text: `${shown.length > 1 ? (index ? "Fim" : "Início") : "Mensagem"}: ${line.length > 140 ? line.slice(0, 139) + "…" : line}` }));
      });
      notesList.replaceChildren(...previewNotes.map(text => make("li", { text })));
      notesList.hidden = !previewNotes.length;
    }
    titleInput.disabled = locked;
    chatInput.disabled = locked;
    extrasBox.hidden = !canExtras();
    noteArea.disabled = locked;
    prioritySelect.disabled = locked;
    sendButton.textContent = busy ? "Enviando ao Ponte…" : retryRequest ? "Tentar de novo" : "Enviar ao Ponte";
    sendButton.disabled = busy || !canImport();
    cancelButton.disabled = busy;
    filesBox.hidden = view === "done";
    pickButton.disabled = locked;
    filesList.replaceChildren();
    for (const [index, item] of attachments.entries()) {
      const remove = button("Tirar", "link");
      remove.setAttribute("aria-label", `Tirar ${item.name}`);
      remove.disabled = locked;
      remove.addEventListener("click", () => { attachments.splice(index, 1); render(); });
      filesList.append(make("li", {}, `${item.name} · ${sizeText(item.bytes.length)} `, remove));
    }
    filesList.hidden = !attachments.length;
    const notes = fileNotes.filter(Boolean);
    status.textContent = [message.text, ...notes].filter(Boolean).join(" ");
    status.className = message.tone;
    status.hidden = !status.textContent;
    doneBox.hidden = view !== "done";
    openButton.hidden = !(view === "done" && result && result.ok === true && canOpen() && Number.isSafeInteger(result.document_id));
    againButton.hidden = view !== "done";
  }

  chatInput.addEventListener("input", () => {
    if (draft && titleInput.value === suggested) { suggested = WA.suggestedTitle(chatInput.value, draft.messages); titleInput.value = suggested; }
  });
  // ---- the Ponte is asked once, when the person opens the panel
  async function checkPonte() {
    if (pinging) return;
    pinging = true; pingMessage = "";
    render();
    const answer = await send({ action: "ponte.ping" });
    pinging = false;
    if (answer && answer.ok === true && Array.isArray(answer.actions)) { ponteActions = answer.actions; pingMessage = ""; }
    else {
      ponteActions = null;
      pingMessage = answer && answer.error && typeof answer.error.message === "string" && answer.error.message ? answer.error.message : "O Ponte não respondeu. Abra o Ponte e tente de novo.";
    }
    render();
  }
  function openPanel() {
    isOpen = true;
    render();
    selectButton.focus();
    if (!pinged) { pinged = true; checkPonte(); }
  }
  pill.addEventListener("click", openPanel);
  // The worker opens the panel once, right after the person turned the feature on with this tab open.
  globalThis.__ponteprojWhatsAppOpen = () => { if (!isOpen) openPanel(); };
  collapse.addEventListener("click", () => { isOpen = false; render(); pill.focus(); });
  retryPing.addEventListener("click", checkPonte);

  // ---- choosing messages: the only place where the page is read
  function showDraft(messages, chat) {
    draft = { messages: messages.map(item => ({ when: item.when, author: item.author, text: item.text })) };
    suggested = WA.suggestedTitle(chat, messages);
    titleInput.value = suggested;
    chatInput.value = WA.cleanChatName(chat);
    view = "preview";
  }
  function viewportRect() { return { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight }; }
  function currentRange() {
    const selection = window.getSelection();
    return selection && selection.rangeCount > 0 && !selection.isCollapsed ? selection.getRangeAt(0) : null;
  }
  async function fetchPhoto(url) {
    if (!WA.isOwnBlobUrl(url)) throw new Error("address");
    const response = await fetch(url);
    if (!response.ok || !response.body) throw new Error("read");
    const reader = response.body.getReader();
    const chunks = [];
    let size = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > WA.MAX_FILE_BYTES) { await reader.cancel(); const error = new Error("size"); error.big = true; throw error; }
      chunks.push(value);
    }
    const data = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { data.set(chunk, offset); offset += chunk.length; }
    return data;
  }
  // The photos of the chosen messages, read while the click is being handled.
  async function readPhotos(messages) {
    const urls = [...new Set(messages.flatMap(item => item.images || []))];
    const kept = { photos: [], unsupported: 0, failed: 0, big: 0, full: 0 };
    for (const url of urls) {
      if (attachments.length + kept.photos.length >= WA.MAX_ATTACHMENTS) { kept.full++; continue; }
      let data;
      try { data = await fetchPhoto(url); }
      catch (error) { if (error.big) kept.big++; else kept.failed++; continue; }
      const type = WA.sniffImageType(data);
      if (!WA.ACCEPTED_PHOTO_TYPES.includes(type)) { kept.unsupported++; continue; }
      if (totalBytes() + kept.photos.reduce((sum, item) => sum + item.bytes.length, 0) + data.length > WA.MAX_TOTAL_BYTES) { kept.full++; continue; }
      const name = WA.uniqueName(`foto-${kept.photos.length + 1}.${type}`, [...attachments, ...kept.photos].map(item => item.name));
      if (!name) { kept.full++; continue; }
      kept.photos.push({ name, bytes: data, kind: "foto" });
    }
    return kept;
  }
  function photoNotes(kept) {
    const notes = [];
    const lines = [[kept.unsupported, "em formato não aceito ficou de fora", "em formato não aceito ficaram de fora"], [kept.failed, "não pôde ser lida", "não puderam ser lidas"], [kept.big, "passa de 8 MB e ficou de fora", "passam de 8 MB e ficaram de fora"], [kept.full, "ficou de fora: o limite é de 10 arquivos e 16 MB", "ficaram de fora: o limite é de 10 arquivos e 16 MB"]];
    for (const [count, one, many] of lines) if (count) notes.push(count === 1 ? `1 foto ${one}.` : `${count} fotos ${many}.`);
    return notes;
  }
  async function choose(kind) {
    if (busy) return;
    say(""); fileNotes = []; result = null; retryRequest = null;
    let found = null;
    try { found = kind === "selection" ? WA.readSelection(document, currentRange()) : WA.readVisible(document, viewportRect()); } catch { found = null; }
    if (!found) {
      recognized = false;
      say("O Ponte não reconheceu esta página do WhatsApp Web. Selecione as mensagens no WhatsApp, copie (Ctrl+C) e cole aqui.", "warning");
      render(); pasteArea.focus();
      return;
    }
    recognized = true;
    if (!found.messages.length) { say(kind === "selection" ? "Não encontrei mensagens nesse trecho. Arraste o mouse sobre as mensagens e tente de novo." : "Não há mensagens visíveis na tela.", "warning"); render(); return; }
    if (found.messages.length > WA.MAX_MESSAGES) { say(`Há ${found.messages.length} mensagens; o limite é ${WA.MAX_MESSAGES}. Escolha um trecho menor.`, "error"); render(); return; }
    busy = true;
    if (found.messages.some(item => item.images && item.images.length)) { say("Lendo as fotos…"); render(); }
    attachments = attachments.filter(item => item.kind !== "foto");
    let kept;
    try { kept = await readPhotos(found.messages); } finally { busy = false; }
    attachments.push(...kept.photos);
    previewNotes = photoNotes(kept);
    if (found.skipped) previewNotes.push(found.skipped === 1 ? "1 mensagem de outro tipo ficou de fora." : `${found.skipped} mensagens de outro tipo ficaram de fora.`);
    if (found.undatedPhotos) previewNotes.push(found.undatedPhotos === 1 ? "1 foto entrou sem data." : `${found.undatedPhotos} fotos entraram sem data.`);
    if (found.undatedFiles) previewNotes.push(found.undatedFiles === 1 ? "1 arquivo entrou sem data." : `${found.undatedFiles} arquivos entraram sem data.`);
    if (found.bare) previewNotes.push("Fotos e arquivos sem legenda entram sem autor.");
    showDraft(found.messages, found.chat);
    say("");
    render();
  }
  // The click handlers read the page; nothing else does.
  selectButton.addEventListener("mousedown", event => event.preventDefault());
  selectButton.addEventListener("click", () => { choose("selection"); });
  visibleButton.addEventListener("click", () => { choose("visible"); });
  pasteButton.addEventListener("click", () => {
    if (busy) return;
    say(""); fileNotes = []; result = null; retryRequest = null;
    const text = pasteArea.value;
    try {
      if (text.length > 4 * 1024 * 1024) throw new WA.WhatsAppError("TOO_BIG", "O texto colado é grande demais. Cole um trecho menor.");
      const messages = WA.parseCopied(text);
      if (messages.length > WA.MAX_MESSAGES) throw new WA.WhatsAppError("TOO_MANY", `Há ${messages.length} mensagens; o limite é ${WA.MAX_MESSAGES}. Cole um trecho menor.`);
      attachments = attachments.filter(item => item.kind !== "foto");
      previewNotes = [];
      showDraft(messages, "");
      recognized = true;
    } catch (error) { say(error instanceof WA.WhatsAppError ? error.message : "Não foi possível ler o texto colado.", "error"); }
    render();
  });

  // ---- files: dropped on the panel or chosen
  async function addFiles(files) {
    fileNotes = [];
    for (const file of Array.from(files || []).slice(0, 50)) {
      const label = WA.sanitizeFileName(file.name);
      if (attachments.length >= WA.MAX_ATTACHMENTS) { fileNotes.push(`${label} ficou de fora: o limite é de 10 arquivos.`); continue; }
      if (file.size > WA.MAX_FILE_BYTES) { fileNotes.push(`${label} passa de 8 MB e ficou de fora.`); continue; }
      if (!WA.fileTypeAccepted(label)) { fileNotes.push(`${label} não é de um tipo aceito pelo Ponte e ficou de fora.`); continue; }
      if (file.size === 0) { fileNotes.push(`${label} está vazio e ficou de fora.`); continue; }
      if (totalBytes() + file.size > WA.MAX_TOTAL_BYTES) { fileNotes.push(`${label} ficou de fora: os arquivos juntos passam de 16 MB.`); continue; }
      let bytes;
      try { bytes = new Uint8Array(await file.arrayBuffer()); } catch { fileNotes.push(`${label} não pôde ser lido.`); continue; }
      if (bytes.length > WA.MAX_FILE_BYTES) { fileNotes.push(`${label} passa de 8 MB e ficou de fora.`); continue; }
      const name = WA.uniqueName(label, attachments.map(item => item.name));
      if (!name) { fileNotes.push(`${label} ficou de fora: já há arquivos demais com esse nome.`); continue; }
      attachments.push({ name, bytes, kind: "arquivo" });
    }
    render();
  }
  pickButton.addEventListener("click", () => fileInput.click());
  fileInput.addEventListener("change", async () => {
    const files = Array.from(fileInput.files || []);
    fileInput.value = "";
    if (!busy && !retryRequest) await addFiles(files);
  });
  // WhatsApp has its own drop target for sending files: events from this panel never reach it.
  const isFileDrag = event => Array.from(event.dataTransfer?.types || []).includes("Files");
  for (const type of ["dragenter", "dragover", "dragleave", "drop"]) {
    host.addEventListener(type, event => {
      event.stopPropagation();
      if (!isOpen || !isFileDrag(event)) return;
      event.preventDefault();
      if (type === "dragenter" || type === "dragover") zone.classList.add("over");
      else zone.classList.remove("over");
      if (type === "drop" && !busy && !retryRequest && view !== "done") addFiles(event.dataTransfer.files);
    });
  }
  // Typing here must not reach the shortcuts of WhatsApp.
  for (const type of ["keydown", "keyup", "keypress", "paste", "copy", "cut"]) host.addEventListener(type, event => event.stopPropagation());

  // ---- sending
  function forget() {
    draft = null; attachments = []; previewNotes = []; fileNotes = []; retryRequest = null; sentExtras = null;
    titleInput.value = ""; chatInput.value = ""; pasteArea.value = ""; noteArea.value = ""; prioritySelect.value = ""; extrasBox.open = false;
  }
  function resultText(response, files) {
    let text = response.outcome === "already" ? "Esta conversa já estava no Ponte." : "Guardado no Ponte.";
    // The Ponte adds no attachment to a record that already exists.
    if (response.outcome === "already" && files) text += " Os arquivos não foram juntados de novo; para juntá-los, abra o registro no Ponte e use Anexos.";
    if (response.outcome !== "already" && response.ui_opened === false) text += " Abra o Ponte para ver o registro.";
    const extras = Core.extrasNotice(response, sentExtras);
    return extras ? `${text} ${extras}` : text;
  }
  async function submit() {
    if (busy || !draft) return;
    let request = retryRequest;
    if (!request) {
      try {
        const record = WA.buildRecord({ title: titleInput.value, origin: chatInput.value, messages: draft.messages });
        const list = attachments.map(item => ({ name: item.name, data_base64: WA.toBase64(item.bytes) }));
        WA.validateAttachments(list);
        request = { action: "ponte.import_record", record, attachments: list };
        sentExtras = canExtras() && extrasBox.open ? Core.buildExtras(noteArea.value, prioritySelect.value) : null;
        if (sentExtras) request.extras = sentExtras;
      } catch (error) { say(error.message || "Não foi possível preparar a conversa.", "error"); render(); return; }
    }
    busy = true; say(""); render();
    let response = await send(request);
    if (!response) response = Core.errorResult("EXTENSION_RELOADED", "A extensão não respondeu. Recarregue a página do WhatsApp Web e escolha as mensagens de novo.");
    busy = false;
    if (response.ok === true) {
      result = response;
      say(resultText(response, Array.isArray(request.attachments) && request.attachments.length > 0), "success");
      forget();
      view = "done";
    } else {
      // A refusal for a reason the person can fix (a file, the record, the note) leaves everything editable;
      // only a transport failure keeps the locked retry of exactly the same request.
      const code = response.error && typeof response.error.code === "string" ? response.error.code : "";
      retryRequest = /^(invalid_|record_invalid$|attachment_invalid$|extras_invalid$|payload_too_large$|ponte_old$)/i.test(code) ? null : request;
      say(response.error && typeof response.error.message === "string" ? response.error.message : "O Ponte não respondeu. Abra o Ponte e tente de novo.", "error");
    }
    render();
  }
  sendButton.addEventListener("click", submit);
  cancelButton.addEventListener("click", () => { forget(); view = "start"; say(""); recognized = true; render(); });
  againButton.addEventListener("click", () => { forget(); result = null; view = "start"; say(""); render(); });
  openButton.addEventListener("click", async () => {
    if (!result || !Number.isSafeInteger(result.document_id)) return;
    openButton.disabled = true;
    const response = await send({ action: "ponte.open", document_id: result.document_id });
    openButton.disabled = false;
    if (response && response.ok === true) say(response.ui_opened === false ? "Abra o Ponte para ver o registro." : message.text, response.ui_opened === false ? "warning" : "success");
    else say(response && response.error && typeof response.error.message === "string" && response.error.message ? response.error.message : "O Ponte não respondeu. Abra o Ponte e tente de novo.", "error");
    render();
  });
  render();
})();
