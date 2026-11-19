package io.github.demusis.ponte

import android.Manifest
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.provider.CalendarContract
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.ZoneId

/** The CalendarContract side of the sync: a thin adapter, all the decisions are in [CalendarRules]. */
internal class DeviceCalendar(private val ctx: Context) : CalendarGateway {
    private val resolver get() = ctx.contentResolver

    /**
     * Every calendar that accepts new events (contributor level or better), visible or not: a calendar just
     * created on Google often arrives hidden and not synced until it is switched on.
     */
    fun calendars(): List<CalInfo> {
        requirePermission()
        val out = ArrayList<CalInfo>()
        try {
            resolver.query(
                CalendarContract.Calendars.CONTENT_URI,
                arrayOf(
                    CalendarContract.Calendars._ID, CalendarContract.Calendars.CALENDAR_DISPLAY_NAME,
                    CalendarContract.Calendars.ACCOUNT_NAME, CalendarContract.Calendars.ACCOUNT_TYPE,
                    CalendarContract.Calendars.OWNER_ACCOUNT, CalendarContract.Calendars.CALENDAR_COLOR,
                    CalendarContract.Calendars.SYNC_EVENTS, CalendarContract.Calendars.VISIBLE
                ),
                "${CalendarContract.Calendars.CALENDAR_ACCESS_LEVEL} >= ?",
                arrayOf(CalendarContract.Calendars.CAL_ACCESS_CONTRIBUTOR.toString()),
                "${CalendarContract.Calendars.ACCOUNT_NAME}, ${CalendarContract.Calendars.CALENDAR_DISPLAY_NAME}"
            )?.use { c ->
                while (c.moveToNext()) {
                    out += CalInfo(
                        c.getLong(0), c.getString(1).orEmpty(), c.getString(2).orEmpty(), c.getString(3).orEmpty(),
                        c.getString(4).orEmpty(), c.getInt(5), c.getInt(6) == 1, c.getInt(7) == 1
                    )
                }
            }
        } catch (e: SecurityException) {
            throw e
        }
        return CalendarChoices.offered(out)
    }

    /** Turns on VISIBLE and SYNC_EVENTS of a calendar; false when the phone refuses. */
    fun enable(calendarId: Long): Boolean {
        requirePermission()
        return try {
            val v = ContentValues().apply {
                put(CalendarContract.Calendars.VISIBLE, 1)
                put(CalendarContract.Calendars.SYNC_EVENTS, 1)
            }
            resolver.update(ContentUris.withAppendedId(CalendarContract.Calendars.CONTENT_URI, calendarId), v, null, null) > 0
        } catch (e: SecurityException) {
            throw e
        } catch (_: Throwable) {
            false
        }
    }

    /** Asks Android to pull the Google calendars now. Never throws: a refusal only means the list stays as it is. */
    fun requestGoogleSync() {
        try {
            val accounts = LinkedHashSet<Pair<String, String>>()
            resolver.query(
                CalendarContract.Calendars.CONTENT_URI,
                arrayOf(CalendarContract.Calendars.ACCOUNT_NAME, CalendarContract.Calendars.ACCOUNT_TYPE),
                "${CalendarContract.Calendars.ACCOUNT_TYPE} = ?", arrayOf("com.google"), null
            )?.use { c -> while (c.moveToNext()) accounts += c.getString(0).orEmpty() to c.getString(1).orEmpty() }
            val extras = android.os.Bundle().apply {
                putBoolean(android.content.ContentResolver.SYNC_EXTRAS_MANUAL, true)
                putBoolean(android.content.ContentResolver.SYNC_EXTRAS_EXPEDITED, true)
            }
            for ((name, type) in accounts) {
                try {
                    android.content.ContentResolver.requestSync(android.accounts.Account(name, type), CalendarContract.AUTHORITY, extras)
                } catch (_: Throwable) {
                }
            }
        } catch (_: Throwable) {
        }
    }

