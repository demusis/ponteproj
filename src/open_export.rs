//! "Exportar tudo em pasta aberta…": the whole library written as plain files
//! that can be read without the Ponte (Markdown for people, one JSON for
//! programs), in an EMPTY folder the person chooses.
//!
//! Nothing here encrypts: the folder is readable by anyone who holds it. The
//! library is only read (never changed), nothing is sent anywhere, and the
//! work goes document by document, so the library is never held in memory
//! (one document with its versions and files at a time; the small sets, such
//! as tasks and resources, are held whole).
//!
//! A marker file, `EXPORTACAO-INCOMPLETA.txt`, is written first and removed
//! only when everything was written: a cancelled, failed or interrupted
//! export is never taken for a complete one. No file is ever overwritten.
//!
//! Shape of `acervo.json` (`versao_do_formato` 1; keys in Portuguese, the
//! records of the Ponte keep their own field names inside `captura`,
//! `analise`, `tarefa`, `projeto`, `atendimento`, `recurso`, `uso`,
//! `categoria`): see [`JSON_SHAPE`], also copied into `LEIA-ME.txt`.
use crate::{
    attachments::AttachmentMeta,
    export::{iso_date, open_export_file_name},
    files::FileSummary,
    model::{Analysis, Capture, DraftProvenance},
    protection,
    storage::Store,
    tasks::Task,
    work_context::WorkContext,
};
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Name of the marker of a folder that must not be taken for complete.
pub const MARKER: &str = "EXPORTACAO-INCOMPLETA.txt";
/// Version of the shape of `acervo.json`.
pub const FORMAT_VERSION: u32 = 1;
/// What the person is told, in the window and in `LEIA-ME.txt`.
pub const NOT_ENCRYPTED: &str = "Esta exportação NÃO é criptografada: qualquer pessoa que tenha esta pasta lê tudo o que há nela (textos, notas, minutas, anexos e PDFs). Guarde-a em lugar seguro e apague-a quando não precisar mais.";

/// The shape of `acervo.json`, for whoever reads it with a program.
pub const JSON_SHAPE: &str = "\
acervo.json (versao_do_formato 1)
  formato, versao_do_formato, exportado_em (ISO 8601), versao_do_ponte, aviso
  documentos[]: id, numero, titulo, pasta (caminho relativo, com /), em_lixeira,
      excluido_em, origem, manual, adicionado_em, ultima_visualizacao,
      versao_atual_id, contexto_de_trabalho, categorias[{uid,nome}],
      nota_atual, notas[{id,criada_em,texto}],
      versoes[{id,numero_da_versao,importada_em,atual,em_lixeira,arquivo,captura,
               pdfs[{id,rotulo,url,sha256,tamanho,situacao_do_texto,arquivo,erro}]}],
      minutas[{id,tipo,criada_em,versao_id,de_versao_antiga,texto,proveniencia,fontes}],
      analises[{id,versao_id,criada_em,analise}],
      tarefas[ids], anexos[{id,nome,tipo,tamanho,sha256,descricao,adicionado_por,
               adicionado_em,em_lixeira,arquivo,erro}],
      vinculos[{id,origem_id,destino_id,tipo,motivo,ativo,criado_em}]
  tarefas[]: id, em_lixeira, excluido_em, documento_numero, tarefa (campos do Ponte:
      uid, document_id, title, status, due_date, project_uid, categories, ...)
  projetos[]: projeto (uid, name, status, ...); atendimentos[]: atendimento (id,
      document_id, ...); recursos[]: recurso (uid, name, ...); usos_de_recursos[]:
      uso (uid, resource_uid, document_id, task_id, ...); categorias[]: categoria
      (uid, name, ...)
  anexos_avulsos[]: dono_tipo (tarefa|atendimento), dono_id, + os campos de um anexo
  lixeira[]: tipo (documento|versao|tarefa|anexo), id, documento_id, excluido_em
Referências entre itens: por id (documentos, tarefas, atendimentos, versões, anexos)
e por uid (projetos, recursos, categorias). Datas em ISO 8601 como o Ponte as guarda.";

/// The person cancelled; the folder was marked incomplete.
#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Exportação cancelada. A pasta ficou marcada com EXPORTACAO-INCOMPLETA.txt: não use o que está nela.")
    }
}
impl std::error::Error for Cancelled {}

#[derive(Debug, Default, Clone)]
pub struct OpenExportReport {
    pub folder: PathBuf,
    pub documents: usize,
    pub trashed_documents: usize,
    pub versions: usize,
    pub tasks: usize,
    pub trashed_tasks: usize,
    pub projects: usize,
    pub attendances: usize,
    pub resources: usize,
    pub resource_uses: usize,
    pub categories: usize,
    /// PDFs and attachments written as files.
    pub files: usize,
    /// Things that could not be read or written; also in `LEIA-ME.txt`.
    pub warnings: Vec<String>,
}

/// The folder must exist and be empty.
pub fn check_target_folder(folder: &Path) -> Result<()> {
    ensure!(folder.is_dir(), "A pasta escolhida não existe.");
    let mut entries = fs::read_dir(folder)
        .with_context(|| format!("Não foi possível abrir a pasta {}.", folder.display()))?;
    ensure!(
        entries.next().is_none(),
        "A pasta escolhida não está vazia. Escolha ou crie uma pasta vazia: a exportação nunca mistura nem sobrescreve arquivos que já existem."
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// A file or folder name that Windows accepts, keeping accents; a path
/// separator becomes a hyphen instead of vanishing.
fn readable(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    open_export_file_name(&name.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(at) if at > 0 && name.len() - at <= 12 => (&name[..at], &name[at..]),
        _ => (name, ""),
    }
}

/// An original file name made safe: the stem as [`readable`], the extension
/// reduced to letters and digits.
fn safe_file_name(name: &str) -> String {
    let name = name.trim();
    let (stem, ext) = split_ext(name);
    let ext: String = ext
        .chars()
        .filter(|c| *c == '.' || c.is_alphanumeric())
        .collect();
    let ext = if ext == "." { String::new() } else { ext };
    format!("{}{}", readable(stem), ext)
}

/// Names already used in one folder (compared without regard to case, as
/// Windows does): a taken name gets " (2)", " (3)"… before its extension.
#[derive(Default)]
struct Names(HashSet<String>);
impl Names {
    fn take(&mut self, wanted: &str, with_extension: bool) -> String {
        let (stem, ext) = if with_extension {
            split_ext(wanted)
        } else {
            (wanted, "")
        };
        let mut candidate = wanted.to_owned();
        let mut n = 2;
        while self.0.contains(&candidate.to_lowercase()) {
            candidate = format!("{stem} ({n}){ext}");
            n += 1;
        }
        self.0.insert(candidate.to_lowercase());
        candidate
    }
}

/// One folder of files: names in use, and what was already written (the same
/// name with the same content is written once).
#[derive(Default)]
struct Dir {
    names: Names,
    written: HashMap<(String, String), String>,
}

// ---------------------------------------------------------------------------
// The JSON records
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CategoryRef {
    uid: String,
    nome: String,
}

#[derive(Serialize, Clone)]
struct FileJson {
    id: i64,
    nome: String,
    tipo: String,
    tamanho: usize,
    sha256: String,
    descricao: String,
    adicionado_por: String,
    adicionado_em: String,
    em_lixeira: bool,
    arquivo: Option<String>,
    erro: Option<String>,
}

#[derive(Serialize)]
struct PdfJson {
    id: i64,
    rotulo: String,
    url: String,
    sha256: String,
    tamanho: usize,
    situacao_do_texto: String,
    arquivo: Option<String>,
    erro: Option<String>,
}

#[derive(Serialize)]
struct VersionJson<'a> {
    id: i64,
    numero_da_versao: usize,
    importada_em: &'a str,
    atual: bool,
    em_lixeira: bool,
    arquivo: String,
    captura: &'a Capture,
    pdfs: Vec<PdfJson>,
}

#[derive(Serialize)]
struct NoteJson {
    id: i64,
    criada_em: String,
    texto: String,
}

#[derive(Serialize)]
struct DraftJson {
    id: i64,
    tipo: String,
    criada_em: String,
    versao_id: i64,
    de_versao_antiga: bool,
    texto: String,
    proveniencia: Option<DraftProvenance>,
    fontes: serde_json::Value,
}

#[derive(Serialize)]
struct AnalysisJson {
    id: i64,
    versao_id: i64,
    criada_em: String,
    analise: Analysis,
}

#[derive(Serialize)]
struct LinkJson {
    id: i64,
    origem_id: i64,
    destino_id: i64,
    tipo: String,
    motivo: String,
    ativo: bool,
    criado_em: String,
}

#[derive(Serialize)]
struct DocumentJson<'a> {
    id: i64,
    numero: &'a str,
    titulo: &'a str,
    pasta: String,
    em_lixeira: bool,
    excluido_em: Option<String>,
    origem: &'a str,
    manual: bool,
    adicionado_em: &'a str,
    ultima_visualizacao: String,
    versao_atual_id: i64,
    contexto_de_trabalho: &'a WorkContext,
    categorias: Vec<CategoryRef>,
    nota_atual: &'a str,
    notas: &'a [NoteJson],
    versoes: Vec<VersionJson<'a>>,
    minutas: &'a [DraftJson],
    analises: &'a [AnalysisJson],
    tarefas: Vec<i64>,
    anexos: &'a [FileJson],
    vinculos: &'a [LinkJson],
}

#[derive(Serialize)]
struct TaskJson<'a> {
    id: i64,
    em_lixeira: bool,
    excluido_em: Option<String>,
    documento_numero: Option<String>,
    tarefa: &'a Task,
}

#[derive(Serialize)]
struct LooseFileJson {
    dono_tipo: &'static str,
    dono_id: i64,
    #[serde(flatten)]
    anexo: FileJson,
}

#[derive(Serialize)]
struct TrashJson {
    tipo: &'static str,
    id: i64,
    documento_id: Option<i64>,
    excluido_em: String,
}

// ---------------------------------------------------------------------------
// The export
// ---------------------------------------------------------------------------

struct DocIndex {
    id: i64,
    number: String,
    title: String,
    folder: String,
    trashed: bool,
    categories: Vec<String>,
}

