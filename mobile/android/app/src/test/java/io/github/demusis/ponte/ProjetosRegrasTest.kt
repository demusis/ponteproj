package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate

/** The rules of the Projetos screens: list, labels, the Projeto field, the form and the timeline numbers (no Android). */
class ProjetosRegrasTest {
    private fun project(
        uid: String, name: String, status: String = ProjectRules.ACTIVE, objective: String = "", assignee: String = "",
        target: String = "", progress: ProjectProgress = ProjectProgress(), risk: ProjectRisk = ProjectRisk()
    ) = ProjectItem(uid, name, objective, assignee, status, "", "", target, 1, progress, risk)

    private val rows = listOf(
        project("a", "Auditoria interna", objective = "Revisar os contratos de 2026"),
        project("b", "Mudança da sede", ProjectRules.PAUSED, objective = "Levar tudo para o prédio novo"),
        project("c", "Campanha de vacinação", ProjectRules.DONE),
        project("d", "Arquivo morto", ProjectRules.ARCHIVED),
        project("e", "Zeladoria", ProjectRules.ACTIVE, objective = "Auditoria dos elevadores")
    )

    private fun task(id: Long, title: String, due: String = "", done: Boolean = false, overdue: Boolean = false, project: String = "") =
        TaskItem(id, 0, title, "", "", due, "", done, overdue, false, projectUid = project)

    // ---- the list

    @Test fun listaMostraSoOsAtivosPorPadrao() {
        assertEquals("ativo", ProjectRules.DEFAULT_FILTER)
        assertEquals(listOf("a", "e"), ProjectRules.visible(rows, ProjectRules.DEFAULT_FILTER, "").map { it.uid })
    }

    @Test fun cadaSituacaoTemSeuFiltro() {
        assertEquals(listOf("b"), ProjectRules.visible(rows, ProjectRules.PAUSED, "").map { it.uid })
        assertEquals(listOf("c"), ProjectRules.visible(rows, ProjectRules.DONE, "").map { it.uid })
        assertEquals(listOf("d"), ProjectRules.visible(rows, ProjectRules.ARCHIVED, "").map { it.uid })
    }

    @Test fun todosPoeOsAtivosPrimeiroSemMudarAOrdemDeCadaGrupo() {
        assertEquals(listOf("a", "e", "b", "c", "d"), ProjectRules.visible(rows, ProjectRules.ALL, "").map { it.uid })
    }

    @Test fun buscaOlhaNomeEObjetivoSemAcentoNemMaiuscula() {
        assertEquals(listOf("a", "e"), ProjectRules.visible(rows, ProjectRules.ALL, "AUDITORIA").map { it.uid })
        assertEquals(listOf("b"), ProjectRules.visible(rows, ProjectRules.ALL, "predio").map { it.uid })
        assertEquals(listOf("b"), ProjectRules.visible(rows, ProjectRules.ALL, " mudanca ").map { it.uid })
        assertTrue(ProjectRules.visible(rows, ProjectRules.ALL, "inexistente").isEmpty())
    }

    @Test fun buscaRespeitaAIncluzaoDaSituacaoEscolhida() {
        assertEquals(listOf("a", "e"), ProjectRules.visible(rows, ProjectRules.ACTIVE, "auditoria").map { it.uid })
        assertTrue(ProjectRules.visible(rows, ProjectRules.ARCHIVED, "auditoria").isEmpty())
    }

    @Test fun abuscaDaTelaDeBuscaPedeDuasLetrasEOlhaTodosOsProjetos() {
        assertTrue(ProjectRules.search(rows, "a").isEmpty())
        assertTrue(ProjectRules.search(rows, "  ").isEmpty())
        assertEquals(listOf("a", "e"), ProjectRules.search(rows, "auditoria").map { it.uid })
        assertEquals(listOf("d"), ProjectRules.search(rows, "morto").map { it.uid })
    }

