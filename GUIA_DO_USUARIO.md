# Ponte — Guia do usuário

O Ponte é um programa do Windows que guarda, no seu computador, os documentos que você importa do SIGADOC, com texto, PDFs e histórico. Nele você anota, prepara minutas, acompanha prazos e, se o gabinete quiser, trabalha em equipe. Nada é enviado ao SIGADOC a partir do Ponte: assinaturas e tramitação continuam lá.

## 1. Instalar

1. Feche o Ponte, se estiver aberto.
2. Baixe o instalador em <https://github.com/demusis/ponteproj/releases/latest>, dê dois cliques em **Ponte-Setup-X.Y.Z.exe** (a versão mais recente) e clique em **Avançar** até o fim. Não precisa de administrador.
3. Feche e abra o navegador (Chrome ou Edge).
4. Abra o Ponte pelo menu Iniciar. A tela inicial diz se o navegador está ligado ao Ponte.
5. Instale a extensão no navegador, uma única vez — de um destes dois jeitos:
   - **Pela loja** (quando o gabinete tiver publicado): abra o link da extensão na Chrome Web Store e clique em **Adicionar ao Chrome** (no Edge, permita "extensões de outras lojas").
   - **Pela pasta instalada**: na tela inicial do Ponte, clique em **Copiar caminho** (ao lado de "Extensão do navegador"); no navegador, abra `chrome://extensions` (ou `edge://extensions`), ative **Modo do desenvolvedor**, clique em **Carregar sem compactação** e cole o caminho na barra de endereço da janela que abre.
   - Feche e abra o navegador de novo.

Para atualizar o Ponte, rode o novo `Ponte-Setup-X.Y.Z.exe`. O instalador faz uma cópia de segurança do acervo antes de mudar qualquer coisa e remove as versões antigas. Para desinstalar, use "Aplicativos instalados" do Windows; os seus dados não são apagados.

Os dados ficam só na sua conta do Windows, protegidos. Outra conta do Windows no mesmo computador não os vê e precisa instalar o Ponte para si.

## 2. Importar um documento

1. No SIGADOC, abra o documento (a ficha, não a lista).
2. No canto da página, clique em **Importar para o assistente**.
3. A mensagem "Importado" aparece e o Ponte abre o documento.

Se aparecer um erro, a mensagem diz o que fazer. As mais comuns:

| Mensagem | O que fazer |
|---|---|
| "O Ponte não foi encontrado neste computador" | Abra o Ponte uma vez e reinicie o navegador. Se continuar, rode o Instalar.cmd. |
| "Não recebemos a confirmação do Ponte" | Clique em **Repetir importação**. Nada é duplicado. |
| "Importação ignorada: o SIGADOC mostra o mesmo conteúdo já importado em …" (em laranja) | Não é erro: o documento já está no Ponte exatamente assim, e nada foi gravado. Se ele mudou no SIGADOC, atualize a página (F5) e importe de novo; o Ponte guarda a nova versão. |
| "A página ainda não terminou de carregar" | Espere a página abrir por completo e tente de novo. |
| "Esta página não é a ficha de um documento" | Abra o documento, não a lista ou a busca. |

Clicar no ícone da extensão mostra se ela está ligada ao Ponte e o que fazer se não estiver. No Ponte, **Menu ▾ ▸ Sobre o Ponte** mostra a versão instalada, quem fez o programa e onde baixar atualizações.

## 3. A ficha do documento

Ao escolher um documento na lista à esquerda, a ficha abre em quatro abas:

- **Resumo** — dados principais, tarefas, análise por IA (opcional) e documentos parecidos.
- **Documento** — texto importado, PDFs, **Anexos**, dados do SIGADOC e histórico de tramitação.
- **Anotações e minutas** — suas notas e o editor de minutas, com versões salvas, exportação (RTF) e comparação.
- **Mais** — vínculos entre documentos, versões importadas e detalhes técnicos.

Documentos juntados aparecem abaixo do documento principal, com a marca ↳. O cabeçalho da ficha tem o botão **Abrir documento principal**.

