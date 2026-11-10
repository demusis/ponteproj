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
    TemplateVersion, WorkTemplate,
};
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// Largest file accepted.
pub const MAX_BUNDLE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct Bundle {
    #[serde(default)]
    pub blocks: Vec<BundleBlock>,
    #[serde(default)]
    pub templates: Vec<BundleTemplate>,
    /// Past cases for the templates, named by template. They always arrive
    /// as "não conferido": only a person approves an example.
    #[serde(default)]
    pub examples: Vec<BundleExample>,
}

#[derive(Debug, Deserialize)]
pub struct BundleExample {
    /// Name of the template (in this file or already in the library).
    pub template: String,
    /// Its kind, needed only when two templates share the name.
    #[serde(default)]
    pub kind: Option<String>,
    pub title: String,
    pub demand: String,
    pub reply: String,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub comments: Vec<String>,
    #[serde(default)]
    pub origin: String,
}

#[derive(Debug, Deserialize)]
pub struct BundleBlock {
    /// How templates in the same file refer to it.
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub topic: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct BundleTemplate {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub usage_criteria: String,
    pub sections: Vec<Section>,
    #[serde(default)]
    pub criteria: Vec<String>,
    #[serde(default)]
    pub references: Vec<Reference>,
    #[serde(default)]
    pub blocks: Vec<BundleRef>,
}

#[derive(Debug, Deserialize)]
pub struct BundleRef {
    pub key: String,
    /// obrigatoria | opcional | condicional
    pub mode: String,
    #[serde(default)]
    pub condition: String,
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
    serde_json::from_slice(&bytes).context("O arquivo não é um conjunto de modelos válido.")
}

impl Store {
    /// The first instruction of `version` without a published version (a
    /// template cannot be published before its instructions).
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
                Some(b) => return Ok(Some(b.name.clone())),
                None => return Ok(Some(reference.block_uid.clone())),
            }
        }
        Ok(None)
    }

    pub fn import_template_bundle(
        &mut self,
        bundle: &Bundle,
        publish: bool,
    ) -> Result<BundleReport> {
        use crate::work_templates::{
            owner_key, validate_block, validate_example, validate_template, MAX_BLOCKS,
            MAX_EXAMPLES_PER_TEMPLATE, MAX_TEMPLATES,
        };
        // Stands in for uids not created yet while the file is checked.
        const PLACEHOLDER_UID: &str = "00000000000000000000000000000000";
        let me = self.active_actor();
        let mine = owner_key(&me);
        // Only what this person sees is reused: a colleague's personal item
        // that arrived by sync is neither matched nor published.
        let all_blocks = self.list_blocks()?;
        let all_templates = self.list_templates()?;
        let existing_blocks: Vec<&InstructionBlock> =
            all_blocks.iter().filter(|b| b.visible_to(&me)).collect();
        let existing_templates: Vec<&WorkTemplate> =
            all_templates.iter().filter(|t| t.visible_to(&me)).collect();

        // 1. The whole file is checked before anything is written, with the
        // same rules the saving applies, so an error never leaves half a set.
        let mut keys = HashSet::new();
        let mut new_blocks = 0usize;
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
            if !existing_blocks
                .iter()
                .any(|b| b.name.trim() == block.name.trim())
            {
                new_blocks += 1;
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
            all_blocks.len() + new_blocks <= MAX_BLOCKS,
            "O arquivo passaria do limite de {MAX_BLOCKS} instruções."
        );
        let mut pairs = HashSet::new();
        let mut new_templates = 0usize;
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
            if !existing_templates
                .iter()
                .any(|t| t.name.trim() == template.name.trim() && t.kind == template.kind)
            {
                new_templates += 1;
            }
        }
        ensure!(
            all_templates.len() + new_templates <= MAX_TEMPLATES,
            "O arquivo passaria do limite de {MAX_TEMPLATES} modelos."
        );
        // Each example names exactly one template (name, and kind when two
        // templates share the name).
        let target_of = |example: &BundleExample| -> Result<(String, String)> {
            let wanted = |name: &str, kind: &str| {
                name.trim() == example.template.trim()
                    && example.kind.as_deref().is_none_or(|k| k == kind)
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
        };
        // Only the examples that will be added count against the limit:
        // titles already in the template (a second import) or repeated in
        // the file are left out.
        let mut per_template: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for example in &bundle.examples {
            let target = target_of(example)?;
            validate_example(&example_of(example, PLACEHOLDER_UID))
                .with_context(|| format!("Exemplo \"{}\"", example.title))?;
            per_template
                .entry(target)
                .or_default()
                .insert(example.title.trim().to_owned());
        }
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
        }

        // 2. Drafts. Publishing comes last, so a refusal there never leaves
        // a template without its instructions.
        let mut report = BundleReport::default();
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
            let (name, kind) = target_of(example)?;
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

        // 3. Publication: instructions first, then templates whose
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
                Some(block) => report.kept.push(format!(
                    "Modelo \"{name}\" ficou como rascunho: a instrução \"{block}\" não está publicada"
                )),
                None => {
                    self.set_template_status(&uid, Status::Published, None)
                        .with_context(|| format!("Publicar o modelo \"{name}\""))?;
                    if existed {
                        report.published.push(format!("Modelo \"{name}\""));
                    }
                }
            }
        }
        Ok(report)
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
    Ok(WorkTemplate {
        name: template.name.trim().to_owned(),
        kind: template.kind.clone(),
        purpose: template.purpose.trim().to_owned(),
        usage_criteria: template.usage_criteria.trim().to_owned(),
        versions: vec![TemplateVersion {
            version: 1,
            sections: template.sections.clone(),
            variables: detect_variables(&all_text),
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
}
