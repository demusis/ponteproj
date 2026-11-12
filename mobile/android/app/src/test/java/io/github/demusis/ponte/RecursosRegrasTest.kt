package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDateTime

/** The plain rules of the Recursos screens, on a fixed clock: Tuesday 10/11/2026, 10:00. */
class RecursosRegrasTest {
    private val now = "2026-11-10T10:00"
    private val s = ResourceSamples

    private fun uid(list: List<ResourceUseItem>) = list.map { it.uid }

    // ---------------------------------------------------------------- the clock and one use

    @Test fun omomentoDoRelogioTemOFormatoDoNucleo() {
        assertEquals("2026-11-10T09:05", ResourceRules.nowMoment(LocalDateTime.of(2026, 11, 10, 9, 5, 59)))
        assertEquals(LocalDateTime.of(2026, 11, 10, 14, 0), ResourceRules.parse("2026-11-10T14:00"))
        assertNull(ResourceRules.parse("10/11/2026 14:00"))
        assertNull(ResourceRules.parse(""))
    }

    @Test fun atrasadoSoEmUsoEComFimJaPassado() {
        val use = s.use("x", status = "em_uso", start = "2026-11-10T08:00", end = "2026-11-10T09:59")
        assertTrue(ResourceRules.isOverdue(use, now))
        // The end at this very minute is not past yet.
        assertFalse(ResourceRules.isOverdue(use.copy(end = now), now))
        assertFalse(ResourceRules.isOverdue(use.copy(end = "2026-11-10T10:01"), now))
        // Not in use: never overdue, whatever the end.
        for (status in listOf("reservado", "devolvido", "cancelado")) {
            assertFalse(status, ResourceRules.isOverdue(use.copy(status = status), now))
        }
        assertFalse("no end, no delay", ResourceRules.isOverdue(use.copy(end = ""), now))
    }

    @Test fun reservaOcupaDoInicioAteOFimSemIncluirOFim() {
        val booked = s.use("x", status = "reservado", start = "2026-11-10T10:00", end = "2026-11-10T12:00")
        assertTrue(ResourceRules.occupiesNow(booked, "2026-11-10T10:00"))
        assertTrue(ResourceRules.occupiesNow(booked, "2026-11-10T11:59"))
        assertFalse(ResourceRules.occupiesNow(booked, "2026-11-10T12:00"))
        assertFalse(ResourceRules.occupiesNow(booked, "2026-11-10T09:59"))
    }

    @Test fun emprestimoNaoDevolvidoContinuaOcupandoDepoisDoFim() {
        val loan = s.use("x", status = "em_uso", start = "2026-11-09T09:00", end = "2026-11-09T17:00")
        assertTrue(ResourceRules.occupiesNow(loan, now))
        assertFalse("not started yet", ResourceRules.occupiesNow(loan, "2026-11-09T08:59"))
        for (status in listOf("devolvido", "cancelado")) {
            assertFalse(status, ResourceRules.occupiesNow(loan.copy(status = status), now))
        }
    }

    @Test fun ocupaODiaQuandoOIntervaloToca() {
        val multi = s.use("x", status = "reservado", start = "2026-11-09T18:00", end = "2026-11-11T08:00")
        assertTrue(ResourceRules.occupiesDay(multi, "2026-11-09"))
        assertTrue(ResourceRules.occupiesDay(multi, "2026-11-10"))
        assertTrue(ResourceRules.occupiesDay(multi, "2026-11-11"))
        assertFalse(ResourceRules.occupiesDay(multi, "2026-11-12"))
        assertFalse(ResourceRules.occupiesDay(multi, "2026-11-08"))
        val loan = s.use("y", status = "em_uso", start = "2026-11-05T09:00", end = "2026-11-05T12:00")
        assertTrue("a loan not returned is still out today", ResourceRules.occupiesDay(loan, "2026-11-10"))
        assertFalse(ResourceRules.occupiesDay(loan.copy(status = "devolvido"), "2026-11-10"))
    }

    // ---------------------------------------------------------------- groups

