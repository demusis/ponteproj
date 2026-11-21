package io.github.demusis.ponte

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.graphics.BitmapFactory
import androidx.lifecycle.Lifecycle
import android.system.Os
import android.webkit.MimeTypeMap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.json.JSONObject
import java.io.File
import java.io.FileNotFoundException
import java.io.InputStream
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.util.concurrent.Executors

/** How the "Guardar no Ponte" screen is working. */
internal enum class ShareMode { CHOOSE, RECORD, ATTACH }

/** What arrived from another app: a file in a private working copy, a text, or both. */
internal data class ShareItem(
    /** Clean display name; empty when only text came. */
    val name: String,
    val size: Long,
    val text: String,
    val subject: String,
    /** The private working file, or null for text only. */
    val path: String?,
    /** What the sender said the type is (only a hint); empty when unknown. */
    val mime: String = "",
    /** SHA-256 of the working file when it was made (to tell, after a restart, that it is intact); empty for text. */
    val hash: String = ""
) {
    val isFile get() = path != null
}

internal data class ShareState(
    /** The file is still being copied into the private working file. */
    val reading: Boolean = false,
    val item: ShareItem? = null,
    val mode: ShareMode = ShareMode.CHOOSE,
    val kinds: List<Choice> = listOf(Choice("outro", "Outro")),
    val kind: String = "outro",
    val title: String = "",
    val titleError: String = "",
    val date: String = "",
    val origin: String = "",
    val notes: String = "",
    /** The document the file will be attached to (mode ATTACH). */
    val target: Doc? = null,
    val description: String = "",
    val busy: Boolean = false,
    val progress: String = "",
    /** Shown over the footer; the item stays so the person can choose the other path or cancel. */
    val error: String = "",
    /** The draft came back after the app was closed ([ShareDraftRules.MSG_RECOVERED] on the form). */
    val recovered: Boolean = false,
    /** The draft could not be written to the disk: the form works, but a close of the app loses it. */
    val keepFailed: Boolean = false,
    /** A discreet note over the form (e.g. a restored save the core could not confirm); empty for none. */
    val note: String = ""
)

/**
 * Files and texts that come into the app from outside: the "Anexar…" of a ficha (a photo or a file
 * the person picks) and what another app shares ("Guardar no Ponte").
 *
 * Rules that hold everywhere here:
 *  - A file is copied, in a background thread and under a hard cap, into a private working file in
 *    its own folder ([DIR]); the core then reads that file, never the Uri.
 *  - Working files are deleted when the work ends (success or failure), when a share is cancelled,
 *    replaced or dropped, and all of them at a cold start.
 *  - The one exception is a share in progress ("Guardar no Ponte"): it is a DRAFT. The received item's
 *    copy lives in the private files folder ([ShareDraftStore.DIR]) and the form is written next to it
 *    (see [ShareDraft]), so a process death, or leaving the app to copy a number, loses nothing. The draft
 *    is removed when it is saved, cancelled, reset, or older than 7 days ([ShareDraftRules.MAX_AGE_MS]).
 *  - What arrives from other apps is untrusted: see [ShareRules] and [stage].
 *  - Everything runs on the UI thread except the copy, the core calls and the draft folder (one thread of its own).
 */
@Suppress("StaticFieldLeak") // Only the application context is retained.
internal object Intake {
    /** Folder of the working copies, inside the cache folder (not backed up, not served by the provider). */
    const val DIR = "entrada"

    /** The marker intent the trampoline sends to [MainActivity]; it carries no content (see [offer]). */
    const val ACTION_RECEIVED = "io.github.demusis.ponte.action.SHARE_RECEIVED"

    /** An image bigger than the limit is shrunk like a photo; it may be up to this size when it arrives. */
    private const val IMAGE_INPUT_CAP = 40L * 1024 * 1024
    private const val OFFER_LIFETIME_MS = 5L * 60 * 1000

    /** The draft is written this long after the last change (typing does not write at every key). */
    private const val SAVE_DELAY_MS = 700L

    @Suppress("StaticFieldLeak") // Assigned ctx.applicationContext in init.
    private lateinit var app: Context
    private val main = Handler(Looper.getMainLooper())

    /** ONE worker for every copy and decode: they run one after the other, never in parallel. */
    private val stager = Executors.newSingleThreadExecutor { r -> Thread(r, "ponte-intake").apply { isDaemon = true } }

    /** The draft folder, inside the app's private files (never the cache, never external storage). */
    private lateinit var store: ShareDraftStore

    fun init(ctx: Context) {
        app = ctx.applicationContext
        if (!::store.isInitialized) {
            store = ShareDraftStore(File(app.filesDir, ShareDraftStore.DIR))
            writer = DraftWriter(store, writerExec)
        }
    }

    private fun ui(block: () -> Unit) {
        main.post { try { block() } catch (_: Throwable) { } }
    }

    private fun dir(): File = File(app.cacheDir, DIR).apply { mkdirs() }

    /** A new, uniquely named empty file in the private folder (the system makes the name, so no two collide). */
    private fun workFile(prefix: String, durable: Boolean = false): File =
        if (durable) store.newCopy(prefix) else File.createTempFile(prefix + "-", ".tmp", dir())

    /**
     * Cold start: the working files of an earlier run (attachments being prepared) are not kept. A share in
     * progress is not among them: its copy lives in the draft folder, which this never looks at, and
     * whatever path a draft points to is never deleted here even if it were found in this folder.
     */
    fun cleanAll(ctx: Context) {
        try {
            val keep = livePaths()
            File(ctx.cacheDir, DIR).listFiles()?.forEach {
                if (it.isFile && ShareDraftRules.mayDelete(it.absolutePath, keep)) it.delete()
            }
        } catch (_: Throwable) {
        }
    }

