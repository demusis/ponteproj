package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The search over the whole library: documents, tasks and atendimentos, grouped, at most eight of
 * each with "e mais N". A search starts 300 ms after the last keystroke, at two characters or more;
 * the controller keeps the text in memory only.
 */
@Composable
internal fun SearchScreen(
    state: SearchState,
    onQuery: (String) -> Unit,
    onBack: () -> Unit,
    onOpenDocument: (Long) -> Unit,
    onOpenTask: (Long) -> Unit,
    onOpenAttendance: (Long) -> Unit
) {
    val c = Ponte.colors
    val focus = remember { FocusRequester() }
    AutoFocus(focus)
    DetailScaffold(title = "Buscar", onBack = onBack) {
        Column(Modifier.fillMaxSize()) {
            SearchBox(
                state.query, onQuery, "Documentos, tarefas e atendimentos",
                Modifier.padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 8.dp), focus
            )
            if (state.loading) LinearProgressIndicator(Modifier.fillMaxWidth(), color = c.accent, trackColor = c.chip)
            val result = state.result
            when {
                !SearchRules.usable(state.query) -> EmptyState(
                    "Busque em todo o acervo.",
                    text = "Digite ao menos 2 letras. A busca olha os documentos, as tarefas e os atendimentos.",
                    icon = Icons.Outlined.Search
                )
                state.error != null -> Notice(state.error, Tone.ERROR, Modifier.padding(16.dp))
                result == null -> if (state.loading) LoadingBox("Buscando…") else Unit
                result.total == 0 && !state.loading -> EmptyState(
                    "Nada encontrado.", text = "Tente outras palavras.", icon = Icons.Outlined.SearchOff
                )
                else -> Results(result, onOpenDocument, onOpenTask, onOpenAttendance)
            }
        }
    }
}

@Composable
private fun Results(
    r: SearchResult,
    onOpenDocument: (Long) -> Unit,
    onOpenTask: (Long) -> Unit,
    onOpenAttendance: (Long) -> Unit
) {
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        group("d", "Documentos", r.documents, { it.id }) { d -> DocumentResult(d) { onOpenDocument(d.id) } }
        group("t", "Tarefas", r.tasks, { it.id }) { t -> TaskResult(t) { onOpenTask(t.id) } }
        group("a", "Atendimentos", r.attendances, { it.id }) { a -> AttendanceCard(a, onClick = { onOpenAttendance(a.id) }) }
    }
}

private fun <T> LazyListScope.group(
    key: String,
    title: String,
    g: SearchGroup<T>,
    id: (T) -> Long,
    row: @Composable (T) -> Unit
) {
    if (g.rows.isEmpty()) return
    item(key = "h-$key") { SectionLabel("$title · ${g.rows.size + g.more}", Modifier.padding(top = 8.dp)) }
    items(g.rows, key = { "$key-${id(it)}" }) { row(it) }
    if (g.more > 0) {
        item(key = "m-$key") {
            Text(
                SearchRules.moreLine(g.more), Modifier.padding(start = 4.dp),
                style = MaterialTheme.typography.bodyMedium, color = Ponte.colors.muted
            )
        }
    }
}

@Composable
private fun DocumentResult(d: SearchDoc, onClick: () -> Unit) {
    val c = Ponte.colors
    PonteCard(onClick = onClick) {
        Column(Modifier.padding(start = 16.dp, top = 12.dp, end = 16.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (d.kind.isNotEmpty()) Tag(d.kind, Tone.ACCENT)
            Text(
                d.title.ifEmpty { "Sem título" }, fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium, color = c.text,
                maxLines = 2, overflow = TextOverflow.Ellipsis
            )
            if (d.number.isNotEmpty()) {
                Text(d.number, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun TaskResult(t: TaskItem, onClick: () -> Unit) {
    val c = Ponte.colors
    PonteCard(onClick = onClick) {
        Row(Modifier.padding(start = 16.dp, top = 12.dp, end = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    t.title, fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium,
                    color = if (t.done) c.muted else c.text,
                    textDecoration = if (t.done) androidx.compose.ui.text.style.TextDecoration.LineThrough else null,
                    maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                Text(
                    t.secondLine, style = MaterialTheme.typography.bodySmall, color = c.muted,
                    maxLines = 1, overflow = TextOverflow.Ellipsis
                )
            }
            if (!t.done && t.dueChip.isNotEmpty()) {
                Spacer(Modifier.width(8.dp))
                Pill(t.dueChip, if (t.overdue) Tone.ERROR else if (t.dueToday) Tone.WARNING else Tone.NEUTRAL)
            }
        }
    }
}
