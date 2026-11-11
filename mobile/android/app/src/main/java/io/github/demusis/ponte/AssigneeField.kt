package io.github.demusis.ponte

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The Responsável field with its helpers underneath: "Eu" (the person's own name, when it is set and
 * not already in the field) and the names used before, filtered by what is typed. Both only fill the
 * typed field through [onChange]; they never touch the stored task, so the draft rules still hold.
 */
@Composable
internal fun AssigneeField(
    value: String,
    onChange: (String) -> Unit,
    suggestions: List<String>,
    myName: String,
    modifier: Modifier = Modifier
) {
    val shown = Assignees.suggest(suggestions, value, myName)
    val offerMe = Assignees.offerMe(myName, value)
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        LabeledField(
            "Responsável", value, onChange,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words)
        )
        if (offerMe || shown.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically
            ) {
                if (offerMe) NameChip("Eu", accent = true, description = "Eu: preencher com o seu nome") { onChange(myName.trim()) }
                for (name in shown) NameChip(name, accent = false, description = name) { onChange(name) }
            }
        }
    }
}

@Composable
private fun NameChip(text: String, accent: Boolean, description: String, onClick: () -> Unit) {
    val c = Ponte.colors
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(50),
        color = if (accent) c.accentContainer else c.surface,
        border = if (accent) null else BorderStroke(1.dp, c.outline),
        modifier = Modifier.heightIn(min = 40.dp).semantics { contentDescription = description }
    ) {
        Box(Modifier.padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
            Text(
                text, fontSize = 14.sp, fontWeight = FontWeight.Medium,
                color = if (accent) c.onAccentContainer else c.secondary,
                maxLines = 1, overflow = TextOverflow.Ellipsis
            )
        }
    }
}
