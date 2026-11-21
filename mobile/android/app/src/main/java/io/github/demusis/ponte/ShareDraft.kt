package io.github.demusis.ponte

import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest

/**
 * The marker written BEFORE the core is asked to create the record (or attach the file): it says "a
 * save was under way". If the process dies, the restored draft finds it and first looks whether the
 * save had already happened (see [ShareDraftRules.afterDeath]), so a record is never made twice.
 *
 * [op] is "record" or "attach"; [id] a random operation id; [baseline] the highest document id the
 * phone knew when the save started (a record made by this save has a higher one); [text] what the
 * record's text will be (to recognise a record made of text only).
 */
internal data class DraftSaving(val op: String, val id: String, val baseline: Long, val at: Long, val text: String)

/**
 * A share in progress, as it is kept on disk: the received item (the private copy is [copy], a
 * file name inside the draft folder; empty for a text) and every field of the form. One draft at a
 * time, like the share itself.
 */
internal data class ShareDraft(
    /** When the share arrived: the 7 days of retention count from here. */
    val createdAt: Long,
    val updatedAt: Long,
    val copy: String,
    val name: String,
    val size: Long,
    val mime: String,
    /** SHA-256 (hex) of the copy, or empty. */
    val hash: String,
    val text: String,
    val subject: String,
    /** A [ShareMode] name. */
    val mode: String,
    val kind: String,
    val title: String,
    val date: String,
    val origin: String,
    val notes: String,
    val targetId: Long,
    val targetNumber: String,
    val targetTitle: String,
    val description: String,
    val saving: DraftSaving?
)

/** What is found of the copy a draft points to. [hash] is null when it was not worked out. */
internal class CopyFacts(val size: Long, val hash: String?)

/** What the stored documents say about one candidate record: its attachments (name, size) and its text. */
internal class DocFacts(val files: List<Pair<String, Long>>, val text: String)

/** A document of the library list, as far as the search for a record already saved needs. */
internal class DocRow(val id: Long, val manual: Boolean)

/**
 * A flat JSON object: strings, whole numbers, booleans and null, nothing nested. Enough for the draft,
 * and plain Kotlin, so JUnit tests the real thing (the Android JSON classes are stubs there).
 */
internal object FlatJson {
    private class Bad : Exception()

    fun write(fields: List<Pair<String, Any?>>): String {
        val sb = StringBuilder("{")
        fields.forEachIndexed { n, (k, v) ->
            if (n > 0) sb.append(',')
            quote(sb, k).append(':')
            when (v) {
                null -> sb.append("null")
                is String -> quote(sb, v)
                is Long -> sb.append(v)
                is Int -> sb.append(v)
                is Boolean -> sb.append(v)
                else -> throw IllegalArgumentException("Tipo sem JSON plano: ${v::class.java.name}")
            }
        }
        return sb.append('}').toString()
    }

    private fun quote(sb: StringBuilder, s: String): StringBuilder {
        sb.append('"')
        for (c in s) {
            when {
                c == '"' -> sb.append("\\\"")
                c == '\\' -> sb.append("\\\\")
                c == '\n' -> sb.append("\\n")
                c == '\r' -> sb.append("\\r")
                c == '\t' -> sb.append("\\t")
                // Every other control, any half of a surrogate pair (a lone one must survive too) and the two
                // line separators are written as \uXXXX: what comes back is exactly what went in.
                c < ' ' || c in '\uD800'..'\uDFFF' || c == ' ' || c == ' ' ->
                    sb.append("\\u").append(Integer.toHexString(c.code or 0x10000).substring(1))
                else -> sb.append(c)
            }
        }
        return sb.append('"')
    }

    /** The object, or null when [text] is not exactly one flat object. */
    fun read(text: String): Map<String, Any?>? = try {
        Parser(text).obj()
    } catch (_: Bad) {
        null
    } catch (_: StackOverflowError) {
        null
    }

    private class Parser(val s: String) {
        var i = 0

        fun fail(): Nothing = throw Bad()
        fun peek(): Char = if (i < s.length) s[i] else fail()
        fun ws() {
            while (i < s.length && (s[i] == ' ' || s[i] == '\n' || s[i] == '\r' || s[i] == '\t')) i++
        }
        fun expect(c: Char) {
            if (peek() != c) fail()
            i++
        }

