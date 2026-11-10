//! Work templates ("modelos de trabalho"): versioned models of a reply, with
//! named instruction blocks, approved examples, support references and
//! checking criteria, personal or shared with the team; and the record of
//! what each AI generation used.
//!
//! Items are addressed by a random `uid` (uuid v4), never by row number: a
//! template created on one computer keeps its identity on every other, so
//! the queue of offline changes needs no number translation for them. The
//! version check of `record_mutation` uses a key derived from the uid
//! ([`version_key`]) for the same reason.
//!
//! A published version is immutable. Drafts produced with it reference
//! `(uid, version)` and the generation record copies the texts that were
//! sent, so revising a template never changes the history of what was made
//! with earlier versions.
//!
//! Every payload is sealed with DPAPI; in clear there are only ids, uids,
//! scope, status, version numbers and a digest of the owner's name.
use crate::{
    intranet::{record_mutation, IntranetMutation},
    protection::{seal, unseal},
    storage::Store,
};
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_TEMPLATES: usize = 500;
pub const MAX_BLOCKS: usize = 500;
/// Blocks one template may reference: the trusted layer of the prompt must
/// stay small enough for a local model.
pub const MAX_BLOCKS_PER_TEMPLATE: usize = 12;
pub const MAX_EXAMPLES_PER_TEMPLATE: usize = 20;
pub const MAX_SECTIONS: usize = 30;
pub const MAX_VARIABLES: usize = 30;
pub const MAX_CRITERIA: usize = 30;
pub const MAX_REFERENCES: usize = 30;
const MAX_NAME: usize = 200;
const MAX_SHORT_TEXT: usize = 2000;
/// A block of instructions; the whole trusted layer is capped at assembly.
const MAX_LONG_TEXT: usize = 16 * 1024;
/// A section's guidance may be a whole base text (old personal templates
/// allowed 128 KiB).
const MAX_SECTION_TEXT: usize = 128 * 1024;
/// `derived_from` of templates made from the old personal templates.
const MIGRATED_FROM: &str = "personal_templates";
const MAX_EXAMPLE_TEXT: usize = 64 * 1024;
const MAX_COMMENTS: usize = 10;
/// What the person typed in "Pedido desta minuta", as recorded.
const MAX_GUIDANCE: usize = 64 * 1024;

/// Who sees a template or block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    /// Only the member who created it (hidden from others, not protected).
    #[default]
    Personal,
    /// Everyone on the team.
    Shared,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Personal => "personal",
            Scope::Shared => "shared",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "personal" => Ok(Scope::Personal),
            "shared" => Ok(Scope::Shared),
            _ => bail!("Escopo inválido."),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Scope::Personal => "Pessoal",
            Scope::Shared => "Compartilhado com a equipe",
        }
    }
}

/// Life cycle of a template or block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    /// Never published; the only version is being written.
    #[default]
    Draft,
    /// Has a published version (and maybe a newer draft).
    Published,
    /// Hidden from the pickers; history kept.
    Archived,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Published => "published",
            Status::Archived => "archived",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "draft" => Ok(Status::Draft),
            "published" => Ok(Status::Published),
            "archived" => Ok(Status::Archived),
            _ => bail!("Situação inválida."),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Draft => "Rascunho",
            Status::Published => "Publicado",
            Status::Archived => "Arquivado",
        }
    }
}

/// How a block takes part in the generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockMode {
    /// Always sent.
    Mandatory,
    /// Sent when the person ticks it.
    Optional,
    /// Sent when the person ticks it; the condition is shown and quoted.
    Conditional { condition: String },
}

impl BlockMode {
    pub fn label(&self) -> &'static str {
        match self {
            BlockMode::Mandatory => "Obrigatória",
            BlockMode::Optional => "Opcional",
            BlockMode::Conditional { .. } => "Condicional",
        }
    }
}

/// One expected part of the text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    /// What the section must contain, or the base text to start from.
    pub guidance: String,
    #[serde(default)]
    pub required: bool,
}

/// A placeholder the person fills in (`{{name}}` in the text).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub required: bool,
}

/// A block of instructions the template uses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRef {
    pub block_uid: String,
    /// Fixed when the template is published; `None` while a draft follows
    /// the block's current published version.
    #[serde(default)]
    pub version: Option<u32>,
    pub mode: BlockMode,
}

/// Support material the analysis may lean on (a norm, a precedent, a
/// manual), named and located, never fetched by the program.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub label: String,
    /// Where to find it: a number, a URL, a folder.
    pub locator: String,
    #[serde(default)]
    pub note: String,
}

/// The content of a template at one version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateVersion {
    pub version: u32,
    #[serde(default)]
    pub published: bool,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub variables: Vec<Variable>,
    #[serde(default)]
    pub block_refs: Vec<BlockRef>,
    #[serde(default)]
    pub references: Vec<Reference>,
    /// What to check before concluding.
    #[serde(default)]
    pub criteria: Vec<String>,
    #[serde(default)]
    pub change_note: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created_at: String,
}

/// The sealed part of a template row.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TemplateHead {
    pub name: String,
    #[serde(default)]
    pub purpose: String,
    pub kind: String,
    #[serde(default)]
    pub usage_criteria: String,
    /// Member name of the owner (shown as "pessoal de …").
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub derived_from: Option<String>,
}

/// A work template with every version.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkTemplate {
    pub id: i64,
    pub uid: String,
    pub name: String,
    pub purpose: String,
    /// resposta | encaminhamento | complementacao.
    pub kind: String,
    pub usage_criteria: String,
    pub scope: Scope,
    pub status: Status,
    pub owner: String,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub derived_from: Option<String>,
    /// The published version in force (0 while never published).
    pub current_version: u32,
    pub versions: Vec<TemplateVersion>,
}

impl WorkTemplate {
    /// A fresh template with one section holding `body`.
    pub fn new(name: &str, kind: &str, body: &str) -> Self {
        Self {
            name: name.into(),
            kind: kind.into(),
            versions: vec![TemplateVersion {
                version: 1,
                sections: vec![Section {
                    title: "Texto".into(),
                    guidance: body.into(),
                    required: true,
                }],
                variables: detect_variables(body),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// The version being written, when there is one.
    pub fn draft_version(&self) -> Option<&TemplateVersion> {
        self.versions.last().filter(|v| !v.published)
    }

    pub fn draft_version_mut(&mut self) -> Option<&mut TemplateVersion> {
        self.versions.last_mut().filter(|v| !v.published)
    }

    /// The published version in force.
    pub fn published_version(&self) -> Option<&TemplateVersion> {
        self.versions
            .iter()
            .find(|v| v.published && v.version == self.current_version)
    }

    pub fn version(&self, number: u32) -> Option<&TemplateVersion> {
        self.versions.iter().find(|v| v.version == number)
    }

    /// Whether `member` sees it: shared ones always, personal ones only
    /// their owner's (a convenience, not a protection).
    pub fn visible_to(&self, member: &str) -> bool {
        self.scope == Scope::Shared || owner_key(&self.owner) == owner_key(member)
    }

    /// Usable in the draft editor: published, not archived, of this kind.
    pub fn offered_for(&self, kind: &str, member: &str) -> bool {
        self.status == Status::Published
            && self.kind == kind
            && self.published_version().is_some()
            && self.visible_to(member)
    }
}

/// One version of an instruction block.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockVersion {
    pub version: u32,
    #[serde(default)]
    pub published: bool,
    pub text: String,
    #[serde(default)]
    pub change_note: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BlockHead {
    pub name: String,
    /// What it is about ("Linguagem e estilo", "Conferências finais").
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
}

/// A reusable, versioned block of instructions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InstructionBlock {
    pub id: i64,
    pub uid: String,
    pub name: String,
    pub topic: String,
    pub scope: Scope,
    pub status: Status,
    pub owner: String,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub current_version: u32,
    pub versions: Vec<BlockVersion>,
}

impl InstructionBlock {
    pub fn new(name: &str, topic: &str, text: &str) -> Self {
        Self {
            name: name.into(),
            topic: topic.into(),
            versions: vec![BlockVersion {
                version: 1,
                text: text.into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    pub fn draft_version(&self) -> Option<&BlockVersion> {
        self.versions.last().filter(|v| !v.published)
    }
    pub fn draft_version_mut(&mut self) -> Option<&mut BlockVersion> {
        self.versions.last_mut().filter(|v| !v.published)
    }
    pub fn published_version(&self) -> Option<&BlockVersion> {
        self.versions
            .iter()
            .find(|v| v.published && v.version == self.current_version)
    }
    pub fn version(&self, number: u32) -> Option<&BlockVersion> {
        self.versions.iter().find(|v| v.version == number)
    }
    pub fn visible_to(&self, member: &str) -> bool {
        self.scope == Scope::Shared || owner_key(&self.owner) == owner_key(member)
    }
}

/// Whether an example may be shown to the AI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExampleStatus {
    /// Pasted, not yet checked by anyone: visible, never sent.
    #[default]
    Unverified,
    Approved,
    Rejected,
}

impl ExampleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ExampleStatus::Unverified => "unverified",
            ExampleStatus::Approved => "approved",
            ExampleStatus::Rejected => "rejected",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "unverified" => Ok(ExampleStatus::Unverified),
            "approved" => Ok(ExampleStatus::Approved),
            "rejected" => Ok(ExampleStatus::Rejected),
            _ => bail!("Situação de exemplo inválida."),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ExampleStatus::Unverified => "Não conferido",
            ExampleStatus::Approved => "Aprovado",
            ExampleStatus::Rejected => "Rejeitado",
        }
    }
}

/// An approved piece of past work: what was asked, what was answered after
/// review, and why it was written that way. Shows form, never facts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Example {
    #[serde(default)]
    pub id: i64,
    pub uid: String,
    pub template_uid: String,
    pub title: String,
    pub demand: String,
    pub reply: String,
    #[serde(default)]
    pub explanation: String,
    /// How to use it ("aproveitar a organização; não reutilizar os fatos").
    #[serde(default)]
    pub comments: Vec<String>,
    /// Where it came from, in words ("ofício de 2024, anonimizado").
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub status: ExampleStatus,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub reviewed_by: String,
    #[serde(default)]
    pub reviewed_at: String,
}

/// What the person asked the AI to do with the editor text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    /// Write from the document and the template.
    #[default]
    New,
    /// Keep the existing text literally; fill gaps and missing sections.
    Complete,
    /// Change only what the request asks.
    Revise,
}

impl Intent {
    pub fn label(self) -> &'static str {
        match self {
            Intent::New => "Gerar nova",
            Intent::Complete => "Completar texto atual",
            Intent::Revise => "Revisar texto atual",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    #[default]
    Accepted,
    Refused {
        reason: String,
    },
}

/// A block as it was sent, with its text at that moment.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsedBlock {
    pub uid: String,
    pub name: String,
    pub version: u32,
    pub mode: String,
    pub text: String,
}

/// A source as it was sent (numbers kept so they can be translated).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsedSource {
    pub id: String,
    pub label: String,
    pub document_id: i64,
    pub capture_id: i64,
    #[serde(default)]
    pub file_id: Option<i64>,
}

