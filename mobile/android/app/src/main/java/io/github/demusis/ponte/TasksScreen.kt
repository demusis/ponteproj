package io.github.demusis.ponte

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.unit.dp

private class TaskGroup(val key: String, val title: String, val tasks: List<TaskItem>, val late: Boolean = false)

/** The Tarefas area (inside MainShell): open tasks grouped by urgency, finished ones below. */
@Composable
internal fun TasksScreen(state: TasksState, onToggle: (TaskItem) -> Unit, onOpen: (TaskItem) -> Unit) {
    val c = Ponte.colors
    var showDone by rememberSaveable { mutableStateOf(false) }
    val groups = remember(state.tasks) {
        val open = state.tasks.filter { !it.done }
        val late = open.filter { it.overdue }
        val today = open.filter { !it.overdue && it.dueToday }
        val next = open.filter { !it.overdue && !it.dueToday && it.dueDate.isNotEmpty() }.sortedBy { it.dueDate }
        val none = open.filter { !it.overdue && !it.dueToday && it.dueDate.isEmpty() }
        listOf(
            TaskGroup("late", "Atrasadas", late, late = true),
            TaskGroup("today", "Para hoje", today),
            TaskGroup("next", "Próximas", next),
            TaskGroup("none", "Sem prazo", none)
        ).filter { it.tasks.isNotEmpty() }
    }
    val done = remember(state.tasks) { state.tasks.filter { it.done }.sortedByDescending { it.id }.take(30) }
    val noOpen = state.tasks.none { !it.done }

    if (state.loading && state.tasks.isEmpty()) {
        LoadingBox("Carregando as tarefas…", Modifier.fillMaxSize())
        return
    }

    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, top = 8.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        state.error?.let { item(key = "error") { Notice(it) } }
        if (noOpen) {
            item(key = "empty") {
                EmptyState(
                    "Nenhuma tarefa em aberto.",
                    text = "Quando surgir algo a fazer, aparece aqui. Aproveite o descanso.",
                    icon = Icons.Outlined.CheckCircle
                )
            }
        }
        for (g in groups) {
            item(key = "h-${g.key}") {
                SectionLabel(
                    "${g.title} · ${g.tasks.size}", Modifier.padding(top = 8.dp),
                    color = if (g.late) c.error else c.muted
                )
            }
            items(g.tasks, key = { "t-${it.id}" }) { t -> TaskCard(t, onToggle, onOpen) }
        }
        if (done.isNotEmpty()) {
            item(key = "h-done") {
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { showDone = !showDone },
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    SectionLabel("Concluídas · ${done.size}", Modifier.weight(1f))
                    Icon(
                        Icons.Outlined.ExpandMore,
                        contentDescription = if (showDone) "Recolher as concluídas" else "Mostrar as concluídas",
                        tint = c.muted, modifier = Modifier.padding(end = 4.dp).size(22.dp).rotate(if (showDone) 180f else 0f)
                    )
                }
            }
            if (showDone) items(done, key = { "d-${it.id}" }) { t -> TaskCard(t, onToggle, onOpen) }
        }
    }
}

@Composable
private fun TaskCard(t: TaskItem, onToggle: (TaskItem) -> Unit, onOpen: (TaskItem) -> Unit) {
    PonteCard {
        TaskRow(
            t, listOf(t.actionLabel, t.documentNumber).filter { it.isNotEmpty() }.joinToString(" · "),
            onToggle = { onToggle(t) }, onOpen = { onOpen(t) },
            modifier = Modifier.padding(start = 6.dp, end = 12.dp, top = 6.dp, bottom = 6.dp)
        )
    }
}
