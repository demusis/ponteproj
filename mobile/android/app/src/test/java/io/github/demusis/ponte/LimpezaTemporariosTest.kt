package io.github.demusis.ponte

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

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
}