struct Run<'a> {
    store: &'a Store,
    root: PathBuf,
    step: &'a dyn Fn(&str),
    cancelled: &'a dyn Fn() -> bool,
    report: OpenExportReport,
    trash: HashMap<(String, i64), (Option<i64>, String)>,
    categories: BTreeMap<String, String>,
    projects: BTreeMap<String, String>,
    docs: Vec<DocIndex>,
}

/// Writes the whole library into `folder` (empty). `step` receives short
/// sentences about what is being written; `cancelled` is asked between
/// items. On cancellation the marker says so and the error is [`Cancelled`].
pub fn export_open(
    store: &Store,
    folder: &Path,
    step: &dyn Fn(&str),
    cancelled: &dyn Fn() -> bool,
) -> Result<OpenExportReport> {
    check_target_folder(folder)?;
    let started = chrono::Local::now();
    let marker = folder.join(MARKER);
    write_new(
        &marker,
        format!(
            "Esta pasta está INCOMPLETA.\n\nA exportação do acervo começou em {} e ainda não terminou (ou foi interrompida). Não use o que está aqui como cópia do acervo.\nSe a exportação terminar bem, este arquivo desaparece.\n",
            started.format("%d/%m/%Y %H:%M")
        )
        .as_bytes(),
    )?;
    let mut run = Run {
        store,
        root: folder.to_path_buf(),
        step,
        cancelled,
        report: OpenExportReport {
            folder: folder.to_path_buf(),
            ..Default::default()
        },
        trash: HashMap::new(),
        categories: BTreeMap::new(),
        projects: BTreeMap::new(),
        docs: Vec::new(),
    };
    match run.all(&started) {
        Ok(()) => {
            fs::remove_file(&marker)
                .context("Não foi possível retirar a marca de pasta incompleta.")?;
            Ok(run.report)
        }
        Err(error) => {
            let why = if error.downcast_ref::<Cancelled>().is_some() {
                "A exportação foi CANCELADA por você.".to_owned()
            } else {
                format!("A exportação PAROU por um erro: {error:#}")
            };
            let _ = fs::write(
                &marker,
                format!(
                    "Esta pasta está INCOMPLETA.\n\n{why}\nO que está aqui é só uma parte do acervo: não use como cópia. Apague a pasta e exporte de novo numa pasta vazia.\n"
                ),
            );
            Err(error)
        }
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Não foi possível criar a pasta {}.", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("Não foi possível criar {}.", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("Não foi possível gravar {}.", path.display()))?;
    Ok(())
}

fn unseal_json<T: DeserializeOwned>(blob: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(&protection::unseal(blob)?)?)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A fenced block whose fence is longer than any run of backticks inside.
fn fenced(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}text\n{}\n{fence}\n", text.trim_end_matches('\n'))
}

fn nonempty<'a>(value: &'a str, otherwise: &'a str) -> &'a str {
    if value.trim().is_empty() {
        otherwise
    } else {
        value
    }
}

struct DocRows {
    latest: i64,
    last_seen: String,
    captures: Vec<(i64, String, Capture)>,
    notes: Vec<NoteJson>,
    drafts: Vec<(i64, i64, String, DraftRow)>,
    analyses: Vec<(i64, i64, String, Analysis)>,
    context: WorkContext,
    links: Vec<LinkJson>,
}

#[derive(Deserialize)]
struct DraftRow {
    #[serde(default)]
    materials: serde_json::Value,
    kind: String,
    body: String,
    #[serde(default)]
    provenance: Option<DraftProvenance>,
}

#[derive(Deserialize)]
struct LinkRow {
    kind: String,
    reason: String,
    created_at: String,
}

