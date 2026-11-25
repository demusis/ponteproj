//! The "Projetos" area: the list of projects with their progress, one project
//! open (its tasks, or its timeline), and the dialogs to create or edit it and
//! to add existing tasks to it. The rules (progress, risk, timeline, saving in
//! each team mode) live in `crate::projects` and the store; here is what the
//! person sees, and the pure helpers that decide what is listed.
mod chart;

use super::today::{day_month, list_card, row};
use super::{Leave, PonteApp};
use crate::projects::{Project, ProjectOverview, ProjectStatus, TimelineRow};
use crate::storage::Store;
use crate::tasks::Task;
use crate::theme::{self, c, icon, S1, S2, S3, S4};
use crate::widgets::{self, Tone};
use chrono::NaiveDate;
use eframe::egui::{self, RichText};
use std::collections::{BTreeMap, BTreeSet};

/// Most tasks the dialog "Incluir tarefas existentes" lists at once.
const CANDIDATE_LIMIT: usize = 300;

/// The views the screen declares for the picture tour (`--features shots`).
#[cfg(feature = "shots")]
pub(super) const SHOTS: &[&str] = &["lista", "aberto", "linha-do-tempo", "formulario", "incluir"];

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Which projects the list shows, by situation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum StatusFilter {
    #[default]
    Active,
    Paused,
    Done,
    Archived,
    All,
}

impl StatusFilter {
    const ALL: [StatusFilter; 5] = [
        StatusFilter::Active,
        StatusFilter::Paused,
        StatusFilter::Done,
        StatusFilter::Archived,
        StatusFilter::All,
    ];

    fn label(self) -> &'static str {
        match self {
            StatusFilter::Active => "Ativos",
            StatusFilter::Paused => "Pausados",
            StatusFilter::Done => "Concluídos",
            StatusFilter::Archived => "Arquivados",
            StatusFilter::All => "Todos",
        }
    }

    /// What an empty list says ("Nenhum projeto ativo.").
    fn none_text(self) -> &'static str {
        match self {
            StatusFilter::Active => "Nenhum projeto ativo.",
            StatusFilter::Paused => "Nenhum projeto pausado.",
            StatusFilter::Done => "Nenhum projeto concluído.",
            StatusFilter::Archived => "Nenhum projeto arquivado.",
            StatusFilter::All => "Nenhum projeto.",
        }
    }

    fn matches(self, status: ProjectStatus) -> bool {
        match self {
            StatusFilter::Active => status == ProjectStatus::Ativo,
            StatusFilter::Paused => status == ProjectStatus::Pausado,
            StatusFilter::Done => status == ProjectStatus::Concluido,
            StatusFilter::Archived => status == ProjectStatus::Arquivado,
            StatusFilter::All => true,
        }
    }
}

/// Where a situation sits when "Todos" lists them together.
fn status_rank(status: ProjectStatus) -> u8 {
    match status {
        ProjectStatus::Ativo => 0,
        ProjectStatus::Pausado => 1,
        ProjectStatus::Concluido => 2,
        ProjectStatus::Arquivado => 3,
    }
}

/// The colour of the pill of a situation.
fn status_tone(status: ProjectStatus) -> Tone {
    match status {
        ProjectStatus::Ativo => Tone::Ok,
        ProjectStatus::Pausado => Tone::Warn,
        ProjectStatus::Concluido => Tone::Info,
        ProjectStatus::Arquivado => Tone::Neutral,
    }
}

/// Every word typed is in the name or the objective (ignoring case and
/// accents). `needle` is already folded; empty matches everything.
fn matches_query(project: &Project, needle: &str) -> bool {
    let haystack = crate::labels::fold(&format!("{}\n{}", project.name, project.objective));
    needle
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

/// The projects the list shows: those in the chosen situation that match the
/// search, by situation (when several are listed) and then by name.
fn list_view<'a>(
    all: &'a [ProjectOverview],
    filter: StatusFilter,
    query: &str,
) -> Vec<&'a ProjectOverview> {
    let needle = crate::labels::fold(query.trim());
    let mut out: Vec<&ProjectOverview> = all
        .iter()
        .filter(|o| filter.matches(o.project.status) && matches_query(&o.project, &needle))
        .collect();
    out.sort_by_key(|o| {
        (
            status_rank(o.project.status),
            o.project.name_key(),
            o.project.uid.clone(),
        )
    });
    out
}

/// How many projects a chip of the filter stands for.
fn count_in(all: &[ProjectOverview], filter: StatusFilter) -> usize {
    all.iter()
        .filter(|o| filter.matches(o.project.status))
        .count()
}

/// "3 de 8 tarefas concluídas".
fn progress_text(done: usize, total: usize) -> String {
    match total {
        0 => "Nenhuma tarefa ainda".to_owned(),
        1 => format!("{done} de 1 tarefa concluída"),
        n => format!("{done} de {n} tarefas concluídas"),
    }
}

/// The warn line of a target date the plan does not meet.
fn target_risk_text(target: NaiveDate, end: chrono::NaiveDateTime) -> String {
    format!(
        "Data-alvo {} em risco: término previsto {}",
        target.format("%d/%m"),
        end.format("%d/%m")
    )
}

/// The tasks of a project in the groups of its list.
#[derive(Debug, Default, PartialEq)]
struct TaskGroups<'a> {
    /// Open tasks whose deadline has passed, the oldest first.
    late: Vec<&'a Task>,
    /// The other open tasks, by deadline (those without one last).
    open: Vec<&'a Task>,
    /// Concluded tasks, the latest concluded first.
    done: Vec<&'a Task>,
}

fn task_groups<'a>(tasks: &'a [Task], today: &str) -> TaskGroups<'a> {
    let mut groups = TaskGroups::default();
    for task in tasks {
        if task.is_done() {
            groups.done.push(task);
        } else if !task.due_date.is_empty() && task.due_date.as_str() < today {
            groups.late.push(task);
        } else {
            groups.open.push(task);
        }
    }
    let by_due = |a: &&Task, b: &&Task| {
        (
            a.due_date.is_empty(),
            &a.due_date,
            a.title.to_lowercase(),
            a.id,
        )
            .cmp(&(
                b.due_date.is_empty(),
                &b.due_date,
                b.title.to_lowercase(),
                b.id,
            ))
    };
    groups.late.sort_by(by_due);
    groups.open.sort_by(by_due);
    groups.done.sort_by(|a, b| {
        b.completed_at
            .cmp(&a.completed_at)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    groups
}

/// The tasks "Incluir tarefas existentes…" offers: open, without project,
/// matching the search (title, notes or assignee, ignoring case and accents),
/// by deadline (without one last), then title.
fn add_candidates<'a>(tasks: &'a [Task], query: &str) -> Vec<&'a Task> {
    let needle = crate::labels::fold(query.trim());
    let mut out: Vec<&Task> = tasks
        .iter()
        .filter(|t| !t.is_done() && t.project_uid.is_empty())
        .filter(|t| {
            let haystack =
                crate::labels::fold(&format!("{}\n{}\n{}", t.title, t.notes, t.assignee));
            needle
                .split_whitespace()
                .all(|word| haystack.contains(word))
        })
        .collect();
    out.sort_by(|a, b| {
        (
            a.due_date.is_empty(),
            &a.due_date,
            a.title.to_lowercase(),
            a.id,
        )
            .cmp(&(
                b.due_date.is_empty(),
                &b.due_date,
                b.title.to_lowercase(),
                b.id,
            ))
    });
    out
}

