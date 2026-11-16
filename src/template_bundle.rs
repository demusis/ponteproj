//! Bringing work templates and instructions from a JSON file into the
//! library (`ponteproj --import-templates ARQUIVO [--publicar]`): a set
//! prepared elsewhere, or handed over by another team. The whole file is
//! checked, with the rules the saving applies, before anything is saved;
//! everything arrives personal and as a draft unless publishing is asked
//! for, and publishing comes last. Among the items this person can see, an
//! instruction with the same name is reused and a template with the same
//! name and kind is left alone, so importing the same file twice changes
//! nothing. Asked to publish, this person's own drafts of the file already
//! in the library are published too (instructions before templates); a
//! colleague's draft or an edit to a published item never is. Examples
//! always arrive as "não conferido".
use crate::storage::Store;
use crate::work_templates::{
    detect_variables, BlockMode, BlockRef, InstructionBlock, Reference, Section, Status,
    TemplateVersion, Variable, WorkTemplate,
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Largest file accepted.
pub const MAX_BUNDLE_BYTES: u64 = 4 * 1024 * 1024;

/// Name written in the `format` field of the file.
pub const BUNDLE_FORMAT: &str = "ponte-modelos";

/// Layout version this Ponte writes and the newest it can read.
pub const BUNDLE_VERSION: u32 = 1;

fn first_version() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Bundle {
    /// Empty in files made before the field existed.
    #[serde(default)]
    pub format: String,
    /// Files made before the field existed are version 1.
    #[serde(default = "first_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<BundleBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<BundleTemplate>,
    /// Past cases for the templates, named by template. They always arrive
    /// as "não conferido": only a person approves an example.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<BundleExample>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BundleExample {
    /// Name of the template (in this file or already in the library).
    pub template: String,
    /// Its kind, needed only when two templates share the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub title: String,
    pub demand: String,
    pub reply: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub explanation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BundleBlock {
    /// How templates in the same file refer to it.
    pub key: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub topic: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BundleTemplate {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub usage_criteria: String,
    pub sections: Vec<Section>,
    /// Labels and "required" flags of the `{{variables}}` of the sections.
    /// Only the variables the sections use are taken; the others are
    /// ignored, and without this field the labels are made from the names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<BundleRef>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BundleRef {
    pub key: String,
    /// obrigatoria | opcional | condicional
    pub mode: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,
}

/// What a person picks to export. Instructions cited by the chosen
/// templates go along even when not listed.
#[derive(Debug, Default, Clone)]
pub struct BundleSelection {
    pub template_uids: Vec<String>,
    pub block_uids: Vec<String>,
    pub include_examples: bool,
}

/// What importing a file would do, without doing it.
#[derive(Debug, Default)]
pub struct BundlePreview {
    /// Names of the instructions that would be created.
    pub new_blocks: Vec<String>,
    /// "Name (kind)" of the templates that would be created.
    pub new_templates: Vec<String>,
    pub new_examples: usize,
    /// Items already in the library, left as they are.
    pub kept: Vec<String>,
}

#[derive(Debug, Default)]
pub struct BundleReport {
    pub blocks_created: usize,
    pub templates_created: usize,
    pub examples_created: usize,
    /// Items already in the library, left as they were.
    pub kept: Vec<String>,
    /// Drafts already in the library, published on request.
    pub published: Vec<String>,
}

fn mode_of(reference: &BundleRef) -> Result<BlockMode> {
    Ok(match reference.mode.trim() {
        "obrigatoria" | "obrigatória" => BlockMode::Mandatory,
        "opcional" => BlockMode::Optional,
        "condicional" => {
            ensure!(
                !reference.condition.trim().is_empty(),
                "A instrução \"{}\" é condicional sem condição.",
                reference.key
            );
            BlockMode::Conditional {
                condition: reference.condition.trim().to_owned(),
            }
        }
        other => bail!(
            "Modo \"{other}\" da instrução \"{}\": use obrigatoria, opcional ou condicional.",
            reference.key
        ),
    })
}

/// Reads and parses a bundle file.
pub fn read_bundle(path: &std::path::Path) -> Result<Bundle> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("Não foi possível ler {}", path.display()))?
        .len();
    ensure!(
        size <= MAX_BUNDLE_BYTES,
        "O arquivo tem mais de {} MB.",
        MAX_BUNDLE_BYTES / (1024 * 1024)
    );
    let bytes = std::fs::read(path)?;
    // Notepad and others write a byte order mark the parser would refuse.
    let bytes = bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF][..])
        .unwrap_or(&bytes);
    let bundle: Bundle = serde_json::from_slice(bytes).map_err(|error| {
        anyhow::anyhow!(
            "O arquivo não é um conjunto de modelos válido (linha {}, coluna {}).",
            error.line(),
            error.column()
        )
    })?;
    // Some other JSON object parses into an empty bundle: say so instead of
    // reporting that everything already exists.
    ensure!(
        !(bundle.format.trim().is_empty()
            && bundle.blocks.is_empty()
            && bundle.templates.is_empty()
            && bundle.examples.is_empty()),
        "O arquivo não é um conjunto de modelos do Ponte."
    );
    ensure!(
        bundle.format.trim().is_empty() || bundle.format == BUNDLE_FORMAT,
        "O arquivo não é um conjunto de modelos do Ponte."
    );
    ensure!(
        bundle.version <= BUNDLE_VERSION,
        "O arquivo foi feito por uma versão mais nova do Ponte. Atualize o Ponte para importá-lo."
    );
    Ok(bundle)
}

/// Writes a bundle as readable JSON: the size is checked first, and the
/// file is written beside the target and renamed over it, so a failure
/// never leaves half a file.
pub fn write_bundle(path: &std::path::Path, bundle: &Bundle) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(bundle)?;
    ensure!(
        bytes.len() as u64 <= MAX_BUNDLE_BYTES,
        "O arquivo passaria de {} MB. Exporte menos itens ou deixe os exemplos de fora.",
        MAX_BUNDLE_BYTES / (1024 * 1024)
    );
    let name = path
        .file_name()
        .context("Informe o nome do arquivo.")?
        .to_string_lossy()
        .into_owned();
    let temp = path.with_file_name(format!(".{name}.tmp"));
    let written = std::fs::write(&temp, &bytes).and_then(|()| std::fs::rename(&temp, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(error).with_context(|| format!("Não foi possível gravar {}", path.display()));
    }
    Ok(())
}

impl Store {
    /// Says why `version` cannot be published: its first instruction without
    /// a published version, or one that no longer exists (a template cannot
    /// be published before its instructions). The text is shown to the person.
    fn unpublished_block(
        &self,
        version: Option<&crate::work_templates::TemplateVersion>,
    ) -> Result<Option<String>> {
        let Some(version) = version else {
            return Ok(None);
        };
        let blocks = self.list_blocks()?;
        for reference in &version.block_refs {
            match blocks.iter().find(|b| b.uid == reference.block_uid) {
                Some(b) if b.published_version().is_some() && b.status != Status::Archived => {}
                Some(b) => {
                    return Ok(Some(format!(
                        "a instrução \"{}\" não está publicada",
                        b.name
                    )))
                }
                None => return Ok(Some("uma instrução que não existe mais".to_owned())),
            }
        }
        Ok(None)
    }

