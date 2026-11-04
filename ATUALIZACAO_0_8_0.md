# Versão 0.8.0 — exclusão com lixeira

Aplicativo 0.8.0, schema 8, extensão 0.1.7. A extensão permanece igual; não é necessário recarregá-la nem reimportar documentos.

## Como usar

- **Documento:** abra a ficha e use **Excluir…** no canto superior direito. A confirmação descreve o efeito antes de mover para a lixeira. Capturas, arquivos, notas, minutas e tarefas do documento ficam preservados e ocultos com ele.
- **Captura:** em **Vínculos e versões**, use **Excluir captura #…**. Se for a captura em uso, a confirmação informa qual captura passará a ser usada. A última captura ativa não pode ser excluída isoladamente; nesse caso, exclua o documento.
- **Tarefa:** abra sua edição no painel de tarefas e use **Excluir tarefa…**. Salve ou cancele alterações pendentes primeiro.
- **Restaurar:** o botão **Lixeira**, na barra superior, lista os itens e permite restaurá-los. Se o documento estiver na lixeira, restaure-o antes de restaurar seus itens individualmente.

A exclusão é local, reversível e não registra atos no SIGADOC. Arquivos exportados e backups permanecem. Esta versão não oferece exclusão permanente nem esvaziamento da lixeira; mover itens não libera espaço em disco.

Restaurar uma captura a devolve ao histórico sem trocar automaticamente a captura atual. Restaurar um documento mantém na lixeira os itens que já tinham sido excluídos individualmente.

## Proteções

- Capturas usadas por minutas ou análises salvas permanecem protegidas, inclusive quando usadas como fontes adicionais de outro documento.
- Tarefas e documentos com dependências ativas não podem ser excluídos enquanto outra tarefa depender deles. A restauração exige recuperar primeiro as dependências necessárias.
- Edições não salvas e operações de IA/leitura em andamento impedem exclusões incompatíveis, evitando perda de trabalho.
- Reimportar não ressuscita silenciosamente um documento ou captura excluído. Restaure o item explicitamente antes de importá-lo novamente.
- A confirmação é validada novamente dentro da transação, protegendo contra mudanças feitas por outra janela enquanto o diálogo está aberto.
- Janelas antigas não podem gravar sobre o esquema novo. Textos em edição nessas janelas continuam disponíveis para consulta e cópia.

## Verificação e instalação

39 testes Rust passaram: 36 de biblioteca e três de transporte nativo. Um teste opcional de IA não foi executado nesse conjunto. Os testes cobriram exclusão/restauração, dependências entre documentos, captura atual, fontes de minutas/análises, migração de referências e mudanças concorrentes.

O teste do executável distribuído passou em banco fictício isolado, incluindo importação nativa, repetição idempotente, PDF, proteção do conteúdo e schema 8. A interface foi conferida com documentos fictícios: confirmação do documento e indicação da captura substituta. As confirmações foram canceladas.

Antes da migração do acervo real foi criado o backup consistente:

`%LOCALAPPDATA%\PonteProj\backups\pre-v080-20261103-094220-499890.sqlite3`.

A conferência após a migração confirmou integridade, vínculos e preservação dos BLOBs anteriores de capturas, notas, minutas, análises, tarefas e contexto. Permanecem um documento, oito capturas e uma tarefa. Nenhum item real foi excluído durante a verificação.

Executável instalado em `%LOCALAPPDATA%\Programs\PonteProj\v0.8.0\ponteproj.exe`.

SHA256: `E7614E2BB1F826E519A4F826EE2B755A052084E2E1CF09437883FA603623C663`.

## Proposta do controle SAIOP

`PROPOSTA_CONTROLE_SAIOP.md` descreve a análise da aba PROCESSOS e a evolução recomendada para atendimentos do gabinete, responsáveis, retorno e importação assistida. Essa integração é uma proposta, ainda não implementada nesta versão. A planilha original não foi alterada nem importada para o acervo.
