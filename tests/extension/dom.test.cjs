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
const URL = "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=SEPLAG-PRO-2026%2F01234&access_token=source-secret#fragmento";
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
  assert.ok(capture.coverage.pending.some(value => value.includes("Extração integral")));
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
    [fixture, path + "?SIGLA=SEPLAG-PRO-2026%2F01234-A"]
  ]) {
    const page = await pageFor(html, url);
    assert.equal(await page.evaluate(() => { try { PonteCapture.extractDocument(document, location.href); return "accepted"; } catch { return "rejected"; } }), "rejected");
    await page.close();
  }
});
test("synthetic DOM adapter extracts explicit fields, excludes secrets, distinguishes links from files", options, async () => {
  const page = await pageFor();
  const capture = await page.evaluate(() => PonteCapture.buildCapture(PonteCapture.extractDocument(document, location.href)));
  assert.equal(capture.document_number, "SEPLAG-PRO-2026/01234");
  assert.equal(capture.title, "Aquisição de material de expediente");
  assert.equal(capture.metadata.document_type, "Processo administrativo");
  assert.equal(capture.metadata.origin, "Unidade de exemplo");
  assert.deepEqual(capture.references.map(value => value.kind), ["original", "attachment", "antecedent"]);
  assert.ok(capture.text.includes("Solicita-se a análise"));
  assert.equal(capture.text.match(/Justificativa de exemplo/g).length, 1);
  assert.ok(!JSON.stringify(capture).includes("secret"));
  assert.ok(!capture.text.includes("Ficha de documento"));
  assert.ok(!Object.hasOwn(capture, "files"));
  assert.ok(capture.coverage.pending.some(value => value.includes("nenhum arquivo")));
  assert.ok(capture.coverage.pending.some(value => value.includes("DOM autenticado real")));
  assert.ok(capture.coverage.pending.some(value => value.includes("iframe")));
  await page.close();
});
test("login, lists, unknown or conflicting identity are refused; no raw-body fallback", options, async () => {
  for (const [html, url, expected] of [
    ['<main><input type="password" value="password-secret"></main>', URL, "LOGIN_PAGE"],
    ['<main><h1>Resultados da pesquisa</h1><dl><dt>Documento</dt><dd>SEPLAG-PRO-2026/01234</dd><dt>Assunto</dt><dd>Resultado de pesquisa</dd></dl></main>', URL, "LIST_PAGE"],
    ['<main><h1>Qualquer página</h1><p>SEPLAG-PRO-2026/01234</p><p>Título possível</p></main>', URL, "DOCUMENT_IDENTITY"],
    [fixture.replace('</dl>', '<dt>Processo</dt><dd>SEPLAG-PRO-2026/99999</dd></dl>'), URL, "DOCUMENT_IDENTITY"],
    [fixture.replace('<main ', '<div hidden><main ').replace('</main>', '</main></div>'), URL, "UNRECOGNIZED_PAGE"],
    [fixture, "https://www.sigadoc.mt.gov.br/login", "LOGIN_PAGE"]
  ]) {
    const page = await pageFor(html, url);
    assert.equal(await page.evaluate(() => { try { PonteCapture.extractDocument(document, location.href); return "accepted"; } catch (error) { return error.code; } }), expected);
    await page.close();
  }
  const page = await pageFor('<main data-document-detail><dl><dt>Documento</dt><dd>SEPLAG-PRO-2026/01234</dd><dt>Assunto</dt><dd>Assunto reconhecido</dd></dl><p>Texto fora de área documental; não copiar</p></main>');
  const snapshot = await page.evaluate(() => PonteCapture.extractDocument(document, location.href));
  assert.equal(snapshot.text, "");
  assert.ok(snapshot.coverage.pending.some(value => value.includes("corpo geral")));
  await page.close();
});
async function instrumentContent(page) {
  await page.evaluate(() => {
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (options) { const result = attach.call(this, options); window.__testShadow = result; return result; };
    window.__requests = [];
    window.__callbacks = [];
    window.chrome = { runtime: { lastError: null, sendMessage: (message, callback) => { window.__requests.push(message.capture); window.__callbacks.push(callback); } } };
  });
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
  await page.evaluate(() => { document.getElementById("official-number").textContent = "SEPLAG-PRO-2026/01235"; document.getElementById("official-title").textContent = "Segundo documento"; history.pushState({}, "", "/doc?sigla=SEPLAG-PRO-2026%2F01235"); });
  await page.waitForTimeout(300);
  await page.evaluate(() => window.__callbacks.shift()({ protocol_version: 1, ok: true, request_id: window.__requests[0].request_id, document_id: 1, capture_id: 1, duplicate: false, message: "Salvo", ui_opened: false }));
  await page.waitForTimeout(50);
  const text = await page.evaluate(() => window.__testShadow.querySelector('[role="status"]').textContent);
  assert.ok(text.startsWith("SEPLAG-PRO-2026/01234:"), text);
  assert.equal(await page.evaluate(() => window.__requests[0].document_number), "SEPLAG-PRO-2026/01234");
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
    capture.metadata.movement_source="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla=SEPLAGPRO202601234";
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
  assert.equal(capture.metadata.access_level,'Limitado entre pessoas');assert.equal(capture.metadata.access_people,undefined);assert.ok(capture.coverage.pending.some(p=>p.includes('Lista do nível de acesso não capturada')));await page.close();
});
