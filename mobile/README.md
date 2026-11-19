# Ponte de bolso (protótipo)

Aplicativo de celular (Android) que acompanha o Ponte do computador no modo **pasta compartilhada**: o celular é mais um participante do arquivo `ponte-sync.ponte` guardado no Google Drive.

## O que faz (0.15.0)

- **Dependências:** No celular, ao editar uma tarefa, em **Depende de**, **Escolher tarefas…** abre **Escolher tarefas**: marque várias, use **Buscar tarefa** e **Confirmar**; as concluídas aparecem com **· Concluída**. Tarefas que criariam ciclo não são oferecidas; o núcleo ainda recusa escolhas inválidas e o formulário mostra o motivo. Essa escolha não está em **Nova tarefa**.

- **Sincronização pedida por você:** em **Sincronizar agora**, na tela de documento ou no diálogo de status, ela acontece ou informa por que não; sem Wi-Fi, oferece **Usar dados móveis desta vez**. As mensagens podem ser: “Escolha primeiro o arquivo ponte-sync.ponte em Ajustes.”, “Informe a senha de sincronização em Ajustes para sincronizar.”, “Abra o aplicativo para liberar a senha e sincronizar.”, “Sincronização cancelada: a senha não foi liberada.” ou “Já há uma sincronização em andamento.” As sincronizações automáticas continuam silenciosas. A versão 0.10.0 instala por cima da 0.9.x.

Quatro áreas numa barra no rodapé — **Hoje**, **Tarefas**, **Documentos** e **E-mail** — e telas próprias para cada coisa que se abre (ficha, texto, nota, nova tarefa, registro por foto, ajustes). Tema claro e escuro, seguindo o aparelho.

