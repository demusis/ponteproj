package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropUp
import androidx.compose.material.icons.filled.KeyboardDoubleArrowUp
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.FilterList
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.FloatingActionButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshContainer
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull

// The flat list language: rows on the page (no card, no outline), a hairline between them inset to the text,
// a round "+" button, pull to refresh, and the filter sheet.

/** Where the text of a task row starts (the round check and its gaps): the hairline is inset to it. */
internal val TASK_TEXT_START = 56.dp

/** Page margin of the flat rows. */
internal val ROW_MARGIN = 16.dp

/** The hairline between two flat rows, [inset] from the start. */
@Composable
internal fun RowDivider(inset: Dp = ROW_MARGIN) {
    HorizontalDivider(Modifier.padding(start = inset), thickness = 1.dp, color = Ponte.colors.divider)
}

/**
 * A flat row of a list: at least 56 dp tall, the press covers the whole width, and a hairline under it. [content]
 * is laid out in a Row with the page margin at both sides.
 */
@Composable
internal fun FlatRow(
    onClick: (() -> Unit)?,
    modifier: Modifier = Modifier,
    inset: Dp = ROW_MARGIN,
    divider: Boolean = true,
    content: @Composable () -> Unit
) {
    Column(modifier.fillMaxWidth()) {
        Box(
            Modifier.fillMaxWidth().heightIn(min = 56.dp)
                .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
                .padding(horizontal = ROW_MARGIN, vertical = 8.dp),
            contentAlignment = Alignment.CenterStart
        ) { content() }
        if (divider) RowDivider(inset)
    }
}

/** The round "+" (or other icon) button of a list: [description] is what the screen reader says. */
@Composable
internal fun PonteFab(icon: ImageVector, description: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    FloatingActionButton(
        onClick = onClick,
        modifier = modifier.padding(16.dp),
        shape = CircleShape,
        containerColor = c.accent, contentColor = c.onAccent,
        elevation = FloatingActionButtonDefaults.elevation(2.dp, 2.dp, 2.dp, 2.dp)
    ) { Icon(icon, contentDescription = description) }
}

/** Bottom padding of a list that has a round button over it: the last row can be lifted clear of it. */
internal val FAB_CLEARANCE = 88.dp

// ---------------------------------------------------------------- task row

/**
 * One task as a flat row: the round check (48 dp), the title (two lines at most), one muted second line, and the
 * deadline at the end. A tap anywhere else on the row opens it.
 */
@Composable
internal fun TaskListRow(
    task: TaskItem,
    onToggle: () -> Unit,
    onOpen: () -> Unit,
    modifier: Modifier = Modifier,
    /** The project is left out where the project is the screen itself. */
    showProject: Boolean = true
) {
    val c = Ponte.colors
    val names = CategoryRules.resolve(task.categories, LocalCategories.current).map { it.name }
    Column(modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = 56.dp)
                .semantics(mergeDescendants = true) { contentDescription = TaskRowText.description(task, names, showProject) }
                .clickable(onClick = onOpen)
                .padding(start = 4.dp, end = ROW_MARGIN, top = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            RoundCheck(task.done, onToggle, if (task.done) "Reabrir a tarefa" else "Concluir a tarefa", size = 48.dp)
            Spacer(Modifier.width(4.dp))
            Column(Modifier.weight(1f).padding(vertical = 4.dp)) {
                Row(verticalAlignment = Alignment.Top) {
                    Text(
                        task.title, Modifier.weight(1f),
                        fontSize = 16.sp, lineHeight = 21.sp,
                        fontWeight = if (task.done) FontWeight.Normal else FontWeight.Medium,
                        color = if (task.done) c.muted else c.text,
                        textDecoration = if (task.done) androidx.compose.ui.text.style.TextDecoration.LineThrough else null,
                        maxLines = 2, overflow = TextOverflow.Ellipsis
                    )
                    if (!task.done && task.dueChip.isNotEmpty()) {
                        Spacer(Modifier.width(8.dp))
                        DueLabel(task)
                    }
                }
                TaskSecondaryLine(task, showProject)
            }
        }
        RowDivider(TASK_TEXT_START)
    }
}

