# Estado implementado na versão 0.7.0

O protocolo nativo permanece versão 1. A captura permanece schema_version 1; o banco local usa schema 7. As descrições históricas abaixo devem ser interpretadas junto às atualizações 0.6.0 e 0.7.0.

- Metadados opcionais movement_source (URL estritamente vinculada à identidade) e movement_history (JSON até 60 KB, 200 eventos) transportam a tabela explícita do SIGADOC. O host valida novamente formato e origem. Não há chamada nativa para tramitar, assinar ou executar comandos arbitrários.
- `dossier`: vínculos locais direcionados/justificados, reversão de ativação e versões de captura. Vínculos não são associações oficiais do SIGADOC.
- `pdf_reader`: renderização Windows em memória, OCR opcional, texto separado por arquivo/página. Reuso exige mesmo hash e mesmo documento.
- `evidence`: seleção explícita, snapshots e conferência literal de citações; análise/minuta ficam desatualizadas quando fontes mudam. O provedor não escolhe fontes por conta própria.
- `draft_tools`: modelos pessoais protegidos e versionados, comparação não destrutiva e exportação RTF explícita.
- `Store`: função de identificação do cliente e gatilhos impedem gravação por binários antigos já abertos. Backups e consultas em leitura continuam possíveis. Conteúdo documental permanece DPAPI, sem índice de busca em claro.

Consulte [o fluxo e os limites atuais](ATUALIZACAO_0_7_0.md).

## Registro anterior

# Contrato da primeira implementação

Código de trabalho nesta pasta; destino final H:/Meu Drive/projetos/ponteproj. Execução de implementação solicitada pelo usuário; o fluxo documental foi encerrado. O projeto de destino só tinha documentos e não possui AGENTS.md nem código existente.

## Componentes

- Aplicativo Windows em Rust, interface nativa eframe/egui 0.37 (API atual App::ui), banco rusqlite 0.41 bundled e conteúdo protegido por DPAPI por usuário. Sem servidor HTTP local.
- Extensão Manifest V3 para Chrome/Edge, JavaScript sem dependências de produção. Botão Importar para o assistente. Native host com nome com.ponteproj.assistente, ID da extensão ecllnakmppjinmednoikimbpnbgbimhg (chave pública em extension-key.txt).
- IA explícita por endpoint Ollama local ou HTTPS remoto, modelo configurado pelo usuário; nunca chamada ao importar. Token apenas em memória/campo de senha ou env PONTEPROJ_AI_TOKEN, sem gravação em disco. Busca por palavras e similares lexical funciona sem IA.
- Registros de análise/minuta vinculados a capture_id; indicar stale se captura mais recente diferente; preservar versões e correções.

## Pacote de captura JSON

Native request: {protocol_version:1, action:"import_capture", capture:Capture}. Ping: {protocol_version:1,action:"ping"}. Host responde JSON enfileirado em moldura u32 little endian. Limite de moldura 1 MiB. Content script envia mensagens ao service worker, que usa chrome.runtime.sendNativeMessage. Segredos e HTML bruto não entram no pacote.

