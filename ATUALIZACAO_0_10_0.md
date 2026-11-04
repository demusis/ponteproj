# Atualização 0.10.0 — Ollama Cloud e configuração de IA salva

**Data:** 2026-11-04
**Extensão:** 0.1.8 (sem mudanças; não precisa recarregar)

> **Correção da 0.9.3:** o instalador `Ponte-Setup-0.9.3.exe` foi gerado com o programa da 0.9.2 (a pasta `v0.9.3` instalada responde "PonteProj 0.9.2"). As correções descritas em ATUALIZACAO_0_9_3.md chegam de fato nesta versão. Quem instalou a 0.9.3 deve instalar a 0.10.0.

## O que muda

- **Configuração de IA salva.** Endereço, modelo e chave passam a ser gravados no acervo, cifrados com a DPAPI da conta do Windows (como o restante dos dados). Não é mais preciso reinformá-los a cada abertura. A opção **Guardar a chave neste computador** pode ser desmarcada para manter a chave só na memória da sessão.
- **Ollama Cloud como opção pronta.** Em Menu ▾ ▸ Configurar IA, a opção **Ollama Cloud** já traz o endereço fixo (ollama.com); basta colar a chave de API criada em ollama.com/settings/keys. A janela avisa que os textos saem do computador para os servidores da Ollama.
- **Modelo escolhido numa caixa de seleção.** O botão **Listar modelos do servidor** consulta o servidor configurado (local, Ollama Cloud ou outro) e lista os modelos que ele oferece, com o tamanho quando informado. A lista é atualizada ao abrir a janela e ao trocar de provedor. Se o servidor não responder, o nome do modelo pode ser digitado como antes.
- **Salvar ao fechar.** As alterações são salvas ao clicar em **Salvar** ou ao fechar a janela; uma configuração inválida mantém a janela aberta com a explicação. Configuração não salva aparece na lista de pendências ao fechar o Ponte.
- A linha "envia dados ao …" ao lado dos botões de IA passa a distinguir Ollama Cloud de outro servidor.

## Para desenvolvedores

- `ai::AiConfig` (provedor, endereços, modelo por provedor, chave e token) é persistida na tabela `ai_config` via `Store::load_ai_config` / `save_ai_config`, sempre selada com `protection::seal`. `AiConfig` não deriva `Debug` para a chave não chegar a logs.
- `ai::list_models` chama `GET /api/tags` com o mesmo validador de endpoint do chat (`endpoint(settings, route)`); `parse_tags` ordena, remove duplicatas e descarta nomes inválidos.
- Testes novos em `ai::tests`: `model_list_is_parsed_sorted_and_cleaned` e `config_is_saved_sealed_and_keys_follow_the_remember_choice` (o blob gravado não contém a chave nem o modelo em claro).