/// "Excluir o projeto …?": what deleting does, in the words of the question.
fn delete_sentence(name: &str, tasks: usize) -> String {
    let what = match tasks {
        0 => "O projeto não tem tarefas.".to_owned(),
        1 => "A 1 tarefa dele continua e volta para “Sem projeto”.".to_owned(),
        n => format!("As {n} tarefas dele continuam e voltam para “Sem projeto”."),
    };
    format!("Excluir o projeto “{name}”? {what}")
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// What the screen asks of the app after a frame.
pub(super) enum Asked {
    /// Conclude this task (as the list of Hoje does, offering to undo).
    Conclude(Task),
    /// Open this task in Tarefas.
    OpenTask(Task),
    /// A new task for this project (uid), in Tarefas.
    NewTask(String),
    /// The library changed: the app refreshes the other screens and tells the
    /// person this, when there is a text.
    Changed(Option<String>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Tasks,
    Timeline,
}

/// A line at the top of the area: what was refused, or a conflict.
struct Banner {
    text: String,
    error: bool,
    /// A newer version of the project exists: "Recarregar" is offered.
    conflict: bool,
}

/// What the library held at the last read, kept between frames.
#[derive(Default)]
struct Loaded {
    stamp: Option<(i64, i64)>,
    today: Option<NaiveDate>,
    open: Option<String>,
    /// Bumped by the screen after its own writes, so the next frame reads again.
    stale: bool,
    overviews: Vec<ProjectOverview>,
    /// Every task of the library (the dialog to add tasks reads them).
    tasks: Vec<Task>,
    /// Number and title of each document, to name the document of a task.
    documents: BTreeMap<i64, (String, String)>,
    /// The open project: its tasks, the rows of its timeline and its version.
    project_tasks: Vec<Task>,
    rows: Vec<TimelineRow>,
    version: i64,
    error: Option<String>,
}

/// The dialog to create or edit a project.
struct ProjectForm {
    project: Project,
    /// The version the edit started from (`None` for a new project).
    base_version: Option<i64>,
    /// The project as the form read it (`None` for a new project): what a
    /// save compares with the project saved now.
    read: Option<Project>,
    /// The project changed elsewhere after the form read it: the version
    /// saved now and its number, until the person chooses what to keep.
    theirs: Option<(Project, i64)>,
    error: Option<String>,
    /// A line that is no error (what "Juntar as duas versões" did).
    notice: Option<String>,
    conflict: bool,
    focus: bool,
}

/// The name of a field of the project, as the form calls it.
fn project_field_label(key: &str) -> &'static str {
    match key {
        "name" => "Nome",
        "objective" => "Objetivo",
        "assignee" => "Responsável",
        "status" => "Situação",
        "start_date" => "Início",
        "target_date" => "Data-alvo",
        _ => "outros dados",
    }
}

impl ProjectForm {
    fn new_project() -> Self {
        Self {
            project: Project::new(""),
            base_version: None,
            read: None,
            theirs: None,
            error: None,
            notice: None,
            conflict: false,
            focus: true,
        }
    }

    fn edit(project: &Project, version: i64) -> Self {
        Self {
            project: project.clone(),
            base_version: Some(version),
            read: Some(project.clone()),
            theirs: None,
            error: None,
            notice: None,
            conflict: false,
            focus: true,
        }
    }

    /// The version a write over `read` (the project as a screen read it)
    /// goes on: `Ok(Some(now))` when the project saved now says the same
    /// (only its version moved), `Ok(None)` when it can no longer be read
    /// (the write then says why), and the refusal when it changed
    /// elsewhere, whatever its version number.
    fn version_over(store: &Store, read: &Project) -> Result<Option<i64>, String> {
        match Self::changed_since(store, read) {
            Ok(None) => Ok(store.project_version(&read.uid).ok()),
            Ok(Some((_, text, _))) => Err(text),
            Err(()) => Ok(None),
        }
    }

    /// `Ok(None)` when the project saved now says the same as `read`;
    /// `Ok(Some((current, refusal, version)))` when it changed elsewhere;
    /// `Err` when it can no longer be read.
    fn changed_since(store: &Store, read: &Project) -> Result<Option<(Project, String, i64)>, ()> {
        let current = match store.get_project(&read.uid) {
            Ok(Some(current)) => current,
            _ => return Err(()),
        };
        if crate::edit_check::same_record(&current, read, crate::edit_check::STAMPS) {
            return Ok(None);
        }
        let meta = store
            .item_version("project", crate::work_templates::version_key(&read.uid))
            .unwrap_or_default();
        let changed: Vec<&str> =
            crate::edit_check::changed_keys(read, &current, crate::edit_check::STAMPS)
                .iter()
                .map(|key| project_field_label(key))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
        let what = if changed.is_empty() {
            String::new()
        } else {
            format!(" Mudou lá: {}.", changed.join(", "))
        };
        let text = crate::edit_check::changed_since_read(
            "este projeto foi alterado",
            &meta,
            &store.active_actor(),
            format!("{what} Use “Juntar as duas versões” para trazer o que mudou lá e revisar antes de salvar, ou “Ficar com a versão de lá” para descartar as suas alterações.").trim_start(),
        );
        Ok(Some((current, text, meta.version)))
    }

    /// "Salvar": the version alone never proves that nobody else changed
    /// the project (two computers that saved it the same number of times
    /// reach the same number with different contents), so the project saved
    /// now is compared with what the form read first. The same (only the
    /// version moved): saved on its current version. Another content:
    /// nothing is written, the typing stays, and the form offers "Juntar
    /// as duas versões" and "Ficar com a versão de lá".
    fn save(&mut self, store: &mut Store) -> Result<Project, String> {
        if self.theirs.is_some() {
            return Err(self.error.clone().unwrap_or_default());
        }
        let mut expected = self.base_version;
        if let (Some(read), Some(_)) = (self.read.clone(), self.base_version) {
            match Self::changed_since(store, &read) {
                Ok(None) => expected = store.project_version(&read.uid).ok().or(expected),
                Ok(Some((current, text, version))) => {
                    self.theirs = Some((current, version));
                    self.conflict = true;
                    self.notice = None;
                    self.error = Some(text.clone());
                    return Err(text);
                }
                Err(()) => {}
            }
        }
        match store.save_project_checked(&self.project, expected) {
            Ok(saved) => Ok(saved),
            Err(error) => {
                let text = error.to_string();
                self.conflict = text.contains("Conflito de edição");
                self.notice = None;
                self.error = Some(text.clone());
                Err(text)
            }
        }
    }

    /// "Juntar as duas versões": the form takes what changed there in the
    /// fields the person did not touch and keeps the person's value in the
    /// others; it now stands on the current version and stays unsaved, so
    /// the person reviews it before saving. A field both sides changed is
    /// named, with the value it had there.
    fn merge_conflict(&mut self) -> bool {
        let (Some((theirs, version)), Some(read)) = (self.theirs.clone(), self.read.clone()) else {
            return false;
        };
        let Some((merged, both)) = crate::edit_check::merge_three(
            &read,
            &theirs,
            &self.project,
            crate::edit_check::STAMPS,
        ) else {
            self.error = Some(
                "Não foi possível juntar as duas versões. Use “Ficar com a versão de lá” e digite de novo o que mudou.".to_owned(),
            );
            return false;
        };
        self.project = merged;
        self.read = Some(theirs.clone());
        self.base_version = Some(version);
        self.theirs = None;
        self.conflict = false;
        self.error = None;
        let mut text =
            "As alterações de lá foram trazidas para o formulário, junto com as suas. Revise e salve."
                .to_owned();
        if !both.is_empty() {
            let there = crate::edit_check::fields_of(&theirs, &[]);
            let shown: Vec<String> = both
                .iter()
                .map(|key| {
                    let value = match there.get(key) {
                        Some(serde_json::Value::String(text)) => text.clone(),
                        Some(other) => other.to_string(),
                        None => String::new(),
                    };
                    format!("{} (lá estava “{value}”)", project_field_label(key))
                })
                .collect();
            text.push_str(&format!(
                " Mudou nos dois lados e ficou o seu: {}.",
                shown.join("; ")
            ));
        }
        self.notice = Some(text);
        true
    }

    /// "Ficar com a versão de lá": the typing goes and the form shows the
    /// project as it is saved now, on its current version.
    fn take_theirs(&mut self, store: &Store) {
        let uid = self.project.uid.clone();
        match store.get_project(&uid) {
            Ok(Some(current)) => {
                let version = store.project_version(&uid).unwrap_or(0);
                *self = ProjectForm::edit(&current, version);
                self.focus = false;
            }
            _ => {
                self.error = Some("O projeto não está mais disponível.".to_owned());
                self.conflict = false;
                self.theirs = None;
            }
        }
    }
}

/// The dialog "Incluir tarefas existentes…".
#[derive(Default)]
struct IncludeDialog {
    query: String,
    chosen: BTreeSet<i64>,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct ProjectsPanel {
    filter: StatusFilter,
    query: String,
    focus_search: bool,
    /// The project on screen, by uid.
    open: Option<String>,
    view: View,
    form: Option<ProjectForm>,
    include: Option<IncludeDialog>,
    confirm_delete: bool,
    banner: Option<Banner>,
    data: Loaded,
}

impl ProjectsPanel {
    /// Ctrl+F.
    pub(super) fn key_find(&mut self) {
        if self.open.is_none() {
            self.focus_search = true;
        }
    }

    /// Ctrl+N and "Novo projeto".
    pub(super) fn start_new_project(&mut self) {
        if self.form.is_none() {
            self.form = Some(ProjectForm::new_project());
        }
    }

    /// Shows this project (from the search); the list is left behind.
    pub(super) fn open_project(&mut self, uid: &str) {
        self.open = Some(uid.to_owned());
        self.view = View::Tasks;
        self.confirm_delete = false;
        self.banner = None;
    }

    /// Esc: closes what is in front. True when it did something.
    pub(super) fn key_escape(&mut self) -> bool {
        if self.include.take().is_some() || self.form.take().is_some() {
            return true;
        }
        if self.confirm_delete {
            self.confirm_delete = false;
            return true;
        }
        if self.open.is_some() {
            self.open = None;
            return true;
        }
        false
    }

    /// Reads the library again when it changed, the day changed or another
    /// project was opened.
    fn reload(&mut self, store: &Store, today: NaiveDate) {
        let stamp = store.library_stamp();
        let fresh = stamp.is_some()
            && self.data.stamp == stamp
            && self.data.today == Some(today)
            && self.data.open == self.open
            && !self.data.stale;
        if fresh {
            return;
        }
        let mut data = Loaded {
            stamp,
            today: Some(today),
            open: self.open.clone(),
            ..Loaded::default()
        };
        match store.project_overview(today) {
            Ok(overviews) => data.overviews = overviews,
            Err(error) => data.error = Some(error.to_string()),
        }
        match store.list_tasks() {
            Ok(tasks) => data.tasks = tasks,
            Err(error) => data.error = Some(error.to_string()),
        }
        if let Ok(documents) = store.list_documents("") {
            data.documents = documents
                .into_iter()
                .map(|d| (d.id, (d.document_number, d.title)))
                .collect();
        }
        if let Some(uid) = &self.open {
            data.project_tasks = data
                .tasks
                .iter()
                .filter(|t| t.project_uid == *uid)
                .cloned()
                .collect();
            data.rows = crate::projects::timeline(uid, &data.tasks);
            data.version = store.project_version(uid).unwrap_or(0);
        }
        self.data = data;
    }

    fn show(&mut self, ui: &mut egui::Ui, store: &mut Store, today: NaiveDate) -> Option<Asked> {
        self.reload(store, today);
        let mut asked = None;
        if let Some(error) = self.data.error.clone() {
            let (fg, bg) = Tone::Danger.colors();
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format!("Não foi possível ler os projetos: {error}"))
                            .color(fg),
                    );
                });
            return None;
        }
        let open = self
            .open
            .clone()
            .and_then(|uid| self.data.overviews.iter().find(|o| o.project.uid == uid))
            .cloned();
        match open {
            Some(overview) => self.open_view(ui, store, &overview, today, &mut asked),
            None => {
                if self.open.take().is_some() {
                    self.banner = Some(Banner {
                        text: "Este projeto não está mais disponível.".to_owned(),
                        error: true,
                        conflict: false,
                    });
                }
                self.list_view(ui, today);
            }
        }
        let ctx = ui.ctx().clone();
        self.form_window(&ctx, store, &mut asked);
        self.include_window(&ctx, store, &mut asked);
        asked
    }

    /// A line at the top: what happened that the person must know.
    fn banner_line(&mut self, ui: &mut egui::Ui) {
        let Some(banner) = &self.banner else {
            return;
        };
        let tone = if banner.error { Tone::Danger } else { Tone::Ok };
        let (fg, bg) = tone.colors();
        let (text, conflict) = (banner.text.clone(), banner.conflict);
        let mut close = false;
        let mut reload = false;
        egui::Frame::new()
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, icon::CLOSE, "Fechar").clicked() {
                            close = true;
                        }
                        if conflict && ui.add(widgets::quiet("Recarregar")).clicked() {
                            reload = true;
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                        });
                    });
                });
            });
        ui.add_space(S2);
        if reload {
            self.data.stale = true;
        }
        if close || reload {
            self.banner = None;
        }
    }

    // -- the list -----------------------------------------------------------

    fn list_view(&mut self, ui: &mut egui::Ui, today: NaiveDate) {
        ui.spacing_mut().item_spacing.y = S2;
        let mut new_project = false;
        ui.horizontal(|ui| {
            widgets::heading(ui, "Projetos");
            crate::help_ui::help(
                ui,
                "Um projeto reúne as tarefas de um mesmo objetivo, com responsável e data-alvo. O progresso, os riscos e a linha do tempo são calculados a partir das tarefas. Uma tarefa pertence a no máximo um projeto, e excluir o projeto não exclui as tarefas.",
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(widgets::primary(format!("{} Novo projeto", icon::PLUS)))
                    .clicked()
                {
                    new_project = true;
                }
                let search = widgets::search(ui, &mut self.query, "Buscar projeto", 260.0);
                if std::mem::take(&mut self.focus_search) {
                    search.request_focus();
                }
            });
        });
        if new_project {
            self.start_new_project();
        }
        self.banner_line(ui);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(S2, S2);
            for filter in StatusFilter::ALL {
                let label = format!(
                    "{} · {}",
                    filter.label(),
                    count_in(&self.data.overviews, filter)
                );
                if widgets::chip(ui, self.filter == filter, label).clicked() {
                    self.filter = filter;
                }
            }
        });
        ui.add_space(S1);
        let shown = list_view(&self.data.overviews, self.filter, &self.query);
        let mut opened: Option<String> = None;
        let mut new_from_empty = false;
        let mut show_all = false;
        egui::ScrollArea::vertical()
            .id_salt("projects-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Room for the scroll bar, so the cards keep their right edge.
                ui.set_max_width(ui.available_width() - 14.0);
                ui.spacing_mut().item_spacing.y = S2;
                if self.data.overviews.is_empty() {
                    ui.add_space(S4);
                    widgets::card(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S2;
                        widgets::section(ui, "Nenhum projeto ainda");
                        ui.add(
                            egui::Label::new(
                                RichText::new("Um projeto reúne as tarefas de um mesmo objetivo, como a organização de uma reunião ou a atualização de um quadro, e mostra quanto já foi feito, o que está atrasado e quando tudo deve terminar.")
                                    .color(c().muted),
                            )
                            .wrap(),
                        );
                        if ui
                            .add(widgets::primary(format!("{} Novo projeto", icon::PLUS)))
                            .clicked()
                        {
                            new_from_empty = true;
                        }
                    });
                } else if shown.is_empty() {
                    ui.add_space(S4 * 2.0);
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = S2;
                        widgets::muted(
                            ui,
                            if self.query.trim().is_empty() {
                                self.filter.none_text().to_owned()
                            } else {
                                format!("Nenhum projeto encontrado para “{}”.", self.query.trim())
                            },
                        );
                        if self.filter != StatusFilter::All
                            && ui.add(widgets::quiet("Ver todos os projetos")).clicked()
                        {
                            show_all = true;
                        }
                    });
                }
                for overview in &shown {
                    if project_card(ui, overview, today) {
                        opened = Some(overview.project.uid.clone());
                    }
                }
                ui.add_space(S4);
            });
        if new_from_empty {
            self.start_new_project();
        }
        if show_all {
            self.filter = StatusFilter::All;
            self.query.clear();
        }
        if let Some(uid) = opened {
            self.open_project(&uid);
        }
    }

    // -- one project ----------------------------------------------------------

    fn open_view(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        overview: &ProjectOverview,
        today: NaiveDate,
        asked: &mut Option<Asked>,
    ) {
        let project = &overview.project;
        let version = self.data.version;
        ui.spacing_mut().item_spacing.y = S2;
        if ui
            .add(widgets::quiet(format!("{} Projetos", icon::LEFT)))
            .on_hover_text("Volta para a lista de projetos. (Esc)")
            .clicked()
        {
            self.open = None;
            self.confirm_delete = false;
            return;
        }
        self.banner_line(ui);
        let mut edit = false;
        let mut new_task = false;
        let mut include = false;
        let mut set_status: Option<ProjectStatus> = None;
        let mut ask_delete = false;
        widgets::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = S2;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = S2;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::pill(ui, project.status.label(), status_tone(project.status));
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&project.name)
                                    .font(theme::font_strong(theme::SIZE_TITLE)),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&project.name);
                    });
                });
            });
            if !project.objective.is_empty() {
                ui.add(egui::Label::new(RichText::new(&project.objective)).wrap());
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = S4;
                for (label, value) in [
                    ("Responsável", project.assignee.as_str()),
                    ("Início", project.start_date.as_str()),
                    ("Data-alvo", project.target_date.as_str()),
                ] {
                    let value = if label == "Responsável" {
                        if value.is_empty() {
                            "não definido".to_owned()
                        } else {
                            value.to_owned()
                        }
                    } else if value.is_empty() {
                        "não definida".to_owned()
                    } else {
                        crate::labels::date(value)
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S1 + 2.0;
                        widgets::small_muted(ui, label);
                        ui.label(RichText::new(value).font(theme::font_strong(theme::SIZE_BODY)));
                    });
                }
            });
            progress_line(ui, overview, true);
            risk_line(ui, overview, project);
            ui.add_space(S1);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(S2, S2);
                if ui.add(widgets::secondary("Editar")).clicked() {
                    edit = true;
                }
                if ui
                    .add(widgets::primary(format!(
                        "{} Nova tarefa neste projeto",
                        icon::PLUS
                    )))
                    .clicked()
                {
                    new_task = true;
                }
                if ui
                    .add(widgets::secondary("Incluir tarefas existentes…"))
                    .on_hover_text(
                        "Escolhe tarefas em aberto que ainda não estão em nenhum projeto.",
                    )
                    .clicked()
                {
                    include = true;
                }
                let more = ui
                    .add(widgets::secondary(format!("Mais {}", icon::DOWN)))
                    .on_hover_text("Arquivar ou excluir o projeto.");
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(190.0);
                    if project.status == ProjectStatus::Arquivado {
                        if ui.button("Reabrir").clicked() {
                            set_status = Some(ProjectStatus::Ativo);
                            ui.close();
                        }
                    } else {
                        if project.status == ProjectStatus::Concluido
                            && ui.button("Reabrir").clicked()
                        {
                            set_status = Some(ProjectStatus::Ativo);
                            ui.close();
                        }
                        if ui.button("Arquivar").clicked() {
                            set_status = Some(ProjectStatus::Arquivado);
                            ui.close();
                        }
                    }
                    if ui
                        .button(RichText::new("Excluir projeto…").color(c().danger))
                        .clicked()
                    {
                        ask_delete = true;
                        ui.close();
                    }
                });
            });
        });
        if self.confirm_delete {
            self.delete_confirmation(ui, store, project, asked);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = S2;
            if widgets::chip(ui, self.view == View::Tasks, "Tarefas").clicked() {
                self.view = View::Tasks;
            }
            if widgets::chip(ui, self.view == View::Timeline, "Linha do tempo").clicked() {
                self.view = View::Timeline;
            }
        });
        ui.add_space(S1);
        let mut conclude: Option<Task> = None;
        let mut open_task: Option<Task> = None;
        let mut remove: Option<Task> = None;
        egui::ScrollArea::vertical()
            .id_salt(("project-open", &project.uid, self.view as u8))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Room for the scroll bar, so the cards keep their right edge.
                ui.set_max_width(ui.available_width() - 14.0);
                match self.view {
                    View::Tasks => {
                        self.tasks_view(ui, today, &mut conclude, &mut open_task, &mut remove)
                    }
                    View::Timeline => {
                        let clicked = chart::show(
                            ui,
                            &chart::Input {
                                uid: &project.uid,
                                rows: &self.data.rows,
                                target: project.target(),
                                today,
                            },
                        );
                        if let Some(id) = clicked {
                            open_task =
                                self.data.project_tasks.iter().find(|t| t.id == id).cloned();
                        }
                    }
                }
                ui.add_space(S4);
            });
        if edit {
            self.form = Some(ProjectForm::edit(project, version));
        }
        if new_task {
            *asked = Some(Asked::NewTask(project.uid.clone()));
        }
        if include {
            self.include = Some(IncludeDialog::default());
        }
        if ask_delete {
            self.confirm_delete = true;
        }
        if let Some(status) = set_status {
            self.change_status(store, project, version, status, asked);
        }
        if let Some(task) = conclude {
            *asked = Some(Asked::Conclude(task));
        } else if let Some(task) = open_task {
            *asked = Some(Asked::OpenTask(task));
        } else if let Some(task) = remove {
            self.take_out(store, &task, asked);
        }
    }

    fn tasks_view(
        &self,
        ui: &mut egui::Ui,
        today: NaiveDate,
        conclude: &mut Option<Task>,
        open: &mut Option<Task>,
        remove: &mut Option<Task>,
    ) {
        let today_text = today.format("%Y-%m-%d").to_string();
        let groups = task_groups(&self.data.project_tasks, &today_text);
        if self.data.project_tasks.is_empty() {
            widgets::card(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new("Este projeto ainda não tem tarefas. Use “Nova tarefa neste projeto” para criar uma, ou “Incluir tarefas existentes…” para trazer tarefas que já estão no Ponte.")
                            .color(c().muted),
                    )
                    .wrap(),
                );
            });
            return;
        }
        let documents = &self.data.documents;
        task_card(
            ui,
            "Atrasadas",
            Some(c().danger),
            &groups.late,
            &today_text,
            documents,
            (conclude, open, remove),
        );
        task_card(
            ui,
            "Abertas",
            None,
            &groups.open,
            &today_text,
            documents,
            (conclude, open, remove),
        );
        if !groups.done.is_empty() {
            let title = format!("CONCLUÍDAS · {}", groups.done.len());
            egui::CollapsingHeader::new(
                RichText::new(title)
                    .font(theme::font_strong(12.0))
                    .color(c().muted),
            )
            .id_salt(("project-done", self.open.as_deref().unwrap_or("")))
            .default_open(false)
            .show(ui, |ui| {
                ui.add_space(S1);
                task_list(
                    ui,
                    &groups.done,
                    &today_text,
                    documents,
                    (conclude, open, remove),
                );
            });
        }
    }

    // -- writes -----------------------------------------------------------------

    /// Shows a refusal at the top; a conflict offers to reload.
    fn refused(&mut self, text: String) {
        let conflict = text.contains("Conflito de edição");
        self.banner = Some(Banner {
            text,
            error: true,
            conflict,
        });
    }

    fn change_status(
        &mut self,
        store: &mut Store,
        project: &Project,
        version: i64,
        status: ProjectStatus,
        asked: &mut Option<Asked>,
    ) {
        let mut changed = project.clone();
        changed.status = status;
        // The version alone never proves the project is the one shown here:
        // a project changed elsewhere (even on the same version number) is
        // never written over with what this screen read.
        let version = match ProjectForm::version_over(store, project) {
            Ok(Some(now)) => now,
            Ok(None) => version,
            Err(text) => {
                self.refused(text);
                return;
            }
        };
        match store.save_project_checked(&changed, Some(version)) {
            Ok(saved) => {
                self.data.stale = true;
                *asked = Some(Asked::Changed(Some(match status {
                    ProjectStatus::Arquivado => format!("Projeto “{}” arquivado.", saved.name),
                    ProjectStatus::Ativo => format!("Projeto “{}” reaberto.", saved.name),
                    other => format!(
                        "Projeto “{}” agora está {}.",
                        saved.name,
                        other.label().to_lowercase()
                    ),
                })));
            }
            Err(error) => self.refused(error.to_string()),
        }
    }

    fn delete_confirmation(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        project: &Project,
        asked: &mut Option<Asked>,
    ) {
        let (fg, bg) = Tone::Danger.colors();
        let tasks = self.data.project_tasks.len();
        let mut delete = false;
        let mut cancel = false;
        egui::Frame::new()
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = S2;
                ui.add(
                    egui::Label::new(
                        RichText::new(delete_sentence(&project.name, tasks)).color(fg),
                    )
                    .wrap(),
                );
                ui.horizontal(|ui| {
                    if ui.add(widgets::danger("Excluir o projeto")).clicked() {
                        delete = true;
                    }
                    if ui.add(widgets::quiet("Cancelar")).clicked() {
                        cancel = true;
                    }
                });
            });
        ui.add_space(S2);
        if cancel {
            self.confirm_delete = false;
        }
        if delete {
            self.confirm_delete = false;
            match store.delete_project_checked(&project.uid, Some(self.data.version)) {
                Ok(()) => {
                    self.open = None;
                    self.data.stale = true;
                    *asked = Some(Asked::Changed(Some(match tasks {
                        0 => format!("Projeto “{}” excluído.", project.name),
                        1 => format!(
                            "Projeto “{}” excluído. A tarefa dele continua, sem projeto.",
                            project.name
                        ),
                        n => format!(
                            "Projeto “{}” excluído. As {n} tarefas dele continuam, sem projeto.",
                            project.name
                        ),
                    })));
                }
                Err(error) => self.refused(error.to_string()),
            }
        }
    }

    /// "Tirar do projeto": the task stays, without project.
    fn take_out(&mut self, store: &mut Store, task: &Task, asked: &mut Option<Asked>) {
        let fresh = store
            .list_tasks()
            .ok()
            .and_then(|tasks| tasks.into_iter().find(|t| t.id == task.id));
        let Some(mut fresh) = fresh else {
            self.data.stale = true;
            self.refused("Esta tarefa não está mais disponível.".to_owned());
            return;
        };
        fresh.project_uid.clear();
        match store.save_task(&fresh) {
            Ok(_) => {
                self.data.stale = true;
                *asked = Some(Asked::Changed(Some(format!(
                    "“{}” saiu do projeto e continua em Tarefas.",
                    task.title
                ))));
            }
            Err(error) => self.refused(format!(
                "Não foi possível tirar a tarefa do projeto: {error}"
            )),
        }
    }

    // -- dialogs ------------------------------------------------------------------

    fn form_window(&mut self, ctx: &egui::Context, store: &mut Store, asked: &mut Option<Asked>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let actor = store.active_actor();
        let editing = form.base_version.is_some();
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let mut reload = false;
        let mut merge = false;
        let waiting = form.theirs.is_some();
        egui::Window::new(if editing {
            "Editar projeto"
        } else {
            "Novo projeto"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = S2;
            ui.set_width(500.0_f32.min(ui.available_width()));
            widgets::field_label(ui, "Nome");
            let name = ui.add(
                egui::TextEdit::singleline(&mut form.project.name)
                    .char_limit(crate::projects::MAX_NAME)
                    .hint_text("ex.: Reunião do Comitê Gestor")
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut form.focus) {
                name.request_focus();
            }
            widgets::field_label(ui, "Objetivo");
            ui.add(
                egui::TextEdit::multiline(&mut form.project.objective)
                    .desired_rows(4)
                    .hint_text("O que este projeto deve entregar, em poucas linhas")
                    .desired_width(f32::INFINITY),
            );
            ui.columns(2, |columns| {
                widgets::field_label(&mut columns[0], "Responsável");
                columns[0].horizontal(|ui| {
                    let width = (ui.available_width() - 52.0).max(60.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut form.project.assignee).desired_width(width),
                    );
                    if ui
                        .add(widgets::secondary("Eu"))
                        .on_hover_text(format!("Atribuir a {actor}"))
                        .clicked()
                    {
                        form.project.assignee = actor.clone();
                    }
                });
                widgets::field_label(&mut columns[1], "Situação");
                egui::ComboBox::from_id_salt("project-status")
                    .selected_text(form.project.status.label())
                    .width(columns[1].available_width() - 24.0)
                    .show_ui(&mut columns[1], |ui| {
                        for status in ProjectStatus::ALL {
                            ui.selectable_value(&mut form.project.status, status, status.label());
                        }
                    });
            });
            ui.columns(2, |columns| {
                widgets::field_label(&mut columns[0], "Início");
                crate::labels::date_field(
                    &mut columns[0],
                    ("project-start", &form.project.uid),
                    &mut form.project.start_date,
                );
                widgets::field_label(&mut columns[1], "Data-alvo");
                crate::labels::date_field(
                    &mut columns[1],
                    ("project-target", &form.project.uid),
                    &mut form.project.target_date,
                );
            });
            if let Some(error) = &form.error {
                let (fg, bg) = Tone::Danger.colors();
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(error).color(fg)).wrap());
                        if form.conflict {
                            ui.horizontal(|ui| {
                                if form.theirs.is_some()
                                    && ui
                                        .add(widgets::secondary("Juntar as duas versões"))
                                        .on_hover_text("Traz o que mudou lá nos campos que você não mexeu e mantém o que você digitou nos outros. O projeto fica para você revisar e salvar.")
                                        .clicked()
                                {
                                    merge = true;
                                }
                                if ui
                                    .add(widgets::quiet("Ficar com a versão de lá"))
                                    .on_hover_text(
                                        "Traz a versão salva agora e descarta o que você digitou.",
                                    )
                                    .clicked()
                                {
                                    reload = true;
                                }
                            });
                        }
                    });
            }
            if let Some(notice) = &form.notice {
                widgets::small_muted(ui, notice);
            }
            ui.add_space(S1);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = S2;
                if ui
                    .add_enabled(!waiting, widgets::primary("Salvar"))
                    .on_disabled_hover_text("Escolha antes o que fazer com a versão de lá.")
                    .clicked()
                {
                    save = true;
                }
                if ui.add(widgets::quiet("Cancelar")).clicked() {
                    cancel = true;
                }
            });
        });
        if !open || cancel {
            self.form = None;
            return;
        }
        if reload {
            form.take_theirs(store);
            return;
        }
        if merge {
            form.merge_conflict();
            return;
        }
        if save {
            let was_new = form.base_version.is_none();
            if let Ok(saved) = form.save(store) {
                let name = saved.name.clone();
                self.form = None;
                self.data.stale = true;
                if was_new {
                    self.open_project(&saved.uid);
                }
                *asked = Some(Asked::Changed(Some(format!("Projeto “{name}” salvo."))));
            }
        }
    }

    fn include_window(
        &mut self,
        ctx: &egui::Context,
        store: &mut Store,
        asked: &mut Option<Asked>,
    ) {
        let Some(uid) = self.open.clone() else {
            self.include = None;
            return;
        };
        let Some(dialog) = self.include.as_mut() else {
            return;
        };
        let candidates = add_candidates(&self.data.tasks, &dialog.query);
        let documents = &self.data.documents;
        let mut open = true;
        let mut cancel = false;
        let mut confirm = false;
        egui::Window::new("Incluir tarefas existentes")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = S2;
                ui.set_width(500.0_f32.min(ui.available_width()));
                widgets::small_muted(
                    ui,
                    "Tarefas em aberto que ainda não estão em nenhum projeto. Marque as que entram neste.",
                );
                widgets::search(ui, &mut dialog.query, "Buscar tarefa", 500.0);
                egui::ScrollArea::vertical()
                    .id_salt("project-include")
                    .max_height(300.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S1;
                        if candidates.is_empty() {
                            widgets::muted(
                                ui,
                                if dialog.query.trim().is_empty() {
                                    "Não há tarefas em aberto fora de projetos."
                                } else {
                                    "Nenhuma tarefa encontrada."
                                },
                            );
                        }
                        for task in candidates.iter().take(CANDIDATE_LIMIT) {
                            let mut on = dialog.chosen.contains(&task.id);
                            let detail = candidate_detail(task, documents);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = S2;
                                if ui.checkbox(&mut on, "").changed() {
                                    if on {
                                        dialog.chosen.insert(task.id);
                                    } else {
                                        dialog.chosen.remove(&task.id);
                                    }
                                }
                                ui.with_layout(
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.y = 1.0;
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&task.title)
                                                    .font(theme::font_strong(theme::SIZE_BODY)),
                                            )
                                            .selectable(false)
                                            .truncate(),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(detail)
                                                    .size(theme::SIZE_SMALL)
                                                    .color(c().muted),
                                            )
                                            .selectable(false)
                                            .truncate(),
                                        );
                                    },
                                );
                            });
                        }
                        if candidates.len() > CANDIDATE_LIMIT {
                            widgets::small_muted(
                                ui,
                                format!(
                                    "e mais {}: use a busca para achar a tarefa.",
                                    candidates.len() - CANDIDATE_LIMIT
                                ),
                            );
                        }
                    });
                if let Some(error) = &dialog.error {
                    let (fg, bg) = Tone::Danger.colors();
                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(RichText::new(error).color(fg)).wrap());
                        });
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = S2;
                    let n = dialog.chosen.len();
                    let label = match n {
                        0 => "Incluir".to_owned(),
                        1 => "Incluir 1 tarefa".to_owned(),
                        n => format!("Incluir {n} tarefas"),
                    };
                    if ui.add_enabled(n > 0, widgets::primary(label)).clicked() {
                        confirm = true;
                    }
                    if ui.add(widgets::quiet("Cancelar")).clicked() {
                        cancel = true;
                    }
                });
            });
        if !open || cancel {
            self.include = None;
            return;
        }
        if confirm {
            let chosen: Vec<i64> = dialog.chosen.iter().copied().collect();
            let (added, failed) = include_tasks(store, &uid, &chosen);
            self.data.stale = true;
            if failed.is_empty() {
                self.include = None;
                *asked = Some(Asked::Changed(Some(match added {
                    1 => "1 tarefa incluída no projeto.".to_owned(),
                    n => format!("{n} tarefas incluídas no projeto."),
                })));
            } else {
                // What went in leaves the list; what did not stays, with the reason.
                dialog
                    .chosen
                    .retain(|id| failed.iter().any(|(f, _)| f == id));
                dialog.error = Some(format!(
                    "{} Não entraram: {}",
                    if added > 0 {
                        format!("{added} incluída(s).")
                    } else {
                        String::new()
                    },
                    failed
                        .iter()
                        .map(|(_, why)| why.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
                if added > 0 {
                    *asked = Some(Asked::Changed(None));
                }
            }
        }
    }
}

/// Puts each task in the project, reading it again first (it may have
/// changed or gone since the list was drawn). Returns how many went in and,
/// for the others, the id and the reason.
fn include_tasks(store: &mut Store, project_uid: &str, ids: &[i64]) -> (usize, Vec<(i64, String)>) {
    let current = store.list_tasks().unwrap_or_default();
    let mut added = 0;
    let mut failed = Vec::new();
    for id in ids {
        let Some(mut task) = current.iter().find(|t| t.id == *id).cloned() else {
            failed.push((*id, "uma tarefa não está mais disponível".to_owned()));
            continue;
        };
        if !task.project_uid.is_empty() && task.project_uid != project_uid {
            failed.push((*id, format!("“{}” já entrou em outro projeto", task.title)));
            continue;
        }
        task.project_uid = project_uid.to_owned();
        match store.save_task(&task) {
            Ok(_) => added += 1,
            Err(error) => failed.push((*id, format!("“{}”: {error}", task.title))),
        }
    }
    (added, failed)
}

/// The small line under a candidate: its document, deadline and assignee.
fn candidate_detail(task: &Task, documents: &BTreeMap<i64, (String, String)>) -> String {
    let mut parts = Vec::new();
    parts.push(match task.document_id {
        None => "Sem documento".to_owned(),
        Some(id) => documents
            .get(&id)
            .map(|(number, _)| number.clone())
            .unwrap_or_else(|| "documento".to_owned()),
    });
    if !task.due_date.is_empty() {
        parts.push(format!("prazo {}", day_month(&task.due_date)));
    }
    if !task.assignee.trim().is_empty() {
        parts.push(task.assignee.trim().to_owned());
    }
    parts.join(" · ")
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// The bar of progress: a track and the part done.
fn progress_bar(ui: &mut egui::Ui, done: usize, total: usize, width: f32) {
    let p = c();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = egui::CornerRadius::same(4);
    ui.painter().rect_filled(rect, radius, p.outline);
    if total > 0 && done > 0 {
        let fraction = (done as f32 / total as f32).clamp(0.0, 1.0);
        let filled = egui::Rect::from_min_size(rect.min, egui::vec2(width * fraction, 8.0));
        let color = if done >= total { p.ok } else { p.primary };
        ui.painter().rect_filled(filled, radius, color);
    }
}

/// The bar, "N de M tarefas concluídas · Próximo prazo: dd/mm", and the
/// pills of what is late and at risk: the pills are drawn first, from the
/// right, and the texts take what is left, so a long text ends in "…".
fn progress_line(ui: &mut egui::Ui, overview: &ProjectOverview, wide_bar: bool) {
    let progress = &overview.progress;
    let mut text = progress_text(progress.done, progress.total);
    if let Some(due) = &progress.next_due {
        text.push_str(&format!(" · Próximo prazo: {}", day_month(due)));
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = S2;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if overview.risk.deliveries_at_risk > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(overview.risk.deliveries_at_risk, "em risco", "em risco"),
                    Tone::Warn,
                )
                .on_hover_text("Entregas do projeto que o plano não cumpre no prazo.");
            }
            if progress.late > 0 {
                widgets::pill(
                    ui,
                    crate::labels::count(progress.late, "atrasada", "atrasadas"),
                    Tone::Danger,
                );
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                progress_bar(
                    ui,
                    progress.done,
                    progress.total,
                    if wide_bar { 260.0 } else { 180.0 },
                );
                ui.add(
                    egui::Label::new(RichText::new(text).color(c().muted))
                        .selectable(false)
                        .truncate(),
                );
            });
        });
    });
}

