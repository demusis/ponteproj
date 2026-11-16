package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The rules of the categories and how they ride in the task draft (no Android). */
class CategoriasRegrasTest {
    private fun cat(uid: String, name: String, color: Int = 0, archived: Boolean = false) = Category(uid, name, color, archived)

    private val all = listOf(
        cat("a", "Urgente", 4), cat("b", "Jurídico", 1), cat("c", "Financeiro", 8), cat("d", "Antiga", 9, archived = true)
    )

    // ---- colour

    @Test fun coresClarasSaoAsDoComputador() {
        // Teal (index 0), light: text = half the hue towards black, background = 16% hue on white.
        val c = CategoryRules.colors(0, dark = false)
        assertEquals(0xFF1E96A0.toInt(), c.dot)
        assertEquals(0xFF0F4B50.toInt(), c.text)
        assertEquals(0xFFDBEEF0.toInt(), c.background)
    }

    @Test fun coresEscurasMisturamComASuperficieEscura() {
        val c = CategoryRules.colors(0, dark = true)
        assertEquals(0xFF1E96A0.toInt(), c.dot)
        // 22% of the hue over the dark surface (26, 36, 40).
        assertEquals(0xFF1B3D42.toInt(), c.background)
        assertNotEquals(CategoryRules.colors(0, true).text, CategoryRules.colors(0, false).text)
    }

    @Test fun cadaIndiceTemUmaCorEIndiceForaDaPaletaFicaNaPaleta() {
        val dots = (0 until CategoryRules.PALETTE_SIZE).map { CategoryRules.colors(it, false).dot }
        assertEquals(CategoryRules.PALETTE_SIZE, dots.toSet().size)
        assertEquals(CategoryRules.colors(9, false), CategoryRules.colors(99, false))
        assertEquals(CategoryRules.colors(0, false), CategoryRules.colors(-3, false))
    }

    // ---- filter

    @Test fun filtroVazioMostraTudo() {
        assertTrue(CategoryRules.matches(emptyList(), emptySet()))
        assertTrue(CategoryRules.matches(listOf("a"), emptySet()))
    }

    @Test fun filtroPegaQualquerUmaDasEscolhidas() {
        val chosen = setOf("a", "b")
        assertTrue(CategoryRules.matches(listOf("b", "c"), chosen))
        assertTrue(CategoryRules.matches(listOf("a"), chosen))
        assertFalse(CategoryRules.matches(listOf("c"), chosen))
        assertFalse(CategoryRules.matches(emptyList(), chosen))
    }

    @Test fun semCategoriaPegaOsQueNaoTemNenhuma() {
        val none = setOf(CategoryRules.NONE)
        assertTrue(CategoryRules.matches(emptyList(), none))
        assertFalse(CategoryRules.matches(listOf("a"), none))
        // Together with a category: either the category or no category.
        val both = setOf(CategoryRules.NONE, "a")
        assertTrue(CategoryRules.matches(emptyList(), both))
        assertTrue(CategoryRules.matches(listOf("a", "c"), both))
        assertFalse(CategoryRules.matches(listOf("c"), both))
    }

    @Test fun filtroLembraSoOQueAindaExiste() {
        val kept = CategoryRules.pruneFilter(setOf("a", "x", CategoryRules.NONE), all)
        assertEquals(setOf("a", CategoryRules.NONE), kept)
    }

    @Test fun marcarEDesmarcarNoFiltro() {
        var s = emptySet<String>()
        s = CategoryRules.toggleFilter(s, "a")
        s = CategoryRules.toggleFilter(s, CategoryRules.NONE)
        assertEquals(setOf("a", CategoryRules.NONE), s)
        assertEquals(setOf(CategoryRules.NONE), CategoryRules.toggleFilter(s, "a"))
        assertEquals("Categorias", CategoryRules.filterLabel(emptySet()))
        assertEquals("Categorias · 2", CategoryRules.filterLabel(s))
    }

    // ---- limit of ten per item

    @Test fun ateDezCategoriasPorItem() {
        var sel = emptyList<String>()
        for (i in 1..CategoryRules.MAX_PER_ITEM) sel = CategoryRules.toggle(sel, "u$i")
        assertEquals(10, sel.size)
        assertTrue(CategoryRules.isFull(sel))
        // The eleventh is not added; one that is on can still be taken off.
        assertEquals(sel, CategoryRules.toggle(sel, "u11"))
        val less = CategoryRules.toggle(sel, "u3")
        assertEquals(9, less.size)
        assertFalse(CategoryRules.isFull(less))
        assertEquals(10, CategoryRules.toggle(less, "u11").size)
    }

    // ---- what the picker offers

    @Test fun listaOferecidaIgnoraMaiusculasEAcentos() {
        assertEquals(listOf("Jurídico"), CategoryRules.offered(all, emptyList(), "juridico").map { it.name })
        assertEquals(listOf("Financeiro"), CategoryRules.offered(all, emptyList(), "  FINAN ").map { it.name })
        assertEquals(listOf("Urgente", "Jurídico", "Financeiro"), CategoryRules.offered(all, emptyList(), "").map { it.name })
    }

    @Test fun arquivadaNaoSeOfereceMasPodeSerRetiradaSeJaEstaNoItem() {
        assertFalse(CategoryRules.offered(all, emptyList(), "").any { it.archived })
        assertFalse(CategoryRules.offered(all, listOf("a"), "").any { it.archived })
        val shown = CategoryRules.offered(all, listOf("d"), "")
        assertEquals("Antiga", shown.last().name)
        // The filter lists an archived category only while it is chosen.
        assertFalse(CategoryRules.filterOptions(all, emptySet()).any { it.archived })
        assertTrue(CategoryRules.filterOptions(all, setOf("d")).any { it.archived })
    }

