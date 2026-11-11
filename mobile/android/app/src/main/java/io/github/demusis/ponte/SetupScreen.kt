package io.github.demusis.ponte

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** First setup in three steps: the Drive file, the sync password, the first sync. */
@Composable
internal fun SetupScreen(
    state: SetupState,
    onPickFile: () -> Unit,
    onPassphrase: (String) -> Unit,
    onRemember: (Boolean) -> Unit,
    onNext: () -> Unit,
    onBack: () -> Unit,
    onSync: () -> Unit,
    onFinish: () -> Unit,
    onSkip: () -> Unit,
    onName: (String) -> Unit = {}
) {
    val c = Ponte.colors
    Column(Modifier.fillMaxSize().background(c.ground).statusBarsPadding().imePadding()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for (i in 0..2) {
                    Box(
                        Modifier.size(width = if (i == state.step) 24.dp else 8.dp, height = 8.dp)
                            .background(if (i <= state.step) c.accent else c.outline, CircleShape)
                    )
                }
            }
            Spacer(Modifier.weight(1f))
            Text("Passo ${state.step + 1} de 3", style = MaterialTheme.typography.bodySmall, color = c.muted)
        }
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            when (state.step) {
                0 -> StepFile(state, onPickFile)
                1 -> StepPassphrase(state, onPassphrase, onRemember)
                else -> StepSync(state, onName)
            }
            Spacer(Modifier.height(8.dp))
        }
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding().padding(start = 20.dp, end = 20.dp, top = 8.dp, bottom = 12.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp)
        ) {
            when (state.step) {
                0 -> {
                    PrimaryButton("Continuar", onNext)
                    Quiet("Agora não", onSkip)
                }
                1 -> {
                    PrimaryButton("Continuar", onNext)
                    Quiet("Voltar", onBack)
                }
                else -> when {
                    state.syncing -> Spacer(Modifier.height(8.dp))
                    state.done -> PrimaryButton("Começar", onFinish)
                    else -> {
                        PrimaryButton("Tentar de novo", onSync)
                        Quiet("Voltar", onBack)
                        Quiet("Agora não", onSkip)
                    }
                }
            }
        }
    }
}

@Composable
private fun Quiet(text: String, onClick: () -> Unit) {
    TextButton(onClick, Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
        Text(text, fontSize = 15.sp, color = Ponte.colors.secondary)
    }
}

@Composable
private fun Mark(icon: ImageVector) {
    val c = Ponte.colors
    Box(Modifier.size(56.dp).background(c.accentContainer, CircleShape), contentAlignment = Alignment.Center) {
        Icon(icon, contentDescription = null, tint = c.onAccentContainer, modifier = Modifier.size(28.dp))
    }
}

@Composable
private fun Heading(text: String) =
    Text(text, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold, color = Ponte.colors.text)

@Composable
private fun Body(text: String) =
    Text(text, fontSize = 15.sp, lineHeight = 22.sp, color = Ponte.colors.secondary)

@Composable
private fun StepFile(state: SetupState, onPickFile: () -> Unit) {
    val c = Ponte.colors
    Spacer(Modifier.height(8.dp))
    Mark(Icons.Outlined.FolderOpen)
    Heading("Ponte de bolso")
    Body(
        "O celular trabalha com o mesmo acervo do Ponte no computador, por meio do arquivo " +
            "ponte-sync.ponte, na pasta do Google Drive que o computador usa."
    )
    if (state.fileName.isNotEmpty()) {
        PonteCard {
            Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.CheckCircle, contentDescription = "Arquivo escolhido", tint = c.success)
                Spacer(Modifier.width(10.dp))
                Text(
                    state.fileName, Modifier.weight(1f), fontSize = 15.sp, fontWeight = FontWeight.Medium,
                    color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis
                )
            }
        }
    }
    OutlinedButton(
        onPickFile, Modifier.fillMaxWidth().heightIn(min = 52.dp), shape = RoundedCornerShape(26.dp),
        border = BorderStroke(1.dp, c.outline)
    ) {
        Text(
            if (state.fileName.isEmpty()) "Escolher arquivo" else "Trocar arquivo",
            fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = c.accent
        )
    }
}

