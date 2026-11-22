package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Os filtros e a área escolhida sobrevivem ao fechar o aplicativo: gravar, ler e conferir com o acervo. */
class FiltrosGuardadosTest {
    private fun doc(id: Long, number: String, title: String) = Doc(id, number, title, "", null)

    private val docs = listOf(
        doc(1, "SEI 0001/2026", "Plano de manejo da BNBO"),
        doc(2, "OF 12/2026", "Auditoria dos contratos"),
        doc(3, "", "Nota manual sem número")
    )

    private fun saved(id: Long, number: String, title: String) = SavedDocument(id, FilterMemory.fingerprint(number, title))

    @Test fun oTextoVaiEVoltaIgual() {
        val all = SavedFilters(
            taskCategories = setOf("u1", CategoryRules.NONE), docCategories = setOf("u2"), project = "p-1",
            document = saved(2, "OF 12/2026", "Auditoria dos contratos"), segment = TasksSegments.PROJECTS, docFilter = DocFilter.EMAILS
        )
        assertEquals(all, FilterMemory.decode(FilterMemory.encode(all)))
    }

    @Test fun osValoresPadraoVoltamPadrao() {
        assertEquals(SavedFilters(), FilterMemory.decode(FilterMemory.encode(SavedFilters())))
        assertEquals(SavedFilters(), FilterMemory.decode(null))
        assertEquals(SavedFilters(), FilterMemory.decode(""))
    }

    @Test fun semDocumentoEUmaEscolhaQueSeGuarda() {
        val all = SavedFilters(document = SavedDocument(0L, ""))
        assertEquals(all, FilterMemory.decode(FilterMemory.encode(all)))
        assertEquals(0L, FilterMemory.restoreDocument(all.document, docs))
        assertEquals(0L, FilterMemory.restoreDocument(all.document, emptyList()))
    }

    @Test fun textoDeOutraVersaoOuEstragadoNaoQuebra() {
        assertEquals(SavedFilters(), FilterMemory.decode("v9\ntc=u1"))
        assertEquals(SavedFilters(), FilterMemory.decode("qualquer coisa"))
        val damaged = FilterMemory.decode("v1\ntc=u1,,u2\nsg=abc\ndf=NADA\ndd=xyz:abc\npj=p1\nsem igual\nlinha=desconhecida")
        assertEquals(setOf("u1", "u2"), damaged.taskCategories)
        assertEquals(TasksSegments.TASKS, damaged.segment)
        assertEquals(DocFilter.ALL, damaged.docFilter)
        assertNull(damaged.document)
        assertEquals("p1", damaged.project)
        // An area that does not exist any more falls back to a valid one.
        assertEquals(TasksSegments.PROJECTS, FilterMemory.decode("v1\nsg=99").segment)
        assertEquals(TasksSegments.TASKS, FilterMemory.decode("v1\nsg=-4").segment)
        // A document id without a fingerprint, or negative, means no document filter.
        assertNull(FilterMemory.decode("v1\ndd=5").document)
        assertNull(FilterMemory.decode("v1\ndd=5:").document)
        assertNull(FilterMemory.decode("v1\ndd=-3:abc").document)
    }

    @Test fun oTextoNaoTrazTituloNemNumeroDoDocumento() {
        val text = FilterMemory.encode(SavedFilters(document = FilterMemory.savedDocument(1L, docs)))
        assertFalse(text.contains("BNBO"))
        assertFalse(text.contains("SEI"))
        assertEquals(1, text.lines().count { it.startsWith("dd=") })
    }

    @Test fun valoresComQuebraDeLinhaNaoDesalinham() {
        val text = FilterMemory.encode(SavedFilters(taskCategories = setOf("a\nb,c"), project = "x\ny"))
        assertEquals(7, text.lines().size)
        val back = FilterMemory.decode(text)
        assertEquals(setOf("abc"), back.taskCategories)
        assertEquals("xy", back.project)
    }

    @Test fun oDocumentoVoltaQuandoIgualNoMesmoId() {
        val kept = FilterMemory.savedDocument(2L, docs)
        assertEquals(2L, FilterMemory.restoreDocument(kept, docs))
        assertNull(FilterMemory.savedDocument(null, docs))
        assertNull(FilterMemory.savedDocument(99L, docs))
    }

    @Test fun oIdMudouNaSincronizacaoMasODocumentoEOMesmo() {
        val kept = FilterMemory.savedDocument(2L, docs)
        // After a sync the document has another local id: the one with the same number and title is found.
        val renumbered = listOf(doc(10, "SEI 0001/2026", "Plano de manejo da BNBO"), doc(11, "OF 12/2026", "Auditoria dos contratos"))
        assertEquals(11L, FilterMemory.restoreDocument(kept, renumbered))
    }

    @Test fun oIdPassouParaOutroDocumentoELimpaEmVezDeFiltrarPeloErrado() {
        val kept = FilterMemory.savedDocument(2L, docs)
        // Id 2 now names another document, and the saved one is nowhere: no filter at all.
        val other = listOf(doc(2, "OF 99/2026", "Outro assunto"), doc(3, "", "Nota manual sem número"))
        assertNull(FilterMemory.restoreDocument(kept, other))
    }