    @Test fun criarSoQuandoNenhumNomeCombina() {
        assertTrue(CategoryRules.canCreate("Cartório", all))
        assertTrue(CategoryRules.canCreate("  Cartório  ", all))
        assertFalse(CategoryRules.canCreate("", all))
        assertFalse(CategoryRules.canCreate("   ", all))
        assertFalse(CategoryRules.canCreate("urgente", all))
        assertFalse(CategoryRules.canCreate("JURIDICO", all))
        // An archived one still holds its name.
        assertFalse(CategoryRules.canCreate("antiga", all))
        // A part of a name is not the name.
        assertTrue(CategoryRules.canCreate("Urg", all))
    }

    @Test fun criarRespeitaOTamanhoDoNomeEOLimiteDaLista() {
        assertTrue(CategoryRules.canCreate("x".repeat(CategoryRules.MAX_NAME), all))
        assertFalse(CategoryRules.canCreate("x".repeat(CategoryRules.MAX_NAME + 1), all))
        val full = (1..CategoryRules.MAX_CATEGORIES).map { cat("u$it", "Nome $it") }
        assertFalse(CategoryRules.canCreate("Outra", full))
    }

    @Test fun proximaCorEAPrimeiraSemUso() {
        assertEquals(0, CategoryRules.nextColor(emptyList()))
        // Used: 4, 1, 8, 9 -> the first free is 0.
        assertEquals(0, CategoryRules.nextColor(all))
        assertEquals(2, CategoryRules.nextColor(listOf(cat("1", "A", 0), cat("2", "B", 1))))
    }

    @Test fun comTodasAsCoresUsadasVaiParaAMenosUsada() {
        val used = (0 until 10).map { cat("u$it", "N$it", it) } + cat("x", "Extra", 0) + cat("y", "Extra 2", 1)
        // Colours 0 and 1 are used twice; 2 is the first used once.
        assertEquals(2, CategoryRules.nextColor(used))
    }

    // ---- chips

    @Test fun chipsSaoOsQueABibliotecaConhece() {
        assertEquals(listOf("Urgente", "Financeiro"), CategoryRules.resolve(listOf("a", "zzz", "c"), all).map { it.name })
        assertTrue(CategoryRules.resolve(emptyList(), all).isEmpty())
    }

    @Test fun linhaCompactaMostraDoisEMaisN() {
        val three = CategoryRules.resolve(listOf("a", "b", "c"), all)
        val (shown, more) = CategoryRules.compact(three)
        assertEquals(listOf("Urgente", "Jurídico"), shown.map { it.name })
        assertEquals(1, more)
        assertEquals(0, CategoryRules.compact(three.take(2)).second)
        assertEquals(0, CategoryRules.compact(emptyList()).second)
    }

    @Test fun nomesIguaisSemAcentoNemMaiuscula() {
        assertEquals(CategoryRules.nameKey("Jurídico "), CategoryRules.nameKey("JURIDICO"))
        assertNotEquals(CategoryRules.nameKey("Jurídico"), CategoryRules.nameKey("Juridica"))
    }

    // ---- the task draft carries the categories

    private fun form(cats: List<String> = emptyList()) =
        TaskForm("Responder", "responder", "normal", "2026-11-12", "Servidor A", "N1", cats)

    private fun task(version: Long, f: TaskForm) = TaskDetail(1, 101, "ORG-OFI-2026/00001", "Assunto", f, false, version, emptyList())

    @Test fun mudarSoAsCategoriasEhMudancaEEnviaALista() {
        val saved = form(listOf("a"))
        assertTrue(saved.copy(categories = listOf("a", "b")).changedFrom(saved))
        assertEquals(mapOf("categories" to listOf("a", "b")), saved.copy(categories = listOf("a", "b")).changes(saved))
        // Taking all off is a change too, and sends an empty list.
        assertEquals(mapOf("categories" to emptyList<String>()), saved.copy(categories = emptyList()).changes(saved))
    }

    @Test fun mesmaListaEmOutraOrdemNaoEMudanca() {
        val saved = form(listOf("a", "b"))
        assertFalse(saved.copy(categories = listOf("b", "a")).changedFrom(saved))
    }

    @Test fun rascunhoComCategoriasSobreviveAoEstadoSalvo() {
        val t = task(3, form(listOf("a", "b")))
        val d = TaskRules.start(t, 1).let { it.copy(form = it.form.copy(categories = listOf("c"))) }
        assertEquals(d, TaskDraft.fromList(d.toList()))
        assertNotNull(TaskDraft.fromList(TaskRules.start(task(1, form()), 0).toList()))
    }

    @Test fun versaoNovaComCategoriasEscolhidasMantemEAvisa() {
        val stored = task(3, form(listOf("a")))
        val typed = TaskRules.start(stored, 1).let { it.copy(form = it.form.copy(categories = listOf("a", "b"))) }
        val newer = task(4, form(listOf("c")))
        val r = TaskRules.reconcile(typed, newer, 1)
        assertTrue(r.conflict)
        assertEquals(listOf("a", "b"), r.form.categories)
        // Nothing typed: starts over from the newer version, categories included.
        val clean = TaskRules.reconcile(TaskRules.start(stored, 1), newer, 1)
        assertFalse(clean.conflict)
        assertEquals(listOf("c"), clean.form.categories)
    }

    @Test fun salvarMandaSoAsCategoriasQueMudaram() {
        val stored = task(3, form(listOf("a")))
        val d = TaskRules.start(stored, 1).let { it.copy(form = it.form.copy(categories = listOf("a", "b"))) }
        val save = d.toSave(stored.id)
        assertEquals(3L, save.version)
        assertEquals(setOf("categories"), save.changes.keys)
    }
}