- **Hoje (0.5.0):** a tela de abertura: tarefas atrasadas, para hoje e as próximas, e os atendimentos com prazo vencido ou para hoje, pelas mesmas regras do computador.
- **Primeira sincronização (0.9.1):** se ela não acontece, o passo 3 diz por quê (por exemplo, *sem Wi-Fi* ou digital não liberada), em vez de só mostrar **Tentar de novo**. Sem Wi-Fi, oferece **Usar dados móveis desta vez**, sem mudar Ajustes.
- **Planejamento e avisos (0.9.0):** acompanha o Ponte 0.39.0 (formato 19/6; atualize o computador principal primeiro). Na tarefa e em **Nova tarefa**: **Início planejado**, **Fim planejado**, a estimativa e **Avisos antes do início** (as opções atuais estão na 0.13.0, abaixo). Cada aviso é uma notificação do Android, com o botão **Adiar 10 min**, e **não traz o título da tarefa**; exige a permissão de notificações, e a tarefa avisa quando ela falta. Em **Tarefa pai**, **Escolher tarefa pai…** busca a tarefa a que esta pertence (só as abertas; nem ela mesma nem as subtarefas dela aparecem), e **Sem tarefa pai** desfaz o vínculo. A tarefa pai mostra **Subtarefas · N de M concluídas**; tocar numa subtarefa a abre. Em **Hoje**, a semana aparecia em lista (tarefas planejadas e reservas por dia; a versão atual está na 0.15.0, abaixo); uma tarefa que espera outra mostra **↳ depende de N**, ou **⚠ começa antes de terminar <título>** quando a ordem está invertida, e a tarefa mostra **Depende de**, com ○, ⚠ e ✓. Ainda não tem no celular: as setas entre tarefas da Semana, a repetição de tarefas e reservas e o calendário `.ics`.
- **Planejamento como o prazo (0.13.0; em Nova tarefa desde a 0.14.0):** ao editar a tarefa e, desde a 0.14.0, em **Nova tarefa**, **Início** e **Fim** se escolhem como o **Prazo**, cada um com uma caixa **Hora** opcional (em branco: 09:00 no início, 18:00 no fim). **Estimativa em dias**; com início e estimativa e o fim vazio, o fim é proposto. Se o fim passa do prazo, aparece *⚠ O fim planejado passa do prazo*, sem impedir de salvar. **Avisos antes do início planejado:** **No início**, **1 dia antes** e **7 dias antes**; avisos de versões anteriores aparecem com *(antigo)*.
- **Hoje (0.15.0):** no alto, uma linha de resumo (*Nada vence hoje.* ou o que vence) e, se houver, *⚠ N atrasadas · ver*, que leva à lista. Depois, a semana: **‹** e **›** (ou deslizar a faixa) mudam de semana, e **Hoje** volta para a atual. A faixa mostra os sete dias com pontinhos (azul: trabalho planejado ou reserva; vermelho: prazo). Toque num dia para ver a agenda dele embaixo, com o título inteiro e, abaixo, o horário e os avisos de dependência. Dia sem nada diz *Dia livre.* Tocar numa linha abre a tarefa ou a reserva. As listas Atrasadas, Para hoje e Próximos dias vêm depois.
- **Categorias (0.8.0):** o celular mostra e edita as categorias de tarefas e documentos (uma lista só para os dois, até 10 por item, com as mesmas dez cores do computador). Etiquetas nas linhas de Hoje, Tarefas, Documentos e na ficha; o campo **Categorias** na tela da tarefa (parte do rascunho, salvo com **Salvar**), em **Nova tarefa** e na ficha do documento (salvo na hora); um seletor com busca em que se pode **criar** a categoria na hora (só se nenhuma tiver o mesmo nome, sem contar maiúsculas e acentos); e o filtro por categoria em Tarefas e em Documentos (qualquer uma das marcadas, ou **Sem categoria**). Renomear, mudar a cor, arquivar e apagar continuam no computador. Se a ficha mudou depois de aberta, o aplicativo recusa e mostra a versão atual. Formato 17/5, o mesmo do Ponte 0.37.0.
- **Recursos (0.7.0, só consulta):** terceiro item do seletor, ao lado de Tarefas e Atendimentos. **Usos** (reservas e empréstimos de salas, equipamentos e livros) em grupos: Atrasados, Em uso agora, Hoje, Próximos dias (14 dias), Mais adiante e Encerrados; **Recursos** (o cadastro, com "N de M em uso agora"). Cada uso e cada recurso abre numa tela com todos os campos, os vínculos (documento, atendimento, tarefa) e a ocupação de hoje. Hoje e o aviso diário passam a contar "reservas de hoje" e "devoluções atrasadas" (só números). O tipo do recurso foi retirado (ele se identifica pelo nome e por "Onde fica"). Reservar e registrar devolução pelo celular estão previstos para uma versão futura. Os dois lados do arquivo de sincronização (formato 4, desde o Ponte 0.34.0 e o Ponte de bolso 0.7.2) precisam estar atualizados juntos.
- **Tarefa (0.5.0):** tocar numa tarefa abre a tela dela: título, ação, prioridade, prazo, responsável e observações; concluir ou reabrir; vincular a um documento, trocar de documento ou desvincular. **Nova tarefa**, na aba Tarefas, cria uma tarefa sem documento. Se a tarefa mudou depois de aberta (em outro aparelho, por exemplo), o aplicativo avisa e não grava por cima.
- **Novidades da 0.6.0:** seu nome em Ajustes, botão **Eu** e sugestões de responsável; **atendimentos** (lista e ficha, só leitura) no seletor Tarefas | Atendimentos; **busca em tudo** (documentos com texto e notas, tarefas, atendimentos); **aviso diário de prazos** opcional, só com quantidades; **Anexar…** na ficha (foto ou arquivo); e o aplicativo **recebe arquivos, fotos e textos compartilhados** por outros aplicativos (tela Guardar no Ponte), sempre depois do bloqueio. O APK passa a ser assinado com chave própria, guardada fora do projeto.
- **Tarefas:** atrasadas, para hoje, próximas, sem prazo e concluídas; marcar conclui na hora, com "Desfazer"; tocar no texto abre a ficha do documento.
- **Documentos:** busca e filtros (todos, com tarefa, e-mails, registros); ficha com tarefas, nota, resumo da análise, PDFs e anexos (abertos no leitor do celular); o texto completo abre numa tela de leitura.
- **Registro por foto:** o botão abre a câmera; depois, um formulário curto cria o registro manual com a foto anexada (redimensionada, abaixo do limite de anexos).
- **E-mail:** a mesma importação por IMAP do computador (contas, pastas, filtro, mensagens marcadas), com o resultado mostrado na própria lista.
- **Sincronização:** ao abrir e pelo botão, só no Wi-Fi por padrão; senha de sincronização guardada pelo Android Keystore e liberada pela digital ou PIN (Android 9 ou mais novo). Na primeira vez, um passo a passo pede o arquivo, a senha e faz a primeira sincronização. O arquivo do Drive é lido e gravado em fluxo (não é carregado inteiro na memória do aplicativo).
- **Segurança (0.4.0):** o aplicativo pede a digital ou o bloqueio de tela do celular ao abrir e depois de um tempo fora (imediatamente, 1 ou 5 minutos); capturas de tela e a prévia na lista de aplicativos recentes ficam bloqueadas por padrão; os arquivos entregues a outros aplicativos (PDFs, anexos, fotos) são apagados ao bloquear, ao abrir e ao voltar do leitor. O bloqueio é uma porta na interface: o acervo já fica selado por uma chave do Android Keystore.
- **Textos (0.4.0):** mesmas palavras do Ponte do computador; erros dizem o que aconteceu e o que fazer; mensagens do núcleo escritas para o computador são adaptadas para o aparelho (`PhoneWords`).

