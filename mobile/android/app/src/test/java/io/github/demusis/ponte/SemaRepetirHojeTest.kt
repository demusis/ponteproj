package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime

/** The week module of Hoje does not repeat what "Atrasadas", "Para hoje" and "Próximos dias" already list. */
class SemaRepetirHojeTest {
    private val monday = LocalDate.of(2026, 11, 23)
    private val today = monday.plusDays(1)
    private val other = monday.plusDays(3)

    private fun deadline(id: String, day: LocalDate, taskId: Long?) = WeekEntry(id, "Tarefa $id", WeekKind.DEADLINE, day = day, taskId = taskId)
    private fun block(id: String, day: LocalDate) = WeekEntry(
        id, "Bloco $id", WeekKind.TASK, start = LocalDateTime.of(day, LocalTime.of(9, 0)), end = LocalDateTime.of(day, LocalTime.of(10, 0)), taskId = 50
    )

    private fun day(date: LocalDate, entries: List<WeekEntry>) = WeekPlan.days(monday, entries).first { it.date == date }

    @Test fun todayDropsDeadlinesAlreadyListedBelow() {
        val d = day(today, listOf(deadline("a", today, 1), deadline("b", today, 2)))
        val v = WeekPlan.dayView(d, today, setOf(1, 2))
        assertTrue(v.day.isEmpty)
        assertEquals(2, v.hiddenDeadlines)
    }

    @Test fun todayKeepsBlocksAndDeadlinesNotListedBelow() {
        val d = day(today, listOf(block("t", today), deadline("a", today, 1), deadline("b", today, 7), deadline("c", today, null)))
        val v = WeekPlan.dayView(d, today, setOf(1))
        assertEquals(1, v.day.blocks.size)
        assertEquals(listOf("b", "c"), v.day.deadlines.map { it.id })
        assertEquals(1, v.hiddenDeadlines)
    }

    @Test fun otherDaysKeepEverything() {
        val d = day(other, listOf(deadline("a", other, 1), block("t", other)))
        val v = WeekPlan.dayView(d, today, setOf(1))
        assertEquals(d, v.day)
        assertEquals(0, v.hiddenDeadlines)
    }

    @Test fun todayWithNothingBelowKeepsTheDeadlines() {
        val d = day(today, listOf(deadline("a", today, 1)))
        val v = WeekPlan.dayView(d, today, emptySet())
        assertEquals(1, v.day.deadlines.size)
        assertEquals(0, v.hiddenDeadlines)
    }

    @Test fun emptyDayStaysEmptyWithoutNotice() {
        val v = WeekPlan.dayView(day(today, emptyList()), today, setOf(1))
        assertTrue(v.day.isEmpty)
        assertEquals(0, v.hiddenDeadlines)
    }

    @Test fun noticeText() = assertEquals("As tarefas de hoje estão logo abaixo.", WeekPlan.BELOW_NOTICE)
}
