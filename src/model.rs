//! What the browser extension sends (`Capture`), its validation and limits,
//! and the summary and detail types the interface shows.
use anyhow::{bail, ensure, Context, Result};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use url::Url;

pub const MAX_CAPTURE_TEXT_BYTES: usize = 500 * 1024;
pub const MAX_CAPTURE_PACKET_BYTES: usize = 1024 * 1024;

/// Documents created by hand in the Ponte (no SIGADOC page). Their number
/// starts with this prefix, which no official number uses, and their source
/// is `ponte://manual/<number>`; the identity hash uses that origin, so a
/// manual record can never merge with a real document.
pub const MANUAL_PREFIX: &str = "MANUAL-";
/// E-mails imported into the Ponte are manual records with a numbering of
/// their own (`EMAIL-AAAA/NNNN`), so a reader sees what they are.
pub const EMAIL_PREFIX: &str = "EMAIL-";
pub const MANUAL_ORIGIN: &str = "ponte://manual/";
/// Number of a manual record created on a connected computer while the main
/// one is out of reach; the main computer assigns the final number.
pub const MANUAL_PENDING: &str = "MANUAL-PENDENTE";
pub const MANUAL_ADAPTER: &str = "manual-v1";

pub fn is_manual_number(number: &str) -> bool {
    let upper = number.trim().to_uppercase();
    upper.starts_with(MANUAL_PREFIX) || upper.starts_with(EMAIL_PREFIX)
}

/// An imported e-mail (`EMAIL-AAAA/NNNN`).
pub fn is_email_number(number: &str) -> bool {
    number.trim().to_uppercase().starts_with(EMAIL_PREFIX)
}

/// The numbering a manual record gets: e-mails their own.
pub fn manual_prefix_for(capture: &Capture) -> &'static str {
    if capture.metadata.get("manual_kind").map(String::as_str) == Some("email") {
        EMAIL_PREFIX
    } else {
        MANUAL_PREFIX
    }
}

/// `MANUAL-AAAA/NNNN` or `EMAIL-AAAA/NNNN`.
pub fn valid_manual_number(number: &str) -> bool {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(r"^(?:MANUAL|EMAIL)-(?:19|20)[0-9]{2}/[0-9]{1,8}$")
            .expect("constant manual pattern")
    });
    pattern.is_match(&number.trim().to_uppercase())
}

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

/// What an import did with the page it received.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportOutcome {
    /// New content: a capture was stored (first import or a new version).
    Imported,
    /// The same request had already been stored; this is the lost
    /// confirmation of that import. Nothing was written again.
    Confirmed,
    /// A new request whose content is identical to a capture already in the
    /// library. Nothing new was stored; the person is warned instead.
    Unchanged,
}

impl ImportOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportOutcome::Imported => "imported",
            ImportOutcome::Confirmed => "confirmed",
            ImportOutcome::Unchanged => "unchanged",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportReceipt {
    pub request_id: String,
    pub document_id: i64,
    pub capture_id: i64,
    /// True for `Confirmed` and `Unchanged`. Kept for the browser extension
    /// and team hosts from before `outcome` existed.
    pub duplicate: bool,
    /// Absent when the receipt comes from an older team host.
    #[serde(default)]
    pub outcome: Option<ImportOutcome>,
    /// When `Unchanged`: when the identical capture was stored (RFC 3339).
    #[serde(default)]
    pub existing_captured_at: Option<String>,
    /// When `Unchanged`: the identical capture is an older version that has
    /// become the current one again.
    #[serde(default)]
    pub previous_version: bool,
}

impl ImportReceipt {
    /// The outcome, also for receipts from hosts that only report `duplicate`:
    /// those behave as before (the document opens, nothing is refused).
    pub fn effective_outcome(&self) -> ImportOutcome {
        match self.outcome {
            Some(outcome) => outcome,
            None if self.duplicate => ImportOutcome::Confirmed,
            None => ImportOutcome::Imported,
        }
    }
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
    /// Created by hand in the Ponte, not imported from the SIGADOC.
    #[serde(default)]
    pub manual: bool,
    /// An e-mail brought into the Ponte (a manual record of kind e-mail,
    /// whatever its number).
    #[serde(default)]
    pub email: bool,
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
    /// Where the text came from; `None` for versions saved before 0.20 or
    /// typed without the AI.
    #[serde(default)]
    pub provenance: Option<DraftProvenance>,
}

