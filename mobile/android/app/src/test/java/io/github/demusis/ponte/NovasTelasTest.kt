package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.layout.layout
import androidx.compose.ui.unit.Constraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.cash.paparazzi.DeviceConfig
import org.junit.Rule
import org.junit.Test
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime

/** Pictures, at 360dp, of the screens of the daily follow-ups, the holidays, the phone calendar and the week. */
private const val SLICE = 1920

class NovasTelasTest {
    @get:Rule val paparazzi = newPaparazzi()

    /**
     * Paparazzi shrinks a picture taller than about 1000 px, which makes a long screen unreadable. So a long
     * screen is laid out at its full height ([heightPx]) and shown in slices of one phone screen (1920 px, at
     * 360dp wide) that are saved as `name_1`, `name_2`...
     */
    private fun narrow(heightPx: Int) {
        total = heightPx
        paparazzi.unsafeUpdateConfig(deviceConfig = DeviceConfig.NEXUS_5.copy(screenHeight = SLICE))
    }

    private var total = SLICE

    private fun sliced(name: String, dark: Boolean, content: @Composable () -> Unit) {
        val count = (total + SLICE - 1) / SLICE
        for (i in 0 until count) {
            paparazzi.snapshot("${name}_${i + 1}") {
                PonteTheme(dark = dark) {
                    Box(Modifier.fillMaxSize().clipToBounds()) {
                        Box(
                            Modifier.layout { m, c ->
                                val p = m.measure(Constraints.fixed(c.maxWidth, total))
                                layout(c.maxWidth, c.maxHeight) { p.place(0, -i * SLICE) }
                            }
                        ) { content() }
                    }
                }
            }
        }
    }

