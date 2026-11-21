package io.github.demusis.ponte

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class LimpezaTemporariosTest {
    private val now = 10_000_000_000L
    private val hour = TempRules.MAX_AGE_MS

    @Test fun soApagaOQueTemMaisDeUmaHora() {
        assertTrue(TempRules.mayDelete("/c/fotos/a.jpg", now - hour - 1, now, emptySet(), false))
        assertFalse(TempRules.mayDelete("/c/fotos/a.jpg", now - hour, now, emptySet(), false))
        assertFalse(TempRules.mayDelete("/c/fotos/a.jpg", now - 1000, now, emptySet(), false))
    }

    @Test fun oQueEstaEmUsoNaoSaiMesmoSendoAntigo() {
        val old = now - 5 * hour
        assertFalse(TempRules.mayDelete("/c/fotos/a.jpg", old, now, setOf("/c/fotos/a.jpg"), false))
        assertTrue(TempRules.mayDelete("/c/fotos/b.jpg", old, now, setOf("/c/fotos/a.jpg"), false))
    }

    @Test fun nadaSaiEnquantoUmRegistroEGravado() {
        assertFalse(TempRules.mayDelete("/c/fotos/a.jpg", now - 5 * hour, now, emptySet(), true))
    }

    @Test fun soExportacoesDeSincronizacaoNaRaizDoCache() {
        assertTrue(TempRules.isSyncExport("sync-123.ponte"))
        assertFalse(TempRules.isSyncExport("sync-123.tmp"))
        assertFalse(TempRules.isSyncExport("outro.ponte"))
    }

    @Test fun apagarReconfereArquivoEIdadeSobOMesmoTrancoDaExportacao() {
        val dir = java.nio.file.Files.createTempDirectory("limpeza").toFile()
        try {
            val f = File(dir, "peticao.pdf").apply { writeText("velho") }
            val decided = now - 5 * hour
            f.setLastModified(decided)
            // Same file, still old: goes.
            assertTrue(TempRules.deleteIfStale(f, decided, now, emptySet(), false))
            assertFalse(f.exists())

            // The decision was taken for an old file, but an export recreated it before the delete.
            val again = File(dir, "peticao.pdf").apply { writeText("novo") }
            again.setLastModified(now + 1000)
            assertFalse(TempRules.deleteIfStale(again, decided, now, emptySet(), false))
            assertTrue(again.exists())

            // Not the same file as the one decided on (last-modified changed), even if it is old too.
            again.setLastModified(now - 2 * hour)
            assertFalse(TempRules.deleteIfStale(again, decided, now, emptySet(), false))
            assertTrue(again.exists())

            // Never while a record is saving or the file is in use.
            assertFalse(TempRules.deleteIfStale(again, now - 2 * hour, now, emptySet(), true))
            assertFalse(TempRules.deleteIfStale(again, now - 2 * hour, now, setOf(again.absolutePath), false))
            assertTrue(again.exists())
        } finally {
            dir.deleteRecursively()
        }
    }
}
