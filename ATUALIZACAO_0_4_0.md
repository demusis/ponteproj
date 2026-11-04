# Versão 0.4.0 — proveniência e providências

A ficha distingue três tipos de informação:

1. **Capturado do SIGADOC:** documento e via, processo vinculado quando explícito, De/origem, remetente quando rotulado, destinatário, detentor da via, data documental, envio/recebimento quando presentes, situação da via, marca/pessoa da assinatura. Espécie, modelo, cadastrante, suporte, classificação e nível de acesso permanecem nos metadados da captura.
2. **Informado pelo usuário:** finalidade (incluindo Ciência e download), remetente, data de recebimento, processo vinculado e observações de contexto. São registros locais protegidos por DPAPI, preservados nas reimportações e disponíveis na busca.
3. **Sugerido pela IA:** classificação, prioridade, prazo textual, providências com motivo e base indicada, além de informações ausentes. Nada é assinado, enviado ou tramitado pelo aplicativo.

## Fluxo

Na ficha, revise a seção **Proveniência e situação no SIGADOC**. Os dados representam a captura, não uma consulta em tempo real. Campo ausente aparece como **Não identificado nesta captura**.

Em **Contexto de trabalho**, selecione a finalidade, complemente os campos e clique em **Salvar contexto**. Para documentos recebidos apenas para conhecimento, selecione **Ciência e download** e registre que não foi solicitada resposta, quando isso estiver confirmado.

Opcionalmente clique em **Analisar contexto e sugerir providências**. O botão informa o provedor/modelo; envia a captura (inclusive texto extraído dos PDFs), o contexto local e as notas salvas. A IA não é acionada na importação. Salve edições antes da análise.

Cada providência sugerida tem justificativa e base indicada. **Preparar tarefa** abre um rascunho local para revisar e salvar. Não cria a tarefa automaticamente, não assume prazo e não substitui uma tarefa que esteja em edição. As ações disponíveis agora incluem tomar ciência, baixar documento e acompanhar, além de examinar, responder, encaminhar e pedir complementação.

Mudanças na captura, no contexto ou nas notas tornam a análise anterior desatualizada. A análise guarda o contexto utilizado; sugestões antigas não podem preparar tarefas até serem atualizadas. Ciência/conclusão registradas localmente não constituem ciência ou tramitação formal no SIGADOC.

## Limites da captura

O campo **De** pode representar autoria/origem; não se presume que seja o remetente da última tramitação. A data documental não é tratada como data de recebimento. A marca de assinatura é transcrita da ficha, sem validação criptográfica. Situação de via e assinatura são apresentadas separadamente. Histórico de movimentações, datas e processos não presentes na ficha não são inventados nem buscados por rotas presumidas. Podem ser complementados pelo usuário, identificados como informação local.

A análise considera o conteúdo importado e o contexto salvo. Não consulta automaticamente outros processos, documentos semelhantes, antecedentes, normas ou histórico remoto. As sugestões são revisáveis e podem estar incorretas.

## Atualização

Aplicativo 0.4.0; extensão 0.1.5; adaptador 1.4.1; banco schema 4. O instalador usa pasta de versão para preservar janelas anteriores com edições não salvas. Recarregue a extensão, atualize a ficha e reimporte para obter os novos campos. O contexto local e as tarefas anteriores são preservados.

## Verificação nesta instalação

Em 02/11/2026: 24 testes Rust e 22 testes JavaScript/DOM passaram. O teste adicional com Ollama usou um caso fictício de ciência e download e retornou duas providências válidas, sem inventar prazo ou obrigação de resposta. O protocolo nativo instalado, PDF local e proteção do conteúdo foram verificados.

A migração preservou os registros criptografados anteriores, a integridade e os vínculos: um documento, cinco capturas e uma tarefa no momento da conferência. O contexto local do exemplo está salvo com finalidade Ciência e download. A ficha real confirmou origem, data documental, detentor e marca de assinatura; remetente da última tramitação, recebimento e processo não estavam explícitos e aparecem como ausentes. A extensão 0.1.5 corrige também a situação da via pela marca visível na tabela; foi recarregada pelo usuário e a captura 6 confirmou na interface Via A — Aguardando Andamento, mantendo Ciência e download e os 5597 caracteres de texto.

Backup anterior à migração: `%LOCALAPPDATA%\PonteProj\backups\pre-v040-20261102-150015-302466.sqlite3`.

SHA256 do executável instalado: `1F249C5E5B0B4428EE426DFCA7D4C51870D804BB2FF7D7098A6498DB9790196F`.
