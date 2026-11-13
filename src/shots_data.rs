//! A sample library for the pictures of the screens: invented documents,
//! tasks, atendimentos and models. Development only.
//!
//! Everything goes through the public routes of the `Store`, so the data is
//! exactly what the application would have stored. Dates are computed from
//! today, so the pictures always look current.
use crate::dossier::Movement;
use crate::manual::ManualFields;
use crate::model::{ActionSuggestion, Analysis, Capture, Coverage};
use crate::saiop::{SaiopAttendance, SaiopEvent};
use crate::storage::Store;
use crate::tasks::Task;
use crate::work_templates::{
    BlockMode, BlockRef, InstructionBlock, Reference, Scope, Section, Status, Variable,
    WorkTemplate,
};
use anyhow::{Context, Result};
use chrono::{Duration, Local, NaiveDate, Utc};
use std::collections::BTreeMap;

/// Days from today of the room asked for in the sample ofício (free at that
/// time) and of the deadline it gives for the vehicles.
const SAMPLE_ROOM_DAY: i64 = 4;
const SAMPLE_VEHICLES_DAY: i64 = 10;

/// Ids of what the first part created, for the parts that follow.
#[derive(Default)]
struct Ids {
    /// `ORG-OFI-2026/00431`
    logistics: i64,
    logistics_capture: i64,
    /// `ORG-MEM-2026/01207`
    staff: i64,
    /// `ORG-DES-2026/00877`
    dispatch: i64,
    /// `ORG-OFI-2026/00398`
    vehicles: i64,
    /// `ORG-OFI-2026/00377`
    schedule: i64,
    /// Manual record "Recibo de entrega de equipamentos".
    receipt: i64,
    /// Imported e-mail.
    mail: Option<i64>,
}

/// Fills an empty library with the sample content.
pub fn seed(store: &mut Store) -> Result<()> {
    // The automatic atendimento of each import would duplicate the ones made
    // by hand below; it is switched off while seeding and restored after.
    store
        .set_auto_attendance(false)
        .context("desligar o atendimento automático")?;
    let result = seed_all(store);
    let restore = store
        .set_auto_attendance(true)
        .context("religar o atendimento automático");
    result?;
    restore
}

fn seed_all(store: &mut Store) -> Result<()> {
    let mut ids = documents(store).context("documentos de exemplo")?;
    note(store, &ids).context("nota de exemplo")?;
    ids.mail = mail(store).context("e-mail de exemplo")?;
    whatsapp(store).context("conversa do WhatsApp de exemplo")?;
    tasks(store, &ids).context("tarefas de exemplo")?;
    attendances(store, &ids).context("atendimentos de exemplo")?;
    analysis(store, &ids).context("análise de exemplo")?;
    templates(store).context("modelos de exemplo")?;
    resources(store, &ids).context("recursos de exemplo")?;
    Ok(())
}

fn day(offset: i64) -> NaiveDate {
    Local::now().date_naive() + Duration::days(offset)
}

fn iso(offset: i64) -> String {
    day(offset).format("%Y-%m-%d").to_string()
}

fn br(offset: i64) -> String {
    day(offset).format("%d/%m/%Y").to_string()
}

/// A capture as the browser extension would send it, `days_ago` days old.
fn capture(
    number: &str,
    kind: &str,
    title: &str,
    sender: &str,
    days_ago: i64,
    text: &str,
    history: &[(i64, &str, &str)],
) -> Capture {
    let compact: String = number.chars().filter(char::is_ascii_alphanumeric).collect();
    let mut metadata = BTreeMap::new();
    metadata.insert("document_type".to_owned(), kind.to_owned());
    metadata.insert("sender".to_owned(), sender.to_owned());
    metadata.insert("Unidade".to_owned(), "Gabinete do Secretário".to_owned());
    metadata.insert("Data do documento".to_owned(), br(-days_ago));
    if !history.is_empty() {
        let events: Vec<Movement> = history
            .iter()
            .map(|(ago, event, description)| Movement {
                scope: "Documento".into(),
                date: format!("{} 09:{:02}", br(-ago), 10 + ago),
                event: (*event).into(),
                registered_unit: "Protocolo Geral".into(),
                registered_person: "Servidor do protocolo".into(),
                attendant_unit: "Gabinete do Secretário".into(),
                attendant_person: "Servidor A".into(),
                description: (*description).into(),
            })
            .collect();
        metadata.insert(
            "movement_source".to_owned(),
            format!(
                "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc/exibirAntigo?sigla={compact}"
            ),
        );
        metadata.insert(
            "movement_history".to_owned(),
            serde_json::to_string(&events).unwrap_or_default(),
        );
    }
    Capture {
        schema_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        source_url: format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla={}",
            number.replace('/', "%2F")
        ),
        document_number: number.to_owned(),
        title: title.to_owned(),
        captured_at: (Utc::now() - Duration::days(days_ago)).to_rfc3339(),
        text: text.to_owned(),
        metadata,
        references: vec![],
        coverage: Coverage {
            adapter: "amostra-v1".into(),
            recognized: vec!["identidade".into(), "texto".into(), "histórico".into()],
            pending: vec![],
        },
    }
}

