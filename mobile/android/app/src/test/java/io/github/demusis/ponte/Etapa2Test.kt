package io.github.demusis.ponte

import app.cash.paparazzi.DeviceConfig
import org.junit.Rule
import org.junit.Test

/**
 * Pictures that fill the gaps of the other screen tests for the second visual pass: the task form and Ajustes
 * with the font at 1.3 and on the narrowest screen, and the screens that had no dark picture.
 */
class Etapa2Test {
    @get:Rule val paparazzi = newPaparazzi()

    private fun font(scale: Float, widthPx: Int = 1080, heightPx: Int = 2340, dpi: Int = 440) {
        paparazzi.unsafeUpdateConfig(
            deviceConfig = DeviceConfig.PIXEL_5.copy(
                screenWidth = widthPx, screenHeight = heightPx, xdpi = dpi, ydpi = dpi,
                density = if (dpi == 320) com.android.resources.Density.XHIGH else DeviceConfig.PIXEL_5.density, fontScale = scale
            )
        )
    }

    private fun task(dark: Boolean, name: String) = paparazzi.shot(name, dark) {
        TaskScreen(
            Samples.taskLinked.id, TaskEditState(id = Samples.taskLinked.id, loading = false, task = Samples.taskLinked),
            Samples.actions, Samples.priorities, {}, {}, {}, {}, {}, {}, {}
        )
    }

    @Test fun tarefaFonte13() {
        font(1.3f, heightPx = 6200)
        task(false, "fonte_1_3")
    }

    @Test fun tarefaFonte13Escuro() {
        font(1.3f, heightPx = 6200)
        task(true, "fonte_1_3_escuro")
    }

    @Test fun tarefa320() {
        font(1f, 640, 4400, 320)
        task(false, "320")
    }

    @Test fun ajustesFonte13() {
        font(1.3f, heightPx = 9000)
        paparazzi.shot("fonte_1_3") { SettingsScreen(Samples.settings, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }
    }

    @Test fun ajustesFonte13Escuro() {
        font(1.3f, heightPx = 9000)
        paparazzi.shot("fonte_1_3_escuro", dark = true) { SettingsScreen(Samples.settings, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }
    }

    @Test fun ajustes320() {
        font(1f, 640, 6400, 320)
        paparazzi.shot("320") { SettingsScreen(Samples.settings, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }
    }

    @Test fun notaClaraEEscura() {
        paparazzi.shot("escura", dark = true) { NoteEditorScreen(Samples.detail.number, Samples.detail.note, {}, {}) }
    }

    @Test fun setupSincronizandoEscuro() = paparazzi.shot("sincronizando_escuro", dark = true) {
        SetupScreen(SetupState(step = 2, fileName = "ponte-sync.ponte", syncing = true), {}, {}, {}, {}, {}, {}, {}, {})
    }

    @Test fun setupFraseEscuro() = paparazzi.shot("frase_escuro", dark = true) {
        SetupScreen(SetupState(step = 1, fileName = "ponte-sync.ponte", canRemember = true), {}, {}, {}, {}, {}, {}, {}, {})
    }

    @Test fun compartilharRegistroEscuro() = paparazzi.shot("registro_escuro", dark = true) {
        ShareScreen(
            ShareState(item = ShareItem("Oficio 0412 resposta.pdf", 1_250_000, "", "", "/privado/in-1.tmp"), kinds = Samples.kinds, title = "Ofício 0412 resposta", mode = ShareMode.RECORD),
            {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}
        )
    }

    @Test fun compartilharAnexarEscuro() = paparazzi.shot("anexar_escuro", dark = true) {
        ShareScreen(
            ShareState(item = ShareItem("Oficio 0412 resposta.pdf", 1_250_000, "", "", "/privado/in-1.tmp"), kinds = Samples.kinds, mode = ShareMode.ATTACH, target = Samples.docs[0]),
            {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}
        )
    }
}