**Algo que não está no SIGADOC** (ligação, memorando, e-mail, papel): clique em **＋ Registro manual** na lista da esquerda, preencha tipo, título, origem, data e texto, e salve. O registro ganha um número `MANUAL-ano/número`, a marca **Manual** em todo lugar e funciona como qualquer documento (tarefas, notas, anexos, IA, equipe). Para alterar, **Editar registro…** na ficha: cada edição vira uma versão.

**Anexos:** em Documento ▸ Anexos (e na ficha de cada tarefa), **Anexar arquivo…** junta um arquivo do seu computador: até 20 por documento ou tarefa, de até 8 MB cada, nos tipos PDF, Word, Excel, PowerPoint, OpenDocument, texto, imagens e e-mails. Programas, scripts, atalhos e arquivos .zip são recusados. Se um arquivo for recusado, a mensagem explica o motivo e o que fazer. Cada anexo pode ter uma descrição; **Ler no Ponte** abre PDFs no leitor do Ponte, **Abrir** usa o programa padrão do Windows, **Salvar como…** grava uma cópia e **Excluir…** manda para a Lixeira. Os anexos ficam no acervo protegido e aparecem para a equipe.

## 4. Tarefas e prazos

- Em **Resumo ▸ Tarefas deste documento**, clique em **Nova tarefa**. Em **Tarefas** (no topo), veja todas.
- Para concluir, clique em **✓ Concluir** na própria lista. Para voltar atrás, **Reabrir**.
- **Exportar para planilhas e para o CERES:** Menu ▸ **Exportar dados para CSV…** grava documentos, tarefas e atendimentos em três arquivos `.csv` (sem textos nem anexos) e abre a pasta. No CERES, envie cada arquivo como um conjunto.
- Enquanto uma tarefa estiver em edição, o rótulo **Tarefas ●** fica laranja no topo. Se você clicar em outra tela ou em outra tarefa, o Ponte pergunta: **Salvar e continuar**, **Descartar alterações** ou **Voltar à tarefa**. Para ler o documento sem sair da tarefa, use **Consultar documento** na ficha.
- **Documentos juntados seguem o principal:** quando a última tarefa aberta de um documento principal é concluída, as tarefas abertas das peças juntadas a ele (e das juntadas a estas) são concluídas automaticamente, com uma anotação dizendo o motivo. A mensagem de confirmação lista o que foi concluído. Tarefas que dependem de outra ainda aberta ficam como estão e são informadas. Reabrir o principal não reabre as peças.
- **Tarefa que depende de outra** aparece recuada à direita, abaixo da linha *↳ depende de: …*, e depois das tarefas de que depende. Com ⛓ no cartão, ainda há tarefa anterior aberta: **✓ Concluir** só libera depois dela (a dica diz qual). **Mais ▾ ▸ Prontas para fazer** lista o que pode ser feito agora. Na ficha, **Cadeia de dependências** mostra o que vem antes e o que espera por ela.
- Datas são digitadas como dd/mm/aaaa ou escolhidas no calendário 📅 (mês com os dias da semana; há também o botão **Hoje**).
- Na lista de documentos, **Filtrar** mostra só os atrasados, os com tarefa aberta ou os concluídos.

## 5. Notas, minutas e IA

- **Notas**: escreva e clique em **Salvar notas**. O ponto laranja na aba indica algo não salvo.
- **Minuta**: escolha o tipo, escreva ou comece por um modelo. **Salvar nova versão** guarda o texto; **Exportar (RTF)** cria um arquivo para o Word; **Comparar com versão** mostra as diferenças.
- **IA** (opcional): **Analisar e sugerir providências** no Resumo, ou **Gerar sugestão com IA** na minuta. A linha ao lado do botão diz para onde os dados vão. Contexto e notas não salvos são salvos antes. A sugestão aparece para revisão e só vira versão quando você salvar.
- **Menu ▸ Configurar IA** define onde a IA roda: neste computador (Ollama, nada sai da máquina), no **Ollama Cloud** (conta em ollama.com; cole a chave de API), num servidor do órgão, ou no **ChatGPT (assinatura)**, usando a cota do seu plano Plus ou Pro: clique em **Entrar com o ChatGPT…**, autorize no navegador e marque o uso do plano. Clique em **Listar modelos do servidor** e escolha o modelo na caixa de seleção. A configuração fica salva e protegida pela sua conta do Windows.
- **Enquanto a IA trabalha**, a barra de baixo mostra a etapa (preparando, recebendo a resposta, conferindo) e o botão **Cancelar**. Se a resposta vier com erro (trecho inventado, prazo que não está no texto), o Ponte pede a correção ao modelo até três vezes e só salva o que passar na conferência.
- **Atenção com a nuvem:** com Ollama Cloud, servidor remoto ou ChatGPT, o texto do documento, as notas e o contexto saem do computador a cada clique em Analisar ou Gerar sugestão (nunca ao importar). Use só com documentos que podem sair do órgão. Com o ChatGPT, quando a cota do plano acaba o Ponte avisa e espera; o limite por aplicativo fica em ChatGPT ▸ Configurações ▸ Uso.

