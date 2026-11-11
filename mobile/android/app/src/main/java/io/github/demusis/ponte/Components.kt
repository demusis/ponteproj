package io.github.demusis.ponte

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
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
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
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
    n >= 1024 * 1024 -> String.format("%.1f MB", n / 1048576.0)
    n >= 1024 -> String.format("%d KB", n / 1024)
    else -> "$n bytes"
}

// ---------------------------------------------------------------- frames

/**
 * Frame of the three areas: large title with the one-line sync status, the sync and settings
 * buttons, a thin progress bar while syncing, the content, and the bottom bar.
 */
@Composable
internal fun MainShell(
    state: ShellState,
    onTab: (Int) -> Unit,
    onSync: () -> Unit,
    onSettings: () -> Unit,
    onStatus: () -> Unit,
    onSearch: (() -> Unit)? = null,
    content: @Composable () -> Unit
) {
    val c = Ponte.colors
    val titles = listOf("Hoje", "Tarefas", "Documentos", "E-mail")
    // The Tarefas area shows its Atendimentos list under another title.
    val heading = if (state.tab == 1 && state.attendances) "Atendimentos" else titles.getOrElse(state.tab) { "" }
    Column(Modifier.fillMaxSize().background(c.ground)) {
        Row(
            Modifier.statusBarsPadding().fillMaxWidth().padding(start = 20.dp, top = 12.dp, end = 8.dp, bottom = 6.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    heading,
                    fontSize = 26.sp, lineHeight = 30.sp, fontWeight = FontWeight.SemiBold, color = c.text
                )
                Row(
                    Modifier.clip(RoundedCornerShape(8.dp))
                        .clickable(onClickLabel = "Ver detalhes da sincronização", onClick = onStatus)
                        .padding(top = 4.dp, bottom = 4.dp, end = 8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    val dot = when (state.statusKind) {
                        StatusKind.OK -> c.success
                        StatusKind.WAITING -> c.warning
                        StatusKind.ERROR -> c.error
                        StatusKind.NEUTRAL -> c.outline
                    }
                    Box(Modifier.size(8.dp).background(dot, CircleShape))
                    Spacer(Modifier.width(6.dp))
                    Text(
                        state.statusLine.ifEmpty { "Ainda não sincronizado" },
                        style = MaterialTheme.typography.bodySmall,
                        color = if (state.statusKind == StatusKind.ERROR) c.error else c.muted,
                        maxLines = 1, overflow = TextOverflow.Ellipsis
                    )
                }
            }
            // Hoje, Tarefas and Documentos: one search over the whole library.
            if (onSearch != null && state.tab in 0..2) {
                IconButton(onClick = onSearch) {
                    Icon(Icons.Outlined.Search, contentDescription = "Buscar em tudo", tint = c.secondary)
                }
            }
            IconButton(onClick = onSync, enabled = state.syncEnabled && !state.syncing) {
                Icon(
                    Icons.Outlined.Sync, contentDescription = "Sincronizar agora",
                    tint = if (state.syncEnabled && !state.syncing) c.accent else c.outline
                )
            }
            IconButton(onClick = onSettings) {
                Icon(Icons.Outlined.Tune, contentDescription = "Abrir Ajustes", tint = c.secondary)
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
        Box(Modifier.fillMaxWidth().height(1.dp).background(c.barBorder))
        NavigationBar(containerColor = c.surface, tonalElevation = 0.dp) {
            val icons = listOf(Icons.Outlined.Today, Icons.Outlined.CheckCircle, Icons.Outlined.Description, Icons.Outlined.Mail)
            val itemColors = NavigationBarItemDefaults.colors(
                selectedIconColor = c.onAccentContainer,
                selectedTextColor = c.onAccentContainer,
                indicatorColor = c.accentContainer,
                unselectedIconColor = c.secondary,
                unselectedTextColor = c.secondary
            )
            val labelSize = navLabelSize(titles)
            titles.forEachIndexed { i, title ->
                NavigationBarItem(
                    selected = state.tab == i,
                    onClick = { onTab(i) },
                    icon = {
                        if (i == 0 && state.badge > 0) {
                            // One node for the icon and the number: the reader says what the number means.
                            val meaning = PhoneWords.plural(
                                state.badge, "tarefa atrasada ou para hoje", "tarefas atrasadas ou para hoje"
                            )
                            BadgedBox(modifier = Modifier.clearAndSetSemantics { contentDescription = meaning }, badge = {
                                Badge(containerColor = c.error, contentColor = if (c.dark) c.errorContainer else Color.White) {
                                    Text(if (state.badge > 99) "99+" else state.badge.toString())
                                }
                            }) { Icon(icons[i], contentDescription = null) }
                        } else {
                            Icon(icons[i], contentDescription = null)
                        }
                    },
                    label = {
                        // Always one line: a name broken in two ("Documen-tos") makes the bar ragged.
                        Text(
                            title, fontSize = labelSize, letterSpacing = 0.sp,
                            fontWeight = if (state.tab == i) FontWeight.Bold else FontWeight.Medium,
                            maxLines = 1, softWrap = false, overflow = TextOverflow.Clip
                        )
                    },
                    colors = itemColors
                )
            }
        }
    }
}

/**
 * The size of the names under the icons of the bottom bar: the usual 12, but never so
 * large that the longest name stops fitting its share of the bar on one line. It is worked
 * out in dp, so a large font setting on the phone (or a narrow screen) shrinks the names
 * instead of breaking them; every name gets the same size.
 */
@Composable
private fun navLabelSize(titles: List<String>): TextUnit {
    val density = LocalDensity.current
    val screen = LocalConfiguration.current.screenWidthDp
    val wanted = 12f * density.fontScale
    return with(density) { NavLabel.sizeDp(screen, titles, wanted).dp.toSp() }
}

/** The arithmetic of [navLabelSize], apart so it can be tested. */
internal object NavLabel {
    /** Room an item keeps free at its sides, and how wide a bold letter is for its size. */
    private const val SIDE_DP = 6f
    private const val LETTER = 0.6f
    private const val SMALLEST_DP = 9f

    fun sizeDp(screenWidthDp: Int, titles: List<String>, wantedDp: Float): Float {
        val longest = titles.maxOfOrNull { it.length } ?: return wantedDp
        if (longest == 0 || titles.isEmpty()) return wantedDp
        val room = screenWidthDp.toFloat() / titles.size - 2 * SIDE_DP
        val fits = room / (longest * LETTER)
        return wantedDp.coerceAtMost(fits).coerceAtLeast(SMALLEST_DP)
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
internal fun Pill(text: String, tone: Tone = Tone.NEUTRAL, modifier: Modifier = Modifier) {
    val (bg, fg) = toneColors(tone)
    Text(
        text, modifier.background(bg, RoundedCornerShape(50)).padding(horizontal = 10.dp, vertical = 4.dp),
        style = MaterialTheme.typography.labelMedium, color = fg, maxLines = 1
    )
}

/** Squarish tag with the kind of a document ("Ofício", "E-mail"). */
@Composable
internal fun Tag(text: String, tone: Tone = Tone.ACCENT, modifier: Modifier = Modifier) {
    val (bg, fg) = toneColors(tone)
    Text(
        text, modifier.background(bg, RoundedCornerShape(6.dp)).padding(horizontal = 8.dp, vertical = 2.dp),
        style = MaterialTheme.typography.labelMedium, color = fg, maxLines = 1, overflow = TextOverflow.Ellipsis
    )
}

/** Round check mark in a 44 dp touch target: empty ring, or green with a tick when [checked]. */
@Composable
internal fun RoundCheck(checked: Boolean, onClick: () -> Unit, description: String, enabled: Boolean = true) {
    val c = Ponte.colors
    Box(
        Modifier.size(44.dp).clip(CircleShape).clickable(enabled = enabled, role = Role.Checkbox, onClickLabel = description, onClick = onClick),
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
    onOpen: (() -> Unit)? = null
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
            if (flag != null) Pill(flag.first, flag.second, Modifier.padding(top = 4.dp))
        }
        if (!task.done && task.dueChip.isNotEmpty()) {
            Spacer(Modifier.width(8.dp))
            Pill(task.dueChip, if (task.overdue) Tone.ERROR else if (task.dueToday) Tone.WARNING else Tone.NEUTRAL)
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
        for ((key, label) in options) {
            val on = key == selected
            Surface(
                onClick = { onSelect(key) },
                shape = RoundedCornerShape(50),
                color = if (on) c.accent else c.surface,
                border = if (on) null else BorderStroke(1.dp, c.outline),
                modifier = Modifier.height(32.dp)
            ) {
                Box(Modifier.padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
                    Text(label, fontSize = 14.sp, fontWeight = FontWeight.Medium, color = if (on) c.onAccent else c.secondary)
                }
            }
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
internal fun LoadingBox(text: String = "Carregando…", modifier: Modifier = Modifier) {
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
internal fun Notice(text: String, tone: Tone = Tone.ERROR, modifier: Modifier = Modifier) {
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
