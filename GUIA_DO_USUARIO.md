# Ponte — Guia do usuário

O Ponte guarda, no seu computador, os documentos que você importa do SIGADOC, com texto, PDFs e histórico de tramitação. Com eles guardados, você anota, prepara minutas, acompanha prazos e controla as demandas do gabinete. O Ponte **nunca envia nada ao SIGADOC**: assinar e tramitar continuam sendo feitos lá.

Este é o guia curto. A documentação completa, com exemplos e todas as opções, está em <https://github.com/demusis/ponteproj>.

## 1. Instalar

1. Baixe o instalador: <https://github.com/demusis/ponteproj/releases/latest/download/Ponte-Setup.exe>. O link sempre traz a versão mais recente.
2. Dê dois cliques em **Ponte-Setup.exe** e avance até o fim. Não precisa de administrador.
3. Feche e abra o navegador (Chrome ou Edge).
4. Instale a extensão, uma única vez:
   - **Pela loja (recomendado):** abra <https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah> e clique em **Adicionar ao Chrome**. No Edge, permita antes "extensões de outras lojas".
   - **Sem acesso à loja:** na tela inicial do Ponte, clique em **Copiar caminho**. No navegador, abra `chrome://extensions`, ative o **Modo do desenvolvedor**, clique em **Carregar sem compactação** e cole o caminho.
5. Feche e abra o navegador de novo.

**Deu certo quando:** a tela inicial do Ponte diz *✓ O Ponte está registrado para o Chrome e Edge*, o ícone da extensão diz *✓ Ligado ao Ponte* e, numa ficha de documento do SIGADOC, aparece o painel **Importar para o assistente**.

**Atualizar:** feche o Ponte e rode o instalador novo. Seus dados são mantidos, e o instalador faz uma cópia de segurança antes.

**Desinstalar:** em *Aplicativos instalados* do Windows. Seus dados não são apagados.

Os dados ficam só na sua conta do Windows, cifrados. Outra conta no mesmo computador não os vê e instala o Ponte para si.

## 2. A janela em um minuto

- **Lista de documentos (esquerda):** sempre visível. O que você seleciona ali é o que os painéis mostram.
- **Cabeçalho:** os botões escolhem a área central. **Ficha** é o documento selecionado. **Tarefas** é o painel de tarefas. **Atendimentos** é o controle do gabinete. **Modelos** guarda os modelos de trabalho.
- **Barra inferior:** um ponto colorido mostra a situação da rede; ali também aparecem os avisos e o progresso das operações demoradas.
- **Menu ▾:** Rede e equipe, Configurar IA, Lixeira, backup, exportação, este guia e Sobre o Ponte.
- **Datas:** digite `dd/mm/aaaa` ou clique no 📅 para escolher no calendário.
- **Nada se perde ao trocar de tela:** se há algo não salvo, o Ponte pergunta antes.

## 3. Importar um documento

1. No SIGADOC, abra o documento (a ficha dele, não a lista).
2. No canto inferior direito da página, clique em **Importar para o assistente**.
3. A mensagem *Importado* aparece e o Ponte abre o documento.

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
- **Documento:** texto importado, PDFs, **Anexos**, dados do SIGADOC e histórico de tramitação.
- **Anotações e minutas:** suas notas e o editor de minutas.
- **Mais:** vínculos entre documentos, versões importadas e dados técnicos.

**Lista de documentos.** **Buscar** procura em número, assunto, texto, notas e PDFs. **Filtrar ▾** filtra por situação das tarefas, prioridade e origem, e define a **Ordem** (mais recentes, prazo, prioridade, número ou assunto). A etiqueta embaixo de cada cartão resume as tarefas do documento: vermelha é atraso, âmbar vence hoje, azul está no prazo, verde está concluído.

**Documentos juntados** aparecem abaixo do principal, com a marca ↳. O cabeçalho da ficha tem **Abrir documento principal**.

