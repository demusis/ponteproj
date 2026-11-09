# Ponte de bolso (protótipo)

Aplicativo de celular (Android) que acompanha o Ponte do computador no modo **pasta compartilhada**: o celular é mais um participante do arquivo `ponte-sync.ponte` guardado no Google Drive.

## O que este protótipo provou (09/11/2026)

- O mesmo núcleo em Rust do Ponte roda no celular (`libponte_mobile.so`), sem a interface do computador (o Ponte compila com `--no-default-features`).
- O celular lê e grava o `ponte-sync.ponte` pelo seletor de arquivos do Android, através do app do Google Drive, sem permissão de internet nem login próprio.
- Uma nota escrita no celular chegou ao Ponte do computador, sem conflito.
- O acervo do celular fica selado com AES-256-GCM; a chave é guardada cifrada pelo Android Keystore.
- O celular só considera uma alteração entregue depois de reler o arquivo no Drive e conferir; até lá, guarda a cópia em `pendente.ponte` e a reenvia na sincronização seguinte.

## Limite conhecido (precisa ser resolvido antes do uso real)

O app do Drive no celular trabalha com uma cópia em cache. Se o computador gravar o arquivo e o celular ainda não tiver recebido essa versão, o celular pode sobrescrevê-la. A correção é no núcleo de sincronização (valendo também para o computador): manter cada alteração enviada na fila até que o `applied` de uma cópia lida mostre que ela chegou.

## Como montar

Ferramentas (portáteis, sem administrador) em `%LOCALAPPDATA%\Programs\PonteProj\devtools\android`: JDK 17, Android SDK 34 com NDK r26d e Gradle 8.7; o Rust com o alvo `aarch64-linux-android`.

```
bash mobile/build-android.sh            # núcleo (arm64) + APK de depuração
bash mobile/build-android.sh --rust-only
```

O APK sai em `mobile/android/app/build/outputs/apk/debug/app-debug.apk`; instale-o no celular (por arquivo, permitindo "instalar apps desconhecidos", ou com `adb install -r`).

## Acervo de teste no computador

`mobile/rust/examples/acervo_teste.rs` faz o papel do computador com um acervo separado do real:

```
acervo_teste criar <pasta-do-acervo> <arquivo.ponte> <frase-senha>
acervo_teste ver   <pasta-do-acervo> <arquivo.ponte> <frase-senha>
```

Compile-o fora da pasta do Drive (`CARGO_TARGET_DIR` numa pasta temporária): a compilação ocupa mais de 1 GB.
