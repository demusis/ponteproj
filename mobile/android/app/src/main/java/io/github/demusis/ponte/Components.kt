package io.github.demusis.ponte

import androidx.compose.foundation.BorderStroke
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.outlined.FilterList
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.delay
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.CalendarToday
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Mail
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.outlined.Today
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset

// Shared pieces of the interface. Every screen is built from these, so the look stays the same
// everywhere. Colors always come from Ponte.colors, text styles from MaterialTheme.typography.

/** Meaning carried by a color: late, today, done, plain, brand. */
internal enum class Tone { ERROR, WARNING, SUCCESS, NEUTRAL, ACCENT }

/** Background and text color of a [tone] (for pills, tags, icon squares). */
@Composable
internal fun toneColors(tone: Tone): Pair<Color, Color> {
    val c = Ponte.colors
    return when (tone) {
        Tone.ERROR -> c.errorContainer to c.error
        Tone.WARNING -> c.warningContainer to c.warning
        Tone.SUCCESS -> c.successContainer to c.success
        Tone.NEUTRAL -> c.chip to c.secondary
        Tone.ACCENT -> c.accentContainer to c.onAccentContainer
    }
}

internal fun formatSize(n: Long): String = when {
    n >= 1024 * 1024 -> String.format(java.util.Locale.forLanguageTag("pt-BR"), "%.1f MB", n / 1048576.0)
    n >= 1024 -> String.format(java.util.Locale.forLanguageTag("pt-BR"), "%d KB", n / 1024)
    else -> "$n bytes"
}

// ---------------------------------------------------------------- frames

/** What the filter icon of the frame needs: whether a filter is on (the icon shows a dot) and what a tap opens. */
internal class ShellFilter(val active: Boolean, val onClick: () -> Unit)

/**
 * Frame of the four tabs: the title (on Tarefas it is the selector of the four areas) with the sync dot after it,
 * the search, filter and overflow buttons, a thin progress bar while syncing, the content, and the bottom bar
 * (icons only). The text of the sync state shows only while it matters (see [SyncDot]).
 */
@Composable
internal fun MainShell(
    state: ShellState,
    onTab: (Int) -> Unit,
    onSync: () -> Unit,
    onSettings: () -> Unit,
    onStatus: () -> Unit,
    onSearch: (() -> Unit)? = null,
    /** An area of the Tarefas tab was chosen from the title (an id of [TasksSegments]). */
    onArea: (Int) -> Unit = {},
    /** The filter of the list in front; null: this list has none and the icon is not drawn. */
    filter: ShellFilter? = null,
    content: @Composable () -> Unit
) {
    val c = Ponte.colors
    // The text of the state: after a sync ends it stays a few seconds, and a tap on the dot shows it for as long.
    var peek by remember { mutableStateOf(false) }
    var recentlyEnded by remember { mutableStateOf(false) }
    var wasSyncing by remember { mutableStateOf(state.syncing) }
    LaunchedEffect(state.syncing) {
        if (wasSyncing && !state.syncing) {
            recentlyEnded = true
            try {
                delay(SyncDot.SHOW_MS)
            } finally {
                recentlyEnded = false
            }
        }
        wasSyncing = state.syncing
    }
    LaunchedEffect(peek) {
        if (peek) {
            try {
                delay(SyncDot.SHOW_MS)
            } finally {
                peek = false
            }
        }
    }
    Column(Modifier.fillMaxSize().background(c.ground)) {
        Column(Modifier.statusBarsPadding().fillMaxWidth().animateContentSize()) {
            Row(
                Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 8.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                    ShellTitle(state, onArea)
                    SyncDotButton(state) { peek = true }
                }
                // Hoje, Tarefas and Documentos: one search over the whole library.
                if (onSearch != null && state.tab in 0..2) {
                    IconButton(onClick = onSearch) {
                        Icon(Icons.Outlined.Search, contentDescription = "Buscar em tudo", tint = c.secondary)
                    }
                }
                if (filter != null) {
                    IconButton(onClick = filter.onClick) {
                        Box(Modifier.size(24.dp)) {
                            Icon(
                                Icons.Outlined.FilterList, contentDescription = FilterSummary.iconDescription(filter.active),
                                tint = if (filter.active) c.accent else c.secondary
                            )
                            if (filter.active) {
                                Box(
                                    Modifier.align(Alignment.TopEnd).offset(x = 3.dp, y = (-3).dp).size(10.dp)
                                        .background(c.accent, CircleShape).border(1.5.dp, c.ground, CircleShape)
                                )
                            }
                        }
                    }
                }
                OverflowMenu(state, onSync, onSettings)
            }
            if (SyncDot.lineVisible(state, recentlyEnded, peek)) {
                Text(
                    SyncDot.text(state),
                    Modifier.fillMaxWidth()
                        .clickable(onClickLabel = "Ver detalhes da sincronização", onClick = onStatus)
                        .padding(start = 16.dp, end = 16.dp, top = 2.dp, bottom = 8.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = if (state.statusKind == StatusKind.ERROR) c.error else c.muted,
                    maxLines = 1, overflow = TextOverflow.Ellipsis
                )
            }
        }
        if (state.syncing) {
            LinearProgressIndicator(
                Modifier.fillMaxWidth().height(3.dp), color = c.accent, trackColor = c.accentContainer
            )
        } else {
            Spacer(Modifier.height(3.dp))
        }
        Box(Modifier.weight(1f).fillMaxWidth()) { content() }
        BottomBar(state, onTab)
    }
}