/// The warn line of a target date at risk, when it is.
fn risk_line(ui: &mut egui::Ui, overview: &ProjectOverview, project: &Project) {
    let (Some(end), Some(target)) = (overview.risk.target_at_risk, project.target()) else {
        return;
    };
    ui.add(
        egui::Label::new(
            RichText::new(format!(
                "{} {}",
                icon::WARNING,
                target_risk_text(target, end)
            ))
            .color(c().warn),
        )
        .wrap(),
    );
}

/// One project of the list: a card that opens it. Returns true when clicked.
fn project_card(ui: &mut egui::Ui, overview: &ProjectOverview, _today: NaiveDate) -> bool {
    let p = c();
    let project = &overview.project;
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(("project-card", &project.uid))
            .sense(egui::Sense::click()),
        |ui| {
            let hovered = ui.response().hovered();
            widgets::card_frame()
                .stroke(egui::Stroke::new(
                    1.0,
                    if hovered { p.primary } else { p.outline },
                ))
                .inner_margin(egui::Margin::symmetric(S4 as i8, S3 as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = S2;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S2;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            widgets::pill(ui, project.status.label(), status_tone(project.status));
                            if !project.assignee.is_empty() {
                                ui.scope(|ui| {
                                    ui.set_max_width(180.0);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&project.assignee)
                                                .size(theme::SIZE_SMALL)
                                                .color(p.muted),
                                        )
                                        .selectable(false)
                                        .truncate(),
                                    );
                                });
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&project.name)
                                                .font(theme::font_strong(theme::SIZE_SECTION)),
                                        )
                                        .selectable(false)
                                        .truncate(),
                                    );
                                },
                            );
                        });
                    });
                    progress_line(ui, overview, false);
                    risk_line(ui, overview, project);
                });
        },
    )
    .response
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

