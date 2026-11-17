package io.github.demusis.ponte

import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter

/**
 * The individual notices of the tasks ("Começa às 09:00"), the rules that need no Android: which
 * moments a task asks to be told about, which of them are due now, which are too old to be told,
 * the ledger that keeps a notice from being shown twice (after a restart, a sync or a clock
 * change), the wording and the weekly grouping of Hoje. Free of Android and JSON types so plain
 * JUnit tests it; [PlannedNotices] (NoticeRuntime.kt) is the Android side.
 *
 * Identity: a notice is the task's uid + the anchor (the start, or the due date at 09:00, as the
 * local moment the task keeps) + the offset in minutes. The numeric id of the notification is NOT
 * a hash of that: the ledger hands out the smallest free number, so two notices never collide.
 * The task's `id` (a local row number) is never used, only its uid.
 */
internal object TaskReminders {
    /** The hour of the due date a task without start is announced at. */
    const val DEFAULT_HOUR = 9

    /**
     * How many offsets a task may use, and the longest one (a year): the same limits the core validates
     * (`reminder_offsets_minutes`: at most ten, up to 525600 minutes), so nothing the core saves is dropped here.
     * There is no arming horizon: the alarm is always for the nearest moment still ahead ([plan]), and every
     * refresh (the alarm itself, the app opening, a sync, boot) plans again, so a far moment is armed when it
     * becomes the nearest.
     */
    const val MAX_OFFSETS = 10
    const val MAX_OFFSET_MINUTES = 525_600

    /** A notice later than this after its moment (the phone was off) is not told, only recorded as past. */
    const val CATCH_UP_MINUTES = 360L

    /** Past this age an entry leaves the ledger, and an older moment is not even looked at. */
    const val FORGET_DAYS = 30L

    const val SNOOZE_MINUTES = 10L

    /** More notices than this at once become one line with a count. */
    const val MAX_AT_ONCE = 5

    /** The ids of notifications start here (1 and 2 belong to the daily notice and the summary). */
    const val FIRST_ID = 1000

    /** A retry after a read that failed (library locked or busy), only while the last notice was near. */
    const val RETRY_MINUTES = 15L

    /** Title on a locked screen, and when the app lock is in force: nothing about the task. */
    const val PUBLIC_TEXT = "Ponte de bolso: há uma tarefa a conferir."
    const val GENERIC_TITLE = "Aviso de tarefa"
    const val SUMMARY_TITLE = "Avisos de tarefas"

    private val MOMENT = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm")
    private val MOMENT_RE = Regex("""\d{4}-\d{2}-\d{2}T\d{2}:\d{2}""")
    private val DAY_RE = Regex("""\d{4}-\d{2}-\d{2}""")

    enum class Kind { START, DUE }

    /** What the notices need to know of a task (read from the core; the title is never stored). */
    data class Task(
        val uid: String,
        val title: String,
        /** `AAAA-MM-DDTHH:MM`, local time, or empty. */
        val scheduledStart: String,
        /** `AAAA-MM-DD` or empty. */
        val dueDate: String,
        val offsets: List<Int>,
        val done: Boolean
    )

    /** One moment a task asks to be told about. */
    data class Occurrence(
        val key: String,
        val uid: String,
        val kind: Kind,
        val anchor: LocalDateTime,
        val offset: Int,
        val trigger: ZonedDateTime
    )

    // ------------------------------------------------------------------ the moments

    /** A uid that can be kept in the ledger and in an intent: short, no separators. */
    fun validUid(uid: String) = uid.isNotBlank() && uid.length <= 80 && uid.none { it == '|' || it == '\t' || it == '\n' || it == '\r' }

    private fun parseMoment(text: String): LocalDateTime? {
        if (!MOMENT_RE.matches(text)) return null
        return try {
            LocalDateTime.parse(text, MOMENT).takeIf { it.format(MOMENT) == text }
        } catch (_: Exception) {
            null
        }
    }

    private fun parseDay(text: String): LocalDate? {
        if (!DAY_RE.matches(text)) return null
        return try {
            LocalDate.parse(text)
        } catch (_: Exception) {
            null
        }
    }

    /** The local moment the offsets count from: the start; without one, the due date at 09:00; else none. */
    fun anchorOf(task: Task): Pair<Kind, LocalDateTime>? {
        parseMoment(task.scheduledStart.trim())?.let { return Kind.START to it }
        parseDay(task.dueDate.trim())?.let { return Kind.DUE to LocalDateTime.of(it, LocalTime.of(DEFAULT_HOUR, 0)) }
        return null
    }

