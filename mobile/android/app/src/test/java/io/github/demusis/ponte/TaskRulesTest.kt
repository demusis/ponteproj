package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The decisions of the task screen, step by step (no Android). */
class TaskRulesTest {
    private fun form(notes: String = "N1", title: String = "Responder") =
        TaskForm(title, "responder", "normal", "2026-11-12", "Servidor A", notes)

    private fun task(id: Long, version: Long, f: TaskForm = form()) =
        TaskDetail(id, 100 + id, "ORG-OFI-2026/0000$id", "Assunto $id", f, false, version, emptyList())

    private fun relation(id: Long, title: String, parent: Long? = null, done: Boolean = false) =
        TaskItem(id, 0, title, "", "", "", "", done, false, false, parentTaskId = parent)

    @Test fun candidatosExcluemPropriaTarefaDescendentesEConcluidas() {
        val rows = listOf(
            relation(1, "Atual"), relation(2, "Filha", 1, true), relation(3, "Neta", 2),
            relation(4, "Outra"), relation(5, "Concluída", done = true), relation(6, "Filha ativa", 1)
        )
        assertEquals(listOf(4L), parentCandidates(rows, 1, "").map { it.id })
    }

    @Test fun candidatosBuscamTituloSemAcentosOuMaiusculas() {
        val rows = listOf(relation(1, "Atual"), relation(2, "Revisão de Ofício"), relation(3, "Outra"))
        assertEquals(listOf(2L), parentCandidates(rows, 1, "REVISAO de oficio").map { it.id })
        assertTrue(parentCandidates(rows, 1, "inexistente").isEmpty())
    }

    @Test fun dependenciasUsamIdsEEstadosSemConfundirSubtarefas() {
        val rows = listOf(relation(1, "Atual"), relation(2, "Pai"), relation(3, "Filha", 1),
            relation(4, "Concluída", done = true), relation(5, "Atrasada").copy(overdue = true))
        val deps = TaskRules.prerequisites(listOf(5, 4, 2, 5), rows)
        assertEquals(listOf(5L, 4L, 2L), deps.map { it.id })
        assertEquals(listOf("⚠", "✓", "○"), deps.map { TaskRules.prerequisiteSymbol(it) })
    }

    @Test fun semanaContaApenasDependenciasAbertasEDetectaOrdem() {
        val task = relation(1, "Atual", parent = 2).copy(dependencies = listOf(3, 4), scheduledStart = "2026-11-17T10:00")
        val parent = relation(2, "Pai").copy(scheduledEnd = "2026-11-17T15:00")
        val open = relation(3, "Preparar").copy(scheduledEnd = "2026-11-17T11:00")
        val done = relation(4, "Pronta", done = true).copy(scheduledEnd = "2026-11-17T12:00")
        val child = relation(5, "Filha", parent = 1)
        val all = listOf(task, parent, open, done, child)
        assertEquals("⚠ começa antes de terminar Preparar" to true, WeekPlan.dependencyNotice(task, all))
        // A tap on the block or on the deadline opens the task itself.
        assertEquals(listOf(1L), WeekPlan.taskEntries(task.copy(dueDate = "2026-11-20"), all).map { it.taskId }.distinct())
        assertEquals("↳ depende de 1" to false, WeekPlan.dependencyNotice(task.copy(scheduledStart = "2026-11-17T11:00"), all))
        assertNull(WeekPlan.dependencyNotice(task, listOf(task, parent, open.copy(done = true), done, child)))
        assertNull(WeekPlan.dependencyNotice(task.copy(dependencies = emptyList()), all))
    }

    @Test fun candidatasDeDependenciaExcluemCiclosEOrdenamConcluidas() {
        val rows = listOf(relation(1, "Atual"), relation(2, "Direta", done = true).copy(dependencies = listOf(1)),
            relation(3, "Indireta").copy(dependencies = listOf(2)), relation(4, "Revisão", done = true),
            relation(5, "Revisão aberta"), relation(6, "Outra"))
        assertEquals(listOf(5L, 6L, 4L), dependencyCandidates(rows, 1, "").map { it.id })
        assertEquals(listOf(5L, 4L), dependencyCandidates(rows, 1, "REVISAO").map { it.id })
    }

