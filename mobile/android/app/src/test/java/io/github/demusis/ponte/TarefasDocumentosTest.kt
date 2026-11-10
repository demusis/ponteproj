package io.github.demusis.ponte

import org.junit.Rule
import org.junit.Test

class TarefasDocumentosTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val all = TasksState(false, null, Samples.tasks)

    @Test fun tarefas() = paparazzi.shotInShell("claro", 0) { TasksScreen(all, {}, {}) }

    @Test fun tarefasEscuro() = paparazzi.shotInShell("escuro", 0, dark = true) { TasksScreen(all, {}, {}) }

    @Test fun tarefasInteira() {
        paparazzi.tall(3600)
        paparazzi.shotInShell("inteira", 0) { TasksScreen(all, {}, {}) }
    }

    @Test fun tarefasVazia() = paparazzi.shotInShell("vazia", 0, shell = Samples.shell.copy(badge = 0)) {
        TasksScreen(TasksState(false, null, emptyList()), {}, {})
    }

    @Test fun tarefasCarregando() = paparazzi.shotInShell(
        "carregando", 0,
        shell = Samples.shell.copy(badge = 0, syncing = true, statusLine = "Sincronizando…", statusKind = StatusKind.NEUTRAL)
    ) { TasksScreen(TasksState(true, null, emptyList()), {}, {}) }

    @Test fun tarefasErro() = paparazzi.shotInShell(
        "erro", 0, shell = Samples.shell.copy(statusLine = "Frase-senha incorreta.", statusKind = StatusKind.ERROR)
    ) { TasksScreen(TasksState(false, "Não foi possível ler as tarefas.", Samples.tasks.take(3)), {}, {}) }

    @Test fun documentos() = paparazzi.shotInShell("claro", 1) {
        DocumentsScreen(Samples.docs, "", {}, DocFilter.ALL, {}, {}, {})
    }

    @Test fun documentosEscuro() = paparazzi.shotInShell("escuro", 1, dark = true) {
        DocumentsScreen(Samples.docs, "", {}, DocFilter.ALL, {}, {}, {})
    }

    @Test fun documentosBusca() = paparazzi.shotInShell("busca", 1) {
        DocumentsScreen(Samples.docs, "veículos", {}, DocFilter.WITH_TASK, {}, {}, {})
    }

    @Test fun documentosSemResultado() = paparazzi.shotInShell("sem_resultado", 1) {
        DocumentsScreen(Samples.docs, "xyz", {}, DocFilter.EMAILS, {}, {}, {})
    }

    @Test fun documentosVazio() = paparazzi.shotInShell(
        "vazio", 1,
        shell = Samples.shell.copy(badge = 0, statusLine = "Ainda não sincronizado", statusKind = StatusKind.NEUTRAL)
    ) { DocumentsScreen(emptyList(), "", {}, DocFilter.ALL, {}, {}, {}) }
}
