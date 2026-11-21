package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FloatingActionButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// The Projetos area: the list, the form, the list to include tasks, and the small pieces other screens
// use (the project chip of a task row, the Projeto field, the project filter).

/** The tone of the situation chip. */
private fun statusTone(status: String): Tone = when (status) {
    ProjectRules.ACTIVE -> Tone.ACCENT
    ProjectRules.DONE -> Tone.SUCCESS
    else -> Tone.NEUTRAL
}

@Composable
internal fun ProjectStatusPill(p: ProjectItem, modifier: Modifier = Modifier) {
    Pill(p.statusLabel.ifEmpty { ProjectRules.statusLabel(p.status) }, modifier, statusTone(p.status))
}

/** The project of a task as a small chip: a folder and the name, cut with an ellipsis. Never wider than 200 dp. */
@Composable
internal fun ProjectChip(name: String, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    Row(
        modifier.widthIn(max = 200.dp).background(c.chip, RoundedCornerShape(50))
            .padding(start = 7.dp, end = 10.dp, top = 3.dp, bottom = 3.dp)
            .clearAndSetSemantics { contentDescription = "Projeto $name" },
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(Icons.Outlined.Folder, contentDescription = null, tint = c.secondary, modifier = Modifier.size(13.dp))
        Spacer(Modifier.width(4.dp))
        Text(
            name, fontSize = 12.sp, lineHeight = 15.sp, fontWeight = FontWeight.Medium, color = c.secondary,
            maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false)
        )
    }
}

/** The thin bar of the progress; the text under it says the same, so a screen reader skips the bar. */
@Composable
internal fun ProjectProgressBar(p: ProjectProgress, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    val complete = p.total > 0 && p.done >= p.total
    LinearProgressIndicator(
        progress = { ProjectRules.fraction(p) },
        modifier = modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)).clearAndSetSemantics { },
        color = if (complete) c.success else c.accent, trackColor = c.chip
    )
}

/** The red "N atrasadas" and amber "N em risco" chips of a project. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ProjectAlerts(p: ProjectItem, modifier: Modifier = Modifier) {
    if (p.progress.late == 0 && p.risk.deliveriesAtRisk == 0) return
    FlowRow(modifier, horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (p.progress.late > 0) Pill(ProjectRules.lateLabel(p.progress.late), tone = Tone.ERROR)
        if (p.risk.deliveriesAtRisk > 0) Pill(ProjectRules.riskLabel(p.risk.deliveriesAtRisk), tone = Tone.WARNING)
    }
}

/** The situation chips: wrapped onto a second line when they do not fit, so the chosen one is always in view. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ProjectFilters(filter: String, onFilter: (String) -> Unit) {
    FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for ((key, label) in ProjectRules.FILTERS) FilterPill(label, key == filter) { onFilter(key) }
    }
}

/** One project in the list, as a flat row. */
@Composable
internal fun ProjectRow(p: ProjectItem, onClick: () -> Unit) {
    val c = Ponte.colors
    FlatRow(onClick, Modifier, ROW_MARGIN) {
        Column(
            Modifier.fillMaxWidth().padding(vertical = 4.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    p.name, Modifier.weight(1f), fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Bold,
                    color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis
                )
                Spacer(Modifier.width(8.dp))
                ProjectStatusPill(p)
            }
            if (p.assignee.isNotEmpty()) {
                Text(
                    "Responsável: ${p.assignee}", style = MaterialTheme.typography.bodySmall, color = c.secondary,
                    maxLines = 1, overflow = TextOverflow.Ellipsis
                )
            }
            ProjectProgressBar(p.progress, Modifier.padding(top = 2.dp))
            Text(ProjectRules.progressLine(p.progress), style = MaterialTheme.typography.bodySmall, color = c.secondary)
            val next = ProjectRules.nextDueLine(p.progress)
            if (next.isNotEmpty()) Text(next, style = MaterialTheme.typography.bodySmall, color = c.muted)
            ProjectAlerts(p)
            val target = ProjectRules.targetRiskLine(p)
            if (target.isNotEmpty()) {
                Text(target, style = MaterialTheme.typography.bodySmall, fontWeight = FontWeight.Medium, color = c.warning)
            }
        }
    }
}

/**
 * The Projetos list (inside MainShell): a search box, the situation chips (Ativos by default) and one card per
 * project, with a button for a new one.
 */
