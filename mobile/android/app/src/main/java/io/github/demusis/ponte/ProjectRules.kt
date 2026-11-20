package io.github.demusis.ponte

import org.json.JSONObject
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.temporal.ChronoUnit

// Projects: an optional grouping of tasks, made on the computer or here. The core does the numbers
// (progress, risk, timeline); this file only reads them and holds the rules of the screens, free of
// Android so they are tested on the computer.

/** How far along a project is, as the core counted it. [nextDue] is AAAA-MM-DD, empty when no open task has a due date. */
internal data class ProjectProgress(
    val total: Int = 0,
    val done: Int = 0,
    val late: Int = 0,
    val nextDue: String = ""
)

/** [targetAtRisk] is the latest planned or projected end (AAAA-MM-DDTHH:MM) when it falls after the target date; else empty. */
internal data class ProjectRisk(val deliveriesAtRisk: Int = 0, val targetAtRisk: String = "")

internal data class ProjectItem(
    val uid: String,
    val name: String,
    val objective: String = "",
    val assignee: String = "",
    /** The core's key: "ativo", "pausado", "concluido" or "arquivado". */
    val status: String = ProjectRules.ACTIVE,
    val statusLabel: String = "",
    /** AAAA-MM-DD or empty. */
    val startDate: String = "",
    val targetDate: String = "",
    /** What a save must present back. */
    val version: Long = 0,
    val progress: ProjectProgress = ProjectProgress(),
    val risk: ProjectRisk = ProjectRisk()
)

/** One row of the timeline: [start] and [end] are AAAA-MM-DDTHH:MM, empty without a planned period; [due] is AAAA-MM-DD. */
internal data class TimelineItem(
    val id: Long,
    val uid: String,
    val title: String,
    val done: Boolean,
    val start: String = "",
    val end: String = "",
    /** The period was worked out from the dependencies, not fixed on the task. */
    val projected: Boolean = false,
    val due: String = "",
    /** The period ends after the due date. */
    val late: Boolean = false
) {
    val hasPeriod: Boolean get() = start.isNotEmpty() && end.isNotEmpty()
}

internal data class TimelineSpan(val start: String, val end: String)

internal data class ProjectDetail(
    val project: ProjectItem,
    val tasks: List<TaskItem>,
    val timeline: List<TimelineItem>,
    val span: TimelineSpan?
)

/** The Projetos list as the core sent it (by name). */
internal data class ProjectsState(
    val loading: Boolean = true,
    val error: String? = null,
    val rows: List<ProjectItem> = emptyList()
) {
    fun byUid(uid: String): ProjectItem? = rows.firstOrNull { it.uid == uid }
}

/** The project screen: the project the screen was opened for ([uid]) and what was read of it. */
internal data class ProjectViewState(
    val uid: String = "",
    val loading: Boolean = true,
    val error: String? = null,
    val detail: ProjectDetail? = null,
    /** A change of situation or a deletion is on its way: the menu waits. */
    val busy: Boolean = false
)

/** The project form: saving, the reason of a refusal, and the [epoch] that restarts it from the stored project. */
internal data class ProjectEditState(
    val busy: Boolean = false,
    val error: String = "",
    /** [error] says the project changed since the form was opened (offers "Ver a versão atual"). */
    val conflict: Boolean = false,
    val epoch: Int = 0
)

/**
 * The parts of the Tarefas area. The ids are stable (they are kept in the saved state); the bar shows them in
 * [ORDER], with Projetos next to Tarefas.
 */
internal object TasksSegments {
    const val TASKS = 0
    const val ATTENDANCES = 1
    const val RESOURCES = 2
    const val PROJECTS = 3

    val ORDER = listOf(TASKS, PROJECTS, ATTENDANCES, RESOURCES)
    val LABELS = mapOf(TASKS to "Tarefas", PROJECTS to "Projetos", ATTENDANCES to "Atendimentos", RESOURCES to PhoneWords.RES_AREA)

    /** The labels in the order of the bar. */
    fun labels(): List<String> = ORDER.map { LABELS.getValue(it) }

    /** The position in the bar of the segment [id], and the segment at a position. */
    fun position(id: Int): Int = ORDER.indexOf(id).coerceAtLeast(0)
    fun at(position: Int): Int = ORDER.getOrElse(position) { TASKS }
}