/// A titled card with the rows of a group of tasks (nothing when empty).
fn task_card(
    ui: &mut egui::Ui,
    title: &str,
    color: Option<egui::Color32>,
    tasks: &[&Task],
    today: &str,
    documents: &BTreeMap<i64, (String, String)>,
    out: (&mut Option<Task>, &mut Option<Task>, &mut Option<Task>),
) {
    if tasks.is_empty() {
        return;
    }
    widgets::overline(ui, &format!("{title} · {}", tasks.len()), color);
    ui.add_space(S1);
    task_list(ui, tasks, today, documents, out);
    ui.add_space(S3);
}

fn task_list(
    ui: &mut egui::Ui,
    tasks: &[&Task],
    today: &str,
    documents: &BTreeMap<i64, (String, String)>,
    out: (&mut Option<Task>, &mut Option<Task>, &mut Option<Task>),
) {
    let (conclude, open, remove) = out;
    list_card(ui, |ui| {
        for (index, task) in tasks.iter().enumerate() {
            let mut conclude_it = false;
            let mut remove_it = false;
            let clicked = row(
                ui,
                ("project-task", task.id),
                index + 1 == tasks.len(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S3;
                        task_row(ui, task, today, documents, &mut conclude_it, &mut remove_it);
                    });
                },
            );
            if conclude_it {
                *conclude = Some((*task).clone());
            } else if remove_it {
                *remove = Some((*task).clone());
            } else if clicked {
                *open = Some((*task).clone());
            }
        }
    });
}