**Algo que não está no SIGADOC** (ligação, e-mail, memorando, papel): clique em **+ Registro manual**, preencha tipo, título, origem, data e texto, e salve. O registro ganha um número `MANUAL-ano/número` e a etiqueta **Manual**, e funciona como qualquer documento. **Editar registro…** salva uma nova versão. Um e-mail salvo como arquivo `.eml` (Gmail: Mostrar original ▸ Baixar original) entra pronto por **+ Importar e-mail (.eml)…**, com os anexos; e-mails importados ganham número próprio `EMAIL-ano/número` e a etiqueta **E-mail**. Para buscar direto na conta, o botão **✉ E-mails** (ao lado de **+ Registro manual**) abre a lista da última pasta e filtro usados; na primeira vez, ou para trocar, use **Menu ▾ ▸ Importar do e-mail (IMAP)…**: cadastre a conta com uma senha de app (no Gmail, com a verificação em duas etapas ativa), escolha a pasta ou o marcador (por exemplo, **Com estrela**), liste e marque as mensagens a importar. O Ponte só lê a conta; nada é marcado ou apagado.

**Anexos.** Em Documento ▸ Anexos, na ficha de uma tarefa ou de um atendimento, **Anexar arquivo…** junta um arquivo do seu computador: até 20 por item, de até 8 MB cada. Aceita PDF, Word, Excel, PowerPoint, OpenDocument, texto, imagens e e-mails; recusa programas, scripts e arquivos compactados, e diz o motivo. **Ler no Ponte** abre PDFs, **Abrir** usa o programa padrão do Windows e **Salvar como…** grava uma cópia.

## 5. Tarefas e prazos

- **Criar:** em Resumo ▸ **Nova tarefa**, ou no painel **Tarefas**. Informe título, responsável (botão **Eu**), ação, prioridade e prazo.
- **O painel segue o documento selecionado:** mostra só as tarefas dele. **Todos os documentos** amplia.
- **Visões:** *Hoje*, *Esta semana*, *Atrasadas*, *Todas* e, em **Mais ▾**, *Prontas para fazer*, *A triar*, *Aguardando terceiros*, *Lembretes* e *Sem prazo*.
- **Concluir:** clique em **✓ Concluir** na própria lista. **Reabrir** volta atrás. Nada é registrado no SIGADOC.
- **Ler sem sair da tarefa:** **Consultar documento** abre o texto e os PDFs por cima.
- **Tarefa que depende de outra** aparece recuada, com o aviso `⛓ 1 de 2 anterior(es) em aberto` e a linha *depende de*, que lista as anteriores. O botão **✓ Concluir** só libera depois delas. A linha *libera* mostra quem espera por ela.
- **Juntados seguem o principal:** concluir a última tarefa de um documento principal conclui as tarefas das peças juntadas a ele. A mensagem lista o que foi concluído.

## 6. Notas, minutas e IA

- **Notas:** escreva e clique em **Salvar notas**. O ponto laranja na aba indica algo não salvo.
- **Minuta:** escolha o tipo (*Resposta*, *Encaminhamento*, *Pedido de complementação*) e clique em **Começar por um modelo** (texto-base, sem IA) ou escreva. **Salvar nova versão** guarda o texto, **Exportar (RTF)** cria um arquivo para o Word e **Comparar com versão ▾** mostra as diferenças. O texto final você cola no SIGADOC.
- **IA (opcional):** **Analisar e sugerir providências**, no Resumo, ou **Gerar nova com IA**, **Completar texto atual** ou **Revisar texto atual**, na minuta, com o que você quer em **Pedido desta minuta**. A sugestão abre para revisão e só é salva quando você salvar.

**Menu ▾ ▸ Configurar IA** define onde a IA roda:

| Opção | Os dados saem do computador? |
|---|---|
| **Ollama neste computador** | Não. Instale o Ollama e baixe um modelo: `ollama pull qwen3:4b-instruct` |
| **Ollama Cloud** | Sim. Exige a chave de API da sua conta em ollama.com |
| **Outro servidor HTTPS** | Sim, para o servidor informado |
| **ChatGPT (assinatura)** | Sim, para a OpenAI. Usa a cota do seu plano Plus ou Pro: clique em **Entrar com o ChatGPT…** |

Depois de escolher, clique em **Listar modelos do servidor**, selecione o modelo e salve.

**Atenção com a nuvem:** nas opções em que os dados saem do computador, o texto do documento, as notas e as fontes são enviados a cada clique em Analisar ou Gerar, nunca ao importar. Use só com documentos que podem sair do órgão.

Enquanto a IA trabalha, a barra inferior mostra a etapa e o botão **Cancelar**. Documentos longos no Ollama local demoram um pouco mais na primeira vez, porque o Ponte pede ao modelo uma janela maior.

