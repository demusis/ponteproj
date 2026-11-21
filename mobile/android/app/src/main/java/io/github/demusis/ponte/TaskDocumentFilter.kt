package io.github.demusis.ponte

/** null: all documents; 0: tasks without a document; positive: a local document id. */
internal object TaskDocumentFilter {
    /** How many documents the filter sheet lists at once; the search narrows the rest. */
    const val MAX_SHOWN = 30

    /** The search box of the sheet shows only when the library has more documents than this. */
    const val SEARCH_FROM = 8

    fun showSearch(docs: List<Doc>) = docs.size > SEARCH_FROM

    /**
     * What the sheet lists. Without a search: the chosen one first (whatever it has), then the documents with open
     * tasks. With a search: every match, the ones with open tasks first.
     */
    fun sheetOptions(docs: List<Doc>, query: String, chosen: Long?): List<Doc> {
        val matches = options(docs, query)
        return if (query.isBlank()) matches.filter { it.openTasks > 0 || it.id == chosen }.sortedBy { it.id != chosen }
        else matches.sortedBy { it.openTasks == 0 }
    }

    /** What the closed section shows: the title of the chosen document (its number when it has no title). */
    fun shortLabel(chosen: Long?, docs: List<Doc>): String = when (chosen) {
        null, 0L -> label(chosen, docs)
        else -> docs.find { it.id == chosen }?.let { headline(it) } ?: label(chosen, docs)
    }

    /** The second line of a document in the sheet: "MANUAL-2026/0006 · 10 tarefas". */
    fun detail(doc: Doc): String {
        val tasks = when (doc.openTasks) {
            0 -> "sem tarefas em aberto"
            1 -> "1 tarefa"
            else -> "${doc.openTasks} tarefas"
        }
        return listOf(doc.number, tasks).filter { it.isNotBlank() }.joinToString(" · ")
    }

    /** The first line: the title, or the number of a document without one. */
    fun headline(doc: Doc): String = doc.title.ifBlank { doc.number }

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