/// Everything one generation used, accepted or not.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GenerationRecord {
    pub uid: String,
    #[serde(default)]
    pub intent: Intent,
    pub document_id: i64,
    pub capture_id: i64,
    /// The draft version saved from it, once saved.
    #[serde(default)]
    pub draft_id: Option<i64>,
    #[serde(default)]
    pub template_uid: Option<String>,
    #[serde(default)]
    pub template_version: Option<u32>,
    #[serde(default)]
    pub template_name: String,
    #[serde(default)]
    pub blocks: Vec<UsedBlock>,
    #[serde(default)]
    pub request_guidance: String,
    #[serde(default)]
    pub example_uids: Vec<String>,
    #[serde(default)]
    pub sources: Vec<UsedSource>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
    /// Digest and size of the trusted layer, and size of the data block.
    #[serde(default)]
    pub system_sha256: String,
    #[serde(default)]
    pub system_bytes: usize,
    #[serde(default)]
    pub data_bytes: usize,
    #[serde(default)]
    pub conflicts_shown: Vec<String>,
    pub outcome: Outcome,
}

/// The `{{name}}` placeholders of a text, as variables.
pub fn detect_variables(text: &str) -> Vec<Variable> {
    let regex = regex::Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_]*)\}\}").unwrap();
    let mut out: Vec<Variable> = Vec::new();
    for found in regex.captures_iter(text) {
        let name = &found[1];
        if !out.iter().any(|v| v.name == name) {
            out.push(Variable {
                name: name.to_owned(),
                label: name.replace('_', " "),
                required: true,
            });
        }
    }
    out
}

/// What a publication fixes: the draft version and each block's version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishPlan {
    pub version: u32,
    pub pins: Vec<(String, u32)>,
    /// Digest of the draft as the publisher saw it: a colleague's edit of
    /// the same draft number is not published in their name.
    #[serde(default)]
    pub draft_sha256: String,
    /// Bringing an archived template back, not publishing a draft.
    #[serde(default)]
    pub unarchive: bool,
}

/// What the draft says, not who saved it or when: the computer that
/// replays a save stamps its own author and time on the same content.
fn draft_digest(version: &TemplateVersion) -> String {
    let refs: Vec<(&str, &BlockMode)> = version
        .block_refs
        .iter()
        .map(|r| (r.block_uid.as_str(), &r.mode))
        .collect();
    let bytes = serde_json::to_vec(&(
        &version.sections,
        &version.variables,
        refs,
        &version.references,
        &version.criteria,
    ))
    .unwrap_or_default();
    format!("{:x}", Sha256::digest(&bytes))
}

/// Stable digest of a member name, so a personal item is found by its owner
/// on any computer without the name in clear.
pub fn owner_key(member: &str) -> Vec<u8> {
    Sha256::digest(member.trim().to_lowercase().as_bytes()).to_vec()
}

