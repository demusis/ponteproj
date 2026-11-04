# Versão 0.5.0 — triagem e painel de trabalho

Primeira etapa da evolução aprovada em 02/11/2026. Aplicativo 0.5.0, schema 5. A extensão continua 0.1.5: não requer recarga para esta etapa.

## Recursos entregues

- Novas importações criam **Triar documento**. Tarefas existentes preservam título, ação, situação, prioridade e prazo.
- No contexto da ficha, informe assuntos (separados por ponto e vírgula), tipo documental, prioridade e motivo. A finalidade permanece separada do assunto. Clique em **Salvar contexto**.
- A classificação local confirmada prevalece na apresentação da biblioteca; a análise original da IA permanece registrada. Alterar o contexto torna análises anteriores desatualizadas.
- No painel: Todas, A triar, Hoje, Próximos 7 dias, Atrasadas, Aguardando terceiros, Lembretes e Sem prazo. A triar corresponde a documentos com finalidade não definida. Hoje usa vencimento na data atual; atrasos têm filtro próprio.
- A busca de tarefas inclui assunto, tipo, finalidade, processo e remetente registrados no contexto local, além de tarefa e documento.
- No editor de tarefa: origem do prazo confirmado, data do lembrete local, checklist e resultado local. Datas usam AAAA-MM-DD; datas impossíveis são recusadas.
- Botões explícitos permitem aplicar à tarefa a finalidade e a prioridade confirmadas no contexto. Ciência e download corresponde a tomar ciência; crie outra tarefa ou uma etapa de checklist para o download se necessário. Nada é alterado automaticamente em tarefas anteriores.
- Lembretes aparecem na visão Lembretes a partir da data escolhida, enquanto o aplicativo está aberto. Não são notificações do Windows nem alarmes em segundo plano.
- O resultado local permite registrar número da resposta e data de envio; a tramitação oficial permanece no SIGADOC. A conclusão da tarefa e os itens do checklist são independentes.

## Verificação e instalação

24 testes automatizados passaram, incluindo persistência dos novos campos, reimportação, classificação confirmada, busca e rejeição de datas inválidas. O teste opcional com IA real não foi acionado nesta etapa.

Backup anterior à migração: `%LOCALAPPDATA%\PonteProj\backups\pre-v050-20261102-203316-816378.sqlite3`.

O instalador usa `v0.5.0` e preserva os executáveis anteriores. Após migrar o acervo, use a versão nova; a versão 0.4.0 recusa schema 5.

## Próximas etapas aprovadas

1. Histórico de movimentações: inspecionar e validar a tela real do SIGADOC; capturar somente informações explícitas, com origem e data. Acrescentar comparação entre capturas.
2. Dossiê por processo: agrupamento por vínculo confirmado, árvore de arquivos e leitor PDF integrado. OCR opcional para documentos digitalizados, com indicação da origem do texto.
3. Análise com evidências: segmentar texto por arquivo/página, permitir seleção de antecedentes e apresentar trechos verificáveis para cada conclusão. Regras pessoais e correções confirmadas devem orientar sugestões revisáveis.
4. Minutas: modelos próprios, comparação de versões, exportação formatada, antecedentes selecionados e vínculo com a resposta oficial. Dependências entre tarefas complementam o checklist desta entrega.

Essas próximas etapas ainda não estão implementadas nesta versão. A lista completa de propostas foi mantida como escopo aprovado, sem confundir entrega com planejamento.

A migração real preservou seis capturas, uma tarefa e o contexto existente. A interface confirmou os novos filtros e a tarefa anterior com situação, prioridade e vencimento preservados.

SHA256 instalado: `E9B25DB745AC20A19211A5DFF822AF5AF04D0516F813A200445B7ACE83A30A62`.
