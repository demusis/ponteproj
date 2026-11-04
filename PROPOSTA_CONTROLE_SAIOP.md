# Controle do gabinete do SAIOP no Ponte

Proposta de evolução baseada na aba `PROCESSOS` do arquivo fornecido, examinada em 03/11/2026. A planilha não foi alterada nem seus registros importados para o acervo.

## O que o controle já representa

A aba combina protocolo de entrada, distribuição a responsáveis, acompanhamento de retorno, produção de resposta e registro de saída. Isso complementa a captura documental do SIGADOC e deve se tornar uma área operacional do Ponte chamada **Processos e demandas**.

Foram encontrados 3.643 registros de trabalho, nas linhas 2 a 3.644. Há 3.642 identificadores preenchidos e 2.554 valores distintos após retirar espaços nas extremidades e uniformizar maiúsculas. Essa contagem não equivale a uma validação dos números oficiais. Dos identificadores repetidos, 853 aparecem com datas de entrada diferentes. Portanto, uma linha precisa ser preservada como um atendimento/passagem pelo gabinete, sem ser descartada como duplicata pelo número.

A coluna de número contém processos, ofícios, comunicações e códigos provisórios. A aplicação deve admitir essas espécies, além de demandas que ainda não tenham número oficial.

## Modelo recomendado

**Processo ou referência documental:** identifica o assunto acompanhado e reúne os documentos, capturas, arquivos, vínculos e atendimentos associados. Um processo pode reunir vários documentos. Uma demanda pode começar vinculada apenas a um documento ou referência provisória.

**Atendimento do gabinete:** representa uma entrada ou reentrada, com data, responsável, origem, situação local, destino, prazo de retorno, observações e encerramento. Várias linhas com o mesmo número podem representar atendimentos legítimos distintos. Se a linha não permitir concluir a relação entre elas, a importação deve mantê-las separadas e sinalizar a associação para revisão.

**Tarefa:** representa uma ação concreta dentro do atendimento, por exemplo examinar os anexos, pedir informação, preparar despacho, revisar minuta ou conferir o retorno. Um atendimento pode ter várias tarefas, responsáveis e prazos. A conclusão de uma tarefa não encerra automaticamente o atendimento.

**Evento:** registra distribuição, alteração de responsável, solicitação de retorno, envio, recebimento, conclusão e reabertura. Preserva data do fato, data do registro, origem da informação e valores anteriores. Eventos locais e histórico capturado do SIGADOC aparecem identificados na mesma linha do tempo.

Capturas continuam sendo versões da observação de um documento. Importar novamente a página não cria uma nova entrada no gabinete nem substitui silenciosamente o controle local.

## Correspondência dos campos

| Na planilha | No Ponte | Tratamento proposto |
| --- | --- | --- |
| A — Contagem | Totais e indicadores | Calculados pela aplicação; a coluna não contém valores nos registros examinados. |
| B — Data | Entrada no gabinete | Data do atendimento, distinta da data de emissão do documento e da data da captura. |
| C — Responsável | Responsável pelo atendimento | Cadastro local com nomes/aliases revisados e histórico de redistribuição. Não equivale ao detentor da via no SIGADOC. |
| D — Nº processo SIGADOC | Referência principal | Guardar o valor original e o identificador validado. Aceitar processo, documento e referência provisória. |
| E, F e G — Link e composição do endereço | Abrir no SIGADOC | Um único comando. Preferir endereço validado da captura; a composição a partir do código é alternativa para itens ainda não capturados. |
| H — Aguardando | Tempo de acompanhamento | Calcular idade do atendimento e tempo na etapa atual, com critério de encerramento explícito. |
| I — Recebido de | Origem da entrada | Unidade/pessoa informada pelo gabinete, com procedência. Não substituir o remetente oficial por inferência. |
| J — Retorno para UAS? | Retorno solicitado | Sim, não ou não informado, com unidade e data próprias. Existem datas nessa coluna, que exigem revisão. |
| K — Prazo de retorno | Prazo do atendimento ou tarefa | Data, motivo, fonte e natureza: prazo oficial, meta interna ou cobrança de retorno. Texto ambíguo permanece pendente de confirmação. |
| L — Observação | Observações e eventos | Preservar a anotação original e permitir novos registros datados, sem sobrescrever o histórico. |
| M — Verificação | Situação, prioridade e marcadores | Desmembrar as dimensões hoje misturadas. Preservar também o rótulo original da planilha. |
| N — Assinado? | Assinatura informada pelo gabinete | Sim, não ou não informado, separado da marca de assinatura capturada no SIGADOC. Campo vazio não significa não assinado. |
| O — Data de trâmite | Saída registrada pelo gabinete | Data e destino do encaminhamento, com evidência opcional. Não declara um ato oficial verificado. |
| P — Tipo de documento a ser produzido | Entrega prevista | Despacho, CI, ofício etc. Aproveitar a lista de opções, embora a coluna esteja vazia nos registros examinados. Associar ao modelo de minuta. |
| Q — Enviado para | Destino da saída | Unidade/pessoa e evento de envio. Permitir retorno e nova distribuição posteriormente. |
| R — Assunto | Assunto de acompanhamento | Texto original pesquisável, resumo local e etiquetas temáticas opcionais. |
| S — Data | Campo a esclarecer | Está vazio nos registros examinados. Não atribuir significado nem criar data automática durante a migração. |

