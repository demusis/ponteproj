# Versão 0.6.0 — dossiês, histórico e leitura de PDFs

Aplicativo 0.6.0, extensão 0.1.6, adaptador 1.6.0, schema 6.

## Dossiê e histórico

A nova aba reúne vínculos locais confirmados, eventos capturados do SIGADOC e versões imutáveis das capturas.

Vínculos são direcionados: o documento escolhido é antecedente, resposta ou relacionado ao documento atual. Exigem outro documento importado e uma justificativa. Vínculos podem ser desativados e reativados, preservando o registro. Não representam juntada, ciência ou tramitação oficial. Exclusão de um documento remove seus vínculos locais.

Cada captura preservada mostra data de obtenção, data de salvamento, URL de origem, texto, referências e hashes dos PDFs. A comparação de capturas indica mudanças em título, metadados, referências e texto; permite consultar antes/depois. Mudanças no adaptador também alteram a captura e não comprovam alteração do original.

## Eventos do SIGADOC

O acesso Ver Mais foi inspecionado em leitura na ficha real. A extensão só consulta a rota de visualização detalhada quando o link explícito corresponde ao mesmo documento. A consulta usa a sessão do navegador, sem seguir redirecionamentos, com limite de 2 MiB e 15 segundos. Falhas registram pendência sem descartar texto e PDFs.

O adaptador exige o cabeçalho exato da tabela, preserva a identificação da via/Geral, data como exibida, evento, cadastrante/lotação, atendente/lotação e descrição. Para pessoas, preserva o nome completo e código do tooltip quando único e explícito. Não infere datas de linhas vizinhas nem identifica cadastrante como remetente. Máximo de 200 eventos e 60 KB de JSON; excesso ou estrutura desconhecida recusam o histórico integral, sem corte silencioso. Não há auditoria remota, acionamento de tramitação ou leitura de conteúdo oculto por classes/atributos reconhecidos.

Para ativar: recarregue manualmente a extensão, atualize a ficha principal e reimporte.

## Leitor e OCR

Em Ficha e análise, use **Ler PDF no aplicativo**. A renderização usa a API nativa do Windows e processa os bytes protegidos em memória, sem exportar PDF ou imagem legível para disco. Há navegação por página e ajuste de resolução. A renderização ocorre em segundo plano e respeita limites de imagem/memória.

**OCR local desta página** usa os idiomas OCR disponíveis no perfil Windows. O resultado é guardado separado do original, protegido por DPAPI e vinculado ao arquivo/página, com idioma e data. Confira com a imagem: OCR pode errar. O resultado ainda não entra automaticamente na análise por IA nem na busca textual nesta versão. A exportação explícita de PDF continua disponível.

## Verificações

27 testes Rust passaram (um teste adicional de IA real permaneceu ignorado). A suíte da extensão passou em 25 testes. O leitor nativo renderizou o PDF fictício e recusou página inválida; o teste OCR real local retornou 39 caracteres em pt-BR para o mesmo PDF fictício. Testes verificaram proteção e exclusão em cascata do OCR, identidade do histórico, datas em branco, limites e vínculos.

O executável empacotado passou na validação nativa/PDF/idempotência/schema 6. SHA256: `BB2289422396FEABF7487ACB60A521AD932792438D8DAB71C331D6B7FC5788FF`.

Backup antes da migração: `%LOCALAPPDATA%\PonteProj\backups\pre-v060-20261102-205442-538195.sqlite3`.

A instalação usa `v0.6.0` e preserva janelas antigas; salve suas edições nelas antes de fechar. Versões anteriores recusam o banco após a migração.

## Continuidade aprovada

Próxima etapa: seleção de antecedentes e páginas para a IA, citações verificadas, modelos próprios, comparação/exportação de minutas e vínculos com a resposta oficial. O agrupamento por processo deve distinguir vínculo confirmado de simples coincidência de metadados.

Migração real conferida: seis capturas, uma tarefa e todos os registros criptografados anteriores preservados, sem erros de integridade ou chaves estrangeiras. A ficha confirmou Ciência e download preservada.