/// The inside of a row of a project's task: the circle, then (drawn first,
/// from the right) the action and the pills, and the title with its document
/// line in what is left, so a long title ends in "…" instead of running under
/// the pills.
fn task_row(
    ui: &mut egui::Ui,
    task: &Task,
    today: &str,
    documents: &BTreeMap<i64, (String, String)>,
    conclude: &mut bool,
    remove: &mut bool,
) {
    let p = c();
    let done = task.is_done();
    if done {
        ui.label(RichText::new(icon::CHECK).size(22.0).color(p.ok));
    } else {
        *conclude = ui
            .add(
                egui::Button::new(RichText::new(icon::CIRCLE).size(22.0).color(p.muted))
                    .frame(false),
            )
            .on_hover_text("Conclui a tarefa. Nada é enviado ao SIGADOC.")
            .clicked();
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        if ui
            .add(widgets::quiet("Tirar do projeto"))
            .on_hover_text("A tarefa continua em Tarefas, sem projeto.")
            .clicked()
        {
            *remove = true;
        }
        if done {
            widgets::pill(ui, "Concluída", Tone::Ok);
        } else if task.due_date.is_empty() {
            widgets::pill(ui, "Sem prazo", Tone::Neutral);
        } else if task.due_date.as_str() < today {
            widgets::pill(
                ui,
                format!("Atrasada · {}", day_month(&task.due_date)),
                Tone::Danger,
            );
        } else if task.due_date == today {
            widgets::pill(ui, "Hoje", Tone::Warn);
        } else {
            widgets::pill(ui, day_month(&task.due_date), Tone::Info);
        }
        if !done && matches!(task.priority.as_str(), "urgente" | "alta") {
            widgets::pill(
                ui,
                crate::labels::priority(&task.priority),
                if task.priority == "urgente" {
                    Tone::Danger
                } else {
                    Tone::Warn
                },
            );
        }
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let mut title = RichText::new(&task.title).font(theme::font_strong(theme::SIZE_BODY));
            if done {
                title = title.color(p.muted);
            }
            ui.add(egui::Label::new(title).selectable(false).truncate());
            let line = match task.document_id {
                None => "Sem documento".to_owned(),
                Some(id) => documents
                    .get(&id)
                    .map(|(number, title)| format!("{number} · {title}"))
                    .unwrap_or_default(),
            };
            if !line.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(line).size(theme::SIZE_SMALL).color(p.muted))
                        .selectable(false)
                        .truncate(),
                );
            }
        });
    });
}

