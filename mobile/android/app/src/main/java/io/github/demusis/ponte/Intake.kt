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
    val path: String?
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
    val error: String = ""
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
 *  - What arrives from other apps is untrusted: see [ShareRules] and [stage].
 *  - Everything runs on the UI thread except the copy and the core calls.
 */
internal object Intake {
    /** Folder of the working copies, inside the cache folder (not backed up, not served by the provider). */
    const val DIR = "entrada"

    /** The marker intent the trampoline sends to [MainActivity]; it carries no content (see [offer]). */
    const val ACTION_RECEIVED = "io.github.demusis.ponte.action.SHARE_RECEIVED"

    /** An image bigger than the limit is shrunk like a photo; it may be up to this size when it arrives. */
    private const val IMAGE_INPUT_CAP = 40L * 1024 * 1024
    private const val OFFER_LIFETIME_MS = 30_000L

    private lateinit var app: Context
    private val main = Handler(Looper.getMainLooper())
    private var counter = 0

    fun init(ctx: Context) {
        app = ctx.applicationContext
    }

    private fun ui(block: () -> Unit) {
        main.post { try { block() } catch (_: Throwable) { } }
    }

    private fun dir(): File = File(app.cacheDir, DIR).apply { mkdirs() }

    private fun workFile(prefix: String): File {
        counter++
        return File(dir(), prefix + "-" + System.currentTimeMillis() + "-" + counter + ".tmp")
    }

    /** Cold start: nothing left from an earlier run is kept (the share is lost with the process; see [receive]). */
    fun cleanAll(ctx: Context) {
        try {
            File(ctx.cacheDir, DIR).listFiles()?.forEach { if (it.isFile) it.delete() }
        } catch (_: Throwable) {
        }
    }

