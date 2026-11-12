# Ponte de bolso (protótipo)

Aplicativo de celular (Android) que acompanha o Ponte do computador no modo **pasta compartilhada**: o celular é mais um participante do arquivo `ponte-sync.ponte` guardado no Google Drive.

## O que faz (0.7.1)

Quatro áreas numa barra no rodapé — **Hoje**, **Tarefas**, **Documentos** e **E-mail** — e telas próprias para cada coisa que se abre (ficha, texto, nota, nova tarefa, registro por foto, ajustes). Tema claro e escuro, seguindo o aparelho.

- **Hoje (0.5.0):** a tela de abertura: tarefas atrasadas, para hoje e as próximas, e os atendimentos com prazo vencido ou para hoje, pelas mesmas regras do computador.
- **Recursos (0.7.0, só consulta):** terceiro item do seletor, ao lado de Tarefas e Atendimentos. **Usos** (reservas e empréstimos de salas, equipamentos e livros) em grupos: Atrasados, Em uso agora, Hoje, Próximos dias (14 dias), Mais adiante e Encerrados; **Recursos** (o cadastro, com "N de M em uso agora"). Cada uso e cada recurso abre numa tela com todos os campos, os vínculos (documento, atendimento, tarefa) e a ocupação de hoje. Hoje e o aviso diário passam a contar "reservas de hoje" e "devoluções atrasadas" (só números). O tipo do recurso foi retirado (ele se identifica pelo nome e por "Onde fica"). Reservar e registrar devolução pelo celular estão previstos para a 0.7.2. Os dois lados do arquivo de sincronização (formato 3) precisam estar atualizados juntos.
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

Ferramentas (portáteis, sem administrador) em `%LOCALAPPDATA%\Programs\PonteProj\devtools\android`: JDK 17, Android SDK 34 com NDK r26d e Gradle 8.7; o Rust com o alvo `aarch64-linux-android`.

```
bash mobile/build-android.sh            # núcleo (arm64) + APK
bash mobile/build-android.sh --rust-only
```

O APK sai em `mobile/android/app/build/outputs/apk/release/app-release.apk` (reduzido pelo R8 e assinado com a mesma chave de depuração das versões anteriores, para instalar por cima sem perder o acervo do aparelho); instale-o no celular (por arquivo, permitindo "instalar apps desconhecidos", ou com `adb install -r`).

## Acervo de teste no computador

`mobile/rust/examples/acervo_teste.rs` faz o papel do computador com um acervo separado do real:

```
acervo_teste criar <pasta-do-acervo> <arquivo.ponte> <frase-senha>
acervo_teste ver   <pasta-do-acervo> <arquivo.ponte> <frase-senha>
```

Compile-o fora da pasta do Drive (`CARGO_TARGET_DIR` numa pasta temporária): a compilação ocupa mais de 1 GB.
