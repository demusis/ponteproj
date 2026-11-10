# Ponte — Guia do usuário

O Ponte guarda, no seu computador, os documentos que você importa do SIGADOC, com texto, PDFs e histórico de tramitação. Com eles guardados, você anota, prepara minutas, acompanha prazos e controla as demandas do gabinete. O Ponte **nunca envia nada ao SIGADOC**: assinar e tramitar continuam sendo feitos lá.

Este é o guia curto. A documentação completa, com exemplos e todas as opções, está em <https://github.com/demusis/ponteproj>.

## 1. Instalar

1. Baixe o instalador: <https://github.com/demusis/ponteproj/releases/latest/download/Ponte-Setup.exe>. O link sempre traz a versão mais recente.
2. Dê dois cliques em **Ponte-Setup.exe** e avance até o fim. Não precisa de administrador.
3. Feche e abra o navegador (Chrome ou Edge).
4. Instale a extensão, uma única vez:
   - **Pela loja (recomendado):** na tela inicial do Ponte, no card **Primeiros passos**, clique em **Abrir a loja de extensões** (ou abra <https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah>) e clique em **Adicionar ao Chrome**. No Edge, permita antes "extensões de outras lojas".
   - **Sem acesso à loja:** no mesmo passo, abra **Instalar pela pasta (alternativa)** e clique em **Copiar caminho**. No navegador, abra `chrome://extensions`, ative o **Modo do desenvolvedor**, clique em **Carregar sem compactação** e cole o caminho. **Abrir pasta** mostra a pasta no Explorador.
5. Feche e abra o navegador de novo.

**Deu certo quando:** na tela inicial do Ponte, **Ponte ligado ao navegador** aparece marcado; o ícone da extensão diz *✓ Ligado ao Ponte*; e, numa ficha de documento do SIGADOC, aparece o painel **Importar para o assistente**. O passo **Extensão instalada no Chrome ou no Edge** só é marcado depois que o primeiro documento chega por ela.

**Atualizar:** feche o Ponte e rode o instalador novo. Seus dados são mantidos, e o instalador faz uma cópia de segurança antes. Para saber se há versão nova, use **Ajustes ▸ Sobre o Ponte ▸ Verificar se há versão nova** (veja a seção 10).

**Desinstalar:** em *Aplicativos instalados* do Windows. Seus dados não são apagados.

Os dados ficam só na sua conta do Windows, cifrados. Outra conta no mesmo computador não os vê e instala o Ponte para si.

## 2. A janela em um minuto

- **Barra lateral (esquerda):** no alto, **Buscar** (Ctrl+K) procura em todo o acervo; veja **Buscar em todo o acervo**, abaixo. Em seguida, escolhe a área. **Hoje** (Ctrl+1) é a tela em que o Ponte abre (com o acervo vazio, abre em Documentos, com o card **Primeiros passos**). **Documentos** (Ctrl+2) tem a lista de documentos e a ficha do selecionado. **Tarefas** (Ctrl+3) é a lista de tarefas. **Atendimentos** (Ctrl+4) é o controle do gabinete. **Modelos** (Ctrl+5) guarda os modelos de trabalho. Um número vermelho em Hoje (tarefas atrasadas ou para hoje, mais atendimentos com prazo vencido ou para hoje) e em Tarefas (só as tarefas) avisa o que pede atenção. Embaixo ficam **Ajustes** e **Ajuda** (abre este guia).
- **Atalhos:** **Ctrl+K** abre a busca em todo o acervo, de qualquer área. **Ctrl+F** vai para a busca da área (de outras áreas, para a busca de documentos). **Ctrl+N** cria: registro manual em Hoje e Documentos, nova tarefa em Tarefas (do documento selecionado), nova entrada em Atendimentos. **Ctrl+S** salva o que está em edição (tarefa, atendimento, notas do documento, nova versão da minuta, modelo, configuração de IA). Em Modelos, **Ctrl+F** (busca), **Ctrl+N** (novo modelo), **Ctrl+S** (salvar rascunho) e **Esc** também funcionam. **Esc** fecha a janela ou o painel lateral aberto, se nada estiver sem salvar.
- **Tarefas e Atendimentos abrem com tudo**, sem seguir o documento selecionado em Documentos. Para ver só os de um documento: em Tarefas, **Ver todas** no card Tarefas da ficha ou **Só as de <número>** ao fim dos botões de visão; em Atendimentos, **Só os de <número>** dentro de **Filtros**. Uma faixa avisa que a lista está restrita e traz **Ver todas** / **Ver todos**.
- **Voltar:** ao ir de uma tarefa, de um atendimento ou de Hoje para um documento, a ficha mostra no alto **Voltar à tarefa**, **Voltar ao atendimento** ou **Voltar a Hoje**.
- **Barra inferior:** um ponto colorido mostra a situação da rede, e **Sincronizar** aparece no modo pasta compartilhada. Clicar na situação abre Ajustes ▸ Equipe e sincronização.
- **Avisos:** aparecem numa caixa no canto inferior direito. Os de sucesso somem sozinhos; os de erro ficam até você fechar.
- **Ajustes:** Equipe e sincronização, E-mail, Importação do SIGADOC, Inteligência artificial, Cópia de segurança e exportação, Lixeira, Aparência (tema Como o Windows, Claro ou Escuro, e **Tamanho do texto**: Normal, Grande ou Maior) e Sobre o Ponte.
- **Datas:** digite `dd/mm/aaaa` ou clique no ícone de calendário; **Hoje** preenche a data de hoje.
- **Nada se perde ao trocar de tela:** se há algo não salvo, o Ponte pergunta antes.

