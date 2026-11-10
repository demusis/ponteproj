const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const Core = require('../../extension/capture-core.js');
const Files = require('../../extension/files.js');
const pdf = fs.readFileSync(path.join(__dirname,'../fixtures/document.pdf'));
const reference = {url:Core.ORIGIN+'/arquivos/teste.pdf', label:'Anexo de exemplo', kind:'attachment'};
test('downloads only supported PDF references, with browser credentials and no redirects', async () => {
  let calls = 0;
  const result = await Files.download(reference, Core, async (url, options) => {
    calls++; assert.equal(url,reference.url); assert.equal(options.credentials,'include'); assert.equal(options.redirect,'error');
    return new Response(pdf,{headers:{'content-type':'application/pdf'}});
  });
  assert.equal(Buffer.from(result.data_base64,'base64').compare(pdf),0);
  for (const ref of [ {...reference,url:'https://evil.test/teste.pdf'}, {...reference,url:Core.ORIGIN+'/sigaex/app/mov/anexar.pdf'}, {...reference,kind:'reference'}, {...reference,url:reference.url+'?token=secret'} ]) {
    await assert.rejects(Files.download(ref,Core,async()=>{calls++;}));
  }
  assert.equal(calls,1);
});
test('rejects login HTML and oversized responses; records partial download failure without dropping page text', async () => {
  await assert.rejects(Files.download(reference,Core,async()=>new Response('<html>Login</html>')),/sem PDF/);
  await assert.rejects(Files.download(reference,Core,async()=>new Response(pdf,{headers:{'content-length':Files.MAX_FILE+1}})),/8 MiB/);
  const capture = {text:'Texto da ficha', references:[reference], coverage:{pending:[]}};
  const result = await Files.prepare(capture,Core,async()=>{throw new TypeError('network detail secret');});
  assert.equal(result.capture.text,capture.text); assert.equal(result.files.length,0);
  assert.match(result.capture.coverage.pending[0],/bloqueado/);
  assert.ok(!JSON.stringify(result).includes('secret'));
  assert.deepEqual(capture.coverage.pending,[]);
});
test('download bundle deduplicates references and refuses unbounded streaming bodies', async () => {
  let calls = 0;
  const capture = {references:[reference,reference],coverage:{pending:[]}};
  const result = await Files.prepare(capture,Core,async()=>{calls++;return new Response(pdf);});
  assert.equal(calls,1); assert.equal(result.files.length,1);
  await assert.rejects(Files.download(reference,Core,async()=>new Response(new Uint8Array(Files.MAX_FILE+1))),/8 MiB/);
});

function pdfOf(bytes) { const data = new Uint8Array(bytes); data.set(Buffer.from('%PDF-')); return data; }
const refs = count => Array.from({length:count}, (_, i) => ({url:Core.ORIGIN+'/arquivos/'+i+'.pdf', label:'Anexo '+i, kind:'attachment'}));
test('downloads at most three PDFs at a time and keeps the order of the page', async () => {
  let running = 0, peak = 0;
  const capture = {references:refs(7), coverage:{pending:[]}};
  const result = await Files.prepare(capture, Core, async url => {
    const index = Number(/([0-9]+)[.]pdf$/.exec(url)[1]);
    running++; peak = Math.max(peak, running);
    await new Promise(resolve => setTimeout(resolve, (7 - index) * 15));
    running--;
    return new Response(pdfOf(20 + index));
  });
  assert.equal(peak, 3);
  assert.deepEqual(result.files.map(file => file.url), capture.references.map(ref => ref.url));
  assert.deepEqual(result.capture.coverage.pending, []);
});
test('errors are reported per file in page order, and more than ten PDFs are cut off', async () => {
  const capture = {references:refs(12), coverage:{pending:[]}};
  let calls = 0;
  const result = await Files.prepare(capture, Core, async url => {
    calls++;
    if (url.endsWith('/1.pdf') || url.endsWith('/4.pdf')) return new Response('<html>Login</html>');
    return new Response(pdfOf(20));
  });
  assert.equal(calls, 10);
  assert.equal(result.files.length, 8);
  assert.equal(result.capture.coverage.pending.length, 3);
  assert.match(result.capture.coverage.pending[0], /^Anexo 1: .*sem PDF/);
  assert.match(result.capture.coverage.pending[1], /^Anexo 4: /);
  assert.match(result.capture.coverage.pending[2], /limite de 10 PDFs/);
});
test('the total size limit stops further downloads and drops the files that do not fit', async () => {
  const mib7 = 7 * 1024 * 1024;
  const capture = {references:refs(6), coverage:{pending:[]}};
  let calls = 0;
  const result = await Files.prepare(capture, Core, async () => { calls++; return new Response(pdfOf(mib7)); });
  // Once three files (21 MiB > 16 MiB) were in, no further download started: not all six were fetched.
  assert.ok(calls >= 3 && calls < 6, String(calls));
  assert.deepEqual(result.files.map(file => file.url), [capture.references[0].url, capture.references[1].url]);
  assert.deepEqual(result.capture.coverage.pending.map(note => note.split(':')[0]), ['Anexo 2', 'Anexo 3', 'Anexo 4', 'Anexo 5']);
  assert.ok(result.capture.coverage.pending.every(note => note.endsWith('Total de PDFs excede 16 MiB')));
});
