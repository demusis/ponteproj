package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Duration
import java.time.ZoneId
import java.time.ZonedDateTime

/** The plain rules behind "Seu nome", the suggestions, the Atendimentos list, the search and the daily notice. */
class NomeBuscaAvisoTest {
    // ---------------------------------------------------------------- the name and the suggestions

    private val names = listOf("José Silva", "Ana Souza", "Carla Dias", "Bruno Lima", "Daniel Reis", "Elisa Prado",
        "Fábio Neves", "Gisele Mota", "Hugo Pires", "Ìris Braga")

    @Test fun nomeSemEspacosEAteOitentaCaracteres() {
        assertEquals("Servidor A", Assignees.cleanName("  Servidor A  "))
        assertEquals("", Assignees.cleanName("   "))
        assertEquals(80, Assignees.cleanName("x".repeat(200)).length)
        // Cut at 80, then the blanks at the new end go too.
        assertEquals("a".repeat(79), Assignees.cleanName("a".repeat(79) + " " + "b".repeat(10)))
    }

    @Test fun semTextoOferecePrimeirosOitoNaOrdemDeUso() {
        assertEquals(names.take(8), Assignees.suggest(names, ""))
    }

    @Test fun filtraSemAcentoNemMaiuscula() {
        assertEquals(listOf("José Silva"), Assignees.suggest(names, "JOSE"))
        assertEquals(listOf("Ana Souza"), Assignees.suggest(names, "souza"))
        assertEquals(listOf("Fábio Neves"), Assignees.suggest(names, "fabio"))
        // Part of a name, anywhere in it.
        assertEquals(listOf("Carla Dias"), Assignees.suggest(names, "ia"))
        assertTrue(Assignees.suggest(names, "zzz").isEmpty())
    }

    @Test fun oProprioNomeEOTextoJaDigitadoNaoViramSugestao() {
        assertFalse("Ana Souza" in Assignees.suggest(names, "", me = "ana souza"))
        assertFalse("José Silva" in Assignees.suggest(names, "José Silva"))
        assertTrue("José Silva" in Assignees.suggest(names, "jose silva"))
    }

    @Test fun nuncaMaisDeOitoMesmoComLimiteMaior() {
        val many = (1..30).map { "Pessoa $it" }
        assertEquals(8, Assignees.suggest(many, "pessoa").size)
        assertEquals(3, Assignees.suggest(many, "pessoa", limit = 3).size)
    }

    @Test fun botaoEuSoQuandoHaNomeEOCampoNaoDiz() {
        assertTrue(Assignees.offerMe("Servidor A", ""))
        assertTrue(Assignees.offerMe("Servidor A", "Servidor B"))
        assertFalse(Assignees.offerMe("Servidor A", "Servidor A"))
        assertFalse(Assignees.offerMe("Servidor A", "  Servidor A "))
        assertFalse(Assignees.offerMe("", "Servidor B"))
        assertFalse(Assignees.offerMe("   ", ""))
    }

    @Test fun sugestaoEEuSoMudamOFormularioDigitado() {
        val task = Samples.taskLinked
        val draft = TaskRules.start(task, 0)
        assertFalse(draft.changed)
        // Tapping "Eu" or a chip is the same as typing: only the form changes.
        val filled = draft.copy(form = draft.form.copy(assignee = "Servidor B"))
        assertTrue(filled.changed)
        assertEquals(draft.baseVersion, filled.baseVersion)
        assertEquals(mapOf("assignee" to "Servidor B"), filled.toSave(task.id).changes)
        assertEquals(task.version, filled.toSave(task.id).version)
        // A newer stored version with something typed is still a conflict: nothing is saved over it.
        val newer = task.copy(version = task.version + 1)
        assertTrue(TaskRules.reconcile(filled, newer, 0).conflict)
        // Nothing typed: the draft just follows the stored task.
        assertEquals(newer.version, TaskRules.reconcile(draft, newer, 0).baseVersion)
    }

    // ---------------------------------------------------------------- the Atendimentos list

    private fun att(
        id: Long, ref: String, open: Boolean = true, subject: String = "", assignee: String = "", obs: String = ""
    ) = AttendanceItem(id, ref, subject, if (open) "Em análise" else "Encerrado", "", "", false, assignee, obs, open)

    private val rows = listOf(
        att(1, "ORG-OFI-2026/00431", subject = "Pedido de apoio logístico", assignee = "José Silva"),
        att(2, "ORG-PRO-2026/00212", subject = "Consulta de pessoal", obs = "Aguardando o ofício do Setor Jurídico"),
        att(3, "ORG-OFI-2026/00100", open = false, subject = "Resposta enviada", assignee = "Ana"),
        att(4, "ORG-OFI-2026/00101", subject = "Outro assunto", assignee = "Ana")
    )

    @Test fun listaSeparaAbertosDeEncerradosMantendoAOrdemDoNucleo() {
        val s = AttendanceRules.split(rows, "")
        assertEquals(listOf(1L, 2L, 4L), s.open.map { it.id })
        assertEquals(listOf(3L), s.closed.map { it.id })
    }

