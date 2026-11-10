package io.github.demusis.ponte

import org.junit.Rule
import org.junit.Test

class FotoSetupTest {
    @get:Rule val paparazzi = newPaparazzi()

    private fun photo(state: PhotoState, name: String, dark: Boolean = false) =
        paparazzi.shot(name, dark) { PhotoScreen(state, {}, {}, {}, {}, {}, {}, {}, {}) }

    private val filled
        get() = PhotoState(
            kinds = Samples.kinds, kind = "papel", title = "Recibo de entrega de equipamentos", date = "2026-11-06",
            origin = "Almoxarifado central", text = "", thumb = Samples.photo(), hasPhoto = true
        )

    @Test fun foto() = photo(filled, "claro")

    @Test fun fotoEscuro() = photo(filled, "escuro", dark = true)

    @Test fun fotoInteira() {
        paparazzi.tall(3400)
        photo(filled.copy(text = "Recebido por Servidor B em 06/11."), "inteira")
    }

    /** Just back from the camera, nothing typed yet, the picture still loading. */
    @Test fun fotoNova() = photo(PhotoState(kinds = Samples.kinds, hasPhoto = true), "nova")

    @Test fun fotoSemFoto() = photo(PhotoState(kinds = Samples.kinds, title = "Recibo", error = "Tire a foto antes de salvar."), "sem_foto")

    @Test fun fotoErroTitulo() = photo(filled.copy(title = "", titleError = "Informe o título."), "erro_titulo")

    @Test fun fotoSalvando() = photo(filled.copy(saving = true, progress = "Preparando a foto…"), "salvando")

    private fun setup(state: SetupState, name: String, dark: Boolean = false) =
        paparazzi.shot(name, dark) { SetupScreen(state, {}, {}, {}, {}, {}, {}, {}, {}) }

    private val synced = "Sincronizado: 42 itens novos e 0 atualizados recebidos • gravado no Drive"

    @Test fun setupArquivo() = setup(SetupState(step = 0), "1_arquivo")

    @Test fun setupArquivoEscolhido() = setup(SetupState(step = 0, fileName = "ponte-sync.ponte"), "1_arquivo_escolhido")

    @Test fun setupArquivoEscuro() = setup(SetupState(step = 0), "1_arquivo_escuro", dark = true)

    @Test fun setupFrase() = setup(
        SetupState(step = 1, fileName = "ponte-sync.ponte", passphrase = "frase muito secreta", remember = true), "2_frase"
    )

    @Test fun setupFraseSemBiometria() = setup(
        SetupState(step = 1, fileName = "ponte-sync.ponte", canRemember = false), "2_frase_sem_biometria"
    )

    @Test fun setupFraseAndroidAntigo() = setup(
        SetupState(step = 1, fileName = "ponte-sync.ponte", canRemember = false, oldAndroid = true), "2_frase_android_antigo"
    )

    @Test fun setupSincronizando() = setup(SetupState(step = 2, fileName = "ponte-sync.ponte", syncing = true), "3_sincronizando")

    @Test fun setupPronto() = setup(SetupState(step = 2, fileName = "ponte-sync.ponte", status = synced, done = true), "3_pronto")

    @Test fun setupProntoEscuro() = setup(
        SetupState(step = 2, fileName = "ponte-sync.ponte", status = synced, done = true), "3_pronto_escuro", dark = true
    )

    @Test fun setupErro() = setup(
        SetupState(
            step = 2, fileName = "ponte-sync.ponte",
            status = "Senha de sincronização incorreta, ou o arquivo está danificado ou ainda sendo gravado pelo Drive. " +
                "Confira a senha e espere o Drive terminar de sincronizar.",
            statusError = true
        ),
        "3_erro"
    )
}
