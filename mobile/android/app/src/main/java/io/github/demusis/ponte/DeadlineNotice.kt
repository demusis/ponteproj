package io.github.demusis.ponte

import java.time.LocalTime
import java.time.ZonedDateTime

/**
 * The rules of the daily deadline notice that need no Android: when the next one is due and what
 * it says. Free of Android types so plain JUnit tests them.
 */
internal object DeadlineNotice {
    /** A time of day, to the minute. */
    data class Time(val hour: Int, val minute: Int) {
        /** "08:00". */
        val key: String get() = "%02d:%02d".format(hour, minute)
        val minutes: Int get() = hour * 60 + minute

        companion object {
            /** The time [m] minutes after midnight; anything out of the day becomes the default. */
            fun fromMinutes(m: Int): Time = if (m in 0 until 24 * 60) Time(m / 60, m % 60) else DEFAULT

            fun fromKey(key: String?): Time {
                val parts = key?.split(':') ?: return DEFAULT
                val h = parts.getOrNull(0)?.toIntOrNull() ?: return DEFAULT
                val m = parts.getOrNull(1)?.toIntOrNull() ?: return DEFAULT
                return if (parts.size == 2 && h in 0..23 && m in 0..59) Time(h, m) else DEFAULT
            }
        }
    }

    val DEFAULT = Time(8, 0)

    /** The times Ajustes offers: from 05:00 to 22:00, every half hour. */
    val CHOICES: List<Time> = (5 * 2..22 * 2).map { Time(it / 2, (it % 2) * 30) }

    /** The title of the notice on the screen of an unlocked phone. */
    const val TITLE = "Prazos de hoje"

    /** What the notice says on a locked screen: no counts, no names. */
    const val PUBLIC_TEXT = "Ponte de bolso: há prazos para hoje."

    /**
     * The first moment after [after] that is [time] on the clock of [after]'s zone. Calendar
     * arithmetic (the day, then the time of day), never "24 hours later", so a day with 23 or 25
     * hours still lands on the chosen time; a time that does not exist that day (the clock jumped
     * over it) moves forward to the first one that does.
     */
    fun nextTrigger(after: ZonedDateTime, time: Time): ZonedDateTime {
        val at = LocalTime.of(time.hour, time.minute)
        val today = ZonedDateTime.of(after.toLocalDate(), at, after.zone)
        return if (today.isAfter(after)) today else ZonedDateTime.of(after.toLocalDate().plusDays(1), at, after.zone)
    }

    /**
     * The notice text from the same counts Hoje shows (tasks late, tasks due today, atendimentos
     * with a deadline), only counts and never a title: "3 tarefas atrasadas, 2 para hoje e 1
     * atendimento com prazo.". Null when nothing is due: then there is no notice.
     */
    fun body(late: Int, today: Int, attendances: Int): String? {
        val l = late.coerceAtLeast(0)
        val t = today.coerceAtLeast(0)
        val a = attendances.coerceAtLeast(0)
        val parts = ArrayList<String>()
        if (l > 0) parts.add(PhoneWords.plural(l, "tarefa atrasada", "tarefas atrasadas"))
        if (t > 0) parts.add(if (l > 0) "$t para hoje" else PhoneWords.plural(t, "tarefa para hoje", "tarefas para hoje"))
        if (a > 0) parts.add(PhoneWords.plural(a, "atendimento com prazo", "atendimentos com prazo"))
        if (parts.isEmpty()) return null
        val joined = if (parts.size == 1) parts[0] else parts.dropLast(1).joinToString(", ") + " e " + parts.last()
        return "$joined."
    }
}
