package io.github.demusis.ponte

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Mail
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** The E-mail area (inside MainShell): account, folder, filter, messages and the import bar. */
@Composable
internal fun MailScreen(
    state: MailState,
    onAccount: (String) -> Unit,
    onFolder: (String) -> Unit,
    onFilter: (String) -> Unit,
    onToggle: (Long) -> Unit,
    onImport: () -> Unit,
    onReload: () -> Unit,
    onAddAccount: () -> Unit,
    onPassword: () -> Unit
) {
    val c = Ponte.colors
    if (state.loading) {
        LoadingBox(modifier = Modifier.fillMaxSize())
        return
    }
    val account = state.account
    if (account == null) {
        EmptyState(
            title = "Nenhuma conta de e-mail",
            text = "Adicione uma conta para trazer mensagens ao acervo.",
            icon = Icons.Outlined.Mail,
            action = { PrimaryButton("Adicionar conta", onAddAccount) }
        )
        return
    }

    val showList = !state.needsPassword && state.listed && state.messages.isNotEmpty()
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            Column(Modifier.padding(start = 16.dp, top = 4.dp, end = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Selector(
                        description = "Escolher a conta", value = account.label,
                        options = state.accounts.map { it.uid to it.label }, selected = account.uid,
                        onSelect = onAccount, enabled = !state.busy, modifier = Modifier.weight(1f)
                    )
                    Selector(
                        description = "Escolher a pasta", value = state.folderLabel(),
                        options = state.folders.map { it.raw to it.name }, selected = state.folder,
                        onSelect = onFolder, enabled = !state.busy, modifier = Modifier.weight(1.25f)
                    )
                    IconButton(onClick = onReload, enabled = !state.busy) {
                        Icon(Icons.Outlined.Refresh, contentDescription = "Atualizar a lista de mensagens", tint = if (state.busy) c.muted else c.accent)
                    }
                }
                FilterChips(
                    options = listOf("todas" to "Todas", "estrela" to "Com estrela", "nao_lidas" to "Não lidas"),
                    selected = state.filter, onSelect = { if (!state.busy) onFilter(it) },
                    modifier = Modifier.padding(end = 8.dp).padding(vertical = 6.dp)
                )
            }
            if (state.busy) {
                LinearProgressIndicator(
                    Modifier.padding(horizontal = 16.dp).fillMaxWidth().height(3.dp),
                    color = c.accent, trackColor = c.chip
                )
                if (state.info.isNotEmpty()) InfoLine(state.info, c.secondary)
            } else if (state.info.isNotEmpty()) {
                if (state.infoError) Notice(state.info, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
                else InfoLine(state.info, c.secondary)
            }

            when {
                state.needsPassword -> EmptyState(
                    title = "Falta a senha de app.",
                    text = "A senha de app desta conta não está guardada neste aparelho. Digite-a para ver as mensagens.",
                    action = { PrimaryButton("Digitar a senha de app", onPassword, enabled = !state.busy) }
                )
                state.listed && state.messages.isEmpty() -> EmptyState(
                    title = "Nenhuma mensagem",
                    text = "Não há mensagens em “${state.folderLabel()}”" + when (state.filter) {
                        "estrela" -> " com estrela."
                        "nao_lidas" -> " não lidas."
                        else -> "."
                    },
                    icon = Icons.Outlined.Mail
                )
                !state.listed && !state.busy && !state.infoError -> EmptyState(
                    title = "Mensagens ainda não listadas",
                    text = "Fora do Wi-Fi, a lista só é buscada quando você toca em Listar mensagens.",
                    action = { PrimaryButton("Listar mensagens", onReload) }
                )
                showList -> LazyColumn(
                    Modifier.fillMaxWidth().weight(1f),
                    contentPadding = PaddingValues(
                        start = 16.dp, top = 8.dp, end = 16.dp,
                        bottom = if (state.checked.isNotEmpty()) 88.dp else 16.dp
                    )
                ) {
                    itemsIndexed(state.messages, key = { _, m -> m.uid }) { i, m ->
                        val first = i == 0
                        val last = i == state.messages.lastIndex
                        val shape = RoundedCornerShape(
                            topStart = if (first) 16.dp else 0.dp, topEnd = if (first) 16.dp else 0.dp,
                            bottomStart = if (last) 16.dp else 0.dp, bottomEnd = if (last) 16.dp else 0.dp
                        )
                        Column(Modifier.background(c.surface, shape)) {
                            MessageRow(m, m.uid in state.checked, !state.busy) { onToggle(m.uid) }
                            if (!last) Box(Modifier.padding(start = 52.dp).fillMaxWidth().height(1.dp).background(c.divider))
                        }
                    }
                }
            }
        }

        if (state.checked.isNotEmpty() && showList) {
            val n = state.checked.size
            Row(
                Modifier.align(Alignment.BottomCenter).padding(start = 16.dp, end = 16.dp, bottom = 12.dp).fillMaxWidth()
                    .background(c.contrastBar, RoundedCornerShape(30.dp))
                    .padding(start = 18.dp, top = 8.dp, end = 8.dp, bottom = 8.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                Text(
                    if (n == 1) "1 marcada" else "$n marcadas", Modifier.weight(1f),
                    fontSize = 14.sp, color = c.onContrastBar
                )
                Button(
                    onClick = onImport, enabled = !state.busy,
                    modifier = Modifier.height(44.dp), shape = RoundedCornerShape(22.dp),
                    contentPadding = PaddingValues(horizontal = 20.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = c.onContrastBar, contentColor = c.contrastBar,
                        disabledContainerColor = c.onContrastBar.copy(alpha = 0.35f), disabledContentColor = c.contrastBar
                    )
                ) { Text(if (state.busy) "Importando…" else "Importar", fontSize = 14.sp, fontWeight = FontWeight.Bold) }
            }
        }
    }
}

