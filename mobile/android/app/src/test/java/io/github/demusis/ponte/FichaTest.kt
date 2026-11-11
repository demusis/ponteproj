package io.github.demusis.ponte

import org.junit.Rule
import org.junit.Test

class FichaTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val state = DocumentState(false, null, Samples.detail)

    private fun ficha(s: DocumentState, name: String, dark: Boolean = false, syncing: Boolean = false) =
        paparazzi.shot(name, dark) { DocumentScreen(s, syncing, {}, {}, {}, {}, {}, {}, {}) }

    @Test fun ficha() = ficha(state, "claro")

    @Test fun fichaEscuro() = ficha(state, "escuro", dark = true)

    @Test fun fichaInteira() {
        paparazzi.tall()
        ficha(state, "inteira")
    }

    @Test fun fichaInteiraEscuro() {
        paparazzi.tall()
        ficha(state, "inteira_escuro", dark = true, syncing = true)
    }

    /** A record with almost nothing: no analysis, no tasks, no note, no text, one photo. */
    @Test fun fichaSimples() = ficha(
        DocumentState(
            false, null,
            DocDetail(
                104, "", "Recibo de entrega de equipamentos", "Papel", "06/11/2026", "Almoxarifado central", "", null,
                emptyList(), listOf(FileItem("attachment", 9, 0, "foto-1762430000000.jpg", 840_000, "")), ""
            )
        ),
        "simples"
    )

    @Test fun fichaCarregando() = ficha(DocumentState(), "carregando")

    @Test fun fichaErro() = ficha(DocumentState(false, "Documento não encontrado neste aparelho.", null), "erro")

    @Test fun texto() = paparazzi.shot("claro") { TextReaderScreen(Samples.detail.number, Samples.detail.text, {}) }

    @Test fun textoEscuro() = paparazzi.shot("escuro", dark = true) { TextReaderScreen(Samples.detail.number, Samples.detail.text, {}) }

    @Test fun textoVazio() = paparazzi.shot("vazio") { TextReaderScreen(Samples.detail.number, "", {}) }

    @Test fun nota() = paparazzi.shot("claro") { NoteEditorScreen(Samples.detail.number, Samples.detail.note, {}, {}) }

    @Test fun notaVazia() = paparazzi.shot("vazia_escuro", dark = true) { NoteEditorScreen(Samples.detail.number, "", {}, {}) }

    @Test fun novaTarefa() = paparazzi.shot("claro") { NewTaskScreen(Samples.detail.number, Samples.actions, {}, { _, _, _, _ -> }) }

    @Test fun novaTarefaEscuro() = paparazzi.shot("escuro", dark = true) {
        NewTaskScreen(Samples.detail.number, Samples.actions, {}, { _, _, _, _ -> })
    }
}
