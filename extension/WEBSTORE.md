# Publicar a extensão na Chrome Web Store (não listada)

Publicar na loja elimina o "Modo do desenvolvedor" e o "Carregar sem compactação": o servidor abre um link, clica em **Adicionar ao Chrome** e a extensão se atualiza sozinha. O Edge instala extensões da Chrome Web Store sem passo extra (basta permitir "extensões de outras lojas" na primeira vez).

O envio exige uma conta Google de desenvolvedor da Chrome Web Store. Esse passo é seu (ou da TI do órgão); o restante está pronto.

## 1. O que já está pronto

| Item | Onde |
|---|---|
| Pacote `.zip` para envio (sem o campo `key`, como a loja exige) | `scripts\build-webstore-package.ps1` → `dist\ponte-extensao-webstore-0.1.8.zip` |
| Textos da ficha (abaixo) | esta página |
| Política de privacidade (página pública) | <https://demusis.github.io/ponte-privacidade/> — fonte em `extension\PRIVACIDADE.md`; repositório `github.com/demusis/ponte-privacidade` |
| Ícone 128×128 e imagens | `extension\icons\icon-128.png`; capturas de tela: tire 1 a 5 imagens de 1280×800 da página do SIGADOC com o botão "Importar para o assistente" (sem dados reais) |
| Autorização do novo ID no Ponte | `extension-store-id.txt` + `scripts\build-installer.ps1` (ver seção 4) |

## 2. Enviar (uma vez)

1. Entre em <https://chrome.google.com/webstore/devconsole> com a conta Google que será a dona da extensão (de preferência uma conta institucional, não pessoal). Pague a taxa única de registro de desenvolvedor (US$ 5) e confirme o e-mail de contato.
2. **Novo item** → envie `dist\ponte-extensao-webstore-0.1.8.zip`.
3. **Ficha da loja** — copie os textos da seção 3. Categoria: *Produtividade*. Idioma: *Português (Brasil)*.
4. **Práticas de privacidade** — respostas na seção 3.
5. **Distribuição** → **Visibilidade: Não listada** (só quem tem o link instala). Regiões: Brasil.
6. **Enviar para análise**. A análise costuma levar de 1 a 3 dias úteis; itens com `nativeMessaging` às vezes recebem perguntas, respondidas pela justificativa abaixo.
7. Depois de publicada, abra a página do item: o endereço termina com o **ID de 32 letras**. Copie-o.

## 3. Textos da ficha e respostas de privacidade

**Nome:** Ponte • Assistente SIGADOC

**Resumo (até 132 caracteres):**
Importa documentos do SIGADOC para o Ponte, o assistente documental instalado no seu computador.

**Descrição:**
```
A extensão Ponte adiciona, em cada documento aberto no SIGADOC (www.sigadoc.mt.gov.br), o botão "Importar para o assistente". Ao clicar, o texto, os PDFs e o histórico de tramitação daquele documento são enviados ao Ponte, um programa instalado no próprio computador, onde o servidor anota, prepara minutas, acompanha prazos e, se o gabinete quiser, trabalha em equipe na rede interna.

A extensão só funciona com o Ponte instalado. Ela não envia nada para a internet, não lê a sua senha nem outras páginas, e não altera, assina nem tramita documentos no SIGADOC.
```

**Finalidade única (single purpose):**
Enviar o documento aberto no SIGADOC ao aplicativo Ponte instalado no computador do usuário.

**Justificativa das permissões:**
- `nativeMessaging` — é o único meio de entregar o documento ao aplicativo Ponte, que roda no computador do usuário. A extensão não tem outro destino de dados.
- Permissão de host `https://www.sigadoc.mt.gov.br/*` — a extensão precisa ler a página do documento aberto no SIGADOC (texto, lista de PDFs e histórico) para montar a importação. Nenhuma outra página é acessada.
- Não há código remoto: todo o código está no pacote.

**Uso de dados (formulário "Práticas de privacidade"):**
- Coleta: *Conteúdo do site* (o texto e os arquivos do documento aberto, lidos para a importação) e *Informações de identificação pessoal* presentes no documento oficial (nomes, matrículas). Nada é coletado fora da página do documento.
- Os dados são transmitidos somente ao aplicativo Ponte no computador do usuário, por mensagens nativas; não são enviados a servidores do desenvolvedor nem a terceiros; não são usados para publicidade, nem vendidos, nem usados para fins alheios à finalidade única; não são usados para avaliar crédito ou empréstimo.
- URL da política de privacidade: `https://demusis.github.io/ponte-privacidade/` (GitHub Pages; para alterar o texto, edite `index.html` no repositório `demusis/ponte-privacidade`).

## 4. Depois da publicação: autorizar o novo ID no Ponte

A loja atribui à extensão um ID próprio, diferente do ID da versão carregada da pasta (`ecllnakmppjinmednoikimbpnbgbimhg`). O Ponte só aceita mensagens dos IDs autorizados no manifesto do host nativo, por isso:

1. Crie o arquivo `extension-store-id.txt` na raiz do projeto com o ID copiado no passo 2.7 (32 letras, nada mais).
2. Gere o instalador de novo: `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1`. O instalador passa a autorizar os dois IDs (pasta e loja).
3. Distribua o novo `dist\Ponte-Setup-X.Y.Z.exe` e o link da loja. Quem já tinha a extensão carregada da pasta deve removê-la para não ficar com duas.

Enquanto o `extension-store-id.txt` não existir, o instalador autoriza apenas a extensão carregada da pasta — o comportamento atual.

## 5. Atualizações seguintes

A cada nova versão da extensão: aumente `"version"` em `extension\manifest.json`, rode `scripts\build-webstore-package.ps1`, envie o novo `.zip` no painel e publique. Os navegadores atualizam sozinhos em algumas horas. O ID não muda.

## Alternativa: política do órgão (sem loja)

Se a TI administra os computadores por GPO/Intune, a política `ExtensionInstallForcelist` do Chrome e do Edge instala a extensão para todos a partir de um `.crx` hospedado num servidor interno (`update_url` próprio). Elimina a loja e o modo desenvolvedor, mas exige empacotar e assinar o `.crx` com a chave privada correspondente a `extension-key.txt` e manter o servidor de atualização. Só faz sentido com a TI envolvida.