@Composable
private fun InfoLine(text: String, color: Color) {
    Text(
        text, Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 6.dp),
        style = MaterialTheme.typography.bodySmall, fontSize = 13.sp, color = color
    )
}

/** Compact field-like button that opens a menu (account, folder). */
@Composable
private fun Selector(
    description: String,
    value: String,
    options: List<Pair<String, String>>,
    selected: String,
    onSelect: (String) -> Unit,
    enabled: Boolean,
    modifier: Modifier = Modifier
) {
    val c = Ponte.colors
    var open by remember { mutableStateOf(false) }
    Box(modifier) {
        Surface(
            onClick = { open = true }, enabled = enabled,
            shape = RoundedCornerShape(12.dp), color = c.surface, border = BorderStroke(1.dp, c.outline),
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp)
        ) {
            Row(
                Modifier.padding(start = 14.dp, end = 10.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                Text(
                    value, Modifier.weight(1f), fontSize = 14.sp, fontWeight = FontWeight.Medium,
                    color = if (enabled) c.text else c.muted, maxLines = 1, overflow = TextOverflow.Ellipsis
                )
                Icon(Icons.Outlined.ExpandMore, contentDescription = description, tint = c.secondary, modifier = Modifier.size(18.dp))
            }
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for ((key, label) in options) {
                DropdownMenuItem(
                    text = { Text(label, fontSize = 16.sp, fontWeight = if (key == selected) FontWeight.SemiBold else FontWeight.Normal) },
                    onClick = { open = false; onSelect(key) }
                )
            }
        }
    }
}

@Composable
private fun MessageRow(m: MailMessage, checked: Boolean, enabled: Boolean, onClick: () -> Unit) {
    val c = Ponte.colors
    val sender = m.from.substringBefore('<').trim().trim('"').ifEmpty { m.from.trim('<', '>', ' ') }
    val subject = m.subject.ifBlank { "(sem assunto)" }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp)
            .then(if (m.imported || !enabled) Modifier else Modifier.clickable(onClick = onClick))
            .padding(end = 14.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(Modifier.size(52.dp), contentAlignment = Alignment.Center) {
            if (!m.imported) {
                Checkbox(
                    checked = checked, onCheckedChange = null, enabled = enabled,
                    colors = CheckboxDefaults.colors(
                        checkedColor = c.accent, checkmarkColor = c.onAccent, uncheckedColor = c.secondary,
                        disabledCheckedColor = c.muted, disabledUncheckedColor = c.muted
                    )
                )
            }
        }
        Column(Modifier.weight(1f).alpha(if (m.imported) 0.6f else 1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    sender, Modifier.weight(1f), fontSize = 15.sp,
                    fontWeight = if (m.imported) FontWeight.Medium else FontWeight.SemiBold,
                    color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis
                )
                if (m.flagged) {
                    Icon(Icons.Filled.Star, contentDescription = "Com estrela", tint = c.warning, modifier = Modifier.size(16.dp))
                }
                Text(m.date, fontSize = 12.sp, color = c.muted, maxLines = 1)
            }
            Text(
                subject, fontSize = 14.sp, lineHeight = 19.sp, color = c.secondary,
                maxLines = 2, overflow = TextOverflow.Ellipsis
            )
            if (m.error != null) {
                Text(m.error, fontSize = 12.sp, lineHeight = 16.sp, color = c.error)
            }
            if (m.imported) {
                Pill("Já no acervo", Tone.SUCCESS, Modifier.padding(top = 2.dp))
            }
        }
    }
}