    /// Checks the whole file with the rules the saving applies and says
    /// what importing it would do. Writes nothing.
    pub fn preview_template_bundle(&self, bundle: &Bundle) -> Result<BundlePreview> {
        use crate::work_templates::{
            validate_block, validate_example, validate_template, MAX_BLOCKS,
            MAX_EXAMPLES_PER_TEMPLATE, MAX_TEMPLATES,
        };
        // Stands in for uids not created yet while the file is checked.
        const PLACEHOLDER_UID: &str = "00000000000000000000000000000000";
        let me = self.active_actor();
        // Only what this person sees is reused: a colleague's personal item
        // that arrived by sync is neither matched nor published.
        let all_blocks = self.list_blocks()?;
        let all_templates = self.list_templates()?;
        let existing_blocks: Vec<&InstructionBlock> =
            all_blocks.iter().filter(|b| b.visible_to(&me)).collect();
        let existing_templates: Vec<&WorkTemplate> =
            all_templates.iter().filter(|t| t.visible_to(&me)).collect();
        let mut preview = BundlePreview::default();

        // The whole file is checked before anything is written, so an error
        // never leaves half a set.
        let mut keys = HashSet::new();
        for block in &bundle.blocks {
            ensure!(
                keys.insert(block.key.as_str()),
                "A chave \"{}\" aparece em duas instruções.",
                block.key
            );
            validate_block(&InstructionBlock::new(
                block.name.trim(),
                block.topic.trim(),
                block.text.trim(),
            ))
            .with_context(|| format!("Instrução \"{}\"", block.name))?;
            if existing_blocks
                .iter()
                .any(|b| b.name.trim() == block.name.trim())
            {
                preview
                    .kept
                    .push(format!("Instrução \"{}\" (já existia)", block.name.trim()));
            } else {
                preview.new_blocks.push(block.name.trim().to_owned());
            }
        }
        let mut stand_in: HashMap<&str, String> = HashMap::new();
        let mut by_name: HashMap<&str, String> = HashMap::new();
        for block in &bundle.blocks {
            let next = format!("{:032x}", by_name.len() + 1);
            let uid = by_name.entry(block.name.trim()).or_insert(next).clone();
            stand_in.insert(block.key.as_str(), uid);
        }
        ensure!(
            all_blocks.len() + preview.new_blocks.len() <= MAX_BLOCKS,
            "O arquivo passaria do limite de {MAX_BLOCKS} instruções."
        );
        let mut pairs = HashSet::new();
        for template in &bundle.templates {
            ensure!(
                pairs.insert((template.name.trim(), template.kind.as_str())),
                "O modelo \"{}\" ({}) aparece duas vezes no arquivo.",
                template.name,
                template.kind
            );
            for reference in &template.blocks {
                ensure!(
                    keys.contains(reference.key.as_str()),
                    "O modelo \"{}\" cita a instrução \"{}\", que não está no arquivo.",
                    template.name,
                    reference.key
                );
            }
            validate_template(&draft_of(template, |key| stand_in[key].clone())?)
                .with_context(|| format!("Modelo \"{}\"", template.name))?;
            if existing_templates
                .iter()
                .any(|t| t.name.trim() == template.name.trim() && t.kind == template.kind)
            {
                preview
                    .kept
                    .push(format!("Modelo \"{}\" (já existia)", template.name.trim()));
            } else {
                preview
                    .new_templates
                    .push(format!("{} ({})", template.name.trim(), template.kind));
            }
        }
        ensure!(
            all_templates.len() + preview.new_templates.len() <= MAX_TEMPLATES,
            "O arquivo passaria do limite de {MAX_TEMPLATES} modelos."
        );
        // Only the examples that will be added count against the limit:
        // titles already in the template (a second import) or repeated in
        // the file are left out.
        let mut per_template: HashMap<(String, String), HashSet<String>> = HashMap::new();
        // Each example's template, and why it is left out when it is.
        let mut targets: Vec<((String, String), Option<String>)> = Vec::new();
        for example in &bundle.examples {
            let target = example_target(bundle, &existing_templates, example)?;
            validate_example(&example_of(example, PLACEHOLDER_UID))
                .with_context(|| format!("Exemplo \"{}\"", example.title))?;
            let blocked = example_not_imported(&existing_templates, &target, example, &me);
            if blocked.is_none() {
                per_template
                    .entry(target.clone())
                    .or_default()
                    .insert(example.title.trim().to_owned());
            }
            targets.push((target, blocked));
        }
        let mut present_in: HashMap<(String, String), Vec<String>> = HashMap::new();
        for ((name, kind), titles) in &per_template {
            let present: Vec<String> = match existing_templates
                .iter()
                .find(|t| t.name.trim() == name && &t.kind == kind)
            {
                Some(t) => self
                    .examples_of(&t.uid)?
                    .into_iter()
                    .map(|e| e.title.trim().to_owned())
                    .collect(),
                None => Vec::new(),
            };
            let adding = titles.iter().filter(|t| !present.contains(t)).count();
            ensure!(
                present.len() + adding <= MAX_EXAMPLES_PER_TEMPLATE,
                "O modelo \"{name}\" passaria do limite de {MAX_EXAMPLES_PER_TEMPLATE} exemplos."
            );
            present_in.insert((name.clone(), kind.clone()), present);
        }
        let mut seen: HashSet<((String, String), String)> = HashSet::new();
        for (example, target) in bundle.examples.iter().zip(targets) {
            let (target, blocked) = target;
            if let Some(line) = blocked {
                preview.kept.push(line);
                continue;
            }
            let title = example.title.trim().to_owned();
            let known = present_in[&target].contains(&title);
            if known || !seen.insert((target.clone(), title)) {
                preview.kept.push(format!(
                    "Exemplo \"{}\" em \"{}\" (já existia)",
                    example.title.trim(),
                    target.0
                ));
            } else {
                preview.new_examples += 1;
            }
        }
        Ok(preview)
    }

    /// Imports the file. The items are saved one by one, so when a save
    /// fails after others went in, the error says what was already added.
    pub fn import_template_bundle(
        &mut self,
        bundle: &Bundle,
        publish: bool,
    ) -> Result<BundleReport> {
        let mut report = BundleReport::default();
        match self.import_items(bundle, publish, &mut report) {
            Ok(()) => Ok(report),
            Err(error) => {
                let Some(note) = partial_note(
                    report.blocks_created,
                    report.templates_created,
                    report.examples_created,
                ) else {
                    return Err(error);
                };
                let cause = format!("{error:#}");
                Err(anyhow::anyhow!("{}. {note}", cause.trim_end_matches('.')))
            }
        }
    }