## Situações e prioridade

A lista de `Verificação` prevê situações como Tramitado, Arquivado, Em andamento, Pendente, Em deliberação, Apensado, Urgente, Em verificação, Agendado, Com prazo, Documento sensível, Aguardando resposta, Stand By e Juntado. Há também regras de cor com outros rótulos. Não é adequado transformar todos em uma única enumeração de status.

Proponho quatro dimensões independentes:

- **Situação do atendimento:** a distribuir, em análise, em elaboração, aguardando deliberação, aguardando resposta, suspenso, encerrado ou cancelado. O mapeamento inicial da planilha será apresentado para revisão.
- **Prioridade:** baixa, normal, alta ou urgente, com justificativa. O prazo vencido é um alerta calculado, separado da prioridade escolhida.
- **Condições e relações:** retorno necessário, agendamento, documento sensível e vínculo informado como apensamento/juntada. Relações vindas da planilha permanecem declarações locais até conferência da fonte oficial.
- **Resultado da etapa:** encaminhado, arquivado no controle local, respondido, ciência registrada pelo usuário etc., com data, destino e evidência quando disponível.

“Tramitado” pode significar saída do gabinete e ainda exigir acompanhamento de retorno. O assistente de importação deve oferecer essa escolha. “Arquivado” na planilha também não equivale automaticamente a arquivamento oficial no SIGADOC. Não encerrar atendimentos apenas por haver uma data de saída, nem por todas as tarefas estarem concluídas.

## Uso diário

A tela **Processos e demandas** oferece uma grade semelhante à planilha, com edição de campos operacionais, pesquisa e filtros por responsável, origem, destino, situação, assunto e período. Colunas iniciais: referência, assunto, responsável, entrada, situação, próxima ação, prazo, tempo de espera e destino. Campos adicionais ficam configuráveis.

As filas úteis são: minhas pendências, sem responsável, prazo vencido, vence hoje, vence nos próximos dias, aguardando resposta, retorno para UAS, aguardando assinatura e sem atualização há determinado período. O intervalo deve ser configurável. Itens sem prazo aparecem explicitamente como sem prazo.

Ao abrir uma linha, a ficha mostra os atendimentos anteriores, documentos capturados, PDFs, histórico oficial, notas locais e tarefas. Comandos: distribuir, criar tarefa, solicitar retorno, preparar documento, registrar saída, encerrar e reabrir.

Um fluxo típico:

1. Registrar uma entrada manualmente, importar uma linha da planilha ou associar uma captura do SIGADOC a um atendimento.
2. Conferir assunto, responsável, prazo e providência. Criar somente as tarefas necessárias.
3. Ler documentos, anexos e antecedentes no acervo e, opcionalmente, solicitar análise à IA.
4. Preparar e revisar a minuta com o tipo de documento previsto.
5. O usuário assina e tramita no SIGADOC. No Ponte, registra a saída e associa a evidência disponível.
6. Se houver retorno esperado, manter o acompanhamento ativo com responsável e data de cobrança.

## Prazos e indicadores