        fun obj(): Map<String, Any?> {
            ws()
            expect('{')
            val m = LinkedHashMap<String, Any?>()
            ws()
            if (peek() == '}') {
                i++
            } else {
                while (true) {
                    ws()
                    val k = str()
                    ws()
                    expect(':')
                    ws()
                    if (m.containsKey(k)) fail()
                    m[k] = value()
                    ws()
                    if (peek() == ',') {
                        i++
                        continue
                    }
                    expect('}')
                    break
                }
            }
            ws()
            if (i != s.length) fail()
            return m
        }

        fun value(): Any? = when (peek()) {
            '"' -> str()
            't' -> lit("true", true)
            'f' -> lit("false", false)
            'n' -> lit("null", null)
            else -> num()
        }

        fun lit(word: String, v: Any?): Any? {
            if (!s.startsWith(word, i)) fail()
            i += word.length
            return v
        }

        fun num(): Long {
            val start = i
            if (peek() == '-') i++
            val digits = i
            while (i < s.length && s[i] in '0'..'9') i++
            if (i == digits || i - digits > 18) fail()
            if (i < s.length && (s[i] == '.' || s[i] == 'e' || s[i] == 'E')) fail()
            return s.substring(start, i).toLongOrNull() ?: fail()
        }

        fun str(): String {
            expect('"')
            val sb = StringBuilder()
            while (true) {
                if (i >= s.length) fail()
                val c = s[i++]
                when {
                    c == '"' -> return sb.toString()
                    c == '\\' -> {
                        if (i >= s.length) fail()
                        when (val e = s[i++]) {
                            '"', '\\', '/' -> sb.append(e)
                            'b' -> sb.append('\b')
                            'f' -> sb.append('\u000c')
                            'n' -> sb.append('\n')
                            'r' -> sb.append('\r')
                            't' -> sb.append('\t')
                            'u' -> {
                                if (i + 4 > s.length) fail()
                                val h = s.substring(i, i + 4)
                                if (!h.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) fail()
                                sb.append(h.toInt(16).toChar())
                                i += 4
                            }
                            else -> fail()
                        }
                    }
                    c < ' ' -> fail()
                    else -> sb.append(c)
                }
            }
        }
    }
}

/**
 * What is decided about the draft of a share, as plain functions of plain values (no Android
 * types), so JUnit can test every rule: the format, the 7 days, the damaged copy, the save marker
 * and the search for a record that a dead process had already made.
 */
internal object ShareDraftRules {
    /** The format number written in every draft; a bigger one comes from a newer app (see [decode]). */
    const val FORMAT = 1

    /** A draft older than this (from the day the share arrived) is discarded at start. */
    const val MAX_AGE_MS = 7L * 24 * 60 * 60 * 1000

    const val MSG_RECOVERED = "Rascunho recuperado."
    const val MSG_LOST = "O item recebido não pôde ser recuperado; compartilhe de novo."
    const val MSG_EXPIRED = "Um rascunho de compartilhamento com mais de 7 dias foi descartado."
    const val MSG_CANNOT_KEEP =
        "Não foi possível guardar o rascunho neste aparelho. O que você digitou continua aqui, " +
            "mas se perde se o aplicativo for fechado."
    const val MSG_CHECKING = "Conferindo se o registro já foi salvo…"
    const val MSG_CANNOT_CHECK =
        "Não foi possível conferir se este item já foi salvo. Veja em Documentos antes de salvar de novo."
    const val MSG_SAVED_BEFORE = "Este item já tinha sido salvo antes de o aplicativo fechar."

    const val OP_RECORD = "record"
    const val OP_ATTACH = "attach"

    /** Most characters kept of each typed field, so a draft stays small (the form limits the rest itself). */
    private const val MAX_FIELD = 4_000
    private const val MAX_NOTES = 100_000
    private const val MAX_TEXT = 600_000

    private val MODES = setOf("CHOOSE", "RECORD", "ATTACH")
    private val NEWER = Regex("""^\s*\{\s*"v"\s*:\s*(\d{1,9})""")

    /** A name made only of plain characters, to be used as a file inside the draft folder. */
    fun safeName(n: String): Boolean =
        n.isNotEmpty() && n.length <= 120 && n != "." && n != ".." &&
            n.none { it == '/' || it == '\\' || it == '\u0000' || it < ' ' } && !n.contains("..")