/// The `item_versions` key of a uid-addressed item: the same on every
/// library, so `expected_version` needs no translation.
pub fn version_key(uid: &str) -> i64 {
    let digest = Sha256::digest(uid.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    (i64::from_be_bytes(bytes) & i64::MAX).max(1)
}

fn new_uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn ensure_uid(uid: &str) -> Result<()> {
    ensure!(
        uid.len() == 32 && uid.bytes().all(|b| b.is_ascii_hexdigit()),
        "O código do item é inválido."
    );
    Ok(())
}

fn ensure_text(value: &str, max: usize, what: &str) -> Result<()> {
    ensure!(
        value.len() <= max
            && !value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "{what}: passou do limite de {} ou tem caracteres inválidos.",
        crate::labels::file_size(max as u64)
    );
    Ok(())
}

fn validate_version(version: &TemplateVersion) -> Result<()> {
    ensure!(
        version.sections.len() <= MAX_SECTIONS
            && version.variables.len() <= MAX_VARIABLES
            && version.block_refs.len() <= MAX_BLOCKS_PER_TEMPLATE
            && version.references.len() <= MAX_REFERENCES
            && version.criteria.len() <= MAX_CRITERIA,
        "Quantidade de seções, variáveis, instruções, referências ou critérios excedida."
    );
    ensure!(
        !version.sections.is_empty(),
        "O modelo precisa de pelo menos uma seção."
    );
    for section in &version.sections {
        ensure!(
            !section.title.trim().is_empty(),
            "Toda seção precisa de título."
        );
        ensure_text(&section.title, MAX_NAME, "O título da seção")?;
        ensure_text(&section.guidance, MAX_SECTION_TEXT, "O texto da seção")?;
    }
    for variable in &version.variables {
        ensure!(
            !variable.name.trim().is_empty()
                && variable
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "Nome de variável inválido (use letras, números e _)."
        );
        ensure_text(&variable.label, MAX_NAME, "O rótulo da variável")?;
    }
    let mut seen = std::collections::BTreeSet::new();
    for reference in &version.block_refs {
        ensure_uid(&reference.block_uid)?;
        ensure!(
            seen.insert(reference.block_uid.clone()),
            "A mesma instrução aparece duas vezes."
        );
        if let BlockMode::Conditional { condition } = &reference.mode {
            ensure!(
                !condition.trim().is_empty(),
                "Descreva a condição da instrução condicional."
            );
            ensure_text(condition, MAX_SHORT_TEXT, "A condição")?;
        }
    }
    for reference in &version.references {
        ensure!(
            !reference.label.trim().is_empty(),
            "Toda referência precisa de nome."
        );
        ensure_text(&reference.label, MAX_NAME, "O nome da referência")?;
        ensure_text(
            &reference.locator,
            MAX_SHORT_TEXT,
            "A localização da referência",
        )?;
        ensure_text(&reference.note, MAX_SHORT_TEXT, "A nota da referência")?;
    }
    for criterion in &version.criteria {
        ensure_text(criterion, MAX_SHORT_TEXT, "O critério")?;
    }
    ensure_text(&version.change_note, MAX_SHORT_TEXT, "A nota da versão")?;
    Ok(())
}

pub(crate) fn validate_template(template: &WorkTemplate) -> Result<()> {
    ensure!(!template.name.trim().is_empty(), "Dê um nome ao modelo.");
    ensure_text(&template.name, MAX_NAME, "O nome")?;
    ensure_text(&template.purpose, MAX_SHORT_TEXT, "A finalidade")?;
    ensure_text(
        &template.usage_criteria,
        MAX_SHORT_TEXT * 2,
        "Os critérios de uso",
    )?;
    ensure!(
        crate::model::valid_kind(&template.kind),
        "Tipo de minuta inválido."
    );
    let version = template
        .versions
        .last()
        .context("O modelo precisa de uma versão.")?;
    validate_version(version)
}

pub(crate) fn validate_block(block: &InstructionBlock) -> Result<()> {
    ensure!(!block.name.trim().is_empty(), "Dê um nome à instrução.");
    ensure_text(&block.name, MAX_NAME, "O nome")?;
    ensure_text(&block.topic, MAX_NAME, "O tema")?;
    let version = block
        .versions
        .last()
        .context("A instrução precisa de uma versão.")?;
    ensure!(
        !version.text.trim().is_empty(),
        "Escreva o texto da instrução."
    );
    ensure_text(&version.text, MAX_LONG_TEXT, "O texto da instrução")?;
    ensure_text(&version.change_note, MAX_SHORT_TEXT, "A nota da versão")?;
    Ok(())
}

pub(crate) fn validate_example(example: &Example) -> Result<()> {
    ensure_uid(&example.template_uid)?;
    ensure!(!example.title.trim().is_empty(), "Dê um título ao exemplo.");
    ensure_text(&example.title, MAX_NAME, "O título")?;
    ensure!(
        !example.reply.trim().is_empty(),
        "O exemplo precisa da resposta revisada."
    );
    ensure_text(&example.demand, MAX_EXAMPLE_TEXT, "A demanda")?;
    ensure_text(&example.reply, MAX_EXAMPLE_TEXT, "A resposta")?;
    ensure_text(&example.explanation, MAX_EXAMPLE_TEXT, "A explicação")?;
    ensure_text(&example.origin, MAX_SHORT_TEXT, "A origem")?;
    ensure!(
        example.comments.len() <= MAX_COMMENTS,
        "Até {MAX_COMMENTS} comentários por exemplo."
    );
    for comment in &example.comments {
        ensure_text(comment, MAX_SHORT_TEXT, "O comentário")?;
    }
    Ok(())
}

/// The provenance a saved draft carries, checked like any team input.
pub(crate) fn validate_provenance(provenance: &crate::model::DraftProvenance) -> Result<()> {
    if let Some(uid) = &provenance.generation_uid {
        ensure_uid(uid)?;
    }
    if let Some(uid) = &provenance.template_uid {
        ensure_uid(uid)?;
    }
    ensure_text(&provenance.template_name, MAX_NAME, "O nome do modelo")?;
    ensure_text(&provenance.model, MAX_NAME, "O modelo de IA")?;
    ensure_text(&provenance.provider, MAX_NAME, "O serviço de IA")?;
    ensure!(
        provenance.request_guidance.len() <= MAX_GUIDANCE,
        "O pedido desta minuta é longo demais."
    );
    Ok(())
}

pub(crate) fn validate_record(record: &GenerationRecord) -> Result<()> {
    ensure_uid(&record.uid)?;
    ensure!(
        record.document_id > 0 && record.capture_id > 0,
        "Os dados da geração do texto não indicam o documento."
    );
    if let Some(uid) = &record.template_uid {
        ensure_uid(uid)?;
    }
    ensure!(
        record.blocks.len() <= MAX_BLOCKS_PER_TEMPLATE
            && record.example_uids.len() <= MAX_EXAMPLES_PER_TEMPLATE
            && record.sources.len() <= 512
            && record.conflicts_shown.len() <= 64,
        "Os dados da geração do texto são grandes demais."
    );
    // Names and identifiers are short and clean; long texts are only
    // bounded (they come from the person and the model, pasted text may
    // carry anything).
    ensure_text(&record.template_name, MAX_NAME, "O nome do modelo")?;
    ensure_text(&record.model, MAX_NAME, "O modelo de IA")?;
    ensure_text(&record.provider, MAX_NAME, "O serviço de IA")?;
    ensure_text(&record.system_sha256, 64, "O resumo do texto enviado")?;
    ensure!(
        record.request_guidance.len() <= MAX_GUIDANCE,
        "O pedido desta minuta é longo demais."
    );
    for block in &record.blocks {
        ensure_uid(&block.uid)?;
        ensure_text(&block.name, MAX_NAME, "O nome da instrução")?;
        ensure!(block.mode.len() <= 4096, "Os dados da geração do texto são inválidos.");
    }
    for uid in &record.example_uids {
        ensure_uid(uid)?;
    }
    for source in &record.sources {
        ensure_text(&source.id, MAX_NAME, "O identificador da fonte")?;
        ensure!(
            source.label.len() <= MAX_SHORT_TEXT,
            "Os dados da geração do texto são inválidos."
        );
    }
    for conflict in &record.conflicts_shown {
        ensure!(
            conflict.len() <= MAX_SHORT_TEXT * 4,
            "Os dados da geração do texto são inválidos."
        );
    }
    if let Outcome::Refused { reason } = &record.outcome {
        ensure!(reason.len() <= 8 * 1024, "Os dados da geração do texto são inválidos.");
    }
    let bytes: usize = record.blocks.iter().map(|b| b.text.len()).sum::<usize>()
        + record.request_guidance.len()
        + record
            .conflicts_shown
            .iter()
            .map(|c| c.len())
            .sum::<usize>();
    ensure!(bytes <= 512 * 1024, "Os dados da geração do texto são grandes demais.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Snapshot rows (team sync): one struct per table, ids as stored.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateRow {
    pub id: i64,
    pub uid: String,
    pub scope: String,
    pub status: String,
    pub owner_hash: Option<Vec<u8>>,
    pub current_version: i64,
    pub head: TemplateHead,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateVersionRow {
    pub id: i64,
    pub template_id: i64,
    pub version: i64,
    pub published: bool,
    pub value: TemplateVersion,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockRow {
    pub id: i64,
    pub uid: String,
    pub scope: String,
    pub status: String,
    pub owner_hash: Option<Vec<u8>>,
    pub current_version: i64,
    pub head: BlockHead,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockVersionRow {
    pub id: i64,
    pub block_id: i64,
    pub version: i64,
    pub published: bool,
    pub value: BlockVersion,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExampleRow {
    pub id: i64,
    pub uid: String,
    pub template_id: i64,
    pub status: String,
    pub value: Example,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerationRow {
    pub id: i64,
    pub uid: String,
    pub document_id: i64,
    pub capture_id: i64,
    pub draft_id: Option<i64>,
    pub template_uid: Option<String>,
    pub record: GenerationRecord,
    pub created_at: String,
}

/// Every templates table, for the team snapshot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TemplatesSnapshot {
    #[serde(default)]
    pub templates: Vec<TemplateRow>,
    #[serde(default)]
    pub template_versions: Vec<TemplateVersionRow>,
    #[serde(default)]
    pub blocks: Vec<BlockRow>,
    #[serde(default)]
    pub block_versions: Vec<BlockVersionRow>,
    #[serde(default)]
    pub examples: Vec<ExampleRow>,
    #[serde(default)]
    pub generations: Vec<GenerationRow>,
    /// Texts of the files attached to examples (see `extraction`).
    #[serde(default)]
    pub attachment_texts: Vec<crate::extraction::AttachmentTextRow>,
}

fn decode<T: for<'a> Deserialize<'a>>(blob: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(&unseal(blob)?)?)
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    seal(&serde_json::to_vec(value)?)
}

impl Store {
    pub(crate) fn ensure_work_templates(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS work_templates(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uid TEXT NOT NULL UNIQUE,
                scope TEXT NOT NULL CHECK(scope IN ('personal','shared')),
                status TEXT NOT NULL CHECK(status IN ('draft','published','archived')),
                owner_hash BLOB,
                current_version INTEGER NOT NULL DEFAULT 0,
                payload BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS work_template_versions(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                template_id INTEGER NOT NULL REFERENCES work_templates(id) ON DELETE CASCADE,
                version INTEGER NOT NULL,
                published INTEGER NOT NULL DEFAULT 0,
                payload BLOB NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(template_id, version)
            );
            CREATE TABLE IF NOT EXISTS instruction_blocks(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uid TEXT NOT NULL UNIQUE,
                scope TEXT NOT NULL CHECK(scope IN ('personal','shared')),
                status TEXT NOT NULL CHECK(status IN ('draft','published','archived')),
                owner_hash BLOB,
                current_version INTEGER NOT NULL DEFAULT 0,
                payload BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS instruction_block_versions(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                block_id INTEGER NOT NULL REFERENCES instruction_blocks(id) ON DELETE CASCADE,
                version INTEGER NOT NULL,
                published INTEGER NOT NULL DEFAULT 0,
                payload BLOB NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(block_id, version)
            );
            CREATE TABLE IF NOT EXISTS template_examples(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uid TEXT NOT NULL UNIQUE,
                template_id INTEGER NOT NULL REFERENCES work_templates(id) ON DELETE CASCADE,
                status TEXT NOT NULL CHECK(status IN ('unverified','approved','rejected')),
                payload BLOB NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS draft_generations(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uid TEXT NOT NULL UNIQUE,
                document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                capture_id INTEGER NOT NULL,
                draft_id INTEGER,
                template_uid TEXT,
                payload BLOB NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS draft_generations_document ON draft_generations(document_id);
            CREATE INDEX IF NOT EXISTS draft_generations_template ON draft_generations(template_uid);
            CREATE TABLE IF NOT EXISTS work_template_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        crate::extraction::ensure_schema(&self.conn)?;
        Ok(())
    }

    // -- reads --------------------------------------------------------------

    /// Every template with its versions, memoized until the library changes.
    pub fn list_templates(&self) -> Result<Vec<WorkTemplate>> {
        self.cached("work_templates", || self.list_templates_uncached())
    }

    fn list_templates_uncached(&self) -> Result<Vec<WorkTemplate>> {
        let mut statement = self.conn.prepare(
            "SELECT id,uid,scope,status,current_version,payload,updated_at FROM work_templates ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Vec<u8>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut versions: BTreeMap<i64, Vec<TemplateVersion>> = BTreeMap::new();
        let mut statement = self.conn.prepare(
            "SELECT template_id,version,published,payload FROM work_template_versions ORDER BY template_id,version",
        )?;
        let version_rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (template_id, version, published, blob) in version_rows {
            let mut value: TemplateVersion = decode(&blob)?;
            value.version = version as u32;
            value.published = published != 0;
            versions.entry(template_id).or_default().push(value);
        }
        let mut out = Vec::with_capacity(rows.len());
        for (id, uid, scope, status, current_version, blob, updated_at) in rows {
            let head: TemplateHead = decode(&blob)?;
            out.push(WorkTemplate {
                id,
                uid,
                name: head.name,
                purpose: head.purpose,
                kind: head.kind,
                usage_criteria: head.usage_criteria,
                scope: Scope::parse(&scope)?,
                status: Status::parse(&status)?,
                owner: head.owner,
                created_by: head.created_by,
                created_at: head.created_at,
                updated_at,
                derived_from: head.derived_from,
                current_version: current_version as u32,
                versions: versions.remove(&id).unwrap_or_default(),
            });
        }
        Ok(out)
    }

    pub fn template_by_uid(&self, uid: &str) -> Result<Option<WorkTemplate>> {
        Ok(self.list_templates()?.into_iter().find(|t| t.uid == uid))
    }

    /// Every instruction block with its versions, memoized.
    pub fn list_blocks(&self) -> Result<Vec<InstructionBlock>> {
        self.cached("instruction_blocks", || self.list_blocks_uncached())
    }

    fn list_blocks_uncached(&self) -> Result<Vec<InstructionBlock>> {
        let mut statement = self.conn.prepare(
            "SELECT id,uid,scope,status,current_version,payload,updated_at FROM instruction_blocks ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Vec<u8>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut versions: BTreeMap<i64, Vec<BlockVersion>> = BTreeMap::new();
        let mut statement = self.conn.prepare(
            "SELECT block_id,version,published,payload FROM instruction_block_versions ORDER BY block_id,version",
        )?;
        let version_rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (block_id, version, published, blob) in version_rows {
            let mut value: BlockVersion = decode(&blob)?;
            value.version = version as u32;
            value.published = published != 0;
            versions.entry(block_id).or_default().push(value);
        }
        let mut out = Vec::with_capacity(rows.len());
        for (id, uid, scope, status, current_version, blob, updated_at) in rows {
            let head: BlockHead = decode(&blob)?;
            out.push(InstructionBlock {
                id,
                uid,
                name: head.name,
                topic: head.topic,
                scope: Scope::parse(&scope)?,
                status: Status::parse(&status)?,
                owner: head.owner,
                created_by: head.created_by,
                created_at: head.created_at,
                updated_at,
                current_version: current_version as u32,
                versions: versions.remove(&id).unwrap_or_default(),
            });
        }
        Ok(out)
    }

    pub fn block_by_uid(&self, uid: &str) -> Result<Option<InstructionBlock>> {
        Ok(self.list_blocks()?.into_iter().find(|b| b.uid == uid))
    }

    /// Every example of every template, memoized.
    pub fn list_examples(&self) -> Result<Vec<Example>> {
        self.cached("template_examples", || {
            let mut statement = self.conn.prepare(
                "SELECT e.id,e.uid,t.uid,e.status,e.payload FROM template_examples e
                 JOIN work_templates t ON t.id=e.template_id ORDER BY e.id",
            )?;
            let rows = statement
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut out = Vec::with_capacity(rows.len());
            for (id, uid, template_uid, status, blob) in rows {
                let mut example: Example = decode(&blob)?;
                example.id = id;
                example.uid = uid;
                example.template_uid = template_uid;
                example.status = ExampleStatus::parse(&status)?;
                out.push(example);
            }
            Ok(out)
        })
    }

    pub fn examples_of(&self, template_uid: &str) -> Result<Vec<Example>> {
        Ok(self
            .list_examples()?
            .into_iter()
            .filter(|e| e.template_uid == template_uid)
            .collect())
    }

    /// Generation records of a document, newest first, memoized.
    pub fn generations_for_document(&self, document_id: i64) -> Result<Vec<GenerationRecord>> {
        self.cached(&format!("generations\u{1}{document_id}"), || {
            let mut statement = self.conn.prepare(
                "SELECT draft_id,payload FROM draft_generations WHERE document_id=?1 ORDER BY id DESC",
            )?;
            let rows = statement
                .query_map([document_id], |r| {
                    Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut out = Vec::with_capacity(rows.len());
            for (draft_id, blob) in rows {
                let mut record: GenerationRecord = decode(&blob)?;
                record.draft_id = draft_id;
                out.push(record);
            }
            Ok(out)
        })
    }

    pub fn generation_by_uid(
        &self,
        document_id: i64,
        uid: &str,
    ) -> Result<Option<GenerationRecord>> {
        Ok(self
            .generations_for_document(document_id)?
            .into_iter()
            .find(|g| g.uid == uid))
    }

    /// Whether any generation used this template (then it cannot be deleted).
    pub fn template_in_use(&self, uid: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT count(*) FROM draft_generations WHERE template_uid=?1",
            [uid],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    // -- templates ----------------------------------------------------------

    /// Creates a template or saves changes to one. A change to a template
    /// whose latest version is published starts a new draft version
    /// (revising); a change to a draft version edits it in place. On a
    /// client computer the change goes to the main one; `expected_version`
    /// detects a colleague's concurrent edit.
    pub fn save_template(
        &mut self,
        template: &WorkTemplate,
        expected_version: Option<i64>,
    ) -> Result<WorkTemplate> {
        let mut template = template.clone();
        if template.uid.is_empty() {
            template.uid = new_uid();
        }
        ensure_uid(&template.uid)?;
        validate_template(&template)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveTemplate {
                    template: template.clone(),
                    expected_version,
                },
            )?;
            return self
                .template_by_uid(&template.uid)?
                .context("O modelo não voltou do computador principal.");
        }
        let actor = self.active_actor();
        let uid = template.uid.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(i64, String, String, i64)> = tx
            .query_row(
                "SELECT id,status,payload,current_version FROM work_templates WHERE uid=?1",
                [&uid],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get::<_, Vec<u8>>(2).map(|_| String::new())?,
                        r.get(3)?,
                    ))
                },
            )
            .optional()?;
        let incoming = template.versions.last().cloned().unwrap_or_default();
        let timestamp = now();
        match existing {
            None => {
                let count: i64 =
                    tx.query_row("SELECT count(*) FROM work_templates", [], |r| r.get(0))?;
                ensure!(
                    (count as usize) < MAX_TEMPLATES,
                    "Limite de {MAX_TEMPLATES} modelos."
                );
                let owner = if template.owner.trim().is_empty() {
                    actor.clone()
                } else {
                    template.owner.clone()
                };
                let head = TemplateHead {
                    name: template.name.trim().to_owned(),
                    purpose: template.purpose.clone(),
                    kind: template.kind.clone(),
                    usage_criteria: template.usage_criteria.clone(),
                    owner: owner.clone(),
                    created_by: actor.clone(),
                    created_at: timestamp.clone(),
                    derived_from: template.derived_from.clone(),
                };
                tx.execute(
                    "INSERT INTO work_templates(uid,scope,status,owner_hash,current_version,payload,updated_at)
                     VALUES(?1,?2,'draft',?3,0,?4,?5)",
                    params![
                        uid,
                        template.scope.as_str(),
                        owner_hash_for(template.scope, &owner),
                        encode(&head)?,
                        timestamp
                    ],
                )?;
                let template_id = tx.last_insert_rowid();
                let value = TemplateVersion {
                    version: 1,
                    published: false,
                    author: actor.clone(),
                    created_at: timestamp.clone(),
                    ..incoming
                };
                tx.execute(
                    "INSERT INTO work_template_versions(template_id,version,published,payload,created_at)
                     VALUES(?1,1,0,?2,?3)",
                    params![template_id, encode(&value)?, timestamp],
                )?;
                record_mutation(
                    &tx,
                    &actor,
                    "template",
                    version_key(&uid),
                    expected_version,
                    "create_template",
                    None,
                    &format!("Criou o modelo de trabalho \"{}\"", head.name),
                )?;
            }
            Some((template_id, status, _, _)) => {
                ensure!(
                    Status::parse(&status)? != Status::Archived,
                    "Desarquive o modelo antes de editá-lo."
                );
                let blob: Vec<u8> = tx.query_row(
                    "SELECT payload FROM work_templates WHERE id=?1",
                    [template_id],
                    |r| r.get(0),
                )?;
                let mut head: TemplateHead = decode(&blob)?;
                head.name = template.name.trim().to_owned();
                head.purpose = template.purpose.clone();
                head.kind = template.kind.clone();
                head.usage_criteria = template.usage_criteria.clone();
                let owner = head.owner.clone();
                tx.execute(
                    "UPDATE work_templates SET scope=?1,owner_hash=?2,payload=?3,updated_at=?4 WHERE id=?5",
                    params![
                        template.scope.as_str(),
                        owner_hash_for(template.scope, &owner),
                        encode(&head)?,
                        timestamp,
                        template_id
                    ],
                )?;
                let last: (i64, i64, Vec<u8>) = tx.query_row(
                    "SELECT version,published,payload FROM work_template_versions WHERE template_id=?1 ORDER BY version DESC LIMIT 1",
                    [template_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                let (number, replacing) = if last.1 == 0 {
                    (last.0, true)
                } else {
                    (last.0 + 1, false)
                };
                let mut value = TemplateVersion {
                    version: number as u32,
                    published: false,
                    author: actor.clone(),
                    created_at: timestamp.clone(),
                    ..incoming
                };
                if replacing {
                    tx.execute(
                        "UPDATE work_template_versions SET payload=?1 WHERE template_id=?2 AND version=?3",
                        params![encode(&value)?, template_id, number],
                    )?;
                } else {
                    // A new draft follows the blocks' current published
                    // versions (publishing pins them again), and a copy
                    // identical to the published version is not a change.
                    for reference in &mut value.block_refs {
                        reference.version = None;
                    }
                    let published: TemplateVersion = decode(&last.2)?;
                    // Compared as it would be published: floating
                    // references at the blocks' current versions, so
                    // adopting a newer instruction counts as a change.
                    let mut following = value.clone();
                    for reference in &mut following.block_refs {
                        reference.version = tx
                            .query_row(
                                "SELECT current_version FROM instruction_blocks WHERE uid=?1",
                                [&reference.block_uid],
                                |r| r.get::<_, i64>(0),
                            )
                            .optional()?
                            .map(|v| v as u32);
                    }
                    if !same_template_content(&published, &following) {
                        tx.execute(
                            "INSERT INTO work_template_versions(template_id,version,published,payload,created_at)
                             VALUES(?1,?2,0,?3,?4)",
                            params![template_id, number, encode(&value)?, timestamp],
                        )?;
                    }
                }
                record_mutation(
                    &tx,
                    &actor,
                    "template",
                    version_key(&uid),
                    expected_version,
                    "save_template",
                    None,
                    &format!(
                        "Alterou o modelo de trabalho \"{}\" (rascunho da versão {number})",
                        head.name
                    ),
                )?;
            }
        }
        tx.commit()?;
        self.template_by_uid(&uid)?
            .context("Modelo não encontrado depois de salvo.")
    }

    /// Publishes the draft version (freezing it and the versions of the
    /// blocks it uses), archives, or brings an archived template back.
    pub fn set_template_status(
        &mut self,
        uid: &str,
        status: Status,
        expected_version: Option<i64>,
    ) -> Result<WorkTemplate> {
        ensure_uid(uid)?;
        let plan = if status == Status::Published {
            self.publish_plan(uid)?
        } else {
            None
        };
        self.set_template_status_with(uid, status, plan, expected_version)
    }

    /// What publishing means as this computer sees it: the draft number and
    /// the block version each reference gets. Sent with the change, so the
    /// main computer (or a later replay) publishes exactly that, not the
    /// blocks' versions of the moment. For an archived template the plan
    /// says "bring it back".
    fn publish_plan(&self, uid: &str) -> Result<Option<PublishPlan>> {
        let template = self
            .template_by_uid(uid)?
            .context("Modelo não encontrado.")?;
        if template.status == Status::Archived {
            return Ok(Some(PublishPlan {
                unarchive: true,
                ..Default::default()
            }));
        }
        let draft = template
            .draft_version()
            .context("Não há alterações a publicar neste modelo.")?;
        let blocks = self.list_blocks()?;
        let mut pins = Vec::new();
        for reference in &draft.block_refs {
            let version = match reference.version {
                Some(version) => version,
                None => blocks
                    .iter()
                    .find(|b| b.uid == reference.block_uid)
                    .context(
                        "Uma instrução deste modelo não existe mais; retire-a antes de publicar.",
                    )?
                    .current_version,
            };
            pins.push((reference.block_uid.clone(), version));
        }
        Ok(Some(PublishPlan {
            version: draft.version,
            pins,
            draft_sha256: draft_digest(draft),
            unarchive: false,
        }))
    }

    pub(crate) fn set_template_status_with(
        &mut self,
        uid: &str,
        status: Status,
        plan: Option<PublishPlan>,
        expected_version: Option<i64>,
    ) -> Result<WorkTemplate> {
        ensure_uid(uid)?;
        ensure!(status != Status::Draft, "Situação inválida.");
        if let Some(plan) = &plan {
            ensure!(
                plan.pins.len() <= MAX_BLOCKS_PER_TEMPLATE,
                "Publicação inválida."
            );
            for (block_uid, _) in &plan.pins {
                ensure_uid(block_uid)?;
            }
        }
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::SetTemplateStatus {
                    uid: uid.to_owned(),
                    status,
                    plan,
                    expected_version,
                },
            )?;
            return self
                .template_by_uid(uid)?
                .context("O modelo não voltou do computador principal.");
        }
        let blocks = self.list_blocks()?;
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (template_id, current_status, blob): (i64, String, Vec<u8>) = tx
            .query_row(
                "SELECT id,status,payload FROM work_templates WHERE uid=?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .context("Modelo não encontrado.")?;
        let head: TemplateHead = decode(&blob)?;
        let current_status = Status::parse(&current_status)?;
        let details = match status {
            Status::Published if current_status == Status::Archived => {
                // Asked to publish a draft, found archived (a colleague
                // archived it meanwhile): never turned into an unarchive.
                ensure!(
                    plan.as_ref().is_none_or(|p| p.unarchive),
                    "O modelo foi arquivado enquanto isso; desarquive-o antes de publicar."
                );
                // Unarchiving only brings it back: published if it ever was,
                // otherwise a draft again; a pending draft stays a draft.
                let current: i64 = tx.query_row(
                    "SELECT current_version FROM work_templates WHERE id=?1",
                    [template_id],
                    |r| r.get(0),
                )?;
                let restored = if current > 0 { "published" } else { "draft" };
                tx.execute(
                    "UPDATE work_templates SET status=?1,updated_at=?2 WHERE id=?3",
                    params![restored, now(), template_id],
                )?;
                format!("Desarquivou o modelo \"{}\"", head.name)
            }
            Status::Published => {
                // Asked to bring back, found active (a colleague did it):
                // a pending draft is never published by that request.
                ensure!(
                    !plan.as_ref().is_some_and(|p| p.unarchive),
                    "O modelo já foi desarquivado enquanto isso."
                );
                let draft: Option<(i64, Vec<u8>)> = tx
                    .query_row(
                        "SELECT version,payload FROM work_template_versions WHERE template_id=?1 AND published=0 ORDER BY version DESC LIMIT 1",
                        [template_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                match draft {
                    Some((number, blob)) => {
                        if let Some(plan) = &plan {
                            ensure!(
                                number == i64::from(plan.version),
                                "O rascunho do modelo mudou desde que foi aberto; confira e publique de novo."
                            );
                        }
                        let mut value: TemplateVersion = decode(&blob)?;
                        if let Some(plan) = &plan {
                            ensure!(
                                plan.draft_sha256.is_empty()
                                    || plan.draft_sha256 == draft_digest(&value),
                                "O rascunho do modelo mudou desde que foi aberto; confira e publique de novo."
                            );
                        }
                        for reference in &mut value.block_refs {
                            let block = blocks
                                .iter()
                                .find(|b| b.uid == reference.block_uid)
                                .with_context(|| {
                                    format!("A instrução {} não existe mais.", reference.block_uid)
                                })?;
                            ensure!(
                                block.status != Status::Archived,
                                "A instrução \"{}\" está arquivada; retire-a do modelo ou desarquive-a.",
                                block.name
                            );
                            let pinned = match reference.version {
                                Some(version) => {
                                    ensure!(
                                        block.version(version).is_some_and(|v| v.published),
                                        "A versão {version} da instrução \"{}\" não está publicada.",
                                        block.name
                                    );
                                    version
                                }
                                None => {
                                    ensure!(
                                        block.published_version().is_some(),
                                        "Publique a instrução \"{}\" antes de publicar o modelo.",
                                        block.name
                                    );
                                    match &plan {
                                        // The version the publisher saw.
                                        Some(plan) => {
                                            let version = plan
                                                .pins
                                                .iter()
                                                .find(|(b, _)| *b == reference.block_uid)
                                                .map(|(_, v)| *v)
                                                .context(
                                                    "Publicação incompleta; publique de novo.",
                                                )?;
                                            ensure!(
                                                block.version(version).is_some_and(|v| v.published),
                                                "A versão {version} da instrução \"{}\" não está publicada.",
                                                block.name
                                            );
                                            version
                                        }
                                        None => block.current_version,
                                    }
                                }
                            };
                            reference.version = Some(pinned);
                        }
                        value.published = true;
                        tx.execute(
                            "UPDATE work_template_versions SET published=1,payload=?1 WHERE template_id=?2 AND version=?3",
                            params![encode(&value)?, template_id, number],
                        )?;
                        tx.execute(
                            "UPDATE work_templates SET status='published',current_version=?1,updated_at=?2 WHERE id=?3",
                            params![number, now(), template_id],
                        )?;
                        format!("Publicou a versão {number} do modelo \"{}\"", head.name)
                    }
                    None => bail!("Não há alterações a publicar neste modelo."),
                }
            }
            Status::Archived => {
                ensure!(
                    current_status != Status::Archived,
                    "O modelo já está arquivado."
                );
                tx.execute(
                    "UPDATE work_templates SET status='archived',updated_at=?1 WHERE id=?2",
                    params![now(), template_id],
                )?;
                format!("Arquivou o modelo \"{}\"", head.name)
            }
            Status::Draft => unreachable!(),
        };
        record_mutation(
            &tx,
            &actor,
            "template",
            version_key(uid),
            expected_version,
            "set_template_status",
            None,
            &details,
        )?;
        tx.commit()?;
        self.template_by_uid(uid)?
            .context("Modelo não encontrado depois da alteração.")
    }

    /// Removes a template for good, with its versions and examples. Refused
    /// while a generation record references it: archive it instead.
    pub fn delete_template(&mut self, uid: &str) -> Result<()> {
        ensure_uid(uid)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::DeleteTemplate {
                    uid: uid.to_owned(),
                },
            )?;
            return Ok(());
        }
        ensure!(
            !self.template_in_use(uid)?,
            "Este modelo foi usado em minutas. Arquive-o em vez de apagar, para o histórico continuar legível."
        );
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (template_id, blob): (i64, Vec<u8>) = tx
            .query_row(
                "SELECT id,payload FROM work_templates WHERE uid=?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .context("Modelo não encontrado.")?;
        let head: TemplateHead = decode(&blob)?;
        // Files of its examples hang from no document: removed here.
        tx.execute(
            "DELETE FROM attachments WHERE owner_kind='example'
             AND owner_id IN (SELECT id FROM template_examples WHERE template_id=?1)",
            [template_id],
        )?;
        tx.execute("DELETE FROM work_templates WHERE id=?1", [template_id])?;
        record_mutation(
            &tx,
            &actor,
            "template",
            version_key(uid),
            None,
            "delete_template",
            None,
            &format!("Excluiu o modelo \"{}\"", head.name),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A copy of a template as a new personal draft (versions start again).
    pub fn duplicate_template(&mut self, uid: &str, name: &str) -> Result<WorkTemplate> {
        let source = self
            .template_by_uid(uid)?
            .context("Modelo não encontrado.")?;
        let mut base = source
            .published_version()
            .or(source.versions.last())
            .cloned()
            .context("O modelo não tem versão.")?;
        for reference in &mut base.block_refs {
            reference.version = None;
        }
        let copy = WorkTemplate {
            id: 0,
            uid: String::new(),
            name: name.into(),
            scope: Scope::Personal,
            status: Status::Draft,
            owner: String::new(),
            derived_from: Some(source.uid.clone()),
            current_version: 0,
            versions: vec![TemplateVersion {
                version: 1,
                published: false,
                change_note: format!("Cópia de \"{}\" (versão {})", source.name, base.version),
                ..base
            }],
            ..source
        };
        self.save_template(&copy, None)
    }

    // -- blocks -------------------------------------------------------------

    /// Creates an instruction block or saves changes to one (same rules as
    /// [`Store::save_template`]).
    pub fn save_block(
        &mut self,
        block: &InstructionBlock,
        expected_version: Option<i64>,
    ) -> Result<InstructionBlock> {
        let mut block = block.clone();
        if block.uid.is_empty() {
            block.uid = new_uid();
        }
        ensure_uid(&block.uid)?;
        validate_block(&block)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveInstructionBlock {
                    block: block.clone(),
                    expected_version,
                },
            )?;
            return self
                .block_by_uid(&block.uid)?
                .context("A instrução não voltou do computador principal.");
        }
        let actor = self.active_actor();
        let uid = block.uid.clone();
        let incoming = block.versions.last().cloned().unwrap_or_default();
        let timestamp = now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(i64, String, Vec<u8>)> = tx
            .query_row(
                "SELECT id,status,payload FROM instruction_blocks WHERE uid=?1",
                [&uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match existing {
            None => {
                let count: i64 =
                    tx.query_row("SELECT count(*) FROM instruction_blocks", [], |r| r.get(0))?;
                ensure!(
                    (count as usize) < MAX_BLOCKS,
                    "Limite de {MAX_BLOCKS} instruções."
                );
                let owner = if block.owner.trim().is_empty() {
                    actor.clone()
                } else {
                    block.owner.clone()
                };
                let head = BlockHead {
                    name: block.name.trim().to_owned(),
                    topic: block.topic.clone(),
                    owner: owner.clone(),
                    created_by: actor.clone(),
                    created_at: timestamp.clone(),
                };
                tx.execute(
                    "INSERT INTO instruction_blocks(uid,scope,status,owner_hash,current_version,payload,updated_at)
                     VALUES(?1,?2,'draft',?3,0,?4,?5)",
                    params![
                        uid,
                        block.scope.as_str(),
                        owner_hash_for(block.scope, &owner),
                        encode(&head)?,
                        timestamp
                    ],
                )?;
                let block_id = tx.last_insert_rowid();
                let value = BlockVersion {
                    version: 1,
                    published: false,
                    author: actor.clone(),
                    created_at: timestamp.clone(),
                    ..incoming
                };
                tx.execute(
                    "INSERT INTO instruction_block_versions(block_id,version,published,payload,created_at)
                     VALUES(?1,1,0,?2,?3)",
                    params![block_id, encode(&value)?, timestamp],
                )?;
                record_mutation(
                    &tx,
                    &actor,
                    "block",
                    version_key(&uid),
                    expected_version,
                    "create_block",
                    None,
                    &format!("Criou a instrução \"{}\"", head.name),
                )?;
            }
            Some((block_id, status, blob)) => {
                ensure!(
                    Status::parse(&status)? != Status::Archived,
                    "Desarquive a instrução antes de editá-la."
                );
                let mut head: BlockHead = decode(&blob)?;
                head.name = block.name.trim().to_owned();
                head.topic = block.topic.clone();
                let owner = head.owner.clone();
                tx.execute(
                    "UPDATE instruction_blocks SET scope=?1,owner_hash=?2,payload=?3,updated_at=?4 WHERE id=?5",
                    params![
                        block.scope.as_str(),
                        owner_hash_for(block.scope, &owner),
                        encode(&head)?,
                        timestamp,
                        block_id
                    ],
                )?;
                let last: (i64, i64, Vec<u8>) = tx.query_row(
                    "SELECT version,published,payload FROM instruction_block_versions WHERE block_id=?1 ORDER BY version DESC LIMIT 1",
                    [block_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                let (number, replacing) = if last.1 == 0 {
                    (last.0, true)
                } else {
                    (last.0 + 1, false)
                };
                let value = BlockVersion {
                    version: number as u32,
                    published: false,
                    author: actor.clone(),
                    created_at: timestamp.clone(),
                    ..incoming
                };
                if replacing {
                    tx.execute(
                        "UPDATE instruction_block_versions SET payload=?1 WHERE block_id=?2 AND version=?3",
                        params![encode(&value)?, block_id, number],
                    )?;
                } else {
                    let published: BlockVersion = decode(&last.2)?;
                    if published.text.trim() != value.text.trim() {
                        tx.execute(
                            "INSERT INTO instruction_block_versions(block_id,version,published,payload,created_at)
                             VALUES(?1,?2,0,?3,?4)",
                            params![block_id, number, encode(&value)?, timestamp],
                        )?;
                    }
                }
                record_mutation(
                    &tx,
                    &actor,
                    "block",
                    version_key(&uid),
                    expected_version,
                    "save_block",
                    None,
                    &format!(
                        "Alterou a instrução \"{}\" (rascunho da versão {number})",
                        head.name
                    ),
                )?;
            }
        }
        tx.commit()?;
        self.block_by_uid(&uid)?
            .context("Instrução não encontrada depois de salva.")
    }

    /// Publishes the draft version of a block, archives it, or brings it back.
    pub fn set_block_status(
        &mut self,
        uid: &str,
        status: Status,
        expected_version: Option<i64>,
    ) -> Result<InstructionBlock> {
        ensure_uid(uid)?;
        ensure!(status != Status::Draft, "Situação inválida.");
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::SetBlockStatus {
                    uid: uid.to_owned(),
                    status,
                    expected_version,
                },
            )?;
            return self
                .block_by_uid(uid)?
                .context("A instrução não voltou do computador principal.");
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (block_id, current_status, blob): (i64, String, Vec<u8>) = tx
            .query_row(
                "SELECT id,status,payload FROM instruction_blocks WHERE uid=?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .context("Instrução não encontrada.")?;
        let head: BlockHead = decode(&blob)?;
        let current_status = Status::parse(&current_status)?;
        let details = match status {
            Status::Published if current_status == Status::Archived => {
                let current: i64 = tx.query_row(
                    "SELECT current_version FROM instruction_blocks WHERE id=?1",
                    [block_id],
                    |r| r.get(0),
                )?;
                let restored = if current > 0 { "published" } else { "draft" };
                tx.execute(
                    "UPDATE instruction_blocks SET status=?1,updated_at=?2 WHERE id=?3",
                    params![restored, now(), block_id],
                )?;
                format!("Desarquivou a instrução \"{}\"", head.name)
            }
            Status::Published => {
                let draft: Option<i64> = tx
                    .query_row(
                        "SELECT version FROM instruction_block_versions WHERE block_id=?1 AND published=0 ORDER BY version DESC LIMIT 1",
                        [block_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                match draft {
                    Some(number) => {
                        tx.execute(
                            "UPDATE instruction_block_versions SET published=1 WHERE block_id=?1 AND version=?2",
                            params![block_id, number],
                        )?;
                        tx.execute(
                            "UPDATE instruction_blocks SET status='published',current_version=?1,updated_at=?2 WHERE id=?3",
                            params![number, now(), block_id],
                        )?;
                        format!("Publicou a versão {number} da instrução \"{}\"", head.name)
                    }
                    None => bail!("Não há alterações a publicar nesta instrução."),
                }
            }
            Status::Archived => {
                ensure!(
                    current_status != Status::Archived,
                    "A instrução já está arquivada."
                );
                tx.execute(
                    "UPDATE instruction_blocks SET status='archived',updated_at=?1 WHERE id=?2",
                    params![now(), block_id],
                )?;
                format!("Arquivou a instrução \"{}\"", head.name)
            }
            Status::Draft => unreachable!(),
        };
        record_mutation(
            &tx,
            &actor,
            "block",
            version_key(uid),
            expected_version,
            "set_block_status",
            None,
            &details,
        )?;
        tx.commit()?;
        self.block_by_uid(uid)?
            .context("Instrução não encontrada depois da alteração.")
    }

    /// Removes a block for good. Refused while any template version
    /// references it.
    pub fn delete_block(&mut self, uid: &str) -> Result<()> {
        ensure_uid(uid)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::DeleteBlock {
                    uid: uid.to_owned(),
                },
            )?;
            return Ok(());
        }
        let users: Vec<String> = self
            .list_templates()?
            .iter()
            .filter(|t| {
                t.versions
                    .iter()
                    .any(|v| v.block_refs.iter().any(|r| r.block_uid == uid))
            })
            .map(|t| t.name.clone())
            .collect();
        ensure!(
            users.is_empty(),
            "Esta instrução é usada {} {}. Arquive-a em vez de apagar.",
            if users.len() == 1 { "pelo modelo" } else { "pelos modelos" },
            users.join(", ")
        );
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (block_id, blob): (i64, Vec<u8>) = tx
            .query_row(
                "SELECT id,payload FROM instruction_blocks WHERE uid=?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .context("Instrução não encontrada.")?;
        let head: BlockHead = decode(&blob)?;
        tx.execute("DELETE FROM instruction_blocks WHERE id=?1", [block_id])?;
        record_mutation(
            &tx,
            &actor,
            "block",
            version_key(uid),
            None,
            "delete_block",
            None,
            &format!("Excluiu a instrução \"{}\"", head.name),
        )?;
        tx.commit()?;
        Ok(())
    }

    // -- examples -----------------------------------------------------------

    /// Creates or updates an example. Approving records who did it.
    pub fn save_example(
        &mut self,
        example: &Example,
        expected_version: Option<i64>,
    ) -> Result<Example> {
        let mut example = example.clone();
        if example.uid.is_empty() {
            example.uid = new_uid();
        }
        ensure_uid(&example.uid)?;
        validate_example(&example)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::SaveExample {
                    example: example.clone(),
                    expected_version,
                },
            )?;
            return self
                .list_examples()?
                .into_iter()
                .find(|e| e.uid == example.uid)
                .context("O exemplo não voltou do computador principal.");
        }
        let actor = self.active_actor();
        let uid = example.uid.clone();
        let timestamp = now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let template_id: i64 = tx
            .query_row(
                "SELECT id FROM work_templates WHERE uid=?1",
                [&example.template_uid],
                |r| r.get(0),
            )
            .optional()?
            .context("Modelo do exemplo não encontrado.")?;
        let existing: Option<(i64, String, Vec<u8>)> = tx
            .query_row(
                "SELECT id,status,payload FROM template_examples WHERE uid=?1",
                [&uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let mut stored = example.clone();
        stored.id = 0;
        let previous_status = match &existing {
            Some((_, status, blob)) => {
                let previous: Example = decode(blob)?;
                let previous_status = ExampleStatus::parse(status)?;
                // Changed text was not reviewed: it goes back to "não
                // conferido" until someone approves it again.
                if previous_status == ExampleStatus::Approved
                    && stored.status == ExampleStatus::Approved
                    && !same_example_content(&previous, &stored)
                {
                    stored.status = ExampleStatus::Unverified;
                }
                stored.created_by = previous.created_by;
                stored.created_at = previous.created_at;
                stored.reviewed_by = previous.reviewed_by;
                stored.reviewed_at = previous.reviewed_at;
                Some(previous_status)
            }
            None => {
                stored.created_by = actor.clone();
                stored.created_at = timestamp.clone();
                None
            }
        };
        if stored.status == ExampleStatus::Approved
            && previous_status != Some(ExampleStatus::Approved)
        {
            stored.reviewed_by = actor.clone();
            stored.reviewed_at = timestamp.clone();
        } else if stored.status != ExampleStatus::Approved {
            stored.reviewed_by.clear();
            stored.reviewed_at.clear();
        }
        let details = match existing {
            None => {
                let count: i64 = tx.query_row(
                    "SELECT count(*) FROM template_examples WHERE template_id=?1",
                    [template_id],
                    |r| r.get(0),
                )?;
                ensure!(
                    (count as usize) < MAX_EXAMPLES_PER_TEMPLATE,
                    "Limite de {MAX_EXAMPLES_PER_TEMPLATE} exemplos por modelo."
                );
                tx.execute(
                    "INSERT INTO template_examples(uid,template_id,status,payload,created_at,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?5)",
                    params![uid, template_id, stored.status.as_str(), encode(&stored)?, timestamp],
                )?;
                format!("Acrescentou o exemplo \"{}\"", stored.title)
            }
            Some((example_id, _, _)) => {
                tx.execute(
                    "UPDATE template_examples SET template_id=?1,status=?2,payload=?3,updated_at=?4 WHERE id=?5",
                    params![template_id, stored.status.as_str(), encode(&stored)?, timestamp, example_id],
                )?;
                format!(
                    "Alterou o exemplo \"{}\" ({})",
                    stored.title,
                    stored.status.label()
                )
            }
        };
        record_mutation(
            &tx,
            &actor,
            "example",
            version_key(&uid),
            expected_version,
            "save_example",
            None,
            &details,
        )?;
        tx.commit()?;
        self.list_examples()?
            .into_iter()
            .find(|e| e.uid == uid)
            .context("Exemplo não encontrado depois de salvo.")
    }

    pub fn delete_example(&mut self, uid: &str) -> Result<()> {
        ensure_uid(uid)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::DeleteExample {
                    uid: uid.to_owned(),
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (example_id, blob): (i64, Vec<u8>) = tx
            .query_row(
                "SELECT id,payload FROM template_examples WHERE uid=?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .context("Exemplo não encontrado.")?;
        let example: Example = decode(&blob)?;
        tx.execute(
            "DELETE FROM attachments WHERE owner_kind='example' AND owner_id=?1",
            [example_id],
        )?;
        tx.execute("DELETE FROM template_examples WHERE id=?1", [example_id])?;
        record_mutation(
            &tx,
            &actor,
            "example",
            version_key(uid),
            None,
            "delete_example",
            None,
            &format!("Excluiu o exemplo \"{}\"", example.title),
        )?;
        tx.commit()?;
        Ok(())
    }

    // -- generation records --------------------------------------------------

    /// Stores what a generation used, accepted or refused. A record sent
    /// twice (a replay) updates the first.
    pub fn record_generation(&mut self, record: &GenerationRecord) -> Result<()> {
        validate_record(record)?;
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                IntranetMutation::RecordGeneration {
                    record: record.clone(),
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let mut stored = record.clone();
        // Who generated is who sends it, never what the record says; the
        // time is kept when plausible (a queued record arrives later).
        stored.created_by = actor.clone();
        let plausible = chrono::DateTime::parse_from_rfc3339(&stored.created_at)
            .is_ok_and(|t| t <= chrono::Utc::now() + chrono::Duration::minutes(5));
        if !plausible {
            stored.created_at = now();
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let belongs: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM captures WHERE id=?1 AND document_id=?2)",
            params![stored.capture_id, stored.document_id],
            |r| r.get(0),
        )?;
        ensure!(belongs, "A importação não pertence a este documento.");
        let existing: Option<(i64, i64)> = tx
            .query_row(
                "SELECT document_id,capture_id FROM draft_generations WHERE uid=?1",
                [&stored.uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((document_id, capture_id)) = existing {
            // A replay (queued change sent twice): the first record stands,
            // and it can never be moved to another document.
            ensure!(
                document_id == stored.document_id && capture_id == stored.capture_id,
                "Este registro de geração pertence a outro documento."
            );
            if let Some(draft_id) = stored.draft_id {
                link_generation(&tx, &stored.uid, document_id, draft_id)?;
            }
            tx.commit()?;
            return Ok(());
        }
        // A draft named by the record must be of the same document.
        if let Some(draft_id) = stored.draft_id {
            let same: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM drafts WHERE id=?1 AND document_id=?2)",
                params![draft_id, stored.document_id],
                |r| r.get(0),
            )?;
            if !same {
                stored.draft_id = None;
            }
        }
        tx.execute(
            "INSERT INTO draft_generations(uid,document_id,capture_id,draft_id,template_uid,payload,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                stored.uid,
                stored.document_id,
                stored.capture_id,
                stored.draft_id,
                stored.template_uid,
                encode(&stored)?,
                stored.created_at
            ],
        )?;
        let what = match &stored.outcome {
            Outcome::Accepted => "concluída".to_owned(),
            Outcome::Refused { reason } => format!("recusada: {reason}"),
        };
        record_mutation(
            &tx,
            &actor,
            "generation",
            version_key(&stored.uid),
            None,
            "record_generation",
            Some(stored.document_id),
            &format!(
                "Geração {what} ({}{})",
                match &stored.template_uid {
                    Some(_) => format!(
                        "modelo \"{}\" v{}",
                        stored.template_name,
                        stored.template_version.unwrap_or(0)
                    ),
                    None => "sem modelo".to_owned(),
                },
                if stored.model.is_empty() {
                    String::new()
                } else {
                    format!(", {}", stored.model)
                }
            ),
        )?;
        tx.commit()?;
        Ok(())
    }

    // -- migration ------------------------------------------------------------

    /// Turns the personal templates of versions before 0.20 into work
    /// templates: one per (name, kind), one published version per old row,
    /// a single section "Texto" with the body. Runs once per library and
    /// goes through the usual saving path (so a client computer sends them
    /// to the main one). Old texts are normalized to the new limits (line
    /// endings, name length); a group that still cannot be saved is skipped
    /// and reported, never retried forever. Returns how many templates were
    /// created and the problems found.
    pub fn migrate_personal_templates(&mut self) -> Result<(usize, Vec<String>)> {
        let done: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM work_template_settings WHERE key='migrated_personal'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if done.as_deref() == Some("1") {
            return Ok((0, Vec::new()));
        }
        let mut old = self.personal_templates()?;
        old.sort_by_key(|(id, _)| *id);
        let mut groups: Vec<((String, String), Vec<String>)> = Vec::new();
        for (_, template) in old {
            let name: String = template.name.trim().chars().take(MAX_NAME / 2).collect();
            let key = (name, template.kind.clone());
            // Other control characters (a form feed pasted from a PDF) are
            // refused by the new rules: they become spaces.
            let body: String = template
                .body
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .chars()
                .map(|c| {
                    if c.is_control() && c != '\n' && c != '\t' {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, bodies)) => bodies.push(body),
                None => groups.push((key, vec![body])),
            }
        }
        // An interrupted run leaves migrated groups behind: recognized by
        // their origin mark, they are not created twice.
        // Only this member's own personal ones: a colleague's migrated
        // template with the same name (arrived by sync) is not this one.
        let me = owner_key(&self.active_actor());
        let already: Vec<(String, String)> = self
            .list_templates()?
            .into_iter()
            .filter(|t| {
                t.derived_from.as_deref() == Some(MIGRATED_FROM)
                    && t.scope == Scope::Personal
                    && owner_key(&t.owner) == me
            })
            .map(|t| (t.name.clone(), t.kind.clone()))
            .collect();
        let mut created = 0;
        let mut problems = Vec::new();
        for ((name, kind), bodies) in groups {
            if already.contains(&(name.clone(), kind.clone())) {
                continue;
            }
            let outcome = (|| -> Result<()> {
                let mut template = WorkTemplate::new(&name, &kind, &bodies[0]);
                template.purpose = "Migrado dos modelos pessoais de versões anteriores.".into();
                template.derived_from = Some(MIGRATED_FROM.into());
                let mut saved = self.save_template(&template, None)?;
                saved = self.set_template_status(&saved.uid, Status::Published, None)?;
                for body in bodies.iter().skip(1) {
                    let mut next = saved.clone();
                    let mut version = saved.published_version().cloned().unwrap_or_default();
                    version.sections = vec![Section {
                        title: "Texto".into(),
                        guidance: body.clone(),
                        required: true,
                    }];
                    version.variables = detect_variables(body);
                    version.change_note = "Versão migrada dos modelos pessoais.".into();
                    next.versions.push(version);
                    saved = self.save_template(&next, None)?;
                    if saved.draft_version().is_some() {
                        saved = self.set_template_status(&saved.uid, Status::Published, None)?;
                    }
                }
                Ok(())
            })();
            match outcome {
                Ok(()) => created += 1,
                Err(error) => problems.push(format!("\"{name}\": {error:#}")),
            }
        }
        // Done only when nothing failed: a group that could not be saved (a
        // busy library, the main computer out of reach) is tried again at
        // the next opening; the ones already created are recognized.
        if problems.is_empty() {
            self.conn.execute(
                "INSERT INTO work_template_settings(key,value) VALUES('migrated_personal','1')
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [],
            )?;
        }
        Ok((created, problems))
    }

    // -- team sync --------------------------------------------------------------

    pub(crate) fn export_templates(&self) -> Result<TemplatesSnapshot> {
        let mut out = TemplatesSnapshot::default();
        let mut statement = self.conn.prepare(
            "SELECT id,uid,scope,status,owner_hash,current_version,payload,updated_at FROM work_templates ORDER BY id",
        )?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<Vec<u8>>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Vec<u8>>(6)?,
                r.get::<_, String>(7)?,
            ))
        })? {
            let (id, uid, scope, status, owner_hash, current_version, blob, updated_at) = row?;
            out.templates.push(TemplateRow {
                id,
                uid,
                scope,
                status,
                owner_hash,
                current_version,
                head: decode(&blob)?,
                updated_at,
            });
        }
        let mut statement = self.conn.prepare(
            "SELECT id,template_id,version,published,payload,created_at FROM work_template_versions ORDER BY id",
        )?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })? {
            let (id, template_id, version, published, blob, created_at) = row?;
            out.template_versions.push(TemplateVersionRow {
                id,
                template_id,
                version,
                published: published != 0,
                value: decode(&blob)?,
                created_at,
            });
        }
        let mut statement = self.conn.prepare(
            "SELECT id,uid,scope,status,owner_hash,current_version,payload,updated_at FROM instruction_blocks ORDER BY id",
        )?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<Vec<u8>>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Vec<u8>>(6)?,
                r.get::<_, String>(7)?,
            ))
        })? {
            let (id, uid, scope, status, owner_hash, current_version, blob, updated_at) = row?;
            out.blocks.push(BlockRow {
                id,
                uid,
                scope,
                status,
                owner_hash,
                current_version,
                head: decode(&blob)?,
                updated_at,
            });
        }
        let mut statement = self.conn.prepare(
            "SELECT id,block_id,version,published,payload,created_at FROM instruction_block_versions ORDER BY id",
        )?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })? {
            let (id, block_id, version, published, blob, created_at) = row?;
            out.block_versions.push(BlockVersionRow {
                id,
                block_id,
                version,
                published: published != 0,
                value: decode(&blob)?,
                created_at,
            });
        }
        let mut statement = self.conn.prepare(
            "SELECT id,uid,template_id,status,payload,created_at,updated_at FROM template_examples ORDER BY id",
        )?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })? {
            let (id, uid, template_id, status, blob, created_at, updated_at) = row?;
            out.examples.push(ExampleRow {
                id,
                uid,
                template_id,
                status,
                value: decode(&blob)?,
                created_at,
                updated_at,
            });
        }
        let mut statement = self.conn.prepare(
            "SELECT id,uid,document_id,capture_id,draft_id,template_uid,payload,created_at FROM draft_generations ORDER BY id",
        )?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Vec<u8>>(6)?,
                r.get::<_, String>(7)?,
            ))
        })? {
            let (id, uid, document_id, capture_id, draft_id, template_uid, blob, created_at) = row?;
            out.generations.push(GenerationRow {
                id,
                uid,
                document_id,
                capture_id,
                draft_id,
                template_uid,
                record: decode(&blob)?,
                created_at,
            });
        }
        out.attachment_texts = self.export_attachment_texts()?;
        Ok(out)
    }
}

