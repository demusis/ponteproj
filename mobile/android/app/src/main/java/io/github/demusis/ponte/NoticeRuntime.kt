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

    /** Sets the next alarm (replacing any other), or cancels it when the notice is off. Never throws. */
    fun arm(ctx: Context) {
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
    /** Tasks late, tasks due today, atendimentos with a deadline: the counts of Hoje. */
    data class Counts(val late: Int, val today: Int, val attendances: Int)

    fun read(ctx: Context): Counts? {
        if (SyncRunner.keyBroken) return null
        if (!IntakeRules.noticeMayRead(
                SyncRunner.ready, SyncRunner.keyBroken, File(ctx.filesDir, "acervo.sqlite3").isFile,
                SyncRunner.libraryKeyStored(ctx)
            )
        ) return null
        if (!SyncRunner.ready) {
            val key = SyncRunner.libraryKeyIfPresent(ctx) ?: return null
            val opened = JSONObject(SyncRunner.core.init(ctx.filesDir.absolutePath, key, phoneMemberName()))
            if (!opened.optBoolean("ok")) return null
        }
        val today = JSONObject(SyncRunner.core.today())
        if (!today.optBoolean("ok")) return null
        return Counts(
            late = today.optJSONArray("late")?.length() ?: 0,
            today = today.optJSONArray("today")?.length() ?: 0,
            attendances = today.optJSONArray("attendances")?.length() ?: 0
        )
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
                    DeadlineNotice.body(counts.late, counts.today, counts.attendances)?.let {
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
        NoticeScheduler.arm(app)
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
            Intent.ACTION_TIME_CHANGED, Intent.ACTION_TIMEZONE_CHANGED -> NoticeScheduler.arm(context.applicationContext)
        }
    }
}
