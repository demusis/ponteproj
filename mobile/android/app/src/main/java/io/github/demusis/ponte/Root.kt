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
import androidx.compose.runtime.saveable.rememberSaveable
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

    // The Recursos screens say who holds what "now": the core decides it again when they open (the
    // list is read as the area is chosen) and then once a minute while one of them is in front.
    val resourcesVisible = !c.locked &&
        (top is Route.ResourceUse || top is Route.Resource || (top == null && c.tab == 1 && c.tasksSegment == TasksSegments.RESOURCES))
    LaunchedEffect(resourcesVisible) {
        while (resourcesVisible) {
            kotlinx.coroutines.delay(60_000L)
            c.refreshResources()
        }
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

    // The filter of the list in front (Tarefas: categories and project; Documentos: categories), behind one icon.
    val categories = LocalCategories.current
    val taskFilterOn = FilterIcon.tasksVisible(c.shell, categories, c.taskCatFilter, c.projects.rows, c.taskProjectFilter)
    val docFilterOn = FilterIcon.documentsVisible(c.shell, categories, c.docCatFilter)
    var filterOpen by rememberSaveable { mutableStateOf(false) }
    val shellFilter = when {
        taskFilterOn -> ShellFilter(FilterSummary.isActive(c.taskCatFilter, c.taskProjectFilter)) { filterOpen = true }
        docFilterOn -> ShellFilter(c.docCatFilter.isNotEmpty()) { filterOpen = true }
        else -> null
    }
    LaunchedEffect(shellFilter == null) { if (shellFilter == null) filterOpen = false }

    Box(Modifier.fillMaxSize().background(Ponte.colors.ground)) {
        if (top == null) {
            MainShell(
                state = c.shell,
                onTab = c::selectTab,
                onSync = { c.requestSync(null) },
                onSettings = c::openSettings,
                onStatus = c::showStatus,
                onSearch = c::openSearch,
                onArea = c::selectSegment,
                filter = shellFilter
            ) {
                holder.SaveableStateProvider("tab:" + c.tab) {
                    // Pull the list down to sync, wherever a sync makes sense (E-mail has its own list and button).
                    val body: @Composable () -> Unit = {
                        when (c.tab) {
                            0 -> TodayScreen(
                                c.today, onToggle = c::toggleTask, onOpen = { c.openTask(it.id) },
                                onOpenAttendance = { c.openAttendance(it.id) }, onOpenResources = c::openResources, week = c.week,
                                weekStart = c.weekStart, onMoveWeek = c::moveWeek, onOpenWeekEntry = c::openWeekEntry,
                                risks = c.risks, onOpenTask = c::openTask,
                                followUpCount = c.attendances.followUps.size, onOpenFollowUps = c::openFollowUps
                            )
                            // Tarefas | Projetos | Atendimentos | Recursos: they have no tab of their own (four is the limit);
                            // the title of the frame chooses among them.
                            1 -> when (c.tasksSegment) {
                                TasksSegments.TASKS -> TasksScreen(
                                    c.tasks, onToggle = c::toggleTask, onOpen = { c.openTask(it.id) },
                                    onNew = c::openNewTaskWithoutDocument,
                                    categoryFilter = c.taskCatFilter, onCategoryFilter = { c.taskCatFilter = it },
                                    projects = c.projects.rows, projectFilter = c.taskProjectFilter,
                                    onProjectFilter = { c.taskProjectFilter = it }
                                )
                                TasksSegments.PROJECTS -> ProjectsScreen(
                                    c.projects, filter = c.projFilter, onFilter = { c.projFilter = it },
                                    query = c.projQuery, onQuery = { c.projQuery = it },
                                    onOpen = { c.openProject(it.uid) }, onNew = { c.openProjectForm("") }
                                )
                                TasksSegments.RESOURCES -> ResourcesScreen(
                                    c.resources,
                                    query = c.resQuery, onQuery = c::typeResQuery,
                                    view = c.resView, onView = { c.resView = it },
                                    onOpenUse = { c.openResourceUse(it.uid) }, onOpenResource = { c.openResource(it.uid) }
                                )
                                else -> AttendancesScreen(
                                    c.attendances, query = c.attQuery, onQuery = { c.attQuery = it },
                                    onOpen = { c.openAttendance(it.id) },
                                    onRegisterFollowUp = c::registerFollowUp, onScheduleFollowUp = c::scheduleFollowUp,
                                    onSync = { c.requestSync(null) }
                                )
                            }
                            2 -> DocumentsScreen(
                                docs = c.docs,
                                query = c.docQuery, onQuery = c::typeDocQuery, matches = c.docMatches,
                                filter = c.docFilter, onFilter = { c.docFilter = it },
                                categoryFilter = c.docCatFilter, onCategoryFilter = { c.docCatFilter = it },
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
                    if (PullRules.enabled(c.tab)) {
                        PullToSync(c.shell.syncing, { c.requestSync(null) }, Modifier.fillMaxSize()) { body() }
                    } else {
                        body()
                    }
                }
            }
        } else {
            holder.SaveableStateProvider(top.key) { RouteScreen(top) }
        }

        if (filterOpen && top == null && shellFilter != null) {
            FilterSheet(onDismiss = { filterOpen = false }) {
                if (taskFilterOn) {
                    FilterSheetContent(
                        categories = if (FilterSummary.hasCategories(categories, c.taskCatFilter)) categories else null,
                        categoryChosen = c.taskCatFilter, onCategory = { c.taskCatFilter = it },
                        projects = if (FilterSummary.hasProjects(c.projects.rows, c.taskProjectFilter)) c.projects.rows else null,
                        projectChosen = c.taskProjectFilter, onProject = { c.taskProjectFilter = it },
                        onClear = { c.taskCatFilter = emptySet(); c.taskProjectFilter = "" },
                        onDone = { filterOpen = false }
                    )
                } else {
                    FilterSheetContent(
                        categories = categories, categoryChosen = c.docCatFilter, onCategory = { c.docCatFilter = it },
                        projects = null, projectChosen = "", onProject = {},
                        onClear = { c.docCatFilter = emptySet() }, onDone = { filterOpen = false }
                    )
                }
            }
        }

        // Above the bottom bar (or the footer of a detail); where a round button floats (Tarefas, Projetos, Documentos), above it too.
        val fab = c.tasksSegment == TasksSegments.TASKS || c.tasksSegment == TasksSegments.PROJECTS
        val lift = if (top == null && ((c.tab == 1 && fab) || c.tab == 2)) 136.dp else 64.dp
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
            onAttachFile = { d -> doc?.let { Intake.attachPick(it.id, d) } },
            onCategories = c::saveDocumentCategories, onCreateCategory = c::createCategory
        )
        is Route.EditTask -> TaskScreen(
            routeId = route.id, state = c.taskEdit, actions = c.actions, priorities = c.priorities,
            onBack = c::back, onSave = c::saveTask, onFinish = c::finishTask,
            onOpenDocument = { c.openTaskDocument() }, onLink = c::openPicker, onUnlink = c::unlinkTask,
            onReload = c::reloadTask, assignees = c.assignees, myName = c.personName,
            onCreateCategory = c::createCategory,
            remindersBlocked = c.taskNoticeBlocked, onRemindersChosen = c::taskRemindersChosen,
            relations = c.taskRelations, onOpenTask = c::openTask, projects = c.projects.rows
        )
        is Route.Attendance -> AttendanceScreen(
            routeId = route.id, state = c.attendance, onBack = c::back,
            onOpenDocument = c::openDocument, onOpenTask = c::openTask, onOpenFile = c::openFile
        )
        is Route.ResourceUse -> ResourceUseScreen(
            uid = route.uid, state = c.resources, onBack = c::back,
            onOpenResource = c::openResource, onOpenDocument = c::openDocument,
            onOpenAttendance = c::openAttendance, onOpenTask = c::openTask
        )
        is Route.Resource -> ResourceScreen(
            uid = route.uid, state = c.resources, onBack = c::back,
            onOpenUse = c::openResourceUse
        )
        Route.Search -> SearchScreen(
            state = c.search, onQuery = c::setSearchQuery, onBack = c::back,
            onOpenDocument = c::openDocument, onOpenTask = c::openTask, onOpenAttendance = c::openAttendance,
            projects = ProjectRules.search(c.projects.rows, c.search.query), onOpenProject = c::openProject
        )
        is Route.Project -> ProjectScreen(
            uid = route.uid, state = c.projectView, onBack = c::back,
            onEdit = { c.openProjectForm(route.uid) }, onNewTask = { c.openNewTaskInProject(route.uid) },
            onInclude = { c.openProjectInclude(route.uid) }, onStatus = c::setProjectStatus, onDelete = c::deleteProject,
            onToggleTask = c::toggleTask, onOpenTask = c::openTask, onRemoveTask = c::removeFromProject
        )
        is Route.ProjectEdit -> ProjectFormScreen(
            uid = route.uid, project = c.projects.byUid(route.uid), loading = c.projects.loading,
            state = c.projectEdit, assignees = c.assignees, myName = c.personName,
            onBack = c::back, onSave = { form, version -> c.saveProject(route.uid, version, form) },
            onReload = c::reloadProjectForm
        )
        is Route.ProjectInclude -> ProjectIncludeScreen(
            projectName = c.projects.byUid(route.uid)?.name ?: c.projectView.detail?.project?.name ?: "",
            tasks = c.tasks.tasks, state = c.projectInclude, onBack = c::back,
            onInclude = { c.includeTasks(route.uid, it) }
        )
        is Route.Pick -> DocumentPickerScreen(
            docs = c.docs, currentId = c.taskEdit.task?.documentId ?: 0L, busy = c.taskEdit.busy,
            onBack = c::back, onPick = c::pickDocument,
            matches = c.pickMatches, onQuery = c::setPickQuery
        )
        is Route.Text ->
            if (doc == null) Waiting("Texto do documento", c.document, c::back)
            else TextReaderScreen(title = docName, text = doc.text, onBack = c::back)
        is Route.Note ->
            if (doc == null) Waiting("Nota", c.document, c::back)
            else NoteEditorScreen(title = docName, initial = doc.note, onBack = c::back, onSave = c::saveNote,
                saving = c.noteSave.busy, saveError = c.noteSave.error)
        is Route.NewTask -> NewTaskScreen(
            document = if (route.id > 0) docName else "",
            // A task without document has nothing to triage.
            actions = if (route.id > 0) c.actions else c.actions.filter { it.key != "triar" },
            onBack = c::back, onCreate = c::createTask, withoutDocument = route.id <= 0,
            assignees = c.assignees, myName = c.personName, onCreateCategory = c::createCategory,
            remindersBlocked = c.taskNoticeBlocked, onRemindersChosen = c::taskRemindersChosen,
            saving = c.newTaskSave.busy, saveError = c.newTaskSave.error,
            projects = c.projects.rows, projectUid = route.projectUid
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
                onSaveAttach = Intake::saveAttach, onOpenEarlier = Intake::openEarlier
            )
        }
        Route.SharePick -> DocumentPickerScreen(
            docs = c.docs, currentId = 0L, busy = false,
            onBack = c::back, onPick = Intake::pickTarget, title = "Anexar a um documento",
            matches = c.pickMatches, onQuery = c::setPickQuery
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
            onNoticeSettings = c::openNoticeSettings,
            onHolidayState = c::setHolidayState, onAddHolidayDay = c::addHolidayDay,
            onRemoveHolidayDay = c::removeHolidayDay,
            onCalendar = c::setCalendarEnabled, onCalendarChoice = c::chooseCalendar,
            onCalendarRefresh = c::refreshCalendars
        )
        is Route.Account -> AccountFormScreen(
            state = c.accountForm, onBack = c::back, onSave = c::saveAccount, onDelete = c::askDeleteAccount
        )
        Route.Setup -> SetupScreen(
            state = c.setup, onPickFile = c::pickFile,
            onPassphrase = c::setSetupPassphrase, onRemember = c::setSetupRemember,
            onNext = c::setupNext, onBack = c::back, onSync = c::setupSync,
            onFinish = c::finishSetup, onSkip = c::finishSetup, onName = c::changePersonName,
            onSyncMobile = c::setupSyncMobile
        )
    }
}

/** A detail that depends on the document, while it is still being read (or could not be). */
@Composable
private fun Waiting(title: String, state: DocumentState, onBack: () -> Unit) {
    DetailScaffold(title = title, onBack = onBack) {
        val error = state.error
        if (error != null) Notice(error, tone = Tone.ERROR, modifier = Modifier.padding(16.dp)) else LoadingBox()
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
                        Notice("Há alterações deste aparelho aguardando envio ao Drive. Elas vão na próxima sincronização.", tone = Tone.WARNING)
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
        AppDialog.CalendarOff -> AlertDialog(
            onDismissRequest = close,
            title = { Text("Desligar a agenda") },
            text = {
                Text(
                    "Remover da agenda os eventos que o Ponte criou? As tarefas não mudam.",
                    style = MaterialTheme.typography.bodyMedium
                )
            },
            confirmButton = { TextButton(onClick = { c.calendarOff(true) }) { Text("Remover os eventos", color = colors.error) } },
            dismissButton = { TextButton(onClick = { c.calendarOff(false) }) { Text("Deixar na agenda") } }
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