### Modelos de trabalho

Um modelo de trabalho guarda a estrutura, as instruções e os exemplos de um tipo de resposta, para você e a IA começarem do mesmo ponto. Ele fica no botão **Modelos** do cabeçalho. É opcional.

O caminho do dia a dia:

1. **Criar:** em uma minuta já revisada, clique em **Guardar como modelo**. O texto vira o rascunho de um modelo novo. Dê um nome.
2. **Estrutura:** ajuste as seções, as variáveis e os critérios de conferência. **Salvar rascunho**.
3. **Instruções:** cadastre as regras de redação e clique em **Publicar versão** em cada uma (só instrução publicada pode ser adicionada a um modelo). Depois, na aba **Instruções** do modelo, escolha **Adicionar instrução ao modelo** e defina cada uma como *obrigatória*, *opcional* ou *condicional*.
4. **Exemplos:** cadastre respostas boas e clique em **Aprovar**. Pode anexar o arquivo da resposta real (PDF com texto, .txt ou .docx) e clicar em **Extrair texto**; se quiser, leia e corrija em **Conferir texto…**. Só exemplo aprovado, com texto extraído ou conferido, é usado. Eles ensinam organização e tom, nunca os fatos do caso.
5. **Testar:** com o documento desejado selecionado na Ficha, veja o que seria enviado à IA. Nada é enviado.
6. **Publicar:** **Publicar versão N**. Para mudar depois, revise: o Ponte cria a versão seguinte, e as minutas antigas continuam ligadas à versão que usaram.
7. **Usar:** em **Anotações e minutas**, escolha o **Modelo de trabalho** (o Ponte pode sugerir um pelas palavras do documento; a escolha é sua), marque as **Instruções opcionais** que valem para o caso e escreva o **Pedido desta minuta**. Depois clique em **Gerar nova com IA**, **Completar texto atual** (preserva o que já está escrito) ou **Revisar texto atual** (abre a comparação antes de substituir).
8. **Conferir:** na lista de versões, **Material utilizado** mostra o modelo, as instruções, os exemplos e as fontes que a geração usou. Um trecho "encontrado na fonte" prova só que a citação existe, não que a conclusão está certa.

Se duas instruções se contradisserem, o Ponte avisa antes de gerar. É só um aviso: decida você.

**Em equipe:** ao publicar com o escopo **Compartilhado com a equipe**, o modelo chega a todos na próxima sincronização. Qualquer integrante pode revisar, e o histórico guarda quem fez cada versão. **Pessoal** apenas esconde o modelo dos outros na tela; não é uma proteção. Os modelos pessoais antigos viram modelos pessoais publicados, sem perda de texto. Esta atualização muda o formato do acervo: na equipe, atualize todos os computadores juntos.

## 7. Atendimentos

Um atendimento é uma **passagem** de um documento ou de uma demanda pelo gabinete: quando entrou, com quem está, em que situação, quando saiu. É o controle que muitos gabinetes fazem em planilha.

- **De onde vêm:** da importação do SIGADOC (o Ponte cria sozinho na primeira importação de cada documento), do botão **Registrar nova entrada** ou de **Importar planilha…**.
- **Desligar a criação automática:** desmarque **Criar atendimento ao importar do SIGADOC**, no alto do painel.
- **Se a criação automática falhar**, o documento é importado assim mesmo e o painel mostra um aviso com o número e o motivo. Use **Tentar de novo** ou **Dispensar** (e registre à mão, se quiser).
- **O painel segue o documento selecionado:** mostra só os atendimentos dele, e **Registrar nova entrada** cria um atendimento já ligado a ele. **Todos os atendimentos** amplia.
- **A ficha** abre ao lado da lista, com **Salvar**, **Encerrar** e **Excluir…** no alto. Só a referência ou o assunto é obrigatório.
- **Ligar a documento:** informe o número de um documento já importado. A referência passa a ser o número dele.
- **Criar tarefa** gera uma tarefa do documento ligado; por isso só fica ativo com um documento ligado.
- **Anexos e linha do tempo:** a ficha aceita arquivos seus e registra cada mudança de situação.
- **Excluir** um atendimento é definitivo, com confirmação. Não há Lixeira para atendimentos.
- **Fila**, a lista suspensa no alto do painel, agrupa os atendimentos (prazo vencido, vence hoje, sem responsável…). **Filtros ▾** abre responsável, origem, destino e situação.
- **Planilha:** **Importar planilha…** lê a aba PROCESSOS de um arquivo .xlsx, mostra uma prévia e importa só as linhas novas. A importação pode ser desfeita.

