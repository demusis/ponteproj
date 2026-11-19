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

/** The cobranças of today (follow-ups): what the screen shows and sends, plain so it is tested. */
internal object FollowUpRules {
    /** What the contact says when nobody chose another one (the computer's default). */
    const val DEFAULT_CHANNEL = "Cobrado por telefone"

    /** Quick choices for the next date: label and days from today. */
    val PRESETS: List<Pair<String, Long>> = listOf("Amanhã" to 1L, "Em 3 dias" to 3L, "Em 1 semana" to 7L)

    /** The days the date field starts at. */
    const val DEFAULT_NEXT_DAYS = 3L

    /** The text of the contact: the way chosen, then the note when there is one ("Cobrado por e-mail: sem resposta"). */
    fun contactText(channel: String, note: String): String {
        val way = channel.trim().ifEmpty { DEFAULT_CHANNEL }
        val extra = note.trim()
        return if (extra.isEmpty()) way else "$way: $extra"
    }

    /** `AAAA-MM-DD` of [today] plus [days]. */
    fun dateIn(today: java.time.LocalDate, days: Long): String = today.plusDays(days).toString()

    /** The next date must be a real date after [today] (the core refuses anything else). */
    fun nextDateOk(nextIso: String, today: java.time.LocalDate): Boolean =
        try { java.time.LocalDate.parse(nextIso.trim()).isAfter(today) } catch (_: Throwable) { false }

    /** The chip of a row: "Hoje" or "Atrasada 3 dias". */
    fun chip(daysOverdue: Int): String = when {
        daysOverdue <= 0 -> "Hoje"
        daysOverdue == 1 -> "Atrasada 1 dia"
        else -> "Atrasada $daysOverdue dias"
    }

    /** "Cobranças de hoje · 3". */
    fun title(count: Int): String = "Cobranças de hoje · $count"

    /** What the core's answer on "Registrar" and "Marcar" says on the phone. */
    fun registeredMessage(nextIso: String): String =
        if (nextIso.isBlank()) "Cobrança registrada." else "Cobrança registrada. Próxima em ${formatDocDate(nextIso)}."

    fun scheduledMessage(nextIso: String): String = "Próxima cobrança marcada para ${formatDocDate(nextIso)}."
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