    /** The offsets as used: within range, no repeats, the farthest first, at most [MAX_OFFSETS]. */
    fun cleanOffsets(offsets: List<Int>): List<Int> =
        offsets.filter { it in 0..MAX_OFFSET_MINUTES }.distinct().sortedDescending().take(MAX_OFFSETS)

    fun keyOf(uid: String, anchor: LocalDateTime, offset: Int) = "$uid|${anchor.format(MOMENT)}|$offset"

    /** The uid of a key made by [keyOf], or null. */
    fun uidOfKey(key: String): String? = key.substringBefore('|', "").takeIf { it.isNotEmpty() && key.count { c -> c == '|' } == 2 }

    /** Every moment of the open tasks that ask for notices (a task without uid, anchor or offsets asks for none). */
    fun occurrences(tasks: List<Task>, zone: ZoneId): List<Occurrence> {
        val out = ArrayList<Occurrence>()
        for (t in tasks) {
            if (t.done || !validUid(t.uid)) continue
            val (kind, anchor) = anchorOf(t) ?: continue
            for (offset in cleanOffsets(t.offsets)) {
                // Calendar arithmetic in the local time, then the zone: a gap in the clock moves forward.
                val trigger = ZonedDateTime.of(anchor.minusMinutes(offset.toLong()), zone)
                out += Occurrence(keyOf(t.uid, anchor, offset), t.uid, kind, anchor, offset, trigger)
            }
        }
        return out
    }

    // ------------------------------------------------------------------ the ledger

    /**
     * What is remembered of a notice that was told (or judged past): its notification [id] (0 when
     * none is on the screen, or it was folded in a count), the moment it was due and, when the person
     * asked for it later, the [snoozeUntilMillis].
     */
    data class Entry(val id: Int, val triggerMillis: Long, val snoozeUntilMillis: Long = 0L)

    /**
     * The notices already told, by key. Text-serialisable (one line per entry) so it lives in the
     * app's preferences: it holds keys (uid, local moment, offset) and numbers, never a title.
     */
    data class Ledger(val entries: Map<String, Entry> = emptyMap()) {
        fun has(key: String) = entries.containsKey(key)

        /** The smallest id from [FIRST_ID] that no entry uses. */
        fun freeId(): Int {
            val used = entries.values.mapNotNull { e -> e.id.takeIf { it > 0 } }.toHashSet()
            var id = FIRST_ID
            while (id in used) id++
            return id
        }

        fun put(key: String, entry: Entry) = Ledger(LinkedHashMap(entries).also { it[key] = entry })

        fun forget(olderThanMillis: Long) = Ledger(entries.filterValues { it.triggerMillis >= olderThanMillis })

        fun encode(): String = buildString {
            append(HEADER).append('\n')
            for ((key, e) in entries) append(e.id).append('\t').append(e.triggerMillis).append('\t').append(e.snoozeUntilMillis).append('\t').append(key).append('\n')
        }

        companion object {
            private const val HEADER = "ledger1"

            /** Reads what [encode] wrote; anything else (null, another version, a damaged line) is dropped, never thrown. */
            fun decode(text: String?): Ledger {
                if (text.isNullOrEmpty()) return Ledger()
                val lines = text.split('\n')
                if (lines.firstOrNull() != HEADER) return Ledger()
                val map = LinkedHashMap<String, Entry>()
                for (line in lines.drop(1)) {
                    val p = line.split('\t', limit = 4)
                    if (p.size != 4) continue
                    val id = p[0].toIntOrNull() ?: continue
                    val trig = p[1].toLongOrNull() ?: continue
                    val snooze = p[2].toLongOrNull() ?: continue
                    if (id < 0 || uidOfKey(p[3]) == null) continue
                    map[p[3]] = Entry(id, trig, snooze)
                }
                return Ledger(map)
            }
        }
    }

    // ------------------------------------------------------------------ the plan

    /** A notice to tell now; [snoozed] when it is the one the person asked to be told again. */
    data class Due(val occurrence: Occurrence, val snoozed: Boolean)

    /**
     * What to do at [now]: tell [show], record [silent] as past without telling, and wake up again
     * at [next] (null: nothing ahead).
     */
    data class Plan(val show: List<Due>, val silent: List<Occurrence>, val next: ZonedDateTime?)

