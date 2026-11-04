# Plano Técnico e Handoff: Identificação, Persistência e Indicador Visual de Documentos Juntados (SIGADOC)

**Data do registro:** 2026-11-03  
**Projeto:** Ponte • Assistente Documental SIGADOC (`ponteproj` v0.8.0)

---

## 1. Estado Atual do Projeto (O que já está pronto e testado)

1. **Controle Operacional do Gabinete — Processos e Demandas (SAIOP)** (`src/saiop.rs`):
   - Entradas e reentradas por passagem no gabinete, 4 dimensões separadas (*Situação*, *Prioridade*, *Condições/Relações*, *Resultado*), indicadores por eventos e importador `.xlsx` nativo da aba `PROCESSOS` com prévia, deduplicação SHA-256 e reversão segura de lote.
2. **Compartilhamento na Intranet com Modo Offline, Sincronização Mútua e Mesclagem de Choques** (`src/intranet.rs`):
   - Modos *Local*, *Anfitrião na Intranet* e *Cliente na Intranet*.
   - Fila offline criptografada com DPAPI (`offline_outbox`) permitindo que estações clientes continuem gravando normalmente quando o PC Anfitrião estiver desligado.
   - Sincronização mútua automática na reconexão (`Store::sync_from_host`), detecção de choques de edição concorrente (`offline_conflicts`) e resolução em 1 clique (**Mesclar as duas versões**, **Manter minha versão** ou **Manter versão do Anfitrião**).
3. **Ícones do Aplicativo e da Extensão** (`src/icon.rs`, `build.rs`, `assets/ponteproj.ico`, `extension/icons/icon-*.png`):
   - Emblema "A Ponte Documental" embutido via `windres.exe` no executável Windows, na janela `eframe`, no cabeçalho interno e na extensão MV3 (preservando a `key` e o ID `ecllnakmppjinmednoikimbpnbgbimhg`).
4. **Filtros e Sinalizadores Visuais de Tarefas na Lista de Documentos** (`src/app.rs`):
   - Filtros por situação das tarefas (*Todos*, *Em aberto*, *● No prazo*, *⚠ Atrasados*, *✓ Concluídos*) e por *Prioridade*.
   - Barra vertical lateral colorida (`5 px`) e pílulas de status em cada documento (vermelho = atrasada, âmbar = vence hoje, azul-petróleo = em aberto no prazo, verde = concluída).

---

## 2. Próxima Tarefa Planejada: Documentos Juntados (`Juntada` no SIGADOC)

### 2.1. Evidência na Ficha do SIGADOC
Quando um documento (ex.: Despacho `SESP-DES-2026/39019`) é juntado dentro de outro documento/processo (ex.: `SESP-DIC-2026/97546`), a página do SIGADOC (`https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir`) apresenta dois sinais explícitos:
1. **Subtítulo da Via (`<h3>`):**
   - Exemplo: `1ª Via (Setor Competente) - Juntado`
   - Já capturado pela extensão em `capture.metadata["via_status"]`.
2. **Tabela de Eventos da ficha (`Data | Lotação | Evento | Descrição`) e histórico detalhado (`exibirAntigo` / `movement_history`):**
   - Linha com Evento = `Juntada` (ou `Apensação`) e Descrição = `Juntado ao documento: SESP-DIC-2026/97546-A` (onde `SESP-DIC-2026/97546-A` é um link `<a href="...">` apontando para a via `-A` do documento-pai `SESP-DIC-2026/97546`).

---

### 2.2. Etapas de Implementação

#### Etapa 1 — Captura na Extensão (`extension/capture-core.js`)
- No laço de coleta de referências (`for (const anchor of root.querySelectorAll("a[href]"))`, por volta das linhas 274–291 de `extension/capture-core.js`):
  1. Atualmente a linha 283 testa `documentNumber(text)`, que rejeita siglas com sufixo de via como `SESP-DIC-2026/97546-A`. Alterar para aceitar também `documentQueryNumber(text)`.
  2. Incluir no cálculo de `context` (linha 278) o texto da linha/célula da tabela (`anchor.closest("tr")?.textContent` ou `anchor.parentElement?.textContent`) sanitizado via `label(...)`.
  3. Quando o contexto da linha/célula contiver `juntad` ou `apensad` (ex.: `Juntado ao documento: SESP-DIC-2026/97546-A`), classificar a referência com `kind = "antecedent"` (ou `"reference"`) e preservar no `label` a indicação `"Juntado ao documento: <SIGLA>"` para que mesmo sem histórico `exibirAntigo` a referência estruturada já venha explícita.
  4. **Regra crítica:** Não alterar `META_KEYS` de forma incompatível nem modificar `manifest.key` ou `extension-id.txt`. Rodar `node --test .\tests\extension\core.test.cjs` para validar os testes da extensão.

