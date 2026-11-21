package io.github.demusis.ponte

/** null: all documents; 0: tasks without a document; positive: a local document id. */
internal object TaskDocumentFilter {
    /** How many documents the filter sheet lists at once; the search narrows the rest. */
    const val MAX_SHOWN = 30

    fun matches(documentId: Long, chosen: Long?): Boolean = when (chosen) {
        null -> true
        0L -> documentId <= 0
        else -> documentId == chosen
    }

    fun label(chosen: Long?, docs: List<Doc>): String = when (chosen) {
        null -> ""
        0L -> "Sem documento"
        else -> docs.find { it.id == chosen }?.let { label(it) } ?: "Documento indisponível"
    }

    fun label(doc: Doc): String = listOf(doc.number, doc.title).filter { it.isNotBlank() }.joinToString(" · ")

    fun options(docs: List<Doc>, query: String): List<Doc> {
        val terms = CategoryRules.nameKey(query).split(Regex("\\s+")).filter { it.isNotEmpty() }
        return docs.filter { doc -> terms.all { it in CategoryRules.nameKey(label(doc)) } }
            .sortedWith(compareBy({ CategoryRules.nameKey(it.title) }, { it.number }, { it.id }))
    }

    // Sync can replace local ids. Clear instead of accidentally selecting a different document.
    fun afterRefresh(chosen: Long?, before: List<Doc>, after: List<Doc>): Long? {
        if (chosen == null || chosen == 0L) return chosen
        val old = before.find { it.id == chosen } ?: return null
        return chosen.takeIf { after.any { it.id == chosen && it.number == old.number && it.title == old.title } }
    }
}