    @Test fun listaVaziaSabeDizerPorQue() {
        assertEquals(ProjectRules.Empty.NONE_AT_ALL, ProjectRules.emptyKind(emptyList(), ""))
        assertEquals(ProjectRules.Empty.NOTHING_FOUND, ProjectRules.emptyKind(rows, "xyz"))
        assertEquals(ProjectRules.Empty.NONE_IN_FILTER, ProjectRules.emptyKind(rows, ""))
        assertEquals("Nenhum projeto ativo.", ProjectRules.noneIn(ProjectRules.ACTIVE))
        assertEquals("Nenhum projeto arquivado.", ProjectRules.noneIn(ProjectRules.ARCHIVED))
    }

    @Test fun osCincoFiltrosEstaoNaOrdemCombinada() {
        assertEquals(
            listOf("Ativos", "Pausados", "Concluídos", "Arquivados", "Todos"), ProjectRules.FILTERS.map { it.second }
        )
        assertEquals(listOf("ativo", "pausado", "concluido", "arquivado", "todos"), ProjectRules.FILTERS.map { it.first })
    }

    // ---- the numbers of a card

    @Test fun andamentoEmPalavras() {
        assertEquals("3 de 8 tarefas concluídas", ProjectRules.progressLine(ProjectProgress(8, 3)))
        assertEquals("1 de 1 tarefa concluída", ProjectRules.progressLine(ProjectProgress(1, 1)))
        assertEquals("0 de 4 tarefas concluídas", ProjectRules.progressLine(ProjectProgress(4, 0)))
        assertEquals("Nenhuma tarefa ainda", ProjectRules.progressLine(ProjectProgress()))
    }

    @Test fun barraDeAndamento() {
        assertEquals(0f, ProjectRules.fraction(ProjectProgress()), 0f)
        assertEquals(0.375f, ProjectRules.fraction(ProjectProgress(8, 3)), 0.0001f)
        assertEquals(1f, ProjectRules.fraction(ProjectProgress(2, 5)), 0f)
    }

    @Test fun proximoPrazoEmDiaEMes() {
        assertEquals("Próximo prazo: 10/11", ProjectRules.nextDueLine(ProjectProgress(4, 1, 1, "2026-11-10")))
        assertEquals("", ProjectRules.nextDueLine(ProjectProgress(4, 1)))
        assertEquals("", ProjectRules.nextDueLine(ProjectProgress(4, 1, 0, "lixo")))
    }

    @Test fun chipsDeAtrasoEDeRisco() {
        assertEquals("1 atrasada", ProjectRules.lateLabel(1))
        assertEquals("3 atrasadas", ProjectRules.lateLabel(3))
        assertEquals("2 em risco", ProjectRules.riskLabel(2))
    }

    @Test fun dataAlvoEmRiscoSoComDataEComRisco() {
        val risky = project("x", "P", target = "2026-11-24", risk = ProjectRisk(1, "2026-11-25T18:00"))
        assertEquals("Data-alvo 24/11 em risco", ProjectRules.targetRiskLine(risky))
        assertEquals("Data-alvo 24/11 em risco: o plano termina em 25/11.", ProjectRules.targetRiskSentence(risky))
        assertEquals("", ProjectRules.targetRiskLine(project("x", "P", target = "2026-11-24")))
        assertEquals("", ProjectRules.targetRiskLine(project("x", "P", risk = ProjectRisk(0, "2026-11-25T18:00"))))
        assertEquals("", ProjectRules.targetRiskSentence(project("x", "P")))
    }

    @Test fun diaEMesDeDataOuDeMomento() {
        assertEquals("05/01", ProjectRules.shortDate("2026-01-05"))
        assertEquals("25/11", ProjectRules.shortDate("2026-11-25T09:00"))
        assertEquals("", ProjectRules.shortDate(""))
        assertEquals("", ProjectRules.shortDate("25/11/2026"))
    }

    @Test fun linhaDeDatasDoProjeto() {
        val p = project("x", "P", target = "2026-11-24").copy(startDate = "2026-11-01")
        assertEquals("Início 01/11/2026 · Data-alvo 24/11/2026", ProjectRules.datesLine(p))
        assertEquals("Início — · Data-alvo —", ProjectRules.datesLine(project("x", "P")))
    }