#### Etapa 2 — Extração Determinística e Persistência no Backend Rust (`src/model.rs`, `src/dossier.rs`, `src/storage.rs`)
1. **Extrator `detect_juntada(capture: &Capture) -> Option<JuntadaInfo>`** (em `src/dossier.rs` ou `src/model.rs`):
   - Estrutura sugerida:
     ```rust
     #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
     pub struct JuntadaInfo {
         pub is_joined: bool,
         pub parent_document_number: Option<String>, // Número base canônico, ex.: "SESP-DIC-2026/97546"
         pub raw_target_via: Option<String>,         // Ex.: "SESP-DIC-2026/97546-A"
     }
     ```
   - Fontes de detecção (em ordem de especificidade):
     a) `dossier::movements(capture)`: procurar eventos cujo `event` seja `Juntada` / `Apensação` e cuja `description` contenha `Juntado ao documento:` seguido de sigla documental (ignorando se houver evento posterior de `Desentranhamento` / `Desapensação`).
     b) `capture.references`: procurar referências cujo `label` contenha `Juntado ao documento:` ou cujo `url` aponte para outra sigla em contexto de juntada.
     c) `capture.text`: procurar na tabela renderizada em texto linhas do tipo `| Juntada | Juntado ao documento: <SIGLA> |` ou `Juntado ao documento: <SIGLA>`.
     d) `capture.metadata.get("via_status")`: se contiver `- Juntado` ou `- Apensado`, marcar `is_joined = true` mesmo que o número-pai não esteja no texto.
   - Normalizar a sigla do documento-pai removendo sufixo de via (`-A`..`-Z`) para obter o `document_number` canônico (ex.: `SESP-DIC-2026/97546-A` $\rightarrow$ `SESP-DIC-2026/97546`).
2. **Enriquecimento de `DocumentSummary` (`src/model.rs` e `Store::list_documents` em `src/storage.rs`):**
   - Adicionar campos com `#[serde(default)]` em `DocumentSummary`:
     - `pub is_joined: bool`
     - `pub joined_to_number: Option<String>` (ex.: `Some("SESP-DIC-2026/97546")`)
     - `pub joined_to_id: Option<i64>` (ID local do documento-pai se ele já existir em `documents` e não estiver na lixeira)
     - `pub joined_children_count: usize` (quantidade de documentos ativos no acervo que estão juntados a este documento)
3. **Criação automática e idempotente de vínculo em `document_links` (`src/storage.rs` / `src/dossier.rs`):**
   - Sempre que `import_capture` / `import_bundle` gravar um documento (e também de forma preguiçosa/idempotente ao abrir o `Store`):
     - Se o documento importado for um filho juntado a `P` (`joined_to_number = Some(P)`) e `P` já existir no acervo (`parent_id`), verificar se já há vínculo em `document_links` entre `child_id` e `parent_id`. Se não houver, inserir automaticamente um vínculo ativo (`kind = "antecedente"`, `reason = "Identificado automaticamente: Juntado ao documento P"`).
     - Se o documento importado for o pai `P`, verificar se outros documentos já importados no acervo apontam `joined_to_number == Some(P)` e criar os vínculos faltantes automaticamente.
   - **Regra de segurança DPAPI:** Qualquer dado gravado em SQLite deve passar por `protection::seal` (nenhum número de documento ou texto em claro no arquivo `.sqlite3`).

#### Etapa 3 — Indicador Visual e Espaçamento (Recuo Hierárquico) na Interface (`src/app.rs`)
1. **Ordenação e Agrupamento na Barra Lateral (`PonteApp::sidebar` em `src/app.rs`):**
   - Ao montar a lista `filtered_docs` na barra lateral esquerda:
     - Se tanto o documento-pai quanto seus documentos juntados (filhos) estiverem presentes na lista filtrada, posicionar os documentos juntados **imediatamente abaixo do respectivo documento-pai**.
2. **Recuo Visual (Espaçamento à Esquerda) e Conector `↳`:**
   - Para todo documento com `doc.is_joined || doc.joined_to_number.is_some()`:
     - Adicionar um espaçamento horizontal à esquerda na linha do cartão (ex.: `18.0 px` quando agrupado sob o pai visível na lista, ou `12.0 px` quando o pai não está na lista/acervo) acompanhado do símbolo **`↳`** em cor de destaque suave antes da barra vertical de status.
   - Abaixo do botão do cartão, exibir uma pílula/etiqueta:
     - Se `joined_to_number` for conhecido: **`↳ Juntado a SESP-DIC-2026/97546`** (ou `↳ Juntado a SESP-DIC-2026/97546 (não importado)` se `joined_to_id.is_none()`).
     - Se apenas `via_status` indicou juntada sem número: **`↳ Documento juntado`**.
   - No cartão do documento-pai (quando `doc.joined_children_count > 0`), exibir a pílula **`📎 N juntado(s)`**.
3. **Cabeçalho da Ficha do Documento Aberto (`PonteApp::document` em `src/app.rs`):**
   - Se o documento aberto estiver juntado a outro, mostrar no topo uma pílula em destaque **`↳ Juntado ao documento SESP-DIC-2026/97546`**. Se `joined_to_id == Some(parent_id)`, incluir um botão/link rápido **"Abrir documento principal"** que chama `self.select(parent_id)`.
   - Se o documento aberto possuir filhos juntados a ele (`joined_children_count > 0`), mostrar no topo **`📎 N documento(s) juntado(s)`** com botões rápidos para abrir cada documento juntado.

---

## 3. Comandos de Compilação e Verificação neste Ambiente

O ambiente utiliza o conjunto portátil de ferramentas Rust + MinGW em `C:\Users\CARLO\AppData\Local\Programs\PonteProj\devtools`. Sempre use `--locked` (já incluído em `scripts/build.ps1`):

```powershell
# 1. Rodar check, todos os testes unitários/integração e gerar o binário release
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1 -Action All -ToolsDirectory 'C:\Users\CARLO\AppData\Local\Programs\PonteProj\devtools'

# 2. Atualizar o binário de distribuição
Copy-Item -LiteralPath .\target\release\ponteproj.exe -Destination .\dist\ponteproj.exe -Force

# 3. Rodar os testes unitários da extensão do navegador (se houver node no PATH)
node --test .\tests\extension\core.test.cjs
```
