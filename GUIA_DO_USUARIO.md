# Ponte — Guia do usuário

O Ponte guarda, no seu computador, os documentos que você importa do SIGADOC, com texto, PDFs e histórico de tramitação. Com eles guardados, você anota, prepara minutas, acompanha prazos, controla as demandas do gabinete e as reservas de salas, livros e equipamentos. O Ponte **nunca envia nada ao SIGADOC**: assinar e tramitar continuam sendo feitos lá.

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

**Atualizar:** feche o Ponte e rode o instalador novo. Seus dados são mantidos, e o instalador faz uma cópia de segurança antes. Para saber se há versão nova, use **Ajustes ▸ Sobre o Ponte ▸ Verificar se há versão nova** (veja a seção 11).

**Desinstalar:** em *Aplicativos instalados* do Windows. Seus dados não são apagados.

Os dados ficam só na sua conta do Windows, cifrados. Outra conta no mesmo computador não os vê e instala o Ponte para si.

## 2. A janela em um minuto

- **Barra lateral (esquerda):** no alto, **Buscar** (Ctrl+K) procura em todo o acervo; veja **Buscar em todo o acervo**, abaixo. Em seguida, escolhe a área. **Hoje** (Ctrl+1) é a tela em que o Ponte abre (com o acervo vazio, abre em Documentos, com o card **Primeiros passos**). **Documentos** (Ctrl+2) tem a lista de documentos e a ficha do selecionado. **Tarefas** (Ctrl+3) é a lista de tarefas. **Atendimentos** (Ctrl+4) é o controle do gabinete. **Modelos** (Ctrl+5) guarda os modelos de trabalho. **Recursos** (Ctrl+6) é o controle de salas, livros e equipamentos, com reservas e empréstimos (seção 8). Um número vermelho em Hoje (tarefas atrasadas ou para hoje, mais atendimentos com prazo vencido ou para hoje), em Tarefas (só as tarefas) e em Recursos (devoluções atrasadas) avisa o que pede atenção. Embaixo ficam **Ajustes** e **Ajuda** (abre este guia).
- **Atalhos:** **Ctrl+K** abre a busca em todo o acervo, de qualquer área. **Ctrl+F** vai para a busca da área (de outras áreas, para a busca de documentos). **Ctrl+N** cria: registro manual em Hoje e Documentos, nova tarefa em Tarefas (do documento selecionado), nova entrada em Atendimentos, nova reserva em Recursos (na aba **Recursos**, novo recurso). **Ctrl+S** salva o que está em edição (tarefa, atendimento, reserva ou recurso, notas do documento, nova versão da minuta, modelo, configuração de IA). Em Modelos, **Ctrl+F** (busca), **Ctrl+N** (novo modelo), **Ctrl+S** (salvar rascunho) e **Esc** também funcionam. **Esc** fecha a janela ou o painel lateral aberto, se nada estiver sem salvar. Em Tarefas, no modo de seleção (seção 5), **Ctrl+A** seleciona todas as tarefas da lista e **Esc** fecha a pergunta aberta e, depois, sai da seleção.
- **Tarefas e Atendimentos abrem com tudo**, sem seguir o documento selecionado em Documentos. Para ver só os de um documento: em Tarefas, **Ver todas** no card Tarefas da ficha ou **Só as de <número>** ao fim dos botões de visão; em Atendimentos, **Só os de <número>** dentro de **Filtros**. Uma faixa avisa que a lista está restrita e traz **Ver todas** / **Ver todos**.
- **Voltar:** ao ir de uma tarefa, de um atendimento ou de Hoje para um documento, a ficha mostra no alto **Voltar à tarefa**, **Voltar ao atendimento** ou **Voltar a Hoje**.
- **Barra inferior:** um ponto colorido mostra a situação da rede, e **Sincronizar** aparece no modo pasta compartilhada. Clicar na situação abre Ajustes ▸ Equipe e sincronização.
- **Avisos:** aparecem numa caixa no canto inferior direito. Os de sucesso somem sozinhos; os de erro ficam até você fechar.
- **Ajustes:** Equipe e sincronização, E-mail, Importação do SIGADOC, Lembretes (aviso diário de prazos, veja a seção 5), Inteligência artificial, Cópia de segurança e exportação, Lixeira, Aparência (tema Como o Windows, Claro ou Escuro, e **Tamanho do texto**: Normal, Grande ou Maior) e Sobre o Ponte. Ao aumentar o texto, o menor tamanho a que a janela pode ser reduzida acompanha o novo tamanho, sem nunca passar do tamanho da tela.
- **Datas:** digite `dd/mm/aaaa` ou clique no ícone de calendário; **Hoje** preenche a data de hoje.
- **Nada se perde ao trocar de tela:** se há algo não salvo, o Ponte pergunta antes.

**Buscar em todo o acervo.** Clique em **Buscar**, no alto da barra lateral, ou aperte **Ctrl+K**. Digite pelo menos duas letras: o Ponte procura em documentos (número, assunto, texto, notas e PDFs), tarefas e atendimentos, e mostra os resultados em três grupos, **Documentos**, **Tarefas** e **Atendimentos**, com até 8 de cada. Os recursos e as reservas de recursos também são encontrados, em grupos próprios. Se há mais, a linha "e mais N" avisa; refine o que digitou. Nas tarefas e nos atendimentos, a busca não diferencia maiúsculas de minúsculas nem acentos. As setas do teclado trocam de resultado, **Enter** abre o escolhido e **Esc** fecha a busca. Também se abre um resultado com o clique. O Ponte leva direto ao documento, à tarefa ou ao atendimento.

