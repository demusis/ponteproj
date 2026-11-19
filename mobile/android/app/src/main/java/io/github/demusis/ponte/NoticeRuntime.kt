package io.github.demusis.ponte

import android.Manifest
import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import org.json.JSONObject
import java.io.File
import java.time.LocalDate
import java.time.ZonedDateTime
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The daily deadline notice, the Android side: the switch and the time (kept in the app's
 * preferences), the alarm that wakes [NoticeReceiver], and the notification. What is decided
 * without Android (the next time, the wording) is in [DeadlineNotice].
 *
 * The alarm is a single inexact one (`setAndAllowWhileIdle`, which needs no special permission)
 * that is set again each time it rings, when the phone starts, when the app is updated, when the
 * clock or the time zone changes and when the app opens, so a lost alarm comes back by itself.
 */
internal object NoticeScheduler {
    const val ACTION_TICK = "io.github.demusis.ponte.NOTICE_TICK"

    /** The notification opens the app with this action, to land on Hoje. */
    const val ACTION_TODAY = "io.github.demusis.ponte.OPEN_TODAY"
    const val CHANNEL = "prazos"
    const val NOTIFICATION_ID = 1
    private const val K_ON = "notice_on"
    private const val K_TIME = "notice_min"

    /** The day (ISO date) the notice last fired: so opening the app after a late alarm never skips a day, and no day gets two. */
    private const val K_SHOWN = "notice_shown"

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("ponte", Context.MODE_PRIVATE)

    /** The switch: off until the person turns it on. */
    fun isOn(ctx: Context) = prefs(ctx).getBoolean(K_ON, false)

    fun time(ctx: Context): DeadlineNotice.Time =
        DeadlineNotice.Time.fromMinutes(prefs(ctx).getInt(K_TIME, DeadlineNotice.DEFAULT.minutes))

    fun setOn(ctx: Context, on: Boolean) {
        prefs(ctx).edit().putBoolean(K_ON, on).apply()
        if (on) skipTodayIfPast(ctx)
    }

    fun setTime(ctx: Context, time: DeadlineNotice.Time) {
        prefs(ctx).edit().putInt(K_TIME, time.minutes).apply()
        skipTodayIfPast(ctx)
    }

    private fun shownOn(ctx: Context): LocalDate? = try {
        prefs(ctx).getString(K_SHOWN, null)?.let { LocalDate.parse(it) }
    } catch (_: Throwable) {
        null
    }

    /** The notice fired (or is being shown) today: the next one is tomorrow's. */
    fun markShown(ctx: Context, day: LocalDate = LocalDate.now()) {
        prefs(ctx).edit().putString(K_SHOWN, day.toString()).apply()
    }

    /** Turning the notice on, or moving its time, after that time today means "from tomorrow", not "now". */
    private fun skipTodayIfPast(ctx: Context) {
        val now = ZonedDateTime.now()
        val t = time(ctx)
        if (!now.toLocalTime().isBefore(java.time.LocalTime.of(t.hour, t.minute))) markShown(ctx, now.toLocalDate())
    }

    /** The app may show notifications: the permission (Android 13 or newer) and the phone's own switch. */
    fun allowed(ctx: Context): Boolean {
        if (Build.VERSION.SDK_INT >= 33 &&
            ctx.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) return false
        return try {
            ctx.getSystemService(NotificationManager::class.java)?.areNotificationsEnabled() ?: false
        } catch (_: Throwable) {
            false
        }
    }

    private fun tick(ctx: Context): PendingIntent = PendingIntent.getBroadcast(
        ctx, 1, Intent(ctx, NoticeReceiver::class.java).setAction(ACTION_TICK),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )

    /**
     * Sets every alarm again: the daily notice's and (in the background) the individual notices of the
     * tasks. This is the entry the app, the boot receiver and the clock changes already call. Never throws.
     */
    fun arm(ctx: Context) {
        armDaily(ctx)
        try {
            PlannedNotices.refreshAsync(ctx.applicationContext)
        } catch (_: Throwable) {
        }
    }

