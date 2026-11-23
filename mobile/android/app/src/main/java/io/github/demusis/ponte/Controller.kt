package io.github.demusis.ponte

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.MediaStore
import android.provider.OpenableColumns
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.Lifecycle
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import kotlin.reflect.KProperty
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/** A state that, when it changes, says so ([onSet]); used by what the lists keep between runs. */
private class Kept<T>(initial: T, private val onSet: () -> Unit) {
    private val state = mutableStateOf(initial)
    operator fun getValue(thisRef: Any?, property: KProperty<*>): T = state.value
    operator fun setValue(thisRef: Any?, property: KProperty<*>, value: T) {
        if (state.value == value) return
        state.value = value
        onSet()
    }
}

/** Passwords typed for this session (not remembered on the device). Memory only. */
internal object MailSession {
    val passwords = HashMap<String, String>()
}

/** A full screen shown over the three areas. [key] names it in the saved state. */
internal sealed class Route {
    abstract val key: String

    /** The ficha of a document. */
    data class Document(val id: Long) : Route() { override val key get() = "doc:$id" }

    /** The whole text of a document, for reading. */
    data class Text(val id: Long) : Route() { override val key get() = "text:$id" }

    /** The note editor. */
    data class Note(val id: Long) : Route() { override val key get() = "note:$id" }

    /** A new task for a document (0: none); [projectUid] presets its project (empty: none). */
    data class NewTask(val id: Long, val projectUid: String = "") : Route() {
        override val key get() = if (projectUid.isEmpty()) "task:$id" else "task:$id@$projectUid"
    }

    /** The screen of one project: its tasks and its timeline. */
    data class Project(val uid: String) : Route() { override val key get() = "proj:$uid" }

    /** The form of a project; an empty uid is a new project. */
    data class ProjectEdit(val uid: String) : Route() { override val key get() = "projedit:$uid" }

    /** The list to pick the tasks to include in the project [uid]. */
    data class ProjectInclude(val uid: String) : Route() { override val key get() = "projinc:$uid" }

    /** The screen of one task (edit, conclude, change its document). */
    data class EditTask(val id: Long) : Route() { override val key get() = "edit:$id" }

    /** The list to pick the document of task [taskId] from. */
    data class Pick(val taskId: Long) : Route() { override val key get() = "pick:$taskId" }

    /** The ficha of an atendimento (read-only). */
    data class Attendance(val id: Long) : Route() { override val key get() = "att:$id" }

    /** The screen of one use of a resource (read-only), by the uid the core gave it. */
    data class ResourceUse(val uid: String) : Route() { override val key get() = "use:$uid" }

    /** The screen of one resource (read-only): its data, next uses and today's occupation. */
    data class Resource(val uid: String) : Route() { override val key get() = "res:$uid" }

    /** The search over documents, tasks and atendimentos. */
    object Search : Route() { override val key get() = "search" }

    object Photo : Route() { override val key get() = "photo" }

    /**
     * "Guardar no Ponte": what another app shared. Not restored from the saved state of the screen ([parse]
     * answers null): after the process dies the share comes back from its own draft on the disk
     * ([Intake.restoreDraft]), which keeps the item and the form. The document picker of it is not restored.
     */
    object Share : Route() { override val key get() = "share" }

    /** The list of documents to attach a shared file to. */
    object SharePick : Route() { override val key get() = "sharepick" }
    object Settings : Route() { override val key get() = "settings" }

    /** The account form; an empty uid is a new account. */
    data class Account(val uid: String) : Route() { override val key get() = "account:$uid" }

    object Setup : Route() { override val key get() = "setup" }

    companion object {
        fun parse(s: String): Route? {
            val arg = s.substringAfter(':', "")
            return when (s.substringBefore(':')) {
                "doc" -> arg.toLongOrNull()?.let { Document(it) }
                "text" -> arg.toLongOrNull()?.let { Text(it) }
                "note" -> arg.toLongOrNull()?.let { Note(it) }
                "task" -> arg.substringBefore('@').toLongOrNull()?.let { NewTask(it, arg.substringAfter('@', "")) }
                "proj" -> arg.takeIf { it.isNotEmpty() }?.let { Project(it) }
                "projedit" -> ProjectEdit(arg)
                "projinc" -> arg.takeIf { it.isNotEmpty() }?.let { ProjectInclude(it) }
                "edit" -> arg.toLongOrNull()?.let { EditTask(it) }
                "pick" -> arg.toLongOrNull()?.let { Pick(it) }
                "att" -> arg.toLongOrNull()?.let { Attendance(it) }
                "use" -> arg.takeIf { it.isNotEmpty() }?.let { ResourceUse(it) }
                "res" -> arg.takeIf { it.isNotEmpty() }?.let { Resource(it) }
                "search" -> Search
                "photo" -> Photo
                "settings" -> Settings
                "account" -> Account(arg)
                "setup" -> Setup
                else -> null
            }
        }
    }
}

/** The name this phone gives itself in the library, when it opens it (`init`). */
internal fun phoneMemberName(): String = "Celular (" + android.os.Build.MODEL + ")"

/** A question or message shown over whatever is on screen. */
internal sealed class AppDialog {
    /** The whole text of the last sync status. */
    data class Status(val text: String, val error: Boolean, val pending: Boolean) : AppDialog()
    object Reset : AppDialog()
    data class DeleteAccount(val account: MailAccount) : AppDialog()
    data class MailPassword(val account: MailAccount) : AppDialog()

    /** Turning off the calendar sync: what to do with the events already there. */
    object CalendarOff : AppDialog()
}

/**
 * What the app shows and does. Lives as long as the process, so a recreated Activity (theme
 * change, rotation) finds the same state and running work; after the process dies the
 * navigation and the photo form come back from the saved instance state.
 *
 * Everything here runs on the UI thread. The passphrase and the mail passwords are never put
 * in the saved state.
 */
@Suppress("StaticFieldLeak") // Only the application context is retained.
internal object Controller {
    private const val SERVER_MSG = "Conversando com o servidor de e-mail…"
    private const val PHOTO_FAILED =
        "Não foi possível guardar o registro. Confira o espaço livre no aparelho e tente de novo."
    private const val START_FAILED ="Não foi possível iniciar a sincronização. Tente de novo."
    private const val K_LOCK = "lock_on"
    private const val K_LOCK_AFTER = "lock_after"
    private const val K_SECURE = "secure_screen"
    private const val K_NAME = "person_name"
    private const val CALENDAR_WAIT_MS = 2000L
    private const val CALENDAR_NOT_ENABLED =
        "Não foi possível ligar a sincronização desta agenda. No app Google Agenda, em Configurações, abra a agenda e deixe Sincronizar ligado."
    private const val CALENDAR_DENIED =
        "Sem a permissão da agenda, as tarefas não podem ser sincronizadas com ela. Permita-a quando o telefone perguntar, ou nas configurações do telefone."

    @Suppress("StaticFieldLeak") // Assigned ctx.applicationContext in init.
    private lateinit var app: Context
    private val prefs get() = app.getSharedPreferences("ponte", Context.MODE_PRIVATE)
    private val activity: MainActivity? get() = MainActivity.live
    private val main = Handler(Looper.getMainLooper())
    private val scope = MainScope()

    private var started = false
    private var idleTask: (() -> Unit)? = null
    private var wasRunning = false
    private var fileUri: Uri? = null
    private var fileName = ""
    private var fileSize = -1L
    private var lastSync = 0L

    val snackbar = SnackbarHostState()

    var tab by mutableIntStateOf(0)
        private set
    val stack = mutableStateListOf<Route>()
    var shell by mutableStateOf(ShellState())
        private set
    var docs by mutableStateOf<List<Doc>>(emptyList())
        private set
    var docQuery by mutableStateOf("")
    var docFilter by Kept(DocFilter.ALL) { persistFilters() }
    /** Ids of the documents the core's search found for [docQuery]; null while nothing is typed. */
    var docMatches by mutableStateOf<Set<Long>?>(null)
        private set
    /** The same for the text typed in the document picker. */
    var pickMatches by mutableStateOf<Set<Long>?>(null)
        private set

    /** Every category of the library; chips and pickers read it (archived ones included). */
    var categories by mutableStateOf<List<Category>>(emptyList())
        private set

    /** The category filters of Tarefas and Documentos: chosen uids, plus [CategoryRules.NONE]. */
    var taskCatFilter by Kept<Set<String>>(emptySet()) { persistFilters() }
    var docCatFilter by Kept<Set<String>>(emptySet()) { persistFilters() }

    /** The project filter of Tarefas: empty (all), [ProjectRules.NONE] or a project uid. */
    var taskProjectFilter by Kept("") { persistFilters() }
    var taskDocumentFilter by Kept<Long?>(null) { documentFilterChanged() }
    var tasks by mutableStateOf(TasksState())
    var taskRelations by mutableStateOf(TasksState())
        private set

    /** The Projetos list of the Tarefas area, read again with the tasks and after a sync. */
    var projects by mutableStateOf(ProjectsState())
        private set

    /** The situation chosen on the Projetos list, and the text typed in its search box (memory only). */
    var projFilter by mutableStateOf(ProjectRules.DEFAULT_FILTER)
    var projQuery by mutableStateOf("")

    /** The project screen, its form and the "Incluir tarefas" list. */
    var projectView by mutableStateOf(ProjectViewState())
        private set
    var projectEdit by mutableStateOf(ProjectEditState())
        private set
    var projectInclude by mutableStateOf(SaveState())
        private set

    /** What the Tarefas area shows: one of the ids of [TasksSegments]. */
    var tasksSegment by Kept(0) { persistFilters() }
        private set
    var attendances by mutableStateOf(AttendancesState())
        private set

    /** The Recursos list of the Tarefas area (uses and register); read again with the tasks and after a sync. */
    var resources by mutableStateOf(ResourcesState())
        private set

    /** What is typed in the search box of the Recursos list (memory only), and which of its two views: 0 Usos, 1 Recursos. */
    var resQuery by mutableStateOf("")
    var resView by mutableIntStateOf(0)

    /** What is typed in the search box of the Atendimentos list (memory only). */
    var attQuery by mutableStateOf("")
    var attendance by mutableStateOf(AttendanceViewState())
        private set
    var search by mutableStateOf(SearchState())
        private set

    /** The names used as responsible person, most used first (the suggestions under the field). */
    var assignees by mutableStateOf<List<String>>(emptyList())
        private set

    /** "Seu nome nas tarefas", as saved (trimmed); empty when the person did not say. */
    var personName by mutableStateOf("")
        private set
    var today by mutableStateOf(TodayState())
        private set
    var taskEdit by mutableStateOf(TaskEditState())
        private set
    var priorities by mutableStateOf(DefaultPriorities)
        private set
    var document by mutableStateOf(DocumentState())
        private set
    var actions by mutableStateOf<List<Choice>>(emptyList())
        private set
    var photo by mutableStateOf(PhotoState())
        private set
    var mail by mutableStateOf(MailState())
        private set
    var settings by mutableStateOf(SettingsState())
        private set
    var accountForm by mutableStateOf(AccountFormState())
        private set
    var setup by mutableStateOf(SetupState())
        private set
    var dialog by mutableStateOf<AppDialog?>(null)

    private var documentId = -1L
    private var manualKinds: List<Choice> = emptyList()

    private fun ui(block: () -> Unit) {
        main.post { try { block() } catch (_: Throwable) { } }
    }

    // ---------------------------------------------------------------- life cycle

    /** Called by every new Activity, before it shows anything. */
    fun attach(a: MainActivity, saved: Bundle?) {
        app = a.applicationContext
        Intake.init(app)
        Holidays.configure(prefs.getString(SyncRunner.HOLIDAYS_KEY, "") ?: "")
        personName = Assignees.cleanName(prefs.getString(K_NAME, "") ?: "")
        // The deadline notice's alarm is set again whenever the app opens (cheap, and it heals a lost one).
        NoticeScheduler.arm(app)
        fileUri = prefs.getString("uri", null)?.let { Uri.parse(it) }
        lastSync = prefs.getLong("last_sync", 0L)
        val fresh = saved == null
        if (fresh) cleanTempAsync()
        if (!started) {
            // A new process always starts behind the lock (if the lock is in force), and nothing
            // handed to other apps in an earlier run is kept. Photos of a form that is coming
            // back from the saved state are the exception.
            locked = LockPolicy.shouldLock(lockSetting(), deviceSecure(), lockDelay(), null, coldStart = true)
            if (locked) lockEpoch++
            val b = saved?.getBundle("photo")
            cleanHandedFiles(setOfNotNull(b?.getString("photo"), b?.getString("new")))
            // An attachment being prepared dies with the process: its files go too. A share in progress is a
            // draft kept in the private files folder: it is not among them (see restoreDraft below).
            Intake.cleanAll(app)
        }
        if (!SyncRunner.ready && !SyncRunner.keyBroken && !SyncRunner.running.get()) {
            SyncRunner.startInit(a, false)
        }
        if (!started) {
            started = true
            restoreFilters()
            if (saved != null) restore(saved) else idleTask = { startup() }
            // The share that was in progress when the process died is read OFF this thread (the draft, its copy, the
            // library check) and comes back on top of the stack when ready (behind the lock when it is in force:
            // nothing of it is composed or said before the unlock).
            Intake.restoreDraft()
            loadChoices()
            refreshTasks()
        } else if (fresh) {
            // The person left with Back and came again while the process was still alive.
            if (!photo.saving) {
                closePhoto()
                val keepShare = Intake.reset()
                stack.clear()
                tab = 0
                // A share in progress is a draft: it stays, as it would after a restart.
                if (keepShare) stack.add(Route.Share)
            }
            dialog = null
            idleTask = { startup() }
            refreshTasks()
        }
        render()
    }

    fun save(out: Bundle) {
        out.putInt("tab", tab)
        out.putStringArrayList("stack", ArrayList(stack.map { it.key }))
        out.putString("query", docQuery)
        out.putString("filter", docFilter.name)
        out.putInt("segment", tasksSegment)
        if (Route.Photo in stack) {
            out.putBundle("photo", Bundle().apply {
                putString("photo", photoPath)
                putString("new", newPath)
                putString("kind", photo.kind)
                putString("title", photo.title)
                putString("origin", photo.origin)
                putString("text", photo.text)
                putString("date", photo.date)
            })
        }
    }

