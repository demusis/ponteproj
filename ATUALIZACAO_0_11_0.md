# Atualização 0.11.0 — Anexos em documentos e tarefas

**Data:** 2026-11-04
**Extensão:** 0.1.8 (sem mudanças; não precisa recarregar)

> **Equipe em rede:** o acervo passa para a versão 10 de esquema. Atualize todos os computadores do gabinete; um Ponte anterior recusa abrir um acervo já atualizado e pede a versão atual.

## O que muda

- **Anexos.** Em **Documento ▸ Anexos** e na ficha de cada tarefa, o botão **Anexar arquivo…** junta um arquivo do seu computador ao documento ou à tarefa: ofício em Word, foto de um despacho, e-mail recebido, planilha. O arquivo fica no acervo, cifrado como todo o resto, e aparece para toda a equipe (no computador principal e nos demais, com fila quando fora de alcance).
- **Cada anexo** mostra nome, tipo, tamanho, quem anexou e quando, com uma **descrição** opcional. Botões: **Ler no Ponte** (PDF, no leitor do Ponte), **Abrir** (demais tipos, no programa padrão do Windows, a partir de uma cópia temporária apagada ao fechar o Ponte), **Salvar como…** e **Excluir…** (vai para a Lixeira, de onde pode ser restaurado).
- **Limites com orientação.** Até 8 MB por arquivo e 20 anexos por documento ou tarefa. Tipos aceitos: PDF, Word, Excel, PowerPoint, OpenDocument, texto (.txt, .rtf, .csv), imagens (.png, .jpg, .gif, .bmp, .tif) e e-mails (.eml, .msg). Programas, scripts, atalhos, arquivos compactados e Office com macros são recusados. Toda recusa diz o motivo e o que fazer: reduzir ou dividir o arquivo, converter para PDF, extrair o .zip, excluir um anexo antigo. O tipo é conferido também pelos primeiros bytes do arquivo, não só pela extensão.
- **Lista de tarefas** mostra **📎 N** quando a tarefa tem anexos. Uma tarefa nova precisa ser salva antes de receber anexos.
- **Lixeira** passa a listar anexos excluídos, com restauração. Excluir um documento ou tarefa leva seus anexos junto; restaurar devolve.
- **Pendências ao fechar** incluem descrições de anexo ainda não salvas.

## Para desenvolvedores

- `src/attachments.rs`: validação (`validate_file`: extensão, assinatura dos bytes, tamanho), `Store::add_attachment` / `list_attachments` / `attachment_bytes` / `update_attachment_description` / `attachment_counts`, cópias temporárias em `%LOCALAPPDATA%\PonteProj\temp\`, e o bloco `AttachmentsPanel` usado pela ficha do documento e pela ficha da tarefa.
- Tabela `attachments(owner_kind, owner_id, document_id, metadata, payload)` com conteúdo selado; `user_version` 10; gatilhos de guarda e de item ativo cobrem a tabela.
- `trash.Kind::Attachment`; a tabela `trash` de acervos anteriores é reconstruída uma vez para aceitar o novo tipo (teste `trash_table_from_older_libraries_is_rebuilt_to_accept_attachments`).
- Rede: mutações `AddAttachment` e `UpdateAttachment`, `RpcRequest::FetchAttachmentBytes`, instantâneo com `attachments` (conteúdo transferido sob demanda e conferido por SHA-256). O teste de rede cobre anexar, descrever, excluir e restaurar via computador principal.
- Leitor de PDF aceita anexos (`PdfPanel::open_attachment`); OCR de anexo não é gravado.
- `windows_ui::save_file` (GetSaveFileNameW) e `windows_ui::open_with_default_app` (ShellExecuteW, verbo "open"). Recursos `Win32_UI_Shell` e `Win32_UI_WindowsAndMessaging` adicionados à crate `windows`.
- `labels::file_size` ("830 KB", "12,4 MB").