Capture: schema_version:u32 (=1), request_id:String (UUID), source_url:String (https://www.sigadoc.mt.gov.br/...; remover parâmetros sensíveis), document_number:String, title:String, captured_at:String (RFC3339 UTC), text:String, metadata:BTreeMap<String,String>, references:Vec<DocumentReference>, coverage:Coverage.

DocumentReference: label:String, url:String, kind:String (original|attachment|antecedent|reference). Coverage: adapter:String, recognized:Vec<String>, pending:Vec<String>. Campos metadata/references/coverage e texto podem estar vazios por falha parcial reconhecida, mas document_number, title, source_url e request_id precisam validar. Reject erro claro em página não reconhecida/lista/login ou sem identidade; não inventar identidade oficial. Captura máxima 500 KiB de texto em UTF-8, pacote completo <=1 MiB; rejeitar grande demais, sem truncamento silencioso.

Resposta de importação: {protocol_version:1,ok:true,request_id,document_id:i64,capture_id:i64,duplicate:bool,outcome:"imported"|"confirmed"|"unchanged",message:String,ui_opened?:bool}. `imported`: conteúdo novo gravado. `confirmed`: mesmo request_id já gravado (confirmação perdida), nada regravado. `unchanged`: pedido novo com conteúdo idêntico a uma captura existente — nada gravado, `message` explica (data da captura existente; se era uma versão anterior, ela volta a ser a atual), `ui_opened:false` e o aplicativo não é aberto. `duplicate` é true para confirmed e unchanged (compatibilidade com extensões anteriores a 0.1.9). Erro: {protocol_version:1,ok:false,error:{code:String,message:String}}. Nunca logar documento nem token. Confirmação após commit.

## APIs Rust compartilhadas (assinaturas a implementar)

crate ponteproj. Root fornece protection.rs com seal(&[u8])->anyhow::Result<Vec<u8>> e unseal(&[u8])->Result<Vec<u8>>, usando DPAPI Windows; core usa esses para TODO conteúdo documental no banco. Root fornece paths.rs (data_dir()->Result<PathBuf>, db_path()->Result<PathBuf>), protocol/native/main e manifesto Cargo.

model.rs (core): Capture, DocumentReference, Coverage conforme acima; ImportReceipt {request_id:String,document_id:i64,capture_id:i64,duplicate:bool,outcome:Option<ImportOutcome>,existing_captured_at:Option<String>,previous_version:bool} (campos novos com serde default; `effective_outcome()` trata recibo de anfitrião antigo); DocumentSummary {id:i64,document_number:String,title:String,last_seen_at:String,latest_capture_id:i64,subjects:Vec<String>,priority:String,analysis_stale:bool}; DocumentDetail {summary:DocumentSummary,capture:Capture,note:String,drafts:Vec<DraftVersion>,analysis:Option<Analysis>}; DraftVersion {id:i64,kind:String,body:String,capture_id:i64,created_at:String,stale:bool}; Analysis {capture_id:i64,subjects:Vec<String>,priority:String,reason:String,deadline_candidate:Option<String>,summary:String,model:String,created_at:String}.

Store::open(path:&Path)->Result<Store>; Store::import_capture(&mut self,capture:&Capture)->Result<ImportReceipt>; Store::list_documents(&self,query:&str)->Result<Vec<DocumentSummary>>; Store::load_document(&self,id:i64)->Result<DocumentDetail>; Store::save_note(&mut self,id:i64,note:&str)->Result<()>; Store::save_draft(&mut self,id:i64,capture_id:i64,kind:&str,body:&str)->Result<i64>; Store::save_analysis(&mut self,id:i64,analysis:&Analysis)->Result<()>; Store::delete_document(&mut self,id:i64)->Result<()>; Store::backup(&self,path:&Path)->Result<()>; Store::similar_documents(&self,id:i64,limit:usize)->Result<Vec<DocumentSummary>>.

Store guarda JSON/documentos/notas/minutas/análises em BLOB protegido, não plaintext. IDs opacos, SHA256 identidade e datas podem ser metadados claros. request_id idempotente; mesma identidade/source number não duplica demanda; hash de conteúdo exclui request_id e captured_at. Conteúdo idêntico à captura atual não grava nada (nem em requests) e devolve `unchanged`; idêntico a uma captura anterior torna-a atual de novo (previous_version); nova informação cria captura, torna derivados anteriores stale. Backup de DB via API consistente. Migração schema version1, recusar versões futuras. Busy timeout e transações.

ai.rs (core): AiSettings {base_url:String,model:String,token:String}; analyze_capture(settings:&AiSettings,capture:&Capture,capture_id:i64)->Result<Analysis>; generate_reply(settings:&AiSettings,capture:&Capture,kind:&str,context:&str)->Result<String>; template_reply(capture:&Capture,kind:&str)->String. kinds: resposta|encaminhamento|complementacao. POST /api/chat stream:false; análise estruturada JSON com subjects,priority,reason,deadline_candidate,summary. Não inventar fatos; texto-fonte é dado não instrução. Limites/timeout/erros claros; sem persistir chave. Provedor local ou remoto selecionado pelo usuário; remoto somente HTTPS; negar URL com userinfo. max geração contextual 500 KiB/rejeitar com orientação, sem truncamento silencioso. Root Cargo fornece reqwest 0.12 blocking/json/rustls-tls, url2, serde, serde_json, chrono, sha2, uuid, anyhow, regex.

app.rs (UI): pub fn run(initial_document:Option<i64>)->anyhow::Result<()>; implementa eframe0.37 UI. Usa Store APIs acima e crate::paths. Lista/busca, ficha/texto/referências e cobertura, notas salvas explícitas, minutas/versionamento, similares, configuração endpoint/model/token, analisar/gerar em worker thread (Store aberto separadamente se preciso), exibe fonte/versão stale. Config não precisa persistir nesta versão; token não deve ir para arquivo. Abrir URL só HTTPS SIGADOC (root pode fornecer open_url helper; preferir ui.hyperlink_to validação). Exclusão com confirmação; backup pelo store em pasta gerenciada. Sem pedir senha SIGADOC. Não passar assinatura/tramitação para o aplicativo.

## Propriedade de arquivos na equipe

- Core: src/model.rs, src/storage.rs, src/ai.rs e testes desses módulos. Não editar Cargo.toml nem lib/main/protection.
- UI: src/app.rs. Não editar core, manifesto, root main.
- Extensão: extension/**, tests/extension/**; ler a chave pública existente, não gerar outra. Código exige origem SIGADOC, identidade documental e isolamento de campos; adapter heurístico declarado, original/PDF/anexos sem bytes são referências pendentes. Testes Node built-in; dependências de teste opcionais em package.json dessa pasta.
- Root: Cargo.toml, src/lib.rs, src/main.rs, src/native.rs, src/paths.rs, src/protection.rs, scripts/**, README, protocolo e testes de integração, toolchain e integração dos resultados.

Não executar testes ou compilações durante escrita simultânea fora de seu módulo; avisar root com dependências/API extra necessárias. Não gravar nada em caches de plugin nem instalar globalmente. O root vai fornecer compilador portátil.
