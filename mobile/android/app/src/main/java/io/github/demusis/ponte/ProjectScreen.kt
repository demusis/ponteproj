package io.github.demusis.ponte

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.LocalDate

/**
 * One project: what it is (name, situation, objective, who, dates), how far along it is, and two tabs: its
 * tasks and its timeline. "Editar", "Nova tarefa" and "Incluir tarefas" are always at hand; "Arquivar" or
 * "Reabrir" and "Excluir projeto" are in the menu.
 */
@Composable
internal fun ProjectScreen(
    uid: String,
    state: ProjectViewState,
    onBack: () -> Unit,
    onEdit: () -> Unit,
    onNewTask: () -> Unit,
    onInclude: () -> Unit,
    onStatus: (ProjectItem, String) -> Unit,
    onDelete: (String) -> Unit,
    onToggleTask: (TaskItem) -> Unit,
    onOpenTask: (Long) -> Unit,
    onRemoveTask: (TaskItem) -> Unit,
    /** Today, for the line of the timeline (a parameter so the pictures are always the same). */
    today: LocalDate = LocalDate.now(),
    /** 0 Tarefas, 1 Linha do tempo. */
    initialTab: Int = 0
) {
    val c = Ponte.colors
    // Only the project this screen was opened for: never the data of another that is loaded.
    val detail = state.detail?.takeIf { state.uid == uid && it.project.uid == uid }
    if (detail == null) {
        DetailScaffold(title = "Projeto", onBack = onBack) {
            val error = state.error.takeIf { state.uid == uid }
            if (error != null) Notice(error, tone = Tone.ERROR, modifier = Modifier.padding(16.dp)) else LoadingBox()
        }
        return
    }
    val p = detail.project
    var menu by rememberSaveable { mutableStateOf(false) }
    var deleting by rememberSaveable { mutableStateOf(false) }
    var tab by rememberSaveable { mutableStateOf(initialTab) }
    var showDone by rememberSaveable { mutableStateOf(false) }
    DetailScaffold(
        title = "Projeto", onBack = onBack,
        actions = {
            TextButton(onClick = onEdit, enabled = !state.busy) {
                Text("Editar", color = if (state.busy) c.muted else c.accent, fontWeight = FontWeight.SemiBold)
            }
            Column {
                IconButton(onClick = { menu = true }, enabled = !state.busy) {
                    Icon(Icons.Outlined.MoreVert, contentDescription = "Mais ações do projeto", tint = c.secondary)
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    if (ProjectRules.canArchive(p)) {
                        DropdownMenuItem(text = { Text("Arquivar", fontSize = 16.sp) }, onClick = { menu = false; onStatus(p, ProjectRules.ARCHIVED) })
                    }
                    if (ProjectRules.canReopen(p)) {
                        DropdownMenuItem(text = { Text("Reabrir", fontSize = 16.sp) }, onClick = { menu = false; onStatus(p, ProjectRules.ACTIVE) })
                    }
                    DropdownMenuItem(
                        text = { Text("Excluir projeto", fontSize = 16.sp, color = c.error) },
                        onClick = { menu = false; deleting = true }
                    )
                }
            }
        }
    ) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = 4.dp, bottom = 32.dp)) {
            state.error?.let { item(key = "error") { Notice(it, modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 8.dp)) } }
            item(key = "head") { ProjectHeader(p, Modifier.padding(horizontal = 16.dp)) }
            item(key = "actions") {
                Row(Modifier.padding(start = 16.dp, top = 12.dp, end = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    PrimaryButton("Nova tarefa", onNewTask, Modifier.weight(1f), enabled = !state.busy)
                    SecondaryButton("Incluir tarefas", onInclude, Modifier.weight(1f), enabled = !state.busy)
                }
            }
            item(key = "tabs") {
                SegmentedTabs(listOf("Tarefas", "Linha do tempo"), tab, { tab = it }, Modifier.padding(top = 8.dp))
            }
            if (tab == 0) {
                tasksTab(detail, showDone, { showDone = !showDone }, onToggleTask, onOpenTask, onRemoveTask)
            } else {
                item(key = "timeline") {
                    TimelineSection(detail, today, onOpenTask, Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
                }
            }
        }
    }
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text("Excluir o projeto?") },
            text = {
                Text(
                    "“${p.name}” será excluído. " + ProjectRules.deleteQuestion(p.progress.total),
                    style = MaterialTheme.typography.bodyMedium
                )
            },
            confirmButton = { TextButton(onClick = { deleting = false; onDelete(p.uid) }) { Text("Excluir projeto", color = c.error) } },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text("Cancelar") } }
        )
    }
}

