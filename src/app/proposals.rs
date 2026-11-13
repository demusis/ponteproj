//! The "Propostas" block of the analysis card, and the window "Material
//! utilizado" of an analysis.
//!
//! The model only proposes; the program checks the quotes, the dates and the
//! resources, and every button here opens a form pre-filled and unsaved. What
//! the block needs from the library (the document's atendimento, the records
//! made from each proposal, whether a reservation is free) is read once per
//! analysis load or refresh into a [`ProposalView`], never per frame.
use super::{display_date, facts, pill, PonteApp};
use crate::evidence::recheck_analysis;
use crate::model::{
    Analysis, AttendanceProposal, DocumentDetail, Fact, FactCheck, Origin, ReservationProposal,
    TaskProposal,
};
use crate::resources::now_moment;
use crate::resources_ui::{check_reservation, Availability, ReservationCheck};
use crate::saiop::{attendance_changes, AttendanceChange, SaiopAttendance};
use crate::storage::Store;
use crate::tasks::Task;
use crate::theme::{self, S1, S2, S3};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};
use std::collections::{BTreeMap, BTreeSet};

/// The record a proposal was already turned into.
#[derive(Clone, Debug)]
pub(super) enum Used {
    Task(Box<Task>),
    Attendance(i64),
    Use(String),
}

/// What the Propostas block needs from the library, computed once.
pub(super) struct ProposalView {
    /// The analysis it was read for (its date), so another one rebuilds it.
    pub created_at: String,
    /// The analysis as the card shows it: checked again against the document
    /// now (nothing is written to the library), with what no longer holds
    /// cleared.
    pub analysis: Analysis,
    /// How many facts or filled-in values the new check cleared or removed.
    pub cleared: u32,
    /// Tasks of the document (the old block looks for its suggestions here).
    pub document_tasks: Vec<Task>,
    /// The document's atendimento, when it has one.
    pub attendance: Option<SaiopAttendance>,
    /// What the attendance proposal would change in it.
    pub attendance_changes: Vec<AttendanceChange>,
    /// Records made from a proposal, by the proposal's id.
    pub used: BTreeMap<String, Used>,
    /// The register check of each reservation proposal, by id.
    pub reservations: BTreeMap<String, ReservationCheck>,
}

impl ProposalView {
    /// Reads the library for the analysis of `detail`; `None` without one.
    pub(super) fn build(store: &Store, detail: &DocumentDetail) -> Option<Self> {
        let (cleaned, cleared) = cleaned_analysis(detail.analysis.as_ref()?, &detail.capture.text);
        let analysis = &cleaned;
        let document_id = detail.summary.id;
        let tasks = store.list_tasks().unwrap_or_default();
        let attendances = store.list_saiop_attendances().unwrap_or_default();
        let uses = store.list_resource_uses().unwrap_or_default();
        let ids: BTreeSet<&str> = analysis.proposals.iter().flat_map(proposal_ids).collect();
        let used = used_records(&tasks, &attendances, &uses, &ids);
        let document_tasks: Vec<Task> = tasks
            .into_iter()
            .filter(|task| task.document_id == Some(document_id))
            .collect();
        let mine: Vec<SaiopAttendance> = attendances
            .into_iter()
            .filter(|a| a.document_id == Some(document_id))
            .collect();
        let attendance = pick_attendance(&mine).cloned();
        let attendance_changes = match (
            &attendance,
            analysis
                .proposals
                .as_ref()
                .and_then(|p| p.attendance.as_ref()),
        ) {
            (Some(current), Some(proposal)) => attendance_changes(current, proposal),
            _ => Vec::new(),
        };
        let mut reservations = BTreeMap::new();
        if let Some(proposals) = &analysis.proposals {
            if !proposals.reservations.is_empty() {
                let resources = store.list_resources().unwrap_or_default();
                let now = now_moment();
                for proposal in &proposals.reservations {
                    reservations.insert(
                        proposal.id.clone(),
                        check_reservation(proposal, &resources, &uses, &now),
                    );
                }
            }
        }
        Some(Self {
            created_at: analysis.created_at.clone(),
            analysis: cleaned.clone(),
            cleared,
            document_tasks,
            attendance,
            attendance_changes,
            used,
            reservations,
        })
    }
}

/// The analysis checked again against the text of the document as it is now,
/// and how many things that check cleared or removed. The stored analysis is
/// never touched: this works on a copy.
pub(super) fn cleaned_analysis(analysis: &Analysis, capture_text: &str) -> (Analysis, u32) {
    let mut copy = analysis.clone();
    let context = copy.human_context.clone();
    let cleared = recheck_analysis(&mut copy, capture_text, &context);
    (copy, cleared)
}

/// The words that say a proposal's data no longer match the document.
pub(super) fn cleared_text(cleared: u32) -> String {
    if cleared == 1 {
        "1 dado desta análise não confere mais com o documento e foi retirado. Analise de novo para atualizar.".to_owned()
    } else {
        format!("{cleared} dados desta análise não conferem mais com o documento e foram retirados. Analise de novo para atualizar.")
    }
}

