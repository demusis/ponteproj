package io.github.demusis.ponte

import java.time.DayOfWeek
import java.time.LocalDate
import java.time.temporal.ChronoUnit
import java.util.Locale

/** A non-working day the person registered ([year] null: every year). */
internal data class CustomDay(val year: Int?, val month: Int, val day: Int, val name: String) {
    fun matches(date: LocalDate) =
        date.monthValue == month && date.dayOfMonth == day && (year == null || year == date.year)

    /** The name shown; an empty name becomes a generic one. */
    val label: String get() = name.ifEmpty { "Dia sem expediente" }
}

/** The phone's own choices: a state and extra days off (same text format as the desktop's feriados.txt). */
internal data class HolidayConfig(val uf: String? = null, val custom: List<CustomDay> = emptyList()) {
    /** Lines `uf SP`, `anual 09-07 Nome`, `dia 2026-03-19 Nome`. */
    fun toText(): String {
        val sb = StringBuilder()
        if (uf != null) sb.append("uf ").append(uf).append('\n')
        for (d in custom) {
            val name = d.name.replace('\n', ' ').replace('\r', ' ').trim()
            val line = if (d.year == null) String.format(Locale.ROOT, "anual %02d-%02d %s", d.month, d.day, name)
            else String.format(Locale.ROOT, "dia %04d-%02d-%02d %s", d.year, d.month, d.day, name)
            sb.append(line.trimEnd()).append('\n')
        }
        return sb.toString()
    }

    companion object {
        private val ISO = Regex("""(\d{1,4})-(\d{1,2})-(\d{1,2})""")

        /** Anything unreadable is skipped; unknown lines are ignored. */
        fun parse(text: String): HolidayConfig {
            var uf: String? = null
            val custom = ArrayList<CustomDay>()
            for (raw in text.lines()) {
                val line = raw.trim()
                val (key, rest) = splitWord(line)
                val (value, name) = splitWord(rest.trimStart())
                val label = name.trim()
                when (key) {
                    "uf" -> uf = Holidays.STATES.firstOrNull { it.first == value }?.first
                    "anual" -> {
                        val parts = value.split('-')
                        val m = if (parts.size == 2) parts[0].toIntOrNull() else null
                        val d = if (parts.size == 2) parts[1].toIntOrNull() else null
                        // 2024 is a leap year: 02-29 is accepted.
                        if (m != null && d != null && validDate(2024, m, d)) custom.add(CustomDay(null, m, d, label))
                    }
                    "dia" -> {
                        val g = ISO.matchEntire(value)?.groupValues
                        if (g != null) {
                            val y = g[1].toInt()
                            val m = g[2].toInt()
                            val d = g[3].toInt()
                            if (validDate(y, m, d)) custom.add(CustomDay(y, m, d, label))
                        }
                    }
                }
            }
            return HolidayConfig(uf, custom)
        }

        /** Splits at the first whitespace: the word and the rest (empty when there is none). */
        private fun splitWord(s: String): Pair<String, String> {
            val i = s.indexOfFirst { it.isWhitespace() }
            return if (i < 0) s to "" else s.substring(0, i) to s.substring(i + 1)
        }

        private fun validDate(y: Int, m: Int, d: Int) =
            try { LocalDate.of(y, m, d); true } catch (_: Throwable) { false }
    }
}

/**
 * Days that are not worked in the planning: the national holidays (same list as the desktop), those
 * of the state the person chose and the days they registered. The last two live on this phone only.
 */
internal object Holidays {
    /** The 27 states: code and full name, in the order of the picker. */
    val STATES: List<Pair<String, String>> = listOf(
        "AC" to "Acre", "AL" to "Alagoas", "AP" to "Amapá", "AM" to "Amazonas", "BA" to "Bahia",
        "CE" to "Ceará", "DF" to "Distrito Federal", "ES" to "Espírito Santo", "GO" to "Goiás",
        "MA" to "Maranhão", "MT" to "Mato Grosso", "MS" to "Mato Grosso do Sul", "MG" to "Minas Gerais",
        "PA" to "Pará", "PB" to "Paraíba", "PR" to "Paraná", "PE" to "Pernambuco", "PI" to "Piauí",
        "RJ" to "Rio de Janeiro", "RN" to "Rio Grande do Norte", "RS" to "Rio Grande do Sul",
        "RO" to "Rondônia", "RR" to "Roraima", "SC" to "Santa Catarina", "SP" to "São Paulo",
        "SE" to "Sergipe", "TO" to "Tocantins"
    )

    fun stateName(uf: String): String? = STATES.firstOrNull { it.first == uf }?.second

    @Volatile private var config = HolidayConfig()

    /** The configuration in use. */
    fun current(): HolidayConfig = config

    /** Makes the saved text the configuration the planning uses from now on. */
    fun configure(text: String) {
        config = HolidayConfig.parse(text)
    }

    /** The text as it is kept: what is unreadable dropped, the lines in the order of [HolidayConfig.toText]. */
    fun canonical(text: String): String = HolidayConfig.parse(text).toText()

    /** Easter Sunday of [year] (anonymous Gregorian algorithm, Meeus/Jones/Butcher). */
    fun easter(year: Int): LocalDate {
        val a = year % 19
        val b = year / 100
        val c = year % 100
        val d = b / 4
        val e = b % 4
        val f = (b + 8) / 25
        val g = (b - f + 1) / 3
        val h = (19 * a + b - d - g + 15) % 30
        val i = c / 4
        val k = c % 4
        val l = (32 + 2 * e + 2 * i - h - k) % 7
        val m = (a + 11 * h + 22 * l) / 451
        val month = (h + l - 7 * m + 114) / 31
        val day = (h + l - 7 * m + 114) % 31 + 1
        return LocalDate.of(year, month, day)
    }

