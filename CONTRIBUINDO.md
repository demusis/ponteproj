# Ponte — Para quem vai mexer no código

Este arquivo reúne o que antes era a seção "Para desenvolvedores" do [`README.md`](README.md): como o programa se divide, onde cada coisa mora, como compilar, testar e publicar. O que a pessoa usuária precisa saber está no [`README.md`](README.md) e no [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md); o aplicativo de celular tem o [`mobile/README.md`](mobile/README.md).

## Sumário

1. [Arquitetura em uma olhada](#1-arquitetura-em-uma-olhada)
2. [Mapa dos módulos](#2-mapa-dos-módulos)
3. [Decisões que vale conhecer](#3-decisões-que-vale-conhecer)
4. [Formatos do acervo e da pasta compartilhada](#4-formatos-do-acervo-e-da-pasta-compartilhada)
5. [Compilar e testar](#5-compilar-e-testar)
6. [Registrar o host para um binário de desenvolvimento](#6-registrar-o-host-para-um-binário-de-desenvolvimento)
7. [Linha de comando e variáveis de ambiente](#7-linha-de-comando-e-variáveis-de-ambiente)
8. [Ponte de bolso (Android)](#8-ponte-de-bolso-android)
9. [Publicar uma versão](#9-publicar-uma-versão)
10. [O que está no repositório](#10-o-que-está-no-repositório)

---

## 1. Arquitetura em uma olhada

```
Chrome/Edge ──(Native Messaging)──► ponteproj.exe (host nativo) ──► acervo.sqlite3 (DPAPI)
   extension/                            src/native.rs                  src/storage.rs
                                              │
   ponteproj.exe (egui) ◄── src/app.rs ───────┘
        │
        ├── Equipe e sincronização: HTTP/JSON na intranet, fila offline, conflitos (src/intranet.rs)
        ├── Pasta compartilhada: arquivo ponte-sync.ponte cifrado (src/sharedfolder.rs)
        └── IA: Ollama (src/ai.rs) ou ChatGPT por assinatura (src/chatgpt.rs)
```

O mesmo executável faz os dois papéis. Aberto pelo usuário, mostra a janela. Aberto pelo navegador, com a origem `chrome-extension://<id>/` como argumento, atende a extensão pela entrada e saída padrão e termina.

O crate tem a *feature* `desktop` (padrão), que traz a janela, o leitor de PDF e todas as telas. Sem ela (`--no-default-features`), a biblioteca guarda só o núcleo: acervo, cifragem, sincronização, e-mail e modelos. É esse núcleo que roda no celular (`mobile/rust`). A *feature* `shots` é só de desenvolvimento: `ponteproj --shots PASTA` abre a janela sobre um acervo de exemplo e salva uma imagem de cada tela; nunca faz parte de uma versão publicada.

## 2. Mapa dos módulos

Uma linha por módulo de `src/`, a partir do comentário de abertura de cada arquivo. Os módulos marcados com ¹ só existem com a *feature* `desktop`.

### Núcleo (também compilado para o celular)

| Módulo | Papel |
|---|---|
| `lib.rs` | A biblioteca com todos os módulos |
| `main.rs` | Decide se abre a janela, atende o navegador ou executa uma opção de linha de comando |
| `storage.rs` | O acervo: esquema SQLite, migrações, proteções de gravação e a API de documentos; `Store` é a única porta de entrada |
| `protection.rs` | Cifra com a DPAPI da conta do Windows; o banco nunca recebe JSON de documento em claro |
| `paths.rs` | Onde fica o acervo: `PONTEPROJ_DATA_DIR` ou `%LOCALAPPDATA%\PonteProj` |
| `model.rs` | O que a extensão envia (`Capture`), validação, limites e tipos de resumo e detalhe |
| `native.rs` | Host do Native Messaging: quadros JSON com tamanho, IDs de extensão aceitos, respostas que sempre explicam a recusa |
| `files.rs` | PDFs importados junto com uma captura: limites, guarda cifrada e exportação |
| `attachments.rs` | Anexos de documentos, tarefas e atendimentos (guardados cifrados; passam pela Lixeira) |
| `dossier.rs` | Vínculos entre documentos e histórico imutável de versões importadas; detecção de juntada |
| `manual.rs` | Registros manuais: documentos criados no Ponte (ligação, memorando, papel, e-mail) |
| `record_import.rs` | Um registro criado junto com seus anexos, tudo ou nada |
| `whatsapp.rs` | Leitor do texto copiado de uma conversa do WhatsApp e o que o registro desse tipo guarda |
| `mail.rs` | E-mails `.eml` importados como registro manual (decodificador MIME próprio) |
| `imap.rs` | Leitura da caixa de e-mail por IMAP sobre TLS, só leitura |
| `tasks.rs` | Tarefas: modelo, validação, dependências e a cascata que conclui as tarefas dos juntados |
| `projection.rs` | Projeção em dias úteis a partir das dependências das tarefas; nunca gravada |
| `risk.rs` | Entregas em risco: tarefas abertas com prazo que o plano ou a projeção não cumpre; nunca gravado |
| `impact.rs` | Efeito de uma mudança de planejamento ainda não salva ("Efeito desta mudança"); nunca gravado |
| `holidays.rs` | Dias sem expediente: feriados nacionais, os do estado escolhido e os dias cadastrados |
| `library_settings.rs` | Configurações da biblioteca: textos pequenos que pertencem ao acervo e sincronizam com ele (hoje, os feriados) |
| `recurrence.rs` | Repetição de tarefas e de usos de recursos: calcula datas e monta cópias em memória, sem gravar |
| `categories.rs` | Categorias de tarefas e documentos (lista única, compartilhada na equipe) |
| `projects.rs` | Projetos: agrupamento opcional de tarefas, com progresso, risco e linha do tempo calculados na hora |
| `resources.rs` | Recursos (salas, livros, equipamentos), usos, e a regra de choque de horário |
| `saiop.rs` | Atendimentos (controle do gabinete) e o importador `.xlsx` da aba `PROCESSOS` |
| `trash.rs` | Lixeira: exclusão reversível, restauração e limpeza por prazo |
| `calendar_ics.rs` | Exportação iCalendar e importação conferida e parcial |
| `export.rs` | Exportação em CSV (documentos, tarefas, atendimentos, recursos, usos de recursos e projetos) |
| `intranet.rs` | Modos de rede, sincronização, fila offline, mesclagem em três vias, conflitos e a confirmação de substituição do acervo |
| `idmap.rs` | Tradução dos números de itens entre acervos (itens criados sem conexão) |
| `sharedfolder.rs` | Arquivo de transporte da pasta compartilhada (PBKDF2 + AES-256-GCM) e a guarda de cópias divergentes |
| `ai.rs` | A IA opcional: configuração, API do Ollama, conferência e reparo de cada resposta |
| `chatgpt.rs` | "Entrar com o ChatGPT": uso do plano da própria pessoa |
| `ai_roles.rs` | As duas etapas da análise (Extrator e Propositor) e o programa que as junta |
| `ai_eval.rs` | A régua: documentos fictícios com o resultado esperado (`ai_casos.json`, `ai_minutas.json`) |
| `evidence.rs` | Fontes para a IA e conferência literal de trechos |
| `prompting.rs` | Montagem do pedido à IA em camadas, detecção de conflito entre instruções |
| `draft_check.rs` | Conferência automática da minuta: código puro, sem IA |
| `draft_tools.rs` | Exportação da minuta em RTF e comparação de versões |
| `work_context.rs` | Contexto de trabalho de cada documento (finalidade, remetente, prioridade, fontes extras) |
| `work_templates.rs` | Modelos de trabalho: versões, instruções, exemplos, referências e registro de geração |
| `template_bundle.rs` | Exportação e importação de modelos e instruções em JSON versionado |
| `extraction.rs` | Texto dos arquivos anexados a exemplos de modelo |
| `suggestion.rs` | Sugestão lexical de modelo e de exemplos (TF-IDF) |
| `netdiag.rs` | Como o programa chega à internet: certificados, proxy, falhas e teste de conexão |
| `update_check.rs` | "Verificar se há versão nova": consulta ao GitHub só a pedido |
| `ficha_export.rs` | A ficha de um documento como uma página `.html` única |
| `pdf_reader.rs` | Leitura de PDF e OCR nativos do Windows, em memória |
| `guide.rs` | O guia do usuário: `GUIA_DO_USUARIO.md` embutido e convertido em página |
| `labels.rs` | Rótulos e formatos mostrados à pessoa; valores internos nunca chegam à tela sem passar por aqui |

### Janela ¹

| Módulo | Papel |
|---|---|
| `app.rs` | A janela principal: áreas, barra lateral, atalhos, barra inferior |
| `app/today.rs` | Área **Hoje** |
| `app/week.rs` | Visão **Semana** de Hoje |
| `app/calendar.rs` | Grade do mês compartilhada por Hoje e Recursos |
| `app/calendar_exchange.rs` | Exportar e conferir `.ics` na tela |
| `app/library.rs` | Lista de documentos e tela inicial |
| `app/ficha.rs` | A ficha do documento e os cartões de cada aba |
| `app/minuta.rs` | Aba de minutas e janelas de revisão do que a IA propõe |
| `app/conference.rs` | Painel **Conferência da minuta** |
| `app/proposals.rs` | Bloco **Propostas** da análise e janela **Material utilizado** |
| `app/queue.rs` | Confirmação de envio, fila de trabalhos de IA e avisos de documento que saiu do acervo |
| `app/search.rs` | Busca em todo o acervo (Ctrl+K) e **Ações** |
| `app/repetition.rs` | Repetição finita, com prévia fixa até gravar |
| `app/projects.rs` | Área **Projetos**; `app/projects/chart.rs` desenha a linha do tempo |
| `app/settings.rs` | Ajustes: seções, configuração de IA, login do ChatGPT, feriados |
| `app/windows.rs` | Janelas e diálogos secundários |
| `app/tour.rs` | Só desenvolvimento: o roteiro de telas de `--shots` |
| `tasks_ui.rs` | Painel **Tarefas**: visões, formulário, dependências, ações em lote |
| `project_ui.rs` | O projeto nas telas de tarefas: etiqueta, lista do formulário e filtro |
| `category_ui.rs` | Categorias nas telas: etiquetas, seletor, filtro e gerenciador |
| `templates_ui.rs` | Área **Modelos** |
| `resources_ui.rs` | Área **Recursos** |
| `mail_ui.rs` | Contas, pasta, filtro e lista de mensagens de e-mail |
| `doc_picker.rs` | Escolher um documento do acervo por busca |
| `reminders.rs` | Aviso diário de prazos e avisos de cada tarefa |
| `windows_ui.rs` | Seletor de arquivos e verificação do registro do host (Win32) |
| `single_instance.rs` | Uma janela por acervo: o segundo início entrega o pedido à janela aberta |
| `help_ui.rs` | O ícone ⓘ de ajuda |
| `theme.rs` | Cores, fontes, tamanhos, espaçamentos e ícones, num lugar só |
| `widgets.rs` | Peças de que toda tela é feita: botões, cartões, abas, etiquetas, aviso flutuante |
| `icon.rs` | Ícone da janela, do executável e da extensão |

Também em `src/`: `ai_casos.json`, `ai_minutas.json` e `whatsapp_vetores.json` (dados dos testes e da régua), `shots.rs` e `shots_data.rs` (só desenvolvimento) e `projects/store_tests.rs` (testes dos projetos no acervo). Fora de `src/`:

| Pasta ou arquivo | Papel |
|---|---|
| `extension/` | Extensão MV3: `capture-core.js` (leitura da página e validação), `content.js` (painel), `history.js` (tramitação), `files.js` e `background.js` (PDFs e host nativo), `popup.*` (inclui as importações recentes e **Usar no WhatsApp Web**), `whatsapp-core.js` e `whatsapp.js` (recurso opcional do WhatsApp Web) |
| `mobile/rust/` | Núcleo do celular: usa a biblioteca do Ponte sem a janela e a expõe por JNI (`libponte_mobile.so`) |
| `mobile/android/` | O aplicativo Android (Kotlin, Jetpack Compose) |
| `installer/Ponte.iss` | Instalador Inno Setup |
| `scripts/` | `build.ps1`, `build-installer.ps1`, `build-webstore-package.ps1`, `install-app.ps1`, `install-native-host.ps1` |
| `tests/` | Fluxo nativo (`native_flow.rs`), extensão (`node --test`, Playwright) e fixtures fictícias |
| `.github/workflows/verificacao.yml` | A verificação a cada envio: formatação, clippy, testes do aplicativo e da extensão |

## 3. Decisões que vale conhecer

- **Identidade de um documento** é o número oficial validado por expressão regular, nunca um ID de página.
- **Tudo o que vai para o SQLite passa por `protection::seal`.** Colunas indexadas guardam resumos criptográficos, nunca o valor: os testes de cifragem procuram números e textos em claro no arquivo do acervo.
- **Capturas são imutáveis.** Conteúdo novo cria versão; conteúdo idêntico é ignorado com aviso, sem gravar nada.
- **Dois IDs de extensão são aceitos:** o da pasta instalada (`extension-id.txt`) e o da Chrome Web Store (`extension-store-id.txt`). O instalador autoriza os dois no navegador e o executável confere os dois.
- **O host nunca fecha calado.** Origem desconhecida ou acervo que não abre são respondidos com `{ok:false,error:{code,message}}`, que a extensão mostra. Os `extras` da importação (nota e prioridade) são conferidos antes de importar e aplicados depois (`apply_extras`): a falha em gravar nunca desfaz a importação. `import_record` recebe uma conversa do WhatsApp Web com até 10 arquivos, confere tudo antes de gravar e responde `imported` ou `already`.
- **Mutações de equipe** (`IntranetMutation`) são a única forma de alterar o acervo em rede; a fila offline e a pasta compartilhada reaplicam as mesmas mutações. Cada alteração enviada ao computador principal leva um identificador desde o primeiro envio, e o principal reconhece o reenvio sem aplicar duas vezes.
- **Substituir o acervo exige confirmação.** Quando a história do computador principal rompe com a da cópia local (revisão menor, ou outro acervo), o cliente não envia a fila e não substitui nada: devolve a mensagem marcada com `[confirmar-substituicao]`. Com o "sim" da pessoa, uma cópia verificada do acervo é guardada antes, em `copias-antes-da-substituicao`, ao lado do arquivo do acervo, nomeada pelo estado dele (o mesmo estado reaproveita a cópia).
- **Pasta compartilhada: nada se perde por uma regravação.** Uma cópia com alterações que não puderam ser reaplicadas é guardada ao lado, como `ponte-sync-divergente-<hash>.ponte`, antes de ser regravada; se isso não for possível, a pasta fica como está e o resumo diz *⚠ N pasta(s) não regravada(s)*. O acervo de cada computador tem ainda a última cópia gravada (`acervo.sqlite3-guarda.ponte`).
- **Modelos de trabalho** usam as tabelas `work_templates`, `work_template_versions`, `instruction_blocks`, `instruction_block_versions`, `template_examples` e `draft_generations`. Os itens são endereçados por `uid` (UUID), não pelo número da linha, o que dispensa tradução de números na sincronização; só o registro de geração, que aponta para documento e captura, passa por `idmap`. A camada confiável do prompt só recebe texto de modelo, instruções e pedido; documentos e exemplos vão como dados.
- **A conferência dos critérios da minuta é um cache local** (`draft_conferences`): recalculável, fora da sincronização e sem mudar o formato.
- **Configurações da biblioteca** (`library_settings`) são textos pequenos com nome, sincronizados em todos os modos; hoje guardam os feriados (chave `holidays`). Se dois aparelhos mudam ao mesmo tempo, vale a mudança aplicada por último.
- **Limites:** 500 KiB de texto e 1 MiB por pacote de captura, até 10 PDFs, 8 MiB por PDF e 16 MiB de PDFs por importação, 8 MiB por anexo e 20 anexos por item, 500 mensagens por conversa do WhatsApp, 100 MiB por planilha `.xlsx` (256 MiB descompactada).
- **A extensão só atua em `https://www.sigadoc.mt.gov.br`** e remove segredos de URL. A única exceção é o recurso opcional do WhatsApp Web (`https://web.whatsapp.com`), que fica desligado até a pessoa ligá-lo e só então pede a permissão ao navegador.
- **Leituras caras ficam em memória até o acervo mudar.** Cada linha é decifrada pela DPAPI, e a interface redesenha várias vezes por segundo. `Store::cached` guarda o resultado de listas como tarefas, documentos e atendimentos e o descarta a cada gravação, desta conexão ou de outra. Uma leitura nova que seja cara e sem efeitos colaterais deve passar por ele.

**Convenções para quem for alterar o código:**

- Toda alteração do acervo (documentos, versões, tarefas, atendimentos, anexos, notas) passa por `Store`, numa transação `Immediate`, e registra-se com `intranet::record_mutation`. Preferências locais deste computador, como o prazo da Lixeira, ficam de fora e não são sincronizadas. Num computador conectado ou em pasta compartilhada, a função começa por `client_config()` e envia a alteração com `remote_mutate`, como uma `IntranetMutation`.
- Toda tabela nova guarda o conteúdo com `protection::seal` e, se pertence a um documento, entra nas proteções de `ensure_write_guards` (`src/storage.rs`).
- Textos para a pessoa ficam em português e dizem o que fazer; comentários e nomes no código ficam em inglês.
- Configurações com segredos não derivam `Debug`; escreva um que oculte o segredo, como em `IntranetConfig`.

## 4. Formatos do acervo e da pasta compartilhada

Hoje: **acervo 21** (`SCHEMA_VERSION`, em `src/storage.rs`) e **arquivo da pasta compartilhada 8** (`FORMAT`, em `src/sharedfolder.rs`). Quando o formato muda, quem está em equipe atualiza todos os computadores e o celular juntos, **o computador principal primeiro**. Uma versão mais antiga recusa o acervo e o arquivo novos, diz para atualizar e **não grava nada**; um computador já atualizado não salva o item novo num principal antigo e avisa (as mensagens `HOST_WITHOUT_…` de `src/intranet.rs`).

| Acervo / pasta | Versão do Ponte | O que passou a fazer parte |
|---|---|---|
| 16 / 4 | 0.34.0 | `facts` e `proposals` da análise (propostas da IA); Ponte de bolso 0.7.2 |
| 17 / 5 | 0.37.0 | Categorias |
| 19 / 6 | 0.39.0 | Planejamento das tarefas (início, fim, estimativa, tarefa pai, avisos) |
| 20 / 7 | 0.47.0 | Feriados do acervo (`library_settings`); Ponte de bolso 0.20.0 |
| 21 / 8 | 0.48.0 | Projetos; Ponte de bolso 0.21.0 |

Os formatos 18 (e os anteriores ao 16) não estão descritos aqui porque o histórico do repositório não os documenta. O texto para a pessoa usuária está em [`README.md`](README.md), seção 6.11.

## 5. Compilar e testar

Toolchain: Rust estável para `x86_64-pc-windows-gnu` (pode ser o conjunto portátil com MinGW) e, para a extensão, Node.js 18 ou mais novo com Playwright. A verificação automática (`.github/workflows/verificacao.yml`) usa o Rust estável no Windows e o Node 22.

```powershell
# check + testes + release, sempre com --locked
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Action All
# com um conjunto portátil de ferramentas (cargo, rustup e MinGW), informe a pasta dele:
#   ... -File .\scripts\build.ps1 -Action All -ToolsDirectory <pasta-das-ferramentas>

# só uma etapa
.\scripts\build.ps1 -Action Check     # cargo check --all-targets --locked
.\scripts\build.ps1 -Action Test      # cargo test --all-targets --locked
.\scripts\build.ps1 -Action Release   # cargo build --release --locked

# qualidade (não fazem parte do All; rode antes de publicar)
.\scripts\build.ps1 -Action Lint      # clippy em todo o código; qualquer aviso falha
.\scripts\build.ps1 -Action Format    # confere a formatação (rustfmt), sem alterar arquivos
.\scripts\build.ps1 -Action LintFix   # aplica as correções automáticas do clippy
.\scripts\build.ps1 -Action FormatFix # formata todo o código
.\scripts\build.ps1 -Action AiSmoke   # teste da IA contra um Ollama local de verdade (ignorado no Test)
```

**Só os testes da biblioteca** (o mais rápido): `cargo test --lib`. Para um módulo, acrescente o nome: `cargo test --lib projects`. O fluxo de ponta a ponta do host nativo é o `tests/native_flow.rs` (`cargo test --test native_flow`).

**Testes da extensão** (os de página usam o Chrome instalado):

```powershell
cd extension; npm install; npm test; cd ..
# ou, com um Playwright já instalado em outra pasta:
$env:PONTEPROJ_PLAYWRIGHT_DIR = 'caminho\para\node_modules\playwright'
node --test .\tests\extension\core.test.cjs .\tests\extension\dom.test.cjs .\tests\extension\files.test.cjs .\tests\extension\whatsapp.test.cjs .\tests\extension\package.test.cjs
```

**Testes do Android** (na pasta `mobile/android`, com o JDK e o SDK configurados; veja a [seção 8](#8-ponte-de-bolso-android)):

```
mobile/android/gradlew --no-daemon -q testReleaseUnitTest
```

Onde ficam os testes: os unitários num `#[cfg(test)] mod tests` no fim de cada arquivo de `src/` (os dos projetos, em `src/projects/store_tests.rs`); o fluxo do host nativo em `tests/native_flow.rs`; os da extensão em `tests/extension/`; os do celular em `mobile/android/app/src/test/` e no núcleo `mobile/rust`. Fixtures em `tests/fixtures/` e `tests/extension/*.html`, sempre com dados fictícios. Sem Playwright, os testes de página da extensão se pulam sozinhos; `PONTEPROJ_BROWSER_CHANNEL` escolhe o navegador (padrão `chrome`).

**Um aviso sobre o fim dos testes.** Foi observado que, em algumas máquinas, o processo de teste do aplicativo termina com uma falha do driver gráfico *depois* de todos os testes terem passado (a falha acontece na saída do processo). A contagem `test result: ok` impressa antes dela é o que vale; a falha em si é inofensiva e não indica problema no código.

Se o projeto estiver numa pasta sincronizada (Google Drive, OneDrive), use `$env:CARGO_BUILD_JOBS='2'` ou aponte `CARGO_TARGET_DIR` para fora dela: a compilação paralela pode derrubar a montagem do drive.
## 6. Registrar o host para um binário de desenvolvimento

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-native-host.ps1 -Executable .\target\release\ponteproj.exe
```

Para conferir que o executável atende uma extensão, chame-o como o navegador faria, com a origem como argumento e um quadro `ping` na entrada padrão. A resposta deve trazer `"ok":true` e a versão. O `ping` informa também as ações aceitas (`import_extras`, `import_record`).

## 7. Linha de comando e variáveis de ambiente

```powershell
ponteproj.exe                                  # abre a janela
ponteproj.exe --open-document 12               # abre a ficha do documento local 12
ponteproj.exe --templates                      # abre a janela na área Modelos
ponteproj.exe --import-json .\tests\fixtures\capture.json   # importa uma captura em JSON
ponteproj.exe --list "treinamento"             # lista documentos (JSON) que casam com o termo
ponteproj.exe --import-templates modelos.json  # traz instruções, modelos (rascunho; --publicar publica) e exemplos (sempre "não conferido")
ponteproj.exe --export-templates modelos.json  # grava modelos e instruções num JSON versionado (--com-exemplos inclui os exemplos)
ponteproj.exe --serve 0.0.0.0:7878             # computador principal sem janela (imprime a chave)
ponteproj.exe --guide                          # abre o guia do usuário no navegador
ponteproj.exe --guide .\guia.html              # grava o guia como página, sem abrir
ponteproj.exe --native-host                    # atende a extensão pela entrada e saída padrão, como o navegador faz
ponteproj.exe --version | --help
```

O binário `release` não abre console (subsistema gráfico); para ver a saída de `--list`, `--version` e `--help` num terminal comum, use `target\debug\ponteproj.exe` ou redirecione a saída. Existe ainda `--shots PASTA [FILTRO]`, só em compilações com `--features shots` (imagens das telas, sobre um acervo de exemplo; `PONTEPROJ_SHOTS_SIZE` escolhe o tamanho da janela).

Variáveis de ambiente: `PONTEPROJ_DATA_DIR` (outra pasta de dados, sempre em disco local), `PONTEPROJ_AI_TOKEN` (chave da IA sem gravá-la), `PONTEPROJ_NO_UI` (o host não abre a janela após importar; útil em testes), e, nos testes da extensão, `PONTEPROJ_PLAYWRIGHT_DIR` e `PONTEPROJ_BROWSER_CHANNEL`.

## 8. Ponte de bolso (Android)

O celular usa o mesmo núcleo em Rust (`mobile/rust`, que depende da biblioteca do Ponte sem a janela) e uma interface em Kotlin (`mobile/android/app/src/main/java/io/github/demusis/ponte/`). `PhoneWords.kt` adapta para o celular os textos que o núcleo escreve para o computador; `Controller.kt` guarda o estado e a trava do aplicativo; `SyncRunner.kt` faz a sincronização pedida e a automática.

```
bash mobile/build-android.sh              # núcleo (arm64) + APK
bash mobile/build-android.sh --rust-only  # só o núcleo, copiado para jniLibs
mobile/android/gradlew --no-daemon -q testReleaseUnitTest
```

O script usa as ferramentas portáteis do projeto, quando existem (variável `PONTE_DEVTOOLS`), ou o que estiver instalado: `ANDROID_HOME` (SDK), `JAVA_HOME` (JDK 17), o NDK mais novo do SDK e o alvo `aarch64-linux-android` do Rust. O APK sai em `mobile/android/app/build/outputs/apk/release/app-release.apk`. A chave de assinatura fica fora do repositório; sem ela, o build usa a chave de depuração e avisa. Mais detalhes, e o que cada versão do celular faz, em [`mobile/README.md`](mobile/README.md).

## 9. Publicar uma versão

1. Atualize `version` em `Cargo.toml` **e** em `Cargo.lock` (entrada `ponteproj`). Se a extensão mudou, atualize `"version"` em `extension/manifest.json` e `extension/package.json`. Se o celular mudou, `versionName` e `versionCode` em `mobile/android/app/build.gradle.kts`. Nunca altere `manifest.key`, `extension-id.txt` ou `extension-key.txt`.
2. `build.ps1 -Action All` (e, antes de publicar, `Lint` e `Format`) e copie o `ponteproj.exe` recém-compilado para `dist\ponteproj.exe`: o instalador empacota o que estiver em `dist\`, e o executável novo está na pasta de destino do Cargo (`target\release`, ou a pasta de `CARGO_TARGET_DIR`, se você a definiu). Confira com `dist\ponteproj.exe --version`. Escreva as notas da versão num arquivo local, fora do repositório: ele vira o texto da Release.
3. `scripts\build-installer.ps1` gera `dist\Ponte-Setup-X.Y.Z.exe` e a cópia de nome fixo `dist\Ponte-Setup.exe` (Inno Setup 6: `winget install JRSoftware.InnoSetup`).
4. `scripts\build-webstore-package.ps1` gera `dist\ponte-extensao-webstore-A.B.C.zip`; envie no painel da Chrome Web Store (ficha, privacidade e roteiro em [`extension/WEBSTORE.md`](extension/WEBSTORE.md)).
5. `gh release create vX.Y.Z dist\Ponte-Setup-X.Y.Z.exe dist\Ponte-Setup.exe dist\ponte-extensao-webstore-A.B.C.zip --notes-file <arquivo-de-notas>`.

O `Ponte-Setup.exe` de nome fixo sustenta o link direto do topo do [`README.md`](README.md) (`releases/latest/download/Ponte-Setup.exe`): sem ele na Release mais recente, o link quebra.

A extensão publicada na loja tem ID próprio (`hhppmkbmainkdfmcfeaanknlnfgihnah`), diferente do da versão carregada da pasta (`ecllnakmppjinmednoikimbpnbgbimhg`). A chave privada da extensão **não está no repositório** e deve ficar em local seguro.

## 10. O que está no repositório

Só o necessário para compilar, instalar e publicar o aplicativo e a extensão: `src/` e `Cargo.*` (aplicativo), `extension/` (extensão), `mobile/` (celular), `assets/` e `build.rs` (ícone), `installer/` e `scripts/` (instalador, host nativo, pacote da loja), `tests/` (fluxo nativo e extensão, com fixtures fictícias), `.github/` (verificação automática), `extension-id.txt`, `extension-key.txt` e `extension-store-id.txt` (identidades públicas da extensão), além do [`README.md`](README.md), deste arquivo, do [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md), da [`LICENSE`](LICENSE) e de [`extension/WEBSTORE.md`](extension/WEBSTORE.md) e [`extension/PRIVACIDADE.md`](extension/PRIVACIDADE.md).

Os instaladores e as notas de cada versão estão nas [Releases](https://github.com/demusis/ponteproj/releases). Documentos de planejamento e a chave privada da extensão ficam fora do repositório.