/** The deadline of an open task: late and today keep their coloured pill, the others are plain muted text. */
@Composable
private fun DueLabel(task: TaskItem) = DueMark(task.dueChip, task.overdue, task.dueToday)

/** A deadline at the end of a flat row: late (red) and today (amber) keep their pill, any other date is plain muted text. */
@Composable
internal fun DueMark(chip: String, late: Boolean, today: Boolean, modifier: Modifier = Modifier) {
    when {
        late -> Pill(chip, modifier, tone = Tone.ERROR)
        today -> Pill(chip, modifier, tone = Tone.WARNING)
        else -> Text(chip, modifier.padding(top = 2.dp), style = MaterialTheme.typography.bodySmall, color = Ponte.colors.muted, maxLines = 1)
    }
}

/**
 * The muted second line of a task row, always one line: the kind and the document number, "▲ alta" when the
 * priority is above normal, the categories as a coloured dot and a name (as many as fit, then "+N"), and the
 * project. What does not fit is left out, never wrapped.
 */
@Composable
internal fun TaskSecondaryLine(task: TaskItem, showProject: Boolean, modifier: Modifier = Modifier) {
    val lead = TaskRowText.lead(task)
    val mark = TaskRowText.mark(task)
    val categories = CategoryRules.resolve(task.categories, LocalCategories.current)
    val project = if (showProject) task.projectName else ""
    if (lead.isEmpty() && mark == null && categories.isEmpty() && project.isEmpty()) return
    val density = LocalDensity.current
    val minProject = with(density) { 64.dp.roundToPx() }
    val moreGap = with(density) { 4.dp.roundToPx() }
    SubcomposeLayout(modifier.fillMaxWidth()) { constraints ->
        val max = constraints.maxWidth
        val open = Constraints()
        // The priority is never left out: the first text gives way to it.
        val markPlaceable = mark?.let { m ->
            subcompose("mark") { MarkAtom(m, separator = lead.isNotEmpty()) }.first().measure(open)
        }
        val leadPlaceable = if (lead.isEmpty()) null else subcompose("lead") {
            SecondaryText(lead, Modifier)
        }.first().measure(Constraints(maxWidth = (max - (markPlaceable?.width ?: 0)).coerceAtLeast(0)))
        var x = (leadPlaceable?.width ?: 0) + (markPlaceable?.width ?: 0)
        var anything = leadPlaceable != null || markPlaceable != null

        val catPlaceables = categories.mapIndexed { i, cat ->
            subcompose("cat$i") { CategoryAtom(cat, separator = anything || i > 0) }.first().measure(open)
        }
        val probe = if (categories.isEmpty()) 0 else subcompose("more-probe") {
            SecondaryText("+${categories.size}", Modifier)
        }.first().measure(open).width
        val shown = TaskRowText.fitting(catPlaceables.map { it.width.toFloat() }, (max - x).toFloat(), 0f, (probe + moreGap).toFloat())
        val shownPlaceables = catPlaceables.take(shown)
        x += shownPlaceables.sumOf { it.width }
        val left = categories.size - shown
        val morePlaceable = if (left > 0 && probe + moreGap <= max - x) {
            subcompose("more") { SecondaryText("+$left", Modifier.padding(start = 4.dp)) }.first().measure(open)
        } else null
        x += morePlaceable?.width ?: 0
        anything = anything || shownPlaceables.isNotEmpty() || morePlaceable != null

        val projectRoom = max - x
        val projectPlaceable = if (project.isNotEmpty() && projectRoom >= minProject) {
            subcompose("project") { ProjectAtom(project, separator = anything) }.first().measure(Constraints(maxWidth = projectRoom))
        } else null

        val all = listOfNotNull(leadPlaceable, markPlaceable) + shownPlaceables + listOfNotNull(morePlaceable, projectPlaceable)
        val height = all.maxOfOrNull { it.height } ?: 0
        layout(max, height) {
            var at = 0
            for (p in all) {
                p.placeRelative(at, (height - p.height) / 2)
                at += p.width
            }
        }
    }
}