    /** After the process died: navigation and the photo form come back. */
    private fun restore(s: Bundle) {
        tab = s.getInt("tab", 0).coerceIn(0, 3)
        tasksSegment = s.getInt("segment", 0).coerceIn(0, TasksSegments.PROJECTS)
        docQuery = s.getString("query") ?: ""
        docFinder.ask(docQuery)
        docFilter = try { DocFilter.valueOf(s.getString("filter") ?: "") } catch (_: Throwable) { DocFilter.ALL }
        stack.clear()
        s.getStringArrayList("stack")?.mapNotNull { Route.parse(it) }?.forEach { stack.add(it) }
        // The first setup starts over (the passphrase is never saved).
        if (Route.Setup in stack) setup = newSetup()
        // One task screen at most: only the top-most one comes back, with the list that belongs to it.
        val kept = TaskRules.restored(stack.toList())
        stack.clear()
        stack.addAll(kept)
        val editor = stack.lastOrNull { it is Route.EditTask } as Route.EditTask?
        if (editor != null) {
            taskGen++
            taskEdit = TaskEditState(id = editor.id)
        }
        for (r in stack.toList()) {
            when (r) {
                is Route.Document -> documentId = r.id
                is Route.Text -> documentId = r.id
                is Route.Note -> documentId = r.id
                is Route.NewTask -> if (r.id > 0) documentId = r.id
                is Route.EditTask -> { loadTask(r.id); loadTaskRelations(r.id) }
                is Route.Pick -> {}
                // Read from the lists, which are read again at start.
                is Route.ResourceUse, is Route.Resource -> {}
                // One project screen at most (see openProject): the top-most one comes back.
                is Route.Project -> if (r == stack.lastOrNull { it is Route.Project }) {
                    projGen++
                    projectView = ProjectViewState(uid = r.uid)
                    loadProject(r.uid)
                } else stack.remove(r)
                // The form and the list start over; the projects are read again at start.
                is Route.ProjectEdit, is Route.ProjectInclude -> {}
                // One atendimento screen at most (see openAttendance): the top-most one comes back.
                is Route.Attendance -> if (r == stack.lastOrNull { it is Route.Attendance }) {
                    attGen++
                    attendance = AttendanceViewState(id = r.id)
                    loadAttendance(r.id)
                } else stack.remove(r)
                // The text of a search is never saved: it comes back empty.
                Route.Search -> {}
                is Route.Account -> loadAccounts { list ->
                    val found = list.firstOrNull { it.uid == r.uid }
                    // An account that no longer exists must not come back as a new one.
                    if (r.uid.isNotEmpty() && found == null) stack.remove(r)
                    else accountForm = AccountFormState(account = found)
                }
                Route.Settings, Route.Setup -> loadFileInfo()
                Route.Photo -> restorePhoto(s.getBundle("photo"))
                // Never in the saved stack (see Route.Share), but the match must be complete.
                Route.Share, Route.SharePick -> stack.remove(r)
            }
        }
        if (documentId >= 0) loadDocument()
        if (tab == 3) openMailTab()
    }

    /**
     * Old export/photo files from a previous run are not needed any more. The scan and the deletions run on a
     * background thread, never on the UI thread. What is in use is read here, on the UI thread, before the
     * thread starts (the photo of a form in progress, a camera picture waiting to be attached, a record being
     * saved), and anything touched in the last hour is left alone, so a share or an intake in progress
     * never loses its file.
     */
    private fun cleanTempAsync() {
        val keep = setOfNotNull(photoPath, newPath) + Intake.keepPaths()
        val saving = photo.saving
        val cache = app.cacheDir
        val worker = Thread({
            try {
                val now = System.currentTimeMillis()
                for (d in listOf(PonteFiles.DIR_SHARED, PonteFiles.DIR_PHOTOS)) {
                    File(cache, d).listFiles()?.forEach {
                        if (it.isFile) TempRules.deleteIfStale(it, it.lastModified(), now, keep, saving)
                    }
                }
                cache.listFiles()?.forEach {
                    if (it.isFile && TempRules.isSyncExport(it.name)) TempRules.deleteIfStale(it, it.lastModified(), now, keep, saving)
                }
            } catch (_: Throwable) {
            }
        }, "ponte-clean-temp")
        worker.isDaemon = true
        worker.start()
    }

    private fun startup() {
        if (SyncRunner.keyBroken) {
            if (Route.Settings !in stack) openSettings()
            return
        }
        if (fileUri == null) {
            if (stack.isEmpty()) {
                setup = newSetup()
                stack.add(Route.Setup)
            }
            return
        }
        if (SyncRunner.passphrase.isEmpty() && !Secrets.isRemembered(app)) {
            status("Informe a senha de sincronização em Ajustes para sincronizar.", false, notify = false)
            return
        }
        requestSync(null, auto = true)
    }

    private fun loadChoices() {
        SyncRunner.background({ Api.choices() }, { ch ->
            actions = ch.optJSONArray("actions").objs().map { Choice(it.s("key"), it.s("label")) }
            val k = ch.optJSONArray("manual_kinds").objs().map { Choice(it.s("key"), it.s("label")) }
            if (k.isNotEmpty()) {
                manualKinds = k
                photo = photo.copy(kinds = k)
                Intake.onKinds(k)
            }
            ch.optJSONObject("limits")?.let { l ->
                AttachLimits.maxBytes = l.optLong("max_attachment_bytes", AttachLimits.maxBytes)
                AttachLimits.maxNameChars = l.optInt("max_name_chars", AttachLimits.maxNameChars)
                AttachLimits.maxDescriptionChars = l.optInt("max_description_chars", AttachLimits.maxDescriptionChars)
                AttachLimits.maxTextBytes = l.optInt("max_text_bytes", AttachLimits.maxTextBytes)
                AttachLimits.maxTitleBytes = l.optInt("max_title_bytes", AttachLimits.maxTitleBytes)
            }
            val p = ch.optJSONArray("priorities").objs().map { Choice(it.s("key"), it.s("label")) }
            if (p.isNotEmpty()) priorities = p
        })
    }

    private fun newSetup() = SetupState(
        canRemember = Secrets.supported() && deviceSecure(), oldAndroid = !Secrets.supported(),
        personName = personName
    )

    // ---------------------------------------------------------------- app lock
    //
    // An access gate in the interface, not encryption: while [locked] nothing of the library is
    // composed (see AppRoot) and no message, prompt or file is handed out. The library on disk
    // is sealed by its own Keystore key either way. It keeps out someone who picks up the phone
    // with the app open or recently used; it does not help against someone who knows the phone's
    // PIN or against a rooted phone. "Locked" and the time spent away live in memory only.

    /** The lock screen replaces everything. A new process starts locked (see [attach]). */
    var locked by mutableStateOf(false)
        private set

    /** Grows at every lock: work started before a lock knows the files it made may be gone. */
    var lockEpoch = 0
        private set

    private var backgroundAt = -1L
    private var unlockBusy = false
    private var deferred = false
    private var cryptoUnlockFailed = false
    private var lockNote: String? = null
    private var autoPrompted = false
    private var openedExternally = false
    private var externalStopped = false

    private fun deviceSecure() = AppLock.deviceSecure(app)

    /** The saved switch; never chosen yet means "on when the phone has a screen lock". */
    private fun lockSetting() =
        if (prefs.contains(K_LOCK)) prefs.getBoolean(K_LOCK, true) else LockPolicy.defaultEnabled(deviceSecure())

    private fun lockDelay() = LockDelay.fromKey(prefs.getString(K_LOCK_AFTER, null))
    fun secureScreen() = prefs.getBoolean(K_SECURE, true)

    // ---------------------------------------------------------------- what the lists keep between runs

    /** True while the saved filters are being put back: nothing is written meanwhile. */
    private var restoringFilters = false

    /** The document filter saved last run, waiting for the documents to be read to be checked ([restoreFilters]). */
    private var pendingDocumentFilter: SavedDocument? = null

    /** The document of the filter as it is kept: not looked up again on every change of another filter. */
    private var keptDocument: SavedDocument? = null

    /** Puts back the filters and the area of the last run; the document is checked when the documents are read. */
    private fun restoreFilters() = applyFilters(FilterMemory.decode(prefs.getString(FilterMemory.KEY, null)))

    /** A new library (a reset, or another sync file): the lists start unfiltered, here and in the preferences. */
    private fun clearFilters() {
        applyFilters(FilterMemory.CLEARED)
        try {
            prefs.edit().remove(FilterMemory.KEY).apply()
        } catch (_: Throwable) {
        }
    }

    private fun applyFilters(kept: SavedFilters) {
        restoringFilters = true
        try {
            taskCatFilter = kept.taskCategories
            docCatFilter = kept.docCategories
            taskProjectFilter = kept.project
            tasksSegment = kept.segment
            docFilter = kept.docFilter
            // "Sem documento" needs no document; any other waits for the library to be read (a local id may have changed).
            taskDocumentFilter = if (kept.document?.id == 0L) 0L else null
            pendingDocumentFilter = kept.document.takeIf { it?.id != 0L }
            keptDocument = kept.document
        } finally {
            restoringFilters = false
        }
        shell = shell.copy(
            attendances = tasksSegment == TasksSegments.ATTENDANCES,
            resources = tasksSegment == TasksSegments.RESOURCES,
            projects = tasksSegment == TasksSegments.PROJECTS
        )
    }

    private fun documentFilterChanged() {
        // The person's own choice wins over a saved one still waiting for the documents.
        if (!restoringFilters) pendingDocumentFilter = null
        keptDocument = FilterMemory.savedDocument(taskDocumentFilter, docs.ifEmpty { SyncRunner.docs })
        persistFilters()
    }

    private fun persistFilters() {
        if (restoringFilters || !::app.isInitialized) return
        val saved = SavedFilters(
            taskCategories = taskCatFilter, docCategories = docCatFilter, project = taskProjectFilter,
            document = keptDocument, segment = tasksSegment, docFilter = docFilter
        )
        try {
            prefs.edit().putString(FilterMemory.KEY, FilterMemory.encode(saved)).apply()
        } catch (_: Throwable) {
        }
    }

    /** The Activity came to the front (not for a rotation: it is not stopped then). */
    fun onActivityStart() {
        // Edits made in the calendar app while away are picked up now, and the ones made from here on as they happen.
        watchCalendar(true)
        if (calendarOn()) main.postDelayed(calDebounce, CALENDAR_WAIT_MS)
        // Back from the Google Agenda app: a calendar switched on there shows up in the list.
        if (Route.Settings in stack && calendarOn()) loadCalendars()
        val t = backgroundAt
        backgroundAt = -1L
        if (externalStopped) {
            // Back from the app that opened a file: it is no longer in front.
            externalStopped = false
            openedExternally = false
            cleanHandedFiles()
        }
        if (locked) {
            // The phone's screen lock was removed meanwhile: the app must still open.
            if (!LockPolicy.active(lockSetting(), deviceSecure())) unlocked()
            return
        }
        if (deferred) {
            if (unlockBusy || Secrets.busy) {
                // A prompt is still pending: its answer decides (see promptDone).
                backgroundAt = t
                return
            }
            deferred = false
        }
        val away = if (t < 0) null else SystemClock.elapsedRealtime() - t
        if (LockPolicy.shouldLock(lockSetting(), deviceSecure(), lockDelay(), away, coldStart = false)) lockNow()
        else syncOnReturn()
    }

    /** When the last return that asked for a sync did (memory only): see [ForegroundSync]. */
    private var returnSyncAt = 0L

    /**
     * Back in front (or just unlocked): a sync of its own when the last good one is old enough ([ForegroundSync]).
     * It is the automatic kind (nothing said at the bottom unless it fails), through [requestSync], so the Wi-Fi
     * rule, a sync already running and the passphrase are all respected; it never asks for the fingerprint by itself.
     */
    private fun syncOnReturn() {
        if (!::app.isInitialized || !activityInFront()) return
        val configured = fileUri != null && !SyncRunner.keyBroken && SyncRunner.ready && SyncRunner.passphrase.isNotEmpty() &&
            idleTask == null && Route.Setup !in stack && !unlockBusy && !Secrets.busy
        val now = System.currentTimeMillis()
        val editing = ForegroundSync.editing(stack.toList())
        if (!ForegroundSync.shouldSync(now, lastSync, locked, SyncRunner.running.get(), configured, returnSyncAt, editing)) return
        returnSyncAt = now
        requestSync(null, auto = true)
    }

    /**
     * A system prompt of the app (the lock's own or a passphrase one) has answered. When the
     * app left the front while it was pending, the lock decision was postponed to here: a
     * success is a fresh authentication (no lock), a failure or cancel decides by the time away.
     */
    private fun promptDone(success: Boolean) {
        if (!deferred) return
        deferred = false
        val t = backgroundAt
        if (success) {
            backgroundAt = -1L
            return
        }
        if (locked || t < 0) return
        val away = SystemClock.elapsedRealtime() - t
        if (LockPolicy.lockAfterPrompt(lockSetting(), deviceSecure(), lockDelay(), away, false)) lockNow()
        // In front again: this time is spent. Still away: onActivityStart decides later.
        if (activityInFront()) backgroundAt = -1L
    }

    /**
     * The Activity left the front. A rotation or similar is not leaving; neither is the system
     * prompt of the lock itself. Trips the app started (file picker, camera, a viewer) count as
     * time away like any other.
     */
    fun onActivityStop(changingConfiguration: Boolean) {
        if (changingConfiguration) return
        watchCalendar(false)
        if (openedExternally) externalStopped = true
        backgroundAt = SystemClock.elapsedRealtime()
        autoPrompted = false
        if (locked) return
        if (unlockBusy || Secrets.busy) {
            // The app's own prompt or intent is in front: decide when it answers.
            deferred = true
            return
        }
        // "Imediatamente": lock now, so neither the first frame on return nor the recents
        // thumbnail shows content. A file just handed to a viewer is not deleted yet (the
        // viewer is reading it): that happens when the person is back.
        if (LockPolicy.shouldLock(lockSetting(), deviceSecure(), lockDelay(), 0L, coldStart = false) &&
            lockDelay() == LockDelay.NOW
        ) lockNow(clean = !openedExternally)
    }

    fun onActivityResumed() {
        // The prompt appears by itself once each time the lock screen is in front.
        if (locked && !autoPrompted && !unlockBusy) {
            autoPrompted = true
            main.post { requestUnlock() }
        }
    }

    /** Only for the instance that is the live one: an old one must not cancel a newer prompt. */
    fun onActivityDestroyed() {
        AppLock.cancel()
        unlockBusy = false
    }

    /** Back on the lock screen: the app goes to the background, still locked, nothing lost. */
    fun moveToBack() {
        activity?.moveTaskToBack(true)
    }

    private fun lockNow(clean: Boolean = true) {
        locked = true
        lockEpoch++
        autoPrompted = false
        // What was searched and found is not kept behind the lock, and a search still running is dropped.
        clearSearch()
        snackbar.currentSnackbarData?.dismiss()
        if (clean) cleanHandedFiles()
    }

    private fun unlocked() {
        if (!locked) return
        locked = false
        // Said only now, never over the lock screen.
        lockNote?.let { lockNote = null; status(it, false) }
        render()
        // The return that was locked: the sync waits for the unlock (a cold start has its own, already running).
        syncOnReturn()
    }

