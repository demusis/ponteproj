//! Explicit local relationships and immutable capture history.
use crate::{model::Capture, protection, storage::Store};
use anyhow::{ensure, Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Movement {
    pub scope: String,
    pub date: String,
    pub event: String,
    pub registered_unit: String,
    pub registered_person: String,
    pub attendant_unit: String,
    pub attendant_person: String,
    pub description: String,
}
pub fn movements(capture: &Capture) -> Result<Vec<Movement>> {
    if let Some(source) = capture.metadata.get("movement_source") {
        let compact: String = capture
            .document_number
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        ensure!(source==&format!("https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla={compact}"),"Fonte do histórico divergente do documento.");
    }
    let Some(json) = capture.metadata.get("movement_history") else {
        return Ok(vec![]);
    };
    ensure!(json.len() <= 60000, "Histórico excede 60 KB.");
    let events: Vec<Movement> =
        serde_json::from_str(json).context("Histórico de eventos inválido")?;
    ensure!(
        !events.is_empty() && events.len() <= 200,
        "Quantidade inválida de eventos."
    );
    let source = capture
        .metadata
        .get("movement_source")
        .context("Histórico sem fonte")?;
    let compact: String = capture
        .document_number
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    ensure!(source == &format!("https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla={compact}"), "Fonte do histórico divergente do documento.");
    for e in &events {
        ensure!(
            !e.scope.trim().is_empty() && !e.event.trim().is_empty(),
            "Evento sem identificação."
        );
        for text in [
            &e.scope,
            &e.date,
            &e.event,
            &e.registered_unit,
            &e.registered_person,
            &e.attendant_unit,
            &e.attendant_person,
            &e.description,
        ] {
            ensure!(
                text.len() <= 8000 && !text.chars().any(|c| c.is_control()),
                "Campo de evento inválido."
            );
        }
    }
    Ok(events)
}

/// Deterministic reading of an explicit SIGADOC "juntada"/"apensação".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JuntadaInfo {
    pub is_joined: bool,
    /// Canonical parent number, e.g. "SESP-DIC-2026/97546".
    pub parent_document_number: Option<String>,
    /// Number exactly as addressed by the source, e.g. "SESP-DIC-2026/97546-A".
    pub raw_target_via: Option<String>,
}

fn juntada_target_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?i)\b(?:juntad[oa]|apensad[oa])\s+(?:ao|à)\s+(?:documento|processo|expediente)?\s*(?:n[º°o.]\s*)?:?\s*([A-Z0-9][A-Z0-9./-]{3,80})")
            .expect("constant juntada pattern")
    })
}
fn juntada_undo_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?i)desentranh|desapens|desjunt|cancelamento d[ae] juntada")
            .expect("constant undo pattern")
    })
}
fn juntada_state_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| regex::Regex::new(r"(?i)juntad|apensad").expect("constant state"))
}
fn event_date(text: &str) -> Option<chrono::NaiveDateTime> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(
            r"\b(\d{1,2})/(\d{1,2})/(\d{2}|\d{4})(?:\s+(\d{1,2}):(\d{2})(?::(\d{2}))?)?",
        )
        .expect("constant date pattern")
    });
    let c = pattern.captures(text)?;
    let number = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u32>().ok());
    let mut year = number(3)? as i32;
    if year < 100 {
        year += 2000;
    }
    chrono::NaiveDate::from_ymd_opt(year, number(2)?, number(1)?)?.and_hms_opt(
        number(4).unwrap_or(0),
        number(5).unwrap_or(0),
        number(6).unwrap_or(0),
    )
}

/// Removes a SIGA via suffix (`-A`..`-Z`) and validates the official identity.
pub fn canonical_parent_number(raw: &str) -> Option<String> {
    let token = raw
        .trim()
        .trim_end_matches(['.', '-', '/', ',', ';', ':'])
        .to_uppercase();
    if crate::model::valid_document_number(&token) {
        return Some(token);
    }
    let (base, suffix) = token.rsplit_once('-')?;
    (suffix.len() == 1
        && suffix.as_bytes()[0].is_ascii_uppercase()
        && crate::model::valid_document_number(base))
    .then(|| base.to_owned())
}

fn juntada_target(text: &str) -> Option<(String, String)> {
    juntada_target_pattern()
        .captures_iter(text)
        .filter_map(|c| {
            let raw = c[1]
                .trim_end_matches(['.', '-', '/', ',', ';', ':'])
                .to_uppercase();
            canonical_parent_number(&raw).map(|number| (number, raw))
        })
        .next()
}

