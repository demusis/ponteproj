package io.github.demusis.ponte

import java.time.LocalDate
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter

/**
 * What the Recursos screens do with the uses and resources the core sent. Plain and clock-free: every
 * rule that needs "now" takes it as a moment text (`AAAA-MM-DDTHH:MM`, local wall time, compared as
 * text exactly as the core does), so it is tested without Android and without the real clock.
 */
internal object ResourceRules {
    /** "Próximos dias" reaches this many days after today. */
    const val NEXT_DAYS = 14

    private val MOMENT = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm")

    // The statuses the core sends (src/resources.rs, `UseStatus::as_str`).
    const val RESERVED = "reservado"
    const val IN_USE = "em_uso"
    const val RETURNED = "devolvido"
    const val CANCELLED = "cancelado"

    /** The clock as a moment text. */
    fun nowMoment(now: LocalDateTime = LocalDateTime.now()): String = now.format(MOMENT)

    fun parse(text: String): LocalDateTime? = try { LocalDateTime.parse(text, MOMENT) } catch (_: Throwable) { null }

    /** `AAAA-MM-DD` of a moment text (whatever is there if it is shorter). */
    fun dayOf(moment: String): String = moment.take(10)

    // ------------------------------------------------------------------ one use

    /** In use and past its expected end (the same rule as the core's `overdue_at`). */
    fun isOverdue(u: ResourceUseItem, now: String): Boolean = u.status == IN_USE && u.end.isNotEmpty() && u.end < now

    /**
     * Whether the use holds its resource at [now]: a reservation during `[start, end)`; a loan from its
     * start on, until it is returned (an item not returned is still out, past its end too).
     */
    fun occupiesNow(u: ResourceUseItem, now: String): Boolean = when (u.status) {
        RESERVED -> u.start <= now && now < u.end
        IN_USE -> u.start <= now
        else -> false
    }

    /** Whether the use is reserved or out and its interval touches the day [today] (`AAAA-MM-DD`). */
    fun occupiesDay(u: ResourceUseItem, today: String): Boolean = when (u.status) {
        RESERVED -> dayOf(u.start) <= today && today <= dayOf(u.end)
        IN_USE -> dayOf(u.start) <= today
        else -> false
    }

    enum class Group { LATE, IN_USE, TODAY, NEXT, LATER, CLOSED }

    /** The group of the list a use belongs to. */
    fun groupOf(u: ResourceUseItem, now: String): Group {
        val today = dayOf(now)
        return when {
            u.status == RETURNED || u.status == CANCELLED -> Group.CLOSED
            isOverdue(u, now) -> Group.LATE
            u.status == IN_USE -> Group.IN_USE
            // A reservation whose time went by without anyone taking the item is over.
            u.end <= now -> Group.CLOSED
            dayOf(u.start) <= today -> Group.TODAY
            dayOf(u.start) <= nextLimit(today) -> Group.NEXT
            else -> Group.LATER
        }
    }

    private fun nextLimit(today: String): String =
        try { LocalDate.parse(today).plusDays(NEXT_DAYS.toLong()).toString() } catch (_: Throwable) { today }

    // ------------------------------------------------------------------ the list of uses

    /** The uses of each group, already ordered for the screen. */
    data class Grouped(
        val late: List<ResourceUseItem>,
        val inUse: List<ResourceUseItem>,
        val today: List<ResourceUseItem>,
        val next: List<ResourceUseItem>,
        val later: List<ResourceUseItem>,
        val closed: List<ResourceUseItem>
    ) {
        val total: Int get() = late.size + inUse.size + today.size + next.size + later.size + closed.size

        /** The groups that have something, in the order of the screen. */
        val shown: List<Pair<Group, List<ResourceUseItem>>>
            get() = listOf(
                Group.LATE to late, Group.IN_USE to inUse, Group.TODAY to today,
                Group.NEXT to next, Group.LATER to later, Group.CLOSED to closed
            ).filter { it.second.isNotEmpty() }
    }

    /**
     * The uses that match [query], in groups: overdue (oldest end first), out now (earliest end first),
     * today and the next days (by start), later ones, and the closed ones (latest first).
     */
    fun group(uses: List<ResourceUseItem>, query: String, now: String): Grouped {
        val by = uses.filter { matches(it, query) }.groupBy { groupOf(it, now) }
        fun of(g: Group, order: Comparator<ResourceUseItem>) = (by[g] ?: emptyList()).sortedWith(order)
        val byEnd = compareBy<ResourceUseItem> { it.end }.thenBy { it.start }
        val byStart = compareBy<ResourceUseItem> { it.start }.thenBy { it.end }
        val latestFirst = compareByDescending<ResourceUseItem> { it.returnedAt.ifEmpty { it.end } }.thenByDescending { it.start }
        return Grouped(
            late = of(Group.LATE, byEnd), inUse = of(Group.IN_USE, byEnd), today = of(Group.TODAY, byStart),
            next = of(Group.NEXT, byStart), later = of(Group.LATER, byStart), closed = of(Group.CLOSED, latestFirst)
        )
    }

