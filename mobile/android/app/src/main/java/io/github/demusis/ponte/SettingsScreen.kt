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
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.IconButton
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
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

/** Under "Seu nome nas tarefas", in Ajustes and in the first setup. */
internal const val NAME_HINT =
    "Usado pelo botão Eu e como responsável das tarefas que você cria aqui. Fica só neste aparelho."

/** Said when the phone does not let the app show notifications. */
internal const val NOTICE_BLOCKED =
    "O Ponte de bolso não pode mostrar notificações. Para receber o aviso, permita as notificações " +
        "do aplicativo nas configurações do telefone."

/** Ajustes: the Drive file, the sync password, mobile data, e-mail accounts, starting over. */
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
    onReset: () -> Unit,
    onName: (String) -> Unit = {},
    onNotice: (Boolean) -> Unit = {},
    onNoticeTime: (String) -> Unit = {},
    onNoticeSettings: () -> Unit = {},
    onHolidayState: (String) -> Unit = {},
    onAddHolidayDay: (String, String, Boolean) -> Unit = { _, _, _ -> },
    onRemoveHolidayDay: (CustomDay) -> Unit = {},
    onCalendar: (Boolean) -> Unit = {},
    onCalendarChoice: (String) -> Unit = {},
    onCalendarRefresh: () -> Unit = {}
) {
    val c = Ponte.colors
    DetailScaffold(title = "Ajustes", onBack = onBack) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(top = 4.dp, bottom = 16.dp)) {
            if (state.keyBroken) {
                SectionBody {
                    Notice(
                        "O acervo deste aparelho não pôde ser aberto. Para voltar a usar o aplicativo, " +
                            "use a opção “Recomeçar o acervo deste aparelho”, no fim desta tela."
                    )
                }
            }

            SectionHeader("Sincronização")
            FlatBlock {
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
                        TextAction(if (state.fileName.isEmpty()) "Escolher arquivo" else "Trocar arquivo", onPickFile)
                    }

                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        LabeledField(
                            "Senha de sincronização", state.passphrase, onPassphrase, password = true,
                            placeholder = if (state.remembered) "••••••••" else "Digite a senha"
                        )
                        if (state.remembered) {
                            Text("Guardada neste aparelho. Digite só para trocar.", fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
                        }
                    }

                    if (state.canRemember) {
                        SwitchRow(
                            "Guardar a senha neste aparelho",
                            "Fica protegida e só abre com a sua digital ou o PIN do celular.",
                            state.remembered, onRemember
                        )
                        if (state.remembered) {
                            TextButton(
                                onClick = onForget, modifier = Modifier.heightIn(min = 48.dp),
                                contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 0.dp),
                                colors = ButtonDefaults.textButtonColors(contentColor = c.accent)
                            ) { Text("Esquecer a senha guardada", fontSize = 15.sp) }
                        }
                    }

                    SwitchRow(
                        "Sincronizar também com dados móveis",
                        "Por padrão, a sincronização espera pelo Wi-Fi.",
                        state.mobileData, onMobile
                    )

                    if (state.pending) {
                        Notice("Há alterações deste aparelho aguardando envio ao Drive. Elas vão na próxima sincronização.", tone = Tone.WARNING)
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

            SectionHeader("Seu nome")
            FlatBlock {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    LabeledField(
                        "Seu nome nas tarefas", state.personName, onName,
                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words)
                    )
                    Text(NAME_HINT, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
                }
            }

            SectionHeader("Aviso de prazos")
            FlatBlock {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    SwitchRow(
                        "Avisar dos prazos todos os dias",
                        "Mostra só quantas tarefas e atendimentos vencem, sem os títulos. Não avisa quando nada vence.",
                        state.noticeOn && !state.noticeBlocked, onNotice
                    )
                    if (state.noticeOn && !state.noticeBlocked) {
                        ChoiceField(
                            "Horário do aviso", DeadlineNotice.CHOICES.map { Choice(it.key, it.key) },
                            state.noticeTime, onNoticeTime
                        )
                    }
                    if (state.noticeBlocked) {
                        Notice(NOTICE_BLOCKED, tone = Tone.WARNING)
                        TextButton(
                            onClick = onNoticeSettings, modifier = Modifier.heightIn(min = 48.dp),
                            contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 0.dp),
                            colors = ButtonDefaults.textButtonColors(contentColor = c.accent)
                        ) { Text("Abrir as configurações do telefone", fontSize = 15.sp) }
                    }
                }
            }

            SectionHeader("Agenda do celular")
            CalendarCard(state, onCalendar, onCalendarChoice, onCalendarRefresh)

            SectionHeader("Feriados")
            HolidaysCard(state, onHolidayState, onAddHolidayDay, onRemoveHolidayDay)

            SectionHeader("Contas de e-mail")
            FlatBlock {
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

            SectionHeader("Segurança")
            FlatBlock {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    if (!state.lockAvailable) {
                        Notice(
                            "Este celular não tem bloqueio de tela. Defina um PIN, padrão ou digital nas " +
                                "configurações do Android para proteger o Ponte de bolso.", tone = Tone.WARNING)
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

            SectionHeader("Sobre")
            FlatBlock {
                Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Ponte de bolso", Modifier.weight(1f), fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text)
                    if (state.version.isNotEmpty()) Text("Versão ${state.version}", fontSize = 14.sp, color = c.muted)
                }
            }

            SectionHeader("Recomeçar o acervo deste aparelho", color = c.error)
            FlatBlock {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(
                        "Apaga a cópia do acervo guardada neste aparelho e a senha de sincronização guardada. " +
                            "Nada é apagado no computador nem no Drive: na próxima sincronização, tudo volta para cá.",
                        style = MaterialTheme.typography.bodySmall, fontSize = 14.sp, lineHeight = 20.sp, color = c.secondary
                    )
                    TextAction("Recomeçar…", onReset, Modifier.offset(x = (-12).dp), danger = true)
                }
            }
        }
    }
}

