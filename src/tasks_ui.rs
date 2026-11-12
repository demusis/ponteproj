//! The Tarefas panel: list views, the task form, dependency marks and
//! attachments of a task.
use crate::theme::{self, c, icon, S1, S2, S3, S4};
use crate::widgets::{self, Tone};
use crate::{storage::Store, tasks::Task};
use eframe::egui::{self, RichText};
use std::collections::BTreeSet;

/// Views over the task list. The first four are the everyday ones; the rest
/// live under "Mais".
const VIEWS: [&str; 9] = [
    "Todas",
    "A triar",
    "Hoje",
    "Próximos 7 dias",
    "Atrasadas",
    "Aguardando",
    "Com lembrete",
    "Sem prazo",
    "Prontas para fazer",
];
const PRIMARY_VIEWS: [usize; 4] = [0, 2, 3, 4];
const SECONDARY_VIEWS: [usize; 5] = [8, 1, 5, 6, 7];
/// Width of the side panel with the task form.
const FORM_WIDTH: f32 = 420.0;
/// Room kept on the right of a row for the priority and deadline pills.
const PILLS_WIDTH: f32 = 170.0;
/// Size of the round conclude control of a row.
const CONTROL: f32 = 22.0;

/// "2026-11-07" → "07/11"; anything else as it is written.
fn short_date(iso: &str) -> String {
    chrono::NaiveDate::parse_from_str(iso.trim(), "%Y-%m-%d")
        .map(|d| d.format("%d/%m").to_string())
        .unwrap_or_else(|_| crate::labels::date(iso))
}

/// Icon and colour that say where a related task stands.
fn state_mark(task: &Task) -> (&'static str, egui::Color32) {
    if task.status == "concluida" {
        (icon::CHECK, c().muted)
    } else if task.overdue() {
        (icon::WARNING, c().danger)
    } else {
        (icon::CLOCK, c().warn)
    }
}

/// A clickable link naming a related task with its state; returns the task
/// when clicked, so the list can open it.
fn dependency_chip(ui: &mut egui::Ui, task: &Task) -> Option<Task> {
    let (mark, color) = state_mark(task);
    let mut text = RichText::new(format!("{mark} {}", task.title))
        .size(theme::SIZE_SMALL)
        .color(color);
    if task.status == "concluida" {
        text = text.strikethrough();
    }
    let response = ui
        .add(egui::Button::new(text).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!(
            "{}{} • clique para abrir",
            crate::tasks::status_label(&task.status),
            if task.due_date.is_empty() {
                String::new()
            } else {
                format!(" • prazo {}", crate::labels::date(&task.due_date))
            }
        ));
    response.clicked().then(|| task.clone())
}

/// Names of tasks for a tooltip: "A, B e C".
fn titles(tasks: &[&Task]) -> String {
    let names: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].to_owned(),
        n => format!("{} e {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// One line of the "Cadeia de dependências" block: state, title, "Abrir".
fn chain_row(ui: &mut egui::Ui, task: &Task, open: &mut Option<Task>) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(S3);
        let (mark, color) = state_mark(task);
        ui.label(RichText::new(mark).color(color));
        ui.label(&task.title);
        widgets::small_muted(
            ui,
            format!("• {}", crate::tasks::status_label(&task.status)),
        );
        if !task.due_date.is_empty() {
            widgets::small_muted(
                ui,
                format!("• prazo {}", crate::labels::date(&task.due_date)),
            );
        }
        if ui.add(widgets::quiet("Abrir")).clicked() {
            *open = Some(task.clone());
        }
    });
}

/// Two fields side by side; `frac` is the share of the left one.
fn two_columns(
    ui: &mut egui::Ui,
    frac: f32,
    left: impl FnOnce(&mut egui::Ui),
    right: impl FnOnce(&mut egui::Ui),
) {
    let gap = S3;
    let total = ui.available_width();
    let left_width = ((total - gap) * frac).floor();
    let right_width = total - gap - left_width;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(left_width);
                left(ui);
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(right_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(right_width);
                right(ui);
            },
        );
    });
}

/// Orders of the list inside each group.
const SORTS: [&str; 3] = ["Prazo", "Prioridade", "Documento"];

type DocumentIndex<'a> = std::collections::HashMap<i64, &'a crate::model::DocumentSummary>;
type Contexts = std::collections::BTreeMap<i64, crate::work_context::WorkContext>;

/// What the library held at the last load, kept between frames.
#[derive(Default)]
struct Loaded {
    tasks: Vec<Task>,
    documents: Vec<crate::model::DocumentSummary>,
    attachment_counts: std::collections::BTreeMap<i64, usize>,
    contexts: Contexts,
}

/// The rules that narrow the list besides the chosen view.
struct Scope<'a> {
    document: Option<i64>,
    /// Search text, already in lower case; empty means no search.
    query: &'a str,
    show_done: bool,
    today: &'a str,
    week: &'a str,
}

/// True when the task belongs to the document filter and the search text.
fn in_scope(
    task: &Task,
    document: Option<&crate::model::DocumentSummary>,
    context: Option<&crate::work_context::WorkContext>,
    scope: &Scope,
) -> bool {
    if scope
        .document
        .is_some_and(|id| task.document_id != Some(id))
    {
        return false;
    }
    if !scope.show_done && task.status == "concluida" {
        return false;
    }
    if scope.query.is_empty() {
        return true;
    }
    let extra = context
        .map(|c| {
            format!(
                "{} {} {} {} {}",
                c.subjects, c.document_type, c.purpose, c.process_number, c.sender
            )
        })
        .unwrap_or_default();
    format!(
        "{} {} {} {} {extra} {} {} {} {}",
        task.title,
        document.map(|d| d.document_number.as_str()).unwrap_or(""),
        document.map(|d| d.title.as_str()).unwrap_or(""),
        if task.document_id.is_none() {
            "sem documento avulsa"
        } else {
            ""
        },
        task.status,
        task.priority,
        task.action,
        task.assignee
    )
    .to_lowercase()
    .contains(scope.query)
}

/// True when the task belongs to the view with this index of `VIEWS`.
fn in_view(
    view: usize,
    task: &Task,
    context: Option<&crate::work_context::WorkContext>,
    deps: &crate::tasks::Dependencies,
    scope: &Scope,
) -> bool {
    let pending = task.status != "concluida";
    let due = task.due_date.as_str();
    match view {
        // Triage is about a document: a task without one never needs it.
        1 => {
            pending
                && task.document_id.is_some()
                && context.is_none_or(|c| c.purpose == "nao definido")
        }
        2 => pending && !due.is_empty() && due == scope.today,
        3 => pending && !due.is_empty() && due >= scope.today && due <= scope.week,
        4 => pending && !due.is_empty() && due < scope.today,
        5 => task.status == "aguardando",
        6 => {
            pending && !task.reminder_date.is_empty() && task.reminder_date.as_str() <= scope.today
        }
        7 => pending && due.is_empty(),
        8 => pending && !deps.is_blocked(task.id),
        _ => true,
    }
}

/// Counts shown on the chips: what each view would list under the current
/// document filter and search. Order: Todas, Hoje, Próximos 7 dias, Atrasadas.
fn view_counts(
    tasks: &[Task],
    documents: &DocumentIndex,
    contexts: &Contexts,
    deps: &crate::tasks::Dependencies,
    scope: &Scope,
) -> [usize; 4] {
    let mut counts = [0; 4];
    for task in tasks {
        let context = task.document_id.and_then(|id| contexts.get(&id));
        if !in_scope(
            task,
            task.document_id.and_then(|id| documents.get(&id).copied()),
            context,
            scope,
        ) {
            continue;
        }
        for (slot, view) in [0, 2, 3, 4].into_iter().enumerate() {
            if in_view(view, task, context, deps, scope) {
                counts[slot] += 1;
            }
        }
    }
    counts
}

/// Reorders a list already in dependency order for the chosen sort; the sort
/// is stable, so ties keep the dependency order.
fn sort_tasks<'a>(
    mut tasks: Vec<&'a Task>,
    sort: usize,
    documents: &DocumentIndex,
) -> Vec<&'a Task> {
    match sort {
        1 => tasks.sort_by_key(|t| match t.priority.as_str() {
            "urgente" => 0,
            "alta" => 1,
            "normal" | "" => 2,
            _ => 3,
        }),
        // Tasks without document come first, then by number.
        2 => tasks.sort_by_key(|t| {
            (
                t.document_id.is_some(),
                t.document_id
                    .and_then(|id| documents.get(&id))
                    .map(|d| d.document_number.to_lowercase())
                    .unwrap_or_default(),
            )
        }),
        _ => {}
    }
    tasks
}

/// The sentence of an empty list, from why it is empty.
fn empty_text(query: &str, narrowed: bool, no_tasks: bool, view: usize, show_done: bool) -> String {
    if !query.trim().is_empty() {
        return format!("Nenhuma tarefa encontrada para “{}”.", query.trim());
    }
    if narrowed {
        return if view == 0 && !show_done {
            "Este documento não tem tarefas em aberto."
        } else {
            "Este documento não tem tarefas nesta visão."
        }
        .to_owned();
    }
    if no_tasks {
        return "Ainda não há tarefas. Cada documento importado ganha uma tarefa de triagem, e “Nova tarefa” cria uma tarefa sem documento."
            .to_owned();
    }
    match view {
        1 => "Nenhuma tarefa a triar.",
        2 => "Nenhuma tarefa para hoje.",
        3 => "Nenhuma tarefa nos próximos 7 dias.",
        4 => "Nenhuma tarefa atrasada.",
        5 => "Nenhuma tarefa aguardando.",
        6 => "Nenhum lembrete vencido.",
        7 => "Nenhuma tarefa sem prazo.",
        8 => "Nenhuma tarefa pronta para fazer.",
        _ if show_done => "Nenhuma tarefa.",
        // Nothing is narrowing the list: say so, or it looks filtered.
        _ => "Não há nenhuma tarefa em aberto no acervo: a lista está completa, sem filtro.",
    }
    .to_owned()
}

/// What the list needs to draw its rows.
struct ListEnv<'a> {
    tasks: &'a [Task],
    documents: &'a [crate::model::DocumentSummary],
    deps: &'a crate::tasks::Dependencies<'a>,
    attachment_counts: &'a std::collections::BTreeMap<i64, usize>,
    focus_before: &'a [i64],
    focus_after: &'a [i64],
    today: &'a str,
    /// Task open in the form, and whether its edit is unsaved.
    editing: Option<i64>,
    editing_dirty: bool,
    /// Selection mode: rows show a checkbox and a tap toggles them.
    selecting: bool,
    selected: &'a BTreeSet<i64>,
}

/// What the person asked for in the list during this frame.
#[derive(Default)]
struct RowOut {
    select: Option<Task>,
    complete: Option<i64>,
    reopen: Option<i64>,
    /// A row to add to or take out of the selection.
    toggle: Option<i64>,
}

/// The square on the left of a row in selection mode, in place of the round
/// conclude control. Returns true when clicked.
fn select_control(ui: &mut egui::Ui, selected: bool) -> bool {
    let p = c();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(CONTROL, CONTROL), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let square = rect.shrink(2.0);
        let radius = egui::CornerRadius::same(5);
        if selected {
            painter.rect_filled(square, radius, p.primary);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                icon::CHECK,
                theme::font(14.0),
                p.on_primary,
            );
        } else {
            let stroke = if response.hovered() {
                p.primary
            } else {
                p.muted
            };
            painter.rect(
                square,
                radius,
                p.surface,
                egui::Stroke::new(1.5, stroke),
                egui::StrokeKind::Inside,
            );
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if selected {
            "Tirar da seleção"
        } else {
            "Selecionar esta tarefa"
        })
        .clicked()
}

/// The round control on the left of a row: concludes a pending task, reopens
/// a concluded one. Returns true when clicked.
fn conclude_control(
    ui: &mut egui::Ui,
    done: bool,
    enabled: bool,
    hint: &str,
    disabled_hint: &str,
) -> bool {
    let p = c();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(CONTROL, CONTROL), sense);
    let hovered = enabled && response.hovered();
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center = rect.center();
        if done {
            painter.circle_filled(center, CONTROL / 2.0 - 1.0, p.ok);
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                icon::CHECK,
                theme::font(14.0),
                p.on_primary,
            );
        } else {
            let (stroke, fill) = if hovered {
                (p.primary, p.primary_soft)
            } else if enabled {
                (p.muted, egui::Color32::TRANSPARENT)
            } else {
                (p.outline, egui::Color32::TRANSPARENT)
            };
            painter.circle(
                center,
                CONTROL / 2.0 - 1.5,
                fill,
                egui::Stroke::new(1.5, stroke),
            );
            if hovered {
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    icon::CHECK,
                    theme::font(14.0),
                    p.primary,
                );
            }
        }
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if enabled {
        response.on_hover_text(hint).clicked()
    } else {
        response.on_hover_text(disabled_hint);
        false
    }
}

/// One task of the list. `first`/`last` round the corners of the card.
fn task_row(
    ui: &mut egui::Ui,
    env: &ListEnv,
    task: &Task,
    first: bool,
    last: bool,
    // True when a task this one waits for is right above it, in the same
    // group: only then the row is drawn hanging from it.
    nested: bool,
    out: &mut RowOut,
) {
    let p = c();
    let editing_this = env.editing == Some(task.id);
    let is_selected = env.selecting && env.selected.contains(&task.id);
    let pending = task.status != "concluida";
    let document = task
        .document_id
        .and_then(|id| env.documents.iter().find(|d| d.id == id));
    let level = usize::from(nested && env.deps.level(task.id) > 0);
    let indent = 18.0 * level as f32;
    let direct = env.deps.direct(task.id);
    let blockers = env.deps.open_blockers(task.id);
    let blocked = pending && !blockers.is_empty();
    let releases: Vec<&Task> = env
        .tasks
        .iter()
        .filter(|t| t.dependencies.contains(&task.id))
        .collect();
    let relation = if env.focus_before.contains(&task.id) {
        Some((p.warn, "Esta tarefa vem antes da tarefa aberta na ficha."))
    } else if env.focus_after.contains(&task.id) {
        Some((
            p.ok,
            "Esta tarefa vem depois da tarefa aberta na ficha: depende dela.",
        ))
    } else {
        None
    };

    // The row answers clicks on its empty parts. Its area is registered
    // before the content (from the last frame), so the controls inside stay
    // on top of it.
    let id = ui.make_persistent_id(("task-row", task.id));
    let cached: Option<egui::Rect> = ui.data(|d| d.get_temp(id));
    let row = cached.map(|rect| ui.interact(rect, id, egui::Sense::click()));
    let background = ui.painter().add(egui::Shape::Noop);

    let inner = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(S3 as i8, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = S3;
                if indent > 0.0 {
                    ui.add_space(indent);
                }
                if env.selecting {
                    if select_control(ui, is_selected) {
                        out.toggle = Some(task.id);
                    }
                } else if pending {
                    let enabled = !(editing_this && env.editing_dirty) && !blocked;
                    let disabled_hint = if blocked {
                        format!("Conclua antes: {}", titles(&blockers))
                    } else {
                        "Salve ou cancele a edição desta tarefa antes de concluí-la.".to_owned()
                    };
                    if conclude_control(
                        ui,
                        false,
                        enabled,
                        "Conclui a tarefa. Nada é enviado ao SIGADOC.",
                        &disabled_hint,
                    ) {
                        out.complete = Some(task.id);
                    }
                } else if conclude_control(
                    ui,
                    true,
                    !(editing_this && env.editing_dirty),
                    "Reabrir esta tarefa",
                    "Salve ou cancele a edição desta tarefa antes de reabri-la.",
                ) {
                    out.reopen = Some(task.id);
                }
                let middle = (ui.available_width() - PILLS_WIDTH - S3).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(middle, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(middle);
                        ui.spacing_mut().item_spacing.y = S1;
                        ui.spacing_mut().interact_size.y = 18.0;
                        ui.spacing_mut().button_padding.y = 0.0;
                        let mut title = RichText::new(&task.title).font(theme::font_strong(theme::SIZE_BODY));
                        if !pending {
                            title = title.color(p.muted).strikethrough();
                        }
                        ui.add(egui::Label::new(title).selectable(false).wrap());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = S1;
                            if let Some(d) = document.filter(|d| d.manual) {
                                let email = d.email;
                                widgets::pill(ui, if email { "E-mail" } else { "Manual" }, Tone::Warn)
                                    .on_hover_text(if email {
                                        "E-mail importado no Ponte, fora do SIGADOC."
                                    } else {
                                        "Registro criado no Ponte, fora do SIGADOC."
                                    });
                            }
                            let mut line = match (task.document_id, document) {
                                (None, _) => "Sem documento".to_owned(),
                                (Some(_), Some(d)) => format!("{} · {}", d.document_number, d.title),
                                (Some(_), None) => "documento".to_owned(),
                            };
                            if pending && task.status != "pendente" {
                                line.push_str(&format!(" · {}", crate::tasks::status_label(&task.status)));
                            }
                            if !task.assignee.trim().is_empty() {
                                line.push_str(&format!(" · {}", task.assignee.trim()));
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(line).size(theme::SIZE_SMALL).color(p.muted),
                                )
                                .selectable(false),
                            );
                            if level > 4 {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("· nível {level}"))
                                            .size(theme::SIZE_SMALL)
                                            .color(p.muted),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text("Posição na cadeia de dependências. O recuo da linha para no 4º nível.");
                            }
                            if let Some(count) = env.attachment_counts.get(&task.id) {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{} {count}", icon::CLIP))
                                            .size(theme::SIZE_SMALL)
                                            .color(p.muted),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text(format!("{} nesta tarefa", crate::labels::count(*count, "anexo", "anexos")));
                            }
                        });
                        if !direct.is_empty() {
                            let (color, text) = if blocked {
                                (
                                    p.warn,
                                    format!("{} de {} em aberto", blockers.len(), crate::labels::count(direct.len(), "anterior", "anteriores")),
                                )
                            } else if pending {
                                (p.ok, crate::labels::count(direct.len(), "anterior concluída", "anteriores concluídas"))
                            } else {
                                (p.muted, crate::labels::count(direct.len(), "anterior", "anteriores"))
                            };
                            let detail = direct
                                .iter()
                                .map(|d| format!("{} {}", state_mark(d).0, d.title))
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = S1;
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{} {text}", icon::LINK))
                                            .size(theme::SIZE_SMALL)
                                            .color(color),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text(if blocked {
                                    format!("Só pode ser concluída depois de:\n{detail}")
                                } else {
                                    format!("Depende de:\n{detail}")
                                });
                                widgets::small_muted(ui, "depende de");
                                for dependency in &direct {
                                    if let Some(clicked) = dependency_chip(ui, dependency) {
                                        if env.selecting {
                                            out.toggle = Some(task.id);
                                        } else {
                                            out.select = Some(clicked);
                                        }
                                    }
                                }
                            });
                        }
                        if !releases.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = S1;
                                widgets::small_muted(ui, "libera a conclusão de")
                                    .on_hover_text("Tarefas que só podem ser concluídas depois desta.");
                                for next in &releases {
                                    if let Some(clicked) = dependency_chip(ui, next) {
                                        if env.selecting {
                                            out.toggle = Some(task.id);
                                        } else {
                                            out.select = Some(clicked);
                                        }
                                    }
                                }
                            });
                        }
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = S2;
                    if !pending {
                        widgets::pill(ui, "Concluída", Tone::Ok);
                    } else if task.due_date.is_empty() {
                        widgets::pill(ui, "Sem prazo", Tone::Neutral);
                    } else if task.overdue() {
                        widgets::pill(ui, format!("Atrasada · {}", short_date(&task.due_date)), Tone::Danger);
                    } else if task.due_date == env.today {
                        widgets::pill(ui, "Hoje", Tone::Warn);
                    } else {
                        widgets::pill(ui, short_date(&task.due_date), Tone::Info);
                    }
                    if task.priority != "normal" && !task.priority.is_empty() {
                        let tone = match task.priority.as_str() {
                            "urgente" => Tone::Danger,
                            "alta" => Tone::Warn,
                            _ => Tone::Neutral,
                        };
                        widgets::pill(ui, crate::labels::priority(&task.priority), tone);
                    }
                });
            });
        });

    let rect = inner.response.rect;
    ui.data_mut(|d| d.insert_temp(id, rect));
    let r = theme::RADIUS_CARD.saturating_sub(1);
    let rounding = egui::CornerRadius {
        nw: if first { r } else { 0 },
        ne: if first { r } else { 0 },
        sw: if last { r } else { 0 },
        se: if last { r } else { 0 },
    };
    let hovered = row.as_ref().is_some_and(|r| r.hovered());
    let fill = if editing_this || is_selected {
        Some(p.primary_soft)
    } else if hovered {
        Some(p.sunken)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter()
            .set(background, egui::Shape::rect_filled(rect, rounding, fill));
    }
    // Quiet dependency line and the mark of the tasks around the open one.
    if level > 0 {
        let x = rect.left() + S3 + indent - 9.0;
        ui.painter().vline(
            x,
            rect.y_range().shrink(6.0),
            egui::Stroke::new(2.0, p.primary.gamma_multiply(0.5)),
        );
    }
    if let Some((color, hint)) = relation {
        let bar = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + 3.0, rect.bottom()),
        );
        ui.painter().rect_filled(bar, 0.0, color);
        if let Some(row) = &row {
            row.clone().on_hover_text(hint);
        }
    }
    if !last {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            egui::Stroke::new(1.0, p.outline),
        );
    }
    // The click only counts when the area it landed on is where this row
    // still is: after the list changes, last frame's area may belong to
    // another row's place.
    let in_place = cached.is_some_and(|old| (old.min - rect.min).length() < 1.0);
    if let Some(row) = row {
        if in_place && row.clicked() {
            if env.selecting {
                out.toggle = Some(task.id);
            } else {
                out.select = Some(task.clone());
            }
        }
    }
}