## 6. Atendimentos (controle do gabinete)

- **Registrar nova entrada** abre a ficha ao lado da lista. Informe pelo menos a referência ou o assunto. **Salvar** fica sempre visível no alto da ficha.
- **Importar planilha…** lê a aba PROCESSOS de um .xlsx. A prévia lista as linhas e marca as que já estão no controle; você pode importar só as novas. Uma importação pode ser desfeita em **Importações anteriores**, com confirmação.
- **Fila** agrupa os atendimentos (vencidos, vence hoje, sem responsável…) e mostra quantos há em cada uma. **Filtros** abre responsável, origem, destino e situação.
- Clique no título de uma coluna para ordenar.
- Com um documento aberto na biblioteca, o painel mostra só os atendimentos dele (**Todos os atendimentos** amplia) e **Registrar nova entrada** cria o atendimento já ligado ao documento aberto, com o número dele como referência.
- **Ligar a documento** liga o atendimento a um documento importado pelo número; a referência vira o número do documento e o Resumo do documento passa a mostrar o atendimento. A ficha aceita anexos depois de salva.

## 7. Trabalhar em equipe (Menu ▸ Rede e equipe)

**Você, em dois computadores (casa e trabalho):** escolha **Pasta compartilhada: eu, em mais de um computador**, clique em **Adicionar pasta…** e aponte para uma pasta do Google Drive, OneDrive ou um pendrive (pode ser mais de uma), crie uma senha de pelo menos 8 caracteres e clique em **Salvar e sincronizar agora**. No outro computador, as mesmas pastas e a mesma senha. O Ponte sincroniza ao abrir, a cada 5 minutos e quando você pedir; a barra de baixo mostra o que está fazendo. Nunca copie o acervo nem instale o Ponte dentro do Drive.

Um computador do gabinete é o **principal**; os outros se conectam a ele pela rede interna. Nada vai para a internet.

- **No computador principal**: escolha "Este é o computador principal do gabinete", clique em **Salvar e ficar disponível agora** e depois em **Copiar código de conexão**. Envie o código aos colegas.
- **Nos outros computadores**: escolha "Conectar ao computador principal", cole o código em **Código de conexão**, clique em **Usar código** e depois em **Salvar e sincronizar acervo agora**.
- O ponto colorido na barra inferior mostra o estado em poucas palavras (em dia, a enviar, sem conexão, conflitos). Passe o mouse para ver a frase completa e clique nele para abrir a janela.
- Sem conexão com o principal, tudo continua funcionando neste computador e é enviado quando a conexão volta.
- Se o mesmo item foi alterado nos dois lados, aparece um **conflito de edição**: a janela mostra, campo a campo, o valor de cada lado e quem alterou. Escolha **Combinar**, **Usar só a minha versão** ou **Usar só a do computador principal**, e confirme.

Se os colegas não conseguirem conectar, peça à TI para liberar a porta 7878 (TCP) no Firewall do Windows do computador principal.

## 8. Backup e lixeira

- **Menu ▸ Fazer backup agora** cria uma cópia protegida do acervo na pasta de backups. Ela só abre na mesma conta do Windows.
- Excluir um documento, tarefa, anexo ou importação pede confirmação e manda para a **Lixeira** (Menu ▸ Lixeira…), de onde pode ser restaurado.
- Ao fechar o Ponte com algo não salvo, a janela lista o que está pendente, com um botão **Ir para** em cada item.
