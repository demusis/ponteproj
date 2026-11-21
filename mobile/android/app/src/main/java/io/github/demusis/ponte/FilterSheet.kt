package io.github.demusis.ponte

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The body of the filter sheet. The categories are pills (any of the ticked ones); Projeto and Documento are
 * sections that open one at a time and show their choice while closed (one choice each; tapping it again clears it).
 * Each part shows only when [categories] / [projects] / [documents] is given. A choice applies at once, and the
 * button at the foot says how many open tasks are left ([resultCount]; null where nothing is counted).
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
    onDone: () -> Unit,
    documents: List<Doc>? = null,
    documentChosen: Long? = null,
    onDocument: (Long?) -> Unit = {},
    resultCount: Int? = null,
    /** The section open at first; null lets [FilterSections.initial] decide (the pictures set it). */
    initialSection: FilterSection? = null
) {
    val p = Ponte.colors
    val active = FilterSummary.isActive(categoryChosen, projectChosen, documentChosen)
    var open by rememberSaveable {
        mutableStateOf(initialSection ?: FilterSections.initial(categories != null, projects != null, documents != null))
    }
    Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 20.dp).padding(bottom = 16.dp)) {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Filtros", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold, color = p.text)
            if (active) TextButton(onClick = onClear) { Text("Limpar", color = p.accent, fontWeight = FontWeight.SemiBold) }
        }
        Column(Modifier.heightIn(max = 440.dp).verticalScroll(rememberScrollState())) {
            if (categories != null) {
                SheetLabel("Categoria", Modifier.padding(top = 8.dp, bottom = 10.dp))
                CategoryPills(categories, categoryChosen, onCategory)
                Spacer(Modifier.height(14.dp))
            }
            if (projects != null) {
                if (categories != null) RowDivider(0.dp)
                SectionHeader(
                    "Projeto", FilterSummary.sectionValue(FilterSummary.projectName(projectChosen, projects)),
                    chosen = projectChosen.isNotEmpty(), open = open == FilterSection.PROJECT
                ) { open = FilterSections.toggle(open, FilterSection.PROJECT) }
                if (open == FilterSection.PROJECT) {
                    ChoiceRow("Sem projeto", on = projectChosen == ProjectRules.NONE, quiet = true) {
                        onProject(FilterSections.pick(projectChosen, ProjectRules.NONE, ""))
                    }
                    for (project in ProjectRules.filterOptions(projects)) {
                        val archived = project.status == ProjectRules.ARCHIVED
                        ChoiceRow(
                            if (archived) "${project.name} (arquivado)" else project.name,
                            on = project.uid == projectChosen, quiet = archived
                        ) { onProject(FilterSections.pick(projectChosen, project.uid, "")) }
                    }
                    Spacer(Modifier.height(6.dp))
                }
            }
            if (documents != null) {
                if (categories != null || projects != null) RowDivider(0.dp)
                SectionHeader(
                    "Documento", FilterSummary.sectionValue(TaskDocumentFilter.shortLabel(documentChosen, documents)),
                    chosen = documentChosen != null, open = open == FilterSection.DOCUMENT
                ) { open = FilterSections.toggle(open, FilterSection.DOCUMENT) }
                if (open == FilterSection.DOCUMENT) {
                    var query by rememberSaveable { mutableStateOf("") }
                    if (TaskDocumentFilter.showSearch(documents)) {
                        QuietSearch(query, { query = it }, "Buscar por número ou título", Modifier.padding(bottom = 6.dp))
                    }
                    val options = remember(documents, query, documentChosen) {
                        TaskDocumentFilter.sheetOptions(documents, query, documentChosen)
                    }
                    if (query.isBlank()) {
                        ChoiceRow("Sem documento", on = documentChosen == 0L, quiet = true) {
                            onDocument(FilterSections.pick(documentChosen, 0L, null))
                        }
                    }
                    for (doc in options.take(TaskDocumentFilter.MAX_SHOWN)) {
                        ChoiceRow(
                            TaskDocumentFilter.headline(doc), on = doc.id == documentChosen,
                            detail = TaskDocumentFilter.detail(doc)
                        ) { onDocument(FilterSections.pick(documentChosen, doc.id, null)) }
                    }
                    val hint = when {
                        options.size > TaskDocumentFilter.MAX_SHOWN -> "Mais ${options.size - TaskDocumentFilter.MAX_SHOWN}: refine a busca."
                        options.isEmpty() && query.isNotBlank() -> "Nenhum documento encontrado."
                        options.isEmpty() -> "Nenhum documento com tarefas em aberto."
                        else -> ""
                    }
                    if (hint.isNotEmpty()) Text(hint, Modifier.padding(vertical = 10.dp), fontSize = 14.sp, color = p.muted)
                    Spacer(Modifier.height(6.dp))
                }
            }
        }
        Spacer(Modifier.size(12.dp))
        PrimaryButton(FilterSummary.doneLabel(resultCount), onClick = onDone)
    }
}

/** The label of a part of the sheet, on the same margin as its rows. */
@Composable
private fun SheetLabel(text: String, modifier: Modifier = Modifier) {
    Text(text.uppercase(), modifier, style = MaterialTheme.typography.labelSmall, fontSize = 13.sp, color = Ponte.colors.muted)
}

