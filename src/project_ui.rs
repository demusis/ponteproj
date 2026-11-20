//! Projects on the task screens: the chip on a task row, the picker of the
//! task form and the filter of the list of tasks. The area "Projetos" itself
//! is `app::projects`; the rules are in `projects`.
use crate::projects::{Project, ProjectStatus};
use crate::theme::{self, c, icon, S1};
use crate::widgets;
use eframe::egui::{self, CornerRadius, Sense};

/// The name cut for a compact place; the hover has the whole name.
fn shorten(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_owned()
    } else {
        let cut: String = name.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    }
}

/// A compact label with the name of a project, for the line of a task row.
/// It is as wide as its text (a long name is cut), so it never pushes the
/// title out of its place: the row draws it where it draws the category chips.
pub fn project_chip(ui: &mut egui::Ui, project: &Project) -> egui::Response {
    let p = c();
    let galley = ui.painter().layout_no_wrap(
        format!("{} {}", icon::FOLDER, shorten(&project.name, 22)),
        theme::font_strong(12.0),
        p.primary,
    );
    let size = galley.size() + egui::vec2(16.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), p.primary_soft);
        ui.painter()
            .galley(rect.min + egui::vec2(8.0, 2.0), galley, p.primary);
    }
    response.on_hover_text(if project.status == ProjectStatus::Arquivado {
        format!("Projeto: {} (arquivado)", project.name)
    } else {
        format!("Projeto: {}", project.name)
    })
}

/// The projects the picker of the task form offers: the ones that are not
/// archived, plus the one the task is in now even when it is archived.
pub fn picker_projects<'a>(all: &'a [Project], current_uid: &str) -> Vec<&'a Project> {
    all.iter()
        .filter(|p| p.status != ProjectStatus::Arquivado || p.uid == current_uid)
        .collect()
}

/// The project a new subtask shows preselected: the one of its parent, unless
/// it already has one. (The store applies the same rule on save.)
pub fn inherited_project(current_uid: &str, parent_uid: Option<&str>) -> String {
    if !current_uid.is_empty() {
        current_uid.to_owned()
    } else {
        parent_uid.unwrap_or_default().to_owned()
    }
}

/// "Projeto": a list with "Sem projeto" and the projects. Returns true when
/// the choice changed. A uid that is not a project any more shows as none.
pub fn project_picker(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &mut String,
    all: &[Project],
) -> bool {
    let offered = picker_projects(all, current);
    let selected = all
        .iter()
        .find(|p| p.uid == *current)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Sem projeto".to_owned());
    let before = current.clone();
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected)
        .width(ui.available_width() - 24.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(current, String::new(), "Sem projeto");
            for project in offered {
                let label = if project.status == ProjectStatus::Arquivado {
                    format!("{} (arquivado)", project.name)
                } else {
                    project.name.clone()
                };
                ui.selectable_value(current, project.uid.clone(), label);
            }
        });
    *current != before
}

/// Which tasks the list shows by project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProjectFilter {
    /// No narrowing.
    #[default]
    Any,
    /// Only tasks that are in no project.
    Without,
    /// Only the tasks of this project (uid).
    Only(String),
}

/// No narrowing, for places that need a filter by reference.
pub static NO_PROJECT_FILTER: ProjectFilter = ProjectFilter::Any;

impl ProjectFilter {
    pub fn is_active(&self) -> bool {
        *self != ProjectFilter::Any
    }

    /// Whether a task with this project (empty: none) is listed.
    pub fn matches(&self, project_uid: &str) -> bool {
        match self {
            ProjectFilter::Any => true,
            ProjectFilter::Without => project_uid.is_empty(),
            ProjectFilter::Only(uid) => project_uid == uid,
        }
    }
}

