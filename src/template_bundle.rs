//! Bringing work templates and instructions from a JSON file into the
//! library (`ponteproj --import-templates ARQUIVO [--publicar]`): a set
//! prepared elsewhere, or handed over by another team. The whole file is
//! checked before anything is saved; everything arrives personal and as a
//! draft unless publishing is asked for; an instruction with the same name
//! is reused and a template with the same name and kind is left alone, so
//! importing the same file twice changes nothing.
use crate::storage::Store;
use crate::work_templates::{
    detect_variables, BlockMode, BlockRef, InstructionBlock, Reference, Section, Status,
    TemplateVersion, WorkTemplate,
};
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// Largest file accepted.
pub const MAX_BUNDLE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct Bundle {
    #[serde(default)]
    pub blocks: Vec<BundleBlock>,
    #[serde(default)]
    pub templates: Vec<BundleTemplate>,
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
    /// Items already in the library, left as they were.
    pub kept: Vec<String>,
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
        "O arquivo passa de {} MB.",
        MAX_BUNDLE_BYTES / (1024 * 1024)
    );
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes).context("O arquivo não é um conjunto de modelos válido (JSON).")
}

impl Store {
    pub fn import_template_bundle(
        &mut self,
        bundle: &Bundle,
        publish: bool,
    ) -> Result<BundleReport> {
        // Everything checked first: keys, modes and kinds.
        let mut seen = std::collections::HashSet::new();
        for block in &bundle.blocks {
            ensure!(
                seen.insert(block.key.as_str()),
                "A chave \"{}\" aparece em duas instruções.",
                block.key
            );
            ensure!(
                !block.name.trim().is_empty() && !block.text.trim().is_empty(),
                "Instrução \"{}\" sem nome ou sem texto.",
                block.key
            );
        }
        for template in &bundle.templates {
            ensure!(
                ["resposta", "encaminhamento", "complementacao"].contains(&template.kind.as_str()),
                "Modelo \"{}\": tipo \"{}\" inválido (resposta, encaminhamento ou complementacao).",
                template.name,
                template.kind
            );
            for reference in &template.blocks {
                ensure!(
                    seen.contains(reference.key.as_str()),
                    "O modelo \"{}\" cita a instrução \"{}\", que não está no arquivo.",
                    template.name,
                    reference.key
                );
                mode_of(reference)?;
            }
        }

        let mut report = BundleReport::default();
        let mut uids: HashMap<&str, String> = HashMap::new();
        let existing_blocks = self.list_blocks()?;
        for block in &bundle.blocks {
            if let Some(found) = existing_blocks
                .iter()
                .find(|b| b.name.trim() == block.name.trim())
            {
                uids.insert(block.key.as_str(), found.uid.clone());
                report
                    .kept
                    .push(format!("Instrução \"{}\" (já existia)", block.name.trim()));
                continue;
            }
            let mut saved = self
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
                saved = self.set_block_status(&saved.uid, Status::Published, None)?;
            }
            uids.insert(block.key.as_str(), saved.uid);
            report.blocks_created += 1;
        }

        let existing = self.list_templates()?;
        for template in &bundle.templates {
            if existing
                .iter()
                .any(|t| t.name.trim() == template.name.trim() && t.kind == template.kind)
            {
                report
                    .kept
                    .push(format!("Modelo \"{}\" (já existia)", template.name.trim()));
                continue;
            }
            let mut block_refs = Vec::new();
            for reference in &template.blocks {
                block_refs.push(BlockRef {
                    block_uid: uids[reference.key.as_str()].clone(),
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
            let draft = WorkTemplate {
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
            };
            let saved = self
                .save_template(&draft, None)
                .with_context(|| format!("Modelo \"{}\"", template.name))?;
            if publish {
                self.set_template_status(&saved.uid, Status::Published, None)
                    .with_context(|| format!("Publicar o modelo \"{}\"", template.name))?;
            }
            report.templates_created += 1;
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE: &str = r#"{
      "blocks":[{"key":"tom","name":"Tom institucional","topic":"estilo","text":"Use linguagem formal e objetiva."}],
      "templates":[{"name":"Ofício de resposta","kind":"resposta","purpose":"Responder ofícios.",
        "usage_criteria":"Quando um ofício pede informação.",
        "sections":[{"title":"Referência","guidance":"Cite {{numero_documento}}.","required":true}],
        "criteria":["Responde a todos os pedidos"],
        "references":[{"label":"LGPD","locator":"Lei 13.709/2018","note":""}],
        "blocks":[{"key":"tom","mode":"obrigatoria"}],
        "source_notes":["x.md"]}]
    }"#;

    #[test]
    fn a_bundle_becomes_drafts_once_and_can_be_published() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let bundle: Bundle = serde_json::from_str(BUNDLE).unwrap();
        let report = store.import_template_bundle(&bundle, false).unwrap();
        assert_eq!((report.blocks_created, report.templates_created), (1, 1));
        let template = &store.list_templates().unwrap()[0];
        assert_eq!(template.status, Status::Draft);
        let version = template.versions.last().unwrap();
        assert_eq!(version.variables[0].name, "numero_documento");
        assert_eq!(version.block_refs.len(), 1);
        // Twice: nothing new.
        let again = store.import_template_bundle(&bundle, false).unwrap();
        assert_eq!((again.blocks_created, again.templates_created), (0, 0));
        assert_eq!(again.kept.len(), 2);
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