    @Test fun patchDeDependenciasApenasQuandoAlteradas() {
        val base = form().copy(dependencies = listOf(2, 3))
        assertFalse("dependencies" in base.copy(dependencies = listOf(3, 2)).changes(base))
        assertEquals(listOf(2L), base.copy(dependencies = listOf(2)).changes(base)["dependencies"])
        assertEquals(emptyList<Long>(), base.copy(dependencies = emptyList()).changes(base)["dependencies"])
        val draft = TaskDraft(0, 1, base, base.copy(dependencies = listOf(4)))
        assertEquals(draft, TaskDraft.fromList(draft.toList()))
    }

    // ---- reminders and the permission to notify

    @Test fun avisoSemPermissaoSoApareceParaTarefaComAvisos() {
        assertTrue(TaskRules.remindersBlockedShown(listOf(15), blocked = true))
        assertFalse("no offsets, nothing to warn about", TaskRules.remindersBlockedShown(emptyList(), blocked = true))
        assertFalse("permission given", TaskRules.remindersBlockedShown(listOf(15), blocked = false))
        assertTrue(TaskRules.REMINDERS_BLOCKED.lines().size == 1 && TaskRules.REMINDERS_BLOCKED.endsWith("."))
    }

    @Test fun permissaoEPedidaAoAcrescentarUmAvisoENaoAoTirar() {
        assertTrue(TaskRules.offsetAdded(emptyList(), listOf(15)))
        assertTrue(TaskRules.offsetAdded(listOf(15), listOf(15, 60)))
        assertFalse(TaskRules.offsetAdded(listOf(15, 60), listOf(15)))
        assertFalse(TaskRules.offsetAdded(emptyList(), emptyList()))
    }

    // ---- which task the screen shows

    @Test fun telaMostraSoATarefaDaRota() {
        val b = TaskEditState(id = 2, loading = false, task = task(2, 1))
        assertEquals(2L, TaskRules.shown(2, b)?.id)
        assertNull("another route id never gets this task", TaskRules.shown(1, b))
        assertNull(TaskRules.shown(2, TaskEditState(id = 2)))
    }

    // ---- H1: A -> ficha -> B -> Back

    @Test fun h1PilhaTemUmaSoTelaDeTarefa() {
        // A was opened from Tarefas; its document card opened the ficha; the person taps task B there.
        val afterFicha = listOf<Route>(Route.EditTask(1), Route.Document(101))
        val afterB = TaskRules.withTask(afterFicha, 2)
        assertEquals(listOf(Route.Document(101), Route.EditTask(2)), afterB)
        // Back from B shows the ficha, and Back from the ficha leaves: never the editor of A.
        assertEquals(Route.Document(101), afterB.dropLast(1).last())
        assertTrue(afterB.dropLast(1).none { it is Route.EditTask })
    }

    @Test fun h1MesmaTarefaDuasVezesFicaUmaSo() {
        val s = TaskRules.withTask(listOf(Route.EditTask(1), Route.Document(101)), 1)
        assertEquals(listOf(Route.Document(101), Route.EditTask(1)), s)
    }

    @Test fun h1AbrirTiraTambemOSeletorDeDocumento() {
        val s = TaskRules.withTask(listOf(Route.EditTask(1), Route.Pick(1), Route.Document(5)), 2)
        assertEquals(listOf(Route.Document(5), Route.EditTask(2)), s)
    }

    @Test fun h1AposMorteDoProcessoSoVoltaATelaDeCima() {
        val saved = listOf<Route>(Route.EditTask(1), Route.Document(101), Route.EditTask(2), Route.Pick(2))
        assertEquals(listOf(Route.Document(101), Route.EditTask(2), Route.Pick(2)), TaskRules.restored(saved))
        // A list that belongs to a task that is not on screen is dropped.
        assertEquals(listOf(Route.EditTask(2)), TaskRules.restored(listOf(Route.Pick(1), Route.EditTask(2))))
        assertEquals(listOf<Route>(Route.Document(1)), TaskRules.restored(listOf(Route.Pick(1), Route.Document(1))))
    }

    @Test fun h1RespostaDeOutraTarefaNaoAparece() {
        // The state still holds B while the screen of A is asked for: A shows loading, not B's form.
        val state = TaskEditState(id = 1, loading = false, task = task(2, 4))
        assertNull(TaskRules.shown(1, state))
    }

