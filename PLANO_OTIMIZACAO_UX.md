# Plano de Otimização de Uso — Ponte v0.8.0

**Data:** 2026-11-03
**Base:** leitura completa das trilhas de uso em `src/app.rs`, `src/tasks_ui.rs`, `src/work_context.rs`, `src/evidence.rs`, `src/draft_tools.rs`, `src/dossier.rs`, `src/saiop.rs`, `src/intranet.rs`, `extension/*`, `scripts/*.ps1` e dos guias `README.md` / `INSTALACAO_LOCAL.md`.
**Limite deste plano:** é uma análise do produto como está no código. Nenhum usuário foi observado. A Onda 4 existe para corrigir o que esta leitura não consegue prever.

---

## 1. Diagnóstico: por que a operação parece complexa

O aplicativo é funcional e seguro, mas expõe ao usuário a mesma quantidade de informação que expõe ao desenvolvedor. Sete causas-raiz explicam quase todo o atrito encontrado:

| # | Causa-raiz | Evidência medida |
|---|---|---|
| A | **Tudo visível ao mesmo tempo** | Ficha aberta na aba Resumo: 35 a 45 controles interativos simultâneos (`app.rs`). SAIOP: 5 filtros + 11 filas + grade de 8 colunas + ficha de ~20 campos numa só tela, com duas áreas de rolagem empilhadas (`saiop.rs:2301-2889`). 8 abas por documento. 9 janelas flutuantes + 3 modos do painel central. |
| B | **Vocabulário técnico na interface** | "captura", "adaptador", "antecedente", "SHA-256", "KiB", "host nativo", "Revisão #N", "choque", "Anfitrião/Cliente", "Col D/J/N", "endereço composto", "loopback", "DPAPI", "SQLite". Valores internos sem tradução: `a_confirmar`, `complementacao`, `tomar ciencia`, `aguardando_resposta`, datas em ISO/RFC3339, JSON bruto em "Contexto salvo utilizado" (`work_context.rs:80`), `{:?}` de BTreeMap no registro bruto (`saiop.rs:2824-2830`). |
| C | **Parágrafos explicativos permanentes** | Mais de 40 textos fixos de 1 a 3 linhas (lista completa na auditoria). O mais longo (`evidence.rs:292`) tem 4 frases. A divulgação da IA (`app.rs:1978`) aparece duas vezes. |
| D | **Feedback disperso e passivo** | Uma barra global no rodapé que nunca some sozinha + 6 canais inline (Tarefas, Contexto, Fontes, Vínculos, Modelos, Lixeira). O spinner fica só no rodapé, longe do botão clicado. "Tarefa salva" aparece no fim de uma área rolável (`tasks_ui.rs:460`). Em modo Cliente, gravações offline entram na fila em silêncio (`intranet.rs:1593-1599`). |
| E | **Digitação onde deveria haver um controle** | Datas em texto livre `AAAA-MM-DD` sem validação (tarefa, contexto, SAIOP: entrada, prazo, saída). Caminho do `.xlsx` digitado à mão, sem seletor (`saiop.rs:2213`). Endereço e token do Anfitrião colados em dois campos separados. Endereço de escuta `0.0.0.0:7878`. |
| F | **Instalação de 11 passos** | PowerShell com `-ExecutionPolicy Bypass`, `chrome://extensions`, Modo do desenvolvedor, "Carregar sem compactação", pasta oculta em `%LOCALAPPDATA%`, conferência de um ID de 32 letras, reinício do navegador. Não há pacote distribuível: o instalador exige a árvore do projeto com `dist\ponteproj.exe` já compilado (`install-app.ps1:27-58`). Atualização repete tudo e exige ler a nota de versão para saber se a extensão deve ser recarregada. |
| G | **Comportamentos surpreendentes** | O app abre no painel Tarefas, não na tela de boas-vindas (`app.rs:478`). Analisar exige notas salvas; gerar minuta não (`app.rs:691` vs `:735`). "Mesclar as duas versões" na verdade mantém todos os campos locais exceto observações (`intranet.rs:1936-1976`). "Criar tarefa vinculada" descarta edições não salvas da ficha SAIOP e pode exibir "Tarefa #0 criada" sem criar nada (`saiop.rs:2862-2868`, `:1326-1337`). A prévia de importação diz "Confira abaixo" e não lista linhas (`saiop.rs:2232-2267`). "Reverter lote" é imediato, sem confirmação (`saiop.rs:2270-2294`). Planilha atualizada reimporta tudo como novo. Não existe botão "Concluir tarefa". A sincronização roda na thread da interface e pode travar a janela por até 4 s a cada ciclo quando o Anfitrião está fora (`intranet.rs:2264`, `app.rs:2211`). |

