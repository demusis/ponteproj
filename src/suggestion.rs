//! Lexical suggestions for the draft: which work template fits the open
//! document, and which approved examples of that template resemble the
//! case. The same TF-IDF and cosine used by "documentos semelhantes", so
//! the suggestion needs no index, no model and nothing outside the
//! library. It is only a suggestion: the person picks, and the pickers
//! never hide anything.
//!
//! Lexical means literal words: a paraphrase is not recognized. Whether
//! that is enough is what the evaluation protocol of the proposal measures
//! before embeddings are considered.
use crate::{
    model::Capture,
    storage::{lexical_terms, Store},
    work_templates::{Example, ExampleStatus, WorkTemplate},
};
use anyhow::Result;
use std::collections::BTreeMap;

/// Below this cosine the match is noise, not a suggestion.
pub const MIN_TEMPLATE_SCORE: f64 = 0.08;
/// How much of a long document text the query uses.
const MAX_QUERY_BYTES: usize = 64 * 1024;

/// A candidate with its similarity to the query (0 to 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Ranked<T> {
    pub item: T,
    pub score: f64,
}

impl<T> Ranked<T> {
    /// The score as a whole percentage for the screen.
    pub fn percent(&self) -> u8 {
        (self.score * 100.0).round().clamp(0.0, 100.0) as u8
    }
}

/// Candidates ordered by similarity to `query`; those with nothing in
/// common come last, in the order given (so a tie keeps the caller's
/// preference, newest first for example).
pub fn rank<T>(query: &str, candidates: Vec<(T, String)>) -> Vec<Ranked<T>> {
    let query_terms = lexical_terms(query);
    let term_sets: Vec<BTreeMap<String, usize>> = candidates
        .iter()
        .map(|(_, text)| lexical_terms(text))
        .collect();
    let mut frequency = BTreeMap::<String, usize>::new();
    for terms in term_sets.iter().chain(std::iter::once(&query_terms)) {
        for term in terms.keys() {
            *frequency.entry(term.clone()).or_default() += 1;
        }
    }
    let total = candidates.len() as f64 + 1.0;
    let weigh = |terms: &BTreeMap<String, usize>| -> BTreeMap<String, f64> {
        terms
            .iter()
            .map(|(word, count)| {
                // The same smoothed IDF as "documentos semelhantes".
                let document_frequency = *frequency.get(word).unwrap_or(&1) as f64;
                let idf = 1.0 + ((total + 1.0) / (document_frequency + 1.0)).ln();
                (word.clone(), (1.0 + (*count as f64).ln()) * idf)
            })
            .collect()
    };
    let norm = |v: &BTreeMap<String, f64>| v.values().map(|x| x * x).sum::<f64>().sqrt();
    let query_vector = weigh(&query_terms);
    let query_norm = norm(&query_vector);
    let mut scored: Vec<(usize, f64)> = term_sets
        .iter()
        .enumerate()
        .map(|(index, terms)| {
            let vector = weigh(terms);
            let denominator = query_norm * norm(&vector);
            if denominator == 0.0 {
                return (index, 0.0);
            }
            let dot: f64 = query_vector
                .iter()
                .map(|(word, value)| value * vector.get(word).unwrap_or(&0.0))
                .sum();
            (index, (dot / denominator).clamp(0.0, 1.0))
        })
        .collect();
    // Stable: equal scores keep the given order.
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut items: Vec<Option<T>> = candidates.into_iter().map(|(item, _)| Some(item)).collect();
    scored
        .into_iter()
        .map(|(index, score)| Ranked {
            item: items[index].take().expect("each index once"),
            score,
        })
        .collect()
}

/// What the document says, for matching: title (weighed twice), text and
/// metadata values.
fn capture_query(capture: &Capture) -> String {
    let mut text = format!("{} {} ", capture.title, capture.title);
    let body = &capture.text;
    let mut end = body.len().min(MAX_QUERY_BYTES);
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    text.push_str(&body[..end]);
    for value in capture.metadata.values() {
        text.push(' ');
        text.push_str(value);
    }
    text
}

/// What a template is about: its name, purpose and usage criteria, plus
/// the titles and demands of its approved examples.
fn template_text(template: &WorkTemplate, examples: &[Example]) -> String {
    let mut text = format!(
        "{} {} {} {}",
        template.name, template.name, template.purpose, template.usage_criteria
    );
    for example in examples
        .iter()
        .filter(|e| e.template_uid == template.uid && e.status == ExampleStatus::Approved)
    {
        text.push(' ');
        text.push_str(&example.title);
        text.push(' ');
        text.push_str(&example.demand);
    }
    text
}

