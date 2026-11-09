package io.github.demusis.ponte

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CameraAlt
import androidx.compose.material.icons.outlined.PhotoCamera
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Record by photo: the picture taken and the few fields of the record. */
@Composable
internal fun PhotoScreen(
    state: PhotoState,
    onTitle: (String) -> Unit,
    onKind: (String) -> Unit,
    onDate: (String) -> Unit,
    onOrigin: (String) -> Unit,
    onText: (String) -> Unit,
    onRetake: () -> Unit,
    onSave: () -> Unit,
    onClose: () -> Unit
) {
    val c = Ponte.colors
    val on = !state.saving
    DetailScaffold(
        title = "Registro por foto", onBack = onClose, close = true,
        footer = {
            if (state.error.isNotEmpty()) {
                Notice(state.error)
                Spacer(Modifier.height(10.dp))
            }
            PrimaryButton("Salvar registro", onSave, loading = state.saving)
            if (state.saving && state.progress.isNotEmpty()) {
                Spacer(Modifier.height(8.dp))
                Text(
                    state.progress, Modifier.fillMaxWidth(), color = c.muted,
                    style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Center
                )
            }
        }
    ) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState())
                .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            PhotoArea(state, on, onRetake)
            ChoiceField("Tipo", state.kinds, state.kind, onKind)
            LabeledField("Título", state.title, onTitle, error = state.titleError, enabled = on)
            DateField("Data", state.date, onDate, emptyText = "Sem data")
            LabeledField("Origem", state.origin, onOrigin, placeholder = "Quem enviou ou de onde veio", enabled = on)
            LabeledField(
                "Observações", state.text, onText, placeholder = "O que está no papel, em poucas palavras",
                singleLine = false, minLines = 3, enabled = on
            )
        }
    }
}

@Composable
private fun PhotoArea(state: PhotoState, enabled: Boolean, onRetake: () -> Unit) {
    val c = Ponte.colors
    Box(
        Modifier.fillMaxWidth().height(220.dp).clip(RoundedCornerShape(16.dp)).background(c.photoGround),
        contentAlignment = Alignment.Center
    ) {
        val thumb = state.thumb
        when {
            thumb != null -> {
                Image(
                    thumb, contentDescription = "Foto do documento", contentScale = ContentScale.Fit,
                    modifier = Modifier.fillMaxSize().padding(8.dp)
                )
                Button(
                    onClick = onRetake, enabled = enabled,
                    modifier = Modifier.align(Alignment.BottomEnd).padding(10.dp).heightIn(min = 48.dp),
                    shape = RoundedCornerShape(24.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = c.surface, contentColor = c.accent,
                        disabledContainerColor = c.chip, disabledContentColor = c.muted
                    )
                ) {
                    Icon(Icons.Outlined.PhotoCamera, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.size(8.dp))
                    Text("Tirar outra", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            state.hasPhoto -> CircularProgressIndicator(Modifier.size(28.dp), color = c.muted, strokeWidth = 3.dp)
            else -> Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Icon(Icons.Outlined.CameraAlt, contentDescription = null, tint = c.secondary, modifier = Modifier.size(36.dp))
                Button(
                    onClick = onRetake, enabled = enabled,
                    modifier = Modifier.heightIn(min = 48.dp),
                    shape = RoundedCornerShape(24.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = c.accent, contentColor = c.onAccent)
                ) { Text("Tirar foto", fontSize = 14.sp, fontWeight = FontWeight.SemiBold) }
            }
        }
    }
}