    /**
     * Asks the person to unlock. When a sync is waiting for the stored passphrase on Android 11+,
     * the prompt IS the one that releases the passphrase (a successful release counts as
     * unlocking), so opening the app asks once. Otherwise the plain screen-lock prompt.
     */
    fun requestUnlock() {
        if (!locked || unlockBusy) return
        val a = activity ?: return
        if (!activityInFront()) return
        if (!LockPolicy.active(lockSetting(), deviceSecure())) {
            unlocked()
            return
        }
        unlockBusy = true
        val title = "Desbloquear o Ponte de bolso"
        val syncDue = idleTask != null && fileUri != null && !SyncRunner.keyBroken
        if (LockPolicy.unlockReleasesPassphrase(
                android.os.Build.VERSION.SDK_INT, Secrets.isRemembered(app), SyncRunner.passphrase.isNotEmpty(),
                syncDue, cryptoUnlockFailed
            )
        ) {
            Secrets.unlock(a, title) { pass, msg ->
                if (pass != null) {
                    unlockBusy = false
                    SyncRunner.passphrase = pass
                    unlocked()
                } else if (Secrets.lastCancelled) {
                    unlockBusy = false
                } else {
                    // The key prompt failed for another reason (key invalid, fingerprint-only key,
                    // cipher error): never use it as the unlock again in this process, keep what it
                    // said for the sync status, and ask with the plain screen-lock prompt at once.
                    cryptoUnlockFailed = true
                    if (!msg.isNullOrEmpty()) lockNote = msg
                    AppLock.verify(a, title, { unlockBusy = false; unlocked() }, { unlockBusy = false })
                }
            }
        } else {
            AppLock.verify(a, title, { unlockBusy = false; unlocked() }, { unlockBusy = false })
        }
    }

    /** Turning the lock off needs the screen lock first; turning it on does not. */
    fun setLockEnabled(on: Boolean) {
        if (on) {
            if (deviceSecure()) prefs.edit().putBoolean(K_LOCK, true).apply()
            TodayWidget.conceal(app)
            TodayWidget.refresh(app)
            updateSettings()
            return
        }
        val a = activity
        if (!deviceSecure()) {
            // Nothing to prove with: the lock is not in force anyway.
            prefs.edit().putBoolean(K_LOCK, false).apply()
            TodayWidget.refresh(app)
            updateSettings()
            return
        }
        if (a == null || unlockBusy) return
        unlockBusy = true
        AppLock.verify(a, "Desligar o bloqueio", {
            unlockBusy = false
            prefs.edit().putBoolean(K_LOCK, false).apply()
            TodayWidget.refresh(app)
            updateSettings()
            promptDone(true)
        }, { unlockBusy = false; promptDone(false) })
    }

    fun setLockDelay(key: String) {
        prefs.edit().putString(K_LOCK_AFTER, LockDelay.fromKey(key).key).apply()
        updateSettings()
    }

    fun setSecureScreen(on: Boolean) {
        prefs.edit().putBoolean(K_SECURE, on).apply()
        activity?.applySecure(on)
        updateSettings()
    }

    // ---- holidays of this phone (a state and extra days off)

    /** Keeps [config], uses it in the phone's rules, hands it to the core and reads the planning again. */
    private fun applyHolidays(config: HolidayConfig) {
        val text = config.toText()
        prefs.edit().putString(SyncRunner.HOLIDAYS_KEY, text).apply()
        Holidays.configure(text)
        updateSettings()
        // Hoje, Semana and the planning are worked out by the core: read them again with the new days.
        // Always queued: the worker is serial, so a change made while the core is still opening
        // lands after its startup read of the preferences instead of being lost.
        SyncRunner.background(
            { Api.setHolidays(text) },
            { confirmed ->
                // What the core stored is the truth: a sync that ran before this save may have shown the
                // library's older value over the edit.
                SyncRunner.mirrorHolidays(app, confirmed)
                updateSettings()
                if (SyncRunner.ready) refreshTasks()
            },
            { say("Não foi possível salvar os feriados no acervo: $it", long = true) }
        )
    }

    /** [uf] is a state code, or empty for none. */
    fun setHolidayState(uf: String) =
        applyHolidays(Holidays.current().copy(uf = Holidays.STATES.firstOrNull { it.first == uf }?.first))

    /** A day off: [iso] is AAAA-MM-DD; with [yearly] only the day and the month count. */
    fun addHolidayDay(iso: String, name: String, yearly: Boolean) {
        val d = try { LocalDate.parse(iso) } catch (_: Throwable) { return }
        val day = CustomDay(if (yearly) null else d.year, d.monthValue, d.dayOfMonth, name.replace('\n', ' ').trim())
        val now = Holidays.current()
        if (day in now.custom) return
        applyHolidays(now.copy(custom = now.custom + day))
    }

    fun removeHolidayDay(day: CustomDay) {
        val now = Holidays.current()
        applyHolidays(now.copy(custom = now.custom - day))
    }

    /** An exported file was handed to a viewer: when the person is back, it is deleted. */
    fun noteExternalOpen() {
        openedExternally = true
    }

    /**
     * Deletes what was handed to other apps: every exported file, and the photos of forms that
     * are not in progress. Photos of the "Registro por foto" being filled stay (or the whole
     * folder while it is being saved) until the record is created or the form is closed.
     */
    private fun cleanHandedFiles(keep: Set<String> = setOfNotNull(photoPath, newPath) + Intake.keepPaths()) {
        try {
            File(app.cacheDir, PonteFiles.DIR_SHARED).listFiles()?.forEach { if (it.isFile) it.delete() }
            File(app.cacheDir, PonteFiles.DIR_PHOTOS).listFiles()?.forEach {
                if (it.isFile && LockPolicy.mayDeletePhoto(it.absolutePath, keep, photo.saving)) it.delete()
            }
        } catch (_: Throwable) {
        }
    }

    // ---------------------------------------------------------------- shell

    /** Copies the process-wide sync state into what the screens show. */
    fun render() {
        try {
            val busy = SyncRunner.running.get()
            takeDocs()
            if (wasRunning && !busy) runEnded()
            wasRunning = busy

            val text = SyncRunner.statusText
            val pending = SyncRunner.pendingFile(app).exists()
            val first = text.lineSequence().firstOrNull()?.trim() ?: ""
            val (line, kind) = when {
                busy -> (if (text.startsWith("Preparando")) "Preparando…" else "Sincronizando…") to StatusKind.NEUTRAL
                SyncRunner.statusError -> first to StatusKind.ERROR
                pending -> "Alterações aguardando envio" to StatusKind.WAITING
                text.startsWith("Sincronizado") || text.isEmpty() || text == "Pronto." ->
                    if (lastSync > 0) syncedLabel() to StatusKind.OK else "Ainda não sincronizado" to StatusKind.NEUTRAL
                else -> first to StatusKind.WAITING
            }
            shell = ShellState(
                tab = tab,
                badge = today.badge,
                syncing = busy,
                statusLine = line,
                statusKind = kind,
                syncEnabled = !SyncRunner.keyBroken,
                attendances = tasksSegment == TasksSegments.ATTENDANCES,
                resources = tasksSegment == TasksSegments.RESOURCES,
                projects = tasksSegment == TasksSegments.PROJECTS
            )
            if (stack.lastOrNull() == Route.Setup && setup.step == 2) {
                setup = setup.copy(
                    syncing = busy,
                    status = SetupRules.reason(busy, text),
                    statusError = SyncRunner.statusError,
                    done = !busy && text.startsWith("Sincronizado"),
                    noWifi = SetupRules.noWifi(busy, text)
                )
            }
            if (Route.Settings in stack) updateSettings()
            // What waits for the worker may ask for the fingerprint: only with the app in front
            // (onResume renders again).
            if (!busy && inFront()) {
                val t = idleTask
                if (t != null) {
                    idleTask = null
                    t()
                }
            }
        } catch (_: Throwable) {
        }
    }

