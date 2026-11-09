package io.github.demusis.ponte

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.util.Base64
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.BaseAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView
import android.widget.Toast
import org.json.JSONObject
import java.io.File
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** A document row shown in the list. noteError is set when the note cannot be read. */
internal data class Doc(
    val id: Long,
    val number: String,
    val title: String,
    val note: String,
    val noteError: String?
)

/** Raised when the core answers {"ok":false}. */
internal class CoreException(message: String) : Exception(message)

/** Raised when the Keystore cannot decrypt the stored library key. */
internal class KeyUnavailableException(cause: Throwable?) : Exception("key unavailable", cause)

/**
 * Process-wide owner of the single worker, the "running" flag and the last visible state, so a
 * rotation (new Activity) neither starts a second sync nor loses the result.
 */
internal object SyncRunner {
    private const val PENDING = "pendente.ponte"
    private const val ALIAS = "ponte_master"

    private val executor = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    val running = AtomicBoolean(false)
    val core = PonteCore()

    @Volatile var ready = false
    @Volatile var keyBroken = false
    @Volatile var statusText = ""
    @Volatile var statusError = false
    @Volatile var docs: List<Doc> = emptyList()

    /** Kept only while the process lives; never saved in instance state. */
    @Volatile var passphrase = ""

    /** Set by the visible Activity (onResume) and cleared in onPause. UI thread only. */
    var listener: (() -> Unit)? = null

    fun setStatus(text: String, error: Boolean) {
        statusText = text
        statusError = error
    }

    fun changed() {
        main.post { try { listener?.invoke() } catch (_: Throwable) { } }
    }

    fun pendingFile(ctx: Context) = File(ctx.filesDir, PENDING)

    /** Runs task on the worker if nothing else is running; returns false when ignored. */
    fun submit(task: () -> Unit): Boolean {
        if (!running.compareAndSet(false, true)) return false
        changed()
        executor.execute {
            try {
                task()
            } catch (e: Throwable) {
                setStatus(e.message ?: e.toString(), true)
            } finally {
                running.set(false)
                changed()
            }
        }
        return true
    }

    // ------------------------------------------------------------ init / reset

    fun startInit(ctx: Context, reset: Boolean): Boolean {
        val app = ctx.applicationContext
        setStatus("Preparando...", false)
        return submit {
            try {
                if (reset) wipeLocalState(app)
                val key = loadOrCreateLibraryKey(app)
                val name = "Celular (" + Build.MODEL + ")"
                check(core.init(app.filesDir.absolutePath, key, name))
                ready = true
                keyBroken = false
                val e = refreshDocs()
                if (e == null) setStatus("Pronto.", false) else setStatus(e, true)
            } catch (e: KeyUnavailableException) {
                keyBroken = true
                setStatus("Não foi possível abrir a chave deste aparelho.", true)
            }
        }
    }

    private fun wipeLocalState(ctx: Context) {
        for (n in listOf("acervo.sqlite3", "acervo.sqlite3-wal", "acervo.sqlite3-shm", PENDING)) {
            File(ctx.filesDir, n).delete()
        }
        prefsOf(ctx).edit().remove("key_ct").remove("key_iv").commit()
        try {
            val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            if (ks.containsAlias(ALIAS)) ks.deleteEntry(ALIAS)
        } catch (_: Exception) {
        }
    }

    /** Reloads the document list; returns an error message or null on success. */
    fun refreshDocs(): String? {
        return try {
            docs = readDocuments()
            null
        } catch (e: Throwable) {
            docs = emptyList()
            e.message ?: e.toString()
        }
    }

    private fun readDocuments(): List<Doc> {
        val arr = check(core.listDocuments()).getJSONArray("documents")
        return (0 until arr.length()).map {
            val d = arr.getJSONObject(it)
            val err = if (d.has("note_error")) d.optString("note_error", "erro desconhecido") else null
            Doc(d.getLong("id"), d.optString("number"), d.optString("title"), d.optString("note"), err)
        }
    }

    private fun check(json: String): JSONObject {
        val o = JSONObject(json)
        if (!o.optBoolean("ok")) throw CoreException(o.optString("error", "Erro desconhecido."))
        return o
    }

    // ------------------------------------------------------------ sync

    fun startSync(ctx: Context, uri: Uri, pass: String, note: Pair<Long, String>?): Boolean {
        val app = ctx.applicationContext
        setStatus("Sincronizando...", false)
        return submit { runSync(app, uri, pass, note) }
    }

