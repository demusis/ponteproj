package io.github.demusis.ponte

import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

// Two-way sync between the open tasks of the phone and one calendar of the device. Everything here is
// plain Kotlin (no Android types): the rules, the engine and the ports it talks through, so JUnit tests
// it with fakes. The CalendarContract side lives in DeviceCalendar.kt.

/** What an event stands for: the deadline of a task, or its fixed planned period. */
internal enum class CalKind(val key: String) { DEADLINE("prazo"), PLAN("plano") }

/** One event of a task: the task's uid and the kind. */
internal data class CalKey(val uid: String, val kind: CalKind)

/** The part of a task the calendar cares about. [due] is AAAA-MM-DD; [start] and [end] AAAA-MM-DDTHH:mm. */
internal data class CalTask(
    val uid: String,
    val id: Long,
    val title: String,
    val done: Boolean,
    val due: String,
    val start: String,
    val end: String,
    /** The task's reminder choices, in minutes before the start. */
    val reminders: List<Int> = emptyList(),
    /** Number of the linked document; may be empty. */
    val docRef: String = "",
    /** The phone's editor would let this task be changed. */
    val editable: Boolean = !done
)

/** An event as the calendar holds it. Times are epoch milliseconds; an all-day event is UTC midnight to UTC midnight. */
internal data class CalEvent(
    val id: Long,
    val title: String,
    val description: String,
    val allDay: Boolean,
    val start: Long,
    val end: Long,
    /** False when the calendar gave neither an end nor a usable duration: never read as an edit, never rewritten. */
    val readable: Boolean = true
)

/** The event a task asks for. [taskStart] and [taskEnd] are the task's own values it was made from. */
internal data class CalWant(
    val key: CalKey,
    val title: String,
    /** The whole description for a new event; null on an update that leaves the person's text alone. */
    val description: String?,
    val allDay: Boolean,
    val start: Long,
    val end: Long,
    val reminders: List<Int>,
    val taskStart: String,
    val taskEnd: String
)

/** What the last sync left in the calendar for one key: the event and the values both sides agreed on. */
internal data class CalMark(
    val eventId: Long,
    val title: String,
    val allDay: Boolean,
    val start: Long,
    val end: Long,
    val reminders: List<Int>,
    val taskStart: String,
    val taskEnd: String
) {
    companion object {
        fun of(eventId: Long, w: CalWant) = CalMark(eventId, w.title, w.allDay, w.start, w.end, w.reminders, w.taskStart, w.taskEnd)
    }
}

internal data class CalSnapshot(val calendarId: Long, val marks: Map<CalKey, CalMark>)

/** A change of the task that the calendar asked for. [revert] is the event as the task has it, to put back if the task refuses. */
internal data class CalTaskChange(
    val key: CalKey,
    val eventId: Long,
    val title: String?,
    val due: String?,
    val start: String?,
    val end: String?,
    val revert: CalWant
)

internal data class CalPlan(
    val create: List<CalWant>,
    val update: List<Pair<Long, CalWant>>,
    val delete: List<Long>,
    val taskChanges: List<CalTaskChange>,
    val warnings: List<String>,
    /** The snapshot after the plan runs; a mark with event id 0 is for an event still to be created. */
    val marks: Map<CalKey, CalMark>
) {
    val isEmpty get() = create.isEmpty() && update.isEmpty() && delete.isEmpty() && taskChanges.isEmpty()
}

internal object CalendarRules {
    const val DEADLINE_PREFIX = "Prazo: "
    const val MAX_REMINDERS = 5
    private const val DAY_MS = 24L * 60 * 60 * 1000
    // Found anywhere in the text: the calendar app may wrap the description in HTML or break the lines.
    private val marker = Regex("""ponte-tarefa:([^:\s<>&"]+):(prazo|plano)""", RegexOption.IGNORE_CASE)
    private val prazoPrefix = Regex("""^\s*prazo\s*:\s*""", RegexOption.IGNORE_CASE)
    private val duration = Regex("""^P(?:(\d+)W)?(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?)?$""")
    private val moment = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm")

    fun markerLine(key: CalKey) = "ponte-tarefa:${key.uid}:${key.kind.key}"

    /** The key written in an event's description, or null. */
    fun keyOf(description: String): CalKey? {
        val m = marker.find(description) ?: return null
        val kind = CalKind.values().first { it.key == m.groupValues[2].lowercase() }
        return CalKey(m.groupValues[1], kind)
    }