@Composable
private fun ProjectHeader(p: ProjectItem, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    PonteCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.Top) {
                Text(
                    p.name, Modifier.weight(1f), style = MaterialTheme.typography.headlineSmall, color = c.text,
                    maxLines = 4, overflow = TextOverflow.Ellipsis
                )
                Spacer(Modifier.width(8.dp))
                ProjectStatusPill(p, Modifier.padding(top = 4.dp))
            }
            if (p.objective.isNotBlank()) {
                Text(p.objective, style = MaterialTheme.typography.bodyMedium, color = c.secondary)
            }
            InfoLine("Responsável", p.assignee.ifEmpty { "Não informado" }, muted = p.assignee.isEmpty())
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                InfoLine("Início", if (p.startDate.isEmpty()) "—" else formatDocDate(p.startDate), Modifier.weight(1f), muted = p.startDate.isEmpty())
                InfoLine("Data-alvo", if (p.targetDate.isEmpty()) "—" else formatDocDate(p.targetDate), Modifier.weight(1f), muted = p.targetDate.isEmpty())
            }
            ProjectProgressBar(p.progress, Modifier.padding(top = 4.dp))
            Text(ProjectRules.progressLine(p.progress), style = MaterialTheme.typography.bodySmall, color = c.secondary)
            val next = ProjectRules.nextDueLine(p.progress)
            if (next.isNotEmpty()) Text(next, style = MaterialTheme.typography.bodySmall, color = c.muted)
            ProjectAlerts(p)
            val risk = ProjectRules.targetRiskSentence(p)
            if (risk.isNotEmpty()) Notice(risk, tone = Tone.WARNING)
        }
    }
}

@Composable
private fun InfoLine(label: String, value: String, modifier: Modifier = Modifier, muted: Boolean = false) {
    val c = Ponte.colors
    Column(modifier) {
        Text(label, style = FieldLabelStyle, color = c.muted)
        Text(
            value, style = MaterialTheme.typography.bodyMedium, color = if (muted) c.muted else c.text,
            maxLines = 2, overflow = TextOverflow.Ellipsis
        )
    }
}

// ---------------------------------------------------------------- the Tarefas tab

private fun LazyListScope.tasksTab(
    detail: ProjectDetail,
    showDone: Boolean,
    onShowDone: () -> Unit,
    onToggle: (TaskItem) -> Unit,
    onOpen: (Long) -> Unit,
    onRemove: (TaskItem) -> Unit
) {
    val groups = ProjectRules.groups(detail.tasks)
    if (detail.tasks.isEmpty()) {
        item(key = "none") {
            EmptyState(
                "Nenhuma tarefa neste projeto.",
                text = "Use Nova tarefa ou Incluir tarefas para começar.",
                icon = Icons.Outlined.CheckCircle
            )
        }
        return
    }
    if (groups.late.isNotEmpty()) {
        item(key = "h-late") {
            SectionLabel("Atrasadas · ${groups.late.size}", Modifier.padding(start = 16.dp, top = 12.dp, end = 16.dp), color = Ponte.colors.error)
        }
        items(groups.late, key = { "l-${it.id}" }) { ProjectTaskCard(it, onToggle, onOpen, onRemove) }
    }
    if (groups.open.isNotEmpty()) {
        item(key = "h-open") {
            SectionLabel("Abertas · ${groups.open.size}", Modifier.padding(start = 16.dp, top = 12.dp, end = 16.dp))
        }
        items(groups.open, key = { "o-${it.id}" }) { ProjectTaskCard(it, onToggle, onOpen, onRemove) }
    }
    if (groups.done.isNotEmpty()) {
        item(key = "h-done") { DoneHeader(groups.done.size, showDone, onShowDone) }
        // Folded until asked for.
        if (showDone) items(groups.done, key = { "d-${it.id}" }) { ProjectTaskCard(it, onToggle, onOpen, onRemove) }
    }
}

/** The "Concluídas" header: tapping it shows or folds the finished tasks. */
@Composable
private fun DoneHeader(count: Int, open: Boolean, onToggle: () -> Unit) {
    val c = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = 12.dp, end = 12.dp).clickable(onClick = onToggle),
        verticalAlignment = Alignment.CenterVertically
    ) {
        SectionLabel("Concluídas · $count", Modifier.weight(1f))
        Icon(
            Icons.Outlined.ExpandMore,
            contentDescription = if (open) "Recolher as concluídas" else "Mostrar as concluídas",
            tint = c.muted, modifier = Modifier.padding(end = 4.dp).size(22.dp).rotate(if (open) 180f else 0f)
        )
    }
}

/** One task of the project: the usual row and a small menu with "Tirar do projeto". */
@Composable
internal fun ProjectTaskCard(t: TaskItem, onToggle: (TaskItem) -> Unit, onOpen: (Long) -> Unit, onRemove: (TaskItem) -> Unit) {
    val c = Ponte.colors
    var menu by remember { mutableStateOf(false) }
    PonteCard(Modifier.padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TaskRow(
                t, t.secondLine, onToggle = { onToggle(t) }, onOpen = { onOpen(t.id) },
                modifier = Modifier.weight(1f).padding(start = 6.dp, top = 6.dp, bottom = 6.dp),
                showProject = false
            )
            Column {
                IconButton(onClick = { menu = true }) {
                    Icon(Icons.Outlined.MoreVert, contentDescription = "Mais ações da tarefa ${t.title}", tint = c.muted)
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text("Tirar do projeto", fontSize = 16.sp) }, onClick = { menu = false; onRemove(t) })
                }
            }
        }
    }
}
