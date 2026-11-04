(function(root) {
  "use strict";
  const MAX_BYTES = 2 * 1024 * 1024;
  const normalize = text => String(text || "").replace(/\s+/g, " ").trim();
  function parse(doc, number) {
    const identity = normalize(doc.querySelector("h2")?.textContent);
    if (identity !== number) throw new Error("Identidade divergente na página de eventos.");
    const events = [];
    let scope = "";
    for (const node of doc.querySelectorAll("h3, table.mov")) {
      if (node.tagName === "H3") {scope = normalize(node.textContent); continue;}
      const headings = Array.from(node.querySelectorAll("thead tr:first-child th")).map(x => normalize(x.textContent));
      if (headings.join("|") !== "Data|Evento|Cadastrante|Atendente|Descrição|Duração" || !scope) throw new Error("Estrutura de eventos não reconhecida.");
      for (const row of node.querySelectorAll("tbody > tr")) {
        if (row.closest("[hidden], [aria-hidden='true'], .d-none") || /display\s*:\s*none/i.test(row.getAttribute("style") || "")) continue;
        const cells = Array.from(row.children).filter(c => c.tagName === "TD");
        if (cells.length < 7 || cells.length > 8 || cells.slice(0,7).some(c => c.colSpan !== 1 || c.rowSpan !== 1)) throw new Error("Colunas de eventos não reconhecidas.");
        const clean = (cell, index) => {
          const clone = cell.cloneNode(true);
          clone.querySelectorAll("script,style,input,textarea,button,[hidden],.d-none,[aria-hidden='true']").forEach(x => x.remove());
          if (index === 3 || index === 5) {
            const titles = [...new Set(Array.from(clone.querySelectorAll("span[title]")).filter(span => normalize(span.textContent)).map(span => normalize(span.getAttribute("title"))).filter(Boolean))];
            if (titles.length === 1) return titles[0];
          }
          return normalize(clone.textContent);
        };
        const values = cells.slice(0,7).map(clean);
        if (!values[1]) continue;
        if (values.some(v => new TextEncoder().encode(v).length > 8000 || /[\u0000-\u001f\u007f]/.test(v))) throw new Error("Campo de evento inválido.");
        events.push({scope, date:values[0], event:values[1], registered_unit:values[2], registered_person:values[3], attendant_unit:values[4], attendant_person:values[5], description:values[6]});
        if (events.length > 200) throw new Error("Histórico excede 200 eventos; não foi importado parcialmente.");
      }
    }
    if (!events.length) throw new Error("Nenhuma tabela de eventos reconhecida.");
    const json = JSON.stringify(events);
    if (new TextEncoder().encode(json).length > 60000) throw new Error("Histórico excede 60 KB; não foi importado parcialmente.");
    return json;
  }
  async function enrich(capture, fetcher = fetch) {
    if (!capture.metadata.movement_source) return capture;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 15000);
    try {
      const url = new URL(capture.metadata.movement_source);
      const compact = capture.document_number.replace(/[^A-Z0-9]/g, "");
      if (url.origin !== "https://www.sigadoc.mt.gov.br" || url.pathname !== "/sigaex/app/expediente/doc/exibirAntigo" || url.search !== `?sigla=${compact}`) throw new Error("Fonte de eventos inválida.");
      const response = await fetcher(url.href, {credentials:"include", redirect:"error", signal:controller.signal});
      if (!response.ok || !response.headers.get("content-type")?.includes("text/html")) throw new Error("Não foi possível obter a página de eventos.");
      const reader = response.body.getReader(); let size = 0; const chunks = [];
      try {
        while (true) {const part = await reader.read(); if (part.done) break; size += part.value.length; if (size > MAX_BYTES) throw new Error("Página de eventos excede 2 MiB."); chunks.push(part.value);}
      } finally {await reader.cancel();}
      const bytes = new Uint8Array(size); let offset=0; for (const chunk of chunks) {bytes.set(chunk,offset);offset+=chunk.length;}
      const doc = new DOMParser().parseFromString(new TextDecoder().decode(bytes), "text/html");
      capture.metadata.movement_history = parse(doc,capture.document_number);
      capture.coverage.recognized.push("Eventos da visualização detalhada, com fonte explícita e datas preservadas como exibidas");
    } catch (error) {
      capture.coverage.pending.push(`Histórico não obtido: ${error.name === "AbortError" ? "tempo limite" : error.message}`);
    } finally {clearTimeout(timer);}
    return capture;
  }
  root.PonteHistory = {parse,enrich};
})(globalThis);