    private fun settings(state: SettingsState, name: String, dark: Boolean, heightPx: Int = 7680) {
        narrow(heightPx)
        sliced(name, dark) { SettingsScreen(state, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }
    }

    private val google1 = "maria.aparecida.dos.santos.oliveira@servidor.exemplo.gov.br"
    private val google2 = "joao.pedro.trabalho.ponte.2026@gmail.com"
    private val calendars = listOf(
        CalInfo(1, "Ponte", google1, color = 0xFF3F51B5.toInt()),
        CalInfo(2, "Agenda de reuniões do Comitê Gestor e das Diretorias", google1, color = 0xFF0B8043.toInt(), syncEvents = false),
        CalInfo(3, "Tarefas pessoais", google2, color = 0xFFD50000.toInt(), visible = false),
        CalInfo(4, "Ponte", google2),
        CalInfo(5, "Minha agenda", "Telefone", accountType = "LOCAL", color = 0xFFF4B400.toInt())
    )
    private val base = SettingsState(version = "0.3.0", remembered = true, fileName = "ponte-acervo.ponte", fileSize = 1_200_000)

    @Test fun agendaDesligada() = settings(base, "desligada", false, 7680)
    @Test fun agendaDesligadaEscuro() = settings(base, "desligada_escuro", true, 7680)

    @Test fun agendaSemAgenda() = settings(base.copy(calendarOn = true), "sem_agenda", false, 7680)
    @Test fun agendaSemAgendaEscuro() = settings(base.copy(calendarOn = true), "sem_agenda_escuro", true, 7680)

    private val agendaCheia = base.copy(
        calendarOn = true, calendars = calendars, calendarChosen = "1",
        calendarStatus = "Última conferência: hoje, 14:32 · 12 tarefas na agenda",
        calendarWarning = "A agenda “Agenda de reuniões do Comitê Gestor e das Diretorias” deixou de aceitar eventos; escolha outra."
    )

    @Test fun agendaCheia() = settings(agendaCheia, "cheia", false, 7680)
    @Test fun agendaCheiaEscuro() = settings(agendaCheia, "cheia_escuro", true, 7680)
    @Test fun agendaAtualizando() = settings(agendaCheia.copy(calendarRefreshing = true, calendarChosen = ""), "atualizando", false, 7680)
    @Test fun agendaAtualizandoEscuro() = settings(agendaCheia.copy(calendarRefreshing = true, calendarChosen = ""), "atualizando_escuro", true, 7680)

    private val customDays = listOf(
        CustomDay(null, 12, 24, "Recesso"),
        CustomDay(2026, 12, 31, "Ponto facultativo de fim de ano para todos os servidores da Coordenação-Geral de Planejamento e Orçamento"),
        CustomDay(2027, 3, 19, "")
    )

    @Test fun feriadosSemEstado() = settings(base, "sem_estado", false, 7680)
    @Test fun feriadosSemEstadoEscuro() = settings(base, "sem_estado_escuro", true, 7680)
    @Test fun feriadosTocantins() = settings(base.copy(holidayUf = "TO", holidayDays = customDays), "tocantins", false, 7680)
    @Test fun feriadosTocantinsEscuro() = settings(base.copy(holidayUf = "TO", holidayDays = customDays), "tocantins_escuro", true, 7680)
    @Test fun feriadosEspiritoSanto() = settings(base.copy(holidayUf = "ES"), "espirito_santo", false, 7680)
    @Test fun feriadosEspiritoSantoEscuro() = settings(base.copy(holidayUf = "ES"), "espirito_santo_escuro", true, 7680)

    // Hoje

    private val risks = listOf(
        RiskItem(1, "Responder ofício sobre o pessoal do Projeto Alfa e a lotação provisória", "2026-11-26", "late_planned", "2026-11-30T18:00", 0, ""),
        RiskItem(2, "Enviar relatório mensal", "2026-11-27", "late_projected", "2026-11-30T18:00", 5, "Conferir planilha de veículos"),
        RiskItem(3, "Preparar minuta de resposta", "2026-11-28", "missing_plan", "", 0, ""),
        RiskItem(4, "Arquivar comprovante", "2026-11-30", "missing_estimate", "", 0, "")
    )

    private fun week(): List<WeekDay> {
        val monday = WeekPlan.mondayOf(LocalDate.now())
        val thu = monday.plusDays(3)
        val e1 = WeekEntry(
            "t1", "Preparar minuta de resposta ao Setor Jurídico sobre a lotação", WeekKind.TASK,
            start = LocalDateTime.of(thu, LocalTime.of(9, 0)), end = LocalDateTime.of(thu, LocalTime.of(12, 0)),
            dependencyNotice = "⚠ começa antes de terminar Conferir planilha de veículos", dependencyWarning = true, taskId = 1
        )
        val e2 = WeekEntry(
            "t2", "Enviar relatório", WeekKind.TASK, start = LocalDateTime.of(thu, LocalTime.of(14, 0)),
            end = LocalDateTime.of(thu, LocalTime.of(18, 0)), dependencyNotice = "↳ depende de 2", taskId = 2
        )
        val d = WeekEntry("d1", "Responder ofício sobre o pessoal do Projeto Alfa", WeekKind.DEADLINE, day = thu, taskId = 3)
        return WeekPlan.days(monday, listOf(e1, e2, d))
    }

    private fun hoje(state: TodayState, name: String, dark: Boolean, heightPx: Int, risks: List<RiskItem>, week: List<WeekDay>, followUps: Int) {
        narrow(heightPx)
        sliced(name, dark) { MainShell(Samples.shell.copy(tab = 0, badge = 4), {}, {}, {}, {}) {
            TodayScreen(
                state, {}, {}, week = week, weekStart = WeekPlan.mondayOf(LocalDate.now()), risks = risks,
                followUpCount = followUps
            )
        } }
    }

    @Test fun hojeCompleto() = hoje(Samples.today, "completo", false, 5760, risks, week(), 3)
    @Test fun hojeCompletoEscuro() = hoje(Samples.today, "completo_escuro", true, 5760, risks, week(), 3)
    @Test fun hojeVazioComCobranca() = hoje(TodayState(loading = false), "vazio", false, 1920, emptyList(), emptyList(), 2)
    @Test fun hojeVazioComCobrancaEscuro() = hoje(TodayState(loading = false), "vazio_escuro", true, 1920, emptyList(), emptyList(), 2)

    // Atendimentos > Cobranças de hoje

    private val chase = listOf(
        AttendanceItem(
            1, "ORG-OFI-2026/00431", "Pedido de apoio logístico para o evento de encerramento do exercício com as delegações estaduais e municipais",
            "Aguardando resposta", "2026-11-06", "há 4 dias", true, assignee = "Maria Aparecida dos Santos Oliveira",
            followUp = "Cobrança marcada para 16/11 (3 dias de atraso)", followUpDays = 3
        ),
        AttendanceItem(
            2, "Consulta sobre o quadro de pessoal da Coordenação-Geral", "Consulta sobre o quadro de pessoal da Coordenação-Geral",
            "Em análise", "", "", false, followUp = "Cobrança marcada para hoje", followUpDays = 0
        )
    )

    private fun fila(dark: Boolean, name: String) {
        narrow(1920)
        sliced(name, dark) { MainShell(Samples.shell.copy(tab = 1), {}, {}, {}, {}) {
            AttendancesScreen(AttendancesState(loading = false, rows = Samples.attendances + chase, followUps = chase), "", {}, {})
        } }
    }

    @Test fun filaCobrancas() = fila(false, "fila")
    @Test fun filaCobrancasEscuro() = fila(true, "fila_escuro")

    @Test fun janelaCobranca() {
        narrow(1920)
        sliced("janela", false) { FollowUpDialog(chase[0], true, listOf("Cobrado por telefone", "Cobrado por e-mail"), {}, { _, _ -> }) }
    }

    @Test fun janelaCobrancaEscuro() {
        narrow(1920)
        sliced("janela_escuro", true) { FollowUpDialog(chase[0], true, listOf("Cobrado por telefone"), {}, { _, _ -> }) }
    }

    // Planejamento

    private fun planejamento(dark: Boolean, name: String) {
        narrow(1920)
        sliced(name, dark) {
            Column(Modifier.fillMaxSize().background(Ponte.colors.ground).padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                PlanningFields(
                    due = "2026-11-26", start = "2026-11-24T09:00", end = "2026-11-30T18:00", estimateMinutes = 5 * TaskRules.MINUTES_PER_DAY,
                    startHour = "09:00", endHour = "18:00", daysText = "5", onStartHour = {}, onEndHour = {}, onDaysText = {}, onChange = { _, _, _ -> }
                )
            }
        }
    }

    @Test fun planejamento() = planejamento(false, "claro")
    @Test fun planejamentoEscuro() = planejamento(true, "escuro")

    // The whole task screen with a plan, a deadline before the planned end and prerequisites.
    private fun tarefa(dark: Boolean, name: String) {
        narrow(5760)
        val form = Samples.taskForm.copy(
            dueDate = "2026-11-26", scheduledStart = "2026-11-24T09:00", scheduledEnd = "2026-11-30T18:00",
            estimateMinutes = 5 * TaskRules.MINUTES_PER_DAY, dependencies = listOf(3L, 1L, 8L)
        )
        val task = TaskDetail(5, 0, "", "", form, false, 3, emptyList())
        val tasks = Samples.tasks + Samples.tasks[0].copy(id = 12, title = "Conferir o quadro de pessoal da Coordenação-Geral de Planejamento e Orçamento")
        sliced(name, dark) {
            TaskScreen(
                5, TaskEditState(id = 5, loading = false, task = task), Samples.actions, Samples.priorities, {}, {}, {}, {}, {}, {}, {},
                relations = TasksState(loading = false, tasks = tasks)
            )
        }
    }

    @Test fun tarefaComPlanoEDependencias() = tarefa(false, "claro")
    @Test fun tarefaComPlanoEDependenciasEscuro() = tarefa(true, "escuro")
}
