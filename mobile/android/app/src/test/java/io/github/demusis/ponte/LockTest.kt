package io.github.demusis.ponte

import org.junit.Rule
import org.junit.Test

/** Pictures of the lock screen and of the security section of Ajustes. */
class LockTest {
    @get:Rule val paparazzi = newPaparazzi()

    @Test fun bloqueio() = paparazzi.shot("claro") { LockScreen({}) }

    @Test fun bloqueioEscuro() = paparazzi.shot("escuro", dark = true) { LockScreen({}) }

    private fun settings(state: SettingsState, name: String, dark: Boolean = false) {
        paparazzi.tall(3800)
        paparazzi.shot(name, dark) { SettingsScreen(state, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }
    }

    @Test fun ajustesBloqueioLigado() = settings(Samples.settings.copy(lockOn = true), "ajustes_ligado")

    @Test fun ajustesBloqueioLigadoEscuro() = settings(Samples.settings.copy(lockOn = true), "ajustes_ligado_escuro", dark = true)

    @Test fun ajustesBloqueioDesligado() = settings(Samples.settings.copy(lockOn = false, secureScreen = false), "ajustes_desligado")

    @Test fun ajustesSemBloqueioDeTela() = settings(
        Samples.settings.copy(lockAvailable = false, lockOn = false), "ajustes_sem_bloqueio_de_tela"
    )

    @Test fun ajustesSemBloqueioDeTelaEscuro() = settings(
        Samples.settings.copy(lockAvailable = false, lockOn = false), "ajustes_sem_bloqueio_de_tela_escuro", dark = true
    )
}
