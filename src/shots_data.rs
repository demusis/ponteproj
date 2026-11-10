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
    tasks(store, &ids).context("tarefas de exemplo")?;
    attendances(store, &ids).context("atendimentos de exemplo")?;
    analysis(store, &ids).context("análise de exemplo")?;
    templates(store).context("modelos de exemplo")?;
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
        "Senhor Diretor,\n\n\
         Solicitamos o apoio logístico desta Diretoria para o Seminário de Integração das Equipes, \
         com o empréstimo de 20 (vinte) cadeiras e de equipamento de sonorização completo, \
         incluindo mesa de som e dois microfones sem fio.\n\n\
         Pedimos ainda a disponibilização de 2 (dois) veículos, com motorista, \
         para o transporte dos palestrantes entre o aeroporto e o local do evento.\n\n\
         Em razão da data marcada, a resposta deve ser enviada a este Protocolo até o fim da próxima semana, \
         para que a organização possa confirmar a programação.",
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

/// A summary with suggested actions, as the assistant would have saved it,
/// without calling any AI.
fn analysis(store: &mut Store, ids: &Ids) -> Result<()> {
    let analysis = Analysis {
        capture_id: ids.logistics_capture,
        subjects: vec!["Apoio logístico".into(), "Evento institucional".into()],
        priority: "alta".into(),
        reason: "Há prazo para a resposta e o pedido envolve cadeiras, som e veículos.".into(),
        deadline_candidate: Some(format!("{} (fim da próxima semana)", br(7))),
        summary: "O Protocolo Geral pede apoio para um seminário: 20 cadeiras, equipamento de som e 2 veículos \
                  com motorista. A resposta deve chegar ao Protocolo até o fim da próxima semana."
            .into(),
        model: "amostra".into(),
        created_at: Utc::now().to_rfc3339(),
        suggested_actions: vec![
            ActionSuggestion {
                action: "responder".into(),
                reason: "O ofício exige resposta dentro do prazo informado.".into(),
                evidence: "\"a resposta deve ser enviada a este Protocolo até o fim da próxima semana\"".into(),
            },
            ActionSuggestion {
                action: "encaminhar".into(),
                reason: "Os veículos dependem da Seção de Transportes.".into(),
                evidence: "\"2 (dois) veículos, com motorista\"".into(),
            },
        ],
        ..Default::default()
    };
    store.save_analysis(ids.logistics, &analysis)
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
        assert!(store.auto_attendance_enabled().unwrap());
    }
}