/// Actions on several tasks at once: what is changed, which tasks are
/// skipped and why, how it is undone and how the result reads. Every task
/// goes through the same store calls as a single change (fresh read, version
/// check, dependency rules); nothing here is a shortcut around them.
mod batch {
    use super::*;
    use std::collections::BTreeMap;

    /// Choices of the priority list, as the task form has them.
    pub const PRIORITIES: [&str; 4] = ["baixa", "normal", "alta", "urgente"];
    /// How long "Desfazer" stays on the result line.
    pub const UNDO_SECONDS: u64 = 30;

    /// What is asked of every selected task.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Change {
        Conclude,
        /// New deadline in ISO form; empty means "Sem prazo".
        Due(String),
        /// New assignee; empty means "sem responsável".
        Assignee(String),
        Priority(String),
        Trash,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Kind {
        Conclude,
        Due,
        Assignee,
        Priority,
        Trash,
        Undo,
    }

    impl Change {
        pub fn kind(&self) -> Kind {
            match self {
                Self::Conclude => Kind::Conclude,
                Self::Due(_) => Kind::Due,
                Self::Assignee(_) => Kind::Assignee,
                Self::Priority(_) => Kind::Priority,
                Self::Trash => Kind::Trash,
            }
        }
    }

    /// Why a task was left as it was.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Reason {
        AlreadyDone,
        Blocked,
        Missing,
        Changed,
        Same,
        Refused(String),
    }

    /// The fields an action touched, which is all an undo puts back.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Touched {
        Status,
        Due,
        Assignee,
        Priority,
        /// A task changed as a side effect (the cascade of a conclusion):
        /// the whole previous state is put back.
        Whole,
        /// Moved to the Lixeira: undone by restoring it.
        Untrash,
    }

    #[derive(Clone, Debug)]
    pub struct UndoItem {
        pub id: i64,
        pub before: Task,
        pub touched: Touched,
        /// Version of the task right after the batch saved it.
        pub version: i64,
    }

    pub struct Skip {
        pub id: i64,
        pub title: String,
        pub reason: Reason,
    }

    #[derive(Default)]
    pub struct Outcome {
        pub done: usize,
        pub skipped: Vec<Skip>,
        pub undo: Vec<UndoItem>,
        pub notes: Vec<String>,
    }

    /// Version and title of each selected task when the person was asked to
    /// confirm: a task that is not at this version when applying is left alone.
    pub type Snapshot = BTreeMap<i64, (i64, String)>;

    /// Reads the version and title of every selected task now.
    pub fn snapshot(store: &Store, selected: &BTreeSet<i64>, tasks: &[Task]) -> Snapshot {
        selected
            .iter()
            .map(|&id| {
                let title = tasks
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                (id, (version_of(store, id), title))
            })
            .collect()
    }

    /// Keeps only the selected ids that are still listed; true when some
    /// were dropped.
    pub fn retain_visible(selected: &mut BTreeSet<i64>, visible: &[i64]) -> bool {
        let before = selected.len();
        selected.retain(|id| visible.contains(id));
        selected.len() != before
    }

    /// Whether a task may take the change. `blocked`: a task it depends on is
    /// still open.
    pub fn eligibility(change: &Change, task: &Task, blocked: bool) -> Result<(), Reason> {
        match change {
            Change::Conclude if task.status == "concluida" => Err(Reason::AlreadyDone),
            Change::Conclude if blocked => Err(Reason::Blocked),
            Change::Due(due) if task.due_date == *due => Err(Reason::Same),
            Change::Assignee(name) if task.assignee.trim() == name.trim() => Err(Reason::Same),
            Change::Priority(priority) if task.priority == *priority => Err(Reason::Same),
            _ => Ok(()),
        }
    }

    /// Applies the change to the fields it touches.
    fn change_task(change: &Change, task: &mut Task) {
        match change {
            Change::Conclude => {
                task.status = "concluida".into();
                task.completed_at = None;
            }
            Change::Due(due) => {
                task.due_date = due.clone();
                // The origin quoted for the old date does not support the new one.
                task.due_source.clear();
            }
            Change::Assignee(name) => task.assignee = name.trim().to_owned(),
            Change::Priority(priority) => task.priority = priority.clone(),
            Change::Trash => {}
        }
    }

    fn touched(change: &Change) -> Touched {
        match change {
            Change::Conclude => Touched::Status,
            Change::Due(_) => Touched::Due,
            Change::Assignee(_) => Touched::Assignee,
            Change::Priority(_) => Touched::Priority,
            Change::Trash => Touched::Untrash,
        }
    }

    /// Puts back what an action touched, leaving the rest of the task as it is now.
    fn restore_fields(touched: Touched, from: &Task, to: &mut Task) {
        match touched {
            Touched::Status => {
                to.status = from.status.clone();
                to.completed_at = from.completed_at.clone();
            }
            Touched::Due => {
                to.due_date = from.due_date.clone();
                to.due_source = from.due_source.clone();
            }
            Touched::Assignee => to.assignee = from.assignee.clone(),
            Touched::Priority => to.priority = from.priority.clone(),
            Touched::Whole => *to = from.clone(),
            Touched::Untrash => {}
        }
    }

    /// Sorts a store refusal into a reason the result can explain.
    fn classify(error: &str) -> Reason {
        if error.contains("Conflito de edição") {
            Reason::Changed
        } else if error.contains("Conclua as tarefas anteriores") {
            Reason::Blocked
        } else {
            Reason::Refused(error.trim().to_owned())
        }
    }

    fn version_of(store: &Store, id: i64) -> i64 {
        store.item_version("task", id).map_or(-1, |v| v.version)
    }

    /// One task of the batch: `task` is as read at the start of the pass and
    /// `expected` the version the person confirmed. The version is compared
    /// right before the write (the save itself checks it again), so a write
    /// that landed since, from anywhere, is a refusal and never adopted.
    pub fn apply_one(
        store: &mut Store,
        task: &Task,
        expected: i64,
        blocked: bool,
        change: &Change,
    ) -> Result<UndoItem, Reason> {
        eligibility(change, task, blocked)?;
        let id = task.id;
        if *change == Change::Trash {
            let target = crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id,
            };
            let plan = store
                .trash_plan(target.clone())
                .map_err(|e| Reason::Refused(e.to_string()))?;
            if let Some(blocker) = plan.blocker {
                return Err(Reason::Refused(blocker));
            }
            // `trash_item` takes no version: compare it here, just before.
            if version_of(store, id) != expected {
                return Err(Reason::Changed);
            }
            store
                .trash_item(target)
                .map_err(|e| Reason::Refused(e.to_string()))?;
            return Ok(UndoItem {
                id,
                before: task.clone(),
                touched: Touched::Untrash,
                version: 0,
            });
        }
        let mut changed = task.clone();
        change_task(change, &mut changed);
        store
            .save_task_checked(&changed, Some(expected))
            .map_err(|e| classify(&e.to_string()))?;
        Ok(UndoItem {
            id,
            before: task.clone(),
            touched: touched(change),
            version: version_of(store, id),
        })
    }

    /// Applies `change` to the tasks of `snapshot`. A task whose version is no
    /// longer the one confirmed is skipped, in every pass; one refused for a
    /// reason that a later task of the batch may lift (a dependency to
    /// conclude, a dependent to delete first) is tried again while the batch
    /// progresses. The task list is read once per pass and kept up to date
    /// with the batch's own writes; it is read again only when concluding a
    /// task may have concluded selected tasks of other documents by cascade.
    pub fn apply_batch(store: &mut Store, snapshot: &Snapshot, change: &Change) -> Outcome {
        let mut out = Outcome::default();
        let title_of = |id: i64| {
            snapshot
                .get(&id)
                .map(|(_, title)| title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "tarefa sem título".to_owned())
        };
        let before_all: BTreeMap<i64, Task> = store
            .list_tasks()
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.id, t))
            .collect();
        let mut list: Vec<Task> = before_all.values().cloned().collect();
        let mut pending = Vec::new();
        for (&id, (version, _)) in snapshot {
            if !before_all.contains_key(&id) {
                out.skipped.push(Skip {
                    id,
                    title: title_of(id),
                    reason: Reason::Missing,
                });
            } else if version_of(store, id) != *version {
                out.skipped.push(Skip {
                    id,
                    title: title_of(id),
                    reason: Reason::Changed,
                });
            } else {
                pending.push(id);
            }
        }
        // Selected tasks concluded by the cascade of this very batch.
        let mut cascaded: BTreeSet<i64> = BTreeSet::new();
        let mut first_pass = true;
        loop {
            if !first_pass {
                list = store.list_tasks().unwrap_or_default();
            }
            first_pass = false;
            let mut progress = false;
            let mut retry = Vec::new();
            for id in std::mem::take(&mut pending) {
                if cascaded.contains(&id) {
                    continue;
                }
                let result = match list.iter().find(|t| t.id == id) {
                    None => Err(Reason::Missing),
                    Some(task) => {
                        let blocked = crate::tasks::Dependencies::new(&list).is_blocked(id);
                        apply_one(store, task, snapshot[&id].0, blocked, change)
                    }
                };
                match result {
                    Ok(item) => {
                        progress = true;
                        out.done += 1;
                        let document = item.before.document_id;
                        out.undo.push(item);
                        // Keep the list as the store now is.
                        if *change == Change::Trash {
                            list.retain(|t| t.id != id);
                        } else if let Some(task) = list.iter_mut().find(|t| t.id == id) {
                            change_task(change, task);
                        }
                        // The last open task of a document concluded: the
                        // pieces joined to it may have been concluded too.
                        let unsettled = |other: &i64| {
                            !out.undo.iter().any(|u| u.id == *other)
                                && !out.skipped.iter().any(|s| s.id == *other)
                        };
                        if *change == Change::Conclude
                            && document.is_some()
                            && !list
                                .iter()
                                .any(|t| t.document_id == document && t.status != "concluida")
                            && snapshot.keys().filter(|o| unsettled(o)).any(|o| {
                                list.iter()
                                    .find(|t| t.id == *o)
                                    .is_some_and(|t| t.document_id != document)
                            })
                        {
                            let fresh = store.list_tasks().unwrap_or_default();
                            let found: Vec<i64> = snapshot
                                .keys()
                                .copied()
                                .filter(|o| unsettled(o))
                                .filter(|o| {
                                    let now = fresh.iter().find(|t| t.id == *o);
                                    let was = list.iter().find(|t| t.id == *o);
                                    matches!((now, was), (Some(n), Some(w))
                                        if n.status == "concluida" && w.status != "concluida")
                                })
                                .collect();
                            for other in found {
                                if let Some(before) = before_all.get(&other) {
                                    out.done += 1;
                                    out.undo.push(UndoItem {
                                        id: other,
                                        before: before.clone(),
                                        touched: Touched::Whole,
                                        version: version_of(store, other),
                                    });
                                    cascaded.insert(other);
                                }
                            }
                            list = fresh;
                        }
                    }
                    Err(reason) => {
                        let again = matches!(
                            (&reason, change),
                            (Reason::Blocked, Change::Conclude)
                                | (Reason::Refused(_), Change::Trash)
                        );
                        if again {
                            retry.push((id, reason));
                        } else {
                            out.skipped.push(Skip {
                                id,
                                title: title_of(id),
                                reason,
                            });
                        }
                    }
                }
            }
            retry.retain(|(id, _)| !cascaded.contains(id));
            if retry.is_empty() || !progress {
                for (id, reason) in retry {
                    out.skipped.push(Skip {
                        id,
                        title: title_of(id),
                        reason,
                    });
                }
                break;
            }
            pending = retry.into_iter().map(|(id, _)| id).collect();
        }
        // Other tasks concluded as a side effect (cascade) are part of what
        // the undo puts back: compared once, after the last write.
        if *change == Change::Conclude {
            for task in store.list_tasks().unwrap_or_default() {
                if out.undo.iter().any(|u| u.id == task.id) {
                    continue;
                }
                if let Some(before) = before_all.get(&task.id) {
                    if before.status != task.status {
                        out.undo.push(UndoItem {
                            id: task.id,
                            before: before.clone(),
                            touched: Touched::Whole,
                            version: version_of(store, task.id),
                        });
                    }
                }
            }
        }
        out.notes = store.take_cascade_notes();
        out
    }

    /// Puts back what the batch changed, task by task. One changed again
    /// since (its version is not the one the batch left) is skipped; the
    /// version is compared right before each write. The task list is read
    /// once.
    pub fn undo_batch(store: &mut Store, items: &[UndoItem]) -> Outcome {
        let mut out = Outcome::default();
        let list = store.list_tasks().unwrap_or_default();
        for item in items.iter().rev() {
            let skip = |reason| Skip {
                id: item.id,
                title: item.before.title.clone(),
                reason,
            };
            if item.touched == Touched::Untrash {
                let target = crate::trash::Target {
                    kind: crate::trash::Kind::Task,
                    id: item.id,
                };
                match store.restore_item(target) {
                    Ok(_) => out.done += 1,
                    Err(e) => out.skipped.push(skip(Reason::Refused(e.to_string()))),
                }
                continue;
            }
            if version_of(store, item.id) != item.version {
                out.skipped.push(skip(Reason::Changed));
                continue;
            }
            let Some(mut task) = list.iter().find(|t| t.id == item.id).cloned() else {
                out.skipped.push(skip(Reason::Missing));
                continue;
            };
            restore_fields(item.touched, &item.before, &mut task);
            match store.save_task_checked(&task, Some(item.version)) {
                Ok(_) => out.done += 1,
                Err(e) => out.skipped.push(skip(classify(&e.to_string()))),
            }
        }
        out.notes = store.take_cascade_notes();
        out
    }

    fn plural(n: usize, one: &str, many: &str) -> String {
        crate::labels::count(n, one, many)
    }

    /// "«A», «B» e «C»" for up to three titles, then "«A», «B», «C» e mais N".
    fn title_list(titles: &[&str]) -> String {
        let quoted: Vec<String> = titles.iter().map(|t| format!("«{t}»")).collect();
        match quoted.len() {
            0 => String::new(),
            1 => quoted[0].clone(),
            2 | 3 => format!(
                "{} e {}",
                quoted[..quoted.len() - 1].join(", "),
                quoted[quoted.len() - 1]
            ),
            n => format!("{} e mais {}", quoted[..3].join(", "), n - 3),
        }
    }

    fn fail_phrase(kind: Kind, k: usize) -> &'static str {
        match (kind, k == 1) {
            (Kind::Conclude, true) => "não pôde ser concluída",
            (Kind::Conclude, false) => "não puderam ser concluídas",
            (Kind::Trash, true) => "não pôde ser excluída",
            (Kind::Trash, false) => "não puderam ser excluídas",
            (Kind::Undo, true) => "não pôde ser desfeita",
            (Kind::Undo, false) => "não puderam ser desfeitas",
            (_, true) => "não foi alterada",
            (_, false) => "não foram alteradas",
        }
    }

    fn reason_text(reason: &Reason, kind: Kind, k: usize) -> String {
        let one = k == 1;
        let text = match reason {
            Reason::AlreadyDone => {
                if one {
                    "já está concluída"
                } else {
                    "já estão concluídas"
                }
            }
            Reason::Blocked => {
                if one {
                    "depende de uma tarefa em aberto"
                } else {
                    "dependem de tarefas em aberto"
                }
            }
            Reason::Missing => {
                if one {
                    "está na Lixeira ou não existe mais"
                } else {
                    "estão na Lixeira ou não existem mais"
                }
            }
            Reason::Changed if kind == Kind::Undo => {
                if one {
                    "foi alterada depois da ação em lote"
                } else {
                    "foram alteradas depois da ação em lote"
                }
            }
            Reason::Changed => {
                if one {
                    "foi alterada depois da seleção"
                } else {
                    "foram alteradas depois da seleção"
                }
            }
            Reason::Same => match (kind, one) {
                (Kind::Due, true) => "já tem esse prazo",
                (Kind::Due, false) => "já têm esse prazo",
                (Kind::Assignee, true) => "já tem esse responsável",
                (Kind::Assignee, false) => "já têm esse responsável",
                (_, true) => "já tem essa prioridade",
                (_, false) => "já têm essa prioridade",
            },
            Reason::Refused(message) => {
                return format!("— {}", message.trim().trim_end_matches('.'));
            }
        };
        text.to_owned()
    }

    /// The sentence that says what was done to `n` tasks.
    pub fn done_sentence(change: &Change, n: usize) -> String {
        match change {
            Change::Conclude => plural(n, "tarefa concluída", "tarefas concluídas"),
            Change::Due(due) if due.is_empty() => {
                plural(n, "tarefa ficou sem prazo", "tarefas ficaram sem prazo")
            }
            Change::Due(due) => format!(
                "{} {}",
                plural(
                    n,
                    "tarefa com o prazo mudado para",
                    "tarefas com o prazo mudado para"
                ),
                crate::labels::date(due)
            ),
            Change::Assignee(name) if name.trim().is_empty() => plural(
                n,
                "tarefa ficou sem responsável",
                "tarefas ficaram sem responsável",
            ),
            Change::Assignee(name) => format!(
                "{} {}",
                plural(n, "tarefa atribuída a", "tarefas atribuídas a"),
                name.trim()
            ),
            Change::Priority(priority) => format!(
                "{} {}",
                plural(n, "tarefa com prioridade", "tarefas com prioridade"),
                crate::labels::priority(priority)
            ),
            Change::Trash => plural(n, "tarefa excluída", "tarefas excluídas"),
        }
    }

    /// The text of the result line: what was done, then one line per reason
    /// for the tasks left as they were, and the notes of the store. `change`
    /// is `None` for an undo; `undone_trash` says it restored deleted tasks.
    pub fn message(change: Option<&Change>, undone_trash: bool, out: &Outcome) -> String {
        let kind = change.map_or(Kind::Undo, Change::kind);
        let mut head = match (change, out.done) {
            (Some(_), 0) => "Nada foi alterado.".to_owned(),
            (None, 0) => "Nada foi desfeito.".to_owned(),
            (Some(change), n) => format!("{}.", done_sentence(change, n)),
            (None, n) if undone_trash => format!(
                "{} da Lixeira.",
                plural(n, "tarefa restaurada", "tarefas restauradas")
            ),
            (None, n) => format!("Alteração desfeita em {}.", plural(n, "tarefa", "tarefas")),
        };
        if out.done > 0 && matches!(change, Some(Change::Trash)) {
            head.push_str(if out.done == 1 {
                " Ela está na Lixeira (Ajustes ▸ Lixeira)."
            } else {
                " Estão na Lixeira (Ajustes ▸ Lixeira)."
            });
        }
        let mut lines = vec![head];
        let mut groups: Vec<(&Reason, Vec<&str>)> = Vec::new();
        for skip in &out.skipped {
            match groups
                .iter_mut()
                .find(|(reason, _)| **reason == skip.reason)
            {
                Some((_, titles)) => titles.push(&skip.title),
                None => groups.push((&skip.reason, vec![&skip.title])),
            }
        }
        for (reason, titles) in groups {
            lines.push(format!(
                "{} {}: {} {}.",
                titles.len(),
                fail_phrase(kind, titles.len()),
                title_list(&titles),
                reason_text(reason, kind, titles.len())
            ));
        }
        for note in &out.notes {
            lines.push(format!("• {note}"));
        }
        lines.join("\n")
    }
}