/** What the project form edits. */
internal data class ProjectForm(
    val name: String = "",
    val objective: String = "",
    val assignee: String = "",
    val status: String = ProjectRules.ACTIVE,
    val startDate: String = "",
    val targetDate: String = ""
) {
    /** Whether anything differs from [saved] (blanks around the name and the responsible person do not count). */
    fun changedFrom(saved: ProjectForm): Boolean =
        name.trim() != saved.name.trim() || objective != saved.objective || assignee.trim() != saved.assignee.trim() ||
            status != saved.status || startDate != saved.startDate || targetDate != saved.targetDate

    companion object {
        fun of(p: ProjectItem) = ProjectForm(p.name, p.objective, p.assignee, p.status, p.startDate, p.targetDate)
    }
}

// ---------------------------------------------------------------- reading the core's JSON

internal fun JSONObject.toProject(): ProjectItem {
    val progress = optJSONObject("progress")
    val risk = optJSONObject("risk")
    return ProjectItem(
        uid = s("uid"), name = s("name"), objective = s("objective"), assignee = s("assignee"),
        status = s("status", ProjectRules.ACTIVE).ifEmpty { ProjectRules.ACTIVE },
        statusLabel = s("status_label"), startDate = s("start_date"), targetDate = s("target_date"),
        version = optLong("version"),
        progress = ProjectProgress(
            total = progress?.optInt("total") ?: 0, done = progress?.optInt("done") ?: 0,
            late = progress?.optInt("late") ?: 0, nextDue = progress?.s("next_due") ?: ""
        ),
        risk = ProjectRisk(risk?.optInt("deliveries_at_risk") ?: 0, risk?.s("target_at_risk") ?: "")
    )
}

internal fun JSONObject.toTimelineItem() = TimelineItem(
    id = optLong("id"), uid = s("uid"), title = s("title"), done = optBoolean("done"),
    start = s("start"), end = s("end"), projected = optBoolean("projected"), due = s("due"), late = optBoolean("late")
)

internal fun JSONObject.toProjectDetail(): ProjectDetail {
    val span = optJSONObject("span")?.let { TimelineSpan(it.s("start"), it.s("end")) }
    return ProjectDetail(
        project = getJSONObject("project").toProject(),
        tasks = optJSONArray("tasks").objs().map { it.toTask() },
        timeline = optJSONArray("timeline").objs().map { it.toTimelineItem() },
        span = span?.takeIf { it.start.isNotEmpty() && it.end.isNotEmpty() }
    )
}

// ---------------------------------------------------------------- the timeline, as numbers

internal enum class BarKind { PLAN, DONE, LATE }

/** A bar: [x] and [width] in the unit of the layout (dp on the screen); [row] counts the rows that have a period. */
internal data class TimelineBar(val row: Int, val x: Float, val width: Float, val kind: BarKind, val projected: Boolean)

internal data class TimelineMark(val row: Int, val x: Float)

internal data class TimelineTick(val x: Float, val label: String)

/** The whole chart worked out; [x] values grow with time, [width] is the whole time area. */
internal data class TimelineLayout(
    val firstDay: LocalDate,
    val days: Int,
    val dayWidth: Float,
    val width: Float,
    /** The items with a period, in the order of the core: one chart row each. */
    val rows: List<TimelineItem>,
    val bars: List<TimelineBar>,
    /** Where the due date of a row falls: the end of that day. */
    val dues: List<TimelineMark>,
    val ticks: List<TimelineTick>,
    /** Saturdays and Sundays, as (from, to). */
    val weekends: List<Pair<Float, Float>>,
    /** The middle of today, or null when today is out of the chart. */
    val todayX: Float?,
    /** The end of the target day, or null without a target date or when it is out of the chart. */
    val targetX: Float?
)

internal object ProjectRules {
    const val ACTIVE = "ativo"
    const val PAUSED = "pausado"
    const val DONE = "concluido"
    const val ARCHIVED = "arquivado"

    /** The filter of the list that shows every project. */
    const val ALL = "todos"
    const val DEFAULT_FILTER = ACTIVE

    /** The task list's filter for "tasks of no project" (never a real uid); empty means every task. */
    const val NONE = "sem-projeto"

    const val MAX_NAME = 200

    /** The text of the core's answer when the project changed since the form was opened. */
    const val CHANGED = "Este projeto foi alterado depois que você o abriu. Abra-o de novo para ver a versão atual."

