package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Surface
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/** The document filter of Tarefas: the rules, and the sheet with its Documento section. */
class FiltroDocumentoTest {
    @get:Rule val paparazzi = newPaparazzi()

    private fun doc(id: Long, number: String, title: String) = Doc(id, number, title, "", null)

    private val docs = listOf(
        doc(1, "SEI 0001/2026", "Plano de manejo da BNBO"),
        doc(2, "OF 12/2026", "Auditoria dos contratos"),
        doc(3, "", "Nota manual sem número")
    )

    @Test fun combina() {
        assertTrue(TaskDocumentFilter.matches(5, null))
        assertTrue(TaskDocumentFilter.matches(0, 0L))
        assertFalse(TaskDocumentFilter.matches(5, 0L))
        assertTrue(TaskDocumentFilter.matches(5, 5L))
        assertFalse(TaskDocumentFilter.matches(0, 5L))
        assertFalse(TaskDocumentFilter.matches(4, 5L))
    }

    @Test fun rotulos() {
        assertEquals("", TaskDocumentFilter.label(null, docs))
        assertEquals("Sem documento", TaskDocumentFilter.label(0L, docs))
        assertEquals("SEI 0001/2026 · Plano de manejo da BNBO", TaskDocumentFilter.label(1L, docs))
        assertEquals("Nota manual sem número", TaskDocumentFilter.label(3L, docs))
        assertEquals("Documento indisponível", TaskDocumentFilter.label(99L, docs))
    }

    @Test fun buscaPorNumeroOuTituloSemAcento() {
        assertEquals(listOf(1L), TaskDocumentFilter.options(docs, "bnbo").map { it.id })
        assertEquals(listOf(2L), TaskDocumentFilter.options(docs, "of 12").map { it.id })
        assertEquals(listOf(3L), TaskDocumentFilter.options(docs, "NUMERO").map { it.id })
        assertEquals(3, TaskDocumentFilter.options(docs, "").size)
    }

    @Test fun aposSincronizarMantemOuLimpa() {
        assertNull(TaskDocumentFilter.afterRefresh(null, docs, docs))
        assertEquals(0L, TaskDocumentFilter.afterRefresh(0L, docs, emptyList()))
        assertEquals(1L, TaskDocumentFilter.afterRefresh(1L, docs, docs))
        assertNull(TaskDocumentFilter.afterRefresh(1L, docs, docs.drop(1)))
        // The id now names another document: clear instead of filtering by the wrong one.
        assertNull(TaskDocumentFilter.afterRefresh(1L, docs, listOf(doc(1, "X 9", "Outro"))))
    }

    @Test fun resumoEIcone() {
        assertTrue(FilterSummary.isActive(emptySet(), "", 0L))
        assertEquals("Filtro: SEI 0001/2026 · Plano de manejo da BNBO", FilterSummary.line(emptyList(), "", TaskDocumentFilter.label(1L, docs)))
        assertTrue(FilterIcon.tasksVisible(Samples.shell.copy(tab = 1), emptyList(), emptySet(), emptyList(), "", docs, null))
    }

    private val tasks = listOf(
        TaskItem(1, 1, "Revisar o plano", "", "", "", "", false, false, false),
        TaskItem(2, 2, "Responder o ofício", "", "", "", "", false, false, false)
    )

    private fun sheet(dark: Boolean, name: String, chosen: Long?) =
        paparazzi.shotInShell(name, 1, dark = dark, filter = ShellFilter(chosen != null) {}) {
            Box(Modifier.fillMaxSize()) {
                TasksScreen(TasksState(false, null, tasks), {}, {}, documents = docs, documentFilter = chosen)
                Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.32f)))
                Surface(
                    Modifier.align(Alignment.BottomCenter), color = Ponte.colors.surface,
                    shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)
                ) {
                    FilterSheetContent(null, emptySet(), {}, null, "", {}, {}, {}, documents = docs, documentChosen = chosen)
                }
            }
        }

    @Test fun folhaComDocumento() = sheet(false, "folha_documento", 1L)
    @Test fun folhaComDocumentoEscuro() = sheet(true, "folha_documento_escuro", null)
}
