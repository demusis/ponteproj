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

/** The Documentos area (inside MainShell): search, filters, the list and the photo button. */
@Composable
internal fun DocumentsScreen(
    docs: List<Doc>,
    query: String,
    onQuery: (String) -> Unit,
    filter: DocFilter,
    onFilter: (DocFilter) -> Unit,
    onOpen: (Long) -> Unit,
    onPhoto: () -> Unit,
    categoryFilter: Set<String> = emptySet(),
    onCategoryFilter: (Set<String>) -> Unit = {},
    /** Ids the core's search found for [query]; null while there is nothing typed. */
    matches: Set<Long>? = null
) {
    val c = Ponte.colors
    val shown = remember(docs, matches, filter, categoryFilter) {
        docs.filter { d ->
            val byFilter = when (filter) {
                DocFilter.ALL -> true
                DocFilter.WITH_TASK -> d.openTasks > 0
                DocFilter.EMAILS -> d.email
                DocFilter.RECORDS -> d.manual
            }
            byFilter && CategoryRules.matches(d.categories, categoryFilter) && (matches == null || d.id in matches)
        }
    }
    val categories = LocalCategories.current
    FabColumn(Icons.Outlined.PhotoCamera, "Registro por foto", onPhoto) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 8.dp)) {
            item(key = "search") {
                OutlinedTextField(
                    value = query, onValueChange = onQuery,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(top = 4.dp, bottom = 4.dp),
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
            // The category filter is behind the filter icon of the frame; while it is on, this line says which.
            if (categoryFilter.isNotEmpty()) {
                item(key = "filter-line") {
                    FilterSummaryRow(
                        FilterSummary.line(FilterSummary.categoryNames(categoryFilter, categories), ""),
                        onClear = { onCategoryFilter(emptySet()) }
                    )
                }
            }
            if (docs.isEmpty()) {
                item(key = "none") {
                    EmptyState(
                        "Nenhum documento neste aparelho.",
                        text = "Puxe a lista para baixo, ou use ⋮ › Sincronizar agora, para trazer o acervo do computador.",
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
                            TextButton(onClick = { onQuery(""); onFilter(DocFilter.ALL); onCategoryFilter(emptySet()) }) {
                                Text("Limpar busca e filtros", color = c.accent, fontWeight = FontWeight.SemiBold)
                            }
                        }
                    )
                }
            } else {
                items(shown, key = { it.id }) { d ->
                    DocRow(d, { onOpen(d.id) })
                }
            }
        }
    }
}

/** One document as a flat row (the list and the document picker). */
@Composable
internal fun DocRow(d: Doc, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    FlatRow(onClick, modifier) {
        Column(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (d.type.isNotEmpty()) Tag(d.type, tone = Tone.ACCENT, modifier = Modifier.widthIn(max = 200.dp))
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
                        Pill(label, tone = if (d.taskOverdue) Tone.ERROR else if (d.taskDueToday) Tone.WARNING else Tone.NEUTRAL, modifier = Modifier.widthIn(max = 260.dp))
                    }
                    Spacer(Modifier.weight(1f))
                    if (d.attachments > 0) {
                        Icon(Icons.Outlined.AttachFile, contentDescription = "Anexos", tint = c.muted, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(2.dp))
                        Text("${d.attachments}", style = MaterialTheme.typography.bodySmall, color = c.muted)
                    }
                }
            }
            CategoryChips(d.categories)
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
