# Ponte de bolso (protótipo)

Aplicativo de celular (Android) que acompanha o Ponte do computador no modo **pasta compartilhada**: o celular é mais um participante do arquivo `ponte-sync.ponte` guardado no Google Drive.

## O que faz (0.3.0)

Três áreas numa barra no rodapé — **Tarefas**, **Documentos** e **E-mail** — e telas próprias para cada coisa que se abre (ficha, texto, nota, nova tarefa, registro por foto, ajustes). Tema claro e escuro, seguindo o aparelho.

- **Tarefas:** atrasadas, para hoje, próximas, sem prazo e concluídas; marcar conclui na hora, com "Desfazer"; tocar no texto abre a ficha do documento.
- **Documentos:** busca e filtros (todos, com tarefa, e-mails, registros); ficha com tarefas, nota, resumo da análise, PDFs e anexos (abertos no leitor do celular); o texto completo abre numa tela de leitura.
- **Registro por foto:** o botão abre a câmera; depois, um formulário curto cria o registro manual com a foto anexada (redimensionada, abaixo do limite de anexos).
- **E-mail:** a mesma importação por IMAP do computador (contas, pastas, filtro, mensagens marcadas), com o resultado mostrado na própria lista.
- **Sincronização:** ao abrir e pelo botão, só no Wi-Fi por padrão; frase-senha guardada pelo Android Keystore e liberada pela digital ou PIN (Android 9 ou mais novo). Na primeira vez, um passo a passo pede o arquivo, a frase-senha e faz a primeira sincronização.

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