    fun isConflict(message: String) = message == CHANGED

    val FILTERS = listOf(
        ACTIVE to "Ativos", PAUSED to "Pausados", DONE to "Concluídos", ARCHIVED to "Arquivados", ALL to "Todos"
    )

    /** The situations of the form. */
    val STATUSES = listOf(
        Choice(ACTIVE, "Ativo"), Choice(PAUSED, "Pausado"), Choice(DONE, "Concluído"), Choice(ARCHIVED, "Arquivado")
    )

    fun statusLabel(status: String): String = STATUSES.firstOrNull { it.key == status }?.label ?: status

    /** One sentence for a person who has no project yet. */
    const val WHAT_IS =
        "Um projeto reúne as tarefas de um mesmo objetivo, com prazo e andamento num só lugar."

    // ------------------------------------------------------------ the list

    private fun rank(status: String) = when (status) {
        ACTIVE -> 0
        PAUSED -> 1
        DONE -> 2
        else -> 3
    }

    /**
     * The projects to show: those of the chosen situation ([ALL] for every one) whose name or objective
     * has the typed words (accents and case ignored). The core's order (by name) is kept; "Todos" puts
     * the active ones first.
     */
    fun visible(rows: List<ProjectItem>, filter: String, query: String): List<ProjectItem> {
        val q = foldText(query.trim())
        val shown = rows.filter {
            (filter == ALL || it.status == filter) &&
                (q.isEmpty() || foldText(it.name).contains(q) || foldText(it.objective).contains(q))
        }
        return if (filter == ALL) shown.sortedBy { rank(it.status) } else shown
    }

    /** The projects the search screen lists for [query] (two letters or more): name or objective, by name. */
    fun search(rows: List<ProjectItem>, query: String): List<ProjectItem> =
        if (query.trim().length < 2) emptyList() else visible(rows, ALL, query)

    /** What the list says when [shown] is empty. */
    enum class Empty { NONE_AT_ALL, NOTHING_FOUND, NONE_IN_FILTER }

    fun emptyKind(all: List<ProjectItem>, query: String): Empty = when {
        all.isEmpty() -> Empty.NONE_AT_ALL
        query.isNotBlank() -> Empty.NOTHING_FOUND
        else -> Empty.NONE_IN_FILTER
    }

    /** "Nenhum projeto ativo." for the empty list of a situation. */
    fun noneIn(filter: String): String = when (filter) {
        ACTIVE -> "Nenhum projeto ativo."
        PAUSED -> "Nenhum projeto pausado."
        DONE -> "Nenhum projeto concluído."
        ARCHIVED -> "Nenhum projeto arquivado."
        else -> "Nenhum projeto."
    }

    // ------------------------------------------------------------ labels

    /** "3 de 8 tarefas concluídas"; a project with no task says so. */
    fun progressLine(p: ProjectProgress): String =
        if (p.total == 0) "Nenhuma tarefa ainda" else "${p.done} de ${p.total} " + if (p.total == 1) "tarefa concluída" else "tarefas concluídas"

    /** 0 to 1 for the bar. */
    fun fraction(p: ProjectProgress): Float = if (p.total <= 0) 0f else (p.done.toFloat() / p.total).coerceIn(0f, 1f)

    /** "Próximo prazo: 10/11", or empty when no open task has a due date. */
    fun nextDueLine(p: ProjectProgress): String {
        val d = shortDate(p.nextDue)
        return if (d.isEmpty()) "" else "Próximo prazo: $d"
    }

    fun lateLabel(n: Int) = if (n == 1) "1 atrasada" else "$n atrasadas"

    fun riskLabel(n: Int) = "$n em risco"

    /** "Data-alvo 24/11 em risco", or empty when the target is not at risk (or has no date). */
    fun targetRiskLine(p: ProjectItem): String {
        val target = shortDate(p.targetDate)
        if (p.risk.targetAtRisk.isEmpty() || target.isEmpty()) return ""
        return "Data-alvo $target em risco"
    }

    /** The longer sentence of the project screen: "Data-alvo 24/11 em risco: o plano termina em 25/11." */
    fun targetRiskSentence(p: ProjectItem): String {
        val line = targetRiskLine(p)
        val end = shortDate(p.risk.targetAtRisk)
        return if (line.isEmpty()) "" else if (end.isEmpty()) "$line." else "$line: o plano termina em $end."
    }