/// The ids of every proposal of an analysis.
fn proposal_ids(proposals: &crate::model::Proposals) -> Vec<&str> {
    proposals
        .tasks
        .iter()
        .map(|p| p.id.as_str())
        .chain(proposals.attendance.iter().map(|p| p.id.as_str()))
        .chain(proposals.reservations.iter().map(|p| p.id.as_str()))
        .collect()
}

/// The records that carry the id of one of `ids`, by id (the first of each
/// kind found wins: a task, then an atendimento, then a use).
pub(super) fn used_records(
    tasks: &[Task],
    attendances: &[SaiopAttendance],
    uses: &[crate::resources::ResourceUse],
    ids: &BTreeSet<&str>,
) -> BTreeMap<String, Used> {
    let mut out = BTreeMap::new();
    for use_ in uses {
        if ids.contains(use_.proposal_id.as_str()) {
            out.insert(use_.proposal_id.clone(), Used::Use(use_.uid.clone()));
        }
    }
    for attendance in attendances {
        for id in &attendance.proposal_ids {
            if ids.contains(id.as_str()) {
                out.insert(id.clone(), Used::Attendance(attendance.id));
            }
        }
    }
    for task in tasks {
        if ids.contains(task.proposal_id.as_str()) {
            out.insert(task.proposal_id.clone(), Used::Task(Box::new(task.clone())));
        }
    }
    out
}

/// The atendimento a document's proposal completes: an open one before a
/// closed one, the newest first.
pub(super) fn pick_attendance(mine: &[SaiopAttendance]) -> Option<&SaiopAttendance> {
    mine.iter()
        .filter(|a| a.is_open())
        .max_by_key(|a| a.id)
        .or_else(|| mine.iter().max_by_key(|a| a.id))
}

/// What a button of the block asks the window to do.
#[derive(Clone, Debug)]
pub(super) enum ProposalAction {
    CreateTask(TaskProposal),
    OpenTask(Box<Task>),
    CompleteAttendance {
        id: i64,
        proposal: AttendanceProposal,
    },
    NewAttendance(AttendanceProposal),
    OpenAttendance(i64),
    Reserve {
        proposal: ReservationProposal,
        resource_uid: Option<String>,
    },
    OpenUse(String),
    RegisterResource(String),
}

fn fact_of<'a>(analysis: &'a Analysis, id: &str) -> Option<&'a Fact> {
    analysis.facts.iter().find(|fact| fact.id == id)
}

/// What the hover of the pill "consta no trecho" says.
pub(super) const PILL_HOVER: &str = "O programa encontrou este dado escrito no trecho citado. Confira se é mesmo o prazo, o horário ou a quantidade que o documento quer dizer.";

/// The pill's words.
pub(super) const PILL_TEXT: &str = "consta no trecho";

/// The line under the title of the block.
pub(super) const INTRO_TEXT: &str =
    "A IA propõe; o Ponte só marca o que encontrou escrito no documento. Revise antes de salvar.";

/// The note of a "Recebido de" the program could not find written.
pub(super) const UNCHECKED_SENDER: &str = "(citado no documento; confira)";

/// What follows a "Recebido de" the program did not find written (`None`:
/// found, the pill shows instead).
pub(super) fn sender_note(checked: bool) -> Option<&'static str> {
    (!checked).then_some(UNCHECKED_SENDER)
}

/// Whether a value may carry the pill: its fact's quote was found written in
/// the document or in a source (never only in the person's own context), and
/// that very value was found in the quote.
fn backs(fact: Option<&Fact>, found: impl Fn(&FactCheck) -> bool) -> bool {
    fact.is_some_and(|f| f.checked.quote && f.checked.origin.is_written() && found(&f.checked))
}

/// "Trecho: “…”".
fn quote_text(fact: &Fact) -> String {
    format!("Trecho: “{}”", fact.quote.trim())
}

/// A label–value line; `checked` adds the pill "consta no trecho".
fn field_line(ui: &mut egui::Ui, label: &str, value: &str, checked: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = S1 + 2.0;
        ui.label(RichText::new(format!("{label}:")).color(theme::c().muted));
        ui.label(value);
        if checked {
            pill(ui, PILL_TEXT, Tone::Ok).on_hover_text(PILL_HOVER);
        }
    });
}

/// Where the program found the excerpt of a fact.
pub(super) fn origin_text(fact: &Fact, analysis: &Analysis) -> String {
    match fact.checked.origin {
        Origin::Document => "no documento".to_owned(),
        Origin::Source => {
            let label = analysis
                .materials
                .iter()
                .find(|s| s.id == fact.source_id)
                .map(|s| s.label.as_str())
                .unwrap_or(fact.source_id.as_str());
            format!("na fonte: {label}")
        }
        Origin::Context => "só no seu contexto (informado por você)".to_owned(),
        Origin::Nowhere => "não encontrado no documento".to_owned(),
    }
}