    /**
     * The decision. A moment not in the [ledger]: ahead of now it is a candidate for [Plan.next];
     * reached, it is told if it is at most [CATCH_UP_MINUTES] old, otherwise recorded silently (a
     * task that arrives by sync with a start long gone must not shout). A moment in the ledger is never told again,
     * except the one snoozed, when its time comes. Done tasks have no moments at all, so they are
     * silent. Moments older than [FORGET_DAYS] are ignored.
     */
    fun plan(tasks: List<Task>, ledger: Ledger, now: ZonedDateTime): Plan {
        val nowMs = now.toInstant().toEpochMilli()
        val forgetMs = now.minusDays(FORGET_DAYS).toInstant().toEpochMilli()
        val show = ArrayList<Due>()
        val silent = ArrayList<Occurrence>()
        var next: ZonedDateTime? = null
        fun candidate(at: ZonedDateTime) { if (next == null || at.isBefore(next)) next = at }
        for (o in occurrences(tasks, now.zone)) {
            val entry = ledger.entries[o.key]
            if (entry != null) {
                if (entry.snoozeUntilMillis > 0) {
                    if (entry.snoozeUntilMillis <= nowMs) show += Due(o, snoozed = true)
                    else candidate(Instant.ofEpochMilli(entry.snoozeUntilMillis).atZone(now.zone))
                }
                continue
            }
            val triggerMs = o.trigger.toInstant().toEpochMilli()
            when {
                triggerMs > nowMs -> candidate(o.trigger)
                triggerMs < forgetMs -> Unit
                nowMs - triggerMs <= CATCH_UP_MINUTES * 60_000L -> show += Due(o, snoozed = false)
                else -> silent += o
            }
        }
        show.sortWith(compareBy({ it.occurrence.trigger.toInstant() }, { it.occurrence.key }))
        return Plan(show, silent, next)
    }

    /** A notification to put on the screen. */
    data class Shown(val occurrence: Occurrence, val id: Int, val snoozed: Boolean)

    /** The outcome of [commit]: the new ledger, what to show, how many are folded in one count and which ids to take off the screen. */
    data class Commit(val ledger: Ledger, val shown: List<Shown>, val folded: Int, val cancel: List<Int>)

    /**
     * Applies a [plan] to the [ledger]: every notice told or judged past is recorded (so it never comes
     * back), the ledger is pruned, and the notifications of notices whose task was concluded, deleted
     * or moved are taken off the screen. The ledger is to be saved BEFORE anything is shown.
     */
    fun commit(tasks: List<Task>, ledger: Ledger, plan: Plan, now: ZonedDateTime): Commit {
        val forgetMs = now.minusDays(FORGET_DAYS).toInstant().toEpochMilli()
        var l = ledger.forget(forgetMs)
        val openKeys = occurrences(tasks, now.zone).mapTo(HashSet()) { it.key }
        val cancel = ArrayList<Int>()
        for ((key, e) in l.entries) {
            if (key !in openKeys && (e.id > 0 || e.snoozeUntilMillis > 0)) {
                if (e.id > 0) cancel += e.id
                l = l.put(key, Entry(0, e.triggerMillis, 0L))
            }
        }
        for (o in plan.silent) l = l.put(o.key, Entry(0, o.trigger.toInstant().toEpochMilli()))
        val shown = ArrayList<Shown>()
        var folded = 0
        plan.show.forEachIndexed { index, d ->
            val o = d.occurrence
            val triggerMs = o.trigger.toInstant().toEpochMilli()
            if (index < MAX_AT_ONCE) {
                val id = l.entries[o.key]?.id?.takeIf { it > 0 } ?: l.freeId()
                l = l.put(o.key, Entry(id, triggerMs))
                shown += Shown(o, id, d.snoozed)
            } else {
                folded++
                val old = l.entries[o.key]
                if (old != null && old.id > 0) cancel += old.id
                l = l.put(o.key, Entry(0, triggerMs))
            }
        }
        return Commit(l, shown, folded, cancel.distinct())
    }

    /** The ledger with [key] to be told again [SNOOZE_MINUTES] after [now]; null when that notice is unknown. */
    fun snooze(ledger: Ledger, key: String, now: ZonedDateTime): Ledger? {
        val e = ledger.entries[key] ?: return null
        return ledger.put(key, e.copy(snoozeUntilMillis = now.plusMinutes(SNOOZE_MINUTES).toInstant().toEpochMilli()))
    }

