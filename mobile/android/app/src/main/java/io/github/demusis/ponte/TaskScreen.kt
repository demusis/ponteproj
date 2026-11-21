package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** The priorities as the core knows them, until the core's own list arrives. */
internal val DefaultPriorities = listOf(
    Choice("baixa", "Baixa"), Choice("normal", "Normal"), Choice("alta", "Alta"), Choice("urgente", "Urgente")
)

/**
 * One task, for editing. What is typed is kept (rotation, lock) and compared with the stored
 * task: Salvar waits for a change, and leaving with one asks first. [onFinish] concludes (or
 * reopens) and takes the typed changes along.
 */
@Composable
internal fun TaskScreen(
    routeId: Long,
    state: TaskEditState,
    actions: List<Choice>,
    priorities: List<Choice>,
    onBack: () -> Unit,
    onSave: (TaskSave) -> Unit,
    onFinish: (TaskSave) -> Unit,
    onOpenDocument: (Long) -> Unit,
    onLink: () -> Unit,
    onUnlink: () -> Unit,
    onReload: () -> Unit,
    assignees: List<String> = emptyList(),
    myName: String = "",
    onCreateCategory: (String, Int, (Category?, String) -> Unit) -> Unit = { _, _, done -> done(null, "") },
    remindersBlocked: Boolean = false,
    onRemindersChosen: () -> Unit = {},
    relations: TasksState = TasksState(),
    onOpenTask: (Long) -> Unit = {},
    projects: List<ProjectItem> = emptyList()
) {
    // Only the task this screen was opened for: never the form of one with the data of another.
    val task = TaskRules.shown(routeId, state)
    if (task == null) {
        DetailScaffold(title = "Tarefa", onBack = onBack, close = true) {
            val error = state.error.takeIf { state.id == routeId }
            if (error != null) Notice(error, tone = Tone.ERROR, modifier = Modifier.padding(16.dp)) else LoadingBox()
        }
        return
    }
    key(task.id) {
        TaskLoaded(state, task, actions, priorities, onBack, onSave, onFinish, onOpenDocument, onLink, onUnlink, onReload, assignees, myName, onCreateCategory, remindersBlocked, onRemindersChosen, relations, onOpenTask, projects)
    }
}

private val DraftSaver = Saver<TaskDraft, List<Any>>(save = { it.toList() }, restore = { TaskDraft.fromList(it) })

