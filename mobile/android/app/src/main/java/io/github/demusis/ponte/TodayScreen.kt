package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import kotlinx.coroutines.launch
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
    onOpenWeekEntry: (WeekEntry) -> Unit = {},
    /** Deliveries at risk; empty: no card. A tap opens the cause ([onOpenTask] gets its id). */
    risks: List<RiskItem> = emptyList(),
    onOpenTask: (Long) -> Unit = {},
    followUpCount: Int = 0,
    onOpenFollowUps: () -> Unit = {}
) {
    val c = Ponte.colors
    if (state.loading && state.isEmpty) {
        LoadingBox(text = "Carregando…", modifier = Modifier.fillMaxSize())
        return
    }
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val currentWeek = weekStart != null && weekStart == WeekPlan.mondayOf(java.time.LocalDate.now())
    val weekDeadlines = if (currentWeek) week.sumOf { it.deadlines.size } else 0
    // Items before the "Atrasadas" header: error, summary, alert, week module.
    val lateIndex = (if (state.error != null) 1 else 0) + 1 + (if (state.late.isNotEmpty()) 1 else 0) + (if (risks.isNotEmpty()) 1 else 0) + (if (followUpCount > 0) 1 else 0) + (if (week.isNotEmpty()) 1 else 0)
    LazyColumn(
        Modifier.fillMaxSize(),
        state = listState,
        contentPadding = PaddingValues(top = 8.dp, bottom = 32.dp)
    ) {
        val pad = Modifier.padding(horizontal = ROW_MARGIN, vertical = 4.dp)
        state.error?.let { item(key = "error") { Notice(it, pad) } }
        item(key = "summary") {
            if (state.isEmpty) {
                val extra = if (weekDeadlines > 0) " · " + (if (weekDeadlines == 1) "1 prazo nesta semana" else "$weekDeadlines prazos nesta semana") else ""
                Text("Nada vence hoje.$extra", Modifier.padding(horizontal = 20.dp, vertical = 4.dp), fontSize = 15.sp, color = c.muted)
            } else {
                Text(
                    state.summary, Modifier.padding(horizontal = 20.dp, vertical = 4.dp),
                    fontSize = 17.sp, fontWeight = FontWeight.Medium,
                    color = if (state.late.isNotEmpty() || state.overdueReturns > 0) c.error else c.text
                )
            }
        }
        if (state.late.isNotEmpty()) item(key = "late-alert") {
            run {
                val n = state.late.size
                Row(
                    pad.fillMaxWidth().heightIn(min = 48.dp)
                        .clip(RoundedCornerShape(12.dp)).background(c.errorContainer)
                        .clickable { scope.launch { listState.animateScrollToItem(lateIndex) } }
                        .padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        "⚠ " + (if (n == 1) "1 atrasada" else "$n atrasadas"), Modifier.weight(1f),
                        fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.error
                    )
                    Text("ver", fontSize = 15.sp, color = c.error)
                }
            }
        }
        if (followUpCount > 0) item(key = "follow-ups") {
            ResourceCountCard("Cobranças de hoje", followUpCount, Tone.ACCENT, onOpenFollowUps)
        }
        if (risks.isNotEmpty()) item(key = "risks") { RiskCard(risks, onOpenTask) }
        if (week.isNotEmpty()) {
            item(key = "week") { WeekModule(week, weekStart, onMoveWeek, onOpenWeekEntry) }
        }
        section("late", "Atrasadas", state.late, onToggle, onOpen, late = true)
        section("today", "Para hoje", state.today, onToggle, onOpen)
        section("upcoming", "Próximos dias", state.upcoming, onToggle, onOpen)
        if (state.attendances.isNotEmpty()) {
            item(key = "h-att") {
                SectionLabel("Atendimentos com prazo · ${state.attendances.size}", Modifier.padding(start = 12.dp, top = 12.dp, bottom = 4.dp))
            }
            items(state.attendances, key = { "a-${it.id}" }) { a -> AttendanceRow(a, onClick = { onOpenAttendance(a) }) }
        }
        if (state.reservationsToday > 0 || state.overdueReturns > 0) {
            item(key = "h-res") { SectionLabel(PhoneWords.RES_AREA, Modifier.padding(start = 12.dp, top = 12.dp, bottom = 4.dp)) }
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
        SectionLabel("$title · ${list.size}", Modifier.padding(start = 12.dp, top = 12.dp, bottom = 4.dp), color = if (late) Ponte.colors.error else Ponte.colors.muted)
    }
    items(list, key = { "t-${it.id}" }) { t -> TaskListRow(t, { onToggle(t) }, { onOpen(t) }) }
}

