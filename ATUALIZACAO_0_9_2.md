# Atualização 0.9.2 — Ajustes de layout e clareza dos PDFs

**Data:** 2026-11-04
**Extensão:** 0.1.8 (sem mudanças; não precisa recarregar)

## O que muda

- **Quadros ocupam toda a largura** e acompanham o redimensionamento da janela (Resumo, Tarefas, atendimentos ligados, instalação na tela inicial, sugestões da IA, arquivos, dados do SIGADOC, histórico, vínculos, lixeira, conflitos, prévia e alertas do SAIOP).
- **Textos quebram linha** em vez de serem cortados: título da ficha, rótulos longos do Resumo e os cartões da lista de documentos (o cartão cresce com o título; a barra colorida acompanha).
- **Divisória da lista de documentos** com limites mais amplos (de 180 px até 60% da janela) para o arrasto ser perceptível; teste automatizado garante que o arrasto redimensiona o painel.
- **PDF original sem redundância confusa:** o botão do Resumo passa a ser **"Ler PDF (cópia no Ponte)"**, com explicação; em **Documento ▸ Arquivos**, cada PDF baixado mostra sua origem e ganha **"Abrir no SIGADOC"** ao lado de "Ler no Ponte"; em **Mais ▸ Links encontrados na página**, os tipos viraram "Original no SIGADOC" / "Anexo no SIGADOC" e, quando o PDF foi baixado, aparece o atalho "cópia baixada: ler no Ponte". A dica ⓘ explica que os links exigem login no SIGADOC e que as cópias locais ficam em Documento ▸ Arquivos.

## Para desenvolvedores

- Teste `app::tests::library_divider_drags_and_central_width_fits_the_window` reproduz o layout da janela com `Context::run_ui` e eventos de ponteiro sintéticos.