fn documents(store: &mut Store) -> Result<Ids> {
    let mut ids = Ids::default();

    let logistics = capture(
        "ORG-OFI-2026/00431",
        "Ofício",
        "Solicitação de apoio logístico para evento",
        "Protocolo Geral",
        1,
        &format!(
            "Senhor Diretor,\n\n\
         Solicitamos o apoio logístico desta Diretoria para o Seminário de Integração das Equipes, \
         com o empréstimo de 20 (vinte) cadeiras e de equipamento de sonorização completo, \
         incluindo mesa de som e dois microfones sem fio.\n\n\
         Pedimos ainda a disponibilização de 2 (dois) veículos, com motorista, \
         para o transporte dos palestrantes entre o aeroporto e o local do evento.\n\n\
         Solicitamos também a reserva da sala de reuniões do 2º andar no dia {room_day}, das 14h às 17h, \
         para a reunião de preparação do evento.\n\n\
         Em razão da data marcada, a resposta deve ser enviada a este Protocolo até o fim da próxima semana, \
         para que a organização possa confirmar a programação. \
         Informamos que a confirmação dos veículos deve chegar ao Protocolo até {vehicles_day}.",
            room_day = br(SAMPLE_ROOM_DAY),
            vehicles_day = br(SAMPLE_VEHICLES_DAY),
        ),
        &[
            (2, "Registro do documento", "Documento recebido e registrado no protocolo."),
            (2, "Despacho", "Encaminhado ao Gabinete para providências."),
            (1, "Recebimento", "Recebido no Gabinete; aguardando distribuição."),
        ],
    );
    let receipt = store.import_capture(&logistics)?;
    ids.logistics = receipt.document_id;
    ids.logistics_capture = receipt.capture_id;

    let staff = capture(
        "ORG-MEM-2026/01207",
        "Memorando",
        "Quadro de pessoal do Projeto Alfa",
        "Coordenação de Projetos",
        4,
        "Encaminhamos o quadro de pessoal previsto para o Projeto Alfa no próximo mês: \
         42 técnicos, 18 analistas e 6 servidores de apoio administrativo, \
         distribuídos em três turnos. Solicitamos a conferência dos nomes e a confirmação das escalas.",
        &[(4, "Registro do documento", "Memorando registrado e remetido ao Gabinete.")],
    );
    ids.staff = store.import_capture(&staff)?.document_id;

    // The despacho says it was juntado to the memorando; the library links
    // the two by itself when it reads that line.
    let dispatch = capture(
        "ORG-DES-2026/00877",
        "Despacho",
        "Despacho de encaminhamento",
        "Chefia de Gabinete",
        3,
        &format!(
            "Encaminhe-se à Coordenação de Projetos para ciência e providências.\n\
             Juntado ao documento ORG-MEM-2026/01207 em {}.",
            br(-3)
        ),
        &[],
    );
    ids.dispatch = store.import_capture(&dispatch)?.document_id;

    let vehicles = capture(
        "ORG-OFI-2026/00398",
        "Ofício",
        "Planilha de distribuição de veículos",
        "Seção de Transportes",
        6,
        "Segue a planilha de distribuição dos veículos por unidade para o trimestre. \
         Constam 31 veículos leves, 8 caminhonetes e 4 veículos de transporte de pessoal. \
         Solicita-se a assinatura do Secretário para a publicação.",
        &[],
    );
    ids.vehicles = store.import_capture(&vehicles)?.document_id;

    let schedule = capture(
        "ORG-OFI-2026/00377",
        "Ofício",
        "Pedido de informações sobre escala",
        "Setor Jurídico",
        9,
        "A Setor Jurídico solicita informações sobre a escala de serviço dos plantões do último mês, \
         com a indicação dos responsáveis por turno. A resposta já foi enviada por ofício próprio.",
        &[],
    );
    ids.schedule = store.import_capture(&schedule)?.document_id;

    ids.receipt = store
        .create_manual_document(&ManualFields {
            kind: "papel".into(),
            title: "Recibo de entrega de equipamentos".into(),
            origin: "Almoxarifado central".into(),
            document_date: iso(-2),
            text: "Recibo assinado pelo almoxarife pela entrega de 12 rádios comunicadores, \
                   12 carregadores e 4 baterias reserva ao Gabinete."
                .into(),
        })?
        .document_id;
    Ok(ids)
}

fn note(store: &mut Store, ids: &Ids) -> Result<()> {
    store.save_note(
        ids.logistics,
        "Aguardar retorno do Servidor B sobre o auditório antes de responder.",
    )
}

/// An e-mail brought in from a `.eml` built here, without the network.
fn mail(store: &mut Store) -> Result<Option<i64>> {
    let date = (Local::now() - Duration::days(1)).to_rfc2822();
    let eml = format!(
        "From: Comitê Gestor <comite@exemplo.org>\r\n\
         To: Servidor A <servidor@exemplo.org>\r\n\
         Subject: Convite: reunião do Comitê Gestor\r\n\
         Date: {date}\r\n\
         Message-ID: <amostra-ggi-001@exemplo.org>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: 8bit\r\n\r\n\
         Prezado Servidor A,\r\n\r\n\
         Convidamos a Diretoria para a reunião ordinária do Comitê Gestor, \
         que tratará da integração das ações de fronteira.\r\n\r\n\
         Pedimos a confirmação de presença até amanhã.\r\n\r\n\
         Atenciosamente,\r\nComitê Gestor\r\n"
    );
    let imported = store.import_eml_bytes(eml.as_bytes())?;
    Ok(Some(imported.receipt.document_id))
}

/// A short conversation as copied from WhatsApp, brought in the way the
/// window does (the paste is read and the record created from it).
fn whatsapp(store: &mut Store) -> Result<()> {
    let date = (Local::now() - Duration::days(1))
        .format("%d/%m/%Y")
        .to_string();
    let today = br(0);
    let copied = [
        format!("[09:12, {date}] Servidor B: Bom dia. Preciso da sala de reuniões do 2º andar no dia {today}, das 14h às 17h, para a reunião da Unidade A."),
        format!("[09:15, {date}] Servidor A: Bom dia. Vou conferir a agenda e confirmo."),
        format!("[09:16, {date}] Servidor B: Ótimo. Pode reservar para a reunião da Unidade A?"),
        format!("[09:20, {date}] Servidor A: Anotei também o número do documento: ORG-OFI-2026/00431."),
        "Se mudar o horário, me avise.".to_owned(),
        format!("[09:21, {date}] Servidor B: Combinado, obrigado."),
    ]
    .join("\n");
    let conversation = crate::whatsapp::parse(&copied)?;
    let title = crate::whatsapp::suggested_title("Servidor B", &conversation);
    store.import_whatsapp("Servidor B", &title, &conversation, "whatsapp-colado")?;
    Ok(())
}

