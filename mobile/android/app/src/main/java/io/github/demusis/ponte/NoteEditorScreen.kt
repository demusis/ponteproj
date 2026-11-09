package io.github.demusis.ponte

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Full-screen editor of the note of a document. */
@Composable
internal fun NoteEditorScreen(title: String, initial: String, onBack: () -> Unit, onSave: (String) -> Unit) {
    val c = Ponte.colors
    var text by rememberSaveable { mutableStateOf(initial) }
    var asking by rememberSaveable { mutableStateOf(false) }
    val changed = text != initial
    val leave = { if (changed) asking = true else onBack() }

    SafeBackHandler(onBack = leave)

    DetailScaffold(
        title = "Nota", onBack = leave, close = true,
        footer = { PrimaryButton("Salvar nota", onClick = { onSave(text) }) }
    ) {
        Column(Modifier.fillMaxSize().padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 16.dp)) {
            if (title.isNotBlank()) {
                Text(
                    title, Modifier.padding(bottom = 8.dp),
                    style = MaterialTheme.typography.bodySmall, color = c.muted,
                    maxLines = 1, overflow = TextOverflow.Ellipsis
                )
            }
            OutlinedTextField(
                value = text, onValueChange = { text = it },
                modifier = Modifier.fillMaxWidth().weight(1f),
                textStyle = MaterialTheme.typography.bodyLarge.copy(fontSize = 17.sp, lineHeight = 25.sp),
                placeholder = { Text("Escreva a nota deste documento", color = c.muted) },
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                shape = RoundedCornerShape(16.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedContainerColor = c.surface, unfocusedContainerColor = c.surface,
                    focusedBorderColor = c.accent, unfocusedBorderColor = c.outline,
                    focusedTextColor = c.text, unfocusedTextColor = c.text, cursorColor = c.accent
                )
            )
        }
    }

    if (asking) {
        AlertDialog(
            onDismissRequest = { asking = false },
            title = { Text("Descartar alterações?") },
            text = { Text("O que você escreveu nesta nota ainda não foi salvo.") },
            confirmButton = { TextButton(onClick = { asking = false; onBack() }) { Text("Descartar", color = c.error) } },
            dismissButton = { TextButton(onClick = { asking = false }) { Text("Continuar editando") } }
        )
    }
}