@Composable
private fun SecondaryText(text: String, modifier: Modifier) {
    Text(
        text, modifier, style = MaterialTheme.typography.bodySmall, color = Ponte.colors.muted,
        maxLines = 1, overflow = TextOverflow.Ellipsis
    )
}

@Composable
private fun Separator() {
    Text("·", Modifier.padding(horizontal = 5.dp), style = MaterialTheme.typography.bodySmall, color = Ponte.colors.muted, maxLines = 1)
}

@Composable
private fun MarkAtom(mark: TaskRowText.Mark, separator: Boolean) {
    val c = Ponte.colors
    val tint = if (mark.key == "urgente") c.error else c.warning
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (separator) Separator()
        Icon(
            if (mark.key == "urgente") Icons.Filled.KeyboardDoubleArrowUp else Icons.Filled.ArrowDropUp,
            contentDescription = null, tint = tint, modifier = Modifier.size(if (mark.key == "urgente") 14.dp else 20.dp)
        )
        Text(mark.word, style = MaterialTheme.typography.bodySmall, fontWeight = FontWeight.Medium, color = tint, maxLines = 1)
    }
}

@Composable
private fun CategoryAtom(category: Category, separator: Boolean) {
    val c = Ponte.colors
    val dot = if (category.archived) c.muted else Color(CategoryRules.colors(category.color, c.dark).dot)
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (separator) Separator()
        Box(Modifier.size(8.dp).background(dot, CircleShape))
        Spacer(Modifier.width(4.dp))
        SecondaryText(category.name, Modifier.widthIn(max = 120.dp))
    }
}

@Composable
private fun ProjectAtom(name: String, separator: Boolean) {
    val c = Ponte.colors
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (separator) Separator()
        Icon(Icons.Outlined.Folder, contentDescription = null, tint = c.muted, modifier = Modifier.size(13.dp))
        Spacer(Modifier.width(3.dp))
        SecondaryText(name, Modifier.weight(1f, fill = false))
    }
}

// ---------------------------------------------------------------- pull to refresh