    private fun readUri(ctx: Context, uri: Uri): ByteArray =
        ctx.contentResolver.openInputStream(uri)!!.use { it.readBytes() }

    private fun sha256(b: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(b).joinToString("") { "%02x".format(it) }

    private fun writeUri(ctx: Context, uri: Uri, data: ByteArray) {
        ctx.contentResolver.openOutputStream(uri, "wt")!!.use { s ->
            s.write(data)
            s.flush()
        }
    }

    private fun runSync(ctx: Context, uri: Uri, pass: String, note: Pair<Long, String>?) {
        val t0 = System.nanoTime()
        val cache = File(ctx.cacheDir, "sync-" + System.currentTimeMillis() + ".ponte")
        val pending = pendingFile(ctx)
        try {
            if (!ready) throw CoreException("O acervo ainda não está pronto.")
            if (note != null) check(core.saveNote(note.first, note.second))

            // a. copy the Drive file to the per-run cache and remember its hash
            val bytes = readUri(ctx, uri)
            val original = sha256(bytes)
            cache.writeBytes(bytes)

            // b. let the core merge; it may rewrite the cache file
            val r = check(core.sync(cache.absolutePath, if (pending.exists()) pending.absolutePath else "", pass))
            val summary = r.optString("summary")
            val conflicts = r.optInt("conflicts", 0)
            var message = "Sincronizado: $summary"
            var isError = false
            var size = bytes.size

            if (r.optBoolean("written")) {
                // c. the merged copy must survive a failed upload: keep it first
                cache.copyTo(pending, overwrite = true)
                if (sha256(readUri(ctx, uri)) != original) {
                    isError = true
                    message = "O arquivo mudou no Drive durante a sincronização. " +
                        "Sua alteração ficou guardada neste aparelho e vai na próxima sincronização."
                } else {
                    // d. write, then verify by reading back; one retry
                    val out = cache.readBytes()
                    val want = sha256(out)
                    var ok = false
                    for (attempt in 1..2) {
                        writeUri(ctx, uri, out)
                        if (sha256(readUri(ctx, uri)) == want) { ok = true; break }
                    }
                    if (ok) {
                        pending.delete()
                        size = out.size
                    } else {
                        isError = true
                        message = "A gravação no Drive não pôde ser conferida; a cópia ficou " +
                            "guardada neste aparelho e vai na próxima sincronização."
                    }
                }
            } else if (pending.exists()) {
                // f. nothing written although the pending copy was read too:
                // the Drive copy already holds everything it had (otherwise
                // the core would have rewritten it), so it is no longer needed.
                pending.delete()
            }

            // e. conflicts
            if (conflicts > 0) {
                isError = true
                message += "\n$conflicts conflito(s): a alteração do computador e a deste " +
                    "aparelho divergiram; confira no computador."
            }

            val listErr = refreshDocs()
            if (listErr != null) {
                isError = true
                message += "\n$listErr"
            }
            val secs = (System.nanoTime() - t0) / 1_000_000_000.0
            setStatus(message + String.format(" (%.1f s, %d bytes)", secs, size), isError)
        } finally {
            cache.delete()
        }
    }

    // ------------------------------------------------------------ library key (AndroidKeyStore)

    private fun prefsOf(ctx: Context) = ctx.getSharedPreferences("ponte", Context.MODE_PRIVATE)

    /** Returns the 32-byte library key, creating it on first run (wrapped by a Keystore AES key). */
    private fun loadOrCreateLibraryKey(ctx: Context): ByteArray {
        val prefs = prefsOf(ctx)
        val stored = prefs.getString("key_ct", null)
        val storedIv = prefs.getString("key_iv", null)
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

        if (stored != null && storedIv != null) {
            // Existing key: any failure here means the key cannot be opened (never regenerate).
            try {
                val master = ks.getKey(ALIAS, null) as SecretKey
                val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                cipher.init(
                    Cipher.DECRYPT_MODE, master,
                    GCMParameterSpec(128, Base64.decode(storedIv, Base64.NO_WRAP))
                )
                return cipher.doFinal(Base64.decode(stored, Base64.NO_WRAP))
            } catch (e: Throwable) {
                throw KeyUnavailableException(e)
            }
        }

        if (!ks.containsAlias(ALIAS)) {
            val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            gen.init(
                KeyGenParameterSpec.Builder(
                    ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setKeySize(256)
                    .build()
            )
            gen.generateKey()
        }
        val master = ks.getKey(ALIAS, null) as SecretKey
        // First run: random 32-byte library key, stored encrypted (ciphertext + IV).
        val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, master)
        val ct = cipher.doFinal(key)
        val saved = prefs.edit()
            .putString("key_ct", Base64.encodeToString(ct, Base64.NO_WRAP))
            .putString("key_iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
            .commit() // synchronous: the key must be on disk before the library is created
        if (!saved) throw CoreException("Não foi possível guardar a chave deste aparelho.")
        return key
    }
}

class MainActivity : Activity() {

    private val prefs by lazy { getSharedPreferences("ponte", Context.MODE_PRIVATE) }

    private var fileUri: Uri? = null

    private lateinit var fileInfo: TextView
    private lateinit var pendingInfo: TextView
    private lateinit var passphrase: EditText
    private lateinit var pickButton: Button
    private lateinit var syncButton: Button
    private lateinit var forgetButton: Button
    private lateinit var resetButton: Button
    private lateinit var status: TextView
    private lateinit var listView: ListView
    private val docAdapter = DocAdapter()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        try {
            buildUi()
            prefs.getString("uri", null)?.let {
                fileUri = Uri.parse(it)
                showFileInfo()
            }
            if (!SyncRunner.ready && !SyncRunner.keyBroken && !SyncRunner.running.get()) {
                SyncRunner.startInit(this, false)
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

    override fun onPause() {
        SyncRunner.listener = null
        super.onPause()
    }

    // ---------------------------------------------------------------- UI

    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()

    private fun buildUi() {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(16), dp(16), dp(16))
        }
        root.addView(TextView(this).apply { text = "Ponte de bolso"; textSize = 22f })

        pickButton = Button(this).apply {
            text = "Escolher o arquivo ponte-sync.ponte no Drive"
            setOnClickListener { pickFile() }
        }
        root.addView(pickButton)

        fileInfo = TextView(this).apply { text = "Nenhum arquivo escolhido." }
        root.addView(fileInfo)

        pendingInfo = TextView(this).apply {
            text = "Alterações guardadas aguardando envio"
            setTextColor(Color.rgb(0xB3, 0x6B, 0x00))
            visibility = View.GONE
        }
        root.addView(pendingInfo)

        passphrase = EditText(this).apply {
            hint = "Frase-senha"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            isSaveEnabled = false // never kept in saved instance state
            setText(SyncRunner.passphrase)
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun afterTextChanged(s: Editable?) {
                    SyncRunner.passphrase = s?.toString() ?: ""
                }
            })
        }
        root.addView(passphrase)

        forgetButton = Button(this).apply {
            text = "Esquecer frase-senha"
            textSize = 12f
            setOnClickListener {
                SyncRunner.passphrase = ""
                passphrase.setText("")
            }
        }
        root.addView(forgetButton)

        syncButton = Button(this).apply {
            text = "Sincronizar"
            setOnClickListener { startSync(null) }
        }
        root.addView(syncButton)

        resetButton = Button(this).apply {
            text = "Recomeçar o acervo deste aparelho"
            visibility = View.GONE
            setOnClickListener { confirmReset() }
        }
        root.addView(resetButton)

        status = TextView(this).apply { setPadding(0, dp(8), 0, dp(8)) }
        root.addView(status)

        listView = ListView(this).apply {
            adapter = docAdapter
            setOnItemClickListener { _, _, pos, _ ->
                SyncRunner.docs.getOrNull(pos)?.let { editNote(it) }
            }
        }
        root.addView(
            listView,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
        )

        setContentView(root)
    }

