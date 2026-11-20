package io.github.demusis.ponte

import org.junit.Assert.*
import org.junit.Test
import java.time.LocalDate

class TodayWidgetRulesTest {
    @Test fun counts() {
        assertEquals("Nada pendente para hoje.", TodayWidgetRules.counts(0, 0, 0, 0))
        assertEquals("1 atrasada", TodayWidgetRules.counts(1, 0, 0, 0))
        assertEquals("2 atrasadas", TodayWidgetRules.counts(2, 0, 0, 0))
        assertEquals("1 para hoje", TodayWidgetRules.counts(0, 1, 0, 0))
        assertEquals("1 cobrança", TodayWidgetRules.counts(0, 0, 1, 0))
        assertEquals("2 cobranças", TodayWidgetRules.counts(0, 0, 2, 0))
        assertEquals("1 em risco", TodayWidgetRules.counts(0, 0, 0, 1))
        assertEquals("1 atrasada · 3 para hoje · 2 cobranças · 1 em risco", TodayWidgetRules.counts(1, 3, 2, 1))
    }
    @Test fun titlesAndPrivacy() {
        val task = TodayWidgetRules.Task("Segredo", "2026-11-18")
        val data = TodayWidgetRules.Data(listOf(task), List(4) { TodayWidgetRules.Task("Hoje $it", "") }, 1, 0)
        val shown = TodayWidgetRules.display(data, false)
        assertEquals("Atrasada · 18/11 · Segredo", shown.lines.first())
        assertEquals("e mais 1", shown.lines.last())
        assertEquals(5, shown.lines.size)
        assertEquals(listOf("Abra o Ponte para ver"), TodayWidgetRules.display(data, true).lines)
        assertEquals(TodayWidgetRules.Display("Abra o Ponte de bolso para começar.", emptyList()), TodayWidgetRules.display(null, false))
    }
    @Test fun date() { assertEquals("qui., 19/11", TodayWidgetRules.date(LocalDate.of(2026, 11, 19))) }
}
