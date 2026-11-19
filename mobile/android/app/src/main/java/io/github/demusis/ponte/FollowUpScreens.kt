package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.LocalDate

/**
 * An atendimento of "Cobranças de hoje": why it is here (the core's words), and the two things to do
 * about it. Tapping the card opens the ficha. The deadline of the atendimento is shown as it is: nothing
 * here changes it.
 */
@Composable
internal fun FollowUpCard(
    a: AttendanceItem,
    onOpen: () -> Unit,
    onRegister: () -> Unit,
    onSchedule: () -> Unit
) {
    val c = Ponte.colors
    PonteCard(onClick = onOpen) {
        Column(Modifier.padding(start = 16.dp, top = 12.dp, end = 12.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    a.reference, Modifier.weight(1f), fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium,
                    color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                Pill(FollowUpRules.chip(a.followUpDays), tone = if (a.followUpDays > 0) Tone.ERROR else Tone.WARNING)
            }
            if (a.subject.isNotEmpty() && a.subject != a.reference) {
                Text(a.subject, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Text(
                a.followUp, style = MaterialTheme.typography.bodyMedium,
                color = if (a.followUpDays > 0) c.error else c.warning
            )
            val line = listOf(
                a.statusLabel, a.assignee, if (a.dueDate.isNotEmpty()) "Prazo ${formatDocDate(a.dueDate)}" else ""
            ).filter { it.isNotEmpty() }.joinToString(" · ")
            if (line.isNotEmpty()) {
                Text(line, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = onRegister, modifier = Modifier.weight(1f).height(44.dp), shape = RoundedCornerShape(22.dp),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 8.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = c.accent, contentColor = c.onAccent)
                ) { Text("Registrar cobrança", fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                OutlinedButton(
                    onClick = onSchedule, modifier = Modifier.weight(1f).height(44.dp), shape = RoundedCornerShape(22.dp),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 8.dp),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = c.accent)
                ) { Text("Próxima cobrança", fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            }
        }
    }
}

/**
 * The window of "Registrar cobrança" ([contact]: the way of contact, a note and, if the person wants,
 * the next date) or of "Próxima cobrança" (only the date). [onConfirm] gets the text of the contact
 * (empty without [contact]) and the next date AAAA-MM-DD (empty when none is wanted).
 */
@Composable
internal fun FollowUpDialog(
    target: AttendanceItem,
    contact: Boolean,
    channels: List<String>,
    onDismiss: () -> Unit,
    onConfirm: (String, String) -> Unit
) {
    val c = Ponte.colors
    val today = LocalDate.now()
    val ways = channels.ifEmpty { listOf(FollowUpRules.DEFAULT_CHANNEL) }
    var channel by rememberSaveable { mutableStateOf(ways.first()) }
    var note by rememberSaveable { mutableStateOf("") }
    var wantNext by rememberSaveable { mutableStateOf(true) }
    var next by rememberSaveable { mutableStateOf(FollowUpRules.dateIn(today, FollowUpRules.DEFAULT_NEXT_DAYS)) }
    val needsDate = !contact || wantNext
    val dateOk = !needsDate || FollowUpRules.nextDateOk(next, today)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (contact) "Registrar cobrança" else "Próxima cobrança") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(target.reference, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(
                    "Fica no histórico do atendimento. O prazo do atendimento não muda.",
                    style = MaterialTheme.typography.bodySmall, color = c.secondary
                )
                if (contact) {
                    ChoiceField("Contato", ways.map { Choice(it, it) }, channel, { channel = it })
                    LabeledField("Observação (opcional)", note, { note = it }, placeholder = "ex.: ficaram de responder até sexta")
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(
                            checked = wantNext, onCheckedChange = { wantNext = it },
                            colors = CheckboxDefaults.colors(checkedColor = c.accent)
                        )
                        Text("Marcar a próxima cobrança", style = MaterialTheme.typography.bodyLarge, color = c.text)
                    }
                }
                if (needsDate) {
                    DateField("Próxima cobrança em", next, { next = it }, Modifier.fillMaxWidth(), emptyText = "Escolha a data")
                    Row(horizontalArrangement = Arrangement.spacedBy(0.dp)) {
                        for ((label, days) in FollowUpRules.PRESETS) {
                            TextButton(onClick = { next = FollowUpRules.dateIn(today, days) }, contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 8.dp)) {
                                Text(label, fontSize = 13.sp, maxLines = 1, color = c.accent)
                            }
                        }
                    }
                    if (!dateOk) {
                        Text("Escolha uma data depois de hoje.", style = MaterialTheme.typography.bodySmall, color = c.error)
                    }
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = dateOk,
                onClick = { onConfirm(if (contact) FollowUpRules.contactText(channel, note) else "", if (needsDate) next else "") }
            ) { Text(if (contact) "Registrar" else "Marcar", color = if (dateOk) c.accent else c.muted, fontWeight = FontWeight.SemiBold) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancelar") } }
    )
}