/// The line "Encontrado: …" of a fact; what is only in the person's context
/// is told apart.
fn origin_line(ui: &mut egui::Ui, fact: &Fact, analysis: &Analysis) {
    let color = if fact.checked.origin == Origin::Context || !fact.checked.quote {
        Tone::Warn.colors().0
    } else {
        theme::c().muted
    };
    ui.add(
        egui::Label::new(
            RichText::new(format!("Encontrado: {}", origin_text(fact, analysis)))
                .size(theme::SIZE_SMALL)
                .color(color),
        )
        .wrap(),
    );
}

fn muted_line(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(theme::SIZE_SMALL)
                .color(theme::c().muted),
        )
        .wrap(),
    );
}

/// The quotes that support a proposal, folded away.
fn quotes_fold(ui: &mut egui::Ui, id: &str, analysis: &Analysis, ids: &[&str]) {
    let found: Vec<&Fact> = ids
        .iter()
        .filter_map(|fact_id| fact_of(analysis, fact_id))
        .collect();
    if found.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(RichText::new("Trechos do documento").size(theme::SIZE_SMALL))
        .id_salt(("proposal-quotes", id))
        .show(ui, |ui| {
            for fact in found {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "• Resumo da IA (não conferido): {}",
                            fact.statement
                        ))
                        .size(theme::SIZE_SMALL),
                    )
                    .wrap(),
                );
                muted_line(ui, &quote_text(fact));
                origin_line(ui, fact, analysis);
            }
        });
}

/// A proposal as a framed row: `body` draws the text, `actions` the buttons
/// (right-aligned, at the top).
fn row(ui: &mut egui::Ui, actions: impl FnOnce(&mut egui::Ui), body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::c().sunken)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    actions(ui);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        body(ui);
                    });
                });
            });
        });
}

fn heading_line(ui: &mut egui::Ui, kind: &str, title: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = S2;
        pill(ui, kind, Tone::Neutral);
        ui.add(
            egui::Label::new(RichText::new(title).font(theme::font_strong(theme::SIZE_BODY)))
                .wrap(),
        );
    });
}

/// The block "Propostas (revise antes de usar)". Returns what a button asked.
pub(super) fn proposals_block(
    ui: &mut egui::Ui,
    analysis: &Analysis,
    view: Option<&ProposalView>,
    stale: bool,
) -> Option<ProposalAction> {
    // What is shown is the analysis checked again against the document now.
    let analysis = view.map(|v| &v.analysis).unwrap_or(analysis);
    let proposals = analysis.proposals.as_ref()?;
    let mut action = None;
    ui.separator();
    widgets::field_label(ui, "Propostas (revise antes de usar)");
    widgets::small_muted(ui, INTRO_TEXT);
    if stale {
        widgets::small_muted(
            ui,
            "Os botões voltam quando a análise for refeita: o documento, o contexto ou as notas mudaram.",
        );
    }
    if proposals.is_empty() {
        muted_line(
            ui,
            &format!("A IA não propôs nada: {}", proposals.nothing_reason.trim()),
        );
        if !proposals.nothing_reason_ai.trim().is_empty() {
            muted_line(
                ui,
                &format!(
                    "Motivo dado pela IA: “{}”",
                    proposals.nothing_reason_ai.trim()
                ),
            );
        }
        return None;
    }
    ui.spacing_mut().item_spacing.y = S2;
    for proposal in &proposals.tasks {
        if let Some(chosen) = task_row(ui, analysis, view, proposal, stale) {
            action = Some(chosen);
        }
    }
    if let Some(proposal) = &proposals.attendance {
        if let Some(chosen) = attendance_row(ui, analysis, view, proposal, stale) {
            action = Some(chosen);
        }
    }
    for proposal in &proposals.reservations {
        if let Some(chosen) = reservation_row(ui, analysis, view, proposal, stale) {
            action = Some(chosen);
        }
    }
    action
}

fn task_row(
    ui: &mut egui::Ui,
    analysis: &Analysis,
    view: Option<&ProposalView>,
    proposal: &TaskProposal,
    stale: bool,
) -> Option<ProposalAction> {
    let action = std::cell::Cell::new(None::<ProposalAction>);
    let used = view.and_then(|v| v.used.get(&proposal.id));
    row(
        ui,
        |ui| match used {
            Some(Used::Task(task)) => {
                if ui
                    .add_enabled(!stale, widgets::quiet("Preparar outra"))
                    .on_hover_text("Cria mais uma tarefa com esta mesma proposta.")
                    .clicked()
                {
                    action.set(Some(ProposalAction::CreateTask(proposal.clone())));
                }
                if ui.add(widgets::quiet("Abrir tarefa")).clicked() {
                    action.set(Some(ProposalAction::OpenTask(task.clone())));
                }
            }
            _ => {
                if ui
                    .add_enabled(!stale, widgets::secondary("Criar tarefa"))
                    .on_hover_text("Abre o formulário da tarefa já preenchido. Nada é salvo antes do seu Salvar.")
                    .clicked()
                {
                    action.set(Some(ProposalAction::CreateTask(proposal.clone())));
                }
            }
        },
        |ui| {
            heading_line(ui, "Tarefa", &proposal.title);
            if let Some(Used::Task(task)) = used {
                ui.horizontal_wrapped(|ui| {
                    pill(ui, "Tarefa criada", Tone::Ok);
                    widgets::small_muted(ui, task.title.as_str());
                });
            }
            field_line(
                ui,
                "Providência",
                crate::labels::task_action(&proposal.action),
                false,
            );
            field_line(
                ui,
                "Prioridade",
                crate::labels::priority(&proposal.priority),
                false,
            );
            if let Some(date) = &proposal.due_date {
                let source = proposal
                    .due_fact
                    .as_deref()
                    .and_then(|id| fact_of(analysis, id));
                field_line(
                    ui,
                    "Prazo",
                    &crate::labels::date(date),
                    backs(source, |c| c.date),
                );
                if !proposal.due_source.trim().is_empty() {
                    muted_line(
                        ui,
                        &format!("Origem do prazo: “{}”", proposal.due_source.trim()),
                    );
                }
            }
            if !proposal.remark.trim().is_empty() {
                muted_line(ui, proposal.remark.trim());
            }
            if !proposal.reason.trim().is_empty() {
                muted_line(ui, &format!("Motivo: {}", proposal.reason.trim()));
            }
            let ids: Vec<&str> = proposal.facts.iter().map(String::as_str).collect();
            quotes_fold(ui, &proposal.id, analysis, &ids);
        },
    );
    action.into_inner()
}