impl Run<'_> {
    fn check(&self) -> Result<()> {
        if (self.cancelled)() {
            return Err(Cancelled.into());
        }
        Ok(())
    }

    fn put(&mut self, rel: &str, bytes: &[u8]) -> Result<()> {
        self.check()?;
        write_new(&self.root.join(rel), bytes)?;
        self.report.files += 1;
        Ok(())
    }

    fn warn(&mut self, text: String) {
        self.report.warnings.push(text);
    }

    fn all(&mut self, started: &chrono::DateTime<chrono::Local>) -> Result<()> {
        (self.step)("Lendo o que há no acervo…");
        let conn = &self.store.conn;
        // The Lixeira, as a map.
        {
            let mut statement =
                conn.prepare("SELECT kind,item_id,document_id,deleted_at FROM trash ORDER BY deleted_at,kind,item_id")?;
            let rows = statement.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            for row in rows {
                let (kind, id, document, at) = row?;
                self.trash.insert((kind, id), (document, at));
            }
        }
        let categories = self.store.list_categories()?;
        self.categories = categories
            .iter()
            .map(|c| (c.uid.clone(), c.name.clone()))
            .collect();
        let projects = self.store.list_projects()?;
        self.projects = projects
            .iter()
            .map(|p| (p.uid.clone(), p.name.clone()))
            .collect();
        let tasks = self.read_tasks()?;
        let mut tasks_by_document: HashMap<i64, Vec<usize>> = HashMap::new();
        for (index, (task, _)) in tasks.iter().enumerate() {
            if let Some(document) = task.document_id {
                tasks_by_document.entry(document).or_default().push(index);
            }
        }

        // acervo.json is written as it goes.
        let json_path = self.root.join("acervo.json");
        let mut json = BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&json_path)
                .context("Não foi possível criar acervo.json.")?,
        );
        let header = serde_json::json!({
            "formato": "ponte-acervo-aberto",
            "versao_do_formato": FORMAT_VERSION,
            "exportado_em": started.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            "versao_do_ponte": env!("CARGO_PKG_VERSION"),
            "aviso": NOT_ENCRYPTED,
        });
        let header = serde_json::to_string(&header)?;
        json.write_all(&header.as_bytes()[..header.len() - 1])?;
        json.write_all(b",\n\"documentos\": [")?;

        let ids: Vec<i64> = {
            let mut statement = conn.prepare("SELECT id FROM documents ORDER BY id")?;
            let ids = statement
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<i64>>>()?;
            ids
        };
        let mut active_names = Names::default();
        let mut trashed_names = Names::default();
        let mut first = true;
        let total = ids.len();
        for (index, id) in ids.into_iter().enumerate() {
            self.check()?;
            let trashed = self
                .trash
                .get(&("document".to_owned(), id))
                .map(|(_, at)| at.clone());
            let names = if trashed.is_some() {
                &mut trashed_names
            } else {
                &mut active_names
            };
            let mut names = std::mem::take(names);
            let result = self.document(
                id,
                trashed,
                &mut names,
                &tasks,
                tasks_by_document.get(&id).map(Vec::as_slice).unwrap_or(&[]),
                index + 1,
                total,
            );
            if self.trash.contains_key(&("document".to_owned(), id)) {
                trashed_names = names;
            } else {
                active_names = names;
            }
            match result {
                Ok(Some(line)) => {
                    if !first {
                        json.write_all(b",")?;
                    }
                    first = false;
                    json.write_all(b"\n")?;
                    json.write_all(line.as_bytes())?;
                }
                Ok(None) => {}
                Err(error) if error.downcast_ref::<Cancelled>().is_some() => return Err(error),
                Err(error) => self.warn(format!(
                    "O documento de código interno {id} não pôde ser exportado: {error:#}"
                )),
            }
        }
        json.write_all(b"\n]")?;

        // Tasks.
        (self.step)("Gravando tarefas, projetos, atendimentos, recursos e categorias…");
        let doc_numbers: HashMap<i64, String> =
            self.docs.iter().map(|d| (d.id, d.number.clone())).collect();
        let task_json: Vec<TaskJson> = tasks
            .iter()
            .map(|(task, trashed)| TaskJson {
                id: task.id,
                em_lixeira: trashed.is_some(),
                excluido_em: trashed.clone(),
                documento_numero: task.document_id.and_then(|d| doc_numbers.get(&d).cloned()),
                tarefa: task,
            })
            .collect();
        json.write_all(b",\n\"tarefas\": ")?;
        serde_json::to_writer(&mut json, &task_json)?;
        self.report.trashed_tasks = tasks.iter().filter(|(_, t)| t.is_some()).count();
        self.report.tasks = tasks.len() - self.report.trashed_tasks;

        json.write_all(b",\n\"projetos\": ")?;
        let project_json: Vec<_> = projects
            .iter()
            .map(|p| serde_json::json!({ "projeto": p }))
            .collect();
        serde_json::to_writer(&mut json, &project_json)?;
        self.report.projects = projects.len();

        let attendances = self.store.list_saiop_attendances()?;
        json.write_all(b",\n\"atendimentos\": ")?;
        let attendance_json: Vec<_> = attendances
            .iter()
            .map(|a| serde_json::json!({ "atendimento": a }))
            .collect();
        serde_json::to_writer(&mut json, &attendance_json)?;
        self.report.attendances = attendances.len();

        let resources = self.store.list_resources()?;
        let uses = self.store.list_resource_uses()?;
        json.write_all(b",\n\"recursos\": ")?;
        let resource_json: Vec<_> = resources
            .iter()
            .map(|r| serde_json::json!({ "recurso": r }))
            .collect();
        serde_json::to_writer(&mut json, &resource_json)?;
        json.write_all(b",\n\"usos_de_recursos\": ")?;
        let use_json: Vec<_> = uses
            .iter()
            .map(|u| serde_json::json!({ "uso": u }))
            .collect();
        serde_json::to_writer(&mut json, &use_json)?;
        self.report.resources = resources.len();
        self.report.resource_uses = uses.len();

        json.write_all(b",\n\"categorias\": ")?;
        let category_json: Vec<_> = categories
            .iter()
            .map(|c| serde_json::json!({ "categoria": c }))
            .collect();
        serde_json::to_writer(&mut json, &category_json)?;
        self.report.categories = categories.len();

        // Files of tasks and atendimentos.
        let loose = self.loose_attachments(&tasks, &attendances)?;
        json.write_all(b",\n\"anexos_avulsos\": ")?;
        serde_json::to_writer(&mut json, &loose)?;

        let trash_json: Vec<TrashJson> = self
            .trash
            .iter()
            .map(|((kind, id), (document, at))| TrashJson {
                tipo: match kind.as_str() {
                    "document" => "documento",
                    "capture" => "versao",
                    "task" => "tarefa",
                    _ => "anexo",
                },
                id: *id,
                documento_id: *document,
                excluido_em: at.clone(),
            })
            .collect();
        let mut trash_json = trash_json;
        trash_json.sort_by(|a, b| (a.tipo, a.id).cmp(&(b.tipo, b.id)));
        json.write_all(b",\n\"lixeira\": ")?;
        serde_json::to_writer(&mut json, &trash_json)?;
        json.write_all(b"\n}\n")?;
        json.flush()
            .context("Não foi possível gravar acervo.json.")?;
        drop(json);
        self.report.files += 1;

        // The Markdown overviews.
        self.check()?;
        self.overviews(
            &tasks,
            &projects,
            &attendances,
            &resources,
            &uses,
            &categories,
        )?;
        self.leia_me(started)?;
        Ok(())
    }

    fn read_tasks(&self) -> Result<Vec<(Task, Option<String>)>> {
        let conn = &self.store.conn;
        let mut statement = conn.prepare("SELECT id,document_id,payload FROM tasks ORDER BY id")?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (id, document, blob) in rows {
            let mut task: Task = unseal_json(&blob)?;
            task.id = id;
            task.document_id = document;
            let trashed = self
                .trash
                .get(&("task".to_owned(), id))
                .or_else(|| document.and_then(|d| self.trash.get(&("document".to_owned(), d))))
                .map(|(_, at)| at.clone());
            out.push((task, trashed));
        }
        Ok(out)
    }

    fn read_document(&self, id: i64) -> Result<DocRows> {
        let conn = &self.store.conn;
        let (latest, last_seen): (i64, String) = conn.query_row(
            "SELECT latest_capture_id,last_seen_at FROM documents WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut captures = Vec::new();
        {
            let mut statement = conn.prepare(
                "SELECT id,created_at,payload FROM captures WHERE document_id=?1 ORDER BY id",
            )?;
            let rows = statement
                .query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (capture_id, at, blob) in rows {
                captures.push((capture_id, at, unseal_json::<Capture>(&blob)?));
            }
        }
        let mut notes = Vec::new();
        {
            let mut statement = conn.prepare(
                "SELECT id,created_at,payload FROM notes WHERE document_id=?1 ORDER BY id",
            )?;
            let rows = statement
                .query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (note_id, at, blob) in rows {
                notes.push(NoteJson {
                    id: note_id,
                    criada_em: at,
                    texto: unseal_json::<String>(&blob)?,
                });
            }
        }
        let mut drafts = Vec::new();
        {
            let mut statement = conn.prepare(
                "SELECT id,capture_id,created_at,payload FROM drafts WHERE document_id=?1 ORDER BY id",
            )?;
            let rows = statement
                .query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (draft_id, capture_id, at, blob) in rows {
                drafts.push((draft_id, capture_id, at, unseal_json::<DraftRow>(&blob)?));
            }
        }
        let mut analyses = Vec::new();
        {
            let mut statement = conn.prepare(
                "SELECT id,capture_id,created_at,payload FROM analyses WHERE document_id=?1 ORDER BY id",
            )?;
            let rows = statement
                .query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (analysis_id, capture_id, at, blob) in rows {
                analyses.push((analysis_id, capture_id, at, unseal_json::<Analysis>(&blob)?));
            }
        }
        let context = match conn
            .query_row(
                "SELECT payload FROM work_context WHERE document_id=?1",
                [id],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })? {
            Some(blob) => unseal_json::<WorkContext>(&blob)?,
            None => WorkContext::default(),
        };
        let mut links = Vec::new();
        {
            let mut statement = conn.prepare(
                "SELECT id,source_id,target_id,active,payload FROM document_links WHERE source_id=?1 OR target_id=?1 ORDER BY id",
            )?;
            let rows = statement
                .query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, bool>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (link_id, source, target, active, blob) in rows {
                let payload: LinkRow = unseal_json(&blob)?;
                links.push(LinkJson {
                    id: link_id,
                    origem_id: source,
                    destino_id: target,
                    tipo: payload.kind,
                    motivo: payload.reason,
                    ativo: active,
                    criado_em: payload.created_at,
                });
            }
        }
        Ok(DocRows {
            latest,
            last_seen,
            captures,
            notes,
            drafts,
            analyses,
            context,
            links,
        })
    }

    /// Writes one attachment's bytes into `rel_dir`; the answer is its JSON
    /// record (with the error, if it could not be read).
    fn place_attachment(
        &mut self,
        id: i64,
        trashed: bool,
        rel_dir: &str,
        dir: &mut Dir,
    ) -> Result<FileJson> {
        match self.store.attachment_bytes(id) {
            Ok((meta, data)) => {
                let path = self.place(dir, rel_dir, &meta.name, &data, &meta.sha256)?;
                Ok(file_json(&meta, trashed, Some(path), None))
            }
            Err(error) => {
                let message = format!("{error:#}");
                self.warn(format!("O anexo {id} não pôde ser lido: {message}"));
                // The metadata alone, when it can still be read.
                let blob: Option<Vec<u8>> = self
                    .store
                    .conn
                    .query_row("SELECT metadata FROM attachments WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .ok();
                let meta = blob
                    .and_then(|b| unseal_json::<AttachmentMeta>(&b).ok())
                    .unwrap_or_else(|| AttachmentMeta {
                        id,
                        name: format!("anexo {id}"),
                        kind: String::new(),
                        size: 0,
                        sha256: String::new(),
                        description: String::new(),
                        added_by: String::new(),
                        added_at: String::new(),
                    });
                Ok(file_json(&meta, trashed, None, Some(message)))
            }
        }
    }

    /// Writes bytes under a safe, never repeated name; the same name with
    /// the same content is written once. The answer is the relative path.
    fn place(
        &mut self,
        dir: &mut Dir,
        rel_dir: &str,
        original: &str,
        data: &[u8],
        sha: &str,
    ) -> Result<String> {
        let safe = safe_file_name(original);
        let sha = if sha.is_empty() {
            format!("{:x}", Sha256::digest(data))
        } else {
            sha.to_owned()
        };
        let key = (safe.to_lowercase(), sha);
        if let Some(path) = dir.written.get(&key) {
            return Ok(path.clone());
        }
        let name = dir.names.take(&safe, true);
        let rel = format!("{rel_dir}/{name}");
        self.put(&rel, data)?;
        dir.written.insert(key, rel.clone());
        Ok(rel)
    }

    #[allow(clippy::too_many_arguments)]
    fn document(
        &mut self,
        id: i64,
        trashed: Option<String>,
        names: &mut Names,
        tasks: &[(Task, Option<String>)],
        task_indexes: &[usize],
        position: usize,
        total: usize,
    ) -> Result<Option<String>> {
        let rows = self.read_document(id)?;
        let Some(latest) = rows
            .captures
            .iter()
            .find(|(capture_id, _, _)| *capture_id == rows.latest)
            .or_else(|| rows.captures.last())
        else {
            self.warn(format!(
                "O documento de código interno {id} não tem nenhuma versão importada e ficou de fora."
            ));
            return Ok(None);
        };
        let latest_id = latest.0;
        let number = latest.2.document_number.clone();
        let title = latest.2.title.clone();
        (self.step)(&format!(
            "Exportando documento {position} de {total}: {number}"
        ));
        let base = if trashed.is_some() {
            "lixeira/documentos"
        } else {
            "documentos"
        };
        let folder_name = names.take(&readable(&format!("{number} - {title}")), false);
        let rel_dir = format!("{base}/{folder_name}");

        // Files: the SIGADOC PDFs of each version, then the attachments.
        let mut dir = Dir::default();
        let attachments_dir = format!("{rel_dir}/anexos");
        let mut pdfs: HashMap<i64, Vec<PdfJson>> = HashMap::new();
        for (capture_id, _, _) in &rows.captures {
            let summaries: Vec<FileSummary> =
                self.store.list_files(*capture_id).unwrap_or_default();
            for summary in summaries {
                let blob: std::result::Result<Vec<u8>, _> = self.store.conn.query_row(
                    "SELECT payload FROM files WHERE id=?1",
                    [summary.id],
                    |r| r.get(0),
                );
                let mut record = PdfJson {
                    id: summary.id,
                    rotulo: summary.label.clone(),
                    url: summary.url.clone(),
                    sha256: summary.sha256.clone(),
                    tamanho: summary.size,
                    situacao_do_texto: summary.text_status.clone(),
                    arquivo: None,
                    erro: None,
                };
                match blob
                    .map_err(anyhow::Error::from)
                    .and_then(|b| protection::unseal(&b))
                {
                    Ok(data) if format!("{:x}", Sha256::digest(&data)) == summary.sha256 => {
                        let original = pdf_name(&summary);
                        record.arquivo = Some(self.place(
                            &mut dir,
                            &attachments_dir,
                            &original,
                            &data,
                            &summary.sha256,
                        )?);
                    }
                    Ok(_) => {
                        record.erro = Some("o conteúdo não confere com o registro".into());
                        self.warn(format!(
                            "O PDF {} de {number} não confere com o registro e não foi exportado.",
                            summary.id
                        ));
                    }
                    Err(error) => {
                        record.erro = Some(format!("{error:#}"));
                        self.warn(format!(
                            "O PDF {} de {number} não pôde ser lido: {error:#}",
                            summary.id
                        ));
                    }
                }
                pdfs.entry(*capture_id).or_default().push(record);
            }
        }
        let attachment_rows: Vec<i64> = {
            let mut statement = self.store.conn.prepare(
                "SELECT id FROM attachments WHERE owner_kind='document' AND owner_id=?1 ORDER BY id",
            )?;
            let ids = statement
                .query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<i64>>>()?;
            ids
        };
        let mut attachments = Vec::new();
        let mut aside = Dir::default();
        for attachment in attachment_rows {
            let in_trash = self
                .trash
                .contains_key(&("attachment".to_owned(), attachment));
            if in_trash && trashed.is_none() {
                let target = format!("lixeira/anexos/documento-{id}");
                let record = self.place_attachment(attachment, true, &target, &mut aside)?;
                attachments.push(record);
            } else {
                let record =
                    self.place_attachment(attachment, in_trash, &attachments_dir, &mut dir)?;
                attachments.push(record);
            }
        }

        // The versions.
        let mut version_json = Vec::new();
        for (index, (capture_id, saved_at, capture)) in rows.captures.iter().enumerate() {
            let n = index + 1;
            let rel = format!("versoes/{n:02}.md");
            let version_trashed = self
                .trash
                .contains_key(&("capture".to_owned(), *capture_id));
            let md = version_markdown(
                n,
                *capture_id,
                saved_at,
                *capture_id == latest_id,
                version_trashed,
                capture,
            );
            self.put(&format!("{rel_dir}/{rel}"), md.as_bytes())?;
            version_json.push(VersionJson {
                id: *capture_id,
                numero_da_versao: n,
                importada_em: saved_at,
                atual: *capture_id == latest_id,
                em_lixeira: version_trashed,
                arquivo: rel,
                captura: capture,
                pdfs: pdfs.remove(capture_id).unwrap_or_default(),
            });
        }
        self.report.versions += rows.captures.len();

        let categories: Vec<CategoryRef> = rows
            .context
            .categories
            .iter()
            .filter_map(|uid| {
                self.categories.get(uid).map(|name| CategoryRef {
                    uid: uid.clone(),
                    nome: name.clone(),
                })
            })
            .collect();
        let draft_json: Vec<DraftJson> = rows
            .drafts
            .iter()
            .map(|(draft_id, capture_id, at, row)| DraftJson {
                id: *draft_id,
                tipo: row.kind.clone(),
                criada_em: at.clone(),
                versao_id: *capture_id,
                de_versao_antiga: *capture_id != latest_id,
                texto: row.body.clone(),
                proveniencia: row.provenance.clone(),
                fontes: row.materials.clone(),
            })
            .collect();
        let analysis_json: Vec<AnalysisJson> = rows
            .analyses
            .iter()
            .map(|(analysis_id, capture_id, at, analysis)| AnalysisJson {
                id: *analysis_id,
                versao_id: *capture_id,
                criada_em: at.clone(),
                analise: analysis.clone(),
            })
            .collect();
        let note = rows
            .notes
            .last()
            .map(|n| n.texto.clone())
            .unwrap_or_default();
        let added_at = rows
            .captures
            .first()
            .map(|(_, at, _)| at.clone())
            .unwrap_or_default();
        let manual = latest.2.is_manual();

        let doc_tasks: Vec<&(Task, Option<String>)> =
            task_indexes.iter().map(|i| &tasks[*i]).collect();
        let md = DocumentMarkdown {
            id,
            number: &number,
            title: &title,
            trashed: trashed.as_deref(),
            latest: &latest.2,
            latest_number: rows
                .captures
                .iter()
                .position(|(c, _, _)| *c == latest_id)
                .unwrap_or(0)
                + 1,
            added_at: &added_at,
            last_seen: &rows.last_seen,
            context: &rows.context,
            categories: &categories,
            versions: &version_json,
            notes: &rows.notes,
            drafts: &draft_json,
            analyses: &analysis_json,
            tasks: &doc_tasks,
            attachments: &attachments,
            links: &rows.links,
            projects: &self.projects,
            category_names: &self.categories,
        }
        .render();
        self.put(&format!("{rel_dir}/documento.md"), md.as_bytes())?;

        let record = DocumentJson {
            id,
            numero: &number,
            titulo: &title,
            pasta: rel_dir.clone(),
            em_lixeira: trashed.is_some(),
            excluido_em: trashed.clone(),
            origem: &latest.2.source_url,
            manual,
            adicionado_em: &added_at,
            ultima_visualizacao: rows.last_seen.clone(),
            versao_atual_id: latest_id,
            contexto_de_trabalho: &rows.context,
            categorias: categories,
            nota_atual: &note,
            notas: &rows.notes,
            versoes: version_json,
            minutas: &draft_json,
            analises: &analysis_json,
            tarefas: doc_tasks.iter().map(|(t, _)| t.id).collect(),
            anexos: &attachments,
            vinculos: &rows.links,
        };
        let line = serde_json::to_string(&record)?;
        self.docs.push(DocIndex {
            id,
            number,
            title,
            folder: rel_dir,
            trashed: trashed.is_some(),
            categories: rows.context.categories.clone(),
        });
        if trashed.is_some() {
            self.report.trashed_documents += 1;
        } else {
            self.report.documents += 1;
        }
        Ok(Some(line))
    }

    fn loose_attachments(
        &mut self,
        tasks: &[(Task, Option<String>)],
        attendances: &[crate::saiop::SaiopAttendance],
    ) -> Result<Vec<LooseFileJson>> {
        let rows: Vec<(i64, String, i64)> = {
            let mut statement = self.store.conn.prepare(
                "SELECT id,owner_kind,owner_id FROM attachments WHERE owner_kind IN ('task','saiop') ORDER BY id",
            )?;
            let rows = statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        (self.step)("Gravando anexos de tarefas e atendimentos…");
        let titles: HashMap<i64, &str> = tasks
            .iter()
            .map(|(t, _)| (t.id, t.title.as_str()))
            .collect();
        let references: HashMap<i64, &str> = attendances
            .iter()
            .map(|a| (a.id, a.reference_raw.as_str()))
            .collect();
        let mut folders: HashMap<String, Dir> = HashMap::new();
        let mut folder_names = Names::default();
        let mut owner_folders: HashMap<(String, i64), String> = HashMap::new();
        let mut out = Vec::new();
        for (id, kind, owner) in rows {
            let in_trash = self.trash.contains_key(&("attachment".to_owned(), id));
            let owner_trashed = kind == "task"
                && tasks
                    .iter()
                    .any(|(t, trashed)| t.id == owner && trashed.is_some());
            let (kind_name, base, label) = if kind == "task" {
                (
                    "tarefa",
                    "anexos-de-tarefas",
                    format!(
                        "tarefa-{owner} - {}",
                        titles.get(&owner).copied().unwrap_or("")
                    ),
                )
            } else {
                (
                    "atendimento",
                    "anexos-de-atendimentos",
                    format!(
                        "atendimento-{owner} - {}",
                        references.get(&owner).copied().unwrap_or("")
                    ),
                )
            };
            let base = if in_trash || owner_trashed {
                "lixeira/anexos"
            } else {
                base
            };
            let key = (format!("{base}/{kind}"), owner);
            let folder = owner_folders
                .entry(key.clone())
                .or_insert_with(|| {
                    let name = folder_names.take(&readable(&label), false);
                    format!("{base}/{name}")
                })
                .clone();
            let dir = folders.entry(folder.clone()).or_default();
            let record = self.place_attachment(id, in_trash, &folder, dir)?;
            out.push(LooseFileJson {
                dono_tipo: kind_name,
                dono_id: owner,
                anexo: record,
            });
        }
        Ok(out)
    }
}

fn file_json(
    meta: &AttachmentMeta,
    trashed: bool,
    path: Option<String>,
    error: Option<String>,
) -> FileJson {
    FileJson {
        id: meta.id,
        nome: meta.name.clone(),
        tipo: meta.kind.clone(),
        tamanho: meta.size,
        sha256: meta.sha256.clone(),
        descricao: meta.description.clone(),
        adicionado_por: meta.added_by.clone(),
        adicionado_em: meta.added_at.clone(),
        em_lixeira: trashed,
        arquivo: path,
        erro: error,
    }
}

/// The name a SIGADOC file had: the last part of its address, else its label.
fn pdf_name(summary: &FileSummary) -> String {
    let from_url = url::Url::parse(&summary.url).ok().and_then(|u| {
        u.path_segments()
            .and_then(|mut s| s.next_back().map(str::to_owned))
    });
    let decoded = from_url
        .map(|s| percent_decode(&s))
        .filter(|s| !s.trim().is_empty());
    let mut name = decoded.unwrap_or_else(|| one_line(&summary.label));
    if name.trim().is_empty() {
        name = format!("arquivo-{}", summary.id);
    }
    if !name.to_lowercase().ends_with(".pdf") {
        name.push_str(".pdf");
    }
    name
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

fn version_markdown(
    n: usize,
    capture_id: i64,
    saved_at: &str,
    current: bool,
    trashed: bool,
    capture: &Capture,
) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# Versão {n:02} — {} — {}",
        one_line(&capture.document_number),
        one_line(&capture.title)
    );
    s.push('\n');
    if trashed {
        s.push_str("> ⚠ Esta versão estava na Lixeira do Ponte.\n\n");
    }
    let _ = writeln!(s, "- Versão atual: {}", if current { "sim" } else { "não" });
    let _ = writeln!(s, "- Código interno da versão: {capture_id}");
    let _ = writeln!(s, "- Guardada no Ponte em: {saved_at}");
    let _ = writeln!(s, "- Capturada em: {}", capture.captured_at);
    let _ = writeln!(s, "- Origem: {}", capture.source_url);
    if !capture.coverage.adapter.is_empty() {
        let _ = writeln!(s, "- Leitor: {}", capture.coverage.adapter);
    }
    for (key, value) in &capture.metadata {
        let _ = writeln!(s, "- {}: {}", one_line(key), one_line(value));
    }
    if !capture.coverage.pending.is_empty() {
        s.push_str("\n## Pendências da captura\n\n");
        for item in &capture.coverage.pending {
            let _ = writeln!(s, "- {}", one_line(item));
        }
    }
    if !capture.references.is_empty() {
        s.push_str("\n## Referências\n\n");
        for reference in &capture.references {
            let _ = writeln!(
                s,
                "- {} ({}): {}",
                one_line(&reference.label),
                reference.kind,
                reference.url
            );
        }
    }
    s.push_str("\n## Texto\n\n");
    s.push_str(&fenced(&capture.text));
    s
}