    /** The copy a share in progress points to (nothing to delete or clean may touch it). */
    private fun livePaths(): Set<String> = setOfNotNull(share.item?.path)

    /** Camera pictures taken for an "Anexar…" that must survive a lock while the camera is in front, and the share's copy. */
    fun keepPaths(): Set<String> =
        setOfNotNull(attachPhoto?.absolutePath, resizing?.absolutePath) + livePaths()

    // ------------------------------------------------------------------ reading a Uri

    private class Staged(val file: File, val name: String, val size: Long)

    private fun ownDirs(): List<String> {
        val out = LinkedHashSet<String>()
        fun add(f: File?) {
            if (f == null) return
            out.add(f.absolutePath)
            try { out.add(f.canonicalPath) } catch (_: Throwable) { }
        }
        add(File(app.applicationInfo.dataDir))
        add(app.filesDir); add(app.cacheDir); add(app.noBackupFilesDir); add(app.codeCacheDir)
        add(app.getExternalFilesDir(null)); add(app.externalCacheDir)
        app.filesDir.parentFile?.let { add(it) }
        return out.toList()
    }

    /**
     * Opens what the sender granted. A provider that gives a real file whose path is inside this
     * app's own folders is refused (it would be handing out our private files). Providers that
     * answer only with a stream are read as a stream.
     */
    private fun openForRead(uri: Uri): InputStream {
        val resolver = app.contentResolver
        val pfd = try {
            resolver.openFileDescriptor(uri, "r")
        } catch (_: FileNotFoundException) {
            null
        } catch (_: UnsupportedOperationException) {
            null
        }
        if (pfd != null) {
            val real = try { Os.readlink("/proc/self/fd/" + pfd.fd) } catch (_: Throwable) { null }
            if (ShareRules.insideDirs(real, ownDirs())) {
                try { pfd.close() } catch (_: Throwable) { }
                throw PonteException(ShareRules.MSG_OWN_FILE)
            }
            return ParcelFileDescriptor.AutoCloseInputStream(pfd)
        }
        return resolver.openInputStream(uri) ?: throw PonteException(ShareRules.MSG_NOT_CONTENT)
    }

    private fun extFor(mime: String): String? = MimeTypeMap.getSingleton().getExtensionFromMimeType(mime)

    /**
     * Copies the file behind [uri] into a private working file (blocking: call from a thread of
     * our own). [allowBigImage]: an image the sender says is an image may arrive up to
     * [IMAGE_INPUT_CAP] and is then shrunk to a JPEG under the attachment limit; everything else
     * stops at the core's limit (the copy never reads past it). The name comes from the provider,
     * is cleaned, and gets the extension of the MIME hint when it has none. The type of the content
     * is not judged here: the core does it when the file is stored.
     */
    private fun stage(
        uri: Uri, mimeHint: String?, allowBigImage: Boolean, cancelled: () -> Boolean = { false },
        durable: Boolean = false
    ): Staged {
        val auth = uri.authority
        if (!"content".equals(uri.scheme, ignoreCase = true) || auth.isNullOrBlank()) {
            throw PonteException(ShareRules.MSG_NOT_CONTENT)
        }
        if (ShareRules.isOwnAuthority(auth, PonteFiles.AUTHORITY)) throw PonteException(ShareRules.MSG_OWN_FILE)
        val resolver = app.contentResolver
        val mime = mimeHint?.takeIf { ShareRules.plausibleMime(it) } ?: try { resolver.getType(uri) } catch (_: Throwable) { null }
        var rawName: String? = null
        var declared = -1L
        try {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
                if (c.moveToFirst()) {
                    val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    val s = c.getColumnIndex(OpenableColumns.SIZE)
                    if (n >= 0 && !c.isNull(n)) rawName = c.getString(n)
                    if (s >= 0 && !c.isNull(s)) declared = c.getLong(s)
                }
            }
        } catch (_: Throwable) {
        }
        val limit = AttachLimits.maxBytes
        var name = ShareRules.cleanName(
            ShareRules.withExtension(ShareRules.cleanName(rawName, AttachLimits.maxNameChars), mime, ::extFor),
            AttachLimits.maxNameChars
        )
        val image = ShareRules.looksLikeImage(mime)
        val big = image && allowBigImage
        val cap = if (big) IMAGE_INPUT_CAP else limit
        val tooBig = if (big) "A imagem passa de ${IMAGE_INPUT_CAP / (1024 * 1024)} MB. Compartilhe uma versão menor."
        else ShareRules.tooBigMessage(name, limit)
        // The size the provider announces is only a hint: it lets an obvious case fail early.
        if (declared > cap) throw PonteException(tooBig)

