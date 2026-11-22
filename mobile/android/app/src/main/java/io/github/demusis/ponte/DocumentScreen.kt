package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.AttachFile
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.PictureAsPdf
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** The ficha of a document: heading, analysis, tasks, note, files and the way to its text. */
@Composable
internal fun DocumentScreen(
    state: DocumentState,
    syncing: Boolean,
    onBack: () -> Unit,
    onSync: () -> Unit,
    onToggleTask: (TaskItem) -> Unit,
    onNewTask: () -> Unit,
    onEditNote: () -> Unit,
    onOpenFile: (FileItem) -> Unit,
    onReadText: () -> Unit,
    onOpenTask: (TaskItem) -> Unit = {},
    /** A file or a photo is being read or attached: the button waits. */
    attachBusy: Boolean = false,
    /** Take a picture to attach; the argument is the description typed (may be empty). */
    onAttachPhoto: (String) -> Unit = {},
    /** Pick a file to attach; the argument is the description typed (may be empty). */
    onAttachFile: (String) -> Unit = {},
    /** The categories chosen for the document (saved at once, with the version of the ficha). */
    onCategories: (List<String>) -> Unit = {},
    onCreateCategory: (String, Int, (Category?, String) -> Unit) -> Unit = { _, _, done -> done(null, "") },
    /** The number may still change on the next sync (a record of this phone not sent yet). */
    provisional: Boolean = false
) {
    val c = Ponte.colors
    val doc = state.doc
    DetailScaffold(
        title = doc?.number?.takeIf { it.isNotBlank() } ?: "Documento",
        onBack = onBack,
        menu = { dismiss ->
            DropdownMenuItem(
                text = { Text(if (syncing) "Sincronizando…" else "Sincronizar agora", fontSize = 16.sp) },
                enabled = !syncing,
                onClick = { dismiss(); onSync() },
                leadingIcon = { Icon(Icons.Outlined.Sync, contentDescription = null) }
            )
        }
    ) {
        Column(Modifier.fillMaxSize()) {
            if (syncing) {
                LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.accent, trackColor = c.chip)
            }
            when {
                state.loading -> LoadingBox()
                state.error != null -> Notice(state.error, modifier = Modifier.padding(16.dp))
                doc != null -> FichaBody(
                    doc, onToggleTask, onNewTask, onEditNote, onOpenFile, onReadText, onOpenTask,
                    attachBusy, onAttachPhoto, onAttachFile, onCategories, onCreateCategory, provisional
                )
            }
        }
    }
}