O cálculo atual de `Aguardando` usa `DATEDIF(Data,TODAY(),"d")`. Foram encontradas 1.714 fórmulas nessa coluna para 3.643 registros. Essa fórmula continua contando até hoje mesmo quando o registro está marcado como Tramitado. Quatro fórmulas também fazem referência a linha diferente da própria. O Ponte deve calcular diretamente a partir dos eventos, sem depender do valor em cache da planilha.

- **Idade do atendimento:** dias corridos da entrada até o encerramento confirmado, ou até hoje enquanto estiver aberto.
- **Tempo na etapa:** dias desde a última mudança de etapa, sem reiniciar a idade total.
- **Espera externa:** período desde o envio que exige retorno até o recebimento correspondente, ou até hoje.
- **Atraso:** diferença para um prazo confirmado enquanto a obrigação correspondente permanece pendente.

Dias úteis só devem ser calculados com regra de contagem e calendário de feriados definidos. Prazo oficial e meta interna precisam aparecer separados. Uma saída pode cumprir uma tarefa e manter outra obrigação de retorno.

O painel do gabinete pode mostrar entradas/saídas por período, atendimentos abertos por responsável, prazos e espera externa. Contagem de atendimentos e contagem de referências distintas são medidas diferentes e devem aparecer identificadas. Não calcular produtividade individual apenas pelo número de linhas, pois reentradas e complexidade diferem.

## Importação da planilha

O primeiro importador deve trabalhar exclusivamente com a aba escolhida, inicialmente `PROCESSOS`, sem trazer os demais cadastros do arquivo.

1. **Prévia:** reconhecer cabeçalhos e mostrar quantas linhas serão importadas, ignoradas ou precisarão de revisão. Linhas contendo somente fórmulas preparadas não são novos atendimentos.
2. **Conferência:** revisar datas inválidas, números ausentes, aliases de responsáveis, rótulos e campos ambíguos. Preservar o valor original. Não corrigir anos ou interpretar “PRAZO DIA 18” sem confirmação.
3. **Associação:** sugerir vínculos com o acervo existente e agrupar referências exatas validadas. Não unir automaticamente códigos com sufixos, vias ou referências provisórias ambíguas. Repetições de número preservam os atendimentos e suas linhas de origem.
4. **Confirmação:** apresentar o efeito concreto antes de gravar. A planilha permanece intacta.
5. **Rastreabilidade:** guardar identificação do lote, hash do arquivo, aba, linha de origem e cópia protegida dos campos importados. Conservar o registro bruto separado do valor revisado.
6. **Reimportação:** um arquivo idêntico não duplica registros. Uma versão alterada exige conciliação: a linha pode ter mudado de posição e não é uma chave estável. Alterações locais não são sobrescritas e exclusões na planilha não removem dados automaticamente.
7. **Reversão:** permitir desfazer o lote preservando alterações posteriores, vínculos e tarefas já utilizados, com prévia dos conflitos e restauração pela lixeira.

Há indícios concretos de revisão necessária: datas de entrada em 2029 e 2062, entradas em texto com ano incompleto, valores textuais ou numéricos em prazo, datas na coluna de retorno Sim/Não e datas de saída anteriores à entrada. Esses casos devem ser sinalizados, sem decidir automaticamente que são erros nem apagar a informação original.

Na coluna Assinado há 799 valores “Sim”; os demais registros estão vazios. A importação não deve converter esses vazios em “Não”. Os 101 campos preenchidos de prazo aparecem em registros marcados Tramitado ou Arquivado, portanto não constituem uma lista confiável de obrigações atualmente abertas.

## IA opcional

A análise pode usar conteúdo, anexos, antecedentes selecionados e contexto do atendimento para sugerir assunto, prioridade justificada, responsável/unidade, próxima ação, tipo de minuta, retorno necessário e prazo identificado no documento.

Cada sugestão deve indicar a fonte e o trecho que a sustenta. Prazos calculados devem mostrar a regra. A IA pode apontar lacunas, mas não inventar datas, pessoas ou atos oficiais. O usuário escolhe quais sugestões aplicar. A análise não assina, tramita, redistribui nem conclui registros automaticamente.

## Evolução da implementação Rust

O Ponte já dispõe de documentos, capturas, tarefas, prioridades, prazos, contexto local, vínculos, minutas e evidências. A principal ampliação é introduzir atendimentos independentes de uma captura e responsáveis de trabalho.

