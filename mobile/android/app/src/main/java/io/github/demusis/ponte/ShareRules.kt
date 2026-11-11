package io.github.demusis.ponte

import java.io.InputStream
import java.io.OutputStream

/**
 * The limits of the core that the phone needs before anything reaches it (the streaming cap, the
 * texts of the dialog). The defaults are the core's numbers today; the real ones arrive with
 * `choices()` (see [Api.choices]) and replace them, so a change in the core is followed here.
 */
internal object AttachLimits {
    @Volatile var maxBytes: Long = 8L * 1024 * 1024
    @Volatile var maxNameChars: Int = 180
    @Volatile var maxDescriptionChars: Int = 500
    @Volatile var maxTextBytes: Int = 500 * 1024
    @Volatile var maxTitleBytes: Int = 1000

    /** "8 MB", for the sentences shown to the person. */
    fun maxLabel(): String = (maxBytes / (1024 * 1024)).toString() + " MB"

    /** "Até 8 MB. Programas e arquivos compactados não são aceitos." */
    fun upFront(): String = "Até " + maxLabel() + ". Programas e arquivos compactados não são aceitos."
}

/** More bytes than allowed were read from a stream; nothing past the limit was written. */
internal class TooBigException(val limit: Long) : Exception("Passou do limite de $limit bytes.")

/**
 * What is decided about something that arrives from another app, as plain functions of plain
 * values (no Android types), so JUnit can test every rule. The content of an intent is never
 * trusted: these rules only say whether it is worth reading at all; the core still judges the
 * file (name, type, content) when it is stored.
 */
internal object ShareRules {
    const val ACTION_SEND = "android.intent.action.SEND"
    const val ACTION_SEND_MULTIPLE = "android.intent.action.SEND_MULTIPLE"

    const val MSG_MULTIPLE = "Compartilhe um arquivo de cada vez."
    const val MSG_NOT_SENT = "O Ponte de bolso não recebe este tipo de compartilhamento."
    const val MSG_NOT_CONTENT = "Não foi possível receber este arquivo do aplicativo que o enviou."
    const val MSG_OWN_FILE = "Este arquivo é do próprio Ponte de bolso e não pode ser guardado de novo."
    const val MSG_EMPTY = "Não há nada para guardar neste compartilhamento."
    const val MSG_BAD_TYPE = "Não foi possível receber este compartilhamento."
    const val MSG_NOT_SET_UP = "Configure o Ponte de bolso antes de receber arquivos."

    enum class Reason { OK, MULTIPLE, NOT_SENT, NOT_CONTENT, OWN_FILE, EMPTY, BAD_TYPE }

    data class Verdict(val reason: Reason, val message: String) {
        val ok get() = reason == Reason.OK
    }

    private val OK = Verdict(Reason.OK, "")

    /**
     * Whether a share is accepted. [scheme] and [authority] are those of the one stream Uri, null
     * when there is none; [streamCount] is how many streams came (a clip with several items, or a
     * list); [mime] is only the sender's hint. [ownAuthority] is the authority of this app's file
     * provider.
     *
     * | input                                                | answer                         |
     * |------------------------------------------------------|--------------------------------|
     * | action SEND_MULTIPLE, or more than one stream        | refused: [MSG_MULTIPLE]        |
     * | any action other than SEND                           | refused: [MSG_NOT_SENT]        |
     * | MIME with spaces, controls or over 255 characters    | refused: [MSG_BAD_TYPE]        |
     * | a stream whose scheme is not content (file, http...) | refused: [MSG_NOT_CONTENT]     |
     * | a content stream without an authority                | refused: [MSG_NOT_CONTENT]     |
     * | a stream whose authority is this app's provider      | refused: [MSG_OWN_FILE]        |
     * | no stream and no text                                | refused: [MSG_EMPTY]           |
     * | one content stream (any MIME), or only text          | accepted                       |
     */
    fun check(
        action: String?, mime: String?, scheme: String?, authority: String?,
        streamCount: Int, hasText: Boolean, ownAuthority: String
    ): Verdict {
        if (action == ACTION_SEND_MULTIPLE || streamCount > 1) return Verdict(Reason.MULTIPLE, MSG_MULTIPLE)
        if (action != ACTION_SEND) return Verdict(Reason.NOT_SENT, MSG_NOT_SENT)
        if (mime != null && !plausibleMime(mime)) return Verdict(Reason.BAD_TYPE, MSG_BAD_TYPE)
        val hasStream = streamCount == 1 || scheme != null || authority != null
        if (hasStream) {
            if (scheme == null || !scheme.equals("content", ignoreCase = true)) {
                return Verdict(Reason.NOT_CONTENT, MSG_NOT_CONTENT)
            }
            val raw = authority?.trim().orEmpty()
            if (raw.isEmpty()) return Verdict(Reason.NOT_CONTENT, MSG_NOT_CONTENT)
            if (isOwnAuthority(raw, ownAuthority)) return Verdict(Reason.OWN_FILE, MSG_OWN_FILE)
            return OK
        }
        if (!hasText) return Verdict(Reason.EMPTY, MSG_EMPTY)
        return OK
    }

