package io.github.demusis.ponte

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import java.io.File

/** One screen shown in the content area. */
internal interface Screen {
    val view: View
    fun refresh() {}

    /** Returns true when the screen handled Back itself. */
    fun onBack(): Boolean = false
}

/** Shell: top bar, tabs and navigation. The screens live in their own files. */
class MainActivity : Activity() {

    internal val ui by lazy { Ui(this) }
    internal val prefs by lazy { getSharedPreferences("ponte", Context.MODE_PRIVATE) }
    internal var fileUri: Uri? = null
    internal var current: Screen? = null

    private lateinit var statusView: TextView
    private lateinit var pendingView: TextView
    private lateinit var syncButton: Button
    private lateinit var content: FrameLayout
    private val tabButtons = ArrayList<Button>()

    private var currentName = "docs"
    private var currentDocId = -1L
    private var lastTab = 0
    private var idleTask: (() -> Unit)? = null
    private var wasRunning = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        live = this
        try {
            buildUi()
            prefs.getString("uri", null)?.let { fileUri = Uri.parse(it) }
            val fresh = savedInstanceState == null
            if (fresh) cleanTemp()
            if (!SyncRunner.ready && !SyncRunner.keyBroken && !SyncRunner.running.get()) {
                SyncRunner.startInit(this, false)
            }
            if (fresh) {
                showDocuments()
                idleTask = { startup() }
            } else {
                restore(savedInstanceState!!)
            }
            render()
        } catch (e: Throwable) {
            Toast.makeText(this, e.message ?: e.toString(), Toast.LENGTH_LONG).show()
        }
    }

    override fun onResume() {
        super.onResume()
        SyncRunner.listener = { render() }
        render()
    }

    override fun onDestroy() {
        if (live === this) live = null
        super.onDestroy()
    }

    override fun onPause() {
        SyncRunner.listener = null
        super.onPause()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putString("screen", currentName)
        outState.putLong("doc", currentDocId)
        outState.putInt("tab", lastTab)
        (current as? PhotoScreen)?.let {
            val b = Bundle()
            it.saveState(b)
            outState.putBundle("photo", b)
        }
    }

    private fun restore(s: Bundle) {
        lastTab = s.getInt("tab", 0)
        when (s.getString("screen")) {
            "tasks" -> showTasks()
            "mail" -> showMail()
            "settings" -> showSettings()
            "doc" -> showDocument(s.getLong("doc", -1L))
            "photo" -> showPhoto().restoreState(s.getBundle("photo"))
            else -> showDocuments()
        }
    }

    /** Old export/photo files from a previous run are not needed any more. */
    private fun cleanTemp() {
        try {
            val limit = System.currentTimeMillis() - 60L * 60 * 1000
            for (d in listOf(PonteFiles.DIR_SHARED, PonteFiles.DIR_PHOTOS)) {
                File(cacheDir, d).listFiles()?.forEach { if (it.isFile && it.lastModified() < limit) it.delete() }
            }
            cacheDir.listFiles()?.forEach {
                if (it.isFile && it.name.startsWith("sync-") && it.name.endsWith(".ponte") &&
                    it.lastModified() < limit) it.delete()
            }
        } catch (_: Throwable) {
        }
    }

    // ---------------------------------------------------------------- UI shell

    private fun buildUi() {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(ui.dp(12), ui.dp(8), ui.dp(12), ui.dp(4))
        }
        val bar = ui.hbox()
        bar.addView(ui.text("Ponte de bolso", 20f, bold = true), ui.weighted())
        bar.addView(ui.button("⚙") { showSettings() }, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        syncButton = ui.button("⟳ Sincronizar") { requestSync(null) }
        bar.addView(syncButton)
        root.addView(bar, ui.wide())

        statusView = ui.text("", 13f)
        root.addView(statusView, ui.wide())
        pendingView = ui.text("Alterações guardadas aguardando envio", 13f, Colors.AMBER)
        pendingView.visibility = View.GONE
        root.addView(pendingView, ui.wide())

        val tabs = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        listOf("Documentos", "Tarefas", "E-mail").forEachIndexed { i, label ->
            val b = ui.button(label) {
                when (i) { 0 -> showDocuments(); 1 -> showTasks(); else -> showMail() }
            }
            b.textSize = 13f
            b.isAllCaps = false
            tabButtons.add(b)
            tabs.addView(b, ui.weighted())
        }
        root.addView(tabs, ui.wide())

        content = FrameLayout(this)
        root.addView(content, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        setContentView(root)
    }

    private fun highlightTabs() {
        tabButtons.forEachIndexed { i, b ->
            b.alpha = if (i == lastTab && currentName != "settings") 1f else 0.55f
        }
    }

    /** Re-renders the shell from the process-wide state. */
    private fun render() {
        try {
            val busy = SyncRunner.running.get()
            syncButton.isEnabled = !busy && !SyncRunner.keyBroken
            statusView.text = SyncRunner.statusText
            statusView.setTextColor(
                if (SyncRunner.statusError) Color.RED
                else if (SyncRunner.statusText.startsWith("Sincronizado")) Colors.GREEN
                else Color.BLACK
            )
            pendingView.visibility =
                if (SyncRunner.pendingFile(this).exists()) View.VISIBLE else View.GONE
            if (wasRunning && !busy) current?.refresh()
            wasRunning = busy
            if (!busy) {
                val t = idleTask
                if (t != null) {
                    idleTask = null
                    t()
                }
            }
        } catch (_: Throwable) {
        }
    }

    internal fun toast(msg: String) = Toast.makeText(this, msg, Toast.LENGTH_LONG).show()

    internal fun status(msg: String, error: Boolean) {
        SyncRunner.setStatus(msg, error)
        render()
    }

    // ---------------------------------------------------------------- Navigation

    private fun show(s: Screen, name: String, tab: Int?) {
        current = s
        currentName = name
        if (tab != null) lastTab = tab
        content.removeAllViews()
        content.addView(s.view, FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        highlightTabs()
    }

    internal fun showDocuments() = show(DocumentsScreen(this), "docs", 0)
    internal fun showTasks() = show(TasksScreen(this), "tasks", 1)
    internal fun showMail() = show(MailScreen(this), "mail", 2)
    internal fun showSettings() = show(SettingsScreen(this), "settings", null)

    internal fun showDocument(id: Long) {
        currentDocId = id
        show(DocumentScreen(this, id), "doc", null)
    }

    internal fun showPhoto(): PhotoScreen {
        val p = PhotoScreen(this)
        show(p, "photo", 0)
        return p
    }

    @Suppress("DEPRECATION")
    internal fun goBack() = onBackPressed()

    @Deprecated("Classic back callback")
    override fun onBackPressed() {
        try {
            if (current?.onBack() == true) return
            when (currentName) {
                "doc", "photo", "settings" -> when (lastTab) {
                    1 -> showTasks()
                    2 -> showMail()
                    else -> showDocuments()
                }
                else -> @Suppress("DEPRECATION") super.onBackPressed()
            }
        } catch (_: Throwable) {
            @Suppress("DEPRECATION") super.onBackPressed()
        }
    }

    // ---------------------------------------------------------------- Startup

    private fun startup() {
        if (SyncRunner.keyBroken) { showSettings(); return }
        val uri = fileUri
        if (uri == null) { chooseHome(); return }
        if (SyncRunner.passphrase.isEmpty() && !Secrets.isRemembered(this)) {
            status("Informe a frase-senha em Ajustes para sincronizar.", false)
            chooseHome()
            return
        }
        requestSync(null, auto = true, then = { chooseHome() })
    }

    /** Opens Tarefas when something is late or due today, else Documentos. */
    private fun chooseHome() {
        SyncRunner.background({ Api.tasks() }, { tasks ->
            val urgent = tasks.any {
                it.s("status") != "concluida" && (it.optBoolean("overdue") || it.optBoolean("due_today"))
            }
            if (urgent && current is DocumentsScreen) showTasks()
        })
    }

    // ---------------------------------------------------------------- Sync

    /** True unless the network is metered and mobile data is not allowed. */
    internal fun networkAllowed(): Boolean {
        if (prefs.getBoolean("mobile_ok", false)) return true
        return try {
            val cm = getSystemService(ConnectivityManager::class.java)
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
    private fun withPassphrase(onOk: (String) -> Unit, onFail: () -> Unit) {
        if (SyncRunner.passphrase.isNotEmpty()) {
            onOk(SyncRunner.passphrase)
            return
        }
        if (!Secrets.isRemembered(this)) {
            status("Informe a frase-senha em Ajustes para sincronizar.", false)
            onFail()
            return
        }
        Secrets.unlock(this) { pass, msg ->
            if (pass != null) {
                SyncRunner.passphrase = pass
                onOk(pass)
            } else {
                status(msg ?: "Frase-senha não liberada.", false)
                onFail()
            }
        }
    }

    /** Saves the note locally without a sync (used when the sync cannot start now). */
    internal fun saveNoteOnly(note: Pair<Long, String>) {
        SyncRunner.background({
            val o = org.json.JSONObject(SyncRunner.core.saveNote(note.first, note.second))
            if (!o.optBoolean("ok")) throw CoreException(o.s("error", "Erro desconhecido."))
            SyncRunner.refreshDocs()
        }, { current?.refresh() }, { status(it, true) })
    }

    /** Starts a sync (optionally saving a note first). Local changes are never lost if it cannot start. */
    internal fun requestSync(note: Pair<Long, String>?, auto: Boolean = false, then: (() -> Unit)? = null) {
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
                status("Sem Wi-Fi: sincronização adiada. Para usar dados móveis, ative em Ajustes.", false)
                then?.invoke()
                return
            }
            withPassphrase({ pass ->
                try {
                    if (SyncRunner.startSync(this, uri, pass, note)) {
                        idleTask = then
                    } else {
                        note?.let { saveNoteOnly(it) }
                        if (!auto) toast("Já há uma sincronização em andamento.")
                        then?.invoke()
                    }
                    render()
                } catch (e: Throwable) {
                    note?.let { saveNoteOnly(it) }
                    status(e.message ?: e.toString(), true)
                }
            }, {
                note?.let { saveNoteOnly(it) }
                then?.invoke()
            })
        } catch (e: Throwable) {
            status(e.message ?: e.toString(), true)
        }
    }

    /** Completes or reopens a task, then syncs. */
    internal fun changeTask(taskId: Long, done: Boolean) {
        SyncRunner.background({ Api.setDone(taskId, done) }, {
            current?.refresh()
            requestSync(null)
        }, { status(it, true) })
    }

    // ---------------------------------------------------------------- File picking

    internal fun pickFile() {
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
            startActivityForResult(i, REQ_PICK)
        } catch (e: Throwable) {
            status(e.message ?: e.toString(), true)
        }
    }

    // Plain Activity: no AndroidX result API available, so the classic callback is used.
    @Deprecated("Classic result callback")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        try {
            when (requestCode) {
                REQ_PHOTO -> (current as? PhotoScreen)?.onCamera(resultCode == RESULT_OK)
                REQ_PICK -> {
                    if (resultCode != RESULT_OK) return
                    val uri = data?.data ?: return
                    contentResolver.takePersistableUriPermission(
                        uri,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    )
                    prefs.edit().putString("uri", uri.toString()).apply()
                    fileUri = uri
                    (current as? SettingsScreen)?.refresh()
                }
            }
        } catch (e: Throwable) {
            status(e.message ?: e.toString(), true)
        }
    }

    internal fun confirmReset() {
        android.app.AlertDialog.Builder(this)
            .setTitle("Recomeçar o acervo deste aparelho")
            .setMessage(
                "Isso apaga o acervo local e a chave deste aparelho. Só se perdem as " +
                    "alterações que ainda não foram enviadas; todo o resto volta do Drive " +
                    "na próxima sincronização. Continuar?"
            )
            .setPositiveButton("Recomeçar") { _, _ ->
                if (!SyncRunner.startInit(this, true)) toast("Aguarde: há uma operação em andamento.")
                render()
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    companion object {
        /** The live Activity (null between destroy and create); background callbacks must use it. */
        @Volatile internal var live: MainActivity? = null
        internal const val REQ_PICK = 1
        internal const val REQ_PHOTO = 2
    }
}
