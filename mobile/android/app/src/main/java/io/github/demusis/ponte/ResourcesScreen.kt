package io.github.demusis.ponte

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CalendarMonth
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Inbox
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** The note at the head of a use or a resource: nothing here can be changed on the phone (yet). */
internal const val RESOURCE_READ_ONLY = "Para reservar ou devolver, use o Ponte no computador."

/**
 * The Recursos list, in the Tarefas area. Two views, chosen by a toggle: **Usos** (reservations and
 * loans, in groups: Atrasados, Em uso agora, Hoje, Próximos dias, Mais adiante and the folded Encerrados)
 * and **Recursos** (the register: name, onde fica, "N de M em uso agora"). Read-only: a tap opens
 * the detail. [header] is a slot under the toggle for what 0.7.1 adds ("Reservar").
 */
@Composable
internal fun ResourcesScreen(
    state: ResourcesState,
    query: String,
    onQuery: (String) -> Unit,
    view: Int,
    onView: (Int) -> Unit,
    onOpenUse: (ResourceUseItem) -> Unit,
    onOpenResource: (ResourceItem) -> Unit,
    header: (@Composable () -> Unit)? = null
) {
    val c = Ponte.colors
    var showClosed by rememberSaveable { mutableStateOf(false) }
    // The core already grouped and searched; nothing is computed over the uses here.
    val rows = state.rowResources

    if (state.loading && state.uses.isEmpty() && state.resources.isEmpty()) {
        LoadingBox(text = "Carregando os recursos…", modifier = Modifier.fillMaxSize())
        return
    }
    val usesView = view == 0
    val hasData = if (usesView) state.uses.isNotEmpty() else state.resources.isNotEmpty()
    val noMatch = hasData && (if (usesView) state.groups.isEmpty() else rows.isEmpty())
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = 4.dp, bottom = 32.dp)) {
        val pad = Modifier.padding(horizontal = ROW_MARGIN, vertical = 4.dp)
        state.error?.let { item(key = "error") { Notice(it, pad) } }
        item(key = "toggle") {
            FilterChips(
                listOf("0" to PhoneWords.RES_USES, "1" to PhoneWords.RES_AREA), view.toString(),
                { onView(it.toInt()) },
                contentPadding = PaddingValues(horizontal = ROW_MARGIN, vertical = 4.dp)
            )
        }
        if (header != null) item(key = "header") { Box(pad) { header() } }
        if (hasData) {
            item(key = "search") {
                SearchBox(query, onQuery, if (usesView) PhoneWords.RES_SEARCH_USES else PhoneWords.RES_SEARCH_RESOURCES, pad)
            }
        }
        when {
            !hasData -> item(key = "none") {
                EmptyState(
                    if (usesView) "Nenhum uso neste aparelho." else "Nenhum recurso neste aparelho.",
                    text = if (usesView) {
                        "As reservas e os empréstimos de recursos do Ponte no computador aparecem aqui depois da sincronização."
                    } else {
                        "As salas, os equipamentos e os livros do Ponte no computador aparecem aqui depois da sincronização."
                    },
                    icon = Icons.Outlined.Inbox
                )
            }
            noMatch -> item(key = "nomatch") {
                EmptyState(
                    "Nada encontrado.", text = "Nada combina com a busca.", icon = Icons.Outlined.SearchOff,
                    action = {
                        TextButton(onClick = { onQuery("") }) {
                            Text("Limpar a busca", color = c.accent, fontWeight = FontWeight.SemiBold)
                        }
                    }
                )
            }
            !usesView -> items(rows, key = { "r-${it.uid}" }) { r ->
                ResourceRow(r, r.usageLine, onClick = { onOpenResource(r) })
            }
            else -> {
                for (group in state.groups) {
                    val list = state.usesOf(group.uids)
                    if (group.key == "encerrados") {
                        item(key = "h-closed") {
                            Row(
                                Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { showClosed = !showClosed }.padding(start = 12.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                SectionLabel("${group.title} · ${list.size}", Modifier.weight(1f))
                                Icon(
                                    Icons.Outlined.ExpandMore,
                                    contentDescription = if (showClosed) "Recolher os encerrados" else "Mostrar os encerrados",
                                    tint = c.muted, modifier = Modifier.padding(end = 16.dp).size(22.dp).rotate(if (showClosed) 180f else 0f)
                                )
                            }
                        }
                        // A search that found closed uses shows them: the person asked for them.
                        if (showClosed || query.isNotBlank()) {
                            items(list, key = { "u-${it.uid}" }) { u -> ResourceUseRow(u, onClick = { onOpenUse(u) }) }
                        }
                    } else {
                        item(key = "h-${group.key}") {
                            SectionLabel(
                                "${group.title} · ${list.size}", Modifier.padding(start = 12.dp, top = 8.dp, bottom = 4.dp),
                                color = if (group.late) c.error else c.muted
                            )
                        }
                        items(list, key = { "u-${it.uid}" }) { u -> ResourceUseRow(u, onClick = { onOpenUse(u) }) }
                    }
                }
            }
        }
    }
}