    /** The name of the national holiday on [date], or null. */
    fun national(date: LocalDate): String? {
        val fixed = when (date.monthValue to date.dayOfMonth) {
            1 to 1 -> "Confraternização Universal"
            4 to 21 -> "Tiradentes"
            5 to 1 -> "Dia do Trabalho"
            9 to 7 -> "Independência"
            10 to 12 -> "Nossa Senhora Aparecida"
            11 to 2 -> "Finados"
            11 to 15 -> "Proclamação da República"
            11 to 20 -> if (date.year >= 2024) "Dia da Consciência Negra" else null
            12 to 25 -> "Natal"
            else -> null
        }
        if (fixed != null) return fixed
        return when (ChronoUnit.DAYS.between(easter(date.year), date)) {
            -48L, -47L -> "Carnaval"
            -2L -> "Sexta-feira Santa"
            60L -> "Corpus Christi"
            else -> null
        }
    }

    /** When a state holiday falls: every year on [month]/[day] from [from] on, or [afterEaster] days after Easter. */
    private class When(val month: Int, val day: Int, val from: Int, val afterEaster: Long?, val name: String)

    private fun fixed(month: Int, day: Int, name: String, from: Int = 0) = When(month, day, from, null, name)

    /** State holidays that are not national (the same table is in `src/holidays.rs`). */
    private val STATE_TABLE: Map<String, List<When>> = mapOf(
        "AC" to listOf(
            fixed(1, 23, "Dia do Evangélico"), fixed(6, 15, "Aniversário do Acre"),
            fixed(9, 5, "Dia da Amazônia"), fixed(11, 17, "Tratado de Petrópolis")
        ),
        "AL" to listOf(fixed(6, 24, "São João"), fixed(6, 29, "São Pedro"), fixed(9, 16, "Emancipação de Alagoas")),
        "AP" to listOf(fixed(3, 19, "São José"), fixed(9, 13, "Criação do Amapá")),
        "AM" to listOf(fixed(9, 5, "Elevação do Amazonas a Província")),
        "BA" to listOf(fixed(7, 2, "Independência da Bahia")),
        "CE" to listOf(fixed(3, 25, "Data Magna do Ceará")),
        "DF" to listOf(fixed(11, 30, "Dia do Evangélico")),
        "ES" to listOf(When(0, 0, 0, 8L, "Nossa Senhora da Penha")),
        "MA" to listOf(fixed(7, 28, "Adesão do Maranhão")),
        "MS" to listOf(fixed(10, 11, "Criação de Mato Grosso do Sul")),
        "PA" to listOf(fixed(8, 15, "Adesão do Pará")),
        "PB" to listOf(fixed(8, 5, "Fundação da Paraíba")),
        "PE" to listOf(fixed(3, 6, "Data Magna de Pernambuco", from = 2017), fixed(6, 24, "São João")),
        "PI" to listOf(fixed(10, 19, "Dia do Piauí")),
        "RJ" to listOf(fixed(4, 23, "São Jorge")),
        "RN" to listOf(fixed(10, 3, "Mártires de Cunhaú e Uruaçu")),
        "RO" to listOf(fixed(1, 4, "Criação de Rondônia"), fixed(6, 18, "Dia do Evangélico")),
        "RR" to listOf(fixed(10, 5, "Criação de Roraima")),
        "RS" to listOf(fixed(9, 20, "Revolução Farroupilha")),
        "SE" to listOf(fixed(7, 8, "Emancipação de Sergipe")),
        "SP" to listOf(fixed(7, 9, "Revolução Constitucionalista")),
        "TO" to listOf(fixed(10, 5, "Criação do Tocantins"), fixed(9, 8, "Nossa Senhora da Natividade"))
    )

    /** The state's holidays as lines for Ajustes: when ("09/07" or "8 dias depois da Páscoa"), and the name. */
    fun stateHolidayLines(uf: String): List<Pair<String, String>> =
        (STATE_TABLE[uf] ?: emptyList())
            .sortedWith(compareBy({ if (it.afterEaster != null) 13 else it.month }, { it.day }))
            .map { w ->
                val whenText = if (w.afterEaster != null) "${w.afterEaster} dias depois da Páscoa"
                else "%02d/%02d".format(w.day, w.month)
                whenText to w.name
            }

    private fun stateHoliday(uf: String, date: LocalDate): String? =
        STATE_TABLE[uf]?.firstOrNull { w ->
            if (w.afterEaster != null) ChronoUnit.DAYS.between(easter(date.year), date) == w.afterEaster
            else date.monthValue == w.month && date.dayOfMonth == w.day && date.year >= w.from
        }?.name

    /** The holiday on [date] under [config]: national, then the state's, then the person's own days. */
    fun holiday(date: LocalDate, config: HolidayConfig): String? =
        national(date)
            ?: config.uf?.let { stateHoliday(it, date) }
            ?: config.custom.firstOrNull { it.matches(date) }?.label

    /** The name of the holiday (or day off) on [date] with the configuration in use, or null. */
    fun holiday(date: LocalDate): String? = holiday(date, config)

    /** Monday to Friday and not a holiday (national, state or the person's own). */
    fun isWorkingDay(date: LocalDate): Boolean =
        date.dayOfWeek != DayOfWeek.SATURDAY && date.dayOfWeek != DayOfWeek.SUNDAY && holiday(date) == null
}
