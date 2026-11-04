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
| "A página ainda não terminou de carregar" | Espere a página abrir por completo e tente de novo. |
| "Esta página não é a ficha de um documento" | Abra o documento, não a lista ou a busca. |

Clicar no ícone da extensão mostra se ela está ligada ao Ponte e o que fazer se não estiver.

## 3. A ficha do documento

Ao escolher um documento na lista à esquerda, a ficha abre em quatro abas:

- **Resumo** — dados principais, tarefas, análise por IA (opcional) e documentos parecidos.
- **Documento** — texto importado, PDFs, dados do SIGADOC e histórico de tramitação.
- **Anotações e minutas** — suas notas e o editor de minutas, com versões salvas, exportação (RTF) e comparação.
- **Mais** — vínculos entre documentos, versões importadas e detalhes técnicos.

Documentos juntados aparecem abaixo do documento principal, com a marca ↳. O cabeçalho da ficha tem o botão **Abrir documento principal**.

## 4. Tarefas e prazos

- Em **Resumo ▸ Tarefas deste documento**, clique em **Nova tarefa**. Em **Tarefas** (no topo), veja todas.
- Para concluir, clique em **✓ Concluir** na própria lista. Para voltar atrás, **Reabrir**.
- **Documentos juntados seguem o principal:** quando a última tarefa aberta de um documento principal é concluída, as tarefas abertas das peças juntadas a ele (e das juntadas a estas) são concluídas automaticamente, com uma anotação dizendo o motivo. A mensagem de confirmação lista o que foi concluído. Tarefas que dependem de outra ainda aberta ficam como estão e são informadas. Reabrir o principal não reabre as peças.
- Datas são digitadas como dd/mm/aaaa (há o botão **Hoje**).
- Na lista de documentos, **Filtrar** mostra só os atrasados, os com tarefa aberta ou os concluídos.

## 5. Notas, minutas e IA

- **Notas**: escreva e clique em **Salvar notas**. O ponto laranja na aba indica algo não salvo.
- **Minuta**: escolha o tipo, escreva ou comece por um modelo. **Salvar nova versão** guarda o texto; **Exportar (RTF)** cria um arquivo para o Word; **Comparar com versão** mostra as diferenças.
- **IA** (opcional): **Analisar e sugerir providências** no Resumo, ou **Gerar sugestão com IA** na minuta. A linha ao lado do botão diz para onde os dados vão. Contexto e notas não salvos são salvos antes. A sugestão aparece para revisão e só vira versão quando você salvar.
- **Menu ▸ Configurar IA** define se a IA roda neste computador (Ollama) ou num servidor do órgão.

## 6. Processos e demandas (controle do gabinete)

- **Registrar nova entrada** abre a ficha ao lado da lista. Informe pelo menos a referência ou o assunto. **Salvar** fica sempre visível no alto da ficha.
- **Importar planilha…** lê a aba PROCESSOS de um .xlsx. A prévia lista as linhas e marca as que já estão no controle; você pode importar só as novas. Uma importação pode ser desfeita em **Importações anteriores**, com confirmação.
- **Fila** agrupa os atendimentos (vencidos, vence hoje, sem responsável…) e mostra quantos há em cada uma. **Filtros** abre responsável, origem, destino e situação.
- Clique no título de uma coluna para ordenar.
- **Ligar a documento** liga o atendimento a um documento importado pelo número; o Resumo do documento passa a mostrar o atendimento.

## 7. Trabalhar em equipe (Menu ▸ Rede e equipe)

Um computador do gabinete é o **principal**; os outros se conectam a ele pela rede interna. Nada vai para a internet.

- **No computador principal**: escolha "Este é o computador principal do gabinete", clique em **Salvar e ficar disponível agora** e depois em **Copiar código de conexão**. Envie o código aos colegas.
- **Nos outros computadores**: escolha "Conectar ao computador principal", cole o código em **Código de conexão**, clique em **Usar código** e depois em **Salvar e sincronizar acervo agora**.
- O indicador no canto superior direito mostra o estado (conectado, sem conexão, conflitos). Clique nele para abrir a janela.
- Sem conexão com o principal, tudo continua funcionando neste computador e é enviado quando a conexão volta.
- Se o mesmo item foi alterado nos dois lados, aparece um **conflito de edição**: a janela mostra, campo a campo, o valor de cada lado e quem alterou. Escolha **Combinar**, **Usar só a minha versão** ou **Usar só a do computador principal**, e confirme.

Se os colegas não conseguirem conectar, peça à TI para liberar a porta 7878 (TCP) no Firewall do Windows do computador principal.

## 8. Backup e lixeira

- **Menu ▸ Fazer backup agora** cria uma cópia protegida do acervo na pasta de backups. Ela só abre na mesma conta do Windows.
- Excluir um documento, tarefa ou importação pede confirmação e manda para a **Lixeira** (Menu ▸ Lixeira…), de onde pode ser restaurado.
- Ao fechar o Ponte com algo não salvo, a janela lista o que está pendente, com um botão **Ir para** em cada item.