    // ---- H2: the draft and the version

    @Test fun h2RascunhoNovoVemDaTarefaCarregada() {
        val d = TaskRules.start(task(1, 3), 0)
        assertEquals(3L, d.baseVersion)
        assertFalse(d.changed)
        assertFalse(d.conflict)
    }

    @Test fun h2VersaoIgualMantemORascunho() {
        val d = TaskRules.start(task(1, 3), 0).let { it.copy(form = it.form.copy(notes = "digitado")) }
        val r = TaskRules.reconcile(d, task(1, 3), 0)
        assertEquals(d, r)
        assertFalse(r.conflict)
    }

    @Test fun h2VersaoNovaSemMudancaReiniciaSemAviso() {
        // Opened at v1 (notes N1); a sync brings v2 (notes N2); the person had typed nothing.
        val draft = TaskRules.start(task(1, 1, form("N1")), 0)
        val r = TaskRules.reconcile(draft, task(1, 2, form("N2")), 0)
        assertEquals(2L, r.baseVersion)
        assertEquals("N2", r.form.notes)
        assertFalse(r.conflict)
        assertFalse(r.changed)
    }

    @Test fun h2VersaoNovaComMudancaMantemOTextoEAvisa() {
        val draft = TaskRules.start(task(1, 1, form("N1")), 0).let { it.copy(form = it.form.copy(title = "Meu título")) }
        val r = TaskRules.reconcile(draft, task(1, 2, form("N2")), 0)
        assertTrue(r.conflict)
        assertEquals("Meu título", r.form.title)
        assertEquals("the draft keeps the version it started from", 1L, r.baseVersion)
    }

    @Test fun h2SalvarMandaAVersaoDeOrigemEnuncaORevertido() {
        // Process death: the draft (v1, notes N1, untouched title typed) comes back; v2 has notes N2.
        val draft = TaskRules.start(task(1, 1, form("N1")), 0).let { it.copy(form = it.form.copy(title = "Novo")) }
        val r = TaskRules.reconcile(draft, task(1, 2, form("N2")), 0)
        val save = r.toSave(1)
        assertEquals("the old version goes, so the core refuses it", 1L, save.version)
        assertEquals(mapOf("title" to "Novo"), save.changes)
        assertFalse("notes N1 is not sent as a change", "notes" in save.changes)
    }

    @Test fun h2PedirAVersaoAtualDescartaORascunho() {
        val draft = TaskRules.start(task(1, 1, form("N1")), 0).let { it.copy(form = it.form.copy(title = "Meu título")) }
        val conflicting = TaskRules.reconcile(draft, task(1, 2, form("N2")), 0)
        assertTrue(conflicting.conflict)
        // "Ver a versão atual": the screen state's epoch grows, the draft starts over from the loaded task.
        val r = TaskRules.reconcile(conflicting, task(1, 2, form("N2")), 1)
        assertEquals(form("N2"), r.form)
        assertEquals(2L, r.baseVersion)
        assertFalse(r.conflict)
    }

    @Test fun h2DepoisDeSalvarAProximaVersaoNaoDaFalsoConflito() {
        // L1: after a save the stored version moves to 3; the draft that restarts at it saves with 3.
        val saved = TaskRules.reconcile(TaskRules.start(task(1, 2), 0), task(1, 3), 0)
        assertEquals(3L, saved.baseVersion)
        assertFalse(saved.conflict)
    }

    @Test fun rascunhoSobreviveAoEstadoSalvo() {
        val d = TaskRules.start(task(1, 7, form("N1")), 2).let { it.copy(form = it.form.copy(notes = "outro"), conflict = true) }
        assertEquals(d, TaskDraft.fromList(d.toList()))
        assertNull(TaskDraft.fromList(listOf("lixo")))
        assertNotNull(TaskDraft.fromList(d.toList()))
    }

    @Test fun mensagemDeConflitoETextoDoNucleo() {
        assertTrue(TaskRules.isConflict(TaskRules.CHANGED))
        assertFalse(TaskRules.isConflict("Não foi possível salvar a tarefa. Tente de novo."))
    }
}
