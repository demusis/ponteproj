package io.github.demusis.ponte

import androidx.compose.foundation.clickable
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
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.text.font.FontWeight
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

/**
 * The Tarefas area (inside MainShell): open tasks grouped by urgency, finished ones below, as flat rows. The
 * category and project filters are behind the filter icon of the frame; while one is on, the first row says which.
 */
@Composable
internal fun TasksScreen(
    state: TasksState,
    onToggle: (TaskItem) -> Unit,
    onOpen: (TaskItem) -> Unit,
    onNew: () -> Unit = {},
    categoryFilter: Set<String> = emptySet(),
    onCategoryFilter: (Set<String>) -> Unit = {},
    projects: List<ProjectItem> = emptyList(),
    projectFilter: String = "",
    onProjectFilter: (String) -> Unit = {}
) {
    val c = Ponte.colors
    var showDone by rememberSaveable { mutableStateOf(false) }
    val categories = LocalCategories.current
    // The category filter: an item with ANY chosen category, or none when "Sem categoria" is chosen.
    // The project filter: one project, or "Sem projeto"; both together narrow the list.
    val all = remember(state.tasks, categoryFilter, projectFilter) {
        state.tasks.filter {
            CategoryRules.matches(it.categories, categoryFilter) && ProjectRules.matchesTask(it.projectUid, projectFilter)
        }
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
    val filtering = FilterSummary.isActive(categoryFilter, projectFilter)
    val clearFilter = { onCategoryFilter(emptySet()); onProjectFilter("") }

    if (state.loading && state.tasks.isEmpty()) {
        LoadingBox(text = "Carregando as tarefas…", modifier = Modifier.fillMaxSize())
        return
    }

    Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = 4.dp, bottom = FAB_CLEARANCE)) {
            state.error?.let { item(key = "error") { Notice(it, Modifier.padding(horizontal = ROW_MARGIN, vertical = 4.dp)) } }
            if (filtering) {
                item(key = "filter-line") {
                    FilterSummaryRow(
                        FilterSummary.line(
                            FilterSummary.categoryNames(categoryFilter, categories), FilterSummary.projectName(projectFilter, projects)
                        ),
                        onClear = clearFilter
                    )
                }
            }
            if (noOpen && filtering) {
                item(key = "empty-filter") {
                    val onlyCategories = projectFilter.isEmpty()
                    EmptyState(
                        if (onlyCategories) "Nenhuma tarefa em aberto com essas categorias." else "Nenhuma tarefa em aberto com esse filtro.",
                        text = if (onlyCategories) "Mude as categorias marcadas ou limpe o filtro." else "Mude o filtro ou limpe-o.",
                        icon = Icons.Outlined.CheckCircle,
                        action = {
                            TextButton(onClick = clearFilter) {
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
                        "${g.title} · ${g.tasks.size}", Modifier.padding(start = 12.dp, top = 12.dp, bottom = 4.dp),
                        color = if (g.late) c.error else c.muted
                    )
                }
                items(g.tasks, key = { "t-${it.id}" }) { t -> TaskListRow(t, { onToggle(t) }, { onOpen(t) }) }
            }
            if (done.isNotEmpty()) {
                item(key = "h-done") {
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { showDone = !showDone }.padding(start = 12.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        SectionLabel("Concluídas · ${done.size}", Modifier.weight(1f))
                        Icon(
                            Icons.Outlined.ExpandMore,
                            contentDescription = if (showDone) "Recolher as concluídas" else "Mostrar as concluídas",
                            tint = c.muted, modifier = Modifier.padding(end = 16.dp).size(22.dp).rotate(if (showDone) 180f else 0f)
                        )
                    }
                }
                if (showDone) items(done, key = { "d-${it.id}" }) { t -> TaskListRow(t, { onToggle(t) }, { onOpen(t) }) }
            }
        }
        PonteFab(Icons.Outlined.Add, "Nova tarefa", onNew, Modifier.align(Alignment.BottomEnd))
    }
}