@Composable
private fun StepPassphrase(state: SetupState, onPassphrase: (String) -> Unit, onRemember: (Boolean) -> Unit) {
    val c = Ponte.colors
    Spacer(Modifier.height(8.dp))
    Mark(Icons.Outlined.Lock)
    Heading("Senha de sincronização")
    Body("Digite a mesma senha que você criou no Ponte do computador. Sem ela, o arquivo de sincronização não abre.")
    LabeledField("Senha de sincronização", state.passphrase, onPassphrase, password = true)
    if (state.canRemember) {
        PonteCard {
            Row(
                Modifier.padding(start = 16.dp, top = 12.dp, end = 12.dp, bottom = 12.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text("Guardar neste aparelho", fontSize = 16.sp, fontWeight = FontWeight.Medium, color = c.text)
                    Text(
                        "Para sincronizar, o celular vai pedir a digital ou o PIN de bloqueio da tela.",
                        fontSize = 13.sp, lineHeight = 18.sp, color = c.muted
                    )
                }
                Spacer(Modifier.width(12.dp))
                Switch(
                    checked = state.remember, onCheckedChange = onRemember,
                    colors = SwitchDefaults.colors(checkedTrackColor = c.accent, checkedThumbColor = c.onAccent)
                )
            }
        }
    } else {
        Text(
            if (state.oldAndroid) {
                "Neste Android a senha não pode ficar guardada com a digital; será preciso digitá-la de novo " +
                    "quando o celular reiniciar ou o aplicativo for fechado."
            } else {
                "Este celular não tem bloqueio de tela, então a senha não pode ficar guardada; será preciso " +
                    "digitá-la de novo quando o celular reiniciar ou o aplicativo for fechado."
            },
            fontSize = 13.sp, lineHeight = 18.sp, color = c.muted
        )
    }
}

@Composable
private fun StepSync(state: SetupState, onName: (String) -> Unit) {
    val c = Ponte.colors
    when {
        state.syncing -> Column(
            Modifier.fillMaxWidth().padding(top = 80.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)
        ) {
            CircularProgressIndicator(Modifier.size(48.dp), color = c.accent, strokeWidth = 4.dp)
            Heading("Trazendo o acervo…")
            Text(
                "Na primeira vez pode levar alguns minutos. Pode deixar o aplicativo aberto.",
                fontSize = 15.sp, lineHeight = 22.sp, color = c.secondary, textAlign = TextAlign.Center
            )
        }
        state.done -> Column(
            Modifier.fillMaxWidth().padding(top = 80.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)
        ) {
            Box(Modifier.size(72.dp).background(c.successContainer, CircleShape), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Check, contentDescription = null, tint = c.success, modifier = Modifier.size(40.dp))
            }
            Heading("Tudo pronto")
            if (state.status.isNotEmpty()) {
                Text(state.status, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted, textAlign = TextAlign.Center)
            }
            // Optional, and saved as it is typed: "Começar" does not depend on it.
            Column(Modifier.fillMaxWidth().padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                LabeledField(
                    "Seu nome nas tarefas (opcional)", state.personName, onName,
                    keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                        capitalization = androidx.compose.ui.text.input.KeyboardCapitalization.Words
                    )
                )
                Text(NAME_HINT, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
            }
        }
        else -> {
            Spacer(Modifier.height(8.dp))
            Mark(Icons.Outlined.Sync)
            Heading("Primeira sincronização")
            if (state.statusError && state.status.isNotEmpty()) Notice(state.status)
            else Body("Falta só trazer o acervo para o celular.")
        }
    }
}