fn attendance_row(
    ui: &mut egui::Ui,
    analysis: &Analysis,
    view: Option<&ProposalView>,
    proposal: &AttendanceProposal,
    stale: bool,
) -> Option<ProposalAction> {
    let action = std::cell::Cell::new(None::<ProposalAction>);
    let used = view.and_then(|v| v.used.get(&proposal.id));
    let existing = view.and_then(|v| v.attendance.as_ref());
    let changes: &[AttendanceChange] = view.map(|v| v.attendance_changes.as_slice()).unwrap_or(&[]);
    row(
        ui,
        |ui| {
            match (used, existing) {
            (Some(Used::Attendance(id)), _) => {
                if ui.add(widgets::quiet("Abrir")).clicked() {
                    action.set(Some(ProposalAction::OpenAttendance(*id)));
                }
            }
            (_, Some(current)) if !changes.is_empty() => {
                if ui
                    .add_enabled(!stale, widgets::secondary("Completar o atendimento"))
                    .on_hover_text("Abre o atendimento com estas alterações no formulário. Nada é salvo antes do seu Salvar.")
                    .clicked()
                {
                    action.set(Some(ProposalAction::CompleteAttendance {
                        id: current.id,
                        proposal: proposal.clone(),
                    }));
                }
            }
            (_, Some(current)) => {
                if ui.add(widgets::quiet("Abrir")).clicked() {
                    action.set(Some(ProposalAction::OpenAttendance(current.id)));
                }
            }
            (_, None) => {
                if ui
                    .add_enabled(!stale, widgets::secondary("Nova entrada"))
                    .on_hover_text("Abre uma entrada nova no controle de atendimentos, já preenchida. Nada é salvo antes do seu Salvar.")
                    .clicked()
                {
                    action.set(Some(ProposalAction::NewAttendance(proposal.clone())));
                }
            }
        }
        },
        |ui| {
            let title = match existing {
                Some(_) => "Completar o atendimento",
                None => "Nova entrada no controle de atendimentos",
            };
            heading_line(ui, "Atendimento", title);
            if matches!(used, Some(Used::Attendance(_))) {
                pill(ui, "Atendimento atualizado", Tone::Ok);
            } else if let Some(current) = existing {
                if changes.is_empty() {
                    muted_line(ui, "O atendimento já tem estes dados: nada a completar.");
                } else {
                    widgets::small_muted(
                        ui,
                        format!(
                            "Mudaria em {}:",
                            if current.reference_raw.is_empty() {
                                "este atendimento"
                            } else {
                                current.reference_raw.as_str()
                            }
                        ),
                    );
                    for change in changes {
                        ui.add(
                            egui::Label::new(format!(
                                "{}: {} → {}",
                                change.label, change.current, change.proposed
                            ))
                            .wrap(),
                        );
                    }
                }
            } else {
                new_entry_lines(ui, analysis, proposal);
            }
            if !proposal.remark.trim().is_empty() {
                muted_line(ui, proposal.remark.trim());
            }
            let ids: Vec<&str> = proposal.facts.iter().map(String::as_str).collect();
            quotes_fold(ui, &proposal.id, analysis, &ids);
        },
    );
    action.into_inner()
}

/// The fields a new entry would be born with.
fn new_entry_lines(ui: &mut egui::Ui, analysis: &Analysis, proposal: &AttendanceProposal) {
    if !proposal.subject.trim().is_empty() {
        field_line(ui, "Assunto", proposal.subject.trim(), false);
    }
    if !proposal.received_from.trim().is_empty() {
        received_from_line(ui, proposal);
    }
    if let Some(date) = &proposal.due_date {
        let source = proposal
            .due_fact
            .as_deref()
            .and_then(|id| fact_of(analysis, id));
        field_line(
            ui,
            "Prazo",
            &format!(
                "{} ({})",
                crate::labels::date(date),
                crate::saiop::due_kind_label(&proposal.due_kind).to_lowercase()
            ),
            backs(source, |c| c.date),
        );
    }
    if !proposal.priority.is_empty() {
        let mut text = crate::saiop::priority_label(&proposal.priority).to_owned();
        if !proposal.priority_reason.trim().is_empty() {
            text.push_str(&format!(" — {}", proposal.priority_reason.trim()));
        }
        field_line(ui, "Prioridade", &text, false);
    }
    if !proposal.planned_doc_type.is_empty() {
        field_line(ui, "Entrega prevista", &proposal.planned_doc_type, false);
    }
}