/// Something the person asked for while another task edit was still unsaved.
/// The panel never honours it by itself: the app asks what to do with the
/// pending edit first and then calls `focus_task` / `start_new`.
// One intent at a time is held while the app asks what to do with an
// unsaved edit; its size does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// Open this task in the form.
    Open(Task),
    /// Start a blank task for this document, or without document (`None`).
    New(Option<i64>),
}

/// Which action of the bar is waiting for its confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AskKind {
    Conclude,
    Due,
    Assignee,
    Priority,
    Trash,
}

/// A confirmation open in the action bar.
struct Ask {
    kind: AskKind,
    snapshot: batch::Snapshot,
}

/// What the person chose in the action bar during this frame.
enum BarAction {
    /// Open the confirmation of an action.
    Ask(AskKind),
    /// Confirmed: apply this change to the tasks of the open confirmation.
    Apply(batch::Change),
    Cancel,
    Undo,
    CloseResult,
}

/// What puts the last batch back, for a short while.
struct BatchUndo {
    at: std::time::Instant,
    items: Vec<batch::UndoItem>,
    trash: bool,
}

#[derive(Default)]
pub struct TaskPanel {
    draft: Option<Task>,
    original: Option<Task>,
    base_version: Option<i64>,
    show_done: bool,
    query: String,
    message: String,
    message_is_error: bool,
    view: usize,
    /// The list is narrowed to this document only when the person asked for
    /// it; the area otherwise opens with every task.
    document_filter: Option<i64>,
    /// Order of the list inside each group (index of `SORTS`).
    sort: usize,
    /// What the store returned at the last frame (its reads are memoized).
    data: Loaded,
    load_error: Option<String>,
    /// Give the keyboard focus to the search box / the title at the next frame.
    focus_search: bool,
    focus_title: bool,
    /// The task as it was before the last conclusion, until the app takes it.
    concluded: Option<Task>,
    trash_requested: Option<crate::trash::Target>,
    /// "Anexos" block of the task being edited.
    attachments: crate::attachments::AttachmentsPanel,
    /// Action refused because of the unsaved edit; the app resolves it.
    blocked: Option<Intent>,
    /// Document the person wants to read without leaving the form.
    consult_requested: Option<i64>,
    /// A use of a resource to open, or a new one to start, in Recursos.
    use_request: Option<crate::resources_ui::UseRequest>,
    /// "Abrir" clicked in the dependency chain of the form.
    open_task_requested: Option<Task>,
    /// The form shows the document search (to choose the document of a new
    /// task or to move the open one).
    picking: bool,
    doc_picker: crate::doc_picker::DocumentPicker,
    /// "Desvincular do documento" waits for its confirmation.
    confirm_unlink: bool,
    /// Selection mode ("Selecionar"): tasks chosen by id for an action on
    /// several at once.
    selecting: bool,
    selected: BTreeSet<i64>,
    /// The confirmation open in the action bar, with the versions the tasks
    /// had when it opened.
    batch_ask: Option<Ask>,
    /// What the person typed or chose in the confirmation.
    batch_due: String,
    batch_due_clear: bool,
    batch_assignee: String,
    batch_assignee_clear: bool,
    batch_priority: String,
    /// Result line of the last batch, and what undoes it until it expires.
    batch_result: Option<(String, Tone)>,
    batch_undo: Option<BatchUndo>,
    /// A batch or its undo changed tasks (the app may refresh other areas).
    batch_changed: bool,
    /// A background job runs (set by the app): deleting waits.
    busy: bool,
    /// Development only: keeps "Mais detalhes" open for the picture.
    #[cfg(feature = "shots")]
    shots_details: bool,
    /// Development only: scroll of the list, to picture its lower groups.
    #[cfg(feature = "shots")]
    shots_scroll: f32,
}
impl TaskPanel {
    pub fn take_trash_request(&mut self) -> Option<crate::trash::Target> {
        self.trash_requested
            .take()
            .or_else(|| self.attachments.take_trash_request())
    }
    /// What the person tried to do while the current edit was unsaved.
    pub fn take_blocked(&mut self) -> Option<Intent> {
        self.blocked.take()
    }
    /// Document to show in the read-only consultation window.
    pub fn take_consult_request(&mut self) -> Option<i64> {
        self.consult_requested.take()
    }
    /// What the person asked about the uses of resources in the form
    /// ("Abrir" a use, "Reservar recurso…"), for the app to serve.
    pub fn take_use_request(&mut self) -> Option<crate::resources_ui::UseRequest> {
        self.use_request.take()
    }
    /// Title of the unsaved task, for the "leave the edit?" question.
    pub fn editing_title(&self) -> Option<String> {
        if !self.dirty() {
            return None;
        }
        let draft = self.draft.as_ref()?;
        let title = draft.title.trim();
        Some(if title.is_empty() {
            if draft.id > 0 {
                format!("tarefa #{}", draft.id)
            } else {
                "nova tarefa (sem título)".to_owned()
            }
        } else {
            title.to_owned()
        })
    }
    /// Saves the task being edited. On failure the reason is returned and
    /// also shown in the panel; the edit stays as it was.
    pub fn save_draft(&mut self, store: &mut Store) -> Result<(), String> {
        let Some(task) = &self.draft else {
            return Ok(());
        };
        match store.save_task_checked(task, self.base_version) {
            Ok(id) => {
                self.draft = store
                    .list_tasks()
                    .ok()
                    .and_then(|tasks| tasks.into_iter().find(|t| t.id == id));
                self.original = self.draft.clone();
                self.base_version = store.item_version("task", id).ok().map(|v| v.version);
                // Another change: the last batch can no longer be undone.
                self.batch_undo = None;
                let mut text = "Tarefa salva.".to_owned();
                for note in store.take_cascade_notes() {
                    text.push_str(" • ");
                    text.push_str(&note);
                }
                self.notice(text, false);
                Ok(())
            }
            Err(e) => {
                let text = format!("Não foi possível salvar a tarefa: {e}");
                self.notice(text.clone(), true);
                Err(text)
            }
        }
    }
    /// Drops the unsaved changes: an existing task goes back to what is
    /// saved, a new one disappears.
    pub fn discard_draft(&mut self) {
        self.draft = self.original.clone();
        self.message.clear();
        if self.draft.is_none() {
            self.base_version = None;
            self.attachments.forget();
        }
    }
    /// PDF attachment to open in the Ponte reader: (attachment id, label).
    pub fn take_attachment_pdf_request(&mut self) -> Option<(i64, String)> {
        self.attachments.take_pdf_request()
    }
    /// True while an attachment description of the open task is not saved.
    pub fn attachments_dirty(&self) -> bool {
        self.attachments.dirty()
    }
    pub fn document_dirty(&self, id: i64) -> bool {
        self.dirty()
            && self
                .draft
                .as_ref()
                .is_some_and(|task| task.document_id == Some(id))
    }
    pub fn task_dirty(&self, id: i64) -> bool {
        self.dirty() && self.draft.as_ref().is_some_and(|task| task.id == id)
    }
    pub fn forget_task(&mut self, id: i64) {
        if self.draft.as_ref().is_some_and(|task| task.id == id) {
            self.draft = None;
            self.original = None;
            self.base_version = None;
            self.message.clear();
            self.attachments.forget();
        }
    }

    /// Narrows the list to one document (asked for by the person, e.g. the
    /// ficha's "Ver todas").
    pub fn focus_document(&mut self, id: i64) {
        self.document_filter = Some(id);
        self.view = 0;
        self.query.clear();
    }
    /// Shows the tasks of every document again.
    pub fn clear_document_filter(&mut self) {
        self.document_filter = None;
    }
    /// Document the list is narrowed to, if any.
    pub fn document_filter(&self) -> Option<i64> {
        self.document_filter
    }
    /// Keeps a document filter only when it already covers this document, so
    /// the task about to open stays in the list.
    fn keep_filter_for(&mut self, document_id: Option<i64>) {
        if self
            .document_filter
            .is_some_and(|id| Some(id) != document_id)
        {
            self.document_filter = None;
        }
        self.view = 0;
        self.query.clear();
    }
    /// Opens a blank task for this document (`None`: a task without document),
    /// unless another edit is pending.
    pub fn start_new(&mut self, document_id: Option<i64>) -> bool {
        if self.dirty() {
            return false;
        }
        self.end_selection();
        self.keep_filter_for(document_id);
        self.draft = Some(Task::blank(document_id));
        self.original = None;
        self.base_version = None;
        self.message.clear();
        self.focus_title = true;
        true
    }
    pub fn focus_task(&mut self, task: &Task) -> bool {
        if self.dirty() {
            return self
                .draft
                .as_ref()
                .is_some_and(|draft| draft.id == task.id && draft.document_id == task.document_id);
        }
        self.end_selection();
        self.keep_filter_for(task.document_id);
        self.draft = Some(task.clone());
        self.original = Some(task.clone());
        self.base_version = None;
        self.focus_title = true;
        true
    }
    /// Esc: closes the task panel when nothing would be lost. True when it
    /// did something or when the panel is open but cannot be closed now.
    pub fn key_escape(&mut self) -> bool {
        if self.draft.is_none() {
            // Selection mode: Esc first closes the confirmation, then leaves.
            if self.batch_ask.take().is_some() {
                return true;
            }
            if self.selecting {
                self.end_selection();
                return true;
            }
            return false;
        }
        if !self.dirty() && !self.attachments.dirty() {
            self.close_form();
        }
        true
    }
    /// Ctrl+S: what "Salvar" does, when it is available. True when a save
    /// was attempted.
    pub fn key_save(&mut self, store: &mut Store) -> bool {
        let Some(task) = &self.draft else {
            return false;
        };
        if !(self.dirty() || task.id == 0) {
            return false;
        }
        let _ = self.save_draft(store);
        true
    }
    /// Ctrl+F: the search box takes the keyboard focus at the next frame.
    pub fn key_find(&mut self) {
        self.focus_search = true;
    }
    /// Ctrl+N: what "Nova tarefa" does: a task for the document the list is
    /// narrowed to (the band says which), otherwise without document. The
    /// document last opened in Documentos is not on this screen, so it does
    /// not decide; the form offers "Escolher documento…".
    pub fn key_new(&mut self, _selected_document: Option<i64>) {
        let target = self.document_filter;
        if !self.start_new(target) {
            self.blocked = Some(Intent::New(target));
        }
    }
    /// The task as it was before the last conclusion (returned once), so the
    /// app can offer "Desfazer".
    pub fn take_concluded(&mut self) -> Option<Task> {
        self.concluded.take()
    }
    /// Reopens a task just concluded, the way "Reabrir" does.
    pub fn undo_conclude(&mut self, store: &mut Store, task_id: i64) -> Result<(), String> {
        self.apply_status_change(store, task_id, false)
    }
    /// The task in the panel; a new, unsaved one has no id.
    pub fn open_task_id(&self) -> Option<i64> {
        self.draft.as_ref().map(|t| t.id).filter(|id| *id > 0)
    }

    /// Opens a task from the list, keeping the current view and filter.
    /// Refused (false) while a different task has unsaved changes.
    pub fn open_task(&mut self, task: &Task, store: &Store) -> bool {
        if self.draft.as_ref().is_some_and(|t| t.id == task.id) {
            return true;
        }
        if self.dirty() {
            return false;
        }
        self.end_selection();
        self.original = Some(task.clone());
        self.draft = Some(task.clone());
        self.base_version = store.item_version("task", task.id).ok().map(|v| v.version);
        self.message.clear();
        self.focus_title = true;
        true
    }