    fn import_items(
        &mut self,
        bundle: &Bundle,
        publish: bool,
        report: &mut BundleReport,
    ) -> Result<()> {
        use crate::work_templates::owner_key;
        // Nothing is written before the whole file passes.
        self.preview_template_bundle(bundle)?;
        let me = self.active_actor();
        let mine = owner_key(&me);
        let all_blocks = self.list_blocks()?;
        let all_templates = self.list_templates()?;
        let existing_blocks: Vec<&InstructionBlock> =
            all_blocks.iter().filter(|b| b.visible_to(&me)).collect();
        let existing_templates: Vec<&WorkTemplate> =
            all_templates.iter().filter(|t| t.visible_to(&me)).collect();
        let target_of =
            |example: &BundleExample| example_target(bundle, &existing_templates, example);

        // Drafts. Publishing comes last, so a refusal there never leaves
        // a template without its instructions.
        let mut uids: HashMap<&str, String> = HashMap::new();
        let mut blocks_to_publish: Vec<(String, String, bool)> = Vec::new();
        for block in &bundle.blocks {
            if let Some(found) = existing_blocks
                .iter()
                .find(|b| b.name.trim() == block.name.trim())
            {
                uids.insert(block.key.as_str(), found.uid.clone());
                // Only a draft never published, and only one's own: an edit
                // to a published item, or a colleague's draft, is not this
                // file's to publish.
                if publish && found.status == Status::Draft && owner_key(&found.owner) == mine {
                    blocks_to_publish.push((found.uid.clone(), found.name.trim().to_owned(), true));
                } else {
                    report
                        .kept
                        .push(format!("Instrução \"{}\" (já existia)", block.name.trim()));
                }
                continue;
            }
            let saved = self
                .save_block(
                    &InstructionBlock::new(
                        block.name.trim(),
                        block.topic.trim(),
                        block.text.trim(),
                    ),
                    None,
                )
                .with_context(|| format!("Instrução \"{}\"", block.name))?;
            if publish {
                blocks_to_publish.push((saved.uid.clone(), saved.name.clone(), false));
            }
            uids.insert(block.key.as_str(), saved.uid);
            report.blocks_created += 1;
        }
        let mut templates_to_publish: Vec<(String, String, bool)> = Vec::new();
        for template in &bundle.templates {
            if let Some(found) = existing_templates
                .iter()
                .find(|t| t.name.trim() == template.name.trim() && t.kind == template.kind)
            {
                if publish && found.status == Status::Draft && owner_key(&found.owner) == mine {
                    templates_to_publish.push((
                        found.uid.clone(),
                        found.name.trim().to_owned(),
                        true,
                    ));
                } else {
                    report
                        .kept
                        .push(format!("Modelo \"{}\" (já existia)", template.name.trim()));
                }
                continue;
            }
            let draft = draft_of(template, |key| uids[key].clone())?;
            let saved = self
                .save_template(&draft, None)
                .with_context(|| format!("Modelo \"{}\"", template.name))?;
            if publish {
                templates_to_publish.push((saved.uid.clone(), saved.name.clone(), false));
            }
            report.templates_created += 1;
        }
        let templates = self.list_templates()?;
        for example in &bundle.examples {
            let target = target_of(example)?;
            if let Some(line) = example_not_imported(&existing_templates, &target, example, &me) {
                report.kept.push(line);
                continue;
            }
            let (name, kind) = target;
            let template = templates
                .iter()
                .filter(|t| t.visible_to(&me))
                .find(|t| t.name.trim() == name && t.kind == kind)
                .context("Modelo do exemplo não encontrado depois de salvo.")?;
            if self
                .examples_of(&template.uid)?
                .iter()
                .any(|e| e.title.trim() == example.title.trim())
            {
                report.kept.push(format!(
                    "Exemplo \"{}\" em \"{}\" (já existia)",
                    example.title.trim(),
                    template.name
                ));
                continue;
            }
            self.save_example(&example_of(example, &template.uid), None)
                .with_context(|| format!("Exemplo \"{}\"", example.title))?;
            report.examples_created += 1;
        }

        // Publication: instructions first, then templates whose
        // instructions are all published.
        for (uid, name, existed) in blocks_to_publish {
            self.set_block_status(&uid, Status::Published, None)
                .with_context(|| format!("Publicar a instrução \"{name}\""))?;
            if existed {
                report.published.push(format!("Instrução \"{name}\""));
            }
        }
        for (uid, name, existed) in templates_to_publish {
            let template = self
                .template_by_uid(&uid)?
                .context("Modelo não encontrado.")?;
            match self.unpublished_block(template.draft_version())? {
                Some(reason) => report
                    .kept
                    .push(format!("Modelo \"{name}\" ficou como rascunho: {reason}")),
                None => {
                    self.set_template_status(&uid, Status::Published, None)
                        .with_context(|| format!("Publicar o modelo \"{name}\""))?;
                    if existed {
                        report.published.push(format!("Modelo \"{name}\""));
                    }
                }
            }
        }
        Ok(())
    }

