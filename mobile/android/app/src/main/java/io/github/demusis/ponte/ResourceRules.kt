package io.github.demusis.ponte

import java.time.LocalDateTime
import java.time.format.DateTimeFormatter

/**
 * What the Recursos screens still say themselves. The rules (groups, who holds what now, the search, the
 * period wording) are the core's: the screens read what it decided (see `resource_view` in the bridge).
 */
internal object ResourceRules {
    private val MOMENT = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm")

    // The statuses the core sends (src/resources.rs, `UseStatus::as_str`).
    const val RESERVED = "reservado"
    const val IN_USE = "em_uso"
    const val RETURNED = "devolvido"
    const val CANCELLED = "cancelado"

    private fun parse(text: String): LocalDateTime? = try { LocalDateTime.parse(text, MOMENT) } catch (_: Throwable) { null }

    /** "10/11/2026 14:00", for the detail; what cannot be read goes out as it is. */
    fun momentText(moment: String): String {
        val m = parse(moment) ?: return moment
        return "%02d/%02d/%d %02d:%02d".format(m.dayOfMonth, m.monthValue, m.year, m.hour, m.minute)
    }

    /** The line of a use in a list: "Servidor B · Setor Jurídico" (whichever exist). */
    fun whoLine(u: ResourceUseItem): String = listOf(u.who, u.sector).filter { it.isNotBlank() }.joinToString(" · ")
}
