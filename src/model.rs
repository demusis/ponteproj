use anyhow::{bail, ensure, Context, Result};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use url::Url;

pub const MAX_CAPTURE_TEXT_BYTES: usize = 500 * 1024;
pub const MAX_CAPTURE_PACKET_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema_version: u32,
    pub request_id: String,
    pub source_url: String,
    pub document_number: String,
    pub title: String,
    pub captured_at: String,
    pub text: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub references: Vec<DocumentReference>,
    #[serde(default)]
    pub coverage: Coverage,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocumentReference {
    pub label: String,
    pub url: String,
    pub kind: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub adapter: String,
    pub recognized: Vec<String>,
    pub pending: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportReceipt {
    pub request_id: String,
    pub document_id: i64,
    pub capture_id: i64,
    pub duplicate: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentSummary {
    pub id: i64,
    pub document_number: String,
    pub title: String,
    pub last_seen_at: String,
    pub latest_capture_id: i64,
    pub subjects: Vec<String>,
    pub priority: String,
    pub analysis_stale: bool,
    /// The capture explicitly says this document is juntado/apensado to another one.
    #[serde(default)]
    pub is_joined: bool,
    /// Canonical number of the parent document (via suffix removed), when shown.
    #[serde(default)]
    pub joined_to_number: Option<String>,
    /// Local id of the parent when it is an active document in this library.
    #[serde(default)]
    pub joined_to_id: Option<i64>,
    /// Active documents in this library that are juntados to this one.
    #[serde(default)]
    pub joined_children_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentDetail {
    pub summary: DocumentSummary,
    pub capture: Capture,
    pub note: String,
    pub drafts: Vec<DraftVersion>,
    pub analysis: Option<Analysis>,
    #[serde(default)]
    pub work_context: crate::work_context::WorkContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftVersion {
    #[serde(default)]
    pub materials: Vec<crate::evidence::Source>,
    pub id: i64,
    pub kind: String,
    pub body: String,
    pub capture_id: i64,
    pub created_at: String,
    pub stale: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Analysis {
    pub capture_id: i64,
    pub subjects: Vec<String>,
    pub priority: String,
    pub reason: String,
    pub deadline_candidate: Option<String>,
    pub summary: String,
    pub model: String,
    pub created_at: String,
    #[serde(default)]
    pub human_context: String,
    #[serde(default)]
    pub suggested_actions: Vec<ActionSuggestion>,
    #[serde(default)]
    pub missing_context: Vec<String>,
    #[serde(default)]
    pub materials: Vec<crate::evidence::Source>,
    #[serde(default)]
    pub citations: Vec<crate::evidence::Citation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSuggestion {
    pub action: String,
    pub reason: String,
    pub evidence: String,
}

/// Validate again at the native host boundary; browser data is untrusted.
impl Capture {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "Versão de captura não suportada.");
        uuid::Uuid::parse_str(&self.request_id).context("request_id deve ser um UUID válido.")?;
        let source = validate_sigadoc_url(&self.source_url)?;
        let number = self.document_number.trim();
        ensure!(
            !number.is_empty() && number.len() <= 160,
            "Número documental ausente ou grande demais."
        );
        ensure!(
            valid_document_number(number),
            "Número documental inválido; importe uma ficha com identidade oficial reconhecida."
        );
        for (key, value) in source.query_pairs() {
            if [
                "sigla",
                "numero",
                "numerodocumento",
                "document_number",
                "documento",
            ]
            .contains(&key.to_ascii_lowercase().as_str())
            {
                let direct_identity = valid_document_number(value.trim())
                    && value.trim().eq_ignore_ascii_case(number);
                // SIGA's exibir route can address a particular copy (-A, -B, …)
                // while its official heading keeps the base document number.
                // This exception never changes the official identity grammar.
                let display_copy = key.eq_ignore_ascii_case("sigla")
                    && source.path() == "/sigaex/app/expediente/doc/exibir"
                    && value.rsplit_once('-').is_some_and(|(base, suffix)| {
                        base == number
                            && valid_document_number(base)
                            && suffix.len() == 1
                            && suffix.as_bytes()[0].is_ascii_uppercase()
                    });
                ensure!(
                    direct_identity || display_copy,
                    "O número documental não corresponde à identidade presente na URL de origem."
                );
            }
        }
        let title = self.title.trim();
        ensure!(
            (3..=1000).contains(&title.len()) && !title.chars().any(char::is_control),
            "Título documental ausente ou inválido."
        );
        let normalized_title = title.to_lowercase();
        ensure!(
            ![
                "login",
                "entrar",
                "lista de documentos",
                "resultados da pesquisa",
                "sigadoc"
            ]
            .contains(&normalized_title.as_str()),
            "A página não é uma ficha documental reconhecida."
        );
        let date = DateTime::parse_from_rfc3339(&self.captured_at)
            .context("captured_at deve ser uma data RFC3339 UTC.")?;
        ensure!(
            date.offset().local_minus_utc() == 0,
            "captured_at deve usar UTC."
        );
        ensure!(
            self.text.len() <= MAX_CAPTURE_TEXT_BYTES,
            "Texto excede 500 KiB; importe uma captura menor, sem truncamento."
        );
        ensure!(
            self.metadata.len() <= 256 && self.references.len() <= 1024,
            "Quantidade de campos ou referências excedida."
        );
        crate::dossier::movements(self)?;
        for (key, value) in &self.metadata {
            ensure!(
                !key.is_empty() && key.len() <= 256 && value.len() <= 64 * 1024,
                "Metadado inválido ou grande demais."
            );
        }
        ensure!(
            self.coverage.adapter.len() <= 256
                && self.coverage.recognized.len() <= 256
                && self.coverage.pending.len() <= 256,
            "Cobertura de captura inválida."
        );
        for field in self
            .coverage
            .recognized
            .iter()
            .chain(&self.coverage.pending)
        {
            ensure!(field.len() <= 2048, "Descrição de cobertura grande demais.");
        }
        for reference in &self.references {
            ensure!(
                matches!(
                    reference.kind.as_str(),
                    "original" | "attachment" | "antecedent" | "reference"
                ),
                "Tipo de referência inválido."
            );
            ensure!(
                reference.label.len() <= 2000,
                "Rótulo de referência grande demais."
            );
            validate_sigadoc_url(&reference.url)
                .context("Referência deve apontar para HTTPS SIGADOC, sem segredo de sessão.")?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_CAPTURE_PACKET_BYTES,
            "Captura excede 1 MiB; importe um pacote menor, sem truncamento."
        );
        Ok(())
    }
}

/// Links never transport browser credentials or cross the allowed origin.
pub fn validate_sigadoc_url(value: &str) -> Result<Url> {
    ensure!(value.len() <= 8192, "URL de origem grande demais.");
    let url = Url::parse(value).context("URL de origem inválida.")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("www.sigadoc.mt.gov.br")
            && url.port_or_known_default() == Some(443),
        "Origem inválida: use https://www.sigadoc.mt.gov.br/."
    );
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "URL deve estar sem credenciais ou fragmentos."
    );
    let path = url.path().to_ascii_lowercase();
    if [
        "login",
        "logout",
        "autentic",
        "j_security_check",
        "jsessionid",
    ]
    .iter()
    .any(|word| path.contains(word))
    {
        bail!("Página de autenticação não pode ser importada.");
    }
    for (key, _) in url.query_pairs() {
        let key = key.to_ascii_lowercase().replace(['_', '-'], "");
        let secret = [
            "token",
            "password",
            "senha",
            "session",
            "sessao",
            "secret",
            "credential",
            "authorization",
            "csrf",
            "xsrf",
            "cookie",
        ]
        .iter()
        .any(|marker| key.contains(marker))
            || ["auth", "sid", "ticket", "apikey"].contains(&key.as_str());
        ensure!(
            !secret,
            "URL contém parâmetro de sessão ou segredo; remova-o antes de importar."
        );
    }
    Ok(url)
}

pub(crate) fn valid_document_number(number: &str) -> bool {
    // Same declared formats as the browser adapter: SIGA acronyms, number/year,
    // year/number and CNJ. An arbitrary DOM numeric ID is never official identity.
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| regex::Regex::new(r"^(?:(?:[A-Z][A-Z0-9]{1,19}[-/]){1,4}(?:19|20)[0-9]{2}[-/][0-9]{1,12}(?:[-/][0-9]{1,8})?|[0-9]{1,12}[/.-](?:19|20)[0-9]{2}|(?:19|20)[0-9]{2}[/.-][0-9]{1,12}|[0-9]{7}-[0-9]{2}\.(?:19|20)[0-9]{2}\.[0-9]\.[0-9]{2}\.[0-9]{4})$").expect("constant document identity pattern"));
    pattern.is_match(&number.to_uppercase())
}

pub(crate) fn valid_kind(kind: &str) -> bool {
    matches!(kind, "resposta" | "encaminhamento" | "complementacao")
}

#[cfg(test)]
pub(crate) fn sample_capture() -> Capture {
    Capture {
        schema_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        source_url:
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F00001"
                .into(),
        document_number: "SEFAZ-PRO-2026/00001".into(),
        title: "Solicitação de material de escritório".into(),
        captured_at: "2026-11-01T12:00:00Z".into(),
        text: "Solicita aquisição de papel e material de escritório para a unidade.".into(),
        metadata: BTreeMap::new(),
        references: vec![],
        coverage: Coverage {
            adapter: "sigadoc-dom-v1".into(),
            recognized: vec!["texto".into()],
            pending: vec!["anexos: apenas referências".into()],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_identity_formats_match_the_declared_browser_adapter() {
        for number in [
            "SEFAZ-PRO-2026/00001",
            "TESTE-2026/00001",
            "12345/2026",
            "2026/12345",
            "0000001-01.2026.1.01.0001",
        ] {
            assert!(valid_document_number(number), "rejected supported format");
        }
        for number in [
            "item1",
            "42",
            "documento",
            "UNKNOWN-ID-123",
            "2026/<script>",
        ] {
            assert!(
                !valid_document_number(number),
                "accepted unofficial identity"
            );
        }
    }

    #[test]
    fn exibir_sigla_can_address_an_uppercase_copy_of_the_exact_official_number() {
        let mut capture = sample_capture();
        capture.document_number = "TESTE-DEL-2026/01354".into();
        for copy in ['A', 'B', 'Z'] {
            capture.source_url = format!("https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-DEL-2026%2F01354-{copy}");
            capture.validate().unwrap();
        }
        for url in [
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-DEL-2026%2F99999-A",
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-DEL-2026%2F01354-AA",
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-DEL-2026%2F01354-a",
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-DEL-2026%2F01354-1",
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-DEL-2026%2F01354-A-lixo",
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=TESTE-DEL-2026%2F01354-A",
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?numero=TESTE-DEL-2026%2F01354-A",
        ] {
            capture.source_url = url.into();
            assert!(capture.validate().is_err(), "accepted an unrelated or invalid copy URL");
        }
        // A copy suffix never becomes an accepted official document number.
        capture.document_number.push_str("-A");
        capture.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir".into();
        assert!(capture.validate().is_err());
    }

    #[test]
    fn accepts_partial_capture_but_rejects_invalid_identity_origin_or_size() {
        let mut c = sample_capture();
        c.text.clear();
        c.metadata.clear();
        c.references.clear();
        c.validate().unwrap();
        c.source_url = "https://www.sigadoc.mt.gov.br.evil.example/doc".into();
        assert!(c.validate().is_err());
        c = sample_capture();
        c.request_id = "random".into();
        assert!(c.validate().is_err());
        c = sample_capture();
        c.document_number = "desconhecido".into();
        assert!(c.validate().is_err());
        c = sample_capture();
        c.document_number = "item1".into();
        assert!(c.validate().is_err());
        c = sample_capture();
        c.document_number = "SEFAZ-PRO-2026/00002".into();
        assert!(c.validate().is_err());
        c = sample_capture();
        c.text = "x".repeat(MAX_CAPTURE_TEXT_BYTES + 1);
        assert!(c.validate().is_err());
        c = sample_capture();
        c.source_url.push_str("&token=secret");
        assert!(c.validate().is_err());
        c = sample_capture();
        c.source_url.push_str("&accessToken=secret");
        assert!(c.validate().is_err());
        c = sample_capture();
        c.source_url.push_str("&csrf=secret");
        assert!(c.validate().is_err());
        c = sample_capture();
        c.captured_at = "2026-11-01T12:00:00-04:00".into();
        assert!(c.validate().is_err());
    }
}
