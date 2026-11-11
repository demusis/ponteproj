package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.TextSnippet
import androidx.compose.material.icons.outlined.AttachFile
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.PictureAsPdf
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * "Guardar no Ponte": what another app shared, and what to do with it. Three steps on one screen
 * ([ShareState.mode]): choose (new record, or attach to a document), the record's few fields,
 * or the chosen document with an optional description.
 */
@Composable
internal fun ShareScreen(
    state: ShareState,
    onBack: () -> Unit,
    onCancel: () -> Unit,
    onRecord: () -> Unit,
    onAttach: () -> Unit,
    onTitle: (String) -> Unit,
    onKind: (String) -> Unit,
    onDate: (String) -> Unit,
    onOrigin: (String) -> Unit,
    onNotes: (String) -> Unit,
    onSaveRecord: () -> Unit,
    onChangeTarget: () -> Unit,
    onDescription: (String) -> Unit,
    onSaveAttach: () -> Unit
) {
    val c = Ponte.colors
    val item = state.item
    val mode = if (item == null) ShareMode.CHOOSE else state.mode
    val footer: (@Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit)? =
        if (mode == ShareMode.CHOOSE) null else ({
            if (state.error.isNotEmpty()) {
                Notice(state.error)
                Spacer(Modifier.height(10.dp))
            }
            if (mode == ShareMode.RECORD) PrimaryButton("Salvar registro", onSaveRecord, loading = state.busy)
            else PrimaryButton("Anexar", onSaveAttach, loading = state.busy, enabled = state.target != null)
            if (state.busy && state.progress.isNotEmpty()) {
                Spacer(Modifier.height(8.dp))
                Text(
                    state.progress, Modifier.fillMaxWidth(), color = c.muted,
                    style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Center
                )
            }
        })
    DetailScaffold(title = "Guardar no Ponte", onBack = onBack, close = mode == ShareMode.CHOOSE, footer = footer) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState())
                .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            when {
                state.reading -> {
                    ReceivedCard(null, reading = true)
                    Notice("Recebendo o arquivo…", Tone.NEUTRAL)
                }
                item == null -> {
                    // The copy failed: nothing arrived. The only way on is to leave.
                    if (state.error.isNotEmpty()) Notice(state.error)
                    SecondaryButton("Fechar", onCancel)
                }
                mode == ShareMode.CHOOSE -> {
                    ReceivedCard(item, reading = false)
                    if (state.error.isNotEmpty()) Notice(state.error)
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        PrimaryButton("Novo registro", onRecord, enabled = !state.busy)
                        Caption(
                            if (item.isFile) "Cria um registro novo no acervo, com este arquivo anexado."
                            else "Cria um registro novo no acervo, com este texto."
                        )
                    }
                    if (item.isFile) {
                        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            SecondaryButton("Anexar a um documento", onAttach, enabled = !state.busy)
                            Caption("Junta o arquivo a um documento que já existe.")
                        }
                    }
                    TextButton(onClick = onCancel, modifier = Modifier.fillMaxWidth().height(48.dp)) {
                        Text("Cancelar", color = c.secondary, fontWeight = FontWeight.SemiBold)
                    }
                }
                mode == ShareMode.RECORD -> {
                    ReceivedCard(item, reading = false, compact = true)
                    ChoiceField("Tipo", state.kinds, state.kind, onKind)
                    LabeledField("Título", state.title, onTitle, error = state.titleError, enabled = !state.busy)
                    DateField("Data", state.date, onDate, emptyText = "Sem data")
                    LabeledField(
                        "Origem", state.origin, onOrigin, placeholder = "Quem enviou ou de onde veio", enabled = !state.busy
                    )
                    if (item.text.isEmpty()) {
                        LabeledField(
                            "Observações", state.notes, onNotes, placeholder = "Opcional",
                            singleLine = false, minLines = 3, enabled = !state.busy
                        )
                    }
                }
                else -> {
                    ReceivedCard(item, reading = false, compact = true)
                    PonteCard {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            SectionLabel("Anexar a")
                            val doc = state.target
                            Text(
                                doc?.let { it.number.ifBlank { it.title } } ?: "Nenhum documento escolhido",
                                fontSize = 16.sp, fontWeight = FontWeight.Medium, color = c.text
                            )
                            if (doc != null && doc.number.isNotBlank() && doc.title.isNotBlank()) {
                                Text(
                                    doc.title, style = MaterialTheme.typography.bodyMedium, color = c.secondary,
                                    maxLines = 2, overflow = TextOverflow.Ellipsis
                                )
                            }
                            TextButton(onClick = onChangeTarget, enabled = !state.busy) {
                                Text("Escolher outro documento", color = c.accent, fontWeight = FontWeight.SemiBold)
                            }
                        }
                    }
                    LabeledField(
                        "Descrição (opcional)", state.description, onDescription,
                        placeholder = "Do que se trata este arquivo", enabled = !state.busy
                    )
                }
            }
        }
    }
}

@Composable
private fun Caption(text: String) {
    Text(
        text, Modifier.fillMaxWidth().padding(horizontal = 8.dp),
        style = MaterialTheme.typography.bodySmall, color = Ponte.colors.muted, textAlign = TextAlign.Center
    )
}

private fun iconFor(name: String): ImageVector {
    val n = name.lowercase()
    return when {
        n.endsWith(".pdf") -> Icons.Outlined.PictureAsPdf
        listOf(".jpg", ".jpeg", ".png", ".gif", ".bmp", ".tif", ".tiff").any { n.endsWith(it) } -> Icons.Outlined.Image
        n.endsWith(".txt") -> Icons.AutoMirrored.Outlined.TextSnippet
        else -> Icons.Outlined.AttachFile
    }
}

/** What arrived: the file's name and size, or the first lines of the text (plain text, never a link). */
@Composable
private fun ReceivedCard(item: ShareItem?, reading: Boolean, compact: Boolean = false) {
    val c = Ponte.colors
    PonteCard {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            SectionLabel("Recebido", Modifier.padding(start = 0.dp))
            when {
                reading || item == null -> Text("Um arquivo", fontSize = 16.sp, color = c.secondary)
                item.isFile -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(40.dp).background(c.chip, RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
                        Icon(iconFor(item.name), contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
                    }
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(
                            item.name, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text,
                            maxLines = 2, overflow = TextOverflow.Ellipsis
                        )
                        Text(formatSize(item.size), style = MaterialTheme.typography.bodySmall, color = c.muted)
                    }
                }
                else -> Row(verticalAlignment = Alignment.Top) {
                    Icon(Icons.Outlined.Description, contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
                    Spacer(Modifier.width(12.dp))
                    Text(
                        ShareRules.preview(item.text, if (compact) 3 else 6),
                        Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, color = c.text
                    )
                }
            }
            if (item != null && item.isFile && item.text.isNotEmpty() && !compact) {
                Text(
                    ShareRules.preview(item.text, 3), style = MaterialTheme.typography.bodyMedium, color = c.secondary
                )
            }
        }
    }
}