    /** When to look again after a read failed: soon, but only while the last planned moment is recent (never forever). */
    fun retryAt(lastNextMillis: Long, now: ZonedDateTime): ZonedDateTime? {
        if (lastNextMillis <= 0L) return null
        val age = now.toInstant().toEpochMilli() - lastNextMillis
        if (age > CATCH_UP_MINUTES * 60_000L) return null
        val retry = now.plusMinutes(RETRY_MINUTES)
        val planned = Instant.ofEpochMilli(lastNextMillis).atZone(now.zone)
        return if (planned.isAfter(retry)) planned else retry
    }

    // ------------------------------------------------------------------ the words

    /**
     * The title shown by the system: always the generic one. The plan keeps the task's
     * title out of the notification (it shows on the lock screen and in other apps' views);
     * the task opens with a tap. Arguments kept so callers stay as they are.
     */
    @Suppress("UNUSED_PARAMETER")
    fun title(taskTitle: String?, lockInForce: Boolean): String = GENERIC_TITLE

    private val DAY_MONTH = DateTimeFormatter.ofPattern("dd/MM")
    private val CLOCK = DateTimeFormatter.ofPattern("HH:mm")

    /** The second line: "Começa hoje às 09:00." or "Prazo amanhã.". Wording only; no title. */
    fun line(o: Occurrence, today: LocalDate): String {
        val day = o.anchor.toLocalDate()
        val when_ = when (day) {
            today -> "hoje"
            today.plusDays(1) -> "amanhã"
            else -> "em " + day.format(DAY_MONTH)
        }
        return when (o.kind) {
            Kind.START -> "Começa $when_ às ${o.anchor.format(CLOCK)}."
            Kind.DUE -> if (day == today || day == today.plusDays(1)) "Prazo $when_." else "Prazo: ${day.format(DAY_MONTH)}."
        }
    }

    /** The line of the count that folds the notices beyond [MAX_AT_ONCE]. */
    fun foldedLine(count: Int) = if (count == 1) "Mais 1 aviso de tarefa." else "Mais $count avisos de tarefas."
}

// ======================================================================== the week

/** What the week of Hoje shows: a task with planned time or a resource use, or a deadline without time. */
internal enum class WeekKind { TASK, USE, DEADLINE }

internal data class WeekEntry(
    val id: String,
    val title: String,
    val kind: WeekKind,
    /** Timed entries: start and end (the end may equal the start: a point). Null for a deadline. */
    val start: LocalDateTime? = null,
    val end: LocalDateTime? = null,
    /** A deadline's day. */
    val day: LocalDate? = null,
    val dependencyNotice: String? = null,
    val dependencyWarning: Boolean = false
)

/** The piece of an entry that falls on one day, cut at midnight ([to] is 24:00 as [LocalTime.MAX] when it goes on). */
internal data class WeekBlock(
    val entry: WeekEntry,
    val from: LocalTime,
    val to: LocalTime,
    val continuesBefore: Boolean,
    val continuesAfter: Boolean
)

internal data class WeekDay(val date: LocalDate, val blocks: List<WeekBlock>, val deadlines: List<WeekEntry>) {
    val isEmpty: Boolean get() = blocks.isEmpty() && deadlines.isEmpty()
}

/**
 * The week as days with hour blocks and, apart, the deadlines (days of the week, not a grid):
 * pure, so it is tested with fixed dates. Entries come from models already available ([task], [use]).
 */
internal object WeekPlan {
    fun dependencyNotice(task: TaskItem, tasks: List<TaskItem>): Pair<String, Boolean>? {
        val open = TaskRules.prerequisites(task.dependencies, tasks).filter { !it.done }
        if (open.isEmpty()) return null
        val start = moment(task.scheduledStart.trim())
        val wrong = open.firstOrNull { prerequisite ->
            val end = moment(prerequisite.scheduledEnd.trim())
            start != null && end != null && start.isBefore(end)
        }
        return if (wrong != null) "⚠ começa antes de terminar ${wrong.title}" to true
        else "↳ depende de ${open.size}" to false
    }