        val work = workFile("in", durable)
        var done = false
        try {
            try {
                openForRead(uri).use { input -> work.outputStream().use { out -> BoundedCopy.copy(input, out, cap, cancelled) } }
            } catch (_: TooBigException) {
                throw PonteException(tooBig)
            }
            var file = work
            // Re-encoded to JPEG when over the limit, or when what the decoder really finds in it is a
            // type the core does not take (HEIC, WebP...): by content, never by the sender's label.
            if (image && ShareRules.needsReencode(realImageType(work), work.length(), limit)) {
                val shrunk = workFile("img", durable)
                try {
                    Images.resizeForUpload(work, shrunk)
                } catch (e: OutOfMemoryError) {
                    shrunk.delete()
                    throw e
                } catch (e: Throwable) {
                    shrunk.delete()
                    throw PonteException(ShareRules.MSG_CONVERT)
                }
                work.delete()
                file = shrunk
                name = ShareRules.replaceExtension(name, "jpg")
            }
            done = true
            return Staged(file, name, file.length())
        } finally {
            if (!done) work.delete()
        }
    }

    /** The type the decoder finds in the content (reads only the header), or null when it is not an image it knows. */
    private fun realImageType(f: File): String? = try {
        val o = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(f.absolutePath, o)
        if (o.outWidth > 0 && o.outHeight > 0) o.outMimeType else null
    } catch (_: Throwable) {
        null
    }

    // ------------------------------------------------------------------ "Anexar…" in the ficha

    /** True while a file is being read or attached from a ficha (the button waits). */
    var attachBusy by mutableStateOf(false)
        private set

    private var attachDocId = 0L
    private var attachDescription = ""
    private var attachPhoto: File? = null

    private fun cleanDescription(d: String) = ShareRules.capChars(d.trim(), AttachLimits.maxDescriptionChars)

    /** The camera, for a picture to attach to document [documentId]. */
    fun attachPhoto(documentId: Long, description: String) {
        if (attachBusy || Controller.locked || documentId <= 0) return
        val a = MainActivity.live ?: return
        val dir = File(app.cacheDir, PonteFiles.DIR_PHOTOS).apply { mkdirs() }
        val f = File(dir, "anexo-" + System.currentTimeMillis() + ".jpg")
        val uri = PonteFiles.uriFor(PonteFiles.DIR_PHOTOS, f.name)
        val i = Intent(MediaStore.ACTION_IMAGE_CAPTURE).apply {
            putExtra(MediaStore.EXTRA_OUTPUT, uri)
            addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
            clipData = ClipData.newRawUri("foto", uri)
        }
        attachDocId = documentId
        attachDescription = cleanDescription(description)
        attachPhoto = f
        try {
            @Suppress("DEPRECATION")
            a.startActivityForResult(i, MainActivity.REQ_ATTACH_PHOTO)
        } catch (_: ActivityNotFoundException) {
            attachPhoto = null
            Controller.say("Este aparelho não tem aplicativo de câmera.", long = true)
        } catch (_: Throwable) {
            attachPhoto = null
            Controller.say("Não foi possível abrir a câmera. Tente de novo.", long = true)
        }
    }

    /** A result of the camera or the picker, already in a private working file, waiting for the unlock. */
    private class Parked(val docId: Long, val file: File, val name: String, val description: String)

    private var parked: Parked? = null

    /** The camera picture being shrunk (kept out of the lock's cleanup meanwhile). */
    @Volatile private var resizing: File? = null

    /**
     * The camera answered. The picture is shrunk to a private working file at once, even if the app
     * locked while the camera was open; then it is attached, or, while locked, PARKED until the
     * person unlocks ([onUnlocked]). After a process death the state is gone and the picture is
     * just deleted (the person takes it again).
     */
    fun onAttachPhotoResult(ok: Boolean) {
        val f = attachPhoto ?: return
        attachPhoto = null
        if (IntakeRules.attachResult(ok, f.isFile && f.length() > 0L, locked = false) == IntakeRules.Result.DROP) {
            f.delete()
            return
        }
        val docId = attachDocId
        val description = attachDescription.ifEmpty { "Foto tirada no celular" }
        attachDocId = 0L
        attachBusy = true
        resizing = f
        stager.execute {
            val out = workFile("foto")
            try {
                Images.resizeForUpload(f, out)
                val name = "foto-" + LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd-HHmmss")) + ".jpg"
                f.delete()
                ui { resizing = null; deliver(docId, out, name, description) }
            } catch (e: Throwable) {
                f.delete()
                out.delete()
                val m = PhoneWords.explain(e, "Não foi possível guardar a foto. Tente de novo.")
                ui { resizing = null; attachBusy = false; Controller.say(m, long = true) }
            }
        }
    }

    /**
     * A working file is ready for [docId]: attach it now, or park it while the app is locked (and
     * still in front of the person; a locked app that is already out of sight keeps nothing).
     */
    private fun deliver(docId: Long, file: File, name: String, description: String) {
        when (IntakeRules.attachResult(true, true, Controller.locked)) {
            IntakeRules.Result.ATTACH -> runAttach(docId, file, name, description)
            IntakeRules.Result.PARK -> {
                val front = MainActivity.live?.lifecycle?.currentState?.isAtLeast(Lifecycle.State.STARTED) == true
                if (front) {
                    parked?.file?.delete()
                    parked = Parked(docId, file, name, description)
                } else {
                    file.delete()
                    attachBusy = false
                }
            }
            IntakeRules.Result.DROP -> { file.delete(); attachBusy = false }
        }
    }

    /** From the compose tree once the app is unlocked: what waited behind the lock is attached now. */
    fun onUnlocked() {
        val p = parked ?: return
        parked = null
        runAttach(p.docId, p.file, p.name, p.description)
    }

    /** The system picker (any type, one file), for a file to attach to document [documentId]. */
    fun attachPick(documentId: Long, description: String) {
        if (attachBusy || Controller.locked || documentId <= 0) return
        val a = MainActivity.live ?: return
        attachDocId = documentId
        attachDescription = cleanDescription(description)
        try {
            val i = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "*/*"
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, false)
            }
            @Suppress("DEPRECATION")
            a.startActivityForResult(i, MainActivity.REQ_ATTACH_FILE)
        } catch (_: Throwable) {
            Controller.say("Não foi possível abrir a lista de arquivos. Tente de novo.", long = true)
        }
    }

    /**
     * The picker answered with [uri] (null when the person gave up). The file is copied at once,
     * with the grant it came with, even if the app locked meanwhile; then attached, or parked
     * while locked.
     */
    fun onAttachPicked(uri: Uri?) {
        if (uri == null || attachDocId <= 0 || attachBusy) return
        val docId = attachDocId
        val description = attachDescription
        attachDocId = 0L
        attachBusy = true
        stager.execute {
            try {
                val s = stage(uri, null, allowBigImage = false)
                ui { deliver(docId, s.file, s.name, description) }
            } catch (e: Throwable) {
                val m = PhoneWords.explain(e, "Não foi possível ler o arquivo. Tente de novo.")
                ui { attachBusy = false; Controller.say(m, long = true) }
            }
        }
    }

    /** Hands the working file to the core; it is deleted on every path. */
    private fun runAttach(docId: Long, work: File, name: String, description: String) {
        SyncRunner.background({
            try {
                Api.attachFile(docId, work.absolutePath, name, description)
                SyncRunner.refreshDocs()
            } finally {
                try { work.delete() } catch (_: Throwable) { }
            }
        }, {
            attachBusy = false
            Controller.render()
            Controller.refreshDocument()
            Controller.say("Anexo adicionado.")
            Controller.requestSync(null, auto = true)
        }, {
            attachBusy = false
            Controller.say(it, long = true)
        }, fail = "Não foi possível anexar o arquivo. Tente de novo.")
    }

    // ------------------------------------------------------------------ what other apps share

    /** What the trampoline validated and read from the sender's intent; nothing else crosses to [MainActivity]. */
    class Offer(
        val action: String?, val mime: String?, val uri: Uri?, val streamCount: Int,
        val text: String, val subject: String, val madeAt: Long = System.currentTimeMillis()
    )

    @Volatile private var offered: Offer? = null

    /**
     * Left in memory by the trampoline for [MainActivity], which takes it once. The intent that
     * brings the app forward is only a marker: whoever else sends that marker finds no offer.
     */
    fun offer(o: Offer) {
        offered = o
    }

    private fun takeOffer(): Offer? {
        val o = offered
        offered = null
        return if (o != null && System.currentTimeMillis() - o.madeAt <= OFFER_LIFETIME_MS) o else null
    }

    var share by mutableStateOf(ShareState())
        private set

    /** A generation: grows whenever the pending share is replaced or dropped, so late work discards itself. */
    @Volatile private var gen = 0

    /** The person chose a kind by hand: the kinds arriving later (see [onKinds]) must not change it. */
    private var kindTouched = false

    /** The "Guardar no Ponte" screen has been shown (unlocked) at least once. */
    private var seen = false

    /** What to tell once the app is unlocked (a refusal that came while it was locked). */
    private var pendingNotice: String? = null

    /** A share is on the stack: being received, a form being filled, or a draft that came back (see [restoreDraft]). */
    fun hasShare() = Route.Share in Controller.stack

    private fun tell(message: String) {
        if (Controller.locked) pendingNotice = message else Controller.say(message, long = true)
    }

    /** From the compose tree, when the app is unlocked: says what waited behind the lock. */
    fun flushNotice() {
        val m = pendingNotice ?: return
        pendingNotice = null
        Controller.say(m, long = true)
    }

    /** The screen was composed: it counts as seen, so a later lock keeps the share waiting. */
    fun markSeen() {
        seen = true
    }

    // ------------------------------------------------------------------ the draft on disk
    //
    // A share with an item is a DRAFT: [snapshot] is written (atomically) to the draft folder shortly after
    // every change, when the app goes to the background, and before the core is asked to save. It is
    // removed when saved, cancelled, reset, or after 7 days. At start, [restoreDraft] puts it back.
    //
    // Every touch of the draft folder goes through [writer], one job at a time, in the order asked. A snapshot
    // takes the next revision on the UI thread and the store refuses a lower one than it has written, so an
    // older snapshot never lands over a newer one (the save marker above all).

    private val saveTick = Runnable { flushAsync() }

    /** The ONE thread of the draft folder: writes, removals and the restore at start run here, in order. */
    private val writerExec = Executors.newSingleThreadExecutor { r -> Thread(r, "ponte-draft").apply { isDaemon = true } }
    private lateinit var writer: DraftWriter

    /** When the share arrived (the 7 days count from here). */
    private var draftCreatedAt = 0L

    /** The save marker: set before the core is called, cleared when the save ends (see [DraftSaving]). */
    private var saving: DraftSaving? = null

    /** The operation id the last save of this form used (the same form saved again reuses it). */
    private var pending: ShareDraftRules.PendingOp? = null

    /** A draft written by a newer app could not be put aside: nothing of ours is written over it. */
    private var draftBlocked = false

    /** The last revision handed out (UI thread only). */
    private var rev = 0L

    /** The draft is being read from the disk (off the UI thread): a share that arrives meanwhile waits for it. */
    private var restoring = false
    private var deferredReceive = false

    /** Grows when the data is reset, so a restore that was still running discards itself. */
    private var restoreToken = 0

    private fun snapshot(): ShareDraft? {
        val s = share
        val item = s.item ?: return null
        if (s.reading) return null
        val now = System.currentTimeMillis()
        if (draftCreatedAt <= 0L) draftCreatedAt = now
        return ShareDraft(
            createdAt = draftCreatedAt, updatedAt = now,
            copy = item.path?.let { File(it).name } ?: "", name = item.name, size = item.size, mime = item.mime,
            hash = item.hash, text = item.text, subject = item.subject, mode = s.mode.name, kind = s.kind,
            title = s.title, date = s.date, origin = s.origin, notes = s.notes,
            targetId = s.target?.id ?: 0L, targetNumber = s.target?.number ?: "", targetTitle = s.target?.title ?: "",
            description = s.description, saving = saving
        )
    }

    /**
     * The snapshot of now, with the next revision, queued for writing: all on the UI thread, so the order of the
     * queue is the order of the revisions. The form is told how it came out (see [reportWrite]). Null when there
     * is nothing to write or the slot is blocked.
     */
    private fun enqueue(): java.util.concurrent.Future<ShareDraftStore.Wrote>? {
        val mine = gen
        if (draftBlocked) {
            reportWrite(ShareDraftStore.Wrote.FAILED)
            return null
        }
        val d = snapshot() ?: return null
        return writer.write(d, ++rev) { w -> ui { if (gen == mine) reportWrite(w) } }
    }

    /** The item is ready: the draft is born (queued at once, so even an immediate kill finds it). */
    private fun startDraft() {
        draftCreatedAt = System.currentTimeMillis()
        saving = null
        pending = null
        flushNow()
    }

    /** A field changed: the draft is written a moment after the last change. */
    private fun scheduleSave() {
        if (share.item == null || share.reading) return
        main.removeCallbacks(saveTick)
        main.postDelayed(saveTick, SAVE_DELAY_MS)
    }

    /** Queued now (and the pending debounce is no longer needed). */
    private fun flushNow(): java.util.concurrent.Future<ShareDraftStore.Wrote>? {
        main.removeCallbacks(saveTick)
        return enqueue()
    }

    private fun flushAsync() {
        enqueue()
    }

    /** The form is told whether the draft is being kept: a discreet warning when the disk refuses it. */
    private fun reportWrite(w: ShareDraftStore.Wrote) {
        if (share.item == null) return
        when (w) {
            ShareDraftStore.Wrote.FAILED -> if (!share.keepFailed) share = share.copy(keepFailed = true)
            ShareDraftStore.Wrote.OK -> if (share.keepFailed) share = share.copy(keepFailed = false)
            ShareDraftStore.Wrote.STALE -> {}
        }
    }

    /**
     * The draft file is gone, with every pending write (the copy [copyName] goes right after it, in the same
     * queue: a write asked earlier never comes back after this).
     */
    private fun forgetDraft(copyName: String?) {
        main.removeCallbacks(saveTick)
        saving = null
        pending = null
        draftCreatedAt = 0L
        if (!draftBlocked) writer.clear(copyName)
        else if (!copyName.isNullOrEmpty() && ShareDraftRules.safeName(copyName)) {
            try { File(store.path(copyName)).delete() } catch (_: Throwable) { }
        }
    }

    /**
     * The marker (with the operation id) is queued before the core is called: if the process dies now, the
     * restored draft asks the core about that very operation. The result is the write to wait for, or null when
     * no marker can be kept (the slot is blocked: there is no draft to come back to either).
     */
    private fun beginSaving(op: ShareDraftRules.PendingOp): java.util.concurrent.Future<ShareDraftStore.Wrote>? {
        saving = DraftSaving(op.op, op.id, System.currentTimeMillis())
        return flushNow()
    }

    /** The core is called only once the marker is on the disk (see [ShareDraftRules.callAfterMarker]). */
    private fun markerGate(marker: java.util.concurrent.Future<ShareDraftStore.Wrote>?, needed: Boolean): () -> Boolean =
        { !needed || (marker != null && ShareDraftRules.durable(marker)) }

    /** The save failed: the draft is a form again (the operation id stays, to be reused by the same form). */
    private fun endSaving() {
        saving = null
        flushNow()
    }

    /**
     * At a cold start: asks for the share that was in progress when the app closed, with every field. The
     * draft is read, checked and its copy hashed on the draft thread, never on the Activity's: the lock
     * screen or the home shows meanwhile, and the share route appears when the answer is ready. The lock
     * comes first by itself: the route waits on the stack and nothing of it is composed (or said) until the
     * app is unlocked. A draft that is too old, or whose copy is gone or changed, is removed and the person is
     * told once ([tell] waits for the unlock). Never throws: whatever is wrong means no draft.
     */
    fun restoreDraft() {
        if (share.item != null || share.reading || restoring) return
        restoring = true
        val token = restoreToken
        val setUp = Controller.isSetUp()
        val now = System.currentTimeMillis()
        try {
            writer.submit {
                val r = try { store.start(now, setUp) } catch (_: Throwable) { ShareDraftStore.Start.None }
                ui { restored(r, token) }
            }
        } catch (_: Throwable) {
            restoring = false
        }
    }

    private fun restored(s: ShareDraftStore.Start, token: Int) {
        restoring = false
        try {
            if (token == restoreToken && share.item == null && !share.reading) {
                when (s) {
                    ShareDraftStore.Start.None -> {}
                    ShareDraftStore.Start.Blocked -> draftBlocked = true
                    is ShareDraftStore.Start.Dropped -> s.message?.let { tell(it) }
                    is ShareDraftStore.Start.Resume -> adopt(s.draft)
                }
            }
        } catch (_: Throwable) {
        }
        Controller.render()
        // A share that came while the draft was being read now meets it (and is refused if there is one).
        if (deferredReceive) {
            deferredReceive = false
            receive()
        }
    }

    private fun adopt(d: ShareDraft) {
        gen++
        val mine = gen
        val item = ShareItem(
            d.name, d.size, d.text, d.subject, if (d.copy.isEmpty()) null else store.path(d.copy), d.mime, d.hash
        )
        val mode = ShareMode.valueOf(ShareDraftRules.normalMode(d.mode, item.isFile, d.targetId > 0))
        val target = if (d.targetId > 0) Doc(d.targetId, d.targetNumber, d.targetTitle, "", null) else null
        // A restored form counts as seen: it waits behind a lock like any form in progress, and is not dropped
        // by the rule for a share nobody has looked at.
        seen = true
        kindTouched = true
        draftCreatedAt = d.createdAt
        saving = d.saving
        pending = null
        val checkFirst = d.saving != null
        share = ShareState(
            item = item, mode = mode, kinds = Controller.photo.kinds, kind = d.kind.ifEmpty { "outro" },
            title = d.title, date = d.date, origin = d.origin, notes = d.notes, target = target,
            description = d.description, recovered = true,
            busy = checkFirst, progress = if (checkFirst) ShareDraftRules.MSG_CHECKING else ""
        )
        if (Route.Share !in Controller.stack) Controller.stack.add(Route.Share)
        if (checkFirst || d.targetId > 0) verifyRestored(d, mine)
    }

    /** What the core says about a restored draft (see [verifyRestored]). */
    private class Checked(val status: ShareDraftRules.OperationStatus?, val targetNumber: String?)

    /**
     * A restored draft is checked against the library, off the UI thread. One that was saving: the core is asked
     * whether ITS operation (the id in the marker) was committed, and only a "yes" ends the draft. And the target
     * document: when its number is no longer the one the form shows, the form must not keep it.
     */
    private fun verifyRestored(d: ShareDraft, mine: Int) {
        SyncRunner.background({
            val sv = d.saving
            val st = if (sv != null && sv.usable) Api.operationStatus(sv.operationId) else null
            val tn = if (d.targetId > 0 && d.targetNumber.isNotEmpty() && st !is ShareDraftRules.OperationStatus.Done) {
                try { Api.document(d.targetId).s("number") } catch (_: Throwable) { null }
            } else null
            Checked(st, tn)
        }, { c ->
            if (gen == mine) applyChecked(d, c)
        }, {
            // The check itself could not run: nothing is known, nothing is lost.
            if (gen == mine) applyChecked(d, Checked(ShareDraftRules.OperationStatus.Unknown, null))
        }, fail = ShareDraftRules.MSG_CANNOT_CHECK)
    }

    private fun applyChecked(d: ShareDraft, c: Checked) {
        when (val rec = ShareDraftRules.recover(d, c.status)) {
            is ShareDraftRules.Recovery.Saved -> {
                dropShare(removeRoute = true)
                finished(rec.documentId, ShareDraftRules.MSG_SAVED_BEFORE)
                return
            }
            is ShareDraftRules.Recovery.Form -> {
                saving = if (rec.keepMarker) d.saving else null
                val sv = d.saving
                pending = if (sv != null && rec.reuseId != null) {
                    ShareDraftRules.PendingOp(sv.op, rec.reuseId, ShareDraftRules.fingerprint(d, sv.op))
                } else null
                share = share.copy(
                    busy = false, progress = "",
                    note = if (rec.unconfirmed) ShareDraftRules.MSG_CANNOT_CHECK else ""
                )
            }
        }
        // The target is still the document the form shows? Not when the library changed under it (a sync).
        val shown = share.target
        if (shown != null && shown.id == d.targetId &&
            ShareDraftRules.targetCheck(d.targetNumber, c.targetNumber) == ShareDraftRules.TargetCheck.CHANGED
        ) {
            pending = null
            share = share.copy(
                target = null, mode = if (share.mode == ShareMode.ATTACH) ShareMode.CHOOSE else share.mode,
                error = ShareDraftRules.MSG_TARGET_CHANGED
            )
        }
        flushNow()
    }

    /** "Recomeçar": the draft and its copy go with the rest of the phone's data. */
    fun wipeForReset() {
        gen++
        restoreToken++
        main.removeCallbacks(saveTick)
        saving = null
        pending = null
        draftCreatedAt = 0L
        draftBlocked = false
        share = ShareState()
        Controller.stack.removeAll { it == Route.Share || it == Route.SharePick }
        // In the draft queue, behind every write asked before it (a late write never brings the draft back).
        writer.submit { store.wipe() }
    }

    /**
     * An offer came in through [MainActivity]. Order: the checks that need no content; "not set up"
     * (told, nothing read); the file is copied under the cap; and the screen is put on the
     * stack at once, so behind a lock it simply waits there (nothing of it is composed while locked).
     */
    fun receive() {
        // The draft of an earlier run is still being read: whether it exists decides what a new share meets
        // (one share at a time), so the offer waits (it stays where it is) and is taken when the read ends.
        if (restoring) {
            deferredReceive = true
            return
        }
        val o = takeOffer() ?: return
        val verdict = ShareRules.check(
            o.action, o.mime, o.uri?.scheme, o.uri?.authority, o.streamCount,
            o.text.isNotEmpty(), PonteFiles.AUTHORITY
        )
        if (!verdict.ok) { tell(verdict.message); return }
        if (!Controller.isSetUp()) { tell(ShareRules.MSG_NOT_SET_UP); return }
        // One share at a time: while one is being received or is on the screen (a form being filled,
        // a save running, a draft that came back after a restart) the first is kept untouched and the
        // person is told. Nothing of the second is read. Only a share whose copy failed (nothing to
        // keep) is replaced.
        if (IntakeRules.arrival(share.reading, share.item != null) == IntakeRules.Arrival.KEEP_FIRST) {
            tell(ShareRules.MSG_KEEP_FIRST)
            return
        }
        dropShare(removeRoute = true)
        gen++
        seen = false
        kindTouched = false
        val mine = gen
        val kinds = Controller.photo.kinds
        val kind = ShareRules.defaultKind(kinds.map { it.key })
        val subject = ShareRules.oneLine(o.subject, AttachLimits.maxTitleBytes)
        val text = ShareRules.plainText(o.text, AttachLimits.maxTextBytes)
        val uri = o.uri
        if (uri == null) {
            val title = subject.ifEmpty { ShareRules.oneLine(text.lineSequence().firstOrNull { it.isNotBlank() }, 120) }
            share = ShareState(item = ShareItem("", 0, text, subject, null, o.mime.orEmpty()), kinds = kinds, kind = kind, title = title)
            Controller.stack.add(Route.Share)
            startDraft()
            return
        }
        share = ShareState(reading = true, kinds = kinds, kind = kind)
        Controller.stack.add(Route.Share)
        stager.execute {
            try {
                // The copy is made in the draft folder: it must outlive the process while the form is open.
                val s = stage(uri, o.mime, allowBigImage = true, cancelled = { IntakeRules.isLate(gen, mine) }, durable = true)
                val hash = try { ShareDraftStore.hashOf(s.file) } catch (_: Throwable) { "" }
                val mime = o.mime?.takeIf { ShareRules.plausibleMime(it) }.orEmpty()
                ui {
                    if (!IntakeRules.isLate(gen, mine)) {
                        val title = subject.ifEmpty { ShareRules.oneLine(ShareRules.titleFromName(s.name), AttachLimits.maxTitleBytes) }
                        share = share.copy(
                            reading = false, error = "",
                            item = ShareItem(s.name, s.size, text, subject, s.file.absolutePath, mime, hash),
                            title = title
                        )
                        startDraft()
                    } else s.file.delete()
                }
            } catch (_: CopyCancelled) {
                // A newer share or a cancel replaced this one: its file was already deleted.
            } catch (e: Throwable) {
                val m = PhoneWords.explain(e, "Não foi possível receber o arquivo. Compartilhe-o de novo.")
                ui {
                    if (!IntakeRules.isLate(gen, mine)) share = share.copy(reading = false, error = m)
                }
            }
        }
    }

    /** The manual kinds arrived (the library's choices load after the app starts): the form uses them. */
    fun onKinds(kinds: List<Choice>) {
        if (kinds.isEmpty() || share.item == null && !share.reading) return
        val keep = kindTouched && kinds.any { it.key == share.kind }
        val before = share.kind
        share = share.copy(kinds = kinds, kind = if (keep) share.kind else ShareRules.defaultKind(kinds.map { it.key }))
        if (share.kind != before) scheduleSave()
    }

    /**
     * Deletes the pending share's draft (first) and its working file, and forgets it; optionally takes its
     * screens off the stack. This is every way a draft ends: saved, cancelled, replaced, never seen.
     */
    private fun dropShare(removeRoute: Boolean) {
        gen++
        forgetDraft(share.item?.path?.let { File(it).name })
        share = ShareState()
        if (removeRoute) Controller.stack.removeAll { it == Route.Share || it == Route.SharePick }
    }

    /** "Cancelar", or Back at the first step. */
    fun cancel() {
        if (share.busy) return
        dropShare(removeRoute = true)
    }

    /**
     * A fresh start of the app (the person had left with Back and came again while the process was alive):
     * what was being attached waits for nothing, but a share in progress is a draft and stays, exactly as it
     * would after a restart. True when it stays (the caller puts its screen back on the stack).
     */
    fun reset(): Boolean {
        if (share.busy) return true
        pendingNotice = null
        dropParked()
        if (share.item != null && !share.reading) {
            Controller.stack.removeAll { it == Route.SharePick }
            return true
        }
        dropShare(removeRoute = true)
        return false
    }

    /**
     * The Activity stopped. A share that arrived and was never shown (the app was locked, or locked
     * on arrival by the timeout) is dropped when the person leaves without unlocking: its working
     * file is deleted and nothing is kept. Not while unlocking is in progress (the fingerprint or
     * PIN screen stops the Activity too), not while saving, and not once the screen has been seen
     * (then it waits behind a later lock like any form in progress).
     */
    fun onHostStop(changingConfiguration: Boolean) {
        val unlocking = Controller.unlockInProgress()
        if (IntakeRules.dropParkedOnStop(changingConfiguration, Controller.locked, unlocking, parked != null)) dropParked()
        if (!changingConfiguration && Controller.locked && !unlocking) pendingNotice = null
        if (IntakeRules.dropShareOnStop(
                changingConfiguration, Controller.locked, unlocking, share.busy, seen, hasShare()
            )
        ) dropShare(removeRoute = true)
        // Going to the background (or being stopped for any reason): the last snapshot is queued and the thread
        // waits for it only a moment (the disk may be slow; the app is leaving anyway).
        if (share.item != null && !share.reading) {
            val f = flushNow()
            if (f != null) {
                try { f.get(ShareDraftRules.STOP_WAIT_MS, java.util.concurrent.TimeUnit.MILLISECONDS) } catch (_: Throwable) { }
            }
        }
    }

    /** A result parked for the unlock is thrown away (the person left without unlocking). */
    private fun dropParked() {
        val p = parked ?: return
        parked = null
        try { p.file.delete() } catch (_: Throwable) { }
        attachBusy = false
    }

    /** Back on the screen: from a form to the first step, from the first step out. */
    fun back() {
        if (share.busy) return
        if (share.mode != ShareMode.CHOOSE) {
            share = share.copy(mode = ShareMode.CHOOSE, error = "", titleError = "")
            scheduleSave()
        } else cancel()
    }

    fun chooseRecord() {
        if (share.item == null || share.busy) return
        share = share.copy(mode = ShareMode.RECORD, error = "")
        scheduleSave()
    }

    /** Only a file can be attached to a document; a shared text can only become a new record. */
    fun chooseAttach() {
        val item = share.item ?: return
        if (!item.isFile || share.busy) return
        share = share.copy(error = "")
        if (Route.SharePick !in Controller.stack) Controller.stack.add(Route.SharePick)
    }

    fun pickTarget(d: Doc) {
        if (share.item == null) return
        Controller.stack.removeAll { it == Route.SharePick }
        share = share.copy(mode = ShareMode.ATTACH, target = d, error = "")
        scheduleSave()
    }

    fun setTitle(v: String) { share = share.copy(title = v, titleError = ""); scheduleSave() }
    fun setKind(v: String) { kindTouched = true; share = share.copy(kind = v); scheduleSave() }
    fun setDate(v: String) { share = share.copy(date = v); scheduleSave() }
    fun setOrigin(v: String) { share = share.copy(origin = v); scheduleSave() }
    fun setNotes(v: String) { share = share.copy(notes = v); scheduleSave() }
    fun setDescription(v: String) {
        share = share.copy(description = ShareRules.capChars(v, AttachLimits.maxDescriptionChars))
        scheduleSave()
    }

    /** The operation id this form saves under: the one of the last try when nothing changed, else a new one. */
    private fun operationFor(op: String): ShareDraftRules.PendingOp? {
        val base = snapshot() ?: return null
        val p = ShareDraftRules.operationFor(pending, op, ShareDraftRules.fingerprint(base, op)) {
            java.util.UUID.randomUUID().toString()
        }
        pending = p
        return p
    }

    /** Creates the manual record: the file becomes its attachment, a text its text. */
    fun saveRecord() {
        val s = share
        val item = s.item ?: return
        if (s.busy) return
        val title = s.title.trim()
        if (title.codePointCount(0, title.length) < 3) {
            share = s.copy(titleError = "Informe um título com pelo menos 3 caracteres.")
            return
        }
        val text = item.text.ifEmpty { ShareRules.plainText(s.notes, AttachLimits.maxTextBytes) }
        val op = operationFor(ShareDraftRules.OP_RECORD) ?: return
        // "operation_id": the core makes the record once for this id, whatever happens to the process or to
        // the answer; the marker below carries the same id, so a restored draft can ask about it.
        val fields = JSONObject()
            .put("kind", s.kind.ifEmpty { "outro" })
            .put("title", ShareRules.cutBytes(title, AttachLimits.maxTitleBytes))
            .put("origin", s.origin.trim())
            .put("date", s.date)
            .put("text", text)
            .put("file_name", item.name)
            .put("file_description", "Recebido de outro aplicativo")
            .put("operation_id", op.id)
            .toString()
        val mine = gen
        share = s.copy(busy = true, progress = "Guardando o registro…", error = "", titleError = "", note = "")
        // The marker is queued BEFORE the core is asked and the core is called only once it is on the disk:
        // a process that dies from here to the end of the save comes back knowing a save was under way, with
        // the id to ask the core about. If the marker cannot be written the core is not called at all.
        val needed = !draftBlocked
        val marker = beginSaving(op)
        SyncRunner.background({
            ShareDraftRules.callAfterMarker(markerGate(marker, needed)) {
                val id = Api.createRecord(fields, item.path ?: "")
                SyncRunner.refreshDocs()
                id
            }
        }, { id ->
            if (gen == mine) {
                dropShare(removeRoute = true)
                finished(id, "Registro salvo.")
            }
        }, { m ->
            if (gen == mine) {
                share = share.copy(busy = false, progress = "", error = m)
                endSaving()
            }
        }, fail = "Não foi possível guardar o registro. Confira o espaço livre no aparelho e tente de novo.")
    }

    /** What an attach came to: done, or the document is not the one the person chose anymore. */
    private enum class AttachResult { DONE, TARGET_CHANGED }

    /** Attaches the file to the chosen document. */
    fun saveAttach() {
        val s = share
        val item = s.item ?: return
        val target = s.target ?: return
        val path = item.path ?: return
        if (s.busy) return
        val op = operationFor(ShareDraftRules.OP_ATTACH) ?: return
        val mine = gen
        val description = ShareRules.capChars(s.description.trim(), AttachLimits.maxDescriptionChars)
            .ifEmpty { "Recebido de outro aplicativo" }
        share = s.copy(busy = true, progress = "Anexando o arquivo…", error = "", note = "")
        val needed = !draftBlocked
        val marker = beginSaving(op)
        SyncRunner.background({
            ShareDraftRules.callAfterMarker(markerGate(marker, needed)) {
                try {
                    // "expected_number": the number the form shows. The id of a row is not enough: a sync may have
                    // replaced the library, and the same id may be another document now.
                    Api.attachFile(target.id, path, item.name, description, op.id, target.number)
                    SyncRunner.refreshDocs()
                    AttachResult.DONE
                } catch (e: CoreException) {
                    if (!ShareDraftRules.isTargetChanged(e.code)) throw e
                    // Nothing was written. The list is read again so the picker shows what exists now.
                    SyncRunner.refreshDocs()
                    AttachResult.TARGET_CHANGED
                }
            }
        }, { r ->
            if (gen == mine) {
                if (r == AttachResult.DONE) {
                    dropShare(removeRoute = true)
                    finished(target.id, "Anexo adicionado.")
                } else {
                    // The draft and its copy stay; the document must be chosen again.
                    pending = null
                    share = share.copy(
                        busy = false, progress = "", target = null,
                        mode = ShareMode.CHOOSE, error = ShareDraftRules.MSG_TARGET_CHANGED
                    )
                    endSaving()
                }
            }
        }, { m ->
            if (gen == mine) {
                share = share.copy(busy = false, progress = "", error = m)
                endSaving()
            }
        }, fail = "Não foi possível anexar o arquivo. Tente de novo.")
    }

    /**
     * The person stays in the app, on the record or the ficha that now holds what came. Said behind the lock
     * (a draft that was found already saved at start) it waits for the unlock.
     */
    private fun finished(documentId: Long, message: String) {
        Controller.render()
        Controller.selectTab(2)
        Controller.openDocument(documentId)
        if (Controller.locked) pendingNotice = message else Controller.say(message)
        Controller.requestSync(null, auto = true)
    }
}
