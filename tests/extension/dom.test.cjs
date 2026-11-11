"use strict";
const { test, before, after } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { createRequire } = require("node:module");
const extensionRequire = createRequire(path.join(__dirname, "../../extension/package.json"));
let playwright;
try { playwright = process.env.PONTEPROJ_PLAYWRIGHT_DIR ? require(process.env.PONTEPROJ_PLAYWRIGHT_DIR) : extensionRequire("playwright"); }
catch { /* DOM tests clearly skip when the optional dev dependency is absent. */ }
let browser;
const fixture = fs.readFileSync(path.join(__dirname, "fixture.html"), "utf8");
const siga112Fixture = fs.readFileSync(path.join(__dirname, "fixture-siga112.html"), "utf8");
const corePath = path.join(__dirname, "../../extension/capture-core.js");
const contentPath = path.join(__dirname, "../../extension/content.js");
const URL = "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=ORG-PRO-2026%2F01234&access_token=source-secret#fragmento";
before(async () => {
  if (playwright) browser = await playwright.chromium.launch({ headless: true, channel: process.env.PONTEPROJ_BROWSER_CHANNEL || "chrome" });
});
after(async () => { await browser?.close(); });
async function pageFor(html = fixture, url = URL) {
  const page = await browser.newPage();
  // Every request is fulfilled locally. These tests do not access SIGADOC.
  await page.route("**/*", route => route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: html }));
  await page.goto(url);
  await page.addScriptTag({ path: corePath });
  return page;
}
const options = { skip: !playwright && "Instale a devDependency playwright ou defina PONTEPROJ_PLAYWRIGHT_DIR para executar fixtures DOM em Chromium." };
test("SIGA content card preserves body and tables, excludes sidebar and recognizes print PDF", options, async () => {
  const html = siga112Fixture.replace("</body>", `<div class="card-sidebar card border-alert bg-white"><div class="card-body"><p>Texto fictício do documento.</p><table><tr><td>Prazo</td><td>05/11/2026</td></tr></table><input value="secret"><button>Tramitar</button></div></div><div class="gt-sidebar"><div class="card-sidebar card border-alert bg-white"><div class="card-body">NAO IMPORTAR MENU</div></div></div><button id="ver-impress-227-o" onclick="javascript:popitup('/sigaex/app/arquivo/exibir?arquivo=TESTEOFI202600042A.pdf&sigla=TESTEOFI202600042A&popup=true&token=secret');">Ver Impressão</button></body>`);
  const page = await pageFor(html, "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A");
  const capture = await page.evaluate(() => PonteCapture.buildCapture(PonteCapture.extractDocument(document, location.href)));
  assert.match(capture.text, /Texto fictício/);
  assert.match(capture.text, /05\/11\/2026/);
  assert.match(capture.text, /\|[^\n]*05\/11\/2026[^\n]*\|/);
  assert.ok(!/NAO IMPORTAR|Tramitar|secret/.test(capture.text));
  assert.equal(capture.references[0].url, "https://www.sigadoc.mt.gov.br/sigaex/app/arquivo/exibir?arquivo=TESTEOFI202600042A.pdf&sigla=TESTEOFI202600042A");
  assert.ok(!JSON.stringify(capture).includes("token="));
  await page.close();
});
test("SIGA v11.2 fictional metadata fixture captures H2/description/classification and preserves via URL", options, async () => {
  const source = "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A&linkVolta=history.back()&token=fixture-secret";
  const page = await pageFor(siga112Fixture, source);
  const capture = await page.evaluate(() => PonteCapture.buildCapture(PonteCapture.extractDocument(document, location.href)));
  assert.equal(capture.document_number, "TESTE-OFI-2026/00042");
  assert.equal(capture.title, "Pedido fictício de revisão de materiais de treinamento");
  assert.equal(capture.metadata.classification, "000.00 - CLASSIFICAÇÃO FICTÍCIA PARA TESTE");
  assert.equal(capture.source_url, "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A");
  assert.equal(capture.text, "");
  assert.equal(capture.references.length, 0);
  assert.ok(capture.coverage.recognized.some(value => value.includes("SIGA v11.2 inspecionada")));
  assert.ok(!capture.coverage.pending.some(value => /PDFs e arquivos anexos ainda não|Extração integral/.test(value)));
  assert.ok(capture.coverage.pending.some(value => value.includes("Autenticidade, assinatura e permissões")));
  assert.ok(!JSON.stringify(capture).includes("secret"));
  assert.ok(!JSON.stringify(capture).includes("Voltar"));
  assert.ok(!JSON.stringify(capture).includes("nao-capturar"));
  await instrumentContent(page);
  assert.equal(await page.evaluate(() => window.__testShadow.querySelector("button").disabled), false);
  await page.evaluate(() => window.__testShadow.querySelector("button").click());
  assert.equal(await page.evaluate(() => window.__requests[0].document_number), "TESTE-OFI-2026/00042");
  await page.close();
});

