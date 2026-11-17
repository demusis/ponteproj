//! Repetição (recorrência) de tarefas e de usos de recursos.
//!
//! Módulo puro: calcula datas e monta **cópias em memória**. Não grava nada,
//! não lê o relógio e não gera cópias sozinho (por exemplo, ao concluir uma
//! tarefa). Quem chama mostra uma prévia, e só depois de a pessoa confirmar
//! grava o lote inteiro, de uma vez, numa única transação (ver "Contrato de
//! persistência" abaixo).
//!
//! # Regras de calendário
//!
//! * A regra é `frequência` + `intervalo` + fim (`count` ocorrências e/ou
//!   data limite `until`). `count` conta a ocorrência original: `count = 4`
//!   semanal significa a tarefa que já existe e mais três cópias.
//! * A k-ésima data é sempre calculada **a partir da âncora** (`âncora + k ×
//!   intervalo`), nunca da data anterior. Por isso não há deriva: 31/01 →
//!   28/02 → 31/03 (e não 28/03), e 29/02/2028 → 28/02/2029 → 29/02/2032.
//! * Em mês curto o dia é reduzido ao último dia do mês; o dia original
//!   volta quando o mês comporta.
//! * `until` é inclusivo. Parar por `count` ou por `until`, o que vier antes.
//!
//! # Fábricas
//!
//! * [`task_occurrences`] devolve só as **cópias novas** (`count - 1`
//!   tarefas, ou menos se `until` cortar antes): a tarefa de origem já é a
//!   primeira ocorrência e continua como está. Lista vazia = a regra não
//!   gera nada além da original (ex.: `count = 1`).
//! * [`resource_occurrences`] faz o mesmo para um uso de recurso: devolve
//!   só as reservas novas, depois da original.
//!
//! # Contrato de persistência (para quem integra)
//!
//! 1. **Prévia primeiro.** Chame a fábrica, mostre datas/horários e, para
//!    recursos, verifique conflitos de cada cópia. Nada foi gravado.
//! 2. **Um lote, uma transação.** Ao confirmar, grave todas as cópias na
//!    mesma transação SQLite (`BEGIN IMMEDIATE`); se uma falhar (validação,
//!    conflito de horário, limite), desfaça tudo — nunca fique com um lote
//!    pela metade.
//! 3. **Idempotência.** Cada cópia já nasce com `uid` próprio (tarefas: UUID
//!    com hífens; usos: UUID simples, como `resources::new_uid`). O lote
//!    prévio deve ser o mesmo que se grava: guarde o `Vec` da prévia e grave
//!    esse vetor; não chame a fábrica de novo na confirmação (geraria uids
//!    novos e, num reenvio, duplicaria). Gravar por `uid` com `INSERT OR
//!    IGNORE` / upsert faz o reenvio do mesmo lote (duplo clique, replay da
//!    fila offline) não duplicar nada. No celular/fila offline, envie o lote
//!    como uma única mutação com os uids já fixados.
//! 4. **Sem efeito colateral.** Concluir uma tarefa não cria a próxima;
//!    repetir é uma ação explícita de "Repetir…" sobre a tarefa/reserva.
//! 5. **Ids.** Tarefas voltam com `id = 0` (o `INSERT` atribui). Dependências
//!    e tarefa-mãe são **limpas**, pois apontariam para a tarefa original
//!    e ligariam cópias a si mesmas por engano; o vínculo do uso de recurso
//!    com uma tarefa (`task_id`) também é limpo. Vínculos com documento
//!    (`document_id`) são mantidos.
use crate::{
    resources::{parse_moment, ResourceUse, UseStatus},
    tasks::Task,
};
use anyhow::{bail, ensure, Context, Result};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// Máximo de ocorrências de uma regra, contando a original.
pub const MAX_COUNT: u16 = 104;
/// Último ano aceito (limita a escrita `AAAA-MM-DD` a quatro dígitos).
const MAX_YEAR: i32 = 9999;
const DATE_FORMAT: &str = "%Y-%m-%d";
const MOMENT_FORMAT: &str = "%Y-%m-%dT%H:%M";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    pub const ALL: [Frequency; 4] = [Self::Daily, Self::Weekly, Self::Monthly, Self::Yearly];

    /// Rótulo da frequência (singular) para a interface.
    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Diária",
            Self::Weekly => "Semanal",
            Self::Monthly => "Mensal",
            Self::Yearly => "Anual",
        }
    }

    /// Maior intervalo aceito para a frequência.
    pub fn max_interval(self) -> u16 {
        match self {
            Self::Daily => 365,
            Self::Weekly => 104,
            Self::Monthly => 24,
            Self::Yearly => 10,
        }
    }
}