/** The title of the frame: plain text, or on the Tarefas tab the button that opens the menu of areas. */
@Composable
private fun RowScope.ShellTitle(state: ShellState, onArea: (Int) -> Unit) {
    val c = Ponte.colors
    val heading = AreaSelector.heading(state)
    if (!AreaSelector.selectable(state)) {
        Text(
            heading, Modifier.weight(1f, fill = false).padding(start = 8.dp),
            fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.SemiBold, color = c.text,
            maxLines = 1, overflow = TextOverflow.Ellipsis
        )
        return
    }
    val area = AreaSelector.of(state)
    var open by remember { mutableStateOf(false) }
    Box(Modifier.weight(1f, fill = false)) {
        Row(
            Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(12.dp))
                .semantics { contentDescription = AreaSelector.description(area) }
                .clickable(role = Role.Button, onClickLabel = "Trocar a área") { open = true }
                .padding(start = 8.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                heading, Modifier.weight(1f, fill = false),
                fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.SemiBold, color = c.text,
                maxLines = 1, overflow = TextOverflow.Ellipsis
            )
            Icon(Icons.Outlined.ExpandMore, contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            AreaMenuItems(area) { open = false; onArea(it) }
        }
    }
}

/** The small coloured dot after the title: the state of the sync. A tap shows its text for a few seconds. */
@Composable
private fun SyncDotButton(state: ShellState, onTap: () -> Unit) {
    val c = Ponte.colors
    val color = when (state.statusKind) {
        StatusKind.OK -> c.success
        StatusKind.WAITING -> c.warning
        StatusKind.ERROR -> c.error
        StatusKind.NEUTRAL -> c.outline
    }
    Box(
        Modifier.size(width = 36.dp, height = 48.dp)
            .semantics { contentDescription = SyncDot.description(state) }
            .clickable(role = Role.Button, onClickLabel = "Ver o estado da sincronização", onClick = onTap),
        contentAlignment = Alignment.Center
    ) {
        Box(Modifier.size(10.dp).background(color, CircleShape))
    }
}

/** "⋮": Sincronizar agora (same rules as the old button) and Ajustes. */
@Composable
private fun OverflowMenu(state: ShellState, onSync: () -> Unit, onSettings: () -> Unit) {
    val c = Ponte.colors
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) {
            Icon(Icons.Outlined.MoreVert, contentDescription = "Mais opções", tint = c.secondary)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            val canSync = state.syncEnabled && !state.syncing
            DropdownMenuItem(
                // While it runs the item says so: that is why it cannot be tapped.
                text = { Text(if (state.syncing) "Sincronizando…" else "Sincronizar agora", fontSize = 16.sp) },
                onClick = { open = false; onSync() },
                enabled = canSync,
                leadingIcon = { Icon(Icons.Outlined.Sync, contentDescription = null) }
            )
            DropdownMenuItem(
                text = { Text("Ajustes", fontSize = 16.sp) },
                onClick = { open = false; onSettings() },
                leadingIcon = { Icon(Icons.Outlined.Tune, contentDescription = null) }
            )
        }
    }
}

