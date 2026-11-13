//! The "Conferência da minuta" panel: what the program checks by itself in a
//! draft (always there, recomputed only when the text changes) and, apart,
//! the opinion of the AI about the criteria of the work template.
//!
//! The verdict of a criterion is the AI's opinion; the only thing the
//! program confirms is that the passage the AI quoted is written in the
//! draft ("trecho encontrado na minuta").
use super::queue::{DraftSlot, PendingSend, QueueKey, QueuedJob, SendPlan};
use super::{display_date, pill, PonteApp, WorkKind, WorkResult};
use crate::ai::{conference_text_sha256, ConferJob, DraftConference, Reporter};
use crate::draft_check::{check_draft, DraftCheck, DraftCheckInput};
use crate::model::{Capture, DraftProvenance};
use crate::theme::{self, S1, S2};
use crate::widgets::{self, Tone};
use crate::work_templates::Intent;
use eframe::egui::{self, RichText};

/// The words of the panel the guide quotes.
pub(super) const TITLE: &str = "Conferência da minuta";
pub(super) const AUTOMATIC_TITLE: &str = "Conferido pelo Ponte";
pub(super) const OPINION_TITLE: &str = "Critérios do modelo de trabalho (opinião da IA)";
pub(super) const UNSUPPORTED_NOTE: &str = "O Ponte não achou este dado no documento, no contexto, nas fontes nem no texto anterior. Confira antes de usar.";
pub(super) const UNSUPPORTED_NONE: &str =
    "Todas as datas, horas, números e valores da minuta constam nas fontes.";
pub(super) const CHANGED: &str = "A minuta mudou depois da conferência.";
pub(super) const NO_TEMPLATE: &str = "Sem modelo de trabalho: só a conferência automática.";
pub(super) const QUOTE_PILL: &str = "trecho encontrado na minuta";

/// What the panel knows about one draft text.
pub(super) struct DraftReview {
    pub document_id: i64,
    /// The text the check ran on.
    pub text: String,
    /// The template version the draft was made with (uid, version).
    pub template_key: Option<(String, u32)>,
    pub template_name: String,
    pub criteria: Vec<String>,
    pub check: DraftCheck,
    /// The conference of this text, or the last one known for the document
    /// (then it is shown as outdated).
    pub conference: Option<DraftConference>,
}

/// The conference of the criteria against the current text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConferenceState {
    None,
    /// It judged exactly this text.
    Current,
    /// It judged another text: the draft changed after it.
    Changed,
}

/// Whether a conference still applies to the text now in the editor.
pub(super) fn conference_state(
    current_text_sha: &str,
    conference: Option<&DraftConference>,
) -> ConferenceState {
    match conference {
        None => ConferenceState::None,
        Some(c) if c.text_sha256 == current_text_sha => ConferenceState::Current,
        Some(_) => ConferenceState::Changed,
    }
}

/// The label and tone of a verdict. Anything unknown reads "Não conferido".
pub(super) fn verdict_label(verdict: &str) -> (&'static str, Tone) {
    match verdict {
        "atende" => ("Atende", Tone::Ok),
        "nao_atende" => ("Não atende", Tone::Warn),
        "nao_se_aplica" => ("Não se aplica", Tone::Neutral),
        _ => ("Não conferido", Tone::Neutral),
    }
}

/// How many criteria got each verdict: atende, não atende, não se aplica,
/// não conferido (anything else).
pub(super) fn conference_counts(conference: &DraftConference) -> (usize, usize, usize, usize) {
    let mut counts = (0, 0, 0, 0);
    for result in &conference.results {
        match result.verdict.as_str() {
            "atende" => counts.0 += 1,
            "nao_atende" => counts.1 += 1,
            "nao_se_aplica" => counts.2 += 1,
            _ => counts.3 += 1,
        }
    }
    counts
}

/// "2 atendem, 1 não atende, …" for the material window.
pub(super) fn counts_text(conference: &DraftConference) -> String {
    let (yes, no, na, unchecked) = conference_counts(conference);
    let mut parts = Vec::new();
    if yes > 0 {
        parts.push(if yes == 1 {
            "1 atende".to_owned()
        } else {
            format!("{yes} atendem")
        });
    }
    if no > 0 {
        parts.push(if no == 1 {
            "1 não atende".to_owned()
        } else {
            format!("{no} não atendem")
        });
    }
    if na > 0 {
        parts.push(format!("{na} não se aplica"));
    }
    if unchecked > 0 {
        parts.push(format!("{unchecked} não conferido"));
    }
    if parts.is_empty() {
        "nenhum critério".to_owned()
    } else {
        parts.join(", ")
    }
}