    /** dd/mm of an AAAA-MM-DD date or an AAAA-MM-DDTHH:MM moment; empty for anything else. */
    fun shortDate(value: String): String {
        val d = dateOf(value) ?: return ""
        return "%02d/%02d".format(d.dayOfMonth, d.monthValue)
    }

    private fun dateOf(value: String): LocalDate? =
        if (value.length < 10) null else try { LocalDate.parse(value.substring(0, 10)) } catch (_: Exception) { null }

    /** "Início 01/11 · Data-alvo 24/11", with "—" for a missing date. */
    fun datesLine(p: ProjectItem): String = "Início " + dayOrDash(p.startDate) + " · Data-alvo " + dayOrDash(p.targetDate)

    private fun dayOrDash(iso: String) = if (iso.isEmpty()) "—" else formatDocDate(iso)

    // ------------------------------------------------------------ the project's tasks

    /** The tasks of a project in the groups of its screen. */
    data class TaskGroups(val late: List<TaskItem>, val open: List<TaskItem>, val done: List<TaskItem>)

    fun groups(tasks: List<TaskItem>): TaskGroups = TaskGroups(
        late = tasks.filter { !it.done && it.overdue },
        open = tasks.filter { !it.done && !it.overdue },
        done = tasks.filter { it.done }
    )

    /**
     * The open tasks with no project that "Incluir tarefas" offers, filtered by the typed words in the title,
     * those with a due date first (earliest first), then by title.
     */
    fun candidates(tasks: List<TaskItem>, query: String): List<TaskItem> {
        val q = foldText(query.trim())
        return tasks.filter { !it.done && it.projectUid.isEmpty() && (q.isEmpty() || foldText(it.title).contains(q)) }
            .sortedWith(
                compareBy<TaskItem> { it.dueDate.isEmpty() }.thenBy { it.dueDate }.thenBy { foldText(it.title) }
            )
    }

    fun includeLabel(n: Int) = if (n <= 0) "Incluir tarefas" else if (n == 1) "Incluir 1 tarefa" else "Incluir $n tarefas"

    fun includedMessage(n: Int) = if (n == 1) "1 tarefa incluída no projeto." else "$n tarefas incluídas no projeto."

    /** What the confirmation of "Excluir projeto" says. */
    fun deleteQuestion(total: Int): String = when (total) {
        0 -> "O projeto será excluído. Não há tarefas nele."
        1 -> "A tarefa do projeto continua em Tarefas e volta para “Sem projeto”."
        else -> "As $total tarefas do projeto continuam em Tarefas e voltam para “Sem projeto”."
    }

    fun deletedMessage(released: Int): String = when (released) {
        0 -> "Projeto excluído."
        1 -> "Projeto excluído. 1 tarefa voltou para “Sem projeto”."
        else -> "Projeto excluído. $released tarefas voltaram para “Sem projeto”."
    }

    // ------------------------------------------------------------ the Projeto field of a task, and the task filter

    /**
     * The choices of the Projeto field: "Sem projeto" (key empty), the projects that are not archived, and the
     * task's own project when it is archived (so it can be seen and kept) or no longer known ([currentName]).
     */
    fun fieldOptions(rows: List<ProjectItem>, currentUid: String, currentName: String = ""): List<Choice> {
        val own = rows.firstOrNull { it.uid == currentUid }
        val list = rows.filter { it.status != ARCHIVED || it.uid == currentUid }
            .map { Choice(it.uid, if (it.status == ARCHIVED) "${it.name} (arquivado)" else it.name) }
        val missing = if (currentUid.isNotEmpty() && own == null) {
            listOf(Choice(currentUid, currentName.ifEmpty { "Projeto não encontrado" }))
        } else emptyList()
        return listOf(Choice("", "Sem projeto")) + list + missing
    }

    /** The field is worth showing when there is something to choose, or the task already has a project. */
    fun fieldShown(options: List<Choice>, currentUid: String) = options.size > 1 || currentUid.isNotEmpty()

    /** The task list's filter: [filter] is empty (all), [NONE], or a project uid. */
    fun matchesTask(projectUid: String, filter: String): Boolean = when (filter) {
        "" -> true
        NONE -> projectUid.isEmpty()
        else -> projectUid == filter
    }