/** The pill of a use: "Atrasado" in red, else its situation. */
@Composable
private fun UsePill(u: ResourceUseItem, modifier: Modifier = Modifier) {
    when {
        u.overdue -> Pill(PhoneWords.RES_OVERDUE, tone = Tone.ERROR, modifier = modifier)
        u.status == ResourceRules.IN_USE -> Pill(u.statusLabel.ifEmpty { "Em uso" }, tone = Tone.ACCENT, modifier = modifier)
        u.status == ResourceRules.RESERVED -> Pill(u.statusLabel.ifEmpty { "Reservado" }, tone = Tone.NEUTRAL, modifier = modifier)
        else -> Pill(u.statusLabel, tone = Tone.NEUTRAL, modifier = modifier)
    }
}

/**
 * A use as a flat row: the resource, who (and sector), the period, and its situation in a pill. A returned
 * or cancelled use is in the muted color.
 */
@Composable
internal fun ResourceUseRow(u: ResourceUseItem, onClick: (() -> Unit)? = null) {
    val c = Ponte.colors
    val closed = u.status == ResourceRules.RETURNED || u.status == ResourceRules.CANCELLED
    FlatRow(onClick) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    u.resourceName.ifEmpty { PhoneWords.RES_RESOURCE_TITLE }, fontSize = 16.sp, lineHeight = 21.sp,
                    fontWeight = FontWeight.Medium, color = if (closed) c.muted else c.text, maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                val who = ResourceRules.whoLine(u)
                if (who.isNotEmpty()) {
                    Text(who, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                val period = u.period +
                    if (u.quantity > 1) " · " + PhoneWords.plural(u.quantity, "unidade", "unidades") else ""
                Text(period, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(8.dp))
            UsePill(u)
        }
    }
}

/** A resource of the register as a flat row: name, where it is, and how many are out now. */
@Composable
internal fun ResourceRow(r: ResourceItem, usage: String, onClick: (() -> Unit)? = null) {
    val c = Ponte.colors
    FlatRow(onClick) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    r.name, fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium,
                    color = if (r.active) c.text else c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                if (r.location.isNotBlank()) {
                    Text(r.location, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
                if (r.active) Text(usage, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1)
            }
            if (!r.active) {
                Spacer(Modifier.width(8.dp))
                Pill(PhoneWords.RES_INACTIVE, tone = Tone.NEUTRAL)
            }
        }
    }
}

// ---------------------------------------------------------------- detail screens

/** The label and text pairs of a ficha, as a flat section. */
@Composable
private fun DataSection(title: String, fields: List<Pair<String, String>>) {
    val shown = fields.filter { it.second.isNotBlank() }
    if (shown.isEmpty()) return
    FlatSection(title) { for ((label, value) in shown) FactRow(label, value) }
}

