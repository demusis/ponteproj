package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * What the app shows while it is locked: nothing of the library, only this. Stateless; the
 * system prompt is started by [Controller].
 */
@Composable
internal fun LockScreen(onUnlock: () -> Unit) {
    val c = Ponte.colors
    Box(Modifier.fillMaxSize().background(c.ground).safeDrawingPadding().padding(horizontal = 24.dp), contentAlignment = Alignment.Center) {
        Column(
            Modifier.widthIn(max = 320.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp)
        ) {
            Box(Modifier.size(72.dp).background(c.accentContainer, CircleShape), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Lock, contentDescription = null, tint = c.onAccentContainer, modifier = Modifier.size(36.dp))
            }
            Text(
                "Ponte de bolso", style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold, color = c.text, textAlign = TextAlign.Center
            )
            Text(
                "Use a digital ou o bloqueio de tela do celular.",
                fontSize = 15.sp, lineHeight = 22.sp, color = c.secondary, textAlign = TextAlign.Center
            )
            PrimaryButton("Desbloquear", onUnlock, Modifier.padding(top = 10.dp), icon = Icons.Outlined.Lock)
        }
    }
}