test('SIGA provenance separates origin, holder, signature and missing movement dates', options, async()=>{
  const html=siga112Fixture.replace('</body>', `<div class="siga-menu-acoes"><h3>1ª Via (Setor Competente) - Aguardando Andamento</h3></div>
  <div id="collapseDocDetalhes"><p><b>De:</b> Autora fictícia</p><p><b>Para:</b></p><p><b>Data: :</b> 22/10/26</p><p><b>Cadastrante:</b> Operador fictício (UNIDADE)</p><p><b>Espécie:</b> DECLARAÇÃO</p><p><b>Processo vinculado:</b> TESTE-PRO-2026/00123</p><p><b>Senha:</b> segredo</p></div>
  <div id="collapseOutrosMob"><table><tr><td>A</td><td>Aguardando Andamento</td><td>Destinatário fictício</td></tr><tr><td>Geral</td><td>Documento Assinado com Senha</td><td>Autora fictícia</td></tr></table></div></body>`);
  const page=await pageFor(html,'https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A');
  const capture=await page.evaluate(()=>PonteCapture.buildCapture(PonteCapture.extractDocument(document,location.href)));
  assert.equal(capture.metadata.origin,'Autora fictícia');assert.equal(capture.metadata.document_date,'22/10/26');
  assert.equal(capture.metadata.signature_status,'Documento Assinado com Senha');assert.equal(capture.metadata.current_holder,'Destinatário fictício');
  assert.equal(capture.metadata.via_status,'Via A — Aguardando Andamento');
  assert.equal(capture.metadata.process_number,'TESTE-PRO-2026/00123');assert.ok(!capture.metadata.sender);assert.ok(!capture.metadata.received_at);
  assert.ok(!JSON.stringify(capture).includes('segredo'));
  assert.ok(capture.coverage.pending.some(v=>v.includes('Remetente da última')));
  await page.close();
});
test("SIGA event table records 'Juntado ao documento' with a via sigla as a structured reference", options, async () => {
  const html = siga112Fixture.replace("</body>", `<div class="vias"><h3>1ª Via (Setor Competente) - Juntado</h3></div>
  <table><tr><th>Data</th><th>Lotação</th><th>Evento</th><th>Descrição</th></tr>
  <tr><td>02/11/2026</td><td>UNIDADE</td><td>Juntada</td><td>Juntado ao documento: <a href="/sigaex/app/expediente/doc/exibir?sigla=TESTE-DIC-2026/00777-A&token=x">TESTE-DIC-2026/00777-A</a></td></tr>
  <tr><td>01/11/2026</td><td>UNIDADE</td><td>Juntada</td><td>Documento juntado: <a href="/sigaex/app/expediente/doc/exibir?sigla=TESTE-DES-2026/00888-A">TESTE-DES-2026/00888-A</a></td></tr></table></body>`);
  const page = await pageFor(html, "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A");
  const capture = await page.evaluate(() => PonteCapture.buildCapture(PonteCapture.extractDocument(document, location.href)));
  assert.equal(capture.metadata.via_status, "1ª Via (Setor Competente) - Juntado");
  const joined = capture.references.find(ref => ref.label === "Juntado ao documento: TESTE-DIC-2026/00777-A");
  assert.ok(joined, JSON.stringify(capture.references));
  assert.equal(joined.kind, "antecedent");
  assert.equal(joined.url, "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-DIC-2026%2F00777-A");
  // The opposite direction (a piece joined into this document) is not mislabeled.
  const other = capture.references.find(ref => ref.url.includes("00888"));
  assert.equal(other.label, "TESTE-DES-2026/00888-A");
  assert.equal(other.kind, "reference");
  assert.ok(!JSON.stringify(capture).includes("token="));
  await page.close();
});
test("SIGA v11.2 refuses mismatched via, junk suffix, other route and missing/extraneous identity", options, async () => {
  const path = "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir";
  for (const [html, url] of [
    [siga112Fixture, path + "?sigla=TESTE-OFI-2026%2F99999-A"],
    [siga112Fixture, path + "?sigla=TESTE-OFI-2026%2F00042-AB"],
    [siga112Fixture, "https://www.sigadoc.mt.gov.br/outra-rota?sigla=TESTE-OFI-2026%2F00042-A"],
    [siga112Fixture.replace("Descrição:", "Observação:"), path + "?sigla=TESTE-OFI-2026%2F00042-A"],
    [siga112Fixture.replace("Descrição:", "Descrição"), path + "?sigla=TESTE-OFI-2026%2F00042-A"],
    [siga112Fixture.replace("<h2>", "<h3>").replace("</h2>", "</h3>"), path + "?sigla=TESTE-OFI-2026%2F00042-A"],
    [siga112Fixture.replace("</h2>", "</h2><h2>TESTE-OFI-2026/00043</h2>"), path + "?sigla=TESTE-OFI-2026%2F00042-A"],
    [siga112Fixture.replace("<p id=\"descricao\">", "<p id=\"descricao\" hidden>"), path + "?sigla=TESTE-OFI-2026%2F00042-A"],
    [fixture, path + "?SIGLA=ORG-PRO-2026%2F01234-A"]
  ]) {
    const page = await pageFor(html, url);
    assert.equal(await page.evaluate(() => { try { PonteCapture.extractDocument(document, location.href); return "accepted"; } catch { return "rejected"; } }), "rejected");
    await page.close();
  }
});
test("synthetic DOM adapter extracts explicit fields, excludes secrets, distinguishes links from files", options, async () => {
  const page = await pageFor();
  const capture = await page.evaluate(() => PonteCapture.buildCapture(PonteCapture.extractDocument(document, location.href)));
  assert.equal(capture.document_number, "ORG-PRO-2026/01234");
  assert.equal(capture.title, "Aquisição de material de expediente");
  assert.equal(capture.metadata.document_type, "Processo administrativo");
  assert.equal(capture.metadata.origin, "Unidade de exemplo");
  assert.deepEqual(capture.references.map(value => value.kind), ["original", "attachment", "antecedent"]);
  assert.ok(capture.text.includes("Solicita-se a análise"));
  assert.equal(capture.text.match(/Justificativa de exemplo/g).length, 1);
  assert.ok(!JSON.stringify(capture).includes("secret"));
  assert.ok(!capture.text.includes("Ficha de documento"));
  assert.ok(!Object.hasOwn(capture, "files"));
  assert.ok(!capture.coverage.pending.some(value => value.includes("nenhum arquivo foi baixado")));
  assert.ok(capture.coverage.pending.some(value => value.includes("Antecedentes e documentos relacionados")));
  assert.ok(capture.coverage.pending.some(value => value.includes("estruturas do SIGADOC além da ficha")));
  assert.ok(capture.coverage.pending.some(value => value.includes("iframe")));
  await page.close();
});
test("login, lists, unknown or conflicting identity are refused; no raw-body fallback", options, async () => {
  for (const [html, url, expected] of [
    ['<main><input type="password" value="password-secret"></main>', URL, "LOGIN_PAGE"],
    ['<main><h1>Resultados da pesquisa</h1><dl><dt>Documento</dt><dd>ORG-PRO-2026/01234</dd><dt>Assunto</dt><dd>Resultado de pesquisa</dd></dl></main>', URL, "LIST_PAGE"],
    ['<main><h1>Qualquer página</h1><p>ORG-PRO-2026/01234</p><p>Título possível</p></main>', URL, "PAGE_NOT_RECOGNIZED"],
    ['<main><h1>Qualquer página</h1><p>ORG-PRO-2026/01234</p><p>Título possível</p></main>', "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/outra?sigla=ORG-PRO-2026%2F01234", "DOCUMENT_IDENTITY"],
    [fixture.replace('</dl>', '<dt>Processo</dt><dd>ORG-PRO-2026/99999</dd></dl>'), URL, "DOCUMENT_IDENTITY"],
    [fixture.replace('<main ', '<div hidden><main ').replace('</main>', '</main></div>'), URL, "UNRECOGNIZED_PAGE"],
    [fixture, "https://www.sigadoc.mt.gov.br/login", "LOGIN_PAGE"]
  ]) {
    const page = await pageFor(html, url);
    assert.equal(await page.evaluate(() => { try { PonteCapture.extractDocument(document, location.href); return "accepted"; } catch (error) { return error.code; } }), expected);
    await page.close();
  }
  const page = await pageFor('<main data-document-detail><dl><dt>Documento</dt><dd>ORG-PRO-2026/01234</dd><dt>Assunto</dt><dd>Assunto reconhecido</dd></dl><p>Texto fora de área documental; não copiar</p></main>');
  const snapshot = await page.evaluate(() => PonteCapture.extractDocument(document, location.href));
  assert.equal(snapshot.text, "");
  assert.ok(snapshot.coverage.pending.some(value => value.includes("corpo geral")));
  await page.close();
});
async function instrumentContent(page, native = {}) {
  await page.evaluate(config => {
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (options) { const result = attach.call(this, options); window.__testShadow = result; return result; };
    window.__requests = [];
    window.__imports = [];
    window.__callbacks = [];
    // Answers of the fake Ponte to the status check; a test may replace them (or "hang").
    window.__calls = [];
    window.__native = { ping: { protocol_version: 1, ok: true, app_version: "0.29.0", actions: ["lookup", "open_document"] }, lookup: { protocol_version: 1, ok: true, found: false }, open: { protocol_version: 1, ok: true, document_id: 1, ui_opened: true }, ...config };
    window.chrome = { runtime: { id: "test-extension", lastError: null, onMessage: { addListener: listener => { window.__onMessage = listener; } }, sendMessage: (message, callback) => {
      if (message.action === "ponte.import_capture") { window.__requests.push(message.capture); window.__imports.push(message); window.__callbacks.push(callback); return; }
      window.__calls.push(message);
      const reply = { "ponte.ping": window.__native.ping, "ponte.lookup": window.__native.lookup, "ponte.open": window.__native.open }[message.action];
      if (reply !== "hang") setTimeout(() => callback(reply), 0);
    } } };
  }, native);
  await page.addScriptTag({ path: contentPath });
}
test("injection remains single across DOM changes; async acknowledgment stays tied to captured document", options, async () => {
  const page = await pageFor();
  await instrumentContent(page);
  await page.addScriptTag({ path: contentPath });
  assert.equal(await page.locator("#ponteproj-import-assistente").count(), 1);
  await page.evaluate(() => window.__testShadow.querySelector("button").click());
  assert.equal(await page.evaluate(() => window.__requests.length), 1);
  assert.ok(!(await page.evaluate(() => window.__testShadow.querySelector('[role="status"]').textContent)).includes("Captura salva"));
  await page.evaluate(() => { document.getElementById("official-number").textContent = "ORG-PRO-2026/01235"; document.getElementById("official-title").textContent = "Segundo documento"; history.pushState({}, "", "/doc?sigla=ORG-PRO-2026%2F01235"); });
  await page.waitForTimeout(300);
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: true, request_id: window.__requests[0].request_id, document_id: 1, capture_id: 1, duplicate: false, message: "Salvo", ui_opened: false }));
  await page.waitForTimeout(50);
  const text = await page.evaluate(() => window.__testShadow.querySelector('[role="status"]').textContent);
  assert.ok(text.startsWith("ORG-PRO-2026/01234:"), text);
  assert.equal(await page.evaluate(() => window.__requests[0].document_number), "ORG-PRO-2026/01234");
  assert.equal(await page.locator("#ponteproj-import-assistente").count(), 1);
  await page.waitForTimeout(400);
  assert.equal(await page.evaluate(() => window.__requests.length), 1);
  await page.evaluate(() => document.getElementById("ponteproj-import-assistente").remove());
  await page.waitForTimeout(300);
  assert.equal(await page.locator("#ponteproj-import-assistente").count(), 1);
  await page.close();
});
test("UI retry after lost acknowledgment reuses original UUID/text; no success before valid commit receipt", options, async () => {
  const page = await pageFor();
  await instrumentContent(page);
  await page.evaluate(() => window.__testShadow.querySelector("button").click());
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: false, error: { code: "HOST_CONNECTION", message: "Confirmação perdida" } }));
  await page.waitForTimeout(50);
  await page.evaluate(() => { document.querySelector(".document-text p").textContent = "Texto novo após a tentativa"; });
  await page.waitForTimeout(300);
  await page.evaluate(() => window.__testShadow.querySelector("button").click());
  const requests = await page.evaluate(() => window.__requests);
  assert.equal(requests.length, 2);
  assert.equal(requests[0].request_id, requests[1].request_id);
  assert.equal(requests[0].text, requests[1].text);
  assert.ok(!requests[1].text.includes("Texto novo"));
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: true, request_id: "different", document_id: 1, capture_id: 1, duplicate: false, message: "Salvo" }));
  await page.waitForTimeout(50);
  const text = await page.evaluate(() => window.__testShadow.querySelector('[role="status"]').textContent);
  assert.ok(!text.includes("Importado"));
  assert.ok(text.includes("não corresponde a esta importação"), text);
  await page.close();
});