    pub fn forget_document(&mut self, id: i64) {
        if self.document_filter == Some(id) {
            self.document_filter = None;
        }
        if self
            .draft
            .as_ref()
            .is_some_and(|t| t.document_id == Some(id))
        {
            self.draft = None;
            self.original = None;
            self.base_version = None;
            self.message.clear();
        }
    }
    pub fn prepare_suggestion(
        &mut self,
        document_id: i64,
        suggestion: &crate::model::ActionSuggestion,
        priority: &str,
    ) -> bool {
        if self.dirty() {
            return false;
        }
        let mut task = Task::new(document_id);
        task.title = format!("{} — revisar providência", suggestion.action);
        task.action = suggestion.action.clone();
        task.priority = priority.into();
        task.notes = format!(
            "Sugestão da IA para revisão.\nMotivo: {}\nBase informada: {}",
            suggestion.reason, suggestion.evidence
        );
        self.end_selection();
        self.draft = Some(task);
        self.original = None;
        self.focus_title = true;
        self.notice(
            "Revise e salve para criar a tarefa. Nada foi enviado ao SIGADOC.",
            false,
        );
        true
    }
    /// True while the form holds something that would be lost. A new task
    /// nobody has touched yet (still exactly what "Nova tarefa" proposes)
    /// has nothing to lose and does not count.
    pub fn dirty(&self) -> bool {
        match (&self.draft, &self.original) {
            (Some(draft), None) => *draft != Task::blank(draft.document_id),
            (draft, original) => draft != original,
        }
    }

    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.message = text.into();
        self.message_is_error = error;
    }

    /// True when the panel may enter selection mode: nothing unsaved is
    /// open and no request is waiting for the app's question about it.
    fn can_select(&self) -> bool {
        !self.dirty() && !self.attachments.dirty() && self.blocked.is_none()
    }

    /// "Selecionar": closes a task form nobody has changed and turns the
    /// selection mode on. Refused (false) while something would be lost.
    fn start_selection(&mut self) -> bool {
        if !self.can_select() {
            return false;
        }
        if self.draft.is_some() {
            self.close_form();
        }
        self.selecting = true;
        self.selected.clear();
        self.reset_batch_inputs();
        true
    }

    /// Leaves selection mode: the selection and any open confirmation go.
    /// The result line of the last batch stays.
    fn end_selection(&mut self) {
        self.selecting = false;
        self.selected.clear();
        self.reset_batch_inputs();
    }

    /// Closes the confirmation of the action bar and forgets what was typed.
    fn reset_batch_inputs(&mut self) {
        self.batch_ask = None;
        self.batch_due.clear();
        self.batch_due_clear = false;
        self.batch_assignee.clear();
        self.batch_assignee_clear = false;
        self.batch_priority.clear();
    }

    /// Adds or takes out one task; a change of the selection closes the
    /// confirmation, which counted the previous one.
    fn toggle_selected(&mut self, id: i64) {
        if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
        self.reset_batch_inputs();
    }

    fn select_all(&mut self, visible: &[i64]) {
        self.selected = visible.iter().copied().collect();
        self.reset_batch_inputs();
    }

    /// True once after a batch or its undo changed tasks, so the app can
    /// refresh what it shows elsewhere (counts on the rail, the ficha).
    pub fn take_batch_changed(&mut self) -> bool {
        std::mem::take(&mut self.batch_changed)
    }

    /// The app says each frame whether a background job runs; deleting
    /// waits for it, as the single delete does.
    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    /// Applies the confirmed change to the tasks of the snapshot. Afterwards
    /// the selection keeps only the tasks that were left as they were.
    fn run_batch(&mut self, store: &mut Store, snapshot: &batch::Snapshot, change: batch::Change) {
        if self.busy && change == batch::Change::Trash {
            // The confirmation stays open until the job is over.
            self.batch_ask = Some(Ask {
                kind: AskKind::Trash,
                snapshot: snapshot.clone(),
            });
            return;
        }
        self.reset_batch_inputs();
        if self.dirty() {
            self.batch_result = Some((
                "Salve ou cancele as alterações da tarefa aberta antes de continuar.".to_owned(),
                Tone::Danger,
            ));
            return;
        }
        let outcome = batch::apply_batch(store, snapshot, &change);
        self.selected = outcome.skipped.iter().map(|s| s.id).collect();
        let tone = if outcome.done == 0 {
            Tone::Danger
        } else if outcome.skipped.is_empty() {
            Tone::Ok
        } else {
            Tone::Warn
        };
        self.batch_result = Some((batch::message(Some(&change), false, &outcome), tone));
        for item in &outcome.undo {
            if item.touched == batch::Touched::Untrash {
                self.forget_task(item.id);
            }
        }
        self.batch_changed |= outcome.done > 0;
        self.batch_undo = (!outcome.undo.is_empty()).then(|| BatchUndo {
            at: std::time::Instant::now(),
            items: outcome.undo,
            trash: change == batch::Change::Trash,
        });
    }

    /// "Desfazer" on the result line: puts back what the last batch changed.
    fn run_undo(&mut self, store: &mut Store) {
        let Some(undo) = self.batch_undo.as_ref() else {
            return;
        };
        if undo.at.elapsed().as_secs() >= batch::UNDO_SECONDS {
            self.batch_undo = None;
            return;
        }
        if self.dirty() || self.attachments.dirty() {
            // The undo stays (its time keeps running): nothing is lost silently.
            self.notice("Salve ou cancele a tarefa aberta antes de desfazer.", true);
            return;
        }
        let Some(undo) = self.batch_undo.take() else {
            return;
        };
        let outcome = batch::undo_batch(store, &undo.items);
        let tone = if outcome.done == 0 {
            Tone::Danger
        } else if outcome.skipped.is_empty() {
            Tone::Ok
        } else {
            Tone::Warn
        };
        self.batch_result = Some((batch::message(None, undo.trash, &outcome), tone));
        self.batch_changed |= outcome.done > 0;
    }

    /// The strip under the filters in selection mode: how many are chosen
    /// and the ways to change that.
    fn selection_band(&mut self, ui: &mut egui::Ui, visible: &[i64]) {
        let count = self.selected.len();
        egui::Frame::new()
            .fill(c().sunken)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = S3;
                    ui.label(
                        RichText::new(if count == 0 {
                            "Nenhuma selecionada".to_owned()
                        } else {
                            crate::labels::count(count, "selecionada", "selecionadas")
                        })
                        .font(theme::font_strong(theme::SIZE_BODY)),
                    );
                    if ui
                        .add_enabled(
                            !visible.is_empty() && count < visible.len(),
                            widgets::quiet("Selecionar todas as visíveis"),
                        )
                        .on_hover_text("Seleciona as tarefas que estão na lista agora (Ctrl+A).")
                        .clicked()
                    {
                        self.select_all(visible);
                    }
                    if ui
                        .add_enabled(count > 0, widgets::quiet("Limpar seleção"))
                        .clicked()
                    {
                        self.selected.clear();
                        self.reset_batch_inputs();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(widgets::secondary("Sair da seleção"))
                            .on_hover_text("Volta à lista de sempre (Esc).")
                            .clicked()
                        {
                            self.end_selection();
                        }
                    });
                });
            });
    }

    /// The bar at the bottom: the result line of the last batch and, while
    /// something is selected, the actions or the confirmation of one.
    fn show_batch_bar(&mut self, ui: &mut egui::Ui, actor: &str) -> Option<BarAction> {
        let mut action = None;
        let count_text = |n: usize| crate::labels::count(n, "tarefa", "tarefas");
        egui::Panel::bottom("task-batch")
            .resizable(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                ui.add_space(S2);
                ui.spacing_mut().item_spacing.y = S2;
                if let Some((text, tone)) = self.batch_result.clone() {
                    let (fg, bg) = tone.colors();
                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                        .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let can_undo = self.batch_undo.is_some();
                                let reserved = 40.0 + if can_undo { 100.0 } else { 0.0 };
                                let width = (ui.available_width() - reserved).max(100.0);
                                ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                                    ui.set_width(width);
                                    ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                                });
                                if can_undo
                                    && ui
                                        .add(widgets::secondary(format!("{} Desfazer", icon::UNDO)))
                                        .on_hover_text("Volta estas tarefas ao que eram, se não tiverem sido alteradas depois. Vale por 30 segundos.")
                                        .clicked()
                                {
                                    action = Some(BarAction::Undo);
                                }
                                if widgets::icon_button(ui, icon::CLOSE, "Fechar").clicked() {
                                    action = Some(BarAction::CloseResult);
                                }
                            });
                        });
                }
                if !(self.selecting && !self.selected.is_empty()) {
                    return;
                }
                widgets::card_frame()
                    .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing = egui::vec2(S2, S2);
                        let Some(kind) = self.batch_ask.as_ref().map(|a| a.kind) else {
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add(widgets::primary(format!("{} Concluir", icon::CHECK)))
                                    .on_hover_text("Conclui as tarefas selecionadas. Nada é enviado ao SIGADOC.")
                                    .clicked()
                                {
                                    action = Some(BarAction::Ask(AskKind::Conclude));
                                }
                                for (label, kind, hint) in [
                                    ("Prazo…", AskKind::Due, "Muda o prazo de todas as selecionadas."),
                                    ("Responsável…", AskKind::Assignee, "Muda o responsável de todas as selecionadas."),
                                    ("Prioridade…", AskKind::Priority, "Muda a prioridade de todas as selecionadas."),
                                ] {
                                    if ui
                                        .add(widgets::secondary(label))
                                        .on_hover_text(hint)
                                        .clicked()
                                    {
                                        action = Some(BarAction::Ask(kind));
                                    }
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_enabled(!self.busy, widgets::danger(format!("{} Excluir…", icon::TRASH)))
                                            .on_hover_text("Manda as tarefas selecionadas para a Lixeira, de onde podem ser restauradas.")
                                            .on_disabled_hover_text("Aguarde a operação em andamento.")
                                            .clicked()
                                        {
                                            action = Some(BarAction::Ask(AskKind::Trash));
                                        }
                                    },
                                );
                            });
                            return;
                        };
                        let n = self.batch_ask.as_ref().map_or(0, |a| a.snapshot.len());
                        let tasks_text = count_text(n);
                        // The sentence of the confirmation, the change it
                        // stands for (None while the choice is incomplete) and
                        // the name of its button.
                        let (sentence, change, button): (String, Option<batch::Change>, &str) =
                            match kind {
                                AskKind::Conclude => (
                                    format!("Concluir {tasks_text}?"),
                                    Some(batch::Change::Conclude),
                                    "Concluir",
                                ),
                                AskKind::Trash => (
                                    format!(
                                        "Excluir {tasks_text}? {} para a Lixeira.",
                                        if n == 1 { "Ela vai" } else { "Elas vão" }
                                    ),
                                    Some(batch::Change::Trash),
                                    "Excluir",
                                ),
                                AskKind::Due => {
                                    ui.horizontal_wrapped(|ui| {
                                        let before = self.batch_due.clone();
                                        crate::labels::date_field(ui, "batch-due", &mut self.batch_due);
                                        if self.batch_due != before {
                                            self.batch_due_clear = false;
                                        }
                                        if widgets::chip(ui, self.batch_due_clear, "Sem prazo").clicked() {
                                            self.batch_due_clear = !self.batch_due_clear;
                                            if self.batch_due_clear {
                                                self.batch_due.clear();
                                            }
                                        }
                                    });
                                    let typed = crate::labels::parse_typed_date(&self.batch_due);
                                    match (self.batch_due_clear, typed) {
                                        (true, _) => (
                                            format!("Tirar o prazo de {tasks_text}?"),
                                            Some(batch::Change::Due(String::new())),
                                            "Tirar prazo",
                                        ),
                                        (false, Some(iso)) => (
                                            format!(
                                                "Mudar o prazo de {tasks_text} para {}?",
                                                crate::labels::date(&iso)
                                            ),
                                            Some(batch::Change::Due(iso)),
                                            "Mudar prazo",
                                        ),
                                        (false, None) => (
                                            format!("Escolha o novo prazo de {tasks_text}."),
                                            None,
                                            "Mudar prazo",
                                        ),
                                    }
                                }
                                AskKind::Assignee => {
                                    ui.horizontal_wrapped(|ui| {
                                        let before = self.batch_assignee.clone();
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.batch_assignee)
                                                .hint_text("Nome do responsável")
                                                .desired_width(220.0),
                                        );
                                        if self.batch_assignee != before {
                                            self.batch_assignee_clear = false;
                                        }
                                        if ui
                                            .add(widgets::secondary("Eu"))
                                            .on_hover_text(format!("Atribuir a {actor}"))
                                            .clicked()
                                        {
                                            self.batch_assignee = actor.to_owned();
                                            self.batch_assignee_clear = false;
                                        }
                                        if widgets::chip(ui, self.batch_assignee_clear, "Sem responsável").clicked() {
                                            self.batch_assignee_clear = !self.batch_assignee_clear;
                                            if self.batch_assignee_clear {
                                                self.batch_assignee.clear();
                                            }
                                        }
                                    });
                                    let name = self.batch_assignee.trim().to_owned();
                                    let valid = name.len() <= 256 && !name.chars().any(char::is_control);
                                    if self.batch_assignee_clear {
                                        (
                                            format!("Deixar {tasks_text} sem responsável?"),
                                            Some(batch::Change::Assignee(String::new())),
                                            "Tirar responsável",
                                        )
                                    } else if name.is_empty() {
                                        (
                                            format!("Digite o responsável de {tasks_text}."),
                                            None,
                                            "Atribuir",
                                        )
                                    } else if !valid {
                                        (
                                            "O nome do responsável é longo demais ou tem caracteres inválidos.".to_owned(),
                                            None,
                                            "Atribuir",
                                        )
                                    } else {
                                        (
                                            format!("Atribuir {tasks_text} a {name}?"),
                                            Some(batch::Change::Assignee(name)),
                                            "Atribuir",
                                        )
                                    }
                                }
                                AskKind::Priority => {
                                    ui.horizontal_wrapped(|ui| {
                                        for choice in batch::PRIORITIES {
                                            if widgets::chip(
                                                ui,
                                                self.batch_priority == choice,
                                                crate::labels::priority(choice),
                                            )
                                            .clicked()
                                            {
                                                self.batch_priority = choice.to_owned();
                                            }
                                        }
                                    });
                                    if self.batch_priority.is_empty() {
                                        (
                                            format!("Escolha a nova prioridade de {tasks_text}."),
                                            None,
                                            "Mudar prioridade",
                                        )
                                    } else {
                                        (
                                            format!(
                                                "Mudar a prioridade de {tasks_text} para {}?",
                                                crate::labels::priority(&self.batch_priority)
                                            ),
                                            Some(batch::Change::Priority(self.batch_priority.clone())),
                                            "Mudar prioridade",
                                        )
                                    }
                                }
                            };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(sentence).font(theme::font_strong(theme::SIZE_BODY)),
                            );
                            let ready = change.is_some();
                            let confirm = if kind == AskKind::Trash {
                                widgets::danger(button)
                            } else if kind == AskKind::Conclude {
                                widgets::primary(button)
                            } else {
                                widgets::secondary(button)
                            };
                            let waits = self.busy && kind == AskKind::Trash;
                            if ui
                                .add_enabled(ready && !waits, confirm)
                                .on_disabled_hover_text(if waits {
                                    "Aguarde a operação em andamento."
                                } else {
                                    "Complete a escolha para continuar."
                                })
                                .clicked()
                            {
                                if let Some(change) = change {
                                    action = Some(BarAction::Apply(change));
                                }
                            }
                            if ui.add(widgets::quiet("Cancelar")).clicked() {
                                action = Some(BarAction::Cancel);
                            }
                        });
                        if kind == AskKind::Conclude {
                            widgets::small_muted(
                                ui,
                                "As que já estão concluídas ou esperam outra tarefa em aberto ficam como estão, e o resultado diz quais.",
                            );
                        }
                    });
            });
        action
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        selected_document: Option<i64>,
    ) -> Option<i64> {
        let mut open = None;
        self.reload(store);
        if let Some(error) = self.load_error.clone() {
            let (fg, bg) = Tone::Danger.colors();
            egui::Frame::new()
                .fill(bg)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
                .outer_margin(egui::Margin::same(S4 as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(error).color(fg));
                    });
                });
            return None;
        }
        // "Desfazer" of the last batch is offered for a short while only.
        if let Some(undo) = &self.batch_undo {
            let left = std::time::Duration::from_secs(batch::UNDO_SECONDS)
                .saturating_sub(undo.at.elapsed());
            if left.is_zero() {
                self.batch_undo = None;
            } else {
                ui.ctx().request_repaint_after(left);
            }
        }
        let actor = store.active_actor();
        let data = std::mem::take(&mut self.data);
        let tasks = &data.tasks;
        let documents = &data.documents;
        let contexts = &data.contexts;
        let index: DocumentIndex = documents.iter().map(|d| (d.id, d)).collect();
        let number_of = |id: i64| {
            index
                .get(&id)
                .map(|d| d.document_number.as_str())
                .unwrap_or("documento não disponível")
        };
        let today = chrono::Local::now().date_naive();
        let today_text = today.format("%Y-%m-%d").to_string();
        let week_text = (today + chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();
        let deps = crate::tasks::Dependencies::new(tasks);
        let query_lower = self.query.trim().to_lowercase();
        let scope = Scope {
            document: self.document_filter,
            query: &query_lower,
            show_done: self.show_done,
            today: &today_text,
            week: &week_text,
        };
        // The chips count what each view would list right now.
        let [all_count, today_count, week_count, overdue_count] =
            view_counts(tasks, &index, contexts, &deps, &scope);
        let visible: Vec<&Task> = tasks
            .iter()
            .filter(|task| {
                let context = task.document_id.and_then(|id| contexts.get(&id));
                in_scope(
                    task,
                    task.document_id.and_then(|id| index.get(&id).copied()),
                    context,
                    &scope,
                ) && in_view(self.view, task, context, &deps, &scope)
            })
            .collect();
        // Each task after the ones it depends on; ties keep the usual order.
        // Another sort is applied on top of that, keeping it for ties.
        let visible = sort_tasks(deps.order(&visible), self.sort, &index);

        // Only what can be seen can stay selected, so no action ever reaches
        // a task the person cannot see.
        let visible_ids: Vec<i64> = visible.iter().map(|t| t.id).collect();
        if self.selecting {
            if batch::retain_visible(&mut self.selected, &visible_ids) {
                self.batch_ask = None;
            }
            // Ctrl+A, unless a text box has the keyboard.
            if !ui.ctx().egui_wants_keyboard_input()
                && ui.input_mut(|i| {
                    i.consume_shortcut(&egui::KeyboardShortcut::new(
                        egui::Modifiers::COMMAND,
                        egui::Key::A,
                    ))
                })
            {
                self.select_all(&visible_ids);
            }
        } else if !self.selected.is_empty() {
            self.selected.clear();
        }
        let mut bar_action: Option<BarAction> = None;

        let mut out = RowOut::default();

        // The task form is the side panel on the right; the rest is the area.
        if self.draft.is_some() {
            egui::Panel::right("task-form")
                .exact_size(FORM_WIDTH)
                .resizable(false)
                .frame(egui::Frame::new().fill(c().surface))
                .show(ui, |ui| {
                    if let Some(id) = self.show_form(ui, store, tasks, documents) {
                        open = Some(id);
                    }
                });
        }

        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(S4 as i8, S3 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = S2;
                // Area header: name, search and the one primary action.
                ui.horizontal(|ui| {
                    widgets::heading(ui, "Tarefas");
                    crate::help_ui::help(ui,
                        "Cada documento importado recebe uma tarefa de triagem. Também é possível criar tarefas sem documento (tarefas avulsas) e vinculá-las a um documento depois. As tarefas são locais: concluir aqui não tramita nem assina nada no SIGADOC.",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // With the list narrowed to a document the task is
                        // for it; otherwise it belongs to no document.
                        let target = self.document_filter;
                        let new_task = ui.add(widgets::primary(format!("{} Nova tarefa", icon::PLUS)));
                        let new_task = match target {
                            Some(id) => new_task.on_hover_text(format!("Cria uma tarefa para {}", number_of(id))),
                            None => new_task.on_hover_text("Cria uma tarefa sem documento (tarefa avulsa). Ela pode ser vinculada a um documento depois."),
                        };
                        if new_task.clicked() && !self.start_new(target) {
                            self.blocked = Some(Intent::New(target));
                        }
                        let search =
                            widgets::search(ui, &mut self.query, "Buscar tarefa ou documento", 280.0);
                        if std::mem::take(&mut self.focus_search) {
                            search.request_focus();
                        }
                        if !self.selecting
                            && ui
                                .add_enabled(self.can_select(), widgets::quiet("Selecionar"))
                                .on_hover_text("Escolhe várias tarefas para concluir, mudar o prazo, o responsável ou a prioridade, ou excluir de uma vez.")
                                .on_disabled_hover_text("Salve ou cancele as alterações da tarefa aberta antes de selecionar.")
                                .clicked()
                        {
                            self.start_selection();
                        }
                    });
                });
                // Second row: the views as chips, and the checkbox on the right.
                let checkbox_width = 330.0;
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2((ui.available_width() - checkbox_width).max(200.0), 0.0),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(S2, S2);
                            for index in PRIMARY_VIEWS {
                                let label = match index {
                                    0 => format!("{} · {all_count}", VIEWS[index]),
                                    2 => format!("{} · {today_count}", VIEWS[index]),
                                    3 => format!("{} · {week_count}", VIEWS[index]),
                                    _ => format!("{} · {overdue_count}", VIEWS[index]),
                                };
                                if widgets::chip(ui, self.view == index, label).clicked() {
                                    self.view = index;
                                }
                            }
                            let in_more = SECONDARY_VIEWS.contains(&self.view);
                            let more_label = if in_more {
                                format!("{} {}", VIEWS[self.view], icon::DOWN)
                            } else {
                                format!("Mais {}", icon::DOWN)
                            };
                            let more = widgets::chip(ui, in_more, more_label);
                            egui::Popup::menu(&more).show(|ui| {
                                for index in SECONDARY_VIEWS {
                                    if ui.selectable_label(self.view == index, VIEWS[index]).clicked() {
                                        self.view = index;
                                    }
                                }
                            });
                            // A discreet way to narrow the list to the document
                            // open in Documentos.
                            if let (None, Some(id)) = (self.document_filter, selected_document) {
                                if ui
                                    .add(widgets::quiet(format!("Só as de {}", number_of(id))))
                                    .on_hover_text("Mostra só as tarefas do documento aberto em Documentos.")
                                    .clicked()
                                {
                                    self.document_filter = Some(id);
                                }
                            }
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.add_space(S1);
                        ui.checkbox(&mut self.show_done, "Mostrar concluídas");
                        egui::ComboBox::from_id_salt("task-sort")
                            .selected_text(format!("Ordenar: {}", SORTS[self.sort.min(2)]))
                            .show_ui(ui, |ui| {
                                for (index, name) in SORTS.iter().enumerate() {
                                    ui.selectable_value(&mut self.sort, index, *name);
                                }
                            })
                            .response
                            .on_hover_text("Ordem das tarefas dentro de cada grupo.");
                    });
                });
                if self.selecting {
                    self.selection_band(ui, &visible_ids);
                }
                // Band that says the list is narrowed to one document.
                if let Some(id) = self.document_filter {
                    let title = index.get(&id).map(|d| d.title.as_str()).unwrap_or("");
                    let text = if title.is_empty() {
                        format!("Mostrando só as tarefas de {}", number_of(id))
                    } else {
                        format!("Mostrando só as tarefas de {} — {title}", number_of(id))
                    };
                    egui::Frame::new()
                        .fill(c().primary_soft)
                        .stroke(egui::Stroke::new(1.0, c().primary))
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                        .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.add(widgets::quiet("Ver todas")).clicked() {
                                            self.document_filter = None;
                                        }
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(text)
                                                            .font(theme::font_strong(theme::SIZE_BODY))
                                                            .color(c().text),
                                                    )
                                                    .truncate(),
                                                );
                                            },
                                        );
                                    },
                                );
                            });
                        });
                }
                if !self.message.is_empty() {
                    let (fg, bg) = if self.message_is_error { Tone::Danger } else { Tone::Ok }.colors();
                    let text = self.message.clone();
                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                        .inner_margin(egui::Margin::symmetric(S3 as i8, S2 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let width = (ui.available_width() - 40.0).max(100.0);
                                ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                                    ui.set_width(width);
                                    ui.add(egui::Label::new(RichText::new(text).color(fg)).wrap());
                                });
                                if widgets::icon_button(ui, icon::CLOSE, "Fechar").clicked() {
                                    self.message.clear();
                                }
                            });
                        });
                }

                // The action bar and the result of the last batch sit at the
                // bottom, under the list.
                if (self.selecting && !self.selected.is_empty()) || self.batch_result.is_some() {
                    bar_action = self.show_batch_bar(ui, &actor);
                }

                // The list: tasks grouped by deadline, one card per group.
                let focus =self.draft.as_ref().map(|t| t.id).filter(|id| *id > 0);
                let focus_before: Vec<i64> = focus
                    .map(|id| deps.direct(id).iter().map(|t| t.id).collect())
                    .unwrap_or_default();
                let focus_after: Vec<i64> = focus
                    .map(|id| {
                        tasks
                            .iter()
                            .filter(|t| t.dependencies.contains(&id))
                            .map(|t| t.id)
                            .collect()
                    })
                    .unwrap_or_default();
                let env = ListEnv {
                    tasks,
                    documents,
                    deps: &deps,
                    attachment_counts: &data.attachment_counts,
                    focus_before: &focus_before,
                    focus_after: &focus_after,
                    today: &today_text,
                    editing: self.draft.as_ref().map(|t| t.id),
                    editing_dirty: self.dirty(),
                    selecting: self.selecting,
                    selected: &self.selected,
                };
                let mut groups: [Vec<&Task>; 5] = Default::default();
                for task in &visible {
                    let slot = if task.status == "concluida" {
                        4
                    } else if task.overdue() {
                        0
                    } else if task.due_date.is_empty() {
                        3
                    } else if task.due_date == today_text {
                        1
                    } else {
                        2
                    };
                    groups[slot].push(task);
                }
                let titles_of_groups = [
                    ("Atrasadas", Some(c().danger)),
                    ("Para hoje", None),
                    ("Próximas", None),
                    ("Sem prazo", None),
                    ("Concluídas", None),
                ];
                // A view that already is one deadline group needs no overline.
                let single_group = matches!(self.view, 2 | 4 | 7);
                #[allow(unused_mut)]
                let mut list_scroll = egui::ScrollArea::vertical()
                    .id_salt("task-manager")
                    .auto_shrink([false, false]);
                #[cfg(feature = "shots")]
                if crate::shots::active() {
                    list_scroll = list_scroll.vertical_scroll_offset(self.shots_scroll);
                }
                list_scroll.show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if visible.is_empty() {
                            let text = empty_text(
                                &self.query,
                                self.document_filter.is_some(),
                                tasks.is_empty(),
                                self.view,
                                self.show_done,
                            );
                            ui.add_space(S4 * 2.0);
                            ui.vertical_centered(|ui| {
                                ui.spacing_mut().item_spacing.y = S2;
                                widgets::muted(ui, text);
                                if !self.query.trim().is_empty() {
                                    if ui.add(widgets::quiet("Limpar a busca")).clicked() {
                                        self.query.clear();
                                    }
                                } else {
                                    ui.horizontal(|ui| {
                                        // Centred pair of ways out of an empty list.
                                        let both = self.document_filter.is_some()
                                            && !self.show_done
                                            && !tasks.is_empty();
                                        let width = if both { 250.0 } else { 150.0 };
                                        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                                        if self.document_filter.is_some()
                                            && ui.add(widgets::quiet("Ver todas")).clicked()
                                        {
                                            self.document_filter = None;
                                        }
                                        if !self.show_done
                                            && !tasks.is_empty()
                                            && ui
                                                .add(widgets::quiet("Mostrar concluídas"))
                                                .on_hover_text("Inclui na lista as tarefas já concluídas.")
                                                .clicked()
                                        {
                                            self.show_done = true;
                                        }
                                    });
                                }
                            });
                        }
                        for (index, group) in groups.iter().enumerate() {
                            if group.is_empty() {
                                continue;
                            }
                            if !single_group {
                                ui.add_space(S3);
                                let (name, color) = titles_of_groups[index];
                                widgets::overline(ui, name, color);
                                ui.add_space(S1);
                            } else {
                                ui.add_space(S2);
                            }
                            widgets::card_frame()
                                .inner_margin(egui::Margin::ZERO)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for (position, task) in group.iter().enumerate() {
                                        // A task hangs from the one it waits
                                        // for only when that one is above it
                                        // in this same group.
                                        // Only the default order keeps them together.
                                        let nested = self.sort == 0
                                            && group[..position]
                                                .iter()
                                                .any(|above| task.dependencies.contains(&above.id));
                                        // Controls keep the task's own id, so a
                                        // click never lands on a neighbour when
                                        // the list changes under the pointer.
                                        ui.push_id(("task", task.id), |ui| {
                                            task_row(
                                                ui,
                                                &env,
                                                task,
                                                position == 0,
                                                position + 1 == group.len(),
                                                nested,
                                                &mut out,
                                            );
                                        });
                                    }
                                });
                        }
                        ui.add_space(S4);
                    });
            });

        if let Some(task) = out.select.or_else(|| self.open_task_requested.take()) {
            if !self.open_task(&task, store) {
                self.blocked = Some(Intent::Open(task));
            }
        }
        if let Some(id) = out.complete {
            let _ = self.apply_status_change(store, id, true);
        }
        if let Some(id) = out.reopen {
            let _ = self.apply_status_change(store, id, false);
        }
        if let Some(id) = out.toggle {
            self.toggle_selected(id);
        }
        if let Some(action) = bar_action {
            match action {
                BarAction::Ask(kind) => {
                    self.reset_batch_inputs();
                    let snapshot = batch::snapshot(store, &self.selected, tasks);
                    self.batch_ask = Some(Ask { kind, snapshot });
                }
                BarAction::Cancel => self.reset_batch_inputs(),
                BarAction::Apply(change) => {
                    if let Some(ask) = self.batch_ask.take() {
                        self.run_batch(store, &ask.snapshot, change);
                    }
                }
                BarAction::Undo => self.run_undo(store),
                BarAction::CloseResult => {
                    self.batch_result = None;
                    self.batch_undo = None;
                }
            }
            ui.ctx().request_repaint();
        }
        self.data = data;
        open
    }

    /// Reads the library again: tasks, documents, attachment counts and the
    /// work context of each document. A failure is kept to be shown.
    fn reload(&mut self, store: &Store) {
        let read = store.list_tasks().and_then(|tasks| {
            let documents = store.list_documents("")?;
            Ok((tasks, documents))
        });
        match read {
            Ok((tasks, documents)) => {
                let contexts = documents
                    .iter()
                    .filter_map(|d| store.load_work_context(d.id).ok().map(|c| (d.id, c)))
                    .collect();
                self.data = Loaded {
                    tasks,
                    documents,
                    attachment_counts: store.attachment_counts("task").unwrap_or_default(),
                    contexts,
                };
                self.load_error = None;
            }
            Err(e) => {
                self.load_error = Some(format!("Não foi possível ler as tarefas: {e}"));
            }
        }
    }

    /// Closes the form of a task nobody has changed.
    fn close_form(&mut self) {
        self.draft = None;
        self.original = None;
        self.base_version = None;
        self.message.clear();
        self.attachments.forget();
    }

    fn show_form(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        tasks: &[Task],
        documents: &[crate::model::DocumentSummary],
    ) -> Option<i64> {
        let mut open = None;
        if let Some(task) = &self.draft {
            if task.id > 0 && self.base_version.is_none() {
                self.base_version = store.item_version("task", task.id).ok().map(|v| v.version);
            }
        }
        let current_actor = store.active_actor();
        let deps = crate::tasks::Dependencies::new(tasks);
        let mut save = false;
        let mut cancel = false;
        let mut conclude = false;
        let mut delete = false;
        let mut close = false;
        let mut open_task: Option<Task> = None;
        // A new task: the document chosen for it (`Some(None)`: none). A saved
        // one: the document it was asked to move to.
        let mut new_link: Option<Option<i64>> = None;
        let mut relink: Option<Option<i64>> = None;
        let dirty = self.dirty();
        let (task_id, task_status, document_id) = {
            let task = self.draft.as_ref()?;
            (task.id, task.status.clone(), task.document_id)
        };
        let panel_margin = egui::Margin::symmetric(S4 as i8, S3 as i8);

        // Header: name of the panel, the unsaved mark and the close button.
        egui::Frame::new().inner_margin(panel_margin).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if task_id > 0 { "Tarefa" } else { "Nova tarefa" })
                        .font(theme::font_strong(17.0)),
                );
                if dirty {
                    widgets::pill(ui, "Não salva", Tone::Warn);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = ui.add_enabled_ui(!dirty && !self.attachments.dirty(), |ui| {
                        widgets::icon_button(ui, icon::CLOSE, "Fechar o painel")
                    });
                    if response.inner.clicked() {
                        close = true;
                    }
                    response
                        .response
                        .on_disabled_hover_text("Salve ou cancele as alterações antes de fechar.");
                });
            });
            let document = document_id.and_then(|id| documents.iter().find(|d| d.id == id));
            ui.add(
                egui::Label::new(
                    RichText::new(match (document_id, document) {
                        (None, _) => "Sem documento".to_owned(),
                        (Some(_), Some(d)) if !d.title.trim().is_empty() => {
                            format!("Documento: {} — {}", d.document_number, d.title.trim())
                        }
                        (Some(_), Some(d)) => format!("Documento: {}", d.document_number),
                        (Some(_), None) => "Documento: não disponível".to_owned(),
                    })
                    .size(theme::SIZE_SMALL)
                    .color(c().muted),
                )
                .wrap(),
            );
            ui.horizontal_wrapped(|ui| {
                if let Some(document_id) = document_id {
                    if ui
                        .add(widgets::quiet(format!("{} Consultar documento", icon::EYE)))
                        .on_hover_text("Mostra o texto e os PDFs do documento numa janela por cima desta ficha, para copiar um prazo ou conferir um trecho sem sair da tarefa.")
                        .clicked()
                    {
                        self.consult_requested = Some(document_id);
                    }
                    if ui
                        .add(widgets::quiet(format!("{} Ir para o documento", icon::DOCUMENT)))
                        .on_hover_text("Abre a ficha completa do documento. Se a tarefa tiver alterações não salvas, o Ponte pergunta antes o que fazer com elas.")
                        .clicked()
                    {
                        open = Some(document_id);
                    }
                }
                if task_id == 0 {
                    // A new task: choose the document before saving.
                    if document_id.is_some() {
                        if ui
                            .add(widgets::quiet("Sem documento"))
                            .on_hover_text("Cria a tarefa sem ligá-la a um documento (tarefa avulsa). Ela pode ser vinculada a um documento depois.")
                            .clicked()
                        {
                            new_link = Some(None);
                        }
                    } else if ui
                        .add(widgets::quiet("Escolher documento…"))
                        .on_hover_text("Liga a nova tarefa a um documento do acervo.")
                        .clicked()
                    {
                        self.picking = true;
                    }
                } else {
                    // A saved task changes its link only here, never through
                    // the form's Salvar.
                    let free = !dirty && !self.attachments.dirty();
                    let hint = "Salve ou cancele as alterações antes de mudar o documento da tarefa.";
                    let label = if document_id.is_none() {
                        "Vincular a um documento…"
                    } else {
                        "Trocar de documento…"
                    };
                    if ui
                        .add_enabled(free, widgets::quiet(label))
                        .on_hover_text(if document_id.is_none() {
                            "Uma tarefa avulsa não pertence a nenhum documento. Vinculá-la a um documento a leva para a ficha dele; ela continua sendo a mesma tarefa, com anexos e lista de providências."
                        } else {
                            "Leva a tarefa para outro documento. Ela continua sendo a mesma tarefa, com anexos e lista de providências."
                        })
                        .on_disabled_hover_text(hint)
                        .clicked()
                    {
                        self.picking = true;
                        self.confirm_unlink = false;
                    }
                    if document_id.is_some()
                        && ui
                            .add_enabled(free, widgets::quiet("Desvincular do documento"))
                            .on_hover_text("Tira a tarefa do documento: ela fica em Tarefas, sem documento.")
                            .on_disabled_hover_text(hint)
                            .clicked()
                    {
                        self.confirm_unlink = true;
                        self.picking = false;
                    }
                }
            });
            if self.confirm_unlink && document_id.is_some() {
                ui.horizontal_wrapped(|ui| {
                    widgets::small_muted(
                        ui,
                        "A tarefa continua em Tarefas, sem documento. Os anexos e a lista de providências ficam com ela.",
                    );
                    if ui.add(widgets::secondary("Desvincular")).clicked() {
                        relink = Some(None);
                        self.confirm_unlink = false;
                    }
                    if ui.add(widgets::quiet("Cancelar")).clicked() {
                        self.confirm_unlink = false;
                    }
                });
            }
            if self.picking {
                ui.add_space(S1);
                widgets::field_label(ui, "Escolher o documento");
                if let Some(chosen) = self.doc_picker.show(
                    ui,
                    store,
                    "Número ou assunto do documento",
                ) {
                    self.picking = false;
                    if task_id == 0 {
                        new_link = Some(Some(chosen.id));
                    } else {
                        relink = Some(Some(chosen.id));
                    }
                }
                if ui.add(widgets::quiet("Cancelar")).clicked() {
                    self.picking = false;
                    self.doc_picker.clear();
                }
            }
            // Uses of rooms, books and equipment tied to a saved task.
            if task_id > 0 {
                let link = crate::resources_ui::UseLink::Task(task_id);
                let uses = crate::resources_ui::linked_uses(store, &link);
                ui.add_space(S1);
                for item in &uses {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(widgets::quiet("Abrir"))
                                .on_hover_text("Abre o uso na área Recursos.")
                                .clicked()
                            {
                                self.use_request =
                                    Some(crate::resources_ui::UseRequest::Open(item.uid.clone()));
                            }
                            widgets::pill(ui, item.label.clone(), item.tone);
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(egui::Label::new(item.text.as_str()).wrap());
                                },
                            );
                        });
                    });
                }
                if ui
                    .add(widgets::quiet(format!(
                        "{} Reservar recurso…",
                        icon::RESOURCES
                    )))
                    .on_hover_text("Reserva ou empresta uma sala, um livro ou um equipamento para esta tarefa.")
                    .clicked()
                {
                    self.use_request = Some(crate::resources_ui::UseRequest::Reserve(link));
                }
            }
        });
        ui.separator();

        // Footer, fixed at the bottom: Salvar, Concluir, Cancelar; Excluir apart.
        egui::Panel::bottom("task-form-foot")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(c().surface)
                    .inner_margin(panel_margin),
            )
            .show(ui, |ui| {
                let top = ui.max_rect();
                ui.painter().hline(
                    (top.left() - S4)..=(top.right() + S4),
                    top.top() - S3,
                    egui::Stroke::new(1.0, c().outline),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = S2;
                    save = ui
                        .add_enabled(dirty || task_id == 0, widgets::primary("Salvar"))
                        .clicked();
                    if task_id > 0 && task_status != "concluida" {
                        conclude = ui
                            .add_enabled(!dirty, widgets::secondary("Concluir"))
                            .on_disabled_hover_text(
                                "Salve ou cancele as alterações antes de concluir.",
                            )
                            .clicked();
                    }
                    cancel = ui
                        .add_enabled(dirty || task_id == 0, widgets::quiet("Cancelar"))
                        .clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        delete = ui
                            .add_enabled(task_id > 0 && !dirty, widgets::danger("Excluir…"))
                            .on_disabled_hover_text(
                                "Salve ou cancele as alterações antes de excluir esta tarefa.",
                            )
                            .clicked();
                    });
                });
            });

        let focus_title = std::mem::take(&mut self.focus_title);
        let Some(task) = &mut self.draft else {
            return None;
        };
        #[cfg(feature = "shots")]
        let force_details = self.shots_details;
        #[allow(unused_mut)]
        let mut form_scroll = egui::ScrollArea::vertical()
            .id_salt("task-form")
            .auto_shrink([false, false]);
        #[cfg(feature = "shots")]
        if crate::shots::active() {
            form_scroll =
                form_scroll.vertical_scroll_offset(if force_details { 330.0 } else { 0.0 });
        }
        form_scroll.show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(S4 as i8, S3 as i8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = S1;
                        widgets::field_label(ui, "Título");
                        let title_edit = ui.add(
                            egui::TextEdit::singleline(&mut task.title).desired_width(f32::INFINITY),
                        );
                        if focus_title {
                            title_edit.request_focus();
                        }
                        ui.add_space(S2);
                        two_columns(
                            ui,
                            0.62,
                            |ui| {
                                ui.horizontal(|ui| {
                                    widgets::field_label(ui, "Prazo");
                                    crate::help_ui::help(
                                        ui,
                                        "Deixe vazio se não houver prazo confirmado. Anote a origem em “Mais detalhes”.",
                                    );
                                });
                                let response =
                                    crate::labels::date_field(ui, ("task-due", task_id), &mut task.due_date);
                                if !task.due_date.trim().is_empty()
                                    && crate::labels::parse_typed_date(&task.due_date).is_none()
                                {
                                    widgets::outline(ui, response.rect.expand(2.0), c().danger);
                                }
                            },
                            |ui| {
                                widgets::field_label(ui, "Prioridade");
                                egui::ComboBox::from_id_salt("Prioridade")
                                    .selected_text(crate::labels::priority(&task.priority).to_owned())
                                    .width(ui.available_width() - 24.0)
                                    .show_ui(ui, |ui| {
                                        for choice in ["baixa", "normal", "alta", "urgente"] {
                                            ui.selectable_value(
                                                &mut task.priority,
                                                choice.to_string(),
                                                crate::labels::priority(choice),
                                            );
                                        }
                                    });
                            },
                        );
                        ui.add_space(S2);
                        two_columns(
                            ui,
                            0.55,
                            |ui| {
                                widgets::field_label(ui, "Responsável");
                                ui.horizontal(|ui| {
                                    let width = (ui.available_width() - 52.0).max(60.0);
                                    ui.add(egui::TextEdit::singleline(&mut task.assignee).desired_width(width));
                                    if ui
                                        .add(widgets::secondary("Eu"))
                                        .on_hover_text(format!("Atribuir a {current_actor}"))
                                        .clicked()
                                    {
                                        task.assignee = current_actor.clone();
                                    }
                                });
                            },
                            |ui| {
                                widgets::field_label(ui, "Situação");
                                egui::ComboBox::from_id_salt("Situação")
                                    .selected_text(crate::tasks::status_label(&task.status).to_owned())
                                    .width(ui.available_width() - 24.0)
                                    .show_ui(ui, |ui| {
                                        for choice in ["pendente", "em analise", "aguardando", "concluida"] {
                                            ui.selectable_value(
                                                &mut task.status,
                                                choice.to_string(),
                                                crate::tasks::status_label(choice),
                                            );
                                        }
                                    });
                            },
                        );
                        ui.add_space(S2);
                        widgets::field_label(ui, "Ação");
                        egui::ComboBox::from_id_salt("Ação")
                            .selected_text(crate::tasks::action_label(&task.action).to_owned())
                            .width(ui.available_width() - 24.0)
                            .show_ui(ui, |ui| {
                                for choice in crate::tasks::ACTIONS {
                                    // Triage is about a document.
                                    if document_id.is_none() && *choice == "triar" && task.action != "triar" {
                                        continue;
                                    }
                                    ui.selectable_value(
                                        &mut task.action,
                                        choice.to_string(),
                                        crate::tasks::action_label(choice),
                                    );
                                }
                            });
                        ui.add_space(S2);
                        widgets::field_label(ui, "Observações");
                        ui.add(
                            egui::TextEdit::multiline(&mut task.notes)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        if let Some(at) = &task.completed_at {
                            widgets::small_muted(ui, format!("Concluída em {}", crate::labels::datetime(at)));
                        }
                        ui.add_space(S2);
                        ui.separator();
                        ui.add_space(S1);
                        #[allow(unused_mut)]
                        let mut more = egui::CollapsingHeader::new(
                            RichText::new("Mais detalhes").font(theme::font_strong(theme::SIZE_BODY)),
                        )
                        .id_salt(("task-more", task_id));
                        #[cfg(feature = "shots")]
                        if force_details {
                            more = more.open(Some(true));
                        }
                        more.show(ui, |ui| {
                            widgets::small_muted(
                                ui,
                                "Origem do prazo, lembrete, resultado, providências, dependências e anexos.",
                            );
                            ui.add_space(S1);
                            widgets::field_label(ui, "Origem do prazo (trecho, arquivo ou orientação)");
                            ui.add(
                                egui::TextEdit::singleline(&mut task.due_source)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(S2);
                            ui.horizontal(|ui| {
                                widgets::field_label(ui, "Lembrar a partir de");
                                crate::help_ui::help(ui, "Lembrete local, mostrado na lista “Com lembrete” enquanto o Ponte estiver aberto.");
                            });
                            crate::labels::date_field(ui, ("task-reminder", task_id), &mut task.reminder_date);
                            ui.add_space(S2);
                            widgets::field_label(ui, "Resultado (número da resposta, data de envio, observações)");
                            ui.add(
                                egui::TextEdit::multiline(&mut task.outcome)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(S2);
                            ui.collapsing("Lista de providências", |ui| {
                                let mut remove = None;
                                for (index, (done, text)) in task.checklist.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.checkbox(done, "");
                                        ui.add(
                                            egui::TextEdit::singleline(text)
                                                .desired_width((ui.available_width() - 40.0).max(60.0)),
                                        );
                                        if widgets::icon_button(ui, icon::CLOSE, "Remover etapa").clicked() {
                                            remove = Some(index);
                                        }
                                    });
                                }
                                if let Some(index) = remove {
                                    task.checklist.remove(index);
                                }
                                if task.checklist.len() < 100
                                    && ui.add(widgets::secondary(format!("{} Adicionar etapa", icon::PLUS))).clicked()
                                {
                                    task.checklist.push((false, String::new()));
                                }
                            });
                            if let Some(doc) = document_id.and_then(|id| store.load_document(id).ok()) {
                                if let Some(analysis) = doc.analysis {
                                    ui.horizontal_wrapped(|ui| {
                                        widgets::small_muted(
                                            ui,
                                            format!(
                                                "{} Sugestão da IA: prioridade {} • prazo mencionado: {}{}",
                                                icon::AI,
                                                crate::labels::priority(&analysis.priority),
                                                analysis
                                                    .deadline_candidate
                                                    .as_deref()
                                                    .unwrap_or("não identificado"),
                                                if doc.summary.analysis_stale {
                                                    " • análise desatualizada"
                                                } else {
                                                    ""
                                                }
                                            ),
                                        );
                                        if !doc.summary.analysis_stale
                                            && ui.add(widgets::quiet("Usar prioridade sugerida")).clicked()
                                        {
                                            task.priority = analysis.priority;
                                        }
                                    });
                                }
                            }
                            if let Some(context) = document_id.and_then(|id| store.load_work_context(id).ok()) {
                                let action = match context.purpose.as_str() {
                                    "ciencia e download" => "tomar ciencia",
                                    "responder" => "responder",
                                    "examinar" => "examinar",
                                    "encaminhar" => "encaminhar",
                                    "acompanhar" => "acompanhar",
                                    _ => "",
                                };
                                ui.horizontal_wrapped(|ui| {
                                    if !action.is_empty()
                                        && ui
                                            .add(widgets::quiet(format!(
                                                "Usar a finalidade da ficha: {}",
                                                crate::tasks::action_label(action)
                                            )))
                                            .on_hover_text("Define a ação da tarefa. O título só é preenchido se estiver vazio.")
                                            .clicked()
                                    {
                                        task.action = action.into();
                                        if task.title.trim().is_empty() {
                                            task.title =
                                                format!("{} o documento", crate::tasks::action_label(action));
                                        }
                                    }
                                    if !context.priority.is_empty()
                                        && ui
                                            .add(widgets::quiet(format!(
                                                "Usar a prioridade da ficha: {}",
                                                crate::labels::priority(&context.priority)
                                            )))
                                            .clicked()
                                    {
                                        task.priority = context.priority;
                                    }
                                });
                            }
                            ui.collapsing("Depende de outras tarefas", |ui| {
                                widgets::small_muted(ui, "Uma tarefa só pode ser concluída depois das tarefas de que ela depende. As que já dependem desta aparecem desativadas: marcá-las criaria um ciclo.");
                                // Marking a task that already waits for this one would
                                // close a loop; saving would refuse it anyway.
                                let cycle_makers: std::collections::BTreeSet<i64> = if task_id > 0 {
                                    deps.descendants(task_id).iter().map(|t| t.id).collect()
                                } else {
                                    Default::default()
                                };
                                let mut options: Vec<&Task> = tasks.iter().filter(|t| t.id != task_id).collect();
                                // Concluded tasks last, greyed, so the live ones stand out.
                                options.sort_by_key(|t| t.status == "concluida");
                                egui::ScrollArea::vertical().id_salt("dependency-options").max_height(180.0).show(ui, |ui| {
                                    for previous in options {
                                        let mut checked = task.dependencies.contains(&previous.id);
                                        let would_cycle = cycle_makers.contains(&previous.id);
                                        let label = format!(
                                            "{} — {}",
                                            previous.title,
                                            crate::tasks::status_label(&previous.status)
                                        );
                                        let text = if previous.status == "concluida" {
                                            RichText::new(label).color(c().muted)
                                        } else {
                                            RichText::new(label)
                                        };
                                        let response = ui.add_enabled(
                                            !would_cycle || checked,
                                            egui::Checkbox::new(&mut checked, text),
                                        );
                                        let response = if would_cycle {
                                            response.on_disabled_hover_text(format!(
                                                "“{}” já depende desta tarefa (direta ou indiretamente). Marcá-la criaria um ciclo.",
                                                previous.title
                                            ))
                                        } else {
                                            response
                                        };
                                        if response.changed() {
                                            if checked {
                                                task.dependencies.push(previous.id);
                                            } else {
                                                task.dependencies.retain(|id| *id != previous.id);
                                            }
                                        }
                                    }
                                });
                                for missing in task.dependencies.clone() {
                                    if !tasks.iter().any(|t| t.id == missing)
                                        && ui
                                            .add(widgets::secondary("Remover dependência de tarefa que não está mais disponível"))
                                            .clicked()
                                    {
                                        task.dependencies.retain(|id| *id != missing);
                                    }
                                }
                            });
                            if task_id > 0 {
                                let before = deps.ancestors(task_id);
                                let after = deps.descendants(task_id);
                                if !before.is_empty() || !after.is_empty() {
                                    ui.add_space(S2);
                                    ui.horizontal_wrapped(|ui| {
                                        widgets::field_label(ui, "Cadeia de dependências");
                                        crate::help_ui::help(ui,
                                            "Tudo o que vem antes desta tarefa (em qualquer nível) e tudo o que espera por ela. Para mudar as dependências diretas, use “Mais detalhes ▸ Depende de outras tarefas”.",
                                        );
                                    });
                                    if !before.is_empty() {
                                        widgets::small_muted(ui, "Antes desta (precisam ser concluídas primeiro):");
                                        for previous in &before {
                                            chain_row(ui, previous, &mut open_task);
                                        }
                                    }
                                    if !after.is_empty() {
                                        widgets::small_muted(ui, "Depois desta (esperam por ela):");
                                        for next in &after {
                                            chain_row(ui, next, &mut open_task);
                                        }
                                    }
                                }
                            }
                            ui.add_space(S2);
                            widgets::field_label(ui, "Anexos");
                            if task_id > 0 {
                                self.attachments
                                    .show(ui, store, crate::attachments::Owner::Task(task_id), false);
                            } else {
                                widgets::small_muted(ui, "Salve a tarefa para poder anexar arquivos a ela.");
                            }
                        });
                    });
            });
        if let Some(link) = new_link {
            self.set_new_task_link(link);
        }
        if let Some(target) = relink {
            let name = target.map(|id| {
                documents
                    .iter()
                    .find(|d| d.id == id)
                    .map(|d| d.document_number.clone())
                    .unwrap_or_else(|| "o documento".to_owned())
            });
            self.apply_relink(store, task_id, target, name);
        }
        if open_task.is_some() {
            self.open_task_requested = open_task;
        }
        if delete {
            self.trash_requested = Some(crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id: task_id,
            });
        }
        if save {
            // The panel already shows the reason when saving fails.
            let _ = self.save_draft(store);
        }
        if cancel {
            self.discard_draft();
        }
        if conclude {
            let _ = self.apply_status_change(store, task_id, true);
        }
        if close && !self.dirty() && !self.attachments.dirty() {
            self.close_form();
        }
        open
    }

    /// Chooses the document of the new task in the form (`None`: no
    /// document). A task nobody has touched is replaced by the blank one for
    /// that choice (the triage title belongs to a document, not to a task
    /// without one); an edited one keeps what was typed.
    fn set_new_task_link(&mut self, document_id: Option<i64>) {
        let Some(task) = &mut self.draft else {
            return;
        };
        if task.id != 0 {
            return;
        }
        if *task == Task::blank(task.document_id) {
            *task = Task::blank(document_id);
        } else {
            task.document_id = document_id;
            if document_id.is_none() && task.action == "triar" {
                task.action = "examinar".into();
            }
        }
        self.focus_title = true;
    }

    /// Moves the open (saved, unedited) task to another document or to none,
    /// and shows it again as it is now stored.
    fn apply_relink(
        &mut self,
        store: &mut Store,
        task_id: i64,
        target: Option<i64>,
        name: Option<String>,
    ) {
        if self.dirty() || task_id == 0 {
            return;
        }
        match store.relink_task(task_id, target, self.base_version) {
            Ok(id) => {
                let task = store
                    .list_tasks()
                    .ok()
                    .and_then(|tasks| tasks.into_iter().find(|t| t.id == id));
                if task.is_some() {
                    self.draft = task.clone();
                    self.original = task;
                }
                self.base_version = store.item_version("task", id).ok().map(|v| v.version);
                self.batch_undo = None;
                self.notice(
                    match name {
                        Some(number) => format!("Tarefa vinculada a {number}."),
                        None => "Tarefa desvinculada: agora está sem documento.".to_owned(),
                    },
                    false,
                );
            }
            Err(error) => self.notice(
                format!("Não foi possível trocar o documento da tarefa: {error}"),
                true,
            ),
        }
    }

    fn apply_status_change(
        &mut self,
        store: &mut Store,
        id: i64,
        complete: bool,
    ) -> Result<(), String> {
        let before = if complete {
            store
                .list_tasks()
                .ok()
                .and_then(|tasks| tasks.into_iter().find(|t| t.id == id))
        } else {
            None
        };
        let result = if complete {
            store.complete_task(id)
        } else {
            store.reopen_task(id)
        };
        match result {
            Ok(updated) => {
                self.batch_undo = None;
                if complete {
                    self.concluded = before;
                }
                if self.draft.as_ref().is_some_and(|t| t.id == id) {
                    self.draft = Some(updated.clone());
                    self.original = Some(updated.clone());
                    self.base_version = store.item_version("task", id).ok().map(|v| v.version);
                }
                let mut text = if complete {
                    format!("Tarefa concluída: {}", updated.title)
                } else {
                    format!("Tarefa reaberta: {}", updated.title)
                };
                for note in store.take_cascade_notes() {
                    text.push_str(" • ");
                    text.push_str(&note);
                }
                self.notice(text, false);
                Ok(())
            }
            Err(e) => {
                let text = format!(
                    "Não foi possível {} a tarefa: {e}",
                    if complete { "concluir" } else { "reabrir" }
                );
                self.notice(text.clone(), true);
                Err(text)
            }
        }
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;

    fn task(id: i64, document: i64, due: &str, status: &str) -> Task {
        let mut t = Task::new(document);
        t.id = id;
        t.title = format!("Tarefa {id}");
        t.due_date = due.into();
        t.status = status.into();
        if status == "concluida" {
            t.completed_at = Some("2026-11-01T10:00:00".into());
        }
        t
    }

    fn scope<'a>(document: Option<i64>, query: &'a str, show_done: bool) -> Scope<'a> {
        Scope {
            document,
            query,
            show_done,
            today: "2026-11-10",
            week: "2026-11-17",
        }
    }

    #[test]
    fn chip_counts_follow_the_list_and_match_the_rail_badge() {
        let tasks = vec![
            task(1, 1, "2026-11-05", "pendente"),
            task(2, 1, "2026-11-10", "pendente"),
            task(3, 2, "2026-11-10", "em analise"),
            task(4, 2, "2026-11-12", "pendente"),
            task(5, 2, "", "pendente"),
            task(6, 1, "2026-11-01", "concluida"),
        ];
        let deps = crate::tasks::Dependencies::new(&tasks);
        let documents = DocumentIndex::new();
        let contexts = Contexts::new();
        let none = scope(None, "", false);
        let [all, today, week, overdue] = view_counts(&tasks, &documents, &contexts, &deps, &none);
        assert_eq!([all, today, week, overdue], [5, 2, 3, 1]);
        // Same rule the rail badge uses: open, with a deadline up to today.
        let badge = tasks
            .iter()
            .filter(|t| {
                t.completed_at.is_none()
                    && !t.due_date.is_empty()
                    && t.due_date.as_str() <= "2026-11-10"
            })
            .count();
        assert_eq!(overdue + today, badge);
        // Narrowed to one document.
        let narrowed = scope(Some(2), "", false);
        assert_eq!(
            view_counts(&tasks, &documents, &contexts, &deps, &narrowed),
            [3, 1, 2, 0]
        );
        // Concluded tasks count in "Todas" only when shown.
        let with_done = scope(None, "", true);
        assert_eq!(
            view_counts(&tasks, &documents, &contexts, &deps, &with_done)[0],
            6
        );
        // A search narrows every count (title "Tarefa 4").
        let searching = scope(None, "tarefa 4", false);
        assert_eq!(
            view_counts(&tasks, &documents, &contexts, &deps, &searching),
            [1, 0, 1, 0]
        );
    }

    #[test]
    fn opens_unfiltered_even_when_a_document_is_selected() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("tasks-ui-open.sqlite3")).unwrap();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let mut panel = TaskPanel::default();
        // `show` no longer reads the selected document into the filter (a
        // drawing test is not possible without a window), so a fresh panel
        // that loads the library has no filter.
        panel.reload(&store);
        assert_eq!(panel.document_filter(), None);
        assert!(!panel.data.tasks.is_empty(), "the list holds every task");
        panel.focus_document(document);
        assert_eq!(panel.document_filter(), Some(document));
        panel.clear_document_filter();
        assert_eq!(panel.document_filter(), None);
    }

    #[test]
    fn escape_never_closes_a_dirty_task() {
        let mut panel = TaskPanel::default();
        assert!(!panel.key_escape(), "nothing open: Esc is not ours");
        let mut open = Task::new(1);
        open.id = 10;
        assert!(panel.focus_task(&open));
        assert!(panel.key_escape());
        assert!(panel.draft.is_none(), "a clean panel closes");
        assert!(panel.focus_task(&open));
        panel.draft.as_mut().unwrap().notes = "ainda escrevendo".into();
        assert!(panel.key_escape(), "handled, even though it stays open");
        assert!(panel.draft.is_some());
        assert!(panel.dirty());
        assert_eq!(panel.open_task_id(), Some(10));
        panel.key_new(None);
        assert_eq!(panel.take_blocked(), Some(Intent::New(None)));
        // The document last opened in Documentos does not decide...
        panel.key_new(Some(1));
        assert_eq!(panel.take_blocked(), Some(Intent::New(None)));
        // ...the one the list is narrowed to does.
        panel.document_filter = Some(1);
        panel.key_new(None);
        assert_eq!(panel.take_blocked(), Some(Intent::New(Some(1))));
    }

    #[test]
    fn conclusion_is_recorded_once_and_can_be_undone() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("tasks-ui-undo.sqlite3")).unwrap();
        store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let id = store.list_tasks().unwrap()[0].id;
        let mut panel = TaskPanel::default();
        panel.apply_status_change(&mut store, id, true).unwrap();
        let before = panel.take_concluded().expect("recorded");
        assert_eq!((before.id, before.status.as_str()), (id, "pendente"));
        assert!(panel.take_concluded().is_none(), "returned once");
        assert_eq!(store.list_tasks().unwrap()[0].status, "concluida");
        panel.undo_conclude(&mut store, id).unwrap();
        assert_eq!(store.list_tasks().unwrap()[0].status, "pendente");
        assert!(panel.undo_conclude(&mut store, id).is_err());
    }

    #[test]
    fn empty_lists_say_why() {
        assert!(empty_text("ofício", false, false, 0, false).contains("“ofício”"));
        assert!(empty_text("", true, false, 0, false).contains("Este documento"));
        assert!(empty_text("", false, true, 0, false).starts_with("Ainda não há tarefas"));
        assert_eq!(
            empty_text("", false, false, 4, false),
            "Nenhuma tarefa atrasada."
        );
    }

    #[test]
    fn navigation_filters_without_losing_unsaved_task() {
        let mut panel = TaskPanel::default();
        let mut first = Task::new(1);
        first.id = 10;
        assert!(panel.focus_task(&first));
        panel.draft.as_mut().unwrap().notes = "Edição ainda não salva".into();
        let mut other = Task::new(2);
        other.id = 20;
        assert!(!panel.focus_task(&other));
        panel.focus_document(2);
        assert_eq!(
            panel.draft.as_ref().unwrap().notes,
            "Edição ainda não salva"
        );
        assert!(panel.focus_task(&first));
        assert!(panel.dirty());
        assert!(
            !panel.start_new(Some(3)),
            "a pending edit is never discarded"
        );
    }

    #[test]
    fn leaving_an_unsaved_task_is_a_question_not_a_silent_loss() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("tasks-ui.sqlite3")).unwrap();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let mut panel = TaskPanel::default();

        // A new task with text typed in: the question names it.
        assert!(panel.start_new(Some(document)));
        assert_eq!(
            panel.editing_title().as_deref(),
            None,
            "a blank new task has nothing to lose"
        );
        panel.draft.as_mut().unwrap().title = "Responder ao ofício".into();
        assert_eq!(
            panel.editing_title().as_deref(),
            Some("Responder ao ofício")
        );

        // "Salvar e continuar" saves and leaves nothing pending.
        panel.save_draft(&mut store).unwrap();
        assert!(!panel.dirty());
        let saved = store.list_tasks().unwrap();
        assert!(saved.iter().any(|t| t.title == "Responder ao ofício"));

        // A refused save keeps the edit and explains.
        panel.draft.as_mut().unwrap().title = "   ".into();
        let error = panel.save_draft(&mut store).unwrap_err();
        assert!(
            error.starts_with("Não foi possível salvar a tarefa:"),
            "{error}"
        );
        assert!(panel.dirty());

        // "Descartar alterações" on an existing task goes back to what is saved.
        panel.discard_draft();
        assert!(!panel.dirty());
        assert_eq!(panel.draft.as_ref().unwrap().title, "Responder ao ofício");

        // Discarding a brand-new task closes the form.
        let mut other = TaskPanel::default();
        assert!(other.start_new(Some(document)));
        other.draft.as_mut().unwrap().notes = "x".into();
        other.discard_draft();
        assert!(other.draft.is_none());

        // Blocked intents are handed to the app instead of being refused with a message.
        panel.draft.as_mut().unwrap().notes = "ainda escrevendo".into();
        assert!(panel.editing_title().is_some());
        assert!(!panel.start_new(Some(document)));
        assert!(
            panel.take_blocked().is_none(),
            "start_new itself does not queue; the panel's button does"
        );
        panel.blocked = Some(Intent::New(Some(document)));
        assert_eq!(panel.take_blocked(), Some(Intent::New(Some(document))));
        assert!(panel.take_blocked().is_none());
    }
}

