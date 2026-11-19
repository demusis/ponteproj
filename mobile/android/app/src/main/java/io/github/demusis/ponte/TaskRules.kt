package io.github.demusis.ponte

/**
 * What the person is typing on the task screen: the form, the task it started from (its version and
 * its values at that moment) and the [epoch] of the screen state it belongs to. Plain values, so the
 * rules below are tested without Android.
 */
internal data class TaskDraft(
    val epoch: Int,
    /** The version of the stored task this draft started from; every save sends this one. */
    val baseVersion: Long,
    val base: TaskForm,
    val form: TaskForm,
    /** The stored task is newer than [baseVersion] and the person has changes: nothing can be saved. */
    val conflict: Boolean = false
) {
    val changed: Boolean get() = form.changedFrom(base)

    fun toSave(taskId: Long) = TaskSave(taskId, baseVersion, base, form)

    /** Flat list of strings and numbers, for the saved state of the screen. */
    fun toList(): List<Any> = listOf(epoch, baseVersion, conflict) + base.toList() + form.toList()

    companion object {
        fun fromList(l: List<Any>): TaskDraft? = try {
            val size = if (l.size >= 29) 13 else if (l.size >= 27) 12 else if (l.size >= 25) 11 else 7
            TaskDraft(
                l[0] as Int, l[1] as Long, TaskForm.fromList(l.subList(3, 3 + size).map { it as String }),
                TaskForm.fromList(l.subList(3 + size, 3 + 2 * size).map { it as String }), l[2] as Boolean
            )
        } catch (_: Throwable) {
            null
        }
    }
}

/** What a save sends: the task, the version the form started from, and only what differs from it. */
internal data class TaskSave(val taskId: Long, val version: Long, val base: TaskForm, val form: TaskForm) {
    val changes: Map<String, Any> get() = form.changes(base)
}

/** The categories travel as one string (uids have no commas) so the list stays flat. */
internal fun TaskForm.toList(): List<String> =
    listOf(title, action, priority, dueDate, assignee, notes, categories.joinToString(","), scheduledStart, scheduledEnd, estimateMinutes.toString(), parentTaskId?.toString() ?: "", reminderOffsetsMinutes.joinToString(","), dependencies.joinToString(","))

internal fun TaskForm.Companion.fromList(l: List<String>) =
    TaskForm(l[0], l[1], l[2], l[3], l[4], l[5], l[6].split(",").filter { it.isNotEmpty() }, l.getOrElse(7) { "" }, l.getOrElse(8) { "" }, l.getOrNull(9)?.toIntOrNull() ?: 0, l.getOrNull(10)?.toLongOrNull(), l.getOrNull(11)?.split(",")?.mapNotNull { it.toIntOrNull() } ?: emptyList(), l.getOrNull(12)?.split(",")?.mapNotNull { it.toLongOrNull() } ?: emptyList())

/** The planning typed on Nova tarefa; empty (the default) sends nothing, as before. */
internal data class NewPlanning(
    val start: String = "", val end: String = "", val estimateMinutes: Int = 0, val reminders: List<Int> = emptyList()
) {
    /** What the create call carries: only what is set, under the core's own keys. */
    fun fields(): Map<String, Any> = buildMap {
        if (start.isNotEmpty()) put("scheduled_start", start)
        if (end.isNotEmpty()) put("scheduled_end", end)
        if (estimateMinutes > 0) put("estimate_minutes", estimateMinutes)
        if (reminders.isNotEmpty()) put("reminder_offsets_minutes", reminders)
    }

    /** Why Criar must wait: only one of start and end is filled (the core wants both or neither); else "". */
    fun problem(): String = if ((start.isEmpty()) != (end.isEmpty())) NewPlanning.BOTH else ""

    companion object {
        const val BOTH = "Preencha o início e o fim do planejamento, ou deixe os dois em branco."
    }
}

/** Decisions of the task screen. */
internal object TaskRules {
    fun prerequisites(ids: List<Long>, tasks: List<TaskItem>): List<TaskItem> {
        val byId = tasks.associateBy { it.id }
        return ids.distinct().mapNotNull { byId[it] }
    }

    fun prerequisiteSymbol(task: TaskItem): String = when {
        task.done -> "✓"
        task.overdue -> "⚠"
        else -> "○"
    }

    // ------------------------------------------------------------ planning (start, end, estimate)