    /** An RFC 2445 duration (P1D, PT1H30M, P2W…) in milliseconds; null when empty, zero or not a duration. */
    fun parseDuration(text: String?): Long? {
        val m = duration.matchEntire(text.orEmpty().trim().uppercase()) ?: return null
        val n = (1..5).map { m.groupValues[it].toLongOrNull() ?: 0L }
        val seconds = n[0] * 604800 + n[1] * 86400 + n[2] * 3600 + n[3] * 60 + n[4]
        return if (seconds > 0) seconds * 1000 else null
    }

    /** The title typed in a deadline event: every "Prazo:" at the start goes, whatever its case or spacing. */
    fun stripDeadlinePrefix(title: String): String {
        var t = title
        while (true) {
            val next = t.replace(prazoPrefix, "")
            if (next == t) return t.trim()
            t = next
        }
    }

    private fun hasMarker(e: CalEvent, key: CalKey) = keyOf(e.description) == key

    /** The want for an update: the person's description stays as it is, with the marker added when it is missing. */
    private fun withMarker(e: CalEvent, w: CalWant): CalWant =
        if (hasMarker(e, w.key)) w.copy(description = null)
        else w.copy(description = (e.description.trimEnd() + "\n" + markerLine(w.key)).trimStart('\n'))

    private fun parseDate(text: String): LocalDate? = try { LocalDate.parse(text.trim()) } catch (_: Exception) { null }
    private fun parseMoment(text: String): LocalDateTime? =
        try { LocalDateTime.parse(text.trim().replace(' ', 'T')) } catch (_: Exception) { null }

    /** The event a task asks for, or null when it asks for none (done, no date, no valid period). */
    fun wantOf(task: CalTask, kind: CalKind, zone: ZoneId): CalWant? {
        if (task.done || task.uid.isBlank()) return null
        val key = CalKey(task.uid, kind)
        val doc = if (task.docRef.isNotBlank()) "Documento: ${task.docRef}\n" else ""
        val description = doc + markerLine(key)
        return when (kind) {
            CalKind.DEADLINE -> {
                // The task keeps a date only, so the deadline is always an all-day event.
                val day = parseDate(task.due) ?: return null
                val start = day.atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()
                CalWant(key, DEADLINE_PREFIX + task.title, description, true, start, start + DAY_MS, emptyList(), task.due, "")
            }
            CalKind.PLAN -> {
                val s = parseMoment(task.start) ?: return null
                val e = parseMoment(task.end) ?: return null
                if (!e.isAfter(s)) return null
                CalWant(
                    key, task.title, description, false,
                    s.atZone(zone).toInstant().toEpochMilli(), e.atZone(zone).toInstant().toEpochMilli(),
                    task.reminders.filter { it >= 0 }.distinct().sorted().take(MAX_REMINDERS), task.start, task.end
                )
            }
        }
    }

    private fun eventDate(allDay: Boolean, startMs: Long, zone: ZoneId): LocalDate =
        Instant.ofEpochMilli(startMs).atZone(if (allDay) ZoneOffset.UTC else zone).toLocalDate()

    /** The task value an event's start stands for: the date and hour, or 09:00 of the day for an all-day event. */
    private fun startMoment(e: CalEvent, zone: ZoneId): String =
        if (e.allDay) "${eventDate(true, e.start, zone)}T${TaskRules.START_HOUR}"
        else Instant.ofEpochMilli(e.start).atZone(zone).toLocalDateTime().format(moment)

    /** The same for the end; an all-day event ends the evening of its last day (its end is the next midnight). */
    private fun endMoment(e: CalEvent, zone: ZoneId): String =
        if (e.allDay) "${eventDate(true, maxOf(e.end, e.start + 1) - 1, zone)}T${TaskRules.END_HOUR}"
        else Instant.ofEpochMilli(e.end).atZone(zone).toLocalDateTime().format(moment)

    private enum class Pick { TASK, EVENT }
    private class Res(val pick: Pick, val conflict: Boolean)

    /** Only the event changed: the event's value. Both changed and they disagree: the task's, and a conflict. */
    private fun resolve(taskChanged: Boolean, eventChanged: Boolean, same: Boolean): Res = when {
        eventChanged && !taskChanged -> Res(Pick.EVENT, false)
        eventChanged && taskChanged && !same -> Res(Pick.TASK, true)
        else -> Res(Pick.TASK, false)
    }

    private fun sameEvent(e: CalEvent, w: CalWant) =
        e.title == w.title && e.allDay == w.allDay && e.start == w.start && e.end == w.end

