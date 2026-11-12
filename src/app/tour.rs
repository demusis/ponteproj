//! Development only: the guided tour of screens taken by `--shots`.
use super::{Area, Leave, PonteApp, Tab};
use eframe::egui;

/// Development only: the screens pictured by `--shots`, in order. Each step
/// sets everything it needs, so any of them can be taken alone.
#[cfg(feature = "shots")]
impl PonteApp {
    const TOUR: &'static [&'static str] = &[
        "hoje",
        "busca",
        "busca-tarefas",
        "hoje-calendario",
        "documentos-inicio",
        "documentos-filtros",
        "ficha-resumo",
        "ficha-documento",
        "ficha-notas",
        "ficha-mais",
        "ficha-manual",
        "ficha-email",
        "ficha-whatsapp",
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
        "ajustes-lembretes",
        "ajustes-sobre",
        "ajustes-sobre-versao-nova",
        "janela-rede",
        "janela-ia",
        "janela-email",
        "janela-lixeira",
        "ajustes-lixeira",
        "ajustes-lixeira-busca",
        "ajustes-lixeira-apagar",
        "janela-registro-manual",
        "janela-colar-whatsapp",
        "janela-sobre",
        "janela-consulta",
    ];

    /// The screens of the app itself, then the views each panel declares
    /// (`SHOTS` in its own file), then everything again in the dark theme.
    pub(super) fn tour_names() -> Vec<String> {
        let mut names: Vec<String> = Self::TOUR.iter().map(|name| (*name).to_owned()).collect();
        let panels: [(&str, &[&str]); 6] = [
            ("tarefas", crate::tasks_ui::SHOTS),
            ("atendimentos", crate::saiop::SHOTS),
            ("modelos", crate::templates_ui::SHOTS),
            ("recursos", crate::resources_ui::SHOTS),
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
        self.trash.shots_open("");
        self.manual_form.open = false;
        self.about_open = false;
        self.consult = None;
        self.filters_open = false;
        self.notice = None;
        self.global_search.open = false;
        self.update.last = None;
        super::today::shots_calendar(ctx, false);
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
            "busca" | "busca-tarefas" => {
                self.refresh();
                self.area = Area::Today;
                self.global_search
                    .shots_open(if name == "busca" { "or" } else { "veículos" });
            }
            "ajustes-sobre" | "ajustes-sobre-versao-nova" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::About;
                if name == "ajustes-sobre-versao-nova" {
                    self.update.last = Some(crate::update_check::Outcome::Newer {
                        latest: "9.9.9".to_owned(),
                        url: crate::update_check::FALLBACK_URL.to_owned(),
                        published: "01/01/2027".to_owned(),
                    });
                }
            }
            "hoje-calendario" => {
                self.refresh();
                self.area = Area::Today;
                super::today::shots_calendar(ctx, true);
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
            "ficha-manual" | "ficha-email" | "ficha-whatsapp" | "ficha-juntada" => {
                let found = self
                    .store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| match name {
                        "ficha-manual" => d.manual && !d.email && !d.whatsapp,
                        "ficha-email" => d.email,
                        "ficha-whatsapp" => d.whatsapp,
                        _ => d.joined_children_count > 0,
                    })
                    .map(|d| d.id);
                if let Some(id) = found {
                    self.area = Area::Documents;
                    self.select(id);
                }
                self.tab = if name == "ficha-juntada" || name == "ficha-whatsapp" {
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
            "ajustes-lembretes" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Reminders;
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
            "janela-lixeira"
            | "ajustes-lixeira"
            | "ajustes-lixeira-busca"
            | "ajustes-lixeira-apagar" => {
                if let Err(error) = crate::shots_data::conflicts_scenario(&mut self.store) {
                    eprintln!("amostra da lixeira: {error:#}");
                }
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Trash;
                self.trash.shots_open(match name {
                    "ajustes-lixeira" => "aviso",
                    "ajustes-lixeira-busca" => "busca",
                    "ajustes-lixeira-apagar" => "apagar",
                    _ => "",
                });
            }
            "janela-registro-manual" => {
                self.area = Area::Documents;
                self.manual_form.open_new();
            }
            "janela-colar-whatsapp" => {
                self.area = Area::Documents;
                self.manual_form.open_new();
                let mut step = super::PasteStep::new();
                step.chat = "Servidor B".into();
                step.text = "[09:12, 10/11/2026] Servidor B: Bom dia. A sala de reuniões do 2º andar está livre na quinta?
[09:15, 10/11/2026] Servidor A: Bom dia. Está, das 14h às 17h.
[09:20, 10/11/2026] Servidor A: Reservei. O documento é o ORG-OFI-2026/00431.
Se mudar o horário, me avise.
[09:21, 10/11/2026] Servidor B: Combinado, obrigado.".into();
                step.reread();
                self.manual_form.paste = Some(step);
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
                } else if let Some(view) = other.strip_prefix("recursos-") {
                    self.resources = Default::default();
                    self.refresh();
                    self.area = Area::Resources;
                    self.resources.shots_open(&mut self.store, view);
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
