package io.github.demusis.ponte

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material3.ButtonDefaults
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Ajustes: the Drive file, the passphrase, mobile data, e-mail accounts, starting over. */
@Composable
internal fun SettingsScreen(
    state: SettingsState,
    onBack: () -> Unit,
    onPickFile: () -> Unit,
    onPassphrase: (String) -> Unit,
    onRemember: (Boolean) -> Unit,
    onForget: () -> Unit,
    onMobile: (Boolean) -> Unit,
    onLock: (Boolean) -> Unit,
    onLockDelay: (String) -> Unit,
    onSecureScreen: (Boolean) -> Unit,
    onAddAccount: () -> Unit,
    onEditAccount: (MailAccount) -> Unit,
    onReset: () -> Unit
) {
    val c = Ponte.colors
    DetailScaffold(title = "Ajustes", onBack = onBack) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 4.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            if (state.keyBroken) {
                Notice(
                    "A chave deste aparelho não pôde ser lida. Para voltar a usar o aplicativo, " +
                        "recomece o acervo deste aparelho (“Recomeçar o acervo deste aparelho”, no fim desta tela)."
                )
            }

            SectionLabel("Sincronização")
            PonteCard {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text("Arquivo no Drive", style = FieldLabelStyle, color = c.secondary)
                            if (state.fileName.isEmpty()) {
                                Text("Nenhum arquivo escolhido", fontSize = 15.sp, color = c.muted)
                            } else {
                                Text(
                                    state.fileName, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text,
                                    maxLines = 2, overflow = TextOverflow.Ellipsis
                                )
                                if (state.fileSize >= 0) Text(formatSize(state.fileSize), fontSize = 13.sp, color = c.muted)
                            }
                        }
                        OutlinedButton(
                            onClick = onPickFile, modifier = Modifier.heightIn(min = 48.dp),
                            border = BorderStroke(1.dp, c.outline),
                            colors = ButtonDefaults.outlinedButtonColors(contentColor = c.accent)
                        ) { Text(if (state.fileName.isEmpty()) "Escolher arquivo" else "Trocar arquivo", fontSize = 14.sp) }
                    }

                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        LabeledField(
                            "Frase-senha", state.passphrase, onPassphrase, password = true,
                            placeholder = if (state.remembered) "••••••••" else "Digite a frase-senha"
                        )
                        if (state.remembered) {
                            Text("Guardada neste aparelho. Digite só para trocar.", fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
                        }
                    }

                    if (state.canRemember) {
                        SwitchRow(
                            "Guardar a frase-senha neste aparelho",
                            "Fica protegida e só abre com a sua digital ou o PIN do celular.",
                            state.remembered, onRemember
                        )
                        if (state.remembered) {
                            TextButton(
                                onClick = onForget, modifier = Modifier.heightIn(min = 48.dp),
                                contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 0.dp),
                                colors = ButtonDefaults.textButtonColors(contentColor = c.accent)
                            ) { Text("Esquecer a frase-senha", fontSize = 15.sp) }
                        }
                    }

                    SwitchRow(
                        "Sincronizar também com dados móveis",
                        "Por padrão, a sincronização espera pelo Wi-Fi.",
                        state.mobileData, onMobile
                    )

                    if (state.pending) {
                        Notice("Há mudanças feitas neste aparelho esperando para ir ao Drive.", Tone.WARNING)
                    }
                    if (state.lastStatus.isNotEmpty()) {
                        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            Text("Última sincronização", style = FieldLabelStyle, color = c.secondary)
                            Text(
                                state.lastStatus, fontSize = 14.sp, lineHeight = 20.sp,
                                color = if (state.lastStatusError) c.error else c.text
                            )
                        }
                    }
                }
            }

            SectionLabel("Contas de e-mail", Modifier.padding(top = 8.dp))
            PonteCard {
                Column {
                    for (a in state.accounts) {
                        Row(
                            Modifier.fillMaxWidth().heightIn(min = 64.dp)
                                .clickable(onClickLabel = "Editar a conta") { onEditAccount(a) }
                                .padding(start = 16.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                Text(
                                    a.label, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text,
                                    maxLines = 1, overflow = TextOverflow.Ellipsis
                                )
                                Text(
                                    "${a.user} · ${a.host}", fontSize = 13.sp, color = c.muted,
                                    maxLines = 1, overflow = TextOverflow.Ellipsis
                                )
                            }
                            Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, contentDescription = null, tint = c.muted)
                        }
                        Box(Modifier.padding(start = 16.dp).fillMaxWidth().height(1.dp).background(c.divider))
                    }
                    TextButton(
                        onClick = onAddAccount,
                        modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(horizontal = 4.dp),
                        colors = ButtonDefaults.textButtonColors(contentColor = c.accent)
                    ) {
                        Icon(Icons.Outlined.Add, contentDescription = null, modifier = Modifier.size(20.dp))
                        Text("Adicionar conta", Modifier.padding(start = 8.dp).weight(1f), fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    }
                }
            }

            SectionLabel("Segurança", Modifier.padding(top = 8.dp))
            PonteCard {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    if (!state.lockAvailable) {
                        Notice(
                            "Este celular não tem bloqueio de tela. Defina um PIN, padrão ou digital nas " +
                                "configurações do Android para proteger o Ponte de bolso.",
                            Tone.WARNING
                        )
                    }
                    SwitchRow(
                        "Bloquear o Ponte de bolso",
                        "Pede a digital ou o bloqueio de tela do celular para abrir.",
                        state.lockOn, onLock, enabled = state.lockAvailable
                    )
                    if (state.lockOn) {
                        ChoiceField(
                            "Pedir de novo", LockDelay.values().map { Choice(it.key, it.label) },
                            state.lockDelay, onLockDelay
                        )
                    }
                    SwitchRow(
                        "Impedir capturas de tela",
                        "Também esconde o conteúdo na lista de aplicativos recentes.",
                        state.secureScreen, onSecureScreen
                    )
                }
            }

            SectionLabel("Sobre", Modifier.padding(top = 8.dp))
            PonteCard {
                Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Ponte de bolso", Modifier.weight(1f), fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text)
                    if (state.version.isNotEmpty()) Text("Versão ${state.version}", fontSize = 14.sp, color = c.muted)
                }
            }

            SectionLabel("Recomeçar o acervo deste aparelho", Modifier.padding(top = 8.dp), color = c.error)
            PonteCard {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(
                        "Apaga a cópia do acervo guardada neste aparelho e a frase-senha guardada. " +
                            "Nada é apagado no computador nem no Drive: na próxima sincronização, tudo volta para cá.",
                        style = MaterialTheme.typography.bodySmall, fontSize = 14.sp, lineHeight = 20.sp, color = c.secondary
                    )
                    OutlinedButton(
                        onClick = onReset, modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                        shape = RoundedCornerShape(24.dp),
                        border = BorderStroke(1.dp, c.error),
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = c.error)
                    ) { Text("Recomeçar…", fontSize = 15.sp, fontWeight = FontWeight.Medium) }
                }
            }
            Box(Modifier.height(16.dp))
        }
    }
}

@Composable
private fun SwitchRow(
    title: String, hint: String, checked: Boolean, onChange: (Boolean) -> Unit, enabled: Boolean = true
) {
    val c = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp)
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onChange),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, fontSize = 15.sp, color = if (enabled) c.text else c.muted)
            Text(hint, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
        }
        Switch(
            checked = checked, onCheckedChange = null, enabled = enabled,
            colors = SwitchDefaults.colors(
                checkedThumbColor = c.onAccent, checkedTrackColor = c.accent,
                uncheckedThumbColor = c.secondary, uncheckedTrackColor = c.chip, uncheckedBorderColor = c.outline
            )
        )
    }
}