    /** Sets the daily notice's next alarm (replacing any other), or cancels it when the notice is off. Never throws. */
    fun armDaily(ctx: Context) {
        try {
            val app = ctx.applicationContext
            val alarms = app.getSystemService(AlarmManager::class.java) ?: return
            val pending = tick(app)
            if (!isOn(app)) {
                alarms.cancel(pending)
                return
            }
            val next = DeadlineNotice.nextAlarm(ZonedDateTime.now(), time(app), shownOn(app))
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, next.toInstant().toEpochMilli(), pending)
        } catch (_: Throwable) {
        }
    }

    /** What a tap on the notification starts: the app, on Hoje. */
    fun openToday(ctx: Context): PendingIntent = PendingIntent.getActivity(
        ctx, 2,
        Intent(ctx, MainActivity::class.java).setAction(ACTION_TODAY)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )

    /** Shows the notice. Only counts are in it; on a locked screen only [DeadlineNotice.PUBLIC_TEXT] shows. */
    fun show(ctx: Context, body: String) {
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        val channel = NotificationChannel(CHANNEL, "Prazos", NotificationManager.IMPORTANCE_DEFAULT).apply {
            description = "Aviso diário com o número de tarefas e atendimentos que vencem."
            lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            setShowBadge(false)
        }
        nm.createNotificationChannel(channel)
        val publicVersion = Notification.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_notice)
            .setContentTitle(DeadlineNotice.PUBLIC_TEXT)
            .build()
        val notice = Notification.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_notice)
            .setContentTitle(DeadlineNotice.TITLE)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setCategory(Notification.CATEGORY_REMINDER)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion)
            .setContentIntent(openToday(ctx))
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .build()
        nm.notify(NOTIFICATION_ID, notice)
    }
}

/**
 * The counts of Hoje, read for the notice without the sync password and without any screen.
 *
 * - The app's process already holds the library (it is open, or it just opened it): the same
 *   core answers. The core serialises its callers, so this waits for a sync that is running; the
 *   time limit of [NoticeJob] covers that wait.
 * - A cold process (the alarm woke the app alone): the library key is unwrapped by the Android
 *   Keystore (a key that needs no fingerprint), the library is opened exactly as the app opens
 *   it (`init` does nothing when it is already open, so the app starting a moment later does not
 *   collide with this) and `today` answers. Nothing is created: a phone that never had a library
 *   shows no notice. Nothing here touches [Controller], so the app stays locked.
 *
 * Any failure answers null: the notice is skipped, never shown with an error.
 */
internal object NoticeCounts {
    /** Tasks late, tasks due today, atendimentos with a deadline, resource reservations today and overdue returns: the counts of Hoje. */
    data class Counts(val notice: String)

    /** The library is open (it was, or it was opened now exactly as the notice does); false when it cannot be, without creating anything. */
    fun openCore(ctx: Context): Boolean {
        if (SyncRunner.keyBroken) return false
        if (!IntakeRules.noticeMayRead(
                SyncRunner.ready, SyncRunner.keyBroken, File(ctx.filesDir, "acervo.sqlite3").isFile,
                SyncRunner.libraryKeyStored(ctx)
            )
        ) return false
        if (!SyncRunner.ready) {
            val key = SyncRunner.libraryKeyIfPresent(ctx) ?: return false
            val opened = JSONObject(SyncRunner.core.init(ctx.filesDir.absolutePath, key, phoneMemberName()))
            if (!opened.optBoolean("ok")) return false
        }
        return true
    }

    /**
     * The tasks as the individual notices need them (uid, title, start, due date, offsets, done), or null when the
     * library cannot be read. A task without uid is left out: only the uid is a lasting identity.
     */
    fun readTasks(ctx: Context): List<TaskReminders.Task>? {
        if (!openCore(ctx)) return null
        val answer = JSONObject(SyncRunner.core.listTasks())
        if (!answer.optBoolean("ok")) return null
        return answer.optJSONArray("tasks").objs().mapNotNull { t ->
            val uid = t.s("uid")
            if (!TaskReminders.validUid(uid)) return@mapNotNull null
            val offsets = t.optJSONArray("reminder_offsets_minutes")?.let { a -> (0 until a.length()).map { a.optInt(it, -1) } } ?: emptyList()
            TaskReminders.Task(
                uid = uid, title = t.s("title"), scheduledStart = t.s("scheduled_start"), dueDate = t.s("due_date"),
                offsets = offsets, done = t.optBoolean("done")
            )
        }
    }

    fun read(ctx: Context): Counts? {
        if (!openCore(ctx)) return null
        val today = JSONObject(SyncRunner.core.today())
        if (!today.optBoolean("ok")) return null
        return Counts(today.optString("notice"))
    }
}