    @Test fun oIdBateMasOConteudoNaoEOutroIgualTemPrecedencia() {
        val kept = FilterMemory.savedDocument(1L, docs)
        // Id 1 is a different document now; the saved one moved to id 7: it is found by number and title.
        val moved = listOf(doc(1, "X 1", "Diferente"), doc(7, "SEI 0001/2026", "Plano de manejo da BNBO"))
        assertEquals(7L, FilterMemory.restoreDocument(kept, moved))
    }

    @Test fun doisDocumentosIguaisSemOIdCertoNaoSeEscolheNenhum() {
        val kept = FilterMemory.savedDocument(1L, docs)
        val twins = listOf(doc(5, "SEI 0001/2026", "Plano de manejo da BNBO"), doc(6, "SEI 0001/2026", "Plano de manejo da BNBO"))
        assertNull(FilterMemory.restoreDocument(kept, twins))
    }

    @Test fun comGemeosOIdSalvoTambemNaoVale() {
        val kept = FilterMemory.savedDocument(1L, docs)
        // The saved id still exists and still has the same number and title, but so does another document:
        // nothing says which one it was, so the filter is cleared (even if the id is one of the twins).
        val twinsWithSavedId = listOf(doc(1, "SEI 0001/2026", "Plano de manejo da BNBO"), doc(6, "SEI 0001/2026", "Plano de manejo da BNBO"))
        assertNull(FilterMemory.restoreDocument(kept, twinsWithSavedId))
        // With a single match, the id that matches is restored (as is the unique document under another id).
        assertEquals(1L, FilterMemory.restoreDocument(kept, docs))
        assertEquals(6L, FilterMemory.restoreDocument(kept, listOf(doc(1, "Outro", "Outro"), doc(6, "SEI 0001/2026", "Plano de manejo da BNBO"))))
    }

    @Test fun umAcervoNovoAbreSemFiltrosEOArquivoOutroLimpa() {
        // What a new library opens with: nothing chosen anywhere, the first area, the "all" chip, no document.
        val cleared = FilterMemory.CLEARED
        assertTrue(cleared.taskCategories.isEmpty() && cleared.docCategories.isEmpty())
        assertEquals("", cleared.project)
        assertNull(cleared.document)
        assertEquals(TasksSegments.TASKS, cleared.segment)
        assertEquals(DocFilter.ALL, cleared.docFilter)
        // What is kept after clearing reads back as nothing, including "Sem documento".
        assertEquals(SavedFilters(), FilterMemory.decode(FilterMemory.encode(cleared)))
        // Picking another sync file clears; picking the same one again does not.
        assertTrue(FilterMemory.clearsOnFilePick("content://drive/a", "content://drive/b"))
        assertTrue(FilterMemory.clearsOnFilePick(null, "content://drive/b"))
        assertFalse(FilterMemory.clearsOnFilePick("content://drive/a", "content://drive/a"))
    }

    @Test fun oDocumentoSumiuOuOAcervoAindaEstaVazio() {
        val kept = FilterMemory.savedDocument(2L, docs)
        assertNull(FilterMemory.restoreDocument(kept, docs.filter { it.id != 2L }))
        assertNull(FilterMemory.restoreDocument(kept, emptyList()))
        assertNull(FilterMemory.restoreDocument(null, docs))
        assertNull(FilterMemory.restoreDocument(SavedDocument(2, ""), docs))
    }

    @Test fun aImpressaoDependeDoNumeroEDoTituloSoDeles() {
        assertEquals(FilterMemory.fingerprint("A 1", "Título"), FilterMemory.fingerprint(" A 1 ", "Título "))
        assertNotEquals(FilterMemory.fingerprint("A 1", "Título"), FilterMemory.fingerprint("A 2", "Título"))
        assertNotEquals(FilterMemory.fingerprint("A", "1 Título"), FilterMemory.fingerprint("A 1", "Título"))
        assertEquals(32, FilterMemory.fingerprint("A", "B").length)
    }

    @Test fun osFiltrosQueNaoExistemMaisContinuamSendoPodados() {
        // What comes back is cut by the same rules as before once the library is read.
        val kept = FilterMemory.decode(FilterMemory.encode(SavedFilters(taskCategories = setOf("u1", "gone", CategoryRules.NONE), project = "p-gone")))
        val pruned = CategoryRules.pruneFilter(kept.taskCategories, listOf(Category("u1", "Estudo", 1)))
        assertEquals(setOf("u1", CategoryRules.NONE), pruned)
        assertEquals("", ProjectRules.pruneFilter(kept.project, emptyList()))
        val keptNone = FilterMemory.decode(FilterMemory.encode(SavedFilters(project = ProjectRules.NONE)))
        assertEquals(ProjectRules.NONE, ProjectRules.pruneFilter(keptNone.project, emptyList()))
        assertTrue(FilterSummary.isActive(kept.taskCategories, kept.project, null))
    }
}