    /** The text of the filter's chip: "Projeto", "Sem projeto" or the project's name. */
    fun filterLabel(filter: String, rows: List<ProjectItem>): String = when (filter) {
        "" -> "Projeto"
        NONE -> "Sem projeto"
        else -> rows.firstOrNull { it.uid == filter }?.name ?: "Projeto"
    }

    /** The projects the filter offers: every one, the archived ones last. */
    fun filterOptions(rows: List<ProjectItem>): List<ProjectItem> = rows.sortedBy { it.status == ARCHIVED }

    /** A filter on a project that does not exist any more is dropped. */
    fun pruneFilter(filter: String, rows: List<ProjectItem>): String =
        if (filter.isEmpty() || filter == NONE || rows.any { it.uid == filter }) filter else ""

    // ------------------------------------------------------------ the form

    /** Why the form cannot be saved, or empty. */
    fun formProblem(form: ProjectForm): String = when {
        form.name.isBlank() -> "Informe o nome."
        form.startDate.isNotEmpty() && form.targetDate.isNotEmpty() && form.targetDate < form.startDate ->
            "A data-alvo não pode ser anterior ao início."
        else -> ""
    }

    /** What `saveProject` takes: [uid] and [version] only for an existing project. */
    fun saveJson(uid: String, version: Long, form: ProjectForm): JSONObject {
        val o = JSONObject()
        if (uid.isNotEmpty()) o.put("uid", uid).put("version", version)
        return o.put("name", form.name.trim()).put("objective", form.objective).put("assignee", form.assignee.trim())
            .put("status", form.status).put("start_date", form.startDate).put("target_date", form.targetDate)
    }

    /** `saveProject` for a change of situation only. */
    fun statusJson(uid: String, version: Long, status: String): JSONObject =
        JSONObject().put("uid", uid).put("version", version).put("status", status)

    /** "Arquivar" for any project that is not archived. */
    fun canArchive(p: ProjectItem) = p.status != ARCHIVED

    /** "Reabrir" brings a concluded or archived project back to active. */
    fun canReopen(p: ProjectItem) = p.status == ARCHIVED || p.status == DONE

    // ------------------------------------------------------------ the timeline chart

    const val ROW_HEIGHT = 44f
    const val HEADER_HEIGHT = 28f
    const val BAR_HEIGHT = 18f
    const val MIN_DAY_WIDTH = 16f
    const val MIN_BAR_WIDTH = 6f
    private const val MIN_LABEL_GAP = 44f
    private const val MAX_EXTENDED_DAYS = 400
    private val TICK_STEPS = listOf(1, 2, 3, 7, 14, 28)

    private fun momentOf(value: String): LocalDateTime? = try { LocalDateTime.parse(value) } catch (_: Exception) { null }

