# Extensão 0.1.7 — adaptador 1.6.1

Corrige a captura do Nível de Acesso em negrito/aninhado e inclui os códigos de pessoas explicitamente listados. Consulte [a conferência dos painéis laterais](../CORRECAO_LATERAL_0_1_7.md). Aplicativo 0.7.0 e schema 7 permanecem compatíveis.

## Histórico anterior

# Extensão 0.1.6 — adaptador 1.6.0

Inclui os eventos da visualização detalhada quando o link Ver Mais corresponde à identidade da ficha principal. Consulte [a documentação atual](../ATUALIZACAO_0_6_0.md). Falhas preservam a captura documental e registram pendência. Não preenche datas nem remetentes por inferência.

## Histórico anterior

# Extensão 0.1.5 — adaptador 1.4.1

Captura os metadados visíveis de proveniência e situação, mantendo ausentes os campos não encontrados. A situação da via pode ser lida da tabela de marcas quando o cabeçalho está junto aos botões de ação. Consulte [o fluxo atual e seus limites](../ATUALIZACAO_0_4_0.md).

PDFs reconhecidos são baixados pela sessão do navegador, até 8 MiB por arquivo, 16 MiB no total e dez arquivos; o pacote nativo admite 24 MiB. Falhas ficam nas pendências, preservando a captura textual. Consulte [a cobertura de arquivos](../ATUALIZACAO_0_3_0.md).

## Histórico técnico anterior (limites e recursos abaixo são das versões indicadas)

# Extensão 0.1.3 — adaptador 1.3.0

A importação agora tenta baixar PDFs reconhecidos usando a sessão do navegador e envia os bytes à ponte nativa. Consulte [ATUALIZACAO_0_3_0.md](../ATUALIZACAO_0_3_0.md). Falhas e rotas não suportadas são registradas nas pendências. Parágrafos, listas e tabelas recebem estrutura textual preservada. A documentação abaixo descreve a base do adaptador antes desta atualização.

# Extensão SIGADOC — primeira implementação

Carregue esta pasta em **Gerenciar extensões → Modo de desenvolvedor → Carregar sem compactação** no Chrome ou Edge. Registre antes o host Windows `com.ponteproj.assistente` pelos scripts que acompanham o aplicativo; o popup verifica essa conexão. A chave pública fixa no manifesto produz o ID `ecllnakmppjinmednoikimbpnbgbimhg`, que deve coincidir com `allowed_origins` no manifesto do host. Não remova nem regenere essa chave.

O botão aparece somente na origem `https://www.sigadoc.mt.gov.br`, na aba principal. Abra uma ficha individual. Ele exige número oficial e título/assunto em campos explícitos; login, lista, identidade desconhecida ou conflitante são recusados. A confirmação de salvamento exige a resposta de commit do host com o mesmo UUID. Ao repetir uma falha na mesma ficha, a extensão reenvia o texto e o UUID da tentativa original. Se a ficha mudou enquanto a importação estava em andamento, a confirmação indica o número do documento capturado.

## Cobertura real desta versão

O adaptador `sigadoc-structured-heuristic/1.2.0` foi testado em fixtures sintéticas no Chrome e no Edge. **A estrutura de metadados da ficha SIGA v11.2 foi inspecionada em leitura autorizada.** Esse suporte parcial exige a rota `/sigaex/app/expediente/doc/exibir`, um número oficial único no H2 visível (ignorando seu botão) e `p#descricao` iniciado pelo rótulo exato em negrito `Descrição:`. A classificação é reconhecida em parágrafo com o rótulo `Classificação:`. O cartão documental visível desta ficha foi reconhecido e sua captura confirmada no aplicativo. Arquivos e outras estruturas do DOM autenticado ainda precisam de implementação/validação.

- Número e assunto: campos estruturados `dt/dd`, linhas de tabela com duas células, `.field/.campo` com rótulo/valor explícitos ou atributos `data-document-number` / `data-document-title` dentro de uma ficha.
- Na estrutura SIGA v11.2 descrita acima, o número oficial do H2 é canônico. A URL pode conter uma via com sufixo de uma letra maiúscula, como `-A`, somente na rota `exibir` e quando sua base coincide exatamente com esse número. A captura preserva a `sigla` da via em `source_url`; divergências, sufixos desconhecidos e identidade ambígua são recusados.
- Texto: somente contêineres documentais reconhecidos (`data-document-text`, `.document-text`, `.document-content`, `#textoDocumento` e equivalentes no código). Nunca usa o corpo geral da página como alternativa.
- Metadados: tipo, classificação, data, origem, destinatário e interessado, quando reconhecidos. Outros campos são omitidos.
- Original/PDF, anexos e antecedentes: referências classificadas pelo rótulo/contexto. **Não baixa arquivos, não lê PDFs ou iframes, não verifica assinatura nem tramitação.** Campos em inputs, textareas ou outras formas não são lidos; essa limitação pode exigir ajuste do adaptador.
- Senhas, campos de formulário, conteúdo oculto, menus e scripts são excluídos. URLs ficam na origem HTTPS autorizada; parâmetros desconhecidos/sensíveis, fragmentos e `;jsessionid` são removidos. Só parâmetros explicitamente nomeados de número documental com formato validado são preservados. Links dependentes dos parâmetros removidos podem precisar ser reabertos manualmente no SIGADOC.
- Sem IA durante importação, servidor HTTP local, conteúdo documental em armazenamento da extensão, comandos nativos arbitrários, acesso externo à extensão ou logs de documento/credenciais. A captura fica em memória até a página fechar ou recarregar.

Texto acima de 500 KiB em UTF-8 ou pacote nativo completo acima de 1 MiB é recusado com erro explícito. Não há truncamento. A extensão usa apenas `nativeMessaging` e permissão de host para o SIGADOC.

## Testes

Com Node 20+ execute `node --test tests/extension/core.test.cjs tests/extension/dom.test.cjs` na raiz do projeto. Os testes de contrato, manifesto, origem, worker, tamanho, identidade e repetição não exigem dependências. Para os testes DOM, instale as dependências de desenvolvimento desta pasta e tenha Chrome disponível; `PONTEPROJ_BROWSER_CHANNEL=msedge` seleciona Edge. Como alternativa, `PONTEPROJ_PLAYWRIGHT_DIR` aponta para um Playwright já instalado. Se essa dependência estiver ausente, os testes DOM são explicitamente marcados como ignorados. Todas as requisições dos testes DOM são interceptadas e atendidas com HTML sintético local; não há login nem acesso ao SIGADOC.

A versão 1.2.0 reconhece também um único cartão documental visível .card-sidebar.card.border-alert.bg-white > .card-body, fora de .gt-sidebar, quando a identidade SIGA 11.2 já foi validada. Células de tabela mantêm separação textual. A referência de impressão é extraída de uma chamada literal e estritamente validada de popitup; nenhum script da página é executado. No endpoint de arquivo, somente arquivo PDF e sigla correspondentes ao documento são preservados. Isso não baixa o arquivo.


