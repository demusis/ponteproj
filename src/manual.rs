//! Manual records: documents the person creates in the Ponte because they
//! never existed in the SIGADOC (a phone call, an internal memo, a paper
//! letter, an e-mail). They are ordinary documents in every other respect
//! (tasks, notes, drafts, attachments, AI, trash, team sync): a creation or
//! an edit is just a capture with the `ponte://manual/` origin, so the main
//! computer numbers it and replicates it like any import.
use crate::model::{
    Capture, Coverage, ImportOutcome, ImportReceipt, MANUAL_ADAPTER, MANUAL_ORIGIN, MANUAL_PENDING,
};
use crate::storage::Store;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

/// Kinds offered in the form: key stored in `metadata["manual_kind"]`,
/// label stored in `metadata["document_type"]` (what the ficha shows).
pub const KINDS: &[(&str, &str)] = &[
    ("memorando", "Memorando ou comunicação interna"),
    ("ligacao", "Ligação telefônica"),
    ("email", "E-mail recebido"),
    ("papel", "Ofício ou documento em papel"),
    ("reuniao", "Reunião ou despacho verbal"),
    ("outro", "Outro"),
];

pub fn kind_label(key: &str) -> &str {
    KINDS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, label)| *label)
        .unwrap_or("Outro")
}

/// What the person fills in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManualFields {
    pub kind: String,
    pub title: String,
    /// Who sent it or where it came from (free text, administrative).
    pub origin: String,
    /// `AAAA-MM-DD` or empty.
    pub document_date: String,
    pub text: String,
}

impl ManualFields {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            KINDS.iter().any(|(k, _)| *k == self.kind),
            "Escolha o tipo do registro."
        );
        let title = self.title.trim();
        ensure!(
            title.chars().count() >= 3
                && title.len() <= 1000
                && !title.chars().any(char::is_control),
            "Informe um título com pelo menos 3 caracteres."
        );
        ensure!(
            self.origin.len() <= 1000 && !self.origin.chars().any(char::is_control),
            "Origem grande demais ou com caracteres inválidos."
        );
        if !self.document_date.trim().is_empty() {
            chrono::NaiveDate::parse_from_str(self.document_date.trim(), "%Y-%m-%d")
                .context("Data do documento inválida; use dd/mm/aaaa.")?;
        }
        ensure!(
            self.text.len() <= crate::model::MAX_CAPTURE_TEXT_BYTES,
            "O texto excede 500 KiB; resuma-o aqui e anexe o arquivo original em Anexos."
        );
        Ok(())
    }

    /// Fields as stored in an existing manual record, for the edit form.
    pub fn from_capture(capture: &Capture) -> Self {
        let meta = |key: &str| capture.metadata.get(key).cloned().unwrap_or_default();
        Self {
            kind: {
                let kind = meta("manual_kind");
                if kind.is_empty() {
                    "outro".into()
                } else {
                    kind
                }
            },
            title: capture.title.clone(),
            origin: meta("origin"),
            document_date: meta("document_date"),
            text: capture.text.clone(),
        }
    }

    fn capture(&self, number: &str, registered_by: &str) -> Capture {
        self.capture_with(
            number,
            registered_by,
            uuid::Uuid::new_v4().to_string(),
            BTreeMap::new(),
        )
    }

    /// The capture with a given request id (an import that must recognize
    /// itself, like an e-mail by its Message-ID) and extra metadata.
    pub(crate) fn capture_with(
        &self,
        number: &str,
        registered_by: &str,
        request_id: String,
        extra: BTreeMap<String, String>,
    ) -> Capture {
        let mut metadata = extra;
        metadata.insert("manual_kind".to_string(), self.kind.clone());
        metadata.insert(
            "document_type".to_string(),
            kind_label(&self.kind).to_string(),
        );
        if !self.origin.trim().is_empty() {
            metadata.insert("origin".to_string(), self.origin.trim().to_string());
        }
        if !self.document_date.trim().is_empty() {
            metadata.insert(
                "document_date".to_string(),
                self.document_date.trim().to_string(),
            );
        }
        if !registered_by.trim().is_empty() {
            metadata.insert(
                "registered_by".to_string(),
                registered_by.trim().to_string(),
            );
        }
        Capture {
            schema_version: 1,
            request_id,
            source_url: format!("{MANUAL_ORIGIN}{number}"),
            document_number: number.to_owned(),
            title: self.title.trim().to_owned(),
            captured_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            text: self.text.trim_end().to_owned(),
            metadata,
            references: vec![],
            coverage: Coverage {
                adapter: MANUAL_ADAPTER.into(),
                recognized: vec!["registro manual".into()],
                pending: vec![],
            },
        }
    }
}

impl Store {
    /// Creates the record; the number is assigned where the import runs
    /// (the main computer, in a team).
    pub fn create_manual_document(&mut self, fields: &ManualFields) -> Result<ImportReceipt> {
        fields.validate()?;
        let actor = self.active_actor();
        let capture = fields.capture(MANUAL_PENDING, &actor);
        self.import_capture(&capture)
    }