@Composable
private fun TaskLoaded(
    state: TaskEditState,
    task: TaskDetail,
    actions: List<Choice>,
    priorities: List<Choice>,
    onBack: () -> Unit,
    onSave: (TaskSave) -> Unit,
    onFinish: (TaskSave) -> Unit,
    onOpenDocument: (Long) -> Unit,
    onLink: () -> Unit,
    onUnlink: () -> Unit,
    onReload: () -> Unit,
    assignees: List<String>,
    myName: String,
    onCreateCategory: (String, Int, (Category?, String) -> Unit) -> Unit,
    remindersBlocked: Boolean,
    onRemindersChosen: () -> Unit,
    relations: TasksState,
    onOpenTask: (Long) -> Unit,
    projects: List<ProjectItem>
) {
    val c = Ponte.colors
    // The draft remembers the version it started from; a newer stored task resets it (nothing
    // typed) or flags a conflict (something typed). See TaskRules.reconcile.
    var stored by rememberSaveable(stateSaver = DraftSaver) { mutableStateOf(TaskRules.start(task, state.epoch)) }
    val draft = TaskRules.reconcile(stored, task, state.epoch)
    LaunchedEffect(draft) { if (draft != stored) stored = draft }
    val form = draft.form
    val edit = { change: (TaskForm) -> TaskForm -> stored = draft.copy(form = change(draft.form)) }
    var tried by rememberSaveable { mutableStateOf(false) }
    var asking by rememberSaveable { mutableStateOf(false) }
    var parentPicker by rememberSaveable { mutableStateOf(false) }
    var dependencyPicker by rememberSaveable { mutableStateOf(false) }
    var pendingSubtask by rememberSaveable { mutableStateOf<Long?>(null) }
    var unlinking by rememberSaveable { mutableStateOf(false) }
    val changed = draft.changed
    val conflict = draft.conflict || state.conflict
    val message = if (draft.conflict) TaskRules.CHANGED else state.formError
    val linked = task.documentId > 0
    val leave = { if (changed) asking = true else onBack() }
    // A task without document has nothing to triage.
    val shownActions = if (!linked && form.action != "triar") actions.filter { it.key != "triar" } else actions
    val titleMissing = form.title.isBlank()
    // What is typed in the hour and days boxes starts over with a new epoch or stored version.
    var startHour by rememberSaveable(state.epoch, task.version) { mutableStateOf(TaskRules.splitMoment(form.scheduledStart).second) }
    var endHour by rememberSaveable(state.epoch, task.version) { mutableStateOf(TaskRules.splitMoment(form.scheduledEnd).second) }
    var daysText by rememberSaveable(state.epoch, task.version) { mutableStateOf(TaskRules.estimateDays(form.estimateMinutes).toString()) }
    // The form only takes a valid hour; Salvar waits for the one that is not.
    val hourInvalid = planningHourInvalid(form.scheduledStart, form.scheduledEnd, startHour, endHour)

    SafeBackHandler(onBack = leave)

    DetailScaffold(
        title = "Tarefa", onBack = leave, close = true,
        // Unlinking is rare and destructive: it lives in the "⋮" menu (and keeps its confirmation).
        menu = if (linked) ({ dismiss ->
            DropdownMenuItem(
                text = { Text("Desvincular do documento", fontSize = 16.sp, color = if (!changed && !state.busy) c.error else c.muted) },
                enabled = !changed && !state.busy,
                onClick = { dismiss(); unlinking = true }
            )
        }) else null,
        footer = {
            if (message.isNotEmpty()) {
                Notice(message, tone = Tone.ERROR, modifier = Modifier.padding(bottom = if (conflict) 4.dp else 12.dp))
            }
            if (conflict) {
                TextButton(onClick = onReload, modifier = Modifier.padding(bottom = 8.dp)) {
                    Text("Ver a versão atual", color = c.accent, fontWeight = FontWeight.SemiBold)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                SecondaryButton(
                    if (task.done) "Reabrir" else "Concluir",
                    onClick = { if (titleMissing) tried = true else onFinish(draft.toSave(task.id)) },
                    modifier = Modifier.weight(1f), enabled = !state.busy && !conflict && !hourInvalid
                )
                PrimaryButton(
                    "Salvar",
                    onClick = { if (titleMissing) tried = true else onSave(draft.toSave(task.id)) },
                    modifier = Modifier.weight(1f), enabled = changed && !conflict && !hourInvalid, loading = state.busy
                )
            }
        }
    ) {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = 4.dp, bottom = 24.dp)) {
            DocumentLine(task, onOpenDocument)
            SectionBody(spacing = 18.dp) {
                LabeledField(
                    "Título", form.title, { v -> tried = false; edit { it.copy(title = v) } },
                    placeholder = "O que precisa ser feito",
                    error = if (tried && titleMissing) "Informe o título." else "",
                    singleLine = false,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
                )
                if (shownActions.isNotEmpty()) ChoiceField("Ação", shownActions, form.action, { v -> edit { it.copy(action = v) } })
                ChoiceField("Prioridade", priorities.ifEmpty { DefaultPriorities }, form.priority, { v -> edit { it.copy(priority = v) } })
                DateField("Prazo", form.dueDate, { v -> edit { it.copy(dueDate = v) } }, emptyText = "Sem prazo")
            }
            PlanningFields(
                form.dueDate, form.scheduledStart, form.scheduledEnd, form.estimateMinutes,
                startHour, endHour, daysText,
                onStartHour = { startHour = it }, onEndHour = { endHour = it }, onDaysText = { daysText = it },
                onChange = { st, en, min -> edit { it.copy(scheduledStart = st, scheduledEnd = en, estimateMinutes = min) } }
            )
            TaskRules.legacyEstimateNote(form.estimateMinutes).takeIf { it.isNotEmpty() }?.let {
                SectionBody { Text(it, style = MaterialTheme.typography.bodySmall, color = c.muted) }
            }
            SectionHeader("Tarefa pai")
            val parentTitle = relations.tasks.firstOrNull { it.id == form.parentTaskId }?.title
            SectionBody(spacing = 4.dp) {
                Text(if (form.parentTaskId == null) "Sem tarefa pai" else parentTitle ?: "Tarefa pai #${form.parentTaskId}")
                if (relations.loading) Text("Carregando tarefas…")
                relations.error?.let { Notice(it, tone = Tone.ERROR) }
            }
            SectionAction("Escolher tarefa pai…", { parentPicker = true }, enabled = !state.busy && !relations.loading && relations.error == null)
            ReminderChips(
                form.reminderOffsetsMinutes, { v -> edit { it.copy(reminderOffsetsMinutes = v) } },
                remindersBlocked, onRemindersChosen
            )
            // "Eu" and the suggestions only fill the typed form: the draft rules above stay as they are.
            SectionBody(Modifier.padding(top = 12.dp)) {
                AssigneeField(form.assignee, { v -> edit { it.copy(assignee = v) } }, assignees, myName)
            }
            // Part of the draft like the other fields: saved with Salvar, refused with the rest on a conflict.
            CategoryField(form.categories, { v -> edit { it.copy(categories = v) } }, onCreateCategory, enabled = !state.busy)
            // Part of the draft too: "Sem projeto" and the projects that are not archived (plus this task's own).
            SectionBody(spacing = 18.dp) {
                ProjectField(form.projectUid, projects, task.projectName, { v -> edit { it.copy(projectUid = v) } })
                LabeledField(
                    "Observações", form.notes, { v -> edit { it.copy(notes = v) } },
                    singleLine = false, minLines = 4,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
                )
            }
            val prerequisites = TaskRules.prerequisites(form.dependencies, relations.tasks)
            SectionHeader("Depende de")
            if (form.dependencies.isNotEmpty()) {
                prerequisites.forEach { prerequisite ->
                    FlatRow({ if (changed) { pendingSubtask = prerequisite.id; asking = true } else onOpenTask(prerequisite.id) }) {
                        Text("${TaskRules.prerequisiteSymbol(prerequisite)} ${prerequisite.title}", style = MaterialTheme.typography.bodyLarge, color = c.text)
                    }
                }
            }
            SectionAction("Escolher tarefas…", { dependencyPicker = true }, enabled = !state.busy && !relations.loading && relations.error == null)
            val subtasks = relations.tasks.filter { it.parentTaskId == task.id }
            if (subtasks.isNotEmpty()) {
                SectionHeader("Subtarefas · ${subtasks.count { it.done }} de ${subtasks.size} concluídas")
                subtasks.forEach { child ->
                    FlatRow({ if (changed) { pendingSubtask = child.id; asking = true } else onOpenTask(child.id) }) {
                        Text(
                            child.title + if (child.done) " · Concluída" else "", style = MaterialTheme.typography.bodyLarge,
                            color = if (child.done) c.muted else c.text
                        )
                    }
                }
            }
            if (task.checklist.isNotEmpty()) Checklist(task.checklist)
            DocumentActions(linked, enabled = !changed && !state.busy, changed = changed, onLink = onLink)
        }
    }

    if (parentPicker) {
        ParentTaskPicker(relations.tasks, task.id, onDismiss = { parentPicker = false }) { id ->
            edit { it.copy(parentTaskId = id) }
            parentPicker = false
        }
    }
    if (dependencyPicker) {
        DependencyPicker(relations.tasks, task.id, form.dependencies, onDismiss = { dependencyPicker = false }) { ids ->
            edit { it.copy(dependencies = ids) }
            dependencyPicker = false
        }
    }
    if (asking) {
        AlertDialog(
            onDismissRequest = { asking = false; pendingSubtask = null },
            title = { Text("Descartar as alterações?") },
            text = { Text("O que você mudou nesta tarefa ainda não foi salvo.") },
            confirmButton = { TextButton(onClick = { asking = false; val id = pendingSubtask; pendingSubtask = null; if (id != null) onOpenTask(id) else onBack() }) { Text("Descartar", color = c.error) } },
            dismissButton = { TextButton(onClick = { asking = false; pendingSubtask = null }) { Text("Continuar editando") } }
        )
    }
    if (unlinking) {
        AlertDialog(
            onDismissRequest = { unlinking = false },
            title = { Text("Desvincular do documento?") },
            text = { Text("A tarefa continua em Tarefas, sem documento.") },
            confirmButton = { TextButton(onClick = { unlinking = false; onUnlink() }) { Text("Desvincular", color = c.error) } },
            dismissButton = { TextButton(onClick = { unlinking = false }) { Text("Cancelar") } }
        )
    }
}