struct DocumentMarkdown<'a> {
    id: i64,
    number: &'a str,
    title: &'a str,
    trashed: Option<&'a str>,
    latest: &'a Capture,
    latest_number: usize,
    added_at: &'a str,
    last_seen: &'a str,
    context: &'a WorkContext,
    categories: &'a [CategoryRef],
    versions: &'a [VersionJson<'a>],
    notes: &'a [NoteJson],
    drafts: &'a [DraftJson],
    analyses: &'a [AnalysisJson],
    tasks: &'a [&'a (Task, Option<String>)],
    attachments: &'a [FileJson],
    links: &'a [LinkJson],
    projects: &'a BTreeMap<String, String>,
    category_names: &'a BTreeMap<String, String>,
}

impl DocumentMarkdown<'_> {
    fn render(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# {} — {}\n",
            one_line(self.number),
            one_line(self.title)
        );
        if let Some(at) = self.trashed {
            let _ = writeln!(
                s,
                "> ⚠ **Este documento estava na Lixeira do Ponte** (excluído em {at}). Está aqui só para consulta.\n"
            );
        }
        s.push_str("## Dados\n\n");
        let _ = writeln!(s, "- Número: {}", self.number);
        let _ = writeln!(s, "- Título: {}", self.title);
        let _ = writeln!(s, "- Código interno no Ponte: {}", self.id);
        let _ = writeln!(s, "- Origem: {}", self.latest.source_url);
        let _ = writeln!(s, "- Entrou no Ponte em: {}", self.added_at);
        let _ = writeln!(s, "- Visto pela última vez em: {}", self.last_seen);
        let _ = writeln!(s, "- Finalidade: {}", self.context.purpose);
        if !self.context.subjects.trim().is_empty() {
            let _ = writeln!(s, "- Assuntos: {}", self.context.subjects);
        }
        if !self.context.document_type.trim().is_empty() {
            let _ = writeln!(s, "- Tipo: {}", self.context.document_type);
        }
        if !self.context.priority.trim().is_empty() {
            let _ = writeln!(s, "- Prioridade: {}", self.context.priority);
        }
        for (label, value) in [
            ("Remetente", &self.context.sender),
            ("Recebido em", &self.context.received_date),
            ("Processo", &self.context.process_number),
        ] {
            if !value.trim().is_empty() {
                let _ = writeln!(s, "- {label}: {value}");
            }
        }
        if !self.categories.is_empty() {
            let names: Vec<&str> = self.categories.iter().map(|c| c.nome.as_str()).collect();
            let _ = writeln!(s, "- Categorias: {}", names.join(", "));
        }
        if !self.context.notes.trim().is_empty() {
            let _ = writeln!(
                s,
                "- Anotações de contexto: {}",
                one_line(&self.context.notes)
            );
        }

        let _ = writeln!(s, "\n## Texto atual (versão {:02})\n", self.latest_number);
        s.push_str(&fenced(&self.latest.text));

        s.push_str("\n## Histórico de versões\n\n");
        for version in self.versions {
            let _ = writeln!(
                s,
                "- Versão {:02}{} — guardada em {} — [`{}`]({}){}",
                version.numero_da_versao,
                if version.atual { " (atual)" } else { "" },
                version.importada_em,
                version.arquivo,
                version.arquivo,
                if version.em_lixeira {
                    " — estava na Lixeira"
                } else {
                    ""
                }
            );
            for pdf in &version.pdfs {
                match (&pdf.arquivo, &pdf.erro) {
                    (Some(path), _) => {
                        let relative = path.rsplit_once("/anexos/").map(|(_, n)| n).unwrap_or(path);
                        let _ = writeln!(
                            s,
                            "  - PDF: [{}](anexos/{})",
                            one_line(&pdf.rotulo),
                            relative
                        );
                    }
                    (None, error) => {
                        let _ = writeln!(
                            s,
                            "  - PDF «{}» não exportado: {}",
                            one_line(&pdf.rotulo),
                            error.as_deref().unwrap_or("motivo desconhecido")
                        );
                    }
                }
            }
        }

        s.push_str("\n## Nota\n\n");
        match self.notes.last() {
            Some(note) if !note.texto.trim().is_empty() => {
                s.push_str(&fenced(&note.texto));
            }
            _ => s.push_str("(sem nota)\n"),
        }
        if self.notes.len() > 1 {
            s.push_str("\n### Histórico da nota\n\n");
            for note in self.notes.iter().rev().skip(1) {
                let _ = writeln!(s, "#### Nota de {}\n", note.criada_em);
                s.push_str(&fenced(&note.texto));
                s.push('\n');
            }
        }

        s.push_str("\n## Minutas\n\n");
        if self.drafts.is_empty() {
            s.push_str("(nenhuma minuta)\n");
        }
        for draft in self.drafts.iter().rev() {
            let _ = writeln!(
                s,
                "### Minuta {} — {} — {}{}\n",
                draft.id,
                draft.tipo,
                draft.criada_em,
                if draft.de_versao_antiga {
                    " (de uma versão antiga do documento)"
                } else {
                    ""
                }
            );
            if let Some(origin) = &draft.proveniencia {
                if !origin.model.is_empty() || !origin.template_name.is_empty() {
                    let _ = writeln!(
                        s,
                        "Origem: modelo «{}», IA «{}»\n",
                        one_line(&origin.template_name),
                        one_line(&origin.model)
                    );
                }
            }
            s.push_str(&fenced(&draft.texto));
            s.push('\n');
        }

        s.push_str("\n## Análise\n\n");
        match self.analyses.last() {
            None => s.push_str("(sem análise)\n"),
            Some(latest) => {
                let a = &latest.analise;
                let _ = writeln!(
                    s,
                    "Feita em {} ({}), sobre a versão de código {}.\n",
                    latest.criada_em,
                    nonempty(&a.model, "modelo não informado"),
                    latest.versao_id
                );
                let _ = writeln!(s, "- Prioridade: {}", a.priority);
                if !a.subjects.is_empty() {
                    let _ = writeln!(s, "- Assuntos: {}", a.subjects.join("; "));
                }
                if let Some(deadline) = &a.deadline_candidate {
                    let _ = writeln!(s, "- Prazo possível: {deadline}");
                }
                let _ = writeln!(s, "- Motivo: {}", one_line(&a.reason));
                let _ = writeln!(s, "\n**Resumo:** {}\n", one_line(&a.summary));
                if !a.facts.is_empty() {
                    s.push_str("Fatos conferidos:\n\n");
                    for fact in &a.facts {
                        let _ = writeln!(
                            s,
                            "- ({}) {} — «{}»",
                            fact.kind,
                            one_line(&fact.statement),
                            one_line(&fact.quote)
                        );
                    }
                    s.push('\n');
                }
                if !a.suggested_actions.is_empty() {
                    s.push_str("Ações sugeridas:\n\n");
                    for action in &a.suggested_actions {
                        let _ = writeln!(
                            s,
                            "- {} — {}",
                            one_line(&action.action),
                            one_line(&action.reason)
                        );
                    }
                    s.push('\n');
                }
                if !a.missing_context.is_empty() {
                    s.push_str("Faltou saber:\n\n");
                    for item in &a.missing_context {
                        let _ = writeln!(s, "- {}", one_line(item));
                    }
                }
                if self.analyses.len() > 1 {
                    let _ = writeln!(
                        s,
                        "\nHá mais {} análise(s) anteriores em `acervo.json`.",
                        self.analyses.len() - 1
                    );
                }
            }
        }

        s.push_str("\n## Tarefas\n\n");
        if self.tasks.is_empty() {
            s.push_str("(nenhuma tarefa)\n");
        }
        for (task, trashed) in self.tasks {
            s.push_str(&task_block(
                task,
                trashed.as_deref(),
                self.projects,
                self.category_names,
                None,
            ));
        }

        s.push_str("\n## Anexos\n\n");
        let active: Vec<&FileJson> = self
            .attachments
            .iter()
            .filter(|a| !a.em_lixeira || self.trashed.is_some())
            .collect();
        if active.is_empty() {
            s.push_str("(nenhum anexo)\n");
        }
        for attachment in active {
            match (&attachment.arquivo, &attachment.erro) {
                (Some(path), _) => {
                    let relative = path.rsplit_once("/anexos/").map(|(_, n)| n).unwrap_or(path);
                    let _ = writeln!(
                        s,
                        "- [{}](anexos/{}) — {}{}",
                        one_line(&attachment.nome),
                        relative,
                        attachment.tipo,
                        if attachment.descricao.trim().is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", one_line(&attachment.descricao))
                        }
                    );
                }
                (None, error) => {
                    let _ = writeln!(
                        s,
                        "- {} — não exportado: {}",
                        one_line(&attachment.nome),
                        error.as_deref().unwrap_or("motivo desconhecido")
                    );
                }
            }
        }
        let in_trash = self.attachments.iter().filter(|a| a.em_lixeira).count();
        if in_trash > 0 && self.trashed.is_none() {
            let _ = writeln!(s, "\n{in_trash} anexo(s) deste documento estavam na Lixeira e estão na seção `lixeira/anexos`.");
        }

        if !self.links.is_empty() {
            s.push_str("\n## Vínculos com outros documentos\n\n");
            for link in self.links {
                let _ = writeln!(
                    s,
                    "- {} → {} ({}){}{}",
                    link.origem_id,
                    link.destino_id,
                    link.tipo,
                    if link.motivo.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", one_line(&link.motivo))
                    },
                    if link.ativo { "" } else { " — desfeito" }
                );
            }
        }
        s
    }
}