    // ---- the tasks of a project

    @Test fun tarefasDoProjetoEmAtrasadasAbertasEConcluidas() {
        val g = ProjectRules.groups(
            listOf(
                task(1, "a", overdue = true), task(2, "b"), task(3, "c", done = true),
                task(4, "d", done = true, overdue = true), task(5, "e", overdue = true)
            )
        )
        assertEquals(listOf(1L, 5L), g.late.map { it.id })
        assertEquals(listOf(2L), g.open.map { it.id })
        assertEquals(listOf(3L, 4L), g.done.map { it.id })
    }

    @Test fun incluirTarefasOfereceSoAsAbertasSemProjeto() {
        val all = listOf(
            task(1, "Livre sem prazo"), task(2, "Livre com prazo", "2026-11-20"), task(3, "Já em projeto", project = "a"),
            task(4, "Concluída", done = true), task(5, "Prazo antes", "2026-11-10"), task(6, "Árvore de natal")
        )
        assertEquals(listOf(5L, 2L, 6L, 1L), ProjectRules.candidates(all, "").map { it.id })
        assertEquals(listOf(6L), ProjectRules.candidates(all, "arvore").map { it.id })
        assertEquals(listOf(5L, 2L, 1L), ProjectRules.candidates(all, "PRAZO").map { it.id })
    }

    @Test fun textosDeIncluirEDeExcluir() {
        assertEquals("Incluir tarefas", ProjectRules.includeLabel(0))
        assertEquals("Incluir 1 tarefa", ProjectRules.includeLabel(1))
        assertEquals("Incluir 4 tarefas", ProjectRules.includeLabel(4))
        assertEquals("1 tarefa incluída no projeto.", ProjectRules.includedMessage(1))
        assertEquals("3 tarefas incluídas no projeto.", ProjectRules.includedMessage(3))
        assertTrue(ProjectRules.deleteQuestion(5).startsWith("As 5 tarefas do projeto continuam em Tarefas"))
        assertTrue(ProjectRules.deleteQuestion(5).contains("“Sem projeto”"))
        assertTrue(ProjectRules.deleteQuestion(1).startsWith("A tarefa do projeto continua"))
        assertEquals("Projeto excluído.", ProjectRules.deletedMessage(0))
        assertEquals("Projeto excluído. 1 tarefa voltou para “Sem projeto”.", ProjectRules.deletedMessage(1))
        assertEquals("Projeto excluído. 4 tarefas voltaram para “Sem projeto”.", ProjectRules.deletedMessage(4))
    }

    // ---- the Projeto field

    @Test fun campoProjetoOfereceSemProjetoEOsNaoArquivados() {
        val options = ProjectRules.fieldOptions(rows, "")
        assertEquals(listOf("", "a", "b", "c", "e"), options.map { it.key })
        assertEquals("Sem projeto", options.first().label)
    }

    @Test fun projetoArquivadoDaTarefaContinuaNaLista() {
        val options = ProjectRules.fieldOptions(rows, "d")
        assertTrue(options.any { it.key == "d" && it.label == "Arquivo morto (arquivado)" })
        assertEquals(1, options.count { it.status() })
    }

    private fun Choice.status() = label.endsWith("(arquivado)")

    @Test fun projetoQueNaoExisteMaisFicaComONomeQueATarefaTinha() {
        val options = ProjectRules.fieldOptions(rows, "zzz", "Projeto antigo")
        assertEquals(Choice("zzz", "Projeto antigo"), options.last())
        assertEquals("Projeto não encontrado", ProjectRules.fieldOptions(rows, "zzz").last().label)
    }

