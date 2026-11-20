package io.github.demusis.ponte

import java.time.LocalDate
import java.util.Locale

internal object TodayWidgetRules {
    data class Task(val title: String, val due: String)
    data class Data(val late: List<Task>, val today: List<Task>, val follow: Int, val risks: Int)
    data class Display(val counts: String, val lines: List<String>)

    fun counts(late: Int, today: Int, follow: Int, risks: Int): String = listOfNotNull(
        if (late > 0) "$late ${if (late == 1) "atrasada" else "atrasadas"}" else null,
        if (today > 0) "$today para hoje" else null,
        if (follow > 0) "$follow ${if (follow == 1) "cobrança" else "cobranças"}" else null,
        if (risks > 0) "$risks em risco" else null
    ).joinToString(" · ").ifEmpty { "Nada pendente para hoje." }

    fun display(data: Data?, locked: Boolean): Display {
        if (data == null) return Display("Abra o Ponte de bolso para começar.", emptyList())
        val counts = counts(data.late.size, data.today.size, data.follow, data.risks)
        if (locked) return Display(counts, listOf("Abra o Ponte para ver"))
        val tasks = data.late.map { task ->
            val date = try { LocalDate.parse(task.due).let { String.format(Locale.ROOT, "%02d/%02d", it.dayOfMonth, it.monthValue) } } catch (_: Exception) { "" }
            "Atrasada${if (date.isNotEmpty()) " · $date" else ""} · ${task.title}"
        } + data.today.map { it.title }
        return Display(counts, tasks.take(4) + if (tasks.size > 4) listOf("e mais ${tasks.size - 4}") else emptyList())
    }

    fun date(day: LocalDate): String {
        val weekday = listOf("seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom.")[day.dayOfWeek.value - 1]
        return String.format(Locale.ROOT, "%s, %02d/%02d", weekday, day.dayOfMonth, day.monthValue)
    }
}
