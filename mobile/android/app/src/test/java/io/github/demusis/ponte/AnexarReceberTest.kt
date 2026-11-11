package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test

/** Pictures of "Anexar…" (the ficha and its choice) and of "Guardar no Ponte" (what another app shared). */
class AnexarReceberTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val detail get() = Samples.detail

    /** The choice of the ficha as the dialog shows it (a dialog window is not drawn by the tool). */
    @Composable
    private fun Chooser(description: String) {
        val c = Ponte.colors
        Box(Modifier.fillMaxSize().background(Color(0x99000000)).padding(24.dp), contentAlignment = Alignment.Center) {
            Surface(shape = RoundedCornerShape(28.dp), color = c.surface) {
                Column(Modifier.padding(24.dp)) {
                    Text("Anexar ao documento", style = MaterialTheme.typography.headlineSmall, color = c.text)
                    Spacer(Modifier.height(16.dp))
                    AttachChooser(description, {}, {}, {})
                    Spacer(Modifier.height(8.dp))
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                        TextButton(onClick = {}) { Text("Cancelar") }
                    }
                }
            }
        }
    }

    @Test fun anexarEscolha() = paparazzi.shot("escolha_claro") { Chooser("") }

    @Test fun anexarEscolhaEscuro() = paparazzi.shot("escolha_escuro", dark = true) { Chooser("Via assinada pelo diretor") }

    @Test fun fichaComAnexar() {
        paparazzi.tall()
        paparazzi.shot("ficha_anexar") {
            DocumentScreen(DocumentState(false, null, detail), false, {}, {}, {}, {}, {}, {}, {})
        }
    }

    @Test fun fichaAnexando() {
        paparazzi.tall()
        paparazzi.shot("ficha_anexando", dark = true) {
            DocumentScreen(DocumentState(false, null, detail), false, {}, {}, {}, {}, {}, {}, {}, attachBusy = true)
        }
    }

    @Test fun fichaSemArquivos() = paparazzi.shot("ficha_sem_arquivos") {
        DocumentScreen(
            DocumentState(
                false, null,
                DocDetail(105, "", "Ligação sobre a escala de trabalho", "Ligação telefônica", "09/11/2026", "Servidor A", "", null, emptyList(), emptyList(), "")
            ),
            false, {}, {}, {}, {}, {}, {}, {}
        )
    }

    // ------------------------------------------------------------------ Guardar no Ponte

    private val kinds = Samples.kinds

    private val file = ShareItem("Oficio 0412 resposta.pdf", 1_250_000, "", "", "/privado/in-1.tmp")
    private val text = ShareItem(
        "", 0,
        "Reunião de alinhamento do Projeto Alfa, na sexta, às 14h, na sala 2.\nPauta: prazos do ofício 0412 e a escala de trabalho.\nLevar a planilha de veículos.",
        "Convite: reunião de alinhamento", null
    )
    private val photo = ShareItem("IMG_20261106_101500.jpg", 840_000, "", "", "/privado/img-1.tmp")

    private fun share(state: ShareState, name: String, dark: Boolean = false) =
        paparazzi.shot(name, dark) { ShareScreen(state, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }

    @Test fun arquivo() = share(ShareState(item = file, kinds = kinds, title = "Oficio 0412 resposta"), "arquivo")

    @Test fun arquivoEscuro() = share(ShareState(item = file, kinds = kinds, title = "Oficio 0412 resposta"), "arquivo_escuro", dark = true)

    @Test fun imagem() = share(ShareState(item = photo, kinds = kinds, title = "IMG_20261106_101500"), "imagem")

    @Test fun texto() = share(ShareState(item = text, kinds = kinds, title = text.subject), "texto")

    @Test fun textoEscuro() = share(ShareState(item = text, kinds = kinds, title = text.subject), "texto_escuro", dark = true)

    @Test fun recebendo() = share(ShareState(reading = true, kinds = kinds), "recebendo")

    @Test fun registroDeArquivo() = share(
        ShareState(item = file, kinds = kinds, mode = ShareMode.RECORD, title = "Oficio 0412 resposta", origin = "Servidor A"),
        "registro_arquivo"
    )

    @Test fun registroDeTexto() = share(
        ShareState(item = text, kinds = kinds, mode = ShareMode.RECORD, title = text.subject, date = "2026-11-06"),
        "registro_texto"
    )

    @Test fun registroTituloCurto() = share(
        ShareState(item = file, kinds = kinds, mode = ShareMode.RECORD, title = "ab", titleError = "Informe um título com pelo menos 3 caracteres."),
        "registro_erro_titulo"
    )

    @Test fun registroSalvando() = share(
        ShareState(item = file, kinds = kinds, mode = ShareMode.RECORD, title = "Oficio 0412 resposta", busy = true, progress = "Guardando o registro…"),
        "registro_salvando"
    )

    @Test fun anexarADocumento() = share(
        ShareState(item = file, kinds = kinds, mode = ShareMode.ATTACH, target = Samples.docs.first(), description = "Resposta assinada"),
        "anexar_documento"
    )

    @Test fun anexarADocumentoEscuro() = share(
        ShareState(item = file, kinds = kinds, mode = ShareMode.ATTACH, target = Samples.docs.first()),
        "anexar_documento_escuro", dark = true
    )

    @Test fun anexarErro() = share(
        ShareState(
            item = file, kinds = kinds, mode = ShareMode.ATTACH, target = Samples.docs.first(),
            error = "Este documento já tem 20 anexos, o máximo. Exclua um anexo que não seja mais necessário (ele vai para a Lixeira e pode ser restaurado) ou junte vários arquivos num único PDF."
        ),
        "anexar_erro"
    )

    @Test fun erroAoReceber() = share(
        ShareState(error = ShareRules.tooBigMessage("planilha-grande.pdf", 8L * 1024 * 1024)),
        "erro_receber"
    )
}