    const val PLANNING_HINT =
        "Quando você vai trabalhar nela. Não muda o prazo. Hora em branco: 09:00 no início e 18:00 no fim."
    const val HOUR_INVALID = "Hora inválida. Use 09:00."
    const val START_HOUR = "09:00"
    const val END_HOUR = "18:00"
    const val MINUTES_PER_DAY = 1440

    private val hourPattern = Regex("""^(\d{1,2})(?:[:hH](\d{2})?)?$""")

    /** "HH:MM" for "09:00", "9:00", "9h", "09h30" or "9"; null when it is not an hour of the day. */
    fun normalizeHour(text: String): String? {
        val m = hourPattern.matchEntire(text.trim()) ?: return null
        val h = m.groupValues[1].toInt()
        val min = m.groupValues[2].ifEmpty { "0" }.toInt()
        if (h !in 0..23 || min !in 0..59) return null
        return "%02d:%02d".format(h, min)
    }

    private fun validDate(date: String) = try {
        java.time.LocalDate.parse(date); true
    } catch (_: Exception) {
        false
    }

    /**
     * The stored moment ("yyyy-MM-ddTHH:mm") of a [date] and an [hour] typed by the person.
     * Blank date: "" (no moment). Blank hour: [defaultHour]. Invalid hour: null.
     */
    fun composeMoment(date: String, hour: String, defaultHour: String): String? {
        if (date.isBlank()) return ""
        if (!validDate(date.trim())) return null
        val h = if (hour.isBlank()) defaultHour else normalizeHour(hour) ?: return null
        return "${date.trim()}T$h"
    }

    /** A stored moment back into its date and the hour to show ("" when there is no valid hour). */
    fun splitMoment(value: String): Pair<String, String> {
        val v = value.trim()
        if (v.length < 10 || !validDate(v.substring(0, 10))) return "" to ""
        val hour = if (v.length > 10 && (v[10] == 'T' || v[10] == ' ')) normalizeHour(v.substring(11).take(5)) else null
        return v.substring(0, 10) to (hour ?: "")
    }

    /** Whole days of an estimate, rounded up: 0 for none. */
    fun estimateDays(minutes: Int): Int = if (minutes <= 0) 0 else 1 + (minutes - 1) / MINUTES_PER_DAY

    /** "Estimativa anterior: N min" for a stored estimate that is not a whole number of days; else "". */
    fun legacyEstimateNote(minutes: Int): String =
        if (minutes > 0 && minutes % MINUTES_PER_DAY != 0) "Estimativa anterior: $minutes min" else ""

    /** The text typed for the days: a whole number from 0, or null. */
    fun parseDays(text: String): Int? = if (text.isBlank()) 0 else text.trim().toIntOrNull()?.takeIf { it in 0..3650 }

    /**
     * The end to fill in after the start or the days changed: start date + (days - 1) at 18:00, only
     * with a start, days above zero and no end yet. Null when nothing should be filled.
     */
    fun autoEnd(start: String, days: Int, end: String): String? {
        if (days <= 0 || end.isNotBlank()) return null
        val (date, _) = splitMoment(start)
        if (date.isEmpty()) return null
        return "${java.time.LocalDate.parse(date).plusDays(days - 1L)}T$END_HOUR"
    }

    /** "⚠ O fim planejado passa do prazo (dd/mm/aaaa)." when the planned end is after a valid due date; else "". */
    fun overDueWarning(dueDate: String, scheduledEnd: String): String {
        val due = dueDate.trim().take(10)
        val (endDate, _) = splitMoment(scheduledEnd)
        if (endDate.isEmpty() || !validDate(due)) return ""
        if (java.time.LocalDate.parse(endDate) <= java.time.LocalDate.parse(due)) return ""
        return "⚠ O fim planejado passa do prazo (${due.substring(8, 10)}/${due.substring(5, 7)}/${due.substring(0, 4)})."
    }

    /** One choice of the reminders: [legacy] ones are offsets the task already had that are no longer offered. */
    data class ReminderOption(val minutes: Int, val label: String, val legacy: Boolean = false)

    private val standardReminders = listOf(
        ReminderOption(0, "No início"), ReminderOption(1440, "1 dia antes"), ReminderOption(10080, "7 dias antes")
    )

    private fun legacyLabel(minutes: Int): String = when {
        minutes % 1440 == 0 -> "${minutes / 1440} ${if (minutes == 1440) "dia" else "dias"} antes (antigo)"
        minutes % 60 == 0 -> "${minutes / 60} ${if (minutes == 60) "hora" else "horas"} antes (antigo)"
        else -> "$minutes min antes (antigo)"
    }