## 8. Mais de um computador

Em **Menu ▾ ▸ Rede e equipe**. Trocar de modo nunca apaga dados.

**Você, em dois computadores (casa e trabalho).** Escolha **Pasta compartilhada: eu, em mais de um computador**, clique em **Adicionar pasta…** e aponte para uma pasta do Google Drive, do OneDrive ou de um pendrive. Crie uma senha de pelo menos 8 caracteres e clique em **Salvar e sincronizar agora**. No outro computador, a mesma pasta e a mesma senha. O Ponte sincroniza ao abrir, a cada 5 minutos e quando você pedir. Guarde a senha: sem ela o arquivo da pasta não abre.

**Equipe do gabinete, na rede interna.** Um computador é o principal e os outros se conectam a ele. Nada vai para a internet.

1. **No principal:** marque **Este é o computador principal do gabinete**, clique em **Salvar e ficar disponível agora** e em **Copiar código de conexão**. Envie o código aos colegas.
2. **Nos outros:** marque **Conectar ao computador principal do gabinete**, cole o código, clique em **Usar código** e em **Salvar e sincronizar acervo agora**. Se o computador já tinha documentos próprios, faça um backup antes: passa a valer o acervo do gabinete.

- Sem conexão com o principal, tudo continua funcionando e é enviado quando a conexão volta.
- Se o mesmo item foi alterado nos dois lados, aparece um **conflito de edição**. A janela mostra os dois valores, campo a campo, e você escolhe **Combinar**, **Usar só a minha versão** ou **Usar só a do computador principal**.
- Se o principal recusou algo criado sem conexão (uma tarefa de um documento que foi para a lixeira, por exemplo), as alterações seguintes desse item aparecem como aviso, só com **Dispensar o aviso**. Refaça à mão o que ainda for necessário.
- Se os colegas não conseguirem conectar, peça à TI para liberar a porta 7878 (TCP) no firewall do computador principal.

**Nunca** instale o Ponte nem copie o acervo para dentro do Google Drive ou do OneDrive: o acervo só abre na conta do Windows que o gravou.

## 9. Backup, lixeira e exportação

- **Backup:** **Menu ▾ ▸ Fazer backup agora** cria uma cópia do acervo na pasta de backups. Ela só abre na mesma conta do Windows.
- **Lixeira:** excluir documento, tarefa, anexo ou versão importada manda para a **Lixeira** (Menu ▾), de onde se restaura. Por padrão nada é apagado de vez. Na janela da Lixeira você pode definir um prazo, de 7 dias a 1 ano, para a limpeza automática.
- **Exportar:** **Menu ▾ ▸ Exportar dados para CSV…** grava documentos, tarefas e atendimentos em três arquivos para o Excel, sem textos nem anexos.
- **Ao fechar** com algo não salvo, o Ponte lista o que está pendente, com um botão **Ir para** em cada item.

## 10. Se algo der errado

| Situação | O que fazer |
|---|---|
| A extensão diz que não está ligada ao Ponte | Instale a versão mais recente do Ponte, abra-o uma vez e reinicie o navegador. |
| A IA diz que o modelo não está instalado | Em Configurar IA, clique em **Listar modelos do servidor** e escolha um da lista. |
| A IA diz que o documento não coube na janela do modelo | Analise sem as fontes extras ou escolha outro provedor. |
| A cota do ChatGPT acabou | Veja o saldo em ChatGPT ▸ Configurações ▸ Uso, espere a renovação ou troque de provedor. |
| O colega não conecta ao computador principal | O principal precisa estar com o Ponte aberto, na mesma rede, com a porta 7878 liberada. |
| "Não foi possível proteger/abrir os dados deste perfil Windows" | O acervo é de outra conta do Windows. Entre com a conta que fez a instalação. |
| Excluí algo sem querer | Menu ▾ ▸ Lixeira ▸ **Restaurar**. |

A lista completa de mensagens e soluções está na seção "Solução de problemas" de <https://github.com/demusis/ponteproj>. A versão instalada aparece em **Menu ▾ ▸ Sobre o Ponte**.