test("an unchanged import is shown as a warning with the host's explanation; older hosts' duplicates too", options, async () => {
  const page = await pageFor();
  await instrumentContent(page);
  await page.evaluate(() => window.__testShadow.querySelector("button").click());
  const hostMessage = "Importação ignorada: o SIGADOC mostra o mesmo conteúdo já importado em 04/11/2026 12:38. Nada foi gravado. Se o documento mudou, atualize a página (F5) e importe de novo.";
  await page.evaluate(text => window.__callbacks.shift()({ protocol_version: 1, ok: true, request_id: window.__requests[0].request_id, document_id: 1, capture_id: 1, duplicate: true, outcome: "unchanged", message: text, ui_opened: false }), hostMessage);
  await page.waitForTimeout(50);
  let status = await page.evaluate(() => { const el = window.__testShadow.querySelector('[role="status"]'); return { text: el.textContent, className: el.className }; });
  assert.equal(status.text, hostMessage);
  assert.ok(status.className.includes("warning"), status.className);
  assert.ok(!status.className.includes("success"), status.className);
  // Next click is a fresh request (new UUID), never a retry of the ignored one.
  await page.evaluate(() => window.__testShadow.querySelector("button").click());
  const requests = await page.evaluate(() => window.__requests);
  assert.equal(requests.length, 2);
  assert.notEqual(requests[0].request_id, requests[1].request_id);
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: true, request_id: window.__requests[1].request_id, document_id: 1, capture_id: 1, duplicate: true, message: "Salvo", ui_opened: false }));
  await page.waitForTimeout(50);
  status = await page.evaluate(() => { const el = window.__testShadow.querySelector('[role="status"]'); return { text: el.textContent, className: el.className }; });
  assert.ok(status.text.includes("já estava importado"), status.text);
  assert.ok(status.text.includes("Abra o Ponte"), status.text);
  assert.ok(status.className.includes("warning"), status.className);
  await page.close();
});