/// The one line on top of the automatic part: how many points the person
/// should look at, never a word like "aprovada".
pub(super) fn check_summary(check: &DraftCheck) -> (String, Tone) {
    let points = usize::from(!check.header)
        + usize::from(!check.cites_number)
        + check.placeholders.len()
        + check
            .sections
            .iter()
            .filter(|s| s.required && !s.found)
            .count()
        + check.unsupported.len()
        + check.notes.len();
    match points {
        0 => (
            "Nada a apontar na conferência automática".to_owned(),
            Tone::Ok,
        ),
        1 => ("1 ponto para conferir".to_owned(), Tone::Warn),
        n => (format!("{n} pontos para conferir"), Tone::Warn),
    }
}

/// Kind of data of an unsupported item, as the person reads it.
fn unsupported_kind(kind: &str) -> &str {
    match kind {
        "data" => "data",
        "hora" => "hora",
        "numero" => "número",
        "valor" => "valor",
        other => other,
    }
}

/// Brings `slot` up to date with `text`: the check runs again only when the
/// text, the document or the template changed (never per frame). A conference
/// of the same text is read from the library; otherwise the last one known
/// for the document stays, marked as outdated by [`conference_state`].
#[allow(clippy::too_many_arguments)]
pub(super) fn refresh_review(
    slot: &mut Option<DraftReview>,
    store: &crate::storage::Store,
    document_id: i64,
    text: &str,
    intent: Intent,
    previous_text: Option<&str>,
    provenance: Option<&DraftProvenance>,
    sources: &[crate::evidence::Source],
) {
    let template_key = provenance.and_then(|p| p.template_uid.clone().zip(p.template_version));
    if let Some(current) = slot {
        if current.document_id == document_id
            && current.text == text
            && current.template_key == template_key
        {
            return;
        }
    }
    let template = template_key.as_ref().and_then(|(uid, version)| {
        store
            .template_by_uid(uid)
            .ok()
            .flatten()
            .and_then(|found| found.version(*version).cloned().map(|v| (found.name, v)))
    });
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let extra: Vec<String> = provenance
        .map(|p| vec![p.request_guidance.clone()])
        .unwrap_or_default();
    let version = template.as_ref().map(|(_, v)| v);
    let check = match DraftCheckInput::for_draft(
        store,
        document_id,
        text,
        intent,
        previous_text,
        version,
        sources,
        &extra,
        &today,
        None,
    ) {
        Ok(input) => check_draft(&input),
        Err(_) => DraftCheck::default(),
    };
    let older = slot
        .take()
        .filter(|old| old.document_id == document_id)
        .and_then(|old| old.conference);
    let criteria: Vec<String> = version.map(|v| v.criteria.clone()).unwrap_or_default();
    let conference = store
        .draft_conference(document_id, &conference_text_sha256(text), &criteria)
        .or(older);
    *slot = Some(DraftReview {
        document_id,
        text: text.to_owned(),
        template_key,
        template_name: template
            .as_ref()
            .map(|(name, _)| name.clone())
            .unwrap_or_default(),
        criteria,
        check,
        conference,
    });
}

fn bullet(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(theme::SIZE_SMALL)
                .color(theme::c().muted),
        )
        .wrap(),
    );
}

fn status_row(ui: &mut egui::Ui, label: &str, text: &str, tone: Tone) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = S1 + 2.0;
        ui.label(RichText::new(format!("{label}:")).color(theme::c().muted));
        pill(ui, text, tone);
    });
}

/// What the panel asked.
pub(super) struct PanelAnswer {
    /// "Conferir a minuta" was pressed.
    pub confer: bool,
}

