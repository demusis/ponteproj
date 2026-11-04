# Plano — Anexos em documentos e tarefas

**Objetivo:** permitir juntar arquivos do próprio computador (ofício em Word, planilha, foto de um despacho, PDF recebido por e-mail) a um documento importado ou a uma tarefa, guardados no acervo com a mesma proteção dos PDFs do SIGADOC e visíveis para a equipe.

## 1. O que a pessoa vê

**Documento ▸ Arquivos** ganha um bloco **Anexos** abaixo das cópias do SIGADOC:

- Botão **Anexar arquivo…** abre a janela de escolha do Windows (já existe no Ponte, usada na importação de planilha).
- Cada anexo aparece como cartão: nome, tipo, tamanho, quem anexou e quando, descrição opcional (uma linha, editável).
- Ações: **Abrir** (PDF abre no leitor do Ponte; os demais abrem no programa padrão do Windows a partir de uma cópia temporária), **Salvar como…**, **Excluir** (vai para a Lixeira, com restauração).

**Tarefa** (ficha no painel Tarefas e em Resumo ▸ Tarefas deste documento) ganha o mesmo bloco, compacto: contagem na lista ("📎 2") e a lista de anexos na ficha aberta.

**Rede e equipe:** anexos seguem o mesmo caminho dos PDFs importados: salvos no computador principal e visíveis em todos os computadores do gabinete. Fora de alcance, o anexo fica na fila e é enviado ao reconectar (como as demais alterações).

**Lixeira:** "anexo" vira um tipo de item, com o nome do arquivo e o documento ou tarefa de origem.

## 2. Limites e segurança

| Regra | Valor | Motivo |
|---|---|---|
| Tamanho por arquivo | 8 MiB | mesmo limite dos PDFs do SIGADOC; mantém o acervo e a sincronização leves |
| Anexos por documento ou tarefa | 20 | evita usar o Ponte como pasta de rede |
| Tipos aceitos | pdf, doc, docx, odt, rtf, txt, xls, xlsx, ods, csv, ppt, pptx, odp, png, jpg, jpeg, gif, bmp, tif, tiff, eml, msg | documentos de trabalho e imagens; **nenhum executável, script ou atalho** (.exe, .bat, .cmd, .ps1, .js, .vbs, .msi, .scr, .lnk, .zip) |
| Conteúdo | gravado cifrado com DPAPI, como todo o acervo; o nome original é guardado como metadado | mesma garantia dos demais dados |
| Abertura | cópia temporária em `%LOCALAPPDATA%\PonteProj\temp\`, apagada ao fechar o Ponte | o programa externo precisa de um arquivo em disco; a pasta fica no perfil do usuário |
| Verificação | o tipo é conferido pela extensão **e** pelos primeiros bytes (assinatura) quando o formato tem uma (PDF, PNG, JPEG, GIF, ZIP-based Office) | impede renomear um .exe para .pdf |
| Duplicata | mesmo conteúdo (SHA-256) já anexado ao mesmo item é recusado com aviso | evita cópias repetidas |

## 3. Como fica por dentro

- **Tabela nova `attachments`**: `id`, `owner_kind` ('document' | 'task'), `owner_id`, `metadata` (selado: nome, tipo, tamanho, sha256, descrição, autor, data), `payload` (selado). Índice por `(owner_kind, owner_id)`. Apagar documento ou tarefa apaga os anexos (lixeira primeiro, como hoje).
- **`src/attachments.rs`**: validação (tipos, limites, assinatura), `add_attachment`, `list_attachments`, `attachment_bytes`, `update_description`, `remove_attachment`; `labels::attachment_kind` para os rótulos.
- **Rede**: `IntranetMutation::AddAttachment { owner, metadata, data_base64 }`, `RemoveAttachment`, `UpdateAttachment`; o instantâneo do principal passa a incluir `attachments_meta` e os conteúdos, como `files_meta` hoje. Fila off-line e mesclagem de descrição (campo de texto) com as regras já existentes.
- **Lixeira**: tipo `attachment` em `trash.rs`, com restauração ao item de origem.
- **Leitor de PDF**: `pdf_reader.open` aceita a origem "anexo" (hoje só aceita `files` por `capture_id`).
- **Backup/versão de esquema**: `user_version` sobe para 10; o Ponte antigo recusa gravar num acervo novo (como já faz hoje), mas continua lendo.
- **Testes**: tipos recusados, limites, assinatura falsa, ida e volta selada, lixeira/restauração, sincronização principal→cliente com anexo (reaproveita o teste de rede existente).

## 4. Esforço e entrega

- **0.11.0**, cerca de um dia de trabalho: armazenamento + rede (metade), interface nos dois lugares + lixeira + leitor (metade).
- Sem mudança na extensão.
- Documentação: README (seção 5 e 8), GUIA_DO_USUARIO (seções 3 e 4), ATUALIZACAO_0_11_0.md.

## 5. Decisões em aberto

1. **Tamanho máximo:** 8 MiB por arquivo atende a documentos e fotos comprimidas; fotos de celular sem compressão podem passar disso. Alternativa: 20 MiB, com o acervo crescendo mais rápido.
2. **Arquivos compactados (.zip):** proposta é recusar, por poderem esconder executáveis. Se houver uso real (lotes de fotos), pode entrar com a mesma inspeção.
3. **Anexo em atendimento (Processos e demandas):** não entra nesta versão; o modelo acima permite incluir depois com `owner_kind = 'saiop'`.