    /** The calendar still exists and accepts events, hidden or not. */
    fun canWrite(calendarId: Long): Boolean {
        requirePermission()
        try {
            resolver.query(
                CalendarContract.Calendars.CONTENT_URI, arrayOf(CalendarContract.Calendars._ID),
                "${CalendarContract.Calendars._ID} = ? AND ${CalendarContract.Calendars.CALENDAR_ACCESS_LEVEL} >= ?",
                arrayOf(calendarId.toString(), CalendarContract.Calendars.CAL_ACCESS_CONTRIBUTOR.toString()), null
            )?.use { return it.moveToFirst() }
        } catch (e: SecurityException) {
            throw e
        }
        return false
    }

    override fun events(calendarId: Long): List<CalEvent> {
        requirePermission()
        val out = ArrayList<CalEvent>()
        try {
            resolver.query(
                CalendarContract.Events.CONTENT_URI,
                arrayOf(
                    CalendarContract.Events._ID, CalendarContract.Events.TITLE, CalendarContract.Events.DESCRIPTION,
                    CalendarContract.Events.ALL_DAY, CalendarContract.Events.DTSTART, CalendarContract.Events.DTEND,
                    CalendarContract.Events.RRULE, CalendarContract.Events.DURATION
                ),
                "${CalendarContract.Events.CALENDAR_ID} = ? AND ${CalendarContract.Events.DELETED} = 0",
                arrayOf(calendarId.toString()), null
            )?.use { c ->
                while (c.moveToNext()) {
                    // A repeating event is the person's own; the Ponte never makes one.
                    if (!c.getString(6).isNullOrEmpty()) continue
                    val start = c.getLong(4)
                    // The end is DTEND, or the start plus DURATION; with neither the event is kept but never read as an edit.
                    val end = if (!c.isNull(5)) c.getLong(5) else CalendarRules.parseDuration(c.getString(7))?.let { start + it }
                    out += CalEvent(
                        c.getLong(0), c.getString(1).orEmpty(), c.getString(2).orEmpty(), c.getInt(3) == 1,
                        start, end ?: start, readable = end != null
                    )
                }
            }
        } catch (e: SecurityException) {
            throw e
        }
        return out
    }

    private fun values(w: CalWant) = ContentValues().apply {
        put(CalendarContract.Events.TITLE, w.title)
        // Null on an update: the person's own text stays.
        w.description?.let { put(CalendarContract.Events.DESCRIPTION, it) }
        put(CalendarContract.Events.ALL_DAY, if (w.allDay) 1 else 0)
        put(CalendarContract.Events.DTSTART, w.start)
        put(CalendarContract.Events.DTEND, w.end)
        // An all-day event is kept in UTC, as the calendar app does.
        put(CalendarContract.Events.EVENT_TIMEZONE, if (w.allDay) "UTC" else ZoneId.systemDefault().id)
        put(CalendarContract.Events.HAS_ALARM, if (w.reminders.isEmpty()) 0 else 1)
    }

    override fun insert(calendarId: Long, want: CalWant): Long {
        requirePermission()
        val v = values(want).apply { put(CalendarContract.Events.CALENDAR_ID, calendarId) }
        val uri = try {
            resolver.insert(CalendarContract.Events.CONTENT_URI, v)
        } catch (e: SecurityException) {
            throw e
        } ?: throw IllegalStateException("insert")
        val id = ContentUris.parseId(uri)
        setReminders(id, want.reminders)
        return id
    }

    override fun update(eventId: Long, want: CalWant) {
        requirePermission()
        try {
            val n = resolver.update(ContentUris.withAppendedId(CalendarContract.Events.CONTENT_URI, eventId), values(want), null, null)
            if (n <= 0) throw IllegalStateException("update")
        } catch (e: SecurityException) {
            throw e
        }
        setReminders(eventId, want.reminders)
    }

    override fun delete(eventId: Long) {
        requirePermission()
        try {
            resolver.delete(ContentUris.withAppendedId(CalendarContract.Events.CONTENT_URI, eventId), null, null)
        } catch (e: SecurityException) {
            throw e
        }
    }