/// Draws the panel for `review`. `skipped` is the sentence the generation
/// left when it did not make the conference; `busy` says the conference is
/// running or waiting.
pub(super) fn draw_panel(
    ui: &mut egui::Ui,
    review: &DraftReview,
    skipped: Option<&str>,
    busy: bool,
) -> PanelAnswer {
    let mut answer = PanelAnswer { confer: false };
    ui.spacing_mut().item_spacing.y = S2;
    let check = &review.check;
    let (summary, tone) = check_summary(check);
    ui.horizontal_wrapped(|ui| {
        widgets::section(ui, AUTOMATIC_TITLE);
        pill(ui, summary, tone);
    });
    widgets::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        status_row(
            ui,
            "Cabeçalho “MINUTA PARA REVISÃO”",
            if check.header { "presente" } else { "ausente" },
            if check.header { Tone::Ok } else { Tone::Warn },
        );
        status_row(
            ui,
            "Número do documento citado",
            if check.cites_number { "sim" } else { "não" },
            if check.cites_number {
                Tone::Ok
            } else {
                Tone::Warn
            },
        );
        status_row(
            ui,
            "Campos a preencher",
            &check.placeholders.len().to_string(),
            if check.placeholders.is_empty() {
                Tone::Ok
            } else {
                Tone::Warn
            },
        );
        for placeholder in &check.placeholders {
            bullet(ui, &format!("• “…{}…”", placeholder.trim()));
        }
        if !check.sections.is_empty() {
            ui.add_space(S1);
            widgets::field_label(ui, "Seções do modelo");
            let mut ordered: Vec<_> = check.sections.iter().collect();
            ordered.sort_by_key(|s| !s.required);
            for section in ordered {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = S1 + 2.0;
                    ui.label(&section.name);
                    if section.required {
                        widgets::small_muted(ui, "obrigatória");
                    }
                    if section.found {
                        pill(ui, "encontrada", Tone::Ok);
                    } else {
                        pill(
                            ui,
                            "não encontrada",
                            if section.required {
                                Tone::Warn
                            } else {
                                Tone::Neutral
                            },
                        );
                    }
                });
            }
        }
        ui.add_space(S1);
        widgets::field_label(ui, "Dados que não constam nas fontes");
        if check.unsupported.is_empty() {
            bullet(ui, UNSUPPORTED_NONE);
        } else {
            for item in &check.unsupported {
                ui.add(
                    egui::Label::new(format!(
                        "{} {} — “{}”",
                        unsupported_kind(&item.kind),
                        item.value,
                        item.context
                    ))
                    .wrap(),
                );
            }
            bullet(ui, UNSUPPORTED_NOTE);
        }
        for note in &check.notes {
            bullet(ui, note);
        }
    });

    // The opinion of the AI, only when the template has criteria.
    if review.template_key.is_none() {
        widgets::small_muted(ui, NO_TEMPLATE);
        return answer;
    }
    if review.criteria.is_empty() {
        return answer;
    }
    ui.add_space(S1);
    widgets::section(ui, OPINION_TITLE);
    if !review.template_name.is_empty() {
        widgets::small_muted(ui, format!("Modelo de trabalho: {}", review.template_name));
    }
    let state = conference_state(
        &conference_text_sha256(&review.text),
        review.conference.as_ref(),
    );
    if let Some(conference) = &review.conference {
        widgets::small_muted(
            ui,
            format!(
                "Opinião da IA ({}), em {}. O Ponte só confirma que o trecho citado está na minuta.",
                conference.model,
                display_date(&conference.at)
            ),
        );
    }
    if state == ConferenceState::Changed {
        super::notice_line(ui, Tone::Warn, CHANGED);
    }
    if let Some(sentence) = skipped.filter(|_| state != ConferenceState::Current) {
        widgets::small_muted(ui, sentence);
    }
    widgets::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = S2;
        match &review.conference {
            Some(conference) => {
                for (index, result) in conference.results.iter().enumerate() {
                    if index > 0 {
                        ui.separator();
                    }
                    let (label, tone) = verdict_label(&result.verdict);
                    ui.horizontal_wrapped(|ui| {
                        pill(ui, label, tone);
                        ui.add(egui::Label::new(&result.criterion).wrap());
                    });
                    if !result.note.trim().is_empty() {
                        bullet(ui, result.note.trim());
                    }
                    if !result.quote.trim().is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Trecho da minuta: “{}”",
                                    result.quote.trim()
                                ))
                                .size(theme::SIZE_SMALL),
                            );
                            if result.quote_found {
                                pill(ui, QUOTE_PILL, Tone::Ok);
                            }
                        });
                    }
                }
            }
            None => {
                for criterion in &review.criteria {
                    ui.horizontal_wrapped(|ui| {
                        pill(ui, "Não conferido", Tone::Neutral);
                        ui.add(egui::Label::new(criterion).wrap());
                    });
                }
            }
        }
    });
    ui.horizontal(|ui| {
        answer.confer = ui
            .add_enabled(!busy, widgets::secondary("Conferir a minuta"))
            .on_hover_text("Pede à IA a opinião sobre os critérios do modelo de trabalho. Envia o texto da minuta ao mesmo destino da geração. Nada é salvo na minuta.")
            .on_disabled_hover_text("Já há uma conferência em andamento ou na fila.")
            .clicked();
    });
    answer
}

