# Versão 0.7.0 — fontes verificáveis e ciclo de trabalho

Aplicativo 0.7.0, schema 7. A extensão permanece 0.1.6, adaptador 1.6.0. Inclui os recursos da [versão 0.6.0](ATUALIZACAO_0_6_0.md): dossiês, eventos, leitor PDF integrado e OCR local.

## Fontes para análise e minutas

Em Ficha e análise, abra **Fontes adicionais e páginas para a IA**. Escolha os documentos antecedentes na biblioteca e clique em **Preparar fontes e páginas localmente**. Esse preparo não aciona IA nem rede. Depois marque explicitamente os textos e páginas que deseja incluir. A captura atual, o contexto e as notas continuam incluídos. A seleção vale para a próxima operação; resultados antigos mantêm as fontes efetivamente usadas.

As fontes distinguem texto agregado da captura (sem página determinada), texto extraído de uma página PDF e OCR salvo de uma página. Cada fonte tem documento, captura, arquivo, página quando conhecida e método. Há consulta do texto antes da seleção. A preparação indica pendências de cobertura; não obtém automaticamente antecedentes remotos nem transforma similaridade em vínculo.

A preparação admite até oito documentos, 200 páginas de texto por PDF e limites locais de quantidade/memória explicitados nas pendências. Uma operação de IA admite até 128 fontes e 500 KiB de texto selecionado; o pacote total da chamada também precisa caber em 500 KiB. Excesso é recusado antes da chamada, sem envio parcial ou corte silencioso. Reduza a seleção se necessário.

## Citações e preservação da base

Com fontes selecionadas, análise e geração de minuta exigem citações estruturadas. O programa recusa IDs não selecionados e trechos que não existam literalmente no texto indicado, tolerando apenas normalização de espaços/quebras de linha. Prazo sugerido deve ser transcrição encontrada na captura, contexto ou fontes, sem cálculo legal presumido.

A conferência literal não comprova a interpretação nem a verdade do conteúdo. Revise as conclusões, especialmente OCR, destinatário, competência e fundamentos.

Análises preservam snapshots protegidos das fontes e apresentam os trechos, arquivo/página e justificativa. Minutas geradas com fontes trazem **Bases para revisão** ao final; as versões salvas também preservam os snapshots. Reimportação de um antecedente ou alteração do OCR usado marca análise/minuta como desatualizada. Não modifica silenciosamente o texto autoral.

A IA permanece opcional. O destino/modelo escolhido é exibido pelo aplicativo. A seleção pode incluir dados privados: confira as fontes e o destino antes de acionar o provedor. Nenhum conteúdo real do acervo foi usado nos testes de IA desta entrega.

## OCR e busca

O texto OCR salvo participa da busca local, sem índice documental em claro. Reimportações reaproveitam OCR somente quando o PDF tem SHA256 idêntico e pertence ao mesmo documento. O texto permanece separado do original, com idioma e data. PDF diferente não herda OCR por semelhança de nome.

## Modelos pessoais e comparação

Em Minutas, abra **Modelos pessoais, comparação e exportação**. Prepare um modelo a partir do editor ou escreva seu conteúdo. Informe nome e tipo e salve uma nova versão protegida. Modelos não substituem versões anteriores; limite de 200 versões.

Variáveis: `{{numero_documento}}`, `{{assunto}}`, `{{destinatario}}`, `{{providencia}}`, `{{prazo}}`. Número e assunto vêm da captura atual; os demais viram campos de conferência/preenchimento. Nenhuma variável executa código. Usar modelo respeita a confirmação de substituição quando o editor contém alterações não salvas.

**Comparar versão com o editor** abre os textos lado a lado, identificando versão e captura. A comparação é um retrato do momento de abertura, sem alterar os textos.

**Exportar minuta em RTF** cria uma cópia legível na pasta local exportados, com página A4, margens, fonte Arial e parágrafos. RTF é compatível com editores que aceitam esse formato; não é DOCX nem PDF. A exportação mantém a referência à captura da minuta e não salva uma versão no acervo. Caracteres especiais, acentos, chaves e barras são escapados; a criação usa nome único e não sobrescreve arquivos existentes.

## Tarefas e dossiês

O editor permite indicar tarefas anteriores. O salvamento recusa dependência inexistente, dependência da própria tarefa, duplicidade, ciclos e conclusão enquanto uma tarefa anterior estiver pendente. Tudo é validado numa transação. Checklist e dependências são distintos; não concluem nem tramitam documentos automaticamente. Se uma tarefa anterior for removida com outro documento, a dependência ausente é mostrada para revisão e remoção explícita.

Na aba Dossiê, coincidência do processo informado apresenta candidatos para conferência. Preparar vínculo não o confirma: exige justificativa e confirmação local. Não presume juntada oficial. Uma resposta oficial pode ser importada e vinculada como resposta ao documento original; seu número/data também podem ser registrados no resultado da tarefa.

## Proteção entre versões e exclusão

Use a janela **Ponte 0.7.0**. Janelas anteriores foram preservadas para copiar eventuais textos ainda não salvos, mas não podem gravar no acervo migrado. Isso impede que um editor antigo descarte campos novos.

Excluir um documento remove seus registros e vínculos diretos. Trechos já incorporados a análises, minutas ou modelos de outros registros e cópias exportadas permanecem nesses registros; não há apagamento retroativo de textos autorais.

## Verificações

32 testes Rust passaram, incluindo citações falsas, snapshots, desatualização por antecedente, dependências/ciclos, atomicidade, modelos protegidos, escape RTF, busca/reuso de OCR e bloqueio de escrita por clientes antigos. A extensão passou em 27 testes.

Ollama local, com caso inteiramente fictício: análise com uma citação conferida e minuta de 1287 bytes com bases conferidas. Leitor PDF e OCR pt-BR também passaram no teste fictício da etapa anterior. O leitor mostrou o PDF real de duas páginas dentro do aplicativo, sem exportação.

O protocolo nativo empacotado passou com repetição idempotente, PDF e schema 7. Migração real preservou seis capturas, uma tarefa e todos os BLOBs anteriores conferidos, com integridade e chaves estrangeiras válidas.

Backup anterior: `%LOCALAPPDATA%\PonteProj\backups\pre-v070-20261102-211834-559734.sqlite3`.

SHA256 instalado: `6A84418F383F821C7D15056A37415B5EB5C2CDB19ED721672410CDC599D92F87`.

Em 03/11/2026 foi conferida a captura real #7, adaptador 1.6.0: quatro eventos foram salvos e não houve pendência de histórico. O nível de acesso exigiu a correção posterior da [extensão 0.1.7](CORRECAO_LATERAL_0_1_7.md).

## Referências técnicas

Leitura em memória: [Windows PDF](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfdocument.loadfromstreamasync). Reconhecimento local: [Windows OCR](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine.trycreatefromuserprofilelanguages). A implementação usa os bindings oficiais windows-rs, sem serviço de OCR externo.

Conferência da interface instalada: preparo local do documento real apresentou três fontes (texto agregado e duas páginas PDF), todas inicialmente desmarcadas. Nenhuma chamada de IA foi feita com esse documento.

A comparação lado a lado foi conferida na interface com a versão salva #3 (captura #4) e o editor vazio. As três minutas anteriores permaneceram no histórico e nenhum texto do editor foi substituído.
