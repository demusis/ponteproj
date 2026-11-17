package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The first-sync step says why it did not sync, whatever the reason (0.9.1). */
class SetupRulesTest {
    private val semWifi = "Sem Wi-Fi: sincronização adiada. Para usar dados móveis, ative em Ajustes."

    @Test fun semWifiApareceEOfereceDadosMoveis() {
        // Not an error for the sync runner, but the setup screen must say it.
        assertEquals(semWifi, SetupRules.reason(busy = false, text = semWifi))
        assertTrue(SetupRules.noWifi(busy = false, text = semWifi))
    }

    @Test fun digitalNaoLiberadaTambemAparece() {
        val msg = "Senha de sincronização não liberada."
        assertEquals(msg, SetupRules.reason(busy = false, text = msg))
        assertFalse(SetupRules.noWifi(busy = false, text = msg))
    }

    @Test fun nadaADizerEnquantoSincronizaQuandoDeuCertoOuAntesDeTentar() {
        assertEquals("", SetupRules.reason(busy = true, text = semWifi))
        assertFalse(SetupRules.noWifi(busy = true, text = semWifi))
        assertEquals("", SetupRules.reason(busy = false, text = "Sincronizado: 3 alterações."))
        assertEquals("", SetupRules.reason(busy = false, text = "Pronto."))
        assertEquals("", SetupRules.reason(busy = false, text = ""))
    }

    @Test fun erroDeVerdadeContinuaAparecendo() {
        val erro = "A senha de sincronização não confere."
        assertEquals(erro, SetupRules.reason(busy = false, text = erro))
    }
}