    /** True while the Activity is on screen (a system prompt can be shown over it). */
    private fun activityInFront(): Boolean {
        val a = activity ?: return false
        return !a.isFinishing && !a.isDestroyed && a.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)
    }

    /** In front and not locked: the only time the sync may ask for the stored passphrase. */
    private fun inFront() = activityInFront() && !locked

    private fun runEnded() {
        val text = SyncRunner.statusText
        if (text.startsWith("Sincronizado")) {
            lastSync = System.currentTimeMillis()
            prefs.edit().putLong("last_sync", lastSync).apply()
        }
        // The holidays may have changed with the library (a sync, or the opening of the core).
        updateSettings()
        // And the file on the Drive may have grown: its size is read again for Ajustes.
        if (Route.Settings in stack) loadFileInfo()
        refreshTasks()
        reloadDocument()
        refreshOpenTask()
        refreshOpenAttendance()
        if (SyncRunner.statusError && stack.lastOrNull() != Route.Setup) {
            say(text.lineSequence().firstOrNull() ?: text, long = true)
        }
    }

    private fun syncedLabel(): String {
        val t = Instant.ofEpochMilli(lastSync).atZone(ZoneId.systemDefault())
        val pattern = if (t.toLocalDate() == LocalDate.now()) "HH:mm" else "dd/MM HH:mm"
        return "Sincronizado · " + t.format(DateTimeFormatter.ofPattern(pattern))
    }

    /** A short message at the bottom, optionally with one action ("Desfazer"). */
    fun say(message: String, action: String? = null, long: Boolean = false, onAction: (() -> Unit)? = null) {
        // Nothing is said over the lock screen: messages may carry document names.
        if (message.isBlank() || locked) return
        scope.launch {
            snackbar.currentSnackbarData?.dismiss()
            val r = snackbar.showSnackbar(
                message, action,
                duration = if (long || action != null) SnackbarDuration.Long else SnackbarDuration.Short
            )
            if (r == SnackbarResult.ActionPerformed) onAction?.invoke()
        }
    }

    /** Sets the sync status line; [notify] also says it at the bottom. */
    private fun status(msg: String, error: Boolean, notify: Boolean = true) {
        SyncRunner.setStatus(msg, error)
        render()
        if (notify) say(msg, long = true)
    }

    fun showStatus() {
        val pending = SyncRunner.pendingFile(app).exists()
        val text = SyncRunner.statusText.ifEmpty { "Ainda não houve sincronização neste aparelho." }
        dialog = AppDialog.Status(text, SyncRunner.statusError, pending)
    }

    // ---------------------------------------------------------------- navigation

    fun selectTab(i: Int) {
        tab = i.coerceIn(0, 3)
        shell = shell.copy(tab = tab)
        when (tab) {
            0, 1 -> refreshTasks()
            3 -> openMailTab()
        }
    }

    /** Tarefas | Projetos | Atendimentos | Recursos, at the top of the Tarefas area (see [TasksSegments]). */
    fun selectSegment(i: Int) {
        tasksSegment = i.coerceIn(0, TasksSegments.PROJECTS)
        shell = shell.copy(
            attendances = tasksSegment == TasksSegments.ATTENDANCES,
            resources = tasksSegment == TasksSegments.RESOURCES,
            projects = tasksSegment == TasksSegments.PROJECTS
        )
        refreshAttendances()
        refreshResources()
        refreshProjects()
    }

    fun openFollowUps() {
        stack.clear()
        tab = 1
        shell = shell.copy(tab = 1)
        attQuery = ""
        selectSegment(1)
    }

    /** From Hoje: the Recursos list, in the Tarefas area. */
    fun openResources() {
        stack.clear()
        tab = 1
        shell = shell.copy(tab = 1)
        resView = 0
        resQuery = ""
        selectSegment(2)
    }

    /** The notice was tapped with the app open: back to Hoje, unless something is half done. */
    fun showToday() {
        // A photo being made, a task or a note being typed stay as they are: the person is in the
        // middle of something. (While locked nothing is shown anyway; this only moves the state.)
        if (NavGuard.busy(stack.toList(), Intake.attachBusy)) return
        stack.clear()
        tab = 0
        shell = shell.copy(tab = 0)
        refreshTasks()
    }

    /** False when Back should leave the app. */
    fun canGoBack(): Boolean {
        val top = stack.lastOrNull() ?: return tab != 0
        return !(top == Route.Setup && setup.step == 0)
    }

    fun back() {
        when (val top = stack.lastOrNull()) {
            null -> if (tab != 0) selectTab(0)
            Route.Photo -> closePhoto()
            Route.Share -> Intake.back()
            Route.Setup -> setupBack()
            is Route.Account -> if (!accountForm.busy) pop()
            is Route.EditTask, is Route.Pick -> if (!taskEdit.busy) pop()
            // While the library is saving, leaving would hide the answer (and lose the form on a refusal).
            is Route.NewTask -> if (!newTaskSave.busy) pop()
            is Route.ProjectEdit -> if (!projectEdit.busy) pop()
            is Route.ProjectInclude -> if (!projectInclude.busy) pop()
            is Route.Note -> if (!noteSave.busy) pop()
            Route.Search -> closeSearch()
            else -> {
                pop()
                if (top is Route.Document) documentId = -1L
            }
        }
    }

    private fun pop() {
        if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex)
    }

    // ---------------------------------------------------------------- sync

    /** True unless the network is metered and mobile data is not allowed. */
    /** Set by "Usar dados móveis desta vez": lets the next sync start on mobile data, once. */
    private var mobileOnce = false

    private fun networkAllowed(): Boolean {
        if (prefs.getBoolean("mobile_ok", false)) return true
        if (mobileOnce) {
            mobileOnce = false
            return true
        }
        return try {
            val cm = app.getSystemService(ConnectivityManager::class.java)
            val net = cm.activeNetwork ?: return true
            val caps = cm.getNetworkCapabilities(net) ?: return true
            caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) ||
                caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
        } catch (_: Throwable) {
            true
        }
    }

    /** Passphrase from memory, or released by fingerprint/PIN when remembered. */
    private fun withPassphrase(auto: Boolean, onOk: (String) -> Unit, onFail: () -> Unit) {
        if (SyncRunner.passphrase.isNotEmpty()) {
            onOk(SyncRunner.passphrase)
            return
        }
        if (!Secrets.isRemembered(app)) {
            syncRefused(SyncRefusal.PASSPHRASE_MISSING, auto)
            onFail()
            return
        }
        val a = activity
        if (a == null || !inFront()) {
            // The fingerprint prompt needs the app in front: the sync is tried again on return.
            if (idleTask == null) idleTask = { requestSync(null, auto = true) }
            syncRefused(SyncRefusal.NOT_FOREGROUND, auto)
            onFail()
            return
        }
        Secrets.unlock(a) { pass, _ ->
            promptDone(pass != null)
            if (pass != null) {
                SyncRunner.passphrase = pass
                onOk(pass)
            } else {
                syncRefused(SyncRefusal.PASSPHRASE_CANCELLED, auto)
                onFail()
            }
        }
    }

    private fun syncRefused(reason: SyncRefusal, auto: Boolean) {
        val feedback = SyncFeedback.decide(reason, auto)
        if (reason != SyncRefusal.BUSY) status(feedback.status, reason == SyncRefusal.NO_FILE, notify = false)
        feedback.message?.let { message ->
            if (feedback.mobileAction) {
                say(message, "Usar dados móveis desta vez", long = true) {
                    mobileOnce = true
                    requestSync(null)
                }
            } else say(message, long = true)
        }
    }

    /** Saves the note locally without a sync (used when the sync cannot start now). */
    private fun saveNoteOnly(note: Pair<Long, String>) {
        SyncRunner.background({
            val o = JSONObject(SyncRunner.core.saveNote(note.first, note.second))
            if (!o.optBoolean("ok")) {
                throw CoreException(PhoneWords.adapt(o.s("error", "Não foi possível guardar a nota.")))
            }
            SyncRunner.refreshDocs()
        }, {
            takeDocs()
            reloadDocument()
        }, { status(it, true) }, fail = "Não foi possível guardar a nota neste aparelho. Tente salvar de novo.")
    }

    /**
     * Starts a sync (optionally saving a note first). The note is never lost when the sync
     * cannot start: it is saved locally and goes in the next one. [auto] is a sync nobody
     * asked for by hand (opening the app, after a change): its excuses stay in the status
     * line and are not said at the bottom.
     */
    fun requestSync(note: Pair<Long, String>?, auto: Boolean = false, then: (() -> Unit)? = null) {
        try {
            if (SyncRunner.running.get()) {
                mobileOnce = false
                note?.let { saveNoteOnly(it) }
                syncRefused(SyncRefusal.BUSY, auto)
                then?.invoke()
                return
            }
            val uri = fileUri
            if (uri == null) {
                note?.let { saveNoteOnly(it) }
                syncRefused(SyncRefusal.NO_FILE, auto)
                then?.invoke()
                return
            }
            if (!networkAllowed()) {
                note?.let { saveNoteOnly(it) }
                syncRefused(SyncRefusal.NO_WIFI, auto)
                then?.invoke()
                return
            }
            withPassphrase(auto, { pass ->
                try {
                    if (SyncRunner.startSync(app, uri, pass, note)) {
                        idleTask = then
                    } else {
                        note?.let { saveNoteOnly(it) }
                        syncRefused(SyncRefusal.BUSY, auto)
                        then?.invoke()
                    }
                    render()
                } catch (e: Throwable) {
                    note?.let { saveNoteOnly(it) }
                    status(PhoneWords.explain(e, START_FAILED), true, notify = !auto)
                }
            }, {
                note?.let { saveNoteOnly(it) }
                then?.invoke()
            })
        } catch (e: Throwable) {
            status(PhoneWords.explain(e, START_FAILED), true, notify = !auto)
        }
    }

    // ---------------------------------------------------------------- tasks

    /**
     * Tasks completed or reopened here whose change still waits for the worker (behind a
     * sync, for instance): task id -> (order of the tap, done). Lists read meanwhile show
     * what the person chose, not what the library still holds.
     */
    private val pendingDone = HashMap<Long, Pair<Long, Boolean>>()
    private var doneSeq = 0L

    private fun TaskItem.withPending(): TaskItem {
        val d = pendingDone[id]?.second ?: return this
        return if (d == done) this
        else copy(done = d, overdue = !d && lateIfOpen, dueToday = !d && todayIfOpen)
    }

    /** Reads the list of categories; a failure keeps the one already shown. */
    private fun refreshCategories() {
        SyncRunner.background({ Api.categories() }, { list ->
            categories = list
            taskCatFilter = CategoryRules.pruneFilter(taskCatFilter, list)
            docCatFilter = CategoryRules.pruneFilter(docCatFilter, list)
        })
    }

    /** Makes a category on the spot (from a picker) and hands it, or the reason it failed, to [done]. */
    fun createCategory(name: String, color: Int, done: (Category?, String) -> Unit) {
        SyncRunner.background({
            val made = Api.createCategory(name.trim(), color)
            made to Api.categories()
        }, { (made, list) ->
            categories = list
            done(made, "")
            requestSync(null, auto = true)
        }, { done(null, it) }, fail = "Não foi possível criar a categoria. Tente de novo.")
    }

    /** Saves the categories of the open document; a ficha that changed meanwhile is refused and reloaded. */
    fun saveDocumentCategories(uids: List<String>) {
        val d = document.doc ?: return
        SyncRunner.background({
            val version = Api.setDocumentCategories(d.id, uids, d.categoriesVersion)
            SyncRunner.refreshDocs()
            version
        }, {
            takeDocs()
            reloadDocument()
            say("Categorias salvas.")
            requestSync(null, auto = true)
        }, {
            reloadDocument()
            say(it, long = true)
        }, fail = "Não foi possível salvar as categorias. Tente de novo.")
    }

    fun refreshTasks() {
        refreshCategories()
        SyncRunner.background({ Api.tasks().map { it.toTask() } }, { read ->
            val list = read.map { it.withPending() }
            tasks = TasksState(loading = false, error = null, tasks = list)
            // The tasks changed here or in a sync: the calendar follows.
            calendarSyncSoon()
            // The list changed (a save, a conclusion, a sync): arm the task notices again, so a
            // notice set a moment ago is not left waiting for the next time the app opens.
            if (::app.isInitialized) PlannedNotices.refreshAsync(app)
        }, { tasks = TasksState(loading = false, error = it, tasks = tasks.tasks) },
            fail = "Não foi possível ler as tarefas. Toque em Sincronizar agora e tente de novo.")
        refreshToday()
        refreshAttendances()
        refreshResources()
        refreshProjects()
        refreshAssignees()
    }

    /** The suggestions of the Responsável field. A failure only means no suggestions. */
    private fun refreshAssignees() {
        SyncRunner.background({ Api.assignees() }, { assignees = it })
    }

    // ---------------------------------------------------------------- projetos

    /**
     * Reads the Projetos list (the numbers are the core's, as of today). A failure keeps the list already
     * shown. It also reads the open project screen again, so any change of a task shows up there.
     */
    fun refreshProjects() {
        SyncRunner.background({ Api.projects() }, { list ->
            projects = ProjectsState(loading = false, error = null, rows = list)
            taskProjectFilter = ProjectRules.pruneFilter(taskProjectFilter, list)
        }, { projects = projects.copy(loading = false, error = it) },
            fail = "Não foi possível ler os projetos. Toque em Sincronizar agora e tente de novo.")
        refreshOpenProject()
    }

    /** Grows whenever the project screen is opened (or restored): an answer asked for an earlier one is dropped. */
    private var projGen = 0

    private fun projectOnScreen(uid: String, gen: Int) =
        gen == projGen && projectView.uid == uid && stack.any { it is Route.Project && it.uid == uid }

    /** Opens a project. There is one project screen at most on the stack (opening another replaces it). */
    fun openProject(uid: String) {
        if (uid.isEmpty()) return
        val r = Route.Project(uid)
        if (stack.lastOrNull() == r) return
        stack.removeAll { it is Route.Project }
        projGen++
        projectView = ProjectViewState(uid = uid)
        stack.add(r)
        loadProject(uid)
    }

    private fun loadProject(uid: String) {
        val gen = projGen
        SyncRunner.background({ Api.project(uid) }, { d ->
            if (projectOnScreen(uid, gen)) projectView = ProjectViewState(uid = uid, loading = false, detail = d)
        }, {
            if (projectOnScreen(uid, gen)) {
                projectView = ProjectViewState(uid = uid, loading = false, error = it, detail = projectView.detail)
            }
        }, fail = "Não foi possível abrir o projeto. Volte e tente de novo.")
    }

    /** A change of a task, a sync or a save: the project screen under whatever is on top is read again. */
    private fun refreshOpenProject() {
        val uid = (stack.lastOrNull { it is Route.Project } as? Route.Project)?.uid ?: return
        if (projectView.uid == uid && !projectView.busy) loadProject(uid)
    }

    /** The form of a project; an empty [uid] is a new one. */
    fun openProjectForm(uid: String) {
        if (!projectEdit.busy) projectEdit = ProjectEditState(epoch = projectEdit.epoch)
        push(Route.ProjectEdit(uid))
    }

    /** "Ver a versão atual": the form starts over from what the library holds now. */
    fun reloadProjectForm() {
        if (projectEdit.busy) return
        projectEdit = ProjectEditState(epoch = projectEdit.epoch + 1)
        refreshProjects()
    }

    /** Saves the form: creates the project (and opens it) or changes it, then the form closes. */
    fun saveProject(uid: String, version: Long, form: ProjectForm) {
        val route = stack.lastOrNull() as? Route.ProjectEdit ?: return
        if (projectEdit.busy || ProjectRules.formProblem(form).isNotEmpty()) return
        projectEdit = projectEdit.copy(busy = true, error = "", conflict = false)
        SyncRunner.background({ Api.saveProject(ProjectRules.saveJson(uid, version, form)) }, { saved ->
            projectEdit = ProjectEditState(epoch = projectEdit.epoch)
            stack.remove(route)
            refreshProjects()
            if (uid.isEmpty()) openProject(saved.uid)
            say(if (uid.isEmpty()) "Projeto criado." else "Projeto salvo.")
            requestSync(null, auto = true)
        }, { message ->
            if (stack.contains(route)) {
                projectEdit = projectEdit.copy(busy = false, error = message, conflict = ProjectRules.isConflict(message))
            } else status(message, true)
        }, fail = "Não foi possível salvar o projeto. Seus dados continuam na tela; tente de novo.")
    }

    /** "Arquivar" or "Reabrir": only the situation changes. */
    fun setProjectStatus(p: ProjectItem, status: String) {
        if (projectView.busy) return
        projectView = projectView.copy(busy = true)
        SyncRunner.background({ Api.saveProject(ProjectRules.statusJson(p.uid, p.version, status)) }, {
            projectView = projectView.copy(busy = false)
            refreshProjects()
            say(if (status == ProjectRules.ARCHIVED) "Projeto arquivado." else "Projeto reaberto.")
            requestSync(null, auto = true)
        }, {
            projectView = projectView.copy(busy = false)
            refreshProjects()
            say(it, long = true)
        }, fail = "Não foi possível mudar a situação do projeto. Tente de novo.")
    }

    /** Deletes the project; its tasks stay, with no project. */
    fun deleteProject(uid: String) {
        if (projectView.busy) return
        projectView = projectView.copy(busy = true)
        SyncRunner.background({ Api.deleteProject(uid) }, { released ->
            stack.removeAll {
                (it is Route.Project && it.uid == uid) || (it is Route.ProjectEdit && it.uid == uid) ||
                    (it is Route.ProjectInclude && it.uid == uid)
            }
            projectView = ProjectViewState()
            refreshTasks()
            say(ProjectRules.deletedMessage(released), long = true)
            requestSync(null, auto = true)
        }, {
            projectView = projectView.copy(busy = false)
            refreshProjects()
            say(it, long = true)
        }, fail = "Não foi possível excluir o projeto. Tente de novo.")
    }

    /**
     * Puts the tasks [ids] in the project [uid] (empty: takes them out of any), one save each at the version
     * just read. Answers how many were done and the reason of the first that was not.
     */
    private fun moveToProject(ids: List<Long>, uid: String): Pair<Int, String?> {
        var done = 0
        var why: String? = null
        for (id in ids) {
            try {
                val d = Api.task(id)
                Api.saveTask(id, d.version, mapOf("project_uid" to uid))
                done++
            } catch (e: CoreException) {
                if (why == null) why = e.message
            } catch (e: Throwable) {
                if (why == null) why = "Não foi possível mudar o projeto da tarefa."
            }
        }
        return done to why
    }

    /** The list of the open tasks with no project, to include some of them in [uid]. */
    fun openProjectInclude(uid: String) {
        if (!projectInclude.busy) projectInclude = SaveState()
        push(Route.ProjectInclude(uid))
    }

    fun includeTasks(uid: String, ids: List<Long>) {
        val route = stack.lastOrNull() as? Route.ProjectInclude ?: return
        if (ids.isEmpty()) return
        projectInclude = SaveGate.begin(projectInclude) ?: return
        SyncRunner.background({ moveToProject(ids, uid) }, { (done, why) ->
            // Whatever was done is in the library; the lists show it either way.
            refreshTasks()
            if (done == 0 && why != null) {
                projectInclude = SaveGate.failed(why)
            } else {
                projectInclude = SaveGate.done()
                SaveGate.close(stack, route)
                say(
                    if (why == null) ProjectRules.includedMessage(done)
                    else ProjectRules.includedMessage(done) + " As outras não puderam ser incluídas: $why",
                    long = why != null
                )
                requestSync(null, auto = true)
            }
        }, {
            projectInclude = SaveGate.failed(it)
        }, fail = "Não foi possível incluir as tarefas. Tente de novo.")
    }

    /** "Tirar do projeto": the task goes back to no project; "Desfazer" puts it back. */
    fun removeFromProject(t: TaskItem) {
        val was = t.projectUid
        SyncRunner.background({ moveToProject(listOf(t.id), "") }, { (done, why) ->
            refreshTasks()
            if (done > 0) {
                say("Tarefa tirada do projeto.", "Desfazer") { putBack(t.id, was) }
                requestSync(null, auto = true)
            } else say(why ?: "Não foi possível tirar a tarefa do projeto.", long = true)
        }, { say(it, long = true) }, fail = "Não foi possível tirar a tarefa do projeto. Tente de novo.")
    }

    private fun putBack(id: Long, uid: String) {
        SyncRunner.background({ moveToProject(listOf(id), uid) }, { (done, why) ->
            refreshTasks()
            if (done > 0) requestSync(null, auto = true) else say(why ?: "Não foi possível desfazer.", long = true)
        }, { say(it, long = true) }, fail = "Não foi possível desfazer. Tente de novo.")
    }

    /** From a project: a new task that starts in it. */
    fun openNewTaskInProject(uid: String) {
        if (actions.isEmpty()) loadChoices()
        if (!newTaskSave.busy) newTaskSave = SaveState()
        push(Route.NewTask(0, uid))
    }

    // ---------------------------------------------------------------- atendimentos (read-only)

    /** Grows whenever the atendimento screen is opened: an answer asked for an earlier one is dropped. */
    private var attGen = 0

    /** Reads the Atendimentos list (it is also what the search of this list filters). */
    private fun refreshAttendances() {
        if (::app.isInitialized) TodayWidget.refresh(app)
        SyncRunner.background({ Api.attendances() to Api.followUps() }, { (rows, chase) ->
            attendances = AttendancesState(
                loading = false, error = null, rows = rows, followUps = chase.rows, channels = chase.channels
            )
        }, { attendances = attendances.copy(loading = false, error = it) },
            fail = "Não foi possível ler os atendimentos. Toque em Sincronizar agora e tente de novo.")
    }

    /**
     * "Registrar cobrança": a contact in the timeline of the atendimento (the way and the note in
     * [contact]) and, when [nextDate] (AAAA-MM-DD) is not empty, the next cobrança. The deadline is
     * not touched. Then the lists are read again and the change goes out with the next sync.
     */
    fun registerFollowUp(id: Long, contact: String, nextDate: String) = changeFollowUp(
        { Api.registerFollowUp(id, contact, nextDate) }, FollowUpRules.registeredMessage(nextDate)
    )

    /** "Próxima cobrança em…": only the date ([nextDate] AAAA-MM-DD). */
    fun scheduleFollowUp(id: Long, nextDate: String) = changeFollowUp(
        { Api.scheduleFollowUp(id, nextDate) }, FollowUpRules.scheduledMessage(nextDate)
    )

    private fun changeFollowUp(write: () -> Unit, done: String) {
        SyncRunner.background({ write() }, {
            refreshAttendances()
            refreshOpenAttendance()
            say(done)
            requestSync(null, auto = true)
        }, { say(it, long = true) },
            fail = "Não foi possível registrar a cobrança. Tente de novo.")
    }

    /**
     * Opens the ficha of an atendimento. There is one at most on the stack (opening another replaces
     * it), so Back never shows a ficha that belongs to another atendimento.
     */
    fun openAttendance(id: Long) {
        if (id <= 0) return
        val r = Route.Attendance(id)
        if (stack.lastOrNull() == r) return
        stack.removeAll { it is Route.Attendance }
        attGen++
        attendance = AttendanceViewState(id = id)
        stack.add(r)
        loadAttendance(id)
    }

    private fun attendanceOnScreen(id: Long, gen: Int) =
        gen == attGen && attendance.id == id && stack.any { it is Route.Attendance && it.id == id }

    private fun loadAttendance(id: Long) {
        val gen = attGen
        SyncRunner.background({ Api.attendance(id) }, { d ->
            if (attendanceOnScreen(id, gen)) attendance = AttendanceViewState(id = id, loading = false, detail = d)
        }, {
            if (attendanceOnScreen(id, gen)) {
                attendance = AttendanceViewState(id = id, loading = false, error = it, detail = attendance.detail)
            }
        }, fail = "Não foi possível abrir o atendimento. Volte e tente de novo.")
    }

    /** A sync ended while the ficha is open: it may have brought a newer version. */
    private fun refreshOpenAttendance() {
        val id = (stack.lastOrNull() as? Route.Attendance)?.id ?: return
        if (attendance.id == id) loadAttendance(id)
    }

    // ---------------------------------------------------------------- recursos (read-only)

    /**
     * The search over the documents for a text typed in a box: the core searches 300 ms after the last
     * keystroke, off the main thread, and only the newest answer is kept. Nothing typed: no call, all shown.
     */
    private class DocFinder(private val apply: (Set<Long>?) -> Unit) {
        private var gen = 0
        private var text = ""
        private val runner = Runnable { run() }

        fun ask(q: String) {
            main.removeCallbacks(runner)
            gen++
            text = q.trim()
            if (text.isEmpty()) apply(null) else main.postDelayed(runner, SearchRules.DEBOUNCE_MS)
        }

        private fun run() {
            if (locked) return
            val asked = ++gen
            val t = text
            SyncRunner.background({ Api.documentIds(t) }, { ids -> if (asked == gen && !locked) apply(ids) }, { },
                fail = "Não foi possível buscar. Tente de novo.")
        }
    }

    private val docFinder = DocFinder { docMatches = it }
    private val pickFinder = DocFinder { pickMatches = it }

    /** Takes the documents the sync runner holds; what the Documentos search found is asked again for them. */
    private fun takeDocs() {
        val before = docs
        docs = SyncRunner.docs
        taskDocumentFilter = TaskDocumentFilter.afterRefresh(taskDocumentFilter, before, docs)
        // The document filter kept from the last run is checked against the library now, once it has been read.
        val pending = pendingDocumentFilter
        if (pending != null && SyncRunner.ready) {
            pendingDocumentFilter = null
            if (taskDocumentFilter == null) taskDocumentFilter = FilterMemory.restoreDocument(pending, docs)
            // Nothing to put back (the document is gone): what is kept forgets it too.
            if (taskDocumentFilter == null) { keptDocument = null; persistFilters() }
        }
        docFinder.ask(docQuery)
    }

    fun typeDocQuery(q: String) {
        docQuery = q
        docFinder.ask(q)
    }

    fun setPickQuery(q: String) = pickFinder.ask(q)

    private val resRunner = Runnable { refreshResources() }

    /** Typing in the search of Recursos: the core searches 300 ms after the last keystroke (at once when emptied). */
    fun typeResQuery(q: String) {
        resQuery = q.take(200)
        main.removeCallbacks(resRunner)
        if (resQuery.isBlank()) refreshResources() else main.postDelayed(resRunner, SearchRules.DEBOUNCE_MS)
    }

    /**
     * Reads the Recursos area as the core decides it now, for the text searched: groups, who holds what,
     * the period wording. Called when the screen opens, after a sync and every minute while it is visible.
     */
    fun refreshResources() {
        val asked = resQuery.trim()
        SyncRunner.background({ Api.resourceView(asked) }, { read ->
            // A newer text was typed while this was read: that one will answer.
            if (asked == resQuery.trim()) resources = read
        }, { resources = resources.copy(loading = false, error = it) },
            fail = "Não foi possível ler os recursos. Toque em Sincronizar agora e tente de novo.")
    }

    /** Opens one use. Opening the one already on the stack brings it back to the top (keys stay unique). */
    fun openResourceUse(uid: String) {
        if (uid.isEmpty()) return
        val r = Route.ResourceUse(uid)
        if (stack.lastOrNull() == r) return
        stack.remove(r)
        stack.add(r)
    }

    fun openResource(uid: String) {
        if (uid.isEmpty()) return
        val r = Route.Resource(uid)
        if (stack.lastOrNull() == r) return
        stack.remove(r)
        stack.add(r)
    }

    // ---------------------------------------------------------------- search

    private var searchGen = 0
    private val searchRunner = Runnable { runSearch() }

    fun openSearch() {
        if (Route.Search in stack) return
        search = SearchState()
        stack.add(Route.Search)
    }

    /** Typing in the search box: a search starts 300 ms after the last keystroke, at two characters or more. */
    fun setSearchQuery(q: String) {
        val text = q.take(200)
        main.removeCallbacks(searchRunner)
        searchGen++
        if (!SearchRules.usable(text)) {
            search = SearchState(query = text)
            return
        }
        search = search.copy(query = text, loading = true, error = null)
        main.postDelayed(searchRunner, SearchRules.DEBOUNCE_MS)
    }

    private fun runSearch() {
        if (locked || Route.Search !in stack) return
        val text = search.query.trim()
        if (!SearchRules.usable(text)) return
        val gen = ++searchGen
        SyncRunner.background({ Api.search(text) }, { found ->
            if (SearchRules.accept(gen, searchGen, locked, Route.Search in stack)) {
                search = search.copy(loading = false, error = null, result = found)
            }
        }, { message ->
            if (SearchRules.accept(gen, searchGen, locked, Route.Search in stack)) {
                search = search.copy(loading = false, error = message, result = null)
            }
        }, fail = "Não foi possível buscar. Tente de novo.")
    }

    /** Forgets the text, the answer and any search on its way. */
    private fun clearSearch() {
        main.removeCallbacks(searchRunner)
        searchGen++
        search = SearchState()
    }

    private fun closeSearch() {
        clearSearch()
        stack.remove(Route.Search)
    }

    /** Reads what Hoje lists; what the person just concluded stays out even before it is saved. */
    var week by mutableStateOf<List<WeekDay>>(emptyList())
        private set

    /** The Monday of the week Hoje shows; moves with [moveWeek]. */
    var weekStart by mutableStateOf(WeekPlan.mondayOf(java.time.LocalDate.now()))
        private set

    /** Earlier (-1) or later (+1) week; 0 comes back to this week. */
    fun moveWeek(delta: Int) {
        weekStart = if (delta == 0) WeekPlan.mondayOf(java.time.LocalDate.now()) else weekStart.plusWeeks(delta.toLong())
        refreshWeek()
    }

    /** A tap on a line of the week: the task (block or deadline), or the resource use. */
    fun openWeekEntry(e: WeekEntry) {
        when {
            e.taskId != null -> openTask(e.taskId)
            e.kind == WeekKind.USE -> openResourceUse(e.id.removePrefix("u-"))
        }
    }

    private fun refreshWeek() {
        val start = weekStart
        SyncRunner.background({
            val all = Api.taskRelations().map { it.toTask() }
            val entries = all.filter { !it.done }.flatMap { WeekPlan.taskEntries(it, all) } +
                Api.resourceView("").uses.mapNotNull { WeekPlan.use(it) }
            WeekPlan.days(start, entries)
        }, { if (start == weekStart) week = it }, { if (start == weekStart) week = emptyList() },
            fail = "Não foi possível carregar a semana.")
    }

    /** Deliveries at risk shown on Hoje (see [RiskRules]). */
    var risks by mutableStateOf<List<RiskItem>>(emptyList())
        private set

    private fun refreshRisks() {
        SyncRunner.background({ Api.atRisk() }, { risks = it }, { risks = emptyList() },
            fail = "Não foi possível verificar as entregas em risco.")
    }

    /** Records of this phone renumbered by a sync and not read yet (the card of Hoje); independent of the status line. */
    var renumberNotices by mutableStateOf<List<RenumberNotice>>(emptyList())
        private set

    private fun refreshRenumberNotices() {
        SyncRunner.background({ Api.renumberNotices() }, { renumberNotices = it }, { },
            fail = "Não foi possível ler os avisos de número alterado.")
    }

    /** "Entendi": marks the notice as read and reads the list again. */
    fun dismissRenumberNotice(id: Long) {
        // Gone at once; the list read afterwards is the truth.
        renumberNotices = renumberNotices.filter { it.id != id }
        SyncRunner.background({ Api.dismissRenumberNotice(id) }, { refreshRenumberNotices() }, { refreshRenumberNotices() },
            fail = "Não foi possível dispensar o aviso. Tente de novo.")
    }

    private fun refreshToday() {
        refreshWeek()
        refreshRisks()
        refreshRenumberNotices()
        SyncRunner.background({
            val o = Api.today()
            val tasksOf = { key: String -> o.optJSONArray(key).objs().map { it.toTask() } }
            TodayState(
                loading = false, late = tasksOf("late"), today = tasksOf("today"), upcoming = tasksOf("upcoming"),
                attendances = o.optJSONArray("attendances").objs().map { it.toAttendance() },
                reservationsToday = o.optJSONObject("resources")?.optInt("reservations_today") ?: 0,
                overdueReturns = o.optJSONObject("resources")?.optInt("overdue") ?: 0,
                summary = o.optString("summary"), badge = o.optInt("badge")
            )
        }, { read ->
            val open = { list: List<TaskItem> -> list.map { it.withPending() }.filter { !it.done } }
            today = read.copy(late = open(read.late), today = open(read.today), upcoming = open(read.upcoming))
            shell = shell.copy(badge = read.badge)
        }, { today = today.copy(loading = false, error = it) },
            fail = "Não foi possível ler o que vence hoje. Toque em Sincronizar agora e tente de novo.")
    }

    /** Completes or reopens a task, then syncs. */
    fun toggleTask(t: TaskItem) = setTaskDone(t, !t.done, offerUndo = !t.done)

    private fun setTaskDone(t: TaskItem, done: Boolean, offerUndo: Boolean) {
        // Shown at once; the reload below brings what the library really holds.
        val seq = ++doneSeq
        pendingDone[t.id] = seq to done
        val settled: () -> Unit = { if (pendingDone[t.id]?.first == seq) pendingDone.remove(t.id) }
        val flip = { x: TaskItem ->
            if (x.id != t.id) x
            else x.copy(done = done, overdue = !done && x.lateIfOpen, dueToday = !done && x.todayIfOpen)
        }
        tasks = tasks.copy(tasks = tasks.tasks.map(flip))
        if (done) {
            val out = { list: List<TaskItem> -> list.filter { it.id != t.id } }
            today = today.copy(late = out(today.late), today = out(today.today), upcoming = out(today.upcoming))
        }
        document =document.copy(doc = document.doc?.let { it.copy(tasks = it.tasks.map(flip)) })
        SyncRunner.background({
            Api.setDone(t.id, done)
            SyncRunner.refreshDocs()
        }, {
            settled()
            takeDocs()
            refreshTasks()
            reloadDocument()
            if (offerUndo) {
                say("Tarefa concluída.", "Desfazer") { setTaskDone(t.copy(done = true), false, offerUndo = false) }
            }
            requestSync(null, auto = true)
        }, {
            settled()
            refreshTasks()
            reloadDocument()
            status(it, true)
        }, fail = if (done) "Não foi possível concluir a tarefa. Tente de novo." else "Não foi possível reabrir a tarefa. Tente de novo.")
    }

    /** Opens a screen over the current one; a second tap on the same button does nothing. */
    private fun push(r: Route) {
        if (stack.lastOrNull() != r) stack.add(r)
    }

    /** From a ficha: the new task belongs to that document. */
    fun openNewTask() {
        if (documentId < 0) return
        if (actions.isEmpty()) loadChoices()
        if (!newTaskSave.busy) newTaskSave = SaveState()
        push(Route.NewTask(documentId))
    }

    /** From Tarefas: a task without document (it can be linked to one later). */
    fun openNewTaskWithoutDocument() {
        if (actions.isEmpty()) loadChoices()
        if (!newTaskSave.busy) newTaskSave = SaveState()
        push(Route.NewTask(0))
    }

    // ---- the task screen

    /**
     * Grows whenever the task screen is opened for a task (or restored): an answer that was asked
     * for an earlier screen is dropped. Together with the task id it says "still the same screen".
     */
    private var taskGen = 0

    private fun taskOnScreen(id: Long, gen: Int) =
        gen == taskGen && taskEdit.id == id && stack.any { it is Route.EditTask && it.id == id }

    /**
     * Opens a task to edit. There is at most one task screen on the stack: opening another task
     * (or the same one again, say from a ficha it was already above) replaces it, so Back from the
     * new one never shows an editor that belongs to another task.
     */
    fun openTask(id: Long) {
        if (id <= 0) return
        val r = Route.EditTask(id)
        if (stack.lastOrNull() == r) return
        if (actions.isEmpty()) loadChoices()
        val next = TaskRules.withTask(stack.toList(), id)
        taskGen++
        taskEdit = TaskEditState(id = id)
        // The hint about a refused permission is about the last try: a new task starts without it.
        taskNoticeBlocked = false
        stack.clear()
        stack.addAll(next)
        loadTask(id)
        loadTaskRelations(id)
    }

    private fun loadTaskRelations(id: Long) {
        val gen = taskGen
        taskRelations = TasksState(loading = true)
        SyncRunner.background({ Api.taskRelations().map { it.toTask() } }, { list ->
            if (taskOnScreen(id, gen)) taskRelations = TasksState(loading = false, tasks = list)
        }, { error ->
            if (taskOnScreen(id, gen)) taskRelations = TasksState(loading = false, error = error)
        }, fail = "Não foi possível carregar as tarefas.")
    }

    /**
     * Reads the task again. [discard]: the person asked for the current version, so the form starts
     * over. [clearBusy]: this read ends a change of document that kept the screen waiting.
     */
    private fun loadTask(id: Long, discard: Boolean = false, clearBusy: Boolean = false) {
        val gen = taskGen
        SyncRunner.background({ Api.task(id) }, { d ->
            if (taskOnScreen(id, gen)) {
                taskEdit = taskEdit.copy(
                    loading = false, error = null, task = d,
                    busy = taskEdit.busy && !clearBusy,
                    formError = if (discard) "" else taskEdit.formError,
                    conflict = !discard && taskEdit.conflict,
                    epoch = if (discard) taskEdit.epoch + 1 else taskEdit.epoch
                )
            }
        }, {
            if (taskOnScreen(id, gen)) {
                taskEdit = if (taskEdit.task?.id == id) taskEdit.copy(busy = taskEdit.busy && !clearBusy, formError = it, conflict = false)
                else TaskEditState(id = id, loading = false, error = it)
            }
        }, fail = "Não foi possível abrir a tarefa. Volte e tente de novo.")
    }

    /** "Ver a versão atual": drops what was typed and shows the stored task. */
    fun reloadTask() {
        val id = (stack.lastOrNull() as? Route.EditTask)?.id ?: return
        if (taskEdit.busy) return
        loadTask(id, discard = true)
    }

    /** A sync ended while the task screen is open: it may have brought a newer version. */
    private fun refreshOpenTask() {
        val id = (stack.lastOrNull() as? Route.EditTask)?.id ?: return
        if (!taskEdit.busy && taskEdit.id == id) { loadTask(id); loadTaskRelations(id) }
    }

    /** Applies the answer of a failed change: the conflict gets its own button. */
    private fun taskFailed(message: String) {
        taskEdit = taskEdit.copy(busy = false, formError = message, conflict = TaskRules.isConflict(message))
    }

    /** Saves what changed in the form; the screen closes when it worked. */
    fun saveTask(s: TaskSave) = saveTask(s, thenFinish = false)

    private fun saveTask(s: TaskSave, thenFinish: Boolean) {
        val t = taskEdit.task ?: return
        if (t.id != s.taskId || taskEdit.busy || s.form.title.isBlank()) return
        val changes = s.changes
        val gen = taskGen
        if (changes.isEmpty()) {
            taskEdit = taskEdit.copy(busy = true)
            stack.remove(Route.EditTask(t.id))
            if (thenFinish) finishNow(t)
            return
        }
        taskEdit = taskEdit.copy(busy = true, formError = "", conflict = false)
        SyncRunner.background({
            val version = Api.saveTask(t.id, s.version, changes)
            SyncRunner.refreshDocs()
            version
        }, { version ->
            takeDocs()
            if (taskOnScreen(t.id, gen)) {
                taskEdit = taskEdit.copy(task = t.copy(form = s.form, version = version), busy = true)
                stack.remove(Route.EditTask(t.id))
            }
            refreshTasks()
            reloadDocument()
            if (thenFinish) finishNow(t) else {
                say("Tarefa salva.")
                requestSync(null, auto = true)
            }
        }, { if (taskOnScreen(t.id, gen)) taskFailed(it) },
            fail = "Não foi possível salvar a tarefa. Tente de novo.")
    }

    /** Concludes the task (or reopens it, when it was concluded), saving typed changes first. */
    fun finishTask(s: TaskSave) {
        val t = taskEdit.task ?: return
        if (t.id != s.taskId || taskEdit.busy || s.form.title.isBlank()) return
        if (s.changes.isNotEmpty()) saveTask(s, thenFinish = true) else {
            taskEdit = taskEdit.copy(busy = true)
            stack.remove(Route.EditTask(t.id))
            finishNow(t)
        }
    }

    private fun finishNow(t: TaskDetail) {
        setTaskDone(t.asItem(), !t.done, offerUndo = !t.done)
        if (t.done) say("Tarefa reaberta.")
    }

    /** "Excluir tarefa…": the task goes to the Lixeira and the list comes back, with a Desfazer. */
    fun deleteTask() {
        val t = taskEdit.task ?: return
        if (taskEdit.busy) return
        val gen = taskGen
        taskEdit = taskEdit.copy(busy = true, formError = "", conflict = false)
        SyncRunner.background({
            Api.deleteTask(t.id, t.version)
            SyncRunner.refreshDocs()
        }, {
            takeDocs()
            if (taskOnScreen(t.id, gen)) stack.remove(Route.EditTask(t.id))
            refreshTasks()
            reloadDocument()
            say(PhoneWords.TASK_DELETED, "Desfazer") { restoreTask(t.id) }
            requestSync(null, auto = true)
        }, { if (taskOnScreen(t.id, gen)) taskFailed(it) },
            fail = "Não foi possível excluir a tarefa. Tente de novo.")
    }

    /** The Desfazer of [deleteTask]: the task comes back from the Lixeira. */
    private fun restoreTask(id: Long) {
        SyncRunner.background({
            Api.restoreTask(id)
            SyncRunner.refreshDocs()
        }, {
            takeDocs()
            refreshTasks()
            reloadDocument()
            say(PhoneWords.TASK_RESTORED)
            requestSync(null, auto = true)
        }, { status(it, true) }, fail = "Não foi possível desfazer a exclusão. Restaure a tarefa pela Lixeira, no Ponte do computador.")
    }

    fun openTaskDocument() {
        val t = taskEdit.task ?: return
        if (t.documentId > 0) openDocument(t.documentId)
    }

    /** Opens the list of documents to link the task to (or move it to). */
    fun openPicker() {
        val t = taskEdit.task ?: return
        if (taskEdit.busy) return
        push(Route.Pick(t.id))
    }

    fun pickDocument(d: Doc) = relink(d.id)

    fun unlinkTask() = relink(0)

    /** Links the task to [documentId], or (0) takes it out of its document. */
    private fun relink(documentId: Long) {
        val t = taskEdit.task ?: return
        if (taskEdit.busy) return
        val gen = taskGen
        taskEdit = taskEdit.copy(busy = true, formError = "", conflict = false)
        SyncRunner.background({
            val version = Api.relinkTask(t.id, documentId, t.version)
            SyncRunner.refreshDocs()
            version
        }, { version ->
            takeDocs()
            if (taskOnScreen(t.id, gen)) {
                stack.removeAll { it is Route.Pick }
                // Stays busy until the task is read again: a save before that would send the old version.
                taskEdit = taskEdit.copy(task = t.copy(version = version), busy = true)
                loadTask(t.id, clearBusy = true)
            }
            refreshTasks()
            reloadDocument()
            say(if (documentId > 0) "Tarefa vinculada ao documento." else "Tarefa desvinculada do documento.")
            requestSync(null, auto = true)
        }, {
            // The reason is shown on the task screen, under the picker.
            if (taskOnScreen(t.id, gen)) {
                stack.removeAll { it is Route.Pick }
                taskFailed(it)
            }
        }, fail = "Não foi possível trocar o documento da tarefa. Tente de novo.")
    }

    fun createTask(
        title: String, action: String, due: String, assignee: String = "", categories: List<String> = emptyList(),
        planning: NewPlanning = NewPlanning(), projectUid: String = ""
    ) {
        val route = stack.lastOrNull() as? Route.NewTask ?: return
        if (title.isBlank() || planning.problem().isNotEmpty()) return
        // A second tap (or a tap while the first is still being saved) starts nothing.
        newTaskSave = SaveGate.begin(newTaskSave) ?: return
        SyncRunner.background({
            Api.createTask(route.id, title.trim(), action, due, assignee.trim(), categories, planning, projectUid)
            // The task exists locally from here on: a failure below must not offer to create it again.
            SyncRunner.refreshDocs()
        }, {
            newTaskSave = SaveGate.done()
            SaveGate.close(stack, route)
            takeDocs()
            refreshTasks()
            reloadDocument()
            say("Tarefa criada.")
            // A refusal or failure of the sync is told by the sync itself; the task is already kept.
            requestSync(null, auto = true)
        }, {
            if (stack.contains(route)) {
                newTaskSave = SaveGate.failed(it)
            } else {
                newTaskSave = SaveGate.done()
                status(it, true)
            }
        }, fail = "Não foi possível criar a tarefa. Seus dados continuam na tela; tente de novo.")
    }

    // ---------------------------------------------------------------- documents

    fun openDocument(id: Long) {
        if (id <= 0) return
        stack.removeAll { it is Route.Document || it is Route.Text || it is Route.Note || it is Route.NewTask }
        if (documentId != id) document = DocumentState()
        documentId = id
        stack.add(Route.Document(id))
        loadDocument()
    }

    private fun loadDocument() {
        val id = documentId
        if (id < 0) return
        SyncRunner.background({ Api.document(id).toDocDetail() }, { d ->
            if (documentId == id) {
                document = DocumentState(loading = false, error = null, doc = d.copy(tasks = d.tasks.map { it.withPending() }))
            }
        }, {
            if (documentId == id) document = DocumentState(loading = false, error = it, doc = document.doc)
        }, fail = "Não foi possível abrir o documento. Volte e tente de novo.")
    }

    private fun reloadDocument() {
        if (stack.any { it is Route.Document }) loadDocument()
    }

    /** True once the first Activity has attached: a later Activity that finds this is a duplicate. */
    fun isStarted() = started

    /** Reads the ficha again (after something was attached to it). */
    fun refreshDocument() = reloadDocument()

    /** True once the Drive file has been chosen: only then can something be received from another app. */
    fun isSetUp() = fileUri != null

    /** The app's own unlock prompt (or the passphrase one) is on screen: leaving the Activity is part of it. */
    fun unlockInProgress() = unlockBusy || Secrets.busy

    fun openText() {
        if (documentId >= 0) push(Route.Text(documentId))
    }

    fun openNote() {
        if (documentId >= 0 && document.doc != null) {
            if (!noteSave.busy) noteSave = SaveState()
            push(Route.Note(documentId))
        }
    }

    /** Progress of "Criar tarefa" (the form stays open until the library confirms). */
    var newTaskSave by mutableStateOf(SaveState())
        private set

    /** Progress of "Salvar nota". */
    var noteSave by mutableStateOf(SaveState())
        private set

    /**
     * Keeps the note on the phone and closes the editor only once that is confirmed; a refusal
     * leaves the text on screen to be sent again. The sync that follows is separate: its trouble
     * never says the note was not saved.
     */
    fun saveNote(text: String) {
        val route = stack.lastOrNull() as? Route.Note ?: return
        val id = route.id
        noteSave = SaveGate.begin(noteSave) ?: return
        SyncRunner.background({
            val o = JSONObject(SyncRunner.core.saveNote(id, text))
            if (!o.optBoolean("ok")) {
                throw CoreException(PhoneWords.adapt(o.s("error", "Não foi possível guardar a nota.")))
            }
            SyncRunner.refreshDocs()
        }, {
            noteSave = SaveGate.done()
            SaveGate.close(stack, route)
            document = document.copy(doc = document.doc?.takeIf { it.id == id }?.copy(note = text) ?: document.doc)
            takeDocs()
            reloadDocument()
            say("Nota salva.")
            requestSync(null, auto = true)
        }, {
            if (stack.contains(route)) {
                noteSave = SaveGate.failed(it)
            } else {
                noteSave = SaveGate.done()
                status(it, true)
            }
        }, fail = "Não foi possível guardar a nota neste aparelho. O texto continua na tela; tente salvar de novo.")
    }

    fun openFile(f: FileItem) {
        val a = activity ?: return
        openExported(a, f.kind, f.id, f.captureId)
    }

    // ---------------------------------------------------------------- record by photo

    /** Photos already stored in the library (process-wide), never to be saved again. */
    private val savedPaths: MutableSet<String> = java.util.Collections.synchronizedSet(HashSet())
    private var photoPath: String? = null
    private var newPath: String? = null

    fun openPhoto() {
        if (Route.Photo in stack) return
        photoPath = null
        newPath = null
        photo = PhotoState(kinds = manualKinds.ifEmpty { PhotoState().kinds })
            .let { s -> s.copy(kind = s.kinds.firstOrNull { it.key == "papel" }?.key ?: s.kinds.first().key) }
        if (manualKinds.isEmpty()) loadChoices()
        stack.add(Route.Photo)
        takePhoto()
    }

    private fun restorePhoto(b: Bundle?) {
        if (b == null) return
        photoPath = b.getString("photo")?.takeIf { it !in savedPaths }
        newPath = b.getString("new")
        photo = PhotoState(
            kind = b.getString("kind") ?: "papel",
            title = b.getString("title") ?: "",
            origin = b.getString("origin") ?: "",
            text = b.getString("text") ?: "",
            date = b.getString("date") ?: "",
            hasPhoto = photoPath != null
        )
        // A camera result may have arrived while the process was gone.
        newPath?.let { p -> if (File(p).length() > 0) acceptNew(p) }
        showThumb()
    }

    fun setPhotoTitle(v: String) { photo = photo.copy(title = v, titleError = "") }
    fun setPhotoKind(v: String) { photo = photo.copy(kind = v) }
    fun setPhotoDate(v: String) { photo = photo.copy(date = v) }
    fun setPhotoOrigin(v: String) { photo = photo.copy(origin = v) }
    fun setPhotoText(v: String) { photo = photo.copy(text = v) }

    fun takePhoto() {
        if (photo.saving) return
        val a = activity ?: return
        val dir = File(app.cacheDir, PonteFiles.DIR_PHOTOS)
        dir.mkdirs()
        val f = File(dir, "foto-" + System.currentTimeMillis() + ".jpg")
        val uri = PonteFiles.uriFor(PonteFiles.DIR_PHOTOS, f.name)
        val i = Intent(MediaStore.ACTION_IMAGE_CAPTURE).apply {
            putExtra(MediaStore.EXTRA_OUTPUT, uri)
            addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
            clipData = ClipData.newRawUri("foto", uri)
        }
        newPath = f.absolutePath
        try {
            @Suppress("DEPRECATION")
            a.startActivityForResult(i, MainActivity.REQ_PHOTO)
        } catch (_: ActivityNotFoundException) {
            newPath = null
            say("Este aparelho não tem aplicativo de câmera.", long = true)
            leaveEmptyPhoto()
        } catch (e: Throwable) {
            newPath = null
            say("Não foi possível abrir a câmera. Tente de novo.", long = true)
            leaveEmptyPhoto()
        }
    }

    /** Result of the camera app. */
    fun onCamera(ok: Boolean) {
        val p = newPath ?: return
        if (ok && File(p).length() > 0) {
            acceptNew(p)
        } else {
            File(p).delete()
            newPath = null
            leaveEmptyPhoto()
        }
    }

    /** Without a picture and with nothing typed there is no reason to stay on the form. */
    private fun leaveEmptyPhoto() {
        if (photoPath == null && stack.lastOrNull() == Route.Photo &&
            photo.title.isEmpty() && photo.origin.isEmpty() && photo.text.isEmpty()
        ) pop()
    }

    private fun acceptNew(p: String) {
        val old = photoPath
        photoPath = p
        newPath = null
        if (old != null && old != p) File(old).delete()
        photo = photo.copy(hasPhoto = true, thumb = null, error = "")
        showThumb()
    }

    private fun showThumb() {
        val p = photoPath ?: return
        Thread {
            val bmp = try { Images.decode(File(p), 900) } catch (_: Throwable) { null }
            ui {
                if (photoPath == p) {
                    photo = if (bmp != null) photo.copy(thumb = bmp.asImageBitmap(), hasPhoto = true)
                    else photo.copy(error = "Não foi possível mostrar a foto.")
                }
            }
        }.start()
    }

    fun savePhoto() {
        if (photo.saving) return
        val t = photo.title.trim()
        if (t.isEmpty()) {
            photo = photo.copy(titleError = "Informe o título.")
            return
        }
        val src = photoPath?.let { File(it) }
        if (src == null || !src.isFile) {
            photo = photo.copy(error = "Tire a foto antes de salvar.")
            return
        }
        val fields = JSONObject()
            .put("kind", photo.kind.ifEmpty { "papel" })
            .put("title", t)
            .put("origin", photo.origin.trim())
            .put("date", photo.date)
            .put("text", photo.text)
            .toString()
        photo = photo.copy(saving = true, progress = "Preparando a foto…", error = "", titleError = "")
        val out = File(src.parentFile, "envio-" + System.currentTimeMillis() + ".jpg")
        Thread {
            try {
                Images.resizeForUpload(src, out)
                ui { photo = photo.copy(progress = "Guardando o registro…") }
                SyncRunner.background({
                    val r = Api.createPhoto(fields, out.absolutePath)
                    SyncRunner.refreshDocs()
                    r.optLong("document_id")
                }, { id ->
                    // Saved: forget everything first so it can never be saved twice.
                    savedPaths.add(src.absolutePath)
                    photoPath = null
                    newPath = null
                    photo = PhotoState(kinds = photo.kinds)
                    src.delete()
                    out.delete()
                    takeDocs()
                    stack.remove(Route.Photo)
                    tab = 2
                    shell = shell.copy(tab = 2)
                    openDocument(id)
                    say("Registro salvo.")
                    requestSync(null, auto = true)
                }, { failPhoto(it, out) }, fail = PHOTO_FAILED)
            } catch (e: Throwable) {
                val m = PhoneWords.explain(e, PHOTO_FAILED)
                ui { failPhoto(m, out) }
            }
        }.start()
    }

    private fun failPhoto(msg: String, out: File) {
        out.delete()
        photo = photo.copy(saving = false, progress = "", error = msg)
    }

    /** Leaving without saving: the pictures are not needed. Not while it is being saved. */
    fun closePhoto() {
        if (photo.saving) return
        photoPath?.let { File(it).delete() }
        newPath?.let { File(it).delete() }
        photoPath = null
        newPath = null
        photo = PhotoState(kinds = photo.kinds)
        stack.remove(Route.Photo)
    }

    // ---------------------------------------------------------------- e-mail

    private var mailLoaded = false
    private var listedFolder = ""

    /** Grows whenever the account on screen is replaced: answers asked before it are ignored. */
    private var mailGen = 0

    private fun pwFor(acc: MailAccount) = if (acc.hasPassword) "" else (MailSession.passwords[acc.uid] ?: "")

    private fun pick(list: List<MailAccount>, uid: String?): MailAccount? =
        list.firstOrNull { it.uid == uid } ?: list.maxByOrNull { it.lastUsed }

    /** Reads the accounts; keeps what is being browsed when the same account is still there. */
    private fun loadAccounts(select: String? = null, then: (List<MailAccount>) -> Unit = {}) {
        SyncRunner.background({ Api.mailAccounts().map { it.toMailAccount() } }, { list ->
            mailLoaded = true
            val acc = pick(list, select ?: mail.account?.uid)
            mail = if (acc?.uid != mail.account?.uid) {
                mailGen++
                MailState(
                    loading = false, accounts = list, account = acc,
                    folder = acc?.folder ?: "INBOX", filter = acc?.filter ?: "todas"
                )
            } else {
                mail.copy(loading = false, accounts = list, account = acc)
            }
            if (Route.Settings in stack) updateSettings()
            then(list)
        }, {
            mail = mail.copy(loading = false, info = it, infoError = true)
            then(mail.accounts)
        }, fail = "Não foi possível ler as contas de e-mail. Tente de novo.")
    }

    /** The E-mail area came on screen: list what the last account was showing. */
    private fun openMailTab() {
        if (!mailLoaded) loadAccounts { browseIfNeeded() } else browseIfNeeded()
    }

    private fun browseIfNeeded() {
        if (tab != 3 || mail.account == null || mail.listed || mail.busy) return
        browse(explicit = false)
    }

    /** Lists folders (the first time) and messages, when the password is at hand. */
    private fun browse(explicit: Boolean) {
        val acc = mail.account ?: return
        if (mail.busy) return
        if (!acc.hasPassword && MailSession.passwords[acc.uid].isNullOrEmpty()) {
            mail = mail.copy(needsPassword = true)
            if (explicit) dialog = AppDialog.MailPassword(acc)
            return
        }
        if (!explicit && !networkAllowed()) {
            mail = mail.copy(
                needsPassword = false, infoError = false,
                info = "Sem Wi-Fi. Toque em atualizar para listar as mensagens pelos dados móveis."
            )
            return
        }
        val pw = pwFor(acc)
        val wanted = mail.folder
        val filter = mail.filter
        val known = mail.folders
        val gen = mailGen
        mail = mail.copy(busy = true, needsPassword = false, info = SERVER_MSG, infoError = false)
        SyncRunner.backgroundNetwork({
            val folders = known.ifEmpty {
                Api.mailFolders(acc.uid, pw).map { MailFolder(it.s("raw"), it.s("name").ifEmpty { it.s("raw") }) }
            }
            val folder = folders.firstOrNull { it.raw == wanted || it.name == wanted }?.raw
                ?: folders.firstOrNull { it.raw.equals("INBOX", ignoreCase = true) }?.raw
                ?: folders.firstOrNull()?.raw
                ?: wanted
            val msgs = Api.mailMessages(acc.uid, pw, folder, filter).map { it.toMailMessage() }
            Triple(folders, folder, msgs)
        }, { (folders, folder, msgs) ->
            // An answer for an account that was changed or removed meanwhile is dropped.
            if (gen == mailGen && mail.account?.uid == acc.uid) {
                listedFolder = folder
                mail = mail.copy(
                    busy = false, info = "", infoError = false, folders = folders, folder = folder,
                    messages = msgs, listed = true, checked = emptySet()
                )
            }
        }, { m ->
            // Maybe a wrong password: ask again next time.
            if (!acc.hasPassword) MailSession.passwords.remove(acc.uid)
            if (gen == mailGen && mail.account?.uid == acc.uid) {
                mail = mail.copy(busy = false, info = m, infoError = true, needsPassword = !acc.hasPassword)
            }
        }, fail = "Não foi possível listar as mensagens. Confira a conexão e a conta de e-mail e tente de novo.")
    }

    fun selectMailAccount(uid: String) {
        if (mail.busy || uid == mail.account?.uid) return
        val acc = mail.accounts.firstOrNull { it.uid == uid } ?: return
        mailGen++
        mail = MailState(loading = false, accounts = mail.accounts, account = acc, folder = acc.folder, filter = acc.filter)
        browse(explicit = true)
    }

    fun selectMailFolder(raw: String) {
        if (mail.busy || raw == mail.folder) return
        mail = mail.copy(folder = raw, messages = emptyList(), listed = false, checked = emptySet())
        browse(explicit = true)
    }

    fun selectMailFilter(key: String) {
        if (mail.busy || key == mail.filter) return
        mail = mail.copy(filter = key, messages = emptyList(), listed = false, checked = emptySet())
        browse(explicit = true)
    }

    fun toggleMail(uid: Long) {
        if (mail.busy) return
        mail = mail.copy(checked = if (uid in mail.checked) mail.checked - uid else mail.checked + uid)
    }

    fun reloadMail() {
        if (mail.account == null) loadAccounts { browseIfNeeded() } else browse(explicit = true)
    }

    fun askMailPassword() {
        mail.account?.let { dialog = AppDialog.MailPassword(it) }
    }

    fun setMailPassword(acc: MailAccount, password: String) {
        dialog = null
        if (password.isEmpty()) return
        MailSession.passwords[acc.uid] = password
        if (mail.account?.uid == acc.uid) browse(explicit = true)
    }

    fun importMail() {
        val acc = mail.account ?: return
        if (mail.checked.isEmpty() || mail.busy) return
        if (!networkAllowed()) {
            mail = mail.copy(info = "Sem Wi-Fi: importação adiada. Para usar dados móveis, ative em Ajustes.", infoError = false)
            return
        }
        val pw = pwFor(acc)
        val folder = listedFolder
        val ids = mail.checked.toList()
        val arr = JSONArray().also { j -> ids.forEach { j.put(it) } }.toString()
        val gen = mailGen
        mail = mail.copy(busy = true, info = SERVER_MSG, infoError = false)
        SyncRunner.backgroundNetwork({
            val r = Api.importMail(acc.uid, pw, folder, arr)
            SyncRunner.refreshDocs()
            r
        }, { results ->
            takeDocs()
            val done = HashSet<Long>()
            val failed = HashMap<Long, String>()
            var fresh = 0
            var already = 0
            for (r in results) {
                val uid = r.optLong("uid")
                if (r.optBoolean("ok")) {
                    done.add(uid)
                    if (r.optBoolean("already")) already++ else fresh++
                } else {
                    failed[uid] = PhoneWords.adapt(r.s("error", "Não foi possível importar esta mensagem."))
                }
            }
            val parts = ArrayList<String>()
            if (fresh > 0) parts.add(if (fresh == 1) "1 mensagem importada" else "$fresh mensagens importadas")
            if (already > 0) parts.add(if (already == 1) "1 já estava no acervo" else "$already já estavam no acervo")
            if (failed.isNotEmpty()) parts.add(if (failed.size == 1) "1 não pôde ser importada" else "${failed.size} não puderam ser importadas")
            val summary = if (parts.isEmpty()) "" else parts.joinToString(" · ") + "."
            if (gen == mailGen && mail.account?.uid == acc.uid) {
                mail = mail.copy(
                    busy = false, info = summary, infoError = failed.isNotEmpty() && done.isEmpty(),
                    checked = failed.keys.toSet(),
                    messages = mail.messages.map {
                        when (it.uid) {
                            in done -> it.copy(imported = true, error = null)
                            in failed -> it.copy(error = failed[it.uid])
                            else -> it
                        }
                    }
                )
            }
            say(summary, long = failed.isNotEmpty())
            if (done.isNotEmpty()) requestSync(null, auto = true)
        }, { m ->
            if (!acc.hasPassword) MailSession.passwords.remove(acc.uid)
            if (gen == mailGen && mail.account?.uid == acc.uid) {
                mail = mail.copy(busy = false, info = m, infoError = true, needsPassword = !acc.hasPassword)
            } else {
                say(m, long = true)
            }
        }, fail = "Não foi possível importar as mensagens. Confira a conexão e tente de novo.")
    }

    // ---------------------------------------------------------------- account form

    fun openAccount(acc: MailAccount?) {
        val r = Route.Account(acc?.uid ?: "")
        if (stack.lastOrNull() == r) return
        accountForm = AccountFormState(account = acc)
        stack.add(r)
    }

    fun saveAccount(input: AccountInput) {
        if (accountForm.busy) return
        val acc = accountForm.account
        val user = input.user.trim()
        val host = input.host.trim()
        if (user.isEmpty() || host.isEmpty()) {
            accountForm = accountForm.copy(error = "Informe o servidor e o usuário.")
            return
        }
        val json = JSONObject()
            .put("uid", acc?.uid ?: "")
            .put("name", input.name.trim().ifEmpty { user })
            .put("host", host)
            .put("port", input.port)
            .put("user", user)
            .put("password", input.password)
            .put("remember", input.remember)
            .put("folder", acc?.folder ?: "INBOX")
            .toString()
        accountForm = accountForm.copy(busy = true, error = "")
        SyncRunner.background({ Api.saveMailAccount(json) }, { uid ->
            if (input.password.isNotEmpty() && !input.remember && uid.isNotEmpty()) {
                MailSession.passwords[uid] = input.password
            }
            accountForm = AccountFormState()
            stack.removeAll { it is Route.Account }
            // A changed account is listed again; a new one becomes the one on screen.
            if (acc == null || mail.account?.uid == uid) {
                mailGen++
                mail = mail.copy(account = null, listed = false, busy = false)
                loadAccounts(select = uid) { browseIfNeeded() }
            } else {
                loadAccounts()
            }
            say("Conta salva.")
        }, { accountForm = accountForm.copy(busy = false, error = it) },
            fail = "Não foi possível salvar a conta. Confira os dados e tente de novo.")
    }

    fun askDeleteAccount() {
        accountForm.account?.let { dialog = AppDialog.DeleteAccount(it) }
    }

    fun deleteAccount(acc: MailAccount) {
        dialog = null
        if (accountForm.busy) return
        accountForm = accountForm.copy(busy = true, error = "")
        SyncRunner.background({ Api.deleteMailAccount(acc.uid) }, {
            MailSession.passwords.remove(acc.uid)
            accountForm = AccountFormState()
            stack.removeAll { it is Route.Account }
            loadAccounts { browseIfNeeded() }
            say("Conta excluída.")
        }, { accountForm = accountForm.copy(busy = false, error = it) },
            fail = "Não foi possível excluir a conta. Tente de novo.")
    }

    // ---------------------------------------------------------------- settings

    fun openSettings() {
        if (Route.Settings in stack) return
        stack.add(Route.Settings)
        nameField = null
        noticeDenied = false
        updateSettings()
        loadFileInfo()
        loadCalendars()
        if (!mailLoaded) loadAccounts()
    }

    private fun version(): String = try {
        app.packageManager.getPackageInfo(app.packageName, 0).versionName ?: ""
    } catch (_: Throwable) {
        ""
    }

    private fun updateSettings() {
        settings = SettingsState(
            fileName = if (fileUri == null) "" else fileName.ifEmpty { "Arquivo escolhido" },
            fileSize = fileSize,
            passphrase = SyncRunner.passphrase,
            canRemember = Secrets.supported() && (deviceSecure() || Secrets.isRemembered(app)),
            remembered = Secrets.isRemembered(app),
            mobileData = prefs.getBoolean("mobile_ok", false),
            accounts = mail.accounts,
            keyBroken = SyncRunner.keyBroken,
            pending = SyncRunner.pendingFile(app).exists(),
            version = version(),
            lastStatus = SyncRunner.statusText,
            lastStatusError = SyncRunner.statusError,
            lockAvailable = deviceSecure(),
            lockOn = LockPolicy.active(lockSetting(), deviceSecure()),
            lockDelay = lockDelay().key,
            secureScreen = secureScreen(),
            personName = nameField ?: personName,
            holidayUf = Holidays.current().uf ?: "",
            holidayDays = Holidays.current().custom,
            noticeOn = NoticeScheduler.isOn(app),
            noticeTime = NoticeScheduler.time(app).key,
            noticeBlocked = noticeDenied || (NoticeScheduler.isOn(app) && !NoticeScheduler.allowed(app)),
            calendarOn = calendarOn(),
            calendars = calendarChoices,
            calendarRefreshing = calendarRefreshing,
            calendarChosen = calendarChosenId().let { if (it >= 0) it.toString() else "" },
            calendarStatus = calendarStatusText(),
            calendarWarning = prefs.getString(CalendarSyncRunner.K_WARN, "") ?: ""
        )
    }

    // ---- the calendar of the phone

    /** The calendars the person can choose from; read again whenever Ajustes opens. */
    private var calendarChoices: List<CalInfo> = emptyList()
    private var calendarRefreshing = false

    /** A calendar sync is already waiting for the worker: more requests join it. */
    private var calObserver: android.database.ContentObserver? = null

    /** The calendar changed (or the app came back): a short wait lets a burst of edits end first. */
    private val calDebounce = Runnable { if (!locked) calendarSyncSoon() }

    private fun calendarOn() = prefs.getBoolean(CalendarSyncRunner.K_ON, false)
    private fun calendarChosenId() = prefs.getLong(CalendarSyncRunner.K_ID, -1L)

    /** The switch "Sincronizar tarefas com a agenda". Turning it on asks for the calendar permission first. */
    fun setCalendarEnabled(on: Boolean) {
        if (on) {
            if (DeviceCalendar.hasPermission(app)) {
                calendarOnNow()
            } else {
                val a = activity
                if (a != null) a.askCalendarPermission() else onCalendarPermission(false)
            }
            return
        }
        // Events already in the calendar: ask what to do with them.
        if (calendarChosenId() >= 0) dialog = AppDialog.CalendarOff else calendarOff(false)
    }

    /** The answer of the system's permission question for the calendar. */
    fun onCalendarPermission(granted: Boolean) {
        if (granted) {
            calendarOnNow()
        } else {
            prefs.edit().putString(CalendarSyncRunner.K_WARN, CALENDAR_DENIED).apply()
            updateSettings()
            say(CALENDAR_DENIED, long = true)
        }
    }

    private fun calendarOnNow() {
        prefs.edit().putBoolean(CalendarSyncRunner.K_ON, true).remove(CalendarSyncRunner.K_WARN).apply()
        CalendarBackground.arm(app)
        loadCalendars()
        watchCalendar(true)
        calendarSyncSoon()
        updateSettings()
    }

    /** Turns the sync off; [remove] takes the Ponte events out of the calendar too. */
    fun calendarOff(remove: Boolean) {
        dialog = null
        prefs.edit().putBoolean(CalendarSyncRunner.K_ON, false).remove(CalendarSyncRunner.K_WARN).apply()
        CalendarBackground.arm(app)
        watchCalendar(false)
        SyncRunner.background({ CalendarSyncRunner.finishOff(app, remove) }, { updateSettings() })
        updateSettings()
    }

    /** The person chose a calendar (key: its id as text). The events of a calendar chosen before are moved by the next run. */
    fun chooseCalendar(key: String) {
        val id = key.toLongOrNull() ?: return
        if (id == calendarChosenId()) return
        prefs.edit().putLong(CalendarSyncRunner.K_ID, id).remove(CalendarSyncRunner.K_WARN).apply()
        updateSettings()
        // A calendar that is hidden or not synced is switched on first, so its events reach Google.
        val needs = calendarChoices.firstOrNull { it.id == id }?.needsEnabling == true
        if (!needs) {
            calendarSyncSoon()
            return
        }
        SyncRunner.background({ CalendarSyncRunner.enable(app, id) }, { ok ->
            if (!ok) say(CALENDAR_NOT_ENABLED, long = true)
            loadCalendars()
            calendarSyncSoon()
        })
    }

    /** "Atualizar a lista": asks Android to pull the Google calendars, waits a moment and reads the list again. */
    fun refreshCalendars() {
        if (calendarRefreshing) return
        calendarRefreshing = true
        updateSettings()
        SyncRunner.background({ CalendarSyncRunner.refresh(app) }, { list ->
            calendarRefreshing = false
            calendarChoices = list
            updateSettings()
        }, {
            calendarRefreshing = false
            updateSettings()
        })
    }

    private fun loadCalendars() {
        SyncRunner.background({ CalendarSyncRunner.calendars(app) }, { list ->
            calendarChoices = list
            updateSettings()
        })
    }

    /** Asks for a reconcile on the worker (behind a sync, if one runs). Cheap when nothing changed. */
    fun calendarSyncSoon() {
        if (!::app.isInitialized || !calendarOn() || !SyncRunner.ready || calendarChosenId() < 0) return
        CalendarSyncRunner.enqueue(app, done = { calendarDone(it) })
    }

    private fun calendarDone(o: CalendarSyncRunner.Outcome?) {
        updateSettings()
        if (o == null) return
        if (o.turnedOff != null) {
            watchCalendar(false)
            say(o.turnedOff, long = true)
            return
        }
        if (o.tasksChanged) {
            // The calendar moved or renamed tasks: show them and send them to the Drive like any other change.
            takeDocs()
            refreshTasks()
            reloadDocument()
            refreshOpenTask()
            requestSync(null, auto = true)
        }
        if (o.notify.isNotEmpty()) say(o.notify.first(), long = true)
    }

    /** While the app is in front, a change in the calendar's events starts a reconcile (after a short wait). */
    private fun watchCalendar(on: Boolean) {
        main.removeCallbacks(calDebounce)
        calObserver?.let { try { app.contentResolver.unregisterContentObserver(it) } catch (_: Throwable) { } }
        calObserver = null
        if (!on || !calendarOn() || !DeviceCalendar.hasPermission(app)) return
        val o = object : android.database.ContentObserver(main) {
            override fun onChange(selfChange: Boolean) {
                main.removeCallbacks(calDebounce)
                main.postDelayed(calDebounce, CALENDAR_WAIT_MS)
            }
        }
        try {
            app.contentResolver.registerContentObserver(android.provider.CalendarContract.Events.CONTENT_URI, true, o)
            calObserver = o
        } catch (_: Throwable) {
        }
    }

    private fun calendarStatusText(): String {
        val at = prefs.getLong(CalendarSyncRunner.K_LAST, 0L)
        if (at <= 0L || !calendarOn()) return ""
        val t = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault())
        val pattern = if (t.toLocalDate() == LocalDate.now()) "HH:mm" else "dd/MM HH:mm"
        return "Última conferência: " + t.format(DateTimeFormatter.ofPattern(pattern))
    }

    // ---- the person's name and the daily notice

    /** The text as typed in the name field (blanks included, so a space between words can be typed). */
    private var nameField: String? = null

    /** The permission was refused: Ajustes says how to allow it, until the next try. */
    private var noticeDenied = false

    /** Saves "Seu nome nas tarefas" as it is typed: trimmed, at most 80 characters. Not a secret. */
    fun changePersonName(v: String) {
        nameField = v.take(Assignees.NAME_MAX)
        personName = Assignees.cleanName(v)
        prefs.edit().putString(K_NAME, personName).apply()
        if (Route.Setup in stack) setup = setup.copy(personName = nameField ?: "")
        updateSettings()
    }

    /**
     * The switch of the daily notice. Turning it on asks for the permission to show notifications
     * (Android 13 or newer); when it is refused the switch goes back off and Ajustes says how to allow it.
     */
    fun setNoticeEnabled(on: Boolean) {
        noticeDenied = false
        if (!on) {
            NoticeScheduler.setOn(app, false)
            NoticeScheduler.arm(app)
            updateSettings()
            return
        }
        if (NoticeScheduler.allowed(app)) {
            noticeOn()
            return
        }
        val a = activity
        if (android.os.Build.VERSION.SDK_INT >= 33 && a != null &&
            app.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            a.askNoticePermission()
        } else {
            // The phone itself blocks this app's notifications: only its settings can allow them.
            onNoticePermission(false)
        }
    }

    private fun noticeOn() {
        NoticeScheduler.setOn(app, true)
        NoticeScheduler.arm(app)
        updateSettings()
    }

    /** The answer of the system's permission question. */
    fun onNoticePermission(granted: Boolean) {
        if (granted && NoticeScheduler.allowed(app)) {
            noticeDenied = false
            noticeOn()
        } else {
            NoticeScheduler.setOn(app, false)
            NoticeScheduler.arm(app)
            noticeDenied = true
            updateSettings()
        }
    }

    fun setNoticeTime(key: String) {
        NoticeScheduler.setTime(app, DeadlineNotice.Time.fromKey(key))
        NoticeScheduler.arm(app)
        updateSettings()
    }

    /** The person set reminders on a task and the phone does not let this app notify (refused, or blocked in its settings). */
    var taskNoticeBlocked by mutableStateOf(false)
        private set

    /**
     * A reminder offset was chosen on a task. Without the permission (Android 13 or newer) the notices of the
     * tasks would stay mute, so it is asked for here, as the daily notice's switch does; if it is refused the
     * task screen says so in one line. Does not touch the switch of the daily notice.
     */
    fun taskRemindersChosen() {
        if (NoticeScheduler.allowed(app)) {
            taskNoticeBlocked = false
            PlannedNotices.refreshAsync(app)
            return
        }
        val a = activity
        if (android.os.Build.VERSION.SDK_INT >= 33 && a != null &&
            app.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            a.askNoticePermission(MainActivity.REQ_TASK_NOTICE)
        } else {
            // The phone itself blocks this app's notifications: only its settings can allow them.
            onTaskNoticePermission(false)
        }
    }

    /** The answer of the system's permission question asked from a task. */
    fun onTaskNoticePermission(granted: Boolean) {
        val ok = granted && NoticeScheduler.allowed(app)
        taskNoticeBlocked = !ok
        if (ok) PlannedNotices.refreshAsync(app)
    }

    /** The phone's own notification settings of this app, where the permission can be given. */
    fun openNoticeSettings() {
        val a = activity ?: return
        try {
            a.startActivity(
                Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                    .putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, app.packageName)
            )
        } catch (_: Throwable) {
            say("Não foi possível abrir as configurações do telefone. Abra-as e procure o Ponte de bolso.", long = true)
        }
    }

    /** Name and size of the Drive file. The provider may hit the network: off the UI thread. */
    private fun loadFileInfo() {
        val uri = fileUri ?: return
        val resolver = app.contentResolver
        Thread {
            var name = ""
            var size = -1L
            try {
                resolver.query(uri, null, null, null, null)?.use { c ->
                    if (c.moveToFirst()) {
                        val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        val z = c.getColumnIndex(OpenableColumns.SIZE)
                        if (n >= 0) name = c.getString(n) ?: ""
                        if (z >= 0 && !c.isNull(z)) size = c.getLong(z)
                    }
                }
            } catch (_: Throwable) {
            }
            ui {
                if (fileUri == uri) {
                    fileName = name
                    fileSize = size
                    updateSettings()
                    setup = setup.copy(fileName = name.ifEmpty { "Arquivo escolhido" })
                }
            }
        }.start()
    }

    fun pickFile() {
        val a = activity ?: return
        try {
            val i = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "*/*"
                addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                        Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                )
            }
            @Suppress("DEPRECATION")
            a.startActivityForResult(i, MainActivity.REQ_PICK)
        } catch (e: Throwable) {
            say("Não foi possível abrir a lista de arquivos. Tente de novo.", long = true)
        }
    }

    /** The person chose the Drive file in the system picker. */
    fun onFilePicked(uri: Uri) {
        try {
            app.contentResolver.takePersistableUriPermission(
                uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            )
            if (FilterMemory.clearsOnFilePick(fileUri?.toString(), uri.toString())) clearFilters()
            prefs.edit().putString("uri", uri.toString()).apply()
            fileUri = uri
            fileName = ""
            fileSize = -1L
            setup = setup.copy(fileName = "Arquivo escolhido")
            updateSettings()
            loadFileInfo()
        } catch (e: Throwable) {
            say("Não foi possível usar este arquivo. Escolha-o de novo.", long = true)
        }
    }

    /** The passphrase typed in Ajustes: kept in memory only. */
    fun setPassphrase(v: String) {
        SyncRunner.passphrase = v
        settings = settings.copy(passphrase = v)
    }

    fun setRemember(on: Boolean) {
        if (!on) {
            Secrets.forget(app)
            updateSettings()
            return
        }
        val p = SyncRunner.passphrase
        val a = activity
        if (p.isEmpty() || a == null) {
            say("Digite a senha de sincronização primeiro.")
            return
        }
        Secrets.remember(a, p) { ok, msg ->
            promptDone(ok)
            updateSettings()
            say(msg)
        }
    }

    fun forgetPassphrase() {
        SyncRunner.passphrase = ""
        Secrets.forget(app)
        updateSettings()
        say("Senha de sincronização esquecida.")
    }

    fun setMobileData(on: Boolean) {
        prefs.edit().putBoolean("mobile_ok", on).apply()
        updateSettings()
    }

    fun askReset() {
        dialog = AppDialog.Reset
    }

    fun reset() {
        dialog = null
        watchCalendar(false)
        if (!SyncRunner.startInit(app, true)) {
            say("Aguarde: há uma operação em andamento.")
        } else {
            // The dialog promises it: the stored passphrase goes too, the one
            // kept behind the fingerprint and the one in memory.
            Secrets.forget(app)
            SyncRunner.passphrase = ""
            // The lists of the old library are not the new one's: nothing stays filtered.
            clearFilters()
            // A share in progress (its draft and its copy) goes with the rest of the phone's data.
            Intake.wipeForReset()
        }
        render()
    }

    // ---------------------------------------------------------------- first setup

    fun setSetupPassphrase(v: String) { setup = setup.copy(passphrase = v) }
    fun setSetupRemember(on: Boolean) { setup = setup.copy(remember = on) }

    fun setupNext() {
        when (setup.step) {
            0 -> if (fileUri != null) setup = setup.copy(step = 1) else say("Escolha o arquivo primeiro.")
            1 -> {
                val p = setup.passphrase
                if (p.isEmpty()) {
                    say("Digite a senha de sincronização.")
                    return
                }
                SyncRunner.passphrase = p
                val go = {
                    setup = setup.copy(step = 2, status = "", statusError = false, done = false)
                    setupSync()
                }
                val a = activity
                if (setup.remember && Secrets.supported() && a != null) {
                    Secrets.remember(a, p) { ok, msg ->
                        promptDone(ok)
                        if (!ok) {
                            setup = setup.copy(remember = false)
                            say(msg, long = true)
                        }
                        go()
                    }
                } else {
                    go()
                }
            }
        }
    }

    /** The first sync; its result is shown on the setup screen itself. */
    fun setupSync() {
        if (SyncRunner.running.get()) {
            mobileOnce = false
            syncRefused(SyncRefusal.BUSY, auto = false)
            render()
            return
        }
        requestSync(null, auto = true)
        render()
    }

    /** "Usar dados móveis desta vez": the first sync on mobile data, without changing Ajustes. */
    fun setupSyncMobile() {
        mobileOnce = true
        setupSync()
    }

    private fun setupBack() {
        if (setup.syncing) return
        if (setup.step > 0) setup = setup.copy(step = setup.step - 1)
    }

    fun setupBackPressed() = setupBack()

    /** Leaves the first setup (finished, or put off: Ajustes has the same choices). */
    fun finishSetup() {
        stack.remove(Route.Setup)
        setup = newSetup()
        tab = 0
        shell = shell.copy(tab = 0)
        refreshTasks()
        render()
    }
}
