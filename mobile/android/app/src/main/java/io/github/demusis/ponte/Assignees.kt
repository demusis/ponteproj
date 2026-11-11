package io.github.demusis.ponte

import java.text.Normalizer

/** Lowercase, without accents and with single spaces: how the app compares names and search text. */
internal fun foldText(s: String): String =
    Normalizer.normalize(s, Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "")
        .lowercase().trim().replace(Regex("\\s+"), " ")

/**
 * The suggestions under the Responsável field. Plain values and functions, so the rules are tested
 * without Android. They only help to fill the typed form: they never touch the task.
 */
internal object Assignees {
    /** At most this many names are shown. */
    const val SHOWN = 8

    /** The most that "Seu nome" may hold. */
    const val NAME_MAX = 80

    /** What is stored of the person's name: no blanks around it, at most [NAME_MAX] characters. */
    fun cleanName(typed: String): String = typed.trim().take(NAME_MAX).trim()

    /**
     * The names of [all] (already most used first, as the library sent them) that fit what is typed:
     * every name when nothing is typed, else the ones that contain it, without caring for capitals or
     * accents. The person's own name [me] is left out (it has its own "Eu" button) and so is the very
     * text already typed. At most [limit].
     */
    fun suggest(all: List<String>, typed: String, me: String = "", limit: Int = SHOWN): List<String> {
        val q = foldText(typed)
        val mine = foldText(me)
        return all.filter { name ->
            val f = foldText(name)
            f.isNotEmpty() && f != mine && name.trim() != typed.trim() && (q.isEmpty() || f.contains(q))
        }.take(limit)
    }

    /** The "Eu" button is offered when the name is set and the field does not already say it. */
    fun offerMe(me: String, typed: String): Boolean = me.isNotBlank() && typed.trim() != me.trim()
}