    /** Replaces the reminders of an event (only the planned ones have any). */
    private fun setReminders(eventId: Long, minutes: List<Int>) {
        try {
            resolver.delete(
                CalendarContract.Reminders.CONTENT_URI, "${CalendarContract.Reminders.EVENT_ID} = ?", arrayOf(eventId.toString())
            )
            for (m in minutes) {
                resolver.insert(CalendarContract.Reminders.CONTENT_URI, ContentValues().apply {
                    put(CalendarContract.Reminders.EVENT_ID, eventId)
                    put(CalendarContract.Reminders.MINUTES, m)
                    put(CalendarContract.Reminders.METHOD, CalendarContract.Reminders.METHOD_ALERT)
                })
            }
        } catch (e: SecurityException) {
            throw e
        }
    }

    private fun requirePermission() {
        if (!hasPermission(ctx)) throw SecurityException("calendar permission")
    }

    companion object {
        fun hasPermission(ctx: Context) =
            ctx.checkSelfPermission(Manifest.permission.READ_CALENDAR) == PackageManager.PERMISSION_GRANTED &&
                ctx.checkSelfPermission(Manifest.permission.WRITE_CALENDAR) == PackageManager.PERMISSION_GRANTED
    }
}

/** The snapshot as a JSON file in the app's private folder. */
internal class JsonCalStore(private val file: File) : CalStore {
    override fun load(): CalSnapshot? = try {
        val o = JSONObject(file.readText())
        val arr = o.getJSONArray("marks")
        val marks = LinkedHashMap<CalKey, CalMark>()
        for (i in 0 until arr.length()) {
            val m = arr.getJSONObject(i)
            val kind = CalKind.values().firstOrNull { it.key == m.getString("kind") } ?: continue
            val rem = m.optJSONArray("reminders")
            marks[CalKey(m.getString("uid"), kind)] = CalMark(
                m.getLong("eventId"), m.getString("title"), m.getBoolean("allDay"), m.getLong("start"), m.getLong("end"),
                if (rem == null) emptyList() else (0 until rem.length()).map { rem.getInt(it) },
                m.optString("taskStart"), m.optString("taskEnd")
            )
        }
        CalSnapshot(o.getLong("calendarId"), marks)
    } catch (_: Throwable) {
        null
    }

    override fun save(snapshot: CalSnapshot?) {
        if (snapshot == null) {
            file.delete()
            return
        }
        val arr = JSONArray()
        for ((k, m) in snapshot.marks) {
            arr.put(
                JSONObject().put("uid", k.uid).put("kind", k.kind.key).put("eventId", m.eventId).put("title", m.title)
                    .put("allDay", m.allDay).put("start", m.start).put("end", m.end)
                    .put("reminders", JSONArray(m.reminders)).put("taskStart", m.taskStart).put("taskEnd", m.taskEnd)
            )
        }
        val part = File(file.parentFile, file.name + ".parte")
        part.writeText(JSONObject().put("calendarId", snapshot.calendarId).put("marks", arr).toString())
        if (!part.renameTo(file)) {
            file.delete()
            part.renameTo(file)
        }
    }
}

/** The tasks of the core, as the calendar sync reads and changes them. Call from the SyncRunner worker only. */
internal object CoreTasks : CalTasks {
    override fun list(): List<CalTask> = Api.tasks().map { it.toTask() }.filter { it.uid.isNotBlank() }.map {
        CalTask(
            uid = it.uid, id = it.id, title = it.title, done = it.done, due = it.dueDate,
            start = it.scheduledStart, end = it.scheduledEnd, reminders = it.reminderOffsetsMinutes, docRef = it.documentNumber
        )
    }

    /**
     * Same path as the task editor: the version is the one just read, and a task that is closed or gone
     * is refused (the core also refuses an empty title or a bad date).
     */
    override fun save(id: Long, fields: Map<String, String>) {
        val t = Api.task(id)
        check(!t.done) { "closed" }
        Api.saveTask(id, t.version, fields)
        SyncRunner.refreshDocs()
    }
}

/** The preferences, the permission and the worker side of the calendar sync. */
internal object CalendarSyncRunner {
    const val K_ON = "cal_on"
    const val K_ID = "cal_id"
    const val K_LAST = "cal_last"
    const val K_WARN = "cal_warn"
    const val FILE = "agenda-sync.json"

