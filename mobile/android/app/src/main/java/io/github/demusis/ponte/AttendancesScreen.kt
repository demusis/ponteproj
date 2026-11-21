package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Inbox
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Two or three words side by side, one of them chosen ("Tarefas | Atendimentos | Recursos"). The size
 * is worked out in dp as for the bottom bar, so a narrow screen or a large font setting shrinks the
 * words instead of cutting them.
 */
@Composable
internal fun SegmentedTabs(options: List<String>, selected: Int, onSelect: (Int) -> Unit, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    val density = androidx.compose.ui.platform.LocalDensity.current
    // The row loses 16 dp at each side and its own 3 dp padding.
    val room = androidx.compose.ui.platform.LocalConfiguration.current.screenWidthDp - 38
    // With more than three words, each gets room in proportion to its length (a short "Tarefas" does not take
    // as much as "Atendimentos"), so the size stays readable on a narrow screen.
    val shares = NavLabel.shares(options)
    val size = with(density) { NavLabel.sizeDp(room, options, 15f * density.fontScale, shares).dp.toSp() }
    Row(
        modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)
            .background(c.chip, RoundedCornerShape(24.dp)).padding(3.dp)
    ) {
        options.forEachIndexed { i, label ->
            val on = i == selected
            Box(
                Modifier.weight(shares[i]).heightIn(min = 44.dp)
                    .background(if (on) c.surface else Color.Transparent, RoundedCornerShape(21.dp))
                    .selectable(selected = on, role = Role.Tab, onClick = { onSelect(i) }),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    label, fontSize = size, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Medium,
                    color = if (on) c.text else c.secondary, maxLines = 1, softWrap = false
                )
            }
        }
    }
}

