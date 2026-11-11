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
import java.io.FileNotFoundException
import java.io.InputStream
import java.io.OutputStream
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
internal class CoreException(message: String) : PonteException(message)

/** Raised when the Keystore cannot decrypt the stored library key. */
internal class KeyUnavailableException(cause: Throwable?) : Exception("key unavailable", cause)

/**
 * Process-wide owner of the single worker, the "running" flag and the last visible state, so a
 * rotation (new Activity) neither starts a second sync nor loses the result.
 */
internal object SyncRunner {
    private const val PENDING = "pendente.ponte"
    private const val ALIAS = "ponte_master"
    private const val SYNC_FAILED = "Não foi possível sincronizar. Tente de novo; se continuar, avise quem cuida do Ponte."
    private const val OPEN_FAILED =
        "Não foi possível abrir o acervo deste aparelho. Feche e abra o Ponte de bolso; se continuar, avise quem cuida do Ponte."

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
    fun submit(fail: String = SYNC_FAILED, task: () -> Unit): Boolean {
        if (!running.compareAndSet(false, true)) return false
        changed()
        executor.execute {
            try {
                task()
            } catch (e: Throwable) {
                setStatus(PhoneWords.explain(e, fail, driveFile = true), true)
            } finally {
                running.set(false)
                changed()
            }
        }
        return true
    }

    /** Runs work on the shared worker (queued behind any sync) and delivers on the UI thread. */
    fun <T> background(
        work: () -> T, ok: (T) -> Unit, err: (String) -> Unit = {}, fail: String = PhoneWords.GENERIC
    ) {
        executor.execute {
            try {
                val r = work()
                main.post { try { ok(r) } catch (_: Throwable) { } }
            } catch (e: Throwable) {
                val m = PhoneWords.explain(e, fail)
                main.post { try { err(m) } catch (_: Throwable) { } }
            }
        }
    }

    /**
     * Like [background] but on a separate worker, so slow network calls never block the core.
     * [err] gets a sentence for the person, never the raw exception text.
     */
    fun <T> backgroundNetwork(
        work: () -> T, ok: (T) -> Unit, err: (String) -> Unit = {}, fail: String = PhoneWords.GENERIC
    ) {
        network.execute {
            try {
                val r = work()
                main.post { try { ok(r) } catch (_: Throwable) { } }
            } catch (e: Throwable) {
                val m = PhoneWords.explain(e, fail)
                main.post { try { err(m) } catch (_: Throwable) { } }
            }
        }
    }

    // ------------------------------------------------------------ init / reset

