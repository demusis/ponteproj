# Atualização 0.9.3 — Correções da revisão de código

**Data:** 2026-11-04
**Extensão:** 0.1.8 (sem mudanças; não precisa recarregar)

## O que muda

- **"cópia baixada: ler no Ponte"** (em Mais ▸ Links encontrados na página) agora abre o PDF diretamente no leitor do Ponte, em vez de só trocar para a aba Documento.
- **Rótulos das cópias locais:** em Documento ▸ Arquivos, um PDF baixado aparece como "cópia do documento original" ou "cópia de anexo". "Original no SIGADOC" / "Anexo no SIGADOC" ficaram só para os links, que exigem login no SIGADOC.
- **Lista de arquivos em painel estreito:** a dica ⓘ vem antes do texto e a linha quebra, em vez de a dica sumir fora da área visível.
- **Barra colorida dos cartões** da lista de documentos voltou a ter o espaçamento original em relação ao texto.
- **Divisória da lista de documentos:** os limites (180 px até 60% da janela) passaram a ser definidos num único lugar, usado tanto pela janela quanto pelo teste automatizado, que agora também confere o limite máximo.
- **Menos consultas ao acervo:** a aba Mais só busca os arquivos do documento quando há links na página.

## Para desenvolvedores

- `App::library_panel()` e as constantes `LIBRARY_DEFAULT_WIDTH`, `LIBRARY_MIN_WIDTH` e `LIBRARY_MAX_FRACTION` em `src/app.rs` são a única fonte dos limites da divisória.
- `labels::downloaded_kind` rotula cópias locais; `labels::reference_kind` rotula links.
- A nota da 0.9.2 foi corrigida: os rótulos da ficha já quebravam linha em layout vertical; a correção de então foi nos quadros estreitos.