/// Writes every templates table from a snapshot (the tables were emptied by
/// the caller), re-sealing the payloads for this Windows account.
pub(crate) fn apply_templates(tx: &Connection, snapshot: &TemplatesSnapshot) -> Result<()> {
    for row in &snapshot.templates {
        tx.execute(
            "INSERT INTO work_templates(id,uid,scope,status,owner_hash,current_version,payload,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                row.id,
                row.uid,
                row.scope,
                row.status,
                row.owner_hash,
                row.current_version,
                encode(&row.head)?,
                row.updated_at
            ],
        )?;
    }
    for row in &snapshot.template_versions {
        tx.execute(
            "INSERT INTO work_template_versions(id,template_id,version,published,payload,created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                row.id,
                row.template_id,
                row.version,
                row.published as i64,
                encode(&row.value)?,
                row.created_at
            ],
        )?;
    }
    for row in &snapshot.blocks {
        tx.execute(
            "INSERT INTO instruction_blocks(id,uid,scope,status,owner_hash,current_version,payload,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                row.id,
                row.uid,
                row.scope,
                row.status,
                row.owner_hash,
                row.current_version,
                encode(&row.head)?,
                row.updated_at
            ],
        )?;
    }
    for row in &snapshot.block_versions {
        tx.execute(
            "INSERT INTO instruction_block_versions(id,block_id,version,published,payload,created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                row.id,
                row.block_id,
                row.version,
                row.published as i64,
                encode(&row.value)?,
                row.created_at
            ],
        )?;
    }
    for row in &snapshot.examples {
        tx.execute(
            "INSERT INTO template_examples(id,uid,template_id,status,payload,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                row.id,
                row.uid,
                row.template_id,
                row.status,
                encode(&row.value)?,
                row.created_at,
                row.updated_at
            ],
        )?;
    }
    for row in &snapshot.generations {
        tx.execute(
            "INSERT INTO draft_generations(id,uid,document_id,capture_id,draft_id,template_uid,payload,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                row.id,
                row.uid,
                row.document_id,
                row.capture_id,
                row.draft_id,
                row.template_uid,
                encode(&row.record)?,
                row.created_at
            ],
        )?;
    }
    crate::extraction::apply_attachment_texts(tx, &snapshot.attachment_texts)?;
    Ok(())
}