    fun startInit(ctx: Context, reset: Boolean): Boolean {
        val app = ctx.applicationContext
        setStatus("Preparando…", false)
        return submit(OPEN_FAILED) {
            try {
                if (reset) wipeLocalState(app)
                val key = loadOrCreateLibraryKey(app)
                val name = phoneMemberName()
                check(core.init(app.filesDir.absolutePath, key, name))
                ready = true
                keyBroken = false
                val e = refreshDocs()
                if (e == null) setStatus("Pronto.", false) else setStatus(e, true)
            } catch (e: KeyUnavailableException) {
                keyBroken = true
                setStatus("Não foi possível abrir o acervo deste aparelho.", true)
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
            PhoneWords.explain(e, "Não foi possível ler os documentos deste aparelho. Feche e abra o Ponte de bolso.")
        }
    }

    private fun readDocuments(): List<Doc> {
        val arr = check(core.listDocuments()).getJSONArray("documents")
        return (0 until arr.length()).map {
            val d = arr.getJSONObject(it)
            val err = if (d.has("note_error")) PhoneWords.adapt(d.optString("note_error", "Não foi possível ler a nota.")) else null
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
        if (!o.optBoolean("ok")) {
            // The one place where the core's texts reach the screen: worded for the phone here.
            throw CoreException(PhoneWords.adapt(o.optString("error", "Não foi possível concluir a operação.")))
        }
        return o
    }

    // ------------------------------------------------------------ sync

    fun startSync(ctx: Context, uri: Uri, pass: String, note: Pair<Long, String>?): Boolean {
        val app = ctx.applicationContext
        setStatus("Sincronizando…", false)
        return submit { runSync(app, uri, pass, note) }
    }

    /** Streams the Drive file into [to] (when given) while hashing it; the limit is enforced as it reads. */
    private fun readUri(ctx: Context, uri: Uri, to: File?, limit: Long = SyncIo.MAX_BYTES): SyncIo.Digest =
        (ctx.contentResolver.openInputStream(uri) ?: throw FileNotFoundException()).use { input ->
            if (to == null) SyncIo.copyHash(input, null, limit)
            else to.outputStream().use { out -> SyncIo.copyHash(input, out, limit) }
        }

    private fun writeUri(ctx: Context, uri: Uri, from: File) {
        (ctx.contentResolver.openOutputStream(uri, "wt") ?: throw FileNotFoundException()).use { s ->
            from.inputStream().use { SyncIo.copyHash(it, s, Long.MAX_VALUE) }
            s.flush()
        }
    }

    private fun runSync(ctx: Context, uri: Uri, pass: String, note: Pair<Long, String>?) {
        // Private working copy of the Drive file (already encrypted with the passphrase):
        // never in the cache folder, never in backups, removed on every path.
        val dir = ctx.noBackupFilesDir
        SyncIo.removeStale(dir)
        val work = File(dir, SyncIo.WORK_PREFIX + System.currentTimeMillis() + SyncIo.WORK_SUFFIX)
        val pending = pendingFile(ctx)
        SyncIo.withCleanup(listOf(work)) {
            if (!ready) throw CoreException("O acervo ainda não está pronto. Espere um instante e tente de novo.")
            if (note != null) check(core.saveNote(note.first, note.second))

            // a. copy the Drive file to the working file and remember its hash
            val original = readUri(ctx, uri, work)

            // b. let the core merge; it may rewrite the working file (atomically)
            val r = check(core.sync(work.absolutePath, if (pending.exists()) pending.absolutePath else "", pass))
            val summary = r.optString("summary")
            val conflicts = r.optInt("conflicts", 0)
            var message = "Sincronizado: " + PhoneWords.adapt(summary)
            var isError = false

            if (r.optBoolean("written")) {
                // c. the core has already taken what this phone had to send: the
                // merged copy must survive whatever happens next, so it is kept
                // before anything else can fail.
                SyncIo.keep(work, pending)
                // the core says how long the file it wrote is: anything else is not what it meant
                if (r.has("bytes") && r.optLong("bytes", -1) != work.length()) {
                    throw CoreException("O arquivo de sincronização não ficou completo. Tente sincronizar de novo.")
                }
                val want = SyncIo.hashFile(work)
                if (!SyncIo.unchanged(original, readUri(ctx, uri, null))) {
                    isError = true
                    message = "O arquivo mudou no Drive durante a sincronização. " +
                        "As alterações ficaram guardadas neste aparelho e vão na próxima sincronização."
                } else {
                    // d. write, then verify by reading back; one retry
                    var ok = false
                    for (attempt in 1..2) {
                        writeUri(ctx, uri, work)
                        // read back whatever was written, however long
                        if (SyncIo.unchanged(want, readUri(ctx, uri, null, Long.MAX_VALUE))) { ok = true; break }
                    }
                    if (ok) {
                        pending.delete()
                    } else {
                        isError = true
                        message = "Não foi possível conferir a gravação no Drive. " +
                            "As alterações ficaram guardadas neste aparelho e vão na próxima sincronização."
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
                // The summary already counts them ("2 conflitos de edição para resolver no computador").
                message += "\nA alteração feita no computador e a feita neste aparelho divergiram; confira no computador."
            }

            val listErr = refreshDocs()
            if (listErr != null) {
                isError = true
                message += "\n$listErr"
            }
            setStatus(message, isError)
        }
    }

    // ------------------------------------------------------------ library key (AndroidKeyStore)

    private fun prefsOf(ctx: Context) = ctx.getSharedPreferences("ponte", Context.MODE_PRIVATE)

    /**
     * The library key of an existing library, or null when this phone never made one. For what
     * only reads (the daily notice): it never creates a key, so it never starts a library.
     */
    fun libraryKeyIfPresent(ctx: Context): ByteArray? {
        val prefs = prefsOf(ctx)
        if (prefs.getString("key_ct", null) == null || prefs.getString("key_iv", null) == null) return null
        return loadOrCreateLibraryKey(ctx)
    }

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
        if (!saved) throw CoreException("Não foi possível preparar o acervo deste aparelho. Libere espaço no aparelho e tente de novo.")
        return key
    }
}

/**
 * The byte plumbing of a sync, free of Android types so plain JUnit can test it. Everything runs
 * on one fixed 64 KiB buffer: the transport file is never held in the Java heap.
 */
internal object SyncIo {
    /** Same limit as the core (512 MB). */
    const val MAX_BYTES: Long = 512L * 1024 * 1024
    const val WORK_PREFIX = "sync-work-"
    const val WORK_SUFFIX = ".ponte"
    const val TOO_BIG = "O arquivo de sincronização passa de 512 MB e não pode ser aberto."
    private const val BUFFER = 64 * 1024

    /** SHA-256 (lowercase hex) and length of a stream or file. */
    data class Digest(val sha256: String, val length: Long)

    /**
     * Reads [input] to its end, hashing it and copying it to [output] when given. Stops with
     * [CoreException] as soon as more than [limit] bytes have been read (the extra bytes are
     * never written).
     */
    fun copyHash(input: InputStream, output: OutputStream?, limit: Long): Digest {
        val md = MessageDigest.getInstance("SHA-256")
        val buf = ByteArray(BUFFER)
        var total = 0L
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            if (total + n > limit) throw CoreException(TOO_BIG)
            md.update(buf, 0, n)
            output?.write(buf, 0, n)
            total += n
        }
        output?.flush()
        return Digest(md.digest().joinToString("") { "%02x".format(it) }, total)
    }

    fun hashFile(file: File): Digest =
        file.inputStream().use { copyHash(it, null, Long.MAX_VALUE) }

    /** The Drive copy is the one that was read when both the hash and the length match. */
    fun unchanged(before: Digest, now: Digest): Boolean =
        before.sha256 == now.sha256 && before.length == now.length

    /** Runs [block] and deletes [files] afterwards, whatever happens (success, error, cancellation). */
    fun <T> withCleanup(files: List<File>, block: () -> T): T {
        try {
            return block()
        } finally {
            for (f in files) {
                try { f.delete() } catch (_: Throwable) { }
            }
        }
    }

    /**
     * Working files left by a process that died mid-sync, and the temporary
     * files the core leaves beside them when it dies while writing
     * (`.<name>.tmp-…`). Only the single worker calls this.
     */
    fun removeStale(dir: File) {
        dir.listFiles()?.forEach {
            val name = it.name
            if ((name.startsWith(WORK_PREFIX) && name.endsWith(WORK_SUFFIX)) || name.startsWith("." + WORK_PREFIX)) {
                try { it.delete() } catch (_: Throwable) { }
            }
        }
    }

    /**
     * Keeps [from] as [to] so that [to] is either the old file or the whole
     * new one, never a part: written beside it, forced to disk, then renamed.
     */
    fun keep(from: File, to: File) {
        val part = File(to.parentFile, to.name + ".parte")
        try {
            from.inputStream().use { input ->
                java.io.FileOutputStream(part).use { out ->
                    input.copyTo(out, BUFFER)
                    out.flush()
                    out.fd.sync()
                }
            }
            if (!part.renameTo(to)) {
                // Some file systems do not rename over an existing file.
                to.delete()
                if (!part.renameTo(to)) throw PonteException("Não foi possível guardar as alterações neste aparelho. Libere espaço e tente de novo.")
            }
        } finally {
            part.delete()
        }
    }
}