/** A row that leads to another screen: small label, one text, chevron. */
@Composable
private fun LinkSection(icon: ImageVector, label: String, text: String, onClick: () -> Unit, secondary: String = "") {
    LinkRow(icon, label, text, onClick, secondary = secondary)
}

/**
 * One use, to read: every field, and the links (the resource, the document, the atendimento and the task
 * it belongs to). [footer] is where 0.7.1 puts "Registrar devolução".
 */
@Composable
internal fun ResourceUseScreen(
    uid: String,
    state: ResourcesState,
    onBack: () -> Unit,
    onOpenResource: (String) -> Unit,
    onOpenDocument: (Long) -> Unit,
    onOpenAttendance: (Long) -> Unit,
    onOpenTask: (Long) -> Unit,
    footer: (@Composable ColumnScope.(ResourceUseItem) -> Unit)? = null
) {
    val u = state.uses.firstOrNull { it.uid == uid }
    DetailScaffold(
        title = PhoneWords.RES_USE_TITLE, onBack = onBack,
        footer = if (u != null && footer != null) ({ footer(u) }) else null
    ) {
        when {
            u != null -> ResourceUseBody(u, state, onOpenResource, onOpenDocument, onOpenAttendance, onOpenTask)
            state.error != null -> Notice(state.error, tone = Tone.ERROR, modifier = Modifier.padding(16.dp))
            state.loading -> LoadingBox()
            else -> Notice("Este uso não foi encontrado. Ele pode ter sido apagado em outro aparelho.", tone = Tone.WARNING, modifier = Modifier.padding(16.dp))
        }
    }
}

@Composable
private fun ResourceUseBody(
    u: ResourceUseItem,
    state: ResourcesState,
    onOpenResource: (String) -> Unit,
    onOpenDocument: (Long) -> Unit,
    onOpenAttendance: (Long) -> Unit,
    onOpenTask: (Long) -> Unit
) {
    val c = Ponte.colors
    val resource = state.resources.firstOrNull { it.uid == u.resourceUid }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = 4.dp, bottom = 32.dp)) {
        FichaHeader(
            u.resourceName.ifEmpty { PhoneWords.RES_RESOURCE_TITLE },
            lines = listOf(ResourceRules.whoLine(u)),
            extra = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    UsePill(u)
                    if (u.overdue) Pill("Fim previsto ${ResourceRules.momentText(u.end)}", tone = Tone.NEUTRAL, modifier = Modifier.weight(1f, fill = false))
                }
            }
        )
        SectionBody { Notice(RESOURCE_READ_ONLY, tone = Tone.NEUTRAL) }
        DataSection(
            "Dados",
            listOf(
                PhoneWords.RES_WHO to u.who,
                PhoneWords.RES_SECTOR to u.sector,
                PhoneWords.RES_PURPOSE to u.purpose,
                PhoneWords.RES_START to ResourceRules.momentText(u.start),
                PhoneWords.RES_END to ResourceRules.momentText(u.end),
                PhoneWords.RES_RETURNED to if (u.returnedAt.isEmpty()) "" else ResourceRules.momentText(u.returnedAt),
                PhoneWords.RES_STATUS to if (u.overdue) "${u.statusLabel} · ${PhoneWords.RES_OVERDUE}" else u.statusLabel,
                PhoneWords.RES_QUANTITY to u.quantity.toString(),
                PhoneWords.RES_NOTES to u.notes
            )
        )
        LinkSection(
            Icons.Outlined.CalendarMonth, PhoneWords.RES_RESOURCE_TITLE, u.resourceName.ifEmpty { PhoneWords.RES_RESOURCE_TITLE },
            onClick = { onOpenResource(u.resourceUid) }, secondary = resource?.location.orEmpty()
        )
        if (u.documentId > 0) {
            LinkSection(Icons.Outlined.Description, "Documento ligado", u.documentLine.ifEmpty { "Documento" }, onClick = { onOpenDocument(u.documentId) })
        }
        if (u.attendanceId > 0) {
            LinkSection(Icons.Outlined.Inbox, "Atendimento ligado", u.attendanceReference.ifEmpty { "Atendimento" }, onClick = { onOpenAttendance(u.attendanceId) })
        }
        if (u.taskId > 0) {
            LinkSection(Icons.Outlined.Description, "Tarefa ligada", u.taskTitle.ifEmpty { "Tarefa" }, onClick = { onOpenTask(u.taskId) })
        }
    }
}

