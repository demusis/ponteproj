# Atualização 0.3.0 — PDFs locais e organização do texto

Instalada em 02/11/2026. Aplicativo em `%LOCALAPPDATA%\Programs\PonteProj\v0.3.0\ponteproj.exe`; extensão 0.1.3, adaptador 1.3.0. O atalho do menu Iniciar e a ponte nativa apontam para essa versão.

## Uso

Recarregue a extensão, atualize a ficha SIGADOC e importe novamente. A extensão usa a sessão atual para tentar obter os PDFs originais e anexos reconhecidos. Não solicita nem armazena senha/cookies. Não assina nem tramita.

Na aba **Texto importado**, parágrafos, listas e linhas/células de tabelas do HTML são organizados em texto. A exibição usa fonte de largura fixa e rolagem horizontal para manter o alinhamento. Essa representação não reproduz integralmente fontes, cores, imagens, células mescladas ou paginação da origem.

Em **Ficha e análise → Arquivos locais desta captura**, cada PDF salvo informa tamanho, hash SHA256 e resultado da extração. **Exportar PDF e abrir pasta** cria uma cópia legível em `%LOCALAPPDATA%\PonteProj\exportados`, com nome aleatório; abra-a no leitor de PDF para consultar a aparência original. A exportação é uma cópia em claro feita por ação do usuário; o arquivo guardado no banco continua protegido por DPAPI.

O texto extraído dos PDFs é acrescentado à captura com indicação da fonte. Dessa forma entra na busca, na análise e na preparação de minutas. Uma captura nova marca análises/minutas anteriores como desatualizadas e preserva notas, versões e tarefas.

## Cobertura e limites

- Download somente de referências classificadas como original/anexo, com rota de arquivo reconhecida ou PDF estático na origem HTTPS autorizada. Ações de inclusão, exclusão e tramitação não são executadas.
- Máximo de 8 MiB por PDF, 16 MiB e 10 PDFs por importação. Redirecionamentos são recusados; sessão expirada, pré-visualização HTML, formatos/rotas não suportados e falhas de rede aparecem nas pendências.
- PDFs escaneados sem texto ou protegidos podem ser salvos sem extração. OCR ainda não implementado.
- Texto combinado limitado a 500 KiB; se a seção de um PDF ultrapassar esse limite, o PDF é guardado e a seção não é incluída, com aviso. Não há corte silencioso do conteúdo.
- O aplicativo informa quais arquivos foram efetivamente guardados. Links restantes continuam sendo referências.
- Anexos em outras telas, antecedentes e rotas ainda não reconhecidas exigem evolução do adaptador.

## Validação

22 testes Rust e 21 testes JavaScript/DOM passaram. O executável instalado passou em teste isolado com PDF fictício: protocolo nativo, repetição idempotente, persistência criptografada, texto extraído, tarefa única e schema 3. A formatação tabular foi testada em página sintética no Chrome e conferida na interface nativa. A exportação foi acionada na interface com o PDF fictício: os bytes e o SHA256 da cópia conferiram com o arquivo original. O piloto real foi confirmado em 02/11/2026: a ficha importada mostrou PDF de 67 KiB salvo no acervo, texto extraído e 5597 caracteres combinados na captura 4.

Backup anterior: `%LOCALAPPDATA%\PonteProj\backups\pre-v030-20261102-101055-623568.sqlite3`.
SHA256 do executável: `11FF097763180F7D3A38F372334529C5467A51FC1A17AB3C8864EB4E00C5CD04`.

Referências técnicas: [Native messaging do Chrome](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging) e [pdf-extract](https://docs.rs/pdf-extract/0.12.1/pdf_extract/). O pacote de captura continua limitado a 1 MiB; o envelope de entrada com PDFs aceita até 24 MiB, e respostas ao navegador permanecem limitadas a 1 MiB.


