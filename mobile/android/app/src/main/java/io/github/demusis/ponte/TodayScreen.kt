package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.material3.TextButton
import androidx.compose.material3.IconButton
import androidx.compose.material3.Icon
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowLeft
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The Hoje area (inside MainShell): what needs attention now. Tasks late, due today and in the
 * next days (the same rows as Tarefas), then the atendimentos whose deadline has come.
 */
@Composable
internal fun TodayScreen(
    state: TodayState,
    onToggle: (TaskItem) -> Unit,
    onOpen: (TaskItem) -> Unit,
    onOpenAttendance: (AttendanceItem) -> Unit = {},
    onOpenResources: () -> Unit = {},
    /** The week (see [WeekPlan.days]): planned tasks and resource uses by day, deadlines apart. Empty: no section. */
    week: List<WeekDay> = emptyList(),
    /** The Monday of [week]; null hides the arrows. */
    weekStart: java.time.LocalDate? = null,
    /** -1 earlier, +1 later, 0 this week. */
    onMoveWeek: (Int) -> Unit = {},
    onOpenWeekEntry: (WeekEntry) -> Unit = {}
) {
    val c = Ponte.colors
    if (state.loading && state.isEmpty) {
        LoadingBox("Carregando…", Modifier.fillMaxSize())
        return
    }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, top = 8.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        state.error?.let { item(key = "error") { Notice(it) } }
        if (state.isEmpty) {
            item(key = "empty") {
                EmptyState(
                    "Nada vence hoje.",
                    text = "Quando uma tarefa ou um atendimento chegar ao prazo, aparece aqui.",
                    icon = Icons.Outlined.CheckCircle
                )
            }
        } else {
            item(key = "summary") {
                val line = state.summary
                Text(
                    line, Modifier.padding(start = 4.dp, bottom = 4.dp),
                    fontSize = 17.sp, fontWeight = FontWeight.Medium,
                    color = if (state.late.isNotEmpty() || state.overdueReturns > 0) c.error else c.text
                )
            }
        }
        section("late", "Atrasadas", state.late, onToggle, onOpen, late = true)
        section("today", "Para hoje", state.today, onToggle, onOpen)
        section("upcoming", "Próximos dias", state.upcoming, onToggle, onOpen)
        weekSection(week, weekStart, onMoveWeek, onOpenWeekEntry)
        if (state.attendances.isNotEmpty()) {
            item(key = "h-att") {
                SectionLabel("Atendimentos com prazo · ${state.attendances.size}", Modifier.padding(top = 8.dp))
            }
            items(state.attendances, key = { "a-${it.id}" }) { a -> AttendanceCard(a, onClick = { onOpenAttendance(a) }) }
        }
        if (state.reservationsToday > 0 || state.overdueReturns > 0) {
            item(key = "h-res") { SectionLabel(PhoneWords.RES_AREA, Modifier.padding(top = 8.dp)) }
            if (state.overdueReturns > 0) {
                item(key = "res-overdue") { ResourceCountCard(PhoneWords.RES_OVERDUE_LINE, state.overdueReturns, Tone.ERROR, onOpenResources) }
            }
            if (state.reservationsToday > 0) {
                item(key = "res-today") { ResourceCountCard(PhoneWords.RES_TODAY_LINE, state.reservationsToday, Tone.ACCENT, onOpenResources) }
            }
        }
    }
}

private fun androidx.compose.foundation.lazy.LazyListScope.section(
    key: String,
    title: String,
    list: List<TaskItem>,
    onToggle: (TaskItem) -> Unit,
    onOpen: (TaskItem) -> Unit,
    late: Boolean = false
) {
    if (list.isEmpty()) return
    item(key = "h-$key") {
        SectionLabel("$title · ${list.size}", Modifier.padding(top = 8.dp), color = if (late) Ponte.colors.error else Ponte.colors.muted)
    }
    items(list, key = { "t-${it.id}" }) { t -> TaskCard(t, onToggle, onOpen) }
}

private val WEEKDAYS = listOf("seg", "ter", "qua", "qui", "sex", "sáb", "dom")

/** "seg 17/11". */
internal fun weekDayTitle(date: java.time.LocalDate) =
    "${WEEKDAYS[date.dayOfWeek.value - 1]} %02d/%02d".format(date.dayOfMonth, date.monthValue)

/**
 * The week of Hoje: a header that moves between weeks, then one card per day (an empty day says so,
 * to show where there is room). Hour blocks first, deadlines after; a tap on a line opens it.
 */
private fun androidx.compose.foundation.lazy.LazyListScope.weekSection(
    week: List<WeekDay>,
    weekStart: java.time.LocalDate?,
    onMove: (Int) -> Unit,
    onOpen: (WeekEntry) -> Unit
) {
    if (week.isEmpty()) return
    item(key = "h-week") {
        val c = Ponte.colors
        Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            SectionLabel("Semana", Modifier.weight(1f))
            if (weekStart != null) {
                val current = weekStart == WeekPlan.mondayOf(java.time.LocalDate.now())
                if (!current) {
                    TextButton({ onMove(0) }) { Text("Esta semana", fontSize = 14.sp, color = c.accent) }
                }
                IconButton({ onMove(-1) }) {
                    Icon(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, contentDescription = "Semana anterior", tint = c.text)
                }
                Text(WeekPlan.weekTitle(weekStart), fontSize = 13.sp, color = c.muted)
                IconButton({ onMove(1) }) {
                    Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, contentDescription = "Próxima semana", tint = c.text)
                }
            }
        }
    }
    items(week, key = { "w-${it.date}" }) { day ->
        val c = Ponte.colors
        val today = day.date == java.time.LocalDate.now()
        PonteCard {
            Column(Modifier.padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        weekDayTitle(day.date) + if (today) " · hoje" else "", Modifier.weight(1f),
                        fontSize = 15.sp, fontWeight = FontWeight.Medium, color = if (today) c.accent else c.text
                    )
                    if (day.isEmpty) Text("Livre", fontSize = 14.sp, color = c.muted)
                }
                day.blocks.forEach { b ->
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 40.dp).clickable { onOpen(b.entry) },
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(WeekPlan.label(b), Modifier.width(112.dp), fontSize = 14.sp, color = c.muted, maxLines = 1)
                        Column(Modifier.weight(1f)) {
                            Text(b.entry.title, fontSize = 15.sp, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            b.entry.dependencyNotice?.let { notice ->
                                Text(notice, fontSize = 13.sp, color = if (b.entry.dependencyWarning) c.warning else c.muted)
                            }
                        }
                    }
                }
                day.deadlines.forEach { d ->
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 40.dp).clickable { onOpen(d) },
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text("Prazo", Modifier.width(112.dp), fontSize = 14.sp, color = c.error, maxLines = 1)
                        Text(d.title, Modifier.weight(1f), fontSize = 15.sp, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }
    }
}

/** A line of Hoje about resources ("Reservas de hoje", 2): a count only, a tap opens the Recursos list. */
@Composable
internal fun ResourceCountCard(title: String, count: Int, tone: Tone, onClick: () -> Unit) {
    val c = Ponte.colors
    PonteCard(onClick = onClick) {
        Row(
            Modifier.heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(title, Modifier.weight(1f), fontSize = 16.sp, fontWeight = FontWeight.Medium, color = c.text, maxLines = 2)
            Spacer(Modifier.width(8.dp))
            Pill(count.toString(), tone)
        }
    }
}
