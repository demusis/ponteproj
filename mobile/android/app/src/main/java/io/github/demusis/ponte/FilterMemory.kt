package io.github.demusis.ponte

import java.security.MessageDigest

/** The document the Tarefas filter was on, as it is kept between runs: the local id and a fingerprint of number and title. */
internal data class SavedDocument(val id: Long, val fingerprint: String)

/**
 * What the lists remember between runs: the category, project and document filters of Tarefas, the category
 * filter and the chip of Documentos, and the area chosen in the Tarefas tab. The text typed in a search is not kept.
 */
internal data class SavedFilters(
    val taskCategories: Set<String> = emptySet(),
    val docCategories: Set<String> = emptySet(),
    val project: String = "",
    /** null: no document filter; id 0 (no fingerprint): "Sem documento". */
    val document: SavedDocument? = null,
    val segment: Int = TasksSegments.TASKS,
    val docFilter: DocFilter = DocFilter.ALL
)

/**
 * Writing and reading what [SavedFilters] keeps (one text, in the app's preferences), and checking a saved
 * document against the library of the moment. Plain values, no Android: tested with plain JUnit.
 *
 * The document filter holds a LOCAL id, and local ids can change after a sync: the text keeps the id and a
 * fingerprint of the number and title (not the words, so the preferences hold no document title); a restore
 * accepts the id only when that document still has the same fingerprint, else the one document that has it.
 */
internal object FilterMemory {
    const val KEY = "filters"
    private const val VERSION = "v1"

    /** The same number and title, as the document filter compares them after a sync (exact text, blanks around ignored). */
    fun fingerprint(number: String, title: String): String {
        val bytes = MessageDigest.getInstance("SHA-256").digest("${number.trim()}\n${title.trim()}".toByteArray(Charsets.UTF_8))
        return bytes.joinToString("") { "%02x".format(it) }.take(32)
    }

    fun fingerprint(doc: Doc) = fingerprint(doc.number, doc.title)

    /** What to keep for the filter [chosen] (see [TaskDocumentFilter]) against the documents [docs] known now. */
    fun savedDocument(chosen: Long?, docs: List<Doc>): SavedDocument? = when (chosen) {
        null -> null
        0L -> SavedDocument(0L, "")
        else -> docs.find { it.id == chosen }?.let { SavedDocument(it.id, fingerprint(it)) }
    }

    /**
     * The filter to put back: "Sem documento" always; a document when the one with that id is still the same
     * (number and title), or else when exactly one document has them; otherwise none (never a different document).
     */
    fun restoreDocument(saved: SavedDocument?, docs: List<Doc>): Long? {
        if (saved == null) return null
        if (saved.id == 0L) return 0L
        if (saved.fingerprint.isEmpty()) return null
        docs.find { it.id == saved.id && fingerprint(it) == saved.fingerprint }?.let { return it.id }
        return docs.filter { fingerprint(it) == saved.fingerprint }.singleOrNull()?.id
    }

    // A value never carries a line break or the separators below.
    private fun clean(value: String) = value.filter { it >= ' ' && it != ',' && it != ':' }

    private fun uids(set: Set<String>) = set.map(::clean).filter { it.isNotEmpty() }.sorted().joinToString(",")

    fun encode(f: SavedFilters): String = listOf(
        VERSION,
        "tc=" + uids(f.taskCategories),
        "dc=" + uids(f.docCategories),
        "pj=" + clean(f.project),
        "dd=" + (f.document?.let { if (it.id == 0L) "0" else "${it.id}:${clean(it.fingerprint)}" } ?: ""),
        "sg=${f.segment}",
        "df=${f.docFilter.name}"
    ).joinToString("\n")

    /** What [text] keeps; an absent, unknown (newer) or damaged text, or a damaged line, gives the default for it. */
    fun decode(text: String?): SavedFilters {
        if (text.isNullOrEmpty()) return SavedFilters()
        val lines = text.split("\n")
        if (lines.firstOrNull() != VERSION) return SavedFilters()
        val values = lines.drop(1).mapNotNull { line ->
            val at = line.indexOf('=')
            if (at <= 0) null else line.substring(0, at) to line.substring(at + 1)
        }.toMap()
        fun set(key: String) = values[key].orEmpty().split(",").filter { it.isNotEmpty() }.toSet()
        val document = values["dd"].orEmpty().let { raw ->
            val id = raw.substringBefore(':').toLongOrNull()
            when {
                raw.isEmpty() || id == null || id < 0 -> null
                id == 0L -> if (raw == "0") SavedDocument(0L, "") else null
                raw.contains(':') -> SavedDocument(id, raw.substringAfter(':')).takeIf { it.fingerprint.isNotEmpty() }
                else -> null
            }
        }
        return SavedFilters(
            taskCategories = set("tc"),
            docCategories = set("dc"),
            project = values["pj"].orEmpty(),
            document = document,
            segment = (values["sg"]?.toIntOrNull() ?: TasksSegments.TASKS).coerceIn(0, TasksSegments.PROJECTS),
            docFilter = try { DocFilter.valueOf(values["df"].orEmpty()) } catch (_: Throwable) { DocFilter.ALL }
        )
    }
}

/** Whether a return to the app should start a sync of its own. */
internal object ForegroundSync {
    /** A sync that ended well this long ago (or longer) makes the next return start another. */
    const val STALE_MS = 5 * 60_000L

    /**
     * True when the app is back in front, [configured] (a file and a passphrase ready, the core open, setup over),
     * not [locked], nothing already [syncing], and the last good sync ([lastSync], 0: never) is [STALE_MS] old or more.
     * [lastAttempt] is the last time a return asked for a sync: one that failed (offline, say) is not asked again
     * before [STALE_MS], so coming and going never repeats the same error. A clock set back counts as old.
     */
    fun shouldSync(
        now: Long, lastSync: Long, locked: Boolean, syncing: Boolean, configured: Boolean, lastAttempt: Long = 0L
    ): Boolean {
        if (!configured || locked || syncing) return false
        if (lastAttempt > 0L && now >= lastAttempt && now - lastAttempt < STALE_MS) return false
        return lastSync <= 0L || now < lastSync || now - lastSync >= STALE_MS
    }
}
