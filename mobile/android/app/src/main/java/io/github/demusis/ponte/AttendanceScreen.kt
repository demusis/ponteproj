package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.clickable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** The note at the head of the ficha: nothing here can be changed on the phone. */
internal const val ATTENDANCE_READ_ONLY = "Para alterar um atendimento, use o Ponte no computador."

/**
 * One atendimento, to read. Every field the computer's ficha shows that is stored, the document it is
 * linked to (tap to open it), its tasks, attachments (opened by the phone's viewer, as the documents'
 * are) and timeline. Nothing is written from here.
 */
@Composable
internal fun AttendanceScreen(
    routeId: Long,
    state: AttendanceViewState,
    onBack: () -> Unit,
    onOpenDocument: (Long) -> Unit,
    onOpenTask: (Long) -> Unit,
    onOpenFile: (FileItem) -> Unit
) {
    // Only the atendimento this screen was opened for, never another one that is loaded.
    val d = state.detail?.takeIf { it.id == routeId }
    DetailScaffold(title = "Atendimento", onBack = onBack) {
        when {
            d != null -> AttendanceBody(d, onOpenDocument, onOpenTask, onOpenFile)
            state.error != null && state.id == routeId -> Notice(state.error, Tone.ERROR, Modifier.padding(16.dp))
            else -> LoadingBox()
        }
    }
}

@Composable
private fun AttendanceBody(
    d: AttendanceDetail,
    onOpenDocument: (Long) -> Unit,
    onOpenTask: (Long) -> Unit,
    onOpenFile: (FileItem) -> Unit
) {
    val c = Ponte.colors
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Column(Modifier.padding(horizontal = 4.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(d.reference, fontSize = 22.sp, lineHeight = 29.sp, fontWeight = FontWeight.SemiBold, color = c.text)
            if (d.subject.isNotBlank() && d.subject != d.reference) {
                Text(d.subject, style = MaterialTheme.typography.bodyLarge, color = c.secondary)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (d.statusLabel.isNotBlank()) Pill(d.statusLabel, if (d.open) Tone.ACCENT else Tone.NEUTRAL, Modifier.weight(1f, fill = false))
                if (d.open && d.dueLabel.isNotBlank()) {
                    Pill(
                        when {
                            d.late -> "Vencido em ${d.dueLabel}"
                            d.dueToday -> "Vence hoje"
                            else -> "Prazo ${d.dueLabel}"
                        },
                        if (d.late) Tone.ERROR else if (d.dueToday) Tone.WARNING else Tone.NEUTRAL
                    )
                }
            }
        }
        Notice(ATTENDANCE_READ_ONLY, Tone.NEUTRAL)

        if (d.reviewFlags.isNotEmpty()) {
            Notice(
                "Alertas de conferência (${d.reviewFlags.size})\n" + d.reviewFlags.joinToString("\n") { "• $it" },
                Tone.WARNING
            )
        }

        if (d.fields.isNotEmpty() || d.flags.isNotEmpty()) {
            PonteCard {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    SectionLabel("Dados", Modifier.padding(start = 0.dp))
                    for (f in d.fields) {
                        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text(f.label, style = FieldLabelStyle, color = c.secondary)
                            Text(f.value, fontSize = 16.sp, lineHeight = 22.sp, color = c.text)
                        }
                    }
                    if (d.flags.isNotEmpty()) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            for (flag in d.flags) Tag(flag, Tone.NEUTRAL)
                        }
                    }
                }
            }
        }

        if (d.documentId > 0) {
            PonteCard(onClick = { onOpenDocument(d.documentId) }) {
                Row(
                    Modifier.heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(Icons.Outlined.Description, contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text("Documento ligado", style = FieldLabelStyle, color = c.secondary)
                        Text(
                            d.documentLine.ifEmpty { "Documento" },
                            fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, color = c.text,
                            maxLines = 3, overflow = TextOverflow.Ellipsis
                        )
                    }
                    Text("Abrir", style = MaterialTheme.typography.labelLarge, color = c.accent, modifier = Modifier.padding(start = 8.dp))
                }
            }
        }

        if (d.tasks.isNotEmpty()) {
            PonteCard {
                Column(Modifier.padding(top = 14.dp, bottom = 6.dp)) {
                    SectionLabel("Tarefas deste atendimento · ${d.tasks.size}", Modifier.padding(start = 12.dp, bottom = 4.dp))
                    for (t in d.tasks) {
                        Column(
                            Modifier.fillMaxWidth().heightIn(min = 52.dp).clickable(onClickLabel = "Abrir a tarefa") { onOpenTask(t.id) }
                                .padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalArrangement = Arrangement.Center
                        ) {
                            Text(
                                t.title, fontSize = 15.sp, fontWeight = FontWeight.Medium,
                                color = if (t.done) c.muted else c.text,
                                textDecoration = if (t.done) TextDecoration.LineThrough else null,
                                maxLines = 2, overflow = TextOverflow.Ellipsis
                            )
                            val sub = listOf(if (t.done) "Concluída" else "", if (t.dueLabel.isNotEmpty()) "prazo ${t.dueLabel}" else "")
                                .filter { it.isNotEmpty() }.joinToString(" · ")
                            if (sub.isNotEmpty()) Text(sub, style = MaterialTheme.typography.bodySmall, color = c.muted)
                        }
                    }
                }
            }
        }

        if (d.attachments.isNotEmpty()) {
            PonteCard {
                Column(Modifier.padding(top = 14.dp, bottom = 6.dp)) {
                    SectionLabel("Anexos · ${d.attachments.size}", Modifier.padding(start = 12.dp, bottom = 4.dp))
                    for (f in d.attachments) FileRow(f) { onOpenFile(f) }
                }
            }
        }

        if (d.events.isNotEmpty()) {
            PonteCard {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    SectionLabel("Linha do tempo", Modifier.padding(start = 0.dp))
                    d.events.forEachIndexed { i, e ->
                        if (i > 0) HorizontalDivider(color = c.divider)
                        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text(
                                listOf(e.whenText, e.actor).filter { it.isNotBlank() }.joinToString(" · "),
                                style = MaterialTheme.typography.bodySmall, color = c.muted
                            )
                            Text(
                                if (e.detail.isBlank()) e.kind else "${e.kind}: ${e.detail}",
                                fontSize = 15.sp, lineHeight = 21.sp, color = c.text
                            )
                        }
                    }
                }
            }
        }
    }
}