    /// The templates a selection names, in library order. An unknown or
    /// invisible uid is an error.
    fn chosen_templates<'a>(
        all: &'a [WorkTemplate],
        me: &str,
        uids: &[String],
    ) -> Result<Vec<&'a WorkTemplate>> {
        for uid in uids {
            ensure!(
                all.iter().any(|t| &t.uid == uid && t.visible_to(me)),
                "Um dos modelos escolhidos não existe ou não está disponível para você."
            );
        }
        Ok(all
            .iter()
            .filter(|t| t.visible_to(me) && uids.contains(&t.uid))
            .collect())
    }

    /// The instructions the chosen templates cite, in order of first
    /// appearance, each with the name of the first template citing it.
    fn cited_blocks(
        templates: &[&WorkTemplate],
        blocks: &[InstructionBlock],
        me: &str,
    ) -> Result<Vec<(String, String)>> {
        let mut out: Vec<(String, String)> = Vec::new();
        for template in templates {
            match unusable_citation(template, blocks, me) {
                Some(Citation::Hidden(block)) => bail!(
                    "O modelo \"{}\" cita a instrução \"{block}\", que é pessoal de outra pessoa e não pode ser exportada por você. Desmarque esse modelo.",
                    template.name.trim()
                ),
                Some(Citation::Gone) => bail!(
                    "O modelo \"{}\" cita uma instrução que não existe mais. Abra o modelo, corrija e tente de novo.",
                    template.name.trim()
                ),
                None => {}
            }
            let Some(version) = exported_version(template) else {
                continue;
            };
            for reference in &version.block_refs {
                if !out.iter().any(|(uid, _)| uid == &reference.block_uid) {
                    out.push((reference.block_uid.clone(), template.name.trim().to_owned()));
                }
            }
        }
        Ok(out)
    }

    /// The version of each cited instruction the chosen templates export:
    /// the one pinned in the template, else the published one, else the
    /// draft. Two templates reaching different versions of the same
    /// instruction cannot share one file.
    fn cited_versions(
        templates: &[&WorkTemplate],
        blocks: &[InstructionBlock],
    ) -> Result<HashMap<String, u32>> {
        let mut chosen: HashMap<String, (u32, String)> = HashMap::new();
        for template in templates {
            let Some(version) = exported_version(template) else {
                continue;
            };
            for reference in &version.block_refs {
                let Some(block) = blocks.iter().find(|b| b.uid == reference.block_uid) else {
                    continue;
                };
                let Some(used) = exported_block_version(block, reference.version) else {
                    continue;
                };
                let name = template.name.trim().to_owned();
                match chosen.get(&block.uid) {
                    Some((other, by)) if *other != used.version => bail!(
                        "A instrução \"{}\" está em versões diferentes: o modelo \"{by}\" usa a versão {other} e o modelo \"{name}\" usa a versão {}. Publique os modelos de novo, para que usem a mesma versão, ou exporte-os em arquivos separados.",
                        block.name.trim(),
                        used.version
                    ),
                    Some(_) => {}
                    None => {
                        chosen.insert(block.uid.clone(), (used.version, name));
                    }
                }
            }
        }
        Ok(chosen.into_iter().map(|(uid, (v, _))| (uid, v)).collect())
    }

    /// The instructions that go along with the given templates, each with
    /// the name of the first template that cites it ("vai junto com ...").
    pub fn blocks_required_by(&self, template_uids: &[String]) -> Result<Vec<(String, String)>> {
        let me = self.active_actor();
        let all = self.list_templates()?;
        let chosen = Self::chosen_templates(&all, &me, template_uids)?;
        Self::cited_blocks(&chosen, &self.list_blocks()?, &me)
    }

    /// A selection of everything the active person sees, with one line
    /// for each template left out because it cites an instruction that
    /// cannot be exported.
    pub fn selection_of_everything(
        &self,
        include_examples: bool,
    ) -> Result<(BundleSelection, Vec<String>)> {
        let me = self.active_actor();
        let all_blocks = self.list_blocks()?;
        let mut skipped = Vec::new();
        let mut template_uids = Vec::new();
        for template in self.list_templates()?.iter().filter(|t| t.visible_to(&me)) {
            match unusable_citation(template, &all_blocks, &me) {
                None => template_uids.push(template.uid.clone()),
                Some(Citation::Hidden(block)) => skipped.push(format!(
                    "Modelo \"{}\" não exportado: cita a instrução \"{block}\", que é pessoal de outra pessoa.",
                    template.name.trim()
                )),
                Some(Citation::Gone) => skipped.push(format!(
                    "Modelo \"{}\" não exportado: cita uma instrução que não existe mais.",
                    template.name.trim()
                )),
            }
        }
        let selection = BundleSelection {
            template_uids,
            block_uids: all_blocks
                .into_iter()
                .filter(|b| b.visible_to(&me))
                .map(|b| b.uid)
                .collect(),
            include_examples,
        };
        Ok((selection, skipped))
    }

    /// Builds the file content for the chosen items, from what the person
    /// sees: the published version, or the draft when there is none. No
    /// uid, owner, author, date or status goes in.
    pub fn export_template_bundle(&self, selection: &BundleSelection) -> Result<Bundle> {
        use crate::work_templates::ExampleStatus;
        ensure!(
            !selection.template_uids.is_empty() || !selection.block_uids.is_empty(),
            "Nada foi escolhido para exportar."
        );
        let me = self.active_actor();
        let all_templates = self.list_templates()?;
        let all_blocks = self.list_blocks()?;
        let templates = Self::chosen_templates(&all_templates, &me, &selection.template_uids)?;
        for uid in &selection.block_uids {
            ensure!(
                all_blocks
                    .iter()
                    .any(|b| &b.uid == uid && b.visible_to(&me)),
                "Uma das instruções escolhidas não existe ou não está disponível para você."
            );
        }
        // Chosen instructions in library order, then the cited ones.
        let mut block_uids: Vec<String> = all_blocks
            .iter()
            .filter(|b| b.visible_to(&me) && selection.block_uids.contains(&b.uid))
            .map(|b| b.uid.clone())
            .collect();
        for (uid, _) in Self::cited_blocks(&templates, &all_blocks, &me)? {
            if !block_uids.contains(&uid) {
                block_uids.push(uid);
            }
        }
        let pinned = Self::cited_versions(&templates, &all_blocks)?;
        let mut bundle = Bundle {
            format: BUNDLE_FORMAT.to_owned(),
            version: BUNDLE_VERSION,
            blocks: Vec::new(),
            templates: Vec::new(),
            examples: Vec::new(),
        };
        let mut keys: HashMap<&str, String> = HashMap::new();
        for (index, uid) in block_uids.iter().enumerate() {
            let block = all_blocks
                .iter()
                .find(|b| &b.uid == uid)
                .context("Instrução não encontrada.")?;
            // A cited instruction goes with the version its templates use.
            let used = match pinned.get(uid) {
                Some(number) => block.version(*number),
                None => block.published_version().or_else(|| block.draft_version()),
            };
            let text = used
                .with_context(|| format!("A instrução \"{}\" não tem texto.", block.name.trim()))?
                .text
                .trim()
                .to_owned();
            let key = format!("i{}", index + 1);
            keys.insert(uid.as_str(), key.clone());
            bundle.blocks.push(BundleBlock {
                key,
                name: block.name.trim().to_owned(),
                topic: block.topic.trim().to_owned(),
                text,
            });
        }
        for template in &templates {
            let version = exported_version(template).with_context(|| {
                format!("O modelo \"{}\" não tem conteúdo.", template.name.trim())
            })?;
            bundle.templates.push(BundleTemplate {
                name: template.name.trim().to_owned(),
                kind: template.kind.clone(),
                purpose: template.purpose.trim().to_owned(),
                usage_criteria: template.usage_criteria.trim().to_owned(),
                sections: version.sections.clone(),
                variables: version.variables.clone(),
                criteria: version.criteria.clone(),
                references: version.references.clone(),
                blocks: version
                    .block_refs
                    .iter()
                    .map(|r| {
                        let (mode, condition) = match &r.mode {
                            BlockMode::Mandatory => ("obrigatoria", String::new()),
                            BlockMode::Optional => ("opcional", String::new()),
                            BlockMode::Conditional { condition } => {
                                ("condicional", condition.clone())
                            }
                        };
                        BundleRef {
                            key: keys[r.block_uid.as_str()].clone(),
                            mode: mode.to_owned(),
                            condition,
                        }
                    })
                    .collect(),
            });
            if selection.include_examples {
                for example in self.examples_of(&template.uid)? {
                    if example.status == ExampleStatus::Rejected {
                        continue;
                    }
                    bundle.examples.push(BundleExample {
                        template: template.name.trim().to_owned(),
                        kind: Some(template.kind.clone()),
                        title: example.title.trim().to_owned(),
                        demand: example.demand.trim().to_owned(),
                        reply: example.reply.trim().to_owned(),
                        explanation: example.explanation.trim().to_owned(),
                        comments: example.comments.clone(),
                        origin: example.origin.trim().to_owned(),
                    });
                }
            }
        }
        // The importer tells items apart by name (and kind): a file with
        // two alike could not be read back.
        let mut names = HashMap::<&str, usize>::new();
        for block in &bundle.blocks {
            *names.entry(block.name.as_str()).or_default() += 1;
        }
        let mut repeated: Vec<String> = bundle
            .blocks
            .iter()
            .filter(|b| names[b.name.as_str()] > 1)
            .map(|b| format!("instrução \"{}\"", b.name))
            .collect();
        let mut pairs = HashMap::<(&str, &str), usize>::new();
        for template in &bundle.templates {
            *pairs
                .entry((template.name.as_str(), template.kind.as_str()))
                .or_default() += 1;
        }
        repeated.extend(
            bundle
                .templates
                .iter()
                .filter(|t| pairs[&(t.name.as_str(), t.kind.as_str())] > 1)
                .map(|t| format!("modelo \"{}\" ({})", t.name, t.kind)),
        );
        ensure!(
            repeated.is_empty(),
            "Há itens com o mesmo nome entre os escolhidos: {}. Escolha só um de cada ou renomeie antes de exportar.",
            repeated.join("; ")
        );
        Ok(bundle)
    }
}

/// Why a template cannot be exported with its instructions.
enum Citation {
    /// Cites a colleague's personal instruction (its name).
    Hidden(String),
    /// Cites an instruction that no longer exists.
    Gone,
}

/// The first citation of `template` the person cannot export, if any.
fn unusable_citation(
    template: &WorkTemplate,
    blocks: &[InstructionBlock],
    me: &str,
) -> Option<Citation> {
    let version = exported_version(template)?;
    for reference in &version.block_refs {
        match blocks.iter().find(|b| b.uid == reference.block_uid) {
            Some(b) if b.visible_to(me) => {}
            Some(b) => return Some(Citation::Hidden(b.name.trim().to_owned())),
            None => return Some(Citation::Gone),
        }
    }
    None
}

/// The text of `block` a reference exports: the pinned version, or the
/// published one when there is no pin or it is gone, or else the draft.
fn exported_block_version(
    block: &InstructionBlock,
    pin: Option<u32>,
) -> Option<&crate::work_templates::BlockVersion> {
    pin.and_then(|n| block.version(n))
        .or_else(|| block.published_version())
        .or_else(|| block.draft_version())
}

/// What an import already put in, for the error of a save that failed
/// halfway; `None` when nothing went in yet.
pub fn partial_note(blocks: usize, templates: usize, examples: usize) -> Option<String> {
    let mut parts = Vec::new();
    if blocks > 0 {
        parts.push(crate::labels::count(blocks, "instrução", "instruções"));
    }
    if templates > 0 {
        parts.push(crate::labels::count(templates, "modelo", "modelos"));
    }
    if examples > 0 {
        parts.push(crate::labels::count(examples, "exemplo", "exemplos"));
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!(
        "Parte já foi adicionada ({}). Importe o mesmo arquivo de novo para completar; nada será duplicado.",
        parts.join(", ")
    ))
}