/// Development only: the views of this screen pictured by `--shots`, by
/// name. The tour calls `shots_open` with each one on a fresh panel.
#[cfg(feature = "shots")]
pub const SHOTS: &[&str] = &[
    "lista",
    "aberta",
    "nova",
    "avulsa",
    "nova-avulsa",
    "vincular",
    "concluidas",
    "detalhes",
    "filtrada",
    "vazia-busca",
    "vazia-lembrete",
    "vazia-documento",
    "selecao",
    "selecao-concluir",
    "selecao-prazo",
    "selecao-excluir",
    "selecao-resultado",
];

#[cfg(feature = "shots")]
impl TaskPanel {
    /// Puts the panel in the state called `view` (see `SHOTS`).
    pub fn shots_open(&mut self, store: &mut crate::storage::Store, view: &str) {
        let tasks = store.list_tasks().unwrap_or_default();
        self.document_filter = None;
        match view {
            "filtrada" => {
                let target = store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| d.document_number == "ORG-OFI-2026/00431");
                if let Some(document) = target {
                    self.focus_document(document.id);
                }
            }
            "vazia-busca" => self.query = "parecer jurídico".into(),
            "vazia-lembrete" => self.view = 6,
            "vazia-documento" => {
                let target = store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| d.document_number == "ORG-OFI-2026/00398");
                if let Some(document) = target {
                    self.focus_document(document.id);
                    self.view = 4;
                }
            }
            "aberta" => {
                if let Some(task) = tasks.iter().find(|t| t.overdue()) {
                    self.open_task(task, store);
                }
            }
            "nova" => {
                let target = store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| d.document_number == "ORG-OFI-2026/00431");
                if let Some(document) = target {
                    self.start_new(Some(document.id));
                }
            }
            "avulsa" | "vincular" => {
                if let Some(task) = tasks.iter().find(|t| t.document_id.is_none()) {
                    self.open_task(task, store);
                }
                if view == "vincular" {
                    self.picking = true;
                    self.doc_picker.shots_query(store, "ORG-OFI");
                }
            }
            "nova-avulsa" => {
                self.start_new(None);
            }
            "selecao" | "selecao-concluir" | "selecao-prazo" | "selecao-excluir"
            | "selecao-resultado" => {
                self.selecting = true;
                let deps = crate::tasks::Dependencies::new(&tasks);
                let open: Vec<&Task> = tasks.iter().filter(|t| t.status != "concluida").collect();
                if view == "selecao-resultado" {
                    // Four concluded, one left because it waits for another
                    // task (what the result line says; nothing is changed).
                    let blocked = open.iter().find(|t| deps.is_blocked(t.id));
                    let title = blocked.map(|t| t.title.clone()).unwrap_or_default();
                    self.selected = blocked.iter().map(|t| t.id).collect();
                    let outcome = batch::Outcome {
                        done: 4,
                        skipped: vec![batch::Skip {
                            id: 0,
                            title,
                            reason: batch::Reason::Blocked,
                        }],
                        ..Default::default()
                    };
                    self.batch_result = Some((
                        batch::message(Some(&batch::Change::Conclude), false, &outcome),
                        Tone::Warn,
                    ));
                    self.batch_undo = Some(BatchUndo {
                        at: std::time::Instant::now(),
                        items: Vec::new(),
                        trash: false,
                    });
                } else {
                    self.selected = open.iter().take(3).map(|t| t.id).collect();
                    let kind = match view {
                        "selecao-concluir" => Some(AskKind::Conclude),
                        "selecao-prazo" => Some(AskKind::Due),
                        "selecao-excluir" => Some(AskKind::Trash),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        if kind == AskKind::Due {
                            self.batch_due = "2026-11-14".into();
                        }
                        let snapshot = batch::snapshot(store, &self.selected, &tasks);
                        self.batch_ask = Some(Ask { kind, snapshot });
                    }
                }
            }
            "concluidas" => {
                self.show_done = true;
                self.shots_scroll = 520.0;
            }
            "detalhes" => {
                let task = tasks
                    .iter()
                    .find(|t| !t.dependencies.is_empty())
                    .or_else(|| tasks.iter().find(|t| t.overdue()));
                if let Some(task) = task {
                    self.open_task(task, store);
                }
                self.shots_details = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod standalone_tests {
    use super::*;

    fn loose(id: i64, due: &str) -> Task {
        let mut t = Task::standalone();
        t.id = id;
        t.title = format!("Avulsa {id}");
        t.due_date = due.into();
        t
    }

    fn scope(query: &str) -> Scope<'_> {
        Scope {
            document: None,
            query,
            show_done: false,
            today: "2026-11-10",
            week: "2026-11-17",
        }
    }

    #[test]
    fn a_new_task_without_document_is_not_unsaved_work_until_touched() {
        let mut panel = TaskPanel::default();
        assert!(panel.start_new(None));
        assert!(!panel.dirty());
        assert_eq!(panel.draft.as_ref().unwrap().document_id, None);
        assert_ne!(panel.draft.as_ref().unwrap().action, "triar");
        panel.draft.as_mut().unwrap().title = "Renovar o certificado".into();
        assert!(panel.dirty());
        assert!(
            !panel.start_new(Some(3)),
            "a pending edit is never discarded"
        );
        // With nothing typed, the choice of document replaces the blank task.
        let mut panel = TaskPanel::default();
        assert!(panel.start_new(Some(3)));
        panel.set_new_task_link(None);
        assert!(!panel.dirty());
        assert_eq!(panel.draft.as_ref().unwrap().title, "");
        panel.set_new_task_link(Some(5));
        assert!(!panel.dirty());
        assert_eq!(panel.draft.as_ref().unwrap().document_id, Some(5));
        assert_eq!(panel.draft.as_ref().unwrap().action, "triar");
        // What was typed is kept; a triage action never stays on a task
        // without document.
        panel.draft.as_mut().unwrap().title = "Conferir o parecer".into();
        panel.set_new_task_link(None);
        let draft = panel.draft.as_ref().unwrap();
        assert_eq!(draft.title, "Conferir o parecer");
        assert_eq!(draft.document_id, None);
        assert_eq!(draft.action, "examinar");
    }

    #[test]
    fn the_list_treats_a_task_without_document_deliberately() {
        let tasks = vec![loose(1, "2026-11-10")];
        let deps = crate::tasks::Dependencies::new(&tasks);
        let scope_all = scope("");
        // It is counted and listed, never "to triage", and found by
        // "sem documento".
        assert!(in_scope(&tasks[0], None, None, &scope_all));
        assert!(!in_view(1, &tasks[0], None, &deps, &scope_all));
        assert!(in_view(2, &tasks[0], None, &deps, &scope_all));
        assert!(in_scope(&tasks[0], None, None, &scope("sem documento")));
        assert_eq!(
            view_counts(
                &tasks,
                &DocumentIndex::new(),
                &Contexts::new(),
                &deps,
                &scope_all
            ),
            [1, 1, 1, 0]
        );
        // Narrowed to a document, it is out.
        let narrowed = Scope {
            document: Some(1),
            ..scope("")
        };
        assert!(!in_scope(&tasks[0], None, None, &narrowed));
        // Sorted by document, the ones without document come first.
        let mut linked = Task::new(1);
        linked.id = 2;
        let documents = DocumentIndex::new();
        let list = vec![&linked, &tasks[0]];
        let sorted = sort_tasks(list, 2, &documents);
        assert_eq!(sorted[0].id, 1);
    }

    #[test]
    fn relinking_from_the_panel_shows_the_task_again_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("avulsa.sqlite3")).unwrap();
        let document = store
            .import_capture(&crate::model::sample_capture())
            .unwrap()
            .document_id;
        let mut task = Task::standalone();
        task.title = "Pedir a sala".into();
        let id = store.save_task(&task).unwrap();
        let mut panel = TaskPanel::default();
        panel.reload(&store);
        let open = panel
            .data
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .clone();
        assert!(panel.open_task(&open, &store));
        panel.apply_relink(&mut store, id, Some(document), Some("SEFAZ".into()));
        assert_eq!(panel.draft.as_ref().unwrap().document_id, Some(document));
        assert!(!panel.dirty());
        assert_eq!(panel.message, "Tarefa vinculada a SEFAZ.");
        panel.apply_relink(&mut store, id, None, None);
        assert_eq!(panel.draft.as_ref().unwrap().document_id, None);
        // Refused while the form has unsaved changes.
        panel.draft.as_mut().unwrap().notes = "sem salvar".into();
        panel.apply_relink(&mut store, id, Some(document), Some("SEFAZ".into()));
        assert_eq!(panel.draft.as_ref().unwrap().document_id, None);
    }
}

