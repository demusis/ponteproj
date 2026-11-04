# Correção da extensão 0.1.7 — painéis laterais

Em 03/11/2026 foi conferida em leitura a captura 7 (adaptador 1.6.0). Suporte, data documental, De/origem, cadastrante, espécie, modelo e classificação oficial estavam salvos. Para estava vazio na origem e foi omitido. Descrição permanece no título/assunto da captura.

O painel Nível de Acesso não estava capturado: o extrator anterior considerava apenas nós textuais diretos e perdia o nível dentro de b/strong. A lista de pessoas também não era coletada.

A extensão 0.1.7, adaptador 1.6.1, lê o nível visível mesmo quando aninhado ou em negrito, separando-o das listas. Os identificadores explicitamente listados são preservados em access_people. Conteúdo oculto, formulários e botões são excluídos. Lista não reconhecida ou excessiva é indicada nas pendências sem captura parcial silenciosa.

Consulte em Ficha e análise → Metadados obtidos na página: medium, document_date, origin, registered_by, document_type, document_model, classification, access_level e access_people. A classificação oficial é diferente dos assuntos/prioridade sugeridos pela IA ou confirmados localmente.

29 testes da extensão passaram. O aplicativo permanece 0.7.0 e o banco schema 7; nenhum conteúdo existente foi alterado. É necessário recarregar a extensão e reimportar para gravar os campos corrigidos. A correção não concede nem altera acesso no SIGADOC.

A mesma captura #7 confirmou quatro eventos reais salvos, sem pendência de histórico. Após recarga e reimportação pelo usuário, a captura #8 (adaptador 1.6.1) confirmou access_level = Limitado entre pessoas e os dois códigos explícitos da lista. Todos os campos preenchidos do primeiro painel também foram conferidos; quatro eventos continuaram presentes. Integridade e chaves estrangeiras passaram.