fn task_block(
    task: &Task,
    trashed: Option<&str>,
    projects: &BTreeMap<String, String>,
    categories: &BTreeMap<String, String>,
    document: Option<&str>,
) -> String {
    let mut s = String::new();
    let done = task.status == "concluida";
    let _ = writeln!(
        s,
        "- [{}] **{}** (tarefa {})",
        if done { "x" } else { " " },
        one_line(&task.title),
        task.id
    );
    let mut facts = vec![format!(
        "Situação: {}",
        crate::tasks::status_label(&task.status)
    )];
    if !task.due_date.is_empty() {
        facts.push(format!("Prazo: {}", iso_date(&task.due_date)));
    }
    facts.push(format!(
        "Prioridade: {}",
        crate::tasks::priority_label(&task.priority)
    ));
    if !task.assignee.trim().is_empty() {
        facts.push(format!("Responsável: {}", one_line(&task.assignee)));
    }
    if !task.action.trim().is_empty() {
        facts.push(format!("Ação: {}", task.action));
    }
    let _ = writeln!(s, "  - {}", facts.join(" · "));
    if let Some(document) = document {
        let _ = writeln!(s, "  - Documento: {document}");
    }
    if let Some(name) = projects.get(&task.project_uid) {
        let _ = writeln!(s, "  - Projeto: {}", one_line(name));
    }
    let names: Vec<&str> = task
        .categories
        .iter()
        .filter_map(|uid| categories.get(uid).map(String::as_str))
        .collect();
    if !names.is_empty() {
        let _ = writeln!(s, "  - Categorias: {}", names.join(", "));
    }
    if !task.scheduled_start.is_empty() {
        let _ = writeln!(
            s,
            "  - Agendada: {} a {}",
            task.scheduled_start, task.scheduled_end
        );
    }
    if !task.reminder_date.is_empty() {
        let _ = writeln!(s, "  - Lembrete: {}", task.reminder_date);
    }
    if let Some(at) = &task.completed_at {
        let _ = writeln!(s, "  - Concluída em: {}", iso_date(at));
    }
    if !task.outcome.trim().is_empty() {
        let _ = writeln!(s, "  - Resultado: {}", one_line(&task.outcome));
    }
    if !task.notes.trim().is_empty() {
        let _ = writeln!(s, "  - Observações: {}", one_line(&task.notes));
    }
    for (checked, item) in &task.checklist {
        let _ = writeln!(
            s,
            "  - [{}] {}",
            if *checked { "x" } else { " " },
            one_line(item)
        );
    }
    if let Some(at) = trashed {
        let _ = writeln!(s, "  - ⚠ Estava na Lixeira (excluída em {at})");
    }
    s
}

