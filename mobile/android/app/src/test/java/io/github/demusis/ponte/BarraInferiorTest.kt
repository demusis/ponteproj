package io.github.demusis.ponte

import app.cash.paparazzi.DeviceConfig
import org.junit.Rule
import org.junit.Test

/** The bottom bar (icons only) on small screens and with a large font setting. */
class BarraInferiorTest {
    @get:Rule val paparazzi = newPaparazzi()

    private fun small(fontScale: Float) = DeviceConfig.PIXEL_5.copy(
        screenWidth = 720, screenHeight = 1440, xdpi = 320, ydpi = 320,
        density = com.android.resources.Density.XHIGH, fontScale = fontScale
    )

    @Test fun telaEstreita() {
        paparazzi.unsafeUpdateConfig(deviceConfig = small(1f))
        paparazzi.shotInShell("estreita", 0) { TodayScreen(TodayState(false, null, Samples.todayLate, Samples.todayDue, emptyList(), emptyList(), summary = "3 tarefas atrasadas · 1 para hoje"), {}, {}, {}) }
    }

    @Test fun letraGrande() {
        paparazzi.unsafeUpdateConfig(deviceConfig = small(1.3f))
        paparazzi.shotInShell("letra_grande", 2) { TodayScreen(TodayState(false, null, Samples.todayLate, Samples.todayDue, emptyList(), emptyList(), summary = "3 tarefas atrasadas · 1 para hoje"), {}, {}, {}) }
    }

    @Test fun letraMuitoGrandeEscuro() {
        paparazzi.unsafeUpdateConfig(deviceConfig = small(1.6f))
        paparazzi.shotInShell("letra_muito_grande_escuro", 2, dark = true) { TodayScreen(TodayState(false, null, Samples.todayLate, Samples.todayDue, emptyList(), emptyList(), summary = "3 tarefas atrasadas · 1 para hoje"), {}, {}, {}) }
    }
}
