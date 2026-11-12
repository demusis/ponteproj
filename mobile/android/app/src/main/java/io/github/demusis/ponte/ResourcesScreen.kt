package io.github.demusis.ponte

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
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
    now: String,
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
    val groups = remember(state.uses, query, now) { ResourceRules.group(state.uses, query, now) }
    val rows = remember(state.resources, query) { ResourceRules.resourceRows(state.resources, query) }

    if (state.loading && state.uses.isEmpty() && state.resources.isEmpty()) {
        LoadingBox("Carregando os recursos…", Modifier.fillMaxSize())
        return
    }
    val usesView = view == 0
    val hasData = if (usesView) state.uses.isNotEmpty() else state.resources.isNotEmpty()
    val noMatch = hasData && (if (usesView) groups.total == 0 else rows.isEmpty())
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, top = 8.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        state.error?.let { item(key = "error") { Notice(it) } }
        item(key = "toggle") {
            FilterChips(
                listOf("0" to PhoneWords.RES_USES, "1" to PhoneWords.RES_AREA), view.toString(),
                { onView(it.toInt()) }
            )
        }
        if (header != null) item(key = "header") { header() }
        if (hasData) {
            item(key = "search") {
                SearchBox(query, onQuery, if (usesView) PhoneWords.RES_SEARCH_USES else PhoneWords.RES_SEARCH_RESOURCES)
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
                ResourceCard(r, ResourceRules.usageLine(r, state.uses, now), onClick = { onOpenResource(r) })
            }
            else -> {
                for ((group, list) in groups.shown) {
                    if (group == ResourceRules.Group.CLOSED) {
                        item(key = "h-closed") {
                            Row(
                                Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { showClosed = !showClosed },
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                SectionLabel("${ResourceRules.groupTitle(group)} · ${list.size}", Modifier.weight(1f))
                                Icon(
                                    Icons.Outlined.ExpandMore,
                                    contentDescription = if (showClosed) "Recolher os encerrados" else "Mostrar os encerrados",
                                    tint = c.muted, modifier = Modifier.padding(end = 4.dp).size(22.dp).rotate(if (showClosed) 180f else 0f)
                                )
                            }
                        }
                        // A search that found closed uses shows them: the person asked for them.
                        if (showClosed || query.isNotBlank()) {
                            items(list, key = { "u-${it.uid}" }) { u -> ResourceUseCard(u, now, onClick = { onOpenUse(u) }) }
                        }
                    } else {
                        item(key = "h-$group") {
                            SectionLabel(
                                "${ResourceRules.groupTitle(group)} · ${list.size}", Modifier.padding(top = 4.dp),
                                color = if (group == ResourceRules.Group.LATE) c.error else c.muted
                            )
                        }
                        items(list, key = { "u-${it.uid}" }) { u -> ResourceUseCard(u, now, onClick = { onOpenUse(u) }) }
                    }
                }
            }
        }
    }
}

/** The pill of a use: "Atrasado" in red, else its situation. */
@Composable
private fun UsePill(u: ResourceUseItem, now: String, modifier: Modifier = Modifier) {
    when {
        ResourceRules.isOverdue(u, now) -> Pill(PhoneWords.RES_OVERDUE, Tone.ERROR, modifier)
        u.status == ResourceRules.IN_USE -> Pill(u.statusLabel.ifEmpty { "Em uso" }, Tone.ACCENT, modifier)
        u.status == ResourceRules.RESERVED -> Pill(u.statusLabel.ifEmpty { "Reservado" }, Tone.NEUTRAL, modifier)
        else -> Pill(u.statusLabel, Tone.NEUTRAL, modifier)
    }
}

/**
 * A use in a card: the resource, who (and sector), the period, and its situation in a pill. A returned
 * or cancelled use is in the muted color.
 */
