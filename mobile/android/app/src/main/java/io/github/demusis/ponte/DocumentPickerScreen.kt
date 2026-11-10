package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Description
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/**
 * Picks the document of a task: the documents the Documentos area already has, searched the same
 * way. [currentId] (the document the task is in now) is left out. [busy]: the choice is being made.
 */
@Composable
internal fun DocumentPickerScreen(
    docs: List<Doc>,
    currentId: Long,
    busy: Boolean,
    onBack: () -> Unit,
    onPick: (Doc) -> Unit
) {
    val c = Ponte.colors
    var query by rememberSaveable { mutableStateOf("") }
    val shown = remember(docs, query, currentId) { docs.filter { it.id != currentId && matchesQuery(it, query) } }
    DetailScaffold(
        title = if (currentId > 0) "Trocar de documento" else "Vincular a um documento",
        onBack = onBack
    ) {
        Column(Modifier.fillMaxSize()) {
            OutlinedTextField(
                value = query, onValueChange = { query = it },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(top = 4.dp, bottom = 8.dp),
                singleLine = true,
                textStyle = MaterialTheme.typography.bodyLarge,
                placeholder = { Text("Buscar por número, título ou origem", color = c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null, tint = c.secondary) },
                trailingIcon = if (query.isEmpty()) null else ({
                    IconButton(onClick = { query = "" }) {
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
            LazyColumn(
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                if (shown.isEmpty()) {
                    item(key = "none") {
                        if (query.isNotBlank()) {
                            EmptyState(
                                "Nada encontrado.", text = "Nenhum documento combina com a busca.",
                                icon = Icons.Outlined.SearchOff,
                                action = {
                                    TextButton(onClick = { query = "" }) {
                                        Text("Limpar a busca", color = c.accent, fontWeight = FontWeight.SemiBold)
                                    }
                                }
                            )
                        } else {
                            EmptyState(
                                "Nenhum documento para escolher.",
                                text = "Toque em sincronizar para trazer o acervo do computador.",
                                icon = Icons.Outlined.Description
                            )
                        }
                    }
                } else {
                    items(shown, key = { it.id }) { d ->
                        DocCard(d.copy(openTasks = 0, attachments = 0), { if (!busy) onPick(d) })
                    }
                }
            }
        }
    }
}