    /** The standard options, then any other offset the task has (selected), smallest first. */
    fun reminderOptions(offsets: List<Int>): List<ReminderOption> =
        standardReminders + offsets.distinct().filter { o -> standardReminders.none { it.minutes == o } }.sorted()
            .map { ReminderOption(it, legacyLabel(it), legacy = true) }

    /** One line under "Avisos antes do início" when the permission to notify was refused. */
    const val REMINDERS_BLOCKED =
        "Sem permissão para notificações, os avisos desta tarefa não aparecem. Permita-as nas configurações do telefone."

    /** The hint is for a task that asks for reminders while the phone does not let the app notify. */
    fun remindersBlockedShown(offsets: List<Int>, blocked: Boolean) = blocked && offsets.isNotEmpty()

    /** Asking for the permission is for the moment an offset is added, not when one is taken away. */
    fun offsetAdded(before: List<Int>, after: List<Int>) = after.any { it !in before }

    /**
     * The text of the core's conflict answer (mobile/rust/src/lib.rs, `TASK_CHANGED`); it also says
     * what the draft rule below detects, so the person reads one sentence for both.
     */
    const val CHANGED = "Esta tarefa foi alterada depois que você a abriu. Abra-a de novo para ver a versão atual."

    /** The task the screen of route [routeId] may show: only that one, never another that is loaded. */
    fun shown(routeId: Long, state: TaskEditState): TaskDetail? = state.task?.takeIf { it.id == routeId }

    /** A new draft for [task], at the screen state's [epoch]. */
    fun start(task: TaskDetail, epoch: Int) = TaskDraft(epoch, task.version, task.form, task.form)

    /**
     * Fits [draft] to the stored [task] just loaded:
     * - the person asked for the current version (other [epoch]): start over from it;
     * - same version: the draft stands;
     * - newer version and nothing typed: start over from it (silently);
     * - newer version and something typed: keep the text, flag the conflict (a save would send the
     *   old version and be refused).
     */
    fun reconcile(draft: TaskDraft, task: TaskDetail, epoch: Int): TaskDraft = when {
        draft.epoch != epoch -> start(task, epoch)
        draft.baseVersion == task.version -> draft.copy(conflict = false)
        !draft.changed -> start(task, epoch)
        else -> draft.copy(conflict = true)
    }

    fun isConflict(message: String) = message == CHANGED

    /**
     * The stack after opening task [id]: at most one task screen, always the new one on top. What was
     * under the old one stays, so Back goes to where the person was (a ficha), never to an editor
     * that belongs to another task.
     */
    fun withTask(stack: List<Route>, id: Long): List<Route> =
        stack.filter { it !is Route.EditTask && it !is Route.Pick } + Route.EditTask(id)

    /** The stack coming back after the process died: only the top-most task screen, and its own list. */
    fun restored(stack: List<Route>): List<Route> {
        val editor = stack.lastOrNull { it is Route.EditTask } as Route.EditTask?
        return stack.filter {
            when (it) {
                is Route.EditTask -> it == editor
                is Route.Pick -> it.taskId == editor?.id
                else -> true
            }
        }
    }
}

/** Possible parents, excluding the whole subtree even through concluded tasks. */
internal fun parentCandidates(tasks: List<TaskItem>, taskId: Long, query: String): List<TaskItem> {
    val excluded = mutableSetOf(taskId)
    val children = tasks.groupBy { it.parentTaskId }
    val pending = ArrayDeque<Long>()
    pending.add(taskId)
    while (pending.isNotEmpty()) {
        for (child in children[pending.removeFirst()].orEmpty()) {
            if (excluded.add(child.id)) pending.add(child.id)
        }
    }
    val q = foldText(query)
    return tasks.filter { !it.done && it.id !in excluded && foldText(it.title).contains(q) }
}

/** Tasks that can be prerequisites without creating a dependency cycle. */
internal fun dependencyCandidates(tasks: List<TaskItem>, taskId: Long, query: String): List<TaskItem> {
    val dependents = mutableMapOf<Long, MutableList<Long>>()
    tasks.forEach { task -> task.dependencies.forEach { dependents.getOrPut(it) { mutableListOf() }.add(task.id) } }
    val excluded = mutableSetOf(taskId)
    val pending = ArrayDeque<Long>()
    pending.add(taskId)
    while (pending.isNotEmpty()) {
        dependents[pending.removeFirst()].orEmpty().forEach { if (excluded.add(it)) pending.add(it) }
    }
    val q = foldText(query)
    return tasks.filter { it.id !in excluded && foldText(it.title).contains(q) }.sortedBy { it.done }
}
