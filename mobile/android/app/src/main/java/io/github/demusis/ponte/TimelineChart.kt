package io.github.demusis.ponte

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Timeline
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.LocalDate

private const val LABEL_SHARE = 0.4f
private val LABEL_MAX = 160.dp

/** The colours of the chart, taken from the theme once. */
private class ChartColors(
    val plan: Color, val done: Color, val late: Color, val weekend: Color, val line: Color,
    val today: Color, val target: Color, val tick: Color, val surface: Color, val mark: Color
)

@Composable
private fun chartColors(): ChartColors {
    val c = Ponte.colors
    return ChartColors(
        plan = c.accent, done = c.muted, late = c.error, weekend = c.chip.copy(alpha = if (c.dark) 0.7f else 0.9f),
        line = c.divider, today = c.warning, target = c.text, tick = c.muted, surface = c.surface, mark = c.text
    )
}

/**
 * The timeline of a project: one row per task that has a planned period, titles in a fixed column on the left
 * (40% of the width, at most 160 dp) and the time on the right, which scrolls sideways. Tasks with no period
 * are listed under "Sem período planejado". A tap on a row opens the task.
 */
@Composable
internal fun TimelineSection(detail: ProjectDetail, today: LocalDate, onOpenTask: (Long) -> Unit, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    val unplanned = ProjectRules.unplanned(detail.timeline)
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val labelWidth = minOf(maxWidth * LABEL_SHARE, LABEL_MAX)
            val timeWidth = maxWidth - labelWidth
            val layout = remember(detail, today, timeWidth) {
                ProjectRules.layout(detail.timeline, detail.span, today, detail.project.targetDate, timeWidth.value)
            }
            if (layout == null) {
                EmptyState(
                    "Nada para mostrar na linha do tempo.",
                    text = if (detail.timeline.isEmpty()) "Este projeto ainda não tem tarefas."
                    else "Nenhuma tarefa tem início e fim planejados. Defina o período na tarefa.",
                    icon = Icons.Outlined.Timeline
                )
            } else {
                Chart(layout, labelWidth, today, onOpenTask)
            }
        }
        if (unplanned.isNotEmpty()) {
            SectionLabel("Sem período planejado · ${unplanned.size}")
            for (item in unplanned) UnplannedCard(item) { onOpenTask(item.id) }
        }
        if (ProjectRules.layout(detail.timeline, detail.span, today, detail.project.targetDate, 100f) != null) Legend()
    }
}

@Composable
private fun Chart(layout: TimelineLayout, labelWidth: androidx.compose.ui.unit.Dp, today: LocalDate, onOpenTask: (Long) -> Unit) {
    val c = Ponte.colors
    val colors = chartColors()
    val density = LocalDensity.current
    val measurer = rememberTextMeasurer()
    val tickStyle = TextStyle(fontSize = 11.sp, color = colors.tick)
    val scroll = rememberScrollState(
        // Starts with today (or the first row) in view.
        with(density) { ((layout.todayX ?: layout.bars.firstOrNull()?.x ?: 0f) - 40f).coerceAtLeast(0f).dp.roundToPx() }
    )
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(c.surface)) {
        Column(Modifier.width(labelWidth)) {
            Spacer(Modifier.height(ProjectRules.HEADER_HEIGHT.dp))
            for (item in layout.rows) RowLabel(item) { onOpenTask(item.id) }
        }
        Row(Modifier.weight(1f).horizontalScroll(scroll)) {
            Canvas(
                Modifier.width(layout.width.dp).height((ProjectRules.HEADER_HEIGHT + ProjectRules.ROW_HEIGHT * layout.rows.size).dp)
                    // The labels carry the words for a screen reader; the picture adds nothing to them.
                    .clearAndSetSemantics { }
                    .pointerInput(layout) {
                        detectTapGestures { at ->
                            val row = ProjectRules.rowAt(at.y / density.density, layout.rows.size)
                            if (row != null) onOpenTask(layout.rows[row].id)
                        }
                    }
            ) { drawTimeline(layout, colors, measurer, tickStyle) }
        }
    }
}

/** The title and the period of a row, in the fixed column. */
@Composable
private fun RowLabel(item: TimelineItem, onClick: () -> Unit) {
    val c = Ponte.colors
    Column(
        Modifier.fillMaxWidth().height(ProjectRules.ROW_HEIGHT.dp)
            .drawBehind { drawLine(c.divider, Offset(0f, size.height), Offset(size.width, size.height), 1f) }
            .clickable(onClick = onClick)
            .semantics(mergeDescendants = true) { contentDescription = ProjectRules.rowDescription(item) }
            .padding(start = 12.dp, end = 8.dp),
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            item.title, fontSize = 13.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium,
            color = if (item.done) c.muted else c.text, textDecoration = if (item.done) TextDecoration.LineThrough else null,
            maxLines = 1, overflow = TextOverflow.Ellipsis
        )
        Text(
            ProjectRules.periodLabel(item), fontSize = 11.sp, lineHeight = 14.sp,
            color = if (item.late && !item.done) c.error else c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis
        )
    }
}

