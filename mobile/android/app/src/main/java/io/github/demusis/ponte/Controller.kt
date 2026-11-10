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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.Lifecycle
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter

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

    /** A new task for a document. */
    data class NewTask(val id: Long) : Route() { override val key get() = "task:$id" }

    /** The screen of one task (edit, conclude, change its document). */
    data class EditTask(val id: Long) : Route() { override val key get() = "edit:$id" }

    /** The list to pick the document of task [taskId] from. */
    data class Pick(val taskId: Long) : Route() { override val key get() = "pick:$taskId" }

    object Photo : Route() { override val key get() = "photo" }
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
                "task" -> arg.toLongOrNull()?.let { NewTask(it) }
                "edit" -> arg.toLongOrNull()?.let { EditTask(it) }
                "pick" -> arg.toLongOrNull()?.let { Pick(it) }
                "photo" -> Photo
                "settings" -> Settings
                "account" -> Account(arg)
                "setup" -> Setup
                else -> null
            }
        }
    }
}

/** The name this phone gives itself in the library (the "Eu" of the task screen). */
internal fun phoneMemberName(): String = "Celular (" + android.os.Build.MODEL + ")"

/** A question or message shown over whatever is on screen. */
internal sealed class AppDialog {
    /** The whole text of the last sync status. */
    data class Status(val text: String, val error: Boolean, val pending: Boolean) : AppDialog()
    object Reset : AppDialog()
    data class DeleteAccount(val account: MailAccount) : AppDialog()
    data class MailPassword(val account: MailAccount) : AppDialog()
}

/**
 * What the app shows and does. Lives as long as the process, so a recreated Activity (theme
 * change, rotation) finds the same state and running work; after the process dies the
 * navigation and the photo form come back from the saved instance state.
 *
 * Everything here runs on the UI thread. The passphrase and the mail passwords are never put
 * in the saved state.
 */
internal object Controller {
    private const val SERVER_MSG = "Conversando com o servidor de e-mail…"
    private const val PHOTO_FAILED =
        "Não foi possível guardar o registro. Confira o espaço livre no aparelho e tente de novo."
    private const val START_FAILED ="Não foi possível iniciar a sincronização. Tente de novo."
    private const val K_LOCK = "lock_on"
    private const val K_LOCK_AFTER = "lock_after"
    private const val K_SECURE = "secure_screen"

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