**Síntese:** o usuário precisa aprender o modelo mental do desenvolvedor (captura → adaptador → cobertura → vínculo → revisão) para executar tarefas que, para ele, são apenas "importar o documento, anotar, responder e acompanhar o prazo".

---

## 2. Princípios para a otimização

1. **Uma tela, uma tarefa.** O que não serve à tarefa atual fica atrás de um clique, não na frente.
2. **A palavra do usuário, não a do código.** Se o termo não existe no SIGADOC nem no dia a dia do gabinete, ele não aparece sem tradução.
3. **Explicar no momento, não o tempo todo.** Texto de ajuda vira tooltip `ⓘ`, primeira-vez ou "Saiba mais"; nunca parágrafo fixo.
4. **Mostrar o resultado onde a ação aconteceu.** Progresso no botão, confirmação ao lado do campo, erro persistente até resolvido.
5. **Nenhuma regra de segurança é relaxada.** Validação de identidade, DPAPI, limites de tamanho, chave da extensão e imutabilidade das capturas não mudam. Este plano só muda a camada de apresentação e o empacotamento.

---

## 3. Plano em ondas

Esforço estimado em dias de trabalho de uma pessoa, já contando testes. As ondas são independentes e podem ser reordenadas; a ordem abaixo prioriza impacto por esforço.

### Onda 0 — Correções de comportamento (2 a 3 dias)

Correções pontuais que não mudam a estrutura e eliminam armadilhas reais.

| Item | Mudança | Arquivo |
|---|---|---|
| 0.1 | Abrir o app na tela de boas-vindas (ou no último documento) em vez do painel Tarefas. | `app.rs:478` (`tasks_open: false`) |
| 0.2 | Traduzir todos os valores internos exibidos: prioridade, situação, tipo de minuta, finalidade, tipo de prazo (`selected_text` em `saiop.rs:2682`), status das dependências. Criar um único módulo `labels.rs` com `fn label(kind, key) -> &str` e usá-lo em todos os pontos. | `app.rs`, `tasks_ui.rs`, `work_context.rs`, `draft_tools.rs`, `saiop.rs`, `intranet.rs` |
| 0.3 | Datas sempre em `dd/mm/aaaa` na exibição (listas de tarefas, pills, versões, modelos, linha do tempo SAIOP, histórico da equipe). Guardar ISO; exibir local. | idem |
| 0.4 | "Contexto salvo utilizado nesta análise": renderizar campo a campo em vez do JSON. Registro bruto da planilha: tabela "coluna → valor" em vez de `{:?}`. | `work_context.rs:80`, `saiop.rs:2824` |
| 0.5 | "Criar tarefa vinculada": salvar a ficha antes de criar; se não há documento vinculado, dizer "Providência registrada na linha do tempo (sem tarefa, pois o atendimento não está ligado a um documento)". | `saiop.rs:2862-2868`, `:1326` |
| 0.6 | "Reverter lote" com confirmação modal listando quantos serão removidos e quantos preservados. | `saiop.rs:2270-2294` |
| 0.7 | Prévia de importação: listar as linhas (referência, assunto, responsável, prazo, alerta) numa grade rolável; marcar as que já existem no controle (mesma referência + mesma entrada) e oferecer "Importar só as novas". | `saiop.rs:2232-2267`, `:1148-1186` |
| 0.8 | Botão **Concluir** direto na lista de tarefas e no grupo "Tarefas deste documento" (1 clique, com "Desfazer" no aviso). | `tasks_ui.rs`, `app.rs:1722` |
| 0.9 | Análise e minuta com a mesma regra: usar o que está no editor e avisar "usando notas não salvas", em vez de bloquear. | `app.rs:691-697`, `:735-757` |
| 0.10 | "Limpar filtros" da lista vazia também limpa a busca. | `app.rs:1257` |
| 0.11 | "Mesclar as duas versões": implementar mesclagem campo a campo (campo alterado só de um lado vence; alterado dos dois lados → mostrar lado a lado e pedir escolha). Até lá, renomear para "Manter minha versão e anexar as observações do Anfitrião", que é o que faz. | `intranet.rs:1936-1976` |
| 0.12 | Sincronização fora da thread da interface (thread + canal, como o `Worker` de IA) e indicador "Sincronizando…" no rodapé. | `intranet.rs:2264`, `app.rs:529`, `:2211` |
| 0.13 | Gravação offline em modo Cliente: a mensagem de sucesso diz "Salvo neste computador; será enviado ao computador principal quando reconectar". | `intranet.rs:1593-1599` |