    private fun cut(s: String, max: Int): String = if (s.length <= max) s else s.substring(0, max)

    /** The draft with every field within what a draft may hold (what [decode] accepts). */
    fun clamped(d: ShareDraft): ShareDraft = d.copy(
        name = cut(d.name, MAX_FIELD), mime = cut(d.mime, 255), text = cut(d.text, MAX_TEXT),
        subject = cut(d.subject, MAX_FIELD), kind = cut(d.kind, MAX_FIELD), title = cut(d.title, MAX_FIELD),
        date = cut(d.date, MAX_FIELD), origin = cut(d.origin, MAX_FIELD), notes = cut(d.notes, MAX_NOTES),
        targetNumber = cut(d.targetNumber, MAX_FIELD), targetTitle = cut(d.targetTitle, MAX_FIELD),
        description = cut(d.description, MAX_FIELD),
        saving = d.saving?.copy(text = cut(d.saving.text, MAX_TEXT))
    )

    fun encode(d: ShareDraft): String {
        val c = clamped(d)
        val f = ArrayList<Pair<String, Any?>>()
        f += "v" to FORMAT
        f += "created" to c.createdAt
        f += "updated" to c.updatedAt
        f += "copy" to c.copy
        f += "name" to c.name
        f += "size" to c.size
        f += "mime" to c.mime
        f += "hash" to c.hash
        f += "text" to c.text
        f += "subject" to c.subject
        f += "mode" to c.mode
        f += "kind" to c.kind
        f += "title" to c.title
        f += "date" to c.date
        f += "origin" to c.origin
        f += "notes" to c.notes
        f += "tid" to c.targetId
        f += "tnum" to c.targetNumber
        f += "ttitle" to c.targetTitle
        f += "desc" to c.description
        c.saving?.let {
            f += "sop" to it.op
            f += "sid" to it.id
            f += "sbase" to it.baseline
            f += "sat" to it.at
            f += "stext" to it.text
        }
        return FlatJson.write(f)
    }

    sealed class Decoded {
        class Ok(val draft: ShareDraft) : Decoded()

        /** Not a draft of this app (damaged, cut, wrong types): treated as no draft. */
        object Corrupt : Decoded()

        /** Written by a newer app: not understood, not to be touched. */
        object Newer : Decoded()
    }

    fun decode(text: String): Decoded {
        if (text.length > 2 * (MAX_TEXT + MAX_NOTES) + 100_000) return Decoded.Corrupt
        // The format number comes first in every draft: a bigger one is told apart before anything else is
        // judged, so a newer app's draft is never mistaken for a damaged one (and deleted).
        NEWER.find(text)?.let { m ->
            val v = m.groupValues[1].toLongOrNull()
            if (v != null && v > FORMAT) return Decoded.Newer
        }
        val m = FlatJson.read(text) ?: return Decoded.Corrupt
        val v = m["v"] as? Long ?: return Decoded.Corrupt
        if (v > FORMAT) return Decoded.Newer
        if (v < 1) return Decoded.Corrupt
        fun s(k: String): String? = m[k] as? String
        fun n(k: String): Long? = m[k] as? Long
        val createdAt = n("created") ?: return Decoded.Corrupt
        val updatedAt = n("updated") ?: return Decoded.Corrupt
        val copy = s("copy") ?: return Decoded.Corrupt
        val name = s("name") ?: return Decoded.Corrupt
        val size = n("size") ?: return Decoded.Corrupt
        val mime = s("mime") ?: return Decoded.Corrupt
        val hash = s("hash") ?: return Decoded.Corrupt
        val body = s("text") ?: return Decoded.Corrupt
        val subject = s("subject") ?: return Decoded.Corrupt
        val mode = s("mode") ?: return Decoded.Corrupt
        val kind = s("kind") ?: return Decoded.Corrupt
        val title = s("title") ?: return Decoded.Corrupt
        val date = s("date") ?: return Decoded.Corrupt
        val origin = s("origin") ?: return Decoded.Corrupt
        val notes = s("notes") ?: return Decoded.Corrupt
        val tid = n("tid") ?: return Decoded.Corrupt
        val tnum = s("tnum") ?: return Decoded.Corrupt
        val ttitle = s("ttitle") ?: return Decoded.Corrupt
        val desc = s("desc") ?: return Decoded.Corrupt
        if (createdAt <= 0 || updatedAt < 0 || size < 0 || tid < 0) return Decoded.Corrupt
        if (mode !in MODES) return Decoded.Corrupt
        // A file: a safe name inside the folder, a display name and a plausible size. A text: nothing else.
        if (copy.isNotEmpty()) {
            if (!safeName(copy) || name.isEmpty()) return Decoded.Corrupt
        } else if (body.isEmpty()) {
            return Decoded.Corrupt
        }
        var saving: DraftSaving? = null
        if (m.containsKey("sop")) {
            val op = s("sop") ?: return Decoded.Corrupt
            val id = s("sid") ?: return Decoded.Corrupt
            val base = n("sbase") ?: return Decoded.Corrupt
            val at = n("sat") ?: return Decoded.Corrupt
            val st = s("stext") ?: return Decoded.Corrupt
            if (op != OP_RECORD && op != OP_ATTACH) return Decoded.Corrupt
            if (id.isEmpty() || base < 0) return Decoded.Corrupt
            if (op == OP_ATTACH && (copy.isEmpty() || tid <= 0)) return Decoded.Corrupt
            saving = DraftSaving(op, id, base, at, st)
        }
        return Decoded.Ok(
            ShareDraft(
                createdAt, updatedAt, copy, name, size, mime, hash, body, subject, mode, kind, title, date, origin,
                notes, tid, tnum, ttitle, desc, saving
            )
        )
    }

