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

    private fun doc(id: Long, number: String, title: String, open: Int = 0) = Doc(id, number, title, "", null, openTasks = open)

    private val docs = listOf(
        doc(1, "SEI 0001/2026", "Plano de manejo da BNBO", open = 10),
        doc(2, "OF 12/2026", "Auditoria dos contratos", open = 1),
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

    @Test fun aFolhaListaPrimeiroOsDocumentosComTarefas() {
        // Without a search: only the ones with open tasks, and the chosen one whatever it has.
        assertEquals(listOf(2L, 1L), TaskDocumentFilter.sheetOptions(docs, "", null).map { it.id })
        assertEquals(listOf(3L, 2L, 1L), TaskDocumentFilter.sheetOptions(docs, "", 3L).map { it.id })
        assertEquals(listOf(1L, 2L), TaskDocumentFilter.sheetOptions(docs, "", 1L).map { it.id })
        // A search reaches every document; the ones with open tasks come first.
        assertEquals(listOf(3L), TaskDocumentFilter.sheetOptions(docs, "manual", null).map { it.id })
        assertEquals(listOf(2L, 1L, 3L), TaskDocumentFilter.sheetOptions(docs, "a", null).map { it.id })
        assertFalse(TaskDocumentFilter.showSearch(docs))
        assertTrue(TaskDocumentFilter.showSearch(List(9) { doc(it + 1L, "N $it", "T $it") }))
    }

    @Test fun asDuasLinhasDoDocumento() {
        assertEquals("Plano de manejo da BNBO", TaskDocumentFilter.headline(docs[0]))
        assertEquals("X 1", TaskDocumentFilter.headline(doc(9, "X 1", "")))
        assertEquals("SEI 0001/2026 · 10 tarefas", TaskDocumentFilter.detail(docs[0]))
        assertEquals("OF 12/2026 · 1 tarefa", TaskDocumentFilter.detail(docs[1]))
        assertEquals("sem tarefas em aberto", TaskDocumentFilter.detail(docs[2]))
        assertEquals("", TaskDocumentFilter.shortLabel(null, docs))
        assertEquals("Sem documento", TaskDocumentFilter.shortLabel(0L, docs))
        assertEquals("Plano de manejo da BNBO", TaskDocumentFilter.shortLabel(1L, docs))
        assertEquals("Documento indisponível", TaskDocumentFilter.shortLabel(99L, docs))
    }

    @Test fun oBotaoDizOQueSobra() {
        assertEquals("Pronto", FilterSummary.doneLabel(null))
        assertEquals("Nenhuma tarefa em aberto", FilterSummary.doneLabel(0))
        assertEquals("Ver 1 tarefa", FilterSummary.doneLabel(1))
        assertEquals("Ver 10 tarefas", FilterSummary.doneLabel(10))
        assertEquals("Todos", FilterSummary.sectionValue(""))
        assertEquals("Projeto X", FilterSummary.sectionValue("Projeto X"))
        val all = listOf(
            TaskItem(1, 1, "a", "", "", "", "", false, false, false, projectUid = "p1"),
            TaskItem(2, 1, "b", "", "", "", "", true, false, false),
            TaskItem(3, 0, "c", "", "", "", "", false, false, false)
        )
        assertEquals(2, FilterSummary.openCount(all, emptySet(), "", null))
        assertEquals(1, FilterSummary.openCount(all, emptySet(), "", 1L))
        assertEquals(1, FilterSummary.openCount(all, emptySet(), "", 0L))
        assertEquals(1, FilterSummary.openCount(all, emptySet(), "p1", 1L))
        assertEquals(0, FilterSummary.openCount(all, emptySet(), "p1", 0L))
    }

    @Test fun umaSecaoAbertaPorVezEUmaEscolhaPorVez() {
        assertEquals(FilterSection.NONE, FilterSections.initial(true, true, true))
        assertEquals(FilterSection.NONE, FilterSections.initial(false, true, true))
        assertEquals(FilterSection.PROJECT, FilterSections.initial(false, true, false))
        assertEquals(FilterSection.DOCUMENT, FilterSections.initial(false, false, true))
        assertEquals(FilterSection.NONE, FilterSections.initial(true, false, true))
        assertEquals(FilterSection.DOCUMENT, FilterSections.toggle(FilterSection.PROJECT, FilterSection.DOCUMENT))
        assertEquals(FilterSection.NONE, FilterSections.toggle(FilterSection.DOCUMENT, FilterSection.DOCUMENT))
        assertEquals(5L, FilterSections.pick<Long?>(null, 5L, null))
        assertNull(FilterSections.pick<Long?>(5L, 5L, null))
        assertEquals("", FilterSections.pick("p1", "p1", ""))
    }

    private val tasks = listOf(
        TaskItem(1, 1, "Revisar o plano", "", "", "", "", false, false, false),
        TaskItem(2, 2, "Responder o ofício", "", "", "", "", false, false, false)
    )
    private val library = listOf(
        Category("u1", "Estudo", 1), Category("u2", "Jurídico", 4), Category("u3", "Financeiro", 8), Category("u4", "Viagem de trabalho", 5)
    )
    private val projects = listOf(
        ProjectItem("p1", "Mudança da sede para o novo prédio", "", "", "ativo", "Ativo", "", "", 1, ProjectProgress()),
        ProjectItem("p2", "Auditoria interna", "", "", "ativo", "Ativo", "", "", 1, ProjectProgress()),
        ProjectItem("p3", "Arquivo morto", "", "", ProjectRules.ARCHIVED, "Arquivado", "", "", 1, ProjectProgress())
    )

    private fun sheet(
        dark: Boolean, name: String, cats: Set<String> = emptySet(), project: String = "", chosen: Long? = null,
        open: FilterSection? = null, full: Boolean = true, many: Boolean = false
    ) = paparazzi.shotInShell(name, 1, dark = dark, filter = ShellFilter(FilterSummary.isActive(cats, project, chosen)) {}) {
        val list = if (many) docs + List(8) { doc(20L + it, "MEMO ${it + 1}/2026", "Memorando ${it + 1}", open = it % 3) } else docs
        Box(Modifier.fillMaxSize()) {
            TasksScreen(TasksState(false, null, tasks), {}, {}, documents = list, documentFilter = chosen)
            Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.32f)))
            Surface(
                Modifier.align(Alignment.BottomCenter), color = Ponte.colors.surface,
                shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)
            ) {
                FilterSheetContent(
                    if (full) library else null, cats, {}, if (full) projects else null, project, {}, {}, {},
                    documents = list, documentChosen = chosen, initialSection = open,
                    resultCount = FilterSummary.openCount(tasks, emptySet(), "", chosen)
                )
            }
        }
    }

    @Test fun folhaFechada() = sheet(false, "folha_fechada")
    @Test fun folhaFechadaComEscolhas() = sheet(false, "folha_escolhas", setOf("u1", "u3"), "p1", 1L)
    @Test fun folhaFechadaComEscolhasEscuro() = sheet(true, "folha_escolhas_escuro", setOf("u1", "u3"), "p1", 1L)
    @Test fun folhaComDocumentoAberto() = sheet(false, "folha_documento", setOf("u2"), chosen = 1L, open = FilterSection.DOCUMENT)
    @Test fun folhaComDocumentoAbertoEscuro() = sheet(true, "folha_documento_escuro", chosen = 1L, open = FilterSection.DOCUMENT, many = true)
    @Test fun folhaComProjetoAberto() = sheet(false, "folha_projeto", project = "p2", open = FilterSection.PROJECT)
    @Test fun folhaSoDeDocumentos() = sheet(false, "folha_so_documento", full = false, many = true)
}