/// Whether a waiting conference still judges the text in front of the person.
pub(super) fn conference_still_current(queued_sha: &str, current_text: Option<&str>) -> bool {
    current_text.is_some_and(|text| conference_text_sha256(text) == queued_sha)
}

impl PonteApp {
    /// The text of a draft slot now (the editor's or the suggestion's).
    pub(super) fn slot_text(&self, slot: DraftSlot) -> Option<String> {
        match slot {
            DraftSlot::Editor(id) => self.editors.get(&id).map(|e| e.body.clone()),
            DraftSlot::Suggestion => self.suggestion.as_ref().map(|s| s.body.clone()),
        }
    }

    fn slot_review(&self, slot: DraftSlot) -> Option<&DraftReview> {
        match slot {
            DraftSlot::Editor(_) => self.review_editor.as_ref(),
            DraftSlot::Suggestion => self.review_suggestion.as_ref(),
        }
    }

    /// "Conferir a minuta": asks first when the text would leave this
    /// computer to a destination not yet agreed for this document, then
    /// starts (or queues) the job.
    pub(super) fn confer_request(&mut self, slot: DraftSlot, ctx: &egui::Context) {
        let Some(review) = self.slot_review(slot) else {
            return;
        };
        let document_id = review.document_id;
        let Some(detail) = self.detail.clone().filter(|d| d.summary.id == document_id) else {
            self.error("Abra o documento da minuta e repita o pedido.");
            return;
        };
        match self.send_plan(
            PendingSend::Conference(slot),
            &detail,
            crate::ai::DraftSends::default(),
        ) {
            SendPlan::Go => self.confer_now(slot, ctx),
            SendPlan::Ask(question) => self.send_confirm = Some(*question),
            SendPlan::Stop => {}
        }
    }

