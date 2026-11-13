# Ponte — Assistente documental SIGADOC

O **Ponte** é um programa para Windows que guarda, no seu computador, os documentos que você importa do SIGADOC, com texto, PDFs e histórico de tramitação. Com eles guardados, você anota, prepara minutas, acompanha prazos (com um aviso diário do Windows, se você quiser), controla as demandas do gabinete, reserva salas, livros e equipamentos e, se quiser, trabalha em equipe.

Ele tem duas partes, que funcionam juntas:

- o **aplicativo**, que abre pelo menu Iniciar e guarda tudo;
- a **extensão do navegador** (Chrome ou Edge), que coloca o botão **Importar para o assistente** nas páginas do SIGADOC e, se você ligar, um painel para guardar conversas no WhatsApp Web ([5.8](#58-conversas-do-whatsapp)).

**O que o Ponte não faz:** ele nunca envia nada ao SIGADOC. Assinar, tramitar e despachar continuam sendo feitos lá. O Ponte só lê, no seu navegador, as páginas às quais você já tem acesso.

**Onde ficam os dados:** no seu computador, cifrados pela sua conta do Windows. Nada vai para a nuvem, a não ser que você escolha uma das opções de IA em nuvem ou a pasta compartilhada, que são explicadas e avisadas no próprio programa.

| | |
|---|---|
| **Instalador** | **[Baixar o Ponte para Windows](https://github.com/demusis/ponteproj/releases/latest/download/Ponte-Setup.exe)** (sempre a versão mais recente) |
| **Extensão do navegador** | [Ponte na Chrome Web Store](https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah) (serve para Chrome e Edge; exige o Ponte 0.19.3 ou posterior) |
| **Guia rápido** | [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md), em onze seções curtas. Dentro do programa: **Ajuda**, na barra lateral |
| **Versão** | 0.34.0 (extensão 0.4.0) — notas de cada versão em [Releases](https://github.com/demusis/ponteproj/releases) |
| **Requisitos** | Windows 10 ou 11, Chrome ou Edge, acesso ao SIGADOC. Não precisa de administrador |
| **Autoria** | Carlo Ralph De Musis e João Gabriel Bezerra Estrela — ver [Autoria e licença](#14-autoria-e-licença) |
| **Licença** | [MIT](LICENSE) |

---

## Sumário

1. [Instalar em 5 minutos](#1-instalar-em-5-minutos)
2. [Conceitos em uma página](#2-conceitos-em-uma-página)
3. [A janela do Ponte](#3-a-janela-do-ponte)
4. [Um passeio com um documento de exemplo](#4-um-passeio-com-um-documento-de-exemplo)
5. [Documentos](#5-documentos)
6. [Tarefas e prazos](#6-tarefas-e-prazos)
7. [Atendimentos (controle do gabinete)](#7-atendimentos-controle-do-gabinete)
8. [Mais de um computador: equipe e pasta compartilhada](#8-mais-de-um-computador-equipe-e-pasta-compartilhada)
9. [IA opcional](#9-ia-opcional)
10. [Dados, segurança, backup e lixeira](#10-dados-segurança-backup-e-lixeira)
11. [Solução de problemas](#11-solução-de-problemas)
12. [Perguntas frequentes](#12-perguntas-frequentes)
13. [Para desenvolvedores](#13-para-desenvolvedores)
14. [Autoria e licença](#14-autoria-e-licença)

---

## 1. Instalar em 5 minutos

Tudo é instalado na sua conta do Windows. Não é preciso senha de administrador.

### Passo a passo

1. **Baixe o instalador** por este link direto, que sempre aponta para a versão mais recente:
   **<https://github.com/demusis/ponteproj/releases/latest/download/Ponte-Setup.exe>**
2. **Dê dois cliques** em `Ponte-Setup.exe` e avance até o fim. Pode deixar marcado **Abrir o Ponte agora**.
   Se o Windows mostrar o aviso "O Windows protegeu o computador", clique em **Mais informações** e em **Executar assim mesmo**: o instalador não é assinado por uma autoridade paga.
3. **Feche e abra o navegador** (Chrome ou Edge).
4. **Instale a extensão**, uma única vez, por um destes dois caminhos:
   - **Pela loja (recomendado):** na tela inicial do Ponte, no card **Primeiros passos**, passo **Extensão instalada no Chrome ou no Edge**, clique em **Abrir a loja de extensões**; ou abra <https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah>. Clique em **Adicionar ao Chrome**. No Edge, permita antes "extensões de outras lojas".
   - **Pela pasta instalada (alternativa, sem acesso à loja):** no mesmo passo, abra **Instalar pela pasta (alternativa)** e clique em **Copiar caminho** (**Abrir pasta** mostra a pasta no Explorador de Arquivos). No navegador, abra `chrome://extensions` (ou `edge://extensions`), ative o **Modo do desenvolvedor**, clique em **Carregar sem compactação** e cole o caminho na barra de endereço da janela que abre.
5. **Feche e abra o navegador** de novo.

### Como saber se deu certo

| Onde olhar | O que deve aparecer |
|---|---|
| Tela inicial do Ponte, card **Primeiros passos** | **Ponte ligado ao navegador** marcado, com *Registrado para o Chrome e Edge.* |
| Ícone da extensão, na barra do navegador | *✓ Ligado ao Ponte 0.34.0.* (a versão instalada) |
| Uma ficha de documento no SIGADOC | o painel **Importar para o assistente**, no canto inferior direito |

O passo **Extensão instalada no Chrome ou no Edge** só é marcado depois que o primeiro documento chega pela extensão; **Primeiro documento importado**, quando o acervo já tem documentos do SIGADOC. Se algum dos três itens da tabela não aparecer, vá para [Solução de problemas](#11-solução-de-problemas).

### O que o instalador faz

- Copia o programa para `%LOCALAPPDATA%\Programs\PonteProj\vX.Y.Z` e a extensão para `%LOCALAPPDATA%\Programs\PonteProj\extension`.
- Registra a ligação entre o navegador e o Ponte, para o Chrome e para o Edge, só na sua conta do Windows.
- Cria o atalho **Ponte** no menu Iniciar (e na área de trabalho, se você marcar).
- **Faz uma cópia de segurança do acervo** antes de mudar qualquer coisa.
- Remove as versões antigas do programa.

### Atualizar e desinstalar

- **Atualizar:** feche o Ponte e rode o instalador novo. Seus dados são mantidos. A extensão da loja se atualiza sozinha. Para saber se há versão nova, use **Ajustes ▸ Sobre o Ponte ▸ Verificar se há versão nova** ([10.6](#106-o-que-sai-do-computador)); o Ponte não baixa nem instala nada sozinho.
- **Desinstalar:** em *Configurações ▸ Aplicativos instalados* do Windows. Os seus dados **não** são apagados; eles ficam em `%LOCALAPPDATA%\PonteProj`.
- **Outra pessoa no mesmo computador:** cada conta do Windows instala o Ponte para si e tem o próprio acervo. Uma conta não vê os dados da outra.

---

## 2. Conceitos em uma página

Vale ler antes de usar: são as palavras que aparecem nas telas.

| Palavra | O que é |
|---|---|
| **Acervo** | O arquivo do Ponte no seu computador, com tudo o que você importou e escreveu. É cifrado pela sua conta do Windows. |
| **Documento** | Um processo, ofício, memorando ou outro expediente do SIGADOC que você importou. No SIGADOC, processo e expediente são igualmente "documentos", e é assim que o Ponte os trata. Cada um tem um número oficial, como `ORG-OFI-2026/00042`. |
| **Versão importada** | Uma fotografia do documento no momento da importação. Importar de novo depois de um despacho cria uma nova versão; as anteriores ficam guardadas. |
| **Registro manual** | Um documento que você mesmo cria, para o que não está no SIGADOC: uma ligação, um e-mail, um ofício em papel, uma conversa do WhatsApp. Recebe um número próprio, como `MANUAL-2026/0007`. |
| **Ficha** | A tela de um documento, com quatro abas: Resumo, Documento, Notas e minutas, Mais. |
| **Tarefa** | Uma providência a tomar sobre um documento, com responsável e prazo. Toda tarefa pertence a um documento. |
| **Atendimento** | Uma **passagem** de um documento ou de uma demanda pelo gabinete: quando entrou, com quem está, em que situação, quando saiu. É o que muitos gabinetes controlam em planilha. O mesmo documento pode ter várias passagens, e um atendimento pode existir sem documento. |
| **Recurso** | Uma sala, um livro, um equipamento ou outra coisa que o gabinete reserva ou empresta, com uma quantidade (por exemplo, `Sala de reuniões do 2º andar`, ou `Projetor`). |
| **Uso** | Uma reserva ou um empréstimo de um recurso, com data e hora de início e de fim previsto, quem usa e, no fim, a devolução. Fica na área **Recursos**. |
| **Anexo** | Um arquivo do seu computador que você junta a um documento, a uma tarefa ou a um atendimento. Não confundir com os PDFs do SIGADOC, que vêm na importação. |
| **Minuta** | O rascunho de uma resposta, encaminhamento ou pedido de complementação, escrito no Ponte e depois colado no SIGADOC. |
| **Lixeira** | Para onde vai o que você exclui. De lá se restaura. |

**Documento, tarefa e atendimento: qual a diferença?** O documento é o conteúdo. A tarefa é o que você precisa fazer com ele. O atendimento é o registro administrativo de que ele passou pelo gabinete. Ao importar um documento do SIGADOC, o Ponte cria os três: o documento, uma tarefa de triagem e um atendimento "Em análise".

---

## 3. A janela do Ponte

```
┌──────────────┬───────────────────────────┬──────────────────────────────────────┐
│ Hoje     3   │ Documentos      [Novo ▾]  │ ← Voltar a Hoje                      │
│ Documentos   │ [Buscar...]               │ ORG-OFI-2026/00042  ● 1 em aberto   │
│ Tarefas  2   │ Todos Com tarefa Atrasados│ Abrir no SIGADOC          ⋯          │
│ Atendimentos │ Filtros                   │ Resumo Documento Notas e minutas Mais│
│ Modelos      │ ORG-OFI-2026/00042       │                                      │
│              │ Pedido de revisão...      │ Análise      Dados do documento      │
│              │ ● 1 em aberto             │ Tarefas      Atendimento      Nota   │
│              │ MANUAL-2026/0001          │                                      │
│              │ ✓ 1 concluída  Manual     │                                      │
│              │                           │                                      │
│ Ajustes      │                           │                                      │
│ Ajuda        │                           │                                      │
├──────────────┴───────────────────────────┴──────────────────────────────────────┤
│ ● em dia há 3 min     Sincronizar                                               │
└─────────────────────────────────────────────────────────────────────────────────┘
```

- **Barra lateral (esquerda):** no alto, **Buscar** (Ctrl+K) procura em todo o acervo (veja **Buscar em todo o acervo**, abaixo). Em seguida, escolhe a área. **Hoje** (Ctrl+1) é a tela em que o Ponte abre; com o acervo vazio, abre em Documentos, na tela inicial com o card **Primeiros passos** ([seção 1](#1-instalar-em-5-minutos)). **Documentos** (Ctrl+2) tem a lista de documentos e a ficha do selecionado. **Tarefas** (Ctrl+3) é a lista de tarefas. **Atendimentos** (Ctrl+4) é o controle do gabinete. **Modelos** (Ctrl+5) é a área dos modelos de trabalho ([seção 9.5](#95-modelos-de-trabalho)). **Recursos** (Ctrl+6) é o controle de salas, livros e equipamentos (veja **A área Recursos**, abaixo). Um número vermelho em **Hoje** (tarefas atrasadas ou para hoje, mais atendimentos com prazo vencido ou para hoje), em **Tarefas** (só as tarefas atrasadas ou para hoje) e em **Recursos** (devoluções atrasadas) mostra o que pede atenção. Embaixo ficam **Ajustes** e **Ajuda**, que abre o guia curto.
- **Hoje:** a tela do começo do dia (veja abaixo, em **A área Hoje**).
- **Documentos:** a lista fica à esquerda e a ficha do documento selecionado, à direita. A lista de documentos existe só nesta área.
- **Tarefas e Atendimentos abrem com tudo.** Não seguem o documento selecionado em Documentos. Para ver só os de um documento, veja [6.1](#61-a-área-tarefas) e [7.2](#72-ver-só-os-de-um-documento).
- **Voltar:** quando você vai de uma tarefa, de um atendimento ou de Hoje até um documento, a ficha mostra no alto **Voltar à tarefa**, **Voltar ao atendimento** ou **Voltar a Hoje**, que leva de volta ao ponto de onde você veio.
- **Atalhos de teclado:**

| Atalho | O que faz |
|---|---|
| **Ctrl+1** a **Ctrl+6** | Ir para Hoje, Documentos, Tarefas, Atendimentos, Modelos e Recursos |
| **Ctrl+K** | Abrir a busca em todo o acervo (documentos, tarefas, atendimentos e recursos), de qualquer área |
| **Ctrl+F** | Ir para a busca da área; a partir de outras áreas, para a busca de documentos |
| **Ctrl+N** | Criar: registro manual (em Hoje e Documentos), nova tarefa (em Tarefas: sem documento, ou do documento pelo qual a lista está filtrada), nova entrada (em Atendimentos), nova reserva (em Recursos; na aba **Recursos**, novo recurso) |
| **Ctrl+S** | Salvar o que está em edição: tarefa, atendimento, reserva ou recurso, notas do documento, modelo ou configuração de IA. Em Notas e minutas, salva a nota ou, estando a nota já salva, uma nova versão da minuta |
| **Ctrl+A** | Em Tarefas, no modo de seleção (**Selecionar**, [6.5](#65-ações-em-lote)): selecionar todas as tarefas da lista |
| **Esc** | Fechar a janela ou o painel lateral aberto, se não houver nada sem salvar. Em Tarefas, no modo de seleção: fechar a pergunta aberta e, depois, sair da seleção |

Na área **Modelos**, **Ctrl+F** (buscar modelo), **Ctrl+N** (**Novo modelo**), **Ctrl+S** (**Salvar rascunho**) e **Esc** também funcionam.

- **Barra inferior:** um ponto colorido diz como está a rede (por exemplo `● só neste computador` ou `● em dia há 3 min`). Passe o mouse para a frase completa; clicar nela abre Ajustes ▸ Equipe e sincronização. No modo pasta compartilhada, o botão **Sincronizar** ao lado sincroniza com a pasta na hora. A lista de documentos se atualiza sozinha.
- **Avisos:** mensagens como *Nota salva* e os erros aparecem numa caixa no canto inferior direito. Os de sucesso somem sozinhos; os de erro ficam até você fechar. O progresso das operações demoradas, como a IA e a sincronização, aparece na barra inferior.
- **Tema e tamanho do texto:** em Ajustes ▸ Aparência. O tema é claro ou escuro; o **Tamanho do texto** (*Normal*, *Grande* ou *Maior*) aumenta letras, ícones e botões de todo o Ponte. O menor tamanho a que a janela pode ser reduzida acompanha o tamanho do texto, sem nunca passar do tamanho da tela.
- **Ajustes** reúne o que não é trabalho do dia a dia, em nove seções:

| Seção | Para quê |
|---|---|
| **Equipe e sincronização** | Trabalhar em equipe ou em mais de um computador; conflitos de edição e alterações a enviar ([seção 8](#8-mais-de-um-computador-equipe-e-pasta-compartilhada)) |
| **E-mail** | Contas, pasta e filtro, lista de mensagens e importação, tudo na própria página, e o botão **Importar um e-mail salvo em arquivo (.eml)…** ([5.5](#55-registros-manuais)) |
| **Importação do SIGADOC** | Criar atendimento ao importar do SIGADOC ([7.1](#71-de-onde-vêm-os-atendimentos)) |
| **Lembretes** | Aviso diário de prazos do Windows: ligar ou desligar, **Horário** e **Mostrar um aviso de teste** ([6.6](#66-lembretes-de-prazos)) |
| **Inteligência artificial** | Escolher onde a IA roda ([seção 9](#9-ia-opcional)). Salva com **Salvar** ou ao sair da seção |
| **Cópia de segurança e exportação** | **Fazer cópia agora**, onde as cópias ficam (**Abrir pasta**) e **Exportar dados para CSV…** ([10.3](#103-backup), [10.5](#105-exportar-dados-csv)) |
| **Lixeira** | Restaurar itens excluídos e definir o prazo de limpeza |
| **Aparência** | Tema: *Como o Windows*, *Claro* ou *Escuro*; **Tamanho do texto**: *Normal*, *Grande* ou *Maior* |
| **Sobre o Ponte** | Versão, **Guia do usuário**, **Verificar se há versão nova** ([10.6](#106-o-que-sai-do-computador)), **Autores e licença…** e endereço do projeto |

**A área Hoje.** Reúne, numa tela só, o que pede atenção no dia, sem precisar abrir cada área. Faixas no alto contam as tarefas atrasadas, as tarefas para hoje e os atendimentos com prazo vencido ou para hoje. Abaixo ficam:

- **Atrasadas** e **Para hoje:** as tarefas. O círculo à esquerda conclui a tarefa; o clique na tarefa a abre em Tarefas; o clique no documento abre a ficha dele, com **Voltar a Hoje** no alto.
- **Próximos dias:** as tarefas que vencem nos dias seguintes.
- **Atendimentos com prazo vencido ou para hoje:** um clique abre o atendimento em Atendimentos.
- **Documentos recentes:** os últimos documentos trabalhados; um clique abre a ficha.

Com recursos cadastrados, a tela mostra também um card fixo, **Recursos em uso e reservados para hoje**, numa lista só: primeiro as devoluções atrasadas, depois os recursos em uso e, por fim, as reservas de hoje; um clique abre o uso. Sem nada em uso nem reservado, o card continua na tela, com *Nenhum recurso em uso nem reservado para hoje* e o botão **Abrir Recursos**. Só some num acervo sem recursos cadastrados.

Quando nada está atrasado nem para hoje, a tela diz *Nenhuma tarefa atrasada ou para hoje*. Ao concluir uma tarefa por aqui, o aviso do canto inferior direito traz **Desfazer**, por alguns segundos.

**Calendário de prazos.** No alto de Hoje, os botões **Lista** e **Calendário** trocam a visão. O calendário mostra o mês com as tarefas e os atendimentos que têm prazo: vermelho é atraso, âmbar é para hoje, azul está no prazo, e os atendimentos levam um ícone. As setas trocam de mês e **Hoje** volta ao mês atual. Clique num item para abri-lo; clique em "+N", ou num dia com itens, para listar os desse dia sob o calendário (**Fechar** esconde a lista). Tarefas concluídas e atendimentos encerrados não aparecem.

**Buscar em todo o acervo.** **Buscar**, no alto da barra lateral, ou **Ctrl+K**, abre uma caixa de busca sobre qualquer área. Digite pelo menos duas letras: o Ponte procura em documentos (número, assunto, texto, notas e PDFs), tarefas e atendimentos, e agrupa os resultados em **Documentos**, **Tarefas** e **Atendimentos**, com até 8 de cada e uma linha "e mais N" quando há outros. Nas tarefas e nos atendimentos, a busca ignora maiúsculas e acentos. As setas movem o destaque, **Enter** (ou o clique) abre o item, levando à ficha, à tarefa ou ao atendimento, e **Esc** fecha. Os recursos e as reservas de recursos também são encontrados, em grupos próprios. A busca da própria área (**Ctrl+F**) continua como antes.

**A área Recursos.** Controla o que o gabinete reserva ou empresta: salas, livros, equipamentos. Cada **recurso** é cadastrado uma vez (nome, **quantidade**, onde fica, identificação); cada reserva ou empréstimo é um **uso**, com data e hora de início e de fim previsto, quem usa, setor e finalidade. A área tem três abas: **Agora e próximos** (atrasados, em uso, hoje e os próximos dias, com busca e filtros), **Calendário** (o mês, por recurso ou todos) e **Recursos** (o cadastro). O Ponte não guarda duas reservas que passem da quantidade do recurso no mesmo horário: recusa e diz quem está com ele e até quando. Os botões **Reservar**, **Registrar uso agora**, **Registrar devolução**, **Prorrogar**, **Cancelar a reserva**, **Reabrir** e **Apagar de vez…** ficam na ficha do uso; o uso pode ser ligado a um documento, a um atendimento e a uma tarefa, e **Criar tarefa de devolução** cria a tarefa com prazo no dia do fim previsto. Um recurso que já teve usos não é apagado: fica **Fora de uso**. Documentos, atendimentos e tarefas mostram os usos ligados a eles e têm **Reservar recurso…**; Hoje e o lembrete diário contam as reservas de hoje e as devoluções atrasadas. **Exportar dados para CSV…** ganha `recursos.csv` e `usos-de-recursos.csv` ([10.5](#105-exportar-dados-csv)), e **Exportar esta lista…** grava só os usos filtrados. Em equipe, duas reservas do mesmo horário feitas sem conexão viram um **conflito de edição** ([seção 8](#8-mais-de-um-computador-equipe-e-pasta-compartilhada)); os horários são os do relógio de cada computador. Esta versão muda o formato do acervo: atualize todos os computadores e o celular juntos. No Ponte de bolso 0.7.0, Recursos é só de consulta.

**Datas:** todo campo de data aceita digitar `dd/mm/aaaa` ou escolher no calendário, pelo ícone de calendário. O calendário abre o mês, com os dias da semana (D S T Q Q S S), setas ◀ ▶ para trocar de mês, hoje em destaque e os atalhos **Hoje** e **Limpar**. Uma data inválida, como `31/02/2026`, fica marcada em vermelho.

**Nada se perde ao trocar de tela.** Se há algo não salvo, o Ponte pergunta antes de sair, e ao fechar o programa lista cada pendência com um botão **Ir para**.

---

## 4. Um passeio com um documento de exemplo

Suponha que chegou ao gabinete o ofício fictício **ORG-OFI-2026/00042 — Pedido de revisão de materiais de treinamento**, com prazo de resposta em 10 dias.

### 4.1 Importar

No SIGADOC, abra o documento (a ficha dele, não a lista de resultados). No canto inferior direito da página aparece o painel da extensão, já com o documento identificado:

```
┌──────────────────────────────────────────────┐
│          Importar para o assistente          │
│ ORG-OFI-2026/00042 — Pedido de revisão de   │
│ materiais de treinamento                     │
│ Importa o texto, os PDFs e o histórico deste │
│ documento para o Ponte.                      │
│ ▸ O que será importado                       │
└──────────────────────────────────────────────┘
```

Clique no botão. Em alguns segundos aparece *Importado. O Ponte abriu o documento.*, e a janela do Ponte abre na ficha. Se algum PDF não puder ser baixado (sessão expirada, arquivo grande demais), a própria mensagem diz qual.

Nesse momento o Ponte criou três coisas: o **documento**, uma **tarefa de triagem** e um **atendimento** "Em análise" com a data de hoje e você como responsável.

### 4.2 Ler

A ficha abre na aba **Resumo**, em cartões: **Análise** (por IA, opcional), **Dados do documento**, **Tarefas**, **Atendimento** e **Nota**. A aba **Documento** traz o texto importado, os PDFs (com leitor próprio e reconhecimento de texto local), os dados do SIGADOC e o histórico de tramitação.

### 4.3 Anotar

Em **Notas e minutas**, escreva nas notas, por exemplo:

> Falar com a Coordenação de Capacitação antes de responder. Planilha anexa tem valores desatualizados (ver p. 3 do PDF).

Clique em **Salvar notas**. O ponto laranja na aba some quando tudo está salvo.

### 4.4 Criar a tarefa com prazo

No Resumo, no cartão **Tarefas**, clique em **Nova tarefa**. Preencha:

| Campo | Valor |
|---|---|
| Título | `Responder ao ofício` |
| Responsável | clique em **Eu** |
| Ação | *Responder* |
| Prazo | `13/11/2026` (digite ou use o calendário) |

Salve. A etiqueta azul embaixo do cartão do documento, na lista da esquerda, passa a mostrar `● 2 em aberto (no prazo) • Prazo: 13/11/2026`. No dia do prazo ela fica âmbar, `⏰ 1 vence hoje (+1 no prazo)`, e depois dele fica vermelha, `⚠ 1 tarefa atrasada (+1 no prazo) • Prazo: 13/11/2026`.

### 4.5 Responder

Ainda em **Notas e minutas**, na parte **Minuta**, que tem três passos:

1. **1. Partir de:** escolha o **Tipo**: *Resposta*. Para usar um modelo de trabalho, escolha-o em **Modelo de trabalho** ([seção 9.5](#95-modelos-de-trabalho)). Clique em **Começar por um modelo** (texto-base, sem IA).
2. **2. Pedir à IA (opcional):** se quiser, escreva o **Pedido desta minuta** e clique em **Gerar com IA**.
3. **3. Texto:** edite o texto e clique em **Salvar nova versão** (ou Ctrl+S, com a nota já salva).
4. **Exportar (RTF)** abre a pasta com o arquivo para o Word, ou **Copiar** para colar direto.

O texto final você cola no SIGADOC. O Ponte não tramita nem assina.

### 4.6 Concluir

Com a resposta enviada pelo SIGADOC, clique no círculo à esquerda de cada tarefa, na área **Tarefas**. O cartão passa a `✓ 2 tarefas concluídas`. Na área **Atendimentos**, abra o atendimento e clique em **Encerrar**, informando a data de saída e para onde foi.

---

## 5. Documentos

### 5.1 A lista de documentos

A lista fica na área **Documentos**, à esquerda da ficha.

- A busca procura em número, assunto, texto, notas, minutas e nos PDFs já lidos.
- Os atalhos *Todos*, *Com tarefa* e *Atrasados · N* filtram a lista na hora.
- **Filtros** abre os demais filtros e a ordem:

| Filtro | Opções |
|---|---|
| Situação das tarefas | *Todos*, *Com tarefa*, *No prazo*, *Atrasados*, *Concluídos* |
| Prioridade | todas, urgente, alta, normal, baixa, não classificada |
| Origem | *Todos*, *SIGADOC*, *Manuais* (aparece quando há registros manuais) |
| **Ordem** | *Mais recentes primeiro* (padrão), *Prazo mais próximo primeiro*, *Prioridade (urgente primeiro)*, *Número do documento*, *Assunto (A–Z)* |

- **Novo** abre um menu: **Registro manual…** cria um documento que não está no SIGADOC ([5.5](#55-registros-manuais)); na janela dele, **Colar conversa do WhatsApp** cria o registro de uma conversa ([5.8](#58-conversas-do-whatsapp)); **Importar e-mail de um arquivo (.eml)…** e **Importar da conta de e-mail…** trazem e-mails.

### 5.2 Como ler um cartão

Cada cartão mostra o número e o assunto e, embaixo, uma **etiqueta colorida** com a situação das tarefas do documento. O Ponte escolhe a etiqueta pela primeira regra que se aplicar, nesta ordem:

| Ordem | Cor da etiqueta | Quando | Exemplo |
|---|---|---|---|
| 1 | vermelha | há pelo menos uma tarefa atrasada (prazo anterior a hoje, não concluída) | `⚠ 1 tarefa atrasada (+3 no prazo) • Prazo: 20/10/2026` |
| 2 | âmbar | nenhuma atrasada, mas alguma vence hoje | `⏰ 1 vence hoje (+2 no prazo)` |
| 3 | azul | há tarefas em aberto, todas no prazo | `● 2 em aberto (no prazo) • Prazo: 13/11/2026` |
| 4 | verde | nenhuma em aberto e pelo menos uma concluída | `✓ 1 tarefa concluída` |
| 5 | cinza | o documento não tem tarefas | `○ Sem tarefas` |

Como ler os números: o primeiro conta as tarefas na situação que dá a cor (atrasadas, ou que vencem hoje), e o que vem entre parênteses são as demais tarefas em aberto. **Prazo** é o prazo mais próximo entre as tarefas em aberto. Nas etiquetas azul e âmbar, um `• ✓ 2` ao final diz quantas já foram concluídas.

O pior caso manda: um documento com uma tarefa atrasada e três no prazo recebe a etiqueta vermelha. Uma tarefa sem prazo conta como "no prazo". Só a etiqueta muda de cor; o cartão em si não.

Outras marcas do cartão: **Manual** (registro manual), `📎 N juntado(s)` (o documento tem outros juntados a ele), `↳ Juntado a …` (está juntado a outro) e *análise desatualizada* (a análise por IA foi feita sobre uma versão anterior).

**Documentos juntados.** Quando a ficha do SIGADOC diz *Juntado ao documento: …*, o Ponte liga os dois sozinho, em qualquer ordem de importação. O juntado aparece logo abaixo do principal, recuado, com a marca ↳.

### 5.3 As quatro abas da ficha

| Aba | O que tem |
|---|---|
| **Resumo** | Cartões **Análise** (botão **Analisar**, depois **Analisar de novo**; quando a análise é anterior às últimas alterações do documento, do contexto ou das notas, uma faixa avisa e também oferece **Analisar de novo**; as **Propostas (revise antes de usar)** de tarefa, atendimento e reserva abrem formulários já preenchidos, [seção 9.4](#94-propostas-conferidas-fila-e-envio); numa análise antiga, as providências sugeridas têm **Criar tarefa**), **Dados do documento** (**Ler o PDF**, **Completar contexto**), **Tarefas** (**Nova tarefa**, **Ver todas**), **Atendimento** e **Nota**; documentos parecidos. |
| **Documento** | **Arquivos (PDF)** com **Ler no Ponte** e exportação (primeiro, quando o documento tem arquivos); **Texto importado**; **Anexos** (arquivos seus); dados do SIGADOC; histórico de tramitação. |
| **Notas e minutas** | **Notas** e **Minuta**. Com a janela larga, as notas ficam à esquerda e a minuta à direita; com a janela estreita, a nota mostra as primeiras linhas, com **Editar nota** (e **Recolher**). O **Editar nota** do card Nota, no Resumo, abre esta aba com a nota aberta e o cursor nela, pronta para digitar. A minuta tem três passos. **1. Partir de:** **Tipo**, **Modelo de trabalho** e **Começar por um modelo**. **2. Pedir à IA (opcional):** **Pedido desta minuta** e os botões **Gerar com IA**, **Completar** e **Revisar**. **3. Texto:** o editor, **Salvar nova versão**, **Copiar**, **Exportar (RTF)**, **Comparar com versão ▾** e **Guardar como modelo**. Abaixo ficam as versões salvas, cada uma com **Material utilizado** quando foi gerada com IA. |
| **Mais** | Vínculos entre documentos (automáticos ou feitos por você); versões importadas, com comparação; o que foi importado e os links encontrados na página; dados técnicos, com as datas escritas como dd/mm/aaaa hh:mm. |

O alto da ficha mostra o número, as etiquetas de situação, a situação de juntada (com **Abrir documento principal**), **Abrir no SIGADOC** e o botão **⋯** (**Mais ações**), à direita, que traz **Imprimir ou salvar a ficha…** ([5.7](#57-imprimir-ou-salvar-a-ficha)) e **Excluir…** (o documento vai para a Lixeira).

### 5.4 Importar de novo o mesmo documento

Importar de novo nunca duplica. O Ponte compara o conteúdo da página (texto, PDFs e dados) com o que já está no acervo:

| A página do SIGADOC está… | O que acontece |
|---|---|
| **igual** à versão em uso | **Nada é gravado.** O painel da extensão avisa, em laranja: *"Importação ignorada: o SIGADOC mostra o mesmo conteúdo já importado em 04/11/2026 12:38. Nada foi gravado. Se o documento mudou, atualize a página (F5) e importe de novo."* O Ponte nem abre. |
| **diferente** (despacho novo, anexo novo, texto alterado) | Guarda uma **nova versão**. As anteriores ficam em **Mais ▸ Versões importadas**, com a comparação do que mudou. Análises e minutas antigas são marcadas como feitas sobre uma importação anterior. |
| igual a uma **versão anterior** (voltou atrás) | Nada novo é gravado; aquela versão volta a ser a atual, e o painel avisa. |

O painel avisa antes do clique: **Já está no Ponte — importado em…** (com **Abrir no Ponte** e o botão **Importar de novo**) ou **Ainda não está no Ponte**. Se o Ponte não responde, o aviso aparece ali mesmo, com **Tentar de novo**. O atalho **Alt+Shift+I** faz o mesmo que o botão (pode ser trocado em chrome://extensions/shortcuts). Com o Ponte já aberto, a importação traz a janela existente para a frente, no documento; não abre outra.

Se a confirmação de uma importação se perder no caminho, o botão vira **Repetir importação**. Repetir não duplica: o Ponte responde *Importação confirmada*.

**Nota e prioridade na hora de importar.** No painel, **Adicionar nota ou prioridade** abre dois campos (o link passa a dizer **Ocultar nota e prioridade**): **Nota para o Ponte**, com até 2000 caracteres, e **Prioridade** (*Não mudar*, *Baixa*, *Normal*, *Alta*, *Urgente*). Ao clicar em **Importar para o assistente**, o Ponte importa o documento e, em seguida:

- **acrescenta a nota** ao fim da nota do documento, num parágrafo novo com a data (`10/11/2026 — responder até sexta`); o que já estava escrito nunca é substituído, e repetir a mesma importação não acrescenta a mesma linha duas vezes;
- **muda a prioridade** do documento, se você escolheu uma.

O painel confirma com *Nota e prioridade enviadas.* (ou *Nota enviada.* / *Prioridade enviada.*), e os campos voltam vazios. Exige o **Ponte 0.31** e a **extensão 0.3.0**: com um Ponte mais antigo, o link não aparece e o ícone da extensão avisa que é preciso atualizar o Ponte; se o Ponte for antigo demais para receber a nota, a importação é feita sem ela e o painel diz *O Ponte instalado não recebe nota nem prioridade; a importação foi feita sem elas. Atualize o Ponte.* Se a nota ou a prioridade for recusada (mais de 2000 caracteres, por exemplo), nada é importado e o painel diz o motivo. Se for a gravação que falhar, a importação vale e o painel diz o que faltou, por exemplo *A importação foi feita, mas a nota não foi salva. Escreva a nota na ficha do documento.* (ou, quando o documento foi alterado ao mesmo tempo por outra pessoa da equipe, acrescenta *porque o documento foi alterado ao mesmo tempo*). A nota vai só ao Ponte do mesmo computador; daí em diante, é uma nota comum e acompanha a equipe ou a pasta compartilhada, se você usa uma delas.

**Importações recentes.** Ao clicar no ícone da extensão, a janelinha lista **Importações recentes**: as últimas importações desta sessão do navegador, até 8, com número, assunto, hora e resultado (*Importado*, *Já estava no Ponte* ou *Não concluída*). **Abrir no Ponte** abre o documento na ficha (com o Ponte fechado, a janelinha pede para abri-lo). **Limpar a lista** esvazia a lista. Ela fica só na memória de sessão do navegador: some ao fechar o navegador, não é enviada a lugar nenhum e guarda apenas número, assunto, hora e resultado, nunca o texto do documento nem a nota.

**Limites de uma importação:** 500 KB de texto, até 10 PDFs, 8 MB por PDF e 16 MB de PDFs no total. O que passar disso é avisado; o texto da página é importado mesmo assim.

### 5.5 Registros manuais

Para o que não está no SIGADOC. Clique em **Novo ▸ Registro manual…** (ou **Criar registro manual…** na tela inicial) e preencha:

| Campo | Exemplo |
|---|---|
| Tipo | *Memorando ou comunicação interna*, *Ligação telefônica*, *E-mail recebido*, *Ofício ou documento em papel*, *Reunião ou despacho verbal*, *Outro* |
| Título | `Ligação do gabinete sobre a reforma` |
| Origem (de quem, de onde) | `Gabinete do Secretário` |
| Data do documento | `03/11/2026` |
| Texto | o que foi pedido, decidido ou recebido |

Ao salvar, o registro recebe o número `MANUAL-ano/sequência`, que nunca se confunde com um número do SIGADOC, e aparece com a etiqueta âmbar **Manual** na lista, na ficha e nas tarefas. E-mails importados (por arquivo ou pela conta) têm numeração própria, `EMAIL-ano/sequência`, e a etiqueta **E-mail**; os importados pelas versões 0.22 e 0.23 recebem o número novo na primeira abertura da 0.23.2, quando o acervo é usado sem equipe (em equipe mantêm o número antigo, com a etiqueta **E-mail**). Tarefas, notas, minutas, anexos, análise por IA, Lixeira e compartilhamento com a equipe funcionam como em qualquer documento. **Editar registro…** salva uma nova versão; as anteriores ficam em Mais ▸ Versões. Para guardar o arquivo original (o e-mail, a foto do papel), use **Documento ▸ Anexos**.

Em equipe, o número é dado pelo computador principal. Se ele estiver fora de alcance, o registro fica como "pendente" até a próxima sincronização.

**E-mails salvos como arquivo.** Um e-mail pode entrar pronto, sem digitar: salve-o como `.eml` (no Gmail, abra a mensagem ▸ ⋮ ▸ **Mostrar original** ▸ **Baixar original**; no Outlook, **Salvar como**) e use **Novo ▸ Importar e-mail de um arquivo (.eml)…** ou **Ajustes ▸ E-mail ▸ Importar um e-mail salvo em arquivo (.eml)…**. O Ponte lê o arquivo neste computador (nada é lido da sua conta de e-mail) e cria um registro manual do tipo *E-mail recebido* com remetente, destinatários, data e assunto no texto, o corpo da mensagem (texto simples, ou o HTML convertido) e os arquivos anexados como **Anexos** do registro. Imagens embutidas sem nome e partes ilegíveis são deixadas de fora, com aviso. O mesmo e-mail importado duas vezes é reconhecido pelo identificador da mensagem e não é duplicado. Nos modelos de trabalho, um `.eml` anexado a um exemplo tem o texto extraído só com assunto e corpo, sem endereços.

**Direto da conta de e-mail (IMAP).** **Novo ▸ Importar da conta de e-mail…** abre uma janela com a lista de mensagens da última conta, pasta e filtro usados, para marcar e importar; **Trocar pasta ou conta…** leva à configuração completa. Na lista de documentos, um e-mail aparece pelo endereço e nome do remetente, com a data em que foi importado. **Ajustes ▸ E-mail** traz essa configuração dentro da página, sem janela à parte: as suas contas, a pasta e o filtro e a lista de mensagens. Cadastre uma (nome, servidor, usuário e **senha de app**; para o Gmail pessoal, ative a verificação em duas etapas e crie a senha em Conta Google ▸ Segurança ▸ Senhas de app: a senha normal não funciona). A senha fica selada só neste computador, e só se você marcar "Lembrar"; sem marcar, é pedida a cada sessão. Conecte, escolha a pasta (a Caixa de entrada, **Com estrela** ou um marcador do Gmail, que aparece como pasta) e o filtro (Todas, Com estrela, Não lidas), liste as cem mais recentes, marque as que quer e clique em **Importar**. Cada mensagem vira o mesmo registro manual do `.eml`, com anexos; as já importadas aparecem indicadas e não se repetem. O Ponte só lê: nada é marcado como lido, movido ou apagado. A conexão é cifrada (TLS) e vai direto ao servidor de e-mail; nada passa por terceiros. Pode haver várias contas.

Uma conversa do WhatsApp também vira registro manual: veja [5.8](#58-conversas-do-whatsapp).

### 5.6 Anexos

Arquivos do seu computador podem ser juntados a um **documento** (Documento ▸ Anexos), a uma **tarefa** (na ficha dela) ou a um **atendimento** (na ficha dele). Tarefa e atendimento precisam estar salvos antes. Clique em **Anexar arquivo…** e escolha o arquivo. Ele passa a fazer parte do acervo, cifrado, e aparece para a equipe. Também dá para **arrastar** arquivos do Explorador de Arquivos do Windows e soltá-los sobre a janela do Ponte, vários de uma vez, desde que haja um único bloco **Anexos** na tela (veja abaixo).

| Regra | Valor |
|---|---|
| Tamanho | até 8 MB por arquivo |
| Quantidade | até 20 por documento, tarefa ou atendimento |
| Tipos aceitos | PDF, Word, Excel, PowerPoint, OpenDocument, texto (.txt, .rtf, .csv), imagens (.png, .jpg, .gif, .bmp, .tif) e e-mails (.eml, .msg) |
| Recusados | programas, scripts, atalhos, arquivos compactados (.zip, .rar) e Office com macros (.docm, .xlsm) |

O tipo é conferido também pelo conteúdo, não só pela extensão. Quando um arquivo é recusado, a mensagem diz o motivo e o que fazer, por exemplo: *"O arquivo tem 12,4 MB; o limite é 8 MB. Para reduzir: salve o PDF com compressão… ou divida o documento em partes"*.

**Arrastar e soltar.** Vale o mesmo limite do botão (8 MB por arquivo, 20 por item) e os mesmos tipos aceitos e recusados; cada arquivo é conferido e o aviso resume quantos foram anexados e por que algum foi recusado. **Pastas são recusadas**: solte os arquivos. Se os arquivos passam do limite de 20, os que sobram ficam de fora, com aviso. O Windows não informa em que ponto da janela o arquivo foi solto, então só um bloco **Anexos** na tela pode receber: com dois ou mais blocos **Anexos** visíveis ao mesmo tempo, o Ponte pede para usar **Anexar arquivo…** do bloco desejado.

Cada anexo tem uma **descrição** opcional e os botões **Ler no Ponte** (PDF) ou **Abrir** (demais tipos, no programa padrão do Windows, a partir de uma cópia temporária apagada ao fechar o Ponte) e **Salvar como…**. Para excluir: em documentos e tarefas, **Excluir…** manda para a Lixeira; em atendimentos, **Apagar de vez…** apaga definitivamente, com confirmação, porque atendimentos não têm Lixeira.

### 5.7 Imprimir ou salvar a ficha

Para levar a ficha de um documento para uma reunião, um processo em papel ou um PDF: no alto da ficha, **⋯ ▸ Imprimir ou salvar a ficha…**. O Ponte pergunta onde guardar o arquivo (o nome sugerido é `Ficha ORG-OFI-2026_00431.html`, com o número do documento), cria **uma página** (arquivo `.html`) e a abre no navegador. De lá, imprima com **Ctrl+P** ou escolha **Salvar como PDF** no lugar da impressora.

**A página traz:** número, título, origem e datas; **Dados do documento**; a **Análise** por IA, com o aviso *Análise feita com apoio de IA. Confira na fonte.* (e, se for o caso, o aviso de que ela é anterior às últimas alterações); as **Tarefas** e os **Atendimentos** do documento; a **Nota**, como está na tela; o texto do documento; **Arquivos e anexos** (só a lista, com os tamanhos) e o **Histórico de tramitação**.

**A página não traz:** os arquivos em si (os PDFs importados e os anexos continuam no Ponte) nem a **minuta**.

Se a nota na tela ainda não foi salva, a página sai com o texto que está na tela, e o aviso do Ponte diz que ela não foi salva no Ponte.

**Atenção:** a página salva é uma **cópia fora do acervo protegido**: quem abrir o arquivo lê o conteúdo, sem a proteção da sua conta do Windows. Guarde-a, ou apague-a, com o mesmo cuidado que teria com o próprio documento, e não a deixe em pasta que outras pessoas leem sem necessidade.

### 5.8 Conversas do WhatsApp

Uma conversa do WhatsApp pode ficar guardada no Ponte, junto dos documentos, com tarefas e notas. O registro é do tipo **Conversa do WhatsApp** e leva a etiqueta **WhatsApp** na lista. São conversas com outras pessoas: guarde só o que o trabalho exige.

**Primeira forma: copiar e colar (não precisa da extensão).**

1. No WhatsApp (no navegador, no aplicativo do computador ou no celular), selecione as mensagens e copie. Se copiou no celular, leve o texto ao computador, por exemplo enviando-o a você mesmo.
2. No Ponte, **Novo ▸ Registro manual…** e, na janela, **Colar conversa do WhatsApp**.
3. Cole (Ctrl+V) em **Mensagens copiadas**. O Ponte mostra a prévia: quantas mensagens leu, o período, os participantes e a primeira e a última mensagem. Se não encontrar mensagens, diz isso e nada é criado.
4. Se quiser, ajuste **Conversa com** (nome da pessoa ou do grupo) e o **Título** (sugerido como "Conversa com <nome> — data da última mensagem"; pelo menos 3 caracteres).
5. **Criar registro**.

**Segunda forma: pela extensão (0.4.0), no WhatsApp Web.** O recurso vem desligado. Para ligar, uma única vez: clique no ícone da extensão ▸ **Usar no WhatsApp Web**, aceite a pergunta do Chrome (acesso a web.whatsapp.com) e recarregue a página do WhatsApp Web. A página ganha um botão pequeno escrito **Ponte**, no canto inferior esquerdo, que abre o painel **Guardar no Ponte**. Então:

1. Arraste o mouse sobre as mensagens e clique em **Trecho selecionado**; ou clique em **Mensagens visíveis na tela**.
2. Confira a prévia (quantidade de mensagens, período, fotos, primeira e última mensagem) e ajuste **Título** e **Conversa com**. **Nota ou prioridade** permite juntar uma nota ([5.4](#54-importar-de-novo-o-mesmo-documento)).
3. Se quiser, junte arquivos baixados do WhatsApp (PDF, Word…): solte-os sobre o painel ou use **Escolher arquivo…**. As fotos das mensagens escolhidas vão sozinhas.
4. **Enviar ao Ponte**. O painel diz *Guardado no Ponte.* e oferece **Abrir no Ponte** e **Guardar outra conversa**.

Se a página não for reconhecida (*O Ponte não reconheceu esta página do WhatsApp Web*), use **Colar mensagens copiadas**, no painel. Para desligar, ícone da extensão ▸ **Desligar**, que retira a permissão.

**O que o registro guarda.** Número `MANUAL-ano/sequência` e uma tarefa de triagem, como todo registro manual. O texto tem uma linha por mensagem, `[dd/mm/aaaa hh:mm] Nome: mensagem`; as linhas seguintes da mesma mensagem ficam recuadas em quatro espaços, para que uma linha dentro de uma mensagem nunca passe por mensagem de outra pessoa (mensagens do sistema do WhatsApp saem como `[dd/mm/aaaa hh:mm] — texto`). Na ficha, **Dados do documento** traz **Conversa com**, **Período**, **Mensagens** e **Origem** (*Extensão do navegador* ou *Colado no Ponte*). Fotos e arquivos ficam em **Anexos**. Acompanha a equipe ou a pasta compartilhada como qualquer registro manual.

**Limites e cuidados:**

| Regra | Valor |
|---|---|
| Mensagens e texto | até 500 mensagens e 500 KB de texto por registro |
| Arquivos | até 8 MB cada, 10 por envio pelo painel, 16 MB juntos; o registro aceita até 20 anexos ([5.6](#56-anexos)) |
| Sem legenda | fotos e arquivos sem legenda entram sem o nome do autor; se a data não puder ser conhecida com certeza, entram como `[sem data]`, e o painel avisa |
| Arquivos da página | PDF, Word etc. não são lidos da página: baixe-os no WhatsApp e solte-os no painel, ou junte-os depois em **Anexos**. As fotos das mensagens escolhidas são lidas |
| Repetição | a mesma seleção enviada duas vezes pelo painel, ou o mesmo texto colado duas vezes, com o mesmo **Conversa com**, não duplica (os arquivos de uma conversa que já existia não são juntados de novo). Uma conversa guardada pelo painel e depois colada da cópia do WhatsApp pode ficar duas vezes, porque os dois textos não são idênticos |
| Lixeira | se a mesma conversa foi guardada antes e está na Lixeira, o Ponte avisa; restaure-a em **Ajustes ▸ Lixeira** |

**O que a extensão não faz.** Lê só as mensagens escolhidas, só quando você clica (nada em segundo plano, ao abrir a página ou ao rolar). Não lê outras conversas nem contatos, não envia, altera nem apaga nada no WhatsApp e fica desligada até você ligar. O que você escolheu só sai da página em **Enviar ao Ponte** e vai somente ao Ponte deste computador ([10.6](#106-o-que-sai-do-computador)).

---

## 6. Tarefas e prazos

Cada documento importado recebe uma tarefa de triagem. Você cria outras no Resumo do documento (**Nova tarefa**) ou na área **Tarefas**.

### 6.1 A área Tarefas

O alto traz a busca e o botão **Nova tarefa**. A lista fica à esquerda, agrupada em ATRASADAS, PARA HOJE, PRÓXIMAS, SEM PRAZO e CONCLUÍDAS. O círculo à esquerda de cada tarefa a conclui (ou a reabre, se já concluída). Clicar na tarefa abre os dados num painel à direita, com **Salvar**, **Concluir**, **Cancelar** e **Excluir…** embaixo.

- **A área abre com todas as tarefas**, qualquer que seja o documento selecionado em Documentos. Para ver só as de um documento, use **Ver todas** no card Tarefas da ficha dele, ou **Só as de ORG-OFI-2026/00042** no fim da fileira de visões. Enquanto a lista está restrita, uma faixa diz *Mostrando só as tarefas de ORG-OFI-2026/00042 — Pedido de revisão de materiais de treinamento*, com **Ver todas** para voltar a tudo.
- **Ordenar:** à direita das visões, *Prazo*, *Prioridade* ou *Documento*.
- **Lista vazia:** a área diz por quê (por exemplo, nenhuma tarefa na visão escolhida ou na busca).
- **Visões:** *Todas*, *Hoje*, *Próximos 7 dias*, *Atrasadas* e, em **Mais ▾**, *Prontas para fazer* (pendentes sem nenhuma tarefa anterior em aberto), *A triar*, *Aguardando*, *Com lembrete* e *Sem prazo*.
- **Mostrar concluídas** inclui as já concluídas na lista.
- **Tarefa sem documento.** Na área **Tarefas**, **Nova tarefa** (ou **Ctrl+N**) cria uma tarefa que não pertence a nenhum documento; na lista ela aparece como **Sem documento**. Com a lista filtrada por um documento (faixa "Mostrando só as tarefas de…"), a tarefa nova é dele. No painel da tarefa, **Vincular a um documento…** abre uma busca por número, assunto ou texto; depois, **Trocar de documento…** e **Desvincular do documento** mudam ou desfazem o vínculo. A tarefa é a mesma o tempo todo: anexos, lista de providências e histórico ficam com ela.
- O painel da tarefa mostra o número e o assunto do documento a que ela pertence.

### 6.2 Os campos

| Campo | Opções ou exemplo |
|---|---|
| Título | `Responder ao ofício` |
| Responsável | `FULANA`, ou o botão **Eu** |
| Ação | *Triar*, *Examinar*, *Tomar ciência*, *Baixar documento*, *Responder*, *Encaminhar*, *Pedir complementação*, *Acompanhar* |
| Situação | *Pendente*, *Em análise*, *Aguardando*, *Concluída* |
| Prioridade | *Baixa*, *Normal*, *Alta*, *Urgente* |
| Prazo | `13/11/2026` |
| Observações | texto livre |
| **Anexos** | arquivos seus ligados à tarefa ([5.6](#56-anexos)) |
| **Mais detalhes** | origem do prazo (ex.: *art. 5º do ofício*), lembrete, resultado, lista de etapas, dependências |

### 6.3 Regras que vale conhecer

- **Concluir** e reabrir ficam na própria lista, no círculo à esquerda da tarefa. Concluir uma tarefa não registra nada no SIGADOC. Logo após concluir, o aviso do canto inferior direito traz **Desfazer**, por alguns segundos, que reabre a tarefa.
- **Sem sair da tarefa:** na ficha, **Consultar documento** abre o texto e os PDFs numa janela por cima, só para leitura. **Ir para o documento** abre a ficha completa, com **Voltar à tarefa** no alto.
- **Alterações não salvas:** com uma tarefa em edição e alterações não salvas, ao clicar em outro lugar o Ponte pergunta: **Salvar e continuar**, **Descartar alterações** ou **Voltar à tarefa**.
- **Juntados seguem o principal:** concluir a última tarefa aberta de um documento principal conclui também as tarefas abertas dos documentos juntados a ele, cada uma com a anotação *"Concluída automaticamente: o documento principal X foi concluído"*. A mensagem lista o que foi concluído. Reabrir o principal não reabre as peças.

### 6.4 Dependências entre tarefas

Uma tarefa pode depender de outras ("só posso responder depois de receber o parecer"). Marque em **Mais detalhes ▸ dependências**.

| O que você vê | O que significa |
|---|---|
| Tarefa recuada na lista, depois das que ela espera | ela depende de outras |
| `⛓ 1 de 2 anterior(es) em aberto` (âmbar) | quantas das tarefas anteriores ainda faltam; passe o mouse para a lista |
| `⛓ 2 anterior(es) concluída(s)` (verde) | já pode ser feita |
| `↳ depende de:` seguido de botões `○ A`, `⚠ B`, `✓ C` | as tarefas anteriores e a situação de cada uma: aberta, atrasada, concluída. Clicar abre a tarefa |
| `↱ libera:` seguido de botões | as tarefas que esperam por esta |
| barra âmbar ou azul à esquerda de um cartão | com uma tarefa aberta na ficha: âmbar marca as anteriores dela, azul as que dependem dela |

Enquanto houver tarefa anterior em aberto, concluir fica desabilitado, com a dica *"Conclua antes: A, B"*. O Ponte não deixa formar ciclos: as opções que criariam um aparecem desabilitadas e explicadas.

### 6.5 Ações em lote

Para concluir, mudar o prazo, o responsável ou a prioridade, ou excluir várias tarefas de uma vez, use o modo de seleção da área **Tarefas**.

1. Clique em **Selecionar**, ao lado da busca. O botão fica desabilitado enquanto a tarefa aberta no painel tem alterações sem salvar; uma tarefa aberta e sem alterações é fechada.
2. Em cada linha, o círculo de concluir vira um quadrado: clique nele para marcar ou desmarcar a tarefa. Uma faixa sob os filtros diz quantas estão selecionadas e traz **Selecionar todas as visíveis** (ou **Ctrl+A**, quando nenhuma caixa de texto está em uso), **Limpar seleção** e **Sair da seleção** (ou **Esc**). Só as tarefas que aparecem na lista podem ficar selecionadas: se a busca ou a visão as esconde, elas saem da seleção.
3. Com ao menos uma marcada, uma barra aparece embaixo, com **Concluir**, **Prazo…**, **Responsável…**, **Prioridade…** e **Excluir…**.
4. Cada botão pergunta antes de agir, com o número de tarefas, e tem **Cancelar**:

| Botão | Pergunta | Confirmação |
|---|---|---|
| **Concluir** | *Concluir N tarefas?* | **Concluir** |
| **Prazo…** | uma data (digitada ou do calendário) ou **Sem prazo** | **Mudar prazo** (ou **Tirar prazo**) |
| **Responsável…** | o nome, **Eu** ou **Sem responsável** | **Atribuir** (ou **Tirar responsável**) |
| **Prioridade…** | *Baixa*, *Normal*, *Alta* ou *Urgente* | **Mudar prioridade** |
| **Excluir…** | *Excluir N tarefas? Elas vão para a Lixeira.* (desabilitado enquanto outra operação, como a sincronização, está em andamento) | **Excluir** |

Os botões de confirmação só ficam ativos quando a escolha está completa. Mudar a seleção fecha a pergunta aberta.

**O que fica de fora.** Cada tarefa passa pelas mesmas regras de uma alteração feita uma a uma. As que não podem receber a ação ficam como estão, e a linha de resultado diz quais e por quê:

| Motivo mostrado | Quando |
|---|---|
| *já está concluída* | ao concluir, a tarefa já estava concluída |
| *depende de uma tarefa em aberto* | ao concluir, falta concluir uma tarefa anterior ([6.4](#64-dependências-entre-tarefas)). Se a anterior também está na seleção, ela é concluída primeiro e a dependente entra em seguida |
| *foi alterada depois da seleção* | alguém (inclusive outro computador) mudou a tarefa entre a seleção e o clique de confirmação |
| *já tem esse prazo / responsável / prioridade* | a tarefa já tinha o valor pedido |
| *está na Lixeira ou não existe mais* | a tarefa foi excluída entre a seleção e a confirmação |

As tarefas que ficaram de fora continuam selecionadas. Ao mudar o prazo, a origem do prazo anotada na tarefa é limpa, porque ela explicava a data antiga.

**O resultado e o Desfazer.** Uma linha embaixo diz o que foi feito (por exemplo, *3 tarefas concluídas.*) e, em seguida, uma linha por motivo, com os títulos das tarefas de fora. Quando concluir a última tarefa de um documento principal conclui também as dos documentos juntados, a linha avisa. **Desfazer** fica na linha por **30 segundos** e devolve as tarefas ao que eram, mexendo só no que a ação mudou; uma tarefa alterada de novo depois da ação em lote é deixada como está, e a linha de resultado do desfazer também diz isso. Tarefas excluídas ficam na **Lixeira** (Ajustes ▸ Lixeira): a linha diz *Estão na Lixeira*, e **Desfazer** as restaura. Nada é enviado ao SIGADOC.

### 6.6 Lembretes de prazos

Uma vez por dia, o Ponte mostra um aviso do Windows com o que pede atenção. O aviso, de título *Prazos de hoje no Ponte*, traz **só as quantidades**, nunca títulos nem números de documentos, porque pode aparecer sobre outras janelas e na tela de bloqueio. Ele conta, com as mesmas regras da área Hoje:

- as tarefas atrasadas e as tarefas para hoje;
- os atendimentos abertos com prazo vencido ou para hoje;
- as tarefas abertas cuja data de lembrete chegou (e que ainda não entraram nas contas acima).

- as reservas de recursos de hoje e as devoluções atrasadas.

Exemplo: *3 tarefas atrasadas, 2 para hoje e 1 atendimento com prazo.* Quando nada vence, não há aviso.

- **Quando:** uma vez por dia, a partir do horário escolhido (9h, de fábrica). Se o Ponte abre depois do horário, o aviso vem cerca de um minuto depois da abertura. Só funciona **enquanto o Ponte está aberto ou minimizado**: fechado, não avisa.
- **Com o Ponte na frente:** o aviso do Windows não é mostrado, e a mesma frase aparece no aviso do canto inferior direito da janela.
- **Clicar no aviso do Windows** traz o Ponte para a frente, em **Hoje**. Se há algo sem salvar, o Ponte pergunta antes de sair da tela, como sempre.
- **Ajustes ▸ Lembretes** (ligado de fábrica): a caixa **Lembrar dos prazos uma vez por dia** liga e desliga; **Horário** escolhe hora e minuto (de 5 em 5 minutos); **Mostrar um aviso de teste** mostra o aviso na hora, com números de exemplo, sem relação com o seu acervo.
- **Se nada aparecer:** abra *Configurações do Windows ▸ Sistema ▸ Notificações* e confira que as notificações do Ponte, e o modo Não perturbe ou Assistente de foco, permitem o aviso.
- **Privacidade:** o aviso é montado no computador, com contagens; nada é enviado a lugar nenhum. A escolha de ligar, o horário e o último dia avisado ficam num arquivo pequeno da pasta de dados (`lembretes.txt`), sem nada do acervo.

---

## 7. Atendimentos (controle do gabinete)

A área **Atendimentos** é o "livro de entrada e saída" do gabinete: cada linha é uma passagem de um documento ou de uma demanda, com entrada, responsável, situação, prazo e resultado. Substitui a planilha de controle e pode ser alimentado por ela.

### 7.1 De onde vêm os atendimentos

| Origem | Como |
|---|---|
| **Importação do SIGADOC** | Na primeira importação de cada documento, o Ponte cria o atendimento sozinho: referência = número do documento, assunto = título, entrada = hoje, responsável = você, situação *Em análise*, já ligado ao documento. Documentos juntados a outro não geram atendimento, e nova versão do mesmo documento não cria outro. A caixa **Criar atendimento ao importar do SIGADOC**, em **Ajustes ▸ Importação do SIGADOC**, desliga isso. Se o registro automático falhar (a importação em si continua valendo), a área mostra um aviso com o número do documento e o motivo, e os botões **Tentar de novo** e **Dispensar**. |
| **À mão** | Botão **Nova entrada**. |
| **Planilha** | Botão **Importar planilha** ([7.4](#74-importar-a-planilha-do-controle)). |

Se já existe um atendimento com o mesmo número e sem documento ligado (vindo da planilha, por exemplo), a importação do SIGADOC **liga** esse atendimento ao documento em vez de criar outro.

### 7.2 Ver só os de um documento

A área abre com todos os atendimentos, qualquer que seja o documento selecionado em Documentos. Para ver só os ligados a um documento, abra **Filtros** e use **Só os de ORG-OFI-2026/00042**. Enquanto a lista está restrita, uma faixa diz *Mostrando só os atendimentos de ORG-OFI-2026/00042*, com **Ver todos** para voltar a tudo. Com um documento selecionado, **Nova entrada** (ou **Ctrl+N**) cria o atendimento já ligado a ele, com o número dele como referência.

### 7.3 A ficha do atendimento

Clicar numa linha da tabela abre a ficha num painel à direita, com os botões **Salvar**, **Descartar alterações**, **Encerrar** (ou **Reabrir**), **Fechar ficha** e **Apagar de vez…**. Na tabela, *Situação* e *Prazo* aparecem como etiquetas coloridas. A tabela mostra as colunas que cabem na largura: *Referência*, *Assunto*, *Situação* e *Prazo* sempre; *Responsável*, *Entrada*, *Tempo* e *Destino / Saída*, conforme o espaço. **Abrir o documento** na ficha leva ao documento ligado, com **Voltar ao atendimento** no alto.

| Seção | Campo | Opções ou exemplo |
|---|---|---|
| Identificação | Referência | `ORG-PRO-2026/01234`, ou um código provisório como `DEMANDA-17` |
| | Entrada | `03/11/2026` |
| | Responsável | `FULANA` (botão **Atribuir a mim**) |
| | Recebido de | `Gabinete do Secretário` |
| Situação e prioridade | Situação | *A distribuir*, *Em análise*, *Em elaboração*, *Aguardando deliberação*, *Aguardando resposta*, *Suspenso (Stand By)*, *Encerrado*, *Cancelado* |
| | Prioridade e motivo | *Alta* / `prazo legal de 15 dias` |
| Prazo | Prazo | `18/11/2026` |
| | Tipo de prazo | *Prazo oficial*, *Meta interna*, *Cobrança de retorno*, *Texto / a confirmar* |
| Condições e relações | Retorno para UAS? | *Não informado*, *Sim*, *Não* |
| | Assinado? | *Não informado*, *Sim*, *Não* |
| | Agendado, Documento sensível | caixas de marcar |
| | Apensado / juntado | `apensado ao ORG-PRO-2026/01200` |
| Resultado | Entrega prevista | *Despacho*, *CI*, *Ofício*, *Informação Técnica*, *Parecer*, *Nota Técnica*, *Termo* |
| | Resultado da etapa | *Em aberto*, *Encaminhado / Tramitado*, *Arquivado no controle local*, *Respondido*, *Ciência registrada* |
| | Data de saída, Enviado para | `20/11/2026` / `Procuradoria` |
| | Assunto, Observações | texto livre |

Só a **referência ou o assunto** é obrigatório.

**Ligar a um documento.** Em **Ligar a um documento do acervo**, digite parte do número, do assunto ou do texto e escolha o documento na lista (setas e Enter também servem). A referência passa a ser o número do documento e fica travada enquanto ligado; **Desligar do documento** libera. O Resumo do documento passa a mostrar o atendimento.

**Tarefas deste atendimento.** Lista as tarefas criadas a partir dele, com situação, prazo e **Abrir**. O botão **Criar tarefa** cria, na área Tarefas, uma tarefa do documento ligado, com a prioridade, o prazo e o responsável do atendimento. Como toda tarefa pertence a um documento, o botão só fica ativo com um documento ligado. A tarefa e o registro dela na linha do tempo são gravados juntos: ou ficam os dois, ou nenhum.

**Anexos.** Depois de salvo, o atendimento aceita arquivos seus ([5.6](#56-anexos)).

**Linha do tempo.** Cada mudança de situação ou de responsável vira um evento datado. Você também registra eventos à mão, como *"cobrado retorno por telefone"*.

**Excluir.** A exclusão de um atendimento é **definitiva**, com confirmação na própria ficha. Os anexos dele são apagados; as tarefas criadas a partir dele e o documento ligado permanecem.

### 7.4 Importar a planilha do controle

Para quem já mantém o controle em Excel (arquivo `.xlsx`, aba `PROCESSOS`):

1. **Importar planilha** → **Escolher planilha…** → **Ler a planilha**.
2. A **prévia** lista cada linha com referência, assunto, responsável, entrada, situação, prazo, alertas e se **já está registrada** (mesma referência e mesma data de entrada). Nada é gravado ainda.
3. Clique em **Importar só as N novas** (ou em importar todas). A planilha original nunca é alterada.

Como as colunas viram campos:

| Coluna | Vira |
|---|---|
| B | Entrada (datas do Excel e textos como `22/10/26` são entendidos) |
| C | Responsável |
| D | Referência (define se a linha já existe) |
| I | Recebido de |
| J | Retorno para UAS? (`Sim`/`Não`; outro valor gera alerta) |
| K | Prazo. Texto como `PRAZO DIA 18` fica como *a confirmar*, com alerta |
| L | Observações |
| M | Verificação, que vira situação, prioridade e resultado. Ex.: `Tramitado` vira *Encaminhado*; com retorno da UAS fica *Aguardando resposta*; `Urgente` vira prioridade urgente |
| N | Assinado? (vazio = *não informado*, nunca "não") |
| O, P, Q, R | Saída, Entrega prevista, Enviado para, Assunto |

Linhas só com fórmulas são ignoradas. A mesma referência com datas de entrada diferentes vira **duas passagens**. Cada atendimento guarda a **linha original da planilha**, visível na ficha. Uma importação pode ser desfeita em **Importações anteriores ▸ Desfazer importação…**: o Ponte pede confirmação e preserva o que você já editou, ligou a tarefa ou anexou.

### 7.5 Filas e filtros

- **Fila:** a lista suspensa no alto da área (por exemplo *Fila: Minhas pendências (3)*) mostra quantos atendimentos há em cada fila: *Minhas pendências*, *Sem responsável*, *Prazo vencido*, *Vence hoje*, *Vence nos próximos dias* (ajuste os dias), *Aguardando resposta*, *Retorno para UAS*, *Aguardando assinatura*, *Sem atualização no período*, *Com alerta de revisão*.
- **Filtros** abre responsável, origem, destino e situação, e **Só os de <número>** ([7.2](#72-ver-só-os-de-um-documento)).
- Clique no título de uma coluna para ordenar. A coluna *Tempo* mostra os dias desde a entrada e os dias aguardando retorno externo.

---

## 8. Mais de um computador: equipe e pasta compartilhada

Em **Ajustes ▸ Equipe e sincronização** você escolhe um de quatro modos. Trocar de modo nunca apaga dados.

No alto da página, um resumo diz como este computador está trabalhando, mostra uma etiqueta de situação (por exemplo *Em dia*, *Sem conexão*, *Pasta fora de alcance*), a última sincronização e, nos modos que sincronizam, o botão **Sincronizar agora**. Os quatro modos aparecem como cartões compactos, em **Como este computador trabalha**:

| Modo (cartão) | Para quem | Como os computadores se falam |
|---|---|---|
| **Só neste computador** | uma pessoa, um computador (padrão) | não se falam |
| **Computador principal** | a máquina que centraliza a equipe | os colegas se conectam a ela pela rede interna |
| **Conectar ao computador principal** | os demais colegas | pela rede interna, com um código de conexão |
| **Pasta compartilhada** | uma pessoa em casa e no trabalho, sem rede entre os dois | por um arquivo cifrado numa pasta do Google Drive, OneDrive ou pendrive |

Os botões principais de cada modo ficam logo depois dos campos, com a etiqueta **Não salvo** enquanto houver alterações sem salvar.

Um aviso que vale para todos: **não instale o Ponte nem copie o acervo para dentro do Google Drive ou do OneDrive.** O acervo é cifrado pela sua conta do Windows e só abre na máquina que o gravou; além disso, um banco de dados dentro de pasta sincronizada se corrompe. Os modos abaixo são a forma segura.

### 8.1 Equipe na rede interna

Um computador do gabinete é o **principal**; os outros se conectam a ele. Nada vai para a internet. Cada computador guarda sua cópia completa e protegida do acervo.

**No computador principal:**

1. *Seu nome na equipe*: `BELTRANO`.
2. Escolha o cartão **Computador principal** e clique em **Salvar e ficar disponível agora**.
3. Clique em **Copiar código de conexão** e envie o texto aos colegas. É algo assim:
   ```
   Código de conexão do Ponte (gabinete):
   ponte://conectar?host=http%3A%2F%2F192.168.1.10%3A7878&chave=ponte-3f9c…
   ```
4. Marque **Ficar disponível para a equipe sempre que o Ponte abrir**, para não repetir o passo 2.

**Nos outros computadores:**

1. *Seu nome na equipe*: `FULANA`.
2. Escolha o cartão **Conectar ao computador principal**, cole o código em *Código de conexão* e clique em **Usar código**. O Ponte testa a conexão na hora. **Testar conexão** repete o teste, e o resultado aparece logo abaixo do botão.
3. Clique em **Salvar e sincronizar acervo agora**. A partir daí vale neste computador o acervo do gabinete. Se ele já tinha documentos próprios, faça antes uma cópia de segurança (**Ajustes ▸ Cópia de segurança e exportação ▸ Fazer cópia agora**): o conteúdo local dá lugar ao do computador principal.

Pronto: importações, notas, minutas, tarefas e atendimentos de todos vão para o mesmo acervo, com o nome de quem fez cada coisa.

**Se o principal desligar**, nada para. Cada computador continua salvando localmente (a mensagem avisa *Salvo neste computador… será enviado ao reconectar*) e envia tudo quando a conexão volta, na ordem em que foi feito. O que foi criado sem conexão (uma tarefa, um atendimento, um anexo) pode receber outro número no principal; o Ponte acompanha essa troca, e as alterações seguintes chegam ao item certo. Se o principal quebrar de vez, qualquer colega pode marcar o próprio computador como principal e distribuir um novo código: o acervo dele já está completo.

**Se os colegas não conseguirem conectar**, peça à TI para liberar a porta **7878 (TCP)** no Firewall do Windows do computador principal. No modo principal, a página traz um aviso com a porta em uso e, ao passar o mouse sobre ele, o comando pronto.

### 8.2 Uma pessoa, dois computadores: pasta compartilhada

Os computadores trocam o acervo por um arquivo chamado `ponte-sync.ponte`, cifrado com uma senha sua, guardado nas pastas que você indicar.

Cada computador guarda também, ao lado do próprio acervo, a última cópia que gravou (`acervo.sqlite3-guarda.ponte`, cifrada com a mesma senha). Se outro participante gravar o arquivo da pasta a partir de uma versão antiga (um celular cujo app do Drive ainda não recebeu a versão nova, dois computadores gravando ao mesmo tempo), o que este computador tinha gravado volta na sincronização seguinte. Quando a pasta só está atrasada (a nuvem ainda não entregou a última gravação), o Ponte continua esperando, como antes.

**No primeiro computador:**

1. Escolha o cartão **Pasta compartilhada**. Se uma pasta estiver fora de alcance, a página diz o que fazer (por exemplo, abrir o Google Drive e esperar a pasta aparecer).
2. **Adicionar pasta…** e escolha uma pasta do Google Drive, do OneDrive, de um pendrive ou da rede. Pode adicionar mais de uma.
3. Crie a **senha de sincronização** (mínimo de 8 caracteres) e repita. Anote-a em lugar seguro: sem ela, o arquivo não abre em lugar nenhum, nem para os autores do Ponte.
4. **Salvar e sincronizar agora**. O arquivo é criado a partir do seu acervo.

**No segundo computador:** instale o Ponte, escolha o mesmo modo, aponte para a mesma pasta e informe a mesma senha. Se ele já tiver documentos próprios, o Ponte pede confirmação antes de substituí-los pelo conteúdo da pasta.

Como funciona no dia a dia:

- **Quando sincroniza:** ao abrir o Ponte, a cada 5 minutos, em **Salvar e sincronizar agora** ou no botão **Sincronizar** da barra inferior. Ao fechar com alterações ainda não enviadas, o Ponte avisa.
- **Espere a nuvem terminar** de sincronizar a pasta antes de abrir o Ponte no outro computador.
- **Só o que mudou** entra. PDFs e anexos só são copiados quando faltam.
- **Várias pastas:** o Ponte usa a cópia mais recente e grava em todas as que alcançar. Se duas pastas divergirem, as alterações de uma e de outra são conciliadas.
- **A senha** fica guardada no acervo de cada computador, cifrada pela conta do Windows. Desmarque **Guardar a senha neste computador** para digitá-la a cada sessão.
- **Proteção do arquivo:** chave derivada da senha por PBKDF2 e cifra AES-256-GCM.

### 8.3 Conflitos de edição

Um conflito acontece só quando o **mesmo item** foi alterado nos dois lados enquanto estavam sem contato. O cartão *Edições em conflito*, em Ajustes ▸ Equipe e sincronização, mostra, campo a campo, o valor de cada lado e **quem alterou** (*só por você*, *só no computador principal*, *pelos dois lados*), e oferece:

- **Combinar as duas versões…** — cada campo fica com o lado que o alterou; textos alterados dos dois lados são juntados; nos demais campos alterados dos dois lados, a sua versão prevalece e o valor do outro lado fica anotado no próprio item. Nada se perde em silêncio.
- **Usar só a minha versão…**
- **Usar só a do computador principal…**

Todas pedem confirmação.

Um caso especial: se o computador principal recusou a criação de um item feito sem conexão (por exemplo, a tarefa de um documento que um colega mandou para a lixeira), as alterações seguintes desse item não têm onde cair. Elas aparecem como aviso (no cartão *Alterações a enviar*, da mesma seção), sem as opções acima, só com **Dispensar o aviso**: refaça a alteração à mão, se ainda for necessária.

### 8.4 O indicador na barra inferior

| Texto | Significado |
|---|---|
| `● só neste computador` | sem rede configurada |
| `● computador principal ativo` | este computador está atendendo a equipe |
| `● conectado • atualizado há 3 min` | conectado ao principal |
| `● sem conexão com o principal • 2 a enviar` | trabalhando sem o principal; duas alterações esperam |
| `● em dia há 3 min` | pasta compartilhada sincronizada |
| `● 2 a enviar` | pasta compartilhada com alterações a enviar |
| `● pasta fora de alcance` | a pasta não está acessível agora |
| `● ⚠ 2 conflito(s) de edição` | há conflitos a decidir |

Passe o mouse para a frase completa e clique para abrir Ajustes ▸ Equipe e sincronização. Durante uma sincronização, a mesma linha mostra a etapa em andamento.

---

## 9. IA opcional

O Ponte funciona inteiro sem IA. Ela serve para duas coisas:

- **Resumo ▸ Análise ▸ Analisar…** (depois, **Analisar de novo**): resume o documento, sugere prioridade e **propõe tarefas, um atendimento e reservas já preenchidos**, cada dado com o trecho do documento que o sustenta. O programa confere os trechos, palavra por palavra, e lê deles as datas, as horas e as quantidades; nada é criado antes do seu **Salvar** em cada formulário ([seção 9.4](#94-propostas-conferidas-fila-e-envio)).
- **Notas e minutas ▸ Gerar com IA, Completar e Revisar:** escrevem uma minuta do tipo escolhido, completam o texto que está no editor ou o revisam. O campo **Pedido desta minuta** recebe o que você quer, como *"solicitar a informação que falta, com tom objetivo"*; com um **Modelo de trabalho** escolhido, a IA segue também as instruções dele ([seção 9.5](#95-modelos-de-trabalho)). A sugestão abre para revisão (**Levar ao editor**, **Copiar sugestão**, **Descartar sugestão**) e só vira versão quando você salvar.

Ao lado de cada botão, uma linha diz para onde os dados vão.

### 9.1 Onde a IA roda

Em **Ajustes ▸ Inteligência artificial**, escolha uma das quatro opções, apresentadas como cartões. Cada cartão diz se os textos saem do computador: *Nada sai deste computador*, *Envia os textos à Ollama*, *Envia os textos ao servidor informado* ou *Envia os textos à OpenAI*.

| Opção | Os dados saem do computador? | Custo | O que informar |
|---|---|---|---|
| **Ollama neste computador** (padrão) | **Não** | nenhum | instale o [Ollama](https://ollama.com) e baixe um modelo: `ollama pull qwen3:4b-instruct` |
| **Ollama Cloud** | Sim, para os servidores da Ollama | conta em ollama.com, gratuita por uso ou plano mensal | a chave de API criada em [ollama.com/settings/keys](https://ollama.com/settings/keys) |
| **Outro servidor** | Sim, para esse servidor | o do servidor | endereço e chave opcional. Serve para um servidor do órgão compatível com a API do Ollama |
| **ChatGPT (assinatura)** | Sim, para a OpenAI, nos EUA | usa a cota do seu plano Plus ou Pro | clique em **Entrar com o ChatGPT…** e autorize no navegador |

Em qualquer opção, clique em **Listar modelos do servidor** e escolha o modelo na lista. Depois, **Salvar** (ou saia da seção: ela é salva ao sair). Na mesma página, o card **Testar a IA com casos fictícios** confere como a IA escolhida se sai ([seção 9.4](#94-propostas-conferidas-fila-e-envio)).

**Sigilo e LGPD.** Nas três opções em que os dados saem do computador, são enviados a cada clique em Analisar ou Gerar, e nunca ao importar: o texto do documento, com número, título, datas e campos descritivos (tipo, origem, remetente…), os nomes dos arquivos e referências, as notas e o contexto salvos e as fontes selecionadas. **Não saem** os endereços (da página do SIGADOC e dos links), os números de controle internos do Ponte, o cadastro de recursos nem os nomes das pessoas da equipe; o casamento de uma reserva proposta com o seu cadastro acontece neste computador. Antes do primeiro envio de cada documento a um provedor externo, o Ponte mostra o que sai, para onde e com qual modelo, e pede a sua confirmação, uma vez por documento e destino; com **Ollama neste computador**, nada sai e não há pergunta. Use as opções em nuvem só com documentos que podem sair do órgão.

A configuração fica no acervo, cifrada pela sua conta do Windows. Desmarque **Guardar a chave neste computador** para manter a chave, ou a sessão do ChatGPT, só até fechar o Ponte.

### 9.2 Ollama neste computador, na prática

1. Instale o Ollama (<https://ollama.com/download>) e deixe-o aberto.
2. No Prompt de Comando, baixe um modelo: `ollama pull qwen3:4b-instruct` (cerca de 2,5 GB).
3. No Ponte, **Ajustes ▸ Inteligência artificial ▸ Ollama neste computador ▸ Listar modelos do servidor**, escolha o modelo e salve.

Se o Ollama deste computador não responder, a página mostra esses mesmos passos (instalar, abrir, baixar um modelo) e o botão **Tentar de novo**.

**Documentos longos.** O Ollama atende, por padrão, com uma janela de 4.096 tokens (de 12 a 16 mil caracteres, somando documento, notas, fontes e a resposta). Quando o pedido passa disso, o Ponte pede sozinho uma janela maior, de 8, 16 ou 32 mil tokens, a menor que serve, e a barra inferior avisa *"Documento longo: pedindo ao modelo uma janela maior"*. Janelas maiores usam mais memória, e a primeira análise longa leva alguns segundos a mais porque o Ollama recarrega o modelo. Se nem 32 mil tokens bastarem, o Ponte explica e sugere analisar sem as fontes extras ou usar outro provedor.

### 9.3 ChatGPT (assinatura), em detalhe

É o programa oficial "Sign in with ChatGPT" da OpenAI para aplicativos de código aberto que rodam no computador da pessoa.

- **Login:** o botão abre o navegador em `auth.openai.com` e volta ao Ponte por uma porta local deste computador. O Ponte não vê a sua senha e não recebe acesso às suas conversas, à memória nem à cobrança do ChatGPT.
- **Plano:** exige ChatGPT Plus ou Pro. Uma conta gratuita entra, mas não libera uso.
- **Cota:** as chamadas são descontadas da cota do plano, a mesma do Codex e de outros aplicativos, com uma janela de 5 horas e uma semanal. Em **ChatGPT ▸ Configurações ▸ Uso** você vê o saldo, ajusta o limite do Ponte, usa uma redefinição ou compra créditos. Quando a cota acaba, o Ponte avisa e **não** usa outro provedor por conta própria.
- **Sair:** apaga a sessão deste computador e pede à OpenAI que a encerre. Confira em ChatGPT ▸ Configurações ▸ Aplicativos conectados.

### 9.4 Propostas conferidas, fila e envio

**A análise em duas etapas.** A IA lê o documento primeiro (*Etapa 1 de 2: lendo o documento…*): lista o que ele diz (pedidos, prazos, datas e horas, quantidades, coisas pedidas, remetente, assunto), cada fato com o trecho literal. O programa confere cada trecho contra o texto do documento, o contexto e as fontes, e lê do trecho a data, a hora ou a quantidade. Depois (*Etapa 2 de 2: preparando as propostas…*), a IA propõe tarefas, um atendimento e reservas usando só os fatos conferidos, e o programa confere cada proposta contra esses fatos. O que não passa na conferência é devolvido ao modelo, item por item e com o motivo, para uma correção; o que continua errado é descartado ou fica em branco, sem derrubar o resto da análise. Há um limite de chamadas por análise. Por serem duas etapas ou mais, a análise leva mais tempo que a de uma chamada só.

**O que aparece em Resumo ▸ Análise ▸ Propostas (revise antes de usar).** Cada proposta vem preenchida, com os trechos que a sustentam (**Trechos do documento**) e o selo **conferido no texto** nos dados conferidos:

- **Tarefa:** título claro, providência, prioridade e, quando o documento escreve uma data, **Prazo** e **Origem do prazo** (o trecho). **Criar tarefa** abre o formulário preenchido, com o responsável em branco. Prazo relativo (*até o fim da próxima semana*) não vira data: fica como observação.
- **Atendimento:** com atendimento já existente (o Ponte cria um ao importar do SIGADOC), **Completar o atendimento**, mostrando o que mudaria (*Prazo: 12/11/2026 → 22/11/2026*); sem ele (registro manual, e-mail, WhatsApp), **Nova entrada** preenchida.
- **Reserva:** o que foi pedido, quantidade, início, fim e finalidade. O programa casa o pedido com o seu cadastro de recursos e confere o horário: **Livre neste horário**, **Ocupado** (por quem e até quando), **Mais de um recurso combina**, **Nenhum recurso cadastrado combina** (com **Cadastrar recurso…**), **Quantidade acima do cadastro** ou **Sem horário certo**. **Reservar** abre o uso preenchido e ligado ao documento; sem hora de término, o formulário propõe 1 hora e avisa *confira o fim*.
- Uma proposta já usada mostra **Tarefa criada**, **Atendimento atualizado** ou **Reserva criada**, com **Abrir**; o vínculo é por identificador da proposta, não pela providência. Se o documento, o contexto ou as notas mudaram depois da análise, os botões ficam desligados até **Analisar de novo**. Análises feitas antes da 0.34.0 continuam com as **Providências sugeridas**.
- **Material utilizado**, no rodapé do card, mostra provedor, modelo, chamadas, etapas (tentativas e itens descartados com o motivo, sem o texto do documento) e os fatos lidos, com o que o programa conferiu.

**O que a conferência garante, e o que não garante.** *Conferido no texto* quer dizer que o **programa** achou o trecho, palavra por palavra, no documento e leu dele a data, a hora ou a quantidade. Não quer dizer que a proposta está certa nem que aquela data é o prazo que vale: a revisão é da pessoa. Nada é criado nem alterado antes do **Salvar** de cada formulário (que abre com *não salvo* e pergunta ao sair), a IA nunca escolhe o responsável e não há agentes autônomos: a análise tem duas etapas verificadas, e nada age sozinho. O texto do documento é tratado como dado, nunca como instrução.

**Envio ao provedor externo.** Veja o que sai e o que não sai em [9.1](#91-onde-a-ia-roda). A pergunta *Enviar este documento a …?* aparece uma vez por documento e destino, com **Enviar** e **Cancelar**. Na segunda etapa vão só os fatos já conferidos.

**Fila, progresso e erros passageiros.** Enquanto a IA trabalha, a barra inferior mostra a etapa (*Etapa 1 de 2…*, *Conferindo os trechos…*, *Conferindo as propostas…*) e o botão **Cancelar**, que funciona também durante a espera da resposta. Pedir uma análise ou minuta com outro trabalho em andamento põe o pedido na fila (*Na fila: …*; cabe um; **Tirar da fila** desiste), e ele não roda se o documento, o contexto ou as notas mudarem enquanto espera. Falha passageira do servidor é repetida até duas vezes, com espera (*Nova tentativa em N s (2 de 3)*); resposta cortada por tamanho pede uma versão mais curta; o limite de espera de cada chamada (120 s no Ollama, 180 s no ChatGPT) é dito como tal: *A IA demorou mais que o limite de espera*. A minuta continua com uma chamada e a conferência de antes (citação existente, exemplo sem cópia, texto preservado).

**Testar a IA com casos fictícios.** Em **Ajustes ▸ Inteligência artificial**, **Testar a IA** envia à IA configurada os documentos fictícios que acompanham o programa (`src/ai_casos.json`: ofício com prazo, pedido de sala por WhatsApp, e-mail com dois pedidos, documento só para ciência, instrução embutida no documento…) e confere o que ela leu e propôs. Não envia nada do acervo, não salva nada nele e, com provedor externo, pergunta antes (uma vez por sessão e destino). O resultado (conferências que passaram por caso, chamadas, tempo e motivos das falhas) pode ser salvo ou copiado e só traz nomes de casos e contagens. Mede aquele modelo com aquelas instruções, varia entre modelos e não garante o acerto nos seus documentos.

### 9.5 Modelos de trabalho

Um modelo de trabalho reúne o que se repete na redação de um tipo de resposta, para que a IA e as pessoas da equipe partam do mesmo ponto. A área é **Modelos**, na barra lateral. Modelos são opcionais: sem eles, a minuta funciona como antes. A lista tem uma caixa de busca (**Buscar modelo**, ou Ctrl+F). **Duplicar…**, **Arquivar** (ou **Desarquivar**) e **Apagar de vez…** ficam no menu **⋯** (**Mais ações**), à direita do título do modelo.

**O que um modelo guarda**

- **Estrutura:** seções, com orientação e marca de obrigatória, e variáveis a preencher.
- **Instruções:** blocos de texto reutilizáveis, cada um *obrigatório*, *opcional* ou *condicional* (com a condição escrita). Um bloco pode servir a vários modelos.
- **Exemplos:** demanda e resposta revisada, com explicação e comentários. Cada exemplo tem situação: *Não conferido*, *Aprovado* ou *Rejeitado*. Só os aprovados são usados.
- **Referências:** nome, onde encontrar (número, endereço, pasta) e observação.
- **Critérios de conferência:** o que a minuta deve cumprir antes de seguir.

**Versões**

O modelo nasce como rascunho e pode ser editado. **Publicar versão N** congela o texto. Para mudar um modelo publicado, revise: o Ponte cria a versão seguinte, em rascunho. As minutas já geradas continuam ligadas à versão que usaram e não mudam.

**Escopo**

- **Pessoal:** aparece só para quem o criou.
- **Compartilhado com a equipe:** aparece para todos, na próxima sincronização.

Pessoal é apenas um filtro de exibição. Não protege o conteúdo: quem tem acesso ao acervo da equipe tem acesso ao dado. Qualquer integrante pode revisar um modelo compartilhado, e o histórico guarda quem fez cada versão.

**As cinco abas**

| Aba | Para quê |
|---|---|
| **Estrutura** | Nome, finalidade, tipo, critérios de uso, escopo, seções, variáveis, critérios de conferência e nota da versão. Botões **Salvar rascunho** e **Publicar versão N**. |
| **Instruções** | Biblioteca de blocos; anexar ao modelo como obrigatória, opcional ou condicional; editar e publicar versões do bloco. |
| **Exemplos** | Cadastrar, **Aprovar** ou **Rejeitar** exemplos; anexar arquivos (PDF com texto, .txt, .docx), **Extrair texto** e **Conferir texto…**. |
| **Referências** | Fontes que o modelo manda conferir. |
| **Testar** | Usa o documento selecionado em Documentos (a aba diz qual está sendo usado) e mostra o que seria enviado à IA: instruções incluídas, tamanhos e conflitos. **Nada é enviado.** |

**Arquivos de exemplo**

Um exemplo pode ter arquivos anexados: um ofício real já respondido, uma minuta aprovada. O Ponte extrai o texto de PDF com camada de texto, de .txt e de .docx (**Extrair texto**); um PDF digitalizado fica como "precisa de OCR" e não é enviado. O texto extraído pode ser lido e corrigido por uma pessoa (**Conferir texto…**), e aí passa a valer o texto conferido. Só vai à IA o texto de arquivo de exemplo **aprovado**, com extração atual (se o arquivo ou o extrator mudar, o texto fica "desatualizado" até ser extraído de novo). O texto dos arquivos é dado, como o dos exemplos: a IA não pode copiá-lo nem tratar seus fatos como do caso. Anexos de exemplos não vão para a Lixeira; a exclusão é definitiva.

**Como usar na minuta**

Em **Notas e minutas**, no passo **1. Partir de**, escolha o **Modelo de trabalho** (só os publicados do tipo aparecem). Enquanto nenhum está escolhido, o Ponte pode sugerir um ("Modelo sugerido pelas palavras do documento", com o percentual de semelhança): é uma comparação lexical entre as palavras do documento e o nome, a finalidade, os critérios de uso e os exemplos aprovados de cada modelo publicado; a escolha continua sua. Dos exemplos aprovados, vão à IA os três mais parecidos com o documento (pelo título e pela demanda); sem semelhança, os três mais recentes. Marque as **Instruções opcionais** que valem para este caso, escreva, no passo **2. Pedir à IA (opcional)**, o **Pedido desta minuta** e clique em **Gerar com IA**, **Completar** (preenche o que falta e preserva o que já está escrito) ou **Revisar** (altera só o necessário e abre a comparação antes de substituir). **Começar por um modelo** preenche o editor com as seções, sem IA. **Guardar como modelo** leva a minuta revisada para a área Modelos, como rascunho de um modelo novo. Em **Modelos**, **Usar este modelo na minuta** faz o caminho inverso.

**Como as instruções chegam à IA**

- As instruções do modelo e o **Pedido desta minuta** vão na parte confiável do pedido, que a IA trata como ordem.
- O documento, as notas, as fontes e os exemplos vão como dados. Dados nunca valem como ordem, mesmo que tragam frases como "ignore as instruções".
- Os exemplos mostram só organização, tom e estrutura. Os fatos, nomes, números e datas deles não valem para o caso. Se a minuta copiar um trecho de exemplo, o programa a recusa e pede nova resposta.
- Instruções que se contradizem, como *"sempre cite a norma"* e *"nunca cite a norma"*, geram um aviso antes de gerar. A detecção é uma heurística por palavras e serve só de alerta: o Ponte não resolve o conflito sozinho, e uma contradição pode passar sem aviso.

**Registro de geração e Material utilizado**

Cada geração é registrada, aceita ou recusada. Na lista de versões, **Material utilizado** mostra: modelo e versão; instruções incluídas, com o texto da época; pedido; exemplos; fontes e arquivos; modelo de IA e provedor; e os conflitos avisados. A conferência dos trechos citados mostra que a citação existe na fonte ("trecho encontrado na fonte"). Ela não comprova que a conclusão esteja certa: a revisão é sua.

**Modelos pessoais de versões anteriores**

Na primeira abertura depois da atualização, cada modelo pessoal antigo vira um modelo pessoal já publicado, com uma versão para cada texto antigo e uma seção **Texto**. Nada se perde. Para compartilhar, mude o escopo e publique uma nova versão.

---

## 10. Dados, segurança, backup e lixeira

### 10.1 Onde ficam os dados

| O quê | Onde |
|---|---|
| Acervo | `%LOCALAPPDATA%\PonteProj\acervo.sqlite3` |
| Backups | `%LOCALAPPDATA%\PonteProj\backups\` |
| Exportações | `%LOCALAPPDATA%\PonteProj\exportados\` |
| Cópias temporárias de anexos abertos | `%LOCALAPPDATA%\PonteProj\temp\` (apagadas ao abrir e ao fechar o Ponte) |
| Programa | `%LOCALAPPDATA%\Programs\PonteProj\` |

Para abrir essas pastas, cole o caminho na barra de endereço do Explorador de Arquivos.

### 10.2 Como os dados são protegidos

Todo conteúdo é gravado cifrado com a **DPAPI** da sua conta do Windows: texto, PDFs, anexos, texto reconhecido, notas, minutas, tarefas, atendimentos e as configurações de rede e de IA. Na prática:

- outra conta do Windows no mesmo computador não consegue ler;
- o arquivo copiado para outro computador não abre;
- um backup só abre na mesma conta do Windows que o criou.

Por isso, **trocar de computador ou reinstalar o Windows exige planejamento**: use a pasta compartilhada ou a equipe ([seção 8](#8-mais-de-um-computador-equipe-e-pasta-compartilhada)) para levar o acervo, não a cópia do arquivo.

### 10.3 Backup

**Ajustes ▸ Cópia de segurança e exportação ▸ Fazer cópia agora** cria uma cópia consistente na pasta de backups. A página mostra onde as cópias ficam, com o botão **Abrir pasta** (a pasta só existe depois da primeira cópia), e a data da última cópia feita nesta sessão; **Copiar o caminho da última cópia** copia o caminho dela. O instalador também faz uma cópia antes de cada atualização.

### 10.4 Lixeira

Excluir documento, tarefa, anexo ou versão importada pede confirmação e manda para a **Lixeira** (Ajustes ▸ Lixeira), de onde se restaura com um clique. Logo após a exclusão, o aviso do canto inferior direito traz **Desfazer**, por alguns segundos.

- **Por padrão, nada é apagado de vez.**
- Na seção Lixeira, **Apagar de vez os itens excluídos há mais de:** aceita *Nunca*, *7 dias*, *30 dias*, *90 dias*, *180 dias* ou *1 ano*. Com um prazo, o Ponte faz a limpeza ao abrir e uma vez por dia, e avisa quantos itens foram apagados. **Apagar agora os vencidos** antecipa.
- Um documento apagado de vez leva junto versões, PDFs, notas, minutas, tarefas e anexos. Backups feitos antes não são afetados.
- Em equipe, a limpeza é feita pelo computador principal, pela configuração dele.
- Atendimentos e seus anexos não passam pela Lixeira: a exclusão deles é definitiva, com confirmação.

### 10.5 Exportar dados (CSV)

**Ajustes ▸ Cópia de segurança e exportação ▸ Exportar dados para CSV…** grava cinco arquivos numa pasta nova, `exportados\csv-AAAAMMDD-HHMMSS\`:

| Arquivo | Conteúdo |
|---|---|
| `documentos.csv` | número, título, tipo, origem, datas, processo, remetente, finalidade, assuntos, prioridade, juntada, contagem de tarefas e de anexos |
| `tarefas.csv` | tarefa, ação, situação, prioridade, responsável, prazo, atraso, conclusão |
| `atendimentos.csv` | as passagens da área Atendimentos |
| `recursos.csv` | ID, nome, quantidade, local, identificação, situação, criado em |
| `usos-de-recursos.csv` | ID, recurso, quantidade, quem, setor, finalidade, início, fim previsto e devolução (data e hora, em colunas separadas), duração em horas, mês, situação, atrasado (*Sim* ou *Não*), documento ligado (número), atendimento ligado (referência), tarefa ligada (*Sim* ou *Não*) |

Formato: UTF-8, separador `;`, datas `AAAA-MM-DD` e uma coluna "Mês" para séries mensais. **Não saem** textos de documentos, notas, minutas, observações nem anexos. Em `usos-de-recursos.csv`, os horários são os do relógio do computador, sem fuso. Os arquivos abrem no Excel e servem de entrada para painéis: marque a coluna `ID` como identificador para reimportar sem duplicar.

### 10.6 O que sai do computador

Por padrão, nada. O que pode sair, e só por escolha sua:

| O quê | Quando | Para onde |
|---|---|---|
| Texto do documento (com número, título, datas e campos descritivos), notas, contexto e fontes; nunca endereços, números internos, cadastro de recursos nem nomes da equipe | A cada clique em **Analisar** ou **Gerar com IA**, nas opções de IA em nuvem, depois da sua confirmação, uma vez por documento ([9.1](#91-onde-a-ia-roda), [9.4](#94-propostas-conferidas-fila-e-envio)) | Ollama Cloud, o servidor informado ou a OpenAI |
| Documentos fictícios de teste | Ao clicar em **Testar a IA**, com provedor externo, depois da confirmação | Ollama Cloud, o servidor informado ou a OpenAI; nada do acervo |
| O acervo, cifrado com a sua senha | Na pasta compartilhada, ao sincronizar ([8.2](#82-uma-pessoa-dois-computadores-pasta-compartilhada)) | A pasta que você indicou |
| A página da ficha ([5.7](#57-imprimir-ou-salvar-a-ficha)) | Ao escolher **Imprimir ou salvar a ficha…** | O arquivo `.html` na pasta que você escolheu, **fora do acervo protegido** |
| A nota escrita no painel da extensão ([5.4](#54-importar-de-novo-o-mesmo-documento)) | Ao clicar em **Importar para o assistente** | Só ao Ponte deste computador, pela ligação entre o navegador e o programa. Depois é uma nota comum do documento: segue para a equipe ou a pasta compartilhada se você usa uma delas |
| As mensagens e os arquivos escolhidos no WhatsApp Web ([5.8](#58-conversas-do-whatsapp)) | Ao clicar em **Enviar ao Ponte**, no painel da extensão (recurso opcional, desligado de fábrica) | Só ao Ponte deste computador, pela ligação entre o navegador e o programa; a extensão não envia nada à internet. Depois é um registro comum: segue para a equipe ou a pasta compartilhada se você usa uma delas |
| Os avisos de prazos do Windows ([6.6](#66-lembretes-de-prazos)) | Uma vez por dia | Para ninguém: são só contagens, mostradas neste computador. Nenhum título ou número de documento entra no aviso |
| A lista de importações recentes ([5.4](#54-importar-de-novo-o-mesmo-documento)) | Durante a sessão do navegador | Para ninguém: fica na memória do navegador e some ao fechá-lo |
| Uma consulta à página de versões no GitHub | Só quando você clica em **Ajustes ▸ Sobre o Ponte ▸ Verificar se há versão nova** | `github.com/demusis/ponteproj`, sem nenhum dado do acervo |

**Verificar se há versão nova.** O Ponte não procura atualização sozinho: a consulta só acontece nesse clique. Ele pergunta ao GitHub qual é a versão mais recente publicada, sem enviar nada do acervo, compara com a instalada e mostra uma destas respostas: *Você já tem a versão mais recente*, *Há uma versão nova* (com a data de publicação, quando houver) ou um aviso de falha (sem internet, o GitHub não respondeu ou a resposta não pôde ser entendida), com **Tentar de novo**. Se há versão nova, **Abrir a página de download** abre a página da versão no navegador. O Ponte não baixa nem instala nada: **feche o Ponte antes de instalar**; o instalador faz uma cópia de segurança do acervo antes de atualizar ([seção 1](#atualizar-e-desinstalar)).

---

## 11. Solução de problemas

A maioria das mensagens já diz o que fazer. Estas são as mais comuns.

### 11.1 Instalação e importação

| Sintoma | O que fazer |
|---|---|
| O botão **Importar para o assistente** não aparece no SIGADOC | A extensão não está instalada, ou a página não é a ficha de um documento. Clique no ícone da extensão: ele diz se está ligado ao Ponte e o que fazer. |
| Ícone da extensão diz *✗ Não foi possível ligar ao Ponte* | Siga os passos que ele mostra: instalar a versão mais recente do Ponte, abri-lo uma vez e reiniciar o navegador. |
| *"O Ponte não foi encontrado neste computador"* | Abra o Ponte uma vez e reinicie o navegador. Se continuar, rode o instalador de novo **com a mesma conta do Windows** que usa o navegador. |
| *"Não recebemos a confirmação do Ponte"* | Clique em **Repetir importação**; nada é duplicado. **Se a mensagem se repete em toda tentativa e a extensão veio da Chrome Web Store, o Ponte instalado é anterior à 0.19.3**: essas versões não reconheciam a extensão da loja. Instale o Ponte mais recente e reinicie o navegador. |
| *"O Ponte abriu e fechou sem responder"* ou *"Esta extensão não é reconhecida por este Ponte"* | O mesmo caso, com a mensagem das versões novas. Instale o Ponte mais recente e reinicie o navegador. |
| *"O navegador não conseguiu iniciar o Ponte"* | Reinstale o Ponte com a sua conta do Windows. Se continuar, o antivírus ou uma regra do órgão pode estar bloqueando o programa: procure a TI. |
| *"O Ponte não conseguiu abrir o acervo deste computador"* | Abra o Ponte pelo menu Iniciar: a tela mostra o motivo. Resolvido, clique em **Repetir importação**. |
| *"Importação ignorada: o SIGADOC mostra o mesmo conteúdo já importado em …"* (laranja) | Não é erro: o documento já está no Ponte exatamente assim. Se ele mudou, atualize a página (F5) e importe de novo. |
| *"A página ainda não terminou de carregar"* | Espere a página abrir por completo e clique de novo. |
| Tela inicial diz *✗ O navegador ainda não foi ligado ao Ponte* | O registro não existe para esta conta do Windows. Rode o instalador. |
| O link **Adicionar nota ou prioridade** não aparece no painel | O Ponte instalado é anterior à 0.31 ou a extensão é anterior à 0.3.0. Atualize o Ponte (e a extensão, se veio da pasta) e reinicie o navegador. |
| *"A importação foi feita, mas a nota não foi salva…"* | O documento foi importado; só a nota falhou. Escreva-a na ficha, em **Notas e minutas**. A prioridade, se também falhou, se escolhe na ficha. |
| O botão **Ponte** não aparece no WhatsApp Web | O recurso não foi ligado (ícone da extensão ▸ **Usar no WhatsApp Web**) ou a página não foi recarregada depois de ligar. Recarregue a página (F5). Exige a extensão 0.4.0. |
| *"O Ponte não reconheceu esta página do WhatsApp Web"* | O WhatsApp pode ter mudado a página. Use **Colar mensagens copiadas**, no painel, ou **Novo ▸ Registro manual… ▸ Colar conversa do WhatsApp**, no Ponte. |
| *"Atualize o Ponte para guardar conversas do WhatsApp"* | O Ponte instalado é anterior à 0.32. Instale a versão mais recente e reinicie o navegador. |
| *"Esta conversa já foi guardada antes e está na Lixeira…"* | Restaure-a em Ajustes ▸ Lixeira. |
| *"O recurso … já está reservado (ou em uso) por … até …"* | Não é erro: o horário escolhido está ocupado. Escolha outro horário ou outro recurso, ou espere a devolução. Em **Ocupação do dia**, na ficha da reserva, aparecem os horários já tomados. |
| O Ponte diz que o arquivo de sincronização ou o acervo é de uma versão mais nova | Outro computador ou o celular já está na versão nova. Instale a versão mais recente do Ponte neste computador (e a do Ponte de bolso). Nada se perdeu. |
| Dois botões "Importar" na página | Há duas extensões, a da pasta e a da loja. Remova a da pasta em `chrome://extensions`. |

### 11.2 IA

| Mensagem | O que fazer |
|---|---|
| *"Não foi possível conectar à IA"* | O Ollama não está aberto, ou o endereço em Ajustes ▸ Inteligência artificial está errado. |
| *"O modelo “X” não está instalado neste servidor"* | Em Ajustes ▸ Inteligência artificial, **Listar modelos do servidor** e escolha um, ou instale com `ollama pull X`. |
| *"O nome de modelo “X” não é aceito pelo Ollama"* | Escolha o modelo pela lista, em vez de digitar. |
| *"O documento, com as notas e as fontes, não coube na janela do modelo"* | Analise sem as fontes extras, use um modelo com janela maior ou escolha outro provedor. |
| *"O computador não tem memória livre para este modelo"* | Feche outros programas, use um modelo menor ou outro provedor. |
| *"A IA excedeu o tempo limite de 120 segundos"* / *"A IA demorou mais que o limite de espera (N segundos)"* | O modelo é lento para este computador ou o servidor está sobrecarregado. A análise faz mais de uma chamada. Tente de novo, use um modelo menor ou verifique o servidor de IA. |
| *"Na fila: …"* depois de clicar em **Analisar** | Não é erro: outro trabalho de IA está em andamento, e o seu começa quando ele terminar. **Tirar da fila**, na barra inferior, desiste. |
| *"Este documento não está mais no acervo"* | O documento foi apagado de vez ou saiu numa sincronização. A lista foi atualizada. Se disser que está na Lixeira, **Abrir a Lixeira** e restaure-o. |
| *"O computador principal está numa versão anterior do Ponte, que ainda não guarda as propostas da IA"* | Atualize o Ponte no computador principal e tente de novo. |
| *"A cota do seu plano ChatGPT para o Ponte acabou por enquanto"* | Veja o saldo em ChatGPT ▸ Configurações ▸ Uso. Use uma redefinição, espere a renovação ou troque de provedor. |
| *"Entre com a sua conta do ChatGPT em Ajustes ▸ Inteligência artificial"* | A sessão expirou ou foi encerrada. Entre de novo. |

### 11.3 Equipe e pasta compartilhada

| Sintoma | O que fazer |
|---|---|
| Colega não consegue conectar ao principal | O principal precisa estar com o Ponte aberto e *disponível*; os dois na mesma rede; porta 7878 liberada no firewall do principal; código de conexão atual (gerar nova chave invalida os códigos antigos). |
| *"Conflito de edição simultânea"* ao salvar | Outra pessoa salvou o mesmo item antes de você. Copie o que escreveu, descarte, abra o item de novo e aplique sua alteração. |
| *"Senha de sincronização incorreta"* | A senha não é a usada para criar o arquivo da pasta. Sem ela o arquivo não abre. |
| *"A pasta ainda não recebeu a versão mais recente"* | Espere o Google Drive ou o OneDrive terminar de sincronizar e tente de novo. |
| `● pasta fora de alcance` | A pasta não está acessível (nuvem parada, pendrive ausente). O Ponte segue com o acervo local e envia depois. |

### 11.4 Dados

| Mensagem | O que fazer |
|---|---|
| *"Não foi possível proteger/abrir os dados deste perfil Windows"* | O acervo pertence a outra conta do Windows. Entre com a conta que fez a instalação. |
| *"Item na lixeira; restaure antes de editar"* | O item foi excluído. Restaure-o em Ajustes ▸ Lixeira. |
| O aviso diário de prazos não aparece | Confira em **Ajustes ▸ Lembretes** se está ligado e se o horário já passou; o Ponte precisa estar aberto ou minimizado, e só avisa quando algo vence. Use **Mostrar um aviso de teste** e, se nem ele aparecer, veja *Configurações do Windows ▸ Sistema ▸ Notificações*. |
| Em Tarefas, uma ação em lote deixou tarefas de fora | Não é erro: a linha de resultado diz quais e por quê ([6.5](#65-ações-em-lote)). **Desfazer** vale por 30 segundos; tarefas excluídas estão em Ajustes ▸ Lixeira. |

---

## 12. Perguntas frequentes

**O Ponte altera alguma coisa no SIGADOC?**
Não. Ele só lê a página que você abriu. Assinar, tramitar e despachar continuam no SIGADOC.

**Preciso de internet?**
Só para acessar o SIGADOC e, se quiser, para a IA em nuvem, a pasta compartilhada ou **Verificar se há versão nova** (só quando você clica; veja [10.6](#106-o-que-sai-do-computador)). O aplicativo em si funciona sem internet.

**Posso usar em dois computadores?**
Sim, pelo modo pasta compartilhada ([8.2](#82-uma-pessoa-dois-computadores-pasta-compartilhada)). Copiar o arquivo do acervo de um para o outro não funciona.

**Posso instalar o Ponte dentro do Google Drive para usar em vários computadores?**
Não. O acervo é cifrado pela conta do Windows de cada máquina, e um banco de dados em pasta sincronizada se corrompe. Use a pasta compartilhada.

**O que acontece se eu importar o mesmo documento duas vezes?**
Se nada mudou, nada é gravado e a extensão avisa. Se mudou, o Ponte guarda uma nova versão e mantém as anteriores.

**A IA lê meus documentos sem eu pedir?**
Não. Nada é enviado à IA ao importar. O envio acontece só quando você clica em **Analisar** (ou **Analisar de novo**) ou em **Gerar com IA**, **Completar** ou **Revisar**, e, com um provedor externo, depois de você confirmar o que sai.

**A IA cria tarefas, atendimentos e reservas sozinha?**
Não. Ela propõe, o programa confere os trechos, e cada formulário abre preenchido e **não salvo**: nada é criado antes do seu **Salvar**. A IA nunca escolhe o responsável.

**Qual a diferença entre a extensão da loja e a da pasta?**
São a mesma extensão. A da loja se atualiza sozinha e não exige o modo do desenvolvedor; a da pasta vem junto com o instalador, para quem não tem acesso à loja. Use só uma das duas.

**Excluí um documento sem querer. E agora?**
Ajustes ▸ Lixeira ▸ **Restaurar**. Por padrão a Lixeira nunca é esvaziada.

**Esqueci a senha da pasta compartilhada.**
O arquivo da pasta não pode ser aberto sem ela. O acervo de cada computador continua intacto: escolha uma pasta nova, crie outra senha e sincronize de novo a partir do computador mais atualizado.

**Onde vejo a versão instalada?**
Em **Ajustes ▸ Sobre o Ponte**, ou no ícone da extensão. Lá também está **Verificar se há versão nova**.

---

## 13. Para desenvolvedores

### 13.1 Arquitetura em uma olhada

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

| Pasta ou arquivo | Papel |
|---|---|
| `src/app.rs` | Janela principal: lista, ficha em 4 abas, menu, barra inferior |
| `src/labels.rs` | Rótulos e datas exibidos; campo de data com calendário |
| `src/help_ui.rs` | O ⓘ de ajuda: explica ao passar o mouse e, ao clicar, deixa a explicação aberta ao lado |
| `src/storage.rs`, `src/protection.rs` | SQLite + DPAPI; `Store` é a única porta de entrada aos dados |
| `src/model.rs` | `Capture` (o que a extensão envia) e sua validação |
| `src/native.rs`, `src/main.rs` | Host nativo: quadros com tamanho, ações `ping`, `import_capture` e `import_bundle`, IDs de extensão aceitos. Os `extras` (nota e prioridade) são conferidos antes de importar e aplicados depois (`apply_extras`: a nota entra ao fim, com a data, e a falha em gravar nunca desfaz a importação); `ping` informa as ações `import_extras` e `import_record`. `import_record` recebe uma conversa do WhatsApp Web com até 10 arquivos: confere tudo (título, texto, mensagens contadas pelo próprio texto, tamanhos) antes de gravar e responde `imported` ou `already` |
| `src/files.rs`, `src/pdf_reader.rs` | PDFs importados; leitor e OCR do Windows |
| `src/dossier.rs` | Vínculos entre documentos, versões importadas, detecção de juntada |
| `src/manual.rs` | Registros manuais |
| `src/whatsapp.rs` | Conversas do WhatsApp: leitor do texto copiado (à mão, sem retrocesso), texto do registro (continuação recuada em 4 espaços), metadados `wa_*`, identificador pelo conteúdo (`request_id`) e `Store::import_whatsapp` (colar). Os vetores de teste, `src/whatsapp_vetores.json`, são os mesmos de `tests/extension/whatsapp-vetores.json`, lidos também pelo leitor em JavaScript |
| `src/record_import.rs` | Registro com anexos, tudo ou nada: valida antes de gravar, mesma conversa (mesmo identificador) responde "já existe", anexo que falha leva o registro à Lixeira; usado por `import_record` |
| `src/attachments.rs` | Anexos de documentos, tarefas e atendimentos, inclusive os arquivos arrastados do Explorador |
| `src/tasks.rs`, `src/tasks_ui.rs` | Tarefas, dependências, painel. Em `tasks_ui.rs`, o `mod batch` faz as ações em lote (seleção, pergunta, tarefas puladas e motivo, desfazer por 30 s) pelas mesmas chamadas do `Store` de uma alteração avulsa |
| `src/reminders.rs` | Lembrete diário de prazos: contagens, decisão por data (não por tempo decorrido) e arquivo `lembretes.txt`; o aviso do Windows fica em `src/windows_ui.rs` |
| `src/saiop.rs` | Atendimentos: modelo, importador `.xlsx` (leitor ZIP/XML próprio), painel |
| `src/resources.rs`, `src/resources_ui.rs` | Recursos: dados selados (`resources`, `resource_uses`), regra de choque de horário dentro da transação de gravação, sincronização e conflito próprio para a reserva dupla; a área com as abas Agora e próximos, Calendário e Recursos |
| `src/trash.rs` | Lixeira, restauração e limpeza por prazo |
| `src/intranet.rs` | Modos de rede, sincronização em thread própria, fila offline, mesclagem em três vias; o computador principal atende cada conexão numa thread, com limite por computador |
| `src/idmap.rs` | Tradução dos números de itens criados sem conexão para os que receberam no acervo da equipe |
| `src/sharedfolder.rs` | Arquivo de transporte da pasta compartilhada (PBKDF2 + AES-256-GCM) |
| `src/ai.rs`, `src/chatgpt.rs` | Cliente Ollama, login e Responses API da OpenAI; execução de uma etapa (formato da resposta, repetição de falhas passageiras, correção por item, orçamento de chamadas, cancelamento); o que é enviado (`capture_for_ai`) |
| `src/ai_roles.rs` | As duas etapas da análise (Extrator: fatos com trecho literal; Propositor: tarefas, atendimento e reservas que citam fatos) e o programa que as encadeia, sempre conferido contra os fatos |
| `src/evidence.rs` | Fontes para a IA, conferência literal de trechos e, por fato, de data, hora e quantidade dentro do trecho |
| `src/ai_eval.rs`, `src/ai_casos.json` | Régua: documentos fictícios com o resultado esperado, rodada nos testes com respostas simuladas e, por **Testar a IA**, ao vivo |
| `src/app/proposals.rs`, `src/app/queue.rs` | Cartão **Propostas** e janela **Material utilizado** da análise; confirmação de envio, fila de trabalhos de IA e avisos de documento que saiu do acervo |
| `src/work_context.rs`, `src/draft_tools.rs` | Contexto de trabalho; texto-base e comparação de minutas |
| `src/work_templates.rs` | Modelos de trabalho: dados e versões, instruções, exemplos, registro de geração, migração dos modelos pessoais antigos |
| `src/prompting.rs` | Montagem do prompt em camadas, detecção de conflitos e conferências (cópia de exemplo, texto preservado ao completar) |
| `src/templates_ui.rs` | Área **Modelos**: lista, abas Estrutura, Instruções, Exemplos, Referências e Testar |
| `src/template_bundle.rs` | Importação de modelos e instruções de um arquivo JSON (`--import-templates`): confere o arquivo inteiro antes de gravar, não duplica, traz como rascunho ou publicado |
| `src/extraction.rs` | Texto dos arquivos de exemplo (PDF, .txt, .docx), com versão do extrator, situação e conferência por pessoa |
| `src/suggestion.rs` | Sugestão lexical (TF-IDF) de modelo para o documento aberto e ordem dos exemplos enviados |
| `src/mail.rs` | Importação de e-mails `.eml`: decodificador MIME próprio (cabeçalhos, base64, quoted-printable, multipart, HTML → texto), registro manual idempotente pelo Message-ID, anexos |
| `src/imap.rs` | Cliente IMAP mínimo sobre TLS (`rustls`, raízes Mozilla): LIST, EXAMINE, UID SEARCH, UID FETCH com literais, só leitura; contas seladas só neste computador; nomes de pasta em UTF-7 modificado |
| `src/mail_ui.rs` | Contas, pasta e filtro, lista das mensagens e importação das marcadas: dentro de Ajustes ▸ E-mail e na janela "Importar do e-mail" (atalho de Documentos ▸ Novo) |
| `src/export.rs` | Exportação CSV (documentos, tarefas, atendimentos, recursos e usos de recursos) |
| `src/app/search.rs` | Busca em todo o acervo (Ctrl+K): documentos, tarefas e atendimentos, com resultados agrupados e teclado |
| `src/ficha_export.rs` | A ficha como página `.html` única, para imprimir ou salvar em PDF (**Imprimir ou salvar a ficha…**) |
| `src/update_check.rs` | **Verificar se há versão nova**: consulta, só a pedido, a última versão publicada no GitHub; não envia dados do acervo, não baixa nem instala |
| `src/guide.rs` | Guia do usuário: `GUIA_DO_USUARIO.md` embutido e convertido em página para o navegador |
| `src/windows_ui.rs`, `src/paths.rs` | Seletores de arquivo, verificação do registro do host, pastas de dados |
| `src/icon.rs`, `build.rs`, `assets/` | Ícone da janela e do executável |
| `src/lib.rs`, `src/main.rs` | A biblioteca com todos os módulos; `main.rs` decide se abre a janela, atende o navegador ou executa uma opção de linha de comando |
| `extension/` | Extensão MV3: `capture-core.js` (leitura da página e validação), `content.js` (painel), `history.js` (tramitação), `files.js` e `background.js` (PDFs e host nativo), `popup.*` (inclui a lista de importações recentes, guardada na memória de sessão do navegador, e o botão **Usar no WhatsApp Web**). Recurso opcional do WhatsApp Web: `whatsapp-core.js` (leitor do texto copiado, regras do registro e dos anexos, leitor da página; funções puras) e `whatsapp.js` (o painel, registrado pelo `background.js` só depois que a pessoa liga o recurso e a permissão é concedida; só lê dentro dos cliques) |
| `installer/Ponte.iss` | Instalador Inno Setup |
| `scripts/` | `build.ps1`, `build-installer.ps1`, `build-webstore-package.ps1`, `install-app.ps1`, `install-native-host.ps1` |
| `tests/` | Fluxo nativo (`native_flow.rs`), extensão (`node --test`, Playwright) e fixtures fictícias |

### 13.2 Decisões que vale conhecer

- **Identidade de um documento** é o número oficial validado por expressão regular, nunca um ID de página.
- **Tudo o que vai para o SQLite passa por `protection::seal`.** Colunas indexadas guardam resumos criptográficos, nunca o valor: os testes de cifragem procuram números e textos em claro no arquivo do acervo.
- **Capturas são imutáveis.** Conteúdo novo cria versão; conteúdo idêntico é ignorado com aviso, sem gravar nada.
- **Dois IDs de extensão são aceitos:** o da pasta instalada (`extension-id.txt`) e o da Chrome Web Store (`extension-store-id.txt`). O instalador autoriza os dois no navegador e o executável confere os dois.
- **O host nunca fecha calado.** Origem desconhecida ou acervo que não abre são respondidos com `{ok:false,error:{code,message}}`, que a extensão mostra.
- **Mutações de equipe** (`IntranetMutation`) são a única forma de alterar o acervo em rede; a fila offline e a pasta compartilhada reaplicam as mesmas mutações.
- **Modelos de trabalho** usam as tabelas `work_templates`, `work_template_versions`, `instruction_blocks`, `instruction_block_versions`, `template_examples` e `draft_generations`. Os itens são endereçados por `uid` (UUID), não pelo número da linha, o que dispensa tradução de números na sincronização; só o registro de geração, que aponta para documento e captura, passa por `idmap`. A camada confiável do prompt só recebe texto de modelo, instruções e pedido; documentos e exemplos vão como dados.
- **`SCHEMA_VERSION` 12 (11 na 0.20.0):** o acervo muda de formato. Quem está em equipe atualiza todos os computadores juntos, o principal primeiro.
- **Formato 16 (arquivo de sincronização 4), na 0.34.0:** a análise passa a guardar `facts` e `proposals`, que uma versão antiga apagaria em silêncio ao regravar. Todos os computadores e o celular (Ponte de bolso 0.7.2) atualizam juntos; um leitor antigo recusa o acervo e o arquivo novos, e um principal antigo recusa a análise com propostas com uma mensagem clara.
- **Limites:** 500 KiB de texto e 1 MiB por pacote de captura, até 10 PDFs, 8 MiB por PDF e 16 MiB de PDFs por importação, 8 MiB por anexo, 100 MiB por planilha `.xlsx` (256 MiB descompactada).
- **A extensão só atua em `https://www.sigadoc.mt.gov.br`** e remove segredos de URL. A única exceção é o recurso opcional do WhatsApp Web (`https://web.whatsapp.com`), que fica desligado até a pessoa ligá-lo e só então pede a permissão ao navegador.
- **Leituras caras ficam em memória até o acervo mudar.** Cada linha é decifrada pela DPAPI, e a interface redesenha várias vezes por segundo. `Store::cached` guarda o resultado de listas como tarefas, documentos e atendimentos e o descarta a cada gravação, desta conexão ou de outra. Uma leitura nova que seja cara e sem efeitos colaterais deve passar por ele.

**Convenções para quem for alterar o código:**

- Toda alteração do acervo (documentos, versões, tarefas, atendimentos, anexos, notas) passa por `Store`, numa transação `Immediate`, e registra-se com `intranet::record_mutation`. Preferências locais deste computador, como o prazo da Lixeira, ficam de fora e não são sincronizadas. Num computador conectado ou em pasta compartilhada, a função começa por `client_config()` e envia a alteração com `remote_mutate`, como uma `IntranetMutation`.
- Toda tabela nova guarda o conteúdo com `protection::seal` e, se pertence a um documento, entra nas proteções de `ensure_write_guards` (`src/storage.rs`).
- Textos para a pessoa ficam em português e dizem o que fazer; comentários e nomes no código ficam em inglês.
- Configurações com segredos não derivam `Debug`; escreva um que oculte o segredo, como em `IntranetConfig`.

### 13.3 Compilar e testar

Toolchain: Rust estável para `x86_64-pc-windows-gnu` (pode ser o conjunto portátil com MinGW) e, para a extensão, Node.js 18+ com Playwright 1.62.

```powershell
# check + testes + release, sempre com --locked
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Action All -ToolsDirectory "$env:LOCALAPPDATA\Programs\PonteProj\devtools"
Copy-Item .\target\release\ponteproj.exe .\dist\ponteproj.exe -Force

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

# testes da extensão (os de DOM usam o Chrome instalado)
cd extension; npm install; npm test; cd ..
# ou, com um Playwright já instalado em outra pasta:
$env:PONTEPROJ_PLAYWRIGHT_DIR = 'caminho\para\node_modules\playwright'
node --test .\tests\extension\core.test.cjs .\tests\extension\dom.test.cjs .\tests\extension\files.test.cjs
```

Onde ficam os testes: os unitários num `#[cfg(test)] mod tests` no fim de cada arquivo de `src/`; o fluxo de ponta a ponta do host nativo em `tests/native_flow.rs`; os da extensão em `tests/extension/`. Fixtures em `tests/fixtures/` e `tests/extension/*.html`, sempre com dados fictícios. Sem Playwright, os testes de página da extensão se pulam sozinhos; `PONTEPROJ_BROWSER_CHANNEL` escolhe o navegador (padrão `chrome`).

Hoje são 264 testes unitários (mais dois que exigem um Ollama ou uma conta do Gmail de verdade e ficam de fora do `Test`) e 4 de integração no aplicativo, e 123 na extensão.

Se o projeto estiver numa pasta sincronizada (Google Drive, OneDrive), use `$env:CARGO_BUILD_JOBS='2'` ou aponte `CARGO_TARGET_DIR` para fora dela: a compilação paralela pode derrubar a montagem do drive.

### 13.4 Registrar o host para um binário de desenvolvimento

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-native-host.ps1 -Executable .\target\release\ponteproj.exe
```

Para conferir que o executável atende uma extensão, chame-o como o navegador faria, com a origem como argumento e um quadro `ping` na entrada padrão. A resposta deve trazer `"ok":true` e a versão.

### 13.5 Linha de comando

```powershell
ponteproj.exe                                  # abre a janela
ponteproj.exe --open-document 12               # abre a ficha do documento local 12
ponteproj.exe --import-json .\tests\fixtures\capture.json   # importa uma captura em JSON
ponteproj.exe --list "treinamento"             # lista documentos (JSON) que casam com o termo
ponteproj.exe --import-templates modelos.json  # traz instruções, modelos (rascunho; --publicar publica) e exemplos (sempre "não conferido")
ponteproj.exe --serve 0.0.0.0:7878             # computador principal sem janela (imprime a chave)
ponteproj.exe --guide                          # abre o guia do usuário no navegador
ponteproj.exe --guide .\guia.html              # grava o guia como página, sem abrir
ponteproj.exe --native-host                    # atende a extensão pela entrada e saída padrão, como o navegador faz
ponteproj.exe --version | --help
```

O binário `release` não abre console (subsistema gráfico); para ver a saída de `--list`, `--version` e `--help` num terminal comum, use `target\debug\ponteproj.exe` ou redirecione a saída.

Variáveis de ambiente: `PONTEPROJ_DATA_DIR` (outra pasta de dados, sempre em disco local), `PONTEPROJ_AI_TOKEN` (chave da IA sem gravá-la) e `PONTEPROJ_NO_UI` (o host não abre a janela após importar; útil em testes).

### 13.6 Publicar uma versão

1. Atualize `version` em `Cargo.toml` **e** em `Cargo.lock` (entrada `ponteproj`). Se a extensão mudou, atualize `"version"` em `extension/manifest.json` e `extension/package.json`, e a constante `EXTENSION_VERSION` em `src/app.rs`. Nunca altere `manifest.key`, `extension-id.txt` ou `extension-key.txt`.
2. `build.ps1 -Action All` e copie `target\release\ponteproj.exe` para `dist\ponteproj.exe` (o instalador empacota o que estiver em `dist\`; confira com `dist\ponteproj.exe --version`). Escreva as notas em `ATUALIZACAO_X_Y_Z.md` (arquivo local, fora do repositório: vira o texto da Release).
3. `scripts\build-installer.ps1` gera `dist\Ponte-Setup-X.Y.Z.exe` e a cópia de nome fixo `dist\Ponte-Setup.exe` (Inno Setup 6: `winget install JRSoftware.InnoSetup`).
4. `scripts\build-webstore-package.ps1` gera `dist\ponte-extensao-webstore-A.B.C.zip`; envie no painel da Chrome Web Store (ficha, privacidade e roteiro em [`extension/WEBSTORE.md`](extension/WEBSTORE.md)).
5. `gh release create vX.Y.Z dist\Ponte-Setup-X.Y.Z.exe dist\Ponte-Setup.exe dist\ponte-extensao-webstore-A.B.C.zip --notes-file ATUALIZACAO_X_Y_Z.md`.

O `Ponte-Setup.exe` de nome fixo sustenta o link direto do topo deste README (`releases/latest/download/Ponte-Setup.exe`): sem ele na Release mais recente, o link quebra.

A extensão publicada na loja tem ID próprio (`hhppmkbmainkdfmcfeaanknlnfgihnah`), diferente do da versão carregada da pasta (`ecllnakmppjinmednoikimbpnbgbimhg`). A chave privada `extension.pem` **não está no repositório** e deve ficar em local seguro.

### 13.7 O que está no repositório

Só o necessário para compilar, instalar e publicar o aplicativo e a extensão: `src/` e `Cargo.*` (aplicativo), `extension/` (extensão), `assets/` e `build.rs` (ícone), `installer/` e `scripts/` (instalador, host nativo, pacote da loja), `tests/` (fluxo nativo e extensão, com fixtures fictícias), `extension-id.txt`, `extension-key.txt` e `extension-store-id.txt` (identidades públicas da extensão), além deste README, do [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md), da [`LICENSE`](LICENSE) e de [`extension/WEBSTORE.md`](extension/WEBSTORE.md) e [`extension/PRIVACIDADE.md`](extension/PRIVACIDADE.md).

Os instaladores e as notas de cada versão estão nas [Releases](https://github.com/demusis/ponteproj/releases). Documentos de planejamento e a chave privada da extensão ficam fora do repositório.

---

## 14. Autoria e licença

O Ponte foi concebido e desenvolvido por **Carlo Ralph De Musis** e **João Gabriel Bezerra Estrela**. No programa, **Ajustes ▸ Sobre o Ponte** mostra estes créditos, a versão e o endereço do projeto.

O código é distribuído sob a licença **MIT** (arquivo [`LICENSE`](LICENSE)): pode ser usado, copiado, modificado e redistribuído livremente, inclusive para fins comerciais, desde que o aviso de autoria e a licença sejam mantidos. O software é fornecido sem garantias.

O Ponte não é um produto do SIGADOC nem da sua mantenedora; ele apenas lê, no navegador da pessoa, as páginas às quais ela já tem acesso. Assinaturas, tramitação e a versão oficial dos documentos continuam no SIGADOC.