/// Regra de repetição. Os campos ausentes no JSON assumem o padrão.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recurrence {
    pub frequency: Frequency,
    /// A cada `interval` dias/semanas/meses/anos; mínimo 1.
    pub interval: u16,
    /// Data limite inclusiva `AAAA-MM-DD`; vazia = sem data limite.
    pub until: String,
    /// Total de ocorrências, contando a original; de 1 a [`MAX_COUNT`].
    pub count: u16,
}

impl Default for Recurrence {
    /// Semanal, toda semana, quatro ocorrências (a original e três cópias).
    fn default() -> Self {
        Self {
            frequency: Frequency::Weekly,
            interval: 1,
            until: String::new(),
            count: 4,
        }
    }
}

fn parse_date(text: &str, what: &str) -> Result<NaiveDate> {
    let date = NaiveDate::parse_from_str(text, DATE_FORMAT).ok();
    match date {
        Some(d) if text.len() == 10 && d.format(DATE_FORMAT).to_string() == text => Ok(d),
        _ => bail!("{what} inválida: “{text}”. Use o formato AAAA-MM-DD."),
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|first| first.pred_opt())
        .map_or(28, |d| d.day())
}

impl Recurrence {
    /// Confere a regra sem precisar de âncora.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=self.frequency.max_interval()).contains(&self.interval),
            "O intervalo da repetição deve ficar entre 1 e {} ({}).",
            self.frequency.max_interval(),
            self.frequency.label().to_lowercase()
        );
        ensure!(
            (1..=MAX_COUNT).contains(&self.count),
            "O número de ocorrências deve ficar entre 1 e {MAX_COUNT}."
        );
        if !self.until.is_empty() {
            let until = parse_date(&self.until, "A data final da repetição")?;
            ensure!(until.year() >= 1, "A data final da repetição é inválida.");
        }
        Ok(())
    }

    /// A `k`-ésima ocorrência (`k = 0` é a âncora), calculada sempre da âncora.
    /// `None` se passar do ano 9999 ou estourar o calendário.
    fn nth(&self, anchor: NaiveDate, k: u32) -> Option<NaiveDate> {
        let steps = u64::from(k) * u64::from(self.interval);
        let date = match self.frequency {
            Frequency::Daily => anchor.checked_add_signed(Duration::days(i64::try_from(steps).ok()?)),
            Frequency::Weekly => {
                anchor.checked_add_signed(Duration::days(i64::try_from(steps.checked_mul(7)?).ok()?))
            }
            Frequency::Monthly | Frequency::Yearly => {
                let months = if self.frequency == Frequency::Yearly { steps.checked_mul(12)? } else { steps };
                let total = u64::from(anchor.month0()) + months;
                let year = i64::from(anchor.year()) + i64::try_from(total / 12).ok()?;
                let year = i32::try_from(year).ok()?;
                let month = u32::try_from(total % 12).ok()? + 1;
                let day = anchor.day().min(last_day_of_month(year, month));
                NaiveDate::from_ymd_opt(year, month, day)
            }
        }?;
        (date.year() <= MAX_YEAR).then_some(date)
    }

    /// Todas as datas da regra a partir da âncora, em ordem crescente. A
    /// primeira é sempre a própria âncora. Para em `count` ocorrências ou ao
    /// passar de `until` (inclusivo), o que vier antes; ao passar do ano 9999
    /// a lista termina ali.
    pub fn dates(&self, anchor: NaiveDate) -> Result<Vec<NaiveDate>> {
        self.validate()?;
        ensure!(
            anchor.year() >= 1 && anchor.year() <= MAX_YEAR,
            "A data inicial da repetição está fora do intervalo permitido."
        );
        let limit = if self.until.is_empty() {
            None
        } else {
            let until = parse_date(&self.until, "A data final da repetição")?;
            ensure!(
                until >= anchor,
                "A data final da repetição não pode ser anterior à primeira ocorrência."
            );
            Some(until)
        };
        let mut dates = Vec::with_capacity(usize::from(self.count));
        for k in 0..u32::from(self.count) {
            let Some(date) = self.nth(anchor, k) else { break };
            if limit.is_some_and(|until| date > until) {
                break;
            }
            dates.push(date);
        }
        Ok(dates)
    }
}