const historyFixture = `<h2>TESTE-OFI-2026/00042</h2><h3>1ª Via (Setor Competente)</h3><table class="mov"><thead><tr><th>Data</th><th>Evento</th><th>Cadastrante</th><th>Atendente</th><th>Descrição</th><th>Duração</th></tr></thead><tbody>
<tr><td>22/10/26</td><td>Recebimento</td><td>UNIDADE</td><td>Destino fictício</td><td>UNIDADE</td><td>Pessoa fictícia</td><td>Ciência<button>NAO_IMPORTAR</button></td><td rowspan="2">3min</td></tr>
<tr><td></td><td>Transferência</td><td>UNIDADE</td><td>Origem fictícia</td><td>UNIDADE</td><td>Pessoa fictícia</td><td></td></tr>
<tr class="d-none"><td></td><td>SEGREDO_OCULTO</td><td></td><td></td><td></td><td></td><td></td></tr>
</tbody></table>`;
test("history preserves blank dates and source roles; rejects identity and column drift", options, async () => {
  const page=await pageFor();
  await page.addScriptTag({path:path.join(__dirname,"../../extension/history.js")});
  const result=await page.evaluate(html=>{
    const doc=new DOMParser().parseFromString(html,"text/html");
    return JSON.parse(PonteHistory.parse(doc,"TESTE-OFI-2026/00042"));
  },historyFixture);
  assert.equal(result.length,2);assert.equal(result[1].date,"");assert.equal(result[1].registered_person,"Origem fictícia");assert.equal(result[0].description,"Ciência");
  for (const html of [historyFixture.replace("00042","00043"),historyFixture.replace("Cadastrante","Remetente"),historyFixture.replace('<td>Recebimento</td>','<td colspan="2">Recebimento</td>')]) {
    const failed=await page.evaluate(html=>{try{PonteHistory.parse(new DOMParser().parseFromString(html,"text/html"),"TESTE-OFI-2026/00042");return false;}catch{return true;}},html);
    assert.equal(failed,true);
  }
  await page.close();
});
test("history fetch uses exact source, no redirects, bounded response and partial capture on failure", options, async () => {
  const page=await pageFor();await page.addScriptTag({path:path.join(__dirname,"../../extension/history.js")});
  const result=await page.evaluate(async html=>{
    const capture=PonteCapture.buildCapture(PonteCapture.extractDocument(document,location.href));
    capture.metadata.movement_source="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla=ORGPRO202601234";
    let options;
    await PonteHistory.enrich(capture, async (_, opts)=>{options={credentials:opts.credentials,redirect:opts.redirect};return new Response(html.replaceAll("TESTE-OFI-2026/00042",capture.document_number),{headers:{"content-type":"text/html"}});});
    const first=JSON.parse(JSON.stringify(capture));delete capture.metadata.movement_history;
    await PonteHistory.enrich(capture,async()=>new Response("x".repeat(2*1024*1024+1),{headers:{"content-type":"text/html"}}));
    return {first,last:capture,options};
  },historyFixture);
  assert.equal(JSON.parse(result.first.metadata.movement_history).length,2);assert.deepEqual(result.options,{credentials:"include",redirect:"error"});
  assert.equal(result.last.metadata.movement_history,undefined);assert.ok(result.last.coverage.pending.some(s=>s.includes("2 MiB")));
  await page.close();
});

test("history source discovery checks canonical identity and discards unrelated parameters", options, async()=>{
  const base="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo";
  const page=await pageFor(siga112Fixture.replace("</body>",`<a id="ver-mais" href="${base}?sigla=TESTEOFI202600042&amp;token=discard">Ver Mais</a></body>`),"https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A");
  const result=await page.evaluate(()=>PonteCapture.buildCapture(PonteCapture.extractDocument(document,location.href)));
  assert.equal(result.metadata.movement_source,base+"?sigla=TESTEOFI202600042");assert.ok(!JSON.stringify(result).includes("discard"));
  await page.evaluate(()=>document.querySelector('#ver-mais').href="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla=OUTRO202600042");
  assert.equal(await page.evaluate(()=>PonteCapture.extractDocument(document,location.href).metadata.movement_source),undefined);
  await page.close();
});

test("history preserves full person identity displayed by the source tooltip", options, async()=>{
  const page=await pageFor();await page.addScriptTag({path:path.join(__dirname,"../../extension/history.js")});
  const result=await page.evaluate(html=>JSON.parse(PonteHistory.parse(new DOMParser().parseFromString(html,"text/html"),"TESTE-OFI-2026/00042")),historyFixture.replace("Origem fictícia",'<span title="NOME FICTÍCIO COMPLETO - TESTE1234">Origem</span>'));
  assert.equal(result[1].registered_person,"NOME FICTÍCIO COMPLETO - TESTE1234");await page.close();
});

