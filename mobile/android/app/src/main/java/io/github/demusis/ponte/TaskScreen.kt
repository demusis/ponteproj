package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
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
    onReload: () -> Unit
) {
    // Only the task this screen was opened for: never the form of one with the data of another.
    val task = TaskRules.shown(routeId, state)
    if (task == null) {
        DetailScaffold(title = "Tarefa", onBack = onBack, close = true) {
            val error = state.error.takeIf { state.id == routeId }
            if (error != null) Notice(error, Tone.ERROR, Modifier.padding(16.dp)) else LoadingBox()
        }
        return
    }
    TaskLoaded(state, task, actions, priorities, onBack, onSave, onFinish, onOpenDocument, onLink, onUnlink, onReload)
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
    onReload: () -> Unit
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
    var unlinking by rememberSaveable { mutableStateOf(false) }
    val changed = draft.changed
    val conflict = draft.conflict || state.conflict
    val message = if (draft.conflict) TaskRules.CHANGED else state.formError
    val linked = task.documentId > 0
    val leave = { if (changed) asking = true else onBack() }
    // A task without document has nothing to triage.
    val shownActions = if (!linked && form.action != "triar") actions.filter { it.key != "triar" } else actions
    val titleMissing = form.title.isBlank()

    SafeBackHandler(onBack = leave)

    DetailScaffold(
        title = "Tarefa", onBack = leave, close = true,
        footer = {
            if (message.isNotEmpty()) {
                Notice(message, Tone.ERROR, Modifier.padding(bottom = if (conflict) 4.dp else 12.dp))
            }
            if (conflict) {
                TextButton(onClick = onReload, modifier = Modifier.padding(bottom = 8.dp)) {
                    Text("Ver a versão atual", color = c.accent, fontWeight = FontWeight.SemiBold)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                SecondaryButton(
                    if (task.done) "Reabrir" else "Concluir",
                    onClick = { if (titleMissing) tried = true else onFinish(draft.toSave(task.id)) },
                    modifier = Modifier.weight(1f), enabled = !state.busy && !conflict
                )
                PrimaryButton(
                    "Salvar",
                    onClick = { if (titleMissing) tried = true else onSave(draft.toSave(task.id)) },
                    modifier = Modifier.weight(1f), enabled = changed && !conflict, loading = state.busy
                )
            }
        }
    ) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState())
                .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp)
        ) {
            DocumentLine(task, onOpenDocument)
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
            LabeledField(
                "Responsável", form.assignee, { v -> edit { it.copy(assignee = v) } },
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words)
            )
            LabeledField(
                "Observações", form.notes, { v -> edit { it.copy(notes = v) } },
                singleLine = false, minLines = 4,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
            )
            if (task.checklist.isNotEmpty()) Checklist(task.checklist)
            DocumentActions(linked, enabled = !changed && !state.busy, changed = changed, onLink = onLink, onUnlink = { unlinking = true })
        }
    }

    if (asking) {
        AlertDialog(
            onDismissRequest = { asking = false },
            title = { Text("Descartar as alterações?") },
            text = { Text("O que você mudou nesta tarefa ainda não foi salvo.") },
            confirmButton = { TextButton(onClick = { asking = false; onBack() }) { Text("Descartar", color = c.error) } },
            dismissButton = { TextButton(onClick = { asking = false }) { Text("Continuar editando") } }
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
        Text("Sem documento", style = MaterialTheme.typography.bodyMedium, color = c.muted)
        return
    }
    PonteCard(onClick = { onOpenDocument(task.documentId) }) {
        Row(Modifier.heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.Description, contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Text(
                task.documentLine.ifEmpty { "Documento" }, Modifier.weight(1f),
                fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, color = c.text,
                maxLines = 3, overflow = TextOverflow.Ellipsis
            )
            Text("Abrir", style = MaterialTheme.typography.labelLarge, color = c.accent, modifier = Modifier.padding(start = 8.dp))
        }
    }
}

/** The checklist, to read only: it is edited on the computer. */
@Composable
private fun Checklist(items: List<ChecklistItem>) {
    val c = Ponte.colors
    PonteCard {
        Column(Modifier.padding(start = 16.dp, top = 14.dp, end = 16.dp, bottom = 14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            SectionLabel("Lista de verificação · ${items.count { it.done }} de ${items.size}", Modifier.padding(start = 0.dp))
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
}

/** Link, change or remove the document. Waits for the changes to be saved first. */
@Composable
private fun DocumentActions(
    linked: Boolean, enabled: Boolean, changed: Boolean, onLink: () -> Unit, onUnlink: () -> Unit
) {
    val c = Ponte.colors
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        SectionLabel("Documento da tarefa")
        SecondaryButton(
            if (linked) "Trocar de documento…" else "Vincular a um documento…",
            onClick = onLink, enabled = enabled
        )
        if (linked) SecondaryButton("Desvincular do documento", onClick = onUnlink, enabled = enabled, danger = true)
        if (changed) {
            Text("Salve as alterações antes de trocar o documento.", style = MaterialTheme.typography.bodySmall, color = c.muted)
        }
    }
}
