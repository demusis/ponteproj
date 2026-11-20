package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
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

    // ------------------------------------------------------------------ Projetos

    private val longName = "Mudança da sede para o novo prédio da Coordenação-Geral de Planejamento e Orçamento"
    private val projetos = listOf(
        ProjectItem(
            "p1", longName, "Levar todos os setores para o prédio novo até o fim do mês.", "Maria Aparecida dos Santos Oliveira",
            "ativo", "Ativo", "2026-11-01", "2026-11-24", 7,
            ProjectProgress(8, 3, 2, "2026-11-10"), ProjectRisk(1, "2026-11-25T18:00")
        ),
        ProjectItem(
            "p2", "Auditoria interna", "Revisar os contratos de 2026.", "Ana", "ativo", "Ativo", "", "2026-12-15", 2,
            ProjectProgress(5, 5, 0, "")
        ),
        ProjectItem(
            "p3", "Revisão do plano de contratações 2027", "Consolidar as demandas dos setores.", "Servidor A", "ativo", "Ativo",
            "2026-11-03", "2026-12-20", 1, ProjectProgress(12, 4, 0, "2026-11-21"), ProjectRisk(3, "")
        ),
        ProjectItem("p4", "Campanha de vacinação", "", "", "pausado", "Pausado", "", "", 1, ProjectProgress()),
        ProjectItem("p5", "Inventário de bens", "Conferir os bens de cada sala.", "Carlos", "concluido", "Concluído", "2026-08-01", "2026-10-30", 4, ProjectProgress(6, 6, 0, "")),
        ProjectItem("p6", "Arquivo morto", "", "", "arquivado", "Arquivado", "", "", 3, ProjectProgress(2, 1, 0, ""))
    )

    private fun projetoTask(id: Long, title: String, action: String, number: String, due: String, chip: String, done: Boolean = false, overdue: Boolean = false) =
        TaskItem(id, if (number.isEmpty()) 0 else 100 + id, title, action, number, due, chip, done, overdue, false,
            bucket = if (done) "concluidas" else if (overdue) "atrasadas" else if (due.isEmpty()) "sem_prazo" else "proximas",
            projectUid = "p1", projectName = longName)

    private val tarefasDoProjeto = listOf(
        projetoTask(21, "Conferir a planilha de veículos do transporte", "Analisar", "PROC. 2026/5512", "2026-11-10", "há 9 dias", overdue = true),
        projetoTask(22, "Reservar a sala para a reunião", "", "", "2026-11-12", "há 7 dias", overdue = true),
        projetoTask(23, "Contratar a transportadora", "Encaminhar", "MEM. 118/2026", "2026-11-24", "24/11"),
        projetoTask(24, "Preparar minuta de resposta ao Setor Jurídico", "Elaborar minuta", "OF. 0398/2026", "2026-11-30", "30/11"),
        projetoTask(25, "Arquivar comprovantes", "Arquivar", "", "", ""),
        projetoTask(26, "Assinar termo de recebimento", "Assinar", "TERMO 22/2026", "2026-11-03", "03/11", done = true),
        projetoTask(27, "Encaminhar escala de trabalho", "", "", "", "", done = true)
    )

    private val linhaDoTempo = listOf(
        TimelineItem(23, "u23", "Contratar a transportadora", false, "2026-11-16T09:00", "2026-11-18T18:00", false, "2026-11-24", false),
        TimelineItem(24, "u24", "Preparar minuta de resposta ao Setor Jurídico", false, "2026-11-19T09:00", "2026-11-23T18:00", false, "2026-11-30", false),
        TimelineItem(21, "u21", "Conferir a planilha de veículos do transporte", false, "2026-11-12T09:00", "2026-11-17T18:00", false, "2026-11-10", true),
        TimelineItem(28, "u28", "Mover os móveis", false, "2026-11-24T09:00", "2026-11-26T18:00", true, "2026-11-24", true),
        TimelineItem(29, "u29", "Revisar", false, "2026-11-27T09:00", "2026-11-27T18:00", true, "", false),
        TimelineItem(26, "u26", "Assinar termo de recebimento", true, "2026-11-10T09:00", "2026-11-11T18:00", false, "2026-11-03", false),
        TimelineItem(22, "u22", "Reservar a sala para a reunião", false, due = "2026-11-12", late = true),
        TimelineItem(27, "u27", "Encaminhar escala de trabalho", true)
    )

    private val detalhe = ProjectDetail(projetos[0], tarefasDoProjeto, linhaDoTempo, TimelineSpan("2026-11-10T00:00", "2026-12-01T00:00"))
    private val hoje = LocalDate.of(2026, 11, 19)

    /** The Tarefas area with Projetos chosen, as on the phone. */
    private fun projetosLista(state: ProjectsState, filter: String, query: String, name: String, dark: Boolean) {
        narrow(1920)
        sliced(name, dark) {
            MainShell(Samples.shell.copy(tab = 1, projects = true), {}, {}, {}, {}) {
                Column(Modifier.fillMaxSize()) {
                    SegmentedTabs(TasksSegments.labels(), TasksSegments.position(TasksSegments.PROJECTS), {})
                    Box(Modifier.weight(1f).fillMaxWidth()) { ProjectsScreen(state, filter, {}, query, {}, {}, {}) }
                }
            }
        }
    }

    @Test fun projetosListaAtivos() = projetosLista(ProjectsState(false, null, projetos), "ativo", "", "ativos", false)
    @Test fun projetosListaTodosEscuro() = projetosLista(ProjectsState(false, null, projetos), "todos", "", "todos_escuro", true)
    @Test fun projetosListaVazia() = projetosLista(ProjectsState(false, null, emptyList()), "ativo", "", "vazia", false)
    @Test fun projetosListaVaziaEscuro() = projetosLista(ProjectsState(false, null, emptyList()), "ativo", "", "vazia_escuro", true)
    @Test fun projetosListaSemResultado() = projetosLista(ProjectsState(false, null, projetos), "arquivado", "", "sem_resultado", false)
    @Test fun projetosListaBuscaSemResultado() = projetosLista(ProjectsState(false, null, projetos), "todos", "xyz", "busca", false)

    private fun projeto(tab: Int, name: String, dark: Boolean, detail: ProjectDetail = detalhe, heightPx: Int = 3840) {
        narrow(heightPx)
        sliced(name, dark) {
            ProjectScreen(
                detail.project.uid, ProjectViewState(detail.project.uid, loading = false, detail = detail),
                {}, {}, {}, {}, { _, _ -> }, {}, {}, {}, {}, today = hoje, initialTab = tab
            )
        }
    }

    @Test fun projetoTarefas() = projeto(0, "tarefas", false)
    @Test fun projetoTarefasEscuro() = projeto(0, "tarefas_escuro", true)
    @Test fun projetoLinhaDoTempo() = projeto(1, "linha", false, heightPx = 3840)
    @Test fun projetoLinhaDoTempoEscuro() = projeto(1, "linha_escuro", true, heightPx = 3840)

    private val projetoSimples = ProjectDetail(projetos[1], emptyList(), emptyList(), null)

    @Test fun projetoSemTarefas() = projeto(0, "sem_tarefas", false, projetoSimples, 1920)
    @Test fun projetoSemTarefasLinhaDoTempo() = projeto(1, "sem_tarefas_linha", false, projetoSimples, 1920)

    private fun formulario(project: ProjectItem?, state: ProjectEditState, name: String, dark: Boolean) {
        narrow(1920)
        sliced(name, dark) {
            ProjectFormScreen(
                project?.uid ?: "", project, false, state, listOf("Maria Aparecida", "Ana", "Servidor A"), "Servidor A", {}, { _, _ -> }, {}
            )
        }
    }

    @Test fun projetoFormularioNovo() = formulario(null, ProjectEditState(), "novo", false)
    @Test fun projetoFormularioEditar() = formulario(projetos[0], ProjectEditState(), "editar", false)
    @Test fun projetoFormularioEditarEscuro() = formulario(projetos[0], ProjectEditState(), "editar_escuro", true)
    @Test fun projetoFormularioErro() =
        formulario(projetos[0], ProjectEditState(error = "Já existe um projeto com esse nome."), "erro", false)
    @Test fun projetoFormularioConflito() =
        formulario(projetos[0], ProjectEditState(error = ProjectRules.CHANGED, conflict = true), "conflito", true)

    private fun incluir(tasks: List<TaskItem>, state: SaveState, name: String, dark: Boolean) {
        narrow(1920)
        sliced(name, dark) { ProjectIncludeScreen(longName, tasks, state, {}, {}) }
    }

    @Test fun projetoIncluirTarefas() = incluir(Samples.tasks, SaveState(), "incluir", false)
    @Test fun projetoIncluirTarefasEscuro() = incluir(Samples.tasks, SaveState(error = "Não foi possível mudar o projeto da tarefa."), "incluir_escuro", true)
    @Test fun projetoIncluirSemTarefas() = incluir(emptyList(), SaveState(), "incluir_vazio", false)

    // The Projeto field of the task screen, the task list with its project chips and filter, the search.
    private fun tarefaComProjeto(dark: Boolean, name: String) {
        narrow(5760)
        val form = Samples.taskForm.copy(projectUid = "p1")
        val task = TaskDetail(5, 0, "", "", form, false, 3, emptyList(), projectName = longName)
        sliced(name, dark) {
            TaskScreen(
                5, TaskEditState(id = 5, loading = false, task = task), Samples.actions, Samples.priorities, {}, {}, {}, {}, {}, {}, {},
                projects = projetos
            )
        }
    }

    @Test fun tarefaComProjeto() = tarefaComProjeto(false, "claro")
    @Test fun tarefaComProjetoEscuro() = tarefaComProjeto(true, "escuro")

    private val tarefasComProjeto = Samples.tasks.mapIndexed { i, t ->
        when (i) {
            0 -> t.copy(projectUid = "p1", projectName = longName)
            1 -> t.copy(projectUid = "p2", projectName = "Auditoria interna", categories = listOf("u1"))
            3 -> t.copy(projectUid = "p3", projectName = "Revisão do plano de contratações 2027")
            else -> t
        }
    }

    private fun listaDeTarefas(filter: String, name: String, dark: Boolean) {
        narrow(1920)
        sliced(name, dark) {
            androidx.compose.runtime.CompositionLocalProvider(LocalCategories provides listOf(Category("u1", "Urgente", 4))) {
                MainShell(Samples.shell.copy(tab = 1), {}, {}, {}, {}) {
                    Column(Modifier.fillMaxSize()) {
                        SegmentedTabs(TasksSegments.labels(), 0, {})
                        Box(Modifier.weight(1f).fillMaxWidth()) {
                            TasksScreen(
                                TasksState(false, null, tarefasComProjeto), {}, {}, projects = projetos, projectFilter = filter
                            )
                        }
                    }
                }
            }
        }
    }

    @Test fun tarefasComProjetoNaLinha() = listaDeTarefas("", "claro", false)
    @Test fun tarefasFiltradasPorProjetoEscuro() = listaDeTarefas("p1", "filtrada_escuro", true)
    @Test fun tarefasSemProjeto() = listaDeTarefas(ProjectRules.NONE, "sem_projeto", false)

    @Test fun filtroDeProjetoFolha() {
        narrow(1920)
        sliced("folha", false) {
            Box(Modifier.fillMaxSize().background(Ponte.colors.ground)) {
                Box(Modifier.background(Ponte.colors.surface).padding(top = 12.dp)) { ProjectFilterContent(projetos, "p3") {} }
            }
        }
    }

    @Test fun novaTarefaNoProjeto() {
        narrow(3840)
        sliced("nova", false) {
            NewTaskScreen(
                "", Samples.actions.filter { it.key != "triar" }, {}, { _, _, _, _, _, _, _ -> }, withoutDocument = true,
                projects = projetos, projectUid = "p3"
            )
        }
    }

    @Test fun buscaComProjetos() {
        narrow(1920)
        sliced("busca", false) {
            SearchScreen(
                SearchState(query = "audit", result = SearchResult(SearchGroup.none(), SearchGroup(listOf(Samples.tasks[0]), 0), SearchGroup.none())),
                {}, {}, {}, {}, {}, projects = ProjectRules.search(projetos, "audit")
            )
        }
    }

    // The bar of the Tarefas area, with four words, at 360 dp and with a large font.
    @Test fun barraDaAreaComQuatro() {
        narrow(1920)
        sliced("barra", false) {
            Column(Modifier.fillMaxSize().background(Ponte.colors.ground)) {
                for (id in TasksSegments.ORDER) SegmentedTabs(TasksSegments.labels(), TasksSegments.position(id), {})
            }
        }
    }
}