@Composable
private fun UnplannedCard(item: TimelineItem, onClick: () -> Unit) {
    val c = Ponte.colors
    PonteCard(onClick = onClick) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 10.dp)) {
            Text(
                item.title, fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium,
                color = if (item.done) c.muted else c.text, textDecoration = if (item.done) TextDecoration.LineThrough else null,
                maxLines = 2, overflow = TextOverflow.Ellipsis
            )
            val line = ProjectRules.unplannedLine(item)
            if (line.isNotEmpty()) Text(line, style = MaterialTheme.typography.bodySmall, color = c.muted)
        }
    }
}

// ---------------------------------------------------------------- drawing

/** Draws the whole chart: weekends, grid, header, bars, due marks, the target line and today. */
private fun DrawScope.drawTimeline(layout: TimelineLayout, k: ChartColors, measurer: TextMeasurer, tickStyle: TextStyle) {
    val d = density
    val headerH = ProjectRules.HEADER_HEIGHT * d
    val rowH = ProjectRules.ROW_HEIGHT * d
    val barH = ProjectRules.BAR_HEIGHT * d
    val rows = layout.rows.size
    val totalH = headerH + rowH * rows
    for ((from, to) in layout.weekends) drawRect(k.weekend, Offset(from * d, 0f), Size((to - from) * d, totalH))
    // Grid: one hair per tick, and a line under the header and under every row.
    for (t in layout.ticks) {
        drawLine(k.line, Offset(t.x * d, headerH * 0.5f), Offset(t.x * d, totalH), d)
        drawText(measurer, t.label, Offset(t.x * d + 3 * d, 5 * d), tickStyle)
    }
    for (i in 0..rows) drawLine(k.line, Offset(0f, headerH + rowH * i), Offset(size.width, headerH + rowH * i), d)
    // Bars: solid for a fixed plan, light with an outline for a projected one.
    for (b in layout.bars) {
        val color = when (b.kind) {
            BarKind.PLAN -> k.plan
            BarKind.DONE -> k.done
            BarKind.LATE -> k.late
        }
        val top = headerH + rowH * b.row + (rowH - barH) / 2
        val corner = CornerRadius(4 * d)
        if (b.projected) {
            drawRoundRect(color.copy(alpha = 0.25f), Offset(b.x * d, top), Size(b.width * d, barH), corner)
            drawRoundRect(color, Offset(b.x * d + d, top + d), Size(b.width * d - 2 * d, barH - 2 * d), corner, style = Stroke(2 * d))
        } else {
            drawRoundRect(color, Offset(b.x * d, top), Size(b.width * d, barH), corner)
        }
    }
    // The due date of a row: a small diamond at the end of that day.
    for (m in layout.dues) {
        val cx = m.x * d
        val cy = headerH + rowH * m.row + rowH / 2
        val r = 6 * d
        val diamond = Path().apply {
            moveTo(cx, cy - r); lineTo(cx + r, cy); lineTo(cx, cy + r); lineTo(cx - r, cy); close()
        }
        drawPath(diamond, k.mark)
        drawPath(diamond, k.surface, style = Stroke(1.5f * d))
    }
    layout.targetX?.let { x ->
        drawLine(k.target, Offset(x * d, 0f), Offset(x * d, totalH), 2 * d, pathEffect = PathEffect.dashPathEffect(floatArrayOf(8 * d, 6 * d)))
    }
    layout.todayX?.let { x -> drawLine(k.today, Offset(x * d, 0f), Offset(x * d, totalH), 2 * d) }
}

// ---------------------------------------------------------------- legend

/** What the colours and lines mean. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Legend() {
    val k = chartColors()
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)
    ) {
        LegendItem("Plano fixo") { Box(Modifier.size(18.dp, 10.dp).background(k.plan, RoundedCornerShape(3.dp))) }
        LegendItem("Projetado") {
            Box(
                Modifier.size(18.dp, 10.dp).background(k.plan.copy(alpha = 0.25f), RoundedCornerShape(3.dp))
                    .border(2.dp, k.plan, RoundedCornerShape(3.dp))
            )
        }
        LegendItem("Concluída") { Box(Modifier.size(18.dp, 10.dp).background(k.done, RoundedCornerShape(3.dp))) }
        LegendItem("Passa do prazo") { Box(Modifier.size(18.dp, 10.dp).background(k.late, RoundedCornerShape(3.dp))) }
        LegendItem("Prazo") { Box(Modifier.size(8.dp).rotate(45f).background(k.mark)) }
        LegendItem("Hoje") { Box(Modifier.size(2.dp, 12.dp).background(k.today)) }
        LegendItem("Data-alvo") {
            Canvas(Modifier.size(2.dp, 12.dp)) {
                drawLine(
                    k.target, Offset(size.width / 2, 0f), Offset(size.width / 2, size.height), size.width,
                    pathEffect = PathEffect.dashPathEffect(floatArrayOf(3 * density, 2 * density))
                )
            }
        }
    }
}

@Composable
private fun LegendItem(label: String, swatch: @Composable () -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Box(Modifier.clearAndSetSemantics { }, contentAlignment = Alignment.Center) { swatch() }
        Text(label, style = MaterialTheme.typography.bodySmall, fontSize = 12.sp, color = Ponte.colors.secondary, maxLines = 1)
    }
}
