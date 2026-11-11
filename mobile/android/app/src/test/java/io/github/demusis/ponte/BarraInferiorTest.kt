package io.github.demusis.ponte

import app.cash.paparazzi.DeviceConfig
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/** The bottom bar on small screens and with a large font setting: the names never break. */
class BarraInferiorTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val names = listOf("Hoje", "Tarefas", "Documentos", "E-mail")

    private fun small(fontScale: Float) = DeviceConfig.PIXEL_5.copy(
        screenWidth = 720, screenHeight = 1440, xdpi = 320, ydpi = 320,
        density = com.android.resources.Density.XHIGH, fontScale = fontScale
    )

    @Test fun telaEstreita() {
        paparazzi.unsafeUpdateConfig(deviceConfig = small(1f))
        paparazzi.shotInShell("estreita", 0) { TodayScreen(TodayState(false, null, Samples.todayLate, Samples.todayDue, emptyList(), emptyList()), {}, {}, {}) }
    }

    @Test fun letraGrande() {
        paparazzi.unsafeUpdateConfig(deviceConfig = small(1.3f))
        paparazzi.shotInShell("letra_grande", 2) { TodayScreen(TodayState(false, null, Samples.todayLate, Samples.todayDue, emptyList(), emptyList()), {}, {}, {}) }
    }

    @Test fun letraMuitoGrandeEscuro() {
        paparazzi.unsafeUpdateConfig(deviceConfig = small(1.6f))
        paparazzi.shotInShell("letra_muito_grande_escuro", 2, dark = true) { TodayScreen(TodayState(false, null, Samples.todayLate, Samples.todayDue, emptyList(), emptyList()), {}, {}, {}) }
    }

    @Test fun oTamanhoDosNomesCabeNaBarra() {
        // A usual phone with the usual font: the wanted size is kept.
        assertEquals(12f, NavLabel.sizeDp(393, names, 12f), 0.001f)
        // A large font setting does not grow the names past what fits one line.
        val large = NavLabel.sizeDp(360, names, 12f * 1.6f)
        assertTrue(large < 12f * 1.6f)
        assertTrue("Documentos".length * 0.6f * large <= 360f / 4 - 12f + 0.001f)
        // A very narrow screen shrinks them, but never below what can be read.
        assertEquals(9f, NavLabel.sizeDp(200, names, 12f), 0.001f)
        // Nothing to measure: the wanted size.
        assertEquals(12f, NavLabel.sizeDp(360, emptyList(), 12f), 0.001f)
    }
}