    @Test fun cadaUsoVaiParaOSeuGrupo() {
        val g = { id: String -> ResourceRules.groupOf(s.uses.first { it.uid == id }, now) }
        assertEquals(ResourceRules.Group.LATE, g("u1"))
        assertEquals(ResourceRules.Group.IN_USE, g("u2"))
        assertEquals(ResourceRules.Group.TODAY, g("u3"))
        assertEquals(ResourceRules.Group.NEXT, g("u4"))
        assertEquals(ResourceRules.Group.NEXT, g("u5"))
        assertEquals(ResourceRules.Group.NEXT, g("u6"))
        assertEquals(ResourceRules.Group.LATER, g("u7"))
        assertEquals(ResourceRules.Group.CLOSED, g("u8"))
        assertEquals(ResourceRules.Group.CLOSED, g("u9"))
    }

    @Test fun limiteDosProximosDiasSaoQuatorzeDias() {
        val at = { day: String -> ResourceRules.groupOf(s.use("x", status = "reservado", start = "${day}T11:00", end = "${day}T12:00"), now) }
        assertEquals(ResourceRules.Group.TODAY, at("2026-11-10"))
        assertEquals(ResourceRules.Group.NEXT, at("2026-11-11"))
        assertEquals(ResourceRules.Group.NEXT, at("2026-11-24"))
        assertEquals(ResourceRules.Group.LATER, at("2026-11-25"))
    }

    @Test fun reservaComHoraMarcadaOuEmAndamentoEDeHoje() {
        val g = { start: String, end: String -> ResourceRules.groupOf(s.use("x", status = "reservado", start = start, end = end), now) }
        assertEquals(ResourceRules.Group.TODAY, g("2026-11-10T10:00", "2026-11-10T11:00"))
        assertEquals(ResourceRules.Group.TODAY, g("2026-11-10T07:00", "2026-11-10T10:01"))
        // Started before today and still going on.
        assertEquals(ResourceRules.Group.TODAY, g("2026-11-09T09:00", "2026-11-11T09:00"))
        // Its end is this very minute (or before): nobody took the item, the reservation is over.
        assertEquals(ResourceRules.Group.CLOSED, g("2026-11-10T08:00", "2026-11-10T10:00"))
        assertEquals(ResourceRules.Group.CLOSED, g("2026-11-10T08:00", "2026-11-10T09:00"))
    }

    @Test fun osGruposSaiemNaOrdemDaTela() {
        val g = ResourceRules.group(s.uses, "", now)
        assertEquals(listOf("u1"), uid(g.late))
        assertEquals(listOf("u2"), uid(g.inUse))
        assertEquals(listOf("u3"), uid(g.today))
        // By start: u6 (11/11 08:00) before u4 (12/11) before u5 (13/11).
        assertEquals(listOf("u6", "u4", "u5"), uid(g.next))
        assertEquals(listOf("u7"), uid(g.later))
        // The latest first: the one returned on 05/11 before the one cancelled for 08/11? Latest end first.
        assertEquals(listOf("u9", "u8"), uid(g.closed))
        assertEquals(9, g.total)
        assertEquals(
            listOf(
                ResourceRules.Group.LATE, ResourceRules.Group.IN_USE, ResourceRules.Group.TODAY,
                ResourceRules.Group.NEXT, ResourceRules.Group.LATER, ResourceRules.Group.CLOSED
            ),
            g.shown.map { it.first }
        )
    }

    @Test fun grupoVazioNaoApareceENadaDeUsosDaGrupoNenhum() {
        val g = ResourceRules.group(listOf(s.uses[0]), "", now)
        assertEquals(listOf(ResourceRules.Group.LATE), g.shown.map { it.first })
        assertEquals(0, ResourceRules.group(emptyList(), "", now).total)
        assertTrue(ResourceRules.group(emptyList(), "", now).shown.isEmpty())
    }