@Composable
internal fun ProjectsScreen(
    state: ProjectsState,
    filter: String,
    onFilter: (String) -> Unit,
    query: String,
    onQuery: (String) -> Unit,
    onOpen: (ProjectItem) -> Unit,
    onNew: () -> Unit
) {
    val c = Ponte.colors
    val shown = remember(state.rows, filter, query) { ProjectRules.visible(state.rows, filter, query) }
    if (state.loading && state.rows.isEmpty()) {
        LoadingBox(text = "Carregando os projetos…", modifier = Modifier.fillMaxSize())
        return
    }
    FabColumn(Icons.Outlined.Add, "Novo projeto", onNew) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = 4.dp, bottom = 8.dp)) {
            val pad = Modifier.padding(horizontal = ROW_MARGIN, vertical = 4.dp)
            state.error?.let { item(key = "error") { Notice(it, pad) } }
            if (state.rows.isNotEmpty()) {
                item(key = "search") { SearchBox(query, onQuery, "Nome ou objetivo", pad) }
                item(key = "filters") { Box(pad) { ProjectFilters(filter, onFilter) } }
            }
            if (shown.isEmpty()) {
                item(key = "empty") {
                    when (ProjectRules.emptyKind(state.rows, query)) {
                        ProjectRules.Empty.NONE_AT_ALL -> EmptyState(
                            "Nenhum projeto ainda.", text = ProjectRules.WHAT_IS, icon = Icons.Outlined.Folder
                        )
                        ProjectRules.Empty.NOTHING_FOUND -> EmptyState(
                            "Nada encontrado.",
                            text = if (filter == ProjectRules.ALL) "Tente outras palavras."
                            else "Nenhum projeto desta situação combina com a busca.",
                            icon = Icons.Outlined.SearchOff,
                            action = {
                                TextButton(onClick = { onQuery("") }) {
                                    Text("Limpar a busca", color = c.accent, fontWeight = FontWeight.SemiBold)
                                }
                            }
                        )
                        ProjectRules.Empty.NONE_IN_FILTER -> EmptyState(
                            ProjectRules.noneIn(filter), text = "Toque em Todos para ver os outros.", icon = Icons.Outlined.Folder,
                            action = {
                                TextButton(onClick = { onFilter(ProjectRules.ALL) }) {
                                    Text("Mostrar todos", color = c.accent, fontWeight = FontWeight.SemiBold)
                                }
                            }
                        )
                    }
                }
            }
            items(shown, key = { "p-${it.uid}" }) { p -> ProjectRow(p) { onOpen(p) } }
        }
    }
}

// ---------------------------------------------------------------- the Projeto field and the task filter

/** "Projeto" on the task screens: "Sem projeto" and the projects; nothing while there is nothing to choose. */
@Composable
internal fun ProjectField(
    selected: String,
    projects: List<ProjectItem>,
    currentName: String,
    onChange: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val options = ProjectRules.fieldOptions(projects, selected, currentName)
    if (!ProjectRules.fieldShown(options, selected)) return
    ChoiceField("Projeto", options, selected, onChange, modifier)
}