/// Rooms, equipment and a book, and their uses around today (so that the
/// pictures always show: one return overdue, one use going on now, two
/// reservations today, several in the next days, a loan of days and one
/// that competes for the notebooks).
fn resources(store: &mut Store, ids: &Ids) -> Result<()> {
    use crate::resources::{format_moment, now_moment, parse_moment, Resource, ResourceStatus};
    use crate::resources::{ResourceUse, UseStatus};
    let mut uid_of: BTreeMap<&str, String> = BTreeMap::new();
    let catalogue = [
        (
            "Sala de reuniões do 2º andar",
            "sala",
            1,
            "2º andar, bloco A",
            "",
            ResourceStatus::Active,
        ),
        (
            "Auditório",
            "sala",
            1,
            "Térreo, bloco A",
            "",
            ResourceStatus::Active,
        ),
        (
            "Projetor",
            "equipamento",
            1,
            "Armário da recepção",
            "Patrimônio 000123",
            ResourceStatus::Active,
        ),
        (
            "Notebook de apresentações",
            "equipamento",
            2,
            "Armário da recepção",
            "Patrimônios 000210 e 000211",
            ResourceStatus::Active,
        ),
        (
            "Cadeiras empilháveis",
            "equipamento",
            40,
            "Depósito do térreo",
            "",
            ResourceStatus::Active,
        ),
        (
            "Manual de redação oficial",
            "livro",
            3,
            "Biblioteca do gabinete",
            "3 exemplares",
            ResourceStatus::Active,
        ),
        (
            "Câmera de vídeo",
            "equipamento",
            1,
            "Armário da recepção",
            "Patrimônio 000345",
            ResourceStatus::Inactive,
        ),
    ];
    for (name, kind, quantity, location, identification, status) in catalogue {
        let resource = Resource {
            name: name.to_owned(),
            kind: kind.to_owned(),
            quantity,
            location: location.to_owned(),
            identification: identification.to_owned(),
            status,
            ..Default::default()
        };
        uid_of.insert(name, store.save_resource(&resource, None)?);
    }
    let now = parse_moment(&now_moment())?;
    let rel = |minutes: i64| format_moment(now + Duration::minutes(minutes));
    let at = |offset: i64, hm: &str| format!("{}T{hm}", iso(offset));
    let chat = store
        .list_documents("")?
        .into_iter()
        .find(|d| d.whatsapp)
        .map(|d| d.id);
    let attendance = store.list_saiop_attendances()?.first().map(|a| a.id);
    let task = store
        .list_tasks()?
        .into_iter()
        .find(|t| t.document_id == Some(ids.logistics))
        .map(|t| t.id);
    let sample = |resource: &str,
                  who: &str,
                  sector: &str,
                  purpose: &str,
                  start: String,
                  end: String,
                  status: UseStatus|
     -> ResourceUse {
        ResourceUse {
            resource_uid: uid_of[resource].clone(),
            who: who.to_owned(),
            sector: sector.to_owned(),
            purpose: purpose.to_owned(),
            start,
            end,
            status,
            ..Default::default()
        }
    };
    let admin = "Setor Administrativo";
    let legal = "Setor Jurídico";
    let mut uses = vec![
        // Late: out for two days and never brought back.
        sample(
            "Projetor",
            "Servidor C",
            admin,
            "Evento de capacitação",
            at(-2, "09:00"),
            at(-1, "17:00"),
            UseStatus::InUse,
        ),
        // Going on now.
        sample(
            "Notebook de apresentações",
            "Servidor A",
            legal,
            "Apresentação ao Comitê Gestor",
            rel(-90),
            rel(120),
            UseStatus::InUse,
        ),
        // Today.
        ResourceUse {
            document_id: chat,
            ..sample(
                "Sala de reuniões do 2º andar",
                "Servidor B",
                admin,
                "Reunião da Unidade A",
                at(0, "14:00"),
                at(0, "17:00"),
                UseStatus::Reserved,
            )
        },
        sample(
            "Auditório",
            "Servidor C",
            admin,
            "Ensaio da cerimônia",
            at(0, "16:00"),
            at(0, "18:00"),
            UseStatus::Reserved,
        ),
        // The next days.
        ResourceUse {
            document_id: Some(ids.logistics),
            task_id: task,
            ..sample(
                "Sala de reuniões do 2º andar",
                "Servidor A",
                legal,
                "Despachos da semana",
                at(1, "09:00"),
                at(1, "10:30"),
                UseStatus::Reserved,
            )
        },
        // A loan of days.
        sample(
            "Manual de redação oficial",
            "Servidor B",
            legal,
            "Empréstimo para consulta",
            at(1, "09:00"),
            at(8, "17:00"),
            UseStatus::Reserved,
        ),
        sample(
            "Sala de reuniões do 2º andar",
            "Servidor B",
            admin,
            "Atendimento externo",
            at(2, "14:00"),
            at(2, "16:00"),
            UseStatus::Reserved,
        ),
        // The notebooks: two at once is the most there is.
        ResourceUse {
            quantity: 2,
            ..sample(
                "Notebook de apresentações",
                "Servidor C",
                admin,
                "Treinamento de servidores",
                at(3, "09:00"),
                at(3, "12:00"),
                UseStatus::Reserved,
            )
        },
        sample(
            "Notebook de apresentações",
            "Servidor B",
            legal,
            "Reunião externa",
            at(2, "10:00"),
            at(2, "13:00"),
            UseStatus::Reserved,
        ),
        // An event: the hall and the chairs, linked to an atendimento.
        ResourceUse {
            saiop_id: attendance,
            quantity: 30,
            ..sample(
                "Cadeiras empilháveis",
                "Servidor A",
                admin,
                "Evento no auditório",
                at(3, "08:00"),
                at(3, "18:00"),
                UseStatus::Reserved,
            )
        },
        ResourceUse {
            saiop_id: attendance,
            ..sample(
                "Auditório",
                "Servidor A",
                admin,
                "Evento no auditório",
                at(3, "09:00"),
                at(3, "17:00"),
                UseStatus::Reserved,
            )
        },
        sample(
            "Sala de reuniões do 2º andar",
            "Servidor C",
            admin,
            "Reunião trimestral",
            at(20, "14:00"),
            at(20, "15:00"),
            UseStatus::Reserved,
        ),
        // Over.
        ResourceUse {
            returned_at: Some(at(-6, "11:40")),
            ..sample(
                "Projetor",
                "Servidor B",
                legal,
                "Capacitação interna",
                at(-6, "09:00"),
                at(-6, "12:00"),
                UseStatus::Returned,
            )
        },
        sample(
            "Sala de reuniões do 2º andar",
            "Servidor C",
            admin,
            "Reunião cancelada",
            at(-3, "10:00"),
            at(-3, "11:00"),
            UseStatus::Cancelled,
        ),
    ];
    uses.sort_by(|a, b| a.start.cmp(&b.start));
    for item in &uses {
        store.save_resource_use(item, None)?;
    }
    Ok(())
}

