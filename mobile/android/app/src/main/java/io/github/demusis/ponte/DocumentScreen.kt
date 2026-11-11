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
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
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
    onOpenTask: (TaskItem) -> Unit = {}
) {
    val c = Ponte.colors
    val doc = state.doc
    DetailScaffold(
        title = doc?.number?.takeIf { it.isNotBlank() } ?: "Documento",
        onBack = onBack,
        actions = {
            IconButton(onClick = onSync, enabled = !syncing) {
                Icon(Icons.Outlined.Sync, contentDescription = "Sincronizar agora", tint = if (syncing) c.muted else c.text)
            }
        }
    ) {
        Column(Modifier.fillMaxSize()) {
            if (syncing) {
                LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.accent, trackColor = c.chip)
            }
            when {
                state.loading -> LoadingBox()
                state.error != null -> Notice(state.error, modifier = Modifier.padding(16.dp))
                doc != null -> FichaBody(doc, onToggleTask, onNewTask, onEditNote, onOpenFile, onReadText, onOpenTask)
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
    onOpenTask: (TaskItem) -> Unit
) {
    val c = Ponte.colors
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        // Header
        Column(Modifier.padding(horizontal = 4.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (doc.type.isNotBlank() || doc.date.isNotBlank()) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    if (doc.type.isNotBlank()) Tag(doc.type, modifier = Modifier.weight(1f, fill = false))
                    if (doc.date.isNotBlank()) Text(doc.date, style = MaterialTheme.typography.bodySmall, color = c.muted)
                }
            }
            if (doc.title.isNotBlank()) {
                Text(doc.title, fontSize = 22.sp, lineHeight = 29.sp, fontWeight = FontWeight.SemiBold, color = c.text)
            }
            if (doc.origin.isNotBlank()) {
                Text(doc.origin, style = MaterialTheme.typography.bodyMedium, color = c.secondary)
            }
        }

        // Tasks: open first
        val tasks = doc.tasks.sortedBy { it.done }
        PonteCard {
            Column(Modifier.padding(start = 8.dp, top = 14.dp, end = 12.dp, bottom = 8.dp)) {
                SectionLabel("Tarefas", Modifier.padding(start = 8.dp, bottom = 4.dp))
                if (tasks.isEmpty()) {
                    Text(
                        "Nenhuma tarefa neste documento.", Modifier.padding(start = 8.dp, top = 4.dp, bottom = 4.dp),
                        style = MaterialTheme.typography.bodyMedium, color = c.muted
                    )
                }
                for (t in tasks) {
                    TaskRow(t, t.actionLabel, onToggle = { onToggleTask(t) }, onOpen = { onOpenTask(t) })
                }
                CardButton("Nova tarefa", Icons.Outlined.Add, onNewTask)
            }
        }

        // Note
        PonteCard {
            Column(Modifier.padding(start = 16.dp, top = 14.dp, end = 8.dp, bottom = 8.dp)) {
                SectionLabel("Nota", Modifier.offset(x = (-4).dp).padding(bottom = 6.dp))
                if (doc.note.isBlank()) {
                    Text("Nenhuma nota neste documento.", Modifier.padding(end = 8.dp), style = MaterialTheme.typography.bodyMedium, color = c.muted)
                } else {
                    Text(doc.note.trim(), Modifier.padding(end = 8.dp), fontSize = 16.sp, lineHeight = 23.sp, color = c.text)
                }
                CardButton(
                    if (doc.note.isBlank()) "Escrever nota" else "Editar nota", Icons.Outlined.Edit, onEditNote,
                    Modifier.offset(x = (-8).dp).padding(top = 4.dp)
                )
            }
        }

        // Analysis
        val an = doc.analysis
        if (an != null && (an.summary.isNotBlank() || an.priority.isNotBlank() || an.deadline.isNotBlank() ||
                an.reason.isNotBlank() || an.suggested.isNotEmpty())
        ) {
            PonteCard {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    SectionLabel("Resumo da análise", Modifier.offset(x = (-4).dp))
                    if (an.summary.isNotBlank()) {
                        Text(an.summary, fontSize = 16.sp, lineHeight = 23.sp, color = c.text)
                    }
                    if (an.priority.isNotBlank() || an.deadline.isNotBlank()) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            if (an.priority.isNotBlank()) {
                                val tone = when (an.priority.trim().lowercase()) {
                                    "alta" -> Tone.ERROR
                                    "média", "media" -> Tone.WARNING
                                    else -> Tone.NEUTRAL
                                }
                                Pill("Prioridade ${an.priority.trim().lowercase()}", tone)
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
                        HorizontalDivider(color = c.divider)
                        Text("Providências sugeridas",style = MaterialTheme.typography.labelMedium, color = c.muted)
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

        // Files
        if (doc.files.isNotEmpty()) {
            PonteCard {
                Column(Modifier.padding(top = 14.dp, bottom = 6.dp)) {
                    SectionLabel("Arquivos e anexos", Modifier.padding(start = 12.dp, bottom = 4.dp))
                    for (f in doc.files) FileRow(f) { onOpenFile(f) }
                }
            }
        }

        // Text preview
        if (doc.text.isNotBlank()) {
            PonteCard {
                Column(Modifier.padding(start = 16.dp, top = 14.dp, end = 8.dp, bottom = 8.dp)) {
                    SectionLabel("Texto", Modifier.offset(x = (-4).dp).padding(bottom = 6.dp))
                    Text(
                        doc.text.trim(), Modifier.padding(end = 8.dp),
                        style = MaterialTheme.typography.bodyMedium, lineHeight = 21.sp, color = c.secondary,
                        maxLines = 6, overflow = TextOverflow.Ellipsis
                    )
                    CardButton("Ler o texto completo", null, onReadText, Modifier.offset(x = (-8).dp).padding(top = 4.dp))
                }
            }
        }
    }
}

/** Text button at the foot of a card, aligned with the card text. */
@Composable
private fun CardButton(text: String, icon: ImageVector?, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    Row(
        modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(8.dp)).clickable(onClick = onClick)
            .padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (icon != null) {
            Icon(icon, contentDescription = null, tint = c.accent, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
        }
        Text(text, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.accent)
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
        Box(Modifier.size(40.dp).background(c.chip, RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
            Icon(fileIcon(f), contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(f.name, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val sub = listOf(formatSize(f.size), f.description).filter { it.isNotBlank() }.joinToString(" · ")
            Text(sub, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}
