# A extensão na Chrome Web Store (item não listado)

A extensão está publicada como item **não listado**: só quem tem o link instala, e o navegador a atualiza sozinho.

- Link para instalar: <https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah>
- ID na loja: `hhppmkbmainkdfmcfeaanknlnfgihnah` (gravado em `extension-store-id.txt`; o instalador do Ponte autoriza este ID e o da extensão carregada da pasta, `ecllnakmppjinmednoikimbpnbgbimhg`).
- Painel do desenvolvedor: <https://chrome.google.com/webstore/devconsole> (conta `carlo.demusis@gmail.com`).
- Política de privacidade (página pública exigida pela loja): <https://demusis.github.io/ponte-privacidade/> — fonte em `extension\PRIVACIDADE.md`, repositório `demusis/ponte-privacidade`.

O Edge instala extensões da Chrome Web Store sem passo extra (basta permitir "extensões de outras lojas" na primeira vez).

## 1. Publicar uma nova versão da extensão

1. Aumente `"version"` em `extension\manifest.json` e em `extension\package.json` (a loja exige número maior que o publicado; hoje: 0.1.10).
2. Gere o pacote: `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-webstore-package.ps1` → `dist\ponte-extensao-webstore-X.Y.Z.zip` (sem o campo `key`, como a loja exige).
3. No painel, abra o item **Ponte • Assistente SIGADOC** → aba **Pacote** (*Package*) → **Enviar novo pacote** (*Upload new package*) → escolha o `.zip`.
4. Se mudou algum texto da ficha, ajuste nas abas **Ficha da loja** (*Store listing*), **Práticas de privacidade** e **Distribuição**. O **Resumo** vem do campo `description` do manifesto e não é editável no painel: para mudá-lo, mude o manifesto e envie um pacote novo.
5. **Enviar para análise** (*Submit for review*). Deixe marcada a publicação automática após a análise (ou publique à mão em até 30 dias). Enquanto a análise não termina, a versão publicada continua no ar.
6. A análise de uma atualização é como a de um item novo: em geral de algumas horas a 3 dias úteis; `nativeMessaging` às vezes gera perguntas, respondidas com a justificativa da seção 2.
7. Depois de publicada, o Chrome e o Edge atualizam a extensão sozinhos em algumas horas (ou na hora, em `chrome://extensions` → **Atualizar**). O ID não muda; nada precisa ser refeito no Ponte.

Só mudar textos da ficha (descrição, capturas de tela) também passa pela análise, mas não exige pacote novo.

## 2. Textos da ficha e respostas de privacidade

**Nome:** Ponte • Assistente SIGADOC

**Resumo (vem do manifesto; até 132 caracteres):**
Leva o documento aberto no SIGADOC para o Ponte, o assistente documental do seu computador: notas, minutas, prazos e equipe.

**Descrição (campo "Descrição" da ficha; texto puro, sem formatação):**
```
O Ponte é um assistente documental que roda no seu próprio computador. Esta extensão é a ponte entre ele e o SIGADOC.

O QUE ELA FAZ
Em cada documento aberto no SIGADOC (www.sigadoc.mt.gov.br) aparece o botão "Importar para o assistente". Um clique leva para o Ponte o texto do documento, os PDFs e o histórico de tramitação. No Ponte você anota, prepara minutas, acompanha prazos e tarefas e, se o gabinete quiser, trabalha em equipe pela rede interna ou por uma pasta compartilhada.

COMO USAR
1. Instale o Ponte (programa para Windows): github.com/demusis/ponteproj/releases
2. Adicione esta extensão ao Chrome ou ao Edge.
3. Abra um documento no SIGADOC e clique em "Importar para o assistente".

PRIVACIDADE
A extensão só lê a ficha do documento aberto e só conversa com o Ponte instalado neste computador. Nada é enviado para a internet nem para os autores. Ela não lê a sua senha nem outras páginas, e não altera, assina nem tramita documentos no SIGADOC.

Requer o Ponte instalado. Sem ele, a extensão avisa que o assistente não foi encontrado e explica o que fazer.
```

**Categoria:** Produtividade. **Idioma:** Português (Brasil). **Visibilidade:** Não listada. **Regiões:** Brasil.

**Finalidade única (single purpose):**
Enviar o documento aberto no SIGADOC ao aplicativo Ponte instalado no computador do usuário.

**Justificativa das permissões:**
- `nativeMessaging` — é o único meio de entregar o documento ao aplicativo Ponte, que roda no computador do usuário. A extensão não tem outro destino de dados.
- Permissão de host `https://www.sigadoc.mt.gov.br/*` — a extensão precisa ler a página do documento aberto no SIGADOC (texto, lista de PDFs e histórico) para montar a importação. Nenhuma outra página é acessada.
- Não há código remoto: todo o código está no pacote.

**Uso de dados (formulário "Práticas de privacidade"):**
- Coleta: *Conteúdo do site* (o texto e os arquivos do documento aberto, lidos para a importação) e *Informações de identificação pessoal* presentes no documento oficial (nomes, matrículas). Nada é coletado fora da página do documento.
- Os dados são transmitidos somente ao aplicativo Ponte no computador do usuário, por mensagens nativas; não são enviados a servidores do desenvolvedor nem a terceiros; não são usados para publicidade, nem vendidos, nem usados para fins alheios à finalidade única; não são usados para avaliar crédito ou empréstimo.
- URL da política de privacidade: `https://demusis.github.io/ponte-privacidade/` (para alterar o texto, edite `index.html` no repositório `demusis/ponte-privacidade`).

**Imagens:** ícone 128×128 em `extension\icons\icon-128.png`; de 1 a 5 capturas de tela de 1280×800 da página do SIGADOC com o botão "Importar para o assistente" e do Ponte, sem dados reais (`dist\loja\`).

## 3. Histórico

- 2026-11-03: item criado e enviado para análise com a 0.1.8. Publicado como não listado.
- Próximo envio: 0.1.10 (`dist\ponte-extensao-webstore-0.1.10.zip`) — só muda o texto da descrição.

## Alternativa: política do órgão (sem loja)

Se a TI administra os computadores por GPO/Intune, a política `ExtensionInstallForcelist` do Chrome e do Edge instala a extensão para todos a partir de um `.crx` hospedado num servidor interno (`update_url` próprio). Elimina a loja e o modo desenvolvedor, mas exige empacotar e assinar o `.crx` com a chave privada correspondente a `extension-key.txt` e manter o servidor de atualização. Só faz sentido com a TI envolvida.