/** Said under the calendar switch. */
internal const val CALENDAR_HINT =
    "Use uma agenda só para o Ponte (por exemplo, “Ponte”). Mudanças de data feitas na agenda voltam para a tarefa. " +
        "Use em um só celular por agenda."

/** Said when the phone has no calendar the Ponte can write to. */
internal const val CALENDAR_NONE = "Nenhuma agenda que aceite novos eventos foi encontrada neste celular."

/** Said under the list: where a calendar that does not show up is usually stuck. */
internal const val CALENDAR_FIND =
    "Não achou a agenda? Ela precisa ser da mesma conta Google deste celular. Crie-a no Google Agenda, toque em " +
        "Atualizar a lista e espere alguns segundos. No app Google Agenda, em Configurações ▸ a agenda, deixe Sincronizar ligado."

/** The switch, the calendar to use and how the last conference went. */
@Composable
private fun CalendarCard(
    state: SettingsState, onSwitch: (Boolean) -> Unit, onChoice: (String) -> Unit, onRefresh: () -> Unit
) {
    val c = Ponte.colors
    FlatBlock {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SwitchRow(
                "Sincronizar tarefas com a agenda",
                "Mostra na agenda o prazo e o período planejado das tarefas abertas. Os atendimentos não vão.",
                state.calendarOn, onSwitch
            )
            if (state.calendarOn) {
                Text(CALENDAR_HINT, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
                val groups = CalendarChoices.grouped(state.calendars)
                if (state.calendarChosen.isEmpty() && groups.isNotEmpty()) {
                    Text("Escolha uma agenda", style = FieldLabelStyle, color = c.secondary)
                }
                if (groups.isEmpty()) Notice(CALENDAR_NONE, tone = Tone.WARNING)
                for ((header, items) in groups) {
                    // An account is an e-mail address: shown as it is (upper case would break it into pieces).
                    Text(header, fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Medium, color = c.secondary)
                    Column {
                        for (cal in items) CalendarRow(cal, cal.id.toString() == state.calendarChosen) { onChoice(cal.id.toString()) }
                    }
                }
                TextButton(
                    onClick = onRefresh, enabled = !state.calendarRefreshing,
                    modifier = Modifier.heightIn(min = 48.dp),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 0.dp),
                    colors = ButtonDefaults.textButtonColors(contentColor = c.accent)
                ) { Text(if (state.calendarRefreshing) "Atualizando…" else "Atualizar a lista", fontSize = 15.sp) }
                Text(CALENDAR_FIND, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
                if (state.calendarStatus.isNotEmpty()) {
                    Text(state.calendarStatus, fontSize = 14.sp, lineHeight = 20.sp, color = c.text)
                }
            }
            if (state.calendarWarning.isNotEmpty()) Notice(state.calendarWarning, tone = Tone.WARNING)
        }
    }
}