    @Test fun campoProjetoSomeQuandoNaoHaNadaParaEscolher() {
        assertFalse(ProjectRules.fieldShown(ProjectRules.fieldOptions(emptyList(), ""), ""))
        assertTrue(ProjectRules.fieldShown(ProjectRules.fieldOptions(rows, ""), ""))
        // Only archived projects and the task has none: nothing to choose.
        val onlyArchived = listOf(project("d", "Arquivo morto", ProjectRules.ARCHIVED))
        assertFalse(ProjectRules.fieldShown(ProjectRules.fieldOptions(onlyArchived, ""), ""))
        // A task that already has a project always shows the field.
        assertTrue(ProjectRules.fieldShown(ProjectRules.fieldOptions(onlyArchived, "d"), "d"))
    }

    // ---- the task filter

    @Test fun filtroDeTarefasPorProjeto() {
        assertTrue(ProjectRules.matchesTask("", ""))
        assertTrue(ProjectRules.matchesTask("a", ""))
        assertTrue(ProjectRules.matchesTask("", ProjectRules.NONE))
        assertFalse(ProjectRules.matchesTask("a", ProjectRules.NONE))
        assertTrue(ProjectRules.matchesTask("a", "a"))
        assertFalse(ProjectRules.matchesTask("b", "a"))
        assertFalse(ProjectRules.matchesTask("", "a"))
    }

    @Test fun rotuloDoFiltroEPoda() {
        assertEquals("Projeto", ProjectRules.filterLabel("", rows))
        assertEquals("Sem projeto", ProjectRules.filterLabel(ProjectRules.NONE, rows))
        assertEquals("Mudança da sede", ProjectRules.filterLabel("b", rows))
        assertEquals("Projeto", ProjectRules.filterLabel("zzz", rows))
        assertEquals("b", ProjectRules.pruneFilter("b", rows))
        assertEquals("", ProjectRules.pruneFilter("zzz", rows))
        assertEquals(ProjectRules.NONE, ProjectRules.pruneFilter(ProjectRules.NONE, emptyList()))
        // The sheet lists every project, the archived ones last.
        assertEquals(listOf("a", "b", "c", "e", "d"), ProjectRules.filterOptions(rows).map { it.uid })
    }

    // ---- the form

    @Test fun formularioExigeNomeEDatasEmOrdem() {
        assertEquals("Informe o nome.", ProjectRules.formProblem(ProjectForm(name = "  ")))
        assertEquals("", ProjectRules.formProblem(ProjectForm(name = "Mudança")))
        assertEquals("", ProjectRules.formProblem(ProjectForm("Mudança", startDate = "2026-11-01")))
        assertEquals("", ProjectRules.formProblem(ProjectForm("Mudança", startDate = "2026-11-01", targetDate = "2026-11-01")))
        assertEquals(
            "A data-alvo não pode ser anterior ao início.",
            ProjectRules.formProblem(ProjectForm("Mudança", startDate = "2026-12-01", targetDate = "2026-11-01"))
        )
    }

    @Test fun formularioSoMudouQuandoAlgoMudou() {
        val saved = ProjectForm("Mudança", "Levar tudo", "Ana", "ativo", "2026-11-01", "2026-11-24")
        assertFalse(saved.changedFrom(saved))
        assertFalse(saved.copy(name = "  Mudança ", assignee = "Ana  ").changedFrom(saved))
        assertTrue(saved.copy(name = "Outro").changedFrom(saved))
        assertTrue(saved.copy(objective = "").changedFrom(saved))
        assertTrue(saved.copy(status = "pausado").changedFrom(saved))
        assertTrue(saved.copy(targetDate = "").changedFrom(saved))
        assertEquals(saved, ProjectForm.of(project("u", "Mudança", "ativo", "Levar tudo", "Ana", "2026-11-24").copy(startDate = "2026-11-01")))
    }

    @Test fun conflitoDeVersaoTemOTextoDoNucleo() {
        assertTrue(ProjectRules.isConflict(ProjectRules.CHANGED))
        assertFalse(ProjectRules.isConflict("Informe o nome."))
        assertTrue(ProjectRules.CHANGED.contains("alterado"))
    }