### Onda 1 — Reduzir o que está na frente (5 a 7 dias)

**1.1 Ficha do documento: de 8 abas para 4**

| Nova aba | Conteúdo | Vem de |
|---|---|---|
| **Resumo** | Cabeçalho, situação, tarefas, resultado da IA **no topo**, botão "Analisar". Contexto e Fontes ficam abaixo, recolhidos. | Resumo atual, reordenado |
| **Documento** | Seções em sequência: Texto, Arquivos (PDFs), Dados do SIGADOC, Histórico. Cada seção é um cabeçalho recolhível aberto por padrão só no Texto. | Dados SIGADOC + Histórico + Arquivos + Texto |
| **Anotações e minutas** | Notas em cima, minuta embaixo (ou lado a lado em telas largas). Botões "Exportar (RTF)" e "Comparar versões" visíveis, não escondidos em "Modelos". | Notas + Minutas |
| **Mais** (ou ícone ⚙ no cabeçalho) | Vínculos locais, versões de captura, cobertura e pendências, dados técnicos, modelos pessoais, excluir. | Vínculos e versões + partes técnicas de Arquivos/Dados |

Remover do Resumo os botões que duplicam abas ("Arquivos e cobertura", "Histórico"). Os indicadores "não salvas" migram para um ponto colorido na aba.

**1.2 Barra lateral**
- Chips de filtro e combo de prioridade escondidos atrás de um botão "Filtrar" enquanto a biblioteca tiver menos de 8 documentos; contagens zero nunca aparecem.
- Cartão do documento: número, título e **uma** pill (a mais urgente). Prioridade só quando definida.
- Biblioteca vazia: só o texto de boas-vindas com os 3 passos e um botão "Verificar se a extensão está conectada" (chama o mesmo ping do popup).

**1.3 Barra superior**
- Esquerda: `Documentos | Tarefas | Processos e demandas` (manter).
- Direita: um único botão **"Menu ▾"** com Backup, Lixeira, Rede, Configurar IA, Sobre. Remover "Atualizar" (a atualização automática já existe) ou movê-lo para o menu.
- O status da rede vira um ícone com cor (verde conectado, cinza local, âmbar offline, vermelho conflito) com tooltip; o texto longo da faixa (`intranet.rs:2659-2706`) some do cabeçalho.

**1.4 SAIOP (Processos e demandas)**
- Filas: combo "Fila: Todas ▾" com contagem em cada opção, em vez de 11 chips.
- Filtros de texto: um campo "Buscar" + botão "Filtros avançados ▸" que abre responsável/origem/destino/situação.
- Grade com ordenação por clique no cabeçalho e altura que ocupa o espaço disponível quando nenhuma ficha está aberta.
- Ficha do atendimento em **painel lateral direito** (ou janela) com botões fixos no rodapé (Salvar / Descartar / Encerrar). Indicador "não salvo" no título.
- Importação de planilha: janela própria aberta por "Importar planilha…", com seletor de arquivo nativo (adicionar `rfd` ao `Cargo.toml`; é a dependência padrão do ecossistema egui e não altera o modelo de dados).
- Remover "(Col D)", "(Col J)" etc. dos rótulos; manter o mapeamento só na janela de importação.
- Agrupar visualmente as 4 dimensões (Situação / Prioridade / Condições / Resultado) com cabeçalhos; expor "Apensado/Juntado" e tags, que hoje são importados e não exibidos.
- Ação manual "Vincular a documento da biblioteca" na ficha (busca por número) e, no sentido inverso, grupo "Atendimentos no controle do gabinete" no Resumo do documento.