    fun taskEntries(task: TaskItem, tasks: List<TaskItem>): List<WeekEntry> {
        val notice = dependencyNotice(task, tasks)
        return task(task.uid, task.title, task.scheduledStart, task.scheduledEnd, task.estimateMinutes, task.dueDate)
            .map { if (it.kind == WeekKind.TASK && notice != null) it.copy(dependencyNotice = notice.first, dependencyWarning = notice.second) else it }
    }

    private val MOMENT = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm")

    fun moment(text: String): LocalDateTime? {
        if (text.length != 16) return null
        return try {
            LocalDateTime.parse(text, MOMENT).takeIf { it.format(MOMENT) == text }
        } catch (_: Exception) {
            null
        }
    }

    private fun day(text: String): LocalDate? = try {
        if (text.length == 10) LocalDate.parse(text) else null
    } catch (_: Exception) {
        null
    }

    /** The Monday of the week of [date]. */
    fun mondayOf(date: LocalDate): LocalDate = date.minusDays((date.dayOfWeek.value - 1).toLong())

    /**
     * A task as week entries: a timed block when it has a valid start (the end is the stated end if after
     * the start, else the start plus the estimate, else a point), and its due date as a separate deadline.
     */
    fun task(uid: String, title: String, scheduledStart: String, scheduledEnd: String, estimateMinutes: Int, dueDate: String): List<WeekEntry> {
        val out = ArrayList<WeekEntry>()
        val start = moment(scheduledStart.trim())
        if (start != null) {
            val stated = moment(scheduledEnd.trim())?.takeIf { it.isAfter(start) }
            val end = stated ?: if (estimateMinutes > 0) start.plusMinutes(estimateMinutes.toLong()) else start
            out += WeekEntry("t-$uid", title, WeekKind.TASK, start, end)
        }
        day(dueDate.trim())?.let { out += WeekEntry("d-$uid", title, WeekKind.DEADLINE, day = it) }
        return out
    }

    /** A use of a resource (reserved or in use) as a block; null for returned, cancelled or without valid moments. */
    fun use(u: ResourceUseItem): WeekEntry? {
        if (u.status == "devolvido" || u.status == "cancelado") return null
        val start = moment(u.start) ?: return null
        val end = moment(u.end)?.takeIf { it.isAfter(start) } ?: start
        return WeekEntry("u-${u.uid}", u.resourceName, WeekKind.USE, start, end)
    }

    /** The seven days from [weekStart], each with its blocks (by start) and deadlines; an entry crossing midnight is cut there. */
    fun days(weekStart: LocalDate, entries: List<WeekEntry>): List<WeekDay> = (0L until 7L).map { i ->
        val date = weekStart.plusDays(i)
        val dayStart = date.atStartOfDay()
        val dayEnd = date.plusDays(1).atStartOfDay()
        val blocks = entries.mapNotNull { e ->
            val s = e.start ?: return@mapNotNull null
            val en = e.end ?: s
            val point = !en.isAfter(s)
            val inDay = if (point) !s.isBefore(dayStart) && s.isBefore(dayEnd) else s.isBefore(dayEnd) && en.isAfter(dayStart)
            if (!inDay) return@mapNotNull null
            val from = if (s.isBefore(dayStart)) dayStart else s
            val to = if (en.isAfter(dayEnd)) dayEnd else en
            WeekBlock(
                e, from.toLocalTime(),
                if (to == dayEnd) LocalTime.MAX else to.toLocalTime(),
                continuesBefore = s.isBefore(dayStart),
                continuesAfter = en.isAfter(dayEnd)
            )
        }.sortedWith(compareBy({ it.from }, { it.entry.id }))
        val deadlines = entries.filter { it.kind == WeekKind.DEADLINE && it.day == date }.sortedBy { it.id }
        WeekDay(date, blocks, deadlines)
    }

    /** "09:00–10:30", "09:00" for a point; "até 10:00", "a partir das 22:00" or "o dia todo" for a block cut at midnight. */
    fun label(b: WeekBlock): String {
        val clock = DateTimeFormatter.ofPattern("HH:mm")
        val from = b.from.format(clock)
        val to = if (b.to == LocalTime.MAX) "24:00" else b.to.format(clock)
        return when {
            b.continuesBefore && b.continuesAfter -> "o dia todo"
            b.continuesBefore -> "até $to"
            b.continuesAfter -> "a partir das $from"
            b.to == b.from -> from
            else -> "$from–$to"
        }
    }
}
