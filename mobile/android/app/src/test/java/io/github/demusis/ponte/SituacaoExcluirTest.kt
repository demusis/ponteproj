package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/** A situação da tarefa (Pendente, Em análise, Aguardando) e o texto de Excluir tarefa, com as telas. */
class SituacaoExcluirTest {
    @get:Rule val paparazzi = newPaparazzi()

    private fun task(status: String = "pendente", done: Boolean = false, documentId: Long = 7) =
        TaskItem(1, documentId, "Verificar possibilidades", "Examinar", "MANUAL-2026/0003", "", "", done, false, false, status = status)

    private val form = TaskForm("Responder ofício", "responder", "normal", "2026-11-12", "Servidor A", "nota")

    // ---------------------------------------------------------------- rules

    @Test fun asOpcoesSaoAsDoComputadorMenosConcluida() {
        assertEquals(listOf("pendente", "em analise", "aguardando"), TaskStatus.openChoices.map { it.key })
        assertEquals(listOf("Pendente", "Em análise", "Aguardando"), TaskStatus.openChoices.map { it.label })
        assertEquals("Concluída", TaskStatus.label("concluida"))
        // An unknown key reads as the first situation, never as an empty word.
        assertEquals("Pendente", TaskStatus.label("sonhando"))
    }

    @Test fun aLinhaDizSoEmAnaliseEAguardando() {
        assertEquals("", TaskStatus.rowWord(task("pendente")))
        assertEquals("Em análise", TaskStatus.rowWord(task("em analise")))
        assertEquals("Aguardando", TaskStatus.rowWord(task("aguardando")))
        // A concluded task says nothing more than "concluída".
        assertEquals("", TaskStatus.rowWord(task("concluida", done = true)))
        assertEquals("", TaskStatus.rowWord(task("aguardando", done = true)))
    }

    @Test fun aSegundaLinhaTrazASituacaoPrimeiroEPreservaOResto() {
        assertEquals("Examinar · MANUAL-2026/0003", TaskRowText.lead(task()))
        assertEquals("Em análise · Examinar · MANUAL-2026/0003", TaskRowText.lead(task("em analise")))
        assertEquals("Aguardando · Examinar · MANUAL-2026/0003", TaskRowText.lead(task("aguardando")))
        assertEquals("Aguardando · Examinar", TaskRowText.lead(task("aguardando", documentId = 0)))
        assertEquals("Em análise", TaskRowText.lead(task("em analise").copy(actionLabel = "", documentId = 0)))
    }

    @Test fun oLeitorDeTelaOuveASituacao() {
        val said = TaskRowText.description(task("aguardando"), emptyList(), true)
        assertTrue(said, said.contains("Aguardando"))
        assertFalse(TaskRowText.description(task(), emptyList(), true).contains("Aguardando"))
    }

    @Test fun aSituacaoMudadaVaiNaMudancaEOutraNao() {
        assertEquals(mapOf<String, Any>("status" to "aguardando"), form.copy(status = "aguardando").changes(form))
        assertTrue(form.changes(form).isEmpty())
        assertFalse(form.copy(status = "pendente").changedFrom(form))
        assertTrue(form.copy(status = "em analise").changedFrom(form))
        // Combined with another field, both travel.
        assertEquals(
            setOf("status", "notes"),
            form.copy(status = "em analise", notes = "outra").changes(form).keys
        )
    }

    @Test fun oRascunhoGuardaASituacaoEAceitaUmRascunhoAntigo() {
        val draft = TaskDraft(2, 5, form, form.copy(status = "aguardando", notes = "x"))
        val back = TaskDraft.fromList(draft.toList())!!
        assertEquals("aguardando", back.form.status)
        assertEquals("pendente", back.base.status)
        assertEquals(draft, back)
        // A draft saved before the field existed (one entry less in each form) still opens, as "pendente".
        val full = draft.toList()
        val old = full.filterIndexed { i, _ -> i != 17 && i != 32 }
        assertEquals(31, old.size)
        val opened = TaskDraft.fromList(old)!!
        assertEquals("pendente", opened.form.status)
        assertEquals("x", opened.form.notes)
    }

    @Test fun oTextoDeExcluirDizLixeiraERestauroNoComputador() {
        val ask = PhoneWords.DELETE_TASK_ASK
        assertTrue(ask, ask.contains("Lixeira"))
        assertTrue(ask, ask.contains("computador"))
        assertTrue(ask, ask.contains("Desfazer"))
        assertEquals("Tarefa excluída.", PhoneWords.TASK_DELETED)
        assertEquals("Tarefa restaurada.", PhoneWords.TASK_RESTORED)
    }

    // ---------------------------------------------------------------- pictures

    private val rows = listOf(
        Samples.tasks[0].copy(status = "em analise"),
        Samples.tasks[2].copy(status = "aguardando", priority = "alta", priorityLabel = "Alta"),
        Samples.tasks[3],
        Samples.tasks[5].copy(status = "aguardando")
    )

    @Test fun listaComSituacao() = paparazzi.shotInShell("lista", 1) { TasksScreen(TasksState(false, null, rows), {}, {}) }

    @Test fun listaComSituacaoEscuro() = paparazzi.shotInShell("lista_escuro", 1, dark = true) { TasksScreen(TasksState(false, null, rows), {}, {}) }

    private fun screen(detail: TaskDetail, dark: Boolean, name: String) {
        paparazzi.tall(3800)
        paparazzi.shot(name, dark) {
            TaskScreen(detail.id, TaskEditState(id = detail.id, loading = false, task = detail), Samples.actions, Samples.priorities, {}, {}, {}, {}, {}, {}, {})
        }
    }

    private val inAnalysis = Samples.taskLinked.copy(form = Samples.taskLinked.form.copy(status = "em analise"))

    @Test fun tarefaComSituacao() = screen(inAnalysis, false, "tarefa")

    @Test fun tarefaComSituacaoEscuro() = screen(inAnalysis, true, "tarefa_escuro")

    @Test fun tarefaConcluidaMostraSituacaoSemEscolha() =
        screen(inAnalysis.copy(done = true, form = inAnalysis.form.copy(status = "concluida")), false, "tarefa_concluida")
}