**1.5 Painel Tarefas**
- Lista à esquerda, formulário à direita (sem rolar para achar o que abriu).
- Visões: reduzir para `Hoje | Esta semana | Atrasadas | Todas` + caixa "Mostrar concluídas"; "A triar", "Aguardando terceiros", "Lembretes", "Sem prazo" viram filtros secundários.
- Campos "Origem do prazo", "Lembrar a partir de", "Resultado local", "Dependências", "Checklist" ficam em "Mais detalhes ▸".

**1.6 Textos permanentes**
- Todos os parágrafos listados na auditoria viram: (a) tooltip `ⓘ` ao lado do título da seção, ou (b) um único "Como funciona" recolhível por tela, ou (c) texto de primeira execução que some após "Entendi".
- A divulgação da IA fica em um só lugar: tooltip do botão "Analisar"/"Gerar", e no texto da janela Configurar IA.

**1.7 Feedback**
- Avisos de sucesso: toast no rodapé que some em 6 s. Erros: persistem até fechar ou até a próxima ação bem-sucedida no mesmo contexto.
- Spinner no próprio botão (`Button` desabilitado com "Analisando… 12 s").
- Unificar os 6 canais inline na barra global, com o texto dizendo onde olhar ("Tarefa salva — veja em Tarefas").
- Modal "Fechar o assistente?" lista as pendências ("Notas do SESP-DES-2026/39019, Ficha SAIOP #12") com botão "Ir para" em cada uma.

### Onda 2 — Vocabulário, datas e rede (4 a 5 dias)

**2.1 Glossário de substituições** (aplicar com busca por string)

| Hoje | Passa a ser |
|---|---|
| captura / Captura #N | documento importado / importado em dd/mm/aaaa (o número vai para "Mais") |
| adaptador, cobertura, reconhecido/pendente | some da interface; vira "O que foi importado" em "Mais" |
| antecedente (tipo de vínculo) | documento anterior |
| relacionado / resposta | relacionado / resposta a |
| Anfitrião / Cliente | computador principal / conectado ao computador principal |
| Integrante | você está identificado como |
| Revisão #N | atualizado há X min (usar `last_sync_at`, hoje gravado e nunca exibido) |
| choque | conflito de edição |
| host nativo, registro, HKCU | o aplicativo Ponte |
| extensão descompactada | extensão do navegador |
| SHA-256, KiB, bytes, UUID | identificação do arquivo (em "Mais"), KB, caracteres, some |
| Stand By | suspenso |
| Retorno para UAS | manter (termo do gabinete), com tooltip |
| endereço composto | abrir no SIGADOC |
| ext 5d (idade/espera) | 5 d fora do gabinete |
| Lote #N • SHA-256 | Importação de dd/mm/aaaa, arquivo X |

**2.2 Datas:** `egui_extras::DatePickerButton` em todos os campos de data (tarefa, contexto, SAIOP). Entrada por digitação continua aceita em `dd/mm/aaaa`.

**2.3 Mensagens de erro da extensão** reescritas com a ação em primeiro lugar. As dez mais prováveis:

| Situação | Texto novo |
|---|---|
| HOST_MISSING | "O aplicativo Ponte não foi encontrado. Abra o Ponte uma vez e reinicie o navegador. Se continuar, reinstale pelo instalador." |
| HOST_FORBIDDEN | "O navegador bloqueou a conexão com o Ponte. Reinstale o Ponte neste computador e reinicie o navegador." |
| HOST_CONNECTION / timeout | "Não recebemos a confirmação do Ponte. Clique em Repetir: nada será duplicado." |
| Página não reconhecida | "Esta página não é a ficha de um documento. Abra o documento pelo SIGADOC e tente de novo." |
| H2/via (SIGA v11.2) | "A ficha ainda não terminou de carregar ou está num formato diferente. Aguarde e tente de novo." |
| Título conflitante | "Não foi possível identificar o assunto com segurança. Confira a ficha no SIGADOC." |
| Texto > 500 KiB | "O documento é grande demais para importar de uma vez. Importe os anexos pelo SIGADOC." |
| PDF 8 MiB / 25 s | pendência visível no painel da página ("1 PDF não baixado: muito grande"), hoje só visível dentro do app |
| Protocolo incompatível | "A extensão e o Ponte estão em versões diferentes. Atualize os dois pelo instalador." |
| Erro DPAPI | "Os dados deste acervo pertencem a outra conta do Windows. Entre com a conta que fez a instalação." |

**2.4 Painel na página do SIGADOC** (`content.js`): só o botão e uma linha de status; sem a linha "Adaptador"; recolhível "Detalhes" com cobertura. Em páginas que não são ficha, mostrar só "Abra um documento para importar", ou nada.

**2.5 Popup da extensão:** uma tela de status (Conectado ao Ponte vX / Não conectado) com o botão "Resolver" que lista a causa provável. Os 3 passos de instalação só aparecem quando não conectado.

**2.6 Rede / Intranet**
- Configuração em assistente de 3 telas: "Este computador vai ser: só meu / o principal do gabinete / conectar ao principal".
- Principal: "Copiar código de conexão" gera **um** texto (`ponte://host?addr=…&token=…`); Conectado: **um** campo "Cole o código de conexão" + "Testar" + "Conectar".
- Avisar sobre firewall com o botão "Abrir regra do Firewall do Windows" (ou instrução de 1 linha).
- "Gerar novo token" com o serviço ativo: aviso de que os colegas precisarão do novo código.
- Trocar o rádio para Cliente não derruba o Anfitrião antes de salvar.
- Conflitos: comparação campo a campo, pré-visualização do resultado antes de aplicar, confirmação única.
- Fila offline: ação "Enviar agora" e "Descartar esta alteração".

### Onda 3 — Instalação e atualização (5 a 8 dias; depende de decisões externas)

**3.1 Pacote distribuível.** Gerar `Ponte-Setup-vX.Y.Z.exe` (Inno Setup, gratuito, instala por usuário sem administrador) que: copia o executável e a extensão, registra o host no HKCU para Chrome e Edge, cria o atalho e faz backup do acervo antes de atualizar. O usuário faz **1 passo**: duplo clique. O script `install-app.ps1` continua como caminho para quem compila.

**3.2 Extensão sem Modo do desenvolvedor.** Duas opções, em ordem de preferência:
- (a) Publicar na Chrome Web Store como **não listada** com a chave atual (mantém o ID `ecllnakmppjinmednoikimbpnbgbimhg`). Instalação vira 1 clique pelo link; atualizações automáticas. Custo: conta de desenvolvedor (taxa única) e revisão da loja.
- (b) Se a publicação não for possível, pedir ao setor de TI a política `ExtensionInstallForcelist`/`ExtensionInstallSources` apontando para o pacote `.crx` interno. Elimina o modo desenvolvedor e os avisos periódicos do navegador.
- (c) Enquanto nenhuma das duas existir: assistente de primeira execução **dentro do app** com botões "Abrir a página de extensões do navegador", "Copiar o caminho da pasta da extensão" e o verificador "Extensão detectada ✓" (o host já recebe o ping; basta gravar o último ping e exibir).

**3.3 Atualização.** No ping, a extensão envia sua versão e o host responde a dele; se divergirem, o app mostra "Recarregue a extensão do navegador" com o botão que abre `chrome://extensions`. O instalador remove versões antigas (hoje ficam no disco e podem ser abertas por engano).

**3.4 Documentação do usuário.** Um `GUIA_DO_USUARIO.md` (ou página no próprio app em "Menu ▸ Ajuda") com: instalar, importar, anotar, responder, acompanhar prazo, trabalhar em equipe, o que fazer se der erro. Sem seletores CSS, hashes ou nomes de tabelas. O `README.md` atual passa a ser documentação de desenvolvimento.

