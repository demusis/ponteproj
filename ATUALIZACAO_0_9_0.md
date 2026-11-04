# Atualização 0.9.0 — Operação simplificada

**Data:** 2026-11-03
**Extensão:** 0.1.8 (recarregue a extensão no navegador após instalar; o ID e a chave não mudaram)

Esta versão aplica o `PLANO_OTIMIZACAO_UX.md` (Ondas 0 a 2 completas; Onda 3 parcial). Nada muda no esquema do banco, na cifragem ou nas regras de validação: um acervo da 0.8.0 abre normalmente.

## O que o usuário vê diferente

- **Instalação por duplo clique** (`Instalar.cmd`). O instalador faz cópia de segurança do acervo antes de mudar algo, recusa rodar com o Ponte aberto e remove versões antigas. A tela inicial do Ponte mostra se o navegador está ligado e tem o botão **Copiar caminho** da extensão.
- **Ficha do documento em 4 abas** (Resumo, Documento, Anotações e minutas, Mais) em vez de 8. O resultado da IA fica no topo do Resumo; Exportar (RTF) e Comparar versões ficaram visíveis.
- **Barra superior** com um único **Menu ▾** (Rede e equipe, Configurar IA, Lixeira, Backup) e um indicador de rede colorido com texto curto. O ícone ao lado de "PONTE" foi retirado.
- **Lista de documentos**: filtros atrás de **Filtrar ▾**; cartões mais limpos; atalho "⚠ N atrasado(s)".
- **Tarefas**: lista à esquerda e ficha à direita; botão **✓ Concluir** em um clique (e **Reabrir**); visões Hoje / Esta semana / Atrasadas / Todas, o resto em "Mais"; campos secundários em "Mais detalhes"; "Nova tarefa" no Resumo do documento.
- **Processos e demandas**: filas num combo com contagem; filtros avançados atrás de **Filtros ▾**; grade ordenável por coluna, ocupando a altura toda; ficha ao lado (não embaixo), com Salvar / Descartar / Encerrar / Fechar sempre visíveis; importação de planilha em janela própria com **seletor de arquivo**; prévia listando as linhas e marcando as já registradas (com "Importar só as novas"); desfazer importação com confirmação; dimensões agrupadas (Identificação, Situação e prioridade, Prazo, Condições e relações, Resultado); **Ligar a documento**; rótulos sem "(Col X)"; linha original da planilha em tabela.
- **Datas** em dd/mm/aaaa em toda a interface, com campo que aceita dd/mm/aaaa e botão **Hoje**.
- **Valores internos traduzidos** (prioridade, situação, tipo de minuta, finalidade, tipo de prazo, eventos) e contexto da análise exibido campo a campo em vez de JSON.
- **Avisos** de sucesso somem sozinhos; erros ficam até serem fechados. Botões de IA mostram "Analisando… N s" / "Gerando… N s". Ao fechar com pendências, a janela lista cada uma com **Ir para**.
- **Rede e equipe**: código de conexão único (copiar no principal, colar no outro); "Anfitrião/Cliente/token/choque" viraram "computador principal / conectar / chave / conflito de edição"; sincronização em segundo plano (a janela não trava mais quando o principal está fora); gravações offline são anunciadas; conflitos mostram os campos lado a lado, com quem alterou, e pedem confirmação; **Combinar** faz mesclagem real em três vias (o estado anterior à edição offline é guardado).
- **Extensão**: mensagens de erro reescritas com a ação primeiro; painel na página menor fora da ficha e sem a linha "Adaptador"; popup com status primeiro; PDFs não baixados aparecem na mensagem de sucesso.

## Correções de comportamento

- O Ponte abre na tela inicial (não no painel Tarefas).
- "Criar tarefa" no atendimento salva a ficha antes e nunca anuncia "Tarefa #0".
- Análise e minuta salvam contexto e notas pendentes em vez de bloquear.
- "Limpar" na lista vazia também limpa a busca.
- Trocar para "Conectar ao computador principal" não derruba mais o serviço antes de salvar; gerar nova chave avisa que os colegas precisarão de novo código.

## Para desenvolvedores

- Novo módulo `src/labels.rs` (rótulos, datas, `date_field`) e `src/windows_ui.rs` (seletor de arquivo e verificação do registro do host, via a crate `windows` já usada).
- `OutboxEntry`/`SyncConflict` ganharam `base` (`#[serde(default)]`); `FieldDiff` e `Store::conflict_differences` alimentam a tela de conflitos.
- `IntranetPanel::refresh_client` agora recebe `db_path` e usa uma thread própria (`SyncWorker`).
- `Store::complete_task` / `reopen_task`; `SaiopImportPreview::already_registered` / `only_new`; `Store::undo_saiop_batch_plan`; `create_task_for_saiop` devolve `Option<i64>`.
- Testes: 50 no crate, 3 de fluxo nativo, 30 da extensão (`node --test`, com Playwright).
- **Instalador `.exe`**: `installer\Ponte.iss` (Inno Setup 6, por usuário, sem administrador) gerado por `scripts\build-installer.ps1` → `dist\Ponte-Setup-0.9.0.exe`. Faz backup do acervo, instala programa e extensão, grava o manifesto do host e as chaves HKCU, cria atalho, remove versões antigas e o legado do instalador por script; desinstalação pelo Windows sem apagar dados.
- **Chrome Web Store**: `scripts\build-webstore-package.ps1` → `dist\ponte-extensao-webstore-0.1.8.zip` (sem `key`); ficha, privacidade e passo a passo em `extension\WEBSTORE.md` e `extension\PRIVACIDADE.md`. O envio exige a conta de desenvolvedor do gabinete. Após a publicação, o ID da loja vai em `extension-store-id.txt` e tanto `build-installer.ps1` quanto `install-native-host.ps1` passam a autorizá-lo junto ao ID da pasta.
- Pendente: envio à loja (ação do responsável pela conta) e teste com usuários (Onda 4).