fn shift_date(text: &str, delta: Duration, what: &str) -> Result<String> {
    if text.is_empty() {
        return Ok(String::new());
    }
    let date = parse_date(text, what)?;
    let shifted = date
        .checked_add_signed(delta)
        .filter(|d| d.year() >= 1 && d.year() <= MAX_YEAR)
        .with_context(|| format!("{what} ficaria fora do intervalo permitido."))?;
    Ok(shifted.format(DATE_FORMAT).to_string())
}

fn shift_moment(text: &str, delta: Duration, what: &str) -> Result<String> {
    let moment = NaiveDateTime::parse_from_str(text, MOMENT_FORMAT).ok();
    let moment = match moment {
        Some(m) if text.len() == 16 && m.format(MOMENT_FORMAT).to_string() == text => m,
        _ => bail!("{what} inválido: “{text}”. Use AAAA-MM-DDTHH:MM."),
    };
    let shifted = moment
        .checked_add_signed(delta)
        .filter(|m| m.year() >= 1 && m.year() <= MAX_YEAR)
        .with_context(|| format!("{what} ficaria fora do intervalo permitido."))?;
    Ok(shifted.format(MOMENT_FORMAT).to_string())
}

fn new_task_uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn new_use_uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Data-âncora da tarefa: o prazo; sem prazo, o dia do início do agendamento.
fn task_anchor(task: &Task) -> Result<NaiveDate> {
    if !task.due_date.is_empty() {
        parse_date(&task.due_date, "O prazo da tarefa")
    } else if !task.scheduled_start.is_empty() {
        let moment = NaiveDateTime::parse_from_str(&task.scheduled_start, MOMENT_FORMAT)
            .ok()
            .filter(|m| m.format(MOMENT_FORMAT).to_string() == task.scheduled_start)
            .with_context(|| "O agendamento da tarefa é inválido. Use AAAA-MM-DDTHH:MM.")?;
        Ok(moment.date())
    } else {
        bail!("Para repetir a tarefa, informe um prazo ou um agendamento.")
    }
}

/// As cópias **novas** de `task` segundo `rule` (a tarefa original é a
/// primeira ocorrência e não entra no resultado).
///
/// A âncora é o prazo; sem prazo, o dia do início do agendamento. Prazo,
/// agendamento (início e fim) e data do lembrete são deslocados pelo mesmo
/// número de dias que separa a ocorrência da âncora, então as distâncias
/// entre eles se mantêm. Cada cópia sai: `id = 0`, `uid` novo, situação
/// "pendente", sem conclusão/desfecho, checklist desmarcado, sem dependências,
/// sem tarefa-mãe e sem `proposal_id`. Mantém título, ação, prioridade,
/// documento, `due_source`, responsável, observações, estimativa, avisos e
/// categorias. Não grava nada.
pub fn task_occurrences(task: &Task, rule: &Recurrence) -> Result<Vec<Task>> {
    let anchor = task_anchor(task)?;
    let dates = rule.dates(anchor)?;
    let mut copies = Vec::with_capacity(dates.len().saturating_sub(1));
    for date in dates.into_iter().skip(1) {
        let delta = date.signed_duration_since(anchor);
        let mut copy = task.clone();
        copy.id = 0;
        copy.uid = new_task_uid();
        copy.status = "pendente".into();
        copy.completed_at = None;
        copy.outcome = String::new();
        copy.parent_task_id = None;
        copy.dependencies = Vec::new();
        copy.proposal_id = String::new();
        for item in &mut copy.checklist {
            item.0 = false;
        }
        copy.due_date = shift_date(&task.due_date, delta, "O prazo da tarefa")?;
        copy.reminder_date = shift_date(&task.reminder_date, delta, "A data do lembrete")?;
        if !task.scheduled_start.is_empty() {
            copy.scheduled_start = shift_moment(&task.scheduled_start, delta, "O início do agendamento")?;
            copy.scheduled_end = shift_moment(&task.scheduled_end, delta, "O fim do agendamento")?;
        }
        copies.push(copy);
    }
    Ok(copies)
}

