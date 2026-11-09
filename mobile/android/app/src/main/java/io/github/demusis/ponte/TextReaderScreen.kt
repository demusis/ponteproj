package io.github.demusis.ponte

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** The whole text of a document, for reading. */
@Composable
internal fun TextReaderScreen(title: String, text: String, onBack: () -> Unit) {
    DetailScaffold(title = title.ifBlank { "Texto" }, onBack = onBack) {
        if (text.isBlank()) {
            EmptyState("Este documento não tem texto.", icon = Icons.Outlined.Description)
        } else {
            Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                SelectionContainer {
                    Text(
                        text.trim(),
                        Modifier.padding(start = 20.dp, top = 8.dp, end = 20.dp, bottom = 32.dp),
                        style = MaterialTheme.typography.bodyLarge,
                        fontSize = 17.sp, lineHeight = 27.sp, color = Ponte.colors.text
                    )
                }
            }
        }
    }
}
