# A extensão na Chrome Web Store (item não listado)

A extensão está publicada como item **não listado**: só quem tem o link instala, e o navegador a atualiza sozinho.

- Link para instalar: <https://chromewebstore.google.com/detail/hhppmkbmainkdfmcfeaanknlnfgihnah>
- ID na loja: `hhppmkbmainkdfmcfeaanknlnfgihnah` (gravado em `extension-store-id.txt`; o instalador do Ponte autoriza este ID e o da extensão carregada da pasta, `ecllnakmppjinmednoikimbpnbgbimhg`).
- Painel do desenvolvedor: <https://chrome.google.com/webstore/devconsole> (conta `carlo.demusis@gmail.com`).
- Política de privacidade (página pública exigida pela loja): <https://demusis.github.io/ponte-privacidade/> — fonte em `extension\PRIVACIDADE.md`, repositório `demusis/ponte-privacidade`.

O Edge instala extensões da Chrome Web Store sem passo extra (basta permitir "extensões de outras lojas" na primeira vez).

## 1. Publicar uma nova versão da extensão

1. Aumente `"version"` em `extension\manifest.json` e em `extension\package.json` (a loja exige número maior que o publicado; hoje: 0.3.0; a próxima é a 0.4.0).
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
Ponte é um assistente documental para quem trabalha com o SIGADOC. Ele roda no seu próprio computador, e esta extensão é a ligação entre os dois: traz para o Ponte o documento que você está lendo no SIGADOC, com um clique.

O QUE A EXTENSÃO FAZ

