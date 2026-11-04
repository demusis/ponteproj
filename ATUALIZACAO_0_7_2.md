# Versão 0.7.2 — resumo e trabalho diário

Aplicativo 0.7.2, schema 7, extensão 0.1.7. Não é necessário recarregar a extensão ou reimportar. A atualização reorganiza a leitura e a navegação, sem modificar documentos, prioridades, prazos ou tarefas salvas.

## Navegação

- **Resumo:** tipo, data documental, modelo, situação oficial, finalidade local salva e classificação oficial. Mostra quantidade de PDFs/texto/referências e observações de cobertura. Há atalhos para o original, arquivos, histórico e contexto local.
- **Dados SIGADOC:** metadados oficiais completos com rótulos em português. Campos ausentes são identificados como não disponíveis nos campos importados.
- **Histórico:** eventos importados com datas, via/escopo, cadastrante, atendente e descrição, preservando os rótulos da fonte.
- **Arquivos:** PDFs e controles de leitura/exportação primeiro; cobertura e pendências em seção própria. Hashes ficam nos detalhes de cada arquivo.
- **Vínculos e versões:** dossiê local, vínculos confirmados e histórico de capturas, sem misturar eventos oficiais com versões locais.
- **Texto**, **Notas** e **Minutas:** recursos existentes preservados.

## Tarefas próximas do documento

O Resumo apresenta pendências vinculadas ao documento, situação, prioridade da tarefa e prazo local em DD/MM/AAAA. Até cinco pendências são mostradas, com contagem completa e acesso ao painel. Tarefas concluídas permanecem no painel, acessíveis pela opção Mostrar concluídas.

**Editar / retomar tarefa** abre a edição da tarefa exata. **Abrir no painel de tarefas** aplica filtro pelo documento; **Mostrar todos os documentos** remove o filtro. Navegação não substitui uma tarefa com edição pendente. Se o filtro for de outro documento, o editor informa explicitamente a qual documento pertence a edição preservada.

Situações, prioridades e ações ganham rótulos com acentos. A busca mantém os termos internos antigos para não prejudicar pesquisas anteriores.

## Contexto e classificação

O formulário local começa recolhido; **Editar contexto local** permite abri-lo. Assuntos/prioridade locais e a finalidade continuam separados da classificação documental oficial. A mensagem genérica não classificada na biblioteca foi substituída por uma indicação explícita de prioridade local ainda não definida.

Fontes para IA e documentos semelhantes ficam disponíveis sem ocupar a leitura principal. A análise continua opcional e exige ação explícita, mostrando destino e modelo. Observações de cobertura permanecem visíveis antes de analisar.

## Verificação

33 testes Rust passaram, incluindo preservação de tarefa não salva ao mudar a navegação. O teste do pacote nativo/PDF/schema 7 passou. A comparação com o backup confirmou oito capturas, uma tarefa e preservação de todos os BLOBs anteriores conferidos.

Na interface real, o Resumo apresentou data/tipo/modelo, finalidade Ciência e download, PDF original e tarefa existente. O atalho de edição abriu a tarefa correta com filtro pelo documento, sem alterar seu conteúdo.

Backup anterior: `%LOCALAPPDATA%\PonteProj\backups\pre-v072-20261103-090639-563150.sqlite3`.

SHA256 instalado: `80EA83CC8F509471D9CBD3C8CC57154ABBADC571A8520CF1A771B450BD8AC913`.

Janelas anteriores foram preservadas para não perder textos eventualmente ainda em edição. Use a janela Ponte 0.7.2 para a apresentação nova.

Também foi conferido o atalho Editar contexto local: o formulário abriu sem alterar os valores e pôde ser recolhido novamente. O resumo ficou pronto para uso, sem edições de conteúdo durante a conferência.