/// "Recebido de": the pill only when the program found the name written;
/// otherwise the value with the note to check it.
fn received_from_line(ui: &mut egui::Ui, proposal: &AttendanceProposal) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = S1 + 2.0;
        ui.label(RichText::new("Recebido de:").color(theme::c().muted));
        ui.label(proposal.received_from.trim());
        match sender_note(proposal.received_from_checked) {
            None => {
                pill(ui, PILL_TEXT, Tone::Ok).on_hover_text(PILL_HOVER);
            }
            Some(note) => {
                widgets::small_muted(ui, note);
            }
        }
    });
}

fn reservation_row(
    ui: &mut egui::Ui,
    analysis: &Analysis,
    view: Option<&ProposalView>,
    proposal: &ReservationProposal,
    stale: bool,
) -> Option<ProposalAction> {
    let action = std::cell::Cell::new(None::<ProposalAction>);
    let used = view.and_then(|v| v.used.get(&proposal.id));
    let check = view.and_then(|v| v.reservations.get(&proposal.id));
    row(
        ui,
        |ui| {
            match used {
            Some(Used::Use(uid)) => {
                if ui.add(widgets::quiet("Abrir")).clicked() {
                    action.set(Some(ProposalAction::OpenUse(uid.clone())));
                }
            }
            _ => {
                if ui
                    .add_enabled(!stale, widgets::secondary("Reservar"))
                    .on_hover_text("Abre a reserva já preenchida e ligada a este documento. Nada é salvo antes do seu Salvar.")
                    .clicked()
                {
                    action.set(Some(ProposalAction::Reserve {
                        proposal: proposal.clone(),
                        resource_uid: check.and_then(|c| c.resource_uid.clone()),
                    }));
                }
            }
        }
        },
        |ui| {
            heading_line(ui, "Reserva", proposal.what.trim());
            if matches!(used, Some(Used::Use(_))) {
                pill(ui, "Reserva criada", Tone::Ok);
            }
            let what_fact = fact_of(analysis, &proposal.what_fact);
            field_line(
                ui,
                "O que foi pedido",
                proposal.what.trim(),
                backs(what_fact, |c| c.quote),
            );
            let quantity_fact = std::iter::once(proposal.what_fact.as_str())
                .chain(proposal.facts.iter().map(String::as_str))
                .filter_map(|id| fact_of(analysis, id))
                .find(|fact| fact.checked.quantity && fact.quantity == Some(proposal.quantity));
            field_line(
                ui,
                "Quantidade",
                &proposal.quantity.to_string(),
                backs(quantity_fact, |c| c.quantity),
            );
            let when = proposal
                .when_fact
                .as_deref()
                .and_then(|id| fact_of(analysis, id));
            match &proposal.start {
                Some(start) => field_line(
                    ui,
                    "Início",
                    &crate::resources::moment_label(start),
                    backs(when, |c| c.date && c.time),
                ),
                None => muted_line(ui, "Início: o documento não traz data e hora certas."),
            }
            match (&proposal.start, &proposal.end) {
                (_, Some(end)) => field_line(
                    ui,
                    "Fim",
                    &crate::resources::moment_label(end),
                    backs(when, |c| c.date && c.end_time),
                ),
                (Some(_), None) => muted_line(ui, "Fim não informado"),
                _ => {}
            }
            if !proposal.purpose.trim().is_empty() {
                field_line(ui, "Finalidade", proposal.purpose.trim(), false);
            }
            if let Some(check) = check {
                if used.is_none() {
                    if let Some(chosen) = availability_lines(ui, proposal, check) {
                        action.set(Some(chosen));
                    }
                }
            }
            if !proposal.remark.trim().is_empty() {
                muted_line(ui, proposal.remark.trim());
            }
            let ids: Vec<&str> = std::iter::once(proposal.what_fact.as_str())
                .chain(proposal.when_fact.iter().map(String::as_str))
                .chain(proposal.facts.iter().map(String::as_str))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            quotes_fold(ui, &proposal.id, analysis, &ids);
        },
    );
    action.into_inner()
}