/** The four tabs as icons only (the name is for the screen reader), 56 dp tall plus the system bar. */
@Composable
internal fun BottomBar(state: ShellState, onTab: (Int) -> Unit) {
    val c = Ponte.colors
    val titles = listOf("Hoje", "Tarefas", "Documentos", "E-mail")
    val icons = listOf(Icons.Outlined.Today, Icons.Outlined.CheckCircle, Icons.Outlined.Description, Icons.Outlined.Mail)
    Box(Modifier.fillMaxWidth().height(1.dp).background(c.barBorder))
    Row(Modifier.fillMaxWidth().background(c.surface).navigationBarsPadding().height(56.dp).selectableGroup()) {
        titles.forEachIndexed { i, title ->
            val on = state.tab == i
            val badge = if (i == 0) state.badge else 0
            // One name for the icon and the number: the reader says what the number means.
            val name = if (badge > 0) {
                title + ", " + PhoneWords.plural(badge, "tarefa atrasada ou para hoje", "tarefas atrasadas ou para hoje")
            } else title
            Box(
                Modifier.weight(1f).fillMaxHeight()
                    .semantics { contentDescription = name }
                    .selectable(selected = on, role = Role.Tab, onClick = { onTab(i) }),
                contentAlignment = Alignment.Center
            ) {
                Box(
                    Modifier.size(width = 64.dp, height = 32.dp).clip(RoundedCornerShape(16.dp))
                        .background(if (on) c.accentContainer else Color.Transparent),
                    contentAlignment = Alignment.Center
                ) {
                    val tint = if (on) c.onAccentContainer else c.secondary
                    if (badge > 0) {
                        BadgedBox(modifier = Modifier.clearAndSetSemantics { }, badge = {
                            Badge(containerColor = c.error, contentColor = if (c.dark) c.errorContainer else Color.White) {
                                Text(if (badge > 99) "99+" else badge.toString())
                            }
                        }) { Icon(icons[i], contentDescription = null, tint = tint) }
                    } else {
                        Icon(icons[i], contentDescription = null, tint = tint)
                    }
                }
            }
        }
    }
}

/**
 * The arithmetic of the words of [SegmentedTabs] (the segmented rows inside a screen): the share of each word and the
 * size that makes every word fit its share on one line.
 */
internal object NavLabel {
    /** Room a word keeps free at its sides, and how wide a bold letter is for its size. */
    private const val SIDE_DP = 6f
    private const val LETTER = 0.6f
    private const val SMALLEST_DP = 9f

    /**
     * The relative width of each of [titles] in a row of segments: equal for up to three (as always), and for
     * more, in proportion to the length of the word (plus a little for the sides).
     */
    fun shares(titles: List<String>): List<Float> =
        if (titles.size <= 3) titles.map { 1f } else titles.map { it.length + 2f }

    /** The size (dp) at which every one of [titles] fits its share of [screenWidthDp] on one line, never above [wantedDp]. */
    fun sizeDp(screenWidthDp: Int, titles: List<String>, wantedDp: Float, shares: List<Float>): Float {
        if (titles.isEmpty() || titles.size != shares.size) return wantedDp
        val total = shares.sum()
        var size = wantedDp
        titles.forEachIndexed { i, title ->
            if (title.isEmpty()) return@forEachIndexed
            val room = screenWidthDp * shares[i] / total - 2 * SIDE_DP
            size = minOf(size, room / (title.length * LETTER))
        }
        return size.coerceAtLeast(SMALLEST_DP)
    }
}

/**
 * Frame of a full-screen detail: back arrow (or X when [close]), one-line title, optional
 * actions on the right, the content, and an optional footer fixed above the keyboard
 * (white band with a top border — put the main button in it).
 * The content gets the whole remaining area and does its own scrolling and padding.
 */
