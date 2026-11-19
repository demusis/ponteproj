package io.github.demusis.ponte

import java.time.DayOfWeek
import java.time.LocalDate

/** Brazilian national holidays: days that are not worked in the planning (same list as the desktop). */
internal object Holidays {
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
    fun holiday(date: LocalDate): String? {
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
        return when (java.time.temporal.ChronoUnit.DAYS.between(easter(date.year), date)) {
            -48L, -47L -> "Carnaval"
            -2L -> "Sexta-feira Santa"
            60L -> "Corpus Christi"
            else -> null
        }
    }

    /** Monday to Friday and not a national holiday. */
    fun isWorkingDay(date: LocalDate): Boolean =
        date.dayOfWeek != DayOfWeek.SATURDAY && date.dayOfWeek != DayOfWeek.SUNDAY && holiday(date) == null
}
