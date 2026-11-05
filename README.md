# Ponte — Assistente documental SIGADOC

O **Ponte** é um programa do Windows que guarda, no seu computador, os documentos que você importa do SIGADOC — com texto, PDFs e histórico de tramitação — para que você anote, prepare minutas, acompanhe prazos, controle as demandas do gabinete e, se quiser, trabalhe em equipe na rede interna. Ele é formado por um aplicativo (Rust + egui + SQLite) e por uma extensão para Chrome/Edge que coloca o botão **Importar para o assistente** nas páginas do SIGADOC.

O Ponte **nunca envia nada ao SIGADOC**: assinaturas e tramitação continuam lá. Nada vai para a nuvem; os dados ficam protegidos na sua conta do Windows.

| | |
|---|---|
| **Instalador** | **[Baixar o Ponte para Windows](https://github.com/demusis/ponteproj/releases/latest/download/Ponte-Setup.exe)** (sempre a versão mais recente) — versões anteriores e notas em [Releases](https://github.com/demusis/ponteproj/releases) |
| **Guia do usuário** | [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md) |
| **Extensão na Chrome Web Store** | publicada (não listada): <https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah> — exige o Ponte 0.19.3 ou posterior; a extensão também vem junto com o instalador |
| **Versão** | 0.19.3 (extensão 0.1.11) — [notas de versão nas Releases](https://github.com/demusis/ponteproj/releases) |
| **Autoria** | Carlo Ralph De Musis e João Gabriel Bezerra Estrela — ver [Autoria e licença](#11-autoria-e-licença) |
| **Licença** | [MIT](LICENSE) |

---

## Sumário

1. [Instalar em 5 minutos](#1-instalar-em-5-minutos)
2. [Um passeio pelo Ponte com um documento de exemplo](#2-um-passeio-pelo-ponte-com-um-documento-de-exemplo)
3. [Documentos: ficha, notas e minutas](#3-documentos-ficha-notas-e-minutas)
4. [Tarefas e prazos](#4-tarefas-e-prazos)
5. [Atendimentos (controle do gabinete)](#5-atendimentos-controle-do-gabinete)
6. [Trabalhar em equipe (Rede e equipe)](#6-trabalhar-em-equipe-rede-e-equipe)
7. [IA opcional](#7-ia-opcional)
8. [Onde ficam os dados, backup e lixeira](#8-onde-ficam-os-dados-backup-e-lixeira)
9. [Solução de problemas](#9-solução-de-problemas)
10. [Para desenvolvedores](#10-para-desenvolvedores)
11. [Autoria e licença](#11-autoria-e-licença)

---

## 1. Instalar em 5 minutos

Não precisa de administrador: tudo é instalado na sua conta do Windows.

1. Baixe o instalador por este link direto, que sempre aponta para a versão mais recente: **<https://github.com/demusis/ponteproj/releases/latest/download/Ponte-Setup.exe>**. Dê dois cliques no arquivo e avance até o fim; pode deixar marcado **Abrir o Ponte agora**. Uma versão específica (`Ponte-Setup-X.Y.Z.exe`) fica em [Releases](https://github.com/demusis/ponteproj/releases).
2. Feche e abra o navegador (Chrome ou Edge).
3. Na tela inicial do Ponte, o quadro **Ligação com o navegador** deve dizer *✓ O Ponte está registrado para o Chrome e Edge*.
4. Instale a extensão **uma vez**, de um destes jeitos:
   - **Pela loja** (recomendado): abra <https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah> e clique em **Adicionar ao Chrome** (no Edge, permita "extensões de outras lojas"). Exige o Ponte 0.19.3 ou posterior.
   - **Pela pasta instalada**: na tela inicial do Ponte, clique em **Copiar caminho**; no navegador abra `chrome://extensions` (ou `edge://extensions`), ative **Modo do desenvolvedor**, clique em **Carregar sem compactação** e cole o caminho na barra de endereço da janela que abre.
5. Feche e abra o navegador de novo. Clique no ícone da extensão: deve mostrar *✓ Ligado ao Ponte X.Y.Z* (a versão instalada; ela também aparece em **Menu ▾ ▸ Sobre o Ponte**).

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

**Criar a tarefa com prazo.** No Resumo, em *Tarefas deste documento*, clique em **Nova tarefa**: título *Responder ao ofício*, responsável **Eu**, prazo `13/11/2026` (digite ou escolha no calendário 📅; há também o botão **Hoje**), salve. A partir daí o cartão do documento na lista da esquerda mostra `● 1 em aberto (no prazo) • Prazo: 13/11/2026`; quando o prazo passar, vira vermelho `⚠ 1 tarefa atrasada`.

**Responder.** Ainda em *Anotações e minutas*, na seção **Minuta**: tipo *Resposta*, clique em **Começar por um modelo** (texto-base sem IA) ou em **Gerar sugestão com IA**; edite; **Salvar nova versão**; **Exportar (RTF)** abre a pasta com o arquivo para o Word. O texto final você cola no SIGADOC — o Ponte não tramita nem assina.

**Concluir.** Com a resposta enviada, clique em **✓ Concluir** ao lado da tarefa. O cartão passa a `✓ 1 tarefa concluída`.

Tudo isso fica só no seu computador (ou no acervo do gabinete, se a equipe estiver conectada — seção 6).

---

## 3. Documentos: ficha, notas e minutas

### A lista da esquerda

- **Buscar** procura em número, assunto, texto, notas, minutas e nos PDFs lidos.
- **Filtrar ▾** abre os filtros por situação das tarefas (*Com tarefa aberta*, *No prazo*, *Atrasados*, *Concluídos*), por prioridade e por origem (SIGADOC ou manuais), e a **Ordem** da lista: mais recentes, prazo mais próximo, prioridade, número ou assunto. Quando há atrasados, um atalho `⚠ N atrasado(s)` aparece ao lado.
- Cada cartão tem uma barra colorida e uma etiqueta: vermelho = tarefa atrasada; âmbar = vence hoje; azul-petróleo = em aberto no prazo; verde = tudo concluído; cinza = sem tarefas.
- **Documentos juntados** (quando a ficha do SIGADOC diz *Juntado ao documento: …*) aparecem logo abaixo do documento principal, com a marca ↳ e a etiqueta `↳ Juntado a SESP-DIC-2026/97546`. O principal mostra `📎 N juntado(s)`. O vínculo entre os dois é criado automaticamente quando ambos estão importados, em qualquer ordem.

### As quatro abas da ficha

| Aba | O que tem |
|---|---|
| **Resumo** | Dados principais; tarefas do documento; atendimentos do controle do gabinete ligados a ele; análise por IA; contexto de trabalho e fontes extras (recolhidos); documentos parecidos. |
| **Documento** | Texto importado (aberto); Arquivos (PDF) com **Ler no Ponte** e **Exportar**; **Anexos** (arquivos do seu computador juntados ao documento); Dados do SIGADOC; Histórico de tramitação. |
| **Anotações e minutas** | Notas no alto; editor de minuta embaixo com tipo, modelo, IA, **Salvar nova versão**, **Copiar**, **Exportar (RTF)**, **Comparar com versão** e as versões salvas. |
| **Mais** | Vínculos entre documentos (manuais ou automáticos) e versões importadas; o que foi importado e referências; dados técnicos. |

O cabeçalho da ficha mostra o número, a etiqueta de tarefas, a situação de juntada (com **Abrir documento principal** em 1 clique) e o botão **Excluir…** (vai para a lixeira, reversível).

### Anexos (arquivos seus, no documento, na tarefa ou no atendimento)

Em **Documento ▸ Anexos**, na ficha de cada tarefa e na ficha de cada atendimento (depois de salvo), **Anexar arquivo…** abre a janela de escolha do Windows. O arquivo passa a fazer parte do acervo (cifrado, como tudo o mais) e aparece para toda a equipe. Exemplos: o ofício em Word que originou o documento, a foto de um despacho, o e-mail com a orientação recebida.

| Regra | Valor |
|---|---|
| Tamanho | até 8 MB por arquivo |
| Quantidade | até 20 por documento ou tarefa |
| Tipos | PDF, Word, Excel, PowerPoint, OpenDocument, texto (.txt, .rtf, .csv), imagens (.png, .jpg, .gif, .bmp, .tif) e e-mails (.eml, .msg) |
| Recusados | programas, scripts, atalhos, arquivos compactados (.zip, .rar) e Office com macros (.docm, .xlsm) |

Quando um arquivo é recusado, a mensagem diz o motivo e o que fazer (por exemplo: *"O arquivo tem 12,4 MB; o limite é 8 MB. Para reduzir: salve o PDF com compressão… ou divida o documento em partes"*). O tipo é conferido também pelo conteúdo, não só pela extensão.

Cada anexo tem uma **descrição** opcional e os botões **Ler no Ponte** (PDF) ou **Abrir** (demais tipos, no programa padrão do Windows, a partir de uma cópia temporária apagada ao fechar o Ponte), **Salvar como…** e **Excluir…** (vai para a Lixeira). Na lista de tarefas, **📎 2** indica quantos anexos a tarefa tem.

### Registros manuais (o que não está no SIGADOC)

Uma ligação telefônica, um memorando interno, um e-mail ou um ofício em papel podem virar um **registro manual**: botão **＋ Registro manual** na lista da esquerda (ou **Criar registro manual…** na tela inicial), com tipo, título, origem, data do documento e texto. O registro recebe um número próprio, `MANUAL-2026/0007`, que nunca se confunde com um número do SIGADOC, e aparece com a pílula âmbar **Manual** na lista, na ficha e nas tarefas. Tarefas, notas, minutas, anexos, análise por IA, Lixeira e compartilhamento com a equipe funcionam como em qualquer documento; a ficha mostra "Dados do registro" no lugar dos dados do SIGADOC e não tem "Abrir no SIGADOC". **Editar registro…** salva uma nova versão (as anteriores ficam em Mais ▸ Versões). Em equipe, o número é dado pelo computador principal; se ele estiver fora de alcance, o registro fica como "pendente" até a próxima sincronização. O filtro **Origem** separa SIGADOC e manuais; na exportação CSV, a coluna "Origem" vale "Manual".

### Reimportar

Importar de novo o mesmo documento nunca duplica nada. O Ponte compara o conteúdo (texto da página, PDFs e dados) com o que já está no acervo:

| A página do SIGADOC está… | O que acontece |
|---|---|
| **igual** à versão em uso no Ponte | **Nada é gravado.** O painel da extensão mostra, em laranja, *"Importação ignorada: o SIGADOC mostra o mesmo conteúdo já importado em 04/11/2026 12:38. Nada foi gravado. Se o documento mudou, atualize a página (F5) e importe de novo."* O Ponte não abre e o documento não muda de lugar na lista. |
| **diferente** (despacho novo, anexo novo, texto alterado) | Guarda uma **nova versão**; as anteriores ficam em **Mais ▸ Versões importadas**, com comparação do que mudou, e análises e minutas antigas são marcadas como feitas sobre uma importação anterior. |
| igual a uma **versão anterior** (voltou atrás) | Nada novo é gravado; essa versão volta a ser a atual e o painel avisa em laranja que a página corresponde a uma versão anterior. |

Se a confirmação de uma importação se perder no caminho (o botão vira **Repetir importação**), repetir não duplica: o Ponte responde *Importação confirmada*.

---

## 4. Tarefas e prazos

Cada documento importado recebe uma tarefa de triagem. Você cria outras no Resumo (**Nova tarefa**) ou no painel **Tarefas** (topo da janela).

O painel tem a **lista à esquerda** e a **ficha à direita**. Visões: *Hoje*, *Esta semana*, *Atrasadas*, *Todas* e, em **Mais ▾**, *Prontas para fazer* (pendentes sem dependência aberta), *A triar*, *Aguardando terceiros*, *Lembretes* e *Sem prazo*.

Campos da tarefa:

| Campo | Exemplo |
|---|---|
| Título | `Responder ao ofício` |
| Responsável | `MARIA` (ou botão **Eu**) |
| Ação / Situação / Prioridade | *Responder* / *Pendente* / *Alta* |
| Prazo | `13/11/2026` — digite dd/mm/aaaa, escolha no calendário 📅 (mês com os dias da semana) ou clique em **Hoje** |
| Observações | texto livre |
| **Anexos** | arquivos do seu computador ligados à tarefa (depois de salvá-la); ver [Anexos](#anexos-arquivos-seus-no-documento-ou-na-tarefa) |
| **Mais detalhes** | origem do prazo (ex.: *art. 5º do ofício*), lembrete, resultado, checklist de etapas, dependências |

Regras úteis:
- **Nada se perde ao trocar de tela.** Com uma tarefa não salva, clicar em **Ficha**, **Atendimentos**, num documento da lista, em outra tarefa ou em **Nova tarefa** abre a pergunta *"A tarefa ‘…’ tem alterações não salvas"* com **Salvar e continuar**, **Descartar alterações** e **Voltar à tarefa**. Enquanto isso, o rótulo **Tarefas ●** fica laranja no topo. Na ficha, **Consultar documento** abre o texto e os PDFs numa janela por cima, só leitura, sem sair da tarefa; **Ir para o documento** abre a ficha completa.
- **✓ Concluir** e **Reabrir** ficam na própria lista; concluir uma tarefa não registra nada no SIGADOC.
- **Juntados seguem o principal:** concluir a última tarefa aberta de um documento principal conclui, em cascata, as tarefas abertas dos documentos juntados a ele (e dos juntados a estes), cada uma com a anotação *"Concluída automaticamente: o documento principal X foi concluído"*. A mensagem de confirmação lista o que foi concluído; tarefas com dependência pendente ficam abertas e são informadas. Reabrir o principal não reabre as peças.
- **Dependências aparecem na lista.** Uma tarefa que depende de outras fica recuada à direita (um degrau por nível, como os documentos juntados), com a linha *↳ depende de: A, B* (concluída riscada ✓, atrasada em vermelho), e é listada depois delas. Enquanto houver dependência aberta, o cartão mostra ⛓ e **✓ Concluir** fica desabilitado com *"Conclua antes: A, B"*. Na ficha, o bloco **Cadeia de dependências** lista o que vem antes e o que espera por ela, em qualquer nível, com **Abrir**. Ao marcar dependências, as que formariam ciclo aparecem desabilitadas e explicadas; o salvamento continua recusando ciclos.
- Datas inválidas (ex.: `31/02/2026`) ficam marcadas em vermelho e o salvamento avisa.

---

## 5. Atendimentos (controle do gabinete)

Área para o controle operacional que muitos gabinetes mantêm numa planilha. No SIGADOC, processos e expedientes são todos **documentos**, e é assim que entram na biblioteca do Ponte. Um **atendimento** é outra coisa: cada passagem de um desses documentos pelo gabinete (ou de uma demanda sem documento: ligação, e-mail, pedido verbal), com entrada, responsável, situação, prioridade, prazo e resultado. O mesmo documento pode ter várias passagens. O atendimento pode ser ligado ao documento importado (**Ligar a documento**), mas existe mesmo sem ele.

### Registrar uma entrada à mão

Clique em **Registrar nova entrada**. A ficha abre **ao lado** da grade, com **Salvar / Descartar / Encerrar / Fechar / Excluir…** sempre visíveis no alto (a exclusão é definitiva, com confirmação; não há Lixeira para atendimentos). Exemplo:

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

**Ao importar do SIGADOC**, a primeira importação de cada documento registra sozinha a entrada dele aqui: referência, assunto, data, responsável (você) e ligação ao documento, com situação "Em análise"; documentos juntados a outro não entram, e um atendimento que já tinha aquele número (da planilha, por exemplo) é ligado em vez de duplicado. A caixa **Criar atendimento ao importar do SIGADOC**, no alto do painel, desliga isso. O atendimento criado pode ser editado ou excluído como qualquer outro.

**Com um documento aberto na biblioteca**, o painel mostra só os atendimentos dele ("Só SEFAZ-PRO-2026/00001"; **Todos os atendimentos** amplia) e **Registrar nova entrada** cria o atendimento já ligado ao documento aberto (mesmo com a lista em "Todos os atendimentos"), com o número dele como referência.

**Ligar a documento**: se o documento já foi importado do SIGADOC, informe o número e clique em **Ligar a documento**. A referência passa a ser o número do documento (fica travada enquanto ligado; **Desligar do documento** libera), o Resumo do documento passa a mostrar o atendimento, e **Criar tarefa** na ficha cria uma tarefa no painel Tarefas ligada a ele. A ficha também aceita **Anexos** (arquivos seus), como documentos e tarefas.

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

Pronto: importações, notas, minutas, tarefas e atendimentos de todos passam a ir para o mesmo acervo, com o nome de quem fez cada coisa. O ponto colorido na barra inferior mostra o estado, em poucas palavras: `● conectado • atualizado agora`, `● sem conexão com o principal • 2 a enviar`, `● computador principal ativo`, `● em dia há 3 min` (pasta compartilhada) ou `● ⚠ N conflito(s) de edição`. Passe o mouse para ver a frase completa e clique para abrir a janela; durante uma sincronização a mesma linha mostra a etapa em andamento.

**Se o principal desligar**, nada para: cada computador continua salvando localmente (a mensagem avisa *Salvo neste computador… será enviado ao reconectar*) e tudo é enviado quando a conexão volta. Se o principal quebrar de vez, qualquer colega pode marcar o próprio computador como principal e distribuir um novo código: o acervo dele já está completo.

**Conflito de edição** acontece só quando o *mesmo item* foi alterado nos dois lados enquanto estavam desconectados. A janela mostra, campo a campo, o valor no principal, o seu e **quem alterou** (*só por você*, *só no computador principal*, *pelos dois lados*), e oferece:
- **Combinar as duas versões…** — cada campo fica com o lado que o alterou; textos alterados dos dois lados são juntados; nos demais campos alterados dos dois lados, a sua versão prevalece e o valor do principal fica anotado no próprio item (nada se perde em silêncio);
- **Usar só a minha versão…**; **Usar só a do computador principal…**.

Todas pedem confirmação. Se os colegas não conseguirem conectar, peça à TI para liberar a porta **7878 (TCP)** no Firewall do Windows do principal (a janela traz o comando).

---

### Uma pessoa, dois computadores: pasta compartilhada

Para usar o Ponte em casa e no trabalho sem rede entre eles, escolha em **Rede e equipe** o modo **Pasta compartilhada: eu, em mais de um computador**. Os computadores trocam o acervo por um arquivo `ponte-sync.ponte`, cifrado com uma senha sua (PBKDF2 + AES-256-GCM), guardado nas pastas que você indicar com **Adicionar pasta…**: Google Drive, OneDrive, pendrive, pasta da rede, uma ou várias. Não instale o Ponte nem copie o acervo para o Drive: o acervo é cifrado pela sua conta do Windows e um SQLite em pasta sincronizada se corrompe; o arquivo de transporte é a forma segura.

- **Persistente:** a lista de pastas e a senha ficam guardadas no acervo de cada computador, cifradas pela conta do Windows (desmarque **Guardar a senha neste computador** para informá-la a cada sessão). No segundo computador, aponte para as mesmas pastas e informe a mesma senha; se ele já tiver documentos, o Ponte pede confirmação antes de substituí-los.
- **Quando sincroniza:** ao abrir, a cada 5 minutos, em **Sincronizar agora** (ou Menu ▾ ▸ Sincronizar com a pasta agora) e com aviso ao fechar se houver alterações não enviadas. A barra de baixo mostra a etapa (lendo as pastas, conciliando, gravando em N pastas, atualizando o acervo) e, ao fim, o resumo do que entrou e saiu.
- **Só o que mudou** entra; PDFs e anexos só são copiados quando faltam. Conflitos (o mesmo item alterado nos dois lados) vão para a tela de conflitos, com as duas versões campo a campo, e você decide; o arquivo recebe tudo o que não conflita e a decisão vai na sincronização seguinte.
- **Várias pastas:** o Ponte usa a cópia mais recente e grava em todas as que alcançar; se duas pastas divergirem, as alterações que uma tem e a outra não são conciliadas pelo diário que cada cópia carrega.
- **Trocar de modo** nunca apaga dados: dá para passar de pasta compartilhada para principal/conectado (mesma rede) e vice-versa; com alterações não enviadas, sincronize antes.

---

## 7. IA opcional

O Ponte funciona inteiro sem IA. Quando você quiser:

- **Resumo ▸ Analisar e sugerir providências**: resume o documento, sugere prioridade, prazo mencionado e providências (cada uma vira tarefa com um clique). Trechos citados são conferidos literalmente contra as fontes.
- **Anotações e minutas ▸ Gerar sugestão com IA**: escreve uma minuta do tipo escolhido, com uma orientação opcional (*"solicitar a informação que falta, com tom objetivo"*). A sugestão abre para revisão e só vira versão quando você salvar.
- Contexto de trabalho e notas não salvos são salvos antes; a linha ao lado do botão diz para onde os dados vão.

**Menu ▾ → Configurar IA** oferece quatro lugares para a IA rodar:

| Opção | Quando usar | O que informar |
|---|---|---|
| **Ollama neste computador** | padrão; nada sai da máquina | endereço (padrão `http://127.0.0.1:11434`); instale o Ollama e baixe um modelo (`ollama pull qwen3:4b-instruct`). Para documentos longos o Ponte pede sozinho uma janela de contexto maior (até 32 mil tokens), o que usa mais memória |
| **Ollama Cloud** | modelos grandes sem GPU local; conta em ollama.com (gratuita por uso ou plano mensal) | a chave de API criada em [ollama.com/settings/keys](https://ollama.com/settings/keys) |
| **Outro servidor HTTPS** | servidor do órgão compatível com a API `/api/chat` do Ollama | endereço e token opcional |
| **ChatGPT (assinatura)** | quem já paga ChatGPT Plus ou Pro e quer usar essa cota, sem chave de API | clique em **Entrar com o ChatGPT…**, autorize o Ponte no navegador (inclusive o **uso do plano**) e escolha o modelo na lista da sua conta |

Em qualquer opção, clique em **Listar modelos do servidor** e escolha o modelo na caixa de seleção: o Ponte consulta o servidor (`/api/tags` no Ollama, `/v1/models` na OpenAI) e mostra o que ele oferece. A configuração é salva no acervo, cifrada com a DPAPI da sua conta do Windows; desmarque **Guardar a chave neste computador** para manter a chave (ou a sessão do ChatGPT) só na memória da sessão. A chave também pode vir da variável `PONTEPROJ_AI_TOKEN`. Com Ollama Cloud, servidor remoto ou ChatGPT, o texto do documento sai do computador: avalie sigilo e LGPD antes de usar.

**Correção automática e progresso.** Toda resposta da IA passa pela conferência do programa (JSON completo, prioridade e providências válidas, trechos citados existentes literalmente nas fontes, prazo transcrito). Se a resposta é recusada, o Ponte devolve ao modelo a resposta anterior e o motivo exato e pede a versão corrigida, até três respostas no total; a conferência continua sendo a última barreira, e nada é salvo sem passar por ela. Enquanto a IA trabalha, a barra de baixo mostra a etapa ("Preparando os dados", "Recebendo a resposta… (N caracteres)", "Conferindo", "Tentativa 2 de 3: …") e o botão **Cancelar**, que interrompe na próxima etapa sem salvar nada. Falhas de rede ou de servidor não são repetidas. Não há agentes autônomos com ferramentas: o texto do documento é tratado como dado, nunca como instrução.

**ChatGPT (assinatura), em detalhe.** É o programa oficial "Sign in with ChatGPT" da OpenAI para aplicativos de código aberto que rodam localmente, como o Ponte. O login abre o navegador em `auth.openai.com` e volta ao Ponte por uma porta local (127.0.0.1); o Ponte não vê a sua senha e não recebe acesso às conversas, à memória nem à cobrança do ChatGPT. As chamadas vão para a API pública da OpenAI e são descontadas da cota da assinatura (Plus ou Pro; conta gratuita entra, mas não libera uso). O limite por aplicativo é ajustado em **ChatGPT ▸ Configurações ▸ Uso**; a cota é a mesma do Codex e dos demais aplicativos (uma janela de 5 horas e uma semanal), e quando acaba o Ponte avisa e **não** usa outro provedor por conta própria. Na mesma página há redefinições de limite e a opção de comprar créditos. A tela de configuração mostra o aviso: a cada clique em Analisar ou Gerar sugestão, documento, notas, contexto e fontes vão para os servidores da OpenAI, nos EUA, sob os termos da OpenAI; nada é enviado ao importar. **Sair** apaga a sessão deste computador e pede à OpenAI que a encerre (confira em ChatGPT ▸ Configurações ▸ Aplicativos conectados).

---

## 8. Onde ficam os dados, backup e lixeira

- **Acervo**: `%LOCALAPPDATA%\PonteProj\acervo.sqlite3` (outra pasta via variável `PONTEPROJ_DATA_DIR`, sempre em disco local). Todo conteúdo — texto, PDFs, anexos, OCR, notas, minutas, tarefas, atendimentos, configuração de rede e de IA — é gravado cifrado com a **DPAPI** da sua conta do Windows: outra conta no mesmo PC, ou outro PC, não consegue ler. Cópias temporárias de anexos abertos em outros programas ficam em `…\PonteProj\temp\` e são apagadas ao abrir e ao fechar o Ponte.
- **Backup**: Menu ▾ → **Fazer backup agora** cria uma cópia consistente em `…\PonteProj\backups\`. O instalador também faz uma antes de cada atualização. Um backup só abre na mesma conta do Windows que o criou.
- **Lixeira**: excluir documento, tarefa, anexo ou versão importada pede confirmação e manda para a Lixeira (Menu ▾), de onde se restaura. Por padrão nada é apagado definitivamente; na própria janela, **Apagar de vez os itens excluídos há mais de** (7 dias a 1 ano) liga a limpeza automática, feita ao abrir o Ponte e uma vez por dia, e **Apagar agora os vencidos** antecipa. Um documento apagado de vez leva junto versões, PDFs, notas, minutas, tarefas e anexos; backups não são afetados. Em equipe, a limpeza é do computador principal, pela configuração dele.
- **Ao fechar** com algo não salvo, a janela lista cada pendência com um botão **Ir para**.
- **Exportar dados (CSV)**: Menu ▾ → **Exportar dados para CSV…** grava, numa pasta nova em `…\PonteProj\exportados\csv-AAAAMMDD-HHMMSS\`, três arquivos: `documentos.csv` (número, título, tipo, datas, processo, remetente, finalidade, assuntos, prioridade, juntada, contagem de tarefas e anexos), `tarefas.csv` (tarefa, ação, situação, prioridade, responsável, prazo, atraso, conclusão) e `atendimentos.csv` (painel Atendimentos). Formato: UTF-8 com BOM, separador `;`, datas `AAAA-MM-DD` e coluna "Mês" (`AAAA-MM-01`) para séries mensais. Não saem textos de documentos, notas, minutas, observações nem anexos. Os arquivos abrem no Excel e entram no **CERES** (painéis do gabinete) como conjuntos: envie cada `.csv` no setor, marque a coluna `ID` como identificador para reimportar sem duplicar, e use "Mês" ou a data como coluna de período.
- **Menu ▾ ▸ Sobre o Ponte** mostra a versão do programa e da extensão, a autoria, a licença e o endereço do projeto.

---

## 9. Solução de problemas

| Sintoma | O que fazer |
|---|---|
| O botão **Importar para o assistente** não aparece no SIGADOC | A extensão não está carregada ou a página não é a ficha de um documento. Clique no ícone da extensão: se disser *✗ Não ligado*, siga os passos que ele mostra. |
| *"O Ponte não foi encontrado neste computador"* | Abra o Ponte uma vez e reinicie o navegador. Se continuar, rode o instalador de novo **com a mesma conta do Windows**. |
| *"Não recebemos a confirmação do Ponte"* | Clique em **Repetir importação**. A repetição usa o mesmo identificador: nada é duplicado. **Se a mensagem se repete em toda tentativa e a extensão veio da Chrome Web Store, o Ponte instalado é anterior à 0.19.3**: essas versões não reconheciam a extensão da loja e fechavam sem responder. Instale o Ponte mais recente e reinicie o navegador. |
| *"O Ponte abriu e fechou sem responder"* / *"Esta extensão não é reconhecida por este Ponte"* | O mesmo caso, com a mensagem das versões novas: instale o Ponte mais recente (a partir da 0.19.3) com a sua conta do Windows e reinicie o navegador. |
| *"O Ponte não conseguiu abrir o acervo deste computador"* | Abra o Ponte pelo menu Iniciar: a tela mostra o motivo (acervo de outra conta do Windows, versão mais nova, arquivo em uso). Resolvido, clique em **Repetir importação**. |
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
| `src/labels.rs` | Rótulos e datas exibidos; campo de data `dd/mm/aaaa` com calendário |
| `src/storage.rs`, `src/protection.rs` | SQLite + DPAPI; `Store` é a única porta de entrada aos dados |
| `src/model.rs` | `Capture` (o que a extensão envia) e sua validação |
| `src/dossier.rs` | Vínculos entre documentos, versões importadas, detecção de juntada |
| `src/tasks.rs`, `src/tasks_ui.rs` | Tarefas, dependências, painel |
| `src/saiop.rs` | Atendimentos: modelo, importador `.xlsx` (leitor ZIP/XML próprio), painel |
| `src/intranet.rs` | Modos principal/conectado, sincronização em thread própria, fila offline, mesclagem em três vias |
| `src/pdf_reader.rs`, `src/evidence.rs`, `src/ai.rs` | Leitor de PDF + OCR do Windows, fontes para a IA, cliente Ollama |
| `src/windows_ui.rs` | Seletor de arquivo e verificação do registro do host (crate `windows`) |
| `extension/` | Extensão MV3: `capture-core.js` (leitura da página e validação), `content.js` (painel), `background.js` (host nativo e PDFs), `popup.*` |
| `installer/Ponte.iss` | Instalador Inno Setup |
| `scripts/` | `build.ps1`, `build-installer.ps1`, `build-webstore-package.ps1`, `install-app.ps1`, `install-native-host.ps1` |
| `tests/` | Fluxo nativo (`native_flow.rs`), extensão (`node --test`, Playwright) e fixtures fictícias |

Decisões que valem conhecer: a identidade de um documento é o número oficial validado por expressão regular (nunca um ID de DOM); toda gravação em SQLite passa por `protection::seal`; as capturas são imutáveis (conteúdo novo cria versão; conteúdo idêntico é ignorado com aviso, sem gravar nada); limites de 500 KiB de texto, 1 MiB por pacote, 8 MiB por PDF; a extensão só aceita `https://www.sigadoc.mt.gov.br` e remove segredos de URL.

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

# testes da extensão (31 testes; os de DOM usam o Chrome instalado)
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
2. `build.ps1 -Action All`, copie `target\release\ponteproj.exe` para `dist\ponteproj.exe` (o instalador empacota o que estiver em `dist\`; confira com `dist\ponteproj.exe --version`), escreva as notas em `ATUALIZACAO_X_Y_Z.md` (arquivo local, fora do repositório: vira o texto da Release).
3. `scripts\build-installer.ps1` → `dist\Ponte-Setup-X.Y.Z.exe` e a cópia de nome fixo `dist\Ponte-Setup.exe` (Inno Setup 6: `winget install JRSoftware.InnoSetup`).
4. `scripts\build-webstore-package.ps1` → `dist\ponte-extensao-webstore-A.B.C.zip`; envie no painel da Chrome Web Store (ficha e privacidade em [`extension/WEBSTORE.md`](extension/WEBSTORE.md)).
5. `gh release create vX.Y.Z dist\Ponte-Setup-X.Y.Z.exe dist\Ponte-Setup.exe dist\ponte-extensao-webstore-A.B.C.zip --notes-file ATUALIZACAO_X_Y_Z.md`. O `Ponte-Setup.exe` de nome fixo é o que sustenta o link direto do topo deste README (`releases/latest/download/Ponte-Setup.exe`): sem ele na Release mais recente, o link quebra.

A extensão publicada na loja tem ID próprio (`hhppmkbmainkdfmcfeaanknlnfgihnah`), registrado em `extension-store-id.txt`; o instalador e o `install-native-host.ps1` autorizam esse ID e o da versão carregada da pasta (`ecllnakmppjinmednoikimbpnbgbimhg`). A chave privada `extension.pem` **não está no repositório** e deve ficar em local seguro.

### O que está no repositório

Só o necessário para compilar, instalar e publicar o aplicativo e a extensão: `src/` e `Cargo.*` (aplicativo), `extension/` (extensão), `assets/` e `build.rs` (ícone), `installer/` e `scripts/` (instalador, host nativo, pacote da loja), `tests/` (fluxo nativo e extensão, com fixtures fictícias), `extension-id.txt`, `extension-key.txt` e `extension-store-id.txt` (identidades públicas da extensão), além deste README, do [`GUIA_DO_USUARIO.md`](GUIA_DO_USUARIO.md), da [`LICENSE`](LICENSE) e de [`extension/WEBSTORE.md`](extension/WEBSTORE.md) e [`extension/PRIVACIDADE.md`](extension/PRIVACIDADE.md) (publicação na loja). As notas de cada versão estão nas [Releases](https://github.com/demusis/ponteproj/releases); documentos de planejamento e a chave privada da extensão ficam fora do repositório.

---

## 11. Autoria e licença

O Ponte foi concebido e desenvolvido por **Carlo Ralph De Musis** e **João Gabriel Bezerra Estrela**. No programa, **Menu ▾ ▸ Sobre o Ponte** mostra estes créditos, a versão e o endereço do projeto.

O código é distribuído sob a licença **MIT** (arquivo [`LICENSE`](LICENSE)): pode ser usado, copiado, modificado e redistribuído livremente, inclusive para fins comerciais, desde que o aviso de autoria e a licença sejam mantidos. O software é fornecido sem garantias.

O Ponte não é um produto do SIGADOC nem da sua mantenedora; ele apenas lê, no navegador da pessoa, as páginas às quais ela já tem acesso. Assinaturas, tramitação e a versão oficial dos documentos continuam no SIGADOC.