/// Sources, most specific first: imported event history, structured references,
/// rendered event table text and the via heading ("- Juntado"/"- Apensado").
pub fn detect_juntada(capture: &Capture) -> Option<JuntadaInfo> {
    // A manual record is free text typed by a person: "juntado ao processo X"
    // there is a note, not an official juntada. Links are made explicitly.
    if capture.is_manual() {
        return None;
    }
    struct Candidate {
        number: String,
        raw: String,
        date: Option<chrono::NaiveDateTime>,
    }
    let own = capture.document_number.trim().to_uppercase();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut undo_dates: Vec<Option<chrono::NaiveDateTime>> = Vec::new();
    let mut push = |found: Option<(String, String)>, date| {
        if let Some((number, raw)) = found.filter(|(number, _)| *number != own) {
            candidates.push(Candidate { number, raw, date });
        }
    };
    for event in movements(capture).unwrap_or_default() {
        let combined = format!("{} {}", event.event, event.description);
        if let Some(found) = juntada_target(&combined) {
            push(Some(found), event_date(&event.date));
        } else if juntada_undo_pattern().is_match(&event.event) {
            undo_dates.push(event_date(&event.date));
        }
    }
    for reference in &capture.references {
        let found = juntada_target(&reference.label).or_else(|| {
            if !juntada_state_pattern().is_match(&reference.label) {
                return None;
            }
            let url = url::Url::parse(&reference.url).ok()?;
            let sigla = url
                .query_pairs()
                .find(|(key, _)| key.eq_ignore_ascii_case("sigla"))?
                .1
                .into_owned();
            canonical_parent_number(&sigla).map(|number| (number, sigla.to_uppercase()))
        });
        push(found, None);
    }
    for line in capture.text.lines() {
        if let Some(found) = juntada_target(line) {
            push(Some(found), event_date(line));
        } else if juntada_undo_pattern().is_match(line) {
            undo_dates.push(event_date(line));
        }
    }
    let via_status = capture.metadata.get("via_status");
    let via_joined = via_status.is_some_and(|v| juntada_state_pattern().is_match(v));
    // The latest dated juntada wins; otherwise the most specific source.
    let chosen = candidates
        .iter()
        .filter(|c| c.date.is_some())
        .max_by_key(|c| c.date)
        .or_else(|| candidates.first());
    let undone = chosen.is_some_and(|c| {
        undo_dates.iter().any(|undo| match (c.date, undo) {
            (Some(joined), Some(undo)) => *undo > joined,
            // Undated undo: trust the current via heading when it is present.
            _ => via_status.is_some() && !via_joined,
        })
    });
    let target = chosen.filter(|_| !undone);
    if !via_joined && target.is_none() {
        return None;
    }
    Some(JuntadaInfo {
        is_joined: true,
        parent_document_number: target.map(|c| c.number.clone()),
        raw_target_via: target.map(|c| c.raw.clone()),
    })
}

pub fn show_history(ui: &mut eframe::egui::Ui, capture: &Capture, document: i64) {
    use eframe::egui;
    ui.group(|ui|{
        ui.set_width(ui.available_width());
        match movements(capture){
            Ok(events)=>{
                ui.strong(format!("Histórico importado do SIGADOC • {} evento(s)",events.len()));
                if events.is_empty(){ui.label("Nenhum histórico importado nesta captura. Reimporte com a extensão atualizada para tentar obter os registros.");return;}
                ui.small("Fonte: tabela de eventos da visualização detalhada. Ordem e datas preservadas como exibidas; não são calculadas a partir da data documental.");
                for (index,event) in events.iter().enumerate(){
                    ui.push_id(("history-record",document,index),|ui|{
                        ui.separator();
                        ui.horizontal_wrapped(|ui|{
                            ui.strong(&event.event);ui.label(if event.date.is_empty(){"Data não exibida neste registro"}else{&event.date});
                        });
                        ui.small(&event.scope);
                        ui.label(format!("Cadastrante: {} / {}",if event.registered_person.is_empty(){"não informado"}else{&event.registered_person},if event.registered_unit.is_empty(){"lotação não informada"}else{&event.registered_unit}));
                        ui.label(format!("Atendente: {} / {}",if event.attendant_person.is_empty(){"não informado"}else{&event.attendant_person},if event.attendant_unit.is_empty(){"lotação não informada"}else{&event.attendant_unit}));
                        if !event.description.is_empty(){ui.add(egui::Label::new(format!("Descrição: {}",event.description)).wrap());}
                    });
                }
                ui.small("Cadastrante e atendente mantêm os papéis escritos no histórico. Uma data vazia não foi completada por inferência.");
                if let Some(source)=capture.metadata.get("movement_source"){ui.hyperlink_to("Consultar histórico no SIGADOC",source);}
            },Err(error)=>{ui.label(format!("Não foi possível apresentar o histórico: {error}"));}
        }
    });
}