/** Reads the counts off the main thread, within a strict time, and shows the notice if something is due. */
internal object NoticeJob {
    /** The receiver has to answer well within what Android allows a broadcast. */
    const val LIMIT_MS = 8_000L

    fun run(ctx: Context, done: () -> Unit) {
        val finished = AtomicBoolean(false)
        val tooLate = AtomicBoolean(false)
        val finish = { if (finished.compareAndSet(false, true)) done() }
        Handler(Looper.getMainLooper()).postDelayed({ tooLate.set(true); finish() }, LIMIT_MS)
        Thread {
            try {
                val counts = NoticeCounts.read(ctx)
                if (counts != null && !tooLate.get() && NoticeScheduler.allowed(ctx)) {
                    // The core words the notice; empty when nothing is due.
                    counts.notice.takeIf { it.isNotEmpty() }?.let {
                        if (!tooLate.get()) NoticeScheduler.show(ctx, it)
                    }
                }
            } catch (_: Throwable) {
                // No notice, no error shown, nothing to retry: tomorrow's alarm is already set.
            } finally {
                finish()
            }
        }.start()
    }
}

/** The alarm rang: set tomorrow's first, then show today's notice if something is due. */
class NoticeReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != NoticeScheduler.ACTION_TICK) return
        val app = context.applicationContext
        // Today's notice is counted as shown the moment the alarm rings (so the next alarm is tomorrow's
        // and a day never gets two), and tomorrow's alarm is set before anything that could fail.
        NoticeScheduler.markShown(app)
        NoticeScheduler.armDaily(app)
        if (!NoticeScheduler.isOn(app)) return
        val pending = goAsync()
        NoticeJob.run(app) { try { pending.finish() } catch (_: Throwable) { } }
    }
}

/** The phone started, the app was updated or the clock changed: the alarm is set again. */
class NoticeBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        when (intent?.action) {
            Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED,
            Intent.ACTION_TIME_CHANGED, Intent.ACTION_TIMEZONE_CHANGED -> {
                val app = context.applicationContext
                NoticeScheduler.armDaily(app)
                // The individual notices are worked out again from the library (the zone or the clock moved the moments).
                val pending = goAsync()
                PlannedNotices.refreshAsync(app) { try { pending.finish() } catch (_: Throwable) { } }
            }
        }
    }
}

/**
 * The individual notices of the tasks, the Android side ([TaskReminders] decides everything that needs no Android).
 *
 * - ONE alarm for the next moment (`setAndAllowWhileIdle`, inexact: no exact-alarm permission), independent of the daily
 *   notice's alarm (another request code, action and receiver). When it rings, the tasks are read again, so a task
 *   concluded, deleted or moved meanwhile is never told; then the next moment is armed.
 * - The ledger of what was told lives in the app's preferences (keys and numbers, no titles) and is saved before the
 *   notification is shown, so a restart, a sync or a clock change never repeats a notice.
 * - Nothing here touches [Controller]: the app stays locked. When the app lock is in force the notification does not
 *   even carry the task's title; on a locked screen only [TaskReminders.PUBLIC_TEXT] shows either way.
 * - The entry points for the rest of the app: [NoticeScheduler.arm] (opening the app, boot, clock and zone changes
 *   already call it) and [refreshAsync], to be called after a sync and after a task is saved or concluded.
 */
internal object PlannedNotices {
    const val ACTION_TICK = "io.github.demusis.ponte.PLANNED_TICK"
    const val ACTION_SNOOZE = "io.github.demusis.ponte.PLANNED_SNOOZE"
    const val EXTRA_KEY = "key"
    const val CHANNEL = "avisos_tarefas"
    const val SUMMARY_ID = 2
    private const val ALARM_CODE = 3
    private const val K_ON = "planned_on"
    private const val K_LEDGER = "planned_ledger"
    private const val K_NEXT = "planned_next"

    /** Same limit as [NoticeJob]: the receiver must answer within what Android allows a broadcast. */
    private const val LIMIT_MS = 8_000L

    /** Only the ledger is guarded by this (never held while the library is read, so the main thread is never blocked for long). */
    private val ledgerLock = Any()