    /**
     * The chart: the days of the core's [span] with a day of room at each side (and today and the target date
     * when they are near enough), a day at least [MIN_DAY_WIDTH] wide and the whole [availableWidth] used when
     * the days are few. Only items with a period are rows. Null when there is nothing to draw.
     */
    fun layout(
        items: List<TimelineItem>,
        span: TimelineSpan?,
        today: LocalDate,
        targetDate: String,
        availableWidth: Float,
        minDayWidth: Float = MIN_DAY_WIDTH
    ): TimelineLayout? {
        val rows = items.filter { it.hasPeriod && momentOf(it.start) != null && momentOf(it.end) != null }
        if (rows.isEmpty()) return null
        var lo = rows.minOf { momentOf(it.start)!!.toLocalDate() }
        var hi = rows.maxOf { momentOf(it.end)!!.toLocalDate() }
        rows.mapNotNull { dateOf(it.due) }.forEach { if (it < lo) lo = it; if (it > hi) hi = it }
        span?.let { s ->
            momentOf(s.start)?.toLocalDate()?.let { if (it < lo) lo = it }
            momentOf(s.end)?.toLocalDate()?.let { if (it > hi) hi = it }
        }
        val target = dateOf(targetDate)
        for (extra in listOfNotNull(today, target)) {
            val nlo = minOf(lo, extra)
            val nhi = maxOf(hi, extra)
            if (ChronoUnit.DAYS.between(nlo, nhi) <= MAX_EXTENDED_DAYS) { lo = nlo; hi = nhi }
        }
        val first = lo.minusDays(1)
        val days = (ChronoUnit.DAYS.between(first, hi) + 2).toInt()
        val dayWidth = maxOf(minDayWidth, availableWidth / days)

        fun dayIndex(d: LocalDate) = ChronoUnit.DAYS.between(first, d).toInt()
        fun x(m: LocalDateTime) = (dayIndex(m.toLocalDate()) + (m.hour * 60 + m.minute) / 1440f) * dayWidth

        val bars = rows.mapIndexed { i, it ->
            val x0 = x(momentOf(it.start)!!)
            val x1 = x(momentOf(it.end)!!)
            val kind = when {
                it.done -> BarKind.DONE
                it.late -> BarKind.LATE
                else -> BarKind.PLAN
            }
            TimelineBar(i, x0, maxOf(x1 - x0, MIN_BAR_WIDTH), kind, it.projected)
        }
        val dues = rows.mapIndexedNotNull { i, it ->
            val d = dateOf(it.due) ?: return@mapIndexedNotNull null
            val idx = dayIndex(d)
            if (idx in 0 until days) TimelineMark(i, (idx + 1) * dayWidth) else null
        }
        val step = TICK_STEPS.firstOrNull { it * dayWidth >= MIN_LABEL_GAP } ?: TICK_STEPS.last()
        val ticks = (0 until days step step).map { i ->
            val d = first.plusDays(i.toLong())
            TimelineTick(i * dayWidth, "%02d/%02d".format(d.dayOfMonth, d.monthValue))
        }
        val weekends = mutableListOf<Pair<Float, Float>>()
        for (i in 0 until days) {
            val dow = first.plusDays(i.toLong()).dayOfWeek
            if (dow == java.time.DayOfWeek.SATURDAY || dow == java.time.DayOfWeek.SUNDAY) {
                val from = i * dayWidth
                val last = weekends.lastOrNull()
                if (last != null && kotlin.math.abs(last.second - from) < 0.001f) {
                    weekends[weekends.lastIndex] = last.first to (i + 1) * dayWidth
                } else weekends.add(from to (i + 1) * dayWidth)
            }
        }
        val todayIdx = dayIndex(today)
        val targetIdx = target?.let { dayIndex(it) }
        return TimelineLayout(
            firstDay = first, days = days, dayWidth = dayWidth, width = days * dayWidth, rows = rows,
            bars = bars, dues = dues, ticks = ticks, weekends = weekends,
            todayX = if (todayIdx in 0 until days) (todayIdx + 0.5f) * dayWidth else null,
            targetX = if (targetIdx != null && targetIdx in 0 until days) (targetIdx + 1) * dayWidth else null
        )
    }

    /** The items the chart cannot place: no start or no end. */
    fun unplanned(items: List<TimelineItem>): List<TimelineItem> = items.filter { !it.hasPeriod }

    /** The chart row under a tap at height [y] (from the top of the chart, header included), or null. */
    fun rowAt(y: Float, rowCount: Int, header: Float = HEADER_HEIGHT, rowHeight: Float = ROW_HEIGHT): Int? {
        if (y < header) return null
        val i = ((y - header) / rowHeight).toInt()
        return if (i in 0 until rowCount) i else null
    }

    /** "24/11 a 26/11" (or "24/11" in one day) for the period of [item]; empty without one. */
    fun periodLabel(item: TimelineItem): String {
        val from = shortDate(item.start)
        val to = shortDate(item.end)
        if (from.isEmpty() || to.isEmpty()) return ""
        return if (from == to) from else "$from a $to"
    }

    /** What a screen reader says for a row: the title, the period, the due date and the state. */
    fun rowDescription(item: TimelineItem): String = buildList {
        add(item.title)
        val period = periodLabel(item)
        add(if (period.isEmpty()) "sem período planejado" else (if (item.projected) "período projetado " else "período ") + period)
        shortDate(item.due).takeIf { it.isNotEmpty() }?.let { add("prazo $it") }
        if (item.done) add("concluída") else if (item.late) add("termina depois do prazo")
    }.joinToString(". ")

    /** The line under "Sem período planejado" for one task. */
    fun unplannedLine(item: TimelineItem): String = buildList {
        shortDate(item.due).takeIf { it.isNotEmpty() }?.let { add("Prazo $it") }
        if (item.done) add("Concluída")
    }.joinToString(" · ")
}
