# Atualização 0.16.0 — IA que corrige a própria resposta, progresso visível e exportação CSV

**Data:** 2026-11-04
**Extensão:** 0.1.9 (sem mudanças; não precisa recarregar)
**Acervo:** sem mudança de esquema (continua na versão 10). Pode atualizar um computador de cada vez.

## Por que assim

Avaliamos usar "agentes" (modelo que decide sozinho ler outros documentos, navegar ou chamar funções) e concluímos que **não vale**: o Ponte trata o texto do documento como dado, nunca como instrução, e um agente ampliaria o risco de o documento manipular o modelo, o custo e a imprevisibilidade, sem ganho para um assistente cujo resultado é sempre revisado por uma pessoa. O que vale é um **loop de correção com o motivo exato** e um **progresso visível**: é o que esta versão traz.

## O que muda

- **Correção automática.** Antes, se a resposta da IA falhava na conferência do programa (JSON incompleto, trecho citado inexistente, prazo que não está no texto, prioridade inválida), a análise era descartada e a pessoa tentava de novo à mão. Agora o Ponte devolve ao modelo a resposta recusada e o motivo, e pede a versão corrigida, **até três respostas no total**. A conferência continua obrigatória: nada é salvo sem passar por ela. Falhas de rede, de servidor ou de tamanho não são repetidas.
- **Progresso por etapas.** A barra de baixo mostra o que está acontecendo: "Preparando os dados (12 KB)", "Enviando ao modelo…", "Recebendo a resposta… (1.250 caracteres)", "Conferindo a resposta contra as fontes…", "Tentativa 2 de 3: … Pedindo correção…". O botão de análise e o de minuta mostram a etapa atual.
- **Cancelar.** Ao lado do progresso, **Cancelar** interrompe a IA na próxima etapa; nada é salvo e a mensagem diz isso. Se a resposta já tinha terminado, ela é aproveitada.
- **Resposta chegando ao vivo.** O Ollama (local, Cloud ou servidor do órgão) passa a responder em fluxo, como o ChatGPT já fazia, então o contador de caracteres anda enquanto o modelo escreve.
- **Exportar dados para CSV** (Menu ▾): três arquivos, `documentos.csv`, `tarefas.csv` e `atendimentos.csv`, numa pasta nova em `exportados\csv-…`, com os dados administrativos (sem textos, notas, minutas, observações ou anexos): UTF-8 com BOM, separador `;`, datas `AAAA-MM-DD` e coluna "Mês" para séries mensais. Prontos para o Excel e para os painéis do CERES (um conjunto por arquivo; use a coluna `ID` como identificador).
- Mensagens de recusa mais precisas, pensadas para o modelo corrigir: o prazo recusado aparece na mensagem, e "trecho não encontrado" vale tanto para análise quanto para minuta.

## Para desenvolvedores

- `ai::Reporter` (clonável; `silent()` para testes e linha de comando): `step(texto)` envia a etapa por canal e `check()` falha quando a pessoa cancelou. Parâmetro novo em `analyze_with_sources`, `generate_reply_with_sources`, `chat` e `chatgpt::respond`.
- `ai::chat_with_repair(settings, system, data, schema, reporter, check)`: até `REPAIR_ATTEMPTS` (3) respostas; nas correções o JSON de dados ganha `previous_answer` (até 32 KiB) e `problem`, e o sistema recebe `REPAIR_NOTE`. `check_analysis` concentra as regras da análise; a minuta com fontes usa o mesmo laço.
- Ollama em `stream:true` com leitura NDJSON (`read_ollama_stream`); `done_reason:"length"` e `error` continuam recusados. `chatgpt::read_stream` reporta caracteres e checa cancelamento por evento.
- `src/export.rs` (novo): `export_all`/`export_to`, `render_csv` (quoting RFC 4180 com `;`), tabelas de documentos, tarefas e atendimentos; teste de ida e volta e de que o texto do documento não sai. `WorkKind::Export` no app.
- `app.rs`: `Worker { kind: WorkKind, progress, latest, cancel }`, `spawn(label, kind, |reporter| …)`, `cancel_worker`, barra de status com etapa e Cancelar; botões usam `WorkKind` em vez de prefixo do rótulo.
- Testes: laço de correção com mock Ollama (resposta inválida → correção com `previous_answer`/`problem` → aceita; três recusas → erro "3 tentativas"; HTTP 500 → sem repetição), leitura NDJSON (chunks, objeto único, `length`, erro, cancelamento), stream do ChatGPT com progresso e cancelamento.