    /**
     * The mode the form comes back in: "attach" only when there is a file and a document chosen
     * (a combination that cannot be shown otherwise falls back to the first step).
     */
    fun normalMode(mode: String, hasFile: Boolean, hasTarget: Boolean): String = when {
        mode == "RECORD" -> "RECORD"
        mode == "ATTACH" && hasFile && hasTarget -> "ATTACH"
        else -> "CHOOSE"
    }

    // ------------------------------------------------------------------ what to do at start

    enum class Verdict { EXPIRED, COPY_LOST, CHECK_FIRST, READY }

    /**
     * The first check of a draft found at start, in this order: more than 7 days old (from the day the
     * share arrived) → [Verdict.EXPIRED]; a file whose copy is gone, of another size or of another hash →
     * [Verdict.COPY_LOST]; a save that was under way → [Verdict.CHECK_FIRST] (see [afterDeath]); else
     * [Verdict.READY]. A text needs no copy.
     */
    fun verdict(d: ShareDraft, now: Long, facts: CopyFacts?): Verdict {
        if (now - d.createdAt > MAX_AGE_MS) return Verdict.EXPIRED
        if (d.copy.isNotEmpty()) {
            if (facts == null || facts.size != d.size) return Verdict.COPY_LOST
            if (d.hash.isNotEmpty() && facts.hash != null && facts.hash != d.hash) return Verdict.COPY_LOST
        }
        return if (d.saving != null) Verdict.CHECK_FIRST else Verdict.READY
    }

    // ------------------------------------------------------------------ the save marker

    /** editing → saving: the marker that is written before the core is called. */
    fun begin(d: ShareDraft, op: String, id: String, baseline: Long, expectedText: String, now: Long): ShareDraft =
        d.copy(updatedAt = now, saving = DraftSaving(op, id, baseline, now, expectedText))

    /** saving → editing: the save failed, the person still has the form. (Success clears the draft: no state.) */
    fun rollback(d: ShareDraft, now: Long): ShareDraft = d.copy(updatedAt = now, saving = null)

    enum class Resolution { SHOW_FORM, ALREADY_SAVED }

    /**
     * A restored draft that was saving: with the record already in the library ([found]) the draft is over
     * ([Resolution.ALREADY_SAVED]); without it the form is back, ready to save ([Resolution.SHOW_FORM], and the
     * marker is rolled back). A draft that was only being edited is always the form.
     */
    fun afterDeath(d: ShareDraft, found: Boolean): Resolution =
        if (d.saving != null && found) Resolution.ALREADY_SAVED else Resolution.SHOW_FORM

    /** The documents that may be the record a dead save made: made by hand and newer than the marker, newest first. */
    fun candidateIds(baseline: Long, docs: List<DocRow>, limit: Int = 30): List<Long> =
        docs.filter { it.manual && it.id > baseline }.map { it.id }.sortedDescending().take(limit)

