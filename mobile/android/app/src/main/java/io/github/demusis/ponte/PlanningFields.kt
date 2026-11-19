package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp

/**
 * The Planejamento fields shared by the new task and the task screens: Início and Fim (a date like
 * Prazo plus an optional hour), the warning over the due date, and the estimate in days. The hour and
 * days boxes are typed text kept by the caller ([startHour], [endHour], [daysText]); the moments and
 * the estimate are the values it stores, and every change arrives whole in [onChange].
 */
@Composable
internal fun PlanningFields(
    due: String, start: String, end: String, estimateMinutes: Int,
    startHour: String, endHour: String, daysText: String,
    onStartHour: (String) -> Unit, onEndHour: (String) -> Unit, onDaysText: (String) -> Unit,
    onChange: (start: String, end: String, estimateMinutes: Int) -> Unit
) {
    val c = Ponte.colors
    val startDate = TaskRules.splitMoment(start).first
    val endDate = TaskRules.splitMoment(end).first
    val startMoment = { date: String, hour: String -> TaskRules.composeMoment(date, hour, TaskRules.START_HOUR) }
    val endMoment = { date: String, hour: String -> TaskRules.composeMoment(date, hour, TaskRules.END_HOUR) }
    // The end is proposed from the start and the days, only while it is empty.
    val withAutoEnd = { s: String, days: Int ->
        val e = TaskRules.autoEnd(s, days, end)
        if (e != null) onEndHour(TaskRules.END_HOUR)
        e ?: end
    }
    val currentDays = TaskRules.parseDays(daysText) ?: 0
    SectionLabel("Planejamento")
    Text(TaskRules.PLANNING_HINT, style = MaterialTheme.typography.bodySmall, color = c.muted)
    PlanningMoment(
        "Início", "09:00", startDate, startHour,
        onDate = { d ->
            if (d.isEmpty()) onStartHour("")
            val m = startMoment(d, if (d.isEmpty()) "" else startHour) ?: startMoment(d, "") ?: ""
            onChange(m, withAutoEnd(m, currentDays), estimateMinutes)
        },
        onHour = { h ->
            onStartHour(h)
            startMoment(startDate, h)?.let { m -> onChange(m, withAutoEnd(m, currentDays), estimateMinutes) }
        },
        error = if (startDate.isNotEmpty() && startMoment(startDate, startHour) == null) TaskRules.HOUR_INVALID else ""
    )
    PlanningMoment(
        "Fim", "18:00", endDate, endHour,
        onDate = { d ->
            if (d.isEmpty()) onEndHour("")
            val m = endMoment(d, if (d.isEmpty()) "" else endHour) ?: endMoment(d, "") ?: ""
            onChange(start, m, estimateMinutes)
        },
        onHour = { h ->
            onEndHour(h)
            endMoment(endDate, h)?.let { m -> onChange(start, m, estimateMinutes) }
        },
        error = if (endDate.isNotEmpty() && endMoment(endDate, endHour) == null) TaskRules.HOUR_INVALID else ""
    )
    TaskRules.overDueWarning(due, end).takeIf { it.isNotEmpty() }?.let {
        Text(it, style = MaterialTheme.typography.bodyMedium, color = c.warning)
    }
    LabeledField(
        "Estimativa em dias (0 = sem estimativa)", daysText,
        { v ->
            TaskRules.parseDays(v)?.let { n ->
                onDaysText(v)
                onChange(start, withAutoEnd(start, n), n * TaskRules.MINUTES_PER_DAY)
            }
        },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number)
    )
}

/** True while an hour box holds text that is not an hour of the day (a date is set, so the hour counts). */
internal fun planningHourInvalid(start: String, end: String, startHour: String, endHour: String): Boolean {
    val s = TaskRules.splitMoment(start).first
    val e = TaskRules.splitMoment(end).first
    return (s.isNotEmpty() && TaskRules.composeMoment(s, startHour, TaskRules.START_HOUR) == null) ||
        (e.isNotEmpty() && TaskRules.composeMoment(e, endHour, TaskRules.END_HOUR) == null)
}

/** "Avisos antes do início planejado": the chips, and the permission line when the phone blocks them. */
@Composable
internal fun ReminderChips(
    offsets: List<Int>, onChange: (List<Int>) -> Unit, remindersBlocked: Boolean, onRemindersChosen: () -> Unit
) {
    Text("Avisos antes do início planejado")
    TaskRules.reminderOptions(offsets).forEach { (minutes, label) ->
        FilterChip(
            selected = minutes in offsets,
            onClick = {
                val after = if (minutes in offsets) offsets - minutes else offsets + minutes
                onChange(after)
                // Without the permission the notices stay mute: ask now, as Ajustes does.
                if (TaskRules.offsetAdded(offsets, after)) onRemindersChosen()
            },
            label = { Text(label) }
        )
    }
    if (TaskRules.remindersBlockedShown(offsets, remindersBlocked)) {
        Notice(TaskRules.REMINDERS_BLOCKED, tone = Tone.WARNING)
    }
}

/** A planned moment like Prazo (date chooser) plus a small optional hour box; [error] shows under them. */
@Composable
private fun PlanningMoment(
    label: String, hourHint: String, date: String, hour: String,
    onDate: (String) -> Unit, onHour: (String) -> Unit, error: String
) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
            DateField(label, date, onDate, Modifier.weight(1f), emptyText = "Sem data")
            LabeledField(
                "Hora", hour, onHour, Modifier.width(112.dp), placeholder = hourHint,
                error = "", keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text)
            )
        }
        if (error.isNotEmpty()) Text(error, style = MaterialTheme.typography.bodySmall, color = Ponte.colors.error)
    }
}