    /**
     * Compares the tasks, the events of the chosen calendar and what the last sync left ([snapshot])
     * and says what to do: write the calendar, change tasks, or nothing. Applying the plan and calling
     * this again gives an empty plan.
     */
    fun reconcile(tasks: List<CalTask>, events: List<CalEvent>, snapshot: CalSnapshot, zone: ZoneId): CalPlan {
        val byUid = LinkedHashMap<String, CalTask>()
        for (t in tasks) if (t.uid.isNotBlank()) byUid[t.uid] = t
        // Ids can be reused after the calendar was wiped: a remembered event id that now carries another
        // marker is not ours any more, and the marker always decides.
        val markerById = events.associate { it.id to keyOf(it.description) }
        val remembered = snapshot.marks.filter { (k, m) -> markerById[m.eventId].let { it == null || it == k } }
        val owned = remembered.values.map { it.eventId }.toSet()
        val idToKey = remembered.entries.associate { it.value.eventId to it.key }
        val groups = LinkedHashMap<CalKey, MutableList<CalEvent>>()
        for (e in events.sortedBy { it.id }) {
            val k = markerById[e.id] ?: idToKey[e.id] ?: continue
            groups.getOrPut(k) { mutableListOf() }.add(e)
        }
        val keys = (byUid.keys.flatMap { u -> CalKind.values().map { CalKey(u, it) } } + groups.keys + remembered.keys)
            .distinct().sortedWith(compareBy({ it.uid }, { it.kind.ordinal }))

        val create = ArrayList<CalWant>()
        val update = ArrayList<Pair<Long, CalWant>>()
        val delete = ArrayList<Long>()
        val changes = ArrayList<CalTaskChange>()
        val warnings = ArrayList<String>()
        val marks = LinkedHashMap<CalKey, CalMark>()
        // A title taken from one event is the task's title for the other event of the same task.
        val pendingTitle = HashMap<String, String>()

        for (key in keys) {
            val task = byUid[key.uid]
            // An event of a task this phone does not know (and never made) belongs to someone else.
            if (task == null && key !in remembered) continue
            val evs = groups[key].orEmpty()
            val old = remembered[key]
            val keep = evs.firstOrNull { it.id == old?.eventId } ?: evs.firstOrNull()
            // Only what this phone made is ever deleted: a copy it does not hold belongs to someone else.
            for (e in evs) if (e !== keep && e.id in owned) delete += e.id

            val effective = task?.let { it.copy(title = pendingTitle[it.uid] ?: it.title) }
            val want = effective?.let { wantOf(it, key.kind, zone) }
            if (effective == null || want == null) {
                keep?.let { if (it.id in owned) delete += it.id }
                continue
            }
            val cur: CalTask = effective
            if (keep == null) {
                create += want
                marks[key] = CalMark.of(0, want)
                continue
            }
            if (!keep.readable) {
                // No usable end: it is neither read as an edit nor rewritten.
                old?.let { marks[key] = it }
                continue
            }
            val safe = withMarker(keep, want)
            val markerMissing = !hasMarker(keep, key)
            val snap = old?.takeIf { it.eventId == keep.id }
            if (snap == null) {
                // Found by its marker with no memory of it (a lost file, another phone): the task wins.
                if (!sameEvent(keep, want) || want.reminders.isNotEmpty() || markerMissing) update += keep.id to safe
                marks[key] = CalMark.of(keep.id, want)
                continue
            }

            var newTitle: String? = null
            var newDue: String? = null
            var newStart: String? = null
            var newEnd: String? = null
            var conflict = false
            var invalid = false

            val titleRes = resolve(want.title != snap.title, keep.title != snap.title, keep.title == want.title)
            conflict = conflict || titleRes.conflict
            if (titleRes.pick == Pick.EVENT) {
                val typed = if (key.kind == CalKind.DEADLINE) stripDeadlinePrefix(keep.title) else keep.title.trim()
                if (typed.isEmpty()) invalid = true else if (typed != cur.title) newTitle = typed
            }
            val shapeChanged = keep.allDay != snap.allDay
            if (key.kind == CalKind.DEADLINE) {
                val eventDue = eventDate(keep.allDay, keep.start, zone).toString()
                val r = resolve(
                    want.taskStart != snap.taskStart,
                    shapeChanged || keep.start != snap.start || keep.end != snap.end,
                    eventDue == cur.due
                )
                conflict = conflict || r.conflict
                if (r.pick == Pick.EVENT && eventDue != cur.due) newDue = eventDue
            } else {
                val rs = resolve(want.taskStart != snap.taskStart, shapeChanged || keep.start != snap.start, startMoment(keep, zone) == cur.start)
                val re = resolve(want.taskEnd != snap.taskEnd, shapeChanged || keep.end != snap.end, endMoment(keep, zone) == cur.end)
                conflict = conflict || rs.conflict || re.conflict
                if (rs.pick == Pick.EVENT && startMoment(keep, zone) != cur.start) newStart = startMoment(keep, zone)
                if (re.pick == Pick.EVENT && endMoment(keep, zone) != cur.end) newEnd = endMoment(keep, zone)
                if (newStart != null || newEnd != null) {
                    val s = parseMoment(newStart ?: cur.start)
                    val e = parseMoment(newEnd ?: cur.end)
                    if (s == null || e == null || !e.isAfter(s)) invalid = true
                }
            }

            val applied = newTitle != null || newDue != null || newStart != null || newEnd != null
            var target = want
            if (applied || invalid) {
                if (invalid || !cur.editable) {
                    // The phone's editor would not accept this: the event goes back to what the task says.
                    update += keep.id to safe
                    marks[key] = CalMark.of(keep.id, want)
                    warnings += "A mudança feita na agenda em \"${cur.title}\" não pôde ser aplicada à tarefa e foi desfeita."
                    continue
                }
                newTitle?.let { pendingTitle[cur.uid] = it }
                val after = cur.copy(
                    title = newTitle ?: cur.title, due = newDue ?: cur.due,
                    start = newStart ?: cur.start, end = newEnd ?: cur.end
                )
                target = wantOf(after, key.kind, zone) ?: want
                changes += CalTaskChange(key, keep.id, newTitle, newDue, newStart, newEnd, safe)
            }
            if (conflict) warnings += "A mudança feita na agenda em \"${cur.title}\" foi desfeita porque a tarefa também mudou."
            if (!sameEvent(keep, target) || snap.reminders != target.reminders || markerMissing) {
                update += keep.id to withMarker(keep, target)
            }
            marks[key] = CalMark.of(keep.id, target)
        }
        return CalPlan(create, update, delete, changes, warnings.distinct(), marks)
    }
}