Estruturas propostas: referências acompanhadas; atendimentos; vínculo atendimento/documento; vínculo atendimento/tarefa; eventos do atendimento; responsáveis e aliases; lotes e linhas de importação. Tarefas existentes continuam funcionando e recebem associação opcional a um atendimento. Não devem depender de uma captura artificial criada para acomodar uma linha do Excel.

Manter conteúdo protegido no armazenamento local, transações para importar e reconciliar lotes, migrações versionadas e lixeira para remoções. Implementar o importador `.xlsx` em Rust com leitura de valores e datas, preservando a origem das fórmulas relevantes sem executá-las como comandos. O endereço composto em E/F/G passa a ser um recurso da aplicação.

O usuário confirmou que o controle é utilizado por várias pessoas em computadores diferentes e que todos acessam a mesma conta no SIGADOC. Portanto, a colaboração passa a integrar a primeira etapa da arquitetura. O cadastro de responsáveis importado da planilha precisa ser associado às pessoas, sem criar automaticamente contas ou conceder acesso.

## Uso por vários usuários na intranet (sem nuvem ou VM)

Considerando que todos os usuários utilizam Windows na mesma intranet e que não há disponibilidade inicial de nuvem ou máquina virtual dedicada, a arquitetura deve ser **simples, autocontida no próprio `ponteproj.exe` e hospedada diretamente no computador de um dos usuários da equipe**:

- **Executável único (`ponteproj.exe`), sem instalar SGBD externo:** em vez de exigir PostgreSQL, VM ou infraestrutura de TI, um dos computadores Windows da equipe atua como **Anfitrião do Gabinete** (podendo ser usado normalmente pelo próprio usuário enquanto hospeda o serviço em segundo plano/janela).
- **SQLite local no computador anfitrião + API HTTP na intranet:** o banco `acervo.sqlite3` (em modo WAL) permanece exclusivamente no disco local do computador anfitrião (`%LOCALAPPDATA%\PonteProj`), evitando qualquer uso de pasta compartilhada SMB, OneDrive ou Google Drive. O processo do `ponteproj.exe` no anfitrião atende os demais computadores da intranet via HTTP local autenticado (`http://<IP-do-anfitrião>:<porta>`), serializando as gravações em transações imediatas (`IMMEDIATE`) do SQLite.
- **Demais computadores como clientes leves:** nos outros computadores Windows, o mesmo `ponteproj.exe` é configurado no modo **Conectar ao Gabinete na Intranet**, informando apenas o endereço (`IP:porta`), o nome/identificador do integrante e o token de acesso compartilhado da equipe.

O gabinete terá um espaço de trabalho comum. Quando um integrante registrar uma entrada, distribuir uma tarefa ou confirmar um prazo, os demais integrantes conectados receberão a atualização. O aplicativo apresentará a situação da conexão com o computador anfitrião e o horário da última sincronização.

### Identificação das pessoas

A conta comum do SIGADOC e a identidade da pessoa no Ponte têm funções diferentes. Cada integrante cadastra ou seleciona sua identidade individual no Ponte (associada a um token/credencial local do gabinete no anfitrião). Assim, o histórico mostrará quem importou, atribuiu, editou, concluiu ou restaurou cada registro, mesmo que todos utilizem a mesma conta no SIGADOC.

O histórico importado do SIGADOC preserva o que a fonte informa. Não atribuir uma assinatura ou tramitação oficial a uma pessoa apenas porque ela estava conectada ao Ponte ou fez uma captura. A autoria das alterações locais é rastreada no Ponte; autoria e efeitos oficiais dependem da evidência do SIGADOC.

Cada navegador continuará acessando a conta SIGADOC com sua sessão autenticada. A extensão captura os dados nessa sessão e os entrega ao `ponteproj.exe` local da máquina do usuário via Native Messaging, que por sua vez envia ao computador anfitrião na intranet. O anfitrião não precisa armazenar nem distribuir a senha ou os cookies do SIGADOC. Assinatura e tramitação continuam realizadas pelo usuário no sistema oficial.

### Acervo comum, proteção DPAPI e contingência entre PCs

Atendimentos, tarefas de equipe, responsáveis, prazos, eventos e os documentos capturados ficam concentrados no banco do computador anfitrião. Rascunhos pessoais e notas privadas podem permanecer no banco local da estação do usuário até que ele decida compartilhá-los com o gabinete.

