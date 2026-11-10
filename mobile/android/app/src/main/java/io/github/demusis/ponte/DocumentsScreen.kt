package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AttachFile
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.PhotoCamera
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FloatingActionButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.text.Normalizer

private fun fold(s: String): String =
    Normalizer.normalize(s, Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "").lowercase()

/** The search of the Documentos tab and of the document picker: number, title, origin, sender. */
internal fun matchesQuery(d: Doc, query: String): Boolean {
    val q = fold(query.trim())
    return q.isEmpty() || fold("${d.number} ${d.title} ${d.origin} ${d.emailFrom}").contains(q)
}

/** The Documentos area (inside MainShell): search, filters, the list and the photo button. */
@Composable
internal fun DocumentsScreen(
    docs: List<Doc>,
    query: String,
    onQuery: (String) -> Unit,
    filter: DocFilter,
    onFilter: (DocFilter) -> Unit,
    onOpen: (Long) -> Unit,
    onPhoto: () -> Unit
) {
    val c = Ponte.colors
    val shown = remember(docs, query, filter) {
        docs.filter { d ->
            val byFilter = when (filter) {
                DocFilter.ALL -> true
                DocFilter.WITH_TASK -> d.openTasks > 0
                DocFilter.EMAILS -> d.email
                DocFilter.RECORDS -> d.manual
            }
            byFilter && matchesQuery(d, query)
        }
    }
    Box(Modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(bottom = 104.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            item(key = "search") {
                OutlinedTextField(
                    value = query, onValueChange = onQuery,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(top = 4.dp),
                    singleLine = true,
                    textStyle = MaterialTheme.typography.bodyLarge,
                    placeholder = { Text("Buscar por número, título ou origem", color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null, tint = c.secondary) },
                    trailingIcon = if (query.isEmpty()) null else ({
                        IconButton(onClick = { onQuery("") }) {
                            Icon(Icons.Outlined.Close, contentDescription = "Limpar a busca", tint = c.secondary)
                        }
                    }),
                    shape = RoundedCornerShape(28.dp),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedContainerColor = c.surface, unfocusedContainerColor = c.surface,
                        focusedBorderColor = c.accent, unfocusedBorderColor = c.outline,
                        focusedTextColor = c.text, unfocusedTextColor = c.text, cursorColor = c.accent
                    )
                )
            }
            item(key = "filters") {
                FilterChips(
                    DocFilter.values().map { it.name to it.label }, filter.name,
                    { k -> onFilter(DocFilter.valueOf(k)) },
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 2.dp)
                )
            }
            if (docs.isEmpty()) {
                item(key = "none") {
                    EmptyState(
                        "Nenhum documento neste aparelho.",
                        text = "Toque em sincronizar para trazer o acervo do computador.",
                        icon = Icons.Outlined.Description
                    )
                }
            } else if (shown.isEmpty()) {
                item(key = "nomatch") {
                    EmptyState(
                        "Nada encontrado.",
                        text = "Nenhum documento combina com a busca e o filtro escolhidos.",
                        icon = Icons.Outlined.SearchOff,
                        action = {
                            TextButton(onClick = { onQuery(""); onFilter(DocFilter.ALL) }) {
                                Text("Limpar busca e filtros", color = c.accent, fontWeight = FontWeight.SemiBold)
                            }
                        }
                    )
                }
            } else {
                items(shown, key = { it.id }) { d ->
                    DocCard(d, { onOpen(d.id) }, Modifier.padding(horizontal = 16.dp))
                }
            }
        }
        ExtendedFloatingActionButton(
            onClick = onPhoto,
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
            containerColor = c.accent, contentColor = c.onAccent,
            elevation = FloatingActionButtonDefaults.elevation(2.dp, 2.dp, 2.dp, 2.dp),
            icon = { Icon(Icons.Outlined.PhotoCamera, contentDescription = null) },
            text = { Text("Registro por foto", fontSize = 15.sp, fontWeight = FontWeight.SemiBold) }
        )
    }
}

@Composable
internal fun DocCard(d: Doc,onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    PonteCard(modifier, onClick) {
        Column(Modifier.padding(start = 16.dp, top = 12.dp, end = 16.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (d.type.isNotEmpty()) Tag(d.type, Tone.ACCENT, Modifier.widthIn(max = 200.dp))
                Spacer(Modifier.weight(1f))
                if (d.date.isNotEmpty()) Text(d.date, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1)
            }
            Text(
                d.title.ifEmpty { "Sem título" },
                fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium, color = c.text,
                maxLines = 2, overflow = TextOverflow.Ellipsis
            )
            val sub = if (d.email && d.emailFrom.isNotEmpty()) d.emailFrom else d.number.ifEmpty { d.origin }
            if (sub.isNotEmpty()) {
                Text(sub, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            if (d.openTasks > 0 || d.attachments > 0) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (d.openTasks > 0) {
                        val n = PhoneWords.plural(d.openTasks, "tarefa", "tarefas")
                        val label = if (d.taskDueLabel.isNotEmpty()) "$n · ${d.taskDueLabel}" else n
                        Pill(label, if (d.taskOverdue) Tone.ERROR else if (d.taskDueToday) Tone.WARNING else Tone.NEUTRAL, Modifier.widthIn(max = 260.dp))
                    }
                    Spacer(Modifier.weight(1f))
                    if (d.attachments > 0) {
                        Icon(Icons.Outlined.AttachFile, contentDescription = "Anexos", tint = c.muted, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(2.dp))
                        Text("${d.attachments}", style = MaterialTheme.typography.bodySmall, color = c.muted)
                    }
                }
            }
            d.noteError?.let {
                Row(verticalAlignment = Alignment.Top) {
                    Icon(Icons.Outlined.WarningAmber, contentDescription = null, tint = c.warning, modifier = Modifier.size(16.dp).padding(top = 2.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(it, style = MaterialTheme.typography.bodySmall, color = c.warning, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