impl Store {
    /// The published template of this kind that best matches the document,
    /// when any matches at all. Memoized by capture and kind.
    pub fn suggest_template(
        &self,
        capture: &Capture,
        kind: &str,
        member: &str,
    ) -> Result<Option<Ranked<WorkTemplate>>> {
        self.cached(
            &format!(
                "suggest_template\u{1}{}\u{1}{kind}\u{1}{member}",
                capture.request_id
            ),
            || {
                let examples = self.list_examples()?;
                let candidates: Vec<(WorkTemplate, String)> = self
                    .list_templates()?
                    .into_iter()
                    .filter(|t| t.offered_for(kind, member))
                    .map(|t| {
                        let text = template_text(&t, &examples);
                        (t, text)
                    })
                    .collect();
                if candidates.is_empty() {
                    return Ok(None);
                }
                Ok(rank(&capture_query(capture), candidates)
                    .into_iter()
                    .next()
                    .filter(|best| best.score >= MIN_TEMPLATE_SCORE))
            },
        )
    }

    /// The approved examples of a template, the ones most like the document
    /// first (by title and demand); with nothing in common, newest first.
    pub fn rank_examples(&self, template_uid: &str, capture: &Capture) -> Result<Vec<Example>> {
        let mut approved: Vec<Example> = self
            .examples_of(template_uid)?
            .into_iter()
            .filter(|e| e.status == ExampleStatus::Approved)
            .collect();
        approved.reverse();
        let candidates = approved
            .into_iter()
            .map(|e| {
                let text = format!("{} {} {}", e.title, e.title, e.demand);
                (e, text)
            })
            .collect();
        Ok(rank(&capture_query(capture), candidates)
            .into_iter()
            .map(|r| r.item)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_templates::Status;

    #[test]
    fn ranking_prefers_shared_vocabulary_and_keeps_order_on_ties() {
        let ranked = rank(
            "pedido de material de escritório para o almoxarifado",
            vec![
                (
                    "ferias",
                    "Resposta a pedido de férias de servidor".to_owned(),
                ),
                (
                    "compras",
                    "Resposta a pedido de material de escritório e almoxarifado".to_owned(),
                ),
                ("nada", "Encaminhamento de denúncia anônima".to_owned()),
                ("vazio", String::new()),
            ],
        );
        assert_eq!(ranked[0].item, "compras");
        assert!(ranked[0].score > ranked[1].score);
        assert!(ranked[0].percent() > 0);
        // Candidates with nothing in common keep their given order, after
        // every match.
        let tail: Vec<&str> = ranked.iter().skip(2).map(|r| r.item).collect();
        assert_eq!(tail, vec!["nada", "vazio"]);
        assert_eq!(ranked[3].score, 0.0);
    }

    #[test]
    fn the_template_about_the_document_is_suggested_and_its_examples_are_ranked() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let capture = crate::model::sample_capture();
        let member = store.active_actor();
        let mut compras = WorkTemplate::new("Resposta a pedido de material", "resposta", "Texto");
        compras.purpose = "Responder pedidos de material de escritório e almoxarifado.".into();
        let compras = store.save_template(&compras, None).unwrap();
        store
            .set_template_status(&compras.uid, Status::Published, None)
            .unwrap();
        let mut ferias = WorkTemplate::new("Resposta a pedido de férias", "resposta", "Texto");
        ferias.purpose = "Responder pedidos de férias e licenças de servidores.".into();
        let ferias = store.save_template(&ferias, None).unwrap();
        store
            .set_template_status(&ferias.uid, Status::Published, None)
            .unwrap();
        // Only published templates of the kind are candidates.
        let draft = store
            .save_template(
                &WorkTemplate::new("Material (rascunho)", "resposta", "Texto"),
                None,
            )
            .unwrap();
        assert_eq!(draft.status, Status::Draft);

        let best = store
            .suggest_template(&capture, "resposta", &member)
            .unwrap()
            .expect("the sample capture asks for office material");
        assert_eq!(best.item.uid, compras.uid, "{}", capture.title);
        assert!(best.score >= MIN_TEMPLATE_SCORE);
        assert!(store
            .suggest_template(&capture, "encaminhamento", &member)
            .unwrap()
            .is_none());

        // Examples: the one whose demand resembles the document comes first;
        // unverified ones are not candidates at all.
        for (title, demand, status) in [
            (
                "Antigo",
                "Pedido de reposição de toner e papel A4",
                ExampleStatus::Approved,
            ),
            (
                "Férias",
                "Servidor pede antecipação de férias",
                ExampleStatus::Approved,
            ),
            (
                "Material",
                "Setor solicita material de escritório para o almoxarifado",
                ExampleStatus::Approved,
            ),
            (
                "Não conferido",
                "material de escritório",
                ExampleStatus::Unverified,
            ),
        ] {
            store
                .save_example(
                    &Example {
                        template_uid: compras.uid.clone(),
                        title: title.into(),
                        demand: demand.into(),
                        reply: "Resposta".into(),
                        status,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap();
        }
        let ranked = store.rank_examples(&compras.uid, &capture).unwrap();
        let titles: Vec<&str> = ranked.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles[0], "Material");
        assert_eq!(titles.len(), 3);
        assert!(!titles.contains(&"Não conferido"));
    }
}