/**
 * One resource, to read: its data, what is out now, who holds it today, and its next uses (tap one to
 * open it). [footer] is where 0.7.1 puts "Reservar".
 */
@Composable
internal fun ResourceScreen(
    uid: String,
    state: ResourcesState,
    onBack: () -> Unit,
    onOpenUse: (String) -> Unit,
    footer: (@Composable ColumnScope.(ResourceItem) -> Unit)? = null
) {
    val r = state.resources.firstOrNull { it.uid == uid }
    DetailScaffold(
        title = PhoneWords.RES_RESOURCE_TITLE, onBack = onBack,
        footer = if (r != null && footer != null) ({ footer(r) }) else null
    ) {
        when {
            r != null -> ResourceBody(r, state, onOpenUse)
            state.error != null -> Notice(state.error, tone = Tone.ERROR, modifier = Modifier.padding(16.dp))
            state.loading -> LoadingBox()
            else -> Notice("Este recurso não foi encontrado. Ele pode ter sido apagado em outro aparelho.", tone = Tone.WARNING, modifier = Modifier.padding(16.dp))
        }
    }
}

@Composable
private fun ResourceBody(r: ResourceItem, state: ResourcesState, onOpenUse: (String) -> Unit) {
    val c = Ponte.colors
    val next = state.usesOf(r.nextUids)
    val today = state.usesOf(r.todayUids)
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = 4.dp, bottom = 32.dp)) {
        FichaHeader(
            r.name,
            lines = if (r.active) listOf(r.usageLine + " · " + r.freeText) else emptyList(),
            extra = if (!r.active) ({ Pill(PhoneWords.RES_INACTIVE, tone = Tone.NEUTRAL) }) else null
        )
        SectionBody { Notice(RESOURCE_READ_ONLY, tone = Tone.NEUTRAL) }
        DataSection(
            "Dados",
            listOf(
                PhoneWords.RES_QUANTITY to r.quantity.toString(),
                PhoneWords.RES_LOCATION to r.location,
                PhoneWords.RES_IDENTIFICATION to r.identification,
                PhoneWords.RES_STATUS to r.statusLabel,
                PhoneWords.RES_NOTES to r.notes
            )
        )
        FlatSection("Ocupação de hoje", divider = next.isNotEmpty()) {
            if (today.isEmpty()) {
                Text("Nenhum uso hoje.", Modifier.padding(horizontal = ROW_MARGIN, vertical = 6.dp), style = MaterialTheme.typography.bodyMedium, color = c.muted)
            }
            today.forEachIndexed { i, u ->
                FlatRow(null, divider = i < today.lastIndex) {
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(u.period, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text)
                        Text(
                            listOf(ResourceRules.whoLine(u), if (u.overdue) PhoneWords.RES_OVERDUE else u.statusLabel)
                                .filter { it.isNotEmpty() }.joinToString(" · "),
                            style = MaterialTheme.typography.bodySmall, color = c.secondary
                        )
                    }
                }
            }
        }
        if (next.isNotEmpty()) {
            FlatSection("Próximos usos · ${next.size}", divider = false) {
                for ((i, u) in next.withIndex()) {
                    FlatRow({ onOpenUse(u.uid) }, divider = i < next.lastIndex) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Text(
                                    u.period, fontSize = 15.sp, fontWeight = FontWeight.Medium,
                                    color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis
                                )
                                val who = ResourceRules.whoLine(u)
                                if (who.isNotEmpty()) Text(who, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                            Spacer(Modifier.width(8.dp))
                            UsePill(u)
                        }
                    }
                }
            }
        }
    }
}