    var tab by mutableStateOf(0)
        private set
    val stack = mutableStateListOf<Route>()
    var shell by mutableStateOf(ShellState())
        private set
    var docs by mutableStateOf<List<Doc>>(emptyList())
        private set
    var docQuery by mutableStateOf("")
    var docFilter by mutableStateOf(DocFilter.ALL)
    var tasks by mutableStateOf(TasksState())
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
        fileUri = prefs.getString("uri", null)?.let { Uri.parse(it) }
        lastSync = prefs.getLong("last_sync", 0L)
        val fresh = saved == null
        if (fresh) cleanTemp()
        if (!started) {
            // A new process always starts behind the lock (if the lock is in force), and nothing
            // handed to other apps in an earlier run is kept. Photos of a form that is coming
            // back from the saved state are the exception.
            locked = LockPolicy.shouldLock(lockSetting(), deviceSecure(), lockDelay(), null, coldStart = true)
            if (locked) lockEpoch++
            val b = saved?.getBundle("photo")
            cleanHandedFiles(setOfNotNull(b?.getString("photo"), b?.getString("new")))
        }
        if (!SyncRunner.ready && !SyncRunner.keyBroken && !SyncRunner.running.get()) {
            SyncRunner.startInit(a, false)
        }
        if (!started) {
            started = true
            if (saved != null) restore(saved) else idleTask = { startup() }
            loadChoices()
            refreshTasks()
        } else if (fresh) {
            // The person left with Back and came again while the process was still alive.
            if (!photo.saving) {
                closePhoto()
                stack.clear()
                tab = 0
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
        docQuery = s.getString("query") ?: ""
        docFilter = try { DocFilter.valueOf(s.getString("filter") ?: "") } catch (_: Throwable) { DocFilter.ALL }
        stack.clear()
        s.getStringArrayList("stack")?.mapNotNull { Route.parse(it) }?.forEach { stack.add(it) }
        // The first setup starts over (the passphrase is never saved).
        if (Route.Setup in stack) setup = newSetup()
        for (r in stack.toList()) {
            when (r) {
                is Route.Document -> documentId = r.id
                is Route.Text -> documentId = r.id
                is Route.Note -> documentId = r.id
                is Route.NewTask -> if (r.id > 0) documentId = r.id
                is Route.EditTask -> loadTask(r.id)
                is Route.Pick -> loadTask(r.taskId)
                is Route.Account -> loadAccounts { list ->
                    val found = list.firstOrNull { it.uid == r.uid }
                    // An account that no longer exists must not come back as a new one.
                    if (r.uid.isNotEmpty() && found == null) stack.remove(r)
                    else accountForm = AccountFormState(account = found)
                }
                Route.Settings, Route.Setup -> loadFileInfo()
                Route.Photo -> restorePhoto(s.getBundle("photo"))
            }
        }
        if (documentId >= 0) loadDocument()
        if (tab == 3) openMailTab()
    }

    /** Old export/photo files from a previous run are not needed any more. */
    private fun cleanTemp() {
        try {
            val limit = System.currentTimeMillis() - 60L * 60 * 1000
            for (d in listOf(PonteFiles.DIR_SHARED, PonteFiles.DIR_PHOTOS)) {
                File(app.cacheDir, d).listFiles()?.forEach { if (it.isFile && it.lastModified() < limit) it.delete() }
            }
            app.cacheDir.listFiles()?.forEach {
                if (it.isFile && it.name.startsWith("sync-") && it.name.endsWith(".ponte") &&
                    it.lastModified() < limit) it.delete()
            }
        } catch (_: Throwable) {
        }
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
            }
            val p = ch.optJSONArray("priorities").objs().map { Choice(it.s("key"), it.s("label")) }
            if (p.isNotEmpty()) priorities = p
        })
    }

    private fun newSetup() = SetupState(
        canRemember = Secrets.supported() && deviceSecure(), oldAndroid = !Secrets.supported()
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

    /** The Activity came to the front (not for a rotation: it is not stopped then). */
    fun onActivityStart() {
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
        snackbar.currentSnackbarData?.dismiss()
        if (clean) cleanHandedFiles()
    }

    private fun unlocked() {
        if (!locked) return
        locked = false
        // Said only now, never over the lock screen.
        lockNote?.let { lockNote = null; status(it, false) }
        render()
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
            updateSettings()
            return
        }
        val a = activity
        if (!deviceSecure()) {
            // Nothing to prove with: the lock is not in force anyway.
            prefs.edit().putBoolean(K_LOCK, false).apply()
            updateSettings()
            return
        }
        if (a == null || unlockBusy) return
        unlockBusy = true
        AppLock.verify(a, "Desligar o bloqueio", {
            unlockBusy = false
            prefs.edit().putBoolean(K_LOCK, false).apply()
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

    /** An exported file was handed to a viewer: when the person is back, it is deleted. */
    fun noteExternalOpen() {
        openedExternally = true
    }

    /**
     * Deletes what was handed to other apps: every exported file, and the photos of forms that
     * are not in progress. Photos of the "Registro por foto" being filled stay (or the whole
     * folder while it is being saved) until the record is created or the form is closed.
     */
    private fun cleanHandedFiles(keep: Set<String> = setOfNotNull(photoPath, newPath)) {
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
            docs = SyncRunner.docs
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
                badge = tasks.tasks.count { !it.done && (it.overdue || it.dueToday) },
                syncing = busy,
                statusLine = line,
                statusKind = kind,
                syncEnabled = !SyncRunner.keyBroken
            )
            if (stack.lastOrNull() == Route.Setup && setup.step == 2) {
                setup = setup.copy(
                    syncing = busy,
                    status = if (busy) "" else text,
                    statusError = SyncRunner.statusError,
                    done = !busy && text.startsWith("Sincronizado")
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
        refreshTasks()
        reloadDocument()
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

    /** False when Back should leave the app. */
    fun canGoBack(): Boolean {
        val top = stack.lastOrNull() ?: return tab != 0
        return !(top == Route.Setup && setup.step == 0)
    }

    fun back() {
        when (val top = stack.lastOrNull()) {
            null -> if (tab != 0) selectTab(0)
            Route.Photo -> closePhoto()
            Route.Setup -> setupBack()
            is Route.Account -> if (!accountForm.busy) pop()
            is Route.EditTask, is Route.Pick -> if (!taskEdit.busy) pop()
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
    private fun networkAllowed(): Boolean {
        if (prefs.getBoolean("mobile_ok", false)) return true
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
    private fun withPassphrase(notify: Boolean, onOk: (String) -> Unit, onFail: () -> Unit) {
        if (SyncRunner.passphrase.isNotEmpty()) {
            onOk(SyncRunner.passphrase)
            return
        }
        if (!Secrets.isRemembered(app)) {
            status("Informe a senha de sincronização em Ajustes para sincronizar.", false, notify)
            onFail()
            return
        }
        val a = activity
        if (a == null || !inFront()) {
            // The fingerprint prompt needs the app in front: the sync is tried again on return.
            if (idleTask == null) idleTask = { requestSync(null, auto = true) }
            onFail()
            return
        }
        Secrets.unlock(a) { pass, msg ->
            promptDone(pass != null)
            if (pass != null) {
                SyncRunner.passphrase = pass
                onOk(pass)
            } else {
                status(msg ?: "Senha de sincronização não liberada.", false, notify)
                onFail()
            }
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
            docs = SyncRunner.docs
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
        val notify = !auto
        try {
            val uri = fileUri
            if (uri == null) {
                note?.let { saveNoteOnly(it) }
                if (!auto) status("Escolha primeiro o arquivo ponte-sync.ponte em Ajustes.", true)
                then?.invoke()
                return
            }
            if (!networkAllowed()) {
                note?.let { saveNoteOnly(it) }
                status("Sem Wi-Fi: sincronização adiada. Para usar dados móveis, ative em Ajustes.", false, notify)
                then?.invoke()
                return
            }
            withPassphrase(notify, { pass ->
                try {
                    if (SyncRunner.startSync(app, uri, pass, note)) {
                        idleTask = then
                    } else {
                        note?.let { saveNoteOnly(it) }
                        if (!auto) say("Já há uma sincronização em andamento.")
                        then?.invoke()
                    }
                    render()
                } catch (e: Throwable) {
                    note?.let { saveNoteOnly(it) }
                    status(PhoneWords.explain(e, START_FAILED), true)
                }
            }, {
                note?.let { saveNoteOnly(it) }
                then?.invoke()
            })
        } catch (e: Throwable) {
            status(PhoneWords.explain(e, START_FAILED), true)
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
        else copy(done = d, overdue = if (d) false else overdue, dueToday = if (d) false else dueToday)
    }

    fun refreshTasks() {
        SyncRunner.background({ Api.tasks().map { it.toTask() } }, { read ->
            val list = read.map { it.withPending() }
            tasks = TasksState(loading = false, error = null, tasks = list)
            shell = shell.copy(badge = list.count { t -> !t.done && (t.overdue || t.dueToday) })
        }, { tasks = TasksState(loading = false, error = it, tasks = tasks.tasks) },
            fail = "Não foi possível ler as tarefas. Toque em Sincronizar agora e tente de novo.")
        refreshToday()
    }

    /** Reads what Hoje lists; what the person just concluded stays out even before it is saved. */
    private fun refreshToday() {
        SyncRunner.background({
            val o = Api.today()
            val tasksOf = { key: String -> o.optJSONArray(key).objs().map { it.toTask() } }
            TodayState(
                loading = false, late = tasksOf("late"), today = tasksOf("today"), upcoming = tasksOf("upcoming"),
                attendances = o.optJSONArray("attendances").objs().map { it.toAttendance() }
            )
        }, { read ->
            val open = { list: List<TaskItem> -> list.map { it.withPending() }.filter { !it.done } }
            today = read.copy(late = open(read.late), today = open(read.today), upcoming = open(read.upcoming))
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
            else x.copy(done = done, overdue = if (done) false else x.overdue, dueToday = if (done) false else x.dueToday)
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
            docs = SyncRunner.docs
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
        push(Route.NewTask(documentId))
    }

    /** From Tarefas: a task without document (it can be linked to one later). */
    fun openNewTaskWithoutDocument() {
        if (actions.isEmpty()) loadChoices()
        push(Route.NewTask(0))
    }

    // ---- the task screen

    /** Opens a task to edit. */
    fun openTask(id: Long) {
        if (id <= 0) return
        val r = Route.EditTask(id)
        if (stack.lastOrNull() == r) return
        if (actions.isEmpty()) loadChoices()
        taskEdit = TaskEditState()
        stack.add(r)
        loadTask(id)
    }

    private fun loadTask(id: Long) {
        SyncRunner.background({ Api.task(id) }, { d ->
            if (stack.any { it is Route.EditTask && it.id == id }) taskEdit = TaskEditState(loading = false, task = d)
        }, {
            if (stack.any { it is Route.EditTask && it.id == id }) taskEdit = TaskEditState(loading = false, error = it)
        }, fail = "Não foi possível abrir a tarefa. Volte e tente de novo.")
    }

    /** Saves what changed in the form; the screen closes when it worked. */
    fun saveTask(f: TaskForm) = saveTask(f, thenFinish = false)

    private fun saveTask(f: TaskForm, thenFinish: Boolean) {
        val t = taskEdit.task ?: return
        if (taskEdit.busy || f.title.isBlank()) return
        val changes = f.changes(t.form)
        if (changes.isEmpty()) {
            stack.remove(Route.EditTask(t.id))
            if (thenFinish) finishNow(t)
            return
        }
        taskEdit = taskEdit.copy(busy = true, formError = "")
        SyncRunner.background({
            Api.saveTask(t.id, t.version, changes)
            SyncRunner.refreshDocs()
        }, {
            docs = SyncRunner.docs
            taskEdit = TaskEditState(loading = false, task = t.copy(form = f))
            stack.remove(Route.EditTask(t.id))
            refreshTasks()
            reloadDocument()
            if (thenFinish) finishNow(t) else {
                say("Tarefa salva.")
                requestSync(null, auto = true)
            }
        }, { taskEdit = taskEdit.copy(busy = false, formError = it) },
            fail = "Não foi possível salvar a tarefa. Tente de novo.")
    }

    /** Concludes the task (or reopens it, when it was concluded), saving typed changes first. */
    fun finishTask(f: TaskForm) {
        val t = taskEdit.task ?: return
        if (taskEdit.busy || f.title.isBlank()) return
        if (f.changedFrom(t.form)) saveTask(f, thenFinish = true) else {
            stack.remove(Route.EditTask(t.id))
            finishNow(t)
        }
    }

    private fun finishNow(t: TaskDetail) {
        setTaskDone(t.asItem(), !t.done, offerUndo = !t.done)
        if (t.done) say("Tarefa reaberta.")
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
        taskEdit = taskEdit.copy(busy = true, formError = "")
        SyncRunner.background({
            Api.relinkTask(t.id, documentId, t.version)
            SyncRunner.refreshDocs()
        }, {
            docs = SyncRunner.docs
            stack.removeAll { it is Route.Pick }
            taskEdit = taskEdit.copy(busy = false)
            loadTask(t.id)
            refreshTasks()
            reloadDocument()
            say(if (documentId > 0) "Tarefa vinculada ao documento." else "Tarefa desvinculada do documento.")
            requestSync(null, auto = true)
        }, {
            // The reason is shown on the task screen, under the picker.
            stack.removeAll { it is Route.Pick }
            taskEdit = taskEdit.copy(busy = false, formError = it)
        }, fail = "Não foi possível trocar o documento da tarefa. Tente de novo.")
    }

    fun createTask(title: String, action: String, due: String) {
        val id = (stack.lastOrNull() as? Route.NewTask)?.id ?: return
        if (title.isBlank()) return
        pop()
        SyncRunner.background({
            Api.createTask(id, title.trim(), action, due)
            SyncRunner.refreshDocs()
        }, {
            docs = SyncRunner.docs
            refreshTasks()
            reloadDocument()
            say("Tarefa criada.")
            requestSync(null, auto = true)
        }, { status(it, true) }, fail = "Não foi possível criar a tarefa. Tente de novo.")
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

    fun openText() {
        if (documentId >= 0) push(Route.Text(documentId))
    }

    fun openNote() {
        if (documentId >= 0 && document.doc != null) push(Route.Note(documentId))
    }

    /**
     * Keeps the note on the phone at once (before any fingerprint prompt, so it cannot be
     * lost) and then sends it; the sync runs on the same worker, after the note is saved.
     */
    fun saveNote(text: String) {
        val id = (stack.lastOrNull() as? Route.Note)?.id ?: return
        pop()
        document = document.copy(doc = document.doc?.takeIf { it.id == id }?.copy(note = text) ?: document.doc)
        say("Nota salva.")
        saveNoteOnly(Pair(id, text))
        requestSync(null, auto = true)
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
                    docs = SyncRunner.docs
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
            docs = SyncRunner.docs
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
        updateSettings()
        loadFileInfo()
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
            secureScreen = secureScreen()
        )
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
        if (!SyncRunner.startInit(app, true)) {
            say("Aguarde: há uma operação em andamento.")
        } else {
            // The dialog promises it: the stored passphrase goes too, the one
            // kept behind the fingerprint and the one in memory.
            Secrets.forget(app)
            SyncRunner.passphrase = ""
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
            render()
            return
        }
        requestSync(null, auto = true)
        render()
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
