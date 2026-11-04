# Plano — Tarefas: edição sem distração e hierarquia de dependências

Duas observações do uso (2026-11-04), com diagnóstico do comportamento atual e a proposta para a **0.12.0**.

## 1. Sair da tarefa em edição sem perceber

### O que acontece hoje

| Ação durante a edição de uma tarefa | Comportamento |
|---|---|
| Clicar em outra tarefa da lista | **Bloqueado**, com aviso "Salve ou cancele a edição atual antes de trocar de tarefa." |
| Nova tarefa / Usar sugestão da IA | **Bloqueado**, com aviso. |
| Clicar em **Documento**, **Processos e demandas** (topo) ou num documento da lista à esquerda | **Permitido**: o painel Tarefas some, a edição continua na memória, sem nenhum sinal na tela. |
| Botão **Abrir documento** da própria ficha | Idem: vai para o documento e a edição fica escondida. |
| Fechar o Ponte | A janela de pendências lista "Tarefa em edição" com **Ir para**. |

O problema é o terceiro caso: a edição não se perde, mas desaparece da vista. A pessoa volta ao painel Tarefas mais tarde e encontra uma ficha "● não salva" que não lembra de ter deixado, ou salva por engano algo incompleto.

### Proposta: travar a saída, não a consulta

Bloquear tudo (janela modal) resolveria a confusão, mas impediria o caso legítimo de **consultar o documento enquanto se escreve a tarefa** (ler o texto importado para copiar um prazo, por exemplo).

Caminho proposto:

1. **Interceptar a saída com a mesma pergunta usada ao fechar o Ponte.** Ao clicar em Documento, Processos e demandas, num documento da lista ou em Abrir documento com uma tarefa não salva, aparece: *"A tarefa ‘Responder ao ofício’ tem alterações não salvas."* com três botões: **Salvar e continuar**, **Descartar alterações** e **Voltar à tarefa**. Nada sai da tela sem decisão.
2. **Consultar sem sair.** O botão **Abrir documento** da ficha passa a abrir o documento numa **janela flutuante de leitura** (texto importado e PDFs, só leitura), por cima do painel de Tarefas. A ficha continua visível e a pessoa copia o que precisa. É o mesmo componente da aba Documento, sem edição.
3. **Lembrete permanente.** Enquanto houver tarefa não salva, o rótulo **Tarefas** no topo mostra um ponto laranja ("Tarefas ●"), como a aba Anotações já faz com notas não salvas.
4. **Nova tarefa a partir do Resumo** continua abrindo o painel; se já houver edição, a pergunta acima aparece em vez do aviso atual.

Sem alteração de dados nem de rede; só interface. Esforço: meio dia, com teste do fluxo de interceptação no harness de interface já existente.

## 2. Hierarquia visual das dependências

### O que acontece hoje

- A dependência é escolhida em **Mais detalhes ▸ Depende de outras tarefas**, numa lista de caixas de seleção com todas as tarefas.
- Nada na lista de tarefas indica que uma tarefa depende de outra ou que está "travada" até as anteriores concluírem; a pessoa descobre ao clicar em ✓ Concluir e receber "Conclua as tarefas anteriores antes de concluir esta tarefa."
- **Ciclos já são impedidos ao salvar** ("As dependências formariam um ciclo."), mas só depois de marcar a caixa e clicar em Salvar; a lista deixa marcar qualquer tarefa, inclusive as que já dependem desta.

As dependências formam um **grafo sem ciclos**, não uma árvore: uma tarefa pode depender de duas e duas podem depender da mesma. A indicação visual tem de aceitar isso sem mentir.

### Proposta

**Na lista de tarefas (painel Tarefas e Resumo ▸ Tarefas deste documento):**

- **Recuo por nível**, como os documentos juntados: uma tarefa aparece deslocada à direita conforme a profundidade na cadeia (0 = não depende de nada; 1 = depende de tarefas de nível 0; e assim por diante). O recuo é limitado a 4 níveis; além disso, mostra "·· nível 6".
- **Etiqueta ↳ depende de: A, B** no cartão, com os títulos das dependências diretas. Dependência concluída aparece riscada (✓), aberta aparece normal, atrasada em vermelho.
- **Estado "aguardando anteriores"**: cartão com ícone ⛓ e o botão ✓ Concluir desabilitado, com a explicação na dica ("Conclua antes: A, B"). Hoje o erro só aparece depois do clique.
- **Ordem**: dentro da mesma visão, uma tarefa é listada depois das tarefas de que depende (ordem topológica, desempatada pelo prazo). Uma tarefa com duas dependências aparece uma única vez, depois da última delas.
- Nova visão em **Mais ▾**: **Prontas para fazer** (pendentes sem dependência aberta), que é a lista do que pode ser atacado agora.

**Na ficha da tarefa:**

- Bloco **Cadeia de dependências** com duas listas recuadas: **Antes desta** (o que precisa concluir primeiro, em todos os níveis) e **Depois desta** (o que está esperando por ela). Cada linha com estado e botão **Abrir**.
- Na lista de caixas de seleção, as tarefas que **formariam ciclo** (as que já dependem desta, direta ou indiretamente) aparecem **desabilitadas** com a dica "Esta tarefa já depende de ‘X’; marcar formaria um ciclo." A regra de salvamento continua como última barreira.
- Tarefas concluídas ficam no fim da lista de opções, em cinza, para não poluir.

**Regras de segurança mantidas:** o salvamento continua recusando ciclos, dependências inexistentes e conclusão antes das anteriores; a cascata dos documentos juntados continua respeitando dependências abertas.

Esforço: um dia (cálculo de níveis e ordem topológica em `tasks.rs`, com testes para diamantes e cadeias longas; recuo, etiquetas e visão nova em `tasks_ui.rs`; bloco na ficha).

## 3. Entrega

Ambos os itens cabem na **0.12.0**, sem mudança de acervo nem de extensão. Ordem sugerida: item 1 primeiro (menor e resolve a confusão relatada), depois o item 2.