    /** One refresh at a time, in the order asked. */
    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "ponte-planned-notices").apply { isDaemon = true } }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("ponte", Context.MODE_PRIVATE)

    /** On by default: a task only announces itself when the person gave it offsets. */
    fun isOn(ctx: Context) = prefs(ctx).getBoolean(K_ON, true)

    fun setOn(ctx: Context, on: Boolean) {
        prefs(ctx).edit().putBoolean(K_ON, on).apply()
        refreshAsync(ctx.applicationContext)
    }

    private fun tickIntent(ctx: Context): PendingIntent = PendingIntent.getBroadcast(
        ctx, ALARM_CODE, Intent(ctx, PlannedNoticeReceiver::class.java).setAction(ACTION_TICK),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )

    private fun cancelAlarm(ctx: Context) {
        try {
            ctx.getSystemService(AlarmManager::class.java)?.cancel(tickIntent(ctx))
        } catch (_: Throwable) {
        }
        prefs(ctx).edit().putLong(K_NEXT, 0L).apply()
    }

    private fun armAt(ctx: Context, at: ZonedDateTime?) {
        try {
            if (at == null) return cancelAlarm(ctx)
            val alarms = ctx.getSystemService(AlarmManager::class.java) ?: return
            val ms = maxOf(at.toInstant().toEpochMilli(), System.currentTimeMillis() + 1_000L)
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, ms, tickIntent(ctx))
            prefs(ctx).edit().putLong(K_NEXT, ms).apply()
        } catch (_: Throwable) {
        }
    }

    /** The app lock is in force (the same rule as [Controller]: its switch, on by default with a secure screen lock). */
    private fun lockInForce(ctx: Context): Boolean = try {
        val p = prefs(ctx)
        val secure = AppLock.deviceSecure(ctx)
        val enabled = if (p.contains("lock_on")) p.getBoolean("lock_on", true) else LockPolicy.defaultEnabled(secure)
        LockPolicy.active(enabled, secure)
    } catch (_: Throwable) {
        true
    }

    /**
     * Works out, on a background thread, what is due now and when the next moment is, then arms it. [done] is called
     * when finished or after [LIMIT_MS] at most (for a receiver's `goAsync`). Never throws.
     */
    fun refreshAsync(ctx: Context, done: () -> Unit = {}) {
        val app = ctx.applicationContext
        val finished = AtomicBoolean(false)
        val tooLate = AtomicBoolean(false)
        val finish = { if (finished.compareAndSet(false, true)) done() }
        try {
            Handler(Looper.getMainLooper()).postDelayed({ tooLate.set(true); finish() }, LIMIT_MS)
            worker.execute {
                try {
                    refresh(app, tooLate)
                } catch (_: Throwable) {
                    // Nothing shown, nothing to retry here: the next arm (app open, sync, boot) tries again.
                } finally {
                    finish()
                }
            }
        } catch (_: Throwable) {
            finish()
        }
    }

    private fun refresh(app: Context, tooLate: AtomicBoolean) {
        if (!isOn(app) || !NoticeScheduler.allowed(app)) return cancelAlarm(app)
        val tasks = NoticeCounts.readTasks(app)
        if (tasks == null) {
            // Library locked away, busy or absent: keep what is armed; if the alarm just rang, look again soon.
            val retry = TaskReminders.retryAt(prefs(app).getLong(K_NEXT, 0L), ZonedDateTime.now())
            if (retry != null) armAt(app, retry)
            return
        }
        if (tooLate.get()) return
        val now = ZonedDateTime.now()
        val commit: TaskReminders.Commit
        val next: ZonedDateTime?
        synchronized(ledgerLock) {
            val ledger = TaskReminders.Ledger.decode(prefs(app).getString(K_LEDGER, null))
            val plan = TaskReminders.plan(tasks, ledger, now)
            commit = TaskReminders.commit(tasks, ledger, plan, now)
            // Saved BEFORE anything is shown: a crash in between can only lose a notice, never repeat one.
            @Suppress("ApplySharedPref") // Persist before posting to avoid duplicate notices after process death.
            prefs(app).edit().putString(K_LEDGER, commit.ledger.encode()).commit()
            next = TaskReminders.plan(tasks, commit.ledger, now).next
        }
        val nm = app.getSystemService(NotificationManager::class.java)
        commit.cancel.forEach { nm?.cancel(it) }
        if (commit.shown.isNotEmpty() || commit.folded > 0) {
            val byUid = tasks.associateBy { it.uid }
            val lock = lockInForce(app)
            commit.shown.forEach { show(app, it, byUid[it.occurrence.uid]?.title, lock, now.toLocalDate()) }
            if (commit.folded > 0) showFolded(app, commit.folded)
        }
        armAt(app, next)
    }

    private fun ensureChannel(nm: NotificationManager) {
        val channel = NotificationChannel(CHANNEL, "Avisos de tarefas", NotificationManager.IMPORTANCE_HIGH).apply {
            description = "Avisos de uma tarefa antes do início ou do prazo, nos tempos que você escolheu."
            lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            setShowBadge(false)
        }
        nm.createNotificationChannel(channel)
    }

    private fun publicVersion(ctx: Context) = Notification.Builder(ctx, CHANNEL)
        .setSmallIcon(R.drawable.ic_notice)
        .setContentTitle(TaskReminders.PUBLIC_TEXT)
        .build()

    private fun snoozeAction(ctx: Context, shown: TaskReminders.Shown): Notification.Action {
        val pending = PendingIntent.getBroadcast(
            ctx, shown.id,
            Intent(ctx, PlannedNoticeActionReceiver::class.java).setAction(ACTION_SNOOZE).putExtra(EXTRA_KEY, shown.occurrence.key),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val builder = Notification.Action.Builder(
            android.graphics.drawable.Icon.createWithResource(ctx, R.drawable.ic_notice), "Adiar 10 min", pending
        )
        // From Android 12 the action waits for the phone to be unlocked (it is a private notification anyway).
        if (Build.VERSION.SDK_INT >= 31) builder.setAuthenticationRequired(true)
        return builder.build()
    }

    private fun show(ctx: Context, shown: TaskReminders.Shown, taskTitle: String?, lock: Boolean, today: LocalDate) {
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        ensureChannel(nm)
        val line = TaskReminders.line(shown.occurrence, today)
        val notice = Notification.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_notice)
            .setContentTitle(TaskReminders.title(taskTitle, lock))
            .setContentText(line)
            .setStyle(Notification.BigTextStyle().bigText(line))
            .setCategory(Notification.CATEGORY_REMINDER)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion(ctx))
            .setContentIntent(NoticeScheduler.openToday(ctx))
            .addAction(snoozeAction(ctx, shown))
            .setAutoCancel(true)
            .build()
        nm.notify(shown.id, notice)
    }

    private fun showFolded(ctx: Context, count: Int) {
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        ensureChannel(nm)
        val notice = Notification.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_notice)
            .setContentTitle(TaskReminders.SUMMARY_TITLE)
            .setContentText(TaskReminders.foldedLine(count))
            .setCategory(Notification.CATEGORY_REMINDER)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion(ctx))
            .setContentIntent(NoticeScheduler.openToday(ctx))
            .setAutoCancel(true)
            .build()
        nm.notify(SUMMARY_ID, notice)
    }

    /** "Adiar 10 min": the notice comes back later (the ledger says when) and the shown one goes away. */
    fun snooze(ctx: Context, key: String, done: () -> Unit) {
        val app = ctx.applicationContext
        var id = 0
        synchronized(ledgerLock) {
            val ledger = TaskReminders.Ledger.decode(prefs(app).getString(K_LEDGER, null))
            id = ledger.entries[key]?.id ?: 0
            val snoozed = TaskReminders.snooze(ledger, key, ZonedDateTime.now())
            @Suppress("ApplySharedPref") // Persist snooze before dismissing the notification.
            if (snoozed != null) prefs(app).edit().putString(K_LEDGER, snoozed.encode()).commit()
        }
        if (id > 0) app.getSystemService(NotificationManager::class.java)?.cancel(id)
        refreshAsync(app, done)
    }
}

/** The individual-notice alarm rang: everything is worked out again from the library. */
class PlannedNoticeReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != PlannedNotices.ACTION_TICK) return
        val pending = goAsync()
        PlannedNotices.refreshAsync(context.applicationContext) { try { pending.finish() } catch (_: Throwable) { } }
    }
}

/** The "Adiar 10 min" button of a notice (not exported: only the app's own notification can send it). */
class PlannedNoticeActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != PlannedNotices.ACTION_SNOOZE) return
        val key = intent.getStringExtra(PlannedNotices.EXTRA_KEY) ?: return
        if (TaskReminders.uidOfKey(key) == null) return
        val pending = goAsync()
        PlannedNotices.snooze(context.applicationContext, key) { try { pending.finish() } catch (_: Throwable) { } }
    }
}