    /** Re-renders everything from the process-wide state (safe after rotation). */
    private fun render() {
        try {
            val busy = SyncRunner.running.get()
            pickButton.isEnabled = !busy
            syncButton.isEnabled = !busy && !SyncRunner.keyBroken
            resetButton.visibility = if (SyncRunner.keyBroken) View.VISIBLE else View.GONE
            resetButton.isEnabled = !busy
            status.text = SyncRunner.statusText
            status.setTextColor(
                if (SyncRunner.statusError) Color.RED
                else if (SyncRunner.statusText.startsWith("Sincronizado")) Color.rgb(0x1B, 0x7F, 0x2A)
                else Color.BLACK
            )
            pendingInfo.visibility =
                if (SyncRunner.pendingFile(this).exists()) View.VISIBLE else View.GONE
            docAdapter.notifyDataSetChanged()
        } catch (_: Throwable) {
        }
    }

    private fun confirmReset() {
        AlertDialog.Builder(this)
            .setTitle("Recomeçar o acervo deste aparelho")
            .setMessage(
                "Isso apaga o acervo local e a chave deste aparelho. Só se perdem as " +
                    "alterações que ainda não foram enviadas; todo o resto volta do Drive " +
                    "na próxima sincronização. Continuar?"
            )
            .setPositiveButton("Recomeçar") { _, _ ->
                if (!SyncRunner.startInit(this, true)) {
                    Toast.makeText(this, "Aguarde: há uma operação em andamento.", Toast.LENGTH_SHORT).show()
                }
                render()
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    private inner class DocAdapter : BaseAdapter() {
        override fun getCount() = SyncRunner.docs.size
        override fun getItem(position: Int) = SyncRunner.docs[position]
        override fun getItemId(position: Int) = SyncRunner.docs[position].id

        // Simple two-line row: "number — title" and a smaller note preview.
        override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
            val d = SyncRunner.docs[position]
            val row = (convertView as? LinearLayout) ?: LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(dp(4), dp(8), dp(4), dp(8))
                addView(TextView(context).apply { textSize = 16f })
                addView(TextView(context).apply {
                    textSize = 12f
                    setTextColor(Color.GRAY)
                    maxLines = 2
                })
            }
            (row.getChildAt(0) as TextView).text = d.number + " — " + d.title
            (row.getChildAt(1) as TextView).text =
                if (d.noteError != null) "(nota indisponível: " + d.noteError + ")" else d.note
            return row
        }
    }

