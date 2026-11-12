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
    /// For an e-mail: the sender as the message had it ("Nome <endereço>").
    #[serde(default)]
    pub email_from: String,
    /// A conversation copied from WhatsApp (a manual record of kind
    /// whatsapp, whatever its number).
    #[serde(default)]
    pub whatsapp: bool,
    /// For a WhatsApp conversation: who it is with, as WhatsApp showed it.
    #[serde(default)]
    pub whatsapp_chat: String,
    /// When the document entered the library (its first version, RFC 3339).
    #[serde(default)]
    pub added_at: String,
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
    /// What the document says, one excerpt each, checked by the program
    /// (schema 16). Empty in analyses made before them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<Fact>,
    /// Proposals made from the checked facts (schema 16); `None` before them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposals: Option<Proposals>,
    /// How the analysis was made (provider, steps, refusals); schema 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<AnalysisRun>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSuggestion {
    pub action: String,
    pub reason: String,
    pub evidence: String,
}

/// Ids the program gives to proposals (a uuid in simple form) and the
/// records made from them keep.
pub(crate) fn valid_proposal_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The kinds of fact the Extrator may report.
pub const FACT_KINDS: &[&str] = &[
    "pedido",
    "prazo",
    "data_hora",
    "quantidade",
    "recurso",
    "remetente",
    "assunto",
    "outro",
];

/// Limits shared by the roles and by `validate_analysis`.
pub const MAX_FACTS: usize = 64;
pub const MAX_FACT_STATEMENT: usize = 300;
pub const FACT_QUOTE_MIN: usize = 8;
pub const FACT_QUOTE_MAX: usize = 600;
pub const MAX_TASK_PROPOSALS: usize = 6;
pub const MAX_RESERVATION_PROPOSALS: usize = 4;

/// One thing the document says, with the literal excerpt that says it. New
/// fields may be added later: no `deny_unknown_fields`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fact {
    /// "f1", "f2"…, given by the model, unique in the analysis.
    pub id: String,
    /// One of [`FACT_KINDS`].
    pub kind: String,
    /// One plain sentence, at most [`MAX_FACT_STATEMENT`] characters.
    pub statement: String,
    /// Literal excerpt ([`FACT_QUOTE_MIN`]..=[`FACT_QUOTE_MAX`] characters
    /// once the white space is collapsed).
    pub quote: String,
    /// "" is the document text or the human context; otherwise the id of a
    /// selected source.
    pub source_id: String,
    /// ISO `AAAA-MM-DD`, only when it is written in the quote.
    pub date: Option<String>,
    /// `HH:MM`, only when it is written in the quote.
    pub time: Option<String>,
    /// `HH:MM` ("das 14h às 17h").
    pub end_time: Option<String>,
    /// Only when it is written in the quote (digits or number words).
    pub quantity: Option<u32>,
    /// Set by the program (`evidence::check_fact`), never taken from the
    /// model: a role builds facts with the default, all false.
    pub checked: FactCheck,
}

/// What the program verified about a fact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactCheck {
    pub quote: bool,
    pub date: bool,
    pub time: bool,
    pub end_time: bool,
    pub quantity: bool,
}

/// What the Propositor suggests. The person reviews and saves each one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Proposals {
    /// At most [`MAX_TASK_PROPOSALS`].
    pub tasks: Vec<TaskProposal>,
    pub attendance: Option<AttendanceProposal>,
    /// At most [`MAX_RESERVATION_PROPOSALS`].
    pub reservations: Vec<ReservationProposal>,
    /// Required, not empty, when all three above are empty.
    pub nothing_reason: String,
}

