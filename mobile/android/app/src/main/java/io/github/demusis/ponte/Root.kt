package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * Everything on screen: the three areas inside their frame, or the detail on top of them,
 * plus the message at the bottom and the questions. Reads [Controller] and tells it what the
 * person did; the screens themselves know nothing about it.
 */
@Composable
internal fun AppRoot() {
    val c = Controller
    val holder = rememberSaveableStateHolder()
    val top = c.stack.lastOrNull()

    // What a screen remembers (scroll, typed text) is kept while it is under another one and
    // dropped when it leaves the stack.
    val keys = c.stack.map { it.key }
    var known by remember { mutableStateOf(emptyList<String>()) }
    LaunchedEffect(keys) {
        known.filter { it !in keys }.forEach { holder.removeState(it) }
        known = keys
    }

    // What waited behind the lock (a refusal of a share) is said once the app is open.
    LaunchedEffect(c.locked) {
        if (!c.locked) {
            Intake.flushNotice()
            // An attach whose camera or file came back while the app was locked.
            Intake.onUnlocked()
        }
    }

    // On the lock screen Back does not unlock.
    SafeBackHandler(enabled = c.canGoBack() && !c.locked) { c.back() }
    // Locked: Back sends the app to the background (not finish), so nothing in progress is lost.
    SafeBackHandler(enabled = c.locked) { c.moveToBack() }

    // While locked nothing of the library is composed: only the lock screen. The state holder
    // above stays, so what each screen remembers (drafts, scroll) is kept and comes back.
    if (c.locked) {
        LockScreen(onUnlock = c::requestUnlock)
        return
    }

    Box(Modifier.fillMaxSize().background(Ponte.colors.ground)) {
        if (top == null) {
            MainShell(
                state = c.shell,
                onTab = c::selectTab,
                onSync = { c.requestSync(null) },
                onSettings = c::openSettings,
                onStatus = c::showStatus,
                onSearch = c::openSearch
            ) {
                holder.SaveableStateProvider("tab:" + c.tab) {
                    when (c.tab) {
                        0 -> TodayScreen(
                            c.today, onToggle = c::toggleTask, onOpen = { c.openTask(it.id) },
                            onOpenAttendance = { c.openAttendance(it.id) }, onOpenResources = c::openResources
                        )
                        // Tarefas | Atendimentos | Recursos: they have no tab of their own (four is the limit).
                        1 -> Column(Modifier.fillMaxSize()) {
                            SegmentedTabs(listOf("Tarefas", "Atendimentos", PhoneWords.RES_AREA), c.tasksSegment, c::selectSegment)
                            Box(Modifier.weight(1f).fillMaxWidth()) {
                                if (c.tasksSegment == 0) {
                                    TasksScreen(
                                        c.tasks, onToggle = c::toggleTask, onOpen = { c.openTask(it.id) },
                                        onNew = c::openNewTaskWithoutDocument
                                    )
                                } else if (c.tasksSegment == 2) {
                                    ResourcesScreen(
                                        c.resources, now = ResourceRules.nowMoment(),
                                        query = c.resQuery, onQuery = { c.resQuery = it },
                                        view = c.resView, onView = { c.resView = it },
                                        onOpenUse = { c.openResourceUse(it.uid) }, onOpenResource = { c.openResource(it.uid) }
                                    )
                                } else {
                                    AttendancesScreen(
                                        c.attendances, query = c.attQuery, onQuery = { c.attQuery = it },
                                        onOpen = { c.openAttendance(it.id) }
                                    )
                                }
                            }
                        }
                        2 -> DocumentsScreen(
                            docs = c.docs,
                            query = c.docQuery, onQuery = { c.docQuery = it },
                            filter = c.docFilter, onFilter = { c.docFilter = it },
                            onOpen = c::openDocument, onPhoto = c::openPhoto
                        )
                        else -> MailScreen(
                            state = c.mail,
                            onAccount = c::selectMailAccount, onFolder = c::selectMailFolder,
                            onFilter = c::selectMailFilter, onToggle = c::toggleMail,
                            onImport = c::importMail, onReload = c::reloadMail,
                            onAddAccount = { c.openAccount(null) }, onPassword = c::askMailPassword
                        )
                    }
                }
            }
        } else {
            holder.SaveableStateProvider(top.key) { RouteScreen(top) }
        }

        // Above the bottom bar (or the footer of a detail); on Documentos, above the photo button too.
        val lift = if (top == null && ((c.tab == 1 && c.tasksSegment == 0) || c.tab == 2)) 152.dp else 84.dp
        SnackbarHost(
            c.snackbar,
            Modifier.align(Alignment.BottomCenter).navigationBarsPadding().imePadding().padding(bottom = lift)
        )
    }

    Dialogs()
}