    @Test fun menuArquivarEReabrir() {
        assertTrue(ProjectRules.canArchive(project("x", "P")))
        assertTrue(ProjectRules.canArchive(project("x", "P", ProjectRules.PAUSED)))
        assertFalse(ProjectRules.canArchive(project("x", "P", ProjectRules.ARCHIVED)))
        assertFalse(ProjectRules.canReopen(project("x", "P")))
        assertTrue(ProjectRules.canReopen(project("x", "P", ProjectRules.ARCHIVED)))
        assertTrue(ProjectRules.canReopen(project("x", "P", ProjectRules.DONE)))
    }

    // ---- the timeline

    private val today = LocalDate.parse("2026-11-19")

    // The sample of the core's own test: a fixed plan, a projected one, one with no plan, one concluded.
    private val items = listOf(
        TimelineItem(1, "u1", "Planejar", false, "2026-11-23T09:00", "2026-11-24T18:00", false, "2026-11-30", false),
        TimelineItem(2, "u2", "Executar", false, "2026-11-25T09:00", "2026-11-25T18:00", true, "2026-11-24", true),
        TimelineItem(3, "u3", "Atrasada", false, due = "2026-11-10"),
        TimelineItem(4, "u4", "Feita", true)
    )
    private val span = TimelineSpan("2026-11-10T00:00", "2026-12-01T00:00")

    private fun layout(width: Float = 200f, target: String = "2026-11-24", todayAt: LocalDate = today) =
        ProjectRules.layout(items, span, todayAt, target, width)!!

    @Test fun linhaDoTempoSoTemLinhaParaQuemTemPeriodo() {
        val l = layout()
        assertEquals(listOf("Planejar", "Executar"), l.rows.map { it.title })
        assertEquals(listOf("Atrasada", "Feita"), ProjectRules.unplanned(items).map { it.title })
        assertNull(ProjectRules.layout(items.drop(2), span, today, "", 200f))
        assertNull(ProjectRules.layout(emptyList(), null, today, "", 200f))
    }

    @Test fun eixoTemUmDiaDeFolgaEmCadaLado() {
        val l = layout()
        // The core's span is 10/11 to 01/12: the chart goes from 09/11 to 02/12.
        assertEquals(LocalDate.parse("2026-11-09"), l.firstDay)
        assertEquals(24, l.days)
    }

    @Test fun diaTemLarguraMinimaQuandoOEspacoNaoBasta() {
        val l = layout(200f)
        assertEquals(ProjectRules.MIN_DAY_WIDTH, l.dayWidth, 0f)
        assertEquals(24 * ProjectRules.MIN_DAY_WIDTH, l.width, 0.001f)
    }

    @Test fun poucosDiasUsamTodoOEspaco() {
        val l = layout(960f)
        assertEquals(40f, l.dayWidth, 0.001f)
        assertEquals(960f, l.width, 0.001f)
    }

    @Test fun barrasNaPosicaoCertaDoDiaEDaHora() {
        val l = layout()
        val d = ProjectRules.MIN_DAY_WIDTH
        val plan = l.bars[0]
        // 23/11 is the 14th day after 09/11; 09:00 is 0.375 of a day, 18:00 is 0.75.
        assertEquals(14.375f * d, plan.x, 0.01f)
        assertEquals((15.75f - 14.375f) * d, plan.width, 0.01f)
        assertEquals(BarKind.PLAN, plan.kind)
        assertFalse(plan.projected)
        assertEquals(0, plan.row)
        assertEquals(1, l.bars[1].row)
    }

    @Test fun barraDeUmDiaCurtoAindaApareceETemTipo() {
        val l = layout()
        val projected = l.bars[1]
        assertTrue(projected.projected)
        // Late wins over plan; the bar is a day at most, but never thinner than the minimum.
        assertEquals(BarKind.LATE, projected.kind)
        assertTrue(projected.width >= ProjectRules.MIN_BAR_WIDTH)
        val oneHour = listOf(TimelineItem(9, "u9", "Rápida", false, "2026-11-25T09:00", "2026-11-25T10:00"))
        val thin = ProjectRules.layout(oneHour, null, today, "", 200f)!!.bars[0]
        assertEquals(ProjectRules.MIN_BAR_WIDTH, thin.width, 0f)
    }