@Composable
internal fun ResourceUseCard(u: ResourceUseItem, now: String, onClick: (() -> Unit)? = null) {
    val c = Ponte.colors
    val closed = u.status == ResourceRules.RETURNED || u.status == ResourceRules.CANCELLED
    PonteCard(onClick = onClick) {
        Row(Modifier.padding(start = 16.dp, top = 12.dp, end = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    u.resourceName.ifEmpty { PhoneWords.RES_RESOURCE_TITLE }, fontSize = 16.sp, lineHeight = 21.sp,
                    fontWeight = FontWeight.Medium, color = if (closed) c.muted else c.text, maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                val who = ResourceRules.whoLine(u)
                if (who.isNotEmpty()) {
                    Text(who, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                val period = ResourceRules.periodText(u.start, u.end) +
                    if (u.quantity > 1) " · " + PhoneWords.plural(u.quantity, "unidade", "unidades") else ""
                Text(period, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(8.dp))
            UsePill(u, now)
        }
    }
}

/** A resource of the register in a card: name, where it is, and how many are out now. */
@Composable
internal fun ResourceCard(r: ResourceItem, usage: String, onClick: (() -> Unit)? = null) {
    val c = Ponte.colors
    PonteCard(onClick = onClick) {
        Row(Modifier.padding(start = 16.dp, top = 12.dp, end = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
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
                Pill(PhoneWords.RES_INACTIVE, Tone.NEUTRAL)
            }
        }
    }
}

// ---------------------------------------------------------------- detail screens

/** A card of label and text pairs, the way the atendimento's ficha shows its data. */
@Composable
private fun DataCard(title: String, fields: List<Pair<String, String>>) {
    val c = Ponte.colors
    val shown = fields.filter { it.second.isNotBlank() }
    if (shown.isEmpty()) return
    PonteCard {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SectionLabel(title, Modifier.padding(start = 0.dp))
            for ((label, value) in shown) {
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(label, style = FieldLabelStyle, color = c.secondary)
                    Text(value, fontSize = 16.sp, lineHeight = 22.sp, color = c.text)
                }
            }
        }
    }
}

/** A tappable card that leads to another screen: small label, one text, "Abrir". */
@Composable
private fun LinkCard(icon: ImageVector, label: String, text: String, onClick: () -> Unit, secondary: String = "") {
    val c = Ponte.colors
    PonteCard(onClick = onClick) {
        Row(
            Modifier.heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(icon, contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(label, style = FieldLabelStyle, color = c.secondary)
                Text(
                    text, fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, color = c.text,
                    maxLines = 3, overflow = TextOverflow.Ellipsis
                )
                if (secondary.isNotBlank()) {
                    Text(secondary, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
            Text("Abrir", style = MaterialTheme.typography.labelLarge, color = c.accent, modifier = Modifier.padding(start = 8.dp))
        }
    }
}

/**
 * One use, to read: every field, and the links (the resource, the document, the atendimento and the task
 * it belongs to). [footer] is where 0.7.1 puts "Registrar devolução".
 */
@Composable
internal fun ResourceUseScreen(
    uid: String,
    state: ResourcesState,
    now: String,
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
            u != null -> ResourceUseBody(u, state, now, onOpenResource, onOpenDocument, onOpenAttendance, onOpenTask)
            state.error != null -> Notice(state.error, Tone.ERROR, Modifier.padding(16.dp))
            state.loading -> LoadingBox()
            else -> Notice("Este uso não foi encontrado. Ele pode ter sido apagado em outro aparelho.", Tone.WARNING, Modifier.padding(16.dp))
        }
    }
}

@Composable
private fun ResourceUseBody(
    u: ResourceUseItem,
    state: ResourcesState,
    now: String,
    onOpenResource: (String) -> Unit,
    onOpenDocument: (Long) -> Unit,
    onOpenAttendance: (Long) -> Unit,
    onOpenTask: (Long) -> Unit
) {
    val c = Ponte.colors
    val resource = state.resources.firstOrNull { it.uid == u.resourceUid }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Column(Modifier.padding(horizontal = 4.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                u.resourceName.ifEmpty { PhoneWords.RES_RESOURCE_TITLE },
                fontSize = 22.sp, lineHeight = 29.sp, fontWeight = FontWeight.SemiBold, color = c.text
            )
            val who = ResourceRules.whoLine(u)
            if (who.isNotEmpty()) Text(who, style = MaterialTheme.typography.bodyLarge, color = c.secondary)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                UsePill(u, now)
                if (ResourceRules.isOverdue(u, now)) Pill("Fim previsto ${ResourceRules.momentText(u.end)}", Tone.NEUTRAL, Modifier.weight(1f, fill = false))
            }
        }
        Notice(RESOURCE_READ_ONLY, Tone.NEUTRAL)
        DataCard(
            "Dados",
            listOf(
                PhoneWords.RES_WHO to u.who,
                PhoneWords.RES_SECTOR to u.sector,
                PhoneWords.RES_PURPOSE to u.purpose,
                PhoneWords.RES_START to ResourceRules.momentText(u.start),
                PhoneWords.RES_END to ResourceRules.momentText(u.end),
                PhoneWords.RES_RETURNED to if (u.returnedAt.isEmpty()) "" else ResourceRules.momentText(u.returnedAt),
                PhoneWords.RES_STATUS to if (ResourceRules.isOverdue(u, now)) "${u.statusLabel} · ${PhoneWords.RES_OVERDUE}" else u.statusLabel,
                PhoneWords.RES_QUANTITY to u.quantity.toString(),
                PhoneWords.RES_NOTES to u.notes
            )
        )
        LinkCard(
            Icons.Outlined.CalendarMonth, PhoneWords.RES_RESOURCE_TITLE, u.resourceName.ifEmpty { PhoneWords.RES_RESOURCE_TITLE },
            onClick = { onOpenResource(u.resourceUid) }, secondary = resource?.location.orEmpty()
        )
        if (u.documentId > 0) {
            LinkCard(Icons.Outlined.Description, "Documento ligado", u.documentLine.ifEmpty { "Documento" }, onClick = { onOpenDocument(u.documentId) })
        }
        if (u.attendanceId > 0) {
            LinkCard(Icons.Outlined.Inbox, "Atendimento ligado", u.attendanceReference.ifEmpty { "Atendimento" }, onClick = { onOpenAttendance(u.attendanceId) })
        }
        if (u.taskId > 0) {
            LinkCard(Icons.Outlined.Description, "Tarefa ligada", u.taskTitle.ifEmpty { "Tarefa" }, onClick = { onOpenTask(u.taskId) })
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
    now: String,
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
            r != null -> ResourceBody(r, state, now, onOpenUse)
            state.error != null -> Notice(state.error, Tone.ERROR, Modifier.padding(16.dp))
            state.loading -> LoadingBox()
            else -> Notice("Este recurso não foi encontrado. Ele pode ter sido apagado em outro aparelho.", Tone.WARNING, Modifier.padding(16.dp))
        }
    }
}