/** "Entregas em risco · N": three rows, the rest behind "ver todas". Full-width rows, so 360dp never clips. */
@Composable
internal fun RiskCard(risks: List<RiskItem>, onOpen: (Long) -> Unit) {
    val c = Ponte.colors
    var all by rememberSaveable { mutableStateOf(false) }
    val shown = if (all) risks else risks.take(3)
    Column(Modifier.fillMaxWidth()) {
        SectionHeader("Entregas em risco · ${risks.size}", color = c.warning)
        shown.forEach { r ->
            FlatRow({ onOpen(RiskRules.openTarget(r)) }) {
                Column {
                    Text(r.title, fontSize = 15.sp, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(RiskRules.riskLine(r), fontSize = 13.sp, color = c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        if (risks.size > 3) {
            SectionAction(if (all) "ver menos" else "ver todas (${risks.size})", { all = !all })
        }
    }
}

private val WEEKDAYS = listOf("seg", "ter", "qua", "qui", "sex", "sáb", "dom")

/** "seg 17/11". */
internal fun weekDayTitle(date: java.time.LocalDate) =
    "${WEEKDAYS[date.dayOfWeek.value - 1]} %02d/%02d".format(date.dayOfMonth, date.monthValue)

/**
 * The week of Hoje: a header that moves between weeks, a strip of seven days (swipe to change week) and
 * the plan of the selected day. Everything fits 360dp: the period title is short and centered.
 */
@Composable
internal fun WeekModule(
    week: List<WeekDay>,
    weekStart: java.time.LocalDate?,
    onMove: (Int) -> Unit,
    onOpen: (WeekEntry) -> Unit
) {
    val c = Ponte.colors
    val today = java.time.LocalDate.now()
    val first = week.first().date
    var selectedText by rememberSaveable(first.toString()) { mutableStateOf(WeekPlan.defaultSelected(first, today).toString()) }
    val selected = week.firstOrNull { it.date.toString() == selectedText } ?: week.first()
    val current = weekStart != null && weekStart == WeekPlan.mondayOf(today)
    val threshold = with(LocalDensity.current) { 48.dp.toPx() }
    Column(Modifier.padding(horizontal = ROW_MARGIN), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (weekStart != null) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton({ onMove(-1) }) {
                    Icon(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, contentDescription = "Semana anterior", tint = c.text)
                }
                Text(
                    WeekPlan.shortWeekTitle(weekStart), Modifier.weight(1f), textAlign = TextAlign.Center,
                    fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text, maxLines = 1
                )
                IconButton({ onMove(1) }) {
                    Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, contentDescription = "Próxima semana", tint = c.text)
                }
            }
            if (!current) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    TextButton({ onMove(0) }) { Text("Hoje", fontSize = 14.sp, color = c.accent) }
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().pointerInput(weekStart) {
                var total = 0f
                detectHorizontalDragGestures(
                    onDragStart = { total = 0f },
                    onDragEnd = {
                        if (total <= -threshold) onMove(1) else if (total >= threshold) onMove(-1)
                        total = 0f
                    },
                    onDragCancel = { total = 0f }
                ) { _, delta -> total += delta }
            }
        ) {
            week.forEach { day ->
                DayCell(day, isToday = day.date == today, isSelected = day.date == selected.date, Modifier.weight(1f)) {
                    selectedText = day.date.toString()
                }
            }
        }
        Text(
            WeekPlan.longDayTitle(selected.date) + if (selected.date == today) " · hoje" else "",
            Modifier.padding(start = 4.dp, top = 4.dp), fontSize = 15.sp, fontWeight = FontWeight.Medium,
            color = if (selected.date == today) c.accent else c.text
        )
        if (selected.isEmpty) {
            Text("Dia livre.", Modifier.padding(start = 4.dp), fontSize = 14.sp, color = c.muted)
        } else {
            run {
                Column {
                    val rows = selected.blocks.size + selected.deadlines.size
                    var n = 0
                    selected.blocks.forEach { b ->
                        val e = b.entry
                        WeekRow(e.title, onClick = { onOpen(e) }) {
                            val line = WeekPlan.label(b) + (if (e.kind == WeekKind.USE) " · reserva" else "")
                            Text(line, fontSize = 13.sp, color = c.muted)
                            e.dependencyNotice?.let {
                                Text(it, fontSize = 13.sp, lineHeight = 18.sp, color = if (e.dependencyWarning) c.warning else c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            }
                        }
                        if (++n < rows) HorizontalDivider(color = c.divider)
                    }
                    selected.deadlines.forEach { d ->
                        WeekRow(d.title, onClick = { onOpen(d) }) {
                            Text("Prazo de entrega", fontSize = 13.sp, color = c.error)
                        }
                        if (++n < rows) HorizontalDivider(color = c.divider)
                    }
                }
            }
        }
    }
}

@Composable
private fun WeekRow(title: String, onClick: () -> Unit, second: @Composable () -> Unit) {
    val c = Ponte.colors
    Column(
        Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(onClick = onClick).padding(horizontal = 4.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.Center
    ) {
        Text(title, fontSize = 15.sp, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
        second()
    }
}

private val INITIALS = listOf("S", "T", "Q", "Q", "S", "S", "D")

@Composable
private fun DayCell(day: WeekDay, isToday: Boolean, isSelected: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val c = Ponte.colors
    val (hasBlocks, hasDeadlines) = WeekPlan.dayMarks(day)
    Column(
        modifier.heightIn(min = 56.dp).padding(horizontal = 2.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(if (isSelected) c.accentContainer else Color.Transparent)
            .clickable(onClick = onClick).padding(vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(INITIALS[day.date.dayOfWeek.value - 1], fontSize = 12.sp, color = c.muted)
        Text(
            day.date.dayOfMonth.toString(), fontSize = 16.sp,
            fontWeight = if (isToday) FontWeight.Bold else FontWeight.Normal,
            color = if (isToday) c.accent else c.text
        )
        Row(Modifier.height(8.dp).padding(top = 3.dp), horizontalArrangement = Arrangement.spacedBy(3.dp)) {
            if (hasBlocks) Box(Modifier.size(5.dp).clip(CircleShape).background(c.accent))
            if (hasDeadlines) Box(Modifier.size(5.dp).clip(CircleShape).background(c.error))
        }
    }
}

/** A line of Hoje about resources ("Reservas de hoje", 2): a count only, a tap opens the Recursos list. */
@Composable
internal fun ResourceCountCard(title: String, count: Int, tone: Tone, onClick: () -> Unit) {
    val c = Ponte.colors
    FlatRow(onClick) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(title, Modifier.weight(1f), fontSize = 16.sp, fontWeight = FontWeight.Medium, color = c.text, maxLines = 2)
            Spacer(Modifier.width(8.dp))
            Pill(count.toString(), tone = tone)
        }
    }
}