/** Where the task belongs: the document (tap to open its ficha) or "Sem documento". */
@Composable
private fun DocumentLine(task: TaskDetail, onOpenDocument: (Long) -> Unit) {
    val c = Ponte.colors
    if (task.documentId <= 0) {
        Text(
            "Sem documento", Modifier.padding(horizontal = ROW_MARGIN, vertical = 8.dp),
            style = MaterialTheme.typography.bodyMedium, color = c.muted
        )
        return
    }
    LinkRow(Icons.Outlined.Description, "Documento", task.documentLine.ifEmpty { "Documento" }, { onOpenDocument(task.documentId) })
}

/** The checklist, to read only: it is edited on the computer. */
@Composable
private fun Checklist(items: List<ChecklistItem>) {
    val c = Ponte.colors
    SectionHeader("Lista de verificação · ${items.count { it.done }} de ${items.size}")
    SectionBody(spacing = 8.dp) {
        for (i in items) {
            Row(verticalAlignment = Alignment.Top) {
                if (i.done) {
                    Box(Modifier.padding(top = 1.dp).size(20.dp).background(c.success, CircleShape), contentAlignment = Alignment.Center) {
                        Icon(Icons.Rounded.Check, contentDescription = "Feito", tint = c.surface, modifier = Modifier.size(14.dp))
                    }
                } else {
                    Box(Modifier.padding(top = 1.dp).size(20.dp).border(2.dp, c.muted, CircleShape))
                }
                Spacer(Modifier.width(12.dp))
                Text(
                    i.text, fontSize = 15.sp, lineHeight = 21.sp,
                    color = if (i.done) c.muted else c.text,
                    textDecoration = if (i.done) TextDecoration.LineThrough else null
                )
            }
        }
        Text("Para mudar a lista, use o Ponte no computador.", style = MaterialTheme.typography.bodySmall, color = c.muted)
    }
}

