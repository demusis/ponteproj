//! Explicit finite repetition, with a stable preview held until commit.
use super::PonteApp;
use crate::{
    recurrence::{Frequency, Recurrence},
    resources::ResourceUse,
    tasks::Task,
    widgets,
};
use eframe::egui;

enum Preview {
    Tasks(Vec<Task>),
    Uses(Vec<ResourceUse>),
}

#[derive(Default)]
pub(super) struct Repetition {
    resource: bool,
    task_id: i64,
    use_uid: String,
    rule: Recurrence,
    preview: Option<Preview>,
}

fn repetition_message(created: usize, total: usize) -> String {
    if created == 0 {
        "Todas as ocorrências já existiam; nada foi criado.".into()
    } else {
        format!(
            "{created} novas ocorrências criadas; {} já existiam.",
            total - created
        )
    }
}

impl PonteApp {
    pub(super) fn repetition_card(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Repetir tarefa ou reserva…", |ui| {
            widgets::muted(ui, "Crie ocorrências diárias, semanais, mensais ou anuais. A original permanece intacta; cada cópia pode ser alterada ou cancelada individualmente.");
            let mut changed = false;
            let mut prepare = false;
            let mut commit = false;
            let state = &mut self.repetition;
            ui.horizontal(|ui| {
                changed |= ui.selectable_value(&mut state.resource, false, "Tarefa").changed();
                changed |= ui.selectable_value(&mut state.resource, true, "Reserva").changed();
            });
            if state.resource {
                let selected = self.today.planning_uses.iter().find(|u| u.uid == state.use_uid).map_or("Escolher reserva", |u| u.purpose.as_str());
                egui::ComboBox::from_id_salt("repeat-use").selected_text(selected).show_ui(ui, |ui| {
                    for item in self.today.planning_uses.iter().filter(|u| u.status.occupying()) {
                        changed |= ui.selectable_value(&mut state.use_uid, item.uid.clone(), format!("{} · {}", item.start, item.purpose)).changed();
                    }
                });
            } else {
                let selected = self.today.planning_tasks.iter().find(|t| t.id == state.task_id).map_or("Escolher tarefa", |t| t.title.as_str());
                egui::ComboBox::from_id_salt("repeat-task").selected_text(selected).show_ui(ui, |ui| {
                    for task in &self.today.planning_tasks {
                        changed |= ui.selectable_value(&mut state.task_id, task.id, &task.title).changed();
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("repeat-frequency").selected_text(state.rule.frequency.label()).show_ui(ui, |ui| {
                    for frequency in Frequency::ALL {
                        changed |= ui.selectable_value(&mut state.rule.frequency, frequency, frequency.label()).changed();
                    }
                });
                ui.label("A cada");
                changed |= ui.add(egui::DragValue::new(&mut state.rule.interval).range(1..=state.rule.frequency.max_interval())).changed();
                ui.label("Ocorrências (inclui a original)");
                changed |= ui.add(egui::DragValue::new(&mut state.rule.count).range(2..=104)).changed();
            });
            if changed { state.preview = None; }
            if ui.button("Conferir próximas ocorrências").clicked() { prepare = true; }
            if let Some(preview) = &state.preview {
                let labels: Vec<_> = match preview {
                    Preview::Tasks(tasks) => tasks.iter().map(|t| format!("{} · prazo {} · {}–{}", t.title, crate::labels::date(&t.due_date), t.scheduled_start, t.scheduled_end)).collect(),
                    Preview::Uses(uses) => uses.iter().map(|u| format!("{} · {}–{}", u.purpose, u.start, u.end)).collect(),
                };
                egui::ScrollArea::vertical().id_salt("repeat-preview").max_height(200.0).show(ui, |ui| { for label in &labels { ui.label(label); } });
                widgets::small_muted(ui, "Dependências e vínculo com tarefa principal não são copiados. Reservas são verificadas novamente ao salvar; um conflito impede o lote inteiro.");
                commit = ui.add_enabled(!labels.is_empty(), widgets::primary(format!("Salvar {} ocorrências", labels.len()))).clicked();
            }
            if prepare {
                let result = if state.resource {
                    self.today.planning_uses.iter().find(|u| u.uid == state.use_uid)
                        .ok_or_else(|| anyhow::anyhow!("Escolha uma reserva."))
                        .and_then(|item| crate::recurrence::resource_occurrences(item, &state.rule)).map(Preview::Uses)
                } else {
                    self.today.planning_tasks.iter().find(|t| t.id == state.task_id)
                        .ok_or_else(|| anyhow::anyhow!("Escolha uma tarefa."))
                        .and_then(|task| crate::recurrence::task_occurrences(task, &state.rule)).map(Preview::Tasks)
                };
                match result { Ok(preview) => state.preview = Some(preview), Err(error) => self.error(error.to_string()) }
            }
            if commit {
                let result = match &self.repetition.preview {
                    Some(Preview::Tasks(tasks)) => self.store.save_repeated_tasks_counted(tasks).map(|(ids, created)| (created, ids.len())),
                    Some(Preview::Uses(uses)) => self.store.save_repeated_uses_counted(uses).map(|(ids, created)| (created, ids.len())),
                    None => return,
                };
                match result {
                    Ok((created, total)) => { self.repetition.preview = None; self.refresh(); self.message(repetition_message(created, total)); }
                    Err(error) => self.error(format!("Nenhuma nova ocorrência foi criada: {}", crate::resources::strip_marker(&error.to_string()))),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::repetition_message;
    #[test]
    fn repetition_messages_report_created_and_existing() {
        assert_eq!(
            repetition_message(2, 3),
            "2 novas ocorrências criadas; 1 já existiam."
        );
        assert_eq!(
            repetition_message(3, 3),
            "3 novas ocorrências criadas; 0 já existiam."
        );
        assert_eq!(
            repetition_message(0, 3),
            "Todas as ocorrências já existiam; nada foi criado."
        );
    }
}