@Composable
private fun RouteScreen(route: Route) {
    val c = Controller
    val doc = c.document.doc
    val docName = doc?.let { it.number.ifEmpty { it.title } } ?: ""
    when (route) {
        is Route.Document -> DocumentScreen(
            state = c.document, syncing = c.shell.syncing,
            onBack = c::back, onSync = { c.requestSync(null) },
            onToggleTask = c::toggleTask, onNewTask = c::openNewTask, onEditNote = c::openNote,
            onOpenFile = c::openFile, onReadText = c::openText, onOpenTask = { c.openTask(it.id) },
            attachBusy = Intake.attachBusy,
            onAttachPhoto = { d -> doc?.let { Intake.attachPhoto(it.id, d) } },
            onAttachFile = { d -> doc?.let { Intake.attachPick(it.id, d) } }
        )
        is Route.EditTask -> TaskScreen(
            routeId = route.id, state = c.taskEdit, actions = c.actions, priorities = c.priorities,
            onBack = c::back, onSave = c::saveTask, onFinish = c::finishTask,
            onOpenDocument = { c.openTaskDocument() }, onLink = c::openPicker, onUnlink = c::unlinkTask,
            onReload = c::reloadTask, assignees = c.assignees, myName = c.personName
        )
        is Route.Attendance -> AttendanceScreen(
            routeId = route.id, state = c.attendance, onBack = c::back,
            onOpenDocument = c::openDocument, onOpenTask = c::openTask, onOpenFile = c::openFile
        )
        is Route.ResourceUse -> ResourceUseScreen(
            uid = route.uid, state = c.resources, now = ResourceRules.nowMoment(), onBack = c::back,
            onOpenResource = c::openResource, onOpenDocument = c::openDocument,
            onOpenAttendance = c::openAttendance, onOpenTask = c::openTask
        )
        is Route.Resource -> ResourceScreen(
            uid = route.uid, state = c.resources, now = ResourceRules.nowMoment(), onBack = c::back,
            onOpenUse = c::openResourceUse
        )
        Route.Search -> SearchScreen(
            state = c.search, onQuery = c::setSearchQuery, onBack = c::back,
            onOpenDocument = c::openDocument, onOpenTask = c::openTask, onOpenAttendance = c::openAttendance
        )
        is Route.Pick -> DocumentPickerScreen(
            docs = c.docs, currentId = c.taskEdit.task?.documentId ?: 0L, busy = c.taskEdit.busy,
            onBack = c::back, onPick = c::pickDocument
        )
        is Route.Text ->
            if (doc == null) Waiting("Texto do documento", c.document, c::back)
            else TextReaderScreen(title = docName, text = doc.text, onBack = c::back)
        is Route.Note ->
            if (doc == null) Waiting("Nota", c.document, c::back)
            else NoteEditorScreen(title = docName, initial = doc.note, onBack = c::back, onSave = c::saveNote)
        is Route.NewTask -> NewTaskScreen(
            document = if (route.id > 0) docName else "",
            // A task without document has nothing to triage.
            actions = if (route.id > 0) c.actions else c.actions.filter { it.key != "triar" },
            onBack = c::back, onCreate = c::createTask, withoutDocument = route.id <= 0,
            assignees = c.assignees, myName = c.personName
        )
        Route.Share -> {
            val sh = Intake.share
            // Reaching here means the app is unlocked and the screen is in front.
            LaunchedEffect(Unit) { Intake.markSeen() }
            ShareScreen(
                state = sh, onBack = Intake::back, onCancel = Intake::cancel,
                onRecord = Intake::chooseRecord, onAttach = Intake::chooseAttach,
                onTitle = Intake::setTitle, onKind = Intake::setKind, onDate = Intake::setDate,
                onOrigin = Intake::setOrigin, onNotes = Intake::setNotes,
                onSaveRecord = Intake::saveRecord,
                onChangeTarget = Intake::chooseAttach, onDescription = Intake::setDescription,
                onSaveAttach = Intake::saveAttach
            )
        }
        Route.SharePick -> DocumentPickerScreen(
            docs = c.docs, currentId = 0L, busy = false,
            onBack = c::back, onPick = Intake::pickTarget, title = "Anexar a um documento"
        )
        Route.Photo -> PhotoScreen(
            state = c.photo,
            onTitle = c::setPhotoTitle, onKind = c::setPhotoKind, onDate = c::setPhotoDate,
            onOrigin = c::setPhotoOrigin, onText = c::setPhotoText,
            onRetake = c::takePhoto, onSave = c::savePhoto, onClose = c::back
        )
        Route.Settings -> SettingsScreen(
            state = c.settings, onBack = c::back, onPickFile = c::pickFile,
            onPassphrase = c::setPassphrase, onRemember = c::setRemember, onForget = c::forgetPassphrase,
            onMobile = c::setMobileData,
            onLock = c::setLockEnabled, onLockDelay = c::setLockDelay, onSecureScreen = c::setSecureScreen,
            onAddAccount = { c.openAccount(null) }, onEditAccount = { c.openAccount(it) },
            onReset = c::askReset,
            onName = c::changePersonName, onNotice = c::setNoticeEnabled, onNoticeTime = c::setNoticeTime,
            onNoticeSettings = c::openNoticeSettings
        )
        is Route.Account -> AccountFormScreen(
            state = c.accountForm, onBack = c::back, onSave = c::saveAccount, onDelete = c::askDeleteAccount
        )
        Route.Setup -> SetupScreen(
            state = c.setup, onPickFile = c::pickFile,
            onPassphrase = c::setSetupPassphrase, onRemember = c::setSetupRemember,
            onNext = c::setupNext, onBack = c::back, onSync = c::setupSync,
            onFinish = c::finishSetup, onSkip = c::finishSetup, onName = c::changePersonName
        )
    }
}

