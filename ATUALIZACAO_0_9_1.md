# Atualização 0.9.1 — Conclusão em cascata dos documentos juntados

**Data:** 2026-11-03
**Extensão:** 0.1.8 (sem mudanças; não precisa recarregar)

## O que muda

- **Juntados seguem o principal.** Quando a última tarefa aberta de um documento principal é concluída, as tarefas abertas dos documentos juntados a ele — e dos juntados a estes — são concluídas automaticamente. Cada tarefa recebe a anotação *"[data] Concluída automaticamente: o documento principal X foi concluído"*, e a mensagem de confirmação lista o que foi concluído.
  - Tarefas que dependem de outra ainda aberta são tentadas depois das demais; o que ficar aberto é informado na mensagem.
  - Reabrir o principal não reabre as peças.
  - Vale para o botão **✓ Concluir**, para o formulário da tarefa (Situação: Concluída) e, em equipe, para gravações vindas dos outros computadores (a cascata roda no computador principal).
- **Contagem de "Todas"** no painel Tarefas agora reflete o que a lista mostra: só as pendentes ou, com "Mostrar concluídas" marcado, todas.

## Para desenvolvedores

- `Store::save_task_checked` chama `cascade_conclusion` após gravar uma conclusão; `Store::take_cascade_notes` entrega o relato à interface. `cascade_suppressed` evita cascatas aninhadas; a árvore de juntadas é percorrida com conjunto de visitados.
- Teste: `tasks::completion_tests::concluding_the_main_document_concludes_the_joined_pieces_in_cascade`.
