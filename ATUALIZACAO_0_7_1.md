# Versão 0.7.1 — apresentação dos metadados importados

Em 03/11/2026, a captura #8 já continha data documental, tipo/espécie, modelo, suporte, classificação oficial, cadastrante, acesso e quatro eventos do SIGADOC. A ficha principal mostrava apenas parte desses dados e expunha o restante com chaves técnicas/JSON, criando a impressão de que não haviam sido importados.

## Alterações

- **Dados do documento no SIGADOC:** número, tipo/espécie, data, suporte, modelo, classificação documental oficial, processo quando presente e código da via.
- **Pessoas, situação e acesso:** origem, cadastrante, destinatário, detentor, situação da via, marca/pessoa da assinatura, nível de acesso e pessoas listadas.
- **Histórico importado do SIGADOC:** eventos apresentados diretamente na ficha, com data, via/escopo, cadastrante, atendente e descrição. A aba Dossiê usa a mesma apresentação.
- Campos formais de remetente, envio e recebimento aparecem quando efetivamente existem nos metadados da ficha. Sua ausência não gera mais linhas dizendo que nada foi identificado na captura quando há registros no histórico.
- Datas dos eventos são preservadas como exibidas. Uma linha sem data permanece identificada como tal, sem herdar data documental nem data de outra linha. Cadastrante/atendente preservam os papéis da fonte; não se presume último remetente por autoria.
- Metadados complementares recebem rótulos em português. JSON, URL de histórico e hashes ficam numa seção técnica própria.

A classificação documental oficial é distinta da classificação local por assunto, finalidade e prioridade. A finalidade Ciência e download e demais registros locais continuam separados dos dados oficiais capturados.

## Atualização e verificação

Aplicativo 0.7.1, banco schema 7 e extensão 0.1.7. Esta correção só altera a apresentação: **não exige recarregar a extensão nem reimportar**. Use a janela Ponte 0.7.1, já aberta pelo instalador.

32 testes Rust e o teste do pacote nativo/PDF passaram. O acervo real permaneceu íntegro: oito capturas e uma tarefa, preservando os BLOBs de notas, minutas, análises, contexto e tarefas comparados com o backup.

Backup anterior: `%LOCALAPPDATA%\PonteProj\backups\pre-v071-20261103-085318-131287.sqlite3`.

SHA256 instalado: `402D7013E1A93EA25C0BE35A37670BEEDF295B3C7CC3D8CB55244E2F322AE8BA`.

A conferência visual da versão instalada confirmou os dados do documento e de acesso na ficha principal, usando a captura #8 já existente.

O histórico foi conferido na ficha principal: Recebimento em 22/10/26 e eventos de Transferência/Criação com data não exibida na fonte. Os papéis e nomes de cadastrante/atendente ficaram legíveis, sem JSON.