/**
 * Pull the list down to sync: the same request as "Sincronizar agora". The spinner holds while the sync runs
 * ([syncing]); a sync that cannot start (no Wi-Fi, not set up) is explained at the bottom by the controller and
 * the spinner goes away at once, so it never spins for ever.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun PullToSync(syncing: Boolean, onRefresh: () -> Unit, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val c = Ponte.colors
    val state = rememberPullToRefreshState()
    val running by rememberUpdatedState(syncing)
    val request by rememberUpdatedState(onRefresh)
    if (state.isRefreshing) {
        LaunchedEffect(Unit) {
            request()
            withTimeoutOrNull(PullRules.MAX_SPIN_MS) {
                // The sync shows itself within a moment, or it was refused and there is nothing to wait for.
                val started = withTimeoutOrNull(PullRules.START_WAIT_MS) { snapshotFlow { running }.first { it } } == true
                if (started) snapshotFlow { running }.first { !it }
            }
            state.endRefresh()
        }
    }
    Box(modifier.nestedScroll(state.nestedScrollConnection)) {
        content()
        PullToRefreshContainer(
            state, Modifier.align(Alignment.TopCenter),
            containerColor = c.surface, contentColor = c.accent
        )
    }
}

// ---------------------------------------------------------------- filters

/** The one-line summary over a filtered list, with its "Limpar". */
@Composable
internal fun FilterSummaryRow(text: String, onClear: () -> Unit, modifier: Modifier = Modifier) {
    val c = Ponte.colors
    Column(modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(start = ROW_MARGIN, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(Icons.Outlined.FilterList, contentDescription = null, tint = c.secondary, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(
                text, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium,
                color = c.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis
            )
            TextButton(onClick = onClear, modifier = Modifier.semantics { contentDescription = "Limpar o filtro" }) {
                Text("Limpar", color = c.accent, fontWeight = FontWeight.SemiBold)
            }
        }
        RowDivider(0.dp)
    }
}

/**
 * The body of the filter sheet: the categories (any of the ticked ones) and the projects (one), each only when
 * [categories] / [projects] is given, and "Limpar" while any filter is on. A choice applies at once.
 */
@Composable
internal fun FilterSheetContent(
    categories: List<Category>?,
    categoryChosen: Set<String>,
    onCategory: (Set<String>) -> Unit,
    projects: List<ProjectItem>?,
    projectChosen: String,
    onProject: (String) -> Unit,
    onClear: () -> Unit,
    onDone: () -> Unit
) {
    val p = Ponte.colors
    val active = FilterSummary.isActive(categoryChosen, projectChosen)
    Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp).padding(bottom = 16.dp)) {
        Row(Modifier.fillMaxWidth().padding(bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Filtros", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium, color = p.text)
            if (active) TextButton(onClick = onClear) { Text("Limpar", color = p.accent, fontWeight = FontWeight.SemiBold) }
        }
        Column(Modifier.heightIn(max = 440.dp).verticalScroll(rememberScrollState())) {
            if (categories != null) {
                SectionLabel("Categoria", Modifier.padding(start = 0.dp, top = 8.dp, bottom = 2.dp))
                PickRow(CategoryRules.NONE in categoryChosen, true, "Sem categoria", { onCategory(CategoryRules.toggleFilter(categoryChosen, CategoryRules.NONE)) }) {
                    Box(Modifier.size(10.dp).background(p.outline, CircleShape))
                    Spacer(Modifier.width(12.dp))
                    Text("Sem categoria", Modifier.weight(1f), fontSize = 16.sp, color = p.text)
                }
                for (c in CategoryRules.filterOptions(categories, categoryChosen)) {
                    PickRow(c.uid in categoryChosen, true, c.name, { onCategory(CategoryRules.toggleFilter(categoryChosen, c.uid)) }) {
                        CategoryDot(c)
                        Text(c.name, Modifier.weight(1f), fontSize = 16.sp, color = p.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            if (projects != null) {
                SectionLabel("Projeto", Modifier.padding(start = 0.dp, top = 12.dp, bottom = 2.dp))
                PickRow(projectChosen.isEmpty(), true, "Todos os projetos", { onProject("") }) {
                    Text("Todos os projetos", Modifier.weight(1f), fontSize = 16.sp, color = p.text)
                }
                PickRow(projectChosen == ProjectRules.NONE, true, "Sem projeto", { onProject(ProjectRules.NONE) }) {
                    Text("Sem projeto", Modifier.weight(1f), fontSize = 16.sp, color = p.text)
                }
                for (project in ProjectRules.filterOptions(projects)) {
                    val archived = project.status == ProjectRules.ARCHIVED
                    PickRow(project.uid == projectChosen, true, project.name, { onProject(project.uid) }) {
                        Text(
                            if (archived) "${project.name} (arquivado)" else project.name, Modifier.weight(1f),
                            fontSize = 16.sp, color = if (archived) p.muted else p.text, maxLines = 1, overflow = TextOverflow.Ellipsis
                        )
                    }
                }
            }
        }
        Spacer(Modifier.size(12.dp))
        PrimaryButton("Pronto", onClick = onDone)
    }
}

/** The filter sheet over the list. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun FilterSheet(onDismiss: () -> Unit, content: @Composable () -> Unit) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Ponte.colors.surface
    ) { content() }
}

// ---------------------------------------------------------------- the area menu

/** The entries of the area menu (current one ticked). Drawn in the real dropdown, and alone in the pictures. */
@Composable
internal fun AreaMenuItems(current: Int, onPick: (Int) -> Unit) {
    val c = Ponte.colors
    for ((id, label) in AreaSelector.options()) {
        val on = id == current
        DropdownMenuItem(
            text = { Text(label, fontSize = 16.sp, fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal, color = c.text) },
            onClick = { onPick(id) },
            trailingIcon = if (on) ({ Icon(Icons.Outlined.Check, contentDescription = null, tint = c.accent) }) else null,
            modifier = Modifier.semantics { selected = on; contentDescription = AreaSelector.optionDescription(id, current) }
        )
    }
}