/// The status line the program computed for a reservation; with no resource
/// that fits, the quiet way to register one.
fn availability_lines(
    ui: &mut egui::Ui,
    proposal: &ReservationProposal,
    check: &ReservationCheck,
) -> Option<ProposalAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| match &check.availability {
        Availability::Free { resource } => {
            pill(ui, "Livre neste horário", Tone::Ok);
            let mut text = format!("em “{resource}”");
            if check.end_assumed {
                text.push_str(" (supondo 1 hora de uso)");
            }
            widgets::small_muted(ui, text);
        }
        Availability::Busy { sentence, .. } => {
            pill(ui, "Ocupado", Tone::Danger);
            ui.add(
                egui::Label::new(
                    RichText::new(sentence)
                        .size(theme::SIZE_SMALL)
                        .color(theme::c().muted),
                )
                .wrap(),
            );
        }
        Availability::TooMany { resource, has } => {
            pill(ui, "Quantidade acima do cadastro", Tone::Warn);
            widgets::small_muted(
                ui,
                format!(
                    "O cadastro tem {has} de “{resource}”; o documento pede {}.",
                    proposal.quantity
                ),
            );
        }
        Availability::NoTime { resource } => {
            pill(ui, "Sem horário certo", Tone::Neutral);
            widgets::small_muted(
                ui,
                format!("“{resource}” combina; informe o horário no formulário."),
            );
        }
        Availability::Several(names) => {
            pill(ui, "Mais de um recurso combina", Tone::Warn);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{}. A reserva abre sem recurso escolhido: escolha no formulário.",
                        names.join(", ")
                    ))
                    .size(theme::SIZE_SMALL)
                    .color(theme::c().muted),
                )
                .wrap(),
            );
        }
        Availability::NoMatch => {
            pill(ui, "Nenhum recurso cadastrado combina", Tone::Neutral);
            if ui
                .add(widgets::quiet("Cadastrar recurso…"))
                .on_hover_text("Abre o cadastro de recursos com este nome já digitado.")
                .clicked()
            {
                action = Some(ProposalAction::RegisterResource(
                    proposal.what.trim().to_owned(),
                ));
            }
        }
    });
    action
}

/// "Leitura do documento" / "Preparação das propostas".
pub(super) fn role_label(role: &str) -> &str {
    match role {
        "extrator" => "Leitura do documento",
        "propositor" => "Preparação das propostas",
        other => other,
    }
}

/// The kind of a fact as the person reads it.
fn fact_kind_label(kind: &str) -> &str {
    match kind {
        "pedido" => "Pedido",
        "prazo" => "Prazo",
        "data_hora" => "Data e hora",
        "quantidade" => "Quantidade",
        "recurso" => "Recurso",
        "remetente" => "Remetente",
        "assunto" => "Assunto",
        _ => "Outro",
    }
}

fn plural_count(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

impl PonteApp {
    /// Reads what the Propostas block needs, for the document now open.
    pub(super) fn refresh_proposal_view(&mut self) {
        self.proposal_view = self
            .detail
            .as_ref()
            .and_then(|detail| ProposalView::build(&self.store, detail));
    }

    /// The window "Material utilizado" of the open analysis: what the program
    /// recorded when it was made, nothing more.
    pub(super) fn analysis_material_window(&mut self, ctx: &egui::Context) {
        let Some(document_id) = self.analysis_material else {
            return;
        };
        // The analysis as the card shows it: checked again against the
        // document (see `cleaned_analysis`).
        let analysis = self
            .detail
            .as_ref()
            .filter(|d| d.summary.id == document_id)
            .and_then(|d| d.analysis.as_ref())
            .map(|stored| {
                self.proposal_view
                    .as_ref()
                    .filter(|view| view.created_at == stored.created_at)
                    .map(|view| view.analysis.clone())
                    .unwrap_or_else(|| stored.clone())
            });
        let Some(analysis) = analysis else {
            self.analysis_material = None;
            return;
        };
        let mut open = true;
        egui::Window::new("Material utilizado na análise")
            .open(&mut open)
            .default_size([660.0, 700.0])
            .show(ctx, |ui| {
                #[cfg(feature = "shots")]
                let scroll = if self.shots_ficha_scroll > 0.0 {
                    egui::ScrollArea::vertical().vertical_scroll_offset(self.shots_ficha_scroll)
                } else {
                    egui::ScrollArea::vertical()
                };
                #[cfg(not(feature = "shots"))]
                let scroll = egui::ScrollArea::vertical();
                scroll.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S3;
                    widgets::card(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S2;
                        let provider = analysis
                            .run
                            .as_ref()
                            .map(|run| run.provider.clone())
                            .filter(|p| !p.trim().is_empty())
                            .unwrap_or_else(|| "não registrado".to_owned());
                        let mut rows = vec![
                            ("IA", provider),
                            ("Modelo", analysis.model.clone()),
                            ("Feita em", display_date(&analysis.created_at)),
                        ];
                        if let Some(run) = &analysis.run {
                            rows.push((
                                "Chamadas à IA",
                                run.calls.to_string(),
                            ));
                        }
                        facts(ui, "analysis-material-facts", &rows);
                    });
                    if let Some(run) = &analysis.run {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Etapas");
                            if run.steps.is_empty() {
                                widgets::small_muted(ui, "Nenhuma etapa registrada.");
                            }
                            for step in &run.steps {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(role_label(&step.role))
                                            .font(theme::font_strong(theme::SIZE_BODY)),
                                    );
                                    widgets::small_muted(
                                        ui,
                                        format!(
                                            "{} • {}",
                                            plural_count(step.attempts, "tentativa", "tentativas"),
                                            if step.dropped == 0 {
                                                "nenhum item descartado".to_owned()
                                            } else {
                                                plural_count(
                                                    step.dropped,
                                                    "item descartado",
                                                    "itens descartados",
                                                )
                                            }
                                        ),
                                    );
                                });
                                for reason in &step.refused {
                                    ui.add(egui::Label::new(format!("• {reason}")).wrap());
                                }
                            }
                            widgets::small_muted(
                                ui,
                                "Os itens recusados não entram na análise; os motivos vêm do programa, sem o texto do documento.",
                            );
                        });
                    }
                    widgets::card(ui, |ui| {
                        widgets::section(ui, format!("Fatos citados pela IA: {}", analysis.facts.len()));
                        if analysis.facts.is_empty() {
                            widgets::small_muted(ui, "Esta análise não registrou fatos.");
                        }
                        for (index, fact) in analysis.facts.iter().enumerate() {
                            if index > 0 {
                                ui.separator();
                            }
                            fact_block(ui, fact, &analysis);
                        }
                    });
                    widgets::small_muted(
                        ui,
                        "O programa confere cada trecho no texto do documento, no contexto ou nas fontes; a interpretação da IA ainda exige a sua revisão.",
                    );
                });
            });
        if !open {
            self.analysis_material = None;
        }
    }
}