**3.5 Conta do Windows e backup.** Explicar em um parágrafo na primeira execução e na janela de backup: os dados só abrem na conta do Windows que os criou; o backup serve para o mesmo computador e conta, ou para restauração pelo suporte. Adicionar "Restaurar backup…" no menu (hoje não existe caminho de restauração).

### Onda 4 — Validar com pessoas reais (2 dias + 1 por rodada)

Sem isto, as ondas anteriores são hipóteses bem fundamentadas, não melhorias confirmadas.

**Roteiro de teste** (5 participantes do gabinete, 30 min cada, sem ajuda do observador):
1. Instalar o Ponte e importar um documento do SIGADOC.
2. Anotar uma observação e criar uma tarefa com prazo para a próxima semana.
3. Concluir essa tarefa.
4. Registrar uma entrada no controle do gabinete e ligá-la ao documento importado.
5. (Equipe) Conectar ao computador principal usando o código recebido.

**Medir:** tempo até concluir, cliques, quantas vezes pediu ajuda ou desistiu, em que tela travou, palavras que o participante usou para descrever o que via (alimenta o glossário da Onda 2).

**Critério de sucesso:** 4 de 5 concluem as tarefas 1 a 4 sem ajuda; tempo da tarefa 1 abaixo de 10 min com instalador, abaixo de 3 min com instalador + loja.

---

## 4. O que não muda

- Validação de identidade documental, limites de tamanho e origem das URLs (`model.rs`).
- Cifragem DPAPI de todo conteúdo persistido (`protection.rs`).
- Chave e ID da extensão (`manifest.key`, `extension-id.txt`, `extension-key.txt`).
- Imutabilidade das capturas, lixeira reversível, histórico de versões.
- Modelo de dados e esquema do SQLite (toda a mudança é de apresentação e empacotamento).

---

## 5. Estado da implementação (2026-11-03, v0.9.0)

| Onda | Estado |
|---|---|
| 0 — Correções de comportamento | **Concluída** (13 itens; sincronização em thread própria; mesclagem em três vias com base guardada) |
| 1 — Reduzir o que está na frente | **Concluída** (ficha em 4 abas, menu único, filtros escondidos, SAIOP com ficha ao lado e grade ordenável, tarefas em duas colunas, textos em dicas, avisos temporários, modal de saída com "Ir para") |
| 2 — Vocabulário, datas e rede | **Concluída** (glossário aplicado, campo de data, mensagens da extensão, painel e popup, código de conexão, conflitos campo a campo) |
| 3 — Instalação e atualização | **Concluída no que depende do código**: instalador `.exe` (`installer\Ponte.iss`, `dist\Ponte-Setup-0.9.0.exe`), backup e limpeza, verificador na tela inicial, `GUIA_DO_USUARIO.md`, pacote e ficha para a Chrome Web Store (`extension\WEBSTORE.md`). Pendente de ação externa: envio à loja com a conta do gabinete e registro do ID em `extension-store-id.txt`. |
| 4 — Validar com pessoas reais | **Pendente** — roteiro na seção 3. |

## 6. Ordem recomendada e marcos

| Marco | Conteúdo | Esforço acumulado |
|---|---|---|
| **v0.8.1** | Onda 0 completa | ~3 dias |
| **v0.9.0** | Onda 1 (ficha em 4 abas, barra superior, SAIOP com filas em combo e seletor de arquivo, feedback) + Onda 2.1–2.3 (glossário, datas, mensagens) | ~12 dias |
| **v0.9.1** | Onda 2.4–2.6 (painel da página, popup, rede em assistente) | ~16 dias |
| **v1.0.0** | Onda 3 (instalador, extensão sem modo desenvolvedor, guia do usuário) | ~24 dias |
| **Contínuo** | Onda 4 após cada marco | +1 dia por rodada |

Se for preciso escolher uma única coisa: **Onda 0 + itens 1.1, 1.3 e 1.6**. São as mudanças com maior redução de carga visível por dia de trabalho, e nenhuma delas toca em dados ou segurança.
