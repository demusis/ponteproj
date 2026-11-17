package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** What the Recursos screens still word themselves; the rules are the core's (see the bridge tests). */
class RecursosRegrasTest {
    private val s = ResourceSamples

    // ---------------------------------------------------------------- the clock and one use

    @Test fun momentoPorExtenso() {
        assertEquals("10/11/2026 14:00", ResourceRules.momentText("2026-11-10T14:00"))
        assertEquals("sem data", ResourceRules.momentText("sem data"))
    }

    @Test fun linhaDeQuemJuntaNomeESetor() {
        assertEquals("Servidor B · Setor Jurídico", ResourceRules.whoLine(s.uses.first { it.uid == "u3" }))
        assertEquals("Servidor C", ResourceRules.whoLine(s.use("x", who = "Servidor C", sector = "")))
        assertEquals("", ResourceRules.whoLine(s.use("x", who = " ", sector = "")))
    }

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

    @Test fun nenhumaPalavraDosRecursosDizTipo() {
        for (text in PhoneWords.RES_TEXTS) assertFalse(text, text.contains("tipo", ignoreCase = true))
        assertFalse(PhoneWords.RES_SEARCH_RESOURCES.contains("tipo", ignoreCase = true))
    }

    @Test fun hojeNaoEstaVazioComSoReservasOuDevolucoes() {
        assertTrue(TodayState(loading = false).isEmpty)
        assertFalse(TodayState(loading = false, reservationsToday = 1).isEmpty)
        assertFalse(TodayState(loading = false, overdueReturns = 2).isEmpty)
    }

    @Test fun rotasDeUsoERecursoVoltamDoEstadoSalvo() {
        assertEquals(Route.ResourceUse("abc123"), Route.parse(Route.ResourceUse("abc123").key))
        assertEquals(Route.Resource("abc123"), Route.parse(Route.Resource("abc123").key))
        assertNull(Route.parse("use:"))
        assertNull(Route.parse("res:"))
        assertTrue(Route.ResourceUse("a").key != Route.Resource("a").key)
    }
}