@Composable
internal fun DetailScaffold(
    title: String,
    onBack: () -> Unit,
    close: Boolean = false,
    actions: @Composable RowScope.() -> Unit = {},
    footer: (@Composable ColumnScope.() -> Unit)? = null,
    content: @Composable () -> Unit
) {
    val c = Ponte.colors
    Column(Modifier.fillMaxSize().background(c.ground).imePadding()) {
        Row(
            Modifier.statusBarsPadding().fillMaxWidth().height(64.dp).padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) {
                if (close) Icon(Icons.Outlined.Close, contentDescription = "Fechar", tint = c.text)
                else Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Voltar", tint = c.text)
            }
            Text(
                title, Modifier.weight(1f).padding(horizontal = 4.dp),
                fontSize = 18.sp, fontWeight = FontWeight.Medium, color = c.text,
                maxLines = 1, overflow = TextOverflow.Ellipsis
            )
            actions()
        }
        Box(Modifier.weight(1f).fillMaxWidth()) { content() }
        if (footer != null) {
            Box(Modifier.fillMaxWidth().height(1.dp).background(c.barBorder))
            Column(
                Modifier.fillMaxWidth().background(c.surface).navigationBarsPadding()
                    .padding(start = 16.dp, top = 12.dp, end = 16.dp, bottom = 16.dp),
                content = footer
            )
        } else {
            Spacer(Modifier.navigationBarsPadding())
        }
    }
}

// ---------------------------------------------------------------- small pieces

/** White rounded card (radius 16, no shadow). Clickable when [onClick] is given. */
@Composable
internal fun PonteCard(
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    content: @Composable () -> Unit
) {
    val shape = RoundedCornerShape(16.dp)
    if (onClick != null) {
        Surface(onClick = onClick, modifier = modifier.fillMaxWidth(), shape = shape, color = Ponte.colors.surface, content = content)
    } else {
        Surface(modifier = modifier.fillMaxWidth(), shape = shape, color = Ponte.colors.surface, content = content)
    }
}

/** Small uppercase label above a group ("ATRASADAS · 2"). */
@Composable
internal fun SectionLabel(text: String, modifier: Modifier = Modifier, color: Color = Ponte.colors.muted) {
    Text(
        text.uppercase(), modifier.padding(start = 4.dp, top = 6.dp, end = 4.dp),
        style = MaterialTheme.typography.labelSmall, fontSize = 13.sp, color = color
    )
}

/** Rounded pill with short text: due dates, priority, state. */
@Composable
internal fun Pill(text: String, modifier: Modifier = Modifier, tone: Tone = Tone.NEUTRAL) {
    val (bg, fg) = toneColors(tone)
    Text(
        text, modifier.background(bg, RoundedCornerShape(50)).padding(horizontal = 10.dp, vertical = 4.dp),
        style = MaterialTheme.typography.labelMedium, color = fg, maxLines = 1
    )
}

/** Squarish tag with the kind of a document ("Ofício", "E-mail"). */
@Composable
internal fun Tag(text: String, modifier: Modifier = Modifier, tone: Tone = Tone.ACCENT) {
    val (bg, fg) = toneColors(tone)
    Text(
        text, modifier.background(bg, RoundedCornerShape(6.dp)).padding(horizontal = 8.dp, vertical = 2.dp),
        style = MaterialTheme.typography.labelMedium, color = fg, maxLines = 1, overflow = TextOverflow.Ellipsis
    )
}

/** Round check mark in a touch target (44 dp, 48 in the lists): empty ring, or green with a tick when [checked]. */
@Composable
internal fun RoundCheck(
    checked: Boolean, onClick: () -> Unit, description: String, enabled: Boolean = true,
    /** The touch target; the drawing inside stays the same. */
    size: androidx.compose.ui.unit.Dp = 44.dp
) {
    val c = Ponte.colors
    Box(
        Modifier.size(size).clip(CircleShape).clickable(enabled = enabled, role = Role.Checkbox, onClickLabel = description, onClick = onClick),
        contentAlignment = Alignment.Center
    ) {
        if (checked) {
            Box(Modifier.size(22.dp).background(c.success, CircleShape), contentAlignment = Alignment.Center) {
                Icon(Icons.Rounded.Check, contentDescription = description, tint = c.surface, modifier = Modifier.size(16.dp))
            }
        } else {
            Box(Modifier.size(22.dp).border(2.dp, c.muted, CircleShape))
        }
    }
}