    @Test fun atrasadosEEmUsoSaemPeloFimMaisAntigo() {
        val a = s.use("a", status = "em_uso", start = "2026-11-01T09:00", end = "2026-11-08T09:00")
        val b = s.use("b", status = "em_uso", start = "2026-11-02T09:00", end = "2026-11-05T09:00")
        val c = s.use("c", status = "em_uso", start = "2026-11-10T09:00", end = "2026-11-10T20:00")
        val d = s.use("d", status = "em_uso", start = "2026-11-10T08:00", end = "2026-11-10T18:00")
        val g = ResourceRules.group(listOf(a, b, c, d), "", now)
        assertEquals(listOf("b", "a"), uid(g.late))
        assertEquals(listOf("d", "c"), uid(g.inUse))
    }

    // ---------------------------------------------------------------- search

    @Test fun buscaIgnoraMaiusculasEAcentos() {
        assertEquals(listOf("u3", "u5", "u7"), uid(ResourceRules.group(s.uses, "SALA reuniões", now).let { it.today + it.next + it.later }))
        assertEquals(listOf("u3", "u5", "u7"), uid(ResourceRules.group(s.uses, "reunioes", now).let { it.today + it.next + it.later }))
        assertEquals(listOf("u3"), uid(ResourceRules.group(s.uses, "juridico", now).today))
        assertEquals(listOf("u1"), uid(ResourceRules.group(s.uses, "SERVIDOR A administrativo", now).late))
    }

    @Test fun buscaExigeTodasAsPalavrasEPeloMenosUmCampo() {
        assertEquals(0, ResourceRules.group(s.uses, "projetor palestra", now).total)
        assertEquals(9, ResourceRules.group(s.uses, "   ", now).total)
        assertEquals(0, ResourceRules.group(s.uses, "zzz", now).total)
        // The document and the task linked to the use are searched too.
        assertEquals(listOf("u3"), uid(ResourceRules.group(s.uses, "ORG-OFI-2026/00431", now).today))
        assertEquals(listOf("u3"), uid(ResourceRules.group(s.uses, "devolver chaves", now).today))
    }

    @Test fun buscaDeRecursoPorNomeTipoLocalEIdentificacao() {
        val names = { q: String -> ResourceRules.resourceRows(s.resources, q).map { it.name } }
        assertEquals(listOf("Projetor"), names("patrimônio 004512"))
        assertEquals(listOf("Manual de redação oficial"), names("biblioteca"))
        assertEquals(listOf("Manual de redação oficial"), names("REDACAO"))
        assertEquals(listOf("Auditório", "Sala de reuniões do 2º andar"), names("sala"))
        assertEquals(emptyList<String>(), names("xyz"))
        assertEquals(s.resources.size, names("").size)
    }

    @Test fun recursosForaDeUsoVaoParaOFimDaLista() {
        val rows = ResourceRules.resourceRows(s.resources, "")
        assertEquals("Câmera antiga", rows.last().name)
        // The order of the others is the core's.
        assertEquals(s.resources.filter { it.active }.map { it.uid }, rows.dropLast(1).map { it.uid })
    }

    // ---------------------------------------------------------------- texts

    @Test fun periodoNoMesmoDiaMostraAHoraDeFimSo() {
        assertEquals("10/11 14:00 – 16:00", ResourceRules.periodText("2026-11-10T14:00", "2026-11-10T16:00"))
        assertEquals("05/01 09:05 – 09:30", ResourceRules.periodText("2026-01-05T09:05", "2026-01-05T09:30"))
    }

    @Test fun periodoEmVariosDiasMostraAsDuasDatas() {
        assertEquals("11/11 08:00 – 12/11 18:00", ResourceRules.periodText("2026-11-11T08:00", "2026-11-12T18:00"))
        assertEquals("31/12/2026 23:00 – 01/01/2027 01:00", ResourceRules.periodText("2026-12-31T23:00", "2027-01-01T01:00"))
    }

    @Test fun periodoQueNaoSeLeVaiComoVeio() {
        assertEquals("amanhã – depois", ResourceRules.periodText("amanhã", "depois"))
        assertEquals("2026-11-10T14:00", ResourceRules.periodText("2026-11-10T14:00", ""))
        assertEquals("", ResourceRules.periodText("", ""))
    }

    @Test fun momentoPorExtenso() {
        assertEquals("10/11/2026 14:00", ResourceRules.momentText("2026-11-10T14:00"))
        assertEquals("sem data", ResourceRules.momentText("sem data"))
    }