/** A calendar of the device the person can write to. */
internal data class CalInfo(
    val id: Long,
    val name: String,
    val account: String,
    /** "com.google", "LOCAL"… */
    val accountType: String = "com.google",
    val owner: String = "",
    /** The calendar's colour as ARGB, or 0. */
    val color: Int = 0,
    val syncEvents: Boolean = true,
    val visible: Boolean = true
) {
    val local: Boolean get() = accountType.equals("LOCAL", ignoreCase = true)

    /** Needs VISIBLE and SYNC_EVENTS turned on before its events reach Google. */
    val needsEnabling: Boolean get() = !local && (!syncEvents || !visible)

    /** The muted second line of its row; empty when there is nothing to say. */
    val note: String
        get() = when {
            local -> "só neste celular"
            needsEnabling -> "não sincronizada — será ligada ao escolher"
            else -> ""
        }

    /** The header it is listed under. */
    val group: String get() = if (local) "Só neste celular" else account
}

/** Which calendars are offered and how the list is laid out (pure, so it is tested). */
internal object CalendarChoices {
    private val hiddenOwners = listOf("#holiday@group.v.calendar.google.com", "#contacts@group.v.calendar.google.com")

    /** Birthdays and holidays of Google are not offered. */
    fun offered(list: List<CalInfo>): List<CalInfo> = list.filterNot { c ->
        c.accountType.equals("com.google", ignoreCase = true) && hiddenOwners.any { c.owner.endsWith(it) }
    }

    /** By account (Google ones first, the local ones last), each group by name. */
    fun grouped(list: List<CalInfo>): List<Pair<String, List<CalInfo>>> =
        offered(list).groupBy { it.group }.map { (g, items) -> g to items.sortedBy { it.name.lowercase() } }
            .sortedWith(compareBy({ it.second.first().local }, { it.first.lowercase() }))
}

/** What the engine needs from the calendar of the device. Throws SecurityException when the permission is gone. */
internal interface CalendarGateway {
    /** Every event of the calendar that is not deleted. */
    fun events(calendarId: Long): List<CalEvent>
    fun insert(calendarId: Long, want: CalWant): Long
    fun update(eventId: Long, want: CalWant)
    fun delete(eventId: Long)
}

/** What the engine needs from the tasks (the phone's core). */
internal interface CalTasks {
    fun list(): List<CalTask>