/** The categories as pills that wrap; "Sem categoria" comes last. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun CategoryPills(categories: List<Category>, chosen: Set<String>, onCategory: (Set<String>) -> Unit) {
    val p = Ponte.colors
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (c in CategoryRules.filterOptions(categories, chosen)) {
            val dot = if (c.archived) p.muted else Color(CategoryRules.colors(c.color, p.dark).dot)
            CategoryPill(c.name, c.uid in chosen, dot) { onCategory(CategoryRules.toggleFilter(chosen, c.uid)) }
        }
        CategoryPill("Sem categoria", CategoryRules.NONE in chosen, null) {
            onCategory(CategoryRules.toggleFilter(chosen, CategoryRules.NONE))
        }
    }
}

/** One category pill: its colour dot (a check while [on]) and its name; soft brand background while [on]. */
@Composable
private fun CategoryPill(name: String, on: Boolean, dot: Color?, onClick: () -> Unit) {
    val p = Ponte.colors
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(50),
        color = if (on) p.accentContainer else p.surface,
        border = BorderStroke(1.dp, if (on) p.accent else p.outline),
        modifier = Modifier.height(36.dp).widthIn(max = 240.dp).semantics {
            role = Role.Checkbox; selected = on; contentDescription = name
            stateDescription = if (on) "Marcada" else "Não marcada"
        }
    ) {
        Row(Modifier.padding(start = 10.dp, end = 14.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(16.dp), contentAlignment = Alignment.Center) {
                when {
                    on -> Icon(Icons.Rounded.Check, contentDescription = null, tint = p.accent, modifier = Modifier.size(16.dp))
                    dot != null -> Box(Modifier.size(10.dp).background(dot, CircleShape))
                    else -> Box(Modifier.size(10.dp).border(1.5.dp, p.muted, CircleShape))
                }
            }
            Spacer(Modifier.width(6.dp))
            Text(
                name, fontSize = 14.sp, fontWeight = FontWeight.Medium, color = if (on) p.onAccentContainer else p.text,
                maxLines = 1, overflow = TextOverflow.Ellipsis
            )
        }
    }
}

/** The line of a section that opens: its label, the choice (or "Todos") and the arrow. */
@Composable
private fun SectionHeader(label: String, value: String, chosen: Boolean, open: Boolean, onClick: () -> Unit) {
    val p = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp).clickable(onClick = onClick).semantics {
            role = Role.Button; contentDescription = "$label: $value"
            stateDescription = if (open) "Aberto" else "Fechado"
        },
        verticalAlignment = Alignment.CenterVertically
    ) {
        SheetLabel(label)
        Spacer(Modifier.width(16.dp))
        Text(
            value, Modifier.weight(1f), fontSize = 15.sp, color = if (chosen) p.accent else p.muted,
            fontWeight = if (chosen) FontWeight.SemiBold else FontWeight.Normal,
            maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.End
        )
        Spacer(Modifier.width(6.dp))
        Icon(if (open) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, contentDescription = null, tint = p.secondary)
    }
}

/** A choice of a one-choice section: the check shows only on the chosen one. [quiet]: second-rank text (Sem…, arquivado). */
@Composable
private fun ChoiceRow(title: String, on: Boolean, detail: String = "", quiet: Boolean = false, onClick: () -> Unit) {
    val p = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = if (detail.isEmpty()) 48.dp else 60.dp).clickable(onClick = onClick).semantics {
            role = Role.RadioButton; selected = on
            contentDescription = if (detail.isEmpty()) title else "$title, $detail"
        },
        verticalAlignment = Alignment.CenterVertically
    ) {
        Column(Modifier.weight(1f).padding(vertical = 6.dp)) {
            Text(
                title, fontSize = 16.sp, color = if (on) p.accent else if (quiet) p.secondary else p.text,
                fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1, overflow = TextOverflow.Ellipsis
            )
            if (detail.isNotEmpty()) {
                Text(detail, fontSize = 13.sp, color = p.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        if (on) {
            Spacer(Modifier.width(12.dp))
            Icon(Icons.Rounded.Check, contentDescription = null, tint = p.accent, modifier = Modifier.size(22.dp))
        }
    }
}

/** A search box without an outline, for inside a sheet. */
@Composable
private fun QuietSearch(query: String, onQuery: (String) -> Unit, placeholder: String, modifier: Modifier = Modifier) {
    val p = Ponte.colors
    BasicTextField(
        value = query, onValueChange = onQuery, singleLine = true,
        textStyle = MaterialTheme.typography.bodyLarge.copy(color = p.text),
        cursorBrush = SolidColor(p.accent),
        modifier = modifier.fillMaxWidth().semantics { contentDescription = placeholder },
        decorationBox = { field ->
            Row(
                Modifier.height(44.dp).background(p.chip, RoundedCornerShape(12.dp)).padding(start = 12.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Icon(Icons.Outlined.Search, contentDescription = null, tint = p.secondary, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Box(Modifier.weight(1f)) {
                    if (query.isEmpty()) Text(placeholder, color = p.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    field()
                }
                if (query.isNotEmpty()) {
                    Box(
                        Modifier.size(40.dp).clickable { onQuery("") }.semantics { contentDescription = "Limpar a busca"; role = Role.Button },
                        contentAlignment = Alignment.Center
                    ) { Icon(Icons.Outlined.Close, contentDescription = null, tint = p.secondary, modifier = Modifier.size(18.dp)) }
                }
            }
        }
    )
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
