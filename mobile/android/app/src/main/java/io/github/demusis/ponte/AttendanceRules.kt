package io.github.demusis.ponte

/** What the Atendimentos list does with the rows the core sent (already ordered). Plain, so it is tested. */
internal object AttendanceRules {
    /** The open rows and, separately, the closed ones, both in the order the core gave. */
    data class Split(val open: List<AttendanceItem>, val closed: List<AttendanceItem>)

    /**
     * Every word of [query] must appear in the reference, subject, responsible person or
     * observation (capitals and accents do not matter), as in the search of the computer.
     */
    fun matches(a: AttendanceItem, query: String): Boolean {
        val words = foldText(query).split(' ').filter { it.isNotEmpty() }
        if (words.isEmpty()) return true
        val haystack = foldText(listOf(a.reference, a.subject, a.assignee, a.observation).joinToString("\n"))
        return words.all { haystack.contains(it) }
    }

    fun split(rows: List<AttendanceItem>, query: String): Split {
        val found = rows.filter { matches(it, query) }
        return Split(found.filter { it.open }, found.filter { !it.open })
    }

    /** "3 em aberto" or "Nenhum em aberto". */
    fun openLine(open: Int): String = if (open <= 0) "Nenhum em aberto" else "$open em aberto"
}

/** The rules of the search screen that do not need Android. */
internal object SearchRules {
    /** Fewer characters than this do not search. */
    const val MIN_CHARS = 2

    /** The pause after the last keystroke before a search starts. */
    const val DEBOUNCE_MS = 300L

    fun usable(query: String): Boolean = query.trim().length >= MIN_CHARS

    /**
     * Whether the answer of search number [gen] may be shown: it is the latest one asked for, the app
     * is not locked and the search screen is still open. Anything else is dropped.
     */
    fun accept(gen: Int, latest: Int, locked: Boolean, screenOpen: Boolean): Boolean =
        gen == latest && !locked && screenOpen

    /** "e mais 3" under a group that was cut. */
    fun moreLine(more: Int): String = "e mais $more"
}