/** A detail that depends on the document, while it is still being read (or could not be). */
@Composable
private fun Waiting(title: String, state: DocumentState, onBack: () -> Unit) {
    DetailScaffold(title = title, onBack = onBack) {
        val error = state.error
        if (error != null) Notice(error, Tone.ERROR, Modifier.padding(16.dp)) else LoadingBox()
    }
}

@Composable
private fun Dialogs() {
    val c = Controller
    val colors = Ponte.colors
    val close = { c.dialog = null }
    when (val d = c.dialog) {
        null -> {}
        is AppDialog.Status -> AlertDialog(
            onDismissRequest = close,
            title = { Text("Sincronização") },
            text = {
                Column(
                    Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Text(d.text, style = MaterialTheme.typography.bodyMedium, color = if (d.error) colors.error else colors.text)
                    if (d.pending) {
                        Notice("Há alterações deste aparelho aguardando envio ao Drive. Elas vão na próxima sincronização.", Tone.WARNING)
                    }
                }
            },
            confirmButton = {
                TextButton(
                    onClick = { close(); c.requestSync(null) },
                    enabled = c.shell.syncEnabled && !c.shell.syncing
                ) { Text("Sincronizar agora") }
            },
            dismissButton = { TextButton(onClick = close) { Text("Fechar") } }
        )
        AppDialog.Reset -> AlertDialog(
            onDismissRequest = close,
            title = { Text("Recomeçar o acervo deste aparelho") },
            text = {
                Text(
                    "Isso apaga o acervo guardado neste aparelho. Só se perdem as alterações que ainda " +
                        "não foram enviadas; todo o resto volta do Drive na próxima sincronização. Continuar?",
                    style = MaterialTheme.typography.bodyMedium
                )
            },
            confirmButton = { TextButton(onClick = c::reset) { Text("Recomeçar", color = colors.error) } },
            dismissButton = { TextButton(onClick = close) { Text("Cancelar") } }
        )
        is AppDialog.DeleteAccount -> AlertDialog(
            onDismissRequest = close,
            title = { Text("Excluir a conta?") },
            text = {
                Text(
                    d.account.label + "\n\nOs documentos já importados continuam no acervo.",
                    style = MaterialTheme.typography.bodyMedium
                )
            },
            confirmButton = { TextButton(onClick = { c.deleteAccount(d.account) }) { Text("Excluir", color = colors.error) } },
            dismissButton = { TextButton(onClick = close) { Text("Cancelar") } }
        )
        is AppDialog.MailPassword -> {
            // Never saved: gone when the question closes.
            var pw by remember(d.account.uid) { mutableStateOf("") }
            AlertDialog(
                onDismissRequest = close,
                title = { Text("Senha de app") },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(
                            "A senha de app da conta " + d.account.label + " não está guardada neste aparelho. " +
                                "Ela vale só enquanto o aplicativo estiver aberto.",
                            style = MaterialTheme.typography.bodyMedium
                        )
                        LabeledField("Senha de app", pw, { pw = it }, password = true)
                        Text(
                            "É a senha criada no site do seu e-mail para aplicativos, não a senha da conta.",
                            style = MaterialTheme.typography.bodySmall, color = colors.muted
                        )
                    }
                },
                confirmButton = {
                    TextButton(onClick = { c.setMailPassword(d.account, pw) }, enabled = pw.isNotEmpty()) { Text("Listar mensagens") }
                },
                dismissButton = { TextButton(onClick = close) { Text("Cancelar") } }
            )
        }
    }
}
