package io.github.demusis.ponte

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/** The card of Hoje about renumbered records and the "Número provisório" marker. */
class RegistroRenumeradoTest {
    @get:Rule val paparazzi = newPaparazzi()

    private fun notice(id: Long, doc: Long = 10 + id) = RenumberNotice(
        id, "Ligação do gabinete $id", "MANUAL-2026/000$id", "MANUAL-2026/001$id",
        "O registro “Ligação do gabinete $id” passou de MANUAL-2026/000$id para MANUAL-2026/001$id ao sincronizar.", doc
    )

    @Test fun mostraOsTresMaisNovosEContaOResto() {
        val five = (1L..5L).map { notice(it) }
        assertEquals(listOf(1L, 2L, 3L), RenumberRules.shown(five).map { it.id })
        assertEquals("e mais 2", RenumberRules.moreText(five))
        assertEquals("e mais 1", RenumberRules.moreText(five.take(4)))
        assertEquals("", RenumberRules.moreText(five.take(3)))
        assertEquals("", RenumberRules.moreText(emptyList()))
        assertEquals(1, RenumberRules.shown(five.take(1)).size)
    }

    @Test fun abrirSoQuandoORegistroPodeSerAberto() {
        assertTrue(RenumberRules.canOpen(notice(1, doc = 14)))
        assertFalse(RenumberRules.canOpen(notice(1, doc = 0)))
        assertEquals("Abrir o registro Ligação do gabinete 1", RenumberRules.openDescription(notice(1)))
    }

    @Test fun oAvisoVemDoNucleo() {
        val n = JSONObject(
            """{"id":3,"title":"Ligação do gabinete","from":"MANUAL-2026/0005","to":"MANUAL-2026/0006","sentence":"O registro passou.","document_id":14,"at":"2026-11-22T19:03:35Z"}"""
        ).toRenumberNotice()
        assertEquals(RenumberNotice(3, "Ligação do gabinete", "MANUAL-2026/0005", "MANUAL-2026/0006", "O registro passou.", 14, "2026-11-22T19:03:35Z"), n)
        assertEquals(0L, JSONObject("""{"id":4}""").toRenumberNotice().documentId)
    }

    @Test fun otextoDaLinhaTrazOMarcador() {
        assertEquals("MANUAL-2026/0005", RenumberRules.rowSub("MANUAL-2026/0005", false))
        assertEquals("MANUAL-2026/0005 · Número provisório", RenumberRules.rowSub("MANUAL-2026/0005", true))
        assertEquals("Número provisório", RenumberRules.rowSub("", true))
        assertEquals("", RenumberRules.rowSub("", false))
        assertEquals("MANUAL-2026/0005 · Número provisório", RenumberRules.headerLine("MANUAL-2026/0005", true))
        assertEquals("Número provisório: pode mudar ao sincronizar.", RenumberRules.PROVISIONAL_NOTE)
    }

    @Test fun jsonAntigoSemProvisorioContinuaValendo() {
        assertFalse(Doc(1, "A", "t", "", null).provisional)
    }

    // ------------------------------------------------------------------ pictures

    private val today = TodayState(loading = false, late = Samples.tasks.take(2), summary = "2 atrasadas")

    @Test fun hojeComUmAviso() = paparazzi.shotInShell("hoje_um_aviso", 0) {
        TodayScreen(today, onToggle = {}, onOpen = {}, notices = listOf(notice(1)))
    }

    @Test fun hojeComCincoAvisosEscuro() = paparazzi.shotInShell("hoje_cinco_avisos_escuro", 0, dark = true) {
        TodayScreen(today, onToggle = {}, onOpen = {}, notices = (1L..5L).map { notice(it, doc = if (it == 2L) 0 else 10 + it) })
    }

    private val docs = listOf(
        Doc(201, "MANUAL-2026/0005", "Ligação do gabinete", "", null, type = "Registro", date = "22/11/2026", manual = true, provisional = true),
        Samples.docs[0]
    )

    @Test fun listaComNumeroProvisorio() = paparazzi.shotInShell("lista_provisorio", 2) {
        DocumentsScreen(docs, "", {}, DocFilter.ALL, {}, {}, {})
    }

    @Test fun fichaComNumeroProvisorio() = paparazzi.shot("ficha_provisorio") {
        DocumentScreen(DocumentState(false, null, Samples.detail.copy(number = "MANUAL-2026/0005")), false, {}, {}, {}, {}, {}, {}, {}, provisional = true)
    }

    @Test fun fichaComNumeroProvisorioEscuro() = paparazzi.shot("ficha_provisorio_escuro", dark = true) {
        DocumentScreen(DocumentState(false, null, Samples.detail.copy(number = "MANUAL-2026/0005")), false, {}, {}, {}, {}, {}, {}, {}, provisional = true)
    }
}
