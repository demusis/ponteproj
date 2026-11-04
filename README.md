# Ponte — Assistente documental SIGADOC

O **Ponte** é um programa do Windows que guarda, no seu computador, os documentos que você importa do SIGADOC — com texto, PDFs e histórico de tramitação — para que você anote, prepare minutas, acompanhe prazos, controle as demandas do gabinete e, se quiser, trabalhe em equipe na rede interna. Ele é formado por um aplicativo (Rust + egui + SQLite) e por uma extensão para Chrome/Edge que coloca o botão **Importar para o assistente** nas páginas do SIGADOC.

O Ponte **nunca envia nada ao SIGADOC**: assinaturas e tramitação continuam lá. Nada vai para a nuvem; os dados ficam protegidos na sua conta do Windows.

| | |
|---|---|
| **Instalador** | <https://github.com/demusis/ponteproj/releases/latest> → `Ponte-Setup-X.Y.Z.exe` |
| **Guia do usuário** | [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md) |
| **Extensão na Chrome Web Store** | em análise (não listada) — enquanto isso, a extensão vem junto com o instalador |
| **Versão** | 0.9.2 (extensão 0.1.8) — [notas de versão](ATUALIZACAO_0_9_2.md) |
| **Licença** | MIT |

---

## Sumário

1. [Instalar em 5 minutos](#1-instalar-em-5-minutos)
2. [Um passeio pelo Ponte com um documento de exemplo](#2-um-passeio-pelo-ponte-com-um-documento-de-exemplo)
3. [Documentos: ficha, notas e minutas](#3-documentos-ficha-notas-e-minutas)
4. [Tarefas e prazos](#4-tarefas-e-prazos)
5. [Processos e demandas (controle do gabinete)](#5-processos-e-demandas-controle-do-gabinete)
6. [Trabalhar em equipe (Rede e equipe)](#6-trabalhar-em-equipe-rede-e-equipe)
7. [IA opcional](#7-ia-opcional)
8. [Onde ficam os dados, backup e lixeira](#8-onde-ficam-os-dados-backup-e-lixeira)
9. [Solução de problemas](#9-solução-de-problemas)
10. [Para desenvolvedores](#10-para-desenvolvedores)

---

## 1. Instalar em 5 minutos

Não precisa de administrador: tudo é instalado na sua conta do Windows.

1. Baixe `Ponte-Setup-X.Y.Z.exe` em <https://github.com/demusis/ponteproj/releases/latest> e dê dois cliques. Avance até o fim; pode deixar marcado **Abrir o Ponte agora**.
2. Feche e abra o navegador (Chrome ou Edge).
3. Na tela inicial do Ponte, o quadro **Ligação com o navegador** deve dizer *✓ O Ponte está registrado para o Chrome e Edge*.
4. Instale a extensão **uma vez**, de um destes jeitos:
   - **Pela loja** (assim que a publicação for aprovada): abra o link da extensão e clique em **Adicionar ao Chrome**.
   - **Pela pasta instalada**: na tela inicial do Ponte, clique em **Copiar caminho**; no navegador abra `chrome://extensions` (ou `edge://extensions`), ative **Modo do desenvolvedor**, clique em **Carregar sem compactação** e cole o caminho na barra de endereço da janela que abre.
5. Feche e abra o navegador de novo. Clique no ícone da extensão: deve mostrar *✓ Ligado ao Ponte 0.9.0*.

O que o instalador faz, caso queira saber: copia o programa para `%LOCALAPPDATA%\Programs\PonteProj\vX.Y.Z` e a extensão para `…\PonteProj\extension`; registra a ligação navegador↔Ponte (chaves `HKCU` do Chrome e do Edge); cria o atalho **Ponte** no menu Iniciar; **faz uma cópia de segurança do acervo** antes de mudar qualquer coisa; remove versões antigas. Para atualizar, basta rodar o instalador novo. A desinstalação fica em *Aplicativos instalados* e **não apaga seus dados**.

---

## 2. Um passeio pelo Ponte com um documento de exemplo

Suponha que chegou ao gabinete o ofício fictício **SESP-OFI-2026/00042 — Pedido de revisão de materiais de treinamento**, com prazo de resposta em 10 dias.

**Importar.** No SIGADOC, abra o documento (a ficha, não a lista). No canto inferior direito da página aparece o painel da extensão com o documento identificado:

```
┌──────────────────────────────────────────────┐
│          Importar para o assistente          │
│ SESP-OFI-2026/00042 — Pedido de revisão de   │
│ materiais de treinamento                     │
│ Importa o texto, os PDFs e o histórico deste │
│ documento para o Ponte.                      │
│ ▸ O que será importado                       │
└──────────────────────────────────────────────┘
```

Clique no botão. Em alguns segundos aparece *Importado. O Ponte abriu o documento.* — e a janela do Ponte abre com a ficha. Se um PDF não puder ser baixado (sessão expirada, arquivo muito grande), a própria mensagem diz qual.

**Ler.** A ficha abre na aba **Resumo**: tipo, data, situação no SIGADOC, tarefas e, mais abaixo, a análise por IA (se você quiser usá-la). A aba **Documento** traz o texto importado, os PDFs (com leitor e OCR local), os dados do SIGADOC e o histórico de tramitação.

**Anotar.** Em **Anotações e minutas**, escreva nas notas, por exemplo:

> Falar com a Coordenação de Capacitação antes de responder. Planilha anexa tem valores desatualizados (ver p. 3 do PDF).

Clique em **Salvar notas**. O ponto laranja na aba some quando tudo está salvo.

**Criar a tarefa com prazo.** No Resumo, em *Tarefas deste documento*, clique em **Nova tarefa**: título *Responder ao ofício*, responsável **Eu**, prazo `13/11/2026` (há o botão **Hoje**), salve. A partir daí o cartão do documento na lista da esquerda mostra `● 1 em aberto (no prazo) • Prazo: 13/11/2026`; quando o prazo passar, vira vermelho `⚠ 1 tarefa atrasada`.

**Responder.** Ainda em *Anotações e minutas*, na seção **Minuta**: tipo *Resposta*, clique em **Começar por um modelo** (texto-base sem IA) ou em **Gerar sugestão com IA**; edite; **Salvar nova versão**; **Exportar (RTF)** abre a pasta com o arquivo para o Word. O texto final você cola no SIGADOC — o Ponte não tramita nem assina.

**Concluir.** Com a resposta enviada, clique em **✓ Concluir** ao lado da tarefa. O cartão passa a `✓ 1 tarefa concluída`.

Tudo isso fica só no seu computador (ou no acervo do gabinete, se a equipe estiver conectada — seção 6).

---

## 3. Documentos: ficha, notas e minutas

### A lista da esquerda

- **Buscar** procura em número, assunto, texto, notas, minutas e nos PDFs lidos.
- **Filtrar ▾** abre os filtros por situação das tarefas (*Com tarefa aberta*, *No prazo*, *Atrasados*, *Concluídos*) e por prioridade. Quando há atrasados, um atalho `⚠ N atrasado(s)` aparece ao lado.
- Cada cartão tem uma barra colorida e uma etiqueta: vermelho = tarefa atrasada; âmbar = vence hoje; azul-petróleo = em aberto no prazo; verde = tudo concluído; cinza = sem tarefas.
- **Documentos juntados** (quando a ficha do SIGADOC diz *Juntado ao documento: …*) aparecem logo abaixo do documento principal, com a marca ↳ e a etiqueta `↳ Juntado a SESP-DIC-2026/97546`. O principal mostra `📎 N juntado(s)`. O vínculo entre os dois é criado automaticamente quando ambos estão importados, em qualquer ordem.

### As quatro abas da ficha

| Aba | O que tem |
|---|---|
| **Resumo** | Dados principais; tarefas do documento; atendimentos do controle do gabinete ligados a ele; análise por IA; contexto de trabalho e fontes extras (recolhidos); documentos parecidos. |
| **Documento** | Texto importado (aberto); Arquivos (PDF) com **Ler no Ponte** e **Exportar**; Dados do SIGADOC; Histórico de tramitação. |
| **Anotações e minutas** | Notas no alto; editor de minuta embaixo com tipo, modelo, IA, **Salvar nova versão**, **Copiar**, **Exportar (RTF)**, **Comparar com versão** e as versões salvas. |
| **Mais** | Vínculos entre documentos (manuais ou automáticos) e versões importadas; o que foi importado e referências; dados técnicos. |

O cabeçalho da ficha mostra o número, a etiqueta de tarefas, a situação de juntada (com **Abrir documento principal** em 1 clique) e o botão **Excluir…** (vai para a lixeira, reversível).

### Reimportar

Importar de novo o mesmo documento não duplica nada: se a página não mudou, o Ponte avisa *Este documento já estava importado*; se mudou, guarda uma nova versão (as anteriores ficam em **Mais ▸ Versões importadas**, com comparação do que mudou) e marca análises e minutas antigas como feitas sobre uma importação anterior.

---

## 4. Tarefas e prazos

Cada documento importado recebe uma tarefa de triagem. Você cria outras no Resumo (**Nova tarefa**) ou no painel **Tarefas** (topo da janela).

O painel tem a **lista à esquerda** e a **ficha à direita**. Visões: *Hoje*, *Esta semana*, *Atrasadas*, *Todas* e, em **Mais ▾**, *A triar*, *Aguardando terceiros*, *Lembretes* e *Sem prazo*.

Campos da tarefa:

| Campo | Exemplo |
|---|---|
| Título | `Responder ao ofício` |
| Responsável | `MARIA` (ou botão **Eu**) |
| Ação / Situação / Prioridade | *Responder* / *Pendente* / *Alta* |
| Prazo | `13/11/2026` — digite dd/mm/aaaa ou clique em **Hoje** |
| Observações | texto livre |
| **Mais detalhes** | origem do prazo (ex.: *art. 5º do ofício*), lembrete, resultado, checklist de etapas, dependências |

Regras úteis:
- **✓ Concluir** e **Reabrir** ficam na própria lista; concluir uma tarefa não registra nada no SIGADOC.
- **Juntados seguem o principal:** concluir a última tarefa aberta de um documento principal conclui, em cascata, as tarefas abertas dos documentos juntados a ele (e dos juntados a estes), cada uma com a anotação *"Concluída automaticamente: o documento principal X foi concluído"*. A mensagem de confirmação lista o que foi concluído; tarefas com dependência pendente ficam abertas e são informadas. Reabrir o principal não reabre as peças.
- Uma tarefa com **dependências** só pode ser concluída depois das tarefas de que depende; o Ponte recusa e explica.
- Datas inválidas (ex.: `31/02/2026`) ficam marcadas em vermelho e o salvamento avisa.

---

## 5. Processos e demandas (controle do gabinete)

Área para o controle operacional que muitos gabinetes mantêm numa planilha: cada **atendimento** é uma passagem de um processo ou documento pelo gabinete, com situação, prioridade, prazo e resultado — mesmo que o documento nunca tenha sido importado do SIGADOC.

### Registrar uma entrada à mão

Clique em **Registrar nova entrada**. A ficha abre **ao lado** da grade, com **Salvar / Descartar / Encerrar / Fechar** sempre visíveis no alto. Exemplo:

| Seção | Campo | Exemplo |
|---|---|---|
| Identificação | Referência | `SESP-PRO-2026/01234` (ou um código provisório, ex.: `DEMANDA-17`) |
| | Entrada | `03/11/2026` |
| | Responsável / Recebido de | `MARIA` / `Gabinete do Secretário` |
| Situação e prioridade | Situação | *Em análise* (depois: *Aguardando deliberação*, *Aguardando resposta*, *Encerrado*…) |
| | Prioridade / motivo | *Alta* / `prazo legal de 15 dias` |
| Prazo | Prazo / tipo | `18/11/2026` / *Prazo oficial* (ou *Meta interna*, *Cobrança de retorno*) |
| Condições e relações | Retorno para UAS?, Assinado?, Agendado, Documento sensível, Apensado/juntado | *Sim* / *Não informado* / ☐ / ☑ / `apensado ao SESP-PRO-2026/01200` |
| Resultado | Entrega prevista / Resultado da etapa / Saída / Enviado para | *Parecer* / *Encaminhado* / `20/11/2026` / `Procuradoria` |
| | Assunto / Observações | texto livre |

Só a **referência ou o assunto** é obrigatório. Cada mudança de situação ou de responsável vira um evento na **Linha do tempo**; você também registra eventos à mão (*"cobrado retorno por telefone"*).

**Ligar a documento**: se o documento já foi importado do SIGADOC, informe o número e clique em **Ligar a documento**. O Resumo do documento passa a mostrar o atendimento, e **Criar tarefa** na ficha cria uma tarefa no painel Tarefas ligada a ele.

### Importar a planilha do controle (.xlsx, aba PROCESSOS)

1. **Importar planilha…** → **Escolher planilha…** (seletor de arquivo) → **Ler a planilha**.
2. A **prévia** lista cada linha com referência, assunto, responsável, entrada, situação, prazo, alertas e se **já está registrada** no controle (mesma referência e mesma data de entrada). Nada é gravado ainda.
3. Clique em **Importar só as N novas** (ou em importar todas). A planilha original nunca é alterada.

Como as colunas da planilha viram campos:

| Coluna | Vira |
|---|---|
| B | Entrada (datas do Excel e textos como `22/10/26` são entendidos) |
| C | Responsável |
| D | Referência (define se a linha já existe) |
| I | Recebido de |
| J | Retorno para UAS? (`Sim`/`Não`; outro valor gera alerta) |
| K | Prazo (data) — texto como `PRAZO DIA 18` fica como *a confirmar*, com alerta |
| L | Observações |
| M | Verificação → situação, prioridade e resultado (ex.: `Tramitado` → *Encaminhado*; com retorno da UAS fica *Aguardando resposta*; `Urgente` → prioridade urgente) |
| N | Assinado? (vazio = *não informado*, nunca "não") |
| O, P, Q, R | Saída, Entrega prevista, Enviado para, Assunto |

Linhas só com fórmulas são ignoradas; a mesma referência com datas de entrada diferentes vira **duas passagens**. Cada linha guarda a **linha original da planilha**, visível na ficha. Uma importação pode ser desfeita em **Importações anteriores ▸ Desfazer importação…** (pede confirmação e preserva o que você já editou ou ligou a tarefa).

### Filas e filtros

**Fila ▾** mostra quantos atendimentos há em cada uma: *Minhas pendências*, *Sem responsável*, *Prazo vencido*, *Vence hoje*, *Vence nos próximos dias* (ajuste os dias), *Aguardando resposta*, *Retorno para UAS*, *Aguardando assinatura*, *Sem atualização no período*, *Com alerta de revisão*. **Filtros ▾** abre responsável, origem, destino e situação. Clique no título de uma coluna para ordenar. A coluna *Idade / Espera* mostra dias desde a entrada e dias aguardando retorno externo.

---

## 6. Trabalhar em equipe (Rede e equipe)

Um computador do gabinete é o **principal**; os outros se conectam a ele pela rede interna. Nada vai para a internet e nenhum arquivo de banco fica em pasta compartilhada (o que corromperia o SQLite). Cada computador guarda sua cópia completa e protegida.

**No computador principal** (Menu ▾ → Rede e equipe):
1. *Seu nome na equipe*: `CARLOS`.
2. Marque **Este é o computador principal do gabinete** e **Salvar e ficar disponível agora**.
3. **Copiar código de conexão** e envie o texto aos colegas. É algo como:
   ```
   Código de conexão do Ponte (gabinete):
   ponte://conectar?host=http%3A%2F%2F192.168.1.10%3A7878&chave=ponte-3f9c…
   ```
   Marque *Ficar disponível para a equipe sempre que o Ponte abrir* para não precisar repetir o passo 2.

**Nos outros computadores**:
1. Nome na equipe: `MARIA`.
2. Marque **Conectar ao computador principal do gabinete**, cole o código em *Código de conexão* e clique em **Usar código** — o Ponte testa a conexão na hora.
3. **Salvar e sincronizar acervo agora**.

Pronto: importações, notas, minutas, tarefas e atendimentos de todos passam a ir para o mesmo acervo, com o nome de quem fez cada coisa. O indicador no canto superior direito mostra o estado: `● Conectado • atualizado agora`, `● Sem conexão • 2 a enviar`, `● Computador principal ativo` ou `⚠ N conflito(s) de edição`. Clique nele para abrir a janela.

**Se o principal desligar**, nada para: cada computador continua salvando localmente (a mensagem avisa *Salvo neste computador… será enviado ao reconectar*) e tudo é enviado quando a conexão volta. Se o principal quebrar de vez, qualquer colega pode marcar o próprio computador como principal e distribuir um novo código: o acervo dele já está completo.

**Conflito de edição** acontece só quando o *mesmo item* foi alterado nos dois lados enquanto estavam desconectados. A janela mostra, campo a campo, o valor no principal, o seu e **quem alterou** (*só por você*, *só no computador principal*, *pelos dois lados*), e oferece:
- **Combinar as duas versões…** — cada campo fica com o lado que o alterou; textos alterados dos dois lados são juntados; nos demais campos alterados dos dois lados, a sua versão prevalece e o valor do principal fica anotado no próprio item (nada se perde em silêncio);
- **Usar só a minha versão…**; **Usar só a do computador principal…**.

Todas pedem confirmação. Se os colegas não conseguirem conectar, peça à TI para liberar a porta **7878 (TCP)** no Firewall do Windows do principal (a janela traz o comando).

---

## 7. IA opcional

O Ponte funciona inteiro sem IA. Quando você quiser:

- **Resumo ▸ Analisar e sugerir providências**: resume o documento, sugere prioridade, prazo mencionado e providências (cada uma vira tarefa com um clique). Trechos citados são conferidos literalmente contra as fontes.
- **Anotações e minutas ▸ Gerar sugestão com IA**: escreve uma minuta do tipo escolhido, com uma orientação opcional (*"solicitar a informação que falta, com tom objetivo"*). A sugestão abre para revisão e só vira versão quando você salvar.
- Contexto de trabalho e notas não salvos são salvos antes; a linha ao lado do botão diz para onde os dados vão.

**Menu ▾ → Configurar IA**: *Ollama neste computador* (padrão `http://127.0.0.1:11434`, modelo `qwen3:4b-instruct` — nada sai da máquina) ou *Servidor remoto HTTPS* compatível com a API `/api/chat` do Ollama, com token opcional (também pela variável `PONTEPROJ_AI_TOKEN`). Endpoint, modelo e token não são gravados em disco.

---

## 8. Onde ficam os dados, backup e lixeira

- **Acervo**: `%LOCALAPPDATA%\PonteProj\acervo.sqlite3` (outra pasta via variável `PONTEPROJ_DATA_DIR`, sempre em disco local). Todo conteúdo — texto, PDFs, OCR, notas, minutas, tarefas, atendimentos, configuração de rede — é gravado cifrado com a **DPAPI** da sua conta do Windows: outra conta no mesmo PC, ou outro PC, não consegue ler.
- **Backup**: Menu ▾ → **Fazer backup agora** cria uma cópia consistente em `…\PonteProj\backups\`. O instalador também faz uma antes de cada atualização. Um backup só abre na mesma conta do Windows que o criou.
- **Lixeira**: excluir documento, tarefa ou versão importada pede confirmação e manda para a Lixeira (Menu ▾), de onde se restaura. Nada é apagado definitivamente.
- **Ao fechar** com algo não salvo, a janela lista cada pendência com um botão **Ir para**.

---

## 9. Solução de problemas

| Sintoma | O que fazer |
|---|---|
| O botão **Importar para o assistente** não aparece no SIGADOC | A extensão não está carregada ou a página não é a ficha de um documento. Clique no ícone da extensão: se disser *✗ Não ligado*, siga os passos que ele mostra. |
| *"O Ponte não foi encontrado neste computador"* | Abra o Ponte uma vez e reinicie o navegador. Se continuar, rode o instalador de novo **com a mesma conta do Windows**. |
| *"Não recebemos a confirmação do Ponte"* | Clique em **Repetir importação**. A repetição usa o mesmo identificador: nada é duplicado. |
| *"A página ainda não terminou de carregar"* | Espere a página abrir por completo e clique de novo. |
| Tela inicial diz *✗ O navegador ainda não foi ligado ao Ponte* | O registro não existe para esta conta do Windows. Rode o instalador. |
| Dois botões "Importar" na página | Há duas extensões (a da pasta e a da loja). Remova a da pasta em `chrome://extensions`. |
| Colega não consegue conectar ao principal | O principal precisa estar com o Ponte aberto e *disponível*; os dois na mesma rede; porta 7878 liberada no firewall do principal; código de conexão atual (gerar nova chave invalida os códigos antigos). |
| *"Conflito de edição simultânea"* ao salvar | Outra pessoa salvou o mesmo item antes de você. Copie o que escreveu, clique em **Descartar**, abra o item de novo e aplique sua alteração. |
| *"Não foi possível proteger/abrir os dados deste perfil Windows"* | O acervo pertence a outra conta do Windows. Entre com a conta que fez a instalação. |

---

## 10. Para desenvolvedores

### Arquitetura em uma olhada

```
Chrome/Edge ──(Native Messaging)──► ponteproj.exe --native-host ──► acervo.sqlite3 (DPAPI)
   extension/                              src/native.rs                 src/storage.rs
                                                │
   ponteproj.exe (egui) ◄── src/app.rs ─────────┘
        │
        └── Rede e equipe: HTTP/JSON na intranet (src/intranet.rs), fila offline e conflitos
```

| Pasta / arquivo | Papel |
|---|---|
| `src/app.rs` | Janela principal: lista, ficha em 4 abas, menu, indicadores |
| `src/labels.rs` | Rótulos e datas exibidos; campo de data `dd/mm/aaaa` |
| `src/storage.rs`, `src/protection.rs` | SQLite + DPAPI; `Store` é a única porta de entrada aos dados |
| `src/model.rs` | `Capture` (o que a extensão envia) e sua validação |
| `src/dossier.rs` | Vínculos entre documentos, versões importadas, detecção de juntada |
| `src/tasks.rs`, `src/tasks_ui.rs` | Tarefas, dependências, painel |
| `src/saiop.rs` | Processos e demandas: modelo, importador `.xlsx` (leitor ZIP/XML próprio), painel |
| `src/intranet.rs` | Modos principal/conectado, sincronização em thread própria, fila offline, mesclagem em três vias |
| `src/pdf_reader.rs`, `src/evidence.rs`, `src/ai.rs` | Leitor de PDF + OCR do Windows, fontes para a IA, cliente Ollama |
| `src/windows_ui.rs` | Seletor de arquivo e verificação do registro do host (crate `windows`) |
| `extension/` | Extensão MV3: `capture-core.js` (leitura da página e validação), `content.js` (painel), `background.js` (host nativo e PDFs), `popup.*` |
| `installer/Ponte.iss` | Instalador Inno Setup |
| `scripts/` | `build.ps1`, `build-installer.ps1`, `build-webstore-package.ps1`, `install-app.ps1`, `install-native-host.ps1` |
| `tests/` | Fluxo nativo (`native_flow.rs`), extensão (`node --test`, Playwright) e fixtures fictícias |

Decisões que valem conhecer: a identidade de um documento é o número oficial validado por expressão regular (nunca um ID de DOM); toda gravação em SQLite passa por `protection::seal`; as capturas são imutáveis (reimportar cria versão); limites de 500 KiB de texto, 1 MiB por pacote, 8 MiB por PDF; a extensão só aceita `https://www.sigadoc.mt.gov.br` e remove segredos de URL.

### Compilar e testar

Toolchain: Rust estável para `x86_64-pc-windows-gnu` (pode ser o conjunto portátil com MinGW) e, para a extensão, Node.js 18+ com Playwright 1.62.

```powershell
# check + testes + release, sempre com --locked
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Action All -ToolsDirectory "$env:LOCALAPPDATA\Programs\PonteProj\devtools"
Copy-Item .\target\release\ponteproj.exe .\dist\ponteproj.exe -Force

# só uma etapa
.\scripts\build.ps1 -Action Check   # cargo check --all-targets --locked
.\scripts\build.ps1 -Action Test    # cargo test --all-targets --locked
.\scripts\build.ps1 -Action Release # cargo build --release --locked

# testes da extensão (30 testes; os de DOM usam o Chrome instalado)
$env:PONTEPROJ_PLAYWRIGHT_DIR = 'caminho\para\node_modules\playwright'
node --test .\tests\extension\core.test.cjs .\tests\extension\dom.test.cjs .\tests\extension\files.test.cjs
```

Se o projeto estiver numa pasta sincronizada (Google Drive, OneDrive), use `$env:CARGO_BUILD_JOBS='2'` ou aponte `CARGO_TARGET_DIR` para fora dela: a compilação paralela pode derrubar a montagem do drive.

### Registrar o host para um binário de desenvolvimento

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-native-host.ps1 -Executable .\target\release\ponteproj.exe
```

### Linha de comando

```powershell
ponteproj.exe                                  # abre a janela
ponteproj.exe --open-document 12               # abre a ficha do documento local 12
ponteproj.exe --import-json .\tests\fixtures\capture.json   # importa uma captura em JSON
ponteproj.exe --list "treinamento"             # lista documentos (JSON) que casam com o termo
ponteproj.exe --serve 0.0.0.0:7878             # computador principal sem janela (imprime a chave)
ponteproj.exe --version | --help
```

O binário `release` não abre console (subsistema gráfico); para ver a saída de `--list`, `--version` e `--help` use `target\debug\ponteproj.exe`.

### Publicar uma versão

1. Atualize `version` em `Cargo.toml` **e** em `Cargo.lock` (entrada `ponteproj`), e `"version"` em `extension/manifest.json` + `extension/package.json` se a extensão mudou. Nunca altere `manifest.key`, `extension-id.txt` ou `extension-key.txt`.
2. `build.ps1 -Action All`, copie o exe para `dist\`, escreva `ATUALIZACAO_X_Y_Z.md`.
3. `scripts\build-installer.ps1` → `dist\Ponte-Setup-X.Y.Z.exe` (Inno Setup 6: `winget install JRSoftware.InnoSetup`).
4. `scripts\build-webstore-package.ps1` → `dist\ponte-extensao-webstore-A.B.C.zip`; envie no painel da Chrome Web Store (ficha e privacidade em [`extension/WEBSTORE.md`](extension/WEBSTORE.md)).
5. `gh release create vX.Y.Z dist\Ponte-Setup-X.Y.Z.exe dist\ponte-extensao-webstore-A.B.C.zip --notes-file ATUALIZACAO_X_Y_Z.md`.

A extensão publicada na loja tem ID próprio (`hhppmkbmainkdfmcfeaanknlnfgihnah`), registrado em `extension-store-id.txt`; o instalador e o `install-native-host.ps1` autorizam esse ID e o da versão carregada da pasta (`ecllnakmppjinmednoikimbpnbgbimhg`). A chave privada `extension.pem` **não está no repositório** e deve ficar em local seguro.

### Documentos do projeto

- [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md) — manual sem termos técnicos.
- [`PLANO_OTIMIZACAO_UX.md`](PLANO_OTIMIZACAO_UX.md) — diagnóstico de usabilidade e plano em ondas (estado atual na seção 5).
- [`ATUALIZACAO_0_9_0.md`](ATUALIZACAO_0_9_0.md) e anteriores — notas de cada versão.
- [`PROPOSTA_CONTROLE_SAIOP.md`](PROPOSTA_CONTROLE_SAIOP.md), [`PLANO_JUNTADA_DOCUMENTOS.md`](PLANO_JUNTADA_DOCUMENTOS.md), [`notas_tecnicas.md`](notas_tecnicas.md), [`INTERFACES.md`](INTERFACES.md) — desenho das funcionalidades.
- [`extension/WEBSTORE.md`](extension/WEBSTORE.md) e [`extension/PRIVACIDADE.md`](extension/PRIVACIDADE.md) — publicação na loja.