#[cfg(test)]
mod batch_tests {
    use super::batch::*;
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("batch.sqlite3")).unwrap();
        (dir, store)
    }

    /// A task without document, saved; `after` are the tasks it waits for.
    fn add(store: &mut Store, title: &str, after: &[i64]) -> i64 {
        let mut task = Task::standalone();
        task.title = title.into();
        task.dependencies = after.to_vec();
        store.save_task(&task).unwrap()
    }

    fn get(store: &Store, id: i64) -> Task {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    fn pick(store: &Store, ids: &[i64]) -> Snapshot {
        snapshot(
            store,
            &ids.iter().copied().collect(),
            &store.list_tasks().unwrap(),
        )
    }

    fn touch(store: &mut Store, id: i64) {
        // Another edit lands: the version moves on.
        let mut task = get(store, id);
        task.notes = "outra edição".into();
        let version = store.item_version("task", id).unwrap().version;
        store.save_task_checked(&task, Some(version)).unwrap();
    }

    #[test]
    fn the_selection_keeps_only_what_is_still_listed() {
        let mut selected: BTreeSet<i64> = [1, 2, 3].into_iter().collect();
        assert!(!retain_visible(&mut selected, &[1, 2, 3, 4]));
        assert!(retain_visible(&mut selected, &[2, 3, 9]));
        assert_eq!(selected, [2, 3].into_iter().collect());
        assert!(retain_visible(&mut selected, &[]));
        assert!(selected.is_empty());
    }

    #[test]
    fn eligibility_is_decided_per_action() {
        let mut task = Task::standalone();
        task.title = "A".into();
        task.due_date = "2026-11-14".into();
        task.assignee = "Servidor A".into();
        assert_eq!(eligibility(&Change::Conclude, &task, false), Ok(()));
        assert_eq!(
            eligibility(&Change::Conclude, &task, true),
            Err(Reason::Blocked)
        );
        assert_eq!(
            eligibility(&Change::Due("2026-11-14".into()), &task, false),
            Err(Reason::Same)
        );
        assert_eq!(
            eligibility(&Change::Due(String::new()), &task, false),
            Ok(())
        );
        assert_eq!(
            eligibility(&Change::Assignee(" Servidor A ".into()), &task, false),
            Err(Reason::Same)
        );
        assert_eq!(
            eligibility(&Change::Priority("normal".into()), &task, false),
            Err(Reason::Same)
        );
        // A blocked task can still be re-dated, and anything can be deleted.
        assert_eq!(
            eligibility(&Change::Due("2026-12-01".into()), &task, true),
            Ok(())
        );
        assert_eq!(eligibility(&Change::Trash, &task, true), Ok(()));
        task.status = "concluida".into();
        assert_eq!(
            eligibility(&Change::Conclude, &task, false),
            Err(Reason::AlreadyDone)
        );
        assert_eq!(
            eligibility(&Change::Priority("alta".into()), &task, false),
            Ok(())
        );
    }

    #[test]
    fn the_result_line_names_what_was_done_and_what_was_not() {
        let skip = |id, title: &str, reason| Skip {
            id,
            title: title.into(),
            reason,
        };
        let out = Outcome {
            done: 4,
            skipped: vec![skip(1, "Responder o ofício", Reason::Blocked)],
            ..Default::default()
        };
        assert_eq!(
            message(Some(&Change::Conclude), false, &out),
            "4 tarefas concluídas.\n1 não pôde ser concluída: «Responder o ofício» depende de uma tarefa em aberto."
        );
        let out = Outcome {
            done: 1,
            skipped: vec![
                skip(1, "A", Reason::AlreadyDone),
                skip(2, "B", Reason::AlreadyDone),
                skip(3, "C", Reason::AlreadyDone),
                skip(4, "D", Reason::AlreadyDone),
                skip(5, "E", Reason::Blocked),
                skip(6, "F", Reason::Changed),
            ],
            notes: vec!["Nota do sistema.".into()],
            ..Default::default()
        };
        assert_eq!(
            message(Some(&Change::Conclude), false, &out),
            "1 tarefa concluída.\n\
             4 não puderam ser concluídas: «A», «B», «C» e mais 1 já estão concluídas.\n\
             1 não pôde ser concluída: «E» depende de uma tarefa em aberto.\n\
             1 não pôde ser concluída: «F» foi alterada depois da seleção.\n\
             • Nota do sistema."
        );
        let out = Outcome {
            done: 2,
            skipped: vec![skip(1, "A", Reason::Same), skip(2, "B", Reason::Same)],
            ..Default::default()
        };
        assert_eq!(
            message(Some(&Change::Due("2026-11-14".into())), false, &out),
            "2 tarefas com o prazo mudado para 14/11/2026.\n2 não foram alteradas: «A» e «B» já têm esse prazo."
        );
        let out = Outcome {
            done: 1,
            ..Default::default()
        };
        assert_eq!(
            message(Some(&Change::Trash), false, &out),
            "1 tarefa excluída. Ela está na Lixeira (Ajustes ▸ Lixeira)."
        );
        assert_eq!(
            message(Some(&Change::Assignee(String::new())), false, &out),
            "1 tarefa ficou sem responsável."
        );
        assert_eq!(message(None, true, &out), "1 tarefa restaurada da Lixeira.");
        assert_eq!(
            message(Some(&Change::Conclude), false, &Outcome::default()),
            "Nada foi alterado."
        );
    }

    #[test]
    fn concluding_skips_what_a_dependency_refuses_and_keeps_going() {
        let (_dir, mut store) = store();
        let free = add(&mut store, "Livre", &[]);
        let done = add(&mut store, "Já feita", &[]);
        store.complete_task(done).unwrap();
        let first = add(&mut store, "Primeira", &[]);
        let second = add(&mut store, "Segunda", &[first]);
        let waiting_for = add(&mut store, "Espera fora", &[]);
        let blocked = add(&mut store, "Depende de fora", &[waiting_for]);
        let snap = pick(&store, &[free, done, second, first, blocked]);
        let out = apply_batch(&mut store, &snap, &Change::Conclude);
        // `second` may come before `first`: it is concluded on a later pass.
        assert_eq!(out.done, 3);
        let reasons: Vec<(i64, &Reason)> = out.skipped.iter().map(|s| (s.id, &s.reason)).collect();
        assert!(reasons.contains(&(done, &Reason::AlreadyDone)));
        assert!(reasons.contains(&(blocked, &Reason::Blocked)));
        assert_eq!(out.skipped.len(), 2);
        for id in [free, first, second] {
            assert_eq!(get(&store, id).status, "concluida");
        }
        assert_eq!(get(&store, blocked).status, "pendente");
        assert_eq!(get(&store, waiting_for).status, "pendente");
    }

    #[test]
    fn a_task_changed_after_the_confirmation_is_refused_not_overwritten() {
        let (_dir, mut store) = store();
        let a = add(&mut store, "A", &[]);
        let b = add(&mut store, "B", &[]);
        let c = add(&mut store, "C", &[]);
        let snap = pick(&store, &[a, b, c]);
        touch(&mut store, b);
        let out = apply_batch(&mut store, &snap, &Change::Due("2026-11-14".into()));
        assert_eq!(out.done, 2);
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(
            (out.skipped[0].id, &out.skipped[0].reason),
            (b, &Reason::Changed)
        );
        assert_eq!(get(&store, a).due_date, "2026-11-14");
        assert_eq!(get(&store, c).due_date, "2026-11-14");
        assert_eq!(get(&store, b).due_date, "");
        assert_eq!(get(&store, b).notes, "outra edição");
    }

    #[test]
    fn priority_and_assignee_apply_to_standalone_tasks_too() {
        let (_dir, mut store) = store();
        store
            .import_capture(&crate::model::sample_capture())
            .unwrap();
        let loose = add(&mut store, "Avulsa", &[]);
        let linked = store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.document_id.is_some())
            .unwrap()
            .id;
        let snap = pick(&store, &[loose, linked]);
        let out = apply_batch(&mut store, &snap, &Change::Priority("urgente".into()));
        assert_eq!((out.done, out.skipped.len()), (2, 0));
        let snap = pick(&store, &[loose, linked]);
        let out = apply_batch(&mut store, &snap, &Change::Priority("urgente".into()));
        assert_eq!((out.done, out.skipped.len()), (0, 2), "nothing to change");
        let snap = pick(&store, &[loose, linked]);
        let out = apply_batch(&mut store, &snap, &Change::Assignee("  Servidor B ".into()));
        assert_eq!(out.done, 2);
        for id in [loose, linked] {
            let task = get(&store, id);
            assert_eq!(task.priority, "urgente");
            assert_eq!(task.assignee, "Servidor B");
        }
        assert_eq!(get(&store, loose).document_id, None);
    }

    #[test]
    fn undo_puts_back_only_what_was_touched_and_skips_what_changed_since() {
        let (_dir, mut store) = store();
        let a = add(&mut store, "A", &[]);
        let b = add(&mut store, "B", &[]);
        let mut task = get(&store, a);
        task.due_date = "2026-11-20".into();
        task.due_source = "Ofício 12".into();
        task.assignee = "Servidor A".into();
        let version = store.item_version("task", a).unwrap().version;
        store.save_task_checked(&task, Some(version)).unwrap();
        let (before_a, before_b) = (get(&store, a), get(&store, b));

        // Due date: the date and its origin go and come back; nothing else.
        let snap = pick(&store, &[a, b]);
        let out = apply_batch(&mut store, &snap, &Change::Due("2026-11-14".into()));
        assert_eq!(out.done, 2);
        assert_eq!(get(&store, a).due_source, "");
        let undone = undo_batch(&mut store, &out.undo);
        assert_eq!((undone.done, undone.skipped.len()), (2, 0));
        assert_eq!(get(&store, a), before_a);
        assert_eq!(get(&store, b), before_b);

        // Conclusion: status and completed_at go back to what they were.
        let snap = pick(&store, &[a, b]);
        let out = apply_batch(&mut store, &snap, &Change::Conclude);
        assert_eq!(out.done, 2);
        assert!(get(&store, a).completed_at.is_some());
        let undone = undo_batch(&mut store, &out.undo);
        assert_eq!(undone.done, 2);
        assert_eq!(get(&store, a), before_a);

        // One task edited after the batch is left alone, the other returns.
        let snap = pick(&store, &[a, b]);
        let out = apply_batch(&mut store, &snap, &Change::Priority("alta".into()));
        touch(&mut store, a);
        let undone = undo_batch(&mut store, &out.undo);
        assert_eq!(undone.done, 1);
        assert_eq!(undone.skipped.len(), 1);
        assert_eq!(undone.skipped[0].reason, Reason::Changed);
        assert_eq!(get(&store, a).priority, "alta", "edited since: kept");
        assert_eq!(get(&store, b).priority, "normal");
        assert!(message(None, false, &undone).contains("alterada depois da ação em lote"));
    }

    #[test]
    fn deleting_sends_to_the_trash_dependents_first_and_can_be_undone() {
        let (_dir, mut store) = store();
        let first = add(&mut store, "Primeira", &[]);
        let second = add(&mut store, "Segunda", &[first]);
        let kept = add(&mut store, "Fica", &[]);
        let needed = add(&mut store, "Precisada", &[]);
        let _needs = add(&mut store, "Precisa", &[needed]);
        // `first` comes before the task that waits for it: a later pass.
        let snap = pick(&store, &[first, second, needed]);
        let out = apply_batch(&mut store, &snap, &Change::Trash);
        assert_eq!(out.done, 2);
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].id, needed);
        assert!(matches!(out.skipped[0].reason, Reason::Refused(_)));
        let listed: Vec<i64> = store.list_tasks().unwrap().iter().map(|t| t.id).collect();
        assert!(!listed.contains(&first) && !listed.contains(&second));
        assert!(listed.contains(&kept) && listed.contains(&needed));
        let undone = undo_batch(&mut store, &out.undo);
        assert_eq!((undone.done, undone.skipped.len()), (2, 0));
        let listed: Vec<i64> = store.list_tasks().unwrap().iter().map(|t| t.id).collect();
        assert!(listed.contains(&first) && listed.contains(&second));
    }

    #[test]
    fn a_task_gone_before_applying_is_reported() {
        let (_dir, mut store) = store();
        let a = add(&mut store, "A", &[]);
        let b = add(&mut store, "B", &[]);
        let snap = pick(&store, &[a, b]);
        store
            .trash_item(crate::trash::Target {
                kind: crate::trash::Kind::Task,
                id: b,
            })
            .unwrap();
        let out = apply_batch(&mut store, &snap, &Change::Conclude);
        assert_eq!(out.done, 1);
        assert_eq!(
            (out.skipped[0].id, &out.skipped[0].reason),
            (b, &Reason::Missing)
        );
    }

    #[test]
    fn selection_mode_follows_the_form_guards() {
        let mut panel = TaskPanel::default();
        let mut open = Task::new(1);
        open.id = 10;
        assert!(panel.focus_task(&open));
        // A clean form is closed by entering the mode.
        assert!(panel.start_selection());
        assert!(panel.draft.is_none() && panel.selecting);
        panel.selected.insert(10);
        // Esc leaves the mode and forgets the selection.
        assert!(panel.key_escape());
        assert!(!panel.selecting && panel.selected.is_empty());
        assert!(!panel.key_escape(), "nothing left for Esc");
        // Unsaved edits, or a request waiting for the app, refuse it.
        assert!(panel.focus_task(&open));
        panel.draft.as_mut().unwrap().notes = "sem salvar".into();
        assert!(!panel.start_selection());
        panel.discard_draft();
        panel.close_form();
        panel.blocked = Some(Intent::New(None));
        assert!(!panel.start_selection());
        panel.blocked = None;
        // Opening or creating a task leaves the mode.
        assert!(panel.start_selection());
        assert!(panel.start_new(None));
        assert!(!panel.selecting);
        panel.close_form();
        assert!(panel.start_selection());
        panel.selected.insert(1);
        panel.toggle_selected(2);
        assert_eq!(panel.selected.len(), 2);
        assert!(panel.focus_task(&open));
        assert!(!panel.selecting && panel.selected.is_empty());
    }
}

