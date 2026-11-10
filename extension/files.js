(function(root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.PonteFiles = api;
})(globalThis, function() {
  'use strict';
  const MAX_FILE = 8 * 1024 * 1024, MAX_TOTAL = 16 * 1024 * 1024, MAX_FILES = 10, PARALLEL = 3;
  function downloadable(ref, Core) {
    if (!['original','attachment'].includes(ref.kind) || Core.cleanUrl(ref.url) !== ref.url) return false;
    const url = new URL(ref.url);
    return url.pathname === '/sigaex/app/arquivo/exibir' || (!url.search && !url.pathname.includes('/app/') && !url.pathname.includes(';') && /\.pdf$/i.test(url.pathname));
  }
  async function download(ref, Core, fetcher) {
    if (!downloadable(ref, Core)) throw new Error('Rota de arquivo ainda não suportada');
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 25000);
    try {
      const response = await fetcher(ref.url, {credentials:'include', redirect:'error', signal:controller.signal, cache:'no-store'});
      if (!response.ok || response.url && !Core.allowedOrigin(response.url)) throw new Error('Download recusado ou sessão expirada');
      if (Number(response.headers.get('content-length') || 0) > MAX_FILE) throw new Error('PDF excede 8 MiB');
      if (!response.body) throw new Error('Resposta vazia');
      const reader = response.body.getReader(), chunks = [];
      let size = 0;
      try {
        while (true) {
          const {done,value} = await reader.read(); if (done) break;
          size += value.length;
          if (size > MAX_FILE) { await reader.cancel(); throw new Error('PDF excede 8 MiB'); }
          chunks.push(value);
        }
      } finally { reader.releaseLock(); }
      const data = new Uint8Array(size); let offset = 0;
      for (const chunk of chunks) { data.set(chunk, offset); offset += chunk.length; }
      if (new TextDecoder().decode(data.slice(0,5)) !== '%PDF-') throw new Error('Resposta sem PDF; pode ser pré-visualização ou sessão expirada');
      let binary = '';
      for (let i=0; i<data.length; i+=8192) binary += String.fromCharCode(...data.subarray(i,i+8192));
      return {url:ref.url, data_base64:btoa(binary), size};
    } catch (error) {
      if (error.name === 'AbortError') throw new Error('Download excedeu 25 segundos');
      if (error instanceof TypeError) throw new Error('Download bloqueado, redirecionado ou sem conexão');
      throw error;
    } finally { clearTimeout(timer); }
  }
  async function prepare(capture, Core, fetcher = fetch) {
    const copy = JSON.parse(JSON.stringify(capture)), files = [];
    const refs = [...new Map(capture.references.filter(r => ['original','attachment'].includes(r.kind)).map(r => [r.url,r])).values()];
    const wanted = refs.slice(0, MAX_FILES), results = new Array(wanted.length);
    // At most PARALLEL downloads at a time; once the finished files already
    // exceed the total limit, no further download is started.
    let next = 0, finished = 0, stop = false;
    async function lane() {
      while (!stop && next < wanted.length) {
        const index = next++;
        try {
          const file = await download(wanted[index], Core, fetcher);
          results[index] = {file}; finished += file.size;
          if (finished > MAX_TOTAL) stop = true;
        } catch (error) { results[index] = {error}; }
      }
    }
    await Promise.all(Array.from({length:Math.min(PARALLEL, wanted.length)}, lane));
    // The result keeps the order of the page and applies the total limit in that order.
    let total = 0;
    wanted.forEach((ref, index) => {
      const entry = results[index];
      if (!entry) { copy.coverage.pending.push(`${ref.label}: Total de PDFs excede 16 MiB`); return; }
      if (entry.error) { copy.coverage.pending.push(`${ref.label}: ${entry.error.message}`); return; }
      const file = entry.file;
      if (total + file.size > MAX_TOTAL) { copy.coverage.pending.push(`${ref.label}: Total de PDFs excede 16 MiB`); return; }
      total += file.size; delete file.size; files.push(file);
    });
    if (refs.length > MAX_FILES) copy.coverage.pending.push('Demais arquivos não baixados: limite de 10 PDFs por importação');
    return {protocol_version:1, action:'import_bundle', capture:copy, files};
  }
  return {downloadable, download, prepare, MAX_FILE, MAX_TOTAL, MAX_FILES, PARALLEL};
});