A interface é feita com Jetpack Compose e Material 3. Cada tela é uma função do seu estado; `gradle recordPaparazziDebug` desenha todas elas no computador, com dados de exemplo, em `mobile/android/app/src/test/snapshots/images` (sem celular nem emulador).

## O que este protótipo provou (09/11/2026)

- O mesmo núcleo em Rust do Ponte roda no celular (`libponte_mobile.so`), sem a interface do computador (o Ponte compila com `--no-default-features`).
- O celular lê e grava o `ponte-sync.ponte` pelo seletor de arquivos do Android, através do app do Google Drive, sem permissão de internet nem login próprio.
- Uma nota escrita no celular chegou ao Ponte do computador, sem conflito.
- O acervo do celular fica selado com AES-256-GCM; a chave é guardada cifrada pelo Android Keystore.
- O celular só considera uma alteração entregue depois de reler o arquivo no Drive e conferir; até lá, guarda a cópia em `pendente.ponte` e a reenvia na sincronização seguinte.

## Gravação sobre uma versão antiga (resolvido na 0.24.0)

O app do Drive no celular trabalha com uma cópia em cache: o celular pode gravar o arquivo a partir de uma versão que o computador já tinha substituído. Desde a 0.24.0, cada participante guarda a última cópia que gravou (`acervo.sqlite3-guarda.ponte`) e a lê junto com a da pasta; o que foi sobrescrito volta na sincronização seguinte. Computador e celular precisam estar na 0.24.0 ou posterior.

## Como montar

Ferramentas (portáteis, sem administrador) em `%LOCALAPPDATA%\Programs\PonteProj\devtools\android`: JDK 17, Android SDK 34 com NDK r26d e o wrapper Gradle do projeto; o Rust com o alvo `aarch64-linux-android`.

```
bash mobile/build-android.sh            # núcleo (arm64) + APK
bash mobile/build-android.sh --rust-only
cd mobile/android && ./gradlew testDebugUnitTest
```

O APK sai em `mobile/android/app/build/outputs/apk/release/app-release.apk` (reduzido pelo R8 e assinado com a chave própria em `~/.ponte-assinatura/`, fora do projeto; sem ela, o build usa a chave de depuração e avisa). **A chave mudou na 0.9.0:** quem tinha uma versão anterior precisa sincronizar, desinstalar e instalar de novo; daí em diante as atualizações instalam por cima; instale-o no celular (por arquivo, permitindo "instalar apps desconhecidos", ou com `adb install -r`).

## Acervo de teste no computador

`mobile/rust/examples/acervo_teste.rs` faz o papel do computador com um acervo separado do real:

```
acervo_teste criar <pasta-do-acervo> <arquivo.ponte> <frase-senha>
acervo_teste ver   <pasta-do-acervo> <arquivo.ponte> <frase-senha>
```

Compile-o fora da pasta do Drive (`CARGO_TARGET_DIR` numa pasta temporária): a compilação ocupa mais de 1 GB.