    @Test fun linhaDeQuemJuntaNomeESetor() {
        assertEquals("Servidor B · Setor Jurídico", ResourceRules.whoLine(s.uses.first { it.uid == "u3" }))
        assertEquals("Servidor C", ResourceRules.whoLine(s.use("x", who = "Servidor C", sector = "")))
        assertEquals("", ResourceRules.whoLine(s.use("x", who = " ", sector = "")))
    }

    // ---------------------------------------------------------------- one resource

    @Test fun quantidadeEmUsoAgoraSomaOsUsosQueOcupam() {
        val notebook = s.resources.first { it.name == "Notebook de apresentações" }
        // u2 (qty 1) is out; a second one reserved for later does not count yet.
        assertEquals(1, ResourceRules.inUseNow(notebook, s.uses, now))
        assertEquals(1, ResourceRules.freeNow(notebook, s.uses, now))
        assertEquals("1 de 2 em uso agora", ResourceRules.usageLine(notebook, s.uses, now))
        val more = s.uses + s.use("n2", resource = notebook, status = "reservado", start = "2026-11-10T09:00", end = "2026-11-10T11:00")
        assertEquals(2, ResourceRules.inUseNow(notebook, more, now))
        assertEquals(0, ResourceRules.freeNow(notebook, more, now))
        // At 11:00 the reservation is over: one is free again.
        assertEquals(1, ResourceRules.freeNow(notebook, more, "2026-11-10T11:00"))
    }

    @Test fun recursoSemUsoAgoraEstaLivreECadeirasSomamQuantidades() {
        val auditorio = s.resources.first { it.name == "Auditório" }
        assertEquals("0 de 1 em uso agora", ResourceRules.usageLine(auditorio, s.uses, now))
        assertEquals(1, ResourceRules.freeNow(auditorio, s.uses, now))
        val chairs = s.resources.first { it.name == "Cadeiras empilháveis" }
        assertEquals("30 de 40 em uso agora", ResourceRules.usageLine(chairs, s.uses, "2026-11-11T09:00"))
        assertEquals(10, ResourceRules.freeNow(chairs, s.uses, "2026-11-11T09:00"))
    }

    @Test fun nuncaMostraQuantidadeLivreNegativa() {
        val one = s.resources.first { it.name == "Projetor" }
        val over = listOf(
            s.use("a", resource = one, status = "em_uso", start = "2026-11-09T09:00", end = "2026-11-09T10:00"),
            s.use("b", resource = one, status = "em_uso", start = "2026-11-09T09:00", end = "2026-11-09T10:00")
        )
        assertEquals(0, ResourceRules.freeNow(one, over, now))
    }

    @Test fun osProximosUsosDeUmRecursoSaemPorInicioSemOsEncerrados() {
        val room = s.resources.first { it.name == "Sala de reuniões do 2º andar" }
        assertEquals(listOf("u3", "u5", "u7"), uid(ResourceRules.nextUses(room, s.uses, now)))
        val projector = s.resources.first { it.name == "Projetor" }
        // u1 is overdue (still out); u8 was returned.
        assertEquals(listOf("u1"), uid(ResourceRules.nextUses(projector, s.uses, now)))
    }

    @Test fun ocupacaoDeHojeDeUmRecurso() {
        val room = s.resources.first { it.name == "Sala de reuniões do 2º andar" }
        assertEquals(listOf("u3"), uid(ResourceRules.occupationToday(room, s.uses, now)))
        val projector = s.resources.first { it.name == "Projetor" }
        assertEquals("the loan not returned is still out today", listOf("u1"), uid(ResourceRules.occupationToday(projector, s.uses, now)))
        val auditorio = s.resources.first { it.name == "Auditório" }
        assertTrue(ResourceRules.occupationToday(auditorio, s.uses, now).isEmpty())
    }

    // ---------------------------------------------------------------- words, Hoje and the notice