    /** type/subtype made of the usual characters; the type is only a hint, never a decision. */
    fun plausibleMime(mime: String): Boolean =
        mime.length <= 255 && mime.isNotEmpty() &&
            mime.none { it.isWhitespace() || Character.isISOControl(it) } && mime.count { it == '/' } <= 1

    /**
     * True when [raw] (the authority of a Uri, which may carry a user id before an "@" or a port)
     * names this app's own provider. Compared without capitals and without those extras, and,
     * to be safe, also when the own authority appears anywhere in it.
     */
    fun isOwnAuthority(raw: String, own: String): Boolean {
        val low = raw.lowercase()
        val ownLow = own.lowercase()
        val host = low.substringAfterLast('@').substringBefore(':').trim()
        return host == ownLow || low.contains(ownLow)
    }

    /**
     * True when a real path (read from an open file descriptor) lies inside one of this app's
     * private folders: a provider that hands out our own files must not be followed.
     */
    fun insideDirs(real: String?, dirs: List<String>): Boolean {
        if (real == null) return false
        val p = real.trimEnd('/')
        return dirs.any { d ->
            val dir = d.trimEnd('/')
            dir.isNotEmpty() && (p == dir || p.startsWith("$dir/"))
        }
    }

    /** The sender says it is an image: only then may a file past the limit be shrunk like a photo. */
    fun looksLikeImage(mime: String?): Boolean = mime != null && mime.lowercase().startsWith("image/")

    // ------------------------------------------------------------------ names and texts

    private fun badChar(cp: Int): Boolean {
        val t = Character.getType(cp)
        return Character.isISOControl(cp) || t == Character.FORMAT.toInt() ||
            t == Character.LINE_SEPARATOR.toInt() || t == Character.PARAGRAPH_SEPARATOR.toInt() ||
            t == Character.SURROGATE.toInt()
    }

    private fun codePoints(s: String): IntArray = s.codePoints().toArray()

    private fun fromCodePoints(cps: List<Int>): String {
        val sb = StringBuilder()
        for (cp in cps) sb.appendCodePoint(cp)
        return sb.toString()
    }

    /**
     * A display name given by another app, made safe to hand to the core: only the last part
     * after any "/" or "\", no control, invisible or direction-changing characters, no spaces or
     * dots at the ends, and at most [maxChars] characters, keeping the extension when it is short.
     * Empty when nothing is left; the core's own validation judges the rest (type, length rules).
     */
    fun cleanName(raw: String?, maxChars: Int): String {
        if (raw == null) return ""
        val last = raw.substringAfterLast('/').substringAfterLast('\\')
        val cps = codePoints(last).filter { !badChar(it) }
        var name = fromCodePoints(cps).trim().trimEnd('.', ' ').trimStart('.')
        name = name.trim()
        val points = codePoints(name)
        if (points.size <= maxChars) return name
        val dot = name.lastIndexOf('.')
        val ext = if (dot > 0 && name.length - dot <= 10) name.substring(dot) else ""
        val extPoints = codePoints(ext).size
        val keep = (maxChars - extPoints).coerceAtLeast(1)
        return fromCodePoints(points.take(keep)).trimEnd('.', ' ') + ext
    }

    private val HAS_EXTENSION = Regex("""\.[A-Za-z0-9]{1,8}$""")

    /**
     * When the sender gave a name without extension (or none), the one its MIME hint suggests is
     * added ([extFor] maps a MIME type to an extension, or null): a gallery photo named "IMG_2031"
     * becomes "IMG_2031.jpg". A name that already has an extension is never changed.
     */
    fun withExtension(name: String, mime: String?, extFor: (String) -> String?): String {
        val base = name.ifEmpty { "compartilhado" }
        if (HAS_EXTENSION.containsMatchIn(base) && name.isNotEmpty()) return name
        val ext = mime?.let { extFor(it.lowercase().substringBefore(';').trim()) }?.trim('.')?.lowercase()
        if (ext.isNullOrEmpty() || !ext.all { it.isLetterOrDigit() }) return name
        return "$base.$ext"
    }