/// One fact: the AI's own summary (not checked), the excerpt, where the
/// program found it and which values it found written in it.
fn fact_block(ui: &mut egui::Ui, fact: &Fact, analysis: &Analysis) {
    ui.horizontal_wrapped(|ui| {
        pill(ui, fact_kind_label(&fact.kind), Tone::Neutral);
        ui.add(
            egui::Label::new(format!("Resumo da IA (não conferido): {}", fact.statement)).wrap(),
        );
    });
    muted_line(ui, &quote_text(fact));
    origin_line(ui, fact, analysis);
    let written = fact.checked.quote && fact.checked.origin.is_written();
    let value = |ui: &mut egui::Ui, text: String, found: bool| {
        if found && written {
            pill(ui, format!("{text} {PILL_TEXT}"), Tone::Ok).on_hover_text(PILL_HOVER);
        } else {
            pill(ui, format!("{text} não conferido"), Tone::Warn);
        }
    };
    ui.horizontal_wrapped(|ui| {
        if !fact.checked.quote {
            pill(ui, "trecho não encontrado", Tone::Danger);
        }
        if let Some(date) = &fact.date {
            value(
                ui,
                format!("data {}", crate::labels::date(date)),
                fact.checked.date,
            );
        }
        if let Some(time) = &fact.time {
            let end = fact
                .end_time
                .as_ref()
                .map(|end| format!(" às {end}"))
                .unwrap_or_default();
            value(
                ui,
                format!("hora {time}{end}"),
                fact.checked.time && fact.end_time.as_ref().is_none_or(|_| fact.checked.end_time),
            );
        }
        if let Some(quantity) = fact.quantity {
            value(ui, format!("quantidade {quantity}"), fact.checked.quantity);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ResourceUse;

    fn task(id: i64, proposal_id: &str) -> Task {
        let mut task = Task::new(1);
        task.id = id;
        task.proposal_id = proposal_id.into();
        task
    }

    fn attendance(id: i64, status: &str, ids: &[&str]) -> SaiopAttendance {
        let mut att = SaiopAttendance::new_manual();
        att.id = id;
        att.status = status.into();
        att.proposal_ids = ids.iter().map(|s| (*s).to_owned()).collect();
        att
    }

    fn use_of(uid: &str, proposal_id: &str) -> ResourceUse {
        ResourceUse {
            uid: uid.into(),
            proposal_id: proposal_id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_proposal_is_used_when_a_record_carries_its_id() {
        let ids: BTreeSet<&str> = ["t1", "a1", "r1", "t2"].into_iter().collect();
        let tasks = vec![task(10, "t1"), task(11, ""), task(12, "outra")];
        let attendances = vec![attendance(20, "em_analise", &["a1", "x"])];
        let uses = vec![use_of("u1", "r1"), use_of("u2", "")];
        let used = used_records(&tasks, &attendances, &uses, &ids);
        assert_eq!(used.len(), 3);
        assert!(matches!(used.get("t1"), Some(Used::Task(t)) if t.id == 10));
        assert!(matches!(used.get("a1"), Some(Used::Attendance(20))));
        assert!(matches!(used.get("r1"), Some(Used::Use(uid)) if uid == "u1"));
        assert!(!used.contains_key("t2"), "nothing carries t2");
    }

    #[test]
    fn a_task_with_the_same_action_but_no_proposal_id_is_not_a_used_proposal() {
        let ids: BTreeSet<&str> = ["t1"].into_iter().collect();
        // The triage task every import creates has an action and no id.
        let mut triage = Task::new(1);
        triage.id = 5;
        triage.action = "responder".into();
        let used = used_records(&[triage], &[], &[], &ids);
        assert!(used.is_empty());
    }

    #[test]
    fn the_atendimento_to_complete_is_the_open_newest_one() {
        let mine = vec![
            attendance(3, "em_analise", &[]),
            attendance(7, "encerrado", &[]),
            attendance(5, "aguardando_resposta", &[]),
        ];
        assert_eq!(pick_attendance(&mine).map(|a| a.id), Some(5));
        let closed = vec![
            attendance(3, "encerrado", &[]),
            attendance(7, "cancelado", &[]),
        ];
        assert_eq!(pick_attendance(&closed).map(|a| a.id), Some(7));
        assert!(pick_attendance(&[]).is_none());
    }

    fn fact(origin: Origin, date: bool) -> Fact {
        Fact {
            id: "f1".into(),
            quote: "prazo até 20/02/2031".into(),
            date: Some("2031-02-20".into()),
            checked: FactCheck {
                quote: true,
                date,
                origin,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn the_pill_needs_a_fact_written_in_the_document_or_a_source() {
        let document = fact(Origin::Document, true);
        assert!(backs(Some(&document), |c| c.date));
        assert!(backs(Some(&fact(Origin::Source, true)), |c| c.date));
        assert!(
            !backs(Some(&fact(Origin::Context, true)), |c| c.date),
            "what only the person's own note says never gets the pill"
        );
        assert!(!backs(Some(&fact(Origin::Nowhere, true)), |c| c.date));
        assert!(
            !backs(Some(&fact(Origin::Document, false)), |c| c.date),
            "that value was not found"
        );
        assert!(!backs(None, |c| c.date));
    }

    #[test]
    fn where_the_excerpt_was_found_is_said_in_words() {
        let mut analysis = Analysis::default();
        analysis.materials.push(crate::evidence::Source {
            id: "s1".into(),
            document_id: 1,
            capture_id: 1,
            label: "Ofício anexo, página 2".into(),
            file_id: None,
            page: Some(2),
            method: "texto".into(),
            text: String::new(),
        });
        assert_eq!(
            origin_text(&fact(Origin::Document, true), &analysis),
            "no documento"
        );
        let mut source = fact(Origin::Source, true);
        source.source_id = "s1".into();
        assert_eq!(
            origin_text(&source, &analysis),
            "na fonte: Ofício anexo, página 2"
        );
        assert_eq!(
            origin_text(&fact(Origin::Context, true), &analysis),
            "só no seu contexto (informado por você)"
        );
    }

    #[test]
    fn a_sender_not_found_written_shows_the_note_and_a_found_one_the_pill() {
        assert_eq!(sender_note(false), Some("(citado no documento; confira)"));
        assert_eq!(sender_note(true), None);
    }

    #[test]
    fn the_pill_words_are_the_decided_ones() {
        assert_eq!(PILL_TEXT, "consta no trecho");
        assert_eq!(
            PILL_HOVER,
            "O programa encontrou este dado escrito no trecho citado. Confira se é mesmo o prazo, o horário ou a quantidade que o documento quer dizer."
        );
    }

    fn seeded() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        crate::shots_data::seed(&mut store).unwrap();
        (dir, store)
    }

    #[test]
    fn the_card_checks_the_analysis_again_and_writes_nothing() {
        let (_dir, store) = seeded();
        let id = store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.document_number == "ORG-OFI-2026/00431")
            .unwrap()
            .id;
        let detail = store.load_document(id).unwrap();
        let stored = detail.analysis.clone().unwrap();
        // Today the document says what the analysis recorded: nothing is cleared.
        let view = ProposalView::build(&store, &detail).unwrap();
        assert_eq!(view.cleared, 0);
        // The document as it is now no longer carries the deadline.
        let mut changed = detail.clone();
        changed.capture.text = changed
            .capture
            .text
            .replace("deve chegar ao Protocolo até", "chegará depois de");
        let view = ProposalView::build(&store, &changed).unwrap();
        assert!(view.cleared > 0, "the lost excerpt is counted");
        assert!(
            view.analysis
                .proposals
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .all(|task| task.due_date.is_none()),
            "no deadline stands on it"
        );
        assert!(
            cleared_text(view.cleared).contains("não conferem mais com o documento")
                || cleared_text(1).contains("não confere mais")
        );
        // Nothing was written: the stored analysis is as it was.
        let again = store.load_document(id).unwrap().analysis.unwrap();
        assert_eq!(
            serde_json::to_string(&again).unwrap(),
            serde_json::to_string(&stored).unwrap()
        );
    }

    #[test]
    fn the_warning_counts_in_plain_words() {
        assert_eq!(cleared_text(1), "1 dado desta análise não confere mais com o documento e foi retirado. Analise de novo para atualizar.");
        assert_eq!(cleared_text(3), "3 dados desta análise não conferem mais com o documento e foram retirados. Analise de novo para atualizar.");
    }

    #[test]
    fn roles_are_named_in_portuguese() {
        assert_eq!(role_label("extrator"), "Leitura do documento");
        assert_eq!(role_label("propositor"), "Preparação das propostas");
        assert_eq!(role_label("outro"), "outro");
    }
}