/// As reservas/empréstimos **novos** de `usage` segundo `rule` (a original é
/// a primeira ocorrência e não entra no resultado).
///
/// A âncora é o dia de `start`. `start` e `end` deslocam-se pelo mesmo número
/// de dias (a duração e a hora do dia se mantêm; um `end` no dia seguinte
/// continua no dia seguinte). Cada cópia sai com `uid` novo, situação
/// "reservado", sem devolução, sem `task_id`/`proposal_id` e sem os campos
/// que o repositório preenche (`created_by`, `created_at`, `updated_at`).
/// Não verifica conflitos de horário: isso é da prévia/gravação, que conhece
/// as demais reservas.
pub fn resource_occurrences(usage: &ResourceUse, rule: &Recurrence) -> Result<Vec<ResourceUse>> {
    let start = parse_moment(&usage.start)?;
    parse_moment(&usage.end)?;
    let anchor = start.date();
    let dates = rule.dates(anchor)?;
    let mut copies = Vec::with_capacity(dates.len().saturating_sub(1));
    for date in dates.into_iter().skip(1) {
        let delta = date.signed_duration_since(anchor);
        let mut copy = usage.clone();
        copy.uid = new_use_uid();
        copy.start = shift_moment(&usage.start, delta, "O início da reserva")?;
        copy.end = shift_moment(&usage.end, delta, "O fim da reserva")?;
        copy.returned_at = None;
        copy.status = UseStatus::Reserved;
        copy.task_id = None;
        copy.proposal_id = String::new();
        copy.created_by = String::new();
        copy.created_at = String::new();
        copy.updated_at = String::new();
        copies.push(copy);
    }
    Ok(copies)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn rule(frequency: Frequency, interval: u16, count: u16) -> Recurrence {
        Recurrence { frequency, interval, until: String::new(), count }
    }

    fn text(dates: &[NaiveDate]) -> Vec<String> {
        dates.iter().map(|d| d.format(DATE_FORMAT).to_string()).collect()
    }

    #[test]
    fn default_rule_is_valid() {
        let r = Recurrence::default();
        assert!(r.validate().is_ok());
        assert_eq!(r.dates(d(2026, 11, 17)).unwrap().len(), 4);
    }

    #[test]
    fn serde_fills_missing_fields() {
        let r: Recurrence = serde_json::from_str(r#"{"frequency":"monthly"}"#).unwrap();
        assert_eq!(r.frequency, Frequency::Monthly);
        assert_eq!((r.interval, r.count), (1, 4));
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Recurrence>(&json).unwrap(), r);
    }

    #[test]
    fn daily_and_weekly_cross_months_and_years() {
        let r = rule(Frequency::Daily, 2, 4);
        assert_eq!(text(&r.dates(d(2026, 12, 30)).unwrap()), ["2026-12-30", "2027-01-01", "2027-01-03", "2027-01-05"]);
        let r = rule(Frequency::Weekly, 2, 3);
        assert_eq!(text(&r.dates(d(2026, 2, 20)).unwrap()), ["2026-02-20", "2026-03-06", "2026-03-20"]);
    }

    #[test]
    fn monthly_keeps_original_day_without_drift() {
        let r = rule(Frequency::Monthly, 1, 5);
        assert_eq!(
            text(&r.dates(d(2027, 1, 31)).unwrap()),
            ["2027-01-31", "2027-02-28", "2027-03-31", "2027-04-30", "2027-05-31"]
        );
        // Ano bissexto: fevereiro de 2028 tem 29.
        assert_eq!(
            text(&r.dates(d(2028, 1, 31)).unwrap())[..3],
            ["2028-01-31", "2028-02-29", "2028-03-31"]
        );
    }

    #[test]
    fn monthly_interval_crosses_year_end() {
        let r = rule(Frequency::Monthly, 5, 4);
        assert_eq!(text(&r.dates(d(2026, 10, 30)).unwrap()), ["2026-10-30", "2027-03-30", "2027-08-30", "2028-01-30"]);
    }

    #[test]
    fn yearly_leap_day_returns_when_possible() {
        let r = rule(Frequency::Yearly, 1, 5);
        assert_eq!(
            text(&r.dates(d(2028, 2, 29)).unwrap()),
            ["2028-02-29", "2029-02-28", "2030-02-28", "2031-02-28", "2032-02-29"]
        );
    }

    #[test]
    fn until_is_inclusive_and_competes_with_count() {
        let mut r = rule(Frequency::Weekly, 1, 10);
        r.until = "2026-12-01".into();
        assert_eq!(text(&r.dates(d(2026, 11, 17)).unwrap()), ["2026-11-17", "2026-11-24", "2026-12-01"]);
        r.until = "2026-11-17".into();
        assert_eq!(r.dates(d(2026, 11, 17)).unwrap(), vec![d(2026, 11, 17)]);
        r.until = "2030-01-01".into();
        r.count = 2;
        assert_eq!(r.dates(d(2026, 11, 17)).unwrap().len(), 2);
    }

    #[test]
    fn bounds_are_rejected() {
        let a = d(2026, 11, 17);
        for bad in [
            rule(Frequency::Weekly, 0, 3),
            rule(Frequency::Weekly, 105, 3),
            rule(Frequency::Daily, 366, 3),
            rule(Frequency::Monthly, 25, 3),
            rule(Frequency::Yearly, 11, 3),
            rule(Frequency::Weekly, 1, 0),
            rule(Frequency::Weekly, 1, 105),
        ] {
            assert!(bad.dates(a).is_err(), "{bad:?}");
        }
        assert!(rule(Frequency::Daily, 365, 104).validate().is_ok());
        for until in ["2026-02-30", "2026-1-5", "17/11/2026", " 2026-11-17", "2026-11-17T10:00"] {
            let mut r = rule(Frequency::Weekly, 1, 3);
            r.until = until.into();
            assert!(r.validate().is_err(), "{until}");
        }
        let mut r = rule(Frequency::Weekly, 1, 3);
        r.until = "2026-11-16".into();
        assert!(r.dates(a).is_err(), "until antes da âncora");
    }

    #[test]
    fn stops_at_year_9999() {
        let r = rule(Frequency::Yearly, 10, 5);
        let dates = r.dates(d(9980, 6, 1)).unwrap();
        assert_eq!(text(&dates), ["9980-06-01", "9990-06-01"]);
        let r = rule(Frequency::Daily, 1, 5);
        assert_eq!(r.dates(d(9999, 12, 30)).unwrap().len(), 2);
    }

    fn sample_task() -> Task {
        let mut t = Task::new(7);
        t.id = 42;
        t.title = "Responder ofício".into();
        t.status = "concluida".into();
        t.completed_at = Some("2026-11-20T10:00:00-03:00".into());
        t.outcome = "feito".into();
        t.due_date = "2026-01-31".into();
        t.due_source = "Art. 5º".into();
        t.scheduled_start = "2026-01-29T14:00".into();
        t.scheduled_end = "2026-01-29T15:30".into();
        t.reminder_date = "2026-01-30".into();
        t.dependencies = vec![1, 2];
        t.parent_task_id = Some(3);
        t.proposal_id = "p".into();
        t.checklist = vec![(true, "a".into()), (false, "b".into())];
        t.categories = vec!["c".repeat(32)];
        t
    }

    #[test]
    fn task_copies_shift_by_same_delta_without_drift() {
        let t = sample_task();
        let copies = task_occurrences(&t, &rule(Frequency::Monthly, 1, 3)).unwrap();
        assert_eq!(copies.len(), 2);
        // Fev: delta de 28 dias a partir de 31/01.
        let c = &copies[0];
        assert_eq!(c.due_date, "2026-02-28");
        assert_eq!(c.scheduled_start, "2026-02-26T14:00");
        assert_eq!(c.scheduled_end, "2026-02-26T15:30");
        assert_eq!(c.reminder_date, "2026-02-27");
        // Mar: calculado da âncora, não de fevereiro.
        let c = &copies[1];
        assert_eq!(c.due_date, "2026-03-31");
        assert_eq!(c.scheduled_start, "2026-03-29T14:00");
        assert_eq!(c.reminder_date, "2026-03-30");
        for c in &copies {
            assert!(c.validate().is_ok(), "{c:?}");
        }
    }

    #[test]
    fn task_copies_are_fresh() {
        let t = sample_task();
        let copies = task_occurrences(&t, &rule(Frequency::Weekly, 1, 3)).unwrap();
        assert_eq!(copies.len(), 2);
        assert_ne!(copies[0].uid, copies[1].uid);
        for c in &copies {
            assert_eq!(c.id, 0);
            assert_ne!(c.uid, t.uid);
            assert!(!c.uid.is_empty());
            assert_eq!(c.status, "pendente");
            assert_eq!(c.completed_at, None);
            assert!(c.outcome.is_empty());
            assert!(c.dependencies.is_empty());
            assert_eq!(c.parent_task_id, None);
            assert!(c.proposal_id.is_empty());
            assert_eq!(c.checklist, vec![(false, "a".into()), (false, "b".into())]);
            assert_eq!(c.document_id, Some(7));
            assert_eq!(c.due_source, "Art. 5º");
            assert_eq!(c.categories, vec!["c".repeat(32)]);
            assert_eq!(c.title, t.title);
        }
        // A original não é alterada.
        assert_eq!(t.id, 42);
        assert_eq!(t.status, "concluida");
    }

    #[test]
    fn task_anchor_falls_back_to_scheduled_start() {
        let mut t = sample_task();
        t.due_date.clear();
        t.reminder_date.clear();
        let copies = task_occurrences(&t, &rule(Frequency::Daily, 3, 2)).unwrap();
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].due_date, "");
        assert_eq!(copies[0].scheduled_start, "2026-02-01T14:00");
        assert_eq!(copies[0].scheduled_end, "2026-02-01T15:30");
        assert!(copies[0].validate().is_ok());
    }

    #[test]
    fn task_without_date_or_with_one_occurrence() {
        let mut t = sample_task();
        assert!(task_occurrences(&t, &rule(Frequency::Weekly, 1, 1)).unwrap().is_empty());
        t.due_date.clear();
        t.scheduled_start.clear();
        t.scheduled_end.clear();
        assert!(task_occurrences(&t, &Recurrence::default()).is_err());
        t.due_date = "2026-02-30".into();
        assert!(task_occurrences(&t, &Recurrence::default()).is_err());
    }

    #[test]
    fn task_shift_beyond_year_9999_fails_instead_of_wrapping() {
        let mut t = sample_task();
        t.due_date = "9999-12-30".into();
        t.scheduled_start.clear();
        t.scheduled_end.clear();
        t.reminder_date = "9999-12-31".into();
        // A data da ocorrência cabe (31/12), mas o lembrete ultrapassaria 9999.
        assert!(task_occurrences(&t, &rule(Frequency::Daily, 1, 2)).is_err());
    }

    fn sample_use() -> ResourceUse {
        ResourceUse {
            uid: "orig".into(),
            resource_uid: "sala".into(),
            who: "Ana".into(),
            start: "2027-01-31T22:00".into(),
            end: "2027-02-01T01:00".into(),
            returned_at: Some("2027-02-01T00:50".into()),
            status: UseStatus::Returned,
            document_id: Some(9),
            task_id: Some(5),
            proposal_id: "p".into(),
            created_by: "x".into(),
            created_at: "c".into(),
            updated_at: "u".into(),
            ..ResourceUse::default()
        }
    }

    #[test]
    fn use_copies_shift_and_reset() {
        let u = sample_use();
        let copies = resource_occurrences(&u, &rule(Frequency::Monthly, 1, 3)).unwrap();
        assert_eq!(copies.len(), 2);
        assert_eq!((copies[0].start.as_str(), copies[0].end.as_str()), ("2027-02-28T22:00", "2027-03-01T01:00"));
        assert_eq!((copies[1].start.as_str(), copies[1].end.as_str()), ("2027-03-31T22:00", "2027-04-01T01:00"));
        for c in &copies {
            assert_ne!(c.uid, u.uid);
            assert!(!c.uid.is_empty());
            assert_eq!(c.status, UseStatus::Reserved);
            assert_eq!(c.returned_at, None);
            assert_eq!(c.task_id, None);
            assert!(c.proposal_id.is_empty() && c.created_by.is_empty());
            assert!(c.created_at.is_empty() && c.updated_at.is_empty());
            assert_eq!((c.document_id, c.resource_uid.as_str(), c.who.as_str()), (Some(9), "sala", "Ana"));
        }
        assert_ne!(copies[0].uid, copies[1].uid);
    }

    #[test]
    fn use_with_invalid_moment_fails() {
        let mut u = sample_use();
        u.start = "2027-01-31".into();
        assert!(resource_occurrences(&u, &Recurrence::default()).is_err());
    }
}
