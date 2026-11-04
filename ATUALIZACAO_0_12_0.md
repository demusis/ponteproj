# Atualização 0.12.0 — Tarefa em edição não some mais da tela

**Data:** 2026-11-04
**Extensão:** 0.1.8 (sem mudanças; não precisa recarregar)

> **Esta versão inclui também o que entrou na 0.10.0 e na 0.11.0**, que não chegaram a ser publicadas: configuração de IA guardada protegida com a opção **Ollama Cloud** e escolha do modelo numa lista ([notas da 0.10.0](https://github.com/demusis/ponteproj/blob/main/ATUALIZACAO_0_10_0.md)), e **anexos** em documentos e tarefas ([notas da 0.11.0](https://github.com/demusis/ponteproj/blob/main/ATUALIZACAO_0_11_0.md)).
>
> **Equipe em rede:** por causa dos anexos, o acervo passa para a versão 10 de esquema. Atualize todos os computadores do gabinete a partir da 0.9.x; um Ponte anterior recusa abrir um acervo já atualizado e pede a versão atual.

## O que muda

Antes, com uma tarefa em edição, clicar em **Documento**, **Processos e demandas**, num documento da lista à esquerda ou em **Abrir documento** trocava de tela sem aviso: a edição continuava na memória, mas sumia da vista, e a pessoa só a reencontrava ao voltar ao painel Tarefas (ou na lista de pendências ao fechar o Ponte).

- **Pergunta antes de sair.** Com uma tarefa não salva, qualquer caminho que a esconderia ou substituiria abre a janela *"A tarefa ‘Responder ao ofício’ tem alterações não salvas."* com três botões: **Salvar e continuar** (grava e segue para onde você clicou), **Descartar alterações** (volta a tarefa ao que estava salvo; uma tarefa nova é apagada) e **Voltar à tarefa**. Nada muda de tela sem decisão. Vale para os botões do topo, a lista de documentos, **Nova tarefa** e **Editar** no Resumo, **Preparar tarefa** da análise de IA e o clique em outra tarefa da lista (que antes só mostrava um aviso).
- **Se salvar falhar** (título vazio, data inválida, dependência em ciclo), o motivo aparece na própria pergunta e a edição fica onde estava: corrija com **Voltar à tarefa** ou descarte.
- **Consultar sem sair.** Na ficha da tarefa, o botão **Abrir documento** virou dois: **Consultar documento** abre o texto importado e os PDFs numa janela por cima do painel, só leitura, para copiar um prazo ou conferir um trecho enquanto escreve; **Ir para o documento** abre a ficha completa (com a pergunta acima, se houver alteração pendente).
- **Lembrete permanente.** Enquanto houver tarefa não salva, o rótulo **Tarefas** no topo mostra **Tarefas ●** em laranja, em qualquer tela, como já acontece com *Anotações e minutas ●*.
- **Tarefa nova intocada não conta.** Clicar em **Nova tarefa** e sair sem digitar nada não dispara a pergunta nem aparece nas pendências: não há nada a perder.

## Para desenvolvedores

- `tasks_ui::TaskPanel`: `dirty()` ignora uma tarefa nova idêntica a `Task::new`; novos métodos `editing_title`, `save_draft`, `discard_draft`, `open_task` (abrir da lista sem mexer no filtro), `take_blocked` (`Intent::Open` / `Intent::New`) e `take_consult_request`. O painel não recusa mais com mensagem: devolve a intenção bloqueada para o app decidir.
- `app.rs`: `enum Leave` (Saiop, Documents, Select, NewTask, OpenTask, EditTask, Suggestion), `navigate()` intercepta quando a tarefa visível está suja ou a intenção a substituiria, `perform_leave()` executa depois da resposta; janela "Tarefa com alterações não salvas" em `confirmations()`; `consult_window()` reaproveita `files_list` e o mesmo visor de texto da aba Documento. A tela fica desabilitada enquanto a pergunta está aberta (`enabled` inclui `leave.is_none()`).
- Teste `leaving_an_unsaved_task_is_a_question_not_a_silent_loss` cobre título da pergunta, salvar/descartar (nova e existente), falha de salvamento mantendo a edição e a fila de intenções.