/** The rounded search box (Atendimentos and the search screen). [focus]: the keyboard goes into it. */
@Composable
internal fun SearchBox(
    query: String,
    onQuery: (String) -> Unit,
    placeholder: String,
    modifier: Modifier = Modifier,
    focus: FocusRequester? = null
) {
    val c = Ponte.colors
    OutlinedTextField(
        value = query, onValueChange = onQuery,
        modifier = modifier.fillMaxWidth().then(if (focus != null) Modifier.focusRequester(focus) else Modifier),
        singleLine = true,
        textStyle = MaterialTheme.typography.bodyLarge,
        placeholder = { Text(placeholder, color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis) },
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

/**
 * The Atendimentos list, in the Tarefas area: the open ones first as the core ordered them (late,
 * due today, by deadline, without deadline), the closed ones folded under "Encerrados". Read-only:
 * a tap opens the ficha.
 */
@Composable
internal fun AttendancesScreen(
    state: AttendancesState,
    query: String,
    onQuery: (String) -> Unit,
    onOpen: (AttendanceItem) -> Unit,
    /** (atendimento id, contact text, next date AAAA-MM-DD or empty). */
    onRegisterFollowUp: (Long, String, String) -> Unit = { _, _, _ -> },
    /** (atendimento id, next date AAAA-MM-DD). */
    onScheduleFollowUp: (Long, String) -> Unit = { _, _ -> },
    /** "Sincronizar agora" da lista vazia. */
    onSync: () -> Unit = {}
) {
    val c = Ponte.colors
    var showClosed by rememberSaveable { mutableStateOf(false) }
    val split = remember(state.rows, query) { AttendanceRules.split(state.rows, query) }
    val chase = remember(state.followUps, query) { state.followUps.filter { AttendanceRules.matches(it, query) } }
    // The window of "Registrar cobrança" / "Próxima cobrança" is for this atendimento (0: none open).
    var dialogId by rememberSaveable { mutableStateOf(0L) }
    var dialogContact by rememberSaveable { mutableStateOf(true) }
    state.followUps.firstOrNull { it.id == dialogId }?.let { target ->
        FollowUpDialog(
            target, contact = dialogContact, channels = state.channels,
            onDismiss = { dialogId = 0 },
            onConfirm = { text, next ->
                dialogId = 0
                if (dialogContact) onRegisterFollowUp(target.id, text, next) else onScheduleFollowUp(target.id, next)
            }
        )
    }

    if (state.loading && state.rows.isEmpty()) {
        LoadingBox(text = "Carregando os atendimentos…", modifier = Modifier.fillMaxSize())
        return
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = 4.dp, bottom = 32.dp)) {
        val pad = Modifier.padding(horizontal = ROW_MARGIN, vertical = 4.dp)
        state.error?.let { item(key = "error") { Notice(it, pad) } }
        if (state.rows.isNotEmpty()) {
            item(key = "search") {
                SearchBox(query, onQuery, "Referência, assunto, responsável", pad)
            }
        }
        if (chase.isNotEmpty()) {
            item(key = "h-chase") { SectionLabel(FollowUpRules.title(chase.size), Modifier.padding(start = 12.dp, top = 8.dp, bottom = 4.dp)) }
            items(chase, key = { "f-${it.id}" }) { a ->
                FollowUpCard(
                    a, onOpen = { onOpen(a) },
                    onRegister = { dialogContact = true; dialogId = a.id },
                    onSchedule = { dialogContact = false; dialogId = a.id }
                )
            }
        }
        if (state.rows.isEmpty()) {
            item(key = "none") {
                EmptyState(
                    "Nenhum atendimento neste aparelho.",
                    text = "Os atendimentos do Ponte no computador aparecem aqui depois da sincronização.",
                    icon = Icons.Outlined.Inbox,
                    action = {
                        TextButton(onClick = onSync) {
                            Text("Sincronizar agora", color = c.accent, fontWeight = FontWeight.SemiBold)
                        }
                    }
                )
            }
        } else if (split.open.isEmpty() && split.closed.isEmpty()) {
            item(key = "nomatch") {
                EmptyState(
                    "Nada encontrado.", text = "Nenhum atendimento combina com a busca.", icon = Icons.Outlined.SearchOff,
                    action = {
                        TextButton(onClick = { onQuery("") }) {
                            Text("Limpar a busca", color = c.accent, fontWeight = FontWeight.SemiBold)
                        }
                    }
                )
            }
        } else {
            item(key = "h-open") {
                SectionLabel("Em aberto · ${split.open.size}", Modifier.padding(start = 12.dp, top = 8.dp, bottom = 4.dp))
            }
            if (split.open.isEmpty()) {
                item(key = "no-open") {
                    Text(
                        "Nenhum atendimento em aberto.", Modifier.padding(horizontal = 20.dp, vertical = 4.dp),
                        style = MaterialTheme.typography.bodyMedium, color = c.muted
                    )
                }
            }
            items(split.open, key = { "a-${it.id}" }) { a -> AttendanceRow(a, onClick = { onOpen(a) }) }
            if (split.closed.isNotEmpty()) {
                item(key = "h-closed") {
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { showClosed = !showClosed }.padding(start = 12.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        SectionLabel("Encerrados · ${split.closed.size}", Modifier.weight(1f))
                        Icon(
                            Icons.Outlined.ExpandMore,
                            contentDescription = if (showClosed) "Recolher os encerrados" else "Mostrar os encerrados",
                            tint = c.muted, modifier = Modifier.padding(end = 16.dp).size(22.dp).rotate(if (showClosed) 180f else 0f)
                        )
                    }
                }
                if (showClosed) items(split.closed, key = { "c-${it.id}" }) { a -> AttendanceRow(a, onClick = { onOpen(a) }) }
            }
        }
    }
}

/**
 * An atendimento as a flat row: reference (or subject), subject, situation and responsible person, and the
 * deadline at the end (only while it is open). Tapping it opens the ficha when [onClick] is given.
 */
@Composable
internal fun AttendanceRow(a: AttendanceItem, onClick: (() -> Unit)? = null) {
    val c = Ponte.colors
    FlatRow(onClick) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f)) {
                Text(
                    a.reference, fontSize = 16.sp, lineHeight = 21.sp, fontWeight = FontWeight.Medium,
                    color = if (a.open) c.text else c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis
                )
                if (a.subject.isNotEmpty() && a.subject != a.reference) {
                    Text(a.subject, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
                val line = listOf(a.statusLabel, a.assignee).filter { it.isNotEmpty() }.joinToString(" · ")
                if (line.isNotEmpty()) {
                    Text(line, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (a.open && a.dueChip.isNotEmpty()) {
                Spacer(Modifier.width(8.dp))
                DueMark(a.dueChip, a.late, a.dueToday)
            }
        }
    }
}

/** Puts the keyboard in [focus] once, when the screen opens (a screen without a window just ignores it). */
@Composable
internal fun AutoFocus(focus: FocusRequester) {
    androidx.compose.runtime.LaunchedEffect(Unit) {
        try { focus.requestFocus() } catch (_: Throwable) { }
    }
}