Como o armazenamento do Ponte utiliza **DPAPI do Windows** vinculada ao usuário da máquina onde o banco reside:

1. **Em operação normal:** o processo `ponteproj.exe` rodando no computador anfitrião realiza toda a criptografia (`seal`) e descriptografia (`unseal`) em memória no próprio perfil Windows do anfitrião antes de responder aos clientes da intranet autenticados pelo token do gabinete.
2. **Troca de computador anfitrião (contingência):** como um computador de usuário pode ser desligado, trocado ou entrar em manutenção, o anfitrião contará com uma função de **Exportar pacote de transferência do gabinete** (protegido por chave/senha do gabinete) e backup automático periódico, permitindo que outro computador Windows da equipe assuma o papel de anfitrião na intranet em poucos minutos caso o PC principal esteja indisponível.

### Alterações simultâneas

Cada registro compartilhado terá um número de versão incremental (`version`). Exemplo: duas pessoas abrem um atendimento ou tarefa na versão 5; a primeira salva no anfitrião e gera a versão 6. Se a segunda tentar salvar a partir da versão 5, o anfitrião recusa a sobrescrita cega, devolve o estado atual (versão 6) e o Ponte permite revisar a edição antes de gravar. Não haverá substituição silenciosa de prazo, responsável, conclusão ou texto.

O anfitrião grava a alteração e seu evento de histórico na mesma transação SQLite. Repetições de envio após oscilação na rede local usam identificadores de operação (`request_id` / `operation_id`) para evitar duplicidade.

### Conexão, hospedagem na estação do usuário e operação

- **Cache local para consulta quando o PC anfitrião estiver desligado:** cada estação cliente mantém um cache local somente-leitura (protegido pelo DPAPI da própria estação) com a última fotografia recebida da grade de processos, tarefas e documentos consultados, além de permitir anotar rascunhos locais. Se o computador anfitrião estiver desligado ou inacessível na intranet, o usuário continua conseguindo consultar prazos e documentos já sincronizados, com aviso claro da data/hora da última atualização.
- **Sem pasta de rede para o banco SQLite:** o arquivo `.sqlite3` ativo nunca fica em pasta compartilhada do Windows (`\\servidor\pasta`), OneDrive ou Google Drive. Toda comunicação entre os PCs Windows ocorre via protocolo HTTP/JSON direto com o processo `ponteproj.exe` do computador anfitrião ([Orientação do SQLite sobre acesso em rede](https://www.sqlite.org/useovernet.html)).
- **Interface web opcional no próprio anfitrião:** como o anfitrião já responde por HTTP na intranet, o mesmo serviço embutido no `ponteproj.exe` pode opcionalmente servir uma página web leve de consulta da grade e tarefas para leitura rápida na rede interna, se desejado futuramente.

## Ordem de entrega

1. **Núcleo operacional do gabinete (Processos e demandas):** modelo de dados no SQLite (referências acompanhadas, atendimentos/passagens pelo gabinete, responsáveis e aliases, eventos, prazos, retorno para UAS e vínculo com documentos/tarefas existentes) + tela de grade e ficha de atendimento no aplicativo.
2. **Migração assistida da planilha (`PROCESSOS`):** leitura da aba `.xlsx` em Rust, prévia, conferência de inconsistências (datas, prazos textuais, reentradas), associação com documentos do acervo, importação idempotente com rastreabilidade de lote e reversão.
3. **Compartilhamento simples na intranet (Anfitrião + Clientes Windows):** servidor HTTP leve embutido no próprio `ponteproj.exe` para rodar no PC de um usuário, autenticação por integrante/token do gabinete, controle de versão otimista contra edição concorrente, sincronização da extensão via cliente local e pacote de contingência para troca de PC anfitrião.
4. **Acompanhamento e assistência:** filas operacionais, indicadores calculados por eventos, cobrança de retorno, exportação para Excel e sugestões opcionais da IA ligadas ao atendimento.

Critérios essenciais: preservar todas as linhas aceitas e sua origem; não perder reentradas; não duplicar o mesmo lote; não sobrescrever revisão humana; distinguir desconhecido de não; distinguir situação local de oficial; calcular espera e atraso a partir dos eventos corretos; manter assinatura e tramitação a cargo do usuário.