/**
 * One task: round check, title, a second line, and the due pill. Used inside a card in Tarefas
 * and inside the "Tarefas" card of the ficha. Tapping the text calls [onOpen] when given.
 */
@Composable
internal fun TaskRow(
    task: TaskItem,
    secondLine: String,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
    onOpen: (() -> Unit)? = null,
    /** The project chip is left out where the project is the screen itself. */
    showProject: Boolean = true
) {
    val c = Ponte.colors
    Row(modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        RoundCheck(task.done, onToggle, if (task.done) "Reabrir a tarefa" else "Concluir a tarefa")
        Spacer(Modifier.width(4.dp))
        Column(
            Modifier.weight(1f)
                .then(if (onOpen != null) Modifier.clip(RoundedCornerShape(8.dp)).clickable(onClick = onOpen) else Modifier)
                .padding(vertical = 4.dp)
        ) {
            Text(
                task.title,
                fontSize = 16.sp, lineHeight = 21.sp,
                fontWeight = if (task.done) FontWeight.Normal else FontWeight.Medium,
                color = if (task.done) c.muted else c.text,
                textDecoration = if (task.done) TextDecoration.LineThrough else null
            )
            if (secondLine.isNotEmpty()) {
                Text(secondLine, style = MaterialTheme.typography.bodySmall, color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            // Only what is not the usual: a normal priority says nothing.
            val flag = priorityFlag(task)
            if (flag != null) Pill(flag.first, tone = flag.second, modifier = Modifier.padding(top = 4.dp))
            CategoryChips(task.categories, Modifier.padding(top = 4.dp))
            if (showProject && task.projectName.isNotEmpty()) ProjectChip(task.projectName, Modifier.padding(top = 4.dp))
        }
        if (!task.done && task.dueChip.isNotEmpty()) {
            Spacer(Modifier.width(8.dp))
            Pill(task.dueChip, tone = if (task.overdue) Tone.ERROR else if (task.dueToday) Tone.WARNING else Tone.NEUTRAL)
        }
    }
}

/** Text and tone of the priority pill of an open task, or null when it is normal (or done). */
internal fun priorityFlag(task: TaskItem): Pair<String, Tone>? {
    if (task.done) return null
    return when (task.priority) {
        "urgente" -> "Urgente" to Tone.ERROR
        "alta" -> "Prioridade alta" to Tone.WARNING
        "baixa" -> "Prioridade baixa" to Tone.NEUTRAL
        else -> null
    }
}

/** Row of filter pills that scrolls sideways. [options] are key to label. */
@Composable
internal fun FilterChips(
    options: List<Pair<String, String>>,
    selected: String,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(0.dp)
) {
    val c = Ponte.colors
    Row(
        modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(contentPadding),
        horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically
    ) {
        for ((key, label) in options) FilterPill(label, key == selected) { onSelect(key) }
    }
}

/** One filter pill: filled when [on], outlined otherwise. Give it a width limit in [modifier] to cut a long [label]. */
@Composable
internal fun FilterPill(label: String, on: Boolean, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val c = Ponte.colors
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(50),
        color = if (on) c.accent else c.surface,
        border = if (on) null else BorderStroke(1.dp, c.outline),
        modifier = modifier.height(32.dp)
    ) {
        Box(Modifier.padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
            Text(
                label, fontSize = 14.sp, fontWeight = FontWeight.Medium, color = if (on) c.onAccent else c.secondary,
                maxLines = 1, overflow = TextOverflow.Ellipsis
            )
        }
    }
}

/** Full-width main button (52 dp, pill). Shows a spinner instead of the text while [loading]. */
@Composable
internal fun PrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    loading: Boolean = false,
    icon: ImageVector? = null
) {
    val c = Ponte.colors
    Button(
        onClick = onClick, enabled = enabled && !loading,
        modifier = modifier.fillMaxWidth().height(52.dp),
        shape = RoundedCornerShape(26.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = c.accent, contentColor = c.onAccent,
            disabledContainerColor = c.chip, disabledContentColor = c.muted
        )
    ) {
        if (loading) {
            CircularProgressIndicator(Modifier.size(20.dp), color = c.muted, strokeWidth = 2.dp)
            Spacer(Modifier.width(10.dp))
        } else if (icon != null) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
        }
        Text(text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
    }
}