#[cfg(test)]
mod batch_review_tests {
    use super::batch::*;
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("batch2.sqlite3")).unwrap();
        (dir, store)
    }

    fn add(store: &mut Store, title: &str) -> i64 {
        let mut task = Task::standalone();
        task.title = title.into();
        store.save_task(&task).unwrap()
    }

    fn get(store: &Store, id: i64) -> Task {
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    fn pick(store: &Store, ids: &[i64]) -> Snapshot {
        snapshot(
            store,
            &ids.iter().copied().collect(),
            &store.list_tasks().unwrap(),
        )
    }

    fn touch(store: &mut Store, id: i64) {
        let mut task = get(store, id);
        task.notes = "outra edição".into();
        let version = store.item_version("task", id).unwrap().version;
        store.save_task_checked(&task, Some(version)).unwrap();
    }

    fn capture(number: &str, text: &str) -> crate::model::Capture {
        let mut c = crate::model::sample_capture();
        c.document_number = number.into();
        c.source_url = format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibir?sigla={}-A",
            number.replace('/', "%2F")
        );
        c.title = format!("Documento {number}");
        c.text = text.into();
        c
    }

    #[test]
    fn a_selected_task_concluded_by_the_cascade_of_the_batch_is_done_and_undone() {
        let (_dir, mut store) = store();
        let parent = store
            .import_capture(&capture("ORG-DIC-2026/97546", "Processo principal."))
            .unwrap();
        let child = store
            .import_capture(&capture(
                "ORG-DES-2026/39019",
                "| Juntada | Juntado ao documento: ORG-DIC-2026/97546-A |",
            ))
            .unwrap();
        let of = |store: &Store, document: i64| {
            store
                .list_tasks()
                .unwrap()
                .into_iter()
                .find(|t| t.document_id == Some(document))
                .unwrap()
                .id
        };
        let (p, c) = (
            of(&store, parent.document_id),
            of(&store, child.document_id),
        );
        let before_child = get(&store, c);
        // The parent first: concluding it concludes the child by itself.
        let snap = pick(&store, &[p, c]);
        let out = apply_batch(&mut store, &snap, &Change::Conclude);
        assert_eq!(
            out.done,
            2,
            "{:?}",
            out.skipped.iter().map(|s| &s.reason).collect::<Vec<_>>()
        );
        assert!(out.skipped.is_empty());
        assert_eq!(get(&store, p).status, "concluida");
        assert_eq!(get(&store, c).status, "concluida");
        assert_eq!(out.undo.iter().filter(|u| u.id == c).count(), 1);
        // The undo reopens both, the child as it was before.
        let undone = undo_batch(&mut store, &out.undo);
        assert_eq!((undone.done, undone.skipped.len()), (2, 0));
        assert_eq!(get(&store, p).status, "pendente");
        assert_eq!(get(&store, c), before_child);
    }

    #[test]
    fn a_write_after_the_version_was_confirmed_is_never_adopted() {
        let (_dir, mut store) = store();
        let a = add(&mut store, "A");
        let b = add(&mut store, "B");
        let expected_a = store.item_version("task", a).unwrap().version;
        let expected_b = store.item_version("task", b).unwrap().version;
        let (task_a, task_b) = (get(&store, a), get(&store, b));
        touch(&mut store, a);
        touch(&mut store, b);
        // A task read before a late write (as a retry pass would hold it).
        for change in [
            Change::Conclude,
            Change::Priority("alta".into()),
            Change::Trash,
        ] {
            let result = apply_one(&mut store, &task_a, expected_a, false, &change);
            assert_eq!(result.err(), Some(Reason::Changed), "{change:?}");
        }
        assert_eq!(get(&store, a).status, "pendente");
        assert_eq!(get(&store, a).priority, "normal");
        assert_eq!(
            get(&store, a).notes,
            "outra edição",
            "the late edit is kept"
        );
        // Deleting too: still listed, nothing in the Lixeira.
        assert!(store.list_tasks().unwrap().iter().any(|t| t.id == a));
        // With the version that was confirmed and still current it goes.
        let current = store.item_version("task", b).unwrap().version;
        assert_ne!(current, expected_b);
        let fresh_b = get(&store, b);
        assert!(apply_one(&mut store, &fresh_b, current, false, &Change::Trash).is_ok());
        let _ = task_b;
    }

    #[test]
    fn undo_waits_for_a_clean_form_and_deleting_waits_for_the_job() {
        let (_dir, mut store) = store();
        let a = add(&mut store, "A");
        let b = add(&mut store, "B");
        let mut panel = TaskPanel::default();
        let snap = pick(&store, &[a, b]);
        panel.run_batch(&mut store, &snap, Change::Priority("alta".into()));
        assert!(panel.batch_undo.is_some());
        // An open form with unsaved edits: the undo stays and says why.
        let open = get(&store, a);
        assert!(panel.focus_task(&open));
        panel.draft.as_mut().unwrap().notes = "sem salvar".into();
        panel.run_undo(&mut store);
        assert!(panel.batch_undo.is_some(), "kept");
        assert_eq!(
            panel.message,
            "Salve ou cancele a tarefa aberta antes de desfazer."
        );
        assert_eq!(get(&store, b).priority, "alta");
        panel.discard_draft();
        panel.close_form();
        panel.run_undo(&mut store);
        assert!(panel.batch_undo.is_none());
        assert_eq!(get(&store, b).priority, "normal");
        // Deleting while a job runs: nothing is deleted, the question stays.
        panel.set_busy(true);
        let snap = pick(&store, &[a, b]);
        panel.run_batch(&mut store, &snap, Change::Trash);
        assert_eq!(store.list_tasks().unwrap().len(), 2);
        assert!(panel.batch_ask.is_some());
        panel.set_busy(false);
        let ask = panel.batch_ask.take().unwrap();
        panel.run_batch(&mut store, &ask.snapshot, Change::Trash);
        assert_eq!(store.list_tasks().unwrap().len(), 0);
    }
}
