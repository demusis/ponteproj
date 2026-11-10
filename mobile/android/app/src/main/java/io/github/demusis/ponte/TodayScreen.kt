package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
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
internal fun TodayScreen(state: TodayState, onToggle: (TaskItem) -> Unit, onOpen: (TaskItem) -> Unit) {
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
                val line = todaySummary(state.late.size, state.today.size, state.attendances.size)
                Text(
                    line, Modifier.padding(start = 4.dp, bottom = 4.dp),
                    fontSize = 17.sp, fontWeight = FontWeight.Medium,
                    color = if (state.late.isNotEmpty()) c.error else c.text
                )
            }
        }
        section("late", "Atrasadas", state.late, onToggle, onOpen, late = true)
        section("today", "Para hoje", state.today, onToggle, onOpen)
        section("upcoming", "Próximos dias", state.upcoming, onToggle, onOpen)
        if (state.attendances.isNotEmpty()) {
            item(key = "h-att") {
                SectionLabel("Atendimentos com prazo · ${state.attendances.size}", Modifier.padding(top = 8.dp))
            }
            items(state.attendances, key = { "a-${it.id}" }) { AttendanceCard(it) }
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

/** An atendimento: reference (or subject), its situation and the deadline. Read-only. */
@Composable
private fun AttendanceCard(a: AttendanceItem) {
    val c = Ponte.colors
    PonteCard {
        Row(Modifier.padding(start = 16.dp, top = 12.dp, end = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    a.reference, fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium, color = c.text,
                    maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                if (a.subject.isNotEmpty() && a.subject != a.reference) {
                    Text(a.subject, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
                if (a.statusLabel.isNotEmpty()) {
                    Text(a.statusLabel, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1)
                }
            }
            if (a.dueChip.isNotEmpty()) {
                Spacer(Modifier.width(8.dp))
                Pill(a.dueChip, if (a.late) Tone.ERROR else Tone.WARNING)
            }
        }
    }
}
