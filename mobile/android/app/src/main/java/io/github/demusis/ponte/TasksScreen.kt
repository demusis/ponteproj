package io.github.demusis.ponte

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FloatingActionButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
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
internal fun TasksScreen(
    state: TasksState,
    onToggle: (TaskItem) -> Unit,
    onOpen: (TaskItem) -> Unit,
    onNew: () -> Unit = {},
    categoryFilter: Set<String> = emptySet(),
    onCategoryFilter: (Set<String>) -> Unit = {}
) {
    val c = Ponte.colors
    var showDone by rememberSaveable { mutableStateOf(false) }
    // The category filter: an item with ANY chosen category, or none when "Sem categoria" is chosen.
    val all = remember(state.tasks, categoryFilter) {
        state.tasks.filter { CategoryRules.matches(it.categories, categoryFilter) }
    }
    // The core says which group each task is in, and sends them in the order to show.
    val groups = remember(all) {
        fun of(bucket: String) = all.filter { !it.done && it.openBucket == bucket }
        listOf(
            TaskGroup("late", "Atrasadas", of("atrasadas"), late = true),
            TaskGroup("today", "Para hoje", of("hoje")),
            TaskGroup("next", "Próximas", of("proximas")),
            TaskGroup("none", "Sem prazo", of("sem_prazo"))
        ).filter { it.tasks.isNotEmpty() }
    }
    val done = remember(all) { all.filter { it.done } }
    val noOpen = all.none { !it.done }
    val filtering = categoryFilter.isNotEmpty()

    if (state.loading && state.tasks.isEmpty()) {
        LoadingBox(text = "Carregando as tarefas…", modifier = Modifier.fillMaxSize())
        return
    }

    Box(Modifier.fillMaxSize()) {
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, top = 8.dp, end = 16.dp, bottom = 104.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        state.error?.let { item(key = "error") { Notice(it) } }
        item(key = "category-filter") { CategoryFilterButton(categoryFilter, onCategoryFilter) }
        if (noOpen && filtering) {
            item(key = "empty-filter") {
                EmptyState(
                    "Nenhuma tarefa em aberto com essas categorias.",
                    text = "Mude as categorias marcadas ou limpe o filtro.",
                    icon = Icons.Outlined.CheckCircle,
                    action = {
                        TextButton(onClick = { onCategoryFilter(emptySet()) }) {
                            Text("Limpar o filtro", color = c.accent, fontWeight = FontWeight.SemiBold)
                        }
                    }
                )
            }
        } else if (noOpen) {
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
    ExtendedFloatingActionButton(
        onClick = onNew,
        modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
        containerColor = c.accent, contentColor = c.onAccent,
        elevation = FloatingActionButtonDefaults.elevation(2.dp, 2.dp, 2.dp, 2.dp),
        icon = { Icon(Icons.Outlined.Add, contentDescription = null) },
        text = { Text("Nova tarefa", fontSize = 15.sp, fontWeight = FontWeight.SemiBold) }
    )
    }
}

/** One task in a card (Hoje and Tarefas): check to conclude, tap to open it. */
@Composable
internal fun TaskCard(t: TaskItem, onToggle: (TaskItem) -> Unit, onOpen: (TaskItem) -> Unit) {
    PonteCard {
        TaskRow(
            t, t.secondLine,
            onToggle = { onToggle(t) }, onOpen = { onOpen(t) },
            modifier = Modifier.padding(start = 6.dp, end = 12.dp, top = 6.dp, bottom = 6.dp)
        )
    }
}