**Buscar em todo o acervo.** Clique em **Buscar**, no alto da barra lateral, ou aperte **Ctrl+K**. Digite pelo menos duas letras: o Ponte procura em documentos (número, assunto, texto, notas e PDFs), tarefas e atendimentos, e mostra os resultados em três grupos, **Documentos**, **Tarefas** e **Atendimentos**, com até 8 de cada. Se há mais, a linha "e mais N" avisa; refine o que digitou. Nas tarefas e nos atendimentos, a busca não diferencia maiúsculas de minúsculas nem acentos. As setas do teclado trocam de resultado, **Enter** abre o escolhido e **Esc** fecha a busca. Também se abre um resultado com o clique. O Ponte leva direto ao documento, à tarefa ou ao atendimento.

## 3. Importar um documento

1. No SIGADOC, abra o documento (a ficha dele, não a lista).
2. No canto inferior direito da página, clique em **Importar para o assistente**.
3. A mensagem *Importado* aparece e o Ponte abre o documento.

O painel avisa antes do clique: **Já está no Ponte — importado em…** (com **Abrir no Ponte** e o botão **Importar de novo**) ou **Ainda não está no Ponte**. Se o Ponte não responde, o aviso aparece ali mesmo, com **Tentar de novo**. O atalho **Alt+Shift+I** faz o mesmo que o botão (pode ser trocado em chrome://extensions/shortcuts). Com o Ponte já aberto, a importação traz a janela existente para a frente, no documento; não abre outra.

Na primeira importação o Ponte cria três coisas: o **documento**, uma **tarefa de triagem** e um **atendimento** "Em análise".

**Importar de novo** nunca duplica. Se o documento não mudou, nada é gravado e a extensão avisa em laranja. Se mudou (despacho novo, anexo novo), o Ponte guarda uma **nova versão** e mantém as anteriores em **Mais ▸ Versões importadas**.

| Mensagem | O que fazer |
|---|---|
| "O Ponte não foi encontrado neste computador" | Abra o Ponte uma vez e reinicie o navegador. Se continuar, rode o instalador com a mesma conta do Windows. |
| "Não recebemos a confirmação do Ponte" | Clique em **Repetir importação**; nada é duplicado. Se a mensagem se repete sempre, instale a versão mais recente do Ponte e reinicie o navegador. |
| "O Ponte abriu e fechou sem responder" | Instale a versão mais recente do Ponte e reinicie o navegador. |
| "Importação ignorada: o SIGADOC mostra o mesmo conteúdo…" | Não é erro: o documento já está no Ponte assim. Se ele mudou, atualize a página (F5) e importe de novo. |
| "A página ainda não terminou de carregar" | Espere a página abrir por completo e tente de novo. |
| O botão não aparece | A página não é a ficha de um documento, ou a extensão não está instalada. Clique no ícone da extensão: ele diz o que fazer. |

## 4. A ficha do documento

A ficha tem quatro abas:

- **Resumo:** dados principais, tarefas, atendimentos ligados, análise por IA (opcional) e documentos parecidos.
- **Documento:** **Arquivos (PDF)** (primeiro, quando o documento os tem), texto importado, **Anexos**, dados do SIGADOC e histórico de tramitação.
- **Notas e minutas:** suas notas e o editor de minutas.
- **Mais:** vínculos entre documentos, versões importadas e dados técnicos.

No alto da ficha, o botão **⋯** (Mais ações) tem **Imprimir ou salvar a ficha…** e **Excluir…**, que manda o documento para a Lixeira. Se a análise por IA é mais antiga que as últimas alterações, uma faixa avisa e oferece **Analisar de novo**.

**Lista de documentos** (área Documentos). A busca procura em número, assunto, texto, notas e PDFs. Os atalhos *Todos*, *Com tarefa* e *Atrasados* filtram a lista, e **Filtros** acrescenta situação das tarefas (*No prazo*, *Concluídos*), **Prioridade**, **Origem** e **Ordem** (mais recentes, prazo, prioridade, número ou assunto). A etiqueta embaixo de cada cartão resume as tarefas do documento: vermelha é atraso, âmbar vence hoje, azul está no prazo, verde está concluído.

**Documentos juntados** aparecem abaixo do principal, com a marca ↳. O cabeçalho da ficha tem **Abrir documento principal**.

**Algo que não está no SIGADOC** (ligação, e-mail, memorando, papel): clique em **Novo ▸ Registro manual…**, preencha tipo, título, origem, data e texto, e salve. O registro ganha um número `MANUAL-ano/número` e a etiqueta **Manual**, e funciona como qualquer documento. **Editar registro…** salva uma nova versão. Um e-mail salvo como arquivo `.eml` (Gmail: Mostrar original ▸ Baixar original) entra pronto por **Novo ▸ Importar e-mail de um arquivo (.eml)…**, com os anexos; e-mails importados ganham número próprio `EMAIL-ano/número` e a etiqueta **E-mail**. Para buscar direto na conta, use **Novo ▸ Importar da conta de e-mail…**, que abre a lista da última pasta e filtro usados; na primeira vez, ou para trocar, use **Ajustes ▸ E-mail** (a página já traz as contas, a pasta e a lista de mensagens): cadastre a conta com uma senha de app (no Gmail, com a verificação em duas etapas ativa), escolha a pasta ou o marcador (por exemplo, **Com estrela**), liste e marque as mensagens a importar. O Ponte só lê a conta; nada é marcado ou apagado.

**Imprimir ou salvar a ficha.** Em **⋯ ▸ Imprimir ou salvar a ficha…**, escolha onde guardar o arquivo. O Ponte cria uma página (arquivo .html) com a ficha e a abre no navegador; de lá, **Ctrl+P ▸ Salvar como PDF** grava um PDF, ou imprima direto. A página traz os dados do documento, a análise (com o aviso de que foi feita com apoio de IA), as tarefas, os atendimentos, a nota, o texto, a lista de arquivos e o histórico de tramitação. Não traz os arquivos em si (PDFs e anexos continuam no Ponte) nem a minuta. **Atenção:** a página salva é uma cópia fora do acervo protegido. Guarde-a, ou apague-a, com o mesmo cuidado que teria com o próprio documento.

**Anexos.** Em Documento ▸ Anexos, na ficha de uma tarefa ou de um atendimento, **Anexar arquivo…** junta um arquivo do seu computador: até 20 por item, de até 8 MB cada. Aceita PDF, Word, Excel, PowerPoint, OpenDocument, texto, imagens e e-mails; recusa programas, scripts e arquivos compactados, e diz o motivo. **Arrastar:** também dá para arrastar arquivos do Explorador de Arquivos do Windows e soltá-los sobre a janela do Ponte, vários de uma vez, quando há um bloco **Anexos** na tela. Valem os mesmos limites; pastas são recusadas. Se dois blocos **Anexos** aparecem juntos, o Ponte não sabe qual escolher: use **Anexar arquivo…** do bloco desejado. **Ler no Ponte** abre PDFs, **Abrir** usa o programa padrão do Windows e **Salvar como…** grava uma cópia.

## 5. Tarefas e prazos

- **Começar o dia por Hoje:** a área reúne as **Atrasadas** e as **Para hoje** (com o círculo para concluir e um clique para abrir), os **Próximos dias**, os **Atendimentos com prazo vencido ou para hoje** e os **Documentos recentes**. Ao concluir uma tarefa, o aviso do canto traz **Desfazer** por alguns segundos.
- **Calendário de prazos:** em Hoje, os botões **Lista** e **Calendário** trocam a visão. O calendário mostra o mês com as tarefas e os atendimentos que têm prazo: vermelho é atraso, âmbar é para hoje, azul está no prazo, e os atendimentos levam um ícone. As setas trocam de mês e **Hoje** volta ao mês atual. Clique num item para abri-lo; clique em "+N" (ou num dia com itens) para listar os desse dia sob o calendário. Tarefas concluídas e atendimentos encerrados não aparecem.
- **Criar:** em Resumo ▸ **Nova tarefa**, ou na área **Tarefas**. Informe título, responsável (botão **Eu**), ação, prioridade e prazo.
- **Visões:** *Todas*, *Hoje*, *Próximos 7 dias*, *Atrasadas* e, em **Mais ▾**, *Prontas para fazer*, *A triar*, *Aguardando*, *Com lembrete* e *Sem prazo*. **Ordenar:** Prazo, Prioridade ou Documento. A lista se divide em Atrasadas, Para hoje, Próximas, Sem prazo e Concluídas; lista vazia diz o motivo. Para ver só as de um documento, use **Só as de <número>**; a faixa *Mostrando só as tarefas de <número>* tem **Ver todas**.
- **Abrir:** clique na tarefa para ver os dados num painel à direita, com **Salvar**, **Concluir**, **Cancelar** e **Excluir…**. Em **Mais detalhes** ficam origem do prazo, lembrete, resultado, checklist, dependências e anexos.
- **Concluir:** clique no círculo à esquerda da tarefa; clicar de novo reabre. Nada é registrado no SIGADOC.
- **Ler sem sair da tarefa:** **Consultar documento** abre o texto e os PDFs por cima.
- **Tarefa que depende de outra** aparece recuada, com o aviso `⛓ 1 de 2 anterior(es) em aberto` e a linha *depende de*, que lista as anteriores. A tarefa só pode ser concluída depois delas. A linha *libera* mostra quem espera por ela.
- **Juntados seguem o principal:** concluir a última tarefa de um documento principal conclui as tarefas das peças juntadas a ele. A mensagem lista o que foi concluído.

## 6. Notas, minutas e IA

- **Notas:** escreva e clique em **Salvar notas** (ou Ctrl+S). O ponto laranja na aba indica algo não salvo. Com a janela larga, as notas ficam à esquerda e a minuta à direita; com a janela estreita, a nota mostra as primeiras linhas, com **Editar nota** e **Recolher**.
- **Minuta** (em Notas e minutas), em três passos. **1. Partir de:** escolha o **Tipo** (*Resposta*, *Encaminhamento*, *Pedido de complementação*) e, se quiser, o **Modelo de trabalho**; **Começar por um modelo** preenche o texto-base, sem IA. **2. Pedir à IA (opcional).** **3. Texto:** escreva ou edite; **Salvar nova versão** guarda o texto (Ctrl+S também, com a nota já salva), **Copiar** copia, **Exportar (RTF)** cria um arquivo para o Word, **Comparar com versão ▾** mostra as diferenças e **Guardar como modelo** leva o texto para a área Modelos. O texto final você cola no SIGADOC.
- **IA (opcional):** **Analisar** (depois, **Analisar de novo**), no card Análise do Resumo, ou, no passo 2 da minuta, **Gerar com IA**, **Completar** ou **Revisar**, com o que você quer em **Pedido desta minuta**. A sugestão abre para revisão e só é salva quando você salvar.

**Ajustes ▸ Inteligência artificial** define onde a IA roda. São quatro cartões, e cada um diz se os textos saem do computador (clique em **Salvar** ou saia da seção):

| Opção | Os dados saem do computador? |
|---|---|
| **Ollama neste computador** | Não (*Nada sai deste computador*). Instale o Ollama e baixe um modelo: `ollama pull qwen3:4b-instruct`. Se o Ollama não responder, a página mostra os passos (instalar, abrir, baixar um modelo) e **Tentar de novo** |
| **Ollama Cloud** | Sim (*Envia os textos à Ollama*). Exige a chave de API da sua conta em ollama.com |
| **Outro servidor** | Sim (*Envia os textos ao servidor informado*) |
| **ChatGPT (assinatura)** | Sim (*Envia os textos à OpenAI*). Usa a cota do seu plano Plus ou Pro: clique em **Entrar com o ChatGPT…** |

Depois de escolher, clique em **Listar modelos do servidor**, selecione o modelo e salve.

**Atenção com a nuvem:** nas opções em que os dados saem do computador, o texto do documento, as notas e as fontes são enviados a cada clique em Analisar ou Gerar com IA, nunca ao importar. Use só com documentos que podem sair do órgão.

Enquanto a IA trabalha, a barra inferior mostra a etapa e o botão **Cancelar**. Documentos longos no Ollama local demoram um pouco mais na primeira vez, porque o Ponte pede ao modelo uma janela maior.

### Modelos de trabalho

Um modelo de trabalho guarda a estrutura, as instruções e os exemplos de um tipo de resposta, para você e a IA começarem do mesmo ponto. Ele fica na área **Modelos** da barra lateral, com busca (**Buscar modelo**) na lista. **Duplicar…**, **Arquivar** (ou **Desarquivar**) e **Apagar de vez…** ficam no menu **⋯**, à direita do título. É opcional.

O caminho do dia a dia:

1. **Criar:** em uma minuta já revisada, clique em **Guardar como modelo**. O texto vira o rascunho de um modelo novo. Dê um nome.
2. **Estrutura:** ajuste as seções, as variáveis e os critérios de conferência. **Salvar rascunho**.
3. **Instruções:** cadastre as regras de redação e clique em **Publicar versão** em cada uma (só instrução publicada pode ser adicionada a um modelo). Depois, na aba **Instruções** do modelo, escolha **Adicionar instrução ao modelo** e defina cada uma como *obrigatória*, *opcional* ou *condicional*.
4. **Exemplos:** cadastre respostas boas e clique em **Aprovar**. Pode anexar o arquivo da resposta real (PDF com texto, .txt ou .docx) e clicar em **Extrair texto**; se quiser, leia e corrija em **Conferir texto…**. Só exemplo aprovado, com texto extraído ou conferido, é usado. Eles ensinam organização e tom, nunca os fatos do caso.
5. **Testar:** com o documento desejado selecionado em Documentos (a aba diz qual está sendo usado), veja o que seria enviado à IA. Nada é enviado.
6. **Publicar:** **Publicar versão N**. Para mudar depois, revise: o Ponte cria a versão seguinte, e as minutas antigas continuam ligadas à versão que usaram.
7. **Usar:** em **Notas e minutas**, escolha o **Modelo de trabalho** (o Ponte pode sugerir um pelas palavras do documento; a escolha é sua), marque as **Instruções opcionais** que valem para o caso e escreva o **Pedido desta minuta**. Depois clique em **Gerar com IA**, **Completar** (preserva o que já está escrito) ou **Revisar** (abre a comparação antes de substituir).
8. **Conferir:** na lista de versões, **Material utilizado** mostra o modelo, as instruções, os exemplos e as fontes que a geração usou. Um trecho "encontrado na fonte" prova só que a citação existe, não que a conclusão está certa.

Se duas instruções se contradisserem, o Ponte avisa antes de gerar. É só um aviso: decida você.

**Em equipe:** ao publicar com o escopo **Compartilhado com a equipe**, o modelo chega a todos na próxima sincronização. Qualquer integrante pode revisar, e o histórico guarda quem fez cada versão. **Pessoal** apenas esconde o modelo dos outros na tela; não é uma proteção. Os modelos pessoais antigos viram modelos pessoais publicados, sem perda de texto. Esta atualização muda o formato do acervo: na equipe, atualize todos os computadores juntos.

## 7. Atendimentos

Um atendimento é uma **passagem** de um documento ou de uma demanda pelo gabinete: quando entrou, com quem está, em que situação, quando saiu. É o controle que muitos gabinetes fazem em planilha.

- **De onde vêm:** da importação do SIGADOC (o Ponte cria sozinho na primeira importação de cada documento), do botão **Nova entrada** ou de **Importar planilha**.
- **Desligar a criação automática:** desmarque **Criar atendimento ao importar do SIGADOC**, em **Ajustes ▸ Importação do SIGADOC**.
- **Se a criação automática falhar**, o documento é importado assim mesmo e o painel mostra um aviso com o número e o motivo. Use **Tentar de novo** ou **Dispensar** (e registre à mão, se quiser).
- **A lista** abre com todos os atendimentos e mostra as colunas que cabem: Referência, Assunto, Situação e Prazo sempre; Responsável, Entrada, Tempo e Destino / Saída conforme o espaço. Para ver só os de um documento, use **Só os de <número>** em **Filtros**; a faixa *Mostrando só os atendimentos de <número>* tem **Ver todos**. Com um documento selecionado, **Nova entrada** cria um atendimento já ligado a ele.
- **A ficha** abre num painel à direita da lista, com **Salvar**, **Encerrar**, **Descartar alterações** e **Apagar de vez…**. Só a referência ou o assunto é obrigatório.
- **Ligar a documento:** informe o número de um documento já importado. A referência passa a ser o número dele.
- **Criar tarefa** gera uma tarefa do documento ligado; por isso só fica ativo com um documento ligado.
- **Anexos e linha do tempo:** a ficha aceita arquivos seus e registra cada mudança de situação.
- **Apagar de vez** um atendimento é definitivo, com confirmação. Não há Lixeira para atendimentos.
- **Fila**, a lista suspensa no alto da área, agrupa os atendimentos (prazo vencido, vence hoje, sem responsável…). **Filtros** abre responsável, origem, destino e situação.
- **Planilha:** **Importar planilha** lê a aba PROCESSOS de um arquivo .xlsx, mostra uma prévia e importa só as linhas novas. A importação pode ser desfeita.

## 8. Mais de um computador

Em **Ajustes ▸ Equipe e sincronização**. No alto, um resumo mostra como este computador está trabalhando, a situação, a última sincronização e, nos modos que sincronizam, **Sincronizar agora**. Abaixo, quatro cartões escolhem o modo: **Só neste computador**, **Computador principal**, **Conectar ao computador principal** e **Pasta compartilhada**. Trocar de modo nunca apaga dados. Enquanto há mudanças sem salvar, aparece a etiqueta **Não salvo**.

**Você, em dois computadores (casa e trabalho).** Escolha **Pasta compartilhada**, clique em **Adicionar pasta…** e aponte para uma pasta do Google Drive, do OneDrive ou de um pendrive. Crie uma senha de pelo menos 8 caracteres e clique em **Salvar e sincronizar agora**. No outro computador, a mesma pasta e a mesma senha. O Ponte sincroniza ao abrir, a cada 5 minutos e quando você pedir. Guarde a senha: sem ela o arquivo da pasta não abre.

**Equipe do gabinete, na rede interna.** Um computador é o principal e os outros se conectam a ele. Nada vai para a internet.

1. **No principal:** escolha **Computador principal**, clique em **Salvar e ficar disponível agora** e em **Copiar código de conexão**. Envie o código aos colegas.
2. **Nos outros:** escolha **Conectar ao computador principal**, cole o código, clique em **Usar código** e em **Salvar e sincronizar acervo agora**. Se o computador já tinha documentos próprios, faça uma cópia de segurança antes: passa a valer o acervo do gabinete.

- Sem conexão com o principal, tudo continua funcionando e é enviado quando a conexão volta.
- Se o mesmo item foi alterado nos dois lados, aparece um **conflito de edição**, no card *Edições em conflito* da mesma seção. Ele mostra os dois valores, campo a campo, e você escolhe **Combinar**, **Usar só a minha versão** ou **Usar só a do computador principal**.
- Se o principal recusou algo criado sem conexão (uma tarefa de um documento que foi para a lixeira, por exemplo), as alterações seguintes desse item aparecem como aviso (card *Alterações a enviar*), só com **Dispensar o aviso**. Refaça à mão o que ainda for necessário.
- Se os colegas não conseguirem conectar, peça à TI para liberar a porta 7878 (TCP) no firewall do computador principal. A própria página, no modo principal, traz esse aviso. **Testar conexão**, nos outros computadores, mostra o resultado logo abaixo do botão.

**Nunca** instale o Ponte nem copie o acervo para dentro do Google Drive ou do OneDrive: o acervo só abre na conta do Windows que o gravou.

## 9. Cópia de segurança, lixeira e exportação

- **Cópia de segurança:** **Ajustes ▸ Cópia de segurança e exportação ▸ Fazer cópia agora** cria uma cópia do acervo na pasta de backups. A página mostra onde as cópias ficam (**Abrir pasta**) e a data da última cópia feita nesta sessão. Ela só abre na mesma conta do Windows.
- **Lixeira:** excluir documento, tarefa, anexo ou versão importada manda para a **Lixeira** (Ajustes), de onde se restaura. Logo após excluir, o aviso do canto traz **Desfazer** por alguns segundos. Por padrão nada é apagado de vez. Na seção Lixeira você pode definir um prazo, de 7 dias a 1 ano, para a limpeza automática.
- **Exportar:** **Ajustes ▸ Cópia de segurança e exportação ▸ Exportar dados para CSV…** grava documentos, tarefas e atendimentos em três arquivos para o Excel, sem textos nem anexos.
- **Ao fechar** com algo não salvo, o Ponte lista o que está pendente, com um botão **Ir para** em cada item.

## 10. Se algo der errado

| Situação | O que fazer |
|---|---|
| A extensão diz que não está ligada ao Ponte | Instale a versão mais recente do Ponte, abra-o uma vez e reinicie o navegador. |
| A IA diz que o modelo não está instalado | Em Ajustes ▸ Inteligência artificial, clique em **Listar modelos do servidor** e escolha um da lista. |
| A IA diz que o documento não coube na janela do modelo | Analise sem as fontes extras ou escolha outro provedor. |
| A cota do ChatGPT acabou | Veja o saldo em ChatGPT ▸ Configurações ▸ Uso, espere a renovação ou troque de provedor. |
| O colega não conecta ao computador principal | O principal precisa estar com o Ponte aberto, na mesma rede, com a porta 7878 liberada. |
| "Não foi possível proteger/abrir os dados deste perfil Windows" | O acervo é de outra conta do Windows. Entre com a conta que fez a instalação. |
| Excluí algo sem querer | Ajustes ▸ Lixeira ▸ **Restaurar**. |

A lista completa de mensagens e soluções está na seção "Solução de problemas" de <https://github.com/demusis/ponteproj>. A versão instalada aparece em **Ajustes ▸ Sobre o Ponte**.

**Verificar se há versão nova.** Em **Ajustes ▸ Sobre o Ponte**, clique em **Verificar se há versão nova**. Só nesse clique o Ponte consulta a página de versões no GitHub; nada do acervo é enviado, e nada é baixado nem instalado. O Ponte diz se há versão nova e, se houver, mostra **Abrir a página de download**. Feche o Ponte antes de instalar: o instalador guarda uma cópia de segurança do acervo antes de atualizar. Sem conexão com a internet, aparece um aviso e **Tentar de novo**.
