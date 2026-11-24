package io.github.demusis.ponte

import java.util.Locale

/**
 * How full the Drive file is, and what never travels between the devices of the library. The same
 * numbers and sentences as the computer (src/folder_health.rs); plain values, no Android, so plain
 * JUnit tests them. The phone has no Lixeira or attachment screens: what frees room is done in
 * the Ponte of the computer, and the text says so.
 */
internal object FolderRoom {
    /** The limit of the file (the core's, 512 MB). */
    const val LIMIT: Long = SyncIo.MAX_BYTES

    /** From this share of the limit the person is warned. */
    const val WARN_PERCENT = 70

    /** What to do when the file nears or reaches the limit. */
    const val ADVICE =
        "Para liberar espaço, esvazie a Lixeira e remova os anexos grandes que não precisam ir para os outros " +
            "aparelhos, no Ponte do computador."

    /**
     * What does not travel to the other devices (checked against what the file carries): each of
     * these stays on the phone.
     */
    const val NOT_SYNCED =
        "Não vão para os outros aparelhos: as contas de e-mail, os avisos de prazos e os lembretes deste aparelho, " +
            "a ligação com a agenda do celular, o bloqueio do aplicativo, o seu nome nas tarefas e os filtros guardados."

    private val ptBr = Locale.forLanguageTag("pt-BR")

    /** `512 MB`, `12,3 MB`, `340 KB`, `20 bytes`. */
    fun size(n: Long): String = when {
        n >= 1024 * 1024 -> {
            val mb = n / 1048576.0
            if (mb >= 100 || Math.abs(mb - Math.round(mb)) < 0.05) String.format(ptBr, "%d MB", Math.round(mb))
            else String.format(ptBr, "%.1f MB", mb)
        }
        n >= 1024 -> String.format(ptBr, "%d KB", Math.round(n / 1024.0))
        else -> "$n bytes"
    }

    /** Whole percent of the limit used, never above 100. */
    fun percent(n: Long): Int = minOf(100L, if (n <= 0) 0L else n * 100 / LIMIT).toInt()

    fun remaining(n: Long): Long = maxOf(0L, LIMIT - n)

    /** The line under the file: how big it is and how much room is left. */
    fun line(n: Long): String =
        if (remaining(n) == 0L) "Tamanho do arquivo: ${size(n)} de ${size(LIMIT)} (100%). Sem espaço restante."
        else "Tamanho do arquivo: ${size(n)} de ${size(LIMIT)} (${percent(n)}%). Cabem mais ${size(remaining(n))}."

    /** The warning from 70% of the limit, with what to do; null below it. */
    fun warning(n: Long): String? {
        if (n < 0 || n * 100 < LIMIT * WARN_PERCENT) return null
        val state =
            if (n >= LIMIT) "Ele chegou ao limite: o que for acrescentado talvez não caiba."
            else "Perto do limite, o que for acrescentado pode não caber."
        return "O arquivo do Drive está com ${size(n)} de ${size(LIMIT)} (${percent(n)}%). $state $ADVICE"
    }
}