test("sidebar access captures bold/nested level and explicit people without mixing their text", options, async()=>{
  for (const level of ['Limitado entre pessoas','<strong>Limitado entre pessoas</strong>','<div><b>Limitado entre pessoas</b></div>']) {
    const html=siga112Fixture.replace('</body>',`<div id="collapseNivelAcesso">${level}<ul><li>TESTE123400</li><li><span>TESTE205056</span></li><li hidden>TESTE999999</li></ul><input value="NAO_CAPTURAR"><button>Alterar acesso</button></div></body>`);
    const page=await pageFor(html,'https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A');
    const capture=await page.evaluate(()=>PonteCapture.buildCapture(PonteCapture.extractDocument(document,location.href)));
    assert.equal(capture.metadata.access_level,'Limitado entre pessoas');assert.equal(capture.metadata.access_people,'TESTE123400; TESTE205056');
    assert.ok(!JSON.stringify(capture).includes('TESTE999999'));assert.ok(!JSON.stringify(capture).includes('NAO_CAPTURAR'));assert.ok(!capture.metadata.access_level.includes('Alterar'));
    await page.close();
  }
});
test("access list with unknown structure is explicitly pending instead of silently partial",options,async()=>{
  const page=await pageFor(siga112Fixture.replace('</body>','<div id="collapseNivelAcesso"><b>Limitado entre pessoas</b><ul><li>TESTE123400</li><li>Formato desconhecido</li></ul></div></body>'),'https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00042-A');
  const capture=await page.evaluate(()=>PonteCapture.buildCapture(PonteCapture.extractDocument(document,location.href)));
  assert.equal(capture.metadata.access_level,'Limitado entre pessoas');assert.equal(capture.metadata.access_people,undefined);assert.ok(capture.coverage.pending.some(p=>p.includes('Lista do nível de acesso não importada')));await page.close();
});

const FOUND = { protocol_version: 1, ok: true, found: true, document_id: 123, imported_at: "10/11/2026 14:30" };
const panelState = page => page.evaluate(() => {
  const root = window.__testShadow, shown = selector => { const el = root.querySelector(selector); return !!el && !el.hidden; };
  return { line: shown(".ponte") ? root.querySelector(".ponte span").textContent : "", retry: shown(".ponte button.link"), open: shown(".open-row"), openNote: shown(".open-note") ? root.querySelector(".open-note").textContent : "", layout: shown(".layout"), button: root.querySelector("button").textContent, disabled: root.querySelector("button").disabled, calls: window.__calls.map(call => call.action) };
});
test("status before any click: document not in the Ponte shows the normal button and asks only once", options, async () => {
  const page = await pageFor();
  await instrumentContent(page);
  await page.waitForTimeout(900);
  let state = await panelState(page);
  assert.deepEqual(state.calls, ["ponte.ping", "ponte.lookup"]);
  assert.equal(await page.evaluate(() => window.__calls[1].document_number), "ORG-PRO-2026/01234");
  assert.deepEqual([state.line.trim(), state.open, state.button], ["Ainda não está no Ponte.", false, "Importar para o assistente"]);
  // DOM noise does not start new checks; the same document is not asked again.
  await page.evaluate(() => { document.querySelector(".document-text p").textContent = "Outro texto"; });
  await page.waitForTimeout(1200);
  assert.equal((await panelState(page)).calls.length, 2);
  // Another document (the page is dynamic) is asked once more.
  await page.evaluate(() => { document.getElementById("official-number").textContent = "ORG-PRO-2026/01235"; history.pushState({}, "", "/doc?sigla=ORG-PRO-2026%2F01235"); });
  await page.waitForTimeout(1000);
  state = await panelState(page);
  assert.equal(state.calls.length, 4);
  assert.equal(await page.evaluate(() => window.__calls[3].document_number), "ORG-PRO-2026/01235");
  await page.close();
});
test("status before any click: a document already in the Ponte says so and can be opened", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { lookup: FOUND });
  await page.waitForTimeout(900);
  const state = await panelState(page);
  assert.equal(state.line, "Já está no Ponte — importado em 10/11/2026 14:30.");
  assert.deepEqual([state.open, state.button, state.disabled, state.retry], [true, "Importar de novo", false, false]);
  await page.evaluate(() => { window.__native.open = { protocol_version: 1, ok: false, error: { code: "unknown_document", message: "Este documento não está mais no acervo do Ponte." } }; });
  await page.evaluate(() => window.__testShadow.querySelector(".open-row button").click());
  await page.waitForTimeout(100);
  assert.deepEqual(await page.evaluate(() => window.__calls.at(-1)), { action: "ponte.open", document_id: 123 });
  assert.equal((await panelState(page)).openNote, "Este documento não está mais no acervo do Ponte.");
  await page.close();
});
test("status before any click: Ponte unreachable warns, keeps the button usable and retries on request", options, async () => {
  const unreachable = { protocol_version: 1, ok: false, error: { code: "HOST_MISSING", message: "O Ponte não foi encontrado neste computador. Abra o Ponte uma vez e reinicie o navegador." } };
  const page = await pageFor();
  await instrumentContent(page, { ping: unreachable });
  await page.waitForTimeout(900);
  let state = await panelState(page);
  assert.ok(state.line.startsWith("O Ponte não foi encontrado neste computador."), state.line);
  assert.deepEqual([state.retry, state.open, state.disabled, state.button], [true, false, false, "Importar para o assistente"]);
  assert.deepEqual(state.calls, ["ponte.ping"]);
  await page.evaluate(found => { window.__native.ping = { protocol_version: 1, ok: true, actions: ["lookup", "open_document"] }; window.__native.lookup = found; }, FOUND);
  await page.evaluate(() => window.__testShadow.querySelector(".ponte button.link").click());
  await page.waitForTimeout(900);
  state = await panelState(page);
  assert.equal(state.line, "Já está no Ponte — importado em 10/11/2026 14:30.");
  assert.equal(state.retry, false);
  await page.close();
});
test("status before any click: an old Ponte only gets a discreet hint and no open link", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: { protocol_version: 1, ok: true, app_version: "0.28.0" } });
  await page.waitForTimeout(900);
  const state = await panelState(page);
  assert.equal(state.line, "Atualize o Ponte para ver se o documento já foi importado.");
  assert.deepEqual([state.retry, state.open, state.button, state.disabled], [false, false, "Importar para o assistente", false]);
  assert.deepEqual(state.calls, ["ponte.ping"]);
  await page.close();
});
test("status check that takes more than 5 s just hides the status line", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: "hang" });
  await page.waitForTimeout(6000);
  const state = await panelState(page);
  assert.deepEqual([state.line, state.retry, state.disabled], ["", false, false]);
  assert.deepEqual(state.calls, ["ponte.ping"]);
  await page.close();
});
test("after an import the status is asked again and the receipt gets an open link", options, async () => {
  const page = await pageFor();
  await instrumentContent(page);
  await page.waitForTimeout(900);
  await page.evaluate(() => window.__testShadow.querySelector("button").click());
  await page.evaluate(found => { window.__native.lookup = found; }, FOUND);
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: true, request_id: window.__requests[0].request_id, document_id: 123, capture_id: 1, duplicate: false, outcome: "imported", message: "Salvo", ui_opened: true, file_notes: ["a.pdf: x", "b.pdf: y"] }));
  await page.waitForTimeout(900);
  const state = await panelState(page);
  assert.deepEqual(state.calls, ["ponte.ping", "ponte.lookup", "ponte.ping", "ponte.lookup"]);
  const text = await page.evaluate(() => window.__testShadow.querySelector('[role="status"]').textContent);
  assert.ok(text.startsWith("Importado. O Ponte abriu o documento."), text);
  assert.ok(text.includes("2 arquivos não baixados: a.pdf: x; b.pdf: y"), text);
  assert.deepEqual([state.open, state.line, state.button], [true, "", "Importar de novo"]);
  await page.evaluate(() => window.__testShadow.querySelector(".open-row button").click());
  await page.waitForTimeout(100);
  assert.deepEqual(await page.evaluate(() => window.__calls.at(-1)), { action: "ponte.open", document_id: 123 });
  await page.close();
});
test("keyboard shortcut acts like the button, only from the extension and never twice at once", options, async () => {
  const page = await pageFor();
  await instrumentContent(page);
  await page.evaluate(() => window.__onMessage({ action: "ponte.shortcut_import" }, { id: "another-extension" }));
  await page.evaluate(() => window.__onMessage({ action: "ponte.shortcut_import" }, { id: "test-extension", tab: { id: 1 } }));
  await page.evaluate(() => window.__onMessage({ action: "ponte.shortcut_import", extra: true }, { id: "test-extension" }));
  assert.equal(await page.evaluate(() => window.__requests.length), 0);
  await page.evaluate(() => window.__onMessage({ action: "ponte.shortcut_import" }, { id: "test-extension" }));
  await page.evaluate(() => window.__onMessage({ action: "ponte.shortcut_import" }, { id: "test-extension" }));
  assert.equal(await page.evaluate(() => window.__requests.length), 1);
  assert.equal((await panelState(page)).disabled, true);
  await page.close();
  // Outside a document page the button is disabled and nothing is sent.
  const other = await pageFor("<main><h1>Qualquer página</h1></main>", "https://www.sigadoc.mt.gov.br/sigaex/app/principal");
  await instrumentContent(other);
  await other.evaluate(() => window.__onMessage({ action: "ponte.shortcut_import" }, { id: "test-extension" }));
  assert.equal(await other.evaluate(() => window.__requests.length), 0);
  assert.equal((await panelState(other)).calls.length, 0);
  await other.close();
});
test("a document page whose content area is not found says so; list pages and complete pages do not", options, async () => {
  let page = await pageFor("<main data-document-detail><dl><dt>Documento</dt><dd>ORG-PRO-2026/01234</dd><dt>Assunto</dt><dd>Assunto reconhecido</dd></dl></main>");
  await instrumentContent(page);
  assert.equal(await page.evaluate(() => { const el = window.__testShadow.querySelector(".layout"); return !el.hidden && el.textContent; }), "O Ponte não reconheceu esta página do SIGADOC. O site pode ter mudado; avise quem cuida do Ponte.");
  await page.close();
  page = await pageFor();
  await instrumentContent(page);
  assert.equal((await panelState(page)).layout, false);
  await page.close();
  page = await pageFor("<main><h1>Resultados da pesquisa</h1><dl><dt>Documento</dt><dd>ORG-PRO-2026/01234</dd><dt>Assunto</dt><dd>Resultado de pesquisa</dd></dl></main>");
  await instrumentContent(page);
  const state = await panelState(page);
  assert.deepEqual([state.layout, state.disabled], [false, true]);
  assert.match(await page.evaluate(() => window.__testShadow.querySelector(".identity").textContent), /Abra um documento/);
  await page.close();
});

