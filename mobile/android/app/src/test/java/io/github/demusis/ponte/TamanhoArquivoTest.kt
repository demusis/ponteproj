package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/** The size of the Drive file in Ajustes: the room left, the warning from 70%, and what does not sync. */
class TamanhoArquivoTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val mb = 1024L * 1024

    @Test fun tamanhosEmPortugues() {
        assertEquals("0 bytes", FolderRoom.size(0))
        assertEquals("340 KB", FolderRoom.size(340 * 1024))
        assertEquals("12,3 MB", FolderRoom.size(12 * mb + 300_000))
        assertEquals("512 MB", FolderRoom.size(FolderRoom.LIMIT))
        assertEquals("100 MB", FolderRoom.size(100 * mb + 400_000))
        assertEquals("3 MB", FolderRoom.size(3 * mb))
    }

    @Test fun aLinhaDizQuantoFalta() {
        assertEquals("Tamanho do arquivo: 120 MB de 512 MB (23%). Cabem mais 392 MB.", FolderRoom.line(120 * mb))
        assertTrue(FolderRoom.line(FolderRoom.LIMIT).contains("Sem espaço restante"))
        assertEquals(100, FolderRoom.percent(FolderRoom.LIMIT * 2))
        assertEquals(0L, FolderRoom.remaining(FolderRoom.LIMIT + 1))
    }

    @Test fun oAvisoComecaEmSetentaPorCento() {
        // 70% of 512 MB is 358,4 MB.
        assertNull(FolderRoom.warning(358 * mb))
        assertNull(FolderRoom.warning(-1))
        val w = FolderRoom.warning(359 * mb)!!
        assertTrue(w, w.startsWith("O arquivo do Drive está com 359 MB de 512 MB (70%)."))
        assertTrue(w.contains("esvazie a Lixeira") && w.contains("anexos grandes") && w.contains("Ponte do computador"))
        val full = FolderRoom.warning(FolderRoom.LIMIT)!!
        assertTrue(full.contains("chegou ao limite") && full.contains("talvez não caiba"))
        assertFalse(full.contains("não consegue"))
    }

    @Test fun oLimiteEoDoNucleo() = assertEquals(512L * 1024 * 1024, FolderRoom.LIMIT)

    @Test fun oQueNaoSincroniza() {
        for (word in listOf("e-mail", "avisos de prazos", "lembretes", "agenda do celular", "bloqueio", "filtros guardados")) {
            assertTrue(word, FolderRoom.NOT_SYNCED.contains(word))
        }
    }

    @Test fun aFalhaDoNucleoGanhaOsMesmosConselhosNoCelular() {
        val core = "Nenhuma pasta pôde ser gravada: O acervo passa de 512 MB e não cabe no arquivo de sincronização " +
            "(daria 600 MB). Nada foi gravado. Para liberar espaço, esvazie a Lixeira (Ajustes ▸ Lixeira) e remova os " +
            "anexos grandes que não precisam ir para os outros aparelhos. " +
            "As alterações continuam guardadas neste computador."
        assertEquals(
            "O acervo passa de 512 MB e não cabe no arquivo de sincronização (daria 600 MB). Nada foi gravado. " +
                "Para liberar espaço, esvazie a Lixeira e remova os anexos grandes que não precisam ir para os outros " +
                "aparelhos, no Ponte do computador. As alterações continuam guardadas neste aparelho.",
            PhoneWords.adapt(core)
        )
        // Without the size the sentence is the same, with no stray text.
        val bare = core.replace(" (daria 600 MB)", "")
        assertEquals(
            "O acervo passa de 512 MB e não cabe no arquivo de sincronização. Nada foi gravado. " +
                "Para liberar espaço, esvazie a Lixeira e remova os anexos grandes que não precisam ir para os outros " +
                "aparelhos, no Ponte do computador. As alterações continuam guardadas neste aparelho.",
            PhoneWords.adapt(bare)
        )
        // Any other failure of writing keeps its sentence.
        assertEquals(
            "Não foi possível gravar o arquivo de sincronização. As alterações continuam guardadas neste aparelho.",
            PhoneWords.adapt("Nenhuma pasta pôde ser gravada: disco cheio. As alterações continuam guardadas neste computador.")
        )
    }

    /**
     * Revisão do pacote, achado 7: the same refusal, when it comes while the core takes its first file of the
     * new format ("A atualização do arquivo da pasta parou: …"), reads as in 0.51 and never sends the person to
     * a screen the phone does not have.
     */
    @Test fun aRecusaAoAtualizarOArquivoDaPastaFalaComoNa051() {
        val core = "A atualização do arquivo da pasta parou: O acervo passa de 512 MB e não cabe no arquivo de " +
            "sincronização (daria 600 MB). Nada foi gravado. Para liberar espaço, esvazie a Lixeira (Ajustes ▸ " +
            "Lixeira) e remova os anexos grandes que não precisam ir para os outros aparelhos. Os arquivos antigos " +
            "ficaram como estavam e as alterações continuam guardadas neste computador; a próxima sincronização " +
            "continua de onde parou."
        val phone = PhoneWords.adapt(core)
        assertEquals(
            "O acervo passa de 512 MB e não cabe no arquivo de sincronização (daria 600 MB). Nada foi gravado. " +
                "Para liberar espaço, esvazie a Lixeira e remova os anexos grandes que não precisam ir para os outros " +
                "aparelhos, no Ponte do computador. As alterações continuam guardadas neste aparelho.",
            phone
        )
        assertFalse(phone, phone.contains("▸"))
        assertFalse(phone, phone.contains(".."))
        assertEquals(
            "O acervo passa de 512 MB e não cabe no arquivo de sincronização. Nada foi gravado. " +
                "Para liberar espaço, esvazie a Lixeira e remova os anexos grandes que não precisam ir para os outros " +
                "aparelhos, no Ponte do computador. As alterações continuam guardadas neste aparelho.",
            PhoneWords.adapt(core.replace(" (daria 600 MB)", ""))
        )
    }

    private fun settings(size: Long, name: String, dark: Boolean = false) {
        paparazzi.tall(2600)
        paparazzi.shot(name, dark) {
            SettingsScreen(Samples.settings.copy(fileSize = size), {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})
        }
    }

    @Test fun ajustesComEspaco() = settings(48_300_000, "com_espaco")

    @Test fun ajustesPertoDoLimite() = settings(400 * mb, "perto_do_limite")

    @Test fun ajustesNoLimiteEscuro() = settings(FolderRoom.LIMIT, "no_limite_escuro", dark = true)
}
