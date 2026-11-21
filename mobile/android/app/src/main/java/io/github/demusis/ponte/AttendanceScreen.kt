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
internal const val ATTENDANCE_READ_ONLY = "Para alterar os dados do atendimento, use o Ponte no computador. Cobranças se registram em Cobranças de hoje."

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
            state.error != null && state.id == routeId -> Notice(state.error, tone = Tone.ERROR, modifier = Modifier.padding(16.dp))
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
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = 4.dp, bottom = 32.dp)) {
        FichaHeader(
            d.reference,
            lines = if (d.subject.isNotBlank() && d.subject != d.reference) listOf(d.subject) else emptyList(),
            extra = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    if (d.statusLabel.isNotBlank()) Pill(d.statusLabel, tone = if (d.open) Tone.ACCENT else Tone.NEUTRAL, modifier = Modifier.weight(1f, fill = false))
                    if (d.open && d.dueLabel.isNotBlank()) {
                        Pill(
                            when {
                                d.late -> "Vencido em ${d.dueLabel}"
                                d.dueToday -> "Vence hoje"
                                else -> "Prazo ${d.dueLabel}"
                            }, tone = if (d.late) Tone.ERROR else if (d.dueToday) Tone.WARNING else Tone.NEUTRAL)
                    }
                }
            }
        )
        SectionBody { Notice(ATTENDANCE_READ_ONLY, tone = Tone.NEUTRAL) }

        if (d.reviewFlags.isNotEmpty()) {
            SectionBody {
                Notice(
                    "Alertas de conferência (${d.reviewFlags.size})\n" + d.reviewFlags.joinToString("\n") { "• $it" }, tone = Tone.WARNING)
            }
        }

        if (d.fields.isNotEmpty() || d.flags.isNotEmpty()) {
            FlatSection("Dados") {
                for (f in d.fields) FactRow(f.label, f.value)
                if (d.flags.isNotEmpty()) {
                    Row(Modifier.padding(horizontal = ROW_MARGIN, vertical = 6.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        for (flag in d.flags) Tag(flag, tone = Tone.NEUTRAL)
                    }
                }
            }
        }

        if (d.documentId > 0) {
            FlatSection(null) {
                LinkRow(Icons.Outlined.Description, "Documento ligado", d.documentLine.ifEmpty { "Documento" }, { onOpenDocument(d.documentId) })
            }
        }

        if (d.tasks.isNotEmpty()) {
            FlatSection("Tarefas deste atendimento · ${d.tasks.size}") {
                for (t in d.tasks) {
                    FlatRow({ onOpenTask(t.id) }) {
                        Column {
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
            FlatSection("Anexos · ${d.attachments.size}") {
                for (f in d.attachments) FileRow(f) { onOpenFile(f) }
            }
        }

        if (d.events.isNotEmpty()) {
            FlatSection("Linha do tempo", divider = false) {
                d.events.forEachIndexed { i, e ->
                    FlatRow(null, divider = i < d.events.lastIndex) {
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