    private fun flat(s: String) = s.replace("\r\n", "\n").replace('\r', '\n').trim()

    /**
     * Whether [f] is what the dead save made: the file of the draft (same name, same size) among its
     * attachments, or, for a text with no file, the same text.
     */
    fun sameRecord(d: ShareDraft, f: DocFacts): Boolean {
        val s = d.saving ?: return false
        if (d.copy.isNotEmpty()) return f.files.any { it.first == d.name.trim() && it.second == d.size }
        return s.text.isNotBlank() && flat(f.text) == flat(s.text)
    }

    /** Whether the document the file was to be attached to already holds it (same name, same size). */
    fun alreadyAttached(d: ShareDraft, files: List<Pair<String, Long>>): Boolean =
        d.copy.isNotEmpty() && files.any { it.first == d.name.trim() && it.second == d.size }

    // ------------------------------------------------------------------ cleaning

    /** Whether a cleaner may delete [path]: never what [keep] holds (the copy a draft points to). */
    fun mayDelete(path: String, keep: Set<String>): Boolean = path !in keep

    /** Whether [path] lies inside the draft folder [dir] (cleaners of other folders leave it alone). */
    fun insideFolder(path: String, dir: String): Boolean {
        val d = dir.trimEnd('/', '\\')
        val p = path.replace('\\', '/')
        val q = d.replace('\\', '/')
        return q.isNotEmpty() && (p == q || p.startsWith("$q/"))
    }
}

/**
 * The draft folder (inside the app's private files, never the cache, never external storage, not served by
 * the provider, not in any backup rule): the draft file and the private copy of the received item.
 * Plain `java.io`, so it is tested with a temporary folder.
 */
