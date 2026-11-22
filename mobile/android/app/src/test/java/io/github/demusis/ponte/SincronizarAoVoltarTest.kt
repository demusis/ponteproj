package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Sincronizar ao voltar para o aplicativo: a regra pura que decide se a volta pede uma sincronização. */
class SincronizarAoVoltarTest {
    private val minute = 60_000L
    private val now = 1_800_000_000_000L

    private fun should(
        last: Long, locked: Boolean = false, syncing: Boolean = false, configured: Boolean = true, attempt: Long = 0L
    ) = ForegroundSync.shouldSync(now, last, locked, syncing, configured, attempt)

    @Test fun cincoMinutosEOLimite() {
        assertEquals(5 * minute, ForegroundSync.STALE_MS)
        assertFalse(should(last = now - 4 * minute - 59_000))
        assertTrue(should(last = now - 5 * minute))
        assertTrue(should(last = now - 2 * 60 * minute))
    }

    @Test fun umaSincronizacaoRecenteNaoSeRepete() {
        assertFalse(should(last = now))
        assertFalse(should(last = now - minute))
    }

    @Test fun nuncaSincronizouPedeSincronizacao() = assertTrue(should(last = 0L))

    @Test fun bloqueadoNaoSincronizaAntesDoDesbloqueio() {
        assertFalse(should(last = 0L, locked = true))
        assertFalse(should(last = now - 60 * minute, locked = true))
    }

    @Test fun jaSincronizandoNaoComecaOutra() = assertFalse(should(last = now - 60 * minute, syncing = true))

    @Test fun semConfiguracaoNaoSincroniza() = assertFalse(should(last = now - 60 * minute, configured = false))

    @Test fun umaTentativaQueFalhouNaoSeRepeteNaMesmaJanela() {
        // The last good sync is old, but a return asked for one a minute ago (offline, say): not again yet.
        assertFalse(should(last = now - 60 * minute, attempt = now - minute))
        assertFalse(should(last = 0L, attempt = now - 4 * minute))
        assertTrue(should(last = now - 60 * minute, attempt = now - 5 * minute))
        assertTrue(should(last = now - 60 * minute, attempt = 0L))
    }

    @Test fun orelogioAtrasadoContaComoAntigo() {
        assertTrue(should(last = now + 10 * minute))
        // ...but a "future" attempt does not hold the next return back for ever.
        assertTrue(should(last = now - 60 * minute, attempt = now + 10 * minute))
    }
}