    /** Saves the given task fields (the keys the editor sends: title, due_date, scheduled_start, scheduled_end). Throws if refused. */
    fun save(id: Long, fields: Map<String, String>)
}

internal interface CalStore {
    fun load(): CalSnapshot?

    /** Null forgets everything. */
    fun save(snapshot: CalSnapshot?)
}

internal data class CalResult(val tasksChanged: Boolean, val warnings: List<String>)

internal class CalendarEngine(
    private val gateway: CalendarGateway,
    private val tasks: CalTasks,
    private val store: CalStore,
    private val zone: () -> ZoneId = { ZoneId.systemDefault() }
) {
    /** One reconcile against [calendarId]. SecurityException is not caught: the caller turns the sync off. */
    fun run(calendarId: Long): CalResult {
        var snapshot = store.load()
        if (snapshot != null && snapshot.calendarId != calendarId) {
            // The person chose another calendar: the events of the old one are removed, new ones are made.
            removeFrom(snapshot.calendarId, snapshot)
            snapshot = null
        }
        var current = snapshot ?: CalSnapshot(calendarId, emptyMap())
        var changed = false
        val warnings = LinkedHashSet<String>()
        for (pass in 1..2) {
            val list = tasks.list()
            // An empty list with events remembered looks like a swapped library, not like tasks all gone: touch nothing.
            if (list.isEmpty() && current.marks.isNotEmpty()) break
            val plan = CalendarRules.reconcile(list, gateway.events(calendarId), current, zone())
            warnings += plan.warnings
            current = apply(calendarId, plan, current, list, warnings)
            store.save(current)
            if (plan.taskChanges.isEmpty()) break
            changed = true
        }
        return CalResult(changed, warnings.toList())
    }

    /** Takes the events of the Ponte out of [calendarId] (the sync was turned off); forgets the snapshot. */
    fun removeEvents(calendarId: Long) {
        val stored = store.load()
        removeFrom(calendarId, stored?.takeIf { it.calendarId == calendarId })
        store.save(null)
    }

    private fun removeFrom(calendarId: Long, snapshot: CalSnapshot?) {
        val ids = snapshot?.marks?.values?.map { it.eventId }?.toSet().orEmpty()
        val events = try { gateway.events(calendarId) } catch (e: SecurityException) { throw e } catch (_: Throwable) { return }
        for (e in events) {
            // Only this phone's own events: copies made by another phone on the same calendar stay.
            if (e.id in ids) quietly { gateway.delete(e.id) }
        }
    }

    private inline fun quietly(block: () -> Unit): Boolean = try {
        block(); true
    } catch (e: SecurityException) {
        throw e
    } catch (_: Throwable) {
        false
    }

    private fun apply(
        calendarId: Long, plan: CalPlan, before: CalSnapshot, list: List<CalTask>, warnings: MutableSet<String>
    ): CalSnapshot {
        val marks = LinkedHashMap(plan.marks)
        val updates = ArrayList(plan.update)
        val byUid = list.associateBy { it.uid }

        // 1. What the calendar asked of the tasks, one save per task.
        for ((uid, group) in plan.taskChanges.groupBy { it.key.uid }) {
            val task = byUid[uid] ?: continue
            val fields = LinkedHashMap<String, String>()
            for (c in group) {
                c.title?.let { fields["title"] = it }
                c.due?.let { fields["due_date"] = it }
                c.start?.let { fields["scheduled_start"] = it }
                c.end?.let { fields["scheduled_end"] = it }
            }
            val ok = quietly { tasks.save(task.id, fields) }
            if (!ok) {
                warnings += "Não foi possível aplicar na tarefa \"${task.title}\" a mudança feita na agenda. Ela foi desfeita."
                for (c in group) {
                    updates.removeAll { it.first == c.eventId }
                    updates += c.eventId to c.revert
                    marks[c.key] = CalMark.of(c.eventId, c.revert)
                }
            }
        }
        // 2. The calendar.
        for (id in plan.delete) quietly { gateway.delete(id) }
        for (w in plan.create) {
            var id = 0L
            if (quietly { id = gateway.insert(calendarId, w) } && id > 0) marks[w.key] = CalMark.of(id, w) else marks.remove(w.key)
        }
        for ((id, w) in updates) {
            if (!quietly { gateway.update(id, w) }) {
                // Retried next time: keep what was remembered (or nothing, for an event that was only adopted).
                val key = w.key
                val old = before.marks[key]
                if (old != null) marks[key] = old else marks.remove(key)
            }
        }
        return CalSnapshot(calendarId, marks)
    }
}