// ---------------------------------------------------------------------------
// The app's side
// ---------------------------------------------------------------------------

impl PonteApp {
    pub(super) fn projects_area(&mut self, ui: &mut egui::Ui) {
        let today = chrono::Local::now().date_naive();
        let asked = self.projects.show(ui, &mut self.store, today);
        match asked {
            None => {}
            Some(Asked::Conclude(task)) => self.conclude_from_today(task),
            Some(Asked::OpenTask(task)) => {
                // The list may be a few seconds old: open what the library holds now.
                let current = self
                    .store
                    .list_tasks()
                    .ok()
                    .and_then(|tasks| tasks.into_iter().find(|t| t.id == task.id));
                match current {
                    Some(current) => self.navigate(Leave::OpenTask(current)),
                    None => {
                        self.refresh();
                        self.message("Esta tarefa não está mais disponível.");
                    }
                }
            }
            Some(Asked::NewTask(uid)) => self.navigate(Leave::NewProjectTask(uid)),
            Some(Asked::Changed(text)) => {
                self.refresh();
                if let Some(text) = text {
                    self.message(text);
                }
            }
        }
    }
}

#[cfg(feature = "shots")]
impl ProjectsPanel {
    /// The project the pictures show: the active one with tasks, a plan and a
    /// target date.
    fn shots_project(store: &Store) -> Option<Project> {
        store
            .list_projects()
            .ok()?
            .into_iter()
            .find(|p| p.status == ProjectStatus::Ativo)
    }

