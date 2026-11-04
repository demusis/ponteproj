# Ponte • Assistente Documental SIGADOC e Controle de Gabinete (SAIOP)

Aplicativo desktop nativo para Windows (escrito em Rust com `eframe`/`egui` e SQLite embarcado) acompanhado de extensão para Google Chrome e Microsoft Edge. O **Ponte** permite capturar fichas e PDFs do SIGADOC por ação explícita do usuário, gerenciar o fluxo operacional do gabinete na área **Processos e demandas (SAIOP)**, controlar tarefas e prazos, ler PDFs com OCR local, elaborar minutas e trabalhar em equipe na **mesma intranet Windows** usando um dos computadores dos próprios usuários como anfitrião — sem depender de nuvem, máquina virtual (VM) ou banco de dados externo.

---

## Sumário

1. [Principais Funcionalidades](#principais-funcionalidades)
2. [Instalação no Windows](#instalação-no-windows)
3. [Guia de Uso no Dia a Dia](#guia-de-uso-no-dia-a-dia)
   - [1. Processos e Demandas (Controle SAIOP e Planilha `.xlsx`)](#1-processos-e-demandas-controle-saiop-e-planilha-xlsx)
   - [2. Uso Compartilhado na Intranet (Anfitrião e Clientes)](#2-uso-compartilhado-na-intranet-anfitrião-e-clientes)
   - [3. Captura Documental no SIGADOC, PDFs, Notas e Minutas](#3-captura-documental-no-sigadoc-pdfs-notas-e-minutas)
   - [4. Tarefas, Prazos e Dependências](#4-tarefas-prazos-e-dependências)
   - [5. Inteligência Artificial Opcional (Local ou HTTPS)](#5-inteligência-artificial-opcional-local-ou-https)
4. [Segurança, Dados Locais (DPAPI) e Backup](#segurança-dados-locais-dpapi-e-backup)
5. [Linha de Comando (CLI)](#linha-de-comando-cli)
6. [Compilação e Execução de Testes](#compilação-e-execução-de-testes)
7. [Solução de Problemas](#solução-de-problemas)

---

## Principais Funcionalidades

- **Processos e demandas (Controle do gabinete SAIOP):**
  - Registro e acompanhamento de entradas e reentradas (passagens pelo gabinete) independentes de captura prévia no SIGADOC.
  - Classificação separada em **quatro dimensões**: *Situação do atendimento* (`A distribuir`, `Em análise`, `Em elaboração`, `Aguardando deliberação`, `Aguardando resposta`, `Suspenso`, `Encerrado`, `Cancelado`), *Prioridade justificada* (`Baixa`, `Normal`, `Alta`, `Urgente`), *Condições/Relações* (`Retorno para UAS`, `Assinado`, `Agendado`, `Documento sensível`, `Apensado/Juntado`) e *Resultado da etapa* (`Encaminhado`, `Arquivado no controle local`, `Respondido`, `Ciência registrada`).
  - **Indicadores calculados por eventos** (sem depender de fórmulas `DATEDIF` em cache na planilha): idade do atendimento, tempo na etapa atual, espera externa e atraso em relação ao prazo.
  - **Importador `.xlsx` nativo (aba `PROCESSOS`):** leitura direta em Rust (sem precisar do Excel instalado), com prévia antes de gravar, descarte de linhas vazias que só contenham fórmulas preparadas, preservação de múltiplas passagens do mesmo número, sinalização de datas atípicas ou prazos textuais para revisão humana, bloqueio idempotente contra duplicação do mesmo arquivo (SHA-256) e **reversão segura de lote** (preservando registros já editados manualmente).
- **Trabalho em equipe na Intranet (sem nuvem, sem VM e sem SGBD externo):**
  - O próprio executável `ponteproj.exe` opera em três modos: **Somente neste computador**, **Anfitrião na Intranet** (hospeda o acervo do gabinete via HTTP/JSON na rede local) e **Cliente na Intranet** (conecta-se ao computador anfitrião).
  - Nenhum arquivo SQLite é colocado em pasta compartilhada SMB (`\\servidor\pasta`), Google Drive ou OneDrive, evitando corrupção de banco.
  - **Identificação por integrante:** mesmo que a equipe utilize uma conta compartilhada no SIGADOC, cada usuário informa seu nome/identificador no Ponte, registrando quem importou, distribuiu, alterou ou concluiu cada item.
  - **Controle de concorrência otimista e resolução de choques com mesclagem:** impede que duas pessoas sobrescrevam silenciosamente a mesma tarefa, nota, contexto ou atendimento ao mesmo tempo, e permite mesclar edições concorrentes feitas enquanto o Anfitrião estava inacessível.
  - **Trabalho contínuo offline, sincronização mútua automática e contingência (failover):** cada estação cliente mantém uma réplica local criptografada (DPAPI) e uma fila offline (`offline_outbox`). Se o computador anfitrião desligar ou quebrar por horas, os clientes continuam trabalhando normalmente (criando/editando tarefas, notas, minutas, capturas e atendimentos SAIOP). Quando o anfitrião volta à rede, todas as estações sincronizam mutuamente de forma automática; caso o mesmo item tenha sido alterado em dois lugares durante a queda, o aplicativo notifica o choque na barra superior e oferece 3 opções em 1 clique: **Mesclar as duas versões (Recomendado)**, **Manter minha versão** ou **Manter versão do Anfitrião**. Qualquer cliente também pode assumir imediatamente como novo Anfitrião.
- **Captura e Biblioteca Documental SIGADOC:**
  - Extensão para Chrome/Edge que reconhece fichas individuais do SIGADOC (`sigadoc.mt.gov.br`), captura metadados, texto e histórico, baixa PDFs originais e anexos reconhecidos e entrega tudo ao aplicativo via *Native Messaging*.
  - Leitor de PDF integrado com **OCR local do Windows** por página e pesquisa textual em todo o acervo.
  - Gestão de **Vínculos e Dossiê** entre documentos relacionados, **Contexto de trabalho**, **Notas** e editor de **Minutas** com modelos editáveis, histórico de versões, comparador lado a lado e exportação `.rtf`.
- **Tarefas Operacionais e Lixeira Reversível:**
  - Criação de tarefas vinculadas a documentos ou atendimentos, com atribuição de responsável (`assignee`), checklist, lembretes, fonte do prazo e dependências entre tarefas.
  - Exclusão reversível de documentos, capturas e tarefas através da **Lixeira** local, com validação de integridade de vínculos e fontes.

---

## Instalação no Windows

**Para quem vai usar o Ponte:** baixe o instalador em <https://github.com/demusis/ponteproj/releases/latest> (`Ponte-Setup-X.Y.Z.exe`), dê dois cliques e siga o `GUIA_DO_USUARIO.md`. Não precisa de administrador. O instalador copia o programa e a extensão para a conta do Windows atual, faz uma cópia de segurança do acervo, registra a ligação com Chrome e Edge, cria o atalho e remove versões antigas; a desinstalação fica em "Aplicativos instalados" e nunca apaga os dados. A tela inicial do Ponte confirma se o navegador está ligado e tem o botão **Copiar caminho** da pasta da extensão.

Para gerar o instalador: `scripts\build-installer.ps1` (requer Inno Setup 6: `winget install JRSoftware.InnoSetup`). O script `installer\Ponte.iss` descreve tudo o que o instalador faz. `Instalar.cmd` continua como alternativa por script.

**Extensão pela Chrome Web Store:** `scripts\build-webstore-package.ps1` gera o `.zip` de envio e `extension\WEBSTORE.md` traz a ficha, as respostas de privacidade e o passo a passo; depois de publicada, o ID da loja entra em `extension-store-id.txt` e o instalador passa a autorizá-lo.

### 1. Instalar pela linha de comando (alternativa ao instalador)

Com o pacote compilado (`dist/ponteproj.exe` atualizado), em um PowerShell normal do Windows, na raiz do projeto:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-app.ps1 -VersionSubdirectory v0.9.0
```

O instalador realiza automaticamente:
1. Uma cópia de segurança de `%LOCALAPPDATA%\PonteProj\acervo.sqlite3` em `…\PonteProj\backups\antes-de-instalar-vX.Y.Z-<data>.sqlite3` (recusa rodar com o Ponte aberto).
2. A cópia de `dist/ponteproj.exe` e dos arquivos de runtime da extensão para `%LOCALAPPDATA%\Programs\PonteProj`.
3. A criação do atalho **PonteProj** no Menu Iniciar do usuário.
4. O registro do host de comunicação nativa (`com.ponteproj.assistente`) no registro do usuário (`HKCU`) para Google Chrome e Microsoft Edge.
5. A remoção das subpastas de versões anteriores (`vX.Y.Z`).

> **Dica:** Para apenas simular e inspecionar o que será copiado sem gravar nada no disco, adicione `-WhatIf` ao comando acima.

### 2. Carregar a extensão no navegador (Chrome ou Edge)

Por segurança, o instalador não modifica políticas nem perfis do navegador automaticamente. Siga estes passos uma única vez:

1. Abra o Ponte e, na tela inicial, clique em **Copiar caminho** (ao lado de "Extensão do navegador"). A pasta é `%LOCALAPPDATA%\Programs\PonteProj\extension`.
2. No Google Chrome, acesse `chrome://extensions` (ou no Microsoft Edge, `edge://extensions`).
3. Ative **Modo do desenvolvedor**.
4. Clique em **Carregar sem compactação** (*Load unpacked*) e cole o caminho copiado na barra de endereço da janela de seleção.
5. Reabra o navegador. O ícone da extensão **Ponte** mostra "✓ Ligado ao Ponte"; se não, abre os passos para resolver. O ID esperado é `ecllnakmppjinmednoikimbpnbgbimhg`.

> Para eliminar o Modo do desenvolvedor, a extensão pode ser publicada na Chrome Web Store como *não listada* (mantendo a chave atual) ou distribuída por política do órgão (`ExtensionInstallForcelist`). Veja `PLANO_OTIMIZACAO_UX.md`, Onda 3.

### Registro rápido apenas da ponte (ambiente de desenvolvimento)

Se estiver desenvolvendo e quiser apontar o Chrome/Edge diretamente para o binário recém-compilado sem copiar arquivos para `%LOCALAPPDATA%\Programs`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-native-host.ps1 -Executable .\target\release\ponteproj.exe
```

---

## Guia de Uso no Dia a Dia

Abra o aplicativo pelo atalho **PonteProj** no Menu Iniciar. No topo da janela principal você encontrará os seletores de área (**Processos e demandas**, **Tarefas** e **Documento**) e os botões de ação (**Atualizar**, **Lixeira**, **Backup protegido**, **Rede / Intranet** e **Configurar IA**).

### 1. Processos e Demandas (Controle SAIOP e Planilha `.xlsx`)

Clique em **Processos e demandas** na barra superior para abrir a área operacional do gabinete:

- **Registrar nova entrada manualmente:** clique em **Registrar nova entrada**, preencha a referência (processo, ofício, comunicação ou código provisório), data de entrada, responsável, origem (*Recebido de*), assunto, situação, prioridade, prazo e se há retorno para UAS. Clique em **Salvar atendimento**.
- **Importar planilha Excel (`.xlsx` — aba `PROCESSOS`):**
  1. Expanda a seção **Importar planilha Excel (.xlsx — aba PROCESSOS) e lotes**.
  2. Informe o caminho completo do arquivo `.xlsx` e marque se registros marcados como *Tramitado* com retorno para UAS devem permanecer abertos em *Aguardando resposta*.
  3. Clique em **Gerar prévia da aba PROCESSOS**. O aplicativo exibirá a quantidade de atendimentos encontrados, quantas linhas contendo apenas fórmulas foram ignoradas, quantas referências possuem múltiplas passagens pelo gabinete, quantos itens foram associados automaticamente a documentos já capturados e quantos possuem alertas de revisão (como datas futuras/atípicas, prazos em texto livre do tipo `"PRAZO DIA 18"` ou datas na coluna Sim/Não).
  4. Clique em **Confirmar e importar lote**. A planilha original nunca é modificada.
  5. Caso precise desfazer a importação de um lote, clique em **Reverter lote** na lista de lotes importados: o Ponte remove os registros intocados daquele lote e preserva automaticamente qualquer atendimento que já tenha recebido edição humana ou tarefa vinculada.
- **Filas de trabalho e filtros:** filtre rapidamente por *Todos*, *Minhas pendências*, *Sem responsável*, *Prazo vencido*, *Vence hoje*, *Vence nos próximos dias*, *Aguardando resposta*, *Retorno para UAS*, *Aguardando assinatura*, *Sem atualização no período* ou *Com alerta de revisão*.
- **Ficha do atendimento:** selecione qualquer linha na grade para editar seus campos, consultar o endereço composto do SIGADOC (ou abrir o documento capturado correspondente), criar uma **Tarefa vinculada** ou registrar novos eventos datados na linha do tempo.

### 2. Uso Compartilhado na Intranet (Anfitrião e Clientes)

Para que vários computadores Windows na mesma rede local trabalhem sobre o mesmo acervo sem nuvem e sem VM:

#### No computador escolhido como Anfitrião do Gabinete:
1. Clique no botão **Rede / Intranet** no topo da janela.
2. Informe seu nome no campo **Seu nome / identificação na equipe**.
3. Selecione o modo **Anfitrião na Intranet (este PC hospeda o gabinete)**.
4. Mantenha o endereço padrão `0.0.0.0:7842` (ou escolha outra porta livre), gere ou confira o **Token do gabinete** e clique em **Salvar e iniciar Anfitrião**.
5. Clique em **Copiar dados de conexão para a equipe** e envie o endereço sugerido (`http://<IP-DO-ANFITRIAO>:7842`) e o token para os colegas.
   - *Nota:* Na primeira execução, o Firewall do Windows poderá perguntar se deseja permitir a comunicação do `ponteproj.exe` em redes privadas/corporativas; marque **Redes privadas** e permita o acesso.

#### Nos computadores dos demais integrantes (Clientes):
1. Clique em **Rede / Intranet** no topo da janela.
2. Informe seu nome individual no campo **Seu nome / identificação na equipe** (ex.: `MARIA`, `JOAO`).
3. Selecione o modo **Cliente na Intranet (conectar ao PC anfitrião)**.
4. Cole o **Endereço do anfitrião** (`http://<IP>:7842`) e o **Token do gabinete**.
5. Clique em **Testar conexão** e depois em **Salvar e sincronizar agora**.
6. A partir desse momento, todas as capturas da extensão do navegador, edições de tarefas, notas, minutas e atendimentos SAIOP feitas na estação cliente são gravadas de forma transacional no computador anfitrião e sincronizadas automaticamente a cada 4 segundos (ou ao clicar em **Atualizar**).

#### Trabalho Offline, Sincronização Mútua e Contingência (se o Anfitrião desligar ou quebrar):
- **Continuar trabalhando sem interrupção (Fila Offline):** se o computador anfitrião desligar ou ficar inacessível por horas, as estações clientes **continuam funcionando normalmente para leitura e gravação**. Todas as novas capturas, notas, minutas, tarefas e atendimentos SAIOP criados ou editados pelo cliente são salvos imediatamente no banco local protegido por DPAPI e registrados na **Fila Offline** (`offline_outbox`).
- **Sincronização mútua automática na reconexão:** assim que o computador anfitrião volta à rede (ou a cada ciclo automático de sincronização / botão **Atualizar**), o cliente envia automaticamente sua fila offline para o anfitrião e, em seguida, baixa todas as novidades que o anfitrião e outros colegas produziram no período.
- **Notificação de choques e opção de Mesclar:** se um mesmo registro (uma mesma nota, contexto, tarefa ou atendimento SAIOP) tiver sido editado no cliente offline e também alterado no anfitrião durante a desconexão:
  1. As alterações sem choque são sincronizadas normalmente.
  2. O item conflitante é preservado com segurança na lista de **Choques de Edição** (`offline_conflicts`), e o botão **⚠ Rede / Intranet (N choque)** fica destacado no topo da janela.
  3. Ao abrir **Rede / Intranet**, o usuário vê lado a lado **Sua versão local** e a **Versão atual no Anfitrião**, podendo resolver com 1 clique:
     - **Mesclar as duas versões (Recomendado):** une textos, notas, checklists, dependências, etiquetas e histórico de eventos de ambas as versões sem perder nenhuma informação e já sincroniza o resultado com o anfitrião.
     - **Manter minha versão (sobrescrever Anfitrião):** aplica a versão local sobre a revisão atual do anfitrião.
     - **Manter versão do Anfitrião (descartar minha edição):** descarta a edição local daquele item e adota a versão do anfitrião.
- **Assumir como novo Anfitrião (Failover imediato):** se o computador anfitrião principal quebrar definitivamente, qualquer estação cliente sincronizada pode abrir **Rede / Intranet**, mudar o modo para **Anfitrião na Intranet** e clicar em **Salvar e iniciar Anfitrião**, passando a atender o restante da equipe a partir do acervo já sincronizado.

### 3. Captura Documental no SIGADOC, PDFs, Notas e Minutas

1. No navegador (Chrome ou Edge), acesse sua conta no SIGADOC (`https://www.sigadoc.mt.gov.br/`) e abra a ficha de um documento.
2. Clique no botão **Importar para o assistente** injetado na página (ou pelo popup da extensão).
3. Na barra lateral esquerda (**Documentos**), além da busca por texto/número, você conta com **filtros rápidos com contadores em tempo real** (*Todos*, *Em aberto*, *● No prazo*, *⚠ Atrasados*, *✓ Concluídos* e filtro por *Prioridade*) e **sinalizadores visuais** em cada cartão (barra lateral colorida + etiqueta):
   - **Vermelho (`⚠ N tarefa atrasada`):** documento possui tarefa em aberto com prazo vencido.
   - **Âmbar (`⏰ N em aberto (vence hoje)`):** documento possui tarefa em aberto que vence na data de hoje.
   - **Azul-petróleo (`● N em aberto (no prazo)`):** documento possui tarefa(s) em aberto dentro do prazo ou sem prazo vencido.
   - **Verde-esmeralda (`✓ N tarefa concluída`):** todas as tarefas vinculadas ao documento foram concluídas.
4. Selecione o item na barra lateral esquerda para navegar pelas abas:
   - **Resumo:** visão geral da captura, tarefas do documento, contexto local de trabalho (finalidade, decisão, pendências), seleção de fontes e análise assistida.
   - **Dados SIGADOC:** todos os metadados oficiais capturados da página.
   - **Histórico:** eventos e movimentações capturados da tabela oficial do SIGADOC.
   - **Arquivos:** lista de PDFs baixados junto com a captura; clique em **Abrir no leitor** para visualizar as páginas do PDF dentro do próprio Ponte e executar **OCR local do Windows** quando o PDF for digitalizado como imagem.
   - **Vínculos e versões:** associação explícita com outros documentos do acervo (anexos, antecedentes, respostas) e histórico de capturas.
   - **Texto:** íntegra do texto extraído da página e dos PDFs.
   - **Notas:** anotações operacionais preservadas entre reimportações.
   - **Minutas:** criação de minutas de *Resposta*, *Encaminhamento* ou *Pedido de complementação*, com aplicação de modelos próprios sem IA, geração opcional com IA, histórico imutável de versões, comparação entre versões e exportação para arquivo `.rtf` (compatível com Microsoft Word e LibreOffice).

### 4. Tarefas, Prazos e Dependências

Na área **Tarefas**, você acompanha todas as ações pendentes dos documentos e atendimentos:
- Filtre por situação, prioridade, responsável, vencimento ou documento selecionado.
- Defina **Responsável** (ou clique em *Atribuir a mim*), **Prazo (AAAA-MM-DD)**, **Lembrete**, **Checklist** de passos e **Dependências** (tarefas anteriores que precisam ser concluídas antes da atual).
- O sistema impede ciclos de dependência e avisa quando se tenta concluir uma tarefa que ainda possui etapa anterior pendente.

### 5. Inteligência Artificial Opcional (Local ou HTTPS)

O Ponte funciona integralmente sem IA. Caso deseje utilizar análise assistida ou geração de rascunhos de minuta, clique em **Configurar IA**:
- **Modo Local (padrão):** conecta-se ao [Ollama](https://ollama.com/) rodando na própria máquina (`http://127.0.0.1:11434`, modelo padrão `qwen3:4b-instruct`). Nenhum dado sai do computador.
- **Modo Remoto (HTTPS):** conecta-se a um servidor HTTPS compatível com a API `/api/chat` do Ollama, permitindo informar token Bearer (ou via variável de ambiente `PONTEPROJ_AI_TOKEN`).
- As citações retornadas pela IA são verificadas literalmente contra o texto das fontes selecionadas, e nenhuma sugestão altera tarefas ou assina/tramita documentos automaticamente.

---

## Segurança, Dados Locais (DPAPI) e Backup

- **Armazenamento local:** por padrão, o banco fica em `%LOCALAPPDATA%\PonteProj\acervo.sqlite3` (pode ser alterado via variável de ambiente `PONTEPROJ_DATA_DIR`, sempre em disco local).
- **Criptografia DPAPI:** todo conteúdo documental (capturas, PDFs, textos extraídos por OCR, notas, contexto, minutas, análises, tarefas, atendimentos SAIOP, configurações de intranet e histórico de equipe) é gravado criptografado como BLOB usando a **Data Protection API (DPAPI)** do usuário do Windows.
- **Backup protegido:** clique em **Backup protegido** no topo da janela para gerar uma cópia consistente e criptografada em `%LOCALAPPDATA%\PonteProj\backups\`.

---

## Linha de Comando (CLI)

O executável aceita os seguintes argumentos:

```powershell
# Abrir a interface gráfica normal
.\target\release\ponteproj.exe

# Abrir diretamente um documento pelo ID local
.\target\release\ponteproj.exe --open-document 1

# Executar como servidor Anfitrião na Intranet em segundo plano/terminal
.\target\release\ponteproj.exe --serve 0.0.0.0:7842

# Importar um arquivo JSON de captura (formato Capture)
.\target\debug\ponteproj.exe --import-json .\tests\fixtures\capture.json

# Listar documentos como JSON no terminal (buscar termo opcional)
.\target\debug\ponteproj.exe --list "termo"

# Exibir versão e ajuda
.\target\debug\ponteproj.exe --version
.\target\debug\ponteproj.exe --help
```

*(Nota: como a compilação `release` utiliza o subsistema gráfico do Windows para não abrir janela de console ao clicar no atalho, utilize `target\debug\ponteproj.exe` caso queira inspecionar a saída de `--list`, `--version` ou `--help` diretamente no console interativo.)*

---

## Compilação e Execução de Testes

O projeto utiliza o script [`scripts/build.ps1`](file:///h:/Meu%20Drive/projetos/ponteproj/scripts/build.ps1), que suporta tanto o Rust instalado no `PATH` quanto o conjunto portátil de ferramentas (`cargo`, `rustup` e `mingw/w64devkit`):

```powershell
# Verificar compilação, rodar toda a suíte de testes e gerar o binário release (--locked)
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Action All -ToolsDirectory "$env:LOCALAPPDATA\Programs\PonteProj\devtools"

# Copiar o executável otimizado para a pasta de distribuição
Copy-Item -LiteralPath .\target\release\ponteproj.exe -Destination .\dist\ponteproj.exe -Force
```

Também é possível executar apenas etapas específicas:
- `-Action Check`: executa `cargo check --all-targets --locked`
- `-Action Test`: executa `cargo test --all-targets --locked` (testes de criptografia DPAPI, migração de schema v9, sincronização Anfitrião/Cliente na intranet, controle de concorrência, importador `.xlsx` da aba `PROCESSOS`, reversão de lote, PDF/OCR e lixeira)
- `-Action Release`: executa `cargo build --release --locked`

Para rodar os testes unitários da extensão do navegador (Node.js):

```powershell
node --test .\tests\extension\core.test.cjs
```

---

## Solução de Problemas

- **Extensão informa "Host não encontrado":** execute novamente `.\scripts\install-app.ps1 -VersionSubdirectory v0.8.0` no terminal do mesmo usuário do Windows, confirme se o ID da extensão em `chrome://extensions` ou `edge://extensions` é `ecllnakmppjinmednoikimbpnbgbimhg` e reinicie o navegador.
- **Cliente na intranet não consegue conectar ao computador Anfitrião:**
  1. Confirme se o `ponteproj.exe` está aberto no computador Anfitrião com o modo **Anfitrião na Intranet** ativo (ou rodando com `--serve`).
  2. Verifique se ambos os computadores estão na mesma rede local/intranet e se o Firewall do Windows no computador Anfitrião permite conexões de entrada para `ponteproj.exe` na porta configurada (padrão `7842`).
  3. Confira se o **Token do gabinete** colado no Cliente é idêntico ao exibido no Anfitrião.
- **Conflito de edição simultânea:** se outro integrante salvar alterações no mesmo registro enquanto você o editava, o Ponte exibirá um aviso preservando o que está na tela; descarte ou copie seu rascunho, clique em **Atualizar** para carregar a versão mais recente do colega e aplique sua complementação.
