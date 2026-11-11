(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.PonteCapture = api;
})(globalThis, function () {
  "use strict";
  const ORIGIN = "https://www.sigadoc.mt.gov.br";
  const HOST = "com.ponteproj.assistente";
  const ADAPTER = "sigadoc-structured-heuristic/1.6.1";
  const SIGA_DETAIL_PATH = "/sigaex/app/expediente/doc/exibir";
  const DOCUMENT_QUERY_KEYS = ["sigla", "numero", "numerodocumento", "document_number", "documento"];
  const MAX_TEXT_BYTES = 500 * 1024;
  const MAX_PAYLOAD_BYTES = 1024 * 1024;
  const META_KEYS = ["document_type", "classification", "document_date", "origin", "recipient", "interested_party", "sender", "sent_at", "received_at", "process_number", "registered_by", "document_model", "medium", "via_status", "signature_status", "signature_person", "current_holder", "access_level", "access_people", "via_code", "movement_source", "movement_history"];
  const NUMBER_LABELS = new Set(["numero", "numero do documento", "numero de documento", "documento", "processo", "numero do processo", "expediente", "sigla"]);
  const TITLE_LABELS = new Set(["assunto", "titulo", "descricao", "descricao do documento"]);
  const META_LABELS = { "tipo": "document_type", "tipo de documento": "document_type", "tipo do documento": "document_type", "classificacao": "classification", "data": "document_date", "data do documento": "document_date", "origem": "origin", "de": "origin", "para": "recipient", "destinatario": "recipient", "interessado": "interested_party", "remetente": "sender", "enviado por": "sender", "data de envio": "sent_at", "data de recebimento": "received_at", "recebido em": "received_at", "processo vinculado": "process_number", "numero do processo": "process_number", "processo": "process_number", "cadastrante": "registered_by", "especie": "document_type", "modelo": "document_model", "suporte": "medium" };
  const NUMBER_RE = /^(?:(?:[A-Z][A-Z0-9]{1,19}[-/]){1,4}(?:19|20)\d{2}[-/]\d{1,12}(?:[-/]\d{1,8})?|\d{1,12}[/.-](?:19|20)\d{2}|(?:19|20)\d{2}[/.-]\d{1,12}|\d{7}-\d{2}\.(?:19|20)\d{2}\.\d\.\d{2}\.\d{4})$/;
  const UNRECOGNIZED_TEXT = "O Ponte não reconheceu esta página do SIGADOC. O site pode ter mudado; avise quem cuida do Ponte.";
  class CaptureError extends Error {
    constructor(code, message) { super(message); this.name = "CaptureError"; this.code = code; }
  }
  function fail(code, message) { throw new CaptureError(code, message); }
  function bytes(value) { return new TextEncoder().encode(value).length; }
  function plain(value) { return !!value && typeof value === "object" && !Array.isArray(value) && [Object.prototype, null].includes(Object.getPrototypeOf(value)); }
  function exactKeys(value, keys) { return plain(value) && Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key)); }
  function normalize(value) { return String(value || "").replace(/\u00a0/g, " ").replace(/[ \t]+/g, " ").replace(/ *\n */g, "\n").replace(/\n{3,}/g, "\n\n").trim(); }
  function label(value) { return normalize(value).normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase().replace(/\s*(?:n[º°o.]|:)\s*$/g, "").replace(/[.:º°]+$/g, "").trim(); }
  function documentNumber(value) {
    const number = normalize(value).replace(/^(?:n[úu]mero(?: d[oe])? (?:documento|processo)|documento|processo|expediente|sigla)\s*(?:n[º°o.]\s*)?[:#-]?\s*/i, "").replace(/^n[º°o.]\s*/i, "").replace(/\s*([/.-])\s*/g, "$1").toUpperCase();
    return NUMBER_RE.test(number) ? number : null;
  }
  function documentQueryNumber(value) {
    const direct = documentNumber(value);
    if (direct) return direct;
    // SIGA vias use a single uppercase letter after the canonical document number.
    const match = /^(.+)-([A-Z])$/.exec(normalize(value));
    return match && documentNumber(match[1]) === match[1] ? match[1] + "-" + match[2] : null;
  }
  function allowedOrigin(value) {
    try { const url = new URL(value); return url.origin === ORIGIN && !url.username && !url.password; } catch { return false; }
  }
  function cleanUrl(value, base = ORIGIN) {
    let url;
    try { url = new URL(value, base); } catch { return null; }
    if (!allowedOrigin(url.href)) return null;
    const safe = new URL(url.origin + url.pathname.replace(/;[^/]*(?=\/|$)/g, ""));
    if (url.pathname === "/sigaex/app/arquivo/exibir") {
      const sigla = url.searchParams.get("sigla");
      const arquivo = url.searchParams.get("arquivo");
      if (/^[A-Z]{2,24}(?:19|20)\d{2}\d{1,12}[A-Z]?$/.test(sigla || "") && arquivo === sigla + ".pdf") {
        safe.searchParams.set("arquivo", arquivo);
        safe.searchParams.set("sigla", sigla);
        return safe.href;
      }
      return null;
    }
    // Only explicitly formatted document numbers can survive a query string.
    // Session IDs, opaque IDs, signatures, tokens and all unknown keys are removed.
    for (const [key, value] of url.searchParams) {
      const number = key.toLowerCase() === "sigla" ? documentQueryNumber(value) : documentNumber(value);
      if (DOCUMENT_QUERY_KEYS.includes(key.toLowerCase()) && number) safe.searchParams.append(key.toLowerCase(), number);
    }
    return safe.href;
  }
  function scrubInlineUrls(value) {
    return String(value).replace(/https?:\/\/[^\s<>"'\])]+/gi, raw => {
      try { const url = new URL(raw); url.username = ""; url.password = ""; url.search = ""; url.hash = ""; url.pathname = url.pathname.replace(/;[^/]*(?=\/|$)/g, ""); return url.href; } catch { return "[URL omitida]"; }
    });
  }
  function excluded(node) {
    if (!node || node.nodeType !== 1) return false;
    if (["INPUT", "TEXTAREA", "SELECT", "OPTION", "BUTTON", "SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "NAV", "HEADER", "FOOTER", "ASIDE", "MENU", "DIALOG", "SVG", "IFRAME", "OBJECT", "EMBED"].includes(node.tagName)) return true;
    if (node.hasAttribute("hidden") || node.hasAttribute("inert") || node.getAttribute("aria-hidden") === "true") return true;
    if (["navigation", "menu", "menubar", "toolbar"].includes(node.getAttribute("role"))) return true;
    if (/(?:password|senha|csrf|xsrf|token|session|sessao|cookie|credential|authorization|toolbar|(?:^|[-_ ])menu(?:[-_ ]|$))/.test(`${node.id || ""} ${node.className || ""}`.toLowerCase())) return true;
    if (/display\s*:\s*none|visibility\s*:\s*(?:hidden|collapse)/i.test(node.getAttribute("style") || "")) return true;
    const view = node.ownerDocument && node.ownerDocument.defaultView;
    if (view && typeof view.getComputedStyle === "function") {
      const style = view.getComputedStyle(node);
      if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse" || style.opacity === "0") return true;
    }
    return false;
  }
  function eligible(node, root) {
    // A detail container can itself be inside a hidden ancestor or a menu.
    for (let current = node; current; current = current.parentElement) if (excluded(current)) return false;
    return true;
  }
  function safeText(node) {
    const parts = [];
    function walk(current) {
      if (current.nodeType === 3) { parts.push(current.nodeValue.replace(/\s+/g, " ")); return; }
      if (current.nodeType !== 1 || excluded(current)) return;
      if (current.tagName === "TABLE") {
        const rows = [...current.querySelectorAll("tr")].filter(row => row.closest("table") === current && eligible(row, current)).map(row => [...row.children].filter(cell => ["TD","TH"].includes(cell.tagName) && !excluded(cell)).map(cell => safeText(cell).replace(/\s+/g," ").trim()));
        const width = [];
        for (const row of rows) row.forEach((cell,i) => { width[i] = Math.max(width[i] || 0, cell.length); });
        parts.push("\n\n" + rows.map(row => "| " + row.map((cell,i) => cell.padEnd(width[i])).join(" | ") + " |").join("\n") + "\n\n");
        return;
      }
      const block = /^(?:P|DIV|SECTION|ARTICLE|TR|TD|TH|DT|DD|H[1-6]|LI|BR)$/.test(current.tagName);
      if (block) parts.push(current.tagName === "BR" ? "\n" : "\n\n");
      if (current.tagName === "LI") parts.push("• ");
      for (const child of current.childNodes) walk(child);
      if (block) parts.push("\n");
    }
    walk(node);
    return scrubInlineUrls(parts.join("")).replace(/\u00a0/g," ").replace(/ *\n */g,"\n").replace(/\n{3,}/g,"\n\n").trim();
  }
  function records(root) {
    const result = [];
    function add(keyNode, valueNode) {
      if (!keyNode || !valueNode || !eligible(keyNode, root) || !eligible(valueNode, root)) return;
      const key = label(safeText(keyNode));
      if (NUMBER_LABELS.has(key) || TITLE_LABELS.has(key) || Object.hasOwn(META_LABELS, key)) result.push({ key, value: safeText(valueNode) });
    }
    for (const dt of root.querySelectorAll("dt")) if (dt.nextElementSibling?.tagName === "DD") add(dt, dt.nextElementSibling);
    for (const row of root.querySelectorAll("tr")) {
      const cells = Array.from(row.children).filter(child => ["TH", "TD"].includes(child.tagName));
      // Rows from result tables have several columns; they are not document fields.
      if (cells.length === 2) add(cells[0], cells[1]);
    }
    for (const field of root.querySelectorAll(".field, .campo, [data-document-field]")) add(field.querySelector(".label, .rotulo, [data-field-label]"), field.querySelector(".value, .valor, [data-field-value]"));
    return result;
  }
  function unique(values) { return Array.from(new Set(values.filter(Boolean))); }
  function validateUrlIdentity(sourceUrl, number) {
    const url = new URL(sourceUrl);
    for (const [key, value] of url.searchParams) {
      if (!DOCUMENT_QUERY_KEYS.includes(key.toLowerCase())) continue;
      const parsed = key.toLowerCase() === "sigla" ? documentQueryNumber(value) : documentNumber(value);
      if (key.toLowerCase() === "sigla" && !parsed) fail("DOCUMENT_IDENTITY", "A sigla na URL não possui um formato documental reconhecido.");
      if (!parsed || parsed === number) continue;
      const via = /^(.+)-([A-Z])$/.exec(parsed);
      if (key.toLowerCase() === "sigla" && url.pathname === SIGA_DETAIL_PATH && via && via[1] === number) continue;
      fail("DOCUMENT_IDENTITY", "O número na URL diverge da ficha visível. Aguarde a página terminar de abrir e tente novamente.");
    }
  }
  function paragraphValue(paragraph, expectedLabel) {
    if (!paragraph || !eligible(paragraph)) return null;
    const heading = paragraph.firstElementChild;
    const expectedHeading = expectedLabel === "descricao" ? "Descrição:" : "Classificação:";
    if (!heading || heading.tagName !== "B" || safeText(heading) !== expectedHeading) return null;
    // The inspected structure starts with the exact bold label, followed by text.
    for (let node = paragraph.firstChild; node && node !== heading; node = node.nextSibling) if (normalize(node.textContent)) return null;
    const text = safeText(paragraph);
    const prefix = safeText(heading);
    return text.startsWith(prefix) ? normalize(text.slice(prefix.length)) : null;
  }
  function siga112Metadata(root, sourceUrl) {
    if (new URL(sourceUrl).pathname !== SIGA_DETAIL_PATH) return null;
    const descriptions = Array.from(root.querySelectorAll("p#descricao")).filter(node => eligible(node));
    if (descriptions.length !== 1) return null;
    const title = paragraphValue(descriptions[0], "descricao");
    if (!title) return null;
    const numbers = unique(Array.from(root.querySelectorAll("h2")).filter(node => eligible(node)).map(node => {
      const text = safeText(node);
      const number = documentNumber(text);
      return number && text === number ? number : null;
    }));
    if (numbers.length !== 1) fail("DOCUMENT_IDENTITY", "A página ainda não terminou de carregar ou mostra mais de um documento. Aguarde e tente de novo.");
    const classifications = unique(Array.from(root.querySelectorAll("p")).map(node => paragraphValue(node, "classificacao")));
    if (classifications.length > 1) fail("DOCUMENT_METADATA", "A página mostra classificações diferentes para o mesmo documento. Confira no SIGADOC antes de importar.");
    return { number: numbers[0], title, classification: classifications[0] || "" };
  }
  function extractDocument(doc, sourceUrl) {
    if (!allowedOrigin(sourceUrl)) fail("UNSUPPORTED_ORIGIN", "Use a extensão somente em https://www.sigadoc.mt.gov.br/.");
    if (doc.querySelector('input[type="password"]') || /\/(?:login|autenticar|autenticacao)(?:[/.?]|$)/i.test(new URL(sourceUrl).pathname)) fail("LOGIN_PAGE", "Entre no SIGADOC e abra a ficha de um documento. A extensão não lê senhas.");
    const detailSelector = "[data-document-detail], #document-detail, #documento-detalhe, #documento, #expediente, .document-detail, .documento-detalhe";
    const explicitRoot = doc.querySelector(detailSelector);
    const root = explicitRoot || doc.querySelector("main, [role='main'], article") || doc.body;
    if (!root || !eligible(root, root)) fail("UNRECOGNIZED_PAGE", "Esta página não é a ficha de um documento. Abra o documento pelo SIGADOC e tente de novo.");
    if (!explicitRoot && /\/(?:listar?|lista|pesquisar?|buscar?|consulta)(?:[/.?]|$)/i.test(new URL(sourceUrl).pathname)) fail("LIST_PAGE", "Abra um documento individual; páginas de lista ou pesquisa não são importadas.");
    if (!explicitRoot && (root.querySelector("[data-document-list], .document-list, #listaDocumentos") || Array.from(root.querySelectorAll("h1, h2")).some(node => /^(?:resultados?(?: da pesquisa)?|lista de documentos|pesquisa de documentos)$/i.test(safeText(node))))) fail("LIST_PAGE", "Abra um documento individual; uma lista de resultados não pode ser importada.");
    const fields = records(root);
    const siga112 = siga112Metadata(root, sourceUrl);
    const sourceSiglas = Array.from(new URL(sourceUrl).searchParams).filter(([key]) => key.toLowerCase() === "sigla").map(([, value]) => value);
    if (!siga112 && sourceSiglas.some(value => !documentNumber(value) && documentQueryNumber(value))) fail("DOCUMENT_IDENTITY", "A página ainda não terminou de carregar ou está num formato que a extensão não reconhece. Aguarde e tente de novo.");
    const numberNodes = Array.from(root.querySelectorAll("[data-document-number]" )).filter(node => eligible(node, root));
    const numbers = unique([siga112?.number, ...numberNodes.map(node => documentNumber(node.getAttribute("data-document-number") || safeText(node))), ...fields.filter(field => NUMBER_LABELS.has(field.key) && !(siga112 && field.key.includes("processo"))).map(field => documentNumber(field.value))]);
    if (!numbers.length && new URL(sourceUrl).pathname === SIGA_DETAIL_PATH) fail("PAGE_NOT_RECOGNIZED", UNRECOGNIZED_TEXT);
    if (numbers.length !== 1) fail("DOCUMENT_IDENTITY", numbers.length ? "A página mostra mais de um número de documento. Abra a ficha de um único documento." : "Não encontrei o número oficial do documento nesta página. Abra a ficha do documento no SIGADOC.");
    validateUrlIdentity(sourceUrl, numbers[0]);
    const titles = unique([siga112?.title, ...Array.from(root.querySelectorAll("[data-document-title]")).filter(node => eligible(node, root)).map(node => normalize(scrubInlineUrls(node.getAttribute("data-document-title") || safeText(node)))), ...fields.filter(field => TITLE_LABELS.has(field.key)).map(field => field.value)]);
    const title = titles[0];
    if (!title || title.length < 3 || title.length > 500 || title === numbers[0] || /^(?:login|pesquisa|consulta|resultados?|lista de documentos)$/i.test(title)) fail("DOCUMENT_TITLE", "Não foi possível identificar o assunto do documento com segurança. Confira a ficha no SIGADOC.");
    if (titles.some(value => value !== title)) fail("DOCUMENT_TITLE", "O assunto aparece de formas diferentes nesta página. Confira no SIGADOC antes de importar.");
    const recognized = ["Número oficial em campo explícito", "Assunto/título em campo explícito"];
    const pending = ["Autenticidade, assinatura e permissões não são verificadas: o Ponte guarda o que o SIGADOC mostra"];
    if (!siga112) pending.unshift("Adaptador heurístico: estruturas do SIGADOC além da ficha já inspecionada ainda não foram validadas em páginas reais");
    if (siga112) recognized.push("Estrutura de metadados SIGA v11.2 inspecionada: H2 oficial e p#descricao com rótulo explícito");
    const metadata = {};
    if (siga112?.classification) { metadata.classification = siga112.classification; recognized.push("Classificação em parágrafo com rótulo explícito"); }
    for (const field of fields) if (Object.hasOwn(META_LABELS, field.key) && field.value) metadata[META_LABELS[field.key]] = field.value;
    if (siga112) {
      for (const p of root.querySelectorAll('#collapseDocDetalhes p')) {
        if (!eligible(p)) continue;
        const heading = p.firstElementChild;
        if (!heading || !['B','STRONG'].includes(heading.tagName)) continue;
        const key = META_LABELS[label(safeText(heading).replace(/[:\s]+$/g,''))];
        const value = normalize(safeText(p).slice(safeText(heading).length)).replace(/^[:\s]+/,'');
        if (key && value) metadata[key] = value;
      }
      const headings = unique([...root.querySelectorAll('h3')].filter(n=>eligible(n)).map(safeText).filter(t=>/^\d+ª?\s*Via\b/i.test(t)));
      if (headings.length === 1) metadata.via_status = headings[0];
      const via = new URL(sourceUrl).searchParams.get('sigla');
      if (via) metadata.via_code = via;
      for (const row of root.querySelectorAll('#collapseOutrosMob tr')) {
        if (!eligible(row)) continue;
        const cells = [...row.children].filter(n=>['TD','TH'].includes(n.tagName));
        if (cells.length < 3) continue;
        const scope = normalize(safeText(cells[0])), state = normalize(safeText(cells[1]));
        const person = normalize(cells[2].querySelector('span[title]')?.getAttribute('title') || safeText(cells[2]));
        if (scope === 'Geral' && /assinado/i.test(state)) { metadata.signature_status = state; if (person) metadata.signature_person = person; }
        if (via?.endsWith('-'+scope) && scope.length === 1) {
          if (person) metadata.current_holder = person;
          if (!metadata.via_status && state) metadata.via_status = `Via ${scope} — ${state}`;
        }
      }
      const access = root.querySelector('#collapseNivelAcesso');
      if (access && eligible(access)) {
        const parts = [];
        function collectLevel(node) {
          if (node.nodeType === 3) { parts.push(node.nodeValue); return; }
          if (node.nodeType !== 1 || ['UL','OL'].includes(node.tagName) || excluded(node)) return;
          for (const child of node.childNodes) collectLevel(child);
        }
        collectLevel(access);
        const value = normalize(parts.join(' '));
        if (value) metadata.access_level = value;
        const people = unique([...access.querySelectorAll('ul li, ol li')].filter(n=>eligible(n)).map(n=>normalize(safeText(n).replace(/^•\s*/,''))).filter(Boolean));
        if (people.length) {
          if (people.every(code=>/^[A-Z][A-Z0-9]{1,31}$/.test(code) && /\d/.test(code)) && people.join('; ').length <= 4000) {
            metadata.access_people = people.join('; ');
            recognized.push('Pessoas/códigos explicitamente listados no nível de acesso');
          } else pending.push('Lista do nível de acesso não importada: formato não reconhecido ou limite excedido; confira na origem');
        }
        if (value) recognized.push('Nível de acesso visível, incluindo rótulo em negrito ou aninhado');
      }
      recognized.push('Proveniência e situação da ficha: rótulos da área de detalhes, via e marcas visíveis');
      if (!metadata.sender || !metadata.received_at) pending.push('Remetente da última tramitação e data de recebimento não identificados nesta ficha; De/autor e data documental não substituem esses dados');
    }
    if (Object.keys(metadata).length) recognized.push("Metadados textuais em campos reconhecidos");
    const textSelector = "[data-document-text], [data-role='document-content'], #conteudoDocumento, #conteudo-documento, #textoDocumento, #texto-documento, #texto, #divTexto, .document-text, .documento-conteudo, .texto-documento, .conteudo-documento, article.document-content, .document-content";
    const textNodes = Array.from(root.querySelectorAll(textSelector)).filter(node => eligible(node, root));
    if (siga112) {
      const cards = Array.from(root.querySelectorAll(".card-sidebar.card.border-alert.bg-white > .card-body"))
        .filter(node => eligible(node, root) && !node.closest(".gt-sidebar"));
      if (cards.length === 1) textNodes.push(cards[0]);
      else if (cards.length > 1) pending.push("Mais de uma área de conteúdo SIGA encontrada; confira o texto na origem");
    }
    const outerTextNodes = textNodes.filter(node => !textNodes.some(other => other !== node && other.contains(node)));
    const text = outerTextNodes.map(safeText).filter(Boolean).join("\n\n");
    if (text) recognized.push("Texto visível em área documental reconhecida");
    else pending.push("Texto documental não localizado em área reconhecida; corpo geral da página não foi copiado");
    if (root.querySelector("iframe, object, embed")) pending.push("Conteúdo em iframe, PDF ou objeto incorporado não foi extraído");
    const references = [];
    if (siga112) {
      const canonical = numbers[0].replace(/[-/]/g, "");
      for (const button of root.querySelectorAll('[id="ver-impress-227-o"][onclick]')) {
        const match = /^\s*(?:javascript:)?popitup\('([^'\r\n]+)'\);?\s*$/.exec(button.getAttribute("onclick") || "");
        if (!match) continue;
        const printUrl = cleanUrl(match[1], sourceUrl);
        if (!printUrl) continue;
        const sigla = new URL(printUrl).searchParams.get("sigla");
        if (sigla === canonical || (sigla?.startsWith(canonical) && /^[A-Z]$/.test(sigla.slice(canonical.length)))) {
          references.push({label:"Versão de impressão (PDF)", url:printUrl, kind:"original"});
          break;
        }
      }
      const auxiliary = root.querySelector("#collapseArqAuxiliares");
      if (!auxiliary || !auxiliary.querySelector("a[href]")) recognized.push("Nenhum link de arquivo auxiliar listado nesta ficha; Incluir Arquivo é uma ação de criação");
    }
    const seen = new Set();
    let stripped = cleanUrl(sourceUrl) !== sourceUrl;
    for (const anchor of root.querySelectorAll("a[href]")) {
      if (!eligible(anchor, root)) continue;
      const text = safeText(anchor);
      if (!text || text.length > 250) continue;
      const context = label(text + " " + (anchor.closest("[data-reference-kind]")?.getAttribute("data-reference-kind") || "") + " " + (anchor.parentElement?.getAttribute("aria-label") || "") + " " + (anchor.closest("section")?.querySelector("h2, h3") ? safeText(anchor.closest("section").querySelector("h2, h3")) : ""));
      // SIGA event tables: "Juntada | Juntado ao documento: ORG-DIC-2026/97546-A",
      // where the sigla links to a via (-A) of the parent document.
      const sigla = documentQueryNumber(text);
      let display = text;
      let kind;
      const cell = anchor.closest("td, th") || anchor.parentElement;
      const row = anchor.closest("tr");
      const rowContext = sigla ? label([cell, row].filter(node => node && eligible(node, root)).map(safeText).join(" ").slice(0, 2000)) : "";
      // Only "juntado ao/apensado ao <sigla>" (this document inside another);
      // the parent's own table says "Documento juntado: <filho>" and is ignored.
      const joinedMatch = /\b(juntad|apensad)[oa]\s+(?:ao|a)\b/.exec(rowContext);
      if (sigla && joinedMatch) {
        kind = "antecedent";
        display = `${joinedMatch[1] === "juntad" ? "Juntado" : "Apensado"} ao documento: ${sigla}`;
      }
      else if (/anex|attachment/.test(context)) kind = "attachment";
      else if (/antecedent|precedente/.test(context)) kind = "antecedent";
      else if (/original|integra|pdf|visualizar documento/.test(context)) kind = "original";
      else if (/documento|referencia|vinculad|relacionad/.test(context) || sigla) kind = "reference";
      else continue;
      const raw = anchor.getAttribute("href");
      const url = cleanUrl(raw, sourceUrl);
      if (!url) { pending.push("Uma referência externa ou não HTTPS foi omitida"); continue; }
      stripped ||= new URL(raw, sourceUrl).href !== url;
      const key = kind + "\n" + url + "\n" + display;
      if (!seen.has(key)) {
        references.push({ label: display, url, kind }); seen.add(key);
        const note = "Juntada/apensação explícita na tabela de eventos registrada como referência";
        if (display !== text && !recognized.includes(note)) recognized.push(note);
      }
    }
    if (references.length) recognized.push("Links documentais classificados por rótulo/contexto");
    if (references.some(ref => ["antecedent", "reference"].includes(ref.kind))) pending.push("Antecedentes e documentos relacionados entram só como referência (link); o conteúdo deles não é importado");
    if (stripped) pending.push("Parâmetros de URL desconhecidos/sensíveis e fragmentos removidos; alguns links podem exigir reabertura no SIGADOC");
    const historyLink = doc.querySelector("a#ver-mais[href]");
    if (historyLink) {
      try {
        const url = new URL(historyLink.getAttribute("href"), sourceUrl);
        const compact = numbers[0].replace(/[^A-Z0-9]/g, "");
        if (url.origin === ORIGIN && url.pathname === "/sigaex/app/expediente/doc/exibirAntigo" && url.searchParams.get("sigla") === compact) {
          metadata.movement_source = `${ORIGIN}${url.pathname}?sigla=${encodeURIComponent(compact)}`;
        }
      } catch { /* optional source must match exact document */ }
    }
    // A real document page whose content area was not found: the site layout may have changed.
    const layoutUnrecognized = new URL(sourceUrl).pathname === SIGA_DETAIL_PATH && !text;
    return { source_url: cleanUrl(sourceUrl), document_number: numbers[0], title, text, metadata, references, layout_unrecognized: layoutUnrecognized, coverage: { adapter: ADAPTER, recognized, pending: unique(pending) } };
  }
  function buildCapture(snapshot, uuid = () => crypto.randomUUID(), now = () => new Date().toISOString()) {
    const capture = { schema_version: 1, request_id: uuid(), source_url: snapshot.source_url, document_number: snapshot.document_number, title: snapshot.title, captured_at: now(), text: snapshot.text, metadata: snapshot.metadata, references: snapshot.references, coverage: snapshot.coverage };
    validateCapture(capture);
    return capture;
  }
  function validString(value, min, max) { return typeof value === "string" && value.length >= min && value.length <= max && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/.test(value); }
  function validateCapture(capture) {
    if (!exactKeys(capture, ["schema_version", "request_id", "source_url", "document_number", "title", "captured_at", "text", "metadata", "references", "coverage"]) || capture.schema_version !== 1) fail("CAPTURE_SCHEMA", "Formato da importação incompatível (schema_version 1 exigido).");
    if (typeof capture.request_id !== "string" || !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(capture.request_id)) fail("REQUEST_ID", "A importação não tem um identificador válido.");
    if (!validString(capture.source_url, 1, 2048) || cleanUrl(capture.source_url) !== capture.source_url) fail("SOURCE_URL", "URL de origem inválida ou com parâmetros não permitidos.");
    if (!validString(capture.document_number, 5, 120) || documentNumber(capture.document_number) !== capture.document_number) fail("DOCUMENT_IDENTITY", "Número oficial do documento inválido.");
    validateUrlIdentity(capture.source_url, capture.document_number);
    if (!validString(capture.title, 3, 500) || normalize(capture.title) !== capture.title) fail("DOCUMENT_TITLE", "Título do documento inválido.");
    if (typeof capture.captured_at !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/.test(capture.captured_at) || !Number.isFinite(Date.parse(capture.captured_at))) fail("CAPTURE_DATE", "Data UTC da leitura inválida.");
    if (new Date(capture.captured_at).toISOString().replace(".000Z", "Z") !== capture.captured_at.replace(".000Z", "Z")) fail("CAPTURE_DATE", "Data UTC da leitura inválida.");
    if (typeof capture.text !== "string") fail("CAPTURE_SCHEMA", "Texto documental inválido.");
    if (bytes(capture.text) > MAX_TEXT_BYTES) fail("TEXT_TOO_LARGE", "O texto deste documento é grande demais para importar de uma vez (limite de 500 KB). Nada foi importado.");
    if (!plain(capture.metadata) || Object.entries(capture.metadata).some(([key, value]) => !META_KEYS.includes(key) || !validString(value, 1, key === "movement_history" ? 60000 : 4000) || (key === "movement_history" && bytes(value) > 60000))) fail("CAPTURE_SCHEMA", "Metadados não reconhecidos ou grandes demais.");
    if (capture.metadata.movement_source && capture.metadata.movement_source !== `${ORIGIN}/sigaex/app/expediente/doc/exibirAntigo?sigla=${capture.document_number.replace(/[^A-Z0-9]/g, "")}`) fail("HISTORY_SOURCE", "Fonte de eventos divergente do documento.");
    if (!Array.isArray(capture.references) || capture.references.length > 2000 || capture.references.some(ref => !exactKeys(ref, ["label", "url", "kind"]) || !validString(ref.label, 1, 250) || !validString(ref.url, 1, 2048) || cleanUrl(ref.url) !== ref.url || !["original", "attachment", "antecedent", "reference"].includes(ref.kind))) fail("CAPTURE_SCHEMA", "Referências inválidas ou com URL não permitida.");
    if (!exactKeys(capture.coverage, ["adapter", "recognized", "pending"]) || capture.coverage.adapter !== ADAPTER || [capture.coverage.recognized, capture.coverage.pending].some(list => !Array.isArray(list) || list.length > 100 || list.some(value => !validString(value, 1, 600)))) fail("CAPTURE_SCHEMA", "Cobertura do adaptador inválida.");
    if (bytes(JSON.stringify({ protocol_version: 1, action: "import_capture", capture })) > MAX_PAYLOAD_BYTES) fail("PAYLOAD_TOO_LARGE", "O documento é grande demais para importar de uma vez (limite de 1 MB). Nada foi importado.");
    return capture;
  }
  function identity(capture) {
    const url = new URL(capture.source_url);
    if (url.pathname === SIGA_DETAIL_PATH) {
      for (const [key, value] of url.searchParams) {
        const via = /^(.+)-([A-Z])$/.exec(value);
        if (key === "sigla" && via && via[1] === capture.document_number) url.searchParams.set(key, capture.document_number);
      }
    }
    return JSON.stringify([url.href, capture.document_number]);
  }
  function errorResult(code, message) { return { protocol_version: 1, ok: false, error: { code, message } }; }
  function validateReceipt(result, requestId) {
    if (!plain(result) || result.protocol_version !== 1 || typeof result.ok !== "boolean") fail("HOST_SCHEMA", "A extensão e o Ponte estão em versões diferentes. Atualize os dois pelo instalador do Ponte.");
    if (!result.ok) {
      if (!plain(result.error) || !validString(result.error.code, 1, 100) || !validString(result.error.message, 1, 2000)) fail("HOST_SCHEMA", "O Ponte respondeu em um formato que esta extensão não reconhece. Atualize os dois pelo instalador do Ponte.");
      return result;
    }
    if (result.request_id !== requestId || !Number.isSafeInteger(result.document_id) || result.document_id < 1 || !Number.isSafeInteger(result.capture_id) || result.capture_id < 1 || typeof result.duplicate !== "boolean" || !validString(result.message, 1, 2000) || (Object.hasOwn(result, "ui_opened") && typeof result.ui_opened !== "boolean") || (Object.hasOwn(result, "outcome") && !OUTCOMES.includes(result.outcome))) fail("HOST_SCHEMA", "A confirmação recebida não corresponde a esta importação. Clique em Repetir: nada será duplicado.");
    return result;
  }
  // What the Ponte did with the page. Hosts from before 0.14 send only
  // `duplicate`; "legacy_duplicate" stands for that case.
  const OUTCOMES = ["imported", "confirmed", "unchanged"];
  function importOutcome(result) {
    if (OUTCOMES.includes(result.outcome)) return result.outcome;
    return result.duplicate ? "legacy_duplicate" : "imported";
  }
  // Note and priority the person may add to an import. The Ponte (0.31 or later)
  // refuses anything outside this shape, so the same rules are applied before sending.
  const EXTRAS_PRIORITIES = ["baixa", "normal", "alta", "urgente"];
  const MAX_NOTE_CHARS = 2000;
  const NOTE_CONTROL_RE = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/;
  function noteLength(note) { return Array.from(note).length; }
  function validateExtras(extras) {
    const keys = plain(extras) ? Object.keys(extras) : [];
    if (!keys.length || keys.some(key => !["note", "priority"].includes(key))) fail("EXTRAS_INVALID", "A nota ou a prioridade não pôde ser enviada. Confira o que foi escrito e tente de novo.");
    if (Object.hasOwn(extras, "note")) {
      const note = extras.note;
      if (typeof note !== "string" || !note || note !== note.trim() || noteLength(note) > MAX_NOTE_CHARS || NOTE_CONTROL_RE.test(note)) fail("EXTRAS_INVALID", "A nota não pôde ser enviada: use até 2000 caracteres, sem caracteres de controle.");
    }
    if (Object.hasOwn(extras, "priority") && !EXTRAS_PRIORITIES.includes(extras.priority)) fail("EXTRAS_INVALID", "A prioridade escolhida não é válida.");
    return extras;
  }
  // What the controls of the panel hold -> the extras to send, or null when both are empty.
  function buildExtras(noteText, priority) {
    const extras = {};
    const note = String(noteText || "").replace(/\r\n?/g, "\n").replace(new RegExp(NOTE_CONTROL_RE.source, "g"), "").trim();
    if (note) extras.note = note;
    if (priority) extras.priority = priority;
    return Object.keys(extras).length ? validateExtras(extras) : null;
  }
  // The sentence about the extras after a delivered import; "" when none were involved.
  // `sent` is what the panel sent; the background marks `extras_skipped` when an older Ponte got none.
  function extrasNotice(result, sent) {
    if (!plain(result) || result.ok !== true) return "";
    if (result.extras_skipped === true) return "O Ponte instalado não recebe nota nem prioridade; a importação foi feita sem elas. Atualize o Ponte.";
    if (!plain(sent)) return "";
    const both = Object.hasOwn(sent, "note") && Object.hasOwn(sent, "priority");
    if (result.extras_applied === true) return both ? "Nota e prioridade enviadas." : Object.hasOwn(sent, "note") ? "Nota enviada." : "Prioridade enviada.";
    if (typeof result.extras_message === "string" && result.extras_message.trim()) return result.extras_message.trim().slice(0, 500);
    return both ? "A nota e a prioridade não foram aplicadas." : Object.hasOwn(sent, "note") ? "A nota não foi aplicada." : "A prioridade não foi aplicada.";
  }
  // Chrome and Edge report the failure in English; each known case gets its
  // own advice, and an unknown one carries the browser's words along.
  const LATEST = "github.com/demusis/ponteproj/releases";
  function nativeError(message) {
    message = typeof message === "string" ? message : "";
    if (/not found|não encontrado|nao encontrado/i.test(message)) return errorResult("HOST_MISSING", "O Ponte não foi encontrado neste computador. Abra o Ponte uma vez e reinicie o navegador. Se continuar, reinstale o Ponte com a sua conta do Windows.");
    if (/forbidden|access.*denied|proibid/i.test(message)) return errorResult("HOST_FORBIDDEN", `O navegador bloqueou a ligação com o Ponte: a versão instalada não reconhece esta extensão. Instale a versão mais recente do Ponte (${LATEST}) e reinicie o navegador.`);
    if (/has exited|exited|encerrou/i.test(message)) return errorResult("HOST_EXITED", `O Ponte abriu e fechou sem responder. Isso acontece com um Ponte antigo, que não reconhece a extensão instalada pela loja: instale a versão mais recente (${LATEST}) com a sua conta do Windows e reinicie o navegador. Nada foi duplicado.`);
    if (/failed to start|falha ao iniciar/i.test(message)) return errorResult("HOST_START", "O navegador não conseguiu iniciar o Ponte. Reinstale o Ponte com a sua conta do Windows e reinicie o navegador; se continuar, o antivírus ou uma regra do órgão pode estar bloqueando o programa.");
    if (/error when communicating|erro ao comunicar/i.test(message)) return errorResult("HOST_PROTOCOL", `A resposta do Ponte chegou ilegível. Atualize o Ponte (${LATEST}) e a extensão, e reinicie o navegador.`);
    const detail = message ? ` (Detalhe do navegador: ${message.slice(0, 160)})` : "";
    return errorResult("HOST_CONNECTION", `Não recebemos a confirmação do Ponte. Clique em Repetir: nada será duplicado.${detail}`);
  }
  // "1 arquivo não baixado" / "3 arquivos não baixados".
  function filesNotDownloaded(count) { return count === 1 ? "1 arquivo não baixado" : `${count} arquivos não baixados`; }
  const OLD_PONTE_HINT = "Atualize o Ponte para ver se o documento já foi importado.";
  // Turns the answers of "ping" and "lookup" into one outcome the panel can show.
  // `lookup` is null when it was not asked (Ponte unreachable or too old).
  function checkOutcome(ping, lookup) {
    if (!plain(ping) || ping.ok !== true) {
      const error = plain(ping) && plain(ping.error) ? ping.error : {};
      return { kind: "unreachable", code: typeof error.code === "string" ? error.code : "", message: typeof error.message === "string" ? error.message : "" };
    }
    const actions = Array.isArray(ping.actions) ? ping.actions.filter(value => typeof value === "string") : [];
    if (!actions.includes("lookup")) return { kind: "old" };
    const canOpen = actions.includes("open_document");
    if (!plain(lookup)) return { kind: "hidden" };
    if (lookup.ok !== true) {
      const error = plain(lookup.error) ? lookup.error : {};
      if (["invalid_request", "unsupported_action"].includes(error.code)) return { kind: "old" };
      return { kind: "unreachable", code: typeof error.code === "string" ? error.code : "", message: typeof error.message === "string" ? error.message : "" };
    }
    if (lookup.found === true && Number.isSafeInteger(lookup.document_id) && lookup.document_id > 0) return { kind: "found", document_id: lookup.document_id, imported_at: typeof lookup.imported_at === "string" ? lookup.imported_at : "", canOpen };
    if (lookup.found === false) return { kind: "missing", canOpen };
    return { kind: "hidden" };
  }
  // What the panel shows before any click. text === "" means: show nothing.
  function statusView(outcome) {
    const none = { text: "", tone: "", retry: false, open: false, importLabel: "Importar para o assistente" };
    if (!outcome) return none;
    if (outcome.kind === "unreachable") {
      const generic = ["HOST_CONNECTION", "HOST_TIMEOUT", ""].includes(outcome.code) || !outcome.message;
      return { ...none, tone: "warning", retry: true, text: generic ? "O Ponte não respondeu. Abra o Ponte e tente de novo." : outcome.message };
    }
    if (outcome.kind === "old") return { ...none, tone: "note", text: OLD_PONTE_HINT };
    if (outcome.kind === "found") return { ...none, tone: "success", open: outcome.canOpen === true, importLabel: "Importar de novo", text: outcome.imported_at ? `Já está no Ponte — importado em ${outcome.imported_at}.` : "Já está no Ponte." };
    return none;
  }
  class ImportSession {
    constructor(send) { this.send = send; this.capture = null; this.state = "idle"; this.inFlight = null; }
    run(candidate) {
      if (this.inFlight) return this.inFlight;
      validateCapture(candidate);
      if (!this.capture || identity(this.capture) !== identity(candidate) || this.state === "success") {
        this.capture = JSON.parse(JSON.stringify(candidate));
        const freeze = value => { if (value && typeof value === "object") { Object.values(value).forEach(freeze); Object.freeze(value); } };
        freeze(this.capture);
      }
      const sent = this.capture;
      this.state = "sending";
      this.inFlight = (async () => {
        let result;
        try { result = validateReceipt(await this.send(sent), sent.request_id); }
        catch (error) { result = errorResult(error.code || "HOST_CONNECTION", error.code ? error.message : nativeError("").error.message); }
        this.state = result.ok ? "success" : "error";
        return { result, capture: sent };
      })().finally(() => { this.inFlight = null; });
      return this.inFlight;
    }
  }
  return { ORIGIN, HOST, ADAPTER, MAX_TEXT_BYTES, MAX_PAYLOAD_BYTES, CaptureError, allowedOrigin, cleanUrl, documentNumber, safeText, extractDocument, buildCapture, validateCapture, validateReceipt, importOutcome, identity, errorResult, filesNotDownloaded, checkOutcome, statusView, UNRECOGNIZED_TEXT, OLD_PONTE_HINT, nativeError, ImportSession, bytes, exactKeys, validateExtras, buildExtras, extrasNotice, MAX_NOTE_CHARS, EXTRAS_PRIORITIES };
});