    // ---------------------------------------------------------------- File picking

    private fun pickFile() {
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
            SyncRunner.setStatus(e.message ?: e.toString(), true)
            render()
        }
    }

    // Plain Activity: no AndroidX result API available, so the classic callback is used.
    @Deprecated("Classic result callback")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQ_PICK || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        try {
            contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            )
            prefs.edit().putString("uri", uri.toString()).apply()
            fileUri = uri
            showFileInfo()
        } catch (e: Throwable) {
            SyncRunner.setStatus(e.message ?: e.toString(), true)
            render()
        }
    }

    /** Queries the provider on a separate thread (it may hit the network) and posts the result. */
    private fun showFileInfo() {
        val uri = fileUri ?: return
        val resolver = contentResolver
        fileInfo.text = "Arquivo escolhido."
        Thread {
            val text = try {
                var t = "Arquivo escolhido."
                resolver.query(uri, null, null, null, null)?.use { c ->
                    if (c.moveToFirst()) {
                        val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        val s = c.getColumnIndex(OpenableColumns.SIZE)
                        val name = if (n >= 0) c.getString(n) else "(sem nome)"
                        val size = if (s >= 0 && !c.isNull(s)) c.getLong(s) else -1L
                        t = "Arquivo: " + name + (if (size >= 0) " (" + size + " bytes)" else "")
                    }
                }
                t
            } catch (e: Throwable) {
                "Arquivo escolhido (não foi possível ler os detalhes)."
            }
            runOnUiThread { try { fileInfo.text = text } catch (_: Throwable) { } }
        }.start()
    }

    // ---------------------------------------------------------------- Notes

    private fun editNote(doc: Doc) {
        if (doc.noteError != null) {
            Toast.makeText(this, "Nota indisponível: " + doc.noteError, Toast.LENGTH_LONG).show()
            return
        }
        val input = EditText(this).apply {
            setText(doc.note)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
            minLines = 4
            gravity = Gravity.TOP or Gravity.START
        }
        AlertDialog.Builder(this)
            .setTitle(doc.number)
            .setView(input)
            .setPositiveButton("Salvar e sincronizar") { _, _ ->
                startSync(Pair(doc.id, input.text.toString()))
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    // ---------------------------------------------------------------- Sync

    private fun startSync(note: Pair<Long, String>?) {
        try {
            val uri = fileUri
            if (uri == null) {
                SyncRunner.setStatus("Escolha primeiro o arquivo ponte-sync.ponte.", true)
                render()
                return
            }
            if (!SyncRunner.startSync(this, uri, SyncRunner.passphrase, note)) {
                Toast.makeText(this, "Já há uma sincronização em andamento.", Toast.LENGTH_SHORT).show()
            }
            render()
        } catch (e: Throwable) {
            SyncRunner.setStatus(e.message ?: e.toString(), true)
            render()
        }
    }

    companion object {
        private const val REQ_PICK = 1
    }
}
