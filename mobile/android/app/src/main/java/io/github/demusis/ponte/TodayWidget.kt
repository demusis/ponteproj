package io.github.demusis.ponte

import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.RemoteViews
import org.json.JSONObject
import java.time.LocalDate
import java.util.concurrent.atomic.AtomicBoolean

class TodayWidget : AppWidgetProvider() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == ACTION_REFRESH || intent.action == Intent.ACTION_DATE_CHANGED ||
            intent.action == Intent.ACTION_TIME_CHANGED || intent.action == Intent.ACTION_TIMEZONE_CHANGED ||
            intent.action == AppWidgetManager.ACTION_APPWIDGET_UPDATE) {
            val app = context.applicationContext
            val manager = AppWidgetManager.getInstance(app)
            if (manager.getAppWidgetIds(ComponentName(app, TodayWidget::class.java)).isEmpty()) return
            val pending = goAsync()
            val finished = AtomicBoolean(false)
            val expired = AtomicBoolean(false)
            val finish = { if (finished.compareAndSet(false, true)) pending.finish() }
            Handler(Looper.getMainLooper()).postDelayed({ expired.set(true); finish() }, NoticeJob.LIMIT_MS)
            Thread {
                try {
                    val data = read(app)
                    if (!expired.get()) render(app, data)
                } catch (_: Throwable) {
                    if (!expired.get()) render(app, null)
                } finally { finish() }
            }.start()
            return
        }
        super.onReceive(context, intent)
    }

    companion object {
        private const val ACTION_REFRESH = "io.github.demusis.ponte.WIDGET_REFRESH"
        fun refresh(ctx: Context) {
            val app = ctx.applicationContext
            if (AppWidgetManager.getInstance(app).getAppWidgetIds(ComponentName(app, TodayWidget::class.java)).isEmpty()) return
            app.sendBroadcast(Intent(app, TodayWidget::class.java).setAction(ACTION_REFRESH))
        }

        private fun read(ctx: Context): TodayWidgetRules.Data? {
            val cold = !SyncRunner.ready
            if (!NoticeCounts.openCore(ctx)) return null
            if (cold) SyncRunner.adoptHolidays(ctx)
            val today = JSONObject(SyncRunner.core.today())
            val follow = JSONObject(SyncRunner.core.listFollowUps())
            val risks = JSONObject(SyncRunner.core.atRisk())
            if (!today.optBoolean("ok") || !follow.optBoolean("ok") || !risks.optBoolean("ok")) return null
            fun tasks(key: String) = today.optJSONArray(key).objs().map { TodayWidgetRules.Task(it.optString("title"), it.optString("due_date")) }
            return TodayWidgetRules.Data(tasks("late"), tasks("today"), follow.optInt("count"), risks.optJSONArray("items")?.length() ?: 0)
        }

        @Synchronized fun conceal(ctx: Context) {
            val app = ctx.applicationContext
            if (AppWidgetManager.getInstance(app).getAppWidgetIds(ComponentName(app, TodayWidget::class.java)).isNotEmpty()) render(app, null)
        }

        @Synchronized private fun render(ctx: Context, data: TodayWidgetRules.Data?) {
            val display = TodayWidgetRules.display(data, PlannedNotices.lockInForce(ctx))
            val views = RemoteViews(ctx.packageName, R.layout.today_widget)
            views.setTextViewText(R.id.widget_date, TodayWidgetRules.date(LocalDate.now()))
            views.setTextViewText(R.id.widget_counts, display.counts)
            val ids = intArrayOf(R.id.widget_line1, R.id.widget_line2, R.id.widget_line3, R.id.widget_line4, R.id.widget_more)
            ids.forEachIndexed { i, id ->
                views.setTextViewText(id, display.lines.getOrNull(i) ?: "")
                views.setViewVisibility(id, if (i < display.lines.size) View.VISIBLE else View.GONE)
            }
            views.setOnClickPendingIntent(R.id.widget_root, NoticeScheduler.openToday(ctx))
            AppWidgetManager.getInstance(ctx).updateAppWidget(ComponentName(ctx, TodayWidget::class.java), views)
        }
    }
}
