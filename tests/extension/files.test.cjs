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