Em cada documento aberto no SIGADOC (www.sigadoc.mt.gov.br) aparece o botão "Importar para o assistente". Ao clicar, o Ponte recebe o texto do documento, os PDFs anexos e o histórico de tramitação. Se o mesmo documento for importado de novo mais tarde, o Ponte guarda a nova versão e mostra o que mudou; se nada mudou, avisa e não duplica. Antes mesmo de clicar, o painel mostra se o documento já está no Ponte e permite abri-lo. Ao importar, é possível acrescentar uma nota e uma prioridade, que o Ponte guarda junto com o documento. O ícone da extensão lista as últimas importações da sessão do navegador, com acesso direto ao documento no Ponte. Atalho do teclado para importar: Alt+Shift+I (pode ser trocado em chrome://extensions/shortcuts).

O QUE VOCÊ FAZ NO PONTE

• Anota cada documento, marca prioridade e situação, e encontra tudo numa biblioteca com busca.
• Prepara minutas de resposta, despacho ou parecer a partir do próprio documento, com apoio de IA opcional: no seu computador (Ollama) ou pela sua assinatura do ChatGPT.
• Cria tarefas com prazo, responsável e lembrete; vê o que vence hoje, esta semana ou já atrasou, e quais tarefas dependem de outras.
• Acompanha atendimentos e documentos que aguardam retorno de terceiros.
• Registra também documentos que não passam pelo SIGADOC, com marca própria.
• Trabalha sozinho, em dois computadores seus (por uma pasta compartilhada, como Google Drive ou OneDrive) ou em equipe, com um computador principal na rede interna do gabinete.
• Exporta os dados principais em CSV.

COMO COMEÇAR

1. Instale o Ponte (programa para Windows, sem precisar de administrador): github.com/demusis/ponteproj/releases
2. Adicione esta extensão ao Chrome ou ao Edge.
3. Abra um documento no SIGADOC e clique em "Importar para o assistente".

A extensão só funciona com o Ponte instalado. Sem ele, o ícone da extensão avisa que o assistente não foi encontrado e explica o que fazer.

PRIVACIDADE

• A extensão lê apenas a ficha do documento que está aberto no SIGADOC. Não lê a sua senha, nem outras páginas, nem o restante da navegação.
• Ela conversa só com o Ponte instalado neste computador, por mensagens nativas do navegador. Nada é enviado para a internet, para os autores ou para terceiros.
• Ela não altera, não assina e não tramita documentos no SIGADOC. Tudo o que você faz no Ponte fica no Ponte.
• No Ponte, os dados ficam cifrados no seu computador. O uso de IA na nuvem (ChatGPT) é opcional, avisado e desligado por padrão.

Política de privacidade: demusis.github.io/ponte-privacidade

Código aberto (MIT): github.com/demusis/ponteproj
Autores: Carlo Ralph De Musis e João Gabriel Bezerra Estrela.
```

**Categoria:** Produtividade. **Idioma:** Português (Brasil). **Visibilidade:** Não listada. **Regiões:** Brasil.

**Finalidade única (single purpose):**
Enviar o documento aberto no SIGADOC ao aplicativo Ponte instalado no computador do usuário.

**Justificativa das permissões:**
- `nativeMessaging` — é o único meio de entregar o documento ao aplicativo Ponte, que roda no computador do usuário. A extensão não tem outro destino de dados.
- `storage` — usado só como memória da sessão (`chrome.storage.session`), para repetir uma importação sem baixar os arquivos de novo quando o navegador interrompe a extensão e para mostrar no ícone da extensão a lista das últimas importações (número, assunto, hora e resultado). Nada é gravado em disco; a cópia some quando a importação termina, e a lista some quando o navegador fecha ou por "Limpar a lista".
- Permissão de host `https://www.sigadoc.mt.gov.br/*` — a extensão precisa ler a página do documento aberto no SIGADOC (texto, lista de PDFs e histórico) para montar a importação. Nenhuma outra página é acessada, a não ser o WhatsApp Web, se o usuário ligar o recurso (abaixo).
- `scripting` (0.4.0) — registrar o script do painel no WhatsApp Web só depois que o usuário liga o recurso (`chrome.scripting.registerContentScripts`). Sem o recurso ligado, nenhum script é registrado nem executado em nenhuma outra página.
- Permissão de host **opcional** `https://web.whatsapp.com/*` (0.4.0) — pedida apenas quando o usuário clica em "Usar no WhatsApp Web" no ícone da extensão; quem atualiza a extensão não recebe esse acesso. Serve para mostrar o painel "Guardar no Ponte" e, só quando o usuário clica, ler as mensagens que ele escolheu na conversa aberta (com hora e autor), as fotos dessas mensagens e os arquivos que ele soltar no painel, para enviá-los ao Ponte no mesmo computador. Não lê contatos nem outras conversas, não envia nem altera nada no WhatsApp e não faz acesso à internet. O usuário pode desligar a qualquer momento ("Desligar"), o que retira a permissão.
- Não há código remoto: todo o código está no pacote.

**Uso de dados (formulário "Práticas de privacidade"):**
- Coleta: *Conteúdo do site* (o texto e os arquivos do documento aberto, lidos para a importação) e *Informações de identificação pessoal* presentes no documento oficial (nomes, matrículas). Nada é coletado fora da página do documento.
- Os dados são transmitidos somente ao aplicativo Ponte no computador do usuário, por mensagens nativas; não são enviados a servidores do desenvolvedor nem a terceiros; não são usados para publicidade, nem vendidos, nem usados para fins alheios à finalidade única; não são usados para avaliar crédito ou empréstimo.
- URL da política de privacidade: `https://demusis.github.io/ponte-privacidade/` (para alterar o texto, edite `index.html` no repositório `demusis/ponte-privacidade`).

**Imagens:** ícone 128×128 em `extension\icons\icon-128.png`; de 1 a 5 capturas de tela de 1280×800 da página do SIGADOC com o botão "Importar para o assistente" e do Ponte, sem dados reais (`dist\loja\`).

## 3. Histórico

- 2026-11-03: item criado e enviado para análise com a 0.1.8. Publicado como não listado.
- 2026-11-05: a extensão da loja só passou a funcionar com o Ponte 0.19.3: versões anteriores do programa não reconheciam o ID da loja e fechavam sem responder ("Não recebemos a confirmação do Ponte").
- 0.1.11: descrição nova e mensagens de erro específicas para cada falha de ligação com o Ponte.
- 0.2.0 (publicada; `dist\ponte-extensao-webstore-0.2.0.zip`): o painel mostra, antes do clique, se o documento já está no Ponte e permite abri-lo (exige o Ponte 0.29 ou mais novo; com um Ponte antigo a importação funciona como antes); atalho Alt+Shift+I; PDFs baixados até 3 por vez; nova permissão `storage` (memória da sessão, justificativa acima); textos revisados. Atualizar também a ficha da loja (descrição acima) e a política de privacidade pública (`PRIVACIDADE.md`).
- Próximo envio: 0.3.0 (`dist\ponte-extensao-webstore-0.3.0.zip`): nota e prioridade opcionais ao importar (exige o Ponte 0.31 ou mais novo; com um Ponte mais antigo o painel não oferece os campos e a importação funciona como antes) e lista das últimas 8 importações da sessão no ícone da extensão (memória da sessão, apagada ao fechar o navegador ou por "Limpar a lista"). Sem permissão nova: `storage` e `nativeMessaging` bastam. Atualizar a política de privacidade pública (`PRIVACIDADE.md`, 11 de novembro de 2026) e a descrição da ficha (parágrafo "O que a extensão faz").

- Próximo envio depois da 0.3.0: **0.4.0** (`dist\ponte-extensao-webstore-0.4.0.zip`): recurso **opcional** para o WhatsApp Web — "Guardar no Ponte" mensagens escolhidas da conversa aberta (trecho selecionado ou mensagens visíveis), as fotos delas e arquivos soltos no painel, ou um texto colado, como registro "Conversa do WhatsApp" (exige o Ponte 0.32 ou mais novo; com um Ponte mais antigo o ícone avisa para atualizar). Desligado até o usuário ligá-lo no ícone da extensão; nova permissão `scripting` e permissão de host opcional `https://web.whatsapp.com/*` (justificativas na seção 2). Ao atualizar, o Chrome não mostra aviso nem desativa a extensão: `scripting` não gera aviso de permissão e uma permissão de host opcional não avisa nem desativa nada. Ninguém recebe acesso ao WhatsApp sem ligar o recurso no ícone da extensão. Guardar duas vezes a mesma seleção pelo painel não duplica o registro; uma conversa guardada pelo painel e depois colada a partir da cópia do WhatsApp pode ficar guardada duas vezes, porque os dois textos não são idênticos. Atualizar antes do envio: a política de privacidade pública (`PRIVACIDADE.md`, seção "Recurso opcional: WhatsApp Web"), o campo "Descrição" (parágrafo abaixo), a justificativa das permissões e o formulário de práticas de privacidade (acrescentar *Comunicações pessoais* ao conteúdo coletado, só quando o usuário liga o recurso; mesmos destinos: somente o Ponte no computador do usuário).

**Parágrafo novo da descrição (0.4.0), depois de "O que a extensão faz":**
```
WHATSAPP WEB (OPCIONAL)

Se você quiser, pode ligar o recurso do WhatsApp Web no ícone da extensão. Ele fica desligado até você ligar. Depois, no WhatsApp Web, aparece um pequeno painel "Ponte": você seleciona mensagens de uma conversa (ou usa as que estão na tela), clica em "Guardar no Ponte", confere o que será guardado e envia. As mensagens, as fotos delas e os arquivos que você soltar no painel vão apenas para o Ponte do seu computador, como um registro "Conversa do WhatsApp"; o que o Ponte faz com ele depois, inclusive repassá-lo ao computador principal ou à pasta compartilhada da equipe, quando você configurou isso, está descrito no guia do próprio Ponte. Guardar duas vezes a mesma seleção pelo painel não duplica; uma conversa guardada pelo painel e depois colada a partir da cópia do WhatsApp pode ficar guardada duas vezes, porque os dois textos não são idênticos. A extensão só lê o que você escolher, quando você clicar; não lê contatos nem outras conversas, e não envia nem altera nada no WhatsApp. São conversas com outras pessoas: use com responsabilidade.
```

## Alternativa: política do órgão (sem loja)

Se a TI administra os computadores por GPO/Intune, a política `ExtensionInstallForcelist` do Chrome e do Edge instala a extensão para todos a partir de um `.crx` hospedado num servidor interno (`update_url` próprio). Elimina a loja e o modo desenvolvedor, mas exige empacotar e assinar o `.crx` com a chave privada correspondente a `extension-key.txt` e manter o servidor de atualização. Só faz sentido com a TI envolvida.