/** One calendar: its colour dot, its name, a muted line when it needs one, and the radio mark. */
@Composable
private fun CalendarRow(cal: CalInfo, selected: Boolean, onClick: () -> Unit) {
    val c = Ponte.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        RadioButton(
            selected = selected, onClick = null,
            colors = RadioButtonDefaults.colors(selectedColor = c.accent, unselectedColor = c.secondary)
        )
        Box(
            Modifier.size(12.dp).clip(CircleShape)
                .background(if (cal.color == 0) c.muted else androidx.compose.ui.graphics.Color(cal.color or 0xFF000000.toInt()))
        )
        Column(Modifier.weight(1f).padding(vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(cal.name, fontSize = 15.sp, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (cal.note.isNotEmpty()) Text(cal.note, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
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

/** Said under "Feriados" in Ajustes. */
internal const val HOLIDAYS_NOTE =
    "Os feriados nacionais já contam. Estaduais e os dias que você cadastrar valem para todos os aparelhos que sincronizam este acervo."
internal const val HOLIDAYS_NO_STATE_DAYS = "Este estado não tem feriado estadual além dos nacionais."
internal const val HOLIDAYS_OTHER_HINT =
    "Recesso, ponto facultativo, feriado da cidade."

/** How a registered day is shown: "19/03/2026" or, every year, "19/03 · todo ano". */
internal fun holidayDayText(day: CustomDay): String =
    if (day.year == null) "%02d/%02d · todo ano".format(day.day, day.month)
    else "%02d/%02d/%d".format(day.day, day.month, day.year)

/** The state, its holidays (read only) and the extra days off. */
@Composable
private fun HolidaysCard(
    state: SettingsState,
    onState: (String) -> Unit,
    onAdd: (String, String, Boolean) -> Unit,
    onRemove: (CustomDay) -> Unit
) {
    val c = Ponte.colors
    var date by rememberSaveable { mutableStateOf("") }
    var name by rememberSaveable { mutableStateOf("") }
    var yearly by rememberSaveable { mutableStateOf(false) }
    FlatBlock {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(HOLIDAYS_NOTE, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
            ChoiceField(
                "Estado", listOf(Choice("", "Nenhum")) + Holidays.STATES.map { Choice(it.first, it.second) },
                state.holidayUf, onState
            )
            if (state.holidayUf.isNotEmpty()) {
                val lines = Holidays.stateHolidayLines(state.holidayUf)
                if (lines.isEmpty()) Text(HOLIDAYS_NO_STATE_DAYS, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
                for ((whenText, holidayName) in lines) {
                    Text("$whenText · $holidayName", fontSize = 14.sp, lineHeight = 20.sp, color = c.secondary)
                }
            }

            Text("Outros dias sem expediente", style = FieldLabelStyle, color = c.secondary)
            Text(HOLIDAYS_OTHER_HINT, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted)
            for (day in state.holidayDays) {
                Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(holidayDayText(day), fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.text)
                        Text(day.label, fontSize = 13.sp, lineHeight = 18.sp, color = c.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }
                    IconButton(onClick = { onRemove(day) }, modifier = Modifier.size(48.dp)) {
                        Icon(Icons.Outlined.Delete, contentDescription = "Remover este dia", tint = c.secondary)
                    }
                }
            }
            DateField("Dia", date, { date = it }, emptyText = "Escolher a data")
            LabeledField(
                "Nome", name, { name = it }, placeholder = "Nome (opcional)",
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
            )
            SwitchRow("Todo ano", "Só o dia e o mês contam.", yearly, { yearly = it })
            TextButton(
                onClick = { onAdd(date, name, yearly); date = ""; name = ""; yearly = false },
                enabled = date.isNotEmpty(),
                modifier = Modifier.fillMaxWidth().heightIn(min = 52.dp),
                colors = ButtonDefaults.textButtonColors(contentColor = c.accent)
            ) {
                Icon(Icons.Outlined.Add, contentDescription = null, modifier = Modifier.size(20.dp))
                Text("Adicionar um dia", Modifier.padding(start = 8.dp).weight(1f), fontSize = 15.sp, fontWeight = FontWeight.Medium)
            }
        }
    }
}