@Composable
private fun FichaBody(
    doc: DocDetail,
    onToggleTask: (TaskItem) -> Unit,
    onNewTask: () -> Unit,
    onEditNote: () -> Unit,
    onOpenFile: (FileItem) -> Unit,
    onReadText: () -> Unit,
    onOpenTask: (TaskItem) -> Unit,
    attachBusy: Boolean,
    onAttachPhoto: (String) -> Unit,
    onAttachFile: (String) -> Unit,
    onCategories: (List<String>) -> Unit,
    onCreateCategory: (String, Int, (Category?, String) -> Unit) -> Unit,
    provisional: Boolean = false
) {
    val c = Ponte.colors
    var chooser by rememberSaveable { mutableStateOf(false) }
    var description by rememberSaveable { mutableStateOf("") }
    if (chooser) {
        AlertDialog(
            onDismissRequest = { chooser = false },
            title = { Text("Anexar ao documento") },
            text = {
                AttachChooser(
                    description, { description = it },
                    onPhoto = { chooser = false; onAttachPhoto(description); description = "" },
                    onFile = { chooser = false; onAttachFile(description); description = "" }
                )
            },
            confirmButton = {},
            dismissButton = { TextButton(onClick = { chooser = false }) { Text("Cancelar") } }
        )
    }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = 4.dp, bottom = 32.dp)) {
        if (provisional) {
            // Text with meaning, not colour alone: the number says it may change, and why it is worth a look.
            Column(
                Modifier.fillMaxWidth().padding(horizontal = ROW_MARGIN, vertical = 4.dp)
                    .semantics(mergeDescendants = true) {},
                verticalArrangement = Arrangement.spacedBy(2.dp)
            ) {
                Text(
                    RenumberRules.headerLine(doc.number, true), style = MaterialTheme.typography.labelMedium,
                    color = c.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                Text(RenumberRules.PROVISIONAL_NOTE, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
            }
        }
        // Header: kind and date as plain text over the title
        FichaHeader(
            doc.title,
            overline = listOf(doc.type, doc.date).filter { it.isNotBlank() }.joinToString(" · "),
            lines = listOf(doc.origin)
        )

        // Categories: the dots and the way to change them
        CategoryField(doc.categories, onCategories, onCreateCategory)
        RowDivider(0.dp)

        // Tasks: open first
        val tasks = doc.tasks.sortedBy { it.done }
        FlatSection("Tarefas") {
            if (tasks.isEmpty()) {
                Text(
                    "Nenhuma tarefa neste documento.", Modifier.padding(horizontal = ROW_MARGIN, vertical = 6.dp),
                    style = MaterialTheme.typography.bodyMedium, color = c.muted
                )
            }
            for (t in tasks) TaskListRow(t, { onToggleTask(t) }, { onOpenTask(t) })
            SectionAction("Nova tarefa", onNewTask, icon = Icons.Outlined.Add)
        }

        // Note
        FlatSection("Nota") {
            SectionBody {
                if (doc.note.isBlank()) {
                    Text("Nenhuma nota neste documento.", style = MaterialTheme.typography.bodyMedium, color = c.muted)
                } else {
                    Text(doc.note.trim(), fontSize = 16.sp, lineHeight = 23.sp, color = c.text)
                }
            }
            SectionAction(if (doc.note.isBlank()) "Escrever nota" else "Editar nota", onEditNote, icon = Icons.Outlined.Edit)
        }

        // Analysis
        val an = doc.analysis
        if (an != null && (an.summary.isNotBlank() || an.priority.isNotBlank() || an.deadline.isNotBlank() ||
                an.reason.isNotBlank() || an.suggested.isNotEmpty())
        ) {
            FlatSection("Resumo da análise") {
                SectionBody(spacing = 10.dp) {
                    if (an.summary.isNotBlank()) {
                        Text(an.summary, fontSize = 16.sp, lineHeight = 23.sp, color = c.text)
                    }
                    if (an.priority.isNotBlank() || an.deadline.isNotBlank()) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            if (an.priority.isNotBlank()) {
                                val tone = when (an.priority.trim().lowercase()) {
                                    "alta" -> Tone.ERROR
                                    "média", "media" -> Tone.WARNING
                                    else -> Tone.NEUTRAL
                                }
                                Pill("Prioridade ${an.priority.trim().lowercase()}", tone = tone)
                            }
                            if (an.deadline.isNotBlank()) {
                                Text(
                                    "Prazo: ${an.deadline}", Modifier.weight(1f, fill = false),
                                    style = MaterialTheme.typography.bodySmall, color = c.secondary
                                )
                            }
                        }
                    }
                    if (an.reason.isNotBlank()) {
                        Text(an.reason, style = MaterialTheme.typography.bodyMedium, color = c.secondary)
                    }
                    if (an.suggested.isNotEmpty()) {
                        Text("Providências sugeridas", style = MaterialTheme.typography.labelMedium, color = c.muted)
                        for (s in an.suggested) {
                            Row {
                                Text("•", Modifier.width(16.dp), color = c.muted)
                                Text(
                                    if (s.reason.isBlank()) s.action else "${s.action} — ${s.reason}",
                                    style = MaterialTheme.typography.bodyMedium, color = c.text
                                )
                            }
                        }
                    }
                }
            }
        }

        // Files: always shown, because it is where a file is attached
        FlatSection("Arquivos e anexos") {
            if (doc.files.isEmpty()) {
                Text(
                    "Nenhum arquivo neste documento.", Modifier.padding(horizontal = ROW_MARGIN, vertical = 6.dp),
                    style = MaterialTheme.typography.bodyMedium, color = c.muted
                )
            }
            for (f in doc.files) FileRow(f) { onOpenFile(f) }
            if (attachBusy) {
                Row(
                    Modifier.padding(horizontal = ROW_MARGIN, vertical = 6.dp).heightIn(min = 48.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    CircularProgressIndicator(Modifier.size(20.dp), color = c.muted, strokeWidth = 2.dp)
                    Spacer(Modifier.width(12.dp))
                    Text("Anexando o arquivo…", style = MaterialTheme.typography.bodyMedium, color = c.muted)
                }
            } else {
                SectionAction("Anexar…", { chooser = true }, icon = Icons.Outlined.AttachFile)
            }
        }

        // Text preview
        if (doc.text.isNotBlank()) {
            FlatSection("Texto", divider = false) {
                SectionBody {
                    Text(
                        doc.text.trim(),
                        style = MaterialTheme.typography.bodyMedium, lineHeight = 21.sp, color = c.secondary,
                        maxLines = 6, overflow = TextOverflow.Ellipsis
                    )
                }
                SectionAction("Ler o texto completo", onReadText)
            }
        }
    }
}

/**
 * What the "Anexar…" choice asks: how big a file may be, an optional description, and the two ways
 * to bring a file ([onPhoto], [onFile]). Alone so it can be drawn without a dialog window.
 */
@Composable
internal fun AttachChooser(
    description: String, onDescription: (String) -> Unit, onPhoto: () -> Unit, onFile: () -> Unit
) {
    val c = Ponte.colors
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Text(AttachLimits.upFront(), style = MaterialTheme.typography.bodyMedium, color = c.secondary)
        LabeledField(
            "Descrição (opcional)", description, { onDescription(ShareRules.capChars(it, AttachLimits.maxDescriptionChars)) },
            placeholder = "Do que se trata este arquivo"
        )
        PrimaryButton("Tirar foto", onPhoto)
        SecondaryButton("Escolher arquivo", onFile)
    }
}

private fun fileIcon(f: FileItem): ImageVector {
    val n = f.name.lowercase()
    return when {
        f.kind == "pdf" || n.endsWith(".pdf") -> Icons.Outlined.PictureAsPdf
        listOf(".jpg", ".jpeg", ".png", ".webp", ".gif", ".heic").any { n.endsWith(it) } -> Icons.Outlined.Image
        else -> Icons.Outlined.AttachFile
    }
}

@Composable
internal fun FileRow(f: FileItem, onClick: () -> Unit) {
    val c = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(onClickLabel = "Abrir arquivo", onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(fileIcon(f), contentDescription = null, tint = c.secondary, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(f.name, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val sub = listOf(formatSize(f.size), f.description).filter { it.isNotBlank() }.joinToString(" · ")
            Text(sub, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}