    @Test fun concluidaTemTipoProprio() {
        val done = listOf(TimelineItem(9, "u9", "Feita", true, "2026-11-25T09:00", "2026-11-26T18:00", late = true))
        assertEquals(BarKind.DONE, ProjectRules.layout(done, null, today, "", 200f)!!.bars[0].kind)
    }

    @Test fun prazosViramMarcasNoFimDoDia() {
        val l = layout()
        val d = ProjectRules.MIN_DAY_WIDTH
        // 30/11 is the 21st day after 09/11, 24/11 the 15th: the marks sit at the end of those days.
        assertEquals(listOf(0 to 22 * d, 1 to 16 * d), l.dues.map { it.row to it.x })
    }

    @Test fun hojeEDataAlvoTemLinhaSoDentroDoGrafico() {
        val l = layout()
        val d = ProjectRules.MIN_DAY_WIDTH
        assertEquals(10.5f * d, l.todayX!!, 0.001f)
        assertEquals(16f * d, l.targetX!!, 0.001f)
        assertNull(layout(target = "").targetX)
        assertNull(layout(target = "lixo").targetX)
        // Far away (more than the chart is willing to stretch): no line, and the axis stays the core's.
        val far = layout(todayAt = LocalDate.parse("2024-01-01"))
        assertNull(far.todayX)
        assertEquals(24, far.days)
    }

    @Test fun hojeOuAlvoPertoAlargamOEixo() {
        val later = layout(target = "2026-12-20")
        assertEquals(LocalDate.parse("2026-11-09"), later.firstDay)
        assertEquals(43, later.days)
        assertNotNull(later.targetX)
        // 20/12 is the 41st day after 09/11: the line sits at the end of it.
        assertEquals(42 * ProjectRules.MIN_DAY_WIDTH, later.targetX!!, 0.001f)
    }

    @Test fun fimDeSemanaFicaSombreadoEmFaixasJuntas() {
        val d = ProjectRules.MIN_DAY_WIDTH
        // From 09/11 (a Monday): Saturday and Sunday of three weekends.
        assertEquals(listOf(5 * d to 7 * d, 12 * d to 14 * d, 19 * d to 21 * d), layout().weekends)
    }

    @Test fun rotulosDoCabecalhoNuncaSeAmontoam() {
        val narrow = layout(200f)
        // 20 dp a day: a label every 3 days (60 dp) is the first step that leaves room.
        assertEquals("09/11", narrow.ticks.first().label)
        assertEquals("12/11", narrow.ticks[1].label)
        assertEquals(8, narrow.ticks.size)
        assertTrue(narrow.ticks.zipWithNext().all { (a, b) -> b.x - a.x >= 44f })
        val wide = layout(960f)
        assertEquals("11/11", wide.ticks[1].label)
        assertTrue(wide.ticks.zipWithNext().all { (a, b) -> b.x - a.x >= 44f })
    }

    @Test fun graficoComecaComHojeAUmTercoDaTela() {
        val l = layout()
        // Today is at 10.5 days; the view is 200 dp wide: it starts a third of the view before today.
        assertEquals(10.5f * 20f - 200f / 3f, ProjectRules.initialScroll(l, 200f), 0.01f)
        // Never past the end, nor before the start.
        assertEquals(0f, ProjectRules.initialScroll(l, 2000f), 0f)
        val noToday = layout(todayAt = LocalDate.parse("2024-01-01"))
        assertEquals(l.bars.minOf { it.x } - 12f, ProjectRules.initialScroll(noToday, 200f), 0.01f)
    }