impl Proposals {
    /// No task, attendance or reservation.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty() && self.attendance.is_none() && self.reservations.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskProposal {
    /// uuid (simple form), set by the program.
    pub id: String,
    /// One of `tasks::ACTIONS`.
    pub action: String,
    pub title: String,
    /// baixa | normal | alta | urgente
    pub priority: String,
    pub reason: String,
    /// Ids of the facts that support it (at least one, each with a checked
    /// quote).
    pub facts: Vec<String>,
    /// A "prazo" or "data_hora" fact whose date was checked.
    pub due_fact: Option<String>,
    /// Set by the program from `due_fact`.
    pub due_date: Option<String>,
    /// Set by the program: the quote of `due_fact`.
    pub due_source: String,
    /// E.g. a relative deadline the program could not read.
    pub remark: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AttendanceProposal {
    pub id: String,
    pub subject: String,
    /// Must appear in a cited "remetente" fact quote or in the metadata of
    /// the capture; otherwise emptied with a remark.
    pub received_from: String,
    pub due_fact: Option<String>,
    pub due_date: Option<String>,
    pub due_source: String,
    /// A key of `saiop::DUE_KIND_OPTIONS`; "a_confirmar" unless the quote
    /// says the date is official.
    pub due_kind: String,
    pub priority: String,
    pub priority_reason: String,
    /// One of `saiop::DOC_TYPE_OPTIONS` or "".
    pub planned_doc_type: String,
    pub facts: Vec<String>,
    pub remark: String,
}

impl Default for AttendanceProposal {
    fn default() -> Self {
        Self {
            id: String::new(),
            subject: String::new(),
            received_from: String::new(),
            due_fact: None,
            due_date: None,
            due_source: String::new(),
            due_kind: "a_confirmar".into(),
            priority: String::new(),
            priority_reason: String::new(),
            planned_doc_type: String::new(),
            facts: Vec::new(),
            remark: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReservationProposal {
    pub id: String,
    /// The thing asked for, as written in the document: it must appear in the
    /// quote of the cited "recurso" fact.
    pub what: String,
    pub what_fact: String,
    /// 1 unless a fact with a checked quantity says more.
    pub quantity: u32,
    /// A "data_hora" fact.
    pub when_fact: Option<String>,
    /// Set by the program from `when_fact`: `AAAA-MM-DDTHH:MM`.
    pub start: Option<String>,
    /// Set by the program from the checked `end_time`; otherwise `None`.
    pub end: Option<String>,
    pub purpose: String,
    pub facts: Vec<String>,
    pub remark: String,
}

impl Default for ReservationProposal {
    fn default() -> Self {
        Self {
            id: String::new(),
            what: String::new(),
            what_fact: String::new(),
            quantity: 1,
            when_fact: None,
            start: None,
            end: None,
            purpose: String::new(),
            facts: Vec::new(),
            remark: String::new(),
        }
    }
}

/// The audit of one analysis ("Material utilizado").
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisRun {
    /// `ai::provider_label`.
    pub provider: String,
    /// One per role call sequence.
    pub steps: Vec<RunStep>,
    /// Model calls in all.
    pub calls: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunStep {
    /// "extrator" | "propositor"
    pub role: String,
    pub attempts: u32,
    /// Reasons of the program for what it refused; never document content.
    pub refused: Vec<String>,
    /// Items dropped after the repair round (or for lack of budget).
    pub dropped: u32,
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
            "Data do registro inválida."
        );
        ensure!(
            self.text.len() <= MAX_CAPTURE_TEXT_BYTES,
            "O texto do registro é maior que 500 KB; reduza-o ou anexe o arquivo."
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
        ensure!(self.metadata.len() <= 256, "Há campos demais no registro.");
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
            "O registro é maior que 1 MB."
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Versão do formato de importação não aceita; atualize o Ponte."
        );
        uuid::Uuid::parse_str(&self.request_id).context("Identificador da importação inválido.")?;
        if self.is_manual() {
            return self.validate_manual();
        }
        let source = validate_sigadoc_url(&self.source_url)?;
        let number = self.document_number.trim();
        ensure!(
            !number.is_empty() && number.len() <= 160,
            "Número do documento ausente ou grande demais."
        );
        ensure!(
            valid_document_number(number) && !is_manual_number(number),
            "Número do documento inválido; importe uma ficha com número oficial reconhecido."
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
                    "O número do documento não corresponde ao endereço de origem."
                );
            }
        }
        let title = self.title.trim();
        ensure!(
            (3..=1000).contains(&title.len()) && !title.chars().any(char::is_control),
            "Título do documento ausente ou inválido."
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
            "A página não é uma ficha de documento reconhecida."
        );
        let date = DateTime::parse_from_rfc3339(&self.captured_at)
            .context("Data da importação inválida.")?;
        ensure!(
            date.offset().local_minus_utc() == 0,
            "Data da importação inválida."
        );
        ensure!(
            self.text.len() <= MAX_CAPTURE_TEXT_BYTES,
            "O texto é maior que 500 KB; importe um documento menor. Nada é cortado."
        );
        ensure!(
            self.metadata.len() <= 256 && self.references.len() <= 1024,
            "Há campos ou referências demais."
        );
        crate::dossier::movements(self)?;
        for (key, value) in &self.metadata {
            ensure!(
                !key.is_empty() && key.len() <= 256 && value.len() <= 64 * 1024,
                "Informação do documento inválida ou grande demais."
            );
        }
        ensure!(
            self.coverage.adapter.len() <= 256
                && self.coverage.recognized.len() <= 256
                && self.coverage.pending.len() <= 256,
            "Dados de cobertura da importação inválidos."
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
                "Nome da referência grande demais."
            );
            validate_sigadoc_url(&reference.url).context(
                "A referência deve apontar para uma página do SIGADOC, sem dados de sessão.",
            )?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_CAPTURE_PACKET_BYTES,
            "A importação é maior que 1 MB; importe um conjunto menor. Nada é cortado."
        );
        Ok(())
    }
}