    /** Camera pictures taken for an "Anexar…" that must survive a lock while the camera is in front. */
    fun keepPaths(): Set<String> = setOfNotNull(attachPhoto?.absolutePath)

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
    private fun stage(uri: Uri, mimeHint: String?, allowBigImage: Boolean): Staged {
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
        val image = allowBigImage && ShareRules.looksLikeImage(mime)
        val cap = if (image) IMAGE_INPUT_CAP else limit
        val tooBig = if (image) "A imagem passa de ${IMAGE_INPUT_CAP / (1024 * 1024)} MB. Compartilhe uma versão menor."
        else ShareRules.tooBigMessage(name, limit)
        // The size the provider announces is only a hint: it lets an obvious case fail early.
        if (declared > cap) throw PonteException(tooBig)

        val work = workFile("in")
        var done = false
        try {
            try {
                openForRead(uri).use { input -> work.outputStream().use { out -> BoundedCopy.copy(input, out, cap) } }
            } catch (_: TooBigException) {
                throw PonteException(tooBig)
            }
            var file = work
            if (image && work.length() > limit) {
                val shrunk = workFile("img")
                try {
                    Images.resizeForUpload(work, shrunk)
                } catch (e: OutOfMemoryError) {
                    shrunk.delete()
                    throw e
                } catch (e: Throwable) {
                    shrunk.delete()
                    throw PonteException("Não foi possível reduzir a imagem. Compartilhe uma versão menor.")
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

    /**
     * The camera answered. After a process death the state above is gone and the picture is
     * just deleted (the person takes it again); nothing is attached behind a lock either.
     */
    fun onAttachPhotoResult(ok: Boolean) {
        val f = attachPhoto ?: return
        attachPhoto = null
        if (!ok || !f.isFile || f.length() == 0L || Controller.locked) {
            f.delete()
            return
        }
        val docId = attachDocId
        val description = attachDescription.ifEmpty { "Foto tirada no celular" }
        attachDocId = 0L
        attachBusy = true
        Thread {
            val out = workFile("foto")
            try {
                Images.resizeForUpload(f, out)
                val name = "foto-" + LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd-HHmmss")) + ".jpg"
                f.delete()
                ui { runAttach(docId, out, name, description) }
            } catch (e: Throwable) {
                f.delete()
                out.delete()
                val m = PhoneWords.explain(e, "Não foi possível guardar a foto. Tente de novo.")
                ui { attachBusy = false; Controller.say(m, long = true) }
            }
        }.start()
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

    /** The picker answered with [uri] (null when the person gave up). Dropped when the app locked meanwhile. */
    fun onAttachPicked(uri: Uri?) {
        if (uri == null || Controller.locked || attachDocId <= 0 || attachBusy) return
        val docId = attachDocId
        val description = attachDescription
        attachDocId = 0L
        attachBusy = true
        Thread {
            try {
                val s = stage(uri, null, allowBigImage = false)
                ui { runAttach(docId, s.file, s.name, description) }
            } catch (e: Throwable) {
                val m = PhoneWords.explain(e, "Não foi possível ler o arquivo. Tente de novo.")
                ui { attachBusy = false; Controller.say(m, long = true) }
            }
        }.start()
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
    private var gen = 0

    /** The "Guardar no Ponte" screen has been shown (unlocked) at least once. */
    private var seen = false

    /** What to tell once the app is unlocked (a refusal that came while it was locked). */
    private var pendingNotice: String? = null

    /** The pending share is in memory only; a process death loses it and the person shares again. */
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

    /**
     * An offer came in through [MainActivity]. Order: the checks that need no content; "not set up"
     * (told, nothing read); the file is copied under the cap; and the screen is put on the
     * stack at once, so behind a lock it simply waits there (nothing of it is composed while locked).
     */
    fun receive() {
        val o = takeOffer() ?: return
        val verdict = ShareRules.check(
            o.action, o.mime, o.uri?.scheme, o.uri?.authority, o.streamCount,
            o.text.isNotEmpty(), PonteFiles.AUTHORITY
        )
        if (!verdict.ok) { tell(verdict.message); return }
        if (!Controller.isSetUp()) { tell(ShareRules.MSG_NOT_SET_UP); return }
        if (share.busy) { tell("Aguarde: outro arquivo está sendo guardado."); return }
        dropShare(removeRoute = true)
        gen++
        seen = false
        val mine = gen
        val kinds = Controller.photo.kinds
        val kind = kinds.firstOrNull { it.key == "outro" }?.key ?: kinds.first().key
        val subject = ShareRules.oneLine(o.subject, AttachLimits.maxTitleBytes)
        val text = ShareRules.plainText(o.text, AttachLimits.maxTextBytes)
        val uri = o.uri
        if (uri == null) {
            val title = subject.ifEmpty { ShareRules.oneLine(text.lineSequence().firstOrNull { it.isNotBlank() }, 120) }
            share = ShareState(item = ShareItem("", 0, text, subject, null), kinds = kinds, kind = kind, title = title)
            Controller.stack.add(Route.Share)
            return
        }
        share = ShareState(reading = true, kinds = kinds, kind = kind)
        Controller.stack.add(Route.Share)
        Thread {
            try {
                val s = stage(uri, o.mime, allowBigImage = true)
                ui {
                    if (gen == mine) {
                        val title = subject.ifEmpty { ShareRules.oneLine(ShareRules.titleFromName(s.name), AttachLimits.maxTitleBytes) }
                        share = share.copy(
                            reading = false, error = "",
                            item = ShareItem(s.name, s.size, text, subject, s.file.absolutePath),
                            title = title
                        )
                    } else s.file.delete()
                }
            } catch (e: Throwable) {
                val m = PhoneWords.explain(e, "Não foi possível receber o arquivo. Compartilhe-o de novo.")
                ui {
                    if (gen == mine) share = share.copy(reading = false, error = m)
                }
            }
        }.start()
    }

    /** Deletes the pending share's working file and forgets it; optionally takes its screens off the stack. */
    private fun dropShare(removeRoute: Boolean) {
        gen++
        share.item?.path?.let { try { File(it).delete() } catch (_: Throwable) { } }
        share = ShareState()
        if (removeRoute) Controller.stack.removeAll { it == Route.Share || it == Route.SharePick }
    }

    /** "Cancelar", or Back at the first step. */
    fun cancel() {
        if (share.busy) return
        dropShare(removeRoute = true)
    }

    /** A fresh start of the app (the person had left with Back): nothing pending survives. */
    fun reset() {
        if (share.busy) return
        pendingNotice = null
        dropShare(removeRoute = true)
    }

    /**
     * The Activity stopped. A share that arrived and was never shown (the app was locked, or locked
     * on arrival by the timeout) is dropped when the person leaves without unlocking: its working
     * file is deleted and nothing is kept. Not while unlocking is in progress (the fingerprint or
     * PIN screen stops the Activity too), not while saving, and not once the screen has been seen
     * (then it waits behind a later lock like any form in progress).
     */
    fun onHostStop(changingConfiguration: Boolean) {
        if (changingConfiguration || !Controller.locked || Controller.unlockInProgress()) return
        pendingNotice = null
        if (share.busy || seen || !hasShare()) return
        dropShare(removeRoute = true)
    }

    /** Back on the screen: from a form to the first step, from the first step out. */
    fun back() {
        if (share.busy) return
        if (share.mode != ShareMode.CHOOSE) share = share.copy(mode = ShareMode.CHOOSE, error = "", titleError = "")
        else cancel()
    }

    fun chooseRecord() {
        if (share.item == null || share.busy) return
        share = share.copy(mode = ShareMode.RECORD, error = "")
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
    }

    fun setTitle(v: String) { share = share.copy(title = v, titleError = "") }
    fun setKind(v: String) { share = share.copy(kind = v) }
    fun setDate(v: String) { share = share.copy(date = v) }
    fun setOrigin(v: String) { share = share.copy(origin = v) }
    fun setNotes(v: String) { share = share.copy(notes = v) }
    fun setDescription(v: String) { share = share.copy(description = ShareRules.capChars(v, AttachLimits.maxDescriptionChars)) }

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
        val fields = JSONObject()
            .put("kind", s.kind.ifEmpty { "outro" })
            .put("title", ShareRules.cutBytes(title, AttachLimits.maxTitleBytes))
            .put("origin", s.origin.trim())
            .put("date", s.date)
            .put("text", text)
            .put("file_name", item.name)
            .put("file_description", "Recebido de outro aplicativo")
            .toString()
        val mine = gen
        share = s.copy(busy = true, progress = "Guardando o registro…", error = "", titleError = "")
        SyncRunner.background({
            val id = Api.createRecord(fields, item.path ?: "")
            SyncRunner.refreshDocs()
            id
        }, { id ->
            if (gen == mine) {
                dropShare(removeRoute = true)
                finished(id, "Registro salvo.")
            }
        }, { m ->
            if (gen == mine) share = share.copy(busy = false, progress = "", error = m)
        }, fail = "Não foi possível guardar o registro. Confira o espaço livre no aparelho e tente de novo.")
    }

    /** Attaches the file to the chosen document. */
    fun saveAttach() {
        val s = share
        val item = s.item ?: return
        val target = s.target ?: return
        val path = item.path ?: return
        if (s.busy) return
        val mine = gen
        val description = ShareRules.capChars(s.description.trim(), AttachLimits.maxDescriptionChars)
            .ifEmpty { "Recebido de outro aplicativo" }
        share = s.copy(busy = true, progress = "Anexando o arquivo…", error = "")
        SyncRunner.background({
            Api.attachFile(target.id, path, item.name, description)
            SyncRunner.refreshDocs()
        }, {
            if (gen == mine) {
                dropShare(removeRoute = true)
                finished(target.id, "Anexo adicionado.")
            }
        }, { m ->
            if (gen == mine) share = share.copy(busy = false, progress = "", error = m)
        }, fail = "Não foi possível anexar o arquivo. Tente de novo.")
    }

    /** The person stays in the app, on the record or the ficha that now holds what came. */
    private fun finished(documentId: Long, message: String) {
        Controller.render()
        Controller.selectTab(2)
        Controller.openDocument(documentId)
        Controller.say(message)
        Controller.requestSync(null, auto = true)
    }
}