    /// Builds the job from the draft as it is now and starts it, or queues it.
    pub(super) fn confer_now(&mut self, slot: DraftSlot, ctx: &egui::Context) {
        let Some(review) = self.slot_review(slot) else {
            return;
        };
        let document_id = review.document_id;
        let text = review.text.clone();
        let criteria = review.criteria.clone();
        let Some(detail) = self.detail.clone().filter(|d| d.summary.id == document_id) else {
            self.error("Abra o documento da minuta e repita o pedido.");
            return;
        };
        let capture: Capture = detail.capture.clone();
        let Some(job) = ConferJob::new(&capture, &text, &criteria) else {
            self.error("O modelo de trabalho desta minuta não tem critérios para conferir.");
            return;
        };
        let settings = match self.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.error(error.to_string());
                self.open_ai_settings(ctx);
                return;
            }
        };
        let number = detail.summary.document_number.clone();
        let sha = conference_text_sha256(&text);
        let context = super::queue::note_and_context(&self.store, &self.editors, document_id);
        let consent = self.consent_of(&settings, document_id, &[]);
        self.run_ai_job(QueuedJob {
            key: QueueKey {
                kind: WorkKind::Conference,
                document_id,
            },
            label: format!("Conferindo a minuta de {number}…"),
            waiting: format!("conferir a minuta de {number}"),
            capture_id: detail.summary.latest_capture_id,
            context: context.unwrap_or_default(),
            consent,
            draft_guard: Some((slot, sha)),
            task: Box::new(move |reporter: Reporter| WorkResult::Conference {
                document_id,
                result: crate::ai::confer_draft(&settings, &job, &reporter)
                    .map_err(|e| e.to_string()),
            }),
            ctx: ctx.clone(),
        });
    }

    /// Whether the conference of this slot is running or waiting.
    pub(super) fn conference_busy(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|w| w.kind == WorkKind::Conference)
            || self
                .queued
                .as_ref()
                .is_some_and(|q| q.key.kind == WorkKind::Conference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::CriterionResult;
    use crate::draft_check::{SectionCheck, Unsupported};

    fn clean() -> DraftCheck {
        DraftCheck {
            header: true,
            cites_number: true,
            ..Default::default()
        }
    }

    #[test]
    fn the_summary_counts_the_points_and_never_says_approved() {
        let (text, tone) = check_summary(&clean());
        assert_eq!(text, "Nada a apontar na conferência automática");
        assert_eq!(tone, Tone::Ok);
        let mut one = clean();
        one.placeholders = vec!["[preencher] o prazo".into()];
        assert_eq!(
            check_summary(&one),
            ("1 ponto para conferir".into(), Tone::Warn)
        );
        let mut two = clean();
        two.header = false;
        two.unsupported = vec![Unsupported {
            kind: "data".into(),
            value: "25/11/2026".into(),
            context: "até 25/11/2026".into(),
        }];
        let (text, tone) = check_summary(&two);
        assert_eq!(text, "2 pontos para conferir");
        assert_eq!(tone, Tone::Warn);
        assert!(!text.to_lowercase().contains("aprovad"));
    }

    #[test]
    fn only_a_missing_required_section_is_a_point() {
        let mut check = clean();
        check.sections = vec![
            SectionCheck {
                name: "Fundamentação".into(),
                required: false,
                found: false,
            },
            SectionCheck {
                name: "Conclusão".into(),
                required: true,
                found: true,
            },
        ];
        assert_eq!(
            check_summary(&check).1,
            Tone::Ok,
            "an optional one missing is not a point"
        );
        check.sections[1].found = false;
        assert_eq!(check_summary(&check).0, "1 ponto para conferir");
    }

    fn conference(sha: &str) -> DraftConference {
        let result = |verdict: &str| CriterionResult {
            criterion: "c".into(),
            verdict: verdict.into(),
            ..Default::default()
        };
        DraftConference {
            results: vec![
                result("atende"),
                result("atende"),
                result("nao_atende"),
                result("nao_se_aplica"),
                result("nao_conferido"),
                result("algo inesperado"),
            ],
            text_sha256: sha.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_conference_applies_only_to_the_text_it_judged() {
        let sha = conference_text_sha256("Texto A");
        let made = conference(&sha);
        assert_eq!(
            conference_state(&sha, Some(&made)),
            ConferenceState::Current
        );
        assert_eq!(
            conference_state(&conference_text_sha256("Texto B"), Some(&made)),
            ConferenceState::Changed
        );
        assert_eq!(conference_state(&sha, None), ConferenceState::None);
    }

    #[test]
    fn a_waiting_conference_runs_only_for_the_text_it_was_asked_for() {
        let sha = conference_text_sha256("Texto A");
        assert!(conference_still_current(&sha, Some("Texto A")));
        assert!(
            conference_still_current(&sha, Some("  Texto A \n")),
            "white space around the text does not count"
        );
        assert!(!conference_still_current(&sha, Some("Texto A, editado")));
        assert!(!conference_still_current(&sha, None), "the draft is gone");
    }

    #[test]
    fn verdicts_read_in_words_and_an_unknown_one_is_not_checked() {
        assert_eq!(verdict_label("atende"), ("Atende", Tone::Ok));
        assert_eq!(verdict_label("nao_atende"), ("Não atende", Tone::Warn));
        assert_eq!(
            verdict_label("nao_se_aplica"),
            ("Não se aplica", Tone::Neutral)
        );
        assert_eq!(
            verdict_label("nao_conferido"),
            ("Não conferido", Tone::Neutral)
        );
        assert_eq!(
            verdict_label("qualquer coisa"),
            ("Não conferido", Tone::Neutral)
        );
        assert_eq!(verdict_label(""), ("Não conferido", Tone::Neutral));
    }

    #[test]
    fn the_counts_say_how_each_criterion_came_out() {
        let made = conference("x");
        assert_eq!(conference_counts(&made), (2, 1, 1, 2));
        assert_eq!(
            counts_text(&made),
            "2 atendem, 1 não atende, 1 não se aplica, 2 não conferido"
        );
        assert_eq!(counts_text(&DraftConference::default()), "nenhum critério");
    }

    #[test]
    fn the_decided_words_are_there() {
        assert_eq!(CHANGED, "A minuta mudou depois da conferência.");
        assert_eq!(QUOTE_PILL, "trecho encontrado na minuta");
        assert_eq!(
            NO_TEMPLATE,
            "Sem modelo de trabalho: só a conferência automática."
        );
        assert!(UNSUPPORTED_NOTE.starts_with("O Ponte não achou este dado no documento"));
    }
}