/// The chip "Projeto" that opens the list to choose from; returns true when
/// the choice changed.
pub fn project_filter_button(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    filter: &mut ProjectFilter,
    all: &[Project],
) -> bool {
    let id = ui.make_persistent_id(("project-filter", id_salt));
    let mut changed = false;
    let label = match filter {
        ProjectFilter::Any => format!("Projeto {}", icon::DOWN),
        ProjectFilter::Without => format!("Sem projeto {}", icon::DOWN),
        ProjectFilter::Only(uid) => {
            let name = all
                .iter()
                .find(|p| p.uid == *uid)
                .map_or("projeto", |p| p.name.as_str());
            format!("Projeto · {} {}", shorten(name, 18), icon::DOWN)
        }
    };
    let button = widgets::chip(ui, filter.is_active(), label)
        .on_hover_text("Mostra só as tarefas de um projeto, ou as que não estão em nenhum.");
    egui::Popup::from_toggle_button_response(&button)
        .id(id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.set_width(240.0);
            let mut choose = |ui: &mut egui::Ui, value: ProjectFilter, text: String| {
                if ui.selectable_label(*filter == value, text).clicked() {
                    *filter = value;
                    changed = true;
                }
            };
            choose(ui, ProjectFilter::Any, "Todos os projetos".to_owned());
            choose(ui, ProjectFilter::Without, "Sem projeto".to_owned());
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .id_salt(id.with("scroll"))
                .show(ui, |ui| {
                    let (live, archived): (Vec<&Project>, Vec<&Project>) = all
                        .iter()
                        .partition(|p| p.status != ProjectStatus::Arquivado);
                    for project in live {
                        choose(
                            ui,
                            ProjectFilter::Only(project.uid.clone()),
                            project.name.clone(),
                        );
                    }
                    for project in archived {
                        choose(
                            ui,
                            ProjectFilter::Only(project.uid.clone()),
                            format!("{} (arquivado)", project.name),
                        );
                    }
                    if all.is_empty() {
                        ui.add_space(S1);
                        widgets::small_muted(ui, "Nenhum projeto ainda.");
                    }
                });
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(uid: &str, name: &str, status: ProjectStatus) -> Project {
        Project {
            uid: uid.into(),
            name: name.into(),
            status,
            ..Project::default()
        }
    }

    #[test]
    fn the_picker_offers_live_projects_and_the_current_one_even_if_archived() {
        let all = vec![
            project("a", "Alfa", ProjectStatus::Ativo),
            project("b", "Beta", ProjectStatus::Arquivado),
            project("c", "Gama", ProjectStatus::Concluido),
        ];
        let names = |current: &str| -> Vec<&str> {
            picker_projects(&all, current)
                .iter()
                .map(|p| p.name.as_str())
                .collect()
        };
        assert_eq!(names(""), ["Alfa", "Gama"], "archived ones are not offered");
        assert_eq!(
            names("b"),
            ["Alfa", "Beta", "Gama"],
            "unless it is the current one"
        );
        assert_eq!(names("zzz"), ["Alfa", "Gama"]);
    }

    #[test]
    fn a_new_subtask_takes_the_project_of_its_parent_unless_it_has_one() {
        assert_eq!(inherited_project("", Some("p1")), "p1");
        assert_eq!(inherited_project("mine", Some("p1")), "mine");
        assert_eq!(inherited_project("", None), "");
        assert_eq!(inherited_project("", Some("")), "");
    }

    #[test]
    fn the_filter_narrows_by_project_or_by_none() {
        let mut filter = ProjectFilter::default();
        assert!(!filter.is_active());
        assert!(filter.matches("") && filter.matches("a"));
        filter = ProjectFilter::Without;
        assert!(filter.is_active());
        assert!(filter.matches("") && !filter.matches("a"));
        filter = ProjectFilter::Only("a".into());
        assert!(filter.matches("a") && !filter.matches("b") && !filter.matches(""));
        assert!(NO_PROJECT_FILTER.matches("anything"));
    }

    #[test]
    fn a_long_name_is_cut_for_the_chip() {
        assert_eq!(shorten("Curto", 22), "Curto");
        let cut = shorten("Atualização do quadro de pessoal da coordenação", 22);
        assert!(cut.ends_with('…') && cut.chars().count() <= 22);
    }

    /// Draws the controls headless: nothing panics and the popup stays closed.
    #[test]
    fn the_controls_draw() {
        let ctx = egui::Context::default();
        let all = vec![
            project("a", "Alfa", ProjectStatus::Ativo),
            project("b", "Beta", ProjectStatus::Arquivado),
        ];
        let mut current = "b".to_owned();
        let mut filter = ProjectFilter::Only("a".into());
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            assert!(!project_picker(ui, "p", &mut current, &all));
            assert!(!project_filter_button(ui, "f", &mut filter, &all));
            project_chip(ui, &all[0]);
        });
        output.textures_delta.clear();
    }
}