/// Kind of the automatic link: the parent is the antecedent of the juntado piece.
pub const JUNTADA_LINK_KIND: &str = "antecedente";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkPayload {
    pub kind: String,
    pub reason: String,
    pub created_at: String,
}
pub struct DocumentLink {
    pub id: i64,
    pub source: i64,
    pub target: i64,
    pub active: bool,
    pub payload: LinkPayload,
}
#[derive(Clone)]
pub struct CaptureVersion {
    pub id: i64,
    pub saved_at: String,
    pub capture: Capture,
}
impl Store {
    pub(crate) fn ensure_dossier(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS document_links(id INTEGER PRIMARY KEY AUTOINCREMENT,source_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,target_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,payload BLOB NOT NULL,kind_hash BLOB NOT NULL,active INTEGER NOT NULL DEFAULT 1,CHECK(source_id<>target_id)); CREATE INDEX IF NOT EXISTS links_source ON document_links(source_id); CREATE INDEX IF NOT EXISTS links_target ON document_links(target_id); CREATE UNIQUE INDEX IF NOT EXISTS links_unique_active ON document_links(source_id,target_id,kind_hash) WHERE active=1;")?;
        tx.commit()?;
        Ok(())
    }
    pub fn load_capture(&self, document: i64, capture: i64) -> Result<Capture> {
        let bytes: Vec<u8> = self.conn.query_row(
            "SELECT payload FROM captures WHERE id=?1 AND document_id=?2",
            params![capture, document],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_slice(&protection::unseal(&bytes)?)?)
    }
    /// Imported versions of a document, memoized until the library changes.
    pub fn capture_versions(&self, document: i64) -> Result<Vec<CaptureVersion>> {
        self.cached(&format!("capture_versions\u{1}{document}"), || {
            self.capture_versions_uncached(document)
        })
    }
    fn capture_versions_uncached(&self, document: i64) -> Result<Vec<CaptureVersion>> {
        let mut statement = self.conn.prepare(
            "SELECT c.id,c.created_at,c.payload FROM captures c WHERE c.document_id=?1 AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='capture' AND t.item_id=c.id) ORDER BY c.id DESC",
        )?;
        let rows = statement.query_map([document], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, saved_at, bytes) = row?;
            Ok(CaptureVersion {
                id,
                saved_at,
                capture: serde_json::from_slice(&protection::unseal(&bytes)?)?,
            })
        })
        .collect()
    }
    pub fn document_links(&self, document: i64) -> Result<Vec<DocumentLink>> {
        let mut statement=self.conn.prepare("SELECT id,source_id,target_id,active,payload FROM document_links WHERE (source_id=?1 OR target_id=?1) AND NOT EXISTS(SELECT 1 FROM trash t WHERE t.kind='document' AND t.item_id IN (source_id,target_id)) ORDER BY id DESC")?;
        let rows = statement.query_map([document], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, Vec<u8>>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (id, source, target, active, bytes) = row?;
            Ok(DocumentLink {
                id,
                source,
                target,
                active,
                payload: serde_json::from_slice(&protection::unseal(&bytes)?)?,
            })
        })
        .collect()
    }
    pub fn add_document_link(
        &mut self,
        source: i64,
        target: i64,
        kind: &str,
        reason: &str,
    ) -> Result<()> {
        ensure!(source != target, "Escolha outro documento.");
        ensure!(
            ["antecedente", "resposta", "relacionado"].contains(&kind),
            "Tipo de vínculo inválido."
        );
        ensure!(
            !reason.trim().is_empty() && reason.len() <= 4000,
            "Informe a base do vínculo (até 4000 bytes)."
        );
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::AddDocumentLink {
                    source,
                    target,
                    kind: kind.to_owned(),
                    reason: reason.to_owned(),
                },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        self.load_document(source)?;
        self.load_document(target)?;
        let existing = self.document_links(source)?;
        ensure!(
            existing.len() < 200,
            "Limite de 200 vínculos por documento."
        );
        ensure!(
            !existing.iter().any(|l| l.source == source
                && l.target == target
                && l.payload.kind == kind
                && l.active),
            "Vínculo já registrado."
        );
        let value = LinkPayload {
            kind: kind.into(),
            reason: reason.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(kind.as_bytes());
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for id in [source, target] {
            let count: i64 = tx.query_row(
                "SELECT count(*) FROM document_links WHERE source_id=?1 OR target_id=?1",
                [id],
                |r| r.get(0),
            )?;
            ensure!(count < 200, "Limite de 200 vínculos por documento.");
        }
        tx.execute(
            "INSERT INTO document_links(source_id,target_id,payload,kind_hash) VALUES(?1,?2,?3,?4)",
            params![
                source,
                target,
                protection::seal(&serde_json::to_vec(&value)?)?,
                hash.as_slice()
            ],
        )?;
        let link_id = tx.last_insert_rowid();
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "document_link",
            link_id,
            None,
            "add_document_link",
            Some(source),
            &format!("Registrou vínculo ({kind}) entre documentos #{source} e #{target}"),
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Active documents of this library explicitly juntados to `parent`.
    pub fn joined_children(&self, parent: i64) -> Result<Vec<crate::model::DocumentSummary>> {
        Ok(self
            .list_documents("")?
            .into_iter()
            .filter(|d| d.joined_to_id == Some(parent))
            .collect())
    }

    /// Creates the missing local link between each juntado document and its
    /// parent when both are active in this library, whatever the import order.
    /// Idempotent: any existing link between the pair (active or deactivated
    /// by the user, in either direction) is respected and never duplicated.
    pub(crate) fn link_juntadas(&mut self) -> Result<usize> {
        if self.client_config()?.is_some() {
            // Clients mirror the host library; the host creates these links.
            return Ok(0);
        }
        let pairs: Vec<(i64, i64, String)> = self
            .list_documents("")?
            .into_iter()
            .filter_map(|d| Some((d.id, d.joined_to_id?, d.joined_to_number?)))
            .collect();
        if pairs.is_empty() {
            return Ok(0);
        }
        let actor = self.active_actor();
        use sha2::{Digest, Sha256};
        let kind = JUNTADA_LINK_KIND;
        let hash = Sha256::digest(kind.as_bytes());
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut created = 0;
        for (child, parent, number) in pairs {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM document_links WHERE (source_id=?1 AND target_id=?2) OR (source_id=?2 AND target_id=?1))",
                params![child, parent],
                |r| r.get(0),
            )?;
            if exists {
                continue;
            }
            let full = [child, parent].iter().try_fold(false, |full, id| {
                tx.query_row(
                    "SELECT count(*) FROM document_links WHERE source_id=?1 OR target_id=?1",
                    [id],
                    |r| r.get::<_, i64>(0),
                )
                .map(|count| full || count >= 200)
            })?;
            if full {
                continue;
            }
            let value = LinkPayload {
                kind: kind.into(),
                reason: format!("Identificado automaticamente: Juntado ao documento {number}"),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            tx.execute(
                "INSERT INTO document_links(source_id,target_id,payload,kind_hash) VALUES(?1,?2,?3,?4)",
                params![
                    child,
                    parent,
                    protection::seal(&serde_json::to_vec(&value)?)?,
                    hash.as_slice()
                ],
            )?;
            let link_id = tx.last_insert_rowid();
            crate::intranet::record_mutation(
                &tx,
                &actor,
                "document_link",
                link_id,
                None,
                "auto_link_juntada",
                Some(child),
                &format!("Vínculo automático de juntada: documento #{child} juntado ao #{parent}"),
            )?;
            created += 1;
        }
        tx.commit()?;
        Ok(created)
    }
    pub fn set_link_active(&mut self, id: i64, active: bool) -> Result<()> {
        if let Some(cfg) = self.client_config()? {
            self.remote_mutate(
                &cfg,
                crate::intranet::IntranetMutation::SetLinkActive { id, active },
            )?;
            return Ok(());
        }
        let actor = self.active_actor();
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        ensure!(
            tx.execute(
                "UPDATE document_links SET active=?1 WHERE id=?2",
                params![active, id]
            )? == 1,
            "Vínculo não encontrado."
        );
        crate::intranet::record_mutation(
            &tx,
            &actor,
            "document_link",
            id,
            None,
            "set_link_active",
            None,
            &format!(
                "Vínculo #{id} {}",
                if active { "reativado" } else { "desativado" }
            ),
        )?;
        tx.commit()?;
        Ok(())
    }
}
/// Metadata differences preserve both sides, including deletions.
pub fn changed_metadata(
    old: &Capture,
    new: &Capture,
) -> Vec<(String, Option<String>, Option<String>)> {
    old.metadata
        .keys()
        .chain(new.metadata.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| old.metadata.get(*key) != new.metadata.get(*key))
        .map(|key| {
            (
                key.clone(),
                old.metadata.get(key).cloned(),
                new.metadata.get(key).cloned(),
            )
        })
        .collect()
}
#[derive(Default)]
struct LinkDraft {
    target: Option<i64>,
    kind: String,
    reason: String,
    message: String,
}
#[derive(Default)]
pub struct DossierPanel {
    trash_requested: Option<crate::trash::Target>,
    drafts: BTreeMap<i64, LinkDraft>,
}
impl DossierPanel {
    pub fn take_trash_request(&mut self) -> Option<crate::trash::Target> {
        self.trash_requested.take()
    }
    pub fn document_dirty(&self, id: i64) -> bool {
        self.drafts
            .get(&id)
            .is_some_and(|draft| draft.target.is_some() || !draft.reason.is_empty())
    }