    /// Puts the screen in the state called `view` (see `SHOTS`).
    pub(super) fn shots_open(&mut self, store: &Store, view: &str) {
        *self = Self::default();
        if view == "lista" {
            self.filter = StatusFilter::All;
            return;
        }
        let Some(project) = Self::shots_project(store) else {
            return;
        };
        self.open_project(&project.uid);
        match view {
            "linha-do-tempo" => self.view = View::Timeline,
            "formulario" => {
                let version = store.project_version(&project.uid).unwrap_or(0);
                self.form = Some(ProjectForm::edit(&project, version));
                if let Some(form) = self.form.as_mut() {
                    form.focus = false;
                }
            }
            "incluir" => {
                self.include = Some(IncludeDialog::default());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{Progress, ProjectRisk};

    fn project(uid: &str, name: &str, objective: &str, status: ProjectStatus) -> ProjectOverview {
        ProjectOverview {
            project: Project {
                uid: uid.into(),
                name: name.into(),
                objective: objective.into(),
                status,
                ..Project::default()
            },
            progress: Progress::default(),
            risk: ProjectRisk::default(),
        }
    }

    fn names(list: &[&ProjectOverview]) -> Vec<String> {
        list.iter().map(|o| o.project.name.clone()).collect()
    }

    fn task(id: i64, title: &str, due: &str) -> Task {
        let mut task = Task::blank(None);
        task.id = id;
        task.title = title.into();
        task.due_date = due.into();
        task
    }

    #[test]
    fn the_list_is_filtered_by_situation_and_searched_by_name_and_objective() {
        let all = vec![
            project(
                "a",
                "Reunião do Comitê",
                "Garantir auditório",
                ProjectStatus::Ativo,
            ),
            project(
                "b",
                "Quadro de pessoal",
                "Republicar a escala",
                ProjectStatus::Pausado,
            ),
            project("c", "Arquivo morto", "", ProjectStatus::Arquivado),
            project(
                "d",
                "Atualização do site",
                "Texto novo",
                ProjectStatus::Ativo,
            ),
            project("e", "Entrega final", "", ProjectStatus::Concluido),
        ];
        assert_eq!(
            names(&list_view(&all, StatusFilter::Active, "")),
            ["Atualização do site", "Reunião do Comitê"],
            "active ones by name, ignoring accents"
        );
        assert_eq!(
            names(&list_view(&all, StatusFilter::Paused, "")),
            ["Quadro de pessoal"]
        );
        assert_eq!(
            names(&list_view(&all, StatusFilter::Done, "")),
            ["Entrega final"]
        );
        assert_eq!(
            names(&list_view(&all, StatusFilter::Archived, "")),
            ["Arquivo morto"]
        );
        // "Todos" groups by situation first, then by name.
        assert_eq!(
            names(&list_view(&all, StatusFilter::All, "")),
            [
                "Atualização do site",
                "Reunião do Comitê",
                "Quadro de pessoal",
                "Entrega final",
                "Arquivo morto"
            ]
        );
        // The search looks at the name and the objective, ignoring case and accents.
        assert_eq!(
            names(&list_view(&all, StatusFilter::All, "REUNIAO")),
            ["Reunião do Comitê"]
        );
        assert_eq!(
            names(&list_view(&all, StatusFilter::All, "escala")),
            ["Quadro de pessoal"]
        );
        assert_eq!(
            names(&list_view(&all, StatusFilter::All, "comite auditorio")),
            ["Reunião do Comitê"],
            "every word must be found"
        );
        assert!(list_view(&all, StatusFilter::Active, "escala").is_empty());
        assert_eq!(count_in(&all, StatusFilter::Active), 2);
        for filter in StatusFilter::ALL {
            assert!(filter.none_text().starts_with("Nenhum projeto"));
        }
        assert_eq!(
            StatusFilter::Archived.none_text(),
            "Nenhum projeto arquivado."
        );
        assert_eq!(count_in(&all, StatusFilter::All), 5);
    }

    #[test]
    fn progress_and_risk_have_their_sentences() {
        assert_eq!(progress_text(0, 0), "Nenhuma tarefa ainda");
        assert_eq!(progress_text(0, 1), "0 de 1 tarefa concluída");
        assert_eq!(progress_text(1, 1), "1 de 1 tarefa concluída");
        assert_eq!(progress_text(3, 8), "3 de 8 tarefas concluídas");
        let target = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let end = chrono::NaiveDate::from_ymd_opt(2026, 11, 25)
            .unwrap()
            .and_hms_opt(18, 0, 0)
            .unwrap();
        assert_eq!(
            target_risk_text(target, end),
            "Data-alvo 20/11 em risco: término previsto 25/11"
        );
    }

    #[test]
    fn the_tasks_of_a_project_are_grouped_late_open_done() {
        let mut done_old = task(1, "Feita antes", "2026-11-01");
        done_old.status = "concluida".into();
        done_old.completed_at = Some("2026-11-02T10:00:00+00:00".into());
        let mut done_new = task(2, "Feita depois", "");
        done_new.status = "concluida".into();
        done_new.completed_at = Some("2026-11-09T10:00:00+00:00".into());
        let tasks = vec![
            task(3, "Sem prazo", ""),
            task(4, "Ontem", "2026-11-09"),
            task(5, "Amanhã", "2026-11-11"),
            task(6, "Hoje", "2026-11-10"),
            task(7, "Há mais tempo", "2026-11-03"),
            done_old,
            done_new,
        ];
        let groups = task_groups(&tasks, "2026-11-10");
        let titles = |v: &[&Task]| v.iter().map(|t| t.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles(&groups.late), ["Há mais tempo", "Ontem"]);
        assert_eq!(
            titles(&groups.open),
            ["Hoje", "Amanhã", "Sem prazo"],
            "today is not late; no deadline goes last"
        );
        assert_eq!(titles(&groups.done), ["Feita depois", "Feita antes"]);
        assert_eq!(task_groups(&[], "2026-11-10"), TaskGroups::default());
    }

    #[test]
    fn only_open_tasks_without_project_are_offered_and_the_search_narrows_them() {
        let mut in_project = task(1, "Já está num projeto", "2026-11-05");
        in_project.project_uid = "x".repeat(32);
        let mut done = task(2, "Concluída", "2026-11-05");
        done.status = "concluida".into();
        let mut notes = task(3, "Pedir os veículos", "");
        notes.notes = "Falar com a Seção de Transportes".into();
        let mut assigned = task(4, "Revisar texto", "2026-11-08");
        assigned.assignee = "Servidor B".into();
        let tasks = vec![
            in_project,
            done,
            notes,
            assigned,
            task(5, "Confirmar a sala", "2026-11-04"),
        ];
        let titles = |q: &str| {
            add_candidates(&tasks, q)
                .iter()
                .map(|t| t.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(""),
            ["Confirmar a sala", "Revisar texto", "Pedir os veículos"],
            "by deadline, the ones without last"
        );
        assert_eq!(titles("SALA"), ["Confirmar a sala"]);
        assert_eq!(
            titles("transportes"),
            ["Pedir os veículos"],
            "notes are searched"
        );
        assert_eq!(
            titles("servidor b"),
            ["Revisar texto"],
            "so is the assignee"
        );
        assert!(
            titles("projeto").is_empty(),
            "a task of a project is never offered"
        );
        assert!(titles("concluída").is_empty());
    }

    #[test]
    fn the_situations_have_a_colour_and_deleting_says_what_happens_to_the_tasks() {
        assert_eq!(status_tone(ProjectStatus::Ativo), Tone::Ok);
        assert_eq!(status_tone(ProjectStatus::Pausado), Tone::Warn);
        assert_eq!(status_tone(ProjectStatus::Concluido), Tone::Info);
        assert_eq!(status_tone(ProjectStatus::Arquivado), Tone::Neutral);
        assert!(delete_sentence("Alfa", 0).contains("não tem tarefas"));
        assert!(delete_sentence("Alfa", 1).contains("A 1 tarefa dele continua"));
        let many = delete_sentence("Alfa", 5);
        assert!(
            many.contains("“Alfa”")
                && many.contains("As 5 tarefas")
                && many.contains("Sem projeto")
        );
    }

    #[test]
    fn a_candidate_line_names_the_document_deadline_and_assignee() {
        let mut documents = BTreeMap::new();
        documents.insert(7, ("ORG-OFI-2026/00431".to_owned(), "Ofício".to_owned()));
        let mut t = task(1, "X", "2026-11-04");
        assert_eq!(
            candidate_detail(&t, &documents),
            "Sem documento · prazo 04/11"
        );
        t.document_id = Some(7);
        t.assignee = " Servidor A ".into();
        assert_eq!(
            candidate_detail(&t, &documents),
            "ORG-OFI-2026/00431 · prazo 04/11 · Servidor A"
        );
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("p.sqlite3")).unwrap();
        (dir, store)
    }

    #[test]
    fn including_tasks_puts_them_in_the_project_and_reports_the_ones_that_cannot() {
        let (_dir, mut store) = store();
        let project = store.save_project(&Project::new("Alfa")).unwrap();
        let other = store.save_project(&Project::new("Beta")).unwrap();
        let mut ids = Vec::new();
        for title in ["Um", "Dois", "Três"] {
            let mut t = Task::blank(None);
            t.title = title.into();
            ids.push(store.save_task(&t).unwrap());
        }
        // The third entered another project after the list was drawn.
        let mut third = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == ids[2])
            .unwrap();
        third.project_uid = other.uid.clone();
        store.save_task(&third).unwrap();
        ids.push(987_654);
        let (added, failed) = include_tasks(&mut store, &project.uid, &ids);
        assert_eq!(added, 2);
        assert_eq!(failed.len(), 2);
        assert!(failed
            .iter()
            .any(|(id, why)| *id == ids[2] && why.contains("já entrou em outro projeto")));
        assert!(failed
            .iter()
            .any(|(id, why)| *id == 987_654 && why.contains("não está mais disponível")));
        let tasks = store.list_tasks().unwrap();
        let in_alfa = tasks
            .iter()
            .filter(|t| t.project_uid == project.uid)
            .count();
        assert_eq!(in_alfa, 2);
        assert_eq!(
            tasks.iter().find(|t| t.id == ids[2]).unwrap().project_uid,
            other.uid,
            "the one in another project was left alone"
        );
    }

    fn app() -> (tempfile::TempDir, PonteApp) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acervo.sqlite3");
        let store = Store::open(&path).unwrap();
        (dir, PonteApp::new(store, path))
    }

    #[test]
    fn a_new_task_for_a_project_opens_the_form_with_the_project_chosen() {
        let (_dir, mut app) = app();
        let project = app.store.save_project(&Project::new("Alfa")).unwrap();
        app.area = super::super::Area::Projects;
        app.navigate(Leave::NewProjectTask(project.uid.clone()));
        assert!(app.area == super::super::Area::Tasks);
        assert_eq!(app.tasks.draft_project_uid(), Some(project.uid.as_str()));
        assert!(
            !app.tasks.dirty(),
            "the project alone is what the button proposes, not unsaved work"
        );
        assert!(app.leave.is_none());
    }

    #[test]
    fn a_new_task_for_a_project_waits_for_an_unsaved_edit() {
        let (_dir, mut app) = app();
        let project = app.store.save_project(&Project::new("Alfa")).unwrap();
        assert!(app.tasks.start_new(None));
        app.tasks.set_draft_title("Em edição");
        app.area = super::super::Area::Projects;
        app.navigate(Leave::NewProjectTask(project.uid.clone()));
        assert!(app.leave.is_some(), "the person is asked first");
        assert!(app.area == super::super::Area::Projects);
        assert_eq!(app.tasks.draft_project_uid(), Some(""));
    }

    #[test]
    fn opening_a_project_from_the_search_goes_to_the_area_with_it_open() {
        let (_dir, mut app) = app();
        let project = app.store.save_project(&Project::new("Alfa")).unwrap();
        app.navigate(Leave::Project(project.uid.clone()));
        assert!(app.area == super::super::Area::Projects);
        assert_eq!(app.projects.open.as_deref(), Some(project.uid.as_str()));
        app.navigate(Leave::NewProject);
        assert!(app.projects.form.is_some());
    }

    /// Runs the panel headless on a library with one project and its tasks, in
    /// each view: nothing panics and the dialogs open and close.
    #[test]
    fn the_screens_draw() {
        let (_dir, mut store) = store();
        let mut project = Project::new("Alfa");
        project.objective = "Um objetivo".into();
        project.target_date = "2026-11-20".into();
        let project = store.save_project(&project).unwrap();
        let mut t = Task::blank(None);
        t.title = "Primeira".into();
        t.project_uid = project.uid.clone();
        t.scheduled_start = "2026-11-03T09:00".into();
        t.scheduled_end = "2026-11-05T18:00".into();
        t.due_date = "2026-11-04".into();
        store.save_task(&t).unwrap();
        let mut free = Task::blank(None);
        free.title = "Solta".into();
        store.save_task(&free).unwrap();
        let ctx = egui::Context::default();
        let mut panel = ProjectsPanel::default();
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let frame = |panel: &mut ProjectsPanel, store: &mut Store| {
            let mut output = ctx.run_ui(input(), |ui| {
                panel.show(ui, store, today);
            });
            output.textures_delta.clear();
        };
        frame(&mut panel, &mut store);
        assert_eq!(panel.data.overviews.len(), 1);
        panel.open_project(&project.uid);
        frame(&mut panel, &mut store);
        assert_eq!(panel.data.project_tasks.len(), 1);
        panel.view = View::Timeline;
        frame(&mut panel, &mut store);
        panel.start_new_project();
        panel.include = Some(IncludeDialog::default());
        frame(&mut panel, &mut store);
        assert!(panel.key_escape(), "closes the dialog");
        assert!(panel.key_escape(), "closes the form");
        assert!(panel.key_escape(), "leaves the project");
        assert!(!panel.key_escape());
    }

    fn project_store() -> (tempfile::TempDir, Store, Project) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("acervo.sqlite3")).unwrap();
        let project = store.save_project(&Project::new("Projeto X")).unwrap();
        (dir, store, project)
    }

    fn set_version(store: &Store, uid: &str, version: i64) {
        store
            .conn
            .execute(
                "UPDATE item_versions SET version=?1 WHERE kind='project' AND item_id=?2",
                rusqlite::params![version, crate::work_templates::version_key(uid)],
            )
            .unwrap();
    }

    /// Review 3, finding 2: only the version moved (an exchange rewrote
    /// the project with the same content, or brought its number back): the
    /// form saves on the current version, on the first try.
    #[test]
    fn rev3_the_project_form_saves_when_only_the_version_moved() {
        let (_dir, mut store, project) = project_store();
        let held = store.project_version(&project.uid).unwrap();
        let mut form = ProjectForm::edit(&project, held);
        form.project.objective = "digitado".into();
        // Rewritten with the same content (new stamps), and the number moved.
        store.save_project(&project).unwrap();
        set_version(&store, &project.uid, held + 5);
        let saved = form.save(&mut store).expect("only the version moved");
        assert_eq!(saved.objective, "digitado");
        // And when the number went back below the one the form holds.
        let held = store.project_version(&project.uid).unwrap();
        let read = store.get_project(&project.uid).unwrap().unwrap();
        let mut form = ProjectForm::edit(&read, held);
        set_version(&store, &project.uid, 1);
        form.project.objective = "de novo".into();
        form.save(&mut store).expect("never a dead end");
        assert_eq!(
            store.get_project(&project.uid).unwrap().unwrap().objective,
            "de novo"
        );
    }

    /// Review 3, finding 1: the project changed elsewhere and the exchange
    /// left it on the number the form holds. Nothing is written, the typing
    /// stays, and "Juntar as duas versões" keeps both.
    #[test]
    fn rev3_the_project_form_never_overwrites_an_edit_on_the_same_version() {
        let (_dir, mut store, project) = project_store();
        let held = store.project_version(&project.uid).unwrap();
        let mut form = ProjectForm::edit(&project, held);
        form.project.objective = "digitado".into();
        let mut there = project.clone();
        there.assignee = "Ana".into();
        store.save_project_checked(&there, Some(held)).unwrap();
        set_version(&store, &project.uid, held);
        let error = form.save(&mut store).unwrap_err();
        assert!(error.contains("Conflito de edição simultânea"), "{error}");
        assert!(error.contains("Responsável"), "{error}");
        assert!(form.conflict && form.theirs.is_some());
        let now = store.get_project(&project.uid).unwrap().unwrap();
        assert_eq!((now.assignee.as_str(), now.objective.as_str()), ("Ana", ""));
        assert_eq!(form.project.objective, "digitado", "the typing stays");
        assert!(form.save(&mut store).is_err(), "waits for the choice");
        assert!(form.merge_conflict());
        assert_eq!(
            (
                form.project.assignee.as_str(),
                form.project.objective.as_str()
            ),
            ("Ana", "digitado")
        );
        form.save(&mut store)
            .expect("joined, on the current version");
        let now = store.get_project(&project.uid).unwrap().unwrap();
        assert_eq!(
            (now.assignee.as_str(), now.objective.as_str()),
            ("Ana", "digitado")
        );
    }

    /// "Ficar com a versão de lá" shows the project as saved now, and the
    /// next save goes through.
    #[test]
    fn rev3_the_project_form_can_keep_the_other_version() {
        let (_dir, mut store, project) = project_store();
        let held = store.project_version(&project.uid).unwrap();
        let mut form = ProjectForm::edit(&project, held);
        form.project.objective = "digitado".into();
        let mut there = project.clone();
        there.objective = "de lá".into();
        store.save_project_checked(&there, Some(held)).unwrap();
        set_version(&store, &project.uid, held);
        assert!(form.save(&mut store).is_err());
        form.take_theirs(&store);
        assert_eq!(form.project.objective, "de lá");
        assert!(!form.conflict && form.theirs.is_none());
        form.project.name = "Projeto Y".into();
        form.save(&mut store).unwrap();
        let now = store.get_project(&project.uid).unwrap().unwrap();
        assert_eq!(
            (now.name.as_str(), now.objective.as_str()),
            ("Projeto Y", "de lá")
        );
    }
}
