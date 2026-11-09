package io.github.demusis.ponte

import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
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
    val noteError: String?,
    /** "Ofício", "Papel"… May be empty. */
    val type: String = "",
    /** dd/mm/aaaa, or empty. */
    val date: String = "",
    val origin: String = "",
    /** A record made by hand or by photo (not captured from the SIGADOC). */
    val manual: Boolean = false,
    /** Came from an e-mail message. */
    val email: Boolean = false,
    val emailFrom: String = "",
    /** How many tasks are open. */
    val openTasks: Int = 0,
    /** dd/mm/aaaa of the earliest open due date, or empty. */
    val taskDueLabel: String = "",
    val taskOverdue: Boolean = false,
    val taskDueToday: Boolean = false,
    /** How many attachments. */
    val attachments: Int = 0
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
    private val network = Executors.newSingleThreadExecutor()
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

    /** Runs work on the shared worker (queued behind any sync) and delivers on the UI thread. */
    fun <T> background(work: () -> T, ok: (T) -> Unit, err: (String) -> Unit = {}) {
        executor.execute {
            try {
                val r = work()
                main.post { try { ok(r) } catch (_: Throwable) { } }
            } catch (e: Throwable) {
                val m = e.message ?: e.toString()
                main.post { try { err(m) } catch (_: Throwable) { } }
            }
        }
    }

    /** Like [background] but on a separate worker, so slow network calls never block the core. */
    fun <T> backgroundNetwork(work: () -> T, ok: (T) -> Unit, err: (String) -> Unit = {}) {
        network.execute {
            try {
                val r = work()
                main.post { try { ok(r) } catch (_: Throwable) { } }
            } catch (e: Throwable) {
                val m = e.message ?: e.toString()
                main.post { try { err(m) } catch (_: Throwable) { } }
            }
        }
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
            Doc(
                d.getLong("id"), d.optString("number"), d.optString("title"), d.optString("note"), err,
                type = d.optString("type"), date = d.optString("date"), origin = d.optString("origin"),
                manual = d.optBoolean("manual"), email = d.optBoolean("email"),
                emailFrom = d.optString("email_from"), openTasks = d.optInt("open_tasks"),
                taskDueLabel = d.optString("task_due_label"), taskOverdue = d.optBoolean("task_overdue"),
                taskDueToday = d.optBoolean("task_due_today"), attachments = d.optInt("attachments")
            )
        }
    }

    fun check(json: String): JSONObject {
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