    const val GONE = "A agenda escolhida não existe mais. A sincronização com a agenda foi desligada; escolha outra em Ajustes."
    const val NO_PERMISSION =
        "O Ponte de bolso não tem mais permissão para usar a agenda. A sincronização com a agenda foi desligada."
    private const val FAILED = "Não foi possível conferir a agenda agora. O Ponte tenta de novo em seguida."

    /**
     * [turnedOff] is the reason the sync switched itself off; [notify] are warnings born in this run.
     */
    class Outcome(val tasksChanged: Boolean, val notify: List<String>, val turnedOff: String?)

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("ponte", Context.MODE_PRIVATE)
    private fun store(ctx: Context) = JsonCalStore(File(ctx.filesDir, FILE))

    fun isOn(ctx: Context) = prefs(ctx).getBoolean(K_ON, false)
    fun chosen(ctx: Context) = prefs(ctx).getLong(K_ID, -1L)

    /** Asks for a fresh copy of the Google calendars, waits a few seconds, then reads the list again. */
    fun refresh(ctx: Context): List<CalInfo> {
        if (!DeviceCalendar.hasPermission(ctx)) return emptyList()
        DeviceCalendar(ctx).requestGoogleSync()
        try { Thread.sleep(REFRESH_WAIT_MS) } catch (_: InterruptedException) { }
        return calendars(ctx)
    }

    /** Switches the chosen calendar on (visible, syncing); false when the phone refuses. */
    fun enable(ctx: Context, id: Long): Boolean = try {
        DeviceCalendar(ctx).enable(id)
    } catch (_: Throwable) {
        false
    }

    private const val REFRESH_WAIT_MS = 4000L

    /** The writable calendars; an empty list without the permission. */
    fun calendars(ctx: Context): List<CalInfo> = try {
        DeviceCalendar(ctx).calendars()
    } catch (_: SecurityException) {
        emptyList()
    }

    /** One reconcile; null when there is nothing to do. Only on the SyncRunner worker. */
    fun run(ctx: Context): Outcome? {
        val p = prefs(ctx)
        if (!p.getBoolean(K_ON, false) || !SyncRunner.ready) return null
        val id = p.getLong(K_ID, -1L)
        if (id < 0) return null
        return try {
            if (!DeviceCalendar.hasPermission(ctx)) return turnOff(ctx, NO_PERMISSION)
            val device = DeviceCalendar(ctx)
            if (!device.canWrite(id)) return turnOff(ctx, GONE)
            val r = CalendarEngine(device, CoreTasks, store(ctx)).run(id)
            val e = p.edit().putLong(K_LAST, System.currentTimeMillis())
            // A warning stays until a newer one replaces it, so it can be read after the screen moved on.
            if (r.warnings.isNotEmpty()) e.putString(K_WARN, r.warnings.joinToString("\n"))
            e.apply()
            Outcome(r.tasksChanged, r.warnings, null)
        } catch (_: SecurityException) {
            turnOff(ctx, NO_PERMISSION)
        } catch (_: Throwable) {
            p.edit().putString(K_WARN, FAILED).apply()
            Outcome(false, emptyList(), null)
        }
    }

    private fun turnOff(ctx: Context, reason: String): Outcome {
        // The memory of the events stays: turning it on again finds them where they were.
        prefs(ctx).edit().putBoolean(K_ON, false).putString(K_WARN, reason).apply()
        return Outcome(false, emptyList(), reason)
    }

    /** Turned off by the person: [remove] takes this phone's events out of the calendar (and forgets them). */
    fun finishOff(ctx: Context, remove: Boolean) {
        val id = chosen(ctx)
        try {
            if (remove && id >= 0 && DeviceCalendar.hasPermission(ctx)) CalendarEngine(DeviceCalendar(ctx), CoreTasks, store(ctx)).removeEvents(id)
        } catch (_: Throwable) {
        }
        // "Deixar na agenda" keeps the memory too: turning it on again finds the events where they were.
    }

    /** The phone's library was wiped or swapped: the events it remembered are no longer its own. */
    fun forget(ctx: Context) {
        store(ctx).save(null)
        prefs(ctx).edit().putBoolean(K_ON, false).remove(K_WARN).apply()
    }
}
