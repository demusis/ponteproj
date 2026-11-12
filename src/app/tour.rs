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
        "busca-sala",
        "busca-projetor",
        "hoje-calendario",
        "documentos-inicio",
        "documentos-filtros",
        "ficha-resumo",
        "ficha-recursos",
        "ficha-documento",
        "ficha-notas",
        "ficha-mais",
        "ficha-manual",
        "ficha-email",
        "ficha-whatsapp",
        "ficha-juntada",
        "ficha-sem-recursos",
        "ficha-propostas",
        "ficha-propostas-reservas",
        "ficha-propostas-conversa",
        "ficha-propostas-antiga",
        "proposta-tarefa",
        "proposta-completar-atendimento",
        "proposta-atendimento-novo",
        "proposta-reserva",
        "proposta-reserva-fim",
        "proposta-cadastrar-recurso",
        "envio-confirmacao",
        "fila-trabalhos",
        "analise-material",
        "ajustes-ia-teste",
        "ficha-texto-largo",
        "aviso-documento-sumiu",
        "tarefas",
        "tarefa-aberta",
        "tarefa-com-recurso",
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

    /// The first document of the sample that `wanted` accepts.
    fn tour_find(&self, wanted: impl Fn(&crate::model::DocumentSummary) -> bool) -> Option<i64> {
        self.store
            .list_documents("")
            .ok()?
            .into_iter()
            .find(|d| wanted(d))
            .map(|d| d.id)
    }

    /// Does what a click on a button of the Propostas block does, so the
    /// picture shows the form right after it.
    fn tour_click(&mut self, name: &str) {
        use super::proposals::ProposalAction;
        let chat = name == "proposta-atendimento-novo";
        let id = if chat {
            self.tour_find(|d| d.whatsapp)
        } else {
            self.tour_document()
        };
        let Some(id) = id else {
            return;
        };
        self.select(id);
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let Some(proposals) = detail.analysis.as_ref().and_then(|a| a.proposals.clone()) else {
            return;
        };
        let reservation = |index: usize, view: &Option<super::proposals::ProposalView>| {
            let proposal = proposals.reservations.get(index).cloned()?;
            let uid = view
                .as_ref()
                .and_then(|v| v.reservations.get(&proposal.id))
                .and_then(|check| check.resource_uid.clone());
            Some((proposal, uid))
        };
        let action = match name {
            "proposta-tarefa" => proposals
                .tasks
                .get(1)
                .cloned()
                .map(ProposalAction::CreateTask),
            "proposta-completar-atendimento" => {
                let attendance = self
                    .proposal_view
                    .as_ref()
                    .and_then(|v| v.attendance.as_ref().map(|a| a.id));
                attendance
                    .zip(proposals.attendance.clone())
                    .map(|(id, proposal)| ProposalAction::CompleteAttendance { id, proposal })
            }
            "proposta-atendimento-novo" => proposals
                .attendance
                .clone()
                .map(ProposalAction::NewAttendance),
            "proposta-reserva" => {
                reservation(0, &self.proposal_view).map(|(proposal, resource_uid)| {
                    ProposalAction::Reserve {
                        proposal,
                        resource_uid,
                    }
                })
            }
            "proposta-reserva-fim" => {
                reservation(0, &self.proposal_view).map(|(mut proposal, resource_uid)| {
                    proposal.end = None;
                    ProposalAction::Reserve {
                        proposal,
                        resource_uid,
                    }
                })
            }
            _ => reservation(2, &self.proposal_view)
                .map(|(proposal, _)| ProposalAction::RegisterResource(proposal.what)),
        };
        if let Some(action) = action {
            self.run_proposal_action(&detail, action);
        }
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
        self.shots_ficha_scroll = 0.0;
        self.worker = None;
        self.queued = None;
        self.send_confirm = None;
        self.analysis_material = None;
        self.ai_test_report = None;
        self.tasks = Default::default();
        self.saiop = Default::default();
        self.resources = Default::default();
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
            "busca" | "busca-tarefas" | "busca-sala" | "busca-projetor" => {
                self.refresh();
                self.area = Area::Today;
                self.global_search.shots_open(match name {
                    "busca" => "or",
                    "busca-sala" => "sala",
                    "busca-projetor" => "projetor",
                    _ => "veículos",
                });
            }
            "ficha-propostas"
            | "ficha-propostas-reservas"
            | "ficha-propostas-conversa"
            | "ficha-propostas-antiga" => {
                let found = match name {
                    "ficha-propostas-conversa" => self.tour_find(|d| d.whatsapp),
                    "ficha-propostas-antiga" => {
                        self.tour_find(|d| d.document_number == "ORG-OFI-2026/00398")
                    }
                    _ => document,
                };
                if let Some(id) = found {
                    self.select(id);
                }
                self.tab = Tab::Overview;
                self.shots_ficha_scroll = match name {
                    "ficha-propostas" => 330.0,
                    "ficha-propostas-reservas" => 800.0,
                    "ficha-propostas-conversa" => 260.0,
                    _ => 200.0,
                };
            }
            "proposta-tarefa"
            | "proposta-completar-atendimento"
            | "proposta-atendimento-novo"
            | "proposta-reserva"
            | "proposta-reserva-fim"
            | "proposta-cadastrar-recurso" => self.tour_click(name),
            "envio-confirmacao" => {
                if let Some(id) = document {
                    self.select(id);
                }
                self.tab = Tab::Overview;
                self.shots_ficha_scroll = 330.0;
                self.send_confirm = Some(super::queue::SendConfirm {
                    what: super::queue::PendingSend::Analysis,
                    document_id: document.unwrap_or_default(),
                    number: "ORG-OFI-2026/00431".to_owned(),
                    provider: "Ollama Cloud".to_owned(),
                    destination: "https://ollama.com".to_owned(),
                    model: "gpt-oss:120b".to_owned(),
                    sources: 2,
                });
            }
            "fila-trabalhos" => {
                if let Some(id) = document {
                    self.select(id);
                }
                self.tab = Tab::Overview;
                self.shots_ficha_scroll = 330.0;
                // A job running (nothing will ever answer) and one waiting.
                let (sender, receiver) = std::sync::mpsc::channel::<super::WorkResult>();
                let (progress_sender, progress) = std::sync::mpsc::channel::<String>();
                std::mem::forget(sender);
                std::mem::forget(progress_sender);
                self.worker = Some(super::Worker {
                    receiver,
                    label: "Analisando ORG-MEM-2026/01207…".to_owned(),
                    kind: super::WorkKind::Analysis,
                    started: std::time::Instant::now(),
                    progress,
                    latest: "Etapa 1 de 2: lendo o documento…".to_owned(),
                    cancel: Default::default(),
                });
                self.queued = Some(super::queue::QueuedJob {
                    key: super::queue::QueueKey {
                        kind: super::WorkKind::Analysis,
                        document_id: document.unwrap_or_default(),
                    },
                    label: "Analisando ORG-OFI-2026/00431…".to_owned(),
                    waiting: "analisar ORG-OFI-2026/00431".to_owned(),
                    capture_id: 0,
                    context: String::new(),
                    task: Box::new(|_| super::WorkResult::AiTest(Err(String::new()))),
                    ctx: ctx.clone(),
                });
            }
            "analise-material" => {
                if let Some(id) = document {
                    self.select(id);
                    self.analysis_material = Some(id);
                }
                self.tab = Tab::Overview;
                self.shots_ficha_scroll = 330.0;
            }
            "ajustes-ia-teste" => {
                self.area = Area::Settings;
                self.settings_section = super::SettingsSection::Ai;
                self.shots_ficha_scroll = 900.0;
                let case = |name: &str, passed, total, failures: &[&str], calls, seconds| {
                    crate::ai_eval::CaseResult {
                        name: name.to_owned(),
                        passed,
                        total,
                        failures: failures.iter().map(|f| (*f).to_owned()).collect(),
                        calls,
                        seconds,
                    }
                };
                self.ai_test_report = Some(Ok(crate::ai_eval::score(vec![
                    case("Ofício com prazo", 14, 14, &[], 3, 4.2),
                    case(
                        "Pedido de sala por WhatsApp",
                        12,
                        14,
                        &[
                            "A data da reunião não foi encontrada no trecho citado.",
                            "A IA propôs uma tarefa que o documento não pede.",
                        ],
                        4,
                        6.8,
                    ),
                    case("Documento sem nada a propor", 5, 5, &[], 2, 2.9),
                ])));
            }
            "ficha-texto-largo" => {
                let manual = self.tour_find(|d| d.manual && !d.email && !d.whatsapp);
                if let Some(id) = manual {
                    self.select(id);
                }
                self.tab = Tab::Document;
            }
            "aviso-documento-sumiu" => {
                // The document open in the window is not in the library any
                // more (deleted for good, or removed by a synchronization).
                self.area = Area::Documents;
                self.selected_id = Some(987_654);
                self.reload(987_654);
            }
            "ficha-sem-recursos" => {
                // A document no use points at.
                let used: std::collections::BTreeSet<i64> = self
                    .store
                    .list_resource_uses()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|u| u.document_id)
                    .collect();
                let free = self
                    .store
                    .list_documents("")
                    .unwrap_or_default()
                    .into_iter()
                    .find(|d| !used.contains(&d.id) && !d.manual)
                    .map(|d| d.id);
                if let Some(id) = free {
                    self.area = Area::Documents;
                    self.select(id);
                }
                self.tab = Tab::Overview;
                self.shots_ficha_scroll = 420.0;
            }
            "ficha-recursos" => {
                // The Resumo scrolled down to "Recursos usados".
                if let Some(id) = document {
                    self.select(id);
                }
                self.tab = Tab::Overview;
                self.shots_ficha_scroll = 900.0;
            }
            "tarefa-com-recurso" => {
                self.tasks = Default::default();
                let linked: std::collections::BTreeSet<i64> = self
                    .store
                    .list_resource_uses()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|u| u.task_id)
                    .collect();
                let task = self
                    .store
                    .list_tasks()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|t| linked.contains(&t.id));
                if let Some(task) = task {
                    self.perform_leave(Leave::OpenTask(task));
                }
                self.area = Area::Tasks;
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