    fun groupTitle(g: Group): String = when (g) {
        Group.LATE -> PhoneWords.RES_GROUP_LATE
        Group.IN_USE -> PhoneWords.RES_GROUP_IN_USE
        Group.TODAY -> PhoneWords.RES_GROUP_TODAY
        Group.NEXT -> PhoneWords.RES_GROUP_NEXT
        Group.LATER -> PhoneWords.RES_GROUP_LATER
        Group.CLOSED -> PhoneWords.RES_GROUP_CLOSED
    }

    // ------------------------------------------------------------------ search

    /** Every word of [query] appears somewhere in [fields] (capitals and accents do not matter). */
    private fun hasAllWords(query: String, fields: List<String>): Boolean {
        val words = foldText(query).split(' ').filter { it.isNotEmpty() }
        if (words.isEmpty()) return true
        val haystack = foldText(fields.joinToString("\n"))
        return words.all { haystack.contains(it) }
    }

    fun matches(u: ResourceUseItem, query: String): Boolean = hasAllWords(
        query,
        listOf(
            u.resourceName, u.who, u.sector, u.purpose, u.documentNumber, u.documentTitle,
            u.attendanceReference, u.taskTitle
        )
    )

    fun matches(r: ResourceItem, query: String): Boolean =
        hasAllWords(query, listOf(r.name, r.location, r.identification))

    // ------------------------------------------------------------------ times

    private fun dayMonth(m: LocalDateTime) = "%02d/%02d".format(m.dayOfMonth, m.monthValue)
    private fun clock(m: LocalDateTime) = "%02d:%02d".format(m.hour, m.minute)

    /**
     * "10/11 14:00 – 16:00" for a period inside one day, "10/11 14:00 – 12/11 09:00" over several days
     * (the year joins the dates only when the two ends are in different years). What cannot be read
     * goes out as it is.
     */
    fun periodText(start: String, end: String): String {
        val s = parse(start)
        val e = parse(end)
        if (s == null || e == null) return listOf(start, end).filter { it.isNotEmpty() }.joinToString(" – ")
        return when {
            s.toLocalDate() == e.toLocalDate() -> "${dayMonth(s)} ${clock(s)} – ${clock(e)}"
            s.year != e.year -> "${dayMonth(s)}/${s.year} ${clock(s)} – ${dayMonth(e)}/${e.year} ${clock(e)}"
            else -> "${dayMonth(s)} ${clock(s)} – ${dayMonth(e)} ${clock(e)}"
        }
    }

    /** "10/11/2026 14:00", for the detail; what cannot be read goes out as it is. */
    fun momentText(moment: String): String {
        val m = parse(moment) ?: return moment
        return "${dayMonth(m)}/${m.year} ${clock(m)}"
    }

    // ------------------------------------------------------------------ one resource

    /** How many of [r] are out at [now]. */
    fun inUseNow(r: ResourceItem, uses: List<ResourceUseItem>, now: String): Int =
        uses.filter { it.resourceUid == r.uid && occupiesNow(it, now) }.sumOf { it.quantity }

    /** How many of [r] are free at [now] (never negative). */
    fun freeNow(r: ResourceItem, uses: List<ResourceUseItem>, now: String): Int =
        (r.quantity - inUseNow(r, uses, now)).coerceAtLeast(0)

    /** "1 de 3 em uso agora". */
    fun usageLine(r: ResourceItem, uses: List<ResourceUseItem>, now: String): String =
        "${inUseNow(r, uses, now)} de ${r.quantity} em uso agora"

    /** The register: the ones that match, those out of use last (the order of the rest is kept). */
    fun resourceRows(resources: List<ResourceItem>, query: String): List<ResourceItem> {
        val found = resources.filter { matches(it, query) }
        return found.filter { it.active } + found.filter { !it.active }
    }

    /** What is still ahead for [r] or out now (by start): not returned, not cancelled, not passed unused. */
    fun nextUses(r: ResourceItem, uses: List<ResourceUseItem>, now: String): List<ResourceUseItem> =
        uses.filter { it.resourceUid == r.uid && groupOf(it, now) != Group.CLOSED }
            .sortedWith(compareBy<ResourceUseItem> { it.start }.thenBy { it.end })

    /** Who holds [r] on the day of [now] (reserved or out, touching the day), by start. */
    fun occupationToday(r: ResourceItem, uses: List<ResourceUseItem>, now: String): List<ResourceUseItem> {
        val today = dayOf(now)
        return uses.filter { it.resourceUid == r.uid && occupiesDay(it, today) }
            .sortedWith(compareBy<ResourceUseItem> { it.start }.thenBy { it.end })
    }

    /** The line of a use in a list: "Servidor B · Setor Jurídico" (whichever exist). */
    fun whoLine(u: ResourceUseItem): String = listOf(u.who, u.sector).filter { it.isNotBlank() }.joinToString(" · ")

    /** What the Hoje line says: "2 reservas de hoje". */
    fun reservationsLine(n: Int): String = PhoneWords.plural(n, "reserva de hoje", "reservas de hoje")

    /** What the Hoje line says: "1 devolução atrasada". */
    fun overdueLine(n: Int): String = PhoneWords.plural(n, "devolução atrasada", "devoluções atrasadas")
}