internal class ShareDraftStore(private val dir: File) {
    companion object {
        /** Folder inside `filesDir`. */
        const val DIR = "rascunho"
        const val FILE = "rascunho.json"
        const val TMP = "rascunho.json.tmp"
        private const val MAX_BYTES = 4L * 1024 * 1024

        /** Held while the draft is written, replaced or removed, so those never interleave. */
        val lock = Any()

        fun hashOf(f: File): String {
            val md = MessageDigest.getInstance("SHA-256")
            f.inputStream().use { input ->
                val buf = ByteArray(64 * 1024)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    md.update(buf, 0, n)
                }
            }
            return md.digest().joinToString("") { String.format("%02x", it) }
        }
    }

    sealed class Read {
        object Absent : Read()
        object Corrupt : Read()
        object Newer : Read()
        class Found(val draft: ShareDraft) : Read()
    }

    sealed class Start {
        /** No draft. */
        object None : Start()

        /** A newer app's draft is there: untouched, and nothing of ours is written over it. */
        object Blocked : Start()

        /** The draft is gone (with its copy); [message] is to be told once, or null. */
        class Dropped(val message: String?) : Start()

        /** The draft to put back on the screen; [checkFirst] when a save was under way. */
        class Resume(val draft: ShareDraft, val checkFirst: Boolean) : Start()
    }

    private fun file() = File(dir, FILE)

    fun path(name: String): String = File(dir, name).absolutePath

    /** A new, uniquely named empty copy file (the system makes the name, so no two collide). */
    fun newCopy(prefix: String): File {
        dir.mkdirs()
        return File.createTempFile("$prefix-", ".bin", dir)
    }

    fun read(): Read {
        val f = file()
        if (!f.isFile) return Read.Absent
        val text = try {
            if (f.length() > MAX_BYTES) return Read.Corrupt
            String(f.readBytes(), Charsets.UTF_8)
        } catch (_: IOException) {
            return Read.Corrupt
        }
        return when (val r = ShareDraftRules.decode(text)) {
            is ShareDraftRules.Decoded.Ok -> Read.Found(r.draft)
            ShareDraftRules.Decoded.Corrupt -> Read.Corrupt
            ShareDraftRules.Decoded.Newer -> Read.Newer
        }
    }

    /**
     * Writes the draft atomically: to a temporary file, flushed to the disk, then renamed over the
     * old one, so a reader sees the old draft or the new one, never half of one. False when it could not
     * be done (no room, no folder): the old draft, if any, is left as it was.
     */
    fun write(d: ShareDraft): Boolean = synchronized(lock) {
        val tmp = File(dir, TMP)
        try {
            dir.mkdirs()
            val bytes = ShareDraftRules.encode(d).toByteArray(Charsets.UTF_8)
            java.io.FileOutputStream(tmp).use { out ->
                out.write(bytes)
                out.flush()
                out.fd.sync()
            }
            try {
                Files.move(tmp.toPath(), file().toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            } catch (_: AtomicMoveNotSupportedException) {
                Files.move(tmp.toPath(), file().toPath(), StandardCopyOption.REPLACE_EXISTING)
            }
            true
        } catch (_: Throwable) {
            try { tmp.delete() } catch (_: Throwable) { }
            false
        }
    }

    /** Removes the draft file (and a half-written one). The copy is removed by whoever knows it. */
    fun deleteDraft() {
        synchronized(lock) {
            try { file().delete() } catch (_: Throwable) { }
            try { File(dir, TMP).delete() } catch (_: Throwable) { }
        }
    }

    /** The draft first, then its copy: a death in between leaves a loose copy (swept later), never a draft without its copy. */
    fun clear(copyName: String?) {
        deleteDraft()
        if (!copyName.isNullOrEmpty() && ShareDraftRules.safeName(copyName)) {
            try { File(dir, copyName).delete() } catch (_: Throwable) { }
        }
    }

    /** What is found of the copy named [name]; null when it is not there. The hash is worked out only on request. */
    fun facts(name: String, withHash: Boolean): CopyFacts? {
        if (!ShareDraftRules.safeName(name)) return null
        val f = File(dir, name)
        if (!f.isFile) return null
        return try {
            CopyFacts(f.length(), if (withHash) hashOf(f) else null)
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * Deletes every file of the folder except the draft file and the copy [live]: what a share left behind
     * (a copy whose draft was never written, a half-written draft). Never the live copy.
     */
    fun sweep(live: String?) {
        synchronized(lock) {
            try {
                dir.listFiles()?.forEach {
                    if (it.isFile && it.name != FILE && ShareDraftRules.mayDelete(it.name, setOfNotNull(live))) {
                        try { it.delete() } catch (_: Throwable) { }
                    }
                }
            } catch (_: Throwable) {
            }
        }
    }

    /** The whole folder: used when the phone's data is reset. */
    fun wipe() {
        synchronized(lock) {
            try {
                dir.listFiles()?.forEach { try { it.delete() } catch (_: Throwable) { } }
                dir.delete()
            } catch (_: Throwable) {
            }
        }
    }

    /**
     * What to do with the folder at start, and the cleaning that goes with it. Never throws: whatever is
     * wrong with the draft means "no draft". A draft of a newer app is kept untouched.
     */
    fun start(now: Long, setUp: Boolean): Start {
        try {
            when (val r = read()) {
                Read.Absent -> {
                    sweep(null)
                    return Start.None
                }
                Read.Newer -> return Start.Blocked
                Read.Corrupt -> {
                    clear(null)
                    sweep(null)
                    return Start.Dropped(ShareDraftRules.MSG_LOST)
                }
                is Read.Found -> {
                    val d = r.draft
                    if (!setUp) {
                        clear(d.copy)
                        sweep(null)
                        return Start.Dropped(null)
                    }
                    val facts = if (d.copy.isEmpty()) null else facts(d.copy, withHash = d.hash.isNotEmpty())
                    return when (ShareDraftRules.verdict(d, now, facts)) {
                        ShareDraftRules.Verdict.EXPIRED -> {
                            clear(d.copy)
                            sweep(null)
                            Start.Dropped(ShareDraftRules.MSG_EXPIRED)
                        }
                        ShareDraftRules.Verdict.COPY_LOST -> {
                            clear(d.copy)
                            sweep(null)
                            Start.Dropped(ShareDraftRules.MSG_LOST)
                        }
                        ShareDraftRules.Verdict.CHECK_FIRST -> {
                            sweep(d.copy.ifEmpty { null })
                            Start.Resume(d, checkFirst = true)
                        }
                        ShareDraftRules.Verdict.READY -> {
                            sweep(d.copy.ifEmpty { null })
                            Start.Resume(d, checkFirst = false)
                        }
                    }
                }
            }
        } catch (_: Throwable) {
            return Start.None
        }
    }
}