    /** The name with its extension swapped (a big photo shrunk to JPEG is no longer a .png). */
    fun replaceExtension(name: String, ext: String): String {
        val stem = if (HAS_EXTENSION.containsMatchIn(name)) name.substringBeforeLast('.') else name
        return (stem.ifEmpty { "foto" }) + "." + ext.trim('.')
    }

    /** A title for a new record from a file name: the name without its extension. */
    fun titleFromName(name: String): String =
        if (HAS_EXTENSION.containsMatchIn(name)) name.substringBeforeLast('.').trim() else name.trim()

    /**
     * Text received from another app, as plain text: line ends unified, controls (except line
     * break and tab) and invisible characters dropped, cut to at most [maxBytes] of UTF-8 on a
     * character boundary. Never interpreted as markup or as an address to fetch.
     */
    fun plainText(raw: String?, maxBytes: Int): String {
        if (raw == null) return ""
        val unified = raw.replace("\r\n", "\n").replace('\r', '\n')
        val kept = codePoints(unified).filter { it == '\n'.code || it == '\t'.code || !badChar(it) }
        return cutBytes(fromCodePoints(kept).trim(), maxBytes)
    }

    /** A subject or title: one line (breaks become spaces), same cleaning, cut to [maxBytes]. */
    fun oneLine(raw: String?, maxBytes: Int): String {
        val flat = plainText(raw, Int.MAX_VALUE).replace(Regex("""\s+"""), " ").trim()
        return cutBytes(flat, maxBytes).trim()
    }

    /** At most [maxBytes] bytes of UTF-8, never in the middle of a character. */
    fun cutBytes(s: String, maxBytes: Int): String {
        if (maxBytes <= 0) return ""
        var bytes = 0
        val sb = StringBuilder()
        for (cp in codePoints(s)) {
            val n = when {
                cp < 0x80 -> 1
                cp < 0x800 -> 2
                cp < 0x10000 -> 3
                else -> 4
            }
            if (bytes + n > maxBytes) break
            bytes += n
            sb.appendCodePoint(cp)
        }
        return sb.toString()
    }

    /** At most [max] characters of a description typed by the person. */
    fun capChars(s: String, max: Int): String {
        val cps = codePoints(s)
        return if (cps.size <= max) s else fromCodePoints(cps.take(max))
    }

    /** The first lines of a text, for the card that shows what arrived. */
    fun preview(text: String, lines: Int = 6, chars: Int = 400): String {
        val head = text.lineSequence().filter { it.isNotBlank() }.take(lines).joinToString("\n") { it.trim() }
        val cps = codePoints(head)
        val cut = cps.size > chars || text.lineSequence().filter { it.isNotBlank() }.count() > lines
        return (if (cps.size > chars) fromCodePoints(cps.take(chars)).trimEnd() else head) + (if (cut) "…" else "")
    }

    /** The sentence for a file past the limit (the size is not known: the copy stopped at the limit). */
    fun tooBigMessage(name: String, limitBytes: Long): String =
        "O arquivo " + (if (name.isEmpty()) "" else "“$name” ") + "passa do limite de " + (limitBytes / (1024 * 1024)) + " MB. " +
            "Reduza o arquivo (PDF com compressão, imagem em resolução menor) ou divida-o em partes e envie uma de cada vez."
}

/** Copying a stream under a hard cap. */
internal object BoundedCopy {
    private const val BUFFER = 64 * 1024

    /**
     * Copies [input] to [output], never writing more than [limit] bytes: as soon as one byte past
     * the limit has been read, it stops with [TooBigException] (the extra byte is not written, and
     * the rest of the stream is never read). Returns the number of bytes copied.
     */
    fun copy(input: InputStream, output: OutputStream, limit: Long): Long {
        val buf = ByteArray(BUFFER)
        var total = 0L
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            if (total + n > limit) {
                val room = (limit - total).toInt()
                if (room > 0) output.write(buf, 0, room)
                throw TooBigException(limit)
            }
            output.write(buf, 0, n)
            total += n
        }
        output.flush()
        return total
    }
}
