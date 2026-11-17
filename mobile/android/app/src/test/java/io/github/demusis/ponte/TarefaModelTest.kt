package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The plain helpers behind Hoje and the task screen. */
class TarefaModelTest {
    private val saved = TaskForm("Responder ofício", "responder", "normal", "2026-11-12", "Servidor A", "nota")

    @Test fun formSemMudancaNaoMudou() {
        assertFalse(saved.changedFrom(saved))
        assertTrue(saved.changes(saved).isEmpty())
    }

    @Test fun espacosNoTituloEmResponsavelNaoContam() {
        val typed = saved.copy(title = "  Responder ofício ", assignee = "Servidor A  ")
        assertFalse(typed.changedFrom(saved))
    }

    @Test fun cadaCampoMudaSozinho() {
        assertEquals(mapOf("title" to "Outro"), saved.copy(title = "Outro").changes(saved))
        assertEquals(mapOf("action" to "arquivar"), saved.copy(action = "arquivar").changes(saved))
        assertEquals(mapOf("priority" to "alta"), saved.copy(priority = "alta").changes(saved))
        assertEquals(mapOf("due_date" to ""), saved.copy(dueDate = "").changes(saved))
        assertEquals(mapOf("assignee" to "Servidor B"), saved.copy(assignee = " Servidor B ").changes(saved))
        assertEquals(mapOf("notes" to "nota 2"), saved.copy(notes = "nota 2").changes(saved))
    }

    @Test fun soOQueMudouVaiParaONucleo() {
        val typed = saved.copy(priority = "urgente", dueDate = "2026-11-20")
        assertEquals(setOf("priority", "due_date"), typed.changes(saved).keys)
        assertTrue(typed.changedFrom(saved))
    }

    @Test fun linhaDoDocumento() {
        val d = TaskDetail(1, 101, "ORG-OFI-2026/00431", "Pedido de apoio", saved, false, 2, emptyList())
        assertEquals("ORG-OFI-2026/00431 — Pedido de apoio", d.documentLine)
        assertEquals("Pedido de apoio", d.copy(documentNumber = "").documentLine)
        assertEquals("ORG-OFI-2026/00431", d.copy(documentTitle = " ").documentLine)
        assertEquals("", d.copy(documentId = 0, documentNumber = "", documentTitle = "").documentLine)
    }

    private fun item(documentId: Long, number: String, action: String = "Responder", priority: String = "normal", done: Boolean = false) =
        TaskItem(1, documentId, "t", action, number, "", "", done, false, false, priority = priority)

    @Test fun segundaLinhaDizSemDocumento() {
        assertEquals("Responder · ORG-OFI-2026/00431", item(5, "ORG-OFI-2026/00431").secondLine)
        assertEquals("Responder · Sem documento", item(0, "").secondLine)
        assertEquals("Sem documento", item(0, "", action = "").secondLine)
    }

    @Test fun prioridadeSoApareceQuandoNaoENormal() {
        assertNull(priorityFlag(item(1, "x")))
        assertNull(priorityFlag(item(1, "x", priority = "urgente", done = true)))
        assertEquals("Urgente", priorityFlag(item(1, "x", priority = "urgente"))?.first)
        assertEquals("Prioridade alta", priorityFlag(item(1, "x", priority = "alta"))?.first)
        assertEquals("Prioridade baixa", priorityFlag(item(1, "x", priority = "baixa"))?.first)
    }
}
