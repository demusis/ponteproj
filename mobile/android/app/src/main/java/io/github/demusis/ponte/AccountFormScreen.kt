package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Form of an e-mail account (new when state.account is null). */
@Composable
internal fun AccountFormScreen(
    state: AccountFormState,
    onBack: () -> Unit,
    onSave: (AccountInput) -> Unit,
    onDelete: () -> Unit
) {
    val c = Ponte.colors
    val acc = state.account
    var name by rememberSaveable { mutableStateOf(acc?.name ?: "") }
    var host by rememberSaveable { mutableStateOf(acc?.host ?: "imap.gmail.com") }
    var port by rememberSaveable { mutableStateOf((acc?.port ?: 993).toString()) }
    var user by rememberSaveable { mutableStateOf(acc?.user ?: "") }
    var keep by rememberSaveable { mutableStateOf(acc?.remember ?: true) }
    // The password is never saved with the instance state.
    var password by remember { mutableStateOf("") }
    val enabled = !state.busy

    DetailScaffold(
        title = if (acc == null) "Nova conta" else "Conta de e-mail",
        onBack = onBack, close = true,
        footer = {
            PrimaryButton(
                "Salvar conta", loading = state.busy,
                onClick = {
                    onSave(AccountInput(name.trim(), host.trim(), port.trim().toIntOrNull() ?: 993, user.trim(), password, keep))
                }
            )
            if (acc != null) {
                TextButton(
                    onClick = onDelete, enabled = enabled,
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(top = 4.dp),
                    colors = ButtonDefaults.textButtonColors(contentColor = c.error, disabledContentColor = c.muted)
                ) { Text("Excluir conta", fontSize = 15.sp) }
            }
        }
    ) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            if (state.error.isNotEmpty()) Notice(state.error)
            LabeledField(
                "Nome (opcional)", name, { name = it }, enabled = enabled,
                placeholder = "Como aparece na lista"
            )
            LabeledField(
                "Servidor", host, { host = it }, enabled = enabled,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri)
            )
            LabeledField(
                "Porta", port, { v -> port = v.filter { it.isDigit() }.take(5) }, enabled = enabled,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number)
            )
            Text(
                "Servidor e porta: os dados de entrada de e-mail (IMAP) do seu provedor. No Gmail, imap.gmail.com e 993.",
                style = MaterialTheme.typography.bodySmall, fontSize = 13.sp, lineHeight = 19.sp, color = c.muted
            )
            LabeledField(
                "Usuário", user, { user = it }, enabled = enabled,
                placeholder = "nome@exemplo.com",
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email)
            )
            LabeledField(
                "Senha de app", password, { password = it }, enabled = enabled, password = true,
                placeholder = if (acc?.hasPassword == true) "Deixe vazio para manter a senha guardada" else "",
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password)
            )
            Text(
                "Senha de app: a senha criada no site do seu e-mail para aplicativos, não a senha da conta. " +
                    "No Gmail: Conta Google ▸ Segurança ▸ Verificação em duas etapas ▸ Senhas de app.",
                style = MaterialTheme.typography.bodySmall, fontSize = 13.sp, lineHeight = 19.sp, color = c.muted
            )
            Row(
                Modifier.fillMaxWidth().heightIn(min = 48.dp)
                    .toggleable(value = keep, enabled = enabled, role = Role.Checkbox, onValueChange = { keep = it }),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Checkbox(
                    checked = keep, onCheckedChange = null, enabled = enabled,
                    colors = CheckboxDefaults.colors(checkedColor = c.accent, checkmarkColor = c.onAccent, uncheckedColor = c.secondary)
                )
                Text("Lembrar a senha neste aparelho", Modifier.padding(start = 12.dp), fontSize = 15.sp, color = c.text)
            }
        }
    }
}
