package io.github.demusis.ponte

import java.io.File
import java.io.IOException
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.concurrent.Executor
import java.util.concurrent.Future
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit

/**
 * The marker written BEFORE the core is asked to create the record (or attach the file): it says "a
 * save was under way". [operationId] is the UUID that goes to the core as "operation_id", so after a
 * process death the core itself says whether that operation was committed (see [ShareDraftRules.recover]);
 * nothing is ever guessed from names, sizes or texts.
 *
 * [op] is "record" or "attach". An empty (or malformed) [operationId] is the marker of the previous build,
 * whose id never reached the core: it can never be resolved, so its draft is kept and shown as a form.
 */
internal data class DraftSaving(val op: String, val operationId: String, val at: Long) {
    val usable: Boolean get() = ShareDraftRules.validOperationId(operationId)
}

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
    /** The shared text. Kept ONCE: the save marker holds no copy of it. */
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
    /** The number of the target document as the form showed it: sent to the core as "expected_number". */
    val targetNumber: String,
    val targetTitle: String,
    val description: String,
    val saving: DraftSaving?
)

/** What is found of the copy a draft points to. [hash] is null when it was not worked out. */
internal class CopyFacts(val size: Long, val hash: String?)

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

    /**
     * Escapes only what JSON requires: the quote, the backslash, the controls below the space, and a
     * surrogate that has no partner (a lone one must survive too, and UTF-8 cannot hold it). A well-formed
     * pair (an emoji) and every other character are written as they are: raw UTF-8 takes 4 bytes for a
     * pair, an escape would take 12.
     */
    private fun quote(sb: StringBuilder, s: String): StringBuilder {
        sb.append('"')
        var i = 0
        while (i < s.length) {
            val c = s[i]
            when {
                c == '"' -> sb.append("\\\"")
                c == '\\' -> sb.append("\\\\")
                c == '\n' -> sb.append("\\n")
                c == '\r' -> sb.append("\\r")
                c == '\t' -> sb.append("\\t")
                c < ' ' -> sb.append("\\u").append(Integer.toHexString(c.code or 0x10000).substring(1))
                Character.isHighSurrogate(c) && i + 1 < s.length && Character.isLowSurrogate(s[i + 1]) -> {
                    sb.append(c).append(s[i + 1])
                    i++
                }
                c in '\uD800'..'\uDFFF' -> sb.append("\\u").append(Integer.toHexString(c.code or 0x10000).substring(1))
                else -> sb.append(c)
            }
            i++
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
            // JSON has no leading zero: "02" is damage, not a number.
            if (i - digits > 1 && s[digits] == '0') fail()
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
 * types), so JUnit can test every rule: the format and its size limit, the 7 days, the damaged copy, the
 * save marker with its operation id, what the core's answer about that operation means, the target
 * that may have changed, and the order of the writes.
 */
internal object ShareDraftRules {
    /** The format number written in every draft; a bigger one comes from a newer app (see [decode]). */
    const val FORMAT = 1

    /** A draft older than this (from the day the share arrived) is discarded at start. */
    const val MAX_AGE_MS = 7L * 24 * 60 * 60 * 1000

    /** A draft of a newer app is kept this long in the quarantine folder, then swept. */
    const val QUARANTINE_MS = 30L * 24 * 60 * 60 * 1000

    const val MSG_RECOVERED = "Rascunho recuperado."
    const val MSG_LOST = "O item recebido não pôde ser recuperado; compartilhe de novo."
    const val MSG_EXPIRED = "Um rascunho de compartilhamento com mais de 7 dias foi descartado."
    const val MSG_NEWER =
        "Há um rascunho de uma versão mais nova do aplicativo. Ele foi guardado à parte e não pôde ser aberto aqui."
    const val MSG_CANNOT_KEEP =
        "Não foi possível guardar o rascunho neste aparelho. O que você digitou continua aqui, " +
            "mas se perde se o aplicativo for fechado."
    const val MSG_CHECKING = "Conferindo se o registro já foi salvo…"
    const val MSG_CANNOT_CHECK =
        "Não foi possível confirmar se este item já tinha sido salvo. Veja em Documentos antes de salvar de novo."
    const val MSG_SAVED_BEFORE = "Este item já tinha sido salvo antes de o aplicativo fechar."
    const val MSG_CANNOT_PREPARE =
        "Não foi possível preparar o item para salvar. Nada foi salvo; o formulário continua aqui, tente de novo."
    const val MSG_TARGET_CHANGED =
        "O documento escolhido mudou ou não existe mais. Nada foi anexado; escolha o documento de novo."

    const val OP_RECORD = "record"
    const val OP_ATTACH = "attach"

    /** The "code" of the core's answer when the document to attach to is no longer the one chosen. */
    const val CODE_TARGET_CHANGED = "target_changed"

    // ------------------------------------------------------------------ the size limit

    /** Most characters kept of each typed field, so a draft stays small (the form limits the rest itself). */
    private const val MAX_FIELD = 4_000
    private const val MAX_NOTES = 100_000

    /**
     * Most characters of the received text a draft holds. The intake keeps a text to [AttachLimits.maxTextBytes]
     * (500 KB by default, at least one byte per character), so this is above any body the app takes.
     */
    private const val MAX_TEXT = 600_000

    /** The widest a character can be once written: a control is escaped to six. */
    private const val WORST_CHAR_BYTES = 6L

    /**
     * The ONE limit on the serialised draft, used by the writer (it refuses to write more) and by the reader
     * (a bigger file or text is not a draft). The draft is clamped (see [clamped]) so that even the worst case
     * is below it ([worstCaseBytes]): a draft the app writes is a draft the app reads back.
     */
    const val MAX_SERIALIZED_BYTES = 8L * 1024 * 1024

    /** What the clamped fields can take at the very most once written (every character as wide as it gets). */
    fun worstCaseBytes(): Long =
        (MAX_TEXT + MAX_NOTES).toLong() * WORST_CHAR_BYTES + 12L * MAX_FIELD * WORST_CHAR_BYTES + 4_096

    private val MODES = setOf("CHOOSE", "RECORD", "ATTACH")

    /** The start of every draft: the format number as a whole-number token, followed by "," or "}". */
    private val NEWER = Regex("""^\s*\{\s*"v"\s*:\s*(0|[1-9]\d{0,8})\s*[,}]""")
    private val UUID = Regex("""^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$""")

    /** A name made only of plain characters, to be used as a file inside the draft folder. */
    fun safeName(n: String): Boolean =
        n.isNotEmpty() && n.length <= 120 && n != "." && n != ".." &&
            n.none { it == '/' || it == '\\' || it == '\u0000' || it < ' ' } && !n.contains("..")

    fun validOperationId(id: String): Boolean = UUID.matches(id)

    private fun cut(s: String, max: Int): String = if (s.length <= max) s else s.substring(0, max)

    /** The draft with every field within what a draft may hold (what [decode] accepts). */
    fun clamped(d: ShareDraft): ShareDraft = d.copy(
        name = cut(d.name, MAX_FIELD), mime = cut(d.mime, 255), text = cut(d.text, MAX_TEXT),
        subject = cut(d.subject, MAX_FIELD), kind = cut(d.kind, MAX_FIELD), title = cut(d.title, MAX_FIELD),
        date = cut(d.date, MAX_FIELD), origin = cut(d.origin, MAX_FIELD), notes = cut(d.notes, MAX_NOTES),
        targetNumber = cut(d.targetNumber, MAX_FIELD), targetTitle = cut(d.targetTitle, MAX_FIELD),
        description = cut(d.description, MAX_FIELD)
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
            f += "sat" to it.at
            f += "sopid" to it.operationId
        }
        return FlatJson.write(f)
    }

    /** The bytes a draft takes on the disk. */
    fun encodeBytes(d: ShareDraft): ByteArray = encode(d).toByteArray(Charsets.UTF_8)

    sealed class Decoded {
        class Ok(val draft: ShareDraft) : Decoded()

        /** Not a draft of this app (damaged, cut, wrong types): treated as no draft. */
        object Corrupt : Decoded()

        /** Written by a newer app: not understood, never deleted (see [ShareDraftStore.quarantine]). */
        object Newer : Decoded()
    }

    /** Whether the start of [head] (the first bytes of a file) says "a format newer than this app's". */
    fun looksNewer(head: String): Boolean {
        val m = NEWER.find(head) ?: return false
        val v = m.groupValues[1].toLongOrNull() ?: return false
        return v > FORMAT
    }

    fun decode(text: String): Decoded {
        if (text.length > MAX_SERIALIZED_BYTES) return Decoded.Corrupt
        // The format number comes first in every draft: a bigger one is told apart before anything else is
        // judged, so a newer app's draft is never mistaken for a damaged one (and deleted). Only a whole-number
        // token counts: "2garbage" is a damaged file.
        if (looksNewer(text.take(80))) return Decoded.Newer
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
            val at = n("sat") ?: return Decoded.Corrupt
            if (op != OP_RECORD && op != OP_ATTACH) return Decoded.Corrupt
            if (op == OP_ATTACH && (copy.isEmpty() || tid <= 0)) return Decoded.Corrupt
            // A marker of the previous build has no "sopid" (its "sid" never reached the core): it is kept, with
            // no usable id, and its draft is shown as a form, never judged by a guess.
            val id = (m["sopid"] as? String)?.takeIf { validOperationId(it) } ?: ""
            saving = DraftSaving(op, id, at)
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

    /** More than 7 days since the share arrived. Decided BEFORE anything is read or hashed. */
    fun isExpired(d: ShareDraft, now: Long): Boolean = now - d.createdAt > MAX_AGE_MS

    /**
     * The first check of a draft found at start, in this order: more than 7 days old (from the day the
     * share arrived) → [Verdict.EXPIRED]; a file whose copy is gone, of another size or of another hash →
     * [Verdict.COPY_LOST]; a save that was under way → [Verdict.CHECK_FIRST] (see [recover]); else
     * [Verdict.READY]. A text needs no copy.
     */
    fun verdict(d: ShareDraft, now: Long, facts: CopyFacts?): Verdict {
        if (isExpired(d, now)) return Verdict.EXPIRED
        if (d.copy.isNotEmpty()) {
            if (facts == null || facts.size != d.size) return Verdict.COPY_LOST
            if (d.hash.isNotEmpty() && facts.hash != null && facts.hash != d.hash) return Verdict.COPY_LOST
        }
        return if (d.saving != null) Verdict.CHECK_FIRST else Verdict.READY
    }

    // ------------------------------------------------------------------ the save marker

    /** editing → saving: the marker that is written before the core is called. */
    fun begin(d: ShareDraft, op: String, operationId: String, now: Long): ShareDraft =
        d.copy(updatedAt = now, saving = DraftSaving(op, operationId, now))

    /** saving → editing: the save failed, the person still has the form. (Success clears the draft: no state.) */
    fun rollback(d: ShareDraft, now: Long): ShareDraft = d.copy(updatedAt = now, saving = null)

    /** What the core says about one operation id (see [recover]); [Unknown] when it could not be read. */
    sealed class OperationStatus {
        class Done(val documentId: Long) : OperationStatus()
        object NotDone : OperationStatus()
        object Unknown : OperationStatus()
    }

    /**
     * The core's answer {"ok":true,"done":bool,"document_id":N} as plain values: anything else (an error, an
     * answer with no "done", a core that does not know the call) is [OperationStatus.Unknown].
     */
    fun statusFrom(ok: Boolean, done: Boolean?, documentId: Long): OperationStatus = when {
        !ok || done == null -> OperationStatus.Unknown
        done -> OperationStatus.Done(documentId)
        else -> OperationStatus.NotDone
    }

    sealed class Recovery {
        /** The operation was committed: the draft is over; open [documentId] (0: unknown) and say so. */
        class Saved(val documentId: Long) : Recovery()

        /**
         * The form is back, ready to save. [reuseId]: the operation id a save of the SAME form reuses (the core
         * does nothing twice for it), or null. [unconfirmed]: the discreet note that this could not be confirmed.
         * [keepMarker]: the marker stays on the draft (the next start looks again).
         */
        class Form(val reuseId: String?, val unconfirmed: Boolean, val keepMarker: Boolean) : Recovery()
    }

    /**
     * What becomes of a restored draft. A draft that was only being edited is the form. A draft that was saving
     * is cleared ONLY when the core says its operation was committed; "not committed" is the form, ready to save
     * again with the same id; an unreadable status (error, older core) keeps the draft and its copy and shows the
     * form with a note; a marker with no usable id (previous build) is the form, with the note, nothing guessed.
     */
    fun recover(d: ShareDraft, status: OperationStatus?): Recovery {
        val s = d.saving ?: return Recovery.Form(null, unconfirmed = false, keepMarker = false)
        if (!s.usable) return Recovery.Form(null, unconfirmed = true, keepMarker = false)
        return when (status) {
            is OperationStatus.Done ->
                Recovery.Saved(if (status.documentId > 0) status.documentId else if (s.op == OP_ATTACH) d.targetId else 0L)
            OperationStatus.NotDone -> Recovery.Form(s.operationId, unconfirmed = false, keepMarker = false)
            else -> Recovery.Form(s.operationId, unconfirmed = true, keepMarker = true)
        }
    }

    // ------------------------------------------------------------------ the operation id of a save

    /** The operation a form's save used, so the same form saved again reuses its id. */
    class PendingOp(val op: String, val id: String, val fingerprint: String)

    /** Everything the call to the core is made of: the same fingerprint means the same call. */
    fun fingerprint(d: ShareDraft, op: String): String = listOf(
        op, d.copy, d.name, d.size.toString(), d.hash, d.text, d.kind, d.title, d.date, d.origin, d.notes,
        d.targetId.toString(), d.targetNumber, d.description
    ).joinToString("\u0000")

    /** [prev] when this is the same call again (the first never failed for good), else a new operation id. */
    fun operationFor(prev: PendingOp?, op: String, fingerprint: String, fresh: () -> String): PendingOp =
        if (prev != null && prev.op == op && prev.fingerprint == fingerprint && validOperationId(prev.id)) prev
        else PendingOp(op, fresh(), fingerprint)

    // ------------------------------------------------------------------ the target of an attach

    enum class TargetCheck { OK, CHANGED, UNKNOWN }

    /**
     * A restored target against the document as the library has it now: [found] is the number read (null when it
     * could not be read: unknown, the core's own check at save time decides). A draft with no number kept has
     * nothing to compare.
     */
    fun targetCheck(expectedNumber: String, found: String?): TargetCheck = when {
        expectedNumber.isEmpty() -> TargetCheck.UNKNOWN
        found == null -> TargetCheck.UNKNOWN
        found == expectedNumber -> TargetCheck.OK
        else -> TargetCheck.CHANGED
    }

    fun isTargetChanged(code: String?): Boolean = code == CODE_TARGET_CHANGED

    // ------------------------------------------------------------------ the order of the writes

    /** How long the main thread waits, at most, for the last write when the app goes to the background. */
    const val STOP_WAIT_MS = 300L

    /** How long a save waits for its marker to reach the disk before it gives up (and does not call the core). */
    const val MARKER_WAIT_MS = 15_000L

    /**
     * The core is called ONLY after the marker snapshot is on the disk: [markerDurable] says whether it is, and
     * when it is not the core is never called ([PonteException] with [MSG_CANNOT_PREPARE]).
     */
    fun <T> callAfterMarker(markerDurable: () -> Boolean, call: () -> T): T {
        if (!markerDurable()) throw PonteException(MSG_CANNOT_PREPARE)
        return call()
    }

    /** Waits for the write behind [f]: true only when it reached the disk. */
    fun durable(f: Future<ShareDraftStore.Wrote>, waitMs: Long = MARKER_WAIT_MS): Boolean =
        try {
            f.get(waitMs, TimeUnit.MILLISECONDS) == ShareDraftStore.Wrote.OK
        } catch (_: Throwable) {
            false
        }

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
 * Every write, removal and start of the draft folder runs here, ONE at a time, in the order it was asked.
 * Together with the revision of each snapshot (see [ShareDraftStore.writeRevision]) this is what keeps an
 * older snapshot from ever landing over a newer one. [exec] must run its jobs one after the other.
 */
internal class DraftWriter(private val store: ShareDraftStore, private val exec: Executor) {
    /** Writes [d] as revision [rev]; [done] (if any) is told the outcome on the writer's thread. */
    fun write(d: ShareDraft, rev: Long, done: ((ShareDraftStore.Wrote) -> Unit)? = null): Future<ShareDraftStore.Wrote> {
        val t = FutureTask<ShareDraftStore.Wrote> {
            val w = store.writeRevision(d, rev)
            try { done?.invoke(w) } catch (_: Throwable) { }
            w
        }
        exec.execute(t)
        return t
    }

    fun <T> submit(job: () -> T): Future<T> {
        val t = FutureTask<T> { job() }
        exec.execute(t)
        return t
    }

    /** The draft first, then its copy: what a saved, cancelled or replaced share leaves behind is nothing. */
    fun clear(copyName: String?): Future<Unit> = submit { store.clear(copyName) }
}

/**
 * The draft folder (inside the app's private files, never the cache, never external storage, not served by
 * the provider, not in any backup rule): the draft file and the private copy of the received item.
 * Plain `java.io`, so it is tested with a temporary folder.
 */
internal class ShareDraftStore(
    private val dir: File,
    private val maxBytes: Long = ShareDraftRules.MAX_SERIALIZED_BYTES
) {
    companion object {
        /** Folder inside `filesDir`. */
        const val DIR = "rascunho"
        const val FILE = "rascunho.json"
        const val TMP = "rascunho.json.tmp"

        /** Inside [DIR]: what a newer app wrote, kept for it and swept after 30 days. */
        const val QUARANTINE = "quarentena"

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

    /** What a write came to: on the disk, refused as older than what is there, or not possible. */
    enum class Wrote { OK, STALE, FAILED }

    sealed class Start {
        /** No draft. */
        object None : Start()

        /** A newer app's draft is there and could not be moved aside: untouched, and nothing of ours is written. */
        object Blocked : Start()

        /** The draft is gone (with its copy), or was put aside; [message] is to be told once, or null. */
        class Dropped(val message: String?) : Start()

        /** The draft to put back on the screen; [checkFirst] when a save was under way. */
        class Resume(val draft: ShareDraft, val checkFirst: Boolean) : Start()
    }

    /** The highest revision on the disk (in memory: the revisions of a run only ever grow). */
    private var lastRev = Long.MIN_VALUE

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
            if (f.length() > maxBytes) {
                // Too big for this app to read: still a newer app's draft when it says so at its start.
                val head = f.inputStream().use { input ->
                    val buf = ByteArray(100)
                    val n = input.read(buf)
                    String(buf, 0, if (n > 0) n else 0, Charsets.UTF_8)
                }
                return if (ShareDraftRules.looksNewer(head)) Read.Newer else Read.Corrupt
            }
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
     * Writes the draft atomically, whatever its revision (used where there is no ordering to keep): see
     * [writeRevision]. False when it could not be done.
     */
    fun write(d: ShareDraft): Boolean = writeAt(d, null) == Wrote.OK

    /**
     * Writes the draft as revision [rev]: a revision LOWER than the last one written is refused ([Wrote.STALE],
     * nothing touched), so an older snapshot that arrives late never replaces a newer one (the save marker
     * above all). Atomic: to a temporary file, flushed to the disk, then renamed over the old one, so a reader
     * sees the old draft or the new one, never half of one. [Wrote.FAILED] when it could not be done (no room,
     * no folder, a draft bigger than the one limit): the old draft, if any, is left as it was.
     */
    fun writeRevision(d: ShareDraft, rev: Long): Wrote = writeAt(d, rev)

    private fun writeAt(d: ShareDraft, rev: Long?): Wrote = synchronized(lock) {
        if (rev != null && rev < lastRev) return Wrote.STALE
        val tmp = File(dir, TMP)
        try {
            dir.mkdirs()
            val bytes = ShareDraftRules.encodeBytes(d)
            if (bytes.size > maxBytes) return Wrote.FAILED
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
            if (rev != null) lastRev = rev
            Wrote.OK
        } catch (_: Throwable) {
            try { tmp.delete() } catch (_: Throwable) { }
            Wrote.FAILED
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

    /**
     * What is found of the copy named [name]; null when it is not there. The hash is worked out only on request,
     * by [hash].
     */
    fun facts(name: String, withHash: Boolean, hash: (File) -> String = ::hashOf): CopyFacts? {
        if (!ShareDraftRules.safeName(name)) return null
        val f = File(dir, name)
        if (!f.isFile) return null
        return try {
            CopyFacts(f.length(), if (withHash) hash(f) else null)
        } catch (_: Throwable) {
            null
        }
    }

    /**
     * Deletes every file of the folder except the draft file and the copy [live]: what a share left behind
     * (a copy whose draft was never written, a half-written draft). Never the live copy, never the quarantine.
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

    /**
     * Puts aside what a NEWER app wrote: every file of the folder (its draft and its copy, whatever their names)
     * moves into `quarentena/<moment>/` with its own name, so the newer version can find it and the active slot
     * is free for a new draft. Nothing is deleted. True when the draft file is no longer in the active slot.
     */
    fun quarantine(now: Long): Boolean = synchronized(lock) {
        try {
            val files = dir.listFiles()?.filter { it.isFile } ?: return true
            if (files.isEmpty()) return true
            var stamp = now
            val root = File(dir, QUARANTINE)
            while (File(root, stamp.toString()).exists()) stamp++
            val box = File(root, stamp.toString())
            if (!box.mkdirs()) return false
            for (f in files) {
                try {
                    Files.move(f.toPath(), File(box, f.name).toPath())
                } catch (_: Throwable) {
                }
            }
            try { box.setLastModified(now) } catch (_: Throwable) { }
            !file().exists()
        } catch (_: Throwable) {
            false
        }
    }

    /** Removes what has been in the quarantine for more than 30 days. */
    fun sweepQuarantine(now: Long) {
        synchronized(lock) {
            try {
                val root = File(dir, QUARANTINE)
                root.listFiles()?.forEach {
                    if (now - it.lastModified() > ShareDraftRules.QUARANTINE_MS) {
                        try { it.deleteRecursively() } catch (_: Throwable) { }
                    }
                }
                if (root.isDirectory && root.list().isNullOrEmpty()) root.delete()
            } catch (_: Throwable) {
            }
        }
    }

    /** The whole folder, the quarantine too: used when the phone's data is reset. */
    fun wipe() {
        synchronized(lock) {
            try {
                dir.deleteRecursively()
            } catch (_: Throwable) {
            }
        }
    }

    /**
     * What to do with the folder at start, and the cleaning that goes with it. Never throws: whatever is
     * wrong with the draft means "no draft". A draft of a newer app is put aside, never deleted. An expired
     * draft is dropped before its copy is looked at (and hashed); [hash] works out the hash of a copy.
     */
    fun start(now: Long, setUp: Boolean, hash: (File) -> String = ::hashOf): Start {
        try {
            sweepQuarantine(now)
            when (val r = read()) {
                Read.Absent -> {
                    sweep(null)
                    return Start.None
                }
                Read.Newer -> {
                    return if (quarantine(now)) Start.Dropped(ShareDraftRules.MSG_NEWER) else Start.Blocked
                }
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
                    // The age comes first: an old draft is never read through (a big copy is not hashed for nothing).
                    if (ShareDraftRules.isExpired(d, now)) {
                        clear(d.copy)
                        sweep(null)
                        return Start.Dropped(ShareDraftRules.MSG_EXPIRED)
                    }
                    val facts = if (d.copy.isEmpty()) null else facts(d.copy, withHash = d.hash.isNotEmpty(), hash = hash)
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