/// How a draft version was produced: enough to read it later even without
/// the generation record (which may live on another computer's copy).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftProvenance {
    /// The generation record, when the text came from the AI.
    #[serde(default)]
    pub generation_uid: Option<String>,
    #[serde(default)]
    pub template_uid: Option<String>,
    #[serde(default)]
    pub template_version: Option<u32>,
    #[serde(default)]
    pub template_name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    /// What the person asked for this draft.
    #[serde(default)]
    pub request_guidance: String,
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
    pub fn is_manual(&self) -> bool {
        self.source_url.starts_with(MANUAL_ORIGIN)
    }

    /// Rules of a manual record: its own number and origin, no SIGADOC
    /// links, no movement history; the rest as any capture.
    fn validate_manual(&self) -> Result<()> {
        let number = self.document_number.trim();
        ensure!(
            number == MANUAL_PENDING || valid_manual_number(number),
            "Número de registro manual inválido."
        );
        ensure!(
            self.source_url == format!("{MANUAL_ORIGIN}{number}"),
            "Origem do registro manual não corresponde ao número."
        );
        let title = self.title.trim();
        ensure!(
            (3..=1000).contains(&title.len()) && !title.chars().any(char::is_control),
            "Informe um título com pelo menos 3 caracteres."
        );
        let date = DateTime::parse_from_rfc3339(&self.captured_at)
            .context("Data do registro inválida.")?;
        ensure!(
            date.offset().local_minus_utc() == 0,
            "Data do registro deve usar UTC."
        );
        ensure!(
            self.text.len() <= MAX_CAPTURE_TEXT_BYTES,
            "O texto do registro excede 500 KiB; reduza-o ou anexe o arquivo."
        );
        ensure!(
            self.references.is_empty(),
            "Registro manual não leva referências do SIGADOC."
        );
        ensure!(
            !self.metadata.contains_key("movement_history")
                && !self.metadata.contains_key("movement_source"),
            "Registro manual não tem histórico de tramitação."
        );
        ensure!(self.metadata.len() <= 256, "Quantidade de campos excedida.");
        for (key, value) in &self.metadata {
            ensure!(
                !key.is_empty() && key.len() <= 256 && value.len() <= 64 * 1024,
                "Campo do registro inválido ou grande demais."
            );
        }
        ensure!(
            self.coverage.adapter == MANUAL_ADAPTER
                && self.coverage.recognized.len() <= 256
                && self.coverage.pending.len() <= 256,
            "Cobertura do registro manual inválida."
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_CAPTURE_PACKET_BYTES,
            "Registro excede 1 MiB."
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "Versão de captura não suportada.");
        uuid::Uuid::parse_str(&self.request_id).context("request_id deve ser um UUID válido.")?;
        if self.is_manual() {
            return self.validate_manual();
        }
        let source = validate_sigadoc_url(&self.source_url)?;
        let number = self.document_number.trim();
        ensure!(
            !number.is_empty() && number.len() <= 160,
            "Número documental ausente ou grande demais."
        );
        ensure!(
            valid_document_number(number) && !is_manual_number(number),
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
pub(crate) fn sample_manual_capture() -> Capture {
    Capture {
        schema_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        source_url: format!("{MANUAL_ORIGIN}{MANUAL_PENDING}"),
        document_number: MANUAL_PENDING.into(),
        title: "Ligação do gabinete sobre a reforma".into(),
        captured_at: "2026-11-04T12:00:00Z".into(),
        text: "Registro da ligação: pedem posição até sexta.".into(),
        metadata: BTreeMap::from([
            (
                "document_type".to_string(),
                "Ligação telefônica".to_string(),
            ),
            ("manual_kind".to_string(), "ligacao".to_string()),
            ("origin".to_string(), "Gabinete do Secretário".to_string()),
        ]),
        references: vec![],
        coverage: Coverage {
            adapter: MANUAL_ADAPTER.into(),
            recognized: vec!["registro manual".into()],
            pending: vec![],
        },
    }
}

#[cfg(test)]
mod manual_tests {
    use super::*;

    #[test]
    fn manual_records_have_their_own_number_and_origin_and_never_pass_as_official() {
        let manual = sample_manual_capture();
        manual.validate().unwrap();
        assert!(manual.is_manual());
        assert!(valid_manual_number("MANUAL-2026/0007") && !valid_manual_number("MANUAL-26/7"));
        assert!(
            valid_document_number("MANUAL-2026/0007"),
            "the official grammar would accept it…"
        );
        // …so a SIGADOC capture carrying a manual number is refused.
        let mut fake = sample_capture();
        fake.document_number = "MANUAL-2026/0007".into();
        fake.source_url =
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=MANUAL-2026%2F0007"
                .into();
        assert!(fake.validate().is_err());
        // And a manual record cannot borrow an official number or SIGADOC links.
        let mut wrong = sample_manual_capture();
        wrong.document_number = "SEFAZ-PRO-2026/00001".into();
        wrong.source_url = format!("{MANUAL_ORIGIN}SEFAZ-PRO-2026/00001");
        assert!(wrong.validate().is_err());
        let mut with_link = sample_manual_capture();
        with_link.references.push(DocumentReference {
            label: "x".into(),
            url: "https://www.sigadoc.mt.gov.br/doc".into(),
            kind: "reference".into(),
        });
        assert!(with_link.validate().is_err());
        let mut mismatch = sample_manual_capture();
        mismatch.document_number = "MANUAL-2026/0001".into();
        assert!(
            mismatch.validate().is_err(),
            "origin must carry the same number"
        );
        let mut short = sample_manual_capture();
        short.title = "ab".into();
        assert!(short.validate().is_err());
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
