//! Development only: the guided tour of screens taken by `--shots`.
use super::{Area, Leave, PonteApp, Tab};
use eframe::egui;

/// Development only: the screens pictured by `--shots`, in order. Each step
/// sets everything it needs, so any of them can be taken alone.
#[cfg(feature = "shots")]
impl PonteApp {
    const TOUR: &'static [&'static str] = &[
        "hoje",
        "documentos-inicio",
        "documentos-filtros",
        "ficha-resumo",
        "ficha-documento",
        "ficha-notas",
        "ficha-mais",
        "ficha-manual",
        "ficha-email",
        "ficha-juntada",
        "tarefas",
        "tarefa-aberta",
        "atendimentos",
        "atendimento-aberto",
        "modelos",
        "ajustes",
        "ajustes-copia",
        "ajustes-aparencia",
        "ajustes-importacao",
        "janela-rede",
        "janela-ia",
        "janela-email",
        "janela-lixeira",
        "janela-registro-manual",
        "janela-sobre",
        "janela-consulta",
    ];

    /// The screens of the app itself, then the views each panel declares
    /// (`SHOTS` in its own file), then everything again in the dark theme.
    pub(super) fn tour_names() -> Vec<String> {
        let mut names: Vec<String> = Self::TOUR.iter().map(|name| (*name).to_owned()).collect();
        let panels: [(&str, &[&str]); 5] = [
            ("tarefas", crate::tasks_ui::SHOTS),
            ("atendimentos", crate::saiop::SHOTS),
            ("modelos", crate::templates_ui::SHOTS),
            ("email", crate::mail_ui::SHOTS),
            ("rede", crate::intranet::SHOTS),
        ];
        for (prefix, views) in panels {
            names.extend(views.iter().map(|view| format!("{prefix}-{view}")));
        }
        let dark: Vec<String> = names.iter().map(|name| format!("escuro-{name}")).collect();
        names.extend(dark);
        names
    }

    pub(super) fn tour_frame(&mut self, ctx: &egui::Context) {
        let Some(mut tour) = self.tour.take() else {
            return;
        };
        ctx.request_repaint();
        if let crate::shots::Tick::Begin(index) = tour.tick(ctx) {
            let names = Self::tour_names();
            match names.get(index).map(String::as_str) {
                None => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                Some(name) if crate::shots::Tour::wanted(name) => {
                    self.tour_setup(name, ctx);
                    tour.named(index, name);
                }
                Some(_) => tour.skip(index),
            }
        }
        self.tour = Some(tour);
    }

    pub(super) fn tour_document(&self) -> Option<i64> {
        self.store
            .list_documents("")
            .ok()?
            .into_iter()
            .find(|d| d.document_number == "ORG-OFI-2026/00431")
            .map(|d| d.id)
    }

    pub(super) fn tour_setup(&mut self, name: &str, ctx: &egui::Context) {
        // Nothing left over from the step before.
        self.intranet.open = false;
        self.settings_open = false;
        self.mail.open = false;
        self.trash.open = false;
        self.manual_form.open = false;
        self.about_open = false;
        self.consult = None;
        self.filters_open = false;
        self.notice = None;
        let dark = name.starts_with("escuro-");
        crate::theme::set_appearance(
            ctx,
            if dark {
                crate::theme::Appearance::Dark
            } else {
                crate::theme::Appearance::Light
            },
        );
        let name = name.strip_prefix("escuro-").unwrap_or(name);
        let document = self.tour_document();
        match name {
            "hoje" => {
                self.refresh();
                self.area = Area::Today;
            }
            "documentos-inicio" => {
                self.area = Area::Documents;
                self.selected_id = None;
                self.detail = None;
            }
            "documentos-filtros" => {
                self.area = Area::Documents;
                self.selected_id = None;
                self.detail = None;
                self.filters_open = true;
            }
            "ficha-manual" | "ficha-email" | "ficha-juntada" => {
                let found = self
                    .store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| match name {
                        "ficha-manual" => d.manual && !d.email,
                        "ficha-email" => d.email,
                        _ => d.joined_children_count > 0,
                    })
                    .map(|d| d.id);
                if let Some(id) = found {
                    self.area = Area::Documents;
                    self.select(id);
                }
                self.tab = if name == "ficha-juntada" {
                    Tab::Overview
                } else {
                    Tab::Document
                };
            }
            "janela-consulta" => {
                self.area = Area::Documents;
                if let Some(id) = document {
                    self.open_consult(id);
                }
            }
            "ficha-resumo" | "ficha-documento" | "ficha-notas" | "ficha-mais" => {
                if let Some(id) = document {
                    self.select(id);
                }
                self.tab = match name {
                    "ficha-documento" => Tab::Document,
                    "ficha-notas" => Tab::Notes,
                    "ficha-mais" => Tab::More,
                    _ => Tab::Overview,
                };
            }
            "tarefas" => {
                self.tasks = Default::default();
                self.area = Area::Tasks;
            }
            "tarefa-aberta" => {
                self.tasks = Default::default();
                let task = self
                    .store
                    .list_tasks()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|t| t.overdue());
                if let Some(task) = task {
                    self.perform_leave(Leave::OpenTask(task));
                }
                self.area = Area::Tasks;
            }
            "atendimentos" => {
                self.saiop = Default::default();
                self.area = Area::Saiop;
            }
            "atendimento-aberto" => {
                self.saiop = Default::default();
                let first = self
                    .store
                    .list_saiop_attendances()
                    .unwrap_or_default()
                    .first()
                    .map(|a| a.id);
                if let Some(id) = first {
                    self.saiop.focus_attendance(&self.store, id);
                }
                self.area = Area::Saiop;
            }
            "modelos" => self.area = Area::Templates,
            "ajustes" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Team;
            }
            "ajustes-copia" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Backup;
            }
            "ajustes-aparencia" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Appearance;
            }
            "ajustes-importacao" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Import;
            }
            "janela-rede" => {
                self.area = Area::Settings;
                self.intranet.open = true;
            }
            "janela-ia" => {
                self.area = Area::Settings;
                self.open_ai_settings(ctx);
            }
            "janela-email" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Mail;
            }
            "janela-lixeira" => {
                self.area = Area::Settings;
                self.trash.open = true;
            }
            "janela-registro-manual" => {
                self.area = Area::Documents;
                self.manual_form.open_new();
            }
            "janela-sobre" => {
                self.area = Area::Settings;
                self.about_open = true;
            }
            other => {
                // Views declared by the panels themselves.
                if let Some(view) = other.strip_prefix("tarefas-") {
                    self.tasks = Default::default();
                    self.area = Area::Tasks;
                    self.tasks.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("atendimentos-") {
                    self.saiop = Default::default();
                    self.area = Area::Saiop;
                    self.saiop.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("modelos-") {
                    self.templates = Default::default();
                    self.area = Area::Templates;
                    self.templates.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("email-") {
                    self.mail = Default::default();
                    self.area = Area::Settings;
                    self.settings_section = super::SettingsSection::Mail;
                    self.mail.shots_open(&mut self.store, view);
                } else if let Some(view) = other.strip_prefix("rede-") {
                    self.area = Area::Settings;
                    self.intranet.shots_open(&mut self.store, view);
                }
            }
        }
    }
}