/// The line for an example that is not imported because its template is
/// already in the library and is not the importer's own personal one.
fn example_not_imported(
    existing_templates: &[&WorkTemplate],
    target: &(String, String),
    example: &BundleExample,
    me: &str,
) -> Option<String> {
    use crate::work_templates::{owner_key, Scope};
    let found = existing_templates
        .iter()
        .find(|t| t.name.trim() == target.0 && t.kind == target.1)?;
    if found.scope == Scope::Personal && owner_key(&found.owner) == owner_key(me) {
        return None;
    }
    Some(format!(
        "Exemplo \"{}\" (não importado: o modelo \"{}\" já existe e é da equipe)",
        example.title.trim(),
        target.0
    ))
}

/// The version a template is exported from: the published one, or the
/// draft when there is none.
fn exported_version(template: &WorkTemplate) -> Option<&TemplateVersion> {
    template
        .published_version()
        .or_else(|| template.draft_version())
}

/// The template an example names: (name, kind). Names exactly one, among
/// the templates of the file and the visible ones of the library.
fn example_target(
    bundle: &Bundle,
    existing_templates: &[&WorkTemplate],
    example: &BundleExample,
) -> Result<(String, String)> {
    let wanted = |name: &str, kind: &str| {
        name.trim() == example.template.trim() && example.kind.as_deref().is_none_or(|k| k == kind)
    };
    let mut found: Vec<(String, String)> = bundle
        .templates
        .iter()
        .filter(|t| wanted(&t.name, &t.kind))
        .map(|t| (t.name.trim().to_owned(), t.kind.clone()))
        .chain(
            existing_templates
                .iter()
                .filter(|t| wanted(&t.name, &t.kind))
                .map(|t| (t.name.trim().to_owned(), t.kind.clone())),
        )
        .collect();
    found.sort();
    found.dedup();
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => bail!(
            "O exemplo \"{}\" cita o modelo \"{}\", que não está no arquivo nem no acervo.",
            example.title,
            example.template
        ),
        _ => bail!(
            "O exemplo \"{}\" cita \"{}\", nome de mais de um modelo: indique o tipo (\"kind\").",
            example.title,
            example.template
        ),
    }
}