    @Test fun asPalavrasDosRecursosNaoDizemRegistroNemAtendimento() {
        for (text in PhoneWords.RES_TEXTS) {
            val low = text.lowercase()
            assertFalse(text, low.contains("registro"))
            assertFalse(text, low.contains("atendimento"))
        }
        assertEquals("Quem", PhoneWords.RES_WHO)
        assertEquals("Setor", PhoneWords.RES_SECTOR)
        assertEquals("Finalidade", PhoneWords.RES_PURPOSE)
        assertEquals("Início", PhoneWords.RES_START)
        assertEquals("Fim previsto", PhoneWords.RES_END)
        assertEquals("Devolvido em", PhoneWords.RES_RETURNED)
        assertEquals("Situação", PhoneWords.RES_STATUS)
        assertEquals("Atrasado", PhoneWords.RES_OVERDUE)
        assertEquals("Reservas de hoje", PhoneWords.RES_TODAY_LINE)
        assertEquals("Devoluções atrasadas", PhoneWords.RES_OVERDUE_LINE)
    }

    @Test fun osTitulosDosGrupos() {
        assertEquals(
            listOf("Atrasados", "Em uso agora", "Hoje", "Próximos dias", "Mais adiante", "Encerrados"),
            ResourceRules.Group.values().map { ResourceRules.groupTitle(it) }
        )
    }

    @Test fun resumoDeHojeContaReservasEDevolucoes() {
        assertEquals("2 reservas de hoje", todaySummary(0, 0, 0, 2, 0))
        assertEquals("1 reserva de hoje · 1 devolução atrasada", todaySummary(0, 0, 0, 1, 1))
        assertEquals("3 devoluções atrasadas", todaySummary(0, 0, 0, 0, 3))
        assertEquals(
            "1 tarefa atrasada · 2 para hoje · 1 atendimento com prazo vencido ou para hoje · 2 reservas de hoje · 1 devolução atrasada",
            todaySummary(1, 2, 1, 2, 1)
        )
        // What existed keeps its words.
        assertEquals("2 tarefas atrasadas", todaySummary(2, 0))
        assertEquals("Nada vence hoje.", todaySummary(0, 0, 0, 0, 0))
    }

    @Test fun hojeNaoEstaVazioComSoReservasOuDevolucoes() {
        assertTrue(TodayState(loading = false).isEmpty)
        assertFalse(TodayState(loading = false, reservationsToday = 1).isEmpty)
        assertFalse(TodayState(loading = false, overdueReturns = 2).isEmpty)
    }

    @Test fun avisoDiarioContaReservasEDevolucoesSemNomes() {
        assertEquals(
            "3 tarefas atrasadas, 2 para hoje, 1 atendimento com prazo, 2 reservas de hoje e 1 devolução atrasada.",
            DeadlineNotice.body(3, 2, 1, 2, 1)
        )
        assertEquals("1 reserva de hoje.", DeadlineNotice.body(0, 0, 0, 1, 0))
        assertEquals("2 devoluções atrasadas.", DeadlineNotice.body(0, 0, 0, 0, 2))
        assertEquals("1 tarefa atrasada e 2 reservas de hoje.", DeadlineNotice.body(1, 0, 0, 2, 0))
        assertNull(DeadlineNotice.body(0, 0, 0, 0, 0))
        assertNull(DeadlineNotice.body(0, 0, 0, -2, -1))
        // The old call keeps working.
        assertEquals("1 atendimento com prazo.", DeadlineNotice.body(0, 0, 1))
    }

    @Test fun oNomeDeUmRecursoNuncaVaiParaOAviso() {
        for (u in s.uses) {
            val text = DeadlineNotice.body(1, 1, 1, 2, 1)!!
            assertFalse(text.contains(u.resourceName))
            assertFalse(text.contains(u.who))
        }
    }

    @Test fun rotasDeUsoERecursoVoltamDoEstadoSalvo() {
        assertEquals(Route.ResourceUse("abc123"), Route.parse(Route.ResourceUse("abc123").key))
        assertEquals(Route.Resource("abc123"), Route.parse(Route.Resource("abc123").key))
        assertNull(Route.parse("use:"))
        assertNull(Route.parse("res:"))
        assertTrue(Route.ResourceUse("a").key != Route.Resource("a").key)
    }
}