/// Links never transport browser credentials or cross the allowed origin.
pub fn validate_sigadoc_url(value: &str) -> Result<Url> {
    ensure!(value.len() <= 8192, "Endereço de origem grande demais.");
    let url = Url::parse(value).context("Endereço de origem inválido.")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("www.sigadoc.mt.gov.br")
            && url.port_or_known_default() == Some(443),
        "Endereço de origem inválido: use https://www.sigadoc.mt.gov.br/."
    );
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "O endereço não pode conter usuário, senha ou fragmento."
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
        bail!("A página de login não pode ser importada.");
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
            "O endereço contém dados de sessão; remova-os antes de importar."
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

/// An analysis with checked facts and one proposal of each kind, built the
/// way the roles will build them (generic sample data only).
#[cfg(test)]
pub(crate) fn sample_proposed_analysis(capture_id: i64) -> Analysis {
    let deadline_quote = "responder até 30/11/2026";
    let checked_all = FactCheck {
        quote: true,
        date: true,
        time: true,
        end_time: true,
        quantity: true,
    };
    Analysis {
        capture_id,
        subjects: vec!["Reserva de cadeiras".into()],
        priority: "normal".into(),
        reason: "O setor pede cadeiras e uma resposta.".into(),
        deadline_candidate: Some(deadline_quote.into()),
        summary: "Pedido de cadeiras para uma reunião.".into(),
        model: "teste".into(),
        created_at: "2026-11-12T12:00:00.000Z".into(),
        facts: vec![
            Fact {
                id: "f1".into(),
                kind: "prazo".into(),
                statement: "A resposta é esperada até 30/11/2026.".into(),
                quote: deadline_quote.into(),
                date: Some("2026-11-30".into()),
                checked: FactCheck {
                    quote: true,
                    date: true,
                    ..FactCheck::default()
                },
                ..Fact::default()
            },
            Fact {
                id: "f2".into(),
                kind: "recurso".into(),
                statement: "Pedem vinte cadeiras.".into(),
                quote: "reserva de 20 (vinte) cadeiras".into(),
                quantity: Some(20),
                checked: FactCheck {
                    quote: true,
                    quantity: true,
                    ..FactCheck::default()
                },
                ..Fact::default()
            },
            Fact {
                id: "f3".into(),
                kind: "data_hora".into(),
                statement: "A reunião é em 12/11/2026, das 14h às 17h.".into(),
                quote: "no dia 12 de novembro de 2026, das 14h às 17h".into(),
                date: Some("2026-11-12".into()),
                time: Some("14:00".into()),
                end_time: Some("17:00".into()),
                checked: FactCheck {
                    quantity: false,
                    ..checked_all
                },
                ..Fact::default()
            },
        ],
        proposals: Some(Proposals {
            tasks: vec![TaskProposal {
                id: "a1b2c3d4e5f60718293a4b5c6d7e8f90".into(),
                action: "responder".into(),
                title: "Responder o ofício sobre as cadeiras".into(),
                priority: "alta".into(),
                reason: "O documento fixa a data da resposta.".into(),
                facts: vec!["f1".into()],
                due_fact: Some("f1".into()),
                due_date: Some("2026-11-30".into()),
                due_source: deadline_quote.into(),
                remark: String::new(),
            }],
            attendance: Some(AttendanceProposal {
                id: "b1b2c3d4e5f60718293a4b5c6d7e8f91".into(),
                subject: "Pedido de cadeiras".into(),
                priority: "normal".into(),
                facts: vec!["f2".into()],
                ..AttendanceProposal::default()
            }),
            reservations: vec![ReservationProposal {
                id: "c1b2c3d4e5f60718293a4b5c6d7e8f92".into(),
                what: "cadeiras".into(),
                what_fact: "f2".into(),
                quantity: 20,
                when_fact: Some("f3".into()),
                start: Some("2026-11-12T14:00".into()),
                end: Some("2026-11-12T17:00".into()),
                purpose: "Reunião do setor".into(),
                facts: vec!["f2".into(), "f3".into()],
                remark: String::new(),
            }],
            nothing_reason: String::new(),
        }),
        run: Some(AnalysisRun {
            provider: "Ollama local".into(),
            steps: vec![RunStep {
                role: "extrator".into(),
                attempts: 1,
                refused: vec![],
                dropped: 0,
            }],
            calls: 2,
        }),
        ..Analysis::default()
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