@Composable
private fun ResourceBody(r: ResourceItem, state: ResourcesState, now: String, onOpenUse: (String) -> Unit) {
    val c = Ponte.colors
    val next = ResourceRules.nextUses(r, state.uses, now)
    val today = ResourceRules.occupationToday(r, state.uses, now)
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Column(Modifier.padding(horizontal = 4.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(r.name, fontSize = 22.sp, lineHeight = 29.sp, fontWeight = FontWeight.SemiBold, color = c.text)
            if (!r.active) Pill(PhoneWords.RES_INACTIVE, Tone.NEUTRAL)
            if (r.active) {
                Text(
                    ResourceRules.usageLine(r, state.uses, now) + " · " +
                        PhoneWords.plural(ResourceRules.freeNow(r, state.uses, now), "livre", "livres"),
                    style = MaterialTheme.typography.bodyMedium, color = c.secondary
                )
            }
        }
        Notice(RESOURCE_READ_ONLY, Tone.NEUTRAL)
        DataCard(
            "Dados",
            listOf(
                PhoneWords.RES_QUANTITY to r.quantity.toString(),
                PhoneWords.RES_LOCATION to r.location,
                PhoneWords.RES_IDENTIFICATION to r.identification,
                PhoneWords.RES_STATUS to r.statusLabel,
                PhoneWords.RES_NOTES to r.notes
            )
        )
        PonteCard {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                SectionLabel("Ocupação de hoje", Modifier.padding(start = 0.dp))
                if (today.isEmpty()) {
                    Text("Nenhum uso hoje.", style = MaterialTheme.typography.bodyMedium, color = c.muted)
                }
                today.forEachIndexed { i, u ->
                    if (i > 0) HorizontalDivider(color = c.divider)
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(ResourceRules.periodText(u.start, u.end), fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text)
                        Text(
                            listOf(ResourceRules.whoLine(u), if (ResourceRules.isOverdue(u, now)) PhoneWords.RES_OVERDUE else u.statusLabel)
                                .filter { it.isNotEmpty() }.joinToString(" · "),
                            style = MaterialTheme.typography.bodySmall, color = c.secondary
                        )
                    }
                }
            }
        }
        if (next.isNotEmpty()) {
            PonteCard {
                Column(Modifier.padding(top = 14.dp, bottom = 6.dp)) {
                    SectionLabel("Próximos usos · ${next.size}", Modifier.padding(start = 12.dp, bottom = 4.dp))
                    for (u in next) {
                        Row(
                            Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(onClickLabel = "Abrir o uso") { onOpenUse(u.uid) }
                                .padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Column(Modifier.weight(1f)) {
                                Text(
                                    ResourceRules.periodText(u.start, u.end), fontSize = 15.sp, fontWeight = FontWeight.Medium,
                                    color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis
                                )
                                val who = ResourceRules.whoLine(u)
                                if (who.isNotEmpty()) Text(who, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                            Spacer(Modifier.width(8.dp))
                            UsePill(u, now)
                        }
                    }
                }
            }
        }
    }
}