/// Links a saved draft version to the generation it came from (inside the
/// draft's transaction).
pub(crate) fn link_generation(
    tx: &Connection,
    generation_uid: &str,
    document_id: i64,
    draft_id: i64,
) -> Result<()> {
    // Only a generation of the same document, not linked yet, to a draft
    // of that document.
    tx.execute(
        "UPDATE draft_generations SET draft_id=?1
         WHERE uid=?2 AND document_id=?3 AND draft_id IS NULL
           AND EXISTS(SELECT 1 FROM drafts WHERE id=?1 AND document_id=?3)",
        params![draft_id, generation_uid, document_id],
    )?;
    Ok(())
}

/// Whether two versions say the same thing, block versions included
/// (author, dates and notes aside).
fn same_template_content(a: &TemplateVersion, b: &TemplateVersion) -> bool {
    let refs = |v: &TemplateVersion| -> Vec<(String, BlockMode, Option<u32>)> {
        v.block_refs
            .iter()
            .map(|r| (r.block_uid.clone(), r.mode.clone(), r.version))
            .collect()
    };
    a.sections == b.sections
        && a.variables == b.variables
        && refs(a) == refs(b)
        && a.references == b.references
        && a.criteria == b.criteria
}

fn same_example_content(a: &Example, b: &Example) -> bool {
    a.template_uid == b.template_uid
        && a.title == b.title
        && a.demand == b.demand
        && a.reply == b.reply
        && a.explanation == b.explanation
        && a.comments == b.comments
}