/** Full-width second button (52 dp, pill, outlined): the other choice beside or under [PrimaryButton]. */
@Composable
internal fun SecondaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    danger: Boolean = false
) {
    val c = Ponte.colors
    val color = if (danger) c.error else c.accent
    androidx.compose.material3.OutlinedButton(
        onClick = onClick, enabled = enabled,
        modifier = modifier.fillMaxWidth().height(52.dp),
        shape = RoundedCornerShape(26.dp),
        border = BorderStroke(1.dp, if (enabled) color else c.outline),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = color, disabledContentColor = c.muted)
    ) {
        Text(text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Centered message for an empty list, with an optional icon and action below. */
@Composable
internal fun EmptyState(
    title: String,
    modifier: Modifier = Modifier,
    text: String = "",
    icon: ImageVector? = null,
    action: (@Composable () -> Unit)? = null
) {
    val c = Ponte.colors
    Column(
        modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 40.dp),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        if (icon != null) {
            Box(Modifier.size(56.dp).background(c.chip, CircleShape), contentAlignment = Alignment.Center) {
                Icon(icon, contentDescription = null, tint = c.secondary, modifier = Modifier.size(28.dp))
            }
            Spacer(Modifier.height(4.dp))
        }
        Text(title, style = MaterialTheme.typography.titleMedium, fontSize = 16.sp, color = c.text, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        if (text.isNotEmpty()) {
            Text(text, style = MaterialTheme.typography.bodySmall, fontSize = 14.sp, lineHeight = 20.sp, color = c.muted, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        }
        if (action != null) {
            Spacer(Modifier.height(8.dp))
            action()
        }
    }
}

/** Centered spinner with a caption, while something loads. */
@Composable
internal fun LoadingBox(modifier: Modifier = Modifier, text: String = "Carregando…") {
    Column(
        modifier.fillMaxWidth().padding(40.dp),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        CircularProgressIndicator(Modifier.size(28.dp), color = Ponte.colors.accent, strokeWidth = 3.dp)
        Text(text, style = MaterialTheme.typography.bodySmall, color = Ponte.colors.muted)
    }
}

/** Message band: red for an error, amber for a warning, neutral otherwise. */
@Composable
internal fun Notice(text: String, modifier: Modifier = Modifier, tone: Tone = Tone.ERROR) {
    val (bg, fg) = toneColors(tone)
    Text(
        text, modifier.fillMaxWidth().background(bg, RoundedCornerShape(12.dp)).padding(horizontal = 14.dp, vertical = 10.dp),
        fontSize = 14.sp, lineHeight = 20.sp, color = fg
    )
}

// ---------------------------------------------------------------- form fields

/** Text field with its label above (white box, thin border, brand border when focused). */
@Composable
internal fun LabeledField(
    label: String,
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    placeholder: String = "",
    error: String = "",
    singleLine: Boolean = true,
    minLines: Int = 1,
    password: Boolean = false,
    enabled: Boolean = true,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default
) {
    val c = Ponte.colors
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(label, style = FieldLabelStyle, color = c.secondary)
        OutlinedTextField(
            value = value, onValueChange = onValueChange,
            modifier = Modifier.fillMaxWidth(),
            enabled = enabled,
            textStyle = MaterialTheme.typography.bodyLarge,
            placeholder = if (placeholder.isEmpty()) null else ({ Text(placeholder, color = if (enabled) c.muted else c.muted.copy(alpha = 0.5f)) }),
            isError = error.isNotEmpty(),
            singleLine = singleLine,
            minLines = if (singleLine) 1 else minLines,
            visualTransformation = if (password) PasswordVisualTransformation() else VisualTransformation.None,
            keyboardOptions = keyboardOptions,
            shape = RoundedCornerShape(12.dp),
            colors = OutlinedTextFieldDefaults.colors(
                focusedContainerColor = c.surface, unfocusedContainerColor = c.surface,
                disabledContainerColor = c.surface, errorContainerColor = c.surface,
                focusedBorderColor = c.accent, unfocusedBorderColor = c.outline, errorBorderColor = c.error,
                focusedTextColor = c.text, unfocusedTextColor = c.text, cursorColor = c.accent
            )
        )
        if (error.isNotEmpty()) Text(error, style = MaterialTheme.typography.bodySmall, color = c.error)
    }
}

/** A box that looks like a field but is tapped (date, choice). */
@Composable
private fun FieldFrame(label: String, onClick: () -> Unit, modifier: Modifier, content: @Composable RowScope.() -> Unit) {
    val c = Ponte.colors
    Column(modifier, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(label, style = FieldLabelStyle, color = c.secondary)
        Row(
            Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(12.dp))
                .background(c.surface).border(1.dp, c.outline, RoundedCornerShape(12.dp))
                .clickable(onClick = onClick).padding(start = 16.dp, end = 12.dp),
            verticalAlignment = Alignment.CenterVertically, content = content
        )
    }
}

/** Choice among a few options, shown as a field that opens a menu. */
@Composable
internal fun ChoiceField(
    label: String,
    options: List<Choice>,
    selected: String,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val c = Ponte.colors
    var open by remember { mutableStateOf(false) }
    Box(modifier) {
        FieldFrame(label, { open = true }, Modifier.fillMaxWidth()) {
            Text(
                options.firstOrNull { it.key == selected }?.label ?: options.firstOrNull()?.label ?: "",
                Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, color = c.text,
                maxLines = 1, overflow = TextOverflow.Ellipsis
            )
            Icon(Icons.Outlined.ExpandMore, contentDescription = null, tint = c.secondary)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for (o in options) {
                DropdownMenuItem(
                    text = { Text(o.label, fontSize = 16.sp, fontWeight = if (o.key == selected) FontWeight.SemiBold else FontWeight.Normal) },
                    onClick = { open = false; onSelect(o.key) }
                )
            }
        }
    }
}

/**
 * Date in one field: shows dd/mm/aaaa (or [emptyText]) and opens the calendar. [iso] and the
 * value given to [onChange] are AAAA-MM-DD, or empty for no date.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DateField(
    label: String,
    iso: String,
    onChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    emptyText: String = "Sem data"
) {
    val c = Ponte.colors
    var open by remember { mutableStateOf(false) }
    FieldFrame(label, { open = true }, modifier) {
        Text(
            if (iso.isEmpty()) emptyText else formatDocDate(iso),
            Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge,
            color = if (iso.isEmpty()) c.muted else c.text, maxLines = 1
        )
        Icon(Icons.Outlined.CalendarToday, contentDescription = null, tint = c.secondary, modifier = Modifier.size(20.dp))
    }
    if (open) {
        val initial = try {
            LocalDate.parse(iso).atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()
        } catch (_: Throwable) {
            null
        }
        val state = rememberDatePickerState(initialSelectedDateMillis = initial)
        DatePickerDialog(
            onDismissRequest = { open = false },
            confirmButton = {
                TextButton(onClick = {
                    open = false
                    state.selectedDateMillis?.let {
                        onChange(Instant.ofEpochMilli(it).atZone(ZoneOffset.UTC).toLocalDate().toString())
                    }
                }) { Text("Confirmar") }
            },
            dismissButton = {
                Row {
                    if (iso.isNotEmpty()) TextButton(onClick = { open = false; onChange("") }) { Text(emptyText) }
                    TextButton(onClick = { open = false }) { Text("Cancelar") }
                }
            }
        ) { DatePicker(state = state, showModeToggle = false) }
    }
}

/**
 * Like BackHandler, but harmless where there is no Activity behind the composition
 * (the screens rendered on the computer for review).
 */
@Composable
internal fun SafeBackHandler(enabled: Boolean = true, onBack: () -> Unit) {
    if (androidx.activity.compose.LocalOnBackPressedDispatcherOwner.current != null) {
        androidx.activity.compose.BackHandler(enabled, onBack)
    }
}

/** For interaction sources that must not show a ripple. */
@Composable
internal fun rememberNoRipple() = remember { MutableInteractionSource() }