/// The draft a template of the file becomes, with `uid_of` giving each
/// instruction key its uid.
fn draft_of(template: &BundleTemplate, uid_of: impl Fn(&str) -> String) -> Result<WorkTemplate> {
    let mut block_refs = Vec::new();
    for reference in &template.blocks {
        block_refs.push(BlockRef {
            block_uid: uid_of(reference.key.as_str()),
            version: None,
            mode: mode_of(reference)?,
        });
    }
    let all_text = template
        .sections
        .iter()
        .map(|s| s.guidance.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // The variables are the ones the sections use; labels and "required"
    // come from the file when it has them.
    let variables = detect_variables(&all_text)
        .into_iter()
        .map(|found| {
            template
                .variables
                .iter()
                .find(|v| v.name == found.name)
                .cloned()
                .unwrap_or(found)
        })
        .collect();
    Ok(WorkTemplate {
        name: template.name.trim().to_owned(),
        kind: template.kind.clone(),
        purpose: template.purpose.trim().to_owned(),
        usage_criteria: template.usage_criteria.trim().to_owned(),
        versions: vec![TemplateVersion {
            version: 1,
            sections: template.sections.clone(),
            variables,
            block_refs,
            references: template.references.clone(),
            criteria: template.criteria.clone(),
            change_note: "Importado de arquivo.".into(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

/// An example of the file, always "não conferido": only a person approves.
fn example_of(example: &BundleExample, template_uid: &str) -> crate::work_templates::Example {
    crate::work_templates::Example {
        template_uid: template_uid.to_owned(),
        title: example.title.trim().to_owned(),
        demand: example.demand.trim().to_owned(),
        reply: example.reply.trim().to_owned(),
        explanation: example.explanation.trim().to_owned(),
        comments: example.comments.clone(),
        origin: example.origin.trim().to_owned(),
        status: crate::work_templates::ExampleStatus::Unverified,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE: &str = r#"{
      "blocks":[{"key":"tom","name":"Tom institucional","topic":"estilo","text":"Use linguagem formal e objetiva."},
                {"key":"prazo","name":"Sem prazos","topic":"prazos","text":"Não prometa prazos."}],
      "templates":[{"name":"Ofício de resposta","kind":"resposta","purpose":"Responder ofícios.",
        "usage_criteria":"Quando um ofício pede informação.",
        "sections":[{"title":"Referência","guidance":"Cite {{numero_documento}}.","required":true}],
        "criteria":["Responde a todos os pedidos"],
        "references":[{"label":"LGPD","locator":"Lei 13.709/2018","note":""}],
        "blocks":[{"key":"tom","mode":"obrigatoria"},{"key":"prazo","mode":"opcional"}],
        "source_notes":["x.md"]}],
      "examples":[{"template":"Ofício de resposta","title":"Pedido de dados","demand":"Solicito informações.","reply":"Em atenção ao ofício, informo.","explanation":"Curto e direto.","comments":["não reutilizar fatos"],"origin":"Caso típico"}]
    }"#;

    #[test]
    fn a_bundle_becomes_drafts_once_and_can_be_published() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let bundle: Bundle = serde_json::from_str(BUNDLE).unwrap();
        let report = store.import_template_bundle(&bundle, false).unwrap();
        assert_eq!((report.blocks_created, report.templates_created), (2, 1));
        assert_eq!(report.examples_created, 1);
        let template_uid = store.list_templates().unwrap()[0].uid.clone();
        let examples = store.examples_of(&template_uid).unwrap();
        assert_eq!(
            examples[0].status,
            crate::work_templates::ExampleStatus::Unverified
        );
        let template = &store.list_templates().unwrap()[0];
        assert_eq!(template.status, Status::Draft);
        let version = template.versions.last().unwrap();
        assert_eq!(version.variables[0].name, "numero_documento");
        assert_eq!(version.block_refs.len(), 2);
        // Twice: nothing new.
        let again = store.import_template_bundle(&bundle, false).unwrap();
        assert_eq!((again.blocks_created, again.templates_created), (0, 0));
        assert_eq!(again.kept.len(), 4);
        assert_eq!(again.examples_created, 0);
        // The same file with publishing: the drafts already there go live.
        let published = store.import_template_bundle(&bundle, true).unwrap();
        assert_eq!(published.published.len(), 3);
        assert_eq!(store.list_templates().unwrap()[0].status, Status::Published);
        assert_eq!(store.list_blocks().unwrap()[0].status, Status::Published);
        let once_more = store.import_template_bundle(&bundle, true).unwrap();
        assert!(once_more.published.is_empty() && once_more.kept.len() == 4);
        // Published on request, instructions first.
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        store.import_template_bundle(&bundle, true).unwrap();
        let template = &store.list_templates().unwrap()[0];
        assert_eq!(template.status, Status::Published);
        assert_eq!(
            template.published_version().unwrap().block_refs[0].version,
            Some(1)
        );
        // A colleague's personal template with the same name (arrived by
        // sync) is neither reused nor published: this person gets their own.
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let mut theirs = WorkTemplate::new("Ofício de resposta", "resposta", "Texto");
        theirs.owner = "Outra pessoa".into();
        store.save_template(&theirs, None).unwrap();
        let report = store.import_template_bundle(&bundle, true).unwrap();
        assert_eq!(report.templates_created, 1);
        let templates = store.list_templates().unwrap();
        assert_eq!(templates.len(), 2);
        assert!(templates
            .iter()
            .any(|t| t.owner == "Outra pessoa" && t.status == Status::Draft));
        // An example naming two templates of different kinds must say which.
        let ambiguous: Bundle = serde_json::from_str(
            &BUNDLE.replace(
                r#""source_notes":["x.md"]}],"#,
                r#""source_notes":["x.md"]},{"name":"Ofício de resposta","kind":"encaminhamento","sections":[{"title":"T","guidance":"G","required":true}]}],"#,
            ),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let error = store
            .import_template_bundle(&ambiguous, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("indique o tipo"), "{error}");
        assert!(store.list_templates().unwrap().is_empty());
        // A wrong file saves nothing.
        let broken: Bundle =
            serde_json::from_str(&BUNDLE.replace(r#""mode":"obrigatoria""#, r#""mode":"sempre""#))
                .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        assert!(store.import_template_bundle(&broken, false).is_err());
        assert!(store.list_blocks().unwrap().is_empty());
    }

    use crate::work_templates::{Example, ExampleStatus, Scope};

    fn open_store(dir: &std::path::Path, name: &str) -> Store {
        Store::open(&dir.join(name)).unwrap()
    }

    fn block_uid(store: &Store, name: &str) -> String {
        store
            .list_blocks()
            .unwrap()
            .into_iter()
            .find(|b| b.name == name)
            .unwrap()
            .uid
    }

    fn template_uid(store: &Store, name: &str) -> String {
        store
            .list_templates()
            .unwrap()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap()
            .uid
    }

    fn example(template_uid: &str, title: &str, status: ExampleStatus) -> Example {
        Example {
            template_uid: template_uid.to_owned(),
            title: title.to_owned(),
            demand: "Solicito informações ao Servidor A.".into(),
            reply: "Em atenção ao ORG-1, informo.".into(),
            explanation: "Curto e direto.".into(),
            comments: vec!["não reutilizar fatos".into()],
            origin: "Caso típico".into(),
            status,
            ..Default::default()
        }
    }

    /// Four instructions, two templates (one published, one draft) and
    /// three examples, all generic.
    fn library(store: &mut Store) {
        let mut uids = HashMap::new();
        for (name, topic, text) in [
            ("Tom institucional", "estilo", "Use linguagem formal."),
            ("Sem prazos", "prazos", "Não prometa prazos."),
            ("Fechamento", "", "Encerre com cordialidade."),
            ("Avulsa", "outros", "Instrução que nenhum modelo cita."),
        ] {
            let saved = store
                .save_block(&InstructionBlock::new(name, topic, text), None)
                .unwrap();
            uids.insert(name, saved.uid);
        }
        for name in ["Tom institucional", "Sem prazos", "Fechamento"] {
            store
                .set_block_status(&uids[name], Status::Published, None)
                .unwrap();
        }
        let mut first = WorkTemplate::new("Resposta ao ORG-1", "resposta", "");
        first.purpose = "Responder ofícios do ORG-1.".into();
        first.usage_criteria = "Quando o Servidor A pede informação.".into();
        let version = &mut first.versions[0];
        version.sections = vec![
            Section {
                title: "Referência".into(),
                guidance: "Cite {{numero_documento}} de {{orgao}}.".into(),
                required: true,
            },
            Section {
                title: "Corpo".into(),
                guidance: "Informe em {{prazo_dias}} dias.".into(),
                required: false,
            },
        ];
        version.variables = vec![
            Variable {
                name: "numero_documento".into(),
                label: "Número do documento".into(),
                required: true,
            },
            Variable {
                name: "orgao".into(),
                label: "Órgão de origem".into(),
                required: false,
            },
            Variable {
                name: "prazo_dias".into(),
                label: "Prazo (dias)".into(),
                required: true,
            },
        ];
        version.criteria = vec!["Responde a todos os pedidos".into()];
        version.references = vec![Reference {
            label: "Norma A".into(),
            locator: "Art. 1".into(),
            note: "nota".into(),
        }];
        version.block_refs = vec![
            BlockRef {
                block_uid: uids["Tom institucional"].clone(),
                version: None,
                mode: BlockMode::Mandatory,
            },
            BlockRef {
                block_uid: uids["Sem prazos"].clone(),
                version: None,
                mode: BlockMode::Optional,
            },
            BlockRef {
                block_uid: uids["Fechamento"].clone(),
                version: None,
                mode: BlockMode::Conditional {
                    condition: "quando houver despedida".into(),
                },
            },
        ];
        let first = store.save_template(&first, None).unwrap();
        let mut second = WorkTemplate::new(
            "Encaminhamento ao Servidor B",
            "encaminhamento",
            "Texto {{assunto}}.",
        );
        second.versions[0].block_refs = vec![BlockRef {
            block_uid: uids["Tom institucional"].clone(),
            version: None,
            mode: BlockMode::Mandatory,
        }];
        let second = store.save_template(&second, None).unwrap();
        store
            .set_template_status(&second.uid, Status::Published, None)
            .unwrap();
        for (title, status) in [
            ("Pedido de dados", ExampleStatus::Unverified),
            ("Pedido aprovado", ExampleStatus::Approved),
            ("Pedido ruim", ExampleStatus::Rejected),
        ] {
            store
                .save_example(&example(&first.uid, title, status), None)
                .unwrap();
        }
    }

    fn write_text(dir: &std::path::Path, text: &str) -> std::path::PathBuf {
        let path = dir.join("entrada.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn export_then_import_keeps_everything() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = open_store(dir.path(), "a.sqlite3");
        library(&mut source);
        let selection = source.selection_of_everything(true).unwrap().0;
        let bundle = source.export_template_bundle(&selection).unwrap();
        assert_eq!(bundle.blocks.len(), 4);
        // Library order for the chosen ones, no key repeated.
        let keys: Vec<&str> = bundle.blocks.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["i1", "i2", "i3", "i4"]);
        // The rejected example stays out.
        assert_eq!(bundle.examples.len(), 2);
        let file = dir.path().join("modelos.json");
        write_bundle(&file, &bundle).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(bytes[0], b'{');
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("\"uid\"") && !text.contains("\"owner\""));
        assert!(!dir.path().join(".modelos.json.tmp").exists());
        let read = read_bundle(&file).unwrap();
        assert_eq!((read.format.as_str(), read.version), (BUNDLE_FORMAT, 1));

        let mut target = open_store(dir.path(), "b.sqlite3");
        let report = target.import_template_bundle(&read, false).unwrap();
        assert_eq!(
            (
                report.blocks_created,
                report.templates_created,
                report.examples_created
            ),
            (4, 2, 2)
        );
        let source_blocks = source.list_blocks().unwrap();
        let target_blocks = target.list_blocks().unwrap();
        let mode_of_ref = |blocks: &[InstructionBlock], r: &BlockRef| {
            let block = blocks.iter().find(|b| b.uid == r.block_uid).unwrap();
            (block.name.clone(), r.mode.clone())
        };
        for original in &source_blocks {
            let copy = target_blocks
                .iter()
                .find(|b| b.name == original.name)
                .unwrap();
            let from = original
                .published_version()
                .or_else(|| original.draft_version())
                .unwrap();
            assert_eq!(copy.topic, original.topic);
            assert_eq!(copy.draft_version().unwrap().text, from.text);
            assert_eq!((copy.status, copy.scope), (Status::Draft, Scope::Personal));
        }
        for original in source.list_templates().unwrap() {
            let copy = target
                .list_templates()
                .unwrap()
                .into_iter()
                .find(|t| t.name == original.name && t.kind == original.kind)
                .unwrap();
            assert_eq!((copy.status, copy.scope), (Status::Draft, Scope::Personal));
            assert_eq!(copy.purpose, original.purpose);
            assert_eq!(copy.usage_criteria, original.usage_criteria);
            let from = original
                .published_version()
                .or_else(|| original.draft_version())
                .unwrap();
            let to = copy.draft_version().unwrap();
            assert_eq!(to.sections, from.sections);
            assert_eq!(to.variables, from.variables);
            assert_eq!(to.criteria, from.criteria);
            assert_eq!(to.references, from.references);
            let wanted: Vec<_> = from
                .block_refs
                .iter()
                .map(|r| mode_of_ref(&source_blocks, r))
                .collect();
            let got: Vec<_> = to
                .block_refs
                .iter()
                .map(|r| mode_of_ref(&target_blocks, r))
                .collect();
            assert_eq!(got, wanted);
        }
        let titles: Vec<String> = target
            .examples_of(&template_uid(&target, "Resposta ao ORG-1"))
            .unwrap()
            .into_iter()
            .map(|e| {
                assert_eq!(e.status, ExampleStatus::Unverified);
                assert_eq!(e.comments, ["não reutilizar fatos"]);
                e.title
            })
            .collect();
        assert_eq!(titles.len(), 2);
        assert!(!titles.contains(&"Pedido ruim".to_owned()));
        // The same file again creates nothing.
        let again = target.import_template_bundle(&read, false).unwrap();
        assert_eq!(
            (
                again.blocks_created,
                again.templates_created,
                again.examples_created
            ),
            (0, 0, 0)
        );
        assert_eq!(again.kept.len(), 8);
    }

    #[test]
    fn a_partial_selection_carries_only_what_it_cites() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(dir.path(), "a.sqlite3");
        library(&mut store);
        let first = template_uid(&store, "Resposta ao ORG-1");
        let second = template_uid(&store, "Encaminhamento ao Servidor B");
        let bundle = store
            .export_template_bundle(&BundleSelection {
                template_uids: vec![second.clone()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(bundle.templates.len(), 1);
        assert_eq!(bundle.blocks.len(), 1);
        assert_eq!(bundle.blocks[0].name, "Tom institucional");
        assert!(bundle.examples.is_empty());
        // Explicit instructions come first, the cited ones after.
        let bundle = store
            .export_template_bundle(&BundleSelection {
                template_uids: vec![first.clone()],
                block_uids: vec![block_uid(&store, "Avulsa"), block_uid(&store, "Fechamento")],
                include_examples: false,
            })
            .unwrap();
        let names: Vec<&str> = bundle.blocks.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            ["Fechamento", "Avulsa", "Tom institucional", "Sem prazos"]
        );
        let required = store.blocks_required_by(&[first, second]).unwrap();
        assert_eq!(required.len(), 3);
        assert_eq!(required[0].1, "Resposta ao ORG-1");
        // Only instructions chosen: no template.
        let only = store
            .export_template_bundle(&BundleSelection {
                block_uids: vec![block_uid(&store, "Avulsa")],
                ..Default::default()
            })
            .unwrap();
        assert_eq!((only.blocks.len(), only.templates.len()), (1, 0));
    }

    #[test]
    fn examples_go_only_when_asked_and_rejected_never() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(dir.path(), "a.sqlite3");
        library(&mut store);
        let mut selection = BundleSelection {
            template_uids: vec![template_uid(&store, "Resposta ao ORG-1")],
            ..Default::default()
        };
        assert!(store
            .export_template_bundle(&selection)
            .unwrap()
            .examples
            .is_empty());
        selection.include_examples = true;
        let bundle = store.export_template_bundle(&selection).unwrap();
        let mut titles: Vec<&str> = bundle.examples.iter().map(|e| e.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["Pedido aprovado", "Pedido de dados"]);
        assert!(bundle
            .examples
            .iter()
            .all(|e| e.kind.as_deref() == Some("resposta")));
    }

    #[test]
    fn unusable_selections_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(dir.path(), "a.sqlite3");
        library(&mut store);
        let error = store
            .export_template_bundle(&BundleSelection::default())
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Nada foi escolhido para exportar.");
        assert!(store
            .export_template_bundle(&BundleSelection {
                template_uids: vec!["0".repeat(32)],
                ..Default::default()
            })
            .is_err());
        // A colleague's personal template is not offered.
        let mut theirs = WorkTemplate::new("Modelo do Servidor C", "resposta", "Texto");
        theirs.owner = "Outra pessoa".into();
        let theirs = store.save_template(&theirs, None).unwrap();
        assert!(store
            .export_template_bundle(&BundleSelection {
                template_uids: vec![theirs.uid],
                ..Default::default()
            })
            .is_err());
        // Two templates with the same name and kind.
        let twin = WorkTemplate::new("Resposta ao ORG-1", "resposta", "Outro");
        let twin = store.save_template(&twin, None).unwrap();
        let error = store
            .export_template_bundle(&BundleSelection {
                template_uids: vec![
                    template_uid(&store, "Encaminhamento ao Servidor B"),
                    twin.uid,
                ]
                .into_iter()
                .chain(
                    store
                        .list_templates()
                        .unwrap()
                        .into_iter()
                        .filter(|t| t.name == "Resposta ao ORG-1")
                        .map(|t| t.uid),
                )
                .collect(),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("Resposta ao ORG-1"), "{error}");
        // Two instructions with the same name.
        let twin = store
            .save_block(&InstructionBlock::new("Avulsa ", "x", "Outro texto."), None)
            .unwrap();
        let error = store
            .export_template_bundle(&BundleSelection {
                block_uids: vec![block_uid(&store, "Avulsa"), twin.uid],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("Avulsa"), "{error}");
    }

    #[test]
    fn version_and_format_are_checked_when_reading() {
        let dir = tempfile::tempdir().unwrap();
        let future = write_text(
            dir.path(),
            r#"{"format":"ponte-modelos","version":2,"blocks":[]}"#,
        );
        let error = read_bundle(&future).unwrap_err().to_string();
        assert!(error.contains("Atualize o Ponte"), "{error}");
        let foreign = write_text(dir.path(), r#"{"format":"outra-coisa","version":1}"#);
        assert!(read_bundle(&foreign).is_err());
        let old = write_text(dir.path(), BUNDLE);
        let bundle = read_bundle(&old).unwrap();
        assert_eq!(bundle.version, 1);
        assert_eq!(bundle.blocks.len(), 2);
    }

    #[test]
    fn variables_of_the_file_give_labels_but_not_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(dir.path(), "a.sqlite3");
        let bundle: Bundle = serde_json::from_str(
            r#"{"templates":[{"name":"Modelo A","kind":"resposta",
              "sections":[{"title":"T","guidance":"Cite {{numero}} e {{outro}}.","required":true}],
              "variables":[{"name":"numero","label":"Número do ORG-1","required":false},
                           {"name":"sobrando","label":"Não usada","required":true}]}]}"#,
        )
        .unwrap();
        store.import_template_bundle(&bundle, false).unwrap();
        let template = &store.list_templates().unwrap()[0];
        let variables = &template.draft_version().unwrap().variables;
        assert_eq!(variables.len(), 2);
        assert_eq!(variables[0].label, "Número do ORG-1");
        assert!(!variables[0].required);
        assert_eq!(variables[1].label, "outro");
        assert!(variables[1].required);
    }

    #[test]
    fn preview_writes_nothing_and_matches_the_import() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = open_store(dir.path(), "a.sqlite3");
        library(&mut source);
        let selection = source.selection_of_everything(true).unwrap().0;
        let bundle = source.export_template_bundle(&selection).unwrap();
        let mut target = open_store(dir.path(), "b.sqlite3");
        let preview = target.preview_template_bundle(&bundle).unwrap();
        assert!(target.list_blocks().unwrap().is_empty());
        assert!(target.list_templates().unwrap().is_empty());
        assert!(preview.kept.is_empty());
        let report = target.import_template_bundle(&bundle, false).unwrap();
        assert_eq!(preview.new_blocks.len(), report.blocks_created);
        assert_eq!(preview.new_templates.len(), report.templates_created);
        assert_eq!(preview.new_examples, report.examples_created);
        assert!(preview
            .new_templates
            .contains(&"Resposta ao ORG-1 (resposta)".to_owned()));
        let after = target.preview_template_bundle(&bundle).unwrap();
        assert!(after.new_blocks.is_empty() && after.new_templates.is_empty());
        assert_eq!(after.new_examples, 0);
        assert_eq!(after.kept.len(), 8);
        assert!(after
            .kept
            .contains(&"Modelo \"Resposta ao ORG-1\" (já existia)".to_owned()));
    }

    #[test]
    fn an_oversize_bundle_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = Bundle {
            format: BUNDLE_FORMAT.into(),
            version: BUNDLE_VERSION,
            blocks: vec![BundleBlock {
                key: "i1".into(),
                name: "Grande".into(),
                topic: String::new(),
                text: "x".repeat(MAX_BUNDLE_BYTES as usize + 1),
            }],
            templates: Vec::new(),
            examples: Vec::new(),
        };
        let file = dir.path().join("grande.json");
        let error = write_bundle(&file, &bundle).unwrap_err().to_string();
        assert!(error.contains("Exporte menos itens"), "{error}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    fn examples_only(template: &str) -> Bundle {
        serde_json::from_str(&format!(
            r#"{{"examples":[{{"template":"{template}","title":"Pedido novo","demand":"Solicito informações ao Servidor A.","reply":"Em atenção ao ORG-1, informo."}}]}}"#
        ))
        .unwrap()
    }

    #[test]
    fn examples_enter_only_the_importers_own_personal_templates() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(dir.path(), "a.sqlite3");
        let mut shared = WorkTemplate::new("Modelo da equipe", "resposta", "Texto");
        shared.scope = Scope::Shared;
        let shared = store.save_template(&shared, None).unwrap();
        let own = store
            .save_template(&WorkTemplate::new("Modelo meu", "resposta", "Texto"), None)
            .unwrap();

        let bundle = examples_only("Modelo da equipe");
        let preview = store.preview_template_bundle(&bundle).unwrap();
        assert_eq!(preview.new_examples, 0);
        assert_eq!(preview.kept.len(), 1);
        assert!(
            preview.kept[0].contains("não importado")
                && preview.kept[0].contains("\"Modelo da equipe\"")
                && preview.kept[0].contains("é da equipe"),
            "{:?}",
            preview.kept
        );
        let report = store.import_template_bundle(&bundle, false).unwrap();
        assert_eq!(report.examples_created, 0);
        assert_eq!(report.kept, preview.kept);
        assert!(store.examples_of(&shared.uid).unwrap().is_empty());

        let bundle = examples_only("Modelo meu");
        let preview = store.preview_template_bundle(&bundle).unwrap();
        assert_eq!((preview.new_examples, preview.kept.len()), (1, 0));
        let report = store.import_template_bundle(&bundle, false).unwrap();
        assert_eq!(report.examples_created, 1);
        assert_eq!(store.examples_of(&own.uid).unwrap().len(), 1);
    }

    #[test]
    fn a_published_template_exports_the_instruction_version_it_pins() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(dir.path(), "a.sqlite3");
        library(&mut store);
        // The instruction is revised and published again; the published
        // template still uses the first version.
        let tom = store
            .block_by_uid(&block_uid(&store, "Tom institucional"))
            .unwrap()
            .unwrap();
        let mut revised = tom.clone();
        revised.versions.last_mut().unwrap().text = "Use linguagem coloquial.".into();
        store.save_block(&revised, None).unwrap();
        store
            .set_block_status(&tom.uid, Status::Published, None)
            .unwrap();
        let first = template_uid(&store, "Resposta ao ORG-1");
        let second = template_uid(&store, "Encaminhamento ao Servidor B");
        let bundle = store
            .export_template_bundle(&BundleSelection {
                template_uids: vec![second.clone()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(bundle.blocks[0].text, "Use linguagem formal.");
        // Chosen on its own, the published version goes.
        let only = store
            .export_template_bundle(&BundleSelection {
                block_uids: vec![tom.uid.clone()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(only.blocks[0].text, "Use linguagem coloquial.");
        // The draft template follows the new version: the two cannot share
        // one file.
        let error = store
            .export_template_bundle(&BundleSelection {
                template_uids: vec![first, second],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        for part in [
            "Tom institucional",
            "Resposta ao ORG-1",
            "Encaminhamento ao Servidor B",
            "Publique os modelos de novo",
        ] {
            assert!(error.contains(part), "{error}");
        }
    }

    /// A template of this person citing `uid`.
    fn citing(store: &mut Store, name: &str, uid: &str) -> WorkTemplate {
        let mut template = WorkTemplate::new(name, "resposta", "Texto");
        template.versions[0].block_refs = vec![BlockRef {
            block_uid: uid.to_owned(),
            version: None,
            mode: BlockMode::Mandatory,
        }];
        store.save_template(&template, None).unwrap()
    }

    #[test]
    fn citing_an_instruction_the_person_cannot_use_is_explained_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open_store(dir.path(), "a.sqlite3");
        library(&mut store);
        let mut theirs = InstructionBlock::new("Instrução do Servidor C", "x", "Texto.");
        theirs.owner = "Outra pessoa".into();
        let theirs = store.save_block(&theirs, None).unwrap();
        let hidden = citing(&mut store, "Modelo com instrução alheia", &theirs.uid);
        let gone = citing(&mut store, "Modelo com instrução perdida", &"1".repeat(32));
        let select = |uid: &str| BundleSelection {
            template_uids: vec![uid.to_owned()],
            ..Default::default()
        };
        let error = store
            .export_template_bundle(&select(&hidden.uid))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "O modelo \"Modelo com instrução alheia\" cita a instrução \"Instrução do Servidor C\", que é pessoal de outra pessoa e não pode ser exportada por você. Desmarque esse modelo."
        );
        let error = store
            .export_template_bundle(&select(&gone.uid))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "O modelo \"Modelo com instrução perdida\" cita uma instrução que não existe mais. Abra o modelo, corrija e tente de novo."
        );
        assert!(store
            .blocks_required_by(std::slice::from_ref(&gone.uid))
            .is_err());
        // Everything is exported except those two.
        let (selection, skipped) = store.selection_of_everything(false).unwrap();
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(skipped[0].contains("Modelo com instrução alheia"));
        assert!(skipped[0].contains("Instrução do Servidor C"));
        assert!(skipped[1].contains("Modelo com instrução perdida"));
        assert!(skipped.iter().all(|line| !line.contains(&"1".repeat(32))));
        assert_eq!(selection.template_uids.len(), 2);
        let bundle = store.export_template_bundle(&selection).unwrap();
        assert_eq!(bundle.templates.len(), 2);
    }

    #[test]
    fn a_failed_import_says_what_was_already_added() {
        assert_eq!(partial_note(0, 0, 0), None);
        let note = partial_note(2, 1, 0).unwrap();
        assert_eq!(
            note,
            "Parte já foi adicionada (2 instruções, 1 modelo). Importe o mesmo arquivo de novo para completar; nada será duplicado."
        );
        assert!(partial_note(1, 0, 3)
            .unwrap()
            .contains("(1 instrução, 3 exemplos)"));
    }

    #[test]
    fn a_file_that_is_not_a_bundle_is_refused_and_a_bom_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        for text in ["{}", r#"{"nome":"Servidor A","itens":[1,2]}"#] {
            let path = write_text(dir.path(), text);
            let error = read_bundle(&path).unwrap_err().to_string();
            assert_eq!(error, "O arquivo não é um conjunto de modelos do Ponte.");
        }
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(BUNDLE.as_bytes());
        let path = dir.path().join("com-bom.json");
        std::fs::write(&path, bytes).unwrap();
        let bundle = read_bundle(&path).unwrap();
        assert_eq!(bundle.blocks.len(), 2);
        let path = write_text(dir.path(), "{\n  \"blocks\": [");
        let error = read_bundle(&path).unwrap_err().to_string();
        assert!(
            error.starts_with("O arquivo não é um conjunto de modelos válido (linha 2, coluna"),
            "{error}"
        );
        assert!(!error.contains("EOF"), "{error}");
    }
}
