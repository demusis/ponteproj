package io.github.demusis.ponte

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import java.util.concurrent.atomic.AtomicBoolean

internal object CalendarSchedule {
    const val INTERVAL_MS = 60 * 60 * 1000L
    fun shouldSchedule(on: Boolean, permitted: Boolean) = on && permitted
}

/** One inexact alarm, rearmed on each tick and after boot. */
internal object CalendarBackground {
    const val ACTION = "io.github.demusis.ponte.CALENDAR_TICK"
    private fun pending(ctx: Context) = PendingIntent.getBroadcast(
        ctx, 0, Intent(ctx, CalendarReceiver::class.java).setAction(ACTION),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )

    fun arm(ctx: Context) {
        try {
            val app = ctx.applicationContext
            val alarms = app.getSystemService(AlarmManager::class.java) ?: return
            val alarm = pending(app)
            if (CalendarSchedule.shouldSchedule(CalendarSyncRunner.isOn(app), DeviceCalendar.hasPermission(app))) {
                alarms.set(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + CalendarSchedule.INTERVAL_MS, alarm)
            } else {
                alarms.cancel(alarm)
            }
        } catch (_: Throwable) { }
    }
}

class CalendarReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != CalendarBackground.ACTION) return
        val app = context.applicationContext
        CalendarBackground.arm(app)
        TodayWidget.refresh(app)
        if (!CalendarSchedule.shouldSchedule(CalendarSyncRunner.isOn(app), DeviceCalendar.hasPermission(app))) return
        val pending = goAsync()
        val finished = AtomicBoolean(false)
        val finish = { if (finished.compareAndSet(false, true)) pending.finish() }
        Handler(Looper.getMainLooper()).postDelayed({ finish() }, 8_000L)
        if (!CalendarSyncRunner.enqueue(app, cold = true) { finish() }) finish()
    }
}