    /// Saves a new version of an existing manual record (same number).
    /// Identical content is reported as `Unchanged`, like any import.
    pub fn update_manual_document(
        &mut self,
        id: i64,
        fields: &ManualFields,
    ) -> Result<ImportReceipt> {
        fields.validate()?;
        let detail = self.load_document(id)?;
        ensure!(
            detail.capture.is_manual(),
            "Este documento veio do SIGADOC; ele não é editado no Ponte."
        );
        ensure!(
            detail.capture.document_number != MANUAL_PENDING,
            "Este registro ainda não recebeu número do computador principal. Aguarde a sincronização para editá-lo."
        );
        let registered_by = detail
            .capture
            .metadata
            .get("registered_by")
            .cloned()
            .unwrap_or_else(|| self.active_actor());
        let capture = fields.capture(&detail.capture.document_number, &registered_by);
        let receipt = self.import_capture(&capture)?;
        ensure!(
            receipt.document_id == id,
            "O registro editado não corresponde ao documento aberto."
        );
        Ok(receipt)
    }
}

/// True when the receipt means "nothing changed" (edit identical to the
/// current version).
pub fn unchanged(receipt: &ImportReceipt) -> bool {
    receipt.effective_outcome() == ImportOutcome::Unchanged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::sample_capture;

    fn fields() -> ManualFields {
        ManualFields {
            kind: "ligacao".into(),
            title: "Ligação do gabinete sobre a reforma".into(),
            origin: "Gabinete do Secretário".into(),
            document_date: "2026-11-04".into(),
            text: "Pedem posição até sexta. Juntado ao processo SEFAZ-PRO-2026/00001 (anotação)."
                .into(),
        }
    }

    #[test]
    fn manual_records_are_numbered_in_sequence_edited_as_versions_and_kept_apart() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let official = store.import_capture(&sample_capture()).unwrap();
        let year = chrono::Local::now().format("%Y").to_string();

        let first = store.create_manual_document(&fields()).unwrap();
        assert_eq!(first.effective_outcome(), ImportOutcome::Imported);
        let detail = store.load_document(first.document_id).unwrap();
        assert_eq!(
            detail.summary.document_number,
            format!("MANUAL-{year}/0001")
        );
        assert!(detail.summary.manual);
        assert_eq!(
            detail.capture.source_url,
            format!("{MANUAL_ORIGIN}MANUAL-{year}/0001")
        );
        assert_eq!(
            detail.capture.metadata["document_type"],
            "Ligação telefônica"
        );
        assert!(!detail.summary.is_joined, "free text never marks a juntada");
        assert_ne!(first.document_id, official.document_id);
        // The triage task exists like for any document.
        assert!(store
            .list_tasks()
            .unwrap()
            .iter()
            .any(|t| t.document_id == first.document_id));

        let second = store.create_manual_document(&fields()).unwrap();
        assert_eq!(
            store
                .load_document(second.document_id)
                .unwrap()
                .summary
                .document_number,
            format!("MANUAL-{year}/0002"),
            "same content, new record: manual records are never merged"
        );

        // Editing = new version with the same number; identical edit is ignored.
        let mut edited = fields();
        edited.text.push_str("\nRetorno dado em 05/11.");
        let version = store
            .update_manual_document(first.document_id, &edited)
            .unwrap();
        assert_eq!(version.effective_outcome(), ImportOutcome::Imported);
        assert_eq!(version.document_id, first.document_id);
        let detail = store.load_document(first.document_id).unwrap();
        assert_eq!(
            detail.summary.document_number,
            format!("MANUAL-{year}/0001")
        );
        assert!(detail.capture.text.ends_with("05/11."));
        assert_eq!(store.capture_versions(first.document_id).unwrap().len(), 2);
        let same = store
            .update_manual_document(first.document_id, &edited)
            .unwrap();
        assert!(unchanged(&same));
        assert!(store
            .update_manual_document(official.document_id, &edited)
            .is_err());
        assert_eq!(
            ManualFields::from_capture(&detail.capture).title,
            fields().title
        );

        // Numbers are not reused after trashing.
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Document,
                id: second.document_id,
            })
            .unwrap();
        let third = store.create_manual_document(&fields()).unwrap();
        assert_eq!(
            store
                .load_document(third.document_id)
                .unwrap()
                .summary
                .document_number,
            format!("MANUAL-{year}/0003")
        );

        // Validation guides the person.
        let mut bad = fields();
        bad.title = "ab".into();
        assert!(store
            .create_manual_document(&bad)
            .unwrap_err()
            .to_string()
            .contains("título"));
        bad = fields();
        bad.document_date = "04/11/2026".into();
        assert!(store.create_manual_document(&bad).is_err());

        // The export tells manual records apart.
        let out = dir.path().join("csv");
        std::fs::create_dir_all(&out).unwrap();
        crate::export::export_to(&store, &out).unwrap();
        let csv = std::fs::read_to_string(out.join("documentos.csv")).unwrap();
        assert!(csv.contains(&format!(
            "MANUAL-{year}/0001;Ligação do gabinete sobre a reforma;Manual;"
        )));
    }
}