// ---- 0.3.0: note and priority at import ----
const EXTRAS_PING = { protocol_version: 1, ok: true, app_version: "0.31.0", actions: ["lookup", "open_document", "import_extras"] };
const extrasState = page => page.evaluate(() => {
  const root = window.__testShadow, row = root.querySelector(".extras-row"), box = root.querySelector(".extras-box"), area = root.querySelector("textarea"), count = root.querySelector(".extras-count");
  const visible = el => { const style = getComputedStyle(el); return !el.hidden && style.display !== "none" && style.visibility !== "hidden"; };
  return { link: visible(row), linkText: row.querySelector("button").textContent, box: visible(box), note: area.value, priority: root.querySelector("select").value, locked: area.disabled && root.querySelector("select").disabled, count: visible(count) ? count.textContent : "", status: root.querySelector('[role="status"]').textContent };
});
const openExtras = page => page.evaluate(() => window.__testShadow.querySelector(".extras-row button").click());
const typeExtras = (page, note, priority) => page.evaluate(([note, priority]) => {
  const root = window.__testShadow, area = root.querySelector("textarea");
  area.value = note; area.dispatchEvent(new Event("input"));
  root.querySelector("select").value = priority;
}, [note, priority]);
const clickImport = page => page.evaluate(() => window.__testShadow.querySelector("button").click());
const answerImport = (page, extra = {}) => page.evaluate(extra => window.__callbacks.shift()({ protocol_version: 1, ok: true, request_id: window.__requests.at(-1).request_id, document_id: 5, capture_id: 1, duplicate: false, outcome: "imported", message: "Salvo", ui_opened: true, ...extra }), extra);
test("note and priority: the link exists only for a Ponte that takes them, and the controls are what was specified", options, async () => {
  let page = await pageFor();
  await instrumentContent(page);
  await page.waitForTimeout(900);
  let state = await extrasState(page);
  assert.deepEqual([state.link, state.box], [false, false]);
  await page.close();
  page = await pageFor();
  await instrumentContent(page, { ping: EXTRAS_PING });
  await page.waitForTimeout(900);
  state = await extrasState(page);
  assert.deepEqual([state.link, state.linkText, state.box], [true, "Adicionar nota ou prioridade", false]);
  await openExtras(page);
  state = await extrasState(page);
  assert.deepEqual([state.box, state.priority, state.count], [true, "", ""]);
  const spec = await page.evaluate(() => {
    const root = window.__testShadow, area = root.querySelector("textarea");
    return { rows: area.rows, max: area.maxLength, placeholder: area.placeholder, labels: [...root.querySelectorAll(".extras-box label")].map(label => label.firstChild.textContent), options: [...root.querySelectorAll("option")].map(option => [option.value, option.textContent]) };
  });
  assert.deepEqual(spec, { rows: 3, max: 2000, placeholder: "Ex.: responder até sexta; falar com o setor de protocolo", labels: ["Nota para o Ponte", "Prioridade"], options: [["", "Não mudar"], ["baixa", "Baixa"], ["normal", "Normal"], ["alta", "Alta"], ["urgente", "Urgente"]] });
  // The counter only appears above 1800 characters.
  await typeExtras(page, "a".repeat(1800), "");
  assert.equal((await extrasState(page)).count, "");
  await typeExtras(page, "a".repeat(1801), "");
  assert.equal((await extrasState(page)).count, "1801 de 2000");
  await page.close();
});
test("note and priority: the size of the panel before any click does not depend on the Ponte's answer", options, async () => {
  const heights = [];
  const answers = [undefined, { ping: EXTRAS_PING }, { ping: EXTRAS_PING, lookup: FOUND }, { ping: { protocol_version: 1, ok: true, app_version: "0.28.0" } }, { ping: { protocol_version: 1, ok: false, error: { code: "HOST_MISSING", message: "O Ponte não foi encontrado neste computador." } } }];
  for (const answer of answers) {
    const page = await pageFor();
    await instrumentContent(page, answer);
    await page.waitForTimeout(900);
    heights.push(await page.evaluate(() => { const rect = window.__testShadow.querySelector("section").getBoundingClientRect(); return [Math.round(rect.width), Math.round(rect.height)]; }));
    await page.close();
  }
  assert.ok(heights.every(size => size[0] === heights[0][0] && size[1] === heights[0][1]), JSON.stringify(heights));
});
test("note and priority go with the import, are cleared after it and say what was done", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: EXTRAS_PING });
  await page.waitForTimeout(900);
  await openExtras(page);
  await typeExtras(page, "  Responder até sexta  ", "alta");
  await clickImport(page);
  const messages = await page.evaluate(() => window.__imports);
  assert.equal(messages.length, 1);
  assert.deepEqual(Object.keys(messages[0]), ["action", "capture", "extras"]);
  assert.deepEqual(messages[0].extras, { note: "Responder até sexta", priority: "alta" });
  assert.equal((await extrasState(page)).locked, true);
  await answerImport(page, { extras_applied: true });
  await page.waitForTimeout(100);
  const state = await extrasState(page);
  assert.equal(state.status, "Importado. O Ponte abriu o documento. Nota e prioridade enviadas.");
  assert.deepEqual([state.note, state.priority, state.box, state.locked], ["", "", false, false]);
  // Nothing typed: the message carries no extras at all.
  await clickImport(page);
  assert.deepEqual(Object.keys(await page.evaluate(() => window.__imports.at(-1))), ["action", "capture"]);
  await page.close();
});
test("note only and priority only say so; a message of the Ponte replaces the sentence when nothing was applied", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: EXTRAS_PING });
  await page.waitForTimeout(900);
  await openExtras(page);
  await typeExtras(page, "Só a nota", "");
  await clickImport(page);
  assert.deepEqual(await page.evaluate(() => window.__imports.at(-1).extras), { note: "Só a nota" });
  await answerImport(page, { extras_applied: true });
  await page.waitForTimeout(50);
  assert.equal((await extrasState(page)).status, "Importado. O Ponte abriu o documento. Nota enviada.");
  await openExtras(page);
  await typeExtras(page, "", "urgente");
  await clickImport(page);
  assert.deepEqual(await page.evaluate(() => window.__imports.at(-1).extras), { priority: "urgente" });
  await answerImport(page, { extras_applied: true });
  await page.waitForTimeout(50);
  assert.equal((await extrasState(page)).status, "Importado. O Ponte abriu o documento. Prioridade enviada.");
  await openExtras(page);
  await typeExtras(page, "Outra", "baixa");
  await clickImport(page);
  await answerImport(page, { extras_applied: false, extras_message: "A nota não foi aplicada: o documento está na lixeira." });
  await page.waitForTimeout(50);
  assert.equal((await extrasState(page)).status, "Importado. O Ponte abriu o documento. A nota não foi aplicada: o documento está na lixeira.");
  await openExtras(page);
  await typeExtras(page, "Mais uma", "");
  await clickImport(page);
  await answerImport(page, { extras_skipped: true });
  await page.waitForTimeout(50);
  assert.equal((await extrasState(page)).status, "Importado. O Ponte abriu o documento. O Ponte instalado não recebe nota nem prioridade; a importação foi feita sem elas. Atualize o Ponte.");
  await page.close();
});
test("note and priority survive an import error and the retry sends exactly the same ones", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: EXTRAS_PING });
  await page.waitForTimeout(900);
  await openExtras(page);
  await typeExtras(page, "Falar com o protocolo", "normal");
  await clickImport(page);
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: false, error: { code: "HOST_CONNECTION", message: "Confirmação perdida" } }));
  await page.waitForTimeout(50);
  let state = await extrasState(page);
  assert.deepEqual([state.note, state.priority, state.box, state.locked], ["Falar com o protocolo", "normal", true, true]);
  assert.equal(await page.evaluate(() => window.__testShadow.querySelector("button").textContent), "Repetir importação");
  await clickImport(page);
  const [first, second] = await page.evaluate(() => window.__imports);
  assert.deepEqual(second.extras, first.extras);
  assert.deepEqual(second.extras, { note: "Falar com o protocolo", priority: "normal" });
  assert.equal(second.capture.request_id, first.capture.request_id);
  await answerImport(page, { extras_applied: true });
  await page.waitForTimeout(50);
  state = await extrasState(page);
  assert.equal(state.status, "Importado. O Ponte abriu o documento. Nota e prioridade enviadas.");
  assert.deepEqual([state.note, state.priority], ["", ""]);
  await page.close();
});
test("note and priority belong to the document on screen: another document number clears them", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: EXTRAS_PING });
  await page.waitForTimeout(900);
  await openExtras(page);
  await typeExtras(page, "Nota do primeiro", "alta");
  // Same document, page changes elsewhere: kept.
  await page.evaluate(() => { document.querySelector(".document-text p").textContent = "Outro texto"; });
  await page.waitForTimeout(400);
  assert.deepEqual((await extrasState(page)).note, "Nota do primeiro");
  await page.evaluate(() => { document.getElementById("official-number").textContent = "ORG-PRO-2026/01235"; history.pushState({}, "", "/doc?sigla=ORG-PRO-2026%2F01235"); });
  await page.waitForTimeout(500);
  const state = await extrasState(page);
  assert.deepEqual([state.note, state.priority, state.box], ["", "", false]);
  await page.close();
});
test("note and priority of a failed import are not sent with another document", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: EXTRAS_PING });
  await page.waitForTimeout(900);
  await openExtras(page);
  await typeExtras(page, "Nota privada", "alta");
  await clickImport(page);
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: false, error: { code: "HOST_CONNECTION", message: "Confirmação perdida" } }));
  await page.evaluate(() => { document.getElementById("official-number").textContent = "ORG-PRO-2026/01235"; history.pushState({}, "", "/doc?sigla=ORG-PRO-2026%2F01235"); });
  await page.waitForTimeout(500);
  assert.deepEqual([(await extrasState(page)).note, (await extrasState(page)).locked], ["", false]);
  await clickImport(page);
  const [, second] = await page.evaluate(() => window.__imports);
  assert.equal(second.capture.document_number, "ORG-PRO-2026/01235");
  assert.equal("extras" in second, false);
  await page.close();
});
test("the keyboard shortcut imports with whatever is in the controls", options, async () => {
  const page = await pageFor();
  await instrumentContent(page, { ping: EXTRAS_PING });
  await page.waitForTimeout(900);
  await openExtras(page);
  await typeExtras(page, "Pelo atalho", "baixa");
  await page.evaluate(() => window.__onMessage({ action: "ponte.shortcut_import" }, { id: "test-extension" }));
  assert.deepEqual(await page.evaluate(() => window.__imports.map(message => message.extras)), [{ note: "Pelo atalho", priority: "baixa" }]);
  await page.close();
});

