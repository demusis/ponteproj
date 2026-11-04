# Atualização 0.14.0 — Importação idêntica avisa e não grava

**Data:** 2026-11-04
**Extensão:** 0.1.9 — **recarregue a extensão** depois de instalar (chrome://extensions ▸ ícone de recarregar no Ponte, ou feche e abra o navegador). A versão 0.1.8 continua funcionando, mas mostra o caso abaixo em verde, com o texto antigo.
**Acervo:** sem mudança de esquema (continua na versão 10). Em equipe, atualize o **computador principal** primeiro: é ele quem decide se a importação é idêntica.

## O que muda

Antes, importar um documento cuja página não tinha mudado era tratado como sucesso: a extensão dizia em verde "já estava importado; nada mudou", o Ponte abria o documento e o acervo ainda registrava o pedido e movia o documento para o topo da lista.

- **Importação idêntica é ignorada, com aviso.** Se a página do SIGADOC está exatamente igual à versão em uso no Ponte (texto, PDFs e dados), **nada é gravado** e o painel da extensão mostra em laranja: *"Importação ignorada: o SIGADOC mostra o mesmo conteúdo já importado em 04/11/2026 12:38. Nada foi gravado. Se o documento mudou, atualize a página (F5) e importe de novo."* O Ponte **não abre** e o documento não muda de lugar na lista.
- **Documento alterado continua importando normalmente**, como nova versão (as anteriores ficam em Mais ▸ Versões importadas). Importar várias vezes ao longo do tempo é o uso esperado.
- **Página igual a uma versão anterior** (o SIGADOC voltou atrás): nada novo é gravado, essa versão volta a ser a atual no Ponte, e o aviso laranja diz isso.
- **Repetir importação** (quando a confirmação se perdeu) responde *"Importação confirmada"*, para não confundir com o caso acima.
- **Menu ▾ ▸ Sobre o Ponte**: janela com a versão do programa e da extensão, a autoria (Carlo Ralph De Musis e João Gabriel Bezerra Estrela), a licença MIT e o endereço do projeto. O README ganhou a seção "Autoria e licença".

## Para desenvolvedores

- `model::ImportReceipt` ganha `outcome: Option<ImportOutcome>` (`imported` | `confirmed` | `unchanged`), `existing_captured_at` e `previous_version`, todos com `serde(default)`; `duplicate` continua (true para confirmed/unchanged). `effective_outcome()` interpreta recibos de anfitriões anteriores.
- `Store::import_capture_with_files`: conteúdo idêntico à captura atual devolve `Unchanged` sem gravar nada (sem `requests`, sem `last_seen_at`); idêntico a uma captura anterior segue o fluxo antigo com `previous_version: true`. Teste novo `identical_content_is_ignored_and_an_older_version_can_become_current_again`.
- `native::handle_request`: resposta com `outcome`; no caso `unchanged`, `message` composta com a data (`labels::datetime`), `ui_opened: false` e o aplicativo não é aberto. Teste novo `unchanged_import_warns_without_opening_the_app`.
- Extensão 0.1.9: `validateReceipt` aceita `outcome` (só os três valores), `importOutcome()` com `legacy_duplicate` para anfitriões antigos; `content.js` mostra o caso `unchanged` em tom `warning` com a mensagem do anfitrião. Testes em `tests/extension/core.test.cjs` e `dom.test.cjs`.
- `INTERFACES.md` atualizado (contrato do recibo).
