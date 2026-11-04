# Atualização 0.15.0 — IA com a assinatura do ChatGPT

**Data:** 2026-11-04
**Extensão:** 0.1.9 (sem mudanças; não precisa recarregar)
**Acervo:** sem mudança de esquema (continua na versão 10). Pode atualizar um computador de cada vez.

## O que muda

- **Novo provedor em Menu ▾ ▸ Configurar IA: ChatGPT (assinatura).** Quem já paga ChatGPT Plus ou Pro pode usar essa cota nas análises e minutas do Ponte, sem chave de API. É o programa oficial "Sign in with ChatGPT" da OpenAI para aplicativos de código aberto que rodam no computador da pessoa.
- **Como entrar:** escolha a opção, clique em **Entrar com o ChatGPT…**. O navegador abre em auth.openai.com; entre com a conta da assinatura e autorize o Ponte, **inclusive o uso do plano**. A autorização volta ao Ponte por uma porta local (127.0.0.1); o Ponte não vê a senha. Depois, **Listar modelos do servidor** mostra os modelos que a sua conta oferece a aplicativos.
- **Aviso sobre a nuvem, na própria tela:** nesta opção, a cada clique em Analisar ou Gerar sugestão, o texto do documento, as notas, o contexto e as fontes selecionadas são enviados aos servidores da OpenAI, nos Estados Unidos, sob os termos da OpenAI. Nada é enviado ao importar. Use só com documentos que podem sair do órgão. O Ponte não recebe acesso às conversas, à memória nem à cobrança do ChatGPT.
- **Cota:** o uso é descontado da assinatura; o limite por aplicativo é ajustado em ChatGPT ▸ Configurações ▸ Uso. Quando acaba, o Ponte explica e **não** passa a usar outro provedor por conta própria. Conta gratuita entra, mas não libera uso do plano; o Ponte diz isso.
- **Sessão:** fica guardada no acervo, cifrada pela sua conta do Windows, e é renovada sozinha. **Sair** apaga a sessão deste computador e pede à OpenAI que a encerre. Desmarcar **Guardar a sessão do ChatGPT neste computador** mantém a sessão só até fechar o Ponte.
- Mensagens de erro em português para cada situação prevista pela OpenAI: cota esgotada, plano sem permissão, sessão encerrada nas configurações do ChatGPT, serviço indisponível.

## Para desenvolvedores

- `src/chatgpt.rs` (novo): fluxo documentado em developers.openai.com/siwc/token-sharing-open-source — registro dinâmico (`client_id=dynamic_agent_client`, `ext_agent_host_id`, `agent_name_hint`), PKCE S256, callback em `http://127.0.0.1:<porta>/auth/callback` (ouvinte TCP próprio, só GET), troca de código e renovação no endpoint de token (`resource=https://api.openai.com/v1`), verificação RS256 do ID token contra o JWKS publicado (`ring`, já compilado pelo rustls) com iss/aud/exp/nonce, escopos (`chatgpt.tokens.use.direct` libera o plano), revogação pelo `revocation_endpoint` da descoberta OIDC. Inferência em `POST /v1/responses` com `store:false`, `stream:true`, `instructions` + `input`, `text.format` json_schema (não estrito) quando há esquema; sucesso só com `response.completed`; `GET /v1/models` filtrando `visibility == "list"`. Erros mapeados por `error.code` e HTTP.
- `ai::AiSettings` ganha `kind: ApiKind` e `chatgpt: Option<SharedSession>` (Arc<Mutex<ChatGptSession>>): a renovação do token acontece na thread de trabalho sob o mutex e o app copia a sessão de volta e salva (`sync_chatgpt_session`), sem perder o refresh token rotativo. `AiConfig` ganha `chatgpt_model`, `chatgpt_host_id` e `chatgpt: ChatGptSession`; `for_storage` descarta os tokens quando a pessoa não quer guardá-los.
- `app.rs`: provedor `AiProvider::ChatGpt`, login em thread (`ponte-chatgpt-login`) com cancelamento, `Copiar endereço` se o navegador não abrir, `Entrar de novo…` para reautorizar, `Sair`.
- Testes: PKCE (vetor da RFC 7636), URL de autorização e callback, verificação de claims, utilitários de sessão, leitura do stream SSE e erros, servidor falso da Responses API (corpo documentado, bearer, 429 com cota), configuração gravada selada sem expor tokens.
- Não validado ao vivo neste computador (exige conta Plus/Pro); as mensagens de erro foram escritas para apontar o ponto exato em caso de divergência da documentação.