// ---- 0.3.0: recent imports in the popup ----
const popupHtml = fs.readFileSync(path.join(__dirname, "../../extension/popup.html"), "utf8").replace(/<script src="popup.js"><\/script>/, "");
async function popupPage(items, ping) {
  const page = await browser.newPage();
  await page.route("**/*", route => route.fulfill({ status: 200, contentType: "text/html; charset=utf-8", body: popupHtml }));
  await page.goto("https://www.sigadoc.mt.gov.br/popup.html");
  await page.evaluate(([items, ping]) => {
    window.__sent = [];
    window.__items = items;
    window.chrome = { runtime: { id: "test-extension", lastError: null, sendMessage: (message, callback) => {
      window.__sent.push(message);
      const reply = { "ponte.ping": ping, "ponte.recent": { protocol_version: 1, ok: true, items: window.__items }, "ponte.recent_clear": { protocol_version: 1, ok: true }, "ponte.open": { protocol_version: 1, ok: true, document_id: message.document_id, ui_opened: false } }[message.action];
      setTimeout(() => callback(reply), 0);
    } } };
  }, [items, ping]);
  await page.addScriptTag({ path: path.join(__dirname, "../../extension/popup.js") });
  await page.waitForTimeout(150);
  return page;
}
test("popup: recent imports with number, subject, time and result; open link only where it can work; clear", options, async () => {
  const long = "Assunto muito longo ".repeat(10).trim();
  const items = [
    { number: "ORG-PRO-2026/00003", title: "Terceiro <b>assunto</b>", time: "2026-11-11T17:32:00.000Z", outcome: "imported", document_id: 7 },
    { number: "ORG-PRO-2026/00002", title: long, time: "2026-11-11T17:20:00.000Z", outcome: "already", document_id: 6 },
    { number: "ORG-PRO-2026/00001", title: "Primeiro", time: "2026-11-11T17:10:00.000Z", outcome: "failed" }
  ];
  const page = await popupPage(items, EXTRAS_PING);
  const rows = await page.evaluate(() => [...document.querySelectorAll("#recent li")].map(li => ({ text: li.innerText.replace(/\s+/g, " ").trim(), open: li.querySelectorAll("button").length, html: li.innerHTML.includes("<b>") })));
  assert.equal(rows.length, 3);
  assert.ok(rows[0].text.startsWith("ORG-PRO-2026/00003") && rows[0].text.includes("Terceiro <b>assunto</b>") && rows[0].text.includes("Importado") && /\d\d:\d\d/.test(rows[0].text), rows[0].text);
  assert.ok(!rows[0].html);
  assert.ok(rows[1].text.includes("Já estava no Ponte") && rows[1].text.includes("…") && !rows[1].text.includes(long));
  assert.ok(rows[2].text.includes("Não concluída"));
  assert.deepEqual(rows.map(row => row.open), [1, 1, 0]);
  assert.equal(await page.evaluate(() => document.getElementById("recent-empty").hidden), true);
  assert.equal(await page.evaluate(() => document.getElementById("extras-old").hidden), true);
  await page.evaluate(() => document.querySelector("#recent li button").click());
  await page.waitForTimeout(100);
  assert.deepEqual(await page.evaluate(() => window.__sent.at(-1)), { action: "ponte.open", document_id: 7 });
  assert.equal(await page.evaluate(() => document.getElementById("recent-note").textContent), "Abra o Ponte para ver o documento.");
  await page.evaluate(() => document.getElementById("recent-clear").click());
  await page.waitForTimeout(100);
  assert.deepEqual(await page.evaluate(() => window.__sent.at(-1)), { action: "ponte.recent_clear" });
  assert.equal(await page.evaluate(() => document.querySelectorAll("#recent li").length), 0);
  assert.equal(await page.evaluate(() => document.getElementById("recent-empty").textContent), "Nenhuma importação nesta sessão do navegador.");
  assert.equal(await page.evaluate(() => document.getElementById("recent-clear").hidden), true);
  await page.close();
});
test("popup: an older Ponte gets the hint about note and priority and no open link", options, async () => {
  const items = [{ number: "ORG-PRO-2026/00003", title: "Assunto", time: "2026-11-11T17:32:00.000Z", outcome: "imported", document_id: 7 }];
  const page = await popupPage(items, { protocol_version: 1, ok: true, app_version: "0.30.0", actions: ["lookup"] });
  assert.equal(await page.evaluate(() => { const el = document.getElementById("extras-old"); return !el.hidden && el.textContent; }), "Atualize o Ponte para enviar nota e prioridade ao importar.");
  assert.equal(await page.evaluate(() => document.querySelectorAll("#recent li button").length), 0);
  await page.close();
});