    @Test fun buscaNaListaPorReferenciaAssuntoResponsavelEObservacao() {
        assertEquals(listOf(1L), AttendanceRules.split(rows, "00431").open.map { it.id })
        assertEquals(listOf(1L), AttendanceRules.split(rows, "LOGISTICO").open.map { it.id })
        assertEquals(listOf(1L), AttendanceRules.split(rows, "jose").open.map { it.id })
        assertEquals(listOf(2L), AttendanceRules.split(rows, "juridico").open.map { it.id })
        // Every word must be there, in any field.
        assertEquals(listOf(2L), AttendanceRules.split(rows, "pessoal ofício").open.map { it.id })
        // The closed ones are searched too.
        val ana = AttendanceRules.split(rows, "ana")
        assertEquals(listOf(4L), ana.open.map { it.id })
        assertEquals(listOf(3L), ana.closed.map { it.id })
        assertTrue(AttendanceRules.split(rows, "zzz").open.isEmpty())
    }

    @Test fun linhaDeAbertos() {
        assertEquals("Nenhum em aberto", AttendanceRules.openLine(0))
        assertEquals("1 em aberto", AttendanceRules.openLine(1))
        assertEquals("12 em aberto", AttendanceRules.openLine(12))
    }

    // ---------------------------------------------------------------- the search

    @Test fun buscaSoComDuasLetrasOuMais() {
        assertFalse(SearchRules.usable(""))
        assertFalse(SearchRules.usable("a"))
        assertFalse(SearchRules.usable("  a  "))
        assertTrue(SearchRules.usable("ab"))
        assertTrue(SearchRules.usable("  ab "))
        assertEquals(300L, SearchRules.DEBOUNCE_MS)
    }

    @Test fun respostaAntigaOuComAppBloqueadoOuTelaFechadaEDescartada() {
        assertTrue(SearchRules.accept(5, 5, locked = false, screenOpen = true))
        assertFalse(SearchRules.accept(4, 5, locked = false, screenOpen = true))
        assertFalse(SearchRules.accept(5, 5, locked = true, screenOpen = true))
        assertFalse(SearchRules.accept(5, 5, locked = false, screenOpen = false))
    }

    @Test fun textoDeEMaisN() {
        assertEquals("e mais 3", SearchRules.moreLine(3))
    }

    @Test fun rotasNovasVoltamPelaChave() {
        assertEquals(Route.Attendance(7), Route.parse(Route.Attendance(7).key))
        assertEquals(Route.Search, Route.parse(Route.Search.key))
        assertNull(Route.parse("att:x"))
    }

    // ---------------------------------------------------------------- the daily notice

    private fun at(zone: String, text: String) = ZonedDateTime.parse(text).withZoneSameLocal(ZoneId.of(zone))

    private val sp = "America/Sao_Paulo"

    @Test fun proximoAvisoHojeSeAindaNaoPassouDaHora() {
        val next = DeadlineNotice.nextTrigger(at(sp, "2026-11-10T07:59:59-03:00"), DeadlineNotice.DEFAULT)
        assertEquals("2026-11-10T08:00", next.toLocalDateTime().toString())
    }

    @Test fun proximoAvisoAmanhaNaHoraExataOuDepois() {
        assertEquals("2026-11-11T08:00", DeadlineNotice.nextTrigger(at(sp, "2026-11-10T08:00:00-03:00"), DeadlineNotice.DEFAULT).toLocalDateTime().toString())
        assertEquals("2026-11-11T08:00", DeadlineNotice.nextTrigger(at(sp, "2026-11-10T08:07:00-03:00"), DeadlineNotice.DEFAULT).toLocalDateTime().toString())
        assertEquals("2026-11-11T08:00", DeadlineNotice.nextTrigger(at(sp, "2026-11-10T23:59:59-03:00"), DeadlineNotice.DEFAULT).toLocalDateTime().toString())
    }

    @Test fun viradaDeDiaEDeMes() {
        val late = DeadlineNotice.Time(0, 30)
        assertEquals("2026-12-01T00:30", DeadlineNotice.nextTrigger(at(sp, "2026-11-30T23:30:00-03:00"), late).toLocalDateTime().toString())
        assertEquals("2026-11-30T00:30", DeadlineNotice.nextTrigger(at(sp, "2026-11-30T00:10:00-03:00"), late).toLocalDateTime().toString())
        assertEquals("2027-01-01T08:00", DeadlineNotice.nextTrigger(at(sp, "2026-12-31T12:00:00-03:00"), DeadlineNotice.DEFAULT).toLocalDateTime().toString())
    }