    @Test fun toqueNoGraficoAchaALinha() {
        assertNull(ProjectRules.rowAt(10f, 2))
        assertEquals(0, ProjectRules.rowAt(ProjectRules.HEADER_HEIGHT + 1f, 2))
        assertEquals(1, ProjectRules.rowAt(ProjectRules.HEADER_HEIGHT + ProjectRules.ROW_HEIGHT + 1f, 2))
        assertNull(ProjectRules.rowAt(ProjectRules.HEADER_HEIGHT + 2 * ProjectRules.ROW_HEIGHT + 1f, 2))
    }

    @Test fun graficoUsaAMesmaAlturaDeLinhaEmPixelsInteirosEmDensidadeFracionaria() {
        val density = 2.625f
        val headerPx = ProjectRules.pixels(ProjectRules.HEADER_HEIGHT, density) // 73.5 -> 74
        val rowPx = ProjectRules.pixels(53f, density) // 139.125 -> 139
        assertEquals(74, headerPx)
        assertEquals(139, rowPx)
        // The rows of the labels start at headerPx + rowPx * i: a tap anywhere inside row i finds row i,
        // even far down, where the unrounded 139.125 px would be 12 px off at row 100.
        for (i in listOf(0, 1, 7, 50, 99, 100)) {
            val top = (headerPx + rowPx * i).toFloat()
            assertEquals(i, ProjectRules.rowAtPx(top, 101, headerPx, rowPx))
            assertEquals(i, ProjectRules.rowAtPx(top + rowPx / 2f, 101, headerPx, rowPx))
            assertEquals(i, ProjectRules.rowAtPx(top + rowPx - 1f, 101, headerPx, rowPx))
        }
        assertNull(ProjectRules.rowAtPx(headerPx - 1f, 101, headerPx, rowPx))
        assertNull(ProjectRules.rowAtPx((headerPx + rowPx * 101).toFloat(), 101, headerPx, rowPx))
        // Whole numbers of dp stay what they were; nothing collapses to zero.
        assertEquals(48, ProjectRules.pixels(48f, 1f))
        assertEquals(1, ProjectRules.pixels(0f, 3f))
    }

    @Test fun linhaDoGraficoTemNoMinimo48DpECrescePelaFonte() {
        assertEquals(48f, ProjectRules.rowHeight(1f), 0f)
        assertEquals(48f, ProjectRules.rowHeight(0.85f), 0f)
        assertEquals(48f, ProjectRules.rowHeight(Float.NaN), 0f)
        // At 2x the two label lines (16 + 14 sp) take 60 dp: the row must hold them.
        assertTrue(ProjectRules.rowHeight(2f) >= 60f)
        assertTrue(ProjectRules.rowHeight(1.5f) < ProjectRules.rowHeight(2f))
        assertTrue(ProjectRules.rowHeight(3f) >= 90f)
        assertEquals(1, ProjectRules.rowAt(ProjectRules.HEADER_HEIGHT + 70f, 3, rowHeight = 60f))
    }

    @Test fun graficoSoExisteComLinhaComPeriodo() {
        assertTrue(ProjectRules.hasChart(items))
        assertFalse(ProjectRules.hasChart(items.filter { !it.hasPeriod }))
        assertFalse(ProjectRules.hasChart(emptyList()))
    }

    @Test fun leitorDeTelaOuveTituloPeriodoPrazoEEstado() {
        assertEquals("23/11 a 24/11", ProjectRules.periodLabel(items[0]))
        assertEquals("25/11", ProjectRules.periodLabel(items[1]))
        assertEquals("", ProjectRules.periodLabel(items[2]))
        assertEquals("Planejar. período 23/11 a 24/11. prazo 30/11", ProjectRules.rowDescription(items[0]))
        assertEquals(
            "Executar. período projetado 25/11. prazo 24/11. termina depois do prazo", ProjectRules.rowDescription(items[1])
        )
        assertEquals("Atrasada. sem período planejado. prazo 10/11", ProjectRules.rowDescription(items[2]))
        assertEquals("Feita. sem período planejado. concluída", ProjectRules.rowDescription(items[3]))
        assertEquals("Prazo 10/11", ProjectRules.unplannedLine(items[2]))
        assertEquals("Concluída", ProjectRules.unplannedLine(items[3]))
    }