/// The triage task every import creates, to be turned into a real task.
fn triage_task(store: &Store, document_id: i64) -> Result<Task> {
    store
        .list_tasks()?
        .into_iter()
        .find(|t| t.document_id == Some(document_id))
        .with_context(|| format!("tarefa de triagem do documento #{document_id}"))
}

#[allow(clippy::too_many_arguments)]
fn shape(
    mut task: Task,
    title: &str,
    action: &str,
    status: &str,
    priority: &str,
    due: Option<i64>,
    assignee: &str,
    notes: &str,
) -> Task {
    task.title = title.into();
    task.action = action.into();
    task.status = status.into();
    task.priority = priority.into();
    task.due_date = due.map(iso).unwrap_or_default();
    task.assignee = assignee.into();
    task.notes = notes.into();
    task
}

fn tasks(store: &mut Store, ids: &Ids) -> Result<()> {
    // Overdue, with a person responsible.
    let first = shape(
        triage_task(store, ids.logistics)?,
        "Confirmar disponibilidade do auditório",
        "pedir complementacao",
        "aguardando",
        "alta",
        Some(-3),
        "Servidor B",
        "O Servidor B ficou de verificar a agenda do auditório.",
    );
    let first_id = store.save_task(&first)?;

    // Due today, waits for the first one.
    let mut reply = shape(
        Task::new(ids.logistics),
        "Responder ao Protocolo Geral sobre o apoio logístico",
        "responder",
        "pendente",
        "alta",
        Some(0),
        "Servidor A",
        "Responder só depois de confirmado o auditório.",
    );
    reply.dependencies = vec![first_id];
    store.save_task(&reply)?;

    let vehicles = shape(
        Task::new(ids.logistics),
        "Pedir os dois veículos à Seção de Transportes",
        "encaminhar",
        "pendente",
        "normal",
        Some(2),
        "",
        "",
    );
    store.save_task(&vehicles)?;

    // Due today.
    let staff = shape(
        triage_task(store, ids.staff)?,
        "Conferir o quadro de pessoal",
        "examinar",
        "em analise",
        "normal",
        Some(0),
        "Servidor A",
        "Conferir nomes e turnos com a Coordenação de Projetos.",
    );
    store.save_task(&staff)?;
    let escala = shape(
        Task::new(ids.staff),
        "Atualizar a escala da equipe do Projeto Alfa",
        "acompanhar",
        "pendente",
        "normal",
        Some(5),
        "",
        "",
    );
    store.save_task(&escala)?;

    // Done.
    let ciencia = shape(
        triage_task(store, ids.dispatch)?,
        "Tomar ciência do despacho de encaminhamento",
        "tomar ciencia",
        "concluida",
        "baixa",
        None,
        "Servidor A",
        "",
    );
    store.save_task(&ciencia)?;

    // Next week.
    let revisar = shape(
        triage_task(store, ids.vehicles)?,
        "Revisar a distribuição de veículos antes da assinatura",
        "examinar",
        "pendente",
        "normal",
        Some(7),
        "Servidor A",
        "",
    );
    store.save_task(&revisar)?;

    // No deadline.
    let arquivar = shape(
        triage_task(store, ids.schedule)?,
        "Arquivar o pedido de informações sobre escala",
        "baixar documento",
        "pendente",
        "baixa",
        None,
        "",
        "",
    );
    store.save_task(&arquivar)?;
    let recibo = shape(
        triage_task(store, ids.receipt)?,
        "Guardar o recibo na pasta do almoxarifado",
        "acompanhar",
        "pendente",
        "baixa",
        None,
        "",
        "",
    );
    store.save_task(&recibo)?;

    if let Some(mail) = ids.mail {
        let presenca = shape(
            triage_task(store, mail)?,
            "Confirmar presença na reunião do Comitê Gestor",
            "responder",
            "pendente",
            "alta",
            Some(1),
            "Servidor A",
            "",
        );
        store.save_task(&presenca)?;
    }

    // Without document: nobody imported anything for them.
    let certificate = shape(
        Task::standalone(),
        "Renovar a assinatura do certificado digital",
        "acompanhar",
        "pendente",
        "alta",
        Some(4),
        "Servidor A",
        "O certificado vence no fim do mês; pedir a renovação ao setor de tecnologia.",
    );
    store.save_task(&certificate)?;
    let room = shape(
        Task::standalone(),
        "Confirmar a sala da reunião de quinta-feira",
        "encaminhar",
        "pendente",
        "normal",
        Some(0),
        "",
        "",
    );
    store.save_task(&room)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn attendance(
    reference: &str,
    subject: &str,
    document_id: Option<i64>,
    received_from: &str,
    assignee: &str,
    status: &str,
    priority: &str,
    due: Option<i64>,
    entry: i64,
) -> SaiopAttendance {
    let mut a = SaiopAttendance::new_manual();
    a.reference_raw = reference.into();
    a.subject = subject.into();
    a.document_id = document_id;
    a.received_from = received_from.into();
    a.assignee = assignee.into();
    a.status = status.into();
    a.priority = priority.into();
    a.due_date = due.map(iso).unwrap_or_default();
    a.due_date_raw = a.due_date.clone();
    a.entry_date = iso(-entry);
    a.entry_date_raw = a.entry_date.clone();
    a.stage_changed_at = a.entry_date.clone();
    a.events.push(SaiopEvent {
        when: (Utc::now() - Duration::days(entry)).to_rfc3339(),
        actor: "Servidor A".into(),
        kind: "entrada".into(),
        detail: format!("Demanda recebida de {received_from}"),
    });
    a
}

fn attendances(store: &mut Store, ids: &Ids) -> Result<()> {
    let mut first = attendance(
        "ORG-OFI-2026/00431",
        "Apoio logístico para o seminário de integração",
        Some(ids.logistics),
        "Protocolo Geral",
        "Servidor A",
        "em_analise",
        "alta",
        Some(0),
        1,
    );
    first.tags = vec!["Ofício".into()];
    store.save_saiop_attendance(&first, None)?;

    let mut second = attendance(
        "ORG-MEM-2026/01207",
        "Quadro de pessoal do Projeto Alfa",
        Some(ids.staff),
        "Coordenação de Projetos",
        "Servidor A",
        "aguardando_resposta",
        "normal",
        Some(4),
        4,
    );
    second.sent_to = "Coordenação de Projetos".into();
    second.due_kind = "cobranca_retorno".into();
    store.save_saiop_attendance(&second, None)?;

    // Waiting for the signature.
    let mut third = attendance(
        "ORG-OFI-2026/00398",
        "Planilha de distribuição de veículos",
        Some(ids.vehicles),
        "Seção de Transportes",
        "Servidor A",
        "aguardando_deliberacao",
        "normal",
        Some(7),
        6,
    );
    third.signed_status = "nao".into();
    third.planned_doc_type = "Ofício".into();
    store.save_saiop_attendance(&third, None)?;

    // Closed.
    let mut fourth = attendance(
        "ORG-OFI-2026/00377",
        "Pedido de informações sobre escala",
        Some(ids.schedule),
        "Setor Jurídico",
        "Servidor A",
        "encerrado",
        "normal",
        Some(-4),
        9,
    );
    fourth.outcome = "respondido".into();
    fourth.signed_status = "sim".into();
    fourth.exit_date = iso(-5);
    fourth.exit_date_raw = fourth.exit_date.clone();
    fourth.closed_at = iso(-5);
    fourth.sent_to = "Setor Jurídico".into();
    store.save_saiop_attendance(&fourth, None)?;

    // Deadline already past.
    let mut fifth = attendance(
        "Recibo de entrega de equipamentos",
        "Recibo de entrega de equipamentos",
        Some(ids.receipt),
        "Almoxarifado central",
        "Servidor A",
        "em_elaboracao",
        "urgente",
        Some(-2),
        5,
    );
    fifth.priority_reason = "Prestação de contas do trimestre".into();
    store.save_saiop_attendance(&fifth, None)?;

    // Not assigned yet.
    let sixth = attendance(
        "Convite do Comitê Gestor",
        "Convite: reunião do Comitê Gestor",
        ids.mail,
        "Comitê Gestor",
        "",
        "a_distribuir",
        "baixa",
        None,
        1,
    );
    store.save_saiop_attendance(&sixth, None)?;
    Ok(())
}

/// A fact of the sample, checked by the program as the real run would: the
/// quote must be written in `text`, and the dates, times and quantities in
/// the quote.
#[allow(clippy::too_many_arguments)]
fn sample_fact(
    text: &str,
    id: &str,
    kind: &str,
    statement: &str,
    quote: &str,
    date: Option<String>,
    times: Option<(&str, Option<&str>)>,
    quantity: Option<u32>,
) -> Result<crate::model::Fact> {
    let mut fact = crate::model::Fact {
        id: id.into(),
        kind: kind.into(),
        statement: statement.into(),
        quote: quote.into(),
        date,
        time: times.map(|(start, _)| start.to_owned()),
        end_time: times.and_then(|(_, end)| end.map(str::to_owned)),
        quantity,
        ..Default::default()
    };
    fact.checked = crate::evidence::check_fact(&mut fact, text, "", &[]);
    anyhow::ensure!(
        fact.checked.quote,
        "o trecho de {id} não está escrito no texto da amostra"
    );
    Ok(fact)
}

fn sample_run() -> crate::model::AnalysisRun {
    use crate::model::RunStep;
    crate::model::AnalysisRun {
        provider: "Ollama neste computador".into(),
        steps: vec![
            RunStep {
                role: "extrator".into(),
                attempts: 2,
                refused: vec![
                    "Um trecho citado não está escrito no documento.".into(),
                    "Uma data não está escrita no trecho do fato.".into(),
                ],
                dropped: 1,
            },
            RunStep {
                role: "propositor".into(),
                attempts: 1,
                refused: Vec::new(),
                dropped: 0,
            },
        ],
        calls: 3,
    }
}

/// Analyses with facts and proposals, as the program would have saved them
/// (every "conferido" mark is made by the checker, not written by hand), and
/// the old kind of suggestion on a third document. No AI is called.
fn analysis(store: &mut Store, ids: &Ids) -> Result<()> {
    use crate::model::{AttendanceProposal, Proposals, ReservationProposal, TaskProposal};
    let room_day = iso(SAMPLE_ROOM_DAY);
    let room_br = br(SAMPLE_ROOM_DAY);
    let vehicles_day = iso(SAMPLE_VEHICLES_DAY);
    let vehicles_br = br(SAMPLE_VEHICLES_DAY);

    // 1. The ofício: two tasks (one with a relative deadline the program could
    // not read), the atendimento it already has, and three reservations (a
    // free room, chairs without a time, vehicles the register does not have).
    let detail = store.load_document(ids.logistics)?;
    let text = detail.capture.text.clone();
    // The context the analysis was made with: the saved note, so it is current.
    let human_context = detail.work_context.analysis_text(&detail.note);
    let facts = vec![
        sample_fact(
            &text,
            "f1",
            "prazo",
            "A resposta deve ser enviada ao Protocolo até o fim da próxima semana.",
            "a resposta deve ser enviada a este Protocolo até o fim da próxima semana",
            None,
            None,
            None,
        )?,
        sample_fact(
            &text,
            "f2",
            "prazo",
            &format!("A confirmação dos veículos deve chegar ao Protocolo até {vehicles_br}."),
            &format!("a confirmação dos veículos deve chegar ao Protocolo até {vehicles_br}"),
            Some(vehicles_day.clone()),
            None,
            None,
        )?,
        sample_fact(
            &text,
            "f3",
            "recurso",
            "O ofício pede o empréstimo de 20 cadeiras.",
            "empréstimo de 20 (vinte) cadeiras",
            None,
            None,
            Some(20),
        )?,
        sample_fact(
            &text,
            "f4",
            "recurso",
            "O ofício pede a reserva da sala de reuniões do 2º andar.",
            "reserva da sala de reuniões do 2º andar",
            None,
            None,
            None,
        )?,
        sample_fact(
            &text,
            "f5",
            "data_hora",
            &format!("A sala é pedida no dia {room_br}, das 14h às 17h."),
            &format!("no dia {room_br}, das 14h às 17h"),
            Some(room_day.clone()),
            Some(("14:00", Some("17:00"))),
            None,
        )?,
        sample_fact(
            &text,
            "f6",
            "recurso",
            "O ofício pede 2 veículos com motorista.",
            "2 (dois) veículos, com motorista",
            None,
            None,
            Some(2),
        )?,
    ];
    let proposals = Proposals {
        tasks: vec![
            TaskProposal {
                id: "p-tarefa-resposta".into(),
                action: "responder".into(),
                title: "Responder ao Protocolo sobre o apoio ao seminário".into(),
                priority: "alta".into(),
                reason: "O ofício pede resposta dentro de um prazo.".into(),
                facts: vec!["f1".into()],
                remark: "Prazo citado sem data: “até o fim da próxima semana”".into(),
                ..Default::default()
            },
            TaskProposal {
                id: "p-tarefa-veiculos".into(),
                action: "encaminhar".into(),
                title: "Pedir os veículos à Seção de Transportes".into(),
                priority: "alta".into(),
                reason: "Os veículos dependem de outra seção e há data para confirmar.".into(),
                facts: vec!["f6".into(), "f2".into()],
                due_fact: Some("f2".into()),
                due_date: Some(vehicles_day.clone()),
                due_source: format!(
                    "a confirmação dos veículos deve chegar ao Protocolo até {vehicles_br}"
                ),
                ..Default::default()
            },
        ],
        attendance: Some(AttendanceProposal {
            id: "p-atendimento".into(),
            subject: "Apoio logístico para o seminário de integração".into(),
            received_from: "Protocolo Geral".into(),
            due_fact: Some("f2".into()),
            due_date: Some(vehicles_day.clone()),
            due_source: format!(
                "a confirmação dos veículos deve chegar ao Protocolo até {vehicles_br}"
            ),
            due_kind: "a_confirmar".into(),
            priority: "alta".into(),
            priority_reason: "Evento com data marcada e prazo para confirmar os veículos".into(),
            planned_doc_type: "Ofício".into(),
            facts: vec!["f2".into()],
            remark: String::new(),
            received_from_checked: false,
        }),
        reservations: vec![
            ReservationProposal {
                id: "p-reserva-sala".into(),
                what: "sala de reuniões do 2º andar".into(),
                what_fact: "f4".into(),
                quantity: 1,
                when_fact: Some("f5".into()),
                start: Some(format!("{room_day}T14:00")),
                end: Some(format!("{room_day}T17:00")),
                purpose: "Reunião de preparação do evento".into(),
                facts: vec!["f5".into()],
                remark: String::new(),
            },
            ReservationProposal {
                id: "p-reserva-cadeiras".into(),
                what: "cadeiras".into(),
                what_fact: "f3".into(),
                quantity: 20,
                purpose: "Seminário de Integração das Equipes".into(),
                ..Default::default()
            },
            ReservationProposal {
                id: "p-reserva-veiculos".into(),
                what: "veículos".into(),
                what_fact: "f6".into(),
                quantity: 2,
                purpose: "Transporte dos palestrantes".into(),
                ..Default::default()
            },
        ],
        nothing_reason: String::new(),
        nothing_reason_ai: String::new(),
    };
    let analysis = Analysis {
        capture_id: ids.logistics_capture,
        human_context,
        subjects: vec!["Apoio logístico".into(), "Evento institucional".into()],
        priority: "alta".into(),
        reason: "Há prazo para a resposta e o pedido envolve cadeiras, som, uma sala e veículos.".into(),
        deadline_candidate: Some("até o fim da próxima semana".into()),
        summary: "O Protocolo Geral pede apoio para um seminário: 20 cadeiras, equipamento de som, \
                  a sala de reuniões do 2º andar e 2 veículos com motorista. A resposta deve chegar \
                  ao Protocolo até o fim da próxima semana."
            .into(),
        model: "amostra".into(),
        created_at: Utc::now().to_rfc3339(),
        suggested_actions: vec![
            ActionSuggestion {
                action: "responder".into(),
                reason: "O ofício exige resposta dentro do prazo informado.".into(),
                evidence: "a resposta deve ser enviada a este Protocolo até o fim da próxima semana".into(),
            },
            ActionSuggestion {
                action: "encaminhar".into(),
                reason: "Os veículos dependem da Seção de Transportes.".into(),
                evidence: "2 (dois) veículos, com motorista".into(),
            },
        ],
        facts,
        proposals: Some(proposals),
        run: Some(sample_run()),
        ..Default::default()
    };
    store.save_analysis(ids.logistics, &analysis)?;

    // 2. The WhatsApp record: a room asked for at a time another use already
    // holds (to picture "Ocupado"), and an entry the document does not have.
    let chat = store
        .list_documents("")?
        .into_iter()
        .find(|d| d.whatsapp)
        .context("conversa do WhatsApp de exemplo")?;
    let detail = store.load_document(chat.id)?;
    let text = detail.capture.text.clone();
    let today_br = br(0);
    let facts = vec![
        sample_fact(
            &text,
            "g1",
            "recurso",
            "Servidor B precisa da sala de reuniões do 2º andar.",
            "Preciso da sala de reuniões do 2º andar",
            None,
            None,
            None,
        )?,
        sample_fact(
            &text,
            "g2",
            "data_hora",
            &format!("A sala é pedida no dia {today_br}, das 14h às 17h."),
            &format!("no dia {today_br}, das 14h às 17h"),
            Some(iso(0)),
            Some(("14:00", Some("17:00"))),
            None,
        )?,
        sample_fact(
            &text,
            "g3",
            "remetente",
            "A conversa é com Servidor B.",
            "Servidor B: Bom dia.",
            None,
            None,
            None,
        )?,
    ];
    let proposals = Proposals {
        attendance: Some(AttendanceProposal {
            id: "p-atendimento-conversa".into(),
            subject: "Reserva da sala de reuniões para a Unidade A".into(),
            received_from: "Servidor B".into(),
            due_kind: "a_confirmar".into(),
            priority: "normal".into(),
            facts: vec!["g3".into()],
            ..Default::default()
        }),
        reservations: vec![ReservationProposal {
            id: "p-reserva-sala-conversa".into(),
            what: "sala de reuniões do 2º andar".into(),
            what_fact: "g1".into(),
            quantity: 1,
            when_fact: Some("g2".into()),
            start: Some(format!("{}T14:00", iso(0))),
            end: Some(format!("{}T17:00", iso(0))),
            purpose: "Reunião da Unidade A".into(),
            facts: vec!["g2".into()],
            remark: String::new(),
        }],
        ..Default::default()
    };
    let analysis = Analysis {
        capture_id: chat.latest_capture_id,
        subjects: vec!["Reserva de sala".into()],
        priority: "normal".into(),
        reason: "A conversa pede uma sala em data e horário certos.".into(),
        summary: "Servidor B pede a sala de reuniões do 2º andar, das 14h às 17h, para a reunião da Unidade A."
            .into(),
        model: "amostra".into(),
        created_at: Utc::now().to_rfc3339(),
        facts,
        proposals: Some(proposals),
        run: Some(crate::model::AnalysisRun {
            provider: "Ollama neste computador".into(),
            steps: vec![
                crate::model::RunStep {
                    role: "extrator".into(),
                    attempts: 1,
                    refused: Vec::new(),
                    dropped: 0,
                },
                crate::model::RunStep {
                    role: "propositor".into(),
                    attempts: 1,
                    refused: Vec::new(),
                    dropped: 0,
                },
            ],
            calls: 2,
        }),
        ..Default::default()
    };
    store.save_analysis(chat.id, &analysis)?;

    // 3. A document analysed before the proposals existed: the old block.
    let old = Analysis {
        capture_id: store.load_document(ids.vehicles)?.summary.latest_capture_id,
        subjects: vec!["Distribuição de veículos".into()],
        priority: "normal".into(),
        reason: "A planilha pede a assinatura do Secretário para publicar.".into(),
        summary: "A Seção de Transportes envia a distribuição dos veículos e pede a assinatura do Secretário."
            .into(),
        model: "amostra".into(),
        created_at: Utc::now().to_rfc3339(),
        suggested_actions: vec![ActionSuggestion {
            action: "encaminhar".into(),
            reason: "A publicação depende da assinatura.".into(),
            evidence: "Solicita-se a assinatura do Secretário para a publicação.".into(),
        }],
        ..Default::default()
    };
    store.save_analysis(ids.vehicles, &old)
}

fn templates(store: &mut Store) -> Result<()> {
    let style = store.save_block(
        &InstructionBlock::new(
            "Linguagem formal",
            "Linguagem e estilo",
            "Use linguagem formal e objetiva, em terceira pessoa, sem gírias e sem abreviações.",
        ),
        None,
    )?;
    store.set_block_status(&style.uid, Status::Published, None)?;
    let checks = store.save_block(
        &InstructionBlock::new(
            "Conferências finais",
            "Conferências finais",
            "Confira números, datas e nomes antes de concluir, e cite o documento de origem.",
        ),
        None,
    )?;
    store.set_block_status(&checks.uid, Status::Published, None)?;

    // Published.
    let mut reply = WorkTemplate::new(
        "Resposta a pedido de informações",
        "resposta",
        "Em atenção ao {{documento}}, informamos que {{informacao}}.",
    );
    reply.purpose = "Responder pedidos de informação de outros órgãos.".into();
    reply.usage_criteria = "Quando o documento só pede dados que o Gabinete já possui.".into();
    reply.scope = Scope::Shared;
    if let Some(version) = reply.draft_version_mut() {
        version.sections = vec![
            Section {
                title: "Abertura".into(),
                guidance: "Cite o documento recebido e agradeça o contato.".into(),
                required: true,
            },
            Section {
                title: "Informações".into(),
                guidance: "Responda ponto a ponto ao que foi perguntado.".into(),
                required: true,
            },
            Section {
                title: "Fecho".into(),
                guidance: "Coloque-se à disposição para esclarecimentos.".into(),
                required: false,
            },
        ];
        version.variables = vec![
            Variable {
                name: "documento".into(),
                label: "Documento recebido".into(),
                required: true,
            },
            Variable {
                name: "informacao".into(),
                label: "Informação a prestar".into(),
                required: true,
            },
        ];
        version.block_refs = vec![
            BlockRef {
                block_uid: style.uid.clone(),
                version: None,
                mode: BlockMode::Mandatory,
            },
            BlockRef {
                block_uid: checks.uid.clone(),
                version: None,
                mode: BlockMode::Optional,
            },
        ];
        version.references = vec![Reference {
            label: "Manual de redação oficial".into(),
            locator: "Pasta Normas / Redação".into(),
            note: String::new(),
        }];
        version.criteria = vec!["A resposta traz todas as informações pedidas.".into()];
    }
    let reply = store.save_template(&reply, None)?;
    store.set_template_status(&reply.uid, Status::Published, None)?;

    // Draft.
    let mut forward = WorkTemplate::new(
        "Encaminhamento a setor",
        "encaminhamento",
        "Encaminhe-se ao(à) {{setor}} para {{providencia}}.",
    );
    forward.purpose = "Despachar um documento a outro setor com o pedido de providência.".into();
    forward.scope = Scope::Personal;
    if let Some(version) = forward.draft_version_mut() {
        version.block_refs = vec![BlockRef {
            block_uid: style.uid.clone(),
            version: None,
            mode: BlockMode::Mandatory,
        }];
    }
    store.save_template(&forward, None)?;
    Ok(())
}

/// Conflicts and Lixeira items for the pictures of "Edições em conflito" and
/// of the Lixeira: an ordinary conflict, two about the same task and one
/// whose document is in the Lixeira, plus a few items of different kinds in
/// the Lixeira. Run once, after `seed`; does nothing when already done.
pub fn conflicts_scenario(store: &mut Store) -> Result<()> {
    use crate::intranet::{IntranetMutation, SyncConflict};
    use crate::trash::{Kind, Target};
    if !store.list_sync_conflicts()?.is_empty() {
        return Ok(());
    }
    let find = |store: &Store, title: &str| -> Result<Task> {
        store
            .list_tasks()?
            .into_iter()
            .find(|t| t.title == title)
            .with_context(|| format!("tarefa de exemplo {title:?}"))
    };
    let conflict =
        |actor: &str, label: &str, task: &Task, base: &Task, summary: String, minutes_ago: i64| {
            SyncConflict {
                id: 0,
                created_at: (Utc::now() - Duration::minutes(minutes_ago)).to_rfc3339(),
                actor: actor.into(),
                item_label: label.into(),
                local_summary: summary,
                host_summary: String::new(),
                reason: "Conflito de edição simultânea".into(),
                mutation: IntranetMutation::SaveTask {
                    task: task.clone(),
                    expected_version: None,
                },
                base: Some(IntranetMutation::SaveTask {
                    task: base.clone(),
                    expected_version: None,
                }),
                unresolved: false,
            }
        };
    let folder = r"[da pasta C:\Exemplo\Pasta\ponte-sync.ponte] ";

    // 1. Changed on both sides, in different fields.
    let current = find(store, "Confirmar disponibilidade do auditório")?;
    let mut base = current.clone();
    base.assignee = "Servidor A".into();
    base.status = "pendente".into();
    let mut mine = base.clone();
    mine.title = "Confirmar disponibilidade do auditório e do som".into();
    mine.status = "em analise".into();
    let ordinary = conflict(
        "Servidor A",
        &format!("Tarefa #{}: {}", mine.id, mine.title),
        &mine,
        &base,
        String::new(),
        95,
    );

    // 2. The same task changed twice on the other device.
    let current = find(store, "Arquivar o pedido de informações sobre escala")?;
    let mut done = current.clone();
    done.status = "concluida".into();
    let mut reopened = current.clone();
    reopened.status = "pendente".into();
    let first = conflict(
        "Celular de Servidor A",
        &format!("{folder}Tarefa #{}: {}", done.id, done.title),
        &done,
        &current,
        String::new(),
        40,
    );
    let second = conflict(
        "Celular de Servidor A",
        &format!("{folder}Tarefa #{}: {}", reopened.id, reopened.title),
        &reopened,
        &done,
        String::new(),
        38,
    );

    // 3. A task whose document went to the Lixeira.
    let trashed = capture(
        "ORG-OFI-2026/00512",
        "Ofício",
        "Pedido de manutenção de equipamentos",
        "Seção de Transportes",
        12,
        "Solicita a manutenção preventiva dos equipamentos de rádio do Gabinete.",
        &[],
    );
    let trashed_doc = store.import_capture(&trashed)?.document_id;
    let triage = store
        .list_tasks()?
        .into_iter()
        .find(|t| t.document_id == Some(trashed_doc))
        .context("tarefa do documento de exemplo")?;
    let mut changed = triage.clone();
    changed.status = "concluida".into();
    let blocked = conflict(
        "Servidor A",
        &format!("Tarefa #{}: {}", changed.id, changed.title),
        &changed,
        &triage,
        String::new(),
        20,
    );
    // A task of that document, trashed first: it shows under the document.
    let minuta = shape(
        Task::new(trashed_doc),
        "Preparar a minuta de resposta",
        "responder",
        "pendente",
        "normal",
        None,
        "",
        "",
    );
    let minuta_id = store.save_task(&minuta)?;
    store.trash_item(Target {
        kind: Kind::Task,
        id: minuta_id,
    })?;
    store.trash_item(Target {
        kind: Kind::Document,
        id: trashed_doc,
    })?;

    // A task and an attachment in the Lixeira by themselves.
    let loose = shape(
        Task::new(
            store
                .list_documents("")?
                .into_iter()
                .find(|d| d.document_number == "ORG-OFI-2026/00377")
                .map(|d| d.id)
                .context("documento de exemplo")?,
        ),
        "Responder ao Setor Jurídico sobre a escala",
        "responder",
        "pendente",
        "normal",
        None,
        "Servidor B",
        "",
    );
    let loose_id = store.save_task(&loose)?;
    store.trash_item(Target {
        kind: Kind::Task,
        id: loose_id,
    })?;
    let vehicles = store
        .list_documents("")?
        .into_iter()
        .find(|d| d.document_number == "ORG-OFI-2026/00398")
        .map(|d| d.id)
        .context("documento de exemplo")?;
    let attachment = store.add_attachment(
        crate::attachments::Owner::Document(vehicles),
        "planilha-de-veiculos.txt",
        "Rascunho da distribuição",
        b"Veiculos por unidade, primeiro trimestre.",
    )?;
    store.trash_item(Target {
        kind: Kind::Attachment,
        id: attachment,
    })?;

    store.shots_insert_conflicts(&[ordinary, first, second, blocked])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_a_believable_library() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("amostra.sqlite3")).unwrap();
        seed(&mut store).unwrap();

        let documents = store.list_documents("").unwrap();
        assert!(documents.len() >= 6, "documents: {}", documents.len());
        assert!(documents
            .iter()
            .any(|d| d.document_number == "ORG-OFI-2026/00431"));
        assert!(documents.iter().any(|d| d.joined_to_number.is_some()));
        assert!(documents.iter().any(|d| d.email));
        let chat = documents.iter().find(|d| d.whatsapp).unwrap();
        assert!(chat.manual && !chat.email && chat.whatsapp_chat == "Servidor B");
        let tasks = store.list_tasks().unwrap();
        assert!(tasks.len() >= 8, "tasks: {}", tasks.len());
        assert!(tasks.iter().any(|t| t.overdue()));
        assert!(tasks.iter().any(|t| t.document_id.is_none()));
        let attendances = store.list_saiop_attendances().unwrap();
        assert!(
            attendances.len() >= 5,
            "atendimentos: {}",
            attendances.len()
        );
        assert!(store.list_templates().unwrap().len() >= 2);
        let resources = store.list_resources().unwrap();
        assert!(resources.iter().any(|r| r.quantity > 1));
        assert!(resources
            .iter()
            .any(|r| r.status == crate::resources::ResourceStatus::Inactive));
        let uses = store.list_resource_uses().unwrap();
        let now = crate::resources::now_moment();
        assert!(
            uses.iter().any(|u| u.overdue_at(&now)),
            "one return is late"
        );
        assert!(uses.iter().any(|u| u.document_id.is_some()));
        assert!(uses.iter().any(|u| u.saiop_id.is_some()));
        assert!(store.auto_attendance_enabled().unwrap());
    }
}
