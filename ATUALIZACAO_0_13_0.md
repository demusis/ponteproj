# Atualização 0.13.0 — Dependências entre tarefas visíveis

**Data:** 2026-11-04
**Extensão:** 0.1.8 (sem mudanças; não precisa recarregar)
**Acervo:** sem mudança de esquema (continua na versão 10). Pode atualizar um computador de cada vez.

## O que muda

Até agora, uma tarefa que dependia de outras não mostrava isso em lugar nenhum: a pessoa só descobria ao clicar em ✓ Concluir e receber a recusa. Dependências formam uma cadeia (uma tarefa pode depender de duas, duas podem depender da mesma), e a lista passa a mostrar essa cadeia sem exigir que ela seja uma árvore.

**Na lista de tarefas (painel Tarefas e Resumo ▸ Tarefas deste documento):**

- **Recuo por nível**, como os documentos juntados na lista da esquerda: uma tarefa que depende de outra aparece deslocada à direita; quem depende de uma tarefa já deslocada, mais ainda. O recuo para em 4 níveis; além disso aparece a marca *·· nível N*.
- **↳ depende de: A, B** abaixo do título, com os títulos das dependências diretas. Concluída aparece riscada com ✓, atrasada em vermelho. Passar o mouse mostra a situação e o prazo de cada uma.
- **⛓ aguardando anteriores:** enquanto houver dependência aberta, o cartão mostra ⛓ (com "Aguardando: A, B" ao passar o mouse) e o botão **✓ Concluir** fica desabilitado com a dica *"Conclua antes: A, B"*, em vez do erro depois do clique.
- **Ordem:** cada tarefa é listada depois das tarefas de que depende; fora isso, a ordem continua a de sempre (atrasadas, prazo, prioridade). Uma tarefa com duas dependências aparece uma vez só, depois da última delas.
- Nova visão em **Mais ▾ ▸ Prontas para fazer**: as pendentes sem nenhuma dependência aberta, isto é, o que pode ser atacado agora.

**Na ficha da tarefa:**

- Bloco **Cadeia de dependências** (só aparece quando há alguma): **Antes desta**, tudo o que precisa ser concluído primeiro, em qualquer nível; **Depois desta**, tudo o que espera por ela. Cada linha mostra situação, prazo e o botão **Abrir** (com a pergunta de salvar/descartar, se a tarefa atual tiver alteração pendente).
- Em **Mais detalhes ▸ Depende de outras tarefas**, as tarefas que **formariam ciclo** (as que já dependem desta, direta ou indiretamente) aparecem desabilitadas com a explicação *"‘X’ já depende desta tarefa; marcar formaria um ciclo."* As concluídas vão para o fim da lista, em cinza.

**Regras mantidas:** salvar continua recusando ciclos, dependências inexistentes e conclusão antes das anteriores; a cascata dos documentos juntados continua respeitando dependências abertas.

## Para desenvolvedores

- `tasks::Dependencies` (novo): `level`, `direct`, `open_blockers`, `is_blocked`, `ancestors`, `descendants` e `order` (ordem topológica estável por Kahn, mantendo a ordem dada nos empates). Tolera ciclo (nunca gerado pelo salvamento) sem travar: teste `levels_order_blockers_and_chains_follow_a_diamond_and_survive_a_cycle` cobre diamante, cadeia de 12 níveis, subconjunto e ciclo.
- `tasks_ui.rs`: visão 8 "Prontas para fazer"; `titles()` e `chain_row()`; lista ordenada por `deps.order`, recuo por `deps.level`, bloco "Cadeia de dependências" na ficha com `open_task_requested` (passa pela mesma fila de intenções da 0.12.0); caixas de seleção desabilitadas para `deps.descendants(task_id)`.
- `app.rs` `document_tasks`: mesma ordem, recuo, ⛓ e Concluir desabilitado no Resumo.