    // ---- where Projetos lives, and what rides in the task

    @Test fun projetosFicaAoLadoDeTarefasNaBarraDaArea() {
        assertEquals(listOf("Tarefas", "Projetos", "Atendimentos", "Recursos"), TasksSegments.labels())
        // The ids of the old three stay as they were (they are kept in the saved state).
        assertEquals(0, TasksSegments.TASKS)
        assertEquals(1, TasksSegments.ATTENDANCES)
        assertEquals(2, TasksSegments.RESOURCES)
        assertEquals(1, TasksSegments.position(TasksSegments.PROJECTS))
        assertEquals(TasksSegments.PROJECTS, TasksSegments.at(1))
        assertEquals(TasksSegments.RESOURCES, TasksSegments.at(3))
        assertEquals(TasksSegments.TASKS, TasksSegments.at(9))
        for (id in 0..3) assertEquals(id, TasksSegments.at(TasksSegments.position(id)))
    }

    @Test fun quatroPalavrasNaBarraDaAreaCabemEmTelaEstreita() {
        val labels = TasksSegments.labels()
        val shares = NavLabel.shares(labels)
        // At 360 dp (room: 322) the words stay at a readable size, on one line each.
        val size = NavLabel.sizeDp(322, labels, 15f, shares)
        assertTrue("size $size", size >= 12f)
        val total = shares.sum()
        labels.forEachIndexed { i, w -> assertTrue(w.length * 0.6f * size <= 322f * shares[i] / total - 12f + 0.001f) }
        // Three words keep the equal parts, and a wide screen leaves the wanted size alone.
        val three = listOf("Tarefas", "Atendimentos", "Recursos")
        assertEquals(listOf(1f, 1f, 1f), NavLabel.shares(three))
        assertEquals(15f, NavLabel.sizeDp(1000, three, 15f, NavLabel.shares(three)), 0.001f)
    }

    @Test fun projetoDaTarefaViajaNoRascunhoEEntraNasMudancas() {
        val saved = TaskForm("T", "responder", "normal", "", "", "", projectUid = "a")
        assertTrue(saved.changes(saved).isEmpty())
        assertEquals(mapOf("project_uid" to "b"), saved.copy(projectUid = "b").changes(saved))
        // "Sem projeto" is an empty text: it clears the project.
        assertEquals(mapOf("project_uid" to ""), saved.copy(projectUid = "").changes(saved))
        val draft = TaskDraft(1, 7, saved, saved.copy(projectUid = "b"))
        assertEquals(draft, TaskDraft.fromList(draft.toList()))
        // A draft saved before projects existed (13 fields) still comes back, with no project.
        val old = draft.toList().toMutableList().also { it.removeAt(3 + 13); it.removeAt(it.lastIndex) }
        assertEquals("", TaskDraft.fromList(old)!!.form.projectUid)
    }

    @Test fun telasDeProjetoNaPilhaESuasChaves() {
        val routes = listOf(
            Route.Project("abc"), Route.ProjectEdit(""), Route.ProjectEdit("abc"), Route.ProjectInclude("abc"),
            Route.NewTask(0, "abc"), Route.NewTask(5), Route.NewTask(0)
        )
        for (r in routes) assertEquals(r.key, r, Route.parse(r.key))
        assertEquals("task:0@abc", Route.NewTask(0, "abc").key)
        assertEquals("task:5", Route.NewTask(5).key)
        assertNull(Route.parse("proj:"))
        assertNull(Route.parse("projinc:"))
    }

    @Test fun telaDeProjetoPodeSerDeixadaPeloAvisoMasFormularioNao() {
        assertFalse(NavGuard.busy(listOf(Route.Project("a")), false))
        assertTrue(NavGuard.busy(listOf(Route.Project("a"), Route.ProjectEdit("a")), false))
        assertTrue(NavGuard.busy(listOf(Route.Project("a"), Route.ProjectInclude("a")), false))
    }
}