## 3. Importar um documento

1. No SIGADOC, abra o documento (a ficha dele, não a lista).
2. No canto inferior direito da página, clique em **Importar para o assistente**.
3. A mensagem *Importado* aparece e o Ponte abre o documento.

O painel avisa antes do clique: **Já está no Ponte — importado em…** (com **Abrir no Ponte** e o botão **Importar de novo**) ou **Ainda não está no Ponte**. Se o Ponte não responde, o aviso aparece ali mesmo, com **Tentar de novo**. O atalho **Alt+Shift+I** faz o mesmo que o botão (pode ser trocado em chrome://extensions/shortcuts). Com o Ponte já aberto, a importação traz a janela existente para a frente, no documento; não abre outra.

**Nota e prioridade na hora de importar.** No painel, clique em **Adicionar nota ou prioridade** (o link passa a dizer **Ocultar nota e prioridade**). Escreva em **Nota para o Ponte** (até 2000 caracteres) e/ou escolha a **Prioridade** (*Baixa*, *Normal*, *Alta* ou *Urgente*; *Não mudar* deixa como está), e então clique em **Importar para o assistente**. A nota é acrescentada ao fim da nota do documento, com a data (por exemplo, `10/11/2026 — responder até sexta`), sem apagar o que já estava escrito; a prioridade é aplicada ao documento. Depois da importação, o painel diz *Nota e prioridade enviadas.* (ou só uma delas). Isso exige o Ponte 0.31 e a extensão 0.3.0; com um Ponte mais antigo, o link não aparece ou o painel avisa que a importação foi feita sem a nota. Se o Ponte não conseguir gravar, a importação vale mesmo assim e o painel diz o que faltou, por exemplo *A importação foi feita, mas a nota não foi salva. Escreva a nota na ficha do documento.*; a prioridade também pode ser escolhida lá. A nota vai só ao Ponte deste computador; ela só sai dele se você usa equipe ou pasta compartilhada (seção 9), como qualquer outra nota.

**Importações recentes.** Clique no ícone da extensão, na barra do navegador: abaixo do estado da ligação, **Importações recentes** lista as últimas importações desta sessão do navegador (até 8), com número, hora e resultado (*Importado*, *Já estava no Ponte* ou *Não concluída*). **Abrir no Ponte** abre o documento; **Limpar a lista** esvazia. A lista fica só na memória do navegador e some quando ele é fechado; nada dela vai para o Ponte.

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

**Algo que não está no SIGADOC** (ligação, e-mail, memorando, papel): clique em **Novo ▸ Registro manual…**, preencha tipo, título, origem, data e texto, e salve. O registro ganha um número `MANUAL-ano/número` e a etiqueta **Manual**, e funciona como qualquer documento. **Editar registro…** salva uma nova versão. Um e-mail salvo como arquivo `.eml` (Gmail: Mostrar original ▸ Baixar original) entra pronto por **Novo ▸ Importar e-mail de um arquivo (.eml)…**, com os anexos; e-mails importados ganham número próprio `EMAIL-ano/número` e a etiqueta **E-mail**. Para buscar direto na conta, use **Novo ▸ Importar da conta de e-mail…**, que abre a lista da última pasta e filtro usados; na primeira vez, ou para trocar, use **Ajustes ▸ E-mail** (a página já traz as contas, a pasta e a lista de mensagens): cadastre a conta com uma senha de app (no Gmail, com a verificação em duas etapas ativa), escolha a pasta ou o marcador (por exemplo, **Com estrela**), liste e marque as mensagens a importar. O Ponte só lê a conta; nada é marcado ou apagado. Uma conversa do WhatsApp também vira registro: veja **Conversas do WhatsApp**, mais abaixo.

**Imprimir ou salvar a ficha.** Em **⋯ ▸ Imprimir ou salvar a ficha…**, escolha onde guardar o arquivo. O Ponte cria uma página (arquivo .html) com a ficha e a abre no navegador; de lá, **Ctrl+P ▸ Salvar como PDF** grava um PDF, ou imprima direto. A página traz os dados do documento, a análise (com o aviso de que foi feita com apoio de IA), as tarefas, os atendimentos, a nota, o texto, a lista de arquivos e o histórico de tramitação. Não traz os arquivos em si (PDFs e anexos continuam no Ponte) nem a minuta. **Atenção:** a página salva é uma cópia fora do acervo protegido. Guarde-a, ou apague-a, com o mesmo cuidado que teria com o próprio documento.

**Anexos.** Em Documento ▸ Anexos, na ficha de uma tarefa ou de um atendimento, **Anexar arquivo…** junta um arquivo do seu computador: até 20 por item, de até 8 MB cada. Aceita PDF, Word, Excel, PowerPoint, OpenDocument, texto, imagens e e-mails; recusa programas, scripts e arquivos compactados, e diz o motivo. **Arrastar:** também dá para arrastar arquivos do Explorador de Arquivos do Windows e soltá-los sobre a janela do Ponte, vários de uma vez, quando há um bloco **Anexos** na tela. Valem os mesmos limites; pastas são recusadas. Se dois blocos **Anexos** aparecem juntos, o Ponte não sabe qual escolher: use **Anexar arquivo…** do bloco desejado. **Ler no Ponte** abre PDFs, **Abrir** usa o programa padrão do Windows e **Salvar como…** grava uma cópia.

### Conversas do WhatsApp

Uma conversa do WhatsApp pode ficar guardada no Ponte, junto dos documentos, com tarefas e notas. O registro é do tipo **Conversa do WhatsApp** e leva a etiqueta **WhatsApp** na lista. São conversas com outras pessoas: guarde só o que o trabalho exige.

**Primeira forma: copiar e colar (não precisa da extensão).**

1. No WhatsApp (no navegador, no aplicativo do computador ou no celular), selecione as mensagens e copie. Se copiou no celular, leve o texto ao computador, por exemplo enviando-o a você mesmo.
2. No Ponte, clique em **Novo ▸ Registro manual…** e, na janela, em **Colar conversa do WhatsApp**.
3. Cole o texto (Ctrl+V) em **Mensagens copiadas**. O Ponte mostra quantas mensagens leu, o período, os participantes e a primeira e a última mensagem. Se não encontrar mensagens, diz isso e nada é criado.
4. Se quiser, ajuste **Conversa com** (o nome da pessoa ou do grupo) e o **Título**. O Ponte sugere "Conversa com <nome> — data da última mensagem"; o título precisa ter pelo menos 3 caracteres.
5. Clique em **Criar registro**.

**Segunda forma: pela extensão, no WhatsApp Web (extensão 0.4.0).** Ela vem desligada. Para ligar, uma única vez:

1. Clique no ícone da extensão, na barra do navegador, e em **Usar no WhatsApp Web**. O navegador pergunta se permite o acesso a web.whatsapp.com: aceite.
2. Recarregue a página do WhatsApp Web (F5).

Depois disso, a página do WhatsApp Web ganha um botão pequeno escrito **Ponte**, no canto inferior esquerdo, que abre o painel, uma caixa com o título **Guardar no Ponte**. Para guardar uma conversa:

1. Abra a conversa. Arraste o mouse sobre as mensagens que quer guardar e, no painel, clique em **Trecho selecionado**. Ou clique em **Mensagens visíveis na tela**, para guardar o que está aparecendo.
2. Confira a prévia: quantas mensagens, o período, as fotos e a primeira e a última mensagem. Ajuste o **Título** e **Conversa com**, se quiser. Em **Nota ou prioridade** dá para juntar uma nota (veja a seção 3).
3. Se tiver arquivos da conversa (PDF, Word…), baixe-os no WhatsApp e solte-os sobre o painel, ou use **Escolher arquivo…**. As fotos das mensagens escolhidas vão sozinhas; os outros arquivos, não.
4. Clique em **Enviar ao Ponte**. O painel diz *Guardado no Ponte.* e oferece **Abrir no Ponte** e **Guardar outra conversa**.

Se o painel não reconhecer a página (*O Ponte não reconheceu esta página do WhatsApp Web*), abra **Colar mensagens copiadas**, no próprio painel, e cole ali o que copiou no WhatsApp. Para desligar, clique no ícone da extensão e em **Desligar**; o navegador retira o acesso.

**O que o registro guarda.** Ele recebe número `MANUAL-ano/número`, como os registros manuais, e uma tarefa de triagem, como todo registro. O texto tem uma linha por mensagem, no formato `[dd/mm/aaaa hh:mm] Nome: mensagem`. As linhas seguintes da mesma mensagem ficam recuadas, para que uma linha dentro de uma mensagem nunca passe por mensagem de outra pessoa. Mensagens do próprio WhatsApp (como "fulano entrou no grupo") aparecem sem nome. Na ficha, **Dados do documento** traz **Conversa com**, **Período**, **Mensagens** e **Origem** (*Extensão do navegador* ou *Colado no Ponte*). Fotos e arquivos ficam em **Anexos**. O registro acompanha a equipe ou a pasta compartilhada como qualquer outro (seção 9). **Editar registro…** funciona como nos demais, e o período e a contagem são lidos de novo do texto.

**Limites e cuidados.**

- Até 500 mensagens e 500 KB de texto por registro. Se passar, guarde a conversa em partes.
- Arquivos: até 8 MB cada, 10 por vez pelo painel, 16 MB juntos; um registro aceita até 20 anexos no total. Outros arquivos podem ser juntados depois, em **Anexos**.
- Fotos e arquivos sem legenda entram sem o nome de quem os enviou. Quando o Ponte não tem como saber a data com certeza, eles entram como `[sem data]`, e o painel avisa.
- O Ponte não lê arquivos (PDF, Word…) da página do WhatsApp Web: baixe-os e solte-os no painel, ou junte-os depois em **Anexos**.
- Guardar de novo a mesma seleção pelo painel não duplica, e colar de novo o mesmo texto também não: o Ponte diz que a conversa já estava lá. Vale para o mesmo texto e o mesmo **Conversa com**; se mudar o nome, o Ponte a trata como outra conversa. Já uma conversa guardada pelo painel e depois colada a partir da cópia do WhatsApp pode ficar duas vezes, porque os dois textos não são idênticos. Os arquivos de uma conversa que já existia não são juntados de novo; use **Anexos**.
- Se a mesma conversa já foi guardada e está na **Lixeira**, o Ponte avisa. Restaure-a em **Ajustes ▸ Lixeira**.

**O que a extensão não faz.** Ela só lê as mensagens que você escolheu, e só no momento em que você clica. Não lê outras conversas nem contatos, não envia, não altera e não apaga nada no WhatsApp, e fica desligada até você ligar. Nada sai do computador: o que você manda vai só ao Ponte deste computador e, de lá, só segue para a equipe ou a pasta compartilhada se você configurou uma delas.

## 5. Tarefas e prazos

- **Começar o dia por Hoje:** a área reúne as **Atrasadas** e as **Para hoje** (com o círculo para concluir e um clique para abrir), os **Próximos dias**, os **Atendimentos com prazo vencido ou para hoje** e os **Documentos recentes**. Ao concluir uma tarefa, o aviso do canto traz **Desfazer** por alguns segundos.
- **Calendário de prazos:** em Hoje, os botões **Lista** e **Calendário** trocam a visão. O calendário mostra o mês com as tarefas e os atendimentos que têm prazo: vermelho é atraso, âmbar é para hoje, azul está no prazo, e os atendimentos levam um ícone. As setas trocam de mês e **Hoje** volta ao mês atual. Clique num item para abri-lo; clique em "+N" (ou num dia com itens) para listar os desse dia sob o calendário. Tarefas concluídas e atendimentos encerrados não aparecem.
- **Criar:** em Resumo ▸ **Nova tarefa**, ou na área **Tarefas**. Informe título, responsável (botão **Eu**), ação, prioridade e prazo.
- **Tarefa sem documento.** Na área **Tarefas**, **Nova tarefa** (ou **Ctrl+N**) cria uma tarefa que não pertence a nenhum documento; na lista ela aparece como **Sem documento**. Com a lista filtrada por um documento (faixa "Mostrando só as tarefas de…"), a tarefa nova é dele. No painel da tarefa, **Vincular a um documento…** abre uma busca por número, assunto ou texto; depois, **Trocar de documento…** e **Desvincular do documento** mudam ou desfazem o vínculo. A tarefa é a mesma o tempo todo: anexos, lista de providências e histórico ficam com ela.
- **Visões:** *Todas*, *Hoje*, *Próximos 7 dias*, *Atrasadas* e, em **Mais ▾**, *Prontas para fazer*, *A triar*, *Aguardando*, *Com lembrete* e *Sem prazo*. **Ordenar:** Prazo, Prioridade ou Documento. A lista se divide em Atrasadas, Para hoje, Próximas, Sem prazo e Concluídas; lista vazia diz o motivo. Para ver só as de um documento, use **Só as de <número>**; a faixa *Mostrando só as tarefas de <número>* tem **Ver todas**.
- **Abrir:** clique na tarefa para ver os dados num painel à direita, com **Salvar**, **Concluir**, **Cancelar** e **Excluir…**. Em **Mais detalhes** ficam origem do prazo, lembrete, resultado, checklist, dependências e anexos.
- **Concluir:** clique no círculo à esquerda da tarefa; clicar de novo reabre. Nada é registrado no SIGADOC.
- **Várias tarefas de uma vez:** em Tarefas, clique em **Selecionar** (fica desabilitado enquanto a tarefa aberta tem alterações sem salvar). Em cada linha, o círculo vira um quadrado: marque as tarefas desejadas. **Selecionar todas as visíveis** (ou **Ctrl+A**) marca as da lista, **Limpar seleção** desmarca tudo e **Sair da seleção** (ou **Esc**) volta à lista de sempre. Só as tarefas visíveis podem ficar selecionadas: se um filtro ou a busca as esconde, elas saem da seleção. Com algo marcado, uma barra embaixo oferece **Concluir**, **Prazo…**, **Responsável…**, **Prioridade…** e **Excluir…**. Cada uma pergunta antes de agir, com **Cancelar**: em **Prazo…**, escolha a data ou **Sem prazo**; em **Responsável…**, digite o nome, use **Eu** ou **Sem responsável**; em **Prioridade…**, escolha uma. **Excluir…** manda as tarefas para a Lixeira, de onde se restauram. O Ponte deixa como estão, e diz quais e por quê, as tarefas que já estão concluídas, que dependem de uma tarefa em aberto, que já têm o prazo, o responsável ou a prioridade pedidos, que foram alteradas depois da seleção ou que foram para a Lixeira. Essas continuam selecionadas. Uma linha de resultado mostra o que foi feito, por exemplo *3 tarefas concluídas.* seguida de uma linha por motivo, e traz **Desfazer**, que vale por 30 segundos e deixa de lado as tarefas alteradas desde então. Nada é enviado ao SIGADOC.
- **Ler sem sair da tarefa:** **Consultar documento** abre o texto e os PDFs por cima.
- **Tarefa que depende de outra** aparece recuada, com o aviso `⛓ 1 de 2 anterior(es) em aberto` e a linha *depende de*, que lista as anteriores. A tarefa só pode ser concluída depois delas. A linha *libera* mostra quem espera por ela.
- **Juntados seguem o principal:** concluir a última tarefa de um documento principal conclui as tarefas das peças juntadas a ele. A mensagem lista o que foi concluído.
- **Lembretes de prazos:** uma vez por dia, no horário escolhido (9h, se você não mudou), o Windows mostra um aviso, *Prazos de hoje no Ponte*, só com as quantidades: tarefas atrasadas, tarefas para hoje, atendimentos com prazo vencido ou para hoje e tarefas cuja data de lembrete chegou, por exemplo *3 tarefas atrasadas, 2 para hoje e 1 atendimento com prazo.* Também entram, só como números, as reservas de recursos de hoje e as devoluções atrasadas. Os títulos nunca aparecem, porque o aviso pode ficar visível sobre outras janelas. Não há aviso quando nada vence. Ele só aparece enquanto o Ponte está aberto ou minimizado, e, se o Ponte abrir depois do horário, aparece pouco depois da abertura. Com o Ponte na frente, a mesma frase surge no aviso do canto da janela. Clicar no aviso do Windows traz o Ponte para a frente, em **Hoje**. Está ligado de fábrica. Em **Ajustes ▸ Lembretes**, desmarque **Lembrar dos prazos uma vez por dia** para desligar, ou escolha o **Horário**; **Mostrar um aviso de teste** mostra como o aviso fica (com números de exemplo). Se nada aparecer, veja *Configurações do Windows ▸ Sistema ▸ Notificações*.

## 6. Notas, minutas e IA

- **Notas:** escreva e clique em **Salvar notas** (ou Ctrl+S). O ponto laranja na aba indica algo não salvo. Com a janela larga, as notas ficam à esquerda e a minuta à direita; com a janela estreita, a nota mostra as primeiras linhas, com **Editar nota** e **Recolher**. **Editar nota**, no card Nota do Resumo, leva às Notas e minutas com a nota aberta e o cursor nela, pronta para digitar. Notas vindas da extensão (seção 3) aparecem ali, ao fim do texto, com a data.
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

**Em equipe:** ao publicar com o escopo **Compartilhado com a equipe**, o modelo chega a todos na próxima sincronização. Qualquer integrante pode revisar, e o histórico guarda quem fez cada versão. **Pessoal** apenas esconde o modelo dos outros na tela; não é uma proteção. Os modelos pessoais também vão na sincronização, mas só aparecem para o nome que os criou: se você usa dois computadores, deixe o mesmo nome nos dois, em **Ajustes ▸ Equipe e sincronização ▸ Seu nome na equipe** (o nome inicial é o do usuário do Windows, que costuma ser diferente em cada computador). Quando há modelos pessoais de outro nome no acervo, a lista de Modelos avisa. Os modelos pessoais antigos viram modelos pessoais publicados, sem perda de texto. Esta atualização muda o formato do acervo: na equipe, atualize todos os computadores juntos.

## 7. Atendimentos

Um atendimento é uma **passagem** de um documento ou de uma demanda pelo gabinete: quando entrou, com quem está, em que situação, quando saiu. É o controle que muitos gabinetes fazem em planilha.

- **De onde vêm:** da importação do SIGADOC (o Ponte cria sozinho na primeira importação de cada documento), do botão **Nova entrada** ou de **Importar planilha**.
- **Desligar a criação automática:** desmarque **Criar atendimento ao importar do SIGADOC**, em **Ajustes ▸ Importação do SIGADOC**.
- **Se a criação automática falhar**, o documento é importado assim mesmo e o painel mostra um aviso com o número e o motivo. Use **Tentar de novo** ou **Dispensar** (e registre à mão, se quiser).
- **A lista** abre com todos os atendimentos e mostra as colunas que cabem: Referência, Assunto, Situação e Prazo sempre; Responsável, Entrada, Tempo e Destino / Saída conforme o espaço. Para ver só os de um documento, use **Só os de <número>** em **Filtros**; a faixa *Mostrando só os atendimentos de <número>* tem **Ver todos**. Com um documento selecionado, **Nova entrada** cria um atendimento já ligado a ele.
- **A ficha** abre num painel à direita da lista, com **Salvar**, **Encerrar**, **Descartar alterações** e **Apagar de vez…**. Só a referência ou o assunto é obrigatório.
- **Ligar a um documento do acervo:** digite na busca parte do número, do assunto ou do texto e escolha o documento na lista. A referência passa a ser o número dele.
- **Criar tarefa** gera uma tarefa do documento ligado; por isso só fica ativo com um documento ligado.
- **Anexos e linha do tempo:** a ficha aceita arquivos seus e registra cada mudança de situação.
- **Apagar de vez** um atendimento é definitivo, com confirmação. Não há Lixeira para atendimentos.
- **Fila**, a lista suspensa no alto da área, agrupa os atendimentos (prazo vencido, vence hoje, sem responsável…). **Filtros** abre responsável, origem, destino e situação.
- **Planilha:** **Importar planilha** lê a aba PROCESSOS de um arquivo .xlsx, mostra uma prévia e importa só as linhas novas. A importação pode ser desfeita.

## 8. Recursos

A área **Recursos** (Ctrl+6) controla o que o gabinete empresta ou reserva: salas, livros, equipamentos. Cada **recurso** é cadastrado uma vez; cada reserva ou empréstimo é um **uso**, com data e hora de início e de fim previsto. Nada é enviado a lugar nenhum: é um controle interno, como os atendimentos.

A área tem três abas: **Agora e próximos**, **Calendário** e **Recursos** (o cadastro). No alto, etiquetas contam os recursos em uso, as reservas de hoje e as devoluções atrasadas.

**Cadastrar um recurso.** Na aba **Recursos**, clique em **Novo recurso** (ou Ctrl+N) e preencha:

| Campo | Exemplo |
|---|---|
| Nome | `Sala de reuniões do 2º andar` |
| Tipo | *Sala*, *Equipamento*, *Livro* ou *Outro* |
| Quantidade | `1` (uma sala); `40` (cadeiras empilháveis); `3` (três exemplares de um livro) |
| Onde fica | `2º andar, bloco A` |
| Identificação | tombo, patrimônio, ISBN… |
| Observações | texto livre |

Só o nome é obrigatório. A **quantidade** diz quantos podem ser usados ao mesmo tempo: com `2` notebooks, duas pessoas reservam o mesmo horário. Um recurso que já teve usos **não é apagado**: clique em **Marcar como fora de uso** (e **Voltar a usar**, para reativar). Ele deixa de aceitar reservas novas; as que já existem continuam, e dá para registrar a devolução. **Apagar de vez…** só aparece para recurso que nunca teve nenhum uso. Não se reduz a quantidade abaixo do que reservas e usos já marcados precisam; o Ponte diz qual uso impede.

**Reservar e registrar o uso.** Em **Reservar** (no alto da área; Ctrl+N), preencha a ficha que abre à direita:

- **Recurso** e, quando há mais de um, **Quantidade**. Logo abaixo, o Ponte diz *Livre neste horário* ou *Ocupado neste horário* (ou, para quantidades, por exemplo *1 de 2 livres neste horário*).
- **Quem** (o botão **Eu** preenche o seu nome), **Setor** e **Finalidade**.
- **Início** e **Fim previsto**, cada um com **data e hora**. A hora pode ser digitada como `9`, `930`, `9:30` ou `09h30`; o Ponte a escreve como `09:30`. O fim precisa ser depois do início.
- **Ocupação do dia:** abaixo das datas, os outros usos do mesmo recurso naquele dia, para você escolher um horário livre antes de salvar.
- **Observações** e **Vínculos** (veja abaixo).

**Reservar** guarda a reserva para o horário escolhido. **Registrar uso agora** faz o uso começar neste momento (situação *Em uso*), para quando alguém já levou o recurso.

**Situações.** Um uso é *Reservado*, depois *Em uso* e, por fim, *Devolvido*; ou *Cancelado*. Quando o fim previsto passa e o recurso não foi devolvido, o uso aparece como **Atrasado** (não é uma situação a escolher: é um aviso que some quando a devolução é registrada). Uma reserva cujo dia passou sem ser usada vai para os encerrados.

**Quando o horário está ocupado.** O Ponte não guarda duas reservas que passem da quantidade do recurso no mesmo horário. Ele recusa e diz quem está com o recurso e até quando, por exemplo: *O recurso “Sala de reuniões do 2º andar” já está reservado por Servidor B até 12/11/2026 17:00.* Troque o horário (ou o recurso) e salve de novo. Um uso que termina às 10:00 não impede outro que começa às 10:00. Um recurso *em uso* e não devolvido continua ocupado até ser devolvido: enquanto isso, o Ponte recusa um uso para agora e diz que a devolução está atrasada, e desde quando. Reservas para mais adiante continuam sendo aceitas.

**Durante e depois do uso**, na ficha:

- **Registrar devolução:** mostra **Devolvido em** com a data e a hora de agora; corrija, se preciso, e clique em **Confirmar devolução** (ou em **Voltar**, para desistir).
- **Prorrogar:** leva ao campo **Fim previsto**; informe a nova data e hora e clique em **Salvar**. Se o novo horário já estiver ocupado, o Ponte avisa.
- **Cancelar a reserva:** a reserva deixa de ocupar o recurso e vai para os encerrados.
- **Reabrir** (em usos devolvidos ou cancelados): um uso devolvido volta a *Em uso*; um cancelado volta a *Reservado*. O horário é conferido de novo: se nesse meio-tempo ele foi ocupado, ou o recurso ficou fora de uso, o Ponte recusa e diz por quê.
- **Apagar de vez…** apaga o uso, com confirmação. Não há Lixeira para usos, como nos atendimentos. O recurso, o documento e a tarefa ligados permanecem.

Em usos devolvidos ou cancelados, os campos ficam só para leitura.

**As três abas.**

- **Agora e próximos:** os usos em grupos: **Atrasados**, **Em uso agora**, **Hoje**, **Próximos dias**, **Mais adiante** e, ao clicar em **Ver mais antigos / encerrados**, **Mais antigos e encerrados**. Clique num uso para abrir a ficha. A busca procura em recurso, quem, setor e finalidade. **Filtros** acrescenta **Tipo**, **Situação**, **Recurso** e **Período** (de… até…); **Limpar** desfaz tudo.
- **Calendário:** o mês, com todos os recursos ou só um, escolhido na lista. As setas trocam de mês e **Hoje** volta ao mês atual. Cada linha mostra a hora e o recurso (ou quem, quando se escolhe um recurso); uso que atravessa dias aparece em cada um. Clique num dia para listar os usos dele; **Reservar neste dia** abre a ficha com o início nesse dia. Reservas canceladas não aparecem.
- **Recursos:** o cadastro. Cada linha mostra o tipo, a situação e *livre: 1 de 1* ou *em uso agora: 1 de 2*. A busca procura em nome, tipo, local e identificação, e **Mostrar fora de uso** inclui ou esconde os recursos fora de uso. Clicar num recurso abre a ficha dele, com os próximos usos e o botão **Reservar…**.

Em **Agora e próximos**, o botão **⋯** (Mais ações) tem **Exportar esta lista…** (veja **Exportar**, abaixo).

**Vínculos.** Na ficha do uso, a seção **Vínculos** liga a reserva ao que a motivou:

- **Documento:** busque por número, assunto ou texto. Vale também para e-mails e conversas do WhatsApp guardados, que são documentos: o pedido de sala que chegou por WhatsApp vira a reserva.
- **Atendimento:** busque por referência, assunto ou responsável.
- **Tarefa:** busque uma tarefa que já existe ou clique em **Criar tarefa de devolução**. Ela cria uma tarefa com prazo no dia do fim previsto, ligada ao documento do uso (quando há) e ao próprio uso. O uso é salvo antes, se tinha alterações.

**Abrir** leva ao item ligado; **Desligar** desfaz o vínculo.

**Onde mais os recursos aparecem.**

- **Ficha do documento, atendimento e tarefa:** mostram os usos ligados a eles, e **Reservar recurso…** abre uma reserva nova já ligada ao item.
- **Hoje:** as **Reservas de hoje** e as **Devoluções atrasadas** aparecem junto do que pede atenção no dia, e as reservas entram no calendário de prazos.
- **Lembrete diário** (seção 5): conta as reservas de hoje e as devoluções atrasadas, só como números e sem alarme na hora da reserva.
- **Busca em todo o acervo** (Ctrl+K): encontra recursos e usos.
- **Ficha impressa do documento:** lista os recursos que ele usou.

**Exportar.** **Ajustes ▸ Cópia de segurança e exportação ▸ Exportar dados para CSV…** passa a gravar dois arquivos a mais, no mesmo padrão dos outros:

| Arquivo | Colunas |
|---|---|
| `recursos.csv` | ID, Nome, Tipo, Quantidade, Local, Identificação, Situação, Criado em |
| `usos-de-recursos.csv` | ID, Recurso, Tipo, Quantidade, Quem, Setor, Finalidade, Início (data), Início (hora), Fim previsto (data), Fim previsto (hora), Devolvido em (data), Devolvido em (hora), Duração em horas, Mês, Situação, Atrasado (*Sim* ou *Não*), Documento ligado (o número), Atendimento ligado (a referência), Tarefa ligada (*Sim* ou *Não*) |

As observações não saem. **Exportar esta lista…** (em **⋯**, na aba **Agora e próximos**) grava só os usos que a lista mostra naquele momento, com os filtros escolhidos, num arquivo que você escolhe (o nome sugerido é `usos-de-recursos.csv`).

**Em equipe.** Recursos e usos acompanham a equipe ou a pasta compartilhada como o resto do acervo (seção 9). Se duas pessoas reservam o mesmo horário do mesmo recurso, cada uma num computador sem conexão, vale a que chegar primeiro à sincronização. A outra vira um **conflito de edição**, com a explicação de quem ocupou o horário e até quando: para mantê-la, mude o horário ou a quantidade e salve de novo; ou use **Dispensar o aviso**. O mesmo acontece ao **apagar de vez** um uso ou um recurso que outra pessoa alterou nesse meio-tempo: em vez de apagar por cima, o Ponte mostra o conflito para você decidir. Os horários são os do relógio de cada computador: todos devem estar no mesmo fuso. **Esta versão muda o formato do acervo: na equipe e com o celular, atualize todos os aparelhos juntos.**

**No celular.** No Ponte de bolso (0.7.0), **Recursos** é o terceiro item do seletor, ao lado de Tarefas e Atendimentos, só para consulta: **Usos** (nos mesmos grupos de Agora e próximos) e **Recursos** (o cadastro, com a ocupação de hoje). **Hoje** e o aviso diário do celular contam reservas de hoje e devoluções atrasadas. Reservar e registrar devolução pelo celular vêm numa versão futura; por ora, use o Ponte no computador.

## 9. Mais de um computador

Em **Ajustes ▸ Equipe e sincronização**. No alto, um resumo mostra como este computador está trabalhando, a situação, a última sincronização e, nos modos que sincronizam, **Sincronizar agora**. Abaixo, quatro cartões escolhem o modo: **Só neste computador**, **Computador principal**, **Conectar ao computador principal** e **Pasta compartilhada**. Trocar de modo nunca apaga dados. Enquanto há mudanças sem salvar, aparece a etiqueta **Não salvo**.

**Você, em dois computadores (casa e trabalho).** Escolha **Pasta compartilhada**, clique em **Adicionar pasta…** e aponte para uma pasta do Google Drive, do OneDrive ou de um pendrive. Crie uma senha de pelo menos 8 caracteres e clique em **Salvar e sincronizar agora**. No outro computador, a mesma pasta e a mesma senha: em **Adicionar pasta…**, escolha o arquivo *ponte-sync.ponte* que já está lá. Ele não é apagado; na primeira sincronização, o segundo computador recebe o acervo que está no arquivo e, se já tiver documentos próprios, o Ponte pergunta antes de trocá-los. O Ponte sincroniza ao abrir, a cada 5 minutos e quando você pedir. Guarde a senha: sem ela o arquivo da pasta não abre.

**Equipe do gabinete, na rede interna.** Um computador é o principal e os outros se conectam a ele. Nada vai para a internet.

1. **No principal:** escolha **Computador principal**, clique em **Salvar e ficar disponível agora** e em **Copiar código de conexão**. Envie o código aos colegas.
2. **Nos outros:** escolha **Conectar ao computador principal**, cole o código, clique em **Usar código** e em **Salvar e sincronizar acervo agora**. Se o computador já tinha documentos próprios, faça uma cópia de segurança antes: passa a valer o acervo do gabinete.

- Sem conexão com o principal, tudo continua funcionando e é enviado quando a conexão volta.
- Se o mesmo item foi alterado nos dois lados, aparece um **conflito de edição**, no card *Edições em conflito* da mesma seção. Cada conflito diz de que item se trata (por exemplo *Tarefa: Triar documento*) e a que documento ele pertence, mostra os dois valores, campo a campo, e você escolhe **Combinar as duas versões**, **Usar só a minha versão** ou **Usar só a do computador principal** (com a pasta compartilhada, **Usar só a versão da pasta**). Quando as duas alterações são iguais, basta **Encerrar o aviso**. Se o item ou o documento dele está na Lixeira, o conflito avisa e oferece **Abrir a Lixeira**, para restaurar e voltar, ou **Dispensar o aviso**, que descarta a alteração sem mudar nada no acervo. Várias alterações do mesmo item aparecem juntas: a última é a que vale, e **Dispensar as anteriores** encerra as demais de uma vez.
- Se o principal recusou algo criado sem conexão (uma tarefa de um documento que foi para a lixeira, por exemplo), as alterações seguintes desse item aparecem como aviso (card *Alterações a enviar*), só com **Dispensar o aviso**. Refaça à mão o que ainda for necessário.
- Se os colegas não conseguirem conectar, peça à TI para liberar a porta 7878 (TCP) no firewall do computador principal. A própria página, no modo principal, traz esse aviso. **Testar conexão**, nos outros computadores, mostra o resultado logo abaixo do botão.

**Nunca** instale o Ponte nem copie o acervo para dentro do Google Drive ou do OneDrive: o acervo só abre na conta do Windows que o gravou.

## 10. Cópia de segurança, lixeira e exportação

- **Cópia de segurança:** **Ajustes ▸ Cópia de segurança e exportação ▸ Fazer cópia agora** cria uma cópia do acervo na pasta de backups. A página mostra onde as cópias ficam (**Abrir pasta**) e a data da última cópia feita nesta sessão. Ela só abre na mesma conta do Windows.
- **Lixeira:** excluir documento, tarefa, anexo ou versão importada manda para a **Lixeira** (Ajustes), de onde se restaura. Logo após excluir, o aviso do canto traz **Desfazer** por alguns segundos. Cada item da Lixeira mostra o número e o título do documento, ou o título da tarefa e o documento dela, e a caixa de busca encontra por número, título ou nome do arquivo. Por padrão nada é apagado de vez. Na seção Lixeira você pode definir um prazo, de 7 dias a 1 ano, para a limpeza automática, que o Ponte faz sozinho ao abrir e uma vez por dia. Para apagar antes disso, use **Apagar de vez…** em um item ou **Esvaziar a lixeira…** para todos. O Ponte pergunta antes, e o que for apagado de vez não volta.
- **Exportar:** **Ajustes ▸ Cópia de segurança e exportação ▸ Exportar dados para CSV…** grava documentos, tarefas, atendimentos, recursos e usos de recursos em cinco arquivos para o Excel, sem textos, observações nem anexos. Uma célula que começaria com `=`, `+`, `-` ou `@` sai com um apóstrofo (`'`) na frente, para que o Excel não a trate como fórmula; datas e números não mudam. Os dois últimos são descritos na seção 8.
- **Ao fechar** com algo não salvo, o Ponte lista o que está pendente, com um botão **Ir para** em cada item.

## 11. Se algo der errado

| Situação | O que fazer |
|---|---|
| A extensão diz que não está ligada ao Ponte | Instale a versão mais recente do Ponte, abra-o uma vez e reinicie o navegador. |
| A IA diz que o modelo não está instalado | Em Ajustes ▸ Inteligência artificial, clique em **Listar modelos do servidor** e escolha um da lista. |
| A IA diz que o documento não coube na janela do modelo | Analise sem as fontes extras ou escolha outro provedor. |
| A cota do ChatGPT acabou | Veja o saldo em ChatGPT ▸ Configurações ▸ Uso, espere a renovação ou troque de provedor. |
| O colega não conecta ao computador principal | O principal precisa estar com o Ponte aberto, na mesma rede, com a porta 7878 liberada. |
| "Não foi possível proteger/abrir os dados deste perfil Windows" | O acervo é de outra conta do Windows. Entre com a conta que fez a instalação. |
| Excluí algo sem querer | Ajustes ▸ Lixeira ▸ **Restaurar**. |
| O botão **Ponte** não aparece no WhatsApp Web | O recurso não foi ligado (ícone da extensão ▸ **Usar no WhatsApp Web**) ou a página não foi recarregada depois de ligar. Recarregue a página (F5). Exige a extensão 0.4.0. |
| "O Ponte não reconheceu esta página do WhatsApp Web" | O WhatsApp pode ter mudado a página. Abra **Colar mensagens copiadas** no painel e cole o que copiou no WhatsApp, ou use **Novo ▸ Registro manual… ▸ Colar conversa do WhatsApp** no Ponte. |
| "Atualize o Ponte para guardar conversas do WhatsApp" | O Ponte instalado é anterior à 0.32. Instale a versão mais recente e reinicie o navegador. |
| O Ponte diz que o recurso já está reservado ou em uso | Não é erro: o horário escolhido está ocupado. A frase diz por quem e até quando. Escolha outro horário, ou outro recurso, ou espere a devolução. Em **Ocupação do dia**, na ficha da reserva, aparecem os horários já tomados. |
| O Ponte diz que o arquivo ou o acervo é de uma versão mais nova | Outro computador ou o celular já está na versão nova. Instale a versão mais recente do Ponte neste computador (e a do Ponte de bolso, no celular). Nada se perdeu. |

A lista completa de mensagens e soluções está na seção "Solução de problemas" de <https://github.com/demusis/ponteproj>. A versão instalada aparece em **Ajustes ▸ Sobre o Ponte**.

**Verificar se há versão nova.** Em **Ajustes ▸ Sobre o Ponte**, clique em **Verificar se há versão nova**. Só nesse clique o Ponte consulta a página de versões no GitHub; nada do acervo é enviado, e nada é baixado nem instalado. O Ponte diz se há versão nova e, se houver, mostra **Abrir a página de download**. Feche o Ponte antes de instalar: o instalador guarda uma cópia de segurança do acervo antes de atualizar. Sem conexão com a internet, aparece um aviso e **Tentar de novo**.