fn owner_hash_for(scope: Scope, owner: &str) -> Option<Vec<u8>> {
    match scope {
        Scope::Personal => Some(owner_key(owner)),
        Scope::Shared => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        (dir, store)
    }

    #[test]
    fn templates_blocks_and_examples_are_versioned_and_sealed() {
        let (dir, mut store) = store();
        let mut block = InstructionBlock::new(
            "Linguagem",
            "Linguagem e estilo",
            "SEGREDO_INSTRUCAO use frases curtas.",
        );
        block.scope = Scope::Shared;
        let block = store.save_block(&block, None).unwrap();
        assert_eq!(block.status, Status::Draft);
        assert!(block.published_version().is_none());
        // A template cannot be published while its block is a draft.
        let mut template = WorkTemplate::new(
            "Resposta padrão",
            "resposta",
            "SEGREDO_MODELO Ref. {{numero_documento}}",
        );
        template.scope = Scope::Shared;
        template.versions[0].block_refs.push(BlockRef {
            block_uid: block.uid.clone(),
            version: None,
            mode: BlockMode::Mandatory,
        });
        let template = store.save_template(&template, None).unwrap();
        assert_eq!(template.versions[0].variables[0].name, "numero_documento");
        let refused = store
            .set_template_status(&template.uid, Status::Published, None)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("Publique a instrução"), "{refused}");
        store
            .set_block_status(&block.uid, Status::Published, None)
            .unwrap();
        let published = store
            .set_template_status(&template.uid, Status::Published, None)
            .unwrap();
        assert_eq!(published.status, Status::Published);
        assert_eq!(published.current_version, 1);
        assert_eq!(
            published.published_version().unwrap().block_refs[0].version,
            Some(1),
            "the block version is pinned at publication"
        );
        // Editing a published template starts version 2 as a draft; the
        // published version is untouched.
        let mut revised = published.clone();
        let mut next = published.published_version().unwrap().clone();
        next.sections[0].guidance = "Texto revisto".into();
        revised.versions.push(next);
        let revised = store.save_template(&revised, None).unwrap();
        assert_eq!(revised.versions.len(), 2);
        assert_eq!(revised.current_version, 1);
        assert!(revised.draft_version().is_some());
        assert_eq!(
            revised.published_version().unwrap().sections[0].guidance,
            "SEGREDO_MODELO Ref. {{numero_documento}}"
        );
        // Saving again edits the same draft instead of adding a third version.
        let again = store.save_template(&revised, None).unwrap();
        assert_eq!(again.versions.len(), 2);
        // Archiving a never-published template and bringing it back leaves
        // it a draft; a published one comes back published.
        let fresh = store
            .save_template(&WorkTemplate::new("Novo", "resposta", "Texto"), None)
            .unwrap();
        store
            .set_template_status(&fresh.uid, Status::Archived, None)
            .unwrap();
        let back = store
            .set_template_status(&fresh.uid, Status::Published, None)
            .unwrap();
        assert_eq!(back.status, Status::Draft);
        assert_eq!(back.current_version, 0);
        // A working copy identical to the published version is not stored.
        let published_only = store
            .set_template_status(&again.uid, Status::Published, None)
            .unwrap();
        assert_eq!(published_only.current_version, 2);
        let mut untouched = published_only.clone();
        let mut copy = published_only.published_version().unwrap().clone();
        copy.version = 3;
        copy.published = false;
        untouched.versions.push(copy);
        let after = store.save_template(&untouched, None).unwrap();
        assert_eq!(after.versions.len(), 2, "no empty revision stored");
        assert!(store
            .set_template_status(&after.uid, Status::Published, None)
            .is_err());
        // Examples: approval records the reviewer; unverified by default.
        let example = Example {
            template_uid: again.uid.clone(),
            title: "Caso de 2024".into(),
            demand: "SEGREDO_DEMANDA".into(),
            reply: "SEGREDO_RESPOSTA".into(),
            ..Default::default()
        };
        let saved = store.save_example(&example, None).unwrap();
        assert_eq!(saved.status, ExampleStatus::Unverified);
        assert!(saved.reviewed_by.is_empty());
        let mut approved = saved.clone();
        approved.status = ExampleStatus::Approved;
        let approved = store.save_example(&approved, None).unwrap();
        assert!(!approved.reviewed_by.is_empty());
        assert_eq!(store.examples_of(&again.uid).unwrap().len(), 1);
        // A stale version is refused.
        let error = store
            .set_template_status(&again.uid, Status::Archived, Some(0))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Conflito de edição"), "{error}");
        // Nothing in clear on disk.
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        let raw = std::fs::read(dir.path().join("acervo.sqlite3")).unwrap();
        for secret in [
            "SEGREDO_INSTRUCAO",
            "SEGREDO_MODELO",
            "SEGREDO_DEMANDA",
            "SEGREDO_RESPOSTA",
            "Resposta padrão",
            "Caso de 2024",
        ] {
            assert!(
                !raw.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "{secret} em claro"
            );
        }
    }

    #[test]
    fn a_template_used_by_a_generation_can_only_be_archived() {
        let (_dir, mut store) = store();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let template = store
            .save_template(&WorkTemplate::new("Ofício", "resposta", "Texto"), None)
            .unwrap();
        let template = store
            .set_template_status(&template.uid, Status::Published, None)
            .unwrap();
        let record = GenerationRecord {
            uid: new_uid(),
            document_id: receipt.document_id,
            capture_id: receipt.capture_id,
            template_uid: Some(template.uid.clone()),
            template_version: Some(1),
            template_name: template.name.clone(),
            outcome: Outcome::Refused {
                reason: "teste".into(),
            },
            ..Default::default()
        };
        store.record_generation(&record).unwrap();
        let listed = store.generations_for_document(receipt.document_id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].template_version, Some(1));
        let error = store
            .delete_template(&template.uid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Arquive-o"), "{error}");
        store
            .set_template_status(&template.uid, Status::Archived, None)
            .unwrap();
        assert!(!store
            .template_by_uid(&template.uid)
            .unwrap()
            .unwrap()
            .offered_for("resposta", "qualquer"));
        // Without generations, deletion cascades to versions and examples.
        let other = store
            .save_template(&WorkTemplate::new("Outro", "resposta", "Texto"), None)
            .unwrap();
        store
            .save_example(
                &Example {
                    template_uid: other.uid.clone(),
                    title: "Ex".into(),
                    reply: "Resposta".into(),
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        store.delete_template(&other.uid).unwrap();
        assert!(store.template_by_uid(&other.uid).unwrap().is_none());
        assert!(store.examples_of(&other.uid).unwrap().is_empty());
        let versions: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM work_template_versions v WHERE NOT EXISTS(SELECT 1 FROM work_templates t WHERE t.id=v.template_id)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(versions, 0);
    }

    #[test]
    fn records_and_examples_resist_rewriting() {
        let (_dir, mut store) = store();
        let one = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let mut other_capture = crate::model::sample_capture();
        other_capture.request_id = uuid::Uuid::new_v4().to_string();
        other_capture.document_number = "SEFAZ-PRO-2026/00999".into();
        other_capture.source_url = other_capture.source_url.replace("00001", "00999");
        let two = store.import_capture(&other_capture).unwrap();
        assert_ne!(one.document_id, two.document_id);
        let record = GenerationRecord {
            uid: new_uid(),
            document_id: one.document_id,
            capture_id: one.capture_id,
            created_by: "Outra pessoa".into(),
            outcome: Outcome::Accepted,
            ..Default::default()
        };
        store.record_generation(&record).unwrap();
        let stored = store.generations_for_document(one.document_id).unwrap();
        assert_ne!(stored[0].created_by, "Outra pessoa");
        // A replay keeps the first record; another document is refused.
        let replay = GenerationRecord {
            model: "outro".into(),
            ..record.clone()
        };
        store.record_generation(&replay).unwrap();
        assert!(store.generations_for_document(one.document_id).unwrap()[0]
            .model
            .is_empty());
        let moved = GenerationRecord {
            document_id: two.document_id,
            capture_id: two.capture_id,
            ..record.clone()
        };
        assert!(store.record_generation(&moved).is_err());
        // A capture of another document is refused too.
        let mismatched = GenerationRecord {
            uid: new_uid(),
            capture_id: two.capture_id,
            ..record.clone()
        };
        assert!(store.record_generation(&mismatched).is_err());

        // An approved example whose text changes needs approval again.
        let template = store
            .save_template(&WorkTemplate::new("Ofício", "resposta", "Texto"), None)
            .unwrap();
        let example = store
            .save_example(
                &Example {
                    template_uid: template.uid.clone(),
                    title: "Ex".into(),
                    reply: "Resposta".into(),
                    status: ExampleStatus::Approved,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        assert_eq!(example.status, ExampleStatus::Approved);
        let renamed = store
            .save_example(
                &Example {
                    origin: "anotação nova".into(),
                    ..example.clone()
                },
                None,
            )
            .unwrap();
        assert_eq!(renamed.status, ExampleStatus::Approved);
        let edited = store
            .save_example(
                &Example {
                    reply: "Resposta alterada".into(),
                    ..renamed
                },
                None,
            )
            .unwrap();
        assert_eq!(edited.status, ExampleStatus::Unverified);
        assert!(edited.reviewed_by.is_empty());
    }

    #[test]
    fn publishing_fixes_what_the_publisher_saw() {
        let (_dir, mut store) = store();
        let block = store
            .save_block(
                &InstructionBlock::new("Tom", "estilo", "Seja cordial."),
                None,
            )
            .unwrap();
        let block = store
            .set_block_status(&block.uid, Status::Published, None)
            .unwrap();
        let mut template = WorkTemplate::new("Ofício", "resposta", "Texto");
        template.versions[0].block_refs.push(BlockRef {
            block_uid: block.uid.clone(),
            version: None,
            mode: BlockMode::Mandatory,
        });
        let template = store.save_template(&template, None).unwrap();
        let plan = store.publish_plan(&template.uid).unwrap().unwrap();
        assert_eq!(plan.pins, vec![(block.uid.clone(), 1)]);
        assert!(!plan.unarchive && !plan.draft_sha256.is_empty());
        // Meanwhile the block gets a version 2.
        let mut newer = block.clone();
        newer.versions.last_mut().unwrap().text = "Seja cordial e breve.".into();
        let newer = store.save_block(&newer, None).unwrap();
        store
            .set_block_status(&newer.uid, Status::Published, None)
            .unwrap();
        // The replayed publication pins what its author saw, version 1.
        let published = store
            .set_template_status_with(&template.uid, Status::Published, Some(plan), None)
            .unwrap();
        assert_eq!(
            published.published_version().unwrap().block_refs[0].version,
            Some(1)
        );
        // A revision that only follows the newer instruction is a change
        // that can be published.
        let revised = store.save_template(&published, None).unwrap();
        assert!(revised.draft_version().is_some());
        let republished = store
            .set_template_status(&revised.uid, Status::Published, None)
            .unwrap();
        assert_eq!(
            republished.published_version().unwrap().block_refs[0].version,
            Some(2)
        );
        // A publication for a draft that was archived meanwhile is refused,
        // not turned into an unarchive.
        let again = store.save_template(
            &WorkTemplate {
                versions: vec![TemplateVersion {
                    criteria: vec!["Nova regra".into()],
                    ..republished.published_version().unwrap().clone()
                }],
                ..republished.clone()
            },
            None,
        );
        let again = again.unwrap();
        let plan = store.publish_plan(&again.uid).unwrap();
        store
            .set_template_status(&again.uid, Status::Archived, None)
            .unwrap();
        assert!(store
            .set_template_status_with(&again.uid, Status::Published, plan, None)
            .is_err());
        // Bringing it back, then a second "bring back" that arrives late,
        // never publishes the pending draft.
        let back = store.publish_plan(&again.uid).unwrap();
        assert!(back.as_ref().unwrap().unarchive);
        store
            .set_template_status_with(&again.uid, Status::Published, back.clone(), None)
            .unwrap();
        assert!(store
            .set_template_status_with(&again.uid, Status::Published, back, None)
            .is_err());
        assert!(store
            .template_by_uid(&again.uid)
            .unwrap()
            .unwrap()
            .draft_version()
            .is_some());
    }

    #[test]
    fn personal_templates_become_work_templates_keeping_bodies() {
        let (_dir, mut store) = store();
        store
            .save_personal_template("Modelo antigo", "resposta", "Primeira versão {{assunto}}")
            .unwrap();
        store
            .save_personal_template("Modelo antigo", "resposta", "Segunda versão")
            .unwrap();
        store
            .save_personal_template("Encaminhar", "encaminhamento", "Encaminho")
            .unwrap();
        store
            .save_personal_template(
                "Com quebras do Windows",
                "resposta",
                "Linha um\r\nLinha dois",
            )
            .unwrap();
        let (created, problems) = store.migrate_personal_templates().unwrap();
        assert_eq!((created, problems.len()), (3, 0), "{problems:?}");
        assert_eq!(store.migrate_personal_templates().unwrap().0, 0, "once");
        let windows = store
            .list_templates()
            .unwrap()
            .into_iter()
            .find(|t| t.name == "Com quebras do Windows")
            .unwrap();
        assert_eq!(
            windows.published_version().unwrap().sections[0].guidance,
            "Linha um\nLinha dois"
        );
        assert_eq!(windows.derived_from.as_deref(), Some("personal_templates"));
        let templates = store.list_templates().unwrap();
        assert_eq!(templates.len(), 3);
        let old = templates
            .iter()
            .find(|t| t.name == "Modelo antigo")
            .unwrap();
        assert_eq!(old.status, Status::Published);
        assert_eq!(old.current_version, 2);
        assert_eq!(old.scope, Scope::Personal);
        assert_eq!(
            old.version(1).unwrap().sections[0].guidance,
            "Primeira versão {{assunto}}"
        );
        assert_eq!(old.version(1).unwrap().variables[0].name, "assunto");
        assert_eq!(
            old.version(2).unwrap().sections[0].guidance,
            "Segunda versão"
        );
        assert!(old.offered_for("resposta", &store.active_actor()));
        assert!(!old.offered_for("resposta", "outra pessoa"));
    }

    #[test]
    fn version_keys_are_stable_and_positive() {
        let a = version_key("0123456789abcdef0123456789abcdef");
        assert_eq!(a, version_key("0123456789abcdef0123456789abcdef"));
        assert!(a > 0);
        assert_ne!(a, version_key("fedcba9876543210fedcba9876543210"));
    }
    #[test]
    fn drafts_saved_before_provenance_existed_still_load() {
        let (_dir, mut store) = store();
        let receipt = store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        // A row exactly as 0.19 wrote it: no provenance field at all.
        let legacy = seal(br#"{"materials":[],"kind":"resposta","body":"Minuta antiga"}"#).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO drafts(document_id,capture_id,payload,created_at) VALUES(?1,?2,?3,?4)",
                params![receipt.document_id, receipt.capture_id, legacy, now()],
            )
            .unwrap();
        let drafts = store.load_document(receipt.document_id).unwrap().drafts;
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].body, "Minuta antiga");
        assert!(drafts[0].provenance.is_none());
    }
}