/** The body of the filter sheet: every project, "Sem projeto" and "Todos os projetos"; a tap chooses and closes. */
@Composable
internal fun ProjectFilterContent(projects: List<ProjectItem>, chosen: String, onPick: (String) -> Unit) {
    val p = Ponte.colors
    Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp).padding(bottom = 16.dp)) {
        Text("Filtrar por projeto", style = MaterialTheme.typography.titleMedium, color = p.text, modifier = Modifier.padding(bottom = 6.dp))
        Column(Modifier.heightIn(max = 420.dp).verticalScroll(rememberScrollState())) {
            PickRow(chosen.isEmpty(), true, "Todos os projetos", { onPick("") }) {
                Text("Todos os projetos", Modifier.weight(1f), fontSize = 16.sp, color = p.text)
            }
            PickRow(chosen == ProjectRules.NONE, true, "Sem projeto", { onPick(ProjectRules.NONE) }) {
                Text("Sem projeto", Modifier.weight(1f), fontSize = 16.sp, color = p.text)
            }
            for (project in ProjectRules.filterOptions(projects)) {
                val archived = project.status == ProjectRules.ARCHIVED
                PickRow(project.uid == chosen, true, project.name, { onPick(project.uid) }) {
                    Text(
                        if (archived) "${project.name} (arquivado)" else project.name, Modifier.weight(1f),
                        fontSize = 16.sp, color = if (archived) p.muted else p.text, maxLines = 1, overflow = TextOverflow.Ellipsis
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the form

/**
 * Form of a project: Nome, Objetivo, Responsável, Situação, Início and Data-alvo. [project] is null for a new
 * one. What is typed is kept (rotation, lock); leaving with a change asks first. A refusal of the core is shown
 * above the button; a project changed elsewhere offers "Ver a versão atual".
 */
@Composable
internal fun ProjectFormScreen(
    uid: String,
    project: ProjectItem?,
    loading: Boolean,
    state: ProjectEditState,
    assignees: List<String>,
    myName: String,
    onBack: () -> Unit,
    onSave: (ProjectForm, Long) -> Unit,
    onReload: () -> Unit
) {
    if (uid.isNotEmpty() && project == null) {
        DetailScaffold(title = "Projeto", onBack = onBack, close = true) {
            if (loading) LoadingBox() else Notice(
                "Projeto não encontrado. Ele pode ter sido excluído em outro aparelho.", tone = Tone.ERROR,
                modifier = Modifier.padding(16.dp)
            )
        }
        return
    }
    // A new epoch or a newer stored version starts the form over from the stored project.
    key(state.epoch, project?.version ?: 0L) { ProjectFormLoaded(uid, project, state, assignees, myName, onBack, onSave, onReload) }
}

@Composable
private fun ProjectFormLoaded(
    uid: String,
    project: ProjectItem?,
    state: ProjectEditState,
    assignees: List<String>,
    myName: String,
    onBack: () -> Unit,
    onSave: (ProjectForm, Long) -> Unit,
    onReload: () -> Unit
) {
    val c = Ponte.colors
    val saved = project?.let { ProjectForm.of(it) } ?: ProjectForm()
    var name by rememberSaveable { mutableStateOf(saved.name) }
    var objective by rememberSaveable { mutableStateOf(saved.objective) }
    var assignee by rememberSaveable { mutableStateOf(saved.assignee) }
    var status by rememberSaveable { mutableStateOf(saved.status) }
    var start by rememberSaveable { mutableStateOf(saved.startDate) }
    var target by rememberSaveable { mutableStateOf(saved.targetDate) }
    var tried by rememberSaveable { mutableStateOf(false) }
    var asking by rememberSaveable { mutableStateOf(false) }
    val form = ProjectForm(name, objective, assignee, status, start, target)
    val changed = form.changedFrom(saved)
    val problem = ProjectRules.formProblem(form)
    val leave = { if (changed) asking = true else onBack() }
    val isNew = uid.isEmpty()

    SafeBackHandler(onBack = leave)

    DetailScaffold(
        title = if (isNew) "Novo projeto" else "Editar projeto", onBack = { if (!state.busy) leave() }, close = true,
        footer = {
            if (state.error.isNotEmpty()) {
                Notice(state.error, tone = Tone.ERROR, modifier = Modifier.padding(bottom = if (state.conflict) 4.dp else 12.dp))
            }
            if (state.conflict) {
                TextButton(onClick = onReload, modifier = Modifier.padding(bottom = 8.dp)) {
                    Text("Ver a versão atual", color = c.accent, fontWeight = FontWeight.SemiBold)
                }
            }
            PrimaryButton(
                if (isNew) "Criar projeto" else "Salvar",
                onClick = { if (problem.isNotEmpty()) tried = true else onSave(form, project?.version ?: 0L) },
                enabled = (isNew || changed) && !state.conflict, loading = state.busy
            )
        }
    ) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState())
                .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp)
        ) {
            LabeledField(
                "Nome", name, { name = it.take(ProjectRules.MAX_NAME); tried = false },
                placeholder = "Como o projeto se chama",
                error = if (tried && name.isBlank()) "Informe o nome." else "",
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
            )
            LabeledField(
                "Objetivo", objective, { objective = it }, placeholder = "O que o projeto quer alcançar",
                singleLine = false, minLines = 3,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
            )
            AssigneeField(assignee, { assignee = it }, assignees, myName)
            ChoiceField("Situação", ProjectRules.STATUSES, status, { status = it })
            DateField("Início", start, { start = it; tried = false }, emptyText = "Sem data")
            DateField("Data-alvo", target, { target = it; tried = false }, emptyText = "Sem data")
            if (tried && problem.isNotEmpty() && !name.isBlank()) {
                Text(problem, style = MaterialTheme.typography.bodySmall, color = c.error)
            }
        }
    }
    if (asking) {
        AlertDialog(
            onDismissRequest = { asking = false },
            title = { Text("Descartar as alterações?") },
            text = { Text("O que você mudou neste projeto ainda não foi salvo.") },
            confirmButton = { TextButton(onClick = { asking = false; onBack() }) { Text("Descartar", color = c.error) } },
            dismissButton = { TextButton(onClick = { asking = false }) { Text("Continuar editando") } }
        )
    }
}

// ---------------------------------------------------------------- Incluir tarefas

/**
 * "Incluir tarefas": the open tasks that have no project, with a search box; the person ticks any number
 * and includes them at once. Tasks of another project are not offered (take them out of it first).
 */
@Composable
internal fun ProjectIncludeScreen(
    projectName: String,
    tasks: List<TaskItem>,
    state: SaveState,
    onBack: () -> Unit,
    onInclude: (List<Long>) -> Unit
) {
    val c = Ponte.colors
    var query by rememberSaveable { mutableStateOf("") }
    var chosen by rememberSaveable { mutableStateOf(emptyList<Long>()) }
    val candidates = remember(tasks, query) { ProjectRules.candidates(tasks, query) }
    // A task that left the list (concluded or put in another project elsewhere) cannot stay ticked.
    val ticked = chosen.filter { id -> tasks.any { it.id == id && !it.done && it.projectUid.isEmpty() } }
    DetailScaffold(
        title = "Incluir tarefas", onBack = { if (!state.busy) onBack() }, close = true,
        footer = {
            if (state.error.isNotEmpty()) Notice(state.error, tone = Tone.ERROR, modifier = Modifier.padding(bottom = 12.dp))
            PrimaryButton(
                ProjectRules.includeLabel(ticked.size), onClick = { onInclude(ticked) },
                enabled = ticked.isNotEmpty(), loading = state.busy
            )
        }
    ) {
        Column(Modifier.fillMaxSize()) {
            Text(
                projectName.ifEmpty { "Projeto" }, Modifier.padding(start = 16.dp, end = 16.dp),
                style = MaterialTheme.typography.titleMedium, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis
            )
            Text(
                "Tarefas em aberto que ainda não estão em um projeto.",
                Modifier.padding(start = 16.dp, end = 16.dp, top = 2.dp, bottom = 8.dp),
                style = MaterialTheme.typography.bodySmall, color = c.muted
            )
            SearchBox(query, { query = it }, "Buscar tarefa", Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
            if (candidates.isEmpty()) {
                EmptyState(
                    if (query.isBlank()) "Nenhuma tarefa livre." else "Nada encontrado.",
                    text = if (query.isBlank()) "Todas as tarefas em aberto já estão em algum projeto."
                    else "Nenhuma tarefa livre combina com a busca.",
                    icon = Icons.Outlined.Folder
                )
            } else {
                LazyColumn(
                    Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(start = 8.dp, top = 4.dp, end = 16.dp, bottom = 16.dp)
                ) {
                    items(candidates, key = { "c-${it.id}" }) { t ->
                        val on = t.id in ticked
                        IncludeRow(t, on, enabled = !state.busy) {
                            chosen = if (on) chosen - t.id else chosen + t.id
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun IncludeRow(t: TaskItem, on: Boolean, enabled: Boolean, onToggle: () -> Unit) {
    val c = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(12.dp))
            .toggleable(value = on, enabled = enabled, role = Role.Checkbox, onValueChange = { onToggle() })
            .semantics { contentDescription = (if (on) "Marcada: " else "") + t.title + if (t.dueChip.isNotEmpty()) ", prazo ${t.dueChip}" else "" }
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Checkbox(
            checked = on, onCheckedChange = null, enabled = enabled,
            colors = CheckboxDefaults.colors(checkedColor = c.accent, uncheckedColor = c.outline, checkmarkColor = c.onAccent),
            modifier = Modifier.padding(horizontal = 12.dp)
        )
        Column(Modifier.weight(1f)) {
            Text(
                t.title, fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, color = c.text,
                maxLines = 2, overflow = TextOverflow.Ellipsis
            )
            val second = listOf(t.actionLabel, if (t.dueChip.isNotEmpty()) "Prazo ${t.dueChip}" else "").filter { it.isNotEmpty() }.joinToString(" · ")
            if (second.isNotEmpty()) {
                Text(
                    second, style = MaterialTheme.typography.bodySmall, color = if (t.overdue) c.error else c.muted,
                    maxLines = 1, overflow = TextOverflow.Ellipsis
                )
            }
        }
    }
}