impl Run<'_> {
    fn overviews(
        &mut self,
        tasks: &[(Task, Option<String>)],
        projects: &[crate::projects::Project],
        attendances: &[crate::saiop::SaiopAttendance],
        resources: &[crate::resources::Resource],
        uses: &[crate::resources::ResourceUse],
        categories: &[crate::categories::Category],
    ) -> Result<()> {
        let docs: HashMap<i64, String> = self
            .docs
            .iter()
            .map(|d| {
                (
                    d.id,
                    format!(
                        "{} — {} (`{}`)",
                        one_line(&d.number),
                        one_line(&d.title),
                        d.folder
                    ),
                )
            })
            .collect();

        // tarefas.md and lixeira/tarefas.md
        for (trash, rel, heading) in [
            (false, "tarefas.md", "Tarefas"),
            (true, "lixeira/tarefas.md", "Tarefas que estavam na Lixeira"),
        ] {
            let mut s = format!("# {heading}\n\n");
            if trash {
                s.push_str("> ⚠ Estas tarefas estavam na Lixeira do Ponte (ou pertenciam a um documento da Lixeira). Estão aqui só para consulta.\n\n");
            }
            let selected: Vec<&(Task, Option<String>)> = tasks
                .iter()
                .filter(|(_, trashed)| trashed.is_some() == trash)
                .collect();
            if selected.is_empty() {
                s.push_str("(nenhuma tarefa)\n");
            }
            let mut order: Vec<String> = ["pendente", "em analise", "aguardando", "concluida"]
                .iter()
                .map(|v| (*v).to_owned())
                .collect();
            for (task, _) in &selected {
                if !order.contains(&task.status) {
                    order.push(task.status.clone());
                }
            }
            for status in order {
                let mut group: Vec<&&(Task, Option<String>)> = selected
                    .iter()
                    .filter(|(t, _)| t.status == status)
                    .collect();
                if group.is_empty() {
                    continue;
                }
                group.sort_by_key(|(t, _)| (t.due_date.is_empty(), t.due_date.clone(), t.id));
                let _ = writeln!(
                    s,
                    "## {} ({})\n",
                    crate::tasks::status_label(&status),
                    group.len()
                );
                for (task, trashed) in group.into_iter().map(|g| &**g) {
                    let document = match task.document_id {
                        Some(id) => docs
                            .get(&id)
                            .map(String::as_str)
                            .or(Some("documento não exportado")),
                        None => Some("sem documento"),
                    };
                    s.push_str(&task_block(
                        task,
                        trashed.as_deref().filter(|_| trash),
                        &self.projects,
                        &self.categories,
                        document,
                    ));
                }
                s.push('\n');
            }
            if trash && selected.is_empty() {
                continue;
            }
            self.put(rel, s.as_bytes())?;
        }

        // projetos.md
        let mut s = String::from("# Projetos\n\n");
        if projects.is_empty() {
            s.push_str("(nenhum projeto)\n");
        }
        for project in projects {
            let _ = writeln!(s, "## {}\n", one_line(&project.name));
            let _ = writeln!(s, "- Situação: {}", project.status.label());
            if !project.objective.trim().is_empty() {
                let _ = writeln!(s, "- Objetivo: {}", one_line(&project.objective));
            }
            if !project.assignee.trim().is_empty() {
                let _ = writeln!(s, "- Responsável: {}", one_line(&project.assignee));
            }
            if !project.start_date.is_empty() {
                let _ = writeln!(s, "- Início: {}", project.start_date);
            }
            if !project.target_date.is_empty() {
                let _ = writeln!(s, "- Meta: {}", project.target_date);
            }
            let _ = writeln!(s, "- Código (uid): {}", project.uid);
            let own: Vec<&(Task, Option<String>)> = tasks
                .iter()
                .filter(|(t, _)| t.project_uid == project.uid)
                .collect();
            if !own.is_empty() {
                s.push_str("\nTarefas do projeto:\n\n");
                for (task, trashed) in own {
                    let _ = writeln!(
                        s,
                        "- [{}] {} (tarefa {}){}",
                        if task.status == "concluida" { "x" } else { " " },
                        one_line(&task.title),
                        task.id,
                        if trashed.is_some() {
                            " — estava na Lixeira"
                        } else {
                            ""
                        }
                    );
                }
            }
            s.push('\n');
        }
        self.put("projetos.md", s.as_bytes())?;

        // atendimentos.md
        let mut s = String::from("# Atendimentos\n\n");
        if attendances.is_empty() {
            s.push_str("(nenhum atendimento)\n");
        }
        for a in attendances {
            let _ = writeln!(
                s,
                "## {} — {}\n",
                one_line(nonempty(&a.reference_raw, "sem referência")),
                one_line(&a.subject)
            );
            let _ = writeln!(s, "- Código interno: {}", a.id);
            let _ = writeln!(s, "- Situação: {}", a.status);
            let _ = writeln!(s, "- Prioridade: {}", a.priority);
            if !a.entry_date.is_empty() {
                let _ = writeln!(s, "- Entrada: {}", iso_date(&a.entry_date));
            }
            if !a.due_date.is_empty() {
                let _ = writeln!(s, "- Prazo: {}", iso_date(&a.due_date));
            }
            if !a.exit_date.is_empty() {
                let _ = writeln!(s, "- Saída: {}", iso_date(&a.exit_date));
            }
            if !a.assignee.trim().is_empty() {
                let _ = writeln!(s, "- Responsável: {}", one_line(&a.assignee));
            }
            if !a.received_from.trim().is_empty() {
                let _ = writeln!(s, "- Recebido de: {}", one_line(&a.received_from));
            }
            if !a.sent_to.trim().is_empty() {
                let _ = writeln!(s, "- Enviado a: {}", one_line(&a.sent_to));
            }
            if let Some(document) = a.document_id.and_then(|d| docs.get(&d)) {
                let _ = writeln!(s, "- Documento: {document}");
            }
            if !a.observation.trim().is_empty() {
                let _ = writeln!(s, "- Observação: {}", one_line(&a.observation));
            }
            if !a.outcome.trim().is_empty() {
                let _ = writeln!(s, "- Resultado: {}", one_line(&a.outcome));
            }
            s.push('\n');
        }
        self.put("atendimentos.md", s.as_bytes())?;

        // recursos.md
        let mut s = String::from("# Recursos\n\n");
        if resources.is_empty() {
            s.push_str("(nenhum recurso)\n");
        }
        for resource in resources {
            let _ = writeln!(s, "## {}\n", one_line(&resource.name));
            let _ = writeln!(s, "- Tipo: {}", resource.kind);
            let _ = writeln!(s, "- Quantidade: {}", resource.quantity);
            let _ = writeln!(s, "- Situação: {}", resource.status.label());
            if !resource.location.trim().is_empty() {
                let _ = writeln!(s, "- Onde fica: {}", one_line(&resource.location));
            }
            if !resource.identification.trim().is_empty() {
                let _ = writeln!(s, "- Identificação: {}", one_line(&resource.identification));
            }
            if !resource.notes.trim().is_empty() {
                let _ = writeln!(s, "- Notas: {}", one_line(&resource.notes));
            }
            let _ = writeln!(s, "- Código (uid): {}", resource.uid);
            let own: Vec<&crate::resources::ResourceUse> = uses
                .iter()
                .filter(|u| u.resource_uid == resource.uid)
                .collect();
            if !own.is_empty() {
                s.push_str("\nUsos (reservas e empréstimos):\n\n");
                for u in own {
                    let _ = write!(
                        s,
                        "- {} a {} — {} — {}",
                        u.start,
                        u.end,
                        u.status.label(),
                        one_line(nonempty(&u.who, "sem nome"))
                    );
                    if !u.sector.trim().is_empty() {
                        let _ = write!(s, " ({})", one_line(&u.sector));
                    }
                    if !u.purpose.trim().is_empty() {
                        let _ = write!(s, " — {}", one_line(&u.purpose));
                    }
                    if let Some(document) = u.document_id.and_then(|d| docs.get(&d)) {
                        let _ = write!(s, " — documento {document}");
                    }
                    s.push('\n');
                }
            }
            s.push('\n');
        }
        let known: HashSet<&str> = resources.iter().map(|r| r.uid.as_str()).collect();
        let orphans: Vec<&crate::resources::ResourceUse> = uses
            .iter()
            .filter(|u| !known.contains(u.resource_uid.as_str()))
            .collect();
        if !orphans.is_empty() {
            s.push_str("## Usos de recursos que não existem mais\n\n");
            for u in orphans {
                let _ = writeln!(
                    s,
                    "- {} a {} — {} ({})",
                    u.start,
                    u.end,
                    one_line(&u.who),
                    u.resource_uid
                );
            }
        }
        self.put("recursos.md", s.as_bytes())?;

        // categorias.md
        let mut s = String::from("# Categorias\n\n");
        if categories.is_empty() {
            s.push_str("(nenhuma categoria)\n");
        }
        for category in categories {
            let _ = writeln!(
                s,
                "## {}{}\n",
                one_line(&category.name),
                if category.archived {
                    " (arquivada)"
                } else {
                    ""
                }
            );
            let _ = writeln!(s, "- Código (uid): {}", category.uid);
            let documents: Vec<&DocIndex> = self
                .docs
                .iter()
                .filter(|d| d.categories.contains(&category.uid))
                .collect();
            if !documents.is_empty() {
                s.push_str("\nDocumentos:\n\n");
                for d in documents {
                    let _ = writeln!(
                        s,
                        "- {} — {} (`{}`){}",
                        one_line(&d.number),
                        one_line(&d.title),
                        d.folder,
                        if d.trashed {
                            " — estava na Lixeira"
                        } else {
                            ""
                        }
                    );
                }
            }
            let own: Vec<&(Task, Option<String>)> = tasks
                .iter()
                .filter(|(t, _)| t.categories.contains(&category.uid))
                .collect();
            if !own.is_empty() {
                s.push_str("\nTarefas:\n\n");
                for (task, trashed) in own {
                    let _ = writeln!(
                        s,
                        "- {} (tarefa {}){}",
                        one_line(&task.title),
                        task.id,
                        if trashed.is_some() {
                            " — estava na Lixeira"
                        } else {
                            ""
                        }
                    );
                }
            }
            s.push('\n');
        }
        self.put("categorias.md", s.as_bytes())?;

        if self.report.trashed_documents > 0
            || self.report.trashed_tasks > 0
            || !self.trash.is_empty()
        {
            self.put(
                "lixeira/ESTES-ITENS-ESTAVAM-NA-LIXEIRA.txt",
                "Tudo o que está dentro da pasta «lixeira» estava na Lixeira do Ponte quando esta exportação foi feita (documentos excluídos, tarefas excluídas, anexos excluídos). Ainda não tinha sido apagado de vez. Está aqui só para consulta e não faz parte do acervo em uso.\n\nVersões importadas que estavam na Lixeira ficam dentro da pasta do próprio documento, em `versoes/`, marcadas no texto.\n".as_bytes(),
            )?;
        }
        Ok(())
    }

    fn leia_me(&mut self, started: &chrono::DateTime<chrono::Local>) -> Result<()> {
        let r = &self.report;
        let mut s = String::new();
        let _ = writeln!(s, "EXPORTAÇÃO ABERTA DO ACERVO DO PONTE\n");
        let _ = writeln!(s, "Feita em {}", started.format("%d/%m/%Y às %H:%M"));
        let _ = writeln!(s, "Versão do Ponte: {}\n", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(s, "ATENÇÃO: {NOT_ENCRYPTED}\n");
        s.push_str("O QUE HÁ AQUI\n\n");
        s.push_str("  LEIA-ME.txt          este arquivo.\n");
        s.push_str("  documentos/          uma pasta por documento («número - título»), com:\n");
        s.push_str("                         documento.md  dados, texto atual, histórico de versões, nota e\n");
        s.push_str("                                       seu histórico, minutas, análise, tarefas, anexos;\n");
        s.push_str("                         versoes/NN.md o texto de cada versão importada;\n");
        s.push_str("                         anexos/       os PDFs do SIGADOC e os anexos, com o nome original\n");
        s.push_str("                                       (nomes repetidos ganham « (2)»; o mesmo arquivo com o\n");
        s.push_str("                                       mesmo nome é gravado uma vez só).\n");
        s.push_str(
            "  tarefas.md           todas as tarefas, também as sem documento, por situação.\n",
        );
        s.push_str("  projetos.md          projetos e suas tarefas.\n");
        s.push_str("  atendimentos.md      atendimentos.\n");
        s.push_str("  recursos.md          recursos e seus usos (reservas e empréstimos).\n");
        s.push_str("  categorias.md        categorias e o que está em cada uma.\n");
        s.push_str("  anexos-de-tarefas/, anexos-de-atendimentos/   anexos que não são de documentos (se houver).\n");
        s.push_str(
            "  acervo.json          tudo em um só arquivo JSON, para programas (forma abaixo).\n",
        );
        s.push_str("  lixeira/             SEÇÃO SEPARADA: o que estava na Lixeira do Ponte (documentos,\n");
        s.push_str("                       tarefas, anexos excluídos que ainda não tinham sido apagados de vez).\n");
        s.push_str("                       Não faz parte do acervo em uso; está aqui só para consulta.\n\n");
        s.push_str("QUANTO\n\n");
        let _ = writeln!(
            s,
            "  Documentos: {} (mais {} da Lixeira), {} versões",
            r.documents, r.trashed_documents, r.versions
        );
        let _ = writeln!(
            s,
            "  Tarefas: {} (mais {} da Lixeira)",
            r.tasks, r.trashed_tasks
        );
        let _ = writeln!(
            s,
            "  Projetos: {}, atendimentos: {}, recursos: {}, usos de recursos: {}, categorias: {}",
            r.projects, r.attendances, r.resources, r.resource_uses, r.categories
        );
        let _ = writeln!(
            s,
            "  Arquivos gravados (PDFs, anexos e textos): {}\n",
            r.files
        );
        s.push_str("O QUE NÃO ESTÁ AQUI, E POR QUÊ\n\n");
        for line in [
            "Senhas, chaves da IA, contas e senhas de e-mail e dados de acesso: são segredos, e esta pasta é aberta.",
            "Ajustes do Ponte, configuração de equipe e de sincronização, filas e histórico de envio: dizem respeito a este computador, não ao conteúdo.",
            "Registros internos de gerações e conferências da IA e consentimentos: só o texto das minutas e as análises vão.",
            "Planilhas originais importadas e linhas brutas dos atendimentos: vai o atendimento como o Ponte o guarda.",
            "Modelos de trabalho (e seus exemplos) e textos extraídos de páginas de PDF para a busca: são do Ponte; os PDFs inteiros vão.",
            "Quem fez cada alteração e as versões internas de sincronização dos itens.",
        ] {
            let _ = writeln!(s, "  - {line}");
        }
        s.push_str("  - O que aparece em «AVISOS» abaixo, se houver.\n\n");
        if !r.warnings.is_empty() {
            s.push_str("AVISOS (o que não pôde ser gravado)\n\n");
            for warning in &r.warnings {
                let _ = writeln!(s, "  - {}", one_line(warning));
            }
            s.push('\n');
        }
        s.push_str("FORMA DO acervo.json\n\n");
        s.push_str(JSON_SHAPE);
        s.push('\n');
        self.put("LEIA-ME.txt", s.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attachments::Owner;
    use crate::files::FileInput;
    use crate::model::{sample_capture, DocumentReference};
    use crate::trash::{Kind, Target};
    use base64::Engine;
    use std::cell::Cell;

    fn capture(number: &str, title: &str, text: &str) -> Capture {
        let mut c = sample_capture();
        c.request_id = uuid::Uuid::new_v4().to_string();
        c.document_number = number.into();
        c.title = title.into();
        c.text = text.into();
        c.source_url = format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla={}",
            number.replace('/', "%2F")
        );
        c
    }

    fn silent(_: &str) {}
    fn never() -> bool {
        false
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        out: PathBuf,
        main_id: i64,
        trashed_id: i64,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let number = "SEFAZ-PRO-2026/00001";
        let first = store
            .import_capture(&capture(
                number,
                "Solicitação de material",
                "Texto da primeira versão.",
            ))
            .unwrap();
        store
            .import_capture(&capture(
                number,
                "Solicitação de material",
                "Texto da segunda versão.",
            ))
            .unwrap();
        // The third version brings a PDF.
        let mut third = capture(
            number,
            "Solicitação de material",
            "Texto da terceira versão, com ``` crases.",
        );
        let url = "https://www.sigadoc.mt.gov.br/arquivos/oficio%20n%C2%BA1.pdf".to_owned();
        third.references.push(DocumentReference {
            label: "Ofício nº 1".into(),
            url: url.clone(),
            kind: "attachment".into(),
        });
        let pdf = include_bytes!("../tests/fixtures/document.pdf");
        let receipt = store
            .import_bundle(
                &third,
                &[FileInput {
                    url,
                    data_base64: base64::engine::general_purpose::STANDARD.encode(pdf),
                }],
            )
            .unwrap();
        let id = first.document_id;
        assert_eq!(receipt.document_id, id);
        store.save_note(id, "Primeira nota.").unwrap();
        store.save_note(id, "Nota refeita com acentuação.").unwrap();
        store
            .save_draft(id, receipt.capture_id, "resposta", "Minuta um.")
            .unwrap();
        store
            .save_draft(id, receipt.capture_id, "resposta", "Minuta dois.")
            .unwrap();
        store
            .save_analysis(
                id,
                &Analysis {
                    capture_id: receipt.capture_id,
                    subjects: vec!["material".into()],
                    priority: "normal".into(),
                    reason: "Sem prazo expresso.".into(),
                    summary: "Resumo da análise de teste.".into(),
                    model: "synthetic-test".into(),
                    created_at: "2026-11-02T10:00:00.000Z".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .add_attachment(
                Owner::Document(id),
                "relatório.txt",
                "Relatório de apoio",
                "conteúdo do anexo".as_bytes(),
            )
            .unwrap();
        let category = store
            .save_category(&crate::categories::Category::new("Urgente", 0))
            .unwrap();
        let mut context = store.load_work_context(id).unwrap();
        context.categories = vec![category.uid.clone()];
        store.save_work_context(id, &context).unwrap();
        let project = store
            .save_project(&crate::projects::Project {
                name: "Projeto Piloto".into(),
                ..Default::default()
            })
            .unwrap();
        let mut task = store.list_tasks().unwrap().remove(0);
        task.title = "Responder o ofício".into();
        task.due_date = "2026-12-01".into();
        task.categories = vec![category.uid.clone()];
        task.project_uid = project.uid.clone();
        store.save_task(&task).unwrap();
        let mut loose = crate::tasks::Task::standalone();
        loose.title = "Tarefa sem documento".into();
        loose.status = "concluida".into();
        store.save_task(&loose).unwrap();
        let resource = crate::resources::Resource {
            name: "Projetor".into(),
            ..Default::default()
        };
        let resource_uid = store.save_resource(&resource, None).unwrap();
        store
            .save_resource_use(
                &crate::resources::ResourceUse {
                    resource_uid,
                    who: "Fulano".into(),
                    start: "2026-11-20T09:00".into(),
                    end: "2026-11-20T10:00".into(),
                    document_id: Some(id),
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        let other = store
            .import_capture(&capture(
                "SEFAZ-PRO-2026/00002",
                "Documento excluído",
                "Texto do excluído.",
            ))
            .unwrap();
        store
            .trash_item(Target {
                kind: Kind::Document,
                id: other.document_id,
            })
            .unwrap();
        let out = dir.path().join("saida");
        fs::create_dir_all(&out).unwrap();
        Fixture {
            _dir: dir,
            store,
            out,
            main_id: id,
            trashed_id: other.document_id,
        }
    }

    fn tree(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    out.push(
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn exports_the_whole_library_as_open_files() {
        let f = fixture();
        let report = export_open(&f.store, &f.out, &silent, &never).unwrap();
        assert_eq!((report.documents, report.trashed_documents), (1, 1));
        assert_eq!(report.versions, 4);
        assert_eq!((report.tasks, report.trashed_tasks), (2, 1));
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let files = tree(&f.out);
        assert!(!files.iter().any(|n| n == MARKER), "the marker is gone");
        let main = "documentos/SEFAZ-PRO-2026-00001 - Solicitação de material";
        for expected in [
            "LEIA-ME.txt".to_owned(),
            "acervo.json".to_owned(),
            "tarefas.md".to_owned(),
            "projetos.md".to_owned(),
            "atendimentos.md".to_owned(),
            "recursos.md".to_owned(),
            "categorias.md".to_owned(),
            format!("{main}/documento.md"),
            format!("{main}/versoes/01.md"),
            format!("{main}/versoes/02.md"),
            format!("{main}/versoes/03.md"),
            format!("{main}/anexos/oficio nº1.pdf"),
            format!("{main}/anexos/relatório.txt"),
            "lixeira/documentos/SEFAZ-PRO-2026-00002 - Documento excluído/documento.md".to_owned(),
            "lixeira/ESTES-ITENS-ESTAVAM-NA-LIXEIRA.txt".to_owned(),
        ] {
            assert!(
                files.contains(&expected),
                "missing {expected} in {files:#?}"
            );
        }
        let document = fs::read_to_string(f.out.join(format!("{main}/documento.md"))).unwrap();
        for essential in [
            "# SEFAZ-PRO-2026/00001 — Solicitação de material",
            "Texto da terceira versão",
            "Histórico de versões",
            "Versão 03 (atual)",
            "Nota refeita com acentuação.",
            "Histórico da nota",
            "Primeira nota.",
            "Minuta um.",
            "Minuta dois.",
            "Resumo da análise de teste.",
            "Responder o ofício",
            "Prazo: 2026-12-01",
            "Categorias: Urgente",
            "Projeto: Projeto Piloto",
            "relatório.txt",
        ] {
            assert!(
                document.contains(essential),
                "documento.md lacks {essential}"
            );
        }
        // The fence is longer than the backticks inside the text.
        assert!(document.contains("````text"));
        let trashed = fs::read_to_string(
            f.out
                .join("lixeira/documentos/SEFAZ-PRO-2026-00002 - Documento excluído/documento.md"),
        )
        .unwrap();
        assert!(trashed.contains("estava na Lixeira"));
        let tasks = fs::read_to_string(f.out.join("tarefas.md")).unwrap();
        assert!(tasks.contains("Tarefa sem documento") && tasks.contains("sem documento"));
        assert!(tasks.contains("## Concluída (1)") && tasks.contains("## Pendente"));
        let leia = fs::read_to_string(f.out.join("LEIA-ME.txt")).unwrap();
        assert!(leia.contains("NÃO é criptografada"));
        assert!(leia.contains("lixeira/"));
        assert!(leia.contains("O QUE NÃO ESTÁ AQUI"));
        assert!(leia.contains(env!("CARGO_PKG_VERSION")));
        // The PDF came out byte for byte.
        let pdf = fs::read(f.out.join(format!("{main}/anexos/oficio nº1.pdf"))).unwrap();
        assert_eq!(pdf, include_bytes!("../tests/fixtures/document.pdf"));

        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(f.out.join("acervo.json")).unwrap()).unwrap();
        assert_eq!(json["versao_do_formato"], FORMAT_VERSION);
        assert_eq!(json["formato"], "ponte-acervo-aberto");
        let documents = json["documentos"].as_array().unwrap();
        assert_eq!(documents.len(), 2);
        let doc = documents.iter().find(|d| d["id"] == f.main_id).unwrap();
        assert_eq!(doc["numero"], "SEFAZ-PRO-2026/00001");
        assert_eq!(doc["em_lixeira"], false);
        assert_eq!(doc["versoes"].as_array().unwrap().len(), 3);
        assert_eq!(doc["versoes"][2]["atual"], true);
        assert_eq!(doc["versoes"][2]["pdfs"].as_array().unwrap().len(), 1);
        assert_eq!(doc["notas"].as_array().unwrap().len(), 2);
        assert_eq!(doc["minutas"].as_array().unwrap().len(), 2);
        assert_eq!(doc["analises"].as_array().unwrap().len(), 1);
        assert_eq!(doc["anexos"].as_array().unwrap().len(), 1);
        assert_eq!(doc["categorias"][0]["nome"], "Urgente");
        assert_eq!(doc["tarefas"].as_array().unwrap().len(), 1);
        let gone = documents.iter().find(|d| d["id"] == f.trashed_id).unwrap();
        assert_eq!(gone["em_lixeira"], true);
        assert!(gone["excluido_em"].is_string());
        let tasks = json["tarefas"].as_array().unwrap();
        assert_eq!(tasks.len(), 3);
        assert!(tasks.iter().any(|t| t["tarefa"]["document_id"].is_null()));
        assert!(tasks.iter().any(|t| t["em_lixeira"] == true));
        assert_eq!(json["projetos"].as_array().unwrap().len(), 1);
        assert_eq!(json["atendimentos"].as_array().unwrap().len(), 2);
        assert_eq!(json["recursos"].as_array().unwrap().len(), 1);
        assert_eq!(json["usos_de_recursos"].as_array().unwrap().len(), 1);
        assert_eq!(json["categorias"].as_array().unwrap().len(), 1);
        assert!(json["lixeira"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["tipo"] == "documento" && i["id"] == f.trashed_id));
    }

    #[test]
    fn a_folder_with_anything_in_it_is_refused_and_left_alone() {
        let f = fixture();
        fs::write(f.out.join("meu.txt"), "meu").unwrap();
        let error = export_open(&f.store, &f.out, &silent, &never).unwrap_err();
        assert!(format!("{error:#}").contains("não está vazia"));
        assert_eq!(tree(&f.out), ["meu.txt"]);
        assert!(check_target_folder(&f.out.join("nao-existe")).is_err());
    }

    #[test]
    fn cancelling_leaves_the_marker_and_no_pretence_of_a_complete_folder() {
        let f = fixture();
        let calls = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() > 6
        };
        let error = export_open(&f.store, &f.out, &silent, &cancelled).unwrap_err();
        assert!(error.downcast_ref::<Cancelled>().is_some());
        let marker = fs::read_to_string(f.out.join(MARKER)).unwrap();
        assert!(marker.contains("INCOMPLETA") && marker.contains("CANCELADA"));
        assert!(!f.out.join("LEIA-ME.txt").exists());
    }

    #[test]
    fn unsafe_titles_become_windows_safe_names_that_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        for (number, title) in [
            ("SEFAZ-PRO-2026/00011", "AUX"),
            ("SEFAZ-PRO-2026/00012", "a<b>:c\"d\"|e?*f. "),
            ("SEFAZ-PRO-2026/00013", "...   "),
        ] {
            store
                .import_capture(&capture(number, title, "texto"))
                .unwrap();
        }
        let out = dir.path().join("saida");
        fs::create_dir_all(&out).unwrap();
        export_open(&store, &out, &silent, &never).unwrap();
        let names: Vec<String> = fs::read_dir(out.join("documentos"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        for name in &names {
            assert!(!name.chars().any(|c| "<>:\"/\\|?*".contains(c)), "{name}");
            assert!(!name.ends_with('.') && !name.ends_with(' '), "{name}");
        }
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.iter().any(|n| n == "SEFAZ-PRO-2026-00011 - AUX"));
        assert!(names.iter().any(|n| n == "SEFAZ-PRO-2026-00012 - abcdef"));
        assert!(names.iter().any(|n| n == "SEFAZ-PRO-2026-00013 -"));
    }

    #[test]
    fn same_name_same_content_is_written_once_and_different_content_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let id = store
            .import_capture(&capture("SEFAZ-PRO-2026/00021", "Anexos", "texto"))
            .unwrap()
            .document_id;
        for (name, body) in [("a.txt", "um"), ("A.TXT", "dois"), ("a.txt", "um")] {
            // The library may refuse an identical repeat; either way the
            // export must keep every distinct content.
            let _ = store.add_attachment(Owner::Document(id), name, "", body.as_bytes());
        }
        let out = dir.path().join("saida");
        fs::create_dir_all(&out).unwrap();
        export_open(&store, &out, &silent, &never).unwrap();
        let folder = out.join("documentos/SEFAZ-PRO-2026-00021 - Anexos/anexos");
        let files = tree(&folder);
        assert_eq!(files.len(), 2, "{files:?}");
        let mut bodies: Vec<String> = files
            .iter()
            .map(|n| fs::read_to_string(folder.join(n)).unwrap())
            .collect();
        bodies.sort();
        assert_eq!(bodies, ["dois", "um"]);
    }

    #[test]
    fn names_are_safe_and_unique() {
        assert_eq!(
            safe_file_name("Relatório: final?.PDF"),
            "Relatório final.PDF"
        );
        assert_eq!(safe_file_name("con.txt"), "_con.txt");
        assert_eq!(readable("a/b"), "a-b");
        let mut names = Names::default();
        assert_eq!(names.take("x.pdf", true), "x.pdf");
        assert_eq!(names.take("X.PDF", true), "X (2).PDF");
        assert_eq!(names.take("x.pdf", true), "x (3).pdf");
        assert_eq!(percent_decode("oficio%20n%C2%BA1.pdf"), "oficio nº1.pdf");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