    @Test fun diaDeMudancaDeHorarioContaPeloRelogioNaoPor24Horas() {
        val lisbon = "Europe/Lisbon"
        // 25-hour day (the clocks go back on 2026-10-25): 08:00 to 08:00 is 25 hours, not 24.
        val before = at(lisbon, "2026-10-24T08:00:05+01:00")
        val next = DeadlineNotice.nextTrigger(before, DeadlineNotice.DEFAULT)
        assertEquals("2026-10-25T08:00", next.toLocalDateTime().toString())
        assertEquals(Duration.ofHours(25).minusSeconds(5), Duration.between(before, next))
        // The next day is back to 24 hours.
        val after = DeadlineNotice.nextTrigger(next.plusSeconds(5), DeadlineNotice.DEFAULT)
        assertEquals("2026-10-26T08:00", after.toLocalDateTime().toString())
        assertEquals(Duration.ofHours(24).minusSeconds(5), Duration.between(next.plusSeconds(5), after))
        // 23-hour day (the clocks go forward on 2026-03-29).
        val spring = DeadlineNotice.nextTrigger(at(lisbon, "2026-03-28T08:00:05+00:00"), DeadlineNotice.DEFAULT)
        assertEquals("2026-03-29T08:00", spring.toLocalDateTime().toString())
        assertEquals(Duration.ofHours(23).minusSeconds(5), Duration.between(at(lisbon, "2026-03-28T08:00:05+00:00"), spring))
    }

    @Test fun horaQueNaoExisteNoDiaDaMudancaVaiParaAFrente() {
        // 01:30 does not exist in Lisbon on 2026-03-29 (01:00 jumps to 02:00): it is 02:30 that day.
        val next = DeadlineNotice.nextTrigger(at("Europe/Lisbon", "2026-03-28T12:00:00+00:00"), DeadlineNotice.Time(1, 30))
        assertEquals("2026-03-29T02:30", next.toLocalDateTime().toString())
        assertTrue(next.isAfter(at("Europe/Lisbon", "2026-03-28T12:00:00+00:00")))
    }

    @Test fun avisoNuncaNoPassado() {
        for (zone in listOf(sp, "Europe/Lisbon", "Asia/Kolkata", "Pacific/Auckland")) {
            var now = ZonedDateTime.of(2026, 1, 1, 0, 0, 0, 0, ZoneId.of(zone))
            repeat(400) {
                for (t in listOf(DeadlineNotice.DEFAULT, DeadlineNotice.Time(0, 0), DeadlineNotice.Time(2, 30), DeadlineNotice.Time(22, 30))) {
                    val next = DeadlineNotice.nextTrigger(now, t)
                    assertTrue("$zone $now $t", next.isAfter(now))
                    assertTrue("$zone $now $t", Duration.between(now, next) <= Duration.ofHours(25))
                }
                now = now.plusHours(17).plusMinutes(13)
            }
        }
    }

    @Test fun textoDoAvisoSoTemNumeros() {
        assertEquals("3 tarefas atrasadas, 2 para hoje e 1 atendimento com prazo.", DeadlineNotice.body(3, 2, 1))
        assertEquals("1 tarefa atrasada, 1 para hoje e 2 atendimentos com prazo.", DeadlineNotice.body(1, 1, 2))
        assertEquals("2 tarefas atrasadas e 1 para hoje.", DeadlineNotice.body(2, 1, 0))
        assertEquals("1 tarefa atrasada e 3 atendimentos com prazo.", DeadlineNotice.body(1, 0, 3))
        assertEquals("2 tarefas para hoje e 1 atendimento com prazo.", DeadlineNotice.body(0, 2, 1))
        assertEquals("1 tarefa atrasada.", DeadlineNotice.body(1, 0, 0))
        assertEquals("4 tarefas para hoje.", DeadlineNotice.body(0, 4, 0))
        assertEquals("1 atendimento com prazo.", DeadlineNotice.body(0, 0, 1))
    }

    @Test fun nadaVenceNadaDeAviso() {
        assertNull(DeadlineNotice.body(0, 0, 0))
        assertNull(DeadlineNotice.body(-1, 0, -3))
    }

    @Test fun horariosDoAviso() {
        assertEquals(DeadlineNotice.Time(8, 0), DeadlineNotice.DEFAULT)
        assertEquals("08:00", DeadlineNotice.DEFAULT.key)
        assertEquals(DeadlineNotice.Time(7, 30), DeadlineNotice.Time.fromKey("07:30"))
        assertEquals(DeadlineNotice.DEFAULT, DeadlineNotice.Time.fromKey("25:00"))
        assertEquals(DeadlineNotice.DEFAULT, DeadlineNotice.Time.fromKey("abc"))
        assertEquals(DeadlineNotice.DEFAULT, DeadlineNotice.Time.fromKey(null))
        assertEquals(DeadlineNotice.Time(9, 5), DeadlineNotice.Time.fromMinutes(9 * 60 + 5))
        assertEquals(DeadlineNotice.DEFAULT, DeadlineNotice.Time.fromMinutes(-1))
        assertEquals(DeadlineNotice.DEFAULT, DeadlineNotice.Time.fromMinutes(24 * 60))
        assertEquals("05:00", DeadlineNotice.CHOICES.first().key)
        assertEquals("22:00", DeadlineNotice.CHOICES.last().key)
        assertEquals(35, DeadlineNotice.CHOICES.size)
        assertTrue(DeadlineNotice.DEFAULT in DeadlineNotice.CHOICES)
        // The public text of a locked screen says nothing about the counts.
        assertEquals("Ponte de bolso: há prazos para hoje.", DeadlineNotice.PUBLIC_TEXT)
    }
}