    pub fn dirty(&self) -> bool {
        self.drafts
            .values()
            .any(|d| d.target.is_some() || !d.reason.is_empty())
    }
    pub fn dirty_documents(&self) -> Vec<i64> {
        self.drafts
            .iter()
            .filter(|(_, d)| d.target.is_some() || !d.reason.is_empty())
            .map(|(id, _)| *id)
            .collect()
    }
    pub fn forget_document(&mut self, id: i64) {
        self.drafts.remove(&id);
        for draft in self.drafts.values_mut() {
            if draft.target == Some(id) {
                draft.target = None;
            }
        }
    }
    pub fn show(
        &mut self,
        ui: &mut eframe::egui::Ui,
        store: &mut Store,
        document: i64,
    ) -> Option<i64> {
        use eframe::egui;
        let mut open = None;
        ui.horizontal(|ui| {
            ui.strong("Vínculos entre documentos");
            ui.label(eframe::egui::RichText::new("ⓘ").color(eframe::egui::Color32::from_rgb(120, 130, 140)))
                .on_hover_text("Vínculos confirmados por você ou criados automaticamente quando a página mostra juntada explícita (“Juntado ao documento”). Não substituem a consulta da tramitação oficial no SIGADOC.");
        });
        let documents = match store.list_documents("") {
            Ok(v) => v,
            Err(e) => {
                ui.label(format!("Falha ao ler biblioteca: {e}"));
                return None;
            }
        };
        let links = match store.document_links(document) {
            Ok(v) => v,
            Err(e) => {
                ui.label(format!("Falha ao ler vínculos: {e}"));
                return None;
            }
        };
        let draft = self.drafts.entry(document).or_default();
        let process = |id: i64| -> Result<(String, String)> {
            let detail = store.load_document(id)?;
            if !detail.work_context.process_number.trim().is_empty() {
                Ok((
                    detail.work_context.process_number.trim().to_uppercase(),
                    "informado pelo usuário".into(),
                ))
            } else {
                Ok((
                    detail
                        .capture
                        .metadata
                        .get("process_number")
                        .map(|p| p.trim().to_uppercase())
                        .unwrap_or_default(),
                    "campo explícito da captura".into(),
                ))
            }
        };
        if let Ok((number, origin)) = process(document) {
            if !number.is_empty() {
                ui.collapsing(format!("Mesmo processo informado: {number} ({origin})"),|ui|{
                ui.small("Coincidência do campo processo não confirma vínculo oficial. Confira e confirme vínculos locais quando apropriado.");
                for other in &documents{if other.id!=document{match process(other.id){Ok((candidate,source)) if candidate==number=>{
                    ui.label(format!("{} — {} — {}",other.document_number,other.title,source));
                    if ui.button(format!("Preparar vínculo com {}",other.document_number)).clicked(){if draft.target.is_some() || !draft.reason.is_empty(){draft.message="Confirme ou cancele o vínculo em edição antes de preparar outro.".into();}else{draft.target=Some(other.id);draft.kind="relacionado".into();draft.reason=format!("Mesmo processo no campo registrado: {number}. Conferido pelo usuário ao confirmar este vínculo.");}}
                },Err(e)=>{ui.label(format!("Não foi possível conferir {}: {e}",other.document_number));},_=>{}}}}
            });
            }
        }

        for link in links {
            let other = if link.source == document {
                link.target
            } else {
                link.source
            };
            let label = documents
                .iter()
                .find(|d| d.id == other)
                .map(|d| format!("{} — {}", d.document_number, d.title))
                .unwrap_or_else(|| "Documento não disponível".into());
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.strong(format!(
                    "{}{}",
                    match (link.payload.kind.as_str(), link.source == document) {
                        ("antecedente", true) => "Este documento vem depois de".to_owned(),
                        ("antecedente", false) => "Este documento vem antes de".to_owned(),
                        ("resposta", true) => "Este documento responde a".to_owned(),
                        ("resposta", false) => "Respondido por".to_owned(),
                        (kind, _) => crate::labels::link_kind(kind).to_owned(),
                    },
                    if link.active {
                        ""
                    } else {
                        " (vínculo desativado)"
                    }
                ));
                if ui.link(label).clicked() {
                    open = Some(other);
                }
                ui.label(&link.payload.reason);
                ui.small(format!(
                    "Confirmado localmente em {}",
                    link.payload.created_at
                ));
                if ui
                    .button(if link.active {
                        "Desativar vínculo local"
                    } else {
                        "Reativar vínculo local"
                    })
                    .clicked()
                {
                    match store.set_link_active(link.id, !link.active) {
                        Ok(()) => draft.message = "Vínculo atualizado; registro preservado.".into(),
                        Err(e) => draft.message = e.to_string(),
                    }
                }
            });
        }
        ui.collapsing("Confirmar novo vínculo",|ui|{
            egui::ComboBox::from_id_salt(("link-target",document)).selected_text(draft.target.and_then(|id|documents.iter().find(|d|d.id==id)).map(|d|d.document_number.as_str()).unwrap_or("Escolha outro documento")).show_ui(ui,|ui|{for d in &documents{if d.id!=document{ui.selectable_value(&mut draft.target,Some(d.id),format!("{} — {}",d.document_number,d.title));}}});
            egui::ComboBox::from_id_salt(("link-kind",document)).selected_text(if draft.kind.is_empty(){"Tipo de vínculo"}else{&draft.kind}).show_ui(ui,|ui|{for kind in ["antecedente","resposta","relacionado"]{ui.selectable_value(&mut draft.kind,kind.into(),kind);}});
            ui.small("Direção: o documento escolhido é antecedente, resposta ou relacionado ao documento atual.");
            ui.label("Base do vínculo (referência explícita ou confirmação pessoal)");ui.text_edit_multiline(&mut draft.reason);
            if ui.button("Confirmar vínculo local").clicked(){if let Some(target)=draft.target{match store.add_document_link(document,target,&draft.kind,&draft.reason){Ok(())=>{draft.target=None;draft.reason.clear();draft.message="Vínculo confirmado e salvo.".into();},Err(e)=>draft.message=e.to_string()}}else{draft.message="Escolha outro documento importado.".into();}}
            if ui.button("Cancelar novo vínculo").clicked(){draft.target=None;draft.reason.clear();draft.message.clear();}
        });
        if !draft.message.is_empty() {
            ui.label(&draft.message);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Versões importadas");
            ui.label(eframe::egui::RichText::new("ⓘ").color(eframe::egui::Color32::from_rgb(120, 130, 140)))
                .on_hover_text("Cada importação do mesmo documento fica guardada. A comparação mostra o que mudou entre importações; não comprova alteração do documento oficial.");
        });
        match store.capture_versions(document) {
            Ok(versions) => {
                for (index, version) in versions.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        if documents
                            .iter()
                            .find(|doc| doc.id == document)
                            .is_some_and(|doc| doc.latest_capture_id == version.id)
                        {
                            ui.strong("Captura em uso");
                        }
                        if ui
                            .small_button(format!("Excluir captura #{}…", version.id))
                            .clicked()
                        {
                            self.trash_requested = Some(crate::trash::Target {
                                kind: crate::trash::Kind::Capture,
                                id: version.id,
                            });
                        }
                    });
                    ui.collapsing(format!("Captura #{} • salva em {}",version.id,version.saved_at),|ui|{
                ui.label(format!("Obtida em {}",version.capture.captured_at));ui.label(format!("Fonte: {}",version.capture.source_url));
                if let Some(previous)=versions.get(index+1){
                    if previous.capture.title!=version.capture.title{ui.label(format!("Título: {} → {}",previous.capture.title,version.capture.title));}
                    for (key,old,new) in changed_metadata(&previous.capture,&version.capture){ui.collapsing(format!("Campo alterado: {key}"),|ui|{ui.label(format!("Antes: {}",old.as_deref().unwrap_or("ausente")));ui.label(format!("Depois: {}",new.as_deref().unwrap_or("ausente")));});}
                    ui.label(if previous.capture.text==version.capture.text{"Texto sem alteração"}else{"Texto diferente da captura anterior; consulte as duas versões abaixo"});
                    if previous.capture.references!=version.capture.references{ui.label("Referências diferentes da captura anterior");}
                    if previous.capture.text!=version.capture.text{ui.collapsing("Texto da captura anterior",|ui|{let mut text=previous.capture.text.as_str();ui.add(egui::TextEdit::multiline(&mut text).desired_rows(8).desired_width(f32::INFINITY));});}
                }else{ui.label("Primeira captura preservada.");}
                ui.collapsing("Texto desta captura",|ui|{let mut text=version.capture.text.as_str();ui.add(egui::TextEdit::multiline(&mut text).desired_rows(8).desired_width(f32::INFINITY));});
                match store.list_files(version.id){Ok(files)=>{for file in files{ui.label(format!("PDF: {} • SHA256 {}",file.label,file.sha256));}},Err(e)=>{ui.label(format!("Falha ao ler arquivos: {e}"));}}
            });
                }
            }
            Err(e) => {
                ui.label(format!("Falha ao ler capturas: {e}"));
            }
        }
        open
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const CHILD: &str = "SESP-DES-2026/39019";
    const PARENT: &str = "SESP-DIC-2026/97546";

    fn capture_for(number: &str, text: &str) -> Capture {
        let mut capture = crate::model::sample_capture();
        capture.document_number = number.into();
        capture.source_url = format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla={}-A",
            number.replace('/', "%2F")
        );
        capture.title = format!("Documento {number}");
        capture.text = text.into();
        capture
    }

    fn joined_child() -> Capture {
        let mut capture = capture_for(
            CHILD,
            "Despacho de encaminhamento.\n\n| Data | Lotação | Evento | Descrição |\n| 02/11/2026 10:15 | SESP-GAB | Juntada | Juntado ao documento: SESP-DIC-2026/97546-A |",
        );
        capture.metadata.insert(
            "via_status".into(),
            "1ª Via (Setor Competente) - Juntado".into(),
        );
        capture
    }

    #[test]
    fn detects_juntada_from_event_table_text_and_normalizes_the_via() {
        let info = detect_juntada(&joined_child()).unwrap();
        assert!(info.is_joined);
        assert_eq!(info.parent_document_number.as_deref(), Some(PARENT));
        assert_eq!(
            info.raw_target_via.as_deref(),
            Some("SESP-DIC-2026/97546-A")
        );
        assert_eq!(
            canonical_parent_number("SESP-DIC-2026/97546-A").as_deref(),
            Some(PARENT)
        );
        assert_eq!(canonical_parent_number("SESP-DIC-2026/97546-AA"), None);
        assert_eq!(canonical_parent_number("lixo"), None);

        // Without the via heading the explicit text is still enough.
        let mut plain = joined_child();
        plain.metadata.clear();
        assert_eq!(
            detect_juntada(&plain)
                .unwrap()
                .parent_document_number
                .as_deref(),
            Some(PARENT)
        );

        // Only the via heading: joined, parent unknown.
        let mut via_only = capture_for(CHILD, "Sem tabela de eventos.");
        via_only
            .metadata
            .insert("via_status".into(), "1ª Via - Apensado".into());
        let info = detect_juntada(&via_only).unwrap();
        assert!(info.is_joined && info.parent_document_number.is_none());

        // A structured reference produced by the extension.
        let mut by_reference = capture_for(CHILD, "");
        by_reference.references.push(crate::model::DocumentReference {
            label: "Juntado ao documento: SESP-DIC-2026/97546-A".into(),
            url: "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=SESP-DIC-2026%2F97546-A".into(),
            kind: "antecedent".into(),
        });
        by_reference.validate().unwrap();
        assert_eq!(
            detect_juntada(&by_reference)
                .unwrap()
                .parent_document_number
                .as_deref(),
            Some(PARENT)
        );

        // Imported event history (exibirAntigo).
        let mut by_history = capture_for(CHILD, "");
        let event = |name: &str, date: &str, description: &str| Movement {
            scope: "Geral".into(),
            date: date.into(),
            event: name.into(),
            registered_unit: String::new(),
            registered_person: String::new(),
            attendant_unit: String::new(),
            attendant_person: String::new(),
            description: description.into(),
        };
        by_history.metadata.insert(
            "movement_source".into(),
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla=SESPDES202639019".into(),
        );
        by_history.metadata.insert(
            "movement_history".into(),
            serde_json::to_string(&vec![event(
                "Juntada",
                "02/11/2026 10:15",
                "Juntado ao documento: SESP-DIC-2026/97546-A",
            )])
            .unwrap(),
        );
        by_history.validate().unwrap();
        assert_eq!(
            detect_juntada(&by_history)
                .unwrap()
                .parent_document_number
                .as_deref(),
            Some(PARENT)
        );
        // A later desentranhamento undoes it.
        by_history.metadata.insert(
            "movement_history".into(),
            serde_json::to_string(&vec![
                event("Desentranhamento", "03/11/2026 09:00", ""),
                event(
                    "Juntada",
                    "02/11/2026 10:15",
                    "Juntado ao documento: SESP-DIC-2026/97546-A",
                ),
            ])
            .unwrap(),
        );
        assert_eq!(detect_juntada(&by_history), None);

        // Never joined to itself; ordinary documents are not joined.
        assert_eq!(
            detect_juntada(&capture_for(
                PARENT,
                "Juntado ao documento: SESP-DIC-2026/97546-A"
            )),
            None
        );
        assert_eq!(detect_juntada(&crate::model::sample_capture()), None);
    }

    fn juntada_links(store: &Store, id: i64) -> Vec<DocumentLink> {
        store
            .document_links(id)
            .unwrap()
            .into_iter()
            .filter(|l| l.payload.reason.starts_with("Identificado automaticamente"))
            .collect()
    }

    #[test]
    fn juntada_link_is_created_automatically_in_both_import_orders_and_stays_encrypted() {
        for parent_first in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("juntada.sqlite3");
            let mut store = Store::open(&path).unwrap();
            let (parent, child) = if parent_first {
                let p = store
                    .import_capture(&capture_for(PARENT, "Processo principal."))
                    .unwrap();
                let summary = store.list_documents("").unwrap();
                assert_eq!(summary[0].joined_children_count, 0);
                let c = store.import_capture(&joined_child()).unwrap();
                (p.document_id, c.document_id)
            } else {
                let c = store.import_capture(&joined_child()).unwrap();
                let orphan = store.load_document(c.document_id).unwrap().summary;
                assert!(orphan.is_joined);
                assert_eq!(orphan.joined_to_number.as_deref(), Some(PARENT));
                assert_eq!(orphan.joined_to_id, None);
                assert!(juntada_links(&store, c.document_id).is_empty());
                let p = store
                    .import_capture(&capture_for(PARENT, "Processo principal."))
                    .unwrap();
                (p.document_id, c.document_id)
            };
            let links = juntada_links(&store, child);
            assert_eq!(links.len(), 1, "parent_first={parent_first}");
            assert_eq!((links[0].source, links[0].target), (child, parent));
            assert!(links[0].active);
            assert_eq!(links[0].payload.kind, JUNTADA_LINK_KIND);

            let documents = store.list_documents("").unwrap();
            let child_summary = documents.iter().find(|d| d.id == child).unwrap();
            assert_eq!(child_summary.joined_to_id, Some(parent));
            let parent_summary = documents.iter().find(|d| d.id == parent).unwrap();
            assert_eq!(parent_summary.joined_children_count, 1);
            assert_eq!(store.joined_children(parent).unwrap()[0].id, child);
            assert_eq!(
                store.load_document(child).unwrap().summary.joined_to_id,
                Some(parent)
            );

            // Idempotent: reimport and reopen never duplicate the link.
            let mut again = joined_child();
            again.request_id = uuid::Uuid::new_v4().to_string();
            again.text.push_str("\nNova observação.");
            store.import_capture(&again).unwrap();
            assert_eq!(store.link_juntadas().unwrap(), 0);
            // A link deactivated by the user is respected, not recreated.
            store.set_link_active(links[0].id, false).unwrap();
            drop(store);
            let mut store = Store::open(&path).unwrap();
            assert_eq!(store.link_juntadas().unwrap(), 0);
            let links = juntada_links(&store, child);
            assert_eq!(links.len(), 1);
            assert!(!links[0].active);

            store
                .conn
                .execute_batch("PRAGMA wal_checkpoint(FULL)")
                .unwrap();
            for entry in std::fs::read_dir(temp.path()).unwrap() {
                let bytes = std::fs::read(entry.unwrap().path()).unwrap();
                for secret in [
                    PARENT,
                    CHILD,
                    "Juntado ao documento",
                    "Identificado automaticamente",
                ] {
                    assert!(
                        !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                        "plaintext {secret} found"
                    );
                }
            }
        }
    }

    #[test]
    fn links_are_reconciled_lazily_when_the_store_is_opened() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lazy.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let parent = store
            .import_capture(&capture_for(PARENT, "Principal."))
            .unwrap();
        let child = store.import_capture(&joined_child()).unwrap();
        // Simulate a library created before automatic links existed.
        store
            .conn
            .execute("DELETE FROM document_links", [])
            .unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let links = juntada_links(&store, parent.document_id);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].source, child.document_id);
    }

    #[test]
    fn history_and_explicit_relationships_preserve_provenance() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let mut first: Capture =
            serde_json::from_str(include_str!("../tests/fixtures/capture.json")).unwrap();
        let a = store.import_capture(&first).unwrap();
        first.request_id = uuid::Uuid::new_v4().to_string();
        first
            .metadata
            .insert("origin".into(), "ORIGEM_PRIVADA_TESTE".into());
        store.import_capture(&first).unwrap();
        let history = store.capture_versions(a.document_id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(
            changed_metadata(&history[1].capture, &history[0].capture).len(),
            1
        );
        let mut second = first.clone();
        second.request_id = uuid::Uuid::new_v4().to_string();
        second.document_number = "TESTE-OFI-2026/00099".into();
        second.source_url="https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla=TESTE-OFI-2026%2F00099".into();
        let b = store.import_capture(&second).unwrap();
        assert!(store
            .add_document_link(a.document_id, a.document_id, "resposta", "base")
            .is_err());
        assert!(store
            .add_document_link(a.document_id, 999, "resposta", "base")
            .is_err());
        store
            .add_document_link(
                a.document_id,
                b.document_id,
                "resposta",
                "VINCULO_PRIVADO_TESTE",
            )
            .unwrap();
        assert!(store
            .add_document_link(a.document_id, b.document_id, "resposta", "Outra base")
            .is_err());
        let link = store.document_links(b.document_id).unwrap().remove(0);
        assert_eq!(link.payload.reason, "VINCULO_PRIVADO_TESTE");
        store.set_link_active(link.id, false).unwrap();
        assert!(!store.document_links(a.document_id).unwrap()[0].active);
        store.set_link_active(link.id, true).unwrap();
        let event = Movement {
            scope: "Geral".into(),
            date: String::new(),
            event: "Criação".into(),
            registered_unit: String::new(),
            registered_person: String::new(),
            attendant_unit: String::new(),
            attendant_person: String::new(),
            description: String::new(),
        };
        first.metadata.insert(
            "movement_history".into(),
            serde_json::to_string(&vec![event]).unwrap(),
        );
        assert!(movements(&first).is_err());
        let compact: String = first
            .document_number
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        first.metadata.insert("movement_source".into(),format!("https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla={compact}"));
        assert_eq!(movements(&first).unwrap()[0].date, "");
        store
            .conn
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .unwrap();
        let raw = std::fs::read(&path).unwrap();
        for secret in ["VINCULO_PRIVADO_TESTE", "ORIGEM_PRIVADA_TESTE"] {
            assert!(!raw
                .windows(secret.len())
                .any(|bytes| bytes == secret.as_bytes()));
        }
        store.delete_document(b.document_id).unwrap();
        assert!(store.document_links(a.document_id).unwrap().is_empty());
    }
}
