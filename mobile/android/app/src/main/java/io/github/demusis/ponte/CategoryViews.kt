package io.github.demusis.ponte

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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** The library's categories, for every chip to find the name and colour of an uid. */
internal val LocalCategories = staticCompositionLocalOf { emptyList<Category>() }

/** One category as a small rounded label: a colour dot and the name (muted when archived). */
@Composable
internal fun CategoryChip(category: Category, modifier: Modifier = Modifier) {
    val p = Ponte.colors
    val tint = CategoryRules.colors(category.color, p.dark)
    val bg = if (category.archived) p.chip else Color(tint.background)
    val fg = if (category.archived) p.muted else Color(tint.text)
    val dot = if (category.archived) p.muted else Color(tint.dot)
    Row(
        modifier.background(bg, RoundedCornerShape(50)).padding(start = 8.dp, end = 10.dp, top = 3.dp, bottom = 3.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(Modifier.size(8.dp).background(dot, CircleShape))
        Spacer(Modifier.width(5.dp))
        Text(category.name, fontSize = 12.sp, lineHeight = 15.sp, fontWeight = FontWeight.Medium, color = fg, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** The compact row of a list: the first chips and "+N" for the rest. Nothing when there are none. */
@Composable
internal fun CategoryChips(uids: List<String>, modifier: Modifier = Modifier, max: Int = 2) {
    if (uids.isEmpty()) return
    val all = LocalCategories.current
    val (shown, more) = CategoryRules.compact(CategoryRules.resolve(uids, all), max)
    if (shown.isEmpty()) return
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        for (c in shown) CategoryChip(c, Modifier.widthIn(max = 120.dp))
        if (more > 0) {
            Text(
                "+$more", fontSize = 12.sp, fontWeight = FontWeight.Medium, color = Ponte.colors.muted,
                modifier = Modifier.semantics { contentDescription = "mais $more categorias" }
            )
        }
    }
}

/** Every chip of an item, wrapping onto more lines (the task screen and the ficha). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun CategoryChipsAll(uids: List<String>, modifier: Modifier = Modifier) {
    val shown = CategoryRules.resolve(uids, LocalCategories.current)
    if (shown.isEmpty()) return
    FlowRow(modifier, horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for (c in shown) CategoryChip(c, Modifier.widthIn(max = 220.dp))
    }
}

/**
 * The "Categorias" field: the chips of the item and a button that opens the picker. The choice is
 * applied to [selected] through [onChange] when the person taps "Pronto". [onCreate] makes a new
 * category (name and colour) and answers it, or the reason it could not be made.
 */
@Composable
internal fun CategoryField(
    selected: List<String>,
    onChange: (List<String>) -> Unit,
    onCreate: (String, Int, (Category?, String) -> Unit) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true
) {
    val p = Ponte.colors
    var open by rememberSaveable { mutableStateOf(false) }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel("Categorias")
        if (selected.isEmpty()) {
            Text("Nenhuma categoria.", style = MaterialTheme.typography.bodyMedium, color = p.muted)
        } else {
            CategoryChipsAll(selected)
        }
        SecondaryButton(
            if (selected.isEmpty()) "Escolher categorias…" else "Alterar categorias…",
            onClick = { open = true }, enabled = enabled
        )
    }
    if (open) {
        CategoryPickerSheet(
            initial = selected, onDismiss = { open = false },
            onDone = { open = false; if (it.toSet() != selected.toSet()) onChange(it) },
            onCreate = onCreate
        )
    }
}

/** The picker as a bottom sheet; the choice is local until "Pronto". */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun CategoryPickerSheet(
    initial: List<String>,
    onDismiss: () -> Unit,
    onDone: (List<String>) -> Unit,
    onCreate: (String, Int, (Category?, String) -> Unit) -> Unit
) {
    val all = LocalCategories.current
    var chosen by rememberSaveable { mutableStateOf(initial) }
    var query by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Ponte.colors.surface
    ) {
        CategoryPickerContent(
            all = all, selected = chosen, query = query, onQuery = { query = it; error = "" },
            onToggle = { chosen = CategoryRules.toggle(chosen, it) },
            onCreate = { name ->
                if (!busy) {
                    busy = true; error = ""
                    onCreate(name, CategoryRules.nextColor(all)) { made, why ->
                        busy = false
                        if (made != null) {
                            chosen = CategoryRules.toggle(chosen, made.uid)
                            query = ""
                        } else error = why
                    }
                }
            },
            busy = busy, error = error, onDone = { onDone(chosen) }
        )
    }
}

/**
 * The body of the picker (also drawn alone in the pictures). Rows are checkable and show the colour
 * dot; "Criar" appears only when no category has the typed name.
 */
@Composable
internal fun CategoryPickerContent(
    all: List<Category>,
    selected: List<String>,
    query: String,
    onQuery: (String) -> Unit,
    onToggle: (String) -> Unit,
    onCreate: (String) -> Unit,
    busy: Boolean,
    error: String,
    onDone: () -> Unit
) {
    val p = Ponte.colors
    val rows = CategoryRules.offered(all, selected, query)
    val full = CategoryRules.isFull(selected)
    Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp).padding(bottom = 16.dp)) {
        Text("Categorias", style = MaterialTheme.typography.titleMedium, color = p.text, modifier = Modifier.padding(bottom = 10.dp))
        SearchBox(query, onQuery, "Buscar ou criar categoria")
        Column(Modifier.heightIn(max = 340.dp).verticalScroll(rememberScrollState()).padding(top = 6.dp)) {
            if (all.isEmpty() && query.isBlank()) {
                Text(
                    "Ainda não há categorias. Escreva um nome acima para criar a primeira.",
                    style = MaterialTheme.typography.bodyMedium, color = p.muted, modifier = Modifier.padding(vertical = 12.dp)
                )
            }
            for (c in rows) {
                val on = c.uid in selected
                PickRow(on, enabled = on || !full, description = (if (on) "Tirar " else "Pôr ") + c.name, onClick = { onToggle(c.uid) }) {
                    CategoryDot(c)
                    Text(
                        if (c.archived) "${c.name} (arquivada)" else c.name, Modifier.weight(1f),
                        fontSize = 16.sp, color = if (c.archived) p.muted else p.text, maxLines = 1, overflow = TextOverflow.Ellipsis
                    )
                }
            }
            if (CategoryRules.canCreate(query, all)) {
                val name = query.trim()
                PickRow(false, enabled = !busy && !full, description = "Criar a categoria $name", onClick = { onCreate(name) }, showCheck = false) {
                    Icon(Icons.Outlined.Add, contentDescription = null, tint = p.accent, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(10.dp))
                    Text("Criar “$name”", Modifier.weight(1f), fontSize = 16.sp, color = p.accent, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        if (full) {
            Text("No máximo ${CategoryRules.MAX_PER_ITEM} categorias por item.", style = MaterialTheme.typography.bodySmall, color = p.warning, modifier = Modifier.padding(top = 8.dp))
        }
        if (error.isNotEmpty()) Notice(error, tone = Tone.ERROR, modifier = Modifier.padding(top = 8.dp))
        Text(
            "Para renomear, mudar a cor ou arquivar categorias, use o Ponte no computador.",
            style = MaterialTheme.typography.bodySmall, color = p.muted, modifier = Modifier.padding(top = 10.dp, bottom = 12.dp)
        )
        PrimaryButton("Pronto", onClick = onDone)
    }
}

/** The filter's sheet body: "Sem categoria" and every category, choose any (an item matches any of them). */
@Composable
internal fun CategoryFilterContent(
    all: List<Category>,
    chosen: Set<String>,
    onToggle: (String) -> Unit,
    onClear: () -> Unit,
    onDone: () -> Unit
) {
    val p = Ponte.colors
    Column(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp).padding(bottom = 16.dp)) {
        Row(Modifier.fillMaxWidth().padding(bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Filtrar por categoria", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium, color = p.text)
            if (chosen.isNotEmpty()) TextButton(onClick = onClear) { Text("Limpar", color = p.accent, fontWeight = FontWeight.SemiBold) }
        }
        Column(Modifier.heightIn(max = 380.dp).verticalScroll(rememberScrollState())) {
            PickRow(CategoryRules.NONE in chosen, true, "Sem categoria", { onToggle(CategoryRules.NONE) }) {
                Box(Modifier.size(10.dp).background(p.outline, CircleShape))
                Spacer(Modifier.width(12.dp))
                Text("Sem categoria", Modifier.weight(1f), fontSize = 16.sp, color = p.text)
            }
            for (c in CategoryRules.filterOptions(all, chosen)) {
                PickRow(c.uid in chosen, true, c.name, { onToggle(c.uid) }) {
                    CategoryDot(c)
                    Text(c.name, Modifier.weight(1f), fontSize = 16.sp, color = p.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        Text(
            "Mostra o que tem qualquer uma das categorias marcadas.",
            style = MaterialTheme.typography.bodySmall, color = p.muted, modifier = Modifier.padding(top = 10.dp, bottom = 12.dp)
        )
        PrimaryButton("Pronto", onClick = onDone)
    }
}

/** The button of a list that opens the filter, and the sheet itself. */
@Composable
internal fun CategoryFilterButton(
    chosen: Set<String>,
    onChange: (Set<String>) -> Unit,
    modifier: Modifier = Modifier
) {
    if (LocalCategories.current.isEmpty() && chosen.isEmpty()) return
    Row(modifier, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        CategoryFilterPill(chosen, onChange)
    }
}

/** The pill of the category filter and its sheet, to sit in a row with other filters. Nothing while there is nothing to filter by. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun CategoryFilterPill(
    chosen: Set<String>,
    onChange: (Set<String>) -> Unit
) {
    val all = LocalCategories.current
    var open by rememberSaveable { mutableStateOf(false) }
    val on = chosen.isNotEmpty()
    // Nothing to filter by while the library has no category (and no filter is on).
    if (all.isEmpty() && !on) return
    FilterPill(CategoryRules.filterLabel(chosen), on, Modifier.widthIn(max = 220.dp)) { open = true }
    if (open) {
        var draft by remember { mutableStateOf(chosen) }
        ModalBottomSheet(
            onDismissRequest = { open = false },
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
            containerColor = Ponte.colors.surface
        ) {
            CategoryFilterContent(
                all, draft, onToggle = { draft = CategoryRules.toggleFilter(draft, it) },
                onClear = { draft = emptySet() },
                onDone = { open = false; onChange(draft) }
            )
        }
    }
}

@Composable
private fun CategoryDot(c: Category) {
    val p = Ponte.colors
    val dot = if (c.archived) p.muted else Color(CategoryRules.colors(c.color, p.dark).dot)
    Box(Modifier.size(10.dp).background(dot, CircleShape))
    Spacer(Modifier.width(12.dp))
}

/** A 48 dp checkable row; [content] is what comes before the check. */
@Composable
internal fun PickRow(
    on: Boolean,
    enabled: Boolean,
    description: String,
    onClick: () -> Unit,
    showCheck: Boolean = true,
    content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit
) {
    val p = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription = description },
        verticalAlignment = Alignment.CenterVertically
    ) {
        content()
        if (showCheck) {
            Box(
                Modifier.size(22.dp).then(
                    if (on) Modifier.background(p.accent, CircleShape)
                    else Modifier.border(2.dp, p.outline, CircleShape)
                ),
                contentAlignment = Alignment.Center
            ) {
                if (on) Icon(Icons.Rounded.Check, contentDescription = null, tint = p.onAccent, modifier = Modifier.size(15.dp))
            }
        }
    }
}

@Composable
private fun SearchBox(value: String, onChange: (String) -> Unit, placeholder: String) {
    val p = Ponte.colors
    OutlinedTextField(
        value = value, onValueChange = onChange, modifier = Modifier.fillMaxWidth(),
        singleLine = true, textStyle = MaterialTheme.typography.bodyLarge,
        placeholder = { Text(placeholder, color = p.muted, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null, tint = p.secondary) },
        trailingIcon = if (value.isEmpty()) null else ({
            IconButton(onClick = { onChange("") }) { Icon(Icons.Outlined.Close, contentDescription = "Limpar a busca", tint = p.secondary) }
        }),
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
        shape = RoundedCornerShape(28.dp),
        colors = OutlinedTextFieldDefaults.colors(
            focusedContainerColor = p.surface, unfocusedContainerColor = p.surface,
            focusedBorderColor = p.accent, unfocusedBorderColor = p.outline,
            focusedTextColor = p.text, unfocusedTextColor = p.text, cursorColor = p.accent
        )
    )
}