/** Link or change the document. Waits for the changes to be saved first. (Unlinking is in the "⋮" menu.) */
@Composable
private fun DocumentActions(linked: Boolean, enabled: Boolean, changed: Boolean, onLink: () -> Unit) {
    val c = Ponte.colors
    SectionHeader("Documento da tarefa")
    SectionAction(if (linked) "Trocar de documento…" else "Vincular a um documento…", onLink, enabled = enabled)
    if (changed) {
        Text(
            "Salve as alterações antes de trocar o documento.", Modifier.padding(horizontal = ROW_MARGIN, vertical = 4.dp),
            style = MaterialTheme.typography.bodySmall, color = c.muted
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ParentTaskPicker(tasks: List<TaskItem>, taskId: Long, onDismiss: () -> Unit, onPick: (Long?) -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    val candidates = parentCandidates(tasks, taskId, query)
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Ponte.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Escolher tarefa pai", style = MaterialTheme.typography.titleMedium)
            SearchBox(query, { query = it }, "Buscar tarefa")
            TextAction("Sem tarefa pai", { onPick(null) })
            Column(Modifier.heightIn(max = 340.dp).verticalScroll(rememberScrollState())) {
                if (candidates.isEmpty()) Text("Nenhuma tarefa encontrada", Modifier.padding(16.dp))
                candidates.forEach { candidate ->
                    Text(
                        candidate.title, Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { onPick(candidate.id) }
                            .padding(horizontal = 12.dp, vertical = 12.dp),
                        style = MaterialTheme.typography.bodyLarge, color = Ponte.colors.text
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DependencyPicker(tasks: List<TaskItem>, taskId: Long, selected: List<Long>, onDismiss: () -> Unit, onPick: (List<Long>) -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    var chosen by rememberSaveable { mutableStateOf(selected) }
    val candidates = dependencyCandidates(tasks, taskId, query)
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Ponte.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Escolher tarefas", style = MaterialTheme.typography.titleMedium)
            SearchBox(query, { query = it }, "Buscar tarefa")
            Column(Modifier.heightIn(max = 340.dp).verticalScroll(rememberScrollState())) {
                if (candidates.isEmpty()) Text("Nenhuma tarefa encontrada", Modifier.padding(16.dp))
                candidates.forEach { candidate ->
                    val toggle = { chosen = if (candidate.id in chosen) chosen - candidate.id else chosen + candidate.id }
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(onClick = toggle),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Checkbox(checked = candidate.id in chosen, onCheckedChange = { toggle() })
                        Text(
                            candidate.title + if (candidate.done) " · Concluída" else "", style = MaterialTheme.typography.bodyLarge,
                            modifier = Modifier.weight(1f), color = if (candidate.done) Ponte.colors.muted else Ponte.colors.text
                        )
                    }
                }
            }
            TextAction("Confirmar", { onPick(chosen) })
        }
    }
}
